//! Typed rows and constructor inputs for the Milestone 1 tables. Field sets
//! mirror `packages/contracts/sqlite/000{1,2}_*.sql` exactly — no column is
//! added or renamed here. Money fields are `i64` paise; timestamps are
//! `String` (ISO8601 UTC text, per the schema's own column comments).

#[derive(Debug, Clone)]
pub struct Outlet {
    pub id: String,
    pub brand_id: String,
    pub name: String,
    pub timezone: String,
    pub config_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct Device {
    pub id: String,
    pub outlet_id: String,
    pub kind: String,
    pub name: String,
    pub last_seen_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct AppUser {
    pub id: String,
    pub tenant_id: String,
    pub outlet_id: String,
    pub email: String,
    pub full_name: String,
    pub password_hash: String,
    pub pin_hash: Option<String>,
    pub is_active: bool,
    pub permissions_json: String,
    pub config_version: i64,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct RestaurantTable {
    pub id: String,
    pub outlet_id: String,
    pub section: String,
    pub label: String,
    pub seat_count: i64,
    pub is_active: bool,
    pub config_version: i64,
}

#[derive(Debug, Clone)]
pub struct MenuCategory {
    pub id: String,
    pub outlet_id: String,
    pub name: String,
    pub sort_order: i64,
    pub config_version: i64,
}

#[derive(Debug, Clone)]
pub struct MenuItem {
    pub id: String,
    pub outlet_id: String,
    pub category_id: String,
    pub name: String,
    pub base_price_paise: i64,
    pub is_available: bool,
    pub config_version: i64,
}

#[derive(Debug, Clone)]
pub struct MenuItemVariant {
    pub id: String,
    pub menu_item_id: String,
    pub name: String,
    pub price_delta_paise: i64,
    pub config_version: i64,
}

#[derive(Debug, Clone)]
pub struct MenuItemModifier {
    pub id: String,
    pub menu_item_id: String,
    pub group_name: String,
    pub option_name: String,
    pub price_delta_paise: i64,
    pub min_selection: i64,
    pub max_selection: i64,
    pub config_version: i64,
}

/// Fields needed to insert a new `"order"` row. `id`/`created_at`/
/// `updated_at` are supplied by the caller (edge is authoritative for
/// operational aggregates — sync.md §50.1) rather than generated here, so
/// callers control UUIDv7 ordering and clock source.
///
/// `source`/`payment_status`/`schema_version` have SQLite-level `DEFAULT`s
/// (`packages/contracts/sqlite/0004_order_canonical_fields.sql`) that exist
/// only because SQLite requires a non-null default when adding a NOT NULL
/// column to an existing table — the ADR-011 0.2.4 addendum is explicit that
/// writers set them explicitly rather than relying on that default. Callers
/// must supply real values, not omit them and hope.
#[derive(Debug, Clone)]
pub struct NewOrder {
    pub id: String,
    pub outlet_id: String,
    pub device_id: String,
    pub order_type: String,
    pub status: String,
    pub table_id: Option<String>,
    pub subtotal_paise: i64,
    pub discount_paise: i64,
    pub taxes_paise: i64,
    pub total_paise: i64,
    pub source: String,
    pub external_order_id: Option<String>,
    pub payment_status: String,
    pub payment_source: Option<String>,
    /// DRAFT -> CONFIRMED transition time. `None` for a brand-new DRAFT
    /// order; stamped by whichever caller performs that transition.
    pub confirmed_at: Option<String>,
    pub source_payload_json: Option<String>,
    pub schema_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct Order {
    pub id: String,
    pub outlet_id: String,
    pub device_id: String,
    pub order_type: String,
    pub status: String,
    pub table_id: Option<String>,
    pub subtotal_paise: i64,
    pub discount_paise: i64,
    pub taxes_paise: i64,
    pub total_paise: i64,
    pub source: String,
    pub external_order_id: Option<String>,
    pub payment_status: String,
    pub payment_source: Option<String>,
    pub confirmed_at: Option<String>,
    pub source_payload_json: Option<String>,
    pub schema_version: i64,
    pub version: i64,
    pub sync_status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct NewOrderItem {
    pub id: String,
    pub order_id: String,
    pub menu_item_id: String,
    pub variant_id: Option<String>,
    pub quantity: i64,
    pub unit_price_paise: i64,
    pub line_total_paise: i64,
    pub notes: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct OrderItem {
    pub id: String,
    pub order_id: String,
    pub menu_item_id: String,
    pub variant_id: Option<String>,
    pub quantity: i64,
    pub unit_price_paise: i64,
    pub line_total_paise: i64,
    pub notes: Option<String>,
    pub created_at: String,
}

/// A snapshot of one modifier selection on an order line
/// (`order_item_modifier`, contracts 0.2.3). Used both to write and to read
/// back the row — like `menu_item_variant`/`menu_item_modifier`, there are
/// no server-generated columns, so one struct suffices instead of a
/// New/read pair. `modifier_id`/`group_name`/`option_name`/
/// `price_delta_paise` are deliberately snapshots, not a foreign key to the
/// live catalog (0003_order_item_modifiers.sql): a completed order's line
/// must never move because the menu changed underneath it.
#[derive(Debug, Clone)]
pub struct OrderItemModifier {
    pub id: String,
    pub order_item_id: String,
    pub modifier_id: String,
    pub group_name: String,
    pub option_name: String,
    pub price_delta_paise: i64,
    pub created_at: String,
}

/// Caller-supplied fields for the `local_outbox` row that
/// [`crate::Db::remove_order_item_with_outbox`] writes — mirrors
/// [`OrderItemAddedMeta`]. `event_type` (the frozen `ItemRemoved` string)
/// and `payload_json` (the full removed line, including its modifiers) are
/// owned by the crate, built from the row it is about to delete, so a
/// caller cannot describe a mismatched removal.
#[derive(Debug, Clone)]
pub struct OrderItemRemovedMeta {
    pub outbox_id: String,
    pub occurred_at: String,
}

#[derive(Debug, Clone)]
pub struct NewTableSession {
    pub id: String,
    pub outlet_id: String,
    pub table_id: String,
    pub state: String,
    pub current_order_id: Option<String>,
    pub guest_count: i64,
    pub opened_by_user_id: Option<String>,
    pub opened_at: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct TableSession {
    pub id: String,
    pub outlet_id: String,
    pub table_id: String,
    pub state: String,
    pub current_order_id: Option<String>,
    pub guest_count: i64,
    pub opened_by_user_id: Option<String>,
    pub opened_at: String,
    pub closed_at: Option<String>,
    pub version: i64,
    pub sync_status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct Kot {
    pub id: String,
    pub order_id: String,
    pub station: String,
    pub sequence: i64,
    pub status: String,
    pub items_json: String,
    pub created_by_device_id: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Caller-supplied fields for the `local_outbox` row that
/// [`crate::Db::add_order_item_with_outbox`] writes, deliberately narrower
/// than [`NewOutboxEntry`]: `event_type` and `payload_json` are owned by
/// the crate (built from the `order_item` row it just wrote, matching the
/// frozen `ItemAdded` event in `packages/contracts/src/types/events.ts`) so
/// a caller cannot describe a mismatched event for a real write. The
/// caller supplies only what the crate genuinely cannot derive: the
/// outbox row's own id and the moment the event occurred.
#[derive(Debug, Clone)]
pub struct OrderItemAddedMeta {
    pub outbox_id: String,
    pub occurred_at: String,
}

/// A local_outbox row to be written in the *same* transaction as the
/// operational write it describes (ADR-007). `id`/`created_at` are supplied
/// by the caller for the same reason as [`NewOrder`].
#[derive(Debug, Clone)]
pub struct NewOutboxEntry {
    pub id: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub event_type: String,
    pub payload_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct OutboxEntry {
    pub id: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub event_type: String,
    pub payload_json: String,
    pub created_at: String,
    pub published_at: Option<String>,
    pub attempt_count: i64,
}

#[derive(Debug, Clone)]
pub struct SyncState {
    pub outlet_id: String,
    pub last_pushed_outbox_id: Option<String>,
    pub last_applied_config_version: i64,
    pub last_sync_attempt_at: Option<String>,
    pub last_sync_success_at: Option<String>,
    pub is_online: bool,
}
