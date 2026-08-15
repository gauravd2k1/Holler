-- Holler Cloud PostgreSQL — per-row config_version on device_credential.
-- Contracts 0.4.5, ADR-017 addendum.
--
-- CLOUD-ONLY, like the table it alters. device_credential is deliberately not
-- an AggregateType and gains no sync direction here (0008's reasoning is
-- unchanged): giving it one would ship credential material to the very device
-- whose identity it establishes.
--
-- WHY. GET /sync/config filters config by `since_version`. Every other config
-- table carries its own per-row config_version -- station, printer,
-- menu_item_station, restaurant_table all declare `config_version INTEGER NOT
-- NULL` and are written with the outlet's freshly bumped value. device_credential
-- was the one exception: it had no such column, so DeviceService.ListEdgeDeviceCredentials
-- substituted the OUTLET's current config_version into every row it returned.
--
-- The consequence was correctness-preserving but coarse. An edge whose
-- watermark already equalled outlet.config_version received no credentials; an
-- edge one version behind received ALL of them, and an unrelated config change
-- elsewhere in the outlet (a renamed table, a new station) re-sent the entire
-- credential collection with its Argon2id hashes. Filtering was outlet-granular
-- where every sibling table is row-granular.
--
-- This closes it. The wire type does NOT change: EdgeDeviceCredential in
-- packages/contracts/src/types/identity.ts has declared `config_version` since
-- 0.4.3. Only the SOURCE of that value changes -- from the outlet's current
-- version to the row's own. TS and Go mirrors are untouched and the drift tests
-- are unaffected.

ALTER TABLE device_credential ADD COLUMN config_version INTEGER;

-- Backfill before NOT NULL. Every existing credential adopts its outlet's
-- CURRENT config_version, which is the value ListEdgeDeviceCredentials was
-- already reporting for it -- so no edge observes a value move backwards.
--
-- Known and accepted: an edge whose watermark is below the outlet's current
-- version re-receives every credential once on its first pull after this
-- migration. That is correct (it was going to receive them all anyway under
-- the old outlet-granular filter) and it happens exactly once. Recorded here
-- rather than discovered in a support ticket.
UPDATE device_credential dc
   SET config_version = o.config_version
  FROM outlet o
 WHERE dc.outlet_id = o.id;

-- Any credential whose outlet somehow did not match above would be left NULL
-- and the SET NOT NULL below would fail loudly. That is the intended
-- behaviour: a credential with no resolvable outlet is a data defect worth
-- stopping a migration for, not something to paper over with a default.
ALTER TABLE device_credential ALTER COLUMN config_version SET NOT NULL;

-- Supports the row-granular pull: WHERE outlet_id = $1 AND config_version > $2.
CREATE INDEX idx_device_credential_outlet_version
    ON device_credential(outlet_id, config_version);

-- WRITE-ORDER NOTE for whoever implements the backend side.
--
-- The credential row must carry the value the outlet is bumped TO, so the
-- order inside the transaction inverts: BumpOutletConfigVersion first, then
-- InsertCredential with the returned version. RevokeActiveCredential must ALSO
-- stamp the new version alongside revoked_at -- a revocation that does not
-- advance its own row's config_version would never reach the edge, which is
-- the more dangerous half: the edge would keep honouring a credential the
-- cloud has revoked.
--
-- This is only safe because the T13 retry (eef7464) put all three of
-- EnrollDevice/RotateCredential/RevokeCredential inside a single WithTx. Before
-- that commit this migration would have introduced a race rather than removed
-- one.
