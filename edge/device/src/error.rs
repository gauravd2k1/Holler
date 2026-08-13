use thiserror::Error;

/// Errors this crate can return. Deliberately typed — no `String`-only
/// errors, no `unwrap`/`expect` outside tests.
#[derive(Debug, Error)]
pub enum DeviceError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("websocket error: {0}")]
    WebSocket(#[from] tungstenite::Error),

    #[error("edge database error: {0}")]
    Db(#[from] holler_edge_database::DbError),

    #[error("kot contract error: {0}")]
    Contract(#[from] crate::contract::KotConvertError),

    #[error("invalid connection request: {0}")]
    InvalidRequest(String),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// The connection's first frame was not a valid, verified `auth` message
    /// (ADR-017 hole 3): missing, malformed, absent within the timeout, or a
    /// `device_token` the configured [`crate::auth::DeviceTokenVerifier`]
    /// rejected. Every case collapses to this one variant deliberately — a
    /// caller must not be able to distinguish "no token" from "wrong token"
    /// from "expired token" any more than `outlet.ErrInvalidDeviceToken`
    /// does on the cloud side.
    #[error("unauthorized: {0}")]
    Unauthorized(String),
}

pub type DeviceResult<T> = Result<T, DeviceError>;
