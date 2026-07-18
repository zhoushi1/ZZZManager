use std::time::Duration;

use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::models::{
    Credentials, CustomHttpAdapterConfig, CustomHttpMethod, Provider, ProxyMode, ProxySettings,
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// Build a reqwest client configured for the given proxy settings.
///
/// - `system`: default reqwest behavior (honors OS/environment proxies).
/// - `none`: disable proxies via `no_proxy()`.
/// - `custom`: route all requests through `ProxySettings::custom_url`.
///
/// Keeps the shared 15s timeout. The custom URL is assumed pre-validated by
/// [`crate::models::validate_proxy_settings`].
pub fn build_client(settings: &ProxySettings) -> AppResult<reqwest::Client> {
    let mut builder = reqwest::Client::builder().timeout(REQUEST_TIMEOUT);

    match settings.mode {
        ProxyMode::System => {}
        ProxyMode::None => {
            builder = builder.no_proxy();
        }
        ProxyMode::Custom => {
            let url = settings
                .custom_url
                .as_deref()
                .ok_or_else(|| AppError::validation("custom proxy mode requires a proxy URL"))?;
            let proxy = reqwest::Proxy::all(url)
                .map_err(|e| AppError::validation(format!("invalid proxy URL: {e}")))?;
            builder = builder.proxy(proxy);
        }
    }

    builder
        .build()
        .map_err(|e| AppError::Request(e.to_string()))
}

/// Normalized result of a provider balance check, before threshold evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckOutcome {
    /// Whether the credential appears valid.
    pub valid: bool,
    pub remaining: Option<f64>,
    pub used: Option<f64>,
    pub total: Option<f64>,
    pub unit: Option<String>,
    pub plan_name: Option<String>,
    /// Human-readable message, primarily for invalid/failed cases.
    pub message: Option<String>,
}

impl CheckOutcome {
    fn invalid(message: impl Into<String>) -> Self {
        CheckOutcome {
            valid: false,
            remaining: None,
            used: None,
            total: None,
            unit: None,
            plan_name: None,
            message: Some(message.into()),
        }
    }
}

/// Perform a balance check against the provider endpoint using a client built
/// from the current proxy settings. `user_agent` is the configured User-Agent
/// header sent on the outbound request (shared by manual and scheduled checks).
pub async fn check_balance(
    provider: Provider,
    base_url: &str,
    credentials: &Credentials,
    custom_adapter: Option<&CustomHttpAdapterConfig>,
    proxy: &ProxySettings,
    user_agent: &str,
) -> AppResult<CheckOutcome> {
    let client = build_client(proxy)?;

    match (provider, credentials) {
        (Provider::NewApi, Credentials::NewApi { access_token, user_id }) => {
            let url = format!("{base_url}/api/user/self");
            let resp = client
                .get(url)
                .header("Content-Type", "application/json")
                .header("Authorization", format!("Bearer {access_token}"))
                .header("User-Agent", user_agent)
                .header("New-Api-User", user_id)
                .send()
                .await
                .map_err(|e| AppError::Request(e.to_string()))?;

            if !resp.status().is_success() {
                let status = resp.status();
                return Ok(CheckOutcome::invalid(format!("查询失败 (HTTP {status})")));
            }

            let body: Value = resp
                .json()
                .await
                .map_err(|e| AppError::Request(e.to_string()))?;
            Ok(extract_new_api(&body))
        }
        (Provider::Sub2Api, Credentials::Sub2Api { api_key }) => {
            let url = format!("{base_url}/v1/usage");
            let resp = client
                .get(url)
                .header("Authorization", format!("Bearer {api_key}"))
                .header("User-Agent", user_agent)
                .send()
                .await
                .map_err(|e| AppError::Request(e.to_string()))?;

            if !resp.status().is_success() {
                let status = resp.status();
                return Ok(CheckOutcome::invalid(format!("查询失败 (HTTP {status})")));
            }

            let body: Value = resp
                .json()
                .await
                .map_err(|e| AppError::Request(e.to_string()))?;
            Ok(extract_sub2api(&body))
        }
        (Provider::CustomHttp, Credentials::CustomHttp { api_key }) => {
            let config = custom_adapter
                .ok_or_else(|| AppError::validation("Custom HTTP requires an adapter config"))?;
            check_custom_http(&client, base_url, api_key, config, user_agent).await
        }
        // Provider/credential mismatch should not happen; guard defensively.
        _ => Err(AppError::validation("credentials do not match provider")),
    }
}

async fn check_custom_http(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    config: &CustomHttpAdapterConfig,
    user_agent: &str,
) -> AppResult<CheckOutcome> {
    let url = custom_url(base_url, &config.path)?;
    let mut request = match config.method {
        CustomHttpMethod::Get => client.get(url),
        CustomHttpMethod::Post => client.post(url),
    };

    let has_user_agent = config
        .headers
        .iter()
        .any(|h| h.name.eq_ignore_ascii_case("user-agent"));
    if !has_user_agent {
        request = request.header("User-Agent", user_agent);
    }

    for header in &config.headers {
        request = request.header(&header.name, render_template(&header.value, base_url, api_key, user_agent));
    }

    if config.method == CustomHttpMethod::Post {
        if let Some(body) = &config.body {
            request = request.body(render_template(body, base_url, api_key, user_agent));
        }
    }

    let resp = request
        .send()
        .await
        .map_err(|e| AppError::Request(e.to_string()))?;

    if !resp.status().is_success() {
        let status = resp.status();
        return Ok(CheckOutcome::invalid(format!("查询失败 (HTTP {status})")));
    }

    let body: Value = resp
        .json()
        .await
        .map_err(|e| AppError::Request(e.to_string()))?;
    Ok(extract_custom_http(&body, config))
}

fn custom_url(base_url: &str, path: &str) -> AppResult<String> {
    if path.starts_with("http://") || path.starts_with("https://") {
        return Ok(path.to_string());
    }
    let base = base_url.trim().trim_end_matches('/');
    let path = path.trim_start_matches('/');
    if base.is_empty() {
        return Err(AppError::validation("Base URL is required"));
    }
    Ok(format!("{base}/{path}"))
}

fn render_template(value: &str, base_url: &str, api_key: &str, user_agent: &str) -> String {
    value
        .replace("{{baseUrl}}", base_url)
        .replace("{{apiKey}}", api_key)
        .replace("{{userAgent}}", user_agent)
}

/// Pure extractor for the Custom HTTP Adapter response body.
pub fn extract_custom_http(body: &Value, config: &CustomHttpAdapterConfig) -> CheckOutcome {
    let valid = match config.valid_path.as_deref() {
        Some(path) => match read_path(body, path) {
            Some(value) => match &config.valid_equals {
                Some(expected) => value == expected,
                None => value_truthy(value),
            },
            None => false,
        },
        None => true,
    };

    let divisor = config.numeric_divisor.unwrap_or(1.0);
    let remaining = read_number(body, &config.remaining_path, divisor);
    let used = config
        .used_path
        .as_deref()
        .and_then(|path| read_number(body, path, divisor));
    let total = config
        .total_path
        .as_deref()
        .and_then(|path| read_number(body, path, divisor));
    let unit = config
        .unit_path
        .as_deref()
        .and_then(|path| read_string(body, path))
        .or_else(|| config.default_unit.clone());
    let plan_name = config
        .plan_name_path
        .as_deref()
        .and_then(|path| read_string(body, path));
    let message = config
        .message_path
        .as_deref()
        .and_then(|path| read_string(body, path));

    if !valid {
        return CheckOutcome::invalid(message.unwrap_or_else(|| "自定义适配器校验失败".to_string()));
    }
    if remaining.is_none() {
        return CheckOutcome::invalid(
            message.unwrap_or_else(|| "自定义适配器未提取到余额".to_string()),
        );
    }

    CheckOutcome {
        valid: true,
        remaining,
        used,
        total,
        unit,
        plan_name,
        message: None,
    }
}

fn read_number(body: &Value, path: &str, divisor: f64) -> Option<f64> {
    read_path(body, path).and_then(|value| {
        value
            .as_f64()
            .or_else(|| value.as_str().and_then(|s| s.parse::<f64>().ok()))
            .map(|n| n / divisor)
    })
}

fn read_string(body: &Value, path: &str) -> Option<String> {
    read_path(body, path).and_then(|value| match value {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    })
}

fn read_path<'a>(body: &'a Value, path: &str) -> Option<&'a Value> {
    let path = path.trim().trim_start_matches('$').trim_start_matches('.');
    if path.is_empty() {
        return Some(body);
    }
    let mut current = body;
    for segment in path.split('.') {
        let segment = segment.trim();
        if segment.is_empty() {
            return None;
        }
        current = if let Ok(index) = segment.parse::<usize>() {
            current.as_array()?.get(index)?
        } else {
            current.get(segment)?
        };
    }
    Some(current)
}

fn value_truthy(value: &Value) -> bool {
    match value {
        Value::Bool(v) => *v,
        Value::Number(n) => n.as_f64().map(|v| v != 0.0).unwrap_or(false),
        Value::String(s) => matches!(
            s.trim().to_ascii_lowercase().as_str(),
            "true" | "1" | "yes" | "active" | "ok" | "success"
        ),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
        Value::Null => false,
    }
}

/// New API divisor: quota units per 1 USD.
const NEW_API_QUOTA_UNIT: f64 = 500_000.0;

/// Pure extractor for the New API `/api/user/self` response body.
pub fn extract_new_api(body: &Value) -> CheckOutcome {
    let success = body.get("success").and_then(Value::as_bool).unwrap_or(false);
    let data = body.get("data");

    if success && data.map(|d| !d.is_null()).unwrap_or(false) {
        let data = data.unwrap();
        let quota = data.get("quota").and_then(Value::as_f64).unwrap_or(0.0);
        let used_quota = data
            .get("used_quota")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let plan_name = data
            .get("group")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or("默认套餐")
            .to_string();

        CheckOutcome {
            valid: true,
            remaining: Some(quota / NEW_API_QUOTA_UNIT),
            used: Some(used_quota / NEW_API_QUOTA_UNIT),
            total: Some((quota + used_quota) / NEW_API_QUOTA_UNIT),
            unit: Some("USD".to_string()),
            plan_name: Some(plan_name),
            message: None,
        }
    } else {
        let message = body
            .get("message")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or("查询失败")
            .to_string();
        CheckOutcome::invalid(message)
    }
}

/// Pure extractor for the Sub2API `/v1/usage` response body.
///
/// Mirrors the reference JS logic:
/// ```js
/// const remaining = response?.remaining ?? response?.quota?.remaining ?? response?.balance;
/// const unit = response?.unit ?? response?.quota?.unit ?? "USD";
/// return { isValid: response?.is_active ?? response?.isValid ?? true, remaining, unit };
/// ```
/// Numeric fields accept either a JSON number or a numeric string.
pub fn extract_sub2api(body: &Value) -> CheckOutcome {
    let quota = body.get("quota");

    // remaining: remaining -> quota.remaining -> balance (legacy)
    let remaining = value_number(body.get("remaining"))
        .or_else(|| value_number(quota.and_then(|q| q.get("remaining"))))
        .or_else(|| value_number(body.get("balance")));

    // unit: unit -> quota.unit -> "USD"
    let unit = value_nonempty_string(body.get("unit"))
        .or_else(|| value_nonempty_string(quota.and_then(|q| q.get("unit"))))
        .unwrap_or_else(|| "USD".to_string());

    // Treat a missing validity flag as active; respect an explicit `false`.
    // is_active -> isValid -> true
    let is_valid = body
        .get("is_active")
        .and_then(Value::as_bool)
        .or_else(|| body.get("isValid").and_then(Value::as_bool))
        .unwrap_or(true);

    CheckOutcome {
        valid: is_valid,
        remaining,
        used: None,
        total: None,
        unit: Some(unit),
        plan_name: None,
        message: if is_valid {
            None
        } else {
            Some("账户未激活".to_string())
        },
    }
}

/// Read a JSON value as an `f64`, accepting either a number or a numeric string.
fn value_number(value: Option<&Value>) -> Option<f64> {
    value.and_then(|v| {
        v.as_f64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok()))
    })
}

/// Read a JSON value as a non-empty string.
fn value_nonempty_string(value: Option<&Value>) -> Option<String> {
    value.and_then(|v| v.as_str()).and_then(|s| {
        let s = s.trim();
        if s.is_empty() {
            None
        } else {
            Some(s.to_string())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn settings(mode: ProxyMode, url: Option<&str>) -> ProxySettings {
        ProxySettings {
            mode,
            custom_url: url.map(str::to_string),
        }
    }

    #[test]
    fn build_client_system_mode_succeeds() {
        assert!(build_client(&settings(ProxyMode::System, None)).is_ok());
    }

    #[test]
    fn build_client_none_mode_succeeds() {
        assert!(build_client(&settings(ProxyMode::None, None)).is_ok());
    }

    #[test]
    fn build_client_custom_accepts_supported_schemes() {
        for url in [
            "http://proxy.example.com:8080",
            "https://proxy.example.com:8443",
            "socks5://127.0.0.1:1080",
        ] {
            assert!(
                build_client(&settings(ProxyMode::Custom, Some(url))).is_ok(),
                "{url} should build a client",
            );
        }
    }

    #[test]
    fn build_client_custom_without_url_errors() {
        let err = build_client(&settings(ProxyMode::Custom, None)).unwrap_err();
        assert!(matches!(err, AppError::Validation(_)));
    }

    #[test]
    fn new_api_extracts_quota_in_usd() {
        let body = json!({
            "success": true,
            "data": { "group": "vip", "quota": 5_000_000, "used_quota": 1_000_000 }
        });
        let outcome = extract_new_api(&body);
        assert!(outcome.valid);
        assert_eq!(outcome.remaining, Some(10.0));
        assert_eq!(outcome.used, Some(2.0));
        assert_eq!(outcome.total, Some(12.0));
        assert_eq!(outcome.unit.as_deref(), Some("USD"));
        assert_eq!(outcome.plan_name.as_deref(), Some("vip"));
    }

    #[test]
    fn new_api_defaults_plan_name_when_group_missing() {
        let body = json!({ "success": true, "data": { "quota": 0, "used_quota": 0 } });
        let outcome = extract_new_api(&body);
        assert_eq!(outcome.plan_name.as_deref(), Some("默认套餐"));
    }

    #[test]
    fn new_api_invalid_uses_message() {
        let body = json!({ "success": false, "message": "无效令牌" });
        let outcome = extract_new_api(&body);
        assert!(!outcome.valid);
        assert_eq!(outcome.message.as_deref(), Some("无效令牌"));
    }

    #[test]
    fn new_api_invalid_falls_back_to_default_message() {
        let body = json!({ "success": false });
        let outcome = extract_new_api(&body);
        assert!(!outcome.valid);
        assert_eq!(outcome.message.as_deref(), Some("查询失败"));
    }

    #[test]
    fn sub2api_extracts_remaining_from_v1_usage() {
        // Primary `/v1/usage` shape: top-level `remaining` + `unit`, `is_active`.
        let body = json!({ "remaining": 42.5, "unit": "USD", "is_active": true });
        let outcome = extract_sub2api(&body);
        assert!(outcome.valid);
        assert_eq!(outcome.remaining, Some(42.5));
        assert_eq!(outcome.unit.as_deref(), Some("USD"));
    }

    #[test]
    fn sub2api_falls_back_to_quota_remaining() {
        // No top-level `remaining`; nested `quota.remaining` and `quota.unit` win.
        let body = json!({ "quota": { "remaining": 12.0, "unit": "CNY" } });
        let outcome = extract_sub2api(&body);
        assert!(outcome.valid);
        assert_eq!(outcome.remaining, Some(12.0));
        assert_eq!(outcome.unit.as_deref(), Some("CNY"));
    }

    #[test]
    fn sub2api_falls_back_to_legacy_balance() {
        // Legacy field still supported as the lowest-priority remaining source.
        let body = json!({ "balance": 7.0 });
        let outcome = extract_sub2api(&body);
        assert!(outcome.valid);
        assert_eq!(outcome.remaining, Some(7.0));
        // Unit defaults to USD when neither `unit` nor `quota.unit` is present.
        assert_eq!(outcome.unit.as_deref(), Some("USD"));
    }

    #[test]
    fn sub2api_remaining_priority_prefers_top_level() {
        // remaining -> quota.remaining -> balance: top-level `remaining` wins.
        let body = json!({
            "remaining": 1.0,
            "quota": { "remaining": 2.0 },
            "balance": 3.0
        });
        assert_eq!(extract_sub2api(&body).remaining, Some(1.0));

        // Without top-level `remaining`, `quota.remaining` beats `balance`.
        let body = json!({ "quota": { "remaining": 2.0 }, "balance": 3.0 });
        assert_eq!(extract_sub2api(&body).remaining, Some(2.0));
    }

    #[test]
    fn sub2api_unit_priority_prefers_top_level() {
        // unit -> quota.unit -> "USD": top-level `unit` wins over `quota.unit`.
        let body = json!({
            "remaining": 1.0,
            "unit": "EUR",
            "quota": { "unit": "CNY" }
        });
        assert_eq!(extract_sub2api(&body).unit.as_deref(), Some("EUR"));

        // Without top-level `unit`, `quota.unit` is used.
        let body = json!({ "remaining": 1.0, "quota": { "unit": "CNY" } });
        assert_eq!(extract_sub2api(&body).unit.as_deref(), Some("CNY"));
    }

    #[test]
    fn sub2api_accepts_numeric_string_remaining() {
        // Numbers may arrive as strings; both top-level and nested are parsed.
        let body = json!({ "remaining": "15.5" });
        assert_eq!(extract_sub2api(&body).remaining, Some(15.5));

        let body = json!({ "quota": { "remaining": "9" } });
        assert_eq!(extract_sub2api(&body).remaining, Some(9.0));
    }

    #[test]
    fn sub2api_missing_validity_defaults_valid() {
        let body = json!({ "remaining": 7.0 });
        let outcome = extract_sub2api(&body);
        assert!(outcome.valid);
        assert_eq!(outcome.remaining, Some(7.0));
    }

    #[test]
    fn sub2api_explicit_is_active_false_is_invalid() {
        let body = json!({ "remaining": 0.0, "is_active": false });
        let outcome = extract_sub2api(&body);
        assert!(!outcome.valid);
        assert_eq!(outcome.message.as_deref(), Some("账户未激活"));
    }

    #[test]
    fn sub2api_explicit_is_valid_false_is_invalid() {
        // Falls back to `isValid` when `is_active` is absent.
        let body = json!({ "remaining": 5.0, "isValid": false });
        let outcome = extract_sub2api(&body);
        assert!(!outcome.valid);
        assert_eq!(outcome.message.as_deref(), Some("账户未激活"));
    }

    fn custom_config() -> CustomHttpAdapterConfig {
        CustomHttpAdapterConfig {
            method: CustomHttpMethod::Get,
            path: "/balance".into(),
            headers: vec![],
            body: None,
            valid_path: Some("success".into()),
            valid_equals: None,
            remaining_path: "data.remaining".into(),
            used_path: Some("data.used".into()),
            total_path: Some("data.total".into()),
            unit_path: Some("data.unit".into()),
            plan_name_path: Some("data.plan".into()),
            message_path: Some("message".into()),
            numeric_divisor: Some(100.0),
            default_unit: Some("USD".into()),
        }
    }

    #[test]
    fn custom_http_extracts_structured_paths() {
        let body = json!({
            "success": true,
            "data": {
                "remaining": 1234,
                "used": "66",
                "total": 1300,
                "unit": "CNY",
                "plan": "pro"
            }
        });
        let outcome = extract_custom_http(&body, &custom_config());
        assert!(outcome.valid);
        assert_eq!(outcome.remaining, Some(12.34));
        assert_eq!(outcome.used, Some(0.66));
        assert_eq!(outcome.total, Some(13.0));
        assert_eq!(outcome.unit.as_deref(), Some("CNY"));
        assert_eq!(outcome.plan_name.as_deref(), Some("pro"));
    }

    #[test]
    fn custom_http_invalid_uses_message_path() {
        let body = json!({ "success": false, "message": "bad key" });
        let outcome = extract_custom_http(&body, &custom_config());
        assert!(!outcome.valid);
        assert_eq!(outcome.message.as_deref(), Some("bad key"));
    }
}
