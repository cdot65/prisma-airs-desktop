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

use std::sync::Arc;
use std::time::Duration;

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
        }))
    }
}

/// The app-owned `scm_sdk` transport.
pub struct NativeTransport {
    client: reqwest::Client,
    revision: u64,
}

impl NativeTransport {
    /// The policy revision this transport was built under.
    pub fn revision(&self) -> u64 {
        self.revision
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
            let response = self
                .client
                .request(request.method().clone(), request.url().clone())
                .headers(request.headers().clone())
                .body(request.body().clone())
                .send()
                .await
                .map_err(|_| transport_error())?;
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
