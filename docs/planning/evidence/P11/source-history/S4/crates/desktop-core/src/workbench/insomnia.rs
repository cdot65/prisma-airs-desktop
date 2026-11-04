//! Insomnia import (C14, v1's rules): only `collection.insomnia.rest/5.0`
//! exports with `schema_version: 5.1`, YAML or JSON, at most 1 MiB and
//! bounded once expanded (depth 64, 50,000 nodes, 16 MiB of text). The
//! export is data: scripts, template expressions, plugins, path parameters
//! and non-default request settings refuse that request; credentials become
//! secret references (no value is stored). The operator selects what to
//! create; requests become unsaved drafts and environments new shared
//! environment files (never replacing a file).

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::rc::Rc;

use serde::Serialize;
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use sha2::{Digest, Sha256};
use toml_edit::{DocumentMut, InlineTable, Item, Table, value};
use ts_rs::TS;

use super::curl::credential_name;
use super::document::{self, Fields};

pub const EXPORT_BYTES: usize = 1024 * 1024;
const MAX_NODES: usize = 50_000;
const MAX_DEPTH: usize = 64;
const EXPANDED_BYTES: usize = 16 * 1024 * 1024;
const MAX_RESOURCES: usize = 1_000;
const MAX_FOLDER_DEPTH: usize = 24;
const MAX_ENVIRONMENTS: usize = 128;
const ENVIRONMENT_BYTES: usize = 1024 * 1024;

/// Why a whole export was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ExportProblem {
    TooLarge,
    /// Not YAML/JSON within the limits (tags, duplicate or complex keys).
    Unreadable,
    /// Another format or schema version.
    Version,
    /// Unknown top-level fields or a malformed collection.
    Structure,
    /// More resources or folder nesting than the import takes.
    Limit,
}

/// Why one resource cannot be converted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ResourceProblem {
    /// Unsupported fields, including scripts.
    Fields,
    PathParameters,
    /// Non-default request settings (redirects, cookies, rendering).
    Settings,
    Method,
    Url,
    /// Template expressions, plugins or unclosed variables.
    Template,
    Header,
    Parameter,
    /// A literal credential in a query, form or JSON field, or the URL.
    Credential,
    Body,
    Json,
    Auth,
    /// No-auth under folder authentication, or disabled authentication.
    AuthReview,
    /// Auth together with an Authorization header.
    AuthCombination,
    /// Inside a folder that was refused.
    Folder,
    /// The generated path is too long.
    Path,
    /// Nested, cyclic or oversized environment values.
    Environment,
}

/// One converted request or environment, or why not.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ResourceView {
    pub index: u32,
    pub label: String,
    pub relative: String,
    pub environment: bool,
    pub problem: Option<ResourceProblem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Export {
    pub source_sha256: String,
    pub collection: String,
    pub requests: Vec<(String, String, Result<Fields, ResourceProblem>)>,
    pub environments: Vec<(String, String, Result<String, ResourceProblem>)>,
}

impl Export {
    pub fn view(&self) -> Vec<ResourceView> {
        let requests = self
            .requests
            .iter()
            .map(|(l, r, f)| (l, r, false, f.as_ref().err()));
        let envs = self
            .environments
            .iter()
            .map(|(l, r, t)| (l, r, true, t.as_ref().err()));
        requests
            .chain(envs)
            .enumerate()
            .map(
                |(i, (label, relative, environment, problem))| ResourceView {
                    index: u32::try_from(i).unwrap_or(u32::MAX),
                    label: label.clone(),
                    relative: relative.clone(),
                    environment,
                    problem: problem.copied(),
                },
            )
            .collect()
    }
}

// ------------------------------------------------------------ bounded YAML

#[derive(Clone)]
struct Seed {
    nodes: Rc<Cell<usize>>,
    bytes: Rc<Cell<usize>>,
    depth: usize,
}

impl Seed {
    fn child(&self) -> Result<Self, &'static str> {
        if self.depth >= MAX_DEPTH || self.nodes.get() >= MAX_NODES {
            return Err("limit");
        }
        self.nodes.set(self.nodes.get() + 1);
        Ok(Self {
            depth: self.depth + 1,
            ..self.clone()
        })
    }
    fn string<E: de::Error>(&self, text: &str) -> Result<(), E> {
        let size = self.bytes.get().saturating_add(text.len());
        if size > EXPANDED_BYTES {
            return Err(E::custom("limit"));
        }
        self.bytes.set(size);
        Ok(())
    }
}

impl<'de> DeserializeSeed<'de> for Seed {
    type Value = Value;
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Seed {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("bounded JSON-compatible YAML")
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Number(v.into()))
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Value, E> {
        Ok(Value::Number(v.into()))
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Value, E> {
        Number::from_f64(v)
            .map(Value::Number)
            .ok_or_else(|| E::custom("number"))
    }
    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_none<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Value, E> {
        self.string(v)?;
        Ok(Value::String(v.into()))
    }
    fn visit_string<E: de::Error>(self, v: String) -> Result<Value, E> {
        self.string(&v)?;
        Ok(Value::String(v))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Value, A::Error> {
        let mut values = Vec::new();
        loop {
            let seed = self.child().map_err(de::Error::custom)?;
            match a.next_element_seed(seed)? {
                Some(v) => values.push(v),
                None => break,
            }
        }
        Ok(Value::Array(values))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Value, A::Error> {
        let mut values = Map::new();
        loop {
            let seed = self.child().map_err(de::Error::custom)?;
            let Some(key) = a.next_key_seed(seed)? else {
                break;
            };
            let Value::String(key) = key else {
                return Err(de::Error::custom("key"));
            };
            if values.contains_key(&key) {
                return Err(de::Error::custom("duplicate"));
            }
            let child = self.child().map_err(de::Error::custom)?;
            values.insert(key, a.next_value_seed(child)?);
        }
        Ok(Value::Object(values))
    }
}

/// Parses within the limits. Parser messages may quote source values, so
/// they are never surfaced.
fn bounded(text: &str) -> Result<Value, ExportProblem> {
    if text.len() > EXPORT_BYTES {
        return Err(ExportProblem::TooLarge);
    }
    let mut documents = serde_yaml_ng::Deserializer::from_str(text);
    let d = documents.next().ok_or(ExportProblem::Unreadable)?;
    let seed = Seed {
        nodes: Rc::new(Cell::new(0)),
        bytes: Rc::new(Cell::new(0)),
        depth: 0,
    };
    let value = seed.deserialize(d).map_err(|_| ExportProblem::Unreadable)?;
    if documents.next().is_some() {
        return Err(ExportProblem::Unreadable);
    }
    Ok(value)
}

// ------------------------------------------------------------- conversion

fn label(v: &Value, fallback: &str) -> String {
    v.get("name")
        .and_then(Value::as_str)
        .unwrap_or(fallback)
        .chars()
        .filter(|c| !c.is_control())
        .take(160)
        .collect()
}

/// A bounded slug; URL-shaped names keep only the host (v1).
fn slug(label: &str) -> String {
    let safe = if label.starts_with("http://") || label.starts_with("https://") {
        url::Url::parse(label)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .unwrap_or_else(|| "request".into())
    } else {
        label.into()
    };
    let mut slug = String::new();
    for byte in safe.bytes() {
        let byte = byte.to_ascii_lowercase();
        if byte.is_ascii_alphanumeric() {
            slug.push(byte as char);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
        if slug.len() >= 40 {
            break;
        }
    }
    slug.truncate(slug.trim_end_matches('-').len());
    if slug.is_empty() {
        "resource".into()
    } else {
        slug
    }
}

fn reference(digest: &str, scope: &str) -> String {
    let d = hex::encode(Sha256::digest(format!("{digest}\0{scope}").as_bytes()));
    format!("import-{}", &d[..32])
}

/// `{{ _.name }}` becomes `{{name}}`; any other expression is refused.
fn translate(text: &str) -> Result<String, ResourceProblem> {
    if text.contains("{%") {
        return Err(ResourceProblem::Template);
    }
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let tail = &rest[start + 2..];
        let end = tail.find("}}").ok_or(ResourceProblem::Template)?;
        let raw = tail[..end].trim();
        let name = raw.strip_prefix("_.").unwrap_or(raw);
        if !document::valid_variable(name) {
            return Err(ResourceProblem::Template);
        }
        out.push_str(&format!("{{{{{name}}}}}"));
        rest = &tail[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

fn keys(v: &Value, allowed: &[&str]) -> bool {
    v.as_object()
        .is_some_and(|m| m.keys().all(|k| allowed.contains(&k.as_str())))
}

fn string<'a>(v: &'a Value, k: &str, problem: ResourceProblem) -> Result<&'a str, ResourceProblem> {
    v.get(k).and_then(Value::as_str).ok_or(problem)
}

fn rows<'a>(v: &'a Value, k: &str) -> Result<&'a [Value], ResourceProblem> {
    match v.get(k) {
        None => Ok(&[]),
        Some(v) => v
            .as_array()
            .map(Vec::as_slice)
            .ok_or(ResourceProblem::Fields),
    }
}

fn disabled(v: &Value) -> Result<bool, ResourceProblem> {
    match v.get("disabled") {
        None => Ok(false),
        Some(v) => v.as_bool().ok_or(ResourceProblem::Fields),
    }
}

/// A value that is only a variable (optionally after Bearer/Basic).
fn managed_credential(text: &str) -> bool {
    let text = text
        .strip_prefix("Bearer ")
        .or_else(|| text.strip_prefix("Basic "))
        .unwrap_or(text);
    text.strip_prefix("{{")
        .and_then(|v| v.strip_suffix("}}"))
        .is_some_and(document::valid_variable)
}

fn literal_secrets(v: &Value) -> bool {
    match v {
        Value::Object(m) => m.iter().any(|(k, v)| {
            (credential_name(k) && !v.as_str().is_some_and(managed_credential))
                || literal_secrets(v)
        }),
        Value::Array(a) => a.iter().any(literal_secrets),
        _ => false,
    }
}

const ROW_KEYS: [&str; 5] = ["id", "name", "value", "disabled", "description"];

fn parameter_rows(v: &Value, k: &str) -> Result<String, ResourceProblem> {
    let mut out = Vec::new();
    for row in rows(v, k)? {
        if !keys(row, &ROW_KEYS) {
            return Err(ResourceProblem::Parameter);
        }
        let name = string(row, "name", ResourceProblem::Parameter)?;
        let text = translate(string(row, "value", ResourceProblem::Parameter)?)?;
        if name.is_empty()
            || name.contains(['\r', '\n', '='])
            || name.starts_with("# ")
            || text.contains(['\r', '\n'])
        {
            return Err(ResourceProblem::Parameter);
        }
        if credential_name(name) && !managed_credential(&text) {
            return Err(ResourceProblem::Credential);
        }
        out.push(format!(
            "{}{name}={text}",
            if disabled(row)? { "# " } else { "" }
        ));
    }
    Ok(out.join("\n"))
}

fn headers_have(text: &str, name: &str) -> bool {
    text.lines().any(|s| {
        s.trim_start_matches("# ")
            .split_once(':')
            .is_some_and(|(n, _)| n.eq_ignore_ascii_case(name))
    })
}

fn validate_url(text: &str) -> Result<(), ResourceProblem> {
    if text.chars().any(char::is_control) {
        return Err(ResourceProblem::Url);
    }
    // Variables become placeholders: a leading variable stands for an origin.
    let mut surrogate = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        surrogate.push_str(&rest[..start]);
        let end = rest[start..].find("}}").ok_or(ResourceProblem::Template)? + start;
        surrogate.push_str(if start == 0 && surrogate.is_empty() {
            "https://example.invalid"
        } else {
            "placeholder"
        });
        rest = &rest[end + 2..];
    }
    surrogate.push_str(rest);
    let url = url::Url::parse(&surrogate).map_err(|_| ResourceProblem::Url)?;
    if !["http", "https"].contains(&url.scheme()) || url.host_str().is_none() {
        return Err(ResourceProblem::Url);
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query_pairs().any(|(k, _)| credential_name(&k))
    {
        return Err(ResourceProblem::Credential);
    }
    Ok(())
}

fn request_settings(v: Option<&Value>) -> Result<(), ResourceProblem> {
    let Some(v) = v else { return Ok(()) };
    let object = v.as_object().ok_or(ResourceProblem::Settings)?;
    for (k, v) in object {
        let ok = match k.as_str() {
            "encodeUrl" | "rebuildPath" | "renderRequestBody" => v.as_bool() == Some(true),
            "followRedirects" => v.as_str() == Some("global"),
            "cookies" => v.as_object().is_some_and(|m| {
                m.len() == 2
                    && m.get("send").and_then(Value::as_bool) == Some(true)
                    && m.get("store").and_then(Value::as_bool) == Some(true)
            }),
            _ => false,
        };
        if !ok {
            return Err(ResourceProblem::Settings);
        }
    }
    Ok(())
}

fn body(source: &Value, fields: &mut Fields) -> Result<(), ResourceProblem> {
    let Some(body) = source.get("body") else {
        return Ok(());
    };
    if !keys(body, &["mimeType", "text", "params"]) {
        return Err(ResourceProblem::Body);
    }
    let mime = string(body, "mimeType", ResourceProblem::Body)?;
    match mime {
        "application/x-www-form-urlencoded" => {
            if body.get("text").is_some() {
                return Err(ResourceProblem::Body);
            }
            fields.body_kind = "form".into();
            fields.body = parameter_rows(body, "params")?;
        }
        "application/json" | "text/plain" | "application/xml" | "text/xml" => {
            if body.get("params").is_some() {
                return Err(ResourceProblem::Body);
            }
            let text = translate(body.get("text").and_then(Value::as_str).unwrap_or(""))?;
            if mime == "application/json" {
                let json: Value = serde_json::from_str(&text).map_err(|_| ResourceProblem::Json)?;
                if literal_secrets(&json) {
                    return Err(ResourceProblem::Credential);
                }
                fields.body_kind = "json".into();
            }
            fields.body = text;
        }
        _ => return Err(ResourceProblem::Body),
    }
    if !headers_have(&fields.headers, "content-type") {
        if !fields.headers.is_empty() {
            fields.headers.push('\n');
        }
        fields.headers.push_str(&format!("Content-Type: {mime}"));
    }
    if fields.body.len() > document::BODY_BYTES {
        return Err(ResourceProblem::Body);
    }
    Ok(())
}

fn auth(
    source: &Value,
    inherited: Option<(&Value, &str)>,
    own: &str,
    headers: &str,
    doc: &mut DocumentMut,
) -> Result<(), ResourceProblem> {
    let Some(auth) = source.get("authentication").or(inherited.map(|(v, _)| v)) else {
        return Ok(());
    };
    let kind = string(auth, "type", ResourceProblem::Auth)?;
    if kind == "none" {
        return if inherited.is_some() {
            Err(ResourceProblem::AuthReview)
        } else {
            Ok(())
        };
    }
    if headers_have(headers, "authorization") {
        return Err(ResourceProblem::AuthCombination);
    }
    let reference = if source.get("authentication").is_none() {
        inherited.map(|(_, r)| r).unwrap_or(own)
    } else {
        own
    };
    let mut table = Table::new();
    match kind {
        "basic" => {
            if !keys(auth, &["type", "username", "password", "disabled"]) {
                return Err(ResourceProblem::Auth);
            }
            table["kind"] = value("basic");
            table["username"] = value(translate(string(auth, "username", ResourceProblem::Auth)?)?);
        }
        "bearer" => {
            if !keys(auth, &["type", "token", "prefix", "disabled"])
                || auth
                    .get("prefix")
                    .is_some_and(|v| v.as_str() != Some("Bearer"))
            {
                return Err(ResourceProblem::Auth);
            }
            table["kind"] = value("bearer");
        }
        "oauth2" => {
            if !keys(
                auth,
                &[
                    "type",
                    "grantType",
                    "accessTokenUrl",
                    "clientId",
                    "clientSecret",
                    "credentialsInBody",
                    "scope",
                ],
            ) || string(auth, "grantType", ResourceProblem::Auth)? != "client_credentials"
                || auth
                    .get("credentialsInBody")
                    .is_some_and(|v| v.as_bool() == Some(true))
            {
                return Err(ResourceProblem::Auth);
            }
            table["kind"] = value("oauth2");
            let token_url = match auth.get("accessTokenUrl").and_then(Value::as_str) {
                Some(u) if !u.trim().is_empty() => translate(u)?,
                _ => "{{token_url}}".into(),
            };
            table["token_url"] = value(token_url);
            let client_id = match auth.get("clientId").and_then(Value::as_str) {
                Some(id) if !id.trim().is_empty() => translate(id)?,
                _ => "{{client_id}}".into(),
            };
            table["client_id"] = value(client_id);
            if let Some(scope) = auth
                .get("scope")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                table["scope"] = value(translate(scope)?);
            }
        }
        "apikey" => {
            if !keys(auth, &["type", "key", "value", "addTo", "disabled"]) {
                return Err(ResourceProblem::Auth);
            }
            let name = string(auth, "key", ResourceProblem::Auth)?;
            let location = string(auth, "addTo", ResourceProblem::Auth)?;
            if !["header", "query"].contains(&location)
                || name.is_empty()
                || name.contains(['\r', '\n'])
                || (location == "header" && http::HeaderName::from_bytes(name.as_bytes()).is_err())
            {
                return Err(ResourceProblem::Auth);
            }
            table["kind"] = value("api-key");
            table["name"] = value(name);
            table["location"] = value(location);
        }
        _ => return Err(ResourceProblem::Auth),
    }
    if auth
        .get("disabled")
        .is_some_and(|v| v.as_bool() == Some(true))
    {
        return Err(ResourceProblem::AuthReview);
    }
    table["secret"] = value(reference);
    doc["auth"] = Item::Table(table);
    Ok(())
}

const REQUEST_KEYS: [&str; 10] = [
    "name",
    "meta",
    "url",
    "method",
    "headers",
    "parameters",
    "body",
    "authentication",
    "settings",
    "pathParameters",
];

fn request(
    source: &Value,
    inherited: Option<(&Value, &str)>,
    own: &str,
) -> Result<Fields, ResourceProblem> {
    if !keys(source, &REQUEST_KEYS) {
        return Err(ResourceProblem::Fields);
    }
    if source
        .get("pathParameters")
        .is_some_and(|v| !v.as_array().is_some_and(Vec::is_empty))
    {
        return Err(ResourceProblem::PathParameters);
    }
    request_settings(source.get("settings"))?;
    let method = string(source, "method", ResourceProblem::Method)?;
    if method.is_empty()
        || method.len() > document::METHOD_CHARS
        || !method.bytes().all(|b| b.is_ascii_alphabetic() || b == b'-')
    {
        return Err(ResourceProblem::Method);
    }
    let url = translate(string(source, "url", ResourceProblem::Url)?)?;
    validate_url(&url)?;
    let mut fields = Fields {
        method: method.into(),
        url,
        ..Fields::default()
    };
    let mut doc = DocumentMut::new();
    let mut headers = Vec::new();
    for row in rows(source, "headers")? {
        if !keys(row, &ROW_KEYS) {
            return Err(ResourceProblem::Header);
        }
        let name = string(row, "name", ResourceProblem::Header)?;
        let text = translate(string(row, "value", ResourceProblem::Header)?)?;
        if http::HeaderName::from_bytes(name.as_bytes()).is_err() || text.contains(['\r', '\n']) {
            return Err(ResourceProblem::Header);
        }
        let text = if credential_name(name) && !managed_credential(&text) {
            if doc.get("variables").is_none() {
                doc["variables"] = Item::Table(Table::new());
            }
            let variable = format!("import_header_{}", headers.len());
            let mut entry = InlineTable::new();
            entry.insert(
                "secret",
                reference(own, &format!("header-{}", headers.len())).into(),
            );
            doc["variables"][&variable] = value(entry);
            format!("{{{{{variable}}}}}")
        } else {
            text
        };
        headers.push(format!(
            "{}{name}: {text}",
            if disabled(row)? { "# " } else { "" }
        ));
    }
    fields.headers = headers.join("\n");
    fields.query = parameter_rows(source, "parameters")?;
    body(source, &mut fields)?;
    auth(source, inherited, own, &fields.headers, &mut doc)?;
    fields.settings = if doc.is_empty() {
        String::new()
    } else {
        doc.to_string()
    };
    document::check(&fields).map_err(|_| ResourceProblem::Fields)?;
    Ok(fields)
}

fn walk(
    items: &[Value],
    parent: &str,
    depth: usize,
    export: &mut Export,
    folders: &mut usize,
    inherited: Option<(&Value, &str)>,
    refused: bool,
) -> Result<(), ExportProblem> {
    if depth > MAX_FOLDER_DEPTH || export.requests.len() + *folders + items.len() > MAX_RESOURCES {
        return Err(ExportProblem::Limit);
    }
    for item in items {
        if let Some(children) = item.get("children") {
            *folders += 1;
            let relative = format!(
                "{parent}/folder-{:04}-{}",
                *folders,
                slug(&label(item, "folder"))
            );
            let auth_type = item
                .get("authentication")
                .and_then(|v| v.get("type"))
                .and_then(Value::as_str);
            let refuse = refused
                || (inherited.is_some() && auth_type == Some("none"))
                || !keys(item, &["name", "meta", "children", "authentication"])
                || !children.is_array();
            let own = reference(&export.source_sha256, &format!("folder-auth/{relative}"));
            let auth = item
                .get("authentication")
                .filter(|_| auth_type != Some("none"))
                .map(|v| (v, own.as_str()))
                .or(inherited);
            if let Some(children) = children.as_array() {
                walk(
                    children,
                    &relative,
                    depth + 1,
                    export,
                    folders,
                    auth,
                    refuse,
                )?;
            }
        } else {
            let relative = format!(
                "{parent}/request-{:04}-{}{}",
                export.requests.len() + 1,
                slug(&label(item, "request")),
                super::project::REQUEST_SUFFIX
            );
            let own = reference(&export.source_sha256, &format!("request-auth/{relative}"));
            let result = if relative.len() > super::project::MAX_PATH {
                Err(ResourceProblem::Path)
            } else if refused {
                Err(ResourceProblem::Folder)
            } else {
                request(item, inherited, &own)
            };
            export
                .requests
                .push((label(item, "Request"), relative, result));
        }
    }
    Ok(())
}

fn scalar(v: &Value) -> Result<String, ResourceProblem> {
    match v {
        Value::String(s) => Ok(s.clone()),
        Value::Bool(_) | Value::Number(_) => Ok(v.to_string()),
        Value::Null => Ok(String::new()),
        _ => Err(ResourceProblem::Environment),
    }
}

fn environment_data(v: &Value) -> Result<BTreeMap<String, Value>, ResourceProblem> {
    if !keys(v, &["name", "meta", "data", "subEnvironments"]) {
        return Err(ResourceProblem::Fields);
    }
    match v.get("data") {
        None => Ok(BTreeMap::new()),
        Some(d) => Ok(d
            .as_object()
            .ok_or(ResourceProblem::Environment)?
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()),
    }
}

/// Expands `{{name}}` references between environment values (32 deep, no
/// cycles, 1 MiB), refusing values derived from credentials.
fn expand(
    name: &str,
    values: &BTreeMap<String, Value>,
    visiting: &mut BTreeSet<String>,
    memo: &mut BTreeMap<String, String>,
) -> Result<String, ResourceProblem> {
    if let Some(text) = memo.get(name) {
        return Ok(text.clone());
    }
    if visiting.len() >= 32 || !visiting.insert(name.into()) {
        return Err(ResourceProblem::Environment);
    }
    let raw = scalar(values.get(name).ok_or(ResourceProblem::Environment)?)?;
    let text = translate(&raw)?;
    let mut out = String::new();
    let mut rest = text.as_str();
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let tail = &rest[start + 2..];
        let end = tail.find("}}").ok_or(ResourceProblem::Template)?;
        let dependency = &tail[..end];
        if credential_name(dependency) {
            return Err(ResourceProblem::Credential);
        }
        out.push_str(&expand(dependency, values, visiting, memo)?);
        if out.len() > ENVIRONMENT_BYTES {
            return Err(ResourceProblem::Environment);
        }
        rest = &tail[end + 2..];
    }
    out.push_str(rest);
    visiting.remove(name);
    memo.insert(name.into(), out.clone());
    Ok(out)
}

fn render(
    values: &BTreeMap<String, Value>,
    digest: &str,
    scope: &str,
) -> Result<String, ResourceProblem> {
    let mut doc = DocumentMut::new();
    doc["format_version"] = value(1);
    doc["variables"] = Item::Table(Table::new());
    let mut visiting = BTreeSet::new();
    let mut memo = BTreeMap::new();
    for key in values.keys() {
        if !document::valid_variable(key) {
            return Err(ResourceProblem::Environment);
        }
        if credential_name(key) {
            let mut entry = InlineTable::new();
            entry.insert(
                "secret",
                reference(digest, &format!("environment/{scope}/{key}")).into(),
            );
            doc["variables"][key.as_str()] = value(entry);
        } else {
            let text = expand(key, values, &mut visiting, &mut memo)?;
            doc["variables"][key.as_str()] = value(text);
        }
    }
    let text = doc.to_string();
    if text.len() > ENVIRONMENT_BYTES {
        return Err(ResourceProblem::Environment);
    }
    Ok(text)
}

fn environments(source: Option<&Value>, export: &mut Export) -> Result<(), ExportProblem> {
    let Some(source) = source else { return Ok(()) };
    let add = |source: &Value,
               values: Result<BTreeMap<String, Value>, ResourceProblem>,
               scope: &str,
               export: &mut Export| {
        let relative = format!(
            "environments/insomnia-env-{}-{}.toml",
            export.environments.len() + 1,
            slug(&label(source, "environment"))
        );
        let digest = export.source_sha256.clone();
        let result = values.and_then(|v| render(&v, &digest, scope));
        export
            .environments
            .push((label(source, "Environment"), relative, result));
    };
    let base = environment_data(source);
    add(source, base.clone(), "base", export);
    if let Some(children) = source.get("subEnvironments") {
        let children = children.as_array().ok_or(ExportProblem::Structure)?;
        if children.len() > MAX_ENVIRONMENTS {
            return Err(ExportProblem::Limit);
        }
        for (index, child) in children.iter().enumerate() {
            let nested = child
                .get("subEnvironments")
                .is_some_and(|v| !v.as_array().is_some_and(Vec::is_empty));
            let values = if nested {
                Err(ResourceProblem::Environment)
            } else {
                base.clone().and_then(|mut values| {
                    values.extend(environment_data(child)?);
                    Ok(values)
                })
            };
            add(child, values, &format!("sub-{index}"), export);
        }
    }
    Ok(())
}

/// Reads an export (data only; nothing is created here).
pub fn parse(text: &str) -> Result<Export, ExportProblem> {
    let value = bounded(text)?;
    if value.get("type").and_then(Value::as_str) != Some("collection.insomnia.rest/5.0")
        || value.get("schema_version").and_then(Value::as_str) != Some("5.1")
    {
        return Err(ExportProblem::Version);
    }
    if !keys(
        &value,
        &[
            "type",
            "schema_version",
            "name",
            "meta",
            "collection",
            "cookieJar",
            "environments",
        ],
    ) {
        return Err(ExportProblem::Structure);
    }
    let mut export = Export {
        source_sha256: hex::encode(Sha256::digest(text.as_bytes())),
        collection: label(&value, "Collection"),
        requests: Vec::new(),
        environments: Vec::new(),
    };
    let items = value
        .get("collection")
        .and_then(Value::as_array)
        .ok_or(ExportProblem::Structure)?;
    let mut folders = 0;
    walk(items, "insomnia", 0, &mut export, &mut folders, None, false)?;
    environments(value.get("environments"), &mut export)?;
    Ok(export)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPORT: &str = r#"type: collection.insomnia.rest/5.0
schema_version: "5.1"
name: Sample
collection:
  - name: Items
    authentication:
      type: bearer
      token: "{{ _.token }}"
    children:
      - name: List items
        method: GET
        url: "{{ _.base }}/items"
        headers:
          - name: X-Api-Key
            value: literal-secret-value
        parameters:
          - name: limit
            value: "10"
      - name: Script
        method: GET
        url: https://a.example/
        scripts:
          preRequest: "alert(1)"
  - name: Create
    method: POST
    url: https://a.example/items
    body:
      mimeType: application/json
      text: '{"name": "{{ _.name }}"}'
environments:
  name: Base
  data:
    base: https://a.example
    token: also-secret
  subEnvironments:
    - name: Prod
      data:
        base: https://prod.example
"#;

    #[test]
    fn converts_supported_requests_and_refuses_scripts() {
        let export = parse(EXPORT).expect("fixture");
        assert_eq!(export.requests.len(), 3);
        let (_, relative, list) = &export.requests[0];
        assert_eq!(
            relative,
            "insomnia/folder-0001-items/request-0001-list-items.request.toml"
        );
        let list = list.as_ref().expect("fixture");
        assert_eq!(list.url, "{{base}}/items");
        assert!(list.headers.contains("X-Api-Key: {{import_header_0}}"));
        assert!(list.settings.contains("kind = \"bearer\""));
        assert_eq!(list.query, "limit=10");
        assert_eq!(export.requests[1].2, Err(ResourceProblem::Fields));
        assert_eq!(
            export.requests[2].2.as_ref().expect("fixture").body_kind,
            "json"
        );
        let text = format!("{export:?}");
        assert!(!text.contains("literal-secret-value") && !text.contains("also-secret"));
        let prod = export.environments[1].2.as_ref().expect("fixture");
        assert!(prod.contains("base = \"https://prod.example\""));
        assert!(prod.contains("token = { secret = \"import-"));
    }

    #[test]
    fn refuses_other_formats_and_oversized_structures() {
        assert_eq!(
            parse("type: other\n").map(|_| ()),
            Err(ExportProblem::Version)
        );
        assert_eq!(parse("a: [").map(|_| ()), Err(ExportProblem::Unreadable));
        assert_eq!(
            parse("a: 1\na: 2\n").map(|_| ()),
            Err(ExportProblem::Unreadable)
        );
        let bomb = "a: &a [x,x,x,x,x,x,x,x,x,x]\nb: &b [*a,*a,*a,*a,*a,*a,*a,*a,*a,*a]\nc: &c [*b,*b,*b,*b,*b,*b,*b,*b,*b,*b]\nd: &d [*c,*c,*c,*c,*c,*c,*c,*c,*c,*c]\ne: [*d,*d,*d,*d,*d,*d,*d,*d,*d,*d]\n";
        assert_eq!(parse(bomb).map(|_| ()), Err(ExportProblem::Unreadable));
        assert_eq!(
            parse(&" ".repeat(EXPORT_BYTES + 1)).map(|_| ()),
            Err(ExportProblem::TooLarge)
        );
        // Depth and node limits, each just inside and just past its bound,
        // in otherwise plain YAML the parser itself accepts.
        let nested = |depth: usize| format!("a: {}{}\n", "[".repeat(depth), "]".repeat(depth));
        assert_eq!(
            parse(&nested(MAX_DEPTH - 2)).map(|_| ()),
            Err(ExportProblem::Version)
        );
        assert_eq!(
            parse(&nested(MAX_DEPTH + 4)).map(|_| ()),
            Err(ExportProblem::Unreadable)
        );
        let wide = |nodes: usize| format!("a: [{}]\n", vec!["1"; nodes].join(","));
        assert_eq!(
            parse(&wide(MAX_NODES - 10)).map(|_| ()),
            Err(ExportProblem::Version)
        );
        assert_eq!(
            parse(&wide(MAX_NODES + 10)).map(|_| ()),
            Err(ExportProblem::Unreadable)
        );
        let extra = "type: collection.insomnia.rest/5.0\nschema_version: \"5.1\"\ncollection: []\nplugins: []\n";
        assert_eq!(parse(extra).map(|_| ()), Err(ExportProblem::Structure));
    }

    #[test]
    fn templates_other_than_simple_variables_are_refused() {
        assert_eq!(
            translate("{{ _.a }}/{{b}}").expect("fixture"),
            "{{a}}/{{b}}"
        );
        assert_eq!(translate("{% now %}"), Err(ResourceProblem::Template));
        assert_eq!(translate("{{ a | upper }}"), Err(ResourceProblem::Template));
        assert_eq!(translate("{{ a"), Err(ResourceProblem::Template));
    }
}
