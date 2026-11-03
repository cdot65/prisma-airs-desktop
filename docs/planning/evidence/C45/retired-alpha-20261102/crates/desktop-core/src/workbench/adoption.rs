//! Profile adoption (C14): an explicit, previewed import of the v1 API
//! Client profile's recent collections and their private environments into
//! this desktop's sealed profile. The source is only read (bounded, no
//! symbolic links) and never changed. Secret values, drafts, history and
//! cookies do not transfer (D015): references keep their names and the
//! operator stores values again here. A private environment that already
//! exists here is never replaced.

use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};
use ts_rs::TS;

use super::environments;
use super::project;

pub const RECENT_BYTES: usize = 64 * 1024;
pub const MAX_CANDIDATES: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum AdoptionProblem {
    /// No v1 API Client profile was found.
    Missing,
    /// It could not be read (or is a link).
    Unreadable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub root: PathBuf,
    pub id: String,
    pub name: String,
    /// Private environments (name, text) that passed validation.
    pub environments: Vec<(String, String)>,
    /// Private environments left out (invalid or oversized).
    pub skipped: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CandidateView {
    pub index: u32,
    pub name: String,
    /// The collection folder's own name (not its full path).
    pub folder: String,
    pub environments: Vec<String>,
    pub skipped: u32,
}

impl Candidate {
    pub fn view(&self, index: usize) -> CandidateView {
        CandidateView {
            index: u32::try_from(index).unwrap_or(u32::MAX),
            name: self.name.clone(),
            folder: self
                .root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            environments: self.environments.iter().map(|(n, _)| n.clone()).collect(),
            skipped: self.skipped,
        }
    }
}

/// v1's private scope: the full SHA-256 of root and identity.
fn v1_scope(root: &Path, id: &str) -> String {
    hex::encode(Sha256::digest(
        format!("{}\0{id}", root.display()).as_bytes(),
    ))
}

fn regular(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file())
}

/// Reads the source profile's adoptable state (read-only).
pub fn read(source: &Path) -> Result<Vec<Candidate>, AdoptionProblem> {
    match std::fs::symlink_metadata(source) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => return Err(AdoptionProblem::Unreadable),
        Err(_) => return Err(AdoptionProblem::Missing),
    }
    let recent = source.join("recent-collections.json");
    let roots: Vec<PathBuf> = if regular(&recent) {
        let bytes = project::read_bounded(&recent, RECENT_BYTES)
            .map_err(|_| AdoptionProblem::Unreadable)?;
        serde_json::from_slice(&bytes).map_err(|_| AdoptionProblem::Unreadable)?
    } else {
        Vec::new()
    };
    let mut out = Vec::new();
    for root in roots.into_iter().take(MAX_CANDIDATES) {
        let Ok(root) = root.canonicalize() else {
            continue;
        };
        let Ok((id, name)) = project::manifest(&root) else {
            continue;
        };
        let mut candidate = Candidate {
            environments: Vec::new(),
            skipped: 0,
            root,
            id,
            name,
        };
        let dir = source
            .join("environments")
            .join(v1_scope(&candidate.root, &candidate.id));
        if std::fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir())
            && let Ok(entries) = std::fs::read_dir(&dir)
        {
            let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
            entries.sort_by_key(|e| e.file_name());
            for entry in entries.into_iter().take(environments::MAX_PRIVATE) {
                let file = entry.file_name().to_string_lossy().into_owned();
                let Some(name) = file.strip_suffix(".toml") else {
                    continue;
                };
                let text = regular(&entry.path())
                    .then(|| {
                        project::read_bounded(&entry.path(), environments::ENVIRONMENT_BYTES).ok()
                    })
                    .flatten()
                    .and_then(|b| String::from_utf8(b).ok())
                    .filter(|t| environments::text_ok(t).is_some());
                match text {
                    Some(text)
                        if environments::valid_name(name) || name == environments::DEFAULTS =>
                    {
                        candidate.environments.push((name.to_owned(), text));
                    }
                    _ => candidate.skipped += 1,
                }
            }
        }
        out.push(candidate);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workbench::testdir::Dir;

    fn tree_hash(root: &Path) -> String {
        let mut paths = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).expect("fixture") {
                let p = e.expect("fixture").path();
                if p.is_dir() {
                    stack.push(p)
                } else {
                    paths.push(p)
                }
            }
        }
        paths.sort();
        let mut h = Sha256::new();
        for p in paths {
            h.update(p.to_string_lossy().as_bytes());
            h.update(std::fs::read(&p).expect("fixture"));
        }
        hex::encode(h.finalize())
    }

    #[test]
    fn reads_recent_collections_and_private_environments_only() {
        let d = Dir::new();
        let collection = d.path().join("work/c");
        std::fs::create_dir_all(&collection).expect("fixture");
        std::fs::write(
            collection.join("collection.toml"),
            "format_version = 1\nid = \"c1\"\nname = \"Work\"\n",
        )
        .expect("fixture");
        let source = d.path().join("api-client");
        let scope = v1_scope(&collection, "c1");
        std::fs::create_dir_all(source.join("environments").join(&scope)).expect("fixture");
        std::fs::write(
            source.join("recent-collections.json"),
            serde_json::to_vec(&[collection.clone(), d.path().join("gone")]).expect("fixture"),
        )
        .expect("fixture");
        let envs = source.join("environments").join(&scope);
        std::fs::write(
            envs.join("dev.toml"),
            "format_version = 1\n[variables]\nhost = \"h\"\nk = { secret = \"r\" }\n",
        )
        .expect("fixture");
        std::fs::write(envs.join("bad.toml"), "format_version = 1\n[auth]\n").expect("fixture");
        std::fs::create_dir_all(source.join("drafts")).expect("fixture");
        std::fs::write(source.join("drafts/x.json"), "{}").expect("fixture");
        let before = tree_hash(&source);
        let found = read(&source).expect("fixture");
        assert_eq!(tree_hash(&source), before);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Work");
        assert_eq!(found[0].environments.len(), 1);
        assert_eq!(found[0].skipped, 1);
        assert_eq!(found[0].view(0).folder, "c");
        assert_eq!(read(&d.path().join("none")), Err(AdoptionProblem::Missing));
    }
}
