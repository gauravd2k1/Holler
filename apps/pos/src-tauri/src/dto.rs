//! Wire DTOs returned across the Tauri IPC boundary. Field names mirror
//! `packages/contracts` exactly where a TS+Zod shape exists (identity.ts's
//! `AuthenticatedPrincipal`, order.ts's `CanonicalOrder`, table.ts) so the
//! frontend can import the generated contract types unchanged.
//!
//! `packages/contracts` has no TS+Zod mirror yet for the menu tables
//! (`menu_category`/`menu_item`/`menu_item_variant`/`menu_item_modifier`) —
//! only the frozen SQLite schema in `packages/contracts/sqlite/0001_init.sql`
//! defines their shape. The menu DTOs below use that schema's column names
//! verbatim rather than inventing a wire shape, so a future contract mirror
//! is a trivial rename-free addition. This is a contract gap, reported.

use serde::Serialize;

use holler_edge_database::model as db;

// ---------------------------------------------------------------- identity --

/// Mirrors `packages/contracts/src/types/identity.ts` `AuthenticatedPrincipalSchema`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AuthenticatedPrincipal {
    pub user_id: String,
    pub tenant_id: String,
    pub outlet_id: String,
    pub full_name: String,
    pub permissions: Vec<String>,
    pub authenticated_offline: bool,
    pub schema_version: u8,
}

impl AuthenticatedPrincipal {
    pub fn from_app_user(u: &db::AppUser) -> Result<Self, serde_json::Error> {
        let permissions: Vec<String> = serde_json::from_str(&u.permissions_json)?;
        Ok(Self {
            user_id: u.id.clone(),
            tenant_id: u.tenant_id.clone(),
            outlet_id: u.outlet_id.clone(),
            full_name: u.full_name.clone(),
            permissions,
            authenticated_offline: true,
            schema_version: 1,
        })
    }
}

// -------------------------------------------------------------------- menu --

#[derive(Debug, Clone, Serialize)]
pub struct MenuCategory {
    pub id: String,
    pub outlet_id: String,
    pub name: String,
    pub sort_order: i64,
    pub config_version: i64,
}

impl From<db::MenuCategory> for MenuCategory {
    fn from(c: db::MenuCategory) -> Self {
        Self {
            id: c.id,
            outlet_id: c.outlet_id,
            name: c.name,
            sort_order: c.sort_order,
            config_version: c.config_version,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MenuItem {
    pub id: String,
    pub outlet_id: String,
    pub category_id: String,
    pub name: String,
    pub base_price_paise: i64,
    pub is_available: bool,
    pub config_version: i64,
}

impl From<db::MenuItem> for MenuItem {
    fn from(m: db::MenuItem) -> Self {
        Self {
            id: m.id,
            outlet_id: m.outlet_id,
            category_id: m.category_id,
            name: m.name,
            base_price_paise: m.base_price_paise,
            is_available: m.is_available,
            config_version: m.config_version,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MenuItemVariant {
    pub id: String,
    pub menu_item_id: String,
    pub name: String,
    pub price_delta_paise: i64,
    pub config_version: i64,
}

impl From<db::MenuItemVariant> for MenuItemVariant {
    fn from(v: db::MenuItemVariant) -> Self {
        Self {
            id: v.id,
            menu_item_id: v.menu_item_id,
            name: v.name,
            price_delta_paise: v.price_delta_paise,
            config_version: v.config_version,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
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

impl From<db::MenuItemModifier> for MenuItemModifier {
    fn from(m: db::MenuItemModifier) -> Self {
        Self {
            id: m.id,
            menu_item_id: m.menu_item_id,
            group_name: m.group_name,
            option_name: m.option_name,
            price_delta_paise: m.price_delta_paise,
            min_selection: m.min_selection,
            max_selection: m.max_selection,
            config_version: m.config_version,
        }
    }
}

// ------------------------------------------------------------------ table --

/// Mirrors `packages/contracts/src/types/table.ts` `RestaurantTableSchema`.
#[derive(Debug, Clone, Serialize)]
pub struct RestaurantTable {
    pub id: String,
    pub outlet_id: String,
    pub section: String,
    pub label: String,
    pub seat_count: i64,
    pub is_active: bool,
    pub config_version: i64,
    pub schema_version: u8,
}

impl From<db::RestaurantTable> for RestaurantTable {
    fn from(t: db::RestaurantTable) -> Self {
        Self {
            id: t.id,
            outlet_id: t.outlet_id,
            section: t.section,
            label: t.label,
            seat_count: t.seat_count,
            is_active: t.is_active,
            config_version: t.config_version,
            schema_version: 1,
        }
    }
}

/// Mirrors `packages/contracts/src/types/table.ts` `TableSessionSchema`.
#[derive(Debug, Clone, Serialize)]
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
    pub created_at: String,
    pub updated_at: String,
    pub schema_version: u8,
}

impl From<db::TableSession> for TableSession {
    fn from(s: db::TableSession) -> Self {
        Self {
            id: s.id,
            outlet_id: s.outlet_id,
            table_id: s.table_id,
            state: s.state,
            current_order_id: s.current_order_id,
            guest_count: s.guest_count,
            opened_by_user_id: s.opened_by_user_id,
            opened_at: s.opened_at,
            closed_at: s.closed_at,
            version: s.version,
            created_at: s.created_at,
            updated_at: s.updated_at,
            schema_version: 1,
        }
    }
}

// ------------------------------------------------------------------ order --
// Mirrors packages/contracts/src/types/order.ts CanonicalOrderSchema and
// OrderItemSchema field-for-field. Milestone 1 excludes tax/discount
// computation (Milestone 3) and modifiers (no order_item_modifier table
// exists yet in the frozen schema) — those fields are always present, per
// the contract, but always zero/empty in this milestone.

#[derive(Debug, Clone, Serialize)]
pub struct OrderItemModifier {
    pub modifier_id: String,
    pub group_name: String,
    pub option_name: String,
    pub price_delta_paise: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrderItem {
    pub id: String,
    pub menu_item_id: String,
    pub variant_id: Option<String>,
    pub quantity: i64,
    pub unit_price_paise: i64,
    pub line_total_paise: i64,
    pub modifiers: Vec<OrderItemModifier>,
    pub notes: Option<String>,
}

impl From<db::OrderItem> for OrderItem {
    fn from(i: db::OrderItem) -> Self {
        Self {
            id: i.id,
            menu_item_id: i.menu_item_id,
            variant_id: i.variant_id,
            quantity: i.quantity,
            unit_price_paise: i.unit_price_paise,
            line_total_paise: i.line_total_paise,
            modifiers: Vec::new(),
            notes: i.notes,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OrderCustomer {
    pub name: Option<String>,
    pub phone: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrderTimestamps {
    pub created_at: String,
    pub confirmed_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CanonicalOrder {
    pub holler_order_id: String,
    pub external_order_id: Option<String>,
    pub source: &'static str,
    pub outlet_id: String,

    pub order_type: String,
    pub status: String,
    pub table_id: Option<String>,

    pub customer: Option<OrderCustomer>,
    pub delivery_address: Option<String>,

    pub items: Vec<OrderItem>,

    pub subtotal_paise: i64,
    pub discount_paise: i64,
    pub packaging_paise: i64,
    pub delivery_charge_paise: i64,
    pub taxes_paise: i64,
    pub aggregator_discount_paise: i64,
    pub merchant_discount_paise: i64,
    pub total_paise: i64,

    pub payment_status: &'static str,
    pub payment_source: Option<String>,

    pub preparation_time_minutes: Option<i64>,
    pub rider: Option<serde_json::Value>,

    pub timestamps: OrderTimestamps,
    pub source_payload: Option<serde_json::Value>,

    pub schema_version: u8,
}

impl CanonicalOrder {
    /// Builds the wire shape directly from the rows about to be (or just)
    /// persisted, without round-tripping through a fabricated `db::Order`
    /// (which would require inventing `version`/`sync_status` values that
    /// were never actually read from storage).
    pub fn from_new_order_and_items(order: &db::NewOrder, items: &[db::NewOrderItem]) -> Self {
        Self {
            holler_order_id: order.id.clone(),
            external_order_id: None,
            source: "POS",
            outlet_id: order.outlet_id.clone(),
            order_type: order.order_type.clone(),
            status: order.status.clone(),
            table_id: order.table_id.clone(),
            customer: None,
            delivery_address: None,
            items: items
                .iter()
                .map(|i| OrderItem {
                    id: i.id.clone(),
                    menu_item_id: i.menu_item_id.clone(),
                    variant_id: i.variant_id.clone(),
                    quantity: i.quantity,
                    unit_price_paise: i.unit_price_paise,
                    line_total_paise: i.line_total_paise,
                    modifiers: Vec::new(),
                    notes: i.notes.clone(),
                })
                .collect(),
            subtotal_paise: order.subtotal_paise,
            discount_paise: order.discount_paise,
            packaging_paise: 0,
            delivery_charge_paise: 0,
            taxes_paise: order.tax_paise,
            aggregator_discount_paise: 0,
            merchant_discount_paise: 0,
            total_paise: order.total_paise,
            payment_status: "UNPAID",
            payment_source: None,
            preparation_time_minutes: None,
            rider: None,
            timestamps: OrderTimestamps {
                created_at: order.created_at.clone(),
                confirmed_at: None,
                updated_at: order.updated_at.clone(),
            },
            source_payload: None,
            schema_version: 1,
        }
    }

    pub fn from_order_and_items(order: db::Order, items: Vec<db::OrderItem>) -> Self {
        Self {
            holler_order_id: order.id,
            external_order_id: None,
            source: "POS",
            outlet_id: order.outlet_id,
            order_type: order.order_type,
            status: order.status,
            table_id: order.table_id,
            customer: None,
            delivery_address: None,
            items: items.into_iter().map(OrderItem::from).collect(),
            subtotal_paise: order.subtotal_paise,
            discount_paise: order.discount_paise,
            packaging_paise: 0,
            delivery_charge_paise: 0,
            taxes_paise: order.tax_paise,
            aggregator_discount_paise: 0,
            merchant_discount_paise: 0,
            total_paise: order.total_paise,
            payment_status: "UNPAID",
            payment_source: None,
            preparation_time_minutes: None,
            rider: None,
            timestamps: OrderTimestamps {
                created_at: order.created_at,
                confirmed_at: None,
                updated_at: order.updated_at,
            },
            source_payload: None,
            schema_version: 1,
        }
    }
}
