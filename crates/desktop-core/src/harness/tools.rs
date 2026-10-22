//! The typed host tools the Harness may call (C21, D048).
//!
//! One namespace, `airs_desktop`, of read tools over the desktop's own
//! management, case library and scanning paths, plus one that stages a
//! change for the operator's review. Every schema forbids unknown fields;
//! arguments are parsed into a closed `ToolCall`, bounded, and refused when
//! they carry anything credential-like. Nothing here performs a call: the
//! authority admits it under the conversation's binding, and only the
//! operator can review and apply a proposal. Results are scrubbed of
//! credential-like fields and bounded before the model sees them.

use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::changes::{ChangeAction, ChangeKind};
use crate::management::ManagementResource;

pub const NAMESPACE: &str = "airs_desktop";
/// The largest tool arguments accepted (serialized).
pub const MAX_ARGUMENTS_BYTES: usize = 40 * 1024;
/// The largest result handed back (the engine's own bound is 32 KiB).
pub const MAX_RESULT_BYTES: usize = 24 * 1024;
/// Rows of an inventory page a tool may ask for.
pub const MAX_TOOL_PAGE: u32 = 50;
/// Scan text a tool may submit (each of prompt and response).
pub const MAX_SCAN_TEXT: usize = 16 * 1024;
/// What every refused call answers (no reason that could steer the model
/// around a check).
pub const UNAVAILABLE: &str = "This tool or its arguments are unavailable.";

/// Field names whose values are never shown to the model.
const SENSITIVE: [&str; 14] = [
    "api_key",
    "apikey",
    "auth_code",
    "authorization",
    "client_secret",
    "password",
    "private_key",
    "refresh_token",
    "access_token",
    "secret",
    "token",
    "x-pan-token",
    "credential",
    "credentials",
];

fn sensitive(key: &str) -> bool {
    let key = key.to_ascii_lowercase().replace('-', "_");
    SENSITIVE
        .iter()
        .any(|s| key == *s || key.ends_with(&format!("_{s}")))
}

/// A tool's function entry.
fn function(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({"type":"function","name":name,"description":description,
        "inputSchema":{"type":"object","properties":properties,"required":required,
            "additionalProperties":false}})
}

const RESOURCES: [&str; 10] = [
    "profiles",
    "topics",
    "applications",
    "apiKeys",
    "deploymentProfiles",
    "dataPatterns",
    "dataProfiles",
    "filteringProfiles",
    "dictionaries",
    "sessions",
];

/// The catalog given to the engine (its `dynamicTools`).
pub fn catalog() -> Value {
    let resource = json!({"type":"string","enum":RESOURCES});
    let id = json!({"type":"string","minLength":1,"maxLength":256});
    let kinds = json!({"type":"string","enum":["profiles","topics","applications","apiKeys",
        "dataPatterns","dataProfiles","filteringProfiles","dictionaries"]});
    json!([{
        "type":"namespace",
        "name":NAMESPACE,
        "description":"Read the active Prisma AIRS tenant through the desktop, and stage changes for the operator to review. Nothing here applies a change.",
        "tools":[
            function("inventory","One page of a Runtime Security inventory of the active tenant (secrets are never included).",
                json!({"resource":resource,"offset":{"type":"integer","minimum":0,"maximum":1000000},
                    "limit":{"type":"integer","minimum":1,"maximum":MAX_TOOL_PAGE}}),&["resource"]),
            function("detail","One item of a Runtime Security inventory, by its identifier.",
                json!({"resource":resource,"id":id}),&["resource","id"]),
            function("session","One Runtime session's actions (metadata only; prompt and response content stay with the operator).",
                json!({"sessionId":id,"appId":id,"appName":id}),&["sessionId","appId","appName"]),
            function("search","Find Runtime sessions by session ID, scan ID or detector.",
                json!({"query":{"type":"string","minLength":1,"maxLength":256}}),&["query"]),
            function("redTeam","AI Red Teaming statistics and the first scans over the last 7, 15 or 30 days.",
                json!({"window":{"type":"integer","enum":[7,15,30]}}),&["window"]),
            function("supplyChain","Model Security and AgentGuard scan statistics.",json!({}),&[]),
            function("scan","Scan a prompt and/or response with one of the tenant's Runtime Security profiles (by name) and return the verdict.",
                json!({"profile":{"type":"string","minLength":1,"maxLength":100},
                    "prompt":{"type":"string","maxLength":MAX_SCAN_TEXT},
                    "response":{"type":"string","maxLength":MAX_SCAN_TEXT}}),&["profile"]),
            function("cases","The tenant's saved test cases (names and verdict expectations).",json!({}),&[]),
            function("proposeChange","Stage a Runtime Security change for the operator to review. It is never applied by this tool: the operator must review and confirm it in the desktop.",
                json!({"kind":kinds,"action":{"type":"string","enum":["create","update","delete"]},
                    "target":{"type":"string","maxLength":256},
                    "body":{"type":"object"},
                    "summary":{"type":"string","maxLength":512}}),&["kind","action","summary"]),
        ]
    }])
}

/// A parsed, bounded tool call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolCall {
    Inventory {
        resource: ManagementResource,
        offset: u32,
        limit: u32,
    },
    Detail {
        resource: ManagementResource,
        id: String,
    },
    Session {
        session_id: String,
        app_id: String,
        app_name: String,
    },
    Search {
        query: String,
    },
    RedTeam {
        window: u8,
    },
    SupplyChain,
    Scan {
        profile: String,
        prompt: Option<String>,
        response: Option<String>,
    },
    Cases,
    ProposeChange {
        kind: ChangeKind,
        action: ChangeAction,
        target: String,
        /// The body as JSON object text.
        body: String,
        summary: String,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InventoryArgs {
    resource: ManagementResource,
    #[serde(default)]
    offset: u32,
    limit: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DetailArgs {
    resource: ManagementResource,
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SessionArgs {
    session_id: String,
    app_id: String,
    app_name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchArgs {
    query: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RedTeamArgs {
    window: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScanArgs {
    profile: String,
    prompt: Option<String>,
    response: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposeArgs {
    kind: ChangeKind,
    action: ChangeAction,
    #[serde(default)]
    target: String,
    #[serde(default)]
    body: Option<Map<String, Value>>,
    summary: String,
}

/// The inventories the tools offer (the schema's enum, enforced).
fn offered(resource: ManagementResource) -> bool {
    !matches!(resource, ManagementResource::DlpCatalog)
}

fn plain(text: &str) -> bool {
    !text.is_empty() && text.len() <= 256 && !text.chars().any(char::is_control)
}

/// Whether a value carries anything credential-like anywhere.
pub fn has_credentials(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(k, v)| sensitive(k) || has_credentials(v)),
        Value::Array(items) => items.iter().any(has_credentials),
        _ => false,
    }
}

impl ToolCall {
    /// The call the engine asked for, or `None` (refused with
    /// [`UNAVAILABLE`]).
    pub fn parse(namespace: Option<&str>, tool: &str, arguments: &Value) -> Option<Self> {
        if namespace != Some(NAMESPACE)
            || serde_json::to_vec(arguments).map_or(true, |b| b.len() > MAX_ARGUMENTS_BYTES)
            || !arguments.is_object()
            || has_credentials(arguments)
        {
            return None;
        }
        let args = arguments.clone();
        let call = match tool {
            "inventory" => {
                let a: InventoryArgs = serde_json::from_value(args).ok()?;
                let limit = a.limit.unwrap_or(25);
                if !offered(a.resource)
                    || !(1..=MAX_TOOL_PAGE).contains(&limit)
                    || a.offset > 1_000_000
                {
                    return None;
                }
                Self::Inventory {
                    resource: a.resource,
                    offset: a.offset,
                    limit,
                }
            }
            "detail" => {
                let a: DetailArgs = serde_json::from_value(args).ok()?;
                (offered(a.resource) && plain(&a.id)).then_some(Self::Detail {
                    resource: a.resource,
                    id: a.id,
                })?
            }
            "session" => {
                let a: SessionArgs = serde_json::from_value(args).ok()?;
                (plain(&a.session_id) && plain(&a.app_id) && plain(&a.app_name)).then_some(
                    Self::Session {
                        session_id: a.session_id,
                        app_id: a.app_id,
                        app_name: a.app_name,
                    },
                )?
            }
            "search" => {
                let a: SearchArgs = serde_json::from_value(args).ok()?;
                plain(&a.query).then_some(Self::Search { query: a.query })?
            }
            "redTeam" => {
                let a: RedTeamArgs = serde_json::from_value(args).ok()?;
                matches!(a.window, 7 | 15 | 30).then_some(Self::RedTeam { window: a.window })?
            }
            "supplyChain" => {
                let _: Empty = serde_json::from_value(args).ok()?;
                Self::SupplyChain
            }
            "scan" => {
                let a: ScanArgs = serde_json::from_value(args).ok()?;
                let ok = |t: &Option<String>| t.as_ref().is_none_or(|t| t.len() <= MAX_SCAN_TEXT);
                let any = a.prompt.as_ref().is_some_and(|t| !t.trim().is_empty())
                    || a.response.as_ref().is_some_and(|t| !t.trim().is_empty());
                (any && plain(&a.profile) && ok(&a.prompt) && ok(&a.response)).then_some(
                    Self::Scan {
                        profile: a.profile,
                        prompt: a.prompt,
                        response: a.response,
                    },
                )?
            }
            "cases" => {
                let _: Empty = serde_json::from_value(args).ok()?;
                Self::Cases
            }
            "proposeChange" => {
                let a: ProposeArgs = serde_json::from_value(args).ok()?;
                // Only the Runtime and DLP kinds: Gateway changes need the
                // operator's own workspace review.
                if !matches!(
                    a.kind,
                    ChangeKind::Profiles
                        | ChangeKind::Topics
                        | ChangeKind::Applications
                        | ChangeKind::ApiKeys
                        | ChangeKind::DataPatterns
                        | ChangeKind::DataProfiles
                        | ChangeKind::FilteringProfiles
                        | ChangeKind::Dictionaries
                ) || !matches!(
                    a.action,
                    ChangeAction::Create | ChangeAction::Update | ChangeAction::Delete
                ) || a.summary.trim().is_empty()
                    || a.summary.chars().count() > 512
                    || a.target.len() > 256
                    || a.target.chars().any(char::is_control)
                {
                    return None;
                }
                let body = Value::Object(a.body.unwrap_or_default()).to_string();
                Self::ProposeChange {
                    kind: a.kind,
                    action: a.action,
                    target: a.target,
                    body,
                    summary: a.summary.chars().filter(|c| !c.is_control()).collect(),
                }
            }
            _ => return None,
        };
        Some(call)
    }

    /// A short, model-free description for the transcript.
    pub fn title(&self) -> String {
        match self {
            Self::Inventory { resource, .. } => {
                format!("Read the {} inventory", resource_name(*resource))
            }
            Self::Detail { resource, id } => format!("Read {} {id}", resource_name(*resource)),
            Self::Session { session_id, .. } => format!("Read session {session_id}"),
            Self::Search { query } => format!("Searched sessions for {query}"),
            Self::RedTeam { window } => format!("Read AI Red Teaming ({window} days)"),
            Self::SupplyChain => "Read Model Security and AgentGuard".to_owned(),
            Self::Scan { .. } => "Scanned text with Runtime Security".to_owned(),
            Self::Cases => "Read the test case library".to_owned(),
            Self::ProposeChange { summary, .. } => format!("Proposed a change: {summary}"),
        }
    }
}

fn resource_name(resource: ManagementResource) -> &'static str {
    match resource {
        ManagementResource::Profiles => "profiles",
        ManagementResource::Topics => "topics",
        ManagementResource::Applications => "applications",
        ManagementResource::ApiKeys => "API keys",
        ManagementResource::DeploymentProfiles => "deployment profiles",
        ManagementResource::DlpCatalog => "DLP catalog",
        ManagementResource::DataProfiles => "data profiles",
        ManagementResource::DataPatterns => "data patterns",
        ManagementResource::FilteringProfiles => "filtering profiles",
        ManagementResource::Dictionaries => "dictionaries",
        ManagementResource::Sessions => "sessions",
    }
}

/// Removes credential-like fields at any depth.
pub fn scrub(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|k, _| !sensitive(k));
            map.values_mut().for_each(scrub);
        }
        Value::Array(items) => items.iter_mut().for_each(scrub),
        _ => {}
    }
}

/// A result for the model: scrubbed, with any of `secrets` removed from its
/// text, and cut to [`MAX_RESULT_BYTES`] with a note.
pub fn result_text(mut value: Value, secrets: &[&str]) -> String {
    structured(&mut value, 0);
    scrub(&mut value);
    let mut text = value.to_string();
    for secret in secrets.iter().filter(|s| !s.is_empty()) {
        text = text.replace(secret, "[redacted]");
    }
    if text.len() > MAX_RESULT_BYTES {
        // Still JSON: the cut text is a string beside a note.
        let mut end = MAX_RESULT_BYTES - 512;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text = json!({"cut": "The result was longer than 24 KiB; ask for a smaller page.",
            "partial": text})
        .to_string();
    }
    text
}

/// JSON carried as text (a page's items, a detail, a session) as values,
/// so the scrub reaches every field before anything goes to the model
/// (C21 review F2). Bounded in size and in how deep text is opened.
fn structured(value: &mut Value, depth: usize) {
    match value {
        Value::String(s)
            if depth < 4
                && s.len() <= 4 * 1024 * 1024
                && matches!(s.trim_start().as_bytes().first(), Some(b'{' | b'[')) =>
        {
            if let Ok(parsed @ (Value::Object(_) | Value::Array(_))) =
                serde_json::from_str::<Value>(s)
            {
                *value = parsed;
                structured(value, depth + 1);
            }
        }
        Value::Object(map) => map.values_mut().for_each(|v| structured(v, depth)),
        Value::Array(items) => items.iter_mut().for_each(|v| structured(v, depth)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_fits_the_engine_and_forbids_unknown_fields() {
        let c = catalog();
        let bytes = serde_json::to_vec(&c).expect("json").len();
        assert!(bytes <= 40_000, "{bytes}");
        let tools = c[0]["tools"].as_array().expect("tools");
        assert!(tools.len() <= 64);
        for t in tools {
            assert_eq!(t["inputSchema"]["additionalProperties"], false, "{t}");
            assert!(serde_json::to_vec(t).expect("json").len() <= 4096, "{t}");
        }
    }

    #[test]
    fn calls_are_typed_bounded_and_never_credential_bearing() {
        let ns = Some(NAMESPACE);
        assert_eq!(
            ToolCall::parse(ns, "inventory", &json!({"resource":"topics","limit":10})),
            Some(ToolCall::Inventory {
                resource: ManagementResource::Topics,
                offset: 0,
                limit: 10
            })
        );
        for (name, args) in [
            ("inventory", json!({"resource":"topics","limit":51})),
            ("inventory", json!({"resource":"topics","extra":1})),
            ("inventory", json!({"resource":"secrets"})),
            ("inventory", json!({"resource":"dlpCatalog"})),
            (
                "proposeChange",
                json!({"kind":"apiKeys","action":"rotate","target":"k","summary":"s"}),
            ),
            ("detail", json!({"resource":"topics","id":""})),
            ("detail", json!({"resource":"topics","id":"a\nb"})),
            ("redTeam", json!({"window":8})),
            ("scan", json!({"profile":"p"})),
            ("scan", json!({"prompt":"hi"})),
            (
                "scan",
                json!({"profile":"p","prompt":"x".repeat(MAX_SCAN_TEXT + 1)}),
            ),
            (
                "proposeChange",
                json!({"kind":"gatewayProviders","action":"create","summary":"s"}),
            ),
            (
                "proposeChange",
                json!({"kind":"topics","action":"create","summary":"s","body":{"api_key":"k"}}),
            ),
            (
                "proposeChange",
                json!({"kind":"topics","action":"apply","summary":"s"}),
            ),
            ("apply", json!({})),
            ("content", json!({"scanId":"1","subRequest":0})),
        ] {
            assert_eq!(ToolCall::parse(ns, name, &args), None, "{name} {args}");
        }
        assert_eq!(ToolCall::parse(Some("other"), "cases", &json!({})), None);
        assert_eq!(ToolCall::parse(None, "cases", &json!({})), None);
        // Only the size refuses this one: its body is otherwise accepted.
        let sized = |n: usize| json!({"kind":"topics","action":"create","summary":"s","body":{"description":"d".repeat(n)}});
        assert!(ToolCall::parse(ns, "proposeChange", &sized(MAX_ARGUMENTS_BYTES - 200)).is_some());
        assert_eq!(
            ToolCall::parse(ns, "proposeChange", &sized(MAX_ARGUMENTS_BYTES)),
            None
        );
    }

    #[test]
    fn results_are_scrubbed_redacted_and_bounded() {
        let v = json!({"keys":[{"api_key_name":"Runtime","api_key":"SECRET-VALUE","auth_code":"X",
            "nested":{"client_secret":"Y","name":"ok"}}],"note":"has KEY-1 inside"});
        let text = result_text(v, &["KEY-1"]);
        assert!(
            !text.contains("SECRET-VALUE") && !text.contains("\"X\"") && !text.contains("\"Y\"")
        );
        assert!(text.contains("Runtime") && text.contains("[redacted]") && !text.contains("KEY-1"));
        let long = result_text(json!({"t":"z".repeat(MAX_RESULT_BYTES * 2)}), &[]);
        assert!(long.len() <= MAX_RESULT_BYTES);
        let cut: Value = serde_json::from_str(&long).expect("still JSON");
        assert!(cut["cut"].as_str().is_some_and(|c| c.contains("24 KiB")));
        // Credentials inside JSON carried as text (a page's items, a
        // detail) are found too, whatever their case or nesting.
        let page = json!({"page": {"items": json!([{"name": "Runtime", "metadata": {
            "Authorization": "Bearer NESTED-CANARY", "X-Api-Key": "MIXED-CANARY",
            "note": "safe-metadata", "inner": json!({"Client-Secret": "DEEP-CANARY"}).to_string()}}]).to_string()}});
        let text = result_text(page, &[]);
        for canary in ["NESTED-CANARY", "MIXED-CANARY", "DEEP-CANARY"] {
            assert!(!text.contains(canary), "{canary}: {text}");
        }
        assert!(text.contains("safe-metadata") && text.contains("Runtime"));
    }
}
