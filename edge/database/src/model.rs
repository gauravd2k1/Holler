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
    /// The item's own pinned tax profile (contracts 0.4.2,
    /// `packages/contracts/sqlite/0007_menu_item_tax_profile.sql`). `None`
    /// means "use the outlet's default profile" — [`crate::tax::resolve_tax_profile`]
    /// is the one place that fallback happens; a `Some` that does not resolve
    /// to an active profile at the item's outlet is a config error, never a
    /// silent fallback (see that function's doc comment).
    pub tax_profile_id: Option<String>,
    /// HSN (goods) / SAC (services) code — contracts 0.4.5,
    /// `packages/contracts/sqlite/0011_menu_item_hsn_sac.sql`. Cloud-owned
    /// config, nullable by design (no invented fallback — a wrong code is
    /// worse than a missing one). `invoice::assemble::build_invoice` reads
    /// this at issue time and snapshots it onto `invoice_line.hsn_sac`; it
    /// rejects issuance outright (`DbError::MissingHsnSac`) rather than
    /// billing a line with none.
    pub hsn_sac: Option<String>,
}

/// `is_default` (contracts 0.5.7, ADR-018 §2.1, migration
/// `0014_menu_default_variant.sql`): at most one default per `menu_item`
/// (partial unique index at the schema layer, not re-validated here). Landed
/// on the wire at 0.5.7 — before that this crate could not sync it even
/// though the column has existed locally since 0014; see
/// `edge/sync::config::WireMenuItemVariant`'s doc comment for the M4 T4b/T4c
/// history of that gap.
#[derive(Debug, Clone)]
pub struct MenuItemVariant {
    pub id: String,
    pub menu_item_id: String,
    pub name: String,
    pub price_delta_paise: i64,
    pub is_default: bool,
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
    /// Short per-outlet human-facing number (`#A184` shape), contracts 0.4.0
    /// (ADR-016 §6). Minted internally by `repo::insert_order` for every
    /// order this crate creates — never accepted from a caller, so a
    /// caller cannot mint a duplicate or out-of-sequence number — hence its
    /// absence from `NewOrder`. `Option` only because a row written before
    /// this crate started minting (pre-0.4.1) has none; every row this crate
    /// writes going forward always has one.
    pub display_number: Option<String>,
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

/// Caller-supplied fields for
/// [`crate::Db::update_order_item_quantity_with_outbox`] — the single-write
/// `SET_ORDER_ITEM_QUANTITY` command (contracts 0.4.0, ADR-016). Carries
/// `outbox_id` since contracts 0.4.1 (ADR-016 addendum): every quantity
/// change now mints a fresh `ItemQuantityChanged` `local_outbox` row (in
/// addition to correcting an already still-unpublished `ItemAdded` row in
/// place, which stays harmless and keeps a not-yet-observed snapshot
/// consistent). See the doc comment on
/// [`crate::Db::update_order_item_quantity_with_outbox`] for the full
/// reasoning.
#[derive(Debug, Clone)]
pub struct OrderItemQuantitySetMeta {
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

// ---------------------------------------------------- Milestone 2: kitchen --
// station / menu_item_station / printer / station_printer are CONFIG
// aggregates (cloud→edge, config_version-versioned, replaced wholesale) per
// ADR-014 §1-2. This crate stores what sync gives it and never originates a
// row in any of the four. Field sets mirror
// `packages/contracts/sqlite/0005_m2_kitchen_stations_printers.sql` exactly.

/// A production destination (MAIN_KITCHEN, TANDOOR, BAR, ...). `code` is
/// unique per `(outlet_id, code)`, never globally (ADR-014 §1) — two outlets
/// both having a TANDOOR is the normal case. `kot.station` stores this
/// `code`, never `id`.
#[derive(Debug, Clone)]
pub struct Station {
    pub id: String,
    pub outlet_id: String,
    pub code: String,
    pub name: String,
    pub sort_order: i64,
    pub is_active: bool,
    pub config_version: i64,
}

/// One row of item→station routing (`menu_item_station`). A join row rather
/// than a column on `menu_item` because an item may route to more than one
/// station (a thali hits MAIN_KITCHEN and TANDOOR) — ADR-014 §2.
#[derive(Debug, Clone)]
pub struct MenuItemStation {
    pub menu_item_id: String,
    pub station_id: String,
    pub config_version: i64,
}

#[derive(Debug, Clone)]
pub struct Printer {
    pub id: String,
    pub outlet_id: String,
    pub name: String,
    pub connection_kind: String,
    pub address: String,
    pub paper_width_mm: i64,
    pub is_active: bool,
    pub config_version: i64,
}

/// Station→printer routing (`station_printer`). Many-to-many both ways.
#[derive(Debug, Clone)]
pub struct StationPrinter {
    pub station_id: String,
    pub printer_id: String,
    pub config_version: i64,
}

/// What one printer is eligible to print (`printer_role`, contracts 0.4.7).
/// CONFIG, cloud→edge, exactly like [`StationPrinter`] — a join table rather
/// than a column on `printer` so that one physical device can carry both
/// roles without a `BOTH` enum member every reader has to special-case, and
/// so adding the concept broke none of the many `Printer` struct literals
/// already in the tree (that migration's own rationale).
///
/// `role` is `KITCHEN` or `BILL`. A printer with NO row here has no role and
/// is a candidate for neither path — absence is never read as consent.
#[derive(Debug, Clone)]
pub struct PrinterRole {
    pub printer_id: String,
    pub role: String,
    pub config_version: i64,
}

/// One line on a station ticket, matching `KotTicketItemSchema`
/// (`packages/contracts/src/types/kot.ts`): `{ order_item_id, name,
/// quantity, modifiers, notes }`. Built by this crate from `order_item` +
/// `order_item_modifier` rows, never accepted from a caller, so a ticket
/// cannot describe items that were not actually ordered.
#[derive(Debug, Clone)]
pub struct KotTicketItem {
    pub order_item_id: String,
    pub name: String,
    pub quantity: i64,
    pub modifiers: Vec<String>,
    pub notes: Option<String>,
}

/// One order line that resolved to zero active stations when
/// `send_order_to_kitchen_with_outbox` tried to route it. Carried on
/// [`crate::DbError::UnroutedKitchenItems`] so the caller — ultimately the
/// cashier, via the Tauri command layer — learns *which* dish did not reach
/// a kitchen, not just that something did not (docs/spec/ordering.md §64:
/// staff must be told whether intervention is necessary).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnroutedKitchenItem {
    pub order_item_id: String,
    pub name: String,
}

/// One order line whose resolved `menu_item.hsn_sac` was NULL or blank at
/// invoice-assembly time — carried on [`crate::DbError::MissingHsnSac`], the
/// `UnroutedKitchenItem` precedent applied to ADR-016 0.4.5 §3's billing
/// completeness rule: name the item, not just the fact that something is
/// missing (§64).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingHsnSacItem {
    pub order_item_id: String,
    pub name: String,
}

/// Caller-supplied fields for [`crate::Db::send_order_to_kitchen_with_outbox`].
/// Unlike every other write in this crate, the number of `kot` rows this
/// call produces is not knowable to the caller ahead of time: it depends on
/// station routing resolved from `menu_item_station`, which can fan a
/// single order out to an arbitrary number of station tickets. So this
/// crate — not the caller — mints the KOT ids and their `KOTCreated`/
/// `SentToKitchen` outbox ids (UUIDv7, via the `uuid` crate), which is the
/// one deliberate exception to the "caller supplies every id" convention
/// used elsewhere in this crate.
#[derive(Debug, Clone)]
pub struct SendToKitchenMeta {
    pub device_id: String,
    pub occurred_at: String,
}

/// Caller-supplied fields for
/// [`crate::Db::transition_kot_status_with_outbox`]. `occurred_at` must be
/// sourced from the edge machine's own clock (sync.md §50.1) by the Tauri
/// command layer, exactly like `OrderConfirmedMeta::confirmed_at` — never a
/// value handed up from a KDS screen. `status_history_id` and `outbox_id`
/// are still caller-supplied (one row each, so the "crate mints ids"
/// exception above does not apply here) except for the conditional
/// `OrderReady` event, whose id this crate mints itself only when a
/// transition actually triggers it.
#[derive(Debug, Clone)]
pub struct KotTransitionMeta {
    pub status_history_id: String,
    pub outbox_id: String,
    pub changed_by_device_id: String,
    pub occurred_at: String,
}

#[derive(Debug, Clone)]
pub struct KotStatusHistoryEntry {
    pub id: String,
    pub kot_id: String,
    pub status: String,
    pub changed_by_device_id: String,
    pub changed_at: String,
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

/// Caller-supplied fields for the `local_outbox` row that
/// [`crate::Db::confirm_order_with_outbox`] writes — mirrors
/// [`OrderItemAddedMeta`]/[`OrderItemRemovedMeta`]. `event_type` (the frozen
/// `OrderConfirmed` string) and `payload_json` (`{ order_id, confirmed_at }`)
/// are owned by the crate, derived from the row it just stamped, so a
/// caller cannot describe a mismatched confirmation. `confirmed_at` is the
/// moment the *edge* recorded the confirmation (sync.md §50.1: the edge is
/// authoritative for order transactions) — this crate does not generate it
/// itself, but it also never lets a cloud-supplied clock anywhere near it;
/// the caller (Tauri command layer) is expected to source it locally.
#[derive(Debug, Clone)]
pub struct OrderConfirmedMeta {
    pub outbox_id: String,
    pub occurred_at: String,
    pub confirmed_at: String,
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

// --------------------------------------------- Milestone 3: billing config --
// compliance_version / tax_profile / tax_rule / outlet_fiscal_profile /
// invoice_series / discount_definition are CONFIG aggregates (ADR-016 §1):
// cloud→edge, versioned by config_version, replaced wholesale — the same
// station/printer precedent from Milestone 2. This crate stores what sync
// gives it and never originates a row in any of the six. Field sets mirror
// `packages/contracts/sqlite/0006_m3_billing.sql` exactly.

/// A named, versioned tax ruleset (`compliance_version`). Invoices pin
/// themselves to one by id so a bill stays reproducible after the rules
/// change (§31) — see `tax::resolve_compliance_version`.
#[derive(Debug, Clone)]
pub struct ComplianceVersion {
    pub id: String,
    pub outlet_id: String,
    pub label: String,
    pub effective_from: String,
    pub notes: Option<String>,
    pub config_version: i64,
}

/// What a menu item (or the outlet default) points at for tax treatment.
/// Never a percentage scattered on the item itself (§31).
#[derive(Debug, Clone)]
pub struct TaxProfile {
    pub id: String,
    pub outlet_id: String,
    pub code: String,
    pub name: String,
    /// `"INCLUSIVE"` or `"EXCLUSIVE"` — matches the `CHECK` in
    /// `0006_m3_billing.sql`. Kept as the raw stored string here, the same
    /// convention as `Order.status`; `tax::domain::PricingMode` is the typed
    /// form the compute engine works in.
    pub pricing_mode: String,
    pub is_default: bool,
    pub is_active: bool,
    pub config_version: i64,
}

/// One component rate inside a profile, effective-dated. Child row
/// travelling in its parent's config bundle (the `menu_item_variant`
/// precedent), not an aggregate of its own.
#[derive(Debug, Clone)]
pub struct TaxRule {
    pub id: String,
    pub tax_profile_id: String,
    pub compliance_version_id: String,
    /// `"CGST" | "SGST" | "IGST" | "CESS"` — the raw stored string;
    /// `tax::domain::TaxComponent` is the typed form.
    pub component: String,
    pub rate_bps: i64,
    pub effective_from: String,
    pub effective_to: Option<String>,
    pub config_version: i64,
}

/// The seller identity printed on a GST invoice (§33), effective-dated.
#[derive(Debug, Clone)]
pub struct OutletFiscalProfile {
    pub id: String,
    pub outlet_id: String,
    pub legal_name: String,
    pub trade_name: String,
    pub address_line1: String,
    pub address_line2: Option<String>,
    pub city: String,
    pub state_code: String,
    pub state_name: String,
    pub pincode: String,
    pub gstin: String,
    pub fssai_number: Option<String>,
    pub invoice_footer_text: Option<String>,
    pub effective_from: String,
    pub config_version: i64,
}

/// The DEFINITION of an invoice number series (ADR-016 §2). The counter
/// (`invoice_sequence`) is edge-local and out of this track's scope (T7b).
#[derive(Debug, Clone)]
pub struct InvoiceSeries {
    pub id: String,
    pub outlet_id: String,
    pub code: String,
    pub prefix_template: String,
    pub reset_policy: String,
    pub padding_width: i64,
    pub is_active: bool,
    pub config_version: i64,
}

/// A discount a cashier may apply (§28). Exactly one of `value_bps`/
/// `value_paise` is set, decided by `method` — enforced by a `CHECK` at the
/// schema layer, mirrored here only as the storage shape, not re-validated.
#[derive(Debug, Clone)]
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
}

// --------------------------------------------- Milestone 3: invoicing (T7b) --
// invoice / invoice_line are EDGE-AUTHORITATIVE (ADR-016 §1): the outlet
// issues bills with the uplink down, the cloud only replays. invoice_sequence
// is EDGE-LOCAL (§2): SQLite only, no Postgres mirror, no AggregateType, no
// sync direction, ever — the counter behind invoice numbering never leaves
// this machine. Field sets mirror `packages/contracts/sqlite/0006_m3_billing.sql`
// exactly.

/// The edge-local counter behind one `(series_id, period_key)` bucket
/// (ADR-016 §2). Never constructed directly by a caller outside this
/// crate — [`crate::invoice::numbering`] is the only writer, via an atomic
/// `INSERT ... ON CONFLICT ... RETURNING` inside the same transaction as the
/// invoice it numbers.
#[derive(Debug, Clone)]
pub struct InvoiceSequence {
    pub series_id: String,
    pub period_key: String,
    pub last_value: i64,
    pub updated_at: String,
}

/// One order line's SHARE to bill on a particular invoice. For an unsplit
/// bill there is exactly one share per `order_item`, at its full quantity.
/// For a split bill (ADR-016 §4) one `order_item` may appear across several
/// shares, one per split part, whose quantities must sum to the order
/// item's own `quantity` exactly — the conservation property
/// [`crate::invoice::assemble::validate_split_conservation`] checks before
/// anything is written. `order_item_id` is what makes that property
/// checkable at all (0006's own comment on `invoice_line.order_item_id`).
#[derive(Debug, Clone)]
pub struct InvoiceLineShare {
    /// This `invoice_line`'s own id — caller-supplied (UUIDv7), matching
    /// every other `New*` row this crate writes except the KOT-fanout
    /// exception documented on [`SendToKitchenMeta`]. The number of lines an
    /// issuance produces IS known to the caller ahead of time (one per
    /// share it supplies), so that exception does not apply here.
    pub id: String,
    pub order_item_id: String,
    /// The quantity THIS share bills — never re-derived from the order
    /// item's own `quantity`, since a split share is usually less than it.
    pub quantity: i64,
    /// Per-unit discount already resolved to paise by the caller. Discount
    /// POLICY (`discount_definition` resolution) is out of T7b's scope —
    /// this crate only applies a number it is given, through the same
    /// tax-engine path as everything else.
    pub discount_per_unit_paise: i64,
}

/// Caller-supplied fields to issue ONE invoice — either the sole invoice
/// over an order, or one part of a split group (ADR-016 §4). `lines` names
/// which order items (and what quantity of each) this invoice bills; this
/// crate resolves each line's tax profile/rates itself (via
/// [`crate::tax::resolve_tax_profile`]/[`crate::tax::resolve_rates`]) rather
/// than trusting a caller-supplied resolution, mirroring how
/// `add_order_item_with_outbox` never trusts a caller-supplied
/// `line_total_paise`.
#[derive(Debug, Clone)]
pub struct IssueInvoiceLinesRequest {
    pub invoice_id: String,
    pub lines: Vec<InvoiceLineShare>,
    pub split_index: i64,
    pub split_count: i64,
}

/// Header fields shared by every part of one issuance call — one order, one
/// series, one moment, one biller. Split into its own struct because
/// [`crate::Db::issue_split_invoices_with_outbox`] takes one of these plus
/// N [`IssueInvoiceLinesRequest`]s, rather than repeating the header N times.
#[derive(Debug, Clone)]
pub struct IssueInvoiceHeader {
    pub outlet_id: String,
    pub order_id: String,
    /// `invoice_series.code` (e.g. `"SALES"`) — resolved to the series row
    /// (and, through it, the edge-local counter) inside the same
    /// transaction as the write.
    pub series_code: String,
    /// The moment of issue (ISO8601 UTC), sourced from the edge machine's
    /// own clock by the caller (sync.md §50.1) — never a value handed up
    /// from anywhere else. Also the instant tax rules/compliance version are
    /// resolved AT (§31).
    pub invoice_date: String,
    /// Outlet-local `YYYY-MM-DD`. Drives both the printed `business_date`
    /// column and (via `reset_policy`) which `invoice_sequence` bucket this
    /// issuance counts against.
    pub business_date: String,
    pub customer_name: Option<String>,
    pub customer_phone: Option<String>,
    pub customer_gstin: Option<String>,
    pub place_of_supply_state_code: String,
    pub channel: String,
    pub tax_liability_party: String,
    pub eco_operator_name: Option<String>,
    pub eco_operator_gstin: Option<String>,
    pub supply_classification: Option<String>,
    pub created_by_user_id: String,
}

/// Caller-supplied fields for the `local_outbox` row an invoice issuance
/// writes (the frozen `InvoiceCreated` event, `packages/contracts/src/types/events.ts`).
/// Mirrors [`OrderConfirmedMeta`]: the caller supplies only what this crate
/// cannot derive — the outbox row's own id and the moment it occurred.
#[derive(Debug, Clone)]
pub struct InvoiceOutboxMeta {
    pub outbox_id: String,
    pub occurred_at: String,
}

/// One computed, persisted line on an issued invoice. Field-for-field
/// `invoice_line` (`0006_m3_billing.sql`), plus nothing else — this crate
/// never adds a column the contract does not carry.
#[derive(Debug, Clone)]
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
}

/// One issued (or cancelled) GST invoice. Field-for-field `invoice`
/// (`0006_m3_billing.sql`); `lines` is not a column — callers that need the
/// lines call [`crate::repo::list_invoice_lines`] separately, matching the
/// `Kot`/`KotTicketItem` split already used for kitchen tickets.
#[derive(Debug, Clone)]
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
    pub tax_snapshot_json: String,
    pub fiscal_profile_json: String,
    pub channel: String,
    pub tax_liability_party: String,
    pub eco_operator_name: Option<String>,
    pub eco_operator_gstin: Option<String>,
    pub supply_classification: Option<String>,
    pub created_by_user_id: String,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub sync_status: String,
}

/// Insert shape for [`Invoice`] — identical field set; kept as a separate
/// type (the `NewOrder`/`Order` precedent) so a caller cannot accidentally
/// supply `version`/`sync_status`, which this crate always sets itself
/// (`1`/`'PENDING'`) at insert time.
#[derive(Debug, Clone)]
pub struct NewInvoice {
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
    pub customer_name: Option<String>,
    pub customer_phone: Option<String>,
    pub customer_gstin: Option<String>,
    pub place_of_supply_state_code: String,
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
    pub tax_snapshot_json: String,
    pub fiscal_profile_json: String,
    pub channel: String,
    pub tax_liability_party: String,
    pub eco_operator_name: Option<String>,
    pub eco_operator_gstin: Option<String>,
    pub supply_classification: Option<String>,
    pub created_by_user_id: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Insert shape for [`InvoiceLine`] — same rationale as [`NewInvoice`].
#[derive(Debug, Clone)]
pub struct NewInvoiceLine {
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
}

// --------------------------------------------- Milestone 3: payments (T7c) --
// `payment` and `cash_shift` are EDGE-AUTHORITATIVE (ADR-016 §1): the outlet
// takes money with the uplink down, the cloud only replays. Field sets
// mirror `packages/contracts/sqlite/0006_m3_billing.sql` exactly.
//
// APPEND-ONLY (docs/spec/payments.md §Conflict policy; ADR-016 payment.ts
// header comment): nothing in this crate ever issues an `UPDATE` or `DELETE`
// against the `payment` table. A void or refund is a NEW row carrying
// `reverses_payment_id`, enforced by [`crate::payment::tender`] before any
// write, mirroring the table's own
// `CHECK (reverses_payment_id IS NULL OR amount_paise <= 0)`.

/// One tender (§34) — insert shape. Kept separate from [`Payment`] (the
/// `NewInvoice`/`Invoice` precedent) so a caller cannot supply `version`/
/// `sync_status`, which this crate always sets itself (`1`/`'PENDING'`) at
/// insert time.
#[derive(Debug, Clone)]
pub struct NewPayment {
    pub id: String,
    pub outlet_id: String,
    pub order_id: String,
    /// `NULL` for a non-cash tender taken outside an open shift. A CASH
    /// tender that *is* tied to a shift additionally produces a
    /// `cash_movement` row — see [`crate::payment::tender::record_payment`].
    pub cash_shift_id: Option<String>,
    pub method: String,
    pub status: String,
    /// Positive on a forward tender; non-positive (`<= 0`) on a reversal —
    /// enforced before this ever reaches SQL, not left to the `CHECK` alone
    /// (§64: the caller gets a typed, actionable error, not a generic
    /// constraint failure).
    pub amount_paise: i64,
    pub tendered_paise: Option<i64>,
    pub change_paise: Option<i64>,
    pub reference: Option<String>,
    pub external_id: Option<String>,
    /// `Some(original_payment_id)` marks this row as a reversal. `None` for
    /// every forward tender.
    pub reverses_payment_id: Option<String>,
    pub captured_at: Option<String>,
    pub created_by_user_id: String,
    pub created_at: String,
    pub updated_at: String,
}

/// One tender, as stored. Field-for-field `payment`
/// (`0006_m3_billing.sql`) plus nothing else.
#[derive(Debug, Clone)]
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
    pub created_by_user_id: String,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub sync_status: String,
}

/// Caller-supplied fields for the `local_outbox` row a payment write emits
/// (`PaymentReceived` for a forward tender, `PaymentRefunded` for a
/// reversal — `crate::payment::tender` decides which). Mirrors
/// [`InvoiceOutboxMeta`]: the caller supplies only what this crate cannot
/// derive.
#[derive(Debug, Clone)]
pub struct PaymentOutboxMeta {
    pub outbox_id: String,
    pub occurred_at: String,
}

/// Insert shape for [`PaymentAllocation`] — how one tender settles against
/// one invoice (`payment_allocation`, `0006_m3_billing.sql`; ADR-016 §1).
/// `crate::payment::tender::record_payment` is the only writer: a forward
/// tender allocates against the invoice its caller names (validated against
/// the invoice's remaining due before anything is written, T9 retry); a
/// reversal allocates against whatever invoice its original payment was
/// allocated to, derived automatically rather than trusted from a caller.
#[derive(Debug, Clone)]
pub struct NewPaymentAllocation {
    pub id: String,
    pub payment_id: String,
    pub invoice_id: String,
    /// Same sign as the payment it comes from — positive on a forward
    /// tender's allocation, non-positive on a reversal's.
    pub amount_paise: i64,
}

/// A `payment_allocation` row, as stored. Field-for-field the table plus
/// nothing else.
#[derive(Debug, Clone)]
pub struct PaymentAllocation {
    pub id: String,
    pub payment_id: String,
    pub invoice_id: String,
    pub amount_paise: i64,
}

/// Cashier-specific register (§39) — insert shape for opening a shift.
/// `status`/`closed_at`/`expected_cash_paise`/`actual_cash_paise`/
/// `variance_paise`/`variance_reason` are never caller-supplied at open
/// time: the row starts `'OPEN'` with every close-time field `NULL`, and
/// only [`crate::payment::cash_shift::close_cash_shift`] ever fills them in
/// (a single in-place transition, the same one-writer shape
/// `kot.status`/`transition_kot_status_with_outbox` already uses — a shift,
/// unlike a payment, is a workflow row with exactly one legal transition,
/// not a financial ledger entry, so it is not append-only).
#[derive(Debug, Clone)]
pub struct NewCashShift {
    pub id: String,
    pub outlet_id: String,
    pub device_id: String,
    pub cashier_user_id: String,
    pub opened_at: String,
    pub opening_cash_paise: i64,
    pub business_date: String,
    pub created_at: String,
    pub updated_at: String,
}

/// A cash shift, as stored. Field-for-field `cash_shift` (`0006_m3_billing.sql`)
/// plus nothing else.
#[derive(Debug, Clone)]
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
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub sync_status: String,
}

/// Caller-supplied fields to close an open shift (§39). `actual_cash_paise`
/// is the human count; `expected_cash_paise` is always DERIVED by this
/// crate from the shift's own `cash_movement` rows, never accepted from a
/// caller — the same "never trust a caller-supplied total" discipline
/// `add_order_item_with_outbox` applies to `line_total_paise`.
#[derive(Debug, Clone)]
pub struct CloseCashShiftRequest {
    pub cash_shift_id: String,
    pub actual_cash_paise: i64,
    pub closed_at: String,
    pub updated_at: String,
    /// Mandatory the instant the derived variance is non-zero (§39) —
    /// [`crate::payment::cash_shift::close_cash_shift`] rejects a non-zero
    /// variance whose reason is `None` or whitespace-only BEFORE writing
    /// anything. A zero variance needs no reason, whatever this carries.
    pub variance_reason: Option<String>,
}

/// Caller-supplied fields for the `local_outbox` row a shift open/close
/// emits (`CashShiftOpened`/`CashShiftClosed`).
#[derive(Debug, Clone)]
pub struct CashShiftOutboxMeta {
    pub outbox_id: String,
    pub occurred_at: String,
}

/// Every movement of physical cash through the drawer (§39). Child row
/// inside the shift's payload, append-only: a correction is another
/// movement, never a rewrite of one already posted. Field-for-field
/// `cash_movement` (`0006_m3_billing.sql`).
#[derive(Debug, Clone)]
pub struct CashMovement {
    pub id: String,
    pub cash_shift_id: String,
    pub kind: String,
    /// Signed: `PAID_OUT` and `CASH_REFUND` are negative, everything else
    /// non-negative.
    pub amount_paise: i64,
    pub reason: Option<String>,
    pub payment_id: Option<String>,
    pub created_by_user_id: String,
    pub created_at: String,
}

/// Insert shape for [`CashMovement`] — same rationale as [`NewInvoice`]/
/// [`NewPayment`].
#[derive(Debug, Clone)]
pub struct NewCashMovement {
    pub id: String,
    pub cash_shift_id: String,
    pub kind: String,
    pub amount_paise: i64,
    pub reason: Option<String>,
    pub payment_id: Option<String>,
    pub created_by_user_id: String,
    pub created_at: String,
}

/// A caller-initiated cash drawer movement not backed by a `payment` row —
/// `PAID_IN`/`PAID_OUT` (§39: "Paid In, Paid Out"). `OPENING_FLOAT`,
/// `CASH_SALE` and `CASH_REFUND` are only ever produced internally by this
/// crate (opening a shift / recording a CASH payment / reversing one), never
/// through this entry point, so a caller cannot mint a movement that looks
/// like a sale without a payment behind it.
#[derive(Debug, Clone)]
pub struct PaidInOutRequest {
    pub id: String,
    pub cash_shift_id: String,
    pub kind: String, // "PAID_IN" | "PAID_OUT"
    pub amount_paise: i64,
    pub reason: String,
    pub created_by_user_id: String,
    pub created_at: String,
}

// ------------------------------------------- device_credential_cache (0.4.3) --
// CONFIG, cloud->edge (ADR-011 pattern extended to devices, ADR-017
// amendment). Mirrors `packages/contracts/sqlite/0008_edge_device_credential_
// cache.sql` exactly. `credential_hash` is a VERIFIER (Argon2id over the
// device's secret), never a bearer token — never log or place this struct's
// `credential_hash` field in an error. A revoked or expired credential STILL
// SYNCS and is stored as-is; rejection is decided by `revoked_at`/
// `expires_at`, never by whether a row exists (see repo::get_device_
// credential_cache_by_id's doc comment).
#[derive(Debug, Clone)]
pub struct DeviceCredentialCache {
    pub credential_id: String,
    pub device_id: String,
    pub tenant_id: String,
    pub outlet_id: String,
    pub credential_hash: String,
    pub device_kind: String,
    pub revoked_at: Option<String>,
    pub expires_at: Option<String>,
    pub config_version: i64,
}

// ------------------------ inventory_item / recipe config (M4, ADR-018) -----
// CONFIG, cloud->edge (`packages/contracts/sqlite/0015_m4_inventory_config.sql`,
// `0019_recipe_output.sql`, `0020_recipe_ingredient_dimension.sql`).
// `inventory_item` and `recipe` are aggregates; the other three are CHILD
// ROWS that ride inside their parent's config bundle and carry no sync
// direction of their own — the menu_item_variant/station_printer precedent.

/// A raw material or purchasable unit. `dimension` fixes which canonical
/// unit every `*_micro` value referencing this item means, and is frozen
/// once any `recipe_ingredient` references the item (the edge-side trigger
/// in 0020). `yield_factor_ppm` is DEFERRED to M5 — always the identity
/// (1_000_000) here; see the column's own doc comment in 0015.
#[derive(Debug, Clone)]
pub struct InventoryItem {
    pub id: String,
    pub outlet_id: String,
    pub sku: String,
    pub name: String,
    pub category: Option<String>,
    /// `"MASS" | "VOLUME" | "COUNT"` — [`crate::inventory::Dimension`].
    pub dimension: String,
    pub reorder_level_micro: Option<i64>,
    pub par_level_micro: Option<i64>,
    pub storage_location: Option<String>,
    pub is_active: bool,
    pub yield_factor_ppm: i64,
    pub config_version: i64,
}

/// A pack-size or cross-dimension (density) conversion, scoped to one
/// `inventory_item` — never a global unit, because a packet size or a
/// density is a property of the substance, not a physical constant
/// (0015's header). `pack_unit_label` must never collide with the frozen
/// dimensional map (kg/g/ml/l/piece/dozen/...) — the edge CHECK this
/// crate's schema carries rejects that at write time.
#[derive(Debug, Clone)]
pub struct ItemUnitConversion {
    pub id: String,
    pub inventory_item_id: String,
    pub pack_unit_label: String,
    /// `"MASS" | "VOLUME" | "COUNT"` — the dimension the pack label is
    /// measured IN, which need not equal the item's own `dimension`
    /// (oil bought in kg, cooked in ml).
    pub source_dimension: String,
    pub numerator: i64,
    pub denominator: i64,
    pub config_version: i64,
}

/// One recipe per sellable `menu_item_variant`. `output_dimension`/
/// `output_quantity_micro` (0.5.1) are what a `SUB_RECIPE` reference into
/// this recipe is measured against — never a dimensionless multiplier; see
/// `crate::inventory::resolve`'s module doc comment for the formula this
/// unlocks.
#[derive(Debug, Clone)]
pub struct Recipe {
    pub id: String,
    pub menu_item_variant_id: String,
    pub name: String,
    pub recipe_version: i64,
    /// `"MASS" | "VOLUME" | "COUNT"`.
    pub output_dimension: String,
    pub output_quantity_micro: i64,
    pub config_version: i64,
}

/// One component of a recipe — either a raw material or a sub-recipe,
/// never both (the table's own CHECK). `quantity_dimension` is the unit the
/// AUTHOR chose when writing this row and must NEVER be derived from the
/// referenced item's/recipe's own dimension — see 0020's header and
/// `crate::inventory::resolve`'s module doc comment for why deriving it
/// makes the drift guard tautological and unable to ever fire.
#[derive(Debug, Clone)]
pub struct RecipeIngredient {
    pub id: String,
    pub recipe_id: String,
    /// `"ITEM" | "SUB_RECIPE"`.
    pub component_kind: String,
    pub inventory_item_id: Option<String>,
    pub sub_recipe_id: Option<String>,
    /// Positive: a recipe consumes. `CHECK (> 0)` at the schema — negative
    /// deltas are a `modifier_ingredient_delta` concept, not this table's.
    pub quantity_micro: i64,
    /// The unit the author actually wrote the quantity in — see the struct
    /// doc comment. Compared against the referent's live dimension only at
    /// resolution time, never here.
    pub quantity_dimension: String,
    pub yield_factor_ppm: i64,
    pub sort_order: i64,
    pub config_version: i64,
}

/// A modifier's effect on stock — SIGNED: "Extra Paneer" is positive, "No
/// Onion" is negative. Absence of a row for a given modifier is not zero,
/// it is "uncosted" — the deduction path must never fabricate one, per the
/// table's own header (the 0.4.7 `printer_role` precedent applied to
/// ingredients).
#[derive(Debug, Clone)]
pub struct ModifierIngredientDelta {
    pub id: String,
    pub menu_item_modifier_id: String,
    pub inventory_item_id: String,
    pub quantity_micro: i64,
    pub config_version: i64,
}

// -------------------------------------- stock_ledger_entry / gap (M4, T2) --
// EDGE-AUTHORITATIVE (edge->cloud), ADR-018 §6/§10.1. See
// `packages/contracts/sqlite/0016_m4_stock_ledger.sql`'s header for the four
// rules every consumer of these two tables must keep; `crate::deduction` is
// their only writer.

/// Caller-supplied fields to insert one `stock_ledger_entry` row. `id` and
/// `entry_seq` are minted by [`crate::deduction::ledger`] at insert time —
/// not by the caller — the same reason `NewOrder`'s KOT ids are minted
/// inside this crate rather than accepted: `entry_seq` is a per-outlet
/// monotonic mark assigned atomically in the SAME transaction as the insert
/// (`UNIQUE (outlet_id, entry_seq)`, the `invoice_sequence` pattern), so a
/// value chosen outside that transaction could never be guaranteed gapless.
///
/// Every field here is a direct mirror of the 0016 column of the same name;
/// see that migration's comments for what each one means and why the row
/// carries no foreign keys. `quantity_applied_micro` is SIGNED — negative
/// for consumption — and already carries its sign; this struct does not
/// re-derive or re-check it.
#[derive(Debug, Clone)]
pub struct NewStockLedgerEntry {
    pub outlet_id: String,
    pub inventory_item_id: String,
    pub inventory_item_name: String,
    /// `"MASS" | "VOLUME" | "COUNT"` — [`crate::inventory::Dimension::as_str`].
    pub dimension: String,
    /// `"PURCHASE" | "CONSUMPTION" | "WASTAGE" | "TRANSFER_IN" |
    /// "TRANSFER_OUT" | "ADJUSTMENT" | "RETURN_TO_VENDOR" |
    /// "PRODUCTION_CONSUMPTION" | "PRODUCTION_OUTPUT"`.
    pub entry_type: String,
    /// `"RECIPE" | "MODIFIER_DELTA" | "MANUAL" | "COUNT_ADJUSTMENT" |
    /// "WASTAGE"` — fixes which of the two provenance groups below must be
    /// populated (the table's own CHECK, mirrored by
    /// [`crate::deduction::ledger::insert_stock_ledger_entry`]'s caller).
    pub origin: String,
    pub quantity_applied_micro: i64,
    pub recipe_id: Option<String>,
    pub recipe_version: Option<i64>,
    pub recipe_name: Option<String>,
    pub source_order_id: Option<String>,
    pub source_order_item_id: Option<String>,
    pub reason_code: Option<String>,
    pub note: Option<String>,
    pub occurred_at: String,
    /// Computed ONCE by [`crate::deduction::business_date::compute_business_date`]
    /// before this struct is built — never recomputed inside the insert.
    pub business_date: String,
    pub created_by_user_id: Option<String>,
    pub modifier_delta_id: Option<String>,
    pub modifier_name: Option<String>,
    pub modifier_delta_version: Option<i64>,
    /// Paise per BASE unit. **This is the field ADR-018 sec.8 deferred to M5
    /// and contracts 0.6.0 finally gives a writer.** It stayed `None` on
    /// every row the product wrote for a whole milestone -- the column
    /// existed, nothing set it, and 0.6.0 removed its exemption from
    /// `scripts/check-contract-field-consumers.mjs` on the strength of this
    /// write path existing.
    ///
    /// `None` on every sale-, wastage- and count-posted row, deliberately:
    /// those consume at the average rather than setting it, and a cost on
    /// them would let issuing stock move the purchase price
    /// (`crate::procurement::cost`).
    pub unit_cost_paise: Option<i64>,
    /// Contracts 0.6.3 (ADR-021): the EXACT money this row is worth, unrounded,
    /// as invoiced — and the ONLY input to weighted average cost.
    ///
    /// `Some(_)` on a receipt and NOWHERE ELSE. `unit_cost_paise` is a derived
    /// per-base-unit RATE, rounded to whole paise once per receipt, so a
    /// weighted average summed from rates inherits a rounding it can never
    /// recover (+/-0.5 paise per gram is +20% at 2.5 paise/g). Only a receipt
    /// has an invoiced total; wastage, counts, variance and outbound movements
    /// are valued AT the average, so writing `quantity x rate` for them would
    /// fabricate precision and feed it back into the average that produced it.
    pub line_total_paise: Option<i64>,
    /// Contracts 0.5.5 (`packages/contracts/sqlite/0023_stock_count_integrity.sql`):
    /// typed provenance for a `COUNT_ADJUSTMENT` row, no FK (the same
    /// no-FK provenance discipline as `recipe_id`/`source_order_id`) —
    /// `Some(_)` only when `origin == "COUNT_ADJUSTMENT"`
    /// ([`crate::stock::count::complete_stock_count`]); `None` everywhere
    /// else. Replaces the `"stock_count:{id}"` string this crate used to
    /// write into `note` before this column existed.
    pub source_stock_count_id: Option<String>,
    /// Contracts 0.6.0 (`packages/contracts/sqlite/0027_m5_procurement.sql`):
    /// typed, no-FK provenance for the three procurement sources, matching
    /// the `source_stock_count_id` shape exactly. Exactly one of the three is
    /// `Some` on any procurement-posted row; all three are `None` on every
    /// sale-, wastage- and count-posted row.
    ///
    /// The 0.5.9 lesson is why these are here at the same time as the write
    /// path that sets them: a column nothing reads is a column that does not
    /// exist, and a fidelity test proves fidelity only for the fields its
    /// fixture populates.
    pub source_grn_id: Option<String>,
    pub source_purchase_return_id: Option<String>,
    pub source_stock_transfer_out_id: Option<String>,
}

/// Caller-supplied fields to insert one `stock_deduction_gap` row — a
/// SIGNAL, never backfilled once the recipe is authored (ADR-018 §10.1).
/// `id` is minted by [`crate::deduction::ledger`], matching every other
/// operational insert in this crate.
#[derive(Debug, Clone)]
pub struct NewStockDeductionGap {
    pub outlet_id: String,
    pub order_id: String,
    pub order_item_id: String,
    pub menu_item_id: String,
    pub menu_item_variant_id: Option<String>,
    pub menu_item_name: String,
    pub quantity: i64,
    /// [`crate::inventory::GapReason::as_str`] — `"NO_RECIPE"`,
    /// `"NO_VARIANT"`, `"CYCLE"`, `"DEPTH_EXCEEDED"`, `"UNKNOWN_UNIT"`, or
    /// `"DIMENSION_MISMATCH"`.
    pub reason: String,
    pub occurred_at: String,
    pub business_date: String,
}

/// A `stock_deduction_gap` row, as stored — field-for-field the table, the
/// read counterpart of [`NewStockDeductionGap`]. Returned by
/// [`crate::Db::list_stock_deduction_gaps`], the M4 acceptance-criterion-5
/// ("items sold with no recipe") report's sanctioned data source — the only
/// prior reader was a private `#[cfg(test)]` helper, unreachable from
/// outside this crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StockDeductionGap {
    pub id: String,
    pub outlet_id: String,
    /// The per-outlet replay mark (contracts 0.5.8), minted from
    /// `stock_deduction_gap_sequence` in the same transaction as the insert.
    /// SEPARATE from the ledger's counter: two ranged streams, two counters,
    /// two cursors. 1-based, so a cursor of 0 means "nothing acked".
    pub entry_seq: i64,
    pub order_id: String,
    pub order_item_id: String,
    pub menu_item_id: String,
    pub menu_item_variant_id: Option<String>,
    pub menu_item_name: String,
    /// Sellable units sold unaccounted, not a micro-quantity — nothing was
    /// resolved to an ingredient, which is the point of the row.
    pub quantity: i64,
    /// `"NO_RECIPE"`, `"NO_VARIANT"`, `"CYCLE"`, `"DEPTH_EXCEEDED"`,
    /// `"UNKNOWN_UNIT"`, `"DIMENSION_MISMATCH"`, or
    /// `"UNRESOLVABLE_REFERENCE"`.
    pub reason: String,
    pub occurred_at: String,
    pub business_date: String,
}

/// A `stock_ledger_entry`, as stored — field-for-field the table plus
/// nothing else, the read counterpart of [`NewStockLedgerEntry`]. Returned
/// by [`crate::stock::wastage::record_wastage`] and by the completed lines
/// of [`crate::stock::count::complete_stock_count`] so a caller can display
/// what was actually written without a second query.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub source_order_id: Option<String>,
    pub source_order_item_id: Option<String>,
    pub reason_code: Option<String>,
    pub note: Option<String>,
    pub occurred_at: String,
    pub business_date: String,
    pub created_by_user_id: Option<String>,
    pub modifier_delta_id: Option<String>,
    pub modifier_name: Option<String>,
    pub modifier_delta_version: Option<i64>,
    pub unit_cost_paise: Option<i64>,
    /// Contracts 0.6.3 — see [`NewStockLedgerEntry::line_total_paise`].
    pub line_total_paise: Option<i64>,
    /// Contracts 0.5.5 — see [`NewStockLedgerEntry::source_stock_count_id`].
    pub source_stock_count_id: Option<String>,
    /// Contracts 0.6.0 (`packages/contracts/sqlite/0027_m5_procurement.sql`):
    /// typed, no-FK provenance for the three procurement sources, matching
    /// the `source_stock_count_id` shape exactly. Exactly one of the three is
    /// `Some` on any procurement-posted row; all three are `None` on every
    /// sale-, wastage- and count-posted row.
    ///
    /// The 0.5.9 lesson is why these are here at the same time as the write
    /// path that sets them: a column nothing reads is a column that does not
    /// exist, and a fidelity test proves fidelity only for the fields its
    /// fixture populates.
    pub source_grn_id: Option<String>,
    pub source_purchase_return_id: Option<String>,
    pub source_stock_transfer_out_id: Option<String>,
}

// ------------------------------ wastage / stock counts / variance (M4, T3) --
// `stock_count`/`stock_count_line` are EDGE-AUTHORITATIVE (edge->cloud;
// `stock_count` is the AggregateType, the line rides as a child row — the
// `invoice`/`invoice_line` precedent). Wastage has NO table of its own: it
// is one more `stock_ledger_entry` row (`entry_type='WASTAGE'`,
// `origin='WASTAGE'`), exactly as the 0016 migration's own `entry_type`
// list documents.

/// Caller-supplied fields to record one wastage event. `quantity_micro` is
/// the MAGNITUDE lost (always `> 0`, rejected otherwise —
/// [`crate::error::DbError::WastageQuantityNotPositive`]); the negative
/// sign that makes it a consumption is applied inside
/// [`crate::stock::wastage::record_wastage`], not carried by the caller.
/// `reason_code` is mandatory and non-blank
/// ([`crate::error::DbError::WastageReasonRequired`]) — an append-only
/// shortfall with no named cause reads as theft.
///
/// `business_date` is deliberately ABSENT here: it is computed once,
/// internally, from `occurred_at` and the outlet's own
/// `timezone`/`day_start_time` — the same discipline
/// `deduction::ledger::deduct_stock_for_confirmed_order` already applies to
/// every other M4 stock write, never accepted from a caller (ADR-018 §9.2).
#[derive(Debug, Clone)]
pub struct NewWastageEntry {
    pub outlet_id: String,
    pub inventory_item_id: String,
    pub quantity_micro: i64,
    pub reason_code: String,
    pub note: Option<String>,
    pub occurred_at: String,
    pub created_by_user_id: Option<String>,
}

/// Caller-supplied fields to open a physical stock count. `business_date` is
/// likewise absent — computed once, internally, from `started_at` (ADR-018
/// §9.2), never accepted from a caller.
#[derive(Debug, Clone)]
pub struct NewStockCount {
    pub id: String,
    pub outlet_id: String,
    pub started_at: String,
    pub counted_by_user_id: Option<String>,
    pub note: Option<String>,
}

/// Caller-supplied identity for a `StockCountOpened`/`StockCountCompleted`
/// `local_outbox` row — the [`CashShiftOutboxMeta`] shape, generalised. The
/// caller supplies only what this crate cannot derive: the outbox row's own
/// id and the moment the event occurred; every other field of the payload
/// is built by this crate from the row it just wrote, so a caller cannot
/// commit a mismatched or misleading event.
#[derive(Debug, Clone)]
pub struct StockCountOutboxMeta {
    pub outbox_id: String,
    pub occurred_at: String,
}

/// A `stock_count`, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StockCount {
    pub id: String,
    pub outlet_id: String,
    pub business_date: String,
    /// `"OPEN" | "COMPLETED"`.
    pub status: String,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub counted_by_user_id: Option<String>,
    pub note: Option<String>,
}

/// Caller-supplied fields to add or correct one counted line on an OPEN
/// count. `expected_quantity_micro` is deliberately absent: it is derived
/// from the bounded stock read AT THE MOMENT OF COUNTING
/// ([`crate::stock::snapshot::get_current_stock_in_tx`]) inside
/// [`crate::stock::count::add_or_update_count_line`], never accepted from a
/// caller and never recomputed once written — ADR-018's "snapshotted at the
/// moment of counting, never recomputed" rule.
#[derive(Debug, Clone)]
pub struct NewStockCountLine {
    pub inventory_item_id: String,
    pub counted_quantity_micro: i64,
    pub note: Option<String>,
}

/// A `stock_count_line`, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StockCountLine {
    pub id: String,
    pub stock_count_id: String,
    pub inventory_item_id: String,
    pub inventory_item_name: String,
    pub dimension: String,
    pub counted_quantity_micro: i64,
    pub expected_quantity_micro: i64,
    pub note: Option<String>,
}

/// One line of a variance report: Actual (counted) vs Theoretical
/// (expected), as quantity and as a basis-point percentage — never a float
/// (§31/0006's `rate_bps` precedent, generalised the way ADR-018 §3
/// generalised money-is-paise to quantity-is-micro). `variance_percentage_bps`
/// is `None` when `expected_quantity_micro` is zero: a percentage OF zero
/// theoretical stock is undefined, not zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StockCountVarianceLine {
    pub inventory_item_id: String,
    pub inventory_item_name: String,
    pub dimension: String,
    pub counted_quantity_micro: i64,
    pub expected_quantity_micro: i64,
    /// `counted_quantity_micro - expected_quantity_micro`. Positive: more
    /// was found than theory predicted. Negative: shrinkage.
    pub variance_quantity_micro: i64,
    pub variance_percentage_bps: Option<i64>,
}

/// A completed count's variance report — DERIVED, never stored as
/// authoritative (ADR-018: "the ledger is the only source of stock").
/// `sales_unaccounted` is the named term ADR-018 §10.1 requires: the number
/// of sellable units sold with no resolvable recipe, up to and including
/// this count's own `business_date`, at this outlet — reported standalone
/// rather than folded into any line's shrinkage, because a
/// `stock_deduction_gap` carries no `inventory_item_id` to attribute it to
/// (that is the entire reason the gap exists: nothing was resolved).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StockCountVarianceReport {
    pub stock_count_id: String,
    pub business_date: String,
    pub lines: Vec<StockCountVarianceLine>,
    pub sales_unaccounted: i64,
}

/// One row of the bounded, outlet-wide current-stock read (ADR-018 §9) —
/// what T5's low-stock surfacing reads from. `current_quantity_micro` is
/// ALWAYS `latest_sealed_snapshot.closing + entries not covered by its
/// mark`, per [`crate::stock::snapshot`]; it is never a stored column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentStockLine {
    pub inventory_item_id: String,
    pub inventory_item_name: String,
    pub dimension: String,
    pub current_quantity_micro: i64,
    pub reorder_level_micro: Option<i64>,
    pub par_level_micro: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct SyncState {
    pub outlet_id: String,
    pub last_pushed_outbox_id: Option<String>,
    pub last_applied_config_version: i64,
    /// Ranged-replay cursors (contracts 0.5.8): the highest `entry_seq` the
    /// cloud has acknowledged for each stream. `entry_seq > cursor` is the
    /// send set. Two streams advance at wildly different rates and mint from
    /// two independent counters, so one mark cannot serve both.
    pub last_acked_ledger_entry_seq: i64,
    pub last_acked_gap_entry_seq: i64,
    pub last_sync_attempt_at: Option<String>,
    pub last_sync_success_at: Option<String>,
    pub is_online: bool,
}

/// One ranged-replay entry this outlet has given up on sending
/// (`sync_replay_block`, contracts 0.5.8) — the human-visible half of the
/// per-entry retry bound.
///
/// WHY THE BOUND IS PER ENTRY. If the cloud permanently rejects entry 7 and
/// the edge retries 7 forever, 8..N never leave the outlet: one bad row
/// becomes an outage, the mirror image of a contiguity check that rejects
/// rather than records. After the budget is spent the cursor moves past the
/// entry and the stream continues; the skipped mark then shows up at the
/// cloud as a hole in `ledger_replay_gap`, so the same fact is visible from
/// both ends.
///
/// Halting sync is survivable — no core outlet path depends on the uplink
/// (ADR-013). Halting it silently is not, which is why this is a row rather
/// than a log line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncReplayBlock {
    pub outlet_id: String,
    /// `"LEDGER"` or `"DEDUCTION_GAP"` — see `repo::ReplayStream`.
    pub stream: String,
    pub entry_seq: i64,
    /// The row that could not be sent, so a human chasing this has something
    /// to look up rather than an ordinal.
    pub record_id: String,
    pub attempts: i64,
    /// The cloud's last word: an HTTP status, or `None` when the row never
    /// got as far as the wire.
    pub last_status: Option<i64>,
    pub last_error: String,
    pub first_attempt_at: String,
    pub last_attempt_at: String,
    /// `None` while the entry is still inside its retry budget. `Some` once
    /// the budget is spent and the cursor has moved past it.
    pub blocked_at: Option<String>,
}

// ---------------------------------- Milestone 5: procurement (T2, ADR-019) --
// `goods_receipt_note`, `purchase_return` and `stock_transfer_out` are
// EDGE-AUTHORITATIVE (edge->cloud); `grn_line`, `purchase_return_line` and
// `stock_transfer_line` ride as child rows; `grn_gap` is its own aggregate on
// a PLAIN outbox (no entry_seq, no cursor -- ADR-019 sec.2); `grn_sequence`
// is edge-local and never leaves the outlet.
//
// THE RULE EVERY TYPE BELOW SERVES: **a GRN never blocks on a PO.** Nothing
// in this group can reject a receipt for a business/config reason. The only
// rejections are malformed caller input (a non-positive quantity, an absurd
// magnitude) -- the same posture `NewWastageEntry` takes, and for the same
// reason: a caller defect is not a shop-floor condition.

/// The largest quantity or money magnitude this crate will accept from a
/// caller or produce from a conversion: `2^53 - 1`, JavaScript's exact
/// integer limit.
///
/// **The binding range limit is JavaScript's 2^53, not `i64`** (ADR-018,
/// CLAUDE.md). Every quantity written here crosses a Tauri IPC boundary into
/// a JS `number`; a value that fits `i64` but not `2^53` arrives in the POS
/// silently rounded, which is precisely the "wrong-but-plausible" failure the
/// micro-unit discipline exists to prevent. Checked at the edge because the
/// edge is the only side that can still tell the difference.
pub const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

/// One line of a goods receipt, as the operator entered it.
///
/// **`quantity_dimension` is the unit the AUTHOR chose and must never be
/// auto-filled from `inventory_item.dimension`** (contracts 0.5.2/0.6.0,
/// ADR-019). If a write path or a UI derives it from the referent the
/// comparison becomes `x == x`, the `DIMENSION_MISMATCH` guard can never
/// fire, and it will look correct in review. It is a required field on this
/// struct -- not an `Option` with a fallback -- so no caller can decline to
/// state it.
#[derive(Debug, Clone)]
pub struct NewGrnLine {
    pub inventory_item_id: String,
    /// The supplier's own unit label off the delivery note -- `"SACK"`,
    /// `"CRATE"`, `"kg"`. Free text; the CONVERSION is what must be exact.
    pub entered_purchase_unit: String,
    /// WHAT THE HUMAN TYPED, in micro-units of `entered_purchase_unit`
    /// (3 sacks = `3_000_000`). Stored verbatim alongside the converted
    /// figure so "what did they actually type?" is answerable from the row
    /// when a receipt turns out 1000x wrong (ADR-019).
    pub entered_quantity_micro: i64,
    /// `"MASS" | "VOLUME" | "COUNT"`. See the struct doc comment.
    pub quantity_dimension: String,
    /// Paise for ONE `entered_purchase_unit` -- the figure on the delivery
    /// note or supplier invoice. The per-base-unit cost the ledger carries is
    /// DERIVED from this at the edge and never recomputed anywhere else
    /// (CLAUDE.md: money is computed by the edge, formatted by the layers
    /// above it).
    pub purchase_price_paise: i64,
    /// Batch and expiry: captured at receipt or never (ADR-019). Nothing in
    /// M5 reads them; M6's expiry alerting does.
    pub batch_code: Option<String>,
    pub expiry_date: Option<String>,
    /// The PO line this receipt line answers, when the caller already knows
    /// it. `None` is ordinary, not an error: the receipt path resolves the
    /// match itself and records a gap when there is none.
    pub purchase_order_line_id: Option<String>,
}

/// A goods receipt as submitted by the receiving screen. `grn_number` is
/// deliberately ABSENT -- it is minted from the edge-local `grn_sequence`
/// inside the same transaction (`invoice_sequence` precedent), never accepted
/// from a caller. `business_date` is likewise absent: computed once,
/// internally, from `received_at` through the one business-date function.
#[derive(Debug, Clone)]
pub struct NewGoodsReceiptNote {
    pub id: String,
    pub outlet_id: String,
    /// `None` is the walk-in / standing-order / emergency-purchase case and
    /// is ACCEPTED -- it records a `NO_PURCHASE_ORDER` gap. A `Some` naming a
    /// PO this edge has never seen is accepted too, as
    /// `PURCHASE_ORDER_NOT_FOUND`.
    pub purchase_order_id: Option<String>,
    /// Same rule: an unconfigured supplier is a `SUPPLIER_NOT_FOUND` gap,
    /// never a refusal.
    pub supplier_id: Option<String>,
    pub delivery_note_ref: Option<String>,
    pub received_at: String,
    pub received_by_user_id: String,
    pub notes: Option<String>,
    pub lines: Vec<NewGrnLine>,
}

/// A `goods_receipt_note` as stored, with the lines and gaps the receipt
/// actually produced. Returned whole so the receiving screen can show what
/// was recorded -- including every gap -- without a second query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoodsReceiptNote {
    pub id: String,
    pub outlet_id: String,
    pub purchase_order_id: Option<String>,
    pub supplier_id: Option<String>,
    pub grn_number: String,
    pub delivery_note_ref: Option<String>,
    pub received_at: String,
    pub received_by_user_id: String,
    pub business_date: String,
    pub notes: Option<String>,
    pub lines: Vec<GrnLine>,
    pub gaps: Vec<GrnGap>,
}

/// A `grn_line` as stored -- field-for-field, both sides of the conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrnLine {
    pub id: String,
    pub grn_id: String,
    pub inventory_item_id: String,
    pub line_number: i64,
    pub purchase_order_line_id: Option<String>,
    pub entered_purchase_unit: String,
    pub entered_quantity_micro: i64,
    pub quantity_dimension: String,
    pub base_quantity_micro: i64,
    /// THE RATE ACTUALLY APPLIED, snapshotted: base micro-units produced by
    /// ONE `entered_purchase_unit`, with `inventory_item.yield_factor_ppm`
    /// already folded in. Storing the effective rate rather than the raw
    /// `supplier_item.pack_size_micro` is what makes the row's own arithmetic
    /// reproducible -- `base_quantity_micro` is derived from THIS number, so
    /// a later edit to the supplier's pack size or to the item's yield cannot
    /// restate a past receipt. See `crate::procurement::convert` for the
    /// two-step rounding argument.
    pub pack_size_micro_applied: i64,
    /// Paise per BASE unit -- the figure the `stock_ledger_entry` carries and
    /// the one weighted average cost is derived from.
    pub unit_cost_paise: i64,
    pub line_total_paise: i64,
    pub batch_code: Option<String>,
    pub expiry_date: Option<String>,
}

/// A `grn_gap` as stored. `detail` is PROSE because a person reads it: M5
/// acceptance criterion 3 is "the gap is visible to a human on the POS", not
/// "a gap row exists" (ADR-019).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrnGap {
    pub id: String,
    pub outlet_id: String,
    pub grn_id: String,
    pub grn_line_id: Option<String>,
    pub inventory_item_id: Option<String>,
    /// One of the eight closed-set reasons -- see
    /// `crate::procurement::GrnGapReason`.
    pub reason: String,
    pub detail: Option<String>,
    pub occurred_at: String,
    pub business_date: String,
}

/// What the receiving screen must show the operator BEFORE they commit
/// (`entryIntentEcho`, ADR-019, M5 acceptance criterion 4): the typed figure,
/// the rate that will be applied, and the base-unit quantity that will
/// actually be recorded.
///
/// Computed by the same function the write path uses, never by a parallel
/// implementation in the UI -- an echo derived independently from the write
/// is an echo that can disagree with it, which is worse than no echo at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrnEntryIntentEcho {
    pub inventory_item_id: String,
    pub inventory_item_name: String,
    pub entered_purchase_unit: String,
    pub entered_quantity_micro: i64,
    pub quantity_dimension: String,
    pub pack_size_micro_applied: i64,
    pub base_quantity_micro: i64,
    /// The dimension the ledger row will carry -- `inventory_item.dimension`,
    /// shown SEPARATELY from `quantity_dimension` so an operator can see the
    /// two disagree. Never used to fill `quantity_dimension` in.
    pub item_dimension: String,
    pub unit_cost_paise: i64,
    pub line_total_paise: i64,
    /// Every gap this line WOULD record if committed as entered. Shown, not
    /// used to block.
    pub gap_reasons: Vec<String>,
}

/// Caller-supplied identity for a procurement `local_outbox` row -- the
/// [`StockCountOutboxMeta`] shape. The caller supplies only the outbox row's
/// own id and the moment; every payload field is built by this crate from the
/// rows it just wrote, so a caller cannot commit a misleading event.
#[derive(Debug, Clone)]
pub struct ProcurementOutboxMeta {
    pub outbox_id: String,
    pub occurred_at: String,
}

/// One line of a purchase return. Same both-sides-of-the-conversion
/// discipline as [`NewGrnLine`]; `purchase_return_line` carries no
/// `pack_size_micro_applied` column, which is reported as a contract
/// asymmetry rather than worked around.
#[derive(Debug, Clone)]
pub struct NewPurchaseReturnLine {
    pub inventory_item_id: String,
    pub grn_line_id: Option<String>,
    pub entered_purchase_unit: String,
    pub entered_quantity_micro: i64,
    /// The unit the author chose. Never derived from the referent.
    pub quantity_dimension: String,
    /// Paise per BASE unit. `None` means "value it at what this outlet
    /// actually paid" -- the weighted average cost derived from the ledger
    /// (`crate::procurement::cost`), never a guess and never a silent zero.
    pub unit_cost_paise: Option<i64>,
}

/// A purchase return as submitted. `return_number` is caller-supplied:
/// contracts 0.6.0 mints a counter for the GRN (`grn_sequence`) and none for
/// this document, so this crate does not invent one -- see
/// `crate::procurement::numbering`.
#[derive(Debug, Clone)]
pub struct NewPurchaseReturn {
    pub id: String,
    pub outlet_id: String,
    pub supplier_id: Option<String>,
    pub grn_id: Option<String>,
    pub return_number: String,
    /// `"DAMAGED" | "EXPIRED" | "WRONG_ITEM" | "QUALITY" | "OVER_DELIVERY" |
    /// "OTHER"`.
    pub reason: String,
    pub returned_at: String,
    pub returned_by_user_id: String,
    pub notes: Option<String>,
    pub lines: Vec<NewPurchaseReturnLine>,
}

/// A `purchase_return` as stored, with its lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurchaseReturn {
    pub id: String,
    pub outlet_id: String,
    pub supplier_id: Option<String>,
    pub grn_id: Option<String>,
    pub return_number: String,
    pub reason: String,
    pub returned_at: String,
    pub returned_by_user_id: String,
    pub business_date: String,
    pub notes: Option<String>,
    pub lines: Vec<PurchaseReturnLine>,
}

/// A `purchase_return_line` as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurchaseReturnLine {
    pub id: String,
    pub purchase_return_id: String,
    pub inventory_item_id: String,
    pub grn_line_id: Option<String>,
    pub line_number: i64,
    pub entered_purchase_unit: String,
    pub entered_quantity_micro: i64,
    pub quantity_dimension: String,
    pub base_quantity_micro: i64,
    pub unit_cost_paise: i64,
}

/// One line of an outbound stock transfer. `stock_transfer_line` carries only
/// a BASE quantity -- there is no purchase-unit pair on that table -- so this
/// struct takes the base figure directly rather than inventing an
/// entered/converted split the schema cannot store.
#[derive(Debug, Clone)]
pub struct NewStockTransferLine {
    pub inventory_item_id: String,
    pub base_quantity_micro: i64,
    /// The unit the author chose. Never derived from the referent.
    pub quantity_dimension: String,
    /// `None` values the dispatch at the weighted average cost derived from
    /// the ledger -- see [`NewPurchaseReturnLine::unit_cost_paise`].
    pub unit_cost_paise: Option<i64>,
}

/// An outbound stock transfer as submitted. OUTBOUND HALF ONLY: this posts
/// `TRANSFER_OUT` at the source outlet. `TRANSFER_IN` and goods-in-transit
/// are M8 -- a transfer spans two edge databases (ADR-019).
#[derive(Debug, Clone)]
pub struct NewStockTransferOut {
    pub id: String,
    pub outlet_id: String,
    /// May name an outlet this edge database has no row for -- deliberately
    /// no FK on the column, so a transfer to a sibling outlet this till has
    /// never synced still dispatches.
    pub destination_outlet_id: String,
    pub transfer_number: String,
    pub dispatched_at: String,
    pub dispatched_by_user_id: String,
    pub notes: Option<String>,
    pub lines: Vec<NewStockTransferLine>,
}

/// A `stock_transfer_out` as stored, with its lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StockTransferOut {
    pub id: String,
    pub outlet_id: String,
    pub destination_outlet_id: String,
    pub transfer_number: String,
    pub dispatched_at: String,
    pub dispatched_by_user_id: String,
    pub business_date: String,
    pub notes: Option<String>,
    pub lines: Vec<StockTransferLine>,
}

/// A `stock_transfer_line` as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StockTransferLine {
    pub id: String,
    pub stock_transfer_out_id: String,
    pub inventory_item_id: String,
    pub line_number: i64,
    pub base_quantity_micro: i64,
    pub quantity_dimension: String,
    pub unit_cost_paise: i64,
}

/// This outlet's own view of how much of a purchase order has arrived --
/// DERIVED on demand from local `grn_line` rows, never stored.
///
/// **THE EDGE'S AND THE CLOUD'S FIGURES LEGITIMATELY DIFFER, AND BOTH ARE
/// RIGHT** (ADR-019). The edge sees only this outlet's receipts; the cloud
/// sees every outlet's. A PO shared across outlets reads "40 of 100" at one
/// till and "90 of 100" in the admin at the same moment. Show both, label
/// which is which, and NEVER reconcile them -- reconciling reintroduces the
/// second writer that keeping receipt state off `purchase_order` exists to
/// avoid (sec.50.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurchaseOrderReceiptProgress {
    pub purchase_order_id: String,
    pub purchase_order_line_id: String,
    pub inventory_item_id: String,
    pub ordered_base_quantity_micro: i64,
    /// Received AT THIS OUTLET only. See the struct doc comment.
    pub received_base_quantity_micro_at_this_outlet: i64,
}

// ------------------------------------- Milestone 5: procurement READS (T3) --
//
// The picker surface. `supplier`, `supplier_item`, `purchase_order` and
// `purchase_order_line` are CLOUD-OWNED CONFIG at the edge (ADR-019, sec.50.1)
// and are read-only here: nothing in this crate writes them outside test
// fixtures and the config apply path.
//
// WHY THESE EXIST AT ALL. Without a read the receiving screen can only take a
// typed UUID for a supplier and a purchase order, which is a screen that
// exists and nobody can use -- the M4 missing-variant-picker defect exactly
// (`variantId: null` hardcoded at the till, while the harness that "proved"
// the criterion selected a variant directly). A receiving clerk cannot type a
// UUID.

/// A `supplier` row, enough to render a picker and show credit terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Supplier {
    pub id: String,
    pub outlet_id: String,
    pub code: String,
    pub name: String,
    pub gstin: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub address: Option<String>,
    pub payment_terms_days: i64,
    pub is_active: bool,
}

/// A `supplier_item` row: what this supplier sells, IN THE UNIT THEY SELL IT
/// IN, and the pack size that converts it.
///
/// `quantity_dimension` is **the unit the author chose, never derived from
/// `inventory_item.dimension`** (contracts 0.5.2, ADR-019 sec.6). A caller
/// rendering a receiving line must carry this value through as it is read: if
/// the UI re-derives it from the item, the comparison the write path makes
/// becomes `x == x`, the `DIMENSION_MISMATCH` guard can never fire, and it
/// will look correct in review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupplierItem {
    pub id: String,
    pub supplier_id: String,
    pub inventory_item_id: String,
    pub inventory_item_name: String,
    pub purchase_unit: String,
    pub pack_size_micro: i64,
    pub quantity_dimension: String,
    /// Advisory prefill only. **Never the price a GRN posts** -- what was
    /// invoiced is a fact, what was expected is a guess.
    pub last_price_paise: Option<i64>,
    pub is_preferred: bool,
}

/// A `purchase_order` header plus its lines, enough to pick one and prefill a
/// receipt.
///
/// **Carries no receipt state, deliberately** (ADR-019 sec.4). Progress is
/// derived on demand by [`crate::Db::purchase_order_receipt_progress`], and
/// the cloud's figure for the same PO legitimately differs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurchaseOrderSummary {
    pub id: String,
    pub outlet_id: String,
    pub supplier_id: String,
    pub supplier_name: String,
    pub po_number: String,
    pub status: String,
    pub expected_date: Option<String>,
    pub notes: Option<String>,
    pub total_paise: i64,
    pub created_at: String,
    pub lines: Vec<PurchaseOrderLineRow>,
}

/// A `purchase_order_line` as stored, plus the item's name so a picker does
/// not need a second read to be legible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurchaseOrderLineRow {
    pub id: String,
    pub purchase_order_id: String,
    pub inventory_item_id: String,
    pub inventory_item_name: String,
    pub line_number: i64,
    pub purchase_unit: String,
    pub ordered_quantity_micro: i64,
    /// See [`SupplierItem::quantity_dimension`] -- same rule, same trap.
    pub quantity_dimension: String,
    pub unit_price_paise: i64,
    pub line_total_paise: i64,
}

// -------------------------------- Milestone 5: procurement CONFIG rows (T3b) --
//
// The four CLOUD_TO_EDGE procurement config families as they are STORED, one
// struct per table, carrying every column the frozen SQLite schema
// (`packages/contracts/sqlite/0027_m5_procurement.sql`) declares -- including
// `config_version`, which the picker-facing read structs above deliberately
// omit because a screen has no use for it.
//
// These exist separately from `Supplier`/`SupplierItem`/`PurchaseOrderSummary`
// /`PurchaseOrderLineRow` because those are JOIN-ENRICHED READ SHAPES (they
// carry `inventory_item_name`, `supplier_name`, and nested lines) and a write
// path must never invent a value for a column it does not own. A struct that
// is both the read projection and the write row would need a name for a field
// that is not a column, and the apply path would have to fabricate one.
//
// WHY THESE LANDED LATE, AND WHAT IT COST. `GET /sync/config` has carried
// `suppliers`, `supplier_items`, `purchase_orders` and `purchase_order_lines`
// since contracts 0.6.0, and `edge/sync`'s `ConfigBundle` had no such fields --
// so serde discarded all four IN SILENCE and every outlet's procurement tables
// were empty. The pickers read nothing and a receipt had no `supplier_item`,
// hence no pack size, hence a `NO_SUPPLIER_ITEM` gap on every delivery. This is
// contracts 0.5.9 inverted: then the edge wrote a field the cloud had never
// heard of and `json.Unmarshal` dropped it. Same lenient decoder, same silence,
// opposite direction. `scripts/check-config-apply-drift.mjs` is the guard.

/// A `supplier` row exactly as the cloud sends it. CLOUD-OWNED (ADR-019,
/// sec.50.1): nothing at the edge writes this outside the config apply path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupplierConfig {
    pub id: String,
    pub outlet_id: String,
    pub code: String,
    pub name: String,
    pub gstin: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub address: Option<String>,
    pub payment_terms_days: i64,
    pub is_active: bool,
    pub config_version: i64,
}

/// A `supplier_item` row exactly as the cloud sends it. A CHILD ROW of
/// `supplier`, so it has NO `config_version` of its own -- the frozen schema
/// gives it none, and its parent's version gates the family.
///
/// `quantity_dimension` is **stored exactly as received and never re-derived
/// from the referenced `inventory_item.dimension`** (contracts 0.5.2, ADR-019
/// sec.6). If the apply path recomputed it, the cloud's write-time rejection
/// would be comparing `x == x`, could never fire, and would look correct in
/// review -- while the edge's `DIMENSION_MISMATCH` gap would likewise never be
/// raised on a genuinely mis-authored row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupplierItemConfig {
    pub id: String,
    pub supplier_id: String,
    pub inventory_item_id: String,
    pub purchase_unit: String,
    pub pack_size_micro: i64,
    pub quantity_dimension: String,
    pub last_price_paise: Option<i64>,
    pub is_preferred: bool,
}

/// A `purchase_order` header exactly as the cloud sends it.
///
/// **No receipt state, and there never will be** (ADR-019 sec.4). Receipt
/// progress is derived from local `grn_line` rows and the cloud's figure for
/// the same PO legitimately differs; a column here would make the outlet a
/// second writer of a cloud aggregate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurchaseOrderConfig {
    pub id: String,
    pub outlet_id: String,
    pub supplier_id: String,
    pub po_number: String,
    pub status: String,
    pub expected_date: Option<String>,
    pub notes: Option<String>,
    pub total_paise: i64,
    pub approved_by_user_id: Option<String>,
    pub approved_at: Option<String>,
    pub created_at: String,
    pub config_version: i64,
}

/// A `purchase_order_line` row exactly as the cloud sends it. A CHILD ROW, so
/// no `config_version` of its own. `quantity_dimension` carries the same rule
/// and the same trap as [`SupplierItemConfig::quantity_dimension`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurchaseOrderLineConfig {
    pub id: String,
    pub purchase_order_id: String,
    pub inventory_item_id: String,
    pub line_number: i64,
    pub purchase_unit: String,
    pub ordered_quantity_micro: i64,
    pub quantity_dimension: String,
    pub unit_price_paise: i64,
    pub line_total_paise: i64,
}
