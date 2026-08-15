-- Holler Edge SQLite — print_job gains an invoice reference.
-- Contracts 0.4.5, ADR-014 + ADR-016 addendum.
--
-- EDGE-LOCAL, unchanged. print_job has no Postgres mirror and is deliberately
-- absent from AggregateType (ADR-014 §3). A print job is a shop-floor
-- mechanism, not a business record, and it gains no sync direction here.
--
-- WHY. T10 built the GST invoice render template (edge/printer/src/template.rs,
-- 4c5b045) and its verification gate found it had ZERO callers: print_job.kot_id
-- was `NOT NULL REFERENCES kot(id)`, so there was no shape by which an invoice
-- could become a print job. A rendered invoice could not reach a printer at all.
-- The renderer is correct and tested; nothing could dispatch it.
--
-- SHAPE CHOSEN: one table, one spool, exactly one of kot_id / invoice_id set.
--
-- The alternative -- a separate invoice_print_job table -- was considered and
-- rejected. It would duplicate the spool's queue, retry-with-backoff, attempt
-- counting and failure surfacing (edge/printer/src/spool.rs), and those are
-- precisely the parts that are hard to get right and must not diverge between
-- two copies. A print job is a print job; what it prints does not change how it
-- queues, retries, or fails. The cost of this choice is that kot_id becomes
-- nullable, which is a change to an existing frozen column and the reason this
-- is a rebuild rather than a plain ALTER.
--
-- SQLite cannot drop NOT NULL in place, so this is the standard 12-step table
-- rebuild. Existing rows all carry a kot_id and are copied unchanged.

CREATE TABLE print_job_new (
    id                  TEXT PRIMARY KEY,       -- UUIDv7, minted by the edge
    -- Exactly one of these is set; see the CHECK below. Both nullable so that
    -- either kind of job is representable, never so that a job can reference
    -- nothing.
    kot_id              TEXT REFERENCES kot(id),
    invoice_id          TEXT REFERENCES invoice(id),
    printer_id          TEXT NOT NULL REFERENCES printer(id),
    status              TEXT NOT NULL DEFAULT 'QUEUED' CHECK (status IN
                          ('QUEUED','PRINTING','PRINTED','FAILED')),
    attempt_count       INTEGER NOT NULL DEFAULT 0,
    last_error          TEXT,
    created_at          TEXT NOT NULL,           -- ISO8601 UTC
    updated_at          TEXT NOT NULL,
    -- A job prints exactly one document. Neither-set was the old NOT NULL's
    -- job; both-set would make "what does this print?" ambiguous at the spool,
    -- which is the one question the spool must never have to guess.
    CHECK ((kot_id IS NOT NULL AND invoice_id IS NULL)
        OR (kot_id IS NULL AND invoice_id IS NOT NULL))
);

INSERT INTO print_job_new
    (id, kot_id, invoice_id, printer_id, status, attempt_count, last_error, created_at, updated_at)
SELECT
     id, kot_id, NULL,       printer_id, status, attempt_count, last_error, created_at, updated_at
  FROM print_job;

DROP TABLE print_job;
ALTER TABLE print_job_new RENAME TO print_job;

-- Idempotency for KOT jobs, unchanged in meaning from 0005: re-queueing the
-- same ticket at the same printer is a no-op rather than a duplicate slip.
--
-- Now PARTIAL. SQLite treats NULLs as distinct in a UNIQUE index, so the
-- original unqualified index would have permitted unlimited (NULL, printer)
-- rows once kot_id became nullable -- silently losing the idempotency it
-- exists to provide for invoice jobs' sake. Each kind gets its own guarded
-- index instead.
CREATE UNIQUE INDEX idx_print_job_kot_printer
    ON print_job(kot_id, printer_id) WHERE kot_id IS NOT NULL;

-- The same guarantee for invoices: one job per (invoice, printer). A reprint
-- is a deliberate act that must mint a new job id, not an accidental duplicate
-- from a double-tap.
CREATE UNIQUE INDEX idx_print_job_invoice_printer
    ON print_job(invoice_id, printer_id) WHERE invoice_id IS NOT NULL;

CREATE INDEX idx_print_job_status ON print_job(status, created_at);
