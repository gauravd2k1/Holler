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
}

pub type DbResult<T> = Result<T, DbError>;
