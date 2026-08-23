//! M4 acceptance criterion 1, against the REAL seed rather than fixtures.
//!
//! > Sell a dish **from the real seed menu** with the network disconnected →
//! > `stock_ledger_entry` rows for every ingredient at recipe quantity × line
//! > quantity, plus the deltas for modifiers actually chosen, and nothing for
//! > modifiers with no delta row.
//!
//! # Why the seed and not a fixture
//!
//! A fixture is written by the same person as the assertion, so it agrees
//! with it by construction: recipes exist because the test needed them to,
//! quantities are round because round numbers were convenient. The seed is
//! the menu an outlet actually gets, and it is the thing that has to work.
//! The 32 real items exist so this test could not quietly be run against
//! something easier.
//!
//! # Offline
//!
//! No network is disconnected here because there is none to disconnect: this
//! crate opens a local SQLite file and never opens a socket. That is the
//! architectural claim ADR-013 makes — a sale completes with the uplink down
//! because the uplink was never on the path — and a test that adds a network
//! only to take it away would be testing a fiction. The uplink half is proved
//! separately, by `edge/sync`'s offline pump tests.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use holler_edge_database::crypto::EncryptionKey;
use holler_edge_database::model::{
    NewOrder, NewOrderItem, NewOutboxEntry, OrderConfirmedMeta, OrderItemModifier,
};
use holler_edge_database::Db;

const KEY_HEX: &str = "1a2b3c4d5e6f70819293a4b5c6d7e8f91a2b3c4d5e6f70819293a4b5c6d7e8f9";
const ORDER_ID: &str = "0191b000-0000-7000-8000-0000000000d1";
const ORDER_ITEM_ID: &str = "0191b000-0000-7000-8000-0000000000d2";
const LINE_QUANTITY: i64 = 3;

fn key() -> EncryptionKey {
    let mut bytes = [0u8; 32];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = u8::from_str_radix(&KEY_HEX[i * 2..i * 2 + 2], 16).expect("hex key");
    }
    EncryptionKey::new(bytes)
}

fn devseed(dir: &Path) {
    let out = Command::new(env!("CARGO_BIN_EXE_devseed"))
        .env("HOLLER_EDGE_DATA_DIR", dir)
        .env("HOLLER_DB_KEY_HEX", KEY_HEX)
        .env(
            "HOLLER_SEED_PASSWORD_HASH",
            "$argon2id$v=19$m=65536,t=3,p=4$c2FsdHNhbHRzYWx0$0000000000000000000000000000000000000000000",
        )
        .output()
        .expect("run devseed");
    assert!(
        out.status.success(),
        "devseed failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// What the seed says this sellable unit consumes: `inventory_item_id →
/// quantity_micro`, read from the recipe rather than restated in the test.
/// Restating it would make the assertion agree with itself.
///
/// SUMMED per item, because a recipe may legitimately name the same
/// inventory item on more than one line, and the ledger posts what the
/// resolver totalled.
fn recipe_ingredients(db: &Db, variant_id: &str) -> BTreeMap<String, i64> {
    let mut stmt = db
        .connection()
        .prepare(
            "SELECT ri.inventory_item_id, ri.quantity_micro
             FROM recipe r
             JOIN recipe_ingredient ri ON ri.recipe_id = r.id
             WHERE r.menu_item_variant_id = ?1
               AND ri.inventory_item_id IS NOT NULL",
        )
        .expect("prepare");
    let mut rows: BTreeMap<String, i64> = BTreeMap::new();
    let mapped = stmt
        .query_map([variant_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })
        .expect("query");
    for row in mapped {
        let (item_id, qty) = row.expect("row");
        *rows.entry(item_id).or_insert(0) += qty;
    }
    assert!(
        !rows.is_empty(),
        "the chosen seeded variant must have ingredients, or this test asserts nothing"
    );
    rows
}

/// A sellable unit from the seed whose recipe resolves to plain inventory
/// items, together with one modifier that HAS a delta row and one that does
/// NOT. Both halves of the criterion need a real example of each, and both
/// are found in the seed rather than assumed.
struct Subject {
    menu_item_id: String,
    variant_id: String,
    modifier_with_delta: String,
    modifier_without_delta: String,
}

fn find_subject(db: &Db) -> Subject {
    let conn = db.connection();
    // A menu item that has: a variant with a FLAT recipe (no sub-recipe
    // components), at least one modifier carrying a delta, and at least one
    // modifier carrying none.
    //
    // WHY FLAT. A sub-recipe's contribution is a scaled rational resolved at
    // the leaf, so predicting it here would mean reimplementing the resolver
    // inside its own test — the assertion would then agree with the code by
    // construction and could not catch it being wrong. Sub-recipe scaling has
    // its own unit tests, with hand-computed expectations, in
    // `edge/database/src/deduction`. This test is about the criterion's claim:
    // every ingredient, at recipe quantity × line quantity.
    //
    // The first attempt at this test skipped that distinction and picked a
    // variant with a sub-recipe; it failed with -150000 against an expected
    // -60000, which is the resolver being right and the test being wrong.
    let (menu_item_id, variant_id): (String, String) = conn
        .query_row(
            "SELECT v.menu_item_id, v.id
             FROM recipe r
             JOIN menu_item_variant v ON v.id = r.menu_item_variant_id
             WHERE NOT EXISTS (SELECT 1 FROM recipe_ingredient ri
                               WHERE ri.recipe_id = r.id AND ri.sub_recipe_id IS NOT NULL)
               AND EXISTS (SELECT 1 FROM menu_item_modifier m
                           JOIN modifier_ingredient_delta d ON d.menu_item_modifier_id = m.id
                           WHERE m.menu_item_id = v.menu_item_id)
               AND EXISTS (SELECT 1 FROM menu_item_modifier m
                           WHERE m.menu_item_id = v.menu_item_id
                             AND NOT EXISTS (SELECT 1 FROM modifier_ingredient_delta d
                                             WHERE d.menu_item_modifier_id = m.id))
             ORDER BY v.id LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect(
            "the seed must contain an item with a recipe, a modifier WITH an ingredient delta \
             and a modifier WITHOUT one — criterion 1 names all three",
        );

    let modifier_with_delta: String = conn
        .query_row(
            "SELECT m.id FROM menu_item_modifier m
             JOIN modifier_ingredient_delta d ON d.menu_item_modifier_id = m.id
             WHERE m.menu_item_id = ?1 ORDER BY m.id LIMIT 1",
            [&menu_item_id],
            |r| r.get(0),
        )
        .expect("modifier with a delta");

    let modifier_without_delta: String = conn
        .query_row(
            "SELECT m.id FROM menu_item_modifier m
             WHERE m.menu_item_id = ?1
               AND NOT EXISTS (SELECT 1 FROM modifier_ingredient_delta d
                               WHERE d.menu_item_modifier_id = m.id)
             ORDER BY m.id LIMIT 1",
            [&menu_item_id],
            |r| r.get(0),
        )
        .expect("modifier without a delta");

    Subject {
        menu_item_id,
        variant_id,
        modifier_with_delta,
        modifier_without_delta,
    }
}

fn scalar(db: &Db, sql: &str) -> String {
    db.connection()
        .query_row(sql, [], |r| r.get(0))
        .expect("scalar query")
}

#[test]
fn a_sale_from_the_real_seed_menu_deducts_every_ingredient_and_only_chosen_deltas() {
    let dir = tempfile::tempdir().expect("tempdir");
    devseed(dir.path());

    let mut db = Db::open(
        &dir.path().join("edge.db.enc"),
        &dir.path().join("edge.db"),
        key(),
    )
    .expect("open the seeded database");

    let subject = find_subject(&db);
    let expected = recipe_ingredients(&db, &subject.variant_id);
    let outlet_id = scalar(&db, "SELECT id FROM outlet LIMIT 1");
    let device_id = scalar(&db, "SELECT id FROM device LIMIT 1");

    let order = NewOrder {
        id: ORDER_ID.to_string(),
        outlet_id: outlet_id.clone(),
        device_id,
        order_type: "DINE_IN".to_string(),
        status: "DRAFT".to_string(),
        table_id: None,
        subtotal_paise: 45_000,
        discount_paise: 0,
        taxes_paise: 0,
        total_paise: 45_000,
        source: "POS".to_string(),
        external_order_id: None,
        payment_status: "UNPAID".to_string(),
        payment_source: None,
        confirmed_at: None,
        source_payload_json: None,
        schema_version: 1,
        created_at: "2026-08-23T12:00:00Z".to_string(),
        updated_at: "2026-08-23T12:00:00Z".to_string(),
    };
    let item = NewOrderItem {
        id: ORDER_ITEM_ID.to_string(),
        order_id: ORDER_ID.to_string(),
        menu_item_id: subject.menu_item_id.clone(),
        variant_id: Some(subject.variant_id.clone()),
        quantity: LINE_QUANTITY,
        unit_price_paise: 15_000,
        line_total_paise: 45_000,
        notes: None,
        created_at: "2026-08-23T12:00:00Z".to_string(),
    };

    // BOTH modifiers are chosen. The one with a delta must move stock; the
    // one without must move none — an absent delta row is not "assume zero
    // and post it anyway", it is nothing to post.
    let modifiers = vec![
        OrderItemModifier {
            id: "0191b000-0000-7000-8000-0000000000d3".to_string(),
            order_item_id: ORDER_ITEM_ID.to_string(),
            modifier_id: subject.modifier_with_delta.clone(),
            group_name: "Options".to_string(),
            option_name: "With delta".to_string(),
            price_delta_paise: 0,
            created_at: "2026-08-23T12:00:00Z".to_string(),
        },
        OrderItemModifier {
            id: "0191b000-0000-7000-8000-0000000000d4".to_string(),
            order_item_id: ORDER_ITEM_ID.to_string(),
            modifier_id: subject.modifier_without_delta.clone(),
            group_name: "Options".to_string(),
            option_name: "No delta".to_string(),
            price_delta_paise: 0,
            created_at: "2026-08-23T12:00:00Z".to_string(),
        },
    ];

    db.create_order_with_outbox_and_modifiers(
        &order,
        &[item],
        &[modifiers],
        &NewOutboxEntry {
            id: "outbox-seed-sale".to_string(),
            aggregate_type: "order".to_string(),
            aggregate_id: ORDER_ID.to_string(),
            event_type: "OrderCreated".to_string(),
            payload_json: "{}".to_string(),
            created_at: "2026-08-23T12:00:00Z".to_string(),
        },
    )
    .expect("create the draft order");

    db.confirm_order_with_outbox(
        ORDER_ID,
        &OrderConfirmedMeta {
            outbox_id: "outbox-seed-confirm".to_string(),
            occurred_at: "2026-08-23T12:05:00Z".to_string(),
            confirmed_at: "2026-08-23T12:05:00Z".to_string(),
        },
    )
    .expect("confirm the sale");

    // --- what actually landed on the ledger -------------------------------

    let rows: Vec<(String, String, Option<String>, i64)> = {
        let mut stmt = db
            .connection()
            .prepare(
                "SELECT inventory_item_id, origin, modifier_delta_id, quantity_applied_micro
                 FROM stock_ledger_entry WHERE source_order_id = ?1",
            )
            .expect("prepare");
        let collected = stmt
            .query_map([ORDER_ID], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .expect("query")
            .collect::<Result<_, _>>()
            .expect("collect");
        collected
    };

    // Every recipe ingredient, at recipe quantity × line quantity.
    for (item_id, per_unit_micro) in &expected {
        let posted: i64 = rows
            .iter()
            .filter(|(id, origin, _, _)| id == item_id && origin == "RECIPE")
            .map(|(_, _, _, qty)| *qty)
            .sum();
        assert_eq!(
            posted,
            -(per_unit_micro * LINE_QUANTITY),
            "ingredient {item_id} must be deducted at recipe quantity × line quantity"
        );
    }

    // The chosen modifier that HAS a delta moved stock.
    let delta_rows: Vec<_> = rows
        .iter()
        .filter(|(_, origin, _, _)| origin == "MODIFIER_DELTA")
        .collect();
    assert!(
        !delta_rows.is_empty(),
        "the chosen modifier carries a delta row, so it must have moved stock"
    );

    // And nothing anywhere is attributable to the modifier that has none.
    // Checked through the ledger's own provenance column rather than by
    // counting rows: a count would pass just as well if the wrong modifier
    // had been posted.
    let from_absent_modifier: i64 = db
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM stock_ledger_entry sle
             JOIN modifier_ingredient_delta d ON d.id = sle.modifier_delta_id
             WHERE sle.source_order_id = ?1 AND d.menu_item_modifier_id = ?2",
            [ORDER_ID, subject.modifier_without_delta.as_str()],
            |r| r.get(0),
        )
        .expect("query");
    assert_eq!(
        from_absent_modifier, 0,
        "a modifier with no delta row must post nothing — an absent row is not an implied zero"
    );

    db.close().expect("seal");
}
