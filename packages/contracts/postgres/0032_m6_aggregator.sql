-- M6 Phase C, contracts 0.8.0 (ADR-022 ACCEPTED 2026-09-08).
--
-- Two aggregates, not one. `aggregator_order` is the inbound DOCUMENT from an
-- external platform and is CLOUD-AUTHORITATIVE, syncing DOWN. The local `order`
-- it produces stays EDGE-AUTHORITATIVE and syncs UP, exactly as every other
-- order does, linked by external_order_id.
--
-- The published guarantee that falls out of that split: A NEW AGGREGATOR ORDER
-- CANNOT ARRIVE WHILE THE UPLINK IS DOWN; ONE THAT HAS ALREADY ARRIVED IS FULLY
-- OPERABLE OFFLINE. The till has no public address, so it cannot receive one
-- directly -- but once a document is on the machine, billing, printing and
-- closing it are local transactions carrying every offline guarantee this
-- product already makes.
--
-- Making `order` cloud-authoritative for this one channel would be split
-- authority on a single aggregate, which §50.1 forbids, and would mean a
-- delivery-heavy outlet could not bill an aggregator order with the line down
-- -- the opposite of the product.

-- ---------------------------------------------------------------------------
-- The inbound document
-- ---------------------------------------------------------------------------
CREATE TABLE aggregator_order (
    id                  UUID PRIMARY KEY,
    tenant_id           UUID NOT NULL REFERENCES tenant(id),
    outlet_id           UUID NOT NULL REFERENCES outlet(id),

    -- Which platform sent it. TEXT, not an enum type: a new platform must not
    -- require a migration, and the drift check (C-4) keeps platform names out
    -- of the core anyway -- this column holds data, not code.
    platform            TEXT NOT NULL,

    -- The platform's own order identifier. TENANT- AND PLATFORM-SCOPED, NEVER
    -- GLOBAL: two platforms can and do issue the same id, so a global unique
    -- would reject the second one as a duplicate of an unrelated order.
    external_order_id   TEXT NOT NULL,

    -- The platform's view of the order's state, verbatim and unmapped. NOT
    -- order.status: a platform status NEVER writes the local order's status
    -- (ADR-014's one-writer rule, applied again). Kept as the platform's own
    -- string so a status we have never seen is recorded rather than coerced
    -- into the nearest local one.
    platform_status     TEXT NOT NULL,

    -- Replace-not-merge needs a version to compare. A document is replaced
    -- WHOLESALE at a newer version, never field-merged with local state -- the
    -- GET /sync/config precedent. Merging would make the edge a second writer
    -- of a cloud-authoritative row.
    document_version    BIGINT NOT NULL DEFAULT 1,

    -- The raw inbound payload, kept whole. It is the record of what an external
    -- system actually asked for, and the thing anyone reaches for in a dispute.
    -- Storing only our parse of it means a mapping bug is unfalsifiable after
    -- the fact.
    raw_payload         JSONB NOT NULL,

    -- Money the platform states. Integer paise, like every other money column
    -- in this schema. NULLABLE because not every inbound shape carries a total
    -- at every stage of its lifecycle, and inventing a zero would be worse.
    stated_total_paise  BIGINT CHECK (stated_total_paise IS NULL OR stated_total_paise >= 0),

    received_at         TIMESTAMPTZ NOT NULL,
    business_date       DATE NOT NULL,

    -- Set when a human at the till accepts the document and the edge creates a
    -- local order from it. NULL means "arrived, not yet accepted" -- a real and
    -- visible operational state, not a missing value. Creation is
    -- OPERATOR-CONFIRMED, never automatic (ADR-022 addendum §2): an
    -- unmappable document must not put unresolved lines into a kitchen.
    accepted_at         TIMESTAMPTZ,
    local_order_id      UUID REFERENCES "order"(id),

    schema_version      INTEGER NOT NULL DEFAULT 1,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (tenant_id, platform, external_order_id)
);

CREATE INDEX aggregator_order_outlet_received_idx
    ON aggregator_order (outlet_id, received_at DESC);

-- Unaccepted documents are the operational queue a till surface reads. Partial
-- index because the interesting set is always the small one.
CREATE INDEX aggregator_order_unaccepted_idx
    ON aggregator_order (outlet_id, received_at)
    WHERE accepted_at IS NULL;

-- ---------------------------------------------------------------------------
-- Its lines. A CHILD ROW, not an aggregate: it travels inside its parent's
-- payload and has no sync direction of its own -- the invoice_line / grn_line
-- precedent.
-- ---------------------------------------------------------------------------
CREATE TABLE aggregator_order_line (
    id                     UUID PRIMARY KEY,
    aggregator_order_id    UUID NOT NULL REFERENCES aggregator_order(id) ON DELETE CASCADE,
    line_number            INTEGER NOT NULL,

    -- What the platform called it, kept whatever happens. If the mapping fails
    -- this is the only description of what the customer ordered.
    external_item_id       TEXT NOT NULL,
    external_item_name     TEXT NOT NULL,

    -- NULLABLE, AND THE NULL IS LOAD-BEARING. An inbound document that cannot
    -- be mapped to a menu item is RECORDED, NOT REFUSED (ADR-022 rule 4) --
    -- the grn_gap precedent. Refusing a delivery order that is already cooking
    -- is the outage, not the protection. No CHECK ties a line to a menu item
    -- and none may be added.
    menu_item_id           UUID REFERENCES menu_item(id),

    quantity               INTEGER NOT NULL CHECK (quantity > 0),
    stated_unit_price_paise BIGINT CHECK (stated_unit_price_paise IS NULL OR stated_unit_price_paise >= 0),

    schema_version         INTEGER NOT NULL DEFAULT 1,

    UNIQUE (aggregator_order_id, line_number)
);
