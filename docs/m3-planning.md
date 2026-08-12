# Planning inputs for Milestone 3 — Billing

Written at the close of Milestone 2 (2026-08-12), while the reasoning is fresh. M3's own scope is §81: tax engine, GST invoice, discounts, split bills, split payments, cash shift, invoice numbering, with the extensive financial tests §66 requires.

These are inputs to the M3 task graph, not the graph itself.

---

## 1. Device enrollment is the first track, or a pre-track

**It blocks any pilot deployment.** Not a hardening nicety — three separate holes that are one missing mechanism, all recorded under "Device enrollment" in `docs/backlog-m2.md`:

- The edge sync worker takes `tenant_id`/`device_id` at construction with nothing to verify them against, so a mis-enrolled node silently mislabels every outbound envelope.
- `GET /sync/config` — the one route that ships Argon2id password and PIN hashes — is gated on an ordinary human bearer token with `user.manage`. An enrolled edge node and a logged-in browser session are indistinguishable to the backend.
- The KDS LAN port authenticates only that a `device_id` exists, over plaintext. Anyone reaching it can read every ticket and call `set_kot_status` — marking food SERVED when it never left the kitchen, or CANCELLED on a live ticket.

**Why it must come before billing rather than after:** M3 puts money on the wire. Invoices, payments and cash-shift records are the first data where an unauthenticated device is a financial problem rather than an operational one. Building a billing surface on top of an unauthenticated edge means the enrollment work later has to re-secure a larger surface than it would today.

Groundwork already laid: `lan.ts` reserves an optional `device_token` handshake parameter (ADR-015), so enrollment lands as a behaviour change — server strictness — rather than a contract shape change, and needs a minor bump rather than 0.4.0. **When verification turns on, move `device_token` out of the query string** (Authorization header or first-frame auth); a secret in a query string is a secret in every proxy log.

Also unresolved and in scope for the same track: `app_user.config_version` bumps only on create and role change, so a future password or PIN change would never reach the edge cache and a cashier would keep authenticating offline with the old credential.

---

## 2. Tracks A and B fold into the M3 task graph — **B before any billing math**

Both were approved at the end of M2 and are unstarted.

**Track A — unrouted-line guard.** A mixed order containing one line with no station sends silently: the line reaches no kitchen and nothing surfaces (`edge/database/src/lib.rs:528-531` skips it; the guard at `542-546` only fires when *every* line is unrouted). 56 hits across 204 harness scenarios. Fix per §64 — staff must be told whether intervention is needed, e.g. *"2 items have no kitchen station — not sent."* Includes the harness regression case and moving the harness's hard-coded `REPORT.md` path under a scratch/run dir.

**Track B — cart surface: quantity control, modifier attachment, `#132-A` post-DRAFT item addition.**

**B is a hard prerequisite for billing math, not an ordering preference.** Billing computes tax and totals over line items — and today:

- There is **no quantity control**: five taps of one item produce five lines of quantity 1. Any tax or discount logic written against that is being validated on a data shape the product will not have once quantity lands.
- **Modifiers cannot be attached to an order line at all** from the shipped command surface (`NewOrderItemRequest` carries no modifiers field). Modifier price deltas are real money and land in the taxable base.

The consequence is already measurable: the e2e harness's money invariant has passed 204/204 scenarios **without ever exercising a modifier price delta**, because nothing can create one. Writing a tax engine against that is writing it against a fiction. Land B, let the money invariant see deltas and real quantities, *then* build billing on top.

Constraint carried forward: **do not implement quantity as remove-then-add.** That is two durable writes with a crash window between them — precisely the loss the durable-cart work eliminated.

`#132-C` cancellation (`cancel_kitchen_items_with_outbox` has no Tauri command) stays in the backlog; it is not a billing prerequisite.

---

## 3. UI polish is deferred until after M3 — no builder time on visuals

No builder spends time on visual refinement, layout, theming or animation during M3. Functional UI only: the control exists, is legible, is reachable, and states what happened.

Two exceptions, because they are correctness rather than polish and are already binding:

- **Never colour-only** (`docs/spec/kitchen.md` §KDS) — status must carry text and time alongside any colour. A colour-blind cook in a hot kitchen gets the same information.
- **§64 error design** — errors tell staff whether intervention is necessary. "Something went wrong" is not acceptable, and neither is silence, which is what the two open P1 defects actually are.

The reasoning: M2 shipped five defects that reached a human, and none of them were visual. The scarce resource is verification of behaviour under real conditions, not appearance.

---

## Carried-forward gates (not new work, but they still bind)

- **ADR-013: nothing has ever run on bare Windows 10.** Now also covers USB/Bluetooth printing — only network printers were exercised against a real socket — and real-LAN latency on outlet-grade hardware.
- **Latency headroom is ~30%, not an order of magnitude.** M2 measured 150–183ms against a 250ms target over real WiFi; the harness measures P50 13ms on one machine, so WiFi adds ~140ms. Re-measure at an outlet with several screens.
- **The e2e harness CI job cannot go red on an invariant failure** — it asserts only harness-level fatals, because the known defects would make it permanently red. Once Track A closes the mixed-order defect, tighten it to per-invariant assertions or a baseline, or it stays a smoke test rather than a regression gate.
- **`devseed` seeds no printer**, so the print path is unexercised in development — which is why a KOT that can never be queued for print was found only on real hardware.
