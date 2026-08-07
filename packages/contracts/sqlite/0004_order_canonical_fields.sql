-- Holler Edge SQLite — reconcile "order" with the CanonicalOrder wire type
-- (contracts 0.2.4). See the ADR-011 0.2.4 addendum.
--
-- WHY: CanonicalOrder carries 25 fields; this table had 14 columns. Fields with
-- no column were synthesized at serialization time, so confirmed_at was
-- silently lost on every replay and the canonical model was partly fictional.
-- Found by the order-level persistence round-trip test added at 0.2.3 — the
-- same class of gap as the missing order_item modifiers, one level up.
--
-- The rename is NOT additive. It is accepted deliberately: a silent name
-- mismatch between the wire (`taxes_paise`) and storage (`tax_paise`) feeding
-- Milestone 3's tax engine is a worse failure than a coordinated rename now.

ALTER TABLE "order" RENAME COLUMN tax_paise TO taxes_paise;

-- Channel of origin. Milestone 1 writes only 'POS'; the enum is the frozen
-- OrderSource list so later channels need no further migration.
ALTER TABLE "order" ADD COLUMN source TEXT NOT NULL DEFAULT 'POS'
    CHECK (source IN ('POS','QR','AGGREGATOR_ZOMATO','AGGREGATOR_SWIGGY','DIRECT'));

-- The aggregator's own order id. NULL for POS/QR/Direct origin.
ALTER TABLE "order" ADD COLUMN external_order_id TEXT;

ALTER TABLE "order" ADD COLUMN payment_status TEXT NOT NULL DEFAULT 'UNPAID'
    CHECK (payment_status IN ('UNPAID','PARTIALLY_PAID','PAID','REFUNDED'));
ALTER TABLE "order" ADD COLUMN payment_source TEXT;

-- DRAFT -> CONFIRMED transition time. Part of the order state machine already
-- implemented in Milestone 1; it had nowhere to live and was being dropped.
ALTER TABLE "order" ADD COLUMN confirmed_at TEXT;

-- Raw external payload, audit only — never parsed as core relational data.
-- Postgres already had this column; the edge did not.
ALTER TABLE "order" ADD COLUMN source_payload_json TEXT;

ALTER TABLE "order" ADD COLUMN schema_version INTEGER NOT NULL DEFAULT 1;

-- The DEFAULTs on the three NOT NULL columns exist only because SQLite requires
-- a non-null default when adding a NOT NULL column to an existing table.
-- Writers set source, payment_status and schema_version explicitly; the
-- defaults are a migration mechanism, not an application-level fallback.
--
-- DELIBERATELY NOT ADDED — each wire field below is synthesized at a fixed
-- value until its owning milestone lands, and the order-level round-trip test
-- pins those exact values so a change cannot pass unnoticed:
--   packaging_paise, delivery_charge_paise            -> 0,    Milestone 6
--   aggregator_discount_paise, merchant_discount_paise -> 0,    Milestone 6
--   customer {name, phone}                            -> NULL, Milestone 6
--   delivery_address                                  -> NULL, Milestone 6
--   rider {name, phone, status}                       -> NULL, Milestone 6
--   preparation_time_minutes                          -> NULL, Milestone 2
