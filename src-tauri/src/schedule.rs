//! Read-only Schedules page aggregation.
//!
//! Builds a [`ScheduleOverview`] describing, for every account, whether and when
//! the runtime scheduler will check it. The eligibility and due semantics here
//! mirror the scheduler exactly ([`crate::scheduler`] and
//! [`crate::accounts::list_enabled_for_schedule`]): a disabled account is never
//! scheduled, an enabled account without credentials is skipped, a never-checked
//! (or bad-timestamp) schedulable account is due now, and otherwise an account is
//! due once its effective interval has elapsed since `last_checked_at`.
//!
//! This module only reads stored state and never returns credentials.

use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use crate::error::AppResult;
use crate::models::{Provider, ScheduleOverview, ScheduleRow, ScheduleStatus};
use crate::scheduler::{effective_interval, is_due};
use crate::settings;

/// Compute the next expected scheduled-check time for a schedulable account.
///
/// Returns `now` when the account is already due (the next tick would check it):
/// never checked, an unparseable timestamp, or past its interval. Otherwise
/// returns `last_checked_at + interval`. Mirrors [`is_due`] so the two never
/// disagree about whether an account is due.
pub fn next_check_at(
    last_checked_at: Option<&str>,
    interval_minutes: i64,
    now: DateTime<Utc>,
) -> DateTime<Utc> {
    let Some(last) = last_checked_at else {
        return now;
    };
    let Ok(parsed) = DateTime::parse_from_rfc3339(last) else {
        return now;
    };
    let due_at = parsed.with_timezone(&Utc) + chrono::Duration::minutes(interval_minutes.max(1));
    // If the account is already overdue, the next check is the next tick (now).
    if due_at <= now {
        now
    } else {
        due_at
    }
}

/// Minimal per-account fields needed to build a schedule row.
#[derive(sqlx::FromRow)]
struct ScheduleAccountRow {
    id: String,
    name: String,
    provider: String,
    enabled: bool,
    check_interval_minutes: Option<i64>,
    access_token: Option<String>,
    user_id: Option<String>,
    api_key: Option<String>,
    last_checked_at: Option<String>,
}

impl ScheduleAccountRow {
    fn has_credentials(&self) -> bool {
        match Provider::from_str(&self.provider) {
            Some(Provider::NewApi) => {
                self.access_token.as_deref().is_some_and(|s| !s.is_empty())
                    && self.user_id.as_deref().is_some_and(|s| !s.is_empty())
            }
            Some(Provider::Sub2Api) => self.api_key.as_deref().is_some_and(|s| !s.is_empty()),
            Some(Provider::CustomHttp) => self.api_key.as_deref().is_some_and(|s| !s.is_empty()),
            None => false,
        }
    }
}

/// Build the Schedules page DTO from current DB state and the global defaults.
pub async fn build_schedule_overview(pool: &SqlitePool) -> AppResult<ScheduleOverview> {
    let app_settings = settings::get_app_settings(pool).await?;
    let default_interval = app_settings.default_check_interval_minutes;
    let now = Utc::now();

    let rows_raw = sqlx::query_as::<_, ScheduleAccountRow>(
        "SELECT id, name, provider, enabled, check_interval_minutes, \
         access_token, user_id, api_key, last_checked_at \
         FROM gateway_accounts ORDER BY created_at ASC",
    )
    .fetch_all(pool)
    .await?;

    let mut rows = Vec::with_capacity(rows_raw.len());
    let mut enabled = 0i64;
    let mut disabled = 0i64;
    let mut schedulable = 0i64;
    let mut unschedulable = 0i64;

    for raw in rows_raw {
        let provider = Provider::from_str(&raw.provider).ok_or_else(|| {
            crate::error::AppError::validation("unknown provider stored for account")
        })?;
        let has_credentials = raw.has_credentials();
        let effective = effective_interval(raw.check_interval_minutes, default_interval);

        if raw.enabled {
            enabled += 1;
        } else {
            disabled += 1;
        }

        // Status and due/next-check derive from the same eligibility rules the
        // scheduler applies: disabled and credentialless accounts are never
        // checked, so they have no next-check time and are not "due".
        let (status, due_now, next) = if !raw.enabled {
            (ScheduleStatus::Disabled, false, None)
        } else if !has_credentials {
            unschedulable += 1;
            (ScheduleStatus::MissingCredentials, false, None)
        } else {
            schedulable += 1;
            let due = is_due(raw.last_checked_at.as_deref(), effective, now);
            let next = next_check_at(raw.last_checked_at.as_deref(), effective, now);
            let status = if due {
                ScheduleStatus::Due
            } else {
                ScheduleStatus::Scheduled
            };
            (status, due, Some(next.to_rfc3339()))
        };

        rows.push(ScheduleRow {
            id: raw.id,
            name: raw.name,
            provider,
            enabled: raw.enabled,
            has_credentials,
            check_interval_minutes: raw.check_interval_minutes,
            effective_interval_minutes: effective,
            last_checked_at: raw.last_checked_at,
            next_check_at: next,
            due_now,
            status,
        });
    }

    Ok(ScheduleOverview {
        default_check_interval_minutes: default_interval,
        scheduler_enabled: app_settings.scheduler_enabled,
        total_accounts: enabled + disabled,
        enabled_accounts: enabled,
        disabled_accounts: disabled,
        schedulable_accounts: schedulable,
        unschedulable_accounts: unschedulable,
        rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::{create_account, record_check};
    use crate::db::init_memory_pool;
    use crate::models::{CreateAccountInput, CredentialsInput, Provider};
    use crate::providers::CheckOutcome;
    use uuid::Uuid;

    #[test]
    fn next_check_is_now_when_never_checked_or_bad_timestamp() {
        let now = Utc::now();
        assert_eq!(next_check_at(None, 30, now), now);
        assert_eq!(next_check_at(Some("not-a-date"), 30, now), now);
    }

    #[test]
    fn next_check_is_now_when_overdue() {
        let now = Utc::now();
        let forty_min_ago = (now - chrono::Duration::minutes(40)).to_rfc3339();
        // Past the 30-minute interval -> due on the next tick (now).
        assert_eq!(next_check_at(Some(&forty_min_ago), 30, now), now);
    }

    #[test]
    fn next_check_is_last_plus_interval_when_not_yet_due() {
        let now = Utc::now();
        let last = now - chrono::Duration::minutes(10);
        let next = next_check_at(Some(&last.to_rfc3339()), 30, now);
        // 10 minutes elapsed of a 30-minute interval -> 20 minutes from now.
        let expected = last + chrono::Duration::minutes(30);
        assert_eq!(next, expected);
        assert!(next > now);
    }

    async fn new_api(
        pool: &SqlitePool,
        name: &str,
        base_url: &str,
        enabled: bool,
        interval: Option<i64>,
    ) -> String {
        create_account(
            pool,
            CreateAccountInput {
                name: name.into(),
                provider: Provider::NewApi,
                base_url: base_url.into(),
                enabled,
                balance_threshold: None,
                usd_credits_per_cny: None,
                check_interval_minutes: interval,
                official_url: None,
                note: None,
                sort_order: None,
                credentials: CredentialsInput {
                    access_token: Some("tok".into()),
                    user_id: Some(name.into()),
                    api_key: None,
                },
                custom_adapter: None,
            },
        )
        .await
        .unwrap()
        .id
    }

    /// Insert a credentialless placeholder directly (disabled, NULL credentials).
    async fn placeholder(pool: &SqlitePool, name: &str, base_url: &str) -> String {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO gateway_accounts \
             (id, name, provider, base_url, enabled, credential_fingerprint, created_at, updated_at) \
             VALUES (?, ?, 'new_api', ?, 0, NULL, ?, ?)",
        )
        .bind(&id)
        .bind(name)
        .bind(base_url)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await
        .unwrap();
        id
    }

    fn healthy() -> CheckOutcome {
        CheckOutcome {
            valid: true,
            remaining: Some(50.0),
            used: None,
            total: None,
            unit: Some("USD".into()),
            plan_name: None,
            message: None,
        }
    }

    fn find<'a>(ov: &'a ScheduleOverview, id: &str) -> &'a ScheduleRow {
        ov.rows.iter().find(|r| r.id == id).expect("row present")
    }

    #[tokio::test]
    async fn empty_db_reports_default_and_zero_counts() {
        let pool = init_memory_pool().await;
        let ov = build_schedule_overview(&pool).await.unwrap();
        // Seeded default interval is 30 minutes.
        assert_eq!(ov.default_check_interval_minutes, 30);
        assert_eq!(ov.total_accounts, 0);
        assert_eq!(ov.enabled_accounts, 0);
        assert_eq!(ov.disabled_accounts, 0);
        assert_eq!(ov.schedulable_accounts, 0);
        assert_eq!(ov.unschedulable_accounts, 0);
        assert!(ov.rows.is_empty());
    }

    #[tokio::test]
    async fn counts_split_by_enabled_and_credentials() {
        let pool = init_memory_pool().await;
        // Enabled + credentialed -> schedulable.
        let a = new_api(&pool, "a", "https://a.example.com", true, None).await;
        // Disabled + credentialed -> disabled, not scheduled.
        let b = new_api(&pool, "b", "https://b.example.com", false, None).await;
        // Enabled placeholder (no credentials) -> unschedulable. Force-enable to
        // prove the credential guard (not just the disabled flag) drives status.
        let c = placeholder(&pool, "c", "https://c.example.com").await;
        sqlx::query("UPDATE gateway_accounts SET enabled = 1 WHERE id = ?")
            .bind(&c)
            .execute(&pool)
            .await
            .unwrap();

        let ov = build_schedule_overview(&pool).await.unwrap();
        assert_eq!(ov.total_accounts, 3);
        assert_eq!(ov.enabled_accounts, 2);
        assert_eq!(ov.disabled_accounts, 1);
        assert_eq!(ov.schedulable_accounts, 1);
        assert_eq!(ov.unschedulable_accounts, 1);

        // Never-checked schedulable account is due now with no override.
        let ra = find(&ov, &a);
        assert_eq!(ra.status, ScheduleStatus::Due);
        assert!(ra.due_now);
        assert!(ra.next_check_at.is_some());
        assert_eq!(ra.effective_interval_minutes, 30);
        assert_eq!(ra.check_interval_minutes, None);

        // Disabled account: never scheduled, no next check, not due.
        let rb = find(&ov, &b);
        assert_eq!(rb.status, ScheduleStatus::Disabled);
        assert!(!rb.due_now);
        assert_eq!(rb.next_check_at, None);

        // Enabled placeholder: missing credentials, never scheduled.
        let rc = find(&ov, &c);
        assert_eq!(rc.status, ScheduleStatus::MissingCredentials);
        assert!(!rc.due_now);
        assert_eq!(rc.next_check_at, None);
        assert!(!rc.has_credentials);
    }

    #[tokio::test]
    async fn override_interval_beats_default_for_effective() {
        let pool = init_memory_pool().await;
        // Override of 15 minutes; a non-positive override falls back to default.
        let a = new_api(&pool, "a", "https://a.example.com", true, Some(15)).await;
        let b = new_api(&pool, "b", "https://b.example.com", true, Some(0)).await;

        let ov = build_schedule_overview(&pool).await.unwrap();
        assert_eq!(ov.default_check_interval_minutes, 30);
        assert_eq!(find(&ov, &a).effective_interval_minutes, 15);
        assert_eq!(find(&ov, &a).check_interval_minutes, Some(15));
        // Stored 0 is preserved as the raw override but the effective interval
        // falls back to the default (matches the scheduler's effective_interval).
        assert_eq!(find(&ov, &b).effective_interval_minutes, 30);
        assert_eq!(find(&ov, &b).check_interval_minutes, Some(0));
    }

    #[tokio::test]
    async fn recently_checked_account_is_scheduled_not_due() {
        let pool = init_memory_pool().await;
        let a = new_api(&pool, "a", "https://a.example.com", true, Some(30)).await;
        // Record a fresh check, so it is within its interval and not yet due.
        record_check(&pool, &a, Provider::NewApi, &healthy(), Some(20.0), None)
            .await
            .unwrap();

        let ov = build_schedule_overview(&pool).await.unwrap();
        let row = find(&ov, &a);
        assert_eq!(row.status, ScheduleStatus::Scheduled);
        assert!(!row.due_now);
        assert!(row.last_checked_at.is_some());
        // Next check is strictly in the future for a just-checked account.
        let next = DateTime::parse_from_rfc3339(row.next_check_at.as_ref().unwrap()).unwrap();
        assert!(next.with_timezone(&Utc) > Utc::now());
    }
}
