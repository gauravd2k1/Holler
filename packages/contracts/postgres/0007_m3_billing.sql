-- Holler Cloud PostgreSQL — Milestone 3 billing: tax engine, GST invoice,
-- discounts, split bills, split payments, cash shift, invoice numbering.
-- Mirrors sqlite/0006_m3_billing.sql. Contracts 0.4.0, ADR-016.
--
-- No DEFAULT gen_random_uuid() on any id below, following 0002's and 0006's
-- precedent rather than 0001's: §74 and the contract review rubric require
-- app-generated UUIDv7, and a DB-side random default silently produces a
-- UUIDv4 whenever a writer forgets to supply one.
--
-- Authority (§50.1):
--   compliance_version, tax_profile, tax_rule, outlet_fiscal_profile,
--   invoice_series, discount_definition   -> CLOUD_TO_EDGE config. The cloud
--                                            owns them and bumps
--                                            outlet.config_version.
--   invoice, invoice_line, payment,
--   payment_allocation, cash_shift,
--   cash_movement                         -> EDGE_TO_CLOUD. The cloud only
--                                            REPLAYS these. No handler here
--                                            issues an invoice number,
--                                            transitions an invoice, or
--                                            captures a payment — the same
--                                            rule ADR-014 set for kot.status.
--
-- DELIBERATELY ABSENT: invoice_sequence. The counter behind a series is
-- edge-local (sqlite/0006 only) and has no mirror here, exactly as print_job
-- and kot_status_history have none. Mirroring it would make the cloud a second
-- writer of invoice numbers, which is precisely what §33's "never generate
-- duplicate invoice numbers" forbids. The issued number arrives on the
-- invoice; the counter that produced it stays at the outlet.
--
-- device_id columns are bare UUIDs with no foreign key, following the
-- "order".device_id precedent in 0001 — there is no cloud device table until
-- ADR-017 adds one for enrollment.

------------------------------------------------------------------------------
-- CONFIG (cloud→edge)
------------------------------------------------------------------------------

CREATE TABLE compliance_version (
    id              UUID PRIMARY KEY,
    outlet_id       UUID NOT NULL REFERENCES outlet(id),
    label           TEXT NOT NULL,
    effective_from  TIMESTAMPTZ NOT NULL,
    notes           TEXT,
    config_version  INTEGER NOT NULL
);

CREATE UNIQUE INDEX idx_compliance_version_outlet_label ON compliance_version(outlet_id, label);

CREATE TABLE tax_profile (
    id              UUID PRIMARY KEY,
    outlet_id       UUID NOT NULL REFERENCES outlet(id),
    code            TEXT NOT NULL,
    name            TEXT NOT NULL,
    -- Belongs to the profile, not the rule: a profile is inclusive or
    -- exclusive as a whole (§31).
    pricing_mode    TEXT NOT NULL CHECK (pricing_mode IN ('INCLUSIVE','EXCLUSIVE')),
    is_default      BOOLEAN NOT NULL DEFAULT false,
    is_active       BOOLEAN NOT NULL DEFAULT true,
    config_version  INTEGER NOT NULL
);

-- Tenant-scoped, never global: two outlets both having GST_5_RESTAURANT is normal.
CREATE UNIQUE INDEX idx_tax_profile_outlet_code ON tax_profile(outlet_id, code);

CREATE TABLE tax_rule (
    id                    UUID PRIMARY KEY,
    tax_profile_id        UUID NOT NULL REFERENCES tax_profile(id),
    compliance_version_id UUID NOT NULL REFERENCES compliance_version(id),
    component             TEXT NOT NULL CHECK (component IN ('CGST','SGST','IGST','CESS')),
    -- Integer basis points, never a float: 2.5% = 250. CLAUDE.md forbids
    -- floating point for money, and a rate that multiplies money obeys the
    -- same rule.
    rate_bps              INTEGER NOT NULL CHECK (rate_bps >= 0),
    effective_from        TIMESTAMPTZ NOT NULL,
    effective_to          TIMESTAMPTZ,
    config_version        INTEGER NOT NULL
);

CREATE INDEX idx_tax_rule_profile ON tax_rule(tax_profile_id, effective_from);

CREATE TABLE outlet_fiscal_profile (
    id                  UUID PRIMARY KEY,
    outlet_id           UUID NOT NULL REFERENCES outlet(id),
    legal_name          TEXT NOT NULL,
    trade_name          TEXT NOT NULL,
    address_line1       TEXT NOT NULL,
    address_line2       TEXT,
    city                TEXT NOT NULL,
    state_code          TEXT NOT NULL,
    state_name          TEXT NOT NULL,
    pincode             TEXT NOT NULL,
    gstin               TEXT NOT NULL,
    fssai_number        TEXT,
    invoice_footer_text TEXT,
    effective_from      TIMESTAMPTZ NOT NULL,
    config_version      INTEGER NOT NULL
);

CREATE UNIQUE INDEX idx_outlet_fiscal_profile_outlet_from
    ON outlet_fiscal_profile(outlet_id, effective_from);

CREATE TABLE invoice_series (
    id              UUID PRIMARY KEY,
    outlet_id       UUID NOT NULL REFERENCES outlet(id),
    code            TEXT NOT NULL,
    -- Tokens: {FY} {YYYY} {MM} {DD} {OUTLET}.
    prefix_template TEXT NOT NULL,
    reset_policy    TEXT NOT NULL CHECK (reset_policy IN ('NEVER','FY','MONTH','DAY')),
    padding_width   INTEGER NOT NULL CHECK (padding_width BETWEEN 1 AND 12),
    is_active       BOOLEAN NOT NULL DEFAULT true,
    config_version  INTEGER NOT NULL
);

CREATE UNIQUE INDEX idx_invoice_series_outlet_code ON invoice_series(outlet_id, code);

CREATE TABLE discount_definition (
    id                  UUID PRIMARY KEY,
    outlet_id           UUID NOT NULL REFERENCES outlet(id),
    code                TEXT NOT NULL,
    name                TEXT NOT NULL,
    scope               TEXT NOT NULL CHECK (scope IN ('LINE','BILL')),
    method              TEXT NOT NULL CHECK (method IN ('PERCENT','AMOUNT')),
    value_bps           INTEGER CHECK (value_bps IS NULL OR value_bps BETWEEN 0 AND 10000),
    value_paise         INTEGER CHECK (value_paise IS NULL OR value_paise >= 0),
    max_discount_paise  INTEGER,
    required_permission TEXT,
    requires_reason     BOOLEAN NOT NULL DEFAULT false,
    is_active           BOOLEAN NOT NULL DEFAULT true,
    effective_from      TIMESTAMPTZ NOT NULL,
    effective_to        TIMESTAMPTZ,
    config_version      INTEGER NOT NULL,
    -- A half-populated discount row is unrepresentable rather than a runtime
    -- surprise in the tax engine ("20% or ₹50?" has no defined answer).
    CHECK ((method = 'PERCENT' AND value_bps IS NOT NULL AND value_paise IS NULL)
        OR (method = 'AMOUNT'  AND value_paise IS NOT NULL AND value_bps IS NULL))
);

CREATE UNIQUE INDEX idx_discount_definition_outlet_code ON discount_definition(outlet_id, code);

------------------------------------------------------------------------------
-- EDGE-AUTHORITATIVE (edge→cloud, replay only, append-only)
------------------------------------------------------------------------------

-- Split bills are N invoices over one order sharing a split_group_id. Each
-- part is a real, independently numbered, independently payable invoice,
-- because that is what the customer physically receives. There is deliberately
-- no bill_split table.
CREATE TABLE invoice (
    id                  UUID PRIMARY KEY,          -- assigned edge-side (UUIDv7)
    outlet_id           UUID NOT NULL REFERENCES outlet(id),
    order_id            UUID NOT NULL REFERENCES "order"(id),

    split_group_id      UUID,
    split_index         INTEGER NOT NULL DEFAULT 1 CHECK (split_index >= 1),
    split_count         INTEGER NOT NULL DEFAULT 1 CHECK (split_count >= 1),

    series_id           UUID NOT NULL REFERENCES invoice_series(id),
    invoice_number      TEXT NOT NULL,
    invoice_date        TIMESTAMPTZ NOT NULL,
    business_date       DATE NOT NULL,             -- outlet-local; may cross midnight

    status              TEXT NOT NULL CHECK (status IN ('ISSUED','CANCELLED')),
    cancelled_reason    TEXT,
    cancelled_at        TIMESTAMPTZ,

    customer_name       TEXT,
    customer_phone      TEXT,
    customer_gstin      TEXT,
    place_of_supply_state_code TEXT NOT NULL,

    subtotal_paise      INTEGER NOT NULL,
    discount_paise      INTEGER NOT NULL DEFAULT 0,
    taxable_value_paise INTEGER NOT NULL,
    cgst_paise          INTEGER NOT NULL DEFAULT 0,
    sgst_paise          INTEGER NOT NULL DEFAULT 0,
    igst_paise          INTEGER NOT NULL DEFAULT 0,
    cess_paise          INTEGER NOT NULL DEFAULT 0,
    round_off_paise     INTEGER NOT NULL DEFAULT 0,
    grand_total_paise   INTEGER NOT NULL,

    -- Reproducibility (§31): the resolved rules AND the seller identity as
    -- they stood at issue time, so reprinting a six-month-old bill after a
    -- rate or GSTIN change produces the original document.
    compliance_version_id UUID NOT NULL REFERENCES compliance_version(id),
    tax_snapshot        JSONB NOT NULL,
    fiscal_profile      JSONB NOT NULL,

    -- ECO fields (§32). MODELLED NOW, REPORTED LATER — Milestone 3 EXCLUDES
    -- names ECO reporting outputs explicitly. Direct and ECO supplies must
    -- never be combined in reporting, which is only possible if the
    -- classification is captured at issue time rather than inferred later.
    channel             TEXT NOT NULL,
    tax_liability_party TEXT NOT NULL CHECK (tax_liability_party IN ('RESTAURANT','ECO')),
    eco_operator_name   TEXT,
    eco_operator_gstin  TEXT,
    supply_classification TEXT,

    created_by_user_id  UUID NOT NULL REFERENCES app_user(id),
    created_at          TIMESTAMPTZ NOT NULL,
    updated_at          TIMESTAMPTZ NOT NULL,
    version             INTEGER NOT NULL DEFAULT 1,
    received_at         TIMESTAMPTZ NOT NULL DEFAULT now(),  -- when cloud received the replay

    -- ADR-016 rounding policy, mirrored verbatim from sqlite/0006 and enforced
    -- in storage rather than trusted to the caller. Tax is summed per
    -- component across the invoice and rounded half-up once; the grand total
    -- is then rounded to the nearest rupee with the delta recorded here.
    -- A replayed bill that violates the policy is rejected at ingest rather
    -- than landing in the cloud as an unreconcilable row.
    CHECK (grand_total_paise =
             taxable_value_paise + cgst_paise + sgst_paise + igst_paise
             + cess_paise + round_off_paise),
    CHECK (round_off_paise BETWEEN -50 AND 50),
    CHECK (grand_total_paise % 100 = 0)
);

-- §33: "Never generate duplicate invoice numbers." Scoped to outlet+series,
-- never global — two outlets issuing FY26/.../000001 on one day is correct.
CREATE UNIQUE INDEX idx_invoice_outlet_series_number
    ON invoice(outlet_id, series_id, invoice_number);
CREATE INDEX idx_invoice_order ON invoice(order_id);
CREATE INDEX idx_invoice_split_group ON invoice(split_group_id) WHERE split_group_id IS NOT NULL;
CREATE INDEX idx_invoice_business_date ON invoice(outlet_id, business_date);

CREATE TABLE invoice_line (
    id                  UUID PRIMARY KEY,
    invoice_id          UUID NOT NULL REFERENCES invoice(id),
    order_item_id       UUID NOT NULL REFERENCES order_item(id),
    line_no             INTEGER NOT NULL,
    description         TEXT NOT NULL,             -- snapshot; never re-read from live menu
    hsn_sac             TEXT,
    quantity            INTEGER NOT NULL CHECK (quantity > 0),
    unit_price_paise    INTEGER NOT NULL,
    gross_paise         INTEGER NOT NULL,
    discount_paise      INTEGER NOT NULL DEFAULT 0,
    taxable_value_paise INTEGER NOT NULL,
    tax_profile_id      UUID NOT NULL REFERENCES tax_profile(id),
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

CREATE TABLE cash_shift (
    id                  UUID PRIMARY KEY,
    outlet_id           UUID NOT NULL REFERENCES outlet(id),
    device_id           UUID NOT NULL,             -- no FK; see header
    cashier_user_id     UUID NOT NULL REFERENCES app_user(id),
    status              TEXT NOT NULL CHECK (status IN ('OPEN','CLOSED')),
    opened_at           TIMESTAMPTZ NOT NULL,
    opening_cash_paise  INTEGER NOT NULL CHECK (opening_cash_paise >= 0),
    closed_at           TIMESTAMPTZ,
    expected_cash_paise INTEGER,
    actual_cash_paise   INTEGER,
    variance_paise      INTEGER,
    variance_reason     TEXT,
    business_date       DATE NOT NULL,
    created_at          TIMESTAMPTZ NOT NULL,
    updated_at          TIMESTAMPTZ NOT NULL,
    version             INTEGER NOT NULL DEFAULT 1,
    received_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- A closed shift is fully accounted for, and §39 requires a reason for a
    -- variance. Unrepresentable rather than validated-somewhere-hopefully.
    CHECK (status = 'OPEN' OR (closed_at IS NOT NULL
                               AND expected_cash_paise IS NOT NULL
                               AND actual_cash_paise IS NOT NULL
                               AND variance_paise IS NOT NULL)),
    CHECK (variance_paise IS NULL OR variance_paise = 0 OR variance_reason IS NOT NULL)
);

CREATE INDEX idx_cash_shift_outlet_business_date ON cash_shift(outlet_id, business_date);

-- §34: never order.payment_method = 'UPI'. A ₹2,000 bill settled as ₹500 cash
-- + ₹1,000 UPI + ₹500 card is three rows here.
--
-- APPEND-ONLY (docs/spec/payments.md §Conflict policy). Nothing updates a
-- captured payment: a void or refund inserts a new row pointing at the
-- original through reverses_payment_id.
CREATE TABLE payment (
    id                  UUID PRIMARY KEY,
    outlet_id           UUID NOT NULL REFERENCES outlet(id),
    order_id            UUID NOT NULL REFERENCES "order"(id),
    cash_shift_id       UUID REFERENCES cash_shift(id),
    method              TEXT NOT NULL CHECK (method IN
                          ('CASH','UPI','CREDIT_CARD','DEBIT_CARD','WALLET','GIFT_CARD',
                           'LOYALTY_POINTS','BANK_TRANSFER','AGGREGATOR_PAID','HOUSE_ACCOUNT','CREDIT')),
    -- Milestone 3 is cash + split only; gateway capture lands in Milestone 7.
    -- The states are modelled now so a Razorpay attempt has somewhere to go.
    status              TEXT NOT NULL CHECK (status IN
                          ('PENDING','CAPTURED','FAILED','VOIDED','REFUNDED')),
    amount_paise        INTEGER NOT NULL,          -- negative on a reversal row
    tendered_paise      INTEGER,
    change_paise        INTEGER,
    reference           TEXT,
    external_id         TEXT,
    reverses_payment_id UUID REFERENCES payment(id),
    captured_at         TIMESTAMPTZ,
    created_by_user_id  UUID NOT NULL REFERENCES app_user(id),
    created_at          TIMESTAMPTZ NOT NULL,
    updated_at          TIMESTAMPTZ NOT NULL,
    version             INTEGER NOT NULL DEFAULT 1,
    received_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (tendered_paise IS NULL OR method = 'CASH'),
    CHECK (reverses_payment_id IS NULL OR amount_paise <= 0)
);

CREATE INDEX idx_payment_order ON payment(order_id);
CREATE INDEX idx_payment_shift ON payment(cash_shift_id);

-- How one tender settles against one or more invoices. This is what makes
-- split payment and split bill compose: one card swipe can settle two parts of
-- a split group, and one part can be settled by three tenders.
CREATE TABLE payment_allocation (
    id           UUID PRIMARY KEY,
    payment_id   UUID NOT NULL REFERENCES payment(id),
    invoice_id   UUID NOT NULL REFERENCES invoice(id),
    amount_paise INTEGER NOT NULL,
    UNIQUE (payment_id, invoice_id)
);

CREATE INDEX idx_payment_allocation_invoice ON payment_allocation(invoice_id);

CREATE TABLE cash_movement (
    id                 UUID PRIMARY KEY,
    cash_shift_id      UUID NOT NULL REFERENCES cash_shift(id),
    kind               TEXT NOT NULL CHECK (kind IN
                         ('OPENING_FLOAT','CASH_SALE','CASH_REFUND','PAID_IN','PAID_OUT')),
    amount_paise       INTEGER NOT NULL,          -- signed: PAID_OUT/CASH_REFUND negative
    reason             TEXT,
    payment_id         UUID REFERENCES payment(id),
    created_by_user_id UUID NOT NULL REFERENCES app_user(id),
    created_at         TIMESTAMPTZ NOT NULL,
    CHECK (kind NOT IN ('PAID_IN','PAID_OUT') OR reason IS NOT NULL)
);

CREATE INDEX idx_cash_movement_shift ON cash_movement(cash_shift_id);

------------------------------------------------------------------------------
-- ORDER: short human-facing display number
------------------------------------------------------------------------------

-- Closes the M2 finding that a printed KOT carries the order's raw UUID.
-- CLAUDE.md §Money/time/identifiers requires human-facing numbers be short
-- ('Order #A184') and forbids exposing sequential PKs as security identifiers,
-- so this is a display string minted edge-side alongside the order, not the PK.
-- Nullable to match sqlite/0006, where SQLite cannot add a NOT NULL column to
-- a populated table without a full rebuild of "order".
ALTER TABLE "order" ADD COLUMN display_number TEXT;

CREATE INDEX idx_order_display_number ON "order"(outlet_id, display_number);
