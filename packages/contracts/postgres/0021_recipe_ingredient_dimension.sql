-- Holler Cloud PostgreSQL — recipe_ingredient.quantity_dimension. Contracts
-- 0.5.2, ADR-018 addendum. Mirror of
-- sqlite/0020_recipe_ingredient_dimension.sql, whose header carries the full
-- reasoning.
--
-- Summary of the decision recorded in the SQLite mirror:
--   * quantity_micro was dimensionless in storage: 220_000_000 meant grams only
--     because the chicken it pointed at declared MASS. Reclassify chicken to
--     COUNT — whole birds, a reasonable thing for a restaurant to do — and
--     every recipe silently reinterprets it as 220 birds, with no error and
--     months of wrong rows in an append-only table before a count catches it.
--   * A stored record of intent survives a change in the referent; a derived
--     value does not. Same argument that settled 0.5.1, one level down.
--   * THE COLUMN IS THE AUTHOR'S UNIT AND IS NEVER AUTO-FILLED FROM THE
--     REFERENT. If a write path or UI populates it by looking up the item's
--     dimension, the comparison becomes x == x and the guard can never fire —
--     and it will look correct in review. The lazy implementation is the
--     tautological one.
--   * The ITEM case is where it earns its keep, not the sub-recipe case that
--     prompted it: sub-recipes are a minority, and every recipe has ITEM
--     ingredients.
--
-- THIS STORE IS THE ENFORCEMENT POINT, because this is where recipes are
-- authored and bulk-imported. The edge deliberately has no equivalent trigger:
-- it never authors config, and aborting a config sync over a defect the outlet
-- cannot fix is worse than a deduction that reports itself. There, a mismatch
-- becomes a DIMENSION_MISMATCH deduction gap and the sale completes.
--
-- Where this genuinely fires: bulk recipe import from a spreadsheet, which is
-- how a restaurant with 200 recipes gets onboarded, has no UI to derive
-- anything from, and has no human checking each row.
--
-- The DEFAULT exists only so the ALTER can add a NOT NULL column. It is not a
-- fallback and no writer may rely on it; the OpenAPI schema lists
-- quantity_dimension as required.
ALTER TABLE recipe_ingredient ADD COLUMN quantity_dimension TEXT NOT NULL
    DEFAULT 'COUNT'
    CHECK (quantity_dimension IN ('MASS','VOLUME','COUNT'));

-- The author's unit must match what the row actually points at. An ITEM row is
-- checked against the inventory item's dimension; a SUB_RECIPE row against the
-- referenced recipe's output_dimension.
--
-- There is nothing to convert through in either case: item_unit_conversion
-- keys on inventory_item_id, and a recipe is not an inventory item. So a
-- mismatch is an authoring error and is rejected, never coerced.
CREATE OR REPLACE FUNCTION recipe_ingredient_dimension_matches_referent()
RETURNS TRIGGER AS $$
DECLARE
    referent_dimension TEXT;
BEGIN
    IF NEW.component_kind = 'ITEM' THEN
        SELECT dimension INTO referent_dimension
          FROM inventory_item WHERE id = NEW.inventory_item_id;
        IF referent_dimension IS NULL THEN
            RAISE EXCEPTION 'recipe_ingredient references inventory_item % which does not exist', NEW.inventory_item_id;
        END IF;
        IF referent_dimension <> NEW.quantity_dimension THEN
            RAISE EXCEPTION 'recipe_ingredient.quantity_dimension is % but inventory_item % is measured in %. The quantity is recorded in the unit the AUTHOR chose; a mismatch is an authoring error and is never converted (ADR-018, contracts 0.5.2)',
                NEW.quantity_dimension, NEW.inventory_item_id, referent_dimension;
        END IF;
    ELSE
        SELECT output_dimension INTO referent_dimension
          FROM recipe WHERE id = NEW.sub_recipe_id;
        IF referent_dimension IS NULL THEN
            RAISE EXCEPTION 'recipe_ingredient references recipe % which does not exist', NEW.sub_recipe_id;
        END IF;
        IF referent_dimension <> NEW.quantity_dimension THEN
            RAISE EXCEPTION 'recipe_ingredient.quantity_dimension is % but recipe % yields %. A recipe is not an inventory item, so there is no density row to convert through (ADR-018, contracts 0.5.2)',
                NEW.quantity_dimension, NEW.sub_recipe_id, referent_dimension;
        END IF;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER recipe_ingredient_dimension_matches_referent
BEFORE INSERT OR UPDATE ON recipe_ingredient
FOR EACH ROW EXECUTE FUNCTION recipe_ingredient_dimension_matches_referent();

-- Reclassifying an ingredient changes the MEANING of every recipe referencing
-- it. That is a migration, not a config-screen edit, and it is forbidden while
-- any reference exists. Detach or migrate the recipes explicitly and it becomes
-- possible again — deliberately, with the recipes in front of you.
CREATE OR REPLACE FUNCTION inventory_item_dimension_frozen_while_referenced()
RETURNS TRIGGER AS $$
BEGIN
    IF OLD.dimension <> NEW.dimension
       AND EXISTS (SELECT 1 FROM recipe_ingredient WHERE inventory_item_id = OLD.id) THEN
        RAISE EXCEPTION 'inventory_item.dimension cannot change while a recipe references this item: it would silently change what every one of those recipes deducts. Migrate the recipes explicitly (ADR-018, contracts 0.5.2)';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER inventory_item_dimension_is_frozen_while_referenced
BEFORE UPDATE ON inventory_item
FOR EACH ROW EXECUTE FUNCTION inventory_item_dimension_frozen_while_referenced();
