//! Writes `stock_ledger_entry` / `stock_deduction_gap` rows from a resolved
//! recipe, and the single entry point [`deduct_stock_for_confirmed_order`]
//! that [`crate::Db::confirm_order_with_outbox`] calls inside its own
//! transaction (Milestone 4, track T2, ADR-018; entry_seq durable-counter
//! and `UNRESOLVABLE_REFERENCE` corrections, contracts 0.5.3 addendum).
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
//! error. A genuine `DbError` (an actual SQLite failure, or a genuinely
//! invalid `occurred_at`/outlet config — see below) still propagates via `?`
//! exactly like every other repo call in this crate; that is the one thing
//! this module does NOT try to swallow, per the task brief.
//!
//! **Stock never blocks a sale.** No balance check exists anywhere below.
//! Negative stock is a variance signal, not an error (ADR-018 Rule 1) — if a
//! future change to this file adds one, it is the bug, not a missing
//! feature.
//!
//! **`occurred_at_utc` is parsed exactly ONCE, at the top of
//! [`deduct_stock_for_confirmed_order`], into a `DateTime<Utc>`** — never
//! defaulted to "now" on a parse failure. `confirmed_at` is internally
//! generated (the Tauri command layer's local clock, per `OrderConfirmedMeta`'s
//! doc comment), so an unparseable value here is a genuine caller defect,
//! not a business gap; it propagates as a real `DbError` and the whole
//! confirm rolls back, exactly like any other malformed input this crate
//! refuses to guess about.
//!
//! **A silent skip is never used to avoid an imprecise reason code.** Every
//! branch below that cannot deduct writes a `stock_deduction_gap` row —
//! see [`write_gap`] / the `UNRESOLVABLE_REFERENCE` arms in
//! [`deduct_modifiers_for_line`] — because `stock_deduction_gap` exists
//! *because* a real failure with an absent signal is an absent feature
//! (ADR-018 0.5.3 addendum). This reverses the original version of this
//! file, which skipped two cases silently; both were found and corrected
//! in review.

use chrono::{DateTime, Utc};
use rusqlite::{params, Transaction};

use crate::error::DbResult;
use crate::inventory::{resolve_recipe_for_variant, ResolveOutcome};
use crate::model::{NewStockDeductionGap, NewStockLedgerEntry, OrderItem};
use crate::repo;

use super::business_date::compute_business_date;

/// `stock_deduction_gap.reason` for a dangling reference the resolver
/// itself does not touch — a `modifier_ingredient_delta.inventory_item_id`
/// pointing at an `inventory_item` row that is not there (contracts 0.5.3).
/// Mirrors `GapReason::UnresolvableReference::as_str()`
/// (`crate::inventory::resolve`) without depending on that type, since this
/// case never goes through recipe resolution at all.
const UNRESOLVABLE_REFERENCE: &str = "UNRESOLVABLE_REFERENCE";

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
             created_by_user_id, modifier_delta_id, modifier_name, modifier_delta_version,
             unit_cost_paise, source_stock_count_id, source_grn_id,
             source_purchase_return_id, source_stock_transfer_out_id, line_total_paise)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18,
                 ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28)",
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
            e.unit_cost_paise,
            e.source_stock_count_id,
            e.source_grn_id,
            e.source_purchase_return_id,
            e.source_stock_transfer_out_id,
            e.line_total_paise,
        ],
    )?;
    Ok(())
}

/// Inserts one `stock_ledger_entry` row, first minting its durable
/// `entry_seq` mark in the SAME transaction (`repo::
/// next_stock_ledger_sequence_value` — contracts 0.5.3, the `invoice_
/// sequence` atomicity argument applied to the ledger: a crash either takes
/// both the mark and the row, or neither, never one without the other).
pub(crate) fn insert_stock_ledger_entry_with_next_seq(
    tx: &Transaction,
    outlet_id: &str,
    occurred_at_utc: &str,
    e: &NewStockLedgerEntry,
) -> DbResult<()> {
    let entry_seq = repo::next_stock_ledger_sequence_value(tx, outlet_id, occurred_at_utc)?;
    let id = uuid::Uuid::now_v7().to_string();
    insert_stock_ledger_entry(tx, &id, entry_seq, e)
}

/// Inserts one `stock_deduction_gap` row, first minting its durable
/// `entry_seq` mark from `stock_deduction_gap_sequence` in the SAME
/// transaction (contracts 0.5.8) — the atomicity argument
/// [`insert_stock_ledger_entry_with_next_seq`] makes for the ledger, applied
/// to the other ranged stream: a crash takes both the mark and the row, or
/// neither. A hole in the sequence reads to the cloud as a lost row, and a
/// reused one as a duplicate.
///
/// The counter is the gap stream's OWN. Sharing the ledger's would put
/// permanent holes in both sequences, and a hole is precisely what the
/// cloud's contiguity check reads as a loss.
pub(crate) fn insert_stock_deduction_gap(
    tx: &Transaction,
    id: &str,
    g: &NewStockDeductionGap,
) -> DbResult<()> {
    let entry_seq =
        repo::next_stock_deduction_gap_sequence_value(tx, &g.outlet_id, &g.occurred_at)?;
    tx.execute(
        "INSERT INTO stock_deduction_gap
            (id, outlet_id, entry_seq, order_id, order_item_id, menu_item_id,
             menu_item_variant_id, menu_item_name, quantity, reason, occurred_at,
             business_date)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            id,
            g.outlet_id,
            entry_seq,
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
/// (mirrors `OrderConfirmedMeta::confirmed_at`), parsed to a real instant
/// exactly once here — see the module doc comment for why a parse failure
/// propagates rather than defaulting to "now". The parsed instant drives
/// `business_date`, computed once for the whole order and reused for every
/// row (they are one sale, one moment); the ORIGINAL string is still what
/// gets stored in each row's own `occurred_at` column, unchanged.
pub(crate) fn deduct_stock_for_confirmed_order(
    tx: &Transaction,
    outlet_id: &str,
    order_id: &str,
    occurred_at_utc: &str,
) -> DbResult<()> {
    let occurred_at: DateTime<Utc> = crate::tax::parse_utc(occurred_at_utc)?;
    let (timezone, day_start_time) = repo::get_outlet_business_date_config(tx, outlet_id)?;
    let business_date = compute_business_date(occurred_at, &timezone, &day_start_time);

    let items: Vec<OrderItem> = repo::list_order_items_in_tx(tx, order_id)?;

    for item in &items {
        deduct_one_line(
            tx,
            outlet_id,
            order_id,
            item,
            occurred_at_utc,
            &business_date,
        )?;
    }

    Ok(())
}

fn deduct_one_line(
    tx: &Transaction,
    outlet_id: &str,
    order_id: &str,
    item: &OrderItem,
    occurred_at_utc: &str,
    business_date: &str,
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
                    unit_cost_paise: None,
                    // No invoiced total: this origin is valued AT the average, not by an
                    // invoice, so writing a rounded quantity x rate product here would
                    // fabricate precision and feed it back into the average (0.6.3).
                    line_total_paise: None,
                    source_stock_count_id: None,
                    source_grn_id: None,
                    source_purchase_return_id: None,
                    source_stock_transfer_out_id: None,
                };
                insert_stock_ledger_entry_with_next_seq(tx, outlet_id, occurred_at_utc, &entry)?;
            }
        }
        ResolveOutcome::Gap(reason) => {
            write_gap(
                tx,
                outlet_id,
                order_id,
                item,
                reason.as_str(),
                occurred_at_utc,
                business_date,
            )?;
        }
    }

    deduct_modifiers_for_line(
        tx,
        outlet_id,
        order_id,
        item,
        occurred_at_utc,
        business_date,
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_gap(
    tx: &Transaction,
    outlet_id: &str,
    order_id: &str,
    item: &OrderItem,
    reason: &str,
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
        reason: reason.to_string(),
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
///
/// **A dangling `inventory_item_id` writes an `UNRESOLVABLE_REFERENCE` gap,
/// never a silent skip** (contracts 0.5.3 addendum — corrected in review;
/// the original version of this function skipped both this case and an
/// i64-overflow case silently, reasoning that no named reason existed for
/// either. That inverted why `stock_deduction_gap` exists.)
fn deduct_modifiers_for_line(
    tx: &Transaction,
    outlet_id: &str,
    order_id: &str,
    item: &OrderItem,
    occurred_at_utc: &str,
    business_date: &str,
) -> DbResult<()> {
    for modifier in repo::list_order_item_modifiers_in_tx(tx, &item.id)? {
        for delta in fetch_modifier_deltas(tx, &modifier.modifier_id)? {
            let Some(inv_item) = fetch_inventory_item(tx, &delta.inventory_item_id)? else {
                write_gap(
                    tx,
                    outlet_id,
                    order_id,
                    item,
                    UNRESOLVABLE_REFERENCE,
                    occurred_at_utc,
                    business_date,
                )?;
                continue;
            };

            // `modifier_ingredient_delta.quantity_micro` is bounded to
            // |value| <= 1e15 by `modifier_ingredient_delta_quantity_is_
            // bounded` (contracts 0.5.3, sqlite/0021) — a thousand tonnes of
            // one ingredient per serving is bad data, not a runtime
            // condition the arithmetic below needs to handle. `item.quantity`
            // carries no equivalent schema bound, so the checked i128
            // multiply stays (this is the one residual overflow case
            // reported alongside this change, at order quantities beyond
            // roughly 9223 servings on a single line — seven orders of
            // magnitude past anything a restaurant cart produces, but not
            // structurally impossible the way the delta side now is).
            // Reported rather than silently reintroduced: a hit here writes
            // the same UNRESOLVABLE_REFERENCE-labelled gap as a dangling
            // reference, the closest available named signal, rather than
            // going back to silence.
            let Some(product) = i128::from(delta.quantity_micro)
                .checked_mul(i128::from(item.quantity))
                .and_then(|p| i64::try_from(p).ok())
            else {
                write_gap(
                    tx,
                    outlet_id,
                    order_id,
                    item,
                    UNRESOLVABLE_REFERENCE,
                    occurred_at_utc,
                    business_date,
                )?;
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
                unit_cost_paise: None,
                // No invoiced total: this origin is valued AT the average, not by an
                // invoice, so writing a rounded quantity x rate product here would
                // fabricate precision and feed it back into the average (0.6.3).
                line_total_paise: None,
                source_stock_count_id: None,
                source_grn_id: None,
                source_purchase_return_id: None,
                source_stock_transfer_out_id: None,
            };
            insert_stock_ledger_entry_with_next_seq(tx, outlet_id, occurred_at_utc, &entry)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Outlet;
    use crate::Db;

    fn seed_outlet(conn: &rusqlite::Connection, id: &str) {
        repo::upsert_outlet(
            conn,
            &Outlet {
                id: id.to_string(),
                brand_id: "brand-1".to_string(),
                name: format!("Outlet {id}"),
                timezone: "Asia/Kolkata".to_string(),
                config_version: 1,
                created_at: "2026-01-01T00:00:00Z".to_string(),
                updated_at: "2026-01-01T00:00:00Z".to_string(),
            },
        )
        .expect("seed outlet");
    }

    fn gap(outlet_id: &str, name: &str, occurred_at: &str) -> NewStockDeductionGap {
        NewStockDeductionGap {
            outlet_id: outlet_id.to_string(),
            order_id: "order-1".to_string(),
            order_item_id: "order-item-1".to_string(),
            menu_item_id: "menu-item-1".to_string(),
            menu_item_variant_id: None,
            menu_item_name: name.to_string(),
            quantity: 1,
            reason: "NO_RECIPE".to_string(),
            occurred_at: occurred_at.to_string(),
            business_date: "2026-08-21".to_string(),
        }
    }

    /// The acceptance-criterion-5 report must show the most recent gaps
    /// first, and must never leak another outlet's rows into this outlet's
    /// report.
    #[test]
    fn list_stock_deduction_gaps_is_newest_first_and_scoped_to_one_outlet() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection(), "outlet-1");
        seed_outlet(db.connection(), "outlet-2");

        let conn = db.connection_mut();
        let tx = conn.transaction().expect("begin");
        insert_stock_deduction_gap(
            &tx,
            "gap-1",
            &gap("outlet-1", "Oldest", "2026-08-21T10:00:00Z"),
        )
        .expect("insert oldest");
        insert_stock_deduction_gap(
            &tx,
            "gap-3",
            &gap("outlet-1", "Newest", "2026-08-21T12:00:00Z"),
        )
        .expect("insert newest");
        insert_stock_deduction_gap(
            &tx,
            "gap-2",
            &gap("outlet-1", "Middle", "2026-08-21T11:00:00Z"),
        )
        .expect("insert middle");
        insert_stock_deduction_gap(
            &tx,
            "gap-9",
            &gap("outlet-2", "Other outlet", "2026-08-21T13:00:00Z"),
        )
        .expect("insert other outlet");
        tx.commit().expect("commit");

        let rows = db.list_stock_deduction_gaps("outlet-1").expect("read gaps");
        let names: Vec<&str> = rows.iter().map(|g| g.menu_item_name.as_str()).collect();
        assert_eq!(
            names,
            vec!["Newest", "Middle", "Oldest"],
            "the report is ordered occurred_at DESC"
        );
        assert!(
            rows.iter().all(|g| g.outlet_id == "outlet-1"),
            "outlet-2's gap must never appear in outlet-1's report"
        );

        // Round-trip one row field-for-field: a read model that silently
        // drops or transposes a column would still satisfy the ordering
        // assertions above.
        let newest = &rows[0];
        assert_eq!(newest.id, "gap-3");
        assert_eq!(newest.order_id, "order-1");
        assert_eq!(newest.order_item_id, "order-item-1");
        assert_eq!(newest.menu_item_id, "menu-item-1");
        assert_eq!(newest.menu_item_variant_id, None);
        assert_eq!(newest.quantity, 1);
        assert_eq!(newest.reason, "NO_RECIPE");
        assert_eq!(newest.occurred_at, "2026-08-21T12:00:00Z");
        assert_eq!(newest.business_date, "2026-08-21");
    }

    /// Two gaps recorded in the same instant still order by insertion, so
    /// the report does not reshuffle between two reads of the same data.
    #[test]
    fn gaps_in_the_same_instant_are_broken_by_id_descending() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection(), "outlet-1");

        let conn = db.connection_mut();
        let tx = conn.transaction().expect("begin");
        for (id, name) in [("gap-a", "First"), ("gap-b", "Second"), ("gap-c", "Third")] {
            insert_stock_deduction_gap(&tx, id, &gap("outlet-1", name, "2026-08-21T10:00:00Z"))
                .expect("insert");
        }
        tx.commit().expect("commit");

        let names: Vec<String> = db
            .list_stock_deduction_gaps("outlet-1")
            .expect("read gaps")
            .into_iter()
            .map(|g| g.menu_item_name)
            .collect();
        assert_eq!(names, vec!["Third", "Second", "First"]);
    }

    /// The report is a fixed-cost read, not a scan whose cost grows with an
    /// append-only signal table.
    #[test]
    fn list_stock_deduction_gaps_is_bounded_at_the_report_limit() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection(), "outlet-1");

        let over = repo::STOCK_DEDUCTION_GAP_REPORT_LIMIT + 25;
        let conn = db.connection_mut();
        let tx = conn.transaction().expect("begin");
        for i in 0..over {
            // Zero-padded so lexical id order matches insertion order.
            insert_stock_deduction_gap(
                &tx,
                &format!("gap-{i:05}"),
                &gap("outlet-1", &format!("Item {i}"), "2026-08-21T10:00:00Z"),
            )
            .expect("insert");
        }
        tx.commit().expect("commit");

        let rows = db.list_stock_deduction_gaps("outlet-1").expect("read gaps");
        assert_eq!(
            rows.len() as i64,
            repo::STOCK_DEDUCTION_GAP_REPORT_LIMIT,
            "the read is hard-bounded even with more rows present"
        );
        assert_eq!(
            rows[0].menu_item_name,
            format!("Item {}", over - 1),
            "the bound keeps the NEWEST rows, never the oldest"
        );
    }
}
