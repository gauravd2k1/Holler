-- Holler Cloud PostgreSQL — inventory and recipe CONFIG. Contracts 0.5.0, ADR-018.
-- Mirror of sqlite/0015_m4_inventory_config.sql, whose header carries the full
-- reasoning. CONFIG, cloud->edge. inventory_item and recipe are aggregates;
-- item_unit_conversion, recipe_ingredient and modifier_ingredient_delta are
-- child rows in the config bundle — not AggregateTypes, no sync direction.
--
-- Summary of the decisions recorded in the SQLite mirror:
--   * CURRENT STOCK AND COST ARE NEVER COLUMNS HERE. Modelled literally from
--     docs/spec/inventory.md, inventory_item would be a cloud-owned config row
--     carrying edge-written columns — the half-config, half-transaction row
--     ADR-011 forbids. Stock lives in the ledger and its snapshot; cost lives
--     on the ledger entry.
--   * Quantities are INTEGER MICRO-UNITS: the canonical unit of the dimension
--     (gram / litre / piece) scaled by 10^6, scale carried in the column name.
--     No float anywhere. The binding range limit is JavaScript's 2^53, not
--     i64: TS and Zod carry these as `number`.
--   * Unit conversions are two-tier. Dimensional ones (kg->g, l->ml) are
--     physical constants frozen in code, NOT rows. Only pack conversions
--     ("1 packet paneer = 200g") are item-scoped data, as integer
--     numerator/denominator.
--   * A recipe binds to a SELLABLE UNIT: unique on menu_item_variant_id.
--     Nullable was rejected because NULL != NULL defeats the unique index.
--   * A modifier with no delta row deducts nothing. Absence is never consent.
--
-- THE CLOUD IS THE ENFORCEMENT POINT FOR TWO THINGS SQL CANNOT EXPRESS, and
-- both are write-path code in backend/internal/inventory, not constraints:
--   1. SUB-RECIPE CYCLES AND DEPTH. A DFS cycle check plus
--      MAX_RECIPE_DEPTH = 8, rejected at the write with the offending path
--      named. The edge repeats the check defensively, because an unbounded
--      loop inside confirm_order would wedge a POS mid-service — but the edge
--      degrades to a deduction gap, never to a failed confirm.
--   2. That every menu item has at least one variant (0014), so the NOT NULL
--      recipe binding is satisfiable.
--
-- Note on ids: these tables take app-generated UUIDv7 (§74) with NO DB-side
-- default, unlike the pre-existing 0001_init.sql tables which carry
-- `DEFAULT gen_random_uuid()`. That pattern is not propagated here.

CREATE TABLE inventory_item (
    id                  UUID PRIMARY KEY,
    outlet_id           UUID NOT NULL REFERENCES outlet(id),
    sku                 TEXT NOT NULL,
    name                TEXT NOT NULL,
    category            TEXT,
    dimension           TEXT NOT NULL CHECK (dimension IN ('MASS','VOLUME','COUNT')),
    reorder_level_micro BIGINT,
    par_level_micro     BIGINT,
    storage_location    TEXT,
    is_active           BOOLEAN NOT NULL DEFAULT TRUE,
    -- DEFERRED, landing M5 (ADR-018 §8). NULL in M4, pinned by exact assertion.
    yield_factor_ppm    INTEGER,
    config_version      INTEGER NOT NULL,
    -- Tenant-scoped, never global.
    UNIQUE (outlet_id, sku)
);

CREATE INDEX idx_inventory_item_outlet ON inventory_item(outlet_id, is_active);

CREATE TABLE item_unit_conversion (
    id                  UUID PRIMARY KEY,
    inventory_item_id   UUID NOT NULL REFERENCES inventory_item(id),
    pack_unit_label     TEXT NOT NULL,
    numerator           BIGINT NOT NULL CHECK (numerator > 0),
    denominator         BIGINT NOT NULL CHECK (denominator > 0),
    config_version      INTEGER NOT NULL,
    UNIQUE (inventory_item_id, pack_unit_label)
);

CREATE TABLE recipe (
    id                   UUID PRIMARY KEY,
    menu_item_variant_id UUID NOT NULL UNIQUE REFERENCES menu_item_variant(id),
    -- Snapshotted into every ledger entry, so a year of ledger is readable
    -- without this table.
    name                 TEXT NOT NULL,
    -- Incremented on EVERY edit, cloud-side. Past ledger entries keep the old
    -- number, so an edit never retro-alters a past deduction.
    recipe_version       INTEGER NOT NULL DEFAULT 1 CHECK (recipe_version >= 1),
    config_version       INTEGER NOT NULL
);

CREATE TABLE recipe_ingredient (
    id                  UUID PRIMARY KEY,
    recipe_id           UUID NOT NULL REFERENCES recipe(id),
    component_kind      TEXT NOT NULL CHECK (component_kind IN ('ITEM','SUB_RECIPE')),
    inventory_item_id   UUID REFERENCES inventory_item(id),
    sub_recipe_id       UUID REFERENCES recipe(id),
    quantity_micro      BIGINT NOT NULL CHECK (quantity_micro > 0),
    -- DEFERRED, landing M5.
    yield_factor_ppm    INTEGER,
    sort_order          INTEGER NOT NULL DEFAULT 0,
    config_version      INTEGER NOT NULL,
    -- Exactly one component reference; both-set and neither-set equally wrong.
    CHECK (
        (component_kind = 'ITEM'       AND inventory_item_id IS NOT NULL AND sub_recipe_id IS NULL)
     OR (component_kind = 'SUB_RECIPE' AND sub_recipe_id     IS NOT NULL AND inventory_item_id IS NULL)
    ),
    -- The one cycle a row-level CHECK can see: a recipe containing itself.
    -- The general case is caught by the recursive-CTE reachability check the
    -- write path runs before accepting any SUB_RECIPE row; see the SQLite
    -- mirror's header, which pins the query. The edge resolver carries an
    -- independent depth/visited backstop and must terminate on a cyclic graph
    -- even if a bad row exists, because an unbounded walk inside
    -- confirm_order's transaction hangs a till mid-service.
    CHECK (sub_recipe_id IS NULL OR sub_recipe_id <> recipe_id)
);

CREATE INDEX idx_recipe_ingredient_recipe ON recipe_ingredient(recipe_id);

CREATE TABLE modifier_ingredient_delta (
    id                      UUID PRIMARY KEY,
    menu_item_modifier_id   UUID NOT NULL REFERENCES menu_item_modifier(id),
    inventory_item_id       UUID NOT NULL REFERENCES inventory_item(id),
    -- Signed: "Extra Paneer" positive, "No Onion" negative. No CHECK on sign,
    -- and none on zero: a deliberate zero is a costed modifier that consumes
    -- nothing, which is different information from an absent row.
    quantity_micro          BIGINT NOT NULL,
    config_version          INTEGER NOT NULL,
    UNIQUE (menu_item_modifier_id, inventory_item_id)
);

CREATE INDEX idx_modifier_ingredient_delta_modifier
    ON modifier_ingredient_delta(menu_item_modifier_id);
