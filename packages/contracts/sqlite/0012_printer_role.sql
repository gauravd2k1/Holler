-- Holler Edge SQLite — printer_role. Contracts 0.4.7, ADR-014 + ADR-016 addendum.
--
-- CONFIG, cloud->edge, exactly like station_printer (0005): the cloud owns it,
-- bumps outlet.config_version, the edge caches and reads it. Not an
-- AggregateType, no sync direction of its own — it travels in the config
-- bundle as a child row, the station_printer / menu_item_variant precedent.
--
-- WHY. T10 built the GST invoice renderer; its gate found it had zero callers.
-- 0.4.5 gave print_job an invoice_id so an invoice could become a print job,
-- and the enqueue path landed at 3e217ee. But `queue_invoice_for_print` has to
-- be told WHICH printer, and nothing in the frozen contract answers that: the
-- printer table (0005) carries name, connection_kind, address, paper_width_mm
-- and is_active, and nothing else. KOTs route station -> station_printer. A
-- bill has no station, so a bill has no route.
--
-- The builder was explicitly told not to invent a convention and did not — it
-- takes printer_id as a caller-supplied argument and only validates it. This
-- table is what lets the caller answer the question without matching on a name
-- string, which CLAUDE.md's "no magic values" rule forbids and which would
-- break the first time someone renamed a printer.
--
-- WHY A JOIN TABLE AND NOT A COLUMN ON printer.
--
-- A `role` column on `printer` was the orchestrator's first recommendation and
-- was rejected after counting the blast radius. `printer` is constructed by
-- struct literal in at least eight places across edge/database, edge/printer
-- and apps/pos tests, plus the TS and Go mirrors and the fixture. Adding a
-- field breaks every one of them simultaneously — the exact multi-crate
-- cascade contracts 0.4.5 caused and that docs/retro.md's 2026-08-15 entry
-- records. This table adds a row shape instead of widening an existing one, so
-- **nothing that compiles today stops compiling**.
--
-- It is also the more honest model. One physical printer at a small outlet
-- often prints both tickets and bills; a single-valued column forces a
-- BOTH member whose meaning has to be special-cased at every read. Two rows
-- say the same thing without the enum gymnastics, and it mirrors how routing
-- already works.
--
-- A printer with NO row here has no role and is not a candidate for either
-- path. That is deliberate: absence must not be silently read as "sure, print
-- bills to it". An outlet that has not configured a bill printer must fail
-- loudly at issue time, naming the problem, exactly as a missing HSN/SAC code
-- does (0011).
CREATE TABLE printer_role (
    printer_id      TEXT NOT NULL REFERENCES printer(id),
    -- KITCHEN: eligible for KOT jobs, routed via station_printer as before —
    -- this table does not replace that routing, it classifies the device.
    -- BILL: eligible for invoice jobs.
    role            TEXT NOT NULL CHECK (role IN ('KITCHEN','BILL')),
    config_version  INTEGER NOT NULL,
    PRIMARY KEY (printer_id, role)
);

CREATE INDEX idx_printer_role_role ON printer_role(role);
