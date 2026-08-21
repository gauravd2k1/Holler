-- Holler Edge SQLite — an upper bound on order_item.quantity. Contracts 0.5.4,
-- ADR-018 addendum.
--
-- WHY. 0.5.3 bounded every stored inventory quantity to 1e15 micro-units so
-- that arithmetic overflow would be unreachable rather than handled. It closed
-- one side of the multiplication and not the other.
--
-- T2 found the hole by testing for it: `quantity_micro x order_item.quantity`
-- still overflows i64 at the boundary value with a line quantity of 9224
-- (i64::MAX / 1e15 = 9223.37). `order_item.quantity` carried only
-- `CHECK (quantity > 0)` and no ceiling at all.
--
-- T2 routed the overflow to an UNRESOLVABLE_REFERENCE gap rather than
-- reintroducing silence or panicking, which was the right call with the
-- vocabulary available — but it is **an imprecise reason for a magnitude
-- problem, not a reference problem**, and 0.5.3's own addendum says a wrong
-- reason code in an append-only table is as unfixable as a wrong quantity.
--
-- **This is the sharpened stopping rule deciding its first real case.** The
-- test is not only "does the change touch existing rows" — adding a bound does
-- not — but "does the INTERIM write rows that would need rewriting". Leaving it
-- until 0.6.0 means every overflow in between is recorded permanently under the
-- wrong reason. So it lands now, before T3 adds three more ledger writers.
--
-- 10,000 is the ceiling: a single order line for ten thousand portions is not a
-- restaurant order, and it leaves the product at most 1e15 x 1e4 = 1e19 —
-- still above i64, so the bound alone is not the proof. What makes overflow
-- unreachable is the pair: no real ingredient quantity approaches 1e15 (that is
-- a thousand tonnes), and no real line approaches 1e4. The edge computes in
-- i128 regardless (edge/database/src/inventory/rational.rs), so the arithmetic
-- has headroom the storage bounds do not need to provide.
--
-- SQLite cannot ADD CONSTRAINT to an existing table, so this is a trigger —
-- the same idiom 0021 used, for the same reason. PostgreSQL gets a real CHECK.
CREATE TRIGGER order_item_quantity_is_bounded
BEFORE INSERT ON order_item
WHEN NEW.quantity > 10000
BEGIN
    SELECT RAISE(ABORT,
        'order_item.quantity exceeds 10000. A single line for ten thousand portions is bad data, not a runtime condition -- the bound exists so that quantity x recipe quantity cannot overflow, rather than being caught and mislabelled (ADR-018, contracts 0.5.4)');
END;

CREATE TRIGGER order_item_quantity_is_bounded_on_update
BEFORE UPDATE OF quantity ON order_item
WHEN NEW.quantity > 10000
BEGIN
    SELECT RAISE(ABORT,
        'order_item.quantity exceeds 10000 (ADR-018, contracts 0.5.4)');
END;
