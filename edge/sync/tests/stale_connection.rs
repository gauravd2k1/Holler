//! A dead pooled connection must not be reported as "offline".
//!
//! Observed 2026-09-02 during the M5 criterion 6 run. A shutdown drain fifteen
//! minutes after startup printed:
//!
//! ```text
//! holler-pos: shutdown drain [orders] published=0 unrouted=36 refused=0
//! holler-pos: shutdown outbox drain found no route to the cloud; 8 row(s) sent
//! holler-pos: shutdown drain [procurement] published=6 unrouted=0 refused=0
//! ```
//!
//! The orders stream "found no route to the cloud" and then, milliseconds
//! later, procurement published six rows through the SAME client in the SAME
//! process. The backend's request log recorded **no request at all** for the
//! stream that went offline — so nothing reached the server, and the cloud was
//! reachable throughout.
//!
//! `ureq` pools connections. A keep-alive socket the server closed while the
//! till sat idle fails at the transport layer on reuse, and that is
//! indistinguishable, on one attempt, from a severed uplink.
//!
//! **The cost of a false offline is not a wasted retry, it is a STOPPED
//! STREAM.** The outbox drains in order, so the first stream after any idle
//! period strands its rows while later streams succeed — and the operator is
//! told the network is down when it is not.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use holler_edge_sync::HttpClient;

/// Reads a whole HTTP request — headers, then exactly `Content-Length` body
/// bytes — before the caller answers.
///
/// **A single `read` here made this file fail roughly once in thirty, on an
/// IDLE machine, and the failure looked exactly like the product defect the
/// file exists to catch.** Diagnosed 2026-09-02: both attempts died on
/// `os error 10054` (WSAECONNRESET), instantly — no timeout was involved.
///
/// The mechanism is a Windows socket rule, not a race in the product. Closing
/// a socket that still holds UNREAD received bytes sends an RST, and an RST
/// **discards whatever is still in the send buffer** — including the 201 this
/// server had already written. So whenever the client's request arrived in
/// more than one TCP segment, one `read` left the tail unread, the drop at the
/// end of the loop iteration reset the connection, and the response the test
/// had already sent was destroyed in flight. The client then reported a
/// transport failure on a request the server had genuinely answered.
///
/// Draining the request first means the close is an orderly FIN and the
/// response survives. **Read the whole request before you answer it** applies
/// to every fake server in this repository, and two others here answer after
/// one `read`.
fn read_whole_request(stream: &mut TcpStream) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let Ok(n) = stream.read(&mut chunk) else { return };
        if n == 0 {
            return;
        }
        buf.extend_from_slice(&chunk[..n]);

        let Some(head_end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
            continue; // headers not complete yet
        };
        let head = String::from_utf8_lossy(&buf[..head_end]).to_lowercase();
        let content_length = head
            .lines()
            .find_map(|line| line.strip_prefix("content-length:"))
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(0);
        if buf.len() >= head_end + 4 + content_length {
            return; // headers and the whole body are in hand
        }
    }
}

/// A server that DROPS the first connection without answering, then serves
/// normally — the shape of a pooled socket the peer has already closed.
///
/// Returns its base URL and a counter of connections accepted, so a test can
/// prove a second attempt was actually made rather than inferring it from a
/// success.
fn server_that_kills_the_first_connection() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let connections = Arc::new(AtomicUsize::new(0));
    let counter = connections.clone();

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let n = counter.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                // The stale-socket case: close with no response at all.
                drop(stream);
                continue;
            }
            read_whole_request(&mut stream);
            let body = b"{}";
            let response = format!(
                "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(body);
            let _ = stream.flush();
        }
    });

    (format!("http://{addr}"), connections)
}

#[test]
fn a_connection_killed_without_a_response_is_retried_rather_than_called_offline() {
    let (base_url, connections) = server_that_kills_the_first_connection();
    let client = HttpClient::new(base_url).with_bearer_token("cred-test.not-a-real-secret");

    let reply = client.post_json("/procurement/goods-receipts", &serde_json::json!({"x": 1}));

    // The whole point: this must NOT be a transport error. Before the retry
    // existed it was, and the stream stopped on it.
    let reply = reply.expect("a killed first connection must not be reported as offline");
    assert!(
        matches!(reply, holler_edge_sync::client::Reply::Ok(_)),
        "the retry must reach the server and take its 201, got {reply:?}"
    );

    // Prove the second attempt happened, rather than inferring it from success:
    // one connection died, one carried the request.
    assert_eq!(
        connections.load(Ordering::SeqCst),
        2,
        "exactly two connections: the dead pooled one, then the retry"
    );
}

/// The retry must NOT hide a genuinely unreachable cloud, and must not spend
/// the bounded shutdown budget discovering that. One extra attempt, then the
/// honest answer.
#[test]
fn a_genuinely_unreachable_cloud_is_still_reported_as_transport_failure() {
    // Bind and drop, so the port is reserved but nothing answers: a connect
    // there is refused rather than left hanging.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    drop(listener);

    let client = HttpClient::new(format!("http://{addr}"));
    let started = std::time::Instant::now();
    let result = client.post_json("/procurement/goods-receipts", &serde_json::json!({"x": 1}));
    let elapsed = started.elapsed();

    assert!(
        result.is_err(),
        "a refused connection must still be reported, not retried into a false success"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(30),
        "two attempts against a refused port must fail fast, took {elapsed:?}"
    );
}

/// An HTTP status is the server ANSWERING. Asking twice does not change its
/// mind, and retrying a 4xx would double every rejected row's load on the
/// cloud.
#[test]
fn an_http_status_is_never_retried() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let connections = Arc::new(AtomicUsize::new(0));
    let counter = connections.clone();

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            counter.fetch_add(1, Ordering::SeqCst);
            // Same rule as the server above: answer only after the whole
            // request is drained, or the close resets the connection and takes
            // this 422 with it — which would read as a transport failure and
            // make this test assert the opposite of what it means to.
            read_whole_request(&mut stream);
            let body = b"{\"error\":\"nope\"}";
            let response = format!(
                "HTTP/1.1 422 Unprocessable Entity\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(body);
            let _ = stream.flush();
        }
    });

    let client = HttpClient::new(format!("http://{addr}"));
    let reply = client
        .post_json("/procurement/goods-receipts", &serde_json::json!({"x": 1}))
        .expect("a 422 is an answer, not a transport failure");

    assert!(
        matches!(
            reply,
            holler_edge_sync::client::Reply::Rejected { status: 422 }
        ),
        "a 422 must surface as Rejected, got {reply:?}"
    );
    assert_eq!(
        connections.load(Ordering::SeqCst),
        1,
        "an answered request must be attempted exactly once"
    );
}

/// Silences an unused-import warning in builds where the helper is not used.
#[allow(dead_code)]
fn _assert_stream_type(_: TcpStream) {}
