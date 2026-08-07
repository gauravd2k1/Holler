//! Error types crossing the Tauri command boundary. Never carries credential
//! material (CLAUDE.md) and never leaks raw SQLite/rusqlite detail that
//! could confuse a cashier-facing UI — messages here are for the frontend
//! developer / logs, not proof of a specific failure mode.

use serde::Serialize;

/// Errors this crate's domain layer can produce, independent of storage.
#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("order {0} was not found")]
    OrderNotFound(String),

    #[error("order {0} is not in DRAFT status and cannot be modified")]
    OrderNotDraft(String),

    #[error("order item {0} was not found on order {1}")]
    OrderItemNotFound(String, String),

    #[error("quantity must be a positive integer")]
    InvalidQuantity,

    #[error("outlet_id/device_id are not configured for this device")]
    DeviceNotProvisioned,
}

/// The single error type every `#[tauri::command]` in this crate returns.
/// Implements [`Serialize`] (not `std::error::Error`) because Tauri's IPC
/// requires the `Err` variant of a command result to serialize to JSON.
#[derive(Debug, Serialize)]
pub struct AppError {
    pub code: &'static str,
    pub message: String,
}

impl AppError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl From<DomainError> for AppError {
    fn from(e: DomainError) -> Self {
        let code = match &e {
            DomainError::OrderNotFound(_) => "ORDER_NOT_FOUND",
            DomainError::OrderNotDraft(_) => "ORDER_NOT_DRAFT",
            DomainError::OrderItemNotFound(_, _) => "ORDER_ITEM_NOT_FOUND",
            DomainError::InvalidQuantity => "INVALID_QUANTITY",
            DomainError::DeviceNotProvisioned => "DEVICE_NOT_PROVISIONED",
        };
        AppError::new(code, e.to_string())
    }
}

impl From<holler_edge_database::DbError> for AppError {
    fn from(e: holler_edge_database::DbError) -> Self {
        use holler_edge_database::DbError;
        match e {
            DbError::CredentialMismatch => {
                AppError::new("CREDENTIAL_MISMATCH", "invalid email or password")
            }
            DbError::MalformedHash => {
                // Never surface the raw stored-hash detail to the frontend.
                AppError::new("CREDENTIAL_MISMATCH", "invalid email or password")
            }
            DbError::NotFound(what) => AppError::new("NOT_FOUND", format!("{what} not found")),
            DbError::InvalidInput(msg) => AppError::new("INVALID_INPUT", msg),
            other => AppError::new("STORAGE_ERROR", other.to_string()),
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;
