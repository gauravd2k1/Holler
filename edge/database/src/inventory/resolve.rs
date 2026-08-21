//! Recipe resolution (ADR-018 §5, §6, §7): sub-recipes resolved transitively
//! to their leaves, exact-rational through the tree, rounded exactly once.
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

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension};

use crate::error::DbError;

use super::rational::Rational;
use super::units::Dimension;

/// Sub-recipe nesting bound, independent of the cloud's own DFS cycle check
/// at write time (ADR-018 §7). Mirrors `MaxRecipeDepth` in
/// `packages/contracts/go/inventory.go`.
pub const MAX_RECIPE_DEPTH: u32 = 8;

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
    /// this), or names an `inventory_item_id` this database has no row
    /// for: config arriving out of order, or a partially-synced catalogue.
    UnknownUnit,
}

impl GapReason {
    /// The exact string `stock_deduction_gap.reason` stores (ADR-018 §10.1
    /// / the task brief's naming, verbatim).
    pub fn as_str(self) -> &'static str {
        match self {
            GapReason::NoVariant => "NO_VARIANT",
            GapReason::NoRecipe => "NO_RECIPE",
            GapReason::Cycle => "CYCLE",
            GapReason::DepthExceeded => "DEPTH_EXCEEDED",
            GapReason::UnknownUnit => "UNKNOWN_UNIT",
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
}

struct IngredientRow {
    component_kind: String,
    inventory_item_id: Option<String>,
    sub_recipe_id: Option<String>,
    quantity_micro: i64,
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
        "SELECT id, name, recipe_version FROM recipe WHERE menu_item_variant_id = ?1",
        [variant_id],
        |row| {
            Ok(RecipeRow {
                id: row.get(0)?,
                name: row.get(1)?,
                version: row.get(2)?,
            })
        },
    )
    .optional()
    .map_err(DbError::from)
}

fn fetch_recipe_by_id(conn: &Connection, recipe_id: &str) -> Result<Option<RecipeRow>, DbError> {
    conn.query_row(
        "SELECT id, name, recipe_version FROM recipe WHERE id = ?1",
        [recipe_id],
        |row| {
            Ok(RecipeRow {
                id: row.get(0)?,
                name: row.get(1)?,
                version: row.get(2)?,
            })
        },
    )
    .optional()
    .map_err(DbError::from)
}

fn fetch_ingredients(conn: &Connection, recipe_id: &str) -> Result<Vec<IngredientRow>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT component_kind, inventory_item_id, sub_recipe_id, quantity_micro \
         FROM recipe_ingredient WHERE recipe_id = ?1 ORDER BY sort_order, id",
    )?;
    let rows = stmt
        .query_map([recipe_id], |row| {
            Ok(IngredientRow {
                component_kind: row.get(0)?,
                inventory_item_id: row.get(1)?,
                sub_recipe_id: row.get(2)?,
                quantity_micro: row.get(3)?,
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
    Ok(row.and_then(|(name, dim_str)| Dimension::parse(&dim_str).map(|dimension| ItemRow { name, dimension })))
}

/// Resolves a sold quantity of a menu item variant to its leaf-level
/// inventory deductions, walking sub-recipes transitively (ADR-018 §7).
///
/// `menu_item_variant_id` is `Option` because the caller's order line may
/// genuinely have none (§2.1's soft spot) — that is `GapReason::NoVariant`,
/// not a panic and not a `DbError`. `quantity` is the count of that variant
/// sold on the line (a plain integer, e.g. `2` for two plates), not a
/// micro-quantity itself.
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

    let mut accum: HashMap<String, (Rational, String, Dimension)> = HashMap::new();
    let mut path: Vec<String> = vec![root.id.clone()];
    let root_multiplier = Rational::from_int(quantity as i128);

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

/// Depth-first walk of one recipe's ingredient list, accumulating exact
/// leaf contributions into `accum` and recursing into sub-recipes.
/// `multiplier` is the exact-rational "how many times does this recipe
/// execute" factor accumulated from the root: at the root it is the order
/// line's plain integer quantity; a `SUB_RECIPE` component's own
/// `quantity_micro` (a micro-scaled multiplier — `1_000_000` means "once",
/// matching the micro-unit convention every other quantity in this schema
/// uses) further scales it on the way down, so
/// `applied_micro = round_half_away_from_zero(recipe_qty × line_qty × pack_ratio × …)`
/// (ADR-018 §5) falls out of repeated exact multiplication with no
/// intermediate rounding.
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
                    return Ok(Some(GapReason::UnknownUnit));
                };
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
                // quantity_micro is a MICRO-SCALED MULTIPLIER for a
                // sub-recipe component (1_000_000 == execute the sub-recipe
                // exactly once), not an absolute leaf quantity — dividing
                // by 1_000_000 turns it back into a plain rational factor
                // before it chains onto the running multiplier.
                let Some(sub_multiplier) =
                    multiplier.checked_mul_ratio(ingredient.quantity_micro as i128, 1_000_000)
                else {
                    return Ok(Some(GapReason::DepthExceeded));
                };
                path.push(sub_recipe.id.clone());
                let gap = walk(conn, &sub_recipe.id, sub_multiplier, path, accum)?;
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
