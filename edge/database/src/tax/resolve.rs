//! Port of `backend/internal/compliance/resolve.go`: `ComplianceVersion` /
//! `TaxProfile` / `TaxRule` resolution. Rules are effective-dated —
//! resolution at a past instant must return what was true then, not what is
//! true now. These functions are pure: they take slices already loaded (by
//! `repo::list_*`, or by a test) and never touch the database themselves.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::error::{DbError, DbResult};
use crate::model::{ComplianceVersion, TaxProfile, TaxRule};

use super::domain::{TaxComponent, COMPONENT_ORDER};
use super::ResolvedRate;

/// Parses a stored ISO8601 UTC `String` (`effective_from`/`effective_to`
/// etc.) into a true instant for comparison. Every timestamp column in this
/// crate is stored as `TEXT`; this is the one place resolution needs a real
/// instant rather than a string round-trip.
pub fn parse_utc(s: &str) -> DbResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| DbError::InvalidInput(format!("invalid timestamp {s:?}: {e}")))
}

/// Returns the `ComplianceVersion` effective for `outlet_id` at instant
/// `at`: the version with the latest `effective_from` that is still `<= at`.
/// §31 requires a historical bill stay reproducible after the rules change,
/// which is only possible if "effective at a past instant" really does mean
/// the past ruleset, not the current one. The `effective_from == at`
/// boundary is INCLUSIVE (the version takes effect AT that instant).
pub fn resolve_compliance_version(
    versions: &[ComplianceVersion],
    outlet_id: &str,
    at: DateTime<Utc>,
) -> DbResult<ComplianceVersion> {
    let mut best: Option<(&ComplianceVersion, DateTime<Utc>)> = None;
    for v in versions {
        if v.outlet_id != outlet_id {
            continue;
        }
        let effective_from = parse_utc(&v.effective_from)?;
        if effective_from > at {
            continue;
        }
        let is_better = match best {
            None => true,
            Some((_, best_from)) => effective_from > best_from,
        };
        if is_better {
            best = Some((v, effective_from));
        }
    }
    best.map(|(v, _)| v.clone()).ok_or_else(|| {
        DbError::InvalidInput(format!(
            "no compliance version effective for outlet {outlet_id} at {}",
            at.to_rfc3339()
        ))
    })
}

/// Returns the tax profile that applies for `outlet_id` at `at`, given
/// `item_tax_profile_id` — the item's OWN pinned profile
/// (`menu_item.tax_profile_id`, contracts 0.4.2). The caller (whoever
/// already loaded the `MenuItem`) passes its `tax_profile_id` straight
/// through rather than this function looking an item up itself.
///
/// `None` means "use the outlet's default profile" (0.4.2's own wording),
/// which is what keeps the common single-rate restaurant configuration-
/// free — nothing to set on any item, every line resolves to the one
/// outlet-wide profile with no per-item data at all.
///
/// A `Some` `item_tax_profile_id` that names a profile which doesn't
/// resolve (wrong outlet, inactive, or simply absent from `profiles`) is a
/// config error and returns one — it must NEVER silently fall back to the
/// outlet default, which would hide exactly the kind of misconfiguration a
/// mixed-rate menu (e.g. a liquor item pinned to the wrong profile) most
/// needs surfaced.
pub fn resolve_tax_profile(
    profiles: &[TaxProfile],
    outlet_id: &str,
    item_tax_profile_id: Option<&str>,
    _at: DateTime<Utc>,
) -> DbResult<TaxProfile> {
    // TaxProfile itself is not effective-dated; its TaxRules are (`_at` is
    // accepted for symmetry with the Go signature and for callers that pass
    // `at` through uniformly, even though this function does not use it).
    if let Some(pinned) = item_tax_profile_id {
        return profiles
            .iter()
            .find(|p| p.id == pinned && p.outlet_id == outlet_id && p.is_active)
            .cloned()
            .ok_or_else(|| {
                DbError::InvalidInput(format!(
                    "item's tax_profile_id {pinned} is not an active tax profile for outlet {outlet_id}"
                ))
            });
    }

    profiles
        .iter()
        .find(|p| p.outlet_id == outlet_id && p.is_default && p.is_active)
        .cloned()
        .ok_or_else(|| {
            DbError::InvalidInput(format!(
                "no active default tax profile for outlet {outlet_id}"
            ))
        })
}

/// Returns one `ResolvedRate` per component effective for `profile_id`
/// under `compliance_version_id` at instant `at`: for each component
/// present in `rules`, the rule with the latest `effective_from <= at`
/// whose `effective_to` is `None` or strictly after `at`.
/// `effective_from == at` is INCLUSIVE, `effective_to == at` is EXCLUSIVE
/// (the old rate no longer applies at that instant).
///
/// Returns an empty `Vec` (not an error) if the profile+version combination
/// carries no rules at all — an unusual but not invalid state (e.g. a
/// zero-rated item) — but returns an error if `profile_id`/
/// `compliance_version_id` do not appear in `rules` together at all, since
/// that combination is typically a caller mistake (wrong version pinned to
/// a profile).
pub fn resolve_rates(
    rules: &[TaxRule],
    profile_id: &str,
    compliance_version_id: &str,
    at: DateTime<Utc>,
) -> DbResult<Vec<ResolvedRate>> {
    let mut latest: HashMap<TaxComponent, (&TaxRule, DateTime<Utc>)> = HashMap::new();
    let mut any_for_pair = false;

    for r in rules {
        if r.tax_profile_id != profile_id || r.compliance_version_id != compliance_version_id {
            continue;
        }
        any_for_pair = true;

        let effective_from = parse_utc(&r.effective_from)?;
        if effective_from > at {
            continue;
        }
        if let Some(to) = &r.effective_to {
            let effective_to = parse_utc(to)?;
            if effective_to <= at {
                continue;
            }
        }

        let component = TaxComponent::parse(&r.component)?;
        let is_better = match latest.get(&component) {
            None => true,
            Some((_, cur_from)) => effective_from > *cur_from,
        };
        if is_better {
            latest.insert(component, (r, effective_from));
        }
    }

    if !any_for_pair {
        return Err(DbError::InvalidInput(format!(
            "no tax rules for profile {profile_id} under compliance version {compliance_version_id}"
        )));
    }

    let mut out = Vec::with_capacity(latest.len());
    for &component in COMPONENT_ORDER.iter() {
        if let Some((rule, _)) = latest.get(&component) {
            out.push(ResolvedRate {
                component,
                rate_bps: rule.rate_bps,
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(id: &str, outlet_id: &str, is_default: bool, is_active: bool) -> TaxProfile {
        TaxProfile {
            id: id.to_string(),
            outlet_id: outlet_id.to_string(),
            code: id.to_string(),
            name: id.to_string(),
            pricing_mode: "EXCLUSIVE".to_string(),
            is_default,
            is_active,
            config_version: 1,
        }
    }

    fn version(id: &str, outlet_id: &str, effective_from: &str) -> ComplianceVersion {
        ComplianceVersion {
            id: id.to_string(),
            outlet_id: outlet_id.to_string(),
            label: id.to_string(),
            effective_from: effective_from.to_string(),
            notes: None,
            config_version: 1,
        }
    }

    fn rule(
        profile_id: &str,
        version_id: &str,
        component: &str,
        rate_bps: i64,
        effective_from: &str,
        effective_to: Option<&str>,
    ) -> TaxRule {
        TaxRule {
            id: format!("{profile_id}-{version_id}-{component}-{effective_from}"),
            tax_profile_id: profile_id.to_string(),
            compliance_version_id: version_id.to_string(),
            component: component.to_string(),
            rate_bps,
            effective_from: effective_from.to_string(),
            effective_to: effective_to.map(|s| s.to_string()),
            config_version: 1,
        }
    }

    // ---------------------------------------------------- resolve_tax_profile --

    /// A pin that names an active profile at the right outlet must resolve
    /// to THAT profile, even though a default also exists — the liquor-item-
    /// on-a-food-default-outlet case ADR-016 names explicitly.
    #[test]
    fn pin_set_and_valid_resolves_to_pinned_profile_not_default() {
        let profiles = vec![
            profile("p-default", "outlet-1", true, true),
            profile("p-liquor", "outlet-1", false, true),
        ];
        let got = resolve_tax_profile(&profiles, "outlet-1", Some("p-liquor"), Utc::now())
            .expect("pinned profile must resolve");
        assert_eq!(got.id, "p-liquor");
    }

    /// The critical invariant: a pin that is set but does not resolve must
    /// ERROR, never silently fall back to the outlet default. This test
    /// fails against a defect that swaps the `.ok_or_else(...)` error for a
    /// fallback to the default profile — the exact silent-fallback shape a
    /// liquor-item-taxed-as-food misconfiguration would produce.
    #[test]
    fn pin_set_but_profile_inactive_errors_never_falls_back_to_default() {
        let profiles = vec![
            profile("p-default", "outlet-1", true, true),
            profile("p-retired", "outlet-1", false, false), // inactive
        ];
        let err = resolve_tax_profile(&profiles, "outlet-1", Some("p-retired"), Utc::now())
            .expect_err("an inactive pinned profile must error, not silently fall back");
        assert!(
            matches!(err, DbError::InvalidInput(_)),
            "expected InvalidInput, got {err:?}"
        );
    }

    /// A pin naming a profile that belongs to a DIFFERENT outlet is a
    /// tenancy boundary, not a lookup miss to paper over. It must error,
    /// never resolve cross-outlet and never fall back to this outlet's
    /// default.
    #[test]
    fn pin_set_but_profile_belongs_to_different_outlet_errors() {
        let profiles = vec![
            profile("p-default", "outlet-1", true, true),
            profile("p-other-outlet", "outlet-2", true, true),
        ];
        let err = resolve_tax_profile(&profiles, "outlet-1", Some("p-other-outlet"), Utc::now())
            .expect_err("a profile belonging to a different outlet must error");
        assert!(matches!(err, DbError::InvalidInput(_)));
    }

    /// A pin naming a profile absent from `profiles` altogether (deleted,
    /// or never synced) must error the same way as inactive/wrong-outlet —
    /// no special-cased silent fallback for "not found" either.
    #[test]
    fn pin_set_but_profile_absent_entirely_errors() {
        let profiles = vec![profile("p-default", "outlet-1", true, true)];
        let err = resolve_tax_profile(&profiles, "outlet-1", Some("does-not-exist"), Utc::now())
            .expect_err("a pin naming a nonexistent profile must error");
        assert!(matches!(err, DbError::InvalidInput(_)));
    }

    /// `None` — the common case — resolves to the outlet's active default,
    /// with zero per-item configuration.
    #[test]
    fn pin_none_resolves_to_outlet_default() {
        let profiles = vec![
            profile("p-nondefault", "outlet-1", false, true),
            profile("p-default", "outlet-1", true, true),
        ];
        let got = resolve_tax_profile(&profiles, "outlet-1", None, Utc::now())
            .expect("default profile must resolve");
        assert_eq!(got.id, "p-default");
    }

    #[test]
    fn pin_none_with_no_active_default_errors() {
        let profiles = vec![profile("p-inactive-default", "outlet-1", true, false)];
        let err = resolve_tax_profile(&profiles, "outlet-1", None, Utc::now())
            .expect_err("no active default must error");
        assert!(matches!(err, DbError::InvalidInput(_)));
    }

    // ----------------------------------------- resolve_compliance_version --

    /// `at == effective_from` is INCLUSIVE: the version takes effect AT that
    /// instant, not strictly after it. An off-by-one here misprices every
    /// bill issued in the same instant as a rate-change rollout.
    #[test]
    fn compliance_version_effective_from_boundary_is_inclusive() {
        let change = parse_utc("2026-04-01T00:00:00Z").unwrap();
        let versions = vec![
            version("v1", "outlet-1", "2025-01-01T00:00:00Z"),
            version("v2", "outlet-1", "2026-04-01T00:00:00Z"),
        ];

        let at_boundary =
            resolve_compliance_version(&versions, "outlet-1", change).expect("resolve at boundary");
        assert_eq!(
            at_boundary.id, "v2",
            "at == effective_from must resolve to the NEW version (inclusive boundary)"
        );

        let just_before = change - chrono::Duration::nanoseconds(1);
        let before_boundary = resolve_compliance_version(&versions, "outlet-1", just_before)
            .expect("resolve just before boundary");
        assert_eq!(
            before_boundary.id, "v1",
            "one instant before effective_from must still resolve to the OLD version"
        );
    }

    #[test]
    fn compliance_version_resolves_past_instant_to_past_ruleset() {
        let versions = vec![
            version("v1", "outlet-1", "2025-01-01T00:00:00Z"),
            version("v2", "outlet-1", "2026-04-01T00:00:00Z"),
            version("v-other-outlet", "outlet-2", "2025-06-01T00:00:00Z"),
        ];

        let got = resolve_compliance_version(
            &versions,
            "outlet-1",
            parse_utc("2025-06-01T00:00:00Z").unwrap(),
        )
        .expect("resolve");
        assert_eq!(
            got.id, "v1",
            "a past instant must return the ruleset live then"
        );

        let err = resolve_compliance_version(
            &versions,
            "outlet-1",
            parse_utc("2024-01-01T00:00:00Z").unwrap(),
        )
        .expect_err("before any version existed must error");
        assert!(matches!(err, DbError::InvalidInput(_)));
    }

    // -------------------------------------------------------- resolve_rates --

    /// `at == effective_from` is INCLUSIVE, `at == effective_to` is
    /// EXCLUSIVE (the old rate no longer applies at that instant). This is
    /// the pair of boundaries a rate-change rollout hinges on: at the exact
    /// changeover instant, the OLD rule must have stopped applying and the
    /// NEW rule must have started, never both or neither.
    #[test]
    fn rates_effective_from_and_effective_to_boundaries() {
        let change = parse_utc("2026-01-01T00:00:00Z").unwrap();
        let rules = vec![
            rule(
                "profile-1",
                "version-1",
                "CGST",
                250,
                "2025-01-01T00:00:00Z",
                Some("2026-01-01T00:00:00Z"),
            ),
            rule(
                "profile-1",
                "version-1",
                "CGST",
                900,
                "2026-01-01T00:00:00Z",
                None,
            ),
        ];

        let at_boundary = resolve_rates(&rules, "profile-1", "version-1", change).expect("resolve");
        assert_eq!(
            rate_of(&at_boundary, TaxComponent::Cgst),
            900,
            "at == effective_to/effective_from boundary must resolve to the NEW rate"
        );

        let just_before = change - chrono::Duration::nanoseconds(1);
        let before_boundary =
            resolve_rates(&rules, "profile-1", "version-1", just_before).expect("resolve");
        assert_eq!(
            rate_of(&before_boundary, TaxComponent::Cgst),
            250,
            "one instant before the boundary must still resolve to the OLD rate"
        );
    }

    #[test]
    fn rates_unknown_profile_version_pair_errors() {
        let rules = vec![rule(
            "profile-1",
            "version-1",
            "CGST",
            250,
            "2025-01-01T00:00:00Z",
            None,
        )];
        let err = resolve_rates(
            &rules,
            "unknown-profile",
            "version-1",
            parse_utc("2026-01-01T00:00:00Z").unwrap(),
        )
        .expect_err("an unknown profile/version pair must error");
        assert!(matches!(err, DbError::InvalidInput(_)));
    }

    #[test]
    fn rates_component_with_no_rule_is_absent_not_zero_entry() {
        let rules = vec![rule(
            "profile-1",
            "version-1",
            "IGST",
            1200,
            "2025-01-01T00:00:00Z",
            None,
        )];
        let got = resolve_rates(
            &rules,
            "profile-1",
            "version-1",
            parse_utc("2026-01-01T00:00:00Z").unwrap(),
        )
        .expect("resolve");
        assert_eq!(
            got.len(),
            1,
            "only IGST has a rule; CGST/SGST/CESS must be absent entirely"
        );
        assert_eq!(got[0].component, TaxComponent::Igst);
    }

    fn rate_of(rates: &[ResolvedRate], component: TaxComponent) -> i64 {
        rates
            .iter()
            .find(|r| r.component == component)
            .map(|r| r.rate_bps)
            .unwrap_or(0)
    }
}
