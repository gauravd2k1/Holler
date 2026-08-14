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
