//! Signing in to a workspace MCP server (C22, D049): OAuth 2.1 as the
//! Gateway publishes it, done by the desktop, never by the engine.
//!
//! 1. An unauthenticated `initialize` to the server's catalog URL must be
//!    answered `401` with a `resource_metadata` challenge on the same
//!    origin (RFC 9728); anything else says why no sign-in is offered.
//! 2. The protected-resource metadata must name exactly that URL and an
//!    authorization server on the same HTTPS origin; that server's metadata
//!    (RFC 8414) must name itself, offer the code flow with S256 for a
//!    public client and client registration, and keep every endpoint on
//!    the same origin, so metadata never grants a new destination.
//! 3. A client is registered for each attempt (RFC 7591), the browser is
//!    sent to authorize with PKCE, `state` and `resource` (RFC 8707) and
//!    the loopback callback ([`super::callback`]), and the code is
//!    exchanged. Only the tool scopes are asked for, never a wildcard.
//!
//! Replies are bounded and redirects refused (client policy); tokens are
//! held as [`Zeroizing`] and never logged.

use std::time::Duration;

use base64::Engine as _;
use serde::Serialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use ts_rs::TS;
use url::Url;
use zeroize::Zeroizing;

use super::identity::{bounded_body, https, printable, random, string, transient};
use super::mcp::valid_token;

const REPLY_LIMIT: usize = 64 * 1024;
const TIMEOUT: Duration = Duration::from_secs(30);
/// The scopes asked for: reading the server and listing and calling its
/// tools (intersected with what the server offers).
const WANTED_SCOPES: [&str; 3] = ["mcp:servers:read", "mcp:tools:list", "mcp:tools:call"];
const CLIENT_NAME: &str = "Prisma AIRS Desktop";
const PROTOCOL_VERSION: &str = "2025-06-18";

/// Why signing in to a server, or renewing its token, failed (sanitized).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS, thiserror::Error)]
#[serde(rename_all = "camelCase")]
pub enum McpSignInFailure {
    /// The server answered without asking for a sign-in.
    #[error("the server did not ask for a sign-in")]
    NotOffered,
    #[error("the server refused access")]
    Denied,
    #[error("the server was not found")]
    NotFound,
    #[error("the server is unavailable")]
    Unavailable,
    /// Its metadata named another resource or origin, or lacked the flow.
    #[error("the server's sign-in metadata is not usable")]
    InvalidMetadata,
    #[error("the server does not offer client registration")]
    RegistrationNotOffered,
    #[error("the server could not be reached")]
    Network,
    #[error("the server returned an invalid response")]
    InvalidResponse,
    #[error("the browser did not return in time")]
    TimedOut,
    #[error("sign-in was cancelled")]
    Cancelled,
    #[error("the browser could not be opened")]
    BrowserUnavailable,
    #[error("the sign-in callback could not be opened")]
    CallbackUnavailable,
    /// The token could not be renewed: sign in again.
    #[error("the sign-in expired")]
    SessionExpired,
    #[error("the sign-in could not be kept")]
    StoreFailed,
}

/// A server's sign-in endpoints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpProvider {
    /// The server's canonical catalog URL (the OAuth resource).
    pub resource: String,
    pub issuer: String,
    pub authorization: Url,
    pub token: Url,
    pub registration: Url,
    pub revocation: Option<Url>,
    /// The scopes asked for, or none when the server offers none of them.
    pub scope: Option<String>,
    /// RFC 9207: the callback must then carry `iss`.
    pub iss_in_callback: bool,
}

/// Tokens from an exchange or a refresh.
pub struct McpTokens {
    pub access: Zeroizing<String>,
    pub refresh: Option<Zeroizing<String>>,
    /// Seconds the access token lives, when the server says.
    pub expires_in: Option<u64>,
}

/// One browser attempt.
pub struct McpAttempt {
    pub client_id: String,
    pub state: Zeroizing<String>,
    pub verifier: Zeroizing<String>,
    pub redirect: String,
    pub url: Url,
}

fn origin(url: &Url) -> String {
    url.origin().ascii_serialization()
}

/// `resource_metadata="..."` of a `Bearer` challenge.
fn resource_metadata(header: &str) -> Option<String> {
    let rest = header.trim().strip_prefix("Bearer")?.trim();
    for part in rest.split(',') {
        let (k, v) = part.trim().split_once('=')?;
        if k.trim().eq_ignore_ascii_case("resource_metadata") {
            let v = v.trim();
            let v = v
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .unwrap_or(v);
            return Some(v.to_owned());
        }
    }
    None
}

/// RFC 8414 §3: the well-known path is inserted before the issuer's path.
fn metadata_url(issuer: &Url) -> Option<Url> {
    let path = issuer.path().trim_end_matches('/');
    let mut url = issuer.clone();
    url.set_path(&format!("/.well-known/oauth-authorization-server{path}"));
    Some(url)
}

fn listed(body: &Map<String, Value>, key: &str, want: &str) -> bool {
    body.get(key)
        .and_then(Value::as_array)
        .is_some_and(|items| items.iter().any(|v| v.as_str() == Some(want)))
}

/// The MCP sign-in endpoints over the app-owned HTTP client.
#[derive(Clone)]
pub struct McpAuthClient {
    http: reqwest::Client,
}

impl McpAuthClient {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }

    async fn json(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<(u16, Map<String, Value>), McpSignInFailure> {
        let response = request
            .timeout(TIMEOUT)
            .header("accept", "application/json")
            .send()
            .await
            .map_err(|_| McpSignInFailure::Network)?;
        let status = response.status().as_u16();
        if (300..400).contains(&status) {
            return Err(McpSignInFailure::InvalidResponse);
        }
        let invalid = if transient(status) {
            McpSignInFailure::Network
        } else {
            McpSignInFailure::InvalidResponse
        };
        let body = bounded_body(response, REPLY_LIMIT)
            .await
            .map_err(|()| invalid)?;
        if body.iter().all(u8::is_ascii_whitespace) {
            return Ok((status, Map::new()));
        }
        match serde_json::from_slice::<Value>(&body) {
            Ok(Value::Object(map)) => Ok((status, map)),
            _ => Err(invalid),
        }
    }

    async fn form(
        &self,
        url: &Url,
        values: &[(&str, &str)],
    ) -> Result<(u16, Map<String, Value>), McpSignInFailure> {
        let body = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(values)
            .finish();
        self.json(
            self.http
                .post(url.clone())
                .header("content-type", "application/x-www-form-urlencoded")
                .body(body),
        )
        .await
    }

    /// Step 1: the server's own challenge names its metadata.
    async fn challenge(&self, resource: &Url) -> Result<Url, McpSignInFailure> {
        let body = json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
            "protocolVersion": PROTOCOL_VERSION, "capabilities": {},
            "clientInfo": {"name": "prisma-airs-desktop", "version": "2"}}});
        let response = self
            .http
            .post(resource.clone())
            .timeout(TIMEOUT)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .map_err(|_| McpSignInFailure::Network)?;
        let status = response.status().as_u16();
        let header = response
            .headers()
            .get("www-authenticate")
            .and_then(|h| h.to_str().ok())
            .map(str::to_owned);
        drop(response);
        match status {
            401 => {}
            200..=299 => return Err(McpSignInFailure::NotOffered),
            403 => return Err(McpSignInFailure::Denied),
            404 => return Err(McpSignInFailure::NotFound),
            s if transient(s) => return Err(McpSignInFailure::Network),
            _ => return Err(McpSignInFailure::Unavailable),
        }
        header
            .as_deref()
            .and_then(resource_metadata)
            .and_then(|m| https(&m))
            .filter(|m| origin(m) == origin(resource))
            .ok_or(McpSignInFailure::InvalidMetadata)
    }

    /// Steps 1-2: the server's sign-in endpoints, all on its own origin.
    pub async fn discover(&self, resource: &str) -> Result<McpProvider, McpSignInFailure> {
        let invalid = McpSignInFailure::InvalidMetadata;
        let resource_url = https(resource).ok_or(invalid)?;
        let metadata = self.challenge(&resource_url).await?;
        let (status, prm) = self.json(self.http.get(metadata)).await?;
        if transient(status) {
            return Err(McpSignInFailure::Network);
        }
        let named = string(prm.get("resource"), 2048).and_then(super::mcp::canonical_url);
        if status != 200 || named.as_deref() != Some(resource) {
            return Err(invalid);
        }
        let issuer = prm
            .get("authorization_servers")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(Value::as_str)
            .filter(|s| s.len() <= 2048)
            .and_then(https)
            .filter(|u| origin(u) == origin(&resource_url) && u.query().is_none())
            .ok_or(invalid)?;
        let (status, body) = self
            .json(self.http.get(metadata_url(&issuer).ok_or(invalid)?))
            .await?;
        if transient(status) {
            return Err(McpSignInFailure::Network);
        }
        let issuer_text = issuer.as_str().trim_end_matches('/');
        let named = string(body.get("issuer"), 2048).map(|s| s.trim_end_matches('/'));
        if status != 200 || named != Some(issuer_text) {
            return Err(invalid);
        }
        if !listed(&body, "response_types_supported", "code")
            || !listed(&body, "code_challenge_methods_supported", "S256")
            || !listed(&body, "token_endpoint_auth_methods_supported", "none")
        {
            return Err(invalid);
        }
        let endpoint = |key: &str| {
            string(body.get(key), 2048)
                .and_then(https)
                .filter(|u| origin(u) == origin(&resource_url))
        };
        let registration = match body.get("registration_endpoint") {
            None | Some(Value::Null) => return Err(McpSignInFailure::RegistrationNotOffered),
            Some(_) => endpoint("registration_endpoint").ok_or(invalid)?,
        };
        let scopes: Vec<&str> = WANTED_SCOPES
            .iter()
            .copied()
            .filter(|s| listed(&body, "scopes_supported", s))
            .collect();
        Ok(McpProvider {
            resource: resource.to_owned(),
            issuer: issuer_text.to_owned(),
            authorization: endpoint("authorization_endpoint").ok_or(invalid)?,
            token: endpoint("token_endpoint").ok_or(invalid)?,
            registration,
            revocation: match body.get("revocation_endpoint") {
                None | Some(Value::Null) => None,
                Some(_) => Some(endpoint("revocation_endpoint").ok_or(invalid)?),
            },
            scope: (!scopes.is_empty()).then(|| scopes.join(" ")),
            iss_in_callback: body.get("authorization_response_iss_parameter_supported")
                == Some(&Value::Bool(true)),
        })
    }

    /// Step 3: a public client for this attempt's loopback redirect.
    pub async fn register(
        &self,
        provider: &McpProvider,
        redirect: &str,
    ) -> Result<String, McpSignInFailure> {
        let body = json!({"client_name": CLIENT_NAME, "redirect_uris": [redirect],
            "grant_types": ["authorization_code", "refresh_token"], "response_types": ["code"],
            "token_endpoint_auth_method": "none"});
        let (status, reply) = self
            .json(
                self.http
                    .post(provider.registration.clone())
                    .header("content-type", "application/json")
                    .body(body.to_string()),
            )
            .await?;
        if transient(status) {
            return Err(McpSignInFailure::Network);
        }
        if !(200..300).contains(&status) {
            return Err(McpSignInFailure::Denied);
        }
        string(reply.get("client_id"), 256)
            .filter(|c| printable(c, 256))
            .map(str::to_owned)
            .ok_or(McpSignInFailure::InvalidResponse)
    }

    fn tokens(status: u16, body: &Map<String, Value>) -> Result<McpTokens, McpSignInFailure> {
        if !(200..300).contains(&status) {
            return Err(match (status, string(body.get("error"), 128)) {
                (_, Some("invalid_grant" | "access_denied" | "unauthorized_client")) => {
                    McpSignInFailure::Denied
                }
                (s, _) if transient(s) => McpSignInFailure::Network,
                _ => McpSignInFailure::InvalidResponse,
            });
        }
        if !string(body.get("token_type"), 32).is_some_and(|t| t.eq_ignore_ascii_case("bearer")) {
            return Err(McpSignInFailure::InvalidResponse);
        }
        let access = string(body.get("access_token"), super::mcp::TOKEN_MAX)
            .filter(|t| valid_token(t))
            .map(|t| Zeroizing::new(t.to_owned()))
            .ok_or(McpSignInFailure::InvalidResponse)?;
        let refresh = match body.get("refresh_token") {
            None | Some(Value::Null) => None,
            v => Some(
                string(v, super::mcp::TOKEN_MAX)
                    .filter(|t| t.bytes().all(|b| b.is_ascii_graphic()))
                    .map(|t| Zeroizing::new(t.to_owned()))
                    .ok_or(McpSignInFailure::InvalidResponse)?,
            ),
        };
        let expires_in = match body.get("expires_in") {
            None | Some(Value::Null) => None,
            Some(v) => Some(
                v.as_u64()
                    .filter(|&s| s > 0 && s <= 366 * 24 * 3600)
                    .ok_or(McpSignInFailure::InvalidResponse)?,
            ),
        };
        Ok(McpTokens {
            access,
            refresh,
            expires_in,
        })
    }

    /// Exchanges the callback's code.
    pub async fn exchange(
        &self,
        provider: &McpProvider,
        attempt: &McpAttempt,
        code: &str,
    ) -> Result<McpTokens, McpSignInFailure> {
        let (status, body) = self
            .form(
                &provider.token,
                &[
                    ("grant_type", "authorization_code"),
                    ("client_id", &attempt.client_id),
                    ("code", code),
                    ("redirect_uri", &attempt.redirect),
                    ("code_verifier", &attempt.verifier),
                    ("resource", &provider.resource),
                ],
            )
            .await?;
        Self::tokens(status, &body)
    }

    /// Renews the access token. A refused refresh means signing in again.
    pub async fn refresh(
        &self,
        token_endpoint: &Url,
        client_id: &str,
        resource: &str,
        refresh_token: &str,
    ) -> Result<McpTokens, McpSignInFailure> {
        let (status, body) = self
            .form(
                token_endpoint,
                &[
                    ("grant_type", "refresh_token"),
                    ("client_id", client_id),
                    ("refresh_token", refresh_token),
                    ("resource", resource),
                ],
            )
            .await?;
        Self::tokens(status, &body).map_err(|failure| match failure {
            McpSignInFailure::Denied => McpSignInFailure::SessionExpired,
            other => other,
        })
    }

    /// Revokes a token (best effort, after it is gone locally).
    pub async fn revoke(
        &self,
        revocation: &Url,
        client_id: &str,
        token: &str,
        hint: &str,
    ) -> Result<(), McpSignInFailure> {
        let (status, _) = self
            .form(
                revocation,
                &[
                    ("token", token),
                    ("token_type_hint", hint),
                    ("client_id", client_id),
                ],
            )
            .await?;
        if (200..300).contains(&status) {
            Ok(())
        } else {
            Err(McpSignInFailure::InvalidResponse)
        }
    }
}

/// Starts an attempt for `client_id` whose callback is
/// `http://127.0.0.1:{port}/callback`.
pub fn attempt(provider: &McpProvider, client_id: &str, port: u16) -> Option<McpAttempt> {
    let state = random(32)?;
    let verifier = random(48)?;
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(verifier.as_bytes()));
    let redirect = format!("http://127.0.0.1:{port}/callback");
    let mut url = provider.authorization.clone();
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("response_type", "code")
            .append_pair("client_id", client_id)
            .append_pair("redirect_uri", &redirect)
            .append_pair("state", &state)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("resource", &provider.resource);
        if let Some(scope) = &provider.scope {
            q.append_pair("scope", scope);
        }
    }
    Some(McpAttempt {
        client_id: client_id.to_owned(),
        state,
        verifier,
        redirect,
        url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_challenge_names_its_metadata() {
        assert_eq!(
            resource_metadata(r#"Bearer error="invalid_token", resource_metadata="https://gw.example/.well-known/oauth-protected-resource/t/mcp""#)
                .as_deref(),
            Some("https://gw.example/.well-known/oauth-protected-resource/t/mcp")
        );
        assert_eq!(resource_metadata(r#"Basic realm="x""#), None);
        assert_eq!(resource_metadata(r#"Bearer error="invalid_token""#), None);
    }

    #[test]
    fn the_metadata_path_is_inserted_before_the_issuers() {
        let issuer = Url::parse("https://gw.example/oauth/team").expect("url");
        assert_eq!(
            metadata_url(&issuer).expect("url").as_str(),
            "https://gw.example/.well-known/oauth-authorization-server/oauth/team"
        );
        let bare = Url::parse("https://gw.example/").expect("url");
        assert_eq!(
            metadata_url(&bare).expect("url").as_str(),
            "https://gw.example/.well-known/oauth-authorization-server"
        );
    }

    #[test]
    fn an_attempt_carries_pkce_state_resource_and_only_the_tool_scopes() {
        let provider = McpProvider {
            resource: "https://gw.example/t/mcp".into(),
            issuer: "https://gw.example".into(),
            authorization: Url::parse("https://gw.example/oauth/t/authorize").expect("url"),
            token: Url::parse("https://gw.example/oauth/token").expect("url"),
            registration: Url::parse("https://gw.example/oauth/register").expect("url"),
            revocation: None,
            scope: Some("mcp:tools:list mcp:tools:call".into()),
            iss_in_callback: false,
        };
        let a = attempt(&provider, "client-1", 4321).expect("attempt");
        let q: std::collections::HashMap<String, String> =
            a.url.query_pairs().into_owned().collect();
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["client_id"], "client-1");
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:4321/callback");
        assert_eq!(q["resource"], "https://gw.example/t/mcp");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["scope"], "mcp:tools:list mcp:tools:call");
        let digest = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(Sha256::digest(a.verifier.as_bytes()));
        assert_eq!(q["code_challenge"], digest);
        assert!(a.state.len() >= 43);
        assert!(!a.url.as_str().contains('*'));
    }

    #[test]
    fn tokens_must_be_bearer_and_fit_the_engine() {
        let ok = json!({"token_type": "Bearer", "access_token": "a".repeat(32),
            "refresh_token": "r".repeat(20), "expires_in": 3600});
        let t = McpAuthClient::tokens(200, ok.as_object().expect("map")).expect("tokens");
        assert_eq!(t.expires_in, Some(3600));
        for bad in [
            json!({"token_type": "mac", "access_token": "a".repeat(32)}),
            json!({"token_type": "Bearer", "access_token": "short"}),
            json!({"token_type": "Bearer", "access_token": "a b c d e f g h i j"}),
            json!({"token_type": "Bearer", "access_token": "a".repeat(32), "expires_in": -1}),
        ] {
            assert!(
                McpAuthClient::tokens(200, bad.as_object().expect("map")).is_err(),
                "{bad}"
            );
        }
        let denied = json!({"error": "invalid_grant"});
        assert!(matches!(
            McpAuthClient::tokens(400, denied.as_object().expect("map")),
            Err(McpSignInFailure::Denied)
        ));
    }
}
