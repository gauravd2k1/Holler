//! Invoice assembly and persistence (ADR-016, task 1/3 of T7b): turns an
//! order's lines (or one split's share of them) into a persisted `invoice` +
//! `invoice_line` rows plus the `InvoiceCreated` outbox event, using T7a's
//! tax engine for every money field — this module never computes tax itself,
//! only wires the engine's inputs and stores its outputs.

use std::collections::HashMap;

use rusqlite::Transaction;

use crate::error::{DbError, DbResult};
use crate::model::{
    Invoice, InvoiceLineShare, InvoiceOutboxMeta, IssueInvoiceHeader, IssueInvoiceLinesRequest,
    MissingHsnSacItem, NewInvoice, NewInvoiceLine, OrderItem, OutletFiscalProfile,
};
use crate::repo;
use crate::tax;

use super::numbering;

/// The conservation property ADR-016 §4 makes binding: across every share
/// in `parts`, each `order_item` must be billed for EXACTLY its own
/// `quantity` — no less (loss), no more (duplication/double-tax), and every
/// `order_item` on the order must be covered by at least one share. Checked
/// BEFORE any write happens (called from [`crate::Db::issue_invoice_with_outbox`]
/// / [`crate::Db::issue_split_invoices_with_outbox`] ahead of the assembly
/// loop), so a violation never leaves a partially-issued invoice behind —
/// the whole call fails together, same shape as
/// `DbError::UnroutedKitchenItems`'s all-or-nothing send.
pub(crate) fn validate_conservation(
    order_items: &[OrderItem],
    parts: &[&[InvoiceLineShare]],
) -> DbResult<()> {
    let mut billed: HashMap<&str, i64> = HashMap::new();
    for part in parts {
        for share in *part {
            if share.quantity <= 0 {
                return Err(DbError::InvalidInput(format!(
                    "invoice line share for order_item {} must bill a positive quantity",
                    share.order_item_id
                )));
            }
            *billed.entry(share.order_item_id.as_str()).or_insert(0) += share.quantity;
        }
    }

    for item in order_items {
        let expected = item.quantity;
        let got = billed.remove(item.id.as_str()).unwrap_or(0);
        if got != expected {
            return Err(DbError::InvalidInput(format!(
                "split conservation violated for order_item {}: order line has quantity {expected} \
                 but the supplied shares total {got}",
                item.id
            )));
        }
    }

    if let Some((stray_id, _)) = billed.into_iter().next() {
        return Err(DbError::InvalidInput(format!(
            "invoice line share references order_item {stray_id}, which is not on this order"
        )));
    }

    Ok(())
}

/// The `outlet_fiscal_profile` effective for `outlet_id` at `at` — the
/// latest row with `effective_from <= at` (same "latest applicable, past
/// instant returns the past ruleset" shape as
/// [`crate::tax::resolve_compliance_version`], but this table lives outside
/// the `tax` module entirely, so its own small resolver lives here).
fn resolve_fiscal_profile(
    profiles: &[OutletFiscalProfile],
    outlet_id: &str,
    at: chrono::DateTime<chrono::Utc>,
) -> DbResult<OutletFiscalProfile> {
    let mut best: Option<(&OutletFiscalProfile, chrono::DateTime<chrono::Utc>)> = None;
    for p in profiles {
        if p.outlet_id != outlet_id {
            continue;
        }
        let effective_from = tax::parse_utc(&p.effective_from)?;
        if effective_from > at {
            continue;
        }
        let better = match best {
            None => true,
            Some((_, best_from)) => effective_from > best_from,
        };
        if better {
            best = Some((p, effective_from));
        }
    }
    best.map(|(p, _)| p.clone()).ok_or_else(|| {
        DbError::InvalidInput(format!(
            "no outlet_fiscal_profile effective for outlet {outlet_id} at {}",
            at.to_rfc3339()
        ))
    })
}

fn fiscal_profile_json(p: &OutletFiscalProfile) -> serde_json::Value {
    serde_json::json!({
        "id": p.id,
        "outlet_id": p.outlet_id,
        "legal_name": p.legal_name,
        "trade_name": p.trade_name,
        "address_line1": p.address_line1,
        "address_line2": p.address_line2,
        "city": p.city,
        "state_code": p.state_code,
        "state_name": p.state_name,
        "pincode": p.pincode,
        "gstin": p.gstin,
        "fssai_number": p.fssai_number,
        "invoice_footer_text": p.invoice_footer_text,
        "effective_from": p.effective_from,
    })
}

/// Assembles (but does NOT persist) ONE invoice's `NewInvoice` row plus its
/// computed lines from `req`'s line shares, inside `tx`. Used for both an
/// unsplit bill (`split_count == 1`) and each part of a split group; the
/// caller ([`crate::Db::issue_invoice_with_outbox`]/
/// [`crate::Db::issue_split_invoices_with_outbox`]) is responsible for
/// calling [`validate_conservation`] across every part BEFORE calling this
/// for any of them, stamping `split_group_id` (this function always leaves
/// it `None` — a split group is a caller-level concept, not something one
/// part's own assembly can name), and then calling [`persist_invoice`] —
/// all inside one shared transaction, so a mid-group failure never leaves
/// some parts issued and others not.
///
/// Every money field is produced by [`tax::compute_invoice`] — this
/// function resolves tax profiles/rates/compliance version/fiscal profile
/// (pure data lookups) and hands the engine its inputs, but performs no
/// arithmetic of its own on paise or basis points.
pub(crate) fn build_invoice(
    tx: &Transaction,
    header: &IssueInvoiceHeader,
    req: &IssueInvoiceLinesRequest,
) -> DbResult<(NewInvoice, Vec<tax::LineComputation>)> {
    let order = repo::get_order(tx, &header.order_id)?.ok_or(DbError::NotFound("order"))?;
    if order.outlet_id != header.outlet_id {
        return Err(DbError::InvalidInput(format!(
            "order {} does not belong to outlet {}",
            header.order_id, header.outlet_id
        )));
    }

    let outlet = repo::get_outlet(tx, &header.outlet_id)?.ok_or(DbError::NotFound("outlet"))?;

    let series = repo::list_invoice_series_for_outlet(tx, &header.outlet_id)?
        .into_iter()
        .find(|s| s.code == header.series_code && s.is_active)
        .ok_or_else(|| {
            DbError::InvalidInput(format!(
                "no active invoice_series {:?} for outlet {}",
                header.series_code, header.outlet_id
            ))
        })?;

    let at = numbering::parse_utc(&header.invoice_date)?;

    let versions = repo::list_compliance_versions_for_outlet(tx, &header.outlet_id)?;
    let profiles = repo::list_tax_profiles_for_outlet(tx, &header.outlet_id)?;
    let menu_items = repo::list_menu_items_for_outlet(tx, &header.outlet_id)?;
    let variants = repo::list_menu_item_variants_for_outlet(tx, &header.outlet_id)?;
    let fiscal_profiles = repo::list_outlet_fiscal_profiles_for_outlet(tx, &header.outlet_id)?;

    let menu_item_by_id: HashMap<&str, &crate::model::MenuItem> =
        menu_items.iter().map(|m| (m.id.as_str(), m)).collect();
    let variant_by_id: HashMap<&str, &crate::model::MenuItemVariant> =
        variants.iter().map(|v| (v.id.as_str(), v)).collect();

    let compliance_version = tax::resolve_compliance_version(&versions, &header.outlet_id, at)?;

    let mut tax_lines: Vec<tax::Line> = Vec::with_capacity(req.lines.len());
    let mut rules_by_profile: HashMap<String, Vec<crate::model::TaxRule>> = HashMap::new();
    // ADR-016 0.4.5 §3 / the accompanying track: an invoice must not issue
    // with a line whose resolved HSN/SAC is NULL or blank. Collected across
    // the whole loop (not returned on the first miss) so one rejection names
    // every offending item, not just the first — a manager fixing the
    // catalogue should not have to retry issuance once per missing code.
    let mut missing_hsn: Vec<MissingHsnSacItem> = Vec::new();

    for share in &req.lines {
        let order_item =
            repo::get_order_item_in_tx(tx, &share.order_item_id)?.ok_or_else(|| {
                DbError::InvalidInput(format!("order_item {} not found", share.order_item_id))
            })?;
        if order_item.order_id != header.order_id {
            return Err(DbError::InvalidInput(format!(
                "order_item {} does not belong to order {}",
                share.order_item_id, header.order_id
            )));
        }

        let menu_item = menu_item_by_id
            .get(order_item.menu_item_id.as_str())
            .ok_or_else(|| {
                DbError::InvalidInput(format!(
                    "menu_item {} not found for order_item {}",
                    order_item.menu_item_id, order_item.id
                ))
            })?;

        let mut description = menu_item.name.clone();
        if let Some(variant_id) = &order_item.variant_id {
            if let Some(v) = variant_by_id.get(variant_id.as_str()) {
                description = format!("{description} ({})", v.name);
            }
        }

        // §31: `invoice_line.hsn_sac` is a SNAPSHOT, resolved here and never
        // re-read from live config afterwards — exactly how `description`
        // (above) already behaves, so a later catalogue correction cannot
        // rewrite an issued invoice. Blank is treated the same as NULL: a
        // whitespace-only code is not a code.
        let resolved_hsn_sac = menu_item
            .hsn_sac
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        if resolved_hsn_sac.is_none() {
            missing_hsn.push(MissingHsnSacItem {
                order_item_id: order_item.id.clone(),
                name: description.clone(),
            });
        }

        let tax_profile = tax::resolve_tax_profile(
            &profiles,
            &header.outlet_id,
            menu_item.tax_profile_id.as_deref(),
            at,
        )?;

        if !rules_by_profile.contains_key(&tax_profile.id) {
            let rules = repo::list_tax_rules_for_profile(tx, &tax_profile.id)?;
            rules_by_profile.insert(tax_profile.id.clone(), rules);
        }
        let rules = rules_by_profile
            .get(&tax_profile.id)
            .expect("just inserted above");
        let rates = tax::resolve_rates(rules, &tax_profile.id, &compliance_version.id, at)?;
        let pricing_mode = tax::PricingMode::parse(&tax_profile.pricing_mode)?;

        tax_lines.push(tax::Line {
            order_item_id: order_item.id.clone(),
            description,
            hsn_sac: resolved_hsn_sac,
            quantity: share.quantity,
            unit_price_paise: order_item.unit_price_paise,
            discount_per_unit_paise: share.discount_per_unit_paise,
            tax_profile_id: tax_profile.id.clone(),
            pricing_mode,
            rates,
        });
    }

    if !missing_hsn.is_empty() {
        return Err(DbError::MissingHsnSac {
            order_id: header.order_id.clone(),
            items: missing_hsn,
        });
    }

    let (line_computations, totals) = tax::compute_invoice(&tax_lines)?;

    // The reproducibility snapshot (§31): every distinct profile actually
    // used on THIS invoice's lines, resolved rules included, as they stood
    // at issue time.
    let all_rules: Vec<crate::model::TaxRule> =
        rules_by_profile.values().flatten().cloned().collect();
    let snapshots = tax::build_tax_snapshots(
        &versions,
        &profiles,
        &all_rules,
        &header.outlet_id,
        &tax_lines,
        at,
    )?;
    let tax_snapshot_json = tax::render_tax_snapshots(&snapshots).to_string();

    let fiscal_profile = resolve_fiscal_profile(&fiscal_profiles, &header.outlet_id, at)?;
    let fiscal_profile_json_text = fiscal_profile_json(&fiscal_profile).to_string();

    let invoice_number = numbering::mint_invoice_number(
        tx,
        &series,
        &outlet,
        &header.business_date,
        &header.invoice_date,
    )?;

    let new_invoice = NewInvoice {
        id: req.invoice_id.clone(),
        outlet_id: header.outlet_id.clone(),
        order_id: header.order_id.clone(),
        split_group_id: None, // set by the split caller via `with_split_group`
        split_index: req.split_index,
        split_count: req.split_count,
        series_id: series.id.clone(),
        invoice_number,
        invoice_date: header.invoice_date.clone(),
        business_date: header.business_date.clone(),
        customer_name: header.customer_name.clone(),
        customer_phone: header.customer_phone.clone(),
        customer_gstin: header.customer_gstin.clone(),
        place_of_supply_state_code: header.place_of_supply_state_code.clone(),
        subtotal_paise: totals.subtotal_paise,
        discount_paise: totals.discount_paise,
        taxable_value_paise: totals.taxable_value_paise,
        cgst_paise: totals.cgst_paise,
        sgst_paise: totals.sgst_paise,
        igst_paise: totals.igst_paise,
        cess_paise: totals.cess_paise,
        round_off_paise: totals.round_off_paise,
        grand_total_paise: totals.grand_total_paise,
        compliance_version_id: compliance_version.id.clone(),
        tax_snapshot_json,
        fiscal_profile_json: fiscal_profile_json_text,
        channel: header.channel.clone(),
        tax_liability_party: header.tax_liability_party.clone(),
        eco_operator_name: header.eco_operator_name.clone(),
        eco_operator_gstin: header.eco_operator_gstin.clone(),
        supply_classification: header.supply_classification.clone(),
        created_by_user_id: header.created_by_user_id.clone(),
        created_at: header.invoice_date.clone(),
        updated_at: header.invoice_date.clone(),
    };
    Ok((new_invoice, line_computations))
}

/// Persists an already-assembled invoice (from [`build_invoice`], with
/// `split_group_id` stamped by the caller if this is part of a split) — the
/// invoice row, its lines, and the `InvoiceCreated` outbox row, all inside
/// `tx`.
pub(crate) fn persist_invoice(
    tx: &Transaction,
    new_invoice: NewInvoice,
    req: &IssueInvoiceLinesRequest,
    line_computations: &[tax::LineComputation],
    outbox_meta: &InvoiceOutboxMeta,
) -> DbResult<Invoice> {
    repo::insert_invoice(tx, &new_invoice)?;

    for (i, (share, lc)) in req.lines.iter().zip(line_computations.iter()).enumerate() {
        let line_no = i64::try_from(i + 1).expect("line index fits i64");
        let new_line = NewInvoiceLine {
            id: share.id.clone(),
            invoice_id: new_invoice.id.clone(),
            order_item_id: lc.order_item_id.clone(),
            line_no,
            description: lc.description.clone(),
            hsn_sac: lc.hsn_sac.clone(),
            quantity: lc.quantity,
            unit_price_paise: lc.unit_price_paise,
            gross_paise: lc.gross_paise,
            discount_paise: lc.discount_paise,
            taxable_value_paise: lc.taxable_value_paise,
            tax_profile_id: lc.tax_profile_id.clone(),
            cgst_rate_bps: lc.cgst_rate_bps,
            cgst_paise: lc.cgst_paise,
            sgst_rate_bps: lc.sgst_rate_bps,
            sgst_paise: lc.sgst_paise,
            igst_rate_bps: lc.igst_rate_bps,
            igst_paise: lc.igst_paise,
            cess_rate_bps: lc.cess_rate_bps,
            cess_paise: lc.cess_paise,
            total_paise: lc.total_paise,
        };
        repo::insert_invoice_line(tx, &new_line)?;
    }

    let stored_invoice =
        repo::get_invoice(tx, &new_invoice.id)?.expect("just inserted this exact row above");
    let stored_lines = repo::list_invoice_lines_in_tx(tx, &new_invoice.id)?;

    repo::insert_invoice_created_outbox(tx, &stored_invoice, &stored_lines, outbox_meta)?;

    Ok(stored_invoice)
}
