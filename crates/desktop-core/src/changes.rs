//! Reviewed Runtime changes (C09): the exact v1 write subset for security
//! profiles, custom topics, applications and API keys, its validation and
//! normalization, the review baseline digest, the before/after preview and
//! the handling of one-time secrets in write responses.
//!
//! Working v1 (74754c02, read-only): `bridge/src/protocol.ts`
//! `validateMutation` (allowed actions, target and body rules, SDK schema
//! parsing), `bridge/src/runtime.ts` (authorship, key creation checks, the
//! baseline and review digests, confirmation text, preview and impact) and
//! `bridge/src/operations.ts` `mutateLive` (the SDK call per action).
//! Nothing here performs IO.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use ts_rs::TS;
use zeroize::Zeroizing;

use crate::management::{ManagementResource, redact};

/// Largest draft body accepted (bytes of JSON text).
pub const MAX_BODY_BYTES: usize = 1024 * 1024;
/// Longest target identifier (v1).
pub const MAX_TARGET: usize = 256;
/// Longest custom topic description in code points (v1).
pub const MAX_TOPIC_DESCRIPTION: usize = 240;
/// Key rotation intervals v1 offers (days).
pub const ROTATION_DAYS: [u64; 3] = [30, 90, 180];

/// What a change applies to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ChangeKind {
    Profiles,
    Topics,
    Applications,
    ApiKeys,
}

impl ChangeKind {
    /// The inventory a change's baseline is read from.
    pub fn resource(self) -> ManagementResource {
        match self {
            Self::Profiles => ManagementResource::Profiles,
            Self::Topics => ManagementResource::Topics,
            Self::Applications => ManagementResource::Applications,
            Self::ApiKeys => ManagementResource::ApiKeys,
        }
    }

    /// The field naming an item for the operator (and the typed confirmation).
    pub fn name_field(self) -> &'static str {
        match self {
            Self::Profiles => "profile_name",
            Self::Topics => "topic_name",
            Self::Applications => "app_name",
            Self::ApiKeys => "api_key_name",
        }
    }
}

/// The action of a change (v1's allowed matrix).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ChangeAction {
    Create,
    Update,
    Delete,
    /// An additional API key for an existing application.
    Version,
    /// Regenerate an API key.
    Rotate,
}

/// The operator's captured draft, exactly as entered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeDraft {
    pub kind: ChangeKind,
    pub action: ChangeAction,
    /// The item's identifier (empty for a create).
    pub target: String,
    /// The body as JSON text (an object).
    pub body: String,
}

/// Why a draft or its review could not proceed (sanitized; no server text).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ChangeProblem {
    /// The action is not supported for this kind (v1 matrix).
    Unsupported,
    /// The target is missing, too long or not the item's identifier.
    Target,
    /// The body is not a JSON object within limits.
    Body,
    /// The body does not satisfy the service contract.
    Contract,
    /// A custom topic description over 240 characters.
    Description,
    /// A key rotation other than 30, 90 or 180 days.
    Rotation,
    /// A required key field is blank.
    KeyFields,
    /// The named application already exists (key create).
    ApplicationExists,
    /// An API key with that name already exists.
    KeyExists,
    /// The inventory was larger than the bounded check could read.
    InventoryLimit,
    /// The deployment profile is not exactly one known profile with a code.
    DeploymentProfile,
    /// The key version's application differs from the source key's.
    SourceMismatch,
    /// The item to change was not found in its complete inventory.
    NotFound,
}

/// A validated, normalized change (v1 `validateMutation` plus authorship).
#[derive(Clone, Debug, PartialEq)]
pub struct Mutation {
    pub kind: ChangeKind,
    pub action: ChangeAction,
    pub target: String,
    /// Normalized body (the SDK request contract's output).
    pub body: Value,
}

fn allowed(kind: ChangeKind, action: ChangeAction) -> bool {
    use ChangeAction::*;
    matches!(
        (kind, action),
        (
            ChangeKind::Profiles | ChangeKind::Topics,
            Create | Update | Delete
        ) | (ChangeKind::Applications, Update | Delete)
            | (ChangeKind::ApiKeys, Create | Version | Rotate | Delete)
    )
}

fn text(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_str)
        .is_some_and(|s| !s.trim().is_empty())
}

/// The SDK request schema a kind/action's body is parsed with.
fn schema(kind: ChangeKind, action: ChangeAction) -> Option<&'static str> {
    match (kind, action) {
        (_, ChangeAction::Delete) => None,
        (ChangeKind::Profiles, _) => Some("CreateSecurityProfileRequestSchema"),
        (ChangeKind::Topics, _) => Some("CreateCustomTopicRequestSchema"),
        (ChangeKind::Applications, _) => Some("CustomerAppSchema"),
        (ChangeKind::ApiKeys, ChangeAction::Rotate) => Some("ApiKeyRegenerateRequestSchema"),
        (ChangeKind::ApiKeys, _) => Some("ApiKeyCreateRequestSchema"),
    }
}

/// Validates and normalizes a captured draft for the management client
/// `client_id` (authorship belongs to the trusted credential boundary).
/// A key create or version still needs its deployment profile's `auth_code`
/// ([`with_auth_code`]) before the contract check; [`normalize`] checks
/// everything else.
pub fn normalize(draft: &ChangeDraft, client_id: &str) -> Result<Mutation, ChangeProblem> {
    let (kind, action) = (draft.kind, draft.action);
    if !allowed(kind, action) {
        return Err(ChangeProblem::Unsupported);
    }
    if draft.target.chars().count() > MAX_TARGET
        || (action != ChangeAction::Create && draft.target.trim().is_empty())
        || (action == ChangeAction::Create && !draft.target.is_empty())
        || draft.target.chars().any(char::is_control)
    {
        return Err(ChangeProblem::Target);
    }
    if draft.body.len() > MAX_BODY_BYTES {
        return Err(ChangeProblem::Body);
    }
    let Ok(Value::Object(mut body)) = serde_json::from_str::<Value>(&draft.body) else {
        return Err(ChangeProblem::Body);
    };
    let author = Value::String(client_id.to_owned());
    if action == ChangeAction::Delete {
        // v1 requires exactly {updated_by}; the desktop supplies it.
        let mut only = Map::new();
        only.insert("updated_by".into(), author);
        return Ok(Mutation {
            kind,
            action,
            target: draft.target.clone(),
            body: Value::Object(only),
        });
    }
    // Server-assigned or secret fields never come from a draft.
    body.remove("auth_code");
    body.remove("api_key");
    match kind {
        ChangeKind::Profiles | ChangeKind::Topics => {
            let field = if action == ChangeAction::Create {
                "created_by"
            } else {
                "updated_by"
            };
            body.insert(field.into(), author);
            body.remove("last_modified_ts");
            if kind == ChangeKind::Topics {
                body.remove("created_ts");
                if body
                    .get("description")
                    .and_then(Value::as_str)
                    .is_some_and(|d| d.chars().count() > MAX_TOPIC_DESCRIPTION)
                {
                    return Err(ChangeProblem::Description);
                }
            }
        }
        ChangeKind::Applications => {
            body.insert("updated_by".into(), author);
        }
        ChangeKind::ApiKeys => {
            let creates = matches!(action, ChangeAction::Create | ChangeAction::Version);
            body.insert(
                (if creates { "created_by" } else { "updated_by" }).into(),
                author,
            );
            let interval = body.get("rotation_time_interval").and_then(Value::as_f64);
            if body.get("rotation_time_unit").and_then(Value::as_str) != Some("days")
                || !interval.is_some_and(|i| ROTATION_DAYS.iter().any(|d| *d as f64 == i))
            {
                return Err(ChangeProblem::Rotation);
            }
            if creates {
                if ![
                    "cust_env",
                    "cust_cloud_provider",
                    "cust_app",
                    "api_key_name",
                    "dp_name",
                ]
                .iter()
                .all(|f| text(body.get(*f)))
                {
                    return Err(ChangeProblem::KeyFields);
                }
            } else {
                let mut reduced = Map::new();
                for field in ["rotation_time_interval", "rotation_time_unit", "updated_by"] {
                    if let Some(value) = body.remove(field) {
                        reduced.insert(field.into(), value);
                    }
                }
                body = reduced;
            }
        }
    }
    let mut mutation = Mutation {
        kind,
        action,
        target: draft.target.clone(),
        body: Value::Object(body),
    };
    // A key create waits for its auth code; everything else is checked now.
    if !matches!(
        (kind, action),
        (
            ChangeKind::ApiKeys,
            ChangeAction::Create | ChangeAction::Version
        )
    ) {
        mutation.body = contract(kind, action, mutation.body)?;
    }
    Ok(mutation)
}

/// Parses `body` with the SDK request contract (v1's schema parse).
fn contract(kind: ChangeKind, action: ChangeAction, body: Value) -> Result<Value, ChangeProblem> {
    match schema(kind, action) {
        None => Ok(body),
        Some(schema) => {
            scm_sdk::validation::parse(schema, body, scm_sdk::validation::Mode::Request)
                .map_err(|_| ChangeProblem::Contract)
        }
    }
}

/// Adds the deployment profile's `auth_code` to a key create or version
/// (v1 injects it server-side; it is never operator input) and checks the
/// contract. `deployments` is the deployment profile list.
pub fn with_auth_code(
    mutation: &Mutation,
    deployments: &[Value],
) -> Result<Mutation, ChangeProblem> {
    let dp_name = mutation.body.get("dp_name").and_then(Value::as_str);
    let matches: Vec<&Value> = deployments
        .iter()
        .filter(|d| d.get("dp_name").and_then(Value::as_str) == dp_name)
        .collect();
    let [only] = matches.as_slice() else {
        return Err(ChangeProblem::DeploymentProfile);
    };
    let Some(code) = only
        .get("auth_code")
        .and_then(Value::as_str)
        .filter(|c| !c.trim().is_empty())
    else {
        return Err(ChangeProblem::DeploymentProfile);
    };
    let mut body = mutation.body.clone();
    if let Value::Object(map) = &mut body {
        map.insert("auth_code".into(), Value::String(code.to_owned()));
    }
    Ok(Mutation {
        body: contract(mutation.kind, mutation.action, body)?,
        ..mutation.clone()
    })
}

/// Canonical JSON (object keys sorted), as v1's `canonical`.
pub fn canonical(value: &Value) -> String {
    match value {
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            format!(
                "{{{}}}",
                keys.iter()
                    .map(|k| format!("{}:{}", Value::String((*k).clone()), canonical(&map[*k])))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        other => other.to_string(),
    }
}

/// SHA-256 of the canonical form, hex.
pub fn digest(value: &Value) -> String {
    hex::encode(Sha256::digest(canonical(value).as_bytes()))
}

/// The operator-facing name of an item (v1 `resourceName`).
pub fn item_name(kind: ChangeKind, item: &Value) -> Option<String> {
    item.get(kind.name_field())
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// The confirmation the operator types: "CREATE" for a create or key
/// version, otherwise the item's name (v1).
pub fn confirmation(mutation: &Mutation, before: Option<&Value>) -> Option<String> {
    match mutation.action {
        ChangeAction::Create | ChangeAction::Version => Some("CREATE".into()),
        _ => before.and_then(|b| item_name(mutation.kind, b)),
    }
}

/// The impact statement (v1's text, verbatim).
pub fn impact(mutation: &Mutation) -> &'static str {
    match (mutation.kind, mutation.action) {
        (ChangeKind::Applications, ChangeAction::Delete) => {
            "Deletes this application and ALL associated API keys. The before snapshot includes the complete matching key inventory."
        }
        (_, ChangeAction::Version) => {
            "Creates an additional API key for this application. The existing key is unchanged; no key is regenerated or deleted."
        }
        (_, ChangeAction::Rotate) => {
            "Replaces this key; update every integration using it. The configured scan credential is unchanged."
        }
        (_, ChangeAction::Delete) => {
            "Removes this resource. Referenced profiles/topics may be rejected by the server."
        }
        _ => {
            "Applies only to this tenant. Existing unknown policy fields are preserved when included in the draft."
        }
    }
}

/// The expected result shown in review (v1 preview `after`), secrets
/// removed: server-determined fields are labelled, a delete has none.
pub fn preview_after(mutation: &Mutation, before: Option<&Value>) -> Option<Value> {
    let merged = |labelled: &[&str], label: &str| -> Option<Value> {
        let mut after = before?.as_object()?.clone();
        for (key, value) in mutation.body.as_object()? {
            after.insert(key.clone(), value.clone());
        }
        for key in labelled {
            if after.contains_key(*key) {
                after.insert((*key).into(), Value::String(label.into()));
            }
        }
        Some(Value::Object(after))
    };
    let after = match (mutation.kind, mutation.action) {
        (_, ChangeAction::Delete) => None,
        (_, ChangeAction::Rotate) => merged(
            &[
                "expiration",
                "api_key_last8",
                "creation_ts",
                "last_modified_ts",
            ],
            "Determined by server on rotation",
        ),
        (ChangeKind::Profiles, ChangeAction::Update) => merged(
            &["profile_id", "revision", "last_modified_ts"],
            "Determined by server on update",
        ),
        (ChangeKind::Topics, ChangeAction::Update) => {
            merged(&["last_modified_ts"], "Determined by server on update")
        }
        (_, ChangeAction::Update) => merged(&[], ""),
        _ => Some(mutation.body.clone()),
    };
    after.map(|mut value| {
        redact(&mut value);
        value
    })
}

/// Removes a one-time API key from a write response, returning it in a
/// zeroizing buffer, and the rest of the response with secret fields
/// removed (`api_key`, `auth_code`, ...).
pub fn take_secret(mut response: Value) -> (Value, Option<Zeroizing<String>>) {
    let secret = response
        .as_object_mut()
        .and_then(|map| map.remove("api_key"))
        .and_then(|v| match v {
            Value::String(s) if !s.is_empty() => Some(Zeroizing::new(s)),
            _ => None,
        });
    redact(&mut response);
    (response, secret)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const CLIENT: &str = "svc@1234567890.iam.panserviceaccount.com";

    fn draft(kind: ChangeKind, action: ChangeAction, target: &str, body: Value) -> ChangeDraft {
        ChangeDraft {
            kind,
            action,
            target: target.into(),
            body: body.to_string(),
        }
    }

    #[test]
    fn only_v1_actions_are_supported() {
        let body = json!({});
        for (kind, action) in [
            (ChangeKind::Applications, ChangeAction::Create),
            (ChangeKind::ApiKeys, ChangeAction::Update),
            (ChangeKind::Profiles, ChangeAction::Rotate),
            (ChangeKind::Topics, ChangeAction::Version),
        ] {
            assert_eq!(
                normalize(&draft(kind, action, "x", body.clone()), CLIENT),
                Err(ChangeProblem::Unsupported)
            );
        }
    }

    #[test]
    fn deletes_carry_only_the_desktops_author() {
        let m = normalize(
            &draft(
                ChangeKind::Topics,
                ChangeAction::Delete,
                "t-1",
                json!({"updated_by": "someone-else", "force": true}),
            ),
            CLIENT,
        )
        .expect("delete");
        assert_eq!(m.body, json!({"updated_by": CLIENT}));
        assert_eq!(
            normalize(
                &draft(ChangeKind::Topics, ChangeAction::Delete, " ", json!({})),
                CLIENT
            ),
            Err(ChangeProblem::Target)
        );
    }

    #[test]
    fn topics_are_authored_stripped_and_bounded() {
        let m = normalize(
            &draft(
                ChangeKind::Topics,
                ChangeAction::Create,
                "",
                json!({"topic_name": "Finance", "description": "Money", "examples": ["a"],
                       "last_modified_ts": "x", "created_ts": "y", "created_by": "forged"}),
            ),
            CLIENT,
        )
        .expect("topic");
        assert_eq!(m.body["created_by"], CLIENT);
        assert!(m.body.get("last_modified_ts").is_none() && m.body.get("created_ts").is_none());
        let long = "x".repeat(241);
        assert_eq!(
            normalize(
                &draft(
                    ChangeKind::Topics,
                    ChangeAction::Create,
                    "",
                    json!({"topic_name": "F", "description": long})
                ),
                CLIENT
            ),
            Err(ChangeProblem::Description)
        );
        assert_eq!(
            normalize(
                &draft(
                    ChangeKind::Topics,
                    ChangeAction::Create,
                    "t",
                    json!({"topic_name": "F"})
                ),
                CLIENT
            ),
            Err(ChangeProblem::Target),
            "a create names no target"
        );
    }

    #[test]
    fn key_rotation_is_reduced_and_bounded() {
        let m = normalize(
            &draft(
                ChangeKind::ApiKeys,
                ChangeAction::Rotate,
                "key-1",
                json!({"rotation_time_interval": 90, "rotation_time_unit": "days",
                       "api_key_name": "renamed", "api_key": "LEAK", "auth_code": "LEAK"}),
            ),
            CLIENT,
        )
        .expect("rotate");
        assert_eq!(
            m.body,
            json!({"rotation_time_interval": 90, "rotation_time_unit": "days", "updated_by": CLIENT})
        );
        for (interval, unit) in [(45, "days"), (90, "weeks")] {
            assert_eq!(
                normalize(
                    &draft(
                        ChangeKind::ApiKeys,
                        ChangeAction::Rotate,
                        "key-1",
                        json!({"rotation_time_interval": interval, "rotation_time_unit": unit})
                    ),
                    CLIENT
                ),
                Err(ChangeProblem::Rotation)
            );
        }
    }

    #[test]
    fn key_creation_needs_its_fields_and_one_deployment_profiles_code() {
        let body = json!({"cust_app": "Bot", "cust_env": "production", "cust_cloud_provider": "aws",
                          "api_key_name": "bot-key", "dp_name": "dp-1", "revoked": false,
                          "rotation_time_interval": 30, "rotation_time_unit": "days",
                          "auth_code": "FORGED"});
        let m = normalize(
            &draft(ChangeKind::ApiKeys, ChangeAction::Create, "", body.clone()),
            CLIENT,
        )
        .expect("create");
        assert!(m.body.get("auth_code").is_none(), "never from the draft");
        let mut blank = body.clone();
        blank["cust_env"] = json!(" ");
        assert_eq!(
            normalize(
                &draft(ChangeKind::ApiKeys, ChangeAction::Create, "", blank),
                CLIENT
            ),
            Err(ChangeProblem::KeyFields)
        );
        let deployments = [
            json!({"dp_name": "dp-1", "auth_code": "CODE-1"}),
            json!({"dp_name": "dp-2", "auth_code": "CODE-2"}),
        ];
        let ready = with_auth_code(&m, &deployments).expect("code");
        assert_eq!(ready.body["auth_code"], "CODE-1");
        assert_eq!(ready.body["created_by"], CLIENT);
        let twice = [deployments[0].clone(), deployments[0].clone()];
        assert_eq!(
            with_auth_code(&m, &twice),
            Err(ChangeProblem::DeploymentProfile)
        );
        assert_eq!(
            with_auth_code(&m, &[json!({"dp_name": "dp-1", "auth_code": ""})]),
            Err(ChangeProblem::DeploymentProfile)
        );
    }

    #[test]
    fn digests_are_canonical_and_previews_label_server_fields() {
        assert_eq!(
            digest(&json!({"b": 1, "a": [2, {"d": 3, "c": 4}]})),
            digest(&json!({"a": [2, {"c": 4, "d": 3}], "b": 1}))
        );
        assert_eq!(canonical(&json!({"b": 1, "a": "x"})), r#"{"a":"x","b":1}"#);
        let before = json!({"api_key_id": "k", "api_key_name": "Runtime", "expiration": "2027",
                            "api_key_last8": "abcd1234", "auth_code": "SECRET"});
        let m = Mutation {
            kind: ChangeKind::ApiKeys,
            action: ChangeAction::Rotate,
            target: "k".into(),
            body: json!({"rotation_time_interval": 90, "rotation_time_unit": "days"}),
        };
        let after = preview_after(&m, Some(&before)).expect("after");
        assert_eq!(after["expiration"], "Determined by server on rotation");
        assert_eq!(after["rotation_time_interval"], 90);
        assert!(after.get("auth_code").is_none());
        assert_eq!(confirmation(&m, Some(&before)).as_deref(), Some("Runtime"));
        assert_eq!(
            impact(&m),
            "Replaces this key; update every integration using it. The configured scan credential is unchanged."
        );
    }

    #[test]
    fn write_responses_give_up_their_one_time_secret() {
        let (rest, secret) = take_secret(json!({"api_key_id": "k", "api_key": "ONE-TIME-SECRET",
                                                "auth_code": "CODE", "api_key_last8": "SECRET"}));
        assert_eq!(
            secret.as_deref().map(String::as_str),
            Some("ONE-TIME-SECRET")
        );
        assert!(!rest.to_string().contains("ONE-TIME-SECRET") && rest.get("auth_code").is_none());
        assert_eq!(rest["api_key_last8"], "SECRET");
    }
}
