//! v1's portable project format (C14): a collection is a folder holding
//! `collection.toml` (`format_version = 1`, `id`, `name`, optional
//! `[variables]`), `*.request.toml` request files with their body sidecars,
//! shared `environments/*.toml` and schema text files. Discovery is
//! read-only and bounded; it never follows symbolic links, enters `.git` or
//! a nested collection, and reports (never repairs) invalid or duplicate
//! identities. Writes are atomic and stay inside the collection.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use toml_edit::{DocumentMut, Item};
use ts_rs::TS;

/// Request and collection metadata files (v1: 1 MiB each).
pub const METADATA_BYTES: usize = 1024 * 1024;
/// Request files a collection may hold (beyond: incomplete inventory).
pub const MAX_REQUESTS: usize = 2_000;
/// Directory entries discovery reads before giving up (incomplete).
pub const MAX_ENTRIES: usize = 20_000;
/// Folder nesting discovery follows.
pub const MAX_DEPTH: usize = 24;
/// Collection-relative paths (bytes).
pub const MAX_PATH: usize = 512;
pub const REQUEST_SUFFIX: &str = ".request.toml";

/// Why a project operation was refused (sanitized; never file contents).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ProjectProblem {
    /// The folder holds no readable `collection.toml` of format 1.
    NotCollection,
    /// A path is absolute, escapes the collection, is hidden or is too long.
    BadPath,
    /// Something already exists at the destination.
    Exists,
    /// v1 required new collections inside a Git repository. No longer
    /// produced (D057: Git is optional); kept so the contract is unchanged.
    NotInRepository,
    /// The inventory is incomplete; fix its issues before changing files.
    Incomplete,
    /// The request is invalid, missing or shares its ID with another.
    Missing,
    /// A file exceeds its limit.
    TooLarge,
    /// The file could not be read or written.
    Io,
}

/// One discovered request (from its file's metadata).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RequestEntry {
    /// Collection-relative, `/`-separated.
    pub relative: String,
    pub id: String,
    /// Navigator text, abbreviated; an opened document reads its own
    /// metadata instead.
    pub method: String,
    pub url: String,
}

/// Why a candidate file was left out of the inventory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum IssueKind {
    /// A symbolic link (never followed).
    Link,
    /// The file could not be read, parsed or validated.
    Invalid,
    /// Its ID is also used by another request; every copy is left out.
    Duplicate,
    /// Its schema association names no readable schema file.
    Schema,
    /// The name is not valid UTF-8 or is too long.
    Name,
    /// Discovery stopped at a limit (requests, entries or depth).
    Limit,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub relative: String,
    pub kind: IssueKind,
}

/// A collection's read-only inventory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Catalog {
    pub root: PathBuf,
    pub id: String,
    pub name: String,
    pub requests: Vec<RequestEntry>,
    /// Folders (collection-relative), including empty ones.
    pub folders: Vec<String>,
    /// Shared environment names (`environments/<name>.toml`).
    pub environments: Vec<String>,
    /// Schema files (`.json`, `.yaml`, `.yml`) found anywhere.
    pub schemas: Vec<String>,
    pub issues: Vec<Issue>,
    /// False when any candidate could not be read or validated, or a limit
    /// was reached: no complete identity inventory may be assumed.
    pub complete: bool,
    /// Secret references named by the collection defaults, shared
    /// environments and request files (never values).
    pub references: std::collections::BTreeSet<String>,
    /// Every body file named by a parseable request file (duplicates and
    /// invalid requests included) and the request files naming it: the
    /// ownership evidence a body write is checked against.
    pub bodies: BTreeMap<String, std::collections::BTreeSet<String>>,
    /// Every schema association named by a parseable request file and the
    /// request files naming it.
    pub schema_links: BTreeMap<String, std::collections::BTreeSet<String>>,
}

pub(crate) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// A fresh 32-hex-digit identity (as v1's).
pub fn new_id() -> Option<String> {
    let mut entropy = [0u8; 16];
    getrandom::fill(&mut entropy).ok()?;
    Some(hex::encode(entropy))
}

/// A collection-relative path as written in project files: `/`-separated
/// normal components, no `..`, `.`, empty, backslash, NUL or `.git`
/// component. `creating` also refuses hidden (dot-prefixed) names.
pub fn relative_path(text: &str, creating: bool) -> Option<PathBuf> {
    if text.is_empty() || text.len() > MAX_PATH || text.contains(['\\', '\0']) {
        return None;
    }
    let mut path = PathBuf::new();
    for part in text.split('/') {
        if part.is_empty()
            || part == "."
            || part == ".."
            || part == ".git"
            || (creating && part.starts_with('.'))
            || part.len() > 255
        {
            return None;
        }
        path.push(part);
    }
    if path.components().all(|c| matches!(c, Component::Normal(_))) {
        Some(path)
    } else {
        None
    }
}

fn slash(path: &Path) -> Option<String> {
    let parts: Option<Vec<&str>> = path.components().map(|c| c.as_os_str().to_str()).collect();
    parts.map(|p| p.join("/"))
}

/// Where `relative` lives in `root`, refusing any existing component that
/// is a symbolic link, `.git` or a nested collection. The destination need
/// not exist.
pub fn destination(root: &Path, relative: &Path) -> Result<PathBuf, ProjectProblem> {
    let mut path = root.to_path_buf();
    let count = relative.components().count();
    for (index, part) in relative.components().enumerate() {
        let Component::Normal(name) = part else {
            return Err(ProjectProblem::BadPath);
        };
        if name == ".git" {
            return Err(ProjectProblem::BadPath);
        }
        path.push(name);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => return Err(ProjectProblem::BadPath),
            Ok(meta) if meta.is_dir() && index + 1 < count => {
                if path.join("collection.toml").exists() {
                    return Err(ProjectProblem::BadPath);
                }
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(ProjectProblem::Io),
        }
    }
    Ok(path)
}

/// Reads at most `limit` bytes of a regular file (`TooLarge` beyond).
pub fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, ProjectProblem> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| ProjectProblem::Io)?;
    if !meta.is_file() {
        return Err(ProjectProblem::Io);
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(limit as u64 + 1).read_to_end(&mut bytes))
        .map_err(|_| ProjectProblem::Io)?;
    if bytes.len() > limit {
        return Err(ProjectProblem::TooLarge);
    }
    Ok(bytes)
}

/// The bytes at `relative`, `None` when absent.
pub fn read_optional(
    root: &Path,
    relative: &Path,
    limit: usize,
) -> Result<Option<Vec<u8>>, ProjectProblem> {
    let path = destination(root, relative)?;
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(ProjectProblem::Io),
        Ok(_) => read_bounded(&path, limit).map(Some),
    }
}

pub(crate) fn parse_metadata(bytes: &[u8]) -> Option<DocumentMut> {
    std::str::from_utf8(bytes).ok()?.parse().ok()
}

pub(crate) fn format_one(doc: &DocumentMut) -> bool {
    doc.get("format_version").and_then(Item::as_integer) == Some(1)
}

pub(crate) fn string<'a>(doc: &'a DocumentMut, key: &str) -> Option<&'a str> {
    doc.get(key)
        .and_then(Item::as_str)
        .filter(|s| !s.is_empty())
}

/// Replaces `relative` atomically (temporary file, fsync, rename, fsync of
/// the folder), creating missing folders inside the collection. An existing
/// file keeps its permissions.
pub fn replace(root: &Path, relative: &Path, bytes: &[u8]) -> Result<(), ProjectProblem> {
    let path = destination(root, relative)?;
    let dir = path.parent().ok_or(ProjectProblem::BadPath)?;
    std::fs::create_dir_all(dir).map_err(|_| ProjectProblem::Io)?;
    // Folders created just now are checked again (no link raced in).
    destination(root, relative)?;
    let mut suffix = [0u8; 8];
    getrandom::fill(&mut suffix).map_err(|_| ProjectProblem::Io)?;
    let temp = dir.join(format!(".{}.wbtmp", hex::encode(suffix)));
    let written = (|| -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        if let Ok(meta) = std::fs::metadata(&path) {
            file.set_permissions(meta.permissions())?;
        }
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temp, &path)?;
        std::fs::File::open(dir)?.sync_all()
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    written.map_err(|_| ProjectProblem::Io)
}

/// Removes `relative` (absent is fine).
pub fn remove(root: &Path, relative: &Path) -> Result<(), ProjectProblem> {
    let path = destination(root, relative)?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(ProjectProblem::Io),
    }
}

/// Reads a collection's manifest: (id, name).
pub fn manifest(root: &Path) -> Result<(String, String), ProjectProblem> {
    let bytes = read_optional(root, Path::new("collection.toml"), METADATA_BYTES)
        .map_err(|_| ProjectProblem::NotCollection)?
        .ok_or(ProjectProblem::NotCollection)?;
    let doc = parse_metadata(&bytes).ok_or(ProjectProblem::NotCollection)?;
    if !format_one(&doc) {
        return Err(ProjectProblem::NotCollection);
    }
    let id = string(&doc, "id")
        .filter(|id| valid_id(id))
        .ok_or(ProjectProblem::NotCollection)?;
    let name = string(&doc, "name").unwrap_or(id);
    Ok((id.to_owned(), name.chars().take(128).collect()))
}

pub(crate) fn schema_file(name: &str) -> bool {
    [".json", ".yaml", ".yml"]
        .iter()
        .any(|ext| name.len() > ext.len() && name.ends_with(ext))
}

/// Opens a collection and discovers its inventory without writing.
pub fn discover(root: &Path) -> Result<Catalog, ProjectProblem> {
    let root = root
        .canonicalize()
        .map_err(|_| ProjectProblem::NotCollection)?;
    let (id, name) = manifest(&root)?;
    let mut catalog = Catalog {
        root,
        id,
        name,
        requests: Vec::new(),
        folders: Vec::new(),
        environments: Vec::new(),
        schemas: Vec::new(),
        issues: Vec::new(),
        complete: true,
        references: std::collections::BTreeSet::new(),
        bodies: BTreeMap::new(),
        schema_links: BTreeMap::new(),
    };
    if let Ok(Some(bytes)) =
        read_optional(&catalog.root, Path::new("collection.toml"), METADATA_BYTES)
        && let Some(doc) = parse_metadata(&bytes)
    {
        super::environments::references_in(&doc, &mut catalog.references);
    }
    let mut walk = Walk {
        identities: BTreeMap::new(),
        entries: 0,
    };
    scan(&mut catalog, &mut walk, Path::new(""), 0);
    for (id, paths) in walk.identities {
        if paths.len() > 1 {
            catalog.requests.retain(|r| r.id != id);
            for relative in paths {
                catalog.issues.push(Issue {
                    relative,
                    kind: IssueKind::Duplicate,
                });
            }
        }
    }
    catalog.requests.sort_by(|a, b| a.relative.cmp(&b.relative));
    catalog.folders.sort();
    catalog.schemas.sort();
    catalog.environments.sort();
    catalog.issues.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok(catalog)
}

struct Walk {
    identities: BTreeMap<String, Vec<String>>,
    entries: usize,
}

fn limit(catalog: &mut Catalog, relative: String) {
    catalog.complete = false;
    catalog.issues.push(Issue {
        relative,
        kind: IssueKind::Limit,
    });
}

fn scan(catalog: &mut Catalog, walk: &mut Walk, directory: &Path, depth: usize) {
    let Ok(read) = std::fs::read_dir(catalog.root.join(directory)) else {
        catalog.complete = false;
        catalog.issues.push(Issue {
            relative: slash(directory).unwrap_or_default(),
            kind: IssueKind::Invalid,
        });
        return;
    };
    // Enumeration stops one entry past the budget, so a huge directory
    // costs no more than the limit it trips; an unreadable entry makes the
    // catalog incomplete rather than silently smaller.
    let budget = MAX_ENTRIES.saturating_sub(walk.entries) + 1;
    let mut entries = Vec::new();
    let mut unreadable = false;
    for entry in read.take(budget) {
        match entry {
            Ok(e) => entries.push(e),
            Err(_) => unreadable = true,
        }
    }
    if unreadable {
        catalog.complete = false;
        catalog.issues.push(Issue {
            relative: slash(directory).unwrap_or_default(),
            kind: IssueKind::Invalid,
        });
    }
    entries.sort_by_key(|e| e.file_name());
    if let Some(folder) = slash(directory).filter(|f| !f.is_empty()) {
        catalog.folders.push(folder);
    }
    for entry in entries {
        walk.entries += 1;
        if walk.entries > MAX_ENTRIES {
            limit(catalog, slash(directory).unwrap_or_default());
            return;
        }
        let relative = directory.join(entry.file_name());
        let Some(text) = slash(&relative).filter(|t| t.len() <= MAX_PATH) else {
            catalog.complete = false;
            catalog.issues.push(Issue {
                relative: relative.to_string_lossy().chars().take(MAX_PATH).collect(),
                kind: IssueKind::Name,
            });
            continue;
        };
        let Ok(kind) = entry.file_type() else {
            catalog.complete = false;
            catalog.issues.push(Issue {
                relative: text,
                kind: IssueKind::Invalid,
            });
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if kind.is_symlink() {
            catalog.issues.push(Issue {
                relative: text,
                kind: IssueKind::Link,
            });
        } else if kind.is_dir() {
            if name == ".git" || entry.path().join("collection.toml").exists() {
                continue;
            }
            if depth + 1 > MAX_DEPTH {
                limit(catalog, text);
                continue;
            }
            scan(catalog, walk, &relative, depth + 1);
        } else if kind.is_file() && name.ends_with(REQUEST_SUFFIX) {
            if catalog.requests.len() >= MAX_REQUESTS {
                limit(catalog, text);
                continue;
            }
            request(catalog, walk, &relative, text);
        } else if kind.is_file() {
            if directory == Path::new("environments")
                && let Some(env) = name.strip_suffix(".toml")
                && super::environments::valid_name(env)
            {
                if let Some(doc) =
                    read_bounded(&entry.path(), super::environments::ENVIRONMENT_BYTES)
                        .ok()
                        .and_then(|b| parse_metadata(&b))
                {
                    super::environments::references_in(&doc, &mut catalog.references);
                }
                catalog.environments.push(env.to_owned());
            } else if schema_file(&name) {
                catalog.schemas.push(text);
            }
        }
    }
}

fn request(catalog: &mut Catalog, walk: &mut Walk, relative: &Path, text: String) {
    let invalid = |catalog: &mut Catalog, text: String| {
        catalog.complete = false;
        catalog.issues.push(Issue {
            relative: text,
            kind: IssueKind::Invalid,
        });
    };
    let Some(doc) = read_bounded(&catalog.root.join(relative), METADATA_BYTES)
        .ok()
        .and_then(|b| parse_metadata(&b))
    else {
        return invalid(catalog, text);
    };
    super::environments::references_in(&doc, &mut catalog.references);
    if let Some(body) = doc.get("body").and_then(Item::as_str) {
        catalog
            .bodies
            .entry(body.to_owned())
            .or_default()
            .insert(text.clone());
    }
    if let Some(schema) = doc.get("schema").and_then(Item::as_str) {
        catalog
            .schema_links
            .entry(schema.to_owned())
            .or_default()
            .insert(text.clone());
    }
    // Even an invalid request reserves its parseable identity, so a valid
    // copy cannot silently absorb it.
    if let Some(id) = string(&doc, "id") {
        walk.identities
            .entry(id.to_owned())
            .or_default()
            .push(text.clone());
    }
    let id = string(&doc, "id").filter(|id| valid_id(id));
    let method = string(&doc, "method");
    let url = string(&doc, "url");
    let (Some(id), Some(method), Some(url), true) = (id, method, url, format_one(&doc)) else {
        return invalid(catalog, text);
    };
    if let Some(body) = doc.get("body") {
        let ok = body
            .as_str()
            .and_then(|b| relative_path(b, false))
            .and_then(|b| destination(&catalog.root, &b).ok())
            .is_some_and(|p| std::fs::symlink_metadata(&p).is_ok_and(|m| m.is_file()));
        if !ok {
            return invalid(catalog, text);
        }
    }
    if let Some(schema) = doc.get("schema") {
        let ok = schema
            .as_str()
            .and_then(|s| relative_path(s, false))
            .and_then(|s| destination(&catalog.root, &s).ok())
            .is_some_and(|p| std::fs::symlink_metadata(&p).is_ok_and(|m| m.is_file()));
        if !ok {
            // A separately owned schema cannot obscure the request's
            // identity or block editing.
            catalog.issues.push(Issue {
                relative: text.clone(),
                kind: IssueKind::Schema,
            });
        }
    }
    catalog.requests.push(RequestEntry {
        relative: text,
        id: id.to_owned(),
        method: method.chars().take(32).collect(),
        url: url.chars().take(2048).collect(),
    });
}

/// Whether `path` is an existing folder holding nothing but Finder's
/// `.DS_Store`: a folder that may become a collection in place.
pub fn empty_folder(path: &Path) -> bool {
    std::fs::read_dir(path)
        .is_ok_and(|mut entries| entries.all(|e| e.is_ok_and(|e| e.file_name() == ".DS_Store")))
}

/// Creates a new collection at `path` with a fresh identity and its folder
/// name: a new folder, or an existing empty one (see [`empty_folder`])
/// initialised in place. Git is optional (D057). Anything else already at
/// `path` is refused (`Exists`).
pub fn create_collection(path: &Path) -> Result<String, ProjectProblem> {
    if path.exists() {
        if !empty_folder(path) {
            return Err(ProjectProblem::Exists);
        }
        let root = path.canonicalize().map_err(|_| ProjectProblem::Io)?;
        let name = folder_name(&root)?;
        return init_collection(&root, &name);
    }
    path.parent()
        .and_then(|p| p.canonicalize().ok())
        .ok_or(ProjectProblem::BadPath)?;
    let name = folder_name(path)?;
    std::fs::create_dir(path).map_err(|_| ProjectProblem::Io)?;
    let root = path.canonicalize().map_err(|_| ProjectProblem::Io)?;
    init_collection(&root, &name).inspect_err(|_| {
        // Only the folder made just now is removed (and only while empty).
        let _ = std::fs::remove_dir(&root);
    })
}

/// A collection's name from its folder (visible, non-empty, UTF-8).
fn folder_name(path: &Path) -> Result<String, ProjectProblem> {
    path.file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.starts_with('.') && !n.is_empty())
        .map(str::to_owned)
        .ok_or(ProjectProblem::BadPath)
}

/// Writes a new `collection.toml` (format 1, fresh identity, `name`) into
/// the existing folder `root`.
fn init_collection(root: &Path, name: &str) -> Result<String, ProjectProblem> {
    let id = new_id().ok_or(ProjectProblem::Io)?;
    let mut doc = DocumentMut::new();
    doc["format_version"] = toml_edit::value(1);
    doc["id"] = toml_edit::value(id.as_str());
    doc["name"] = toml_edit::value(name);
    replace(
        root,
        Path::new("collection.toml"),
        doc.to_string().as_bytes(),
    )?;
    Ok(id)
}

/// Creates an (empty) folder inside the collection.
pub fn create_folder(catalog: &Catalog, relative: &str) -> Result<(), ProjectProblem> {
    if !catalog.complete {
        return Err(ProjectProblem::Incomplete);
    }
    let rel = relative_path(relative, true).ok_or(ProjectProblem::BadPath)?;
    let path = destination(&catalog.root, &rel)?;
    if path.exists() {
        return Err(ProjectProblem::Exists);
    }
    std::fs::create_dir_all(&path).map_err(|_| ProjectProblem::Io)?;
    destination(&catalog.root, &rel).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workbench::testdir::Dir;

    fn collection() -> (Dir, PathBuf) {
        let dir = Dir::new();
        let root = dir.path().join("c");
        std::fs::create_dir(&root).expect("fixture");
        std::fs::write(
            root.join("collection.toml"),
            "format_version = 1\nid = \"coll\"\nname = \"Coll\"\n",
        )
        .expect("fixture");
        (dir, root)
    }

    #[test]
    fn paths_stay_inside_and_visible() {
        assert!(relative_path("a/b.request.toml", true).is_some());
        for bad in [
            "", "/a", "../a", "a/../b", "a//b", ".hidden", "a\\b", ".git/x",
        ] {
            assert!(relative_path(bad, true).is_none(), "{bad}");
        }
        assert!(relative_path(".hidden/x", false).is_some());
        // Reading allows hidden names, so `..` and `.` must be refused on
        // their own there.
        for bad in ["../a", "a/../b", "a/..", "./a", "a/./b"] {
            assert!(relative_path(bad, false).is_none(), "{bad}");
        }
        assert!(relative_path(&"a".repeat(MAX_PATH + 1), false).is_none());
    }

    #[test]
    fn discovery_reports_duplicates_links_and_invalid_files() {
        let (_d, root) = collection();
        let req = |id: &str| {
            format!(
                "format_version = 1\nid = \"{id}\"\nmethod = \"GET\"\nurl = \"https://example.com/\"\n"
            )
        };
        std::fs::create_dir(root.join("f")).expect("fixture");
        std::fs::write(root.join("a.request.toml"), req("one")).expect("fixture");
        std::fs::write(root.join("f/b.request.toml"), req("one")).expect("fixture");
        std::fs::write(root.join("f/c.request.toml"), req("two")).expect("fixture");
        std::fs::write(root.join("bad.request.toml"), "not = [toml").expect("fixture");
        std::os::unix::fs::symlink(root.join("a.request.toml"), root.join("l.request.toml"))
            .expect("fixture");
        std::fs::create_dir(root.join(".git")).expect("fixture");
        std::fs::write(root.join(".git/x.request.toml"), req("three")).expect("fixture");
        std::fs::create_dir_all(root.join("nested")).expect("fixture");
        std::fs::write(
            root.join("nested/collection.toml"),
            "format_version = 1\nid = \"n\"\n",
        )
        .expect("fixture");
        std::fs::write(root.join("nested/n.request.toml"), req("four")).expect("fixture");
        std::fs::create_dir(root.join("environments")).expect("fixture");
        std::fs::write(root.join("environments/dev.toml"), "format_version = 1\n")
            .expect("fixture");
        std::fs::write(root.join("f/api.yaml"), "openapi: 3.1.0\n").expect("fixture");
        let catalog = discover(&root).expect("fixture");
        assert_eq!(catalog.id, "coll");
        assert_eq!(
            catalog
                .requests
                .iter()
                .map(|r| r.relative.as_str())
                .collect::<Vec<_>>(),
            ["f/c.request.toml"]
        );
        assert!(!catalog.complete);
        let kinds: Vec<_> = catalog
            .issues
            .iter()
            .map(|i| (i.relative.as_str(), i.kind))
            .collect();
        assert!(kinds.contains(&("a.request.toml", IssueKind::Duplicate)));
        assert!(kinds.contains(&("f/b.request.toml", IssueKind::Duplicate)));
        assert!(kinds.contains(&("bad.request.toml", IssueKind::Invalid)));
        assert!(kinds.contains(&("l.request.toml", IssueKind::Link)));
        assert_eq!(catalog.environments, ["dev"]);
        assert_eq!(catalog.schemas, ["f/api.yaml"]);
        assert_eq!(catalog.folders, ["environments", "f"]);
    }

    #[test]
    fn a_directory_past_the_entry_budget_makes_the_catalog_incomplete() {
        let (_d, root) = collection();
        let big = root.join("big");
        std::fs::create_dir(&big).expect("fixture");
        for i in 0..MAX_ENTRIES + 10 {
            std::fs::write(big.join(format!("{i}.txt")), "").expect("fixture");
        }
        let catalog = discover(&root).expect("fixture");
        assert!(!catalog.complete);
        assert!(
            catalog
                .issues
                .iter()
                .any(|i| i.relative == "big" && i.kind == IssueKind::Limit)
        );
    }

    #[test]
    fn writes_refuse_links_git_and_nested_collections() {
        let (d, root) = collection();
        let outside = d.path().join("outside");
        std::fs::create_dir(&outside).expect("fixture");
        std::os::unix::fs::symlink(&outside, root.join("link")).expect("fixture");
        assert_eq!(
            replace(&root, Path::new("link/x.txt"), b"x"),
            Err(ProjectProblem::BadPath)
        );
        assert!(!outside.join("x.txt").exists());
        std::fs::create_dir_all(root.join("n")).expect("fixture");
        std::fs::write(root.join("n/collection.toml"), "").expect("fixture");
        assert_eq!(
            replace(&root, Path::new("n/x.txt"), b"x"),
            Err(ProjectProblem::BadPath)
        );
        replace(&root, Path::new("new/deep/x.txt"), b"x").expect("fixture");
        assert_eq!(
            std::fs::read(root.join("new/deep/x.txt")).expect("fixture"),
            b"x"
        );
    }

    #[test]
    fn collections_are_created_outside_repositories_too() {
        let d = Dir::new();
        assert!(!d.path().ancestors().any(|p| p.join(".git").exists()));
        let id = create_collection(&d.path().join("c")).expect("fixture");
        assert_eq!(
            manifest(&d.path().join("c")).expect("fixture"),
            (id, "c".to_owned())
        );
        // Inside a repository works as before.
        std::fs::create_dir(d.path().join("repo")).expect("fixture");
        std::fs::create_dir(d.path().join("repo/.git")).expect("fixture");
        let id = create_collection(&d.path().join("repo/api")).expect("fixture");
        assert_eq!(
            manifest(&d.path().join("repo/api")).expect("fixture"),
            (id, "api".to_owned())
        );
    }

    #[test]
    fn an_existing_empty_folder_becomes_a_collection_in_place() {
        let d = Dir::new();
        let empty = d.path().join("Empty API");
        std::fs::create_dir(&empty).expect("fixture");
        std::fs::write(empty.join(".DS_Store"), b"finder").expect("fixture");
        assert!(empty_folder(&empty));
        let id = create_collection(&empty).expect("fixture");
        assert_eq!(
            manifest(&empty).expect("fixture"),
            (id, "Empty API".to_owned())
        );
        assert_eq!(
            std::fs::read(empty.join(".DS_Store")).expect("fixture"),
            b"finder"
        );
        assert!(discover(&empty).expect("fixture").complete);
    }

    #[test]
    fn anything_else_already_there_is_refused_and_left_alone() {
        let (d, root) = collection();
        let before = std::fs::read(root.join("collection.toml")).expect("fixture");
        assert!(!empty_folder(&root));
        assert_eq!(create_collection(&root), Err(ProjectProblem::Exists));
        assert_eq!(
            std::fs::read(root.join("collection.toml")).expect("fixture"),
            before
        );
        let full = d.path().join("notes");
        std::fs::create_dir(&full).expect("fixture");
        std::fs::write(full.join("readme.txt"), "x").expect("fixture");
        assert!(!empty_folder(&full));
        assert_eq!(create_collection(&full), Err(ProjectProblem::Exists));
        assert!(!full.join("collection.toml").exists());
        let hidden = d.path().join("hidden");
        std::fs::create_dir(&hidden).expect("fixture");
        std::fs::create_dir(hidden.join(".git")).expect("fixture");
        assert_eq!(create_collection(&hidden), Err(ProjectProblem::Exists));
        let file = d.path().join("file.txt");
        std::fs::write(&file, "x").expect("fixture");
        assert!(!empty_folder(&file));
        assert_eq!(create_collection(&file), Err(ProjectProblem::Exists));
        assert_eq!(
            create_collection(&d.path().join("missing/c")),
            Err(ProjectProblem::BadPath)
        );
    }
}
