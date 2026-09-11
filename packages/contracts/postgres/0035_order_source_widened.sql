-- contracts 0.8.1 (ADR-026) — order.source gains AGGREGATOR and TABLE_TAB
--
-- The cloud half of the widening. The reasoning is in the SQLite file and in
-- ADR-026 and is not repeated here; what differs is the mechanism and one
-- guard.
--
-- MECHANISM. PostgreSQL can drop and re-add a CHECK, so this is two statements
-- against a table it never rewrites. The SQLite side cannot, and rebuilds the
-- whole table — which is why that file carries assertions and this one does
-- not need them.
--
-- THE GUARD. Nothing has ever written AGGREGATOR_ZOMATO or AGGREGATOR_SWIGGY:
-- every order in this store is POS or DIRECT. Those two members are carried
-- across DEPRECATED rather than removed, because removing a member from a CHECK
-- is a breaking change and this bump is additive. If a row carrying one is ever
-- found, the widening STOPS rather than proceeding — a deprecation note that
-- silently widens over live data it claims nothing writes is a note that is
-- already wrong.

DO $$
DECLARE
    legacy_rows bigint;
BEGIN
    SELECT count(*) INTO legacy_rows
    FROM "order"
    WHERE source IN ('AGGREGATOR_ZOMATO', 'AGGREGATOR_SWIGGY');

    IF legacy_rows > 0 THEN
        RAISE EXCEPTION
            'contracts 0.8.1: % order row(s) carry AGGREGATOR_ZOMATO or AGGREGATOR_SWIGGY. ADR-026 deprecates both on the stated basis that nothing has ever written them. Stop and report rather than widening over them.',
            legacy_rows;
    END IF;
END
$$;

ALTER TABLE "order" DROP CONSTRAINT order_source_check;

ALTER TABLE "order" ADD CONSTRAINT order_source_check
    CHECK (source IN ('POS','QR','AGGREGATOR_ZOMATO','AGGREGATOR_SWIGGY',
                      'DIRECT','AGGREGATOR','TABLE_TAB'));
