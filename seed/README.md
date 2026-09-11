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
arrays in a stable order. One top-level object:

```jsonc
{
  "schema_version": 1,
  "generated_by": "edge/database/src/bin/devseed.rs --emit-json",

  "tenant":  { "id": "...", "name": "..." },
  "brand":   { "id": "...", "tenant_id": "...", "name": "..." },
  "outlet":  {
    "id": "...", "brand_id": "...", "name": "...",
    "timezone": "Asia/Kolkata", "day_start_time": "05:00"
  },

  "tax_profiles": [
    { "id": "...", "outlet_id": "...", "name": "...", "is_default": false }
  ],
  "compliance_versions": [ { "id": "...", "...": "..." } ],
  "tax_rules": [ { "id": "...", "tax_profile_id": "...", "compliance_version_id": "...", "...": "..." } ],

  "menu_categories": [
    { "id": "...", "outlet_id": "...", "name": "...", "sort_order": 0 }
  ],
  "menu_items": [
    {
      "id": "...", "outlet_id": "...", "category_id": "...",
      "name": "...", "base_price_paise": 0,
      "is_available": true,
      "tax_profile_id": "..." ,          // null means fall back to the outlet default
      "hsn_sac": "...",                  // NEVER null and NEVER blank — an invoice
                                         // cannot issue without it (contracts 0.4.5)
      "station_code": "..."              // consumed by the EDGE only, to build
                                         // menu_item_station; there is no such
                                         // table in Postgres
    }
  ],
  "menu_item_variants": [
    { "id": "...", "menu_item_id": "...", "name": "...",
      "price_delta_paise": 0, "is_default": true }
  ],
  "menu_item_modifiers": [
    { "id": "...", "menu_item_id": "...", "group_name": "...",
      "option_name": "...", "price_delta_paise": 0,
      "min_selection": 0, "max_selection": 1 }
  ],

  "inventory_items": [
    { "id": "...", "outlet_id": "...", "sku": "...", "name": "...",
      "category": "...", "dimension": "MASS",
      "reorder_level_micro": 5000000 }   // null is legal and deliberate
  ],
  "item_unit_conversions": [
    { "id": "...", "inventory_item_id": "...", "...": "..." }
  ],
  "recipes": [
    { "id": "...", "menu_item_variant_id": "...",
      "output_dimension": "MASS", "output_quantity_micro": 1000000 }
  ],
  "recipe_ingredients": [
    { "id": "...", "recipe_id": "...",
      "inventory_item_id": "...",        // exactly one of these two is set
      "sub_recipe_id": null,
      "quantity_micro": 220000000,
      "quantity_dimension": "MASS"       // THE UNIT THE AUTHOR CHOSE. See below
    }
  ],
  "modifier_ingredient_deltas": [
    { "id": "...", "menu_item_modifier_id": "...", "...": "..." }
  ],

  "suppliers": [ { "id": "...", "outlet_id": "...", "name": "...", "...": "..." } ],
  "supplier_items": [
    { "id": "...", "supplier_id": "...", "inventory_item_id": "...",
      "pack_size_micro": 1000000, "...": "..." }
  ],

  "goods_receipt": { "...": "..." },     // see the exception above
  "opening_stock": [
    { "inventory_item_id": "...", "quantity_micro": 0, "...": "..." }
  ]
}
```

`"...": "..."` above marks a group whose remaining columns are taken verbatim
from the frozen schema — the emitter writes every column the store requires, and
the reader writes every column it reads. Neither side invents one.

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
   - zero rows in `sync_replay_block`
   - zero rows in `stock_deduction_gap`
   - zero blocked rows in `local_outbox`
   - the POS sync banner is empty

`apps/pos/.env.dev` carries the edge encryption key and is deny-ruled to agents,
so **the operator runs this script.** It must therefore be non-interactive,
report each assertion by name, and exit non-zero on the first failure — an
assertion whose output has to be read by a human to know whether it passed is
not an assertion.
