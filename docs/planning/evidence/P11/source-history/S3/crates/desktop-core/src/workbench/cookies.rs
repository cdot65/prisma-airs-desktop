//! The API Client's cookies (C15): one jar per collection on this Mac,
//! used only by requests without tenant credentials. Set-Cookie is parsed
//! by the `cookie` crate (already locked) under RFC 6265 rules narrowed for
//! safety: every cookie is host-only (a `Domain` attribute other than the
//! responding host itself is refused, so no public-suffix list is needed and
//! no cookie reaches a sibling or parent host), `Secure` cookies go only
//! over HTTPS, paths follow RFC 6265 path-match. Bounded as v1: 1,000
//! cookies, 4,096 bytes per Set-Cookie, an 8 MiB sealed jar. Session
//! cookies live until the desktop quits; persistent ones are sealed with
//! the profile and expire on their own terms.

use serde::{Deserialize, Serialize};

pub const MAX_COOKIES: usize = 1000;
pub const SET_COOKIE_BYTES: usize = 4096;
pub const JAR_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stored {
    pub name: String,
    pub value: String,
    /// The exact host it was set by (lowercase).
    pub host: String,
    pub path: String,
    pub secure: bool,
    /// Seconds since the Unix epoch; `None` for a session cookie.
    pub expires: Option<i64>,
    /// When it was set (for the request order and eviction).
    pub created: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Jar {
    cookies: Vec<Stored>,
    /// Persistent cookies changed since the jar was last sealed.
    pub dirty: bool,
}

/// RFC 6265 default-path of a request path.
fn default_path(path: &str) -> String {
    match path.rfind('/') {
        Some(0) | None => "/".to_owned(),
        Some(i) => path[..i].to_owned(),
    }
}

fn path_matches(cookie: &str, request: &str) -> bool {
    request == cookie
        || (request.starts_with(cookie)
            && (cookie.ends_with('/') || request[cookie.len()..].starts_with('/')))
}

impl Jar {
    pub fn from_stored(cookies: Vec<Stored>, now: i64) -> Self {
        let mut jar = Self {
            cookies,
            dirty: false,
        };
        jar.cookies.retain(|c| c.expires.is_some_and(|e| e > now));
        jar.cookies.truncate(MAX_COOKIES);
        jar
    }

    pub fn len(&self) -> usize {
        self.cookies.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cookies.is_empty()
    }

    /// The persistent cookies to seal.
    pub fn persistent(&self, now: i64) -> Vec<Stored> {
        self.cookies
            .iter()
            .filter(|c| c.expires.is_some_and(|e| e > now))
            .cloned()
            .collect()
    }

    /// The Cookie header for `url`, if any cookie applies.
    pub fn header(&self, url: &url::Url, now: i64) -> Option<String> {
        let host = url.host_str()?.to_ascii_lowercase();
        let https = url.scheme() == "https";
        let mut matching: Vec<&Stored> = self
            .cookies
            .iter()
            .filter(|c| {
                c.host == host
                    && (!c.secure || https)
                    && c.expires.is_none_or(|e| e > now)
                    && path_matches(&c.path, url.path())
            })
            .collect();
        // Longer paths first, then earlier creation (RFC 6265 5.4).
        matching.sort_by(|a, b| {
            b.path
                .len()
                .cmp(&a.path.len())
                .then(a.created.cmp(&b.created))
        });
        (!matching.is_empty()).then(|| {
            matching
                .iter()
                .map(|c| format!("{}={}", c.name, c.value))
                .collect::<Vec<_>>()
                .join("; ")
        })
    }

    /// Takes the Set-Cookie headers of a response from `url`.
    pub fn store<'a>(&mut self, url: &url::Url, headers: impl Iterator<Item = &'a [u8]>, now: i64) {
        let Some(host) = url.host_str().map(str::to_ascii_lowercase) else {
            return;
        };
        for raw in headers {
            if raw.len() > SET_COOKIE_BYTES {
                continue;
            }
            let Ok(text) = std::str::from_utf8(raw) else {
                continue;
            };
            let Ok(parsed) = cookie::Cookie::parse(text) else {
                continue;
            };
            if parsed.name().is_empty() {
                continue;
            }
            if let Some(domain) = parsed.domain()
                && !domain.trim_start_matches('.').eq_ignore_ascii_case(&host)
            {
                continue;
            }
            if parsed.secure() == Some(true) && url.scheme() != "https" {
                continue;
            }
            let path = parsed
                .path()
                .filter(|p| p.starts_with('/'))
                .map_or_else(|| default_path(url.path()), str::to_owned);
            let expires = match (parsed.max_age(), parsed.expires_datetime()) {
                (Some(age), _) => Some(now.saturating_add(age.whole_seconds())),
                (None, Some(at)) => Some(at.unix_timestamp()),
                (None, None) => None,
            };
            let existing = self
                .cookies
                .iter()
                .position(|c| c.name == parsed.name() && c.host == host && c.path == path);
            let created = existing.map_or(now, |i| self.cookies[i].created);
            if let Some(i) = existing {
                let old = self.cookies.remove(i);
                self.dirty |= old.expires.is_some();
            }
            if expires.is_some_and(|e| e <= now) {
                // An expiry in the past deletes the cookie.
                continue;
            }
            self.dirty |= expires.is_some();
            self.cookies.push(Stored {
                name: parsed.name().to_owned(),
                value: parsed.value().to_owned(),
                host: host.clone(),
                path,
                secure: parsed.secure() == Some(true),
                expires,
                created,
            });
            if self.cookies.len() > MAX_COOKIES {
                // The oldest goes first.
                let oldest = self
                    .cookies
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, c)| c.created)
                    .map(|(i, _)| i);
                if let Some(i) = oldest {
                    self.cookies.remove(i);
                    self.dirty = true;
                }
            }
        }
    }

    pub fn clear(&mut self) {
        self.dirty |= !self.cookies.is_empty();
        self.cookies.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> url::Url {
        url::Url::parse(text).expect("fixture")
    }

    #[test]
    fn cookies_are_host_only_secure_and_path_scoped() {
        let mut jar = Jar::default();
        let headers: [&[u8]; 5] = [
            b"a=1; Path=/api",
            b"b=2; Secure",
            b"c=3; Domain=example.test",
            b"d=4; Domain=api.example.test; Path=/; Max-Age=60",
            b"e=5; Max-Age=0",
        ];
        jar.store(
            &url("http://api.example.test/api/v1/x"),
            headers.into_iter(),
            1000,
        );
        // `b` was refused over http, `c` names a parent domain.
        assert_eq!(jar.len(), 2);
        assert_eq!(
            jar.header(&url("http://api.example.test/api/v1/y"), 1000)
                .as_deref(),
            Some("a=1; d=4")
        );
        assert_eq!(
            jar.header(&url("http://api.example.test/apix"), 1000)
                .as_deref(),
            Some("d=4")
        );
        assert_eq!(
            jar.header(&url("http://other.example.test/api"), 1000),
            None
        );
        assert_eq!(
            jar.header(&url("http://api.example.test/api"), 1061)
                .as_deref(),
            Some("a=1")
        );
        assert!(jar.dirty);
        assert_eq!(jar.persistent(1000).len(), 1);
        // A Secure cookie set over HTTPS never goes back over HTTP.
        jar.store(
            &url("https://api.example.test/"),
            [b"s=1; Secure; Path=/".as_slice()].into_iter(),
            1000,
        );
        assert_eq!(
            jar.header(&url("https://api.example.test/x"), 1000)
                .as_deref(),
            Some("d=4; s=1")
        );
        assert_eq!(
            jar.header(&url("http://api.example.test/x"), 1000)
                .as_deref(),
            Some("d=4")
        );
        let restored = Jar::from_stored(jar.persistent(1000), 1000);
        assert_eq!(
            restored
                .header(&url("http://api.example.test/"), 1000)
                .as_deref(),
            Some("d=4")
        );
    }

    #[test]
    fn the_jar_is_bounded() {
        let mut jar = Jar::default();
        let big = format!("x={}", "v".repeat(SET_COOKIE_BYTES));
        jar.store(&url("http://h.test/"), [big.as_bytes()].into_iter(), 0);
        assert!(jar.is_empty());
        for i in 0..(MAX_COOKIES + 5) {
            let line = format!("n{i}=v");
            jar.store(
                &url("http://h.test/"),
                [line.as_bytes()].into_iter(),
                i as i64,
            );
        }
        assert_eq!(jar.len(), MAX_COOKIES);
        assert!(
            jar.header(&url("http://h.test/"), 0)
                .is_some_and(|h| !h.contains("n0="))
        );
    }
}
