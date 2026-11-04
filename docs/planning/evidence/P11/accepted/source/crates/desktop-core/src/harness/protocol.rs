//! The pinned engine's `airs-desktop/1` wire (engine 9331a58, D046): one
//! JSON object per line in each direction. The bootstrap frame is the only
//! one that carries a credential; it is encoded into a single zeroizing
//! buffer of the frame's bound, so no partial copy of the key is freed
//! without being cleared. Every incoming frame is parsed and bounded here
//! before any of it reaches a view.

use serde::Deserialize;
use serde_json::{Map, Value, json};
use zeroize::{Zeroize, Zeroizing};

use super::HarnessFailure;

/// The bootstrap frame and its receipt (the engine's `FRAME_LIMIT`).
pub const FRAME_LIMIT: usize = 64 * 1024;
/// Any later frame (the engine's `REPLAY_LIMIT`).
pub const EVENT_LIMIT: usize = 4 * 1024 * 1024;
pub const PROTOCOL: &str = "airs-desktop/1";
/// The engine's credential bound (it refuses a longer key).
pub const MAX_KEY_BYTES: usize = 16 * 1024;
/// The engine accepts at most eight 8 KiB text items per turn; v1's native
/// prompt bound is 32 KiB.
pub const MAX_PROMPT_BYTES: usize = 32 * 1024;
const TEXT_ITEM_BYTES: usize = 8 * 1024;

/// What a conversation is bound to, as the engine names it. The engine
/// echoes it in its receipt, and the receipt must match exactly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub tenant: String,
    pub workspace: String,
    pub credential: String,
    pub credential_epoch: u64,
    pub endpoint: String,
    /// The selected project (canonical absolute path), if any (C21).
    pub project: Option<String>,
    /// The operator allowed changes in the project, with approval (C21).
    pub write: bool,
    /// The workspace MCP servers, in bootstrap order (C22): fixed for the
    /// conversation. The engine's context holds each one's name, ID and
    /// URL; its revision and grant are checked by the desktop.
    pub mcp: Vec<super::mcp::ScopeServer>,
}

impl Binding {
    fn local_access(&self) -> &'static str {
        if self.write {
            "workspaceWrite"
        } else {
            "readOnly"
        }
    }
}

/// A UUID derived from a native identifier for one purpose (the engine
/// requires UUIDs; v2 identifiers are not). SHA-256 of the domain and the
/// identifier, shaped as an RFC 9562 version 8 UUID.
pub fn derived_uuid(domain: &str, id: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(b"airs-v2/harness/");
    h.update(domain.as_bytes());
    h.update([0]);
    h.update(id.as_bytes());
    let mut b: [u8; 16] = h.finalize()[..16].try_into().expect("16 bytes");
    b[6] = (b[6] & 0x0f) | 0x80;
    b[8] = (b[8] & 0x3f) | 0x80;
    let x = hex::encode(b);
    format!(
        "{}-{}-{}-{}-{}",
        &x[..8],
        &x[8..12],
        &x[12..16],
        &x[16..20],
        &x[20..]
    )
}

/// A lowercase hyphenated UUID that is not nil.
pub fn is_uuid(text: &str) -> bool {
    text.len() == 36
        && text.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit() && !b.is_ascii_uppercase(),
        })
        && text.bytes().any(|b| b != b'0' && b != b'-')
}

/// The bootstrap frame with its newline, in one zeroizing buffer that
/// never grows past [`FRAME_LIMIT`]. Each scope server gets its token, or
/// the signed-out placeholder (C22).
pub fn bootstrap(
    binding: &Binding,
    key: &str,
    mcp: &[Option<Zeroizing<String>>],
) -> Result<Zeroizing<Vec<u8>>, HarnessFailure> {
    if key.is_empty() || key.len() > MAX_KEY_BYTES || key.chars().any(char::is_control) {
        return Err(HarnessFailure::CredentialMissing);
    }
    if binding.mcp.len() > super::mcp::MAX_SERVERS || mcp.len() != binding.mcp.len() {
        return Err(HarnessFailure::Protocol);
    }
    let mut value = json!({"id":0,"method":"desktop/bootstrap","params":{"v":1,
        "tenantId":binding.tenant,"workspaceId":binding.workspace,
        "credentialId":binding.credential,"credentialEpoch":binding.credential_epoch,
        "gatewayEndpoint":binding.endpoint,"apiKey":key,
        "projectPath":binding.project,"localAccess":binding.local_access(),
        "gatewayConfig":null,"model":null}});
    if !binding.mcp.is_empty() {
        value["params"]["mcpServers"] = super::mcp::bootstrap_entries(&binding.mcp, mcp);
    }
    let mut frame = Frame(Zeroizing::new(Vec::with_capacity(FRAME_LIMIT)), false);
    let written = serde_json::to_writer(&mut frame, &value);
    if let Some(Value::String(secret)) = value["params"].get_mut("apiKey") {
        secret.zeroize();
    }
    if let Some(Value::Array(servers)) = value["params"].get_mut("mcpServers") {
        for s in servers {
            if let Some(Value::String(token)) = s.get_mut("accessToken") {
                token.zeroize();
            }
        }
    }
    if frame.1 && !binding.mcp.is_empty() {
        return Err(HarnessFailure::McpTooLarge);
    }
    if frame.1 || written.is_err() {
        return Err(HarnessFailure::Protocol);
    }
    frame.0.push(b'\n');
    Ok(frame.0)
}

/// Bytes being encoded; refuses to reach the limit instead of reallocating.
struct Frame(Zeroizing<Vec<u8>>, bool);

impl std::io::Write for Frame {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len() + bytes.len() >= FRAME_LIMIT {
            self.1 = true;
            return Err(std::io::ErrorKind::OutOfMemory.into());
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The engine's answer to the bootstrap.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    pub v: u8,
    pub protocol: String,
    pub runtime_version: String,
    pub tenant_id: String,
    pub workspace_id: String,
    pub credential_id: String,
    pub credential_epoch: u64,
    #[serde(default)]
    pub mcp_servers: Vec<Value>,
}

impl Receipt {
    /// It names exactly this binding, the pinned protocol and runtime, and
    /// the scope's MCP servers in order.
    pub fn matches(&self, binding: &Binding, runtime_version: &str) -> bool {
        self.v == 1
            && self.protocol == PROTOCOL
            && self.runtime_version == runtime_version
            && self.tenant_id == binding.tenant
            && self.workspace_id == binding.workspace
            && self.credential_id == binding.credential
            && self.credential_epoch == binding.credential_epoch
            && self.mcp_servers.len() == binding.mcp.len()
            && self
                .mcp_servers
                .iter()
                .zip(&binding.mcp)
                .all(|(got, want)| {
                    got.as_object().is_some_and(|o| o.len() == 2)
                        && got["name"].as_str() == Some(want.name.as_str())
                        && got["serverId"].as_str() == Some(want.server_id.as_str())
                })
    }
}

/// The host's requests. There is deliberately no shell, file, login or
/// Apply method here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Initialize,
    ThreadStart,
    TurnStart,
    TurnInterrupt,
    /// The thread's replay after a turn, for protected history (C20).
    HistoryRead,
    /// A saved replay into a new engine under the same context (C20).
    HistoryRestore,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Initialize => "initialize",
            Self::ThreadStart => "thread/start",
            Self::TurnStart => "turn/start",
            Self::TurnInterrupt => "turn/interrupt",
            Self::HistoryRead => "desktop/history/read",
            Self::HistoryRestore => "desktop/history/restore",
        }
    }
}

/// One encoded host frame with its newline. No `Debug`: a prompt can be
/// sensitive.
pub struct Outbound(pub Zeroizing<Vec<u8>>);

fn encode(value: &Value) -> Result<Outbound, HarnessFailure> {
    encode_within(value, FRAME_LIMIT)
}

fn encode_within(value: &Value, limit: usize) -> Result<Outbound, HarnessFailure> {
    let mut bytes =
        Zeroizing::new(serde_json::to_vec(value).map_err(|_| HarnessFailure::Protocol)?);
    if bytes.len() >= limit {
        return Err(HarnessFailure::Bounds);
    }
    bytes.push(b'\n');
    Ok(Outbound(bytes))
}

pub fn request(id: u64, method: Method, params: Value) -> Result<Outbound, HarnessFailure> {
    if id == 0 || !params.is_object() {
        return Err(HarnessFailure::Protocol);
    }
    // The engine reads a restore up to its replay limit, anything else up
    // to its frame limit.
    let limit = if method == Method::HistoryRestore {
        EVENT_LIMIT
    } else {
        FRAME_LIMIT
    };
    encode_within(
        &json!({"id":id,"method":method.as_str(),"params":params}),
        limit,
    )
}

/// A saved replay with the context it was exported under, and the
/// desktop's current tool catalog (C21).
pub fn restore_params(context: &Value, replay: &Value) -> Value {
    json!({"context":context,"replay":replay,"dynamicTools":super::tools::catalog()})
}

pub fn initialized() -> Outbound {
    Outbound(Zeroizing::new(b"{\"method\":\"initialized\"}\n".to_vec()))
}

/// A reply to one of the engine's requests.
pub fn reply(id: &RequestId, result: Value) -> Result<Outbound, HarnessFailure> {
    encode(&json!({"id":id.to_value(),"result":result}))
}

/// A JSON-RPC refusal of one of the engine's requests.
pub fn refuse(id: &RequestId, code: i32) -> Result<Outbound, HarnessFailure> {
    encode(
        &json!({"id":id.to_value(),"error":{"code":code,"message":"Not available in Prisma AIRS Desktop"}}),
    )
}

pub fn initialize_params(version: &str) -> Value {
    json!({"clientInfo":{"name":"prisma_airs_desktop","title":"Prisma AIRS Desktop","version":version},
        "capabilities":{"experimentalApi":true}})
}

/// The thread: the desktop's typed tools, and the selected project (if
/// any) as its working directory, read-only unless the operator allowed
/// changes (C21). Without a project there is no local environment. The
/// engine forces its own sandbox profile (one process, no network) and asks
/// for approval before anything it does not consider safe.
pub fn thread_params(binding: &Binding) -> Value {
    let sandbox = if binding.write {
        "workspace-write"
    } else {
        "read-only"
    };
    let mut params = json!({"ephemeral":true,"approvalPolicy":"on-request","approvalsReviewer":"user",
        "sandbox":sandbox,"dynamicTools":super::tools::catalog(),"cwd":binding.project});
    if binding.project.is_none() {
        params["environments"] = json!([]);
    }
    params
}

/// The prompt as at most eight 8 KiB text items, split on UTF-8 boundaries.
pub fn turn_params(thread: &str, prompt: &str) -> Result<Value, HarnessFailure> {
    if prompt.trim().is_empty() || prompt.len() > MAX_PROMPT_BYTES {
        return Err(HarnessFailure::Bounds);
    }
    let mut input = Vec::new();
    let mut rest = prompt;
    while !rest.is_empty() {
        let mut end = rest.len().min(TEXT_ITEM_BYTES);
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        input.push(json!({"type":"text","text":&rest[..end]}));
        rest = &rest[end..];
    }
    Ok(json!({"threadId":thread,"input":input}))
}

/// A JSON-RPC id the engine used.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum RequestId {
    Number(u64),
    Text(String),
}

impl RequestId {
    fn parse(value: Value) -> Option<Self> {
        match value {
            Value::Number(n) => n.as_u64().map(Self::Number),
            Value::String(s)
                if !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control) =>
            {
                Some(Self::Text(s))
            }
            _ => None,
        }
    }
    fn to_value(&self) -> Value {
        match self {
            Self::Number(n) => json!(n),
            Self::Text(s) => json!(s),
        }
    }
}

/// One frame from the engine, shape-checked.
#[derive(Debug)]
pub enum Incoming {
    Response {
        id: RequestId,
        result: Value,
    },
    Failure {
        id: RequestId,
        code: Option<i64>,
    },
    Request {
        id: RequestId,
        method: String,
        params: Value,
    },
    Notification {
        method: String,
        params: Value,
    },
}

impl Incoming {
    pub fn decode(line: &[u8]) -> Result<Self, HarnessFailure> {
        if line.is_empty() || line.len() > EVENT_LIMIT {
            return Err(HarnessFailure::Bounds);
        }
        let value: Value = serde_json::from_slice(line).map_err(|_| HarnessFailure::Protocol)?;
        let Value::Object(mut map) = value else {
            return Err(HarnessFailure::Protocol);
        };
        if map.keys().any(|k| {
            !matches!(
                k.as_str(),
                "id" | "method" | "params" | "result" | "error" | "jsonrpc"
            )
        }) {
            return Err(HarnessFailure::Protocol);
        }
        if map.remove("jsonrpc").is_some_and(|v| v != "2.0") {
            return Err(HarnessFailure::Protocol);
        }
        let id = match map.remove("id") {
            Some(v) => Some(RequestId::parse(v).ok_or(HarnessFailure::Protocol)?),
            None => None,
        };
        let take = |map: &mut Map<String, Value>, k: &str| map.remove(k);
        let (method, params, result, error) = (
            take(&mut map, "method"),
            take(&mut map, "params"),
            take(&mut map, "result"),
            take(&mut map, "error"),
        );
        match (id, method, params, result, error) {
            (Some(id), None, None, Some(result), None) => Ok(Self::Response { id, result }),
            (Some(id), None, None, None, Some(Value::Object(e))) => Ok(Self::Failure {
                id,
                code: e.get("code").and_then(Value::as_i64),
            }),
            (id, Some(Value::String(method)), params, None, None)
                if !method.is_empty()
                    && method.len() <= 128
                    && !method.chars().any(char::is_control) =>
            {
                let params = params.unwrap_or_else(|| json!({}));
                if !params.is_object() {
                    return Err(HarnessFailure::Protocol);
                }
                Ok(match id {
                    Some(id) => Self::Request { id, method, params },
                    None => Self::Notification { method, params },
                })
            }
            _ => Err(HarnessFailure::Protocol),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> Binding {
        Binding {
            tenant: derived_uuid("tenant", "tn-0123456789abcdef"),
            workspace: "16f7e90d-382a-4e78-b577-1b01eb5f8297".into(),
            credential: derived_uuid("credential", "cr-0011223344556677"),
            credential_epoch: 1,
            endpoint: "https://gateway.example/rt/v1".into(),
            project: None,
            write: false,
            mcp: Vec::new(),
        }
    }

    #[test]
    fn mcp_servers_are_bootstrapped_in_scope_order_and_echoed_exactly() {
        let mut b = binding();
        let server = |name: &str, id: &str| super::super::mcp::ScopeServer {
            name: name.into(),
            server_id: id.into(),
            url: format!("https://gw.example/{name}/mcp"),
            revision: "r".into(),
            grant: Some("g".into()),
        };
        b.mcp = vec![server("tools", "s1"), server("docs", "s2")];
        let token = Zeroizing::new("t".repeat(40));
        let frame = bootstrap(&b, "gw-key", &[Some(token.clone()), None]).expect("frame");
        let v: Value = serde_json::from_slice(&frame).expect("json");
        let servers = v["params"]["mcpServers"].as_array().expect("servers");
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0]["name"], "tools");
        assert_eq!(servers[0]["accessToken"], token.as_str());
        assert_eq!(
            servers[1]["accessToken"],
            super::super::mcp::SIGNED_OUT_TOKEN
        );
        // Tokens and scope must agree in length.
        assert_eq!(
            bootstrap(&b, "gw-key", &[None]).err(),
            Some(HarnessFailure::Protocol)
        );
        // Too many large tokens for one frame: said, not truncated.
        let big = Zeroizing::new("t".repeat(16 * 1024));
        b.mcp = (0..8)
            .map(|n| server(&format!("s{n}"), &format!("id-{n}")))
            .collect();
        let tokens: Vec<_> = (0..8).map(|_| Some(big.clone())).collect();
        assert_eq!(
            bootstrap(&b, "gw-key", &tokens).err(),
            Some(HarnessFailure::McpTooLarge)
        );
        b.mcp.truncate(2);
        let receipt = |mcp: Value| -> Receipt {
            serde_json::from_value(
                json!({"v":1,"protocol":PROTOCOL,"runtimeVersion":"0.1.7-desktop.2",
                "tenantId":b.tenant,"workspaceId":b.workspace,"credentialId":b.credential,
                "credentialEpoch":1,"mcpServers":mcp}),
            )
            .expect("fixture")
        };
        let ok = json!([{"name":"s0","serverId":"id-0"},{"name":"s1","serverId":"id-1"}]);
        assert!(receipt(ok).matches(&b, "0.1.7-desktop.2"));
        for bad in [
            json!([]),
            json!([{"name":"s1","serverId":"id-1"},{"name":"s0","serverId":"id-0"}]),
            json!([{"name":"s0","serverId":"id-0"}]),
            json!([{"name":"s0","serverId":"id-0","url":"x"},{"name":"s1","serverId":"id-1"}]),
        ] {
            assert!(
                !receipt(bad.clone()).matches(&b, "0.1.7-desktop.2"),
                "{bad}"
            );
        }
    }

    #[test]
    fn bootstrap_and_receipt_name_the_same_binding() {
        let b = binding();
        assert!(is_uuid(&b.tenant) && is_uuid(&b.credential) && is_uuid(&b.workspace));
        assert_ne!(b.tenant, derived_uuid("credential", "tn-0123456789abcdef"));
        assert!(!is_uuid("00000000-0000-0000-0000-000000000000"));
        assert!(!is_uuid("16F7E90D-382A-4E78-B577-1B01EB5F8297"));
        let frame = bootstrap(&b, "gw-key", &[]).expect("fixture");
        assert_eq!(frame.last(), Some(&b'\n'));
        let v: Value = serde_json::from_slice(&frame).expect("fixture");
        assert_eq!(v["method"], "desktop/bootstrap");
        assert_eq!(v["params"]["apiKey"], "gw-key");
        assert_eq!(v["params"]["localAccess"], "readOnly");
        assert_eq!(v["params"]["projectPath"], Value::Null);
        assert!(v["params"].get("mcpServers").is_none());
        let receipt = |tenant: &str, mcp: Value| -> Receipt {
            serde_json::from_value(
                json!({"v":1,"protocol":PROTOCOL,"runtimeVersion":"0.1.7-desktop.2",
                "tenantId":tenant,"workspaceId":b.workspace,"credentialId":b.credential,
                "credentialEpoch":1,"mcpServers":mcp}),
            )
            .expect("fixture")
        };
        assert!(receipt(&b.tenant, json!([])).matches(&b, "0.1.7-desktop.2"));
        assert!(!receipt(&b.tenant, json!([])).matches(&b, "0.1.8"));
        assert!(!receipt(&b.credential, json!([])).matches(&b, "0.1.7-desktop.2"));
        assert!(
            !receipt(&b.tenant, json!([{"name":"x","serverId":"y"}]))
                .matches(&b, "0.1.7-desktop.2")
        );
        // An unknown receipt field is not a receipt.
        assert!(
            serde_json::from_value::<Receipt>(
                json!({"v":1,"protocol":PROTOCOL,"runtimeVersion":"r",
            "tenantId":"t","workspaceId":"w","credentialId":"c","credentialEpoch":1,"extra":1})
            )
            .is_err()
        );
    }

    #[test]
    fn credentials_and_frames_are_bounded() {
        let b = binding();
        assert_eq!(
            bootstrap(&b, "", &[]).err(),
            Some(HarnessFailure::CredentialMissing)
        );
        assert_eq!(
            bootstrap(&b, "a\nb", &[]).err(),
            Some(HarnessFailure::CredentialMissing)
        );
        assert_eq!(
            bootstrap(&b, &"k".repeat(MAX_KEY_BYTES + 1), &[]).err(),
            Some(HarnessFailure::CredentialMissing)
        );
        assert!(bootstrap(&b, &"k".repeat(MAX_KEY_BYTES), &[]).is_ok());
        let mut long = b.clone();
        long.endpoint = format!("https://{}.example/v1", "a".repeat(FRAME_LIMIT));
        assert_eq!(
            bootstrap(&long, "k", &[]).err(),
            Some(HarnessFailure::Protocol)
        );
        assert_eq!(turn_params("t", "  ").err(), Some(HarnessFailure::Bounds));
        assert_eq!(
            turn_params("t", &"x".repeat(MAX_PROMPT_BYTES + 1)).err(),
            Some(HarnessFailure::Bounds)
        );
        // Split on character boundaries into at most eight items.
        let prompt = "é".repeat(MAX_PROMPT_BYTES / 2);
        let items = turn_params("t", &prompt).expect("fixture")["input"]
            .as_array()
            .expect("fixture")
            .clone();
        assert!(items.len() <= 8);
        assert_eq!(
            items
                .iter()
                .map(|i| i["text"].as_str().expect("fixture"))
                .collect::<String>(),
            prompt
        );
        assert_eq!(
            request(0, Method::TurnStart, json!({})).err(),
            Some(HarnessFailure::Protocol)
        );
        assert_eq!(
            request(1, Method::TurnStart, json!([])).err(),
            Some(HarnessFailure::Protocol)
        );
    }

    #[test]
    fn incoming_frames_have_one_shape_each() {
        let ok = |s: &str| Incoming::decode(s.as_bytes());
        assert!(matches!(
            ok(r#"{"id":3,"result":{}}"#),
            Ok(Incoming::Response {
                id: RequestId::Number(3),
                ..
            })
        ));
        assert!(matches!(
            ok(r#"{"id":3,"error":{"code":-32600}}"#),
            Ok(Incoming::Failure {
                code: Some(-32600),
                ..
            })
        ));
        assert!(matches!(
            ok(r#"{"method":"turn/started","params":{}}"#),
            Ok(Incoming::Notification { .. })
        ));
        assert!(matches!(
            ok(r#"{"id":"a","method":"item/tool/call","params":{}}"#),
            Ok(Incoming::Request { .. })
        ));
        for bad in [
            "",
            "[]",
            "not json",
            r#"{"id":1}"#,
            r#"{"id":1,"result":{},"error":{}}"#,
            r#"{"method":""}"#,
            r#"{"method":"x","params":[]}"#,
            r#"{"id":-1,"result":{}}"#,
            r#"{"id":1.5,"result":{}}"#,
            r#"{"id":1,"result":{},"other":1}"#,
            r#"{"jsonrpc":"1.0","id":1,"result":{}}"#,
            r#"{"id":1,"error":"text"}"#,
            r#"{"id":1,"method":"x","result":{}}"#,
        ] {
            assert!(Incoming::decode(bad.as_bytes()).is_err(), "{bad}");
        }
        let big = format!(
            r#"{{"method":"x","params":{{"t":"{}"}}}}"#,
            "a".repeat(EVENT_LIMIT)
        );
        assert_eq!(
            Incoming::decode(big.as_bytes()).err(),
            Some(HarnessFailure::Bounds)
        );
    }
}
