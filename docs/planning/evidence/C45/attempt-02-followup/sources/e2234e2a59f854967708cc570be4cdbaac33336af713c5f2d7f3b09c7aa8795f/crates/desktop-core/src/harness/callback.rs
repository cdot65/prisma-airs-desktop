//! The loopback redirect for company sign-in (C20, D047; RFC 8252 §7.3).
//!
//! A listener on `127.0.0.1` at an ephemeral port lives only for one
//! attempt. It answers plain `GET /callback` requests whose `Host` names
//! that address and port, bounded in count, size and time. Only a callback
//! carrying this attempt's `state` (compared in constant time) ends the
//! attempt; anything else is answered and ignored, so another local page
//! can neither complete nor cancel it. An `iss` parameter must be the
//! configured issuer, and it is required when the provider says it sends
//! one (RFC 9207).

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::Instant;
use zeroize::Zeroizing;

use super::identity::{SignInFailure, constant_eq};

/// How long the browser has to come back.
pub const ATTEMPT: Duration = Duration::from_secs(300);
/// How long one connection may take to send its request.
const PER_CONNECTION: Duration = Duration::from_secs(3);
/// Connections accepted in one attempt.
const MAX_CONNECTIONS: usize = 32;
const REQUEST_LIMIT: usize = 8 * 1024;
const CODE_LIMIT: usize = 4 * 1024;

const DONE: &str = "<!doctype html><meta charset=utf-8><title>Prisma AIRS Desktop</title>\
<p>Sign-in is complete. You can close this window and return to Prisma AIRS Desktop.</p>";
const NOT_DONE: &str = "<!doctype html><meta charset=utf-8><title>Prisma AIRS Desktop</title>\
<p>Sign-in was not completed. Return to Prisma AIRS Desktop to try again.</p>";

/// One attempt's listener.
pub struct Callback {
    listener: TcpListener,
    port: u16,
}

/// What the callback named for this attempt.
pub struct Code(pub Zeroizing<String>);

impl Callback {
    /// Listens on `127.0.0.1` at a port the system chooses.
    pub async fn bind() -> Result<Self, SignInFailure> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|_| SignInFailure::CallbackUnavailable)?;
        let port = listener
            .local_addr()
            .map_err(|_| SignInFailure::CallbackUnavailable)?
            .port();
        Ok(Self { listener, port })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Waits for this attempt's callback until `deadline`.
    pub async fn wait(
        self,
        state: &str,
        issuer: &str,
        iss_required: bool,
        deadline: Instant,
    ) -> Result<Code, SignInFailure> {
        let host = format!("127.0.0.1:{}", self.port);
        for _ in 0..MAX_CONNECTIONS {
            let accepted = tokio::time::timeout_at(deadline, self.listener.accept()).await;
            let Ok(Ok((mut stream, _))) = accepted else {
                return Err(SignInFailure::TimedOut);
            };
            let until = deadline.min(Instant::now() + PER_CONNECTION);
            let request = tokio::time::timeout_at(until, read_request(&mut stream))
                .await
                .ok()
                .flatten();
            let verdict = request
                .as_deref()
                .map(|r| judge(r, &host, state, issuer, iss_required))
                .unwrap_or(Verdict::Ignore(400));
            match verdict {
                Verdict::Ignore(status) => {
                    respond(&mut stream, status, NOT_DONE).await;
                }
                Verdict::Code(code) => {
                    respond(&mut stream, 200, DONE).await;
                    return Ok(Code(code));
                }
                Verdict::Error => {
                    respond(&mut stream, 200, NOT_DONE).await;
                    return Err(SignInFailure::Denied);
                }
            }
        }
        Err(SignInFailure::TimedOut)
    }
}

/// The request head (up to the blank line), bounded.
async fn read_request(stream: &mut TcpStream) -> Option<String> {
    let mut head = Vec::new();
    let mut buf = [0u8; 1024];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = stream.read(&mut buf).await.ok()?;
        if n == 0 || head.len() + n > REQUEST_LIMIT {
            return None;
        }
        head.extend_from_slice(&buf[..n]);
    }
    String::from_utf8(head).ok()
}

async fn respond(stream: &mut TcpStream, status: u16, body: &str) {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Bad Request",
    };
    let reply = format!(
        "HTTP/1.1 {status} {reason}\r\ncontent-type: text/html; charset=utf-8\r\n\
         content-length: {}\r\ncache-control: no-store\r\nreferrer-policy: no-referrer\r\n\
         connection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = tokio::time::timeout(PER_CONNECTION, async {
        let _ = stream.write_all(reply.as_bytes()).await;
        let _ = stream.shutdown().await;
    })
    .await;
}

enum Verdict {
    Ignore(u16),
    Code(Zeroizing<String>),
    /// The provider sent this attempt back with an error.
    Error,
}

fn judge(request: &str, host: &str, state: &str, issuer: &str, iss_required: bool) -> Verdict {
    let mut lines = request.split("\r\n");
    let Some(line) = lines.next() else {
        return Verdict::Ignore(400);
    };
    let mut parts = line.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Verdict::Ignore(400);
    };
    if !version.starts_with("HTTP/1.") {
        return Verdict::Ignore(400);
    }
    // Exactly one Host header naming this listener (DNS rebinding).
    let hosts: Vec<&str> = lines
        .filter_map(|l| l.split_once(':'))
        .filter(|(name, _)| name.trim().eq_ignore_ascii_case("host"))
        .map(|(_, value)| value.trim())
        .collect();
    if hosts != [host] {
        return Verdict::Ignore(400);
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != "/callback" {
        return Verdict::Ignore(404);
    }
    if method != "GET" {
        return Verdict::Ignore(405);
    }
    let pairs: Vec<(String, Zeroizing<String>)> = url::form_urlencoded::parse(query.as_bytes())
        .map(|(k, v)| (k.into_owned(), Zeroizing::new(v.into_owned())))
        .collect();
    let one = |name: &str| {
        let mut found = pairs.iter().filter(|(k, _)| k == name);
        match (found.next(), found.next()) {
            (Some((_, v)), None) => Ok(Some(v.as_str())),
            (None, _) => Ok(None),
            _ => Err(()),
        }
    };
    let (Ok(given_state), Ok(code), Ok(error), Ok(iss)) =
        (one("state"), one("code"), one("error"), one("iss"))
    else {
        return Verdict::Ignore(400);
    };
    if !given_state.is_some_and(|s| constant_eq(s.as_bytes(), state.as_bytes())) {
        return Verdict::Ignore(400);
    }
    if iss.map_or(iss_required, |i| i != issuer) {
        return Verdict::Ignore(400);
    }
    match (code, error) {
        (_, Some(_)) => Verdict::Error,
        (Some(code), None)
            if !code.is_empty()
                && code.len() <= CODE_LIMIT
                && code.bytes().all(|b| b.is_ascii_graphic()) =>
        {
            Verdict::Code(Zeroizing::new(code.to_owned()))
        }
        _ => Verdict::Ignore(400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ISS: &str = "https://id.example";

    fn get(target: &str, host: &str) -> String {
        format!("GET {target} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: test\r\n\r\n")
    }

    #[test]
    fn only_this_attempts_callback_is_taken() {
        let host = "127.0.0.1:5000";
        let code = |v: Verdict| match v {
            Verdict::Code(c) => Some(c.to_string()),
            _ => None,
        };
        assert_eq!(
            code(judge(
                &get("/callback?code=abc&state=s1", host),
                host,
                "s1",
                ISS,
                false
            )),
            Some("abc".into())
        );
        // Another state, a missing state, a repeated parameter, another
        // host or path are ignored, never an end.
        for request in [
            get("/callback?code=abc&state=s2", host),
            get("/callback?code=abc", host),
            get("/callback?code=abc&state=s1&state=s1", host),
            get("/callback?code=abc&code=def&state=s1", host),
            get("/callback?code=abc&state=s1", "localhost:5000"),
            get("/callback?code=abc&state=s1", "127.0.0.1:5001"),
            get("/other?code=abc&state=s1", host),
            format!("POST /callback?code=abc&state=s1 HTTP/1.1\r\nHost: {host}\r\n\r\n"),
            format!(
                "GET /callback?code=abc&state=s1 HTTP/1.1\r\nHost: {host}\r\nHost: {host}\r\n\r\n"
            ),
            get("/callback?state=s1", host),
            get(
                &format!("/callback?state=s1&code={}", "a".repeat(CODE_LIMIT + 1)),
                host,
            ),
        ] {
            assert!(
                matches!(judge(&request, host, "s1", ISS, false), Verdict::Ignore(_)),
                "{request}"
            );
        }
        // An error for this attempt ends it as declined.
        assert!(matches!(
            judge(
                &get("/callback?error=access_denied&state=s1", host),
                host,
                "s1",
                ISS,
                false
            ),
            Verdict::Error
        ));
        // RFC 9207: `iss` must be the issuer, and is required when promised.
        let evil = get(
            "/callback?code=abc&state=s1&iss=https%3A%2F%2Fevil.example",
            host,
        );
        assert!(matches!(
            judge(&evil, host, "s1", ISS, false),
            Verdict::Ignore(_)
        ));
        assert!(matches!(
            judge(
                &get("/callback?code=abc&state=s1", host),
                host,
                "s1",
                ISS,
                true
            ),
            Verdict::Ignore(_)
        ));
        let good = get(
            "/callback?code=abc&state=s1&iss=https%3A%2F%2Fid.example",
            host,
        );
        assert!(code(judge(&good, host, "s1", ISS, true)).is_some());
    }
}
