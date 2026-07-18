use serde::{Serialize, Serializer};

/// Application error surfaced to Tauri commands.
///
/// Serializes to a plain string so the frontend receives a readable message
/// via the command's `Err` channel.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Validation(String),

    #[error("account not found")]
    NotFound,

    #[error("an account with the same provider, base URL, and credentials already exists")]
    Duplicate,

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    #[error("request error: {0}")]
    Request(String),

    #[error("file error: {0}")]
    Io(String),
}

impl AppError {
    pub fn validation(msg: impl Into<String>) -> Self {
        AppError::Validation(msg.into())
    }
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
