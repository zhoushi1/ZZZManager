use sqlx::SqlitePool;
use tauri::State;
use tauri_plugin_dialog::DialogExt;

use crate::about;
use crate::about::{AppInfo, UpdateCheckResult};
use crate::accounts;
use crate::config_io;
use crate::error::{AppError, AppResult};
use crate::hooks;
use crate::models::{
    AccountCredentialsView, AccountView, AppSettings, CreateAccountInput, CreateHookInput,
    DeliveryView, ExportedConfig, HistoryEntry, HistoryQuery, HookView, ImportOptions,
    ImportReport, Overview, ProxySettings, ScheduleOverview, UpdateAccountInput,
    UpdateAppSettingsInput, UpdateHookInput, UpdateProxySettingsInput,
};
use crate::notifications;
use crate::overview;
use crate::schedule;
use crate::settings;

/// Shared application state holding the SQLite connection pool.
pub struct AppState {
    pub pool: SqlitePool,
}

#[tauri::command]
pub async fn list_accounts(state: State<'_, AppState>) -> AppResult<Vec<AccountView>> {
    accounts::list_accounts(&state.pool).await
}

#[tauri::command]
pub async fn create_account(
    state: State<'_, AppState>,
    input: CreateAccountInput,
) -> AppResult<AccountView> {
    accounts::create_account(&state.pool, input).await
}

#[tauri::command]
pub async fn update_account(
    state: State<'_, AppState>,
    id: String,
    input: UpdateAccountInput,
) -> AppResult<AccountView> {
    accounts::update_account(&state.pool, &id, input).await
}

#[tauri::command]
pub async fn delete_account(state: State<'_, AppState>, id: String) -> AppResult<()> {
    accounts::delete_account(&state.pool, &id).await
}

#[tauri::command]
pub async fn reorder_accounts(
    state: State<'_, AppState>,
    ordered_ids: Vec<String>,
) -> AppResult<Vec<AccountView>> {
    accounts::reorder_accounts(&state.pool, ordered_ids).await
}

#[tauri::command]
pub async fn get_account_credentials(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<AccountCredentialsView> {
    accounts::get_account_credentials(&state.pool, &id).await
}

/// Manually check a single account by id, regardless of enabled state.
/// Writes a history row and updates the account's latest-result snapshot.
/// Disabled manual checks emit no notification events (notifications are out of
/// scope for this release).
#[tauri::command]
pub async fn check_account(state: State<'_, AppState>, id: String) -> AppResult<AccountView> {
    let (provider, base_url, credentials, threshold, usd_credits_per_cny, custom_adapter) =
        accounts::load_credentials(&state.pool, &id).await?;
    let proxy = settings::get_proxy_settings(&state.pool).await?;
    let app_settings = settings::get_app_settings(&state.pool).await?;
    // Fall back to the global default threshold when the account has no override,
    // matching the effective-threshold rule used by scheduled checks.
    let effective =
        crate::scheduler::effective_threshold(threshold, app_settings.default_balance_threshold);
    match crate::providers::check_balance(
        provider,
        &base_url,
        &credentials,
        custom_adapter.as_ref(),
        &proxy,
        &app_settings.user_agent,
    )
    .await
    {
        Ok(outcome) => {
            accounts::record_check(
                &state.pool,
                &id,
                provider,
                &outcome,
                Some(effective),
                usd_credits_per_cny,
            )
            .await?;
        }
        // A request/transport failure is a recordable check result, not a command
        // failure: persist it as `failed` so the UI can show it and the schedule
        // advances, then return the updated view instead of surfacing an error.
        Err(crate::error::AppError::Request(message)) => {
            accounts::record_failed_check(&state.pool, &id, provider, &message).await?;
        }
        // Other errors (credential/provider mismatch, etc.) still surface.
        Err(e) => return Err(e),
    }
    accounts::get_account(&state.pool, &id).await
}

#[tauri::command]
pub async fn recent_checks(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> AppResult<Vec<HistoryEntry>> {
    accounts::recent_history(&state.pool, limit.unwrap_or(20)).await
}

/// Query balance check history with optional account/result/provider filters and
/// a clamped limit (default 100, max 500), newest first.
#[tauri::command]
pub async fn query_history(
    state: State<'_, AppState>,
    query: HistoryQuery,
) -> AppResult<Vec<HistoryEntry>> {
    accounts::query_history(&state.pool, &query).await
}

/// Compute the dashboard overview from current DB state. Contains only
/// aggregate counts and non-sensitive recent activity, never credentials.
#[tauri::command]
pub async fn get_overview(state: State<'_, AppState>) -> AppResult<Overview> {
    overview::build_overview(&state.pool).await
}

/// Compute the Schedules page snapshot: the global default cadence, account
/// counts by schedulability, and a per-account row describing whether and when
/// the runtime scheduler will check it. Derived from stored state only; never
/// contains credentials. Due/next-check semantics mirror the scheduler.
#[tauri::command]
pub async fn get_schedule_overview(state: State<'_, AppState>) -> AppResult<ScheduleOverview> {
    schedule::build_schedule_overview(&state.pool).await
}

#[tauri::command]
pub async fn get_proxy_settings(state: State<'_, AppState>) -> AppResult<ProxySettings> {
    settings::get_proxy_settings(&state.pool).await
}

#[tauri::command]
pub async fn update_proxy_settings(
    state: State<'_, AppState>,
    input: UpdateProxySettingsInput,
) -> AppResult<ProxySettings> {
    settings::update_proxy_settings(&state.pool, input).await
}

#[tauri::command]
pub async fn get_app_settings(state: State<'_, AppState>) -> AppResult<AppSettings> {
    settings::get_app_settings(&state.pool).await
}

#[tauri::command]
pub async fn update_app_settings(
    state: State<'_, AppState>,
    input: UpdateAppSettingsInput,
) -> AppResult<AppSettings> {
    settings::update_app_settings(&state.pool, input).await
}

/// Toggle the global automatic-scheduling switch used by the accounts page
/// header. When disabled the runtime scheduler performs no automatic balance
/// checks; manual checks and per-account participation are unaffected. Returns
/// the updated settings so the frontend can reflect the stored value.
#[tauri::command]
pub async fn set_scheduler_enabled(
    state: State<'_, AppState>,
    enabled: bool,
) -> AppResult<AppSettings> {
    settings::set_scheduler_enabled(&state.pool, enabled).await
}

#[tauri::command]
pub async fn list_hooks(state: State<'_, AppState>) -> AppResult<Vec<HookView>> {
    hooks::list_hooks(&state.pool).await
}

#[tauri::command]
pub async fn create_hook(
    state: State<'_, AppState>,
    input: CreateHookInput,
) -> AppResult<HookView> {
    hooks::create_hook(&state.pool, input).await
}

#[tauri::command]
pub async fn update_hook(
    state: State<'_, AppState>,
    id: String,
    input: UpdateHookInput,
) -> AppResult<HookView> {
    hooks::update_hook(&state.pool, &id, input).await
}

#[tauri::command]
pub async fn delete_hook(state: State<'_, AppState>, id: String) -> AppResult<()> {
    hooks::delete_hook(&state.pool, &id).await
}

/// Send a fixed test payload to a single hook and report whether the POST
/// succeeded. Uses the current proxy settings and configured User-Agent.
#[tauri::command]
pub async fn test_hook(state: State<'_, AppState>, id: String) -> AppResult<bool> {
    let proxy = settings::get_proxy_settings(&state.pool).await?;
    let app_settings = settings::get_app_settings(&state.pool).await?;
    notifications::test_hook(&state.pool, &id, &proxy, &app_settings, chrono::Utc::now()).await
}

#[tauri::command]
pub async fn recent_deliveries(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> AppResult<Vec<DeliveryView>> {
    notifications::recent_deliveries(&state.pool, limit.unwrap_or(20)).await
}

/// Produce a Configuration Export document. Credentials are excluded unless the
/// App User explicitly opts in via `include_credentials`; the credential
/// fingerprint is never included. History (balance checks, deliveries) is never
/// exported.
#[tauri::command]
pub async fn export_config(
    state: State<'_, AppState>,
    include_credentials: bool,
) -> AppResult<ExportedConfig> {
    config_io::export_config(&state.pool, include_credentials).await
}

/// Export the configuration and write it to a file chosen through the native
/// save dialog. This is the reliable desktop path — the browser download hack is
/// unreliable inside the Tauri WebView.
///
/// Returns `Ok(Some(path))` with the written path on success, or `Ok(None)` when
/// the App User dismisses the save dialog. Cancellation is a normal outcome, not
/// an error, so the frontend can distinguish it from a failure.
#[tauri::command]
pub async fn export_config_to_file(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    include_credentials: bool,
) -> AppResult<Option<String>> {
    let config = config_io::export_config(&state.pool, include_credentials).await?;
    let json = serde_json::to_string_pretty(&config)
        .map_err(|e| AppError::validation(format!("Failed to serialize export: {e}")))?;
    let default_name = config_io::export_file_name(&config.exported_at);

    // The native save dialog is blocking, so run it via the callback API and
    // bridge back to async with a channel. `None` means the user cancelled.
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_file_name(&default_name)
        .add_filter("JSON", &["json"])
        .save_file(move |path| {
            let _ = tx.send(path);
        });
    let chosen = rx
        .await
        .map_err(|e| AppError::Io(format!("save dialog closed unexpectedly: {e}")))?;

    let Some(path) = chosen else {
        return Ok(None);
    };
    let path_buf = path
        .into_path()
        .map_err(|e| AppError::Io(format!("invalid save path: {e}")))?;
    std::fs::write(&path_buf, json.as_bytes())
        .map_err(|e| AppError::Io(format!("failed to write {}: {e}", path_buf.display())))?;
    Ok(Some(path_buf.display().to_string()))
}

/// Apply a previously exported Configuration document. Accepts the parsed JSON
/// object (the frontend handles file I/O). Returns an [`ImportReport`] describing
/// what was created, updated, or skipped.
#[tauri::command]
pub async fn import_config(
    state: State<'_, AppState>,
    input: ExportedConfig,
    options: ImportOptions,
) -> AppResult<ImportReport> {
    config_io::import_config(&state.pool, input, options).await
}

/// Return the static app identity (name, running version, GitHub URL) for the
/// Settings "About" card. The version comes from build metadata, never the
/// frontend.
#[tauri::command]
pub fn get_app_info() -> AppInfo {
    about::app_info()
}

/// Check GitHub for a newer release than the running version. Honors the
/// configured proxy. Network/API failures surface as errors so the frontend can
/// show them instead of hanging.
#[tauri::command]
pub async fn check_for_update(state: State<'_, AppState>) -> AppResult<UpdateCheckResult> {
    let proxy = settings::get_proxy_settings(&state.pool).await?;
    about::check_for_update(&proxy).await
}

/// Report whether launch-on-login is currently enabled. This reflects the real
/// OS-level state queried through the autostart plugin, not any stored value:
/// autostart is a system setting the user can also change outside the app.
#[tauri::command]
pub fn get_autostart_enabled(app: tauri::AppHandle) -> AppResult<bool> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch()
        .is_enabled()
        .map_err(|e| AppError::Io(format!("failed to read autostart state: {e}")))
}

/// Enable or disable launch-on-login through the autostart plugin, then return
/// the freshly re-queried state so the frontend can reconcile its UI with what
/// the OS actually recorded rather than assuming the write succeeded blindly.
#[tauri::command]
pub fn set_autostart_enabled(app: tauri::AppHandle, enabled: bool) -> AppResult<bool> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    result.map_err(|e| {
        AppError::Io(format!(
            "failed to {} autostart: {e}",
            if enabled { "enable" } else { "disable" }
        ))
    })?;
    manager
        .is_enabled()
        .map_err(|e| AppError::Io(format!("failed to read autostart state: {e}")))
}

/// Toggle account enabled state. Used to control whether an account participates
/// in scheduled checks. Returns the updated account view.
#[tauri::command]
pub async fn set_account_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> AppResult<AccountView> {
    accounts::set_account_enabled(&state.pool, &id, enabled).await
}
