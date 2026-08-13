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
//! What does NOT close here, stated plainly rather than implied: the only
//! interoperable, already-frozen cloud route this crate can call without a
//! backend change is `GET /sync/config`, which is gated by
//! `outlet.DeviceAuthenticate` and 404s a caller-supplied `outlet_id` that
//! does not match the presented credential's own
//! (`backend/cmd/api/syncconfig.go`). That proves "this token is a
//! currently-valid credential belonging to a device enrolled at this
//! outlet" — it does NOT prove the token belongs to the specific `device_id`
//! claimed in the WS handshake query string, because that route's response
//! never echoes a resolved device_id back to the caller. Attribution
//! (`kot_status_history.changed_by_device_id`, the audit trail) therefore
//! still comes from the connection's claimed `device_id`, exactly as before
//! this change. What this closes is the practical attack named in
//! docs/backlog-m2.md: a captured or guessed `device_id` alone, with no
//! secret, can no longer open a connection. A verifier that could also bind
//! the secret to its own device_id needs a cloud route this crate does not
//! have — noted in this track's report as a real, unresolved gap, not
//! silently narrowed.

use crate::error::{DeviceError, DeviceResult};

/// Verifies a presented `device_token` against `outlet_id`. Implementations
/// must fail closed: any ambiguity (transport failure, malformed token,
/// unexpected response) is `Err`, never a default-allow.
pub trait DeviceTokenVerifier: Send + Sync {
    fn verify(&self, token: &str, outlet_id: &str) -> DeviceResult<()>;
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
