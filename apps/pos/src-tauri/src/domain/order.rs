//! Order business logic — pure functions, no SQLite/Tauri concerns (CLAUDE.md
//! "business logic outside UI components / command handlers"). This module
//! decides *what* an order row and its items should contain; `commands::orders`
//! decides only *when* to call it and how to hand the result to
//! `holler_edge_database::Db`.
//!
//! Money is `i64` paise throughout (CLAUDE.md). Milestone 1 excludes tax and
//! discount computation (Milestone 3): `total_paise` therefore always equals
//! `subtotal_paise` here, and `discount_paise`/`tax_paise` are always the
//! caller-supplied zero.

use holler_edge_database::model::{NewOrder, NewOrderItem, Order};

use crate::error::DomainError;

/// One line the cashier has added to the cart. `unit_price_paise` is a
/// snapshot taken by the caller from the live menu at add-time — this
/// function never re-reads or recomputes it (ordering.md: line items are
/// append-only / snapshot semantics).
#[derive(Debug, Clone)]
pub struct DraftOrderItemInput {
    pub menu_item_id: String,
    pub variant_id: Option<String>,
    pub quantity: i64,
    pub unit_price_paise: i64,
    pub notes: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DraftOrderInput {
    pub outlet_id: String,
    pub device_id: String,
    pub order_type: String,
    pub table_id: Option<String>,
    pub items: Vec<DraftOrderItemInput>,
}

/// `unit_price_paise * quantity`. The only place a line total is computed —
/// callers must not duplicate this arithmetic.
pub fn line_total_paise(unit_price_paise: i64, quantity: i64) -> Result<i64, DomainError> {
    if quantity <= 0 {
        return Err(DomainError::InvalidQuantity);
    }
    Ok(unit_price_paise * quantity)
}

/// Sums line totals into an order subtotal. Milestone 1: total == subtotal
/// (no tax/discount computation).
pub fn compute_totals(line_totals_paise: &[i64]) -> (i64, i64) {
    let subtotal: i64 = line_totals_paise.iter().sum();
    (subtotal, subtotal)
}

/// Builds the `NewOrder`/`NewOrderItem` rows for a brand-new DRAFT order,
/// with edge-generated ids/timestamps supplied by the caller (order ids are
/// minted at the edge as UUIDv7, sync.md §74 — never assigned by the cloud).
pub fn build_new_draft_order(
    order_id: String,
    item_ids: Vec<String>,
    input: &DraftOrderInput,
    now_iso: &str,
) -> Result<(NewOrder, Vec<NewOrderItem>), DomainError> {
    assert_eq!(
        item_ids.len(),
        input.items.len(),
        "caller must supply exactly one id per item"
    );

    let mut items = Vec::with_capacity(input.items.len());
    let mut line_totals = Vec::with_capacity(input.items.len());
    for (id, item) in item_ids.into_iter().zip(input.items.iter()) {
        let line_total = line_total_paise(item.unit_price_paise, item.quantity)?;
        line_totals.push(line_total);
        items.push(NewOrderItem {
            id,
            order_id: order_id.clone(),
            menu_item_id: item.menu_item_id.clone(),
            variant_id: item.variant_id.clone(),
            quantity: item.quantity,
            unit_price_paise: item.unit_price_paise,
            line_total_paise: line_total,
            notes: item.notes.clone(),
            created_at: now_iso.to_string(),
        });
    }

    let (subtotal_paise, total_paise) = compute_totals(&line_totals);

    let order = NewOrder {
        id: order_id,
        outlet_id: input.outlet_id.clone(),
        device_id: input.device_id.clone(),
        order_type: input.order_type.clone(),
        status: "DRAFT".to_string(),
        table_id: input.table_id.clone(),
        subtotal_paise,
        discount_paise: 0,
        tax_paise: 0,
        total_paise,
        created_at: now_iso.to_string(),
        updated_at: now_iso.to_string(),
    };

    Ok((order, items))
}

/// Rejects any mutation of an order that is not in `DRAFT` status
/// (ordering.md order state machine — illegal transitions/edits must be
/// rejected at the command layer, not just the UI).
pub fn assert_draft(order: &Order) -> Result<(), DomainError> {
    if order.status != "DRAFT" {
        return Err(DomainError::OrderNotDraft(order.id.clone()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_total_multiplies_paise() {
        assert_eq!(line_total_paise(15000, 3).unwrap(), 45000);
    }

    #[test]
    fn line_total_rejects_non_positive_quantity() {
        assert!(matches!(
            line_total_paise(15000, 0),
            Err(DomainError::InvalidQuantity)
        ));
        assert!(matches!(
            line_total_paise(15000, -1),
            Err(DomainError::InvalidQuantity)
        ));
    }

    #[test]
    fn totals_sum_and_never_add_tax_or_discount_in_m1() {
        let (subtotal, total) = compute_totals(&[45000, 20000]);
        assert_eq!(subtotal, 65000);
        assert_eq!(total, 65000, "M1 excludes tax/discount computation");
    }

    #[test]
    fn build_new_draft_order_computes_snapshot_totals() {
        let input = DraftOrderInput {
            outlet_id: "outlet-1".into(),
            device_id: "device-1".into(),
            order_type: "DINE_IN".into(),
            table_id: None,
            items: vec![
                DraftOrderItemInput {
                    menu_item_id: "item-1".into(),
                    variant_id: None,
                    quantity: 2,
                    unit_price_paise: 15000,
                    notes: None,
                },
                DraftOrderItemInput {
                    menu_item_id: "item-2".into(),
                    variant_id: None,
                    quantity: 1,
                    unit_price_paise: 20000,
                    notes: Some("no onions".into()),
                },
            ],
        };
        let (order, items) = build_new_draft_order(
            "order-1".into(),
            vec!["item-row-1".into(), "item-row-2".into()],
            &input,
            "2026-08-07T10:00:00.000Z",
        )
        .unwrap();

        assert_eq!(order.status, "DRAFT");
        assert_eq!(order.subtotal_paise, 50000);
        assert_eq!(order.total_paise, 50000);
        assert_eq!(order.discount_paise, 0);
        assert_eq!(order.tax_paise, 0);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].line_total_paise, 30000);
        assert_eq!(items[1].line_total_paise, 20000);
    }

    #[test]
    fn assert_draft_rejects_non_draft_status() {
        let order = Order {
            id: "order-1".into(),
            outlet_id: "outlet-1".into(),
            device_id: "device-1".into(),
            order_type: "DINE_IN".into(),
            status: "BILLED".into(),
            table_id: None,
            subtotal_paise: 0,
            discount_paise: 0,
            tax_paise: 0,
            total_paise: 0,
            version: 1,
            sync_status: "PENDING".into(),
            created_at: "2026-08-07T10:00:00.000Z".into(),
            updated_at: "2026-08-07T10:00:00.000Z".into(),
        };
        assert!(matches!(
            assert_draft(&order),
            Err(DomainError::OrderNotDraft(_))
        ));
    }
}
