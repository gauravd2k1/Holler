-- Holler Cloud PostgreSQL — procurement. Contracts 0.6.0, ADR-019.
--
-- Mirror of sqlite/0027_m5_procurement.sql. Read that file first: it carries
-- the reasoning, and this one carries only what differs.
--
-- The three CLOUD-ONLY shapes (supplier_invoice, supplier_credit,
-- role.po_approval_limit_paise) are NOT here — they ship as
-- 0029_supplier_accounts.sql. SINGLE_STORE_MIGRATIONS pairs files by stem, so
-- a cloud-only table hidden inside this mirrored file would be undeclarable
-- and unchecked. Likewise grn_sequence, which is edge-local and ships as
-- sqlite/0028_grn_sequence.sql.
--
-- ============================================================================
-- THE RULE THIS FILE EXISTS TO ENFORCE (restated, because it binds here too)
-- ============================================================================
--
-- A GRN NEVER BLOCKS ON A PURCHASE ORDER. goods_receipt_note.purchase_order_id
-- is NULLABLE, grn_line.purchase_order_line_id is NULLABLE, supplier_id is
-- NULLABLE, and no CHECK ties a receipt to an order. That absence is
-- deliberate on BOTH sides — a cloud-side NOT NULL would refuse the replay of
-- a receipt the edge correctly accepted, which is the same outage arriving one
-- hop later and much harder to see.
--
-- ============================================================================
-- WHY purchase_order CARRIES NO RECEIPT STATE, AND WHAT THAT MEANS HERE
-- ============================================================================
--
-- purchase_order.status carries CLOUD TRANSITIONS ONLY. Receipt progress is
-- DERIVED, never stored, and the two derivations legitimately disagree:
--
--   THE EDGE derives it from ITS OWN grn_line rows
--   (edge/database/src/procurement/receipt.rs). An outlet knows what IT
--   received. It cannot know what a sibling outlet received against a shared
--   PO, and it must not need to — it works with the uplink down.
--
--   THE CLOUD derives it from EVERY replayed grn_line across every outlet
--   (backend/internal/procurement/repository.go). It sees more.
--
-- SO THE TWO NUMBERS DIFFER, AND THAT IS CORRECT, NOT A BUG. A shared PO
-- part-received at two outlets reads "40 of 100" at one till and "90 of 100"
-- in the admin, simultaneously, and both are right for the question each is
-- answering.
--
-- THIS PARAGRAPH EXISTS BECAUSE THE OBVIOUS "FIX" IS THE DEFECT. Someone will
-- find the discrepancy, read it as drift, and reconcile it by making one side
-- authoritative — writing cloud-computed progress back to the edge, or letting
-- the edge post progress up. Either one reintroduces the second writer that
-- keeping receipt state off this row exists to avoid (§50.1, ADR-011). If the
-- two figures need to be seen together, show BOTH AND LABEL THEM. Do not
-- reconcile them.

-- ---------------------------------------------------------------------------
-- supplier — AGGREGATE, cloud->edge
-- ---------------------------------------------------------------------------
CREATE TABLE supplier (
    id                  UUID PRIMARY KEY,       -- app-generated UUIDv7 (§74). No DB-side default, ever.
    outlet_id           UUID NOT NULL REFERENCES outlet(id),
    code                TEXT NOT NULL,
    name                TEXT NOT NULL,
    gstin               TEXT,
    phone               TEXT,
    email               TEXT,
    address             TEXT,
    payment_terms_days  INTEGER NOT NULL DEFAULT 0 CHECK (payment_terms_days >= 0),
    is_active           BOOLEAN NOT NULL DEFAULT TRUE,
    config_version      BIGINT NOT NULL,
    created_at          TIMESTAMPTZ NOT NULL,
    updated_at          TIMESTAMPTZ NOT NULL,
    UNIQUE (outlet_id, code)
);

CREATE INDEX idx_supplier_outlet ON supplier(outlet_id, is_active);

-- ---------------------------------------------------------------------------
-- supplier_item — CHILD ROW of supplier
-- ---------------------------------------------------------------------------
CREATE TABLE supplier_item (
    id                  UUID PRIMARY KEY,
    supplier_id         UUID NOT NULL REFERENCES supplier(id) ON DELETE CASCADE,
    inventory_item_id   UUID NOT NULL REFERENCES inventory_item(id),
    purchase_unit       TEXT NOT NULL,
    pack_size_micro     BIGINT NOT NULL CHECK (pack_size_micro > 0),

    -- THE UNIT THE AUTHOR CHOSE, NEVER DERIVED FROM THE REFERENT (0.5.2).
    -- THE CLOUD IS THE SIDE THAT REJECTS A MISMATCH, at write time, against
    -- inventory_item.dimension. The edge cannot: it degrades to a
    -- DIMENSION_MISMATCH gap and still accepts the receipt.
    --
    -- IF A WRITE PATH OR UI AUTO-FILLS THIS FROM inventory_item.dimension THE
    -- COMPARISON BECOMES x == x AND THIS REJECTION CAN NEVER FIRE — and it
    -- will look correct in review. That is the entire failure mode 0.5.2 was
    -- written about, and it is this column's only real risk.
    quantity_dimension  TEXT NOT NULL CHECK (quantity_dimension IN ('MASS','VOLUME','COUNT')),

    last_price_paise    BIGINT CHECK (last_price_paise IS NULL OR last_price_paise >= 0),
    is_preferred        BOOLEAN NOT NULL DEFAULT FALSE,
    UNIQUE (supplier_id, inventory_item_id, purchase_unit)
);

CREATE INDEX idx_supplier_item_inventory ON supplier_item(inventory_item_id);

-- ---------------------------------------------------------------------------
-- purchase_order — AGGREGATE, cloud->edge. THE CLOUD IS THE ONLY WRITER.
-- ---------------------------------------------------------------------------
CREATE TABLE purchase_order (
    id                  UUID PRIMARY KEY,
    outlet_id           UUID NOT NULL REFERENCES outlet(id),
    supplier_id         UUID NOT NULL REFERENCES supplier(id),
    po_number           TEXT NOT NULL,
    status              TEXT NOT NULL CHECK (status IN
                            ('DRAFT','PENDING_APPROVAL','APPROVED','SENT','CANCELLED','CLOSED')),
    expected_date       DATE,
    notes               TEXT,
    total_paise         BIGINT NOT NULL DEFAULT 0 CHECK (total_paise >= 0),
    approved_by_user_id UUID REFERENCES app_user(id),
    approved_at         TIMESTAMPTZ,
    created_at          TIMESTAMPTZ NOT NULL,
    updated_at          TIMESTAMPTZ NOT NULL,
    config_version      BIGINT NOT NULL,
    UNIQUE (outlet_id, po_number),

    -- An approval is both fields or neither. A half-recorded approval is how
    -- "who authorised this spend" becomes unanswerable.
    CONSTRAINT purchase_order_approval_is_whole
        CHECK ((approved_by_user_id IS NULL) = (approved_at IS NULL)),

    CONSTRAINT purchase_order_approved_states_need_an_approver
        CHECK (status NOT IN ('APPROVED','SENT','CLOSED') OR approved_by_user_id IS NOT NULL)
);

CREATE INDEX idx_purchase_order_outlet ON purchase_order(outlet_id, status);
CREATE INDEX idx_purchase_order_supplier ON purchase_order(supplier_id);

CREATE TABLE purchase_order_line (
    id                      UUID PRIMARY KEY,
    purchase_order_id       UUID NOT NULL REFERENCES purchase_order(id) ON DELETE CASCADE,
    inventory_item_id       UUID NOT NULL REFERENCES inventory_item(id),
    line_number             INTEGER NOT NULL,
    purchase_unit           TEXT NOT NULL,
    ordered_quantity_micro  BIGINT NOT NULL CHECK (ordered_quantity_micro > 0),
    quantity_dimension      TEXT NOT NULL CHECK (quantity_dimension IN ('MASS','VOLUME','COUNT')),
    unit_price_paise        BIGINT NOT NULL CHECK (unit_price_paise >= 0),
    line_total_paise        BIGINT NOT NULL CHECK (line_total_paise >= 0),
    UNIQUE (purchase_order_id, line_number)
);

CREATE INDEX idx_po_line_item ON purchase_order_line(inventory_item_id);

-- ---------------------------------------------------------------------------
-- goods_receipt_note — AGGREGATE, edge->cloud. IMMUTABLE.
-- ---------------------------------------------------------------------------
--
-- tenant_id is carried here and NOT on the SQLite twin, matching every other
-- edge->cloud table in this schema: an edge database IS one outlet, so the
-- scope is implicit there and must be explicit here.
CREATE TABLE goods_receipt_note (
    id                  UUID PRIMARY KEY,       -- minted at the edge
    tenant_id           UUID NOT NULL REFERENCES tenant(id),
    outlet_id           UUID NOT NULL REFERENCES outlet(id),

    -- NULLABLE, AND THIS IS THE POINT OF THE FILE. See the header.
    purchase_order_id   UUID REFERENCES purchase_order(id),
    supplier_id         UUID REFERENCES supplier(id),

    grn_number          TEXT NOT NULL,
    delivery_note_ref   TEXT,
    received_at         TIMESTAMPTZ NOT NULL,
    received_by_user_id UUID NOT NULL REFERENCES app_user(id),
    business_date       DATE NOT NULL,
    notes               TEXT,
    schema_version      INTEGER NOT NULL DEFAULT 1,
    ingested_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (outlet_id, grn_number)
);

CREATE INDEX idx_grn_outlet_date ON goods_receipt_note(outlet_id, business_date);
CREATE INDEX idx_grn_po ON goods_receipt_note(purchase_order_id);

-- Enforcement for the claim in the header above the table. Guards UPDATE and
-- DELETE in one trigger; the SQLite twin needs two, and has both.
--
-- The claim itself is worded ABOVE the table and never here, on purpose: the
-- lint in edge/database/src/migrations.rs attributes a claim to the nearest
-- CREATE TABLE by line distance, and this table is long enough that the same
-- wording placed beside its trigger lands on `grn_line` — a child row with no
-- trigger of its own (the invoice_line precedent). The lint then fails on a
-- table that is not the subject. It did exactly that when this file was first
-- written, which is why this comment says what it says and stays keyword-free.
CREATE OR REPLACE FUNCTION goods_receipt_note_is_immutable() RETURNS TRIGGER AS $$
BEGIN
    RAISE EXCEPTION 'goods_receipt_note is IMMUTABLE: correct a receipt with a purchase_return or a COUNT_ADJUSTMENT, never by mutating it';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER goods_receipt_note_immutable
    BEFORE UPDATE OR DELETE ON goods_receipt_note
    FOR EACH ROW EXECUTE FUNCTION goods_receipt_note_is_immutable();

CREATE TABLE grn_line (
    id                          UUID PRIMARY KEY,
    grn_id                      UUID NOT NULL REFERENCES goods_receipt_note(id) ON DELETE CASCADE,
    inventory_item_id           UUID NOT NULL REFERENCES inventory_item(id),
    line_number                 INTEGER NOT NULL,
    purchase_order_line_id      UUID REFERENCES purchase_order_line(id),
    entered_purchase_unit       TEXT NOT NULL,
    entered_quantity_micro      BIGINT NOT NULL CHECK (entered_quantity_micro > 0),
    quantity_dimension          TEXT NOT NULL CHECK (quantity_dimension IN ('MASS','VOLUME','COUNT')),
    base_quantity_micro         BIGINT NOT NULL CHECK (base_quantity_micro > 0),
    pack_size_micro_applied     BIGINT NOT NULL CHECK (pack_size_micro_applied > 0),
    unit_cost_paise             BIGINT NOT NULL CHECK (unit_cost_paise >= 0),
    line_total_paise            BIGINT NOT NULL CHECK (line_total_paise >= 0),

    -- MODELLED NOW, ALERTED IN M6. Batch identity is captured at receipt or
    -- never. EXEMPT with M6 named; the exemption comes out when M6 lands.
    batch_code                  TEXT,
    expiry_date                 DATE,

    UNIQUE (grn_id, line_number)
);

CREATE INDEX idx_grn_line_item ON grn_line(inventory_item_id);
CREATE INDEX idx_grn_line_expiry ON grn_line(expiry_date) WHERE expiry_date IS NOT NULL;

-- ---------------------------------------------------------------------------
-- grn_gap — AGGREGATE, edge->cloud. PLAIN OUTBOX, no entry_seq.
-- ---------------------------------------------------------------------------
--
-- Deliberately NOT a ranged stream, unlike stock_deduction_gap. A grn_gap is a
-- discrete event a buyer acts on — a handful a week — not a per-sale row
-- arriving all day. It therefore has no entry_seq, no counter, no cursor and
-- no ledger_replay_gap contiguity checking. See sqlite/0027 for the full
-- reasoning; note that this means NOTHING here needs the 0.5.8 machinery.
CREATE TABLE grn_gap (
    id                  UUID PRIMARY KEY,
    tenant_id           UUID NOT NULL REFERENCES tenant(id),
    outlet_id           UUID NOT NULL REFERENCES outlet(id),
    grn_id              UUID NOT NULL REFERENCES goods_receipt_note(id),
    grn_line_id         UUID REFERENCES grn_line(id),
    inventory_item_id   UUID REFERENCES inventory_item(id),
    reason              TEXT NOT NULL CHECK (reason IN (
                            'NO_PURCHASE_ORDER',
                            'PURCHASE_ORDER_NOT_FOUND',
                            'PO_LINE_NOT_FOUND',
                            'QUANTITY_EXCEEDS_ORDERED',
                            'NO_SUPPLIER_ITEM',
                            'NO_UNIT_CONVERSION',
                            'DIMENSION_MISMATCH',
                            'SUPPLIER_NOT_FOUND'
                        )),
    detail              TEXT,
    occurred_at         TIMESTAMPTZ NOT NULL,
    business_date       DATE NOT NULL,
    schema_version      INTEGER NOT NULL DEFAULT 1,
    ingested_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_grn_gap_outlet ON grn_gap(outlet_id, business_date);

-- ---------------------------------------------------------------------------
-- purchase_return — AGGREGATE, edge->cloud. IMMUTABLE.
-- ---------------------------------------------------------------------------
CREATE TABLE purchase_return (
    id                  UUID PRIMARY KEY,
    tenant_id           UUID NOT NULL REFERENCES tenant(id),
    outlet_id           UUID NOT NULL REFERENCES outlet(id),
    supplier_id         UUID REFERENCES supplier(id),
    grn_id              UUID REFERENCES goods_receipt_note(id),
    return_number       TEXT NOT NULL,
    reason              TEXT NOT NULL CHECK (reason IN
                            ('DAMAGED','EXPIRED','WRONG_ITEM','QUALITY','OVER_DELIVERY','OTHER')),
    returned_at         TIMESTAMPTZ NOT NULL,
    returned_by_user_id UUID NOT NULL REFERENCES app_user(id),
    business_date       DATE NOT NULL,
    notes               TEXT,
    schema_version      INTEGER NOT NULL DEFAULT 1,
    ingested_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (outlet_id, return_number)
);

CREATE OR REPLACE FUNCTION purchase_return_is_immutable() RETURNS TRIGGER AS $$
BEGIN
    RAISE EXCEPTION 'purchase_return is IMMUTABLE: append a correcting movement, never mutate';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER purchase_return_immutable
    BEFORE UPDATE OR DELETE ON purchase_return
    FOR EACH ROW EXECUTE FUNCTION purchase_return_is_immutable();

CREATE TABLE purchase_return_line (
    id                      UUID PRIMARY KEY,
    purchase_return_id      UUID NOT NULL REFERENCES purchase_return(id) ON DELETE CASCADE,
    inventory_item_id       UUID NOT NULL REFERENCES inventory_item(id),
    grn_line_id             UUID REFERENCES grn_line(id),
    line_number             INTEGER NOT NULL,
    entered_purchase_unit   TEXT NOT NULL,
    entered_quantity_micro  BIGINT NOT NULL CHECK (entered_quantity_micro > 0),
    quantity_dimension      TEXT NOT NULL CHECK (quantity_dimension IN ('MASS','VOLUME','COUNT')),
    base_quantity_micro     BIGINT NOT NULL CHECK (base_quantity_micro > 0),
    unit_cost_paise         BIGINT NOT NULL CHECK (unit_cost_paise >= 0),
    UNIQUE (purchase_return_id, line_number)
);

-- ---------------------------------------------------------------------------
-- stock_transfer_out — AGGREGATE, edge->cloud. OUTBOUND HALF ONLY (M5).
-- ---------------------------------------------------------------------------
--
-- destination_outlet_id IS a real FK here and is NOT one in SQLite, because
-- the destination outlet exists in the cloud's schema and may not exist in the
-- source outlet's edge database at all. That asymmetry is the point of the
-- column, not an oversight.
CREATE TABLE stock_transfer_out (
    id                      UUID PRIMARY KEY,
    tenant_id               UUID NOT NULL REFERENCES tenant(id),
    outlet_id               UUID NOT NULL REFERENCES outlet(id),      -- source
    destination_outlet_id   UUID NOT NULL REFERENCES outlet(id),
    transfer_number         TEXT NOT NULL,
    dispatched_at           TIMESTAMPTZ NOT NULL,
    dispatched_by_user_id   UUID NOT NULL REFERENCES app_user(id),
    business_date           DATE NOT NULL,
    notes                   TEXT,
    schema_version          INTEGER NOT NULL DEFAULT 1,
    ingested_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (outlet_id, transfer_number),
    CONSTRAINT stock_transfer_out_not_to_itself
        CHECK (destination_outlet_id <> outlet_id)
);

CREATE OR REPLACE FUNCTION stock_transfer_out_is_immutable() RETURNS TRIGGER AS $$
BEGIN
    RAISE EXCEPTION 'stock_transfer_out is IMMUTABLE: a dispatch that was wrong is corrected by a return movement';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER stock_transfer_out_immutable
    BEFORE UPDATE OR DELETE ON stock_transfer_out
    FOR EACH ROW EXECUTE FUNCTION stock_transfer_out_is_immutable();

CREATE TABLE stock_transfer_line (
    id                      UUID PRIMARY KEY,
    stock_transfer_out_id   UUID NOT NULL REFERENCES stock_transfer_out(id) ON DELETE CASCADE,
    inventory_item_id       UUID NOT NULL REFERENCES inventory_item(id),
    line_number             INTEGER NOT NULL,
    base_quantity_micro     BIGINT NOT NULL CHECK (base_quantity_micro > 0),
    quantity_dimension      TEXT NOT NULL CHECK (quantity_dimension IN ('MASS','VOLUME','COUNT')),
    unit_cost_paise         BIGINT NOT NULL CHECK (unit_cost_paise >= 0),
    UNIQUE (stock_transfer_out_id, line_number)
);

-- ---------------------------------------------------------------------------
-- stock_ledger_entry — provenance for the three new sources
-- ---------------------------------------------------------------------------
--
-- The source_stock_count_id lesson (0.5.5 added the column, 0.5.9 finally put
-- it on the wire) is the reason these three are added to the Go struct, the
-- Zod schema, the OpenAPI shape AND both halves of the repository INSERT and
-- SELECT in the same version they appear here. A column the cloud has never
-- heard of is silently discarded by json.Unmarshal and is NULL for every row.
--
-- TRANSFER_IN provenance is deliberately NOT pre-added: no consumer until M8.
ALTER TABLE stock_ledger_entry ADD COLUMN source_grn_id UUID REFERENCES goods_receipt_note(id);
ALTER TABLE stock_ledger_entry ADD COLUMN source_purchase_return_id UUID REFERENCES purchase_return(id);
ALTER TABLE stock_ledger_entry ADD COLUMN source_stock_transfer_out_id UUID REFERENCES stock_transfer_out(id);
