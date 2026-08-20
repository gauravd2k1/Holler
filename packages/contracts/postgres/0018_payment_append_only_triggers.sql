-- Holler Cloud PostgreSQL — payment append-only, enforced by the storage
-- engine. Contracts 0.5.0, ADR-016 (0.4.5 addendum), ADR-018.
--
-- Mirror of sqlite/0009_payment_append_only_triggers.sql, arriving two
-- contract versions late.
--
-- WHY THIS EXISTS AS ITS OWN MIGRATION, AND WHY IT IS A DEFECT FIX.
--
-- postgres/0007_m3_billing.sql:286 says, in a comment:
--
--     -- APPEND-ONLY (docs/spec/payments.md §Conflict policy). Nothing updates a
--
-- and there was nothing behind it. The SQLite side got real BEFORE UPDATE and
-- BEFORE DELETE triggers at 0.4.5; PostgreSQL got the sentence. So the
-- guarantee ADR-016 leans on — that a tender is corrected by an appended
-- reversal and never by a mutation — was STRUCTURAL AT THE EDGE AND
-- DOCUMENTATION IN THE CLOUD.
--
-- That is the wrong way round. The edge is a single process nobody has a
-- console on. The cloud is where a support engineer sits with a psql prompt at
-- 2am, and it is the one place where "just fix the row" is a keystroke away.
-- The protection was absent from precisely the environment that needed it.
--
-- This is recorded as an M3 DEFECT in docs/RESUME.md's acceptance record —
-- the second, alongside the UTC business-date bucketing. Fixing it here does
-- not retire the fact that M3 was reported complete while carrying both.
--
-- The lint that now prevents a recurrence:
-- `every_append_only_claim_has_a_trigger_behind_it`
-- (edge/database/src/migrations.rs) fails the build when a table is described
-- as APPEND-ONLY or IMMUTABLE in either store without enforcement behind it.
-- Writing it turned up two more claims of the same shape — `audit_event` and
-- `cash_movement` — so `payment` was not alone, and this class needed a guard
-- rather than one more fix.

CREATE OR REPLACE FUNCTION payment_append_only()
RETURNS TRIGGER AS $$
BEGIN
    IF TG_OP = 'UPDATE' THEN
        RAISE EXCEPTION 'payment is append-only: reverse it with a new row carrying reverses_payment_id and a non-positive amount_paise, never UPDATE (ADR-016, contracts 0.4.5)';
    ELSE
        RAISE EXCEPTION 'payment is append-only: a payment row is never deleted, reverse it with a new row carrying reverses_payment_id (ADR-016, contracts 0.4.5)';
    END IF;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER payment_is_append_only
BEFORE UPDATE OR DELETE ON payment
FOR EACH ROW EXECUTE FUNCTION payment_append_only();
