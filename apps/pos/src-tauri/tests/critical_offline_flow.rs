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
use holler_edge_database::crypto::EncryptionKey;
use holler_edge_database::{model, repo, Db};
use holler_edge_device::contract::KdsLanMessage;
use holler_edge_device::Hub;

use holler_pos_lib::commands::auth::login_impl;
use holler_pos_lib::commands::kitchen::{
    list_failed_print_jobs_impl, list_kots_for_order_impl, list_stations_impl,
    send_order_to_kitchen_impl, transition_kot_status_impl,
};
use holler_pos_lib::commands::menu::{list_menu_categories_impl, list_menu_items_impl};
use holler_pos_lib::commands::orders::{
    add_order_item_impl, create_order_impl, get_active_draft_order_impl, get_order_impl,
    list_orders_impl, remove_order_item_impl, update_order_item_quantity_impl,
    update_order_shape_impl, NewOrderItemModifierRequest, NewOrderItemRequest,
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

/// Extends `seed` with a station, a network printer routed to it, and that
/// station wired as `item-1`'s route — the minimal config a KOT test needs
/// (ADR-014 §1-2). Kept separate from `seed` so tests that do not touch the
/// kitchen stay unaffected.
fn seed_kitchen(db: &Db) {
    let conn = db.connection();
    repo::upsert_station(
        conn,
        &model::Station {
            id: "station-1".to_string(),
            outlet_id: OUTLET_ID.to_string(),
            code: "MAIN_KITCHEN".to_string(),
            name: "Main Kitchen".to_string(),
            sort_order: 1,
            is_active: true,
            config_version: 1,
        },
    )
    .expect("seed station");

    repo::replace_menu_item_stations(conn, "item-1", &["station-1".to_string()], 1)
        .expect("seed menu_item_station");

    repo::upsert_printer(
        conn,
        &model::Printer {
            id: "printer-1".to_string(),
            outlet_id: OUTLET_ID.to_string(),
            name: "Kitchen Printer".to_string(),
            connection_kind: "ESCPOS_NETWORK".to_string(),
            // Deliberately unreachable: no listener exists in this test
            // process, so the immediate print attempt fails and lands the
            // job in FAILED — exactly the case `list_failed_print_jobs`
            // exists to surface. Nothing here opens a real socket to the
            // network; connect failure is local and instant.
            address: "127.0.0.1:1".to_string(),
            paper_width_mm: 80,
            is_active: true,
            config_version: 1,
        },
    )
    .expect("seed printer");

    repo::replace_station_printers(conn, "station-1", &["printer-1".to_string()], 1)
        .expect("seed station_printer");
}

fn seeded_state() -> AppState {
    let db = Db::open_in_memory_for_tests().expect("open in-memory db");
    seed(&db);
    AppState::new(db, OUTLET_ID.to_string(), DEVICE_ID.to_string())
}

/// Same seeded kitchen fixture as `seeded_kitchen_state`, but wired to a
/// caller-supplied `Hub` (no real socket bound) so T12's notify wiring
/// (`commands::kitchen::notify_kot`) can be asserted directly — proves the
/// wiring exists and fires, independent of the actual WebSocket transport
/// already covered by `edge/device`'s own tests and the cross-language
/// `tests/integration/kds-lan` suite.
fn seeded_kitchen_state_with_hub(hub: std::sync::Arc<Hub>) -> AppState {
    let db = Db::open_in_memory_for_tests().expect("open in-memory db");
    seed(&db);
    seed_kitchen(&db);
    AppState::new_with_hub(db, OUTLET_ID.to_string(), DEVICE_ID.to_string(), hub)
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
            modifiers: vec![],
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
            modifiers: vec![],
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
            modifiers: vec![],
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
                modifiers: vec![],
            },
            NewOrderItemRequest {
                menu_item_id: "item-1".to_string(),
                variant_id: None,
                quantity: 1,
                unit_price_paise: 25000,
                notes: Some("extra spicy".to_string()),
                modifiers: vec![],
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
            modifiers: vec![],
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

// --------------------------------------------------------------- menu read --

#[test]
fn menu_categories_are_readable_for_the_outlet() {
    let state = seeded_state();
    let categories = list_menu_categories_impl(&state).expect("list categories");
    assert_eq!(categories.len(), 1);
    assert_eq!(categories[0].name, "Starters");
}

// ------------------------------------------------ add/remove item (cart persistence) --

#[test]
fn add_and_remove_item_on_a_draft_order_round_trip_through_storage() {
    let state = seeded_state();
    let order = create_order_impl(
        &state,
        "DINE_IN".to_string(),
        None,
        vec![NewOrderItemRequest {
            menu_item_id: "item-1".to_string(),
            variant_id: None,
            quantity: 1,
            unit_price_paise: 25000,
            notes: None,
            modifiers: vec![],
        }],
    )
    .expect("create order");
    assert_eq!(order.items.len(), 1);

    let after_add = add_order_item_impl(
        &state,
        &order.holler_order_id,
        NewOrderItemRequest {
            menu_item_id: "item-1".to_string(),
            variant_id: None,
            quantity: 2,
            unit_price_paise: 25000,
            notes: Some("no onions".to_string()),
            modifiers: vec![],
        },
    )
    .expect("add item to draft order");
    assert_eq!(after_add.items.len(), 2);
    assert_eq!(after_add.subtotal_paise, 25000 + 50000);
    assert_eq!(after_add.total_paise, after_add.subtotal_paise);

    let added_item_id = after_add
        .items
        .iter()
        .find(|i| i.notes.as_deref() == Some("no onions"))
        .expect("added line present")
        .id
        .clone();

    let after_remove =
        remove_order_item_impl(&state, &order.holler_order_id, &added_item_id).expect("remove item");
    assert_eq!(after_remove.items.len(), 1);
    assert_eq!(after_remove.subtotal_paise, 25000);
}

/// THE TEST THAT MATTERS (T9): a cart line written through to SQLite as it
/// happens must still be there after the process that wrote it is gone and
/// a completely fresh one opens the same encrypted file. This is what
/// distinguishes "an API exists that can prevent the loss" from "the loss
/// does not happen" (docs/retro.md 2026-08-10) — nothing here asserts that
/// `add_order_item`/`remove_order_item` were *called*; it constructs a
/// cart, discards the entire in-memory `AppState`/`Db` (no shared
/// connection, no in-memory-only handle — a real file under a temp dir,
/// closed and reopened), and asserts what a fresh read of SQLite alone
/// produces.
#[test]
fn a_draft_order_survives_the_pos_process_ending_and_a_fresh_one_reopening() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sealed = dir.path().join("edge.db.enc");
    let plaintext = dir.path().join("edge.db");

    let order_id: String;
    let first_item_id: String;
    let second_item_id: String;

    // ---- "session 1": the cashier builds a cart, nothing is ever sent. ----
    {
        let db = Db::open(&sealed, &plaintext, EncryptionKey::new([42u8; 32])).expect("open db");
        seed(&db);
        let state = AppState::new(db, OUTLET_ID.to_string(), DEVICE_ID.to_string());

        // First line lands -> this is what creates the DRAFT order (task
        // requirement 1: "A DRAFT order is created when the first line
        // lands, not at Send").
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
                modifiers: vec![],
            }],
        )
        .expect("first line persists the draft order");
        order_id = order.holler_order_id.clone();
        first_item_id = order.items[0].id.clone();

        // A second line lands via add-item, as a second tap on the menu
        // would drive.
        let after_second = add_order_item_impl(
            &state,
            &order_id,
            NewOrderItemRequest {
                menu_item_id: "item-1".to_string(),
                variant_id: None,
                quantity: 1,
                unit_price_paise: 25000,
                notes: Some("no onions".to_string()),
                modifiers: vec![],
            },
        )
        .expect("second line persists");
        second_item_id = after_second
            .items
            .iter()
            .find(|i| i.notes.as_deref() == Some("no onions"))
            .expect("second line present")
            .id
            .clone();

        // The whole point: no `create_order` at "Send" — the cashier never
        // sends. `state`/`db` are dropped here without any further app
        // action, exactly as a killed process leaves no chance to run
        // cleanup code the frontend would have to explicitly trigger.
    }

    // ---- "session 2": a fresh process, fresh AppState, same file. ----
    let db2 = Db::open(&sealed, &plaintext, EncryptionKey::new([42u8; 32]))
        .expect("reopen must succeed");
    let state2 = AppState::new(db2, OUTLET_ID.to_string(), DEVICE_ID.to_string());

    let recovered = get_active_draft_order_impl(&state2)
        .expect("recovery read must succeed")
        .expect("a DRAFT order must be recoverable for this device");

    assert_eq!(recovered.holler_order_id, order_id);
    assert_eq!(recovered.status, "DRAFT");
    assert_eq!(recovered.order_type, "DINE_IN");
    assert_eq!(recovered.table_id.as_deref(), Some("table-1"));
    assert_eq!(recovered.items.len(), 2, "both lines must survive");

    let recovered_first = recovered
        .items
        .iter()
        .find(|i| i.id == first_item_id)
        .expect("first line recoverable");
    assert_eq!(recovered_first.quantity, 2);
    assert_eq!(recovered_first.unit_price_paise, 25000);
    assert_eq!(recovered_first.line_total_paise, 50000);

    let recovered_second = recovered
        .items
        .iter()
        .find(|i| i.id == second_item_id)
        .expect("second line recoverable");
    assert_eq!(recovered_second.quantity, 1);
    assert_eq!(recovered_second.notes.as_deref(), Some("no onions"));

    assert_eq!(
        recovered.subtotal_paise,
        50000 + 25000,
        "recomputed totals must also survive, in integer paise"
    );

    // Also reachable the ordinary way a re-opened app would query it.
    let fetched = get_order_impl(&state2, &order_id)
        .expect("get_order must succeed")
        .expect("order still exists");
    assert_eq!(fetched.items.len(), 2);
}

/// A DRAFT order with no active device match (e.g. a different device's
/// order, or none at all) must recover as `None`, not error and not return
/// someone else's in-progress cart.
#[test]
fn recovery_finds_nothing_when_there_is_no_draft_order_for_this_device() {
    let state = seeded_state();
    assert!(get_active_draft_order_impl(&state)
        .expect("must succeed with no orders at all")
        .is_none());

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
            modifiers: vec![],
        }],
    )
    .expect("create order");
    holler_pos_lib::commands::orders::confirm_order_impl(&state, &order.holler_order_id)
        .expect("confirm order");

    // The only order for this device/outlet is now CONFIRMED, not DRAFT —
    // recovery must not resurrect it into the cart.
    assert!(get_active_draft_order_impl(&state)
        .expect("must succeed")
        .is_none());
}

/// `#132-A` (docs/spec/kitchen.md's #132 -> #132-A change history,
/// docs/m3-planning.md Track B) widened `add_order_item` beyond DRAFT: a
/// cashier adding a line after the order is already CONFIRMED (or with the
/// kitchen) must succeed, not be rejected the way it used to be. This test
/// used to assert the opposite (`add_item_rejects_a_non_draft_order`) —
/// renamed and inverted once the widening landed; the genuine
/// terminal-status rejection is
/// `add_item_rejects_an_order_that_has_already_left_the_active_lifecycle`
/// below.
#[test]
fn add_item_succeeds_on_a_confirmed_order_the_132a_post_draft_path() {
    let state = seeded_state();
    let order = create_order_impl(
        &state,
        "DINE_IN".to_string(),
        None,
        vec![NewOrderItemRequest {
            menu_item_id: "item-1".to_string(),
            variant_id: None,
            quantity: 1,
            unit_price_paise: 25000,
            notes: None,
            modifiers: vec![],
        }],
    )
    .expect("create order");

    holler_pos_lib::commands::orders::confirm_order_impl(&state, &order.holler_order_id)
        .expect("confirm order");

    let after_add = add_order_item_impl(
        &state,
        &order.holler_order_id,
        NewOrderItemRequest {
            menu_item_id: "item-1".to_string(),
            variant_id: None,
            quantity: 1,
            unit_price_paise: 25000,
            notes: None,
            modifiers: vec![],
        },
    )
    .expect("add-item must succeed post-confirmation (#132-A)");
    assert_eq!(after_add.items.len(), 2);
}

/// The genuine terminal-status rejection: once the order is `SERVED` (no
/// Tauri command reaches that state today, so it is stamped directly at the
/// storage layer here, exactly as `edge/database`'s own equivalent tests
/// do), a line can no longer be added.
#[test]
fn add_item_rejects_an_order_that_has_already_left_the_active_lifecycle() {
    let state = seeded_state();
    let order = create_order_impl(
        &state,
        "DINE_IN".to_string(),
        None,
        vec![NewOrderItemRequest {
            menu_item_id: "item-1".to_string(),
            variant_id: None,
            quantity: 1,
            unit_price_paise: 25000,
            notes: None,
            modifiers: vec![],
        }],
    )
    .expect("create order");

    {
        let db = state.db.lock().unwrap();
        db.connection()
            .execute(
                "UPDATE \"order\" SET status = 'SERVED' WHERE id = ?1",
                [&order.holler_order_id],
            )
            .unwrap();
    }

    let err = add_order_item_impl(
        &state,
        &order.holler_order_id,
        NewOrderItemRequest {
            menu_item_id: "item-1".to_string(),
            variant_id: None,
            quantity: 1,
            unit_price_paise: 25000,
            notes: None,
            modifiers: vec![],
        },
    )
    .expect_err("must reject amendment of a served order");
    assert_eq!(err.code, "ORDER_NOT_DRAFT");
}

// ------------------------------------------- T14: order-shape lock (P0) --
// docs/retro.md P0 regression: the DRAFT order created on the first tapped
// item locked order_type/table_id at whatever they were at that moment
// (DINE_IN/no table by default), and the POS had no command to correct
// them — Send could never enable. These tests are the regression: they must
// fail against the pre-fix behaviour (no `update_order_shape` existed at
// all) and pass once the shape stays editable through DRAFT.

/// The cashier's exact stuck path: tap an item (DRAFT locks in at the
/// default DINE_IN/no table), then correct the order type — must persist to
/// SQLite, not just an in-memory intention.
#[test]
fn order_type_can_be_changed_after_the_first_item_lands_and_it_persists() {
    let state = seeded_state();
    let order = create_order_impl(
        &state,
        "DINE_IN".to_string(),
        None,
        vec![NewOrderItemRequest {
            menu_item_id: "item-1".to_string(),
            variant_id: None,
            quantity: 1,
            unit_price_paise: 25000,
            notes: None,
            modifiers: vec![],
        }],
    )
    .expect("first tap creates the draft order");
    assert_eq!(order.order_type, "DINE_IN");

    let updated = update_order_shape_impl(
        &state,
        &order.holler_order_id,
        "TAKEAWAY".to_string(),
        None,
    )
    .expect("shape change on a DRAFT order must succeed");
    assert_eq!(updated.order_type, "TAKEAWAY");
    assert_eq!(updated.table_id, None);

    // Read back from storage directly — not just the call's return value —
    // proving the change actually landed in SQLite.
    let fetched = get_order_impl(&state, &order.holler_order_id)
        .expect("get_order")
        .expect("order exists");
    assert_eq!(fetched.order_type, "TAKEAWAY");
}

/// The exact escape from the stuck-DINE_IN-with-no-table bug: add an item on
/// the default DINE_IN, set a table, and the order becomes sendable
/// (sendability itself is a frontend concern, but the table must actually
/// land on the order for that frontend check to ever pass).
#[test]
fn setting_a_table_after_the_first_item_lands_makes_the_order_carry_it() {
    let state = seeded_state();
    let order = create_order_impl(
        &state,
        "DINE_IN".to_string(),
        None,
        vec![NewOrderItemRequest {
            menu_item_id: "item-1".to_string(),
            variant_id: None,
            quantity: 1,
            unit_price_paise: 25000,
            notes: None,
            modifiers: vec![],
        }],
    )
    .expect("first tap creates the draft order with no table");
    assert_eq!(order.table_id, None);

    let updated = update_order_shape_impl(
        &state,
        &order.holler_order_id,
        "DINE_IN".to_string(),
        Some("table-1".to_string()),
    )
    .expect("setting a table on a DRAFT order must succeed");
    assert_eq!(updated.table_id.as_deref(), Some("table-1"));
}

/// Once the order has left DRAFT (confirmed / with the kitchen), the shape
/// is history — this must be a rejection, not a silent no-op.
#[test]
fn order_shape_cannot_be_changed_once_the_order_leaves_draft() {
    let state = seeded_state();
    let order = create_order_impl(
        &state,
        "DINE_IN".to_string(),
        Some("table-1".to_string()),
        vec![NewOrderItemRequest {
            menu_item_id: "item-1".to_string(),
            variant_id: None,
            quantity: 1,
            unit_price_paise: 25000,
            notes: None,
            modifiers: vec![],
        }],
    )
    .expect("create order");

    holler_pos_lib::commands::orders::confirm_order_impl(&state, &order.holler_order_id)
        .expect("confirm order");

    let err = update_order_shape_impl(
        &state,
        &order.holler_order_id,
        "TAKEAWAY".to_string(),
        None,
    )
    .expect_err("shape must be immutable once the order has left DRAFT");
    assert_eq!(err.code, "ORDER_NOT_DRAFT");

    let fetched = get_order_impl(&state, &order.holler_order_id)
        .expect("get_order")
        .expect("order exists");
    assert_eq!(
        fetched.order_type, "DINE_IN",
        "the rejected attempt must not have changed the stored shape"
    );
    assert_eq!(fetched.table_id.as_deref(), Some("table-1"));
}

/// The startup-hydrate case: a DRAFT order recovered after a fresh process
/// start (crash / restart) must still be shape-editable, and the correction
/// must survive a second reopen of the same encrypted file — proving the
/// fix actually rescues the cashier already stuck with this bug, not just a
/// freshly created order in the same process.
#[test]
fn a_recovered_draft_orders_shape_can_be_corrected_and_it_survives_reopening() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sealed = dir.path().join("edge.db.enc");
    let plaintext = dir.path().join("edge.db");

    let order_id: String;

    // ---- "session 1": cashier taps an item, gets stuck at DINE_IN/no table. ----
    {
        let db = Db::open(&sealed, &plaintext, EncryptionKey::new([9u8; 32])).expect("open db");
        seed(&db);
        let state = AppState::new(db, OUTLET_ID.to_string(), DEVICE_ID.to_string());

        let order = create_order_impl(
            &state,
            "DINE_IN".to_string(),
            None,
            vec![NewOrderItemRequest {
                menu_item_id: "item-1".to_string(),
                variant_id: None,
                quantity: 1,
                unit_price_paise: 25000,
                notes: None,
                modifiers: vec![],
            }],
        )
        .expect("first tap creates the draft order");
        order_id = order.holler_order_id.clone();
        // Process ends here without the cashier ever fixing the shape or
        // sending — models the crash/restart this bug left cashiers stuck
        // behind.
    }

    // ---- "session 2": a fresh process recovers the stuck draft and fixes it. ----
    {
        let db2 = Db::open(&sealed, &plaintext, EncryptionKey::new([9u8; 32]))
            .expect("reopen must succeed");
        let state2 = AppState::new(db2, OUTLET_ID.to_string(), DEVICE_ID.to_string());

        let recovered = get_active_draft_order_impl(&state2)
            .expect("recovery read must succeed")
            .expect("the stuck draft must be recoverable");
        assert_eq!(recovered.holler_order_id, order_id);
        assert_eq!(recovered.order_type, "DINE_IN");
        assert_eq!(recovered.table_id, None);

        update_order_shape_impl(
            &state2,
            &recovered.holler_order_id,
            "DINE_IN".to_string(),
            Some("table-1".to_string()),
        )
        .expect("the recovered draft's shape must be editable");
    }

    // ---- "session 3": the correction itself must be durable. ----
    let db3 =
        Db::open(&sealed, &plaintext, EncryptionKey::new([9u8; 32])).expect("reopen must succeed");
    let state3 = AppState::new(db3, OUTLET_ID.to_string(), DEVICE_ID.to_string());
    let fetched = get_order_impl(&state3, &order_id)
        .expect("get_order")
        .expect("order still exists");
    assert_eq!(fetched.order_type, "DINE_IN");
    assert_eq!(fetched.table_id.as_deref(), Some("table-1"));
}

// ---------------------------------------------------------------- kitchen --

fn seeded_kitchen_state() -> AppState {
    let db = Db::open_in_memory_for_tests().expect("open in-memory db");
    seed(&db);
    seed_kitchen(&db);
    AppState::new(db, OUTLET_ID.to_string(), DEVICE_ID.to_string())
}

#[test]
fn send_to_kitchen_produces_one_ticket_per_routed_station_and_it_is_listable() {
    let state = seeded_kitchen_state();
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
            modifiers: vec![],
        }],
    )
    .expect("create order");

    holler_pos_lib::commands::orders::confirm_order_impl(&state, &order.holler_order_id)
        .expect("confirm order");
    let kots =
        send_order_to_kitchen_impl(&state, &order.holler_order_id).expect("send to kitchen");
    assert_eq!(kots.len(), 1, "item-1 routes to exactly one station");
    assert_eq!(kots[0].station, "MAIN_KITCHEN");
    assert_eq!(kots[0].status, "NEW");
    assert_eq!(kots[0].items.len(), 1);
    assert_eq!(kots[0].items[0].quantity, 2);

    let listed =
        list_kots_for_order_impl(&state, &order.holler_order_id).expect("list kots for order");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, kots[0].id);
}

/// Regression for docs/backlog-m2.md Track A / docs/m3-planning.md §2 Track
/// A, at the Tauri command boundary the POS UI actually calls: a mixed
/// order with one routed line and one unrouted line used to send
/// "successfully", ticketing only the routed item and dropping the other
/// with no signal anywhere. This asserts `send_order_to_kitchen_impl` now
/// returns `AppError { code: "UNROUTED_KITCHEN_ITEMS", .. }` naming the
/// dropped item, and that **no** `Kot` was created for either line — the
/// routed item must not go to the kitchen while the unrouted one silently
/// vanishes.
#[test]
fn send_to_kitchen_rejects_a_mixed_order_and_names_the_unrouted_item() {
    let state = seeded_kitchen_state();

    // A second menu item that is never routed to any station — the config
    // gap this test exercises.
    {
        let db = state.db.lock().expect("lock db");
        repo::upsert_menu_item(
            db.connection(),
            &model::MenuItem {
                id: "item-unrouted".to_string(),
                outlet_id: OUTLET_ID.to_string(),
                category_id: "cat-1".to_string(),
                name: "Mystery Side".to_string(),
                base_price_paise: 8000,
                is_available: true,
                config_version: 1,
            },
        )
        .expect("seed unrouted menu item");
    }

    let order = create_order_impl(
        &state,
        "DINE_IN".to_string(),
        Some("table-1".to_string()),
        vec![
            NewOrderItemRequest {
                menu_item_id: "item-1".to_string(),
                variant_id: None,
                quantity: 1,
                unit_price_paise: 25000,
                notes: None,
                // Added when T3's modifier support merged with this T2 test.
                // Empty is the point: this scenario is about station routing,
                // not modifier pricing.
                modifiers: Vec::new(),
            },
            NewOrderItemRequest {
                menu_item_id: "item-unrouted".to_string(),
                variant_id: None,
                quantity: 1,
                unit_price_paise: 8000,
                notes: None,
                modifiers: Vec::new(),
            },
        ],
    )
    .expect("create order");

    holler_pos_lib::commands::orders::confirm_order_impl(&state, &order.holler_order_id)
        .expect("confirm order");

    let err = send_order_to_kitchen_impl(&state, &order.holler_order_id)
        .expect_err("mixed order with an unrouted line must be rejected");
    assert_eq!(err.code, "UNROUTED_KITCHEN_ITEMS");
    assert!(
        err.message.contains("Mystery Side"),
        "message must name the unrouted item, got: {}",
        err.message
    );

    let listed =
        list_kots_for_order_impl(&state, &order.holler_order_id).expect("list kots for order");
    assert!(
        listed.is_empty(),
        "neither line — including the routed one — may be ticketed on a rejected send"
    );
}

#[test]
fn kot_status_transitions_through_the_state_machine_and_rejects_illegal_moves() {
    let state = seeded_kitchen_state();
    let order = create_order_impl(
        &state,
        "DINE_IN".to_string(),
        None,
        vec![NewOrderItemRequest {
            menu_item_id: "item-1".to_string(),
            variant_id: None,
            quantity: 1,
            unit_price_paise: 25000,
            notes: None,
            modifiers: vec![],
        }],
    )
    .expect("create order");
    holler_pos_lib::commands::orders::confirm_order_impl(&state, &order.holler_order_id)
        .expect("confirm order");
    let kots = send_order_to_kitchen_impl(&state, &order.holler_order_id).expect("send");
    let kot_id = kots[0].id.clone();

    let after_ack = transition_kot_status_impl(
        &state,
        &order.holler_order_id,
        &kot_id,
        "ACKNOWLEDGED",
    )
    .expect("NEW -> ACKNOWLEDGED");
    assert_eq!(after_ack[0].status, "ACKNOWLEDGED");

    let err = transition_kot_status_impl(&state, &order.holler_order_id, &kot_id, "SERVED")
        .expect_err("ACKNOWLEDGED -> SERVED is illegal (must pass through PREPARING/READY)");
    assert_eq!(err.code, "ILLEGAL_KOT_STATUS_TRANSITION");
}

// ------------------------------------------------------- T12: LAN notify --

#[test]
fn send_to_kitchen_notifies_the_hub_with_an_upserted_kot() {
    let hub = std::sync::Arc::new(Hub::new());
    let subscription = hub.subscribe(OUTLET_ID, None);
    let state = seeded_kitchen_state_with_hub(hub);

    let order = create_order_impl(
        &state,
        "DINE_IN".to_string(),
        None,
        vec![NewOrderItemRequest {
            menu_item_id: "item-1".to_string(),
            variant_id: None,
            quantity: 1,
            unit_price_paise: 25000,
            notes: None,
            modifiers: vec![],
        }],
    )
    .expect("create order");
    holler_pos_lib::commands::orders::confirm_order_impl(&state, &order.holler_order_id)
        .expect("confirm order");
    let kots = send_order_to_kitchen_impl(&state, &order.holler_order_id).expect("send");

    let message = subscription
        .receiver
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("hub must publish a message after send-to-kitchen");
    match message {
        KdsLanMessage::KotUpserted { kot, outlet_id, .. } => {
            assert_eq!(kot.id, kots[0].id);
            assert_eq!(outlet_id, OUTLET_ID);
        }
        other => panic!("expected kot_upserted, got {other:?}"),
    }
}

#[test]
fn transition_kot_status_notifies_upserted_then_removed_on_terminal_status() {
    let hub = std::sync::Arc::new(Hub::new());
    let subscription = hub.subscribe(OUTLET_ID, None);
    let state = seeded_kitchen_state_with_hub(hub);

    let order = create_order_impl(
        &state,
        "DINE_IN".to_string(),
        None,
        vec![NewOrderItemRequest {
            menu_item_id: "item-1".to_string(),
            variant_id: None,
            quantity: 1,
            unit_price_paise: 25000,
            notes: None,
            modifiers: vec![],
        }],
    )
    .expect("create order");
    holler_pos_lib::commands::orders::confirm_order_impl(&state, &order.holler_order_id)
        .expect("confirm order");
    let kots = send_order_to_kitchen_impl(&state, &order.holler_order_id).expect("send");
    let kot_id = kots[0].id.clone();

    // Drain the kot_upserted from send-to-kitchen itself.
    subscription
        .receiver
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("send-to-kitchen notification");

    // NEW -> ACKNOWLEDGED -> PREPARING -> READY -> SERVED: a POS-driven walk
    // through the full legal state machine, mirroring
    // kot_status_transitions_through_the_state_machine_and_rejects_illegal_moves.
    for status in ["ACKNOWLEDGED", "PREPARING", "READY"] {
        transition_kot_status_impl(&state, &order.holler_order_id, &kot_id, status)
            .unwrap_or_else(|e| panic!("transition to {status} failed: {e:?}"));
        let message = subscription
            .receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap_or_else(|_| panic!("expected a hub notification for {status}"));
        match message {
            KdsLanMessage::KotUpserted { kot, .. } => assert_eq!(kot.status.as_db_str(), status),
            other => panic!("expected kot_upserted for {status}, got {other:?}"),
        }
    }

    transition_kot_status_impl(&state, &order.holler_order_id, &kot_id, "SERVED")
        .expect("READY -> SERVED");
    let message = subscription
        .receiver
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("expected a hub notification for SERVED");
    match message {
        KdsLanMessage::KotRemoved {
            kot_id: removed_id, ..
        } => assert_eq!(removed_id, kot_id, "a terminal status must announce kot_removed, not kot_upserted"),
        other => panic!("expected kot_removed for SERVED, got {other:?}"),
    }
}

#[test]
fn stations_are_readable_for_the_outlet() {
    let state = seeded_kitchen_state();
    let stations = list_stations_impl(&state).expect("list stations");
    assert_eq!(stations.len(), 1);
    assert_eq!(stations[0].code, "MAIN_KITCHEN");
}

/// The staff-visible failure view (docs/spec/hardware-printing.md): the
/// seeded printer's address is unreachable, so send-to-kitchen's own
/// best-effort print attempt fails and the job must show up here — a
/// silently swallowed print failure is exactly the bug this proves absent.
#[test]
fn a_print_failure_is_visible_to_staff_after_send_to_kitchen() {
    let state = seeded_kitchen_state();
    let order = create_order_impl(
        &state,
        "DINE_IN".to_string(),
        None,
        vec![NewOrderItemRequest {
            menu_item_id: "item-1".to_string(),
            variant_id: None,
            quantity: 1,
            unit_price_paise: 25000,
            notes: None,
            modifiers: vec![],
        }],
    )
    .expect("create order");
    holler_pos_lib::commands::orders::confirm_order_impl(&state, &order.holler_order_id)
        .expect("confirm order");
    send_order_to_kitchen_impl(&state, &order.holler_order_id).expect("send to kitchen");

    let failed = list_failed_print_jobs_impl(&state).expect("list failed print jobs");
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].printer_name, "Kitchen Printer");
    assert_eq!(failed[0].kot_station, "MAIN_KITCHEN");
    assert!(failed[0].last_error.is_some());
}

// ------------------------------------------------- Milestone 3 Track B --
// Quantity control, modifier attachment and #132-A post-DRAFT item addition
// (docs/m3-planning.md Track B, docs/backlog-m2.md P1 "No quantity control
// on a cart line"). The purpose stated in the task brief: the money
// invariant must be able to see real quantities AND real modifier price
// deltas, reachable through the exact commands the frontend will call.

/// The exact defect from the wild: five taps of one item must become one
/// line of quantity 5 through the Tauri command surface, not five separate
/// lines of quantity 1.
#[test]
fn update_order_item_quantity_through_the_tauri_command_surface() {
    let state = seeded_state();
    let order = create_order_impl(
        &state,
        "DINE_IN".to_string(),
        None,
        vec![NewOrderItemRequest {
            menu_item_id: "item-1".to_string(),
            variant_id: None,
            quantity: 1,
            unit_price_paise: 25000,
            notes: None,
            modifiers: vec![],
        }],
    )
    .expect("create order");
    let item_id = order.items[0].id.clone();

    let updated = update_order_item_quantity_impl(&state, &order.holler_order_id, &item_id, 5)
        .expect("quantity update must succeed");

    assert_eq!(updated.items.len(), 1, "still one line, never five");
    assert_eq!(updated.items[0].quantity, 5);
    assert_eq!(updated.items[0].line_total_paise, 125000);
    assert_eq!(updated.subtotal_paise, 125000);
    assert_eq!(updated.total_paise, 125000);
}

/// Zero/negative quantity is rejected at the command boundary with the same
/// `INVALID_QUANTITY` code `add_order_item`/`create_order` already use.
#[test]
fn update_order_item_quantity_rejects_non_positive_quantity() {
    let state = seeded_state();
    let order = create_order_impl(
        &state,
        "DINE_IN".to_string(),
        None,
        vec![NewOrderItemRequest {
            menu_item_id: "item-1".to_string(),
            variant_id: None,
            quantity: 1,
            unit_price_paise: 25000,
            notes: None,
            modifiers: vec![],
        }],
    )
    .expect("create order");
    let item_id = order.items[0].id.clone();

    let err = update_order_item_quantity_impl(&state, &order.holler_order_id, &item_id, 0)
        .expect_err("zero quantity must be rejected");
    assert_eq!(err.code, "INVALID_QUANTITY");
}

/// `#132-A`: a quantity change must succeed even after the order has left
/// DRAFT (here, CONFIRMED) — matching `add_order_item`'s widened gate.
#[test]
fn update_order_item_quantity_succeeds_after_confirmation_132a() {
    let state = seeded_state();
    let order = create_order_impl(
        &state,
        "DINE_IN".to_string(),
        None,
        vec![NewOrderItemRequest {
            menu_item_id: "item-1".to_string(),
            variant_id: None,
            quantity: 1,
            unit_price_paise: 25000,
            notes: None,
            modifiers: vec![],
        }],
    )
    .expect("create order");
    let item_id = order.items[0].id.clone();

    holler_pos_lib::commands::orders::confirm_order_impl(&state, &order.holler_order_id)
        .expect("confirm order");

    let updated = update_order_item_quantity_impl(&state, &order.holler_order_id, &item_id, 3)
        .expect("quantity change must succeed on a CONFIRMED order (#132-A)");
    assert_eq!(updated.items[0].quantity, 3);
}

/// THE DEFECT the orchestrator's verifier found, exercised through the
/// actual Tauri command boundary: a quantity change on a line the kitchen
/// already has a ticket for must be rejected with an actionable error, not
/// silently applied while the kitchen's copy goes stale. `#132-A` widened
/// `update_order_item_quantity` to work post-DRAFT, but only for lines no
/// `kot` has frozen a snapshot of yet — this is the line that already has
/// one.
#[test]
fn update_order_item_quantity_rejects_an_already_ticketed_line() {
    let state = seeded_kitchen_state();
    let order = create_order_impl(
        &state,
        "DINE_IN".to_string(),
        None,
        vec![NewOrderItemRequest {
            menu_item_id: "item-1".to_string(),
            variant_id: None,
            quantity: 1,
            unit_price_paise: 25000,
            notes: None,
            modifiers: vec![],
        }],
    )
    .expect("create order");
    let item_id = order.items[0].id.clone();

    holler_pos_lib::commands::orders::confirm_order_impl(&state, &order.holler_order_id)
        .expect("confirm order");
    send_order_to_kitchen_impl(&state, &order.holler_order_id).expect("send to kitchen tickets it");

    let err = update_order_item_quantity_impl(&state, &order.holler_order_id, &item_id, 5)
        .expect_err("quantity change on an already-ticketed line must be rejected");
    assert_eq!(err.code, "ORDER_ITEM_ALREADY_TICKETED");
    assert!(
        err.message.contains("cancel"),
        "the error must name the sanctioned alternative (#132-C cancel + re-add), got: {}",
        err.message
    );

    // Nothing changed — the rejected call must not have written anything.
    let fetched = get_order_impl(&state, &order.holler_order_id)
        .expect("get_order")
        .expect("order exists");
    assert_eq!(fetched.items[0].quantity, 1);
}

/// THE CRASH TEST for quantity control, mirroring
/// `a_draft_order_survives_the_pos_process_ending_and_a_fresh_one_reopening`:
/// a quantity change written through as the cashier taps must survive the
/// POS process ending with no graceful shutdown and a completely fresh
/// process reopening the same encrypted file. Nothing here asserts that
/// `update_order_item_quantity` was *called*; it drops the whole `AppState`/
/// `Db` (no shared connection, no in-memory-only handle) and asserts what a
/// fresh read of SQLite alone produces.
#[test]
fn a_quantity_change_survives_the_pos_process_ending_and_a_fresh_one_reopening() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sealed = dir.path().join("edge.db.enc");
    let plaintext = dir.path().join("edge.db");

    let order_id: String;
    let item_id: String;

    // ---- "session 1": the cashier adds a line, then bumps its quantity. ----
    {
        let db = Db::open(&sealed, &plaintext, EncryptionKey::new([17u8; 32])).expect("open db");
        seed(&db);
        let state = AppState::new(db, OUTLET_ID.to_string(), DEVICE_ID.to_string());

        let order = create_order_impl(
            &state,
            "DINE_IN".to_string(),
            None,
            vec![NewOrderItemRequest {
                menu_item_id: "item-1".to_string(),
                variant_id: None,
                quantity: 1,
                unit_price_paise: 25000,
                notes: None,
                modifiers: vec![],
            }],
        )
        .expect("create order");
        order_id = order.holler_order_id.clone();
        item_id = order.items[0].id.clone();

        update_order_item_quantity_impl(&state, &order_id, &item_id, 5)
            .expect("quantity update persists");

        // Process ends here with no graceful shutdown — models a crash right
        // after the quantity tap.
    }

    // ---- "session 2": a fresh process, fresh AppState, same file. ----
    let db2 = Db::open(&sealed, &plaintext, EncryptionKey::new([17u8; 32]))
        .expect("reopen must succeed");
    let state2 = AppState::new(db2, OUTLET_ID.to_string(), DEVICE_ID.to_string());

    let recovered = get_active_draft_order_impl(&state2)
        .expect("recovery read must succeed")
        .expect("draft order must be recoverable");
    assert_eq!(recovered.holler_order_id, order_id);
    let recovered_item = recovered
        .items
        .iter()
        .find(|i| i.id == item_id)
        .expect("line recoverable");
    assert_eq!(
        recovered_item.quantity, 5,
        "the quantity change must have survived the crash, not just the return value"
    );
    assert_eq!(recovered_item.line_total_paise, 125000);
    assert_eq!(recovered.subtotal_paise, 125000);
}

/// Modifier attachment end to end: a modifier chosen on the very first tap
/// (the line that creates the order) must have its `price_delta_paise` land
/// in the line total and the order's own subtotal/total — reachable purely
/// through `create_order`, not just in principle inside `edge/database`.
#[test]
fn create_order_persists_a_modifier_and_its_price_delta_reaches_order_totals() {
    let state = seeded_state();

    let order = create_order_impl(
        &state,
        "DINE_IN".to_string(),
        None,
        vec![NewOrderItemRequest {
            menu_item_id: "item-1".to_string(),
            variant_id: None,
            quantity: 2,
            unit_price_paise: 25000,
            notes: None,
            modifiers: vec![NewOrderItemModifierRequest {
                modifier_id: "modifier-1".to_string(),
                group_name: "Size".to_string(),
                option_name: "Large".to_string(),
                price_delta_paise: 5000,
            }],
        }],
    )
    .expect("create order with a modifier");

    // (unit_price 25000 + modifier delta 5000) * quantity 2 = 60000.
    assert_eq!(order.items[0].modifiers.len(), 1);
    assert_eq!(order.items[0].modifiers[0].price_delta_paise, 5000);
    assert_eq!(order.items[0].line_total_paise, 60000);
    assert_eq!(order.subtotal_paise, 60000);
    assert_eq!(order.total_paise, 60000);

    // Round-trips through storage — the modifier is not just present on the
    // return value of the write, it survives a fresh read.
    let fetched = get_order_impl(&state, &order.holler_order_id)
        .expect("get_order")
        .expect("order exists");
    assert_eq!(fetched.items[0].modifiers.len(), 1);
    assert_eq!(fetched.items[0].modifiers[0].option_name, "Large");
    assert_eq!(fetched.items[0].line_total_paise, 60000);
}

/// Modifier attachment via `add_order_item` — the second-tap path, and
/// (`#132-A`) legal even after the order has left DRAFT.
#[test]
fn add_order_item_persists_a_modifier_after_confirmation_132a() {
    let state = seeded_state();
    let order = create_order_impl(
        &state,
        "DINE_IN".to_string(),
        None,
        vec![NewOrderItemRequest {
            menu_item_id: "item-1".to_string(),
            variant_id: None,
            quantity: 1,
            unit_price_paise: 25000,
            notes: None,
            modifiers: vec![],
        }],
    )
    .expect("create order");

    holler_pos_lib::commands::orders::confirm_order_impl(&state, &order.holler_order_id)
        .expect("confirm order");

    let after_add = add_order_item_impl(
        &state,
        &order.holler_order_id,
        NewOrderItemRequest {
            menu_item_id: "item-1".to_string(),
            variant_id: None,
            quantity: 1,
            unit_price_paise: 25000,
            notes: None,
            modifiers: vec![NewOrderItemModifierRequest {
                modifier_id: "modifier-2".to_string(),
                group_name: "Spice".to_string(),
                option_name: "Extra Hot".to_string(),
                price_delta_paise: 1000,
            }],
        },
    )
    .expect("add-item with a modifier must succeed post-confirmation (#132-A)");

    let added_line = after_add
        .items
        .iter()
        .find(|i| !i.modifiers.is_empty())
        .expect("the added line carries its modifier");
    assert_eq!(added_line.modifiers[0].price_delta_paise, 1000);
    assert_eq!(added_line.line_total_paise, 26000);
    assert_eq!(after_add.subtotal_paise, 25000 + 26000);
}

/// Composition test (CLAUDE.md: "Test compositions, not just each operation
/// alone" — the exact shape of the M2 order-type-lock regression): a line
/// added after send-to-kitchen (`#132-A`), with a modifier, then a quantity
/// change on that same line, then send-to-kitchen again — every step must
/// leave the money invariant correct, and the addition must reach the
/// kitchen as a fresh ticket (idempotent-by-delta), not silently disappear.
#[test]
fn addition_modifier_and_quantity_change_compose_correctly_after_send_to_kitchen() {
    let state = seeded_kitchen_state();
    let order = create_order_impl(
        &state,
        "DINE_IN".to_string(),
        Some("table-1".to_string()),
        vec![NewOrderItemRequest {
            menu_item_id: "item-1".to_string(),
            variant_id: None,
            quantity: 1,
            unit_price_paise: 25000,
            notes: None,
            modifiers: vec![],
        }],
    )
    .expect("create order");

    holler_pos_lib::commands::orders::confirm_order_impl(&state, &order.holler_order_id)
        .expect("confirm order");
    let first_kots =
        send_order_to_kitchen_impl(&state, &order.holler_order_id).expect("first send");
    assert_eq!(first_kots.len(), 1);

    // #132-A: add a second line, with a modifier, after the kitchen already
    // has a ticket.
    let after_add = add_order_item_impl(
        &state,
        &order.holler_order_id,
        NewOrderItemRequest {
            menu_item_id: "item-1".to_string(),
            variant_id: None,
            quantity: 2,
            unit_price_paise: 25000,
            notes: None,
            modifiers: vec![NewOrderItemModifierRequest {
                modifier_id: "modifier-3".to_string(),
                group_name: "Spice".to_string(),
                option_name: "Extra Hot".to_string(),
                price_delta_paise: 1000,
            }],
        },
    )
    .expect("post-send addition must succeed (#132-A)");
    let added_item_id = after_add
        .items
        .iter()
        .find(|i| !i.modifiers.is_empty())
        .expect("added line present")
        .id
        .clone();

    // (25000 + 1000) * 2 = 52000.
    assert_eq!(after_add.subtotal_paise, 25000 + 52000);

    // Change the new line's quantity — still legal post-send.
    let after_qty =
        update_order_item_quantity_impl(&state, &order.holler_order_id, &added_item_id, 3)
            .expect("quantity change must succeed post-send (#132-A)");
    let resized_line = after_qty
        .items
        .iter()
        .find(|i| i.id == added_item_id)
        .expect("resized line present");
    // (25000 + 1000) * 3 = 78000.
    assert_eq!(resized_line.line_total_paise, 78000);
    assert_eq!(after_qty.subtotal_paise, 25000 + 78000);

    // Sending again must ticket only the addition, as a fresh #132-A-style
    // ticket, not silently drop it and not mutate the first ticket.
    let second_kots =
        send_order_to_kitchen_impl(&state, &order.holler_order_id).expect("second send");
    assert_eq!(second_kots.len(), 1, "the addition produces one new ticket");
    assert_ne!(second_kots[0].id, first_kots[0].id);
    assert_eq!(second_kots[0].items[0].quantity, 3);

    let all_kots = list_kots_for_order_impl(&state, &order.holler_order_id).expect("list kots");
    assert_eq!(all_kots.len(), 2, "both tickets must be visible to the kitchen");
}
