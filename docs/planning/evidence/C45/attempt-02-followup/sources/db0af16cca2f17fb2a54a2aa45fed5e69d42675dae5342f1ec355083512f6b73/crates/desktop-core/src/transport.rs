//! App-owned SDK transport under the native transport policy (DESIGN
//! "Native transport policy").
//!
//! [`TransportPolicy`] is versioned Rust state: Direct (the managed default,
//! ignoring ambient proxies), System (macOS proxy settings and the proxy
//! environment, read by a fresh client built per admission) or an explicit
//! proxy; trust is the OS platform verifier plus optional reviewed PEM roots.
//! [`NativeTransport`] implements `scm_sdk::transport::Transport`: it sends the
//! exact admitted request, disables redirects and retries, accepts only gzip
//! (the SDK applies its byte limits to the decoded stream), bounds response
//! headers and chunk sizes, maps every failure to a sanitized SDK error and
//! releases the connection when the future or body is dropped.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::Bytes;
use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use scm_sdk::transport::{Transport, TransportRequest, TransportResponse};
use scm_sdk::{Error, ErrorKind};

/// Most response header bytes accepted (names plus values).
pub const MAX_RESPONSE_HEADER_BYTES: usize = 64 * 1024;
/// Most response headers accepted.
pub const MAX_RESPONSE_HEADERS: usize = 128;
/// Largest single decoded body chunk passed to the SDK.
pub const MAX_CHUNK_BYTES: usize = 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// How requests reach the network.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProxyMode {
    /// No proxy, regardless of environment or macOS settings.
    Direct,
    /// macOS HTTP/HTTPS proxy settings and HTTP(S)_PROXY/ALL_PROXY/NO_PROXY,
    /// read when each operation's client is built.
    System,
    /// An operator-reviewed proxy URL.
    Explicit { url: String },
}

/// Versioned transport policy. Any change produces a new revision; clients,
/// reviews and contexts bound to the old revision are retired by the core.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransportPolicy {
    pub revision: u64,
    pub proxy: ProxyMode,
    /// Additional reviewed PEM roots, on top of the OS trust store.
    pub extra_roots: Vec<Vec<u8>>,
}

impl Default for TransportPolicy {
    fn default() -> Self {
        Self {
            revision: 1,
            proxy: ProxyMode::Direct,
            extra_roots: Vec::new(),
        }
    }
}

/// Why a client could not be built (sanitized).
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PolicyError {
    #[error("the proxy setting is invalid")]
    Proxy,
    #[error("an additional root certificate is invalid")]
    Root,
    #[error("the HTTP client could not be created")]
    Client,
}

fn build_client(policy: &TransportPolicy) -> Result<reqwest::Client, PolicyError> {
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(CONNECT_TIMEOUT)
        .https_only(true)
        .http1_max_headers(MAX_RESPONSE_HEADERS)
        .http2_max_header_list_size(MAX_RESPONSE_HEADER_BYTES as u32)
        .gzip(true)
        .referer(false);
    builder = match &policy.proxy {
        ProxyMode::Direct => builder.no_proxy(),
        ProxyMode::System => builder,
        ProxyMode::Explicit { url } => {
            let parsed = url::Url::parse(url).map_err(|_| PolicyError::Proxy)?;
            if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
                return Err(PolicyError::Proxy);
            }
            builder.proxy(reqwest::Proxy::all(parsed.as_str()).map_err(|_| PolicyError::Proxy)?)
        }
    };
    if !policy.extra_roots.is_empty() {
        let mut roots = Vec::new();
        for pem in &policy.extra_roots {
            for der in parse_roots(pem)? {
                roots.push(
                    reqwest::Certificate::from_der(der.as_ref()).map_err(|_| PolicyError::Root)?,
                );
            }
        }
        builder = builder.tls_certs_merge(roots);
    }
    builder.build().map_err(|_| PolicyError::Client)
}

/// The API Client's client (C15) under `policy`: the same proxy and trust
/// policy, no redirects or retries (each hop is admitted by the caller), no
/// automatic decompression or headers of its own, HTTP allowed (generic
/// workbench targets; tenant credentials are admitted for HTTPS only), an
/// idle read limit and no proxy at all when only loopback is reachable.
pub(crate) fn workbench_client(
    policy: &TransportPolicy,
    loopback_only: bool,
) -> Result<reqwest::Client, PolicyError> {
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(WORKBENCH_IDLE)
        .http1_max_headers(MAX_RESPONSE_HEADERS)
        .http2_max_header_list_size(MAX_RESPONSE_HEADER_BYTES as u32)
        .gzip(false)
        .referer(false);
    if loopback_only {
        // `localhost` is admitted in loopback-only builds; it never
        // reaches a resolver.
        builder = builder.resolve("localhost", std::net::SocketAddr::from(([127, 0, 0, 1], 0)));
    }
    builder = match (&policy.proxy, loopback_only) {
        (_, true) | (ProxyMode::Direct, _) => builder.no_proxy(),
        (ProxyMode::System, false) => builder,
        (ProxyMode::Explicit { url }, false) => {
            let parsed = url::Url::parse(url).map_err(|_| PolicyError::Proxy)?;
            if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
                return Err(PolicyError::Proxy);
            }
            builder.proxy(reqwest::Proxy::all(parsed.as_str()).map_err(|_| PolicyError::Proxy)?)
        }
    };
    if !policy.extra_roots.is_empty() {
        let mut roots = Vec::new();
        for pem in &policy.extra_roots {
            for der in parse_roots(pem)? {
                roots.push(
                    reqwest::Certificate::from_der(der.as_ref()).map_err(|_| PolicyError::Root)?,
                );
            }
        }
        builder = builder.tls_certs_merge(roots);
    }
    builder.build().map_err(|_| PolicyError::Client)
}

/// The API Client's idle read limit (v1: 120 s).
pub const WORKBENCH_IDLE: Duration = Duration::from_secs(120);

/// Parses one reviewed PEM input. Every block must be a certificate usable
/// as a trust anchor and there must be at least one: an unparseable root is
/// refused, never silently ignored (reqwest would accept it lazily).
pub fn parse_roots(
    pem: &[u8],
) -> Result<Vec<rustls_pki_types::CertificateDer<'static>>, PolicyError> {
    use rustls_pki_types::pem::PemObject;
    let certificates = rustls_pki_types::CertificateDer::pem_slice_iter(pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| PolicyError::Root)?;
    if certificates.is_empty() {
        return Err(PolicyError::Root);
    }
    for certificate in &certificates {
        webpki::anchor_from_trusted_cert(certificate).map_err(|_| PolicyError::Root)?;
    }
    Ok(certificates)
}

/// Builds transports for one policy revision. Direct and explicit clients
/// are pooled for the revision; System builds a fresh client per admission.
pub struct TransportFactory {
    policy: TransportPolicy,
    pooled: Option<reqwest::Client>,
}

impl TransportFactory {
    pub fn new(policy: TransportPolicy) -> Result<Self, PolicyError> {
        let pooled = match policy.proxy {
            ProxyMode::System => {
                build_client(&policy)?;
                None
            }
            _ => Some(build_client(&policy)?),
        };
        Ok(Self { policy, pooled })
    }

    pub fn policy(&self) -> &TransportPolicy {
        &self.policy
    }

    /// The transport for one admitted operation.
    pub fn for_admission(&self) -> Result<Arc<NativeTransport>, PolicyError> {
        let client = match &self.pooled {
            Some(client) => client.clone(),
            None => build_client(&self.policy)?,
        };
        Ok(Arc::new(NativeTransport {
            client,
            revision: self.policy.revision,
            log: None,
        }))
    }

    /// The transport for one admitted operation on a tenant's behalf, whose
    /// calls are recorded in `log` as "Show the calls" shows them (C33).
    pub fn for_tenant(
        &self,
        log: &CallLog,
        tag: CallTag,
    ) -> Result<Arc<NativeTransport>, PolicyError> {
        let client = match &self.pooled {
            Some(client) => client.clone(),
            None => build_client(&self.policy)?,
        };
        Ok(Arc::new(NativeTransport {
            client,
            revision: self.policy.revision,
            log: Some((log.clone(), tag)),
        }))
    }
}

/// What a recorded call was made for (C33).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum CallPurpose {
    /// A Runtime Security scan.
    Scan,
    /// A management read or change (including its token request).
    Management,
    /// An AI Gateway inference (Playground, files).
    Inference,
}

/// The credential a recorded call carried, by kind: the core attached the
/// real value; it is never recorded (C33).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum CallCredential {
    ScanKey,
    ManagementClient,
    WorkspaceKey,
}

/// Whose call it was and why (C33).
#[derive(Clone, Debug)]
pub struct CallTag {
    pub tenant: String,
    pub purpose: CallPurpose,
    pub credential: CallCredential,
}

/// One call as "Show the calls" shows it (C33): the request line without its
/// query, header names (never values) and sizes, and the response status,
/// duration and content type. Bodies are never kept: a response can carry
/// an issued key or a token, and request bodies carry tenant content.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct CallView {
    /// Increasing per app run.
    #[ts(type = "number")]
    pub sequence: u64,
    pub purpose: CallPurpose,
    pub credential: CallCredential,
    pub method: String,
    /// `https://host[:port]`.
    pub origin: String,
    pub path: String,
    /// The URL carried a query (withheld).
    pub query: bool,
    /// Request header names, sorted; the credential header is named but
    /// its value is the core's alone.
    pub request_headers: Vec<String>,
    #[ts(type = "number")]
    pub request_bytes: u64,
    /// The response status; absent when the call failed before one.
    pub status: Option<u16>,
    /// Until the response head (or the failure).
    #[ts(type = "number")]
    pub duration_ms: u64,
    pub content_type: Option<String>,
    pub response_headers: Vec<String>,
}

/// Calls recorded for "Show the calls", newest last (C33): in memory only,
/// the newest [`MAX_CALLS`] of the app run.
#[derive(Clone, Default)]
pub struct CallLog(Arc<Mutex<CallRecords>>);

#[derive(Default)]
struct CallRecords {
    next: u64,
    calls: VecDeque<(String, CallView)>,
}

/// Calls the log keeps (all tenants together).
pub const MAX_CALLS: usize = 200;
/// Calls a tenant's view shows, newest last.
pub const MAX_SHOWN_CALLS: usize = 50;

impl CallLog {
    fn record(&self, tenant: &str, call: impl FnOnce(u64) -> CallView) {
        if let Ok(mut log) = self.0.lock() {
            log.next += 1;
            let view = call(log.next);
            log.calls.push_back((tenant.to_owned(), view));
            while log.calls.len() > MAX_CALLS {
                log.calls.pop_front();
            }
        }
    }

    /// A tenant's newest calls, oldest first.
    pub fn for_tenant(&self, tenant: &str) -> Vec<CallView> {
        let Ok(log) = self.0.lock() else {
            return Vec::new();
        };
        let mut shown: Vec<CallView> = log
            .calls
            .iter()
            .rev()
            .filter(|(t, _)| t == tenant)
            .take(MAX_SHOWN_CALLS)
            .map(|(_, c)| c.clone())
            .collect();
        shown.reverse();
        shown
    }

    /// Forgets a tenant's calls (it was removed).
    pub fn forget(&self, tenant: &str) {
        if let Ok(mut log) = self.0.lock() {
            log.calls.retain(|(t, _)| t != tenant);
        }
    }
}

/// Bounds of a recorded call, as the presentation's contract accepts them
/// (C33): longer header names are left out, longer paths cut.
pub const CALL_HEADER_NAME_LIMIT: usize = 64;
pub const CALL_HEADERS_LIMIT: usize = 128;
pub const CALL_PATH_LIMIT: usize = 2048;
pub const CALL_METHOD_LIMIT: usize = 16;
pub const CALL_CONTENT_TYPE_LIMIT: usize = 128;

fn header_names(headers: &http::HeaderMap) -> Vec<String> {
    let mut names: Vec<String> = headers
        .keys()
        .map(|k| k.as_str())
        .filter(|k| k.len() <= CALL_HEADER_NAME_LIMIT)
        .map(str::to_owned)
        .collect();
    names.sort();
    names.dedup();
    names.truncate(CALL_HEADERS_LIMIT);
    names
}

/// What may be recorded of a request (C33): its method, an HTTPS origin and
/// the path, never the query or any header value. `None`: not an HTTPS
/// call, so nothing is recorded.
fn recorded_request(
    method: &http::Method,
    url: &url::Url,
    headers: &http::HeaderMap,
    body_bytes: u64,
) -> Option<(String, String, String, bool, Vec<String>, u64)> {
    if url.scheme() != "https" {
        return None;
    }
    let method = method.as_str();
    if method.is_empty() || method.len() > CALL_METHOD_LIMIT {
        return None;
    }
    Some((
        method.to_owned(),
        url.origin().ascii_serialization(),
        url.path().chars().take(CALL_PATH_LIMIT).collect(),
        url.query().is_some(),
        header_names(headers),
        body_bytes,
    ))
}

/// The app-owned `scm_sdk` transport.
pub struct NativeTransport {
    client: reqwest::Client,
    revision: u64,
    /// Where its calls are recorded, and for whom (C33).
    log: Option<(CallLog, CallTag)>,
}

impl NativeTransport {
    /// The policy revision this transport was built under.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The policy's HTTP client, for the Academy identity and learner
    /// adapter (same trust, proxy, redirect and retry policy).
    pub(crate) fn http(&self) -> &reqwest::Client {
        &self.client
    }
}

impl std::fmt::Debug for NativeTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeTransport")
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

fn transport_error() -> Error {
    Error::new(ErrorKind::Transport)
}

fn headers_within_limits(headers: &http::HeaderMap) -> bool {
    let bytes: usize = headers
        .iter()
        .map(|(k, v)| k.as_str().len() + v.len())
        .sum();
    headers.len() <= MAX_RESPONSE_HEADERS && bytes <= MAX_RESPONSE_HEADER_BYTES
}

/// Splits oversized decoded chunks so the SDK sees bounded pieces.
fn bounded(chunk: Bytes) -> Vec<Bytes> {
    if chunk.len() <= MAX_CHUNK_BYTES {
        return vec![chunk];
    }
    (0..chunk.len())
        .step_by(MAX_CHUNK_BYTES)
        .map(|start| chunk.slice(start..(start + MAX_CHUNK_BYTES).min(chunk.len())))
        .collect()
}

impl Transport for NativeTransport {
    fn send<'a>(
        &'a self,
        request: TransportRequest,
    ) -> BoxFuture<'a, scm_sdk::Result<TransportResponse>> {
        Box::pin(async move {
            let started = Instant::now();
            // What may be recorded of the request: never a value of any header,
            // never the query, never the body (C33).
            let recorded = self.log.as_ref().and_then(|_| {
                recorded_request(
                    request.method(),
                    request.url(),
                    request.headers(),
                    request.body().len() as u64,
                )
            });
            let sent = self
                .client
                .request(request.method().clone(), request.url().clone())
                .headers(request.headers().clone())
                .body(request.body().clone())
                .send()
                .await;
            if let (
                Some((log, tag)),
                Some((method, origin, path, query, request_headers, request_bytes)),
            ) = (&self.log, recorded)
            {
                let (status, content_type, response_headers) = match &sent {
                    Ok(r) => (
                        Some(r.status().as_u16()),
                        r.headers()
                            .get(http::header::CONTENT_TYPE)
                            .and_then(|v| v.to_str().ok())
                            .map(|v| v.chars().take(CALL_CONTENT_TYPE_LIMIT).collect()),
                        header_names(r.headers()),
                    ),
                    Err(_) => (None, None, Vec::new()),
                };
                let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                log.record(&tag.tenant, |sequence| CallView {
                    sequence,
                    purpose: tag.purpose,
                    credential: tag.credential,
                    method,
                    origin,
                    path,
                    query,
                    request_headers,
                    request_bytes,
                    status,
                    duration_ms,
                    content_type,
                    response_headers,
                });
            }
            let response = sent.map_err(|_| transport_error())?;
            let status = response.status();
            let headers = response.headers().clone();
            if !headers_within_limits(&headers) {
                return Err(Error::new(ErrorKind::Limit));
            }
            let body = response
                .bytes_stream()
                .map(|chunk| match chunk {
                    Ok(chunk) => {
                        futures_util::stream::iter(bounded(chunk).into_iter().map(Ok)).left_stream()
                    }
                    Err(_) => {
                        futures_util::stream::iter(vec![Err(transport_error())]).right_stream()
                    }
                })
                .flatten();
            Ok(TransportResponse::new(status, headers, Box::pin(body)))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recorded_call_stays_within_the_contracts_bounds() {
        let mut headers = http::HeaderMap::new();
        let long = "x".repeat(CALL_HEADER_NAME_LIMIT + 1);
        let longest = "y".repeat(CALL_HEADER_NAME_LIMIT);
        headers.insert(
            http::HeaderName::from_bytes(long.as_bytes()).expect("name"),
            http::HeaderValue::from_static("secret-value"),
        );
        headers.insert(
            http::HeaderName::from_bytes(longest.as_bytes()).expect("name"),
            http::HeaderValue::from_static("secret-value"),
        );
        headers.insert("x-pan-token", http::HeaderValue::from_static("SECRET"));
        let path = format!("/{}", "p".repeat(CALL_PATH_LIMIT * 2));
        let url =
            url::Url::parse(&format!("https://example.test:8443{path}?token=SECRET")).expect("url");
        let (method, origin, shown, query, names, bytes) =
            recorded_request(&http::Method::POST, &url, &headers, 12).expect("recorded");
        assert_eq!(method, "POST");
        assert_eq!(origin, "https://example.test:8443");
        assert_eq!(shown.chars().count(), CALL_PATH_LIMIT);
        assert!(!shown.contains('?') && !shown.contains("SECRET"));
        assert!(query);
        // Names only, never a value; a name past the bound is left out.
        assert_eq!(names, vec!["x-pan-token".to_owned(), longest]);
        assert_eq!(bytes, 12);
        // Many names: at most the bound.
        let mut many = http::HeaderMap::new();
        for n in 0..(CALL_HEADERS_LIMIT + 10) {
            many.insert(
                http::HeaderName::from_bytes(format!("h-{n:04}").as_bytes()).expect("name"),
                http::HeaderValue::from_static("v"),
            );
        }
        assert_eq!(header_names(&many).len(), CALL_HEADERS_LIMIT);
        // Not HTTPS: nothing is recorded.
        let plain = url::Url::parse("http://127.0.0.1:8080/items").expect("url");
        assert!(recorded_request(&http::Method::GET, &plain, &headers, 0).is_none());
    }

    #[test]
    fn chunks_are_bounded_without_losing_bytes() {
        let big = Bytes::from(vec![7u8; MAX_CHUNK_BYTES * 2 + 3]);
        let parts = bounded(big.clone());
        assert_eq!(parts.len(), 3);
        assert!(parts.iter().all(|p| p.len() <= MAX_CHUNK_BYTES));
        assert_eq!(parts.concat(), big.to_vec());
    }

    #[test]
    fn invalid_policies_are_refused() {
        let policy = |proxy| TransportPolicy {
            proxy,
            ..TransportPolicy::default()
        };
        assert_eq!(
            TransportFactory::new(policy(ProxyMode::Explicit {
                url: "ftp://proxy".into()
            }))
            .err(),
            Some(PolicyError::Proxy)
        );
        assert_eq!(
            TransportFactory::new(TransportPolicy {
                extra_roots: vec![b"not a certificate".to_vec()],
                ..TransportPolicy::default()
            })
            .err(),
            Some(PolicyError::Root)
        );
        assert!(TransportFactory::new(policy(ProxyMode::Direct)).is_ok());
        assert!(TransportFactory::new(policy(ProxyMode::System)).is_ok());
    }
}
