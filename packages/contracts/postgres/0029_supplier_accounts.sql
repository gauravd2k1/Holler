-- Holler Cloud PostgreSQL — supplier accounts and the PO approval limit.
-- Contracts 0.6.0, ADR-019.
--
-- CLOUD-ONLY. No SQLite mirror, deliberately not AggregateTypes — the
-- refresh_token / device_credential / ledger_replay_gap precedent.
--
-- ---------------------------------------------------------------------------
-- WHY THIS IS ITS OWN FILE RATHER THAN A SECTION INSIDE 0028
-- ---------------------------------------------------------------------------
--
-- Because SINGLE_STORE_MIGRATIONS (edge/database/src/migrations.rs) pairs
-- migrations BY FILENAME STEM. Cloud-only tables sitting inside a mirrored
-- file are invisible to that lint: 0028 has an SQLite twin, so the pair
-- passes, and the asymmetry inside it is never declared and never checked.
--
-- Putting them here forces the declaration, and the lint fails if anyone ever
-- adds an SQLite mirror. Same reasoning as sqlite/0028_grn_sequence.sql, from
-- the other side.

-- ---------------------------------------------------------------------------
-- role.po_approval_limit_paise — the AMOUNT half of approval
-- ---------------------------------------------------------------------------
--
-- Two independent gates, and both are needed. The procurement.approve
-- PERMISSION decides who may approve at all; this LIMIT decides up to what
-- value. A permission alone makes every approver unlimited, which is not how
-- any restaurant group delegates spend.
--
-- NULL MEANS "MAY NOT APPROVE ANY AMOUNT". Absence is never read as unlimited
-- — contracts 0.4.7's printer_role rule, where a printer with no role row is a
-- candidate for neither path. A NULL limit that defaulted to unlimited would
-- turn every unconfigured role into an unbounded approver, silently.
--
-- ROLE-LEVEL, NOT PER-USER, and `role` is tenant-scoped
-- (UNIQUE (tenant_id, code), 0002), so two tenants of different scale hold
-- genuinely separate limits. FILED TRIGGER: the first request for a
-- per-person ceiling. That is a per-user_role column and a migration, not a
-- free change, and the decision is recorded in ADR-019 so nobody re-derives it.
--
-- Cloud-only because THERE IS NO role TABLE IN SQLITE — the edge flattens
-- permissions into app_user.permissions_json and never models a role at all.
-- Correct rather than merely tolerated: PO approval happens in the admin,
-- against the cloud, by someone who is not standing at a till. The edge never
-- approves a purchase order and must never be able to.
ALTER TABLE role ADD COLUMN po_approval_limit_paise BIGINT
    CHECK (po_approval_limit_paise IS NULL OR po_approval_limit_paise >= 0);

-- ---------------------------------------------------------------------------
-- supplier_invoice / supplier_credit — MODELLED, NOT ACTED ON (M7)
-- ---------------------------------------------------------------------------
--
-- The fields land now so the shape does not change when accounts posting and
-- settlement arrive in M7 (the yield_factor_ppm / unit_cost_paise precedent).
-- Nothing in M5 writes a payment against these; T1 creates and lists them only.
--
-- Cloud-only for the reason supplier accounting is a back-office function: an
-- outlet does not reconcile a supplier ledger with the uplink down, and giving
-- the edge a copy would create a second authority over money owed.
CREATE TABLE supplier_invoice (
    id                  UUID PRIMARY KEY,
    tenant_id           UUID NOT NULL REFERENCES tenant(id),
    outlet_id           UUID NOT NULL REFERENCES outlet(id),
    supplier_id         UUID NOT NULL REFERENCES supplier(id),
    grn_id              UUID REFERENCES goods_receipt_note(id),
    supplier_invoice_no TEXT NOT NULL,
    invoice_date        DATE NOT NULL,
    due_date            DATE,
    subtotal_paise      BIGINT NOT NULL CHECK (subtotal_paise >= 0),
    tax_paise           BIGINT NOT NULL DEFAULT 0 CHECK (tax_paise >= 0),
    total_paise         BIGINT NOT NULL CHECK (total_paise >= 0),

    -- DEFERRED TO M7 AND INERT UNTIL THEN. Only 'RECEIVED' is written in M5;
    -- the settlement states exist so the column does not change shape later.
    -- Nothing transitions this in M5 and no M5 code path may.
    status              TEXT NOT NULL DEFAULT 'RECEIVED' CHECK (status IN
                            ('RECEIVED','APPROVED','PART_PAID','PAID','DISPUTED','CANCELLED')),

    created_at          TIMESTAMPTZ NOT NULL,
    updated_at          TIMESTAMPTZ NOT NULL,
    UNIQUE (supplier_id, supplier_invoice_no)
);

CREATE INDEX idx_supplier_invoice_outlet ON supplier_invoice(outlet_id, invoice_date);

CREATE TABLE supplier_credit (
    id                  UUID PRIMARY KEY,
    tenant_id           UUID NOT NULL REFERENCES tenant(id),
    outlet_id           UUID NOT NULL REFERENCES outlet(id),
    supplier_id         UUID NOT NULL REFERENCES supplier(id),
    purchase_return_id  UUID REFERENCES purchase_return(id),
    credit_note_no      TEXT NOT NULL,
    credit_date         DATE NOT NULL,
    amount_paise        BIGINT NOT NULL CHECK (amount_paise >= 0),
    created_at          TIMESTAMPTZ NOT NULL,
    updated_at          TIMESTAMPTZ NOT NULL,
    UNIQUE (supplier_id, credit_note_no)
);

CREATE INDEX idx_supplier_credit_outlet ON supplier_credit(outlet_id, credit_date);
