-- Holler Edge SQLite — Milestone 3 billing: tax engine, GST invoice,
-- discounts, split bills, split payments, cash shift, invoice numbering.
-- Contracts 0.4.0, ADR-016.
--
-- The authority split this migration encodes (§50.1):
--   tax_profile, tax_rule, compliance_version,
--   outlet_fiscal_profile, invoice_series,
--   discount_definition                     -> CONFIG, cloud→edge, config_version.
--   invoice, invoice_line, payment,
--   payment_allocation, cash_shift,
--   cash_movement                           -> EDGE-authoritative, edge→cloud.
--   invoice_sequence                        -> EDGE-LOCAL. SQLite only. No
--                                              Postgres mirror, no AggregateType,
--                                              never a sync direction.
-- No table below is half-config, half-transaction.
--
-- The invoice_series / invoice_sequence split is the load-bearing one. A
-- series *definition* (its prefix, its reset policy) is a management decision
-- and belongs to the cloud. The *counter* is shop-floor state that must keep
-- issuing numbers with the uplink down, so it is edge-local and never syncs —
-- the print_job precedent from 0005, applied to numbering. The issued number
-- travels to the cloud on the invoice; the counter that produced it does not.
-- ADR-013 makes the outlet a single writer over one SQLite file, which is what
-- makes a local counter concurrency-safe rather than merely convenient.

------------------------------------------------------------------------------
-- CONFIG (cloud→edge)
------------------------------------------------------------------------------

-- A named, versioned ruleset. Invoices snapshot the id AND the resolved rules,
-- so a bill stays reproducible after the rules change (§31).
CREATE TABLE compliance_version (
    id                  TEXT PRIMARY KEY,       -- UUIDv7, minted by the cloud
    outlet_id           TEXT NOT NULL REFERENCES outlet(id),
    label               TEXT NOT NULL,          -- 'GST 2026-04 restaurant', human-readable
    effective_from      TEXT NOT NULL,          -- ISO8601 UTC
    notes               TEXT,
    config_version      INTEGER NOT NULL
);

CREATE UNIQUE INDEX idx_compliance_version_outlet_label ON compliance_version(outlet_id, label);

-- A tax profile is what a menu item points at. Never a percentage scattered
-- on the item itself (§31: "Do NOT scatter tax percentages throughout the
-- application").
CREATE TABLE tax_profile (
    id                  TEXT PRIMARY KEY,       -- UUIDv7, minted by the cloud
    outlet_id           TEXT NOT NULL REFERENCES outlet(id),
    code                TEXT NOT NULL,          -- 'GST_5_RESTAURANT', stable machine code
    name                TEXT NOT NULL,
    -- Whether the menu price already contains the tax. Belongs to the profile,
    -- not the rule: a profile is inclusive or exclusive as a whole, and mixing
    -- the two across components of one profile has no coherent meaning.
    pricing_mode        TEXT NOT NULL CHECK (pricing_mode IN ('INCLUSIVE','EXCLUSIVE')),
    is_default          INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0,1)),
    is_active           INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0,1)),
    config_version      INTEGER NOT NULL
);

-- Tenant-scoped, never global: two outlets both having GST_5_RESTAURANT is normal.
CREATE UNIQUE INDEX idx_tax_profile_outlet_code ON tax_profile(outlet_id, code);

-- A component rate inside a profile, effective-dated. Child row travelling in
-- its parent's config bundle — the menu_item_variant precedent, not an
-- aggregate of its own.
CREATE TABLE tax_rule (
    id                  TEXT PRIMARY KEY,       -- UUIDv7, minted by the cloud
    tax_profile_id      TEXT NOT NULL REFERENCES tax_profile(id),
    compliance_version_id TEXT NOT NULL REFERENCES compliance_version(id),
    component           TEXT NOT NULL CHECK (component IN ('CGST','SGST','IGST','CESS')),
    -- Integer basis points, never a float and never a percentage string.
    -- 2.5% = 250. CLAUDE.md forbids floating point for money; a rate that
    -- multiplies money is money-adjacent enough to obey the same rule.
    rate_bps            INTEGER NOT NULL CHECK (rate_bps >= 0),
    effective_from      TEXT NOT NULL,          -- ISO8601 UTC
    effective_to        TEXT,                   -- NULL = open-ended
    config_version      INTEGER NOT NULL
);

CREATE INDEX idx_tax_rule_profile ON tax_rule(tax_profile_id, effective_from);

-- The seller identity printed on a GST invoice (§33). Effective-dated because
-- a GSTIN or trade name can change and historical invoices must keep the
-- identity that was current when they were issued.
CREATE TABLE outlet_fiscal_profile (
    id                  TEXT PRIMARY KEY,       -- UUIDv7, minted by the cloud
    outlet_id           TEXT NOT NULL REFERENCES outlet(id),
    legal_name          TEXT NOT NULL,
    trade_name          TEXT NOT NULL,
    address_line1       TEXT NOT NULL,
    address_line2       TEXT,
    city                TEXT NOT NULL,
    state_code          TEXT NOT NULL,          -- GST state code, '27' for Maharashtra
    state_name          TEXT NOT NULL,
    pincode             TEXT NOT NULL,
    gstin               TEXT NOT NULL,
    fssai_number        TEXT,
    invoice_footer_text TEXT,
    effective_from      TEXT NOT NULL,          -- ISO8601 UTC
    config_version      INTEGER NOT NULL
);

CREATE UNIQUE INDEX idx_outlet_fiscal_profile_outlet_from
    ON outlet_fiscal_profile(outlet_id, effective_from);

-- The DEFINITION of an invoice number series. The counter lives in
-- invoice_sequence below and never leaves this machine.
CREATE TABLE invoice_series (
    id                  TEXT PRIMARY KEY,       -- UUIDv7, minted by the cloud
    outlet_id           TEXT NOT NULL REFERENCES outlet(id),
    code                TEXT NOT NULL,          -- 'SALES', 'CREDIT_NOTE'
    -- Tokens: {FY} {YYYY} {MM} {DD} {OUTLET}. 'FY{FY}/{OUTLET}/' renders
    -- 'FY26/PNQ/' and with padding_width 6 yields FY26/PNQ/001423 — the
    -- human-facing format CLAUDE.md §Money/time/identifiers requires.
    prefix_template     TEXT NOT NULL,
    reset_policy        TEXT NOT NULL CHECK (reset_policy IN ('NEVER','FY','MONTH','DAY')),
    padding_width       INTEGER NOT NULL CHECK (padding_width BETWEEN 1 AND 12),
    is_active           INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0,1)),
    config_version      INTEGER NOT NULL
);

CREATE UNIQUE INDEX idx_invoice_series_outlet_code ON invoice_series(outlet_id, code);

-- A discount a cashier may apply. An ad-hoc discount is still governed by one
-- of these rows: the row is what carries the permission and reason
-- requirements (§28 bill.discount / bill.discount.override).
CREATE TABLE discount_definition (
    id                  TEXT PRIMARY KEY,       -- UUIDv7, minted by the cloud
    outlet_id           TEXT NOT NULL REFERENCES outlet(id),
    code                TEXT NOT NULL,
    name                TEXT NOT NULL,
    scope               TEXT NOT NULL CHECK (scope IN ('LINE','BILL')),
    method              TEXT NOT NULL CHECK (method IN ('PERCENT','AMOUNT')),
    -- Exactly one of these carries the value, decided by method. The CHECK
    -- makes a half-populated row unrepresentable rather than a runtime
    -- surprise in the tax engine.
    value_bps           INTEGER CHECK (value_bps IS NULL OR value_bps BETWEEN 0 AND 10000),
    value_paise         INTEGER CHECK (value_paise IS NULL OR value_paise >= 0),
    max_discount_paise  INTEGER,                -- cap for PERCENT; NULL = uncapped
    required_permission TEXT,                   -- NULL = any user who can bill
    requires_reason     INTEGER NOT NULL DEFAULT 0 CHECK (requires_reason IN (0,1)),
    is_active           INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0,1)),
    effective_from      TEXT NOT NULL,
    effective_to        TEXT,
    config_version      INTEGER NOT NULL,
    CHECK ((method = 'PERCENT' AND value_bps IS NOT NULL AND value_paise IS NULL)
        OR (method = 'AMOUNT'  AND value_paise IS NOT NULL AND value_bps IS NULL))
);

CREATE UNIQUE INDEX idx_discount_definition_outlet_code ON discount_definition(outlet_id, code);

------------------------------------------------------------------------------
-- EDGE-LOCAL (SQLite only — no Postgres mirror, no AggregateType, no direction)
------------------------------------------------------------------------------

-- The counter behind invoice_series. Deliberately absent from AggregateType,
-- for the same reason print_job and kot_status_history are: giving it a sync
-- direction would make the cloud a second writer of invoice numbers, and §33
-- requires numbering be concurrency-safe with duplicates never generated.
-- One outlet, one SQLite file, one writer (ADR-013) — so an atomic UPDATE ...
-- RETURNING against this table is the whole concurrency story, and it works
-- with the uplink down.
CREATE TABLE invoice_sequence (
    series_id           TEXT NOT NULL REFERENCES invoice_series(id),
    -- The reset bucket the counter belongs to, derived from reset_policy:
    -- 'ALL' | 'FY2026' | '2026-08' | '2026-08-12'. Making it explicit means a
    -- policy change starts a fresh bucket rather than silently rewinding a
    -- live counter.
    period_key          TEXT NOT NULL,
    last_value          INTEGER NOT NULL DEFAULT 0 CHECK (last_value >= 0),
    updated_at          TEXT NOT NULL,
    PRIMARY KEY (series_id, period_key)
);

------------------------------------------------------------------------------
-- EDGE-AUTHORITATIVE (edge→cloud, append-only)
------------------------------------------------------------------------------

-- A GST invoice (§33). Split bills are N invoices over one order sharing a
-- split_group_id — each part is a real, independently numbered, independently
-- payable invoice, because that is what the customer physically receives.
-- There is deliberately no bill_split table.
CREATE TABLE invoice (
    id                  TEXT PRIMARY KEY,       -- UUIDv7, minted by the edge
    outlet_id           TEXT NOT NULL REFERENCES outlet(id),
    order_id            TEXT NOT NULL REFERENCES "order"(id),

    -- NULL for an unsplit bill. Non-NULL rows sharing a value are the parts of
    -- one split; the conservation property (Σ split lines = order lines,
    -- exactly) is asserted by the §66 financial suite, not by SQL, because it
    -- spans rows this constraint layer cannot see.
    split_group_id      TEXT,
    split_index         INTEGER NOT NULL DEFAULT 1 CHECK (split_index >= 1),
    split_count         INTEGER NOT NULL DEFAULT 1 CHECK (split_count >= 1),

    series_id           TEXT NOT NULL REFERENCES invoice_series(id),
    invoice_number      TEXT NOT NULL,
    invoice_date        TEXT NOT NULL,          -- ISO8601 UTC (CLAUDE.md §Time)
    business_date       TEXT NOT NULL,          -- outlet-local YYYY-MM-DD; may cross midnight

    status              TEXT NOT NULL CHECK (status IN ('ISSUED','CANCELLED')),
    cancelled_reason    TEXT,
    cancelled_at        TEXT,

    -- Buyer. All optional: a walk-in dine-in customer supplies none of it.
    customer_name       TEXT,
    customer_phone      TEXT,
    customer_gstin      TEXT,
    place_of_supply_state_code TEXT NOT NULL,

    -- Money. Every field integer paise (CLAUDE.md §Money).
    subtotal_paise      INTEGER NOT NULL,       -- gross of lines before discount
    discount_paise      INTEGER NOT NULL DEFAULT 0,
    taxable_value_paise INTEGER NOT NULL,
    cgst_paise          INTEGER NOT NULL DEFAULT 0,
    sgst_paise          INTEGER NOT NULL DEFAULT 0,
    igst_paise          INTEGER NOT NULL DEFAULT 0,
    cess_paise          INTEGER NOT NULL DEFAULT 0,
    round_off_paise     INTEGER NOT NULL DEFAULT 0,
    grand_total_paise   INTEGER NOT NULL,

    -- Reproducibility (§31). The resolved rules AND the seller identity as
    -- they stood at issue time, so reprinting a six-month-old bill after a
    -- rate change or a GSTIN change produces the original document.
    compliance_version_id TEXT NOT NULL REFERENCES compliance_version(id),
    tax_snapshot_json   TEXT NOT NULL,
    fiscal_profile_json TEXT NOT NULL,

    -- ECO fields (§32). MODELLED NOW, REPORTED LATER — Milestone 3 EXCLUDES
    -- names ECO reporting outputs explicitly. Direct and ECO supplies must
    -- never be combined in reporting, which is only possible if the
    -- classification is captured at issue time rather than inferred later.
    channel             TEXT NOT NULL,          -- POS | QR | AGGREGATOR_* | DIRECT
    tax_liability_party TEXT NOT NULL CHECK (tax_liability_party IN ('RESTAURANT','ECO')),
    eco_operator_name   TEXT,
    eco_operator_gstin  TEXT,
    supply_classification TEXT,

    created_by_user_id  TEXT NOT NULL REFERENCES app_user(id),
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    version             INTEGER NOT NULL DEFAULT 1,
    sync_status         TEXT NOT NULL DEFAULT 'PENDING'
                          CHECK (sync_status IN ('PENDING','SYNCED','FAILED')),

    -- ADR-016 rounding policy, enforced in storage rather than trusted to the
    -- caller: tax is summed per component across the invoice and rounded
    -- half-up once, then the grand total is rounded to the nearest rupee with
    -- the delta recorded here. Both halves of that policy are checkable, so a
    -- bill that violates them is unstorable rather than merely untested.
    -- (Table-level constraints must follow every column definition — placing
    -- them beside the money columns is a syntax error in SQLite.)
    CHECK (grand_total_paise =
             taxable_value_paise + cgst_paise + sgst_paise + igst_paise
             + cess_paise + round_off_paise),
    CHECK (round_off_paise BETWEEN -50 AND 50),
    CHECK (grand_total_paise % 100 = 0)
);

-- §33: "Never generate duplicate invoice numbers." Scoped to the outlet and
-- series, never global — two outlets issuing FY26/.../000001 on the same day
-- is correct behaviour, not a collision.
CREATE UNIQUE INDEX idx_invoice_outlet_series_number
    ON invoice(outlet_id, series_id, invoice_number);
CREATE INDEX idx_invoice_order ON invoice(order_id);
CREATE INDEX idx_invoice_split_group ON invoice(split_group_id) WHERE split_group_id IS NOT NULL;
CREATE INDEX idx_invoice_sync ON invoice(sync_status) WHERE sync_status <> 'SYNCED';
CREATE INDEX idx_invoice_business_date ON invoice(outlet_id, business_date);

-- A line on the invoice. Child row inside its invoice's payload, not an
-- aggregate. order_item_id is what makes the split conservation property
-- checkable: every order line must appear across the group exactly once in
-- total quantity.
CREATE TABLE invoice_line (
    id                  TEXT PRIMARY KEY,       -- UUIDv7, minted by the edge
    invoice_id          TEXT NOT NULL REFERENCES invoice(id),
    order_item_id       TEXT NOT NULL REFERENCES order_item(id),
    line_no             INTEGER NOT NULL,
    description         TEXT NOT NULL,          -- snapshot; never re-read from live menu
    hsn_sac             TEXT,
    quantity            INTEGER NOT NULL CHECK (quantity > 0),
    unit_price_paise    INTEGER NOT NULL,
    gross_paise         INTEGER NOT NULL,
    discount_paise      INTEGER NOT NULL DEFAULT 0,
    taxable_value_paise INTEGER NOT NULL,
    tax_profile_id      TEXT NOT NULL REFERENCES tax_profile(id),
    cgst_rate_bps       INTEGER NOT NULL DEFAULT 0,
    cgst_paise          INTEGER NOT NULL DEFAULT 0,
    sgst_rate_bps       INTEGER NOT NULL DEFAULT 0,
    sgst_paise          INTEGER NOT NULL DEFAULT 0,
    igst_rate_bps       INTEGER NOT NULL DEFAULT 0,
    igst_paise          INTEGER NOT NULL DEFAULT 0,
    cess_rate_bps       INTEGER NOT NULL DEFAULT 0,
    cess_paise          INTEGER NOT NULL DEFAULT 0,
    total_paise         INTEGER NOT NULL,
    UNIQUE (invoice_id, line_no)
);

CREATE INDEX idx_invoice_line_invoice ON invoice_line(invoice_id);
CREATE INDEX idx_invoice_line_order_item ON invoice_line(order_item_id);

-- Cashier-specific register (§39). Declared before payment because a cash
-- payment references its shift. Expected cash is derived from movements;
-- actual is counted by a human; variance is the difference and needs a reason.
CREATE TABLE cash_shift (
    id                  TEXT PRIMARY KEY,       -- UUIDv7, minted by the edge
    outlet_id           TEXT NOT NULL REFERENCES outlet(id),
    device_id           TEXT NOT NULL REFERENCES device(id),
    cashier_user_id     TEXT NOT NULL REFERENCES app_user(id),
    status              TEXT NOT NULL CHECK (status IN ('OPEN','CLOSED')),
    opened_at           TEXT NOT NULL,
    opening_cash_paise  INTEGER NOT NULL CHECK (opening_cash_paise >= 0),
    closed_at           TEXT,
    expected_cash_paise INTEGER,
    actual_cash_paise   INTEGER,
    variance_paise      INTEGER,
    variance_reason     TEXT,
    business_date       TEXT NOT NULL,          -- outlet-local YYYY-MM-DD
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    version             INTEGER NOT NULL DEFAULT 1,
    sync_status         TEXT NOT NULL DEFAULT 'PENDING'
                          CHECK (sync_status IN ('PENDING','SYNCED','FAILED')),
    -- A closed shift is fully accounted for, and §39 requires a reason for a
    -- variance. Unrepresentable rather than validated-somewhere-hopefully.
    CHECK (status = 'OPEN' OR (closed_at IS NOT NULL
                               AND expected_cash_paise IS NOT NULL
                               AND actual_cash_paise IS NOT NULL
                               AND variance_paise IS NOT NULL)),
    CHECK (variance_paise IS NULL OR variance_paise = 0 OR variance_reason IS NOT NULL)
);

-- One open shift per cashier per device at a time.
CREATE UNIQUE INDEX idx_cash_shift_open_device_cashier
    ON cash_shift(device_id, cashier_user_id) WHERE status = 'OPEN';
CREATE INDEX idx_cash_shift_sync ON cash_shift(sync_status) WHERE sync_status <> 'SYNCED';

-- A tender. §34: never order.payment_method = 'UPI'. A ₹2,000 bill settled as
-- ₹500 cash + ₹1,000 UPI + ₹500 card is three rows here.
--
-- APPEND-ONLY (docs/spec/payments.md §Conflict policy). Nothing updates a
-- captured payment: a void or refund inserts a new row pointing at the
-- original through reverses_payment_id. Only sync_status and version are ever
-- rewritten in place, and neither is financial data.
CREATE TABLE payment (
    id                  TEXT PRIMARY KEY,       -- UUIDv7, minted by the edge
    outlet_id           TEXT NOT NULL REFERENCES outlet(id),
    order_id            TEXT NOT NULL REFERENCES "order"(id),
    cash_shift_id       TEXT REFERENCES cash_shift(id),  -- NULL for non-cash outside a shift
    method              TEXT NOT NULL CHECK (method IN
                          ('CASH','UPI','CREDIT_CARD','DEBIT_CARD','WALLET','GIFT_CARD',
                           'LOYALTY_POINTS','BANK_TRANSFER','AGGREGATOR_PAID','HOUSE_ACCOUNT','CREDIT')),
    -- Milestone 3 is cash + split only; gateway capture lands in Milestone 7.
    -- The states are modelled now so a Razorpay attempt has somewhere to go.
    status              TEXT NOT NULL CHECK (status IN
                          ('PENDING','CAPTURED','FAILED','VOIDED','REFUNDED')),
    amount_paise        INTEGER NOT NULL,       -- negative on a reversal row
    tendered_paise      INTEGER,                -- cash only; what the customer handed over
    change_paise        INTEGER,                -- cash only
    reference           TEXT,                   -- UTR / auth code / manual card slip number
    external_id         TEXT,                   -- gateway id; Milestone 7
    reverses_payment_id TEXT REFERENCES payment(id),
    captured_at         TEXT,
    created_by_user_id  TEXT NOT NULL REFERENCES app_user(id),
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    version             INTEGER NOT NULL DEFAULT 1,
    sync_status         TEXT NOT NULL DEFAULT 'PENDING'
                          CHECK (sync_status IN ('PENDING','SYNCED','FAILED')),
    CHECK (tendered_paise IS NULL OR method = 'CASH'),
    CHECK (reverses_payment_id IS NULL OR amount_paise <= 0)
);

CREATE INDEX idx_payment_order ON payment(order_id);
CREATE INDEX idx_payment_shift ON payment(cash_shift_id);
CREATE INDEX idx_payment_sync ON payment(sync_status) WHERE sync_status <> 'SYNCED';

-- How one tender settles against one or more invoices. This is what makes
-- split payment and split bill compose: one card swipe can settle two parts of
-- a split group, and one part can be settled by three tenders.
CREATE TABLE payment_allocation (
    id                  TEXT PRIMARY KEY,       -- UUIDv7, minted by the edge
    payment_id          TEXT NOT NULL REFERENCES payment(id),
    invoice_id          TEXT NOT NULL REFERENCES invoice(id),
    amount_paise        INTEGER NOT NULL,
    UNIQUE (payment_id, invoice_id)
);

CREATE INDEX idx_payment_allocation_invoice ON payment_allocation(invoice_id);

-- Every movement of physical cash through the drawer (§39). Child row inside
-- the shift's payload. Append-only: a correction is another movement.
CREATE TABLE cash_movement (
    id                  TEXT PRIMARY KEY,       -- UUIDv7, minted by the edge
    cash_shift_id       TEXT NOT NULL REFERENCES cash_shift(id),
    kind                TEXT NOT NULL CHECK (kind IN
                          ('OPENING_FLOAT','CASH_SALE','CASH_REFUND','PAID_IN','PAID_OUT')),
    amount_paise        INTEGER NOT NULL,       -- signed: PAID_OUT and CASH_REFUND negative
    reason              TEXT,
    payment_id          TEXT REFERENCES payment(id),
    created_by_user_id  TEXT NOT NULL REFERENCES app_user(id),
    created_at          TEXT NOT NULL,
    CHECK (kind NOT IN ('PAID_IN','PAID_OUT') OR reason IS NOT NULL)
);

CREATE INDEX idx_cash_movement_shift ON cash_movement(cash_shift_id);

------------------------------------------------------------------------------
-- ORDER: short human-facing display number
------------------------------------------------------------------------------

-- Closes the M2 finding that a printed KOT carries the order's raw UUID.
-- CLAUDE.md §Money/time/identifiers requires human-facing numbers be short
-- ('Order #A184') and forbids exposing sequential PKs as security identifiers,
-- so this is a display string minted alongside the order, not the PK.
--
-- Nullable in storage because SQLite cannot add a NOT NULL column to a
-- populated table without a full table rebuild, and a rebuild of "order" —
-- with order_item, kot and now invoice referencing it — is a worse risk than
-- a nullable column. The edge create path always populates it; readers fall
-- back to the id ONLY for rows written before 0.4.0.
ALTER TABLE "order" ADD COLUMN display_number TEXT;

CREATE INDEX idx_order_display_number ON "order"(outlet_id, display_number);
