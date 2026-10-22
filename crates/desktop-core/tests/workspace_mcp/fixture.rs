//! A synthetic Gateway MCP plane on the desk's TLS server (C22): the
//! management catalog (`GET {DATA}/mcp-servers`), each server's Streamable
//! HTTP endpoint at `/mcp/<slug>/mcp` that requires its bearer, and the
//! OAuth surface the desktop signs in with — RFC 9728 protected-resource
//! metadata, RFC 8414 server metadata, RFC 7591 registration, authorize
//! (played by the test as the browser), token (code with S256 and
//! `resource`, refresh with rotation) and revocation.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::desk::DATA;
use crate::support::{Reply, Seen};

pub const RESULT: &str = "C22-FIXTURE-MCP-RESULT";

/// One published server.
#[derive(Clone)]
pub struct Server {
    pub id: String,
    pub slug: String,
    pub workspace: String,
    pub updated: String,
}

#[derive(Clone)]
struct Code {
    challenge: String,
    redirect: String,
    client: String,
    resource: String,
}

#[derive(Clone)]
pub struct Issued {
    pub resource: String,
    pub client: String,
}

pub struct Mcp {
    pub origin: String,
    pub servers: Vec<Server>,
    /// Seconds an access token lives.
    pub lifetime: u64,
    pub refuse_refresh: bool,
    /// The catalog answers 500.
    pub catalog_down: bool,
    clients: HashMap<String, String>,
    codes: HashMap<String, Code>,
    pub access: HashMap<String, Issued>,
    refresh: HashMap<String, Issued>,
    pub revoked: Vec<(String, String)>,
    pub registrations: usize,
    pub refreshes: usize,
    /// Tool calls each server ran, with the bearer they carried.
    pub calls: Vec<(String, String)>,
    /// Each call's execution trace: its ID (returned in the result) and the
    /// arguments the server ran it with (P06, C22 review G3).
    pub traces: Vec<(String, Value)>,
    /// Every MCP request's server and bearer (or none).
    pub requests: Vec<(String, Option<String>)>,
    /// The authorize URLs the browser stand-in completed.
    pub authorized: Vec<url::Url>,
    serial: usize,
}

pub type Shared = Arc<Mutex<Mcp>>;

fn reply(status: u16, body: &Value) -> Reply {
    let body = body.to_string().into_bytes();
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

impl Mcp {
    pub fn new(origin: &str) -> Self {
        Self {
            origin: origin.to_owned(),
            servers: Vec::new(),
            lifetime: 3600,
            refuse_refresh: false,
            catalog_down: false,
            clients: HashMap::new(),
            codes: HashMap::new(),
            access: HashMap::new(),
            refresh: HashMap::new(),
            revoked: Vec::new(),
            registrations: 0,
            refreshes: 0,
            calls: Vec::new(),
            traces: Vec::new(),
            requests: Vec::new(),
            authorized: Vec::new(),
            serial: 0,
        }
    }

    pub fn url(&self, slug: &str) -> String {
        format!("{}/mcp/{slug}/mcp", self.origin)
    }

    pub fn issuer(&self) -> String {
        format!("{}/oauth", self.origin)
    }

    pub fn publish(&mut self, id: &str, slug: &str, workspace: &str) {
        self.servers.push(Server {
            id: id.to_owned(),
            slug: slug.to_owned(),
            workspace: workspace.to_owned(),
            updated: "2026-10-01T00:00:00Z".to_owned(),
        });
    }

    fn next(&mut self, prefix: &str) -> String {
        self.serial += 1;
        format!("{prefix}-{:04}-{}", self.serial, "x".repeat(24))
    }

    fn tokens(&mut self, resource: &str, client: &str) -> Value {
        let access = self.next("mcp-access");
        let refresh = self.next("mcp-refresh");
        let issued = Issued {
            resource: resource.to_owned(),
            client: client.to_owned(),
        };
        self.access.insert(access.clone(), issued.clone());
        self.refresh.insert(refresh.clone(), issued);
        json!({"access_token": access, "token_type": "Bearer", "expires_in": self.lifetime,
            "refresh_token": refresh, "scope": "mcp:tools:list mcp:tools:call"})
    }

    /// The browser stand-in: completes the authorization the app opened.
    pub fn authorize(&mut self, url: &url::Url) -> String {
        let q: HashMap<String, String> = url.query_pairs().into_owned().collect();
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["code_challenge_method"], "S256");
        assert!(q["redirect_uri"].starts_with("http://127.0.0.1:"));
        assert_eq!(self.clients.get(&q["client_id"]), Some(&q["redirect_uri"]));
        assert_eq!(
            q.get("scope").map(String::as_str),
            Some("mcp:servers:read mcp:tools:list mcp:tools:call")
        );
        self.authorized.push(url.clone());
        let code = self.next("code");
        self.codes.insert(
            code.clone(),
            Code {
                challenge: q["code_challenge"].clone(),
                redirect: q["redirect_uri"].clone(),
                client: q["client_id"].clone(),
                resource: q["resource"].clone(),
            },
        );
        format!(
            "{}?code={code}&state={}",
            q["redirect_uri"],
            url::form_urlencoded::byte_serialize(q["state"].as_bytes()).collect::<String>()
        )
    }

    pub fn route(&mut self, seen: &Seen) -> Option<Reply> {
        let mut parts = seen.request_line.split(' ');
        let method = parts.next()?.to_owned();
        let target = parts.next()?.to_owned();
        let (path, query) = target.split_once('?').unwrap_or((target.as_str(), ""));
        if method == "GET" && path == format!("{DATA}/mcp-servers") {
            return Some(self.catalog(query));
        }
        if let Some(rest) = path.strip_prefix("/.well-known/oauth-protected-resource/mcp/") {
            let slug = rest.strip_suffix("/mcp")?;
            return Some(reply(
                200,
                &json!({"resource": self.url(slug), "authorization_servers": [self.issuer()],
                    "bearer_methods_supported": ["header"]}),
            ));
        }
        if path == "/.well-known/oauth-authorization-server/oauth" {
            let issuer = self.issuer();
            return Some(reply(
                200,
                &json!({"issuer": issuer,
                    "authorization_endpoint": format!("{issuer}/authorize"),
                    "token_endpoint": format!("{issuer}/token"),
                    "registration_endpoint": format!("{issuer}/register"),
                    "revocation_endpoint": format!("{issuer}/revoke"),
                    "response_types_supported": ["code"],
                    "grant_types_supported": ["authorization_code", "refresh_token"],
                    "code_challenge_methods_supported": ["S256"],
                    "token_endpoint_auth_methods_supported": ["none"],
                    "scopes_supported": ["mcp:servers:read", "mcp:servers:*", "mcp:tools:list",
                        "mcp:tools:call", "mcp:*"]}),
            ));
        }
        if path == "/oauth/register" && method == "POST" {
            let body: Value = serde_json::from_slice(&seen.body).unwrap_or_default();
            assert_eq!(body["token_endpoint_auth_method"], "none");
            let redirect = body["redirect_uris"][0]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            self.registrations += 1;
            let client = format!("mcp-client-{}", self.registrations);
            self.clients.insert(client.clone(), redirect);
            return Some(reply(201, &json!({"client_id": client})));
        }
        if path == "/oauth/token" && method == "POST" {
            return Some(self.token(&form(&seen.body)));
        }
        if path == "/oauth/revoke" && method == "POST" {
            let f = form(&seen.body);
            let token = f.get("token").cloned().unwrap_or_default();
            self.access.remove(&token);
            self.refresh.remove(&token);
            self.revoked
                .push((token, f.get("token_type_hint").cloned().unwrap_or_default()));
            return Some(reply(200, &json!({})));
        }
        if let Some(rest) = path.strip_prefix("/mcp/") {
            let slug = rest.strip_suffix("/mcp")?.to_owned();
            return Some(self.server(&slug, &method, seen));
        }
        None
    }

    fn catalog(&self, query: &str) -> Reply {
        if self.catalog_down {
            return reply(500, &json!({"error": {"message": "down"}}));
        }
        let q: HashMap<String, String> = url::form_urlencoded::parse(query.as_bytes())
            .into_owned()
            .collect();
        let workspace = q.get("workspace_id").cloned().unwrap_or_default();
        let rows: Vec<Value> = self
            .servers
            .iter()
            .filter(|s| s.workspace == workspace)
            .map(|s| {
                json!({"id": s.id, "name": format!("{} tools", s.slug), "slug": s.slug,
                    "url": self.url(&s.slug), "status": "active", "workspace_id": s.workspace,
                    "last_updated_at": s.updated, "auth_type": "oauth",
                    "mcp_integration_url": "https://upstream.example/never-kept", "object": "mcp_server"})
            })
            .collect();
        reply(
            200,
            &json!({"object": "list", "total": rows.len(), "data": rows}),
        )
    }

    fn token(&mut self, f: &HashMap<String, String>) -> Reply {
        match f.get("grant_type").map(String::as_str) {
            Some("authorization_code") => {
                let Some(code) = f.get("code").and_then(|c| self.codes.remove(c)) else {
                    return reply(400, &json!({"error": "invalid_grant"}));
                };
                let digest = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
                    Sha256::digest(f.get("code_verifier").cloned().unwrap_or_default()),
                );
                if digest != code.challenge
                    || f.get("redirect_uri") != Some(&code.redirect)
                    || f.get("client_id") != Some(&code.client)
                    || f.get("resource") != Some(&code.resource)
                {
                    return reply(400, &json!({"error": "invalid_grant"}));
                }
                let body = self.tokens(&code.resource, &code.client);
                reply(200, &body)
            }
            Some("refresh_token") => {
                self.refreshes += 1;
                let old = f.get("refresh_token").cloned().unwrap_or_default();
                let issued = self.refresh.get(&old).cloned();
                match issued {
                    Some(i)
                        if !self.refuse_refresh
                            && f.get("client_id") == Some(&i.client)
                            && f.get("resource") == Some(&i.resource) =>
                    {
                        self.refresh.remove(&old);
                        let body = self.tokens(&i.resource, &i.client);
                        reply(200, &body)
                    }
                    _ => reply(400, &json!({"error": "invalid_grant"})),
                }
            }
            _ => reply(400, &json!({"error": "unsupported_grant_type"})),
        }
    }

    fn server(&mut self, slug: &str, method: &str, seen: &Seen) -> Reply {
        let url = self.url(slug);
        let bearer = seen
            .header("authorization")
            .and_then(|h| h.strip_prefix("Bearer "))
            .map(str::to_owned);
        self.requests.push((slug.to_owned(), bearer.clone()));
        let valid = bearer
            .as_ref()
            .and_then(|b| self.access.get(b))
            .is_some_and(|i| i.resource == url);
        if !valid {
            let challenge = format!(
                "Bearer error=\"invalid_token\", resource_metadata=\"{}/.well-known/oauth-protected-resource/mcp/{slug}/mcp\"",
                self.origin
            );
            let body = br#"{"error":"unauthorized"}"#.to_vec();
            return Reply::Raw {
                head: format!(
                    "HTTP/1.1 401 Unauthorized\r\nwww-authenticate: {challenge}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close",
                    body.len()
                ),
                body,
            };
        }
        if method != "POST" {
            return reply(405, &json!({}));
        }
        let body: Value = serde_json::from_slice(&seen.body).unwrap_or_default();
        let Some(id) = body.get("id").cloned() else {
            return Reply::Raw {
                head: "HTTP/1.1 202 Accepted\r\ncontent-length: 0\r\nconnection: close".into(),
                body: Vec::new(),
            };
        };
        let result = match body["method"].as_str() {
            Some("initialize") => json!({"protocolVersion": body["params"]["protocolVersion"],
                "capabilities": {"tools": {}}, "serverInfo": {"name": slug, "version": "1.0.0"}}),
            Some("tools/list") => json!({"tools": [{"name": "lookup",
                "description": "Look up a workspace record",
                "inputSchema": {"type": "object", "properties": {"query": {"type": "string"}},
                    "required": ["query"]}}]}),
            Some("tools/call") => {
                self.calls
                    .push((slug.to_owned(), bearer.clone().unwrap_or_default()));
                let trace = format!("trace-{slug}-{}", self.calls.len());
                let arguments = body["params"]["arguments"].clone();
                let text = format!(
                    "{RESULT} {trace} query={}",
                    arguments["query"].as_str().unwrap_or_default()
                );
                self.traces.push((trace, arguments));
                json!({"content": [{"type": "text", "text": text}], "isError": false})
            }
            _ => json!({}),
        };
        reply(200, &json!({"jsonrpc": "2.0", "id": id, "result": result}))
    }
}
