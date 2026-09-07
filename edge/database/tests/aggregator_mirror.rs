//! M6 Phase C, C1: the edge's `aggregator_order` mirror.
//!
//! THE PROPERTY MOST AT RISK HERE IS NOT CORRECTNESS, IT IS AUTHORITY. The
//! table is cloud-authoritative (ADR-022): the edge receives documents and
//! never authors one. That is easy to state and easy to erode — the first time
//! somebody needs to "just update the status locally", split authority arrives
//! and nothing fails.
//!
//! So these tests pin the properties that would break silently: idempotency by
//! id, replace-not-merge on version, and — the important one —
//! **`aggregator_order` CARRIES NO EDGE-WRITTEN COLUMN AT ALL**.
//!
//! An earlier version of this table had two, `accepted_at` and
//! `local_order_id`, guarded by omitting them from the upsert's SET list so a
//! cloud document could not clear them. That guard worked and was tested. It
//! was still split authority on a cloud-authoritative aggregate, and the
//! contract rubric says to split the aggregate rather than guard the column —
//! so acceptance moved off the table and is DERIVED from the local order that
//! acceptance creates. The test below is retargeted at that shape: it fails if
//! anything reintroduces an edge-written column here.

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
             -- A device, so a test can create an order. NO ORDER IS SEEDED:
             -- acceptance IS the existence of one, so pre-seeding it would
             -- pre-satisfy the very thing the derivation test measures.
             INSERT INTO device (id, outlet_id, kind, name, created_at)
             VALUES ('device-1', 'outlet-1', 'POS', 'Till 1', '2026-09-08T00:00:00Z');",
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

/// THE MIRROR CARRIES NO ACCEPTANCE STATE, AND ACCEPTANCE IS STILL DERIVABLE.
///
/// This replaces an earlier test that asserted a cloud document could not clear
/// `accepted_at`. That test passed, and the shape it protected was still wrong:
/// two edge-written columns on a cloud-authoritative table are split authority
/// however carefully the upsert is maintained, and they grow — `printed_at` and
/// `closed_at` were the obvious next two.
///
/// The property now asserted is stronger and needs no guard to stay true:
///
///   1. `aggregator_order` HAS no acceptance columns, so nothing can clear them
///      and nothing can drift.
///   2. Acceptance is derived from the local `order` — the row whose existence
///      IS the acceptance — and survives any number of later cloud documents,
///      because a cloud document cannot touch an `order` at all.
///
/// If a future change reintroduces an edge-written column on the mirror, part
/// one fails on the schema.
#[test]
fn the_mirror_has_no_acceptance_columns_and_acceptance_is_derived() {
    let db = seeded_db();
    let lines = vec![line("line-1", "I1", Some("menu-1"))];
    repo::apply_aggregator_order(db.connection(), &document(1, "Accepted"), &lines).expect("v1");

    // 1. THE SCHEMA ITSELF. Asserted against the table, not against a struct:
    // a struct can drop a field while the column lingers, and the column is
    // what a future writer would reach for.
    let columns: Vec<String> = {
        let mut stmt = db
            .connection()
            .prepare("SELECT name FROM pragma_table_info('aggregator_order')")
            .expect("pragma");
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .expect("query")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect");
        rows
    };
    for forbidden in ["accepted_at", "local_order_id", "printed_at", "closed_at"] {
        assert!(
            !columns.iter().any(|c| c == forbidden),
            "aggregator_order grew an edge-written column `{forbidden}`. It is cloud-authoritative:              acceptance and every state that follows it belong on the local order, derived, not stored here"
        );
    }

    // 2. THE DERIVATION. Nothing accepted yet: no local order exists.
    assert!(
        repo::aggregator_order_acceptance(db.connection(), "outlet-1", "EXT-1")
            .expect("derive")
            .is_none(),
        "with no local order there is nothing to accept"
    );
    assert_eq!(
        repo::list_unaccepted_aggregator_orders(db.connection(), "outlet-1")
            .expect("queue")
            .len(),
        1,
        "an unaccepted document must appear in the queue a till reads"
    );

    // Accepting IS creating the local order. That is the only write.
    db.connection()
        .execute(
            "INSERT INTO \"order\" (id, outlet_id, device_id, order_type, status, external_order_id, created_at, updated_at)
             VALUES ('order-9', 'outlet-1', 'device-1', 'DELIVERY', 'CONFIRMED', 'EXT-1', '2026-09-08T12:05:00Z', '2026-09-08T12:05:00Z')",
            [],
        )
        .expect("accept by creating the local order");

    let (accepted_at, local_order_id) =
        repo::aggregator_order_acceptance(db.connection(), "outlet-1", "EXT-1")
            .expect("derive")
            .expect("the local order exists, so the document is accepted");
    assert_eq!(accepted_at, "2026-09-08T12:05:00Z");
    assert_eq!(local_order_id, "order-9");

    assert!(
        repo::list_unaccepted_aggregator_orders(db.connection(), "outlet-1")
            .expect("queue")
            .is_empty(),
        "an accepted document must leave the queue"
    );

    // 3. AND IT SURVIVES LATER CLOUD DOCUMENTS -- structurally, not by a guard.
    // A cloud document cannot touch an `order` row, so there is no path by
    // which this can be cleared.
    repo::apply_aggregator_order(db.connection(), &document(2, "In-progress"), &lines).expect("v2");
    repo::apply_aggregator_order(db.connection(), &document(3, "Cancelled"), &lines).expect("v3");

    let still = repo::aggregator_order_acceptance(db.connection(), "outlet-1", "EXT-1")
        .expect("derive")
        .expect("acceptance must survive any number of later platform documents");
    assert_eq!(
        still.1, "order-9",
        "a cloud document reached the local order — the till has an order in a kitchen for a document that now says nobody accepted it"
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
