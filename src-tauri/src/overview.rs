//! Overview aggregation for the dashboard.
//!
//! Computes a single [`Overview`] snapshot from current DB state: account
//! counts by latest result, total remaining balance grouped by unit, and a
//! short tail of recent checks and notification deliveries. Everything here is
//! derived from stored snapshots and history; no credentials are read or
//! returned.

use sqlx::{Row, SqlitePool};

use crate::accounts;
use crate::error::AppResult;
use crate::models::{Overview, RemainingByUnit};
use crate::notifications;

/// Number of recent checks/deliveries surfaced on the Overview page.
const RECENT_LIMIT: i64 = 8;

/// Build the Overview DTO from current DB state.
pub async fn build_overview(pool: &SqlitePool) -> AppResult<Overview> {
    let counts = account_counts(pool).await?;
    let remaining_by_unit = remaining_by_unit(pool).await?;
    let recent_checks = accounts::recent_history(pool, RECENT_LIMIT).await?;
    let recent_deliveries = notifications::recent_deliveries(pool, RECENT_LIMIT).await?;

    Ok(Overview {
        total_accounts: counts.total,
        enabled_accounts: counts.enabled,
        unchecked_accounts: counts.unchecked,
        low_balance_count: counts.low_balance,
        failed_count: counts.failed,
        invalid_credential_count: counts.invalid_credential,
        remaining_by_unit,
        recent_checks,
        recent_deliveries,
    })
}

/// Aggregate account counts by enabled state and latest result category.
struct AccountCounts {
    total: i64,
    enabled: i64,
    unchecked: i64,
    low_balance: i64,
    failed: i64,
    invalid_credential: i64,
}

/// Compute all account counts in one pass over `gateway_accounts`. `unchecked`
/// counts accounts whose `last_result` is NULL (never checked); the per-result
/// counts are keyed on the latest snapshot, so an account contributes to at most
/// one of low_balance/failed/invalid_credential.
async fn account_counts(pool: &SqlitePool) -> AppResult<AccountCounts> {
    let row = sqlx::query(
        "SELECT \
         COUNT(*) AS total, \
         COALESCE(SUM(enabled), 0) AS enabled, \
         COALESCE(SUM(last_result IS NULL), 0) AS unchecked, \
         COALESCE(SUM(last_result = 'low_balance'), 0) AS low_balance, \
         COALESCE(SUM(last_result = 'failed'), 0) AS failed, \
         COALESCE(SUM(last_result = 'invalid_credential'), 0) AS invalid_credential \
         FROM gateway_accounts",
    )
    .fetch_one(pool)
    .await?;

    Ok(AccountCounts {
        total: row.try_get("total")?,
        enabled: row.try_get("enabled")?,
        unchecked: row.try_get("unchecked")?,
        low_balance: row.try_get("low_balance")?,
        failed: row.try_get("failed")?,
        invalid_credential: row.try_get("invalid_credential")?,
    })
}

/// Sum the latest remaining balance grouped by unit. Only accounts with both a
/// non-null `last_remaining` and `last_unit` contribute; results are ordered by
/// descending total so the largest pool sorts first.
async fn remaining_by_unit(pool: &SqlitePool) -> AppResult<Vec<RemainingByUnit>> {
    let rows = sqlx::query(
        "SELECT last_unit AS unit, SUM(last_remaining) AS total, COUNT(*) AS account_count \
         FROM gateway_accounts \
         WHERE last_remaining IS NOT NULL AND last_unit IS NOT NULL \
         GROUP BY last_unit ORDER BY total DESC",
    )
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(RemainingByUnit {
            unit: row.try_get("unit")?,
            total: row.try_get("total")?,
            account_count: row.try_get("account_count")?,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::{create_account, record_check, record_failed_check};
    use crate::db::init_memory_pool;
    use crate::models::{CreateAccountInput, CredentialsInput, Provider};
    use crate::providers::CheckOutcome;

    async fn seed_new_api(pool: &SqlitePool, name: &str, base_url: &str, enabled: bool) -> String {
        create_account(
            pool,
            CreateAccountInput {
                name: name.into(),
                provider: Provider::NewApi,
                base_url: base_url.into(),
                enabled,
                balance_threshold: Some(20.0),
                usd_credits_per_cny: None,
                check_interval_minutes: None,
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

    fn outcome(remaining: f64) -> CheckOutcome {
        CheckOutcome {
            valid: true,
            remaining: Some(remaining),
            used: Some(1.0),
            total: Some(remaining + 1.0),
            unit: Some("USD".into()),
            plan_name: None,
            message: None,
        }
    }

    #[tokio::test]
    async fn overview_counts_by_state_and_result() {
        let pool = init_memory_pool().await;

        // Healthy enabled account (remaining above threshold).
        let a = seed_new_api(&pool, "healthy", "https://a.example.com", true).await;
        record_check(
            &pool,
            &a,
            Provider::NewApi,
            &outcome(50.0),
            Some(20.0),
            None,
        )
        .await
        .unwrap();

        // Low balance disabled account.
        let b = seed_new_api(&pool, "low", "https://b.example.com", false).await;
        record_check(&pool, &b, Provider::NewApi, &outcome(5.0), Some(20.0), None)
            .await
            .unwrap();

        // Failed account.
        let c = seed_new_api(&pool, "failed", "https://c.example.com", true).await;
        record_failed_check(&pool, &c, Provider::NewApi, "boom")
            .await
            .unwrap();

        // Unchecked account (never checked).
        seed_new_api(&pool, "unchecked", "https://d.example.com", true).await;

        let ov = build_overview(&pool).await.unwrap();
        assert_eq!(ov.total_accounts, 4);
        assert_eq!(ov.enabled_accounts, 3);
        assert_eq!(ov.unchecked_accounts, 1);
        assert_eq!(ov.low_balance_count, 1);
        assert_eq!(ov.failed_count, 1);
        assert_eq!(ov.invalid_credential_count, 0);

        // Remaining is summed only over accounts with figures (healthy 50 + low 5).
        // The failed account nulls its figures; the unchecked one never had any.
        assert_eq!(ov.remaining_by_unit.len(), 1);
        assert_eq!(ov.remaining_by_unit[0].unit, "USD");
        assert_eq!(ov.remaining_by_unit[0].total, 55.0);
        assert_eq!(ov.remaining_by_unit[0].account_count, 2);

        // Three checks were recorded across the accounts.
        assert_eq!(ov.recent_checks.len(), 3);
        assert!(ov.recent_deliveries.is_empty());
    }

    #[tokio::test]
    async fn overview_empty_db_is_all_zeroes() {
        let pool = init_memory_pool().await;
        let ov = build_overview(&pool).await.unwrap();
        assert_eq!(ov.total_accounts, 0);
        assert_eq!(ov.enabled_accounts, 0);
        assert_eq!(ov.unchecked_accounts, 0);
        assert!(ov.remaining_by_unit.is_empty());
        assert!(ov.recent_checks.is_empty());
    }
}
