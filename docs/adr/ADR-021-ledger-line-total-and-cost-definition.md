# ADR-021 — `stock_ledger_entry.line_total_paise`, and what "weighted average cost" means here

**Status:** Accepted (2026-09-02)
**Date:** 2026-09-02
**Contracts:** additive minor bump to **v0.6.3** — `sqlite/0030_ledger_line_total.sql`, `postgres/0031_ledger_line_total.sql`
**Extends:** ADR-018 (M4 inventory, §5 rounding and §8 cost-on-the-ledger), ADR-019 (M5 procurement).
**Supersedes:** nothing.

## Context

M5 acceptance criterion 7 asked for a weighted average cost that matches an
independently computed figure. It did — and checking *why* it did found two
separate problems, one arithmetic and one definitional.

`unit_cost_paise` is a **rate**: paise per base unit, computed in
`edge/database/src/procurement/convert.rs` as
`line_total_paise × 10⁶ / base_quantity_micro` and **rounded to a whole integer,
once per receipt**. Weighted average cost then summed `quantity × rate`, so it
inherited a rounding the ledger had already committed to and could never recover.

**The error is ±0.5 paise on a per-gram figure, so its relative size scales
inversely with price.** 9.5 → 10 is +5.3%; 4.5 → 5 is +11.1%; 2.5 → 3 is +20%.
It is one-directional per item, and worst exactly where an outlet buys most of
its weight — cheap staples.

It survived acceptance because the acceptance dataset happened to divide evenly:
three receipts at 10, 10 and 18 paise/g. The third was chosen to make the average
*move*, not to make the rounding *fail*.

## Decisions

### 1. The ledger stores the invoiced total, and cost divides once

`stock_ledger_entry.line_total_paise` holds the exact money a row is worth, as
invoiced, unrounded. `procurement::cost` sums that column and
`quantity_applied_micro` and performs **one** division, in `i128`, rounded half
away from zero — the ADR-018 §5 rule, now actually followed rather than described.

The fix is structural rather than a rescale to micro-paise: the intermediate
rounding is **removed**, not made finer. The module doc that claimed rounding
happened "exactly once, at the end" becomes true instead of corrected.

`unit_cost_paise` **survives as a derived display rate only.** It is still
written, still shown, and still the figure an outbound movement is valued at. It
is **not** an averaging input, and it is **pinned by a drift test** rather than
by a comment: for every costed receipt row, `unit_cost_paise` must equal
`round_half_away(line_total_paise × 10⁶ / quantity_applied_micro)`.

### 2. Receipts set it; every other origin leaves it NULL

Only a receipt has an invoiced total. Wastage, count adjustments, variance and
outbound movements are valued **at** the average, so writing a `quantity × rate`
product for them would fabricate a precision that does not exist and then feed it
back into the average that produced it.

The CHECK is therefore **directional**, not a strict pairing:

```sql
CHECK (line_total_paise IS NULL OR unit_cost_paise IS NOT NULL)
```

A total never appears without its rate; a rate may stand alone. A strict pairing
would reject the majority of the ledger.

### 3. The averaging set does not move; the arithmetic does

This was expected to be a behaviour change and **is not**. Enumerating the nine
`NewStockLedgerEntry` construction sites showed exactly two write a cost:
`procurement/receipt.rs` (positive) and `procurement/movement.rs` (negative). The
old filter — `unit_cost_paise IS NOT NULL AND quantity_applied_micro > 0` —
already admitted receipts and nothing else, and the live edge database confirmed
it: nine positive `COUNT_ADJUSTMENT` rows, **none costed**.

So a positive count adjustment never dragged the average, and never could have.
It is kept out by `stock/count.rs` writing no cost at all — **one layer earlier
than the averaging filter**.

**`line_total_paise IS NOT NULL` will not keep a future costed non-receipt row
out.** Whoever adds a cost will very likely add a total with it; the two travel
together by convention now, and the filter would admit the row. The defence is
`count_adjustments_are_uncosted_and_never_enter_the_average`, which fails the day
a count adjustment is costed — a change that would look like an improvement.

### 4. A purchase return does not move the average

A return is costed and **outbound**, so `quantity_applied_micro > 0` excludes it
and it leaves `line_total_paise` NULL. Returning goods therefore leaves the
purchase-weighted figure untouched. That is a consequence of decision 5, not a
separate rule, and it is stated here so it is not rediscovered as a defect.

### 5. Holler implements a LIFETIME CUMULATIVE PURCHASE-WEIGHTED AVERAGE, not weighted average cost of stock on hand

Stated in those words because the difference matters to whoever reads the number.

**The averaging query is unbounded.** It filters on outlet, item, a non-null
total and a positive quantity — and nothing else. No `through_entry_seq`, no
`business_date`, no on-hand term. The live edge database holds twelve sealed
`stock_balance_snapshot` rows carrying exactly the high-water marks the balance
path bounds its reads with; the cost path references none of them.

Consequence: **stock bought at old prices drags the figure permanently, while
the outlet pays current prices.** An owner reading "average cost" generally means
the value of what is on the shelf. That is a different number.

**Only half of this was decided.** Excluding outbound rows is deliberate and
argued in `procurement/cost.rs` — "including an outbound row would let the act of
issuing stock move the purchase price". The *unbounded over all time* property is
**recorded nowhere**: the words "on hand", "moving average" and "periodic
average" appear in no design document in this repository. It is a consequence of
writing the simplest query, not a choice anyone made.

**The definition change is deliberately NOT folded into 0.6.3.** This version
fixes the arithmetic and does not touch which rows are summed. The divergence
from on-hand WAC is filed in `docs/backlog.md` with the trigger **before the
first pilot at the latest**, since food costing is a headline claim.

Two notes for whoever scopes it:

- **Criterion 7 is definition-neutral as written** — "after two receipts at
  different prices" — so it passes under either definition and cannot tell you
  which one you built. That is how this reached acceptance.
- **The bounding mechanism already exists.** `stock_balance_snapshot.through_entry_seq`
  is what an on-hand WAC would read from, and the balance path already uses it.

## Consequences

- The SQLite side is a **table rebuild**, so it must carry three indexes and
  three triggers back by hand. `migrations.rs` now asserts after the migration
  that the insert-only guard **actually fires** — by attempting a real `UPDATE`
  and requiring rejection, not by looking a trigger up by name — and that the
  durable `stock_ledger_sequence` counter still leads `MAX(entry_seq)`, so a
  rebuild cannot regress ranged replay below the cloud's high-water mark (0.5.8).
- **The backfill reconstructs; it does not recover.** Pre-0.6.3 rows never stored
  a total, so the only figure available is `quantity × rate` — the rounded number
  this ADR exists because of. Those rows are as accurate as the old path and no
  more. Rounding is half away from zero in both engines, with the sign handled
  explicitly, because integer division truncates toward zero and would otherwise
  round positives down and negatives up.
- **Overflow bound:** `quantity_applied_micro × unit_cost_paise` must fit `i64`
  wherever it is still formed. Quantity is bounded at 1e15 micro-units by
  `stock_ledger_entry_quantity_is_bounded`, so the product overflows past ~9,223
  paise per base unit. The new averaging path never forms that product — it sums
  money and multiplies by 10⁶ inside an `i128`.
- The consumer list reaches the wire: `edge/sync/src/ranged.rs`, the Go struct,
  the Zod schema, the OpenAPI shape and both repositories' INSERT *and* SELECT.
  A column the edge writes and the wire drops is the 0.5.9 defect, and here it
  would mean the two stores disagreeing about money.
- A **costed fixture** was added (`stock_ledger_entry_goods_receipt.json`).
  Both existing ledger fixtures are null in both money columns, and a null
  round-trips through a dropped field perfectly — the drift check would have been
  green on absent data.
