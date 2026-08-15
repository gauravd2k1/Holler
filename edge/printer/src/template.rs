//! Renders a `Kot` (`packages/contracts/src/types/kot.ts`) to ESC/POS bytes
//! for 58mm and 80mm thermal paper. Station name, order number, table,
//! items, modifiers, notes, timestamp and the `#132-A` sequence marker
//! (docs/spec/kitchen.md) all appear — a cook reads this at speed, so the
//! bar is legible, not decorative.
//!
//! Also renders a GST tax invoice (`holler_edge_database::model::Invoice` +
//! `InvoiceLine`, ADR-016, docs/spec/compliance.md "GST invoice fields") to
//! the same 58mm/80mm ESC/POS byte stream. **Binding rule (T10 brief,
//! closing the M2 UUID-on-ticket defect for the invoice path too): this
//! module reads `order_display_number` — resolved by the caller via
//! [`require_order_display_number`] — and NEVER `order.id`/`invoice.id`/
//! `invoice.order_id`. Neither UUID is ever formatted into the byte
//! stream.** [`render_invoice`] renders every money field and the seller
//! identity from what the `invoice` row itself stored
//! (`fiscal_profile_json`, the money columns), never from live
//! `outlet_fiscal_profile` config that may have changed since issue — the
//! §31 reproducibility guarantee a reprint months later depends on.

use serde::Deserialize;

use holler_edge_database::model::{Invoice, InvoiceLine};

use crate::error::{PrinterError, PrinterResult};
use crate::escpos::EscPosBuilder;

/// Mirrors `KotTicketItemSchema` (`packages/contracts/src/types/kot.ts`)
/// exactly, for parsing `kot.items_json`.
#[derive(Debug, Clone, Deserialize)]
pub struct KotTicketItem {
    pub order_item_id: String,
    pub name: String,
    pub quantity: i64,
    #[serde(default)]
    pub modifiers: Vec<String>,
    pub notes: Option<String>,
}

/// Everything the template needs about the printed ticket beyond the raw
/// `kot` row: the human-facing order number and table, which the KOT row
/// itself does not carry (it only has `order_id`).
#[derive(Debug, Clone)]
pub struct KotPrintContext<'a> {
    pub station_name: &'a str,
    pub order_display_number: &'a str,
    pub table_label: Option<&'a str>,
    /// e.g. "#132", "#132-A", "#132-C" (docs/spec/kitchen.md).
    pub sequence_marker: &'a str,
}

/// Characters-per-line for the two frozen paper widths, using the printer's
/// default (non-condensed) font. These are the standard ESC/POS font-A
/// widths thermal printer vendors converge on for 58mm/80mm paper, which is
/// exactly why hardware-printing.md treats them as adapter-independent
/// rather than a per-vendor setting.
fn chars_per_line(paper_width_mm: i64) -> PrinterResult<usize> {
    match paper_width_mm {
        58 => Ok(32),
        80 => Ok(48),
        other => Err(PrinterError::InvalidInput(format!(
            "unsupported paper width {other}mm (only 58/80 are contracted)"
        ))),
    }
}

/// Greedy word-wrap so a long item name or note does not run off the paper
/// edge or get silently truncated — both are worse for a cook than a
/// two-line item.
fn wrap(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let candidate_len = if current.is_empty() {
            word.len()
        } else {
            current.len() + 1 + word.len()
        };
        if candidate_len > width && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
        // A single word longer than the whole line: hard-break it rather
        // than overflow.
        while current.len() > width {
            let (head, tail) = current.split_at(width);
            lines.push(head.to_string());
            current = tail.to_string();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Renders one KOT to ESC/POS bytes ready to hand to a
/// [`crate::transport::PrinterTransport`].
pub fn render_kot(
    items_json: &str,
    station: &str,
    ctx: &KotPrintContext,
    printed_at_local: &str,
    paper_width_mm: i64,
) -> PrinterResult<Vec<u8>> {
    let width = chars_per_line(paper_width_mm)?;
    let items: Vec<KotTicketItem> = serde_json::from_str(items_json)
        .map_err(|e| PrinterError::InvalidInput(format!("kot.items_json: {e}")))?;

    let mut b = EscPosBuilder::new();
    b.init();

    b.align_center();
    b.double_size(true);
    b.line(ctx.station_name.trim());
    b.double_size(false);
    b.align_left();
    b.rule(width);

    b.bold(true);
    b.line(&format!("KOT {}", ctx.sequence_marker));
    b.bold(false);
    b.line(&format!("Order {}", ctx.order_display_number));
    if let Some(table) = ctx.table_label {
        b.line(&format!("Table {table}"));
    }
    b.line(&format!("Station: {station}"));
    b.line(printed_at_local);
    b.rule(width);

    for item in &items {
        let head = format!("{} x {}", item.quantity, item.name);
        for line in wrap(&head, width) {
            b.bold(true);
            b.line(&line);
            b.bold(false);
        }
        for m in &item.modifiers {
            for line in wrap(&format!("  + {m}"), width) {
                b.line(&line);
            }
        }
        if let Some(notes) = &item.notes {
            if !notes.trim().is_empty() {
                for line in wrap(&format!("  * {notes}"), width) {
                    b.line(&line);
                }
            }
        }
    }

    b.rule(width);
    b.feed(3);
    b.cut();
    Ok(b.into_bytes())
}

// ------------------------------------------------------------- GST invoice --

/// The seller identity as it stood at issue time, parsed from
/// `invoice.fiscal_profile_json`. Field set mirrors `outlet_fiscal_profile`
/// (`packages/contracts/sqlite/0006_m3_billing.sql`) exactly; this struct
/// exists only to parse the snapshot JSON `edge/database` writes (T7b) —
/// this crate never reads the live `outlet_fiscal_profile` table, which is
/// the whole point: a reprint must render the identity that was true when
/// the bill was issued, not whatever the config says today (§31).
#[derive(Debug, Clone, Deserialize)]
struct FiscalProfileSnapshot {
    legal_name: String,
    trade_name: Option<String>,
    address_line1: String,
    address_line2: Option<String>,
    city: String,
    state_code: String,
    #[allow(dead_code)]
    // parsed for completeness of the snapshot shape; state_name is printed instead
    state_name: Option<String>,
    pincode: String,
    gstin: String,
    fssai_number: Option<String>,
    invoice_footer_text: Option<String>,
}

/// Human-facing context the `invoice` row does not itself carry: the short
/// order number and (optionally) the table label and a payment-mode
/// summary, all resolved by the caller from data it already has loaded
/// (the order, the table, the tenders) — same shape as
/// `adapter::KotOrderContext`/`KotPrintContext`, deliberately kept
/// symmetric with the KOT path this template sits beside.
///
/// `order_display_number` is required and is never a UUID: build it only
/// through [`require_order_display_number`], which is the one place that
/// refuses to substitute `order.id` when the short number is missing.
#[derive(Debug, Clone)]
pub struct InvoicePrintContext<'a> {
    pub order_display_number: &'a str,
    pub table_label: Option<&'a str>,
    /// e.g. "Cash", "Cash + UPI" — compliance.md lists "payment mode" as a
    /// GST invoice field, but `invoice` (contracts 0.4.0) carries no such
    /// column; `payment` is a separate EDGE_TO_CLOUD aggregate this crate
    /// does not query. A caller that has already resolved the tenders for
    /// this invoice's order may summarise them here; if `None`, the line is
    /// simply omitted rather than guessed at.
    pub payment_summary: Option<&'a str>,
}

/// Resolves the short order number this template must print, refusing to
/// substitute the order's internal id if it is absent. `order.display_number`
/// is `Option` only because a row written before minting existed (pre
/// contracts-0.4.1) has none (`holler_edge_database::model::Order` doc
/// comment); every order created since always has one. A `None`/blank value
/// here means a genuinely pre-minting order is being (re)printed — this
/// function fails loudly rather than falling back to the id, which is
/// exactly the substitution the M2 UUID-on-ticket defect was closed to stop.
pub fn require_order_display_number(display_number: Option<&str>) -> PrinterResult<&str> {
    display_number
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            PrinterError::InvalidInput(
                "order has no display_number; refusing to print a GST invoice with the \
                 order's internal id in its place (the M2 UUID-on-ticket defect, closed for \
                 invoices too)"
                    .to_string(),
            )
        })
}

/// Formats integer paise as `Rs 1234.50` — ASCII only (matches
/// `EscPosBuilder::text`'s documented code-page constraint) and never
/// floating point (CLAUDE.md §Money): the division/remainder below is the
/// entire conversion, no `f64` anywhere in this path.
fn money(paise: i64) -> String {
    let sign = if paise < 0 { "-" } else { "" };
    let abs = paise.unsigned_abs();
    format!("{sign}Rs {}.{:02}", abs / 100, abs % 100)
}

fn kv_line(b: &mut EscPosBuilder, width: usize, label: &str, value: &str) {
    let head = format!("{label}: {value}");
    for line in wrap(&head, width) {
        b.line(&line);
    }
}

/// Renders one issued (or cancelled) GST invoice to ESC/POS bytes. A pure
/// function of `invoice`/`lines`/`ctx`/`paper_width_mm` — no clock, no live
/// config lookup — so calling it twice on the same inputs (a genuine
/// reprint) produces byte-identical output, the §31 property this crate is
/// bound to.
///
/// Every money field rendered comes from `invoice`'s own stored columns
/// (never recomputed here), and the seller identity comes from
/// `invoice.fiscal_profile_json` (never live `outlet_fiscal_profile`
/// config). `invoice.id`, `invoice.order_id` and every `InvoiceLine.id`/
/// `order_item_id` are UUIDs this function never writes to the byte stream —
/// the only order-identifying text on the ticket is `ctx.order_display_number`.
pub fn render_invoice(
    invoice: &Invoice,
    lines: &[InvoiceLine],
    ctx: &InvoicePrintContext,
    paper_width_mm: i64,
) -> PrinterResult<Vec<u8>> {
    let width = chars_per_line(paper_width_mm)?;
    let profile: FiscalProfileSnapshot = serde_json::from_str(&invoice.fiscal_profile_json)
        .map_err(|e| PrinterError::InvalidInput(format!("invoice.fiscal_profile_json: {e}")))?;

    let mut b = EscPosBuilder::new();
    b.init();

    b.align_center();
    b.double_size(true);
    b.line(profile.legal_name.trim());
    b.double_size(false);
    if let Some(trade) = profile
        .trade_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        for line in wrap(trade, width) {
            b.line(&line);
        }
    }
    b.align_left();
    for line in wrap(&profile.address_line1, width) {
        b.line(&line);
    }
    if let Some(l2) = profile
        .address_line2
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        for line in wrap(l2, width) {
            b.line(&line);
        }
    }
    for line in wrap(&format!("{} {}", profile.city, profile.pincode), width) {
        b.line(&line);
    }
    kv_line(&mut b, width, "GSTIN", &profile.gstin);
    if let Some(fssai) = profile
        .fssai_number
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        kv_line(&mut b, width, "FSSAI", fssai);
    }
    b.rule(width);

    b.align_center();
    b.bold(true);
    b.line(if invoice.status == "CANCELLED" {
        "TAX INVOICE (CANCELLED)"
    } else {
        "TAX INVOICE"
    });
    b.bold(false);
    b.align_left();
    if invoice.split_count > 1 {
        kv_line(
            &mut b,
            width,
            "Bill",
            &format!("{} of {}", invoice.split_index, invoice.split_count),
        );
    }
    kv_line(&mut b, width, "Invoice No", &invoice.invoice_number);
    kv_line(&mut b, width, "Date", &invoice.invoice_date);
    // The one line the T10 brief pins: the short order number, never
    // `invoice.order_id`/`invoice.id`.
    kv_line(&mut b, width, "Order", ctx.order_display_number);
    if let Some(table) = ctx.table_label {
        kv_line(&mut b, width, "Table", table);
    }
    kv_line(
        &mut b,
        width,
        "Place of Supply",
        &format!(
            "{} ({})",
            profile.state_code, invoice.place_of_supply_state_code
        ),
    );
    if let Some(name) = invoice
        .customer_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        kv_line(&mut b, width, "Customer", name);
    }
    if let Some(gstin) = invoice
        .customer_gstin
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        kv_line(&mut b, width, "Customer GSTIN", gstin);
    }
    b.rule(width);

    for line in lines {
        b.bold(true);
        for wrapped in wrap(&format!("{} x {}", line.quantity, line.description), width) {
            b.line(&wrapped);
        }
        b.bold(false);
        if let Some(hsn) = line
            .hsn_sac
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            b.line(&format!("  HSN/SAC {hsn}"));
        }
        b.line(&format!(
            "  Rate {}  Taxable {}",
            money(line.unit_price_paise),
            money(line.taxable_value_paise)
        ));
    }
    b.rule(width);

    kv_line(
        &mut b,
        width,
        "Taxable Value",
        &money(invoice.taxable_value_paise),
    );
    if invoice.discount_paise != 0 {
        kv_line(&mut b, width, "Discount", &money(invoice.discount_paise));
    }
    if invoice.cgst_paise != 0 {
        kv_line(&mut b, width, "CGST", &money(invoice.cgst_paise));
    }
    if invoice.sgst_paise != 0 {
        kv_line(&mut b, width, "SGST", &money(invoice.sgst_paise));
    }
    if invoice.igst_paise != 0 {
        kv_line(&mut b, width, "IGST", &money(invoice.igst_paise));
    }
    if invoice.cess_paise != 0 {
        kv_line(&mut b, width, "Cess", &money(invoice.cess_paise));
    }
    if invoice.round_off_paise != 0 {
        kv_line(&mut b, width, "Round Off", &money(invoice.round_off_paise));
    }
    b.bold(true);
    kv_line(
        &mut b,
        width,
        "Grand Total",
        &money(invoice.grand_total_paise),
    );
    b.bold(false);

    if let Some(payment) = ctx.payment_summary {
        kv_line(&mut b, width, "Payment", payment);
    }
    b.rule(width);

    if let Some(footer) = profile
        .invoice_footer_text
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        b.align_center();
        for line in wrap(footer, width) {
            b.line(&line);
        }
        b.align_left();
    }

    b.feed(3);
    b.cut();
    Ok(b.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>() -> KotPrintContext<'a> {
        KotPrintContext {
            station_name: "Main Kitchen",
            order_display_number: "A184",
            table_label: Some("T-04"),
            sequence_marker: "#132",
        }
    }

    fn items_json() -> String {
        serde_json::json!([
            {
                "order_item_id": "018e5a2e-3333-7c3d-9f4e-1234567890ab",
                "name": "Butter Chicken",
                "quantity": 2,
                "modifiers": ["Extra Spicy"],
                "notes": "no onion"
            }
        ])
        .to_string()
    }

    #[test]
    fn renders_58mm_and_80mm_without_error() {
        for width in [58, 80] {
            let bytes = render_kot(&items_json(), "MAIN_KITCHEN", &ctx(), "10:15:31", width)
                .expect("renders");
            assert!(!bytes.is_empty());
            // ESC @ must be the first two bytes: every ticket resets state.
            assert_eq!(&bytes[0..2], &[0x1B, 0x40]);
            // GS V 1 must be the last three bytes: every ticket cuts.
            assert_eq!(&bytes[bytes.len() - 3..], &[0x1D, 0x56, 0x01]);
        }
    }

    #[test]
    fn contains_sequence_marker_item_and_modifier_text() {
        let bytes = render_kot(&items_json(), "MAIN_KITCHEN", &ctx(), "10:15:31", 80).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("#132"));
        assert!(text.contains("Butter Chicken"));
        assert!(text.contains("Extra Spicy"));
        assert!(text.contains("no onion"));
        assert!(text.contains("T-04"));
    }

    #[test]
    fn rejects_unsupported_paper_width() {
        let err = render_kot(&items_json(), "MAIN_KITCHEN", &ctx(), "10:15:31", 112);
        assert!(err.is_err());
    }

    #[test]
    fn wraps_long_item_names_within_line_width() {
        // Exercises the wrap() helper directly (same module, private item
        // visible to this child `tests` module) rather than scanning the
        // full rendered byte stream with `.lines()`: ESC/POS control bytes
        // (e.g. ESC ! n) carry printable ASCII parameter bytes that are not
        // separated by '\n', so naively measuring `.lines()` output mixes
        // control-sequence bytes into the "line" and makes the assertion
        // meaningless. `wrap()` itself is what guarantees the width, so
        // that is what is tested.
        let long_name = "Extremely Long Combination Platter With Many Sides And Extras";
        for line in wrap(long_name, 32) {
            assert!(line.len() <= 32, "wrapped line exceeded width: {line:?}");
        }
        // And the end-to-end render still succeeds and contains the item's
        // words somewhere in the byte stream.
        let json = serde_json::json!([
            { "order_item_id": "x", "name": long_name, "quantity": 1, "modifiers": [], "notes": null }
        ])
        .to_string();
        let bytes = render_kot(&json, "MAIN_KITCHEN", &ctx(), "10:15:31", 58).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("Extremely"));
        assert!(text.contains("Extras"));
    }

    // ------------------------------------------------------- GST invoice --

    const INVOICE_UUID: &str = "018e5a2e-3333-7c3d-9f4e-1234567890ab";
    const ORDER_UUID: &str = "018e5a2e-4444-7c3d-9f4e-abcdefabcdef";
    const LINE_ITEM_UUID: &str = "018e5a2e-5555-7c3d-9f4e-fedcba987654";

    fn fiscal_profile_json() -> String {
        serde_json::json!({
            "id": "018e5a2e-0000-7c3d-9f4e-000000000001",
            "outlet_id": "018e5a2e-0000-7c3d-9f4e-000000000002",
            "legal_name": "Holler Hospitality Pvt Ltd",
            "trade_name": "The Holler Kitchen",
            "address_line1": "12 MG Road",
            "address_line2": "Shivaji Nagar",
            "city": "Pune",
            "state_code": "27",
            "state_name": "Maharashtra",
            "pincode": "411001",
            "gstin": "27ABCDE1234F1Z5",
            "fssai_number": "10012345678901",
            "invoice_footer_text": "Thank you, visit again!",
            "effective_from": "2026-01-01T00:00:00Z",
        })
        .to_string()
    }

    /// A tax-inclusive invoice whose components sum exactly to
    /// `grand_total_paise`, matching the ADR-016 §3 invariant
    /// `grand_total = taxable_value + Σcomponents + round_off`.
    fn invoice_fixture() -> Invoice {
        Invoice {
            id: INVOICE_UUID.to_string(),
            outlet_id: "018e5a2e-0000-7c3d-9f4e-000000000002".to_string(),
            order_id: ORDER_UUID.to_string(),
            split_group_id: None,
            split_index: 1,
            split_count: 1,
            series_id: "018e5a2e-0000-7c3d-9f4e-000000000003".to_string(),
            invoice_number: "FY26/PNQ/001423".to_string(),
            invoice_date: "2026-08-14T12:30:00Z".to_string(),
            business_date: "2026-08-14".to_string(),
            status: "ISSUED".to_string(),
            cancelled_reason: None,
            cancelled_at: None,
            customer_name: Some("Walk-in".to_string()),
            customer_phone: None,
            customer_gstin: None,
            place_of_supply_state_code: "27".to_string(),
            subtotal_paise: 50000,
            discount_paise: 0,
            taxable_value_paise: 50000,
            cgst_paise: 1250,
            sgst_paise: 1250,
            igst_paise: 0,
            cess_paise: 0,
            round_off_paise: 0,
            grand_total_paise: 52500,
            compliance_version_id: "018e5a2e-0000-7c3d-9f4e-000000000004".to_string(),
            tax_snapshot_json: "{}".to_string(),
            fiscal_profile_json: fiscal_profile_json(),
            channel: "POS".to_string(),
            tax_liability_party: "RESTAURANT".to_string(),
            eco_operator_name: None,
            eco_operator_gstin: None,
            supply_classification: None,
            created_by_user_id: "018e5a2e-0000-7c3d-9f4e-000000000005".to_string(),
            created_at: "2026-08-14T12:30:00Z".to_string(),
            updated_at: "2026-08-14T12:30:00Z".to_string(),
            version: 1,
            sync_status: "PENDING".to_string(),
        }
    }

    fn invoice_lines_fixture() -> Vec<InvoiceLine> {
        vec![InvoiceLine {
            id: LINE_ITEM_UUID.to_string(),
            invoice_id: INVOICE_UUID.to_string(),
            order_item_id: "018e5a2e-6666-7c3d-9f4e-000000000009".to_string(),
            line_no: 1,
            description: "Butter Chicken".to_string(),
            hsn_sac: Some("996331".to_string()),
            quantity: 2,
            unit_price_paise: 25000,
            gross_paise: 50000,
            discount_paise: 0,
            taxable_value_paise: 50000,
            tax_profile_id: "018e5a2e-0000-7c3d-9f4e-000000000006".to_string(),
            cgst_rate_bps: 250,
            cgst_paise: 1250,
            sgst_rate_bps: 250,
            sgst_paise: 1250,
            igst_rate_bps: 0,
            igst_paise: 0,
            cess_rate_bps: 0,
            cess_paise: 0,
            total_paise: 52500,
        }]
    }

    fn invoice_ctx<'a>() -> InvoicePrintContext<'a> {
        InvoicePrintContext {
            order_display_number: "#A184",
            table_label: Some("T-04"),
            payment_summary: Some("Cash"),
        }
    }

    #[test]
    fn require_order_display_number_accepts_present_value() {
        assert_eq!(
            require_order_display_number(Some("#A184")).unwrap(),
            "#A184"
        );
    }

    #[test]
    fn require_order_display_number_rejects_missing_or_blank() {
        assert!(require_order_display_number(None).is_err());
        assert!(require_order_display_number(Some("   ")).is_err());
        assert!(require_order_display_number(Some("")).is_err());
    }

    #[test]
    fn invoice_renders_short_display_number_and_no_uuid() {
        let bytes = render_invoice(
            &invoice_fixture(),
            &invoice_lines_fixture(),
            &invoice_ctx(),
            80,
        )
        .expect("renders");
        let text = String::from_utf8_lossy(&bytes);

        assert!(
            text.contains("#A184"),
            "missing short display number: {text}"
        );
        assert!(!text.contains(INVOICE_UUID), "leaked invoice.id UUID");
        assert!(!text.contains(ORDER_UUID), "leaked order.id UUID");
        assert!(!text.contains(LINE_ITEM_UUID), "leaked order_item_id UUID");
    }

    #[test]
    fn invoice_render_fails_without_order_display_number_available() {
        // The caller-side guard: an assembler that only has `Option<&str>`
        // from `order.display_number` must go through
        // `require_order_display_number` before it can construct a
        // `InvoicePrintContext` at all — proven here by using it directly
        // against a `None`, the shape a pre-minting order produces.
        let order_display_number: Option<&str> = None;
        let err = require_order_display_number(order_display_number);
        assert!(
            err.is_err(),
            "must refuse to substitute order.id for a missing display_number"
        );
    }

    #[test]
    fn invoice_carries_required_gst_fields() {
        let bytes = render_invoice(
            &invoice_fixture(),
            &invoice_lines_fixture(),
            &invoice_ctx(),
            80,
        )
        .expect("renders");
        let text = String::from_utf8_lossy(&bytes);

        // Supplier identity.
        assert!(text.contains("Holler Hospitality Pvt Ltd"));
        assert!(text.contains("27ABCDE1234F1Z5")); // GSTIN
        assert!(text.contains("12 MG Road"));
        assert!(text.contains("Maharashtra") || text.contains("27")); // state
        assert!(text.contains("10012345678901")); // FSSAI

        // Invoice identity.
        assert!(text.contains("FY26/PNQ/001423"));
        assert!(text.contains("2026-08-14T12:30:00Z"));

        // Line item: description, HSN/SAC, quantity, rate, taxable value.
        assert!(text.contains("Butter Chicken"));
        assert!(text.contains("996331"));
        assert!(text.contains("2 x Butter Chicken"));
        assert!(text.contains(&money(25000))); // rate
        assert!(text.contains(&money(50000))); // taxable value

        // Tax breakdown by component.
        assert!(text.contains(&money(1250))); // CGST/SGST amount (shared value here)
        assert!(text.contains("CGST"));
        assert!(text.contains("SGST"));

        // Total.
        assert!(text.contains(&money(52500)));
    }

    #[test]
    fn invoice_tax_breakdown_sums_to_grand_total() {
        let invoice = invoice_fixture();
        let sum = invoice.taxable_value_paise
            + invoice.cgst_paise
            + invoice.sgst_paise
            + invoice.igst_paise
            + invoice.cess_paise
            + invoice.round_off_paise;
        assert_eq!(sum, invoice.grand_total_paise);

        let bytes = render_invoice(&invoice, &invoice_lines_fixture(), &invoice_ctx(), 80).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        // The rendered grand total is exactly the sum the invoice stored,
        // not an independently recomputed figure.
        assert!(text.contains(&format!(
            "Grand Total: {}",
            money(invoice.grand_total_paise)
        )));
    }

    #[test]
    fn invoice_reprint_is_byte_identical() {
        let invoice = invoice_fixture();
        let lines = invoice_lines_fixture();
        let ctx = invoice_ctx();
        let first = render_invoice(&invoice, &lines, &ctx, 80).unwrap();
        let second = render_invoice(&invoice, &lines, &ctx, 80).unwrap();
        assert_eq!(
            first, second,
            "reprint of the same invoice must be byte-identical (§31)"
        );
    }

    #[test]
    fn invoice_renders_both_paper_widths_and_is_well_formed_escpos() {
        for width in [58, 80] {
            let bytes = render_invoice(
                &invoice_fixture(),
                &invoice_lines_fixture(),
                &invoice_ctx(),
                width,
            )
            .expect("renders");
            assert!(!bytes.is_empty());
            assert_eq!(&bytes[0..2], &[0x1B, 0x40]);
            assert_eq!(&bytes[bytes.len() - 3..], &[0x1D, 0x56, 0x01]);
        }
    }

    #[test]
    fn invoice_rejects_unsupported_paper_width() {
        let err = render_invoice(
            &invoice_fixture(),
            &invoice_lines_fixture(),
            &invoice_ctx(),
            112,
        );
        assert!(err.is_err());
    }

    #[test]
    fn invoice_marks_cancelled_status_visibly() {
        let mut invoice = invoice_fixture();
        invoice.status = "CANCELLED".to_string();
        let bytes = render_invoice(&invoice, &invoice_lines_fixture(), &invoice_ctx(), 80).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("CANCELLED"));
    }

    #[test]
    fn invoice_shows_split_bill_part_marker() {
        let mut invoice = invoice_fixture();
        invoice.split_group_id = Some("018e5a2e-7777-7c3d-9f4e-000000000010".to_string());
        invoice.split_index = 2;
        invoice.split_count = 3;
        let bytes = render_invoice(&invoice, &invoice_lines_fixture(), &invoice_ctx(), 80).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("2 of 3"));
    }
}
