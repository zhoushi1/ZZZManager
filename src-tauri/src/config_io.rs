//! Configuration export / import.
//!
//! Configuration Export is a JSON backup/migration document (see the domain
//! glossary). It captures the app's *configuration* — global defaults, proxy
//! settings, gateway accounts, and notification hooks — and never any activity
//! history (balance checks or notification deliveries).
//!
//! Credentials are excluded by default. They are only written to an export when
//! the App User explicitly asks for them, and only read from an import when the
//! importer explicitly opts in. The credential fingerprint is never exported;
//! it is derived from the credentials on import instead.

use chrono::Utc;
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::accounts::{credential_fingerprint, normalize_base_url, validate_usd_credits_per_cny};
use crate::error::{AppError, AppResult};
use crate::models::{
    validate_custom_http_adapter, validate_hook, validate_official_url, validate_proxy_settings,
    Credentials, ExportedAccount, ExportedConfig, ExportedCredentials, ExportedHook,
    ExportedSettings, ImportOptions, ImportReport, Provider, ProxyMode, UpdateAppSettingsInput,
    UpdateProxySettingsInput, EXPORT_FORMAT_VERSION,
};
use crate::settings;

/// Build a full [`ExportedConfig`] from current DB state.
///
/// When `include_credentials` is false (the default caller behavior) account
/// credential values are omitted entirely; the account metadata is still
/// exported. The credential fingerprint is never included in either mode.
pub async fn export_config(
    pool: &SqlitePool,
    include_credentials: bool,
) -> AppResult<ExportedConfig> {
    let app_settings = settings::get_app_settings(pool).await?;
    let proxy = settings::get_proxy_settings(pool).await?;

    let settings = ExportedSettings {
        default_balance_threshold: app_settings.default_balance_threshold,
        default_check_interval_minutes: app_settings.default_check_interval_minutes,
        history_retention_days: app_settings.history_retention_days,
        user_agent: app_settings.user_agent,
        notification_cooldown_minutes: Some(app_settings.notification_cooldown_minutes),
        scheduler_enabled: app_settings.scheduler_enabled,
        proxy_mode: proxy.mode,
        proxy_url: proxy.custom_url,
    };

    let accounts = export_accounts(pool, include_credentials).await?;
    let hooks = export_hooks(pool).await?;

    Ok(ExportedConfig {
        format_version: EXPORT_FORMAT_VERSION,
        exported_at: Utc::now().to_rfc3339(),
        includes_credentials: include_credentials,
        settings,
        accounts,
        hooks,
    })
}

/// Default filename for a saved Configuration Export, derived from the
/// document's RFC3339 `exported_at` timestamp: `zzz-manager-config-YYYY-MM-DD.json`.
///
/// The date is the leading `YYYY-MM-DD` of the timestamp. If the string is too
/// short to contain a date it is used verbatim, so the helper never panics.
pub fn export_file_name(exported_at: &str) -> String {
    let date = exported_at.get(0..10).unwrap_or(exported_at);
    format!("zzz-manager-config-{date}.json")
}

/// Read all accounts for export. Credentials are only populated when
/// `include_credentials` is true. `credential_fingerprint` is intentionally not
/// selected so it can never leak into the document.
async fn export_accounts(
    pool: &SqlitePool,
    include_credentials: bool,
) -> AppResult<Vec<ExportedAccount>> {
    let rows = sqlx::query(
        "SELECT name, provider, base_url, enabled, balance_threshold, usd_credits_per_cny, check_interval_minutes, \
         official_url, note, sort_order, access_token, user_id, api_key, custom_adapter_config \
         FROM gateway_accounts ORDER BY sort_order DESC, created_at ASC",
    )
    .fetch_all(pool)
    .await?;

    let mut accounts = Vec::with_capacity(rows.len());
    for row in rows {
        let provider_str: String = row.try_get("provider")?;
        let provider = Provider::from_str(&provider_str)
            .ok_or_else(|| AppError::validation("unknown provider stored for account"))?;

        let credentials = if include_credentials {
            match provider {
                Provider::NewApi => Some(ExportedCredentials {
                    access_token: row.try_get("access_token")?,
                    user_id: row.try_get("user_id")?,
                    api_key: None,
                }),
                Provider::Sub2Api => Some(ExportedCredentials {
                    access_token: None,
                    user_id: None,
                    api_key: row.try_get("api_key")?,
                }),
                Provider::CustomHttp => Some(ExportedCredentials {
                    access_token: None,
                    user_id: None,
                    api_key: row.try_get("api_key")?,
                }),
            }
        } else {
            None
        };

        let custom_adapter = match row.try_get::<Option<String>, _>("custom_adapter_config")? {
            Some(raw) => Some(serde_json::from_str(&raw).map_err(|e| {
                AppError::validation(format!("Invalid custom adapter export: {e}"))
            })?),
            None => None,
        };

        accounts.push(ExportedAccount {
            name: row.try_get("name")?,
            provider,
            base_url: row.try_get("base_url")?,
            enabled: row.try_get("enabled")?,
            balance_threshold: row.try_get("balance_threshold")?,
            usd_credits_per_cny: row.try_get("usd_credits_per_cny")?,
            check_interval_minutes: row.try_get("check_interval_minutes")?,
            official_url: row.try_get("official_url")?,
            note: row.try_get("note")?,
            sort_order: row.try_get("sort_order")?,
            credentials,
            custom_adapter,
        });
    }
    Ok(accounts)
}

/// Read all notification hooks for export.
async fn export_hooks(pool: &SqlitePool) -> AppResult<Vec<ExportedHook>> {
    let rows = sqlx::query(
        "SELECT name, url, hook_type, enabled FROM notification_hooks ORDER BY created_at ASC",
    )
    .fetch_all(pool)
    .await?;

    let mut hooks = Vec::with_capacity(rows.len());
    for row in rows {
        let hook_type_str: String = row.try_get("hook_type")?;
        let hook_type = crate::models::NotificationHookType::from_str(&hook_type_str)
            .unwrap_or(crate::models::NotificationHookType::Generic);
        hooks.push(ExportedHook {
            name: row.try_get("name")?,
            url: row.try_get("url")?,
            hook_type,
            enabled: row.try_get("enabled")?,
        });
    }
    Ok(hooks)
}

/// Apply an [`ExportedConfig`] to the database, returning a readable
/// [`ImportReport`].
///
/// Import always upserts settings and validates every record before writing.
/// Account handling depends on whether usable credentials are present:
///
/// * Accounts with credentials (and `import_credentials = true`) are created or
///   updated as full accounts. Identity is provider + normalized base URL +
///   credential fingerprint, matching the credentialed unique index.
/// * Accounts without usable credentials (either the export omitted them or
///   `import_credentials = false`) are imported as **disabled credentialless
///   placeholders** so their metadata survives a backup/migration round-trip.
///   Their identity is provider + normalized base URL + name (see migration
///   `0005`). Placeholders are always created disabled regardless of the exported
///   `enabled` flag, so the scheduler never tries to check an account with no
///   credentials. The App User can later add credentials via the edit form.
///
/// `overwrite_existing` chooses the duplicate behavior for both kinds: when false
/// (the safer default) an incoming account whose identity already exists is
/// skipped and counted, never erroring; when true it updates the matching row.
pub async fn import_config(
    pool: &SqlitePool,
    input: ExportedConfig,
    options: ImportOptions,
) -> AppResult<ImportReport> {
    if input.format_version != EXPORT_FORMAT_VERSION {
        return Err(AppError::validation(format!(
            "Unsupported export format version {}; expected {}",
            input.format_version, EXPORT_FORMAT_VERSION
        )));
    }

    let mut report = ImportReport::default();

    import_settings(pool, &input.settings, &mut report).await?;

    for account in input.accounts {
        import_account(pool, account, &options, &mut report).await?;
    }

    for hook in input.hooks {
        import_hook(pool, hook, &options, &mut report).await?;
    }

    Ok(report)
}

/// Validate and apply the settings/proxy portion of an import. Reuses the same
/// validators as the live settings commands so an import cannot store values the
/// UI would reject.
async fn import_settings(
    pool: &SqlitePool,
    settings: &ExportedSettings,
    report: &mut ImportReport,
) -> AppResult<()> {
    // Validate globals through the shared validator.
    settings::update_app_settings(
        pool,
        UpdateAppSettingsInput {
            default_balance_threshold: settings.default_balance_threshold,
            default_check_interval_minutes: settings.default_check_interval_minutes,
            history_retention_days: settings.history_retention_days,
            user_agent: settings.user_agent.clone(),
            notification_cooldown_minutes: settings.notification_cooldown_minutes.unwrap_or(60),
        },
    )
    .await
    .map_err(|e| AppError::validation(format!("Invalid settings in import: {e}")))?;

    // The master scheduler switch lives on the settings row too, but is not part
    // of the globals validator, so apply it explicitly. Older exports without the
    // field default to true (see `ExportedSettings::scheduler_enabled`).
    settings::set_scheduler_enabled(pool, settings.scheduler_enabled).await?;

    // Validate proxy through the shared validator. `custom_url` is only relevant
    // for custom mode; the validator drops it otherwise.
    let proxy_input = UpdateProxySettingsInput {
        mode: settings.proxy_mode,
        custom_url: match settings.proxy_mode {
            ProxyMode::Custom => settings.proxy_url.clone(),
            _ => None,
        },
    };
    // Validate up-front for a clear error, then persist.
    validate_proxy_settings(proxy_input.clone())
        .map_err(|e| AppError::validation(format!("Invalid proxy settings in import: {e}")))?;
    settings::update_proxy_settings(pool, proxy_input).await?;

    report.settings_updated = 1;
    Ok(())
}

/// Import a single account. See [`import_config`] for the credential/identity
/// and overwrite rules.
async fn import_account(
    pool: &SqlitePool,
    account: ExportedAccount,
    options: &ImportOptions,
    report: &mut ImportReport,
) -> AppResult<()> {
    let name = account.name.trim().to_string();
    if name.is_empty() {
        report
            .warnings
            .push("Skipped an account with an empty name.".to_string());
        report.accounts_skipped += 1;
        return Ok(());
    }
    let base_url = normalize_base_url(&account.base_url);
    if base_url.is_empty() {
        report
            .warnings
            .push(format!("Skipped account '{name}': missing base URL."));
        report.accounts_skipped += 1;
        return Ok(());
    }
    let custom_adapter = match account.provider {
        Provider::CustomHttp => {
            let config = account.custom_adapter.clone().ok_or_else(|| {
                AppError::validation(format!(
                    "Custom HTTP account '{name}' is missing adapter config"
                ))
            })?;
            Some(validate_custom_http_adapter(config).map_err(|e| {
                AppError::validation(format!("Invalid custom adapter for account '{name}': {e}"))
            })?)
        }
        Provider::NewApi | Provider::Sub2Api => None,
    };
    let custom_adapter_json = custom_adapter
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| {
            AppError::validation(format!("Invalid custom adapter for account '{name}': {e}"))
        })?;

    let official_url = validate_official_url(account.official_url.as_deref()).map_err(|e| {
        AppError::validation(format!(
            "Invalid official website for account '{name}': {e}"
        ))
    })?;
    let usd_credits_per_cny =
        validate_usd_credits_per_cny(account.usd_credits_per_cny).map_err(|e| {
            AppError::validation(format!(
                "Invalid balance conversion for account '{name}': {e}"
            ))
        })?;

    let note = account
        .note
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let sort_order = account.sort_order;

    // Resolve credentials only when the importer opted in AND the document
    // actually carries them for this account.
    let credentials = if options.import_credentials {
        match resolve_import_credentials(account.provider, account.credentials.as_ref()) {
            Ok(creds) => creds,
            Err(message) => {
                report
                    .warnings
                    .push(format!("Skipped account '{name}': {message}"));
                report.accounts_skipped += 1;
                return Ok(());
            }
        }
    } else {
        None
    };

    // Without usable credentials we import a disabled placeholder that preserves
    // the account metadata. See [`import_config`] for the full rule.
    let Some(credentials) = credentials else {
        return import_placeholder_account(
            pool,
            &account,
            &name,
            &base_url,
            official_url.as_deref(),
            options,
            report,
        )
        .await;
    };

    let fingerprint = credential_fingerprint(&credentials);
    let (access_token, user_id, api_key) = split_credentials(&credentials);
    let now = Utc::now().to_rfc3339();

    // Match the unique identity (provider, base_url, fingerprint).
    let existing_id: Option<String> = sqlx::query(
        "SELECT id FROM gateway_accounts \
         WHERE provider = ? AND base_url = ? AND credential_fingerprint = ?",
    )
    .bind(account.provider.as_str())
    .bind(&base_url)
    .bind(&fingerprint)
    .fetch_optional(pool)
    .await?
    .map(|row| row.get::<String, _>("id"));

    if let Some(id) = existing_id {
        if !options.overwrite_existing {
            report.accounts_skipped += 1;
            report
                .warnings
                .push(format!("Skipped account '{name}': already exists."));
            return Ok(());
        }
        sqlx::query(
            "UPDATE gateway_accounts SET name = ?, enabled = ?, balance_threshold = ?, usd_credits_per_cny = ?, \
             check_interval_minutes = ?, official_url = ?, note = ?, sort_order = ?, access_token = ?, user_id = ?, api_key = ?, \
             custom_adapter_config = ?, updated_at = ? WHERE id = ?",
        )
        .bind(&name)
        .bind(account.enabled)
        .bind(account.balance_threshold)
        .bind(usd_credits_per_cny)
        .bind(account.check_interval_minutes)
        .bind(&official_url)
        .bind(&note)
        .bind(sort_order)
        .bind(&access_token)
        .bind(&user_id)
        .bind(&api_key)
        .bind(&custom_adapter_json)
        .bind(&now)
        .bind(&id)
        .execute(pool)
        .await?;
        report.accounts_updated += 1;
        return Ok(());
    }

    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO gateway_accounts \
         (id, name, provider, base_url, enabled, balance_threshold, usd_credits_per_cny, check_interval_minutes, \
          official_url, note, sort_order, access_token, user_id, api_key, custom_adapter_config, credential_fingerprint, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&name)
    .bind(account.provider.as_str())
    .bind(&base_url)
    .bind(account.enabled)
    .bind(account.balance_threshold)
    .bind(usd_credits_per_cny)
    .bind(account.check_interval_minutes)
    .bind(&official_url)
    .bind(&note)
    .bind(sort_order)
    .bind(&access_token)
    .bind(&user_id)
    .bind(&api_key)
    .bind(&custom_adapter_json)
    .bind(&fingerprint)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;
    report.accounts_created += 1;
    Ok(())
}

/// Import (or update) a credentialless placeholder account.
///
/// Placeholders are always stored **disabled** with a NULL credential
/// fingerprint, regardless of the exported `enabled` flag: an account with no
/// credentials must never be picked up by the scheduler. Identity is
/// (provider, normalized base_url, name), matching the placeholder unique index
/// from migration `0005`. When a matching placeholder already exists, the
/// `overwrite_existing` flag decides between skip-with-warning and update.
async fn import_placeholder_account(
    pool: &SqlitePool,
    account: &ExportedAccount,
    name: &str,
    base_url: &str,
    official_url: Option<&str>,
    options: &ImportOptions,
    report: &mut ImportReport,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    let note = account
        .note
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let sort_order = account.sort_order;

    // Placeholder identity is provider + base_url + name (no fingerprint).
    let existing_id: Option<String> = sqlx::query(
        "SELECT id FROM gateway_accounts \
         WHERE provider = ? AND base_url = ? AND name = ? AND credential_fingerprint IS NULL",
    )
    .bind(account.provider.as_str())
    .bind(base_url)
    .bind(name)
    .fetch_optional(pool)
    .await?
    .map(|row| row.get::<String, _>("id"));

    if let Some(id) = existing_id {
        if !options.overwrite_existing {
            report.accounts_skipped += 1;
            report.warnings.push(format!(
                "Skipped account '{name}': a placeholder without credentials already exists."
            ));
            return Ok(());
        }
        // Update metadata only; the account stays a disabled placeholder and any
        // credentials the user has since added are left untouched by not
        // overwriting the credential columns here.
        sqlx::query(
            "UPDATE gateway_accounts SET balance_threshold = ?, usd_credits_per_cny = ?, check_interval_minutes = ?, \
             official_url = ?, note = ?, sort_order = ?, custom_adapter_config = ?, updated_at = ? WHERE id = ?",
        )
        .bind(account.balance_threshold)
        .bind(account.usd_credits_per_cny)
        .bind(account.check_interval_minutes)
        .bind(official_url)
        .bind(&note)
        .bind(sort_order)
        .bind(placeholder_adapter_json(account)?)
        .bind(&now)
        .bind(&id)
        .execute(pool)
        .await?;
        report.accounts_updated += 1;
        report.warnings.push(format!(
            "Imported account '{name}' as a disabled placeholder (no credentials). \
             Add credentials to enable balance checks."
        ));
        return Ok(());
    }

    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO gateway_accounts \
         (id, name, provider, base_url, enabled, balance_threshold, usd_credits_per_cny, check_interval_minutes, \
          official_url, note, sort_order, access_token, user_id, api_key, custom_adapter_config, credential_fingerprint, created_at, updated_at) \
         VALUES (?, ?, ?, ?, 0, ?, ?, ?, ?, ?, ?, NULL, NULL, NULL, ?, NULL, ?, ?)",
    )
    .bind(&id)
    .bind(name)
    .bind(account.provider.as_str())
    .bind(base_url)
    .bind(account.balance_threshold)
    .bind(account.usd_credits_per_cny)
    .bind(account.check_interval_minutes)
    .bind(official_url)
    .bind(&note)
    .bind(sort_order)
    .bind(placeholder_adapter_json(account)?)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;
    report.accounts_created += 1;
    report.warnings.push(format!(
        "Imported account '{name}' as a disabled placeholder (no credentials). \
         Add credentials to enable balance checks."
    ));
    Ok(())
}

/// Resolve exported credentials for a provider into validated [`Credentials`],
/// or `Ok(None)` when the export carried none. Returns a human-readable message
/// (not an [`AppError`]) on partial/invalid credentials so the caller can add it
/// to the import report and keep going.
fn resolve_import_credentials(
    provider: Provider,
    credentials: Option<&ExportedCredentials>,
) -> Result<Option<Credentials>, String> {
    let Some(creds) = credentials else {
        return Ok(None);
    };
    match provider {
        Provider::NewApi => {
            let access_token = non_empty(&creds.access_token);
            let user_id = non_empty(&creds.user_id);
            match (access_token, user_id) {
                (Some(access_token), Some(user_id)) => Ok(Some(Credentials::NewApi {
                    access_token,
                    user_id,
                })),
                (None, None) => Ok(None),
                _ => Err(
                    "New API credentials are incomplete (need access token and user ID)"
                        .to_string(),
                ),
            }
        }
        Provider::Sub2Api => match non_empty(&creds.api_key) {
            Some(api_key) => Ok(Some(Credentials::Sub2Api { api_key })),
            None => Ok(None),
        },
        Provider::CustomHttp => match non_empty(&creds.api_key) {
            Some(api_key) => Ok(Some(Credentials::CustomHttp { api_key })),
            None => Ok(None),
        },
    }
}

fn placeholder_adapter_json(account: &ExportedAccount) -> AppResult<Option<String>> {
    if account.provider != Provider::CustomHttp {
        return Ok(None);
    }
    let Some(config) = account.custom_adapter.clone() else {
        return Ok(None);
    };
    let config = validate_custom_http_adapter(config)?;
    serde_json::to_string(&config)
        .map(Some)
        .map_err(|e| AppError::validation(format!("Invalid custom adapter config: {e}")))
}

fn non_empty(value: &Option<String>) -> Option<String> {
    value
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn split_credentials(
    credentials: &Credentials,
) -> (Option<String>, Option<String>, Option<String>) {
    match credentials {
        Credentials::NewApi {
            access_token,
            user_id,
        } => (Some(access_token.clone()), Some(user_id.clone()), None),
        Credentials::Sub2Api { api_key } | Credentials::CustomHttp { api_key } => {
            (None, None, Some(api_key.clone()))
        }
    }
}

/// Import a single notification hook. Identity is the (validated) URL: a hook
/// with a matching URL is updated when `overwrite_existing` is true, otherwise
/// skipped; a new URL is inserted. Missing hook_type defaults to generic for
/// backward compatibility with older exports.
async fn import_hook(
    pool: &SqlitePool,
    hook: ExportedHook,
    options: &ImportOptions,
    report: &mut ImportReport,
) -> AppResult<()> {
    let (name, url) = match validate_hook(&hook.name, &hook.url) {
        Ok(pair) => pair,
        Err(e) => {
            report
                .warnings
                .push(format!("Skipped hook '{}': {e}", hook.name.trim()));
            return Ok(());
        }
    };
    let now = Utc::now().to_rfc3339();

    let existing_id: Option<String> =
        sqlx::query("SELECT id FROM notification_hooks WHERE url = ?")
            .bind(&url)
            .fetch_optional(pool)
            .await?
            .map(|row| row.get::<String, _>("id"));

    if let Some(id) = existing_id {
        if !options.overwrite_existing {
            report
                .warnings
                .push(format!("Skipped hook '{name}': URL already exists."));
            return Ok(());
        }
        sqlx::query(
            "UPDATE notification_hooks SET name = ?, url = ?, hook_type = ?, enabled = ?, updated_at = ? \
             WHERE id = ?",
        )
        .bind(&name)
        .bind(&url)
        .bind(hook.hook_type.as_str())
        .bind(hook.enabled)
        .bind(&now)
        .bind(&id)
        .execute(pool)
        .await?;
        report.hooks_updated += 1;
        return Ok(());
    }

    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO notification_hooks (id, name, url, hook_type, enabled, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&name)
    .bind(&url)
    .bind(hook.hook_type.as_str())
    .bind(hook.enabled)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;
    report.hooks_created += 1;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_pool;
    use crate::models::{CreateAccountInput, CreateHookInput, CredentialsInput};

    fn new_api_input(name: &str, base_url: &str, token: &str, uid: &str) -> CreateAccountInput {
        CreateAccountInput {
            name: name.into(),
            provider: Provider::NewApi,
            base_url: base_url.into(),
            enabled: true,
            balance_threshold: Some(20.0),
            usd_credits_per_cny: None,
            check_interval_minutes: Some(30),
            official_url: None,
            note: None,
            sort_order: None,
            credentials: CredentialsInput {
                access_token: Some(token.into()),
                user_id: Some(uid.into()),
                api_key: None,
            },
            custom_adapter: None,
        }
    }

    async fn seed_account(pool: &SqlitePool) {
        crate::accounts::create_account(
            pool,
            new_api_input("Main", "https://api.example.com", "tok", "42"),
        )
        .await
        .expect("seed account");
    }

    #[test]
    fn export_file_name_uses_date_prefix() {
        assert_eq!(
            export_file_name("2026-07-07T12:34:56+00:00"),
            "zzz-manager-config-2026-07-07.json"
        );
        // A short/degenerate timestamp is used verbatim rather than panicking.
        assert_eq!(export_file_name("2026"), "zzz-manager-config-2026.json");
    }

    #[tokio::test]
    async fn export_excludes_credentials_by_default() {
        let pool = init_memory_pool().await;
        seed_account(&pool).await;

        let config = export_config(&pool, false).await.unwrap();
        assert_eq!(config.format_version, EXPORT_FORMAT_VERSION);
        assert!(!config.includes_credentials);
        assert_eq!(config.accounts.len(), 1);
        assert!(config.accounts[0].credentials.is_none());

        // Metadata is still present.
        assert_eq!(config.accounts[0].name, "Main");
        assert_eq!(config.accounts[0].base_url, "https://api.example.com");

        // No secret and no fingerprint anywhere in the serialized document.
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("tok"));
        assert!(!json.to_lowercase().contains("fingerprint"));
    }

    #[tokio::test]
    async fn export_includes_credentials_when_requested() {
        let pool = init_memory_pool().await;
        seed_account(&pool).await;

        let config = export_config(&pool, true).await.unwrap();
        assert!(config.includes_credentials);
        let creds = config.accounts[0]
            .credentials
            .as_ref()
            .expect("credentials present");
        assert_eq!(creds.access_token.as_deref(), Some("tok"));
        assert_eq!(creds.user_id.as_deref(), Some("42"));

        // Still never the fingerprint.
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.to_lowercase().contains("fingerprint"));
    }

    #[tokio::test]
    async fn import_creates_account_with_credentials() {
        let source = init_memory_pool().await;
        seed_account(&source).await;
        let config = export_config(&source, true).await.unwrap();

        let target = init_memory_pool().await;
        let report = import_config(
            &target,
            config,
            ImportOptions {
                import_credentials: true,
                overwrite_existing: false,
            },
        )
        .await
        .unwrap();

        assert_eq!(report.accounts_created, 1);
        assert_eq!(report.accounts_updated, 0);
        assert_eq!(report.accounts_skipped, 0);
        assert_eq!(report.settings_updated, 1);

        let accounts = crate::accounts::list_accounts(&target).await.unwrap();
        assert_eq!(accounts.len(), 1);
        assert!(accounts[0].has_credentials);
    }

    #[tokio::test]
    async fn import_creates_disabled_placeholder_without_credentials() {
        let source = init_memory_pool().await;
        seed_account(&source).await;
        // Export WITHOUT credentials.
        let config = export_config(&source, false).await.unwrap();

        let target = init_memory_pool().await;
        let report = import_config(
            &target,
            config,
            ImportOptions {
                import_credentials: true,
                overwrite_existing: false,
            },
        )
        .await
        .unwrap();

        // No credentials in the document -> imported as a disabled placeholder,
        // not skipped.
        assert_eq!(report.accounts_created, 1);
        assert_eq!(report.accounts_skipped, 0);
        assert!(report.warnings.iter().any(|w| w.contains("placeholder")));

        let accounts = crate::accounts::list_accounts(&target).await.unwrap();
        assert_eq!(accounts.len(), 1);
        // Placeholder: metadata preserved, credentials absent, and disabled so the
        // scheduler never touches it.
        assert_eq!(accounts[0].name, "Main");
        assert_eq!(accounts[0].base_url, "https://api.example.com");
        assert!(!accounts[0].has_credentials);
        assert!(!accounts[0].enabled);
    }

    #[tokio::test]
    async fn import_forces_placeholder_disabled_even_when_export_enabled() {
        // The seeded account is enabled=true; a credentialless import must still
        // produce a disabled placeholder so no scheduled check runs without creds.
        let source = init_memory_pool().await;
        seed_account(&source).await;
        let config = export_config(&source, false).await.unwrap();
        assert!(config.accounts[0].enabled, "seed export should be enabled");

        let target = init_memory_pool().await;
        import_config(&target, config, ImportOptions::default())
            .await
            .unwrap();

        let accounts = crate::accounts::list_accounts(&target).await.unwrap();
        assert!(!accounts[0].enabled);
    }

    #[tokio::test]
    async fn import_creates_placeholder_when_option_disabled() {
        let source = init_memory_pool().await;
        seed_account(&source).await;
        // Export WITH credentials, but import with credential import disabled.
        let config = export_config(&source, true).await.unwrap();

        let target = init_memory_pool().await;
        let report = import_config(
            &target,
            config,
            ImportOptions {
                import_credentials: false,
                overwrite_existing: false,
            },
        )
        .await
        .unwrap();

        // Credentials present in the doc but import_credentials is off -> the
        // account is imported as a disabled placeholder without its credentials.
        assert_eq!(report.accounts_created, 1);
        assert_eq!(report.accounts_skipped, 0);
        let accounts = crate::accounts::list_accounts(&target).await.unwrap();
        assert_eq!(accounts.len(), 1);
        assert!(!accounts[0].has_credentials);
        assert!(!accounts[0].enabled);
    }

    #[tokio::test]
    async fn import_duplicate_placeholder_is_skipped_without_overwrite() {
        let source = init_memory_pool().await;
        seed_account(&source).await;
        let config = export_config(&source, false).await.unwrap();

        let target = init_memory_pool().await;
        // First import creates the placeholder.
        import_config(&target, config.clone(), ImportOptions::default())
            .await
            .unwrap();

        // Second import (same provider + base_url + name) is skipped.
        let report = import_config(&target, config, ImportOptions::default())
            .await
            .unwrap();
        assert_eq!(report.accounts_created, 0);
        assert_eq!(report.accounts_updated, 0);
        assert_eq!(report.accounts_skipped, 1);
        assert!(report.warnings.iter().any(|w| w.contains("already exists")));
        assert_eq!(
            crate::accounts::list_accounts(&target).await.unwrap().len(),
            1
        );
    }

    #[tokio::test]
    async fn import_duplicate_placeholder_updates_metadata_with_overwrite() {
        // A placeholder already exists; a second credentialless import with the
        // same identity but different metadata updates it (still disabled).
        let source = init_memory_pool().await;
        crate::accounts::create_account(&source, {
            let mut input = new_api_input("Main", "https://api.example.com", "tok", "42");
            input.balance_threshold = Some(99.0);
            input.usd_credits_per_cny = Some(12.0);
            input.check_interval_minutes = Some(120);
            input
        })
        .await
        .unwrap();
        let config = export_config(&source, false).await.unwrap();

        let target = init_memory_pool().await;
        // Seed an existing placeholder with the same identity but old metadata.
        import_config(
            &target,
            {
                let mut base = export_config(&source, false).await.unwrap();
                base.accounts[0].balance_threshold = Some(1.0);
                base.accounts[0].usd_credits_per_cny = Some(2.0);
                base
            },
            ImportOptions::default(),
        )
        .await
        .unwrap();

        let report = import_config(
            &target,
            config,
            ImportOptions {
                import_credentials: false,
                overwrite_existing: true,
            },
        )
        .await
        .unwrap();

        assert_eq!(report.accounts_updated, 1);
        assert_eq!(report.accounts_created, 0);
        let accounts = crate::accounts::list_accounts(&target).await.unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].balance_threshold, Some(99.0));
        assert_eq!(accounts[0].usd_credits_per_cny, Some(12.0));
        assert!(!accounts[0].enabled);
        assert!(!accounts[0].has_credentials);
    }

    #[tokio::test]
    async fn import_duplicate_is_skipped_without_overwrite() {
        let source = init_memory_pool().await;
        seed_account(&source).await;
        let config = export_config(&source, true).await.unwrap();

        let target = init_memory_pool().await;
        seed_account(&target).await; // same identity already present

        let report = import_config(
            &target,
            config,
            ImportOptions {
                import_credentials: true,
                overwrite_existing: false,
            },
        )
        .await
        .unwrap();

        assert_eq!(report.accounts_created, 0);
        assert_eq!(report.accounts_updated, 0);
        assert_eq!(report.accounts_skipped, 1);
        assert_eq!(
            crate::accounts::list_accounts(&target).await.unwrap().len(),
            1
        );
    }

    #[tokio::test]
    async fn import_duplicate_updates_with_overwrite() {
        let source = init_memory_pool().await;
        // Export an account whose name differs from the target's existing name
        // but shares the same identity (provider + url + credentials).
        crate::accounts::create_account(
            &source,
            new_api_input("Renamed", "https://api.example.com", "tok", "42"),
        )
        .await
        .unwrap();
        let config = export_config(&source, true).await.unwrap();

        let target = init_memory_pool().await;
        seed_account(&target).await; // name "Main", same identity

        let report = import_config(
            &target,
            config,
            ImportOptions {
                import_credentials: true,
                overwrite_existing: true,
            },
        )
        .await
        .unwrap();

        assert_eq!(report.accounts_updated, 1);
        assert_eq!(report.accounts_created, 0);
        let accounts = crate::accounts::list_accounts(&target).await.unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].name, "Renamed");
    }

    #[tokio::test]
    async fn import_hooks_create_skip_and_overwrite() {
        let source = init_memory_pool().await;
        crate::hooks::create_hook(
            &source,
            CreateHookInput {
                name: "Ops".into(),
                url: "https://hooks.example.com/a".into(),
                hook_type: crate::models::NotificationHookType::Feishu,
                enabled: true,
            },
        )
        .await
        .unwrap();
        let config = export_config(&source, false).await.unwrap();

        // Fresh target: hook is created.
        let target = init_memory_pool().await;
        let report = import_config(&target, config.clone(), ImportOptions::default())
            .await
            .unwrap();
        assert_eq!(report.hooks_created, 1);
        assert_eq!(report.hooks_updated, 0);

        // Verify hook_type was imported
        let hooks = crate::hooks::list_hooks(&target).await.unwrap();
        assert_eq!(
            hooks[0].hook_type,
            crate::models::NotificationHookType::Feishu
        );

        // Import again without overwrite: skipped (same URL).
        let report = import_config(&target, config.clone(), ImportOptions::default())
            .await
            .unwrap();
        assert_eq!(report.hooks_created, 0);
        assert_eq!(report.hooks_updated, 0);

        // Import again with overwrite: updated.
        let report = import_config(
            &target,
            config,
            ImportOptions {
                import_credentials: false,
                overwrite_existing: true,
            },
        )
        .await
        .unwrap();
        assert_eq!(report.hooks_updated, 1);
        assert_eq!(crate::hooks::list_hooks(&target).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn import_rejects_unsupported_format_version() {
        let pool = init_memory_pool().await;
        let mut config = export_config(&pool, false).await.unwrap();
        config.format_version = 999;
        let err = import_config(&pool, config, ImportOptions::default())
            .await
            .expect_err("bad version");
        assert!(matches!(err, AppError::Validation(_)));
    }

    #[tokio::test]
    async fn import_applies_settings_and_proxy() {
        let source = init_memory_pool().await;
        settings::update_app_settings(
            &source,
            UpdateAppSettingsInput {
                default_balance_threshold: 42.0,
                default_check_interval_minutes: 15,
                history_retention_days: 7,
                user_agent: "custom/1.0".into(),
                notification_cooldown_minutes: 60,
            },
        )
        .await
        .unwrap();
        settings::update_proxy_settings(
            &source,
            UpdateProxySettingsInput {
                mode: ProxyMode::Custom,
                custom_url: Some("socks5://127.0.0.1:1080".into()),
            },
        )
        .await
        .unwrap();
        let config = export_config(&source, false).await.unwrap();

        let target = init_memory_pool().await;
        import_config(&target, config, ImportOptions::default())
            .await
            .unwrap();

        let app = settings::get_app_settings(&target).await.unwrap();
        assert_eq!(app.default_balance_threshold, 42.0);
        assert_eq!(app.user_agent, "custom/1.0");
        let proxy = settings::get_proxy_settings(&target).await.unwrap();
        assert_eq!(proxy.mode, ProxyMode::Custom);
        assert_eq!(proxy.custom_url.as_deref(), Some("socks5://127.0.0.1:1080"));
    }
}
