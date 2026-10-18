//! Academy identity and learner transport over the app-owned HTTP client.
//!
//! Sign-in is OAuth 2.0 device authorization (RFC 8628) against the Clerk
//! issuer the Academy server names in its public `/api/desktop/config`; the
//! desktop is a public client (no secret). Every endpoint is discovered from
//! the issuer's RFC 8414 metadata and must be HTTPS on the issuer's own
//! origin. Responses are bounded, redirects are refused (client policy) and
//! transient provider failures read as `Network` so saved work stays usable.
//!
//! Learner calls are single tRPC procedure requests (never batched) to the
//! Academy origin for a pinned allowlist of learner procedures and their
//! kinds ([`crate::learner`]). Inputs and outputs stay in their SuperJSON
//! wire form: the presentation's tRPC client serializes and deserializes;
//! Rust validates the envelope, screens server-private fields and never
//! revives values.

use std::time::Duration;

use serde::Serialize;
use serde_json::{Map, Value};
use ts_rs::TS;
use url::Url;
use zeroize::Zeroizing;

use crate::learner::ProcedureKind;

/// The Academy scope a desktop token must carry.
pub const REQUIRED_SCOPE: &str = "academy:learn";
const AUTH_RESPONSE_LIMIT: usize = 128 * 1024;
/// Largest learner reply accepted (lesson and bundle projections).
pub const CALL_RESPONSE_LIMIT: usize = 16 * 1024 * 1024;
/// Largest serialized learner input accepted.
pub const CALL_INPUT_LIMIT: usize = 1024 * 1024;
const AUTH_TIMEOUT: Duration = Duration::from_secs(30);
/// Keys whose presence means a server-private assessment projection leaked.
const PRIVATE_KEYS: [&str; 5] = ["answerKey", "serverSpec", "rubric", "goldens", "evaluation"];

/// Why an Academy sign-in step failed (sanitized; no provider text).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS, thiserror::Error)]
#[serde(rename_all = "camelCase")]
pub enum AcademyFailure {
    #[error("Academy desktop sign-in is not configured on this server")]
    NotConfigured,
    #[error("the Academy sign-in configuration is invalid")]
    InvalidConfig,
    #[error("Academy could not be reached")]
    Network,
    #[error("Academy returned an invalid response")]
    InvalidResponse,
    #[error("sign-in was declined")]
    Denied,
    #[error("the sign-in code expired")]
    Expired,
    #[error("the Academy session expired")]
    SessionExpired,
    #[error("Academy did not grant learning access")]
    ScopeMissing,
    #[error("the Academy sign-in configuration changed")]
    AccountChanged,
    #[error("Academy sign-in could not complete")]
    AuthFailed,
}

/// The public desktop client registration Academy publishes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopConfig {
    pub issuer: String,
    pub client_id: String,
    pub scope: String,
}

/// Issuer endpoints from RFC 8414 metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoints {
    pub device_authorization: Url,
    pub token: Url,
    pub revocation: Url,
}

/// A started device authorization.
pub struct DeviceStart {
    pub device_code: Zeroizing<String>,
    pub user_code: String,
    pub verification_uri: Url,
    pub verification_uri_complete: Option<Url>,
    pub expires_in: Duration,
    pub interval: Duration,
}

/// Tokens from a grant or refresh.
pub struct Tokens {
    pub access: Zeroizing<String>,
    pub refresh: Zeroizing<String>,
    pub expires_in: Duration,
}

/// One device-code poll.
pub enum Poll {
    Pending,
    SlowDown,
    Granted(Tokens),
}

/// What a learner call produced, in wire form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallReply {
    /// `result.data` exactly as sent (SuperJSON `{json, meta?}`), screened.
    Data(String),
    /// A tRPC error envelope (`error`), with the HTTP status.
    Error { error: String, status: u16 },
}

/// What a presigned avatar PUT is known to have done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreOutcome {
    /// The store accepted the bytes.
    Stored,
    /// The store refused them (4xx): nothing was stored.
    Refused,
    /// No connection was made: nothing was sent.
    NotSent,
    /// Sent, but the reply was an error or never arrived: the bytes may be
    /// stored.
    Uncertain,
}

/// Why a learner call produced no tRPC reply.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum CallFailure {
    /// Transport failure (the request may or may not have been processed).
    Network,
    /// The service did not answer in time.
    Timeout,
    /// The reply was not a tRPC envelope, was too large, redirected, or
    /// carried a server-private assessment field.
    InvalidResponse,
    /// The access token was refused and could not be renewed.
    SessionExpired,
    /// No usable credential (signed out, vault failure).
    CredentialUnavailable,
    /// Stopped by the operator or a learner/account change, or a native
    /// sheet the operator dismissed.
    Cancelled,
    /// A native sheet (print, image picker) could not open.
    HostUnavailable,
    /// The chosen photo is not a complete JPEG, PNG or WebP of at most
    /// 16 MiB and 8192 pixels a side.
    UnsupportedImage,
    /// Academy named an upload destination outside the reviewed set.
    DestinationRefused,
    /// The photo was not stored, or its store is not known to have taken
    /// it (the call's outcome says which).
    UploadFailed,
    /// Offline work storage could not be opened or written; nothing was
    /// written in plaintext and saved work was kept.
    OfflineUnavailable,
    /// Academy does not give this account the course to download.
    NotDownloadable,
    /// The item does not fit in this account's offline storage.
    OfflineFull,
}

fn https(text: &str) -> Option<Url> {
    let url = Url::parse(text).ok()?;
    (url.scheme() == "https"
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none())
    .then_some(url)
}

/// An `https://host[:port]` origin with no path, query or credentials.
pub fn origin_of(text: &str) -> Option<String> {
    let url = https(text)?;
    (url.path() == "/" && url.query().is_none()).then(|| url.origin().ascii_serialization())
}

fn string(value: &Value, max: usize) -> Option<&str> {
    value.as_str().filter(|s| !s.is_empty() && s.len() <= max)
}

fn seconds(value: Option<&Value>, fallback: Option<u64>) -> Option<Duration> {
    let n = match value {
        None => fallback?,
        Some(v) => v.as_u64()?,
    };
    (1..=31_536_000)
        .contains(&n)
        .then(|| Duration::from_secs(n))
}

fn form(values: &[(&str, &str)]) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(values)
        .finish()
}

/// Reads a whole body up to `limit` bytes.
async fn bounded_body(response: reqwest::Response, limit: usize) -> Result<Vec<u8>, ()> {
    use futures_util::StreamExt;
    if response.content_length().is_some_and(|n| n > limit as u64) {
        return Err(());
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| ())?;
        if body.len() + chunk.len() > limit {
            return Err(());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn transient(status: u16) -> bool {
    matches!(status, 500 | 502 | 503 | 504)
}

/// The Academy service for one transport policy revision.
#[derive(Clone)]
pub struct AcademyClient {
    http: reqwest::Client,
    origin: String,
}

impl AcademyClient {
    pub fn new(http: reqwest::Client, origin: &str) -> Option<Self> {
        Some(Self {
            http,
            origin: origin_of(origin)?,
        })
    }

    pub fn origin(&self) -> &str {
        &self.origin
    }

    async fn auth_json(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<(u16, Map<String, Value>), AcademyFailure> {
        let response = request
            .timeout(AUTH_TIMEOUT)
            .header("accept", "application/json")
            .send()
            .await
            .map_err(|_| AcademyFailure::Network)?;
        let status = response.status().as_u16();
        if (300..400).contains(&status) {
            return Err(AcademyFailure::InvalidResponse);
        }
        let body = bounded_body(response, AUTH_RESPONSE_LIMIT).await;
        let body = match body {
            Ok(body) => body,
            Err(()) if transient(status) => return Err(AcademyFailure::Network),
            Err(()) => return Err(AcademyFailure::InvalidResponse),
        };
        // Revocation may answer with an empty success.
        let parsed = if body.iter().all(u8::is_ascii_whitespace) {
            Value::Object(Map::new())
        } else {
            match serde_json::from_slice::<Value>(&body) {
                Ok(v) => v,
                Err(_) if transient(status) => return Err(AcademyFailure::Network),
                Err(_) => return Err(AcademyFailure::InvalidResponse),
            }
        };
        match parsed {
            Value::Object(map) => Ok((status, map)),
            _ if transient(status) => Err(AcademyFailure::Network),
            _ => Err(AcademyFailure::InvalidResponse),
        }
    }

    /// The Academy-published desktop client registration.
    pub async fn config(&self) -> Result<DesktopConfig, AcademyFailure> {
        let (status, body) = self
            .auth_json(self.http.get(format!("{}/api/desktop/config", self.origin)))
            .await?;
        // The documented 503 (not configured) carries this exact body; other
        // transient failures stay Network.
        if status == 503
            && body.get("error").and_then(Value::as_str)
                == Some("Desktop sign-in is not configured.")
        {
            return Err(AcademyFailure::NotConfigured);
        }
        if transient(status) {
            return Err(AcademyFailure::Network);
        }
        if status != 200 {
            return Err(AcademyFailure::NotConfigured);
        }
        let issuer = body
            .get("issuer")
            .and_then(|v| string(v, 2048))
            .and_then(origin_of)
            .ok_or(AcademyFailure::InvalidConfig)?;
        let client_id = body
            .get("clientId")
            .and_then(|v| string(v, 256))
            .filter(|s| {
                s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            })
            .ok_or(AcademyFailure::InvalidConfig)?;
        let scope = body
            .get("scope")
            .and_then(|v| string(v, 1024))
            .ok_or(AcademyFailure::InvalidConfig)?;
        let scopes: Vec<&str> = scope.split(' ').collect();
        if !scopes.contains(&REQUIRED_SCOPE) || !scopes.contains(&"offline_access") {
            return Err(AcademyFailure::InvalidConfig);
        }
        Ok(DesktopConfig {
            issuer,
            client_id: client_id.to_owned(),
            scope: scope.to_owned(),
        })
    }

    /// The issuer's endpoints; each must be HTTPS on the issuer origin.
    pub async fn endpoints(&self, config: &DesktopConfig) -> Result<Endpoints, AcademyFailure> {
        let (status, body) = self
            .auth_json(self.http.get(format!(
                "{}/.well-known/oauth-authorization-server",
                config.issuer
            )))
            .await?;
        if transient(status) {
            return Err(AcademyFailure::Network);
        }
        if status != 200
            || body.get("issuer").and_then(Value::as_str) != Some(config.issuer.as_str())
        {
            return Err(AcademyFailure::InvalidConfig);
        }
        let endpoint = |key: &str| {
            body.get(key)
                .and_then(|v| string(v, 2048))
                .and_then(https)
                .filter(|u| u.origin().ascii_serialization() == config.issuer)
                .ok_or(AcademyFailure::InvalidConfig)
        };
        Ok(Endpoints {
            device_authorization: endpoint("device_authorization_endpoint")?,
            token: endpoint("token_endpoint")?,
            revocation: endpoint("revocation_endpoint")?,
        })
    }

    async fn post_form(
        &self,
        url: &Url,
        values: &[(&str, &str)],
    ) -> Result<(u16, Map<String, Value>), AcademyFailure> {
        self.auth_json(
            self.http
                .post(url.clone())
                .header("content-type", "application/x-www-form-urlencoded")
                .body(form(values)),
        )
        .await
    }

    /// Starts a device authorization.
    pub async fn start(
        &self,
        config: &DesktopConfig,
        endpoints: &Endpoints,
    ) -> Result<DeviceStart, AcademyFailure> {
        let (status, body) = self
            .post_form(
                &endpoints.device_authorization,
                &[("client_id", &config.client_id), ("scope", &config.scope)],
            )
            .await?;
        if !(200..300).contains(&status) {
            return Err(error_failure(&body, status));
        }
        let invalid = AcademyFailure::InvalidResponse;
        Ok(DeviceStart {
            device_code: Zeroizing::new(
                body.get("device_code")
                    .and_then(|v| string(v, 32_768))
                    .ok_or(invalid)?
                    .to_owned(),
            ),
            user_code: body
                .get("user_code")
                .and_then(|v| string(v, 64))
                .filter(|s| {
                    s.chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == ' ')
                })
                .ok_or(invalid)?
                .to_owned(),
            verification_uri: body
                .get("verification_uri")
                .and_then(|v| string(v, 2048))
                .and_then(https)
                .ok_or(invalid)?,
            verification_uri_complete: match body.get("verification_uri_complete") {
                None | Some(Value::Null) => None,
                Some(v) => Some(string(v, 2048).and_then(https).ok_or(invalid)?),
            },
            expires_in: seconds(body.get("expires_in"), None).ok_or(invalid)?,
            interval: seconds(body.get("interval"), Some(5)).ok_or(invalid)?,
        })
    }

    /// One device-code poll (RFC 8628 §3.4–3.5).
    pub async fn poll(
        &self,
        config: &DesktopConfig,
        endpoints: &Endpoints,
        device_code: &str,
    ) -> Result<Poll, AcademyFailure> {
        let (status, body) = self
            .post_form(
                &endpoints.token,
                &[
                    ("client_id", &config.client_id),
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ("device_code", device_code),
                ],
            )
            .await?;
        if status == 400 {
            match body.get("error").and_then(Value::as_str) {
                Some("authorization_pending") => return Ok(Poll::Pending),
                Some("slow_down") => return Ok(Poll::SlowDown),
                _ => {}
            }
        }
        if !(200..300).contains(&status) {
            return Err(error_failure(&body, status));
        }
        tokens(&body, None).map(Poll::Granted)
    }

    /// Renews the access token.
    pub async fn refresh(
        &self,
        config: &DesktopConfig,
        endpoints: &Endpoints,
        refresh_token: &str,
    ) -> Result<Tokens, AcademyFailure> {
        let (status, body) = self
            .post_form(
                &endpoints.token,
                &[
                    ("client_id", &config.client_id),
                    ("grant_type", "refresh_token"),
                    ("refresh_token", refresh_token),
                ],
            )
            .await?;
        if !(200..300).contains(&status) {
            return Err(error_failure(&body, status));
        }
        tokens(&body, Some(refresh_token))
    }

    /// Revokes a refresh token (best effort by the caller).
    pub async fn revoke(
        &self,
        config: &DesktopConfig,
        endpoints: &Endpoints,
        refresh_token: &str,
    ) -> Result<(), AcademyFailure> {
        let (status, body) = self
            .post_form(
                &endpoints.revocation,
                &[
                    ("client_id", &config.client_id),
                    ("token", refresh_token),
                    ("token_type_hint", "refresh_token"),
                ],
            )
            .await?;
        if (200..300).contains(&status) {
            Ok(())
        } else {
            Err(error_failure(&body, status))
        }
    }

    /// One learner procedure request. Queries use GET with the serialized
    /// input in `input`; mutations POST it as the JSON body.
    pub async fn call(
        &self,
        procedure: &str,
        kind: ProcedureKind,
        input: Option<&str>,
        access_token: Option<&str>,
        timeout: Duration,
    ) -> Result<CallReply, CallFailure> {
        let mut url = Url::parse(&format!("{}/api/trpc/{procedure}", self.origin))
            .map_err(|_| CallFailure::InvalidResponse)?;
        let request = match kind {
            ProcedureKind::Query => {
                if let Some(input) = input {
                    url.query_pairs_mut().append_pair("input", input);
                }
                self.http.get(url)
            }
            ProcedureKind::Mutation => self
                .http
                .post(url)
                .header("content-type", "application/json")
                .body(input.unwrap_or("{}").to_owned()),
        };
        let request = request
            .timeout(timeout)
            .header("accept", "application/json");
        // Public procedures are sent with no credential at all.
        let request = match access_token {
            Some(token) => request.bearer_auth(token),
            None => request,
        };
        let response = request.send().await.map_err(|e| {
            if e.is_timeout() {
                CallFailure::Timeout
            } else {
                CallFailure::Network
            }
        })?;
        let status = response.status().as_u16();
        if (300..400).contains(&status) {
            return Err(CallFailure::InvalidResponse);
        }
        let body = bounded_body(response, CALL_RESPONSE_LIMIT)
            .await
            .map_err(|()| CallFailure::InvalidResponse)?;
        let envelope: Value =
            serde_json::from_slice(&body).map_err(|_| CallFailure::InvalidResponse)?;
        let Value::Object(mut envelope) = envelope else {
            return Err(CallFailure::InvalidResponse);
        };
        if let Some(mut error) = envelope.remove("error") {
            if !error.is_object() || (200..300).contains(&status) {
                return Err(CallFailure::InvalidResponse);
            }
            // Error envelopes reach the renderer too: the same private-field
            // boundary applies (no drill exception for errors).
            screen("", &mut error)?;
            return Ok(CallReply::Error {
                error: error.to_string(),
                status,
            });
        }
        if !(200..300).contains(&status) {
            return Err(CallFailure::InvalidResponse);
        }
        let mut data = envelope
            .remove("result")
            .and_then(|mut r| r.as_object_mut().and_then(|r| r.remove("data")))
            .filter(|d| d.as_object().is_some_and(|d| d.contains_key("json")))
            .ok_or(CallFailure::InvalidResponse)?;
        screen(procedure, &mut data)?;
        Ok(CallReply::Data(data.to_string()))
    }
}

impl AcademyClient {
    /// Stores an avatar at a presigned upload destination: a single PUT of
    /// the bytes with their content type and no credential (the signature
    /// is in the URL). The caller has checked the destination. A refusal
    /// (4xx) stored nothing; a request that never connected was not sent;
    /// anything else after sending (5xx, a lost reply, a timeout) may have
    /// stored the bytes.
    pub async fn put_upload(
        &self,
        url: Url,
        content_type: &str,
        bytes: Vec<u8>,
        timeout: Duration,
    ) -> StoreOutcome {
        let sent = self
            .http
            .put(url)
            .timeout(timeout)
            .header("content-type", content_type)
            .body(bytes)
            .send()
            .await;
        match sent {
            Ok(response) if response.status().is_success() => StoreOutcome::Stored,
            Ok(response) if response.status().is_client_error() => StoreOutcome::Refused,
            Ok(_) => StoreOutcome::Uncertain,
            Err(error) if error.is_connect() => StoreOutcome::NotSent,
            Err(_) => StoreOutcome::Uncertain,
        }
    }

    /// Reads one public Academy image without a credential: its declared
    /// content type and at most `limit` bytes. The caller has checked the
    /// address.
    pub async fn get_image(
        &self,
        url: Url,
        limit: usize,
        timeout: Duration,
    ) -> Result<(String, Vec<u8>), CallFailure> {
        let response = self
            .http
            .get(url)
            .timeout(timeout)
            .header("accept", "image/jpeg, image/png, image/webp")
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    CallFailure::Timeout
                } else {
                    CallFailure::Network
                }
            })?;
        if response.status().as_u16() != 200 {
            return Err(CallFailure::InvalidResponse);
        }
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|v| {
                v.split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_ascii_lowercase()
            })
            .unwrap_or_default();
        let body = bounded_body(response, limit)
            .await
            .map_err(|()| CallFailure::InvalidResponse)?;
        Ok((content_type, body))
    }
}

fn error_failure(body: &Map<String, Value>, status: u16) -> AcademyFailure {
    if transient(status) {
        return AcademyFailure::Network;
    }
    match body.get("error").and_then(Value::as_str) {
        Some("access_denied") => AcademyFailure::Denied,
        Some("expired_token") => AcademyFailure::Expired,
        Some("invalid_grant") => AcademyFailure::SessionExpired,
        _ => AcademyFailure::AuthFailed,
    }
}

fn tokens(
    body: &Map<String, Value>,
    previous_refresh: Option<&str>,
) -> Result<Tokens, AcademyFailure> {
    let invalid = AcademyFailure::InvalidResponse;
    if !body
        .get("token_type")
        .and_then(Value::as_str)
        .is_some_and(|t| t.eq_ignore_ascii_case("bearer"))
    {
        return Err(invalid);
    }
    if let Some(scope) = body.get("scope") {
        let scope = string(scope, 1024).ok_or(invalid)?;
        if !scope.split(' ').any(|s| s == REQUIRED_SCOPE) {
            return Err(AcademyFailure::ScopeMissing);
        }
    }
    let access = body
        .get("access_token")
        .and_then(|v| string(v, 32_768))
        .ok_or(invalid)?;
    let refresh = match body.get("refresh_token") {
        Some(v) => string(v, 32_768).ok_or(invalid)?,
        None => previous_refresh.ok_or(invalid)?,
    };
    Ok(Tokens {
        access: Zeroizing::new(access.to_owned()),
        refresh: Zeroizing::new(refresh.to_owned()),
        expires_in: seconds(body.get("expires_in"), None).ok_or(invalid)?,
    })
}

/// Rejects server-private assessment fields anywhere in a reply. The one
/// authored exception is the orientation drill, whose practice answers are
/// public to the learner by design (`orientation.get` → `script.alerts[*]`).
fn screen(procedure: &str, data: &mut Value) -> Result<(), CallFailure> {
    fn walk(value: &Value, depth: usize, allow_answer_key: bool) -> bool {
        if depth > 128 {
            return false;
        }
        match value {
            Value::Array(items) => items.iter().all(|i| walk(i, depth + 1, false)),
            Value::Object(map) => map.iter().all(|(key, child)| {
                let private = PRIVATE_KEYS.contains(&key.as_str());
                (!private || (allow_answer_key && key == "answerKey" && child.is_u64()))
                    && walk(child, depth + 1, false)
            }),
            _ => true,
        }
    }
    let json = data.get("json").ok_or(CallFailure::InvalidResponse)?;
    let ok = if procedure == "orientation.get" {
        // Only the drill's alert entries may carry a numeric answerKey.
        let alerts = json.pointer("/script/alerts").and_then(Value::as_array);
        let mut clean = json.clone();
        if let Some(list) = clean
            .pointer_mut("/script/alerts")
            .and_then(Value::as_array_mut)
        {
            list.clear();
        }
        walk(&clean, 0, false) && alerts.is_none_or(|a| a.iter().all(|alert| walk(alert, 0, true)))
    } else {
        walk(json, 0, false)
    };
    if ok {
        Ok(())
    } else {
        Err(CallFailure::InvalidResponse)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn origins_are_bare_https() {
        assert_eq!(
            origin_of("https://academy.cdot.io"),
            Some("https://academy.cdot.io".into())
        );
        assert_eq!(
            origin_of("https://academy.cdot.io/"),
            Some("https://academy.cdot.io".into())
        );
        for bad in [
            "http://academy.cdot.io",
            "https://a.io/path",
            "https://u:p@a.io",
            "https://a.io/?q",
            "https://a.io/#f",
        ] {
            assert_eq!(origin_of(bad), None, "{bad}");
        }
    }

    #[test]
    fn private_fields_are_screened_except_the_orientation_drill() {
        let mut leak = json!({"json": {"unit": {"steps": [{"answerKey": 2}]}}});
        assert_eq!(
            screen("units.get", &mut leak),
            Err(CallFailure::InvalidResponse)
        );
        let mut deep = json!({"json": {"a": {"b": [{"rubric": "x"}]}}});
        assert_eq!(
            screen("labs.get", &mut deep),
            Err(CallFailure::InvalidResponse)
        );
        let mut drill =
            json!({"json": {"script": {"alerts": [{"options": ["a", "b"], "answerKey": 1}]}}});
        assert_eq!(screen("orientation.get", &mut drill), Ok(()));
        let mut drill_leak =
            json!({"json": {"script": {"alerts": [{"answerKey": 1}], "rubric": {}}}});
        assert_eq!(
            screen("orientation.get", &mut drill_leak),
            Err(CallFailure::InvalidResponse)
        );
        let mut drill_text = json!({"json": {"script": {"alerts": [{"answerKey": "B"}]}}});
        assert_eq!(
            screen("orientation.get", &mut drill_text),
            Err(CallFailure::InvalidResponse)
        );
        let mut fine = json!({"json": {"title": "Prompt injection"}, "meta": {"values": {}}});
        assert_eq!(screen("units.get", &mut fine), Ok(()));
    }

    #[test]
    fn token_replies_require_bearer_scope_and_refresh() {
        let body = |v: Value| v.as_object().cloned().expect("object");
        let ok = body(
            json!({"token_type": "Bearer", "access_token": "a", "refresh_token": "r", "expires_in": 3600, "scope": "openid academy:learn"}),
        );
        assert!(tokens(&ok, None).is_ok());
        let scope = body(
            json!({"token_type": "bearer", "access_token": "a", "refresh_token": "r", "expires_in": 60, "scope": "openid"}),
        );
        assert_eq!(
            tokens(&scope, None).err(),
            Some(AcademyFailure::ScopeMissing)
        );
        let mac = body(
            json!({"token_type": "mac", "access_token": "a", "refresh_token": "r", "expires_in": 60}),
        );
        assert_eq!(
            tokens(&mac, None).err(),
            Some(AcademyFailure::InvalidResponse)
        );
        let no_refresh =
            body(json!({"token_type": "bearer", "access_token": "a", "expires_in": 60}));
        assert_eq!(
            tokens(&no_refresh, None).err(),
            Some(AcademyFailure::InvalidResponse)
        );
        assert!(tokens(&no_refresh, Some("kept")).is_ok_and(|t| t.refresh.as_str() == "kept"));
    }
}
