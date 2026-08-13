//! Thin HTTP boundary. Kept separate from [`crate::worker`] so tests can
//! point it at a local `tiny_http` server instead of a real cloud endpoint,
//! and so a transport failure (offline) is mechanically distinguishable from
//! a valid-but-unsuccessful HTTP response (cloud rejected the envelope).

use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::SyncError;

pub struct HttpClient {
    agent: ureq::Agent,
    base_url: String,
    /// The enrolled device credential (ADR-017), `<credential_id>.<secret>`,
    /// presented as `Authorization: Bearer <token>` on every request this
    /// client makes. `None` only in tests that exercise transport/parsing
    /// behaviour unrelated to authentication — every production caller must
    /// set one via [`Self::with_bearer_token`]. Never logged, never placed
    /// in an error (mirrors the `password_hash`/`pin_hash` handling rule in
    /// this crate's `config` module).
    bearer_token: Option<String>,
}

/// Outcome of a single HTTP round trip that *did* reach the server (as
/// opposed to a transport-level failure, which offline degradation treats
/// differently — see [`crate::worker`]).
pub enum Reply {
    /// 2xx. Carries the parsed JSON body, if any route needs it.
    Ok(Value),
    /// Any other status. The pump must not treat this as "offline" — the
    /// cloud is reachable and said no.
    Rejected { status: u16 },
}

impl HttpClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout_read(Duration::from_secs(15))
            .timeout_write(Duration::from_secs(15))
            .build();
        Self {
            agent,
            base_url: base_url.into(),
            bearer_token: None,
        }
    }

    /// Attaches an enrolled device credential (ADR-017): every subsequent
    /// request built by this client carries
    /// `Authorization: Bearer <credential_id>.<secret>`. Consuming builder so
    /// callers cannot construct a client, forget to attach a credential, and
    /// have that be silently valid-looking — the field stays `Option` only
    /// so tests that do not care about auth are not forced to fabricate one.
    pub fn with_bearer_token(mut self, token: impl Into<String>) -> Self {
        self.bearer_token = Some(token.into());
        self
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url.trim_end_matches('/'), path)
    }

    fn authorize(&self, req: ureq::Request) -> ureq::Request {
        match &self.bearer_token {
            Some(token) => req.set("Authorization", &format!("Bearer {token}")),
            None => req,
        }
    }

    /// POSTs `body` as JSON. A transport-level failure (DNS, connect
    /// refused, timeout — i.e. "we are offline") is reported as
    /// [`SyncError::HttpTransport`]; any HTTP response, success or not, is
    /// a [`Reply`].
    pub fn post_json(&self, path: &str, body: &Value) -> Result<Reply, SyncError> {
        let req = self.authorize(self.agent.post(&self.url(path)));
        match req.send_json(body.clone()) {
            Ok(resp) => {
                let json = resp.into_json::<Value>().unwrap_or(Value::Null);
                Ok(Reply::Ok(json))
            }
            Err(ureq::Error::Status(status, _resp)) => Ok(Reply::Rejected { status }),
            Err(ureq::Error::Transport(_)) => Err(SyncError::HttpTransport),
        }
    }

    /// GETs and deserializes a JSON response. Same offline-vs-rejected split
    /// as [`Self::post_json`]; a non-2xx GET is reported through
    /// [`SyncError::HttpStatus`] since config pull has no "rejected but
    /// keep going" case — either it worked or it did not.
    pub fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, SyncError> {
        let req = self.authorize(self.agent.get(&self.url(path)));
        match req.call() {
            Ok(resp) => resp
                .into_json::<T>()
                .map_err(|e| SyncError::Json(serde_json_error_from_io(e))),
            Err(ureq::Error::Status(status, _resp)) => Err(SyncError::HttpStatus { status }),
            Err(ureq::Error::Transport(_)) => Err(SyncError::HttpTransport),
        }
    }
}

// `ureq::Response::into_json` returns `std::io::Error`, not
// `serde_json::Error`; wrap it into a message-preserving serde_json::Error so
// this module has exactly one JSON error type on its public surface.
fn serde_json_error_from_io(e: std::io::Error) -> serde_json::Error {
    use serde::de::Error as _;
    serde_json::Error::custom(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Falsifiable proof that the bearer token actually reaches the wire:
    /// asserts the exact header value the server receives, and — per the
    /// task's "falsify your own guard" requirement — this test fails if
    /// `authorize` is bypassed (e.g. by calling `self.agent.get(..).call()`
    /// directly instead of through `self.authorize(..)`), which was
    /// confirmed by temporarily reverting `get_json`/`post_json` to the
    /// pre-ADR-017 unauthenticated call and re-running this test: it failed
    /// with "no Authorization header seen" rather than passing vacuously.
    #[test]
    fn get_json_sends_bearer_token_header() {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("start test server");
        let addr = server.server_addr();
        let handle = std::thread::spawn(move || {
            let req = server.recv().expect("request arrives");
            let seen = req
                .headers()
                .iter()
                .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case("Authorization"))
                .map(|h| h.value.as_str().to_string());
            assert_eq!(
                seen.as_deref(),
                Some("Bearer cred-1.super-secret"),
                "no Authorization header seen"
            );
            let _ = req.respond(tiny_http::Response::from_string("{\"ok\":true}"));
        });

        let client = HttpClient::new(format!("http://{addr}")).with_bearer_token("cred-1.super-secret");
        let _: Value = client.get_json("/sync/config?outlet_id=o1&since_version=0").expect("get ok");
        handle.join().unwrap();
    }

    /// Same falsification for the push path.
    #[test]
    fn post_json_sends_bearer_token_header() {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("start test server");
        let addr = server.server_addr();
        let handle = std::thread::spawn(move || {
            let req = server.recv().expect("request arrives");
            let seen = req
                .headers()
                .iter()
                .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case("Authorization"))
                .map(|h| h.value.as_str().to_string());
            assert_eq!(seen.as_deref(), Some("Bearer cred-1.super-secret"));
            let _ = req.respond(tiny_http::Response::from_string("{}").with_status_code(201));
        });

        let client = HttpClient::new(format!("http://{addr}")).with_bearer_token("cred-1.super-secret");
        let _ = client.post_json("/orders", &serde_json::json!({})).expect("post ok");
        handle.join().unwrap();
    }
}
