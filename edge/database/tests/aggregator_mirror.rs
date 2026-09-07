//! M6 Phase C, C1: the edge's `aggregator_order` mirror.
//!
//! THE PROPERTY MOST AT RISK HERE IS NOT CORRECTNESS, IT IS AUTHORITY. The
//! table is cloud-authoritative (ADR-022): the edge receives documents and
//! never authors one. That is easy to state and easy to erode — the first time
//! somebody needs to "just update the status locally", split authority arrives
//! and nothing fails.
//!
//! So these tests pin the three properties that would break silently:
//! idempotency by id, replace-not-merge on version, and the fact that a later
//! cloud document CANNOT clear a local acceptance.

use holler_edge_database::{model, repo, Db};

fn seeded_db() -> Db {
    let db = Db::open_in_memory_for_tests().expect("in-memory database");
    db.connection()
        .execute_batch(
            "INSERT INTO outlet (id, brand_id, name, timezone, config_version, created_at, updated_at)
             VALUES ('outlet-1', 'brand-1', 'Test Outlet', 'Asia/Kolkata', 1, '2026-09-08T00:00:00Z', '2026-09-08T00:00:00Z');
             INSERT INTO sync_state (outlet_id, last_applied_config_version, is_online)
             VALUES ('outlet-1', 0, 0);
             -- A REAL menu_item, not a stub id. menu_item_id is a foreign key,
             -- so a synthesised uuid would make the mapped case fail for a
             -- reason that has nothing to do with what is being tested -- and
             -- the unmapped case would keep passing, quietly leaving the mapped
             -- path uncovered.
             INSERT INTO menu_category (id, outlet_id, name, sort_order, config_version)
             VALUES ('cat-1', 'outlet-1', 'Beverages', 1, 1);
             INSERT INTO menu_item (id, outlet_id, category_id, name, base_price_paise, is_available, config_version, hsn_sac)
             VALUES ('menu-1', 'outlet-1', 'cat-1', 'Masala Chai', 4500, 1, 1, '9963');
             -- A REAL order and device too: local_order_id is a foreign key,
             -- and the acceptance test is worthless if the row it points at
             -- cannot exist.
             INSERT INTO device (id, outlet_id, kind, name, created_at)
             VALUES ('device-1', 'outlet-1', 'POS', 'Till 1', '2026-09-08T00:00:00Z');
             INSERT INTO \"order\" (id, outlet_id, device_id, order_type, status, created_at, updated_at)
             VALUES ('order-9', 'outlet-1', 'device-1', 'DELIVERY', 'CONFIRMED', '2026-09-08T12:05:00Z', '2026-09-08T12:05:00Z');",
        )
        .expect("seeding");
    db
}

fn document(version: i64, status: &str) -> model::AggregatorOrder {
    model::AggregatorOrder {
        id: "doc-1".to_string(),
        tenant_id: "tenant-1".to_string(),
        outlet_id: "outlet-1".to_string(),
        platform: "test-platform".to_string(),
        external_order_id: "EXT-1".to_string(),
        platform_status: status.to_string(),
        document_version: version,
        raw_payload: "{}".to_string(),
        stated_total_paise: Some(13887),
        received_at: "2026-09-08T12:00:00Z".to_string(),
        business_date: "2026-09-08".to_string(),
        accepted_at: None,
        local_order_id: None,
        created_at: "2026-09-08T12:00:00Z".to_string(),
        updated_at: "2026-09-08T12:00:00Z".to_string(),
    }
}

fn line(id: &str, external: &str, menu_item: Option<&str>) -> model::AggregatorOrderLine {
    model::AggregatorOrderLine {
        id: id.to_string(),
        aggregator_order_id: "doc-1".to_string(),
        line_number: 1,
        external_item_id: external.to_string(),
        external_item_name: "Some Dish".to_string(),
        menu_item_id: menu_item.map(str::to_string),
        quantity: 2,
        stated_unit_price_paise: Some(4500),
    }
}

/// Re-applying the same document is a no-op, not a duplicate.
///
/// The pull is at-least-once by construction: a drain that dies after writing
/// and before advancing its cursor re-reads the same page. If that produced a
/// second document, every crash would double an outlet's delivery orders.
#[test]
fn applying_the_same_document_twice_leaves_one_row() {
    let db = seeded_db();
    let doc = document(1, "Accepted");
    let lines = vec![line("line-1", "I1", Some("menu-1"))];

    assert!(repo::apply_aggregator_order(db.connection(), &doc, &lines).expect("first apply"));
    // Same version: ignored outright, and that is the correct answer rather
    // than an error. A re-read of an unchanged page is ordinary.
    assert!(
        !repo::apply_aggregator_order(db.connection(), &doc, &lines).expect("second apply"),
        "an equal version must be ignored, not re-applied"
    );

    let count: i64 = db
        .connection()
        .query_row("SELECT count(*) FROM aggregator_order", [], |r| r.get(0))
        .expect("count");
    assert_eq!(count, 1, "re-applying a document must not create a second one");

    let lines_count: i64 = db
        .connection()
        .query_row("SELECT count(*) FROM aggregator_order_line", [], |r| r.get(0))
        .expect("count lines");
    assert_eq!(lines_count, 1, "lines must not accumulate across applies");
}

/// A NEWER document replaces; an OLDER one is ignored outright.
///
/// Replace-not-merge, the `GET /sync/config` rule applied to a document stream.
/// The older-is-ignored half matters more than it looks: platforms retry, and a
/// retry of an earlier state must not walk the document backwards — a
/// cancelled order reverting to "Accepted" would put it back in the kitchen.
#[test]
fn a_newer_document_replaces_and_an_older_one_is_ignored() {
    let db = seeded_db();
    let lines = vec![line("line-1", "I1", Some("menu-1"))];

    repo::apply_aggregator_order(db.connection(), &document(2, "Accepted"), &lines).expect("v2");
    assert!(
        repo::apply_aggregator_order(db.connection(), &document(3, "Cancelled"), &lines)
            .expect("v3"),
        "a newer version must replace"
    );
    assert!(
        !repo::apply_aggregator_order(db.connection(), &document(1, "Accepted"), &lines)
            .expect("v1"),
        "an older version must be ignored, or a platform retry walks the document backwards"
    );

    let status: String = db
        .connection()
        .query_row(
            "SELECT platform_status FROM aggregator_order WHERE id = 'doc-1'",
            [],
            |r| r.get(0),
        )
        .expect("status");
    assert_eq!(
        status, "Cancelled",
        "the newest version must stand; an older retry must not revert it"
    );
}

/// A LATER CLOUD DOCUMENT MUST NOT CLEAR A LOCAL ACCEPTANCE.
///
/// `accepted_at` and `local_order_id` record that a HUMAN AT THIS TILL accepted
/// the document and that an `order` now exists for it. A platform status update
/// arriving afterwards must not erase that: the local order is
/// edge-authoritative and already in a kitchen.
///
/// This is the single assertion standing between the mirror and split
/// authority, and it fails silently if the upsert's SET list ever grows.
#[test]
fn a_later_cloud_document_cannot_clear_a_local_acceptance() {
    let db = seeded_db();
    let lines = vec![line("line-1", "I1", Some("menu-1"))];
    repo::apply_aggregator_order(db.connection(), &document(1, "Accepted"), &lines).expect("v1");

    // The till accepts: this is the ONLY edge-side write to these columns, and
    // it writes local state, never the platform's.
    db.connection()
        .execute(
            "UPDATE aggregator_order SET accepted_at = '2026-09-08T12:05:00Z', local_order_id = 'order-9'
             WHERE id = 'doc-1'",
            [],
        )
        .expect("accept");

    // A newer platform document arrives.
    repo::apply_aggregator_order(db.connection(), &document(2, "In-progress"), &lines).expect("v2");

    let (accepted, local): (Option<String>, Option<String>) = db
        .connection()
        .query_row(
            "SELECT accepted_at, local_order_id FROM aggregator_order WHERE id = 'doc-1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("read back");

    assert_eq!(
        accepted.as_deref(),
        Some("2026-09-08T12:05:00Z"),
        "a cloud document cleared a local acceptance — the till has an order in a kitchen for a document that now says nobody accepted it"
    );
    assert_eq!(
        local.as_deref(),
        Some("order-9"),
        "a cloud document cleared the link to the local order"
    );
}

/// An unmapped line survives the round trip.
///
/// ADR-022 rule 4: a line nothing local matches is RECORDED, NOT REFUSED.
/// Refusing a delivery order that is already cooking is the outage, not the
/// protection — and the platform's own name for the dish is the only
/// description anyone has of what the customer ordered.
#[test]
fn an_unmapped_line_is_stored_with_its_platform_name() {
    let db = seeded_db();
    let lines = vec![line("line-1", "SKU-UNKNOWN", None)];
    repo::apply_aggregator_order(db.connection(), &document(1, "Accepted"), &lines).expect("apply");

    let (menu_item, name): (Option<String>, String) = db
        .connection()
        .query_row(
            "SELECT menu_item_id, external_item_name FROM aggregator_order_line",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("read line");

    assert!(menu_item.is_none(), "nothing local matched, so this must stay NULL");
    assert_eq!(name, "Some Dish", "the platform's own name must survive");
}

/// The cursor is absent until something is pulled, and round-trips after.
#[test]
fn the_pull_cursor_starts_absent_and_round_trips() {
    let db = seeded_db();
    assert!(
        repo::get_aggregator_pull_cursor(db.connection(), "outlet-1")
            .expect("read")
            .is_none(),
        "nothing pulled yet must read as absent, not as a sentinel"
    );

    repo::set_aggregator_pull_cursor(db.connection(), "outlet-1", "cursor-abc").expect("set");
    assert_eq!(
        repo::get_aggregator_pull_cursor(db.connection(), "outlet-1").expect("read back"),
        Some("cursor-abc".to_string())
    );
}
