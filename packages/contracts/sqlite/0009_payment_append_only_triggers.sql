-- Holler Edge SQLite — payment append-only, enforced by the storage engine.
-- Contracts 0.4.5, ADR-016 addendum.
--
-- WHY. Payments are append-only by design (ADR-016): a void or refund is a NEW
-- row carrying reverses_payment_id and a non-positive amount, never an update
-- of the row it reverses. Money that can be edited in place cannot be audited,
-- and §53 requires financial records never be silently lost or overwritten.
--
-- Until now that rule was enforced only by discipline. The T7c verification
-- gate established that repo::insert_payment is the sole writer and that no
-- `UPDATE payment` or `DELETE FROM payment` exists anywhere in the workspace --
-- true, and still true. But it also found that Db::connection() is plain `pub`
-- and returns &rusqlite::Connection, whose execute() takes &self rather than
-- &mut self. Every sibling crate (edge/device, edge/printer, edge/sync) holds
-- it, in production code and not merely in tests. Its doc comment claimed the
-- handle was not exposed beyond the crate's own modules, which was false.
--
-- So the guarantee rested on nobody writing a line of raw SQL that the type
-- system permits and the codebase already demonstrates elsewhere
-- (edge/database/src/lib.rs does exactly this shape against "order" in test
-- code). These triggers move the rule from convention into the engine: it now
-- holds regardless of who holds a connection, with no change to any sibling
-- crate's API.
--
-- SCOPE: `payment` ONLY.
--
-- cash_shift is deliberately NOT covered. Its OPEN -> CLOSED transition is a
-- legitimate in-place UPDATE (close_cash_shift_in_tx), guarded by
-- `WHERE ... AND status = 'OPEN'` so a double-close cannot rewrite a closed
-- shift's counted amount. A blanket no-update trigger there would break a
-- correct path rather than protect one. Closing that surface properly needs
-- the connection()-visibility work, which is not this migration.
--
-- payment_allocation is likewise not covered here. It is insert-only in
-- practice (repo.rs writes it only alongside its payment, inside the same
-- transaction) but it was wired only at dc1c5be and has not had the
-- workspace-wide audit `payment` has had. Covering it is a candidate for a
-- later version once that audit exists -- deliberately not assumed.

CREATE TRIGGER payment_is_append_only_no_update
BEFORE UPDATE ON payment
BEGIN
    SELECT RAISE(ABORT,
        'payment is append-only: reverse it with a new row carrying reverses_payment_id and a non-positive amount_paise, never UPDATE (ADR-016, contracts 0.4.5)');
END;

CREATE TRIGGER payment_is_append_only_no_delete
BEFORE DELETE ON payment
BEGIN
    SELECT RAISE(ABORT,
        'payment is append-only: a payment row is never deleted, reverse it with a new row carrying reverses_payment_id (ADR-016, contracts 0.4.5)');
END;
