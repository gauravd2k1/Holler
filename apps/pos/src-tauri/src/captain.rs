//! The captain HTTP listener — demo build item 0 (`docs/captain-api.md`).
//!
//! A second, plaintext-HTTP listener beside the KDS WebSocket LAN server
//! that already runs in this process (`state.rs`). It serves the built
//! `apps/captain/dist` page at `/` and a small JSON API under `/api/`, so a
//! waiter's phone on the outlet's own LAN can pick a table, add items and
//! send an order to the kitchen without ever talking to the cloud.
//!
//! Binding posture matches `start_lan_server` exactly, for the reason
//! recorded there: Milestone 1's acceptance is that a cashier works fully
//! offline, so a LAN feature that cannot bind its port must never take the
//! till down with it. A bind failure here logs and returns; it is never
//! propagated to `AppState::open`.
//!
//! Authentication is local-first, offline-capable, with **no cloud
//! fallback** — the same `device_credential_cache` table and the same
//! Argon2id check `edge/device`'s `CachedCredentialVerifier` uses for the
//! KDS path, applied here directly against `holler_edge_database` rather
//! than through that crate: `CachedCredentialVerifier::verify` only answers
//! "does this token belong to *some* device enrolled at this outlet", not
//! "which one" (its own doc comment), and this route needs the specific
//! `device_id` (docs/captain-api.md "The identity trap"). A credential
//! absent from the local cache, revoked, or expired is `401`; absence is
//! unknown, never allow.
//!
//! `POST /api/orders` and `POST /api/orders/{orderId}/items` attribute the
//! order to the **resolved credential's own `device_id`**, never to
//! `state.device_id` (the till) and never to anything a request body
//! supplies — see [`crate::commands::orders::create_order_impl_as`] and the
//! "Order attribution" section of the spec.

use std::io::Cursor;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use holler_edge_database::model::DeviceCredentialCache;
use serde::{Deserialize, Serialize};
use tiny_http::{Header, Method, Request, Response, Server};

use crate::commands::kitchen::send_order_to_kitchen_impl;
use crate::commands::menu::{
    list_menu_categories_impl, list_menu_item_modifiers_impl, list_menu_item_variants_impl,
    list_menu_items_impl,
};
use crate::commands::orders::{
    add_order_item_impl, confirm_order_impl, create_order_impl_as, get_order_impl,
    NewOrderItemRequest,
};
use crate::commands::tables::{get_open_table_session_impl, list_tables_impl};
use crate::error::AppError;
use crate::state::AppState;

/// Default bind address for the captain HTTP listener. Overridable with
/// `HOLLER_CAPTAIN_BIND_ADDR`.
pub const DEFAULT_CAPTAIN_BIND_ADDR: &str = "0.0.0.0:9320";

/// Resolves the directory containing the built `apps/captain/dist`.
///
/// Defaults to a path computed at compile time from this crate's own
/// manifest directory, deliberately not the process's working directory —
/// `cargo run` and `tauri dev` do not agree on that, and getting it wrong
/// must not be a silent "captain page renders nothing" failure. Overridable
/// with `HOLLER_CAPTAIN_DIST_DIR` for a packaged build that places `dist`
/// somewhere else.
fn dist_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("HOLLER_CAPTAIN_DIST_DIR") {
        return PathBuf::from(dir);
    }
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../captain/dist"))
}

/// Starts the captain HTTP listener on its own thread and returns
/// immediately. Never fatal to POS startup — see the module doc.
///
/// Returns the address actually bound (useful in tests that request an
/// ephemeral port via `:0`); `None` if the listener could not bind or its
/// thread could not be spawned. The production caller (`state.rs`) ignores
/// the return value — a bind failure there is only ever logged.
pub fn start_captain_server(addr: SocketAddr, state: Arc<AppState>) -> Option<SocketAddr> {
    let server = match Server::http(addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "holler-pos: captain HTTP listener failed to bind {addr}: {e}; the captain \
                 page will not be reachable this session (POS itself is unaffected)"
            );
            return None;
        }
    };
    let bound_addr = server.server_addr().to_ip();
    if addr.ip().is_unspecified() {
        eprintln!(
            "holler-pos: captain HTTP listener on {addr} (0.0.0.0 — reachable from anywhere on \
             this LAN; /api/ requests must present an enrolled WAITER device credential, \
             verified against the local cache)"
        );
    } else {
        eprintln!("holler-pos: captain HTTP listener on {addr}");
    }

    let dist = dist_dir();

    let spawned = std::thread::Builder::new()
        .name("holler-captain-http".to_string())
        .spawn(move || {
            for request in server.incoming_requests() {
                handle_request(request, &state, &dist);
            }
        });
    if let Err(e) = spawned {
        eprintln!(
            "holler-pos: could not start the captain HTTP thread ({e}); the captain page will \
             not be reachable this session (POS itself is unaffected)"
        );
        return None;
    }
    bound_addr
}

// ------------------------------------------------------------- routing --

fn handle_request(mut request: Request, state: &AppState, dist: &Path) {
    let method = request.method().clone();
    let path = request.url().split('?').next().unwrap_or("/").to_string();

    let outcome = if let Some(rest) = path.strip_prefix("/api/") {
        route_api(&mut request, state, &method, rest)
    } else {
        Ok(serve_static(dist, &path))
    };

    let response = match outcome {
        Ok(r) => r,
        Err((status, err)) => error_response(status, &err),
    };
    if let Err(e) = request.respond(response) {
        eprintln!("holler-pos: captain HTTP response write failed: {e}");
    }
}

type ApiResult = Result<Response<Cursor<Vec<u8>>>, (u16, AppError)>;

fn route_api(request: &mut Request, state: &AppState, method: &Method, rest: &str) -> ApiResult {
    let segments: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();

    match (method, segments.as_slice()) {
        (Method::Get, ["session"]) => {
            let cred = authenticate(request, state)?;
            handle_session(state, &cred)
        }
        (Method::Get, ["tables"]) => {
            authenticate(request, state)?;
            handle_tables(state)
        }
        (Method::Get, ["menu"]) => {
            authenticate(request, state)?;
            handle_menu(state)
        }
        (Method::Post, ["orders"]) => {
            let cred = authenticate(request, state)?;
            let body = read_body(request)?;
            handle_create_order(state, &cred, &body)
        }
        (Method::Post, ["orders", order_id, "items"]) => {
            authenticate(request, state)?;
            let body = read_body(request)?;
            handle_add_item(state, order_id, &body)
        }
        (Method::Post, ["orders", order_id, "send"]) => {
            authenticate(request, state)?;
            handle_send(state, order_id)
        }
        _ => Err((
            404,
            AppError {
                code: "NOT_FOUND",
                message: format!("no captain route for {method:?} /api/{rest}"),
            },
        )),
    }
}

// ------------------------------------------------------------ handlers --

#[derive(Debug, Serialize)]
struct SessionResponse {
    device_id: String,
    outlet_id: String,
    outlet_name: String,
    device_kind: String,
}

fn handle_session(state: &AppState, cred: &DeviceCredentialCache) -> ApiResult {
    let outlet_name = {
        let db = lock_db(state)?;
        holler_edge_database::repo::get_outlet(db.connection(), &state.outlet_id)
            .map_err(|e| storage_error(e.into()))?
            .map(|o| o.name)
            .unwrap_or_else(|| state.outlet_id.clone())
    };
    json_response(
        200,
        &SessionResponse {
            device_id: cred.device_id.clone(),
            outlet_id: state.outlet_id.clone(),
            outlet_name,
            device_kind: cred.device_kind.clone(),
        },
    )
}

#[derive(Debug, Serialize)]
struct CaptainTable {
    id: String,
    name: String,
    seats: i64,
    open_session_id: Option<String>,
    open_order_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct TablesResponse {
    tables: Vec<CaptainTable>,
}

fn handle_tables(state: &AppState) -> ApiResult {
    let tables = list_tables_impl(state).map_err(|e| (500, e))?;
    let mut out = Vec::with_capacity(tables.len());
    for t in tables {
        let session = get_open_table_session_impl(state, &t.id).map_err(|e| (500, e))?;
        out.push(CaptainTable {
            id: t.id,
            name: t.label,
            seats: t.seat_count,
            open_session_id: session.as_ref().map(|s| s.id.clone()),
            open_order_id: session.and_then(|s| s.current_order_id),
        });
    }
    json_response(200, &TablesResponse { tables: out })
}

#[derive(Debug, Serialize)]
struct CaptainVariant {
    id: String,
    name: String,
    price_delta_paise: i64,
    is_default: bool,
}

#[derive(Debug, Serialize)]
struct CaptainModifier {
    id: String,
    group_name: String,
    option_name: String,
    price_delta_paise: i64,
    min_selection: i64,
    max_selection: i64,
}

#[derive(Debug, Serialize)]
struct CaptainMenuItem {
    id: String,
    category_id: String,
    name: String,
    base_price_paise: i64,
    is_available: bool,
    variants: Vec<CaptainVariant>,
    modifiers: Vec<CaptainModifier>,
}

#[derive(Debug, Serialize)]
struct CaptainCategory {
    id: String,
    name: String,
    sort_order: i64,
}

#[derive(Debug, Serialize)]
struct MenuResponse {
    categories: Vec<CaptainCategory>,
    items: Vec<CaptainMenuItem>,
}

/// One call, not four — docs/captain-api.md: "a phone on a restaurant
/// hotspot should fetch the catalogue once." `is_available` is served as
/// stored; the page's own snoozed-item filter is the page's job, not this
/// route's (same document, "GET /api/menu").
fn handle_menu(state: &AppState) -> ApiResult {
    let categories = list_menu_categories_impl(state).map_err(|e| (500, e))?;
    let items = list_menu_items_impl(state).map_err(|e| (500, e))?;
    let variants = list_menu_item_variants_impl(state).map_err(|e| (500, e))?;
    let modifiers = list_menu_item_modifiers_impl(state).map_err(|e| (500, e))?;

    let out_items = items
        .into_iter()
        .map(|item| {
            let item_variants = variants
                .iter()
                .filter(|v| v.menu_item_id == item.id)
                .map(|v| CaptainVariant {
                    id: v.id.clone(),
                    name: v.name.clone(),
                    price_delta_paise: v.price_delta_paise,
                    is_default: v.is_default,
                })
                .collect();
            let item_modifiers = modifiers
                .iter()
                .filter(|m| m.menu_item_id == item.id)
                .map(|m| CaptainModifier {
                    id: m.id.clone(),
                    group_name: m.group_name.clone(),
                    option_name: m.option_name.clone(),
                    price_delta_paise: m.price_delta_paise,
                    min_selection: m.min_selection,
                    max_selection: m.max_selection,
                })
                .collect();
            CaptainMenuItem {
                id: item.id,
                category_id: item.category_id,
                name: item.name,
                base_price_paise: item.base_price_paise,
                is_available: item.is_available,
                variants: item_variants,
                modifiers: item_modifiers,
            }
        })
        .collect();

    let out_categories = categories
        .into_iter()
        .map(|c| CaptainCategory {
            id: c.id,
            name: c.name,
            sort_order: c.sort_order,
        })
        .collect();

    json_response(
        200,
        &MenuResponse {
            categories: out_categories,
            items: out_items,
        },
    )
}

#[derive(Debug, Deserialize)]
struct CreateOrderRequest {
    order_type: String,
    table_id: Option<String>,
    items: Vec<NewOrderItemRequest>,
}

/// A variant is mandatory on every line (docs/captain-api.md, CLAUDE.md's
/// M4 criterion-1 lesson: the till hardcoded `variantId: null` for a whole
/// milestone and no sale it took ever wrote a stock ledger row). Rejected
/// here with a clear error rather than passed through.
fn reject_missing_variant(items: &[NewOrderItemRequest]) -> Result<(), (u16, AppError)> {
    if items.iter().any(|i| i.variant_id.is_none()) {
        return Err((
            400,
            AppError {
                code: "VARIANT_REQUIRED",
                message: "every order line must carry a variant_id".to_string(),
            },
        ));
    }
    Ok(())
}

fn handle_create_order(state: &AppState, cred: &DeviceCredentialCache, body: &[u8]) -> ApiResult {
    perf_mark("captain_order_received", "-");
    let req: CreateOrderRequest = serde_json::from_slice(body).map_err(|e| {
        (
            400,
            AppError {
                code: "INVALID_INPUT",
                message: format!("could not parse request body: {e}"),
            },
        )
    })?;
    if req.items.is_empty() {
        return Err((
            400,
            AppError {
                code: "EMPTY_ORDER",
                message: "an order must have at least one item".to_string(),
            },
        ));
    }
    reject_missing_variant(&req.items)?;

    let order = create_order_impl_as(
        state,
        &cred.device_id,
        req.order_type,
        req.table_id,
        req.items,
    )
    .map_err(|e| (400, e))?;
    json_response(201, &order)
}

fn handle_add_item(state: &AppState, order_id: &str, body: &[u8]) -> ApiResult {
    let item: NewOrderItemRequest = serde_json::from_slice(body).map_err(|e| {
        (
            400,
            AppError {
                code: "INVALID_INPUT",
                message: format!("could not parse request body: {e}"),
            },
        )
    })?;
    reject_missing_variant(std::slice::from_ref(&item))?;

    let order = add_order_item_impl(state, order_id, item).map_err(|e| (400, e))?;
    json_response(201, &order)
}

#[derive(Debug, Serialize)]
struct KotSummary {
    id: String,
    station: String,
    sequence: i64,
    status: String,
}

#[derive(Debug, Serialize)]
struct SendResponse {
    order: crate::dto::CanonicalOrder,
    kots: Vec<KotSummary>,
}

/// Confirms the order and cuts the KOTs — docs/captain-api.md: "That second
/// call is what puts the ticket on the hub, and the hub is what the KDS is
/// watching." Sending an order with no lines is `400`, checked BEFORE
/// confirming so a doomed send never leaves the order half-transitioned.
/// One line, one format, four places (this file, `edge/device/src/hub.rs`,
/// `apps/kds`, `apps/pos`). Grep `HOLLER-PERF` out of the logs and the
/// intervals are subtractions.
///
/// DELIBERATELY NOT A TIMER IN THE CODE. The two intervals that matter cross
/// a process boundary and a WebSocket -- captain POST to KDS render, and till
/// tap to bill open -- so no single process can measure either one. Stamped
/// lines from each side, correlated by order id, can be read from the
/// operator's own demo runs without anything of ours attaching to the live
/// ports.
pub fn perf_mark(event: &str, id: &str) {
    println!(
        "HOLLER-PERF ts={} event={event} id={id}",
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    );
}

fn handle_send(state: &AppState, order_id: &str) -> ApiResult {
    perf_mark("captain_send_received", order_id);
    let existing = get_order_impl(state, order_id).map_err(|e| (500, e))?;
    let Some(existing) = existing else {
        return Err((
            404,
            AppError {
                code: "ORDER_NOT_FOUND",
                message: format!("order {order_id} not found"),
            },
        ));
    };
    if existing.items.is_empty() {
        return Err((
            400,
            AppError {
                code: "EMPTY_ORDER",
                message: "cannot send an order with no lines to the kitchen".to_string(),
            },
        ));
    }

    let confirmed = confirm_order_impl(state, order_id).map_err(|e| (400, e))?;
    let kots = send_order_to_kitchen_impl(state, order_id).map_err(|e| (400, e))?;
    let order = get_order_impl(state, order_id)
        .map_err(|e| (500, e))?
        .unwrap_or(confirmed);

    let kot_summaries = kots
        .into_iter()
        .map(|k| KotSummary {
            id: k.id,
            station: k.station,
            sequence: k.sequence,
            status: k.status,
        })
        .collect();

    json_response(
        200,
        &SendResponse {
            order,
            kots: kot_summaries,
        },
    )
}

// ----------------------------------------------------------------- auth --

/// Splits a device token `"<credential_id>.<secret>"` into its two halves.
/// Malformed input (no `.`, or an empty half) is `None`, treated as
/// `UNAUTHORIZED` — never as "unknown, try somewhere else" (there is no
/// cloud fallback here at all; see the module doc).
fn split_token(token: &str) -> Option<(&str, &str)> {
    let (credential_id, secret) = token.split_once('.')?;
    if credential_id.is_empty() || secret.is_empty() {
        return None;
    }
    Some((credential_id, secret))
}

fn unauthorized(message: impl Into<String>) -> (u16, AppError) {
    (
        401,
        AppError {
            code: "UNAUTHORIZED",
            message: message.into(),
        },
    )
}

/// Resolves and verifies the presented `device_token` against
/// `device_credential_cache`, returning the specific credential row.
///
/// Deliberately reimplements (rather than calls into) `edge/device`'s
/// `CachedCredentialVerifier`: that type's own `verify()` returns
/// `Result<()>` — it answers "does this token belong to *some* device
/// enrolled at this outlet", not "which one" (its own doc comment) — and
/// this route needs the specific `device_id` to attribute the order to,
/// never trusting anything the request itself supplies
/// (docs/captain-api.md "The identity trap"). The checks mirror that type's
/// `check_cached_row` exactly: outlet match, `device_kind == "WAITER"`, not
/// revoked, not expired, Argon2id secret match. No cloud fallback — a
/// credential absent from the local cache is `401`, not "unknown, ask the
/// cloud": requiring a cloud URL to authenticate would make this route
/// depend on the uplink, which ADR-013 forbids for every LAN-facing surface
/// in this process.
fn authenticate(request: &Request, state: &AppState) -> Result<DeviceCredentialCache, (u16, AppError)> {
    let token = bearer_token(request)
        .ok_or_else(|| unauthorized("missing or malformed Authorization header"))?;
    let Some((credential_id, secret)) = split_token(&token) else {
        return Err(unauthorized("malformed device_token"));
    };

    let row = {
        let db = lock_db(state)?;
        holler_edge_database::repo::get_device_credential_cache_by_id(db.connection(), credential_id)
            .map_err(|e| storage_error(e.into()))?
    };
    let Some(row) = row else {
        return Err(unauthorized("device credential not cached locally"));
    };

    if row.outlet_id != state.outlet_id {
        return Err(unauthorized(
            "device credential does not belong to this outlet",
        ));
    }
    if row.device_kind != "WAITER" {
        return Err(unauthorized(format!(
            "device credential is for kind {}, not WAITER",
            row.device_kind
        )));
    }
    if row.revoked_at.is_some() {
        return Err(unauthorized("device credential has been revoked"));
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
            return Err(unauthorized("device credential has expired"));
        }
    }
    holler_edge_database::auth::verify_password(secret, &row.credential_hash)
        .map_err(|_| unauthorized("device credential secret did not verify"))?;

    Ok(row)
}

fn bearer_token(request: &Request) -> Option<String> {
    for h in request.headers() {
        if h.field.equiv("Authorization") {
            let value = h.value.as_str();
            return value.strip_prefix("Bearer ").map(|s| s.to_string());
        }
    }
    None
}

// -------------------------------------------------------------- helpers --

fn lock_db(state: &AppState) -> Result<std::sync::MutexGuard<'_, holler_edge_database::Db>, (u16, AppError)> {
    state.db.lock().map_err(|_| {
        (
            500,
            AppError {
                code: "LOCK_POISONED",
                message: "database lock poisoned".to_string(),
            },
        )
    })
}

fn storage_error(e: AppError) -> (u16, AppError) {
    (500, e)
}

fn read_body(request: &mut Request) -> Result<Vec<u8>, (u16, AppError)> {
    let mut buf = Vec::new();
    request.as_reader().read_to_end(&mut buf).map_err(|e| {
        (
            400,
            AppError {
                code: "INVALID_INPUT",
                message: format!("could not read request body: {e}"),
            },
        )
    })?;
    Ok(buf)
}

fn json_response(status: u16, body: &impl Serialize) -> ApiResult {
    let bytes = serde_json::to_vec(body).map_err(|e| {
        (
            500,
            AppError {
                code: "SERIALIZATION_ERROR",
                message: e.to_string(),
            },
        )
    })?;
    Ok(Response::from_data(bytes)
        .with_status_code(status)
        .with_header(json_content_type()))
}

fn error_response(status: u16, err: &AppError) -> Response<Cursor<Vec<u8>>> {
    let bytes = serde_json::to_vec(err).unwrap_or_else(|_| b"{\"code\":\"SERIALIZATION_ERROR\",\"message\":\"could not encode error\"}".to_vec());
    Response::from_data(bytes)
        .with_status_code(status)
        .with_header(json_content_type())
}

fn json_content_type() -> Header {
    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
        .expect("static header name/value is always valid ASCII")
}

// ------------------------------------------------------------- static --

/// Serves the built captain page from `dist`. Unauthenticated by design —
/// the pair screen has no token yet when it first loads
/// (docs/captain-api.md "Authentication").
fn serve_static(dist: &Path, path: &str) -> Response<Cursor<Vec<u8>>> {
    let rel = if path == "/" { "index.html" } else { path.trim_start_matches('/') };

    // Refuse path traversal outright rather than relying on the filesystem
    // to reject it — a captain page is served over plaintext HTTP on a flat
    // LAN and this is the one surface that touches the filesystem by path.
    if rel.split('/').any(|seg| seg == "..") {
        return Response::from_string("bad request").with_status_code(400);
    }

    let candidate = dist.join(rel);
    let file_path = if candidate.is_file() {
        candidate
    } else {
        // SPA fallback: any unmatched path serves index.html so client-side
        // routing (if the page ever grows any) still resolves.
        dist.join("index.html")
    };

    match std::fs::read(&file_path) {
        Ok(bytes) => {
            let content_type = mime_for(&file_path);
            Response::from_data(bytes)
                .with_status_code(200)
                .with_header(
                    Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes())
                        .expect("static header name/value is always valid ASCII"),
                )
        }
        Err(_) => Response::from_string(format!(
            "captain page not built or not found at {}; run `pnpm build` in apps/captain",
            dist.display()
        ))
        .with_status_code(404),
    }
}

fn mime_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("webmanifest") => "application/manifest+json",
        _ => "application/octet-stream",
    }
}
