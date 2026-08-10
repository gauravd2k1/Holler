-- Holler Edge SQLite — Milestone 2 kitchen: stations, item routing, printers,
-- the print spool, and the KOT status trail. Contracts 0.3.0, ADR-014.
--
-- The authority split this migration encodes (§50.1):
--   station, menu_item_station,
--   printer, station_printer          -> CONFIG, cloud→edge, config_version.
--   kot (already present), print_job  -> EDGE-authoritative.
-- No table below is half-config, half-transaction.

-- A production destination: MAIN_KITCHEN, TANDOOR, BAR, DESSERT, BEVERAGE,
-- PACKAGING, CHINESE (docs/spec/kitchen.md §Stations). Cloud-owned config.
CREATE TABLE station (
    id                  TEXT PRIMARY KEY,       -- UUIDv7, minted by the cloud
    outlet_id           TEXT NOT NULL REFERENCES outlet(id),
    -- Stable machine code. kot.station stores this string rather than
    -- station_id, so a KOT already on the pass survives a station rename and
    -- stays readable in an outbox payload the cloud has not yet drained.
    code                TEXT NOT NULL,
    name                TEXT NOT NULL,
    sort_order          INTEGER NOT NULL DEFAULT 0,
    is_active           INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0,1)),
    config_version      INTEGER NOT NULL
);

-- Tenant-scoped, never global: two outlets both having a TANDOOR is normal.
CREATE UNIQUE INDEX idx_station_outlet_code ON station(outlet_id, code);
CREATE INDEX idx_station_outlet_id ON station(outlet_id);

-- An item may route to more than one station, so this is a join table rather
-- than a station_id column on menu_item. A thali hits MAIN_KITCHEN and
-- TANDOOR and must produce a ticket at both.
CREATE TABLE menu_item_station (
    menu_item_id        TEXT NOT NULL REFERENCES menu_item(id),
    station_id          TEXT NOT NULL REFERENCES station(id),
    config_version      INTEGER NOT NULL,
    PRIMARY KEY (menu_item_id, station_id)
);

CREATE INDEX idx_menu_item_station_station_id ON menu_item_station(station_id);

CREATE TABLE printer (
    id                  TEXT PRIMARY KEY,       -- UUIDv7, minted by the cloud
    outlet_id           TEXT NOT NULL REFERENCES outlet(id),
    name                TEXT NOT NULL,
    -- Transport, not vendor. Brand differences are edge/printer adapter
    -- details. Label printers are excluded from Milestone 2 (§81).
    connection_kind     TEXT NOT NULL CHECK (connection_kind IN
                          ('ESCPOS_NETWORK','ESCPOS_USB','ESCPOS_BLUETOOTH')),
    -- Interpreted only by the matching adapter: host:port, USB path, or MAC.
    address             TEXT NOT NULL,
    paper_width_mm      INTEGER NOT NULL CHECK (paper_width_mm IN (58,80)),
    is_active           INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0,1)),
    config_version      INTEGER NOT NULL
);

CREATE UNIQUE INDEX idx_printer_outlet_name ON printer(outlet_id, name);
CREATE INDEX idx_printer_outlet_id ON printer(outlet_id);

-- Station → printer routing (docs/spec/hardware-printing.md §Printing).
-- Many-to-many both ways: one printer can serve two stations, and a station
-- can fan out to two printers.
CREATE TABLE station_printer (
    station_id          TEXT NOT NULL REFERENCES station(id),
    printer_id          TEXT NOT NULL REFERENCES printer(id),
    config_version      INTEGER NOT NULL,
    PRIMARY KEY (station_id, printer_id)
);

CREATE INDEX idx_station_printer_printer_id ON station_printer(printer_id);

-- The print spool. EDGE-LOCAL: it is never pushed to the cloud, has no
-- local_outbox event, and is deliberately absent from AggregateType.
--
-- This mirrors the refresh_token precedent (0.2.1) from the other side: that
-- table is cloud-only and was deliberately excluded from AggregateType because
-- listing it would promise a sync direction for something that never syncs. A
-- spool entry is a fact about one outlet's paper and one printer's socket.
--
-- It lives in packages/contracts/sqlite despite crossing no boundary because
-- this directory is the single source of the edge schema; splitting it would
-- leave the edge with two migration sources to keep in step.
CREATE TABLE print_job (
    id                  TEXT PRIMARY KEY,       -- UUIDv7, minted by the edge
    kot_id              TEXT NOT NULL REFERENCES kot(id),
    printer_id          TEXT NOT NULL REFERENCES printer(id),
    status              TEXT NOT NULL DEFAULT 'QUEUED' CHECK (status IN
                          ('QUEUED','PRINTING','PRINTED','FAILED')),
    attempt_count       INTEGER NOT NULL DEFAULT 0,
    last_error          TEXT,
    created_at          TEXT NOT NULL,           -- ISO8601 UTC
    updated_at          TEXT NOT NULL
);

-- One job per (kot, printer). A late printer ack must never cause a duplicate
-- KOT (docs/spec/hardware-printing.md §Printing), and the cheapest way to
-- guarantee that is to make the duplicate unrepresentable rather than to have
-- the retry path remember not to create one.
CREATE UNIQUE INDEX idx_print_job_kot_printer ON print_job(kot_id, printer_id);
-- Drives the spool sweep: pending work, oldest first.
CREATE INDEX idx_print_job_status ON print_job(status, created_at);

-- KOT status history. The kot row carries the CURRENT status; this carries how
-- it got there, which is what kitchen-timing analytics actually need. Append
-- only — a transition is a fact that happened, never edited or deleted.
--
-- It is not synced as its own aggregate: each row's cloud counterpart is a
-- KOTStatusChanged outbox event, so the transition crosses the boundary as an
-- event rather than as a second authority over kot state.
CREATE TABLE kot_status_history (
    id                    TEXT PRIMARY KEY,     -- UUIDv7, minted by the edge
    kot_id                TEXT NOT NULL REFERENCES kot(id),
    status                TEXT NOT NULL CHECK (status IN
                            ('NEW','ACKNOWLEDGED','PREPARING','READY','SERVED','CANCELLED')),
    changed_by_device_id  TEXT NOT NULL REFERENCES device(id),
    changed_at            TEXT NOT NULL          -- ISO8601 UTC, edge clock (§50.1)
);

CREATE INDEX idx_kot_status_history_kot_id ON kot_status_history(kot_id, changed_at);

-- Milestone 2 lands preparation_time_minutes, per the 0.2.4 deferral note in
-- 0004_order_canonical_fields.sql. It was synthesized as NULL and the
-- order-level round-trip test pinned that value; persisting it means that pin
-- moves to the column. Nullable by design: an order nobody has quoted a prep
-- time for is the normal case, not a missing value.
ALTER TABLE "order" ADD COLUMN preparation_time_minutes INTEGER;
