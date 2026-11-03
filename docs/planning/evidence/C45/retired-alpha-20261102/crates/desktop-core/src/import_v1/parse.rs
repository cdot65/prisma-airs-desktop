//! v1's formats as v1 (74754c02) writes them, decoded strictly: an unknown
//! field or version is refused rather than guessed at (D050).

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use super::Problem;

/// `tenants.json` (v1 `settings.rs` `Metadata`, version 1).
pub const TENANTS_FILE: &str = "tenants.json";
pub const TENANTS_LIMIT: u64 = 128 * 1024;
pub const MAX_V1_TENANTS: usize = 64;
/// `fpfn-reports.json` (v1 `fpfn.rs`, version 1).
pub const REPORTS_FILE: &str = "fpfn-reports.json";
pub const REPORTS_LIMIT: u64 = 4 * 1024 * 1024;
/// `appearance.json` (v1 `ds::ThemeChoice`).
pub const APPEARANCE_FILE: &str = "appearance.json";
pub const APPEARANCE_LIMIT: u64 = 4 * 1024;
/// `navigation.json`: never read (not mapped).
pub const NAVIGATION_FILE: &str = "navigation.json";
/// Keychain items.
pub const CREDENTIALS_LIMIT: usize = 64 * 1024;
pub const CASES_LIMIT: usize = 256 * 1024;
pub const HISTORY_LIMIT: usize = 8 * 1024 * 1024;
/// `academy-local/<sha256(account)>/attempt-<sha256(id)>.bin`.
pub const ACADEMY_DIR: &str = "academy-local";
pub const ACADEMY_RECORD_LIMIT: u64 = 16 * 1024 * 1024;
pub const ACADEMY_ENTRIES: usize = 4096;
const ACADEMY_MAGIC: &[u8] = b"ACADEMY\x01";

/// v1's reserved Academy sandbox tenant (never imported).
const ACADEMY_SANDBOX: &str = "acade001-0000-4000-8000-000000000001";

/// Lowercase hyphenated UUID text, as v1's `Uuid` serializes.
pub fn is_uuid(text: &str) -> bool {
    text.len() == 36
        && text.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => matches!(b, b'0'..=b'9' | b'a'..=b'f'),
        })
        && text != "00000000-0000-0000-0000-000000000000"
}

/// Lowercase hex SHA-256 of `value` (v1's account and record digests).
pub fn digest(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

pub fn is_digest(name: &str) -> bool {
    name.len() == 64 && name.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum V1Storage {
    Protected,
    Session,
}

/// v1 `Tenant`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V1Tenant {
    pub id: String,
    pub name: String,
    #[allow(dead_code)]
    pub environment: String,
    pub tsg_id: String,
    pub storage: V1Storage,
    pub credential_ref: Option<String>,
}

/// v1 `Metadata`; the Gateway key references and preferences are read only
/// to be refused (they name credentials and caches).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct V1Metadata {
    version: u8,
    tenants: Vec<V1Tenant>,
    #[serde(default)]
    pending_cleanup: Vec<String>,
    #[serde(default)]
    gateway_keys: Vec<serde::de::IgnoredAny>,
    #[serde(default)]
    gateway_preferences: serde_json::Map<String, Value>,
}

/// The tenants v1 keeps, without the sandbox, and the Keychain accounts it
/// was retiring (their data is stale and never imported).
#[derive(Debug)]
pub struct V1Tenants {
    pub tenants: Vec<V1Tenant>,
    pub retired: Vec<String>,
    /// Gateway key references and preferences v1 held (not imported).
    pub gateway_items: usize,
}

pub fn tenants(bytes: &[u8]) -> Result<V1Tenants, Problem> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| Problem::Corrupt)?;
    if value.get("version").and_then(Value::as_u64) != Some(1) {
        return Err(if value.get("version").is_some() {
            Problem::UnsupportedVersion
        } else {
            Problem::Corrupt
        });
    }
    let meta: V1Metadata = serde_json::from_value(value).map_err(|_| Problem::Corrupt)?;
    debug_assert_eq!(meta.version, 1);
    if meta.tenants.len() > MAX_V1_TENANTS + 1 {
        return Err(Problem::TooLarge);
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut tenants = Vec::new();
    for tenant in meta.tenants {
        if !is_uuid(&tenant.id)
            || !seen.insert(tenant.id.clone())
            || tenant
                .credential_ref
                .as_deref()
                .is_some_and(|r| !is_uuid(r))
        {
            return Err(Problem::Corrupt);
        }
        if tenant.id != ACADEMY_SANDBOX {
            tenants.push(tenant);
        }
    }
    if tenants.len() > MAX_V1_TENANTS {
        return Err(Problem::TooLarge);
    }
    Ok(V1Tenants {
        tenants,
        retired: meta.pending_cleanup,
        gateway_items: meta.gateway_keys.len() + meta.gateway_preferences.len(),
    })
}

/// The one field of v1's secret `Credentials` the import may use. Every
/// other field (client secret, scan key, other endpoints) is skipped by the
/// decoder without being kept.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScanEndpoint {
    #[serde(default)]
    scan_endpoint: String,
}

/// v1's scan endpoint for a tenant (empty: the default region).
pub fn scan_endpoint(bytes: &[u8]) -> Result<String, Problem> {
    serde_json::from_slice::<ScanEndpoint>(bytes)
        .map(|c| c.scan_endpoint)
        .map_err(|_| Problem::Corrupt)
}

/// v1 `appearance.json`'s appearance (`system`, `light` or `dark`); its
/// theme family and time zone have no v2 equivalent.
pub fn appearance(bytes: &[u8]) -> Result<crate::wire::Theme, Problem> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| Problem::Corrupt)?;
    let object = value.as_object().ok_or(Problem::Corrupt)?;
    if object
        .keys()
        .any(|k| !matches!(k.as_str(), "family" | "appearance" | "zone"))
    {
        return Err(Problem::Corrupt);
    }
    match object
        .get("appearance")
        .and_then(Value::as_str)
        .unwrap_or("system")
    {
        "system" => Ok(crate::wire::Theme::System),
        "light" => Ok(crate::wire::Theme::Light),
        "dark" => Ok(crate::wire::Theme::Dark),
        _ => Err(Problem::Corrupt),
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum V1ReportKind {
    FalsePositive,
    FalseNegative,
}

/// v1 `fpfn::Report`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct V1Report {
    scan_id: String,
    #[serde(default)]
    scan_sub_req_id: u64,
    session_id: String,
    application_id: String,
    application: String,
    kind: V1ReportKind,
    verdict: String,
    #[serde(default)]
    action: String,
    detectors: Vec<String>,
    profile: String,
    #[serde(default)]
    category: String,
    reason: String,
    reported_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct V1TenantReports {
    tenant: String,
    reports: Vec<V1Report>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct V1ReportBook {
    #[allow(dead_code)]
    version: u8,
    tenants: Vec<V1TenantReports>,
}

/// v1 reports per v1 tenant, as v2 reports. A report whose sub-request
/// does not fit v2's is counted, not imported.
pub struct V1Reports {
    pub tenant: String,
    pub reports: Vec<crate::sessions::Report>,
    pub unfit: usize,
}

pub fn reports(bytes: &[u8]) -> Result<Vec<V1Reports>, Problem> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| Problem::Corrupt)?;
    match value.get("version").and_then(Value::as_u64) {
        Some(1) => {}
        Some(_) => return Err(Problem::UnsupportedVersion),
        None => return Err(Problem::Corrupt),
    }
    let book: V1ReportBook = serde_json::from_value(value).map_err(|_| Problem::Corrupt)?;
    let mut out = Vec::new();
    for entry in book.tenants {
        if !is_uuid(&entry.tenant) || entry.reports.len() > crate::sessions::MAX_REPORTS {
            return Err(Problem::Corrupt);
        }
        let mut reports = Vec::new();
        let mut unfit = 0;
        for r in entry.reports {
            let Ok(sub) = u32::try_from(r.scan_sub_req_id) else {
                unfit += 1;
                continue;
            };
            reports.push(crate::sessions::Report {
                scan_id: r.scan_id,
                scan_sub_req_id: sub,
                session_id: r.session_id,
                application_id: r.application_id,
                application: r.application,
                kind: match r.kind {
                    V1ReportKind::FalsePositive => crate::sessions::ReportKind::FalsePositive,
                    V1ReportKind::FalseNegative => crate::sessions::ReportKind::FalseNegative,
                },
                verdict: r.verdict,
                action: r.action,
                detectors: r.detectors,
                profile: r.profile,
                category: r.category,
                reason: r.reason,
                reported_at: r.reported_at,
            });
        }
        out.push(V1Reports {
            tenant: entry.tenant,
            reports,
            unfit,
        });
    }
    Ok(out)
}

/// v1's saved cases for a tenant (a JSON array of v1 `TestCase`, which v2's
/// case shape reads unchanged), validated against the v1 tenant.
pub fn cases(bytes: &[u8], v1_tenant: &str) -> Result<Vec<crate::library::TestCase>, Problem> {
    let cases: Vec<crate::library::TestCase> =
        serde_json::from_slice(bytes).map_err(|_| Problem::Corrupt)?;
    if cases.len() > crate::library::CASE_LIMIT {
        return Err(Problem::TooLarge);
    }
    let mut ids = std::collections::BTreeSet::new();
    for case in &cases {
        if case.validate(v1_tenant).is_err() || !ids.insert(case.id.clone()) {
            return Err(Problem::Corrupt);
        }
    }
    Ok(cases)
}

/// One v1 Harness history entry.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct V1Entry {
    pub id: String,
    #[allow(dead_code)]
    pub turn_id: Option<String>,
    pub kind: V1EntryKind,
    #[allow(dead_code)]
    pub title: String,
    pub text: String,
    pub status: V1EntryStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum V1EntryKind {
    User,
    Assistant,
    Tool,
    Approval,
    Notice,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum V1EntryStatus {
    Running,
    Completed,
    Failed,
    Interrupted,
    AwaitingReview,
    #[serde(other)]
    Other,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct V1Conversation {
    pub id: String,
    pub entries: Vec<V1Entry>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub updated_at: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct V1Context {
    pub tenant: String,
    pub workspace: String,
}

/// v1 `Snapshot` (version 1). Its options and engine replay are never
/// imported (another engine's replay; project and model choices).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct V1Snapshot {
    pub version: u8,
    pub context: V1Context,
    #[allow(dead_code)]
    options: serde::de::IgnoredAny,
    pub conversation: V1Conversation,
    #[allow(dead_code)]
    runtime_replay: Option<serde::de::IgnoredAny>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct V1Archive {
    version: u8,
    tenant: String,
    #[allow(dead_code)]
    reference: String,
    conversations: Vec<V1Snapshot>,
}

/// A tenant's archived Harness conversations (`harness-history-{ref}`).
pub fn history(bytes: &[u8], v1_tenant: &str) -> Result<Vec<V1Snapshot>, Problem> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| Problem::Corrupt)?;
    match value.get("version").and_then(Value::as_u64) {
        Some(1) => {}
        Some(_) => return Err(Problem::UnsupportedVersion),
        None => return Err(Problem::Corrupt),
    }
    let archive: V1Archive = serde_json::from_value(value).map_err(|_| Problem::Corrupt)?;
    if archive.version != 1 || archive.tenant != v1_tenant {
        return Err(Problem::Corrupt);
    }
    for s in &archive.conversations {
        if s.version != 1 {
            return Err(Problem::UnsupportedVersion);
        }
        if s.context.tenant != v1_tenant
            || !is_uuid(&s.context.workspace)
            || !is_uuid(&s.conversation.id)
        {
            return Err(Problem::Corrupt);
        }
    }
    Ok(archive.conversations)
}

/// Opens one sealed v1 Academy record (`ACADEMY\x01`, 24-byte nonce,
/// XChaCha20-Poly1305 with associated data `academy:v1:{owner}:{name}`).
/// `owner` is v1's account identifier, which the v1 directory digests.
pub fn academy_record(key: &[u8], owner: &str, name: &str, bytes: &[u8]) -> Result<Value, Problem> {
    if bytes.len() < ACADEMY_MAGIC.len() + 24 + 16 || !bytes.starts_with(ACADEMY_MAGIC) {
        return Err(Problem::Corrupt);
    }
    let cipher = XChaCha20Poly1305::new_from_slice(key).map_err(|_| Problem::Corrupt)?;
    let start = ACADEMY_MAGIC.len();
    let plain = Zeroizing::new(
        cipher
            .decrypt(
                XNonce::from_slice(&bytes[start..start + 24]),
                Payload {
                    msg: &bytes[start + 24..],
                    aad: format!("academy:v1:{owner}:{name}").as_bytes(),
                },
            )
            .map_err(|_| Problem::Corrupt)?,
    );
    serde_json::from_slice(&plain).map_err(|_| Problem::Corrupt)
}

/// A v1 attempt waiting to be saved to Academy: its draft and the version it
/// was based on. Attempts already saved, in conflict, or for another account
/// are not drafts to import.
pub struct V1Draft {
    pub attempt_id: String,
    pub base_version: u32,
    pub draft: Value,
}

pub enum DraftCheck {
    Draft(V1Draft),
    NotDirty,
    Conflict,
    OtherAccount,
}

pub fn draft(attempt: &Value, owner: &str) -> Result<DraftCheck, Problem> {
    if attempt["accountId"].as_str() != Some(owner) {
        return Ok(DraftCheck::OtherAccount);
    }
    if attempt["dirty"] != Value::Bool(true) {
        return Ok(DraftCheck::NotDirty);
    }
    if !attempt["conflict"].is_null() {
        return Ok(DraftCheck::Conflict);
    }
    let attempt_id = attempt["attemptId"]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 512)
        .ok_or(Problem::Corrupt)?;
    let base_version = attempt["draftVersion"]
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or(Problem::Corrupt)?;
    if !attempt["draft"].is_object() {
        return Err(Problem::Corrupt);
    }
    Ok(DraftCheck::Draft(V1Draft {
        attempt_id: attempt_id.to_owned(),
        base_version,
        draft: attempt["draft"].clone(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const T: &str = "11111111-2222-4333-8444-555555555555";

    #[test]
    fn tenants_are_strict_and_drop_the_sandbox() {
        let doc = json!({"version": 1, "tenants": [
            {"id": T, "name": "Prod", "environment": "us", "tsg_id": "1234567890",
             "storage": "protected", "credential_ref": T},
            {"id": ACADEMY_SANDBOX, "name": "Academy Sandbox", "environment": "x",
             "tsg_id": "", "storage": "session", "credential_ref": null}],
            "pending_cleanup": ["cases-x"]});
        let parsed = tenants(doc.to_string().as_bytes()).expect("parsed");
        assert_eq!(parsed.tenants.len(), 1);
        assert_eq!(parsed.retired, vec!["cases-x".to_owned()]);
        let v2 = json!({"version": 2, "tenants": []});
        assert_eq!(
            tenants(v2.to_string().as_bytes()).expect_err("refused"),
            Problem::UnsupportedVersion
        );
        let extra = json!({"version": 1, "tenants": [], "secret": "x"});
        assert_eq!(
            tenants(extra.to_string().as_bytes()).expect_err("refused"),
            Problem::Corrupt
        );
        assert_eq!(tenants(b"not json").expect_err("refused"), Problem::Corrupt);
    }

    #[test]
    fn only_the_scan_endpoint_is_taken_from_credentials() {
        let secret = json!({"clientId": "id", "clientSecret": "CANARY", "tsgId": "1",
            "runtimeKey": "CANARY", "managementEndpoint": "", "tokenEndpoint": "",
            "scanEndpoint": "https://scan.example", "dashboardEndpoint": "", "dlpEndpoint": ""});
        assert_eq!(
            scan_endpoint(secret.to_string().as_bytes()).expect("fixture"),
            "https://scan.example"
        );
    }

    #[test]
    fn appearance_maps_only_light_dark_or_system() {
        assert_eq!(
            appearance(br#"{"family":"ocean","appearance":"dark","zone":"utc"}"#).expect("fixture"),
            crate::wire::Theme::Dark
        );
        assert_eq!(
            appearance(b"{}").expect("fixture"),
            crate::wire::Theme::System
        );
        assert_eq!(
            appearance(br#"{"appearance":"neon"}"#).expect_err("refused"),
            Problem::Corrupt
        );
        assert_eq!(
            appearance(br#"{"other":1}"#).expect_err("refused"),
            Problem::Corrupt
        );
    }

    #[test]
    fn academy_records_open_only_with_their_key_owner_and_name() {
        let key = [7u8; 32];
        let cipher = XChaCha20Poly1305::new_from_slice(&key).expect("fixture");
        let mut n = [0u8; 24];
        getrandom::fill(&mut n).expect("fixture");
        let nonce = XNonce::from_slice(&n);
        let name = format!("attempt-{}.bin", digest("a1"));
        let body = json!({"accountId": "owner", "attemptId": "a1", "dirty": true,
            "draftVersion": 4, "draft": {"steps": {}}, "conflict": null});
        let sealed = cipher
            .encrypt(
                nonce,
                Payload {
                    msg: body.to_string().as_bytes(),
                    aad: format!("academy:v1:owner:{name}").as_bytes(),
                },
            )
            .expect("fixture");
        let mut bytes = ACADEMY_MAGIC.to_vec();
        bytes.extend_from_slice(nonce);
        bytes.extend_from_slice(&sealed);
        let value = academy_record(&key, "owner", &name, &bytes).expect("opened");
        assert!(
            matches!(draft(&value, "owner").expect("fixture"), DraftCheck::Draft(d) if d.base_version == 4)
        );
        assert!(matches!(
            draft(&value, "other").expect("fixture"),
            DraftCheck::OtherAccount
        ));
        assert_eq!(
            academy_record(&key, "other", &name, &bytes).expect_err("refused"),
            Problem::Corrupt
        );
        assert_eq!(
            academy_record(&[8u8; 32], "owner", &name, &bytes).expect_err("refused"),
            Problem::Corrupt
        );
    }
}
