//! Goods receipt (GRN) at the edge, and the `PURCHASE` ledger entries it
//! posts — contracts 0.6.0, ADR-019, Milestone 5 track T2.
//!
//! ============================================================================
//! THE RULE THIS FILE EXISTS TO ENFORCE
//! ============================================================================
//!
//! **A GRN NEVER BLOCKS ON A PO.**
//!
//! No PO at all, a PO that never synced here, a PO amended after dispatch, an
//! item the PO does not list, an over-delivery, no `supplier_item`, an
//! unconvertible unit, a dimension mismatch, an unknown supplier — **each
//! records a `grn_gap` and ACCEPTS THE RECEIPT.**
//!
//! Refusing a delivery standing in the kitchen doorway is the outage;
//! recording the gap is the protection. A refused receipt does not stop the
//! goods entering the kitchen — it only stops the system knowing they did,
//! which is strictly worse than an accepted receipt with a gap attached.
//! This is ADR-018's "stock never blocks a sale" and "a missing recipe never
//! fails a confirm", generalised to the inbound side.
//!
//! The only thing below that returns `Err` is malformed CALLER input (a
//! non-positive quantity, a magnitude past 2^53, a receipt with no lines) or
//! a genuine SQLite failure. If a future change adds a business check here,
//! that is the bug, not a missing feature.
//!
//! ============================================================================
//! ONE TRANSACTION, JUDGED AGAINST THE CRASH
//! ============================================================================
//!
//! The `goods_receipt_note`, every `grn_line`, every `grn_gap`, every
//! `PURCHASE` `stock_ledger_entry` (with its `entry_seq` mark), the
//! `grn_sequence` advance that minted the number, and the outbox rows all
//! commit TOGETHER OR NOT AT ALL. [`record_goods_receipt`] takes a
//! `&Transaction` and opens none of its own, exactly like
//! `deduct_stock_for_confirmed_order`: the edge is a single SQLite writer,
//! so the receipt needs no lock of its own.
//!
//! M5 acceptance criterion 2 kills the POS between the GRN write and the
//! ledger post. [`crate::crash::AFTER_GRN_BEFORE_LEDGER`] is the abort point
//! that makes that deterministic rather than a guessed kill moment, and it
//! sits at exactly that boundary below.
//!
//! ============================================================================
//! NULLING A DANGLING LINK IS NOT LOSING IT
//! ============================================================================
//!
//! `goods_receipt_note.purchase_order_id` and `supplier_id` carry real
//! FOREIGN KEYs, and `PRAGMA foreign_keys` is ON (`crate::pragma`). A receipt
//! naming a PO this edge never synced therefore CANNOT store that id: the
//! insert would be refused, and refusing is the one thing this path may never
//! do. So the column is stored NULL and the id the operator gave is preserved
//! verbatim in the gap's `detail`, where a human reads it. The fact survives;
//! only the join does not, and the join could not have resolved anyway.

use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension, Transaction};

use crate::error::{DbError, DbResult};
use crate::model::{
    GoodsReceiptNote, GrnEntryIntentEcho, GrnGap, GrnLine, NewGoodsReceiptNote, NewGrnLine,
    NewStockLedgerEntry, ProcurementOutboxMeta,
};
use crate::repo;

use super::convert::{
    fetch_receiving_item, resolve_line_conversion, GrnGapReason, LineConversion, ReceivingItem,
};
use super::numbering::next_grn_number;

/// `stock_ledger_entry.origin` for every row a GOODS RECEIPT posts.
///
/// **This was `MANUAL` until contracts 0.6.2, and `MANUAL` was a lie.** The
/// `origin` CHECK admitted no procurement member until
/// `packages/contracts/sqlite/0029_ledger_origin_procurement.sql` rebuilt the
/// table to add `GOODS_RECEIPT`, `PURCHASE_RETURN` and `STOCK_TRANSFER` --
/// one per provenance column 0027 had already added -- so a variance report
/// grouping by `origin` could not tell a delivery from a hand adjustment,
/// which is the exact distinction the column exists to preserve.
///
/// The value pairs with `source_grn_id`: `origin` and provenance can never
/// disagree about which document produced the movement, because nothing in
/// this module sets one without the other. `recipe_id` and
/// `modifier_delta_id` stay NULL, which is what the extended provenance
/// CHECK's "no recipe, no modifier" branch requires.
pub(crate) const ORIGIN_GOODS_RECEIPT: &str = "GOODS_RECEIPT";

/// `stock_ledger_entry.entry_type` for a receipt.
const ENTRY_TYPE_PURCHASE: &str = "PURCHASE";

/// One resolved line, held in memory between the conversion pass and the
/// write pass so the echo and the write cannot diverge.
struct ResolvedLine {
    line_number: i64,
    item: ReceivingItem,
    conversion: LineConversion,
    purchase_order_line_id: Option<String>,
}

fn require_lines(req: &NewGoodsReceiptNote) -> DbResult<()> {
    if req.lines.is_empty() {
        return Err(DbError::InvalidInput(
            "a goods receipt must have at least one line".to_string(),
        ));
    }
    Ok(())
}

fn supplier_exists(tx: &Transaction, supplier_id: &str) -> DbResult<bool> {
    let found: Option<i64> = tx
        .query_row(
            "SELECT 1 FROM supplier WHERE id = ?1",
            params![supplier_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(found.is_some())
}

fn purchase_order_exists(tx: &Transaction, purchase_order_id: &str) -> DbResult<bool> {
    let found: Option<i64> = tx
        .query_row(
            "SELECT 1 FROM purchase_order WHERE id = ?1",
            params![purchase_order_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(found.is_some())
}

/// The PO line this receipt line answers, matched on the item. Returns the
/// line's id, its ordered quantity and the unit it was ordered in, so the
/// over-delivery comparison can be made in BASE units on both sides rather
/// than comparing a supplier unit against a canonical one.
struct MatchedPoLine {
    id: String,
    ordered_quantity_micro: i64,
    purchase_unit: String,
    quantity_dimension: String,
}

fn match_purchase_order_line(
    tx: &Transaction,
    purchase_order_id: &str,
    inventory_item_id: &str,
    explicit_line_id: Option<&str>,
) -> DbResult<Option<MatchedPoLine>> {
    let sql = "SELECT id, ordered_quantity_micro, purchase_unit, quantity_dimension \
               FROM purchase_order_line \
               WHERE purchase_order_id = ?1 AND ";
    let row = if let Some(line_id) = explicit_line_id {
        tx.query_row(
            &format!("{sql}id = ?2"),
            params![purchase_order_id, line_id],
            |row| {
                Ok(MatchedPoLine {
                    id: row.get(0)?,
                    ordered_quantity_micro: row.get(1)?,
                    purchase_unit: row.get(2)?,
                    quantity_dimension: row.get(3)?,
                })
            },
        )
        .optional()?
    } else {
        tx.query_row(
            &format!("{sql}inventory_item_id = ?2 ORDER BY line_number LIMIT 1"),
            params![purchase_order_id, inventory_item_id],
            |row| {
                Ok(MatchedPoLine {
                    id: row.get(0)?,
                    ordered_quantity_micro: row.get(1)?,
                    purchase_unit: row.get(2)?,
                    quantity_dimension: row.get(3)?,
                })
            },
        )
        .optional()?
    };
    Ok(row)
}

/// Base-unit quantity already received against one PO line, AT THIS OUTLET.
///
/// **This is deliberately a local figure.** ADR-019 §4: the edge derives
/// receipt progress from its own `grn_line` rows and the cloud from every
/// outlet's, the two legitimately differ, and neither is reconciled to the
/// other.
fn received_base_so_far(tx: &Transaction, purchase_order_line_id: &str) -> DbResult<i64> {
    let total: i64 = tx.query_row(
        "SELECT COALESCE(SUM(base_quantity_micro), 0) FROM grn_line \
         WHERE purchase_order_line_id = ?1",
        params![purchase_order_line_id],
        |row| row.get(0),
    )?;
    Ok(total)
}

#[allow(clippy::too_many_arguments)]
fn insert_grn_gap(
    tx: &Transaction,
    outlet_id: &str,
    grn_id: &str,
    grn_line_id: Option<&str>,
    inventory_item_id: Option<&str>,
    reason: GrnGapReason,
    detail: &str,
    occurred_at: &str,
    business_date: &str,
) -> DbResult<GrnGap> {
    let id = uuid::Uuid::now_v7().to_string();
    // A PLAIN OUTBOX ROW: no entry_seq, no counter, no cursor, no contiguity
    // check (ADR-019 §2). A gap is a discrete event a buyer acts on, a
    // handful a week — not the per-sale stream stock_deduction_gap is, which
    // is the only reason that one earned 0.5.8's ranged-sync machinery.
    tx.execute(
        "INSERT INTO grn_gap
            (id, outlet_id, grn_id, grn_line_id, inventory_item_id, reason, detail,
             occurred_at, business_date)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            id,
            outlet_id,
            grn_id,
            grn_line_id,
            inventory_item_id,
            reason.as_str(),
            detail,
            occurred_at,
            business_date,
        ],
    )?;
    Ok(GrnGap {
        id,
        outlet_id: outlet_id.to_string(),
        grn_id: grn_id.to_string(),
        grn_line_id: grn_line_id.map(str::to_string),
        inventory_item_id: inventory_item_id.map(str::to_string),
        reason: reason.as_str().to_string(),
        detail: Some(detail.to_string()),
        occurred_at: occurred_at.to_string(),
        business_date: business_date.to_string(),
    })
}

/// Resolves one submitted line without writing anything. Shared by the write
/// path and by [`entry_intent_echo`], so **the echo cannot disagree with what
/// the receipt will record** — an independently derived echo is worse than
/// none (ADR-019 §3, acceptance criterion 4).
fn resolve_line(
    tx: &Transaction,
    supplier_id: Option<&str>,
    line: &NewGrnLine,
) -> DbResult<Option<(ReceivingItem, LineConversion)>> {
    let Some(item) = fetch_receiving_item(tx, &line.inventory_item_id)? else {
        return Ok(None);
    };
    let conversion = resolve_line_conversion(
        tx,
        supplier_id,
        &line.inventory_item_id,
        &item,
        &line.entered_purchase_unit,
        line.entered_quantity_micro,
        // THE AUTHOR'S OWN DECLARATION, passed straight through. Never
        // `item.dimension` — that would make the comparison x == x.
        &line.quantity_dimension,
        line.purchase_price_paise,
    )?;
    Ok(Some((item, conversion)))
}

/// The `entryIntentEcho` the receiving screen MUST show before the operator
/// commits (ADR-019 §3, M5 acceptance criterion 4): what was typed, the rate
/// that will be applied, and the base-unit quantity that will actually be
/// recorded — plus every gap the line would produce.
///
/// Read-only. Runs the same resolution the write path runs.
pub(crate) fn entry_intent_echo(
    tx: &Transaction,
    supplier_id: Option<&str>,
    line: &NewGrnLine,
) -> DbResult<GrnEntryIntentEcho> {
    let Some((item, conversion)) = resolve_line(tx, supplier_id, line)? else {
        return Err(DbError::NotFound("inventory_item"));
    };
    Ok(GrnEntryIntentEcho {
        inventory_item_id: line.inventory_item_id.clone(),
        inventory_item_name: item.name,
        entered_purchase_unit: line.entered_purchase_unit.clone(),
        entered_quantity_micro: line.entered_quantity_micro,
        quantity_dimension: line.quantity_dimension.clone(),
        pack_size_micro_applied: conversion.pack_size_micro_applied,
        base_quantity_micro: conversion.base_quantity_micro,
        item_dimension: item.dimension,
        unit_cost_paise: conversion.unit_cost_paise,
        line_total_paise: conversion.line_total_paise,
        gap_reasons: conversion
            .gaps
            .iter()
            .map(|(reason, _)| reason.as_str().to_string())
            .collect(),
    })
}

/// Records one goods receipt and posts its `PURCHASE` ledger entries, all
/// inside `tx`. **Caller must have already checked `procurement.manage`** —
/// no permission lookup exists in this crate (see `crate::stock`'s module
/// doc comment for why).
///
/// `business_date` is computed ONCE, here, from `received_at` through the
/// single business-date function (`repo::get_outlet_business_date_config` +
/// `compute_business_date`), never accepted from a caller and never derived
/// by slicing a UTC instant — the M3 defect M5 T7b deleted.
pub(crate) fn record_goods_receipt(
    tx: &Transaction,
    req: NewGoodsReceiptNote,
) -> DbResult<GoodsReceiptNote> {
    require_lines(&req)?;

    let received_at: DateTime<Utc> = crate::tax::parse_utc(&req.received_at)?;
    let (timezone, day_start_time) = repo::get_outlet_business_date_config(tx, &req.outlet_id)?;
    let business_date = crate::deduction::business_date::compute_business_date(
        received_at,
        &timezone,
        &day_start_time,
    );

    // ---- Pass 1: resolve every line BEFORE anything is written, so a
    // caller-input rejection (the only rejection there is) happens before the
    // counter is advanced rather than half way through a receipt.
    let mut resolved: Vec<ResolvedLine> = Vec::with_capacity(req.lines.len());
    let mut unknown_item_lines: Vec<&NewGrnLine> = Vec::new();
    for (index, line) in req.lines.iter().enumerate() {
        match resolve_line(tx, req.supplier_id.as_deref(), line)? {
            Some((item, conversion)) => resolved.push(ResolvedLine {
                line_number: index as i64 + 1,
                item,
                conversion,
                purchase_order_line_id: line.purchase_order_line_id.clone(),
            }),
            // An item this edge has no row for cannot become a `grn_line`
            // (the column has a real FK) nor a ledger entry (which needs the
            // item's name and dimension). It is not silently dropped: it
            // becomes a header-level gap below, with the id in the prose.
            None => unknown_item_lines.push(line),
        }
    }

    let grn_number = next_grn_number(tx, &req.outlet_id, &business_date)?;

    // ---- Header links. A dangling link is stored NULL and preserved in the
    // gap's prose — see the module header for why nulling is not losing.
    let mut header_gaps: Vec<(GrnGapReason, String)> = Vec::new();

    let stored_supplier_id = match req.supplier_id.as_deref() {
        Some(supplier_id) if supplier_exists(tx, supplier_id)? => Some(supplier_id.to_string()),
        Some(supplier_id) => {
            header_gaps.push((
                GrnGapReason::SupplierNotFound,
                format!(
                    "Delivery recorded against supplier {supplier_id:?}, which this outlet \
                     has no row for. The goods were received; confirm who they came from."
                ),
            ));
            None
        }
        None => None,
    };

    let stored_purchase_order_id = match req.purchase_order_id.as_deref() {
        Some(po_id) if purchase_order_exists(tx, po_id)? => Some(po_id.to_string()),
        Some(po_id) => {
            header_gaps.push((
                GrnGapReason::PurchaseOrderNotFound,
                format!(
                    "Received against purchase order {po_id:?}, which has never reached \
                     this till. The goods were received; check the order in the admin."
                ),
            ));
            None
        }
        None => {
            header_gaps.push((
                GrnGapReason::NoPurchaseOrder,
                "Received with no purchase order — walk-in delivery, standing order or \
                 emergency purchase. The goods were received; raise or attach an order \
                 if one should exist."
                    .to_string(),
            ));
            None
        }
    };

    tx.execute(
        "INSERT INTO goods_receipt_note
            (id, outlet_id, purchase_order_id, supplier_id, grn_number, delivery_note_ref,
             received_at, received_by_user_id, business_date, notes, schema_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1)",
        params![
            req.id,
            req.outlet_id,
            stored_purchase_order_id,
            stored_supplier_id,
            grn_number,
            req.delivery_note_ref,
            req.received_at,
            req.received_by_user_id,
            business_date,
            req.notes,
        ],
    )?;

    let mut gaps: Vec<GrnGap> = Vec::new();
    for (reason, detail) in header_gaps {
        gaps.push(insert_grn_gap(
            tx,
            &req.outlet_id,
            &req.id,
            None,
            None,
            reason,
            &detail,
            &req.received_at,
            &business_date,
        )?);
    }
    for line in unknown_item_lines {
        gaps.push(insert_grn_gap(
            tx,
            &req.outlet_id,
            &req.id,
            None,
            None,
            GrnGapReason::NoUnitConversion,
            &format!(
                "Item {:?} is not stocked at this outlet, so its line could not be \
                 recorded or posted to stock. {} {} was delivered — add the item and \
                 correct the count.",
                line.inventory_item_id, line.entered_quantity_micro, line.entered_purchase_unit
            ),
            &req.received_at,
            &business_date,
        )?);
    }

    // ============================================================
    // M5 ACCEPTANCE CRITERION 2's exact window. Everything above is
    // written; not one ledger row is. A process death here must leave the
    // receipt and the ledger AGREEING — which, because both are inside one
    // transaction, means neither exists.
    // ============================================================
    crate::crash::maybe_abort(crate::crash::AFTER_GRN_BEFORE_LEDGER);

    let mut lines: Vec<GrnLine> = Vec::new();
    for resolved_line in &resolved {
        let submitted = &req.lines[(resolved_line.line_number - 1) as usize];
        let grn_line_id = uuid::Uuid::now_v7().to_string();

        // ---- PO matching. Every branch here records and continues.
        let mut line_gaps: Vec<(GrnGapReason, String)> = resolved_line
            .conversion
            .gaps
            .iter()
            .map(|(reason, detail)| (*reason, detail.clone()))
            .collect();

        let mut stored_po_line_id: Option<String> = None;
        if let Some(po_id) = stored_purchase_order_id.as_deref() {
            match match_purchase_order_line(
                tx,
                po_id,
                &submitted.inventory_item_id,
                resolved_line.purchase_order_line_id.as_deref(),
            )? {
                Some(po_line) => {
                    stored_po_line_id = Some(po_line.id.clone());
                    // Over-delivery is compared in BASE units on both sides:
                    // the PO ordered in its own purchase unit, which is not
                    // necessarily the one the delivery arrived in.
                    let ordered_base = ordered_quantity_in_base_units(
                        tx,
                        stored_supplier_id.as_deref(),
                        &submitted.inventory_item_id,
                        &resolved_line.item,
                        &po_line,
                    )?;
                    let already = received_base_so_far(tx, &po_line.id)?;
                    let after = already.saturating_add(resolved_line.conversion.base_quantity_micro);
                    if ordered_base > 0 && after > ordered_base {
                        line_gaps.push((
                            GrnGapReason::QuantityExceedsOrdered,
                            format!(
                                "Over-delivery: {after} base units received against {ordered_base} \
                                 ordered on this line, counting only receipts at THIS outlet. \
                                 The goods were received; agree the excess with the supplier."
                            ),
                        ));
                    }
                }
                None => line_gaps.push((
                    GrnGapReason::PoLineNotFound,
                    format!(
                        "{} was delivered but the purchase order does not list it — it may \
                         have been added after the order was sent. The goods were received.",
                        resolved_line.item.name
                    ),
                )),
            }
        }

        tx.execute(
            "INSERT INTO grn_line
                (id, grn_id, inventory_item_id, line_number, purchase_order_line_id,
                 entered_purchase_unit, entered_quantity_micro, quantity_dimension,
                 base_quantity_micro, pack_size_micro_applied, unit_cost_paise,
                 line_total_paise, batch_code, expiry_date)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                grn_line_id,
                req.id,
                submitted.inventory_item_id,
                resolved_line.line_number,
                stored_po_line_id,
                submitted.entered_purchase_unit,
                submitted.entered_quantity_micro,
                submitted.quantity_dimension,
                resolved_line.conversion.base_quantity_micro,
                resolved_line.conversion.pack_size_micro_applied,
                resolved_line.conversion.unit_cost_paise,
                resolved_line.conversion.line_total_paise,
                submitted.batch_code,
                submitted.expiry_date,
            ],
        )?;

        for (reason, detail) in line_gaps {
            gaps.push(insert_grn_gap(
                tx,
                &req.outlet_id,
                &req.id,
                Some(&grn_line_id),
                Some(&submitted.inventory_item_id),
                reason,
                &detail,
                &req.received_at,
                &business_date,
            )?);
        }

        // ---- The ledger row. POSITIVE: a purchase adds stock (0016's own
        // sign convention). `unit_cost_paise` is set here for the first time
        // in this product's history — ADR-018 §8 deferred the column to
        // exactly this write.
        let entry = NewStockLedgerEntry {
            outlet_id: req.outlet_id.clone(),
            inventory_item_id: submitted.inventory_item_id.clone(),
            inventory_item_name: resolved_line.item.name.clone(),
            // The ITEM's dimension, because that is what the quantity is now
            // expressed in after conversion. The author's declaration is
            // stored on the grn_line and compared, never substituted here.
            dimension: resolved_line.item.dimension.clone(),
            entry_type: ENTRY_TYPE_PURCHASE.to_string(),
            origin: ORIGIN_GOODS_RECEIPT.to_string(),
            quantity_applied_micro: resolved_line.conversion.base_quantity_micro,
            recipe_id: None,
            recipe_version: None,
            recipe_name: None,
            source_order_id: None,
            source_order_item_id: None,
            reason_code: None,
            note: None,
            occurred_at: req.received_at.clone(),
            business_date: business_date.clone(),
            created_by_user_id: Some(req.received_by_user_id.clone()),
            modifier_delta_id: None,
            modifier_name: None,
            modifier_delta_version: None,
            unit_cost_paise: Some(resolved_line.conversion.unit_cost_paise),
            source_stock_count_id: None,
            source_grn_id: Some(req.id.clone()),
            source_purchase_return_id: None,
            source_stock_transfer_out_id: None,
        };
        crate::deduction::ledger::insert_stock_ledger_entry_with_next_seq(
            tx,
            &req.outlet_id,
            &req.received_at,
            &entry,
        )?;

        lines.push(GrnLine {
            id: grn_line_id,
            grn_id: req.id.clone(),
            inventory_item_id: submitted.inventory_item_id.clone(),
            line_number: resolved_line.line_number,
            purchase_order_line_id: stored_po_line_id,
            entered_purchase_unit: submitted.entered_purchase_unit.clone(),
            entered_quantity_micro: submitted.entered_quantity_micro,
            quantity_dimension: submitted.quantity_dimension.clone(),
            base_quantity_micro: resolved_line.conversion.base_quantity_micro,
            pack_size_micro_applied: resolved_line.conversion.pack_size_micro_applied,
            unit_cost_paise: resolved_line.conversion.unit_cost_paise,
            line_total_paise: resolved_line.conversion.line_total_paise,
            batch_code: submitted.batch_code.clone(),
            expiry_date: submitted.expiry_date.clone(),
        });
    }

    Ok(GoodsReceiptNote {
        id: req.id,
        outlet_id: req.outlet_id,
        purchase_order_id: stored_purchase_order_id,
        supplier_id: stored_supplier_id,
        grn_number,
        delivery_note_ref: req.delivery_note_ref,
        received_at: req.received_at,
        received_by_user_id: req.received_by_user_id,
        business_date,
        notes: req.notes,
        lines,
        gaps,
    })
}

/// Converts a PO line's ordered quantity into base units, so an
/// over-delivery check compares like with like. Reuses the same conversion
/// the receipt uses; any gap it produces is DISCARDED here, because a gap
/// about the ORDER is not a gap about the RECEIPT and would mislead the
/// buyer reading the receipt's gap list.
fn ordered_quantity_in_base_units(
    tx: &Transaction,
    supplier_id: Option<&str>,
    inventory_item_id: &str,
    item: &ReceivingItem,
    po_line: &MatchedPoLine,
) -> DbResult<i64> {
    let conversion = resolve_line_conversion(
        tx,
        supplier_id,
        inventory_item_id,
        item,
        &po_line.purchase_unit,
        po_line.ordered_quantity_micro,
        &po_line.quantity_dimension,
        0,
    )?;
    Ok(conversion.base_quantity_micro)
}

/// [`record_goods_receipt`] plus its `local_outbox` rows, in the SAME
/// transaction as every row the receipt wrote.
///
/// TWO aggregate types ride out of this one call: `goods_receipt_note` and
/// `grn_gap` (ADR-019 §9). A gap records what could not be matched ABOUT
/// THIS RECEIPT and belongs beside the receipt it explains; a gap arriving
/// by a different path could not be joined to it. Transport, cursors and
/// retry budgets are T3's, not this crate's — this writes the rows and the
/// events and nothing else.
pub(crate) fn record_goods_receipt_with_outbox(
    tx: &Transaction,
    req: NewGoodsReceiptNote,
    meta: &ProcurementOutboxMeta,
) -> DbResult<GoodsReceiptNote> {
    let stored = record_goods_receipt(tx, req)?;
    repo::insert_goods_received_outbox(tx, &stored, meta)?;
    for (index, gap) in stored.gaps.iter().enumerate() {
        repo::insert_grn_gap_recorded_outbox(tx, gap, meta, index)?;
    }
    Ok(stored)
}
