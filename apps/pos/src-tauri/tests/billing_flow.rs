//! Integration tests for T9's billing commands (ADR-016, docs/spec/payments.md).
//! Exercises the full Tauri command-layer wiring (not just the underlying
//! `holler_edge_database` crate, which already has its own unit tests) —
//! issue a GST invoice, take a split tender across two methods, and the §39
//! mandatory-variance-reason gate on closing a cash shift.

use holler_edge_database::{model, repo, Db};

use holler_pos_lib::commands::billing::{
    close_cash_shift_impl, find_open_cash_shift_impl, issue_invoice_impl, open_cash_shift_impl,
    record_payment_impl,
};
use holler_pos_lib::commands::orders::{create_order_impl, NewOrderItemRequest};
use holler_pos_lib::state::AppState;

const OUTLET_ID: &str = "outlet-1";
const DEVICE_ID: &str = "device-1";
const USER_ID: &str = "user-1";

fn seed_billing_config(db: &Db) {
    let conn = db.connection();
    repo::upsert_outlet(
        conn,
        &model::Outlet {
            id: OUTLET_ID.to_string(),
            brand_id: "brand-1".to_string(),
            name: "Test Outlet".to_string(),
            timezone: "Asia/Kolkata".to_string(),
            config_version: 1,
            created_at: "2026-08-01T00:00:00Z".to_string(),
            updated_at: "2026-08-01T00:00:00Z".to_string(),
        },
    )
    .expect("seed outlet");

    repo::upsert_device(
        conn,
        &model::Device {
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
        conn,
        &model::AppUser {
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
        conn,
        &model::MenuCategory {
            id: "cat-1".to_string(),
            outlet_id: OUTLET_ID.to_string(),
            name: "Mains".to_string(),
            sort_order: 1,
            config_version: 1,
        },
    )
    .expect("seed category");

    repo::upsert_menu_item(
        conn,
        &model::MenuItem {
            id: "item-1".to_string(),
            outlet_id: OUTLET_ID.to_string(),
            category_id: "cat-1".to_string(),
            name: "Thali".to_string(),
            base_price_paise: 20_000,
            is_available: true,
            config_version: 1,
            tax_profile_id: None,
        },
    )
    .expect("seed menu item");

    let compliance_version_id = "cv-1".to_string();
    repo::upsert_compliance_version(
        conn,
        &model::ComplianceVersion {
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
        conn,
        &model::TaxProfile {
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
            conn,
            &model::TaxRule {
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
        conn,
        &model::OutletFiscalProfile {
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
        conn,
        &model::InvoiceSeries {
            id: "series-sales".to_string(),
            outlet_id: OUTLET_ID.to_string(),
            code: "SALES".to_string(),
            prefix_template: "INV-".to_string(),
            reset_policy: "NEVER".to_string(),
            padding_width: 6,
            is_active: true,
            config_version: 1,
        },
    )
    .expect("seed invoice series");
}

fn app_state() -> AppState {
    let db = Db::open_in_memory_for_tests().expect("open db");
    seed_billing_config(&db);
    AppState::new(db, OUTLET_ID.to_string(), DEVICE_ID.to_string())
}

#[test]
fn issuing_a_bill_computes_gst_and_the_grand_total_matches_the_line_total() {
    let state = app_state();
    let order = create_order_impl(
        &state,
        "DINE_IN".to_string(),
        None,
        vec![NewOrderItemRequest {
            menu_item_id: "item-1".to_string(),
            variant_id: None,
            quantity: 2,
            unit_price_paise: 20_000,
            notes: None,
            modifiers: vec![],
        }],
    )
    .expect("create order");

    let invoice = issue_invoice_impl(&state, &order.holler_order_id, USER_ID).expect("issue invoice");

    // 2 x Rs.200 = Rs.400 taxable, +2.5% CGST +2.5% SGST = Rs.20 tax -> Rs.420.
    assert_eq!(invoice.taxable_value_paise, 40_000);
    assert_eq!(invoice.cgst_paise, 1_000);
    assert_eq!(invoice.sgst_paise, 1_000);
    assert_eq!(invoice.grand_total_paise, 42_000);
    assert_eq!(invoice.status, "ISSUED");
    assert_eq!(invoice.lines.len(), 1);
}

#[test]
fn a_split_tender_across_two_methods_is_recorded_as_two_append_only_payments() {
    let state = app_state();
    let order = create_order_impl(
        &state,
        "DINE_IN".to_string(),
        None,
        vec![NewOrderItemRequest {
            menu_item_id: "item-1".to_string(),
            variant_id: None,
            quantity: 1,
            unit_price_paise: 20_000,
            notes: None,
            modifiers: vec![],
        }],
    )
    .expect("create order");
    let invoice = issue_invoice_impl(&state, &order.holler_order_id, USER_ID).expect("issue invoice");
    assert_eq!(invoice.grand_total_paise, 21_000); // Rs.200 + 5% GST

    // Split: Rs.100 cash + Rs.110 UPI = Rs.210, matching the §35 shape.
    // Both name `invoice.id` — T9 retry: this is what lets the edge validate
    // each tender against the invoice's actual remaining due.
    let cash = record_payment_impl(
        &state,
        &order.holler_order_id,
        "CASH",
        10_000,
        Some(10_000),
        Some(0),
        None,
        None,
        None,
        Some(invoice.id.clone()),
        USER_ID,
    )
    .expect("cash tender");
    let upi = record_payment_impl(
        &state,
        &order.holler_order_id,
        "UPI",
        11_000,
        None,
        None,
        None,
        None,
        None,
        Some(invoice.id.clone()),
        USER_ID,
    )
    .expect("upi tender");

    assert_eq!(cash.amount_paise + upi.amount_paise, invoice.grand_total_paise);
    assert_eq!(cash.status, "CAPTURED");
    assert_eq!(upi.status, "CAPTURED");
    // UPI is never allowed to carry cash-drawer fields (PaymentSchema's own
    // refine in packages/contracts/src/types/payment.ts).
    assert_eq!(upi.tendered_paise, None);
    assert_eq!(upi.change_paise, None);

    // T9 retry, Defect 1: one paisa more than what remains (already zero
    // after the split above) must be rejected at the edge, not merely by a
    // disabled button.
    let err = record_payment_impl(
        &state,
        &order.holler_order_id,
        "CASH",
        1,
        Some(1),
        Some(0),
        None,
        None,
        None,
        Some(invoice.id.clone()),
        USER_ID,
    )
    .expect_err("a tender against an already fully-settled invoice must be rejected");
    assert_eq!(err.code, "FORWARD_PAYMENT_EXCEEDS_REMAINING_DUE");
    // §64: the message must name the actual amount outstanding, not a
    // generic failure.
    assert!(err.message.contains('0'));
}

#[test]
fn a_non_zero_variance_close_is_rejected_without_a_reason_and_the_shift_stays_open() {
    let state = app_state();
    let shift = open_cash_shift_impl(&state, USER_ID, 20_000).expect("open shift");
    assert_eq!(shift.status, "OPEN");

    let err = close_cash_shift_impl(&state, &shift.id, 25_000, None)
        .expect_err("a non-zero variance close without a reason must be rejected");
    assert_eq!(err.code, "CASH_VARIANCE_REASON_REQUIRED");
    // §64: the message must name the actual variance, not a generic failure.
    assert!(err.message.contains("paise"));

    let with_reason = close_cash_shift_impl(
        &state,
        &shift.id,
        25_000,
        Some("counted extra float left in drawer".to_string()),
    )
    .expect("close with a reason succeeds");
    assert_eq!(with_reason.status, "CLOSED");
    assert_eq!(with_reason.variance_paise, Some(5_000));
}

/// T9 retry, Defect 2: a POS restart loses the in-memory shift id
/// (`apps/pos/src/store/cashShift.ts`), but `find_open_cash_shift_impl` —
/// the same query the POS calls on startup — recovers it through the
/// command layer without the caller ever supplying a shift id, and the
/// recovered shift can then be closed normally.
#[test]
fn find_open_cash_shift_recovers_after_a_simulated_restart_and_can_then_be_closed() {
    let state = app_state();
    assert!(
        find_open_cash_shift_impl(&state, USER_ID)
            .expect("query")
            .is_none(),
        "nothing open yet"
    );

    let opened = open_cash_shift_impl(&state, USER_ID, 20_000).expect("open shift");

    // Simulate the restart: nothing about the shift id survives except what
    // is durable in SQLite — the command layer is asked to find it purely
    // from device_id (state) + cashier_user_id, no id supplied.
    let recovered = find_open_cash_shift_impl(&state, USER_ID)
        .expect("query")
        .expect("the open shift must be recovered without knowing its id");
    assert_eq!(recovered.id, opened.id);
    assert_eq!(recovered.status, "OPEN");

    let closed = close_cash_shift_impl(&state, &recovered.id, 20_000, None)
        .expect("the recovered shift can be closed normally");
    assert_eq!(closed.status, "CLOSED");

    assert!(
        find_open_cash_shift_impl(&state, USER_ID)
            .expect("query")
            .is_none(),
        "a closed shift is no longer found as open"
    );
}
