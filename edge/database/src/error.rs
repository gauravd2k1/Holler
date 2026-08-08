use thiserror::Error;

/// All errors this crate can return. Deliberately typed and specific — no
/// `String`-only errors, no `unwrap`/`expect` in library paths.
#[derive(Debug, Error)]
pub enum DbError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(std::io::Error),

    #[error("encryption error: {0}")]
    Encryption(&'static str),

    #[error("migration error: {0}")]
    Migration(String),

    #[error("credential verification failed")]
    CredentialMismatch,

    #[error("malformed stored credential hash")]
    MalformedHash,

    #[error("not found: {0}")]
    NotFound(&'static str),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// A caller tried to amend (add/remove an order_item on) an order that
    /// is not DRAFT. Amendment is only legal pre-confirmation; the edge
    /// enforces this itself rather than trusting the caller (see
    /// `Db::add_order_item_with_outbox` / `Db::remove_order_item_with_outbox`).
    #[error("order {order_id} is not amendable: status is {status}, not DRAFT")]
    OrderNotAmendable { order_id: String, status: String },

    /// A caller tried to confirm (DRAFT -> CONFIRMED) an order that is not
    /// DRAFT. The transition is only legal once, from DRAFT; the edge
    /// enforces this itself rather than trusting the caller (see
    /// `Db::confirm_order_with_outbox`).
    #[error("order {order_id} is not confirmable: status is {status}, not DRAFT")]
    OrderNotConfirmable { order_id: String, status: String },
}

pub type DbResult<T> = Result<T, DbError>;
