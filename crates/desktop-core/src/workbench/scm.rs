//! The built-in Strata Cloud Manager collection (C14): v1's sanitized
//! catalog of 193 request specifications (`scm-catalog.json`, copied from
//! v1 74754c02 `src/api_client/scm/catalog.json`, SHA-256 914a9ee7…037a8c)
//! written as an ordinary collection in the private API Client folder. The
//! entries are generic request specifications, not dedicated workflows or a
//! claim of live qualification. Credentials are `tenant.*` references that
//! only native tenant authority may resolve (never the workbench secret
//! store, and nothing is sent in C14). A refresh replaces or removes a file
//! only while it is exactly as last written; edits are kept and deleted
//! files are not recreated.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use toml_edit::{ArrayOfTables, DocumentMut, InlineTable, Item, Table, value};
use ts_rs::TS;

use super::document::hash_bytes;
use super::project::{self, ProjectProblem};

pub const COLLECTION_ID: &str = "prisma-airs-scm";
pub const NAME: &str = "Strata Cloud Manager";
pub const FOLDER: &str = "scm";
/// Reserved: references the workbench secret store never holds.
pub const MANAGED_PREFIX: &str = "tenant.";
const NO_TENANT: &str = "tenant.select";
pub const CATALOG_SHA256: &str = "914a9ee76723f3fa5bc566806aedbd7c0f7de2f457339c117ac6df3864037a8c";

#[derive(Debug, Deserialize)]
pub struct Catalog {
    pub format: String,
    pub version: u32,
    pub variables: BTreeMap<String, String>,
    pub requests: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
pub struct Entry {
    pub id: String,
    pub folder: String,
    pub name: String,
    pub method: String,
    pub url: String,
    pub headers: Vec<Row>,
    pub query: Vec<Row>,
    pub auth: String,
    pub body_kind: String,
    pub body: String,
}

#[derive(Debug, Deserialize)]
pub struct Row {
    pub name: String,
    pub value: String,
    pub enabled: bool,
}

const CATALOG: &str = include_str!("scm-catalog.json");

/// The embedded catalog (validated by tests).
pub fn catalog() -> &'static Catalog {
    static PARSED: OnceLock<Catalog> = OnceLock::new();
    PARSED.get_or_init(|| serde_json::from_str(CATALOG).expect("valid embedded SCM catalog"))
}

/// What a refresh did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Refresh {
    pub written: u32,
    /// Files left alone because they were edited after being written.
    pub kept: u32,
    pub removed: u32,
}

fn segment(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' => '-',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim().to_owned();
    if cleaned.is_empty() {
        "request".into()
    } else {
        cleaned.chars().take(120).collect()
    }
}

/// The request file and its body sidecar for an entry (v1's layout).
pub fn paths(entry: &Entry) -> (String, String) {
    let mut parts: Vec<String> = entry
        .folder
        .split('/')
        .filter(|p| !p.is_empty())
        .map(segment)
        .collect();
    let name = segment(&entry.name);
    parts.push(format!("{name}{}", project::REQUEST_SUFFIX));
    let relative = parts.join("/");
    let stem = relative.trim_end_matches(".toml");
    let body = format!(
        "{stem}.{}",
        if entry.body_kind == "json" {
            "body.json"
        } else {
            "body.txt"
        }
    );
    (relative, body)
}

fn rows(entries: &[Row]) -> ArrayOfTables {
    let mut rows = ArrayOfTables::new();
    for row in entries {
        let mut table = Table::new();
        table["name"] = value(&row.name);
        table["value"] = value(&row.value);
        if !row.enabled {
            table["enabled"] = value(false);
        }
        rows.push(table);
    }
    rows
}

pub fn request_text(entry: &Entry, body: &str) -> String {
    let mut doc = DocumentMut::new();
    doc["format_version"] = value(1);
    doc["id"] = value(&entry.id);
    doc["method"] = value(&entry.method);
    doc["url"] = value(&entry.url);
    if !entry.body.is_empty() {
        doc["body"] = value(body);
    }
    if entry.body_kind != "text" {
        doc["body_kind"] = value(&entry.body_kind);
    }
    if !entry.headers.is_empty() {
        doc["headers"] = Item::ArrayOfTables(rows(&entry.headers));
    }
    if !entry.query.is_empty() {
        doc["query"] = Item::ArrayOfTables(rows(&entry.query));
    }
    if entry.auth == "oauth2" {
        let mut auth = Table::new();
        auth["kind"] = value("oauth2");
        auth["token_url"] = value("{{token_url}}");
        auth["client_id"] = value("{{client_id}}");
        auth["client_secret"] = value("{{client_secret}}");
        auth["scope"] = value("tsg_id:{{tsg_id}}");
        doc["auth"] = Item::Table(auth);
    }
    doc.to_string()
}

fn secret(reference: &str) -> Item {
    let mut table = InlineTable::new();
    table.insert("secret", reference.into());
    value(table)
}

pub fn collection_text() -> String {
    let catalog = catalog();
    let mut doc = DocumentMut::new();
    doc["format_version"] = value(1);
    doc["id"] = value(COLLECTION_ID);
    doc["name"] = value(NAME);
    let mut variables = Table::new();
    for (name, url) in &catalog.variables {
        variables[name] = value(url);
    }
    variables["tsg_id"] = value("");
    for name in ["client_id", "client_secret", "x_pan_token"] {
        variables[name] = secret(NO_TENANT);
    }
    doc["variables"] = Item::Table(variables);
    format!(
        "# Managed by Prisma AIRS Desktop. Edits are kept; unedited files are refreshed\n# when the built-in Strata Cloud Manager catalog changes.\n{doc}"
    )
}

fn place(
    root: &Path,
    key: &str,
    bytes: &[u8],
    previous: &BTreeMap<String, String>,
    next: &mut BTreeMap<String, String>,
    refresh: &mut Refresh,
) -> Result<(), ProjectProblem> {
    let relative = project::relative_path(key, false).ok_or(ProjectProblem::BadPath)?;
    let recorded = previous.get(key);
    match project::read_optional(root, &relative, 16 * 1024 * 1024)? {
        Some(existing) => {
            let current = hash_bytes(&existing);
            if recorded == Some(&current) || existing == bytes {
                if existing != bytes {
                    project::replace(root, &relative, bytes)?;
                    refresh.written += 1;
                }
                next.insert(key.to_owned(), hash_bytes(bytes));
            } else {
                refresh.kept += 1;
                if let Some(recorded) = recorded {
                    next.insert(key.to_owned(), recorded.clone());
                }
            }
        }
        None => {
            if let Some(recorded) = recorded {
                // Deleted after a refresh: respected.
                next.insert(key.to_owned(), recorded.clone());
            } else {
                project::replace(root, &relative, bytes)?;
                next.insert(key.to_owned(), hash_bytes(bytes));
                refresh.written += 1;
            }
        }
    }
    Ok(())
}

/// Creates or refreshes the collection at `root` (inside the private API
/// Client folder); returns the new write record and what was done.
pub fn refresh(
    root: &Path,
    previous: &BTreeMap<String, String>,
) -> Result<(BTreeMap<String, String>, Refresh), ProjectProblem> {
    std::fs::create_dir_all(root).map_err(|_| ProjectProblem::Io)?;
    let root = root.canonicalize().map_err(|_| ProjectProblem::Io)?;
    let mut next = BTreeMap::new();
    let mut refresh = Refresh::default();
    let mut wanted = BTreeSet::new();
    place(
        &root,
        "collection.toml",
        collection_text().as_bytes(),
        previous,
        &mut next,
        &mut refresh,
    )?;
    wanted.insert("collection.toml".to_owned());
    for entry in &catalog().requests {
        let (relative, body) = paths(entry);
        place(
            &root,
            &relative,
            request_text(entry, &body).as_bytes(),
            previous,
            &mut next,
            &mut refresh,
        )?;
        wanted.insert(relative);
        if !entry.body.is_empty() {
            place(
                &root,
                &body,
                entry.body.as_bytes(),
                previous,
                &mut next,
                &mut refresh,
            )?;
            wanted.insert(body);
        }
    }
    for (key, recorded) in previous {
        if wanted.contains(key) {
            continue;
        }
        let Some(relative) = project::relative_path(key, false) else {
            continue;
        };
        if let Ok(Some(bytes)) = project::read_optional(&root, &relative, 16 * 1024 * 1024) {
            if &hash_bytes(&bytes) == recorded {
                project::remove(&root, &relative)?;
                refresh.removed += 1;
            } else {
                refresh.kept += 1;
            }
        }
    }
    Ok((next, refresh))
}

pub fn folder(private: &Path) -> PathBuf {
    private.join(FOLDER)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workbench::testdir::Dir;

    #[test]
    fn the_catalog_is_v1s_and_every_entry_is_a_valid_request() {
        use sha2::Digest;
        assert_eq!(
            hex::encode(sha2::Sha256::digest(CATALOG.as_bytes())),
            CATALOG_SHA256
        );
        let catalog = catalog();
        assert_eq!(catalog.format, "prisma-airs-desktop/scm-catalog");
        assert_eq!(catalog.requests.len(), 193);
        let mut methods = BTreeMap::<&str, usize>::new();
        for entry in &catalog.requests {
            *methods.entry(entry.method.as_str()).or_default() += 1;
        }
        assert_eq!(
            methods,
            BTreeMap::from([
                ("DELETE", 17),
                ("GET", 112),
                ("PATCH", 1),
                ("POST", 46),
                ("PUT", 17)
            ])
        );
        let d = Dir::new();
        let (_, done) = refresh(d.path(), &BTreeMap::new()).expect("fixture");
        let found = project::discover(d.path()).expect("fixture");
        assert!(found.complete, "{:?}", found.issues);
        assert_eq!(found.requests.len(), 193);
        assert_eq!(found.id, COLLECTION_ID);
        assert!(done.written >= 194);
        for entry in found.requests.iter().take(40) {
            let doc = super::super::document::open(&found.root, entry).expect("fixture");
            assert!(
                super::super::document::check(&doc.saved).is_ok(),
                "{}",
                entry.relative
            );
        }
    }

    #[test]
    fn refresh_keeps_edits_and_respects_deletions() {
        let d = Dir::new();
        let (record, _) = refresh(d.path(), &BTreeMap::new()).expect("fixture");
        let found = project::discover(d.path()).expect("fixture");
        let edited = &found.requests[0].relative;
        let deleted = &found.requests[1].relative;
        std::fs::write(d.path().join(edited), "format_version = 1\n# mine\n").expect("fixture");
        std::fs::remove_file(d.path().join(deleted)).expect("fixture");
        let (_, again) = refresh(d.path(), &record).expect("fixture");
        assert_eq!(again.kept, 1);
        assert_eq!(again.written, 0);
        assert!(!d.path().join(deleted).exists());
        assert!(
            std::fs::read_to_string(d.path().join(edited))
                .expect("fixture")
                .contains("# mine")
        );
    }
}
