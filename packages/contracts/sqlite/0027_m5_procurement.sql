-- Holler Edge SQLite — procurement. Contracts 0.6.0, ADR-019.
--
-- ============================================================================
-- THE RULE THIS FILE EXISTS TO ENFORCE
-- ============================================================================
--
-- A GRN NEVER BLOCKS ON A PURCHASE ORDER.
--
-- Goods arrive against a PO that never synced to this outlet, against a PO
-- amended after dispatch, and with no PO at all. Every one of those cases
-- records a gap and ACCEPTS THE RECEIPT.
--
-- This is M4's "stock never blocks a sale" and "a missing or broken recipe
-- never fails a confirm", generalised to the inbound side, and it is the same
-- argument: refusing a delivery standing in the kitchen doorway because a row
-- is missing is the outage, not the protection. The driver is holding the
-- crate. The gap is a report; the refusal would be a business stoppage.
--
-- Structurally: goods_receipt_note.purchase_order_id is NULLABLE, grn_line
-- .purchase_order_line_id is NULLABLE, and there is NO CHECK anywhere tying a
-- receipt to an order. That absence is deliberate and must not be "tidied up".
--
-- ============================================================================
-- AUTHORITY SPLIT (§50.1, the ADR-011/014/016/018 line)
-- ============================================================================
--
--   CONFIG, cloud->edge   supplier (aggregate), purchase_order (aggregate)
--   CHILD ROWS            supplier_item, purchase_order_line — travel inside
--                         their parent's config bundle. Not aggregates, no
--                         sync direction, ever.
--   EDGE-AUTHORITATIVE    goods_receipt_note, purchase_return,
--   edge->cloud           stock_transfer_out, grn_gap
--   CHILD ROWS            grn_line, purchase_return_line, stock_transfer_line
--   EDGE-LOCAL            grn_sequence — SQLite only, no Postgres mirror, no
--                         AggregateType, no sync direction, EVER. It ships as
--                         its OWN migration (0028_grn_sequence.sql), not in
--                         this file, so that SINGLE_STORE_MIGRATIONS can
--                         actually see it — that lint pairs files by stem, so
--                         a single-store table hidden inside a mirrored file
--                         is undeclarable and unchecked.
--   CLOUD-ONLY            supplier_invoice, supplier_credit and
--                         role.po_approval_limit_paise — Postgres only, and
--                         likewise in their own migration
--                         (postgres/0029_supplier_accounts.sql) for the same
--                         reason. Deliberately not AggregateTypes.
--
-- ---------------------------------------------------------------------------
-- WHY purchase_order CARRIES NO RECEIPT STATE
-- ---------------------------------------------------------------------------
--
-- The obvious modelling — purchase_order.status including PARTIALLY_RECEIVED
-- and CLOSED-on-receipt — is EXACTLY the half-config, half-transaction row
-- ADR-011 forbids. Receiving happens at the edge; the PO is a cloud-owned
-- config row. A status column the edge writes makes the outlet a second
-- writer of a cloud aggregate, which §50.1 exists to prevent, and it is
-- unresolvable on replay when two outlets receive against one PO.
--
-- So purchase_order.status carries ONLY cloud transitions (DRAFT ->
-- PENDING_APPROVAL -> APPROVED -> SENT, or -> CANCELLED, plus a manual
-- CLOSED). Receipt progress is DERIVED — at the cloud from replayed GRN
-- lines, at the edge from local ones. Nobody writes it anywhere.
--
-- The same reasoning as invoice_series (cloud config) versus invoice_sequence
-- (edge-local counter): the definition and the running total are different
-- kinds of thing and do not belong on one row.

-- ---------------------------------------------------------------------------
-- supplier — AGGREGATE, cloud->edge
-- ---------------------------------------------------------------------------
CREATE TABLE supplier (
    id                  TEXT PRIMARY KEY,       -- UUIDv7, app-generated (§74). Never a DB-side default.
    outlet_id           TEXT NOT NULL REFERENCES outlet(id),
    code                TEXT NOT NULL,
    name                TEXT NOT NULL,

    -- A supplier's GSTIN appears on the purchase side of a GST return. It is
    -- nullable because unregistered suppliers are ordinary in this market;
    -- it is NOT validated into existence by a fallback, for the same reason
    -- menu_item.hsn_sac has none.
    gstin               TEXT,

    phone               TEXT,
    email               TEXT,
    address             TEXT,

    -- Credit terms. Consumed by supplier_invoice due-date derivation (M7);
    -- surfaced read-only on the supplier screen in M5 (T5).
    payment_terms_days  INTEGER NOT NULL DEFAULT 0 CHECK (payment_terms_days >= 0),

    is_active           INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    config_version      INTEGER NOT NULL,

    -- Tenant-scoped, never global (contract rubric). Two outlets may both
    -- have a supplier coded VEG-01 and they are different companies.
    UNIQUE (outlet_id, code)
);

CREATE INDEX idx_supplier_outlet ON supplier(outlet_id, is_active);

-- ---------------------------------------------------------------------------
-- supplier_item — CHILD ROW of supplier. Not an aggregate.
-- ---------------------------------------------------------------------------
--
-- What this supplier sells, in the unit THEY sell it in. This is the row that
-- makes receiving in the supplier's units possible, and it is the row that
-- makes it dangerous — see the dimension rule below.
CREATE TABLE supplier_item (
    id                  TEXT PRIMARY KEY,
    supplier_id         TEXT NOT NULL REFERENCES supplier(id) ON DELETE CASCADE,
    inventory_item_id   TEXT NOT NULL REFERENCES inventory_item(id),

    -- The supplier's own unit: 'SACK', 'CRATE', 'CARTON', 'TIN', 'KG', 'L'.
    -- Free text on purpose — it is a label on a delivery note, not an enum
    -- this product gets to define. The CONVERSION is what must be exact.
    purchase_unit       TEXT NOT NULL,

    -- How much base-dimension quantity one purchase_unit contains, in integer
    -- micro-units (0015's rule: canonical unit x 10^6, scale in the name).
    -- One 50 kg sack = 50_000_000_000.
    pack_size_micro     INTEGER NOT NULL CHECK (pack_size_micro > 0),

    -- THE UNIT THE AUTHOR CHOSE, NEVER DERIVED FROM THE REFERENT.
    --
    -- This is contracts 0.5.2's rule (recipe_ingredient.quantity_dimension)
    -- applied to the purchase side, and it is here for the identical reason:
    -- without it, pack_size_micro is dimensionless in storage, and
    -- reclassifying an inventory_item from MASS to COUNT silently
    -- reinterprets every pack size against it.
    --
    -- IF A WRITE PATH OR UI AUTO-FILLS THIS FROM inventory_item.dimension,
    -- THE COMPARISON BECOMES x == x AND THE GUARD CAN NEVER FIRE — and it
    -- will look correct in review. The cloud rejects a mismatch at write
    -- time; the edge degrades to a DIMENSION_MISMATCH gap and still accepts
    -- the receipt.
    quantity_dimension  TEXT NOT NULL CHECK (quantity_dimension IN ('MASS','VOLUME','COUNT')),

    -- Last price seen, in paise per purchase_unit. Advisory: it prefills a PO
    -- line and is never the price a GRN posts. The GRN carries its own cost,
    -- because what was invoiced is a fact and what was expected is a guess.
    last_price_paise    INTEGER CHECK (last_price_paise IS NULL OR last_price_paise >= 0),

    is_preferred        INTEGER NOT NULL DEFAULT 0 CHECK (is_preferred IN (0, 1)),

    UNIQUE (supplier_id, inventory_item_id, purchase_unit)
);

CREATE INDEX idx_supplier_item_inventory ON supplier_item(inventory_item_id);

-- ---------------------------------------------------------------------------
-- purchase_order — AGGREGATE, cloud->edge. Read-only at the edge.
-- ---------------------------------------------------------------------------
CREATE TABLE purchase_order (
    id                  TEXT PRIMARY KEY,
    outlet_id           TEXT NOT NULL REFERENCES outlet(id),
    supplier_id         TEXT NOT NULL REFERENCES supplier(id),

    -- Issued by the cloud, which is the only writer. Distinct from
    -- grn_sequence, which is edge-local — the two numbers are minted by
    -- different authorities and must never share a counter.
    po_number           TEXT NOT NULL,

    -- CLOUD TRANSITIONS ONLY. No receipt state — see the header. The edge
    -- never writes this column and no edge code path may.
    status              TEXT NOT NULL CHECK (status IN
                            ('DRAFT','PENDING_APPROVAL','APPROVED','SENT','CANCELLED','CLOSED')),

    expected_date       TEXT,                   -- ISO date, outlet-local business date
    notes               TEXT,

    -- Integer paise, like all money (CLAUDE.md). Sum of its lines, computed
    -- cloud-side at write time; the edge formats it and never recomputes it.
    total_paise         INTEGER NOT NULL DEFAULT 0 CHECK (total_paise >= 0),

    -- Approval provenance. Both NULL until approved, both set together.
    -- role.po_approval_limit_paise is what gates the transition (T1).
    approved_by_user_id TEXT REFERENCES app_user(id),
    approved_at         TEXT,

    created_at          TEXT NOT NULL,
    config_version      INTEGER NOT NULL,

    UNIQUE (outlet_id, po_number),

    -- An approval is both fields or neither. A half-recorded approval is how
    -- "who authorised this spend" becomes unanswerable.
    CHECK ((approved_by_user_id IS NULL) = (approved_at IS NULL)),

    -- APPROVED, SENT and CLOSED are unreachable without an approval on record.
    CHECK (status NOT IN ('APPROVED','SENT','CLOSED') OR approved_by_user_id IS NOT NULL)
);

CREATE INDEX idx_purchase_order_outlet ON purchase_order(outlet_id, status);
CREATE INDEX idx_purchase_order_supplier ON purchase_order(supplier_id);

-- ---------------------------------------------------------------------------
-- purchase_order_line — CHILD ROW of purchase_order. Not an aggregate.
-- ---------------------------------------------------------------------------
CREATE TABLE purchase_order_line (
    id                      TEXT PRIMARY KEY,
    purchase_order_id       TEXT NOT NULL REFERENCES purchase_order(id) ON DELETE CASCADE,
    inventory_item_id       TEXT NOT NULL REFERENCES inventory_item(id),
    line_number             INTEGER NOT NULL,

    purchase_unit           TEXT NOT NULL,
    ordered_quantity_micro  INTEGER NOT NULL CHECK (ordered_quantity_micro > 0),

    -- 0.5.2's rule again. Never auto-filled from inventory_item.dimension.
    quantity_dimension      TEXT NOT NULL CHECK (quantity_dimension IN ('MASS','VOLUME','COUNT')),

    unit_price_paise        INTEGER NOT NULL CHECK (unit_price_paise >= 0),
    line_total_paise        INTEGER NOT NULL CHECK (line_total_paise >= 0),

    UNIQUE (purchase_order_id, line_number)
);

CREATE INDEX idx_po_line_item ON purchase_order_line(inventory_item_id);

-- ---------------------------------------------------------------------------
-- goods_receipt_note — AGGREGATE, edge->cloud. IMMUTABLE once written.
-- ---------------------------------------------------------------------------
--
-- The outlet receives goods with the uplink down and the cloud replays. Same
-- split as invoice / payment / cash_shift (ADR-016).
--
-- IMMUTABLE: a receipt is corrected by an appended purchase_return or a
-- COUNT_ADJUSTMENT, never by mutating the record of what arrived. The
-- trigger below enforces it — migrations.rs lints every IMMUTABLE claim in a
-- contract migration for a trigger behind it, because "APPEND-ONLY" sat in a
-- comment on `payment` with nothing behind it for two milestones.
CREATE TABLE goods_receipt_note (
    id                  TEXT PRIMARY KEY,       -- UUIDv7, minted at the edge
    outlet_id           TEXT NOT NULL REFERENCES outlet(id),

    -- NULLABLE, AND THIS IS THE POINT OF THE FILE. Goods with no PO are
    -- received; a gap is recorded. See the header.
    purchase_order_id   TEXT REFERENCES purchase_order(id),

    -- Also nullable: an unknown supplier is a gap, not a refusal. A crate on
    -- the doorstep from a supplier nobody has configured still contains food
    -- that is going into the walk-in whether this product likes it or not.
    supplier_id         TEXT REFERENCES supplier(id),

    -- Minted from grn_sequence, edge-local. Never leaves as a counter; only
    -- the issued number travels. The invoice_sequence precedent exactly.
    grn_number          TEXT NOT NULL,

    -- The supplier's own document reference, off the delivery note. Free
    -- text, no format assumed, no uniqueness — it is their number, not ours.
    delivery_note_ref   TEXT,

    received_at         TEXT NOT NULL,          -- ISO8601 UTC
    received_by_user_id TEXT NOT NULL REFERENCES app_user(id),

    -- Outlet-local business day, computed by compute_business_date() from
    -- outlet.timezone and outlet.day_start_time (contracts 0.5.0). NOT the
    -- first ten characters of a UTC instant — see docs/m5-planning.md §1.2
    -- for what that shortcut costs on the billing side.
    business_date       TEXT NOT NULL,

    notes               TEXT,
    schema_version      INTEGER NOT NULL DEFAULT 1,

    UNIQUE (outlet_id, grn_number)
);

CREATE INDEX idx_grn_outlet_date ON goods_receipt_note(outlet_id, business_date);
CREATE INDEX idx_grn_po ON goods_receipt_note(purchase_order_id);

CREATE TRIGGER goods_receipt_note_immutable
BEFORE UPDATE ON goods_receipt_note
BEGIN
    SELECT RAISE(ABORT, 'goods_receipt_note is IMMUTABLE: correct a receipt with a purchase_return or a COUNT_ADJUSTMENT, never by mutating it');
END;

CREATE TRIGGER goods_receipt_note_no_delete
BEFORE DELETE ON goods_receipt_note
BEGIN
    SELECT RAISE(ABORT, 'goods_receipt_note is IMMUTABLE: rows are never deleted');
END;

-- ---------------------------------------------------------------------------
-- grn_line — CHILD ROW of goods_receipt_note. Not an aggregate.
-- ---------------------------------------------------------------------------
--
-- THE CONVERSION HAPPENS ONCE, AT THE EDGE, AND BOTH SIDES ARE STORED.
--
-- entered_quantity_micro is what the human typed, in entered_purchase_unit.
-- base_quantity_micro is what the ledger receives. Storing both makes the
-- conversion auditable after the fact — when a receipt turns out to be 1000x
-- wrong, the question "what did they actually type?" must be answerable from
-- the row, not reconstructed from a rate that may since have changed.
--
-- Receiving is the THIRD quantity-entry path in this product and the one with
-- the worst odds: larger quantities than a stock count, read off a delivery
-- note written in the SUPPLIER's units, entered by someone reconciling
-- against a document rather than counting a shelf (docs/M5_HANDOFF.md §2.1).
CREATE TABLE grn_line (
    id                          TEXT PRIMARY KEY,
    grn_id                      TEXT NOT NULL REFERENCES goods_receipt_note(id) ON DELETE CASCADE,
    inventory_item_id           TEXT NOT NULL REFERENCES inventory_item(id),
    line_number                 INTEGER NOT NULL,

    -- NULLABLE: a line with no matching PO line is received and gapped.
    purchase_order_line_id      TEXT REFERENCES purchase_order_line(id),

    entered_purchase_unit       TEXT NOT NULL,
    entered_quantity_micro      INTEGER NOT NULL CHECK (entered_quantity_micro > 0),

    -- 0.5.2's rule, third instance. Never auto-filled from the referent.
    quantity_dimension          TEXT NOT NULL CHECK (quantity_dimension IN ('MASS','VOLUME','COUNT')),

    -- The converted figure the stock_ledger_entry carries. Equal to
    -- entered_quantity_micro when the purchase unit IS the base unit.
    base_quantity_micro         INTEGER NOT NULL CHECK (base_quantity_micro > 0),

    -- The rate used, snapshotted. A supplier_item pack size may be edited
    -- later; this receipt's arithmetic must stay reproducible regardless.
    pack_size_micro_applied     INTEGER NOT NULL CHECK (pack_size_micro_applied > 0),

    -- Cost per BASE unit, integer paise. This is the field that finally
    -- consumes stock_ledger_entry.unit_cost_paise and lets weighted average
    -- cost be derived (ADR-018 deferred it here explicitly).
    unit_cost_paise             INTEGER NOT NULL CHECK (unit_cost_paise >= 0),
    line_total_paise            INTEGER NOT NULL CHECK (line_total_paise >= 0),

    -- Batch and expiry: MODELLED NOW, ALERTED IN M6.
    --
    -- BATCH IDENTITY IS CAPTURED AT RECEIPT OR NEVER. You cannot retrofit
    -- which crate a chicken came out of, so unlike almost every other
    -- deferred field these cannot wait for their consumer without losing the
    -- data permanently — the same argument that landed source_stock_count_id
    -- immediately rather than at 0.6.0: a ledger entry is never rewritten, so a
    -- field absent at insert is absent forever.
    --
    -- Nothing reads them in M5. Both are declared in
    -- scripts/check-contract-field-consumers.mjs EXEMPT with M6 named, the
    -- yield_factor_ppm precedent, and BOTH EXEMPTIONS COME OUT when M6's
    -- expiry alerting lands. An exemption that outlives its reason is a
    -- silenced failure.
    batch_code                  TEXT,
    expiry_date                 TEXT,

    UNIQUE (grn_id, line_number)
);

CREATE INDEX idx_grn_line_item ON grn_line(inventory_item_id);
CREATE INDEX idx_grn_line_expiry ON grn_line(expiry_date) WHERE expiry_date IS NOT NULL;

-- ---------------------------------------------------------------------------
-- grn_gap — AGGREGATE, edge->cloud. The other half of "never blocks".
-- ---------------------------------------------------------------------------
--
-- The stock_deduction_gap precedent (0.5.0), inbound. A rule that says "accept
-- and record" is worth nothing without the row that records, and worth little
-- without a screen that shows it — M5 acceptance criterion 3 requires the gap
-- to be VISIBLE TO A HUMAN ON THE POS, not merely present in a table.
--
-- ---------------------------------------------------------------------------
-- PLAIN ENVELOPE OUTBOX, NOT A RANGED STREAM. Deliberate, and the reasoning
-- is ADR-018's own transport rule applied to itself rather than copied.
--
-- stock_deduction_gap earned entry_seq and a private counter (0.5.8) because
-- it is a HIGH-VOLUME STREAM: one row per unresolvable line per sale, all day.
-- Contiguity marks, per-stream cursors, ledger_replay_gap healing and a
-- per-entry retry budget all exist to stop one bad row wedging thousands
-- behind it.
--
-- A grn_gap is not that. It is a DISCRETE EVENT A BUYER ACTS ON — a handful a
-- week, each one a question for a human ("why did this arrive with no PO?").
-- Giving it ranged-sync machinery would buy contiguity guarantees nothing
-- needs and cost T3 a second cursor pair, a second counter, and a second
-- retry budget to maintain and to get wrong.
--
-- So: NO entry_seq, NO grn_gap_sequence, ordinary envelope outbox. If gap
-- volume ever turns out to be stream-shaped, promoting it is a migration —
-- and the 0.5.8 trap applies then: NOT NULL on a populated table needs
-- rebuild-and-backfill, never a constant default under a UNIQUE key.
-- ---------------------------------------------------------------------------
CREATE TABLE grn_gap (
    id                  TEXT PRIMARY KEY,
    outlet_id           TEXT NOT NULL REFERENCES outlet(id),
    grn_id              TEXT NOT NULL REFERENCES goods_receipt_note(id),
    grn_line_id         TEXT REFERENCES grn_line(id),
    inventory_item_id   TEXT REFERENCES inventory_item(id),

    reason              TEXT NOT NULL CHECK (reason IN (
                            'NO_PURCHASE_ORDER',        -- received with no PO at all
                            'PURCHASE_ORDER_NOT_FOUND', -- PO referenced but never synced to this edge
                            'PO_LINE_NOT_FOUND',        -- item received that the PO does not list
                            'QUANTITY_EXCEEDS_ORDERED', -- over-delivery; accepted, flagged
                            'NO_SUPPLIER_ITEM',         -- no supplier_item row for this item+unit
                            'NO_UNIT_CONVERSION',       -- purchase unit not convertible to base
                            'DIMENSION_MISMATCH',       -- entered dimension != item dimension
                            'SUPPLIER_NOT_FOUND'        -- delivery from an unconfigured supplier
                        )),

    detail              TEXT,                   -- human-readable, for the POS screen
    occurred_at         TEXT NOT NULL,
    business_date       TEXT NOT NULL,

    schema_version      INTEGER NOT NULL DEFAULT 1
);

CREATE INDEX idx_grn_gap_unresolved ON grn_gap(outlet_id, business_date);

-- ---------------------------------------------------------------------------
-- purchase_return — AGGREGATE, edge->cloud. + purchase_return_line child.
-- ---------------------------------------------------------------------------
--
-- Posts RETURN_TO_VENDOR ledger entries. One of the three previously-dead
-- entry_type CHECK branches this milestone finally writes (docs/m5-planning.md
-- §1.4); TRANSFER_IN and the two PRODUCTION_* values stay dead and are
-- declared EXEMPT with M8 named.
CREATE TABLE purchase_return (
    id                  TEXT PRIMARY KEY,
    outlet_id           TEXT NOT NULL REFERENCES outlet(id),
    supplier_id         TEXT REFERENCES supplier(id),
    grn_id              TEXT REFERENCES goods_receipt_note(id),   -- nullable: same rule as above
    return_number       TEXT NOT NULL,
    reason              TEXT NOT NULL CHECK (reason IN
                            ('DAMAGED','EXPIRED','WRONG_ITEM','QUALITY','OVER_DELIVERY','OTHER')),
    returned_at         TEXT NOT NULL,
    returned_by_user_id TEXT NOT NULL REFERENCES app_user(id),
    business_date       TEXT NOT NULL,
    notes               TEXT,
    schema_version      INTEGER NOT NULL DEFAULT 1,
    UNIQUE (outlet_id, return_number)
);

CREATE TRIGGER purchase_return_immutable
BEFORE UPDATE ON purchase_return
BEGIN
    SELECT RAISE(ABORT, 'purchase_return is IMMUTABLE: append a correcting movement, never mutate');
END;

-- SQLite needs a SECOND trigger for DELETE; Postgres says BEFORE UPDATE OR
-- DELETE in one. Written out here because the mirrored file guards both and a
-- guarantee enforced on one side only is the exact defect 0.5.0 closed on
-- payment, audit_event and cash_movement.
CREATE TRIGGER purchase_return_no_delete
BEFORE DELETE ON purchase_return
BEGIN
    SELECT RAISE(ABORT, 'purchase_return is IMMUTABLE: rows are never deleted');
END;

CREATE TABLE purchase_return_line (
    id                      TEXT PRIMARY KEY,
    purchase_return_id      TEXT NOT NULL REFERENCES purchase_return(id) ON DELETE CASCADE,
    inventory_item_id       TEXT NOT NULL REFERENCES inventory_item(id),
    grn_line_id             TEXT REFERENCES grn_line(id),
    line_number             INTEGER NOT NULL,
    entered_purchase_unit   TEXT NOT NULL,
    entered_quantity_micro  INTEGER NOT NULL CHECK (entered_quantity_micro > 0),
    quantity_dimension      TEXT NOT NULL CHECK (quantity_dimension IN ('MASS','VOLUME','COUNT')),
    base_quantity_micro     INTEGER NOT NULL CHECK (base_quantity_micro > 0),
    unit_cost_paise         INTEGER NOT NULL CHECK (unit_cost_paise >= 0),
    UNIQUE (purchase_return_id, line_number)
);

-- ---------------------------------------------------------------------------
-- stock_transfer_out — AGGREGATE, edge->cloud. OUTBOUND HALF ONLY (M5).
-- ---------------------------------------------------------------------------
--
-- Posts TRANSFER_OUT ledger entries at the SOURCE outlet. The destination
-- receipt (TRANSFER_IN) and goods-in-transit reconciliation are M8, with the
-- rest of multi-outlet: a transfer spans two edge databases, which is
-- multi-outlet machinery and not something to half-build here.
--
-- destination_outlet_id is recorded now so M8 has the link it needs and no
-- migration has to find it later. It is READ BY THE CLOUD in M5 (the transfer
-- list), so it is not an unconsumed field.
CREATE TABLE stock_transfer_out (
    id                      TEXT PRIMARY KEY,
    outlet_id               TEXT NOT NULL REFERENCES outlet(id),   -- source
    destination_outlet_id   TEXT NOT NULL,                          -- may not exist in THIS edge db
    transfer_number         TEXT NOT NULL,
    dispatched_at           TEXT NOT NULL,
    dispatched_by_user_id   TEXT NOT NULL REFERENCES app_user(id),
    business_date           TEXT NOT NULL,
    notes                   TEXT,
    schema_version          INTEGER NOT NULL DEFAULT 1,
    UNIQUE (outlet_id, transfer_number),
    CHECK (destination_outlet_id <> outlet_id)
);

CREATE TRIGGER stock_transfer_out_immutable
BEFORE UPDATE ON stock_transfer_out
BEGIN
    SELECT RAISE(ABORT, 'stock_transfer_out is IMMUTABLE: a dispatch that was wrong is corrected by a return movement');
END;

CREATE TRIGGER stock_transfer_out_no_delete
BEFORE DELETE ON stock_transfer_out
BEGIN
    SELECT RAISE(ABORT, 'stock_transfer_out is IMMUTABLE: rows are never deleted');
END;

CREATE TABLE stock_transfer_line (
    id                      TEXT PRIMARY KEY,
    stock_transfer_out_id   TEXT NOT NULL REFERENCES stock_transfer_out(id) ON DELETE CASCADE,
    inventory_item_id       TEXT NOT NULL REFERENCES inventory_item(id),
    line_number             INTEGER NOT NULL,
    base_quantity_micro     INTEGER NOT NULL CHECK (base_quantity_micro > 0),
    quantity_dimension      TEXT NOT NULL CHECK (quantity_dimension IN ('MASS','VOLUME','COUNT')),
    unit_cost_paise         INTEGER NOT NULL CHECK (unit_cost_paise >= 0),
    UNIQUE (stock_transfer_out_id, line_number)
);

-- ---------------------------------------------------------------------------
-- stock_ledger_entry — provenance for the three new sources
-- ---------------------------------------------------------------------------
--
-- The source_stock_count_id precedent (0.5.5/0.5.9), which cost a silent
-- NULL column in Postgres for every row because the cloud never learned the
-- field existed. THREE separate columns rather than one polymorphic pair,
-- matching what is already there.
--
-- ADDING COLUMNS TO A HIGH-VOLUME TABLE, DELIBERATELY. 0015 warns against
-- exactly this — "adding a column to a multi-million-row table on a spinning
-- disk, at an outlet, during an upgrade, is not an operation this product
-- should ever have to perform." That warning is about NOT NULL adds, which
-- need rebuild-and-backfill (the 0.5.8 trap). A NULLABLE ADD COLUMN is
-- metadata-only in SQLite and does not rewrite the table, so it is safe here
-- and the distinction is the whole reason these are nullable.
--
-- TRANSFER_IN provenance is NOT pre-added. It has no consumer until M8 and
-- would be a column nothing reads — the rule this milestone's version was
-- cut three fields to honour.
ALTER TABLE stock_ledger_entry ADD COLUMN source_grn_id TEXT REFERENCES goods_receipt_note(id);
ALTER TABLE stock_ledger_entry ADD COLUMN source_purchase_return_id TEXT REFERENCES purchase_return(id);
ALTER TABLE stock_ledger_entry ADD COLUMN source_stock_transfer_out_id TEXT REFERENCES stock_transfer_out(id);

-- ---------------------------------------------------------------------------
-- NOT HERE: role.po_approval_limit_paise
-- ---------------------------------------------------------------------------
--
-- THERE IS NO `role` TABLE IN THIS STORE. The edge does not model roles at
-- all — it flattens the resolved permission list into
-- app_user.permissions_json per outlet (0002), replaced wholesale per
-- config_version. Discovered while checking that a money limit sits on a
-- tenant-scoped row; `role` is tenant-scoped in PostgreSQL
-- (UNIQUE (tenant_id, code)) and simply absent here.
--
-- That asymmetry is correct rather than merely tolerated: PO approval happens
-- in the admin, against the cloud, by someone who is not standing at a till.
-- The edge never approves a purchase order and must never be able to, so it
-- has no use for the limit and giving it one would only create a second place
-- for the number to be wrong.
--
-- The column therefore lands in postgres/0029_supplier_accounts.sql only,
-- declared in SINGLE_STORE_MIGRATIONS with this reason.
