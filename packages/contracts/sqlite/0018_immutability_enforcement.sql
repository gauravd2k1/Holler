-- Holler Edge SQLite — immutability enforcement for audit_event, cash_movement
-- and invoice. Contracts 0.5.0, ADR-016, ADR-018.
--
-- WHY THIS EXISTS. All three tables have described themselves as append-only
-- or immutable in a comment since Milestone 1 or 3, with nothing behind the
-- claim. They were found by `every_append_only_claim_has_a_trigger_behind_it`
-- (edge/database/src/migrations.rs) on the run that made that lint pass — the
-- lint was written for `payment`, and immediately found three more of the same
-- shape.
--
-- Together with the UTC business-date bucketing and `payment` itself, that is
-- FOUR structural guarantees that were written as comments and implemented on
-- at most one side. At four it stops being a list of defects and becomes a
-- finding about how M2 and M3 were verified; docs/RESUME.md records it that
-- way, as one correction rather than four entries.

-- ---------------------------------------------------------------------------
-- audit_event — the worst of the three
-- ---------------------------------------------------------------------------
--
-- `0002:63` calls this a "Local append-only audit". An audit log that can be
-- edited is not an audit log. The exposure is not that a row might be changed
-- by accident: it is that this table is precisely the thing you reach for WHEN
-- YOU SUSPECT AN EDIT, and until now it offered no more assurance than the
-- rows it was meant to vouch for.
CREATE TRIGGER audit_event_is_append_only_no_update
BEFORE UPDATE ON audit_event
BEGIN
    SELECT RAISE(ABORT,
        'audit_event is append-only: an audit trail that can be edited is not an audit trail. Append a correcting event (contracts 0.5.0)');
END;

CREATE TRIGGER audit_event_is_append_only_no_delete
BEFORE DELETE ON audit_event
BEGIN
    SELECT RAISE(ABORT,
        'audit_event is append-only: an audit row is never deleted (contracts 0.5.0)');
END;

-- ---------------------------------------------------------------------------
-- cash_movement — same shape as payment, same fix
-- ---------------------------------------------------------------------------
--
-- `0006:392`: "Append-only: a correction is another movement." That sentence
-- describes the intended discipline exactly, and nothing enforced it.
CREATE TRIGGER cash_movement_is_append_only_no_update
BEFORE UPDATE ON cash_movement
BEGIN
    SELECT RAISE(ABORT,
        'cash_movement is append-only: a correction is another movement, never an edit (contracts 0.5.0)');
END;

CREATE TRIGGER cash_movement_is_append_only_no_delete
BEFORE DELETE ON cash_movement
BEGIN
    SELECT RAISE(ABORT,
        'cash_movement is append-only: a movement row is never deleted (contracts 0.5.0)');
END;

-- ---------------------------------------------------------------------------
-- invoice — a trigger, not a wording change
-- ---------------------------------------------------------------------------
--
-- An earlier draft proposed aligning `0006:176`'s wording with the PostgreSQL
-- twin's "append-only REPLAY", on the grounds that `invoice` genuinely has a
-- mutable ISSUED -> CANCELLED status and so cannot be blanket-immutable.
--
-- That was the wrong call. The shape is the one this milestone already built
-- for `stock_count`: **every column immutable, with exactly one legal
-- transition.** Without it, anyone with a psql prompt or a sqlite3 shell can
-- change an invoice total — a worse exposure than the `payment` one, because
-- an invoice is the document handed to the customer and filed with GST.
--
-- Legal: ISSUED -> CANCELLED, setting cancelled_at and cancelled_reason.
-- Everything else, including any re-cancellation or an un-cancellation, aborts.
--
-- MAINTENANCE HAZARD, STATED: this WHEN clause enumerates columns, so a column
-- added to `invoice` later is NOT covered until it is added here. SQLite has no
-- whole-row comparison (the PostgreSQL mirror uses `to_jsonb(OLD) <>
-- to_jsonb(NEW)` and covers new columns automatically). The enumeration is
-- therefore itself a claim that could quietly become false — so
-- `invoice_immutability_trigger_covers_every_column`
-- (edge/database/src/migrations.rs) reads PRAGMA table_info and fails if any
-- column is missing from this list.
CREATE TRIGGER invoice_is_immutable_except_cancellation
BEFORE UPDATE ON invoice
WHEN NOT (
        OLD.status = 'ISSUED' AND NEW.status = 'CANCELLED'
    AND NEW.id                         IS OLD.id
    AND NEW.outlet_id                  IS OLD.outlet_id
    AND NEW.order_id                   IS OLD.order_id
    AND NEW.split_group_id             IS OLD.split_group_id
    AND NEW.split_index                IS OLD.split_index
    AND NEW.split_count                IS OLD.split_count
    AND NEW.series_id                  IS OLD.series_id
    AND NEW.invoice_number             IS OLD.invoice_number
    AND NEW.invoice_date               IS OLD.invoice_date
    AND NEW.business_date              IS OLD.business_date
    AND NEW.customer_name              IS OLD.customer_name
    AND NEW.customer_phone             IS OLD.customer_phone
    AND NEW.customer_gstin             IS OLD.customer_gstin
    AND NEW.place_of_supply_state_code IS OLD.place_of_supply_state_code
    AND NEW.subtotal_paise             IS OLD.subtotal_paise
    AND NEW.discount_paise             IS OLD.discount_paise
    AND NEW.taxable_value_paise        IS OLD.taxable_value_paise
    AND NEW.cgst_paise                 IS OLD.cgst_paise
    AND NEW.sgst_paise                 IS OLD.sgst_paise
    AND NEW.igst_paise                 IS OLD.igst_paise
    AND NEW.cess_paise                 IS OLD.cess_paise
    AND NEW.round_off_paise            IS OLD.round_off_paise
    AND NEW.grand_total_paise          IS OLD.grand_total_paise
    AND NEW.compliance_version_id      IS OLD.compliance_version_id
    AND NEW.tax_snapshot_json          IS OLD.tax_snapshot_json
    AND NEW.fiscal_profile_json        IS OLD.fiscal_profile_json
    AND NEW.channel                    IS OLD.channel
    AND NEW.tax_liability_party        IS OLD.tax_liability_party
    AND NEW.eco_operator_name          IS OLD.eco_operator_name
    AND NEW.eco_operator_gstin         IS OLD.eco_operator_gstin
    AND NEW.supply_classification      IS OLD.supply_classification
    AND NEW.created_by_user_id         IS OLD.created_by_user_id
    AND NEW.created_at                 IS OLD.created_at
)
BEGIN
    SELECT RAISE(ABORT,
        'invoice is immutable: the only legal update is ISSUED -> CANCELLED setting cancelled_at and cancelled_reason. Correct a bill with a credit note, never by editing it (ADR-016, contracts 0.5.0)');
END;

CREATE TRIGGER invoice_is_never_deleted
BEFORE DELETE ON invoice
BEGIN
    SELECT RAISE(ABORT,
        'invoice is never deleted: an issued GST invoice is a filed document. Cancel it, or issue a credit note (ADR-016, contracts 0.5.0)');
END;
