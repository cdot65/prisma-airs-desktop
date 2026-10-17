//! Synthetic native endpoints: a test CA, TLS servers with scripted replies
//! and a CONNECT proxy that records what it forwarded. Everything binds to
//! 127.0.0.1 and uses fixed fixture hostnames; nothing reaches a product
//! endpoint.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// A test certificate authority.
pub struct TestCa {
    pub pem: String,
    issuer: Issuer<'static, KeyPair>,
}

fn validity(params: &mut CertificateParams, days: i64) {
    let now = time::OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::days(1);
    params.not_after = now + time::Duration::days(days);
}

impl TestCa {
    pub fn new(name: &str) -> Self {
        let mut params = CertificateParams::new(Vec::<String>::new()).expect("params");
        params.distinguished_name.push(DnType::CommonName, name);
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
            KeyUsagePurpose::DigitalSignature,
        ];
        validity(&mut params, 30);
        let key = KeyPair::generate().expect("key");
        let cert = params.self_signed(&key).expect("ca");
        Self {
            pem: cert.pem(),
            issuer: Issuer::new(params, key),
        }
    }

    /// A server certificate for `names` (DNS names or IP literals).
    pub fn leaf(&self, names: &[&str]) -> (CertificateDer<'static>, PrivateKeyDer<'static>) {
        let mut params =
            CertificateParams::new(names.iter().map(|n| n.to_string()).collect::<Vec<_>>())
                .expect("params");
        params.distinguished_name.push(DnType::CommonName, names[0]);
        params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyEncipherment,
        ];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        validity(&mut params, 7);
        let key = KeyPair::generate().expect("key");
        let cert = params.signed_by(&key, &self.issuer).expect("leaf");
        (
            cert.der().clone(),
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
        )
    }
}

/// A request as the server received it.
#[derive(Clone, Debug, Default)]
pub struct Seen {
    pub request_line: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Seen {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// What the server does after reading a request.
#[derive(Clone)]
pub enum Reply {
    /// Raw HTTP/1.1 response head (without the blank line) and body.
    Raw { head: String, body: Vec<u8> },
    /// Never answer; record when the client goes away.
    Hang,
}

impl Reply {
    pub fn ok(body: &[u8]) -> Self {
        Reply::Raw {
            head: format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close",
                body.len()
            ),
            body: body.to_vec(),
        }
    }
}

#[derive(Default)]
pub struct ServerLog {
    pub seen: Vec<Seen>,
    pub connections: usize,
    pub closed_by_client: usize,
}

pub struct TlsServer {
    pub port: u16,
    pub log: Arc<Mutex<ServerLog>>,
}

async fn read_request<S: AsyncReadExt + Unpin>(stream: &mut S) -> Option<Seen> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.split("\r\n");
    let request_line = lines.next()?.to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect();
    let length: usize = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    Some(Seen {
        request_line,
        headers,
        body,
    })
}

impl TlsServer {
    /// Serves every connection with `reply` using the given certificate.
    pub async fn start(
        cert: (CertificateDer<'static>, PrivateKeyDer<'static>),
        reply: Reply,
    ) -> Self {
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("versions")
            .with_no_client_auth()
            .with_single_cert(vec![cert.0], cert.1)
            .expect("server config");
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let log = Arc::new(Mutex::new(ServerLog::default()));
        let server_log = Arc::clone(&log);
        tokio::spawn(async move {
            loop {
                let Ok((tcp, _)) = listener.accept().await else {
                    return;
                };
                server_log.lock().expect("log").connections += 1;
                let acceptor = acceptor.clone();
                let reply = reply.clone();
                let log = Arc::clone(&server_log);
                tokio::spawn(async move {
                    let Ok(mut tls) = acceptor.accept(tcp).await else {
                        return;
                    };
                    let Some(seen) = read_request(&mut tls).await else {
                        return;
                    };
                    log.lock().expect("log").seen.push(seen);
                    match reply {
                        Reply::Raw { head, body } => {
                            let _ = tls.write_all(format!("{head}\r\n\r\n").as_bytes()).await;
                            let _ = tls.write_all(&body).await;
                            let _ = tls.shutdown().await;
                        }
                        Reply::Hang => {
                            let mut sink = [0u8; 64];
                            // Returns when the client closes its side.
                            loop {
                                match tls.read(&mut sink).await {
                                    Ok(0) | Err(_) => break,
                                    Ok(_) => {}
                                }
                            }
                            log.lock().expect("log").closed_by_client += 1;
                        }
                    }
                });
            }
        });
        Self { port, log }
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.log.lock().expect("log").seen.clone()
    }

    pub fn closed_by_client(&self) -> usize {
        self.log.lock().expect("log").closed_by_client
    }
}

/// A CONNECT proxy that forwards every tunnel to 127.0.0.1:`target` and
/// records the authorities it was asked for.
pub struct Proxy {
    pub port: u16,
    pub tunnels: Arc<Mutex<Vec<String>>>,
}

impl Proxy {
    pub async fn start(target: u16) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let tunnels = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&tunnels);
        tokio::spawn(async move {
            loop {
                let Ok((mut client, _)) = listener.accept().await else {
                    return;
                };
                let log = Arc::clone(&log);
                tokio::spawn(async move {
                    let Some(seen) = read_request(&mut client).await else {
                        return;
                    };
                    let mut parts = seen.request_line.split_whitespace();
                    if parts.next() != Some("CONNECT") {
                        let _ = client
                            .write_all(b"HTTP/1.1 405 Method Not Allowed\r\n\r\n")
                            .await;
                        return;
                    }
                    log.lock()
                        .expect("log")
                        .push(parts.next().unwrap_or_default().to_owned());
                    let Ok(mut upstream) = TcpStream::connect(("127.0.0.1", target)).await else {
                        return;
                    };
                    let _ = client
                        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                        .await;
                    let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
                });
            }
        });
        Self { port, tunnels }
    }

    pub fn tunnels(&self) -> Vec<String> {
        self.tunnels.lock().expect("log").clone()
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

/// Waits until `condition` holds or the timeout passes.
pub async fn eventually(mut condition: impl FnMut() -> bool) -> bool {
    for _ in 0..200 {
        if condition() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    condition()
}
