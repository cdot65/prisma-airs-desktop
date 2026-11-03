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

/// What the engine is told when the operator declines a read (C32).
pub const DECLINED: &str = "The operator declined this read, so nothing was read.";

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
        "dataPatterns","dataProfiles","filteringProfiles","dictionaries","gatewayGuardrails"]});
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
            function("mcpServers","The MCP servers of this conversation's AI Gateway workspace (names, namespaces and status), as the workspace's MCP catalog lists them.",json!({}),&[]),
            function("proposeChange","Stage a change for the operator to review: a Runtime Security or DLP change, an additional API key for an application (action version: the start of a key rotation, which the operator finishes in Administration), or a guardrail of this conversation's AI Gateway workspace (create or update). It is never applied by this tool: the operator must review and confirm it in the desktop.",
                json!({"kind":kinds,"action":{"type":"string","enum":["create","update","delete","version"]},
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
    /// The conversation's workspace MCP catalog (`GatewayRead::McpServers`, C32).
    McpServers,
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
            "mcpServers" => {
                let _: Empty = serde_json::from_value(args).ok()?;
                Self::McpServers
            }
            "proposeChange" => {
                let a: ProposeArgs = serde_json::from_value(args).ok()?;
                // The Runtime and DLP kinds (create, update, delete); an API
                // key's version, the start of a rotation the operator
                // finishes in Administration (C32); and a guardrail of the
                // conversation's own Gateway workspace, created or updated,
                // reviewed under that workspace (C32). Nothing else.
                let runtime = matches!(
                    a.kind,
                    ChangeKind::Profiles
                        | ChangeKind::Topics
                        | ChangeKind::Applications
                        | ChangeKind::ApiKeys
                        | ChangeKind::DataPatterns
                        | ChangeKind::DataProfiles
                        | ChangeKind::FilteringProfiles
                        | ChangeKind::Dictionaries
                ) && matches!(
                    a.action,
                    ChangeAction::Create | ChangeAction::Update | ChangeAction::Delete
                );
                let rotation = a.kind == ChangeKind::ApiKeys && a.action == ChangeAction::Version;
                let guardrail = a.kind == ChangeKind::GatewayGuardrails
                    && matches!(a.action, ChangeAction::Create | ChangeAction::Update);
                if !(runtime || rotation || guardrail)
                    || a.summary.trim().is_empty()
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

    /// Whether the call waits for the operator's decision in the page
    /// (C32): every read of the tenant does; staging a proposal reads
    /// nothing and only ever opens the operator's own review.
    pub fn needs_decision(&self) -> bool {
        !matches!(self, Self::ProposeChange { .. })
    }

    /// The tool's name, as the engine called it.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Inventory { .. } => "inventory",
            Self::Detail { .. } => "detail",
            Self::Session { .. } => "session",
            Self::Search { .. } => "search",
            Self::RedTeam { .. } => "redTeam",
            Self::SupplyChain => "supplyChain",
            Self::Scan { .. } => "scan",
            Self::Cases => "cases",
            Self::McpServers => "mcpServers",
            Self::ProposeChange { .. } => "proposeChange",
        }
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
            Self::McpServers => "Read the workspace's MCP servers".to_owned(),
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
        // Still JSON: the cut text is a string beside a note. Escaping can
        // grow it (quotes, backslashes, control characters), so the prefix
        // shrinks until the whole envelope fits (C21 review R2-F4).
        let mut end = MAX_RESULT_BYTES - 512;
        loop {
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            let envelope = json!({"cut": CUT_NOTE, "partial": &text[..end]}).to_string();
            if envelope.len() <= MAX_RESULT_BYTES {
                return envelope;
            }
            // Each pass removes at least the overflow, which escaping at
            // most sextuples.
            let over = envelope.len() - MAX_RESULT_BYTES;
            end = end.saturating_sub(over.div_ceil(6).max(1));
        }
    }
    text
}

const CUT_NOTE: &str = "The result was longer than 24 KiB; ask for a smaller page.";

/// What stands in for JSON carried as text that is too deep or too large
/// to open and check: it is never passed on unchecked (C21 review R2-F2).
const UNCHECKED: &str = "[nested data omitted: too deep or too large to check]";

/// JSON carried as text (a page's items, a detail, a session) as values,
/// so the scrub reaches every field before anything goes to the model
/// (C21 review F2). Bounded in size and in how deep text is opened.
fn structured(value: &mut Value, depth: usize) {
    match value {
        Value::String(s) if matches!(s.trim_start().as_bytes().first(), Some(b'{' | b'[')) => {
            if depth >= 4 || s.len() > 4 * 1024 * 1024 {
                // Past the bounds: replaced, never forwarded unchecked.
                *value = Value::String(UNCHECKED.to_owned());
            } else if let Ok(parsed @ (Value::Object(_) | Value::Array(_))) =
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
        // Encoded deeper than it is opened, or larger: replaced, never
        // passed on (C21 review R2-F2).
        let mut deep = json!({"Authorization": "Bearer FIVE-DEEP-CANARY"}).to_string();
        for _ in 0..5 {
            deep = json!({"wrapped": deep}).to_string();
        }
        let big = json!({"Authorization": "Bearer BIG-CANARY", "pad": "x".repeat(4 * 1024 * 1024)})
            .to_string();
        let text = result_text(
            json!({"name": "Runtime", "metadata": deep, "large": big,
            "note": "safe-metadata"}),
            &[],
        );
        assert!(
            !text.contains("FIVE-DEEP-CANARY") && !text.contains("BIG-CANARY"),
            "{text}"
        );
        assert!(
            text.contains(UNCHECKED) && text.contains("safe-metadata") && text.contains("Runtime")
        );
        // The cut envelope fits its bound whatever escaping costs (C21
        // review R2-F4), and stays valid JSON.
        for heavy in [
            "\"".repeat(30_000),
            "\\".repeat(30_000),
            "\u{1}".repeat(30_000),
            "é".repeat(30_000),
        ] {
            let text = result_text(json!({"t": heavy}), &[]);
            assert!(text.len() <= MAX_RESULT_BYTES, "{}", text.len());
            let cut: Value = serde_json::from_str(&text).expect("still JSON");
            assert_eq!(cut["cut"], CUT_NOTE);
        }
    }

    /// C32: the Harness may hand off a key rotation (an API key version)
    /// and propose a guardrail of its own Gateway workspace (create or
    /// update); every other Gateway change, and a key rotation in place,
    /// stays out of its reach.
    #[test]
    fn proposals_admit_the_rotation_hand_off_and_workspace_guardrails_only() {
        let ns = Some(NAMESPACE);
        let call = |kind: &str, action: &str| {
            ToolCall::parse(
                ns,
                "proposeChange",
                &json!({"kind": kind, "action": action, "target": if action == "create" { "" } else { "x-1" },
                        "body": {}, "summary": "s"}),
            )
        };
        assert!(call("apiKeys", "version").is_some());
        assert!(call("gatewayGuardrails", "create").is_some());
        assert!(call("gatewayGuardrails", "update").is_some());
        assert!(call("topics", "update").is_some());
        for (kind, action) in [
            ("gatewayGuardrails", "delete"),
            ("gatewayRoutes", "create"),
            ("gatewayServiceKeys", "rotate"),
            ("gatewayWorkspaces", "create"),
            ("apiKeys", "rotate"),
            ("topics", "version"),
        ] {
            assert!(call(kind, action).is_none(), "{kind} {action}");
        }
    }
}
