//! The controlled live runner (C25): drives each designated family's checks
//! through the production application core (`desktop_core::Core`, its
//! authority intents, the app-owned transport and the published SDK), never
//! a parallel client, and writes one redacted receipt per check.
//!
//! The runner refuses a manifest `live::check` rejects and every blocked
//! family. A secret is resolved only when its check needs it, held in
//! memory (zeroized on drop) and never logged or written: a receipt that
//! would contain one is refused. Budgets are charged before each request
//! and a request over its cap is never sent. An unexpected tenant,
//! authority or cost, or an outcome that may or may not have taken effect,
//! stops the run at once; the stopping receipt says it needs reconciliation.
//! A browser step (device, company or MCP sign-in) or an access change is
//! the operator's: the runner prompts, then waits a bounded time for the
//! core to show the result, and records the step.
//!
//! A run against a local fake service is recorded as `fixture` evidence,
//! never as live evidence.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use desktop_core::context::ContextRef;
use desktop_core::management::{ManagementEndpoints, ManagementResult};
use desktop_core::tenants::{Destination, Region};
use desktop_core::transport::{ProxyMode, TransportPolicy};
use desktop_core::{
    AuthorityHandle, Core, CoreConfig, Delivery, Intent, Observation, Receipt, ShellSnapshot,
    ViewScope,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zeroize::Zeroizing;

use crate::live::{self, Family, LiveManifest, SecretRef};
use crate::live_adapters::{Confirmations, Host, Operator, TerminalOperator};
use crate::{Repo, Result, Subject, invalid, io_err, now_utc, random_hex, write_json};

pub const RECEIPT_SCHEMA: &str = "airs-v2/live-receipt/1";
pub const RUN_SCHEMA: &str = "airs-v2/live-run/1";

/// Families whose checks need the native desktop app rather than xtask.
pub const NATIVE_ONLY: [&str; 1] = ["L16"];

/// How long the operator has for a browser step or an access change unless
/// the family sets `operator_timeout_secs`.
const OPERATOR_TIMEOUT: Duration = Duration::from_secs(300);

/// What kind of evidence a run produces.
#[derive(Clone, Debug)]
pub enum Evidence {
    /// The designated services, with the production transport policy.
    Live,
    /// Local fake services; its receipts are `fixture` and never close a
    /// live requirement.
    Fixture(FixtureTargets),
}

/// Where a fixture run's services are (a live run never overrides these).
#[derive(Clone, Debug, Default)]
pub struct FixtureTargets {
    /// Trusted roots (PEM) for the fake services' certificates.
    pub extra_roots: Vec<Vec<u8>>,
    /// The management, DLP, Red Team and Gateway planes, all at one origin.
    pub management: Option<String>,
    /// The Academy origin (otherwise a family's `academy_origin`).
    pub academy: Option<String>,
    /// How long a fixture run waits for anything (default 10 s).
    pub settle: Option<Duration>,
}

impl Evidence {
    pub fn fixture(extra_roots: Vec<Vec<u8>>) -> Self {
        Evidence::Fixture(FixtureTargets {
            extra_roots,
            ..FixtureTargets::default()
        })
    }

    pub(crate) fn class(&self) -> &'static str {
        match self {
            Evidence::Live => "live",
            Evidence::Fixture(_) => "fixture",
        }
    }

    fn settle(&self) -> Duration {
        match self {
            Evidence::Live => Duration::from_secs(90),
            Evidence::Fixture(f) => f.settle.unwrap_or(Duration::from_secs(10)),
        }
    }
}

/// Resolves a secret reference at the moment of use.
pub trait Resolve: Send + Sync {
    fn resolve(&self, reference: &SecretRef) -> Result<Zeroizing<String>>;
}

/// The macOS Keychain (through `security`, value to memory only) or an
/// owner-only file.
pub struct SystemResolver;

impl Resolve for SystemResolver {
    fn resolve(&self, reference: &SecretRef) -> Result<Zeroizing<String>> {
        let bytes = match reference {
            SecretRef::Keychain { service, account } => {
                let out = std::process::Command::new("/usr/bin/security")
                    .args(["find-generic-password", "-s", service, "-a", account, "-w"])
                    .stdin(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .output()
                    .map_err(|_| {
                        invalid(format!("security could not run for {service}/{account}"))
                    })?;
                if !out.status.success() {
                    return Err(invalid(format!(
                        "no Keychain item {service}/{account} (or access was refused)"
                    )));
                }
                Zeroizing::new(out.stdout)
            }
            SecretRef::File { path } => Zeroizing::new(
                std::fs::read(path).map_err(|_| invalid(format!("cannot read {path}")))?,
            ),
        };
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| invalid("a secret is not UTF-8"))?
            .trim_end_matches(['\n', '\r']);
        if text.is_empty() {
            return Err(invalid("a secret reference resolved to nothing"));
        }
        Ok(Zeroizing::new(text.to_owned()))
    }
}

/// Why the run stopped: further checks would not be safe.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stop {
    pub family: String,
    pub check: String,
    pub reason: String,
    /// The outcome may or may not have taken effect; an operator must
    /// reconcile it before anything else runs.
    pub reconcile: bool,
}

/// The consumption ledger's schema.
pub const CONSUMPTION_SCHEMA: &str = "airs-v2/live-consumption/1";

/// The live consumption ledger beside the receipts: append-only and never
/// reset, so a manifest's caps bound every run together. Fixture runs keep
/// their own ledger and never charge this one.
pub const LIVE_LEDGER: &str = "consumption.jsonl";
pub const FIXTURE_LEDGER: &str = "consumption-fixture.jsonl";

/// One line of the consumption ledger. `kind` is `spend` (charged before a
/// request), `report` (consumption the service reported afterwards) or
/// `note` (spend outside the runner, recorded by an operator).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Charge {
    pub schema: String,
    pub recorded_utc: String,
    /// The run (its start time and a random suffix), or `operator-note`.
    pub run: String,
    pub manifest_sha256: String,
    pub family: String,
    pub budget: String,
    pub n: u64,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Where a guard records what it charges.
struct Ledger {
    path: PathBuf,
    run: String,
    sha: String,
}

impl Ledger {
    /// Appends one charge durably; nothing is charged if it cannot.
    fn append(&self, family: &str, budget: &str, n: u64, kind: &str) -> Result<()> {
        use std::io::Write;
        let charge = Charge {
            schema: CONSUMPTION_SCHEMA.to_owned(),
            recorded_utc: now_utc(),
            run: self.run.clone(),
            manifest_sha256: self.sha.clone(),
            family: family.to_owned(),
            budget: budget.to_owned(),
            n,
            kind: kind.to_owned(),
            reason: None,
        };
        let mut line = serde_json::to_vec(&charge).map_err(|e| invalid(e.to_string()))?;
        line.push(b'\n');
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(io_err(parent))?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(io_err(&self.path))?;
        file.write_all(&line).map_err(io_err(&self.path))?;
        file.sync_all().map_err(io_err(&self.path))
    }
}

/// Every charge in a consumption ledger (none when it does not exist). A
/// line that does not parse fails: spend that cannot be counted is never
/// taken for none.
pub fn read_charges(path: &Path) -> Result<Vec<Charge>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io_err(path)(e)),
    };
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, l)| {
            let charge: Charge = serde_json::from_str(l)
                .map_err(|e| invalid(format!("{} line {}: {e}", path.display(), i + 1)))?;
            if charge.schema != CONSUMPTION_SCHEMA {
                return Err(invalid(format!(
                    "{} line {}: schema {}",
                    path.display(),
                    i + 1,
                    charge.schema
                )));
            }
            Ok(charge)
        })
        .collect()
}

/// Budget caps and disposable names, enforced before any request.
#[derive(Default)]
pub struct Guard {
    /// (family, budget key) → (limit, spent in earlier runs, spent now).
    spent: BTreeMap<(String, String), (u64, u64, u64)>,
    ledger: Option<Ledger>,
}

impl Guard {
    pub fn new(manifest: &LiveManifest) -> Self {
        let mut spent = BTreeMap::new();
        for (id, family) in &manifest.families {
            for (key, limit) in &family.budget {
                spent.insert((id.clone(), key.clone()), (*limit, 0, 0));
            }
        }
        Self {
            spent,
            ledger: None,
        }
    }

    /// A guard that counts every charge already in the ledger at `path`
    /// against the caps and appends each new one there before it is
    /// spent. Earlier spend counts whichever manifest version it was made
    /// under: a revised scope does not refill a budget.
    pub fn with_ledger(manifest: &LiveManifest, path: &Path, run: &str, sha: &str) -> Result<Self> {
        let mut guard = Self::new(manifest);
        for charge in read_charges(path)? {
            if let Some((_, prior, _)) = guard.spent.get_mut(&(charge.family, charge.budget)) {
                *prior = prior.saturating_add(charge.n);
            }
        }
        guard.ledger = Some(Ledger {
            path: path.to_path_buf(),
            run: run.to_owned(),
            sha: sha.to_owned(),
        });
        Ok(guard)
    }

    /// Charges `n` of `family`'s `key` budget, or refuses (nothing charged)
    /// when that would exceed the cap across every run, the budget was
    /// never set, or the charge cannot be recorded.
    pub fn spend(&mut self, family: &str, key: &str, n: u64) -> std::result::Result<(), String> {
        let Some((limit, prior, used)) = self.spent.get_mut(&(family.to_owned(), key.to_owned()))
        else {
            return Err(format!("{family} has no {key} budget"));
        };
        let total = prior.saturating_add(*used);
        if total.saturating_add(n) > *limit {
            return Err(format!(
                "{family} {key} budget exhausted ({total} of {limit} used across runs; {n} more refused)"
            ));
        }
        if let Some(ledger) = &self.ledger {
            ledger
                .append(family, key, n, "spend")
                .map_err(|e| format!("{family} {key} charge could not be recorded: {e}"))?;
        }
        *used += n;
        Ok(())
    }

    /// Records consumption the service reported after the fact (inference
    /// tokens); refuses (but records) when it went over the cap.
    pub fn report(&mut self, family: &str, key: &str, n: u64) -> std::result::Result<(), String> {
        let Some((limit, prior, used)) = self.spent.get_mut(&(family.to_owned(), key.to_owned()))
        else {
            return Err(format!("{family} has no {key} budget"));
        };
        *used = used.saturating_add(n);
        if let Some(ledger) = &self.ledger {
            ledger
                .append(family, key, n, "report")
                .map_err(|e| format!("{family} {key} consumption could not be recorded: {e}"))?;
        }
        let total = prior.saturating_add(*used);
        if total > *limit {
            return Err(format!(
                "{family} {key} over its cap ({total} of {limit} across runs)"
            ));
        }
        Ok(())
    }

    /// Consumption across every run against each cap for `family`.
    pub fn consumption(&self, family: &str) -> BTreeMap<String, Value> {
        self.spent
            .iter()
            .filter(|((f, _), _)| f == family)
            .map(|((_, k), (limit, prior, used))| {
                (
                    k.clone(),
                    json!({"used": prior.saturating_add(*used), "limit": limit}),
                )
            })
            .collect()
    }
}

/// Whether `name` may be created for a family: only under its designated
/// disposable prefix (a family without one creates nothing).
pub fn disposable(prefix: Option<&str>, name: &str) -> std::result::Result<(), String> {
    match prefix {
        None => Err(format!(
            "{name}: this family has no disposable prefix and creates nothing"
        )),
        Some("") => Err("an empty disposable prefix designates nothing".to_owned()),
        Some(p) if name.starts_with(p) && name.len() > p.len() => Ok(()),
        Some(p) => Err(format!("{name} is not under the disposable prefix {p}")),
    }
}

/// A remote identifier as recorded: identifier characters only, bounded.
pub fn sanitize_id(id: &str) -> String {
    if !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._:-".contains(c))
    {
        id.to_owned()
    } else {
        "<unrecognized>".to_owned()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CheckReceipt {
    pub schema: String,
    pub evidence: String,
    pub family: String,
    pub check: String,
    pub workflows: Vec<String>,
    pub environment: String,
    pub manifest_sha256: String,
    pub started_utc: String,
    pub finished_utc: String,
    /// The exact non-secret inputs; credentials appear by name only.
    pub inputs: BTreeMap<String, Value>,
    /// `passed`, `failed`, `blocked`, `needs-reconciliation` or
    /// `native-app-required`.
    pub outcome: String,
    pub detail: String,
    pub observed: BTreeMap<String, Value>,
    /// Sanitized remote request/trace identifiers.
    pub ids: BTreeMap<String, String>,
    pub readback: String,
    pub cleanup: String,
    pub consumption: BTreeMap<String, Value>,
    /// A step the operator completed (or did not) outside the runner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_step: Option<Value>,
    /// What produced the receipt: [`implementation`].
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub implementation: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FamilyRun {
    pub id: String,
    /// `passed`, `failed`, `stopped`, `blocked` or `needs-native-app`.
    pub status: String,
    pub checks: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunRecord {
    pub schema: String,
    pub evidence: String,
    pub manifest_sha256: String,
    pub started_utc: String,
    pub finished_utc: String,
    pub families: Vec<FamilyRun>,
    pub stopped: Option<Stop>,
    /// What produced the run: [`implementation`].
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub implementation: BTreeMap<String, String>,
}

/// The implementation a run executed: the repository's source fingerprint
/// (every source file, evidence and reviews excluded), the resolved
/// dependency lockfile and the runner binary itself. A live run refuses to
/// start without them; a fixture run outside a repository marks the source
/// unavailable.
pub(crate) fn implementation(repo: &Repo, live: bool) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    match crate::fingerprint::source(repo) {
        Ok(source) => {
            out.insert("source_fingerprint".to_owned(), source);
        }
        Err(e) if live => return Err(invalid(format!("the source fingerprint: {e}"))),
        Err(_) => {
            out.insert("source_fingerprint".to_owned(), "unavailable".to_owned());
        }
    }
    let lock = repo.join("Cargo.lock");
    match std::fs::read(&lock) {
        Ok(bytes) => {
            out.insert("cargo_lock_sha256".to_owned(), crate::sha256_hex(&bytes));
        }
        Err(e) if live => return Err(invalid(format!("Cargo.lock: {e}"))),
        Err(_) => {}
    }
    let exe = std::env::current_exe().map_err(|e| invalid(format!("the runner binary: {e}")))?;
    let bytes = std::fs::read(&exe).map_err(io_err(&exe))?;
    out.insert("runner_sha256".to_owned(), crate::sha256_hex(&bytes));
    Ok(out)
}

impl RunRecord {
    pub fn passed(&self) -> bool {
        self.stopped.is_none() && self.families.iter().all(|f| f.status == "passed")
    }
}

/// What a check found, before it becomes a receipt.
pub(crate) struct Found {
    pub outcome: &'static str,
    pub detail: String,
    pub observed: BTreeMap<String, Value>,
    pub ids: BTreeMap<String, String>,
    pub stop: Option<(String, bool)>,
    pub readback: String,
    pub cleanup: String,
    pub operator_step: Option<Value>,
}

impl Found {
    pub fn new(outcome: &'static str, detail: impl Into<String>) -> Self {
        Self {
            outcome,
            detail: detail.into(),
            observed: BTreeMap::new(),
            ids: BTreeMap::new(),
            stop: None,
            readback: "not applicable: the check creates no remote resource".to_owned(),
            cleanup: "nothing created".to_owned(),
            operator_step: None,
        }
    }

    pub fn passed(detail: impl Into<String>) -> Self {
        Self::new("passed", detail)
    }

    pub fn failed(detail: impl Into<String>) -> Self {
        Self::new("failed", detail)
    }

    pub fn blocked(detail: impl Into<String>) -> Self {
        Self::new("blocked", detail)
    }

    pub fn native(detail: impl Into<String>) -> Self {
        Self::new("native-app-required", detail)
    }

    pub fn check(ok: bool, passed: impl Into<String>, failed: impl Into<String>) -> Self {
        if ok {
            Self::passed(passed)
        } else {
            Self::failed(failed)
        }
    }

    pub fn stopping(mut self, reason: impl Into<String>, reconcile: bool) -> Self {
        self.stop = Some((reason.into(), reconcile));
        if reconcile {
            self.outcome = "needs-reconciliation";
        }
        self
    }

    pub fn with(mut self, key: &str, value: Value) -> Self {
        self.observed.insert(key.to_owned(), value);
        self
    }

    pub fn id(mut self, key: &str, value: &str) -> Self {
        self.ids.insert(key.to_owned(), sanitize_id(value));
        self
    }
}

/// A check's identity and the workflow rows it evidences.
#[derive(Clone, Copy)]
pub(crate) struct Spec {
    pub id: &'static str,
    pub workflows: &'static [&'static str],
}

pub(crate) type Outcomes = Vec<(String, String)>;

/// What one family produced.
pub(crate) struct FamilyResult {
    pub outcomes: Outcomes,
    pub stop: Option<Stop>,
}

/// Options for one desk (a production core with this run's host).
#[derive(Clone, Default)]
pub(crate) struct DeskOptions {
    /// API Client requests may go to the network (production hosts).
    pub workbench_live: bool,
    pub folder: Option<PathBuf>,
    pub offline_root: Option<PathBuf>,
    pub workbench_root: Option<PathBuf>,
    pub save_dir: Option<PathBuf>,
    pub image: Option<Vec<u8>>,
    pub academy_origin: Option<String>,
}

/// A production core with in-memory OS adapters.
pub(crate) struct Desk {
    core: Core,
    handle: AuthorityHandle,
    session: String,
    sequence: u64,
    pub settle: Duration,
    pub confirmations: Arc<Confirmations>,
}

impl Desk {
    pub async fn send(&mut self, intent: Intent) -> Result<Receipt> {
        self.sequence += 1;
        self.handle
            .dispatch(
                Delivery {
                    session: self.session.clone(),
                    sequence: self.sequence,
                },
                intent,
            )
            .await
            .map_err(|r| invalid(format!("refused: {r}")))
    }

    /// Sends an intent written as JSON (the presentation's wire form). The
    /// JSON is consumed, so a secret in it moves into the intent.
    pub async fn send_json(&mut self, intent: Value) -> Result<Receipt> {
        let intent: Intent =
            serde_json::from_value(intent).map_err(|e| invalid(format!("intent: {e}")))?;
        self.send(intent).await
    }

    /// Sends and returns the admitted operation.
    pub async fn admit(&mut self, intent: Value) -> Result<desktop_core::runtime::OperationId> {
        match self.send_json(intent).await? {
            Receipt::Admitted {
                operation: Some(operation),
                ..
            } => Ok(operation),
            Receipt::Admitted { .. } => Err(invalid("admitted without an operation")),
        }
    }

    pub fn snap(&self) -> Result<ShellSnapshot> {
        match self.handle.observe(ViewScope::Shell) {
            Ok(Observation::Shell { snapshot, .. }) => Ok(snapshot),
            _ => Err(invalid("the shell snapshot is unavailable")),
        }
    }

    pub fn json(&self) -> Result<Value> {
        serde_json::to_value(self.snap()?).map_err(|e| invalid(format!("snapshot: {e}")))
    }

    pub fn context(&self) -> Result<ContextRef> {
        self.snap()?
            .active
            .map(|a| a.context)
            .ok_or_else(|| invalid("no active tenant"))
    }

    pub fn context_text(&self) -> Result<String> {
        Ok(self.context()?.as_str().to_owned())
    }

    /// Waits up to the desk's bound for `ready`.
    pub async fn wait(&self, ready: impl FnMut(&ShellSnapshot) -> bool) -> Result<ShellSnapshot> {
        self.wait_for(self.settle, ready).await
    }

    pub async fn wait_for(
        &self,
        bound: Duration,
        mut ready: impl FnMut(&ShellSnapshot) -> bool,
    ) -> Result<ShellSnapshot> {
        let deadline = tokio::time::Instant::now() + bound;
        loop {
            let snap = self.snap()?;
            if ready(&snap) {
                return Ok(snap);
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(invalid("timed out waiting for the core"));
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    /// Like [`Desk::wait`] over the snapshot's JSON (no error on timeout).
    pub async fn until(
        &self,
        bound: Duration,
        mut ready: impl FnMut(&Value) -> bool,
    ) -> Option<Value> {
        let deadline = tokio::time::Instant::now() + bound;
        loop {
            if let Ok(json) = self.json()
                && ready(&json)
            {
                return Some(json);
            }
            if tokio::time::Instant::now() >= deadline {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    /// Creates and selects a tenant (no credential).
    pub async fn tenant(
        &mut self,
        label: &str,
        tsg: Option<&str>,
        destination: &Destination,
    ) -> Result<()> {
        let before = self.snap()?.tenants.len();
        self.send_json(json!({"kind": "createTenant", "profile": {
            "label": label, "tsgId": tsg, "destination": destination}}))
            .await?;
        let snap = self.wait(|s| s.tenants.len() > before).await?;
        let id = snap
            .tenants
            .iter()
            .find(|t| t.label == label)
            .map(|t| t.id.clone())
            .ok_or_else(|| invalid("the created tenant is not listed"))?;
        self.send(Intent::SelectTenant {
            tenant: id.clone(),
            resolution: None,
        })
        .await?;
        self.wait(|s| s.active.as_ref().is_some_and(|a| a.tenant == id))
            .await?;
        Ok(())
    }

    fn active_tenant(&self, snap: &ShellSnapshot) -> Option<desktop_core::TenantView> {
        let active = snap.active.as_ref()?;
        snap.tenants.iter().find(|t| t.id == active.tenant).cloned()
    }

    /// The active tenant's scan key, for this session only.
    pub async fn scan_key(&mut self, key: &Zeroizing<String>) -> Result<()> {
        let context = self.context_text()?;
        self.send_json(
            json!({"kind": "configureScanCredential", "context": context,
            "mode": "session", "key": (**key).clone()}),
        )
        .await?;
        self.wait(|s| {
            self.active_tenant(s)
                .is_some_and(|t| t.scan.state == desktop_core::ScanState::Ready)
        })
        .await?;
        Ok(())
    }

    /// The active tenant's management OAuth client, for this session only.
    pub async fn management_key(
        &mut self,
        client_id: &str,
        secret: &Zeroizing<String>,
    ) -> Result<()> {
        let context = self.context_text()?;
        self.send_json(
            json!({"kind": "configureManagementCredential", "context": context,
            "mode": "session", "clientId": client_id, "secret": (**secret).clone()}),
        )
        .await?;
        self.wait(|s| {
            self.active_tenant(s)
                .is_some_and(|t| t.management.state == desktop_core::ScanState::Ready)
        })
        .await?;
        Ok(())
    }

    /// Whether the active tenant has a management capability.
    pub fn management_configured(&self) -> Result<bool> {
        let snap = self.snap()?;
        Ok(self
            .active_tenant(&snap)
            .is_some_and(|t| t.management.state != desktop_core::ScanState::Missing))
    }

    /// One management read (verify, page, detail, sessions, Red Team,
    /// Gateway…) through the `management` intent, and its single result.
    pub async fn read(&mut self, request: Value) -> Result<ManagementResult> {
        let context = self.context_text()?;
        let operation = self
            .admit(json!({"kind": "management", "context": context, "request": request}))
            .await?;
        let Ok(Observation::Management(handle)) =
            self.handle.observe(ViewScope::Management(operation))
        else {
            return Err(invalid("the management result cannot be observed"));
        };
        tokio::time::timeout(self.settle, handle.result())
            .await
            .map_err(|_| invalid("the management read did not settle in time"))
    }

    /// One API Client request through the `workbench` intent, and its
    /// result (as JSON).
    pub async fn workbench(&mut self, request: Value) -> Result<Value> {
        let operation = self
            .admit(json!({"kind": "workbench", "request": request}))
            .await?;
        let Ok(Observation::Workbench(handle)) =
            self.handle.observe(ViewScope::Workbench(operation))
        else {
            return Err(invalid("the API Client result cannot be observed"));
        };
        let result = tokio::time::timeout(self.settle, handle.result())
            .await
            .map_err(|_| invalid("the API Client request did not settle in time"))?;
        serde_json::to_value(result).map_err(|e| invalid(format!("result: {e}")))
    }

    /// One learner call through the `academyCall` intent family, and its
    /// result (as JSON).
    pub async fn academy(&mut self, intent: Value) -> Result<Value> {
        let operation = self.admit(intent).await?;
        let Ok(Observation::AcademyCall(handle)) =
            self.handle.observe(ViewScope::AcademyCall(operation))
        else {
            return Err(invalid("the learner call cannot be observed"));
        };
        let result = tokio::time::timeout(self.settle, handle.result())
            .await
            .map_err(|_| invalid("the learner call did not settle in time"))?;
        serde_json::to_value(result).map_err(|e| invalid(format!("result: {e}")))
    }

    /// The selected Gateway workspace's native reference, if any.
    pub fn workspace_ref(&self) -> Option<String> {
        self.json().ok()?["gateway"]["selected"]["reference"]
            .as_str()
            .map(str::to_owned)
    }

    /// Lists the Gateway workspaces and selects `workspace`; returns its
    /// reference.
    pub async fn select_workspace(&mut self, workspace: &str) -> Result<String> {
        let listed = self
            .read(json!({"kind": "gateway", "workspace": null,
                "read": {"kind": "inventory", "resource": "workspaces"}}))
            .await?;
        if let ManagementResult::Failed { failure, .. } = listed {
            return Err(invalid(format!(
                "the workspaces could not be listed: {failure:?}"
            )));
        }
        let context = self.context_text()?;
        self.send_json(json!({"kind": "selectGatewayWorkspace", "context": context,
            "workspace": workspace}))
            .await?;
        let found = self
            .until(self.settle, |j| {
                j["gateway"]["selected"]["workspace"]["id"].as_str() == Some(workspace)
            })
            .await;
        found
            .and_then(|j| {
                j["gateway"]["selected"]["reference"]
                    .as_str()
                    .map(str::to_owned)
            })
            .ok_or_else(|| invalid(format!("workspace {workspace} could not be selected")))
    }
}

impl Drop for Desk {
    fn drop(&mut self) {
        self.core.shutdown();
    }
}

/// One run's state.
pub(crate) struct Run<'a> {
    pub repo: &'a Repo,
    pub manifest: &'a LiveManifest,
    pub sha: &'a str,
    pub evidence: &'a Evidence,
    resolver: &'a dyn Resolve,
    pub operator: Arc<dyn Operator>,
    pub guard: Guard,
    secrets: Vec<Zeroizing<String>>,
    /// Run-owned folders (collections, offline work), removed at the end.
    pub scratch: PathBuf,
    /// What this run executes ([`implementation`]), in every receipt.
    pub implementation: BTreeMap<String, String>,
    /// The chunk whose evidence the receipts and run record belong to (C25
    /// for the C25 families; C31's live row writes under C31). The
    /// consumption ledger stays C25's: one append-only budget ledger.
    pub subject: Subject,
}

impl Run<'_> {
    pub fn family(&self, id: &str) -> Result<&Family> {
        self.manifest
            .families
            .get(id)
            .ok_or_else(|| invalid(format!("{id} is missing from the manifest")))
    }

    pub fn designated(&self, id: &str) -> bool {
        self.manifest
            .families
            .get(id)
            .is_some_and(|f| f.status == "designated")
    }

    pub fn nonsecret(&self, family: &str, key: &str) -> Option<String> {
        self.manifest
            .families
            .get(family)?
            .nonsecret
            .get(key)
            .cloned()
    }

    pub fn prefix(&self, family: &str) -> Option<String> {
        self.manifest
            .families
            .get(family)?
            .disposable_prefix
            .clone()
    }

    /// Resolves a credential now; `None` when the family names none.
    pub fn secret(&mut self, family: &str, name: &str) -> Result<Option<Zeroizing<String>>> {
        let Some(reference) = self.family(family)?.credentials.get(name).cloned() else {
            return Ok(None);
        };
        let value = self.resolver.resolve(&reference)?;
        self.secrets.push(value.clone());
        Ok(Some(value))
    }

    pub fn operator_timeout(&self, family: &str) -> Duration {
        let bound = self
            .nonsecret(family, "operator_timeout_secs")
            .and_then(|s| s.parse::<u64>().ok())
            .map(Duration::from_secs)
            .unwrap_or(OPERATOR_TIMEOUT);
        match self.evidence {
            Evidence::Live => bound,
            Evidence::Fixture(f) => f.settle.unwrap_or(Duration::from_secs(10)).min(bound),
        }
    }

    /// A fresh production core for one family.
    pub fn desk(&self, options: DeskOptions) -> Result<Desk> {
        let mut config = CoreConfig::default();
        if options.workbench_live && matches!(self.evidence, Evidence::Live) {
            config.workbench_network = desktop_core::workbench::admission::Network::Live;
        }
        if let Some(origin) = &options.academy_origin {
            config.academy_origin = origin.clone();
        }
        if let Evidence::Fixture(f) = self.evidence {
            if let Some(origin) = &f.management {
                config.management = ManagementEndpoints::at_origin(origin);
            }
            if let Some(origin) = &f.academy {
                config.academy_origin = origin.clone();
            }
        }
        let confirmations = Arc::new(Confirmations::default());
        let host = Host {
            operator: Arc::clone(&self.operator),
            confirmations: Arc::clone(&confirmations),
            folder: options.folder,
            offline_root: options.offline_root,
            workbench_root: options.workbench_root,
            save_dir: options.save_dir,
            image: options.image,
        };
        let core = Core::new(config, crate::live_adapters::adapters(&host));
        core.start();
        if let Evidence::Fixture(f) = self.evidence {
            core.set_transport_policy(TransportPolicy {
                revision: 2,
                proxy: ProxyMode::Direct,
                extra_roots: f.extra_roots.clone(),
            })
            .map_err(|e| invalid(format!("transport policy: {e}")))?;
        }
        let handle = core.operator();
        let session = handle
            .register_delivery_session()
            .map_err(|e| invalid(format!("delivery session: {e:?}")))?
            .nonce;
        Ok(Desk {
            core,
            handle,
            session,
            sequence: 0,
            settle: self.evidence.settle(),
            confirmations,
        })
    }

    /// A new run-owned folder.
    pub fn folder(&self, name: &str) -> Result<PathBuf> {
        let dir = self.scratch.join(format!("{name}-{}", random_hex(4)?));
        std::fs::create_dir_all(&dir).map_err(crate::io_err(&dir))?;
        dir.canonicalize().map_err(crate::io_err(&dir))
    }

    /// The family's tenant destination: `region` or `origin` (US region by
    /// default for families that only manage, never scan).
    pub fn destination(&self, family: &str) -> std::result::Result<Destination, String> {
        let f = self
            .manifest
            .families
            .get(family)
            .ok_or_else(|| format!("{family} is missing"))?;
        destination(f, true)
    }

    /// Writes the receipt, refusing one that contains any resolved secret.
    pub fn record(
        &self,
        family: &str,
        spec: Spec,
        started: String,
        inputs: BTreeMap<String, Value>,
        found: &Found,
        budget: Option<&str>,
    ) -> Result<()> {
        let receipt = CheckReceipt {
            schema: RECEIPT_SCHEMA.to_owned(),
            evidence: self.evidence.class().to_owned(),
            family: family.to_owned(),
            check: spec.id.to_owned(),
            workflows: spec.workflows.iter().map(|w| (*w).to_owned()).collect(),
            environment: self.family(family)?.environment.clone(),
            manifest_sha256: self.sha.to_owned(),
            started_utc: started,
            finished_utc: now_utc(),
            inputs,
            outcome: found.outcome.to_owned(),
            detail: found.detail.clone(),
            observed: found.observed.clone(),
            ids: found.ids.clone(),
            readback: found.readback.clone(),
            cleanup: found.cleanup.clone(),
            // The caps this check was charged against (L18 may share L01's).
            consumption: self.guard.consumption(budget.unwrap_or(family)),
            operator_step: found.operator_step.clone(),
            implementation: self.implementation.clone(),
        };
        let bytes =
            serde_json::to_vec_pretty(&receipt).map_err(|e| invalid(format!("receipt: {e}")))?;
        let text = String::from_utf8_lossy(&bytes);
        if self
            .secrets
            .iter()
            .any(|s| s.len() >= 4 && text.contains(s.as_str()))
        {
            return Err(invalid(format!(
                "{family}/{}: the receipt would contain a secret; nothing was written",
                spec.id
            )));
        }
        write_json(
            &self
                .repo
                .evidence_dir(self.subject)
                .join("live")
                .join(family)
                .join(format!("{}.json", spec.id)),
            &receipt,
        )
    }

    /// Records `found` and folds it into `outcomes`; returns whether the run
    /// must stop.
    #[allow(clippy::too_many_arguments)]
    pub fn settle(
        &self,
        family: &str,
        spec: Spec,
        started: String,
        inputs: BTreeMap<String, Value>,
        found: Found,
        budget: Option<&str>,
        outcomes: &mut Outcomes,
    ) -> Result<Option<Stop>> {
        self.record(family, spec, started, inputs, &found, budget)?;
        outcomes.push((spec.id.to_owned(), found.outcome.to_owned()));
        Ok(found.stop.map(|(reason, reconcile)| Stop {
            family: family.to_owned(),
            check: spec.id.to_owned(),
            reason,
            reconcile,
        }))
    }

    /// The family's non-secret inputs, with the named credentials (by name).
    pub fn inputs(&self, family: &str, credentials: &[&str]) -> BTreeMap<String, Value> {
        let mut inputs: BTreeMap<String, Value> = self
            .manifest
            .families
            .get(family)
            .map(|f| {
                f.nonsecret
                    .iter()
                    .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                    .collect()
            })
            .unwrap_or_default();
        if !credentials.is_empty() {
            inputs.insert(
                "credentials".to_owned(),
                json!(
                    credentials
                        .iter()
                        .map(|c| format!("{family}.{c}"))
                        .collect::<Vec<_>>()
                ),
            );
        }
        inputs.insert("storage".to_owned(), json!("session"));
        inputs
    }

    /// Opens a desk with the family's tenant and management client; on a
    /// missing input, a blocked reason.
    pub async fn management_desk(
        &mut self,
        family: &str,
        options: DeskOptions,
    ) -> Result<std::result::Result<Desk, String>> {
        self.management_desk_as(family, "client_id", "client_secret", options)
            .await
    }

    /// As [`Run::management_desk`] with another principal's names.
    pub async fn management_desk_as(
        &mut self,
        family: &str,
        id_key: &str,
        secret_key: &str,
        options: DeskOptions,
    ) -> Result<std::result::Result<Desk, String>> {
        let Some(client_id) = self.nonsecret(family, id_key) else {
            return Ok(Err(format!("{family} names no {id_key}")));
        };
        let Some(tsg) = self.nonsecret(family, "tsg_id") else {
            return Ok(Err(format!("{family} names no tsg_id")));
        };
        let destination = match self.destination(family) {
            Ok(d) => d,
            Err(e) => return Ok(Err(e)),
        };
        let Some(secret) = self.secret(family, secret_key)? else {
            return Ok(Err(format!(
                "{family} designates no {secret_key} credential"
            )));
        };
        let mut desk = self.desk(options)?;
        desk.tenant(&format!("C25 {family}"), Some(&tsg), &destination)
            .await?;
        desk.management_key(&client_id, &secret).await?;
        Ok(Ok(desk))
    }
}

pub(crate) fn destination(
    family: &Family,
    default_us: bool,
) -> std::result::Result<Destination, String> {
    match (
        family.nonsecret.get("region"),
        family.nonsecret.get("origin"),
    ) {
        (Some(r), None) => Ok(Destination::Region {
            region: match r.as_str() {
                "us" => Region::UnitedStates,
                "eu" => Region::Europe,
                "in" => Region::India,
                "sg" => Region::Singapore,
                other => return Err(format!("unknown region {other}")),
            },
        }),
        (None, Some(o)) => Destination::Custom { origin: o.clone() }
            .validate()
            .ok_or_else(|| format!("{o} is not an HTTPS origin")),
        (None, None) if default_us => Ok(Destination::Region {
            region: Region::UnitedStates,
        }),
        _ => Err("exactly one of region or origin is required".to_owned()),
    }
}

/// Waits for an operator step: prompts, then polls `done` (the core's own
/// state) until it holds or the bound passes. The result says what the
/// operator was asked and how long it took; the runner never completes it.
pub(crate) async fn operator_step(
    operator: &dyn Operator,
    kind: &str,
    instruction: &str,
    url: Option<&url::Url>,
    bound: Duration,
    mut done: impl AsyncFnMut() -> bool,
) -> (bool, Value) {
    let prompted = now_utc();
    operator.prompt(instruction, url);
    let start = tokio::time::Instant::now();
    let mut completed = false;
    while start.elapsed() < bound {
        if done().await {
            completed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    (
        completed,
        json!({"kind": kind, "instruction": instruction, "prompted_utc": prompted,
               "completed": completed, "finished_utc": now_utc(),
               "bound_secs": bound.as_secs(),
               "judged_by": "the core's own state; the runner never supplies the result"}),
    )
}

/// A management result as recorded: its kind and shape, never item content.
pub(crate) fn summarize(result: &ManagementResult) -> Value {
    match result {
        ManagementResult::Verified => json!({"result": "verified"}),
        ManagementResult::Page { page } => json!({"result": "page",
            "resource": page.resource, "offset": page.offset, "limit": page.limit,
            "count": page.count, "next_offset": page.next_offset,
            "complete": page.complete, "total": page.total}),
        ManagementResult::Failed { failure, outcome } => {
            json!({"result": "failed", "failure": failure, "outcome": outcome})
        }
        ManagementResult::Gateway {
            gateway: desktop_core::gateway::GatewayResult::List { list },
        } => json!({"result": "gatewayList", "resource": list.resource, "count": list.count,
            "total": list.total, "complete": list.complete}),
        other => {
            let kind = serde_json::to_value(other)
                .ok()
                .and_then(|v| v.get("kind").and_then(Value::as_str).map(str::to_owned))
                .unwrap_or_else(|| "other".to_owned());
            json!({"result": kind})
        }
    }
}

fn summary_status(id: &str, outcomes: &Outcomes, stop: &Option<Stop>) -> FamilyRun {
    let status = if stop.as_ref().is_some_and(|s| s.family == id) {
        "stopped"
    } else if outcomes.iter().all(|(_, o)| o == "passed") && !outcomes.is_empty() {
        "passed"
    } else if outcomes.iter().any(|(_, o)| o == "failed") {
        "failed"
    } else if outcomes.iter().any(|(_, o)| o == "native-app-required")
        && outcomes
            .iter()
            .all(|(_, o)| o == "passed" || o == "native-app-required")
    {
        "needs-native-app"
    } else {
        "blocked"
    };
    FamilyRun {
        id: id.to_owned(),
        status: status.to_owned(),
        checks: outcomes.iter().map(|(c, o)| format!("{c}: {o}")).collect(),
    }
}

/// Runs `families` (every designated family when empty) and writes
/// `evidence/C25/live/run.json`. Refuses a manifest `live::check` rejects
/// and any requested family that is blocked.
pub async fn run(
    repo: &Repo,
    manifest: &LiveManifest,
    sha: &str,
    resolver: &dyn Resolve,
    operator: Arc<dyn Operator>,
    evidence: Evidence,
    families: &[String],
) -> Result<RunRecord> {
    run_for(
        repo,
        manifest,
        sha,
        resolver,
        operator,
        evidence,
        families,
        Subject::Chunk(25),
    )
    .await
}

/// [`run`] with the receipts and run record under `subject`'s evidence
/// (a later chunk's live row run inside a C25 family's designation).
#[allow(clippy::too_many_arguments)]
pub async fn run_for(
    repo: &Repo,
    manifest: &LiveManifest,
    sha: &str,
    resolver: &dyn Resolve,
    operator: Arc<dyn Operator>,
    evidence: Evidence,
    families: &[String],
    subject: Subject,
) -> Result<RunRecord> {
    let failures = live::check(repo, manifest);
    if !failures.is_empty() {
        return Err(invalid(format!(
            "the live manifest is not acceptable: {}",
            failures.join("; ")
        )));
    }
    let requested: Vec<String> = if families.is_empty() {
        manifest
            .families
            .iter()
            .filter(|(_, f)| f.status == "designated")
            .map(|(id, _)| id.clone())
            .collect()
    } else {
        families.to_vec()
    };
    for id in &requested {
        match manifest.families.get(id) {
            None => return Err(invalid(format!("{id} is not a live family"))),
            Some(f) if f.status != "designated" => {
                return Err(invalid(format!("{id} is blocked: {}", f.reason)));
            }
            Some(_) => {}
        }
    }
    let implementation = implementation(repo, matches!(evidence, Evidence::Live))?;
    let started = now_utc();
    let scratch = std::env::temp_dir()
        .canonicalize()
        .map_err(|e| invalid(format!("temporary directory: {e}")))?
        .join(format!("airs-v2-live-{}", random_hex(6)?));
    let mut run = Run {
        repo,
        manifest,
        sha,
        evidence: &evidence,
        resolver,
        operator,
        guard: Guard::with_ledger(
            manifest,
            &repo
                .evidence_dir(Subject::Chunk(25))
                .join("live")
                .join(match &evidence {
                    Evidence::Live => LIVE_LEDGER,
                    Evidence::Fixture(_) => FIXTURE_LEDGER,
                }),
            // Runs may start within the same second.
            &format!("{started}/{}", random_hex(4)?),
            sha,
        )?,
        secrets: Vec::new(),
        scratch,
        implementation: implementation.clone(),
        subject,
    };
    let run_implementation = implementation;
    // Each requested family's earlier receipts move to live/history/ so a
    // check this run does not reach never shows an older run's result.
    let live_dir = repo.evidence_dir(subject).join("live");
    let history = live_dir.join("history").join(started.replace(':', ""));
    for id in &requested {
        let dir = live_dir.join(id);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                let to = history.join(id);
                std::fs::create_dir_all(&to).map_err(io_err(&to))?;
                let target = to.join(entry.file_name());
                std::fs::rename(&path, &target).map_err(io_err(&path))?;
            }
        }
    }
    let mut results = Vec::new();
    let mut stopped: Option<Stop> = None;
    // The Academy families share one learner sign-in.
    let academy: Vec<String> = requested
        .iter()
        .filter(|id| crate::live_academy::FAMILIES.contains(&id.as_str()))
        .cloned()
        .collect();
    let mut academy_done = false;
    for id in &requested {
        if stopped.is_some() {
            results.push(FamilyRun {
                id: id.clone(),
                status: "blocked".to_owned(),
                checks: vec!["not run: the run stopped".to_owned()],
            });
            continue;
        }
        let result = match id.as_str() {
            "L01" | "L18" => crate::live_scan::run(&mut run, id).await?,
            "L02" | "L05" | "L06" | "L07" | "L10" => crate::live_reads::run(&mut run, id).await?,
            "L03" | "L04" | "L08" => crate::live_changes::run(&mut run, id).await?,
            "L09" => crate::live_inference::run(&mut run, id).await?,
            "L15" => crate::live_workbench::run(&mut run, id).await?,
            "L16" => native_family(&mut run, id)?,
            "L17" => crate::live_mcp::run(&mut run, id).await?,
            _ if academy.contains(id) => {
                if academy_done {
                    continue;
                }
                academy_done = true;
                let shared = crate::live_academy::run(&mut run, &academy).await?;
                for (family, outcomes) in shared.families {
                    results.push(summary_status(&family, &outcomes, &shared.stop));
                }
                stopped = shared.stop;
                continue;
            }
            _ => return Err(invalid(format!("{id} has no runner"))),
        };
        results.push(summary_status(id, &result.outcomes, &result.stop));
        stopped = result.stop;
    }
    let _ = std::fs::remove_dir_all(&run.scratch);
    let record = RunRecord {
        schema: RUN_SCHEMA.to_owned(),
        evidence: evidence.class().to_owned(),
        manifest_sha256: sha.to_owned(),
        started_utc: started,
        finished_utc: now_utc(),
        families: results,
        stopped,
        implementation: run_implementation,
    };
    write_json(
        &repo.evidence_dir(subject).join("live").join("run.json"),
        &record,
    )?;
    Ok(record)
}

/// L16: the native Harness engine runs only inside the desktop app.
fn native_family(run: &mut Run<'_>, id: &str) -> Result<FamilyResult> {
    const CHECKS: [Spec; 4] = [
        Spec {
            id: "L16-engine-conversation",
            workflows: &["W42"],
        },
        Spec {
            id: "L16-company-sign-in",
            workflows: &["W43"],
        },
        Spec {
            id: "L16-history-restart",
            workflows: &["W44"],
        },
        Spec {
            id: "L16-proposal-review",
            workflows: &["W45"],
        },
    ];
    let mut outcomes = Vec::new();
    for spec in CHECKS {
        let found = Found::native(
            "the sealed Harness engine is launched only by the desktop host (its process-group \
             launcher and bundled engine resource); protected disk, process and sandbox \
             assertions need the signed app. Run this check with the native acceptance driver \
             against the designated scope.",
        );
        run.settle(
            id,
            spec,
            now_utc(),
            run.inputs(id, &[]),
            found,
            None,
            &mut outcomes,
        )?;
    }
    Ok(FamilyResult {
        outcomes,
        stop: None,
    })
}

/// `xtask live-run`: a live run against the designated services.
pub fn main(
    repo: &Repo,
    scope: &std::path::Path,
    families: &[String],
    subject: Subject,
) -> Result<RunRecord> {
    let (manifest, sha) = live::load(repo, scope)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| invalid(format!("runtime: {e}")))?;
    runtime.block_on(run_for(
        repo,
        &manifest,
        &sha,
        &SystemResolver,
        Arc::new(TerminalOperator),
        Evidence::Live,
        families,
        subject,
    ))
}
