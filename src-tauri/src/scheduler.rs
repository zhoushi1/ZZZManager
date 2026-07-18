//! Runtime-only balance-check scheduler.
//!
//! Per ADR-0002 scheduled checks run only while the Tauri desktop app process
//! is alive; there is no OS-level background service. A single background task
//! ticks on an interval, checks enabled accounts that are due, records history
//! and latest snapshots exactly like a manual check, and prunes old history.
//!
//! The tick loop is intentionally serial: at most one account is checked at a
//! time, and each tick fully completes before the next is scheduled. This keeps
//! the first release simple and avoids overlapping checks for the same account.

use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use crate::accounts::{self, SchedulableAccount};
use crate::providers;
use crate::settings;

/// How often the scheduler wakes up to look for due accounts. This is the
/// resolution of the schedule, not the per-account interval.
const TICK: Duration = Duration::from_secs(60);

/// Short delay before the first tick so app startup is not blocked and any
/// never-checked accounts get picked up promptly rather than after 30 minutes.
const STARTUP_DELAY: Duration = Duration::from_secs(5);

/// The effective balance threshold for an account: its own override when set,
/// otherwise the global default.
pub fn effective_threshold(account: Option<f64>, default: f64) -> f64 {
    account.unwrap_or(default)
}

/// Values placed in balance notifications. Converted accounts notify in CNY;
/// all other accounts keep the provider's original amount and unit.
fn notification_balance_values(
    outcome: Option<&crate::providers::CheckOutcome>,
    usd_credits_per_cny: Option<f64>,
) -> (Option<f64>, Option<String>) {
    let Some(outcome) = outcome else {
        return (None, None);
    };
    if let Some(converted) = accounts::convert_usd_credits_to_cny(
        outcome.remaining,
        outcome.unit.as_deref(),
        usd_credits_per_cny,
    ) {
        return (Some(converted), Some("CNY".to_string()));
    }
    (outcome.remaining, outcome.unit.clone())
}

/// The effective check interval in minutes: the account override when set and
/// valid (>= 1), otherwise the global default.
pub fn effective_interval(account: Option<i64>, default: i64) -> i64 {
    match account {
        Some(minutes) if minutes >= 1 => minutes,
        _ => default,
    }
}

/// Whether an account is due for a scheduled check as of `now`.
///
/// An account that has never been checked (`last_checked_at` is `None`) is
/// always due. An unparseable timestamp is treated as due so a bad value never
/// wedges the schedule. Otherwise it is due once `interval_minutes` have passed
/// since the last check.
pub fn is_due(last_checked_at: Option<&str>, interval_minutes: i64, now: DateTime<Utc>) -> bool {
    let Some(last) = last_checked_at else {
        return true;
    };
    let Ok(parsed) = DateTime::parse_from_rfc3339(last) else {
        return true;
    };
    let interval = chrono::Duration::minutes(interval_minutes.max(1));
    now.signed_duration_since(parsed.with_timezone(&Utc)) >= interval
}

/// Check a single due account and record the outcome, using the effective
/// threshold (account override or global default). After recording, evaluate
/// notification events and deliver to enabled hooks (scheduled checks only;
/// manual checks never notify).
///
/// Request/transport failures are recorded as a `failed` check (history row and
/// snapshot) so `last_checked_at` advances and the account is not retried on
/// every tick. Other internal errors (DB, credential/provider mismatch) are
/// returned to the caller and logged/skipped. Webhook delivery failures never
/// propagate here — see [`crate::notifications::evaluate_and_notify`].
async fn check_one(
    pool: &SqlitePool,
    account: &SchedulableAccount,
    proxy: &crate::models::ProxySettings,
    app_settings: &crate::models::AppSettings,
    now: DateTime<Utc>,
) -> crate::error::AppResult<()> {
    let threshold = effective_threshold(
        account.balance_threshold,
        app_settings.default_balance_threshold,
    );

    let (result, outcome) = match providers::check_balance(
        account.provider,
        &account.base_url,
        &account.credentials,
        account.custom_adapter.as_ref(),
        proxy,
        &app_settings.user_agent,
    )
    .await
    {
        Ok(outcome) => {
            let result = accounts::record_check(
                pool,
                &account.id,
                account.provider,
                &outcome,
                Some(threshold),
                account.usd_credits_per_cny,
            )
            .await?;
            (result, Some(outcome))
        }
        Err(crate::error::AppError::Request(message)) => {
            let result =
                accounts::record_failed_check(pool, &account.id, account.provider, &message)
                    .await?;
            (result, None)
        }
        Err(e) => return Err(e),
    };

    let (notification_remaining, notification_unit) =
        notification_balance_values(outcome.as_ref(), account.usd_credits_per_cny);
    let ctx = crate::notifications::NotificationContext {
        account_id: &account.id,
        account_name: &account.name,
        provider: account.provider,
        remaining: notification_remaining,
        unit: notification_unit,
        // The effective threshold is already expressed in CNY for converted accounts.
        threshold: Some(threshold),
        message: outcome.as_ref().and_then(|o| o.message.clone()),
        checked_at: now.to_rfc3339(),
    };
    if let Err(e) =
        crate::notifications::evaluate_and_notify(pool, result, ctx, proxy, app_settings, now).await
    {
        eprintln!(
            "scheduler: notification eval failed for account {}: {e}",
            account.id
        );
    }

    Ok(())
}

/// Run one scheduler pass: check every enabled account that is due, then prune
/// history older than the retention window. Errors on individual accounts are
/// logged and skipped so one bad account never stalls the rest of the tick.
async fn run_tick(pool: &SqlitePool, now: DateTime<Utc>) {
    let app_settings = match settings::get_app_settings(pool).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("scheduler: failed to load app settings: {e}");
            return;
        }
    };

    // The global master switch gates all automatic balance checks. When off we
    // short-circuit before touching proxy settings, the account list, or any
    // balance/notification work — but history cleanup still runs so retention
    // keeps working while paused. Manual checks and the per-account `enabled`
    // flag are unaffected by this switch.
    if !app_settings.scheduler_enabled {
        run_cleanup(pool, &app_settings, now).await;
        return;
    }

    let proxy = match settings::get_proxy_settings(pool).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("scheduler: failed to load proxy settings: {e}");
            return;
        }
    };
    let accounts_list = match accounts::list_enabled_for_schedule(pool).await {
        Ok(list) => list,
        Err(e) => {
            eprintln!("scheduler: failed to list accounts: {e}");
            return;
        }
    };

    for account in &accounts_list {
        let interval = effective_interval(
            account.check_interval_minutes,
            app_settings.default_check_interval_minutes,
        );
        if !is_due(account.last_checked_at.as_deref(), interval, now) {
            continue;
        }
        if let Err(e) = check_one(pool, account, &proxy, &app_settings, now).await {
            eprintln!("scheduler: check failed for account {}: {e}", account.id);
        }
    }

    run_cleanup(pool, &app_settings, now).await;
}

/// Prune history older than the retention window. Runs once per tick regardless
/// of whether any account was checked (including while the master switch is off);
/// it only removes rows past the retention window.
async fn run_cleanup(
    pool: &SqlitePool,
    app_settings: &crate::models::AppSettings,
    now: DateTime<Utc>,
) {
    let cutoff = now - chrono::Duration::days(app_settings.history_retention_days.max(1));
    if let Err(e) = accounts::delete_history_older_than(pool, &cutoff.to_rfc3339()).await {
        eprintln!("scheduler: history cleanup failed: {e}");
    }
}

/// Spawn the background scheduler on the Tauri async runtime. It lives for the
/// lifetime of the app process and stops when the process exits (ADR-0002).
pub fn spawn(pool: SqlitePool) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(STARTUP_DELAY).await;
        loop {
            run_tick(&pool, Utc::now()).await;
            tokio::time::sleep(TICK).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::{create_account, delete_history_older_than, record_check};
    use crate::db::init_memory_pool;
    use crate::models::{CreateAccountInput, CredentialsInput, Provider};
    use crate::providers::CheckOutcome;

    #[test]
    fn effective_threshold_prefers_account_override() {
        assert_eq!(effective_threshold(Some(5.0), 20.0), 5.0);
        assert_eq!(effective_threshold(None, 20.0), 20.0);
        // Explicit zero override is honored, not treated as "unset".
        assert_eq!(effective_threshold(Some(0.0), 20.0), 0.0);
    }

    #[test]
    fn notification_values_use_converted_cny_balance() {
        let outcome = CheckOutcome {
            valid: true,
            remaining: Some(120.0),
            used: None,
            total: None,
            unit: Some("USD".into()),
            plan_name: None,
            message: None,
        };

        assert_eq!(
            notification_balance_values(Some(&outcome), Some(12.0)),
            (Some(10.0), Some("CNY".into()))
        );
        assert_eq!(
            notification_balance_values(Some(&outcome), None),
            (Some(120.0), Some("USD".into()))
        );
    }

    #[test]
    fn effective_interval_prefers_valid_account_override() {
        assert_eq!(effective_interval(Some(15), 30), 15);
        assert_eq!(effective_interval(None, 30), 30);
        // A stored non-positive interval falls back to the default.
        assert_eq!(effective_interval(Some(0), 30), 30);
        assert_eq!(effective_interval(Some(-5), 30), 30);
    }

    #[test]
    fn never_checked_account_is_due() {
        assert!(is_due(None, 30, Utc::now()));
    }

    #[test]
    fn unparseable_timestamp_is_due() {
        assert!(is_due(Some("not-a-date"), 30, Utc::now()));
    }

    #[test]
    fn recently_checked_account_is_not_due() {
        let now = Utc::now();
        let ten_min_ago = (now - chrono::Duration::minutes(10)).to_rfc3339();
        assert!(!is_due(Some(&ten_min_ago), 30, now));
    }

    #[test]
    fn account_past_interval_is_due() {
        let now = Utc::now();
        let forty_min_ago = (now - chrono::Duration::minutes(40)).to_rfc3339();
        assert!(is_due(Some(&forty_min_ago), 30, now));
    }

    #[tokio::test]
    async fn history_retention_removes_only_old_rows() {
        let pool = init_memory_pool().await;
        let account = create_account(
            &pool,
            CreateAccountInput {
                name: "Main".into(),
                provider: Provider::Sub2Api,
                base_url: "https://api.example.com".into(),
                enabled: true,
                balance_threshold: None,
                usd_credits_per_cny: None,
                check_interval_minutes: None,
                official_url: None,
                note: None,
                sort_order: None,
                credentials: CredentialsInput {
                    api_key: Some("key".into()),
                    ..Default::default()
                },
                custom_adapter: None,
            },
        )
        .await
        .unwrap();

        let outcome = CheckOutcome {
            valid: true,
            remaining: Some(10.0),
            used: None,
            total: None,
            unit: Some("USD".into()),
            plan_name: None,
            message: None,
        };
        // Write one history row (checked_at = now).
        record_check(&pool, &account.id, Provider::Sub2Api, &outcome, None, None)
            .await
            .unwrap();

        // Backdate an old row directly so we can prove the cutoff boundary.
        let old = (Utc::now() - chrono::Duration::days(40)).to_rfc3339();
        sqlx::query(
            "INSERT INTO balance_check_history \
             (id, account_id, provider, result, notified, checked_at) \
             VALUES (?, ?, 'sub2api', 'healthy', 0, ?)",
        )
        .bind(uuid_v4())
        .bind(&account.id)
        .bind(&old)
        .execute(&pool)
        .await
        .unwrap();

        let cutoff = (Utc::now() - chrono::Duration::days(30)).to_rfc3339();
        let removed = delete_history_older_than(&pool, &cutoff).await.unwrap();
        assert_eq!(removed, 1, "only the 40-day-old row should be deleted");

        let remaining = crate::accounts::recent_history(&pool, 10).await.unwrap();
        assert_eq!(remaining.len(), 1, "the fresh row should survive");
    }

    #[tokio::test]
    async fn recorded_failure_advances_schedule() {
        // A never-checked account is due; after recording a request failure its
        // snapshot advances so it is no longer due on the next tick (not retried
        // every 60s). This covers the scheduler's request-error branch behavior
        // via the account record path it delegates to.
        let pool = init_memory_pool().await;
        let account = create_account(
            &pool,
            CreateAccountInput {
                name: "Main".into(),
                provider: Provider::Sub2Api,
                base_url: "https://api.example.com".into(),
                enabled: true,
                balance_threshold: None,
                usd_credits_per_cny: None,
                check_interval_minutes: Some(30),
                official_url: None,
                note: None,
                sort_order: None,
                credentials: CredentialsInput {
                    api_key: Some("key".into()),
                    ..Default::default()
                },
                custom_adapter: None,
            },
        )
        .await
        .unwrap();

        // Never checked -> due.
        assert!(is_due(None, 30, Utc::now()));

        crate::accounts::record_failed_check(
            &pool,
            &account.id,
            Provider::Sub2Api,
            "request error: connection refused",
        )
        .await
        .unwrap();

        let view = crate::accounts::get_account(&pool, &account.id)
            .await
            .unwrap();
        let last = view
            .last_checked_at
            .expect("last_checked_at should advance");
        // With a fresh timestamp and a 30-minute interval, the account is no
        // longer due, so the scheduler will not re-check it on the next tick.
        assert!(!is_due(Some(&last), 30, Utc::now()));
    }

    #[tokio::test]
    async fn disabled_scheduler_skips_account_checks() {
        // A due, credentialed, enabled account is never checked while the global
        // master switch is off: run_tick must not advance last_checked_at. This
        // needs no network because the disabled guard short-circuits before any
        // balance request. History cleanup still runs (proven not to panic here).
        let pool = init_memory_pool().await;
        let account = create_account(
            &pool,
            CreateAccountInput {
                name: "Main".into(),
                provider: Provider::Sub2Api,
                base_url: "https://api.example.com".into(),
                enabled: true,
                balance_threshold: None,
                usd_credits_per_cny: None,
                check_interval_minutes: Some(30),
                official_url: None,
                note: None,
                sort_order: None,
                credentials: CredentialsInput {
                    api_key: Some("key".into()),
                    ..Default::default()
                },
                custom_adapter: None,
            },
        )
        .await
        .unwrap();

        // Never checked -> would be due if the scheduler were running.
        assert!(is_due(None, 30, Utc::now()));

        crate::settings::set_scheduler_enabled(&pool, false)
            .await
            .unwrap();

        run_tick(&pool, Utc::now()).await;

        let view = crate::accounts::get_account(&pool, &account.id)
            .await
            .unwrap();
        assert!(
            view.last_checked_at.is_none(),
            "disabled scheduler must not check any account",
        );
    }

    fn uuid_v4() -> String {
        uuid::Uuid::new_v4().to_string()
    }
}
