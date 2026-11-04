//! A synthetic management cloud on one TLS origin, assembled from the
//! desktop-core fixtures' reply shapes: the OAuth grant (a designated and a
//! restricted principal), AIRS management (profiles, topics, applications,
//! keys and their writes), DLP, sessions, the Runtime overview, Red Team
//! and Model/skill observations, AI Gateway (admin, data and IAM planes,
//! with changes and provisioning), Gateway inference and the IAM access
//! policies the API Client's managed collection reads and writes. A
//! restricted principal is denied every request, or (`Restricted`) only
//! its writes, or (a fault) nothing.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::common::{TSG, parts, raw, restricted, token};
use crate::support::{Reply, Seen};

pub const DASH: &str = "/aisec/v1/mgmt/dashboard/v2/sessions/";
pub const DATA: &str = "/ai_gw/v2";
pub const ADMIN: &str = "/ai_gw/admin/v2";
pub const IAM: &str = "/iam/v1";
pub const RUNTIME: &str = "/rt/v1";
pub const WS_A: &str = "16f7e90d-382a-4e78-b577-1b01eb5f8297";
pub const PROVIDER: &str = "7e3a0000-0000-4000-8000-000000000001";
pub const ROUTE: &str = "7e3a0000-0000-4000-8000-000000000002";
pub const INTEGRATION: &str = "f6692544-3265-49be-9711-bbdcebc079eb";
pub const RED_TEAM_SCAN: &str = "11111111-2222-4333-8444-555555555555";
pub const ONE_TIME: &str = "SYNTHETIC-ONE-TIME-KEY-c25-";
pub const MODEL: &str = "@openai-prod/gpt-4o";

fn listing(rows: Vec<Value>) -> Value {
    let total = rows.len();
    json!({"object": "list", "total": total, "success": true, "has_more": false, "data": rows})
}

fn workspace(id: &str, name: &str, slug: &str) -> Value {
    json!({"id": id, "slug": slug, "name": name, "icon": null, "description": null,
           "created_at": "2026-10-01T00:00:00Z", "last_updated_at": "2026-10-02T00:00:00Z",
           "is_default": 0, "status": "active", "scope_name": format!("scope-{slug}"),
           "object": "workspace", "defaults": {"config_id": ROUTE, "allow_config_override": true},
           "usage_limits": null, "rate_limits": null})
}

fn provider(id: &str, ws: &str, name: &str) -> Value {
    json!({"id": id, "ai_provider_name": "openai", "slug": name, "name": name,
           "usage_limits": null, "status": "active", "note": null, "created_at": "2026-10-01T00:00:00Z",
           "expires_at": null, "last_reset_at": null, "rate_limits": [],
           "integration_id": INTEGRATION, "object": "provider", "workspace_id": ws})
}

fn route(id: &str, ws: &str, config: &Value) -> Value {
    json!({"id": id, "name": "default-route", "slug": "default-route", "organisation_id": "org-1",
           "is_default": 1, "status": "active", "owner_id": "owner-1", "updated_by": "owner-1",
           "created_at": "2026-10-01T00:00:00Z", "last_updated_at": "2026-10-02T00:00:00Z",
           "workspace_id": ws, "object": "config", "format": "json", "type": "ROUTING",
           "version_id": "v-1", "config": config.to_string()})
}

fn guardrail(id: &str, ws: &str) -> Value {
    json!({"id": id, "name": "pii", "slug": "pii", "organisation_id": "org-1", "status": "active",
           "owner_id": "owner-1", "updated_by": null, "created_at": "2026-10-01T00:00:00Z",
           "last_updated_at": "2026-10-02T00:00:00Z", "workspace_id": ws, "object": "guardrail",
           "checks": [], "actions": {"deny": true, "async": false, "sequential": false},
           "version_id": "v-1"})
}

fn key(id: &str, ws: &str, name: &str) -> Value {
    json!({"id": id, "name": name, "object": "api-key", "status": "active",
           "scopes": ["completions.write"], "workspace_id": ws})
}

fn integration() -> Value {
    json!({"id": INTEGRATION, "name": "OpenAI", "owner_id": "owner-1", "status": "active",
           "created_at": "2026-10-01T00:00:00Z", "last_updated_at": null, "slug": "openai",
           "description": null, "workspace_id": null, "ai_provider_id": "openai",
           "object": "integration", "configurations": "{}"})
}

fn merge(target: &mut Value, edits: &Value) {
    if let (Some(target), Some(edits)) = (target.as_object_mut(), edits.as_object()) {
        for (k, v) in edits {
            target.insert(k.clone(), v.clone());
        }
    }
}

fn no_content() -> Reply {
    Reply::Raw {
        head: "HTTP/1.1 204 No Content\r\ncontent-length: 0\r\nconnection: close".into(),
        body: Vec::new(),
    }
}

fn sse(parts: Vec<Vec<u8>>) -> Reply {
    Reply::Stream {
        head: "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\nconnection: close"
            .into(),
        parts: parts.into_iter().map(|p| (0, p)).collect(),
        hang: false,
    }
}

fn chunk(id: &str, text: Option<&str>, finish: Option<&str>, usage: Option<Value>) -> Vec<u8> {
    let mut delta = json!({});
    if let Some(text) = text {
        delta["content"] = json!(text);
    }
    let mut event = json!({"id": id, "object": "chat.completion.chunk", "created": 1_760_000_000,
        "model": "gpt-4o", "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]});
    if let Some(usage) = usage {
        event["usage"] = usage;
    }
    format!("data: {event}\n\n").into_bytes()
}

/// How the inference endpoint answers a stream.
#[derive(Clone)]
pub enum Chat {
    /// A short complete answer with usage.
    Text,
    /// Many paced chunks (long enough to be stopped).
    Paced,
}

/// What the restricted principal may do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Restricted {
    /// Every request is refused (403).
    DenyAll,
    /// Reads succeed; writes are refused (403).
    ReadOnly,
    /// A fault: its writes are applied too.
    WritesAllowed,
}

pub struct Cloud {
    serial: u32,
    pub restricted: Restricted,
    /// Whether the Red Team scan list shows the seeded completed scan.
    pub red_team_scans: bool,
    pub profiles: Vec<Value>,
    pub topics: Vec<Value>,
    pub apps: Vec<Value>,
    pub keys: Vec<Value>,
    pub dlp: BTreeMap<&'static str, Vec<Value>>,
    pub workspaces: Vec<Value>,
    pub items: BTreeMap<&'static str, Vec<Value>>,
    pub scopes: BTreeMap<String, Value>,
    pub policies: Vec<Value>,
    pub chat: Chat,
    /// Usage the inference endpoint reports for a complete answer.
    pub tokens: u64,
    /// Every write (method and path) this cloud accepted.
    pub writes: Vec<String>,
}

impl Cloud {
    pub fn new() -> Self {
        let mut dlp = BTreeMap::new();
        dlp.insert("data-patterns", Vec::new());
        dlp.insert(
            "data-profiles",
            vec![json!({"id": "dprof-1", "name": "Payments", "detection_rules": [
                {"rule_type": "expression_tree", "expression_tree": {"rule_item": {
                    "detection_technique": "regex", "id": "pattern-1", "name": "Card numbers", "version": 1,
                    "match_type": "include", "confidence_level": "high", "occurrence_operator_type": "any",
                    "supported_confidence_levels": ["low", "high"]}}}]})],
        );
        dlp.insert(
            "data-filtering-profiles",
            vec![json!({"id": "filter-1", "name": "Default", "file_based": false, "non_file_based": true})],
        );
        dlp.insert("dictionaries", Vec::new());
        let mut items: BTreeMap<&'static str, Vec<Value>> = BTreeMap::new();
        items.insert("providers", vec![provider(PROVIDER, WS_A, "openai-prod")]);
        items.insert(
            "configs",
            vec![route(
                ROUTE,
                WS_A,
                &json!({"targets": [{"provider": "@openai-prod"}]}),
            )],
        );
        items.insert(
            "guardrails",
            vec![guardrail("7e3a0000-0000-4000-8000-000000000003", WS_A)],
        );
        items.insert(
            "api-keys/service",
            vec![key("7e3a0000-0000-4000-8000-000000000004", WS_A, "ci-key")],
        );
        items.insert("api-keys/user", Vec::new());
        let mut scopes = BTreeMap::new();
        scopes.insert(
            "scope-team-a".to_owned(),
            json!({"name": "scope-team-a", "description": "Team A", "tsg_id": TSG, "id": "id-team-a",
                   "resources": [{"resource_type": "workspace", "resource_id": "team-a"}]}),
        );
        Self {
            serial: 100,
            restricted: Restricted::DenyAll,
            red_team_scans: true,
            profiles: (0..3)
                .map(|n| json!({"profile_id": format!("00000000-0000-4000-8000-{n:012}"),
                    "profile_name": format!("Profile {n}"), "revision": 1, "active": true, "policy": {}}))
                .collect(),
            topics: vec![json!({"topic_id": "11111111-1111-4111-8111-111111111111",
                "topic_name": "Finance", "revision": 1, "description": "Money talk", "examples": ["wire"]})],
            apps: vec![json!({"customer_appId": "app-1", "app_name": "Support bot", "cloud_provider": "aws",
                "environment": "production", "tsg_id": TSG,
                "api_keys_dp_info": [{"api_key_name": "Support key", "auth_code": "SYNTHETIC-DP-CODE",
                    "dp_name": "dp-1"}]})],
            keys: vec![json!({"api_key_id": "key-1", "api_key_name": "Support key", "cust_app": "Support bot",
                "api_key_last8": "abcd1234", "expiration": "2027-01-01T00:00:00Z", "revoked": false,
                "auth_code": "STORED-CODE"})],
            dlp,
            workspaces: vec![workspace(WS_A, "Team A", "team-a")],
            items,
            scopes,
            policies: Vec::new(),
            chat: Chat::Text,
            tokens: 8,
            writes: Vec::new(),
        }
    }

    fn id(&mut self) -> String {
        self.serial += 1;
        format!("7e3a0000-0000-4000-8000-{:012}", self.serial)
    }

    pub fn route(&mut self, seen: &Seen) -> Reply {
        if let Some(reply) = token(seen) {
            return reply;
        }
        let (method, path, query) = parts(seen);
        if restricted(seen) {
            let refused = match self.restricted {
                Restricted::DenyAll => true,
                Restricted::ReadOnly => method != "GET",
                Restricted::WritesAllowed => false,
            };
            if refused {
                return raw(403, &json!({"error": "forbidden"}));
            }
        }
        if method != "GET" {
            self.writes.push(format!("{method} {path}"));
        }
        let body: Value = serde_json::from_slice(&seen.body).unwrap_or(Value::Null);
        let q = |name: &str| query.get(name).cloned().unwrap_or_default();
        if let Some(rest) = path.strip_prefix("/dlp/v2/api/") {
            return self.dlp(&method, rest, seen, &body);
        }
        if path.starts_with(DASH) || path == "/aisec/v1/mgmt/reports/scancontent" {
            return sessions(&path, &q);
        }
        if !self.red_team_scans && path == "/ai-red-teaming/data-plane/v1/scan" {
            return raw(200, &json!({"data": [], "pagination": {"total_items": 0}}));
        }
        if let Some(reply) = red_team(&path) {
            return reply;
        }
        if path.starts_with("/aisec/") {
            return self.runtime(&method, &path, &q, &body);
        }
        if let Some(rest) = path.strip_prefix(RUNTIME) {
            return self.inference(&method, rest, &body);
        }
        if let Some(rest) = path.strip_prefix(IAM) {
            return self.iam(&method, rest, &body);
        }
        if let Some(rest) = path.strip_prefix(ADMIN) {
            return self.gateway(&method, true, rest, &q, &body);
        }
        if let Some(rest) = path.strip_prefix(DATA) {
            return self.gateway(&method, false, rest, &q, &body);
        }
        raw(404, &json!({"error": "not found"}))
    }

    fn runtime(
        &mut self,
        method: &str,
        path: &str,
        q: &dyn Fn(&str) -> String,
        body: &Value,
    ) -> Reply {
        let tsg = format!("/tsg/{TSG}");
        let page = |items: &[Value], key: &str| {
            let offset: usize = q("offset").parse().unwrap_or(0);
            let limit: usize = q("limit").parse().unwrap_or(100);
            let end = (offset + limit).min(items.len());
            let mut reply = json!({ key: items[offset.min(end)..end].to_vec() });
            if key == "ai_profiles" && end < items.len() {
                reply["next_offset"] = json!(end);
            }
            raw(200, &reply)
        };
        match (method, path) {
            // As the live service (C25): inactive profiles are not listed.
            ("GET", p) if p == format!("/aisec/v1/mgmt/profiles{tsg}") => {
                let listed: Vec<Value> = self
                    .profiles
                    .iter()
                    .filter(|p| p["active"] != json!(false))
                    .cloned()
                    .collect();
                page(&listed, "ai_profiles")
            }
            ("GET", p) if p == format!("/aisec/v1/mgmt/topics{tsg}") => {
                page(&self.topics, "custom_topics")
            }
            ("GET", p) if p == format!("/aisec/v1/mgmt/customerapp{tsg}") => {
                page(&self.apps, "customer_apps")
            }
            ("GET", p) if p == format!("/aisec/v1/mgmt/apikeys{tsg}") => {
                page(&self.keys, "api_keys")
            }
            ("GET", "/aisec/v1/mgmt/deploymentprofiles") => raw(
                200,
                &json!({"status": "success",
                "deployment_profiles": [{"dp_name": "dp-1", "auth_code": "SYNTHETIC-DP-CODE", "status": "active"}]}),
            ),
            ("GET", "/aisec/v1/mgmt/dlpprofiles") => raw(200, &json!({"dlp_profiles": []})),
            ("GET", "/aisec/v1/mgmt/dashboard/v2/sessions/sessionschart") => {
                raw(200, &json!({"bucket_size_seconds": 3600, "buckets": []}))
            }
            ("POST", "/aisec/v1/mgmt/topic") => {
                let mut topic = body.clone();
                topic["topic_id"] = json!(format!("33333333-3333-4333-8333-{:012}", self.serial));
                self.serial += 1;
                topic["revision"] = json!(1);
                self.topics.push(topic.clone());
                raw(200, &topic)
            }
            ("PUT", p) if p.starts_with("/aisec/v1/mgmt/topic/uuid/") => {
                let id = p.rsplit('/').next().unwrap_or("");
                let Some(topic) = self.topics.iter_mut().find(|t| t["topic_id"] == id) else {
                    return raw(404, &json!({"error": "not found"}));
                };
                let revision = topic["revision"].as_i64().unwrap_or(0) + 1;
                *topic = body.clone();
                topic["topic_id"] = json!(id);
                topic["revision"] = json!(revision);
                raw(200, &topic.clone())
            }
            ("DELETE", p) if p.starts_with("/aisec/v1/mgmt/topic/") => {
                let id = p.rsplit('/').next().unwrap_or("").to_owned();
                self.topics.retain(|t| t["topic_id"] != id.as_str());
                raw(200, &json!({"message": "deleted"}))
            }
            ("POST", "/aisec/v1/mgmt/profile") => {
                let mut profile = body.clone();
                profile["profile_id"] =
                    json!(format!("44444444-4444-4444-8444-{:012}", self.serial));
                self.serial += 1;
                profile["revision"] = json!(1);
                self.profiles.push(profile.clone());
                raw(200, &profile)
            }
            ("DELETE", p) if p.starts_with("/aisec/v1/mgmt/profile/") => {
                let id = p.rsplit('/').next().unwrap_or("").to_owned();
                self.profiles.retain(|t| t["profile_id"] != id.as_str());
                raw(200, &json!({"message": "deleted"}))
            }
            ("PUT", "/aisec/v1/mgmt/customerapp") => {
                let id = q("customer_app_id");
                let Some(app) = self
                    .apps
                    .iter_mut()
                    .find(|a| a["customer_appId"] == id.as_str())
                else {
                    return raw(404, &json!({"error": "not found"}));
                };
                // As the live service (C25): an update needs the app's code.
                if body.get("auth_code").and_then(Value::as_str).is_none() {
                    return raw(400, &json!("auth_code is required"));
                }
                let infos = app.get("api_keys_dp_info").cloned();
                *app = body.clone();
                app["customer_appId"] = json!(id);
                app["tsg_id"] = json!(TSG);
                if let Some(infos) = infos {
                    app["api_keys_dp_info"] = infos;
                }
                if let Some(o) = app.as_object_mut() {
                    o.remove("auth_code");
                }
                raw(200, &app.clone())
            }
            ("DELETE", "/aisec/v1/mgmt/customerapp") => {
                let name = q("app_name");
                self.apps.retain(|a| a["app_name"] != name.as_str());
                self.keys.retain(|k| k["cust_app"] != name.as_str());
                raw(200, &json!({"message": "deleted"}))
            }
            ("POST", "/aisec/v1/mgmt/apikey") => {
                self.serial += 1;
                let secret = format!("{ONE_TIME}{}", self.serial);
                let key = json!({"api_key_id": format!("key-{}", self.serial), "api_key_name": body["api_key_name"],
                    "cust_app": body["cust_app"], "api_key_last8": &secret[secret.len() - 8..],
                    "expiration": "2027-06-01T00:00:00Z", "revoked": false, "auth_code": body["auth_code"]});
                if !self.apps.iter().any(|a| a["app_name"] == body["cust_app"]) {
                    self.apps
                        .push(json!({"customer_appId": format!("app-{}", self.serial),
                        "app_name": body["cust_app"], "cloud_provider": body["cust_cloud_provider"],
                        "environment": body["cust_env"], "tsg_id": TSG,
                        "api_keys_dp_info": [{"api_key_name": body["api_key_name"],
                            "auth_code": body["auth_code"], "dp_name": body["dp_name"]}]}));
                }
                self.keys.push(key.clone());
                let mut reply = key;
                reply["api_key"] = json!(secret);
                raw(200, &reply)
            }
            ("POST", p) if p.starts_with("/aisec/v1/mgmt/apikey/regenerate/") => {
                let id = p.rsplit('/').next().unwrap_or("");
                self.serial += 1;
                let secret = format!("{ONE_TIME}{}", self.serial);
                let Some(key) = self.keys.iter_mut().find(|k| k["api_key_id"] == id) else {
                    return raw(404, &json!({"error": "not found"}));
                };
                key["api_key_last8"] = json!(&secret[secret.len() - 8..]);
                // As the live service (C25): a rotated key has a new ID.
                key["api_key_id"] = json!(format!("key-{}", self.serial));
                let mut reply = key.clone();
                reply["api_key"] = json!(secret);
                raw(200, &reply)
            }
            ("DELETE", p) if p.starts_with("/aisec/v1/mgmt/apikey/delete/") => {
                let name =
                    url::form_urlencoded::parse(p.rsplit('/').next().unwrap_or("").as_bytes())
                        .map(|(k, v)| format!("{k}{v}"))
                        .collect::<String>()
                        .replace("%20", " ");
                let name = name.replace('+', " ");
                let app = self
                    .keys
                    .iter()
                    .find(|k| k["api_key_name"] == name.as_str())
                    .map(|k| k["cust_app"].clone());
                self.keys.retain(|k| k["api_key_name"] != name.as_str());
                // As the live service (C25): an app goes with its last key.
                if let Some(app) = app
                    && !self.keys.iter().any(|k| k["cust_app"] == app)
                {
                    self.apps.retain(|a| a["app_name"] != app);
                }
                raw(200, &json!({"message": "deleted"}))
            }
            _ => raw(404, &json!({"error": "not found"})),
        }
    }

    fn dlp(&mut self, method: &str, rest: &str, seen: &Seen, body: &Value) -> Reply {
        let (collection, id) = match rest.split_once('/') {
            Some((c, id)) => (c.to_owned(), Some(id.to_owned())),
            None => (rest.to_owned(), None),
        };
        let dictionary = collection == "dictionaries";
        let body = if dictionary && method != "GET" && method != "DELETE" {
            multipart_json(seen)
        } else {
            body.clone()
        };
        self.serial += 1;
        let serial = self.serial;
        let key: &'static str = match collection.as_str() {
            "data-patterns" => "data-patterns",
            "data-profiles" => "data-profiles",
            "data-filtering-profiles" => "data-filtering-profiles",
            "dictionaries" => "dictionaries",
            _ => return raw(404, &json!({"error": "not found"})),
        };
        let store = self.dlp.get_mut(key).expect("store");
        match (method, id) {
            ("GET", None) => raw(
                200,
                &json!({"content": store.clone(), "last": true, "totalElements": store.len()}),
            ),
            ("GET", Some(id)) => match store.iter().find(|i| i["id"] == id.as_str()) {
                Some(item) => raw(200, item),
                None => raw(404, &json!({"error": "not found"})),
            },
            ("POST", None) => {
                let mut item = body;
                item["id"] = json!(format!("{collection}-{serial}"));
                store.push(item.clone());
                raw(200, &item)
            }
            ("PUT", Some(id)) => match store.iter_mut().find(|i| i["id"] == id.as_str()) {
                Some(item) => {
                    if key == "data-filtering-profiles" {
                        // As the live service (C25): fields left out of a
                        // filtering profile (its name, its own fields) stay.
                        if let (Some(kept), Some(sent)) = (item.as_object_mut(), body.as_object()) {
                            kept.extend(sent.clone());
                        }
                    } else {
                        *item = body;
                    }
                    item["id"] = json!(id);
                    if dictionary {
                        return no_content();
                    }
                    raw(200, &item.clone())
                }
                None => raw(404, &json!({"error": "not found"})),
            },
            ("DELETE", Some(id)) => {
                if dictionary {
                    store.retain(|i| i["id"] != id.as_str());
                } else if let Some(item) = store.iter_mut().find(|i| i["id"] == id.as_str()) {
                    // As the live service (C25): a deleted item is archived,
                    // still readable by its ID with status `deleted`.
                    let name = item["name"].as_str().unwrap_or_default().to_owned();
                    item["name"] = json!(format!("{name}_archived_20261025145341"));
                    item["status"] = json!("deleted");
                }
                no_content()
            }
            _ => raw(405, &json!({"error": "method"})),
        }
    }

    fn iam(&mut self, method: &str, rest: &str, body: &Value) -> Reply {
        if let Some(name) = rest.strip_prefix("/scopes") {
            let name = name.trim_start_matches('/').to_owned();
            return match method {
                "POST" => {
                    let name = body["name"].as_str().unwrap_or("").to_owned();
                    let scope = json!({"name": name, "description": body["description"], "tsg_id": TSG,
                        "id": format!("id-{name}"), "resources": []});
                    self.scopes.insert(name, scope.clone());
                    raw(200, &scope)
                }
                "GET" => match self.scopes.get(&name) {
                    Some(s) => raw(200, s),
                    None => raw(404, &json!({"error": "not found"})),
                },
                "PUT" => match self.scopes.get_mut(&name) {
                    Some(s) => {
                        s["resources"] = body["resources"].clone();
                        s["description"] = body["description"].clone();
                        raw(200, &s.clone())
                    }
                    None => raw(404, &json!({"error": "not found"})),
                },
                "DELETE" => {
                    self.scopes.remove(&name);
                    no_content()
                }
                _ => raw(405, &json!({})),
            };
        }
        if let Some(name) = rest.strip_prefix("/access_policies") {
            let name = name.trim_start_matches('/').to_owned();
            return match (method, name.is_empty()) {
                ("GET", true) => raw(200, &json!({"items": self.policies})),
                ("POST", true) => {
                    self.policies.push(body.clone());
                    raw(200, body)
                }
                ("DELETE", false) => {
                    self.policies.retain(|p| p["name"] != name.as_str());
                    raw(200, &json!({"deleted": name}))
                }
                _ => raw(404, &json!({})),
            };
        }
        raw(404, &json!({"error": "not found"}))
    }

    fn gateway(
        &mut self,
        method: &str,
        admin: bool,
        rest: &str,
        q: &dyn Fn(&str) -> String,
        body: &Value,
    ) -> Reply {
        let segments: Vec<&str> = rest.trim_start_matches('/').split('/').collect();
        let ws = q("workspace_id");
        match (method, segments.as_slice()) {
            ("GET", ["workspaces"]) => {
                let active: Vec<Value> = self
                    .workspaces
                    .iter()
                    .filter(|w| w["status"] == "active")
                    .cloned()
                    .collect();
                raw(200, &listing(active))
            }
            ("GET", ["workspaces", id]) => match self.workspaces.iter().find(|w| w["id"] == *id) {
                Some(w) => raw(200, w),
                None => raw(404, &json!({"error": "not found"})),
            },
            ("POST", ["workspaces"]) if admin => {
                let id = self.id();
                let name = body["name"].as_str().unwrap_or("new").to_owned();
                let slug = name.to_lowercase().replace(' ', "-");
                let mut created = workspace(&id, &name, &slug);
                created["scope_name"] = body["scope_name"].clone();
                self.workspaces.push(created);
                raw(
                    200,
                    &json!({"id": id, "name": name, "slug": slug, "description": null,
                    "created_at": "2026-10-20T00:00:00Z", "last_updated_at": "2026-10-20T00:00:00Z",
                    "scope_name": body["scope_name"], "object": "workspace"}),
                )
            }
            ("DELETE", ["workspaces", id]) if admin => {
                match self.workspaces.iter_mut().find(|w| w["id"] == *id) {
                    Some(w) => {
                        w["status"] = json!("archived");
                        no_content()
                    }
                    None => raw(404, &json!({"error": "not found"})),
                }
            }
            ("GET", ["integrations"]) if admin => raw(200, &listing(vec![integration()])),
            ("GET", ["logs", "charts", _]) => raw(
                200,
                &json!({"success": true, "data": {"records": [
                {"x": "2026-10-20T00:00:00Z", "y": 1}], "total": 1, "total_llm_count": 1,
                "total_llm_error_count": 0, "isQuotaExceeded": false}}),
            ),
            ("GET", ["logs", "groups", _]) => raw(
                200,
                &json!({"object": "list", "is_quota_exceeded": false,
                "total": 0, "data": []}),
            ),
            ("GET", ["logs"]) => raw(
                200,
                &json!({"success": true, "data": {"records": [], "total": 0,
                "capturedTotal": 0, "isQuotaExceeded": false}}),
            ),
            ("GET", ["policies", _]) => {
                raw(200, &json!({"object": "list", "total": 0, "data": []}))
            }
            (
                _,
                [
                    collection @ ("providers" | "configs" | "guardrails"),
                    tail @ ..,
                ],
            ) => {
                let name: &'static str = match *collection {
                    "providers" => "providers",
                    "configs" => "configs",
                    _ => "guardrails",
                };
                self.collection(method, name, tail, &ws, body)
            }
            (_, ["api-keys", kind @ ("service" | "user"), tail @ ..]) => {
                let name = if *kind == "service" {
                    "api-keys/service"
                } else {
                    "api-keys/user"
                };
                self.collection(method, name, tail, &ws, body)
            }
            _ => raw(404, &json!({"error": "not found"})),
        }
    }

    fn collection(
        &mut self,
        method: &str,
        name: &'static str,
        tail: &[&str],
        ws: &str,
        body: &Value,
    ) -> Reply {
        match (method, tail) {
            ("GET", []) => {
                let rows: Vec<Value> = self.items[name]
                    .iter()
                    .filter(|i| i["workspace_id"] == ws)
                    .cloned()
                    .collect();
                raw(200, &listing(rows))
            }
            ("GET", [id]) => match self.items[name].iter().find(|i| i["id"] == *id) {
                Some(i) => raw(200, i),
                None => raw(404, &json!({"error": "not found"})),
            },
            ("POST", []) => {
                let id = self.id();
                let mut created = match name {
                    "providers" => provider(&id, ws, "new"),
                    "configs" => route(&id, ws, &json!({})),
                    "guardrails" => guardrail(&id, ws),
                    _ => key(&id, ws, "new"),
                };
                merge(&mut created, body);
                if name == "configs" {
                    created["config"] = json!(body["config"].to_string());
                }
                created["id"] = json!(id);
                let slug = created["name"].clone();
                self.items.get_mut(name).expect("collection").push(created);
                raw(
                    200,
                    &match name {
                        "providers" => json!({"id": id, "slug": slug, "object": "provider"}),
                        "configs" | "guardrails" => {
                            json!({"id": id, "version_id": "v-1", "slug": slug, "object": "config"})
                        }
                        _ => {
                            json!({"id": id, "success": true, "key": format!("gw-ONE-TIME-{id}"), "object": "api-key"})
                        }
                    },
                )
            }
            ("POST", [id, "rotate"]) => {
                if !self.items[name].iter().any(|i| i["id"] == *id) {
                    return raw(404, &json!({"error": "not found"}));
                }
                raw(
                    200,
                    &json!({"id": id, "key": "gw-ONE-TIME-rotated",
                    "key_transition_expires_at": "2026-10-21T00:00:00Z"}),
                )
            }
            ("PUT", [id]) => {
                let Some(item) = self
                    .items
                    .get_mut(name)
                    .and_then(|v| v.iter_mut().find(|i| i["id"] == *id))
                else {
                    return raw(404, &json!({"error": "not found"}));
                };
                merge(item, body);
                if name == "configs" {
                    if let Some(config) = body.get("config") {
                        item["config"] = json!(config.to_string());
                    }
                    item["version_id"] = json!("v-2");
                }
                raw(200, &json!({"success": true, "id": id}))
            }
            ("DELETE", [id]) => {
                self.items
                    .get_mut(name)
                    .expect("collection")
                    .retain(|i| i["id"] != *id);
                no_content()
            }
            _ => raw(404, &json!({"error": "not found"})),
        }
    }

    fn inference(&mut self, method: &str, rest: &str, body: &Value) -> Reply {
        if method != "POST" || rest != "/chat/completions" {
            return raw(404, &json!({"error": "not found"}));
        }
        if body["model"] != MODEL {
            return raw(
                404,
                &json!({"error": {"message": "model not found", "type": "invalid_request_error"}}),
            );
        }
        self.serial += 1;
        let id = format!("chatcmpl-{}", self.serial);
        let usage = json!({"prompt_tokens": 3, "completion_tokens": self.tokens - 3, "total_tokens": self.tokens});
        match self.chat {
            Chat::Text => sse(vec![
                chunk(&id, Some("ready"), None, None),
                chunk(&id, None, Some("stop"), Some(usage)),
                b"data: [DONE]\n\n".to_vec(),
            ]),
            Chat::Paced => Reply::Stream {
                head: "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\nconnection: close"
                    .into(),
                parts: (0..400).map(|i| (25, chunk(&id, Some(&format!("{i} ")), None, None))).collect(),
                hang: true,
            },
        }
    }
}

/// The `json` part of a dictionary upload.
fn multipart_json(seen: &Seen) -> Value {
    let text = String::from_utf8_lossy(&seen.body);
    text.split("\r\n\r\n")
        .nth(1)
        .and_then(|part| part.split("\r\n--").next())
        .and_then(|json| serde_json::from_str(json).ok())
        .unwrap_or(Value::Null)
}

fn sessions(path: &str, q: &dyn Fn(&str) -> String) -> Reply {
    // Sessions of two applications, each with the time it was last seen.
    let row = |n: usize| {
        json!({"session_id": format!("ses-{n}"), "application_id": format!("app-{}", n % 2),
        "application_name": format!("Bot {}", n % 2), "status": "block",
        "last_seen": format!("2026-10-25T0{n}:00:00Z"),
        "detection_type_violation_breakdown": [{"detection_type": "dlp", "violation_breakdown": {"total": 0}}]})
    };
    if path == format!("{DASH}sessionsoverview") {
        let items: Vec<Value> = (0..3).map(row).collect();
        return raw(
            200,
            &json!({"items": items, "pagination": {"total_items": 3}}),
        );
    }
    if path == format!("{DASH}session") {
        let session = q("session_id");
        let actions: Vec<Value> = (0..2)
            .map(|k| json!({"scan_id": format!("scn-{session}-{k}"), "scan_sub_req_id": 0, "session_id": session,
                "application_id": q("app_id"), "status": "allow", "profile_name": "Production guardrails",
                "detection_type_violation_breakdown": [], "violation_breakdown": {"total": 0}}))
            .collect();
        return raw(
            200,
            &json!({"session_id": session, "application_id": q("app_id"),
            "application_name": q("app_name"), "session_actions": actions}),
        );
    }
    if path == format!("{DASH}sessiontransaction") {
        return raw(
            200,
            &json!({"scan_id": q("scan_id"), "scan_sub_req_id": 0, "verdict": "benign",
            "prompt": "SYNTHETIC-PROMPT-c25", "response": "SYNTHETIC-RESPONSE-c25",
            "scan_contents": [{"prompt": "SYNTHETIC-PROMPT-c25", "response": "SYNTHETIC-RESPONSE-c25"}]}),
        );
    }
    if path == "/aisec/v1/mgmt/reports/scancontent" {
        return raw(
            200,
            &json!({"scan_id": q("scan_id"), "report_id": "R-1", "category": "benign",
            "scan_contents": {"prompt": "SYNTHETIC-PROMPT-c25", "response": "SYNTHETIC-RESPONSE-c25"}}),
        );
    }
    raw(404, &json!({"error": "not found"}))
}

fn red_team(path: &str) -> Option<Reply> {
    const RT: &str = "/ai-red-teaming/data-plane";
    let reply = match path {
        p if p == format!("{RT}/v1/dashboard/scan-statistics") => {
            json!({"total_scans": 1, "targets_scanned": 1,
            "scan_status": [{"name": "COMPLETED", "count": 1}], "risk_profile": [],
            "targets_scanned_by_type": [{"name": "APPLICATION", "count": 1}]})
        }
        p if p == format!("{RT}/v1/scan") => json!({"data": [{"uuid": RED_TEAM_SCAN, "tsg_id": TSG,
            "name": "Seeded campaign", "target": {"name": "Seeded target"},
            "target_id": "22222222-3333-4444-8555-666666666666", "target_type": "APPLICATION",
            "job_type": "STATIC", "status": "COMPLETED", "created_at": "2026-10-18T10:00:00Z"}],
            "pagination": {"total_items": 1}}),
        p if p == format!("{RT}/v1/report/static/{RED_TEAM_SCAN}/list-attacks") => json!({"data": [
            {"uuid": RED_TEAM_SCAN, "job_id": RED_TEAM_SCAN, "prompt": "SEEDED-ATTACK", "threat": false,
             "severity": "LOW", "attack_type": "NORMAL", "category": "security", "sub_category": "JAILBREAK",
             "sub_category_display_name": "Jailbreak", "multi_turn": false}],
            "pagination": {"total_items": 1}}),
        "/aims/data/v1/scans" => json!({"scans": [], "pagination": {"total_items": 0}}),
        "/aiag/data/v1/stats/scans" => {
            json!({"unique_skills_scanned": {"count": 0, "percent_change": null},
            "total_vulnerabilities_found": {"count": 0, "percent_change": null}, "top_vulnerability": null})
        }
        "/aiag/data/v1/scans" => json!({"scans": [], "pagination": {"total_items": 0}}),
        _ => return None,
    };
    Some(raw(200, &reply))
}
