//! Native transport and SDK binding contracts, against synthetic TLS
//! endpoints on this Mac (real rustls + macOS platform verifier) and an
//! independent scripted transport. No product endpoint is contacted.

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use desktop_core::scan::ScanBinding;
use desktop_core::transport::{ProxyMode, TransportFactory, TransportPolicy};
use futures_util::future::BoxFuture;
use http::{HeaderMap, Method, StatusCode};
use scm_sdk::transport::{Transport, TransportRequest, TransportResponse};
use scm_sdk::{ErrorKind, RawRequest, RemoteOutcome};
use zeroize::Zeroizing;

use crate::support::{Proxy, Reply, TestCa, TlsServer, eventually};

const KEY: &str = "synthetic-scan-key-0001";

fn policy(ca: &TestCa, proxy: ProxyMode) -> TransportPolicy {
    TransportPolicy {
        revision: 1,
        proxy,
        extra_roots: vec![ca.pem.as_bytes().to_vec()],
    }
}

fn binding(origin: &str, transport: Arc<dyn Transport>) -> ScanBinding {
    ScanBinding::new(origin, Zeroizing::new(KEY.to_owned()), transport).expect("binding")
}

async fn send(binding: &ScanBinding, body: &[u8]) -> scm_sdk::Result<Bytes> {
    let request = RawRequest::new(Method::POST, "/v1/scan/sync/request")
        .body(Bytes::copy_from_slice(body), "application/json")
        .expect("request");
    let options = ScanBinding::request_options(Duration::from_secs(10)).expect("options");
    binding.client().send_bytes(request, Some(&options)).await
}

async fn localhost_server(ca: &TestCa, reply: Reply) -> TlsServer {
    TlsServer::start(ca.leaf(&["localhost"]), reply).await
}

fn origin(host: &str, server: &TlsServer) -> String {
    format!("https://{host}:{}", server.port)
}

#[tokio::test(flavor = "multi_thread")]
async fn the_exact_request_reaches_the_server_with_scan_headers_only() {
    let ca = TestCa::new("airs-v2 test ca");
    let server = localhost_server(&ca, Reply::ok(br#"{"ok":true}"#)).await;
    let factory = TransportFactory::new(policy(&ca, ProxyMode::Direct)).expect("factory");
    let scan = binding(
        &origin("localhost", &server),
        factory.for_admission().expect("transport"),
    );
    let body = br#"{"ai_profile":{"profile_name":"fixture"},"contents":[{"prompt":"hello"}]}"#;
    let reply = send(&scan, body).await.expect("sent");
    assert_eq!(reply.as_ref(), br#"{"ok":true}"#);
    let seen = server.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].request_line, "POST /v1/scan/sync/request HTTP/1.1");
    assert_eq!(seen[0].body, body, "the admitted body, byte for byte");
    assert_eq!(seen[0].header("x-pan-token"), Some(KEY));
    assert!(
        seen[0]
            .header("x-payload-hash")
            .is_some_and(|h| h.len() == 64)
    );
    assert_eq!(seen[0].header("accept-encoding"), Some("gzip"), "gzip only");
    assert_eq!(seen[0].header("content-type"), Some("application/json"));
}

#[tokio::test(flavor = "multi_thread")]
async fn redirects_are_returned_to_the_sdk_not_followed() {
    let ca = TestCa::new("airs-v2 test ca");
    let server = localhost_server(&ca, Reply::Raw {
        head: "HTTP/1.1 302 Found\r\nlocation: https://elsewhere.example/v1/scan/sync/request\r\ncontent-length: 0\r\nconnection: close".into(),
        body: Vec::new(),
    })
    .await;
    let factory = TransportFactory::new(policy(&ca, ProxyMode::Direct)).expect("factory");
    let scan = binding(
        &origin("localhost", &server),
        factory.for_admission().expect("transport"),
    );
    let error = send(&scan, b"{}").await.expect_err("redirect refused");
    assert_eq!(error.kind(), ErrorKind::Destination);
    assert_eq!(error.remote_outcome(), RemoteOutcome::Rejected);
    assert_eq!(
        server.seen().len(),
        1,
        "no second request to the redirect target"
    );
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(bytes).expect("compress");
    encoder.finish().expect("finish")
}

#[tokio::test(flavor = "multi_thread")]
async fn gzip_is_decoded_and_the_decoded_size_is_bounded() {
    let ca = TestCa::new("airs-v2 test ca");
    let small = gzip(br#"{"verdict":"benign"}"#);
    let server = localhost_server(&ca, Reply::Raw {
        head: format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-encoding: gzip\r\ncontent-length: {}\r\nconnection: close",
            small.len()
        ),
        body: small,
    })
    .await;
    let factory = TransportFactory::new(policy(&ca, ProxyMode::Direct)).expect("factory");
    let scan = binding(
        &origin("localhost", &server),
        factory.for_admission().expect("transport"),
    );
    assert_eq!(
        send(&scan, b"{}").await.expect("decoded").as_ref(),
        br#"{"verdict":"benign"}"#
    );

    // 24 MiB of zeros compress to a few KiB; the 16 MiB decoded ceiling holds.
    let bomb = gzip(&vec![b'0'; 24 * 1024 * 1024]);
    assert!(bomb.len() < 64 * 1024);
    let server = localhost_server(&ca, Reply::Raw {
        head: format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-encoding: gzip\r\ncontent-length: {}\r\nconnection: close",
            bomb.len()
        ),
        body: bomb,
    })
    .await;
    let scan = binding(
        &origin("localhost", &server),
        factory.for_admission().expect("transport"),
    );
    let error = send(&scan, b"{}").await.expect_err("bounded");
    assert_eq!(error.kind(), ErrorKind::Limit);
}

#[tokio::test(flavor = "multi_thread")]
async fn response_headers_are_bounded() {
    let ca = TestCa::new("airs-v2 test ca");
    let many: String = (0..300).map(|i| format!("\r\nx-filler-{i}: v")).collect();
    let server = localhost_server(
        &ca,
        Reply::Raw {
            head: format!("HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close{many}"),
            body: b"{}".to_vec(),
        },
    )
    .await;
    let factory = TransportFactory::new(policy(&ca, ProxyMode::Direct)).expect("factory");
    let scan = binding(
        &origin("localhost", &server),
        factory.for_admission().expect("transport"),
    );
    let error = send(&scan, b"{}").await.expect_err("too many headers");
    assert!(
        matches!(error.kind(), ErrorKind::Transport | ErrorKind::Limit),
        "{error:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn untrusted_wrong_host_and_root_less_policies_fail_before_any_request() {
    let trusted = TestCa::new("airs-v2 trusted ca");
    let other = TestCa::new("airs-v2 untrusted ca");
    let factory = TransportFactory::new(policy(&trusted, ProxyMode::Direct)).expect("factory");

    let untrusted = TlsServer::start(other.leaf(&["localhost"]), Reply::ok(b"{}")).await;
    let scan = binding(
        &origin("localhost", &untrusted),
        factory.for_admission().expect("transport"),
    );
    assert_eq!(
        send(&scan, b"{}").await.expect_err("untrusted").kind(),
        ErrorKind::Transport
    );

    let wrong_host = TlsServer::start(trusted.leaf(&["other.example"]), Reply::ok(b"{}")).await;
    let scan = binding(
        &origin("localhost", &wrong_host),
        factory.for_admission().expect("transport"),
    );
    assert_eq!(
        send(&scan, b"{}").await.expect_err("wrong host").kind(),
        ErrorKind::Transport
    );

    let good = localhost_server(&trusted, Reply::ok(b"{}")).await;
    let os_only = TransportFactory::new(TransportPolicy::default()).expect("factory");
    let scan = binding(
        &origin("localhost", &good),
        os_only.for_admission().expect("transport"),
    );
    assert_eq!(
        send(&scan, b"{}")
            .await
            .expect_err("test CA is not an OS root")
            .kind(),
        ErrorKind::Transport
    );
    assert!(untrusted.seen().is_empty() && wrong_host.seen().is_empty() && good.seen().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn dropping_a_request_releases_its_connection() {
    let ca = TestCa::new("airs-v2 test ca");
    let server = localhost_server(&ca, Reply::Hang).await;
    let factory = TransportFactory::new(policy(&ca, ProxyMode::Direct)).expect("factory");
    let scan = binding(
        &origin("localhost", &server),
        factory.for_admission().expect("transport"),
    );
    let pending = tokio::time::timeout(Duration::from_millis(500), send(&scan, b"{}")).await;
    assert!(pending.is_err(), "still waiting when dropped");
    assert!(
        eventually(|| server.closed_by_client() == 1).await,
        "the connection was closed"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn transport_errors_are_sanitized() {
    let ca = TestCa::new("airs-v2 test ca");
    let other = TestCa::new("airs-v2 untrusted ca");
    let server = TlsServer::start(other.leaf(&["localhost"]), Reply::ok(b"{}")).await;
    let factory = TransportFactory::new(policy(&ca, ProxyMode::Direct)).expect("factory");
    let scan = binding(
        &origin("localhost", &server),
        factory.for_admission().expect("transport"),
    );
    let error = send(&scan, b"{}").await.expect_err("untrusted");
    let shown = format!("{error} {error:?}");
    for leaked in [
        "localhost",
        &server.port.to_string(),
        KEY,
        "certificate",
        "x-pan-token",
    ] {
        assert!(!shown.contains(leaked), "{leaked} in {shown}");
    }
}

/// Proxy modes read the process environment, so they share one test.
#[tokio::test(flavor = "multi_thread")]
#[allow(unsafe_code)]
async fn proxy_modes_follow_the_policy_not_ambient_settings() {
    fn set_proxy(url: Option<&str>) {
        for name in [
            "HTTPS_PROXY",
            "https_proxy",
            "HTTP_PROXY",
            "http_proxy",
            "ALL_PROXY",
            "all_proxy",
            "NO_PROXY",
            "no_proxy",
        ] {
            // SAFETY: this is the only test in the binary that reads or writes
            // proxy variables; other tests use Direct, which ignores them.
            unsafe { std::env::remove_var(name) };
        }
        if let Some(url) = url {
            // SAFETY: as above.
            unsafe { std::env::set_var("HTTPS_PROXY", url) };
        }
    }
    let ca = TestCa::new("airs-v2 test ca");
    // Proxied requests name a fixture host the client never resolves; the
    // proxy tunnels it to the local server.
    let proxied = TlsServer::start(ca.leaf(&["fixture.test"]), Reply::ok(b"{}")).await;
    let direct = localhost_server(&ca, Reply::ok(b"{}")).await;
    let first = Proxy::start(proxied.port).await;
    let second = Proxy::start(proxied.port).await;
    let fixture_origin = format!("https://fixture.test:{}", proxied.port);

    // Explicit: always the reviewed proxy.
    set_proxy(Some(&second.url()));
    let explicit = TransportFactory::new(policy(&ca, ProxyMode::Explicit { url: first.url() }))
        .expect("explicit");
    send(
        &binding(&fixture_origin, explicit.for_admission().expect("t")),
        b"{}",
    )
    .await
    .expect("via explicit proxy");
    assert_eq!(
        first.tunnels(),
        vec![format!("fixture.test:{}", proxied.port)]
    );
    assert!(
        second.tunnels().is_empty(),
        "the environment proxy was not used"
    );

    // Direct: an ambient proxy is never adopted.
    let managed = TransportFactory::new(policy(&ca, ProxyMode::Direct)).expect("direct");
    send(
        &binding(
            &origin("localhost", &direct),
            managed.for_admission().expect("t"),
        ),
        b"{}",
    )
    .await
    .expect("direct");
    assert_eq!(direct.seen().len(), 1);
    assert!(second.tunnels().is_empty());

    // System: each admission builds its client from the current settings;
    // a change applies to the next admission, not to one already built.
    set_proxy(Some(&first.url()));
    let system = TransportFactory::new(policy(&ca, ProxyMode::System)).expect("system");
    let before_change = system.for_admission().expect("t");
    set_proxy(Some(&second.url()));
    let after_change = system.for_admission().expect("t");
    send(&binding(&fixture_origin, after_change), b"{}")
        .await
        .expect("via second");
    send(&binding(&fixture_origin, before_change), b"{}")
        .await
        .expect("via first");
    assert_eq!(second.tunnels().len(), 1);
    assert_eq!(first.tunnels().len(), 2);
    set_proxy(None);
}

/// An independent transport: records requests, returns scripted replies.
struct Scripted {
    calls: Mutex<Vec<(String, HeaderMap)>>,
    reply: Mutex<Box<dyn FnMut() -> scm_sdk::Result<TransportResponse> + Send>>,
}

impl Scripted {
    fn new(
        reply: impl FnMut() -> scm_sdk::Result<TransportResponse> + Send + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(Vec::new()),
            reply: Mutex::new(Box::new(reply)),
        })
    }

    fn calls(&self) -> usize {
        self.calls.lock().expect("calls").len()
    }
}

impl Transport for Scripted {
    fn send<'a>(
        &'a self,
        request: TransportRequest,
    ) -> BoxFuture<'a, scm_sdk::Result<TransportResponse>> {
        self.calls
            .lock()
            .expect("calls")
            .push((request.url().to_string(), request.headers().clone()));
        let reply = (self.reply.lock().expect("reply"))();
        Box::pin(async move { reply })
    }
}

#[test]
fn bindings_require_an_exact_https_origin_and_a_usable_key() {
    let transport = Scripted::new(|| {
        Ok(TransportResponse::from_bytes(
            StatusCode::OK,
            HeaderMap::new(),
            Bytes::new(),
        ))
    });
    for origin in [
        "https://scan.example.com/v1",
        "http://scan.example.com",
        "https://scan.example.com:443",
    ] {
        assert!(
            ScanBinding::new(origin, Zeroizing::new(KEY.into()), transport.clone()).is_err(),
            "{origin}"
        );
    }
    for key in ["", "line\nbreak", &"k".repeat(17 * 1024)] {
        assert!(
            ScanBinding::new(
                "https://scan.example.com",
                Zeroizing::new(key.into()),
                transport.clone()
            )
            .is_err(),
            "unusable key"
        );
    }
}

#[tokio::test]
async fn a_retired_generation_sends_nothing() {
    let transport = Scripted::new(|| {
        Ok(TransportResponse::from_bytes(
            StatusCode::OK,
            HeaderMap::new(),
            Bytes::new(),
        ))
    });
    let scan = binding("https://scan.example.com", transport.clone());
    scan.retire();
    let error = send(&scan, b"{}").await.expect_err("retired");
    assert_eq!(error.kind(), ErrorKind::Cancelled);
    assert_eq!(error.remote_outcome(), RemoteOutcome::NotSent);
    assert_eq!(transport.calls(), 0);
}

#[tokio::test]
async fn transport_failures_are_unknown_and_never_retried() {
    let transport = Scripted::new(|| Err(scm_sdk::Error::new(ErrorKind::Transport)));
    let scan = binding("https://scan.example.com", transport.clone());
    let error = send(&scan, b"{}").await.expect_err("failed");
    assert_eq!(
        error.remote_outcome(),
        RemoteOutcome::Unknown,
        "dispatch may have begun"
    );
    assert_eq!(transport.calls(), 1, "no retry");
}

#[tokio::test]
async fn a_cross_origin_redirect_never_forwards_the_credential() {
    let transport = Scripted::new(|| {
        let mut headers = HeaderMap::new();
        headers.insert(
            "location",
            "https://attacker.example/steal".parse().expect("header"),
        );
        Ok(TransportResponse::from_bytes(
            StatusCode::TEMPORARY_REDIRECT,
            headers,
            Bytes::new(),
        ))
    });
    let scan = binding("https://scan.example.com", transport.clone());
    let error = send(&scan, b"{}").await.expect_err("redirect");
    assert_eq!(error.remote_outcome(), RemoteOutcome::Rejected);
    let calls = transport.calls.lock().expect("calls").clone();
    assert_eq!(calls.len(), 1);
    assert!(
        calls[0].0.starts_with("https://scan.example.com/"),
        "only the approved origin"
    );
}
