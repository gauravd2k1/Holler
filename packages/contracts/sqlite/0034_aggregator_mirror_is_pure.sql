-- M6 Phase C. MAKE THE EDGE'S aggregator_order MIRROR PURE.
--
-- 0032 gave aggregator_order two edge-written columns, accepted_at and
-- local_order_id, and guarded them by omitting them from the upsert's SET list
-- so a later cloud document could not clear them. That guard worked and was
-- tested. It was still the wrong shape.
--
-- WHY IT WAS WRONG EVEN THOUGH IT WORKED. aggregator_order is
-- CLOUD-AUTHORITATIVE (ADR-022). Two edge-written columns on it are split
-- authority however carefully the SET list is maintained, and the contract
-- rubric says so directly: no split-authority columns -- SPLIT THE AGGREGATE
-- INSTEAD. A guard is a documented obligation, and this repository's own history
-- is a list of documented obligations that held right up until they didn't.
--
-- It also grows. printed_at and closed_at are the obvious next two, and each
-- would arrive with the same reasoning and the same guard.
--
-- WHAT REPLACES IT: NOTHING, BECAUSE ACCEPTANCE IS ALREADY DERIVABLE.
-- Accepting a document IS creating the local order for it. So:
--
--   accepted    <=>  a row exists in "order" whose external_order_id matches
--                    the document's
--   accepted_at  =   that order's created_at
--   local_order_id = that order's id
--
-- No second table, no denormalised copy, and nothing that can drift from the
-- thing it describes. The mirror becomes read-only with NO exceptions, which is
-- a sentence with no caveat to maintain.

-- The partial index on accepted_at goes with the column it indexes.
DROP INDEX IF EXISTS aggregator_order_unaccepted_idx;

ALTER TABLE aggregator_order DROP COLUMN accepted_at;
ALTER TABLE aggregator_order DROP COLUMN local_order_id;

-- The derivation's hot path. "Which documents has nobody accepted?" becomes a
-- LEFT JOIN from aggregator_order to "order" on external_order_id, and without
-- this index that is a scan of every order the outlet has ever taken -- on the
-- one query a till runs to decide what to show a cashier.
--
-- outlet_id leads because every edge query is outlet-scoped; external_order_id
-- is nullable on "order" (null for POS, QR and direct orders, which are most of
-- them) and SQLite indexes nulls, so the partial clause keeps the index to the
-- rows that can actually match.
CREATE INDEX idx_order_external_order_id
    ON "order" (outlet_id, external_order_id)
    WHERE external_order_id IS NOT NULL;
