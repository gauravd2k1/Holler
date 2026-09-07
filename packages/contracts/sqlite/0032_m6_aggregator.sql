-- M6 Phase C, contracts 0.8.0 (ADR-022 ACCEPTED 2026-09-08). The edge half.
--
-- Only the two mirrored shapes are here. aggregator_platform_credential,
-- aggregator_item_map and aggregator_callback_receipt are POSTGRES-ONLY and
-- declared in SINGLE_STORE_MIGRATIONS with their reasons -- the edge never
-- talks to a platform, so it has no use for a credential, a resolver or a
-- record of what arrived at the cloud.
--
-- aggregator_order is CLOUD-AUTHORITATIVE and syncs DOWN, replace-not-merge.
-- The edge NEVER authors one of these rows: it receives them, shows them, and
-- on operator accept creates a local `order` -- which is edge-authoritative and
-- syncs up, exactly as every other order does.

CREATE TABLE aggregator_order (
    id                  TEXT PRIMARY KEY,
    tenant_id           TEXT NOT NULL,
    outlet_id           TEXT NOT NULL REFERENCES outlet(id),

    platform            TEXT NOT NULL,

    -- Tenant- and platform-scoped, never global: two platforms can and do
    -- issue the same id.
    external_order_id   TEXT NOT NULL,

    -- The platform's own state string, unmapped. A platform status NEVER
    -- writes order.status -- one writer, as ADR-014 requires for kot.status.
    platform_status     TEXT NOT NULL,

    -- Replace-not-merge compares on this. A newer document replaces the row
    -- WHOLESALE; an older or equal one is ignored outright, the same rule
    -- apply_bundle already applies to the config bundle.
    document_version    INTEGER NOT NULL DEFAULT 1,

    raw_payload         TEXT NOT NULL,

    stated_total_paise  INTEGER,

    received_at         TEXT NOT NULL,
    business_date       TEXT NOT NULL,

    -- NULL means "arrived, not yet accepted": a visible operational state, not
    -- a missing value. Creation of the local order is OPERATOR-CONFIRMED
    -- (ADR-022 addendum §2).
    accepted_at         TEXT,
    local_order_id      TEXT REFERENCES "order"(id),

    schema_version      INTEGER NOT NULL DEFAULT 1,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,

    UNIQUE (tenant_id, platform, external_order_id),
    CHECK (stated_total_paise IS NULL OR stated_total_paise >= 0)
);

CREATE INDEX aggregator_order_outlet_received_idx
    ON aggregator_order (outlet_id, received_at DESC);

-- The unaccepted queue: what a till surface reads to make an unaccepted
-- document loud. ADR-022's addendum names an order sitting unaccepted as a real
-- failure mode created by the operator-confirmed decision, so the index that
-- finds them exists from the start rather than being added after someone is
-- bitten.
CREATE INDEX aggregator_order_unaccepted_idx
    ON aggregator_order (outlet_id, received_at)
    WHERE accepted_at IS NULL;

-- Child row, no sync direction, travels inside its parent's payload.
CREATE TABLE aggregator_order_line (
    id                      TEXT PRIMARY KEY,
    aggregator_order_id     TEXT NOT NULL REFERENCES aggregator_order(id) ON DELETE CASCADE,
    line_number             INTEGER NOT NULL,

    external_item_id        TEXT NOT NULL,
    external_item_name      TEXT NOT NULL,

    -- NULLABLE, AND THE NULL IS LOAD-BEARING. An unmappable line is RECORDED,
    -- NOT REFUSED (ADR-022 rule 4, the grn_gap precedent). No CHECK ties a line
    -- to a menu item and none may be added: refusing a delivery order that is
    -- already cooking is the outage, not the protection.
    menu_item_id            TEXT REFERENCES menu_item(id),

    quantity                INTEGER NOT NULL,
    stated_unit_price_paise INTEGER,

    schema_version          INTEGER NOT NULL DEFAULT 1,

    UNIQUE (aggregator_order_id, line_number),
    CHECK (quantity > 0),
    CHECK (stated_unit_price_paise IS NULL OR stated_unit_price_paise >= 0)
);
