-- M6 Phase C, contracts 0.8.0 (ADR-022). THE CLOUD-ONLY AGGREGATOR SHAPES.
--
-- SEPARATE FILE ON PURPOSE, AND THE SEPARATION IS THE POINT.
-- SINGLE_STORE_MIGRATIONS in edge/database/src/migrations.rs pairs migrations by
-- FILE STEM, so a single-store table hidden inside a mirrored migration cannot
-- be declared -- the guard would see 0032 present in both stores and conclude
-- the pair is complete, while three of its tables exist in only one. That is
-- exactly why grn_sequence ships as sqlite/0028 and the M5 accounts shapes as
-- postgres/0029 rather than riding along inside the mirrored procurement file.
--
-- None of the three below ever reaches an edge, and none is an AggregateType:
--
--   aggregator_platform_credential -- the till has no public address and never
--     talks to a platform, so an outlet has no use for a credential it cannot
--     spend. The refresh_token / device_credential precedent.
--   aggregator_item_map -- resolution happens at the CLOUD when a document
--     arrives, so the edge receives lines already naming local menu items.
--     Mirroring the map means two resolvers that can disagree, and the
--     disagreement surfaces as a wrong dish.
--   aggregator_callback_receipt -- the cloud's record of what arrived at the
--     cloud. The inbound mirror of sync_outbox_block, which is edge-local for
--     the same reason pointed the other way.

-- Platform API credentials. NEVER MIRRORED TO AN EDGE, and that is not a
-- convenience: the till has no public address and never talks to a platform, so
-- an outlet has no use for a credential it cannot spend. Same reasoning as
-- refresh_token and device_credential -- credential material does not travel to
-- a machine that does not need it.
CREATE TABLE aggregator_platform_credential (
    id              UUID PRIMARY KEY,
    tenant_id       UUID NOT NULL REFERENCES tenant(id),
    outlet_id       UUID REFERENCES outlet(id),
    platform        TEXT NOT NULL,

    -- Hashed or encrypted at the application layer before it reaches this
    -- column. The name says secret so nothing logs it by accident, and it joins
    -- password_hash / pin_hash / token_hash / device_token_hash on the audit
    -- redact list.
    credential_secret TEXT NOT NULL,
    is_active       BOOLEAN NOT NULL DEFAULT TRUE,

    schema_version  INTEGER NOT NULL DEFAULT 1,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (tenant_id, platform, outlet_id)
);

-- Platform item id -> menu_item. POSTGRES-ONLY: resolution happens at the cloud
-- when a document arrives, so the edge receives lines that already name local
-- menu items. Mirroring the map would give two resolvers that can disagree, and
-- the disagreement would surface as a wrong dish.
CREATE TABLE aggregator_item_map (
    id                UUID PRIMARY KEY,
    tenant_id         UUID NOT NULL REFERENCES tenant(id),
    outlet_id         UUID NOT NULL REFERENCES outlet(id),
    platform          TEXT NOT NULL,
    external_item_id  TEXT NOT NULL,
    menu_item_id      UUID NOT NULL REFERENCES menu_item(id),

    schema_version    INTEGER NOT NULL DEFAULT 1,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (tenant_id, outlet_id, platform, external_item_id)
);

-- Inbound callback dedupe. Platforms retry, and a retried callback must be
-- idempotent rather than producing a second document. The UNIQUE key IS the
-- mechanism: an insert that conflicts is a duplicate, and that is the whole
-- check.
--
-- POSTGRES-ONLY and never an AggregateType: this is the cloud's record of what
-- arrived at the cloud. It is the inbound mirror of sync_outbox_block, which is
-- edge-local for exactly the same reason pointed the other way.
CREATE TABLE aggregator_callback_receipt (
    id             UUID PRIMARY KEY,
    tenant_id      UUID NOT NULL REFERENCES tenant(id),
    platform       TEXT NOT NULL,

    -- The platform's own message identifier. Beckn calls it message_id; a
    -- sync-REST platform calls it something else. The column is named for what
    -- it does here, not for what any one platform calls it -- platform
    -- vocabulary belongs inside its adapter (C-4).
    message_id     TEXT NOT NULL,

    action         TEXT NOT NULL,
    received_at    TIMESTAMPTZ NOT NULL DEFAULT now(),

    schema_version INTEGER NOT NULL DEFAULT 1,

    UNIQUE (tenant_id, platform, message_id)
);
