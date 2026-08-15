//! Blocking WebSocket server: accepts KDS screens on the outlet LAN, sends a
//! snapshot on connect, then streams `KdsLanMessage`s from the [`Hub`] and
//! handles `KdsLanCommand::SetKotStatus` as intent (ADR-014 §6).
//!
//! Threaded, not async: a KDS fleet at one outlet is a handful of screens,
//! and staying off an async runtime keeps the dependency graph small on the
//! 4GB/spinning-disk hardware this ships to (ADR-013). Each connection gets
//! two threads — a reader (blocking `WebSocket::read`, handles commands) and
//! a writer (drains its `Hub` subscription, pushes frames) — built over two
//! independent `WebSocket` framings of the same duplex `TcpStream` via
//! `try_clone`. A slow write on one connection only blocks that
//! connection's own writer thread.

use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use tungstenite::handshake::server::{Callback, ErrorResponse, Request, Response};
use tungstenite::protocol::{Role, WebSocket};
use tungstenite::Message;

use holler_edge_database::model::KotTransitionMeta;
use holler_edge_database::Db;

use crate::auth::DeviceTokenVerifier;
use crate::contract::{KdsLanCommand, KdsLanMessage, Kot as WireKot, KotStatus};
use crate::error::DeviceError;
use crate::hub::Hub;

/// How long a connection may sit between the WS handshake completing and a
/// valid `auth` frame arriving (ADR-017 hole 3, `lan.ts`'s "first-frame auth
/// message" option — chosen over an `Authorization` header because a
/// browser `WebSocket` cannot set custom headers on the handshake at all;
/// `apps/kds` is a browser app). No snapshot, no command handling, nothing
/// else happens on a connection before this either succeeds or the
/// connection is closed.
const AUTH_FRAME_TIMEOUT: Duration = Duration::from_secs(5);

/// KDS -> edge, first frame only. NOT part of `packages/contracts/src/types/
/// lan.ts`'s `KdsLanCommandSchema` — that schema is KOT status intent, and
/// this crate must not overload it with connection auth. `lan.ts`'s own
/// transport note names "a first-frame auth message" as one of the two
/// sanctioned ways to move `device_token` out of the query string but does
/// not define its shape, since contracts is read-only to builder tracks;
/// this hand-mirrored shape is the smallest thing that satisfies it and is
/// flagged in this track's report as a candidate for promotion into `lan.ts`
/// by the orchestrator, the same way `device_token` itself was reserved
/// ahead of use.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AuthFrame {
    Auth { device_token: String },
}

/// Default interval between heartbeats. `docs/spec/kitchen.md` gives a
/// <250ms *propagation* target for a state change, not a heartbeat cadence —
/// this interval only drives "is the screen still there", so it is set far
/// looser than the propagation target on purpose.
pub const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

/// Poll granularity for a connection's reader thread when no data is
/// waiting — bounds how quickly a shutdown request or a dead-peer detection
/// is noticed, without busy-looping.
const READ_POLL_INTERVAL: Duration = Duration::from_millis(200);

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Parsed connection identity from the WS handshake request's query string:
/// `ws://<host>:<port>/kds?outlet_id=<uuid>&device_id=<uuid>[&station=<code>]`.
///
/// `device_id` here is the connection's identity for the lifetime of the
/// socket — every command arriving on this connection is attributed to it,
/// never to whatever `device_id` a `KdsLanCommand` payload claims (ADR-014
/// §6 / lan.ts).
#[derive(Debug, Clone)]
struct ConnRequest {
    outlet_id: String,
    device_id: String,
    station: Option<String>,
}

fn parse_query(uri_query: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for pair in uri_query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let mut parts = pair.splitn(2, '=');
        let key = parts.next().unwrap_or_default();
        let value = parts.next().unwrap_or_default();
        map.insert(key.to_string(), value.to_string());
    }
    map
}

#[derive(Clone)]
struct HandshakeCallback {
    parsed: Arc<Mutex<Option<Result<ConnRequest, String>>>>,
}

impl Callback for HandshakeCallback {
    fn on_request(self, request: &Request, response: Response) -> Result<Response, ErrorResponse> {
        let query = request.uri().query().unwrap_or("");
        let params = parse_query(query);
        let result = match (params.get("outlet_id"), params.get("device_id")) {
            (Some(outlet_id), Some(device_id))
                if !outlet_id.is_empty() && !device_id.is_empty() =>
            {
                Ok(ConnRequest {
                    outlet_id: outlet_id.clone(),
                    device_id: device_id.clone(),
                    station: params.get("station").filter(|s| !s.is_empty()).cloned(),
                })
            }
            _ => Err("outlet_id and device_id query parameters are required".to_string()),
        };
        let mut guard = self.parsed.lock().unwrap_or_else(|e| e.into_inner());
        let is_err = result.is_err();
        *guard = Some(result);
        drop(guard);
        if is_err {
            let resp = tungstenite::http::Response::builder()
                .status(400)
                .body(Some(
                    "outlet_id and device_id query parameters are required".to_string(),
                ))
                .expect("static 400 response is well-formed");
            return Err(resp);
        }
        Ok(response)
    }
}

/// Handle to a running LAN server (see [`start`]). Dropping this does not
/// stop the server — call [`LanServerHandle::shutdown`] explicitly.
pub struct LanServerHandle {
    pub hub: Arc<Hub>,
    local_addr: SocketAddr,
    shutdown_flag: Arc<AtomicBool>,
    accept_thread: Option<thread::JoinHandle<()>>,
    heartbeat_thread: Option<thread::JoinHandle<()>>,
}

impl LanServerHandle {
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Stops accepting new connections and the heartbeat loop. Existing
    /// connections are not force-closed; each notices the flag and winds
    /// down on its own next poll.
    pub fn shutdown(mut self) {
        self.shutdown_flag.store(true, Ordering::SeqCst);
        // Unblock the accept() call with a local dummy connection.
        if let Ok(stream) = TcpStream::connect(self.local_addr) {
            drop(stream);
        }
        if let Some(t) = self.accept_thread.take() {
            let _ = t.join();
        }
        if let Some(t) = self.heartbeat_thread.take() {
            let _ = t.join();
        }
    }
}

/// Starts the LAN WebSocket server bound to `addr` (use `127.0.0.1:0` or
/// `0.0.0.0:0` in tests to get an ephemeral port).
///
/// `verifier` is mandatory (ADR-017 hole 3): every connection must
/// authenticate with a first-frame `auth` message before it receives a
/// snapshot or has a command accepted. There is no "no verifier" overload —
/// a caller that wants the pre-ADR-017 unauthenticated posture would have to
/// construct one that always returns `Ok(())`, which is exactly the kind of
/// thing that must be visible in a diff, not the silent default.
pub fn start(
    addr: SocketAddr,
    db: Arc<Mutex<Db>>,
    heartbeat_interval: Duration,
    verifier: Arc<dyn DeviceTokenVerifier>,
) -> std::io::Result<LanServerHandle> {
    let listener = TcpListener::bind(addr)?;
    let local_addr = listener.local_addr()?;
    let hub = Arc::new(Hub::new());
    let shutdown_flag = Arc::new(AtomicBool::new(false));

    let accept_hub = hub.clone();
    let accept_db = db.clone();
    let accept_shutdown = shutdown_flag.clone();
    let accept_verifier = verifier.clone();
    let accept_thread = thread::spawn(move || {
        for incoming in listener.incoming() {
            if accept_shutdown.load(Ordering::SeqCst) {
                break;
            }
            let Ok(stream) = incoming else {
                continue;
            };
            let hub = accept_hub.clone();
            let db = accept_db.clone();
            let verifier = accept_verifier.clone();
            thread::spawn(move || {
                if let Err(err) = handle_connection(stream, hub, db, verifier) {
                    log::debug!("kds lan: connection ended: {err}");
                }
            });
        }
    });

    let heartbeat_hub = hub.clone();
    let heartbeat_shutdown = shutdown_flag.clone();
    let heartbeat_thread = thread::spawn(move || loop {
        if heartbeat_shutdown.load(Ordering::SeqCst) {
            break;
        }
        thread::sleep(heartbeat_interval);
        if heartbeat_shutdown.load(Ordering::SeqCst) {
            break;
        }
        heartbeat_hub.heartbeat_all(&now_rfc3339());
    });

    Ok(LanServerHandle {
        hub,
        local_addr,
        shutdown_flag,
        accept_thread: Some(accept_thread),
        heartbeat_thread: Some(heartbeat_thread),
    })
}

fn handle_connection(
    stream: TcpStream,
    hub: Arc<Hub>,
    db: Arc<Mutex<Db>>,
    verifier: Arc<dyn DeviceTokenVerifier>,
) -> Result<(), crate::error::DeviceError> {
    stream.set_nodelay(true).ok();
    let parsed = Arc::new(Mutex::new(None));
    let callback = HandshakeCallback {
        parsed: parsed.clone(),
    };
    let mut socket = match tungstenite::accept_hdr(stream, callback) {
        Ok(s) => s,
        Err(_) => return Ok(()), // rejected (missing/invalid query params) or IO error mid-handshake
    };
    let conn_request = {
        let guard = parsed.lock().unwrap_or_else(|e| e.into_inner());
        match guard.clone() {
            Some(Ok(req)) => req,
            _ => {
                let _ = socket.close(None);
                return Ok(());
            }
        }
    };

    // ADR-017 hole 3: outlet_id/device_id (checked above) identify a
    // connection, they do not authenticate it. Nothing past this point —
    // no snapshot, no subscription, no command handling — happens until the
    // first frame is a verified `auth` message. A rejection here closes the
    // socket and returns Ok(()) (a normal connection-ended outcome, logged
    // at debug by `start`'s caller), never leaking whether the outlet_id or
    // device_id was itself valid.
    if let Err(err) = authenticate_first_frame(&mut socket, &conn_request, verifier.as_ref()) {
        log::warn!(
            "kds lan: rejected connection outlet={} device={}: {err}",
            conn_request.outlet_id,
            conn_request.device_id
        );
        let _ = socket.close(None);
        return Ok(());
    }

    let raw = socket.get_ref().try_clone()?;
    raw.set_read_timeout(Some(READ_POLL_INTERVAL))?;
    let mut writer_socket = WebSocket::from_raw_socket(raw, Role::Server, None);

    let subscription = hub.subscribe(&conn_request.outlet_id, conn_request.station.clone());
    log::info!(
        "kds lan: connect outlet={} device={} station={:?} conn={}",
        conn_request.outlet_id,
        conn_request.device_id,
        conn_request.station,
        subscription.conn_id
    );

    // Snapshot on connect (and this whole function re-runs on reconnect, so
    // "on reconnect" falls out of "on connect" — a screen unplugged for an
    // hour gets a fresh snapshot, never a replayed backlog).
    let snapshot = build_snapshot(
        &db,
        &conn_request.outlet_id,
        conn_request.station.as_deref(),
    )?;
    if writer_socket
        .send(Message::Text(serde_json::to_string(&snapshot)?.into()))
        .is_err()
    {
        hub.unsubscribe(&conn_request.outlet_id, subscription.conn_id);
        return Ok(());
    }

    let outlet_id = conn_request.outlet_id.clone();
    let device_id = conn_request.device_id.clone();
    let writer_outlet_id = outlet_id.clone();
    let writer_conn_id = subscription.conn_id;
    let writer_hub = hub.clone();

    let writer_thread = thread::spawn(move || {
        loop {
            match subscription.receiver.recv_timeout(Duration::from_secs(1)) {
                Ok(message) => {
                    let text = match serde_json::to_string(&message) {
                        Ok(t) => t,
                        Err(_) => continue,
                    };
                    if writer_socket.send(Message::Text(text.into())).is_err() {
                        break;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        writer_hub.unsubscribe(&writer_outlet_id, writer_conn_id);
    });

    // Reader loop: blocks (up to READ_POLL_INTERVAL) on incoming frames and
    // handles `KdsLanCommand::SetKotStatus`. Runs on this thread so the
    // function returns (and the connection is torn down) when the peer goes
    // away, which also unblocks the writer thread via channel disconnect
    // once `hub.unsubscribe` above / drop below runs.
    loop {
        match socket.read() {
            Ok(Message::Text(text)) => {
                if let Err(err) = handle_command(&text, &outlet_id, &device_id, &db, &hub) {
                    log::warn!(
                        "kds lan: rejected command from conn={}: {}",
                        writer_conn_id,
                        err
                    );
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => continue, // ping/pong/binary: tungstenite auto-answers pings
            Err(tungstenite::Error::Io(ref e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(_) => break,
        }
    }

    // Unsubscribing twice (here, and again when the writer thread's channel
    // disconnects) is intentional and safe — `Hub::unsubscribe` is a no-op
    // once the conn_id is already gone.
    hub.unsubscribe(&outlet_id, writer_conn_id);
    let _ = writer_thread.join();
    log::info!(
        "kds lan: disconnect outlet={} device={} conn={}",
        outlet_id,
        device_id,
        writer_conn_id
    );
    Ok(())
}

/// Blocks (bounded by [`AUTH_FRAME_TIMEOUT`]) for the connection's first
/// frame, requires it to be a valid `auth` message, and verifies its
/// `device_token` against `conn_request.outlet_id`. Returns `Ok(())` only
/// when verification actually succeeded; every other outcome (timeout,
/// non-auth first frame, malformed JSON, a token the verifier rejects, the
/// peer closing early, a transport error) is `Err` — this function fails
/// closed by construction, not by convention: there is no code path that
/// returns `Ok(())` without a successful `verifier.verify(..)` call.
fn authenticate_first_frame(
    socket: &mut WebSocket<TcpStream>,
    conn_request: &ConnRequest,
    verifier: &dyn DeviceTokenVerifier,
) -> Result<(), DeviceError> {
    // A short, dedicated read timeout for the auth phase only — distinct
    // from READ_POLL_INTERVAL, which governs the steady-state command loop
    // once a connection is trusted.
    socket
        .get_ref()
        .set_read_timeout(Some(Duration::from_millis(200)))
        .ok();
    let deadline = Instant::now() + AUTH_FRAME_TIMEOUT;

    loop {
        if Instant::now() >= deadline {
            return Err(DeviceError::Unauthorized(
                "no auth frame within timeout".to_string(),
            ));
        }
        match socket.read() {
            Ok(Message::Text(text)) => {
                let frame: AuthFrame = serde_json::from_str(&text).map_err(|e| {
                    DeviceError::Unauthorized(format!(
                        "first frame was not a valid auth message: {e}"
                    ))
                })?;
                let AuthFrame::Auth { device_token } = frame;
                return verifier.verify(&device_token, &conn_request.outlet_id);
            }
            Ok(Message::Close(_)) => {
                return Err(DeviceError::Unauthorized(
                    "connection closed before auth".to_string(),
                ));
            }
            // Ping/pong/binary before auth: not the frame we need yet, keep
            // waiting up to the deadline. tungstenite auto-answers pings.
            Ok(_) => continue,
            Err(tungstenite::Error::Io(ref e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(e) => {
                return Err(DeviceError::Unauthorized(format!(
                    "transport error before auth: {e}"
                )))
            }
        }
    }
}

fn build_snapshot(
    db: &Arc<Mutex<Db>>,
    outlet_id: &str,
    station: Option<&str>,
) -> Result<KdsLanMessage, crate::error::DeviceError> {
    let db = db.lock().unwrap_or_else(|e| e.into_inner());
    let kots = db.list_kots_for_outlet(outlet_id, station)?;
    drop(db);
    let wire_kots: Result<Vec<WireKot>, _> = kots
        .iter()
        .filter(|k| !is_terminal_status(&k.status))
        .map(WireKot::from_db)
        .collect();
    Ok(KdsLanMessage::Snapshot {
        outlet_id: outlet_id.to_string(),
        sent_at: now_rfc3339(),
        kots: wire_kots?,
    })
}

fn is_terminal_status(status: &str) -> bool {
    KotStatus::from_db_str(status)
        .map(KotStatus::is_terminal)
        .unwrap_or(false)
}

fn handle_command(
    text: &str,
    outlet_id: &str,
    device_id: &str,
    db: &Arc<Mutex<Db>>,
    hub: &Arc<Hub>,
) -> Result<(), crate::error::DeviceError> {
    let command: KdsLanCommand = serde_json::from_str(text)
        .map_err(|e| crate::error::DeviceError::InvalidRequest(e.to_string()))?;
    match command {
        KdsLanCommand::SetKotStatus {
            kot_id,
            status,
            device_id: claimed_device_id,
            requested_at: _,
        } => {
            // Deliberately ignored: authorization/attribution uses the
            // connection's own device_id, never the payload's (ADR-014 §6).
            let _ = claimed_device_id;

            let meta = KotTransitionMeta {
                status_history_id: uuid::Uuid::now_v7().to_string(),
                outbox_id: uuid::Uuid::now_v7().to_string(),
                changed_by_device_id: device_id.to_string(),
                occurred_at: now_rfc3339(),
            };

            let mut guard = db.lock().unwrap_or_else(|e| e.into_inner());
            guard.transition_kot_status_with_outbox(&kot_id, status.as_db_str(), &meta)?;
            // Re-read the row: transition_kot_status_with_outbox does not
            // return the updated Kot, and this crate never guesses field
            // values the database itself owns.
            let kots = guard.list_kots_for_outlet(outlet_id, None)?;
            drop(guard);
            let updated = kots.into_iter().find(|k| k.id == kot_id);
            let sent_at = now_rfc3339();
            if let Some(kot) = updated {
                if status.is_terminal() {
                    hub.notify_kot_removed(outlet_id, &kot_id, &sent_at);
                } else {
                    let wire = WireKot::from_db(&kot)?;
                    hub.notify_kot_upserted(outlet_id, &wire, &sent_at);
                }
            }
            Ok(())
        }
    }
}
