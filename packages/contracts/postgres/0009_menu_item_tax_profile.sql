-- Holler Cloud PostgreSQL — per-item tax profile. Mirrors
-- sqlite/0007_menu_item_tax_profile.sql. Contracts 0.4.2, ADR-016 addendum.
--
-- CONFIG, cloud→edge: a menu item's tax treatment is a management decision,
-- travelling in the item's existing config bundle. Not a new aggregate.
--
-- NULLABLE, and null is meaningful: "use the outlet's default profile". Keeps
-- the change additive over existing rows, and keeps the common single-rate
-- restaurant configuration-free rather than making every item name the same
-- profile.
--
-- RESOLUTION HAPPENS AT BILLING TIME; THE LINE STORES WHAT WAS APPLIED.
-- This column is an input to that resolution, never a substitute for the
-- snapshot. invoice_line already carries tax_profile_id plus per-component
-- *_rate_bps and *_paise, and those are the historical record: re-pointing an
-- item at a different profile tomorrow must never alter what a bill issued
-- today says it charged. §31 requires historical bills stay reproducible, and
-- an invoice that recomputed itself from current config would not be.
ALTER TABLE menu_item ADD COLUMN tax_profile_id UUID REFERENCES tax_profile(id);

CREATE INDEX idx_menu_item_tax_profile ON menu_item(tax_profile_id)
    WHERE tax_profile_id IS NOT NULL;
