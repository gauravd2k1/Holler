-- M6 Phase C. The cloud half of making aggregator_order carry no acceptance
-- state. Mirrors sqlite/0034 -- see that file for the full reasoning.
--
-- WHY THE CLOUD DROPS THEM TOO, rather than keeping them "just in case".
-- NOTHING WRITES THEM HERE. Acceptance happens at a till: a human accepts a
-- document and the edge creates a local `order` for it, which then replays up
-- edge->cloud like every other order. So the cloud derives acceptance exactly as
-- the edge does -- from the order's existence and its external_order_id -- and a
-- column nothing writes is a column that does not exist.
--
-- Keeping them cloud-side would also invite the next reader to populate them
-- from somewhere, which is how the split authority this migration removes gets
-- reintroduced at the other end.
DROP INDEX IF EXISTS aggregator_order_unaccepted_idx;

ALTER TABLE aggregator_order DROP COLUMN accepted_at;
ALTER TABLE aggregator_order DROP COLUMN local_order_id;

-- The same derivation index as the edge's, for the same reason: without it,
-- "which documents were accepted?" scans every order the outlet has taken.
CREATE INDEX IF NOT EXISTS idx_order_external_order_id
    ON "order" (outlet_id, external_order_id)
    WHERE external_order_id IS NOT NULL;
