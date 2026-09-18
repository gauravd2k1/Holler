# B2-T0 — the write paths into the edge database that do not originate in the POS webview

**This is an enumeration of SINKS, not a walk of the screens.** "Which screens
look stale?" is a search over a list nobody maintains, answered by recall and
confirmation bias. "What writes the edge database without the webview knowing?"
is a search over a closed set the code already enforces, and it is the one that
can be finished.

Established by inspection on 2026-09-18, every claim carrying the file and line
it came from.

---

## The correction this enumeration forces

`docs/m7-kickoff.md` §F4 states that *"there is no push from the LAN hub into
the till's UI"* and that the kitchen panel therefore only refreshes on a
remount. **The repository contradicts that and the repository wins.**

A push channel exists and has since D14 (`97bc3dc`):

| Piece | Location |
|---|---|
| The event name, declared on both sides and drift-checked | `apps/pos/src-tauri/src/lib.rs:21`, `apps/pos/src/lib/kitchenEvents.ts:21`, `scripts/check-kitchen-event-drift.mjs` |
| The emitter — a thread subscribing to the in-process hub for **every** station, outlet-wide, forwarding `KotUpserted` / `KotRemoved` as a Tauri event | `apps/pos/src-tauri/src/lib.rs:62-90` |
| The listener, which invalidates **both** the KOT key and the orders key | `apps/pos/src/components/OrderListScreen.tsx:312-327` |

**So the mechanism B2-T2 was to "propose" is already built, already correct in
shape (the payload is an id; the row is re-read through the same command, never
patched from the event), and already covers any write that touches a KOT —
including one made by the captain.**

The defect is narrower and is a MOUNTING defect, not a missing channel:

> **The listener lived in `KotsPanel` (`OrderListScreen.tsx:280`), which is
> mounted only while an order's Kitchen panel is expanded.** Collapsed — which
> is the normal state, and the only state during the demo failure — nothing in
> the process was listening, so the event was emitted into nothing and the
> orders list was never invalidated.
>
> **FIXED 2026-09-18.** The subscription is now `KitchenChangedListener`,
> mounted in `App` inside `QueryClientProvider` and outside the router, so it
> is alive on every screen and across every navigation. It invalidates
> `orders` and **every** `kots` key by predicate, because a bump can land on an
> order whose panel is closed or on a screen that is not the order list. The
> guard checks the mount point rather than trusting it.

That single fact explains both reported symptoms at once, and explains why they
looked like two defects:

- **Kitchen status appears to update only on a remount.** With the panel open
  it already updates live. What was observed as "only on remount" is the
  collapsed case plus `staleTime: 0` refetching on the next mount.
- **A waiter's order never appeared in the order list.** A captain order that
  has not yet been sent to the kitchen produces **no KOT**, so the hub has
  nothing to broadcast, so no event fires no matter who is listening. See sink
  2a below.

---

## The sinks

**FIVE** writers reach the edge database inside the POS process without a Tauri
mutation running in the webview. Nothing else does: the till's own commands all
invalidate through `queryClient` at their call sites.

> **This section said FOUR when it was first written, and the fifth was found
> by B2-T3's guard on its first run** — by refusing a key (`unacceptedAggregatorOrders`)
> that this hand-written enumeration had not ruled on. That is the enumeration
> lesson turned on its author: a list assembled by reading is a list with
> something missing, and the only reliable finder is a check over a closed set
> the code already enforces. Two other keys (`grnGaps`,
> `purchaseOrderReceiptProgress`) and one wrong ruling (`blockedReplays`, which
> this document had recorded as polled and which is not) came out of the same
> run.

### 1. The LAN server — `set_kot_status`

| | |
|---|---|
| **Entry** | `edge/device/src/server.rs:345`, the only inbound LAN command (`edge/device/src/contract.rs:165`) |
| **Writes** | `kot.status`, `kot_status_history` |
| **Announces itself?** | **YES** — writes through the hub, which broadcasts `KotUpserted`, which `lib.rs:86` forwards as `holler://kitchen-changed` |
| **Keys that must react** | `kots(orderId)`, `orders` |
| **Status** | **COVERED** since 2026-09-18 — `KitchenChangedListener`, mounted in `App`. Was covered only while a Kitchen panel was expanded |

### 2. The captain HTTP API — three write routes

All three run inside the POS process (`apps/pos/src-tauri/src/captain.rs`) and
**none of them emits anything**: `captain.rs` contains no `emit` call.

| Route | Line | Writes | Reaches the hub? |
|---|---|---|---|
| **2a. `handle_create_order`** → `create_order_impl_as` | `captain.rs:447,469` | `order` | **NO.** No KOT exists yet, so there is nothing for the hub to broadcast. **This is the demo failure** |
| **2b. `handle_add_item`** → `add_order_item_impl` | `captain.rs:480,492` | `order_item` | **NO**, for the same reason, unless the order is already ticketed |
| **2c. `handle_send`** → `confirm_order_impl` + `send_order_to_kitchen_impl_as` | `captain.rs:531,568,579` | `order.status`, `kot`, `kot_item` | **YES**, incidentally — creating a KOT broadcasts `KotUpserted` |

**Keys that must react:** `orders`, `order(orderId)`, `kots(orderId)`,
`tables` (a captain order opens a table session).

**Status: PARTLY COVERED.** 2c is covered through the KOT channel — incidentally,
because creating a ticket broadcasts — and now from anywhere rather than only
from an open panel. **2a and 2b still have no channel at all**: an order that
has only been created or added to produces no KOT, so nothing broadcasts. They
are covered by the 15s fallback poll on `useOrdersQuery` and nothing better.
Emitting on the captain's own writes remains the exact fix and is not done.

### 3. The config pull and apply

| | |
|---|---|
| **Entry** | `edge/sync/src/config.rs:599` `pull_and_apply_config`, driven by the A5 periodic loop since contracts 0.7.0 |
| **Writes** | every cloud→edge config table: `menu_item`, `menu_category`, `menu_item_variant`, `menu_item_modifier`, `restaurant_table`, `station`, `printer`, `tax_profile`, `tax_rule`, `discount_definition`, `inventory_item`, `recipe`, … |
| **Announces itself?** | **NO** |
| **Keys that must react** | `menuItems`, `menuCategories`, `menuItemVariants`, `tables`, `stations`, `discountDefinitions`, `outletIdentity` |
| **Status** | **UNCOVERED.** A price edited in the cloud reaches the edge database and does not reach the screen until the app restarts or something else invalidates. Note this was dead code with no production caller until 0.7.0, so the staleness is newer than the queries are |

### 4. The outbox pump

| | |
|---|---|
| **Entry** | the A5 periodic loop, same lock as the config apply |
| **Writes** | `outbox` row state, `sync_state` cursors, `sync_replay_block` |
| **Announces itself?** | **NO** |
| **Keys that must react** | `blockedOutboxRows`, `unroutableOutboxRows`, `persistentlyFailingOutboxRows`, `blockedReplays` |
| **Status** | **PARTLY COVERED.** Three of the four are on a 15s `refetchInterval` (`queries.ts:129,151,162`), written for exactly this reason — "a condition someone must act on has to reach the screen without anyone navigating to it". **`blockedReplays` is NOT**, which this document asserted before B2-T3's guard checked it |

### 5. The aggregator pull

| | |
|---|---|
| **Entry** | `edge/sync/src/aggregator.rs:75` `pull_and_apply_aggregator_orders`, called from the A5 worker loop (`edge/sync/src/worker.rs:313`) under the same database lock as the outbox pump and the config apply |
| **Writes** | `aggregator_order`, and the `order` rows those documents become (`repo::apply_aggregator_order`, `aggregator.rs:120`) |
| **Announces itself?** | **NO** |
| **Keys that must react** | `unacceptedAggregatorOrders`, `orders` |
| **Status** | **UNCOVERED.** An aggregator order lands in the edge database from a background loop and reaches no screen until something else refetches. **This is demo step 6's path** — the fake ONDC order that arrives and is accepted on the till |

### Not a sink, recorded so nobody re-derives it

**The print spool** (`failedPrintJobs`, `queries.ts:118`, 5s poll) is a
webview-initiated write whose *completion* is asynchronous. It is polled for the
same reason as sink 4 and needs nothing further.

---

## The verdict per key

Every key in `queryKeys` (`apps/pos/src/lib/queries.ts:34-63`), against the
sinks above.

| Key | Reachable by a non-webview writer | Covered? |
|---|---|---|
| `orders` | 1, 2a, 2b, 2c, 5 | **YES** — event from anywhere (sinks 1, 2c), plus a 15s fallback poll for 2a, 2b and 5, which no event reaches |
| `order(id)` | 2a, 2b, 2c | **NO** — the single-order key is not invalidated by the listener. Reached only by the order list's own refetch |
| `kots(id)` | 1, 2c | **YES** — every `kots` key is invalidated by predicate, from anywhere |
| `tables` | 2a (opens a table session) | **NO** |
| `menuItems`, `menuCategories`, `menuItemVariants`, `stations`, `discountDefinitions`, `outletIdentity` | 3 | **NO** |
| `blockedOutboxRows`, `unroutableOutboxRows`, `persistentlyFailingOutboxRows` | 4 | **YES** — 15s poll |
| `blockedReplays` | 4 | **NO** — its three siblings are polled and this one is not. Recorded here as polled before the guard checked it |
| `unacceptedAggregatorOrders` | 5 | **NO** — demo step 6's path |
| `purchaseOrderReceiptProgress(id)` | 3 (`purchase_order` travels on `GET /sync/config` since 0.6.0) | **NO** |
| `failedPrintJobs`, `currentStock` | not sinks | **YES** — 5s and 15s polls for the till's own async work |
| `grnGaps`, `invoices(id)`, `payments(id)`, `cashShift(id)`, `stockDeductionGaps`, `kotStatusTransitions`, `stockCount*` | none | **N/A** — every writer is a till-side mutation that invalidates at its call site |

---

## What the fix should be, and what it should not be

**Not a blanket `refetchInterval` on everything.** Sink 3 fires at the config
loop's interval and sink 2 fires when a human taps a phone; polling every key
would put a floor under the till's idle work to cover two events an hour.

The shape the repository already chose, and the one to extend:

1. **Move the `holler://kitchen-changed` listener up** from `KotsPanel` to a
   component that is always mounted, so sinks 1 and 2c reach the orders list
   whether or not a panel is open. The listener already invalidates both keys;
   nothing about it needs redesigning.
2. **Emit on the captain's own writes** (2a, 2b), which is the only sink with
   no channel at all. The POS process knows exactly when it wrote — an event is
   exact where a poll is eventually-consistent, and this is the same mechanism
   D14 already proved.
3. **Then decide about sink 3**, which is genuinely a polling case: the config
   apply is a Rust loop with no natural UI event and a menu edit is not
   time-critical. A long interval on the six config keys is defensible where a
   5s poll is not.

**The `refetchInterval: 5000` on `useOrdersQuery`** currently sitting on branch
`m7-b2-stale-screens` is a stopgap that covers sinks 1, 2a, 2b and 2c for one
key at the cost of a permanent 5s tick. It is the right shape only if 1 and 2
are not done; it should not land alongside them.


---

## The guard that holds this document to the code

`scripts/check-query-key-freshness.mjs` (B2-T3), wired into `.githooks/pre-push`
and the `contracts` CI job. Every key in `queryKeys` must carry a ruling —
`poll`, `event`, `uncovered` with the track that owns it, or `exempt` with a
reason. A key added without one fails the build.

**What it was watched doing.** Three planted violations, each caught: a new
unruled key, a `poll` ruling whose `refetchInterval` was removed, and an
`event` ruling whose invalidation was removed. And, before it was trusted,
what it LETS THROUGH:

- **A `refetchInterval` of one hour passes as a poll.** It checks for the
  property, not for a sane value.
- ~~A key ruled `event` passes even when its listener is mounted somewhere it
  will not be.~~ **CLOSED 2026-09-18.** This was the live defect, so the guard
  now checks that `KitchenChangedListener` is mounted in `App.tsx` — watched
  RED with the mount removed. It is still a NAME check, not a React check: a
  listener mounted inside a conditionally-rendered `App` subtree would pass.
- **A new SINK with no new key passes.** The guard is key-centric; if a sixth
  writer starts writing rows behind an already-ruled key, nothing fires.

Those three are the honest boundary of it. The first check — refuse an unruled
key — is the one with teeth, and it is the one that found sink 5.


---

## What changed on 2026-09-18, and what did not

**Done:** the listener moved out of `KotsPanel` into `KitchenChangedListener`,
mounted in `App`; it invalidates `orders` and every `kots` key; `useOrdersQuery`
carries a **15s** fallback poll (not 5s — the event carries the cases a human
watches for, so the poll is a backstop, and 15s matches the outbox queries);
the guard gained a mount-point check and an `alsoPolled` rule.

**Not done, and deliberately named rather than left implicit:**

- **Sinks 2a and 2b still have no event.** A captain order that has only been
  created or added to reaches the screen by poll alone. `captain.rs` contains
  no `emit` call; adding one is the exact fix.
- **Sink 3 (config apply) and sink 5 (aggregator pull) are still uncovered** —
  11 keys ruled so. Both are Rust loops with no natural UI event; a long
  interval is defensible where a 5s poll is not.
- **`order(orderId)` is not invalidated by the listener.** Only the list key
  is.
- **NONE OF THIS IS VERIFIED IN THE TAURI RELEASE WINDOW.** 264 POS tests,
  `tsc`, eslint and `pnpm build` pass either way — they cannot see a Tauri
  event listener's mount point any more than they could see the defect it
  fixes. The acceptance row is `docs/visual-verify.md`.
