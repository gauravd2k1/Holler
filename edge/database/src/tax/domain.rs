//! Port of `backend/internal/compliance/domain.go`.

use crate::error::{DbError, DbResult};

/// One tax component. `rate_bps` on every rate/rule is integer basis points
/// (2.5% = 250), never a float — CLAUDE.md forbids floating point for money.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaxComponent {
    Cgst,
    Sgst,
    Igst,
    Cess,
}

impl TaxComponent {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaxComponent::Cgst => "CGST",
            TaxComponent::Sgst => "SGST",
            TaxComponent::Igst => "IGST",
            TaxComponent::Cess => "CESS",
        }
    }

    /// Parses the raw stored string (`tax_rule.component`, the schema's own
    /// `CHECK (component IN ('CGST','SGST','IGST','CESS'))`). An unknown
    /// value is a config error, not silently ignored — a row that reaches
    /// this function already passed the schema CHECK, so this only ever
    /// fails against genuinely malformed input.
    pub fn parse(s: &str) -> DbResult<Self> {
        match s {
            "CGST" => Ok(TaxComponent::Cgst),
            "SGST" => Ok(TaxComponent::Sgst),
            "IGST" => Ok(TaxComponent::Igst),
            "CESS" => Ok(TaxComponent::Cess),
            other => Err(DbError::InvalidInput(format!(
                "unknown tax component {other:?}"
            ))),
        }
    }
}

/// The fixed, deterministic iteration order for tax components: CGST, SGST,
/// IGST, then CESS stacked on top. Used everywhere a stable order matters
/// (largest-remainder distribution, snapshot rendering) so two runs over
/// identical input produce byte-identical output — matches
/// `compliance.componentOrder` exactly.
pub const COMPONENT_ORDER: [TaxComponent; 4] = [
    TaxComponent::Cgst,
    TaxComponent::Sgst,
    TaxComponent::Igst,
    TaxComponent::Cess,
];

/// Whether the menu price already contains the tax (`tax_profile.pricing_mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PricingMode {
    Inclusive,
    Exclusive,
}

impl PricingMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            PricingMode::Inclusive => "INCLUSIVE",
            PricingMode::Exclusive => "EXCLUSIVE",
        }
    }

    /// Parses the raw stored string (`tax_profile.pricing_mode`, the
    /// schema's own `CHECK`).
    pub fn parse(s: &str) -> DbResult<Self> {
        match s {
            "INCLUSIVE" => Ok(PricingMode::Inclusive),
            "EXCLUSIVE" => Ok(PricingMode::Exclusive),
            other => Err(DbError::InvalidInput(format!(
                "unknown pricing mode {other:?}"
            ))),
        }
    }
}

/// One tax component's rate as resolved for a moment in time (Task 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedRate {
    pub component: TaxComponent,
    pub rate_bps: i64,
}

/// One billable line handed to the engine. Money fields are per-unit so
/// that splitting a line's `quantity` across N split invoices (ADR-016 §4)
/// distributes gross/discount exactly, with no residual paise to
/// reconcile.
#[derive(Debug, Clone)]
pub struct Line {
    /// The order line this bills — what makes the split-group conservation
    /// property checkable (ADR-016 §4).
    pub order_item_id: String,
    pub description: String,
    pub hsn_sac: Option<String>,
    pub quantity: i64,
    /// Tax-INCLUSIVE or tax-EXCLUSIVE depending on `pricing_mode` (Task 4).
    pub unit_price_paise: i64,
    /// Subtracted from `unit_price_paise` before tax. Per-unit (not a whole-
    /// line lump) so a split of `quantity` distributes the discount exactly.
    pub discount_per_unit_paise: i64,
    pub tax_profile_id: String,
    pub pricing_mode: PricingMode,
    /// This line's resolved rates (Task 2's `resolve_rates` output), carried
    /// on the line because different lines in one invoice may sit under
    /// different tax profiles (e.g. a liquor item taxed differently from
    /// food on the same bill — the mixed-rate-bill case).
    pub rates: Vec<ResolvedRate>,
}

/// One computed `invoice_line`, field-for-field compatible with
/// `packages/contracts/sqlite/0006_m3_billing.sql`'s `invoice_line` columns
/// (minus `id`/`invoice_id`/`line_no`, which the invoice-assembly caller —
/// T7b — assigns).
#[derive(Debug, Clone)]
pub struct LineComputation {
    pub order_item_id: String,
    pub description: String,
    pub hsn_sac: Option<String>,
    pub quantity: i64,

    pub unit_price_paise: i64,
    pub gross_paise: i64,
    pub discount_paise: i64,
    pub taxable_value_paise: i64,

    pub tax_profile_id: String,

    pub cgst_rate_bps: i64,
    pub cgst_paise: i64,
    pub sgst_rate_bps: i64,
    pub sgst_paise: i64,
    pub igst_rate_bps: i64,
    pub igst_paise: i64,
    pub cess_rate_bps: i64,
    pub cess_paise: i64,

    pub total_paise: i64,
}

/// The invoice-level money summary, field-for-field compatible with the
/// money columns on `invoice` (`0006_m3_billing.sql`). These are the
/// AUTHORITATIVE totals (ADR-016 §3): computed from the raw, unrounded
/// per-component sum across every line and rounded once — never by summing
/// each line's own (separately rounded) display components.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InvoiceTotals {
    pub subtotal_paise: i64,
    pub discount_paise: i64,
    pub taxable_value_paise: i64,
    pub cgst_paise: i64,
    pub sgst_paise: i64,
    pub igst_paise: i64,
    pub cess_paise: i64,
    pub round_off_paise: i64,
    pub grand_total_paise: i64,
}

/// `component`'s rate within `rates`, `0` if `rates` carries no rule for it
/// (e.g. an IGST-only profile has no CGST/SGST rule).
pub(super) fn rate_for(rates: &[ResolvedRate], component: TaxComponent) -> i64 {
    rates
        .iter()
        .find(|r| r.component == component)
        .map(|r| r.rate_bps)
        .unwrap_or(0)
}

pub(super) fn sum_rate_bps(rates: &[ResolvedRate]) -> i64 {
    rates.iter().map(|r| r.rate_bps).sum()
}

pub(super) fn component_value(lc: &LineComputation, c: TaxComponent) -> i64 {
    match c {
        TaxComponent::Cgst => lc.cgst_paise,
        TaxComponent::Sgst => lc.sgst_paise,
        TaxComponent::Igst => lc.igst_paise,
        TaxComponent::Cess => lc.cess_paise,
    }
}

pub(super) fn set_component_value(lc: &mut LineComputation, c: TaxComponent, v: i64) {
    match c {
        TaxComponent::Cgst => lc.cgst_paise = v,
        TaxComponent::Sgst => lc.sgst_paise = v,
        TaxComponent::Igst => lc.igst_paise = v,
        TaxComponent::Cess => lc.cess_paise = v,
    }
}
