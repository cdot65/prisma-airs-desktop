//! Sessions investigation and metadata-only FP/FN reports (C11).
//!
//! Working v1 (74754c02, read-only): `bridge/src/runtime.ts` (search walks
//! and their bounds), `bridge/src/operations.ts` (sessions reads),
//! `src/model.rs` (search classification), `src/investigation_cache.rs`
//! (cache bounds), `src/fpfn.rs` and `src/fpfn/reports.rs` (report model,
//! limits, upsert/remove/undo, CSV) and `src/presentation.rs` (CSV
//! escaping). IO is limited to the sealed investigation snapshot and the
//! report file, both written atomically with owner-only permissions.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::vault::{RecordId, Vault, VaultError};

/// Sessions per inventory page (v1 `cache::PAGE_SIZE`).
pub const PAGE_SIZE: u32 = 25;
/// Sessions per search page (v1).
pub const SEARCH_PAGE: u32 = 100;
/// Pages a session-ID search walks over 30 days (4,000 sessions, v1).
pub const SESSION_SEARCH_PAGES: u32 = 40;
/// Sessions a detector search reads over 24 hours (v1 `DETECTOR_SCAN`).
pub const DETECTOR_SCAN: u32 = 1_000;
/// Matches at which a detector search stops (v1 `DETECTOR_MATCHES`).
pub const DETECTOR_MATCHES: u32 = 100;
/// Session actions read per session (v1: one page of 25).
pub const SESSION_ACTIONS: u32 = 25;
/// Longest search query (v1).
pub const MAX_QUERY: usize = 256;
/// Entries kept per cache category (v1).
pub const CACHE_ENTRIES: usize = 50;
/// Largest sealed cache snapshot (v1: 4 MiB).
pub const CACHE_BYTES: usize = 4 * 1024 * 1024;

/// What a search query names (v1 `model.rs` heuristics).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum SearchKind {
    SessionId,
    ScanId,
    Detector,
}

fn is_uuid(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    parts.len() == 5
        && [8, 4, 4, 4, 12]
            .iter()
            .zip(&parts)
            .all(|(n, p)| p.len() == *n && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Classifies a trimmed query: a scan ID (`scn`/`scan` prefix or a UUID), a
/// session ID (`ses`/`session` prefix) or a detector code
/// (`^[a-z0-9_]{1,64}$`); `None` when it is none of these or too long.
pub fn classify(query: &str) -> Option<(SearchKind, String)> {
    let query = query.trim();
    if query.is_empty() || query.chars().count() > MAX_QUERY || query.chars().any(char::is_control)
    {
        return None;
    }
    let lower = query.to_ascii_lowercase();
    if lower.starts_with("scn") || lower.starts_with("scan") || is_uuid(query) {
        return Some((SearchKind::ScanId, query.to_owned()));
    }
    if lower.starts_with("ses") || lower.starts_with("session") {
        return Some((SearchKind::SessionId, query.to_owned()));
    }
    let detector = !query.is_empty()
        && query.len() <= 64
        && query
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    detector.then(|| (SearchKind::Detector, query.to_owned()))
}

/// Whether a sessions-overview row reports a violation of `detector`: an
/// entry of its `detection_type_violation_breakdown` with that
/// `detection_type` and any positive count (v1 `runtime.ts`).
pub fn reports_detector(row: &Value, detector: &str) -> bool {
    row.get("detection_type_violation_breakdown")
        .and_then(Value::as_array)
        .is_some_and(|entries| {
            entries.iter().any(|e| {
                e.get("detection_type").and_then(Value::as_str) == Some(detector)
                    && e.get("violation_breakdown")
                        .and_then(Value::as_object)
                        .is_some_and(|counts| {
                            counts.values().any(|n| n.as_f64().is_some_and(|n| n > 0.0))
                        })
            })
        })
}

/// v1's verdict of a violation breakdown: any positive count is a violation,
/// all-zero counts are none, anything else says nothing.
fn violation_verdict(counts: &Value) -> Option<bool> {
    let buckets =
        ["critical", "high", "medium", "low"].map(|k| counts.get(k).and_then(Value::as_u64));
    let total = counts.get("total").and_then(Value::as_u64);
    if total.is_some_and(|n| n > 0) || buckets.iter().any(|n| n.is_some_and(|n| n > 0)) {
        Some(true)
    } else if total == Some(0) || buckets.iter().all(|n| *n == Some(0)) {
        Some(false)
    } else {
        None
    }
}

/// A scan row's verdict, action and violating detector codes (v1
/// `sessions/model.rs` `Scan::read`).
pub fn scan_verdict(row: &Value) -> (&'static str, String, Vec<String>) {
    let codes: Vec<String> = row
        .get("detection_type_violation_breakdown")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter(|e| {
                    violation_verdict(e.get("violation_breakdown").unwrap_or(&Value::Null))
                        == Some(true)
                })
                .filter_map(|e| {
                    e.get("detection_type")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default();
    let status = row
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let counts = violation_verdict(row.get("violation_breakdown").unwrap_or(&Value::Null));
    let verdict = if !codes.is_empty()
        || counts == Some(true)
        || matches!(status.as_str(), "block" | "blocked" | "alert" | "alerted")
    {
        "malicious"
    } else if counts == Some(false) || matches!(status.as_str(), "allow" | "allowed") {
        "benign"
    } else {
        "unknown"
    };
    let action = match status.as_str() {
        "block" | "blocked" => "Blocked".to_owned(),
        "alert" | "alerted" => "Alerted".to_owned(),
        "allow" | "allowed" => "Allowed".to_owned(),
        other => other.to_owned(),
    };
    (verdict, action, codes)
}

/// What the operator says in a report: everything else comes from the scan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReportInput {
    pub kind: ReportKind,
    pub category: String,
    pub reason: String,
}

/// A report of the scan `row` (a session action) in a session of
/// `application`, reported `at`: the scan's metadata, never its content.
pub fn report_of(row: &Value, application: &str, input: ReportInput, at: String) -> Report {
    let text = |key: &str| {
        row.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let (verdict, action, detectors) = scan_verdict(row);
    Report {
        scan_id: text("scan_id"),
        scan_sub_req_id: row
            .get("scan_sub_req_id")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(0),
        session_id: text("session_id"),
        application_id: text("application_id"),
        application: application.to_owned(),
        kind: input.kind,
        verdict: verdict.into(),
        action,
        detectors: detectors.into_iter().take(MAX_DETECTORS).collect(),
        profile: text("profile_name"),
        category: input.category,
        reason: input.reason,
        reported_at: at,
    }
}

/// `secs` since the Unix epoch as RFC 3339 UTC, seconds precision.
pub fn rfc3339(secs: u64) -> String {
    let (y, m, d, hh, mm, ss) = civil(secs);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// `secs` as v1's export file stamp, `YYYYMMDD-HHMM` (UTC).
pub fn file_stamp(secs: u64) -> String {
    let (y, m, d, hh, mm, _) = civil(secs);
    format!("{y:04}{m:02}{d:02}-{hh:02}{mm:02}")
}

/// Civil UTC date and time (Howard Hinnant's days-from-civil inverse).
fn civil(secs: u64) -> (i64, u32, u32, u64, u64, u64) {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d, rem / 3600, rem % 3600 / 60, rem % 60)
}

/// A file-name slug of a tenant name (v1: lowercase ASCII, dashes).
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-');
    let out: String = out.chars().take(40).collect();
    if out.is_empty() { "tenant".into() } else { out }
}

// ---------------------------------------------------------------- reports

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ReportKind {
    FalsePositive,
    FalseNegative,
}

impl ReportKind {
    /// v1's quick-pick reason categories for each kind.
    pub fn categories(self) -> &'static [&'static str] {
        match self {
            Self::FalsePositive => &[
                "Benign business content",
                "Expected internal data",
                "Wrong detector",
                "Test or sample data",
            ],
            Self::FalseNegative => &[
                "Prompt injection missed",
                "Sensitive data missed",
                "Harmful content missed",
                "Malicious URL missed",
            ],
        }
    }
}

/// Report limits (v1 `fpfn.rs`).
pub const MAX_REPORTS: usize = 2_000;
pub const MAX_REASON: usize = 2_000;
pub const MAX_FIELD: usize = 512;
pub const MAX_DETECTORS: usize = 32;

/// One FP/FN report: scan metadata and the operator's reason, never a
/// prompt or response (v1 `Report`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Report {
    pub scan_id: String,
    pub scan_sub_req_id: u32,
    pub session_id: String,
    pub application_id: String,
    pub application: String,
    pub kind: ReportKind,
    /// `malicious`, `benign` or `unknown` (as the scan reported it).
    pub verdict: String,
    pub action: String,
    pub detectors: Vec<String>,
    pub profile: String,
    /// The operator's quick-pick category.
    pub category: String,
    pub reason: String,
    /// RFC 3339, seconds.
    pub reported_at: String,
}

/// Why a report was not saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ReportProblem {
    /// The reason is empty or over 2,000 characters.
    Reason,
    /// A field is over 512 characters or has control characters.
    Field,
    /// More than 32 detectors.
    Detectors,
    /// The tenant already has 2,000 reports.
    Full,
    /// A verdict other than malicious, benign or unknown.
    Verdict,
    /// The scan is not in this investigation (read its session first).
    Scan,
}

/// Checks a report against v1's limits.
pub fn validate(report: &Report) -> Result<(), ReportProblem> {
    let reason = report.reason.trim();
    if reason.is_empty() || report.reason.chars().count() > MAX_REASON {
        return Err(ReportProblem::Reason);
    }
    let field_ok = |s: &str| s.chars().count() <= MAX_FIELD && !s.chars().any(char::is_control);
    let fields = [
        &report.scan_id,
        &report.session_id,
        &report.application_id,
        &report.application,
        &report.verdict,
        &report.action,
        &report.profile,
        &report.category,
        &report.reported_at,
    ];
    if report.scan_id.trim().is_empty() || !fields.iter().all(|f| field_ok(f)) {
        return Err(ReportProblem::Field);
    }
    // A category is optional, and one of the kind's quick picks when given.
    if !report.category.is_empty() && !report.kind.categories().contains(&report.category.as_str())
    {
        return Err(ReportProblem::Field);
    }
    if report.detectors.len() > MAX_DETECTORS || !report.detectors.iter().all(|d| field_ok(d)) {
        return Err(ReportProblem::Detectors);
    }
    if !matches!(report.verdict.as_str(), "malicious" | "benign" | "unknown") {
        return Err(ReportProblem::Verdict);
    }
    Ok(())
}

/// What a removal (or an edit) displaced, for one Undo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Displaced {
    /// The report as it was.
    pub report: Report,
    /// Where it stood (newest first).
    pub position: usize,
}

/// A tenant's reports, newest first, identified by (scan, sub-request).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportBook {
    pub reports: Vec<Report>,
}

impl ReportBook {
    fn position(&self, scan_id: &str, sub: u32) -> Option<usize> {
        self.reports
            .iter()
            .position(|r| r.scan_id == scan_id && r.scan_sub_req_id == sub)
    }

    /// Saves a report: an already-reported scan is edited in place keeping
    /// its original `reported_at` (v1); a new one goes first.
    pub fn save(&mut self, report: Report) -> Result<Option<Displaced>, ReportProblem> {
        validate(&report)?;
        match self.position(&report.scan_id, report.scan_sub_req_id) {
            Some(position) => {
                let previous = self.reports[position].clone();
                let mut edited = report;
                edited.reported_at = previous.reported_at.clone();
                self.reports[position] = edited;
                Ok(Some(Displaced {
                    report: previous,
                    position,
                }))
            }
            None => {
                if self.reports.len() >= MAX_REPORTS {
                    return Err(ReportProblem::Full);
                }
                self.reports.insert(0, report);
                Ok(None)
            }
        }
    }

    /// Removes a scan's report, returning what Undo needs.
    pub fn remove(&mut self, scan_id: &str, sub: u32) -> Option<Displaced> {
        let position = self.position(scan_id, sub)?;
        Some(Displaced {
            report: self.reports.remove(position),
            position,
        })
    }

    /// Restores a removed report at its position. A report saved for that
    /// scan since is newer and is never overwritten (v1 ADR 0020).
    pub fn undo(&mut self, displaced: &Displaced) -> bool {
        let report = &displaced.report;
        if self
            .position(&report.scan_id, report.scan_sub_req_id)
            .is_some()
            || self.reports.len() >= MAX_REPORTS
        {
            return false;
        }
        let position = displaced.position.min(self.reports.len());
        self.reports.insert(position, report.clone());
        true
    }
}

/// One CSV cell (v1 `presentation.rs`): a non-numeric value starting with
/// `= + - @` or a tab or carriage return gets a `'` prefix (formula guard);
/// values with `,` `"` or line breaks are quoted with `"` doubled.
pub fn csv_cell(value: &str) -> String {
    let numeric = !value.is_empty() && value.parse::<f64>().is_ok();
    let guarded = if !numeric && value.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{value}")
    } else {
        value.to_owned()
    };
    if guarded.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}

/// The CSV columns (v1, in order).
pub const CSV_COLUMNS: [&str; 16] = [
    "tenant",
    "tsg_id",
    "kind",
    "scan_id",
    "scan_sub_req_id",
    "session_id",
    "application",
    "application_id",
    "verdict",
    "action",
    "detectors",
    "profile",
    "reason_category",
    "reason",
    "reported_at",
    "exported_at",
];

/// The reports as RFC 4180 CSV with CRLF line endings (v1 export).
pub fn csv(tenant: &str, tsg: &str, reports: &[Report], exported_at: &str) -> String {
    let mut out = CSV_COLUMNS.join(",");
    out.push_str("\r\n");
    for r in reports {
        let kind = match r.kind {
            ReportKind::FalsePositive => "false_positive",
            ReportKind::FalseNegative => "false_negative",
        };
        let sub = r.scan_sub_req_id.to_string();
        let detectors = r.detectors.join(";");
        let row = [
            tenant,
            tsg,
            kind,
            &r.scan_id,
            &sub,
            &r.session_id,
            &r.application,
            &r.application_id,
            &r.verdict,
            &r.action,
            &detectors,
            &r.profile,
            &r.category,
            &r.reason,
            &r.reported_at,
            exported_at,
        ];
        out.push_str(
            &row.iter()
                .map(|c| csv_cell(c))
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push_str("\r\n");
    }
    out
}

// ------------------------------------------------- investigation cache

/// What an investigation keeps: session reads, transactions and the content
/// the operator revealed, each by identity, newest first (v1 snapshot).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InvestigationCache {
    pub sessions: Vec<(String, Value)>,
    pub transactions: Vec<(String, Value)>,
    pub contents: Vec<(String, Value)>,
}

impl InvestigationCache {
    fn keep(list: &mut Vec<(String, Value)>, key: String, value: Value) {
        list.retain(|(k, _)| k != &key);
        list.insert(0, (key, value));
        list.truncate(CACHE_ENTRIES);
    }

    pub fn put_session(&mut self, key: String, value: Value) {
        Self::keep(&mut self.sessions, key, value);
    }

    pub fn put_transaction(&mut self, key: String, value: Value) {
        Self::keep(&mut self.transactions, key, value);
    }

    pub fn put_content(&mut self, key: String, value: Value) {
        Self::keep(&mut self.contents, key, value);
    }

    pub fn get<'a>(list: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
        list.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty() && self.transactions.is_empty() && self.contents.is_empty()
    }

    /// Serialized within [`CACHE_BYTES`], evicting the oldest entries first.
    pub fn bounded_bytes(&self) -> Option<Vec<u8>> {
        let mut cache = self.clone();
        loop {
            let bytes = serde_json::to_vec(&cache).ok()?;
            if bytes.len() <= CACHE_BYTES {
                return Some(bytes);
            }
            // Drop the oldest entry of the largest category.
            let largest = [
                &mut cache.contents,
                &mut cache.transactions,
                &mut cache.sessions,
            ]
            .into_iter()
            .max_by_key(|l| l.len())?;
            largest.pop()?;
        }
    }

    /// The metadata an operator may export: session and transaction reads
    /// without any prompt or response content.
    pub fn metadata(&self) -> Value {
        let mut sessions = Value::Array(self.sessions.iter().map(|(_, v)| v.clone()).collect());
        let mut transactions =
            Value::Array(self.transactions.iter().map(|(_, v)| v.clone()).collect());
        strip_content(&mut sessions);
        strip_content(&mut transactions);
        serde_json::json!({"sessions": sessions, "transactions": transactions})
    }
}

/// Removes prompt and response text wherever it appears.
pub fn strip_content(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for key in [
                "prompt",
                "response",
                "scan_contents",
                "prompt_masked_data",
                "response_masked_data",
            ] {
                map.remove(key);
            }
            map.values_mut().for_each(strip_content);
        }
        Value::Array(items) => items.iter_mut().for_each(strip_content),
        _ => {}
    }
}

/// Seals `plain` for `aad` with a fresh 24-byte nonce (nonce || ciphertext).
pub fn seal(key: &[u8; 32], aad: &[u8], plain: &[u8]) -> Option<Vec<u8>> {
    let mut nonce = [0u8; 24];
    getrandom::fill(&mut nonce).ok()?;
    let cipher = XChaCha20Poly1305::new(chacha20poly1305::Key::from_slice(key));
    let sealed = cipher
        .encrypt(XNonce::from_slice(&nonce), Payload { msg: plain, aad })
        .ok()?;
    let mut out = nonce.to_vec();
    out.extend_from_slice(&sealed);
    Some(out)
}

/// Opens what [`seal`] produced for the same key and `aad`.
pub fn open(key: &[u8; 32], aad: &[u8], sealed: &[u8]) -> Option<Vec<u8>> {
    if sealed.len() < 24 {
        return None;
    }
    let cipher = XChaCha20Poly1305::new(chacha20poly1305::Key::from_slice(key));
    cipher
        .decrypt(
            XNonce::from_slice(&sealed[..24]),
            Payload {
                msg: &sealed[24..],
                aad,
            },
        )
        .ok()
}

/// Writes `bytes` atomically with mode 0600 in a 0700 directory.
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("no directory"))?;
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let mut suffix = [0u8; 8];
    getrandom::fill(&mut suffix).map_err(|_| std::io::Error::other("entropy"))?;
    let temp = dir.join(format!(".{}.tmp", hex::encode(suffix)));
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&temp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

// ------------------------------------------- persisted investigation

/// Associated-data schema of a sealed investigation snapshot.
const INVESTIGATION_SCHEMA: &str = "prisma-airs-desktop-v2/investigation/1";

/// The credential-namespace Keychain record holding the sealing key of a
/// management credential's investigation snapshot. Derived from the
/// credential's record, so it needs no persisted reference and is deleted
/// with the credential (the cleanup ledger) or by a reset (the namespace).
pub fn key_record(credential: &RecordId) -> RecordId {
    let digest = crate::offline::digest(&format!("investigation-key:{}", credential.as_str()));
    RecordId::parse(&format!("cr-{}", &digest[..16])).expect("16 hex digits")
}

/// Where a credential's sealed snapshot lives under the sessions root.
pub fn snapshot_path(root: &Path, credential: &RecordId) -> PathBuf {
    let digest = crate::offline::digest(&format!("investigation:{}", credential.as_str()));
    root.join("investigations")
        .join(format!("{}.sealed", &digest[..32]))
}

/// Serializes every placement of an investigation key or snapshot with
/// their removal (credential cleanup, reset): a write checks under it that
/// its investigation is still live, so nothing retired reappears after its
/// removal succeeded.
static PLACEMENT: Mutex<()> = Mutex::new(());

pub fn placement() -> std::sync::MutexGuard<'static, ()> {
    PLACEMENT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Removes a retired credential's snapshot and key; true when both are gone.
pub fn forget(root: Option<&Path>, keychain: &dyn Vault, credential: &RecordId) -> bool {
    let _placement = placement();
    let file =
        root.is_none_or(
            |root| match std::fs::remove_file(snapshot_path(root, credential)) {
                Ok(()) => true,
                Err(e) => e.kind() == std::io::ErrorKind::NotFound,
            },
        );
    let key = matches!(
        keychain.delete(&key_record(credential)),
        Ok(()) | Err(VaultError::NotFound)
    );
    file && key
}

/// Where a Keychain-stored credential's investigation is sealed.
pub struct Persistence {
    pub root: PathBuf,
    pub credential: RecordId,
    pub keychain: Arc<dyn Vault>,
}

impl Persistence {
    fn aad(&self) -> Vec<u8> {
        format!(
            "{INVESTIGATION_SCHEMA}:{}",
            crate::offline::digest(self.credential.as_str())
        )
        .into_bytes()
    }

    fn key(&self, create: bool) -> Option<zeroize::Zeroizing<[u8; 32]>> {
        let record = key_record(&self.credential);
        let bytes = match self.keychain.get(&record) {
            Ok(bytes) => bytes,
            Err(VaultError::NotFound) if create => {
                let fresh = crate::offline::new_key()?;
                self.keychain.put(&record, &fresh).ok()?;
                fresh
            }
            Err(_) => return None,
        };
        let mut key = zeroize::Zeroizing::new([0u8; 32]);
        (bytes.len() == 32).then(|| key.copy_from_slice(&bytes))?;
        Some(key)
    }

    fn load(&self) -> Option<InvestigationCache> {
        let path = snapshot_path(&self.root, &self.credential);
        let sealed = read_bounded(&path, CACHE_BYTES as u64 + 64).ok()??;
        let plain = self
            .key(false)
            .and_then(|key| open(&key, &self.aad(), &sealed))
            .and_then(|plain| serde_json::from_slice(&plain).ok());
        if plain.is_none() {
            // A snapshot that does not open under this credential's key is
            // never shown; it is removed so it cannot be retried.
            let _ = std::fs::remove_file(&path);
        }
        plain
    }

    fn store(&self, cache: &InvestigationCache) -> bool {
        let path = snapshot_path(&self.root, &self.credential);
        if cache.is_empty() {
            return match std::fs::remove_file(&path) {
                Ok(()) => true,
                Err(e) => e.kind() == std::io::ErrorKind::NotFound,
            };
        }
        let Some(bytes) = cache.bounded_bytes() else {
            return false;
        };
        self.key(true)
            .and_then(|key| seal(&key, &self.aad(), &bytes))
            .is_some_and(|sealed| write_private(&path, &sealed).is_ok())
    }
}

/// Reads a file of at most `limit` bytes; `None` when it does not exist.
fn read_bounded(path: &Path, limit: u64) -> std::io::Result<Option<Vec<u8>>> {
    use std::io::Read;
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(std::io::Error::other("too large"));
    }
    Ok(Some(bytes))
}

/// How an investigation is kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum InvestigationStorage {
    /// In memory for this credential generation only (session credentials,
    /// or no protected storage).
    Memory,
    /// Sealed on this Mac under a Keychain key (Keychain credentials).
    Protected,
    /// Protected storage failed; kept in memory only.
    Unavailable,
}

/// One credential generation's investigation: session reads, transactions
/// and revealed content. The cache never authorizes anything: every read,
/// cached or not, first binds the current credential.
pub struct Investigation {
    cache: Mutex<InvestigationCache>,
    persistence: Option<Persistence>,
    loaded: tokio::sync::OnceCell<()>,
    /// Snapshot writes, one at a time, each of the cache as it is then.
    writes: tokio::sync::Mutex<()>,
    failed: std::sync::atomic::AtomicBool,
    /// Set when its credential generation retires: nothing more is placed.
    retired: std::sync::atomic::AtomicBool,
}

impl Investigation {
    pub fn new(persistence: Option<Persistence>) -> Arc<Self> {
        Arc::new(Self {
            cache: Mutex::default(),
            persistence,
            loaded: tokio::sync::OnceCell::new(),
            writes: tokio::sync::Mutex::new(()),
            failed: std::sync::atomic::AtomicBool::new(false),
            retired: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// Stops all further placement. A write already checked under the
    /// placement lock finishes first; cleanup and reset remove under the same
    /// lock, so they always come after it. (Never blocks: called with the
    /// authority locked.)
    pub fn retire(&self) {
        self.retired
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn storage(&self) -> InvestigationStorage {
        match (
            &self.persistence,
            self.failed.load(std::sync::atomic::Ordering::Relaxed),
        ) {
            (None, _) => InvestigationStorage::Memory,
            (Some(_), false) => InvestigationStorage::Protected,
            (Some(_), true) => InvestigationStorage::Unavailable,
        }
    }

    /// Loads the sealed snapshot once (Keychain credentials only).
    pub async fn ready(self: &Arc<Self>) {
        self.loaded
            .get_or_init(|| async {
                let this = Arc::clone(self);
                let loaded = tokio::task::spawn_blocking(move || {
                    this.persistence.as_ref().and_then(Persistence::load)
                })
                .await
                .ok()
                .flatten();
                if let (Some(loaded), Ok(mut cache)) = (loaded, self.cache.lock()) {
                    // Anything read before the load finished is newer.
                    for (k, v) in loaded.sessions.into_iter().rev() {
                        if InvestigationCache::get(&cache.sessions, &k).is_none() {
                            cache.sessions.push((k, v));
                        }
                    }
                    for (k, v) in loaded.transactions.into_iter().rev() {
                        if InvestigationCache::get(&cache.transactions, &k).is_none() {
                            cache.transactions.push((k, v));
                        }
                    }
                    for (k, v) in loaded.contents.into_iter().rev() {
                        if InvestigationCache::get(&cache.contents, &k).is_none() {
                            cache.contents.push((k, v));
                        }
                    }
                    cache.sessions.truncate(CACHE_ENTRIES);
                    cache.transactions.truncate(CACHE_ENTRIES);
                    cache.contents.truncate(CACHE_ENTRIES);
                }
            })
            .await;
    }

    pub fn with<R>(&self, f: impl FnOnce(&mut InvestigationCache) -> R) -> Option<R> {
        self.cache.lock().ok().map(|mut cache| f(&mut cache))
    }

    /// Counts per category (sessions, transactions, revealed contents).
    pub fn counts(&self) -> (u32, u32, u32) {
        self.with(|c| {
            let n = |l: &Vec<(String, Value)>| u32::try_from(l.len()).unwrap_or(u32::MAX);
            (n(&c.sessions), n(&c.transactions), n(&c.contents))
        })
        .unwrap_or_default()
    }

    /// Seals the cache as it is now (a no-op for memory-only investigations).
    pub async fn persist(self: &Arc<Self>) {
        if self.persistence.is_none() {
            return;
        }
        let _write = self.writes.lock().await;
        let this = Arc::clone(self);
        let stored = tokio::task::spawn_blocking(move || {
            let _placement = placement();
            if this.retired.load(std::sync::atomic::Ordering::SeqCst) {
                return true;
            }
            let snapshot = this.with(|c| c.clone()).unwrap_or_default();
            this.persistence
                .as_ref()
                .is_some_and(|p| p.store(&snapshot))
        })
        .await
        .unwrap_or(false);
        self.failed
            .store(!stored, std::sync::atomic::Ordering::Relaxed);
    }

    /// Empties the cache, sealed snapshot included.
    pub async fn clear(self: &Arc<Self>) {
        self.ready().await;
        self.with(|c| *c = InvestigationCache::default());
        self.persist().await;
    }
}

// ------------------------------------------------------------ report file

/// Largest report file (v1: 4 MiB).
pub const REPORT_FILE_BYTES: u64 = 4 * 1024 * 1024;

/// Where a tenant's reports live under the sessions root (named by a digest
/// of the tenant ID).
pub fn reports_path(root: &Path, tenant: &str) -> PathBuf {
    let digest = crate::offline::digest(&format!("reports:{tenant}"));
    root.join("reports").join(format!("{}.json", &digest[..32]))
}

/// Why a tenant's reports could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ReportStoreFailure {
    /// The file could not be read or replaced.
    Unavailable,
    /// The file is not a report book (it was kept, not overwritten).
    Unreadable,
}

/// Reads a tenant's reports (none when there is no file yet). Every report
/// is checked against the limits; a file that fails is reported, not reset.
pub fn load_reports(path: &Path) -> Result<ReportBook, ReportStoreFailure> {
    let Some(bytes) =
        read_bounded(path, REPORT_FILE_BYTES).map_err(|_| ReportStoreFailure::Unavailable)?
    else {
        return Ok(ReportBook::default());
    };
    let book: ReportBook =
        serde_json::from_slice(&bytes).map_err(|_| ReportStoreFailure::Unreadable)?;
    let valid =
        book.reports.len() <= MAX_REPORTS && book.reports.iter().all(|r| validate(r).is_ok());
    valid.then_some(book).ok_or(ReportStoreFailure::Unreadable)
}

/// Replaces a tenant's report file (0600, atomic).
pub fn store_reports(path: &Path, book: &ReportBook) -> Result<(), ReportStoreFailure> {
    let bytes = serde_json::to_vec(book).map_err(|_| ReportStoreFailure::Unavailable)?;
    if bytes.len() as u64 > REPORT_FILE_BYTES {
        return Err(ReportStoreFailure::Unavailable);
    }
    write_private(path, &bytes).map_err(|_| ReportStoreFailure::Unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn report(scan: &str) -> Report {
        Report {
            scan_id: scan.into(),
            scan_sub_req_id: 0,
            session_id: "ses-1".into(),
            application_id: "app-1".into(),
            application: "Bot".into(),
            kind: ReportKind::FalsePositive,
            verdict: "malicious".into(),
            action: "block".into(),
            detectors: vec!["dlp".into(), "prompt_injection".into()],
            profile: "Prod".into(),
            category: "Benign business content".into(),
            reason: "Order numbers look like card numbers".into(),
            reported_at: "2026-10-19T10:00:00Z".into(),
        }
    }

    #[test]
    fn queries_are_classified_like_v1() {
        assert_eq!(
            classify("  scn-123 ").map(|c| c.0),
            Some(SearchKind::ScanId)
        );
        assert_eq!(
            classify("11111111-2222-4333-8444-555555555555").map(|c| c.0),
            Some(SearchKind::ScanId)
        );
        assert_eq!(
            classify("session-abc").map(|c| c.0),
            Some(SearchKind::SessionId)
        );
        assert_eq!(
            classify("prompt_injection").map(|c| c.0),
            Some(SearchKind::Detector)
        );
        assert_eq!(classify("Not A Detector!"), None);
        assert_eq!(classify(&"a".repeat(257)), None);
        assert_eq!(classify(""), None);
    }

    #[test]
    fn reports_upsert_remove_and_undo_without_overwriting_newer() {
        let mut book = ReportBook::default();
        assert_eq!(book.save(report("s1")), Ok(None));
        assert_eq!(book.save(report("s2")), Ok(None));
        assert_eq!(book.reports[0].scan_id, "s2", "newest first");
        // Editing keeps the original reported_at.
        let mut edit = report("s1");
        edit.reason = "Edited".into();
        edit.reported_at = "2027-01-01T00:00:00Z".into();
        let displaced = book.save(edit).expect("saved").expect("edit");
        assert_eq!(
            displaced.report.reason,
            "Order numbers look like card numbers"
        );
        assert_eq!(book.reports[1].reported_at, "2026-10-19T10:00:00Z");
        assert_eq!(book.reports[1].reason, "Edited");
        // Remove then undo restores the position.
        let removed = book.remove("s2", 0).expect("removed");
        assert_eq!(book.reports.len(), 1);
        assert!(book.undo(&removed));
        assert_eq!(book.reports[0].scan_id, "s2");
        // An undo never overwrites a report saved since.
        let removed = book.remove("s2", 0).expect("removed");
        let mut newer = report("s2");
        newer.reason = "Newer".into();
        book.save(newer).expect("saved");
        assert!(!book.undo(&removed));
        assert_eq!(book.reports[0].reason, "Newer");
    }

    #[test]
    fn reports_are_bounded() {
        let mut long = report("s");
        long.reason = "x".repeat(2_001);
        assert_eq!(validate(&long), Err(ReportProblem::Reason));
        let mut blank = report("s");
        blank.reason = "  ".into();
        assert_eq!(validate(&blank), Err(ReportProblem::Reason));
        let mut many = report("s");
        many.detectors = vec!["d".into(); 33];
        assert_eq!(validate(&many), Err(ReportProblem::Detectors));
        let mut field = report("s");
        field.application = "a".repeat(513);
        assert_eq!(validate(&field), Err(ReportProblem::Field));
        let mut verdict = report("s");
        verdict.verdict = "allowed".into();
        assert_eq!(validate(&verdict), Err(ReportProblem::Verdict));
        let mut book = ReportBook {
            reports: (0..MAX_REPORTS).map(|n| report(&format!("s{n}"))).collect(),
        };
        assert_eq!(book.save(report("new")), Err(ReportProblem::Full));
    }

    #[test]
    fn csv_is_rfc4180_with_v1s_formula_guard() {
        assert_eq!(csv_cell("=SUM(A1)"), "'=SUM(A1)");
        assert_eq!(csv_cell("-12"), "-12", "numbers are not guarded");
        assert_eq!(csv_cell("@x"), "'@x");
        assert_eq!(csv_cell("a,b"), "\"a,b\"");
        assert_eq!(csv_cell("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_cell("line\nbreak"), "\"line\nbreak\"");
        let mut r = report("s1");
        r.reason = "=HYPERLINK(\"x\")".into();
        let out = csv("Tenant, A", "123", &[r], "2026-10-19T11:00:00Z");
        let lines: Vec<&str> = out.split("\r\n").collect();
        assert_eq!(lines[0], CSV_COLUMNS.join(","));
        assert!(lines[1].starts_with("\"Tenant, A\",123,false_positive,s1,0,ses-1,Bot,app-1,malicious,block,dlp;prompt_injection,Prod,"));
        assert!(
            lines[1].contains("\"'=HYPERLINK(\"\"x\"\")\""),
            "{}",
            lines[1]
        );
        assert!(out.ends_with("\r\n"));
    }

    #[test]
    fn the_cache_is_bounded_sealed_and_exports_metadata_only() {
        let mut cache = InvestigationCache::default();
        for n in 0..60 {
            cache.put_session(format!("s{n}"), json!({"id": n}));
        }
        assert_eq!(cache.sessions.len(), CACHE_ENTRIES);
        assert_eq!(cache.sessions[0].0, "s59", "newest first");
        cache.put_content(
            "c".into(),
            json!({"prompt": "SECRET PROMPT", "response": "R"}),
        );
        cache.put_transaction(
            "t".into(),
            json!({"scan_id": "x", "scan_contents": {"prompt": "P"}, "tokens": 3}),
        );
        let metadata = cache.metadata().to_string();
        assert!(!metadata.contains("SECRET PROMPT") && !metadata.contains("scan_contents"));
        assert!(metadata.contains("\"tokens\":3"));
        let key = [7u8; 32];
        let bytes = cache.bounded_bytes().expect("bytes");
        let sealed = seal(&key, b"tenant-a", &bytes).expect("sealed");
        assert!(!sealed.windows(13).any(|w| w == b"SECRET PROMPT"));
        assert_eq!(
            open(&key, b"tenant-a", &sealed).as_deref(),
            Some(bytes.as_slice())
        );
        assert_eq!(
            open(&key, b"tenant-b", &sealed),
            None,
            "bound to its tenant"
        );
        // Oversized caches evict their oldest entries.
        let mut big = InvestigationCache::default();
        for n in 0..10 {
            big.put_content(format!("c{n}"), json!({"prompt": "x".repeat(600 * 1024)}));
        }
        let bounded: InvestigationCache =
            serde_json::from_slice(&big.bounded_bytes().expect("bytes")).expect("json");
        assert!(bounded.contents.len() < 10 && bounded.contents[0].0 == "c9");
    }
}
