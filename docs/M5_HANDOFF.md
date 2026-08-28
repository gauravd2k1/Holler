# M5 handoff — written 2026-08-28 at the `m4-complete` tag

M4 (Inventory & Recipes) is tagged. This is what M5 inherits: what is true, what
is deliberately deferred, and the three things that will cost a day each if
rediscovered rather than read.

---

## 1. State at the tag

- **Contracts FROZEN at v0.5.9.** SQLite through 0026, Postgres through 0027.
  Read-only to builders (ADR-008). `scripts/check-milestone-marker.mjs` fails the
  build if the CLAUDE.md heading disagrees with `package.json`.
- **All seven M4 acceptance criteria observed against the shipping binaries**,
  none evidenced by a test harness. Criterion 1 was the last to close: the till
  now resolves a variant (`7e88d1c`), which had blocked it since 2026-08-27.
- **`docs/RESUME.md` §2(a) is CLOSED** by that commit — a sale through the POS
  deducts stock. §2(b) (`gh` unauthenticated) is still open and still means a
  push is fire-and-forget.
- **The two ADR-013 hardware gates remain open** and block **M3**, not M4:
  ESC/POS on paper, and the bare 4GB Windows 10 VM run. Parked 2026-08-20,
  revisit ~2 September 2026. Do not re-litigate them; source the hardware.

---

## 2. Three constraints on M5 work, in the order they will bite

### 2.1 The receiving screen gets `entryIntentEcho` at build time, not after

M5 procurement introduces a **third quantity-entry path**. There are currently
two, both fixed on 2026-08-28 (`afb5aa0`): the physical count and wastage. The
receiving screen is the same 1000x trap with worse odds:

- larger quantities than a count,
- read off a delivery note written in the **supplier's** units,
- entered by someone reconciling against a document, not counting a shelf.

`entryIntentEcho(verb, quantity, dimension, itemName, qualifier?)` lives in
`apps/pos/src/domain/inventory.ts`. Use it. A receipt is a **movement**, so it
takes no qualifier — `Receiving 5,000 millilitres of Sunflower Oil`. See
`docs/retro.md` 2026-08-28 for why a label alone is not the fix.

### 2.2 The enum consumer check must be built in this order or it ships inert

`scripts/check-contract-field-consumers.mjs` currently covers **fields**. It
should cover **enum values** too — but the corpus must be narrowed first.

Measured on 2026-08-28: the six unwritten `stock_ledger_entry.entry_type` values
appear in the consumer roots **only** in a doc comment enumerating the CHECK
constraint (`edge/database/src/model.rs:1248-1250`), plus `"PURCHASE"` once in a
test fixture (`edge/database/src/stock/variance.rs:150`).

So:

1. **First** exclude doc comments and `#[cfg(test)]` modules from the consumer
   corpus. Without this the enum check reports all six green on day one — a
   comment listing permitted values is indistinguishable from a branch acting on
   one, under a grep. This is the DECLARED-versus-ACTED-ON gap the script's own
   header already admits to and files at `docs/RESUME.md` §6.
2. **Then** add the six to `EXEMPT` with the milestone named:
   - `PURCHASE`, `TRANSFER_IN`, `TRANSFER_OUT`, `RETURN_TO_VENDOR` — **M5**
     (procurement, inter-outlet transfer).
   - `PRODUCTION_CONSUMPTION`, `PRODUCTION_OUTPUT` — **M8** (central kitchen;
     currently on M4's EXCLUDES list under `semi_finished_batch`).

Declaring them puts the roadmap in the schema. Six dead CHECK branches read as
oversight to whoever opens that migration next, and **no ADR currently says why
`TRANSFER_IN` exists at all** — these exemption reasons will be the only written
record of that scheduling.

The class this check exists for stands at **eleven instances across M4**, up from
the five it was written against.

### 2.3 Deferred M5 columns already on the exemption list

Already modelled, already exempt, waiting for procurement to consume them:

- `unit_cost_paise` — the ledger shape must not change when purchasing lands.
- `yield_factor_ppm` — trim/yield loss is authored with procurement;
  `YIELD_FACTOR_PPM_IDENTITY` is the only value any current path uses.

When M5 wires these, **remove the exemptions**. An exemption that outlives its
reason is a silenced failure.

---

## 3. Verification methods M5 should not have to rediscover

- **Enumerate sinks, not surfaces**, to prove a UI-level concern is covered
  (CLAUDE.md, "Response rules for agents"). A screen can be missed; a write path
  cannot.
- **Build-green is not dev-works.** Name the runtime a frontend change was
  observed in. Two incidents, both invisible to every green suite.
- **Drive the shipped surface by hand at every milestone boundary.** Twenty
  minutes of clicking found six defects on 2026-08-24. Every entry in
  `docs/retro.md` since has been found the same way, including this milestone's
  last two.
- **A fidelity test proves fidelity only for the fields its fixture populates**
  (contracts 0.5.9). Every provenance group needs its own populated row.

---

## 4. M5 scope reminder

Procurement: POs, GRN, suppliers, purchase pricing feeding `unit_cost_paise`,
and the batch/expiry **alerting** deferred from M4 (the fields are already
modelled). Inter-outlet transfer is what `TRANSFER_IN`/`TRANSFER_OUT` are for.

Still excluded: central kitchen (M8), aggregator auto-snooze on stock-out,
food-cost dashboards, the menu-engineering matrix, the waiter app (M9).
