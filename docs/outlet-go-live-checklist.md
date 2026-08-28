# Outlet go-live configuration checklist

**Why this document exists.** Four separate backlog entries, filed in four
places across M2–M4, turned out to be one thing: **an outlet cannot trade until
it is configured, and every one of these fails correctly and loudly rather than
falling back.** That design is deliberate and is not up for revision — a wrong
tax code that looks configured is worse than a missing one, and a bill queued
into a printer that does not exist is worse than a refusal by name. But a
loud failure is only a good design if somebody is holding the list of things
that must be true before it fires.

This is that list. Written 2026-08-28 (M5 T0), from `docs/m5-planning.md` §1.5.

**Run it before any rollout, demo, or pilot.** Nothing here is optional, and
nothing here has a default that will do.

---

## 1. Catalogue — blocks every invoice

- [ ] **`menu_item.hsn_sac` set on every sellable item.**
      It is NULL on every row of every existing edge database. The edge
      **rejects invoice issuance** when any line's code is NULL or blank
      (contracts 0.4.5), because a GST invoice without HSN/SAC is not a
      compliant document. There is no fallback and there will not be one.
      **Until this is done the outlet cannot issue a single bill.**
- [ ] Every sellable item has at least one variant, or the till cannot resolve
      one. Resolution is by cardinality (0 → null, 1 → silent, 2+ → mandatory
      picker); an item with no variant sells but deducts no stock and lands on
      the stock-deduction-gaps report as `NO_VARIANT`.
- [ ] Recipes authored for every item whose stock should move. An item with no
      recipe **still sells** (by design) and appears on the gaps report as
      `NO_RECIPE`. That report is the tool for finishing this list, not an
      error log.

## 2. Compliance and billing — blocks every invoice

- [ ] **Tax profile** created and active, and assigned per item where the
      outlet default does not apply (`menu_item.tax_profile_id`, 0.4.2 — null
      falls back to the outlet default).
- [ ] **Outlet fiscal profile** set: GSTIN, legal name, registered address.
- [ ] **Invoice series** created and active, with a prefix whose date token
      matches its `reset_policy`. **A non-`NEVER` policy whose prefix lacks the
      matching date token yields duplicate invoice numbers across periods.**
      Today that is caught only by the UNIQUE index, at issue time, in front of
      a customer — config-write validation is filed to M6.
- [ ] Discount definitions created, if the outlet uses any.

`scripts/dev-bootstrap.ps1 -WithBilling` seeds all of the above for
**development only**. It is not a rollout tool.

## 3. Hardware — blocks printing, not billing

- [ ] **At least one printer with the `BILL` role.** A printer with no role row
      is a candidate for **neither** path — absence is never read as "sure,
      print bills to it" (contracts 0.4.7). An outlet with no BILL printer
      fails loudly by name at issue time; `print_invoice` does not queue into
      nothing.
- [ ] At least one printer with the `KITCHEN` role, and station→printer
      routing set for every station that should print.
- [ ] **Verified on paper**, not just in the file sink. The file-sink transport
      proves the byte stream; it does not prove a device accepts it. Vendor
      ESC/POS dialects, 58mm vs 80mm layout, the cutter, codepage and non-ASCII
      glyphs, and USB/Bluetooth-SPP timing are all unproven until a real
      printer has printed. *(ADR-013 open gate.)*

## 4. Inventory — blocks nothing, but makes the low-stock signal meaningless

- [ ] **Real reorder levels on every stocked item.** The seeded values are
      placeholders: 28 of 32 items read LOW, which buries the signal the
      low-stock banner exists to give. **A banner that is always on trains
      people to ignore it**, and it will still be ignored on the day it is
      right.
- [ ] Par levels set where the outlet reorders to a target rather than a
      threshold.
- [ ] Opening stock established by a physical count. Note that `devseed.rs`
      writes **no** ledger rows at all, so a stocked item can only have been
      stocked by a count — there is no other inbound path until M5's GRN lands.
- [ ] Unit conversions authored for every item bought in a unit other than its
      base unit. From M5, receiving converts the supplier's purchase unit
      exactly once, at the edge, through `item_unit_conversion`; a missing
      conversion is a gap, not a refusal.

## 5. Identity and devices

- [ ] Real users created with real roles. The dev principal is not a rollout
      account.
- [ ] Permissions actually cover the surfaces staff will use. `inventory.manage`
      and `inventory.count` were missing from the dev principal until
      `a6e02d7`, and the symptom was screens that simply did not work.
- [ ] **Each POS/KDS device enrolled** through `POST /devices/enroll`
      (ADR-017). The plaintext token is returned **once**; there is no second
      chance to read it.
- [ ] `outlet.timezone` and `outlet.day_start_time` set. The business day may
      cross midnight, and these two define where it starts. Leaving
      `day_start_time` at the `00:00` schema default is a decision, not a
      default — make it deliberately.

## 6. Sync

- [ ] The edge has pulled `GET /sync/config` at least once and holds users,
      menu, stations, printers, tax config and device credentials.
- [ ] Confirm the edge SQLite file is **encrypted at rest** and that no backup
      copies it anywhere unencrypted (ADR-011). It caches Argon2id hashes so
      login works offline.

---

## Verifying the list rather than trusting it

Every item above is a **sink**, not a screen — which is the point. Do not check
this list by opening screens and looking; check it by asking what refuses to
work and confirming it no longer refuses:

1. Issue one real invoice end to end. That single act proves §1 HSN/SAC, §2
   entirely, and §3's BILL printer at once.
2. Sell one dish and confirm ledger rows appear. That proves §1 variants and
   recipes, and §4's opening stock.
3. Open the low-stock banner and confirm the LOW set is small and plausible.
   If most of the catalogue is LOW, §4 is not done.
4. Disconnect the network and repeat 1 and 2. That is the product's core
   promise and the only check that proves the outlet can trade on its own.
