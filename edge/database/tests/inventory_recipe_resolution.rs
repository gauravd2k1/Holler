//! Milestone 4, track T1 (ADR-018): recipe resolution, transitive through
//! sub-recipes, exact-rational, rounded once — against realistic fixtures
//! (a real menu item shape, real spec quantities from `docs/spec/
//! inventory.md`'s Butter Chicken example), not synthetic placeholders.
//!
//! Every fixture insert is asserted to have actually landed before any
//! later assertion depends on it — a failed INSERT silently leaves zero
//! rows and makes every later assertion trivially pass, which is exactly
//! the failure mode this file exists to avoid repeating.
//!
//! Runtime: `cargo test`, native Windows (ADR-013 — this crate has no
//! non-Windows target).

use rusqlite::{params, Connection};

use holler_edge_database::inventory::{resolve_recipe_for_variant, GapReason, ResolveOutcome};
use holler_edge_database::model::{MenuCategory, MenuItem, MenuItemVariant, Outlet};
use holler_edge_database::repo;
use holler_edge_database::Db;

const OUTLET_ID: &str = "outlet-inv-1";

fn seed_outlet_and_category(db: &Db) {
    repo::upsert_outlet(
        db.connection(),
        &Outlet {
            id: OUTLET_ID.to_string(),
            brand_id: "brand-inv-1".to_string(),
            name: "Inventory Test Outlet".to_string(),
            timezone: "Asia/Kolkata".to_string(),
            config_version: 1,
            created_at: "2026-08-20T00:00:00Z".to_string(),
            updated_at: "2026-08-20T00:00:00Z".to_string(),
        },
    )
    .expect("seed outlet");

    repo::upsert_menu_category(
        db.connection(),
        &MenuCategory {
            id: "cat-inv-1".to_string(),
            outlet_id: OUTLET_ID.to_string(),
            name: "Mains".to_string(),
            sort_order: 1,
            config_version: 1,
        },
    )
    .expect("seed category");
}

/// Creates a real menu item + one real variant (the `recipe.
/// menu_item_variant_id` grain, ADR-018 §2) and asserts both rows actually
/// landed before returning the variant id — the "assert your fixtures
/// exist" rule.
fn seed_menu_item_with_variant(db: &Db, item_id: &str, variant_id: &str, name: &str) -> String {
    repo::upsert_menu_item(
        db.connection(),
        &MenuItem {
            id: item_id.to_string(),
            outlet_id: OUTLET_ID.to_string(),
            category_id: "cat-inv-1".to_string(),
            name: name.to_string(),
            base_price_paise: 30_000,
            is_available: true,
            config_version: 1,
            tax_profile_id: None,
            hsn_sac: Some("9963".to_string()),
        },
    )
    .expect("seed menu item");

    repo::upsert_menu_item_variant(
        db.connection(),
        &MenuItemVariant {
            id: variant_id.to_string(),
            menu_item_id: item_id.to_string(),
            name: "Regular".to_string(),
            price_delta_paise: 0,
            config_version: 1,
        },
    )
    .expect("seed menu item variant");

    let item_count: i64 = db
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM menu_item WHERE id = ?1",
            [item_id],
            |r| r.get(0),
        )
        .expect("count menu_item");
    assert_eq!(item_count, 1, "menu_item fixture did not land: {item_id}");
    let variant_count: i64 = db
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM menu_item_variant WHERE id = ?1",
            [variant_id],
            |r| r.get(0),
        )
        .expect("count menu_item_variant");
    assert_eq!(
        variant_count, 1,
        "menu_item_variant fixture did not land: {variant_id}"
    );

    variant_id.to_string()
}

/// Inserts an `inventory_item` row directly (no repo helper exists for this
/// M4 config table yet — T1 is resolution only, not the config write path)
/// and asserts it landed.
fn insert_inventory_item(conn: &Connection, id: &str, name: &str, dimension: &str) {
    conn.execute(
        "INSERT INTO inventory_item (id, outlet_id, sku, name, dimension, config_version) \
         VALUES (?1, ?2, ?3, ?4, ?5, 1)",
        params![id, OUTLET_ID, id, name, dimension],
    )
    .expect("insert inventory_item");
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM inventory_item WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .expect("count inventory_item");
    assert_eq!(count, 1, "inventory_item fixture did not land: {id}");
}

/// Inserts a `recipe` row bound to `variant_id` and asserts it landed.
fn insert_recipe(conn: &Connection, id: &str, variant_id: &str, name: &str) {
    conn.execute(
        "INSERT INTO recipe (id, menu_item_variant_id, name, config_version) \
         VALUES (?1, ?2, ?3, 1)",
        params![id, variant_id, name],
    )
    .expect("insert recipe");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM recipe WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .expect("count recipe");
    assert_eq!(count, 1, "recipe fixture did not land: {id}");
}

fn insert_item_ingredient(
    conn: &Connection,
    id: &str,
    recipe_id: &str,
    inventory_item_id: &str,
    quantity_micro: i64,
) {
    conn.execute(
        "INSERT INTO recipe_ingredient \
            (id, recipe_id, component_kind, inventory_item_id, sub_recipe_id, quantity_micro, config_version) \
         VALUES (?1, ?2, 'ITEM', ?3, NULL, ?4, 1)",
        params![id, recipe_id, inventory_item_id, quantity_micro],
    )
    .expect("insert ITEM recipe_ingredient");
    assert_row_exists(conn, "recipe_ingredient", id);
}

fn insert_sub_recipe_ingredient(
    conn: &Connection,
    id: &str,
    recipe_id: &str,
    sub_recipe_id: &str,
    quantity_micro: i64,
) {
    conn.execute(
        "INSERT INTO recipe_ingredient \
            (id, recipe_id, component_kind, inventory_item_id, sub_recipe_id, quantity_micro, config_version) \
         VALUES (?1, ?2, 'SUB_RECIPE', NULL, ?3, ?4, 1)",
        params![id, recipe_id, sub_recipe_id, quantity_micro],
    )
    .expect("insert SUB_RECIPE recipe_ingredient");
    assert_row_exists(conn, "recipe_ingredient", id);
}

fn assert_row_exists(conn: &Connection, table: &str, id: &str) {
    let count: i64 = conn
        .query_row(&format!("SELECT COUNT(*) FROM {table} WHERE id = ?1"), [id], |r| {
            r.get(0)
        })
        .unwrap_or_else(|_| panic!("could not count {table}"));
    assert_eq!(count, 1, "{table} fixture did not land: {id}");
}

fn leaf<'a>(
    outcome: &'a ResolveOutcome,
    inventory_item_id: &str,
) -> Option<&'a holler_edge_database::inventory::ResolvedLeaf> {
    match outcome {
        ResolveOutcome::Resolved(r) => r
            .leaves
            .iter()
            .find(|l| l.inventory_item_id == inventory_item_id),
        ResolveOutcome::Gap(_) => None,
    }
}

// ---------------------------------------------------------------------
// Flat recipe: real menu-item shape (Paneer Tikka style — a variant with
// two direct ingredients, no sub-recipe).
// ---------------------------------------------------------------------

#[test]
fn resolves_a_flat_recipe_scaled_by_order_quantity() {
    let db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet_and_category(&db);
    let variant_id = seed_menu_item_with_variant(
        &db,
        "item-paneer-tikka",
        "variant-paneer-tikka-half",
        "Paneer Tikka",
    );

    insert_inventory_item(db.connection(), "inv-paneer", "Paneer", "MASS");
    insert_inventory_item(db.connection(), "inv-butter", "Butter", "MASS");

    insert_recipe(
        db.connection(),
        "recipe-paneer-tikka-half",
        &variant_id,
        "Paneer Tikka (Half)",
    );
    // 200g paneer, 20g butter per serving.
    insert_item_ingredient(
        db.connection(),
        "ri-paneer",
        "recipe-paneer-tikka-half",
        "inv-paneer",
        200_000_000,
    );
    insert_item_ingredient(
        db.connection(),
        "ri-butter",
        "recipe-paneer-tikka-half",
        "inv-butter",
        20_000_000,
    );

    // Two servings sold.
    let outcome = resolve_recipe_for_variant(db.connection(), Some(&variant_id), 2)
        .expect("resolve should not be a DbError");
    let ResolveOutcome::Resolved(resolution) = &outcome else {
        panic!("expected a resolution, got {outcome:?}");
    };
    assert_eq!(resolution.recipe_id, "recipe-paneer-tikka-half");
    assert_eq!(resolution.recipe_version, 1);
    assert_eq!(resolution.leaves.len(), 2);
    assert_eq!(
        leaf(&outcome, "inv-paneer").unwrap().applied_micro,
        400_000_000
    );
    assert_eq!(
        leaf(&outcome, "inv-butter").unwrap().applied_micro,
        40_000_000
    );
}

// ---------------------------------------------------------------------
// Sub-recipe: docs/spec/inventory.md's own worked example — Butter
// Chicken (Chicken 220g, Makhani gravy 180ml, Butter 20g, Cream 30ml,
// Kasuri methi 2g) — with Makhani Gravy modelled as a genuine sub-recipe
// that itself contributes butter and cream, exercising accumulation of
// the SAME inventory_item across two branches of the tree.
// ---------------------------------------------------------------------

#[test]
fn resolves_a_sub_recipe_transitively_and_accumulates_shared_leaves_across_branches() {
    let db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet_and_category(&db);
    let bc_variant = seed_menu_item_with_variant(
        &db,
        "item-butter-chicken",
        "variant-butter-chicken",
        "Butter Chicken",
    );
    // Makhani Gravy needs a menu_item_variant_id too (recipe.
    // menu_item_variant_id is NOT NULL, ADR-018 §2) even though it is never
    // sold directly — an internal, non-sellable variant, the same pattern
    // the ADR anticipates for a sub-recipe target.
    let gravy_variant = seed_menu_item_with_variant(
        &db,
        "item-makhani-gravy-internal",
        "variant-makhani-gravy-internal",
        "Makhani Gravy (internal)",
    );

    for (id, name, dim) in [
        ("inv-chicken", "Chicken", "MASS"),
        ("inv-butter", "Butter", "MASS"),
        ("inv-cream", "Cream", "VOLUME"),
        ("inv-kasuri-methi", "Kasuri Methi", "MASS"),
        ("inv-tomato", "Tomato", "MASS"),
    ] {
        insert_inventory_item(db.connection(), id, name, dim);
    }

    // Makhani Gravy sub-recipe: "1x" = tomato 150g, butter 15g, cream 15g.
    insert_recipe(
        db.connection(),
        "recipe-makhani-gravy",
        &gravy_variant,
        "Makhani Gravy",
    );
    insert_item_ingredient(
        db.connection(),
        "ri-gravy-tomato",
        "recipe-makhani-gravy",
        "inv-tomato",
        150_000_000,
    );
    insert_item_ingredient(
        db.connection(),
        "ri-gravy-butter",
        "recipe-makhani-gravy",
        "inv-butter",
        15_000_000,
    );
    insert_item_ingredient(
        db.connection(),
        "ri-gravy-cream",
        "recipe-makhani-gravy",
        "inv-cream",
        15_000_000,
    );

    // Butter Chicken root: chicken 220g, 1x Makhani Gravy, butter 20g,
    // cream 30g, kasuri methi 2g direct.
    insert_recipe(
        db.connection(),
        "recipe-butter-chicken",
        &bc_variant,
        "Butter Chicken",
    );
    insert_item_ingredient(
        db.connection(),
        "ri-bc-chicken",
        "recipe-butter-chicken",
        "inv-chicken",
        220_000_000,
    );
    insert_sub_recipe_ingredient(
        db.connection(),
        "ri-bc-gravy",
        "recipe-butter-chicken",
        "recipe-makhani-gravy",
        1_000_000, // exactly once
    );
    insert_item_ingredient(
        db.connection(),
        "ri-bc-butter",
        "recipe-butter-chicken",
        "inv-butter",
        20_000_000,
    );
    insert_item_ingredient(
        db.connection(),
        "ri-bc-cream",
        "recipe-butter-chicken",
        "inv-cream",
        30_000_000,
    );
    insert_item_ingredient(
        db.connection(),
        "ri-bc-kasuri-methi",
        "recipe-butter-chicken",
        "inv-kasuri-methi",
        2_000_000,
    );

    let outcome = resolve_recipe_for_variant(db.connection(), Some(&bc_variant), 1)
        .expect("resolve should not be a DbError");
    let ResolveOutcome::Resolved(resolution) = &outcome else {
        panic!("expected a resolution, got {outcome:?}");
    };
    assert_eq!(resolution.recipe_id, "recipe-butter-chicken");
    // 5 distinct leaves: chicken, butter (direct+gravy), cream
    // (direct+gravy), kasuri methi, tomato (gravy only).
    assert_eq!(resolution.leaves.len(), 5);

    assert_eq!(leaf(&outcome, "inv-chicken").unwrap().applied_micro, 220_000_000);
    assert_eq!(leaf(&outcome, "inv-tomato").unwrap().applied_micro, 150_000_000);
    // Butter: 20g direct + 15g via one gravy application == 35g.
    assert_eq!(leaf(&outcome, "inv-butter").unwrap().applied_micro, 35_000_000);
    // Cream: 30g direct + 15g via gravy == 45g.
    assert_eq!(leaf(&outcome, "inv-cream").unwrap().applied_micro, 45_000_000);
    assert_eq!(
        leaf(&outcome, "inv-kasuri-methi").unwrap().applied_micro,
        2_000_000
    );
}

// ---------------------------------------------------------------------
// Rounding: exact resolution disagrees with rounding at each level
// (ADR-018 §5) — proved through a real DB-backed three-level chain, not
// only the pure-math unit test in inventory::rational.
// ---------------------------------------------------------------------

#[test]
fn rounds_exactly_once_at_the_leaf_and_disagrees_with_per_level_rounding() {
    let db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet_and_category(&db);
    let root_variant =
        seed_menu_item_with_variant(&db, "item-rounding-demo", "variant-rounding-demo", "Rounding Demo");
    let level1_variant = seed_menu_item_with_variant(
        &db,
        "item-rounding-l1",
        "variant-rounding-l1",
        "Rounding L1 (internal)",
    );
    let level2_variant = seed_menu_item_with_variant(
        &db,
        "item-rounding-l2",
        "variant-rounding-l2",
        "Rounding L2 (internal)",
    );

    insert_inventory_item(db.connection(), "inv-essence", "Rare Essence", "MASS");

    // Level 2 (deepest): 1x = 5 micro-grams of essence.
    insert_recipe(db.connection(), "recipe-rounding-l2", &level2_variant, "L2");
    insert_item_ingredient(
        db.connection(),
        "ri-l2-essence",
        "recipe-rounding-l2",
        "inv-essence",
        5,
    );

    // Level 1: 0.333334x of level 2 (an integer quantity_micro
    // approximating 1/3 -- quantity_micro is always an integer, so this is
    // the closest a single row can get to a genuine repeating fraction).
    insert_recipe(db.connection(), "recipe-rounding-l1", &level1_variant, "L1");
    insert_sub_recipe_ingredient(
        db.connection(),
        "ri-l1-l2",
        "recipe-rounding-l1",
        "recipe-rounding-l2",
        333_334,
    );

    // Root: 0.333334x of level 1.
    insert_recipe(db.connection(), "recipe-rounding-root", &root_variant, "Root");
    insert_sub_recipe_ingredient(
        db.connection(),
        "ri-root-l1",
        "recipe-rounding-root",
        "recipe-rounding-l1",
        333_334,
    );

    let outcome = resolve_recipe_for_variant(db.connection(), Some(&root_variant), 1)
        .expect("resolve should not be a DbError");
    let ResolveOutcome::Resolved(_) = &outcome else {
        panic!("expected a resolution, got {outcome:?}");
    };

    // Exact: 1 * (333334/1000000) * (333334/1000000) * 5
    //      = 555_557_777_780 / 1_000_000_000_000 ~= 0.5556 -> rounds to 1.
    //
    // Per-level rounding would instead round 0.333334 -> 0 at the first
    // sub-recipe step, collapsing the whole chain to 0 forever — the exact
    // drift ADR-018 §5 exists to prevent. Round-once-at-the-leaf gives 1.
    assert_eq!(leaf(&outcome, "inv-essence").unwrap().applied_micro, 1);
}

// ---------------------------------------------------------------------
// Gap outcomes: never a DbError, never a failed transaction.
// ---------------------------------------------------------------------

#[test]
fn no_variant_is_a_gap_not_an_error() {
    let db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet_and_category(&db);
    let outcome = resolve_recipe_for_variant(db.connection(), None, 1).expect("no DbError");
    assert_eq!(outcome, ResolveOutcome::Gap(GapReason::NoVariant));
}

#[test]
fn missing_recipe_for_a_real_variant_is_a_gap_not_an_error() {
    let db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet_and_category(&db);
    let variant_id =
        seed_menu_item_with_variant(&db, "item-no-recipe", "variant-no-recipe", "No Recipe Item");

    let outcome =
        resolve_recipe_for_variant(db.connection(), Some(&variant_id), 1).expect("no DbError");
    assert_eq!(outcome, ResolveOutcome::Gap(GapReason::NoRecipe));
}

#[test]
fn a_recipe_referencing_an_unsynced_inventory_item_is_an_unknown_unit_gap() {
    // Simulates config arriving out of order — recipe_ingredient synced
    // before the inventory_item it names — which the resolver must survive
    // defensively even though ordinary sync ingestion, with foreign_keys
    // ON, cannot itself produce this state. Foreign keys are turned off
    // for exactly the one INSERT that manufactures the dangling reference,
    // then restored, matching ADR-018 §7's framing: "config arrives over a
    // wire from a service that may be older than this rule, or from a
    // database restored from before it."
    let db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet_and_category(&db);
    let variant_id = seed_menu_item_with_variant(
        &db,
        "item-dangling",
        "variant-dangling",
        "Dangling Reference Item",
    );
    insert_recipe(db.connection(), "recipe-dangling", &variant_id, "Dangling");

    db.connection()
        .execute("PRAGMA foreign_keys = OFF", [])
        .expect("disable foreign_keys for the dangling-fixture insert");
    insert_item_ingredient(
        db.connection(),
        "ri-dangling",
        "recipe-dangling",
        "inv-does-not-exist",
        1_000_000,
    );
    db.connection()
        .execute("PRAGMA foreign_keys = ON", [])
        .expect("restore foreign_keys");

    let outcome =
        resolve_recipe_for_variant(db.connection(), Some(&variant_id), 1).expect("no DbError");
    assert_eq!(outcome, ResolveOutcome::Gap(GapReason::UnknownUnit));
}

// ---------------------------------------------------------------------
// FALSIFICATION: the resolver must terminate on a genuine cycle, and must
// stop at the depth limit, rather than hang the only SQLite writer at the
// outlet (ADR-018 §7). Both guards are constructed adversarially here and
// watched to actually trip — not merely exercised on the happy path.
// ---------------------------------------------------------------------

#[test]
fn a_genuine_two_step_cycle_terminates_as_a_cycle_gap() {
    // A -> B -> C -> A. The cloud write-time DFS check is supposed to
    // reject this at authoring time; this constructs it directly in
    // SQLite (foreign_keys OFF only for the one edge that closes the
    // loop -- recipe_ingredient.sub_recipe_id has no self-contained way to
    // reference a recipe id that must, by definition, already exist before
    // the edge closing the loop is inserted) to prove the EDGE'S OWN
    // backstop, independent of that cloud check, actually stops it.
    let db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet_and_category(&db);
    let variant_a = seed_menu_item_with_variant(&db, "item-a", "variant-a", "A (internal)");
    let variant_b = seed_menu_item_with_variant(&db, "item-b", "variant-b", "B (internal)");
    let variant_c = seed_menu_item_with_variant(&db, "item-c", "variant-c", "C (internal)");

    insert_recipe(db.connection(), "recipe-a", &variant_a, "A");
    insert_recipe(db.connection(), "recipe-b", &variant_b, "B");
    insert_recipe(db.connection(), "recipe-c", &variant_c, "C");

    // A -> B, B -> C: both are ordinary forward references to
    // already-existing rows, no FK trick needed.
    insert_sub_recipe_ingredient(db.connection(), "ri-a-b", "recipe-a", "recipe-b", 1_000_000);
    insert_sub_recipe_ingredient(db.connection(), "ri-b-c", "recipe-b", "recipe-c", 1_000_000);
    // C -> A closes the cycle.
    insert_sub_recipe_ingredient(db.connection(), "ri-c-a", "recipe-c", "recipe-a", 1_000_000);

    // Prerequisite: the loop-closing row must actually exist, or the
    // "resolver terminates on a cycle" claim below would be trivially true
    // for the wrong reason (nothing to walk).
    assert_row_exists(db.connection(), "recipe_ingredient", "ri-c-a");

    let outcome =
        resolve_recipe_for_variant(db.connection(), Some(&variant_a), 1).expect("no DbError");

    // FALSIFIED (not merely asserted): with `path.contains(&sub_id)`
    // temporarily disabled, this exact fixture was run and produced
    // `Gap(DepthExceeded)` instead of `Gap(Cycle)` -- the walk still
    // terminated, but only because `MAX_RECIPE_DEPTH` independently caught
    // it after wasted recursion around the loop, proving the cycle guard
    // is not redundant with the depth guard: it is what makes this
    // resolver report the RIGHT reason, immediately, rather than merely
    // surviving by accident of the other guard. Restored before this file
    // was finalised.
    assert_eq!(outcome, ResolveOutcome::Gap(GapReason::Cycle));
}

#[test]
fn a_chain_deeper_than_max_recipe_depth_terminates_as_a_depth_exceeded_gap() {
    use holler_edge_database::inventory::MAX_RECIPE_DEPTH;

    let db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet_and_category(&db);

    // A straight, ACYCLIC chain of MAX_RECIPE_DEPTH + 3 recipes, each
    // referencing the next as its sole sub-recipe, terminating in one leaf
    // ingredient. Acyclic on purpose: this proves the depth guard trips
    // independently of the cycle guard, on a graph the cycle check alone
    // would happily walk forever... "forever" bounded only by how many
    // recipes exist, which is why the depth guard exists as a SEPARATE
    // backstop, not a restatement of the cycle guard.
    let depth: u32 = MAX_RECIPE_DEPTH + 3;
    let mut recipe_ids = Vec::with_capacity(depth as usize);
    for level in 0..depth {
        let item_id = format!("item-depth-{level}");
        let variant_id = format!("variant-depth-{level}");
        seed_menu_item_with_variant(&db, &item_id, &variant_id, &format!("Depth {level}"));
        let recipe_id = format!("recipe-depth-{level}");
        insert_recipe(db.connection(), &recipe_id, &variant_id, &format!("D{level}"));
        recipe_ids.push(recipe_id);
    }
    insert_inventory_item(db.connection(), "inv-depth-leaf", "Depth Leaf", "MASS");

    for level in 0..(depth as usize - 1) {
        insert_sub_recipe_ingredient(
            db.connection(),
            &format!("ri-depth-{level}"),
            &recipe_ids[level],
            &recipe_ids[level + 1],
            1_000_000,
        );
    }
    // The deepest recipe finally references a real leaf, so a resolver
    // with no depth bound would walk all the way down and succeed with a
    // real (if absurd) answer -- proving the guard trips on DEPTH, not on
    // "there is nothing further to find".
    insert_item_ingredient(
        db.connection(),
        "ri-depth-leaf",
        recipe_ids.last().unwrap(),
        "inv-depth-leaf",
        1_000_000,
    );

    let root_variant_id = "variant-depth-0";
    let outcome =
        resolve_recipe_for_variant(db.connection(), Some(root_variant_id), 1).expect("no DbError");

    // FALSIFIED (not merely asserted): with `walk`'s depth check
    // temporarily disabled (`if false && path.len() ...`), this exact
    // fixture was run and it resolved successfully instead of gapping --
    // `ResolveOutcome::Resolved(..)` with `applied_micro: 1_000_000` on the
    // deep leaf -- confirming the walk really does reach the bottom of an
    // over-deep chain when nothing stops it, and that this guard, not
    // something else (e.g. the cycle guard, irrelevant here since the
    // chain is acyclic), is what turns that into a gap. Restored before
    // this file was finalised.
    assert_eq!(outcome, ResolveOutcome::Gap(GapReason::DepthExceeded));
}
