//! Integration tests for T9's billing commands (ADR-016, docs/spec/payments.md).
//! Exercises the full Tauri command-layer wiring (not just the underlying
//! `holler_edge_database` crate, which already has its own unit tests) —
//! issue a GST invoice, take a split tender across two methods, and the §39
//! mandatory-variance-reason gate on closing a cash shift.

use holler_edge_database::{model, repo, Db};

use holler_pos_lib::commands::billing::{
    close_cash_shift_impl, find_open_cash_shift_impl, issue_invoice_impl,
    issue_split_invoices_impl, list_invoices_for_order_impl, list_invoices_for_split_group_impl,
    open_cash_shift_impl, record_payment_impl, LineDiscountInput, SplitLineInput, SplitPartInput,
};
use holler_pos_lib::commands::kitchen::{list_failed_print_jobs_impl, retry_failed_print_jobs_impl};
use holler_pos_lib::commands::orders::{create_order_impl, NewOrderItemRequest};
use holler_pos_lib::dto::FailedPrintJobTarget;
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
            hsn_sac: Some("9963".to_string()),
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

/// Seeds one `discount_definition` row directly (bypassing any caller-side
/// shape checks, the same way `seed_billing_config` seeds every other config
/// row) so a test can control every governance field independently.
#[allow(clippy::too_many_arguments)]
fn seed_discount_definition(
    db: &Db,
    id: &str,
    code: &str,
    scope: &str,
    method: &str,
    value_bps: Option<i64>,
    value_paise: Option<i64>,
    max_discount_paise: Option<i64>,
    required_permission: Option<&str>,
    requires_reason: bool,
) {
    repo::upsert_discount_definition(
        db.connection(),
        &model::DiscountDefinition {
            id: id.to_string(),
            outlet_id: OUTLET_ID.to_string(),
            code: code.to_string(),
            name: code.to_string(),
            scope: scope.to_string(),
            method: method.to_string(),
            value_bps,
            value_paise,
            max_discount_paise,
            required_permission: required_permission.map(str::to_string),
            requires_reason,
            is_active: true,
            effective_from: "2020-01-01T00:00:00Z".to_string(),
            effective_to: None,
            config_version: 1,
        },
    )
    .expect("seed discount definition");
}

/// The worked example this track's report cites verbatim: 2 x Rs.200.00
/// (unit_price_paise 20000), a 10% LINE discount, GST 5% (2.5% CGST + 2.5%
/// SGST) EXCLUSIVE pricing.
///
/// Per unit: 10% of Rs.200.00 = Rs.20.00 (2000 paise) discount.
/// Gross = 2 x 20000 = 40000. Discount = 2 x 2000 = 4000. Net (taxable) =
/// 36000. CGST = SGST = 36000 * 2.5% = 900 each -> tax = 1800.
/// Grand total = 36000 + 1800 = 37800 (already a whole rupee, round_off 0).
#[test]
fn a_line_discount_reduces_the_taxable_value_and_gst_is_computed_on_the_net() {
    let state = app_state();
    {
        let db = state.db.lock().expect("lock");
        seed_discount_definition(
            &db,
            "disc-staff10",
            "STAFF10",
            "LINE",
            "PERCENT",
            Some(1000), // 10.00%
            None,
            None,
            None,
            false,
        );
    }

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

    let invoice = issue_invoice_impl(
        &state,
        &order.holler_order_id,
        USER_ID,
        &[LineDiscountInput {
            order_item_id: order.items[0].id.clone(),
            discount_definition_id: "disc-staff10".to_string(),
            reason: None,
        }],
    )
    .expect("issue invoice with a line discount");

    let line = &invoice.lines[0];
    assert_eq!(line.gross_paise, 40_000, "gross is the undiscounted amount");
    assert_eq!(line.discount_paise, 4_000, "discount reduces the taxable base, not the gross");
    assert_eq!(line.taxable_value_paise, 36_000, "GST is computed on the post-discount net");
    assert_eq!(line.cgst_paise, 900);
    assert_eq!(line.sgst_paise, 900);
    assert_eq!(line.total_paise, 36_000 + 900 + 900);

    assert_eq!(invoice.discount_paise, 4_000);
    assert_eq!(invoice.taxable_value_paise, 36_000);
    assert_eq!(invoice.cgst_paise, 900);
    assert_eq!(invoice.sgst_paise, 900);
    // Conservation: components sum to the tax total, and
    // grand_total = Σ(components) + round_off, |round_off| <= 50 (ADR-016 §3).
    let pre_round =
        invoice.taxable_value_paise + invoice.cgst_paise + invoice.sgst_paise + invoice.igst_paise
            + invoice.cess_paise;
    assert_eq!(invoice.grand_total_paise, pre_round + invoice.round_off_paise);
    assert!(invoice.round_off_paise.abs() <= 50);
    assert_eq!(invoice.grand_total_paise, 37_800);
    assert_eq!(invoice.round_off_paise, 0);
}

/// Falsification companion to the worked example above: with NO discount
/// supplied, the same order must produce the pre-M3-track figures
/// (`issuing_a_bill_computes_gst_and_the_grand_total_matches_the_line_total`)
/// — proving the discount path only fires when actually asked for.
#[test]
fn no_discount_supplied_bills_at_full_price_exactly_as_before() {
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

    let invoice =
        issue_invoice_impl(&state, &order.holler_order_id, USER_ID, &[]).expect("issue invoice");
    assert_eq!(invoice.lines[0].discount_paise, 0);
    assert_eq!(invoice.taxable_value_paise, 40_000);
    assert_eq!(invoice.grand_total_paise, 42_000);
}

/// §28/ADR-016 binding: `requires_reason` must actually block application,
/// not merely be advisory — no reason, no discount, and the invoice is never
/// issued at all (all-or-nothing, matching every other billing guard here).
#[test]
fn a_discount_requiring_a_reason_is_rejected_without_one() {
    let state = app_state();
    {
        let db = state.db.lock().expect("lock");
        seed_discount_definition(
            &db, "disc-mgr", "MGR_COMP", "LINE", "AMOUNT", None, Some(5_000), None, None, true,
        );
    }
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

    let err = issue_invoice_impl(
        &state,
        &order.holler_order_id,
        USER_ID,
        &[LineDiscountInput {
            order_item_id: order.items[0].id.clone(),
            discount_definition_id: "disc-mgr".to_string(),
            reason: None,
        }],
    )
    .expect_err("a discount requiring a reason must be rejected without one");
    assert_eq!(err.code, "DISCOUNT_REASON_REQUIRED");

    // Supplying the reason is what unblocks it — the SAME call, only the
    // reason changed.
    let ok = issue_invoice_impl(
        &state,
        &order.holler_order_id,
        USER_ID,
        &[LineDiscountInput {
            order_item_id: order.items[0].id.clone(),
            discount_definition_id: "disc-mgr".to_string(),
            reason: Some("manager comp — customer complaint".to_string()),
        }],
    )
    .expect("a real reason satisfies the gate");
    assert_eq!(ok.lines[0].discount_paise, 5_000);
}

/// §28/ADR-016 binding: `required_permission` must actually block a cashier
/// lacking it — the seeded `USER_ID` carries `permissions_json: "[]"`
/// (`seed_billing_config`), so it never satisfies a definition naming a
/// permission.
#[test]
fn a_discount_requiring_a_permission_is_rejected_for_a_user_lacking_it() {
    let state = app_state();
    {
        let db = state.db.lock().expect("lock");
        seed_discount_definition(
            &db,
            "disc-override",
            "OVERRIDE20",
            "LINE",
            "PERCENT",
            Some(2000),
            None,
            None,
            Some("bill.discount.override"),
            false,
        );
    }
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

    let err = issue_invoice_impl(
        &state,
        &order.holler_order_id,
        USER_ID,
        &[LineDiscountInput {
            order_item_id: order.items[0].id.clone(),
            discount_definition_id: "disc-override".to_string(),
            reason: None,
        }],
    )
    .expect_err("a user lacking the required permission must be rejected");
    assert_eq!(err.code, "DISCOUNT_PERMISSION_DENIED");
}

/// BILL scope is reported unimplemented, not silently narrowed to a line
/// discount it was never defined as.
#[test]
fn a_bill_scope_discount_is_rejected_as_unimplemented() {
    let state = app_state();
    {
        let db = state.db.lock().expect("lock");
        seed_discount_definition(
            &db, "disc-bill", "BILL_FLAT", "BILL", "AMOUNT", None, Some(1_000), None, None, false,
        );
    }
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

    let err = issue_invoice_impl(
        &state,
        &order.holler_order_id,
        USER_ID,
        &[LineDiscountInput {
            order_item_id: order.items[0].id.clone(),
            discount_definition_id: "disc-bill".to_string(),
            reason: None,
        }],
    )
    .expect_err("BILL scope must not be silently applied");
    assert_eq!(err.code, "DISCOUNT_SCOPE_NOT_SUPPORTED");
}

/// The negative-discount case turned out to be guarded one layer BEFORE the
/// tax engine: `sqlite/0006_m3_billing.sql`'s own
/// `CHECK(value_paise IS NULL OR value_paise >= 0)` rejects the write
/// itself, so a negative `discount_definition.value_paise` can never reach
/// `issue_invoice` at all through any path, this one included. This test
/// records that finding rather than asserting a wrong one: seeding a
/// negative `value_paise` fails at `upsert_discount_definition`, before any
/// order or invoice is ever involved.
#[test]
fn a_negative_discount_value_is_rejected_by_sqlite_before_it_can_ever_reach_billing() {
    let state = app_state();
    let db = state.db.lock().expect("lock");
    let err = repo::upsert_discount_definition(
        db.connection(),
        &model::DiscountDefinition {
            id: "disc-neg".to_string(),
            outlet_id: OUTLET_ID.to_string(),
            code: "NEGATIVE".to_string(),
            name: "NEGATIVE".to_string(),
            scope: "LINE".to_string(),
            method: "AMOUNT".to_string(),
            value_bps: None,
            value_paise: Some(-500),
            max_discount_paise: None,
            required_permission: None,
            requires_reason: false,
            is_active: true,
            effective_from: "2020-01-01T00:00:00Z".to_string(),
            effective_to: None,
            config_version: 1,
        },
    )
    .expect_err("a negative value_paise must be rejected by the storage layer's own CHECK");
    assert!(format!("{err}").contains("value_paise"));
}

/// Proves the edge tax engine's own OVER-LIMIT guard fires THROUGH this
/// command path, not merely at the engine's own unit-test level: an AMOUNT
/// discount whose configured `value_paise` exceeds the line's
/// `unit_price_paise` — a shape SQLite's CHECK does not and cannot catch
/// (it has no line to compare against) — is still rejected, with a legible
/// §64 code, by `edge/database/src/tax/engine.rs::compute_line_base`.
#[test]
fn the_edges_own_discount_guard_fires_through_issue_invoice_for_an_excessive_discount() {
    let state = app_state();
    {
        let db = state.db.lock().expect("lock");
        seed_discount_definition(
            &db, "disc-huge", "TOO_BIG", "LINE", "AMOUNT", None, Some(1_000_000), None, None,
            false,
        );
    }
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

    let excessive_err = issue_invoice_impl(
        &state,
        &order.holler_order_id,
        USER_ID,
        &[LineDiscountInput {
            order_item_id: order.items[0].id.clone(),
            discount_definition_id: "disc-huge".to_string(),
            reason: None,
        }],
    )
    .expect_err("a discount exceeding unit_price_paise must be rejected");
    assert_eq!(excessive_err.code, "INVALID_INPUT");
    assert!(excessive_err.message.contains("unit_price_paise"));
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

    let invoice = issue_invoice_impl(&state, &order.holler_order_id, USER_ID, &[]).expect("issue invoice");

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
    let invoice = issue_invoice_impl(&state, &order.holler_order_id, USER_ID, &[]).expect("issue invoice");
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

// ----------------------------------------- §64 failed-invoice visibility --
// docs/spec/hardware-printing.md: "Print failures must be visible to
// staff." A failed KOT print job has always surfaced through
// `list_failed_print_jobs_impl`; this closes the same gap for a failed
// *invoice* print job (the defect `edge/printer::list_failed_jobs`'s
// `LEFT JOIN` fix addressed, and this crate's DTO layer had flattened back
// out via `.unwrap_or_default()`). `commands::kitchen::
// invoice_print_ctx_unwired` guarantees any invoice print attempt through
// this crate's own sweep fails (apps/pos does not enqueue invoice print
// jobs yet) — so enqueuing one directly via
// `holler_edge_printer::adapter::queue_invoice_for_print` and sweeping is
// the only way to produce a failed invoice job here, and it is exactly the
// case a cashier needs to see.
#[test]
fn a_failed_invoice_print_job_reaches_the_failed_jobs_view_with_its_invoice_number() {
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
    let invoice = issue_invoice_impl(&state, &order.holler_order_id, USER_ID, &[]).expect("issue invoice");

    {
        let db = state.db.lock().expect("lock");
        holler_edge_database::repo::upsert_printer(
            db.connection(),
            &holler_edge_database::model::Printer {
                id: "printer-bill".to_string(),
                outlet_id: OUTLET_ID.to_string(),
                name: "Bill Printer".to_string(),
                connection_kind: "ESCPOS_NETWORK".to_string(),
                // Deliberately unreachable, same trick
                // `critical_offline_flow.rs` uses: connect fails instantly,
                // no real socket opens.
                address: "127.0.0.1:1".to_string(),
                paper_width_mm: 80,
                is_active: true,
                config_version: 1,
            },
        )
        .expect("seed bill printer");

        holler_edge_printer::adapter::queue_invoice_for_print(
            db.connection(),
            &invoice.id,
            "printer-bill",
            "2026-08-16T10:00:00Z",
            || "print-job-invoice-1".to_string(),
        )
        .expect("queue invoice for print");
    }

    // `invoice_print_ctx_unwired` makes this attempt fail loudly rather
    // than silently, per `commands::kitchen`'s own doc comment.
    let failed = retry_failed_print_jobs_impl(&state).expect("retry sweep");
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].target, FailedPrintJobTarget::Invoice);
    assert_eq!(failed[0].invoice_number.as_deref(), Some(invoice.invoice_number.as_str()));
    assert_eq!(failed[0].invoice_id.as_deref(), Some(invoice.id.as_str()));
    assert!(failed[0].kot_id.is_none());
    assert!(failed[0].kot_station.is_none());
    assert!(failed[0].last_error.is_some());

    // list_failed_print_jobs_impl (the read the POS actually polls) agrees.
    let listed = list_failed_print_jobs_impl(&state).expect("list failed print jobs");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].target, FailedPrintJobTarget::Invoice);
}

// -------------------------------------------------------------- split bills --
// The gap this track closes: `issue_split_invoices_impl` reaching
// `Db::issue_split_invoices_with_outbox` through the Tauri command surface
// (ADR-016 §4, §66).

/// Sum of `last_value` across every `invoice_sequence` row — the same probe
/// `edge/database/tests/invoice_hsn_sac.rs`'s gapless-numbering assertion
/// uses, reused here at the POS command layer to prove a rejected split
/// through THIS surface burns no number either.
fn invoice_sequence_total(state: &AppState) -> i64 {
    let db = state.db.lock().expect("lock");
    db.connection()
        .query_row(
            "SELECT COALESCE(SUM(last_value), 0) FROM invoice_sequence",
            [],
            |row| row.get(0),
        )
        .expect("read invoice_sequence")
}

/// A correct 2-way split: one order line of quantity 2, split into two
/// parts of quantity 1 each. Two independently numbered invoices are
/// issued together, `Σ(split invoice lines) = order lines` exactly (ADR-016
/// §4), and each part is independently payable — settling part 1 in full
/// leaves part 2's remaining due untouched.
#[test]
fn a_correct_two_way_split_issues_two_independently_numbered_independently_payable_invoices() {
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
    let order_item_id = order.items[0].id.clone();

    let parts = vec![
        SplitPartInput {
            lines: vec![SplitLineInput {
                order_item_id: order_item_id.clone(),
                quantity: 1,
            }],
        },
        SplitPartInput {
            lines: vec![SplitLineInput {
                order_item_id: order_item_id.clone(),
                quantity: 1,
            }],
        },
    ];

    let invoices = issue_split_invoices_impl(&state, &order.holler_order_id, USER_ID, &parts, &[])
        .expect("issue a correct 2-way split");
    assert_eq!(invoices.len(), 2);

    // Independently numbered.
    assert_ne!(invoices[0].id, invoices[1].id);
    assert_ne!(
        invoices[0].invoice_number, invoices[1].invoice_number,
        "each split part must carry its own invoice number"
    );
    assert_eq!(invoices[0].split_index, 1);
    assert_eq!(invoices[1].split_index, 2);
    assert_eq!(invoices[0].split_count, 2);
    assert_eq!(invoices[1].split_count, 2);
    assert_eq!(invoices[0].split_group_id, invoices[1].split_group_id);
    assert!(invoices[0].split_group_id.is_some());

    // Each part bills quantity 1 of the Rs.200 item: Rs.200 + 5% GST = Rs.210.
    assert_eq!(invoices[0].grand_total_paise, 21_000);
    assert_eq!(invoices[1].grand_total_paise, 21_000);

    // Σ(split invoice lines) = order lines exactly: quantities sum back to
    // the order's own quantity, and paise sum back to what an unsplit bill
    // of the same order would total.
    let total_quantity: i64 = invoices
        .iter()
        .flat_map(|inv| inv.lines.iter())
        .map(|l| l.quantity)
        .sum();
    assert_eq!(total_quantity, 2);
    let total_grand: i64 = invoices.iter().map(|inv| inv.grand_total_paise).sum();
    assert_eq!(total_grand, 42_000, "matches the unsplit-order grand total exactly");

    // Listable by split group, and both parts are visible as unpaid.
    let group_id = invoices[0].split_group_id.clone().unwrap();
    let listed = list_invoices_for_split_group_impl(&state, &group_id).expect("list split group");
    assert_eq!(listed.len(), 2);

    // Independently payable: settle part 1 in full.
    let pay1 = record_payment_impl(
        &state,
        &order.holler_order_id,
        "CASH",
        21_000,
        Some(21_000),
        Some(0),
        None,
        None,
        None,
        Some(invoices[0].id.clone()),
        USER_ID,
    )
    .expect("settle part 1");
    assert_eq!(pay1.amount_paise, 21_000);

    // Part 2's remaining due is untouched by part 1's settlement: a tender
    // exceeding part 2's OWN remaining due (21_000) is rejected naming that
    // amount, not zero.
    let over = record_payment_impl(
        &state,
        &order.holler_order_id,
        "CASH",
        21_001,
        Some(21_001),
        Some(0),
        None,
        None,
        None,
        Some(invoices[1].id.clone()),
        USER_ID,
    )
    .expect_err("part 2 must still have its own full remaining due, unaffected by part 1");
    assert_eq!(over.code, "FORWARD_PAYMENT_EXCEEDS_REMAINING_DUE");

    // ...but tendering exactly part 2's remaining due succeeds.
    let pay2 = record_payment_impl(
        &state,
        &order.holler_order_id,
        "CASH",
        21_000,
        Some(21_000),
        Some(0),
        None,
        None,
        None,
        Some(invoices[1].id.clone()),
        USER_ID,
    )
    .expect("settle part 2 independently of part 1");
    assert_eq!(pay2.amount_paise, 21_000);
}

/// An over-billed split (both parts claim the same unit, doubling the
/// order's quantity) is rejected atomically, with no invoice row left for
/// either part and no invoice number consumed — falsified against
/// `invoice_sequence_total` before/after, the same probe the edge crate's
/// own HSN/SAC split test uses.
#[test]
fn an_over_billed_split_is_rejected_atomically_and_burns_no_invoice_number() {
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
    let order_item_id = order.items[0].id.clone();
    let before = invoice_sequence_total(&state);

    // Two parts, each claiming the SAME quantity-1 line against an order
    // line whose real quantity is 1 — over-billed by one unit.
    let parts = vec![
        SplitPartInput {
            lines: vec![SplitLineInput {
                order_item_id: order_item_id.clone(),
                quantity: 1,
            }],
        },
        SplitPartInput {
            lines: vec![SplitLineInput {
                order_item_id: order_item_id.clone(),
                quantity: 1,
            }],
        },
    ];

    let err = issue_split_invoices_impl(&state, &order.holler_order_id, USER_ID, &parts, &[])
        .expect_err("an over-billed split must be rejected");
    assert_eq!(err.code, "INVALID_INPUT");
    assert!(err.message.contains(&order_item_id));

    assert!(
        list_invoices_for_order_impl(&state, &order.holler_order_id)
            .expect("list")
            .is_empty(),
        "a rejected over-billed split must leave no invoice row for the order"
    );
    assert_eq!(
        invoice_sequence_total(&state),
        before,
        "a rejected over-billed split must not consume any invoice number"
    );
}

/// An under-billed split (one part covers only half the order's quantity)
/// is rejected the same way — atomically, no row, no number consumed.
#[test]
fn an_under_billed_split_is_rejected_atomically_and_burns_no_invoice_number() {
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
    let order_item_id = order.items[0].id.clone();
    let before = invoice_sequence_total(&state);

    // Only ONE part, billing quantity 1 of an order line whose real
    // quantity is 2 — under-billed by one unit, nothing bills the rest.
    let parts = vec![SplitPartInput {
        lines: vec![SplitLineInput {
            order_item_id: order_item_id.clone(),
            quantity: 1,
        }],
    }];

    let err = issue_split_invoices_impl(&state, &order.holler_order_id, USER_ID, &parts, &[])
        .expect_err("an under-billed split must be rejected");
    assert_eq!(err.code, "INVALID_INPUT");
    assert!(err.message.contains(&order_item_id));

    assert!(
        list_invoices_for_order_impl(&state, &order.holler_order_id)
            .expect("list")
            .is_empty(),
        "a rejected under-billed split must leave no invoice row for the order"
    );
    assert_eq!(
        invoice_sequence_total(&state),
        before,
        "a rejected under-billed split must not consume any invoice number"
    );
}

/// A split part carrying a discounted line still prices correctly: the
/// SAME 10%-off worked example as
/// `a_line_discount_reduces_the_taxable_value_and_gst_is_computed_on_the_net`,
/// but issued as one part of a 2-way split rather than as a whole-order
/// bill — proving `build_split_part_lines` reuses `discounts_by_item`
/// rather than silently dropping it for the split path.
#[test]
fn a_split_part_with_a_discounted_line_prices_correctly() {
    let state = app_state();
    {
        let db = state.db.lock().expect("lock");
        seed_discount_definition(
            &db,
            "disc-staff10",
            "STAFF10",
            "LINE",
            "PERCENT",
            Some(1000), // 10.00%
            None,
            None,
            None,
            false,
        );
    }

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
    let order_item_id = order.items[0].id.clone();

    let parts = vec![
        SplitPartInput {
            lines: vec![SplitLineInput {
                order_item_id: order_item_id.clone(),
                quantity: 1,
            }],
        },
        SplitPartInput {
            lines: vec![SplitLineInput {
                order_item_id: order_item_id.clone(),
                quantity: 1,
            }],
        },
    ];
    let discounts = vec![LineDiscountInput {
        order_item_id: order_item_id.clone(),
        discount_definition_id: "disc-staff10".to_string(),
        reason: None,
    }];

    let invoices =
        issue_split_invoices_impl(&state, &order.holler_order_id, USER_ID, &parts, &discounts)
            .expect("issue a split with a discounted line");
    assert_eq!(invoices.len(), 2);

    // Per unit: 10% of Rs.200.00 = Rs.20.00 (2000 paise) discount, applied
    // to EACH part's single unit (same figures as the whole-order worked
    // example, halved because each part carries quantity 1 not 2).
    for invoice in &invoices {
        let line = &invoice.lines[0];
        assert_eq!(line.quantity, 1);
        assert_eq!(line.gross_paise, 20_000);
        assert_eq!(line.discount_paise, 2_000, "the 10% discount applies per part too");
        assert_eq!(line.taxable_value_paise, 18_000);
        assert_eq!(line.cgst_paise, 450);
        assert_eq!(line.sgst_paise, 450);
        assert_eq!(invoice.grand_total_paise, 18_000 + 450 + 450);
    }

    // Conservation still holds with the discount applied.
    let total_grand: i64 = invoices.iter().map(|inv| inv.grand_total_paise).sum();
    assert_eq!(total_grand, 2 * (18_000 + 450 + 450));
}
