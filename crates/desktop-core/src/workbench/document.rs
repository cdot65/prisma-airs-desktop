//! Request documents (C14). The presentation owns the editor text; Rust
//! owns what was read from disk (the baseline), the revision that names it
//! and whether a captured snapshot can be written faithfully. A request's
//! metadata is edited through `toml_edit`, so comments, row decoration,
//! extension keys and CRLF line endings survive a Save (v1).

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, Value};
use ts_rs::TS;

use super::project::{self, ProjectProblem, REQUEST_SUFFIX};

/// Body text the editor holds (larger or non-text bodies are kept as they
/// are and cannot be edited here).
pub const BODY_BYTES: usize = 4 * 1024 * 1024;
pub const URL_CHARS: usize = 8_192;
pub const METHOD_CHARS: usize = 32;
pub const BODY_KINDS: [&str; 5] = ["text", "json", "form", "multipart", "file"];
/// Built-in methods (others: letters and `-` only, as v1's custom method).
pub const METHODS: [&str; 7] = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

/// The editable snapshot of a request, exactly as the editor holds it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Fields {
    pub method: String,
    pub url: String,
    /// One `Name: value` row per line; `# ` disables a row.
    pub headers: String,
    /// One `name=value` row per line; `# ` disables a row.
    pub query: String,
    pub body: String,
    pub body_kind: String,
    /// `[auth]` and `[variables]` TOML; values are references, never
    /// credentials.
    pub settings: String,
}

impl Default for Fields {
    fn default() -> Self {
        Self {
            method: "GET".into(),
            url: "https://example.com/".into(),
            headers: String::new(),
            query: String::new(),
            body: String::new(),
            body_kind: "text".into(),
            settings: String::new(),
        }
    }
}

/// Which part of a snapshot cannot be written faithfully.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum FieldName {
    Method,
    Url,
    Headers,
    Query,
    Body,
    BodyKind,
    Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FieldProblem {
    pub field: FieldName,
    /// The 1-based row, for row fields.
    pub line: Option<u32>,
}

impl FieldProblem {
    fn at(field: FieldName, line: Option<usize>) -> Self {
        Self {
            field,
            line: line.map(|l| u32::try_from(l + 1).unwrap_or(u32::MAX)),
        }
    }
}

/// One parsed row of a rows field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    pub value: String,
    pub enabled: bool,
}

/// `Name: value` rows (blank lines skipped).
pub fn header_rows(text: &str) -> Result<Vec<Row>, FieldProblem> {
    rows(text, ':', FieldName::Headers, true)
}

/// `name=value` rows (blank lines skipped; values kept verbatim).
pub fn query_rows(text: &str) -> Result<Vec<Row>, FieldProblem> {
    rows(text, '=', FieldName::Query, false)
}

fn rows(text: &str, split: char, field: FieldName, trim: bool) -> Result<Vec<Row>, FieldProblem> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, line)| {
            let (enabled, line) = match line.strip_prefix("# ") {
                Some(rest) => (false, rest),
                None => (true, line),
            };
            let (name, value) = line
                .split_once(split)
                .ok_or(FieldProblem::at(field, Some(i)))?;
            let name = name.trim();
            if name.is_empty() || name.starts_with("# ") || name.contains(['\r', '=']) {
                return Err(FieldProblem::at(field, Some(i)));
            }
            Ok(Row {
                name: name.to_owned(),
                value: if trim { value.trim() } else { value }.to_owned(),
                enabled,
            })
        })
        .collect()
}

pub(crate) fn valid_reference(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

pub(crate) fn valid_variable(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
}

/// `[variables]`: strings or `{secret = "reference"}` only.
pub(crate) fn variables_ok(doc: &DocumentMut) -> bool {
    let Some(variables) = doc.get("variables") else {
        return true;
    };
    let Some(table) = variables.as_table_like() else {
        return false;
    };
    table.iter().all(|(name, value)| {
        valid_variable(name)
            && (value.is_str()
                || (value.as_table_like().is_some_and(|t| t.len() == 1)
                    && value
                        .get("secret")
                        .and_then(Item::as_str)
                        .is_some_and(valid_reference)))
    })
}

const AUTH_KEYS: [&str; 9] = [
    "kind",
    "secret",
    "username",
    "name",
    "location",
    "token_url",
    "client_id",
    "client_secret",
    "scope",
];
pub const AUTH_KINDS: [&str; 5] = ["none", "basic", "bearer", "api-key", "oauth2"];

/// Request settings: only `[auth]` (named references, never inline
/// credentials) and `[variables]`.
pub fn settings_ok(text: &str) -> Option<DocumentMut> {
    let doc: DocumentMut = text.parse().ok()?;
    if !doc.iter().all(|(k, _)| matches!(k, "auth" | "variables")) || !variables_ok(&doc) {
        return None;
    }
    if let Some(auth) = doc.get("auth") {
        let table = auth.as_table_like()?;
        if !table
            .iter()
            .all(|(k, v)| AUTH_KEYS.contains(&k) && v.is_str())
        {
            return None;
        }
        if let Some(kind) = auth.get("kind").and_then(Item::as_str)
            && !AUTH_KINDS.contains(&kind)
        {
            return None;
        }
        if let Some(secret) = auth.get("secret").and_then(Item::as_str)
            && !valid_reference(secret)
        {
            return None;
        }
    }
    Some(doc)
}

fn method_ok(method: &str) -> bool {
    !method.is_empty()
        && method.len() <= METHOD_CHARS
        && method.bytes().all(|b| b.is_ascii_alphabetic() || b == b'-')
}

/// Whether `fields` can be written faithfully (the first problem if not).
/// Malformed JSON in a JSON body is kept as typed (v1: a warning).
pub fn check(fields: &Fields) -> Result<(), FieldProblem> {
    if !method_ok(&fields.method) {
        return Err(FieldProblem::at(FieldName::Method, None));
    }
    if fields.url.trim().is_empty()
        || fields.url.chars().count() > URL_CHARS
        || fields.url.contains(['\r', '\n'])
    {
        return Err(FieldProblem::at(FieldName::Url, None));
    }
    header_rows(&fields.headers)?;
    query_rows(&fields.query)?;
    if !BODY_KINDS.contains(&fields.body_kind.as_str()) {
        return Err(FieldProblem::at(FieldName::BodyKind, None));
    }
    if fields.body.len() > BODY_BYTES {
        return Err(FieldProblem::at(FieldName::Body, None));
    }
    if fields.body_kind == "multipart" && !fields.body.is_empty() && !multipart_ok(&fields.body) {
        return Err(FieldProblem::at(FieldName::Body, None));
    }
    if !fields.settings.trim().is_empty() && settings_ok(&fields.settings).is_none() {
        return Err(FieldProblem::at(FieldName::Settings, None));
    }
    Ok(())
}

/// v1's multipart body: `[[parts]]` rows with `name` and exactly one of
/// `value` or `file` (collection-relative), optional `mime`.
fn multipart_ok(text: &str) -> bool {
    let Ok(doc) = text.parse::<DocumentMut>() else {
        return false;
    };
    if !doc.iter().all(|(k, _)| k == "parts") {
        return false;
    }
    let Some(parts) = doc.get("parts").and_then(Item::as_array_of_tables) else {
        return false;
    };
    parts.iter().all(|row| {
        row.iter()
            .all(|(k, v)| matches!(k, "name" | "value" | "file" | "mime") && v.is_str())
            && row.get("name").and_then(Item::as_str).is_some()
            && (row.contains_key("value") != row.contains_key("file"))
            && row
                .get("file")
                .and_then(Item::as_str)
                .is_none_or(|f| project::relative_path(f, false).is_some())
    })
}

/// A file's content hash (`None` when absent), streamed without a limit.
pub fn hash_file(root: &Path, relative: &str) -> Result<Option<String>, ProjectProblem> {
    let rel = project::relative_path(relative, false).ok_or(ProjectProblem::BadPath)?;
    let path = project::destination(root, &rel)?;
    let mut file = match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(ProjectProblem::Io),
        Ok(m) if !m.is_file() => return Err(ProjectProblem::Io),
        Ok(_) => std::fs::File::open(&path).map_err(|_| ProjectProblem::Io)?,
    };
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer).map_err(|_| ProjectProblem::Io)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(Some(hex::encode(hash.finalize())))
}

pub(crate) fn hash_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// What a document was read from: each file's hash (`None`: absent).
pub type Baseline = BTreeMap<String, Option<String>>;

/// The revision naming a baseline.
pub fn revision(baseline: &Baseline) -> String {
    let mut hash = Sha256::new();
    for (path, digest) in baseline {
        hash.update(path.as_bytes());
        hash.update([0]);
        match digest {
            Some(d) => {
                hash.update([1]);
                hash.update(d.as_bytes());
            }
            None => hash.update([0]),
        }
    }
    hex::encode(&hash.finalize()[..16])
}

/// A request document held by Rust.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub id: String,
    pub relative: String,
    /// The metadata text at the baseline (or as created).
    pub syntax: String,
    pub baseline: Baseline,
    pub saved: Fields,
    /// Never written yet.
    pub new: bool,
    /// The body is text within the editor limit.
    pub body_editable: bool,
    /// A changed reference (independent body copy) still to be saved.
    pub metadata_dirty: bool,
}

/// Why a document could not be opened.
pub fn open(root: &Path, entry: &project::RequestEntry) -> Result<Document, ProjectProblem> {
    let rel = project::relative_path(&entry.relative, false).ok_or(ProjectProblem::BadPath)?;
    let bytes = project::read_optional(root, &rel, project::METADATA_BYTES)?
        .ok_or(ProjectProblem::Missing)?;
    let syntax = String::from_utf8(bytes.clone()).map_err(|_| ProjectProblem::Missing)?;
    let doc: DocumentMut = syntax.parse().map_err(|_| ProjectProblem::Missing)?;
    // The fields come from the metadata the baseline hashes, never from the
    // (abbreviated, possibly older) inventory entry.
    if doc.get("id").and_then(Item::as_str) != Some(entry.id.as_str()) {
        return Err(ProjectProblem::Missing);
    }
    let method = doc
        .get("method")
        .and_then(Item::as_str)
        .ok_or(ProjectProblem::Missing)?
        .to_owned();
    let url = doc
        .get("url")
        .and_then(Item::as_str)
        .ok_or(ProjectProblem::Missing)?
        .to_owned();
    let mut baseline = Baseline::from([(entry.relative.clone(), Some(hash_bytes(&bytes)))]);
    let mut body = String::new();
    let mut body_editable = true;
    if let Some(reference) = doc.get("body").and_then(Item::as_str) {
        let digest = hash_file(root, reference)?;
        let rel = project::relative_path(reference, false).ok_or(ProjectProblem::BadPath)?;
        match project::read_optional(root, &rel, BODY_BYTES) {
            Ok(Some(bytes)) => match String::from_utf8(bytes) {
                Ok(text) => body = text,
                Err(_) => body_editable = false,
            },
            Err(ProjectProblem::TooLarge) => body_editable = false,
            Ok(None) => return Err(ProjectProblem::Missing),
            Err(e) => return Err(e),
        }
        baseline.insert(reference.to_owned(), digest);
    }
    let saved = Fields {
        method,
        url,
        headers: rows_from(&doc, "headers", ": ").ok_or(ProjectProblem::Missing)?,
        query: rows_from(&doc, "query", "=").ok_or(ProjectProblem::Missing)?,
        body,
        body_kind: doc
            .get("body_kind")
            .and_then(Item::as_str)
            .unwrap_or("text")
            .to_owned(),
        settings: settings_from(&doc),
    };
    Ok(Document {
        id: entry.id.clone(),
        relative: entry.relative.clone(),
        syntax,
        baseline,
        saved,
        new: false,
        body_editable,
        metadata_dirty: false,
    })
}

fn rows_from(doc: &DocumentMut, key: &str, join: &str) -> Option<String> {
    let Some(item) = doc.get(key) else {
        return Some(String::new());
    };
    let rows = item.as_array_of_tables()?;
    let mut lines = Vec::new();
    for row in rows {
        let name = row.get("name").and_then(Item::as_str)?;
        let value = row.get("value").and_then(Item::as_str)?;
        let enabled = match row.get("enabled") {
            Some(v) => v.as_bool()?,
            None => true,
        };
        if name.contains(['\r', '\n']) || value.contains(['\r', '\n']) || name.starts_with("# ") {
            return None;
        }
        if join == "=" && name.contains('=') {
            return None;
        }
        lines.push(format!(
            "{}{name}{join}{value}",
            if enabled { "" } else { "# " }
        ));
    }
    Some(lines.join("\n"))
}

fn settings_from(doc: &DocumentMut) -> String {
    let mut settings = DocumentMut::new();
    for key in ["auth", "variables"] {
        if let Some(item) = doc.get(key) {
            settings[key] = item.clone();
        }
    }
    settings.to_string()
}

fn field_value(doc: &mut DocumentMut, name: &str, value: &str) {
    if doc.get(name).and_then(Item::as_str) == Some(value) {
        return;
    }
    let mut replacement = Value::from(value);
    if let Some(old) = doc.get(name).and_then(Item::as_value) {
        *replacement.decor_mut() = old.decor().clone();
    }
    doc[name] = Item::Value(replacement);
}

/// Rewrites a rows array, keeping each surviving row's decoration and
/// extension keys (exact matches first, then by name).
fn edit_rows(doc: &mut DocumentMut, key: &str, rows: Vec<Row>) {
    let old = doc.get(key).and_then(Item::as_array_of_tables).cloned();
    let mut used = std::collections::BTreeSet::new();
    let mut out = ArrayOfTables::new();
    for row in rows {
        let find = |exact: bool, used: &std::collections::BTreeSet<usize>| {
            old.as_ref().and_then(|rows| {
                rows.iter()
                    .enumerate()
                    .find(|(i, r)| {
                        !used.contains(i)
                            && r.get("name").and_then(Item::as_str) == Some(row.name.as_str())
                            && (!exact
                                || (r.get("value").and_then(Item::as_str)
                                    == Some(row.value.as_str())
                                    && r.get("enabled").and_then(Item::as_bool).unwrap_or(true)
                                        == row.enabled))
                    })
                    .map(|(i, _)| i)
            })
        };
        let matched = find(true, &used).or_else(|| find(false, &used));
        let mut table = matched
            .and_then(|i| {
                used.insert(i);
                old.as_ref().and_then(|rows| rows.get(i)).cloned()
            })
            .unwrap_or_else(Table::new);
        for (k, v) in [("name", row.name), ("value", row.value)] {
            let decor = table
                .get(k)
                .and_then(Item::as_value)
                .map(|v| v.decor().clone());
            let mut value = Value::from(v);
            if let Some(decor) = decor {
                *value.decor_mut() = decor;
            }
            table[k] = Item::Value(value);
        }
        if !row.enabled || table.contains_key("enabled") {
            table["enabled"] = toml_edit::value(row.enabled);
        }
        table.set_position(None);
        out.push(table);
    }
    if out.is_empty() {
        doc.remove(key);
    } else {
        doc[key] = Item::ArrayOfTables(out);
    }
}

impl Document {
    pub fn revision(&self) -> String {
        revision(&self.baseline)
    }

    /// The body sidecar this document writes, if any.
    pub fn body_reference(&self) -> Option<String> {
        self.syntax
            .parse::<DocumentMut>()
            .ok()?
            .get("body")
            .and_then(Item::as_str)
            .map(str::to_owned)
    }

    pub fn schema(&self) -> Option<String> {
        self.syntax
            .parse::<DocumentMut>()
            .ok()?
            .get("schema")
            .and_then(Item::as_str)
            .map(str::to_owned)
    }

    pub fn dirty(&self, fields: &Fields) -> bool {
        self.new || self.metadata_dirty || fields != &self.saved
    }

    /// The metadata text `fields` saves as (call after `check`).
    pub fn edited_syntax(&self, fields: &Fields) -> Option<String> {
        let mut doc: DocumentMut = self.syntax.parse().ok()?;
        field_value(&mut doc, "method", &fields.method);
        field_value(&mut doc, "url", &fields.url);
        if fields.body_kind != self.saved.body_kind || (self.new && fields.body_kind != "text") {
            field_value(&mut doc, "body_kind", &fields.body_kind);
        }
        if fields.headers != self.saved.headers {
            edit_rows(&mut doc, "headers", header_rows(&fields.headers).ok()?);
        }
        if fields.query != self.saved.query {
            edit_rows(&mut doc, "query", query_rows(&fields.query).ok()?);
        }
        if fields.settings != self.saved.settings {
            let mut settings = if fields.settings.trim().is_empty() {
                DocumentMut::new()
            } else {
                settings_ok(&fields.settings)?
            };
            for key in ["auth", "variables"] {
                match settings.remove(key) {
                    Some(item) => doc[key] = item,
                    None => {
                        doc.remove(key);
                    }
                }
            }
        }
        let text = doc.to_string();
        Some(if self.syntax.contains("\r\n") {
            text.replace("\r\n", "\n").replace('\n', "\r\n")
        } else {
            text
        })
    }

    /// Sets (or clears) the schema association; the schema file itself is
    /// separately owned and never rewritten by a request Save.
    pub fn associate_schema(&mut self, schema: Option<&str>) -> bool {
        let Ok(mut doc) = self.syntax.parse::<DocumentMut>() else {
            return false;
        };
        match schema {
            Some(s) => field_value(&mut doc, "schema", s),
            None => {
                doc.remove("schema");
            }
        }
        let edited = doc.to_string();
        if edited != self.syntax {
            self.syntax = edited;
            self.metadata_dirty = true;
        }
        true
    }
}

/// A new, unsaved request at `relative` (a `.request.toml` path) with an
/// independent body sidecar beside it.
pub fn create(relative: &str) -> Result<Document, ProjectProblem> {
    if !relative.ends_with(REQUEST_SUFFIX) || project::relative_path(relative, true).is_none() {
        return Err(ProjectProblem::BadPath);
    }
    let id = project::new_id().ok_or(ProjectProblem::Io)?;
    let body = format!("{}.body.txt", relative.trim_end_matches(REQUEST_SUFFIX));
    let mut doc = DocumentMut::new();
    doc["format_version"] = toml_edit::value(1);
    doc["id"] = toml_edit::value(id.as_str());
    doc["method"] = toml_edit::value("GET");
    doc["url"] = toml_edit::value("https://example.com/");
    doc["body"] = toml_edit::value(body.as_str());
    Ok(Document {
        id,
        relative: relative.to_owned(),
        syntax: doc.to_string(),
        baseline: Baseline::from([(relative.to_owned(), None), (body, None)]),
        saved: Fields::default(),
        new: true,
        body_editable: true,
        metadata_dirty: false,
    })
}

/// Why a Save did not write.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SaveRefusal {
    /// The snapshot cannot be written faithfully ("Fix to save").
    Invalid { problem: FieldProblem },
    /// The files changed on disk since the baseline.
    Conflict,
    /// The collection changed or its inventory is incomplete.
    Incomplete,
    /// The request's identity moved to another file.
    Moved,
    /// Another request now uses this body file.
    SharedBody,
    /// An earlier save of this request was interrupted.
    Interrupted,
    /// Something already exists at a new request's path.
    Exists,
    /// The files could not be written.
    Io,
}

/// One planned file change of a Save.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub relative: String,
    pub before: Option<String>,
    pub after: String,
    #[serde(skip)]
    pub bytes: Vec<u8>,
}

/// Whether the files on disk still equal the baseline (and no other body
/// reference appeared in the metadata).
pub fn unchanged_on_disk(root: &Path, document: &Document) -> Result<bool, ProjectProblem> {
    for (path, digest) in &document.baseline {
        if &hash_file(root, path)? != digest {
            return Ok(false);
        }
    }
    if !document.new
        && let Some(bytes) = project::read_optional(
            root,
            &project::relative_path(&document.relative, false).ok_or(ProjectProblem::BadPath)?,
            project::METADATA_BYTES,
        )?
        && let Some(reference) = project::parse_metadata(&bytes)
            .and_then(|d| d.get("body").and_then(Item::as_str).map(str::to_owned))
        && !document.baseline.contains_key(&reference)
    {
        return Ok(false);
    }
    Ok(true)
}

/// Plans a Save of `fields`: the refusal, or the file changes (body first,
/// metadata last). Read-only.
pub fn plan(
    root: &Path,
    catalog: &project::Catalog,
    collection: &str,
    document: &Document,
    fields: &Fields,
) -> Result<Vec<Change>, SaveRefusal> {
    check(fields).map_err(|problem| SaveRefusal::Invalid { problem })?;
    if !catalog.complete || catalog.id != collection {
        return Err(SaveRefusal::Incomplete);
    }
    if catalog
        .requests
        .iter()
        .any(|r| r.id == document.id && r.relative != document.relative)
    {
        return Err(SaveRefusal::Moved);
    }
    // A file whose identity another file now also claims has no single
    // owner to write for.
    if catalog
        .issues
        .iter()
        .any(|i| i.kind == project::IssueKind::Duplicate && i.relative == document.relative)
    {
        return Err(SaveRefusal::Incomplete);
    }
    match unchanged_on_disk(root, document) {
        Ok(true) => {}
        Ok(false) => {
            return Err(if document.new {
                SaveRefusal::Exists
            } else {
                SaveRefusal::Conflict
            });
        }
        Err(_) => return Err(SaveRefusal::Io),
    }
    if !document.dirty(fields) {
        return Ok(Vec::new());
    }
    let reference = document.body_reference();
    let mut changes = Vec::new();
    let body_changed = document.new || fields.body != document.saved.body;
    if body_changed {
        if !document.body_editable {
            return Err(SaveRefusal::Invalid {
                problem: FieldProblem::at(FieldName::Body, None),
            });
        }
        let Some(reference) = &reference else {
            return Err(SaveRefusal::Invalid {
                problem: FieldProblem::at(FieldName::Body, None),
            });
        };
        if shared(catalog, &document.relative, reference) {
            return Err(SaveRefusal::SharedBody);
        }
        let bytes = fields.body.clone().into_bytes();
        changes.push(Change {
            relative: reference.clone(),
            before: document.baseline.get(reference).cloned().flatten(),
            after: hash_bytes(&bytes),
            bytes,
        });
    }
    let edited = document
        .edited_syntax(fields)
        .ok_or(SaveRefusal::Invalid {
            problem: FieldProblem::at(FieldName::Settings, None),
        })?
        .into_bytes();
    let before = document.baseline.get(&document.relative).cloned().flatten();
    if before.as_deref() != Some(hash_bytes(&edited).as_str()) {
        changes.push(Change {
            relative: document.relative.clone(),
            before,
            after: hash_bytes(&edited),
            bytes: edited,
        });
    }
    Ok(changes)
}

/// Whether any other request file in the collection (a duplicate or
/// invalid one included) references `body`.
fn shared(catalog: &project::Catalog, relative: &str, body: &str) -> bool {
    catalog
        .bodies
        .get(body)
        .is_some_and(|owners| owners.iter().any(|r| r != relative))
}

/// The document after its planned changes were written.
pub fn committed(document: &Document, fields: &Fields, changes: &[Change]) -> Document {
    let mut next = document.clone();
    for change in changes {
        next.baseline
            .insert(change.relative.clone(), Some(change.after.clone()));
        if change.relative == document.relative {
            next.syntax = String::from_utf8_lossy(&change.bytes).into_owned();
        }
    }
    next.saved = fields.clone();
    next.new = false;
    next.metadata_dirty = false;
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_round_trip_and_report_their_line() {
        assert_eq!(
            header_rows("A: 1\n\n# B: two words ").expect("fixture"),
            vec![
                Row {
                    name: "A".into(),
                    value: "1".into(),
                    enabled: true
                },
                Row {
                    name: "B".into(),
                    value: "two words".into(),
                    enabled: false
                },
            ]
        );
        assert_eq!(
            header_rows("A: 1\nbroken").expect_err("fixture"),
            FieldProblem {
                field: FieldName::Headers,
                line: Some(2)
            }
        );
        assert_eq!(
            query_rows("q= spaced ").expect("fixture")[0].value,
            " spaced "
        );
        assert!(query_rows("=v").is_err());
    }

    #[test]
    fn settings_hold_references_never_credentials() {
        assert!(settings_ok("[auth]\nkind = \"bearer\"\nsecret = \"token\"\n").is_some());
        assert!(settings_ok("[variables]\nhost = \"x\"\nkey = { secret = \"k\" }\n").is_some());
        assert!(settings_ok("[auth]\nkind = \"bearer\"\ntoken = \"abc\"\n").is_none());
        assert!(settings_ok("[auth]\nkind = \"digest\"\n").is_none());
        assert!(settings_ok("[auth]\nsecret = \"bad ref\"\n").is_none());
        assert!(settings_ok("[variables]\nkey = { secret = \"k\", value = \"v\" }\n").is_none());
        assert!(settings_ok("[environment]\n").is_none());
        assert!(settings_ok("[auth\n").is_none());
    }

    #[test]
    fn checks_name_the_field_that_cannot_be_written() {
        let ok = Fields::default();
        assert_eq!(check(&ok), Ok(()));
        let field = |f: Fields| check(&f).expect_err("fixture").field;
        assert_eq!(
            field(Fields {
                method: "G T".into(),
                ..ok.clone()
            }),
            FieldName::Method
        );
        assert_eq!(
            field(Fields {
                url: " ".into(),
                ..ok.clone()
            }),
            FieldName::Url
        );
        assert_eq!(
            field(Fields {
                body_kind: "xml".into(),
                ..ok.clone()
            }),
            FieldName::BodyKind
        );
        assert_eq!(
            field(Fields {
                body_kind: "multipart".into(),
                body: "[[parts]]\nvalue = \"x\"\n".into(),
                ..ok.clone()
            }),
            FieldName::Body
        );
        assert_eq!(
            check(&Fields {
                body_kind: "json".into(),
                body: "{not json".into(),
                ..ok.clone()
            }),
            Ok(())
        );
    }

    #[test]
    fn edits_keep_comments_extensions_and_line_endings() {
        let syntax = "# kept\r\nformat_version = 1\r\nid = \"a\"\r\nmethod = \"GET\" # verb\r\nurl = \"https://a/\"\r\nx-ext = 3\r\n\r\n[[headers]]\r\nname = \"A\"\r\nvalue = \"1\"\r\nnote = \"kept\"\r\n";
        let doc = Document {
            id: "a".into(),
            relative: "a.request.toml".into(),
            syntax: syntax.into(),
            baseline: Baseline::new(),
            saved: Fields {
                method: "GET".into(),
                url: "https://a/".into(),
                headers: "A: 1".into(),
                ..Fields::default()
            },
            new: false,
            body_editable: true,
            metadata_dirty: false,
        };
        let fields = Fields {
            method: "POST".into(),
            headers: "B: 2\nA: 1".into(),
            ..doc.saved.clone()
        };
        let edited = doc.edited_syntax(&fields).expect("fixture");
        assert!(edited.starts_with("# kept\r\n"));
        assert!(edited.contains("method = \"POST\" # verb\r\n"));
        assert!(edited.contains("x-ext = 3"));
        assert!(edited.contains("note = \"kept\""));
        assert!(!edited.replace("\r\n", "").contains('\n'));
        assert!(edited.find("\"B\"").expect("fixture") < edited.find("\"A\"").expect("fixture"));
    }
}
