//! Sending an admitted request (C15). Rust builds the actual request from
//! the rendered plan and the secrets admission allowed (the presentation
//! never supplies a prepared request): v1's auth kinds, bodies (text, JSON,
//! form, multipart and stored files streamed from the collection, 64 MiB per
//! file and 128 MiB per multipart body) and OAuth client-credentials tokens
//! (minted without redirects, cached in memory only, dropped after a 401 and
//! never followed by a resend). The response streams into a sealed body
//! file (512 MiB at most) while the first 256 KiB are kept for display.
//!
//! Redirects: a request with tenant credentials never follows one (the 3xx
//! is the result); one with workbench secrets follows only within its
//! origin; an anonymous one follows up to 10 hops. Every hop passes the
//! build's network mode, cross-origin hops drop credentials and cookies
//! follow their own origin rules. Nothing is ever sent again by itself:
//! not after a 401, a lost response, a timeout or a cancel.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

use super::admission::{
    Admitted, BodyProblem, CredentialClass, Network, Registered, SecretFailure, SendRefusal,
};
use super::cookies::Jar;
use super::history::{self, BodyWriter, RunFailure, RunState};
use super::project;
use super::render::{Auth, Body, PartValue, Plan, Secrets, Template};

pub const FILE_BYTES: u64 = 64 * 1024 * 1024;
pub const MULTIPART_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_REDIRECTS: usize = 10;
const TOKEN_TIMEOUT: Duration = Duration::from_secs(30);
const TOKEN_BYTES: usize = 64 * 1024;
const READ_PIECE: usize = 256 * 1024;
/// Response header values kept per header.
const HEADER_TEXT: usize = 1024;
pub const REDACTED: &str = "[REDACTED]";

/// A v1 secret value: non-empty, no surrounding whitespace, no line break.
pub fn secret_ok(value: &str) -> Result<(), SecretFailure> {
    if value.is_empty() || value.trim() != value || value.contains(['\r', '\n']) {
        return Err(SecretFailure::Invalid);
    }
    Ok(())
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(TABLE[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// One piece of a request body.
#[derive(Clone, Debug)]
enum Chunk {
    Bytes(Bytes),
    /// A regular file inside the collection, exactly `len` bytes; with
    /// `sha256`, the content a review was bound to.
    File {
        path: PathBuf,
        len: u64,
        sha256: Option<String>,
    },
}

/// A file being streamed: what is left and, when pinned, its running hash.
struct Reading {
    file: std::fs::File,
    left: u64,
    pin: Option<(Sha256, String)>,
}

/// A request body that can be streamed again for a 307/308 hop.
#[derive(Clone, Debug)]
pub struct Payload {
    chunks: Vec<Chunk>,
    pub len: u64,
}

impl Payload {
    fn bytes(bytes: Vec<u8>) -> Self {
        Self {
            len: bytes.len() as u64,
            chunks: vec![Chunk::Bytes(Bytes::from(bytes))],
        }
    }

    fn body(&self, progress: Arc<AtomicU64>) -> reqwest::Body {
        reqwest::Body::wrap_stream(self.stream(progress))
    }

    fn stream(
        &self,
        progress: Arc<AtomicU64>,
    ) -> impl futures_util::Stream<Item = std::io::Result<Bytes>> + Send + 'static {
        let chunks = self.chunks.clone();
        futures_util::stream::unfold(
            (chunks.into_iter(), None::<Reading>),
            move |(mut rest, mut open)| {
                let progress = Arc::clone(&progress);
                async move {
                    loop {
                        if let Some(reading) = open.take() {
                            if reading.left == 0 {
                                continue;
                            }
                            let want =
                                READ_PIECE.min(usize::try_from(reading.left).unwrap_or(READ_PIECE));
                            let read = tokio::task::spawn_blocking(move || {
                                use std::io::Read;
                                let mut reading = reading;
                                let mut buffer = vec![0u8; want];
                                let n = reading.file.read(&mut buffer)?;
                                buffer.truncate(n);
                                Ok::<_, std::io::Error>((reading, buffer))
                            })
                            .await
                            .map_err(std::io::Error::other)
                            .and_then(|r| r);
                            return match read {
                                Ok((_, buffer)) if buffer.is_empty() => Some((
                                    Err(std::io::Error::other("file shrank while sending")),
                                    (rest, None),
                                )),
                                Ok((mut reading, buffer)) => {
                                    reading.left -= buffer.len() as u64;
                                    if let Some((hash, _)) = &mut reading.pin {
                                        hash.update(&buffer);
                                    }
                                    // A pinned file's last piece goes only
                                    // when the whole file is what was
                                    // reviewed.
                                    if reading.left == 0
                                        && let Some((hash, expected)) = reading.pin.take()
                                        && hex::encode(hash.finalize()) != expected
                                    {
                                        return Some((
                                            Err(std::io::Error::other(
                                                "file changed since it was reviewed",
                                            )),
                                            (rest, None),
                                        ));
                                    }
                                    progress.fetch_add(buffer.len() as u64, Ordering::Relaxed);
                                    Some((Ok(Bytes::from(buffer)), (rest, Some(reading))))
                                }
                                Err(e) => Some((Err(e), (rest, None))),
                            };
                        }
                        match rest.next()? {
                            Chunk::Bytes(b) => {
                                progress.fetch_add(b.len() as u64, Ordering::Relaxed);
                                return Some((Ok(b), (rest, None)));
                            }
                            Chunk::File { path, len, sha256 } => match std::fs::File::open(&path) {
                                Ok(file) => {
                                    open = Some(Reading {
                                        file,
                                        left: len,
                                        pin: sha256.map(|s| (Sha256::new(), s)),
                                    });
                                }
                                Err(e) => return Some((Err(e), (rest, None))),
                            },
                        }
                    }
                }
            },
        )
    }
}

/// A client-credentials grant, minted only after admission.
pub struct TokenGrant {
    pub url: url::Url,
    pub client_id: Zeroizing<String>,
    pub secret: Zeroizing<String>,
    pub scope: String,
}

impl TokenGrant {
    /// RFC 6749 2.3.1 client authentication: each part form-urlencoded,
    /// then Basic (the encoded pair, without the scheme).
    fn basic(&self) -> Zeroizing<String> {
        let encode = |text: &str| -> Zeroizing<String> {
            Zeroizing::new(url::form_urlencoded::byte_serialize(text.as_bytes()).collect())
        };
        let pair = Zeroizing::new(format!(
            "{}:{}",
            encode(&self.client_id).as_str(),
            encode(&self.secret).as_str()
        ));
        Zeroizing::new(base64(pair.as_bytes()))
    }

    fn key(&self) -> [u8; 32] {
        let mut hash = Sha256::new();
        for part in [
            self.url.as_str(),
            self.client_id.as_str(),
            self.secret.as_str(),
            self.scope.as_str(),
        ] {
            hash.update(part.as_bytes());
            hash.update([0]);
        }
        hash.finalize().into()
    }
}

/// The request Rust will send.
pub struct Prepared {
    pub method: reqwest::Method,
    pub url: url::Url,
    pub headers: Vec<(http::HeaderName, http::HeaderValue)>,
    pub payload: Option<Payload>,
    pub grant: Option<TokenGrant>,
    /// Every secret value used (longest first), for redaction.
    pub redactions: Vec<Zeroizing<String>>,
    pub class: CredentialClass,
}

impl Prepared {
    /// `text` with every secret value replaced.
    pub fn redact(&self, text: &str) -> String {
        redact(&self.redactions, text)
    }

    /// Some secret is too short to redact reliably (v1: under 8
    /// characters): recorded details are then withheld.
    pub fn short_secret(&self) -> bool {
        self.redactions.iter().any(|s| s.chars().count() < 8)
    }

    fn mutation(&self) -> bool {
        !matches!(self.method.as_str(), "GET" | "HEAD" | "OPTIONS")
    }
}

/// `text` with each of `secrets` (longest first) replaced.
fn redact(secrets: &[Zeroizing<String>], text: &str) -> String {
    let mut out = text.to_owned();
    for secret in secrets {
        if !secret.is_empty() {
            out = out.replace(secret.as_str(), REDACTED);
        }
    }
    out
}

/// A collection file for a body: inside, regular, within `limit`.
fn body_file(root: &Path, relative: &str, limit: u64) -> Result<(PathBuf, u64), SendRefusal> {
    let missing = SendRefusal::Body {
        problem: BodyProblem::Missing,
    };
    let rel = project::relative_path(relative, false).ok_or(missing.clone())?;
    let path = project::destination(root, &rel).map_err(|_| missing.clone())?;
    let meta = std::fs::symlink_metadata(&path).map_err(|_| missing.clone())?;
    if !meta.is_file() {
        return Err(missing);
    }
    if meta.len() > limit {
        return Err(SendRefusal::Body {
            problem: BodyProblem::TooLarge,
        });
    }
    Ok((path, meta.len()))
}

fn header(
    name: &str,
    value: &str,
    sensitive: bool,
) -> Result<(http::HeaderName, http::HeaderValue), SendRefusal> {
    let name = http::HeaderName::from_bytes(name.as_bytes()).map_err(|_| SendRefusal::Render {
        problem: super::render::RenderProblem::Name,
    })?;
    let mut value = http::HeaderValue::from_str(value).map_err(|_| SendRefusal::Render {
        problem: super::render::RenderProblem::Name,
    })?;
    value.set_sensitive(sensitive);
    Ok((name, value))
}

fn quoted(name: &str) -> String {
    name.replace('"', "%22").replace(['\r', '\n'], " ")
}

/// Builds the request from `plan` with the secrets admission allowed.
/// Files are measured now and streamed when sent; a file in `pins` (by
/// collection path) must still hold that content when its last byte goes.
pub fn prepare(
    plan: &Plan,
    admitted: &Admitted,
    secrets: &Secrets,
    root: &Path,
    pins: &std::collections::BTreeMap<String, String>,
) -> Result<Prepared, SendRefusal> {
    let resolve = |t: &Template| {
        t.resolve(secrets).ok_or_else(|| SendRefusal::Secret {
            reference: t.secrets().next().unwrap_or_default().to_owned(),
            failure: SecretFailure::Missing,
        })
    };
    let method =
        reqwest::Method::from_bytes(plan.method.as_bytes()).map_err(|_| SendRefusal::Url)?;
    let mut url = url::Url::parse(&resolve(&plan.url)?).map_err(|_| SendRefusal::Url)?;
    if url.origin() != admitted.url.origin() {
        return Err(SendRefusal::Url);
    }
    let mut query = Vec::new();
    for (n, v) in &plan.query {
        query.push((resolve(n)?, resolve(v)?));
    }
    let mut headers = Vec::new();
    for (name, value) in &plan.headers {
        headers.push(header(
            name,
            &resolve(value)?,
            value.secrets().next().is_some(),
        )?);
    }
    let secret = |r: &str| {
        secrets.get(r).cloned().ok_or_else(|| SendRefusal::Secret {
            reference: r.to_owned(),
            failure: SecretFailure::Missing,
        })
    };
    let mut grant = None;
    // Credentials derived from the secrets as they go on the wire.
    let mut derived: Vec<Zeroizing<String>> = Vec::new();
    match &plan.auth {
        Auth::None => {}
        Auth::Basic {
            username,
            secret: r,
        } => {
            let pair = Zeroizing::new(format!(
                "{}:{}",
                resolve(username)?.as_str(),
                secret(r)?.as_str()
            ));
            let encoded = Zeroizing::new(base64(pair.as_bytes()));
            headers.push(header(
                "authorization",
                &format!("Basic {}", encoded.as_str()),
                true,
            )?);
            derived.push(encoded);
        }
        Auth::Bearer { secret: r } => {
            headers.push(header(
                "authorization",
                &format!("Bearer {}", secret(r)?.as_str()),
                true,
            )?);
        }
        Auth::ApiKey {
            name,
            query: true,
            secret: r,
        } => query.push((Zeroizing::new(name.clone()), secret(r)?)),
        Auth::ApiKey {
            name, secret: r, ..
        } => headers.push(header(name, &secret(r)?, true)?),
        Auth::OAuth2 {
            token_url,
            client_id,
            scope,
            secret: r,
        } => {
            let token =
                url::Url::parse(&resolve(token_url)?).map_err(|_| SendRefusal::TokenDestination)?;
            grant = Some(TokenGrant {
                url: token,
                client_id: resolve(client_id)?,
                secret: secret(r)?,
                scope: resolve(scope)?.to_string(),
            });
        }
    }
    if !query.is_empty() {
        let mut pairs = url.query_pairs_mut();
        for (n, v) in &query {
            pairs.append_pair(n, v);
        }
    }
    let has_type = plan.has_header("content-type");
    let mut content_type = None;
    let payload = match &plan.body {
        Body::None => None,
        Body::Text { text, json } => {
            if *json {
                // v1: a JSON body says so unless a type was set.
                content_type = Some("application/json".to_owned());
            }
            Some(Payload::bytes(resolve(text)?.as_bytes().to_vec()))
        }
        Body::Form(rows) => {
            let mut form = url::form_urlencoded::Serializer::new(String::new());
            for (n, v) in rows {
                form.append_pair(&resolve(n)?, &resolve(v)?);
            }
            content_type = Some("application/x-www-form-urlencoded".to_owned());
            Some(Payload::bytes(form.finish().into_bytes()))
        }
        Body::Multipart(parts) => {
            let mut boundary = [0u8; 12];
            getrandom::fill(&mut boundary).map_err(|_| SendRefusal::Private)?;
            let boundary = format!("airs-{}", hex::encode(boundary));
            let mut chunks = Vec::new();
            let push = |chunks: &mut Vec<Chunk>, bytes: Vec<u8>| {
                chunks.push(Chunk::Bytes(Bytes::from(bytes)));
            };
            let mut files = 0u64;
            for part in parts {
                match &part.value {
                    PartValue::Text(t) => {
                        let mut head = format!(
                            "--{boundary}\r\nContent-Disposition: form-data; name=\"{}\"\r\n\r\n",
                            quoted(&part.name)
                        )
                        .into_bytes();
                        head.extend_from_slice(resolve(t)?.as_bytes());
                        head.extend_from_slice(b"\r\n");
                        push(&mut chunks, head);
                    }
                    PartValue::File { relative, mime } => {
                        let (path, size) = body_file(root, relative, FILE_BYTES)?;
                        files += size;
                        if files > MULTIPART_BYTES {
                            return Err(SendRefusal::Body {
                                problem: BodyProblem::TooLarge,
                            });
                        }
                        let file_name = relative.rsplit('/').next().unwrap_or(relative);
                        push(
                            &mut chunks,
                            format!(
                                "--{boundary}\r\nContent-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: {}\r\n\r\n",
                                quoted(&part.name),
                                quoted(file_name),
                                mime.as_deref().filter(|m| !m.contains(['\r', '\n'])).unwrap_or("application/octet-stream")
                            )
                            .into_bytes(),
                        );
                        chunks.push(Chunk::File {
                            path,
                            len: size,
                            sha256: pins.get(relative).cloned(),
                        });
                        push(&mut chunks, b"\r\n".to_vec());
                    }
                }
            }
            push(&mut chunks, format!("--{boundary}--\r\n").into_bytes());
            content_type = Some(format!("multipart/form-data; boundary={boundary}"));
            let len = chunks
                .iter()
                .map(|c| match c {
                    Chunk::Bytes(b) => b.len() as u64,
                    Chunk::File { len, .. } => *len,
                })
                .sum();
            Some(Payload { chunks, len })
        }
        Body::File { relative } => {
            let (path, len) = body_file(root, relative, FILE_BYTES)?;
            Some(Payload {
                chunks: vec![Chunk::File {
                    path,
                    len,
                    sha256: pins.get(relative).cloned(),
                }],
                len,
            })
        }
    };
    if let Some(ct) = content_type
        && !has_type
    {
        headers.push(header("content-type", &ct, false)?);
    }
    let mut redactions: Vec<Zeroizing<String>> = secrets.values().cloned().collect();
    redactions.extend(derived);
    if let Some(g) = &grant {
        redactions.push(g.client_id.clone());
        redactions.push(g.basic());
    }
    redactions.sort_by_key(|s| std::cmp::Reverse(s.len()));
    Ok(Prepared {
        method,
        url,
        headers,
        payload,
        grant,
        redactions,
        class: admitted.class,
    })
}

/// A minted token and when it stops being reused.
type Cached = (Zeroizing<String>, Instant);

/// Tokens minted for client-credentials grants, in memory only.
#[derive(Default)]
pub struct Tokens {
    cache: std::sync::Mutex<HashMap<[u8; 32], Cached>>,
}

impl Tokens {
    pub fn clear(&self) {
        if let Ok(mut c) = self.cache.lock() {
            c.clear();
        }
    }

    fn cached(&self, key: &[u8; 32]) -> Option<Zeroizing<String>> {
        let cache = self.cache.lock().ok()?;
        cache
            .get(key)
            .filter(|(_, until)| Instant::now() < *until)
            .map(|(t, _)| t.clone())
    }

    fn invalidate(&self, key: &[u8; 32]) {
        if let Ok(mut c) = self.cache.lock() {
            c.remove(key);
        }
    }

    async fn bearer(
        &self,
        client: &reqwest::Client,
        grant: &TokenGrant,
        network: Network,
    ) -> Result<Zeroizing<String>, RunFailure> {
        let key = grant.key();
        if let Some(token) = self.cached(&key) {
            return Ok(token);
        }
        if !network.admits(&grant.url) {
            return Err(RunFailure::Redirect);
        }
        let form = {
            let mut form = url::form_urlencoded::Serializer::new(String::new());
            form.append_pair("grant_type", "client_credentials");
            if !grant.scope.is_empty() {
                form.append_pair("scope", &grant.scope);
            }
            form.finish()
        };
        let mut auth = http::HeaderValue::from_str(&format!("Basic {}", grant.basic().as_str()))
            .map_err(|_| RunFailure::Token)?;
        auth.set_sensitive(true);
        let response = client
            .post(grant.url.clone())
            .header(http::header::AUTHORIZATION, auth)
            .header(
                http::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .header(http::header::ACCEPT, "application/json")
            .body(form)
            .timeout(TOKEN_TIMEOUT)
            .send()
            .await
            .map_err(|_| RunFailure::Token)?;
        if !response.status().is_success() {
            return Err(RunFailure::Token);
        }
        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| RunFailure::Token)?;
            if body.len() + chunk.len() > TOKEN_BYTES {
                return Err(RunFailure::Token);
            }
            body.extend_from_slice(&chunk);
        }
        let parsed: serde_json::Value =
            serde_json::from_slice(&body).map_err(|_| RunFailure::Token)?;
        let token = parsed
            .get("access_token")
            .and_then(serde_json::Value::as_str)
            .filter(|t| secret_ok(t).is_ok())
            .ok_or(RunFailure::Token)?;
        let token = Zeroizing::new(token.to_owned());
        let lifetime = parsed
            .get("expires_in")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(900)
            .clamp(1, 86_400);
        let reuse = lifetime - (lifetime / 2).min(60);
        if let Ok(mut c) = self.cache.lock() {
            c.insert(
                key,
                (token.clone(), Instant::now() + Duration::from_secs(reuse)),
            );
        }
        Ok(token)
    }
}

/// How a run ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ended {
    pub state: RunState,
    pub failure: Option<RunFailure>,
    pub uncertain: bool,
    pub status: Option<u16>,
    pub headers: Vec<(String, String)>,
    pub redirect: Option<String>,
    pub elapsed_ms: u64,
    pub bytes: u64,
    pub stored: bool,
    /// The first bytes of the body, for display.
    pub preview: Vec<u8>,
    /// A 401 to a minted token: the token was dropped (send again for a
    /// new one; nothing was resent).
    pub token_dropped: bool,
}

fn failure_of(error: &reqwest::Error) -> RunFailure {
    if error.is_timeout() {
        return RunFailure::Timeout;
    }
    let mut source: Option<&dyn std::error::Error> = Some(error);
    while let Some(e) = source {
        let text = e.to_string().to_ascii_lowercase();
        if text.contains("certificate") || text.contains("tls") || text.contains("handshake") {
            return RunFailure::Tls;
        }
        source = e.source();
    }
    if error.is_connect() {
        RunFailure::Connect
    } else {
        RunFailure::Request
    }
}

/// What a transfer needs besides the prepared request.
pub struct Transfer {
    pub client: reqwest::Client,
    pub network: Network,
    /// The collection's jar (requests without tenant credentials).
    pub cookies: Option<Arc<std::sync::Mutex<Jar>>>,
    pub tokens: Arc<Tokens>,
    pub cancel: CancellationToken,
    /// Bytes sent and received so far.
    pub progress: Arc<AtomicU64>,
    /// Registered Prisma origins: a generic redirect never enters one.
    pub registered: Registered,
    /// Whether the request may still go (its tenant context is still the
    /// active one), asked before a token is minted and before each send.
    pub still: Arc<dyn Fn() -> bool + Send + Sync>,
}

fn same_origin(a: &url::Url, b: &url::Url) -> bool {
    a.origin() == b.origin()
}

/// Sends `prepared` once (following only admitted redirects) and streams
/// the response into `sink`.
pub async fn send(prepared: &Prepared, transfer: Transfer, sink: BodyWriter) -> Ended {
    let started = Instant::now();
    let mutation = prepared.mutation();
    let mut ended = Ended {
        state: RunState::Failed,
        failure: None,
        uncertain: false,
        status: None,
        headers: Vec::new(),
        redirect: None,
        elapsed_ms: 0,
        bytes: 0,
        stored: false,
        preview: Vec::new(),
        token_dropped: false,
    };
    let finish = |mut ended: Ended| {
        ended.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        ended
    };
    let retired = |sink: BodyWriter, mut ended: Ended| {
        sink.discard();
        ended.failure = Some(RunFailure::Retired);
        finish(ended)
    };
    // The bearer token comes first: nothing else is sent if it fails.
    let bearer = match &prepared.grant {
        Some(_) if !(transfer.still)() => return retired(sink, ended),
        Some(grant) => {
            let minted = tokio::select! {
                biased;
                () = transfer.cancel.cancelled() => Err(None),
                minted = transfer.tokens.bearer(&transfer.client, grant, transfer.network) => minted.map_err(Some),
            };
            match minted {
                Ok(token) => Some(token),
                Err(None) => {
                    sink.discard();
                    ended.state = RunState::Cancelled;
                    return finish(ended);
                }
                Err(Some(failure)) => {
                    sink.discard();
                    ended.failure = Some(failure);
                    return finish(ended);
                }
            }
        }
        None => None,
    };
    // A minted token is a wire credential too: never shown or kept.
    let mut redactions = prepared.redactions.clone();
    if let Some(token) = &bearer {
        redactions.push(token.clone());
        redactions.sort_by_key(|s| std::cmp::Reverse(s.len()));
    }
    let mut url = prepared.url.clone();
    let mut method = prepared.method.clone();
    let mut with_body = true;
    let mut hop = 0usize;
    let response = loop {
        if !transfer.network.admits(&url) {
            sink.discard();
            ended.failure = Some(RunFailure::Redirect);
            return finish(ended);
        }
        let crossed = !same_origin(&url, &prepared.url);
        let mut request = transfer.client.request(method.clone(), url.clone());
        for (name, value) in &prepared.headers {
            // Credentials and cookies never cross to another origin.
            let credential = value.is_sensitive()
                || matches!(
                    name.as_str(),
                    "authorization" | "cookie" | "proxy-authorization"
                );
            if crossed && credential {
                continue;
            }
            if !with_body && name == http::header::CONTENT_TYPE {
                continue;
            }
            request = request.header(name.clone(), value.clone());
        }
        if let Some(token) = &bearer {
            let mut value = match http::HeaderValue::from_str(&format!("Bearer {}", token.as_str()))
            {
                Ok(v) => v,
                Err(_) => {
                    sink.discard();
                    ended.failure = Some(RunFailure::Token);
                    return finish(ended);
                }
            };
            value.set_sensitive(true);
            request = request.header(http::header::AUTHORIZATION, value);
        }
        if let Some(jar) = &transfer.cookies
            && let Some(cookie) = jar
                .lock()
                .ok()
                .and_then(|j| j.header(&url, history::now() as i64))
            && let Ok(value) = http::HeaderValue::from_str(&cookie)
        {
            request = request.header(http::header::COOKIE, value);
        }
        if with_body && let Some(payload) = &prepared.payload {
            request = request
                .header(http::header::CONTENT_LENGTH, payload.len)
                .body(payload.body(Arc::clone(&transfer.progress)));
        }
        if !(transfer.still)() {
            return retired(sink, ended);
        }
        let sent = tokio::select! {
            biased;
            () = transfer.cancel.cancelled() => None,
            r = request.send() => Some(r),
        };
        let response = match sent {
            None => {
                sink.discard();
                ended.state = RunState::Cancelled;
                ended.uncertain = mutation;
                return finish(ended);
            }
            Some(Err(e)) => {
                sink.discard();
                let failure = failure_of(&e);
                ended.uncertain =
                    mutation && !matches!(failure, RunFailure::Connect | RunFailure::Tls);
                ended.failure = Some(failure);
                return finish(ended);
            }
            Some(Ok(r)) => r,
        };
        if let Some(jar) = &transfer.cookies
            && let Ok(mut jar) = jar.lock()
        {
            jar.store(
                &url,
                response
                    .headers()
                    .get_all(http::header::SET_COOKIE)
                    .iter()
                    .map(http::HeaderValue::as_bytes),
                history::now() as i64,
            );
        }
        let status = response.status();
        let location = response
            .headers()
            .get(http::header::LOCATION)
            .and_then(|l| l.to_str().ok())
            .and_then(|l| url.join(l).ok());
        let follow = status.is_redirection()
            && hop < MAX_REDIRECTS
            && location.as_ref().is_some_and(|next| {
                // Entering a registered origin needs its own admission.
                !transfer.registered.contains(next)
                    && match prepared.class {
                        CredentialClass::Anonymous => true,
                        CredentialClass::Workbench => same_origin(next, &prepared.url),
                        CredentialClass::Managed => false,
                    }
            });
        if follow && let Some(next) = location {
            match status.as_u16() {
                303 if method != reqwest::Method::HEAD => {
                    method = reqwest::Method::GET;
                    with_body = false;
                }
                301 | 302 if method == reqwest::Method::POST => {
                    method = reqwest::Method::GET;
                    with_body = false;
                }
                _ => {}
            }
            url = next;
            hop += 1;
            continue;
        }
        if status.is_redirection() {
            ended.redirect = location.map(|l| redact(&redactions, l.as_str()));
        }
        break response;
    };
    let status = response.status();
    ended.status = Some(status.as_u16());
    if status == reqwest::StatusCode::UNAUTHORIZED
        && let Some(grant) = &prepared.grant
    {
        transfer.tokens.invalidate(&grant.key());
        ended.token_dropped = true;
    }
    let withhold = prepared.short_secret();
    ended.headers = response
        .headers()
        .iter()
        .take(history::MAX_HEADERS)
        .map(|(n, v)| {
            let text = String::from_utf8_lossy(v.as_bytes());
            let shown = if withhold || v.is_sensitive() {
                REDACTED.to_owned()
            } else {
                redact(&redactions, &text)
                    .chars()
                    .take(HEADER_TEXT)
                    .collect()
            };
            (n.as_str().to_owned(), shown)
        })
        .collect();
    // The body is sealed on a blocking writer as it arrives.
    // `None` aborts: the partial body is removed, never sealed as whole.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Option<Bytes>>(8);
    let writer = tokio::task::spawn_blocking(move || {
        let mut sink = sink;
        while let Some(message) = rx.blocking_recv() {
            match message {
                Some(chunk) => {
                    if sink.write(&chunk).is_err() {
                        sink.discard();
                        return false;
                    }
                }
                None => {
                    sink.discard();
                    return false;
                }
            }
        }
        sink.finish().is_ok()
    });
    let mut stream = response.bytes_stream();
    let mut outcome: Option<(RunState, Option<RunFailure>)> = None;
    loop {
        let next = tokio::select! {
            biased;
            () = transfer.cancel.cancelled() => {
                outcome = Some((RunState::Cancelled, None));
                break;
            }
            next = stream.next() => next,
        };
        match next {
            None => break,
            Some(Err(e)) => {
                outcome = Some((RunState::Failed, Some(failure_of(&e))));
                break;
            }
            Some(Ok(chunk)) => {
                ended.bytes += chunk.len() as u64;
                transfer
                    .progress
                    .fetch_add(chunk.len() as u64, Ordering::Relaxed);
                if ended.bytes > history::MAX_RESPONSE {
                    outcome = Some((RunState::Failed, Some(RunFailure::TooLarge)));
                    break;
                }
                if ended.preview.len() < history::PREVIEW_BYTES {
                    let take = (history::PREVIEW_BYTES - ended.preview.len()).min(chunk.len());
                    ended.preview.extend_from_slice(&chunk[..take]);
                }
                if tx.send(Some(chunk)).await.is_err() {
                    outcome = Some((RunState::Failed, Some(RunFailure::Storage)));
                    break;
                }
            }
        }
    }
    if outcome.is_some() {
        let _ = tx.send(None).await;
    }
    drop(tx);
    let stored = writer.await.unwrap_or(false);
    match outcome {
        None if stored => {
            ended.state = RunState::Completed;
            ended.stored = true;
        }
        None => {
            ended.state = RunState::Completed;
            ended.failure = Some(RunFailure::Storage);
        }
        Some((state, failure)) => {
            // An incomplete body is never kept as if complete.
            ended.state = if state == RunState::Cancelled {
                state
            } else {
                RunState::Completed
            };
            ended.failure = failure;
            ended.stored = false;
        }
    }
    finish(ended)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648() {
        for (plain, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("user:pass", "dXNlcjpwYXNz"),
        ] {
            assert_eq!(base64(plain.as_bytes()), encoded);
        }
    }

    #[tokio::test]
    async fn a_pinned_file_that_changed_never_sends_its_last_piece() {
        let dir = crate::workbench::testdir::Dir::new();
        let path = dir.path().join("body.bin");
        std::fs::write(&path, vec![7u8; READ_PIECE + 10]).expect("fixture");
        let reviewed = hex::encode(Sha256::digest(vec![7u8; READ_PIECE + 10]));
        let payload = |pin: &str| Payload {
            chunks: vec![
                Chunk::Bytes(Bytes::from_static(b"head")),
                Chunk::File {
                    path: path.clone(),
                    len: (READ_PIECE + 10) as u64,
                    sha256: Some(pin.to_owned()),
                },
            ],
            len: (READ_PIECE + 14) as u64,
        };
        let pieces: Vec<_> = payload(&reviewed)
            .stream(Arc::default())
            .collect::<Vec<_>>()
            .await;
        assert!(pieces.iter().all(Result::is_ok));
        assert_eq!(
            pieces
                .iter()
                .map(|p| p.as_ref().map_or(0, Bytes::len))
                .sum::<usize>(),
            READ_PIECE + 14
        );
        // Changed after the review: the first piece may go, never the last.
        std::fs::write(&path, vec![8u8; READ_PIECE + 10]).expect("fixture");
        let pieces: Vec<_> = payload(&reviewed)
            .stream(Arc::default())
            .collect::<Vec<_>>()
            .await;
        let sent: usize = pieces
            .iter()
            .filter_map(|p| p.as_ref().ok())
            .map(Bytes::len)
            .sum();
        assert!(pieces.last().is_some_and(Result::is_err));
        assert_eq!(sent, 4 + READ_PIECE, "the file's last piece was withheld");
    }

    #[test]
    fn secrets_follow_v1_rules() {
        assert!(secret_ok("value").is_ok());
        for bad in ["", " lead", "trail ", "a\nb"] {
            assert_eq!(secret_ok(bad), Err(SecretFailure::Invalid));
        }
    }
}
