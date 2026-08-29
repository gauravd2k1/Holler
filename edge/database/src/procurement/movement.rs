//! Purchase returns (`RETURN_TO_VENDOR`) and outbound stock transfers
//! (`TRANSFER_OUT`) — contracts 0.6.0, ADR-019, Milestone 5 track T2.
//!
//! Both post their ledger entries **the same way a receipt does**: the
//! document, its lines and every ledger row commit inside ONE transaction,
//! or none of them do.
//!
//! ============================================================================
//! TWO OF THE THREE PREVIOUSLY-DEAD `entry_type` BRANCHES
//! ============================================================================
//!
//! `RETURN_TO_VENDOR` and `TRANSFER_OUT` have existed in the
//! `stock_ledger_entry` CHECK since contracts 0.5.0 with no writer. This
//! module is the writer. `TRANSFER_IN` and the two `PRODUCTION_*` values stay
//! dead and are exempt with M8 named — a transfer spans two edge databases,
//! which is multi-outlet machinery and not something to half-build here
//! (ADR-019 §8).
//!
//! ============================================================================
//! NEGATIVE, AND NEGATIVE STOCK IS PERMITTED
//! ============================================================================
//!
//! Both post NEGATIVE quantities (0016: consumption negative, purchase
//! positive). **There is no balance check anywhere below.** Returning more
//! than the ledger thinks is on the shelf drives the balance negative, and a
//! negative balance is a variance signal, not an error (ADR-018 Rule 1). If a
//! future change adds a check here, that is the bug.
//!
//! ============================================================================
//! WHAT A MOVEMENT IS WORTH
//! ============================================================================
//!
//! `unit_cost_paise` is NOT NULL on both line tables. When the caller does
//! not state a price, the line is valued at this outlet's weighted average
//! cost, derived from the ledger (`super::cost`) — what the outlet actually
//! paid, not a guess. An item with no costed receipt has no average, and the
//! line then carries zero WITH that fact recorded in the document's `notes`
//! trail being the caller's responsibility; zero is the only value the column
//! admits, and it is reached only after both the caller and the ledger have
//! declined to price the row.
//!
//! ============================================================================
//! NUMBERING
//! ============================================================================
//!
//! `return_number` and `transfer_number` are CALLER-SUPPLIED. Contracts 0.6.0
//! ships `grn_sequence` and no counter for either of these documents, and
//! this crate does not invent one: a `MAX(number) + 1` derivation is exactly
//! the defect `stock_ledger_sequence` was created to remove. Reported as a
//! contract gap. Both columns are `UNIQUE (outlet_id, number)`, so a
//! duplicate is refused by the schema rather than silently accepted.

use chrono::{DateTime, Utc};
use rusqlite::{params, Transaction};

use crate::error::{DbError, DbResult};
use crate::model::{
    NewPurchaseReturn, NewStockLedgerEntry, NewStockTransferOut, ProcurementOutboxMeta,
    PurchaseReturn, PurchaseReturnLine, StockTransferLine, StockTransferOut, MAX_SAFE_INTEGER,
};
use crate::repo;

use super::convert::{fetch_receiving_item, resolve_line_conversion};
use super::cost::weighted_average_cost_paise;
use super::receipt::PROCUREMENT_ORIGIN;

const ENTRY_TYPE_RETURN_TO_VENDOR: &str = "RETURN_TO_VENDOR";
const ENTRY_TYPE_TRANSFER_OUT: &str = "TRANSFER_OUT";

/// Resolves what one outbound line is worth per base unit: the caller's
/// price if they stated one, else this outlet's weighted average cost, else
/// zero. Never negative.
fn valuation_paise(
    tx: &Transaction,
    outlet_id: &str,
    inventory_item_id: &str,
    caller_supplied: Option<i64>,
) -> DbResult<i64> {
    if let Some(stated) = caller_supplied {
        if stated < 0 || stated > MAX_SAFE_INTEGER {
            return Err(DbError::InvalidInput(format!(
                "unit_cost_paise must be between 0 and 2^53-1, got {stated}"
            )));
        }
        return Ok(stated);
    }
    Ok(weighted_average_cost_paise(tx, outlet_id, inventory_item_id)?.unwrap_or(0))
}

fn require_business_date(
    tx: &Transaction,
    outlet_id: &str,
    occurred_at_utc: &str,
) -> DbResult<String> {
    let occurred_at: DateTime<Utc> = crate::tax::parse_utc(occurred_at_utc)?;
    let (timezone, day_start_time) = repo::get_outlet_business_date_config(tx, outlet_id)?;
    Ok(crate::deduction::business_date::compute_business_date(
        occurred_at,
        &timezone,
        &day_start_time,
    ))
}

/// Records one purchase return and posts its `RETURN_TO_VENDOR` ledger
/// entries, all inside `tx`. **Caller must have already checked
/// `procurement.manage`.**
pub(crate) fn record_purchase_return(
    tx: &Transaction,
    req: NewPurchaseReturn,
) -> DbResult<PurchaseReturn> {
    if req.lines.is_empty() {
        return Err(DbError::InvalidInput(
            "a purchase return must have at least one line".to_string(),
        ));
    }
    let business_date = require_business_date(tx, &req.outlet_id, &req.returned_at)?;

    tx.execute(
        "INSERT INTO purchase_return
            (id, outlet_id, supplier_id, grn_id, return_number, reason, returned_at,
             returned_by_user_id, business_date, notes, schema_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1)",
        params![
            req.id,
            req.outlet_id,
            req.supplier_id,
            req.grn_id,
            req.return_number,
            req.reason,
            req.returned_at,
            req.returned_by_user_id,
            business_date,
            req.notes,
        ],
    )?;

    let mut lines = Vec::with_capacity(req.lines.len());
    for (index, line) in req.lines.iter().enumerate() {
        let line_number = index as i64 + 1;
        let Some(item) = fetch_receiving_item(tx, &line.inventory_item_id)? else {
            return Err(DbError::NotFound("inventory_item"));
        };
        // The same conversion the receipt uses, so a return entered in the
        // supplier's unit lands on the ledger in base units. Any gap it
        // reports is not persisted: `grn_gap.grn_id` is NOT NULL, so a return
        // has nowhere to record one. Reported as a contract gap.
        let conversion = resolve_line_conversion(
            tx,
            req.supplier_id.as_deref(),
            &line.inventory_item_id,
            &item,
            &line.entered_purchase_unit,
            line.entered_quantity_micro,
            &line.quantity_dimension,
            0,
        )?;
        let unit_cost_paise = valuation_paise(
            tx,
            &req.outlet_id,
            &line.inventory_item_id,
            line.unit_cost_paise,
        )?;

        let line_id = uuid::Uuid::now_v7().to_string();
        tx.execute(
            "INSERT INTO purchase_return_line
                (id, purchase_return_id, inventory_item_id, grn_line_id, line_number,
                 entered_purchase_unit, entered_quantity_micro, quantity_dimension,
                 base_quantity_micro, unit_cost_paise)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                line_id,
                req.id,
                line.inventory_item_id,
                line.grn_line_id,
                line_number,
                line.entered_purchase_unit,
                line.entered_quantity_micro,
                line.quantity_dimension,
                conversion.base_quantity_micro,
                unit_cost_paise,
            ],
        )?;

        post_outbound_ledger_entry(
            tx,
            &req.outlet_id,
            &line.inventory_item_id,
            &item.name,
            &item.dimension,
            ENTRY_TYPE_RETURN_TO_VENDOR,
            conversion.base_quantity_micro,
            unit_cost_paise,
            &req.returned_at,
            &business_date,
            Some(&req.returned_by_user_id),
            LedgerProvenance::PurchaseReturn(&req.id),
        )?;

        lines.push(PurchaseReturnLine {
            id: line_id,
            purchase_return_id: req.id.clone(),
            inventory_item_id: line.inventory_item_id.clone(),
            grn_line_id: line.grn_line_id.clone(),
            line_number,
            entered_purchase_unit: line.entered_purchase_unit.clone(),
            entered_quantity_micro: line.entered_quantity_micro,
            quantity_dimension: line.quantity_dimension.clone(),
            base_quantity_micro: conversion.base_quantity_micro,
            unit_cost_paise,
        });
    }

    Ok(PurchaseReturn {
        id: req.id,
        outlet_id: req.outlet_id,
        supplier_id: req.supplier_id,
        grn_id: req.grn_id,
        return_number: req.return_number,
        reason: req.reason,
        returned_at: req.returned_at,
        returned_by_user_id: req.returned_by_user_id,
        business_date,
        notes: req.notes,
        lines,
    })
}

/// Records one outbound stock transfer and posts its `TRANSFER_OUT` ledger
/// entries, all inside `tx`. **Caller must have already checked
/// `procurement.manage`.** OUTBOUND HALF ONLY — no `TRANSFER_IN` row is
/// written anywhere, deliberately (M8).
pub(crate) fn record_stock_transfer_out(
    tx: &Transaction,
    req: NewStockTransferOut,
) -> DbResult<StockTransferOut> {
    if req.lines.is_empty() {
        return Err(DbError::InvalidInput(
            "a stock transfer must have at least one line".to_string(),
        ));
    }
    let business_date = require_business_date(tx, &req.outlet_id, &req.dispatched_at)?;

    tx.execute(
        "INSERT INTO stock_transfer_out
            (id, outlet_id, destination_outlet_id, transfer_number, dispatched_at,
             dispatched_by_user_id, business_date, notes, schema_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1)",
        params![
            req.id,
            req.outlet_id,
            req.destination_outlet_id,
            req.transfer_number,
            req.dispatched_at,
            req.dispatched_by_user_id,
            business_date,
            req.notes,
        ],
    )?;

    let mut lines = Vec::with_capacity(req.lines.len());
    for (index, line) in req.lines.iter().enumerate() {
        let line_number = index as i64 + 1;
        if line.base_quantity_micro <= 0 || line.base_quantity_micro > MAX_SAFE_INTEGER {
            return Err(DbError::InvalidInput(format!(
                "base_quantity_micro must be between 1 and 2^53-1, got {}",
                line.base_quantity_micro
            )));
        }
        let Some(item) = fetch_receiving_item(tx, &line.inventory_item_id)? else {
            return Err(DbError::NotFound("inventory_item"));
        };
        let unit_cost_paise = valuation_paise(
            tx,
            &req.outlet_id,
            &line.inventory_item_id,
            line.unit_cost_paise,
        )?;

        let line_id = uuid::Uuid::now_v7().to_string();
        tx.execute(
            "INSERT INTO stock_transfer_line
                (id, stock_transfer_out_id, inventory_item_id, line_number,
                 base_quantity_micro, quantity_dimension, unit_cost_paise)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                line_id,
                req.id,
                line.inventory_item_id,
                line_number,
                line.base_quantity_micro,
                line.quantity_dimension,
                unit_cost_paise,
            ],
        )?;

        post_outbound_ledger_entry(
            tx,
            &req.outlet_id,
            &line.inventory_item_id,
            &item.name,
            &item.dimension,
            ENTRY_TYPE_TRANSFER_OUT,
            line.base_quantity_micro,
            unit_cost_paise,
            &req.dispatched_at,
            &business_date,
            Some(&req.dispatched_by_user_id),
            LedgerProvenance::StockTransferOut(&req.id),
        )?;

        lines.push(StockTransferLine {
            id: line_id,
            stock_transfer_out_id: req.id.clone(),
            inventory_item_id: line.inventory_item_id.clone(),
            line_number,
            base_quantity_micro: line.base_quantity_micro,
            quantity_dimension: line.quantity_dimension.clone(),
            unit_cost_paise,
        });
    }

    Ok(StockTransferOut {
        id: req.id,
        outlet_id: req.outlet_id,
        destination_outlet_id: req.destination_outlet_id,
        transfer_number: req.transfer_number,
        dispatched_at: req.dispatched_at,
        dispatched_by_user_id: req.dispatched_by_user_id,
        business_date,
        notes: req.notes,
        lines,
    })
}

/// Which of 0027's three typed provenance columns this row fills. An enum
/// rather than three `Option<&str>` parameters, so a caller cannot set two
/// of them or none — the exactly-one discipline `origin` already applies to
/// the recipe/modifier pair, expressed in the type system this time.
enum LedgerProvenance<'a> {
    PurchaseReturn(&'a str),
    StockTransferOut(&'a str),
}

#[allow(clippy::too_many_arguments)]
fn post_outbound_ledger_entry(
    tx: &Transaction,
    outlet_id: &str,
    inventory_item_id: &str,
    inventory_item_name: &str,
    dimension: &str,
    entry_type: &str,
    base_quantity_micro: i64,
    unit_cost_paise: i64,
    occurred_at: &str,
    business_date: &str,
    created_by_user_id: Option<&str>,
    provenance: LedgerProvenance,
) -> DbResult<()> {
    let (source_purchase_return_id, source_stock_transfer_out_id) = match provenance {
        LedgerProvenance::PurchaseReturn(id) => (Some(id.to_string()), None),
        LedgerProvenance::StockTransferOut(id) => (None, Some(id.to_string())),
    };
    let entry = NewStockLedgerEntry {
        outlet_id: outlet_id.to_string(),
        inventory_item_id: inventory_item_id.to_string(),
        inventory_item_name: inventory_item_name.to_string(),
        dimension: dimension.to_string(),
        entry_type: entry_type.to_string(),
        origin: PROCUREMENT_ORIGIN.to_string(),
        // NEGATIVE: stock leaves the outlet. `base_quantity_micro` is
        // validated positive before it reaches here, so the negation is exact
        // rather than a magnitude assumption.
        quantity_applied_micro: -base_quantity_micro,
        recipe_id: None,
        recipe_version: None,
        recipe_name: None,
        source_order_id: None,
        source_order_item_id: None,
        reason_code: None,
        note: None,
        occurred_at: occurred_at.to_string(),
        business_date: business_date.to_string(),
        created_by_user_id: created_by_user_id.map(str::to_string),
        modifier_delta_id: None,
        modifier_name: None,
        modifier_delta_version: None,
        unit_cost_paise: Some(unit_cost_paise),
        source_stock_count_id: None,
        source_grn_id: None,
        source_purchase_return_id,
        source_stock_transfer_out_id,
    };
    crate::deduction::ledger::insert_stock_ledger_entry_with_next_seq(
        tx,
        outlet_id,
        occurred_at,
        &entry,
    )
}

/// [`record_purchase_return`] plus its `local_outbox` row, in the same
/// transaction as every row it wrote.
pub(crate) fn record_purchase_return_with_outbox(
    tx: &Transaction,
    req: NewPurchaseReturn,
    meta: &ProcurementOutboxMeta,
) -> DbResult<PurchaseReturn> {
    let stored = record_purchase_return(tx, req)?;
    repo::insert_purchase_returned_outbox(tx, &stored, meta)?;
    Ok(stored)
}

/// [`record_stock_transfer_out`] plus its `local_outbox` row, in the same
/// transaction as every row it wrote.
pub(crate) fn record_stock_transfer_out_with_outbox(
    tx: &Transaction,
    req: NewStockTransferOut,
    meta: &ProcurementOutboxMeta,
) -> DbResult<StockTransferOut> {
    let stored = record_stock_transfer_out(tx, req)?;
    repo::insert_stock_dispatched_outbox(tx, &stored, meta)?;
    Ok(stored)
}
