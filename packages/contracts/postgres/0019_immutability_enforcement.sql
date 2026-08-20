-- Holler Cloud PostgreSQL — immutability enforcement for audit_event,
-- cash_movement and invoice. Contracts 0.5.0, ADR-016, ADR-018.
-- Mirror of sqlite/0018_immutability_enforcement.sql, whose header carries the
-- full reasoning.
--
-- All three described themselves as append-only or immutable in a comment,
-- with nothing behind the claim, and were found by
-- `every_append_only_claim_has_a_trigger_behind_it` on the run that made that
-- lint pass. With the UTC business-date bucketing and `payment` (0018), that is
-- four structural guarantees written as comments and implemented on at most one
-- side — recorded in docs/RESUME.md as one finding about how M2 and M3 were
-- verified, not as four separate defects.

CREATE OR REPLACE FUNCTION append_only_guard()
RETURNS TRIGGER AS $$
BEGIN
    RAISE EXCEPTION '% is append-only: correct it by appending a new row, never by UPDATE or DELETE (contracts 0.5.0)', TG_TABLE_NAME;
END;
$$ LANGUAGE plpgsql;

-- An audit trail that can be edited is not an audit trail, and this is the
-- table you reach for precisely when you suspect an edit.
CREATE TRIGGER audit_event_is_append_only
BEFORE UPDATE OR DELETE ON audit_event
FOR EACH ROW EXECUTE FUNCTION append_only_guard();

-- "A correction is another movement" -- the comment described the discipline
-- exactly and nothing enforced it.
CREATE TRIGGER cash_movement_is_append_only
BEFORE UPDATE OR DELETE ON cash_movement
FOR EACH ROW EXECUTE FUNCTION append_only_guard();

-- invoice: every column immutable, exactly one legal transition. The same
-- shape as stock_count (0016), and a worse exposure than payment's -- an
-- invoice is the document handed to the customer and filed with GST, and until
-- now anyone with a psql prompt could change its total.
--
-- Whole-row comparison rather than an enumerated column list, so a column
-- added to `invoice` later is covered automatically. The SQLite mirror cannot
-- do this (no whole-row comparison), enumerates instead, and is therefore
-- guarded by a test that checks the enumeration against PRAGMA table_info.
CREATE OR REPLACE FUNCTION invoice_immutable_except_cancellation()
RETURNS TRIGGER AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'invoice is never deleted: an issued GST invoice is a filed document. Cancel it, or issue a credit note (ADR-016, contracts 0.5.0)';
    END IF;

    IF NOT (OLD.status = 'ISSUED' AND NEW.status = 'CANCELLED') THEN
        RAISE EXCEPTION 'invoice is immutable: the only legal update is ISSUED -> CANCELLED (ADR-016, contracts 0.5.0)';
    END IF;

    IF (to_jsonb(OLD) - 'status' - 'cancelled_at' - 'cancelled_reason')
    <> (to_jsonb(NEW) - 'status' - 'cancelled_at' - 'cancelled_reason') THEN
        RAISE EXCEPTION 'invoice is immutable: a cancellation may set only cancelled_at and cancelled_reason. Correct a bill with a credit note, never by editing it (ADR-016, contracts 0.5.0)';
    END IF;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER invoice_is_immutable_except_cancellation
BEFORE UPDATE OR DELETE ON invoice
FOR EACH ROW EXECUTE FUNCTION invoice_immutable_except_cancellation();
