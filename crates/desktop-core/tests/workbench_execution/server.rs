//! A plain-HTTP loopback fixture server for the API Client: it records each
//! request exactly as received (request line, headers, body) and answers
//! from a route, including large streamed bodies, closes without a reply
//! (lost responses) and hangs. It binds 127.0.0.1 only.

use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub use crate::support::Seen;

/// What the server answers.
#[derive(Clone)]
pub enum Answer {
    /// Status, extra headers and a body.
    Body {
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    },
    /// `len` bytes of a repeating pattern, streamed in 64 KiB writes.
    Stream { len: u64 },
    /// Read the request, then close (a lost response).
    Close,
    /// Never answer.
    Hang,
}

impl Answer {
    pub fn ok(body: &str) -> Self {
        Self::status(200, &[("content-type", "application/json")], body)
    }

    pub fn status(status: u16, headers: &[(&str, &str)], body: &str) -> Self {
        Self::Body {
            status,
            headers: headers
                .iter()
                .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
                .collect(),
            body: body.as_bytes().to_vec(),
        }
    }
}

pub type Route = Arc<dyn Fn(&Seen) -> Answer + Send + Sync>;

pub struct Http {
    pub port: u16,
    seen: Arc<Mutex<Vec<Seen>>>,
    accept: tokio::task::JoinHandle<()>,
}

/// The repeating pattern streamed bodies are made of.
pub fn pattern(offset: u64) -> u8 {
    (offset % 251) as u8
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> Option<Seen> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
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

impl Http {
    pub async fn start(route: Route) -> Self {
        Self::start_on(0, route).await
    }

    /// Listens on `port` (0: any free port).
    pub async fn start_on(port: u16, route: Route) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", port)).await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        let accept = tokio::spawn(async move {
            loop {
                let Ok((mut tcp, _)) = listener.accept().await else {
                    return;
                };
                let route = Arc::clone(&route);
                let log = Arc::clone(&log);
                tokio::spawn(async move {
                    let Some(request) = read_request(&mut tcp).await else {
                        return;
                    };
                    let answer = route(&request);
                    log.lock().expect("log").push(request);
                    match answer {
                        Answer::Body {
                            status,
                            headers,
                            body,
                        } => {
                            let mut head = format!("HTTP/1.1 {status} Fixture\r\n");
                            for (n, v) in headers {
                                head.push_str(&format!("{n}: {v}\r\n"));
                            }
                            head.push_str(&format!(
                                "content-length: {}\r\nconnection: close\r\n\r\n",
                                body.len()
                            ));
                            let _ = tcp.write_all(head.as_bytes()).await;
                            let _ = tcp.write_all(&body).await;
                            let _ = tcp.shutdown().await;
                        }
                        Answer::Stream { len } => {
                            let head = format!(
                                "HTTP/1.1 200 Fixture\r\ncontent-type: application/octet-stream\r\ncontent-length: {len}\r\nconnection: close\r\n\r\n"
                            );
                            if tcp.write_all(head.as_bytes()).await.is_err() {
                                return;
                            }
                            let mut sent = 0u64;
                            let mut piece = vec![0u8; 64 * 1024];
                            while sent < len {
                                let n = (len - sent).min(piece.len() as u64) as usize;
                                for (i, b) in piece[..n].iter_mut().enumerate() {
                                    *b = pattern(sent + i as u64);
                                }
                                if tcp.write_all(&piece[..n]).await.is_err() {
                                    return;
                                }
                                sent += n as u64;
                            }
                            let _ = tcp.shutdown().await;
                        }
                        Answer::Close => {
                            let _ = tcp.shutdown().await;
                        }
                        Answer::Hang => {
                            let mut sink = [0u8; 64];
                            loop {
                                match tcp.read(&mut sink).await {
                                    Ok(0) | Err(_) => break,
                                    Ok(_) => {}
                                }
                            }
                        }
                    }
                });
            }
        });
        Self { port, seen, accept }
    }

    /// Stops listening (the port refuses connections: offline).
    pub async fn stop(self) {
        self.accept.abort();
        let _ = self.accept.await;
    }

    pub fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().expect("log").clone()
    }
}
