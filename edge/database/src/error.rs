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
}

pub type DbResult<T> = Result<T, DbError>;
