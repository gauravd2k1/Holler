//! Port of `backend/internal/compliance/snapshot.go` — Task 5: the
//! `tax_snapshot` an invoice stores (§31). §31 requires a historical bill
//! stay reproducible after the rules change, which is only possible if the
//! bill records which ruleset produced it — not just the numbers that fell
//! out of applying it. This is deliberately more than the totals: it names
//! the compliance version, the profile and every resolved rate, so a
//! reprint six months later shows the original rules verbatim even if the
//! outlet's current rates have since moved on.

use std::collections::HashMap;

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{json, Value};

use crate::error::DbResult;
use crate::model::{ComplianceVersion, TaxProfile, TaxRule};

use super::resolve::{resolve_compliance_version, resolve_rates};
use super::{Line, ResolvedRate};

/// One profile's resolved ruleset, as it stood at `resolved_at`. Field names
/// match `compliance.TaxSnapshot`'s JSON tags exactly, since this is the
/// shape both engines must serialize identically for the invoice's
/// `tax_snapshot_json` column to be meaningful regardless of which engine
/// wrote it.
#[derive(Debug, Clone)]
pub struct TaxSnapshot {
    pub compliance_version_id: String,
    pub compliance_version_label: String,
    pub tax_profile_id: String,
    pub tax_profile_code: String,
    pub pricing_mode: String,
    pub rates: Vec<ResolvedRate>,
    pub resolved_at: DateTime<Utc>,
}

impl TaxSnapshot {
    /// Renders this snapshot as JSON, field names matching
    /// `compliance.TaxSnapshot`'s `json:"..."` tags exactly:
    /// `compliance_version_id`, `compliance_version_label`, `tax_profile_id`,
    /// `tax_profile_code`, `pricing_mode`, `rates`, `resolved_at`.
    pub fn to_json(&self) -> Value {
        json!({
            "compliance_version_id": self.compliance_version_id,
            "compliance_version_label": self.compliance_version_label,
            "tax_profile_id": self.tax_profile_id,
            "tax_profile_code": self.tax_profile_code,
            "pricing_mode": self.pricing_mode,
            "rates": self.rates.iter().map(|r| json!({
                "component": r.component.as_str(),
                "rate_bps": r.rate_bps,
            })).collect::<Vec<_>>(),
            "resolved_at": self.resolved_at.to_rfc3339_opts(SecondsFormat::Millis, true),
        })
    }
}

/// Assembles the snapshot for one profile/version/rate-set resolution. An
/// invoice whose lines span more than one tax profile records one
/// `TaxSnapshot` per distinct profile via `build_tax_snapshots` below.
pub fn build_tax_snapshot(
    version: &ComplianceVersion,
    profile: &TaxProfile,
    rates: Vec<ResolvedRate>,
    resolved_at: DateTime<Utc>,
) -> TaxSnapshot {
    TaxSnapshot {
        compliance_version_id: version.id.clone(),
        compliance_version_label: version.label.clone(),
        tax_profile_id: profile.id.clone(),
        tax_profile_code: profile.code.clone(),
        pricing_mode: profile.pricing_mode.clone(),
        rates,
        resolved_at,
    }
}

/// Renders every entry of a `build_tax_snapshots` result into the
/// `{tax_profile_id: {...that profile's TaxSnapshot as JSON...}}` shape
/// `invoice.tax_snapshot_json` stores.
///
/// This is the piece that keeps a mixed-rate bill reproducible in fact, not
/// just in name (§31): since 0.4.2 let different lines resolve to different
/// profiles, a snapshot naming only ONE of them would silently lose the
/// rules for every other line on the same invoice. Storing every profile
/// `build_tax_snapshots` found, keyed by id, is what makes a reprint six
/// months later able to show the correct historical rate for EVERY line,
/// not just whichever profile happened to be resolved last.
pub fn render_tax_snapshots(snapshots: &HashMap<String, TaxSnapshot>) -> Value {
    let mut map = serde_json::Map::with_capacity(snapshots.len());
    for (profile_id, snap) in snapshots {
        map.insert(profile_id.clone(), snap.to_json());
    }
    Value::Object(map)
}

/// Resolves and renders one `TaxSnapshot` per distinct `tax_profile_id`
/// used across `lines`, keyed by `tax_profile_id` so an invoice-assembly
/// caller (T7b) can look one up per line when assembling the final invoice.
/// Every profile referenced by a line must resolve or this returns an
/// error naming which one failed.
pub fn build_tax_snapshots(
    versions: &[ComplianceVersion],
    profiles: &[TaxProfile],
    rules: &[TaxRule],
    outlet_id: &str,
    lines: &[Line],
    at: DateTime<Utc>,
) -> DbResult<HashMap<String, TaxSnapshot>> {
    let version = resolve_compliance_version(versions, outlet_id, at)?;

    let profile_by_id: HashMap<&str, &TaxProfile> =
        profiles.iter().map(|p| (p.id.as_str(), p)).collect();

    let mut out: HashMap<String, TaxSnapshot> = HashMap::new();
    for line in lines {
        if out.contains_key(&line.tax_profile_id) {
            continue;
        }
        let Some(profile) = profile_by_id.get(line.tax_profile_id.as_str()) else {
            continue;
        };
        let rates = resolve_rates(rules, &profile.id, &version.id, at)?;
        out.insert(
            line.tax_profile_id.clone(),
            build_tax_snapshot(&version, profile, rates, at),
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tax::domain::PricingMode;
    use crate::tax::resolve::parse_utc;

    fn version(id: &str, outlet_id: &str) -> ComplianceVersion {
        ComplianceVersion {
            id: id.to_string(),
            outlet_id: outlet_id.to_string(),
            label: format!("{id}-label"),
            effective_from: "2026-01-01T00:00:00Z".to_string(),
            notes: None,
            config_version: 1,
        }
    }

    fn profile(id: &str, outlet_id: &str, code: &str) -> TaxProfile {
        TaxProfile {
            id: id.to_string(),
            outlet_id: outlet_id.to_string(),
            code: code.to_string(),
            name: code.to_string(),
            pricing_mode: "EXCLUSIVE".to_string(),
            is_default: false,
            is_active: true,
            config_version: 1,
        }
    }

    fn rule(profile_id: &str, version_id: &str, component: &str, rate_bps: i64) -> TaxRule {
        TaxRule {
            id: format!("{profile_id}-{version_id}-{component}"),
            tax_profile_id: profile_id.to_string(),
            compliance_version_id: version_id.to_string(),
            component: component.to_string(),
            rate_bps,
            effective_from: "2026-01-01T00:00:00Z".to_string(),
            effective_to: None,
            config_version: 1,
        }
    }

    fn line(order_item_id: &str, tax_profile_id: &str) -> Line {
        Line {
            order_item_id: order_item_id.to_string(),
            description: "line".to_string(),
            hsn_sac: None,
            quantity: 1,
            unit_price_paise: 1000,
            discount_per_unit_paise: 0,
            tax_profile_id: tax_profile_id.to_string(),
            pricing_mode: PricingMode::Exclusive,
            rates: vec![],
        }
    }

    /// The reproducibility invariant a mixed-rate bill depends on: a
    /// three-profile invoice must capture all THREE snapshots, not just
    /// whichever profile a naive "first line wins" implementation would
    /// keep. Fails against a defect that returns only one entry (or
    /// overwrites earlier entries) for a multi-profile line set.
    #[test]
    fn build_tax_snapshots_captures_every_profile_on_a_mixed_rate_bill() {
        let outlet_id = "outlet-1";
        let versions = vec![version("cv-1", outlet_id)];
        let profiles = vec![
            profile("p-food", outlet_id, "GST_5_FOOD"),
            profile("p-liquor", outlet_id, "GST_18_LIQUOR"),
            profile("p-cess", outlet_id, "GST_12_CESS"),
        ];
        let rules = vec![
            rule("p-food", "cv-1", "CGST", 250),
            rule("p-food", "cv-1", "SGST", 250),
            rule("p-liquor", "cv-1", "CGST", 900),
            rule("p-liquor", "cv-1", "SGST", 900),
            rule("p-cess", "cv-1", "CGST", 600),
            rule("p-cess", "cv-1", "SGST", 600),
            rule("p-cess", "cv-1", "CESS", 280),
        ];
        let lines = vec![
            line("item-1", "p-food"),
            line("item-2", "p-liquor"),
            line("item-3", "p-cess"),
            // A second line under an already-captured profile must not
            // produce a duplicate or a second resolution — exactly one
            // snapshot per distinct profile id.
            line("item-4", "p-food"),
        ];

        let at = parse_utc("2026-06-01T00:00:00Z").unwrap();
        let snapshots = build_tax_snapshots(&versions, &profiles, &rules, outlet_id, &lines, at)
            .expect("build snapshots");

        assert_eq!(
            snapshots.len(),
            3,
            "a three-profile bill must capture all three snapshots, not just one"
        );
        assert!(snapshots.contains_key("p-food"));
        assert!(snapshots.contains_key("p-liquor"));
        assert!(snapshots.contains_key("p-cess"));

        assert_eq!(snapshots["p-liquor"].tax_profile_code, "GST_18_LIQUOR");
        let liquor_rates: Vec<_> = snapshots["p-liquor"].rates.iter().map(|r| r.rate_bps).collect();
        assert_eq!(liquor_rates, vec![900, 900]);

        let cess_components: Vec<_> = snapshots["p-cess"]
            .rates
            .iter()
            .map(|r| r.component.as_str())
            .collect();
        assert_eq!(cess_components, vec!["CGST", "SGST", "CESS"]);

        // render_tax_snapshots must carry every profile through to JSON too
        // — the wire/storage shape, not just the in-memory map.
        let rendered = render_tax_snapshots(&snapshots);
        let obj = rendered.as_object().expect("object");
        assert_eq!(obj.len(), 3, "rendered JSON must carry all three profiles");
        assert!(obj.contains_key("p-food"));
        assert!(obj.contains_key("p-liquor"));
        assert!(obj.contains_key("p-cess"));
    }

    #[test]
    fn build_tax_snapshots_errors_when_a_referenced_compliance_version_is_missing() {
        let outlet_id = "outlet-1";
        let lines = vec![line("item-1", "p-food")];
        let err = build_tax_snapshots(&[], &[], &[], outlet_id, &lines, Utc::now())
            .expect_err("no compliance version at all must error");
        assert!(matches!(err, crate::error::DbError::InvalidInput(_)));
    }
}
