-- Holler Edge SQLite — inventory and recipe CONFIG. Contracts 0.5.0, ADR-018.
--
-- CONFIG, cloud->edge, all five tables. A raw material's definition and a
-- recipe are management decisions, exactly like menu_item and tax_profile.
-- `inventory_item` and `recipe` are aggregates; `item_unit_conversion`,
-- `recipe_ingredient` and `modifier_ingredient_delta` are CHILD ROWS that
-- travel inside their parent's config bundle — the menu_item_variant /
-- station_printer / invoice_line precedent. None of the three is an
-- AggregateType and none has a sync direction. Ever.
--
-- ============================================================================
-- THE RULE THIS FILE EXISTS TO ENFORCE
-- ============================================================================
--
-- CURRENT STOCK IS NEVER A COLUMN HERE.
--
-- docs/spec/inventory.md lists "Current cost, Weighted average cost, Last
-- purchase price" as fields of an inventory item, and docs/domain/
-- INVENTORY_MODEL.md describes current stock as a materialised projection.
-- Modelled literally, inventory_item would be a cloud-OWNED config row
-- carrying four EDGE-WRITTEN columns — the precise half-config,
-- half-transaction row ADR-011 forbids, and a silent second writer of the
-- kind §50.1 exists to prevent.
--
-- So: stock lives in the ledger (0016) and its sealed snapshot (0017). Cost
-- lives on the ledger entry, because a weighted average is derived from
-- edge-recorded purchases. Neither belongs on a row the cloud owns.
--
-- ============================================================================
-- QUANTITIES ARE INTEGER MICRO-UNITS. NO FLOAT, ANYWHERE, EVER.
-- ============================================================================
--
-- The money-is-paise rule generalised, with ONE scaling rule rather than a
-- per-dimension choice: the canonical unit of the dimension, scaled by 10^6,
-- with the scale carried in the column name.
--
--     MASS   -> micro-grams      VOLUME -> micro-litres     COUNT -> micro-pieces
--
-- An earlier draft used mg / ml / milli-piece and accepted a 1 ml precision
-- floor. It was revised because the mitigation it offered — amortise an
-- essence across a prep batch — needs semi_finished_batch, which ADR-018 §7
-- defers to M5. A workaround requiring a table that does not ship is not a
-- mitigation. Micro-units put 0.5 piece and 0.5 ml on the same footing.
--
-- THE BINDING RANGE LIMIT IS JAVASCRIPT, NOT i64. TypeScript and Zod carry
-- these as `number`, so the ceiling is Number.MAX_SAFE_INTEGER (2^53, ~9.0e15),
-- not i64's 9.2e18. A 50 kg sack is 5e10 micro-grams — five orders of
-- magnitude of headroom. Intermediates are i128 in Rust and never cross the
-- wire.

-- ---------------------------------------------------------------------------
-- inventory_item — AGGREGATE, cloud->edge
-- ---------------------------------------------------------------------------
CREATE TABLE inventory_item (
    id                  TEXT PRIMARY KEY,       -- UUIDv7, app-generated (§74). Never a DB-side default.
    outlet_id           TEXT NOT NULL REFERENCES outlet(id),
    sku                 TEXT NOT NULL,
    name                TEXT NOT NULL,
    category            TEXT,

    -- Fixes which canonical unit every *_micro value on this item means. An
    -- item's dimension never changes; changing it would silently reinterpret
    -- every historical ledger row that referenced it, which is why the ledger
    -- snapshots this value rather than joining for it (0016).
    dimension           TEXT NOT NULL CHECK (dimension IN ('MASS','VOLUME','COUNT')),

    -- Low-stock signalling. Config: the manager decides the threshold.
    -- Crossing it is a SIGNAL, never a block — ADR-018 Rule 1.
    reorder_level_micro INTEGER,
    par_level_micro     INTEGER,

    storage_location    TEXT,
    is_active           INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),

    -- DEFERRED, landing M5 (ADR-018 §8), and INERT until then.
    --
    -- The default is the IDENTITY -- 1_000_000 ppm = 100% -- nothing reads it,
    -- and a round-trip test pins it to exactly that value. A yield quietly
    -- applied in M4 would change every deduction in the product while looking
    -- like an ordinary data-entry field, which is the worst way for a number
    -- to become wrong. It is inert on purpose, not merely unused.
    --
    -- It lands NOW because adding a column to a multi-million-row table on a
    -- spinning disk, at an outlet, during an upgrade, is not an operation this
    -- product should ever have to perform.
    -- ppm: parts per million, so 92.5% is 925000. Integer, like everything else.
    yield_factor_ppm    INTEGER NOT NULL DEFAULT 1000000 CHECK (yield_factor_ppm > 0),

    config_version      INTEGER NOT NULL,

    -- Tenant-scoped, never global: an SKU is unique within the outlet that
    -- stocks it. Two outlets may both call something PANEER-1KG.
    UNIQUE (outlet_id, sku)
);

CREATE INDEX idx_inventory_item_outlet ON inventory_item(outlet_id, is_active);

-- ---------------------------------------------------------------------------
-- item_unit_conversion — CHILD ROW of inventory_item. Not an aggregate.
-- ---------------------------------------------------------------------------
--
-- TIER 2 of a two-tier scheme. Tier 1 — kg->g, l->ml, dozen->piece — is
-- DIMENSIONAL, global, and frozen as a constant map in packages/contracts, NOT
-- as a table: those are physical constants, not configuration, and giving them
-- a config write path would create a way to get them wrong per tenant.
--
-- Tier 2 is what genuinely varies. "1 packet paneer = 200 g" is a property of
-- THAT paneer, not of packets: two outlets, or two suppliers, may disagree,
-- and a global packet size would be wrong for one of them.
--
-- Ratios are integer numerator/denominator. A conversion is a rational
-- multiplication, never a decimal factor — the same reason money is paise.
CREATE TABLE item_unit_conversion (
    id                  TEXT PRIMARY KEY,       -- UUIDv7, app-generated
    inventory_item_id   TEXT NOT NULL REFERENCES inventory_item(id),
    pack_unit_label     TEXT NOT NULL,          -- 'packet', 'sack', 'crate', 'bottle'

    -- The dimension the label is measured IN, which need NOT be the item's own.
    --
    -- CROSS-DIMENSION CONVERSION IS ITEM-SCOPED, ALWAYS. Oil is bought in kg
    -- and cooked in ml. Density varies per ingredient, so g<->ml is NOT a
    -- physical constant and has no place in the frozen map: a single global
    -- g->ml factor would be a wrong number for every ingredient it touched.
    -- The frozen map holds WITHIN-dimension conversions only (kg->g, l->ml,
    -- dozen->piece); anything crossing dimensions is a property of the
    -- substance and lives here, per item.
    source_dimension    TEXT NOT NULL CHECK (source_dimension IN ('MASS','VOLUME','COUNT')),

    numerator           INTEGER NOT NULL CHECK (numerator > 0),
    denominator         INTEGER NOT NULL CHECK (denominator > 0),
    config_version      INTEGER NOT NULL,
    -- A pack label may NEVER be a unit the frozen dimensional map already
    -- defines. If an item could define its own kg->g there would be two
    -- sources of truth for the same conversion and a silent precedence rule
    -- deciding which wins -- and a silent precedence rule between two
    -- disagreeing numbers is how a deduction becomes quietly wrong.
    -- Tier 1 is physics and lives in code; Tier 2 is per-item packaging and
    -- lives here. The two sets are disjoint by construction.
    CHECK (lower(pack_unit_label) NOT IN (
        'mg','g','kg','ml','l','litre','liter','piece','pieces','pc','dozen'
    )),
    UNIQUE (inventory_item_id, pack_unit_label)
);

-- ---------------------------------------------------------------------------
-- recipe — AGGREGATE, cloud->edge. One per SELLABLE UNIT.
-- ---------------------------------------------------------------------------
--
-- A recipe binds at the same grain as a price: the variant, which after 0014
-- every sellable item has. UNIQUE on menu_item_variant_id alone is the whole
-- constraint — a variant belongs to exactly one item, which belongs to exactly
-- one brand, so this is inherently tenant-scoped and needs no composite key.
-- ADR-018 §2 describes it as unique per (tenant, item, variant); unique on the
-- variant is the same rule expressed tighter, with no denormalised columns
-- that could disagree with each other.
--
-- NULLABLE WAS REJECTED ON A STRUCTURAL DEFECT, not a preference: NULL != NULL,
-- so a unique index over a nullable variant would not prevent two "applies to
-- all variants" recipes for one item. See 0014's header.
--
-- Four variants means four recipes. That duplication is mitigated in the
-- AUTHORING UI with copy-recipe-from-variant, and NEVER in the data layer: an
-- authoring convenience that becomes a resolution rule in the deduction path
-- is exactly the fallback branch this design removes.
CREATE TABLE recipe (
    id                  TEXT PRIMARY KEY,       -- UUIDv7, app-generated
    menu_item_variant_id TEXT NOT NULL UNIQUE REFERENCES menu_item_variant(id),

    -- Snapshotted into every ledger entry this recipe produces, so a year of
    -- ledger is readable without this table — which, being config, will have
    -- been overwritten by sync many times over. ADR-018 §6.
    name                TEXT NOT NULL,

    -- Incremented cloud-side on EVERY edit. Past ledger entries keep the old
    -- number, so a recipe edit can never retro-alter a past deduction. This is
    -- a provenance number, not a recipe-history feature; M4 ships no history.
    recipe_version      INTEGER NOT NULL DEFAULT 1 CHECK (recipe_version >= 1),

    config_version      INTEGER NOT NULL
);

-- ---------------------------------------------------------------------------
-- recipe_ingredient — CHILD ROW of recipe. Not an aggregate.
-- ---------------------------------------------------------------------------
--
-- A component is EITHER a raw material OR a sub-recipe, never both and never
-- neither — the print_job.invoice_id precedent (0010), where neither-set and
-- both-set are equally rejected rather than one silently winning.
--
-- SUB-RECIPES RESOLVE TRANSITIVELY at deduction time: a semi-finished
-- component expands to its leaves. Physical batch production, with expected
-- versus actual yield, is semi_finished_batch and is DEFERRED TO M5 (ADR-018
-- §7) — §81's M4 list does not include it and it is meaningless without the
-- procurement side.
--
-- ============================================================================
-- CYCLES AND DEPTH ARE BOUNDED AT THREE LEVELS. A CYCLE HERE WEDGES A POS.
-- ============================================================================
--
-- This is the sharpest hazard in the file. Resolution runs INSIDE
-- confirm_order's transaction, so an unbounded walk of a cyclic graph does not
-- produce a wrong number — it hangs the till, mid-service, holding a write
-- lock on the only SQLite writer at the outlet.
--
-- LEVEL 1 — row CHECK, below: sub_recipe_id <> recipe_id. A CHECK cannot see a
-- cycle of length two or more (it evaluates one row, and SQLite forbids
-- subqueries in CHECK), but it catches the shortest and most easily
-- mis-authored one: a recipe containing itself.
--
-- LEVEL 2 — CLOUD WRITE TIME, the real guard. Before accepting any
-- recipe_ingredient with component_kind = 'SUB_RECIPE', the cloud runs a
-- recursive-CTE reachability check and rejects the write if the proposed child
-- can reach the parent, naming the offending path. The query, pinned here so
-- both stores implement the same thing:
--
--     WITH RECURSIVE reach(recipe_id) AS (
--         SELECT :proposed_sub_recipe_id
--         UNION                          -- UNION, not UNION ALL: dedup is
--         SELECT ri.sub_recipe_id        -- what makes this terminate on a
--           FROM recipe_ingredient ri    -- graph that already contains a
--           JOIN reach r                 -- cycle, rather than looping.
--             ON ri.recipe_id = r.recipe_id
--          WHERE ri.sub_recipe_id IS NOT NULL
--     )
--     SELECT 1 FROM reach WHERE recipe_id = :parent_recipe_id;
--
-- A row returned means the edge would cycle. Reject the write.
-- Depth is bounded in the same pass: MAX_RECIPE_DEPTH = 8.
--
-- LEVEL 3 — EDGE RESOLVER BACKSTOP, and it is not optional. The resolver
-- carries its own visited-set and depth counter and MUST TERMINATE ON A CYCLIC
-- GRAPH EVEN IF A BAD ROW EXISTS. Config arrives over a wire from a cloud that
-- may be older than Level 2, or from a database restored from before it. The
-- edge never assumes the cloud validated anything.
--
-- The edge backstop degrades to a DEDUCTION GAP (reason 'CYCLE' or
-- 'DEPTH_EXCEEDED'), never to a failed confirm — ADR-018 Rule 2. A cycle that
-- reaches an outlet loses that item's stock accuracy for that sale and reports
-- itself; it does not stop the restaurant trading, and it does not hang it.
CREATE TABLE recipe_ingredient (
    id                  TEXT PRIMARY KEY,       -- UUIDv7, app-generated
    recipe_id           TEXT NOT NULL REFERENCES recipe(id),

    component_kind      TEXT NOT NULL CHECK (component_kind IN ('ITEM','SUB_RECIPE')),
    inventory_item_id   TEXT REFERENCES inventory_item(id),
    sub_recipe_id       TEXT REFERENCES recipe(id),

    -- Positive: a recipe consumes. Negative deltas are a MODIFIER concept, and
    -- they live in modifier_ingredient_delta below.
    quantity_micro      INTEGER NOT NULL CHECK (quantity_micro > 0),

    -- DEFERRED, landing M5 (ADR-018 §8), and INERT: identity default, nothing
    -- reads it, pinned by exact assertion. See inventory_item.yield_factor_ppm.
    yield_factor_ppm    INTEGER NOT NULL DEFAULT 1000000 CHECK (yield_factor_ppm > 0),

    sort_order          INTEGER NOT NULL DEFAULT 0,
    config_version      INTEGER NOT NULL,

    -- Exactly one component reference. Both-set and neither-set are equally
    -- wrong; neither is coerced into the other.
    CHECK (
        (component_kind = 'ITEM'       AND inventory_item_id IS NOT NULL AND sub_recipe_id IS NULL)
     OR (component_kind = 'SUB_RECIPE' AND sub_recipe_id     IS NOT NULL AND inventory_item_id IS NULL)
    ),

    -- The one cycle a row-level CHECK can see: a recipe containing itself.
    -- This is the shortest cycle and the easiest to author by accident (pick
    -- the wrong row in a dropdown), so it is worth catching in the store even
    -- though the store cannot catch the general case.
    CHECK (sub_recipe_id IS NULL OR sub_recipe_id <> recipe_id)
);

CREATE INDEX idx_recipe_ingredient_recipe ON recipe_ingredient(recipe_id);

-- ---------------------------------------------------------------------------
-- modifier_ingredient_delta — CHILD of menu_item_modifier. Not an aggregate.
-- ---------------------------------------------------------------------------
--
-- Deduction is modifier-aware, and this is the separate table that makes it so
-- WITHOUT expanding recipe_ingredient to cover variants. It is a child of a
-- child (menu_item_modifier belongs to menu_item), so it rides in the
-- MenuItem config payload and needs no route of its own.
--
-- SIGNED, deliberately: "Extra Paneer" is positive, "No Onion" is negative.
--
-- A MODIFIER WITH NO ROW HERE DEDUCTS NOTHING. Absence is never read as
-- consent — 0.4.7's printer_role rule applied to ingredients. A modifier
-- nobody has costed must move no stock, rather than guessing at a quantity.
CREATE TABLE modifier_ingredient_delta (
    id                      TEXT PRIMARY KEY,   -- UUIDv7, app-generated
    menu_item_modifier_id   TEXT NOT NULL REFERENCES menu_item_modifier(id),
    inventory_item_id       TEXT NOT NULL REFERENCES inventory_item(id),

    -- Signed. No CHECK on sign, and no CHECK on zero either: a deliberate zero
    -- row is a costed modifier that happens to consume nothing, which is
    -- different information from an absent row.
    quantity_micro          INTEGER NOT NULL,

    config_version          INTEGER NOT NULL,
    UNIQUE (menu_item_modifier_id, inventory_item_id)
);

CREATE INDEX idx_modifier_ingredient_delta_modifier
    ON modifier_ingredient_delta(menu_item_modifier_id);
