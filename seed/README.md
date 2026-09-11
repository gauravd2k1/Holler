# `seed/` — the single demo seed source

Demo build work items 1 and 2 (`docs/demo-kickoff.md`). **One seed source loads
an identical menu, variants, recipes, inventory and supplier into BOTH the cloud
and the edge.**

## Why this directory exists

Before it, there were two independent seeders describing the same outlet by
hand-mirrored constants:

- `edge/database/src/bin/devseed.rs` — ~39 menu items, variants, modifiers, five
  stations, three tax profiles, a full larder, real recipes with sub-recipes.
- `backend/cmd/devseed/main.go` — **two** menu items (chai, thali), one variant,
  three inventory items, no recipes.

The comment at the top of each says *"MUST match the constants in"* the other.
Nothing enforced it and the two diverged by a factor of twenty. That divergence
is not cosmetic: it is the direct cause of the thirteen permanently-blocked rows
in the live edge outbox, each `missing_reference (HTTP 422)` on
`order_item_variant_id_fkey` — the till sent a real variant to a cloud that had
one variant row in total.

**A claim in a comment is worth nothing unless something fails when it goes
false.** This directory makes the two seeders read one file, and a drift check
makes a hand-edit of the derived half fail the build.

## The shape: ONE emitter, ONE committed artefact, TWO readers

```
edge/database/src/bin/devseed.rs --emit-json
                 |
                 v
        seed/demo-outlet.json        (COMMITTED, generated, never hand-edited)
                 |
        +--------+--------+
        |                 |
        v                 v
  edge devseed      backend/cmd/devseed
   (SQLite)             (Postgres)
```

**The Rust seed structs are the authoring source; `demo-outlet.json` is
generated from them and committed; both seeders consume the JSON.** The Rust
seeder consumes its own emitted file rather than its in-memory structs, so the
edge and the cloud are fed by the same bytes and neither store can be fed a
shape the other never saw.

Hand-transcribing the catalogue into a JSON file by hand was the obvious
alternative and was rejected: the transcription step is exactly where the two
descriptions drift, which is the defect being fixed, one layer out.

**`seed/demo-outlet.json` is GENERATED. Never hand-edit it.** Edit the seed data
in `edge/database/src/bin/devseed.rs`, re-emit, and commit both.
`scripts/check-seed-drift.mjs` regenerates and fails the build if the committed
file differs.

## Scope: what is shared and what is not

Only rows that genuinely exist in **both** stores belong in the shared file.
A row that exists in one store only stays in that store's own seeder, and the
reason is recorded there.

**Shared (in `demo-outlet.json`, written to both stores):**
`tenant`, `brand`, `outlet`, `menu_category`, `menu_item`, `menu_item_variant`,
`menu_item_modifier`, `tax_profile`, `tax_rule`, `compliance_version`,
`inventory_item`, `item_unit_conversion`, `recipe`, `recipe_ingredient`,
`modifier_ingredient_delta`, `supplier`, `supplier_item`.

**Cloud only** (stays in `backend/cmd/devseed/main.go`): `role`,
`role_permission`, `app_user`, `user_role`. There is **no `role` table in SQLite
at all** — the edge flattens permissions into `app_user.permissions_json` — so
`po_approval_limit_paise` is Postgres-only by necessity and by design
(contracts 0.6.0).

**Edge only** (stays in `edge/database/src/bin/devseed.rs`): `device`,
`station`, `menu_item_station`, `printer`, `station_printer`, `printer_role`,
`restaurant_table`, `invoice_series`, `outlet_fiscal_profile`,
`discount_definition`, `app_user` with its cached Argon2id hashes, `sync_state`.

### The goods receipt is a deliberate exception — read this before touching it

The demo story's step 5 shows **the received GRN in the admin console**, and the
admin console reads the **cloud**. A `goods_receipt_note` is edge-authoritative
and reaches the cloud by replay — but **`edge/sync/src/route.rs` maps only
`order` and `table_session`** (carried gap A7), so a GRN seeded at the edge can
never reach the cloud, at all, ever.

The GRN and its stock ledger rows are therefore seeded into **both** stores
directly, from the same shared description, and that is a demo accommodation
rather than a model of how a receipt travels in production. Two consequences a
builder must not tidy away:

- The cloud's copy of a GRN is a **REPLICA** and every surface must label it as
  one (contracts 0.7.0). Seeding it directly does not change that.
- **Do not read this as A7 being fixed or as a reason to fix it.** A7 is
  pilot-only work and is on the demo EXCLUDES list.

## Identity: ids are deterministic, never freshly minted

Every id in the emitted file is produced by the pure `seq -> id` functions
already in `edge/database/src/bin/devseed.rs`
(`menu_item_id`, `inventory_item_id`, `recipe_id`, …), each in its own disjoint
UUID range. A re-emit with unchanged inputs produces byte-identical output.
This is what makes the drift check meaningful and what lets the two stores agree
without a handshake.

Fixed ids that predate this file (`TENANT_ID`, `OUTLET_ID`, `DEVICE_ID`,
`CASHIER_ID`, `ITEM_CHAI_ID`, `ITEM_THALI_ID`, `STATION_ID`/`MAIN_KITCHEN`, …)
**keep their exact current values**. `tests/e2e-scenario/harness` pins several
of them — including chai's exact 4000-paise price, its single-station routing
and its `tax_profile_id = None` fallback. Renaming, re-pricing or re-routing any
of them silently breaks that harness.

## File format

`seed/demo-outlet.json`, UTF-8, LF, two-space indent, keys in the order below,
arrays in a stable order. One top-level object.

**`backend/cmd/devseed/seedfile.go` is the executable statement of this format.**
It decodes with `DisallowUnknownFields`, so a field the emitter invents and this
document does not list is a **loud decode failure**, not a silent drop. That is
deliberate: `json.Unmarshal`'s leniency already cost this repo a column once
(contracts 0.5.9 — the edge wrote `source_stock_count_id`, the cloud had never
heard of it, and every Postgres row was NULL in silence).

Every field below is required unless marked nullable. Nullable means the key is
present with a JSON `null`, never absent.

```
schema_version            int         // 1
generated_by              string      // "edge/database/src/bin/devseed.rs --emit-json"

tenant                    { id, name }
brand                     { id, tenant_id, name }
outlet                    { id, brand_id, name, timezone, day_start_time }

tax_profiles[]            { id, outlet_id, code, name, pricing_mode, is_default, is_active }
compliance_versions[]     { id, outlet_id, label, effective_from, notes? }
tax_rules[]               { id, tax_profile_id, compliance_version_id, component,
                            rate_bps, effective_from, effective_to? }

menu_categories[]         { id, outlet_id, name, sort_order }
menu_items[]              { id, outlet_id, category_id, name, base_price_paise,
                            is_available, tax_profile_id?, hsn_sac, station_code }
menu_item_variants[]      { id, menu_item_id, name, price_delta_paise, is_default }
menu_item_modifiers[]     { id, menu_item_id, group_name, option_name,
                            price_delta_paise, min_selection, max_selection }

inventory_items[]         { id, outlet_id, sku, name, category?, dimension,
                            reorder_level_micro? }
item_unit_conversions[]   { id, inventory_item_id, pack_unit_label,
                            source_dimension, numerator, denominator }
recipes[]                 { id, menu_item_variant_id, name,
                            output_dimension, output_quantity_micro }
recipe_ingredients[]      { id, recipe_id, inventory_item_id?, sub_recipe_id?,
                            quantity_micro, quantity_dimension }
modifier_ingredient_deltas[] { id, menu_item_modifier_id, inventory_item_id,
                            quantity_micro }

suppliers[]               { id, outlet_id, code, name, gstin?, phone?, email?,
                            address?, payment_terms_days, is_active }
supplier_items[]          { id, supplier_id, inventory_item_id, purchase_unit,
                            pack_size_micro, quantity_dimension,
                            last_price_paise?, is_preferred }

goods_receipt             { id, outlet_id, purchase_order_id?, supplier_id?,
                            grn_number, delivery_note_ref?, received_at,
                            received_by_user_id, business_date, notes?,
                            lines[], ledger_entries[] }
  lines[]                 { id, inventory_item_id, line_number,
                            purchase_order_line_id?, entered_purchase_unit,
                            entered_quantity_micro, quantity_dimension,
                            base_quantity_micro, pack_size_micro_applied,
                            unit_cost_paise, line_total_paise,
                            batch_code?, expiry_date? }

opening_stock[]           <stock_ledger_entry>
goods_receipt.ledger_entries[] <stock_ledger_entry>

stock_ledger_entry        { id, outlet_id?, entry_seq?, inventory_item_id,
                            inventory_item_name, dimension, entry_type, origin,
                            quantity_micro, recipe_id?, recipe_version?,
                            recipe_name?, reason_code?, note?, occurred_at,
                            business_date, created_by_user_id?,
                            modifier_delta_id?, modifier_name?,
                            modifier_delta_version?, unit_cost_paise?,
                            line_total_paise?, source_grn_id?,
                            source_purchase_return_id?,
                            source_stock_transfer_out_id?,
                            source_stock_count_id? }
```

Notes that are load-bearing, not stylistic:

- **`menu_item.station_code` is consumed by the EDGE only**, to build
  `menu_item_station`. Postgres has no such table; the cloud reader decodes it
  and discards it deliberately.
- **`menu_item.tax_profile_id` nullable means "fall back to the outlet
  default"**, and only the legacy chai/thali pair relies on that path.
- **`menu_item.hsn_sac` is never null and never blank.** An invoice cannot issue
  without it (contracts 0.4.5). Both readers fail loudly on a blank rather than
  inserting one.
- **`recipe_ingredient` sets exactly one of `inventory_item_id` /
  `sub_recipe_id`**, never both, never neither.
- **`stock_ledger_entry.entry_seq` nullable means "assign from the store's
  current high-water mark".** It is 1-based and cursors default to 0 meaning
  "nothing acked" — a 0-based sequence skips every outlet's first entry,
  permanently and silently (contracts 0.5.8).
- **`line_total_paise` is set on receipt-origin rows and NULL on every other
  origin** (contracts 0.6.3). The CHECK is directional: a total never appears
  without its rate, a rate may stand alone. Opening-stock rows are not receipts.
- **`source_stock_count_id` and the other three provenance columns travel.**
  Each must be populated on at least one row of the fixture, or a fidelity test
  passes on absent data — 0.5.9's lesson, where a null round-tripped through a
  nonexistent field perfectly and the storage compare stayed green.

### Three rules that bind whoever touches the emitter or either reader

1. **`quantity_dimension` is the unit the author chose, NEVER derived from the
   referent** (contracts 0.5.2). If the emitter fills it from
   `inventory_item.dimension`, the guard's comparison becomes `x == x`, it can
   never fire, and **it will look correct in review**. Emit the authored value.
2. **Quantities are integer micro-units and the scale is not uniform.** `g`,
   `piece` and `l` scale ×1\_000\_000; `mg` and `ml` scale ×1\_000, because they
   are already 1/1000 of their dimension's canonical unit. 220 g is
   `220_000_000`; 180 ml is `180_000` — **not** `180_000_000`. Neither value
   fails a CHECK; getting it wrong silently mis-deducts every volume ingredient
   by 1000×.
3. **Money is integer paise, end to end.** Never a float, in the JSON or in
   either reader.

## The one command

`scripts/demo-reset.ps1` resets **cloud + edge** to this state, and it is the
only supported way to get there. It must, in order:

1. Verify the backend by **PID**, never by the port answering — the old process
   answers identically, so a health check passing is consistent with nothing
   having restarted.
2. Drop and re-apply the Postgres schema, then run `backend/cmd/devseed`.
3. Delete the edge data directory (`edge.db.enc` **and** the plaintext
   `edge.db` that every run leaves beside it, gap A6), then run the edge
   devseed. sqlite migration 0035 applies here, at this clean bootstrap.
4. **Assert, and fail loudly on any of them:**
   - zero rows in `sync_replay_block` (ranged-stream give-ups)
   - zero rows in `stock_deduction_gap` — **not `grn_gap`**. The demo seed
     legitimately produces one `NO_PURCHASE_ORDER` row in `grn_gap`, because a
     GRN never blocks on a PO (ADR-019). Asserting zero on that table makes a
     correct seed look broken.
   - zero rows in `sync_outbox_block` with `blocked_at` set. **This is what
     "zero blocked rows in the outbox" actually means** — `local_outbox` carries
     no blocked flag of its own (contracts 0.6.4), so an assertion written
     against `local_outbox` has nothing to read.
   - the POS sync banner is empty. **There is no way to assert this from a
     database**, so the check queries the closest proxy — `sync_outbox_block`
     with `blocked_at IS NULL AND attempts >= 3`, which is what the banner reads
     — and both the tool and the script must say, in their output, that it is a
     proxy and not an observation of `SyncBlockedBanner.tsx`. A check that
     claims to have looked at a screen it never opened is worse than one that
     admits its limit.

`apps/pos/.env.dev` carries the edge encryption key and is deny-ruled to agents,
so **the operator runs this script.** It must therefore be non-interactive,
report each assertion by name, and exit non-zero on the first failure — an
assertion whose output has to be read by a human to know whether it passed is
not an assertion.
