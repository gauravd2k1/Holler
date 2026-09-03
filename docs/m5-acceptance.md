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
- **`edge/sync`'s `stale_connection.rs` failure is DIAGNOSED AND FIXED, and it
  was neither a flake nor a hardware finding.** It failed on an **idle** machine
  about 1 run in 30, in **0.00 s**, on **`os error 10054`** (WSAECONNRESET) — no
  timeout was involved (the agent's are connect 5 s, read 15 s, write 15 s). The
  test's own fake server answered after a single `read`, so a request split
  across TCP segments left bytes unread; closing such a socket on Windows sends
  an RST, and an RST **discards the send buffer, including the 201 already
  written**. The test therefore manufactured, intermittently, the exact
  false-offline it exists to detect. Fixed by draining the whole request in both
  fake servers in that file: **0 failures in 200 idle runs, 0 in 100 runs under
  48 busy loops on 24 cores**, against ~1 in 30 before. No retry budget was ever
  at risk — a transport failure is classified transient and charges nothing.

- **Every command in the table above now runs through
  `node scripts/assert-tests-ran.mjs`, which fails a job that executes zero
  tests.** The counts are re-measured through it as of 2026-09-02:
  `edge/database` **316**, `edge/sync` **56**, `edge/printer` **45**,
  `edge/device` **11**, `apps/pos/src-tauri` **80**, `apps/pos` **230**,
  `apps/kds` **30**, `packages/contracts` **67**. `backend` could not be
  re-measured — PostgreSQL is not running (Docker Desktop is down), and its
  suite is the one that must never be run with `HOLLER_SKIP_PG_TESTS=1`.

---

## Reconciliation: GRN/20260902/0001, 0002, 0003 — RESOLVED 2026-09-03

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

### The answer: the POST-DRAIN naming was right, the baseline was wrong

Read 2026-09-03 with Docker up and the sealed edge file readable. **Three rows
exist in both stores**, and `local_outbox.published_at` — the outbox's own record
of publication, not an inference from clustering — settles which two were pending
at the baseline:

| GRN | edge `published_at` | edge `attempt_count` | cloud `ingested_at` |
|---|---|---|---|
| `GRN/20260902/0001` | 2026-09-02T05:13:13.287Z | 0 | 2026-09-02 05:13:13.272Z |
| `GRN/20260902/0002` | 2026-09-02T05:48:26.702Z | 0 | 2026-09-02 05:48:26.692Z |
| `GRN/20260902/0003` | 2026-09-02T05:48:27.033Z | 0 | 2026-09-02 05:48:27.020Z |

`0001` was published at 05:13, well before the drain; `0002` and `0003` published
a second apart at 05:48, which is the drain. **So the two receipts pending at the
baseline were `0002` and `0003`** — the post-drain comparison was correct and the
pre-drain baseline was wrong. Criterion 1's screen capture of `GRN/20260902/0002`
at the moment of receipt agrees.

**No verdict changes.** Criterion 6 was judged on `published=6`, the outbox count
moving 126 → 120, and a field-by-field compare — none of which depends on the
ordinal.

**The pending-row count is confirmed at exactly 120** (published 75), settling the
other carried-forward item in the same reading. Its composition is the more useful
finding, and it is now M6 A2/A3 evidence:

```
kot          KOTStatusChanged     37   max_attempt_count=0
kot          KOTCreated           16   max_attempt_count=0
stock_count  StockCountOpened     12   max_attempt_count=0
stock_count  StockCountCompleted  10   max_attempt_count=0
order        OrderConfirmed        8   max_attempt_count=0
order        OrderReady            8   max_attempt_count=0
order        SentToKitchen         8   max_attempt_count=0
order        ItemQuantityChanged   7   max_attempt_count=0
order        OrderCreated          7   max_attempt_count=0
order        ItemAdded             6   max_attempt_count=10
invoice      InvoiceCreated        1   max_attempt_count=0
```

**One group spent its attempts; 114 rows were never attempted at all.** That is
head-of-line blocking (M6 A2) and a budget that counts without ever terminating
(M6 A3), visible in one query.

**Correction to the SQL recorded above: `goods_receipt_note` has no `created_at`
column in either store.** The cloud query fails with `ERROR: column "created_at"
does not exist` and the edge query with `no such column: created_at`. The columns
are `received_at` (when the delivery was taken) and, cloud-side only,
`ingested_at` (when the replay landed). The cloud query as run:

```sql
SELECT grn_number, id, received_at, business_date, ingested_at
  FROM goods_receipt_note
 WHERE grn_number LIKE 'GRN/20260902/%'
 ORDER BY grn_number;
```

**How the edge was read, and what was protected.** `Db::open` applies migrations,
seals unsealed stock snapshots, writes an unclean-session marker and re-seals on
close — so opening the artefact would have changed it. Instead the sealed file was
**copied**, the copy decrypted and opened READ ONLY by a scratchpad-only reader
that never entered the repository, and the plaintext overwritten and deleted
afterwards. `edge.db.enc` was never opened: sha256
`5d1297113003b9664038dfdbefc27289b61b86a9bcf19265dae5d04371a446c7` before and
after, and the copy carried the same digest. ADR-011's rule that the edge database
is never copied anywhere unencrypted is kept — what was copied was the sealed
file.

Recorded by the operator's reading, 2026-09-03, taken during M6 A1 and
alongside `99875cc` ("fix(m6-a1): report a foreign-key violation as 422, not
500"), which the 120-row composition above is the evidence for. The reading
itself changed no file in the repository — it is a query against two stores,
and this section is its only durable record.

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
