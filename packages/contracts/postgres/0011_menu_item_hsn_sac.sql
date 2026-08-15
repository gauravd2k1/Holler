-- Holler Cloud PostgreSQL — menu_item gains hsn_sac.
-- Contracts 0.4.5, ADR-016 addendum. Mirror of sqlite/0011_menu_item_hsn_sac.sql,
-- whose header carries the full reasoning.
--
-- CONFIG, cloud->edge. The cloud is the authority: hsn_sac is set through menu
-- management and reaches the edge on GET /sync/config like every other
-- menu_item column, bumping outlet.config_version on write.
--
-- Summary of the decision recorded in the SQLite mirror:
--   * invoice_line.hsn_sac has been NULL on every line ever issued because no
--     table carried a code to read. A GST tax invoice requires one per line.
--   * It belongs on menu_item, not tax_profile: HSN/SAC classifies what is
--     sold, a tax profile classifies how it is rated. Prepared food (SAC 9963)
--     and packaged water (HSN 2201) can share one 5% profile.
--   * Nullable on purpose. A NOT NULL with an invented default would stamp a
--     plausible, wrong, legally-meaningful code on packaged goods; a wrong HSN
--     is worse than a missing one because it looks configured. The
--     completeness rule is enforced at invoice-issue time, not by this column.

ALTER TABLE menu_item ADD COLUMN hsn_sac TEXT;
