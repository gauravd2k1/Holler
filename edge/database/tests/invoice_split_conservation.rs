//! T7b task 3 (ADR-016 §4) verification: the binding conservation property
//! for split bills — Σ(split invoice lines) = order lines EXACTLY, no loss,
//! no duplication, no double-tax — checked property-style over generated
//! orders, plus the group round-off bound and the atomic all-or-nothing
//! rejection of a malformed split.
//!
//! Runtime: `cargo test`, native Windows (this crate has no non-Windows
//! target — ADR-013).

mod support;

use std::collections::HashMap;

use holler_edge_database::model::InvoiceLineShare;
use holler_edge_database::model::InvoiceOutboxMeta;
use holler_edge_database::Db;

/// A tiny deterministic PRNG (xorshift32) — this crate takes no dependency
/// on `proptest`/`quickcheck`, so the "property-style over generated
/// orders" the task calls for is hand-rolled and reproducible (a fixed seed
/// per test, not real randomness) rather than pulled in as a new dependency
/// for one test file.
struct Xorshift32(u32);
impl Xorshift32 {
    fn new(seed: u32) -> Self {
        Xorshift32(seed | 1)
    }
    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    /// Returns a value in `[1, max]`.
    fn range1(&mut self, max: i64) -> i64 {
        1 + (self.next_u32() as i64) % max
    }
}

/// Splits `total` (>=1) into `parts` positive shares summing to EXACTLY
/// `total` — the generator for one order item's per-split quantities.
fn random_partition(rng: &mut Xorshift32, total: i64, parts: i64) -> Vec<i64> {
    assert!(parts >= 1 && total >= parts);
    if parts == 1 {
        return vec![total];
    }
    // Stars-and-bars: pick `parts - 1` distinct cut points in [1, total-1].
    let mut cuts: Vec<i64> = Vec::new();
    while cuts.len() < (parts - 1) as usize {
        let c = rng.range1(total - 1);
        if !cuts.contains(&c) {
            cuts.push(c);
        }
    }
    cuts.sort_unstable();
    let mut shares = Vec::with_capacity(parts as usize);
    let mut prev = 0;
    for c in &cuts {
        shares.push(c - prev);
        prev = *c;
    }
    shares.push(total - prev);
    shares
}

/// One property-run: builds an order with `n_items` lines of random
/// quantity, splits it `n_splits` ways, issues the split group, and asserts
/// the conservation property plus the money reconciliation.
fn run_property_case(seed: u32, n_items: i64, n_splits: i64) {
    let mut rng = Xorshift32::new(seed);
    let mut db = Db::open_in_memory_for_tests().expect("open db");
    support::seed(&db, "SALES", "NEVER");

    let order_id = format!("order-prop-{seed}");
    let quantities: Vec<i64> = (0..n_items).map(|_| rng.range1(9)).collect(); // 1..=9 each
    let item_ids = support::create_order(&mut db, &order_id, 12_345, &quantities);

    // For every order item, partition its quantity across the n_splits
    // parts (a part may legitimately get 0 of a given item — a split bill
    // where "my part" has none of some dish is normal).
    let mut per_split_shares: Vec<Vec<InvoiceLineShare>> = vec![Vec::new(); n_splits as usize];
    for (item_idx, &qty) in quantities.iter().enumerate() {
        let parts_to_use = std::cmp::min(n_splits, qty); // can't give more non-empty parts than quantity
        let partitioned = random_partition(&mut rng, qty, parts_to_use);
        // Distribute the `parts_to_use` non-zero shares across n_splits
        // split slots (some slots get zero => simply no share entry for
        // that item on that part).
        for (k, share_qty) in partitioned.into_iter().enumerate() {
            let split_slot = (k as i64 % n_splits) as usize;
            per_split_shares[split_slot].push(InvoiceLineShare {
                id: format!("{order_id}-split{split_slot}-item{item_idx}"),
                order_item_id: item_ids[item_idx].clone(),
                quantity: share_qty,
                discount_per_unit_paise: 0,
            });
        }
    }

    let header = support::header(&order_id, "SALES", "2026-08-12", "2026-08-12T10:00:00Z");
    let parts: Vec<(String, Vec<InvoiceLineShare>)> = per_split_shares
        .into_iter()
        .enumerate()
        .map(|(i, shares)| (format!("{order_id}-invoice-{i}"), shares))
        .collect();
    let metas: Vec<InvoiceOutboxMeta> = (0..parts.len())
        .map(|i| InvoiceOutboxMeta {
            outbox_id: format!("{order_id}-outbox-{i}"),
            occurred_at: "2026-08-12T10:00:00Z".to_string(),
        })
        .collect();

    let issued = db
        .issue_split_invoices_with_outbox(&header, format!("{order_id}-split-group"), parts, &metas)
        .unwrap_or_else(|e| {
            panic!(
                "seed {seed} (n_items={n_items}, n_splits={n_splits}): split issuance failed: {e}"
            )
        });

    assert_eq!(issued.len() as i64, n_splits);

    // --- Conservation: every order_item's billed quantity across every
    // split invoice's lines must equal its OWN order-line quantity exactly.
    let mut billed_by_item: HashMap<String, i64> = HashMap::new();
    let mut group_round_off_abs_sum: i64 = 0;
    for inv in &issued {
        assert_eq!(
            inv.split_group_id.as_deref(),
            Some(format!("{order_id}-split-group").as_str())
        );
        assert!(
            inv.round_off_paise >= -50 && inv.round_off_paise <= 50,
            "each part's own round_off must stay within the invoice CHECK bound"
        );
        group_round_off_abs_sum += inv.round_off_paise.abs();

        let lines = db.list_invoice_lines(&inv.id).expect("read invoice lines");
        for line in lines {
            *billed_by_item
                .entry(line.order_item_id.clone())
                .or_insert(0) += line.quantity;
        }
    }

    for (idx, &expected_qty) in quantities.iter().enumerate() {
        let item_id = &item_ids[idx];
        let got = billed_by_item.get(item_id).copied().unwrap_or(0);
        assert_eq!(
            got, expected_qty,
            "seed {seed}: order_item {item_id} has quantity {expected_qty} but split lines total {got}"
        );
    }
    assert_eq!(
        billed_by_item.len(),
        item_ids.len(),
        "seed {seed}: every order item must appear in the split — no line silently dropped"
    );

    // --- Group round-off bound (ADR-016 §4): |round_off| summed across the
    // group is bounded by 50 paise * split_count.
    assert!(
        group_round_off_abs_sum <= 50 * n_splits,
        "seed {seed}: group round-off {group_round_off_abs_sum} exceeds 50 * split_count ({})",
        50 * n_splits
    );

    // --- No double-tax: summing every part's taxable_value_paise must equal
    // what ONE unsplit invoice over the same lines would have taxed, since
    // every paisa of taxable value on the order appears on exactly one
    // split line (the conservation property already proven above applied to
    // money rather than quantity).
    let sum_taxable: i64 = issued.iter().map(|i| i.taxable_value_paise).sum();
    let expected_taxable: i64 = quantities.iter().map(|&q| 12_345 * q).sum();
    assert_eq!(
        sum_taxable, expected_taxable,
        "seed {seed}: Σ(split taxable_value_paise) must equal the order's own taxable value — no loss or duplication of money"
    );
}

#[test]
fn split_conservation_holds_across_many_generated_orders() {
    // A spread of item counts / split counts, each run with several seeds —
    // "over generated orders", reproducibly.
    let cases: &[(i64, i64)] = &[
        (1, 2),
        (2, 2),
        (3, 2),
        (3, 3),
        (4, 3),
        (5, 4),
        (2, 5),
        (6, 2),
    ];
    for &(n_items, n_splits) in cases {
        for seed in 0..5u32 {
            run_property_case(seed * 97 + 1, n_items, n_splits);
        }
    }
}

/// A split whose shares do NOT sum to an order item's quantity (loss: one
/// unit of a two-quantity line is never billed to anyone) must be rejected
/// — and rejected ATOMICALLY: no invoice from the attempted group is
/// persisted, not even the parts whose own shares looked fine.
#[test]
fn under_billed_split_is_rejected_atomically() {
    let mut db = Db::open_in_memory_for_tests().expect("open db");
    support::seed(&db, "SALES", "NEVER");
    let order_id = "order-under";
    let item_ids = support::create_order(&mut db, order_id, 10_000, &[2]);

    let header = support::header(order_id, "SALES", "2026-08-12", "2026-08-12T10:00:00Z");
    let parts = vec![(
        "invoice-under-1".to_string(),
        vec![InvoiceLineShare {
            id: "line-under-1".to_string(),
            order_item_id: item_ids[0].clone(),
            quantity: 1, // order line has quantity 2 — one unit is unaccounted for
            discount_per_unit_paise: 0,
        }],
    )];
    let metas = vec![InvoiceOutboxMeta {
        outbox_id: "outbox-under-1".to_string(),
        occurred_at: "2026-08-12T10:00:00Z".to_string(),
    }];

    let err = db
        .issue_split_invoices_with_outbox(&header, "under-group".to_string(), parts, &metas)
        .expect_err("an under-billed split must be rejected");
    assert!(
        format!("{err}").contains("conservation"),
        "expected a conservation error, got {err}"
    );

    assert!(
        db.list_invoices_for_order(order_id)
            .expect("list")
            .is_empty(),
        "a rejected split must leave NO invoice behind, not even a partially-issued one"
    );
}

/// The mirror case: shares that OVER-bill an order item (duplication /
/// double-tax) must be rejected just as firmly as under-billing.
#[test]
fn over_billed_split_is_rejected_atomically() {
    let mut db = Db::open_in_memory_for_tests().expect("open db");
    support::seed(&db, "SALES", "NEVER");
    let order_id = "order-over";
    let item_ids = support::create_order(&mut db, order_id, 10_000, &[2]);

    let header = support::header(order_id, "SALES", "2026-08-12", "2026-08-12T10:00:00Z");
    let parts = vec![
        (
            "invoice-over-1".to_string(),
            vec![InvoiceLineShare {
                id: "line-over-1".to_string(),
                order_item_id: item_ids[0].clone(),
                quantity: 2,
                discount_per_unit_paise: 0,
            }],
        ),
        (
            "invoice-over-2".to_string(),
            vec![InvoiceLineShare {
                id: "line-over-2".to_string(),
                order_item_id: item_ids[0].clone(),
                quantity: 1, // 2 + 1 = 3, but the order line only has quantity 2
                discount_per_unit_paise: 0,
            }],
        ),
    ];
    let metas = vec![
        InvoiceOutboxMeta {
            outbox_id: "outbox-over-1".to_string(),
            occurred_at: "2026-08-12T10:00:00Z".to_string(),
        },
        InvoiceOutboxMeta {
            outbox_id: "outbox-over-2".to_string(),
            occurred_at: "2026-08-12T10:00:00Z".to_string(),
        },
    ];

    let err = db
        .issue_split_invoices_with_outbox(&header, "over-group".to_string(), parts, &metas)
        .expect_err("an over-billed split must be rejected");
    assert!(
        format!("{err}").contains("conservation"),
        "expected a conservation error, got {err}"
    );

    assert!(
        db.list_invoices_for_order(order_id).expect("list").is_empty(),
        "a rejected split must leave NO invoice behind, including the first part that looked correct on its own"
    );
}

/// A correct 3-way split, checked end to end against
/// [`holler_edge_database::Db::list_invoices_for_split_group`] — the exact
/// read path a reprint/reconciliation screen would use.
#[test]
fn a_correct_three_way_split_is_independently_payable_and_numbered() {
    let mut db = Db::open_in_memory_for_tests().expect("open db");
    support::seed(&db, "SALES", "NEVER");
    let order_id = "order-3way";
    let item_ids = support::create_order(&mut db, order_id, 30_000, &[3]);

    let header = support::header(order_id, "SALES", "2026-08-12", "2026-08-12T10:00:00Z");
    let parts: Vec<(String, Vec<InvoiceLineShare>)> = (0..3)
        .map(|i| {
            (
                format!("invoice-3way-{i}"),
                vec![InvoiceLineShare {
                    id: format!("line-3way-{i}"),
                    order_item_id: item_ids[0].clone(),
                    quantity: 1,
                    discount_per_unit_paise: 0,
                }],
            )
        })
        .collect();
    let metas: Vec<InvoiceOutboxMeta> = (0..3)
        .map(|i| InvoiceOutboxMeta {
            outbox_id: format!("outbox-3way-{i}"),
            occurred_at: "2026-08-12T10:00:00Z".to_string(),
        })
        .collect();

    let issued = db
        .issue_split_invoices_with_outbox(&header, "3way-group".to_string(), parts, &metas)
        .expect("issue 3-way split");

    let mut numbers: Vec<String> = issued.iter().map(|i| i.invoice_number.clone()).collect();
    numbers.sort();
    assert_eq!(
        numbers,
        vec!["INV-000001", "INV-000002", "INV-000003"],
        "each part is independently numbered"
    );

    for (i, inv) in issued.iter().enumerate() {
        assert_eq!(inv.split_index, i as i64 + 1);
        assert_eq!(inv.split_count, 3);
    }

    let by_group = db
        .list_invoices_for_split_group("3way-group")
        .expect("list by split group");
    assert_eq!(
        by_group.len(),
        3,
        "every part must be reachable by its split_group_id — the reprint/reconciliation read path"
    );
}
