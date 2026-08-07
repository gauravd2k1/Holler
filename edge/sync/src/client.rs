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
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url.trim_end_matches('/'), path)
    }

    /// POSTs `body` as JSON. A transport-level failure (DNS, connect
    /// refused, timeout — i.e. "we are offline") is reported as
    /// [`SyncError::HttpTransport`]; any HTTP response, success or not, is
    /// a [`Reply`].
    pub fn post_json(&self, path: &str, body: &Value) -> Result<Reply, SyncError> {
        match self.agent.post(&self.url(path)).send_json(body.clone()) {
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
        match self.agent.get(&self.url(path)).call() {
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
