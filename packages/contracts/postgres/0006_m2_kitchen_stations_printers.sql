-- Holler Cloud PostgreSQL — Milestone 2 kitchen: stations, item routing and
-- printers. Mirrors sqlite/0005_m2_kitchen_stations_printers.sql.
-- Contracts 0.3.0, ADR-014.
--
-- No DEFAULT gen_random_uuid() on any id below. The 0001 tables carry one for
-- historical reasons; §74 and the contract review rubric require ids to be
-- app-generated UUIDv7, and a DB-side random default silently produces a
-- UUIDv4 whenever a writer forgets to supply one — exactly the case the rule
-- exists to catch. Following 0002's precedent rather than 0001's.
--
-- Authority (§50.1): every table here is CLOUD_TO_EDGE config. The cloud owns
-- them and bumps outlet.config_version; the edge replaces them wholesale.

CREATE TABLE station (
    id              UUID PRIMARY KEY,
    outlet_id       UUID NOT NULL REFERENCES outlet(id),
    -- Stable machine code. kot.station stores this string rather than
    -- station_id, so a ticket survives a station rename.
    code            TEXT NOT NULL,
    name            TEXT NOT NULL,
    sort_order      INTEGER NOT NULL DEFAULT 0,
    is_active       BOOLEAN NOT NULL DEFAULT true,
    config_version  INTEGER NOT NULL
);

-- Tenant-scoped, never global: two outlets both having a TANDOOR is normal.
CREATE UNIQUE INDEX idx_station_outlet_code ON station(outlet_id, code);
CREATE INDEX idx_station_outlet_id ON station(outlet_id);

-- A join table, not a station_id column on menu_item: an item may route to
-- more than one station (docs/spec/kitchen.md §Stations).
CREATE TABLE menu_item_station (
    menu_item_id    UUID NOT NULL REFERENCES menu_item(id),
    station_id      UUID NOT NULL REFERENCES station(id),
    config_version  INTEGER NOT NULL,
    PRIMARY KEY (menu_item_id, station_id)
);

CREATE INDEX idx_menu_item_station_station_id ON menu_item_station(station_id);

CREATE TABLE printer (
    id              UUID PRIMARY KEY,
    outlet_id       UUID NOT NULL REFERENCES outlet(id),
    name            TEXT NOT NULL,
    -- Transport, not vendor. Label printers are excluded from Milestone 2 (§81).
    connection_kind TEXT NOT NULL CHECK (connection_kind IN
                      ('ESCPOS_NETWORK','ESCPOS_USB','ESCPOS_BLUETOOTH')),
    address         TEXT NOT NULL,
    paper_width_mm  INTEGER NOT NULL CHECK (paper_width_mm IN (58,80)),
    is_active       BOOLEAN NOT NULL DEFAULT true,
    config_version  INTEGER NOT NULL
);

CREATE UNIQUE INDEX idx_printer_outlet_name ON printer(outlet_id, name);
CREATE INDEX idx_printer_outlet_id ON printer(outlet_id);

CREATE TABLE station_printer (
    station_id      UUID NOT NULL REFERENCES station(id),
    printer_id      UUID NOT NULL REFERENCES printer(id),
    config_version  INTEGER NOT NULL,
    PRIMARY KEY (station_id, printer_id)
);

CREATE INDEX idx_station_printer_printer_id ON station_printer(printer_id);

-- DELIBERATELY ABSENT: print_job.
--
-- The spool is edge-local and exists only in sqlite/. Mirroring it here would
-- create a cloud table with no writer and no authority — the refresh_token
-- case in reverse (cloud-only, excluded from AggregateType for the same
-- reason). Print failures are surfaced to staff at the outlet, which is the
-- only place anyone can act on them.
--
-- ALSO ABSENT: kot_status_history. Its transitions reach the cloud as
-- KOTStatusChanged outbox events replayed onto the existing kot row, not as a
-- mirrored table. One authority for kot state, one path to it.

-- Milestone 2 lands preparation_time_minutes, per the 0.2.4 deferral note in
-- 0005_order_canonical_fields.sql. Replayed from the edge like every other
-- order column; the cloud never computes it.
ALTER TABLE "order" ADD COLUMN preparation_time_minutes INTEGER;
