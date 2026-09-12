//! Renders a sample receipt to HTML so it can be LOOKED AT.
//!
//! COMMITTED ON PURPOSE. The demo build's earlier screenshot harnesses lived
//! in a session's scratch directory and only their PNGs were committed, so the
//! next pass had to rebuild them from nothing. This one is part of the repo:
//! `cargo run --bin receipt_preview -- <out.html>`.
//!
//! It renders through `render_invoice_html`, the SAME function the print path
//! calls, so what you see here is what a customer gets — not a mock-up of it.
//! The fixture is Gong data at Gong prices, because a receipt reviewed with
//! "Butter Chicken ₹250" on it is a receipt reviewed against the wrong menu.
//!
//! DEV-ONLY. Nothing in the shipped print path calls this, and it writes
//! wherever it is told rather than into the spool.

use holler_edge_database::model::{Invoice, InvoiceLine};
use holler_edge_printer::template::{render_invoice_html, InvoicePrintContext};

const OUTLET_ID: &str = "0191a000-0000-7000-8000-00000000000a";
const INVOICE_ID: &str = "01a09600-0000-7000-8000-000000000001";
const ORDER_ID: &str = "01a09600-0000-7000-8000-000000000002";

/// The seller identity as `outlet_fiscal_profile` stores it. Fictional in
/// FORMAT-VALID shapes: a real registration never belongs in a fixture.
fn fiscal_profile_json() -> String {
    serde_json::json!({
        "legal_name": "Gong Hospitality Pvt Ltd",
        "trade_name": "Gong — Modern Asian",
        "address_line1": "123 MG Road",
        "address_line2": "Camp",
        "city": "Pune",
        "state_code": "27",
        "state_name": "Maharashtra",
        "pincode": "411001",
        "gstin": "27AAAAA0000A1Z5",
        "fssai_number": "11522998000123",
        "invoice_footer_text": "Thank you — dev fixture, not a real bill"
    })
    .to_string()
}

/// Three lines rather than one: a receipt with a single line hides every
/// alignment problem a column has, and the demo bill is never one dish.
fn lines() -> Vec<InvoiceLine> {
    let mut out = Vec::new();
    let mut push = |no: i64, description: &str, qty: i64, unit: i64| {
        let gross = unit * qty;
        let cgst = gross * 250 / 10_000;
        out.push(InvoiceLine {
            id: format!("01a09600-0000-7000-8000-0000000000{no:02}"),
            invoice_id: INVOICE_ID.to_string(),
            order_item_id: format!("01a09600-0000-7000-8000-0000000001{no:02}"),
            line_no: no,
            description: description.to_string(),
            hsn_sac: Some("996331".to_string()),
            quantity: qty,
            unit_price_paise: unit,
            gross_paise: gross,
            discount_paise: 0,
            taxable_value_paise: gross,
            tax_profile_id: "0191e600-0000-7000-8000-000000000001".to_string(),
            cgst_rate_bps: 250,
            cgst_paise: cgst,
            sgst_rate_bps: 250,
            sgst_paise: cgst,
            igst_rate_bps: 0,
            igst_paise: 0,
            cess_rate_bps: 0,
            cess_paise: 0,
            total_paise: gross + cgst * 2,
        });
    };
    // A long name, a multiple quantity and a short name: the three shapes a
    // receipt column has to survive.
    push(1, "Kimchi Stone Bowl — Chicken", 1, 79_500);
    push(2, "Pad Thai", 2, 59_500);
    push(3, "Iced Tea", 2, 21_500);
    out
}

fn invoice(lines: &[InvoiceLine]) -> Invoice {
    let taxable: i64 = lines.iter().map(|l| l.taxable_value_paise).sum();
    let cgst: i64 = lines.iter().map(|l| l.cgst_paise).sum();
    let sgst: i64 = lines.iter().map(|l| l.sgst_paise).sum();
    let total = taxable + cgst + sgst;
    // Rounded to the rupee with the delta carried in round_off_paise, exactly
    // as the edge does it (contracts 0.4.0).
    let rounded = (total + 50) / 100 * 100;

    Invoice {
        id: INVOICE_ID.to_string(),
        outlet_id: OUTLET_ID.to_string(),
        order_id: ORDER_ID.to_string(),
        split_group_id: None,
        split_index: 1,
        split_count: 1,
        series_id: "0191a000-0000-7000-8000-000000000040".to_string(),
        invoice_number: "FY26/PNQ/001423".to_string(),
        invoice_date: "2026-09-12T08:30:00Z".to_string(),
        business_date: "2026-09-12".to_string(),
        status: "ISSUED".to_string(),
        cancelled_reason: None,
        cancelled_at: None,
        customer_name: Some("Walk-in".to_string()),
        customer_phone: None,
        customer_gstin: None,
        place_of_supply_state_code: "27".to_string(),
        subtotal_paise: taxable,
        discount_paise: 0,
        taxable_value_paise: taxable,
        cgst_paise: cgst,
        sgst_paise: sgst,
        igst_paise: 0,
        cess_paise: 0,
        round_off_paise: rounded - total,
        grand_total_paise: rounded,
        compliance_version_id: "0191e700-0000-7000-8000-000000000001".to_string(),
        tax_snapshot_json: "{}".to_string(),
        fiscal_profile_json: fiscal_profile_json(),
        channel: "POS".to_string(),
        tax_liability_party: "RESTAURANT".to_string(),
        eco_operator_name: None,
        eco_operator_gstin: None,
        supply_classification: None,
        created_by_user_id: "0191a000-0000-7000-8000-00000000000c".to_string(),
        created_at: "2026-09-12T08:30:00Z".to_string(),
        updated_at: "2026-09-12T08:30:00Z".to_string(),
        version: 1,
        sync_status: "PENDING".to_string(),
    }
}

fn main() -> Result<(), String> {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "receipt-preview.html".to_string());

    let lines = lines();
    let invoice = invoice(&lines);
    let ctx = InvoicePrintContext {
        order_display_number: "#A187",
        table_label: Some("T1"),
        // EXACTLY THE SHAPE THE PRODUCT BUILDS -- method labels joined with
        // " + " and no amounts (apps/pos/src-tauri/src/commands/kitchen.rs).
        // The first version of this fixture invented
        // "Cash Rs 1,000.00 . UPI Rs 1,463.00", which made the preview show a
        // receipt line the till cannot produce and sent me looking for a
        // currency-symbol inconsistency that only existed in the fixture. A
        // preview that renders something the product never emits is worse
        // than no preview.
        payment_summary: Some("Cash + UPI"),
    };

    let html = render_invoice_html(&invoice, &lines, &ctx)
        .map_err(|e| format!("rendering the receipt: {e}"))?;
    std::fs::write(&out, html).map_err(|e| format!("writing {out}: {e}"))?;
    println!("receipt_preview: wrote {out}");
    println!(
        "receipt_preview: grand total {} paise",
        invoice.grand_total_paise
    );
    Ok(())
}
