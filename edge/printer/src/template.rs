//! Renders a `Kot` (`packages/contracts/src/types/kot.ts`) to ESC/POS bytes
//! for 58mm and 80mm thermal paper. Station name, order number, table,
//! items, modifiers, notes, timestamp and the `#132-A` sequence marker
//! (docs/spec/kitchen.md) all appear — a cook reads this at speed, so the
//! bar is legible, not decorative.

use serde::Deserialize;

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
            let bytes = render_kot(
                &items_json(),
                "MAIN_KITCHEN",
                &ctx(),
                "10:15:31",
                width,
            )
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
}
