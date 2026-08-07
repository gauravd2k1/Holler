//! Integration tests proving Milestone 1's acceptance criterion: "internet
//! may be disconnected and the cashier can still create restaurant orders."
//!
//! Every test here uses `Db::open_in_memory_for_tests()` — no network socket
//! is ever opened, no HTTP client exists in this crate's dependency graph at
//! all (only `tauri`, `holler-edge-database`, `serde*`, `uuid`, `chrono`,
//! `thiserror`), so "WAN unavailable" is not merely simulated: nothing in
//! this dependency tree is capable of reaching the network.

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine as _;
use holler_edge_database::{model, repo, Db};

use holler_pos_lib::commands::auth::login_impl;
use holler_pos_lib::commands::menu::list_menu_items_impl;
use holler_pos_lib::commands::orders::{
    create_order_impl, get_order_impl, list_orders_impl, NewOrderItemRequest,
};
use holler_pos_lib::commands::tables::list_tables_impl;
use holler_pos_lib::state::AppState;

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD_NO_PAD;

/// Mirrors backend/internal/platform/crypto/password.go's Argon2id encoding
/// so tests exercise the exact wire format the cloud hash minter produces.
fn hash_password(plaintext: &str) -> String {
    let salt = [7u8; 16];
    let params = Params::new(64 * 1024, 2, 4, Some(32)).expect("valid argon2 params");
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; 32];
    argon2
        .hash_password_into(plaintext.as_bytes(), &salt, &mut key)
        .expect("hash");
    format!(
        "$argon2id$v={}$m={},t={},p={}${}${}",
        Version::V0x13 as u32,
        64 * 1024,
        2,
        4,
        B64.encode(salt),
        B64.encode(key)
    )
}

const OUTLET_ID: &str = "outlet-1";
const DEVICE_ID: &str = "device-1";
const TENANT_ID: &str = "tenant-1";

fn seed(db: &Db) {
    let conn = db.connection();
    repo::upsert_outlet(
        conn,
        &model::Outlet {
            id: OUTLET_ID.to_string(),
            brand_id: "brand-1".to_string(),
            name: "Test Outlet".to_string(),
            timezone: "Asia/Kolkata".to_string(),
            config_version: 1,
            created_at: "2026-08-07T00:00:00.000Z".to_string(),
            updated_at: "2026-08-07T00:00:00.000Z".to_string(),
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
            created_at: "2026-08-07T00:00:00.000Z".to_string(),
        },
    )
    .expect("seed device");

    repo::replace_app_user(
        conn,
        &model::AppUser {
            id: "user-1".to_string(),
            tenant_id: TENANT_ID.to_string(),
            outlet_id: OUTLET_ID.to_string(),
            email: "cashier@holler.test".to_string(),
            full_name: "Cashier One".to_string(),
            password_hash: hash_password("correct horse battery staple"),
            pin_hash: None,
            is_active: true,
            permissions_json: serde_json::to_string(&["order.create", "order.modify"]).unwrap(),
            config_version: 1,
            updated_at: "2026-08-07T00:00:00.000Z".to_string(),
        },
    )
    .expect("seed app_user");

    repo::upsert_menu_category(
        conn,
        &model::MenuCategory {
            id: "cat-1".to_string(),
            outlet_id: OUTLET_ID.to_string(),
            name: "Starters".to_string(),
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
            name: "Paneer Tikka".to_string(),
            base_price_paise: 25000,
            is_available: true,
            config_version: 1,
        },
    )
    .expect("seed menu item");

    repo::upsert_restaurant_table(
        conn,
        &model::RestaurantTable {
            id: "table-1".to_string(),
            outlet_id: OUTLET_ID.to_string(),
            section: "Main".to_string(),
            label: "T1".to_string(),
            seat_count: 4,
            is_active: true,
            config_version: 1,
        },
    )
    .expect("seed table");
}

fn seeded_state() -> AppState {
    let db = Db::open_in_memory_for_tests().expect("open in-memory db");
    seed(&db);
    AppState::new(db, OUTLET_ID.to_string(), DEVICE_ID.to_string())
}

// ------------------------------------------------------------ offline login --

#[test]
fn offline_login_succeeds_with_correct_password() {
    let state = seeded_state();
    let principal = login_impl(
        &state,
        "cashier@holler.test",
        "correct horse battery staple",
    )
    .expect("login should succeed");

    assert!(principal.authenticated_offline);
    assert_eq!(principal.outlet_id, OUTLET_ID);
    assert_eq!(principal.tenant_id, TENANT_ID);
    assert!(principal.permissions.contains(&"order.create".to_string()));
}

#[test]
fn offline_login_fails_with_wrong_password() {
    let state = seeded_state();
    let err =
        login_impl(&state, "cashier@holler.test", "wrong password").expect_err("login should fail");
    assert_eq!(err.code, "CREDENTIAL_MISMATCH");
}

#[test]
fn offline_login_fails_for_unknown_email() {
    let state = seeded_state();
    let err = login_impl(&state, "nobody@holler.test", "whatever").expect_err("must fail");
    assert_eq!(err.code, "CREDENTIAL_MISMATCH");
}

// -------------------------------------------------------------- menu/table --

#[test]
fn menu_items_are_readable_for_the_outlet() {
    let state = seeded_state();
    let items = list_menu_items_impl(&state).expect("list menu items");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].base_price_paise, 25000);
}

#[test]
fn tables_are_readable_for_the_outlet() {
    let state = seeded_state();
    let tables = list_tables_impl(&state).expect("list tables");
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].label, "T1");
}

// ------------------------------------------------------------------- order --

/// THE CRITICAL TEST required by the task: a full create-order flow, with
/// nothing in this process capable of reaching a network, proving Milestone
/// 1's acceptance criterion — "internet may be disconnected and the cashier
/// can still create restaurant orders."
#[test]
fn creates_an_order_fully_offline_and_it_is_immediately_readable() {
    let state = seeded_state();

    let order = create_order_impl(
        &state,
        "DINE_IN".to_string(),
        Some("table-1".to_string()),
        vec![NewOrderItemRequest {
            menu_item_id: "item-1".to_string(),
            variant_id: None,
            quantity: 2,
            unit_price_paise: 25000,
            notes: None,
        }],
    )
    .expect("order creation must succeed with the WAN unavailable");

    assert_eq!(order.status, "DRAFT");
    assert_eq!(order.subtotal_paise, 50000);
    assert_eq!(
        order.total_paise, 50000,
        "M1 excludes tax/discount computation"
    );
    assert_eq!(order.items.len(), 1);
    assert_eq!(order.items[0].line_total_paise, 50000);
    assert_eq!(order.source, "POS");
    assert_eq!(order.schema_version, 1);

    // Round-trips through storage.
    let fetched = get_order_impl(&state, &order.holler_order_id)
        .expect("get_order should succeed")
        .expect("order must exist");
    assert_eq!(fetched.holler_order_id, order.holler_order_id);
    assert_eq!(fetched.total_paise, 50000);

    let listed = list_orders_impl(&state).expect("list_orders should succeed");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].holler_order_id, order.holler_order_id);
}

/// The order row and its `local_outbox` row must commit atomically
/// (ADR-007) — asserted directly against the outbox, not just inferred from
/// the order being readable.
#[test]
fn order_and_its_outbox_row_are_both_present_and_committed_together() {
    let state = seeded_state();

    let order = create_order_impl(
        &state,
        "TAKEAWAY".to_string(),
        None,
        vec![NewOrderItemRequest {
            menu_item_id: "item-1".to_string(),
            variant_id: None,
            quantity: 1,
            unit_price_paise: 25000,
            notes: None,
        }],
    )
    .expect("create order");

    let db = state.db.lock().unwrap();
    assert!(db.get_order(&order.holler_order_id).unwrap().is_some());

    let pending = repo::list_unpublished_outbox(db.connection(), 10).unwrap();
    assert!(pending
        .iter()
        .any(|e| e.aggregate_id == order.holler_order_id
            && e.aggregate_type == "order"
            && e.event_type == "OrderCreated"));
}

/// A failed order write (an item referencing a nonexistent menu item — a
/// FOREIGN KEY violation) must leave neither the order row nor its outbox
/// row committed.
#[test]
fn a_failed_order_write_leaves_neither_order_nor_outbox_row() {
    let state = seeded_state();

    let result = create_order_impl(
        &state,
        "DINE_IN".to_string(),
        None,
        vec![NewOrderItemRequest {
            menu_item_id: "does-not-exist".to_string(),
            variant_id: None,
            quantity: 1,
            unit_price_paise: 10000,
            notes: None,
        }],
    );
    assert!(result.is_err(), "FK violation must fail the write");

    let db = state.db.lock().unwrap();
    let orders = repo::list_orders_for_outlet(db.connection(), OUTLET_ID).unwrap();
    assert!(orders.is_empty(), "no order row may survive a failed write");

    let pending = repo::list_unpublished_outbox(db.connection(), 10).unwrap();
    assert!(
        pending.is_empty(),
        "no outbox row may survive a failed write"
    );
}

/// Quantity <= 0 is rejected by the domain layer before any SQLite write is
/// attempted, and produces integer-paise arithmetic (never float) all the
/// way through.
#[test]
fn totals_arithmetic_is_integer_paise_and_rejects_non_positive_quantity() {
    let state = seeded_state();

    let order = create_order_impl(
        &state,
        "DINE_IN".to_string(),
        None,
        vec![
            NewOrderItemRequest {
                menu_item_id: "item-1".to_string(),
                variant_id: None,
                quantity: 3,
                unit_price_paise: 25000,
                notes: None,
            },
            NewOrderItemRequest {
                menu_item_id: "item-1".to_string(),
                variant_id: None,
                quantity: 1,
                unit_price_paise: 25000,
                notes: Some("extra spicy".to_string()),
            },
        ],
    )
    .expect("create order");
    assert_eq!(order.subtotal_paise, 100000);
    assert_eq!(order.total_paise, 100000);

    let err = create_order_impl(
        &state,
        "DINE_IN".to_string(),
        None,
        vec![NewOrderItemRequest {
            menu_item_id: "item-1".to_string(),
            variant_id: None,
            quantity: 0,
            unit_price_paise: 25000,
            notes: None,
        }],
    )
    .expect_err("zero quantity must be rejected");
    assert_eq!(err.code, "INVALID_QUANTITY");

    let db = state.db.lock().unwrap();
    let orders = repo::list_orders_for_outlet(db.connection(), OUTLET_ID).unwrap();
    assert_eq!(
        orders.len(),
        1,
        "the rejected order must not have been written"
    );
}
