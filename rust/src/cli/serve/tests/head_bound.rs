//! Upstream 0.48.0 #2684: whole-head bound (16 KiB cap + 10 s TOTAL deadline).

use super::*;
use std::time::Instant;

/// Connected (server, client) TCP pair on loopback.
pub(super) async fn connected_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let client = TcpStream::connect(addr).await.unwrap();
    let (server, _) = listener.accept().await.unwrap();
    (server, client)
}

pub(super) fn head_test_config(budget: Duration, token: Option<&str>) -> ServeConfig {
    ServeConfig {
        host: "127.0.0.1".to_string(),
        port: 8080,
        token_digest: token.map(|t| sha256_digest(t.as_bytes())),
        metrics_enabled: false,
        head_read_budget: budget,
        identity: Some(DashboardIdentity::Redacted),
        dashboard: None,
        request_timeout: None,
        operations: data::DataOperations::default(),
    }
}

/// Generous budget for tests that must not trip the deadline.
pub(super) fn fast_budget() -> Duration {
    Duration::from_millis(2_000)
}

/// Complete request head whose `\r\n\r\n` terminator's final byte is
/// exactly byte 16,384 — the upstream-valid boundary.
fn head_at_exact_cap() -> Vec<u8> {
    let mut head = String::from("GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Pad: ");
    let pad = HEAD_CAP - head.len() - 4;
    head.push_str(&"a".repeat(pad));
    head.push_str("\r\n\r\n");
    assert_eq!(head.len(), HEAD_CAP);
    head.into_bytes()
}

/// Send `request`, read until the server closes, return the raw response.
/// Strict outer timeouts turn a hang into a test failure, not a stalled CI.
async fn request_roundtrip(request: &[u8], budget: Duration, token: Option<&str>) -> String {
    let (server, mut client) = connected_pair().await;
    let config = head_test_config(budget, token);
    let server_task = tokio::spawn(async move { handle_client(server, &config).await });
    client.write_all(request).await.unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), client.read_to_end(&mut response))
        .await
        .expect("client read timed out")
        .unwrap();
    // Dropping the client lets the server-side drain finish immediately.
    drop(client);
    server_task.await.unwrap().unwrap();
    String::from_utf8_lossy(&response).into_owned()
}

#[test]
fn invalid_request_response_is_pinned() {
    let response = invalid_request_response();
    assert!(response.starts_with("HTTP/1.1 400 Bad Request\r\n"));
    assert!(response.contains("Cache-Control: no-store\r\n"));
    assert!(response.contains("Connection: close\r\n"));
    assert!(response.ends_with(r#"{"error":"invalid request"}"#));
}

#[test]
fn find_header_end_offsets() {
    assert_eq!(find_header_end(b"\r\n\r\n"), Some(4));
    assert_eq!(find_header_end(b"a\r\n\r\n"), Some(5));
    assert_eq!(find_header_end(b"aa\r\n\r\n"), Some(6));
    assert_eq!(find_header_end(b"a\r\n\r"), None);
    assert_eq!(find_header_end(b"a\r\n\rXX"), None);
    // Terminator straddling a chunk boundary.
    assert_eq!(find_header_end(b"abc\r\n\r"), None);
    assert_eq!(find_header_end(b"abc\r\n\r\ndef"), Some(7));
}

#[tokio::test]
async fn head_reader_accepts_terminator_ending_exactly_at_cap() {
    // Upstream boundary: a terminator whose final byte is byte 16,384 is valid.
    let (mut server, mut client) = connected_pair().await;
    client.write_all(&head_at_exact_cap()).await.unwrap();
    let head = read_request_head(&mut server, fast_budget()).await.unwrap();
    assert_eq!(head.len(), HEAD_CAP);
}

#[tokio::test]
async fn head_ending_exactly_at_cap_parses_and_routes_normally() {
    let response = request_roundtrip(&head_at_exact_cap(), fast_budget(), None).await;
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "exact-cap head must route to /health, got: {response}"
    );
}

#[tokio::test]
async fn head_reader_rejects_at_cap_without_terminator() {
    let (mut server, mut client) = connected_pair().await;
    client.write_all(&[b'x'; HEAD_CAP]).await.unwrap();
    let result = read_request_head(&mut server, fast_budget()).await;
    assert_eq!(result, Err(HeadReadError::Oversize));
}

#[tokio::test]
async fn head_reader_maps_incomplete_eof() {
    let (mut server, mut client) = connected_pair().await;
    client
        .write_all(b"GET /health HTTP/1.1\r\nHost: 127.")
        .await
        .unwrap();
    client.shutdown().await.unwrap();
    let result = read_request_head(&mut server, fast_budget()).await;
    assert_eq!(result, Err(HeadReadError::UnexpectedEof));
}

#[tokio::test]
async fn head_reader_maps_total_deadline_on_silent_client() {
    let (mut server, _client) = connected_pair().await;
    let result = read_request_head(&mut server, Duration::from_millis(150)).await;
    assert_eq!(result, Err(HeadReadError::Deadline));
}

#[tokio::test]
async fn oversized_head_rejected_before_auth_or_routing() {
    // A complete-looking authenticated request line drowned past the cap with
    // no terminator: must be rejected before any bearer evaluation.
    let mut junk = String::from(
        "GET /usage HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer s3cret\r\nX-Pad: ",
    );
    junk.push_str(&"a".repeat(HEAD_CAP));
    assert!(junk.len() > HEAD_CAP);
    let response = request_roundtrip(junk.as_bytes(), fast_budget(), Some("s3cret")).await;
    assert!(response.starts_with("HTTP/1.1 400"), "got: {response}");
    // Proof the bearer gate / routing never ran: not 401, not the usage payload.
    assert!(!response.starts_with("HTTP/1.1 401"));
    assert!(response.contains("Cache-Control: no-store\r\n"));
    assert!(response.contains("Connection: close\r\n"));
    assert!(response.contains(r#""error":"invalid request""#));
}

#[tokio::test]
async fn incomplete_head_eof_gets_pinned_400() {
    let (server, mut client) = connected_pair().await;
    let config = head_test_config(fast_budget(), None);
    let server_task = tokio::spawn(async move { handle_client(server, &config).await });
    client
        .write_all(b"GET /health HTTP/1.1\r\nHost: 127.")
        .await
        .unwrap();
    client.shutdown().await.unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    drop(client);
    server_task.await.unwrap().unwrap();
    let response = String::from_utf8_lossy(&response);
    assert!(response.starts_with("HTTP/1.1 400"), "got: {response}");
    assert!(response.contains("Cache-Control: no-store\r\n"));
    assert!(response.contains(r#""error":"invalid request""#));
}

#[tokio::test]
async fn silent_client_is_closed_at_total_deadline() {
    let budget = Duration::from_millis(250);
    let (server, mut client) = connected_pair().await;
    let config = head_test_config(budget, None);
    let server_task = tokio::spawn(async move { handle_client(server, &config).await });
    let started = Instant::now();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    let elapsed = started.elapsed();
    drop(client);
    server_task.await.unwrap().unwrap();
    assert!(
        elapsed >= budget,
        "deadline fired early: {elapsed:?} < {budget:?}"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "silent client outlived the total deadline: {elapsed:?}"
    );
    let response = String::from_utf8_lossy(&response);
    assert!(response.starts_with("HTTP/1.1 400"), "got: {response}");
}

#[tokio::test]
async fn trickling_bytes_do_not_reset_total_head_deadline() {
    // One byte every 60 ms: under a per-read timeout this client would hold its
    // connection for the full 3 s loop; the 400 ms TOTAL budget must kill it.
    // (Red→green mirrored from upstream CLIServeRequestDeadlineLinuxTests.)
    let budget = Duration::from_millis(400);
    let (server, mut client) = connected_pair().await;
    let config = head_test_config(budget, None);
    let server_task = tokio::spawn(async move { handle_client(server, &config).await });

    let started = Instant::now();
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(60)).await;
        if client.write_all(b"a").await.is_err() {
            break;
        }
        // Stop trickling the moment the server answers or closes.
        // peek() does NOT consume bytes — the full response stays readable.
        let mut peek = [0_u8; 1];
        if tokio::time::timeout(Duration::from_millis(10), client.peek(&mut peek))
            .await
            .is_ok()
        {
            break;
        }
    }
    let mut response = Vec::new();
    // Best-effort drain to unblock the server; the deadline assertions below are the real check.
    let _drained = client.read_to_end(&mut response).await;
    let elapsed = started.elapsed();
    drop(client);
    server_task.await.unwrap().unwrap();

    assert!(
        elapsed >= budget,
        "deadline fired early: {elapsed:?} < {budget:?}"
    );
    // Upper ceiling 2.5 s: under a per-read-reset design this client would
    // hold the connection for the whole 50-byte loop (~3.5 s incl. peeks),
    // so this still fails red — while tolerating full-suite scheduling lag.
    assert!(
        elapsed < Duration::from_millis(2_500),
        "trickling bytes extended the overall deadline: {elapsed:?}"
    );
    let response = String::from_utf8_lossy(&response);
    assert!(
        response.starts_with("HTTP/1.1 400"),
        "trickling client must get the pinned 400, got: {response}"
    );
    assert!(response.contains("Cache-Control: no-store\r\n"));
    assert!(response.contains(r#""error":"invalid request""#));
}

#[tokio::test]
async fn authenticated_request_succeeds_and_bad_tokens_stay_401() {
    // Deterministic 200: /cost with a provider the local scanner reports as
    // unsupported — full auth pass, zero network/disk access.
    let ok = request_roundtrip(
            b"GET /cost?provider=gemini HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer s3cret\r\n\r\n",
            fast_budget(),
            Some("s3cret"),
        )
        .await;
    assert!(ok.starts_with("HTTP/1.1 200"), "got: {ok}");
    assert!(ok.contains("\"supported\":false"));

    let wrong = request_roundtrip(
            b"GET /cost?provider=gemini HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer nope\r\n\r\n",
            fast_budget(),
            Some("s3cret"),
        )
        .await;
    assert!(wrong.starts_with("HTTP/1.1 401"), "got: {wrong}");

    let missing = request_roundtrip(
        b"GET /cost?provider=gemini HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
        fast_budget(),
        Some("s3cret"),
    )
    .await;
    assert!(missing.starts_with("HTTP/1.1 401"), "got: {missing}");
}

#[tokio::test]
async fn host_gate_unchanged_on_hardened_path() {
    let forbidden = request_roundtrip(
        b"GET /health HTTP/1.1\r\nHost: example.com\r\n\r\n",
        fast_budget(),
        None,
    )
    .await;
    assert!(forbidden.starts_with("HTTP/1.1 403"), "got: {forbidden}");
    assert!(forbidden.contains(r#""error":"forbidden host""#));

    let ok = request_roundtrip(
        b"GET /health HTTP/1.1\r\nHost: localhost:9999\r\n\r\n",
        fast_budget(),
        None,
    )
    .await;
    assert!(ok.starts_with("HTTP/1.1 200"), "got: {ok}");
}

#[tokio::test]
async fn over_cap_connection_closes_immediately_without_response() {
    // Upstream 0.48.0 parity: maximumConnections = 16; slot 17 is closed at
    // once, no response bytes, and a freed slot is usable again.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let config = Arc::new(head_test_config(Duration::from_secs(60), None));
    let server_task = tokio::spawn(serve_listener(listener, config, MAX_CONNECTIONS));

    // Fill every permit with trickling clients that never complete a head.
    let mut tricklers = Vec::new();
    for _ in 0..MAX_CONNECTIONS {
        let mut client = TcpStream::connect(addr).await.unwrap();
        tricklers.push(tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(100)).await;
                if client.write_all(b"a").await.is_err() {
                    break;
                }
            }
        }));
    }

    // Probe until the gate is provably full: an over-cap connection gets an
    // immediate EOF with zero response bytes.
    let mut rejected_seen = false;
    for _ in 0..40 {
        let mut probe = TcpStream::connect(addr).await.unwrap();
        let mut buf = [0_u8; 16];
        match tokio::time::timeout(Duration::from_millis(300), probe.read(&mut buf)).await {
            Ok(Ok(0)) => {
                rejected_seen = true;
                break;
            }
            // Probe landed in a still-filling slot; free it and retry.
            _ => drop(probe),
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        rejected_seen,
        "over-cap connection never got the immediate close"
    );

    // Ending the tricklers releases their permits via EOF; a normal client
    // must then be served (strict outer timeout). Permit release races the
    // server's graceful close-drain window, so a single fixed wait can see a
    // connection reset; retry within a bounded budget instead.
    for task in &tricklers {
        task.abort();
    }
    let request = b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
    let mut served: Option<String> = None;
    let retry = tokio::time::Instant::now();
    while retry.elapsed() < Duration::from_secs(2) {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let Ok(mut good) = TcpStream::connect(addr).await else {
            continue;
        };
        if good.write_all(request).await.is_err() {
            continue;
        }
        let mut response = Vec::new();
        match tokio::time::timeout(Duration::from_secs(5), good.read_to_end(&mut response)).await {
            // A reset mid-handshake is the drain race; retry.
            Ok(Err(_)) | Err(_) => continue,
            Ok(Ok(_)) => {}
        }
        if String::from_utf8_lossy(&response).starts_with("HTTP/1.1 200") {
            served = Some(String::from_utf8_lossy(&response).into_owned());
            break;
        }
    }
    let served = served.expect("no freed slot served a normal request within retry budget");
    assert!(
        served.starts_with("HTTP/1.1 200"),
        "freed slot must serve a normal request, got: {served}"
    );
    server_task.abort();
}

#[tokio::test]
async fn deadline_driven_release_frees_gated_slot() {
    // Regression (review follow-up): a semaphore permit MUST be owned for the
    // whole handle_client future and released when the SERVER's total head
    // deadline completes its 400/close path — not by client EOF/manual drop.
    // The holder client stays connected the entire test.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let holder_budget = Duration::from_millis(500);
    let config = Arc::new(head_test_config(holder_budget, None));
    let server_task = tokio::spawn(serve_listener(listener, config, 1));

    // Fill the single permit with a holder that never sends a single byte.
    let mut holder = TcpStream::connect(addr).await.unwrap();

    // Synchronize until that permit is provably held: an over-cap probe gets
    // an immediate close with zero response bytes.
    let mut rejected = false;
    for _ in 0..40 {
        let mut probe = TcpStream::connect(addr).await.unwrap();
        let mut buf = [0_u8; 16];
        match tokio::time::timeout(Duration::from_millis(300), probe.read(&mut buf)).await {
            Ok(Ok(0)) => {
                rejected = true;
                break;
            }
            // Probe landed while the holder was still being accepted; retry.
            _ => drop(probe),
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(rejected, "over-cap probe was never closed immediately");

    // Causality phase: the holder is NOT dropped/aborted/shut down. The
    // server's injected total head deadline expires on its own, completing
    // the pinned 400/close path. read_to_end returns at the server's FIN;
    // the holder socket itself stays OPEN.
    let mut holder_response = Vec::new();
    tokio::time::timeout(
        Duration::from_secs(3),
        holder.read_to_end(&mut holder_response),
    )
    .await
    .expect("server never drove its deadline/close on the held slot")
    .unwrap();
    let holder_response = String::from_utf8_lossy(&holder_response);
    assert!(
        holder_response.starts_with("HTTP/1.1 400"),
        "deadline path must answer the holder with the pinned 400, got: {holder_response}"
    );
    assert!(holder_response.contains("Cache-Control: no-store\r\n"));
    assert!(holder_response.contains(r#""error":"invalid request""#));

    // The permit frees only when the server task finishes — after the
    // deadline AND the bounded (~1 s) graceful-drain that runs while the
    // still-connected holder stays silent. Retry a normal client until the
    // freed slot serves it; early retries may still be over-cap closed.
    let started = Instant::now();
    let health = loop {
        let attempt = async {
            let mut good = TcpStream::connect(addr).await.ok()?;
            good.write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
                .await
                .ok()?;
            let mut response = Vec::new();
            tokio::time::timeout(Duration::from_millis(800), good.read_to_end(&mut response))
                .await
                .ok()?
                .ok()?;
            Some(String::from_utf8_lossy(&response).into_owned())
        };
        if let Some(text) = attempt.await
            && text.starts_with("HTTP/1.1 200")
        {
            break text;
        }
        assert!(
            started.elapsed() < Duration::from_secs(8),
            "permit was never released after the server deadline + graceful drain"
        );
        tokio::time::sleep(Duration::from_millis(120)).await;
    };
    assert!(health.contains("\"status\":\"ok\""), "got: {health}");
    // Holder is still connected throughout everything above; cleanup only
    // after all success assertions.
    server_task.abort();
    drop(holder);
}
