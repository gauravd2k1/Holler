//! Recipe resolution (ADR-018 §5, §6, §7; contracts 0.5.1 `recipe.output_*`
//! addendum, `packages/contracts/sqlite/0019_recipe_output.sql`):
//! sub-recipes resolved transitively to their leaves, exact-rational through
//! the tree, rounded exactly once.
//!
//! **One formula, at every level:**
//! `multiplier = requested_quantity / recipe.output_quantity_micro`.
//! `requested_quantity` is an ABSOLUTE amount of the recipe's own declared
//! output (`recipe.output_dimension`), never a dimensionless execution
//! count. At the root, the order requests `line_qty` SERVINGS — an absolute
//! COUNT quantity of `line_qty × 1_000_000` micro-pieces — against
//! whatever `output_quantity_micro` the root recipe itself declares one
//! execution to produce; for a `SUB_RECIPE` component, the request is the
//! parent's own `quantity_micro` on that ingredient row, interpreted as an
//! absolute amount of the CHILD's declared output. There is no special case
//! for the root and no separate "micro-scaled multiplier" convention for a
//! sub-recipe row — 0.5.0 had one (`quantity_micro / 1_000_000` on a
//! `SUB_RECIPE` row meant "execute this many times"), and it was rejected:
//! under that reading, editing a sub-recipe's own yield silently rescales
//! every recipe that references it, with no error, until a physical count
//! catches the variance. `output_quantity_micro` fixes the deduction to the
//! ABSOLUTE quantity the parent actually asked for; a re-yielded sub-recipe
//! changes only its own `output_quantity_micro` and the ratio recomputes
//! correctly the very next resolution.
//!
//! **A broken recipe never fails a confirm.** [`resolve_recipe_for_variant`]
//! reports "this could not be resolved, and why" ([`GapReason`]) as an
//! ordinary `Ok(ResolveOutcome::Gap(_))`, not as a [`DbError`] that would
//! abort the caller's transaction. `DbError` is reserved for a genuine
//! SQLite failure (a real read error) — never for "no recipe exists" or "the
//! graph cycles".
//!
//! **Cycle and depth guards are an independent backstop**, not a trust of
//! the cloud's own write-time check (ADR-018 §7 Level 2). This resolver runs
//! inside `confirm_order`'s transaction, on the outlet's only SQLite writer:
//! an unbounded walk does not produce a wrong number, it wedges the till
//! mid-service. `path` carries the current DFS path (recipe ids from the
//! root down); a sub-recipe id already on that path is a cycle, caught
//! *before* recursing into it, so the walk never even attempts the loop.
//! `MAX_RECIPE_DEPTH` bounds nesting depth independently of the cycle check,
//! so even a long acyclic chain (which a real menu will never produce, but a
//! restored backup or an older writer might) cannot recurse unboundedly.
//!
//! **`DIMENSION_MISMATCH` — three independent checks, not one.**
//!
//! 1. **The root.** An order always requests a COUNT quantity (servings
//!    sold), regardless of what the resolved recipe declares, so a root
//!    recipe whose `output_dimension` is not `COUNT` is a genuine authoring
//!    defect — e.g. a recipe meant only as a sub-recipe component (a gravy,
//!    `VOLUME`) mistakenly bound to a sellable `menu_item_variant`.
//! 2. **Every `ITEM` row** (contracts 0.5.2, `recipe_ingredient
//!    .quantity_dimension`, `packages/contracts/sqlite/
//!    0020_recipe_ingredient_dimension.sql`). Before 0.5.2 a stored
//!    `quantity_micro` was dimensionless — 220_000_000 meant grams only
//!    because the referenced item happened to declare MASS, so reclassifying
//!    that item silently reinterpreted every recipe that used it. 0.5.2
//!    added the author's own recorded unit; `walk` compares it against the
//!    referenced `inventory_item.dimension` at resolution time. **Never
//!    derived from the referent** — deriving it would make the comparison
//!    `x == x` and the guard could never fire, the exact defect 0.5.2's
//!    migration header warns every future writer against.
//! 3. **Every `SUB_RECIPE` row**, same column, same check, against the
//!    referenced recipe's `output_dimension` instead of an item's
//!    `dimension` — the postgres/0021 trigger's `ELSE` branch, mirrored here
//!    defensively. Before 0.5.2 this arm was tautologically consistent (see
//!    the git history of this comment): `recipe_ingredient` carried no
//!    dimension tag of its own, so a `SUB_RECIPE` row's implicit dimension
//!    *was* whatever the child currently declared, by construction, and no
//!    drift could ever be observed from stored data alone. 0.5.2 gives the
//!    row an independent opinion to compare against.
//!
//! In every case: no cross-dimension conversion is attempted (a recipe has
//! no `item_unit_conversion` density row, and converting an item's own
//! author-chosen unit would defeat the point of recording it) — a mismatch
//! is reported, never coerced, exactly like a cycle.

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension};

use crate::error::DbError;

use super::rational::Rational;
use super::units::Dimension;

/// Sub-recipe nesting bound, independent of the cloud's own DFS cycle check
/// at write time (ADR-018 §7). Mirrors `MaxRecipeDepth` in
/// `packages/contracts/go/inventory.go`.
pub const MAX_RECIPE_DEPTH: u32 = 8;

/// One canonical serving, in COUNT micro-units — what an order line's plain
/// integer `quantity` is scaled by to become the root's `requested_quantity`
/// (ADR-018 addendum: "the request is line_qty × 1 serving").
const ONE_SERVING_MICRO: i128 = 1_000_000;

/// Every reason a recipe resolution can fail to produce a deduction,
/// reported rather than raised. `T2` turns this into a `stock_deduction_gap`
/// row and lets the sale complete (ADR-018 Rule 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapReason {
    /// The order line carries no `menu_item_variant_id` to resolve a recipe
    /// against — the ADR-018 §2.1 soft spot: "every menu item has at least
    /// one variant" is a cross-row invariant no `CHECK` can express, and a
    /// line that somehow reaches deduction with a null variant lands here
    /// rather than failing the confirm.
    NoVariant,
    /// No `recipe` row exists for this variant (or, mid-tree, a
    /// `recipe_ingredient.sub_recipe_id` names a recipe id that no longer
    /// exists — a dangling reference is a config gap, not a cycle).
    NoRecipe,
    /// A `sub_recipe_id` reachable from the root recipe reaches back to a
    /// recipe already on the current path. Caught by the resolver's own
    /// visited-set, independent of the cloud's write-time DFS check.
    Cycle,
    /// The walk exceeded `MAX_RECIPE_DEPTH` levels of sub-recipe nesting,
    /// or an intermediate rational accumulation overflowed `i128` — the
    /// same defensive family: a graph too extreme to resolve safely inside
    /// a transaction that must not fail.
    DepthExceeded,
    /// A component row is missing the reference its `component_kind`
    /// requires (defensively — the schema's own `CHECK` should prevent
    /// this). Also covers a recipe whose own `output_quantity_micro` is not
    /// positive — the schema's own `CHECK` should prevent this too, but
    /// this resolver never trusts a constraint that may have been written
    /// by an older schema version. **Not** used for a dangling reference —
    /// see [`GapReason::UnresolvableReference`] (contracts 0.5.3).
    UnknownUnit,
    /// The order's implicit request (a COUNT quantity — servings sold) does
    /// not match the resolved root recipe's own `output_dimension`. No
    /// cross-dimension conversion exists for a recipe (unlike an
    /// `inventory_item`, it has no `item_unit_conversion` density row to
    /// convert through) — contracts 0.5.1.
    DimensionMismatch,
    /// An `ITEM` component names an `inventory_item_id` this database has no
    /// row for: config arriving out of order, or a partially-synced
    /// catalogue. Contracts 0.5.3 — added because the prior classification
    /// (`UnknownUnit`) approximated a dangling reference instead of naming
    /// it, and "a wrong reason code in an append-only table is as unfixable
    /// as a wrong quantity" (ADR-018 0.5.3 addendum). Deliberately distinct
    /// from `NoRecipe`, which is a dangling reference to a `recipe` row
    /// rather than an `inventory_item` row — the Go/TS enum comment reads
    /// "a delta OR INGREDIENT referencing an item that is not there",
    /// naming both siblings this variant now covers on the edge side.
    UnresolvableReference,
}

impl GapReason {
    /// The exact string `stock_deduction_gap.reason` stores (ADR-018 §10.1
    /// / the task brief's naming, verbatim; `DIMENSION_MISMATCH` per the
    /// contracts 0.5.1 `StockDeductionGapReason` enum member).
    pub fn as_str(self) -> &'static str {
        match self {
            GapReason::NoVariant => "NO_VARIANT",
            GapReason::NoRecipe => "NO_RECIPE",
            GapReason::Cycle => "CYCLE",
            GapReason::DepthExceeded => "DEPTH_EXCEEDED",
            GapReason::UnknownUnit => "UNKNOWN_UNIT",
            GapReason::DimensionMismatch => "DIMENSION_MISMATCH",
            GapReason::UnresolvableReference => "UNRESOLVABLE_REFERENCE",
        }
    }
}

/// One resolved leaf deduction: the exact-rational sum across every branch
/// of the tree that touched this `inventory_item_id`, rounded exactly once
/// (ADR-018 §5). `dimension` and `inventory_item_name` are snapshotted here
/// so `T2` can write a self-describing `stock_ledger_entry` (ADR-018 §6)
/// without a second lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLeaf {
    pub inventory_item_id: String,
    pub inventory_item_name: String,
    pub dimension: Dimension,
    /// The quantity actually applied, in the item's own dimension's
    /// micro-units. Signed in principle (a resolver reused for a modifier
    /// delta could produce a negative value); recipe resolution itself only
    /// ever produces non-negative values, since `recipe_ingredient
    /// .quantity_micro` is `CHECK (> 0)`.
    pub applied_micro: i64,
}

/// The root recipe's provenance plus every leaf it resolved to. `recipe_id`
/// / `recipe_version` / `recipe_name` are what `T2` snapshots onto every
/// `stock_ledger_entry` this resolution produces (ADR-018 §6) — the *root*
/// recipe's identity, not any sub-recipe's, since a ledger entry explains
/// "which sale produced this row", not "which sub-recipe leaf it came
/// through".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeResolution {
    pub recipe_id: String,
    pub recipe_name: String,
    pub recipe_version: i64,
    pub leaves: Vec<ResolvedLeaf>,
}

/// The outcome of a resolution attempt — never a [`DbError`] for a business
/// reason, only for a genuine read failure (ADR-018 Rule 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveOutcome {
    Resolved(RecipeResolution),
    Gap(GapReason),
}

struct RecipeRow {
    id: String,
    name: String,
    version: i64,
    /// `recipe.output_dimension` (contracts 0.5.1) — what ONE row's
    /// `output_quantity_micro` is denominated in.
    output_dimension: String,
    /// `recipe.output_quantity_micro` (contracts 0.5.1) — what one
    /// execution of this recipe's own ingredient list produces, in
    /// `output_dimension`'s canonical micro-units. `CHECK (> 0)`.
    output_quantity_micro: i64,
}

struct IngredientRow {
    component_kind: String,
    inventory_item_id: Option<String>,
    sub_recipe_id: Option<String>,
    quantity_micro: i64,
    /// `recipe_ingredient.quantity_dimension` (contracts 0.5.2, ADR-018
    /// addendum) — the unit the AUTHOR chose, never derived from the
    /// referent. Compared against the referent's own dimension at
    /// resolution time (`walk`, below); the edge deliberately carries no
    /// trigger enforcing agreement (only the cloud does, at write time), so
    /// this stored value and the referent's current dimension can
    /// legitimately disagree — that disagreement is exactly what
    /// `GapReason::DimensionMismatch` reports.
    quantity_dimension: String,
}

struct ItemRow {
    name: String,
    dimension: Dimension,
}

fn fetch_recipe_by_variant(
    conn: &Connection,
    variant_id: &str,
) -> Result<Option<RecipeRow>, DbError> {
    conn.query_row(
        "SELECT id, name, recipe_version, output_dimension, output_quantity_micro \
         FROM recipe WHERE menu_item_variant_id = ?1",
        [variant_id],
        |row| {
            Ok(RecipeRow {
                id: row.get(0)?,
                name: row.get(1)?,
                version: row.get(2)?,
                output_dimension: row.get(3)?,
                output_quantity_micro: row.get(4)?,
            })
        },
    )
    .optional()
    .map_err(DbError::from)
}

fn fetch_recipe_by_id(conn: &Connection, recipe_id: &str) -> Result<Option<RecipeRow>, DbError> {
    conn.query_row(
        "SELECT id, name, recipe_version, output_dimension, output_quantity_micro \
         FROM recipe WHERE id = ?1",
        [recipe_id],
        |row| {
            Ok(RecipeRow {
                id: row.get(0)?,
                name: row.get(1)?,
                version: row.get(2)?,
                output_dimension: row.get(3)?,
                output_quantity_micro: row.get(4)?,
            })
        },
    )
    .optional()
    .map_err(DbError::from)
}

fn fetch_ingredients(conn: &Connection, recipe_id: &str) -> Result<Vec<IngredientRow>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT component_kind, inventory_item_id, sub_recipe_id, quantity_micro, quantity_dimension \
         FROM recipe_ingredient WHERE recipe_id = ?1 ORDER BY sort_order, id",
    )?;
    let rows = stmt
        .query_map([recipe_id], |row| {
            Ok(IngredientRow {
                component_kind: row.get(0)?,
                inventory_item_id: row.get(1)?,
                sub_recipe_id: row.get(2)?,
                quantity_micro: row.get(3)?,
                quantity_dimension: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn fetch_item(conn: &Connection, item_id: &str) -> Result<Option<ItemRow>, DbError> {
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT name, dimension FROM inventory_item WHERE id = ?1",
            [item_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(row.and_then(|(name, dim_str)| {
        Dimension::parse(&dim_str).map(|dimension| ItemRow { name, dimension })
    }))
}

/// Resolves a sold quantity of a menu item variant to its leaf-level
/// inventory deductions, walking sub-recipes transitively (ADR-018 §7).
///
/// `menu_item_variant_id` is `Option` because the caller's order line may
/// genuinely have none (§2.1's soft spot) — that is `GapReason::NoVariant`,
/// not a panic and not a `DbError`. `quantity` is the count of SERVINGS
/// requested on the line (e.g. `2` for two plates of a normal 1-serving
/// dish; for a menu item whose recipe declares `output_quantity_micro`
/// covering more than one serving — a sharing platter — `quantity` is still
/// a serving count, not a count of platters: see the module doc comment's
/// "one formula, every level").
pub fn resolve_recipe_for_variant(
    conn: &Connection,
    menu_item_variant_id: Option<&str>,
    quantity: i64,
) -> Result<ResolveOutcome, DbError> {
    let Some(variant_id) = menu_item_variant_id.filter(|v| !v.trim().is_empty()) else {
        return Ok(ResolveOutcome::Gap(GapReason::NoVariant));
    };

    let Some(root) = fetch_recipe_by_variant(conn, variant_id)? else {
        return Ok(ResolveOutcome::Gap(GapReason::NoRecipe));
    };

    // THE ONE INDEPENDENTLY-CHECKABLE DIMENSION MISMATCH (module doc
    // comment): an order always requests a COUNT quantity (servings sold),
    // regardless of what the resolved recipe declares.
    if Dimension::parse(&root.output_dimension) != Some(Dimension::Count) {
        return Ok(ResolveOutcome::Gap(GapReason::DimensionMismatch));
    }
    let Some(root_multiplier) = safe_multiplier(
        Rational::from_int(quantity as i128 * ONE_SERVING_MICRO),
        root.output_quantity_micro,
    ) else {
        return Ok(ResolveOutcome::Gap(GapReason::UnknownUnit));
    };

    let mut accum: HashMap<String, (Rational, String, Dimension)> = HashMap::new();
    let mut path: Vec<String> = vec![root.id.clone()];

    if let Some(gap) = walk(conn, &root.id, root_multiplier, &mut path, &mut accum)? {
        return Ok(ResolveOutcome::Gap(gap));
    }

    let mut leaves: Vec<ResolvedLeaf> = Vec::with_capacity(accum.len());
    for (item_id, (sum, name, dimension)) in accum {
        let rounded = sum.round_half_away_from_zero();
        let Ok(applied_micro) = i64::try_from(rounded) else {
            // Outside i64 range: the ADR-018 §3 safe-integer ceiling is
            // JavaScript's 2^53, five orders of magnitude below i64::MAX,
            // so a legitimate recipe can never reach here. Only a
            // pathological graph could, and it belongs with this
            // resolver's other defensive overflow outcomes.
            return Ok(ResolveOutcome::Gap(GapReason::DepthExceeded));
        };
        leaves.push(ResolvedLeaf {
            inventory_item_id: item_id,
            inventory_item_name: name,
            dimension,
            applied_micro,
        });
    }
    // Deterministic ordering: two resolutions of the same recipe against
    // the same DB must produce the same leaf order, since T2 will persist
    // ledger rows in this order.
    leaves.sort_by(|a, b| a.inventory_item_id.cmp(&b.inventory_item_id));

    Ok(ResolveOutcome::Resolved(RecipeResolution {
        recipe_id: root.id,
        recipe_name: root.name,
        recipe_version: root.version,
        leaves,
    }))
}

/// `requested / output_quantity_micro`, as an exact rational — the ONE
/// formula this module applies at every level (module doc comment).
/// `None` if `output_quantity_micro` is not positive (defensive: the
/// schema's own `CHECK (output_quantity_micro > 0)` should make this
/// impossible, but this resolver never trusts a constraint that may have
/// been written by an older schema version) or on `i128` overflow.
fn safe_multiplier(requested: Rational, output_quantity_micro: i64) -> Option<Rational> {
    if output_quantity_micro <= 0 {
        return None;
    }
    requested.checked_mul_ratio(1, output_quantity_micro as i128)
}

/// Depth-first walk of one recipe's ingredient list, accumulating exact
/// leaf contributions into `accum` and recursing into sub-recipes.
/// `multiplier` is the exact-rational "how many times does this recipe's
/// own ingredient list apply" factor for THIS recipe — already computed by
/// the caller as `requested_quantity / this_recipe.output_quantity_micro`
/// (the module doc comment's "one formula, every level"; see
/// [`resolve_recipe_for_variant`] for the root and the `SUB_RECIPE` arm
/// below for every subsequent level). Every ingredient row's own
/// `quantity_micro` is an amount **per one execution's worth of THIS
/// recipe's output** — for an `ITEM` component, an absolute amount of the
/// leaf item's own dimension; for a `SUB_RECIPE` component, an absolute
/// amount of the CHILD's own declared output — so `multiplier ×
/// quantity_micro` is always a well-formed absolute quantity, chained
/// through the tree as an exact rational with no intermediate rounding.
///
/// Returns `Ok(Some(gap))` the instant any gap condition is found —
/// short-circuiting the walk rather than continuing to accumulate against a
/// tree already known to be unresolvable. Returns `Ok(None)` on a clean
/// walk. A real `DbError` (an actual SQLite failure) still propagates via
/// `?`, distinct from every `GapReason`.
fn walk(
    conn: &Connection,
    recipe_id: &str,
    multiplier: Rational,
    path: &mut Vec<String>,
    accum: &mut HashMap<String, (Rational, String, Dimension)>,
) -> Result<Option<GapReason>, DbError> {
    if path.len() as u32 > MAX_RECIPE_DEPTH {
        return Ok(Some(GapReason::DepthExceeded));
    }

    for ingredient in fetch_ingredients(conn, recipe_id)? {
        match ingredient.component_kind.as_str() {
            "ITEM" => {
                let Some(item_id) = ingredient.inventory_item_id else {
                    return Ok(Some(GapReason::UnknownUnit));
                };
                let Some(item) = fetch_item(conn, &item_id)? else {
                    // Dangling reference: contracts 0.5.3, UnresolvableReference
                    // — not UnknownUnit. A wrong reason code in an append-only
                    // table is as unfixable as a wrong quantity.
                    return Ok(Some(GapReason::UnresolvableReference));
                };
                // contracts 0.5.2: the AUTHOR's recorded unit must still
                // match what this row actually points at. Never derived
                // from `item.dimension` here — that would make the
                // comparison `x == x` and the guard could never fire (the
                // exact defect the 0.5.2 migration header warns against).
                if Dimension::parse(&ingredient.quantity_dimension) != Some(item.dimension) {
                    return Ok(Some(GapReason::DimensionMismatch));
                }
                let Some(contribution) =
                    multiplier.checked_mul_ratio(ingredient.quantity_micro as i128, 1)
                else {
                    return Ok(Some(GapReason::DepthExceeded));
                };
                let entry = accum
                    .entry(item_id)
                    .or_insert_with(|| (Rational::zero(), item.name, item.dimension));
                let Some(new_sum) = entry.0.checked_add(contribution) else {
                    return Ok(Some(GapReason::DepthExceeded));
                };
                entry.0 = new_sum;
            }
            "SUB_RECIPE" => {
                let Some(sub_id) = ingredient.sub_recipe_id else {
                    return Ok(Some(GapReason::UnknownUnit));
                };
                // THE CYCLE GUARD. Checked BEFORE recursing, against the
                // current DFS path from the root — the resolver's own
                // visited-set, independent of the cloud's write-time check
                // (ADR-018 §7 Level 3). A cycle is caught in O(depth) time,
                // never attempted.
                if path.contains(&sub_id) {
                    return Ok(Some(GapReason::Cycle));
                }
                let Some(sub_recipe) = fetch_recipe_by_id(conn, &sub_id)? else {
                    // Dangling reference: config gap, not a cycle.
                    return Ok(Some(GapReason::NoRecipe));
                };
                // contracts 0.5.2: same check as the ITEM arm, against the
                // referenced recipe's `output_dimension` rather than an
                // inventory item's `dimension` — the postgres/0021 trigger's
                // ELSE branch, mirrored defensively at the edge.
                if Dimension::parse(&ingredient.quantity_dimension)
                    != Dimension::parse(&sub_recipe.output_dimension)
                {
                    return Ok(Some(GapReason::DimensionMismatch));
                }
                // `ingredient.quantity_micro` is an ABSOLUTE quantity of the
                // CHILD's own declared output (contracts 0.5.1) — "180 ml of
                // Makhani Gravy" — never a dimensionless execution count.
                // Scaled by this recipe's own multiplier first (still an
                // absolute request, in the child's canonical unit — module
                // doc comment on why no per-row dimension tag is needed:
                // the request is tautologically in the child's own
                // currently-declared dimension), THEN divided by the
                // child's own `output_quantity_micro` to become the
                // child's multiplier. Same formula as the root, no special
                // case.
                let Some(requested_of_child) =
                    multiplier.checked_mul_ratio(ingredient.quantity_micro as i128, 1)
                else {
                    return Ok(Some(GapReason::DepthExceeded));
                };
                let Some(child_multiplier) =
                    safe_multiplier(requested_of_child, sub_recipe.output_quantity_micro)
                else {
                    return Ok(Some(GapReason::UnknownUnit));
                };
                path.push(sub_recipe.id.clone());
                let gap = walk(conn, &sub_recipe.id, child_multiplier, path, accum)?;
                path.pop();
                if let Some(gap) = gap {
                    return Ok(Some(gap));
                }
            }
            _ => {
                // The schema's own CHECK forbids any other component_kind.
                // This resolver never trusts a constraint from a possibly-
                // older writer, per this module's doc comment.
                return Ok(Some(GapReason::UnknownUnit));
            }
        }
    }
    Ok(None)
}
