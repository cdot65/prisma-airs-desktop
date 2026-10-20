//! Gateway workspace and configuration reads (C16, D042): the tenant's
//! workspaces, and within the selected workspace its providers, routes
//! (configs), guardrails and service/user keys; the tenant-wide
//! integrations with their models; and the selected workspace's inference
//! prerequisites. Every read goes through the SDK's typed Gateway
//! operations over the tenant's management OAuth client (data, admin and
//! IAM planes at the management origin). Items leave the core only as
//! allow-listed projections (v1 `projectGateway`): key values, provider
//! keys and model configuration, integration secrets and credential-bearing
//! routing or guardrail values never do. Nothing here writes, provisions or
//! sends inference.

use scm_sdk::RequestOptions;
use scm_sdk::airs::gateway::{
    AIGatewayClient, AIGatewayWorkspaceGetOptions, AIGatewayWorkspaceListOptions,
    AIGatewayWorkspaceScopedListOptions,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ts_rs::TS;

use crate::management::{DetailFound, ManagementFailure, PageError};

/// Largest inventory page v1 accepted (larger replies are refused).
pub const MAX_ROWS: usize = 200;
/// The scope a Gateway key needs to call inference (SDK `GATEWAY_SCOPES`).
pub const INFERENCE_SCOPE: &str = "completions.write";
/// Largest serialized projection handed to the presentation.
const MAX_BYTES: usize = 8 * 1024 * 1024;
/// Deepest routing document walked for protected fields (v1).
const MAX_DEPTH: usize = 32;

/// A Gateway inventory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum GatewayResource {
    Workspaces,
    Providers,
    /// Tenant-wide (admin plane): shared by the workspaces granted them.
    Integrations,
    /// Routing configurations (the SDK's configs).
    Routes,
    Guardrails,
    ServiceKeys,
    UserKeys,
}

impl GatewayResource {
    /// Read within the selected workspace (the others are tenant-wide).
    pub fn scoped(self) -> bool {
        !matches!(self, Self::Workspaces | Self::Integrations)
    }
}

/// One Gateway read the presentation asks for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum GatewayRead {
    /// One inventory (at most 200 rows, as the service returned them).
    Inventory { resource: GatewayResource },
    /// One item; a workspace item is shown only when it is proven to belong
    /// to the selected workspace.
    Detail {
        resource: GatewayResource,
        id: String,
    },
    /// An integration's models.
    IntegrationModels { id: String },
    /// What inference from the selected workspace would need: its routes,
    /// providers, integrations and inference-scoped service keys, the IAM
    /// scope's workspace binding and each plane's access.
    Prerequisites,
}

impl GatewayRead {
    pub fn is_valid(&self) -> bool {
        match self {
            Self::Inventory { .. } | Self::Prerequisites => true,
            Self::Detail { id, .. } | Self::IntegrationModels { id } => is_id(id),
        }
    }

    /// Bound to the selected workspace (admitted only with its reference).
    pub fn scoped(&self) -> bool {
        match self {
            Self::Inventory { resource } | Self::Detail { resource, .. } => resource.scoped(),
            Self::IntegrationModels { .. } => false,
            Self::Prerequisites => true,
        }
    }
}

/// A Gateway identifier: 1-128 ASCII letters, digits, `-` or `_`.
pub fn is_id(id: &str) -> bool {
    (1..=128).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The selected workspace a scoped read is bound to (resolved natively from
/// the listed workspaces; never supplied by the presentation).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, serde::Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListedWorkspace {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub scope_name: String,
}

/// One inventory as the service returned it (projected).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewayList {
    pub resource: GatewayResource,
    /// The workspace a scoped inventory belongs to.
    pub workspace: Option<String>,
    /// The projected rows as a JSON array.
    pub rows: String,
    pub count: u32,
    /// The service's total, when it is an integer.
    pub total: Option<u32>,
    /// True only when the service said nothing follows (`has_more` false),
    /// or said nothing and its total is no more than the rows received.
    pub complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewayDetail {
    pub resource: GatewayResource,
    pub id: String,
    pub found: DetailFound,
    /// The projected item as JSON when found.
    pub item: Option<String>,
}

/// An integration's models (projected; pricing is not shown).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewayModels {
    pub integration: String,
    pub models: String,
    pub count: u32,
    pub total: Option<u32>,
    /// The service's total, if any, is no more than the models received.
    pub complete: bool,
    pub allow_all: bool,
}

/// Whether the workspace's IAM scope binds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ScopeBinding {
    Bound,
    /// The scope exists and does not list the workspace.
    Unbound,
    /// The service has no such scope.
    Absent,
    /// The credential may not read the scope.
    Denied,
    /// The scope could not be read.
    Failed,
}

/// What one plane's reads established about access.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum PlaneAccess {
    /// A read succeeded.
    Granted,
    /// Every read was refused for permissions.
    Denied,
    /// Every read failed otherwise.
    Failed,
}

/// The selected workspace's inference prerequisites. Each source keeps its
/// data or its sanitized failure; nothing is inferred from a failed one.
/// The inference credential itself is not configured here (C18), and no
/// other credential stands in for it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewayPrerequisites {
    pub workspace: String,
    pub routes: Option<GatewayList>,
    pub routes_failure: Option<ManagementFailure>,
    pub providers: Option<GatewayList>,
    pub providers_failure: Option<ManagementFailure>,
    pub integrations: Option<GatewayList>,
    pub integrations_failure: Option<ManagementFailure>,
    pub service_keys: Option<GatewayList>,
    pub service_keys_failure: Option<ManagementFailure>,
    /// Service keys read whose scopes include `completions.write`.
    pub inference_keys: u32,
    pub scope_name: String,
    pub scope: ScopeBinding,
    pub data_plane: PlaneAccess,
    pub admin_plane: PlaneAccess,
    pub iam_plane: PlaneAccess,
}

/// What a Gateway read produced.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum GatewayResult {
    List {
        list: GatewayList,
    },
    Detail {
        detail: GatewayDetail,
    },
    Models {
        models: GatewayModels,
    },
    Prerequisites {
        prerequisites: Box<GatewayPrerequisites>,
    },
}

/// The selected workspace's listed identity and the scope it reads under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceScope {
    pub id: String,
    pub slug: String,
    pub scope_name: String,
}

impl From<&ListedWorkspace> for WorkspaceScope {
    fn from(w: &ListedWorkspace) -> Self {
        Self {
            id: w.id.clone(),
            slug: w.slug.clone(),
            scope_name: w.scope_name.clone(),
        }
    }
}

// ---- Projection (v1 bridge/src/gateway.ts and gateway-routing.ts) ----

/// Nonsecret scalar fields any Gateway item may show (v1 `baseFields`).
const BASE_FIELDS: [&str; 28] = [
    "id",
    "name",
    "slug",
    "workspace_id",
    "verified_workspace_id",
    "organisation_id",
    "integration_id",
    "ai_provider_id",
    "scope_name",
    "object",
    "status",
    "description",
    "note",
    "type",
    "target",
    "version_id",
    "version",
    "created_at",
    "updated_at",
    "expires_at",
    "next_usage_reset_at",
    "is_default",
    "is_enabled",
    "allow_all_models",
    "user_id",
    "removed",
    "readback_pending",
    "key_transition_expires_at",
];

const LIMIT_FIELDS: [&str; 9] = [
    "type",
    "unit",
    "value",
    "credit_limit",
    "current_usage",
    "alert_threshold",
    "periodic_reset",
    "periodic_reset_days",
    "next_usage_reset_at",
];

const MODEL_FIELDS: [&str; 6] = [
    "slug",
    "name",
    "enabled",
    "is_custom",
    "is_finetune",
    "base_model_slug",
];

fn scalar(value: &Value) -> bool {
    matches!(
        value,
        Value::Null | Value::String(_) | Value::Number(_) | Value::Bool(_)
    )
}

fn pick(value: &Map<String, Value>, fields: &[&str]) -> Map<String, Value> {
    fields
        .iter()
        .filter_map(|k| {
            value
                .get(*k)
                .filter(|v| scalar(v))
                .map(|v| ((*k).to_owned(), v.clone()))
        })
        .collect()
}

/// A field name whose value is, or carries, a credential (v1 `secretField`,
/// case-insensitive; `-` and `_` are ignored, which only widens it).
pub fn secret_field(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    let plain: String = lower.chars().filter(|c| *c != '-' && *c != '_').collect();
    lower == "key"
        || lower == "auth"
        || lower.ends_with("token")
        || [
            "cookie",
            "apikey",
            "accesstoken",
            "refreshtoken",
            "clientsecret",
            "password",
            "credential",
            "authorization",
            "authtoken",
            "secret",
            "masked",
            "headers",
            "privatekey",
            "accesskey",
            "serviceaccountjson",
        ]
        .iter()
        .any(|word| plain.contains(word))
}

/// A `forward_headers` list of plain header names (not credentials).
fn forwarding_names(key: &str, value: &Value) -> bool {
    let name: String = key
        .chars()
        .filter(|c| *c != '-' && *c != '_')
        .collect::<String>()
        .to_ascii_lowercase();
    name == "forwardheaders"
        && match value {
            Value::Bool(_) => true,
            Value::Array(names) => names.iter().all(|n| {
                n.as_str().is_some_and(|n| {
                    (1..=128).contains(&n.len())
                        && n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                        && !secret_field(n)
                        && ![
                            "cookie",
                            "set-cookie",
                            "proxy-authorization",
                            "x-portkey-config",
                        ]
                        .contains(&n.to_ascii_lowercase().as_str())
                })
            }),
            _ => false,
        }
}

fn guarded(key: &str, value: &Value) -> bool {
    secret_field(key) && !forwarding_names(key, value)
}

/// A protected value that is already masked by the service.
fn masked(value: &Value, protected: bool) -> bool {
    match value {
        Value::String(text) => {
            protected
                && (text.contains("[REDACTED]")
                    || text
                        .chars()
                        .collect::<Vec<_>>()
                        .windows(3)
                        .any(|w| w.iter().all(|c| *c == '*' || *c == '•')))
        }
        Value::Array(items) => items.iter().any(|v| masked(v, protected)),
        Value::Object(map) => map
            .iter()
            .any(|(k, v)| masked(v, protected || secret_field(k))),
        _ => false,
    }
}

/// Whether any field at any depth carries a credential.
pub fn has_credentials(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.iter().any(has_credentials),
        Value::Object(map) => map.iter().any(|(k, v)| guarded(k, v) || has_credentials(v)),
        _ => false,
    }
}

/// The document without its credential-bearing fields.
pub fn public_document(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(public_document).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(k, v)| !guarded(k, v))
                .map(|(k, v)| (k.clone(), public_document(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Where the credential-bearing fields are (paths and shapes, never values).
fn protected_fields(value: &Value, path: &mut Vec<Value>, depth: usize) -> Option<Vec<Value>> {
    if depth > MAX_DEPTH {
        return None;
    }
    let mut found = Vec::new();
    match value {
        Value::Array(items) => {
            for (i, child) in items.iter().enumerate() {
                path.push(Value::from(i));
                found.extend(protected_fields(child, path, depth + 1)?);
                path.pop();
            }
        }
        Value::Object(map) => {
            for (key, child) in map {
                path.push(Value::from(key.as_str()));
                if guarded(key, child) {
                    let shape = match child {
                        Value::Array(_) => "array",
                        Value::Object(_) => "object",
                        Value::String(_) => "string",
                        _ => return None,
                    };
                    found.push(serde_json::json!({
                        "path": path.clone(),
                        "type": shape,
                        "masked": masked(child, true),
                    }));
                } else {
                    found.extend(protected_fields(child, path, depth + 1)?);
                }
                path.pop();
            }
        }
        _ => {}
    }
    Some(found)
}

fn limits(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().take(64).map(limits).collect()),
        Value::Object(map) => Value::Object(pick(map, &LIMIT_FIELDS)),
        _ => Value::Null,
    }
}

/// A JSON document given as an object or as a JSON string (`null`: empty).
fn document(value: Option<&Value>) -> Option<Map<String, Value>> {
    match value {
        None | Some(Value::Null) => Some(Map::new()),
        Some(Value::Object(map)) => Some(map.clone()),
        Some(Value::String(text)) => match serde_json::from_str(text) {
            Ok(Value::Object(map)) => Some(map),
            _ => None,
        },
        Some(_) => None,
    }
}

/// The allow-listed projection of one item; `None` when it has no ID or a
/// detail document is malformed.
pub fn project(resource: GatewayResource, value: &Value, detail: bool) -> Option<Value> {
    let item = value.as_object()?;
    item.get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())?;
    let mut safe = pick(item, &BASE_FIELDS);
    for key in ["usage_limits", "rate_limits"] {
        if let Some(v) = item.get(key) {
            safe.insert(key.to_owned(), limits(v));
        }
    }
    if let Some(Value::Object(defaults)) = item.get("defaults") {
        let kept: Map<String, Value> = ["config_id", "allow_config_override"]
            .iter()
            .filter_map(|k| {
                defaults
                    .get(*k)
                    .filter(|v| v.is_string() || v.is_boolean())
                    .map(|v| ((*k).to_owned(), v.clone()))
            })
            .collect();
        safe.insert("defaults".to_owned(), Value::Object(kept));
    }
    if let Some(Value::Array(scopes)) = item.get("scopes") {
        let scopes: Vec<Value> = scopes
            .iter()
            .filter(|s| s.is_string())
            .take(64)
            .cloned()
            .collect();
        safe.insert("scopes".to_owned(), Value::Array(scopes));
    }
    if !detail {
        return Some(Value::Object(safe));
    }
    match resource {
        GatewayResource::Integrations if item.contains_key("configurations") => {
            let configuration = Value::Object(document(item.get("configurations"))?);
            let names: Vec<Value> = configuration
                .as_object()
                .into_iter()
                .flat_map(|m| m.keys())
                .filter(|k| secret_field(k))
                .map(|k| Value::from(k.as_str()))
                .collect();
            safe.insert("configurations".to_owned(), public_document(&configuration));
            safe.insert("protected_fields".to_owned(), Value::Array(names));
            safe.insert(
                "masked_configuration".to_owned(),
                Value::Bool(masked(&configuration, false)),
            );
        }
        GatewayResource::Routes => {
            let config = Value::Object(document(item.get("config")).filter(|m| !m.is_empty())?);
            let fields = protected_fields(&config, &mut Vec::new(), 0)?;
            safe.insert(
                "credential_bearing".to_owned(),
                Value::Bool(has_credentials(&config)),
            );
            safe.insert("config".to_owned(), public_document(&config));
            safe.insert("protected_routing_fields".to_owned(), Value::Array(fields));
        }
        GatewayResource::Guardrails => {
            let checks = item.get("checks").cloned().unwrap_or(Value::Null);
            let actions = item.get("actions").cloned().unwrap_or(Value::Null);
            if has_credentials(&checks) || has_credentials(&actions) {
                safe.insert("credential_bearing".to_owned(), Value::Bool(true));
            } else {
                if checks.is_array() {
                    safe.insert("checks".to_owned(), checks);
                }
                if actions.is_object() {
                    safe.insert("actions".to_owned(), actions);
                }
            }
        }
        _ => {}
    }
    Some(Value::Object(safe))
}

fn integer(value: Option<&Value>) -> Option<u32> {
    value
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
}

fn bounded(value: &Value) -> Option<String> {
    let text = value.to_string();
    (text.len() <= MAX_BYTES).then_some(text)
}

/// One inventory reply, projected, with its completeness (v1 `page`). A
/// reply over 200 rows, with duplicate IDs, a total below its rows, or a
/// scoped row from another workspace is refused as invalid.
pub fn list(
    resource: GatewayResource,
    reply: &Value,
    workspace: Option<&str>,
) -> Option<GatewayList> {
    let reply = reply.as_object()?;
    let data = reply.get("data")?.as_array()?;
    if data.len() > MAX_ROWS {
        return None;
    }
    let mut ids = std::collections::BTreeSet::new();
    let mut rows = Vec::with_capacity(data.len());
    for row in data {
        if resource.scoped()
            && let Some(owner) = row.get("workspace_id")
            && owner.as_str() != workspace
        {
            return None;
        }
        let projected = project(resource, row, false)?;
        if !ids.insert(projected["id"].as_str()?.to_owned()) {
            return None;
        }
        rows.push(projected);
    }
    let total = integer(reply.get("total"));
    let count = u32::try_from(rows.len()).ok()?;
    if total.is_some_and(|t| t < count) {
        return None;
    }
    let complete = match reply.get("has_more") {
        Some(Value::Bool(more)) => !more,
        _ => total.is_some_and(|t| t <= count),
    };
    Some(GatewayList {
        resource,
        workspace: resource
            .scoped()
            .then(|| workspace.map(str::to_owned))
            .flatten(),
        rows: bounded(&Value::Array(rows))?,
        count,
        total,
        complete,
    })
}

/// The workspaces a Workspaces inventory listed (active ones; the service
/// returns only those by default).
pub fn listed_workspaces(list: &GatewayList) -> Vec<ListedWorkspace> {
    let rows: Vec<Value> = serde_json::from_str(&list.rows).unwrap_or_default();
    rows.iter()
        .filter_map(|r| {
            let text = |k: &str| r.get(k).and_then(Value::as_str).map(str::to_owned);
            let id = text("id").filter(|id| is_id(id))?;
            Some(ListedWorkspace {
                name: text("name").unwrap_or_else(|| id.clone()),
                slug: text("slug").unwrap_or_default(),
                scope_name: text("scope_name").unwrap_or_default(),
                id,
            })
        })
        .collect()
}

/// An integration's models (v1 shows names and enablement only).
pub fn models(integration: &str, reply: &Value) -> Option<GatewayModels> {
    let reply = reply.as_object()?;
    let items = reply.get("models")?.as_array()?;
    if items.len() > 1000 {
        return None;
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut models = Vec::with_capacity(items.len());
    for item in items {
        let item = item.as_object()?;
        let slug = item.get("slug")?.as_str()?;
        if !seen.insert(slug.to_owned()) {
            return None;
        }
        models.push(Value::Object(pick(item, &MODEL_FIELDS)));
    }
    let count = u32::try_from(models.len()).ok()?;
    let total = integer(reply.get("total"));
    if total.is_some_and(|t| t < count) {
        return None;
    }
    Some(GatewayModels {
        integration: integration.to_owned(),
        models: bounded(&Value::Array(models))?,
        count,
        total,
        complete: total.is_none_or(|t| t <= count),
        allow_all: reply.get("allow_all_models")?.as_bool()?,
    })
}

/// Whether `scope` binds the workspace `slug` (the SDK's binding shape).
pub fn binds(scope: &Value, slug: &str) -> Option<bool> {
    let resources = scope.get("resources")?.as_array()?;
    Some(
        !slug.is_empty()
            && resources.iter().any(|r| {
                r.get("resource_type").and_then(Value::as_str) == Some("workspace")
                    && r.get("resource_id").and_then(Value::as_str) == Some(slug)
            }),
    )
}

/// Service keys among `list` whose scopes allow inference.
pub fn inference_keys(list: &GatewayList) -> u32 {
    let rows: Vec<Value> = serde_json::from_str(&list.rows).unwrap_or_default();
    let n = rows
        .iter()
        .filter(|r| {
            r.get("scopes")
                .and_then(Value::as_array)
                .is_some_and(|s| s.iter().any(|s| s.as_str() == Some(INFERENCE_SCOPE)))
        })
        .count();
    u32::try_from(n).unwrap_or(u32::MAX)
}

// ---- Reads ----

fn json<T: Serialize>(reply: &T) -> Result<Value, PageError> {
    serde_json::to_value(reply).map_err(|_| PageError::Invalid)
}

fn scoped_query(
    scope: Option<&WorkspaceScope>,
) -> Result<AIGatewayWorkspaceScopedListOptions, PageError> {
    scope
        .map(|s| AIGatewayWorkspaceScopedListOptions::new(s.id.clone()))
        .ok_or(PageError::Invalid)
}

/// One inventory reply as JSON.
async fn inventory(
    gw: &AIGatewayClient,
    resource: GatewayResource,
    scope: Option<&WorkspaceScope>,
    options: Option<&RequestOptions>,
) -> Result<Value, PageError> {
    match resource {
        GatewayResource::Workspaces => json(
            &gw.workspaces()
                .list(&AIGatewayWorkspaceListOptions::default(), options)
                .await?,
        ),
        GatewayResource::Integrations => json(&gw.integrations().list(options).await?),
        GatewayResource::Providers => {
            json(&gw.providers().list(&scoped_query(scope)?, options).await?)
        }
        GatewayResource::Routes => json(&gw.configs().list(&scoped_query(scope)?, options).await?),
        GatewayResource::Guardrails => {
            json(&gw.guardrails().list(&scoped_query(scope)?, options).await?)
        }
        GatewayResource::ServiceKeys => json(
            &gw.api_keys()
                .list_service(&scoped_query(scope)?, options)
                .await?,
        ),
        GatewayResource::UserKeys => json(
            &gw.api_keys()
                .list_user(&scoped_query(scope)?, options)
                .await?,
        ),
    }
}

async fn item(
    gw: &AIGatewayClient,
    resource: GatewayResource,
    id: &str,
    options: Option<&RequestOptions>,
) -> Result<Value, PageError> {
    match resource {
        GatewayResource::Workspaces => json(
            &gw.workspaces()
                .get(id, &AIGatewayWorkspaceGetOptions::default(), options)
                .await?,
        ),
        GatewayResource::Integrations => json(&gw.integrations().get(id, options).await?),
        GatewayResource::Providers => json(&gw.providers().get(id, options).await?),
        GatewayResource::Routes => json(&gw.configs().get(id, options).await?),
        GatewayResource::Guardrails => json(&gw.guardrails().get(id, options).await?),
        GatewayResource::ServiceKeys => json(&gw.api_keys().get_service(id, options).await?),
        GatewayResource::UserKeys => json(&gw.api_keys().get_user(id, options).await?),
    }
}

async fn read_list(
    gw: &AIGatewayClient,
    resource: GatewayResource,
    scope: Option<&WorkspaceScope>,
    options: Option<&RequestOptions>,
) -> Result<GatewayList, PageError> {
    let reply = inventory(gw, resource, scope, options).await?;
    list(resource, &reply, scope.map(|s| s.id.as_str())).ok_or(PageError::Invalid)
}

/// One item: its reply must carry the requested ID; a workspace item must
/// name the selected workspace, or (when it names none) appear in that
/// workspace's inventory. Absence is claimed only from a complete one.
async fn read_detail(
    gw: &AIGatewayClient,
    resource: GatewayResource,
    id: &str,
    scope: Option<&WorkspaceScope>,
    options: Option<&RequestOptions>,
) -> Result<GatewayDetail, PageError> {
    let reply = item(gw, resource, id, options).await?;
    if reply.get("id").and_then(Value::as_str) != Some(id) {
        return Err(PageError::Invalid);
    }
    let mut found = DetailFound::Found;
    let mut reply = reply;
    if resource.scoped() {
        let scope = scope.ok_or(PageError::Invalid)?;
        match reply.get("workspace_id") {
            Some(owner) if owner.as_str() != Some(scope.id.as_str()) => {
                return Err(PageError::Invalid);
            }
            Some(_) => {}
            None => {
                let inventory = read_list(gw, resource, Some(scope), options).await?;
                let rows: Vec<Value> = serde_json::from_str(&inventory.rows).unwrap_or_default();
                if !rows.iter().any(|r| r["id"].as_str() == Some(id)) {
                    found = if inventory.complete {
                        DetailFound::Absent
                    } else {
                        DetailFound::Incomplete
                    };
                }
            }
        }
        if let Some(map) = reply.as_object_mut() {
            map.insert(
                "verified_workspace_id".to_owned(),
                Value::from(scope.id.as_str()),
            );
        }
    }
    let item = match found {
        DetailFound::Found => Some(
            bounded(&project(resource, &reply, true).ok_or(PageError::Invalid)?)
                .ok_or(PageError::Invalid)?,
        ),
        _ => None,
    };
    Ok(GatewayDetail {
        resource,
        id: id.to_owned(),
        found,
        item,
    })
}

fn failure_of(error: &PageError) -> ManagementFailure {
    match error {
        PageError::Invalid => ManagementFailure::InvalidResponse,
        PageError::Sdk(e) => ManagementFailure::from_sdk(e),
    }
}

fn sourced<T>(read: &Result<T, PageError>) -> Option<ManagementFailure> {
    read.as_ref().err().map(failure_of)
}

fn plane<T>(reads: &[&Result<T, PageError>]) -> PlaneAccess {
    if reads.iter().any(|r| r.is_ok()) {
        PlaneAccess::Granted
    } else if reads
        .iter()
        .all(|r| r.as_ref().err().map(failure_of) == Some(ManagementFailure::Denied))
    {
        PlaneAccess::Denied
    } else {
        PlaneAccess::Failed
    }
}

/// The selected workspace's prerequisites. A read in which no source
/// answered fails.
async fn prerequisites(
    gw: &AIGatewayClient,
    scope: &WorkspaceScope,
    options: Option<&RequestOptions>,
) -> Result<GatewayPrerequisites, PageError> {
    let routes = read_list(gw, GatewayResource::Routes, Some(scope), options).await;
    let providers = read_list(gw, GatewayResource::Providers, Some(scope), options).await;
    let keys = read_list(gw, GatewayResource::ServiceKeys, Some(scope), options).await;
    let integrations = read_list(gw, GatewayResource::Integrations, None, options).await;
    let scope_read = if scope.scope_name.is_empty() {
        Err(PageError::Invalid)
    } else {
        gw.iam_scopes()
            .get(&scope.scope_name, options)
            .await
            .map_err(PageError::from)
            .and_then(|s| json(&s))
    };
    let binding = match &scope_read {
        Ok(value) => match binds(value, &scope.slug) {
            Some(true) => ScopeBinding::Bound,
            Some(false) => ScopeBinding::Unbound,
            None => ScopeBinding::Failed,
        },
        Err(error) => match failure_of(error) {
            ManagementFailure::NotFound => ScopeBinding::Absent,
            ManagementFailure::Denied => ScopeBinding::Denied,
            _ => ScopeBinding::Failed,
        },
    };
    if routes.is_err()
        && providers.is_err()
        && keys.is_err()
        && integrations.is_err()
        && scope_read.is_err()
    {
        return Err(routes.err().unwrap_or(PageError::Invalid));
    }
    let data_plane = plane(&[&routes, &providers, &keys]);
    let admin_plane = plane(&[&integrations]);
    let iam_plane = plane(&[&scope_read]);
    Ok(GatewayPrerequisites {
        workspace: scope.id.clone(),
        routes_failure: sourced(&routes),
        providers_failure: sourced(&providers),
        integrations_failure: sourced(&integrations),
        service_keys_failure: sourced(&keys),
        inference_keys: keys.as_ref().map_or(0, inference_keys),
        routes: routes.ok(),
        providers: providers.ok(),
        integrations: integrations.ok(),
        service_keys: keys.ok(),
        scope_name: scope.scope_name.clone(),
        scope: binding,
        data_plane,
        admin_plane,
        iam_plane,
    })
}

/// Runs one admitted Gateway read (`scope`: the selected workspace, for a
/// scoped read).
pub(crate) async fn read(
    gw: &AIGatewayClient,
    read: &GatewayRead,
    scope: Option<&WorkspaceScope>,
    options: Option<&RequestOptions>,
) -> Result<GatewayResult, PageError> {
    if read.scoped() && scope.is_none() {
        return Err(PageError::Invalid);
    }
    match read {
        GatewayRead::Inventory { resource } => read_list(gw, *resource, scope, options)
            .await
            .map(|list| GatewayResult::List { list }),
        GatewayRead::Detail { resource, id } => read_detail(gw, *resource, id, scope, options)
            .await
            .map(|detail| GatewayResult::Detail { detail }),
        GatewayRead::IntegrationModels { id } => {
            let reply = json(&gw.integrations().get_models(id, options).await?)?;
            models(id, &reply)
                .ok_or(PageError::Invalid)
                .map(|models| GatewayResult::Models { models })
        }
        GatewayRead::Prerequisites => {
            let scope = scope.ok_or(PageError::Invalid)?;
            prerequisites(gw, scope, options)
                .await
                .map(|prerequisites| GatewayResult::Prerequisites {
                    prerequisites: Box::new(prerequisites),
                })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const WS: &str = "16f7e90d-382a-4e78-b577-1b01eb5f8297";

    fn rows(list: &GatewayList) -> Vec<Value> {
        serde_json::from_str(&list.rows).expect("rows")
    }

    #[test]
    fn inventories_claim_completeness_only_from_the_service() {
        let page = |data: Vec<Value>, extra: Value| {
            let mut reply = json!({"object": "list", "data": data});
            for (k, v) in extra.as_object().expect("object") {
                reply[k] = v.clone();
            }
            list(GatewayResource::Providers, &reply, Some(WS))
        };
        let row = |id: &str| json!({"id": id, "name": id, "workspace_id": WS});
        let p = page(vec![row("a"), row("b")], json!({"total": 2})).expect("page");
        assert!(p.complete && p.count == 2 && p.total == Some(2));
        let p = page(vec![row("a")], json!({"total": 5})).expect("page");
        assert!(!p.complete, "a total above the rows is partial");
        let p = page(vec![row("a")], json!({"total": 1, "has_more": true})).expect("page");
        assert!(!p.complete, "has_more decides");
        let p = page(vec![row("a")], json!({"total": 9, "has_more": false})).expect("page");
        assert!(p.complete);
        assert!(page(vec![row("a")], json!({})).is_some_and(|p| !p.complete));
        assert!(page(vec![row("a"), row("b")], json!({"total": 1})).is_none());
        assert!(page(vec![row("a"), row("a")], json!({"total": 2})).is_none());
        assert!(page(vec![json!({"name": "no id"})], json!({"total": 1})).is_none());
        let many: Vec<Value> = (0..201).map(|i| row(&format!("r{i}"))).collect();
        assert!(page(many, json!({"total": 201})).is_none());
        // A row naming another workspace is never shown as this one's.
        let foreign = json!({"id": "x", "workspace_id": "other"});
        assert!(page(vec![foreign], json!({"total": 1})).is_none());
    }

    #[test]
    fn projections_never_carry_key_material() {
        let provider = json!({
            "id": "p1", "name": "openai", "workspace_id": WS, "integration_id": "i1",
            "key": "sk-live-canary", "model_config": {"api_key": "canary"},
            "usage_limits": {"credit_limit": 10, "type": "cost", "secret": "canary"},
            "rate_limits": [{"type": "requests", "unit": "rpm", "value": 60}],
        });
        let out = project(GatewayResource::Providers, &provider, true).expect("project");
        let text = out.to_string();
        assert!(
            !text.contains("canary") && !text.contains("model_config"),
            "{text}"
        );
        assert_eq!(
            out["usage_limits"],
            json!({"credit_limit": 10, "type": "cost"})
        );
        assert_eq!(out["rate_limits"][0]["value"], 60);
        let key =
            json!({"id": "k1", "name": "ci", "key": "canary", "scopes": ["completions.write", 7]});
        let out = project(GatewayResource::ServiceKeys, &key, true).expect("project");
        assert_eq!(
            out,
            json!({"id": "k1", "name": "ci", "scopes": ["completions.write"]})
        );
        let integration = json!({
            "id": "i1", "name": "openai", "masked_key": "sk-****abcd", "slug": "openai",
            "configurations": "{\"api_key\":\"canary\",\"region\":\"us\",\"auth\":{\"token\":\"canary\"}}",
        });
        let out = project(GatewayResource::Integrations, &integration, true).expect("project");
        let text = out.to_string();
        assert!(!text.contains("canary") && !text.contains("sk-"), "{text}");
        assert_eq!(out["configurations"], json!({"region": "us"}));
        assert_eq!(out["protected_fields"], json!(["api_key", "auth"]));
    }

    #[test]
    fn routes_show_their_public_document_and_where_credentials_are() {
        let route = json!({
            "id": "r1", "name": "default", "workspace_id": WS,
            "config": "{\"strategy\":{\"mode\":\"fallback\"},\"targets\":[{\"provider\":\"@openai\",\"api_key\":\"canary\",\"forward_headers\":[\"x-trace\"]},{\"override_params\":{\"headers\":{\"x\":\"canary\"}}}]}",
        });
        let out = project(GatewayResource::Routes, &route, true).expect("project");
        let text = out.to_string();
        assert!(!text.contains("canary"), "{text}");
        assert_eq!(out["credential_bearing"], true);
        assert_eq!(
            out["config"]["targets"][0]["forward_headers"],
            json!(["x-trace"])
        );
        assert_eq!(
            out["protected_routing_fields"],
            json!([
                {"path": ["targets", 0, "api_key"], "type": "string", "masked": false},
                {"path": ["targets", 1, "override_params", "headers"], "type": "object", "masked": false},
            ])
        );
        let malformed = json!({"id": "r2", "config": "not json"});
        assert!(project(GatewayResource::Routes, &malformed, true).is_none());
        // The list view never carries the document.
        let row = project(GatewayResource::Routes, &route, false).expect("row");
        assert!(row.get("config").is_none());
    }

    #[test]
    fn guardrails_with_credentials_show_only_that_they_have_them() {
        let plain = json!({"id": "g1", "checks": [{"id": "pii"}], "actions": {"deny": true}});
        let out = project(GatewayResource::Guardrails, &plain, true).expect("project");
        assert_eq!(out["checks"], json!([{"id": "pii"}]));
        let bearing =
            json!({"id": "g2", "checks": [{"id": "x", "parameters": {"apiKey": "canary"}}]});
        let out = project(GatewayResource::Guardrails, &bearing, true).expect("project");
        assert!(!out.to_string().contains("canary"));
        assert_eq!(out["credential_bearing"], true);
        assert!(out.get("checks").is_none());
    }

    #[test]
    fn secret_field_names_follow_v1() {
        for key in [
            "key",
            "Auth",
            "api_key",
            "apiKey",
            "x-api-key",
            "access_token",
            "bearerToken",
            "client_secret",
            "Authorization",
            "headers",
            "private_key",
            "service_account_json",
            "set-cookie",
            "masked_key",
        ] {
            assert!(secret_field(key), "{key}");
        }
        for key in [
            "name",
            "region",
            "keyword_list",
            "authority",
            "weight",
            "provider",
        ] {
            assert!(!secret_field(key), "{key}");
        }
        assert!(!guarded("forward_headers", &json!(["x-trace"])));
        assert!(guarded("forward_headers", &json!(["authorization"])));
    }

    #[test]
    fn models_scope_bindings_and_inference_keys() {
        let reply = json!({"object": "list", "allow_all_models": false, "total": 2, "models": [
            {"slug": "gpt-4o", "enabled": true, "pricing_config": {"x": 1}},
            {"slug": "o3", "enabled": false, "name": "o3"},
        ]});
        let m = models("i1", &reply).expect("models");
        assert!(m.complete && m.count == 2 && !m.allow_all);
        assert!(!m.models.contains("pricing"));
        let short = json!({"allow_all_models": true, "total": 5, "models": [{"slug": "a", "enabled": true}]});
        assert!(!models("i1", &short).expect("models").complete);
        let scope = json!({"name": "s", "resources": [{"resource_type": "workspace", "resource_id": "team-a"}]});
        assert_eq!(binds(&scope, "team-a"), Some(true));
        assert_eq!(binds(&scope, "team-b"), Some(false));
        assert_eq!(binds(&json!({}), "team-a"), None);
        let keys = list(
            GatewayResource::ServiceKeys,
            &json!({"total": 2, "data": [
                {"id": "k1", "scopes": ["completions.write"]},
                {"id": "k2", "scopes": ["models.read"]},
            ]}),
            Some(WS),
        )
        .expect("keys");
        assert_eq!(inference_keys(&keys), 1);
    }

    #[test]
    fn listed_workspaces_and_reads_are_bounded() {
        let l = list(
            GatewayResource::Workspaces,
            &json!({"total": 2, "data": [
                {"id": WS, "name": "Team A", "slug": "team-a", "scope_name": "scope-a"},
                {"id": "bad id!", "name": "x"},
            ]}),
            None,
        )
        .expect("list");
        assert_eq!(l.workspace, None);
        assert_eq!(
            listed_workspaces(&l),
            vec![ListedWorkspace {
                id: WS.to_owned(),
                name: "Team A".to_owned(),
                slug: "team-a".to_owned(),
                scope_name: "scope-a".to_owned(),
            }]
        );
        assert!(
            !GatewayRead::Detail {
                resource: GatewayResource::Providers,
                id: "a/b".into()
            }
            .is_valid()
        );
        assert!(GatewayRead::IntegrationModels { id: "i-1".into() }.is_valid());
        assert!(GatewayRead::Prerequisites.scoped());
        assert!(
            !GatewayRead::Inventory {
                resource: GatewayResource::Integrations
            }
            .scoped()
        );
        assert_eq!(rows(&l).len(), 2);
    }
}
