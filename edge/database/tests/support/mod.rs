//! Shared seeding helpers for the T7b integration tests
//! (`tests/invoice_numbering_stress.rs`, `tests/invoice_split_conservation.rs`).
//! Lives under `tests/support/` (not `tests/support.rs`) so cargo does not
//! treat it as its own test binary — the `tests/common/mod.rs` convention.

use holler_edge_database::model::*;
use holler_edge_database::repo;
use holler_edge_database::Db;

pub const OUTLET_ID: &str = "outlet-1";
pub const DEVICE_ID: &str = "device-1";
pub const USER_ID: &str = "user-1";
pub const CATEGORY_ID: &str = "category-1";
pub const MENU_ITEM_ID: &str = "menu-item-1";

/// Seeds outlet/device/app_user/menu/billing-config fixtures sufficient to
/// issue invoices against `OUTLET_ID` — one default active GST-5 tax
/// profile (CGST 2.5% + SGST 2.5%, EXCLUSIVE pricing), one outlet fiscal
/// profile, and one invoice series `series_code`/`reset_policy`.
pub fn seed(db: &Db, series_code: &str, reset_policy: &str) {
    repo::upsert_outlet(
        db.connection(),
        &Outlet {
            id: OUTLET_ID.to_string(),
            brand_id: "brand-1".to_string(),
            name: "Pune".to_string(),
            timezone: "Asia/Kolkata".to_string(),
            config_version: 1,
            created_at: "2026-08-01T00:00:00Z".to_string(),
            updated_at: "2026-08-01T00:00:00Z".to_string(),
        },
    )
    .expect("seed outlet");

    repo::upsert_device(
        db.connection(),
        &Device {
            id: DEVICE_ID.to_string(),
            outlet_id: OUTLET_ID.to_string(),
            kind: "POS".to_string(),
            name: "Till 1".to_string(),
            last_seen_at: None,
            created_at: "2026-08-01T00:00:00Z".to_string(),
        },
    )
    .expect("seed device");

    repo::replace_app_user(
        db.connection(),
        &AppUser {
            id: USER_ID.to_string(),
            tenant_id: "tenant-1".to_string(),
            outlet_id: OUTLET_ID.to_string(),
            email: "cashier@example.in".to_string(),
            full_name: "Cashier".to_string(),
            password_hash: "argon2id$dummy".to_string(),
            pin_hash: None,
            is_active: true,
            permissions_json: "[]".to_string(),
            config_version: 1,
            updated_at: "2026-08-01T00:00:00Z".to_string(),
        },
    )
    .expect("seed app_user");

    repo::upsert_menu_category(
        db.connection(),
        &MenuCategory {
            id: CATEGORY_ID.to_string(),
            outlet_id: OUTLET_ID.to_string(),
            name: "Mains".to_string(),
            sort_order: 1,
            config_version: 1,
        },
    )
    .expect("seed category");

    repo::upsert_menu_item(
        db.connection(),
        &MenuItem {
            id: MENU_ITEM_ID.to_string(),
            outlet_id: OUTLET_ID.to_string(),
            category_id: CATEGORY_ID.to_string(),
            name: "Thali".to_string(),
            base_price_paise: 20_000,
            is_available: true,
            config_version: 1,
            tax_profile_id: None, // falls back to the outlet default seeded below
            // SAC 9963 (restaurant service) — ADR-016 0.4.5 §3: an invoice
            // cannot issue with a line whose HSN/SAC is NULL, so this
            // fixture must carry one for `seed()`'s callers to reach a
            // successful issuance at all.
            hsn_sac: Some("9963".to_string()),
        },
    )
    .expect("seed menu item");

    let compliance_version_id = "cv-1".to_string();
    repo::upsert_compliance_version(
        db.connection(),
        &ComplianceVersion {
            id: compliance_version_id.clone(),
            outlet_id: OUTLET_ID.to_string(),
            label: "GST 2026-04".to_string(),
            effective_from: "2020-01-01T00:00:00Z".to_string(),
            notes: None,
            config_version: 1,
        },
    )
    .expect("seed compliance version");

    let tax_profile_id = "profile-gst5".to_string();
    repo::upsert_tax_profile(
        db.connection(),
        &TaxProfile {
            id: tax_profile_id.clone(),
            outlet_id: OUTLET_ID.to_string(),
            code: "GST_5".to_string(),
            name: "GST 5%".to_string(),
            pricing_mode: "EXCLUSIVE".to_string(),
            is_default: true,
            is_active: true,
            config_version: 1,
        },
    )
    .expect("seed tax profile");

    for (component, rate_bps) in [("CGST", 250i64), ("SGST", 250i64)] {
        repo::upsert_tax_rule(
            db.connection(),
            &TaxRule {
                id: format!("{tax_profile_id}-{component}"),
                tax_profile_id: tax_profile_id.clone(),
                compliance_version_id: compliance_version_id.clone(),
                component: component.to_string(),
                rate_bps,
                effective_from: "2020-01-01T00:00:00Z".to_string(),
                effective_to: None,
                config_version: 1,
            },
        )
        .expect("seed tax rule");
    }

    repo::upsert_outlet_fiscal_profile(
        db.connection(),
        &OutletFiscalProfile {
            id: "fiscal-1".to_string(),
            outlet_id: OUTLET_ID.to_string(),
            legal_name: "Test Restaurant Pvt Ltd".to_string(),
            trade_name: "Test Outlet".to_string(),
            address_line1: "123 MG Road".to_string(),
            address_line2: None,
            city: "Pune".to_string(),
            state_code: "27".to_string(),
            state_name: "Maharashtra".to_string(),
            pincode: "411001".to_string(),
            gstin: "27AAAAA0000A1Z5".to_string(),
            fssai_number: None,
            invoice_footer_text: None,
            effective_from: "2020-01-01T00:00:00Z".to_string(),
            config_version: 1,
        },
    )
    .expect("seed fiscal profile");

    repo::upsert_invoice_series(
        db.connection(),
        &InvoiceSeries {
            id: format!("series-{series_code}"),
            outlet_id: OUTLET_ID.to_string(),
            code: series_code.to_string(),
            prefix_template: "INV-".to_string(),
            reset_policy: reset_policy.to_string(),
            padding_width: 6,
            is_active: true,
            config_version: 1,
        },
    )
    .expect("seed invoice series");
}

/// Creates an order with `quantities.len()` order items, each of the seeded
/// menu item, quantity from `quantities`, at `unit_price_paise` per unit.
/// Returns `(order_id, Vec<order_item_id>)`.
pub fn create_order(
    db: &mut Db,
    order_id: &str,
    unit_price_paise: i64,
    quantities: &[i64],
) -> Vec<String> {
    let order = NewOrder {
        id: order_id.to_string(),
        outlet_id: OUTLET_ID.to_string(),
        device_id: DEVICE_ID.to_string(),
        order_type: "DINE_IN".to_string(),
        status: "DRAFT".to_string(),
        table_id: None,
        subtotal_paise: 0,
        discount_paise: 0,
        taxes_paise: 0,
        total_paise: 0,
        source: "POS".to_string(),
        external_order_id: None,
        payment_status: "UNPAID".to_string(),
        payment_source: None,
        confirmed_at: None,
        source_payload_json: None,
        schema_version: 1,
        created_at: "2026-08-12T10:00:00Z".to_string(),
        updated_at: "2026-08-12T10:00:00Z".to_string(),
    };

    let mut item_ids = Vec::with_capacity(quantities.len());
    let mut items = Vec::with_capacity(quantities.len());
    for (i, &qty) in quantities.iter().enumerate() {
        let item_id = format!("{order_id}-item-{i}");
        items.push(NewOrderItem {
            id: item_id.clone(),
            order_id: order_id.to_string(),
            menu_item_id: MENU_ITEM_ID.to_string(),
            variant_id: None,
            quantity: qty,
            unit_price_paise,
            line_total_paise: unit_price_paise * qty,
            notes: None,
            created_at: "2026-08-12T10:00:00Z".to_string(),
        });
        item_ids.push(item_id);
    }

    let outbox = NewOutboxEntry {
        id: format!("outbox-{order_id}"),
        aggregate_type: "order".to_string(),
        aggregate_id: order_id.to_string(),
        event_type: "OrderCreated".to_string(),
        payload_json: "{}".to_string(),
        created_at: "2026-08-12T10:00:00Z".to_string(),
    };

    db.create_order_with_outbox(&order, &items, &outbox)
        .expect("create order");

    item_ids
}

// Not every test binary that includes this shared module calls every helper
// (each `tests/*.rs` file is compiled as its own crate) — `#[allow]` here
// rather than in each binary, matching the pattern already needed for a
// module shared across independent integration test crates.
#[allow(dead_code)]
pub fn header(
    order_id: &str,
    series_code: &str,
    business_date: &str,
    invoice_date: &str,
) -> IssueInvoiceHeader {
    IssueInvoiceHeader {
        outlet_id: OUTLET_ID.to_string(),
        order_id: order_id.to_string(),
        series_code: series_code.to_string(),
        invoice_date: invoice_date.to_string(),
        business_date: business_date.to_string(),
        customer_name: None,
        customer_phone: None,
        customer_gstin: None,
        place_of_supply_state_code: "27".to_string(),
        channel: "POS".to_string(),
        tax_liability_party: "RESTAURANT".to_string(),
        eco_operator_name: None,
        eco_operator_gstin: None,
        supply_classification: None,
        created_by_user_id: USER_ID.to_string(),
    }
}
