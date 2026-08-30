//! Purchase-unit conversion for the inbound path (ADR-019 §3, contracts
//! 0.6.0). **The conversion happens EXACTLY ONCE, at the edge, and both
//! sides are stored.**
//!
//! ============================================================================
//! THE RULE THIS FILE EXISTS TO ENFORCE
//! ============================================================================
//!
//! **Nothing here can refuse a receipt.** Every unresolvable condition
//! returns a [`GrnGapReason`] alongside a usable conversion, never an error:
//! no `supplier_item` row, an unconvertible unit label, a dimension the
//! author declared that disagrees with the item — each degrades and the
//! receipt still lands. This is `crate::inventory::resolve`'s `Ok(Gap(_))`
//! posture, inbound. A genuine caller defect (a non-positive quantity, a
//! magnitude past 2^53) is still a typed `DbError`, exactly as
//! `NewWastageEntry` treats one: that is malformed input, not a shop-floor
//! condition.
//!
//! ============================================================================
//! `quantity_dimension` IS THE UNIT THE AUTHOR CHOSE
//! ============================================================================
//!
//! It arrives on [`crate::model::NewGrnLine`] as a required field and is
//! compared against `inventory_item.dimension`. **If any write path or UI
//! ever fills it in from the item, the comparison becomes `x == x`, the
//! `DIMENSION_MISMATCH` guard can never fire, and it will look correct in
//! review** (contracts 0.5.2, ADR-019 §6). Nothing in this module reads the
//! item's dimension for any purpose other than that comparison and the
//! ledger row's own `dimension` column.
//!
//! ============================================================================
//! NO FLOAT, AND WHY THE ROUNDING IS TWO-STEP RATHER THAN ONE-SHOT
//! ============================================================================
//!
//! Every quantity is an integer count of micro-units; intermediates are
//! exact `i128` rationals through `crate::inventory::round_ratio_half_away_from_zero`.
//!
//! The applied rate is rounded FIRST and stored, and `base_quantity_micro`
//! is then derived from the STORED rate:
//!
//! ```text
//! pack_size_micro_applied = round_half_away(pack_rate x yield_ppm / 1e6)
//! base_quantity_micro     = round_half_away(entered_quantity_micro
//!                                           x pack_size_micro_applied / 1e6)
//! ```
//!
//! A single-shot `round(entered x pack x yield / 1e12)` would be very
//! slightly more precise and would make the row NOT REPRODUCIBLE FROM
//! ITSELF: an auditor holding the `grn_line` could not recompute
//! `base_quantity_micro` from `entered_quantity_micro` and
//! `pack_size_micro_applied` without also knowing the yield factor in force
//! on the day — and `grn_line` has no column to snapshot that in. ADR-019 §3
//! says the snapshot exists so the arithmetic survives a later
//! `supplier_item` edit; the yield lives on `inventory_item` and is just as
//! editable, so it is folded into the ONE snapshotted rate. Reproducibility
//! from the row beats a sub-micro-unit precision difference when the
//! question being asked is "this receipt is 1000x wrong, what happened?".
//!
//! `a_stored_line_recomputes_its_own_base_quantity` is the guard on that.

use rusqlite::{params, OptionalExtension, Transaction};

use crate::error::{DbError, DbResult};
use crate::inventory::{convert_tier1, round_ratio_half_away_from_zero};
use crate::model::MAX_SAFE_INTEGER;

/// One micro-unit scale factor: a `*_micro` value is its unit x 10^6
/// (contracts 0015). Named rather than written as a literal at each site,
/// per the no-magic-numbers rule.
pub(crate) const MICRO: i64 = 1_000_000;

/// The identity yield, in parts per million: 1_000_000 ppm = 100%
/// (`inventory_item.yield_factor_ppm`'s own default).
pub(crate) const IDENTITY_PPM: i64 = 1_000_000;

/// The eight `grn_gap.reason` values, and they are a CLOSED SET — the
/// migration's own CHECK will reject anything else, so this enum is the
/// only sanctioned way to name one. Mirrors
/// `packages/contracts/sqlite/0027_m5_procurement.sql`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GrnGapReason {
    /// Received with no PO at all — walk-in delivery, standing order,
    /// emergency purchase.
    NoPurchaseOrder,
    /// A PO was referenced but never synced to this edge.
    PurchaseOrderNotFound,
    /// An item was received that the PO does not list, including one added
    /// after dispatch.
    PoLineNotFound,
    /// Over-delivery against the ordered quantity. Accepted, flagged.
    QuantityExceedsOrdered,
    /// No `supplier_item` row for this item and unit.
    NoSupplierItem,
    /// The purchase unit is not convertible to the item's base unit.
    NoUnitConversion,
    /// The dimension the author entered differs from the item's dimension.
    DimensionMismatch,
    /// A delivery from a supplier this edge has no row for.
    SupplierNotFound,
}

impl GrnGapReason {
    pub fn as_str(self) -> &'static str {
        match self {
            GrnGapReason::NoPurchaseOrder => "NO_PURCHASE_ORDER",
            GrnGapReason::PurchaseOrderNotFound => "PURCHASE_ORDER_NOT_FOUND",
            GrnGapReason::PoLineNotFound => "PO_LINE_NOT_FOUND",
            GrnGapReason::QuantityExceedsOrdered => "QUANTITY_EXCEEDS_ORDERED",
            GrnGapReason::NoSupplierItem => "NO_SUPPLIER_ITEM",
            GrnGapReason::NoUnitConversion => "NO_UNIT_CONVERSION",
            GrnGapReason::DimensionMismatch => "DIMENSION_MISMATCH",
            GrnGapReason::SupplierNotFound => "SUPPLIER_NOT_FOUND",
        }
    }
}

/// The `inventory_item` facts the inbound path snapshots onto the rows it
/// writes — name and dimension for the ledger entry (0016's no-FK
/// provenance rule) and the yield factor applied at receipt.
#[derive(Debug, Clone)]
pub(crate) struct ReceivingItem {
    pub(crate) name: String,
    /// The item's OWN dimension. Read for the ledger row and for the
    /// mismatch comparison — never to fill in the author's declaration.
    pub(crate) dimension: String,
    pub(crate) yield_factor_ppm: i64,
}

pub(crate) fn fetch_receiving_item(
    tx: &Transaction,
    inventory_item_id: &str,
) -> DbResult<Option<ReceivingItem>> {
    tx.query_row(
        "SELECT name, dimension, yield_factor_ppm FROM inventory_item WHERE id = ?1",
        params![inventory_item_id],
        |row| {
            Ok(ReceivingItem {
                name: row.get(0)?,
                dimension: row.get(1)?,
                yield_factor_ppm: row.get(2)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

/// A fully resolved receipt line: both sides of the conversion, the money,
/// and every gap the resolution produced. Gaps are carried, never thrown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LineConversion {
    /// The rate ACTUALLY APPLIED, yield folded in — see the module header.
    pub(crate) pack_size_micro_applied: i64,
    pub(crate) base_quantity_micro: i64,
    /// Paise per BASE unit.
    pub(crate) unit_cost_paise: i64,
    pub(crate) line_total_paise: i64,
    pub(crate) gaps: Vec<(GrnGapReason, String)>,
}

/// Where the pack rate came from, before yield is applied. Kept separate
/// from the gap list so the caller can tell "resolved by a supplier row"
/// from "resolved by the frozen dimensional map" without parsing prose.
struct PackRate {
    /// Exact rational: one purchase unit = `num / den` base micro-units.
    num: i128,
    den: i128,
    gaps: Vec<(GrnGapReason, String)>,
}

/// Resolves how many base micro-units one `purchase_unit` contains, in this
/// order, degrading with a gap at each step and never failing:
///
/// 1. `supplier_item (supplier_id, inventory_item_id, purchase_unit)` — the
///    supplier's own declared pack size. This is the authoritative rate when
///    it exists, and it is the ONLY one that carries a supplier-declared
///    `quantity_dimension` for the mismatch check.
/// 2. The frozen Tier 1 dimensional map (`kg`, `l`, `dozen`, ...) — physics,
///    not configuration. A delivery note reading "50 kg" converts without any
///    supplier row at all.
/// 3. `item_unit_conversion` — the per-item Tier 2 pack ratio, which is where
///    an outlet-defined "CRATE" lives if the supplier row is missing.
/// 4. Identity. `base_quantity_micro == entered_quantity_micro`, with a
///    `NO_UNIT_CONVERSION` gap. **The receipt still lands.** Recording the
///    typed figure unconverted, flagged, beats refusing the delivery.
fn resolve_pack_rate(
    tx: &Transaction,
    supplier_id: Option<&str>,
    inventory_item_id: &str,
    purchase_unit: &str,
) -> DbResult<PackRate> {
    let mut gaps = Vec::new();

    if let Some(supplier_id) = supplier_id {
        let found: Option<i64> = tx
            .query_row(
                "SELECT pack_size_micro FROM supplier_item \
                 WHERE supplier_id = ?1 AND inventory_item_id = ?2 AND purchase_unit = ?3",
                params![supplier_id, inventory_item_id, purchase_unit],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(pack_size_micro) = found {
            return Ok(PackRate {
                num: i128::from(pack_size_micro),
                den: 1,
                gaps,
            });
        }
    }

    gaps.push((
        GrnGapReason::NoSupplierItem,
        format!(
            "No supplier_item row for this item in unit {purchase_unit:?}; \
             the rate was resolved from the unit label instead."
        ),
    ));

    if let Ok((_, micro)) = convert_tier1(purchase_unit, 1) {
        return Ok(PackRate {
            num: micro,
            den: 1,
            gaps,
        });
    }

    let tier2: Option<(i64, i64)> = tx
        .query_row(
            "SELECT numerator, denominator FROM item_unit_conversion \
             WHERE inventory_item_id = ?1 AND pack_unit_label = ?2",
            params![inventory_item_id, purchase_unit],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if let Some((numerator, denominator)) = tier2 {
        if denominator > 0 && numerator > 0 {
            return Ok(PackRate {
                num: i128::from(numerator),
                den: i128::from(denominator),
                gaps,
            });
        }
    }

    gaps.push((
        GrnGapReason::NoUnitConversion,
        format!(
            "Unit {purchase_unit:?} is not convertible to this item's base unit. \
             The quantity was recorded EXACTLY AS ENTERED, unconverted — check it \
             before trusting the stock figure."
        ),
    ));
    Ok(PackRate {
        num: i128::from(MICRO),
        den: 1,
        gaps,
    })
}

/// Validates one caller-supplied magnitude. **This is the only thing on the
/// inbound path that rejects**, and it rejects malformed input rather than a
/// business condition — a zero or negative received quantity is not a
/// delivery, and a quantity past 2^53 is a value the POS could not display
/// without silently rounding it.
fn require_in_safe_range(value: i64, field: &str) -> DbResult<()> {
    if value <= 0 {
        return Err(DbError::InvalidInput(format!(
            "{field} must be greater than zero, got {value}"
        )));
    }
    if value > MAX_SAFE_INTEGER {
        return Err(DbError::InvalidInput(format!(
            "{field} is {value}, past the 2^53 exact-integer limit every quantity \
             in this product is bounded by"
        )));
    }
    Ok(())
}

fn require_non_negative_money(value: i64, field: &str) -> DbResult<()> {
    if value < 0 {
        return Err(DbError::InvalidInput(format!(
            "{field} must not be negative, got {value}"
        )));
    }
    if value > MAX_SAFE_INTEGER {
        return Err(DbError::InvalidInput(format!(
            "{field} is {value}, past the 2^53 exact-integer limit"
        )));
    }
    Ok(())
}

/// Folds `yield_factor_ppm` into the pack rate and rounds ONCE, producing
/// the effective rate that is stored on the row.
///
/// **`yield_factor_ppm` is applied here, at receipt** — the consumer
/// `scripts/check-contract-field-consumers.mjs` names when it removed the
/// field's exemption at 0.6.0 ("now applied during receipt conversion").
/// An item declared at 92.5% yield posts 92.5% of the gross delivered
/// quantity to the ledger.
fn effective_rate(pack: &PackRate, yield_factor_ppm: i64) -> Option<i64> {
    // The identity yield on a whole-number pack rate is EXACT -- no rounding
    // step at all, so the overwhelmingly common case cannot lose a
    // micro-unit to a rounding rule it does not need.
    if yield_factor_ppm == IDENTITY_PPM && pack.den == 1 {
        return i64::try_from(pack.num).ok();
    }
    let num = pack.num.checked_mul(i128::from(yield_factor_ppm))?;
    let den = pack.den.checked_mul(i128::from(MICRO))?;
    if den <= 0 {
        return None;
    }
    let rounded = round_ratio_half_away_from_zero(num, den);
    i64::try_from(rounded).ok()
}

/// Applies a stored rate to a typed quantity. **This is the function an
/// auditor re-runs against a persisted `grn_line`**, which is why it takes
/// only the two columns the row carries.
pub(crate) fn base_quantity_from_stored_rate(
    entered_quantity_micro: i64,
    pack_size_micro_applied: i64,
) -> Option<i64> {
    let num =
        i128::from(entered_quantity_micro).checked_mul(i128::from(pack_size_micro_applied))?;
    let rounded = round_ratio_half_away_from_zero(num, i128::from(MICRO));
    i64::try_from(rounded).ok()
}

/// Resolves one receipt line end to end. **Never returns `Err` for a
/// business or config reason** — see the module header for the one class it
/// does reject.
///
/// `declared_dimension` is the author's own declaration off
/// [`crate::model::NewGrnLine::quantity_dimension`]. It is compared, never
/// derived.
// Eight parameters, and each one is a distinct fact the resolution needs;
// bundling them into a struct would only move the same list one line up and
// hide which of them is the author's declaration -- the one parameter whose
// provenance is the whole point of this function.
#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_line_conversion(
    tx: &Transaction,
    supplier_id: Option<&str>,
    inventory_item_id: &str,
    item: &ReceivingItem,
    purchase_unit: &str,
    entered_quantity_micro: i64,
    declared_dimension: &str,
    purchase_price_paise: i64,
) -> DbResult<LineConversion> {
    require_in_safe_range(entered_quantity_micro, "entered_quantity_micro")?;
    require_non_negative_money(purchase_price_paise, "purchase_price_paise")?;

    let pack = resolve_pack_rate(tx, supplier_id, inventory_item_id, purchase_unit)?;
    let mut gaps = pack.gaps.clone();

    // THE GUARD THAT CAN ONLY FIRE BECAUSE THE AUTHOR DECLARED THE UNIT.
    // Never `declared_dimension = item.dimension` anywhere above this line.
    if declared_dimension != item.dimension {
        gaps.push((
            GrnGapReason::DimensionMismatch,
            format!(
                "Received as {declared_dimension} but {} is stocked as {}. \
                 The receipt was accepted; the quantity is almost certainly wrong.",
                item.name, item.dimension
            ),
        ));
    }

    let mut applied = effective_rate(&pack, item.yield_factor_ppm).unwrap_or(0);
    let mut base = base_quantity_from_stored_rate(entered_quantity_micro, applied).unwrap_or(0);

    // `base_quantity_micro` is `NOT NULL CHECK (> 0)` and must stay inside
    // the 2^53 window. A rate that overflows, rounds the quantity away to
    // nothing, or produces an undisplayable magnitude falls back to the
    // identity — the typed figure, recorded verbatim and flagged — because
    // the alternative is refusing a delivery that is already in the kitchen.
    if applied <= 0 || base <= 0 || base > MAX_SAFE_INTEGER {
        gaps.push((
            GrnGapReason::NoUnitConversion,
            format!(
                "The conversion for unit {purchase_unit:?} did not produce a usable \
                 base quantity (rate {applied}, result {base}). The quantity was \
                 recorded EXACTLY AS ENTERED, unconverted."
            ),
        ));
        applied = MICRO;
        base = entered_quantity_micro;
    }

    // Money, integer paise, computed here and nowhere above this crate.
    // line_total is the invoice figure: price per purchase unit x the typed
    // quantity of purchase units. unit_cost is then that total spread over
    // the base units actually received, which is what the ledger carries and
    // what weighted average cost is derived from.
    let line_total_paise = round_ratio_half_away_from_zero(
        i128::from(entered_quantity_micro).saturating_mul(i128::from(purchase_price_paise)),
        i128::from(MICRO),
    );
    let line_total_paise = i64::try_from(line_total_paise).unwrap_or(MAX_SAFE_INTEGER);
    let unit_cost_paise = round_ratio_half_away_from_zero(
        i128::from(line_total_paise).saturating_mul(i128::from(MICRO)),
        i128::from(base),
    );
    let unit_cost_paise = i64::try_from(unit_cost_paise).unwrap_or(MAX_SAFE_INTEGER);

    gaps.sort();
    gaps.dedup_by(|a, b| a.0 == b.0);

    Ok(LineConversion {
        pack_size_micro_applied: applied,
        base_quantity_micro: base,
        unit_cost_paise,
        line_total_paise,
        gaps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{grams, kilograms};
    use crate::procurement::testsupport::{seed_inventory_item, seed_outlet, seed_supplier_item};
    use crate::Db;

    const OUTLET: &str = "outlet-1";
    const SUPPLIER: &str = "supplier-1";
    const RICE: &str = "item-rice";

    fn item(tx: &Transaction, id: &str) -> ReceivingItem {
        fetch_receiving_item(tx, id)
            .expect("read item")
            .expect("item must exist")
    }

    /// The ADR-018 §3 worked example, inbound: a 50 kg sack.
    #[test]
    fn a_supplier_pack_size_converts_the_typed_figure_to_base_units() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection(), OUTLET);
        seed_inventory_item(db.connection(), RICE, OUTLET, "Rice", "MASS", IDENTITY_PPM);
        seed_supplier_item(
            db.connection(),
            OUTLET,
            SUPPLIER,
            RICE,
            "SACK",
            kilograms(50),
            "MASS",
        );

        let conn = db.connection_mut();
        let tx = conn.transaction().expect("begin");
        let it = item(&tx, RICE);
        let c = resolve_line_conversion(
            &tx,
            Some(SUPPLIER),
            RICE,
            &it,
            "SACK",
            2 * MICRO,
            "MASS",
            200_000,
        )
        .expect("resolve");

        assert_eq!(c.pack_size_micro_applied, kilograms(50));
        assert_eq!(c.base_quantity_micro, kilograms(100));
        assert_eq!(c.line_total_paise, 400_000, "2 sacks at Rs 2000 each");
        // Rs 4000 over 100 kg = 4 paise per gram, the base unit for MASS.
        assert_eq!(c.unit_cost_paise, 4);
        assert!(c.gaps.is_empty(), "a fully configured line records no gap");
    }

    /// The invariant the two-step rounding buys: the row recomputes its own
    /// converted quantity from the two columns it stores, with no reference
    /// to `supplier_item` or to the yield factor in force that day.
    #[test]
    fn a_stored_line_recomputes_its_own_base_quantity() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection(), OUTLET);
        // A non-identity yield, so the fold-in is actually exercised.
        seed_inventory_item(db.connection(), RICE, OUTLET, "Rice", "MASS", 925_000);
        seed_supplier_item(
            db.connection(),
            OUTLET,
            SUPPLIER,
            RICE,
            "SACK",
            kilograms(50),
            "MASS",
        );

        let conn = db.connection_mut();
        let tx = conn.transaction().expect("begin");
        let it = item(&tx, RICE);
        let c = resolve_line_conversion(
            &tx,
            Some(SUPPLIER),
            RICE,
            &it,
            "SACK",
            3 * MICRO,
            "MASS",
            200_000,
        )
        .expect("resolve");

        assert_eq!(
            base_quantity_from_stored_rate(3 * MICRO, c.pack_size_micro_applied),
            Some(c.base_quantity_micro),
            "an auditor holding only the grn_line must reach the same number"
        );
    }

    /// `yield_factor_ppm` is APPLIED, not inert: 92.5% of a 50 kg sack.
    #[test]
    fn the_item_yield_factor_is_applied_during_receipt_conversion() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection(), OUTLET);
        seed_inventory_item(db.connection(), RICE, OUTLET, "Rice", "MASS", 925_000);
        seed_supplier_item(
            db.connection(),
            OUTLET,
            SUPPLIER,
            RICE,
            "SACK",
            kilograms(50),
            "MASS",
        );

        let conn = db.connection_mut();
        let tx = conn.transaction().expect("begin");
        let it = item(&tx, RICE);
        let c = resolve_line_conversion(
            &tx,
            Some(SUPPLIER),
            RICE,
            &it,
            "SACK",
            MICRO,
            "MASS",
            200_000,
        )
        .expect("resolve");

        // 50_000_000_000 micro-grams x 0.925, independently: 46.25 kg.
        assert_eq!(c.pack_size_micro_applied, 46_250_000_000);
        assert_eq!(c.base_quantity_micro, grams(46_250));
    }

    /// A missing supplier row degrades to the frozen Tier 1 map and STILL
    /// converts — the delivery note said "kg" and physics is not
    /// configuration.
    #[test]
    fn a_missing_supplier_item_falls_back_to_the_frozen_unit_map_and_gaps() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection(), OUTLET);
        seed_inventory_item(db.connection(), RICE, OUTLET, "Rice", "MASS", IDENTITY_PPM);

        let conn = db.connection_mut();
        let tx = conn.transaction().expect("begin");
        let it = item(&tx, RICE);
        let c = resolve_line_conversion(&tx, None, RICE, &it, "kg", 25 * MICRO, "MASS", 4_000)
            .expect("resolve");

        assert_eq!(c.base_quantity_micro, kilograms(25));
        assert!(
            c.gaps
                .iter()
                .any(|(r, _)| *r == GrnGapReason::NoSupplierItem),
            "the fallback is recorded, not silent: {:?}",
            c.gaps
        );
        assert!(
            !c.gaps
                .iter()
                .any(|(r, _)| *r == GrnGapReason::NoUnitConversion),
            "a unit the frozen map knows is not an unconvertible unit"
        );
    }

    /// An unknown unit label records `NO_UNIT_CONVERSION` and records the
    /// typed figure verbatim. **The receipt is not refused.**
    #[test]
    fn an_unconvertible_unit_records_what_was_typed_and_accepts_the_line() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection(), OUTLET);
        seed_inventory_item(db.connection(), RICE, OUTLET, "Rice", "MASS", IDENTITY_PPM);

        let conn = db.connection_mut();
        let tx = conn.transaction().expect("begin");
        let it = item(&tx, RICE);
        let c = resolve_line_conversion(&tx, None, RICE, &it, "GUNNY", 7 * MICRO, "MASS", 100)
            .expect("resolve");

        assert_eq!(c.pack_size_micro_applied, MICRO, "identity rate");
        assert_eq!(
            c.base_quantity_micro,
            7 * MICRO,
            "recorded exactly as entered"
        );
        assert!(c
            .gaps
            .iter()
            .any(|(r, _)| *r == GrnGapReason::NoUnitConversion));
    }

    /// THE `x == x` TRAP, falsified. The author declares VOLUME for an item
    /// stocked as MASS; the guard fires. It can only ever fire because the
    /// declaration is an input rather than a lookup.
    #[test]
    fn a_declared_dimension_that_disagrees_with_the_item_gaps_and_still_receives() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection(), OUTLET);
        seed_inventory_item(db.connection(), RICE, OUTLET, "Rice", "MASS", IDENTITY_PPM);

        let conn = db.connection_mut();
        let tx = conn.transaction().expect("begin");
        let it = item(&tx, RICE);
        let c = resolve_line_conversion(&tx, None, RICE, &it, "kg", MICRO, "VOLUME", 100)
            .expect("resolve");

        assert!(
            c.gaps
                .iter()
                .any(|(r, _)| *r == GrnGapReason::DimensionMismatch),
            "the author said VOLUME, the item is MASS: {:?}",
            c.gaps
        );
        assert!(
            c.base_quantity_micro > 0,
            "a mismatch degrades to a gap, it never refuses the receipt"
        );
    }

    /// The matching negative control: when the author's declaration agrees,
    /// no mismatch gap is produced. Without this, the test above would pass
    /// against an implementation that gapped unconditionally.
    #[test]
    fn a_declared_dimension_that_agrees_produces_no_mismatch_gap() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection(), OUTLET);
        seed_inventory_item(db.connection(), RICE, OUTLET, "Rice", "MASS", IDENTITY_PPM);

        let conn = db.connection_mut();
        let tx = conn.transaction().expect("begin");
        let it = item(&tx, RICE);
        let c = resolve_line_conversion(&tx, None, RICE, &it, "kg", MICRO, "MASS", 100)
            .expect("resolve");

        assert!(!c
            .gaps
            .iter()
            .any(|(r, _)| *r == GrnGapReason::DimensionMismatch));
    }

    /// The one class this path DOES reject: malformed caller input. A
    /// quantity of zero is not a delivery, and a magnitude past 2^53 would
    /// reach the POS silently rounded.
    #[test]
    fn a_non_positive_or_undisplayable_quantity_is_a_typed_error_not_a_gap() {
        let mut db = Db::open_in_memory_for_tests().expect("open db");
        seed_outlet(db.connection(), OUTLET);
        seed_inventory_item(db.connection(), RICE, OUTLET, "Rice", "MASS", IDENTITY_PPM);

        let conn = db.connection_mut();
        let tx = conn.transaction().expect("begin");
        let it = item(&tx, RICE);

        assert!(matches!(
            resolve_line_conversion(&tx, None, RICE, &it, "kg", 0, "MASS", 100),
            Err(DbError::InvalidInput(_))
        ));
        assert!(matches!(
            resolve_line_conversion(&tx, None, RICE, &it, "kg", -1, "MASS", 100),
            Err(DbError::InvalidInput(_))
        ));
        assert!(matches!(
            resolve_line_conversion(
                &tx,
                None,
                RICE,
                &it,
                "kg",
                MAX_SAFE_INTEGER + 1,
                "MASS",
                100
            ),
            Err(DbError::InvalidInput(_))
        ));
    }

    #[test]
    fn the_eight_gap_reasons_are_the_closed_set_the_migration_checks() {
        let all = [
            GrnGapReason::NoPurchaseOrder,
            GrnGapReason::PurchaseOrderNotFound,
            GrnGapReason::PoLineNotFound,
            GrnGapReason::QuantityExceedsOrdered,
            GrnGapReason::NoSupplierItem,
            GrnGapReason::NoUnitConversion,
            GrnGapReason::DimensionMismatch,
            GrnGapReason::SupplierNotFound,
        ];
        assert_eq!(all.len(), 8);
        let rendered: Vec<&str> = all.iter().map(|r| r.as_str()).collect();
        assert_eq!(
            rendered,
            vec![
                "NO_PURCHASE_ORDER",
                "PURCHASE_ORDER_NOT_FOUND",
                "PO_LINE_NOT_FOUND",
                "QUANTITY_EXCEEDS_ORDERED",
                "NO_SUPPLIER_ITEM",
                "NO_UNIT_CONVERSION",
                "DIMENSION_MISMATCH",
                "SUPPLIER_NOT_FOUND",
            ]
        );
    }
}
