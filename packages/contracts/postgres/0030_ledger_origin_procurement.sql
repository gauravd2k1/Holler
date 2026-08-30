-- Holler Cloud PostgreSQL — stock_ledger_entry.origin learns procurement.
-- Contracts 0.6.2, ADR-019 addendum. Mirror of
-- sqlite/0029_ledger_origin_procurement.sql; read that file for the reasoning.
--
-- PostgreSQL can drop and re-add a CHECK, so this is a constraint swap where
-- the edge needed a full table rebuild. The asymmetry is the one 0021 already
-- noted in the other direction, and it is why the two files look nothing alike
-- while expressing the same constraint.
--
-- BOTH CHECKS MOVE. The origin enum is the obvious one; the provenance CHECK
-- pins each origin to the shape of its companion columns, and its
-- 'MANUAL','COUNT_ADJUSTMENT','WASTAGE' branch would otherwise reject every new
-- member at insert time with a message about recipe_id. Extending only the enum
-- gives a migration that applies cleanly and then fails on the first receipt.
--
-- THE CONSTRAINTS ARE FOUND, NOT NAMED BY GUESS. Both CHECKs in 0016 are
-- written inline and unnamed, so their identifiers are whatever PostgreSQL
-- auto-generated — stock_ledger_entry_origin_check for the column one, and an
-- ordinal stock_ledger_entry_check / _check1 / ... for the table one, depending
-- on how many unnamed table constraints preceded it. Hard-coding either is a
-- guess that fails on a database built by a slightly different path, and
-- `DROP CONSTRAINT IF EXISTS` would swallow the miss in silence and leave the
-- old CHECK in force. So both are located by their definition text and dropped
-- by their real name, then re-added WITH explicit names so no future migration
-- has to do this again.

DO $$
DECLARE
    victim RECORD;
    dropped INTEGER := 0;
BEGIN
    FOR victim IN
        SELECT conname
        FROM pg_constraint
        WHERE conrelid = 'stock_ledger_entry'::regclass
          AND contype = 'c'
          AND pg_get_constraintdef(oid) LIKE '%MODIFIER_DELTA%'
    LOOP
        EXECUTE format('ALTER TABLE stock_ledger_entry DROP CONSTRAINT %I', victim.conname);
        dropped := dropped + 1;
    END LOOP;

    -- Exactly two CHECKs mention MODIFIER_DELTA: the origin enum and the
    -- provenance rule. Anything else means the schema moved under this
    -- migration, and continuing would re-add constraints beside ones this
    -- file has not seen.
    IF dropped <> 2 THEN
        RAISE EXCEPTION
            'expected 2 origin-related CHECK constraints on stock_ledger_entry, dropped %. The schema has changed; fix this migration rather than the count.', dropped;
    END IF;
END $$;

ALTER TABLE stock_ledger_entry
    ADD CONSTRAINT stock_ledger_entry_origin_check
    CHECK (origin IN (
        'RECIPE','MODIFIER_DELTA','MANUAL',
        'COUNT_ADJUSTMENT','WASTAGE',
        -- One per provenance column already on the row (source_grn_id,
        -- source_purchase_return_id, source_stock_transfer_out_id), so origin
        -- and provenance cannot disagree about which document produced the
        -- movement.
        'GOODS_RECEIPT','PURCHASE_RETURN','STOCK_TRANSFER'));

ALTER TABLE stock_ledger_entry
    ADD CONSTRAINT stock_ledger_entry_provenance_check
    CHECK (
        (origin = 'RECIPE'
            AND recipe_id IS NOT NULL
            AND modifier_delta_id IS NULL)
     OR (origin = 'MODIFIER_DELTA'
            AND modifier_delta_id IS NOT NULL
            AND recipe_id IS NULL)
        -- The procurement origins join the "no recipe, no modifier" branch: a
        -- receipt is not attributable to a recipe or a modifier delta, and
        -- claiming otherwise would be the half-attributed movement this CHECK
        -- exists to prevent.
     OR (origin IN ('MANUAL','COUNT_ADJUSTMENT','WASTAGE',
                    'GOODS_RECEIPT','PURCHASE_RETURN','STOCK_TRANSFER')
            AND recipe_id IS NULL
            AND modifier_delta_id IS NULL)
    );
