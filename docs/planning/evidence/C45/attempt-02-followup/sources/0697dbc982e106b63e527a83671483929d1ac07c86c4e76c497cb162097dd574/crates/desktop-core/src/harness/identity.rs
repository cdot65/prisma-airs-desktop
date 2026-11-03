//! Company sign-in for the Harness Gateway credential (C20, D047).
//!
//! OpenID Connect authorization code with PKCE (S256) through the system
//! browser and a loopback callback ([`super::callback`]); the desktop is a
//! public client. The provider is the issuer the operator entered: its
//! discovery document must name exactly that issuer and only HTTPS
//! endpoints on the issuer's origin, so metadata never grants a new
//! destination. Replies are bounded, redirects refused (client policy) and
//! transient provider failures read as `Network`.
//!
//! Both the ID token and the Gateway access token are JWTs verified against
//! the issuer's JWKS before anything is kept: asymmetric algorithms only,
//! the key's type and curve matching the header, issuer, audience,
//! authorized party, nonce (ID token) and expiry, not-before and issued-at
//! with a bounded skew judged on the wall clock the caller supplies. An
//! unknown key ID allows one bounded JWKS refresh. Opaque access tokens are
//! refused: no introspection contract exists (D047).

use std::time::Duration;

use base64::Engine as _;
use jsonwebtoken::jwk::{AlgorithmParameters, EllipticCurve, Jwk, JwkSet, PublicKeyUse};
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use ts_rs::TS;
use url::Url;
use zeroize::Zeroizing;

const REPLY_LIMIT: usize = 128 * 1024;
const JWKS_LIMIT: usize = 64 * 1024;
const MAX_KEYS: usize = 32;
const TOKEN_LIMIT: usize = 16 * 1024;
const TIMEOUT: Duration = Duration::from_secs(30);
/// Clock skew allowed on `exp`, `nbf` and `iat`.
pub const SKEW: u64 = 30;
/// The smallest RSA modulus accepted (2048 bits).
const MIN_RSA_BYTES: usize = 256;
const ALGORITHMS: [Algorithm; 9] = [
    Algorithm::RS256,
    Algorithm::RS384,
    Algorithm::RS512,
    Algorithm::PS256,
    Algorithm::PS384,
    Algorithm::PS512,
    Algorithm::ES256,
    Algorithm::ES384,
    Algorithm::EdDSA,
];

/// Why company sign-in or a renewal failed (sanitized; no provider text).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS, thiserror::Error)]
#[serde(rename_all = "camelCase")]
pub enum SignInFailure {
    #[error("the identity provider's configuration is not usable")]
    InvalidConfig,
    #[error("the identity provider could not be reached")]
    Network,
    #[error("the identity provider returned an invalid response")]
    InvalidResponse,
    #[error("sign-in was declined")]
    Denied,
    #[error("the browser did not return in time")]
    TimedOut,
    #[error("sign-in was cancelled")]
    Cancelled,
    #[error("the browser could not be opened")]
    BrowserUnavailable,
    #[error("the sign-in callback could not be opened")]
    CallbackUnavailable,
    #[error("a token did not verify")]
    InvalidToken,
    #[error("the Gateway token is not a signed JWT")]
    OpaqueToken,
    #[error("the token is for another Gateway workspace")]
    WrongWorkspace,
    #[error("a different account signed in")]
    DifferentAccount,
    #[error("the session expired")]
    SessionExpired,
    #[error("the session could not be kept")]
    StoreFailed,
}

/// What the operator entered for the Gateway's identity provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SsoConfig {
    /// Exactly as the provider names itself (`iss`), without a trailing `/`.
    pub issuer: String,
    pub client_id: String,
    /// The Gateway audience the access token must carry.
    pub audience: String,
}

pub(super) fn https(text: &str) -> Option<Url> {
    let url = Url::parse(text).ok()?;
    (url.scheme() == "https"
        && url.host_str().is_some_and(|h| !h.is_empty())
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none())
    .then_some(url)
}

pub(super) fn printable(text: &str, max: usize) -> bool {
    !text.is_empty() && text.len() <= max && text.bytes().all(|b| b.is_ascii_graphic())
}

impl SsoConfig {
    /// A configuration with an `https` issuer (no credentials, query or
    /// fragment), a client ID and an audience of printable ASCII.
    pub fn parse(issuer: &str, client_id: &str, audience: &str) -> Option<Self> {
        let issuer = issuer.trim().trim_end_matches('/');
        if issuer.len() > 2048 {
            return None;
        }
        let url = https(issuer)?;
        if url.query().is_some() || !printable(client_id, 256) || !printable(audience, 512) {
            return None;
        }
        Some(Self {
            issuer: issuer.to_owned(),
            client_id: client_id.to_owned(),
            audience: audience.to_owned(),
        })
    }

    fn origin(&self) -> String {
        https(&self.issuer)
            .map(|u| u.origin().ascii_serialization())
            .unwrap_or_default()
    }
}

/// The provider's endpoints, from its discovery document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provider {
    pub authorization: Url,
    pub token: Url,
    pub jwks: Url,
    pub revocation: Option<Url>,
    /// `offline_access` is listed, so it is asked for.
    pub offline_access: bool,
    /// RFC 9207: the callback must then carry `iss`.
    pub iss_in_callback: bool,
}

/// Tokens from a code exchange or a refresh.
pub struct Tokens {
    pub access: Zeroizing<String>,
    pub id_token: Option<Zeroizing<String>>,
    pub refresh: Option<Zeroizing<String>>,
}

/// One browser sign-in attempt: its secrets and the URL to open.
pub struct Attempt {
    pub state: Zeroizing<String>,
    pub nonce: Zeroizing<String>,
    pub verifier: Zeroizing<String>,
    pub redirect: String,
    pub url: Url,
}

/// The verified account behind an ID token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub subject: String,
    /// `preferred_username`, else `email`, else the subject.
    pub label: String,
}

/// A verified Gateway access token and when it stops being usable.
pub struct Access {
    pub token: Zeroizing<String>,
    pub subject: String,
    /// Wall-clock seconds since the epoch.
    pub expires: u64,
    pub issued: u64,
}

impl Access {
    /// Renew before `expires - min(120 s, lifetime / 2)` (DESIGN).
    pub fn renew_at(&self) -> u64 {
        let lifetime = self.expires.saturating_sub(self.issued);
        self.expires.saturating_sub((lifetime / 2).min(120))
    }
}

pub(super) fn random(bytes: usize) -> Option<Zeroizing<String>> {
    let mut raw = Zeroizing::new(vec![0u8; bytes]);
    getrandom::fill(&mut raw).ok()?;
    Some(Zeroizing::new(
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&*raw),
    ))
}

/// Starts an attempt whose callback is `http://127.0.0.1:{port}/callback`.
pub fn attempt(provider: &Provider, config: &SsoConfig, port: u16) -> Option<Attempt> {
    let state = random(32)?;
    let nonce = random(32)?;
    let verifier = random(48)?;
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(verifier.as_bytes()));
    let redirect = format!("http://127.0.0.1:{port}/callback");
    let scope = if provider.offline_access {
        "openid offline_access"
    } else {
        "openid"
    };
    let mut url = provider.authorization.clone();
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &config.client_id)
        .append_pair("redirect_uri", &redirect)
        .append_pair("scope", scope)
        .append_pair("state", &state)
        .append_pair("nonce", &nonce)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256");
    Some(Attempt {
        state,
        nonce,
        verifier,
        redirect,
        url,
    })
}

/// Reads a whole body up to `limit` bytes.
pub(super) async fn bounded_body(response: reqwest::Response, limit: usize) -> Result<Vec<u8>, ()> {
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

pub(super) fn transient(status: u16) -> bool {
    matches!(status, 500 | 502 | 503 | 504)
}

pub(super) fn string(value: Option<&Value>, max: usize) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= max)
}

pub(super) fn token(value: Option<&Value>) -> Option<Zeroizing<String>> {
    string(value, TOKEN_LIMIT)
        .filter(|s| s.bytes().all(|b| b.is_ascii_graphic()))
        .map(|s| Zeroizing::new(s.to_owned()))
}

/// The provider's identity endpoints over the app-owned HTTP client.
#[derive(Clone)]
pub struct IdentityClient {
    http: reqwest::Client,
}

impl IdentityClient {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }

    async fn json(
        &self,
        request: reqwest::RequestBuilder,
        limit: usize,
    ) -> Result<(u16, Map<String, Value>), SignInFailure> {
        let response = request
            .timeout(TIMEOUT)
            .header("accept", "application/json")
            .send()
            .await
            .map_err(|_| SignInFailure::Network)?;
        let status = response.status().as_u16();
        if (300..400).contains(&status) {
            return Err(SignInFailure::InvalidResponse);
        }
        let invalid = if transient(status) {
            SignInFailure::Network
        } else {
            SignInFailure::InvalidResponse
        };
        let body = bounded_body(response, limit).await.map_err(|()| invalid)?;
        // Revocation may answer with an empty success.
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
    ) -> Result<(u16, Map<String, Value>), SignInFailure> {
        let body = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(values)
            .finish();
        self.json(
            self.http
                .post(url.clone())
                .header("content-type", "application/x-www-form-urlencoded")
                .body(body),
            REPLY_LIMIT,
        )
        .await
    }

    /// OpenID discovery: the document must name exactly the configured
    /// issuer, support the code flow with S256, and keep every endpoint
    /// HTTPS on the issuer's origin.
    pub async fn discover(&self, config: &SsoConfig) -> Result<Provider, SignInFailure> {
        let (status, body) = self
            .json(
                self.http.get(format!(
                    "{}/.well-known/openid-configuration",
                    config.issuer
                )),
                REPLY_LIMIT,
            )
            .await?;
        if transient(status) {
            return Err(SignInFailure::Network);
        }
        let invalid = SignInFailure::InvalidConfig;
        if status != 200 || string(body.get("issuer"), 2048) != Some(config.issuer.as_str()) {
            return Err(invalid);
        }
        let origin = config.origin();
        let endpoint = |key: &str| {
            string(body.get(key), 2048)
                .and_then(https)
                .filter(|u| u.origin().ascii_serialization() == origin)
        };
        let listed = |key: &str, want: &str| {
            body.get(key)
                .and_then(Value::as_array)
                .is_some_and(|items| items.iter().any(|v| v.as_str() == Some(want)))
        };
        if !listed("code_challenge_methods_supported", "S256")
            || !listed("response_types_supported", "code")
        {
            return Err(invalid);
        }
        Ok(Provider {
            authorization: endpoint("authorization_endpoint").ok_or(invalid)?,
            token: endpoint("token_endpoint").ok_or(invalid)?,
            jwks: endpoint("jwks_uri").ok_or(invalid)?,
            revocation: match body.get("revocation_endpoint") {
                None | Some(Value::Null) => None,
                Some(_) => Some(endpoint("revocation_endpoint").ok_or(invalid)?),
            },
            offline_access: listed("scopes_supported", "offline_access"),
            iss_in_callback: body.get("authorization_response_iss_parameter_supported")
                == Some(&Value::Bool(true)),
        })
    }

    /// The issuer's signing keys.
    pub async fn keys(&self, provider: &Provider) -> Result<JwkSet, SignInFailure> {
        let (status, body) = self
            .json(self.http.get(provider.jwks.clone()), JWKS_LIMIT)
            .await?;
        if transient(status) {
            return Err(SignInFailure::Network);
        }
        if status != 200 {
            return Err(SignInFailure::InvalidConfig);
        }
        let keys = body
            .get("keys")
            .and_then(Value::as_array)
            .filter(|k| k.len() <= MAX_KEYS)
            .ok_or(SignInFailure::InvalidConfig)?;
        // A key this verifier cannot read is skipped, never fatal: providers
        // publish encryption and symmetric entries too.
        let keys = keys
            .iter()
            .filter_map(|k| serde_json::from_value::<Jwk>(k.clone()).ok())
            .collect();
        Ok(JwkSet { keys })
    }

    fn tokens(status: u16, body: &Map<String, Value>) -> Result<Tokens, SignInFailure> {
        if !(200..300).contains(&status) {
            return Err(match (status, string(body.get("error"), 128)) {
                (_, Some("invalid_grant" | "access_denied" | "unauthorized_client")) => {
                    SignInFailure::Denied
                }
                (s, _) if transient(s) => SignInFailure::Network,
                _ => SignInFailure::InvalidResponse,
            });
        }
        if !string(body.get("token_type"), 32).is_some_and(|t| t.eq_ignore_ascii_case("bearer")) {
            return Err(SignInFailure::InvalidResponse);
        }
        let access = token(body.get("access_token")).ok_or(SignInFailure::InvalidResponse)?;
        let id_token = match body.get("id_token") {
            None | Some(Value::Null) => None,
            v => Some(token(v).ok_or(SignInFailure::InvalidResponse)?),
        };
        let refresh = match body.get("refresh_token") {
            None | Some(Value::Null) => None,
            v => Some(token(v).ok_or(SignInFailure::InvalidResponse)?),
        };
        Ok(Tokens {
            access,
            id_token,
            refresh,
        })
    }

    /// Exchanges the callback's code (with the PKCE verifier).
    pub async fn exchange(
        &self,
        provider: &Provider,
        config: &SsoConfig,
        attempt: &Attempt,
        code: &str,
    ) -> Result<Tokens, SignInFailure> {
        let (status, body) = self
            .form(
                &provider.token,
                &[
                    ("grant_type", "authorization_code"),
                    ("client_id", &config.client_id),
                    ("code", code),
                    ("redirect_uri", &attempt.redirect),
                    ("code_verifier", &attempt.verifier),
                ],
            )
            .await?;
        let tokens = Self::tokens(status, &body)?;
        if tokens.id_token.is_none() {
            return Err(SignInFailure::InvalidResponse);
        }
        Ok(tokens)
    }

    /// Renews the access token. A refused refresh means signing in again.
    pub async fn refresh(
        &self,
        provider: &Provider,
        config: &SsoConfig,
        refresh_token: &str,
    ) -> Result<Tokens, SignInFailure> {
        let (status, body) = self
            .form(
                &provider.token,
                &[
                    ("grant_type", "refresh_token"),
                    ("client_id", &config.client_id),
                    ("refresh_token", refresh_token),
                ],
            )
            .await?;
        Self::tokens(status, &body).map_err(|failure| match failure {
            SignInFailure::Denied => SignInFailure::SessionExpired,
            other => other,
        })
    }

    /// Revokes a refresh token (best effort by the caller).
    pub async fn revoke(
        &self,
        provider: &Provider,
        config: &SsoConfig,
        refresh_token: &str,
    ) -> Result<(), SignInFailure> {
        let Some(url) = &provider.revocation else {
            return Ok(());
        };
        let (status, _) = self
            .form(
                url,
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
            Err(SignInFailure::InvalidResponse)
        }
    }
}

/// The decoding key for `kid` and `alg`, if the set has a matching
/// signature key of the right type, curve and size.
fn decoding_key(keys: &JwkSet, kid: Option<&str>, alg: Algorithm) -> Option<DecodingKey> {
    keys.keys.iter().find_map(|jwk| {
        if kid.is_some() && jwk.common.key_id.as_deref() != kid {
            return None;
        }
        if matches!(jwk.common.public_key_use, Some(ref u) if *u != PublicKeyUse::Signature) {
            return None;
        }
        if let Some(declared) = jwk.common.key_algorithm
            && declared.to_string() != format!("{alg:?}")
        {
            return None;
        }
        let fits = match (&jwk.algorithm, alg) {
            (
                AlgorithmParameters::RSA(rsa),
                Algorithm::RS256
                | Algorithm::RS384
                | Algorithm::RS512
                | Algorithm::PS256
                | Algorithm::PS384
                | Algorithm::PS512,
            ) => base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(rsa.n.as_bytes())
                .is_ok_and(|n| n.len() >= MIN_RSA_BYTES),
            (AlgorithmParameters::EllipticCurve(ec), Algorithm::ES256) => {
                ec.curve == EllipticCurve::P256
            }
            (AlgorithmParameters::EllipticCurve(ec), Algorithm::ES384) => {
                ec.curve == EllipticCurve::P384
            }
            (AlgorithmParameters::OctetKeyPair(okp), Algorithm::EdDSA) => {
                okp.curve == EllipticCurve::Ed25519
            }
            _ => false,
        };
        fits.then(|| DecodingKey::from_jwk(jwk).ok()).flatten()
    })
}

/// Why one verification did not pass: an unknown key may be fetched again.
enum Unverified {
    UnknownKey,
    Invalid,
}

/// Checks the signature with the set's key; claims are checked by the
/// caller against the supplied clock.
fn verified_claims(token: &str, keys: &JwkSet) -> Result<Map<String, Value>, Unverified> {
    if token.len() > TOKEN_LIMIT || token.split('.').count() != 3 {
        return Err(Unverified::Invalid);
    }
    let header = jsonwebtoken::decode_header(token).map_err(|_| Unverified::Invalid)?;
    // Only asymmetric algorithms: never `none` and never a shared secret.
    if !ALGORITHMS.contains(&header.alg) {
        return Err(Unverified::Invalid);
    }
    let key =
        decoding_key(keys, header.kid.as_deref(), header.alg).ok_or(Unverified::UnknownKey)?;
    let mut validation = Validation::new(header.alg);
    validation.algorithms = vec![header.alg];
    // Time and audience are judged below on the caller's clock.
    validation.validate_exp = false;
    validation.validate_nbf = false;
    validation.validate_aud = false;
    validation.required_spec_claims.clear();
    jsonwebtoken::decode::<Map<String, Value>>(token, &key, &validation)
        .map(|data| data.claims)
        .map_err(|_| Unverified::Invalid)
}

fn audience_has(claims: &Map<String, Value>, want: &str) -> (bool, usize) {
    match claims.get("aud") {
        Some(Value::String(one)) => (one == want, 1),
        Some(Value::Array(many)) => (many.iter().any(|v| v.as_str() == Some(want)), many.len()),
        _ => (false, 0),
    }
}

/// `exp` in the future, `nbf` and `iat` not after now (each with skew).
fn timely(claims: &Map<String, Value>, now: u64) -> Option<(u64, u64)> {
    let exp = claims.get("exp")?.as_u64()?;
    let iat = claims.get("iat")?.as_u64()?;
    if exp + SKEW <= now || iat > now + SKEW || iat >= exp {
        return None;
    }
    if let Some(nbf) = claims.get("nbf")
        && nbf.as_u64()? > now + SKEW
    {
        return None;
    }
    Some((exp, iat))
}

/// Verifies tokens against the issuer's keys, refreshing them at most once
/// per verification for an unknown key ID (so each later key rotation is
/// followed too; P06, C20 review suggestion).
pub struct Verifier {
    keys: Option<JwkSet>,
}

impl Verifier {
    pub fn new() -> Self {
        Self { keys: None }
    }

    async fn claims(
        &mut self,
        client: &IdentityClient,
        provider: &Provider,
        token: &str,
    ) -> Result<Map<String, Value>, SignInFailure> {
        if self.keys.is_none() {
            self.keys = Some(client.keys(provider).await?);
        }
        let keys = self.keys.as_ref().ok_or(SignInFailure::InvalidToken)?;
        match verified_claims(token, keys) {
            Ok(claims) => Ok(claims),
            Err(Unverified::UnknownKey) => {
                let keys = client.keys(provider).await?;
                let claims = verified_claims(token, &keys).map_err(|_| SignInFailure::InvalidToken);
                self.keys = Some(keys);
                claims
            }
            Err(_) => Err(SignInFailure::InvalidToken),
        }
    }

    /// The ID token of a code exchange: issuer, the client as audience (and
    /// authorized party when there are several), the attempt's nonce and
    /// its times.
    pub async fn id_token(
        &mut self,
        client: &IdentityClient,
        provider: &Provider,
        config: &SsoConfig,
        token: &str,
        nonce: &str,
        now: u64,
    ) -> Result<Account, SignInFailure> {
        let claims = self.claims(client, provider, token).await?;
        let invalid = SignInFailure::InvalidToken;
        if string(claims.get("iss"), 2048) != Some(config.issuer.as_str()) {
            return Err(invalid);
        }
        let (has, count) = audience_has(&claims, &config.client_id);
        let azp = string(claims.get("azp"), 256);
        if !has
            || (count > 1 && azp != Some(config.client_id.as_str()))
            || azp.is_some_and(|a| a != config.client_id)
        {
            return Err(invalid);
        }
        let nonce_ok = string(claims.get("nonce"), 512)
            .is_some_and(|n| constant_eq(n.as_bytes(), nonce.as_bytes()));
        if !nonce_ok || timely(&claims, now).is_none() {
            return Err(invalid);
        }
        let subject = string(claims.get("sub"), 255).ok_or(invalid)?.to_owned();
        let label = string(claims.get("preferred_username"), 256)
            .or_else(|| string(claims.get("email"), 256))
            .unwrap_or(&subject)
            .chars()
            .filter(|c| !c.is_control())
            .collect();
        Ok(Account { subject, label })
    }

    /// The Gateway access token: a JWT from the issuer for the configured
    /// audience, issued to this client when it says so, and naming the
    /// chosen workspace when it names one. Its `sub` is the account the
    /// grant belongs to; once a grant exists (`expected`), a token for
    /// another subject is a different account.
    #[allow(clippy::too_many_arguments)]
    pub async fn access_token(
        &mut self,
        client: &IdentityClient,
        provider: &Provider,
        config: &SsoConfig,
        token: Zeroizing<String>,
        expected: Option<&str>,
        workspace_slug: &str,
        now: u64,
    ) -> Result<Access, SignInFailure> {
        if !looks_like_jwt(&token) {
            return Err(SignInFailure::OpaqueToken);
        }
        let claims = self.claims(client, provider, &token).await?;
        let invalid = SignInFailure::InvalidToken;
        if string(claims.get("iss"), 2048) != Some(config.issuer.as_str())
            || !audience_has(&claims, &config.audience).0
        {
            return Err(invalid);
        }
        for party in ["azp", "client_id"] {
            if let Some(value) = claims.get(party)
                && value.as_str() != Some(config.client_id.as_str())
            {
                return Err(invalid);
            }
        }
        let subject = string(claims.get("sub"), 255).ok_or(invalid)?;
        if expected.is_some_and(|e| e != subject) {
            return Err(SignInFailure::DifferentAccount);
        }
        if let Some(named) = claims.get("portkey_workspace")
            && named.as_str() != Some(workspace_slug)
        {
            return Err(SignInFailure::WrongWorkspace);
        }
        let (expires, issued) = timely(&claims, now).ok_or(invalid)?;
        let subject = subject.to_owned();
        Ok(Access {
            token,
            subject,
            expires,
            issued,
        })
    }
}

impl Default for Verifier {
    fn default() -> Self {
        Self::new()
    }
}

/// Three segments whose first is a JSON header naming an algorithm: a JWT
/// (verified or refused), as opposed to an opaque token.
fn looks_like_jwt(token: &str) -> bool {
    let mut parts = token.split('.');
    let (Some(header), Some(_), Some(_), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(header)
        .ok()
        .and_then(|h| serde_json::from_slice::<Value>(&h).ok())
        .is_some_and(|h| h.get("alg").is_some_and(Value::is_string))
}

/// Compares secrets without an early exit on the first difference.
pub fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configurations_are_https_and_printable() {
        let c = SsoConfig::parse(" https://id.example/oauth2/default/ ", "client", "aud")
            .expect("valid");
        assert_eq!(c.issuer, "https://id.example/oauth2/default");
        assert_eq!(c.origin(), "https://id.example");
        for (issuer, client, audience) in [
            ("http://id.example", "client", "aud"),
            ("https://user:pw@id.example", "client", "aud"),
            ("https://id.example?x=1", "client", "aud"),
            ("https://id.example#f", "client", "aud"),
            ("https://id.example", "", "aud"),
            ("https://id.example", "cli ent", "aud"),
            ("https://id.example", "client", "a\nud"),
        ] {
            assert!(
                SsoConfig::parse(issuer, client, audience).is_none(),
                "{issuer} {client} {audience:?}"
            );
        }
    }

    #[test]
    fn a_token_is_renewed_inside_its_margin() {
        let access = |issued, expires| Access {
            token: Zeroizing::new(String::new()),
            subject: String::new(),
            expires,
            issued,
        };
        // min(120 s, lifetime / 2) before expiry.
        assert_eq!(access(1000, 4600).renew_at(), 4480);
        assert_eq!(access(1000, 1006).renew_at(), 1003);
        assert!(constant_eq(b"state", b"state"));
        assert!(!constant_eq(b"state", b"statf") && !constant_eq(b"state", b"stat"));
    }

    #[test]
    fn an_attempt_carries_pkce_state_nonce_and_the_loopback() {
        let provider = Provider {
            authorization: Url::parse("https://id.example/authorize").expect("url"),
            token: Url::parse("https://id.example/token").expect("url"),
            jwks: Url::parse("https://id.example/jwks").expect("url"),
            revocation: None,
            offline_access: true,
            iss_in_callback: true,
        };
        let config = SsoConfig::parse("https://id.example", "client", "aud").expect("valid");
        let a = attempt(&provider, &config, 4242).expect("attempt");
        let b = attempt(&provider, &config, 4242).expect("attempt");
        assert_ne!(a.state.as_str(), b.state.as_str());
        let q: std::collections::HashMap<String, String> =
            a.url.query_pairs().into_owned().collect();
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:4242/callback");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(
            q["code_challenge"],
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(Sha256::digest(a.verifier.as_bytes()))
        );
        assert_eq!(q["scope"], "openid offline_access");
        assert_eq!(q["state"].len(), 43);
        assert_eq!(q["nonce"].len(), 43);
        assert!(!a.url.as_str().contains(a.verifier.as_str()));
    }
}
