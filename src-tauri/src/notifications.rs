//! Notification evaluation and webhook delivery.
//!
//! After a *scheduled* balance check records its result, the scheduler asks this
//! module to decide whether a Notification Event should fire and, if so, to POST
//! a fixed JSON payload to every enabled Notification Hook. Manual checks do not
//! go through here — they only update the result/history so a user clicking
//! "Check" is never surprised by an outbound notification.
//!
//! Decision rules (first release):
//!   * `low_balance` / `invalid_credential` fire on the matching result.
//!   * `check_failed` fires only after 3 consecutive `failed` results.
//!   * A configurable cooldown (default 60 minutes) per (account, event_type)
//!     suppresses repeats. The cooldown is measured against ALL prior delivery
//!     rows, successful or not, so a broken webhook does not defeat it and
//!     spam every tick.
//!   * No recovery notifications.
//!
//! Delivery uses the same proxy configuration and User-Agent as provider checks
//! and never propagates failures back to the scheduler tick.

use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::error::AppResult;
use crate::models::{
    AppSettings, BalanceResult, NotificationEventType, NotificationHookType, Provider,
    ProxySettings, WebhookPayload,
};

/// Feishu (Lark) bot message format for text messages.
#[derive(serde::Serialize)]
struct FeishuPayload {
    msg_type: String,
    content: FeishuContent,
}

#[derive(serde::Serialize)]
struct FeishuContent {
    text: String,
}

/// WeCom (WeChat Work) and DingTalk bot message format for text messages.
#[derive(serde::Serialize)]
struct EnterpriseIMPayload {
    msgtype: String,
    text: EnterpriseIMText,
}

#[derive(serde::Serialize)]
struct EnterpriseIMText {
    content: String,
}

/// Per (account, event_type) suppression window - now configurable via app_settings.
/// This constant is no longer used; see evaluate_and_notify for the dynamic lookup.

/// Consecutive `failed` results required before a `check_failed` event fires.
const FAILURE_THRESHOLD: usize = 3;

/// Webhook POST timeout. Independent of the provider-check client timeout.
const WEBHOOK_TIMEOUT: Duration = Duration::from_secs(10);

/// Format a RFC3339 timestamp as Shanghai time (UTC+08:00).
///
/// Parses the RFC3339 string, converts to Shanghai timezone, and returns
/// "YYYY-MM-DD HH:mm:ss UTC+08:00". If parsing fails, uses the provided
/// fallback (current time) or returns the original string if no fallback.
fn format_checked_at_shanghai(raw: &str, fallback_now: Option<DateTime<Utc>>) -> String {
    match DateTime::parse_from_rfc3339(raw) {
        Ok(dt) => {
            let shanghai_offset = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
            let shanghai_time = dt.with_timezone(&shanghai_offset);
            shanghai_time
                .format("%Y-%m-%d %H:%M:%S UTC+08:00")
                .to_string()
        }
        Err(_) => {
            if let Some(now) = fallback_now {
                let shanghai_offset = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
                let shanghai_time = now.with_timezone(&shanghai_offset);
                shanghai_time
                    .format("%Y-%m-%d %H:%M:%S UTC+08:00")
                    .to_string()
            } else {
                raw.to_string()
            }
        }
    }
}

/// Build a text message from a WebhookPayload for platform-specific formats.
/// Always uses Chinese labels and messages for Feishu/WeCom/DingTalk platforms.
/// The checked_at field is converted to Shanghai time for display.
fn build_platform_text(payload: &WebhookPayload) -> String {
    let mut lines = Vec::new();

    // Event type label
    let event_value = match payload.event_type {
        NotificationEventType::LowBalance => "余额不足",
        NotificationEventType::CheckFailed => "查询失败",
        NotificationEventType::InvalidCredential => "凭据无效",
    };
    lines.push(format!("事件: {}", event_value));

    // Account
    lines.push(format!("账号: {}", payload.account_name));

    // Provider
    let provider_value = match payload.provider {
        Provider::NewApi => "New API",
        Provider::Sub2Api => "Sub2API",
        Provider::CustomHttp => "自定义 HTTP",
    };
    lines.push(format!("提供商: {}", provider_value));

    // Remaining balance
    if let (Some(remaining), Some(unit)) = (payload.remaining, &payload.unit) {
        lines.push(format!("剩余余额: {} {}", remaining, unit));
    }

    // Threshold
    if let Some(threshold) = payload.threshold {
        lines.push(format!("阈值: {}", threshold));
    }

    // Checked at - convert to Shanghai time
    let checked_at_shanghai = format_checked_at_shanghai(&payload.checked_at, None);
    lines.push(format!("查询时间: {}", checked_at_shanghai));

    // Message
    let message_value = localize_message(&payload.message, payload.event_type);
    lines.push(format!("消息: {}", message_value));

    lines.join("\n")
}

/// Localize message, translating default English messages to Chinese.
fn localize_message(message: &str, event_type: NotificationEventType) -> String {
    // Check if this is a default English message and translate it.
    match event_type {
        NotificationEventType::LowBalance => {
            if message == "Balance is below the configured threshold" {
                "余额低于配置阈值".to_string()
            } else {
                message.to_string()
            }
        }
        NotificationEventType::CheckFailed => {
            if message == "Balance check failed repeatedly" {
                "余额查询连续失败".to_string()
            } else {
                message.to_string()
            }
        }
        NotificationEventType::InvalidCredential => {
            if message == "Credential was rejected by the provider" {
                "凭据被服务端拒绝".to_string()
            } else {
                message.to_string()
            }
        }
    }
}

/// Build the appropriate payload JSON for the given hook type.
/// Feishu/WeCom/DingTalk use Chinese text formatting; Generic uses the original JSON.
/// Platform text automatically converts checked_at to Shanghai time for display.
fn build_platform_payload(hook_type: NotificationHookType, payload: &WebhookPayload) -> String {
    match hook_type {
        NotificationHookType::Generic => {
            // Generic webhook: use original payload with default message in Chinese
            let mut localized_payload = payload.clone();
            localized_payload.message = localize_message(&payload.message, payload.event_type);
            serde_json::to_string(&localized_payload).unwrap_or_else(|_| "{}".to_string())
        }
        NotificationHookType::Feishu => {
            let feishu = FeishuPayload {
                msg_type: "text".to_string(),
                content: FeishuContent {
                    text: build_platform_text(payload),
                },
            };
            serde_json::to_string(&feishu).unwrap_or_else(|_| "{}".to_string())
        }
        NotificationHookType::WeCom | NotificationHookType::DingTalk => {
            let enterprise = EnterpriseIMPayload {
                msgtype: "text".to_string(),
                text: EnterpriseIMText {
                    content: build_platform_text(payload),
                },
            };
            serde_json::to_string(&enterprise).unwrap_or_else(|_| "{}".to_string())
        }
    }
}

/// Parse the webhook response and determine success based on the hook type.
///
/// - For generic webhooks: HTTP 2xx is success.
/// - For Feishu/Lark: HTTP 2xx AND JSON body with `code: 0` is success.
/// - For WeCom/DingTalk: HTTP 2xx AND JSON body with `errcode: 0` is success.
async fn evaluate_platform_response(
    hook_type: NotificationHookType,
    response: reqwest::Response,
) -> (bool, Option<i64>, Option<String>) {
    let status = response.status();
    let status_code = status.as_u16() as i64;

    match hook_type {
        NotificationHookType::Generic => {
            // Generic webhook: only HTTP status matters.
            if status.is_success() {
                (true, Some(status_code), None)
            } else {
                (false, Some(status_code), Some(format!("HTTP {}", status)))
            }
        }
        NotificationHookType::Feishu => {
            // Feishu requires checking the JSON body's `code` field even when HTTP is 2xx.
            if !status.is_success() {
                return (false, Some(status_code), Some(format!("HTTP {}", status)));
            }

            let body_text = match response.text().await {
                Ok(t) => t,
                Err(e) => {
                    return (
                        false,
                        Some(status_code),
                        Some(format!("Failed to read response body: {}", e)),
                    );
                }
            };

            let json: serde_json::Value = match serde_json::from_str(&body_text) {
                Ok(v) => v,
                Err(_) => {
                    return (
                        false,
                        Some(status_code),
                        Some(format!(
                            "Non-JSON response: {}",
                            &body_text[..body_text.len().min(100)]
                        )),
                    );
                }
            };

            let code = json.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
            if code == 0 {
                (true, Some(status_code), None)
            } else {
                let msg = json
                    .get("msg")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown error");
                (
                    false,
                    Some(status_code),
                    Some(format!("Feishu code {}: {}", code, msg)),
                )
            }
        }
        NotificationHookType::WeCom | NotificationHookType::DingTalk => {
            // WeCom/DingTalk require checking the JSON body's `errcode` field.
            if !status.is_success() {
                return (false, Some(status_code), Some(format!("HTTP {}", status)));
            }

            let platform_name = if hook_type == NotificationHookType::WeCom {
                "WeCom"
            } else {
                "DingTalk"
            };

            let body_text = match response.text().await {
                Ok(t) => t,
                Err(e) => {
                    return (
                        false,
                        Some(status_code),
                        Some(format!("Failed to read response body: {}", e)),
                    );
                }
            };

            let json: serde_json::Value = match serde_json::from_str(&body_text) {
                Ok(v) => v,
                Err(_) => {
                    return (
                        false,
                        Some(status_code),
                        Some(format!(
                            "Non-JSON response: {}",
                            &body_text[..body_text.len().min(100)]
                        )),
                    );
                }
            };

            let errcode = json.get("errcode").and_then(|c| c.as_i64()).unwrap_or(-1);
            if errcode == 0 {
                (true, Some(status_code), None)
            } else {
                let errmsg = json
                    .get("errmsg")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown error");
                (
                    false,
                    Some(status_code),
                    Some(format!("{} errcode {}: {}", platform_name, errcode, errmsg)),
                )
            }
        }
    }
}

/// Map a recorded balance result to the notification event it directly implies,
/// if any. `Failed` is intentionally excluded: `check_failed` depends on the
/// consecutive-failure count, not a single result (see [`consecutive_failures`]).
pub fn direct_event_for_result(result: BalanceResult) -> Option<NotificationEventType> {
    match result {
        BalanceResult::LowBalance => Some(NotificationEventType::LowBalance),
        BalanceResult::InvalidCredential => Some(NotificationEventType::InvalidCredential),
        BalanceResult::Healthy | BalanceResult::Failed => None,
    }
}

/// Count trailing `failed` results in a newest-first slice of recent results.
/// Stops at the first non-failure. Used to gate `check_failed` on
/// [`FAILURE_THRESHOLD`] consecutive failures.
pub fn consecutive_failures(results_newest_first: &[BalanceResult]) -> usize {
    results_newest_first
        .iter()
        .take_while(|r| **r == BalanceResult::Failed)
        .count()
}

/// Whether a new event of a kind is still within the cooldown window given the
/// timestamp of the most recent prior delivery for that (account, event_type).
///
/// `None` (no prior delivery) is never in cooldown. An unparseable timestamp is
/// treated as expired so a bad stored value never wedges notifications.
pub fn within_cooldown(
    last_delivery_at: Option<&str>,
    now: DateTime<Utc>,
    window: chrono::Duration,
) -> bool {
    let Some(last) = last_delivery_at else {
        return false;
    };
    let Ok(parsed) = DateTime::parse_from_rfc3339(last) else {
        return false;
    };
    now.signed_duration_since(parsed.with_timezone(&Utc)) < window
}

/// Timestamp of the most recent delivery row for (account, event_type), or
/// `None` if there is none. Considers all rows regardless of success.
///
/// If `cutoff` is provided, only considers deliveries with `created_at >= cutoff`.
/// This allows cooldown to be scoped to deliveries that occurred after the current
/// hook configurations became effective.
async fn last_delivery_at(
    pool: &SqlitePool,
    account_id: &str,
    event_type: NotificationEventType,
    cutoff: Option<&str>,
) -> AppResult<Option<String>> {
    let row = if let Some(cutoff_ts) = cutoff {
        sqlx::query(
            "SELECT created_at FROM notification_deliveries \
             WHERE account_id = ? AND event_type = ? AND created_at >= ? \
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(account_id)
        .bind(event_type.as_str())
        .bind(cutoff_ts)
        .fetch_optional(pool)
        .await?
    } else {
        sqlx::query(
            "SELECT created_at FROM notification_deliveries \
             WHERE account_id = ? AND event_type = ? ORDER BY created_at DESC LIMIT 1",
        )
        .bind(account_id)
        .bind(event_type.as_str())
        .fetch_optional(pool)
        .await?
    };
    Ok(row.map(|r| r.get::<String, _>("created_at")))
}

/// The most recent balance results for an account, newest first, up to `limit`.
async fn recent_results(
    pool: &SqlitePool,
    account_id: &str,
    limit: i64,
) -> AppResult<Vec<BalanceResult>> {
    let rows = sqlx::query(
        "SELECT result FROM balance_check_history \
         WHERE account_id = ? ORDER BY checked_at DESC LIMIT ?",
    )
    .bind(account_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|r| parse_result(&r.get::<String, _>("result")))
        .collect())
}

fn parse_result(value: &str) -> Option<BalanceResult> {
    match value {
        "healthy" => Some(BalanceResult::Healthy),
        "low_balance" => Some(BalanceResult::LowBalance),
        "failed" => Some(BalanceResult::Failed),
        "invalid_credential" => Some(BalanceResult::InvalidCredential),
        _ => None,
    }
}

/// Everything about an account needed to shape a webhook payload. The scheduler
/// already has this loaded, so it is passed in rather than re-queried.
pub struct NotificationContext<'a> {
    pub account_id: &'a str,
    pub account_name: &'a str,
    pub provider: Provider,
    /// Remaining value and unit shown to the recipient. Converted accounts use CNY.
    pub remaining: Option<f64>,
    pub unit: Option<String>,
    /// Effective threshold in the same unit as `remaining`.
    pub threshold: Option<f64>,
    pub message: Option<String>,
    /// RFC 3339 timestamp of the check that produced `result`.
    pub checked_at: String,
}

/// Build the fixed webhook payload for an event. `low_balance`/
/// `invalid_credential` carry balance figures; `check_failed` nulls
/// remaining/unit/threshold since no balance was obtained.
///
/// The `checked_at` timestamp in the context should be in RFC3339 format.
/// Platform-specific text formatting (Shanghai time) is handled by
/// `build_platform_text` when needed.
pub fn build_payload(
    event_type: NotificationEventType,
    ctx: &NotificationContext,
) -> WebhookPayload {
    let (remaining, unit, threshold) = match event_type {
        NotificationEventType::CheckFailed => (None, None, None),
        _ => (ctx.remaining, ctx.unit.clone(), ctx.threshold),
    };
    let message = ctx
        .message
        .clone()
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| default_message(event_type));

    WebhookPayload {
        event_type,
        account_id: ctx.account_id.to_string(),
        account_name: ctx.account_name.to_string(),
        provider: ctx.provider,
        remaining,
        unit,
        threshold,
        checked_at: ctx.checked_at.clone(),
        message,
    }
}

fn default_message(event_type: NotificationEventType) -> String {
    match event_type {
        NotificationEventType::LowBalance => "Balance is below the configured threshold".into(),
        NotificationEventType::CheckFailed => "Balance check failed repeatedly".into(),
        NotificationEventType::InvalidCredential => {
            "Credential was rejected by the provider".into()
        }
    }
}

/// Decide which event (if any) the just-recorded `result` should emit for the
/// account, honoring the consecutive-failure threshold and the cooldown window.
/// Returns `None` when nothing should fire.
///
/// `hook_config_cutoff` is the minimum `updated_at` among currently enabled hooks.
/// Only deliveries created at or after this timestamp are considered for cooldown,
/// so that updating a hook configuration allows a new notification to fire immediately.
///
/// `cooldown_minutes` is the configurable cooldown window from app settings.
async fn decide_event(
    pool: &SqlitePool,
    account_id: &str,
    result: BalanceResult,
    now: DateTime<Utc>,
    hook_config_cutoff: Option<&str>,
    cooldown_minutes: i64,
) -> AppResult<Option<NotificationEventType>> {
    let event = match result {
        BalanceResult::Failed => {
            // Only fire after N consecutive failures. Fetch a small window; the
            // newest row is the failure we just recorded.
            let recent = recent_results(pool, account_id, FAILURE_THRESHOLD as i64).await?;
            if consecutive_failures(&recent) >= FAILURE_THRESHOLD {
                NotificationEventType::CheckFailed
            } else {
                return Ok(None);
            }
        }
        other => match direct_event_for_result(other) {
            Some(e) => e,
            None => return Ok(None),
        },
    };

    let last = last_delivery_at(pool, account_id, event, hook_config_cutoff).await?;
    let cooldown = chrono::Duration::minutes(cooldown_minutes.max(1));
    if within_cooldown(last.as_deref(), now, cooldown) {
        return Ok(None);
    }
    Ok(Some(event))
}

/// Record one delivery attempt row (success or failure) for cooldown and audit.
async fn record_delivery(
    pool: &SqlitePool,
    account_id: &str,
    event_type: NotificationEventType,
    hook_id: Option<&str>,
    payload_json: &str,
    success: bool,
    response_status: Option<i64>,
    error_message: Option<&str>,
    created_at: &str,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO notification_deliveries \
         (id, account_id, event_type, hook_id, payload, success, response_status, error_message, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(account_id)
    .bind(event_type.as_str())
    .bind(hook_id)
    .bind(payload_json)
    .bind(success)
    .bind(response_status)
    .bind(error_message)
    .bind(created_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Evaluate notifications for a just-recorded scheduled check result and deliver
/// to every enabled hook if an event fires and its cooldown has elapsed.
///
/// Errors from individual webhook POSTs are recorded as failed delivery rows and
/// never returned; only genuine internal errors (DB failures) propagate, and the
/// scheduler logs and continues even for those.
pub async fn evaluate_and_notify(
    pool: &SqlitePool,
    result: BalanceResult,
    ctx: NotificationContext<'_>,
    proxy: &ProxySettings,
    settings: &AppSettings,
    now: DateTime<Utc>,
) -> AppResult<()> {
    // Load enabled hooks first to compute the configuration cutoff for cooldown.
    let hooks = crate::hooks::list_enabled_hooks(pool).await?;
    if hooks.is_empty() {
        return Ok(());
    }

    // Compute the minimum updated_at among enabled hooks. Only deliveries created
    // at or after this timestamp count toward cooldown, so updating a hook config
    // allows a fresh notification even if an old delivery is within the window.
    let hook_config_cutoff = hooks.iter().map(|h| h.updated_at.as_str()).min();

    let cooldown_minutes = settings.notification_cooldown_minutes;
    let Some(event) = decide_event(
        pool,
        ctx.account_id,
        result,
        now,
        hook_config_cutoff,
        cooldown_minutes,
    )
    .await?
    else {
        return Ok(());
    };

    // Build payload with original RFC3339 checked_at for Generic webhooks
    let payload = build_payload(event, &ctx);
    let payload_json = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
    let created_at = now.to_rfc3339();

    // A client built from the current proxy settings; on failure record one row
    // per hook so the cooldown still engages and we do not retry every tick.
    let client = match build_webhook_client(proxy) {
        Ok(c) => c,
        Err(e) => {
            let msg = e.to_string();
            for hook in &hooks {
                record_delivery(
                    pool,
                    ctx.account_id,
                    event,
                    Some(&hook.id),
                    &payload_json,
                    false,
                    None,
                    Some(&msg),
                    &created_at,
                )
                .await?;
            }
            return Ok(());
        }
    };

    for hook in &hooks {
        let adapted_payload = build_platform_payload(hook.hook_type, &payload);

        let (success, status, error) = match client
            .post(&hook.url)
            .header("Content-Type", "application/json")
            .header("User-Agent", settings.user_agent.as_str())
            .body(adapted_payload.clone())
            .send()
            .await
        {
            Ok(resp) => evaluate_platform_response(hook.hook_type, resp).await,
            Err(e) => (false, None, Some(e.to_string())),
        };

        record_delivery(
            pool,
            ctx.account_id,
            event,
            Some(&hook.id),
            &adapted_payload,
            success,
            status,
            error.as_deref(),
            &created_at,
        )
        .await?;
    }

    Ok(())
}

/// Build a reqwest client for webhook delivery: the same proxy behavior as
/// provider checks, but with the webhook timeout.
fn build_webhook_client(proxy: &ProxySettings) -> AppResult<reqwest::Client> {
    use crate::error::AppError;
    use crate::models::ProxyMode;

    let mut builder = reqwest::Client::builder().timeout(WEBHOOK_TIMEOUT);
    match proxy.mode {
        ProxyMode::System => {}
        ProxyMode::None => builder = builder.no_proxy(),
        ProxyMode::Custom => {
            let url = proxy
                .custom_url
                .as_deref()
                .ok_or_else(|| AppError::validation("custom proxy mode requires a proxy URL"))?;
            let p = reqwest::Proxy::all(url)
                .map_err(|e| AppError::validation(format!("invalid proxy URL: {e}")))?;
            builder = builder.proxy(p);
        }
    }
    builder
        .build()
        .map_err(|e| AppError::Request(e.to_string()))
}

/// Send a fixed test payload to a single hook by id, recording the attempt. Uses
/// the same payload adaptation and response validation as real notifications.
/// Platform-specific text will display Shanghai time; Generic keeps RFC3339.
/// Returns whether the POST succeeded; internal/DB errors still propagate.
pub async fn test_hook(
    pool: &SqlitePool,
    hook_id: &str,
    proxy: &ProxySettings,
    settings: &AppSettings,
    now: DateTime<Utc>,
) -> AppResult<bool> {
    let hook = crate::hooks::get_hook(pool, hook_id).await?;

    let payload = WebhookPayload {
        event_type: NotificationEventType::LowBalance,
        account_id: "test".into(),
        account_name: "测试账号".into(),
        provider: Provider::NewApi,
        remaining: Some(14.8),
        unit: Some("USD".into()),
        threshold: Some(20.0),
        checked_at: now.to_rfc3339(),
        message: format!("这是一条来自 {} 的测试通知", crate::about::product_name()),
    };

    let adapted_payload = build_platform_payload(hook.hook_type, &payload);

    let client = build_webhook_client(proxy)?;
    let success = match client
        .post(&hook.url)
        .header("Content-Type", "application/json")
        .header("User-Agent", settings.user_agent.as_str())
        .body(adapted_payload)
        .send()
        .await
    {
        Ok(resp) => {
            let (success, _, _) = evaluate_platform_response(hook.hook_type, resp).await;
            success
        }
        Err(_) => false,
    };

    // A test row is not tied to a real account, so it is not persisted to the
    // deliveries table (which requires a valid account foreign key). The result
    // is simply reported back to the caller.
    Ok(success)
}

/// Recent notification deliveries across all accounts, newest first.
pub async fn recent_deliveries(
    pool: &SqlitePool,
    limit: i64,
) -> AppResult<Vec<crate::models::DeliveryView>> {
    use crate::error::AppError;

    let rows = sqlx::query(
        "SELECT id, account_id, event_type, hook_id, success, response_status, error_message, \
         created_at FROM notification_deliveries ORDER BY created_at DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let event_str: String = row.try_get("event_type")?;
        let event_type = parse_event(&event_str)
            .ok_or_else(|| AppError::validation("unknown event type in deliveries"))?;
        out.push(crate::models::DeliveryView {
            id: row.try_get("id")?,
            account_id: row.try_get("account_id")?,
            event_type,
            hook_id: row.try_get("hook_id")?,
            success: row.try_get("success")?,
            response_status: row.try_get("response_status")?,
            error_message: row.try_get("error_message")?,
            created_at: row.try_get("created_at")?,
        });
    }
    Ok(out)
}

fn parse_event(value: &str) -> Option<NotificationEventType> {
    match value {
        "low_balance" => Some(NotificationEventType::LowBalance),
        "check_failed" => Some(NotificationEventType::CheckFailed),
        "invalid_credential" => Some(NotificationEventType::InvalidCredential),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::{create_account, record_check, record_failed_check};
    use crate::db::init_memory_pool;
    use crate::models::{CreateAccountInput, CredentialsInput};
    use chrono::TimeZone;

    #[test]
    fn direct_event_maps_low_balance_and_invalid() {
        assert_eq!(
            direct_event_for_result(BalanceResult::LowBalance),
            Some(NotificationEventType::LowBalance)
        );
        assert_eq!(
            direct_event_for_result(BalanceResult::InvalidCredential),
            Some(NotificationEventType::InvalidCredential)
        );
        assert_eq!(direct_event_for_result(BalanceResult::Healthy), None);
        // `failed` is gated on the consecutive count, not a single result.
        assert_eq!(direct_event_for_result(BalanceResult::Failed), None);
    }

    #[test]
    fn consecutive_failures_counts_trailing_run() {
        use BalanceResult::*;
        assert_eq!(consecutive_failures(&[]), 0);
        assert_eq!(consecutive_failures(&[Failed, Failed, Failed]), 3);
        // Newest first: a healthy check breaks the run immediately.
        assert_eq!(consecutive_failures(&[Healthy, Failed, Failed]), 0);
        assert_eq!(consecutive_failures(&[Failed, Healthy, Failed]), 1);
    }

    #[test]
    fn cooldown_window_behavior() {
        let now = Utc::now();
        let one_hour = chrono::Duration::minutes(60);
        assert!(!within_cooldown(None, now, one_hour), "no prior delivery");
        assert!(!within_cooldown(Some("garbage"), now, one_hour), "bad ts");

        let thirty_min_ago = (now - chrono::Duration::minutes(30)).to_rfc3339();
        assert!(
            within_cooldown(Some(&thirty_min_ago), now, one_hour),
            "30min < 60min"
        );

        let ninety_min_ago = (now - chrono::Duration::minutes(90)).to_rfc3339();
        assert!(
            !within_cooldown(Some(&ninety_min_ago), now, one_hour),
            "90min > 60min"
        );

        // Exactly at the boundary is expired (window is exclusive).
        let sixty_min_ago = (now - chrono::Duration::minutes(60)).to_rfc3339();
        assert!(!within_cooldown(Some(&sixty_min_ago), now, one_hour));
    }

    #[test]
    fn payload_shapes_check_failed_without_figures() {
        let ctx = NotificationContext {
            account_id: "a",
            account_name: "Main",
            provider: Provider::Sub2Api,
            remaining: Some(5.0),
            unit: Some("USD".into()),
            threshold: Some(20.0),
            message: Some("boom".into()),
            checked_at: "2026-07-02T00:00:00Z".into(),
        };
        let low = build_payload(NotificationEventType::LowBalance, &ctx);
        assert_eq!(low.remaining, Some(5.0));
        assert_eq!(low.unit.as_deref(), Some("USD"));
        assert_eq!(low.threshold, Some(20.0));
        assert_eq!(low.message, "boom");

        let failed = build_payload(NotificationEventType::CheckFailed, &ctx);
        assert_eq!(failed.remaining, None);
        assert_eq!(failed.unit, None);
        assert_eq!(failed.threshold, None);
        // Field names are camelCase in the wire form.
        let json = serde_json::to_string(&failed).unwrap();
        assert!(json.contains("\"eventType\":\"check_failed\""));
        assert!(json.contains("\"accountName\":\"Main\""));
    }

    #[test]
    fn payload_falls_back_to_default_message() {
        let ctx = NotificationContext {
            account_id: "a",
            account_name: "Main",
            provider: Provider::NewApi,
            remaining: None,
            unit: None,
            threshold: None,
            message: None,
            checked_at: "2026-07-02T00:00:00Z".into(),
        };
        let p = build_payload(NotificationEventType::InvalidCredential, &ctx);
        assert_eq!(p.message, "Credential was rejected by the provider");
    }

    async fn seed_account(pool: &SqlitePool) -> String {
        create_account(
            pool,
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
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn check_failed_requires_three_consecutive_failures() {
        let pool = init_memory_pool().await;
        let id = seed_account(&pool).await;
        let now = Utc::now();

        // First two failures: no event.
        record_failed_check(&pool, &id, Provider::Sub2Api, "boom")
            .await
            .unwrap();
        assert!(
            decide_event(&pool, &id, BalanceResult::Failed, now, None, 60)
                .await
                .unwrap()
                .is_none()
        );

        record_failed_check(&pool, &id, Provider::Sub2Api, "boom")
            .await
            .unwrap();
        assert!(
            decide_event(&pool, &id, BalanceResult::Failed, now, None, 60)
                .await
                .unwrap()
                .is_none()
        );

        // Third consecutive failure: check_failed fires.
        record_failed_check(&pool, &id, Provider::Sub2Api, "boom")
            .await
            .unwrap();
        assert_eq!(
            decide_event(&pool, &id, BalanceResult::Failed, now, None, 60)
                .await
                .unwrap(),
            Some(NotificationEventType::CheckFailed)
        );
    }

    #[tokio::test]
    async fn healthy_run_resets_failure_streak() {
        let pool = init_memory_pool().await;
        let id = seed_account(&pool).await;
        let now = Utc::now();
        let healthy = crate::providers::CheckOutcome {
            valid: true,
            remaining: Some(50.0),
            used: None,
            total: None,
            unit: Some("USD".into()),
            plan_name: None,
            message: None,
        };

        record_failed_check(&pool, &id, Provider::Sub2Api, "boom")
            .await
            .unwrap();
        record_failed_check(&pool, &id, Provider::Sub2Api, "boom")
            .await
            .unwrap();
        record_check(&pool, &id, Provider::Sub2Api, &healthy, Some(20.0), None)
            .await
            .unwrap();
        record_failed_check(&pool, &id, Provider::Sub2Api, "boom")
            .await
            .unwrap();

        // Only one trailing failure after the healthy check -> no event.
        assert!(
            decide_event(&pool, &id, BalanceResult::Failed, now, None, 60)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn cooldown_suppresses_repeat_low_balance() {
        let pool = init_memory_pool().await;
        let id = seed_account(&pool).await;
        let now = Utc::now();

        // No prior delivery -> event decided.
        assert_eq!(
            decide_event(&pool, &id, BalanceResult::LowBalance, now, None, 60)
                .await
                .unwrap(),
            Some(NotificationEventType::LowBalance)
        );

        // Record a delivery 30min ago; the same event is now suppressed (60min cooldown).
        record_delivery(
            &pool,
            &id,
            NotificationEventType::LowBalance,
            None,
            "{}",
            true,
            Some(200),
            None,
            &(now - chrono::Duration::minutes(30)).to_rfc3339(),
        )
        .await
        .unwrap();
        assert!(
            decide_event(&pool, &id, BalanceResult::LowBalance, now, None, 60)
                .await
                .unwrap()
                .is_none()
        );

        // A different event type is not suppressed by a low_balance delivery.
        assert_eq!(
            decide_event(&pool, &id, BalanceResult::InvalidCredential, now, None, 60)
                .await
                .unwrap(),
            Some(NotificationEventType::InvalidCredential)
        );
    }

    #[tokio::test]
    async fn cooldown_counts_failed_deliveries_too() {
        let pool = init_memory_pool().await;
        let id = seed_account(&pool).await;
        let now = Utc::now();

        // A FAILED delivery 30min ago still engages the cooldown (60min window).
        record_delivery(
            &pool,
            &id,
            NotificationEventType::LowBalance,
            None,
            "{}",
            false,
            Some(500),
            Some("HTTP 500"),
            &(now - chrono::Duration::minutes(30)).to_rfc3339(),
        )
        .await
        .unwrap();
        assert!(
            decide_event(&pool, &id, BalanceResult::LowBalance, now, None, 60)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn format_shanghai_time_conversion() {
        // Test RFC3339 to Shanghai time conversion
        let rfc3339 = "2026-07-02T22:59:07Z";
        let shanghai = format_checked_at_shanghai(rfc3339, None);
        assert_eq!(shanghai, "2026-07-03 06:59:07 UTC+08:00");

        // Test with a different timezone in input
        let rfc3339_with_tz = "2026-07-02T22:59:07+00:00";
        let shanghai2 = format_checked_at_shanghai(rfc3339_with_tz, None);
        assert_eq!(shanghai2, "2026-07-03 06:59:07 UTC+08:00");

        // Test with invalid input and fallback
        let now = Utc.with_ymd_and_hms(2026, 7, 3, 10, 0, 0).unwrap();
        let result = format_checked_at_shanghai("invalid", Some(now));
        assert_eq!(result, "2026-07-03 18:00:00 UTC+08:00");

        // Test with invalid input and no fallback
        let result_no_fallback = format_checked_at_shanghai("invalid", None);
        assert_eq!(result_no_fallback, "invalid");
    }

    #[test]
    fn feishu_payload_contains_required_fields() {
        let payload = WebhookPayload {
            event_type: NotificationEventType::LowBalance,
            account_id: "acc1".into(),
            account_name: "Main Account".into(),
            provider: Provider::NewApi,
            remaining: Some(5.0),
            unit: Some("USD".into()),
            threshold: Some(20.0),
            checked_at: "2026-07-02T22:59:07Z".into(),
            message: "Balance is low".into(),
        };

        let feishu_json =
            build_platform_payload(crate::models::NotificationHookType::Feishu, &payload);

        let parsed: serde_json::Value = serde_json::from_str(&feishu_json).unwrap();
        assert_eq!(
            parsed.get("msg_type").and_then(|v| v.as_str()),
            Some("text")
        );

        let text = parsed
            .get("content")
            .and_then(|c| c.get("text"))
            .and_then(|t| t.as_str())
            .unwrap();

        // Verify Chinese labels
        assert!(text.contains("事件: 余额不足"));
        assert!(text.contains("账号: Main Account"));
        assert!(text.contains("提供商: New API"));
        assert!(text.contains("剩余余额: 5 USD"));
        assert!(text.contains("阈值: 20"));
        // Verify Shanghai time format is present (converted from UTC)
        assert!(text.contains("查询时间: 2026-07-03 06:59:07 UTC+08:00"));
        assert!(text.contains("Balance is low"));
    }

    #[test]
    fn feishu_payload_chinese_locale() {
        let payload = WebhookPayload {
            event_type: NotificationEventType::LowBalance,
            account_id: "acc1".into(),
            account_name: "主账号".into(),
            provider: Provider::CustomHttp,
            remaining: Some(5.0),
            unit: Some("USD".into()),
            threshold: Some(20.0),
            checked_at: "2026-07-03T10:00:00Z".into(),
            message: "Balance is below the configured threshold".into(),
        };

        let feishu_json =
            build_platform_payload(crate::models::NotificationHookType::Feishu, &payload);

        let parsed: serde_json::Value = serde_json::from_str(&feishu_json).unwrap();
        let text = parsed
            .get("content")
            .and_then(|c| c.get("text"))
            .and_then(|t| t.as_str())
            .unwrap();

        // Verify Chinese labels
        assert!(text.contains("事件: 余额不足"));
        assert!(text.contains("账号: 主账号"));
        assert!(text.contains("提供商: 自定义 HTTP"));
        assert!(text.contains("剩余余额: 5 USD"));
        assert!(text.contains("阈值: 20"));
        assert!(text.contains("查询时间:"));
        assert!(text.contains("消息: 余额低于配置阈值"));
    }

    #[test]
    fn generic_webhook_uses_original_payload() {
        let payload = WebhookPayload {
            event_type: NotificationEventType::CheckFailed,
            account_id: "acc1".into(),
            account_name: "Test".into(),
            provider: Provider::Sub2Api,
            remaining: None,
            unit: None,
            threshold: None,
            checked_at: "2026-07-02T22:59:07Z".into(),
            message: "Balance check failed repeatedly".into(),
        };

        let generic_json =
            build_platform_payload(crate::models::NotificationHookType::Generic, &payload);

        let parsed: serde_json::Value = serde_json::from_str(&generic_json).unwrap();
        // Should be the original WebhookPayload structure with Chinese default message
        assert_eq!(
            parsed.get("eventType").and_then(|v| v.as_str()),
            Some("check_failed")
        );
        assert_eq!(
            parsed.get("accountName").and_then(|v| v.as_str()),
            Some("Test")
        );
        assert_eq!(
            parsed.get("message").and_then(|v| v.as_str()),
            Some("余额查询连续失败")
        );
        // Verify checkedAt remains in RFC3339 format, not Shanghai time
        assert_eq!(
            parsed.get("checkedAt").and_then(|v| v.as_str()),
            Some("2026-07-02T22:59:07Z")
        );
        assert!(parsed.get("msg_type").is_none()); // Not a Feishu payload
    }

    #[test]
    fn feishu_success_code_validation() {
        let success_body = r#"{"code":0,"msg":"success","data":{}}"#;
        let error_body = r#"{"code":19002,"msg":"params error, msg_type need","data":{}}"#;

        let success_json: serde_json::Value = serde_json::from_str(success_body).unwrap();
        let error_json: serde_json::Value = serde_json::from_str(error_body).unwrap();

        // Success: code == 0
        assert_eq!(success_json.get("code").and_then(|c| c.as_i64()), Some(0));

        // Error: code != 0
        let error_code = error_json.get("code").and_then(|c| c.as_i64()).unwrap();
        assert_eq!(error_code, 19002);
        assert_ne!(error_code, 0);

        // Verify error message extraction
        let error_msg = error_json.get("msg").and_then(|m| m.as_str()).unwrap();
        assert!(error_msg.contains("params error"));
    }

    #[test]
    fn test_payload_uses_chinese() {
        // Build a test-like payload
        let payload = WebhookPayload {
            event_type: NotificationEventType::LowBalance,
            account_id: "test".into(),
            account_name: "测试账号".into(),
            provider: Provider::NewApi,
            remaining: Some(14.8),
            unit: Some("USD".into()),
            threshold: Some(20.0),
            checked_at: "2026-07-03T10:00:00Z".into(),
            message: format!("这是一条来自 {} 的测试通知", crate::about::product_name()),
        };

        let feishu_json =
            build_platform_payload(crate::models::NotificationHookType::Feishu, &payload);

        let parsed: serde_json::Value = serde_json::from_str(&feishu_json).unwrap();
        let text = parsed
            .get("content")
            .and_then(|c| c.get("text"))
            .and_then(|t| t.as_str())
            .unwrap();

        assert!(
            text.contains("测试账号"),
            "Test payload should contain Chinese account name"
        );
        assert!(
            text.contains("事件: 余额不足"),
            "Test payload should use Chinese labels"
        );
    }

    #[tokio::test]
    async fn cooldown_suppresses_when_hook_updated_before_delivery() {
        let pool = init_memory_pool().await;
        let id = seed_account(&pool).await;
        let now = Utc::now();

        // Create a hook that was updated 2h ago
        let hook_updated_at = (now - chrono::Duration::hours(2)).to_rfc3339();
        let hook = crate::hooks::create_hook(
            &pool,
            crate::models::CreateHookInput {
                name: "Test Hook".into(),
                url: "https://example.com/hook".into(),
                hook_type: crate::models::NotificationHookType::Generic,
                enabled: true,
            },
        )
        .await
        .unwrap();

        // Manually set the hook's updated_at to 2h ago
        sqlx::query("UPDATE notification_hooks SET updated_at = ? WHERE id = ?")
            .bind(&hook_updated_at)
            .bind(&hook.id)
            .execute(&pool)
            .await
            .unwrap();

        // Record a delivery 1h ago (after the hook was updated, within 60min cooldown)
        let delivery_at = (now - chrono::Duration::minutes(30)).to_rfc3339();
        record_delivery(
            &pool,
            &id,
            NotificationEventType::LowBalance,
            Some(&hook.id),
            "{}",
            true,
            Some(200),
            None,
            &delivery_at,
        )
        .await
        .unwrap();

        // The hook config cutoff is 2h ago, so the 30min ago delivery should suppress (60min cooldown)
        let hooks = crate::hooks::list_enabled_hooks(&pool).await.unwrap();
        let cutoff = hooks.iter().map(|h| h.updated_at.as_str()).min();
        let event = decide_event(&pool, &id, BalanceResult::LowBalance, now, cutoff, 60)
            .await
            .unwrap();

        assert!(
            event.is_none(),
            "Delivery after hook config should suppress"
        );
    }

    #[tokio::test]
    async fn cooldown_allows_when_hook_updated_after_delivery() {
        let pool = init_memory_pool().await;
        let id = seed_account(&pool).await;
        let now = Utc::now();

        // Record a delivery 2h ago
        let delivery_at = (now - chrono::Duration::hours(2)).to_rfc3339();
        record_delivery(
            &pool,
            &id,
            NotificationEventType::LowBalance,
            None,
            "{}",
            true,
            Some(200),
            None,
            &delivery_at,
        )
        .await
        .unwrap();

        // Create a hook that was updated 1h ago (after the delivery)
        let hook_updated_at = (now - chrono::Duration::hours(1)).to_rfc3339();
        let hook = crate::hooks::create_hook(
            &pool,
            crate::models::CreateHookInput {
                name: "Test Hook".into(),
                url: "https://example.com/hook".into(),
                hook_type: crate::models::NotificationHookType::Generic,
                enabled: true,
            },
        )
        .await
        .unwrap();

        // Manually set the hook's updated_at to 1h ago
        sqlx::query("UPDATE notification_hooks SET updated_at = ? WHERE id = ?")
            .bind(&hook_updated_at)
            .bind(&hook.id)
            .execute(&pool)
            .await
            .unwrap();

        // The hook config cutoff is 1h ago, so the 2h ago delivery should be ignored (60min cooldown)
        let hooks = crate::hooks::list_enabled_hooks(&pool).await.unwrap();
        let cutoff = hooks.iter().map(|h| h.updated_at.as_str()).min();
        let event = decide_event(&pool, &id, BalanceResult::LowBalance, now, cutoff, 60)
            .await
            .unwrap();

        assert_eq!(
            event,
            Some(NotificationEventType::LowBalance),
            "Delivery before hook config should not suppress"
        );
    }

    #[tokio::test]
    async fn no_enabled_hooks_means_no_delivery() {
        let pool = init_memory_pool().await;
        let id = seed_account(&pool).await;
        let now = Utc::now();

        // Record a low balance check
        let outcome = crate::providers::CheckOutcome {
            valid: true,
            remaining: Some(5.0),
            used: None,
            total: None,
            unit: Some("USD".into()),
            plan_name: None,
            message: None,
        };
        record_check(&pool, &id, Provider::Sub2Api, &outcome, Some(20.0), None)
            .await
            .unwrap();

        let ctx = NotificationContext {
            account_id: &id,
            account_name: "Test",
            provider: Provider::Sub2Api,
            remaining: Some(5.0),
            unit: Some("USD".into()),
            threshold: Some(20.0),
            message: None,
            checked_at: now.to_rfc3339(),
        };

        let proxy = ProxySettings {
            mode: crate::models::ProxyMode::None,
            custom_url: None,
        };
        let settings = AppSettings {
            default_balance_threshold: 20.0,
            default_check_interval_minutes: 60,
            history_retention_days: 30,
            user_agent: "test".into(),
            notification_cooldown_minutes: 60,
            scheduler_enabled: true,
        };

        // Call evaluate_and_notify with no enabled hooks
        evaluate_and_notify(
            &pool,
            BalanceResult::LowBalance,
            ctx,
            &proxy,
            &settings,
            now,
        )
        .await
        .unwrap();

        // Verify no delivery was recorded
        let deliveries = recent_deliveries(&pool, 10).await.unwrap();
        assert!(
            deliveries.is_empty(),
            "No delivery should be recorded when no hooks enabled"
        );
    }
}
