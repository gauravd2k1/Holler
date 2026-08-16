-- Holler Cloud PostgreSQL — printer_role. Contracts 0.4.7.
-- Mirror of sqlite/0012_printer_role.sql, whose header carries the full
-- reasoning. CONFIG, cloud->edge, child row in the config bundle like
-- station_printer (0006). Not an AggregateType; no sync direction of its own.
--
-- Summary of the decision recorded in the SQLite mirror:
--   * An invoice print job needs a target printer. KOTs route
--     station -> station_printer; a bill has no station, so nothing in the
--     frozen contract could answer "which printer prints the bill".
--   * A join table rather than a `role` column on `printer`, because `printer`
--     is built by struct literal in eight-plus places across three crates plus
--     the TS/Go mirrors — widening it breaks all of them at once, the exact
--     cascade contracts 0.4.5 caused (docs/retro.md, 2026-08-15). Adding a row
--     shape breaks nothing that compiles today.
--   * Two rows also model a shared printer honestly, without a BOTH enum
--     member that every reader has to special-case.
--   * A printer with no row here has no role. Absence is never read as
--     permission; an outlet with no BILL printer fails loudly at issue time.
CREATE TABLE printer_role (
    printer_id      UUID NOT NULL REFERENCES printer(id),
    role            TEXT NOT NULL CHECK (role IN ('KITCHEN','BILL')),
    config_version  INTEGER NOT NULL,
    PRIMARY KEY (printer_id, role)
);

CREATE INDEX idx_printer_role_role ON printer_role(role);
