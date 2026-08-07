-- Holler Cloud PostgreSQL — reconcile "order" with the CanonicalOrder wire type
-- (contracts 0.2.4). Mirrors sqlite/0004_order_canonical_fields.sql.
-- See the ADR-011 0.2.4 addendum.
--
-- The cloud REPLAYS these rows from the edge (§50.1); it never originates or
-- recomputes them. Part of the `order` operational aggregate, so no new
-- AggregateType entry.
--
-- The rename is NOT additive, and is accepted deliberately — see the SQLite
-- migration's note on why a silent wire/storage name mismatch is worse.

ALTER TABLE "order" RENAME COLUMN tax_paise TO taxes_paise;

ALTER TABLE "order" ADD COLUMN source TEXT NOT NULL DEFAULT 'POS'
    CHECK (source IN ('POS','QR','AGGREGATOR_ZOMATO','AGGREGATOR_SWIGGY','DIRECT'));

ALTER TABLE "order" ADD COLUMN external_order_id TEXT;

ALTER TABLE "order" ADD COLUMN payment_status TEXT NOT NULL DEFAULT 'UNPAID'
    CHECK (payment_status IN ('UNPAID','PARTIALLY_PAID','PAID','REFUNDED'));
ALTER TABLE "order" ADD COLUMN payment_source TEXT;

ALTER TABLE "order" ADD COLUMN confirmed_at TIMESTAMPTZ;

ALTER TABLE "order" ADD COLUMN schema_version INTEGER NOT NULL DEFAULT 1;

-- source_payload JSONB already exists here from 0001_init.sql — the edge is the
-- side that was missing it.

-- The DEFAULTs on the NOT NULL columns are a migration mechanism for existing
-- rows, not an application fallback: ingest sets source, payment_status and
-- schema_version from the replayed envelope payload.
--
-- DELIBERATELY NOT ADDED — synthesized at a fixed value until their milestone,
-- pinned by the order-level round-trip test:
--   packaging_paise, delivery_charge_paise            -> 0,    Milestone 6
--   aggregator_discount_paise, merchant_discount_paise -> 0,    Milestone 6
--   customer {name, phone}                            -> NULL, Milestone 6
--   delivery_address                                  -> NULL, Milestone 6
--   rider {name, phone, status}                       -> NULL, Milestone 6
--   preparation_time_minutes                          -> NULL, Milestone 2
