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
}

pub type DbResult<T> = Result<T, DbError>;
