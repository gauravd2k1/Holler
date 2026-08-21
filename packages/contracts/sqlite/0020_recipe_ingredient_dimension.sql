-- Holler Edge SQLite — recipe_ingredient.quantity_dimension. Contracts 0.5.2,
-- ADR-018 addendum.
--
-- CONFIG, cloud->edge. A column on an existing child row.
--
-- ============================================================================
-- A STORED RECORD OF INTENT SURVIVES A CHANGE IN THE REFERENT. A DERIVED ONE
-- DOES NOT.
-- ============================================================================
--
-- Until now `recipe_ingredient.quantity_micro` was dimensionless in storage:
-- 220_000_000 meant grams only because the chicken it pointed at declared MASS.
-- Nothing recorded what the AUTHOR meant.
--
-- Someone reclassifies chicken from MASS to COUNT — whole birds, a perfectly
-- reasonable thing for a restaurant to do. Every recipe referencing it now
-- silently reinterprets 220_000_000 as 220 BIRDS. No error. Wrong deductions on
-- every plate, and months of wrong rows in an append-only table before a
-- physical count catches the variance.
--
-- This is the argument that settled 0.5.1, one level down. There it was
-- rescaling a sub-recipe; here it is reclassifying an ingredient. Same shape:
-- a stored value that silently means something different after an unrelated
-- edit.
--
-- ============================================================================
-- THE COLUMN IS THE AUTHOR'S UNIT. IT IS NEVER AUTO-FILLED FROM THE REFERENT.
-- ============================================================================
--
-- READ THIS BEFORE IMPLEMENTING A WRITE PATH OR AN AUTHORING UI.
--
-- If anything populates `quantity_dimension` by looking up the referenced
-- item's `dimension` (or the referenced recipe's `output_dimension`), the
-- comparison below becomes x == x and THE GUARD CAN NEVER FIRE. It will look
-- correct in review — the rows will all be consistent, every test will pass,
-- and the column will be decoration.
--
-- **The lazy implementation is the tautological one.** Take the dimension from
-- the unit the author actually chose: they typed "220 g", so it is MASS.
--
-- Where this genuinely fires is bulk recipe import from a spreadsheet — which
-- is how a restaurant with 200 recipes gets onboarded, has no UI to derive
-- anything from, and has no human checking each row.
--
-- ============================================================================
-- WHERE THE CHECK LIVES
-- ============================================================================
--
-- **Cloud (postgres/0021): triggers that REJECT a mismatch at write time.**
-- That store is where recipes are authored and imported.
--
-- **Edge (this file): NO trigger, deliberately.** The edge never authors
-- config — it receives it. A trigger here would abort a config sync over a
-- defect the outlet cannot fix, and a sync that dies is worse than a deduction
-- that reports itself. Instead the resolver compares the stored dimension
-- against the referent at resolution time and returns a `DIMENSION_MISMATCH`
-- deduction gap, completing the sale (ADR-018 Rule 2). That check is only
-- possible now: before this column it was tautological, which is why the gap
-- reason could previously fire at the root and nowhere else.
--
-- The DEFAULT below exists ONLY so the ALTER can add a NOT NULL column; SQLite
-- requires one. It is not a fallback and no writer may rely on it — the
-- OpenAPI schema lists quantity_dimension as required. No recipe has been
-- authored anywhere, so no row takes the default in practice.
ALTER TABLE recipe_ingredient ADD COLUMN quantity_dimension TEXT NOT NULL
    DEFAULT 'COUNT'
    CHECK (quantity_dimension IN ('MASS','VOLUME','COUNT'));

-- ============================================================================
-- RECLASSIFYING AN INGREDIENT IS A MIGRATION, NOT AN EDIT
-- ============================================================================
--
-- Changing an inventory item's dimension changes the MEANING of every recipe
-- that references it. That is not something to do quietly in a config screen
-- while 200 recipes silently change what they deduct.
--
-- So it is forbidden outright while any recipe_ingredient references the item.
-- Detach the references, or migrate the recipes explicitly, and the change
-- becomes possible again — deliberately, and with the recipes in front of you.
--
-- This trigger IS at the edge as well as the cloud, unlike the mismatch check
-- above, because it guards against a local mutation rather than against
-- arriving config: config replace deletes and reinserts, it does not UPDATE a
-- dimension in place.
CREATE TRIGGER inventory_item_dimension_is_frozen_while_referenced
BEFORE UPDATE OF dimension ON inventory_item
WHEN OLD.dimension <> NEW.dimension
 AND EXISTS (SELECT 1 FROM recipe_ingredient WHERE inventory_item_id = OLD.id)
BEGIN
    SELECT RAISE(ABORT,
        'inventory_item.dimension cannot change while a recipe references this item: it would silently change what every one of those recipes deducts. Migrate the recipes explicitly (ADR-018, contracts 0.5.2)');
END;
