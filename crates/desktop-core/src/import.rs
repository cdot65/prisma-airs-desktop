//! Explicit, reviewed import of nonsecret tenant-profile metadata.
//!
//! Two source formats are recognized: the v2 profile document and the
//! airs-cli tenant registry (`{version, active?, tenants:[{name, configPath,
//! tsgId}]}`) that v1 reads. Only label, TSG identifier and destination are
//! taken (the registry has no destination, so its candidates use the default
//! region and say so). Every other field is discarded and counted; secret-like
//! fields and credential-file references are counted separately, and a
//! referenced file is never opened. The preview carries only nonsecret values;
//! the raw bytes are zeroized after parsing. Import never changes an existing
//! tenant and never brings a credential: each imported tenant needs a fresh
//! scan key.

use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;
use zeroize::Zeroizing;

use crate::tenants::{Destination, ProfileDraft, Region};

/// Largest import file read.
pub const MAX_IMPORT_BYTES: usize = 1024 * 1024;
/// Most candidates in one preview.
pub const MAX_CANDIDATES: usize = 256;
/// Schema of the v2 profile document.
pub const PROFILE_DOCUMENT_SCHEMA: &str = "prisma-airs-desktop-v2/tenant-profiles/1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ImportFormat {
    ProfileDocument,
    AirsCliRegistry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum CandidateProblem {
    InvalidLabel,
    InvalidTsgId,
    InvalidDestination,
}

/// One importable profile, as shown for review.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ImportCandidate {
    pub index: u32,
    pub label: String,
    pub tsg_id: Option<String>,
    pub destination: Option<Destination>,
    /// True when the source named no destination and the default is used.
    pub default_destination: bool,
    pub problem: Option<CandidateProblem>,
    /// An existing tenant already has this label (it is not changed).
    pub existing_label: bool,
}

/// Counts of source content that was not imported.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Discarded {
    /// Fields that look like secrets (keys, secrets, tokens, passwords).
    pub secret_fields: u32,
    /// References to other files (for example credential `configPath`s).
    pub file_references: u32,
    pub other_fields: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportPreview {
    pub format: ImportFormat,
    pub candidates: Vec<ImportCandidate>,
    pub discarded: Discarded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS, thiserror::Error)]
#[serde(rename_all = "camelCase")]
pub enum ImportError {
    #[error("the file is too large to import")]
    TooLarge,
    #[error("the file is not a supported profile format")]
    Unsupported,
    #[error("the file contains no profiles")]
    Empty,
}

fn is_secret_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [
        "key",
        "secret",
        "token",
        "password",
        "passwd",
        "credential",
        "auth",
    ]
    .iter()
    .any(|w| n.contains(w))
}

fn is_reference_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.ends_with("path") || n.ends_with("file") || n.ends_with("dir")
}

fn classify(name: &str, discarded: &mut Discarded) {
    if is_secret_name(name) {
        discarded.secret_fields += 1;
    } else if is_reference_name(name) {
        discarded.file_references += 1;
    } else {
        discarded.other_fields += 1;
    }
}

fn text(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_owned)
}

/// Parses an import file into a reviewable preview. `existing` are the
/// current tenant labels (for the duplicate-label flag).
pub fn preview(
    bytes: Zeroizing<Vec<u8>>,
    existing: &[String],
) -> Result<ImportPreview, ImportError> {
    if bytes.len() > MAX_IMPORT_BYTES {
        return Err(ImportError::TooLarge);
    }
    let root: Value = serde_json::from_slice(&bytes).map_err(|_| ImportError::Unsupported)?;
    drop(bytes);
    let Value::Object(root) = root else {
        return Err(ImportError::Unsupported);
    };
    let mut discarded = Discarded::default();
    let (format, entries) =
        if root.get("schema").and_then(Value::as_str) == Some(PROFILE_DOCUMENT_SCHEMA) {
            for name in root
                .keys()
                .filter(|k| !matches!(k.as_str(), "schema" | "profiles"))
            {
                classify(name, &mut discarded);
            }
            (ImportFormat::ProfileDocument, root.get("profiles"))
        } else if root.get("version").and_then(Value::as_u64) == Some(1)
            && root.get("tenants").is_some_and(Value::is_array)
        {
            for name in root
                .keys()
                .filter(|k| !matches!(k.as_str(), "version" | "tenants" | "active"))
            {
                classify(name, &mut discarded);
            }
            (ImportFormat::AirsCliRegistry, root.get("tenants"))
        } else {
            return Err(ImportError::Unsupported);
        };
    let Some(Value::Array(entries)) = entries else {
        return Err(ImportError::Unsupported);
    };
    if entries.is_empty() {
        return Err(ImportError::Empty);
    }
    if entries.len() > MAX_CANDIDATES {
        return Err(ImportError::TooLarge);
    }
    let mut candidates = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let Value::Object(entry) = entry else {
            discarded.other_fields += 1;
            continue;
        };
        let (label_key, kept): (&str, &[&str]) = match format {
            ImportFormat::ProfileDocument => ("label", &["label", "tsgId", "destination"]),
            ImportFormat::AirsCliRegistry => ("name", &["name", "tsgId"]),
        };
        for name in entry.keys().filter(|k| !kept.contains(&k.as_str())) {
            classify(name, &mut discarded);
        }
        let label = text(entry.get(label_key)).unwrap_or_default();
        let tsg_id = text(entry.get("tsgId"));
        let (destination, default_destination) = match format {
            ImportFormat::ProfileDocument => (
                entry
                    .get("destination")
                    .and_then(|d| serde_json::from_value::<Destination>(d.clone()).ok()),
                false,
            ),
            ImportFormat::AirsCliRegistry => (
                Some(Destination::Region {
                    region: Region::UnitedStates,
                }),
                true,
            ),
        };
        let draft = destination.clone().map(|destination| ProfileDraft {
            label: label.clone(),
            tsg_id: tsg_id.clone(),
            destination,
            environment: Default::default(),
        });
        let (shown, problem) = match draft.clone().and_then(ProfileDraft::validate) {
            Some(valid) => (valid, None),
            None => {
                let problem = if destination.and_then(Destination::validate).is_none() {
                    CandidateProblem::InvalidDestination
                } else if (ProfileDraft {
                    label: label.clone(),
                    tsg_id: None,
                    destination: Destination::Region {
                        region: Region::UnitedStates,
                    },
                    environment: Default::default(),
                })
                .validate()
                .is_none()
                {
                    CandidateProblem::InvalidLabel
                } else {
                    CandidateProblem::InvalidTsgId
                };
                (
                    ProfileDraft {
                        label: label
                            .chars()
                            .take(crate::tenants::MAX_LABEL_CHARS)
                            .collect(),
                        tsg_id: None,
                        destination: Destination::Region {
                            region: Region::UnitedStates,
                        },
                        environment: Default::default(),
                    },
                    Some(problem),
                )
            }
        };
        candidates.push(ImportCandidate {
            index: index as u32,
            existing_label: existing.iter().any(|l| l == &shown.label),
            label: shown.label,
            tsg_id: if problem.is_none() {
                shown.tsg_id
            } else {
                None
            },
            destination: if problem.is_none() {
                Some(shown.destination)
            } else {
                None
            },
            default_destination,
            problem,
        });
    }
    if candidates.is_empty() {
        return Err(ImportError::Empty);
    }
    Ok(ImportPreview {
        format,
        candidates,
        discarded,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(value: serde_json::Value) -> Zeroizing<Vec<u8>> {
        Zeroizing::new(serde_json::to_vec(&value).expect("json"))
    }

    #[test]
    fn airs_cli_registry_imports_names_and_tsg_ids_only() {
        let source = serde_json::json!({
            "version": 1,
            "active": "prod",
            "tenants": [
                {"name": "prod", "configPath": "/Users/x/.config/airs/prod.json", "tsgId": "1234567890"},
                {"name": "lab", "configPath": "/tmp/lab.json", "tsgId": "42", "airsApiKey": "synthetic-secret"}
            ]
        });
        let preview = preview(bytes(source), &["lab".to_owned()]).expect("preview");
        assert_eq!(preview.format, ImportFormat::AirsCliRegistry);
        assert_eq!(preview.candidates.len(), 2);
        assert_eq!(preview.candidates[0].label, "prod");
        assert_eq!(preview.candidates[0].tsg_id.as_deref(), Some("1234567890"));
        assert!(preview.candidates[0].default_destination);
        assert!(preview.candidates[1].existing_label);
        assert_eq!(
            preview.discarded,
            Discarded {
                secret_fields: 1,
                file_references: 2,
                other_fields: 0
            }
        );
        let shown = format!("{:?}", preview.candidates);
        assert!(!shown.contains("synthetic-secret") && !shown.contains("/tmp/lab.json"));
    }

    #[test]
    fn profile_documents_keep_whitelisted_fields_and_flag_invalid_entries() {
        let source = serde_json::json!({
            "schema": PROFILE_DOCUMENT_SCHEMA,
            "exportedBy": "someone",
            "profiles": [
                {"label": "EU", "tsgId": "77", "destination": {"kind": "region", "region": "europe"}, "clientSecret": "s"},
                {"label": "Custom", "destination": {"kind": "custom", "origin": "https://scan.example.com/"}},
                {"label": "Bad", "destination": {"kind": "custom", "origin": "http://plain.example.com"}},
                {"label": "", "destination": {"kind": "region", "region": "india"}}
            ]
        });
        let preview = preview(bytes(source), &[]).expect("preview");
        let c = &preview.candidates;
        assert_eq!(
            c[0].destination,
            Some(Destination::Region {
                region: Region::Europe
            })
        );
        assert_eq!(
            c[1].destination,
            Some(Destination::Custom {
                origin: "https://scan.example.com".into()
            })
        );
        assert_eq!(c[2].problem, Some(CandidateProblem::InvalidDestination));
        assert_eq!(c[3].problem, Some(CandidateProblem::InvalidLabel));
        assert_eq!(preview.discarded.secret_fields, 1);
        assert_eq!(preview.discarded.other_fields, 1);
    }

    #[test]
    fn unsupported_oversized_and_empty_sources_are_refused() {
        assert_eq!(
            preview(Zeroizing::new(b"not json".to_vec()), &[]).err(),
            Some(ImportError::Unsupported)
        );
        assert_eq!(
            preview(bytes(serde_json::json!([1, 2])), &[]).err(),
            Some(ImportError::Unsupported)
        );
        assert_eq!(
            preview(bytes(serde_json::json!({"version": 1, "tenants": []})), &[]).err(),
            Some(ImportError::Empty)
        );
        assert_eq!(
            preview(Zeroizing::new(vec![b' '; MAX_IMPORT_BYTES + 1]), &[]).err(),
            Some(ImportError::TooLarge)
        );
    }
}
