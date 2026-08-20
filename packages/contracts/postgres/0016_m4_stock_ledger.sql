-- Holler Cloud PostgreSQL — the stock ledger. Contracts 0.5.0, ADR-018.
-- Mirror of sqlite/0016_m4_stock_ledger.sql, whose header carries the full
-- reasoning. EDGE-AUTHORITATIVE, edge->cloud: stock_ledger_entry, stock_count
-- and stock_deduction_gap are AggregateTypes and the cloud only REPLAYS them.
-- stock_count_line is a child row inside its parent's payload.
--
-- THERE IS NO postgres/0017. stock_balance_snapshot is EDGE-LOCAL, SQLite
-- only, deliberately unmirrored — the invoice_sequence precedent. The cloud
-- may re-derive its own stock view by summing these entries; it may never
-- mirror the edge's projection, and stock never syncs downward.
--
-- Summary of the four rules recorded in the SQLite mirror:
--   1. Stock never blocks a sale. Negative stock is permitted and is a
--      variance signal. No CHECK constrains a balance to be non-negative.
--   2. A missing or broken recipe never fails a confirm; it records a
--      stock_deduction_gap and the sale completes.
--   3. Concurrent deduction is serialized by transaction, because the edge is
--      a single SQLite writer and LAN clients are command clients, not
--      writers.
--   4. Stock never syncs downward.
--
-- Ids are app-generated UUIDv7 (§74) with no DB-side default — for these
-- tables the id is minted at the EDGE and replayed, so a cloud-side default
-- would be actively wrong, not merely inconsistent.

CREATE TABLE stock_ledger_entry (
    id                  UUID PRIMARY KEY,
    outlet_id           UUID NOT NULL REFERENCES outlet(id),

    -- Snapshotted, NO FK to config: a recipe edit must never retro-alter a
    -- past deduction, and a year of ledger must stay readable without the
    -- config tables, which sync overwrites repeatedly.
    inventory_item_id   UUID NOT NULL,
    inventory_item_name TEXT NOT NULL,
    dimension           TEXT NOT NULL CHECK (dimension IN ('MASS','VOLUME','COUNT')),

    entry_type          TEXT NOT NULL CHECK (entry_type IN (
                            'PURCHASE','CONSUMPTION','WASTAGE',
                            'TRANSFER_IN','TRANSFER_OUT','ADJUSTMENT',
                            'RETURN_TO_VENDOR',
                            'PRODUCTION_CONSUMPTION','PRODUCTION_OUTPUT')),

    origin              TEXT NOT NULL CHECK (origin IN (
                            'RECIPE','MODIFIER_DELTA','MANUAL',
                            'COUNT_ADJUSTMENT','WASTAGE')),

    -- THE QUANTITY ACTUALLY APPLIED, and it is AUTHORITATIVE. The recipe
    -- reference below is provenance only: this value is never re-derived, and
    -- a later recipe edit cannot invalidate it. Named for which quantity it
    -- means, like its counted_/expected_/closing_ siblings.
    -- Signed. Deliberately no CHECK on sign; see Rule 1.
    quantity_applied_micro BIGINT NOT NULL,

    -- Provenance, nullable, no FK.
    recipe_id           UUID,
    recipe_version      INTEGER,
    recipe_name         TEXT,
    source_order_id     UUID,
    source_order_item_id UUID,

    reason_code         TEXT,
    note                TEXT,

    occurred_at         TIMESTAMPTZ NOT NULL,
    -- Outlet-local business day per 0013, computed once at the EDGE at write
    -- time and replayed verbatim. The cloud never recomputes it: it does not
    -- own the definition's inputs at the moment the row was written.
    business_date       DATE NOT NULL,

    created_by_user_id  UUID,

    -- DEFERRED, landing M5 (ADR-018 §8).
    unit_cost_paise     BIGINT
);

CREATE INDEX idx_stock_ledger_entry_item_date
    ON stock_ledger_entry(outlet_id, inventory_item_id, business_date);

CREATE INDEX idx_stock_ledger_entry_order
    ON stock_ledger_entry(source_order_id);

-- Append-only, enforced by the database — the `payment` precedent (0009 in the
-- SQLite line). A wrong deduction is corrected by appending an ADJUSTMENT,
-- never by editing history.
CREATE OR REPLACE FUNCTION stock_ledger_entry_append_only()
RETURNS TRIGGER AS $$
BEGIN
    RAISE EXCEPTION 'stock_ledger_entry is append-only: correct it by appending an ADJUSTMENT entry, never UPDATE or DELETE (ADR-018, contracts 0.5.0)';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER stock_ledger_entry_is_append_only
BEFORE UPDATE OR DELETE ON stock_ledger_entry
FOR EACH ROW EXECUTE FUNCTION stock_ledger_entry_append_only();

CREATE TABLE stock_count (
    id                  UUID PRIMARY KEY,
    outlet_id           UUID NOT NULL REFERENCES outlet(id),
    business_date       DATE NOT NULL,
    status              TEXT NOT NULL CHECK (status IN ('OPEN','COMPLETED')),
    started_at          TIMESTAMPTZ NOT NULL,
    completed_at        TIMESTAMPTZ,
    counted_by_user_id  UUID,
    note                TEXT
);

CREATE INDEX idx_stock_count_outlet_date ON stock_count(outlet_id, business_date);

CREATE TABLE stock_count_line (
    id                  UUID PRIMARY KEY,
    stock_count_id      UUID NOT NULL REFERENCES stock_count(id),
    inventory_item_id   UUID NOT NULL,
    inventory_item_name TEXT NOT NULL,
    dimension           TEXT NOT NULL CHECK (dimension IN ('MASS','VOLUME','COUNT')),
    counted_quantity_micro  BIGINT NOT NULL,
    -- Theoretical balance at the moment of counting, snapshotted so variance
    -- stays reproducible. Recomputing later would compare today's theory
    -- against yesterday's shelf.
    expected_quantity_micro BIGINT NOT NULL,
    note                TEXT,
    UNIQUE (stock_count_id, inventory_item_id)
);

-- Mutable while OPEN, immutable once COMPLETED. Without it the append-only
-- ledger has a side door: a completed count posts COUNT_ADJUSTMENT entries
-- that cannot be edited, and editing the count they were derived from leaves
-- evidence and effect disagreeing. Status-conditional rather than blanket,
-- because a count in progress is genuinely a working document.
CREATE OR REPLACE FUNCTION stock_count_immutable_once_completed()
RETURNS TRIGGER AS $$
BEGIN
    IF OLD.status = 'COMPLETED' THEN
        RAISE EXCEPTION 'stock_count is immutable once COMPLETED: it is the evidence behind append-only COUNT_ADJUSTMENT ledger entries. Take a new count (ADR-018, contracts 0.5.0)';
    END IF;
    RETURN CASE TG_OP WHEN 'DELETE' THEN OLD ELSE NEW END;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER stock_count_is_immutable_once_completed
BEFORE UPDATE OR DELETE ON stock_count
FOR EACH ROW EXECUTE FUNCTION stock_count_immutable_once_completed();

-- Same rule one level down: the line is where the number lives, so protecting
-- only the parent would leave the door open.
CREATE OR REPLACE FUNCTION stock_count_line_immutable_once_completed()
RETURNS TRIGGER AS $$
BEGIN
    IF (SELECT status FROM stock_count WHERE id = OLD.stock_count_id) = 'COMPLETED' THEN
        RAISE EXCEPTION 'stock_count_line is immutable once its count is COMPLETED (ADR-018, contracts 0.5.0)';
    END IF;
    RETURN CASE TG_OP WHEN 'DELETE' THEN OLD ELSE NEW END;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER stock_count_line_is_immutable_once_completed
BEFORE UPDATE OR DELETE ON stock_count_line
FOR EACH ROW EXECUTE FUNCTION stock_count_line_immutable_once_completed();

-- A SIGNAL, never a correction. Deductions are never backfilled when a recipe
-- is later authored; in the variance report this appears as a named term
-- ("N sales unaccounted"), never folded into shrinkage.
--
-- Ingest shares POST /inventory/ledger-entries with stock_ledger_entry. It is
-- still a real AggregateType with an AggregateAuthority entry — validateAuthority
-- rejects an unknown aggregate type outright — it simply gets no second route.
CREATE TABLE stock_deduction_gap (
    id                  UUID PRIMARY KEY,
    outlet_id           UUID NOT NULL REFERENCES outlet(id),
    order_id            UUID NOT NULL,
    order_item_id       UUID NOT NULL,
    menu_item_id        UUID NOT NULL,
    menu_item_variant_id UUID,
    menu_item_name      TEXT NOT NULL,
    -- Sellable units sold unaccounted, not a micro-quantity: nothing was
    -- resolved to an ingredient, which is the point of the row.
    quantity            INTEGER NOT NULL CHECK (quantity > 0),
    reason              TEXT NOT NULL CHECK (reason IN (
                            'NO_RECIPE','NO_VARIANT','CYCLE',
                            'DEPTH_EXCEEDED','UNKNOWN_UNIT')),
    occurred_at         TIMESTAMPTZ NOT NULL,
    business_date       DATE NOT NULL
);

CREATE INDEX idx_stock_deduction_gap_outlet_date
    ON stock_deduction_gap(outlet_id, business_date);

CREATE INDEX idx_stock_deduction_gap_item
    ON stock_deduction_gap(menu_item_id);
