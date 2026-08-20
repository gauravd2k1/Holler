-- Holler Edge SQLite — the stock ledger. Contracts 0.5.0, ADR-018.
--
-- EDGE-AUTHORITATIVE, edge->cloud. The outlet consumes, wastes and counts
-- stock with the uplink down; the cloud only replays. stock_ledger_entry,
-- stock_count and stock_deduction_gap are AggregateTypes.
-- stock_count_line is a CHILD ROW travelling inside its parent's payload —
-- not an aggregate, no sync direction, the invoice_line precedent.
--
-- ============================================================================
-- FOUR RULES BIND EVERY CONSUMER OF THIS FILE
-- ============================================================================
--
-- 1. STOCK NEVER BLOCKS A SALE. Negative stock is PERMITTED and is a variance
--    signal, not an error. There is deliberately no CHECK constraining a
--    balance to be non-negative, in either store. A restaurant that has sold
--    more than its records say it held has a counting problem, not a reason to
--    refuse a customer.
--
-- 2. A MISSING OR BROKEN RECIPE NEVER FAILS A CONFIRM. No recipe, an
--    unresolvable unit, a cycle, a depth overrun, a line with no variant —
--    every one records a stock_deduction_gap and lets the sale complete.
--    "Items sold with no recipe" is a VISIBLE report (§64: staff are told
--    whether intervention is needed).
--
-- 3. CONCURRENT DEDUCTION IS SERIALIZED BY TRANSACTION. The edge is a SINGLE
--    SQLite WRITER (ADR-013: one native executable over one SQLite file, WAL).
--    LAN clients — the KDS today, the waiter app at M9 — are COMMAND CLIENTS,
--    not writers: they send commands to that process and it performs the
--    write. Deduction therefore runs inside the same transaction as
--    confirm_order and needs no lock of its own.
--    This is written down because it is load-bearing: the day a second process
--    writes this file, this rule and ReplayTransition's duplicate handling
--    both break, and they break SILENTLY.
--
-- 4. STOCK NEVER SYNCS DOWNWARD. The cloud MAY re-derive a stock view by
--    summing the ingested ledger. It may NEVER mirror the edge's snapshot, and
--    no route ever sends a stock quantity cloud->edge. The ledger is the only
--    thing that crosses, and it crosses upward.

-- ---------------------------------------------------------------------------
-- stock_ledger_entry — AGGREGATE, edge->cloud. Append-only. Self-describing.
-- ---------------------------------------------------------------------------
--
-- THE ENTRY IS READABLE WITHOUT ANY OTHER TABLE. It stores the quantity
-- ACTUALLY APPLIED as the authoritative value and snapshots its context, with
-- NO foreign keys to config or to orders — the order_item_modifier precedent:
-- snapshot the values, never point at a live catalogue row.
--
-- Three consequences, all intended:
--   * A recipe edit NEVER retro-alters a past deduction. The old entry keeps
--     the old applied quantity and the old version number.
--   * Deleting a recipe orphans nothing. There is no FK to violate.
--   * An auditor can read a year of ledger without the recipe table — which,
--     being cloud config, will have been overwritten by sync many times over.
--
-- ROUNDING HAPPENS ONCE, AT THE LEAF, BEFORE THE VALUE REACHES THIS TABLE.
-- Sub-recipe resolution accumulates as an exact i128 rational through the
-- whole tree; the applied quantity is rounded HALF AWAY FROM ZERO exactly once
-- when it is written here. Rounding at each level of a tree accumulates drift,
-- the same reasoning ADR-016 §3 used for per-component tax.
CREATE TABLE stock_ledger_entry (
    id                  TEXT PRIMARY KEY,       -- UUIDv7, app-generated (§74)
    outlet_id           TEXT NOT NULL REFERENCES outlet(id),

    -- THE HIGH-WATER MARK. A per-outlet monotonic counter assigned by the edge
    -- at insert, in the same transaction — the invoice_sequence pattern, and
    -- edge-local for the same reason.
    --
    -- WHY A COUNTER AND NOT JUST business_date. A snapshot seals a day. If the
    -- delta query were "business_date > snapshot.business_date", an entry that
    -- ARRIVES after its day is sealed while CARRYING that day's business_date
    -- would be excluded from the snapshot (it did not exist at seal time) AND
    -- from the delta (its date is too old). It would vanish from the derived
    -- balance permanently, with no error — and a seal is never UPDATEd, so
    -- nothing would ever put it back.
    --
    -- That is not hypothetical. A count started 23:40 and completed 00:15
    -- posts COUNT_ADJUSTMENT entries dated to the earlier business day.
    -- Cloud-side re-derivation over a replayed ledger arrives in replay order,
    -- not business-date order. Any adjustment carrying an explicit business
    -- date does the same.
    --
    -- So the delta selects everything NOT COVERED BY THE MARK
    -- (entry_seq > through_entry_seq), never everything after the date. A late
    -- arrival gets a mark above the seal, so it self-heals into the very next
    -- read instead of disappearing.
    entry_seq           INTEGER NOT NULL,

    -- Snapshotted, NO FK. See the header above.
    inventory_item_id   TEXT NOT NULL,
    inventory_item_name TEXT NOT NULL,
    dimension           TEXT NOT NULL CHECK (dimension IN ('MASS','VOLUME','COUNT')),

    entry_type          TEXT NOT NULL CHECK (entry_type IN (
                            'PURCHASE','CONSUMPTION','WASTAGE',
                            'TRANSFER_IN','TRANSFER_OUT','ADJUSTMENT',
                            'RETURN_TO_VENDOR',
                            'PRODUCTION_CONSUMPTION','PRODUCTION_OUTPUT')),

    -- WHY the entry knows where it came from as well as what it is: a
    -- CONSUMPTION posted by a recipe and one posted by a modifier delta are
    -- the same entry_type and different facts, and the variance report has to
    -- be able to tell them apart without re-deriving anything.
    origin              TEXT NOT NULL CHECK (origin IN (
                            'RECIPE','MODIFIER_DELTA','MANUAL',
                            'COUNT_ADJUSTMENT','WASTAGE')),

    -- THE QUANTITY ACTUALLY APPLIED, and it is AUTHORITATIVE. Not "what the
    -- recipe says" — what this deduction did. The recipe reference below is
    -- provenance only, so this value never has to be re-derived and cannot be
    -- invalidated by a later recipe edit.
    --
    -- Named for what it is: the sibling columns are counted_quantity_micro,
    -- expected_quantity_micro and closing_quantity_micro, and a bare
    -- `quantity_micro` here would be the only one that did not say which
    -- quantity it meant.
    --
    -- SIGNED. Consumption is negative, purchase positive. Deliberately no
    -- CHECK on sign: see Rule 1.
    quantity_applied_micro INTEGER NOT NULL,

    -- Provenance, all nullable, all WITHOUT FK.
    recipe_id           TEXT,
    recipe_version      INTEGER,
    recipe_name         TEXT,
    source_order_id     TEXT,
    source_order_item_id TEXT,

    reason_code         TEXT,                   -- wastage: SPOILAGE, PREP_LOSS, BREAKAGE, ...
    note                TEXT,

    occurred_at         TEXT NOT NULL,          -- ISO8601 UTC (CLAUDE.md §Time)

    -- Outlet-local business day per 0013's definition, computed ONCE at write
    -- time and never recomputed on read. Unlike invoice.business_date (0006),
    -- whose identical comment was not true of its implementation, this column
    -- lands together with the outlet.day_start_time that defines it.
    business_date       TEXT NOT NULL,

    created_by_user_id  TEXT,

    -- Modifier provenance, and it carries the same weight as the recipe
    -- provenance above. A row deducted from a modifier_ingredient_delta is
    -- explained by NONE of recipe_id/version/name — so without these three,
    -- "extra paneer took 50g" becomes unauditable the moment somebody edits
    -- the delta. Identical hole to the recipe one, one table over.
    -- Nullable and no FK, exactly like the recipe fields.
    modifier_delta_id       TEXT,
    modifier_name           TEXT,   -- 'Spice: Hot', 'Extra Paneer' — as shown to the cashier
    modifier_delta_version  INTEGER,-- the delta row's config_version when applied

    -- DEFERRED, landing M5 (ADR-018 §8). NULL in M4, pinned by exact
    -- assertion. Cost lives HERE and not on inventory_item because a weighted
    -- average is derived from edge-recorded purchases: on the cloud-owned
    -- config row it would be a split-authority column.
    unit_cost_paise     INTEGER,

    -- EXACTLY-ONE PROVENANCE, keyed on origin. Two nullable provenance groups
    -- with nothing joining them would let a row claim both a recipe and a
    -- modifier, or neither while calling itself RECIPE -- and a half-attributed
    -- deduction is the thing this provenance exists to prevent. Same
    -- exactly-one discipline as recipe_ingredient's component_kind.
    CHECK (
        (origin = 'RECIPE'
            AND recipe_id IS NOT NULL
            AND modifier_delta_id IS NULL)
     OR (origin = 'MODIFIER_DELTA'
            AND modifier_delta_id IS NOT NULL
            AND recipe_id IS NULL)
     OR (origin IN ('MANUAL','COUNT_ADJUSTMENT','WASTAGE')
            AND recipe_id IS NULL
            AND modifier_delta_id IS NULL)
    ),
    -- The mark is per outlet and gapless-monotonic; a duplicate would make
    -- "not covered by the mark" ambiguous in exactly the way the date was.
    UNIQUE (outlet_id, entry_seq)
);

-- The read pattern the snapshot depends on: everything for one item since a
-- sealed date. Leading outlet_id keeps it usable for whole-outlet sweeps too.
CREATE INDEX idx_stock_ledger_entry_item_date
    ON stock_ledger_entry(outlet_id, inventory_item_id, business_date);

CREATE INDEX idx_stock_ledger_entry_order
    ON stock_ledger_entry(source_order_id);

-- APPEND-ONLY, enforced by the database rather than by convention — the
-- `payment` precedent (0009). A wrong deduction is corrected by appending an
-- ADJUSTMENT entry, never by editing history. This is the table where that
-- matters most: it is the source of truth for stock, and it is the fastest
-- growing table in the product.
CREATE TRIGGER stock_ledger_entry_is_append_only_no_update
BEFORE UPDATE ON stock_ledger_entry
BEGIN
    SELECT RAISE(ABORT,
        'stock_ledger_entry is append-only: correct it by appending an ADJUSTMENT entry, never UPDATE (ADR-018, contracts 0.5.0)');
END;

CREATE TRIGGER stock_ledger_entry_is_append_only_no_delete
BEFORE DELETE ON stock_ledger_entry
BEGIN
    SELECT RAISE(ABORT,
        'stock_ledger_entry is append-only: a ledger row is never deleted, append an ADJUSTMENT entry instead (ADR-018, contracts 0.5.0)');
END;

-- ---------------------------------------------------------------------------
-- stock_count / stock_count_line — AGGREGATE + CHILD, edge->cloud
-- ---------------------------------------------------------------------------
--
-- WHY COUNTS ARE IN M4 AT ALL. M4's acceptance is that selling a dish produces
-- correct ledger entries. A physical count is the only mechanism that can
-- FALSIFY that claim: theoretical deduction is arithmetic over data we
-- control and will always agree with itself. Actual = Opening + Purchases +
-- Transfers In - Transfers Out - Closing is the independent measurement.
-- Shipping deduction without the instrument that checks it would repeat the
-- §66 lesson: an invariant nobody has watched fail is not a gate.
--
-- A completed count posts COUNT_ADJUSTMENT entries to the ledger, so the
-- ledger stays the single source of stock. The count itself is the evidence;
-- the adjustment is the effect. Variance is DERIVED and never stored as
-- authoritative.
CREATE TABLE stock_count (
    id                  TEXT PRIMARY KEY,       -- UUIDv7, app-generated
    outlet_id           TEXT NOT NULL REFERENCES outlet(id),
    business_date       TEXT NOT NULL,          -- per 0013
    status              TEXT NOT NULL CHECK (status IN ('OPEN','COMPLETED')),
    started_at          TEXT NOT NULL,          -- ISO8601 UTC
    completed_at        TEXT,
    counted_by_user_id  TEXT,
    note                TEXT
);

CREATE INDEX idx_stock_count_outlet_date ON stock_count(outlet_id, business_date);

CREATE TABLE stock_count_line (
    id                  TEXT PRIMARY KEY,       -- UUIDv7, app-generated
    stock_count_id      TEXT NOT NULL REFERENCES stock_count(id),

    -- Snapshotted like the ledger's, and for the same reason: a count from six
    -- months ago must stay readable after the item has been renamed or
    -- deactivated.
    inventory_item_id   TEXT NOT NULL,
    inventory_item_name TEXT NOT NULL,
    dimension           TEXT NOT NULL CHECK (dimension IN ('MASS','VOLUME','COUNT')),

    counted_quantity_micro  INTEGER NOT NULL,

    -- The theoretical balance AT THE MOMENT OF COUNTING, snapshotted so the
    -- variance stays reproducible. Recomputing it later would compare today's
    -- theory against yesterday's shelf. Signed: theory can be negative
    -- (Rule 1).
    expected_quantity_micro INTEGER NOT NULL,

    note                TEXT,
    UNIQUE (stock_count_id, inventory_item_id)
);

-- MUTABLE WHILE OPEN, IMMUTABLE ONCE COMPLETED. Without this, the append-only
-- ledger has a side door: a completed count posts COUNT_ADJUSTMENT entries the
-- ledger will not let you edit, and then someone edits the count those entries
-- were derived from, leaving the evidence and the effect disagreeing with no
-- record of which changed.
--
-- This is status-conditional rather than blanket because a count is genuinely
-- a working document while it is being taken — a stocktaker walks the shelves
-- and corrects their own typing. The cash_shift note in 0009 explains why a
-- blanket trigger there would have broken a correct path; the same reasoning
-- applies here, and the WHEN clause is how both are satisfied at once.
CREATE TRIGGER stock_count_is_immutable_once_completed
BEFORE UPDATE ON stock_count
WHEN OLD.status = 'COMPLETED'
BEGIN
    SELECT RAISE(ABORT,
        'stock_count is immutable once COMPLETED: it is the evidence behind COUNT_ADJUSTMENT ledger entries, which are append-only. Take a new count (ADR-018, contracts 0.5.0)');
END;

CREATE TRIGGER stock_count_no_delete_once_completed
BEFORE DELETE ON stock_count
WHEN OLD.status = 'COMPLETED'
BEGIN
    SELECT RAISE(ABORT,
        'stock_count is immutable once COMPLETED: a completed count is never deleted (ADR-018, contracts 0.5.0)');
END;

-- Same rule, one level down. The line is where the number actually lives, so
-- protecting only the parent would leave the door open.
CREATE TRIGGER stock_count_line_is_immutable_once_completed
BEFORE UPDATE ON stock_count_line
WHEN (SELECT status FROM stock_count WHERE id = OLD.stock_count_id) = 'COMPLETED'
BEGIN
    SELECT RAISE(ABORT,
        'stock_count_line is immutable once its count is COMPLETED (ADR-018, contracts 0.5.0)');
END;

CREATE TRIGGER stock_count_line_no_delete_once_completed
BEFORE DELETE ON stock_count_line
WHEN (SELECT status FROM stock_count WHERE id = OLD.stock_count_id) = 'COMPLETED'
BEGIN
    SELECT RAISE(ABORT,
        'stock_count_line is immutable once its count is COMPLETED (ADR-018, contracts 0.5.0)');
END;

-- ---------------------------------------------------------------------------
-- stock_deduction_gap — AGGREGATE, edge->cloud. A SIGNAL, never a correction.
-- ---------------------------------------------------------------------------
--
-- WHY IT IS CLOUD-VISIBLE and not an edge-local table:
--
--   * The person who can SEE it and the person who can FIX it are different
--     people in different places. Fixing a gap means authoring a recipe, which
--     is cloud config under recipe.manage. A POS-only report reaches a cashier
--     who cannot act on it.
--   * Variance is read in the cloud, and whatever EXPLAINS a number must live
--     where the number is read. An edge-only gap record makes the cloud
--     variance report unexplainable by construction — and an unexplained
--     shortfall reads as theft.
--
-- IT IS A SIGNAL, NOT A CORRECTION. Deductions are NEVER backfilled when the
-- recipe is later authored: that would retro-alter history, which the no-FK
-- provenance model above exists to make impossible. In the variance report it
-- appears as a NAMED TERM — "N sales unaccounted" — and is never folded into
-- shrinkage.
--
-- INGEST: it shares POST /inventory/ledger-entries with stock_ledger_entry
-- rather than taking a route of its own. It is still a real AggregateType with
-- an AggregateAuthority entry, because validateAuthority rejects an unknown
-- aggregate type outright; what it does not get is a second route.
CREATE TABLE stock_deduction_gap (
    id                  TEXT PRIMARY KEY,       -- UUIDv7, app-generated
    outlet_id           TEXT NOT NULL REFERENCES outlet(id),

    -- Provenance, no FK, same rule as the ledger.
    order_id            TEXT NOT NULL,
    order_item_id       TEXT NOT NULL,
    menu_item_id        TEXT NOT NULL,
    menu_item_variant_id TEXT,                  -- NULL is itself one of the reasons
    menu_item_name      TEXT NOT NULL,

    -- Units sold that went unaccounted. A plain count of sellable units, NOT a
    -- micro-quantity: nothing was resolved to an ingredient, which is the
    -- whole point of the row.
    quantity            INTEGER NOT NULL CHECK (quantity > 0),

    reason              TEXT NOT NULL CHECK (reason IN (
                            'NO_RECIPE',        -- nothing authored for this sellable unit
                            'NO_VARIANT',       -- line carried no variant; 0014's invariant failed
                            'CYCLE',            -- sub-recipe cycle reached the edge
                            'DEPTH_EXCEEDED',   -- deeper than MAX_RECIPE_DEPTH
                            'UNKNOWN_UNIT')),   -- no conversion for a pack unit

    occurred_at         TEXT NOT NULL,          -- ISO8601 UTC
    business_date       TEXT NOT NULL           -- per 0013
);

CREATE INDEX idx_stock_deduction_gap_outlet_date
    ON stock_deduction_gap(outlet_id, business_date);

CREATE INDEX idx_stock_deduction_gap_item
    ON stock_deduction_gap(menu_item_id);
