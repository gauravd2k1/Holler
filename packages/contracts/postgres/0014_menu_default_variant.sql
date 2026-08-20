-- Holler Cloud PostgreSQL — menu_item_variant.is_default. Contracts 0.5.0, ADR-018 §2.1.
-- Mirror of sqlite/0014_menu_default_variant.sql, whose header carries the full
-- reasoning. CONFIG, cloud->edge, a column on an existing child row of
-- menu_item. Not a new aggregate.
--
-- Summary of the decision recorded in the SQLite mirror:
--   * A recipe binds NOT NULL to a sellable unit, one unique index, no
--     fallback branch. Nullable was rejected because NULL != NULL defeats the
--     unique index, so two "all variants" recipes could coexist for one item.
--   * NOT NULL needs every sellable item to resolve to a variant, and today it
--     does not: order_item.variant_id is nullable and seeded items exist with
--     no variant.
--   * So: a default variant per item, auto-created as 'Regular' at delta 0 for
--     items authored with none, and add_order_item stamps
--     variant_id = chosen ?? default at line creation. The rule lives at the
--     write, once, not in every reader.
--   * order_item.variant_id stays nullable and historical rows are never
--     backfilled — 0.5.0 stays additive and no outlet database is migrated.
--   * "Every item has >= 1 variant" is a cross-row invariant no constraint can
--     express; it is enforced at THIS store's menu write path, in devseed, and
--     by a CI assertion. ADR-018 Rule 2 is the safety net.
--
-- The cloud is the enforcement point: it owns menu config, so the
-- auto-creation of a default variant happens here, and the edge only ever
-- receives items that already have one.
ALTER TABLE menu_item_variant ADD COLUMN is_default BOOLEAN NOT NULL DEFAULT FALSE;

-- Partial, and deliberately so: it enforces AT MOST ONE default per item,
-- rather than distinguishing rows by a nullable value — the misuse the recipe
-- binding rejects.
CREATE UNIQUE INDEX idx_menu_item_variant_one_default
    ON menu_item_variant(menu_item_id) WHERE is_default;
