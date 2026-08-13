-- Holler Edge SQLite — per-item tax profile. Contracts 0.4.2, ADR-016 addendum.
--
-- CONFIG, cloud→edge: a menu item's tax treatment is a management decision,
-- travelling in the item's existing config bundle. Not a new aggregate.
--
-- Why this exists: 0.4.0 froze invoice_line.tax_profile_id, implying each line
-- resolves its own profile — but gave menu_item no way to say WHICH profile it
-- uses. So the M3 tax engine could only ever resolve the outlet default, and
-- every line on a bill took the same rate. An ordinary Indian restaurant sells
-- food at 5% and an aerated drink at 28% plus cess on the same ticket, so that
-- engine could not price a realistic mixed bill at all. Found by the T6
-- verification gate, not by the 0.4.0 rubric review.
--
-- NULLABLE, and null is meaningful: it means "use the outlet's default
-- profile". That keeps the change additive over existing rows, and keeps the
-- common case — a restaurant where everything is taxed alike — configuration-
-- free rather than requiring every item to name the same profile.
ALTER TABLE menu_item ADD COLUMN tax_profile_id TEXT REFERENCES tax_profile(id);

CREATE INDEX idx_menu_item_tax_profile ON menu_item(tax_profile_id)
    WHERE tax_profile_id IS NOT NULL;
