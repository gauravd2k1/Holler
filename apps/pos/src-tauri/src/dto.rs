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
//!
//! Kot/Station/PrintJob DTOs below mirror `packages/contracts/src/types/`
//! `kot.ts`/`station.ts`/`printer.ts` exactly (ADR-014, contracts 0.3.0).

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
    // tax_profile_id (contracts 0.4.2) and hsn_sac (0.4.5) are NULLABLE in
    // MenuItemSchema but NOT optional, and Zod treats those differently: a
    // missing key fails `.parse` exactly like a wrong type. Omitting them
    // here rejected every list_menu_items call, and PosScreen renders
    // "Loading menu…" on `!hydrated`, so a rejected query is indistinguishable
    // from a slow one. Both are inputs to resolution only — billing snapshots
    // what it applied onto invoice_line, so passing them to the UI never
    // affects an issued bill (§31).
    pub tax_profile_id: Option<String>,
    pub hsn_sac: Option<String>,
    pub config_version: i64,
    pub schema_version: u8,
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
            tax_profile_id: m.tax_profile_id,
            hsn_sac: m.hsn_sac,
            config_version: m.config_version,
            schema_version: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MenuItemVariant {
    pub id: String,
    pub menu_item_id: String,
    pub name: String,
    pub price_delta_paise: i64,
    // is_default (contracts 0.5.0) and schema_version are both required by
    // MenuItemVariantSchema. No POS caller parses variants today, so this was
    // latent rather than broken — but it is the same drift that took the menu
    // down, and a wire type is not "fine because nothing reads it yet".
    pub is_default: bool,
    pub config_version: i64,
    pub schema_version: u8,
}

impl From<db::MenuItemVariant> for MenuItemVariant {
    fn from(v: db::MenuItemVariant) -> Self {
        Self {
            id: v.id,
            menu_item_id: v.menu_item_id,
            name: v.name,
            price_delta_paise: v.price_delta_paise,
            is_default: v.is_default,
            config_version: v.config_version,
            schema_version: 1,
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
// OrderItemSchema field-for-field. Milestone 1 excluded tax/discount
// computation (Milestone 3, in progress) and modifiers — Milestone 3 Track B
// (docs/m3-planning.md) closes the modifier half: `order_item_modifier`
// (contracts 0.2.3) has always existed and `holler_edge_database` has always
// been able to store/recompute deltas through it; the gap this crate closes
// is that every read path here returned an empty `modifiers: Vec::new()`
// regardless of what was actually stored, so a modifier's price_delta_paise
// never reached a caller after the write, only inside the outbox event.

#[derive(Debug, Clone, Serialize)]
pub struct OrderItemModifier {
    pub modifier_id: String,
    pub group_name: String,
    pub option_name: String,
    pub price_delta_paise: i64,
}

impl From<db::OrderItemModifier> for OrderItemModifier {
    fn from(m: db::OrderItemModifier) -> Self {
        Self {
            modifier_id: m.modifier_id,
            group_name: m.group_name,
            option_name: m.option_name,
            price_delta_paise: m.price_delta_paise,
        }
    }
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

impl OrderItem {
    /// The real conversion — call sites that actually know a line's
    /// modifiers (every current one does, via
    /// `holler_edge_database::repo::list_order_item_modifiers_for_order`)
    /// must use this, not a bare `From<db::OrderItem>` that would silently
    /// drop them again.
    pub fn from_db(item: db::OrderItem, modifiers: Vec<db::OrderItemModifier>) -> Self {
        Self {
            id: item.id,
            menu_item_id: item.menu_item_id,
            variant_id: item.variant_id,
            quantity: item.quantity,
            unit_price_paise: item.unit_price_paise,
            line_total_paise: item.line_total_paise,
            modifiers: modifiers.into_iter().map(OrderItemModifier::from).collect(),
            notes: item.notes,
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
    pub source: String,
    pub outlet_id: String,

    /// Short per-outlet human-facing number (`#A184` shape, contracts 0.4.0
    /// ADR-016 §6). `None` when this DTO was built pre-persist
    /// (`from_new_order_and_items`, before `Db::insert_order` has minted
    /// one) or for a legacy row written before this crate started minting
    /// (pre-0.4.1).
    pub display_number: Option<String>,

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

    pub payment_status: String,
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
    ///
    /// `item_modifiers[i]` is `items[i]`'s modifier list — the two slices
    /// must be the same length, matching
    /// `holler_edge_database::Db::create_order_with_outbox_and_modifiers`'s
    /// own contract (this is the DTO built from exactly the rows passed to
    /// that call).
    pub fn from_new_order_and_items(
        order: &db::NewOrder,
        items: &[db::NewOrderItem],
        item_modifiers: &[Vec<db::OrderItemModifier>],
    ) -> Self {
        assert_eq!(
            items.len(),
            item_modifiers.len(),
            "caller must supply exactly one modifier list per item"
        );
        Self {
            holler_order_id: order.id.clone(),
            external_order_id: order.external_order_id.clone(),
            source: order.source.clone(),
            outlet_id: order.outlet_id.clone(),
            // Not yet minted — this DTO is built before the order row is
            // persisted. `Db::create_order_with_outbox[_and_modifiers]`
            // patches the real value into the OrderCreated event payload
            // separately (`repo::patch_order_created_display_number`); a
            // caller that needs the real number back re-reads the order
            // after the create call, as `create_order_impl` does.
            display_number: None,
            order_type: order.order_type.clone(),
            status: order.status.clone(),
            table_id: order.table_id.clone(),
            customer: None,
            delivery_address: None,
            items: items
                .iter()
                .zip(item_modifiers.iter())
                .map(|(i, modifiers)| OrderItem {
                    id: i.id.clone(),
                    menu_item_id: i.menu_item_id.clone(),
                    variant_id: i.variant_id.clone(),
                    quantity: i.quantity,
                    unit_price_paise: i.unit_price_paise,
                    line_total_paise: i.line_total_paise,
                    modifiers: modifiers
                        .iter()
                        .cloned()
                        .map(OrderItemModifier::from)
                        .collect(),
                    notes: i.notes.clone(),
                })
                .collect(),
            subtotal_paise: order.subtotal_paise,
            discount_paise: order.discount_paise,
            packaging_paise: 0,
            delivery_charge_paise: 0,
            taxes_paise: order.taxes_paise,
            aggregator_discount_paise: 0,
            merchant_discount_paise: 0,
            total_paise: order.total_paise,
            payment_status: order.payment_status.clone(),
            payment_source: order.payment_source.clone(),
            preparation_time_minutes: None,
            rider: None,
            timestamps: OrderTimestamps {
                created_at: order.created_at.clone(),
                confirmed_at: order.confirmed_at.clone(),
                updated_at: order.updated_at.clone(),
            },
            source_payload: order
                .source_payload_json
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok()),
            schema_version: 1,
        }
    }

    /// Builds the wire shape from already-persisted rows, filling in each
    /// line's `modifiers` from `modifiers_by_item` (keyed by
    /// `order_item.id`, as returned by
    /// `holler_edge_database::repo::list_order_item_modifiers_for_order`) —
    /// a line with no entry gets an empty list, which is correct for a line
    /// that genuinely has no modifiers rather than an error.
    pub fn from_order_and_items(
        order: db::Order,
        items: Vec<db::OrderItem>,
        modifiers_by_item: &std::collections::HashMap<String, Vec<db::OrderItemModifier>>,
    ) -> Self {
        Self {
            holler_order_id: order.id,
            external_order_id: order.external_order_id,
            source: order.source,
            outlet_id: order.outlet_id,
            display_number: order.display_number,
            order_type: order.order_type,
            status: order.status,
            table_id: order.table_id,
            customer: None,
            delivery_address: None,
            items: items
                .into_iter()
                .map(|i| {
                    let modifiers = modifiers_by_item.get(&i.id).cloned().unwrap_or_default();
                    OrderItem::from_db(i, modifiers)
                })
                .collect(),
            subtotal_paise: order.subtotal_paise,
            discount_paise: order.discount_paise,
            packaging_paise: 0,
            delivery_charge_paise: 0,
            taxes_paise: order.taxes_paise,
            aggregator_discount_paise: 0,
            merchant_discount_paise: 0,
            total_paise: order.total_paise,
            payment_status: order.payment_status,
            payment_source: order.payment_source,
            preparation_time_minutes: None,
            rider: None,
            timestamps: OrderTimestamps {
                created_at: order.created_at,
                confirmed_at: order.confirmed_at,
                updated_at: order.updated_at,
            },
            source_payload: order
                .source_payload_json
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok()),
            schema_version: order.schema_version as u8,
        }
    }
}

// ------------------------------------------------------------ kitchen (M2) --
// Mirrors packages/contracts/src/types/kot.ts, station.ts, printer.ts
// field-for-field (ADR-014, contracts 0.3.0).

#[derive(Debug, Clone, Serialize)]
pub struct KotTicketItem {
    pub order_item_id: String,
    pub name: String,
    pub quantity: i64,
    pub modifiers: Vec<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Kot {
    pub id: String,
    pub order_id: String,
    pub station: String,
    pub sequence: i64,
    pub status: String,
    pub items: Vec<KotTicketItem>,
    pub created_by_device_id: String,
    pub created_at: String,
    pub updated_at: String,
    pub schema_version: u8,
}

/// Fails loudly (rather than silently dropping the ticket's items) if
/// `items_json` does not parse as the frozen `KotTicketItem[]` shape — a
/// malformed row here means the edge wrote something that cannot be shown
/// to the cashier or the kitchen, and that must not pass silently.
impl TryFrom<db::Kot> for Kot {
    type Error = serde_json::Error;

    fn try_from(k: db::Kot) -> Result<Self, Self::Error> {
        #[derive(serde::Deserialize)]
        struct RawItem {
            order_item_id: String,
            name: String,
            quantity: i64,
            #[serde(default)]
            modifiers: Vec<String>,
            notes: Option<String>,
        }
        let raw: Vec<RawItem> = serde_json::from_str(&k.items_json)?;
        Ok(Self {
            id: k.id,
            order_id: k.order_id,
            station: k.station,
            sequence: k.sequence,
            status: k.status,
            items: raw
                .into_iter()
                .map(|i| KotTicketItem {
                    order_item_id: i.order_item_id,
                    name: i.name,
                    quantity: i.quantity,
                    modifiers: i.modifiers,
                    notes: i.notes,
                })
                .collect(),
            created_by_device_id: k.created_by_device_id,
            created_at: k.created_at,
            updated_at: k.updated_at,
            schema_version: 1,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Station {
    pub id: String,
    pub outlet_id: String,
    pub code: String,
    pub name: String,
    pub sort_order: i64,
    pub is_active: bool,
    pub config_version: i64,
    pub schema_version: u8,
}

impl From<db::Station> for Station {
    fn from(s: db::Station) -> Self {
        Self {
            id: s.id,
            outlet_id: s.outlet_id,
            code: s.code,
            name: s.name,
            sort_order: s.sort_order,
            is_active: s.is_active,
            config_version: s.config_version,
            schema_version: 1,
        }
    }
}

/// What a failed `print_job` prints — mirrors
/// `holler_edge_printer::model::PrintJobTarget`, which is where this is
/// actually decided (`FailedPrintJobView::target`). This DTO exists only so
/// the frontend gets an explicit discriminant on the wire instead of having
/// to infer the kind from which of `kot_id`/`invoice_id` is present.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FailedPrintJobTarget {
    Kot,
    Invoice,
}

/// A failed `print_job`, joined with the printer name and, depending on
/// `target`, the KOT's station or the invoice's number, so the POS's
/// staff-visible failure view (docs/spec/hardware-printing.md "Print
/// failures must be visible to staff") does not need a second round trip
/// per row and does not lose either kind of job (§64: a bill that silently
/// exhausted its print retries is the same defect one layer up from a
/// dropped KOT). `print_job` itself has no wire shape in
/// `packages/contracts` beyond `PrintJobSchema` — this view type layers the
/// extra display fields on top; the frontend validates it against its own
/// local schema rather than `PrintJobSchema.extend(...)`, because
/// `PrintJobSchema.kot_id` is a required uuid and can no longer describe an
/// invoice-linked row.
///
/// Exactly one of `kot_id`/`kot_station` or `invoice_id`/`invoice_number` is
/// `Some`, mirroring `FailedPrintJobView` — `target` is the field a caller
/// should branch on, never field-nullness.
#[derive(Debug, Clone, Serialize)]
pub struct FailedPrintJob {
    pub id: String,
    pub target: FailedPrintJobTarget,
    pub kot_id: Option<String>,
    pub kot_station: Option<String>,
    pub invoice_id: Option<String>,
    pub invoice_number: Option<String>,
    pub printer_id: String,
    pub status: String,
    pub attempt_count: i64,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub printer_name: String,
    pub schema_version: u8,
}

// ----------------------------------------------------------- billing (M3) --
// Mirrors packages/contracts/src/types/invoice.ts InvoiceSchema/InvoiceLineSchema
// and packages/contracts/src/types/payment.ts PaymentSchema/CashShiftSchema/
// CashMovementSchema field-for-field (ADR-016, contracts 0.4.0). Every money
// field here is copied verbatim from `holler_edge_database` — this crate
// computes no tax or tender arithmetic of its own (CLAUDE.md: the edge
// computes, the UI formats).

/// Mirrors `packages/contracts/src/types/tax.ts` `DiscountDefinitionSchema`
/// field-for-field. Read-only here — this crate never writes a
/// `discount_definition` row (CLOUD_TO_EDGE config, ADR-016 §1); a POS
/// command only reads the outlet's catalogue to offer a cashier a choice.
#[derive(Debug, Clone, Serialize)]
pub struct DiscountDefinition {
    pub id: String,
    pub outlet_id: String,
    pub code: String,
    pub name: String,
    pub scope: String,
    pub method: String,
    pub value_bps: Option<i64>,
    pub value_paise: Option<i64>,
    pub max_discount_paise: Option<i64>,
    pub required_permission: Option<String>,
    pub requires_reason: bool,
    pub is_active: bool,
    pub effective_from: String,
    pub effective_to: Option<String>,
    pub config_version: i64,
    pub schema_version: u8,
}

impl From<db::DiscountDefinition> for DiscountDefinition {
    fn from(d: db::DiscountDefinition) -> Self {
        Self {
            id: d.id,
            outlet_id: d.outlet_id,
            code: d.code,
            name: d.name,
            scope: d.scope,
            method: d.method,
            value_bps: d.value_bps,
            value_paise: d.value_paise,
            max_discount_paise: d.max_discount_paise,
            required_permission: d.required_permission,
            requires_reason: d.requires_reason,
            is_active: d.is_active,
            effective_from: d.effective_from,
            effective_to: d.effective_to,
            config_version: d.config_version,
            schema_version: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct InvoiceLine {
    pub id: String,
    pub invoice_id: String,
    pub order_item_id: String,
    pub line_no: i64,
    pub description: String,
    pub hsn_sac: Option<String>,
    pub quantity: i64,
    pub unit_price_paise: i64,
    pub gross_paise: i64,
    pub discount_paise: i64,
    pub taxable_value_paise: i64,
    pub tax_profile_id: String,
    pub cgst_rate_bps: i64,
    pub cgst_paise: i64,
    pub sgst_rate_bps: i64,
    pub sgst_paise: i64,
    pub igst_rate_bps: i64,
    pub igst_paise: i64,
    pub cess_rate_bps: i64,
    pub cess_paise: i64,
    pub total_paise: i64,
    pub schema_version: u8,
}

impl From<db::InvoiceLine> for InvoiceLine {
    fn from(l: db::InvoiceLine) -> Self {
        Self {
            id: l.id,
            invoice_id: l.invoice_id,
            order_item_id: l.order_item_id,
            line_no: l.line_no,
            description: l.description,
            hsn_sac: l.hsn_sac,
            quantity: l.quantity,
            unit_price_paise: l.unit_price_paise,
            gross_paise: l.gross_paise,
            discount_paise: l.discount_paise,
            taxable_value_paise: l.taxable_value_paise,
            tax_profile_id: l.tax_profile_id,
            cgst_rate_bps: l.cgst_rate_bps,
            cgst_paise: l.cgst_paise,
            sgst_rate_bps: l.sgst_rate_bps,
            sgst_paise: l.sgst_paise,
            igst_rate_bps: l.igst_rate_bps,
            igst_paise: l.igst_paise,
            cess_rate_bps: l.cess_rate_bps,
            cess_paise: l.cess_paise,
            total_paise: l.total_paise,
            schema_version: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Invoice {
    pub id: String,
    pub outlet_id: String,
    pub order_id: String,
    pub split_group_id: Option<String>,
    pub split_index: i64,
    pub split_count: i64,
    pub series_id: String,
    pub invoice_number: String,
    pub invoice_date: String,
    pub business_date: String,
    pub status: String,
    pub cancelled_reason: Option<String>,
    pub cancelled_at: Option<String>,
    pub customer_name: Option<String>,
    pub customer_phone: Option<String>,
    pub customer_gstin: Option<String>,
    pub place_of_supply_state_code: String,
    pub lines: Vec<InvoiceLine>,
    pub subtotal_paise: i64,
    pub discount_paise: i64,
    pub taxable_value_paise: i64,
    pub cgst_paise: i64,
    pub sgst_paise: i64,
    pub igst_paise: i64,
    pub cess_paise: i64,
    pub round_off_paise: i64,
    pub grand_total_paise: i64,
    pub compliance_version_id: String,
    pub tax_snapshot: serde_json::Value,
    pub fiscal_profile: serde_json::Value,
    pub channel: String,
    pub tax_liability_party: String,
    pub eco_operator_name: Option<String>,
    pub eco_operator_gstin: Option<String>,
    pub supply_classification: Option<String>,
    pub created_by_user_id: String,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub schema_version: u8,
}

impl Invoice {
    /// Fails loudly (rather than silently substituting an empty object) if
    /// `tax_snapshot_json`/`fiscal_profile_json` do not parse — a malformed
    /// row here means the edge wrote something a GST invoice screen cannot
    /// show, and that must not pass silently (mirrors `Kot::try_from`'s
    /// discipline on `items_json`).
    pub fn from_db(
        inv: db::Invoice,
        lines: Vec<db::InvoiceLine>,
    ) -> Result<Self, serde_json::Error> {
        let tax_snapshot: serde_json::Value = serde_json::from_str(&inv.tax_snapshot_json)?;
        let fiscal_profile: serde_json::Value = serde_json::from_str(&inv.fiscal_profile_json)?;
        Ok(Self {
            id: inv.id,
            outlet_id: inv.outlet_id,
            order_id: inv.order_id,
            split_group_id: inv.split_group_id,
            split_index: inv.split_index,
            split_count: inv.split_count,
            series_id: inv.series_id,
            invoice_number: inv.invoice_number,
            invoice_date: inv.invoice_date,
            business_date: inv.business_date,
            status: inv.status,
            cancelled_reason: inv.cancelled_reason,
            cancelled_at: inv.cancelled_at,
            customer_name: inv.customer_name,
            customer_phone: inv.customer_phone,
            customer_gstin: inv.customer_gstin,
            place_of_supply_state_code: inv.place_of_supply_state_code,
            lines: lines.into_iter().map(InvoiceLine::from).collect(),
            subtotal_paise: inv.subtotal_paise,
            discount_paise: inv.discount_paise,
            taxable_value_paise: inv.taxable_value_paise,
            cgst_paise: inv.cgst_paise,
            sgst_paise: inv.sgst_paise,
            igst_paise: inv.igst_paise,
            cess_paise: inv.cess_paise,
            round_off_paise: inv.round_off_paise,
            grand_total_paise: inv.grand_total_paise,
            compliance_version_id: inv.compliance_version_id,
            tax_snapshot,
            fiscal_profile,
            channel: inv.channel,
            tax_liability_party: inv.tax_liability_party,
            eco_operator_name: inv.eco_operator_name,
            eco_operator_gstin: inv.eco_operator_gstin,
            supply_classification: inv.supply_classification,
            created_by_user_id: inv.created_by_user_id,
            created_at: inv.created_at,
            updated_at: inv.updated_at,
            version: inv.version,
            schema_version: 1,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Payment {
    pub id: String,
    pub outlet_id: String,
    pub order_id: String,
    pub cash_shift_id: Option<String>,
    pub method: String,
    pub status: String,
    pub amount_paise: i64,
    pub tendered_paise: Option<i64>,
    pub change_paise: Option<i64>,
    pub reference: Option<String>,
    pub external_id: Option<String>,
    pub reverses_payment_id: Option<String>,
    pub captured_at: Option<String>,
    /// Always empty: `payment_allocation` (payment<->invoice settlement) is
    /// unimplemented in `holler_edge_database` (T7c disclosure) — `payment`
    /// ties directly to `order_id` today. Present on the DTO so it matches
    /// `PaymentSchema`'s wire shape (which defaults it to `[]`) rather than
    /// omitting the field.
    pub allocations: Vec<serde_json::Value>,
    pub created_by_user_id: String,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub schema_version: u8,
}

impl From<db::Payment> for Payment {
    fn from(p: db::Payment) -> Self {
        Self {
            id: p.id,
            outlet_id: p.outlet_id,
            order_id: p.order_id,
            cash_shift_id: p.cash_shift_id,
            method: p.method,
            status: p.status,
            amount_paise: p.amount_paise,
            tendered_paise: p.tendered_paise,
            change_paise: p.change_paise,
            reference: p.reference,
            external_id: p.external_id,
            reverses_payment_id: p.reverses_payment_id,
            captured_at: p.captured_at,
            allocations: Vec::new(),
            created_by_user_id: p.created_by_user_id,
            created_at: p.created_at,
            updated_at: p.updated_at,
            version: p.version,
            schema_version: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CashMovement {
    pub id: String,
    pub cash_shift_id: String,
    pub kind: String,
    pub amount_paise: i64,
    pub reason: Option<String>,
    pub payment_id: Option<String>,
    pub created_by_user_id: String,
    pub created_at: String,
    pub schema_version: u8,
}

impl From<db::CashMovement> for CashMovement {
    fn from(m: db::CashMovement) -> Self {
        Self {
            id: m.id,
            cash_shift_id: m.cash_shift_id,
            kind: m.kind,
            amount_paise: m.amount_paise,
            reason: m.reason,
            payment_id: m.payment_id,
            created_by_user_id: m.created_by_user_id,
            created_at: m.created_at,
            schema_version: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CashShift {
    pub id: String,
    pub outlet_id: String,
    pub device_id: String,
    pub cashier_user_id: String,
    pub status: String,
    pub opened_at: String,
    pub opening_cash_paise: i64,
    pub closed_at: Option<String>,
    pub expected_cash_paise: Option<i64>,
    pub actual_cash_paise: Option<i64>,
    pub variance_paise: Option<i64>,
    pub variance_reason: Option<String>,
    pub business_date: String,
    pub movements: Vec<CashMovement>,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub schema_version: u8,
}

impl CashShift {
    pub fn from_db(s: db::CashShift, movements: Vec<db::CashMovement>) -> Self {
        Self {
            id: s.id,
            outlet_id: s.outlet_id,
            device_id: s.device_id,
            cashier_user_id: s.cashier_user_id,
            status: s.status,
            opened_at: s.opened_at,
            opening_cash_paise: s.opening_cash_paise,
            closed_at: s.closed_at,
            expected_cash_paise: s.expected_cash_paise,
            actual_cash_paise: s.actual_cash_paise,
            variance_paise: s.variance_paise,
            variance_reason: s.variance_reason,
            business_date: s.business_date,
            movements: movements.into_iter().map(CashMovement::from).collect(),
            created_at: s.created_at,
            updated_at: s.updated_at,
            version: s.version,
            schema_version: 1,
        }
    }
}

impl From<holler_edge_printer::model::FailedPrintJobView> for FailedPrintJob {
    /// Discriminates via `FailedPrintJobView::target` (which itself
    /// delegates to `PrintJob::target`, decoded from the same CHECK the row
    /// obeys) rather than by testing which of `kot_id`/`invoice_id` is
    /// `Some` — one place decides "what kind of job is this", not a second
    /// one re-derived here. `target()` only errors if the row violates its
    /// own CHECK, which this crate's own write paths make impossible; that
    /// is a defensive `expect`, not a case this DTO needs to model as
    /// fallible to its caller.
    fn from(v: holler_edge_printer::model::FailedPrintJobView) -> Self {
        let target = v
            .target()
            .expect("print_job row violates its own kot_id/invoice_id CHECK");
        let target = match target {
            holler_edge_printer::model::PrintJobTarget::Kot(_) => FailedPrintJobTarget::Kot,
            holler_edge_printer::model::PrintJobTarget::Invoice(_) => FailedPrintJobTarget::Invoice,
        };
        Self {
            id: v.job.id,
            target,
            kot_id: v.job.kot_id,
            kot_station: v.kot_station,
            invoice_id: v.job.invoice_id,
            invoice_number: v.invoice_number,
            printer_id: v.job.printer_id,
            status: v.job.status.as_db_str().to_string(),
            attempt_count: v.job.attempt_count,
            last_error: v.job.last_error,
            created_at: v.job.created_at,
            updated_at: v.job.updated_at,
            printer_name: v.printer_name,
            schema_version: 1,
        }
    }
}

// --------------------------------------------------------- inventory (M4) --
// ADR-018, `packages/contracts/src/types/inventory.ts`. Field names mirror
// that file exactly, the same discipline every other DTO in this module
// follows, so the frontend's generated Zod schemas parse these unchanged.

/// The bounded, outlet-wide current-stock read (ADR-018 §9) — what T5's
/// low-stock surfacing and the wastage/count item pickers both read from.
/// No `packages/contracts` mirror exists for this shape (it is a POS-local
/// read projection, not a stored aggregate) — a local Zod schema on the
/// frontend, the `MenuCategory` precedent this module's own doc comment
/// already names.
#[derive(Debug, Clone, Serialize)]
pub struct CurrentStockLine {
    pub inventory_item_id: String,
    pub inventory_item_name: String,
    pub dimension: String,
    pub current_quantity_micro: i64,
    pub reorder_level_micro: Option<i64>,
    pub par_level_micro: Option<i64>,
    pub schema_version: u8,
}

impl From<db::CurrentStockLine> for CurrentStockLine {
    fn from(l: db::CurrentStockLine) -> Self {
        Self {
            inventory_item_id: l.inventory_item_id,
            inventory_item_name: l.inventory_item_name,
            dimension: l.dimension,
            current_quantity_micro: l.current_quantity_micro,
            reorder_level_micro: l.reorder_level_micro,
            par_level_micro: l.par_level_micro,
            schema_version: 1,
        }
    }
}

/// Mirrors `StockDeductionGapSchema` — one row of the "items sold with no
/// recipe" report (M4 acceptance criterion 5, ADR-018 §10.1). `quantity` is
/// sellable units, NOT a micro-quantity: nothing resolved to an ingredient,
/// which is the point of the row, so the frontend must not run it through
/// any micro formatter.
#[derive(Debug, Clone, Serialize)]
pub struct StockDeductionGap {
    pub id: String,
    pub outlet_id: String,
    /// The ranged-replay mark (contracts 0.5.8), from the gap stream's own
    /// counter.
    pub entry_seq: i64,
    pub order_id: String,
    pub order_item_id: String,
    pub menu_item_id: String,
    pub menu_item_variant_id: Option<String>,
    pub menu_item_name: String,
    pub quantity: i64,
    pub reason: String,
    pub occurred_at: String,
    pub business_date: String,
    pub schema_version: u8,
}

impl From<db::StockDeductionGap> for StockDeductionGap {
    fn from(g: db::StockDeductionGap) -> Self {
        Self {
            id: g.id,
            outlet_id: g.outlet_id,
            entry_seq: g.entry_seq,
            order_id: g.order_id,
            order_item_id: g.order_item_id,
            menu_item_id: g.menu_item_id,
            menu_item_variant_id: g.menu_item_variant_id,
            menu_item_name: g.menu_item_name,
            quantity: g.quantity,
            reason: g.reason,
            occurred_at: g.occurred_at,
            business_date: g.business_date,
            schema_version: 1,
        }
    }
}

/// Mirrors `StockLedgerEntrySchema` — returned by `record_wastage` so the
/// cashier's screen can display what was actually written without a second
/// read.
#[derive(Debug, Clone, Serialize)]
pub struct StockLedgerEntry {
    pub id: String,
    pub outlet_id: String,
    pub entry_seq: i64,
    pub inventory_item_id: String,
    pub inventory_item_name: String,
    pub dimension: String,
    pub entry_type: String,
    pub origin: String,
    pub quantity_applied_micro: i64,
    pub recipe_id: Option<String>,
    pub recipe_version: Option<i64>,
    pub recipe_name: Option<String>,
    pub modifier_delta_id: Option<String>,
    pub modifier_name: Option<String>,
    pub modifier_delta_version: Option<i64>,
    pub source_order_id: Option<String>,
    pub source_order_item_id: Option<String>,
    pub reason_code: Option<String>,
    pub note: Option<String>,
    pub occurred_at: String,
    pub business_date: String,
    pub created_by_user_id: Option<String>,
    pub unit_cost_paise: Option<i64>,
    pub schema_version: u8,
}

impl From<db::StockLedgerEntry> for StockLedgerEntry {
    fn from(e: db::StockLedgerEntry) -> Self {
        Self {
            id: e.id,
            outlet_id: e.outlet_id,
            entry_seq: e.entry_seq,
            inventory_item_id: e.inventory_item_id,
            inventory_item_name: e.inventory_item_name,
            dimension: e.dimension,
            entry_type: e.entry_type,
            origin: e.origin,
            quantity_applied_micro: e.quantity_applied_micro,
            recipe_id: e.recipe_id,
            recipe_version: e.recipe_version,
            recipe_name: e.recipe_name,
            modifier_delta_id: e.modifier_delta_id,
            modifier_name: e.modifier_name,
            modifier_delta_version: e.modifier_delta_version,
            source_order_id: e.source_order_id,
            source_order_item_id: e.source_order_item_id,
            reason_code: e.reason_code,
            note: e.note,
            occurred_at: e.occurred_at,
            business_date: e.business_date,
            created_by_user_id: e.created_by_user_id,
            unit_cost_paise: e.unit_cost_paise,
            schema_version: 1,
        }
    }
}

/// Mirrors `StockCountSchema`.
#[derive(Debug, Clone, Serialize)]
pub struct StockCount {
    pub id: String,
    pub outlet_id: String,
    pub business_date: String,
    pub status: String,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub counted_by_user_id: Option<String>,
    pub note: Option<String>,
    pub schema_version: u8,
}

impl From<db::StockCount> for StockCount {
    fn from(c: db::StockCount) -> Self {
        Self {
            id: c.id,
            outlet_id: c.outlet_id,
            business_date: c.business_date,
            status: c.status,
            started_at: c.started_at,
            completed_at: c.completed_at,
            counted_by_user_id: c.counted_by_user_id,
            note: c.note,
            schema_version: 1,
        }
    }
}

/// Mirrors `StockCountLineSchema`.
#[derive(Debug, Clone, Serialize)]
pub struct StockCountLine {
    pub id: String,
    pub stock_count_id: String,
    pub inventory_item_id: String,
    pub inventory_item_name: String,
    pub dimension: String,
    pub counted_quantity_micro: i64,
    pub expected_quantity_micro: i64,
    pub note: Option<String>,
    pub schema_version: u8,
}

impl From<db::StockCountLine> for StockCountLine {
    fn from(l: db::StockCountLine) -> Self {
        Self {
            id: l.id,
            stock_count_id: l.stock_count_id,
            inventory_item_id: l.inventory_item_id,
            inventory_item_name: l.inventory_item_name,
            dimension: l.dimension,
            counted_quantity_micro: l.counted_quantity_micro,
            expected_quantity_micro: l.expected_quantity_micro,
            note: l.note,
            schema_version: 1,
        }
    }
}

/// One line of a completed count's variance report — no `packages/contracts`
/// mirror exists (it is DERIVED, never stored — ADR-018), so this is a
/// POS-local wire shape, field-for-field `StockCountVarianceLine`.
#[derive(Debug, Clone, Serialize)]
pub struct StockCountVarianceLine {
    pub inventory_item_id: String,
    pub inventory_item_name: String,
    pub dimension: String,
    pub counted_quantity_micro: i64,
    pub expected_quantity_micro: i64,
    pub variance_quantity_micro: i64,
    pub variance_percentage_bps: Option<i64>,
    pub schema_version: u8,
}

impl From<db::StockCountVarianceLine> for StockCountVarianceLine {
    fn from(l: db::StockCountVarianceLine) -> Self {
        Self {
            inventory_item_id: l.inventory_item_id,
            inventory_item_name: l.inventory_item_name,
            dimension: l.dimension,
            counted_quantity_micro: l.counted_quantity_micro,
            expected_quantity_micro: l.expected_quantity_micro,
            variance_quantity_micro: l.variance_quantity_micro,
            variance_percentage_bps: l.variance_percentage_bps,
            schema_version: 1,
        }
    }
}

/// A completed count's variance report. `sales_unaccounted` is the named
/// "N sales unaccounted" term (ADR-018 §10.1) — rendered standalone by the
/// count screen, never folded into any line's shrinkage.
#[derive(Debug, Clone, Serialize)]
pub struct StockCountVarianceReport {
    pub stock_count_id: String,
    pub business_date: String,
    pub lines: Vec<StockCountVarianceLine>,
    pub sales_unaccounted: i64,
    pub schema_version: u8,
}

impl From<db::StockCountVarianceReport> for StockCountVarianceReport {
    fn from(r: db::StockCountVarianceReport) -> Self {
        Self {
            stock_count_id: r.stock_count_id,
            business_date: r.business_date,
            lines: r
                .lines
                .into_iter()
                .map(StockCountVarianceLine::from)
                .collect(),
            sales_unaccounted: r.sales_unaccounted,
            schema_version: 1,
        }
    }
}

/// One ranged-replay entry this outlet has given up on sending
/// (contracts 0.5.8, `sync_replay_block`).
///
/// This is the human-visible half of the per-entry retry bound. The bound
/// exists so a single row the cloud will never accept cannot hold back every
/// row behind it — but an outlet that has quietly stopped sending part of its
/// stock history is exactly the kind of failure that must not be discovered
/// months later in a variance report. Halting sync is survivable; halting it
/// silently is not.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncReplayBlock {
    pub outlet_id: String,
    /// `"LEDGER"` or `"DEDUCTION_GAP"`.
    pub stream: String,
    pub entry_seq: i64,
    /// The row that could not be sent, so the person chasing this has
    /// something to look up rather than an ordinal.
    pub record_id: String,
    pub attempts: i64,
    pub last_status: Option<i64>,
    pub last_error: String,
    pub first_attempt_at: String,
    pub last_attempt_at: String,
    /// Always present on a row this command returns — it lists only entries
    /// whose budget is spent.
    pub blocked_at: Option<String>,
}

impl From<db::SyncReplayBlock> for SyncReplayBlock {
    fn from(b: db::SyncReplayBlock) -> Self {
        Self {
            outlet_id: b.outlet_id,
            stream: b.stream,
            entry_seq: b.entry_seq,
            record_id: b.record_id,
            attempts: b.attempts,
            last_status: b.last_status,
            last_error: b.last_error,
            first_attempt_at: b.first_attempt_at,
            last_attempt_at: b.last_attempt_at,
            blocked_at: b.blocked_at,
        }
    }
}
