//! Reviewed Gateway changes (C17, D043): v1's Gateway write subset, its
//! validation and normalization, the baselines and reference checks a
//! review is bound to, the one SDK call each change makes, the readback,
//! and workspace provisioning's per-step outcomes with their reconciliation
//! read.
//!
//! Working v1 (74754c02, read-only): `bridge/src/gateway.ts` (`mutation`,
//! `mutate`, `provisionWorkspace`, `deletionImpact`, `references`,
//! `stableConfiguration`, the review in `gatewayOperation`) and
//! `bridge/src/gateway-configuration.ts` (`validateConfiguration`,
//! `validateParameters`). The change flow (reviews, the native
//! confirmation, single-use apply, one-time secrets) is C09's; this module
//! supplies the Gateway rules. Drafts never carry credentials: integration
//! keys, secret mappings and credential-bearing routing, guardrail or
//! provider configuration are refused, and a change that would replace or
//! lose stored credentials is refused too (D043).

use std::future::Future;

use scm_sdk::RequestOptions;
use scm_sdk::airs::gateway::{
    AIGatewayClient, AIGatewayPlane, AIGatewayWorkspaceGetOptions, AIGatewayWorkspaceListOptions,
    AIGatewayWorkspaceProvisionOptions, AIGatewayWorkspaceScopedListOptions,
    GatewayWorkspaceProvisionRequest, ProvisionCleanup, ProvisionError, ProvisionScopeState,
    ProvisionStage, ProvisionWorkspaceState,
};
use scm_sdk::validation::{Mode, parse};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio::time::Instant;
use ts_rs::TS;
use zeroize::Zeroizing;

use crate::changes::{
    ChangeAction, ChangeDraft, ChangeKind, ChangeProblem, MAX_BODY_BYTES, Mutation, canonical,
};
use crate::gateway::{
    GatewayResource, MAX_ROWS, WorkspaceScope, binds, has_credentials, is_id, models,
    public_document,
};
use crate::management::{ManagementBinding, ManagementFailure, PageError};

/// Deepest catalog parameter schema walked (v1).
const MAX_SCHEMA_DEPTH: usize = 16;
/// Concurrent detail reads in a reference check (v1 `readBatch`).
const BATCH: usize = 4;
/// Fields the service assigns; dropped from a draft (an edit may start from
/// a detail).
const SERVER_FIELDS: [&str; 6] = [
    "id",
    "object",
    "created_at",
    "last_updated_at",
    "verified_workspace_id",
    "owner_id",
];
/// Traffic counters and server timestamps are not configuration changes
/// (v1 `stableConfiguration`): they never make a review conflict.
const VOLATILE_FIELDS: [&str; 6] = [
    "current_usage",
    "updated_at",
    "last_used_at",
    "next_usage_reset_at",
    "request_count",
    "token_count",
];
/// The draft flag that reuses an existing IAM scope when provisioning.
pub const EXISTING_SCOPE: &str = "existing_scope";

/// v1's Gateway action matrix.
pub fn allowed(kind: ChangeKind, action: ChangeAction) -> bool {
    use ChangeAction::*;
    use ChangeKind as K;
    matches!(
        (kind, action),
        (
            K::GatewayWorkspaces | K::GatewayProviders | K::GatewayRoutes | K::GatewayGuardrails,
            Create | Update | Delete
        ) | (
            K::GatewayServiceKeys | K::GatewayUserKeys,
            Create | Update | Rotate | Delete
        ) | (K::GatewayIntegrations, Create | Update | Delete | Models)
    )
}

/// The SDK request schema a body is parsed with.
fn schema(resource: GatewayResource, action: ChangeAction) -> Option<&'static str> {
    use GatewayResource as R;
    let create = action == ChangeAction::Create;
    Some(match (resource, action) {
        (_, ChangeAction::Delete) => return None,
        (_, ChangeAction::Rotate) => "GatewayApiKeyRotateRequestSchema",
        (_, ChangeAction::Models) => "GatewayIntegrationModelsBulkUpdateRequestSchema",
        (R::Workspaces, _) if create => "GatewayWorkspaceCreateRequestSchema",
        (R::Workspaces, _) => "GatewayWorkspaceUpdateRequestSchema",
        (R::Providers, _) if create => "GatewayProviderCreateRequestSchema",
        (R::Providers, _) => "GatewayProviderUpdateRequestSchema",
        (R::Routes, _) if create => "GatewayConfigCreateRequestSchema",
        (R::Routes, _) => "GatewayConfigUpdateRequestSchema",
        (R::Guardrails, _) if create => "GatewayGuardrailCreateRequestSchema",
        (R::Guardrails, _) => "GatewayGuardrailUpdateRequestSchema",
        (R::ServiceKeys, _) if create => "GatewayServiceApiKeyCreateRequestSchema",
        (R::UserKeys, _) if create => "GatewayUserApiKeyCreateRequestSchema",
        (R::ServiceKeys | R::UserKeys, _) => "GatewayApiKeyUpdateRequestSchema",
        (R::Integrations, _) if create => "GatewayIntegrationCreateRequestSchema",
        (R::Integrations, _) => "GatewayIntegrationUpdateRequestSchema",
    })
}

/// Validates and normalizes a captured Gateway draft. `tsg` is the
/// management credential's tenant (the organisation a create belongs to);
/// `workspace` the chosen workspace's ID for a change inside it (never the
/// draft's).
pub fn normalize(
    draft: &ChangeDraft,
    tsg: &str,
    workspace: Option<&str>,
) -> Result<Mutation, ChangeProblem> {
    let (kind, action) = (draft.kind, draft.action);
    let resource = kind.gateway().ok_or(ChangeProblem::Unsupported)?;
    if !allowed(kind, action) {
        return Err(ChangeProblem::Unsupported);
    }
    let creates = action == ChangeAction::Create;
    if (creates && !draft.target.is_empty()) || (!creates && !is_id(&draft.target)) {
        return Err(ChangeProblem::Target);
    }
    if draft.body.len() > MAX_BODY_BYTES {
        return Err(ChangeProblem::Body);
    }
    let Ok(Value::Object(mut body)) = serde_json::from_str::<Value>(&draft.body) else {
        return Err(ChangeProblem::Body);
    };
    let mutation = |body: Map<String, Value>| Mutation {
        kind,
        action,
        target: draft.target.clone(),
        body: Value::Object(body),
    };
    if action == ChangeAction::Delete {
        // v1: a delete carries nothing.
        return if body.is_empty() {
            Ok(mutation(body))
        } else {
            Err(ChangeProblem::Body)
        };
    }
    for field in SERVER_FIELDS {
        body.remove(field);
    }
    // Credentials are never a draft's: integration keys and secret
    // mappings, and any credential-bearing configuration.
    if body.contains_key("key")
        || body.contains_key("secret_mappings")
        || has_credentials(&Value::Object(body.clone()))
    {
        return Err(ChangeProblem::CredentialPolicy);
    }
    let mut existing_scope = None;
    if matches!(action, ChangeAction::Create | ChangeAction::Update) {
        if action == ChangeAction::Update && body.is_empty() {
            return Err(ChangeProblem::Body);
        }
        // The chosen workspace is the core's: a draft naming another is
        // refused, and a create inside it is placed there.
        if resource.scoped() {
            let workspace = workspace.ok_or(ChangeProblem::WorkspaceMismatch)?;
            match body.get("workspace_id") {
                Some(Value::String(named)) if named == workspace => {}
                None => {}
                Some(_) => return Err(ChangeProblem::WorkspaceMismatch),
            }
            let carries = creates || resource == GatewayResource::Routes;
            if carries {
                body.insert("workspace_id".into(), Value::from(workspace));
            } else {
                body.remove("workspace_id");
            }
        } else if resource == GatewayResource::Integrations
            && let Some(named) = body.get("workspace_id")
            && named.as_str() != workspace
        {
            return Err(ChangeProblem::WorkspaceMismatch);
        }
        // A create belongs to the credential's tenant.
        let organised = creates
            && matches!(
                resource,
                GatewayResource::ServiceKeys
                    | GatewayResource::UserKeys
                    | GatewayResource::Integrations
                    | GatewayResource::Guardrails
            );
        if organised {
            match body.get("organisation_id") {
                Some(Value::String(named)) if named == tsg => {}
                None => {
                    body.insert("organisation_id".into(), Value::from(tsg));
                }
                Some(_) => return Err(ChangeProblem::Contract),
            }
        }
        if resource == GatewayResource::Workspaces && creates {
            existing_scope = match body.remove(EXISTING_SCOPE) {
                None | Some(Value::Bool(false)) => Some(false),
                Some(Value::Bool(true)) => Some(true),
                Some(_) => return Err(ChangeProblem::Body),
            };
            // The operator names the exact IAM scope (a generated name would
            // differ between review and apply).
            let named = body.get("scope_name").and_then(Value::as_str);
            if !named.is_some_and(|n| scm_sdk::shared::iam::validate_scope_name(n).is_ok()) {
                return Err(ChangeProblem::Contract);
            }
        }
    }
    let schema = schema(resource, action).ok_or(ChangeProblem::Unsupported)?;
    let parsed =
        parse(schema, Value::Object(body), Mode::Request).map_err(|_| ChangeProblem::Contract)?;
    let Value::Object(mut body) = parsed else {
        return Err(ChangeProblem::Contract);
    };
    if let Some(reuse) = existing_scope {
        body.insert(EXISTING_SCOPE.into(), Value::Bool(reuse));
    }
    Ok(mutation(body))
}

/// The configuration a review's baseline digest covers: traffic counters
/// and server timestamps removed (v1 `stableConfiguration`).
pub fn stable(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(stable).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(k, _)| !VOLATILE_FIELDS.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), stable(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Whether `value` names one of `ids` as itself, `@id` or an `@id/...`
/// path (v1 `references`).
pub fn references(value: &Value, ids: &[&str]) -> bool {
    match value {
        Value::String(s) => ids.iter().any(|id| {
            !id.is_empty()
                && (s == id
                    || s.strip_prefix('@').is_some_and(|rest| {
                        rest == *id || rest.strip_prefix(id).is_some_and(|p| p.starts_with('/'))
                    }))
        }),
        Value::Array(items) => items.iter().any(|v| references(v, ids)),
        Value::Object(map) => map.values().any(|v| references(v, ids)),
        _ => false,
    }
}

/// A routing document as an object (the service may return it as text).
pub fn routing_document(value: Option<&Value>) -> Option<Value> {
    match value? {
        Value::String(text) => serde_json::from_str(text).ok(),
        other @ Value::Object(_) => Some(other.clone()),
        _ => None,
    }
}

/// Validates `value` against a guardrail catalog parameter schema (v1
/// `validateParameters`). Unsupported constraints (references,
/// combinators, conditionals and patterns) fail closed.
pub fn valid_parameters(schema: &Value, value: &Value, depth: usize) -> bool {
    let Some(schema) = schema.as_object() else {
        return false;
    };
    if depth > MAX_SCHEMA_DEPTH
        || [
            "$ref", "oneOf", "anyOf", "allOf", "not", "if", "then", "else", "pattern",
        ]
        .iter()
        .any(|k| schema.contains_key(*k))
    {
        return false;
    }
    if let Some(options) = schema.get("enum").and_then(Value::as_array)
        && !options.iter().any(|o| canonical(o) == canonical(value))
    {
        return false;
    }
    if let Some(constant) = schema.get("const")
        && canonical(constant) != canonical(value)
    {
        return false;
    }
    let types: Vec<&str> = match schema.get("type") {
        Some(Value::String(t)) => vec![t.as_str()],
        Some(Value::Array(ts)) => ts.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    let typed = |t: &str| match t {
        "null" => value.is_null(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "number" => value.is_number(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        _ => false,
    };
    if !types.is_empty() && !types.iter().any(|t| typed(t)) {
        return false;
    }
    let number = |key: &str| schema.get(key).and_then(Value::as_f64);
    if let Some(n) = value.as_f64()
        && (!n.is_finite()
            || number("minimum").is_some_and(|m| n < m)
            || number("maximum").is_some_and(|m| n > m)
            || number("exclusiveMinimum").is_some_and(|m| n <= m)
            || number("exclusiveMaximum").is_some_and(|m| n >= m))
    {
        return false;
    }
    if let Some(s) = value.as_str() {
        let length = s.chars().count() as f64;
        if number("minLength").is_some_and(|m| length < m)
            || number("maxLength").is_some_and(|m| length > m)
        {
            return false;
        }
    }
    if let Some(items) = value.as_array() {
        let n = items.len() as f64;
        if items.len() > 256
            || number("minItems").is_some_and(|m| n < m)
            || number("maxItems").is_some_and(|m| n > m)
        {
            return false;
        }
        if schema.get("uniqueItems") == Some(&Value::Bool(true)) {
            let mut seen: Vec<String> = items.iter().map(canonical).collect();
            seen.sort();
            seen.dedup();
            if seen.len() != items.len() {
                return false;
            }
        }
        if let Some(item) = schema.get("items").filter(|i| i.is_object())
            && !items.iter().all(|v| valid_parameters(item, v, depth + 1))
        {
            return false;
        }
    }
    if let Some(map) = value.as_object() {
        if let Some(required) = schema.get("required").and_then(Value::as_array)
            && !required
                .iter()
                .all(|k| k.as_str().is_some_and(|k| map.contains_key(k)))
        {
            return false;
        }
        let properties = schema.get("properties").and_then(Value::as_object);
        for (key, child) in map {
            match properties
                .and_then(|p| p.get(key))
                .filter(|p| p.is_object())
            {
                Some(property) => {
                    if !valid_parameters(property, child, depth + 1) {
                        return false;
                    }
                }
                None => match schema.get("additionalProperties") {
                    Some(Value::Bool(false)) => return false,
                    Some(extra @ Value::Object(_))
                        if !valid_parameters(extra, child, depth + 1) =>
                    {
                        return false;
                    }
                    _ => {}
                },
            }
        }
    }
    true
}

/// The concrete SDK schema of a provider family's configuration (v1
/// `gateway-configuration.ts`); unknown families stay server-validated.
pub fn family_schema(slug: &str) -> Option<&'static str> {
    Some(match slug {
        "open-ai" => "GatewayOpenAIConfigurationSchema",
        "azure-openai" => "GatewayAzureOpenAIConfigurationSchema",
        "bedrock" => "GatewayBedrockConfigurationSchema",
        "sagemaker" => "GatewaySageMakerConfigurationSchema",
        "vertex-ai" => "GatewayVertexAIConfigurationSchema",
        "azure-ai" => "GatewayAzureAIConfigurationSchema",
        "workers-ai" => "GatewayWorkersAIConfigurationSchema",
        "hugging-face" | "huggingface" => "GatewayHuggingFaceConfigurationSchema",
        "cortex" => "GatewayCortexConfigurationSchema",
        _ => return None,
    })
}

/// The confirmation the operator types: "CREATE" for a create, otherwise
/// the item's name (its identifier when it has none).
pub fn confirmation(mutation: &Mutation, before: Option<&Value>) -> Option<String> {
    if mutation.action == ChangeAction::Create {
        return Some("CREATE".into());
    }
    let name = before
        .and_then(|b| b.get("name"))
        .and_then(Value::as_str)
        .filter(|n| !n.trim().is_empty());
    Some(name.map_or_else(|| mutation.target.clone(), str::to_owned))
}

/// What a change does, for the review and the native confirmation.
pub fn impact(mutation: &Mutation) -> &'static str {
    use ChangeAction as A;
    use ChangeKind as K;
    match (mutation.kind, mutation.action) {
        (K::GatewayWorkspaces, A::Create) => {
            "Provisions a workspace in three separate requests: the IAM scope (unless an existing one is reused), the workspace, then the scope's binding to it. Nothing is retried. Only if the service definitely refuses the workspace is the IAM scope this change created deleted again; a reused scope, or one kept because the workspace exists or may exist, stays. The result reports every step, that cleanup and what reading the tenant back found."
        }
        (K::GatewayWorkspaces, A::Update) => {
            "Changes this workspace's settings, defaults and limits for everyone who uses it."
        }
        (K::GatewayWorkspaces, A::Delete) => {
            "Archives this workspace. It must hold no routes, providers, guardrails or keys. The desktop cannot unarchive it."
        }
        (K::GatewayServiceKeys | K::GatewayUserKeys, A::Create) => {
            "Creates a Gateway key in this workspace. Its value is returned once, held in memory only and shown only in the desktop's dialog."
        }
        (_, A::Rotate) => {
            "Issues a new value for this key; the old value stops working when the transition period ends. Update every client that uses it. The new value is shown once."
        }
        (K::GatewayIntegrations, A::Delete) => {
            "Deletes this integration for the whole tenant. Providers that use it, in any workspace, and workspaces it is granted to block the change."
        }
        (K::GatewayIntegrations, A::Models) => {
            "Replaces which of this integration's models are enabled, for every workspace that uses it."
        }
        (K::GatewayIntegrations, _) => {
            "Applies to the whole tenant: every workspace granted this integration sees the change. Credentials are not entered here."
        }
        (_, A::Delete) => {
            "Deletes this item from the workspace. Configuration that still references it is listed and blocks the change."
        }
        _ => "Applies to the chosen workspace only. Credentials are not entered here.",
    }
}

/// The expected result shown in review: the public draft over the item's
/// allow-listed projection (a delete has none).
pub fn preview_after(mutation: &Mutation, before: Option<&Value>) -> Option<Value> {
    let resource = mutation.kind.gateway()?;
    let body = public_document(&mutation.body);
    match mutation.action {
        ChangeAction::Delete => None,
        ChangeAction::Create => Some(body),
        ChangeAction::Models => Some(body),
        ChangeAction::Rotate => {
            let mut after = before
                .and_then(|b| crate::gateway::project(resource, b, true))
                .unwrap_or(Value::Object(Map::new()));
            if let Value::Object(map) = &mut after {
                map.insert("key".into(), Value::from("New value, shown once"));
                if let Some(period) = body.get("key_transition_period_ms") {
                    map.insert("key_transition_period_ms".into(), period.clone());
                }
            }
            Some(after)
        }
        ChangeAction::Update | ChangeAction::Version => {
            let mut after = before
                .and_then(|b| crate::gateway::project(resource, b, true))
                .unwrap_or(Value::Object(Map::new()));
            if let (Value::Object(map), Value::Object(edits)) = (&mut after, body) {
                for (key, value) in edits {
                    map.insert(key, value);
                }
            }
            Some(after)
        }
    }
}

/// The item as the presentation may see it (allow-listed).
pub fn shown(kind: ChangeKind, item: &Value) -> Option<String> {
    let resource = kind.gateway()?;
    crate::gateway::project(resource, item, true).map(|v| v.to_string())
}

// ---- IO ----

/// Why a Gateway baseline, write or readback did not complete.
#[derive(Debug)]
pub enum ChangeError {
    Problem(ChangeProblem),
    Page(PageError),
}

impl From<ChangeProblem> for ChangeError {
    fn from(problem: ChangeProblem) -> Self {
        Self::Problem(problem)
    }
}

impl From<PageError> for ChangeError {
    fn from(error: PageError) -> Self {
        Self::Page(error)
    }
}

impl From<scm_sdk::Error> for ChangeError {
    fn from(error: scm_sdk::Error) -> Self {
        Self::Page(PageError::Sdk(error))
    }
}

/// What a review is bound to: the item as read (kept in the core, never
/// shown unprojected) and the configuration that still references it.
#[derive(Clone, Debug)]
pub struct Baseline {
    pub before: Option<Value>,
    pub references: Vec<String>,
}

fn json<T: Serialize>(reply: &T) -> Result<Value, PageError> {
    serde_json::to_value(reply).map_err(|_| PageError::Invalid)
}

fn model<T: serde::de::DeserializeOwned>(body: &Value) -> Result<T, PageError> {
    let mut body = body.clone();
    if let Value::Object(map) = &mut body {
        map.remove(EXISTING_SCOPE);
    }
    serde_json::from_value(body).map_err(|_| PageError::Invalid)
}

fn not_found(error: &PageError) -> bool {
    matches!(error, PageError::Sdk(e) if ManagementFailure::from_sdk(e) == ManagementFailure::NotFound)
}

/// One complete inventory's raw rows (at most 200); an incomplete one is
/// [`ChangeProblem::InventoryLimit`].
async fn rows(
    gw: &AIGatewayClient,
    resource: GatewayResource,
    workspace: Option<&str>,
    options: Option<&RequestOptions>,
) -> Result<Vec<Value>, ChangeError> {
    let scoped = || {
        workspace
            .map(AIGatewayWorkspaceScopedListOptions::new)
            .ok_or(PageError::Invalid)
    };
    let reply = match resource {
        GatewayResource::Workspaces => json(
            &gw.workspaces()
                .list(&AIGatewayWorkspaceListOptions::default(), options)
                .await?,
        )?,
        GatewayResource::Integrations => json(&gw.integrations().list(options).await?)?,
        GatewayResource::Providers => json(&gw.providers().list(&scoped()?, options).await?)?,
        GatewayResource::Routes => json(&gw.configs().list(&scoped()?, options).await?)?,
        GatewayResource::Guardrails => json(&gw.guardrails().list(&scoped()?, options).await?)?,
        GatewayResource::ServiceKeys => {
            json(&gw.api_keys().list_service(&scoped()?, options).await?)?
        }
        GatewayResource::UserKeys => json(&gw.api_keys().list_user(&scoped()?, options).await?)?,
    };
    let list = crate::gateway::list(resource, &reply, workspace).ok_or(PageError::Invalid)?;
    if !list.complete {
        return Err(ChangeProblem::InventoryLimit.into());
    }
    let data = reply
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .ok_or(PageError::Invalid)?;
    Ok(data)
}

/// One item, unprojected (it stays in the core).
async fn item(
    gw: &AIGatewayClient,
    resource: GatewayResource,
    id: &str,
    options: Option<&RequestOptions>,
) -> Result<Value, PageError> {
    let reply = match resource {
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
    }?;
    if reply.get("id").and_then(Value::as_str) != Some(id) {
        return Err(PageError::Invalid);
    }
    Ok(reply)
}

/// The item to change, proven to be in the chosen workspace (by its
/// `workspace_id`, or by the workspace's complete inventory when it names
/// none). Absent is [`ChangeProblem::NotFound`].
async fn current(
    gw: &AIGatewayClient,
    resource: GatewayResource,
    id: &str,
    scope: Option<&WorkspaceScope>,
    options: Option<&RequestOptions>,
) -> Result<Value, ChangeError> {
    let reply = match item(gw, resource, id, options).await {
        Ok(reply) => reply,
        Err(error) if not_found(&error) => return Err(ChangeProblem::NotFound.into()),
        Err(error) => return Err(error.into()),
    };
    if resource.scoped() {
        let scope = scope.ok_or(ChangeProblem::WorkspaceMismatch)?;
        match reply.get("workspace_id") {
            Some(owner) if owner.as_str() != Some(scope.id.as_str()) => {
                return Err(ChangeProblem::WorkspaceMismatch.into());
            }
            Some(_) => {}
            None => {
                let listed = rows(gw, resource, Some(&scope.id), options).await?;
                if !listed.iter().any(|r| r["id"].as_str() == Some(id)) {
                    return Err(ChangeProblem::NotFound.into());
                }
            }
        }
    }
    Ok(reply)
}

/// Runs `read` over `rows` four at a time; the first failure fails all.
async fn batched<F, Fut>(rows: &[Value], read: F) -> Result<Vec<(Value, Value)>, ChangeError>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Result<Value, ChangeError>>,
{
    let mut out = Vec::with_capacity(rows.len());
    for chunk in rows.chunks(BATCH) {
        let reads = chunk
            .iter()
            .map(|row| read(row["id"].as_str().unwrap_or_default().to_owned()));
        let results = futures_util::future::join_all(reads).await;
        for (row, result) in chunk.iter().zip(results) {
            out.push((row.clone(), result?));
        }
    }
    Ok(out)
}

fn label(collection: &str, row: &Value) -> String {
    let name = row
        .get("name")
        .and_then(Value::as_str)
        .or_else(|| row.get("id").and_then(Value::as_str))
        .unwrap_or("unknown");
    format!("{collection}: {name}")
}

/// The configuration that still references `before` (v1 `deletionImpact`).
/// Any inventory that cannot be read completely refuses the review.
async fn referencing(
    gw: &AIGatewayClient,
    resource: GatewayResource,
    before: &Value,
    scope: Option<&WorkspaceScope>,
    options: Option<&RequestOptions>,
) -> Result<Vec<String>, ChangeError> {
    use GatewayResource as R;
    let id = before["id"].as_str().unwrap_or_default();
    let slug = before["slug"].as_str().unwrap_or_default();
    let ids = [id, slug];
    let mut found = Vec::new();
    match resource {
        R::ServiceKeys | R::UserKeys => {}
        R::Workspaces => {
            // Archiving needs an empty workspace.
            for (collection, name) in [
                (R::Routes, "Route"),
                (R::Providers, "Provider"),
                (R::Guardrails, "Guardrail"),
                (R::ServiceKeys, "Service key"),
                (R::UserKeys, "User key"),
            ] {
                for row in rows(gw, collection, Some(id), options).await? {
                    found.push(label(name, &row));
                }
            }
        }
        R::Integrations => {
            // Providers in every listed workspace, and the workspaces it
            // is granted to.
            for workspace in rows(gw, R::Workspaces, None, options).await? {
                let ws = workspace["id"].as_str().unwrap_or_default();
                let providers = rows(gw, R::Providers, Some(ws), options).await?;
                let details = batched(&providers, |p| async move {
                    Ok(item(gw, R::Providers, &p, options).await?)
                })
                .await?;
                for (row, detail) in details {
                    if detail.get("integration_id").and_then(Value::as_str) == Some(id) {
                        found.push(format!(
                            "{} (workspace {})",
                            label("Provider", &row),
                            workspace["name"].as_str().unwrap_or(ws)
                        ));
                    }
                }
            }
            let access = json(&gw.integrations().get_workspaces(id, options).await?)?;
            let bindings = access
                .get("workspaces")
                .and_then(Value::as_array)
                .ok_or(PageError::Invalid)?;
            if bindings.len() > MAX_ROWS
                || access
                    .get("total")
                    .and_then(Value::as_u64)
                    .is_some_and(|t| t as usize > bindings.len())
            {
                return Err(ChangeProblem::InventoryLimit.into());
            }
            if access["global_workspace_access"]["enabled"] == Value::Bool(true) {
                found.push("Global workspace access".into());
            }
            for binding in bindings {
                if binding.get("enabled") != Some(&Value::Bool(false)) {
                    found.push(format!(
                        "Workspace access: {}",
                        binding["id"].as_str().unwrap_or("unknown")
                    ));
                }
            }
        }
        R::Providers | R::Routes | R::Guardrails => {
            let ws = scope.map(|s| s.id.as_str());
            let mut scan = vec![(R::Routes, "Route")];
            if resource == R::Routes {
                scan.push((R::ServiceKeys, "Service key"));
                scan.push((R::UserKeys, "User key"));
            }
            for (collection, name) in scan {
                let listed: Vec<Value> = rows(gw, collection, ws, options)
                    .await?
                    .into_iter()
                    .filter(|r| r["id"].as_str() != Some(id))
                    .collect();
                let details = batched(&listed, |other| async move {
                    Ok(item(gw, collection, &other, options).await?)
                })
                .await?;
                for (row, mut detail) in details {
                    if let Some(document) = routing_document(detail.get("config")) {
                        detail["config"] = document;
                    }
                    if references(&detail, &ids) {
                        found.push(label(name, &row));
                    }
                }
            }
            if resource == R::Routes
                && let Some(scope) = scope
            {
                let workspace = item(gw, R::Workspaces, &scope.id, options).await?;
                if references(&workspace["defaults"], &ids) {
                    found.push("Workspace default configuration".into());
                }
            }
        }
    }
    Ok(found)
}

/// Checks a guardrail's checks against the catalog (v1): every check names
/// a catalog evaluator and its parameters meet that evaluator's schema; a
/// Prisma AIRS check needs exactly one usable AIRS connector.
async fn guardrail_checks(
    gw: &AIGatewayClient,
    checks: &[Value],
    before: Option<&Value>,
    options: Option<&RequestOptions>,
) -> Result<(), ChangeError> {
    let catalog = json(&gw.guardrails().get_catalog(options).await?)?;
    let evals = catalog
        .get("evals")
        .and_then(Value::as_array)
        .ok_or(PageError::Invalid)?;
    let previous = before
        .and_then(|b| b.get("checks"))
        .and_then(Value::as_array);
    for (index, check) in checks.iter().enumerate() {
        // An unchanged check is not re-validated (v1).
        if previous
            .and_then(|p| p.get(index))
            .is_some_and(|p| canonical(p) == canonical(check))
        {
            continue;
        }
        let evaluator = evals
            .iter()
            .find(|e| e.get("id").is_some_and(|id| Some(id) == check.get("id")))
            .ok_or(ChangeProblem::Configuration)?;
        let parameters = check
            .get("parameters")
            .cloned()
            .unwrap_or(Value::Object(Map::new()));
        if !evaluator["parameters"].is_object()
            || !valid_parameters(&evaluator["parameters"], &parameters, 0)
        {
            return Err(ChangeProblem::Configuration.into());
        }
    }
    if checks
        .iter()
        .any(|c| c["id"].as_str() == Some("panw-prisma-airs.intercept"))
    {
        let plugins = json(&gw.plugins().list(options).await?)?;
        let connectors: Vec<&Value> = plugins
            .get("data")
            .and_then(Value::as_array)
            .ok_or(PageError::Invalid)?
            .iter()
            .filter(|p| p["integration_slug"].as_str() == Some("panw-prisma-airs"))
            .collect();
        let usable = match connectors.as_slice() {
            [only] => {
                only["is_enabled"] != Value::Bool(false)
                    && only["status"].as_str() != Some("archived")
                    && only["credentials"]["AIRS_API_KEY"]
                        .as_str()
                        .is_some_and(|k| !k.is_empty())
            }
            _ => false,
        };
        if !usable {
            return Err(ChangeProblem::Configuration.into());
        }
    }
    Ok(())
}

/// Checks an integration's configuration against its provider family's
/// concrete SDK schema (v1 `validateConfiguration`; nulls clear optional
/// fields).
async fn integration_configuration(
    gw: &AIGatewayClient,
    body: &Value,
    before: Option<&Value>,
    options: Option<&RequestOptions>,
) -> Result<(), ChangeError> {
    let Some(configurations) = body.get("configurations") else {
        return Ok(());
    };
    let provider = body
        .get("ai_provider_id")
        .or_else(|| before.and_then(|b| b.get("ai_provider_id")))
        .and_then(Value::as_str)
        .ok_or(ChangeProblem::Configuration)?;
    let catalog = json(&gw.integrations().catalog(options).await?)?;
    let family = catalog
        .get("data")
        .and_then(Value::as_array)
        .ok_or(PageError::Invalid)?
        .iter()
        .find(|row| row["id"].as_str() == Some(provider))
        .and_then(|row| row["slug"].as_str())
        .ok_or(ChangeProblem::Configuration)?;
    let Some(schema) = family_schema(family) else {
        return Ok(());
    };
    let Value::Object(map) = configurations else {
        return Err(ChangeProblem::Configuration.into());
    };
    let effective: Map<String, Value> = map
        .iter()
        .filter(|(_, v)| !v.is_null())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    parse(schema, Value::Object(effective), Mode::Request)
        .map(|_| ())
        .map_err(|_| ChangeProblem::Configuration.into())
}

/// Whether an integration holds credentials a configuration change could
/// replace or lose.
fn integration_protected(before: &Value) -> bool {
    let text = |k: &str| before[k].as_str().is_some_and(|s| !s.is_empty());
    text("key")
        || text("masked_key")
        || before["secret_mappings"]
            .as_array()
            .is_some_and(|m| !m.is_empty())
        || routing_document(before.get("configurations")).is_some_and(|c| has_credentials(&c))
}

/// Reads the baseline a review (and its apply) is bound to: the item and
/// what references it, and the checks a create or update needs first.
pub async fn baseline(
    binding: &ManagementBinding,
    mutation: &Mutation,
    scope: Option<&WorkspaceScope>,
    deadline: Instant,
) -> Result<Baseline, ChangeError> {
    use ChangeAction as A;
    use GatewayResource as R;
    let resource = mutation.kind.gateway().ok_or(ChangeProblem::Unsupported)?;
    let gw = binding.gateway();
    let options = ManagementBinding::options(deadline);
    let o = options.as_ref();
    let body = &mutation.body;
    let before = match mutation.action {
        A::Create => None,
        _ => Some(current(gw, resource, &mutation.target, scope, o).await?),
    };
    let mut before = before;
    match (resource, mutation.action) {
        (R::Workspaces, A::Create) => {
            let name = body["scope_name"].as_str().unwrap_or_default();
            let reuse = body[EXISTING_SCOPE] == Value::Bool(true);
            match gw.iam_scopes().get(name, o).await.map_err(PageError::from) {
                Ok(_) if !reuse => return Err(ChangeProblem::ScopeExists.into()),
                Ok(_) => {}
                Err(error) if not_found(&error) && reuse => {
                    return Err(ChangeProblem::ScopeMissing.into());
                }
                Err(error) if not_found(&error) => {}
                Err(error) => return Err(error.into()),
            }
        }
        (R::Guardrails, A::Create | A::Update) => {
            if let Some(checks) = body.get("checks").and_then(Value::as_array) {
                // New checks never carry credentials, so replacing checks
                // that hold some would lose them.
                let stored = before
                    .as_ref()
                    .and_then(|b| b.get("checks"))
                    .is_some_and(has_credentials);
                if stored {
                    return Err(ChangeProblem::ProtectedConfiguration.into());
                }
                guardrail_checks(gw, checks, before.as_ref(), o).await?;
            }
        }
        (R::Routes, A::Update) => {
            let stored = before
                .as_ref()
                .and_then(|b| routing_document(b.get("config")))
                .is_some_and(|c| has_credentials(&c));
            if body.get("config").is_some() && stored {
                return Err(ChangeProblem::ProtectedConfiguration.into());
            }
        }
        (R::Integrations, A::Create | A::Update) => {
            if body.get("configurations").is_some()
                && before.as_ref().is_some_and(integration_protected)
            {
                return Err(ChangeProblem::ProtectedConfiguration.into());
            }
            integration_configuration(gw, body, before.as_ref(), o).await?;
        }
        (R::Integrations, A::Models) => {
            let related = json(&gw.integrations().get_models(&mutation.target, o).await?)?;
            if let Some(Value::Object(map)) = before.as_mut() {
                map.insert("models".into(), related);
            }
        }
        _ => {}
    }
    let references = match (&before, mutation.action) {
        (Some(item), A::Delete) => referencing(gw, resource, item, scope, o).await?,
        _ => Vec::new(),
    };
    Ok(Baseline { before, references })
}

/// What one applied Gateway write returned: its reply (credentials
/// removed) and any one-time key, taken out first.
pub struct Written {
    pub reply: Value,
    pub secret: Option<Zeroizing<String>>,
}

/// Removes a one-time key (`key`) from a write reply, then every
/// credential-bearing field.
pub fn take_key(mut reply: Value) -> Written {
    let secret = reply
        .as_object_mut()
        .and_then(|m| m.remove("key"))
        .and_then(|v| match v {
            Value::String(s) if !s.is_empty() => Some(Zeroizing::new(s)),
            _ => None,
        });
    let mut reply = public_document(&reply);
    crate::management::redact(&mut reply);
    Written { reply, secret }
}

/// Makes the one SDK call a reviewed change makes (retries off; nothing
/// is retried). Workspace creation is [`provision`].
pub async fn write(
    binding: &ManagementBinding,
    mutation: &Mutation,
    tsg: &str,
    deadline: Instant,
) -> Result<Written, PageError> {
    use ChangeAction as A;
    use GatewayResource as R;
    let resource = mutation.kind.gateway().ok_or(PageError::Invalid)?;
    let gw = binding.gateway();
    let options = ManagementBinding::options(deadline);
    let o = options.as_ref();
    let id = mutation.target.as_str();
    let body = &mutation.body;
    let empty = || Ok::<Value, PageError>(Value::Object(Map::new()));
    let reply: Value = match (resource, mutation.action) {
        (R::Workspaces, A::Update) => json(&gw.workspaces().update(id, &model(body)?, o).await?)?,
        (R::Workspaces, A::Delete) => {
            gw.workspaces().delete(id, o).await?;
            empty()?
        }
        (R::Providers, A::Create) => json(&gw.providers().create(&model(body)?, o).await?)?,
        (R::Providers, A::Update) => json(&gw.providers().update(id, &model(body)?, o).await?)?,
        (R::Providers, A::Delete) => {
            gw.providers().delete(id, o).await?;
            empty()?
        }
        (R::Routes, A::Create) => json(&gw.configs().create(&model(body)?, o).await?)?,
        (R::Routes, A::Update) => json(&gw.configs().update(id, &model(body)?, o).await?)?,
        (R::Routes, A::Delete) => {
            gw.configs().delete(id, o).await?;
            empty()?
        }
        (R::Guardrails, A::Create) => json(&gw.guardrails().create(&model(body)?, o).await?)?,
        (R::Guardrails, A::Update) => json(&gw.guardrails().update(id, &model(body)?, o).await?)?,
        (R::Guardrails, A::Delete) => {
            gw.guardrails().delete(id, o).await?;
            empty()?
        }
        (R::ServiceKeys, A::Create) => {
            json(&gw.api_keys().create_service(&model(body)?, o).await?)?
        }
        (R::UserKeys, A::Create) => json(&gw.api_keys().create_user(&model(body)?, o).await?)?,
        (R::ServiceKeys, A::Update) => {
            json(&gw.api_keys().update_service(id, &model(body)?, o).await?)?
        }
        (R::UserKeys, A::Update) => json(&gw.api_keys().update_user(id, &model(body)?, o).await?)?,
        (R::ServiceKeys, A::Rotate) => json(
            &gw.api_keys()
                .rotate_service(id, Some(&model(body)?), o)
                .await?,
        )?,
        (R::UserKeys, A::Rotate) => json(
            &gw.api_keys()
                .rotate_user(id, Some(&model(body)?), o)
                .await?,
        )?,
        (R::ServiceKeys, A::Delete) => {
            gw.api_keys().delete_service(id, o).await?;
            empty()?
        }
        (R::UserKeys, A::Delete) => {
            gw.api_keys().delete_user(id, o).await?;
            empty()?
        }
        (R::Integrations, A::Create) => json(&gw.integrations().create(&model(body)?, o).await?)?,
        (R::Integrations, A::Update) => {
            json(&gw.integrations().update(id, &model(body)?, o).await?)?
        }
        (R::Integrations, A::Delete) => {
            gw.integrations().delete(id, tsg, o).await?;
            empty()?
        }
        (R::Integrations, A::Models) => {
            json(&gw.integrations().set_models(id, &model(body)?, o).await?)?
        }
        _ => return Err(PageError::Invalid),
    };
    Ok(take_key(reply))
}

/// The state of one provisioning step as the operator must know it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum StepState {
    /// Not attempted (an earlier step stopped the workflow).
    NotStarted,
    /// The request was not sent.
    NotSent,
    /// The service refused it; nothing was created by it.
    Rejected,
    /// It may or may not have taken effect.
    Unknown,
    Completed,
    /// An existing IAM scope was reused.
    Existing,
    /// A scope this workflow created was deleted again after the workspace
    /// was definitely not created.
    Deleted,
}

/// What a reconciliation read found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Reconciled {
    Found,
    Absent,
    /// The inventory was larger than one read; absence is not claimed.
    Incomplete,
    Failed,
    /// More than one workspace could be this attempt's; none is claimed.
    Ambiguous,
}

/// What happened to an IAM scope this attempt created once a later step
/// failed (the SDK deletes it only after a workspace definitely refused).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ScopeCleanup {
    /// No scope of this attempt needed removing (none created, a reused
    /// one, or every step completed).
    NotNeeded,
    /// The scope was kept for reconciliation (the workspace exists or may).
    Retained,
    /// The scope was deleted again.
    Deleted,
    /// Deleting it was refused; the scope remains.
    Failed,
    /// Whether it was deleted is not known.
    Unknown,
}

/// One workspace provisioning: each request's outcome and what reading the
/// tenant afterwards found. Local cancellation never claims a rollback.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Provisioning {
    pub scope_name: String,
    pub scope: StepState,
    pub workspace: StepState,
    pub binding: StepState,
    /// The SDK's removal of a scope this attempt created.
    pub cleanup: ScopeCleanup,
    /// The workspace, once its creation is known.
    pub workspace_id: Option<String>,
    pub workspace_slug: Option<String>,
    /// Reconciliation: the IAM scope as read afterwards.
    pub scope_read: Reconciled,
    /// Whether that scope lists this attempt's workspace (when both were
    /// read and the workspace was identified).
    pub bound_read: Option<bool>,
    /// Reconciliation: this attempt's workspace in the admin inventory (by
    /// its identity when known; otherwise the one new workspace with the
    /// requested name and scope).
    pub workspace_read: Reconciled,
}

impl Provisioning {
    fn new(mutation: &Mutation) -> Self {
        Self {
            scope_name: mutation.body["scope_name"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            scope: StepState::NotSent,
            workspace: StepState::NotStarted,
            binding: StepState::NotStarted,
            cleanup: ScopeCleanup::NotNeeded,
            workspace_id: None,
            workspace_slug: None,
            scope_read: Reconciled::Failed,
            bound_read: None,
            workspace_read: Reconciled::Failed,
        }
    }

    /// Every step completed (or reused) and reading back confirmed the
    /// workspace, its scope and the binding.
    pub fn verified(&self) -> bool {
        matches!(self.scope, StepState::Completed | StepState::Existing)
            && self.workspace == StepState::Completed
            && self.binding == StepState::Completed
            && self.scope_read == Reconciled::Found
            && self.workspace_read == Reconciled::Found
            && self.bound_read == Some(true)
    }
}

fn cleanup(r: &scm_sdk::airs::gateway::ProvisionRecovery) -> ScopeCleanup {
    // A reused scope is never this attempt's to remove.
    if r.scope_state == ProvisionScopeState::Existing {
        return ScopeCleanup::NotNeeded;
    }
    match r.cleanup {
        ProvisionCleanup::Retained => ScopeCleanup::Retained,
        ProvisionCleanup::Deleted => ScopeCleanup::Deleted,
        ProvisionCleanup::Failed if r.scope_state == ProvisionScopeState::Unknown => {
            ScopeCleanup::Unknown
        }
        ProvisionCleanup::Failed => ScopeCleanup::Failed,
        _ => ScopeCleanup::NotNeeded,
    }
}

fn step(outcome: scm_sdk::RemoteOutcome) -> StepState {
    match outcome {
        scm_sdk::RemoteOutcome::NotSent => StepState::NotSent,
        scm_sdk::RemoteOutcome::Rejected => StepState::Rejected,
        _ => StepState::Unknown,
    }
}

/// The steps of a failed provisioning, from the SDK's recovery facts.
fn failed_steps(error: &ProvisionError, reuse: bool) -> (StepState, StepState, StepState) {
    let r = error.recovery();
    let scope = match r.scope_state {
        ProvisionScopeState::Existing => StepState::Existing,
        ProvisionScopeState::Created => StepState::Completed,
        ProvisionScopeState::Unknown => StepState::Unknown,
        ProvisionScopeState::Deleted => StepState::Deleted,
        _ if reuse => StepState::Existing,
        _ if r.stage == ProvisionStage::Preflight => StepState::NotSent,
        _ => step(error.error().remote_outcome()),
    };
    let workspace = match (r.stage, r.workspace_state) {
        (_, ProvisionWorkspaceState::Created) => StepState::Completed,
        (_, ProvisionWorkspaceState::Unknown) => StepState::Unknown,
        (ProvisionStage::CreateWorkspace, _) => {
            r.workspace_outcome.map_or(StepState::NotSent, step)
        }
        _ => StepState::NotStarted,
    };
    let binding = match r.stage {
        ProvisionStage::BindWorkspace => step(error.error().remote_outcome()),
        _ => StepState::NotStarted,
    };
    (scope, workspace, binding)
}

/// Creates (or reuses) the IAM scope, the workspace and the binding (the
/// SDK's bounded provisioning; never retried), then reads back what
/// exists. Returns the steps and the failure that stopped it, if any.
pub async fn provision(
    binding: &ManagementBinding,
    mutation: &Mutation,
    prior: Option<&[String]>,
    deadline: Instant,
) -> (Provisioning, Option<scm_sdk::Error>) {
    let gw = binding.gateway();
    let options = ManagementBinding::options(deadline);
    let o = options.as_ref();
    let body = &mutation.body;
    let reuse = body[EXISTING_SCOPE] == Value::Bool(true);
    let params = AIGatewayWorkspaceProvisionOptions {
        existing_scope: reuse,
    };
    let request: Result<GatewayWorkspaceProvisionRequest, _> = model(body);
    let mut report = Provisioning::new(mutation);
    let Ok(request) = request else {
        return (
            report,
            Some(scm_sdk::Error::new(scm_sdk::ErrorKind::Validation)),
        );
    };
    let failure = match gw.workspaces().provision(&request, &params, o).await {
        Ok(done) => {
            report.scope = if reuse {
                StepState::Existing
            } else {
                StepState::Completed
            };
            report.workspace = StepState::Completed;
            report.binding = StepState::Completed;
            report.workspace_id = Some(done.workspace.id.clone());
            report.workspace_slug = Some(done.workspace.slug.clone());
            None
        }
        Err(error) => {
            let (scope, workspace, bound) = failed_steps(&error, reuse);
            report.scope = scope;
            report.workspace = workspace;
            report.binding = bound;
            report.cleanup = cleanup(error.recovery());
            if let Some(created) = &error.recovery().workspace {
                report.workspace_id = Some(created.id.clone());
                report.workspace_slug = Some(created.slug.clone());
            }
            Some(error.into_parts().0)
        }
    };
    reconcile(gw, &mut report, mutation, prior, deadline).await;
    (report, failure)
}

/// The workspaces that already use a reused scope (from one complete admin
/// read; `None` when that cannot be established).
pub async fn prior_workspaces(
    binding: &ManagementBinding,
    body: &Value,
    deadline: Instant,
) -> Option<Vec<String>> {
    let options = ManagementBinding::options(deadline);
    let mut params = AIGatewayWorkspaceListOptions::default();
    params.plane = AIGatewayPlane::Admin;
    let reply = json(
        &binding
            .gateway()
            .workspaces()
            .list(&params, options.as_ref())
            .await
            .ok()?,
    )
    .ok()?;
    let list = crate::gateway::list(GatewayResource::Workspaces, &reply, None)?;
    let scope = body["scope_name"].as_str()?;
    list.complete.then(|| {
        reply["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|w| w["scope_name"].as_str() == Some(scope))
            .filter_map(|w| w["id"].as_str().map(str::to_owned))
            .collect()
    })
}

/// A provisioning cancelled after it started: every step's outcome is
/// unknown here, so the tenant is read back to show what exists.
pub async fn cancelled(
    binding: &ManagementBinding,
    mutation: &Mutation,
    prior: Option<&[String]>,
    deadline: Instant,
) -> Provisioning {
    let reuse = mutation.body[EXISTING_SCOPE] == Value::Bool(true);
    let mut report = Provisioning::new(mutation);
    // A reused scope is never deleted; a created one may have been.
    (report.scope, report.cleanup) = if reuse {
        (StepState::Existing, ScopeCleanup::NotNeeded)
    } else {
        (StepState::Unknown, ScopeCleanup::Unknown)
    };
    report.workspace = StepState::Unknown;
    report.binding = StepState::Unknown;
    reconcile(binding.gateway(), &mut report, mutation, prior, deadline).await;
    report
}

/// Reads back what provisioning left: this attempt's workspace in the admin
/// inventory and the IAM scope (and whether it lists that workspace).
async fn reconcile(
    gw: &AIGatewayClient,
    report: &mut Provisioning,
    mutation: &Mutation,
    prior: Option<&[String]>,
    deadline: Instant,
) {
    let options = ManagementBinding::options(deadline);
    let o = options.as_ref();
    if report.scope == StepState::NotSent && report.workspace == StepState::NotStarted {
        // Nothing was sent: there is nothing to reconcile.
        report.scope_read = Reconciled::Absent;
        report.workspace_read = Reconciled::Absent;
        return;
    }
    let mut params = AIGatewayWorkspaceListOptions::default();
    params.plane = AIGatewayPlane::Admin;
    match gw.workspaces().list(&params, o).await.map(|r| json(&r)) {
        Ok(Ok(reply)) => match crate::gateway::list(GatewayResource::Workspaces, &reply, None) {
            Some(list) => {
                let rows = reply["data"].as_array().cloned().unwrap_or_default();
                report.workspace_read = identify(report, &rows, mutation, prior, list.complete);
            }
            None => report.workspace_read = Reconciled::Failed,
        },
        _ => report.workspace_read = Reconciled::Failed,
    }
    match gw
        .iam_scopes()
        .get(&report.scope_name, o)
        .await
        .map_err(PageError::from)
    {
        Ok(scope) => {
            report.scope_read = Reconciled::Found;
            report.bound_read = match (json(&scope), &report.workspace_slug) {
                (Ok(scope), Some(slug)) => binds(&scope, slug),
                _ => None,
            };
        }
        Err(error) if not_found(&error) => report.scope_read = Reconciled::Absent,
        Err(_) => report.scope_read = Reconciled::Failed,
    }
}

/// Whether a models read shows the reviewed change: every requested
/// model's enablement and, when the draft sets it, the all-models setting.
pub fn models_show(body: &Value, read: &crate::gateway::GatewayModels) -> bool {
    let enabled: Vec<(String, bool)> = serde_json::from_str::<Vec<Value>>(&read.models)
        .unwrap_or_default()
        .iter()
        .filter_map(|m| Some((m["slug"].as_str()?.to_owned(), m["enabled"].as_bool()?)))
        .collect();
    let models = body["models"].as_array().is_none_or(|wanted| {
        wanted.iter().all(|w| {
            enabled.iter().any(|(slug, on)| {
                Some(slug.as_str()) == w["slug"].as_str() && Some(*on) == w["enabled"].as_bool()
            })
        })
    });
    let all = body
        .get("allow_all_models")
        .and_then(Value::as_bool)
        .is_none_or(|wanted| wanted == read.allow_all);
    models && all
}

/// Finds this attempt's workspace among `rows`: by the identity the
/// service returned when known; otherwise the one workspace with the
/// requested name and scope that did not already use the scope. Another
/// workspace sharing a reused scope is never attributed to the attempt.
fn identify(
    report: &mut Provisioning,
    rows: &[Value],
    mutation: &Mutation,
    prior: Option<&[String]>,
    complete: bool,
) -> Reconciled {
    let missing = if complete {
        Reconciled::Absent
    } else {
        Reconciled::Incomplete
    };
    if let Some(id) = report.workspace_id.clone() {
        return match rows.iter().find(|w| w["id"].as_str() == Some(id.as_str())) {
            Some(w) => {
                if report.workspace_slug.is_none() {
                    report.workspace_slug = w["slug"].as_str().map(str::to_owned);
                }
                Reconciled::Found
            }
            None => missing,
        };
    }
    let name = mutation.body["name"].as_str();
    let candidates: Vec<&Value> = rows
        .iter()
        .filter(|w| w["scope_name"].as_str() == Some(report.scope_name.as_str()))
        .filter(|w| w["name"].as_str() == name)
        .filter(|w| {
            let id = w["id"].as_str().unwrap_or_default();
            prior.is_some_and(|p| !p.iter().any(|known| known == id))
        })
        .collect();
    match candidates.as_slice() {
        [w] => {
            report.workspace_id = w["id"].as_str().map(str::to_owned);
            report.workspace_slug = w["slug"].as_str().map(str::to_owned);
            Reconciled::Found
        }
        [] if prior.is_none() => Reconciled::Ambiguous,
        [] => missing,
        _ => Reconciled::Ambiguous,
    }
}

/// What reading a changed item back found.
pub enum ReadBack {
    /// The item, projected for the presentation.
    Found(String),
    /// It is read back but does not show the reviewed change.
    Differs(String),
    Absent,
    Incomplete,
    Failed,
}

/// Reads the changed item back: absent from a complete inventory after a
/// delete or archive; otherwise by its identifier (the reply's for a
/// create). A routing update must show the reviewed document under a new
/// version; a model change the reviewed enablement.
pub async fn readback(
    binding: &ManagementBinding,
    mutation: &Mutation,
    before: Option<&Value>,
    reply: &Value,
    scope: Option<&WorkspaceScope>,
    deadline: Instant,
) -> ReadBack {
    use ChangeAction as A;
    let Some(resource) = mutation.kind.gateway() else {
        return ReadBack::Failed;
    };
    let gw = binding.gateway();
    let options = ManagementBinding::options(deadline);
    let o = options.as_ref();
    if mutation.action == A::Delete {
        let workspace = scope.map(|s| s.id.as_str());
        return match rows(gw, resource, workspace, o).await {
            Ok(rows)
                if rows
                    .iter()
                    .any(|r| r["id"].as_str() == Some(&mutation.target)) =>
            {
                ReadBack::Differs(String::new())
            }
            Ok(_) => ReadBack::Absent,
            Err(ChangeError::Problem(ChangeProblem::InventoryLimit)) => ReadBack::Incomplete,
            Err(_) => ReadBack::Failed,
        };
    }
    if mutation.action == A::Models {
        return match gw.integrations().get_models(&mutation.target, o).await {
            Ok(reply) => match json(&reply).ok().and_then(|r| models(&mutation.target, &r)) {
                Some(read) => {
                    let text = serde_json::to_string(&read).unwrap_or_default();
                    if models_show(&mutation.body, &read) {
                        ReadBack::Found(text)
                    } else {
                        ReadBack::Differs(text)
                    }
                }
                None => ReadBack::Failed,
            },
            Err(_) => ReadBack::Failed,
        };
    }
    let target = match mutation.action {
        A::Create => reply
            .get("id")
            .or_else(|| reply.get("data").and_then(|d| d.get("id")))
            .and_then(Value::as_str)
            .filter(|id| is_id(id))
            .map(str::to_owned),
        _ => Some(mutation.target.clone()),
    };
    let Some(target) = target else {
        return ReadBack::Failed;
    };
    match current(gw, resource, &target, scope, o).await {
        Ok(item) => {
            let shown = shown(mutation.kind, &item).unwrap_or_default();
            let differs = resource == GatewayResource::Routes
                && mutation.action == A::Update
                && mutation.body.get("config").is_some()
                && (routing_document(item.get("config")).map(|c| canonical(&c))
                    != Some(canonical(&mutation.body["config"]))
                    || (before.and_then(|b| b.get("version_id")).is_some()
                        && before.and_then(|b| b.get("version_id")) == item.get("version_id")));
            if differs {
                ReadBack::Differs(shown)
            } else {
                ReadBack::Found(shown)
            }
        }
        Err(ChangeError::Problem(ChangeProblem::NotFound)) => ReadBack::Absent,
        Err(ChangeError::Problem(ChangeProblem::InventoryLimit)) => ReadBack::Incomplete,
        Err(_) => ReadBack::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const TSG: &str = "1234567890";
    const WS: &str = "16f7e90d-382a-4e78-b577-1b01eb5f8297";
    const OTHER: &str = "16f7e90d-382a-4e78-b577-1b01eb5f8298";
    const ID: &str = "7e3a0000-0000-4000-8000-000000000001";

    fn draft(kind: ChangeKind, action: ChangeAction, target: &str, body: Value) -> ChangeDraft {
        ChangeDraft {
            kind,
            action,
            target: target.into(),
            body: body.to_string(),
            workspace: None,
        }
    }

    #[test]
    fn a_models_readback_needs_every_requested_model_and_the_all_models_setting() {
        let read = |all: bool| {
            crate::gateway::models(
                ID,
                &json!({"object": "list", "allow_all_models": all, "total": 2, "models": [
                    {"slug": "gpt-4o", "enabled": true}, {"slug": "o3", "enabled": false}]}),
            )
            .expect("models")
        };
        let rows = json!([{"slug": "gpt-4o", "enabled": true}]);
        // No all-models setting in the draft: only the rows count.
        assert!(models_show(&json!({"models": rows}), &read(true)));
        // Both values, taken as reviewed.
        assert!(models_show(
            &json!({"models": rows, "allow_all_models": false}),
            &read(false)
        ));
        assert!(models_show(
            &json!({"models": rows, "allow_all_models": true}),
            &read(true)
        ));
        // Matching rows but an ignored setting: differs.
        assert!(!models_show(
            &json!({"models": rows, "allow_all_models": false}),
            &read(true)
        ));
        assert!(!models_show(
            &json!({"models": rows, "allow_all_models": true}),
            &read(false)
        ));
        // A requested row the service did not take.
        let off = json!([{"slug": "o3", "enabled": true}]);
        assert!(!models_show(
            &json!({"models": off, "allow_all_models": false}),
            &read(false)
        ));
    }

    #[test]
    fn only_the_v1_gateway_matrix_is_accepted() {
        for (kind, action) in [
            (ChangeKind::GatewayWorkspaces, ChangeAction::Rotate),
            (ChangeKind::GatewayProviders, ChangeAction::Models),
            (ChangeKind::GatewayRoutes, ChangeAction::Version),
            (ChangeKind::GatewayIntegrations, ChangeAction::Rotate),
            (ChangeKind::Profiles, ChangeAction::Create),
        ] {
            assert_eq!(
                normalize(&draft(kind, action, ID, json!({})), TSG, Some(WS)),
                Err(ChangeProblem::Unsupported),
                "{kind:?} {action:?}"
            );
        }
    }

    #[test]
    fn scoped_creates_are_placed_in_the_chosen_workspace_only() {
        let body = json!({"name": "ci", "scopes": ["completions.write"], "type": "service"});
        let m = normalize(
            &draft(
                ChangeKind::GatewayServiceKeys,
                ChangeAction::Create,
                "",
                body.clone(),
            ),
            TSG,
            Some(WS),
        )
        .expect("normalized");
        assert_eq!(m.body["workspace_id"], WS);
        assert_eq!(m.body["organisation_id"], TSG);
        let mut other = body.clone();
        other["workspace_id"] = json!(OTHER);
        assert_eq!(
            normalize(
                &draft(
                    ChangeKind::GatewayServiceKeys,
                    ChangeAction::Create,
                    "",
                    other
                ),
                TSG,
                Some(WS)
            ),
            Err(ChangeProblem::WorkspaceMismatch)
        );
        assert_eq!(
            normalize(
                &draft(
                    ChangeKind::GatewayServiceKeys,
                    ChangeAction::Create,
                    "",
                    body
                ),
                TSG,
                None
            ),
            Err(ChangeProblem::WorkspaceMismatch)
        );
    }

    #[test]
    fn drafts_never_carry_credentials() {
        for (kind, body) in [
            (
                ChangeKind::GatewayIntegrations,
                json!({"ai_provider_id": "x", "name": "n", "slug": "n", "key": "sk-1"}),
            ),
            (
                ChangeKind::GatewayIntegrations,
                json!({"ai_provider_id": "x", "name": "n", "slug": "n", "configurations": {"api_key": "sk-1"}}),
            ),
            (
                ChangeKind::GatewayRoutes,
                json!({"name": "r", "config": {"targets": [{"provider": "@p", "api_key": "sk-1"}]}}),
            ),
            (
                ChangeKind::GatewayGuardrails,
                json!({"name": "g", "checks": [{"id": "c", "parameters": {"apiKey": "sk-1"}}]}),
            ),
        ] {
            assert_eq!(
                normalize(&draft(kind, ChangeAction::Create, "", body), TSG, Some(WS)),
                Err(ChangeProblem::CredentialPolicy),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn provisioning_names_its_exact_scope() {
        let m = normalize(
            &draft(
                ChangeKind::GatewayWorkspaces,
                ChangeAction::Create,
                "",
                json!({"name": "Team C", "scope_name": "scope-team-c", "existing_scope": true}),
            ),
            TSG,
            None,
        )
        .expect("normalized");
        assert_eq!(m.body[EXISTING_SCOPE], true);
        for body in [
            json!({"name": "Team C"}),
            json!({"name": "Team C", "scope_name": "bad name"}),
        ] {
            assert_eq!(
                normalize(
                    &draft(
                        ChangeKind::GatewayWorkspaces,
                        ChangeAction::Create,
                        "",
                        body
                    ),
                    TSG,
                    None
                ),
                Err(ChangeProblem::Contract)
            );
        }
    }

    #[test]
    fn deletes_carry_nothing_and_targets_are_identifiers() {
        assert_eq!(
            normalize(
                &draft(
                    ChangeKind::GatewayProviders,
                    ChangeAction::Delete,
                    ID,
                    json!({"x": 1})
                ),
                TSG,
                Some(WS)
            ),
            Err(ChangeProblem::Body)
        );
        assert_eq!(
            normalize(
                &draft(
                    ChangeKind::GatewayProviders,
                    ChangeAction::Delete,
                    "../x",
                    json!({})
                ),
                TSG,
                Some(WS)
            ),
            Err(ChangeProblem::Target)
        );
    }

    #[test]
    fn references_match_ids_and_at_paths_only() {
        let doc = json!({"targets": [{"provider": "@openai-prod/gpt"}, {"config": ID}]});
        assert!(references(&doc, &["openai-prod"]));
        assert!(references(&doc, &[ID]));
        assert!(!references(&doc, &["openai"]));
        assert!(!references(&doc, &[""]));
    }

    #[test]
    fn counters_never_make_a_review_conflict() {
        let a = json!({"name": "k", "usage_limits": {"current_usage": 3, "credit_limit": 10}});
        let b = json!({"name": "k", "usage_limits": {"current_usage": 9, "credit_limit": 10}});
        assert_eq!(canonical(&stable(&a)), canonical(&stable(&b)));
    }

    #[test]
    fn catalog_parameters_are_checked_and_fail_closed() {
        let schema = json!({"type": "object", "properties": {
            "scan_scope": {"type": "string", "enum": ["last_message", "all_messages"]},
            "timeout": {"type": "number", "minimum": 1, "maximum": 60}}, "additionalProperties": false});
        assert!(valid_parameters(
            &schema,
            &json!({"scan_scope": "all_messages", "timeout": 5}),
            0
        ));
        assert!(!valid_parameters(
            &schema,
            &json!({"scan_scope": "everything"}),
            0
        ));
        assert!(!valid_parameters(&schema, &json!({"timeout": 99}), 0));
        assert!(!valid_parameters(&schema, &json!({"extra": true}), 0));
        assert!(!valid_parameters(
            &json!({"type": "string", "pattern": "^a"}),
            &json!("a"),
            0
        ));
    }

    #[test]
    fn a_one_time_key_is_taken_before_anything_is_shown() {
        let written = take_key(json!({"id": ID, "key": "gw-SECRET", "data": {"api_key": "x"}}));
        assert_eq!(
            written.secret.as_deref().map(String::as_str),
            Some("gw-SECRET")
        );
        assert!(!written.reply.to_string().contains("SECRET"));
        assert!(!written.reply.to_string().contains("api_key"));
    }
}
