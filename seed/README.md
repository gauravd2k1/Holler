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

## Onboarding a new restaurant

**Writing one file. Never editing code.**

1. `Copy-Item seed\outlet.example.toml seed\outlet.toml` — `seed/outlet.toml` is gitignored and per-installation.
2. Fill in the restaurant's name, legal entity, address, `state_code`, `pincode`, `gstin`, `fssai`, `invoice_prefix`, `invoice_footer_text`, `timezone` and `day_start_time`.
3. Optionally set `upi_vpa` / `upi_payee_name` (absent means no QR at all, on screen or on paper) and `logo_path` (absent changes no layout).
4. `.\scripts\demo-up.ps1 -Fresh` — pass `-OutletFile <path>` only for a file somewhere other than `seed/outlet.toml`.

Both seeders read that file. **There is no default and no fallback to the
example**: a missing file stops the run rather than seeding a placeholder
restaurant onto someone's machine. Every field is validated at load and a
failure names the field — GSTIN shape and its agreement with `state_code`,
six-digit pincode, `[A-Z]{1,4}/` invoice prefix, ASCII-only on every printed
string (the ESC/POS stream has no codepage translation), UPI address shape,
IANA timezone, and a `logo_path` that resolves.

**`seed/demo-outlet.json` is emitted from `seed/outlet.example.toml`**, which
is what `scripts/check-seed-drift.mjs` and the test suite read — the real
`outlet.toml` cannot be committed and would make CI depend on whose machine
ran it. A bootstrap or reset re-emits the catalogue from the installation's
own file into a run-local temporary and points both seeders at that, so the
cloud and the edge are still fed by the same bytes. The catalogue records the
sha256 of the identity file it came from, and **the edge seeder refuses a
catalogue whose digest disagrees with the identity file it was handed** —
otherwise one restaurant's name lands on the `outlet`/`tenant`/`brand` rows and
another's on the GST invoice, each internally consistent, every screen
plausible, and the bill wrong.

**Once the admin "Outlet settings" screen exists (Tier 2,
`docs/pilot-readiness.md`), this file becomes the bootstrap default and stops
being the source of truth**: the cloud will own `outlet` and
`outlet_fiscal_profile`, delivered to the edge by the config pull.

## The shape: ONE emitter, ONE committed artefact, TWO readers

```
menu_imgs_gong/gong_menu.xlsx          seed/outlet.toml   (WHO the restaurant is;
   (the CLIENT'S card -- hand-corrected)    |              gitignored, per-installation;
                 |                          |              seed/outlet.example.toml is
                 |  scripts/menu-to-seed.py  |              the committed template)
                 v                          |
edge/database/src/bin/devseed/client_menu.rs|   (COMMITTED, generated)
       + seed/menu-manifest.json            |   (counts + checksum)
                 |                          |
                 +------------+-------------+
                              v
        edge/database/src/bin/devseed.rs --emit-json --outlet-file
                              |
                              v
        seed/demo-outlet.json        (COMMITTED, generated, never hand-edited;
                 |                    emitted from the EXAMPLE identity, and
                 |                    carrying its sha256 so a mismatch is
                 |                    refused rather than silently seeded)
        +--------+--------+
        |                 |
        v                 v
  edge devseed      backend/cmd/devseed
   (SQLite)             (Postgres)
```

## The menu comes from the client's workbook, not from this repository

`menu_imgs_gong/gong_menu.xlsx`, sheet `Menu`, is the authoring source for the
catalogue: 346 rows with `include = Y`, transcribed from the printed card.
`scripts/menu-to-seed.py` turns it into `devseed/client_menu.rs`. **Neither
the generated module nor `demo-outlet.json` is ever hand-edited** -- correct the
workbook and re-run:

```
python scripts/menu-to-seed.py
cd edge/database && cargo run --bin devseed -- --emit-json ../../seed/demo-outlet.json
```

Three things the generator decides, each recorded because a reader will
otherwise have to reverse-engineer it from the output:

- **The card prints an ABSOLUTE price per variant** (Laksa Veg 425, Chicken 485,
  Prawn 495) while the contract stores one `base_price_paise` plus a
  `price_delta_paise` per variant. The base is the CHEAPEST printed variant and
  every delta is that variant's printed price minus the base, so nothing rings
  up at a price the card does not print and no delta is negative.
- **A single-price item gets one `Regular` variant at delta 0.** A recipe binds
  to a `menu_item_variant_id` (ADR-018 §2.1) and `apps/captain` refuses to order
  an item with no variant at all.
- **The three `Add Chicken` / `Add Prawns` / `Add Mixed Meat` rows under Staple
  become MODIFIERS on the other Staple dishes, not items** -- the workbook's own
  Read me says so, and a priced supplement sold as a standalone line would ring
  up as a dish.

**`ALCOHOL_VAT_UNCONFIGURED` is a zero-rate profile, and that is a hole, not a
tax position.** The card taxes every bar line as VAT; `tax_rule.component` is
CHECKed to CGST/SGST/IGST/CESS under contracts 0.8.1, so VAT cannot be
expressed. Rather than print a real rate under a wrong label, the 141 bar items
carry CGST 0 / SGST 0: they are orderable and billable and **no wrong tax amount
is ever charged or printed**. The demo script does not bill a bar item. The VAT
component is filed in `docs/pilot-readiness.md` as a contracts item -- any bar
in India needs it.

**`veg_flag` and `description` are in the workbook and NOT in the seed.**
`menu_item` has no column for either under 0.8.1. Both are filed for the next
additive bump; FSSAI requires the veg marker on a menu, so this is a pilot
blocker rather than a nicety.

**The Rust seed structs are the authoring source; `demo-outlet.json` is
generated from them and committed; both seeders consume the JSON.** The Rust
seeder consumes its own emitted file rather than its in-memory structs, so the
edge and the cloud are fed by the same bytes and neither store can be fed a
shape the other never saw.

Hand-transcribing the catalogue into a JSON file by hand was the obvious
alternative and was rejected: the transcription step is exactly where the two
descriptions drift, which is the defect being fixed, one layer out.

**`seed/demo-outlet.json` is GENERATED. Never hand-edit it.** Edit the seed data
in `edge/database/src/bin/devseed.rs` (or, for the menu, the workbook), re-emit,
and commit both. `scripts/check-seed-drift.mjs` regenerates and fails the build
if the committed file differs, and `client_menu_matches_the_generated_manifest`
fails if the generated menu module was hand-edited or the workbook changed
without a regeneration.

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

**Edge only** (stays in `edge/database/src/bin/devseed.rs`): `invoice_series`,
`outlet_fiscal_profile`, `discount_definition`, `app_user` with its cached
Argon2id hashes, `sync_state`.

**`device` MOVED INTO THE SHARED CATALOGUE ON 2026-09-16 (D11), UNENROLLED AND
WITHOUT A CREDENTIAL.** The till minted its row straight into edge SQLite and
nothing ever registered it with the backend, so every till-authored order
replayed to a cloud with no row to join to: four orders on the live stack
resolve to nothing (scenario board S-SYNC-13) and the admin Orders screen
cannot name who took an order. `postgres/0008` says in as many words that a
device row may exist unenrolled — "an admin registered it ahead of install; it
simply cannot sync until it holds a credential" — and that is exactly what is
seeded. **`enrolled_at` stays NULL and no `device_credential` is written**: an
unenrolled row grants nothing, and the plaintext token is returned once at
enrollment and never read back. The WAITER device is deliberately absent, being
created by a real enrollment at `demo-up.ps1` step 7.

**MOVED INTO THE SHARED CATALOGUE ON 2026-09-16 (D10):** `restaurant_table`,
`station`, `printer`, `printer_role`, `station_printer` — and
`menu_item_station`, which needs no new field because the cloud seeder derives
it from each item's existing `station_code` instead of discarding it.

They were listed as edge-only above, and that was wrong in a way nothing could
see. **All six are CLOUD-OWNED CONFIG** (ADR-011, ADR-014) and
`GET /sync/config` ships all six, so the cloud was serving empty arrays for
families it is the authority for: the live stack measured
`restaurant_table=0, station=0, printer=0` (scenario board S-SYNC-11). An
outlet ran on a floor plan and a kitchen layout the cloud could not reproduce,
and because config apply **upserts without pruning**, a pull could neither add
them nor remove them. The same shape as the inventory-config gap already filed.

The description now lives in three functions in `devseed.rs`
(`seed_restaurant_tables`, `seed_stations`, `seed_printers` +
`seed_station_printers`) that BOTH the edge seeding path and
`build_shared_catalogue` read, so the two stores cannot fork.

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
schema_version            int         // 2
generated_by              string      // "edge/database/src/bin/devseed.rs --emit-json"
outlet_source_sha256      string      // sha256 of the seed/outlet.toml this was emitted from

tenant                    { id, name }
brand                     { id, tenant_id, name }
outlet                    { id, brand_id, name, timezone, day_start_time }
outlet_identity           { restaurant_name, legal_name, outlet_name,
                            address_line1, address_line2?, city, state_code,
                            state_name, pincode, gstin, fssai?,
                            invoice_prefix, invoice_footer_text }

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

- **`outlet_identity` is consumed by the EDGE only**, to build
  `outlet_fiscal_profile` and `invoice_series`. Postgres has both tables, but
  the cloud does not seed them (see "Scope" above), so the cloud reader decodes
  the block, validates it, and writes none of it — the `station_code`
  precedent. It is declared in `seedfile.go` rather than omitted because that
  decoder runs with `DisallowUnknownFields`: "decode it and choose not to write
  it" is a recorded decision, while "never hear about it" is the contracts
  0.5.9 defect. Tier 2's admin "Outlet settings" screen is where the cloud
  starts writing these.
- **`outlet_source_sha256` is the identity file's digest, and the edge seeder
  refuses a catalogue whose digest disagrees with the identity file it was
  handed.** Without it, an installation with its own `outlet.toml` that seeded
  from the committed catalogue would name one restaurant on the
  `outlet`/`tenant`/`brand` rows and another on the GST invoice — internally
  consistent on every screen, wrong on the bill.
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
