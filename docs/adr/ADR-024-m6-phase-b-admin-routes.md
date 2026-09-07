# ADR-024 — Contracts 0.7.0: the two admin routes Phase B needs, and the error-code enum

- **Status: ACCEPTED and APPLIED, 2026-09-07.** Approved by the operator with
  the amendment in §3.2 (admin shows the cloud replica and labels it), and
  extended during implementation to cover hosting the config pull — see §8,
  which is the part that changed most between draft and landing.
- Supersedes nothing. Extends ADR-019 (procurement) and ADR-016/017 (menu and
  device config).
- Contracts **0.6.4 → 0.7.0**. All three changes are **additive**: no column
  changes meaning, no route changes shape, no existing consumer breaks.

---

## 1. Context

M6 Phase B builds three admin surfaces — menu and pricing, suppliers and pack
sizes, and the goods-receipt list. Two of the three have no API to build
against. The gap was found by inspecting the route tables before writing any
frontend code.

| Surface | What exists today | Gap |
|---|---|---|
| Suppliers & pack sizes | `GET/POST/PATCH /procurement/suppliers`, carrying the whole `supplier_item` price list in the bundle | **None. Fully served.** |
| Menu & pricing | `GET/POST /menu/items`, `GET/POST /menu/categories`, availability, stations | **No edit route.** `menu/repository.go:159` is a plain `INSERT` with no `ON CONFLICT`, so a price cannot be changed at all |
| Goods-receipt list | `POST /procurement/goods-receipts` (edge ingest only) | **No read route.** `GetGoodsReceiptNoteByID` exists in the repository but is used only for ingest idempotency; OpenAPI has `post` and nothing else under that path |

The freeze that prevented this landing earlier has lifted on schedule: *"Contracts
stay FROZEN at v0.6.4 through Phase A. 0.7.0 lands only after Phase A is green."*
Phase A closed 2026-09-07.

---

## 2. THE FINDING THAT MUST BE READ BEFORE THE REST: a cloud menu price change does not reach the till

**`pull_and_apply_config` has exactly one caller in this repository and it is a
test.**

```
edge/sync/src/config.rs:599      pub fn pull_and_apply_config(...)
edge/sync/tests/worker_integration.rs:436    <- the only caller
```

Nothing in `apps/pos/src-tauri` calls it. `AppState::drain_outbox` pumps the
general outbox, procurement and the two ranged stock streams; it never pulls
config. So the cloud→edge half of §50.1 is **built and unhosted**.

**The consequence for Phase B, stated plainly so no screen implies otherwise:**
a price edited in the admin console changes the cloud row and **the till keeps
selling at the old price, indefinitely.** There is no interval at which it
converges, because nothing asks.

This is the third instance of one pattern in this milestone — the mechanism
written, the caller missing, every test green:

- A5: `drain_outbox` existed with two callers, neither periodic.
- A7: the cloud ingest routes exist; `edge/sync/src/route.rs` maps two
  aggregates, so 78 rows can never be sent.
- This: the config pull exists; nothing hosts it.

It is also already half-recorded — `edge/sync` has no host outside a test
process is a standing backlog item — but recorded as an abstraction, not as
"menu prices do not propagate", which is the form a reader can act on.

### What this ADR requires because of it

1. **The admin menu screen MUST NOT imply the till has updated.** It shows the
   cloud row and says so, with the outlet's last-applied `config_version` and
   the time it was applied where that is known. The same rule ADR-019 §5 applies
   to PO receipt progress: **show both, label them, never reconcile them.**
2. **~~A backlog entry is filed for hosting the config pull~~ — SUPERSEDED.
   The pull is HOSTED in this ADR, see §8.** The draft deferred it as a
   separate §50.1 decision; the operator ruled that shipping a pricing screen
   whose edits reach nothing is worse than making the authority change here,
   and that the offline-safe default needs no decision. The draft's reasoning
   is kept above rather than deleted so the change of position is visible.
3. **This ADR does not claim price propagation.** It claims a price can be
   *edited centrally*. Anyone reading "menu and pricing shipped" as "prices
   reach tills" is reading something this document does not say.

---

## 3. Decision

### 3.1 `PATCH /menu/items/{itemId}` — edit a menu item

Menu is **cloud config, syncing down** (§50.1), so the write belongs on the
cloud and there is no authority question: the till never authors a menu item.

**Mutable field set — exhaustive. Anything not listed is immutable and is
rejected, not ignored:**

| Field | Mutable | Note |
|---|---|---|
| `name` | yes | |
| `base_price_paise` | yes | **integer paise**, never a decimal on the wire |
| `category_id` | yes | must resolve inside the same outlet |
| `is_available` | **no** | `POST /menu/items/{itemId}/availability` already owns this and is an EDGE→CLOUD replay route; a second writer is the §50.1 violation this table exists to avoid |
| `tax_profile_id` | yes | null falls back to the outlet default (0.4.2) |
| `hsn_sac` | yes | **may not be set to NULL or blank** — an invoice cannot issue without it (0.4.5) |
| `id`, `outlet_id`, `tenant_id` | **never** | present in the body is **400 `invalid_input`**, not a silent ignore. 400 rather than the 422 this ADR first proposed: `httpx` maps `ErrInvalidInput` that way for every route here, and the house convention wins over a new one for a single route. Observed 2026-09-07. |
| `config_version` | server-assigned | monotonic increment on every accepted PATCH, so `GET /sync/config`'s `since_version` filter reaches the edit |

**Tenant scoping is a 404, not a 403 and never a 200.** The lookup is
`WHERE id = $1 AND outlet_id = $2` with the outlet resolved from the
authenticated principal's tenant. A well-formed `itemId` belonging to another
tenant is indistinguishable from one that does not exist — the §74 rule that a
sequential or guessable identifier is never a security boundary, applied to the
response as well as to the id.

**`POST /menu/items` stays create-only.** `repository.go:159` is NOT turned into
an upsert to make PATCH easier: an upsert would make a duplicate create silently
succeed and overwrite, which is precisely the class of defect this milestone
spent A1 on. A duplicate POST raises `23505` and is a **409 through the shared
classifier** (`internal/platform/storage`).

**The write path goes through `storage.Wrap`** and is added to
`docs/m6-a1-sink-audit.md` as a **new sink with its own red-then-green** — the
FK on `category_id` is a `23503` waiting to happen, and A1's whole finding was
that unclassified sinks report client-data failures as 500.

### 3.2 `GET /procurement/goods-receipts` and `GET /procurement/goods-receipts/{grnId}`

A GRN is **edge-authoritative** (ADR-019). The cloud holds a **replica** of what
an outlet recorded, and the admin console must label it as such.

- **Tenant- and outlet-scoped**, from the authenticated principal. Same 404
  rule as above.
- **Paginated** — `limit` (default 50, max 200) and an opaque `cursor`. Receipts
  accumulate for the life of an outlet; an unbounded list is a slow outage.
- **Read-only.** No PATCH, no DELETE, no status transition. `goods_receipt_note`
  is an immutable snapshot of what arrived; a correction is a
  `purchase_return`, never an edit.
- **Returns the whole snapshot**, explicitly including:
  - every `grn_line` with `entered_quantity_micro`, `base_quantity_micro` and
    `pack_size_micro_applied` — **all three**, because ADR-019 §3 requires "what
    did they actually type?" to be answerable from the row, and a list that
    returns only the base quantity destroys exactly that;
  - `line_total_paise` (0.6.3) — the exact invoiced money the row is worth;
  - the nullable `purchase_order_id`, `supplier_id` and per-line
    `purchase_order_line_id`, **as nulls where they are null**. A GRN never
    blocks on a PO and the read path must not imply it did;
  - the associated **`grn_gap` rows**, on the detail route. A gap is the record
    of what could not be matched about *this* receipt and belongs beside it —
    the same reasoning that put both aggregate types on the one ingest route.
- **`GET` by id exists alongside the list** so M6 C6's field-by-field
  comparison is against **one whole record**, not a list entry that may carry a
  projection. A criterion compared against a summary proves the summary.

### 3.3 The error-code enum, in this bump and not the next

Already filed against 0.7.0 (`docs/backlog.md`, M6 A1 planning 2026-09-03) and
it lands here rather than at 0.7.1. Today OpenAPI types the error body as
`{ code: string, message: string }` with **no enumeration**, while the edge
branches on the string — so a cloud-side rename is a silent behaviour change no
drift test can see. A1 put `missing_reference` on the wire and the edge reads
it; this bump adds two more routes that can return codes.

Scope: enumerate every `code` the ingest and admin routes can return — at
minimum `missing_reference`, plus everything the A1 sink audit lists and the two
routes above — in OpenAPI, the Zod schema and the mirrored Go constants, **with
the edge's classifier reading the generated set rather than string literals.**
Leaving it to 0.7.1 means shipping two more `code`-returning routes onto an
unenumerated field, which is the position that made this necessary.

---

## 4. The OpenAPI diff, for review

Proposed, **not applied.** Paths only; schema additions elided for length but
enumerated beneath.

```yaml
  /menu/items/{itemId}:
    patch:
      summary: >
        Edit a menu item. CONFIG, cloud→edge (§50.1): the cloud is the only
        author of a menu item and the till never writes one.

        MUTABLE: name, base_price_paise, category_id, tax_profile_id, hsn_sac.
        IMMUTABLE AND 422 IF PRESENT: id, outlet_id, tenant_id, is_available.
        is_available is excluded deliberately — POST /menu/items/{itemId}/
        availability already owns it and is an EDGE→CLOUD replay route, so
        accepting it here would create the second writer §50.1 forbids.

        hsn_sac MAY NOT be set to NULL or blank: an invoice cannot issue
        without it (0.4.5), so clearing it here would break billing at every
        till that later syncs the row.

        config_version is server-assigned and increments on every accepted
        PATCH, so GET /sync/config's since_version filter reaches the edit.

        A NOTE THE ADMIN UI MUST HONOUR: nothing hosts the edge's config pull
        today (ADR-024 §2), so an accepted PATCH changes the cloud row and does
        NOT change what any till sells. Do not render this as "updated".
      parameters:
        - name: itemId
          in: path
          required: true
          schema: { type: string, format: uuid }
      requestBody:
        required: true
        content:
          application/json:
            schema: { $ref: "#/components/schemas/MenuItemPatch" }
      responses:
        "200": { description: The updated item. }
        "404":
          description: >
            No such item IN THIS TENANT. A well-formed itemId belonging to
            another tenant returns 404, never 403 and never 200 — an id is not
            a security boundary (§74).
        "409": { description: Duplicate, via the shared SQLSTATE classifier. }
        "422": { description: Immutable field present, or a blank hsn_sac. }

  /procurement/goods-receipts:
    get:
      summary: >
        List goods receipts for an outlet. READ-ONLY REPLICA.

        A GRN is EDGE-AUTHORITATIVE (ADR-019): this is the cloud's copy of what
        an outlet recorded, and any surface rendering it must label it as the
        replica. It is not the outlet's live view and the two may legitimately
        differ, exactly as PO receipt progress does.

        Returns the immutable snapshot: every grn_line with entered_quantity_
        micro, base_quantity_micro AND pack_size_micro_applied — all three,
        because "what did they actually type?" must stay answerable from the
        row — plus line_total_paise.

        purchase_order_id, supplier_id and purchase_order_line_id are returned
        AS NULL where they are null. A GRN never blocks on a PO and this route
        must not imply otherwise.
      parameters:
        - { name: outlet_id, in: query, required: true, schema: { type: string, format: uuid } }
        - { name: limit, in: query, schema: { type: integer, default: 50, maximum: 200 } }
        - { name: cursor, in: query, schema: { type: string } }
      responses:
        "200": { description: A page of receipts, with next_cursor. }

  /procurement/goods-receipts/{grnId}:
    get:
      summary: >
        One goods receipt with its lines AND its grn_gap rows. A gap is the
        record of what could not be matched about THIS receipt and belongs
        beside it — the same reason both aggregate types share the ingest
        route.

        Exists alongside the list so a field-by-field comparison (M6 C6) is
        against one whole record rather than a list entry that may carry a
        projection. A criterion compared against a summary proves the summary.
      responses:
        "200": { description: The receipt, its lines, and its gaps. }
        "404": { description: No such receipt IN THIS TENANT. }
```

**Schema additions:** `MenuItemPatch` (the five mutable fields, all optional,
`additionalProperties: false` so an immutable field is a 422 rather than a
silent ignore); `GoodsReceiptNoteRead` (note + lines + nullable provenance);
`GoodsReceiptPage` (`items` + `next_cursor`); `GoodsReceiptDetail`
(`GoodsReceiptNoteRead` + `gaps`); and `ErrorCode`, the enum from §3.3, which
`ErrorResponse.code` narrows to.

---

## 5. Verification

**Persistence round-trip for the GET, per the condition set:** ingest a GRN
through the existing `POST /procurement/goods-receipts`, read it back through
the new `GET /procurement/goods-receipts/{grnId}`, re-serialise, and
**byte-compare against the fixture**. Deferred or absent fields are pinned by
**exact assertion**, never by absence.

This is contracts 0.5.9's lesson applied before the fact rather than after: *a
fidelity test proves fidelity only for the fields its fixture populates*, and
that hole stayed green for four versions because the fixture was a wastage entry
on which every provenance field is legitimately null. **The fixture here must
populate every provenance group** — a receipt WITH a PO and supplier, a receipt
with NEITHER, and a line carrying `pack_size_micro_applied` — or the same hole
reopens under a new field's name.

**Red-then-green on the PATCH sink**, as A1 requires of every new
`httpx.Error`-returning path: a `category_id` that does not exist must be
watched producing a 500 before the classifier maps it, then a 4xx after.

**Evidence separates Executed from Read-verified**, as the verifier rubric
requires. Nothing in this ADR is evidenced by a test harness alone where a route
is claimed to work end to end.

---

## 6. Self-review against the contract rubric

| Rule | Finding |
|---|---|
| IDs app-generated UUIDv7/ULID, never DB-side random defaults | **Pass.** No new identifier is minted; PATCH addresses an existing row and both GETs are read-only |
| No nullable columns in primary keys | **Pass.** No key changes |
| Every aggregate single-authority per §50.1 | **Pass, and it is the reason for two of the decisions.** Menu is cloud-authored so PATCH is cloud-side; `is_available` is excluded because the availability route is edge→cloud and accepting it on PATCH would create a second writer. GRN stays edge-authoritative and the new routes are read-only |
| No credential material in audit values, logs or wire types | **Pass.** Neither route touches `password_hash`, `pin_hash`, `token_hash` or `device_token_hash` |
| Uniqueness constraints tenant-scoped, not global | **Pass.** No new constraint; both routes scope their lookup by tenant and return 404 across the boundary |
| Additive change to a frozen contract needs a version bump + ADR | **This document, plus 0.6.4 → 0.7.0** |
| An additive change has a consumer list | **Partially satisfied, and the gap is named.** `MenuItemPatch` and the GRN read shapes need the Go structs, the Zod schema, the OpenAPI shape and the repository SELECT before they are landed. `ErrorCode` has a consumer that must change in the same bump — the edge classifier must read the generated set instead of string literals, or the enum is a column nothing reads |

**One rubric item I cannot mark Pass:** the consumer list above is a commitment,
not an observation. 0.5.2 and 0.5.9 both shipped a column nothing read, and both
looked complete at this stage of the argument. The enum in particular is worth
nothing until the edge classifier consumes it.

---

## 8. Hosting the config pull — added during implementation

**From 0.7.0 the cloud is the menu authority in practice, not only on paper.**
`pull_and_apply_config` is now called by `AppState::drain_outbox`, which the A5
periodic loop already drives, so a till converges on the cloud's menu at its
pump interval instead of never.

Three properties, each chosen rather than inherited:

- **The pull runs INSIDE the same database lock as the outbox pump**, in the
  same `drain_outbox` call and before it. A config apply rewrites menu, tax and
  user rows while a pump reads and writes the outbox; the two interleaving on
  one SQLite connection is the kind of fault that surfaces as a corrupt read
  once a month and is never reproduced.
- **A failed pull is logged and nothing else.** The till keeps its last applied
  config and carries on selling. An outlet with no uplink is the normal case
  (ADR-013), so surfacing an error would turn the expected condition into an
  alarm, and refusing to continue would stop a shop trading because head office
  was unreachable. This is the offline-safe default and it needed no decision.
- **The stop flag is re-checked before the pull**, so a pump that wakes during
  shutdown cannot apply a bundle into a database that is being sealed.

**APPLY MERGES, IT DOES NOT PRUNE — and this was checked rather than assumed.**
`repo::upsert_menu_item` is an `INSERT ... ON CONFLICT(id) DO UPDATE` guarded by
`excluded.config_version >= menu_item.config_version`, and nothing deletes rows
absent from the bundle. Two consequences:

1. **The dev seed's extra items survive a pull.** The edge seeds 39 items and
   the cloud seeds 2; after a pull the edge still has 39. The drift that served
   as M6 C7's stimulus is therefore still in place — which no longer matters,
   because **C7 is closed** (observed 2026-09-07), but a reader expecting the
   drift to vanish should know it does not.
2. **A menu item DELETED in the cloud is never removed from an edge.** There is
   no tombstone and no prune, so a withdrawn dish stays sellable at every till
   that ever saw it. Filed, trigger *before the first pilot*. It is a real gap
   and it is not this ADR's to fix: deletion semantics for cloud→edge config is
   its own decision, and doing it as a rider on a route addition is how the
   wrong default gets set.

---

## 7. What this ADR deliberately does not do

- **It does not add PO or staff routes.** Both admin surfaces are deferred from
  Phase B and filed.
- **It does not make `POST /menu/items` an upsert.**
- **It does not add a GRN write, status or correction route.** A receipt is
  corrected by an appended `purchase_return`, never by an edit.
