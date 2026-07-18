//! Notification hook CRUD.
//!
//! A notification hook is a user-configured generic HTTP endpoint (see
//! [`crate::notifications`] for delivery). This module owns the persistence and
//! validation for the hooks themselves; it does not perform any HTTP.

use chrono::Utc;
use sqlx::{FromRow, SqlitePool};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{validate_hook, CreateHookInput, HookView, NotificationHookType, UpdateHookInput};

#[derive(FromRow)]
struct HookRow {
    id: String,
    name: String,
    url: String,
    hook_type: String,
    enabled: bool,
    created_at: String,
    updated_at: String,
}

impl From<HookRow> for HookView {
    fn from(row: HookRow) -> Self {
        let hook_type = NotificationHookType::from_str(&row.hook_type)
            .unwrap_or(NotificationHookType::Generic);
        HookView {
            id: row.id,
            name: row.name,
            url: row.url,
            hook_type,
            enabled: row.enabled,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

const HOOK_COLUMNS: &str = "id, name, url, hook_type, enabled, created_at, updated_at";

/// List all notification hooks, newest first.
pub async fn list_hooks(pool: &SqlitePool) -> AppResult<Vec<HookView>> {
    let query = format!("SELECT {HOOK_COLUMNS} FROM notification_hooks ORDER BY created_at DESC");
    let rows = sqlx::query_as::<_, HookRow>(&query).fetch_all(pool).await?;
    Ok(rows.into_iter().map(HookView::from).collect())
}

/// Fetch a single hook by id.
pub async fn get_hook(pool: &SqlitePool, id: &str) -> AppResult<HookView> {
    let query = format!("SELECT {HOOK_COLUMNS} FROM notification_hooks WHERE id = ?");
    let row = sqlx::query_as::<_, HookRow>(&query)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(row.into())
}

/// Create a notification hook after validating its name and URL.
pub async fn create_hook(pool: &SqlitePool, input: CreateHookInput) -> AppResult<HookView> {
    let (name, url) = validate_hook(&input.name, &input.url)?;
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO notification_hooks (id, name, url, hook_type, enabled, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&name)
    .bind(&url)
    .bind(input.hook_type.as_str())
    .bind(input.enabled)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;

    get_hook(pool, &id).await
}

/// Update a notification hook after validating its name and URL.
pub async fn update_hook(
    pool: &SqlitePool,
    id: &str,
    input: UpdateHookInput,
) -> AppResult<HookView> {
    let (name, url) = validate_hook(&input.name, &input.url)?;
    let now = Utc::now().to_rfc3339();

    let affected = sqlx::query(
        "UPDATE notification_hooks SET name = ?, url = ?, hook_type = ?, enabled = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&name)
    .bind(&url)
    .bind(input.hook_type.as_str())
    .bind(input.enabled)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await?
    .rows_affected();
    if affected == 0 {
        return Err(AppError::NotFound);
    }

    get_hook(pool, id).await
}

/// Delete a notification hook by id. Existing delivery history rows keep their
/// `hook_id` (it is not a foreign key) so the audit/cooldown trail survives.
pub async fn delete_hook(pool: &SqlitePool, id: &str) -> AppResult<()> {
    let affected = sqlx::query("DELETE FROM notification_hooks WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    if affected == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// List every enabled hook. Used by the notification pipeline to fan a
/// Notification Event out to all configured endpoints.
pub async fn list_enabled_hooks(pool: &SqlitePool) -> AppResult<Vec<HookView>> {
    let query = format!(
        "SELECT {HOOK_COLUMNS} FROM notification_hooks WHERE enabled = 1 ORDER BY created_at ASC"
    );
    let rows = sqlx::query_as::<_, HookRow>(&query).fetch_all(pool).await?;
    Ok(rows.into_iter().map(HookView::from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_memory_pool;

    #[tokio::test]
    async fn create_list_update_delete_round_trip() {
        let pool = init_memory_pool().await;

        let created = create_hook(
            &pool,
            CreateHookInput {
                name: "Ops".into(),
                url: "https://hooks.example.com/a".into(),
                hook_type: NotificationHookType::Generic,
                enabled: true,
            },
        )
        .await
        .unwrap();
        assert_eq!(created.name, "Ops");
        assert_eq!(created.hook_type, NotificationHookType::Generic);
        assert!(created.enabled);

        let listed = list_hooks(&pool).await.unwrap();
        assert_eq!(listed.len(), 1);

        let updated = update_hook(
            &pool,
            &created.id,
            UpdateHookInput {
                name: "Ops renamed".into(),
                url: "https://hooks.example.com/b".into(),
                hook_type: NotificationHookType::Feishu,
                enabled: false,
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.name, "Ops renamed");
        assert_eq!(updated.url, "https://hooks.example.com/b");
        assert_eq!(updated.hook_type, NotificationHookType::Feishu);
        assert!(!updated.enabled);

        // Disabled hooks are excluded from the enabled list.
        assert!(list_enabled_hooks(&pool).await.unwrap().is_empty());

        delete_hook(&pool, &created.id).await.unwrap();
        assert!(list_hooks(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn create_rejects_invalid_url() {
        let pool = init_memory_pool().await;
        let err = create_hook(
            &pool,
            CreateHookInput {
                name: "Bad".into(),
                url: "ftp://nope".into(),
                hook_type: crate::models::NotificationHookType::Generic,
                enabled: true,
            },
        )
        .await
        .expect_err("invalid scheme rejected");
        assert!(matches!(err, AppError::Validation(_)));
    }

    #[tokio::test]
    async fn update_missing_hook_is_not_found() {
        let pool = init_memory_pool().await;
        let err = update_hook(
            &pool,
            "missing",
            UpdateHookInput {
                name: "x".into(),
                url: "https://hooks.example.com".into(),
                hook_type: crate::models::NotificationHookType::Generic,
                enabled: true,
            },
        )
        .await
        .expect_err("missing hook");
        assert!(matches!(err, AppError::NotFound));
    }
}
