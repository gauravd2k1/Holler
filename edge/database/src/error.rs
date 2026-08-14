use thiserror::Error;

use crate::model::UnroutedKitchenItem;

/// Renders the item list for [`DbError::UnroutedKitchenItems`]'s `Display`
/// impl. A free function rather than inline in the `#[error(...)]` string
/// because the message needs a joined name list, not just one field.
fn format_unrouted_items(items: &[UnroutedKitchenItem]) -> String {
    items
        .iter()
        .map(|i| i.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

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

    /// A caller tried to amend an order line while the order is in a status
    /// that no longer permits it. Two different legal-status sets share this
    /// one variant: removing a line or correcting the order's shape is only
    /// legal from DRAFT (`Db::remove_order_item_with_outbox`,
    /// `Db::update_order_shape_with_outbox`); adding a line or changing a
    /// line's quantity is legal through DRAFT/CONFIRMED/SENT_TO_KITCHEN/
    /// PREPARING — the `#132-A` post-DRAFT addition path
    /// (`Db::add_order_item_with_outbox`,
    /// `Db::update_order_item_quantity_with_outbox`). The edge enforces
    /// whichever set applies itself rather than trusting the caller; `status`
    /// carries the order's actual status so the caller can render a specific
    /// message rather than inferring one.
    #[error("order {order_id} is not amendable: status is {status}")]
    OrderNotAmendable { order_id: String, status: String },

    /// A caller tried to confirm (DRAFT -> CONFIRMED) an order that is not
    /// DRAFT. The transition is only legal once, from DRAFT; the edge
    /// enforces this itself rather than trusting the caller (see
    /// `Db::confirm_order_with_outbox`).
    #[error("order {order_id} is not confirmable: status is {status}, not DRAFT")]
    OrderNotConfirmable { order_id: String, status: String },

    /// A caller tried to send an order to the kitchen while it is in a
    /// status that cannot legally produce KOTs (e.g. still DRAFT, or
    /// already SERVED/BILLED/PAID/CLOSED/CANCELLED). See
    /// `Db::send_order_to_kitchen_with_outbox`.
    #[error("order {order_id} is not sendable to kitchen: status is {status}")]
    OrderNotSendableToKitchen { order_id: String, status: String },

    /// A `send_order_to_kitchen_with_outbox` call resolved zero unticketed,
    /// station-routed order items. Never a silent no-op — either the order
    /// truly has nothing new for the kitchen, or its items are not routed
    /// to any active station, and the caller needs to know which.
    #[error("order {order_id} has no unticketed, station-routed items to send")]
    NothingToSendToKitchen { order_id: String },

    /// A `send_order_to_kitchen_with_outbox` call found a *mix* of routed
    /// and unrouted unticketed order lines (an all-unrouted call still gets
    /// `NothingToSendToKitchen`, unchanged from before this variant
    /// existed). Rejected outright rather than sent-with-partial-loss:
    /// nothing in `packages/contracts` distinguishes "deliberately
    /// non-production line" from "routing config gap", so treating an
    /// unrouted line as intentional would silently drop real dishes
    /// (docs/backlog-m2.md, "A mixed order sends silently when one line
    /// has no station"; docs/m3-planning.md §2 Track A). No `kot` row is
    /// written for *any* line in the same call — the whole send fails
    /// together so the cashier is never told part of an order reached the
    /// kitchen when it did not, and can retry once routing is fixed or the
    /// unrouted item is removed.
    #[error(
        "order {order_id} has {} item(s) with no kitchen station route: {}",
        items.len(),
        format_unrouted_items(items)
    )]
    UnroutedKitchenItems {
        order_id: String,
        items: Vec<UnroutedKitchenItem>,
    },

    /// A caller requested an illegal KOT status transition. Never a silent
    /// no-op (docs/spec/kitchen.md statuses; ADR-014 §4 — `kot.status` has
    /// exactly one writer, and that writer enforces the state machine).
    #[error("kot {kot_id} cannot transition from {from} to {to}")]
    IllegalKotStatusTransition {
        kot_id: String,
        from: String,
        to: String,
    },

    /// A caller tried to change the quantity of a line that some `kot` row
    /// already carries a frozen snapshot of
    /// (`repo::already_ticketed_order_item_ids`;
    /// `Db::update_order_item_quantity_with_outbox`). `kot.items_json` is
    /// written once at ticket creation and never revised in place — the
    /// kitchen's copy would silently disagree with the edge's if the
    /// quantity changed underneath it with no signal either way, exactly the
    /// print-visibility defect shape this milestone is closing elsewhere
    /// (docs/backlog-m2.md P1 "A KOT that can never be queued for print is
    /// invisible to staff"). Deliberately a hard rejection rather than a
    /// silent re-ticket or a mutated ticket: the `#132-C` cancellation path
    /// (`Db::cancel_kitchen_items_with_outbox`) is the sanctioned way to
    /// retract an already-ticketed line, followed by a fresh
    /// `add_order_item_with_outbox` at the corrected quantity — `error.rs`
    /// carries the message so a caller can render it verbatim (§64: staff
    /// must be told whether intervention is necessary, and what it is).
    #[error(
        "order item {order_item_id} on order {order_id} is already ticketed at the kitchen; \
         its quantity cannot be changed in place — cancel the line and add a replacement \
         with the new quantity"
    )]
    OrderItemAlreadyTicketed {
        order_item_id: String,
        order_id: String,
    },

    /// A caller tried to record a forward payment (`reverses_payment_id ==
    /// None`) with a non-positive `amount_paise`. Payments are append-only
    /// (docs/spec/payments.md §Conflict policy): a forward tender is always
    /// money coming in, never zero or negative — a correction is a reversal
    /// row, not a forward row with a strange sign.
    #[error(
        "payment amount {amount_paise} paise is invalid for a forward tender; a forward \
         payment must be > 0 (a correction or refund is a reversal row, with its own \
         reverses_payment_id and a non-positive amount)"
    )]
    ForwardPaymentAmountNotPositive { amount_paise: i64 },

    /// A caller tried to record a reversal (`reverses_payment_id ==
    /// Some(_)`) with a positive `amount_paise`. Mirrors the `payment` table's
    /// own `CHECK (reverses_payment_id IS NULL OR amount_paise <= 0)` —
    /// rejected here first so the caller gets a specific, actionable message
    /// (§64) rather than a generic SQLite constraint failure.
    #[error(
        "reversal amount {amount_paise} paise is invalid for payment {reverses_payment_id}; \
         a reversal (void or refund) must carry a non-positive amount"
    )]
    ReversalAmountNotNonPositive {
        reverses_payment_id: String,
        amount_paise: i64,
    },

    /// A caller tried to reverse a payment id that has no matching `payment`
    /// row — never a silent no-op, since a reversal with nothing behind it
    /// would be unaudited money.
    #[error("payment {payment_id} not found; cannot record a reversal against it")]
    ReversedPaymentNotFound { payment_id: String },

    /// A caller tried to reverse a payment whose settled amount (its own
    /// `amount_paise` plus every reversal already posted against it) is
    /// already zero. Rejected rather than silently doubling the reversal —
    /// the requirement this crate enforces before any write, not merely
    /// documents.
    #[error(
        "payment {payment_id} is already fully reversed (settled amount is 0); \
         it cannot be reversed again"
    )]
    PaymentAlreadyFullyReversed { payment_id: String },

    /// A caller tried to reverse more than a payment has left to give — the
    /// requested reversal's magnitude exceeds the payment's remaining
    /// settled amount. Rejected rather than letting the settled total go
    /// negative (over-refunding money that was never taken).
    #[error(
        "reversal of {requested_paise} paise against payment {payment_id} exceeds its \
         remaining settled amount of {remaining_paise} paise"
    )]
    ReversalExceedsRemaining {
        payment_id: String,
        requested_paise: i64,
        remaining_paise: i64,
    },

    /// A caller tried to open a second `cash_shift` for a cashier on a
    /// device that already has one open — the schema's own
    /// `idx_cash_shift_open_device_cashier` unique index forbids this, but
    /// this crate checks first so the caller gets a specific, actionable
    /// message (§64) instead of a raw constraint failure.
    #[error(
        "cashier {cashier_user_id} already has an open cash shift ({existing_shift_id}) on \
         device {device_id}; close it before opening a new one"
    )]
    CashShiftAlreadyOpen {
        device_id: String,
        cashier_user_id: String,
        existing_shift_id: String,
    },

    /// A caller tried to close (or otherwise act on) a `cash_shift` that is
    /// not currently `OPEN` — either it does not exist, or it was already
    /// closed. Never a silent no-op.
    #[error("cash shift {cash_shift_id} is not open (status is {status})")]
    CashShiftNotOpen {
        cash_shift_id: String,
        status: String,
    },

    /// §39, binding on this crate: a `cash_shift` close whose derived
    /// variance is non-zero MUST carry a non-blank reason. Caught here
    /// before any write — a rejected close, not a close that silently
    /// records an unexplained shortfall or overage. The message states the
    /// exact variance so staff know whether intervention is necessary and
    /// what is required of them (§64).
    #[error(
        "cash shift {cash_shift_id} close rejected: counted cash differs from expected by \
         {variance_paise} paise (expected {expected_paise}, counted {actual_paise}); a \
         non-blank reason is required to close with a variance"
    )]
    CashVarianceReasonRequired {
        cash_shift_id: String,
        expected_paise: i64,
        actual_paise: i64,
        variance_paise: i64,
    },

    /// A caller tried to post a `PAID_IN`/`PAID_OUT` cash movement with a
    /// blank reason. Mirrors the `cash_movement` table's own
    /// `CHECK (kind NOT IN ('PAID_IN','PAID_OUT') OR reason IS NOT NULL)` —
    /// rejected here first for the same §64 reason as the payment/variance
    /// checks above.
    #[error("a {kind} cash movement requires a non-blank reason")]
    CashMovementReasonRequired { kind: String },
}

pub type DbResult<T> = Result<T, DbError>;
