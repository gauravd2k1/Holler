-- Holler Edge SQLite — the GRN counter. Contracts 0.6.0, ADR-019.
--
-- EDGE-LOCAL. SQLite only, no PostgreSQL mirror, no AggregateType, no sync
-- direction, ever.
--
-- ---------------------------------------------------------------------------
-- WHY THIS IS ITS OWN FILE RATHER THAN A TABLE INSIDE 0027
-- ---------------------------------------------------------------------------
--
-- Because SINGLE_STORE_MIGRATIONS (edge/database/src/migrations.rs) pairs
-- migrations BY FILENAME STEM. A single-store table sitting inside a mirrored
-- file is invisible to that lint: the file has a twin, so the pair passes, and
-- the asymmetry inside it is never declared and never checked.
--
-- That would make the exemption mechanism decorative exactly where it matters
-- most — on a counter whose whole correctness argument is that it must never
-- be mirrored. So the counter gets its own file, gets declared with a reason,
-- and the lint fails if a mirror is ever added.
--
-- ---------------------------------------------------------------------------
-- WHY IT MUST NEVER BE MIRRORED
-- ---------------------------------------------------------------------------
--
-- The invoice_sequence precedent (ADR-016). Mirroring a counter makes the
-- cloud a second minter of a number the outlet issues, which §33 forbids. The
-- ISSUED NUMBER travels on the goods_receipt_note; the COUNTER that produced
-- it never leaves the outlet.
--
-- Note what this file therefore does NOT contain: no grn_gap_sequence. grn_gap
-- ships as a plain envelope outbox, not a ranged stream — it is a discrete
-- event a buyer acts on, a handful a week, not a per-sale row arriving all day
-- like stock_deduction_gap. No entry_seq, no counter, no cursor, no contiguity
-- check. See 0027 for that reasoning in full.
--
-- ---------------------------------------------------------------------------
-- 1-BASED, NOT 0-BASED
-- ---------------------------------------------------------------------------
--
-- next_value starts at 1 and the CHECK enforces it. The 0.5.8 lesson: a
-- 0-based sequence skips every outlet's first entry, permanently and silently,
-- and nothing downstream can distinguish that from an outlet that received
-- nothing.
--
-- Keyed by business_date as well as outlet, so GRN numbering resets per
-- outlet-local business day — computed by compute_business_date() from
-- outlet.timezone and outlet.day_start_time, NOT by slicing a UTC instant.
-- That shortcut is what splits one trading night across two dates on the
-- billing side (docs/m5-planning.md §1.2); it is not repeated here.

CREATE TABLE grn_sequence (
    outlet_id       TEXT NOT NULL,
    business_date   TEXT NOT NULL,
    next_value      INTEGER NOT NULL DEFAULT 1 CHECK (next_value >= 1),
    PRIMARY KEY (outlet_id, business_date)
);
