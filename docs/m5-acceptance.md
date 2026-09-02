# M5 Procurement — acceptance record

**Milestone 5 is CLOSED**, against `packages/contracts/` **v0.6.3** (ADR-019 with
its three addenda, plus ADR-020 and ADR-021), migrations through **sqlite 0030 /
postgres 0031**.

All seven acceptance criteria were observed against the binaries that ship. None
is evidenced by a test harness, which is the rule this milestone set for itself
(CLAUDE.md; `docs/retro.md`, 2026-08-11).

**Observer:** the operator (repository owner), driving the shipping POS and the
running backend by hand. Criteria 1, 3, 4 and 6 were observed on real screens on
**2026-09-02**; criteria 2, 5 and 7 earlier, on the dates given below.

---

## Why this file exists

The evidence for four of these criteria lived only in a chat transcript. A
session restart erased it, and the next session reconstructed a criteria table
from git history alone — concluding that criteria 1, 3, 4 and 6 were unobserved
while holding, in the log it had just read, the commit that was made *after* the
criterion-6 run and *because* of it.

The reconstruction was stated with the same confidence as a read of the record.

**Acceptance evidence that lives only in a session is lost when the session is.**
The same family as a test whose subject nothing else constructs: the fact
existed; the record of it did not. The rule that follows from this — *a milestone
does not close until its acceptance evidence is committed to the repository* —
is now in CLAUDE.md and in every builder agent file.

---

## The seven criteria

### 1. Receive a delivery with the network disconnected

> GRN recorded, `PURCHASE` ledger entries at the converted base-unit quantity
> with `unit_cost_paise` set, and stock rises by the received amount.

**MET — observed 2026-09-02 on the shipping POS.**

- **The offline precondition was established and independently verified, for the
  first time in any milestone.** The backend was stopped **by PID**, and
  `scripts/check-cloud-unreachable.ps1` was run against the cloud base URL: all
  three probes agreed — nothing listening, TCP refused, `/health` no answer.
- **The check was falsified before it was trusted:** the same script was run
  first while the cloud was up, and printed `STOP`. A check that has never been
  seen to say no is not evidence.
- Receipt recorded as **`GRN/20260902/0002`**. Atta rose **400000 g → 500000 g**.

This closes the defect recorded in `docs/retro.md` (2026-09-02, *"A test
condition the environment cannot produce is not a weak test, it is no test"*):
every prior "with the network disconnected" step in this project was performed
by switching WiFi off against a cloud at `http://localhost:8080`, which no change
to the network stack can make unreachable.

### 2. Kill the POS between the GRN write and the ledger post

> GRN and ledger agree on reopen. Judged against the crash, not the API.

**MET — observed 2026-09-01, both ways.**

Driven through `crashpoint --grn`, which calls `Db::record_goods_receipt` — the
same entry point the POS uses — against a copy of the real sealed edge database,
and read back by an independent reopen with `sqlite3`:

| Run | `goods_receipt_note` | `grn_line` | `stock_ledger_entry` |
|---|---|---|---|
| abort at `after_grn_before_ledger` | 0 | 0 | 0 |
| abort at `after_ledger_before_commit` | 1 | 1 | 1 |

Positive row: `PURCHASE | 5000000000 micro | unit_cost 4 paise | entry_seq 21 |
origin GOODS_RECEIPT | business_date 2026-08-23`. Both aborts terminated
abnormally (`0xC0000409`), not by a clean exit.

**The second crash point was added for this run and is the point of it.** Before
it, the criterion rested on "0 rows everywhere" plus a `UNIQUE` violation — and 0
rows is exactly what a receipt path that silently wrote nothing would also
produce. An absence means something only when the same reopen can be shown to
find the rows when they were written.

### 3. Receive against a PO that never synced to the edge

> The receipt completes, a gap is recorded, and the gap is visible to a human on
> the POS.

**MET — observed 2026-09-02 on the shipping POS.**

The `/procurement/gaps` screen showed **`PURCHASE_ORDER_NOT_FOUND`** and
**`NO_SUPPLIER_ITEM`** against delivery
**`01a05ea9-118e-7071-8fc5-a01a5690be29`**. The receipt itself stood.

This is ADR-019's first rule observed rather than asserted: *a GRN never blocks
on a PO*.

### 4. A purchase-unit quantity converts, and the screen echoes what it will record

> `entryIntentEcho` before the operator commits.

**MET — observed 2026-09-02 on the shipping POS.**

The echo rendered before commit and read:

- **4 sack → 100000 g**
- **1 sack = 25000 g**
- **Cost ₹0.10 per base unit · line total ₹9500.00**

The pack-size gap was shown **inside the echo block**, where the operator reads
the number they are about to commit — not on a separate screen they may never
open.

### 5. A PO over the approver's limit is refused with a message that says what to do next

**MET — observed 2026-09-01, against the API.**

Scoped to the API deliberately: `apps/admin` does not exist and is M6, so
purchase orders are raised through the API, and that is the honest statement of
what M5 delivers rather than a criterion quietly re-scoped to fit.

Seeded: role `BUYER` at `po_approval_limit_paise` 5,000,000 (₹50,000) holding
`procurement.manage` + `procurement.approve`, on user `buyer@holler.test`; role
`OWNER` at 50,000,000 (₹500,000) with `procurement.approve` and **deliberately no
user** — `RolesAbleToApprove` selects role rows, so a role with no holder is
enough to be named. Two ceilings are required, not one: with a single role the
refusal is correct and names nobody, which is the half of the message that tells
the caller what to do next.

1. PO raised at 2,500,000 paise, under the buyer's ceiling → `PENDING_APPROVAL`.
2. Buyer approves → `APPROVED`, `approved_by_user_id` and `approved_at` set.
3. Amend upward to 10,000,000 paise → **`PENDING_APPROVAL`, with
   `approved_by_user_id` and `approved_at` both cleared.** Confirmed by an
   independent PostgreSQL read, not only the response body.
4. Buyer re-approves → **403**, with all three §64 elements present:

```json
{"code":"po_exceeds_approval_limit","message":"this purchase order exceeds your
role's approval limit: this purchase order totals 10000000 paise and your role's
approval limit is 5000000 paise. Next: ask one of these roles to approve it
instead: [Owner]","total_paise":10000000,"limit_paise":5000000,
"can_be_approved_by_roles":["Owner"]}
```

A fifth thing fell out of the run: **the amend route refuses to grant approval.**
`PATCH` with `status: "APPROVED"` is rejected with *"may only be reached through
POST /procurement/purchase-orders/{id}/approve"*, so the revocation in step 3
cannot be undone through the same call that triggers it.

### 6. A GRN created at the edge replays to the cloud and reads back identically

> With a fixture that populates every provenance field.

**MET — observed 2026-09-02, through the shipping POS's own shutdown drain.**

Not a harness: the drain ran from `RunEvent::Exit` in the POS process, hosted per
ADR-020, and the backend's request log recorded the receiving end.

**Predicted before the run, then confirmed** — `published=6`, outbox
**126 → 120**, orders stream unchanged. All three came true.

- Edge row and cloud row **field-identical across eight fields**.
- **Both gap rows and two ledger entries carried.**
- **`line_total_paise = 950000` intact on the wire** — the contracts 0.6.3 field,
  on a populated row rather than a null-heavy one.
- The edge database **sealed cleanly** afterwards.

Predicting the three numbers first and then reading them is what makes this an
observation rather than a description: a drain that mis-routed would have had to
make the outbox's own count agree with its report, and it cannot.

### 7. Weighted average cost after two receipts at different prices

> Matches an independently computed figure.

**MET — observed 2026-09-02.** The live figure, 13 paise/g over three receipts,
matched the invoices exactly.

Checking *why* it matched found two problems the criterion could not see:

1. **Per-receipt rounding.** `unit_cost_paise` is a RATE rounded to whole paise
   once per receipt, and the average summed that rate — ±0.5 paise on a per-gram
   figure, **+20% at 2.5 paise/g**, one-directional per item and worst on cheap
   staples. The dataset passed only because 10, 10 and 18 divide evenly; it was
   chosen to make the average vary, not to make the rounding fail. **Fixed** by
   ADR-021 / contracts 0.6.3: the ledger stores `line_total_paise` and the
   division happens once.
2. **An undocumented definition.** The averaging query is unbounded, so what
   shipped is a **lifetime cumulative purchase-weighted average, not weighted
   average cost of stock on hand**. Only half of that was ever decided. Not
   folded into 0.6.3; filed in `docs/backlog.md` against the first pilot.

**The retro line for this milestone:** an acceptance criterion satisfied by
either of two definitions cannot tell you which one you built. Criterion 7 is
definition-neutral as written, so it passes under both and can report neither.

---

## Verification run at close (2026-09-02)

Executed from the repository root on this machine, after `262e03a`:

| Suite | Command | Result |
|---|---|---|
| `edge/sync` | `cargo test --manifest-path edge/sync/Cargo.toml` | **56 passed, 0 failed** (3 consecutive runs) |
| `edge/database` | `cargo test --manifest-path edge/database/Cargo.toml` | **passed, 0 failed** |
| `edge/device` | `cargo test --manifest-path edge/device/Cargo.toml` | **11 passed, 0 failed** |
| `edge/printer` | `cargo test --manifest-path edge/printer/Cargo.toml` | **45 passed, 0 failed** |
| seams | `cargo check --all-targets` on `apps/pos/src-tauri`, `tests/e2e-scenario/harness`, `tests/integration/kds-lan-bridge` | **3/3 clean** |

Two notes a future session needs, because both cost time here:

- **`make` is not on PATH in the Bash tool on this machine, and there is no
  workspace `Cargo.toml` at the repository root.** `make check-seams` and a bare
  `cargo test` both fail, and the failure can exit **0** through a pipe — so a
  green-looking line proved nothing. Run the three seam manifests directly, or
  run `make` from PowerShell.
- **`edge/sync`'s `stale_connection.rs` failed once and has not reproduced.**
  `a_connection_killed_without_a_response_is_retried_rather_than_called_offline`
  reported `HttpTransport` on the first run after a cold build, while three
  `cargo check` jobs were saturating the machine; it passed alone immediately
  after and 3/3 in repeat full-suite runs. Recorded as load-sensitive rather than
  dismissed — filed in `docs/backlog.md`.

---

## Reconciliation: GRN/20260902/0001, 0002, 0003 — UNRESOLVED

The criterion-6 run produced a contradiction that must not enter the record as
though it were settled:

- The **pre-drain baseline** named the two pending receipts **0001 and 0002**.
- The **post-drain comparison** named them **0002 and 0003**.

One of those is wrong. It cannot be resolved from this repository: the numbers
were reported in the session, the session is gone, and neither store can be read
right now — Docker Desktop is down so PostgreSQL is not running, and the edge
database is encrypted with a key supplied at runtime through
`HOLLER_DB_KEY_HEX`, which is deliberately not stored anywhere in the repository
(`docs/DEV_SETUP.md`).

**It does not affect any criterion's verdict.** Criterion 6 was judged on
`published=6`, the outbox count moving 126 → 120, and a field-by-field compare of
the edge and cloud rows — none of which depends on which ordinal a receipt
carries. Criterion 1 recorded `GRN/20260902/0002` from the screen at the time of
receipt.

**How to resolve it** (~5 minutes, once both stores are readable):

1. Start Docker Desktop, then `docker compose up -d postgres`.
2. Edge, with the run's `HOLLER_DB_KEY_HEX` exported — three columns per number:

   ```sql
   SELECT g.grn_number, g.id, o.published_at, o.attempt_count
     FROM goods_receipt_note g
     LEFT JOIN local_outbox o
       ON o.aggregate_id = g.id AND o.aggregate_type = 'goods_receipt_note'
    WHERE g.grn_number LIKE 'GRN/20260902/%'
    ORDER BY g.grn_number;
   ```

3. Cloud:

   ```sql
   SELECT grn_number, id, created_at
     FROM goods_receipt_note
    WHERE grn_number LIKE 'GRN/20260902/%'
    ORDER BY grn_number;
   ```

Three rows at the edge with all three present in the cloud and `published_at`
set on each says the post-drain naming was right; two rows says the baseline was.
Record the answer here and delete this section's UNRESOLVED heading — do not
leave it inferred.

---

## Carried forward into M6 — pilot blockers

Every item below is in `docs/backlog.md`, the single register. **None blocks M5's
close; all block a pilot.** Listed here so the milestone's exit hands them over
explicitly rather than leaving them to be rediscovered.

| Item | Why it blocks a pilot |
|---|---|
| **A 500 on a replayed row wedges the outbox forever** | Observed live. `POST /orders/{id}/items` returns 500 on an FK violation (a permanent client-data fault reported as a server fault); the edge treats 5xx as transient, so the row never spends retry budget, and the general outbox has no per-entry budget at all — one unreplayable row strands every row behind it. 120 rows observed pending. |
| **Abnormal exit bypasses the shutdown drain** | `RunEvent::Exit` does not fire on Ctrl+C, taskkill, crash or power loss. A till that crashes once never syncs again until someone closes it cleanly, and looks healthy throughout. |
| **No periodic pump while the till is open** | A full trading day sits in one encrypted file on one spinning disk with no off-machine copy. Raised in priority by the row above; one timer calling the already-bounded `AppState::drain_outbox` fixes both. |
| **Cost is a lifetime cumulative average, not weighted average cost of stock on hand** | Undocumented product definition behind a headline claim. `stock_balance_snapshot.through_entry_seq` is the bound an on-hand WAC would read and already exists. |
| **Tax-inclusive vs tax-exclusive purchase price is not distinguished at entry** | The delivery-note figure is commonly GST-inclusive in this market; typing it lands an inflated cost on every ledger row, silently. |
| **The cloud menu seed is a token (2 rows) and the edge's is real (43)** | Root cause of the replay 500. With the inventory-config gap, says the cloud→edge config push has never been exercised for either catalogue. |
| **Device enrollment has no operator-facing flow** | Contract shapes exist; no flow does, and there is no device LIST route. |
| **`outlet.manage` is a de-facto admin role** | One grant gates table config, GSTIN/compliance config and hardware enrollment. T7a's `billing.manage` was the first slice, not the remedy. |

Two **PARKED** hardware gates remain open and block **M3**, not M5: ESC/POS on
paper, and the bare 4GB Windows 10 VM run (ADR-013).
