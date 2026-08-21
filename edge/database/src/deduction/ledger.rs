//! Writes `stock_ledger_entry` / `stock_deduction_gap` rows from a resolved
//! recipe, and the single entry point [`deduct_stock_for_confirmed_order`]
//! that [`crate::Db::confirm_order_with_outbox`] calls inside its own
//! transaction (Milestone 4, track T2, ADR-018).
//!
//! ============================================================================
//! THE RULE THIS FILE EXISTS TO ENFORCE
//! ============================================================================
//!
//! **This function must never be able to abort `confirm_order`'s transaction
//! over a business/config reason.** [`crate::inventory::resolve_recipe_for_variant`]
//! already reports "could not resolve, and why" as `Ok(Gap(_))`, never a
//! `DbError`; this module keeps that property all the way through by turning
//! every gap into a `stock_deduction_gap` INSERT rather than a `?`-propagated
//! error. A genuine `DbError` (an actual SQLite failure — a real read/write
//! error) still propagates via `?` exactly like every other repo call in this
//! crate; that is the one thing this module does NOT try to swallow, per the
//! task brief.
//!
//! **Stock never blocks a sale.** No balance check exists anywhere below.
//! Negative stock is a variance signal, not an error (ADR-018 Rule 1) — if a
//! future change to this file adds one, it is the bug, not a missing
//! feature.

use rusqlite::{params, Transaction};

use crate::error::DbResult;
use crate::inventory::{resolve_recipe_for_variant, GapReason, ResolveOutcome};
use crate::model::{NewStockDeductionGap, NewStockLedgerEntry, OrderItem};
use crate::repo;

use super::business_date::compute_business_date;

/// The highest `entry_seq` already used at this outlet, or `0` if none.
/// Read once per [`deduct_stock_for_confirmed_order`] call and advanced
/// locally for every row that call writes — a single query rather than one
/// `MAX` per row, which matters given the volume this table is designed for
/// (ADR-018: ~15,000 rows/day). Safe without a `SELECT ... FOR UPDATE`-style
/// lock because ADR-018 Rule 3 holds: the edge is a single SQLite writer, and
/// this function runs inside the caller's transaction on that one writer, so
/// no concurrent insert can observe or advance this value between the read
/// and this call's own inserts.
///
/// The `UNIQUE (outlet_id, entry_seq)` index this table already carries
/// makes `outlet_id, entry_seq` an efficient seek for `MAX`, so this is not
/// a full table scan even as the table grows into the millions of rows.
fn max_entry_seq(tx: &Transaction, outlet_id: &str) -> DbResult<i64> {
    tx.query_row(
        "SELECT COALESCE(MAX(entry_seq), 0) FROM stock_ledger_entry WHERE outlet_id = ?1",
        params![outlet_id],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

pub(crate) fn insert_stock_ledger_entry(
    tx: &Transaction,
    id: &str,
    entry_seq: i64,
    e: &NewStockLedgerEntry,
) -> DbResult<()> {
    tx.execute(
        "INSERT INTO stock_ledger_entry
            (id, outlet_id, entry_seq, inventory_item_id, inventory_item_name, dimension,
             entry_type, origin, quantity_applied_micro, recipe_id, recipe_version, recipe_name,
             source_order_id, source_order_item_id, reason_code, note, occurred_at, business_date,
             created_by_user_id, modifier_delta_id, modifier_name, modifier_delta_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18,
                 ?19, ?20, ?21, ?22)",
        params![
            id,
            e.outlet_id,
            entry_seq,
            e.inventory_item_id,
            e.inventory_item_name,
            e.dimension,
            e.entry_type,
            e.origin,
            e.quantity_applied_micro,
            e.recipe_id,
            e.recipe_version,
            e.recipe_name,
            e.source_order_id,
            e.source_order_item_id,
            e.reason_code,
            e.note,
            e.occurred_at,
            e.business_date,
            e.created_by_user_id,
            e.modifier_delta_id,
            e.modifier_name,
            e.modifier_delta_version,
        ],
    )?;
    Ok(())
}

pub(crate) fn insert_stock_deduction_gap(
    tx: &Transaction,
    id: &str,
    g: &NewStockDeductionGap,
) -> DbResult<()> {
    tx.execute(
        "INSERT INTO stock_deduction_gap
            (id, outlet_id, order_id, order_item_id, menu_item_id, menu_item_variant_id,
             menu_item_name, quantity, reason, occurred_at, business_date)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            id,
            g.outlet_id,
            g.order_id,
            g.order_item_id,
            g.menu_item_id,
            g.menu_item_variant_id,
            g.menu_item_name,
            g.quantity,
            g.reason,
            g.occurred_at,
            g.business_date,
        ],
    )?;
    Ok(())
}

fn menu_item_name(tx: &Transaction, menu_item_id: &str) -> DbResult<String> {
    tx.query_row(
        "SELECT name FROM menu_item WHERE id = ?1",
        params![menu_item_id],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

struct ModifierDeltaRow {
    id: String,
    inventory_item_id: String,
    quantity_micro: i64,
    config_version: i64,
}

fn fetch_modifier_deltas(
    tx: &Transaction,
    menu_item_modifier_id: &str,
) -> DbResult<Vec<ModifierDeltaRow>> {
    let mut stmt = tx.prepare(
        "SELECT id, inventory_item_id, quantity_micro, config_version \
         FROM modifier_ingredient_delta WHERE menu_item_modifier_id = ?1",
    )?;
    let rows = stmt
        .query_map(params![menu_item_modifier_id], |row| {
            Ok(ModifierDeltaRow {
                id: row.get(0)?,
                inventory_item_id: row.get(1)?,
                quantity_micro: row.get(2)?,
                config_version: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

struct InventoryItemRow {
    name: String,
    dimension: String,
}

fn fetch_inventory_item(
    tx: &Transaction,
    inventory_item_id: &str,
) -> DbResult<Option<InventoryItemRow>> {
    use rusqlite::OptionalExtension;
    tx.query_row(
        "SELECT name, dimension FROM inventory_item WHERE id = ?1",
        params![inventory_item_id],
        |row| {
            Ok(InventoryItemRow {
                name: row.get(0)?,
                dimension: row.get(1)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

/// Runs the recipe/modifier deduction for one just-confirmed order, writing
/// `stock_ledger_entry` rows for everything that resolves and
/// `stock_deduction_gap` rows for everything that does not — all inside
/// `tx`, the SAME transaction [`crate::Db::confirm_order_with_outbox`] uses
/// for the order's own status change (ADR-018 Rule 3: deduction needs no
/// lock of its own because the edge is a single SQLite writer).
///
/// `occurred_at_utc` is the moment the edge recorded the confirmation
/// (mirrors `OrderConfirmedMeta::confirmed_at`) — used both as every written
/// row's `occurred_at` and as the instant `business_date` is computed from,
/// once, for the whole order (every leaf of every line in one confirm shares
/// one `business_date`, which is correct: they are one sale, one moment).
pub(crate) fn deduct_stock_for_confirmed_order(
    tx: &Transaction,
    outlet_id: &str,
    order_id: &str,
    occurred_at_utc: &str,
) -> DbResult<()> {
    let (timezone, day_start_time) = repo::get_outlet_business_date_config(tx, outlet_id)?;
    let business_date = compute_business_date(occurred_at_utc, &timezone, &day_start_time);

    let mut next_seq = max_entry_seq(tx, outlet_id)? + 1;
    let items: Vec<OrderItem> = repo::list_order_items_in_tx(tx, order_id)?;

    for item in &items {
        deduct_one_line(
            tx,
            outlet_id,
            order_id,
            item,
            occurred_at_utc,
            &business_date,
            &mut next_seq,
        )?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn deduct_one_line(
    tx: &Transaction,
    outlet_id: &str,
    order_id: &str,
    item: &OrderItem,
    occurred_at_utc: &str,
    business_date: &str,
    next_seq: &mut i64,
) -> DbResult<()> {
    match resolve_recipe_for_variant(tx, item.variant_id.as_deref(), item.quantity)? {
        ResolveOutcome::Resolved(resolution) => {
            for leaf in &resolution.leaves {
                let entry = NewStockLedgerEntry {
                    outlet_id: outlet_id.to_string(),
                    inventory_item_id: leaf.inventory_item_id.clone(),
                    inventory_item_name: leaf.inventory_item_name.clone(),
                    dimension: leaf.dimension.as_str().to_string(),
                    entry_type: "CONSUMPTION".to_string(),
                    origin: "RECIPE".to_string(),
                    // Consumption is negative (0016 header: "Consumption is
                    // negative, purchase positive"). The resolver's own
                    // `applied_micro` is always non-negative (it is a sum of
                    // positive `recipe_ingredient.quantity_micro` rows scaled
                    // by a non-negative multiplier), so negating it here is
                    // exact, not a magnitude assumption.
                    quantity_applied_micro: -leaf.applied_micro,
                    recipe_id: Some(resolution.recipe_id.clone()),
                    recipe_version: Some(resolution.recipe_version),
                    recipe_name: Some(resolution.recipe_name.clone()),
                    source_order_id: Some(order_id.to_string()),
                    source_order_item_id: Some(item.id.clone()),
                    reason_code: None,
                    note: None,
                    occurred_at: occurred_at_utc.to_string(),
                    business_date: business_date.to_string(),
                    created_by_user_id: None,
                    modifier_delta_id: None,
                    modifier_name: None,
                    modifier_delta_version: None,
                };
                let id = uuid::Uuid::now_v7().to_string();
                insert_stock_ledger_entry(tx, &id, *next_seq, &entry)?;
                *next_seq += 1;
            }
        }
        ResolveOutcome::Gap(reason) => {
            write_gap(tx, outlet_id, order_id, item, reason, occurred_at_utc, business_date)?;
        }
    }

    deduct_modifiers_for_line(tx, outlet_id, order_id, item, occurred_at_utc, business_date, next_seq)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_gap(
    tx: &Transaction,
    outlet_id: &str,
    order_id: &str,
    item: &OrderItem,
    reason: GapReason,
    occurred_at_utc: &str,
    business_date: &str,
) -> DbResult<()> {
    let name = menu_item_name(tx, &item.menu_item_id)?;
    let gap = NewStockDeductionGap {
        outlet_id: outlet_id.to_string(),
        order_id: order_id.to_string(),
        order_item_id: item.id.clone(),
        menu_item_id: item.menu_item_id.clone(),
        menu_item_variant_id: item.variant_id.clone(),
        menu_item_name: name,
        quantity: item.quantity,
        reason: reason.as_str().to_string(),
        occurred_at: occurred_at_utc.to_string(),
        business_date: business_date.to_string(),
    };
    let id = uuid::Uuid::now_v7().to_string();
    insert_stock_deduction_gap(tx, &id, &gap)
}

/// **Modifier deltas deduct too, and a modifier with no delta row deducts
/// nothing** — absence is never consent (ADR-018 §2, the `printer_role`
/// rule applied to ingredients). Each selected modifier on this line is
/// looked up in `modifier_ingredient_delta`; every row found there produces
/// one `MODIFIER_DELTA`-origin ledger entry, scaled by the LINE's quantity
/// (all units on one cart line share the same modifier selections — there is
/// no per-unit modifier granularity in this cart model, so "2x, extra
/// paneer" deducts extra paneer twice).
///
/// **No rounding here.** `modifier_ingredient_delta.quantity_micro` is
/// already an integer count of micro-units for ONE serving-worth of the
/// modifier; multiplying by the line's integer `quantity` is exact integer
/// arithmetic with nothing left to round — unlike recipe resolution, this
/// path never produces a non-terminating fraction, so ADR-018 §5's "round
/// once, at the leaf" has nothing to do here.
#[allow(clippy::too_many_arguments)]
fn deduct_modifiers_for_line(
    tx: &Transaction,
    outlet_id: &str,
    order_id: &str,
    item: &OrderItem,
    occurred_at_utc: &str,
    business_date: &str,
    next_seq: &mut i64,
) -> DbResult<()> {
    for modifier in repo::list_order_item_modifiers_in_tx(tx, &item.id)? {
        for delta in fetch_modifier_deltas(tx, &modifier.modifier_id)? {
            // i128 first so an astronomically large (and never realistic —
            // see ADR-018 §3's safe-integer headroom argument) product is
            // caught by the `i64::try_from` below rather than wrapping.
            // There is no `stock_deduction_gap` reason that names "modifier
            // arithmetic overflow" (the schema's CHECK lists NO_RECIPE /
            // NO_VARIANT / CYCLE / DEPTH_EXCEEDED / UNKNOWN_UNIT /
            // DIMENSION_MISMATCH, none of which fit), so this deliberately
            // skips writing a ledger row rather than mis-labelling the
            // cause with a reason that does not describe it — unreachable
            // in practice, since `quantity_micro` and line `quantity` are
            // both bounded far inside `i64` in any real modifier/cart shape.
            let Some(product) = i128::from(delta.quantity_micro)
                .checked_mul(i128::from(item.quantity))
                .and_then(|p| i64::try_from(p).ok())
            else {
                continue;
            };

            let Some(inv_item) = fetch_inventory_item(tx, &delta.inventory_item_id)? else {
                // Dangling reference (config arrived out of order, or a
                // partially-synced catalogue) — the same posture the
                // resolver takes toward a missing `inventory_item`
                // (`GapReason::UnknownUnit`). No named-reason gap exists for
                // "modifier delta pointed at a missing item" either, and
                // for the same containment reason as the overflow branch
                // above: skip this one delta row's deduction rather than
                // reporting a cause the schema cannot represent, and let
                // the rest of the line's deduction proceed normally.
                continue;
            };

            let entry = NewStockLedgerEntry {
                outlet_id: outlet_id.to_string(),
                inventory_item_id: delta.inventory_item_id.clone(),
                inventory_item_name: inv_item.name,
                dimension: inv_item.dimension,
                entry_type: "CONSUMPTION".to_string(),
                origin: "MODIFIER_DELTA".to_string(),
                // Same sign convention as the recipe arm: a positive delta
                // ("Extra Paneer") consumes more, so the ledger entry is
                // negative; a negative delta ("No Onion") consumes less, so
                // negating it yields a positive entry that gives stock back.
                quantity_applied_micro: -product,
                recipe_id: None,
                recipe_version: None,
                recipe_name: None,
                source_order_id: Some(order_id.to_string()),
                source_order_item_id: Some(item.id.clone()),
                reason_code: None,
                note: None,
                occurred_at: occurred_at_utc.to_string(),
                business_date: business_date.to_string(),
                created_by_user_id: None,
                modifier_delta_id: Some(delta.id.clone()),
                modifier_name: Some(format!("{}: {}", modifier.group_name, modifier.option_name)),
                modifier_delta_version: Some(delta.config_version),
            };
            let id = uuid::Uuid::now_v7().to_string();
            insert_stock_ledger_entry(tx, &id, *next_seq, &entry)?;
            *next_seq += 1;
        }
    }
    Ok(())
}
