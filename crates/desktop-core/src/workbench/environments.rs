//! Environments and variables (C14, v1's rules). Shared environments are
//! `environments/<name>.toml` files in the collection; private ones live
//! only on this Mac (sealed with the workbench profile) and are never
//! committed. A variable is a plain string or `{secret = "reference"}`;
//! files hold the reference, never the value. Precedence (later wins):
//! collection defaults, private `_defaults`, the selected shared
//! environment, the selected private environment, the request's own
//! `[variables]`. The preview resolves `{{name}}` one level deep with every
//! secret masked; nothing here sends anything.

use std::collections::BTreeMap;

use serde::Serialize;
use toml_edit::{DocumentMut, Item};
use ts_rs::TS;

use super::document::{self, Fields};

/// Environment files (v1: 1 MiB).
pub const ENVIRONMENT_BYTES: usize = 1024 * 1024;
/// Private environments kept per collection.
pub const MAX_PRIVATE: usize = 128;
/// The reserved private environment applied under every selection.
pub const DEFAULTS: &str = "_defaults";

pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}

/// An environment's text: `format_version = 1` and `[variables]` only.
pub fn text_ok(text: &str) -> Option<DocumentMut> {
    let doc: DocumentMut = text.parse().ok()?;
    (super::project::format_one(&doc)
        && doc
            .iter()
            .all(|(k, _)| matches!(k, "format_version" | "variables"))
        && document::variables_ok(&doc))
    .then_some(doc)
}

pub fn new_text() -> String {
    "format_version = 1\n\n[variables]\n".to_owned()
}

/// Where an environment comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Origin {
    Shared,
    Local,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Layer {
    Collection,
    LocalDefaults,
    Shared,
    Local,
    Request,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Value {
    Plain(String),
    Secret(String),
}

/// One effective variable (a secret's value never appears).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VariableView {
    pub name: String,
    pub layer: Layer,
    /// The secret reference, for secret variables.
    pub secret: Option<String>,
    /// Whether that secret has a value on this Mac.
    pub stored: Option<bool>,
}

/// The layers a preview resolves with (texts as read or held).
#[derive(Clone, Debug, Default)]
pub struct Layers {
    pub collection: Option<String>,
    pub local_defaults: Option<String>,
    pub shared: Option<String>,
    pub local: Option<String>,
}

/// The request as it would be prepared, with every secret masked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub query: Vec<(String, String)>,
    pub body_kind: String,
    /// The first 4 KiB of the rendered body.
    pub body: String,
    pub body_bytes: u32,
    pub variables: Vec<VariableView>,
    /// Variables used but not defined.
    pub unresolved: Vec<String>,
    /// Secret references used but without a value on this Mac.
    pub missing_secrets: Vec<String>,
    /// The auth kind and the references it names (never values).
    pub auth: Option<String>,
    pub auth_references: Vec<String>,
    /// The request cannot be prepared as typed.
    pub problem: Option<document::FieldProblem>,
    /// A JSON body that does not parse (sent as typed).
    pub json_warning: bool,
}

pub const MASK: &str = "••••••";
const PREVIEW_BODY: usize = 4 * 1024;

fn layer_values(text: &str, layer: Layer, into: &mut BTreeMap<String, (Value, Layer)>) {
    let Ok(doc) = text.parse::<DocumentMut>() else {
        return;
    };
    let Some(table) = doc.get("variables").and_then(Item::as_table_like) else {
        return;
    };
    for (name, value) in table.iter() {
        let value = match value.as_str() {
            Some(v) => Value::Plain(v.to_owned()),
            None => match value.get("secret").and_then(Item::as_str) {
                Some(r) => Value::Secret(r.to_owned()),
                None => continue,
            },
        };
        into.insert(name.to_owned(), (value, layer));
    }
}

/// `{{name}}` substitution, one level, `{{{` escapes a brace (v1).
fn substitute(
    text: &str,
    values: &BTreeMap<String, (Value, Layer)>,
    used: &mut Vec<String>,
    unresolved: &mut Vec<String>,
) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        if rest[start + 2..].starts_with('{') {
            out.push_str(&rest[..start + 1]);
            rest = &rest[start + 1..];
            continue;
        }
        out.push_str(&rest[..start]);
        let Some(end) = rest[start + 2..].find("}}") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let raw = &rest[start + 2..start + 2 + end];
        let name = raw.trim();
        match values.get(name) {
            Some((Value::Plain(v), _)) if !v.contains("{{") => {
                used.push(name.to_owned());
                out.push_str(v);
            }
            Some((Value::Secret(_), _)) => {
                used.push(name.to_owned());
                out.push_str(MASK);
            }
            _ => {
                if document::valid_variable(name) && !unresolved.iter().any(|u| u == name) {
                    unresolved.push(name.to_owned());
                }
                out.push_str("{{");
                out.push_str(raw);
                out.push_str("}}");
            }
        }
        rest = &rest[start + 2 + end + 2..];
    }
    out.push_str(rest);
    out
}

/// Previews `fields` under `layers`; `stored` says whether a secret
/// reference has a value on this Mac.
pub fn preview(fields: &Fields, layers: &Layers, stored: &dyn Fn(&str) -> bool) -> Preview {
    let mut values = BTreeMap::new();
    for (text, layer) in [
        (&layers.collection, Layer::Collection),
        (&layers.local_defaults, Layer::LocalDefaults),
        (&layers.shared, Layer::Shared),
        (&layers.local, Layer::Local),
        (&Some(fields.settings.clone()), Layer::Request),
    ] {
        if let Some(text) = text {
            layer_values(text, layer, &mut values);
        }
    }
    let mut used = Vec::new();
    let mut unresolved = Vec::new();
    let problem = document::check(fields).err();
    let mut sub = |t: &str| substitute(t, &values, &mut used, &mut unresolved);
    let url = sub(&fields.url);
    let headers = document::header_rows(&fields.headers)
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.enabled)
        .map(|r| (sub(&r.name), sub(&r.value)))
        .collect();
    let query = document::query_rows(&fields.query)
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.enabled)
        .map(|r| (sub(&r.name), sub(&r.value)))
        .collect();
    let body = if fields.body_kind == "file" {
        String::new()
    } else {
        sub(&fields.body)
    };
    let (auth, auth_references) = match document::settings_ok(&fields.settings) {
        Some(doc) => match doc.get("auth") {
            Some(a) => (
                a.get("kind").and_then(Item::as_str).map(str::to_owned),
                // `secret` names a reference; `client_secret` (OAuth) is a
                // single `{{variable}}` that must hold one.
                a.get("secret")
                    .and_then(Item::as_str)
                    .map(str::to_owned)
                    .into_iter()
                    .chain(
                        a.get("client_secret")
                            .and_then(Item::as_str)
                            .and_then(|v| v.strip_prefix("{{")?.strip_suffix("}}"))
                            .and_then(|name| match values.get(name.trim()) {
                                Some((Value::Secret(r), _)) => Some(r.clone()),
                                _ => None,
                            }),
                    )
                    .collect(),
            ),
            None => (None, Vec::new()),
        },
        None => (None, Vec::new()),
    };
    used.sort();
    used.dedup();
    let variables: Vec<VariableView> = values
        .iter()
        .map(|(name, (value, layer))| VariableView {
            name: name.clone(),
            layer: *layer,
            secret: match value {
                Value::Secret(r) => Some(r.clone()),
                Value::Plain(_) => None,
            },
            stored: match value {
                Value::Secret(r) => Some(stored(r)),
                Value::Plain(_) => None,
            },
        })
        .collect();
    let mut missing_secrets: Vec<String> = variables
        .iter()
        .filter(|v| used.contains(&v.name) && v.stored == Some(false))
        .filter_map(|v| v.secret.clone())
        .collect();
    if auth.as_deref() != Some("none") {
        for r in &auth_references {
            if document::valid_reference(r) && !stored(r) && !missing_secrets.contains(r) {
                missing_secrets.push(r.clone());
            }
        }
    }
    Preview {
        method: fields.method.clone(),
        url,
        headers,
        query,
        body_kind: fields.body_kind.clone(),
        body_bytes: u32::try_from(body.len()).unwrap_or(u32::MAX),
        body: body.chars().take(PREVIEW_BODY).collect(),
        variables,
        unresolved,
        missing_secrets,
        auth,
        auth_references,
        problem,
        json_warning: fields.body_kind == "json"
            && serde_json::from_str::<serde_json::Value>(&fields.body).is_err(),
    }
}

/// The secret references a document names (`[variables]` secrets and
/// `[auth] secret`).
pub fn references_in(doc: &DocumentMut, into: &mut std::collections::BTreeSet<String>) {
    if let Some(table) = doc.get("variables").and_then(Item::as_table_like) {
        for (_, v) in table.iter() {
            if let Some(r) = v.get("secret").and_then(Item::as_str)
                && document::valid_reference(r)
            {
                into.insert(r.to_owned());
            }
        }
    }
    if let Some(r) = doc
        .get("auth")
        .and_then(|a| a.get("secret"))
        .and_then(Item::as_str)
        && document::valid_reference(r)
    {
        into.insert(r.to_owned());
    }
}

/// The secret references texts name (for the Secrets list).
pub fn references(texts: &[&str]) -> Vec<String> {
    let mut found = std::collections::BTreeSet::new();
    for text in texts {
        if let Ok(doc) = text.parse::<DocumentMut>() {
            references_in(&doc, &mut found);
        }
    }
    found.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_apply_in_order_and_secrets_stay_masked() {
        let layers = Layers {
            collection: Some("format_version = 1\nid = \"c\"\n[variables]\nhost = \"col\"\nkeep = \"c\"\n".into()),
            local_defaults: Some("format_version = 1\n[variables]\nhost = \"defaults\"\n".into()),
            shared: Some("format_version = 1\n[variables]\nhost = \"shared\"\ntoken = { secret = \"tok\" }\n".into()),
            local: Some("format_version = 1\n[variables]\nhost = \"local\"\n".into()),
        };
        let fields = Fields {
            url: "https://{{host}}/{{keep}}/{{nope}}".into(),
            headers: "Authorization: Bearer {{token}}".into(),
            settings: "[variables]\nkeep = \"req\"\n".into(),
            ..Fields::default()
        };
        let p = preview(&fields, &layers, &|r| r == "other");
        assert_eq!(p.url, "https://local/req/{{nope}}");
        assert_eq!(
            p.headers,
            vec![("Authorization".into(), format!("Bearer {MASK}"))]
        );
        assert_eq!(p.unresolved, ["nope"]);
        assert_eq!(p.missing_secrets, ["tok"]);
        assert!(!format!("{p:?}").contains("secret-value"));
        let host = p
            .variables
            .iter()
            .find(|v| v.name == "host")
            .expect("fixture");
        assert_eq!(host.layer, Layer::Local);
    }

    #[test]
    fn environment_texts_hold_variables_only() {
        assert!(text_ok(&new_text()).is_some());
        assert!(text_ok("format_version = 1\n[variables]\nk = { secret = \"r\" }\n").is_some());
        assert!(text_ok("format_version = 2\n").is_none());
        assert!(text_ok("format_version = 1\n[auth]\n").is_none());
        assert!(valid_name("dev-1") && !valid_name("a b") && !valid_name(""));
    }
}
