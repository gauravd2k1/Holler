//! Device token verification for the KDS LAN handshake (ADR-017 hole 3,
//! docs/backlog-m2.md "Device enrollment — HARD TRIGGER").
//!
//! Before this module, `server.rs` trusted the `device_id` query parameter
//! alone: any string parsing as non-empty opened the socket. `device_id` is
//! a UUID, not a secret (ADR-017 §caption on `lan.ts`'s transport note), so
//! that was equivalent to no authentication at all on a flat restaurant LAN.
//!
//! What closes here: a KDS screen must additionally prove it holds a
//! currently-valid, cloud-issued device credential (the same
//! `POST /devices/enroll` mechanism `edge/sync` presents on the cloud sync
//! path) before the server ever sends a snapshot or accepts a command.
//!
//! ## ADR-017 amendment (0.4.3): verification is local-first, not cloud-first
//!
//! The first implementation ([`CloudConfigOracleVerifier`]) called the cloud
//! for every new connection. The verification gate ruled that a blocker: a
//! browser reload, a waking tablet, or a router blip during a WAN outage left
//! the kitchen screen unable to re-authenticate and receiving no tickets —
//! worse than unauthenticated-but-available, exactly when local-first is
//! meant to protect. Contracts 0.4.3 closes the gap the same way ADR-011
//! closes it for human logins: the device credential's Argon2id **verifier**
//! now syncs to `device_credential_cache` on `GET /sync/config`
//! (`holler_edge_sync::config::apply_bundle`), and [`CachedCredentialVerifier`]
//! checks a presented token against that local cache with the uplink down.
//!
//! The cloud path ([`CloudConfigOracleVerifier`]) is kept only as a fallback
//! for a credential that has never synced to this node — never as the
//! primary path, and never consulted once a `credential_id` IS present
//! locally (a locally-cached row's `revoked_at`/`expires_at` are the only
//! things that can reject it; the cloud is not re-asked "is this still
//! good").
//!
//! What does NOT close here, stated plainly rather than implied: the ingest
//! routes (order/table_session/kot push) sit behind human-JWT
//! `auth.Authenticate`, which a device credential cannot satisfy. ADR-017's
//! 0.4.3 amendment records `DeviceAuthenticate` gating ingest as the fix, but
//! that is backend work, a separate track, and is NOT assumed done by
//! anything in this crate or its tests.

use std::sync::{Arc, Mutex};

use holler_edge_database::model::DeviceCredentialCache;
use holler_edge_database::Db;

use crate::error::{DeviceError, DeviceResult};

/// Verifies a presented `device_token` against `outlet_id`. Implementations
/// must fail closed: any ambiguity (transport failure, malformed token,
/// unexpected response) is `Err`, never a default-allow.
pub trait DeviceTokenVerifier: Send + Sync {
    fn verify(&self, token: &str, outlet_id: &str) -> DeviceResult<()>;
}

/// Splits a device token `"<credential_id>.<secret>"` into its two parts.
/// Malformed input (no `.`, or an empty half) is `None` — callers must treat
/// that as `Unauthorized`, never as "unknown, try the cloud".
fn split_token(token: &str) -> Option<(&str, &str)> {
    let (credential_id, secret) = token.split_once('.')?;
    if credential_id.is_empty() || secret.is_empty() {
        return None;
    }
    Some((credential_id, secret))
}

/// Primary, offline-capable verifier (ADR-017 amendment 0.4.3): checks a
/// presented `device_token` against `device_credential_cache`, the table
/// `holler_edge_sync::config::apply_bundle` populates from `GET
/// /sync/config` — exactly the ADR-011 pattern already used for cashier
/// logins, applied to devices.
///
/// A `credential_id` absent from the local cache is NOT itself a rejection —
/// while offline that is indistinguishable from "not yet synced" — it falls
/// through to `cloud_fallback` if one is configured, or is `Unauthorized` if
/// not. A `credential_id` that IS present is decided entirely locally: by
/// `revoked_at`/`expires_at`/`device_kind` and the Argon2id check, never by
/// calling the cloud again.
pub struct CachedCredentialVerifier {
    db: Arc<Mutex<Db>>,
    /// The `device_kind` this server accepts (`"KDS"` for the KDS LAN
    /// server) — refuses a correctly-signed credential minted for a
    /// different kind of device, e.g. a `PRINTER_BRIDGE` credential
    /// presented by something claiming to be a KDS screen.
    expected_device_kind: &'static str,
    /// Consulted only when `credential_id` is not (yet) cached locally.
    /// `None` means "no fallback": an uncached credential is rejected rather
    /// than silently trusted or silently retried forever.
    cloud_fallback: Option<Arc<dyn DeviceTokenVerifier>>,
}

impl CachedCredentialVerifier {
    pub fn new(
        db: Arc<Mutex<Db>>,
        expected_device_kind: &'static str,
        cloud_fallback: Option<Arc<dyn DeviceTokenVerifier>>,
    ) -> Self {
        Self {
            db,
            expected_device_kind,
            cloud_fallback,
        }
    }

    /// The local-only half of verification: `Ok(None)` means "not cached,
    /// caller must decide whether to fall back"; `Ok(Some(()))` is not a
    /// meaningful state so this returns `DeviceResult<Option<DeviceCredentialCache>>`
    /// and the caller runs the actual field checks — kept separate from
    /// [`Self::verify`] so a unit test can assert on "not found" precisely.
    fn lookup(&self, credential_id: &str) -> DeviceResult<Option<DeviceCredentialCache>> {
        let guard = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let found = holler_edge_database::repo::get_device_credential_cache_by_id(
            guard.connection(),
            credential_id,
        )
        .map_err(DeviceError::Db)?;
        Ok(found)
    }

    fn check_cached_row(
        &self,
        row: &DeviceCredentialCache,
        secret: &str,
        outlet_id: &str,
    ) -> DeviceResult<()> {
        if row.outlet_id != outlet_id {
            return Err(DeviceError::Unauthorized(
                "device credential does not belong to this outlet".to_string(),
            ));
        }
        if row.device_kind != self.expected_device_kind {
            return Err(DeviceError::Unauthorized(format!(
                "device credential is for kind {}, not {}",
                row.device_kind, self.expected_device_kind
            )));
        }
        // Rejection is decided by these two fields, NEVER by the row's
        // absence (ADR-017 amendment) — a row that exists but is revoked or
        // expired is exactly the case this whole mechanism exists to let the
        // edge learn about while offline.
        if row.revoked_at.is_some() {
            return Err(DeviceError::Unauthorized(
                "device credential has been revoked".to_string(),
            ));
        }
        if let Some(expires_at) = &row.expires_at {
            let expired = match (
                chrono::DateTime::parse_from_rfc3339(expires_at),
                chrono::Utc::now(),
            ) {
                (Ok(exp), now) => exp < now,
                // A malformed expires_at must not accidentally verify as
                // "never expires" — fail closed.
                (Err(_), _) => true,
            };
            if expired {
                return Err(DeviceError::Unauthorized(
                    "device credential has expired".to_string(),
                ));
            }
        }
        holler_edge_database::auth::verify_password(secret, &row.credential_hash).map_err(|_| {
            DeviceError::Unauthorized("device credential secret did not verify".to_string())
        })
    }
}

impl DeviceTokenVerifier for CachedCredentialVerifier {
    fn verify(&self, token: &str, outlet_id: &str) -> DeviceResult<()> {
        let Some((credential_id, secret)) = split_token(token) else {
            return Err(DeviceError::Unauthorized(
                "malformed device_token".to_string(),
            ));
        };

        match self.lookup(credential_id)? {
            Some(row) => self.check_cached_row(&row, secret, outlet_id),
            None => match &self.cloud_fallback {
                Some(fallback) => fallback.verify(token, outlet_id),
                None => Err(DeviceError::Unauthorized(
                    "device credential not cached locally and no fallback configured".to_string(),
                )),
            },
        }
    }
}

/// Production verifier: calls the cloud's `GET /sync/config` as a
/// verification oracle. `since_version` is set high enough that the
/// response's filtered arrays come back empty (see
/// `holler_edge_sync::worker::VERIFY_SINCE_VERSION` for the identical
/// reasoning on the `edge/sync` side of the same mechanism) — this call
/// exists to observe the status code, not to read the body.
///
/// Cost, stated honestly: this requires the edge node to reach the cloud for
/// every NEW KDS connection. An already-open connection is unaffected by a
/// later WAN outage (verification happens once, at connect time, not per
/// frame), but a KDS screen that reconnects while the outlet is offline
/// cannot be freshly verified. There is no contracted route today that lets
/// the edge cache other devices' credential hashes locally the way
/// `EdgeUserCacheEntry` lets it cache human ones (ADR-011) —
/// `device_credential` is deliberately cloud-only, no SQLite mirror
/// (ADR-017 §1) — so a local, offline-tolerant verification path would need
/// a new contracted route. Flagged in this track's report rather than
/// silently worked around.
pub struct CloudConfigOracleVerifier {
    agent: ureq::Agent,
    base_url: String,
}

impl CloudConfigOracleVerifier {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .timeout_connect(std::time::Duration::from_secs(5))
                .timeout_read(std::time::Duration::from_secs(10))
                .build(),
            base_url: base_url.into(),
        }
    }

    /// Same reasoning as `holler_edge_sync::worker::VERIFY_SINCE_VERSION`:
    /// large enough to make the response body cheap, small enough to stay
    /// within a 32-bit `int` on the Go side (`strconv.Atoi`).
    const VERIFY_SINCE_VERSION: i64 = i32::MAX as i64;
}

impl DeviceTokenVerifier for CloudConfigOracleVerifier {
    fn verify(&self, token: &str, outlet_id: &str) -> DeviceResult<()> {
        if token.is_empty() {
            return Err(DeviceError::Unauthorized("empty device_token".to_string()));
        }
        let url = format!(
            "{}/sync/config?outlet_id={outlet_id}&since_version={}",
            self.base_url.trim_end_matches('/'),
            Self::VERIFY_SINCE_VERSION,
        );
        match self
            .agent
            .get(&url)
            .set("Authorization", &format!("Bearer {token}"))
            .call()
        {
            Ok(_resp) => Ok(()),
            Err(ureq::Error::Status(status, _resp)) => Err(DeviceError::Unauthorized(format!(
                "cloud rejected device_token: status {status}"
            ))),
            Err(ureq::Error::Transport(e)) => Err(DeviceError::Unauthorized(format!(
                "could not reach cloud to verify device_token: {e}"
            ))),
        }
    }
}
