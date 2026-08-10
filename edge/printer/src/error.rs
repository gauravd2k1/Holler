use thiserror::Error;

/// All errors this crate can return. Deliberately typed and specific — no
/// `String`-only errors, no `unwrap`/`expect` in library paths (CLAUDE.md).
#[derive(Debug, Error)]
pub enum PrinterError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("edge database error: {0}")]
    Db(#[from] holler_edge_database::DbError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("not found: {0}")]
    NotFound(&'static str),

    #[error("no active printer routed for station {station_code}")]
    NoPrinterRouted { station_code: String },

    #[error("transport error talking to printer {printer_id} ({address}): {message}")]
    Transport {
        printer_id: String,
        address: String,
        message: String,
    },

    #[error("unsupported connection kind: {0}")]
    UnsupportedConnectionKind(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),
}

pub type PrinterResult<T> = Result<T, PrinterError>;
