# Demo build — status, evidence and what is gated

**Written 2026-09-11.** The brief is `docs/demo-kickoff.md`. This file is the
record of what was built, **how it was verified**, and what still depends on a
human. The chat is not the record — a verdict that exists only in a session
transcript is erased by a restart, and what replaces it is a reconstruction from
git history stated with the confidence of a read.

Read `CLAUDE.md`'s `## Current milestone:` block for scope and EXCLUDES.

---

## Scenario board triage against the six-step path (2026-09-12)

Every non-passing row, with a verdict. **Only rows on the six-step path get
fixed**; the rest are listed and left, per the operator's Wednesday scope.

| Row | Step | Verdict | Why |
|---|---|---|---|
| **S-CAP-06** | 1a | **FIXED** (`4f98b06`) | 12 items had no variant, so the captain refused to cart them. Every item now carries one; guarded by a test. |
| **S-SYNC-08** | 1a | **FIXED** (`4f98b06` + `f7d9c8f`) | Same root: cloud variant coverage now matches the edge row for row, proven on a clean seed. |
| **S-SYNC-10** | 6 | **FIXED at the cause** (`4f98b06`) | Orders with a total and no lines were the variant-FK refusal seen from the cloud. Re-verify after the reset; it is a consequence, not its own defect. |
| **S-ADM-08** | 4 | **FIXED** (`7e94837`) | The Orders screen exists. The route had existed since M1 and was only missing from the spec. |
| **S-BE-09** | pre | **FIXED** (`c9195f5`) | Login budget configurable; the demo build runs 50 attempts. The identical-401 behaviour is unchanged by design. |
| **S-CAP-20** | 1a | **OPERATOR ACTION, not a code fix** | The T29 fix has no backfill. The demo WAITER must be re-ENROLLED (re-pairing does not help). On the day-of checklist. |
| **S-ADM-09** | 5 | **CUT** — operator ruling 2026-09-12 | No cloud read route exists for inventory; a variance screen needs new OpenAPI paths. Step 5 is Orders + GRN. |
| **S-SYNC-04** | 4/5/6 | **CARRIED (A7), do not fix** | Only `order` and `table_session` are routed. The demo story depends on `order` replaying, which it does. Pilot work. |
| **S-SYNC-13** | 1a/6 | **CARRIED, do not fix** | The till's devseed device has no cloud row, so till-authored orders replay unattributable. Cosmetic in the back office; nothing on the six steps reads it. |
| **S-SYNC-11** | 1a/2 | **NOT A DEFECT** — operator ruling | `restaurant_table`/`station`/`printer` stay edge-only. No admin screen renders them; removed from the brief. |
| **S-API-02** | 2 | **OFF-PATH, leave** | `PATCH /menu/items` answers 400 where the contract says 422. The load-bearing half — that it refuses rather than silently ignores — is correct. Nothing in the demo touches it. |
| **S-SYNC-06** | 6 | NOT TESTABLE | Unchanged. |
| **S-CUI-06** | 1a | NOT TESTABLE | Unchanged. |

**Nothing on the six-step path is left failing in code.** What remains between
here and Wednesday is the operator's re-enrolment (S-CAP-20), a post-reset
re-verification of S-SYNC-10, and the polish/perf work below.

## Work items

| # | Item | State | Evidence |
|---|---|---|---|
| **0** | `apps/captain` | **BUILT, NOT PASSED** — the cut-off condition needs a human | below |
| **0b** | UPI QR, screen + receipt | **DONE** | below |
| **1** | Seed parity | **BUILT; cloud half compared row for row, edge half is a CI test; the operator's reset is the last step** | below |
| **2** | Demo seed content + reset command | **BUILT; never run with `-Force`** | below |
| **3** | Rendered receipt | **DONE** | below |
| **4** | Sync banner legibility | **DONE, observed in Chromium** | below |
| **5** | Offline tick | **NOT STARTED** — conditional on observing sluggishness with the cloud down |  |
| **B** | Admin Orders screen (conditional, ruled 2026-09-12) | **BUILT and observed in a browser**; the route already existed and was undocumented | below |
| **6** | Presentability | **DONE for POS and admin; KDS unreachable** | below |
| **7** | `docs/demo-script.md` | **DAY-OF CHECKLIST WRITTEN; the six steps are NOT** — Tuesday | the file itself |
| **8** | Three timed rehearsals | **NOT STARTED** — Tuesday, needs item 2's reset to run first |  |

---

## What is gated on a person, not on code

Everything here is blocked on something no agent in this project can do.

1. **`apps/pos/.env.dev` carries the edge encryption key and is deny-ruled to
   agents.** So **the operator runs `scripts/demo-reset.ps1 -Force`**, and that
   first run is also the first end-to-end execution of the whole sequence — see
   item 2 below for exactly which steps have never executed.
2. **A WAITER device must be enrolled against the cloud** and its token pasted
   into a phone. Item 0's cut-off cannot be attempted before this.
3. **The POS Tauri window cannot be launched from a tool with redirected
   stdio** — it never appears. Nothing that requires the POS shell has been
   observed running by anyone.
4. **The POS icon is a 16×16 placeholder** from the original scaffold, 1086
   bytes. Needs real artwork; no builder can produce it.

## Decisions outstanding with the operator

- **Demo steps 4 and 5 name admin screens that do not exist.** `apps/admin` has
  three tabs — Menu and pricing, Suppliers, Goods receipts (`App.tsx`). There is
  **no orders screen and no stock variance screen**, and no cloud read route to
  build them on: `/orders` is POST-only ingest, and `/inventory/ledger-entries`,
  `/inventory/items`, `/inventory/counts` and `/inventory/recipes` are all
  POST-only. Only `/orders/{id}` has a GET and there is no list route. An admin
  orders list therefore needs **new OpenAPI paths — a contract change**, against
  a brief that needs none. Options put to the operator: **A** cut both from the
  story; **B** orders only (new `GET /orders` + Go handler + admin tab, ~1.5–2
  builder-days, contracts 0.9.0); **C** orders and stock (~3–4 days).
- **`inventory_item_name` is missing from `GoodsReceiptLineReadSchema` and
  `SupplierItemSchema`.** Both carry `inventory_item_id` only, unlike
  `stock_ledger_entry`/`stock_count_line`, which already denormalise the name
  for exactly this reason. Consequence: **the admin GRN screen cannot name a
  single ingredient on a receipt**, and demo step 5 shows that receipt. The
  presentability pass withheld the UUID rather than fabricate a name, which was
  correct. Folds naturally into option B if that is taken.

---

## Item 1 and 2 — seed parity

### The `[3c/4]` 404 was a missing DEVICE, not a missing route (2026-09-12)

The operator's post-reset bootstrap failed KDS enrolment with a 404, which
reads as an unserved route. It is not: `POST /devices/{deviceId}/credentials/rotate`
is registered at `backend/internal/outlet/device_http.go:29` and answers.

**What actually happened.** `dev-bootstrap.ps1` remembers
`(cloud, outlet, kind, name) -> device id` in
`%LOCALAPPDATA%\Holler\dev-bootstrap-state.json`, so it can ROTATE a
credential rather than enrol a second device — there is deliberately no device
LIST route, so that memory is the only way to answer "does this device already
exist?" against a remote cloud. `demo-reset.ps1` then drops the entire public
schema. **Measured after the operator's reset: 281 menu items, 0 devices, 0
credentials — and the state file still naming two device ids.** The bootstrap
rotated one of them and the backend correctly said 404.

**Proven, not inferred**, against the live backend on 8080: a rotate on the
exact stale id the state file names returns **HTTP 404**, and an enrol against
the same cloud returns **HTTP 201 with an 80-character token**. The probe
device was deleted afterwards; the cloud is back to 0 devices.

**Three fixes, each closing a different door:**

1. **On a local cloud, Postgres is authoritative — including when it says "no
   such device".** The old order consulted the state file whenever the lookup
   came back empty, which is exactly how a stale id outlives its own database.
   The state file exists for a REMOTE cloud, where the lookup cannot run at
   all. A stale entry found this way is now pruned as it is discovered.
2. **A rotate that 404s falls back to enrolling**, for every cloud — the same
   recovery a human would do by hand, and it covers a restored database or a
   device deleted by hand. Every other status still throws: a 401 means the
   caller lacks `outlet.manage`, and silently enrolling a second device would
   hide that.
3. **The reset prunes the state entries it just invalidated.** The script that
   destroyed the rows is the one that knows they are gone.

### The bootstrap's LAN address was the WSL virtual switch

`Get-LanIPv4` took the FIRST non-loopback IPv4 address. On this machine that is
**`172.28.176.1` — the Hyper-V vEthernet adapter WSL uses** — while the real LAN
is `192.168.0.106` on Wi-Fi. A phone on the hotspot cannot route to a WSL
virtual switch, so `VITE_KDS_LAN_URL` and the captain URL would have pointed at
an address that answers **only on the machine that printed it**, and the failure
appears on the phone, at the demo, and nowhere earlier.

**The rule is now the interface that owns the default route**, which is what
"reachable from another device on this network" means and is a fact Windows
already knows. It prints the adapter it chose, so the line can be read rather
than trusted. With no default route at all — an offline outlet, the normal case
per ADR-013 — it falls back to the first non-virtual adapter (skipping
vEthernet/WSL/Hyper-V/VirtualBox/VMware/Bluetooth/TAP) and says so; with only
virtual adapters it warns in red.

**`-LanHost` overrides it outright, and that is the demo-day setting**, because
a hotspot's address changes on every reconnect and a stale address baked into
`apps\kds\.env.dev` produces a KDS that loads, looks fine and never connects.
Documented in `docs/lan-setup.md` §1a.

**Watched, old rule against new, on this machine:** old picks `172.28.176.1`;
new prints `LAN address 192.168.0.106 on 'Wi-Fi' (the default-route interface)`;
`-LanHost 192.168.43.12` returns exactly that.

### The reset half-ran, and left the one state nothing else can detect (2026-09-12)

**What happened on the operator's run:** `demo-reset.ps1 -Force` destroyed
`edge.db.enc`, then **failed to delete the plaintext `edge.db` because the POS
was running and held it**. That leaves no sealed database and a stale
plaintext — and `recover_crash_leftovers` **reseals a leftover when no sealed
file exists**, so the next POS start would have promoted a pre-reset database
to the live one, silently and with no error anywhere.

**The T25 guard cannot cover this, and should not be changed to.** Its key
check (`verify_key_opens_sealed`) is deliberately a no-op when there is no
sealed file, because a leftover with no `.enc` is a genuine first-run crash
whose committed rows must not be discarded — `docs/spec/sync.md` is explicit
that local transactions are never deleted. The edge is right to reseal. The
only place that can tell *"first-run crash"* from *"a reset was interrupted"*
is **the reset itself**, which is why all three fixes below live there.

**1. Preflight refuses before anything is destroyed.** Two independent checks,
because they fail in different situations: any Holler POS process existing at
all (by name, and by running out of `apps\pos\src-tauri\target`), and the
files actually being locked (an exclusive open — precisely what `Remove-Item`
needs, so it cannot report "free" for a file that then refuses to delete). The
POS branch **names the pid**; the lock branch names the files and any process
it can plausibly attribute.

**2. Deletion order is reversed: plaintext first, sealed last**, with a
`try/catch` that reports whether the sealed file survived. A failure at any
point now leaves the outlet openable rather than leaving a bare leftover.

**3. The reset asserts no leftover exists after seeding.** This is the
assertion the T25 hole makes necessary: after a reset a plaintext file must
never exist, and nothing else in the system will ever say so.

### Watched firing, each one

- **POS preflight** — refused on the operator's actual machine and named the
  real process: `a Holler POS process is running (holler-pos pid 47296).
  NOTHING HAS BEEN DESTROYED.` Both files still on disk afterwards.
- **That check caught its own false positive first.** The initial version
  matched anything under `apps\pos` and named **`esbuild pid 41572` ahead of
  the till**, because the bundler runs out of `apps\pos\node_modules`.
  Narrowed to the Tauri build output directory, where the only thing that can
  open the edge database lives. The candidate-holder list in the lock branch
  had the same disease — a path match on `holler` hits every process running
  from this repository — and now matches process NAMES only, saying plainly
  when it cannot name the holder rather than guessing.
- **Lock preflight** — a POS is running on this machine, so the lock branch is
  unreachable behind it; proved on a **planted copy** with the POS branch
  disabled (the C8 planted-branch precedent, stated as such). With a real
  exclusive handle held on `edge.db`: `another process is holding …edge.db.
  NOTHING HAS BEEN DESTROYED.` Files intact.
- **Deletion order** — observed twice. In the clean run the log reads
  `deleting …edge.db` then `deleting …edge.db.enc`, in that order. In the
  failure run (handle held, preflight planted off so the delete is reached):
  `could not delete …edge.db … edge.db.enc is STILL PRESENT and the outlet can
  still be opened`, with both files on disk — **the operator's exact failure,
  now arriving with the sealed database intact.**
- **The leftover assertion** — planted a seed that seals and leaves a
  plaintext behind: `the edge devseed sealed …edge.db.enc but left a PLAINTEXT
  leftover behind: …edge.db`, with the next action saying not to start the POS.
- **And the positive case**, because a guard only ever seen going red proves
  only that it can fire: a REAL `devseed` run through the same path reports
  `seeded and sealed …, with no plaintext leftover`, all four demo-assert
  invariants OK, exit 0, and `edge.db.enc` alone in the directory.

One deliberate non-fix found while testing: a file with a **deny-delete ACL**
trips the lock preflight rather than the deletion step, because an exclusive
open fails on it too. That is the correct outcome — an undeletable file should
stop the run before anything is destroyed — so it was left alone.

**Two things this cost the operator's stack, stated rather than tidied away.**
The test runs used a scratch data directory and a scratch database throughout,
but `-BackendPort 8099` does not reach the backend it starts (the API reads
`PORT`, which the script never sets), so step 1 launched a backend that bound
**8080** and the original dev-up backend (pid 9360) is gone. 8080 is now served
by **pid 23052**, healthy, against the same database as before. The
`-BackendPort` parameter being half-wired is filed.

### Conditional B -- the admin Orders screen, and what it found (2026-09-12)

**THE ROUTE WAS NEVER MISSING, AND THE BOARD SAYS OTHERWISE.** S-ADM-08 records
"no cloud read route to build them on". The repo disagrees and the repo wins:
`backend/internal/ordering/http.go:32` has routed `GET /orders` since Milestone
1 -- `listOrders` to `svc.ListOrders` to `PostgresRepository.ListByOutlet`. What
was missing was its entry in `packages/contracts/openapi/openapi.yaml`, which is
what the earlier survey read. The conditional's own test -- "returns the EXISTING
order wire type unchanged" -- is met exactly: `CanonicalOrder`, unwrapped, no new
schema at all.

The route is now DOCUMENTED as it behaves, including the two properties a caller
must plan around and which were deliberately NOT changed here: the result is
**unbounded** (no limit, no cursor) and ordered **created_at ASCENDING**. The
newest-first ordering the screen wants is applied in the admin client, over the
response, rather than by editing a Milestone 1 query during a demo build. Both
are filed for pilot.

### Two cloud defects the screen exposed, neither visible from Go

**1. The cloud dropped `display_number` entirely.** Not in the INSERT, not in
the SELECT -- `grep display_number backend/internal/` returned nothing at all.
The edge mints it, the wire type carries it, both stores have the column, and
every replayed order in Postgres held NULL while every read served null. "Order
#A184" is the only name a human has for an order, so **the back office could not
name a single one**, and demo step 4 is "the order appears in admin". This is
contracts 0.5.9's `source_stock_count_id` again, one layer out: a column
something upstream writes, that nothing downstream reads.

**2. Timestamps were served with a `+05:30` offset.** pgx returns a `timestamptz`
in the connection's session timezone and Go then marshals RFC3339 with that
offset. The instant is correct; the shape is not. `CanonicalOrderSchema` types
these as `z.string().datetime()`, which accepts a `Z` and **rejects an offset**,
so every order the cloud served failed validation in the browser -- the screen
rendered a wall of Zod errors and no rows.

Why neither was caught: **nothing had ever read an order back from the cloud in
TypeScript.** The till reads the edge; the admin had no orders screen; and the
contract's own round-trip fixtures are authored with a `Z`, so the Go/TS drift
tests passed against a spelling the server never produces. Green on data the
real path does not generate.

Both are fixed in `PostgresRepository` and pinned by
`TestPostgresRepository_DisplayNumberAndUtcTimestampsSurviveTheRoundTrip`, which
asserts **the marshalled bytes** rather than the `time.Time` -- a `time.Time`
comparison is equal under both spellings and would have proved nothing. Each
half was **falsified separately**: removing the UTC normalisation produced
`got "2026-09-11T19:00:00+05:30"`, and passing `nil` for the display number
produced `got <nil>, want "A184"`.

### The screen itself

`OrdersScreen.tsx`: order number, IST time, type, status, items, payment, total.
It labels itself a **replica** in as many words, exactly as `GoodsReceiptsScreen`
does, because an order is edge-authoritative and one rung while the uplink was
down is real and complete at the till while absent here.

**The Items column counts lines rather than naming dishes**, because `OrderItem`
carries `menu_item_id` and no `name` -- the THIRD surface hit by the same
missing contract shape as `GoodsReceiptLineReadSchema` and `SupplierItemSchema`.
A name could be joined from the live menu, but that would print today's name
against a line sold under the old one, which is precisely what
`unit_price_paise` being a snapshot exists to prevent. Reported, not worked
around. An order whose item rows never replayed reads **"no lines synced"**
rather than an empty cell, which would look like a rendering fault.

**Observed in a real browser, not on a green build.** Chromium at 1440x900
against the real admin dev server and a real backend: signed in by clicking (no
`page.goto` after login, which would drop the in-memory session), clicked the
Orders tab, and asserted before capturing -- heading visible, 3 rows, the
replica note present, **zero UUIDs anywhere in the rendered text**, zero console
errors. Screenshot: `docs/demo-screens/scenarios/admin-08-orders.png`. The first
attempt at this is what produced the Zod wall above, which is the whole argument
for looking.

The backend for that run was a SECOND backend on 8081 against a SCRATCH database
seeded from the committed file, with three fixture orders inserted into it -- a
normal billed order with lines, a partly-replayed one with none, and a cancelled
takeaway. **Nothing touched the operator's data directory or their dev
database**, and both scratch processes and the scratch database were stopped and
dropped afterwards.

### A live-stack finding worth more than the screen

**The dev `holler` database cannot log anyone in right now**, and the reason is
not a credential: `owner@holler.test` and `cashier@holler.test` carry
`$argon2id$fixture-hash-not-a-r...`. Running the Go suite with
`HOLLER_TEST_DATABASE_URL` pointed at the shared dev database -- which is what
`docs/RESUME.md` tells you to do -- overwrites those rows with test fixtures. A
401 from the till or the console after a test run is THAT, not a typo and not
the rate limiter. It is the same misread CLAUDE.md already records costing a
debugging detour, arriving by a new route. The operator's reset fixes it.

### S-BE-09 -- the login budget is widened for the demo build (2026-09-12)

`LoginRateLimitAttempts` was a hard 5 per 15 minutes, and **a rate-limited
login is indistinguishable from a wrong password by design** (ADR-012: the
endpoint must leak nothing about whether an account exists). Five fumbled
attempts from one machine therefore lock every client on that IP for fifteen
minutes with no signal a human can act on -- and during a demo the same laptop
signs into the till, the admin console and the captain page in one sitting. The
board recorded a correct owner password refused four times in a row after the
budget was spent.

**What changed: the COUNT, and only for a deployment that asks.**
`config.Load` reads `HOLLER_LOGIN_RATE_LIMIT_ATTEMPTS` and
`HOLLER_LOGIN_RATE_LIMIT_WINDOW`, defaulting to the ADR-012 policy values, and
`auth.WithLoginRateLimit` applies them. `scripts/dev-up.ps1` -- **a dev script,
never a deployment** -- sets 50 attempts for the demo backend. The production
default in `ratelimit.go` is untouched.

**What deliberately did NOT change:** the identical 401 body, the IP-alone and
IP+tenant key pair that stops header rotation resetting the budget, and the
fail-closed path where a limiter error denies rather than allows. The board
called this a UX gap and not a security defect, and the fix keeps it that way.

Three properties are asserted, because a configurable limiter that silently
stops limiting is worse than a fixed one: the widened budget is genuinely in
force (the attempt refused under the default is allowed under the override);
**it still ends** (a limiter that never refuses is not a limiter); and a
non-positive override is IGNORED at the service and REFUSED at startup by
`config.Load`, so a typo or a `0` meaning "off" cannot quietly become either
"refuse everything" or "no limit". **Falsified** by making
`WithLoginRateLimit` ignore its `attempts` argument and watching the widening
test fail at `attempt 6 must NOT be rate limited`.

Verified: `go test -count=1` over `internal/auth`, `internal/platform/config`
and `cmd/api` through `scripts/assert-tests-ran.mjs`, 3 packages executed,
against live Postgres. `internal/platform/config` had NO test file at all
before this; it has one now.

### The menu is the client's own card now (2026-09-12)

`menu_imgs_gong/gong_menu.xlsx` replaced the invented Indian dev menu.
**277 items across 46 categories, 343 variants, 21 modifier options** --
every one of the workbook's 346 `include = Y` rows landed, and the generator
reports rejections by reason rather than dropping rows quietly (it reported
none).

**A generator, not a transcription.** `scripts/gong-menu-to-seed.py` reads the
workbook and writes `edge/database/src/bin/devseed/gong_menu.rs`, which
`devseed` consumes as its `SEED_CATEGORIES`. Hand-transcribing 346 priced rows
into Rust is exactly the step where two descriptions of one menu drift, which
is the defect the seed directory exists to prevent, one layer out.

Decisions the generator makes, each recorded in `seed/README.md` as well:

- **Absolute printed prices become base + delta.** The card prints Laksa Veg
  425 / Chicken 485 / Prawn 495; the contract stores one base price and a delta
  per variant. Base is the cheapest printed variant, so nothing rings up at a
  price the card does not print and no delta is negative.
- **Single-price items get one `Regular` variant** -- the rule the previous
  section landed, now applied by the generator rather than by hand.
- **The three Staple add-on rows become modifiers**, per the workbook's own
  Read me. Two of the seven Staple dishes carry costed deltas
  (`Add Chicken` 80 g, `Add Prawns` 70 g, `Add Mixed Meat` as TWO rows -- 45 g
  chicken plus 35 g prawn, because a delta is per (modifier, inventory item)
  and a blended SKU would be fabricated). The other five carry the same
  modifiers uncosted, which is legitimate and is the path that also needs seed
  coverage.

**The larder and recipes moved with the menu.** 38 inventory items (no paneer,
no atta, no kasuri methi -- prawns, tofu, pak choi, kimchi, jasmine rice, udon,
fish sauce, coconut milk), **16 root recipes plus 2 sub-recipe batches**, 85
ingredient rows, 7 supplier items and the seeded GRN all re-pointed. The two
sub-recipe carriers kept their ids and became **Thai Green Curry Paste** (300 ml
batch) and **Stone Bowl Sauce Base** (480 ml batch).

**Demo step 3's dish is pinned by a test.** `Kimchi / Chicken` (a Stone Bowl)
pulls 60 ml of a 480 ml sauce batch -- a 1/8 multiplier, never 1x -- and the
test asserts the leaf arithmetic: 150 g chicken direct; soy sauce reachable
ONLY through the sub-recipe at exactly 22.5 ml; and spring onion appearing BOTH
directly (15 g) and inside the batch (5 g) and therefore SUMMING to 20 g rather
than overwriting. `Iced Tea` is the second demo line, so the stock screen shows
two unrelated items moving.

**`ALCOHOL_VAT_UNCONFIGURED`: 141 bar items, CGST 0 / SGST 0.** The card taxes
alcohol as VAT and `tax_rule.component` is CHECKed to CGST/SGST/IGST/CESS under
0.8.1, so VAT is not expressible. The operator's ruling was a dedicated
zero-rate profile so **no wrong tax amount is ever charged or printed**, falling
back to GST-5 only if a zero rate were inexpressible -- it is expressible
(`rate_bps INTEGER NOT NULL CHECK (rate_bps >= 0)`), so the fallback was not
taken. **This is a hole, stated as one:** the bar's real VAT is not collected,
the demo script must not bill a bar item, and the VAT component is filed in
`docs/pilot-readiness.md`.

**`veg_flag` and `description` are dropped, deliberately.** `menu_item` has no
column for either. Filed for the next additive contracts bump;
**FSSAI requires the veg marker on a menu**, so it is a pilot blocker, not a
nicety. The data stays in the workbook.

**The legacy chai/thali pair survives, hidden.** `tests/e2e-scenario/harness`
pins those two ids, that exact 4000-paise price, that single-station routing and
that `tax_profile_id = None` fallback, so removing them would break the harness
silently. Both are now `is_available: false` in a category named
`Test fixtures (internal -- not sold)` sorting last. A greyed-out Masala Chai on
a modern Asian till is a blemish; an orderable one is a wrong menu.

**The spec/seed guard was re-pointed, not deleted.**
`spec_and_seed_agree_on_item_and_variant_counts` parsed `HOLLER_DEV_MENU_SPEC.md`
because that document and `SEED_CATEGORIES` were two hand-maintained copies of
one menu. The Gong catalogue is generated, so the drift worth catching changed
shape: a hand-edit of the generated file, or a workbook edit with no
regeneration. `gong_menu_matches_the_generated_manifest` rebuilds the counts AND
a canonical projection of every item, price, station, tax class, variant delta
and modifier delta, and compares both against `seed/gong-menu-manifest.json`.
**Falsified** by changing one price in the generated file by a single paisa and
watching it fail by name. The checksum is FNV-1a rather than a real digest
because this crate carries no hashing dependency and the threat is an accidental
hand-edit, not an adversary -- twelve lines of arithmetic on both sides beat a
new dependency in the edge binary for a dev-only guard.

### Seed parity after the swap, per store

- **Cloud, row for row:** clean scratch database (`holler_seedcheck`), the real
  Go seeder, the real committed file, `EXIT=0`; then **ten tables compared by
  id, field for field** -- `menu_category` 48, `menu_item` 281,
  `menu_item_variant` 347, `menu_item_modifier` 23, `recipe` 18,
  `recipe_ingredient` 85, `inventory_item` 38, `supplier_item` 7,
  `tax_profile` 4, `tax_rule` 8. **Zero rows on one side only, zero content
  differences.**
- **Edge, row for row, and now permanent:**
  `edge_rows_match_the_shared_catalogue_row_for_row` compares every field of
  every catalogue row against what `seed()` actually stored in SQLite. It runs
  in CI on every commit rather than once during a reset. **Falsified** by adding
  1 paisa in the edge write path and watching it name the row and column.
- **Edge, end to end:** a scratch data directory bootstrapped from the committed
  file -- 281 items, 48 categories, 18 recipes, 38 inventory items,
  `GRN/20260809/0001` with 7 lines and the expected single `NO_PURCHASE_ORDER`
  gap -- then `scripts/demo-assert` on the sealed result:
  **0 blocked ranged-stream rows, 0 stock deduction gaps, 0 blocked outbox rows,
  0 persistently-failing rows (the banner proxy). All checks OK.** That is work
  item 1's post-seed assertion, met on a clean bootstrap. It ran against a
  scratch directory and a scratch key, never the operator's data directory.
- 329 edge tests executed through `scripts/assert-tests-ran.mjs`, clippy and fmt
  clean, `check-seed-drift` green, `go test -count=1 ./cmd/devseed/...` green
  against live Postgres, and all three `check-seams` crates check clean.

### Every menu item now carries a variant (2026-09-12)

Twelve of forty-three seeded items had no `menu_item_variant` row at all:
Veg Thali, Samosa (2 pc), Pani Puri (6 pc), Aloo Tikki Chaat, Seekh Kebab
(4 pc), Egg Bhurji, Jeera Rice, Steamed Rice, Laccha Paratha, Filter Coffee,
Gulab Jamun (2 pc), Gajar Halwa. That is the scenario board's S-CAP-06, and
the cloud-side histogram in S-SYNC-08 (`0|12, 1|9, 2|22`) is the same twelve
seen from Postgres, not a second defect.

**A variant-less item fails twice, silently and in different directions.** A
`recipe` binds to a `menu_item_variant_id` (ADR-018 §2.1), so such an item can
carry no recipe and **selling it deducts no stock whatsoever** — demo step 3
reads exactly that. And `apps/captain` refuses to put it in a cart at all
(`MenuCartScreen.tsx:56-60` — *"has no variant configured. This is a seed
defect"*), so on the waiter's phone twelve of forty-three items were
un-orderable in step 1a. Neither failure announces itself on a screen.

Fixed at the authoring source, both halves together, because the seed is bound
to a spec document by a test: the eleven `variants: &[]` rows in
`SEED_CATEGORIES` and the matching eleven em-dash cells in
`HOLLER_DEV_MENU_SPEC.md`, plus the legacy `ITEM_THALI_ID` fixture, which
takes a new fixed `VARIANT_THALI_ID`. Every added variant is named `Regular`
with `price_delta_paise: 0` and `is_default: true`, so **no price moves** and
the legacy ids, prices and routing `tests/e2e-scenario/harness` pins are
untouched. Six items already carried a `Regular` for the costing reason alone;
this extends that precedent to the rest rather than inventing a convention.

**The old fixture became the guard.** `samosa_has_no_variant_at_all` asserted
the defect — it needed a variant-less item to exist. It is now
`a_null_variant_is_a_novariant_gap_and_no_seeded_item_is_variant_less`, which
keeps the real subject (a null variant resolves to `GapReason::NoVariant`,
a path aggregator lines still reach) and adds the inverse: no seeded item may
be variant-less. **Falsified** by planting `variants: &[]` back on Samosa and
watching it fail by name — `these have none: ["Samosa (2 pc)"]` — then
reverting. `spec_and_seed_agree_on_item_and_variant_counts` moved 50 -> 61 with
both sides edited together, which is the whole point of that guard.

### Verified, and on which store

- Edge authoring source: 8 tests executed via
  `scripts/assert-tests-ran.mjs`, `cargo fmt --check` and
  `cargo clippy --bin devseed` clean. (`cargo fmt` also absorbed three
  pre-existing hunks in that file — an import order and a `println!` wrap —
  that were red before this work touched it.)
- Re-emission byte-stable: emitted twice, SHA-256 identical
  (`F99D4454…3763AC`), and `check-seed-drift.mjs` green against the committed
  file.
- Cloud: `go test -count=1 ./cmd/devseed/...` against live Postgres, and the
  real seeder run against a **clean scratch database**
  (`holler_seedcheck`) from the real committed file — `EXIT=0`, then queried:
  **43 items, 0 variant-less, 65 variant rows, histogram `1|21, 2|22`**,
  matching the emitted file exactly.

### The row-for-row comparison, half-closed

The UNRESOLVED item above asked for contents, not counts. Cloud-versus-file is
now done on the clean seed, by id, field for field:
`menu_item` 43, `menu_item_variant` 65, `recipe` 24, `recipe_ingredient` 93,
`inventory_item` 32, `supplier_item` 7 — **zero rows only in one side, zero
content differences in any table.** The edge half still needs
`scripts/demo-reset.ps1 -Force`, which only the operator can run, but both
stores read the same committed bytes.

### A finding: the cloud seeder cannot renumber ids in place

Running the seeder against the **already-seeded** dev database failed loudly:

```
seeding menu_item_variant Half: ERROR: duplicate key value violates unique
constraint "idx_menu_item_variant_one_default" (SQLSTATE 23505)
```

Adding variants mid-sequence shifts every later `menu_variant_id(seq)`, and the
seeder **upserts without pruning** (contracts 0.7.0, config apply has the same
property), so the previous seed's default row for that item survives beside the
new one and the one-default-per-item partial index rejects the pair. It is not
a defect in the fix and it is not silent — but it means **a seed change that
renumbers ids is only safe through `scripts/demo-reset.ps1 -Force`**, which
drops the schema first. The live dev `holler` database therefore still holds
the pre-change variant rows until that reset runs.


**Design: one emitter, one committed generated artefact, two readers.**
`edge/database/src/bin/devseed.rs --emit-json` produces `seed/demo-outlet.json`;
both seeders consume that file. Hand-transcribing the catalogue into JSON was
considered and rejected — the transcription step is exactly where two
descriptions drift, which is the defect being fixed, one layer out. Full
reasoning in `seed/README.md`.

**What it replaced:** two seeders describing one outlet by hand-mirrored
constants, each with a comment saying it MUST match the other, diverged by a
factor of twenty (edge ~39 menu items, cloud two). That divergence is the direct
cause of the thirteen permanently-blocked rows in the live edge outbox, each
`missing_reference (HTTP 422)` on `order_item_variant_id_fkey`.

**Content:** 43 menu items, 10 categories, 24 recipes, 93 `recipe_ingredient`
rows, 32 inventory items with opening stock, one supplier with pack sizes, and
`GRN/20260809/0001` with 7 lines.

### A defect the gate caught, and why three green signals missed it

The emitter produced `tax_rule.id` as `format!("{id}-{component}")` —
`0191e600-...-000001-CGST`, **not a UUID**. `tax_rule.id` is `UUID PRIMARY KEY`
in Postgres, which rejected it on the first insert with SQLSTATE `22P02`.

It survived three green signals:

- **SQLite has no UUID type**, so every edge test stored it happily.
- **The emitter is byte-stable**, so `check-seed-drift.mjs` regenerated the same
  broken bytes and stayed green.
- **The cloud's test fixture hand-authored a valid UUID in that exact field**, so
  the Go tests passed against a fixture that diverged from the real emitter on
  the one field that mattered.

It was only visible from the store that enforces the type. Fixed by
`tax_rule_id(seq)` in its own disjoint range (`6467bc8`), then **proved against
Postgres**: the Go seeder run against the real committed file, exit code captured
unpiped, `EXIT=0`, six real UUID rows confirmed by direct query.

The same sweep found the identical pattern in `seed_billing()`, left alone with a
reason: it is gated behind `HOLLER_SEED_BILLING`, edge-only, and never reaches
the shared JSON or Postgres.

### Verified

- Edge: 324 tests, `cargo test` native Windows; clippy and fmt clean.
- Cloud: 6 tests, `go test` native, against live Postgres.
- Re-emission byte-stability: emitted twice, `diff` identical.
- `check-seed-drift.mjs` falsified by hand-editing `outlet.name` and watching it
  report STALE.

### UNRESOLVED

**Half-closed on 2026-09-12** — cloud-versus-committed-file is now compared by
id, field for field, on a clean scratch database (six tables, zero
differences; see the seed-parity section above). What remains is the EDGE
half, which needs `scripts/demo-reset.ps1 -Force` and therefore the operator.
The original statement follows, unedited.

**Row-for-row comparison of the two stores has never happened.** The live
Postgres carries 991 `menu_item`, 123 `tax_profile` and 345 `recipe` rows from
unrelated prior work, so there is no known state to compare against. Settled by:
run `scripts/demo-reset.ps1 -Force`, then diff `menu_item`,
`menu_item_variant`, `recipe`, `recipe_ingredient`, `inventory_item` and
`supplier_item` between Postgres and the edge SQLite by id. **A matching count
with mismatched contents is exactly what the old hand-mirrored constants
achieved**, so compare contents, not counts.

### The goods receipt is seeded into both stores directly

`edge/sync/src/route.rs` maps only `order` and `table_session` (carried gap A7),
so a GRN seeded at the edge can never reach the cloud, and demo step 5 shows it
in the admin console. It is therefore written to both stores from the same shared
description. **That is an accommodation for A7, not a fix for it**, and the
cloud's copy is still a replica that every surface must label as one (contracts
0.7.0).

### `.gitattributes`

`check-seed-drift.mjs` compares raw strings and the repository has
`core.autocrlf=true` with no `.gitattributes` at all, so a fresh clone would have
written the file CRLF against an LF emitter and reported STALE on every clean
checkout. Pinned at `1551bee`. A guard that cries wolf gets switched off, and
switching this one off restores the condition it exists to prevent.

---

## Item 2 — the reset command

`scripts/demo-reset.ps1` plus `scripts/demo-assert/`, a dev-only Rust binary that
opens the sealed edge database, asserts four invariants by name with actual row
counts, and reseals on the way out either way.

**The assertion tool was falsified**: a blocked row was planted and `run()`
watched reporting FAIL rather than passing silently.

**Two assertions in the original spec named things that cannot be queried**, both
found while implementing them and corrected in `seed/README.md` at `02a51fd`:

- `local_outbox` **carries no blocked flag**; the rows live in
  `sync_outbox_block` with `blocked_at` set (contracts 0.6.4). Written literally,
  that assertion would have passed by querying a column that does not exist.
- **"The POS sync banner is empty" cannot be answered by a database.** The check
  queries what the banner reads and is required to say in its output that it is a
  **proxy**, not an observation of `SyncBlockedBanner.tsx`.

`stock_deduction_gap` is **not** `grn_gap`. The demo seed legitimately produces
one `NO_PURCHASE_ORDER` row in `grn_gap`, because a GRN never blocks on a PO
(ADR-019); asserting zero there makes a correct seed look broken.

**Never exercised**: the real `-Force` run — deliberately, since it drops the
shared dev Postgres schema and deletes the shared edge database while other work
had live state in both. Specifically unexecuted through the script: the
`DROP SCHEMA ... CASCADE`, the `Remove-Item` on `edge.db.enc`/`edge.db`, both
devseed invocations as driven by the script, and the backend kill.

Guards: `-Force` or `-WhatIf` required; the full destruction banner with resolved
absolute paths prints **even when the run refuses**, so a caller who forgot
`-Force` still sees what refusing avoided. **No backup step of any kind** — the
edge database is never copied anywhere unencrypted (ADR-011), and a convenience
backup is how that rule gets broken.

---

## Item 0 — `apps/captain`

**Approved reduced scope**: pair, tables, menu+cart, send. No bill screen, no
payment, no paid modifiers. `order.source` stays `POS`. **No contract change was
needed** — `WAITER` already exists in `device.kind`.

The JSON API was pinned in `docs/captain-api.md` **before either half was
built**, so the frontend and the listener could be built in parallel from one
document.

**Verified as a seam, not just as two halves.** All five endpoints were compared
field-for-field between the Rust wire structs and the frontend's Zod schemas:
**no mismatches**, including ones that currently happen not to matter. The auth
reimplementation was checked rule by rule against `CachedCredentialVerifier` and
preserves every one, fail-closed on an unparseable expiry included. **No path
exists for a client-supplied `device_id`** — confirmed by reading both sides.

**The hub gap.** The listener's first test suite built its state with
`AppState::new`, which sets `hub: None`, making `notify_kot` a no-op. So 5/5
green proved the KOT row landed in SQLite and **nothing about the hub** — while
the cut-off condition is *reaches the KDS **and the hub***, and the hub is what
the KDS renders from. Closed at `d805218`: the test now subscribes to a real
`Hub` and asserts a `KotUpserted` frame arrives with the matching `kot.id`,
`order_id` and `station`. **Falsified** by pointing the subscription at a decoy
`Hub` never wired into the running state and watching `recv_timeout` fire.

### Known, filed, not fixed

**A captain-originated KOT's `created_by_device_id` is the till, not the
waiter.** `send_order_to_kitchen_impl` stamps it from `state.device_id`.
`order.device_id` **is** correctly attributed to the waiter — these are two
different columns and only the KOT one is wrong. It is invisible today: **the KDS
renders no ticket origin at all, for any ticket.** Also, attribution is captured
only at order-create time, so appended lines record nothing about who added them
in either direction.

### Still standing between here and the cut-off

A WAITER device enrolled; a real phone on the hotspot; three runs from a clean
reset with the POS process staying up across all three. **No browser has ever
talked to the real listener** — the frontend's own tests run against mocks, and
the seam is confirmed by static comparison plus a hand-rolled HTTP client sending
the frontend's exact shapes.

---

## Item 3 and 0b — the receipt

The file sink emits **raw ESC/POS bytes**, plus a byte-derived `.txt` companion
that already existed. `render_invoice_html` adds a third representation: an
**independently coded** HTML renderer, not derived from the byte stream.

**An equivalence test binds them line for line**, and it was falsified by
changing the HTML renderer's GSTIN label to "GST No" and watching it catch the
divergence — which is what demonstrates the two are genuinely independent.

The HTML opens on print, **invoices only**: a browser tab per kitchen ticket
would be worse than none.

**This proves the render, not the device.** It does **not** close the parked
ESC/POS-on-paper gate (ADR-013) — no physical printer exists in this
environment.

### The UPI QR

On the invoice screen and on the receipt. The screen's QR was verified by
rendering it in real Chromium and **decoding the pixels back with `jsqr`**; the
receipt's by reconstructing the SVG's dark modules and decoding with `rqrr`.
Both round-trip to the expected link.

`qrcode` is pinned at `=0.14.1`, default features off, **zero transitive
dependencies**; the decoder is dev-only and not in the shipped binary.

**A divergence the shared vector could not catch.** The screen passed
`inv.invoice_number` as the transaction note and the receipt passed
`ctx.order_display_number` — two QRs for one bill carrying different references.
The pinned vector fixes the encoding given four inputs and says nothing about
which field supplies them: **a vector test proves agreement only for the
arguments both sides are handed.** Aligned on the invoice number at `29df694`,
falsified by pointing one side at the other field and watching
`tn=%23A184` versus `tn=FY26%2FPNQ%2F001423`. Slash encoding inside a query
parameter confirmed by the same real round-trip.

**Two environment variables must hold the same value.** Vite requires the
`VITE_` prefix to expose a variable to the client bundle, so the screen reads
`VITE_HOLLER_DEMO_UPI_VPA`/`_PAYEE_NAME` and the native side reads the
unprefixed pair. **Nothing detects a mismatch.** If the VPA is unset, **no QR
renders at all** — a QR aimed at an empty payee opens a real payment app on a
customer's phone pointed at nobody.

**This is not a payment integration.** Nothing reconciles a scan against a
received payment; the cashier still records the tender.

---

## Items 4 and 6 — banner and presentability

UUID leaks found and fixed on the order list (raw order UUID, truncated KOT
UUID), billing (cash-shift UUID, `reverses_payment_id`), the admin header
(outlet UUID), and the aggregator screen — which showed the raw `menu_item_id`
on every **matched** line and the item name only when unmatched, backwards from
what a human needs. IST rendering added; storage stays UTC.

`bundle.icon` was `[]`, so **the installed executable has been shipping with no
icon at all**. Wired to the existing `.ico`, which is itself a placeholder.

The `SyncBlockedBanner` rewrite preserves the distinction its header draws
between rows **given up on** (`blocked_at` set) and rows **still trying**
(`blocked_at` null, attempts high). Telling an operator "given up" when the truth
is "still retrying" is a different lie in the same family as halting sync
silently.

### Observed in a real browser, and it found three bugs

Chromium via the Playwright already installed under `apps/kds`, driven against
each app's real dev server. **Admin at 1440x900** (backend confirmed by PID
58148 through a health fetch, not by the port answering) and **POS at 1440x900**
with Tauri IPC injected as contract-shaped fixtures, since no Tauri shell can be
launched here.

**The banner deliverable was inverted, and only a browser could see it.**
`.pos-screen` is a CSS grid with every cell claimed by a named area; the three
banners carried no `grid-area`, so auto-placement collapsed the sync banner into
the **140px category sidebar** — the exact opposite of *full width, legible from
a metre*. It was a regression introduced by the presentability pass's own CSS,
and 234 passing tests, a clean `tsc` and a clean `vite build` all saw nothing.
Fixed with a dedicated `.pos-banners { grid-area: banners }` row.

Two pre-existing bugs found in the same run:

- **`.sync-blocked-list { display: flex }` beat the `hidden` attribute**, so
  "Hide details" relabelled the button and left every row visible. This is the
  `[hidden]{display:none}` specificity trap, live.
- `OrderListScreen.tsx` mapped rows with `<>` shorthand fragments, which cannot
  carry a `key` — a real React console warning.

Plus a smaller one: the IST helper rendered lowercase `am`/`pm`, which reads as a
typo on a bill.

After the fixes, screenshots confirm the populated banner full width with both a
**will not reach the cloud** row (HTTP 422, `missing_reference`) and a **still
retrying** row (HTTP 503), each reading `Order #A184 - Butter Chicken` and never
a UUID; the collapse toggle genuinely hiding rows; and the banner element count
at **0 when both queries are empty** — fully absent rather than an empty box.

One false alarm was ruled out by the agent itself: a raw UUID on a category tab
traced to its own fixture using `display_order` where the contract says
`sort_order`, which failed Zod validation and emptied the category map. Not a
product defect.

**The KDS could not be reached**, and was not worked around.
`VITE_KDS_DEVICE_TOKEN` lives in `apps/kds/.env.dev`, which is deny-ruled to
agents exactly as the POS's is, and no edge LAN server is running here anyway.

---


## Presentability, raised from a work item to a REQUIREMENT

The operator raised it mid-build, with four parts: the Holler logo on every
surface; **one design token set shared by all four apps, with no screen using
ad-hoc values**; every screen the demo touches screenshotted for review; and a
PWA manifest on the captain page.

**Theme decision, the operator's:** light everywhere, **the KDS alone on the
dark surface**. The captain page follows the till. The reasoning, which should
not be relitigated: a till in a bright room reads better light and matches the
brand, while a kitchen display is glanced at from across a hot line where
dark-on-light glare is worse — which is why every KDS on the market is dark.

### `packages/ui` — the token set

`packages/ui` was **completely empty**; four apps carried 1,053 lines of
unrelated CSS and the till alone used `#333`, `#f66`, `#b06fd0`, `#3a6` and a
dozen more values chosen per screen.

Every primitive is lifted from `website/holler-branded-website.html`, the
existing brand system. **Two values are derived and both say so at their
definition**: the brand carries no red and a till needs one, so a void or a
permanently-blocked sync row does not read as the same severity as a warning;
and the brand's muted text colour is tuned for paper and fails legibility on
ink.

**The constraint that keeps two surface modes from becoming two products:** the
dark block remaps semantic tokens only and never redefines a primitive. The four
status colours are deliberately **absent** from it, so they inherit the light
values unchanged — a cook and a cashier read the same green as the same green.
Where contrast needs work on ink, the tinted **ground** moves and the status
colour does not.

Three rules in `base.css` came from real defects rather than taste:
`[hidden] { display: none !important }` (a `display: flex` rule beat the
attribute, so "Hide details" relabelled the button and left every row visible);
`--control-height: 44px`, the smallest target reliably hit without looking on a
touchscreen till or a one-handed phone; and `.money`'s tabular figures, so a
column of totals lines up and does not jitter.

**Both adoption passes needed ZERO new tokens.** Two independent builders, four
apps, no gaps — reasonable evidence the semantic layer was drawn at the right
altitude.

### The screenshots — and what nearly shipped instead

`docs/demo-screens/` holds the reviewed set with an `index.md` naming each
screen, its demo step and what changed.

**The first attempt produced nine byte-identical copies of the login screen
under nine different filenames.** `page.goto()` reloads after login dropped the
in-memory auth store, so navigation never landed and the harness photographed
whatever was on screen. Caught by hashing the files, not by looking at them.

That is this repository's recurring failure in visual form: **an artefact that
looks like coverage and is one observation** — the same shape as a fidelity test
passing on absent data, or a suite reporting zero executed tests and exiting 0.
Had it been reported as "nine screens observed", the claim would have survived
until the operator opened them.

The discipline that replaced it, and that every later capture follows:

- **Navigate by clicking in-app; never `page.goto()` after login.**
- **Assert the screen's own unique content before capturing**, throwing loudly
  rather than photographing whatever is there.
- **Hash the full set afterwards** and confirm every file is pairwise-distinct.

Two fixture traps found by that discipline, both of which silently empty a
screen: an ambiguous `:has-text("Kitchen")` selector clicking "Send to Kitchen"
instead of the intended control, and a hand-built invoice whose
`grand_total_paise: 87150` failed `InvoiceSchema`'s **whole-rupee refine** and
skipped the entire invoice panel including the UPI QR.

### Token adoption is not the same as looking designed

The first POS pass applied every colour token correctly and the screens still
did not look like a product: **`.btn` was never applied** — every control was a
raw browser default about 24px tall, against a token set shipping 44px — and
**no sub-screen carried the logo**, so clicking from the till to Orders or
Billing left the product with no identity at all. Neither was a token problem;
the classes simply were not used.

That gap was only visible by **opening the screenshots and looking at them**.
The pass reported success truthfully by its own terms — tokens adopted, tests
green, screens observed — and the product still looked unfinished.

### Two defects the reviews caught, reported rather than fixed

- **`SuppliersScreen` rendered a raw `inventory_item_id`** on screen. Fixed by
  withholding — `ingredient on file` — exactly as `GoodsReceiptsScreen` already
  did. Same root cause: `SupplierItemSchema` carries no `inventory_item_name`.
  **That is the second surface hit by the pending contract decision.**
- **The admin and the till formatted money differently** — `1800.00` against
  `₹1800.00`. Aligned, and split into a ₹-prefixed display formatter and a plain
  one, because `MenuScreen`'s **editable** price input is seeded from it and a
  `₹` there breaks `parseRupeesToPaise` on save. A cosmetic fix would have
  introduced a real defect.

### Logo, icons and the PWA manifest

`imgs/` holds the mark; **there is no SVG in this repository**, every asset is
raster.

- **`bundle.icon` was `[]`** — the installed executable had carried **no icon at
  all**, and `icon.ico` was a 16x16 placeholder of 1086 bytes from the original
  scaffold. Replaced with a real multi-resolution `.ico`, **verified by parsing
  the `ICONDIRENTRY` table by hand**: six entries at 16/24/32/48/64/256.
  Window and installer icons both come from the same `bundle.icon` array — the
  first `.ico` is embedded at resource ID `32512` (`IDI_APPLICATION`), read from
  `tauri-build`'s source rather than assumed.
- The receipt header carries the mark as a data URI. The full-size asset would
  have added **~108KB of base64 to every rendered receipt**; downscaled to 180px
  wide it adds ~51KB, screenshot-verified as still sharp at its display size.
  Proposed rather than absorbed silently.
- **The captain's `.webmanifest` was served as `application/octet-stream`** by
  the POS listener, because `mime_for()` had no case for it. A manifest with the
  wrong content type is **ignored silently**, so "Add to Home Screen" would have
  given a screenshot icon and a browser-chrome launch. **Vite's dev server types
  it correctly on its own**, so verification against Vite looked clean and only
  the shipped path was wrong — the failure would have appeared on the phone, at
  the demo, and nowhere earlier. Fixed, with the test asserting the served
  header over the real socket rather than unit-testing the map, because a unit
  test would have passed the day the defect shipped.

### Outstanding

- **No Inter font is shipped.** None exists in the repository and fetching one
  was forbidden — an outlet with no uplink is the normal case (ADR-013) and a
  web font that fails to load silently re-renders the whole product in a
  fallback. All four apps therefore use the `--font-sans` stack, which resolves
  to Segoe UI on Windows. It looks deliberate rather than broken, but it is not
  the brand face. **With the operator.**
- **The POS icon artwork** is the existing raster mark; no designer asset
  exists.

## A finding about the milestone that just closed

**`adr020_outbox_drain` has been red since 2026-09-08, and M6 was tagged
`m6-complete` on 2026-09-11 with it red.** `bdc40d3` added the aggregator pull to
`AppState::drain_outbox` during M6 Phase C; that test's fake cloud excludes only
`/sync/config` from its ingest counter, so it counts a legitimate call as a
replay. Both commits are ancestors of this work's base, confirmed by
`git merge-base --is-ancestor` — nothing in the demo build caused it.

The close therefore either did not run the suite or did not read what it
returned. Filed in `docs/backlog.md` at `e883820`, **with an instruction not to
fix it by widening the exclusion**: the test's subject is that the drain
publishes before the seal and nothing after, and whether the aggregator pull
belongs inside `drain_outbox` at all is the question to answer first.

---

## The devseed idempotence fix is in a commit that does not mention it

`edge/database/src/bin/devseed.rs`'s GRN and opening-stock seeding was not
idempotent: a second run against the same data directory died on
`UNIQUE constraint failed: goods_receipt_note.id` and **aborted before the
bootstrap's device-enrolment steps**, leaving the edge seeded and the KDS and
captain devices un-enrolled — the worst of the three outcomes, because it looks
like it half-worked. The operator re-running the bootstrap on demo morning is
the normal case, not the exceptional one.

The enumeration found a second instance of the same shape: `write_opening_stock`
would have failed on the very next run, invisible only because the GRN aborted
the process first. Both now check for their fixed-id row and skip, which never
attempts a second write against `stock_ledger_entry`'s append-only trigger and
so neither weakens nor routes around it. Three consecutive runs produce
identical counts — `goods_receipt_note: 1`, `grn_line: 7`, `grn_gap: 1` (the
expected `NO_PURCHASE_ORDER`, neither multiplied nor eliminated),
`stock_ledger_entry: 39`, `stock_count_line: 32`.

**That fix lives in `e9ec3dc`, whose message is
`style(admin): adopt @holler/ui tokens and add the header logo`.** Two builders
were running in parallel and the styling track staged a file it did not own,
sweeping the devseed change into its own commit — the exact incident CLAUDE.md's
commit rules describe, this time caused by concurrency rather than by
`git add -A`. The commit is pushed, history rewriting is denied, and rewriting
shared history to correct a message would trade a real risk for a cosmetic gain.
So the pointer is recorded here instead: **`git log` on `devseed.rs` will show an
admin CSS commit message; the change is real, tested and correct.**

The reusable lesson is narrower than "be careful": **staging by path is not
enough when tracks overlap in time.** A builder that stages only its own paths
can still capture another builder's work if that work is sitting staged in the
same index. Ownership boundaries in a brief prevent two agents editing one file;
they do not prevent one agent committing another's.

## Two commits are missing their `Claude-Session:` trailer

`e9e7337` and `29df694`. Both are pushed. The builder flagged them rather than
amending, which was correct — rewriting shared history to add a trailer trades a
real risk for a cosmetic gain. Recorded here instead.
