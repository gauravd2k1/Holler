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
        let message = e.to_string();
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
            DbError::OrderNotAmendable { .. } => AppError::new("ORDER_NOT_DRAFT", message),
            DbError::OrderNotConfirmable { .. } => AppError::new("ORDER_NOT_CONFIRMABLE", message),
            DbError::OrderNotSendableToKitchen { .. } => {
                AppError::new("ORDER_NOT_SENDABLE_TO_KITCHEN", message)
            }
            DbError::NothingToSendToKitchen { .. } => {
                AppError::new("NOTHING_TO_SEND_TO_KITCHEN", message)
            }
            DbError::UnroutedKitchenItems { ref items, .. } => {
                // Cashier-facing wording per §64 (docs/spec/ordering.md): name
                // the items, state plainly that nothing was sent, so the
                // control on screen (see PosScreen.tsx) can render this
                // verbatim rather than a generic failure.
                let names = items
                    .iter()
                    .map(|i| i.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let n = items.len();
                let noun = if n == 1 { "item has" } else { "items have" };
                AppError::new(
                    "UNROUTED_KITCHEN_ITEMS",
                    format!("{n} {noun} no kitchen station — not sent: {names}"),
                )
            }
            DbError::IllegalKotStatusTransition { .. } => {
                AppError::new("ILLEGAL_KOT_STATUS_TRANSITION", message)
            }
            // §64: never silence — the message already names the sanctioned
            // alternative (cancel the ticketed line via #132-C, then add a
            // replacement at the corrected quantity), so this code exists
            // purely so the frontend can distinguish it from a generic
            // storage failure and render that message rather than "Something
            // went wrong".
            DbError::OrderItemAlreadyTicketed { .. } => {
                AppError::new("ORDER_ITEM_ALREADY_TICKETED", message)
            }
            // §64, billing (ADR-016, docs/spec/payments.md): every one of
            // these carries a specific, actionable message from
            // `edge/database/src/error.rs` already — a variance amount, a
            // remaining reversible amount, an existing shift id — so the
            // frontend can render `message` verbatim instead of "Something
            // went wrong", and the `code` lets it distinguish cases where it
            // needs to (e.g. collecting a variance reason and retrying).
            DbError::ForwardPaymentAmountNotPositive { .. } => {
                AppError::new("FORWARD_PAYMENT_AMOUNT_NOT_POSITIVE", message)
            }
            DbError::ReversalAmountNotNonPositive { .. } => {
                AppError::new("REVERSAL_AMOUNT_NOT_NON_POSITIVE", message)
            }
            DbError::ReversedPaymentNotFound { .. } => {
                AppError::new("REVERSED_PAYMENT_NOT_FOUND", message)
            }
            DbError::PaymentAlreadyFullyReversed { .. } => {
                AppError::new("PAYMENT_ALREADY_FULLY_REVERSED", message)
            }
            DbError::ReversalExceedsRemaining { .. } => {
                AppError::new("REVERSAL_EXCEEDS_REMAINING", message)
            }
            DbError::CashShiftAlreadyOpen { .. } => {
                AppError::new("CASH_SHIFT_ALREADY_OPEN", message)
            }
            DbError::CashShiftNotOpen { .. } => AppError::new("CASH_SHIFT_NOT_OPEN", message),
            // The binding §39 case: the UI must never present a dead end
            // here — this code is what lets it show "counted cash differs by
            // X, enter a reason" and re-submit, rather than a generic error.
            DbError::CashVarianceReasonRequired { .. } => {
                AppError::new("CASH_VARIANCE_REASON_REQUIRED", message)
            }
            DbError::CashMovementReasonRequired { .. } => {
                AppError::new("CASH_MOVEMENT_REASON_REQUIRED", message)
            }
            // T9 retry, Defect 1 (double-settlement): the edge's own message
            // already names the invoice and the exact remaining due (§64) —
            // rendered verbatim rather than a vaguer rewrite.
            DbError::ForwardPaymentExceedsRemainingDue { .. } => {
                AppError::new("FORWARD_PAYMENT_EXCEEDS_REMAINING_DUE", message)
            }
            DbError::InvoiceNotFoundForPayment { .. } => {
                AppError::new("INVOICE_NOT_FOUND_FOR_PAYMENT", message)
            }
            // Milestone 4 (ADR-018 §11, T5): the edge's own message already
            // names the offending magnitude — surfaced verbatim (§64) rather
            // than a generic "something went wrong", so a wastage entry
            // submitted with no reason tells the cashier exactly why it was
            // rejected.
            DbError::WastageReasonRequired => AppError::new("WASTAGE_REASON_REQUIRED", message),
            DbError::WastageQuantityNotPositive { .. } => {
                AppError::new("WASTAGE_QUANTITY_NOT_POSITIVE", message)
            }
            // A caller tried to add/correct a line on, or complete, a count
            // that is not currently OPEN — the message already names the
            // count id and its actual status.
            DbError::StockCountNotOpen { .. } => AppError::new("STOCK_COUNT_NOT_OPEN", message),
            _other => AppError::new("STORAGE_ERROR", message),
        }
    }
}

impl From<holler_edge_printer::PrinterError> for AppError {
    fn from(e: holler_edge_printer::PrinterError) -> Self {
        use holler_edge_printer::PrinterError;
        let message = e.to_string();
        match e {
            PrinterError::NoPrinterRouted { .. } => AppError::new("NO_PRINTER_ROUTED", message),
            PrinterError::NotFound(what) => AppError::new("NOT_FOUND", format!("{what} not found")),
            PrinterError::Db(db_err) => AppError::from(db_err),
            _other => AppError::new("PRINTER_ERROR", message),
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;
