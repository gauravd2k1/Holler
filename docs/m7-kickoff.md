# Milestone 7 — kickoff and scoping

Written 2026-09-17, after the Shinjuku Yakitori client demo. This file turns the
operator's demo feedback (F1–F6) into planned work. **It is a proposal until the
operator approves it**; a landing milestone recorded here is a filing, not a
commitment, in the sense `docs/backlog.md` already defines.

The demo-period freeze ("no new features", `CLAUDE.md`, *Scope until Wednesday*)
is lifted by the operator as of this session.

---

## 0. State at the start of this session

**CI status of `2b62795` (HEAD), run `35219643824`: 15 of 16 jobs green, one
red — `e2e-scenario`.** `backend`, which `docs/RESUME.md` recorded as **RED, NEW,
CAUSE UNKNOWN** on `045b68e`, is **green on this run with no code change to the
backend**. That settles the question RESUME.md recorded as unresolved by the
query it named: it was flaky, not real. The record is corrected rather than
quietly dropped.

`e2e-scenario` fails one assertion:

```
invariant 9_tax_reconciliation must pass on every scenario that checked it:
expected 33 to be +0
issue_invoice rejected on a CONFIRMED-or-later order with lines:
{"code":"INVALID_INPUT","message":"no tax rules for profile
 0191c000-0000-7000-8000-000000000002 under compliance version
 0191a000-0000-7000-8000-000000000040"}
```

**This is fully diagnosed already, in `docs/RESUME.md` (§ "`e2e-scenario` —
diagnosed, deliberately NOT fixed"), and the fix is already agreed as post-demo
open item 3. Read that section; do not re-derive it.** In short: the harness
mints a **second** compliance version for the outlet, so
`resolve_compliance_version` picks **devseed's** (`0191a000-…-0040`), under
which the harness's own tax profile (`0191c000-…-0002`, rules pinned to the
harness's `…-0001`) has no rules. The harness's own comment saying devseed seeds
no `tax_profile` row is **stale** — `seed_billing` does now.

RESUME.md also rules out the two things a reader would otherwise suspect: it is
**not** the zero-rate bar profile (`devseed.rs:2936-2940` gives it real rules at
0 bps — rules present, rate zero) and it is **not reachable by a printed bill**.

So the red is a harness-seeding defect, not the taxed bill path, which the demo
exercised on real hardware. **The agreed fix is one line of intent: the harness
reuses devseed's compliance version instead of minting its own.** It is
scheduled as **M7-B0** below — it was deliberately not fixed during the freeze,
and that reason has expired.

### Corrections to the record

- **`docs/RESUME.md` opens "THE DEMO DID NOT RUN."** The operator reports the
  demo ran and went well. The repository is the authority on the code; the
  operator is the authority on what happened in the room. RESUME.md is corrected
  in this session.
- **`CLAUDE.md` says "Contracts stay FROZEN at v0.8.1" in the demo-build
  section**, while its own contracts heading and `packages/contracts/package.json`
  both read **0.8.3**. The demo-era sentence is stale (0.8.2 and 0.8.3 landed
  after it was written). **0.8.3 is the real baseline** and every version number
  below counts from it.
- **F1 is not an unexplored bug.** It was investigated during the rehearsal and
  the finding is committed at `46c176a` / `docs/demo-runbook.md:416-450`. The
  hypotheses named in the feedback — bind address, localhost vs LAN IP, CORS,
  port — were **measured and eliminated at the time**. Detail in §F1.

### Carried, still live, now unfrozen

The demo freeze deferred three Phase A gaps and they are still open. With the
demo behind us they are the strongest candidates for M7 alongside the feedback:

- **A6 — no exit path seals the edge database.** Neither a window close nor
  `Ctrl+C` fires `RunEvent::Exit`, so a plaintext `edge.db` is left beside the
  `.enc` on every run and the `.enc` is only as current as the last successful
  seal. **No trustworthy backup can be taken before a risky migration.**
- **A7 — 78 rows on the live edge database have no route and can never be
  sent** (55 `kot`, 22 `stock_count`, 1 `invoice`, measured 2026-09-07).
  `edge/sync/src/route.rs` maps only `order` and `table_session`.
- **A4** — carried, see `docs/RESUME.md`.

These are pilot blockers, not demo blockers, and the reason for deferring them
has expired.

---

## F1 — Admin console fails on the demo laptop ("Failed to fetch")

### Root cause

**Already established, 2026-09-17, mid-rehearsal (`46c176a`).** The admin
console signed in from VS Code's Simple Browser and failed in Chrome **at the
same URL, on the same machine, at the same moment**. Everything server-side was
measured clean at that moment:

- `/health` returned 200 on both `127.0.0.1` and `[::1]`
- preflight from `http://localhost:5175` returned 204 **with**
  `Access-Control-Allow-Origin`
- `apps/admin/.env.local` carried all three required variables
- Vite had been started hours after that file was last written

So the request never left the browser. The four candidate causes, in likelihood
order, are recorded at `docs/demo-runbook.md:433-447`: Chrome HTTPS-First
upgrading `http://localhost:8080`, an extension blocking cross-port localhost, a
stale service worker on 5175, the wrong origin in the address bar.

**Say this out loud before acting:** the feedback's hypotheses (bind address,
localhost vs LAN IP, CORS, port) were tested that day and none of them held. Re-
running that investigation would repeat work the repository already holds.

### What is nonetheless a real defect

The browser-side failure is a symptom of a product shape that will not survive a
pilot, and *that* is the M7 work:

1. **The admin console has no production serving story.** It is run by a human
   typing `pnpm dev` in `apps/admin` (`CLAUDE.md`, *Rebuilding the stack from
   cold*, step 4). A client cannot operate that, and a Vite dev server is a
   fourth runtime with its own failure modes.
2. **CORS allows exactly one origin and compares the string**
   (`backend/internal/.../config.go:62`). `http://127.0.0.1:5175` is refused
   with no `Access-Control-Allow-Origin` and the browser reports that as
   "Failed to fetch", which reads as a rejected password.
3. **The sign-in form cannot tell the operator which of the three failures
   happened.** The three messages are documented side by side in the runbook but
   the screen does not distinguish a blocked request from a refused credential.

### Milestone placement

**M7, bug track B1.** Not a patch against M6 — the fix is a serving change, not
a one-line correction.

### Tracks

| Track | Work |
|---|---|
| **B1-T0** | Reproduce the Chrome failure once with the Console snippet at `docs/demo-runbook.md:450` so the cause **names itself**. Record which of the four it was. This is the falsifier: without it, every fix below is speculative |
| **B1-T1** | Serve `apps/admin` as a **static build** from the backend (or a static file server), not a dev server. Removes the 5175 origin entirely and with it the whole class |
| **B1-T2** | Make the sign-in form distinguish *request never left* from *API refused* from *missing config*, on screen, in the operator's words. `session.ts:85`, `api.ts:35` already hold the three branches |
| **B1-T3** | Widen the CORS allowlist handling so a same-machine origin variant is not a silent refusal. `-AdminOrigin` already accepts a comma-separated list; the default does not |

### Contract impact

**None.** No schema, no sync payload, no wire type. `config.go` and the admin
app only.

### Acceptance criteria

1. The Console snippet run in Chrome on the demo laptop names the cause; the
   name is written into the acceptance file. *(A criterion a pre-fix binary also
   passes is not evidence — so this one records the **cause**, not the symptom.)*
2. The admin console is opened from a **static build** on the demo laptop, in
   Chrome, with extensions enabled and HTTPS-First left at its default, and
   signs in.
3. Each of the three sign-in failure modes is provoked deliberately and the
   screen names it correctly. Provoke the credential failure with a wrong
   password, the config failure by unsetting a variable, the transport failure
   by stopping the backend.
4. Evidence committed per the standing rule: what was observed, how the
   precondition was established, who observed it, on what date.

---

## F4 — Kitchen status does not refresh on the order window

### Root cause

**Half of this was already filed.** `docs/backlog.md` carries the row *"KOT
return leg: the BUMP works, the two SURFACES that consume it do not"*, and its
item (a) names `useKotsForOrderQuery` with no `refetchInterval` as the cause. It
was filed 2026-09-14 with the trigger *before the first pilot*, and **NO WORK
BEFORE WEDNESDAY** — that embargo has expired. Say this out loud rather than
re-deriving it: the repository already held the answer.

That row also makes two corrections worth carrying, because the brief it was
filed from was wrong on both: **the KDS bump already works over 9310** and
already writes at the edge through the KOT state machine
(`edge/device/src/server.rs:345`), and **no contract change is needed** — every
KOT status has existed in `KotStatusSchema` since the M0.5 freeze.

**Confirmed again by inspection this session, and it is the same defect as the
order list.**

`useKotsForOrderQuery` (`apps/pos/src/lib/queries.ts:103`) is a plain
`useQuery` with **no `refetchInterval`**, and `apps/pos/src/App.tsx:14` sets
`refetchOnWindowFocus: false`. The KOT panel is a child component mounted only
while the row is expanded (`OrderListScreen.tsx:227,290`). So:

- while the panel is open, **nothing ever refetches it** — the KDS bumping a
  ticket changes a row the till has no reason to re-read;
- collapsing and re-expanding **unmounts and remounts** the child, and the
  default `staleTime: 0` makes the mount refetch.

That is exactly the reported symptom: updates appear only after toggling
"Hide Kitchen / Kitchen". Not a missing subscription — there was never a
subscription; it is a cached query with no trigger.

### The same defect was found in the order list this session

A waiter's order placed from the captain app did not appear in the POS order
list. `useOrdersQuery` (`queries.ts:95`) had the same shape. **The captain
server writes the order inside the POS process**
(`apps/pos/src-tauri/src/captain.rs:469` → `create_order_impl_as`), so no Tauri
mutation runs in the webview and nothing invalidates the key. Every
`invalidateQueries` on that key is a till-side mutation
(`OrderListScreen.tsx:72,101,317,337`, `PosScreen.tsx:282`).

A one-line `refetchInterval: 5000` was added to `useOrdersQuery` during that
session and is **uncommitted, and unverified in the Tauri release window**. It
is folded into this track rather than left loose.

**The general shape is worth naming, because a third instance is likely:** the
till is no longer the only writer of its own state. The KDS writes KOT status
over the LAN socket; the captain writes orders over HTTP; both land in the edge
database inside the POS process without the webview hearing anything. Every
react-query key whose underlying row can be written by a non-till actor is
stale by construction.

### Milestone placement

**M7, bug track B2.** The one-line fixes are a patch; the enumeration is the
milestone work.

### Tracks

| Track | Work |
|---|---|
| **B2-T0** | **Enumerate the sinks, not the screens.** List every write path into the edge database that does not originate in the POS webview — the LAN server's `set_kot_status` (`edge/device/src/contract.rs:165`), every captain route (`captain.rs`), the sync config apply, the outbox pump. For each, list the react-query keys it invalidates. That closed set, not a walk of the UI, is the scope |
| **B2-T1** | Land the polling for the keys B2-T0 identifies: orders, KOTs for an order, table sessions. Commit the loose `useOrdersQuery` change here |
| **B2-T2** | Decide whether polling is the right mechanism or a stopgap. The POS process **knows** when the captain or the KDS wrote — a Tauri event emitted from the Rust side would be exact instead of eventually-consistent. Propose, do not build, inside this track |
| **B2-T3** | A guard: something that fails when a query key reachable from a non-webview writer has neither a poll nor an invalidation. Without it this recurs under the next feature's name |

### Contract impact

**None** for B2-T0/T1/T3. B2-T2 may add a Tauri event name, which is not a
contract surface (`packages/contracts` is untouched).

### Acceptance criteria

1. With the order window open and untouched, a KDS bump is visible on the till
   within the stated interval. **Observed in the Tauri release window**, not in
   `pnpm build` or the dev server — the four-runtimes rule.
2. With the POS order list open and untouched, an order placed from a waiter's
   phone appears within the stated interval.
3. B2-T0's sink list is committed as a file, and every key on it is accounted
   for by a poll, an invalidation, or an explicit "does not need one" with the
   reason.
4. B2-T3's guard is watched **letting a planted violation through before it is
   trusted** — a check is falsified by what it lets through, not by what it
   catches.

---

## F2 — Prep-time timer per table (waiter app)

### Design

Three separable parts. Only the first touches contracts.

**1. The attribute.** `menu_item.average_prep_seconds`, an integer, nullable,
cloud-owned config (cloud→edge), admin-editable, seeded from the operator's
list.

Seconds rather than minutes, and an integer, by the same rule money and
quantities already follow: a stored number means one thing, and "1.5" in a
minutes column is a rounding argument nobody wants. Nullable because an item
with no measured prep time must not block anything — the same shape as "a
missing recipe never fails a confirm".

**2. The anchor.** **No new timestamp column is needed.**
`kot_status_history(kot_id, status, changed_at)` already records every
transition, edge-local, on the edge clock
(`packages/contracts/sqlite/0005_m2_kitchen_stations_printers.sql:109-118`).
"Accepted by the kitchen" is a row in that table. The timer is
`changed_at + average_prep_seconds`, computed from stored rows.

This is what makes the restart requirement free: the timer is **derived, never
stored and never held in a client's memory**. A captain app that restarts
recomputes the same value; a phone with a wrong clock is irrelevant because the
anchor is the edge's.

**3. The surface.** The captain app's table page.

### Per-item or max-of-items?

**Recommend: the table timer is the MAX across the order's outstanding items;
per-item times are shown when the row is expanded.**

The waiter's question at a table is "when can I go back", and that is the
slowest item. A sum is wrong (stations work in parallel). A min is actively
misleading — it goes green while the table is still waiting. Per-item still
matters for the one dish that is late, so it is kept, one level down.

An item with a null `average_prep_seconds` contributes **nothing to the max**
and is shown as elapsed-only, with words. It must never make the table read
"ready".

**Show elapsed as well as remaining, and keep counting past zero.** A timer that
stops at 00:00 tells the waiter nothing about a table that has been waiting four
minutes too long, which is exactly the table that needs them.

### Milestone placement

**M7.** It is the client's own request, it is bounded, and its contract impact
is one column.

### Tracks

| Track | Work |
|---|---|
| **F2-T0** | **Contract 0.9.0: `menu_item.average_prep_seconds`.** Needs operator approval and an ADR **before commit**. The consumer list is the deliverable, not an afterthought — see below |
| **F2-T1** | Admin: edit the field on the menu item form. `PATCH /menu/items/{itemId}` mutates exactly five fields today (ADR-024) and this is a sixth — the route's field list is part of the contract change, not a free extension |
| **F2-T2** | Seed: load the operator's prep-time list into `seed/` alongside the menu |
| **F2-T3** | Edge: a read that returns, for a table, the outstanding KOT items with their accept time and prep seconds. Plain `fn …_impl(&AppState, …)` per the existing captain pattern |
| **F2-T4** | Captain JSON API: extend the table page payload. **Write the API document before the code**, per the item-0 rule that already governs this app |
| **F2-T5** | Captain UI: the table timer, the expanded per-item view, the null and overdue states |

### Contract impact

**YES — this touches frozen contracts. Contracts 0.9.0, ADR required, operator
approval before commit.**

The additive-change consumer list, which is what "complete" means here
(0.5.2 and 0.5.9 both landed incomplete and the column was silently dead):

| Surface | Change |
|---|---|
| `packages/contracts/sqlite/00xx` | `ALTER TABLE menu_item ADD COLUMN average_prep_seconds INTEGER` |
| `packages/contracts/postgres/00xx` | same |
| `packages/contracts/src/types/menu.ts` | Zod schema |
| Go struct (`backend/internal/menu`) | field |
| `openapi.yaml` | `MenuItem` shape **and** the `PATCH /menu/items/{itemId}` mutable-field list |
| Cloud repository | INSERT **and** SELECT — the 0.5.9 defect was exactly this hop |
| `GET /sync/config` bundle | the column travels or the edge never sees it |
| Edge `repo.rs` | INSERT and SELECT |
| `apps/pos/src-tauri/src/dto.rs` | the Tauri DTO, which nothing machine-checks (backlog) |
| `packages/contracts/fixtures/` | a fixture row with the field **populated**, not null — a fidelity test proves fidelity only for the fields its fixture populates |

`CHECK (average_prep_seconds IS NULL OR average_prep_seconds > 0)`. Not `>= 0`:
a zero-second prep time is a data-entry error wearing a valid value.

### Acceptance criteria

1. A prep time set in the admin console reaches a till through
   `GET /sync/config` **without a reseed**, and the value is read back from the
   edge database.
2. An order placed from a waiter's phone and accepted by the kitchen shows a
   countdown on that table's page on that phone, within the LAN, **with the
   cloud stopped**.
3. The captain app is force-closed and reopened; the timer shows the same
   remaining time, ±1 second. Falsifier: it must be wrong if the timer were held
   in memory or anchored on the phone's clock — so the test **changes the
   phone's clock** and the timer does not move.
4. An item with a null prep time never causes a table to read "ready", and says
   so in words.
5. A table past its prep time keeps counting and is visibly distinct.
6. `node scripts/check-contract-field-consumers.mjs` passes with **no exemption**
   for the new column. An exemption here would be the 0.5.2 defect by name.

---

## F3 — Dynamic pricing on occupancy (optional, toggleable)

### Design

**This is the largest item in the feedback and the only one that changes what a
stored number means.** It deserves the most scrutiny, not the least, because it
is marked optional.

What exists already:

- `discount_definition` is a real table, cloud→edge config, with
  `method IN ('PERCENT','AMOUNT')`, `value_bps`, `max_discount_paise`,
  `required_permission`, `is_active`, `effective_from`/`to`
  (`packages/contracts/sqlite/0006_m3_billing.sql:127-150`).
- `invoice.discount_paise` and `invoice_line.discount_paise` exist
  (`0006:212-213, 284`).
- Occupancy is derivable from `table_session`, which is **edge-authoritative**
  (ADR-011).

What does not exist:

- **Any rule that selects a discount automatically.** Today a cashier applies one.
- **Any record on the invoice of *which* definition was applied or *why*.**
  `discount_paise` is a number with no provenance. The feedback explicitly
  requires the why, so this is a contract change, not a UI change.
- Any occupancy threshold configuration.

### The three things that make this harder than it looks

1. **Tax.** A discount changes the taxable value. Tax is computed per line at
   full precision, summed per component, rounded half-up to paise **once**
   (`CLAUDE.md`, v0.4.0 rule; `edge/database/src/tax/`). A dynamic discount
   enters *before* that rounding or the arithmetic is wrong. It must never be
   applied in TypeScript or the Tauri layer.
2. **Authority.** The rule is cloud config; the occupancy it reads is
   edge-authoritative; the price the customer pays is computed at the edge. That
   is consistent with §50.1 — **provided the cloud never computes a price.** A
   cloud-side "current effective price" endpoint would make the cloud a second
   writer of money and is forbidden.
3. **Time.** Occupancy moves while an order is open. **The rule must be
   evaluated once and pinned**, at a named moment, or a waiter quotes one price
   and the bill prints another. Recommendation: pin at **order confirm**, record
   the evaluation on the order, and never re-evaluate. The alternative (evaluate
   at bill time) is defensible and must be *chosen*, not left to whichever code
   path runs first.

### Milestone placement

**Recommend: NOT M7. Propose M8, or an M7 Phase 2 after F2 and F6 land.**

Reasons, stated so the operator can overrule with the facts in hand:

- It is the only feedback item that changes money arithmetic, and money
  arithmetic is the part of this system with the most rules already written in
  blood.
- It needs its own ADR with a real design, not an amendment to an existing one.
- F2 and F6 are visible to the client on the next demo; F3 is a pricing policy
  the client has not yet operated a single service with.
- M7 already carries two bug tracks, a contract bump for F2, and the carried
  pilot blockers A6/A7.

If the operator wants it in M7, the honest trade is **A6 and A7 move out**, and
that should be a deliberate choice rather than a discovery in week three.

### Tracks (as proposed, wherever it lands)

| Track | Work |
|---|---|
| **F3-T0** | **ADR: occupancy-triggered pricing.** Threshold modes (seats vs tables — both required), evaluation moment, tax interaction, authority split, what the bill records. No code until approved |
| **F3-T1** | Contract: the rule table, the occupancy config, and the invoice provenance columns |
| **F3-T2** | Edge: occupancy computation from `table_session`, rule evaluation, pinning |
| **F3-T3** | Edge tax path: the discount enters the per-line computation before the single rounding |
| **F3-T4** | Admin: rule configuration screen, including the on/off toggle |
| **F3-T5** | Captain and till: the effective price shown, labelled as discounted |
| **F3-T6** | Receipt and invoice: the discount and its reason on the printed bill |

### Contract impact

**YES, substantially. A version bump and a full ADR.** At minimum:

- a rule table (cloud→edge config, `config_version`, the `discount_definition`
  precedent)
- occupancy threshold configuration — outlet-level, so likely on `outlet` or a
  child of the rule
- **invoice provenance**: the applied definition id and the reason. Whether this
  is columns on `invoice` or a child row is an ADR decision. A child row is the
  better shape if more than one rule can apply — and the feedback's "some or all
  menu items" implies exactly that.
- item scope: all / selected items / categories — a join table, not a column,
  and not a `BOTH` member. The `printer_role` precedent (0.4.7) is the one to
  follow, including its rule that **absence is never read as "yes"**.

### Acceptance criteria (draft, to be fixed by the ADR)

1. With the rule off, every price and every bill is byte-identical to today.
   This is the criterion that protects the existing money path and it is first
   deliberately.
2. Occupancy crossing the threshold changes the price shown on the captain app
   and on the till, in both threshold modes.
3. A bill issued under the rule records which rule applied and why, and the
   printed receipt says so in words a customer can read.
4. Tax on a discounted bill reconciles: the e2e harness's
   `9_tax_reconciliation` invariant passes on discounted scenarios.
5. An order confirmed under the rule and billed after occupancy recovered pays
   the **pinned** price, and the evidence names which moment was pinned.
6. The rule is toggled off mid-service and an open order behaves as the ADR
   says it must — not as whichever code path happens to run.

---

## F5 — Competitive research note (offline is not our moat)

### The correction, recorded

Earlier research treated offline-first as a differentiator. **Petpooja also
works without internet.** Offline is a baseline expectation in this market, not
a moat. This is a factual correction to the product argument and it belongs in
the record, not only in a chat.

Note what it does **not** change: ADR-013's deployment target and the
local-first architecture are still right. The outlet hardware argument stands on
its own. What changes is the **pitch**, and any roadmap item justified primarily
by "we work offline and they do not".

### Milestone placement

**M7, research track R1. Deliverable is a note. No code.**

### Tracks

| Track | Work |
|---|---|
| **R1-T0** | Petpooja, Posist, and two others — candidates: Rista, UrbanPiper (aggregator-side), Gofrugal, Dotpe. Feature inventory, pricing posture, where each is strong |
| **R1-T1** | **What we lack**, itemised against them — honestly, including things we have decided not to build |
| **R1-T2** | **Candidate differentiators, ranked**, each with what it would cost and what would have to be true for it to matter |
| **R1-T3** | Where each cites a source, cite it. A competitive note assembled from recall is how a roadmap gets built on a wrong premise — which is what happened here |

### Contract impact

**None.**

### Acceptance criteria

1. `docs/competitive-landscape.md` exists, committed, with sources.
2. The differentiator list is **ranked and costed**, and nothing on it has been
   started.
3. The offline correction is reflected wherever the old claim appears —
   `docs/vision.md` and any pitch material. A corrected belief that survives in
   three documents is not corrected.

---

## F6 — Menu layout and captain app look basic

### Design direction (proposal only)

The standard is the one already written into the demo work: **no dev labels, no
raw UUIDs, no internal notes; every empty state has words; every wait has a
spinner.** F6 raises it from "not embarrassing" to "looks finished".

Assets already in the tree, untracked: `menu_imgs_gong/` and
`docs/gong_menu.xlsx`. **These are the real menu's images and data** and they
should be the source for any mockup, not invented placeholders — a mockup built
on fake content hides exactly the problems real content causes (long Japanese
dish names, missing images, one category with thirty items).

Direction to propose, phone-first, in the order the operator already set for
polish: **captain page, KDS ticket, invoice screen, receipt**.

- Menu grid with category rail, item cards carrying image, name, price, and the
  section label the search results already show (`045b68e`).
- Holler logo and a consistent type scale across till, captain, KDS and admin.
- A defined behaviour for an item with **no image** — this is the state that
  makes a grid look broken, and roughly half the menu will be in it.

### Milestone placement

**M7 for the mockup direction and the captain/menu refresh. Implementation is a
separate track after the operator approves the direction**, per the feedback.

### Tracks

| Track | Work |
|---|---|
| **F6-T0** | Mockup direction: captain menu grid, item card, table page. Built from `menu_imgs_gong/` and `docs/gong_menu.xlsx`. **Approval gate** |
| **F6-T1** | Decide how images are stored and served. See contract impact — this is the one part of F6 that is not purely visual |
| **F6-T2** | Captain app implementation |
| **F6-T3** | Till menu screen implementation |
| **F6-T4** | Logo, type scale and shared tokens across the four surfaces |
| **F6-T5** | Screenshot every screen the demo story touches, as the demo work item 6 already required |

### Contract impact

**T0 and T4: none. T1: possibly yes, and it should be decided before any image
appears on a screen.**

An item image needs a reference. Three options, in increasing contract cost:

1. **Convention, no contract change** — images bundled with the app and matched
   on the item's existing code. Zero contract impact; breaks the moment an
   outlet wants to upload its own.
2. **`menu_item.image_path`** — additive column, cloud→edge config, full
   consumer list. The item is then onboarded by writing a path, consistent with
   "onboarding a restaurant is writing one file".
3. **An asset store** — out of scope for M7.

**Recommend option 1 for M7** so the visual refresh is not gated on a contract
bump, with option 2 filed to the backlog with its trigger (*the first outlet
that wants its own photographs*) named.

### Acceptance criteria

1. The direction is approved before implementation starts.
2. The captain menu is reviewed on a **real phone on the hotspot**, not a
   desktop browser at phone width.
3. An item with no image renders deliberately, and the operator confirms the
   grid still reads well with the real proportion of missing images.
4. **Any interaction over 1s on the phone is a defect** — the demo-era rule,
   kept. Report numbers, not impressions.
5. Every screen the demo story touches is screenshotted and committed.

---

## Proposed M7 scope

In dependency order.

| # | Item | Type | Contract |
|---|---|---|---|
| **B0** | `e2e-scenario` red: the harness mints a second compliance version (§0; fix already agreed in `docs/RESUME.md`) | Bug | None |
| **B1** | Admin console serving and sign-in error clarity (F1) | Bug | None |
| **B2** | Non-webview writers do not reach the screen (F4, and the order list) | Bug | None |
| **A6** | No exit path seals the edge database — carried pilot blocker | Bug | None |
| **A7** | 78 rows have no sync route — carried pilot blocker | Bug | None |
| **R1** | Competitive research note (F5) | Research | None |
| **F2** | Prep-time timer per table | Feature | **0.9.0 + ADR** |
| **F6** | Visual refresh, mockups first (F6-T0 gate) | Feature | None if option 1 |

B0–B2 first: they are cheap, they are all "the screen does not show what the
database holds", and B2's enumeration is a prerequisite for trusting anything
F2 puts on a screen.

A6 and A7 are in because the reason they were deferred has expired and both
block a pilot. **If F3 comes into M7, these are what move out.**

## Deferred

| Item | Why | Trigger |
|---|---|---|
| **F3 — dynamic pricing** | Changes money arithmetic; needs its own ADR; M7 is full | Operator decision, or M8 |
| **F6 option 2 — `menu_item.image_path`** | Contract bump not needed for the refresh itself | The first outlet that wants its own photographs |
| **A4** | Carried from M6 Phase A | Pilot |
| Everything already in `docs/backlog.md` | Unchanged by this session | As filed |

---

## Standing rules that bind this milestone

- **No contract change is committed without operator approval and a versioned
  ADR written first.** F2 and F3 both trip this.
- **An additive contract change has a consumer list, and it is not landed until
  every consumer carries it** — including the wire types and the repository's
  INSERT/SELECT (0.5.9).
- **A milestone does not close until its acceptance evidence is committed.** The
  chat is not the record.
- **A test invocation reporting zero tests executed is a failure.** Run test
  commands through `node scripts/assert-tests-ran.mjs`.
- **Every report opens with the CI status of HEAD.** Local green is not green.
- **Build-green is not dev-works.** There are four runtimes and the fourth is the
  Tauri release window.
