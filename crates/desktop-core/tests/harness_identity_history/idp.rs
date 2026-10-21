//! A synthetic OpenID provider on the test server's `/idp` path: discovery,
//! JWKS, the token endpoint (authorization code with PKCE, refresh with
//! rotation) and revocation. Tokens are ES256 JWTs signed with a key made
//! for the test; a second key stands in for a rotation or a forger.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::support::{Reply, Seen};

pub const CLIENT: &str = "airs-desktop-c20";
pub const AUDIENCE: &str = "https://gateway.example/c20";
pub const SUBJECT: &str = "user-c20-alice";
pub const OTHER_SUBJECT: &str = "user-c20-mallory";

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs()
}

/// A signing key and its public JWK.
pub struct SigningKey {
    pub kid: String,
    der: Vec<u8>,
    pub jwk: Value,
}

impl SigningKey {
    pub fn new(kid: &str) -> Self {
        let pair = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).expect("key");
        let raw = pair.public_key_raw();
        Self {
            kid: kid.to_owned(),
            der: pair.serialize_der(),
            jwk: json!({"kty": "EC", "crv": "P-256", "kid": kid, "use": "sig", "alg": "ES256",
                "x": b64(&raw[1..33]), "y": b64(&raw[33..65])}),
        }
    }

    pub fn sign(&self, claims: &Value) -> String {
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(self.kid.clone());
        jsonwebtoken::encode(&header, claims, &EncodingKey::from_ec_der(&self.der)).expect("sign")
    }
}

/// How the next tokens are made (a test changes these to break them).
#[derive(Clone)]
pub struct Issue {
    pub subject: String,
    pub lifetime: u64,
    /// Claims merged into the access token (null removes one).
    pub access: Value,
    /// Claims merged into the ID token.
    pub id: Value,
    /// Sign with the forger's key instead.
    pub forged: bool,
    /// Replace the access token with this string.
    pub raw_access: Option<String>,
}

impl Default for Issue {
    fn default() -> Self {
        Self {
            subject: SUBJECT.to_owned(),
            lifetime: 3600,
            access: json!({}),
            id: json!({}),
            forged: false,
            raw_access: None,
        }
    }
}

struct Code {
    challenge: String,
    nonce: String,
    redirect: String,
}

pub struct Idp {
    pub issuer: String,
    pub key: SigningKey,
    pub forger: SigningKey,
    /// Published in the JWKS alongside `key` from the `rotate_after`th
    /// JWKS read on.
    pub rotated: Option<SigningKey>,
    pub rotate_after: u32,
    /// Sign new tokens with the rotated key.
    pub sign_rotated: bool,
    pub issue: Issue,
    /// Discovery document overrides (null removes a field).
    pub metadata: Value,
    pub refuse_refresh: bool,
    codes: HashMap<String, Code>,
    refresh: HashMap<String, String>,
    pub revoked: Vec<String>,
    pub refreshes: u32,
    pub jwks_reads: u32,
    pub workspace_slug: String,
}

fn merge(target: &mut Value, patch: &Value) {
    if let (Some(t), Some(p)) = (target.as_object_mut(), patch.as_object()) {
        for (k, v) in p {
            if v.is_null() {
                t.remove(k);
            } else {
                t.insert(k.clone(), v.clone());
            }
        }
    }
}

fn reply(status: u16, body: &Value) -> Reply {
    let body = serde_json::to_vec(body).expect("json");
    Reply::Raw {
        head: format!(
            "HTTP/1.1 {status} Status\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close",
            body.len()
        ),
        body,
    }
}

fn form(body: &[u8]) -> HashMap<String, String> {
    url::form_urlencoded::parse(body).into_owned().collect()
}

impl Idp {
    pub fn new(origin: &str, workspace_slug: &str) -> Self {
        Self {
            issuer: format!("{origin}/idp"),
            key: SigningKey::new("c20-key-1"),
            forger: SigningKey::new("c20-key-1"),
            rotated: None,
            rotate_after: 0,
            sign_rotated: false,
            issue: Issue::default(),
            metadata: json!({}),
            refuse_refresh: false,
            codes: HashMap::new(),
            refresh: HashMap::new(),
            revoked: Vec::new(),
            refreshes: 0,
            jwks_reads: 0,
            workspace_slug: workspace_slug.to_owned(),
        }
    }

    /// Lets the browser stand-in complete the authorization the app opened:
    /// the code is bound to its challenge, nonce and redirect.
    pub fn authorize(&mut self, url: &url::Url) -> (String, String) {
        let q: HashMap<String, String> = url.query_pairs().into_owned().collect();
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["client_id"], CLIENT);
        assert_eq!(q["code_challenge_method"], "S256");
        assert!(q["redirect_uri"].starts_with("http://127.0.0.1:"));
        let code = format!("code-{}", self.codes.len() + 1);
        self.codes.insert(
            code.clone(),
            Code {
                challenge: q["code_challenge"].clone(),
                nonce: q["nonce"].clone(),
                redirect: q["redirect_uri"].clone(),
            },
        );
        (
            code.clone(),
            format!(
                "{}?code={}&state={}&iss={}",
                q["redirect_uri"],
                code,
                url::form_urlencoded::byte_serialize(q["state"].as_bytes()).collect::<String>(),
                url::form_urlencoded::byte_serialize(self.issuer.as_bytes()).collect::<String>()
            ),
        )
    }

    fn signer(&self) -> &SigningKey {
        if self.issue.forged {
            &self.forger
        } else if self.sign_rotated {
            self.rotated.as_ref().expect("rotated key")
        } else {
            &self.key
        }
    }

    fn tokens(&mut self, nonce: Option<&str>) -> Value {
        let now = now();
        let issue = self.issue.clone();
        let mut access = json!({"iss": self.issuer, "aud": [AUDIENCE], "azp": CLIENT,
            "sub": issue.subject, "iat": now, "exp": now + issue.lifetime,
            "portkey_workspace": self.workspace_slug});
        merge(&mut access, &issue.access);
        let access = issue
            .raw_access
            .clone()
            .unwrap_or_else(|| self.signer().sign(&access));
        let refresh = format!("refresh-{}-{}", self.refresh.len() + 1, issue.subject);
        self.refresh.insert(refresh.clone(), issue.subject.clone());
        let mut body = json!({"token_type": "Bearer", "access_token": access,
            "expires_in": issue.lifetime, "refresh_token": refresh});
        if let Some(nonce) = nonce {
            let mut id = json!({"iss": self.issuer, "aud": CLIENT, "sub": issue.subject,
                "iat": now, "exp": now + 300, "nonce": nonce,
                "preferred_username": format!("{}@example.com", issue.subject)});
            merge(&mut id, &issue.id);
            body["id_token"] = json!(self.signer().sign(&id));
        }
        body
    }

    pub fn route(&mut self, seen: &Seen) -> Option<Reply> {
        let target = seen.request_line.split(' ').nth(1)?;
        let path = target.split_once('?').map_or(target, |(p, _)| p);
        let path = path.strip_prefix("/idp")?;
        Some(match path {
            "/.well-known/openid-configuration" => {
                let mut doc = json!({"issuer": self.issuer,
                    "authorization_endpoint": format!("{}/authorize", self.issuer),
                    "token_endpoint": format!("{}/token", self.issuer),
                    "jwks_uri": format!("{}/jwks", self.issuer),
                    "revocation_endpoint": format!("{}/revoke", self.issuer),
                    "response_types_supported": ["code"],
                    "code_challenge_methods_supported": ["S256"],
                    "scopes_supported": ["openid", "offline_access"],
                    "authorization_response_iss_parameter_supported": true});
                merge(&mut doc, &self.metadata);
                reply(200, &doc)
            }
            "/jwks" => {
                self.jwks_reads += 1;
                let mut keys = vec![self.key.jwk.clone()];
                if self.jwks_reads > self.rotate_after {
                    keys.extend(self.rotated.as_ref().map(|k| k.jwk.clone()));
                }
                reply(200, &json!({"keys": keys}))
            }
            "/token" => {
                let f = form(&seen.body);
                match f.get("grant_type").map(String::as_str) {
                    Some("authorization_code") => {
                        let Some(code) = f.get("code").and_then(|c| self.codes.remove(c)) else {
                            return Some(reply(400, &json!({"error": "invalid_grant"})));
                        };
                        let verifier = f.get("code_verifier").cloned().unwrap_or_default();
                        let challenge = b64(&Sha256::digest(verifier.as_bytes()));
                        if challenge != code.challenge
                            || f.get("redirect_uri") != Some(&code.redirect)
                            || f.get("client_id").map(String::as_str) != Some(CLIENT)
                        {
                            return Some(reply(400, &json!({"error": "invalid_grant"})));
                        }
                        let body = self.tokens(Some(&code.nonce));
                        reply(200, &body)
                    }
                    Some("refresh_token") => {
                        self.refreshes += 1;
                        let token = f.get("refresh_token").cloned().unwrap_or_default();
                        if self.refuse_refresh || self.refresh.remove(&token).is_none() {
                            return Some(reply(400, &json!({"error": "invalid_grant"})));
                        }
                        let body = self.tokens(None);
                        reply(200, &body)
                    }
                    _ => reply(400, &json!({"error": "unsupported_grant_type"})),
                }
            }
            "/revoke" => {
                let f = form(&seen.body);
                if let Some(token) = f.get("token") {
                    self.revoked.push(token.clone());
                    self.refresh.remove(token);
                }
                reply(200, &json!({}))
            }
            _ => reply(404, &json!({"error": "not here"})),
        })
    }
}

pub type Shared = Arc<Mutex<Idp>>;
