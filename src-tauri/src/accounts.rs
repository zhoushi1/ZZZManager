use chrono::Utc;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{
    validate_custom_http_adapter, validate_official_url, AccountView, BalanceResult,
    CreateAccountInput, Credentials, CredentialsInput, CustomHttpAdapterConfig, HistoryEntry,
    HistoryQuery, Provider, UpdateAccountInput,
};
use crate::providers::CheckOutcome;

/// Normalize a base URL: trim surrounding whitespace and remove any trailing
/// slashes. Provider adapters append their own fixed paths.
pub fn normalize_base_url(raw: &str) -> String {
    raw.trim().trim_end_matches('/').to_string()
}

/// Validate the optional provider-credit conversion rate.
pub fn validate_usd_credits_per_cny(value: Option<f64>) -> AppResult<Option<f64>> {
    if let Some(rate) = value {
        if !rate.is_finite() || rate <= 0.0 {
            return Err(AppError::validation(
                "USD credits per CNY must be greater than zero",
            ));
        }
    }
    Ok(value)
}

/// Convert a provider-reported USD credit amount to its CNY purchase value.
/// Returns `None` when conversion is not configured or the provider unit is not USD.
pub fn convert_usd_credits_to_cny(
    value: Option<f64>,
    unit: Option<&str>,
    usd_credits_per_cny: Option<f64>,
) -> Option<f64> {
    let value = value?;
    let rate = usd_credits_per_cny?;
    if !rate.is_finite()
        || rate <= 0.0
        || !unit.is_some_and(|unit| unit.eq_ignore_ascii_case("USD"))
    {
        return None;
    }
    Some(value / rate)
}

/// Deterministic fingerprint of the credentials used for duplicate detection.
/// Never stores or exposes the raw secret values.
pub fn credential_fingerprint(credentials: &Credentials) -> String {
    let mut hasher = Sha256::new();
    match credentials {
        Credentials::NewApi {
            access_token,
            user_id,
        } => {
            hasher.update(b"new_api\0");
            hasher.update(user_id.as_bytes());
            hasher.update(b"\0");
            hasher.update(access_token.as_bytes());
        }
        Credentials::Sub2Api { api_key } => {
            hasher.update(b"sub2api\0");
            hasher.update(api_key.as_bytes());
        }
        Credentials::CustomHttp { api_key } => {
            hasher.update(b"custom_http\0");
            hasher.update(api_key.as_bytes());
        }
    }
    format!("{:x}", hasher.finalize())
}

/// Resolve required credentials from input for a create operation.
fn resolve_new_credentials(provider: Provider, input: &CredentialsInput) -> AppResult<Credentials> {
    match provider {
        Provider::NewApi => {
            let access_token = non_empty(&input.access_token)
                .ok_or_else(|| AppError::validation("New API requires an access token"))?;
            let user_id = non_empty(&input.user_id)
                .ok_or_else(|| AppError::validation("New API requires a user ID"))?;
            Ok(Credentials::NewApi {
                access_token,
                user_id,
            })
        }
        Provider::Sub2Api => {
            let api_key = non_empty(&input.api_key)
                .ok_or_else(|| AppError::validation("Sub2API requires an API key"))?;
            Ok(Credentials::Sub2Api { api_key })
        }
        Provider::CustomHttp => {
            let api_key = non_empty(&input.api_key)
                .ok_or_else(|| AppError::validation("Custom HTTP requires an API key"))?;
            Ok(Credentials::CustomHttp { api_key })
        }
    }
}

fn non_empty(value: &Option<String>) -> Option<String> {
    value
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Raw row from `gateway_accounts`, including credential columns for internal use.
#[derive(sqlx::FromRow)]
struct AccountRow {
    id: String,
    name: String,
    provider: String,
    base_url: String,
    enabled: bool,
    balance_threshold: Option<f64>,
    usd_credits_per_cny: Option<f64>,
    check_interval_minutes: Option<i64>,
    official_url: Option<String>,
    note: Option<String>,
    sort_order: i64,
    access_token: Option<String>,
    user_id: Option<String>,
    api_key: Option<String>,
    custom_adapter_config: Option<String>,
    // Selected so `AccountRow` maps the full column list; presence/absence is
    // reflected through `has_credentials`, so the raw value is not read directly.
    #[allow(dead_code)]
    credential_fingerprint: Option<String>,
    last_result: Option<String>,
    last_remaining: Option<f64>,
    last_used: Option<f64>,
    last_total: Option<f64>,
    last_unit: Option<String>,
    last_plan_name: Option<String>,
    last_message: Option<String>,
    last_checked_at: Option<String>,
    created_at: String,
    updated_at: String,
}

impl AccountRow {
    fn provider(&self) -> AppResult<Provider> {
        Provider::from_str(&self.provider)
            .ok_or_else(|| AppError::validation("unknown provider stored for account"))
    }

    fn credentials(&self) -> AppResult<Credentials> {
        match self.provider()? {
            Provider::NewApi => Ok(Credentials::NewApi {
                access_token: self.access_token.clone().unwrap_or_default(),
                user_id: self.user_id.clone().unwrap_or_default(),
            }),
            Provider::Sub2Api => Ok(Credentials::Sub2Api {
                api_key: self.api_key.clone().unwrap_or_default(),
            }),
            Provider::CustomHttp => Ok(Credentials::CustomHttp {
                api_key: self.api_key.clone().unwrap_or_default(),
            }),
        }
    }

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

    fn custom_adapter(&self) -> AppResult<Option<CustomHttpAdapterConfig>> {
        let Some(raw) = self.custom_adapter_config.as_deref() else {
            return Ok(None);
        };
        serde_json::from_str(raw)
            .map(Some)
            .map_err(|e| AppError::validation(format!("invalid stored custom adapter: {e}")))
    }

    fn into_view(self) -> AppResult<AccountView> {
        let provider = self.provider()?;
        let has_credentials = self.has_credentials();
        let custom_adapter = self.custom_adapter()?;
        let last_result = self.last_result.as_deref().and_then(parse_result);
        Ok(AccountView {
            id: self.id,
            name: self.name,
            provider,
            base_url: self.base_url,
            enabled: self.enabled,
            balance_threshold: self.balance_threshold,
            usd_credits_per_cny: self.usd_credits_per_cny,
            check_interval_minutes: self.check_interval_minutes,
            official_url: self.official_url,
            note: self.note,
            sort_order: self.sort_order,
            custom_adapter,
            has_credentials,
            last_result,
            last_remaining: self.last_remaining,
            last_used: self.last_used,
            last_total: self.last_total,
            last_unit: self.last_unit,
            last_plan_name: self.last_plan_name,
            last_message: self.last_message,
            last_checked_at: self.last_checked_at,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
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

const ACCOUNT_COLUMNS: &str = "id, name, provider, base_url, enabled, balance_threshold, \
    usd_credits_per_cny, check_interval_minutes, official_url, note, sort_order, access_token, user_id, api_key, custom_adapter_config, credential_fingerprint, \
    last_result, last_remaining, last_used, last_total, last_unit, last_plan_name, \
    last_message, last_checked_at, created_at, updated_at";

async fn fetch_row(pool: &SqlitePool, id: &str) -> AppResult<AccountRow> {
    let query = format!("SELECT {ACCOUNT_COLUMNS} FROM gateway_accounts WHERE id = ?");
    sqlx::query_as::<_, AccountRow>(&query)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)
}

/// List all accounts, newest first. Never includes raw credentials.
pub async fn list_accounts(pool: &SqlitePool) -> AppResult<Vec<AccountView>> {
    let query = format!(
        "SELECT {ACCOUNT_COLUMNS} FROM gateway_accounts ORDER BY sort_order DESC, created_at DESC"
    );
    let rows = sqlx::query_as::<_, AccountRow>(&query)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(AccountRow::into_view).collect()
}

/// Fetch a single account view by id.
pub async fn get_account(pool: &SqlitePool, id: &str) -> AppResult<AccountView> {
    fetch_row(pool, id).await?.into_view()
}

/// Create a new account, rejecting duplicates by
/// provider + normalized base URL + credential fingerprint.
pub async fn create_account(
    pool: &SqlitePool,
    input: CreateAccountInput,
) -> AppResult<AccountView> {
    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::validation("Name is required"));
    }
    let base_url = normalize_base_url(&input.base_url);
    if base_url.is_empty() {
        return Err(AppError::validation("Base URL is required"));
    }
    let credentials = resolve_new_credentials(input.provider, &input.credentials)?;
    let fingerprint = credential_fingerprint(&credentials);
    let usd_credits_per_cny = validate_usd_credits_per_cny(input.usd_credits_per_cny)?;
    let official_url = validate_official_url(input.official_url.as_deref())?;
    let note = input
        .note
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let custom_adapter = resolve_create_custom_adapter(input.provider, input.custom_adapter)?;
    let custom_adapter_json = serialize_custom_adapter(custom_adapter.as_ref())?;

    // Determine sort_order: if provided and valid, use it; otherwise default to max + 1
    let sort_order = match input.sort_order {
        Some(order) => order,
        None => {
            let max_order: Option<i64> =
                sqlx::query_scalar("SELECT MAX(sort_order) FROM gateway_accounts")
                    .fetch_optional(pool)
                    .await?
                    .flatten();
            max_order.unwrap_or(0) + 1
        }
    };

    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let (access_token, user_id, api_key) = split_credentials(&credentials);

    let result = sqlx::query(
        "INSERT INTO gateway_accounts \
         (id, name, provider, base_url, enabled, balance_threshold, usd_credits_per_cny, check_interval_minutes, \
          official_url, note, sort_order, access_token, user_id, api_key, custom_adapter_config, credential_fingerprint, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&name)
    .bind(input.provider.as_str())
    .bind(&base_url)
    .bind(input.enabled)
    .bind(input.balance_threshold)
    .bind(usd_credits_per_cny)
    .bind(input.check_interval_minutes)
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
    .await;

    map_insert_result(result)?;
    get_account(pool, &id).await
}

/// Update an existing account. Credentials left empty are preserved.
pub async fn update_account(
    pool: &SqlitePool,
    id: &str,
    input: UpdateAccountInput,
) -> AppResult<AccountView> {
    let existing = fetch_row(pool, id).await?;
    let provider = existing.provider()?;

    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::validation("Name is required"));
    }
    let base_url = normalize_base_url(&input.base_url);
    if base_url.is_empty() {
        return Err(AppError::validation("Base URL is required"));
    }

    // Merge submitted credentials over existing ones, then re-validate.
    let credentials = merge_credentials(provider, &existing, &input.credentials)?;
    let fingerprint = credential_fingerprint(&credentials);
    let (access_token, user_id, api_key) = split_credentials(&credentials);
    let usd_credits_per_cny = validate_usd_credits_per_cny(input.usd_credits_per_cny)?;
    let official_url = validate_official_url(input.official_url.as_deref())?;
    let note = input
        .note
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let sort_order = input.sort_order.unwrap_or(existing.sort_order);
    let custom_adapter =
        resolve_update_custom_adapter(provider, existing.custom_adapter()?, input.custom_adapter)?;
    let custom_adapter_json = serialize_custom_adapter(custom_adapter.as_ref())?;
    let now = Utc::now().to_rfc3339();

    let result = sqlx::query(
        "UPDATE gateway_accounts SET name = ?, base_url = ?, enabled = ?, \
         balance_threshold = ?, usd_credits_per_cny = ?, check_interval_minutes = ?, official_url = ?, note = ?, sort_order = ?, access_token = ?, user_id = ?, \
         api_key = ?, custom_adapter_config = ?, credential_fingerprint = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&name)
    .bind(&base_url)
    .bind(input.enabled)
    .bind(input.balance_threshold)
    .bind(usd_credits_per_cny)
    .bind(input.check_interval_minutes)
    .bind(&official_url)
    .bind(&note)
    .bind(sort_order)
    .bind(&access_token)
    .bind(&user_id)
    .bind(&api_key)
    .bind(&custom_adapter_json)
    .bind(&fingerprint)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await;

    map_insert_result(result)?;
    get_account(pool, id).await
}

/// Delete an account by id.
pub async fn delete_account(pool: &SqlitePool, id: &str) -> AppResult<()> {
    let affected = sqlx::query("DELETE FROM gateway_accounts WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    if affected == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Reorder accounts by assigning new sort_order values based on the provided order.
/// The first ID in the list gets the highest sort_order (equal to list length),
/// decreasing by 1 for each subsequent ID.
/// Returns the updated list of all accounts in the new sort order.
pub async fn reorder_accounts(
    pool: &SqlitePool,
    ordered_ids: Vec<String>,
) -> AppResult<Vec<AccountView>> {
    if ordered_ids.is_empty() {
        return list_accounts(pool).await;
    }

    let mut tx = pool.begin().await?;
    let now = Utc::now().to_rfc3339();
    let list_length = ordered_ids.len() as i64;

    for (index, id) in ordered_ids.iter().enumerate() {
        let new_order = list_length - (index as i64);
        let affected =
            sqlx::query("UPDATE gateway_accounts SET sort_order = ?, updated_at = ? WHERE id = ?")
                .bind(new_order)
                .bind(&now)
                .bind(id)
                .execute(&mut *tx)
                .await?
                .rows_affected();

        if affected == 0 {
            return Err(AppError::NotFound);
        }
    }

    tx.commit().await?;
    list_accounts(pool).await
}

/// Get account credentials for editing. Only called on-demand by the edit form.
/// Returns NotFound if the account doesn't exist.
pub async fn get_account_credentials(
    pool: &SqlitePool,
    id: &str,
) -> AppResult<crate::models::AccountCredentialsView> {
    use crate::models::AccountCredentialsView;

    let row = fetch_row(pool, id).await?;
    let provider = row.provider()?;

    Ok(AccountCredentialsView {
        provider,
        access_token: row.access_token,
        user_id: row.user_id,
        api_key: row.api_key,
    })
}

fn merge_credentials(
    provider: Provider,
    existing: &AccountRow,
    input: &CredentialsInput,
) -> AppResult<Credentials> {
    match provider {
        Provider::NewApi => {
            let access_token = non_empty(&input.access_token)
                .or_else(|| existing.access_token.clone())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| AppError::validation("New API requires an access token"))?;
            let user_id = non_empty(&input.user_id)
                .or_else(|| existing.user_id.clone())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| AppError::validation("New API requires a user ID"))?;
            Ok(Credentials::NewApi {
                access_token,
                user_id,
            })
        }
        Provider::Sub2Api => {
            let api_key = non_empty(&input.api_key)
                .or_else(|| existing.api_key.clone())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| AppError::validation("Sub2API requires an API key"))?;
            Ok(Credentials::Sub2Api { api_key })
        }
        Provider::CustomHttp => {
            let api_key = non_empty(&input.api_key)
                .or_else(|| existing.api_key.clone())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| AppError::validation("Custom HTTP requires an API key"))?;
            Ok(Credentials::CustomHttp { api_key })
        }
    }
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

fn resolve_create_custom_adapter(
    provider: Provider,
    input: Option<CustomHttpAdapterConfig>,
) -> AppResult<Option<CustomHttpAdapterConfig>> {
    match provider {
        Provider::CustomHttp => {
            let config = input
                .ok_or_else(|| AppError::validation("Custom HTTP requires an adapter config"))?;
            validate_custom_http_adapter(config).map(Some)
        }
        Provider::NewApi | Provider::Sub2Api => Ok(None),
    }
}

fn resolve_update_custom_adapter(
    provider: Provider,
    existing: Option<CustomHttpAdapterConfig>,
    input: Option<CustomHttpAdapterConfig>,
) -> AppResult<Option<CustomHttpAdapterConfig>> {
    match provider {
        Provider::CustomHttp => {
            let config = input
                .or(existing)
                .ok_or_else(|| AppError::validation("Custom HTTP requires an adapter config"))?;
            validate_custom_http_adapter(config).map(Some)
        }
        Provider::NewApi | Provider::Sub2Api => Ok(None),
    }
}

fn serialize_custom_adapter(config: Option<&CustomHttpAdapterConfig>) -> AppResult<Option<String>> {
    config
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| AppError::validation(format!("invalid custom adapter config: {e}")))
}

fn map_insert_result(
    result: Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error>,
) -> AppResult<()> {
    match result {
        Ok(_) => Ok(()),
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => Err(AppError::Duplicate),
        Err(e) => Err(AppError::Database(e)),
    }
}

/// Record a balance check outcome: write a history row and update the account's
/// latest-result snapshot. Returns the recorded result category.
pub async fn record_check(
    pool: &SqlitePool,
    id: &str,
    provider: Provider,
    outcome: &CheckOutcome,
    threshold: Option<f64>,
    usd_credits_per_cny: Option<f64>,
) -> AppResult<BalanceResult> {
    let result = classify_outcome(outcome, threshold, usd_credits_per_cny);
    let now = Utc::now().to_rfc3339();
    let history_id = Uuid::new_v4().to_string();

    // History row. `notified` stays 0: notifications are out of scope here.
    sqlx::query(
        "INSERT INTO balance_check_history \
         (id, account_id, provider, result, remaining, used, total, unit, plan_name, message, \
          notified, checked_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?)",
    )
    .bind(&history_id)
    .bind(id)
    .bind(provider.as_str())
    .bind(result.as_str())
    .bind(outcome.remaining)
    .bind(outcome.used)
    .bind(outcome.total)
    .bind(&outcome.unit)
    .bind(&outcome.plan_name)
    .bind(&outcome.message)
    .bind(&now)
    .execute(pool)
    .await?;

    // Latest snapshot on the account for the UI.
    sqlx::query(
        "UPDATE gateway_accounts SET last_result = ?, last_remaining = ?, last_used = ?, \
         last_total = ?, last_unit = ?, last_plan_name = ?, last_message = ?, \
         last_checked_at = ? WHERE id = ?",
    )
    .bind(result.as_str())
    .bind(outcome.remaining)
    .bind(outcome.used)
    .bind(outcome.total)
    .bind(&outcome.unit)
    .bind(&outcome.plan_name)
    .bind(&outcome.message)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await?;

    Ok(result)
}

/// Record a request/transport failure: write a `failed` history row and update
/// the account's latest snapshot to `failed`. Unlike [`record_check`], no
/// balance figures are available, so remaining/used/total/unit/plan_name are all
/// stored as `NULL` and only the readable error `message` is kept.
///
/// This is distinct from [`BalanceResult::InvalidCredential`], which is reserved
/// for reachable providers that reject the credential (HTTP non-success or an
/// invalid JSON body); a `failed` result means the check could not be completed.
pub async fn record_failed_check(
    pool: &SqlitePool,
    id: &str,
    provider: Provider,
    message: &str,
) -> AppResult<BalanceResult> {
    let now = Utc::now().to_rfc3339();
    let history_id = Uuid::new_v4().to_string();
    let result = BalanceResult::Failed;

    sqlx::query(
        "INSERT INTO balance_check_history \
         (id, account_id, provider, result, remaining, used, total, unit, plan_name, message, \
          notified, checked_at) VALUES (?, ?, ?, ?, NULL, NULL, NULL, NULL, NULL, ?, 0, ?)",
    )
    .bind(&history_id)
    .bind(id)
    .bind(provider.as_str())
    .bind(result.as_str())
    .bind(message)
    .bind(&now)
    .execute(pool)
    .await?;

    sqlx::query(
        "UPDATE gateway_accounts SET last_result = ?, last_remaining = NULL, last_used = NULL, \
         last_total = NULL, last_unit = NULL, last_plan_name = NULL, last_message = ?, \
         last_checked_at = ? WHERE id = ?",
    )
    .bind(result.as_str())
    .bind(message)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await?;

    Ok(result)
}

/// Map a check outcome plus its effective threshold into a result category.
/// When a credit conversion rate is configured, the threshold and comparison
/// both use CNY purchase value while stored provider balances remain unchanged.
pub fn classify_outcome(
    outcome: &CheckOutcome,
    threshold: Option<f64>,
    usd_credits_per_cny: Option<f64>,
) -> BalanceResult {
    if !outcome.valid {
        return BalanceResult::InvalidCredential;
    }
    let remaining = convert_usd_credits_to_cny(
        outcome.remaining,
        outcome.unit.as_deref(),
        usd_credits_per_cny,
    )
    .or(outcome.remaining);
    match (threshold, remaining) {
        (Some(threshold), Some(remaining)) if remaining < threshold => BalanceResult::LowBalance,
        _ => BalanceResult::Healthy,
    }
}

/// Fetch recent balance check history across all accounts, newest first.
/// Thin wrapper over [`query_history`] with no filters, kept for the existing
/// `recent_checks` command and internal callers.
pub async fn recent_history(pool: &SqlitePool, limit: i64) -> AppResult<Vec<HistoryEntry>> {
    query_history(
        pool,
        &HistoryQuery {
            limit: Some(limit),
            ..Default::default()
        },
    )
    .await
}

/// Largest number of history rows a single query may return.
const HISTORY_LIMIT_MAX: i64 = 500;
/// Default number of history rows when the caller does not specify a limit.
const HISTORY_LIMIT_DEFAULT: i64 = 100;

/// Clamp a requested history limit into `1..=HISTORY_LIMIT_MAX`, defaulting when
/// absent or non-positive.
fn clamp_history_limit(requested: Option<i64>) -> i64 {
    match requested {
        Some(n) if n >= 1 => n.min(HISTORY_LIMIT_MAX),
        _ => HISTORY_LIMIT_DEFAULT,
    }
}

/// Query balance check history with optional filters, newest first. Each row is
/// joined to its owning account for the display name. Filters combine with AND;
/// the limit is clamped to `1..=HISTORY_LIMIT_MAX`.
pub async fn query_history(
    pool: &SqlitePool,
    filter: &HistoryQuery,
) -> AppResult<Vec<HistoryEntry>> {
    let limit = clamp_history_limit(filter.limit);

    // Build the WHERE clause dynamically from the present filters. Bind order
    // below must match the order these predicates are appended.
    let mut where_clauses: Vec<&str> = Vec::new();
    if filter.account_id.is_some() {
        where_clauses.push("h.account_id = ?");
    }
    if filter.result.is_some() {
        where_clauses.push("h.result = ?");
    }
    if filter.provider.is_some() {
        where_clauses.push("h.provider = ?");
    }
    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", where_clauses.join(" AND "))
    };

    let sql = format!(
        "SELECT h.id, h.account_id, a.name AS account_name, a.usd_credits_per_cny, h.provider, h.result, \
         h.remaining, h.used, h.total, h.unit, h.plan_name, h.message, h.checked_at \
         FROM balance_check_history h \
         LEFT JOIN gateway_accounts a ON a.id = h.account_id \
         {where_sql} ORDER BY h.checked_at DESC LIMIT ?"
    );

    let mut query = sqlx::query(&sql);
    if let Some(account_id) = &filter.account_id {
        query = query.bind(account_id);
    }
    if let Some(result) = filter.result {
        query = query.bind(result.as_str());
    }
    if let Some(provider) = filter.provider {
        query = query.bind(provider.as_str());
    }
    query = query.bind(limit);

    let rows = query.fetch_all(pool).await?;

    let mut entries = Vec::with_capacity(rows.len());
    for row in rows {
        let provider_str: String = row.try_get("provider")?;
        let result_str: String = row.try_get("result")?;
        let provider = Provider::from_str(&provider_str)
            .ok_or_else(|| AppError::validation("unknown provider in history"))?;
        let result = parse_result(&result_str)
            .ok_or_else(|| AppError::validation("unknown result in history"))?;
        entries.push(HistoryEntry {
            id: row.try_get("id")?,
            account_id: row.try_get("account_id")?,
            account_name: row.try_get("account_name")?,
            usd_credits_per_cny: row.try_get("usd_credits_per_cny")?,
            provider,
            result,
            remaining: row.try_get("remaining")?,
            used: row.try_get("used")?,
            total: row.try_get("total")?,
            unit: row.try_get("unit")?,
            plan_name: row.try_get("plan_name")?,
            message: row.try_get("message")?,
            checked_at: row.try_get("checked_at")?,
        });
    }
    Ok(entries)
}

/// Internal accessor used by the check command to load credentials.
///
/// Returns a clear validation error for a credentialless placeholder rather than
/// handing back empty credentials (which would produce a confusing provider-side
/// failure or, worse, a panic downstream). Callers should surface this message.
pub async fn load_credentials(
    pool: &SqlitePool,
    id: &str,
) -> AppResult<(
    Provider,
    String,
    Credentials,
    Option<f64>,
    Option<f64>,
    Option<CustomHttpAdapterConfig>,
)> {
    let row = fetch_row(pool, id).await?;
    if !row.has_credentials() {
        return Err(AppError::validation(
            "This account has no credentials yet. Add credentials before running a balance check.",
        ));
    }
    let provider = row.provider()?;
    let credentials = row.credentials()?;
    let custom_adapter = row.custom_adapter()?;
    if provider == Provider::CustomHttp && custom_adapter.is_none() {
        return Err(AppError::validation(
            "This custom HTTP account has no adapter config.",
        ));
    }
    Ok((
        provider,
        row.base_url.clone(),
        credentials,
        row.balance_threshold,
        row.usd_credits_per_cny,
        custom_adapter,
    ))
}

/// An enabled account with everything the scheduler needs to check it.
/// Credential values stay inside the backend and are never serialized.
#[derive(Debug, Clone)]
pub struct SchedulableAccount {
    pub id: String,
    pub name: String,
    pub provider: Provider,
    pub base_url: String,
    pub credentials: Credentials,
    pub custom_adapter: Option<CustomHttpAdapterConfig>,
    /// Per-account threshold override, if any.
    pub balance_threshold: Option<f64>,
    /// USD credits received for one CNY, when converted alerting is enabled.
    pub usd_credits_per_cny: Option<f64>,
    /// Per-account interval override in minutes, if any.
    pub check_interval_minutes: Option<i64>,
    pub last_checked_at: Option<String>,
}

/// List enabled accounts eligible for scheduled checks. Disabled accounts are
/// excluded by the query so they are never picked up by the scheduler.
///
/// Accounts without usable credentials are also excluded here: a credentialless
/// placeholder cannot be checked, so it must never reach the provider layer even
/// if it were somehow enabled. Placeholders are created disabled, but this guard
/// makes the invariant explicit and robust to future edits.
pub async fn list_enabled_for_schedule(pool: &SqlitePool) -> AppResult<Vec<SchedulableAccount>> {
    let query = format!(
        "SELECT {ACCOUNT_COLUMNS} FROM gateway_accounts WHERE enabled = 1 ORDER BY created_at ASC"
    );
    let rows = sqlx::query_as::<_, AccountRow>(&query)
        .fetch_all(pool)
        .await?;

    let mut accounts = Vec::with_capacity(rows.len());
    for row in rows {
        if !row.has_credentials() {
            continue;
        }
        let provider = row.provider()?;
        let credentials = row.credentials()?;
        let custom_adapter = row.custom_adapter()?;
        accounts.push(SchedulableAccount {
            id: row.id,
            name: row.name,
            provider,
            base_url: row.base_url,
            credentials,
            custom_adapter,
            balance_threshold: row.balance_threshold,
            usd_credits_per_cny: row.usd_credits_per_cny,
            check_interval_minutes: row.check_interval_minutes,
            last_checked_at: row.last_checked_at,
        });
    }
    Ok(accounts)
}

/// Delete balance check history rows older than `cutoff` (an RFC 3339 string).
/// Timestamps are stored as RFC 3339, which sorts lexicographically in time
/// order, so a plain string comparison is correct here. Returns rows removed.
pub async fn delete_history_older_than(pool: &SqlitePool, cutoff: &str) -> AppResult<u64> {
    let affected = sqlx::query("DELETE FROM balance_check_history WHERE checked_at < ?")
        .bind(cutoff)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(affected)
}

/// Toggle account enabled state. Returns the updated account view.
/// Returns NotFound if the account does not exist.
pub async fn set_account_enabled(
    pool: &SqlitePool,
    id: &str,
    enabled: bool,
) -> AppResult<AccountView> {
    let now = Utc::now().to_rfc3339();
    let affected =
        sqlx::query("UPDATE gateway_accounts SET enabled = ?, updated_at = ? WHERE id = ?")
            .bind(enabled)
            .bind(&now)
            .bind(id)
            .execute(pool)
            .await?
            .rows_affected();

    if affected == 0 {
        return Err(AppError::NotFound);
    }

    get_account(pool, id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_pool;

    #[test]
    fn normalize_strips_trailing_slashes_and_whitespace() {
        assert_eq!(
            normalize_base_url("https://api.example.com/"),
            "https://api.example.com"
        );
        assert_eq!(
            normalize_base_url("  https://api.example.com///  "),
            "https://api.example.com"
        );
        assert_eq!(
            normalize_base_url("https://api.example.com"),
            "https://api.example.com"
        );
    }

    #[test]
    fn conversion_rate_must_be_a_finite_positive_number() {
        assert_eq!(validate_usd_credits_per_cny(None).unwrap(), None);
        assert_eq!(
            validate_usd_credits_per_cny(Some(12.0)).unwrap(),
            Some(12.0)
        );
        assert!(validate_usd_credits_per_cny(Some(0.0)).is_err());
        assert!(validate_usd_credits_per_cny(Some(-1.0)).is_err());
        assert!(validate_usd_credits_per_cny(Some(f64::INFINITY)).is_err());
    }

    #[test]
    fn fingerprint_is_stable_and_credential_sensitive() {
        let a = Credentials::NewApi {
            access_token: "tok".into(),
            user_id: "42".into(),
        };
        let b = Credentials::NewApi {
            access_token: "tok".into(),
            user_id: "42".into(),
        };
        let c = Credentials::NewApi {
            access_token: "other".into(),
            user_id: "42".into(),
        };
        assert_eq!(credential_fingerprint(&a), credential_fingerprint(&b));
        assert_ne!(credential_fingerprint(&a), credential_fingerprint(&c));
    }

    #[test]
    fn fingerprint_differs_across_providers() {
        let new_api = Credentials::NewApi {
            access_token: "secret".into(),
            user_id: "1".into(),
        };
        let sub2api = Credentials::Sub2Api {
            api_key: "secret".into(),
        };
        assert_ne!(
            credential_fingerprint(&new_api),
            credential_fingerprint(&sub2api)
        );
    }

    fn new_api_input(base_url: &str) -> CreateAccountInput {
        CreateAccountInput {
            name: "Main".into(),
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
                access_token: Some("tok".into()),
                user_id: Some("42".into()),
                api_key: None,
            },
            custom_adapter: None,
        }
    }

    #[tokio::test]
    async fn create_rejects_duplicate_identity() {
        let pool = init_memory_pool().await;
        let created = create_account(&pool, new_api_input("https://api.example.com/"))
            .await
            .expect("first create");
        assert!(created.has_credentials);
        assert_eq!(created.base_url, "https://api.example.com");

        // Same provider + normalized URL + credentials -> duplicate.
        let err = create_account(&pool, new_api_input("https://api.example.com"))
            .await
            .expect_err("duplicate should fail");
        assert!(matches!(err, AppError::Duplicate));
    }

    #[tokio::test]
    async fn create_requires_new_api_credentials() {
        let pool = init_memory_pool().await;
        let mut input = new_api_input("https://api.example.com");
        input.credentials.user_id = None;
        let err = create_account(&pool, input)
            .await
            .expect_err("missing user id");
        assert!(matches!(err, AppError::Validation(_)));
    }

    #[tokio::test]
    async fn create_stores_credit_conversion_rate() {
        let pool = init_memory_pool().await;
        let mut input = new_api_input("https://api.example.com");
        input.usd_credits_per_cny = Some(12.0);

        let created = create_account(&pool, input).await.expect("create");

        assert_eq!(created.usd_credits_per_cny, Some(12.0));
    }

    #[tokio::test]
    async fn create_stores_official_url_and_update_can_change_it() {
        let pool = init_memory_pool().await;
        let mut input = new_api_input("https://api.example.com");
        input.official_url = Some("  https://example.com/pricing  ".into());
        let created = create_account(&pool, input).await.expect("create");
        assert_eq!(
            created.official_url.as_deref(),
            Some("https://example.com/pricing")
        );

        // Update can clear it by submitting an empty value.
        let updated = update_account(
            &pool,
            &created.id,
            UpdateAccountInput {
                name: "Main".into(),
                base_url: "https://api.example.com".into(),
                enabled: true,
                balance_threshold: None,
                usd_credits_per_cny: None,
                check_interval_minutes: None,
                official_url: Some("".into()),
                note: None,
                sort_order: None,
                credentials: CredentialsInput::default(),
                custom_adapter: None,
            },
        )
        .await
        .expect("update");
        assert_eq!(updated.official_url, None);
    }

    #[tokio::test]
    async fn create_rejects_invalid_official_url() {
        let pool = init_memory_pool().await;
        let mut input = new_api_input("https://api.example.com");
        input.official_url = Some("ftp://example.com".into());
        let err = create_account(&pool, input)
            .await
            .expect_err("invalid official url");
        assert!(matches!(err, AppError::Validation(_)));
    }

    /// Insert a credentialless placeholder directly, mirroring how an import
    /// stores one (disabled, NULL credential columns and fingerprint).
    async fn insert_placeholder(pool: &SqlitePool, name: &str, base_url: &str) -> String {
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
        .expect("insert placeholder");
        id
    }

    #[tokio::test]
    async fn placeholder_view_reports_no_credentials() {
        let pool = init_memory_pool().await;
        let id = insert_placeholder(&pool, "Imported", "https://api.example.com").await;
        let view = get_account(&pool, &id).await.unwrap();
        assert!(!view.has_credentials);
        assert!(!view.enabled);
    }

    #[tokio::test]
    async fn load_credentials_errors_for_placeholder() {
        let pool = init_memory_pool().await;
        let id = insert_placeholder(&pool, "Imported", "https://api.example.com").await;
        let err = load_credentials(&pool, &id)
            .await
            .expect_err("placeholder has no credentials");
        assert!(matches!(err, AppError::Validation(_)));
    }

    #[tokio::test]
    async fn schedule_skips_credentialless_placeholder() {
        let pool = init_memory_pool().await;
        // A credentialed enabled account is schedulable; a placeholder is not.
        create_account(&pool, new_api_input("https://api.example.com"))
            .await
            .unwrap();
        // Force-enable a placeholder to prove the credential guard, not just the
        // disabled flag, keeps it out of the schedule.
        let id = insert_placeholder(&pool, "Imported", "https://other.example.com").await;
        sqlx::query("UPDATE gateway_accounts SET enabled = 1 WHERE id = ?")
            .bind(&id)
            .execute(&pool)
            .await
            .unwrap();

        let schedulable = list_enabled_for_schedule(&pool).await.unwrap();
        assert_eq!(schedulable.len(), 1);
        assert_eq!(schedulable[0].base_url, "https://api.example.com");
    }

    #[tokio::test]
    async fn placeholder_can_be_promoted_by_adding_credentials() {
        // Adding credentials to a placeholder via update_account turns it into a
        // full credentialed account (fingerprint set, has_credentials true).
        let pool = init_memory_pool().await;
        let id = insert_placeholder(&pool, "Imported", "https://api.example.com").await;
        let updated = update_account(
            &pool,
            &id,
            UpdateAccountInput {
                name: "Imported".into(),
                base_url: "https://api.example.com".into(),
                enabled: true,
                balance_threshold: None,
                usd_credits_per_cny: None,
                check_interval_minutes: None,
                official_url: None,
                note: None,
                sort_order: None,
                credentials: CredentialsInput {
                    access_token: Some("tok".into()),
                    user_id: Some("42".into()),
                    api_key: None,
                },
                custom_adapter: None,
            },
        )
        .await
        .expect("promote placeholder");
        assert!(updated.has_credentials);
        assert!(updated.enabled);
    }

    #[tokio::test]
    async fn list_view_never_exposes_raw_credentials() {
        let pool = init_memory_pool().await;
        create_account(&pool, new_api_input("https://api.example.com"))
            .await
            .unwrap();
        let views = list_accounts(&pool).await.unwrap();
        assert_eq!(views.len(), 1);
        // AccountView has no credential fields by construction; confirm the
        // serialized form contains no secret value.
        let json = serde_json::to_string(&views[0]).unwrap();
        assert!(!json.contains("tok"));
        assert!(json.contains("\"hasCredentials\":true"));
    }

    #[tokio::test]
    async fn update_preserves_credentials_when_left_blank() {
        let pool = init_memory_pool().await;
        let created = create_account(&pool, new_api_input("https://api.example.com"))
            .await
            .unwrap();
        let updated = update_account(
            &pool,
            &created.id,
            UpdateAccountInput {
                name: "Renamed".into(),
                base_url: "https://api.example.com".into(),
                enabled: false,
                balance_threshold: None,
                usd_credits_per_cny: None,
                check_interval_minutes: None,
                official_url: None,
                note: None,
                sort_order: None,
                credentials: CredentialsInput::default(),
                custom_adapter: None,
            },
        )
        .await
        .expect("update");
        assert_eq!(updated.name, "Renamed");
        assert!(!updated.enabled);
        assert!(updated.has_credentials);
    }

    #[tokio::test]
    async fn record_check_writes_history_and_snapshot() {
        let pool = init_memory_pool().await;
        let created = create_account(&pool, new_api_input("https://api.example.com"))
            .await
            .unwrap();
        let outcome = CheckOutcome {
            valid: true,
            remaining: Some(5.0),
            used: Some(1.0),
            total: Some(6.0),
            unit: Some("USD".into()),
            plan_name: Some("vip".into()),
            message: None,
        };
        // threshold 20 > remaining 5 -> low balance.
        let result = record_check(
            &pool,
            &created.id,
            Provider::NewApi,
            &outcome,
            Some(20.0),
            None,
        )
        .await
        .unwrap();
        assert_eq!(result, BalanceResult::LowBalance);

        let view = get_account(&pool, &created.id).await.unwrap();
        assert_eq!(view.last_result, Some(BalanceResult::LowBalance));
        assert_eq!(view.last_remaining, Some(5.0));

        let history = recent_history(&pool, 10).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].result, BalanceResult::LowBalance);
    }

    #[tokio::test]
    async fn record_check_uses_converted_value_but_keeps_raw_history() {
        let pool = init_memory_pool().await;
        let created = create_account(&pool, new_api_input("https://api.example.com"))
            .await
            .unwrap();
        let outcome = CheckOutcome {
            valid: true,
            remaining: Some(120.0),
            used: None,
            total: None,
            unit: Some("USD".into()),
            plan_name: None,
            message: None,
        };

        let result = record_check(
            &pool,
            &created.id,
            Provider::NewApi,
            &outcome,
            Some(20.0),
            Some(12.0),
        )
        .await
        .unwrap();

        assert_eq!(result, BalanceResult::LowBalance);
        let history = recent_history(&pool, 10).await.unwrap();
        assert_eq!(history[0].remaining, Some(120.0));
        assert_eq!(history[0].unit.as_deref(), Some("USD"));
    }

    #[tokio::test]
    async fn record_failed_check_writes_failed_history_and_snapshot() {
        let pool = init_memory_pool().await;
        let created = create_account(&pool, new_api_input("https://api.example.com"))
            .await
            .unwrap();

        // Seed a healthy snapshot first to prove a later failure clears the figures.
        let healthy = CheckOutcome {
            valid: true,
            remaining: Some(50.0),
            used: Some(1.0),
            total: Some(51.0),
            unit: Some("USD".into()),
            plan_name: Some("vip".into()),
            message: None,
        };
        record_check(
            &pool,
            &created.id,
            Provider::NewApi,
            &healthy,
            Some(20.0),
            None,
        )
        .await
        .unwrap();

        let result =
            record_failed_check(&pool, &created.id, Provider::NewApi, "connection timed out")
                .await
                .unwrap();
        assert_eq!(result, BalanceResult::Failed);

        // Snapshot flips to failed, keeps the message, and nulls the balance figures.
        let view = get_account(&pool, &created.id).await.unwrap();
        assert_eq!(view.last_result, Some(BalanceResult::Failed));
        assert_eq!(view.last_message.as_deref(), Some("connection timed out"));
        assert_eq!(view.last_remaining, None);
        assert_eq!(view.last_used, None);
        assert_eq!(view.last_total, None);
        assert_eq!(view.last_unit, None);
        assert_eq!(view.last_plan_name, None);
        assert!(view.last_checked_at.is_some());

        // Newest history row is the failure with null figures; the healthy row remains.
        let history = recent_history(&pool, 10).await.unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].result, BalanceResult::Failed);
        assert_eq!(history[0].message.as_deref(), Some("connection timed out"));
        assert_eq!(history[0].remaining, None);
        assert_eq!(history[0].total, None);
        assert_eq!(history[1].result, BalanceResult::Healthy);
    }

    #[test]
    fn classify_maps_threshold_and_validity() {
        let healthy = CheckOutcome {
            valid: true,
            remaining: Some(50.0),
            used: None,
            total: None,
            unit: None,
            plan_name: None,
            message: None,
        };
        assert_eq!(
            classify_outcome(&healthy, Some(20.0), None),
            BalanceResult::Healthy
        );
        assert_eq!(
            classify_outcome(&healthy, None, None),
            BalanceResult::Healthy
        );

        let low = CheckOutcome {
            remaining: Some(5.0),
            ..healthy.clone()
        };
        assert_eq!(
            classify_outcome(&low, Some(20.0), None),
            BalanceResult::LowBalance
        );

        let converted_low = CheckOutcome {
            remaining: Some(120.0),
            unit: Some("USD".into()),
            ..healthy.clone()
        };
        assert_eq!(
            classify_outcome(&converted_low, Some(20.0), Some(12.0)),
            BalanceResult::LowBalance
        );
        assert_eq!(
            classify_outcome(&converted_low, Some(5.0), Some(12.0)),
            BalanceResult::Healthy
        );

        let invalid = CheckOutcome {
            valid: false,
            ..healthy
        };
        assert_eq!(
            classify_outcome(&invalid, Some(20.0), Some(12.0)),
            BalanceResult::InvalidCredential
        );
    }

    #[test]
    fn history_limit_is_clamped() {
        // Non-positive / absent -> default.
        assert_eq!(clamp_history_limit(None), HISTORY_LIMIT_DEFAULT);
        assert_eq!(clamp_history_limit(Some(0)), HISTORY_LIMIT_DEFAULT);
        assert_eq!(clamp_history_limit(Some(-5)), HISTORY_LIMIT_DEFAULT);
        // In-range passes through; above max is capped.
        assert_eq!(clamp_history_limit(Some(10)), 10);
        assert_eq!(
            clamp_history_limit(Some(HISTORY_LIMIT_MAX + 1)),
            HISTORY_LIMIT_MAX
        );
    }

    #[tokio::test]
    async fn query_history_filters_by_account_result_and_provider() {
        let pool = init_memory_pool().await;

        let new_api = create_account(&pool, new_api_input("https://new.example.com"))
            .await
            .unwrap();
        let sub2api = create_account(
            &pool,
            CreateAccountInput {
                name: "Sub".into(),
                provider: Provider::Sub2Api,
                base_url: "https://sub.example.com".into(),
                enabled: true,
                balance_threshold: Some(20.0),
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

        let low = CheckOutcome {
            valid: true,
            remaining: Some(5.0),
            used: None,
            total: None,
            unit: Some("USD".into()),
            plan_name: None,
            message: None,
        };
        let healthy = CheckOutcome {
            remaining: Some(80.0),
            ..low.clone()
        };

        // new_api: one low_balance; sub2api: one healthy + one failed.
        record_check(&pool, &new_api.id, Provider::NewApi, &low, Some(20.0), None)
            .await
            .unwrap();
        record_check(
            &pool,
            &sub2api.id,
            Provider::Sub2Api,
            &healthy,
            Some(20.0),
            None,
        )
        .await
        .unwrap();
        record_failed_check(&pool, &sub2api.id, Provider::Sub2Api, "boom")
            .await
            .unwrap();

        // No filters: all three rows, newest first, with account names joined.
        let all = query_history(&pool, &HistoryQuery::default())
            .await
            .unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].result, BalanceResult::Failed);
        assert_eq!(all[0].account_name.as_deref(), Some("Sub"));

        // Filter by account.
        let by_account = query_history(
            &pool,
            &HistoryQuery {
                account_id: Some(new_api.id.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(by_account.len(), 1);
        assert_eq!(by_account[0].account_id, new_api.id);
        assert_eq!(by_account[0].result, BalanceResult::LowBalance);

        // Filter by result.
        let failed = query_history(
            &pool,
            &HistoryQuery {
                result: Some(BalanceResult::Failed),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].result, BalanceResult::Failed);

        // Filter by provider.
        let sub_rows = query_history(
            &pool,
            &HistoryQuery {
                provider: Some(Provider::Sub2Api),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(sub_rows.len(), 2);
        assert!(sub_rows.iter().all(|r| r.provider == Provider::Sub2Api));

        // Combined filters (AND): sub2api + healthy -> exactly one row.
        let combined = query_history(
            &pool,
            &HistoryQuery {
                provider: Some(Provider::Sub2Api),
                result: Some(BalanceResult::Healthy),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(combined.len(), 1);
        assert_eq!(combined[0].remaining, Some(80.0));
    }

    #[tokio::test]
    async fn set_account_enabled_toggles_and_updates_timestamp() {
        let pool = init_memory_pool().await;
        let created = create_account(&pool, new_api_input("https://api.example.com"))
            .await
            .unwrap();
        assert!(created.enabled);

        // Disable it.
        let disabled = set_account_enabled(&pool, &created.id, false)
            .await
            .unwrap();
        assert!(!disabled.enabled);
        assert_ne!(disabled.updated_at, created.updated_at);

        // Re-enable it.
        let re_enabled = set_account_enabled(&pool, &created.id, true).await.unwrap();
        assert!(re_enabled.enabled);
        assert_ne!(re_enabled.updated_at, disabled.updated_at);
    }

    #[tokio::test]
    async fn set_account_enabled_returns_not_found_for_missing_id() {
        let pool = init_memory_pool().await;
        let err = set_account_enabled(&pool, "nonexistent-id", true)
            .await
            .expect_err("should fail");
        assert!(matches!(err, AppError::NotFound));
    }

    #[tokio::test]
    async fn set_account_enabled_affects_schedule_list() {
        let pool = init_memory_pool().await;
        let created = create_account(&pool, new_api_input("https://api.example.com"))
            .await
            .unwrap();
        assert!(created.enabled);

        // Initially schedulable.
        let schedulable = list_enabled_for_schedule(&pool).await.unwrap();
        assert_eq!(schedulable.len(), 1);
        assert_eq!(schedulable[0].id, created.id);

        // Disable it; should drop from schedule.
        set_account_enabled(&pool, &created.id, false)
            .await
            .unwrap();
        let schedulable = list_enabled_for_schedule(&pool).await.unwrap();
        assert_eq!(schedulable.len(), 0);

        // Re-enable; should return to schedule.
        set_account_enabled(&pool, &created.id, true).await.unwrap();
        let schedulable = list_enabled_for_schedule(&pool).await.unwrap();
        assert_eq!(schedulable.len(), 1);
    }

    #[tokio::test]
    async fn reorder_accounts_updates_sort_order() {
        let pool = init_memory_pool().await;

        // Create two accounts with different credentials
        let account_a = create_account(&pool, {
            let mut input = new_api_input("https://api-a.example.com");
            input.name = "Account A".into();
            input.credentials.access_token = Some("token_a".into());
            input
        })
        .await
        .unwrap();

        let account_b = create_account(&pool, {
            let mut input = new_api_input("https://api-b.example.com");
            input.name = "Account B".into();
            input.credentials.access_token = Some("token_b".into());
            input
        })
        .await
        .unwrap();

        // Initial order: B should be first (higher sort_order from being created later)
        let initial = list_accounts(&pool).await.unwrap();
        assert_eq!(initial.len(), 2);
        assert_eq!(initial[0].id, account_b.id);
        assert_eq!(initial[1].id, account_a.id);
        assert!(initial[0].sort_order > initial[1].sort_order);
        // Verify consecutive integers: difference should be 1
        assert_eq!(initial[0].sort_order - initial[1].sort_order, 1);

        // Reorder: put A first, B second
        let reordered = reorder_accounts(&pool, vec![account_a.id.clone(), account_b.id.clone()])
            .await
            .unwrap();

        assert_eq!(reordered.len(), 2);
        assert_eq!(reordered[0].id, account_a.id);
        assert_eq!(reordered[1].id, account_b.id);
        assert!(reordered[0].sort_order > reordered[1].sort_order);
        // Verify consecutive integers after reorder: difference should be 1
        assert_eq!(reordered[0].sort_order - reordered[1].sort_order, 1);
        // First item should have sort_order = 2 (list length), second = 1
        assert_eq!(reordered[0].sort_order, 2);
        assert_eq!(reordered[1].sort_order, 1);

        // Verify the order persists in a fresh list call
        let after = list_accounts(&pool).await.unwrap();
        assert_eq!(after[0].id, account_a.id);
        assert_eq!(after[1].id, account_b.id);
    }
}
