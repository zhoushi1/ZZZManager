use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// Outbound HTTP proxy mode. Applies to provider balance checks and future
/// webhook delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyMode {
    /// Use the operating system / environment proxy behavior (default).
    System,
    /// Disable proxies for outbound HTTP requests.
    None,
    /// Use a user-provided proxy URL.
    Custom,
}

impl ProxyMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ProxyMode::System => "system",
            ProxyMode::None => "none",
            ProxyMode::Custom => "custom",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "system" => Some(ProxyMode::System),
            "none" => Some(ProxyMode::None),
            "custom" => Some(ProxyMode::Custom),
            _ => None,
        }
    }
}

/// Proxy configuration returned to the frontend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxySettings {
    pub mode: ProxyMode,
    /// The custom proxy URL, present only when `mode` is `custom`.
    pub custom_url: Option<String>,
}

/// Payload for updating proxy settings from the frontend.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProxySettingsInput {
    pub mode: ProxyMode,
    #[serde(default)]
    pub custom_url: Option<String>,
}

/// Validate an update payload into resolved `ProxySettings`.
///
/// For `custom` mode the URL must parse, use an `http`, `https`, or `socks5`
/// scheme, and have a host. For `system` / `none` the URL is ignored.
pub fn validate_proxy_settings(input: UpdateProxySettingsInput) -> AppResult<ProxySettings> {
    match input.mode {
        ProxyMode::System | ProxyMode::None => Ok(ProxySettings {
            mode: input.mode,
            custom_url: None,
        }),
        ProxyMode::Custom => {
            let raw = input
                .custom_url
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| AppError::validation("Custom proxy requires a proxy URL"))?;

            let parsed = reqwest::Url::parse(raw)
                .map_err(|_| AppError::validation("Proxy URL is not a valid URL"))?;

            match parsed.scheme() {
                "http" | "https" | "socks5" => {}
                other => {
                    return Err(AppError::validation(format!(
                        "Proxy URL scheme '{other}' is not supported; use http, https, or socks5"
                    )));
                }
            }

            if parsed.host_str().map(str::is_empty).unwrap_or(true) {
                return Err(AppError::validation("Proxy URL must include a host"));
            }

            Ok(ProxySettings {
                mode: ProxyMode::Custom,
                custom_url: Some(raw.to_string()),
            })
        }
    }
}

/// Global check defaults returned to the frontend. These back the Settings page
/// globals and the runtime scheduler. Proxy settings are kept as a separate DTO
/// for backward compatibility with the existing proxy commands.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    /// Fallback balance threshold when an account has no override.
    pub default_balance_threshold: f64,
    /// Fallback schedule cadence, in minutes, when an account has no override.
    pub default_check_interval_minutes: i64,
    /// How long balance check history rows are retained, in days.
    pub history_retention_days: i64,
    /// User-Agent header sent on outbound balance checks.
    pub user_agent: String,
    /// Notification cooldown window in minutes. Repeats of the same (account, event_type)
    /// are suppressed within this window. Must be >= 1.
    pub notification_cooldown_minutes: i64,
    /// Global automatic-scheduling switch. When false, the runtime scheduler
    /// performs no automatic balance checks or notifications; manual checks and
    /// the per-account `enabled` flag are unaffected. Defaults to true.
    pub scheduler_enabled: bool,
}

/// Payload for updating the global check defaults from the frontend.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAppSettingsInput {
    pub default_balance_threshold: f64,
    pub default_check_interval_minutes: i64,
    pub history_retention_days: i64,
    pub user_agent: String,
    pub notification_cooldown_minutes: i64,
}

/// Validate an update payload into resolved `AppSettings`.
///
/// - thresholds must be finite and `>= 0`
/// - check interval must be `>= 1` minute
/// - retention must be `>= 1` day
/// - user agent must be non-empty (after trimming)
/// - notification cooldown must be `>= 1` minute
pub fn validate_app_settings(input: UpdateAppSettingsInput) -> AppResult<AppSettings> {
    if !input.default_balance_threshold.is_finite() || input.default_balance_threshold < 0.0 {
        return Err(AppError::validation(
            "Global balance threshold must be zero or greater",
        ));
    }
    if input.default_check_interval_minutes < 1 {
        return Err(AppError::validation(
            "Default check interval must be at least 1 minute",
        ));
    }
    if input.history_retention_days < 1 {
        return Err(AppError::validation(
            "History retention must be at least 1 day",
        ));
    }
    let user_agent = input.user_agent.trim().to_string();
    if user_agent.is_empty() {
        return Err(AppError::validation("User-Agent must not be empty"));
    }
    if input.notification_cooldown_minutes < 1 {
        return Err(AppError::validation(
            "Notification cooldown must be at least 1 minute",
        ));
    }

    Ok(AppSettings {
        default_balance_threshold: input.default_balance_threshold,
        default_check_interval_minutes: input.default_check_interval_minutes,
        history_retention_days: input.history_retention_days,
        user_agent,
        notification_cooldown_minutes: input.notification_cooldown_minutes,
        // The master scheduler switch is not part of the Settings-page globals
        // (see UpdateAppSettingsInput); it defaults to on here and the real
        // stored value is surfaced by `settings::get_app_settings`.
        scheduler_enabled: true,
    })
}

/// Supported gateway providers with built-in balance check adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provider {
    #[serde(rename = "new_api")]
    NewApi,
    #[serde(rename = "sub2api")]
    Sub2Api,
    #[serde(rename = "custom_http")]
    CustomHttp,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Provider::NewApi => "new_api",
            Provider::Sub2Api => "sub2api",
            Provider::CustomHttp => "custom_http",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "new_api" => Some(Provider::NewApi),
            "sub2api" => Some(Provider::Sub2Api),
            "custom_http" => Some(Provider::CustomHttp),
            _ => None,
        }
    }
}

/// HTTP method supported by the structured Custom HTTP Adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum CustomHttpMethod {
    Get,
    Post,
}

/// A request header template for the Custom HTTP Adapter.
///
/// Values may contain `{{apiKey}}`, `{{baseUrl}}`, or `{{userAgent}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomHttpHeader {
    pub name: String,
    pub value: String,
}

/// Structured configuration for the Custom HTTP Adapter.
///
/// It intentionally avoids arbitrary JavaScript. The adapter describes a single
/// JSON HTTP request and a set of JSON-path-ish extractors over the response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomHttpAdapterConfig {
    pub method: CustomHttpMethod,
    /// Relative path appended to `base_url`, or a full `http(s)` URL.
    pub path: String,
    #[serde(default)]
    pub headers: Vec<CustomHttpHeader>,
    /// Optional text/JSON body template, only sent for POST requests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Optional validity field. Missing means the credential is considered valid
    /// when the request succeeds and the remaining balance can be extracted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_path: Option<String>,
    /// Optional expected value for `valid_path`; when absent, truthiness is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_equals: Option<serde_json::Value>,
    /// Required numeric response path for remaining balance.
    pub remaining_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_name_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_path: Option<String>,
    /// Optional divisor applied to all numeric extractor values. Use 500000 for
    /// New API-style quota units, for example.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub numeric_divisor: Option<f64>,
    /// Unit used when `unit_path` is empty or does not resolve to a string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_unit: Option<String>,
}

/// Validate and normalize a Custom HTTP Adapter config before storing/running it.
pub fn validate_custom_http_adapter(
    input: CustomHttpAdapterConfig,
) -> AppResult<CustomHttpAdapterConfig> {
    let path = input.path.trim().to_string();
    if path.is_empty() {
        return Err(AppError::validation("Custom adapter path is required"));
    }
    if path.starts_with("http://") || path.starts_with("https://") {
        let parsed = reqwest::Url::parse(&path)
            .map_err(|_| AppError::validation("Custom adapter URL is not valid"))?;
        if parsed.host_str().map(str::is_empty).unwrap_or(true) {
            return Err(AppError::validation(
                "Custom adapter URL must include a host",
            ));
        }
    }

    let remaining_path = input.remaining_path.trim().to_string();
    if remaining_path.is_empty() {
        return Err(AppError::validation(
            "Custom adapter remaining balance path is required",
        ));
    }

    let numeric_divisor = match input.numeric_divisor {
        Some(v) if v.is_finite() && v > 0.0 => Some(v),
        Some(_) => {
            return Err(AppError::validation(
                "Custom adapter numeric divisor must be greater than zero",
            ));
        }
        None => None,
    };

    let mut headers = Vec::with_capacity(input.headers.len());
    for header in input.headers {
        let name = header.name.trim().to_string();
        if name.is_empty() {
            return Err(AppError::validation(
                "Custom adapter header name is required",
            ));
        }
        if name.contains('\r') || name.contains('\n') {
            return Err(AppError::validation(
                "Custom adapter header name must not contain newlines",
            ));
        }
        reqwest::header::HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| AppError::validation(format!("Invalid header name: {name}")))?;
        headers.push(CustomHttpHeader {
            name,
            value: header.value,
        });
    }

    Ok(CustomHttpAdapterConfig {
        method: input.method,
        path,
        headers,
        body: input.body.filter(|s| !s.trim().is_empty()),
        valid_path: trim_optional_path(input.valid_path),
        valid_equals: input.valid_equals,
        remaining_path,
        used_path: trim_optional_path(input.used_path),
        total_path: trim_optional_path(input.total_path),
        unit_path: trim_optional_path(input.unit_path),
        plan_name_path: trim_optional_path(input.plan_name_path),
        message_path: trim_optional_path(input.message_path),
        numeric_divisor,
        default_unit: input
            .default_unit
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
    })
}

fn trim_optional_path(value: Option<String>) -> Option<String> {
    value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Outcome category of a balance check. Kept separate from account enabled state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BalanceResult {
    Healthy,
    LowBalance,
    Failed,
    InvalidCredential,
}

impl BalanceResult {
    pub fn as_str(self) -> &'static str {
        match self {
            BalanceResult::Healthy => "healthy",
            BalanceResult::LowBalance => "low_balance",
            BalanceResult::Failed => "failed",
            BalanceResult::InvalidCredential => "invalid_credential",
        }
    }
}

/// Credentials accepted from the frontend. Which fields are required depends on
/// the provider. On update, empty/absent fields mean "keep existing".
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialsInput {
    #[serde(default)]
    pub access_token: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
}

/// Payload for creating an account.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAccountInput {
    pub name: String,
    pub provider: Provider,
    pub base_url: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub balance_threshold: Option<f64>,
    /// Provider-reported USD credits received for one CNY paid.
    #[serde(default)]
    pub usd_credits_per_cny: Option<f64>,
    #[serde(default)]
    pub check_interval_minutes: Option<i64>,
    /// Optional official website link. Validated as an http(s) URL when present.
    #[serde(default)]
    pub official_url: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub sort_order: Option<i64>,
    #[serde(default)]
    pub credentials: CredentialsInput,
    #[serde(default)]
    pub custom_adapter: Option<CustomHttpAdapterConfig>,
}

/// Payload for updating an account. Credentials left empty are preserved.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAccountInput {
    pub name: String,
    pub base_url: String,
    pub enabled: bool,
    #[serde(default)]
    pub balance_threshold: Option<f64>,
    /// Provider-reported USD credits received for one CNY paid.
    #[serde(default)]
    pub usd_credits_per_cny: Option<f64>,
    #[serde(default)]
    pub check_interval_minutes: Option<i64>,
    /// Optional official website link. Validated as an http(s) URL when present.
    #[serde(default)]
    pub official_url: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub sort_order: Option<i64>,
    #[serde(default)]
    pub credentials: CredentialsInput,
    #[serde(default)]
    pub custom_adapter: Option<CustomHttpAdapterConfig>,
}

fn default_enabled() -> bool {
    true
}

/// Resolved credentials used for fingerprinting and requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Credentials {
    NewApi {
        access_token: String,
        user_id: String,
    },
    Sub2Api {
        api_key: String,
    },
    CustomHttp {
        api_key: String,
    },
}

/// Account view returned to the frontend. Never contains raw credential values.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub id: String,
    pub name: String,
    pub provider: Provider,
    pub base_url: String,
    pub enabled: bool,
    pub balance_threshold: Option<f64>,
    pub usd_credits_per_cny: Option<f64>,
    pub check_interval_minutes: Option<i64>,
    /// Optional official website link (http(s) URL), or `None` when unset.
    pub official_url: Option<String>,
    pub note: Option<String>,
    pub sort_order: i64,
    pub custom_adapter: Option<CustomHttpAdapterConfig>,
    /// True when credentials are stored, so the UI can indicate configured state
    /// without exposing secret values.
    pub has_credentials: bool,
    pub last_result: Option<BalanceResult>,
    pub last_remaining: Option<f64>,
    pub last_used: Option<f64>,
    pub last_total: Option<f64>,
    pub last_unit: Option<String>,
    pub last_plan_name: Option<String>,
    pub last_message: Option<String>,
    pub last_checked_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Account credentials returned when editing. Only called on-demand by the edit form.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountCredentialsView {
    pub provider: Provider,
    pub access_token: Option<String>,
    pub user_id: Option<String>,
    pub api_key: Option<String>,
}

/// The kinds of Notification Event the first release can emit. Each maps 1:1 to
/// the `eventType` field in the webhook payload and the `event_type` column used
/// for cooldown lookups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationEventType {
    LowBalance,
    CheckFailed,
    InvalidCredential,
}

impl NotificationEventType {
    pub fn as_str(self) -> &'static str {
        match self {
            NotificationEventType::LowBalance => "low_balance",
            NotificationEventType::CheckFailed => "check_failed",
            NotificationEventType::InvalidCredential => "invalid_credential",
        }
    }
}

/// Notification hook type: determines payload format and response validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationHookType {
    /// Generic HTTP webhook: original WebhookPayload JSON, HTTP 2xx means success.
    Generic,
    /// Feishu (Lark) bot: text message format, requires HTTP 2xx AND `code: 0` in body.
    Feishu,
    /// WeCom (WeChat Work) bot: text message format, requires HTTP 2xx AND `errcode: 0` in body.
    WeCom,
    /// DingTalk bot: text message format, requires HTTP 2xx AND `errcode: 0` in body.
    DingTalk,
}

impl NotificationHookType {
    pub fn as_str(self) -> &'static str {
        match self {
            NotificationHookType::Generic => "generic",
            NotificationHookType::Feishu => "feishu",
            NotificationHookType::WeCom => "wecom",
            NotificationHookType::DingTalk => "dingtalk",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "generic" => Some(NotificationHookType::Generic),
            "feishu" => Some(NotificationHookType::Feishu),
            "wecom" => Some(NotificationHookType::WeCom),
            "dingtalk" => Some(NotificationHookType::DingTalk),
            _ => None,
        }
    }
}

impl Default for NotificationHookType {
    fn default() -> Self {
        NotificationHookType::Generic
    }
}

/// A notification hook returned to the frontend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookView {
    pub id: String,
    pub name: String,
    pub url: String,
    pub hook_type: NotificationHookType,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Payload for creating a notification hook.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateHookInput {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub hook_type: NotificationHookType,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

/// Payload for updating a notification hook.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateHookInput {
    pub name: String,
    pub url: String,
    pub hook_type: NotificationHookType,
    pub enabled: bool,
}

/// Validate and normalize a hook (name, url) pair.
///
/// The name must be non-empty after trimming, and the URL must parse as an
/// `http`/`https` URL with a host. Other schemes are rejected because the first
/// release only performs generic HTTP(S) webhook POSTs. Returns the trimmed
/// name and URL on success.
pub fn validate_hook(name: &str, url: &str) -> AppResult<(String, String)> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::validation("Hook name is required"));
    }

    let url = url.trim().to_string();
    if url.is_empty() {
        return Err(AppError::validation("Hook URL is required"));
    }
    let parsed = reqwest::Url::parse(&url)
        .map_err(|_| AppError::validation("Hook URL is not a valid URL"))?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(AppError::validation(format!(
                "Hook URL scheme '{other}' is not supported; use http or https"
            )));
        }
    }
    if parsed.host_str().map(str::is_empty).unwrap_or(true) {
        return Err(AppError::validation("Hook URL must include a host"));
    }

    Ok((name, url))
}

/// Validate and normalize an optional account official website link.
///
/// Empty or absent means "no link" and resolves to `None`. A present value must
/// parse as an `http`/`https` URL with a host, mirroring [`validate_hook`]'s URL
/// rules; other schemes and hostless URLs are rejected. Returns the trimmed URL,
/// or `None` when unset.
pub fn validate_official_url(url: Option<&str>) -> AppResult<Option<String>> {
    let Some(url) = url.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| AppError::validation("Official website is not a valid URL"))?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(AppError::validation(format!(
                "Official website scheme '{other}' is not supported; use http or https"
            )));
        }
    }
    if parsed.host_str().map(str::is_empty).unwrap_or(true) {
        return Err(AppError::validation("Official website must include a host"));
    }
    Ok(Some(url.to_string()))
}

/// The fixed JSON body POSTed to a Notification Hook for a Notification Event.
/// Field names are camelCase and this shape is not user-configurable in the
/// first release. `remaining`, `unit`, and `threshold` are null for events that
/// have no balance figures (e.g. `check_failed`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookPayload {
    pub event_type: NotificationEventType,
    pub account_id: String,
    pub account_name: String,
    pub provider: Provider,
    pub remaining: Option<f64>,
    pub unit: Option<String>,
    pub threshold: Option<f64>,
    pub checked_at: String,
    pub message: String,
}

/// A notification delivery/event history row returned to the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryView {
    pub id: String,
    pub account_id: String,
    pub event_type: NotificationEventType,
    pub hook_id: Option<String>,
    pub success: bool,
    pub response_status: Option<i64>,
    pub error_message: Option<String>,
    pub created_at: String,
}

/// A balance check history entry returned to the frontend.
///
/// `account_name` is joined from the owning account when it still exists; a
/// history row can outlive its account only briefly, since the foreign key
/// cascades on delete, but the field is optional to be safe.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: String,
    pub account_id: String,
    pub account_name: Option<String>,
    /// Current account conversion rate. History keeps raw provider values.
    pub usd_credits_per_cny: Option<f64>,
    pub provider: Provider,
    pub result: BalanceResult,
    pub remaining: Option<f64>,
    pub used: Option<f64>,
    pub total: Option<f64>,
    pub unit: Option<String>,
    pub plan_name: Option<String>,
    pub message: Option<String>,
    pub checked_at: String,
}

/// Filters for querying balance check history. All filters are optional and
/// combine with AND; `limit` is clamped to a sensible range by the query layer.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryQuery {
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub result: Option<BalanceResult>,
    #[serde(default)]
    pub provider: Option<Provider>,
    #[serde(default)]
    pub limit: Option<i64>,
}

/// A snapshot of current DB state for the Overview page. Contains only
/// aggregate counts and non-sensitive recent activity; never any credentials.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub total_accounts: i64,
    pub enabled_accounts: i64,
    pub unchecked_accounts: i64,
    pub low_balance_count: i64,
    pub failed_count: i64,
    pub invalid_credential_count: i64,
    /// Total remaining balance grouped by unit, from each account's latest
    /// snapshot. Newest built-in providers report USD, so USD is typically the
    /// only entry, but the shape supports future multi-unit providers.
    pub remaining_by_unit: Vec<RemainingByUnit>,
    pub recent_checks: Vec<HistoryEntry>,
    pub recent_deliveries: Vec<DeliveryView>,
}

/// A per-unit sum of remaining balance across accounts.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemainingByUnit {
    pub unit: String,
    pub total: f64,
    /// How many accounts contributed to this unit's total.
    pub account_count: i64,
}

/// Why an account is or is not part of the scheduled check rotation. Mirrors the
/// scheduler's own eligibility rules ([`crate::accounts::list_enabled_for_schedule`]),
/// so the Schedules page can explain each account's state without re-deriving it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleStatus {
    /// Enabled, has credentials, and past its interval: due on the next tick.
    Due,
    /// Enabled and has credentials, but not yet past its interval.
    Scheduled,
    /// Enabled but missing credentials, so the scheduler skips it.
    MissingCredentials,
    /// Disabled, so the scheduler never checks it.
    Disabled,
}

/// One account's row on the Schedules page. Derived entirely from stored account
/// state plus the global default interval; never contains credentials.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleRow {
    pub id: String,
    pub name: String,
    pub provider: Provider,
    pub enabled: bool,
    pub has_credentials: bool,
    /// The per-account interval override in minutes, if any (raw stored value).
    pub check_interval_minutes: Option<i64>,
    /// The interval actually used: the override when valid, else the default.
    pub effective_interval_minutes: i64,
    pub last_checked_at: Option<String>,
    /// When the next scheduled check is expected, RFC 3339. `None` for accounts
    /// the scheduler never checks (disabled or missing credentials). When the
    /// account is already due this is `now` (the next tick would check it).
    pub next_check_at: Option<String>,
    /// True when a schedulable account is past its interval (or never checked).
    pub due_now: bool,
    pub status: ScheduleStatus,
}

/// The Schedules page snapshot: the global default cadence, account counts by
/// schedulability, and a per-account row list. Contains no credentials.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleOverview {
    /// The global fallback interval in minutes (Settings controls this).
    pub default_check_interval_minutes: i64,
    /// The global automatic-scheduling master switch. When false, no account is
    /// checked automatically regardless of its own `enabled` flag.
    pub scheduler_enabled: bool,
    pub total_accounts: i64,
    pub enabled_accounts: i64,
    pub disabled_accounts: i64,
    /// Enabled accounts that also have credentials: the scheduler's rotation.
    pub schedulable_accounts: i64,
    /// Enabled accounts missing credentials: skipped by the scheduler.
    pub unschedulable_accounts: i64,
    pub rows: Vec<ScheduleRow>,
}

// ---------------------------------------------------------------------------
// Configuration export / import
// ---------------------------------------------------------------------------

/// The export format schema version. Bump this when the shape of
/// [`ExportedConfig`] changes in a backward-incompatible way.
pub const EXPORT_FORMAT_VERSION: i64 = 1;

/// Global defaults and proxy configuration bundled into an export. Mirrors
/// [`AppSettings`] plus the proxy settings, so a single blob captures the whole
/// Settings page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedSettings {
    pub default_balance_threshold: f64,
    pub default_check_interval_minutes: i64,
    pub history_retention_days: i64,
    pub user_agent: String,
    /// Notification cooldown in minutes. Backward-compatible: defaults to 60 if absent.
    #[serde(default = "default_cooldown_minutes")]
    pub notification_cooldown_minutes: Option<i64>,
    /// Global automatic-scheduling switch. Backward-compatible: older exports
    /// without this field default to true (auto-scheduling on).
    #[serde(default = "default_scheduler_enabled")]
    pub scheduler_enabled: bool,
    pub proxy_mode: ProxyMode,
    /// Present only when `proxy_mode` is `custom`.
    #[serde(default)]
    pub proxy_url: Option<String>,
}

fn default_cooldown_minutes() -> Option<i64> {
    Some(60)
}

fn default_scheduler_enabled() -> bool {
    true
}

/// Credentials as they appear inside an export. Only populated when the App User
/// explicitly chooses to include sensitive values. Which fields are set depends
/// on the provider (New API: access token + user ID; Sub2API: API key).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedCredentials {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

/// A gateway account inside an export. Always carries the non-sensitive
/// configuration; `credentials` is only present when credentials were requested
/// on export. The credential fingerprint is intentionally never exported.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedAccount {
    pub name: String,
    pub provider: Provider,
    pub base_url: String,
    pub enabled: bool,
    #[serde(default)]
    pub balance_threshold: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usd_credits_per_cny: Option<f64>,
    #[serde(default)]
    pub check_interval_minutes: Option<i64>,
    /// Optional official website link. Validated on import like the live form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub official_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default)]
    pub sort_order: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credentials: Option<ExportedCredentials>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_adapter: Option<CustomHttpAdapterConfig>,
}

/// A notification hook inside an export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedHook {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub hook_type: NotificationHookType,
    pub enabled: bool,
}

/// The full configuration export/import document. Excludes all history (balance
/// checks and notification deliveries) by design; it is a configuration backup,
/// not an activity log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedConfig {
    /// Schema version; currently always [`EXPORT_FORMAT_VERSION`].
    pub format_version: i64,
    /// RFC 3339 timestamp of when the export was produced.
    pub exported_at: String,
    /// True when this export was produced with credentials included. Purely
    /// informational for the reader; import still gates on its own option.
    pub includes_credentials: bool,
    pub settings: ExportedSettings,
    pub accounts: Vec<ExportedAccount>,
    pub hooks: Vec<ExportedHook>,
}

/// Options controlling an import. Both credential import and overwriting are
/// opt-in so a plain import never silently applies secrets or clobbers existing
/// records.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOptions {
    /// Import credential values from the document. When false, credentials in
    /// the document are ignored entirely.
    #[serde(default)]
    pub import_credentials: bool,
    /// When true, an incoming account matching an existing identity updates it;
    /// when false, the incoming account is skipped (reported, not an error).
    #[serde(default)]
    pub overwrite_existing: bool,
}

/// The result of an import, returned to the frontend so it can show a readable
/// summary. `warnings` explains anything skipped (e.g. accounts without
/// credentials that could not be created).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub accounts_created: i64,
    pub accounts_updated: i64,
    pub accounts_skipped: i64,
    pub hooks_created: i64,
    pub hooks_updated: i64,
    /// 1 when the settings row was updated, 0 otherwise.
    pub settings_updated: i64,
    pub warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn custom(url: Option<&str>) -> UpdateProxySettingsInput {
        UpdateProxySettingsInput {
            mode: ProxyMode::Custom,
            custom_url: url.map(str::to_string),
        }
    }

    #[test]
    fn system_and_none_drop_custom_url() {
        let system = validate_proxy_settings(UpdateProxySettingsInput {
            mode: ProxyMode::System,
            custom_url: Some("http://ignored:8080".into()),
        })
        .unwrap();
        assert_eq!(system.mode, ProxyMode::System);
        assert_eq!(system.custom_url, None);

        let none = validate_proxy_settings(UpdateProxySettingsInput {
            mode: ProxyMode::None,
            custom_url: Some("http://ignored:8080".into()),
        })
        .unwrap();
        assert_eq!(none.mode, ProxyMode::None);
        assert_eq!(none.custom_url, None);
    }

    #[test]
    fn custom_accepts_supported_schemes() {
        for url in [
            "http://proxy.example.com:8080",
            "https://proxy.example.com:8443",
            "socks5://127.0.0.1:1080",
            "http://user:pass@proxy.example.com:8080",
        ] {
            let settings = validate_proxy_settings(custom(Some(url)))
                .unwrap_or_else(|e| panic!("{url} should be accepted: {e}"));
            assert_eq!(settings.mode, ProxyMode::Custom);
            assert_eq!(settings.custom_url.as_deref(), Some(url));
        }
    }

    #[test]
    fn custom_trims_surrounding_whitespace() {
        let settings =
            validate_proxy_settings(custom(Some("  http://proxy.example.com:8080  "))).unwrap();
        assert_eq!(
            settings.custom_url.as_deref(),
            Some("http://proxy.example.com:8080")
        );
    }

    #[test]
    fn custom_requires_a_url() {
        assert!(matches!(
            validate_proxy_settings(custom(None)),
            Err(AppError::Validation(_))
        ));
        assert!(matches!(
            validate_proxy_settings(custom(Some("   "))),
            Err(AppError::Validation(_))
        ));
    }

    #[test]
    fn custom_rejects_unsupported_scheme() {
        assert!(matches!(
            validate_proxy_settings(custom(Some("ftp://proxy.example.com"))),
            Err(AppError::Validation(_))
        ));
        assert!(matches!(
            validate_proxy_settings(custom(Some("socks4://127.0.0.1:1080"))),
            Err(AppError::Validation(_))
        ));
    }

    #[test]
    fn custom_rejects_missing_host() {
        assert!(matches!(
            validate_proxy_settings(custom(Some("http://"))),
            Err(AppError::Validation(_))
        ));
        assert!(matches!(
            validate_proxy_settings(custom(Some("not a url"))),
            Err(AppError::Validation(_))
        ));
    }

    fn app_settings(
        threshold: f64,
        interval: i64,
        retention: i64,
        user_agent: &str,
    ) -> UpdateAppSettingsInput {
        UpdateAppSettingsInput {
            default_balance_threshold: threshold,
            default_check_interval_minutes: interval,
            history_retention_days: retention,
            user_agent: user_agent.to_string(),
            notification_cooldown_minutes: 60,
        }
    }

    #[test]
    fn app_settings_accepts_sensible_values_and_trims_user_agent() {
        let settings =
            validate_app_settings(app_settings(20.0, 30, 30, "  my-agent/1.0 ")).unwrap();
        assert_eq!(settings.default_balance_threshold, 20.0);
        assert_eq!(settings.default_check_interval_minutes, 30);
        assert_eq!(settings.history_retention_days, 30);
        assert_eq!(settings.user_agent, "my-agent/1.0");
    }

    #[test]
    fn app_settings_allows_zero_threshold() {
        assert!(validate_app_settings(app_settings(0.0, 1, 1, "agent")).is_ok());
    }

    #[test]
    fn app_settings_rejects_negative_threshold() {
        assert!(matches!(
            validate_app_settings(app_settings(-1.0, 30, 30, "agent")),
            Err(AppError::Validation(_))
        ));
    }

    #[test]
    fn app_settings_rejects_non_finite_threshold() {
        assert!(matches!(
            validate_app_settings(app_settings(f64::NAN, 30, 30, "agent")),
            Err(AppError::Validation(_))
        ));
    }

    #[test]
    fn app_settings_rejects_interval_below_one() {
        assert!(matches!(
            validate_app_settings(app_settings(20.0, 0, 30, "agent")),
            Err(AppError::Validation(_))
        ));
    }

    #[test]
    fn app_settings_rejects_retention_below_one() {
        assert!(matches!(
            validate_app_settings(app_settings(20.0, 30, 0, "agent")),
            Err(AppError::Validation(_))
        ));
    }

    #[test]
    fn app_settings_rejects_empty_user_agent() {
        assert!(matches!(
            validate_app_settings(app_settings(20.0, 30, 30, "   ")),
            Err(AppError::Validation(_))
        ));
    }

    #[test]
    fn hook_accepts_http_and_https_and_trims() {
        let (name, url) = validate_hook("  Ops channel  ", "  https://hooks.example.com/x  ")
            .expect("valid https hook");
        assert_eq!(name, "Ops channel");
        assert_eq!(url, "https://hooks.example.com/x");

        let (_, url) = validate_hook("h", "http://192.168.1.10:9000/notify").unwrap();
        assert_eq!(url, "http://192.168.1.10:9000/notify");
    }

    #[test]
    fn hook_rejects_empty_name() {
        assert!(matches!(
            validate_hook("   ", "https://hooks.example.com"),
            Err(AppError::Validation(_))
        ));
    }

    #[test]
    fn hook_rejects_empty_url() {
        assert!(matches!(
            validate_hook("h", "   "),
            Err(AppError::Validation(_))
        ));
    }

    #[test]
    fn hook_rejects_non_http_scheme() {
        for url in [
            "ftp://hooks.example.com",
            "socks5://127.0.0.1:1080",
            "ws://x",
        ] {
            assert!(
                matches!(validate_hook("h", url), Err(AppError::Validation(_))),
                "{url} should be rejected",
            );
        }
    }

    #[test]
    fn hook_rejects_invalid_or_hostless_url() {
        assert!(matches!(
            validate_hook("h", "not a url"),
            Err(AppError::Validation(_))
        ));
        assert!(matches!(
            validate_hook("h", "http://"),
            Err(AppError::Validation(_))
        ));
    }

    #[test]
    fn official_url_accepts_empty_as_none() {
        assert_eq!(validate_official_url(None).unwrap(), None);
        assert_eq!(validate_official_url(Some("")).unwrap(), None);
        assert_eq!(validate_official_url(Some("   ")).unwrap(), None);
    }

    #[test]
    fn official_url_accepts_http_and_https_and_trims() {
        assert_eq!(
            validate_official_url(Some("  https://example.com/pricing  ")).unwrap(),
            Some("https://example.com/pricing".to_string())
        );
        assert_eq!(
            validate_official_url(Some("http://192.168.1.10:9000")).unwrap(),
            Some("http://192.168.1.10:9000".to_string())
        );
    }

    #[test]
    fn official_url_rejects_non_http_scheme() {
        for url in ["ftp://example.com", "socks5://127.0.0.1", "ws://x"] {
            assert!(
                matches!(
                    validate_official_url(Some(url)),
                    Err(AppError::Validation(_))
                ),
                "{url} should be rejected",
            );
        }
    }

    #[test]
    fn official_url_rejects_invalid_or_hostless_url() {
        assert!(matches!(
            validate_official_url(Some("not a url")),
            Err(AppError::Validation(_))
        ));
        assert!(matches!(
            validate_official_url(Some("http://")),
            Err(AppError::Validation(_))
        ));
    }
}
