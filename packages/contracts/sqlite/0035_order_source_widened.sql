-- contracts 0.8.1 (ADR-026) — order.source gains AGGREGATOR and TABLE_TAB
--
-- WHY. The CHECK written in 0004 is a closed set — POS, QR, AGGREGATOR_ZOMATO,
-- AGGREGATOR_SWIGGY, DIRECT — and two channels that now exist cannot name
-- themselves in it. The aggregator accept path writes DIRECT and says so in its
-- own comment (`apps/pos/src-tauri/src/commands/aggregator.rs`): DIRECT was the
-- only member that was not a false claim, because a member naming one platform
-- is a stored value that lies about an order from any other, and POS claims a
-- cashier typed it. The
-- table-ordering device (ADR-025) has the same problem and no member at all.
--
-- WHY ONE GENERIC MEMBER, NOT ONE PER PLATFORM. `aggregator_order.platform` is
-- free TEXT on purpose, and 0032 states the reason: a new platform must not
-- require a migration, and the aggregator boundary check keeps platform names
-- out of the core. A per-platform member here would re-import exactly that
-- decision — every new platform a contracts bump — and would put a platform's
-- identity in a shared schema file. The platform is already recorded, twice:
-- `aggregator_order.platform` and `order.source_payload_json`. AGGREGATOR is
-- true for every platform; a platform-named member becomes a new lie the first
-- time a different platform arrives down the same path.
--
-- AGGREGATOR_ZOMATO AND AGGREGATOR_SWIGGY STAY. Removing a member from a CHECK
-- is a breaking change and this bump is additive. They are DEPRECATED in
-- ADR-026, in the Zod enum, in the Go constants and in the OpenAPI schema, with
-- the removal trigger "the next breaking contracts bump". Nothing has ever
-- written either one: the assertion in `edge/database/src/migrations.rs` refuses
-- to run this migration if any row carries them, and the postgres half raises
-- rather than widening over live legacy data.
--
-- NOTHING WRITES THE NEW MEMBERS YET. The accept path keeps writing DIRECT until
-- it is switched in its own change, and TABLE_TAB has no writer at all because
-- ADR-025 is PROPOSED and no code exists. Both are pinned by exact assertion
-- (`scripts/check-order-source-drift.mjs`), the same discipline 0004 applied to
-- the fields it deliberately did not add.
--
-- THE MECHANISM IS A REBUILD, because SQLite cannot ALTER a CHECK. That makes
-- this migration strictly more dangerous than the postgres half, which is one
-- DROP CONSTRAINT and one ADD CONSTRAINT. `migrations.rs` asserts afterwards
-- that the row count is unchanged, that a sampled row is byte-identical across
-- the rebuild, and that the four indexes are back — the 0029/0030 precedent,
-- where a rebuild that silently dropped what it was carrying would have passed
-- every test in the suite.
--
-- (Wording note, as in 0029 and 0030: the claim lint attributes a phrase to the
-- nearest table by LINE DISTANCE, so properties of the real table are described
-- up here rather than beside the transient rebuild table below.)

PRAGMA foreign_keys = OFF;

CREATE TABLE order_rebuild (
    id                  TEXT PRIMARY KEY,
    outlet_id           TEXT NOT NULL REFERENCES outlet(id),
    device_id           TEXT NOT NULL REFERENCES device(id),
    order_type          TEXT NOT NULL CHECK (order_type IN
                          ('DINE_IN','TAKEAWAY','DELIVERY','AGGREGATOR','QR','ROOM_SERVICE','CATERING')),
    status              TEXT NOT NULL DEFAULT 'DRAFT',
    table_id            TEXT,
    subtotal_paise      INTEGER NOT NULL DEFAULT 0,
    discount_paise      INTEGER NOT NULL DEFAULT 0,
    taxes_paise         INTEGER NOT NULL DEFAULT 0,
    total_paise         INTEGER NOT NULL DEFAULT 0,
    version             INTEGER NOT NULL DEFAULT 1,
    sync_status         TEXT NOT NULL DEFAULT 'PENDING' CHECK (sync_status IN ('PENDING','SYNCED','FAILED')),
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    -- The widened set. AGGREGATOR and TABLE_TAB are the additions; the two
    -- platform-named members are carried across deprecated, not dropped.
    source              TEXT NOT NULL DEFAULT 'POS'
                          CHECK (source IN ('POS','QR','AGGREGATOR_ZOMATO','AGGREGATOR_SWIGGY',
                                            'DIRECT','AGGREGATOR','TABLE_TAB')),
    external_order_id   TEXT,
    payment_status      TEXT NOT NULL DEFAULT 'UNPAID'
                          CHECK (payment_status IN ('UNPAID','PARTIALLY_PAID','PAID','REFUNDED')),
    payment_source      TEXT,
    confirmed_at        TEXT,
    source_payload_json TEXT,
    schema_version      INTEGER NOT NULL DEFAULT 1,
    preparation_time_minutes INTEGER,
    display_number      TEXT
);

INSERT INTO order_rebuild (
    id, outlet_id, device_id, order_type, status, table_id,
    subtotal_paise, discount_paise, taxes_paise, total_paise,
    version, sync_status, created_at, updated_at,
    source, external_order_id, payment_status, payment_source,
    confirmed_at, source_payload_json, schema_version,
    preparation_time_minutes, display_number
)
SELECT
    id, outlet_id, device_id, order_type, status, table_id,
    subtotal_paise, discount_paise, taxes_paise, total_paise,
    version, sync_status, created_at, updated_at,
    source, external_order_id, payment_status, payment_source,
    confirmed_at, source_payload_json, schema_version,
    preparation_time_minutes, display_number
FROM "order";

DROP TABLE "order";

ALTER TABLE order_rebuild RENAME TO "order";

-- Indexes, restored verbatim from 0001, 0006 and 0034. DROP TABLE takes them
-- with it in silence.
CREATE INDEX idx_order_outlet_id ON "order"(outlet_id);

CREATE INDEX idx_order_sync_status ON "order"(sync_status);

CREATE INDEX idx_order_display_number ON "order"(outlet_id, display_number);

CREATE INDEX idx_order_external_order_id
    ON "order" (outlet_id, external_order_id)
    WHERE external_order_id IS NOT NULL;

PRAGMA foreign_keys = ON;
