//! Native acceptance records (T09 and chunk-specific native assertions).
//!
//! `native-prepare` builds and launches the fixture app and records its
//! executable hash. A separate acceptance record, produced by actual
//! computer-use interaction or a recorded operator procedure, must name that
//! exact hash and report every check. A build exit status is never
//! acceptance.

use serde::{Deserialize, Serialize};

use crate::{Result, invalid};

pub const NATIVE_SCHEMA: &str = "airs-v2/native-acceptance/1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Method {
    /// Computer-use automation driving the real native window.
    Cua,
    /// A recorded manual operator procedure.
    Operator,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckResult {
    Pass,
    Fail,
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeApp {
    pub bundle: String,
    pub identifier: String,
    pub executable_sha256: String,
    pub fixture: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeCheck {
    pub id: String,
    pub description: String,
    pub result: CheckResult,
    pub observation: String,
    pub evidence: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeAcceptance {
    pub schema: String,
    pub subject: String,
    pub recorded_utc: String,
    pub method: Method,
    pub performed_by: String,
    pub host: String,
    pub app: NativeApp,
    pub checks: Vec<NativeCheck>,
    pub limitations: Vec<String>,
}

impl NativeAcceptance {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let record: Self = serde_json::from_slice(bytes)
            .map_err(|e| invalid(format!("native acceptance record is malformed: {e}")))?;
        if record.schema != NATIVE_SCHEMA {
            return Err(invalid("native acceptance record has the wrong schema"));
        }
        Ok(record)
    }

    /// Reasons this record cannot count as native acceptance under `expect`.
    pub fn failures(&self, subject: &str, expect: &NativeExpectation) -> Vec<String> {
        let mut reasons = Vec::new();
        if self.subject != subject {
            reasons.push(format!(
                "native record is for {} not {subject}",
                self.subject
            ));
        }
        if self.app.executable_sha256 != expect.prepared_executable_sha256 {
            reasons.push("native record does not name the prepared fixture executable".to_owned());
        }
        match expect.current_executable_sha256.as_deref() {
            Some(current) if current == expect.prepared_executable_sha256 => {}
            Some(_) => reasons
                .push("the prepared fixture bytes changed after native acceptance".to_owned()),
            None => reasons.push("the prepared fixture executable is no longer present".to_owned()),
        }
        if expect.prepared_bundle.as_deref() != Some(self.app.bundle.as_str()) {
            reasons.push("native record names a bundle other than the prepared fixture".to_owned());
        }
        match (
            &expect.prepared_bundle_sha256,
            &expect.current_bundle_sha256,
        ) {
            (Some(prepared), Some(current)) if prepared == current => {}
            _ => reasons.push(
                "the prepared fixture bundle changed or is missing (any bundle byte)".to_owned(),
            ),
        }
        if expect.prepared_identifier.as_deref() != Some(expect.fixture_identifier.as_str())
            || expect.current_identifier.as_deref() != Some(expect.fixture_identifier.as_str())
        {
            reasons.push(
                "the prepared bundle's CFBundleIdentifier is not the fixture identity".to_owned(),
            );
        }
        if !self.app.fixture || self.app.identifier != expect.fixture_identifier {
            reasons.push(format!(
                "native acceptance must use the fixture identity {} (record names {})",
                expect.fixture_identifier, self.app.identifier
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        for check in &self.checks {
            if !seen.insert(check.id.as_str()) {
                reasons.push(format!("native check {} appears more than once", check.id));
            }
            if !expect.required_checks.contains(&check.id) {
                reasons.push(format!(
                    "native check {} is not in the required inventory",
                    check.id
                ));
            }
            if check.result != CheckResult::Pass {
                reasons.push(format!("native check {} is {:?}", check.id, check.result));
            }
            if check.observation.trim().is_empty() {
                reasons.push(format!("native check {} has no observation", check.id));
            }
            if check.evidence.is_empty() {
                reasons.push(format!("native check {} cites no evidence", check.id));
            }
            for path in &check.evidence {
                if !expect.evidence_files.contains(path) {
                    reasons.push(format!(
                        "native check {} cites {path}, which the packet does not hash-bind",
                        check.id
                    ));
                }
            }
        }
        for required in &expect.required_checks {
            if !seen.contains(required.as_str()) {
                reasons.push(format!("required native check {required} is missing"));
            }
        }
        reasons
    }
}

/// What a native acceptance record must satisfy for one chunk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeExpectation {
    pub required_checks: Vec<String>,
    pub fixture_identifier: String,
    /// Executable hash recorded by the frozen native-prepare run.
    pub prepared_executable_sha256: String,
    /// The prepared bundle path, tree hash and identifier (native-prepare.json).
    pub prepared_bundle: Option<String>,
    pub prepared_bundle_sha256: Option<String>,
    pub prepared_identifier: Option<String>,
    /// The prepared bundle as it exists now (None if absent).
    pub current_executable_sha256: Option<String>,
    pub current_bundle_sha256: Option<String>,
    pub current_identifier: Option<String>,
    /// Evidence paths hash-bound by the review packet.
    pub evidence_files: std::collections::BTreeSet<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeChecksFile {
    #[allow(dead_code)]
    schema: String,
    #[allow(dead_code)]
    description: String,
    fixture_identifier: String,
    chunks: std::collections::BTreeMap<String, Vec<RequiredCheck>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequiredCheck {
    id: String,
    #[allow(dead_code)]
    check: String,
}

/// Required check IDs and fixture identity from
/// docs/architecture/native-checks.json for the given chunks (a phase passes
/// all of its chunks: its integrated acceptance repeats every chunk check).
/// A chunk without an inventory fails.
pub fn required_checks(path: &std::path::Path, chunks: &[String]) -> Result<(Vec<String>, String)> {
    let file: NativeChecksFile = crate::read_json(path)?;
    let mut ids = Vec::new();
    for chunk in chunks {
        let checks = file
            .chunks
            .get(chunk)
            .ok_or_else(|| invalid(format!("no native check inventory for {chunk}")))?;
        if checks.is_empty() {
            return Err(invalid(format!(
                "native check inventory for {chunk} is empty"
            )));
        }
        ids.extend(checks.iter().map(|c| c.id.clone()));
    }
    Ok((ids, file.fixture_identifier))
}
