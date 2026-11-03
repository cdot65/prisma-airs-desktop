//! Schema files (C14, v1): JSON or YAML text in the collection, edited and
//! saved as text (no validation or request generation) and associated with
//! requests by reference. A schema is separately owned: a request Save never
//! rewrites it, and its own Save checks its revision like a request's.

use std::path::Path;

use serde::Serialize;
use ts_rs::TS;

use super::document::{hash_bytes, hash_file};
use super::project::{self, ProjectProblem};

pub const SCHEMA_BYTES: usize = 4 * 1024 * 1024;

/// A schema as read: its text and the revision naming those bytes
/// (`None`: a new file).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SchemaText {
    pub relative: String,
    pub text: String,
    pub revision: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum SchemaProblem {
    /// Not a `.json`, `.yaml` or `.yml` path inside the collection.
    BadPath,
    Missing,
    /// Larger than 4 MiB or not UTF-8 text.
    NotText,
    /// The file changed on disk since it was read (or now exists).
    Conflict,
    Io,
}

fn path(relative: &str, creating: bool) -> Result<std::path::PathBuf, SchemaProblem> {
    let name = relative.rsplit('/').next().unwrap_or_default();
    if !project::schema_file(name) {
        return Err(SchemaProblem::BadPath);
    }
    project::relative_path(relative, creating).ok_or(SchemaProblem::BadPath)
}

pub fn open(root: &Path, relative: &str) -> Result<SchemaText, SchemaProblem> {
    let rel = path(relative, false)?;
    let bytes = match project::read_optional(root, &rel, SCHEMA_BYTES) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Err(SchemaProblem::Missing),
        Err(ProjectProblem::TooLarge) => return Err(SchemaProblem::NotText),
        Err(ProjectProblem::BadPath) => return Err(SchemaProblem::BadPath),
        Err(_) => return Err(SchemaProblem::Io),
    };
    let revision = Some(hash_bytes(&bytes));
    let text = String::from_utf8(bytes).map_err(|_| SchemaProblem::NotText)?;
    Ok(SchemaText {
        relative: relative.to_owned(),
        text,
        revision,
    })
}

/// Writes `text` if the file still holds `revision` (`None`: must not
/// exist yet); returns the new revision.
pub fn save(
    root: &Path,
    relative: &str,
    revision: Option<&str>,
    text: &str,
) -> Result<String, SchemaProblem> {
    let rel = path(relative, revision.is_none())?;
    if text.len() > SCHEMA_BYTES {
        return Err(SchemaProblem::NotText);
    }
    let current = hash_file(root, relative).map_err(|e| match e {
        ProjectProblem::BadPath => SchemaProblem::BadPath,
        _ => SchemaProblem::Io,
    })?;
    if current.as_deref() != revision {
        return Err(SchemaProblem::Conflict);
    }
    project::replace(root, &rel, text.as_bytes()).map_err(|e| match e {
        ProjectProblem::BadPath => SchemaProblem::BadPath,
        _ => SchemaProblem::Io,
    })?;
    Ok(hash_bytes(text.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workbench::testdir::Dir;

    #[test]
    fn schemas_save_only_over_the_revision_they_were_read_at() {
        let d = Dir::new();
        let first = save(d.path(), "schemas/api.yaml", None, "openapi: 3.1.0\n").expect("fixture");
        assert_eq!(
            save(d.path(), "schemas/api.yaml", None, "x"),
            Err(SchemaProblem::Conflict)
        );
        let read = open(d.path(), "schemas/api.yaml").expect("fixture");
        assert_eq!(read.revision.as_deref(), Some(first.as_str()));
        std::fs::write(d.path().join("schemas/api.yaml"), "changed elsewhere").expect("fixture");
        assert_eq!(
            save(d.path(), "schemas/api.yaml", Some(&first), "mine"),
            Err(SchemaProblem::Conflict)
        );
        assert_eq!(
            std::fs::read_to_string(d.path().join("schemas/api.yaml")).expect("fixture"),
            "changed elsewhere"
        );
        assert_eq!(
            save(d.path(), "../x.yaml", None, "x"),
            Err(SchemaProblem::BadPath)
        );
        assert_eq!(
            save(d.path(), "a.txt", None, "x"),
            Err(SchemaProblem::BadPath)
        );
        assert_eq!(open(d.path(), "none.json"), Err(SchemaProblem::Missing));
    }
}
