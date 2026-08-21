-- Holler Cloud PostgreSQL — recipe output. Contracts 0.5.1, ADR-018 addendum.
-- Mirror of sqlite/0019_recipe_output.sql, whose header carries the full
-- reasoning. CONFIG, cloud->edge. NOT NULL on every recipe.
--
-- Summary of the decision recorded in the SQLite mirror:
--   * 0.5.0 gave `recipe` no output, so a SUB_RECIPE ingredient's
--     quantity_micro could only be a dimensionless multiplier. Under that
--     reading, rescaling a sub-recipe silently corrupts every parent: a gravy
--     that moves from 300ml to 3-litre batches makes every dish referencing it
--     wrong by 10x, with no error, until a physical count catches the variance.
--   * Every recipe has an output, not only those referenced as sub-recipes.
--     Nullable-with-enforcement-at-reference-time is the shape this contract
--     keeps rejecting, and it unifies the arithmetic:
--         multiplier = requested_quantity / recipe.output_quantity_micro
--     with no special case for the root, and a 2-serving platter expressible.
--   * The multiplier is NEVER materialised as a rounded number. 100/300 is not
--     clean; carry the rational to the leaf and round once there.
--
-- THE CLOUD IS THE ENFORCEMENT POINT FOR THE DIMENSION RULE. A parent asking
-- for 180 g of a recipe that yields ml is an authoring error and is rejected
-- HERE, at write time, alongside the recursive-CTE cycle check that already
-- guards this table (postgres/0015). There is no conversion to attempt: a
-- recipe is not an inventory item, so no density row exists — item_unit_conversion
-- keys on inventory_item_id. The edge repeats the check defensively and
-- degrades to a deduction gap rather than failing a confirm.
--
-- The DEFAULTs exist only so this applies to rows written during 0.5.0, and are
-- the identity for a single-serving dish. No outlet has authored a recipe and
-- no ledger row exists.
ALTER TABLE recipe ADD COLUMN output_dimension TEXT NOT NULL DEFAULT 'COUNT'
    CHECK (output_dimension IN ('MASS','VOLUME','COUNT'));

ALTER TABLE recipe ADD COLUMN output_quantity_micro BIGINT NOT NULL DEFAULT 1000000
    CHECK (output_quantity_micro > 0);
