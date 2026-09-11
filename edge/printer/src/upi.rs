//! UPI QR for the bill amount on the rendered HTML receipt (T11, demo build
//! only — see `docs/demo-kickoff.md` item 0b).
//!
//! **This is NOT a payment integration.** Nothing reads this QR back — no
//! code path reconciles a scan against a received payment. The cashier
//! still records the tender exactly as before; the QR only gives a
//! customer's UPI app something to point at.
//!
//! There is no UPI payee address anywhere in the frozen contract set
//! (v0.8.1) — `outlet_fiscal_profile` and every other config shape carry no
//! VPA field, and none is being added here: contracts are FROZEN and the
//! demo needs no contract change. The payee is read from process
//! environment instead, deliberately under **different variable names**
//! than the invoice-screen half (`apps/pos/src/domain/upi.ts`) uses:
//! Vite only bundles client-visible env vars prefixed `VITE_`, so that
//! side reads `VITE_HOLLER_DEMO_UPI_VPA` /
//! `VITE_HOLLER_DEMO_UPI_PAYEE_NAME`, while this native process reads the
//! unprefixed `HOLLER_DEMO_UPI_VPA` / `HOLLER_DEMO_UPI_PAYEE_NAME`.
//!
//! **Divergence risk, stated plainly:** these are two separate variables
//! that must be set to the same value or the on-screen QR and the printed
//! receipt's QR will show two different payees, and nothing in this
//! codebase detects that. Whatever sets up the demo environment must set
//! both `VITE_HOLLER_DEMO_UPI_VPA`/`HOLLER_DEMO_UPI_VPA` (and the payee-name
//! pair) from one source.
//!
//! This module deliberately reimplements
//! `apps/pos/src/domain/upi.ts::buildUpiPaymentLink` rather than sharing
//! code with it (no Rust/TypeScript code-sharing boundary exists in this
//! repo) — the two are pinned to agree by a shared test vector, asserted on
//! both sides (`tests::shared_vector_matches_the_pinned_typescript_output`
//! below and the vector added to `upi.test.ts`).

use crate::error::{PrinterError, PrinterResult};

/// Reads the demo-build UPI payee from process environment.
///
/// Returns `None` when no VPA is configured (unset or blank after
/// trimming) — callers must render NO QR in that case. A QR pointing at an
/// empty or wrong payee is worse than no QR at a client demo: it opens a
/// real payment app on a customer's phone aimed at nobody. Fail visibly
/// absent, never silently wrong (mirrors `readUpiDemoPayee` in
/// `apps/pos/src/domain/upi.ts`).
pub fn read_upi_demo_payee() -> Option<(String, String)> {
    let vpa = std::env::var("HOLLER_DEMO_UPI_VPA")
        .unwrap_or_default()
        .trim()
        .to_string();
    if vpa.is_empty() {
        return None;
    }
    let configured_name = std::env::var("HOLLER_DEMO_UPI_PAYEE_NAME")
        .unwrap_or_default()
        .trim()
        .to_string();
    let payee_name = if configured_name.is_empty() {
        vpa.clone()
    } else {
        configured_name
    };
    Some((vpa, payee_name))
}

/// Percent-encodes exactly as JavaScript's `encodeURIComponent` does:
/// every byte except the unreserved set `A-Z a-z 0-9 - _ . ! ~ * ' ( )` is
/// replaced by `%XX` (uppercase hex). Operates byte-by-byte over the UTF-8
/// encoding of `s`, so a multi-byte character is encoded one `%XX` per
/// byte, matching `encodeURIComponent`'s behaviour on non-ASCII input.
fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        let c = byte as char;
        if c.is_ascii_alphanumeric() || "-_.!~*'()".contains(c) {
            out.push(c);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Formats integer paise as a plain (no currency symbol) decimal rupee
/// string with exactly two digits after the point, e.g. `125550` ->
/// `"1255.50"`. Integer division/remainder only — never a float division
/// of paise by 100 (CLAUDE.md §Money). Mirrors
/// `formatPaiseAsPlainDecimal` in `apps/pos/src/domain/money.ts`, including
/// its rejection of a negative amount: a UPI request for a negative amount
/// is not a thing a payment app can act on.
fn format_paise_as_plain_decimal(paise: i64) -> PrinterResult<String> {
    if paise < 0 {
        return Err(PrinterError::InvalidInput(format!(
            "UPI amount must not be negative, got {paise} paise"
        )));
    }
    let rupees = paise / 100;
    let remainder_paise = paise % 100;
    Ok(format!("{rupees}.{remainder_paise:02}"))
}

/// Builds the standard UPI deep link for a fixed amount:
/// `upi://pay?pa=<vpa>&pn=<payee name>&am=<amount>&cu=INR&tn=<note>`.
///
/// Every parameter is URL-encoded individually. `amount_paise` is the
/// exact invoice total the `invoice` row stored — never recomputed —
/// converted to a plain two-decimal rupee string using integer arithmetic
/// only. `note` is the human-facing order/invoice number, never a UUID
/// (mirrors `buildUpiPaymentLink` in `apps/pos/src/domain/upi.ts` exactly;
/// see the shared test vector).
pub fn build_upi_payment_link(
    vpa: &str,
    payee_name: &str,
    amount_paise: i64,
    note: &str,
) -> PrinterResult<String> {
    let amount = format_paise_as_plain_decimal(amount_paise)?;
    Ok(format!(
        "upi://pay?pa={}&pn={}&am={}&cu=INR&tn={}",
        encode_uri_component(vpa),
        encode_uri_component(payee_name),
        encode_uri_component(&amount),
        encode_uri_component(note),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact vector pinned in the T11 brief — the string the
    /// TypeScript implementation (`apps/pos/src/domain/upi.ts`) was
    /// observed to produce and decode back out of a real rendered QR.
    /// Inputs: VPA `demo@upi`, payee name `Holler Demo Kitchen`, amount
    /// 125550 paise, note `A184`.
    #[test]
    fn shared_vector_matches_the_pinned_typescript_output() {
        let link =
            build_upi_payment_link("demo@upi", "Holler Demo Kitchen", 125550, "A184").unwrap();
        assert_eq!(
            link,
            "upi://pay?pa=demo%40upi&pn=Holler%20Demo%20Kitchen&am=1255.50&cu=INR&tn=A184"
        );
    }

    #[test]
    fn rejects_negative_amount() {
        assert!(build_upi_payment_link("demo@upi", "Demo", -100, "A184").is_err());
    }

    #[test]
    fn encodes_special_characters_in_payee_name() {
        let link = build_upi_payment_link("demo@upi", "Holler & Co", 100, "A184").unwrap();
        assert!(link.contains("pn=Holler%20%26%20Co"));
    }

    #[test]
    fn read_upi_demo_payee_is_none_when_vpa_unset_or_blank() {
        // SAFETY (test-only): `std::env::remove_var`/`set_var` are used only
        // inside single-threaded test functions in this module; no
        // production path calls them. Each test sets both variables it
        // reads to avoid leaking state from another test in the same
        // process (Rust test binaries run tests in threads sharing one
        // process-wide environment).
        std::env::remove_var("HOLLER_DEMO_UPI_VPA");
        std::env::remove_var("HOLLER_DEMO_UPI_PAYEE_NAME");
        assert!(read_upi_demo_payee().is_none());

        std::env::set_var("HOLLER_DEMO_UPI_VPA", "   ");
        assert!(read_upi_demo_payee().is_none());
        std::env::remove_var("HOLLER_DEMO_UPI_VPA");
    }

    #[test]
    fn read_upi_demo_payee_falls_back_to_vpa_as_display_name() {
        std::env::set_var("HOLLER_DEMO_UPI_VPA", "demo@upi");
        std::env::remove_var("HOLLER_DEMO_UPI_PAYEE_NAME");
        assert_eq!(
            read_upi_demo_payee(),
            Some(("demo@upi".to_string(), "demo@upi".to_string()))
        );
        std::env::remove_var("HOLLER_DEMO_UPI_VPA");
    }

    #[test]
    fn read_upi_demo_payee_uses_configured_name() {
        std::env::set_var("HOLLER_DEMO_UPI_VPA", "demo@upi");
        std::env::set_var("HOLLER_DEMO_UPI_PAYEE_NAME", "Holler Demo Kitchen");
        assert_eq!(
            read_upi_demo_payee(),
            Some(("demo@upi".to_string(), "Holler Demo Kitchen".to_string()))
        );
        std::env::remove_var("HOLLER_DEMO_UPI_VPA");
        std::env::remove_var("HOLLER_DEMO_UPI_PAYEE_NAME");
    }
}
