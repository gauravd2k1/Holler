-- Holler Cloud PostgreSQL — an upper bound on order_item.quantity. Contracts
-- 0.5.4, ADR-018 addendum. Mirror of sqlite/0022_order_item_quantity_bound.sql,
-- whose header carries the full reasoning.
--
-- Summary: 0.5.3 bounded stored inventory quantities to 1e15 micro-units so
-- overflow would be unreachable, and closed only one side of the
-- multiplication. T2 found the other: quantity_micro x order_item.quantity
-- still overflows int64 at the boundary with a line quantity of 9224, and
-- order_item.quantity had no ceiling at all. T2 routed the overflow to an
-- UNRESOLVABLE_REFERENCE gap, which is an imprecise reason for a magnitude
-- problem -- and a wrong reason code in an append-only table is as unfixable as
-- a wrong quantity.
--
-- The sharpened stopping rule decides it: the test is not only whether the
-- change touches existing rows, but whether the INTERIM writes rows that would
-- need rewriting. It does, so the bound lands now rather than at 0.6.0.
ALTER TABLE order_item
    ADD CONSTRAINT order_item_quantity_is_bounded
    CHECK (quantity <= 10000);
