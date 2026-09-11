# `apps/captain` — the LAN JSON API

Demo build work item 0, approved reduced scope, **cut-off Monday 18:00 IST**.
This document is written **before any code**, so the frontend and the Rust
listener can be built in parallel from one agreed shape. Where this document and
the repository disagree, the repository wins — say so out loud rather than
coding around it.

## What this is, and what it is not

A waiter opens a page on their phone, picks a table, adds items, and sends.
The ticket reaches the KDS and the hub exactly as a till-authored one does.

It **is not** the M9 Flutter waiter app, and it **is not** ADR-025's customer
tab. It is a **staff** device. There is no bill screen, no payment, and no
modifiers unless they are free.

The demo step it exists for is **1a**: *an order placed from the waiter's phone
appears on the KDS and the hub*. The client's mental model is
**device-at-table → hub → kitchen**; without this, the demo shows him what he
already has. A build that satisfies every sentence below while missing that has
missed the item.

## Where it runs

**Inside the POS process**, beside the KDS LAN server that is already there.

`apps/pos/src-tauri/src/state.rs:231` already starts
`holler_edge_device::server::start` on `HOLLER_LAN_BIND_ADDR` (default
`0.0.0.0:9310`). That server is **WebSocket only, over a raw `TcpListener` +
`tungstenite`** — it has no HTTP layer and serves no files
(`edge/device/src/server.rs`). Its only inbound command is `set_kot_status`
(`edge/device/src/contract.rs:165`). **There is no order create or append on the
LAN surface today.**

So this is a **second listener**:

- `HOLLER_CAPTAIN_BIND_ADDR`, default `0.0.0.0:9320`. Plaintext HTTP.
- Serves the built `apps/captain/dist` at `/` and its assets.
- Serves the JSON API below under `/api/`.
- Never fatal to POS startup. If it cannot bind, log it and carry on — the same
  posture `start_lan_server` already takes, and for the same reason: Milestone
  1's acceptance is that a cashier can work fully offline, and a LAN feature
  failing to bind must not take the till down with it.

`tiny_http` is already in `apps/pos/src-tauri/Cargo.toml` as a **dev**-dependency
(line 58). Promote it to a real dependency rather than adding a second HTTP
stack.

Plaintext HTTP on a flat LAN is accepted **for the demo, on our own hotspot**.
The security gate review for a LAN-facing device stays filed for pilot.

## Authentication

Every `/api/` request carries:

```
Authorization: Bearer <device_token>
```

where `device_token` is the `"<credential_id>.<secret>"` a cloud enrolment
issued (`POST /devices/enroll`). A browser `fetch` can set that header, so none
of the first-frame-auth machinery the KDS needs applies here — that exists only
because a browser `WebSocket` cannot set headers on the handshake at all.

Verification is **local-first, offline-capable**, against
`device_credential_cache`, exactly as the KDS path does:

```rust
CachedCredentialVerifier::new(db.clone(), "WAITER", None)
```

`WAITER` is **already a valid `device.kind`** in the frozen contract
(`packages/contracts/src/types/identity.ts:203`,
`backend/internal/outlet/device.go:18`). **No contract change is needed.** The
constructor is already parameterised; the POS passes `"KDS"` today
(`state.rs:735`). Pass no cloud fallback, for the reason recorded there: needing
a cloud URL to start would make the POS's own startup depend on the uplink.

### The identity trap — read this before implementing

`DeviceTokenVerifier::verify()` returns `Result<()>`. It answers *"does this
token belong to some device enrolled at this outlet"* — **not which one**. Its
own doc comment says so.

This API needs the specific `device_id`, so it must resolve the
`device_credential_cache` row (`credential_id`, `device_id`, `tenant_id`,
`outlet_id`, `device_kind`, `revoked_at`, `expires_at`), not merely verify.

**The `device_id` comes from the resolved credential and NEVER from the request
body or a query parameter.** This is the rule `edge/device/src/server.rs` already
applies to `set_kot_status`: the connection's established identity wins, never
whatever the payload claims (ADR-014 §6, ADR-015). A captain endpoint that
accepts a `device_id` field has re-opened it.

A revoked or expired credential is `401`. A credential that has never synced to
this node is `401` — absence is unknown, never allow.

## Order attribution — the known trap

`create_order_impl` and `add_order_item_impl`
(`apps/pos/src-tauri/src/commands/orders.rs`) take their `device_id` from
`state.device_id`, which is **the till**. Called as they stand, every order a
waiter places is recorded as authored by the till, on every screen, and **it
reads correctly in review**.

They therefore gain a `device_id` override, taken from the resolved credential.
That changes `pub` signatures across crates that do not share a cargo workspace,
so **`make check-seams` is mandatory** after.

`order.source` stays **`"POS"`**. Not `TABLE_TAB` — that is ADR-025's customer
tab, landing proposed for M8, and `scripts/check-order-source-drift.mjs` fails
the build if anything under `edge/` or `apps/pos/src-tauri/src` emits it.

## The endpoints

Five. Nothing else. Every response is JSON; every error is
`{ "code": "...", "message": "..." }` with the codes the POS already uses.

### `GET /api/session`

Validates the pasted token and tells the page who it is. This is what the pair
screen calls.

```jsonc
200 {
  "device_id": "...",
  "outlet_id": "...",
  "outlet_name": "...",
  "device_kind": "WAITER"
}
401 { "code": "UNAUTHORIZED", "message": "..." }
```

### `GET /api/tables`

```jsonc
200 {
  "tables": [
    {
      "id": "...",
      "name": "...",
      "seats": 4,
      "open_session_id": "...",   // null when the table is free
      "open_order_id": "..."      // null when nothing is running on it
    }
  ]
}
```

Backed by `list_tables_impl` and `get_open_table_session_impl`
(`commands/tables.rs`).

### `GET /api/menu`

One call, not four — a phone on a restaurant hotspot should fetch the catalogue
once.

```jsonc
200 {
  "categories": [ { "id": "...", "name": "...", "sort_order": 0 } ],
  "items": [
    {
      "id": "...", "category_id": "...", "name": "...",
      "base_price_paise": 0,
      "is_available": true,
      "variants": [
        { "id": "...", "name": "...", "price_delta_paise": 0, "is_default": true }
      ],
      "modifiers": [
        { "id": "...", "group_name": "...", "option_name": "...",
          "price_delta_paise": 0, "min_selection": 0, "max_selection": 1 }
      ]
    }
  ]
}
```

Backed by `list_menu_items_impl`, `list_menu_categories_impl`,
`list_menu_item_variants_impl`, `list_menu_item_modifiers_impl`
(`commands/menu.rs`).

**`is_available` is served as it is stored and the page must respect it.** It is
the field a till stock-out snoozes; a captain page that lets a waiter order a
snoozed item is showing the kitchen something it has just said it cannot make.

**Only free modifiers (`price_delta_paise == 0`) are selectable in the reduced
scope.** Serve them all — the filter is the page's, and stating it here keeps
the API honest for when the restriction lifts.

**A variant is mandatory on every line.** The till hardcoded `variantId: null`
for a whole milestone, which meant no sale it took ever wrote a stock ledger
row, and separately satisfied `order_item_variant_id_fkey` trivially so nothing
noticed. Pick `is_default` where present; if an item has no variant at all, that
is a seed defect to report, not a null to send.

### `POST /api/orders`

Creates a `DRAFT` order on a table and adds its first lines.

```jsonc
// request
{
  "order_type": "DINE_IN",
  "table_id": "...",
  "items": [
    {
      "menu_item_id": "...",
      "variant_id": "...",          // required, never null
      "quantity": 1,
      "unit_price_paise": 0,
      "notes": null,
      "modifiers": [
        { "modifier_id": "...", "group_name": "...",
          "option_name": "...", "price_delta_paise": 0 }
      ]
    }
  ]
}

201 <CanonicalOrder>              // apps/pos/src-tauri/src/dto.rs:312, verbatim
```

`CanonicalOrder` is returned as-is. Inventing a second order shape for the phone
is how the two drift.

### `POST /api/orders/{orderId}/items`

Appends to an order that already exists — **including one the kitchen already
has**. This is the whole point of "append-only": a waiter walks back to the
table and adds two more.

Request body is one `items[]` element from above. Response is `201
<CanonicalOrder>`.

`add_order_item_impl` is **already legal through DRAFT / CONFIRMED /
SENT_TO_KITCHEN / PREPARING** (`#132-A`, `commands/orders.rs`). Append-after-send
is solved; do not re-implement it and do not narrow it.

### `POST /api/orders/{orderId}/send`

Confirms the order and cuts the KOTs.

```jsonc
200 {
  "order": <CanonicalOrder>,
  "kots": [ { "id": "...", "station": "...", "sequence": 0, "status": "NEW" } ]
}
```

Backed by `confirm_order_impl` then `send_order_to_kitchen_impl`
(`commands/kitchen.rs:232`). That second call is what puts the ticket on the
hub, and the hub is what the KDS is watching — **it is the half of step 1a the
client is actually looking at.** An implementation that creates the order and
stops has built nothing the demo can show.

Sending an order with no lines is `400`, not an empty KOT set.

## Out of scope, explicitly

No bill screen. No payment. No paid modifiers. No order cancel, no line removal,
no quantity edit, no table transfer, no customer tab, no `table_device`
principal, no `TABLE_TAB` source, no cloud calls of any kind from the phone.

The page talks to the till and nothing else. That is not a simplification for
the demo — orders are edge-authoritative and a table-side device is a LAN client
of the till, never a cloud client.

## The pass condition

A WAITER device enrolled, a real phone on our hotspot places an order that
reaches **the KDS and the hub**, **three times from a clean reset**. Anything
less by Monday 18:00 IST and captain is cut, step 1a comes out of the demo
script, and no further work happens on it.

Note what "three times from a clean reset" excludes: a run where the order
reached the KDS because the POS had been restarted between attempts proves the
startup path, not this one. Keep the POS process up across all three.
