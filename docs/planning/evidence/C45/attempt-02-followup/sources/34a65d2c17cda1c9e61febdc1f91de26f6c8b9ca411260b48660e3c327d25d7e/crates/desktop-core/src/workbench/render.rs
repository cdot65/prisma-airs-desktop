//! A request rendered for Send (C15). v1's rules: the variable layers of
//! `environments`, `{{name}}` substituted one level deep (`{{{` escapes a
//! brace), unresolved or unclosed names and values that themselves hold
//! `{{` refused; v1's auth kinds and body kinds. Secrets are never read
//! here: every place a secret goes is a slot naming its reference, so
//! admission can classify where each credential would be sent before any
//! value is obtained (ADR 0004).

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use toml_edit::{DocumentMut, Item};
use ts_rs::TS;
use zeroize::Zeroizing;

use super::document::{self, FieldProblem, Fields};
use super::environments::{self, Layer, Layers, Value};

/// One piece of a rendered string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece {
    Text(String),
    /// The value of this secret reference goes here.
    Secret(String),
}

/// A rendered string whose secrets are still slots.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Template(pub Vec<Piece>);

impl Template {
    pub fn text(text: &str) -> Self {
        let mut t = Self::default();
        t.push_text(text);
        t
    }

    fn push_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(Piece::Text(last)) = self.0.last_mut() {
            last.push_str(text);
        } else {
            self.0.push(Piece::Text(text.to_owned()));
        }
    }

    /// The text, when no secret goes into it.
    pub fn plain(&self) -> Option<String> {
        let mut out = String::new();
        for piece in &self.0 {
            match piece {
                Piece::Text(t) => out.push_str(t),
                Piece::Secret(_) => return None,
            }
        }
        Some(out)
    }

    pub fn secrets(&self) -> impl Iterator<Item = &str> {
        self.0.iter().filter_map(|p| match p {
            Piece::Secret(r) => Some(r.as_str()),
            Piece::Text(_) => None,
        })
    }

    /// The text with each secret masked (never its value).
    pub fn masked(&self) -> String {
        self.with(|_| environments::MASK.to_owned())
    }

    /// The text with each secret named by its reference: what a review is
    /// bound to.
    pub fn bound(&self) -> String {
        self.with(|r| format!("{{{{secret:{r}}}}}"))
    }

    fn with(&self, secret: impl Fn(&str) -> String) -> String {
        self.0
            .iter()
            .map(|p| match p {
                Piece::Text(t) => t.clone(),
                Piece::Secret(r) => secret(r),
            })
            .collect()
    }

    /// The text with each secret's value.
    pub fn resolve(&self, secrets: &Secrets) -> Option<Zeroizing<String>> {
        let mut out = Zeroizing::new(String::new());
        for piece in &self.0 {
            match piece {
                Piece::Text(t) => out.push_str(t),
                Piece::Secret(r) => out.push_str(secrets.get(r)?),
            }
        }
        Some(out)
    }
}

/// Resolved secret values by reference (zeroized when dropped).
pub type Secrets = BTreeMap<String, Zeroizing<String>>;

/// Why a request cannot be rendered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RenderProblem {
    /// The snapshot itself is not valid (as Save reports it).
    Field { problem: FieldProblem },
    /// `{{name}}` names no variable.
    Unresolved { name: String },
    /// A `{{` is never closed.
    Unclosed,
    /// A variable's value itself holds `{{` (no nested templates).
    Nested { name: String },
    /// A secret would go into a header, parameter or part name.
    SecretInName,
    /// A header or part name is not a valid name after rendering.
    Name,
    /// The URL is not an absolute http(s) URL after rendering.
    Url,
    /// The auth settings are incomplete or contradictory.
    Auth,
    /// A multipart body sets its own Content-Type.
    MultipartContentType,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Auth {
    None,
    Basic {
        username: Template,
        secret: String,
    },
    Bearer {
        secret: String,
    },
    ApiKey {
        name: String,
        query: bool,
        secret: String,
    },
    OAuth2 {
        token_url: Template,
        client_id: Template,
        scope: Template,
        /// The client secret's reference.
        secret: String,
    },
}

impl Auth {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Basic { .. } => "basic",
            Self::Bearer { .. } => "bearer",
            Self::ApiKey { .. } => "api-key",
            Self::OAuth2 { .. } => "oauth2",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PartValue {
    Text(Template),
    /// A collection-relative file (read when sent).
    File {
        relative: String,
        mime: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    pub name: String,
    pub value: PartValue,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Body {
    None,
    /// Text or JSON (templated).
    Text {
        text: Template,
        json: bool,
    },
    /// `name=value` rows, form-urlencoded when sent.
    Form(Vec<(Template, Template)>),
    Multipart(Vec<Part>),
    /// A collection-relative file sent as stored (never templated).
    File {
        relative: String,
    },
}

/// The request as it will be prepared, secrets still slots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub method: String,
    pub url: Template,
    /// Enabled parameters, appended to the URL's query when sent.
    pub query: Vec<(Template, Template)>,
    /// Enabled headers, in order (auth and content type not yet added).
    pub headers: Vec<(String, Template)>,
    pub auth: Auth,
    pub body: Body,
    /// The variables used, with the layer each came from.
    pub used: BTreeMap<String, Layer>,
}

impl Plan {
    /// Every secret reference the request would use, and where.
    pub fn references(&self) -> BTreeSet<String> {
        let mut found: BTreeSet<String> = BTreeSet::new();
        let mut add = |t: &Template| found.extend(t.secrets().map(str::to_owned));
        add(&self.url);
        for (n, v) in &self.query {
            add(n);
            add(v);
        }
        for (_, v) in &self.headers {
            add(v);
        }
        match &self.body {
            Body::Text { text, .. } => add(text),
            Body::Form(rows) => {
                for (n, v) in rows {
                    add(n);
                    add(v);
                }
            }
            Body::Multipart(parts) => {
                for p in parts {
                    if let PartValue::Text(t) = &p.value {
                        add(t);
                    }
                }
            }
            Body::None | Body::File { .. } => {}
        }
        match &self.auth {
            Auth::None => {}
            Auth::Basic { username, secret } => {
                add(username);
                found.insert(secret.clone());
            }
            Auth::Bearer { secret } | Auth::ApiKey { secret, .. } => {
                found.insert(secret.clone());
            }
            Auth::OAuth2 {
                token_url,
                client_id,
                scope,
                secret,
            } => {
                add(token_url);
                add(client_id);
                add(scope);
                found.insert(secret.clone());
            }
        }
        found
    }

    /// Whether an enabled header named `name` (any case) is present.
    pub fn has_header(&self, name: &str) -> bool {
        self.headers
            .iter()
            .any(|(n, _)| n.eq_ignore_ascii_case(name))
    }
}

struct Renderer<'a> {
    values: &'a BTreeMap<String, (Value, Layer)>,
    used: BTreeMap<String, Layer>,
}

impl Renderer<'_> {
    /// `{{name}}` substitution, one level, `{{{` escapes a brace (v1).
    fn template(&mut self, text: &str) -> Result<Template, RenderProblem> {
        let mut out = Template::default();
        let mut rest = text;
        while let Some(start) = rest.find("{{") {
            if rest[start + 2..].starts_with('{') {
                out.push_text(&rest[..start + 1]);
                rest = &rest[start + 1..];
                continue;
            }
            out.push_text(&rest[..start]);
            let Some(end) = rest[start + 2..].find("}}") else {
                return Err(RenderProblem::Unclosed);
            };
            let name = rest[start + 2..start + 2 + end].trim();
            match self.values.get(name) {
                Some((Value::Plain(v), layer)) => {
                    if v.contains("{{") {
                        return Err(RenderProblem::Nested {
                            name: name.to_owned(),
                        });
                    }
                    self.used.insert(name.to_owned(), *layer);
                    out.push_text(v);
                }
                Some((Value::Secret(r), layer)) => {
                    self.used.insert(name.to_owned(), *layer);
                    out.0.push(Piece::Secret(r.clone()));
                }
                None => {
                    return Err(RenderProblem::Unresolved {
                        name: name.to_owned(),
                    });
                }
            }
            rest = &rest[start + 2 + end + 2..];
        }
        out.push_text(rest);
        Ok(out)
    }

    /// A name (header, part): rendered, no secret, a valid token.
    fn name(&mut self, text: &str) -> Result<String, RenderProblem> {
        let name = self
            .template(text)?
            .plain()
            .ok_or(RenderProblem::SecretInName)?;
        if http::HeaderName::from_bytes(name.as_bytes()).is_err() {
            return Err(RenderProblem::Name);
        }
        Ok(name)
    }

    /// `client_secret = "{{variable}}"` must name a secret variable.
    fn secret_variable(&mut self, text: &str) -> Result<String, RenderProblem> {
        let name = text
            .trim()
            .strip_prefix("{{")
            .and_then(|v| v.strip_suffix("}}"))
            .map(str::trim)
            .ok_or(RenderProblem::Auth)?;
        match self.values.get(name) {
            Some((Value::Secret(r), layer)) => {
                self.used.insert(name.to_owned(), *layer);
                Ok(r.clone())
            }
            _ => Err(RenderProblem::Auth),
        }
    }
}

fn string(table: &Item, key: &str) -> Option<String> {
    table.get(key).and_then(Item::as_str).map(str::to_owned)
}

fn auth(settings: &str, r: &mut Renderer) -> Result<Auth, RenderProblem> {
    if settings.trim().is_empty() {
        return Ok(Auth::None);
    }
    let doc = document::settings_ok(settings).ok_or(RenderProblem::Auth)?;
    let Some(a) = doc.get("auth") else {
        return Ok(Auth::None);
    };
    let reference = |a: &Item| {
        string(a, "secret")
            .filter(|s| document::valid_reference(s))
            .ok_or(RenderProblem::Auth)
    };
    match a.get("kind").and_then(Item::as_str).unwrap_or("none") {
        "none" => Ok(Auth::None),
        "basic" => {
            let username = r.template(&string(a, "username").ok_or(RenderProblem::Auth)?)?;
            // v1: a username holding `:` cannot be sent as Basic.
            if username
                .plain()
                .is_some_and(|u| u.contains(':') || u.is_empty())
            {
                return Err(RenderProblem::Auth);
            }
            Ok(Auth::Basic {
                username,
                secret: reference(a)?,
            })
        }
        "bearer" => Ok(Auth::Bearer {
            secret: reference(a)?,
        }),
        "api-key" => {
            let name = string(a, "name").ok_or(RenderProblem::Auth)?;
            let name = r.name(&name)?;
            let query = match string(a, "location").as_deref() {
                Some("query") => true,
                Some("header") | None => false,
                Some(_) => return Err(RenderProblem::Auth),
            };
            Ok(Auth::ApiKey {
                name,
                query,
                secret: reference(a)?,
            })
        }
        "oauth2" => {
            let token_url = r.template(&string(a, "token_url").ok_or(RenderProblem::Auth)?)?;
            let client_id = r.template(&string(a, "client_id").ok_or(RenderProblem::Auth)?)?;
            let scope = r.template(&string(a, "scope").unwrap_or_default())?;
            let secret = match (string(a, "secret"), string(a, "client_secret")) {
                (Some(_), None) => reference(a)?,
                (None, Some(variable)) => r.secret_variable(&variable)?,
                _ => return Err(RenderProblem::Auth),
            };
            Ok(Auth::OAuth2 {
                token_url,
                client_id,
                scope,
                secret,
            })
        }
        _ => Err(RenderProblem::Auth),
    }
}

fn multipart(text: &str, r: &mut Renderer) -> Result<Vec<Part>, RenderProblem> {
    let body = || RenderProblem::Field {
        problem: FieldProblem {
            field: document::FieldName::Body,
            line: None,
        },
    };
    let doc: DocumentMut = text.parse().map_err(|_| body())?;
    let Some(parts) = doc.get("parts").and_then(Item::as_array_of_tables) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for row in parts {
        let name = row.get("name").and_then(Item::as_str).unwrap_or_default();
        let name = r
            .template(name)?
            .plain()
            .ok_or(RenderProblem::SecretInName)?;
        let value = match (
            row.get("value").and_then(Item::as_str),
            row.get("file").and_then(Item::as_str),
        ) {
            (Some(v), None) => PartValue::Text(r.template(v)?),
            (None, Some(f)) => PartValue::File {
                relative: f.to_owned(),
                mime: row.get("mime").and_then(Item::as_str).map(str::to_owned),
            },
            _ => return Err(body()),
        };
        out.push(Part { name, value });
    }
    Ok(out)
}

/// Renders `fields` under `layers`. `stored_body` names the document's body
/// file when its body cannot be held as editor text (sent as stored).
pub fn render(
    fields: &Fields,
    layers: &Layers,
    stored_body: Option<&str>,
) -> Result<Plan, RenderProblem> {
    document::check(fields).map_err(|problem| RenderProblem::Field { problem })?;
    let values = environments::values(fields, layers);
    let mut r = Renderer {
        values: &values,
        used: BTreeMap::new(),
    };
    let url = r.template(fields.url.trim())?;
    let rows = |text: &str, query: bool| {
        if query {
            document::query_rows(text)
        } else {
            document::header_rows(text)
        }
        .map_err(|problem| RenderProblem::Field { problem })
    };
    let mut query = Vec::new();
    for row in rows(&fields.query, true)?.into_iter().filter(|r| r.enabled) {
        let name = r.template(&row.name)?;
        if name.plain().is_none() {
            return Err(RenderProblem::SecretInName);
        }
        query.push((name, r.template(&row.value)?));
    }
    let mut headers = Vec::new();
    for row in rows(&fields.headers, false)?
        .into_iter()
        .filter(|r| r.enabled)
    {
        headers.push((r.name(&row.name)?, r.template(&row.value)?));
    }
    let auth = auth(&fields.settings, &mut r)?;
    let body = match (fields.body_kind.as_str(), stored_body) {
        ("file", Some(relative)) => Body::File {
            relative: relative.to_owned(),
        },
        ("file", None) => Body::None,
        (_, Some(relative)) => Body::File {
            relative: relative.to_owned(),
        },
        ("form", None) => {
            let mut out = Vec::new();
            for row in rows(&fields.body, true)?.into_iter().filter(|r| r.enabled) {
                out.push((r.template(&row.name)?, r.template(&row.value)?));
            }
            Body::Form(out)
        }
        ("multipart", None) => {
            if headers
                .iter()
                .any(|(n, _)| n.eq_ignore_ascii_case("content-type"))
            {
                return Err(RenderProblem::MultipartContentType);
            }
            Body::Multipart(multipart(&fields.body, &mut r)?)
        }
        (_, None) if fields.body.is_empty() => Body::None,
        (_, None) => Body::Text {
            text: r.template(&fields.body)?,
            json: fields.body_kind == "json",
        },
    };
    if let Auth::ApiKey { name, query: q, .. } = &auth {
        // v1: the key never silently replaces an operator row.
        let clash = if *q {
            query
                .iter()
                .any(|(n, _)| n.plain().as_deref() == Some(name.as_str()))
        } else {
            headers.iter().any(|(n, _)| n.eq_ignore_ascii_case(name))
        };
        if clash {
            return Err(RenderProblem::Auth);
        }
    }
    Ok(Plan {
        method: fields.method.clone(),
        url,
        query,
        headers,
        auth,
        body,
        used: r.used,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layers() -> Layers {
        Layers {
            collection: Some(
                "format_version = 1\nid = \"c\"\n[variables]\nhost = \"api.example\"\ntoken = { secret = \"dev-token\" }\ncs = { secret = \"client-secret\" }\n".into(),
            ),
            ..Layers::default()
        }
    }

    #[test]
    fn secrets_stay_slots_and_names_never_hold_them() {
        let fields = Fields {
            method: "POST".into(),
            url: "https://{{host}}/items?x={{{host}}}".into(),
            headers: "X-Token: {{token}}\n# X-Off: 1".into(),
            query: "q={{token}}".into(),
            body: "{\"t\": \"{{token}}\"}".into(),
            body_kind: "json".into(),
            settings: "[auth]\nkind = \"oauth2\"\ntoken_url = \"https://{{host}}/token\"\nclient_id = \"id\"\nclient_secret = \"{{cs}}\"\n".into(),
        };
        let plan = render(&fields, &layers(), None).expect("fixture");
        assert_eq!(
            plan.url.plain().as_deref(),
            Some("https://api.example/items?x={api.example}")
        );
        assert_eq!(plan.headers.len(), 1);
        assert_eq!(plan.headers[0].1.bound(), "{{secret:dev-token}}");
        assert_eq!(
            plan.references().into_iter().collect::<Vec<_>>(),
            ["client-secret", "dev-token"]
        );
        let mut secrets = Secrets::new();
        secrets.insert("dev-token".into(), Zeroizing::new("v".into()));
        assert_eq!(
            plan.headers[0]
                .1
                .resolve(&secrets)
                .as_deref()
                .map(String::as_str),
            Some("v")
        );
        assert_eq!(plan.headers[0].1.masked(), environments::MASK);
        let named = Fields {
            headers: "{{token}}: x".into(),
            ..fields.clone()
        };
        assert_eq!(
            render(&named, &layers(), None),
            Err(RenderProblem::SecretInName)
        );
    }

    #[test]
    fn unresolved_unclosed_and_nested_names_are_refused() {
        let base = Fields::default();
        let with = |url: &str| Fields {
            url: url.into(),
            ..base.clone()
        };
        assert_eq!(
            render(&with("https://{{nope}}/"), &layers(), None),
            Err(RenderProblem::Unresolved {
                name: "nope".into()
            })
        );
        assert_eq!(
            render(&with("https://{{host/"), &layers(), None),
            Err(RenderProblem::Unclosed)
        );
        let nested = Layers {
            collection: Some("format_version = 1\n[variables]\na = \"{{b}}\"\n".into()),
            ..Layers::default()
        };
        assert_eq!(
            render(&with("https://{{a}}/"), &nested, None),
            Err(RenderProblem::Nested { name: "a".into() })
        );
    }

    #[test]
    fn auth_kinds_follow_v1() {
        let with = |settings: &str| Fields {
            settings: settings.into(),
            ..Fields::default()
        };
        let basic = render(
            &with("[auth]\nkind = \"basic\"\nusername = \"a:b\"\nsecret = \"s\"\n"),
            &layers(),
            None,
        );
        assert_eq!(basic, Err(RenderProblem::Auth));
        let key = Fields {
            headers: "X-Key: literal".into(),
            ..with("[auth]\nkind = \"api-key\"\nname = \"x-key\"\nsecret = \"s\"\n")
        };
        assert_eq!(render(&key, &layers(), None), Err(RenderProblem::Auth));
        let oauth = render(
            &with(
                "[auth]\nkind = \"oauth2\"\ntoken_url = \"https://t/\"\nclient_id = \"i\"\nclient_secret = \"{{host}}\"\n",
            ),
            &layers(),
            None,
        );
        assert_eq!(
            oauth,
            Err(RenderProblem::Auth),
            "client_secret must name a secret variable"
        );
    }

    #[test]
    fn bodies_by_kind() {
        let multipart = Fields {
            method: "POST".into(),
            body_kind: "multipart".into(),
            body: "[[parts]]\nname = \"a\"\nvalue = \"{{host}}\"\n[[parts]]\nname = \"f\"\nfile = \"data/x.bin\"\nmime = \"application/x-test\"\n".into(),
            ..Fields::default()
        };
        let plan = render(&multipart, &layers(), None).expect("fixture");
        assert_eq!(
            plan.body,
            Body::Multipart(vec![
                Part {
                    name: "a".into(),
                    value: PartValue::Text(Template::text("api.example"))
                },
                Part {
                    name: "f".into(),
                    value: PartValue::File {
                        relative: "data/x.bin".into(),
                        mime: Some("application/x-test".into())
                    }
                }
            ])
        );
        let typed = Fields {
            headers: "Content-Type: text/plain".into(),
            ..multipart
        };
        assert_eq!(
            render(&typed, &layers(), None),
            Err(RenderProblem::MultipartContentType)
        );
        let stored = render(&Fields::default(), &layers(), Some("b.bin")).expect("fixture");
        assert_eq!(
            stored.body,
            Body::File {
                relative: "b.bin".into()
            }
        );
    }
}
