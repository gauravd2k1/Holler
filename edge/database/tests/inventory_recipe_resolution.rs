//! Milestone 4, track T1 (ADR-018; contracts 0.5.1 `recipe.output_*`
//! addendum, `packages/contracts/sqlite/0019_recipe_output.sql`): recipe
//! resolution, transitive through sub-recipes, exact-rational, rounded
//! once — against realistic fixtures (a real menu item shape, the spec's
//! own worked Butter Chicken example, read out of `docs/spec/inventory.md`
//! itself), not synthetic placeholders.
//!
//! Every fixture insert is asserted to have actually landed before any
//! later assertion depends on it — a failed INSERT silently leaves zero
//! rows and makes every later assertion trivially pass, which is exactly
//! the failure mode this file exists to avoid repeating.
//!
//! Runtime: `cargo test`, native Windows (ADR-013 — this crate has no
//! non-Windows target).

use std::fs;

use rusqlite::{params, Connection};

use holler_edge_database::inventory::{
    convert_tier1, resolve_recipe_for_variant, GapReason, ResolveOutcome,
};
use holler_edge_database::model::{MenuCategory, MenuItem, MenuItemVariant, Outlet};
use holler_edge_database::repo;
use holler_edge_database::Db;

const OUTLET_ID: &str = "outlet-inv-1";

/// One canonical serving, in COUNT micro-units — mirrors
/// `inventory::resolve`'s private `ONE_SERVING_MICRO`; duplicated here
/// (rather than exported) because a test fixture should compute its
/// expectations independently of the production constant it is checking.
const ONE_SERVING_MICRO: i64 = 1_000_000;

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

/// Inserts a `recipe` row bound to `variant_id`, with an explicit
/// `output_dimension`/`output_quantity_micro` (contracts 0.5.1 — every
/// recipe has one, not only recipes used as sub-recipes) at every call
/// site, deliberately: relying on the migration's `DEFAULT` would hide a
/// mismatch bug that an explicit value forces a test author to confront.
/// Asserts the row landed.
fn insert_recipe(
    conn: &Connection,
    id: &str,
    variant_id: &str,
    name: &str,
    output_dimension: &str,
    output_quantity_micro: i64,
) {
    conn.execute(
        "INSERT INTO recipe \
            (id, menu_item_variant_id, name, output_dimension, output_quantity_micro, config_version) \
         VALUES (?1, ?2, ?3, ?4, ?5, 1)",
        params![id, variant_id, name, output_dimension, output_quantity_micro],
    )
    .expect("insert recipe");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM recipe WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .expect("count recipe");
    assert_eq!(count, 1, "recipe fixture did not land: {id}");
}

/// A one-serving dish: `output_dimension = COUNT`,
/// `output_quantity_micro = 1_000_000` — the shape every directly-sellable
/// (non-platter) recipe in these fixtures uses.
fn insert_one_serving_recipe(conn: &Connection, id: &str, variant_id: &str, name: &str) {
    insert_recipe(conn, id, variant_id, name, "COUNT", ONE_SERVING_MICRO);
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
        .query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE id = ?1"),
            [id],
            |r| r.get(0),
        )
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

    insert_one_serving_recipe(
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
// The spec's own worked example, read out of the spec itself
// (`docs/spec/inventory.md:16`), so spec and code cannot silently drift
// apart again. Makhani Gravy is a real sub-recipe with a real yield (300ml
// per batch) different from the 180ml Butter Chicken actually uses — the
// exact scenario contracts 0.5.1 exists for: the sub-recipe's own output
// quantity is independent of how much of it a parent asks for, and a
// change to one does not silently rescale the other.
// ---------------------------------------------------------------------

/// Splits an item like `"Makhani gravy 180ml"` into
/// `("Makhani gravy", 180, "ml")` — trailing alphabetic run is the unit,
/// the digit run immediately before it is the quantity, everything before
/// that (trimmed) is the name. No regex crate is a dependency of this
/// workspace; this hand-rolled scan is small enough not to need one.
fn split_name_quantity_unit(item: &str) -> (String, i64, String) {
    let item = item.trim();
    let chars: Vec<char> = item.chars().collect();
    let mut i = chars.len();
    let unit_start = {
        let mut j = i;
        while j > 0 && chars[j - 1].is_ascii_alphabetic() {
            j -= 1;
        }
        j
    };
    let unit: String = chars[unit_start..i].iter().collect();
    i = unit_start;
    let qty_start = {
        let mut j = i;
        while j > 0 && chars[j - 1].is_ascii_digit() {
            j -= 1;
        }
        j
    };
    let qty: i64 = chars[qty_start..i]
        .iter()
        .collect::<String>()
        .parse()
        .unwrap_or_else(|e| panic!("could not parse quantity out of {item:?}: {e}"));
    let name: String = chars[..qty_start].iter().collect::<String>().trim().to_string();
    (name, qty, unit)
}

/// Reads `docs/spec/inventory.md`, finds its own Butter Chicken worked
/// example, and returns the five `(name, quantity, unit)` triples it
/// states. Panics with a clear message if the line's shape ever changes —
/// a brittle guard here is the point: it is what keeps this test and the
/// spec from silently disagreeing.
fn read_butter_chicken_example_from_spec() -> Vec<(String, i64, String)> {
    let spec_path = format!(
        "{}/../../docs/spec/inventory.md",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = fs::read_to_string(&spec_path)
        .unwrap_or_else(|e| panic!("could not read {spec_path}: {e}"));
    let marker = "Butter Chicken: ";
    let start = text
        .find(marker)
        .unwrap_or_else(|| panic!("spec no longer contains {marker:?}: {spec_path}"))
        + marker.len();
    let rest = &text[start..];
    let end = rest
        .find(')')
        .unwrap_or_else(|| panic!("no closing ')' after {marker:?} in {spec_path}"));
    let example = &rest[..end];
    example.split(", ").map(split_name_quantity_unit).collect()
}

#[test]
fn the_spec_line_parses_to_the_five_quantities_this_fixture_encodes() {
    // "Butter Chicken: Chicken 220g, Makhani gravy 180ml, Butter 20g,
    // Cream 30ml, Kasuri methi 2g" — docs/spec/inventory.md:16, quoted here
    // so a diff on either side is visible without cross-referencing.
    let parsed = read_butter_chicken_example_from_spec();
    assert_eq!(
        parsed,
        vec![
            ("Chicken".to_string(), 220, "g".to_string()),
            ("Makhani gravy".to_string(), 180, "ml".to_string()),
            ("Butter".to_string(), 20, "g".to_string()),
            ("Cream".to_string(), 30, "ml".to_string()),
            ("Kasuri methi".to_string(), 2, "g".to_string()),
        ],
        "docs/spec/inventory.md's Butter Chicken example no longer matches \
         this test's literal expectation — update BOTH sides deliberately, \
         not just this assertion"
    );
}

#[test]
fn resolves_the_spec_butter_chicken_example_with_a_real_sub_recipe_yield() {
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

    // Makhani Gravy: a real sub-recipe with a real yield, 300 ml a batch —
    // deliberately NOT 180ml (what Butter Chicken actually uses), so the
    // resolved 180/300 = 0.6 multiplier is genuinely exercised rather than
    // trivially 1. Batch composition (tomato/butter/cream) is this test's
    // own invented internals; the spec gives no sub-recipe breakdown.
    let (_, gravy_output, gravy_unit) = ("Makhani gravy".to_string(), 300i64, "ml".to_string());
    let (gravy_dimension, gravy_output_micro) =
        convert_tier1(&gravy_unit, gravy_output as i128).expect("ml is a Tier 1 unit");
    insert_recipe(
        db.connection(),
        "recipe-makhani-gravy",
        &gravy_variant,
        "Makhani Gravy",
        gravy_dimension.as_str(),
        gravy_output_micro as i64,
    );
    insert_item_ingredient(
        db.connection(),
        "ri-gravy-tomato",
        "recipe-makhani-gravy",
        "inv-tomato",
        convert_tier1("g", 250).unwrap().1 as i64,
    );
    insert_item_ingredient(
        db.connection(),
        "ri-gravy-butter",
        "recipe-makhani-gravy",
        "inv-butter",
        convert_tier1("g", 25).unwrap().1 as i64,
    );
    insert_item_ingredient(
        db.connection(),
        "ri-gravy-cream",
        "recipe-makhani-gravy",
        "inv-cream",
        convert_tier1("ml", 25).unwrap().1 as i64,
    );

    // The spec's own five quantities, read from the file rather than
    // hand-copied twice.
    let spec = read_butter_chicken_example_from_spec();
    let by_name = |n: &str| spec.iter().find(|(name, _, _)| name == n).unwrap();
    let (_, chicken_qty, chicken_unit) = by_name("Chicken");
    let (_, gravy_used_qty, gravy_used_unit) = by_name("Makhani gravy");
    let (_, butter_qty, butter_unit) = by_name("Butter");
    let (_, cream_qty, cream_unit) = by_name("Cream");
    let (_, kasuri_qty, kasuri_unit) = by_name("Kasuri methi");

    insert_one_serving_recipe(
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
        convert_tier1(chicken_unit, *chicken_qty as i128).unwrap().1 as i64,
    );
    insert_sub_recipe_ingredient(
        db.connection(),
        "ri-bc-gravy",
        "recipe-butter-chicken",
        "recipe-makhani-gravy",
        convert_tier1(gravy_used_unit, *gravy_used_qty as i128)
            .unwrap()
            .1 as i64,
    );
    insert_item_ingredient(
        db.connection(),
        "ri-bc-butter",
        "recipe-butter-chicken",
        "inv-butter",
        convert_tier1(butter_unit, *butter_qty as i128).unwrap().1 as i64,
    );
    insert_item_ingredient(
        db.connection(),
        "ri-bc-cream",
        "recipe-butter-chicken",
        "inv-cream",
        convert_tier1(cream_unit, *cream_qty as i128).unwrap().1 as i64,
    );
    insert_item_ingredient(
        db.connection(),
        "ri-bc-kasuri-methi",
        "recipe-butter-chicken",
        "inv-kasuri-methi",
        convert_tier1(kasuri_unit, *kasuri_qty as i128).unwrap().1 as i64,
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

    assert_eq!(
        leaf(&outcome, "inv-chicken").unwrap().applied_micro,
        220_000_000
    );
    // Tomato: gravy-only, scaled by 180/300 = 0.6 of the batch's 250g.
    assert_eq!(
        leaf(&outcome, "inv-tomato").unwrap().applied_micro,
        150_000_000
    );
    // Butter: 20g direct + (0.6 * 25g via gravy = 15g) == 35g.
    assert_eq!(
        leaf(&outcome, "inv-butter").unwrap().applied_micro,
        35_000_000
    );
    // Cream: 30ml direct + (0.6 * 25ml via gravy = 15ml) == 45ml. VOLUME's
    // Tier 1 canonical unit is the litre, so "ml" scales by 1_000 (not
    // 1_000_000 -- that is "g"'s MASS factor): 45ml = 45_000 micro-litres.
    assert_eq!(
        leaf(&outcome, "inv-cream").unwrap().applied_micro,
        45_000
    );
    assert_eq!(
        leaf(&outcome, "inv-kasuri-methi").unwrap().applied_micro,
        2_000_000
    );
}

// ---------------------------------------------------------------------
// The sharing platter: a recipe whose own output covers more than one
// serving, and an order that requests fewer servings than a whole
// execution — both unrepresentable under the old "multiplier" reading,
// both now exact-rational, no rounding needed.
// ---------------------------------------------------------------------

#[test]
fn a_two_serving_recipe_is_expressible_and_scales_by_servings_requested_not_executions() {
    let db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet_and_category(&db);
    let variant_id = seed_menu_item_with_variant(
        &db,
        "item-family-platter",
        "variant-family-platter",
        "Family Platter",
    );
    insert_inventory_item(db.connection(), "inv-chicken-platter", "Chicken", "MASS");

    // Recipe as authored: ONE execution yields 2 servings, and uses 500g
    // chicken total for that one execution.
    insert_recipe(
        db.connection(),
        "recipe-family-platter",
        &variant_id,
        "Family Platter",
        "COUNT",
        2 * ONE_SERVING_MICRO,
    );
    insert_item_ingredient(
        db.connection(),
        "ri-platter-chicken",
        "recipe-family-platter",
        "inv-chicken-platter",
        500_000_000,
    );

    // 2 servings requested (one whole platter, one execution): the FULL
    // 500g applies, not half and not doubled.
    let outcome = resolve_recipe_for_variant(db.connection(), Some(&variant_id), 2)
        .expect("resolve should not be a DbError");
    let ResolveOutcome::Resolved(_) = &outcome else {
        panic!("expected a resolution, got {outcome:?}");
    };
    assert_eq!(
        leaf(&outcome, "inv-chicken-platter").unwrap().applied_micro,
        500_000_000,
        "one whole platter (2 servings requested) must deduct exactly one \
         execution's ingredients, not a rescaled amount"
    );

    // 4 servings requested (two whole platters, two executions): doubles.
    let outcome4 = resolve_recipe_for_variant(db.connection(), Some(&variant_id), 4)
        .expect("resolve should not be a DbError");
    assert_eq!(
        leaf(&outcome4, "inv-chicken-platter").unwrap().applied_micro,
        1_000_000_000
    );

    // 1 serving requested (half a platter, e.g. a half-portion sold from a
    // shared preparation): an EXACT half-execution, no rounding needed
    // since 500_000_000 * 0.5 is itself exact.
    let outcome1 = resolve_recipe_for_variant(db.connection(), Some(&variant_id), 1)
        .expect("resolve should not be a DbError");
    assert_eq!(
        leaf(&outcome1, "inv-chicken-platter").unwrap().applied_micro,
        250_000_000
    );
}

// ---------------------------------------------------------------------
// Rounding: exact resolution disagrees with rounding at each level
// (ADR-018 §5) — proved through a real DB-backed three-level chain, not
// only the pure-math unit test in inventory::rational, and now via the
// output-quantity formula (contracts 0.5.1) rather than the old
// "micro-scaled multiplier" one.
// ---------------------------------------------------------------------

#[test]
fn rounds_exactly_once_at_the_leaf_and_disagrees_with_per_level_rounding() {
    let db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet_and_category(&db);
    let root_variant = seed_menu_item_with_variant(
        &db,
        "item-rounding-demo",
        "variant-rounding-demo",
        "Rounding Demo",
    );
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

    // Level 2 (deepest): one execution yields 1_000_000 micro-units of its
    // own declared (arbitrary, MASS here) output, and uses 5 micro-grams
    // of essence per execution.
    insert_recipe(
        db.connection(),
        "recipe-rounding-l2",
        &level2_variant,
        "L2",
        "MASS",
        1_000_000,
    );
    insert_item_ingredient(
        db.connection(),
        "ri-l2-essence",
        "recipe-rounding-l2",
        "inv-essence",
        5,
    );

    // Level 1: one execution yields 1_000_000 micro-units of its own
    // output, and requests 333_334 micro-units of L2's output per
    // execution — an integer quantity_micro approximating 1/3 (the
    // closest a single row can get to a genuine repeating fraction, since
    // quantity_micro is always an integer).
    insert_recipe(
        db.connection(),
        "recipe-rounding-l1",
        &level1_variant,
        "L1",
        "MASS",
        1_000_000,
    );
    insert_sub_recipe_ingredient(
        db.connection(),
        "ri-l1-l2",
        "recipe-rounding-l1",
        "recipe-rounding-l2",
        333_334,
    );

    // Root: one serving requests 333_334 micro-units of L1's output.
    insert_one_serving_recipe(db.connection(), "recipe-rounding-root", &root_variant, "Root");
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

/// Falsifies the "never materialise the multiplier as a rounded number"
/// rule the 0.5.1 migration header restates (the 333_334/1e6 defect
/// arriving from the new output-quantity direction): if the CHILD's
/// multiplier were rounded to an integer before being applied to the
/// leaf, instead of carried as an exact rational, this exact fixture
/// would collapse to 0 instead of resolving to 1. This test does not
/// disable code (the production path already carries the exact rational,
/// proved by the test above); it independently hand-computes what a
/// rounded-multiplier implementation WOULD produce and asserts that wrong
/// answer disagrees with the real one, so the two paths cannot silently
/// converge by coincidence.
#[test]
fn a_naively_rounded_multiplier_would_disagree_with_the_exact_resolver() {
    // Same 333_334/1_000_000 two-level chain as the test above.
    let requested_of_l1 = 333_334i128;
    let l1_multiplier_rounded = {
        // round_half_away_from_zero(333334 / 1_000_000) = round(0.333334) = 0
        let n = requested_of_l1;
        let d = 1_000_000i128;
        (2 * n + d) / (2 * d)
    };
    assert_eq!(
        l1_multiplier_rounded, 0,
        "a multiplier rounded at the first sub-recipe level must collapse to 0"
    );
    // Once rounded to the integer 0, every subsequent level multiplies 0 by
    // something and the leaf receives 0 — permanently disagreeing with the
    // exact resolver's real answer of 1 (asserted by
    // `rounds_exactly_once_at_the_leaf_and_disagrees_with_per_level_rounding`
    // above, against the actual production code path).
    let naive_leaf_result = l1_multiplier_rounded * 333_334 / 1_000_000 * 5;
    assert_eq!(naive_leaf_result, 0);
    assert_ne!(
        naive_leaf_result, 1,
        "the naive per-level-rounded answer must NOT match the exact resolver's answer"
    );
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
    insert_one_serving_recipe(db.connection(), "recipe-dangling", &variant_id, "Dangling");

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
// DIMENSION_MISMATCH (contracts 0.5.1): a root recipe whose own declared
// output isn't COUNT can never satisfy an order, which always requests a
// COUNT quantity (servings sold) — an authoring defect (e.g. a gravy
// recipe mistakenly bound to a sellable variant) the cloud is meant to
// reject at write time, gapped defensively at the edge.
// ---------------------------------------------------------------------

#[test]
fn a_root_recipe_whose_output_is_not_count_is_a_dimension_mismatch_gap() {
    let db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet_and_category(&db);
    let variant_id = seed_menu_item_with_variant(
        &db,
        "item-mis-bound-gravy",
        "variant-mis-bound-gravy",
        "Mis-bound Gravy",
    );
    insert_inventory_item(db.connection(), "inv-tomato-mismatch", "Tomato", "MASS");
    // Authored (in error) as a VOLUME-yielding recipe bound directly to a
    // sellable variant, instead of only ever being referenced as a
    // SUB_RECIPE component.
    insert_recipe(
        db.connection(),
        "recipe-mis-bound-gravy",
        &variant_id,
        "Mis-bound Gravy",
        "VOLUME",
        300_000_000,
    );
    insert_item_ingredient(
        db.connection(),
        "ri-mismatch-tomato",
        "recipe-mis-bound-gravy",
        "inv-tomato-mismatch",
        250_000_000,
    );

    let outcome =
        resolve_recipe_for_variant(db.connection(), Some(&variant_id), 1).expect("no DbError");
    assert_eq!(outcome, ResolveOutcome::Gap(GapReason::DimensionMismatch));
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
    // SQLite to prove the EDGE'S OWN backstop, independent of that cloud
    // check, actually stops it.
    let db = Db::open_in_memory_for_tests().expect("open db");
    seed_outlet_and_category(&db);
    let variant_a = seed_menu_item_with_variant(&db, "item-a", "variant-a", "A (internal)");
    let variant_b = seed_menu_item_with_variant(&db, "item-b", "variant-b", "B (internal)");
    let variant_c = seed_menu_item_with_variant(&db, "item-c", "variant-c", "C (internal)");

    insert_one_serving_recipe(db.connection(), "recipe-a", &variant_a, "A");
    insert_one_serving_recipe(db.connection(), "recipe-b", &variant_b, "B");
    insert_one_serving_recipe(db.connection(), "recipe-c", &variant_c, "C");

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

    // FALSIFIED (not merely asserted), RE-VERIFIED against the 0.5.1
    // output-quantity formula: with `path.contains(&sub_id)` temporarily
    // disabled, this exact fixture was rerun and produced
    // `Gap(DepthExceeded)` instead of `Gap(Cycle)` -- the walk still
    // terminated, but only because `MAX_RECIPE_DEPTH` independently caught
    // it after wasted recursion around the loop, proving the cycle guard
    // is not redundant with the depth guard even under the new formula
    // (the guard runs before any output-quantity arithmetic, so its own
    // logic is unchanged by that rewrite). Restored before this file was
    // finalised.
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
        insert_one_serving_recipe(db.connection(), &recipe_id, &variant_id, &format!("D{level}"));
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

    // FALSIFIED (not merely asserted), RE-VERIFIED against the 0.5.1
    // output-quantity formula: with `walk`'s depth check temporarily
    // disabled (`if false && path.len() ...`), this exact fixture was
    // rerun and it resolved successfully instead of gapping --
    // `ResolveOutcome::Resolved(..)` with `applied_micro: 1_000_000` on
    // the deep leaf -- confirming the walk really does reach the bottom of
    // an over-deep chain when nothing stops it, under the new formula too
    // (same `path.len()` check, evaluated before any output-quantity
    // arithmetic). Restored before this file was finalised.
    assert_eq!(outcome, ResolveOutcome::Gap(GapReason::DepthExceeded));
}
