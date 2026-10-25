//! The controlled live scope (C25): the owner-approved private execution
//! manifest and the access report derived from it.
//!
//! The manifest names accounts, environments, permitted operations, budgets
//! and cleanup for every live family L01–L18 (`docs/planning/live-checks.md`,
//! `release-contract.md`). It lives outside the repository, readable only by
//! its owner, and names each secret by where it is stored, never by value.
//! Validation never resolves a secret. A family without designated access is
//! blocked, never dropped: every required workflow row reports which of its
//! families are blocked.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::ledger::Ledger;
use crate::{Repo, Result, invalid, io_err, now_utc, read_json, sha256_hex, write_json};

pub const SCHEMA: &str = "airs-v2/live-manifest/1";
pub const REPORT_SCHEMA: &str = "airs-v2/live-scope/1";
pub const REPORT: &str = "docs/planning/evidence/C25/live-scope.json";
const LEDGER: &str = "docs/architecture/workflow-ledger.json";
/// Keychain services a manifest may name: v2 test items only.
pub const V2_SERVICE_PREFIX: &str = "io.cdot.prisma-airs-desktop.v2.";
/// v1's own Keychain service, which C25 never touches.
pub const V1_SERVICE: &str = "io.cdot.prisma-airs-desktop";
pub const NO_MANIFEST: &str = "no owner-approved live manifest";

/// Families whose operations change remote state: each needs disposable
/// names and cleanup.
pub const MUTATING: [&str; 5] = ["L03", "L04", "L08", "L13", "L17"];
/// Families that consume scans, inference or assessments: each needs a
/// budget.
pub const CONSUMING: [&str; 5] = ["L01", "L09", "L12", "L13", "L16"];

pub fn families() -> Vec<String> {
    (1..=18).map(|n| format!("L{n:02}")).collect()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    pub approved_by: String,
    pub approved_utc: String,
    pub decision: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    pub kind: String,
}

/// Where a secret is stored. The manifest never holds the value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum SecretRef {
    Keychain { service: String, account: String },
    File { path: String },
}

impl SecretRef {
    fn reference(&self) -> String {
        match self {
            Self::Keychain { service, account } => format!("keychain:{service}/{account}"),
            Self::File { path } => format!("file:{path}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Family {
    pub status: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub environment: String,
    #[serde(default)]
    pub credentials: BTreeMap<String, SecretRef>,
    #[serde(default)]
    pub nonsecret: BTreeMap<String, String>,
    #[serde(default)]
    pub permitted_operations: Vec<String>,
    #[serde(default)]
    pub disposable_prefix: Option<String>,
    #[serde(default)]
    pub budget: BTreeMap<String, u64>,
    #[serde(default)]
    pub cleanup: Vec<String>,
    #[serde(default)]
    pub irreversible_exclusions: Vec<String>,
}

impl Family {
    fn designated(&self) -> bool {
        self.status == "designated"
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveManifest {
    pub schema: String,
    /// Free text for the operator; never read as scope.
    #[serde(default)]
    pub note: String,
    pub owner_approval: Approval,
    pub environments: BTreeMap<String, Environment>,
    pub families: BTreeMap<String, Family>,
}

/// A nonsecret key that names a secret.
fn secret_name(key: &str) -> bool {
    key.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|t| {
            matches!(
                t,
                "secret"
                    | "password"
                    | "passwd"
                    | "token"
                    | "key"
                    | "apikey"
                    | "credential"
                    | "credentials"
                    | "privatekey"
            )
        })
}

/// A nonsecret value that looks like a secret: a PEM block, a signed token
/// (JWT), or a long mixed-case alphanumeric run. URLs, numeric IDs and
/// lowercase UUIDs are not.
fn secret_value(value: &str) -> bool {
    if value.contains("-----BEGIN") {
        return true;
    }
    if value.starts_with("eyJ") && value.matches('.').count() >= 2 {
        return true;
    }
    value
        .split(|c: char| !(c.is_ascii_alphanumeric() || "_-+/=".contains(c)))
        .any(|run| {
            run.len() >= 32
                && run.bytes().any(|b| b.is_ascii_uppercase())
                && run.bytes().any(|b| b.is_ascii_lowercase())
                && run.bytes().any(|b| b.is_ascii_digit())
        })
}

fn inside(repo: &Repo, path: &Path) -> bool {
    let root = repo
        .root()
        .canonicalize()
        .unwrap_or(repo.root().to_path_buf());
    let full = path.canonicalize().unwrap_or(path.to_path_buf());
    full.starts_with(&root) || path.starts_with(repo.root())
}

/// Reasons the manifest is not an acceptable scope (empty when it is).
pub fn check(repo: &Repo, manifest: &LiveManifest) -> Vec<String> {
    let mut failures = Vec::new();
    if manifest.schema != SCHEMA {
        failures.push(format!("schema {} is not {SCHEMA}", manifest.schema));
    }
    let a = &manifest.owner_approval;
    if [&a.approved_by, &a.approved_utc, &a.decision]
        .iter()
        .any(|v| v.trim().is_empty())
    {
        failures.push("the owner approval is incomplete".to_owned());
    }
    for (name, env) in &manifest.environments {
        if env.kind != "staging" && env.kind != "test" {
            failures.push(format!(
                "environment {name} is {}; only staging or test is allowed",
                env.kind
            ));
        }
    }
    let ids = families();
    for id in &ids {
        if !manifest.families.contains_key(id) {
            failures.push(format!(
                "{id} is missing; an undesignated family is blocked, not omitted"
            ));
        }
    }
    for (id, family) in &manifest.families {
        if !ids.contains(id) {
            failures.push(format!("{id} is not a live family"));
            continue;
        }
        for (key, value) in &family.nonsecret {
            if secret_name(key) {
                failures.push(format!(
                    "{id} nonsecret {key} names a secret; use a credential reference"
                ));
            } else if secret_value(value) {
                failures.push(format!("{id} nonsecret {key} looks like a secret value"));
            }
        }
        for (name, secret) in &family.credentials {
            match secret {
                SecretRef::Keychain { service, account } => {
                    if service == V1_SERVICE || !service.starts_with(V2_SERVICE_PREFIX) {
                        failures.push(format!(
                            "{id} credential {name} names Keychain service {service}; only {V2_SERVICE_PREFIX}* test items"
                        ));
                    }
                    if account.trim().is_empty() {
                        failures.push(format!("{id} credential {name} has no Keychain account"));
                    }
                }
                SecretRef::File { path } => {
                    let p = Path::new(path);
                    if !p.is_absolute() {
                        failures.push(format!("{id} credential {name} file path is not absolute"));
                    } else if inside(repo, p) {
                        failures.push(format!(
                            "{id} credential {name} file is inside the repository"
                        ));
                    }
                }
            }
        }
        match family.status.as_str() {
            "blocked" => {
                if family.reason.trim().is_empty() {
                    failures.push(format!("{id} is blocked without a reason"));
                }
            }
            "designated" => {
                if !manifest.environments.contains_key(&family.environment) {
                    failures.push(format!(
                        "{id} names unknown environment {:?}",
                        family.environment
                    ));
                }
                if family.permitted_operations.is_empty() {
                    failures.push(format!("{id} is designated with no permitted operations"));
                }
                if MUTATING.contains(&id.as_str())
                    && (family
                        .disposable_prefix
                        .as_deref()
                        .is_none_or(|p| p.trim().is_empty())
                        || family.cleanup.is_empty())
                {
                    failures.push(format!(
                        "{id} changes remote state and needs a disposable prefix and cleanup"
                    ));
                }
                if CONSUMING.contains(&id.as_str()) && family.budget.is_empty() {
                    failures.push(format!("{id} consumes service capacity and needs a budget"));
                }
            }
            other => failures.push(format!("{id} status {other} is not designated or blocked")),
        }
    }
    if let (Some(l01), Some(l18)) = (manifest.families.get("L01"), manifest.families.get("L18"))
        && l01.designated()
        && l18.designated()
    {
        if l01.environment == l18.environment {
            failures
                .push("L18 shares L01's environment; isolation needs a distinct scope".to_owned());
        }
        if l18
            .credentials
            .values()
            .any(|s| l01.credentials.values().any(|t| t == s))
        {
            failures.push(
                "L18 shares a credential reference with L01; isolation needs distinct credentials"
                    .to_owned(),
            );
        }
    }
    failures
}

/// Reads and checks a private manifest file; returns it with its sha256.
pub fn load(repo: &Repo, path: &Path) -> Result<(LiveManifest, String)> {
    if inside(repo, path) {
        return Err(invalid(format!(
            "{} is inside the repository; the live manifest is private",
            path.display()
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)
            .map_err(io_err(path))?
            .permissions()
            .mode();
        if mode & 0o077 != 0 {
            return Err(invalid(format!(
                "{} is readable by group or others (mode {:o}); use 0600",
                path.display(),
                mode & 0o777
            )));
        }
    }
    let bytes = std::fs::read(path).map_err(io_err(path))?;
    let manifest: LiveManifest =
        serde_json::from_slice(&bytes).map_err(|source| crate::Error::Json {
            path: path.to_path_buf(),
            source,
        })?;
    let failures = check(repo, &manifest);
    if !failures.is_empty() {
        return Err(invalid(format!(
            "the live manifest is not acceptable: {}",
            failures.join("; ")
        )));
    }
    Ok((manifest, sha256_hex(&bytes)))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialName {
    pub name: String,
    pub reference: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FamilyAccess {
    pub id: String,
    pub status: String,
    pub reason: String,
    pub environment: String,
    pub environment_kind: Option<String>,
    pub credentials: Vec<CredentialName>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowAccess {
    pub id: String,
    pub workflow: String,
    pub families: Vec<String>,
    pub access_status: String,
    pub blocked_families: Vec<String>,
    pub owner: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub families_designated: usize,
    pub families_blocked: usize,
    pub rows_required: usize,
    pub rows_designated: usize,
    pub rows_blocked: usize,
    pub rows_owned_by_c26: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub schema: String,
    pub generated_utc: String,
    pub manifest_sha256: Option<String>,
    pub owner_approval: Option<Approval>,
    pub families: Vec<FamilyAccess>,
    pub rows: Vec<RowAccess>,
    pub summary: Summary,
}

fn row_families(live_column: &str) -> Vec<String> {
    let mut out: Vec<String> = live_column
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| {
            t.len() == 3 && t.starts_with('L') && t[1..].bytes().all(|b| b.is_ascii_digit())
        })
        .map(str::to_owned)
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Maps every required ledger row to its families' access under `scope`
/// (none: every family blocked).
pub fn report(ledger: &Ledger, scope: Option<(&LiveManifest, &str)>) -> Report {
    let families: Vec<FamilyAccess> = self::families()
        .into_iter()
        .map(
            |id| match scope.and_then(|(m, _)| m.families.get(&id).map(|f| (m, f))) {
                Some((m, f)) => FamilyAccess {
                    status: f.status.clone(),
                    reason: f.reason.clone(),
                    environment: f.environment.clone(),
                    environment_kind: m.environments.get(&f.environment).map(|e| e.kind.clone()),
                    credentials: f
                        .credentials
                        .iter()
                        .map(|(name, s)| CredentialName {
                            name: name.clone(),
                            reference: s.reference(),
                        })
                        .collect(),
                    id,
                },
                None => FamilyAccess {
                    id,
                    status: "blocked".to_owned(),
                    reason: NO_MANIFEST.to_owned(),
                    environment: String::new(),
                    environment_kind: None,
                    credentials: Vec::new(),
                },
            },
        )
        .collect();
    let designated = |id: &str| {
        families
            .iter()
            .any(|f| f.id == id && f.status == "designated")
    };
    let rows: Vec<RowAccess> = ledger
        .rows
        .iter()
        .filter(|r| r.live.requirement == "required")
        .map(|r| {
            let fams = row_families(&r.live_column);
            let blocked: Vec<String> = fams.iter().filter(|f| !designated(f)).cloned().collect();
            let (access_status, owner) = if fams.is_empty() {
                ("final-artifact".to_owned(), Some("C26".to_owned()))
            } else if blocked.is_empty() {
                ("designated".to_owned(), None)
            } else {
                ("blocked-by-access".to_owned(), None)
            };
            RowAccess {
                id: r.id.clone(),
                workflow: r.workflow.clone(),
                families: fams,
                access_status,
                blocked_families: blocked,
                owner,
            }
        })
        .collect();
    let count = |s: &str| rows.iter().filter(|r| r.access_status == s).count();
    let summary = Summary {
        families_designated: families.iter().filter(|f| f.status == "designated").count(),
        families_blocked: families.iter().filter(|f| f.status != "designated").count(),
        rows_required: rows.len(),
        rows_designated: count("designated"),
        rows_blocked: count("blocked-by-access"),
        rows_owned_by_c26: count("final-artifact"),
    };
    Report {
        schema: REPORT_SCHEMA.to_owned(),
        generated_utc: now_utc(),
        manifest_sha256: scope.map(|(_, sha)| sha.to_owned()),
        owner_approval: scope.map(|(m, _)| m.owner_approval.clone()),
        families,
        rows,
        summary,
    }
}

/// Validates the scope (if any) and writes the C25 access report.
pub fn run(repo: &Repo, scope: Option<&Path>) -> Result<Report> {
    let ledger: Ledger = read_json(&repo.join(LEDGER))?;
    let loaded = scope.map(|p| load(repo, p)).transpose()?;
    let record = report(&ledger, loaded.as_ref().map(|(m, sha)| (m, sha.as_str())));
    write_json(&repo.join(REPORT), &record)?;
    Ok(record)
}
