# M6 A1 — the ingest sink audit

**Status: READ-VERIFIED, not executed.** Every row below was established by
reading the repository. **Exactly one path in this document has been observed
failing and then observed fixed** — `ordering.AppendItem`'s foreign key on
`order_item.menu_item_id`, which is M6 C7's subject and is marked **Executed**.
Everything else is a claim about code that nothing has yet run. The two classes
are kept apart deliberately: M5 closed on the rule that *a criterion is not
closed by an agent's summary of a run*, and a read is not a run.

Written 2026-09-03 during M6 A1, alongside `99875cc`.

---

## Why the audit exists

A1's defect was found by accident. `POST /orders/{id}/items` answered a foreign
key violation with **500 `internal_error`**, the edge classified 5xx as
transient, and one unreplayable row stranded 114 others behind it. Nothing about
that was specific to orders — it was a repository wrapping `pgconn.PgError` with
`fmt.Errorf` and an HTTP boundary with no case for it.

So the question this document answers is not "which handlers look risky" but
**which write paths can hand an unclassified integrity error to the HTTP
boundary**. CLAUDE.md's rule: *enumerate the SINKS, not the surfaces.* A handler
can be missed; a write cannot.

## Method, and what it cannot see

1. Every `.Exec(`/`.QueryRow(`/`.Query(` call site in `backend/internal/<context>`
   whose statement window contains `INSERT`, `UPDATE` or `DELETE`, excluding
   `_test.go`. **147 write sinks across ten bounded contexts.**
2. Each classified by what its error handling does: `storage.Wrap`/`Classify`
   (**classified**), a local `isUniqueViolation`/`pgUniqueViolation` check
   (**unique-only** — 23505 handled, 23503 not), or neither (**unclassified**).
3. Cross-referenced against the twelve `MountIngest` routes, because an edge
   replay is the only caller that can wedge an outbox.

**A CORRECTION THIS DOCUMENT EARNED ON ITS FIRST USE.** The `kitchen.InsertKot`
row below named the wrong mechanism. The scan reads repository call sites, so it
saw `fmt.Errorf` wrapping an INSERT with an `order_id` foreign key and inferred a
500. The service layer pre-checks that parent with `repo.OrderOutlet` and returns
**404**, so the FK never fires and the failure mode is a different — and worse —
one. **A sink audit that stops at the repository can name the wrong failure for
the right route.** The conclusion held (this route mishandles a missing parent);
the stated cause did not. Read the service path before writing the fix, and
expect the same correction on the remaining rows.

**Limits, stated so nobody reads more into this than it supports.** The scan
matches a 40-line window after each call site, so a read-only function sitting
next to a write can be attributed a write's state; the per-function table below
was spot-checked by hand, the per-context counts were not. It cannot tell which
foreign keys are reachable with real data — only which paths would report a
violation as a server fault **if** one occurred. And it says nothing about
whether a given FK is likely to fire; where this document argues that a given
foreign key matters, that is reasoning about the data model, not measurement.

---

## The finding

**The A1 defect is not confined to ordering.** At least four more ingest routes
reach an INSERT with foreign keys and wrap the error with `fmt.Errorf`, so they
answer 500 on exactly the condition A1 fixed for orders.

| Ingest route | Repository sink | FK columns that can violate | State |
|---|---|---|---|
| `POST /orders/{id}/items` | `ordering.AppendItem` (`repository.go:238`) | `order_id`, `menu_item_id`, `variant_id` | **classified — Executed, red then green** |
| `POST /orders` | `ordering.InsertOrder` (`repository.go:110`) | `outlet_id`, `device_id`, `table_id` | classified (Read-verified) |
| `POST /orders/{id}/confirm`, `/send-to-kitchen`, `/cancel` | `ordering.UpdateStatus`, `ConfirmOrder` | — (UPDATE by id) | classified (Read-verified) |
| `POST /orders/{id}/kots` | `kitchen.IngestKot` (`service.go:347`), NOT `InsertKot` | **`order_id`** — a KOT for an order the cloud never accepted | **FIXED in A1b — and the mechanism recorded here was WRONG.** `IngestKot` pre-checks with `repo.OrderOutlet`, which returns `ErrNotFound`, so the reply was **404** and the FK was unreachable outside a race. 404 is worse than the assumed 500: the edge classifies it TRANSIENT, so the ticket retried forever AND took a global stop with it, which A2's per-aggregate blocking does not catch. Now 422 `missing_reference`, observed red-then-green |
| `POST /kots/{kotId}/status` | `kitchen.UpdateKotStatus` (`repository.go:418`) | `kot_id` | **UNCLASSIFIED** |
| `POST /invoices` | `payments.InsertInvoice` (`repository.go:73`), `insertInvoiceLine` (`:142`) | `outlet_id`, `order_id`, `invoice_series_id`, `menu_item_id` on lines | unique-only → **UNCLASSIFIED for 23503** |
| `POST /payments` | `payments.InsertPayment` (`repository.go:270`), `insertAllocation` (`:298`) | **`outlet_id`, `order_id`, `cash_shift_id`, `reverses_payment_id`**; allocation's `invoice_id` | **FIXED in A1b — mechanism as recorded.** Unlike the KOT route, `IngestPayment` pre-checks nothing, so 23503 reached the driver and returned 500. Observed red (`payment_order_id_fkey (SQLSTATE 23503)`, unmapped) then green as 422 `missing_reference` |
| `POST /cash-shifts`, `/cash-shifts/{id}/close` | `payments.InsertCashShift`, `CloseCashShift`, `insertMovement` | `outlet_id`, `cash_shift_id`, `created_by_user_id` | **UNCLASSIFIED** |
| `POST /inventory/ledger-entries` | `inventory.InsertLedgerEntry` (`repository.go:560`) | `inventory_item_id`, `outlet_id`, `source_stock_count_id` | unique-only → **UNCLASSIFIED for 23503** |
| `POST /inventory/counts` | `inventory.InsertStockCount` (`repository.go:782`) | `outlet_id`, `inventory_item_id` on lines | **UNCLASSIFIED** |
| `POST /procurement/goods-receipts` | `procurement.InsertGoodsReceiptNote` (`repository.go:667`) | `outlet_id`, `purchase_order_id`, `supplier_id`, `received_by_user_id`; `inventory_item_id` on lines | **UNCLASSIFIED** |
| `POST /procurement/purchase-returns` | `procurement.InsertPurchaseReturn` (`repository.go:802`) | `supplier_id`, `outlet_id`, line `inventory_item_id` | **UNCLASSIFIED** |
| `POST /procurement/stock-transfers-out` | `procurement.InsertStockTransferOut` (`repository.go:880`) | `outlet_id`, destination outlet, line `inventory_item_id` | **UNCLASSIFIED** |
| `POST /menu/items/{itemId}/availability` | `menu.UpdateItemAvailability` (`repository.go:186`) | `menu_item_id` | **UNCLASSIFIED** |

**Two of these are worse than orders were, for the same reason orders were bad.**
`kitchen.InsertKot` carries `order_id`, and a KOT can only exist for an order —
so any order the cloud rejects produces a KOT that can never land either.
`payments.InsertPayment` carries four foreign keys including `order_id` and
`cash_shift_id`; a wedge there strands money movements, and `payment` is
append-only in both stores, so nothing later corrects it.

Confirmed by reading the code, not by running it:

```go
// kitchen/repository.go:361
if err != nil {
    return Kot{}, false, fmt.Errorf("kitchen: inserting kot: %w", err)
}
// payments/repository.go:270
if err != nil {
    return Payment{}, false, fmt.Errorf("payments: inserting payment: %w", err)
}
```

Both are the exact shape `ordering` had before `99875cc`.

## Per-context write-sink counts

Machine-generated, **not** hand-verified row by row; the window heuristic
described above inflates "unclassified" where a read sits beside a write. Use it
to size the work, not to cite a specific line.

| Context | classified | unique-only | unclassified | total |
|---|---|---|---|---|
| ordering | 5 | 0 | 0 | 5 |
| auth | 0 | 0 | 19 | 19 |
| compliance | 0 | 11 | 5 | 16 |
| inventory | 0 | 9 | 9 | 18 |
| kitchen | 0 | 4 | 12 | 16 |
| menu | 0 | 0 | 10 | 10 |
| outlet | 0 | 4 | 9 | 13 |
| payments | 0 | 1 | 8 | 9 |
| procurement | 0 | 22 | 12 | 34 |
| tables | 0 | 5 | 2 | 7 |
| **total** | **5** | **56** | **86** | **147** |

## What happens to this list

- ~~The seven local `isUniqueViolation` helpers migrate to
  `internal/platform/storage`~~ — **DONE** in A1's second commit, with
  `scripts/check-sqlstate-classifier.mjs` failing the build if a new local copy
  or a bare SQLSTATE literal appears anywhere under `backend/internal`. Every
  **unique-only** row above is now classified for 23503 as well, since
  `Classify` handles both. **The guard was watched failing**: reintroducing
  `const pgUniqueViolation = "23505"` and an `isUniqueViolation` in
  `tables/repository.go` produced two named violations and exit 1.
- ~~The ingest routes in the table above are A1's remaining work~~ — **kitchen and
  payments are DONE as A1b**, each with its own red-then-green. The remaining ten
  are filed in `docs/backlog.md` with a trigger, not carried here: a list of known
  defects living only in a document nobody re-reads is how they float.
- **Expect the A1b correction to repeat.** Two routes, two different failure modes:
  the KOT route pre-checked its parent and answered **404** (worse than a 500,
  because the edge treats 404 as transient), while the payment route pre-checked
  nothing and answered **500**. The scan cannot tell those apart, so each remaining
  row needs its service path read before its fix is written.
- **Non-ingest write paths are NOT A1's scope.** `auth`, `outlet`, `tables`,
  `compliance` and most of `menu` are called by a human through a browser, where
  a 500 is a bad error message rather than a wedged outbox. They are worth
  fixing and they are not what M6 C7 observes. Filed, not scheduled.

**None of the unclassified rows has been observed failing.** When each is fixed
it gets the same treatment `ordering` got — a test asserting the post-fix
contract, watched red first — or it does not count as fixed.
