use chrono::Utc;
use sqlx::{Row, SqlitePool};

use crate::error::{AppError, AppResult};
use crate::models::{
    AppSettings, ProxyMode, ProxySettings, UpdateAppSettingsInput, UpdateProxySettingsInput,
};

fn default_user_agent() -> String {
    format!("{}/{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
}

fn resolve_user_agent(stored: String) -> String {
    let legacy_default = format!("{}/0.1", env!("CARGO_PKG_NAME"));
    if stored == legacy_default {
        default_user_agent()
    } else {
        stored
    }
}

/// Fetch the current proxy settings. Existing installs (and the seeded row)
/// default to system proxy.
pub async fn get_proxy_settings(pool: &SqlitePool) -> AppResult<ProxySettings> {
    let row = sqlx::query("SELECT proxy_mode, proxy_url FROM app_settings WHERE id = 1")
        .fetch_optional(pool)
        .await?;

    let Some(row) = row else {
        // No row yet: fall back to the documented default.
        return Ok(ProxySettings {
            mode: ProxyMode::System,
            custom_url: None,
        });
    };

    let mode_str: String = row.try_get("proxy_mode")?;
    let mode = ProxyMode::from_str(&mode_str)
        .ok_or_else(|| AppError::validation("unknown proxy mode stored in settings"))?;
    let custom_url: Option<String> = row.try_get("proxy_url")?;

    // Only surface the custom URL when the mode actually uses it.
    let custom_url = match mode {
        ProxyMode::Custom => custom_url,
        _ => None,
    };

    Ok(ProxySettings { mode, custom_url })
}

/// Validate and persist proxy settings, returning the stored view.
pub async fn update_proxy_settings(
    pool: &SqlitePool,
    input: UpdateProxySettingsInput,
) -> AppResult<ProxySettings> {
    let settings = crate::models::validate_proxy_settings(input)?;
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO app_settings (id, proxy_mode, proxy_url, updated_at) \
         VALUES (1, ?, ?, ?) \
         ON CONFLICT(id) DO UPDATE SET \
         proxy_mode = excluded.proxy_mode, \
         proxy_url = excluded.proxy_url, \
         updated_at = excluded.updated_at",
    )
    .bind(settings.mode.as_str())
    .bind(settings.custom_url.as_deref())
    .bind(&now)
    .execute(pool)
    .await?;

    Ok(settings)
}

/// Fetch the global check defaults. Since migration 0003 the columns are
/// `NOT NULL` with sensible defaults, so a present row always carries values.
/// If the row is somehow missing we fall back to the documented defaults.
pub async fn get_app_settings(pool: &SqlitePool) -> AppResult<AppSettings> {
    let row = sqlx::query(
        "SELECT default_balance_threshold, default_check_interval_minutes, \
         history_retention_days, user_agent, notification_cooldown_minutes, \
         scheduler_enabled \
         FROM app_settings WHERE id = 1",
    )
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(AppSettings {
            default_balance_threshold: 20.0,
            default_check_interval_minutes: 30,
            history_retention_days: 30,
            user_agent: default_user_agent(),
            notification_cooldown_minutes: 60,
            scheduler_enabled: true,
        });
    };

    Ok(AppSettings {
        default_balance_threshold: row.try_get("default_balance_threshold")?,
        default_check_interval_minutes: row.try_get("default_check_interval_minutes")?,
        history_retention_days: row.try_get("history_retention_days")?,
        user_agent: resolve_user_agent(row.try_get("user_agent")?),
        notification_cooldown_minutes: row.try_get("notification_cooldown_minutes")?,
        scheduler_enabled: row.try_get::<i64, _>("scheduler_enabled")? != 0,
    })
}

/// Validate and persist the global check defaults, returning the stored view.
/// Preserves the proxy columns on the shared settings row.
pub async fn update_app_settings(
    pool: &SqlitePool,
    input: UpdateAppSettingsInput,
) -> AppResult<AppSettings> {
    let settings = crate::models::validate_app_settings(input)?;
    let now = Utc::now().to_rfc3339();

    // Seed the row if it is missing (defaults for proxy columns), then apply the
    // validated globals. The proxy columns are only defaulted on first insert;
    // an existing row keeps its proxy configuration.
    sqlx::query(
        "INSERT INTO app_settings \
         (id, proxy_mode, proxy_url, default_balance_threshold, \
          default_check_interval_minutes, history_retention_days, user_agent, \
          notification_cooldown_minutes, updated_at) \
         VALUES (1, 'system', NULL, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(id) DO UPDATE SET \
         default_balance_threshold = excluded.default_balance_threshold, \
         default_check_interval_minutes = excluded.default_check_interval_minutes, \
         history_retention_days = excluded.history_retention_days, \
         user_agent = excluded.user_agent, \
         notification_cooldown_minutes = excluded.notification_cooldown_minutes, \
         updated_at = excluded.updated_at",
    )
    .bind(settings.default_balance_threshold)
    .bind(settings.default_check_interval_minutes)
    .bind(settings.history_retention_days)
    .bind(&settings.user_agent)
    .bind(settings.notification_cooldown_minutes)
    .bind(&now)
    .execute(pool)
    .await?;

    // The UPDATE branch never touches `scheduler_enabled`, so an existing master
    // switch survives a globals save. Re-read so the returned view reflects the
    // real stored switch rather than the `true` placeholder from validation.
    get_app_settings(pool).await
}

/// Toggle the global automatic-scheduling switch, returning the updated
/// settings. This is the only writer of `scheduler_enabled`; the globals save
/// (`update_app_settings`) intentionally leaves it untouched. Seeds the row with
/// documented defaults if it is somehow missing.
pub async fn set_scheduler_enabled(pool: &SqlitePool, enabled: bool) -> AppResult<AppSettings> {
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO app_settings (id, proxy_mode, proxy_url, scheduler_enabled, updated_at) \
         VALUES (1, 'system', NULL, ?, ?) \
         ON CONFLICT(id) DO UPDATE SET \
         scheduler_enabled = excluded.scheduler_enabled, \
         updated_at = excluded.updated_at",
    )
    .bind(if enabled { 1_i64 } else { 0 })
    .bind(&now)
    .execute(pool)
    .await?;

    get_app_settings(pool).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_pool;

    #[tokio::test]
    async fn defaults_to_system_proxy() {
        let pool = init_memory_pool().await;
        let settings = get_proxy_settings(&pool).await.unwrap();
        assert_eq!(settings.mode, ProxyMode::System);
        assert_eq!(settings.custom_url, None);
    }

    #[tokio::test]
    async fn update_and_read_custom_round_trips() {
        let pool = init_memory_pool().await;
        let stored = update_proxy_settings(
            &pool,
            UpdateProxySettingsInput {
                mode: ProxyMode::Custom,
                custom_url: Some("socks5://127.0.0.1:1080".into()),
            },
        )
        .await
        .unwrap();
        assert_eq!(stored.mode, ProxyMode::Custom);
        assert_eq!(
            stored.custom_url.as_deref(),
            Some("socks5://127.0.0.1:1080")
        );

        let read = get_proxy_settings(&pool).await.unwrap();
        assert_eq!(read, stored);
    }

    #[tokio::test]
    async fn switching_away_from_custom_hides_url() {
        let pool = init_memory_pool().await;
        update_proxy_settings(
            &pool,
            UpdateProxySettingsInput {
                mode: ProxyMode::Custom,
                custom_url: Some("http://proxy.example.com:8080".into()),
            },
        )
        .await
        .unwrap();

        let none = update_proxy_settings(
            &pool,
            UpdateProxySettingsInput {
                mode: ProxyMode::None,
                custom_url: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(none.mode, ProxyMode::None);
        assert_eq!(none.custom_url, None);

        let read = get_proxy_settings(&pool).await.unwrap();
        assert_eq!(read.custom_url, None);
    }

    #[tokio::test]
    async fn update_rejects_invalid_custom_url() {
        let pool = init_memory_pool().await;
        let err = update_proxy_settings(
            &pool,
            UpdateProxySettingsInput {
                mode: ProxyMode::Custom,
                custom_url: Some("ftp://nope".into()),
            },
        )
        .await
        .expect_err("invalid scheme should be rejected");
        assert!(matches!(err, AppError::Validation(_)));
    }

    #[tokio::test]
    async fn app_settings_default_to_seeded_values() {
        let pool = init_memory_pool().await;
        let settings = get_app_settings(&pool).await.unwrap();
        assert_eq!(settings.default_balance_threshold, 20.0);
        assert_eq!(settings.default_check_interval_minutes, 30);
        assert_eq!(settings.history_retention_days, 30);
        assert_eq!(settings.user_agent, default_user_agent());
    }

    #[tokio::test]
    async fn app_settings_update_round_trips() {
        let pool = init_memory_pool().await;
        let stored = update_app_settings(
            &pool,
            UpdateAppSettingsInput {
                default_balance_threshold: 5.5,
                default_check_interval_minutes: 15,
                history_retention_days: 7,
                user_agent: "custom-agent/9.9".into(),
                notification_cooldown_minutes: 60,
            },
        )
        .await
        .unwrap();
        assert_eq!(stored.default_balance_threshold, 5.5);

        let read = get_app_settings(&pool).await.unwrap();
        assert_eq!(read, stored);
    }

    #[tokio::test]
    async fn scheduler_enabled_defaults_to_true() {
        let pool = init_memory_pool().await;
        let settings = get_app_settings(&pool).await.unwrap();
        assert!(settings.scheduler_enabled);
    }

    #[tokio::test]
    async fn set_scheduler_enabled_round_trips() {
        let pool = init_memory_pool().await;

        let off = set_scheduler_enabled(&pool, false).await.unwrap();
        assert!(!off.scheduler_enabled);
        assert!(!get_app_settings(&pool).await.unwrap().scheduler_enabled);

        let on = set_scheduler_enabled(&pool, true).await.unwrap();
        assert!(on.scheduler_enabled);
        assert!(get_app_settings(&pool).await.unwrap().scheduler_enabled);
    }

    #[tokio::test]
    async fn updating_app_settings_preserves_scheduler_switch() {
        let pool = init_memory_pool().await;
        // Pause the scheduler, then save the Settings-page globals.
        set_scheduler_enabled(&pool, false).await.unwrap();

        let stored = update_app_settings(
            &pool,
            UpdateAppSettingsInput {
                default_balance_threshold: 12.0,
                default_check_interval_minutes: 20,
                history_retention_days: 45,
                user_agent: "agent".into(),
                notification_cooldown_minutes: 90,
            },
        )
        .await
        .unwrap();

        // The globals save must not turn the master switch back on. Both the
        // returned view and a fresh read must still report it paused.
        assert!(!stored.scheduler_enabled);
        assert!(!get_app_settings(&pool).await.unwrap().scheduler_enabled);
    }

    #[tokio::test]
    async fn updating_app_settings_preserves_proxy_configuration() {
        let pool = init_memory_pool().await;
        update_proxy_settings(
            &pool,
            UpdateProxySettingsInput {
                mode: ProxyMode::Custom,
                custom_url: Some("socks5://127.0.0.1:1080".into()),
            },
        )
        .await
        .unwrap();

        update_app_settings(
            &pool,
            UpdateAppSettingsInput {
                default_balance_threshold: 42.0,
                default_check_interval_minutes: 10,
                history_retention_days: 14,
                user_agent: "agent".into(),
                notification_cooldown_minutes: 60,
            },
        )
        .await
        .unwrap();

        // Proxy row must survive the globals update.
        let proxy = get_proxy_settings(&pool).await.unwrap();
        assert_eq!(proxy.mode, ProxyMode::Custom);
        assert_eq!(proxy.custom_url.as_deref(), Some("socks5://127.0.0.1:1080"));
    }
}
