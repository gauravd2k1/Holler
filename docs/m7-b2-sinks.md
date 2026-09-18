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

> **The listener lives in `KotsPanel` (`OrderListScreen.tsx:280`), which is
> mounted only while an order's Kitchen panel is expanded.** Collapsed — which
> is the normal state, and the only state during the demo failure — nothing in
> the process is listening, so the event is emitted into nothing and the orders
> list is never invalidated.

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

Four writers reach the edge database inside the POS process without a Tauri
mutation running in the webview. Nothing else does: the till's own commands all
invalidate through `queryClient` at their call sites.

### 1. The LAN server — `set_kot_status`

| | |
|---|---|
| **Entry** | `edge/device/src/server.rs:345`, the only inbound LAN command (`edge/device/src/contract.rs:165`) |
| **Writes** | `kot.status`, `kot_status_history` |
| **Announces itself?** | **YES** — writes through the hub, which broadcasts `KotUpserted`, which `lib.rs:86` forwards as `holler://kitchen-changed` |
| **Keys that must react** | `kots(orderId)`, `orders` |
| **Status** | **Channel exists; listener is mounted in the wrong place.** Covered only while a Kitchen panel is expanded |

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

**Status: UNCOVERED.** 2c is covered by accident, through the KOT channel,
and only while a panel is expanded. 2a and 2b have no channel at all.

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
| **Status** | **COVERED, by polling.** All four are on a 15s `refetchInterval` (`queries.ts:129,151,162,217`), written for exactly this reason — "a condition someone must act on has to reach the screen without anyone navigating to it" |

### Not a sink, recorded so nobody re-derives it

**The print spool** (`failedPrintJobs`, `queries.ts:118`, 5s poll) is a
webview-initiated write whose *completion* is asynchronous. It is polled for the
same reason as sink 4 and needs nothing further.

---

## The verdict per key

Every key in `queryKeys` (`apps/pos/src/lib/queries.ts:34-59`), against the
sinks above.

| Key | Reachable by a non-webview writer | Covered? |
|---|---|---|
| `orders` | 1, 2a, 2b, 2c | **NO** — only via the `KotsPanel` listener, mounted only when expanded |
| `order(id)` | 2a, 2b, 2c | **NO** |
| `kots(id)` | 1, 2c | **PARTIAL** — live while the panel is expanded, which is when it is rendered. Arguably sufficient; stated rather than assumed |
| `tables` | 2a (opens a table session) | **NO** |
| `menuItems`, `menuCategories`, `menuItemVariants`, `stations`, `discountDefinitions`, `outletIdentity` | 3 | **NO** |
| `blockedOutboxRows`, `unroutableOutboxRows`, `persistentlyFailingOutboxRows`, `blockedReplays` | 4 | **YES** — 15s poll |
| `failedPrintJobs` | not a sink | **YES** — 5s poll |
| `invoices(id)`, `payments(id)`, `cashShift(id)`, `currentStock`, `stockDeductionGaps`, `kotStatusTransitions`, `stockCount*` | none | **N/A** — every writer is a till-side mutation that invalidates at its call site |

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
