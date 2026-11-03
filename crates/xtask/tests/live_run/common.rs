//! Shared fixtures: an in-memory secret resolver, a stand-in operator, a
//! manifest builder and receipt helpers.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use xtask::Repo;
use xtask::live::{self, LiveManifest, SecretRef};
use xtask::live_adapters::Operator;
use xtask::live_run::{self, CheckReceipt, Evidence, FixtureTargets, Resolve, RunRecord};
use zeroize::Zeroizing;

use crate::support::{Reply, Seen, TestCa, TlsServer};

static NEXT: AtomicUsize = AtomicUsize::new(0);

pub fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "airs-v2-live-run-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).expect("scratch");
    dir
}

/// Secrets by Keychain account, held only by the test.
pub struct Secrets(pub BTreeMap<String, String>);

impl Resolve for Secrets {
    fn resolve(&self, reference: &SecretRef) -> xtask::Result<Zeroizing<String>> {
        let SecretRef::Keychain { account, .. } = reference else {
            panic!("tests use keychain references only");
        };
        Ok(Zeroizing::new(
            self.0.get(account).expect("known account").clone(),
        ))
    }
}

pub fn secrets(pairs: &[(&str, &str)]) -> Secrets {
    Secrets(
        pairs
            .iter()
            .map(|(a, v)| ((*a).to_owned(), (*v).to_owned()))
            .collect(),
    )
}

pub fn keychain(account: &str) -> Value {
    json!({"kind": "keychain", "service": "io.cdot.airs-desktop.c25", "account": account})
}

type Prompted = dyn Fn(&str, Option<&url::Url>) + Send + Sync;

/// The operator stand-in: records each prompt and acts like the person in
/// the browser (through `act`).
pub struct Browser {
    pub prompts: Mutex<Vec<(String, Option<String>)>>,
    act: Box<Prompted>,
}

impl Browser {
    pub fn new(act: impl Fn(&str, Option<&url::Url>) + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            prompts: Mutex::new(Vec::new()),
            act: Box::new(act),
        })
    }

    pub fn idle() -> Arc<Self> {
        Self::new(|_, _| {})
    }

    pub fn prompts(&self) -> Vec<(String, Option<String>)> {
        self.prompts.lock().expect("prompts").clone()
    }
}

impl Operator for Browser {
    fn prompt(&self, instruction: &str, url: Option<&url::Url>) {
        self.prompts
            .lock()
            .expect("prompts")
            .push((instruction.to_owned(), url.map(|u| u.to_string())));
        (self.act)(instruction, url);
    }
}

/// Every family blocked; tests designate the ones they drive.
pub fn manifest() -> Value {
    let mut families = serde_json::Map::new();
    for id in live::families() {
        families.insert(
            id.clone(),
            json!({"status": "blocked", "reason": "not designated in this fixture"}),
        );
    }
    json!({
        "schema": live::SCHEMA,
        "owner_approval": {"approved_by": "owner", "approved_utc": "2026-10-25T00:00:00Z", "decision": "fixture"},
        "environments": {"staging-a": {"kind": "staging"}, "test-b": {"kind": "test"}},
        "families": families
    })
}

pub fn designate(manifest: &mut Value, id: &str, mut family: Value) {
    family["status"] = json!("designated");
    if family.get("environment").is_none() {
        family["environment"] = json!("staging-a");
    }
    if family.get("permitted_operations").is_none() {
        family["permitted_operations"] = json!(["read"]);
    }
    if family.get("irreversible_exclusions").is_none() {
        family["irreversible_exclusions"] = json!(["production tenants"]);
    }
    manifest["families"][id] = family;
}

pub struct Fixture {
    pub ca: TestCa,
    pub management: Option<String>,
    pub academy: Option<String>,
}

impl Fixture {
    pub fn new() -> Self {
        Self {
            ca: TestCa::new("airs-v2 c25 ca"),
            management: None,
            academy: None,
        }
    }

    pub async fn serve(&self, reply: Reply) -> TlsServer {
        TlsServer::start(self.ca.leaf(&["localhost"]), reply).await
    }

    pub fn evidence(&self) -> Evidence {
        Evidence::Fixture(FixtureTargets {
            extra_roots: vec![self.ca.pem.as_bytes().to_vec()],
            management: self.management.clone(),
            academy: self.academy.clone(),
            settle: Some(Duration::from_secs(10)),
        })
    }

    pub async fn run(
        &self,
        value: Value,
        families: &[&str],
        secrets: &Secrets,
        operator: Arc<dyn Operator>,
    ) -> (Repo, xtask::Result<RunRecord>) {
        let repo = Repo::new(scratch());
        let record = self.run_in(&repo, value, families, secrets, operator).await;
        (repo, record)
    }

    /// One run in an existing repository, so that runs can follow each
    /// other there.
    pub async fn run_in(
        &self,
        repo: &Repo,
        value: Value,
        families: &[&str],
        secrets: &Secrets,
        operator: Arc<dyn Operator>,
    ) -> xtask::Result<RunRecord> {
        let manifest: LiveManifest = serde_json::from_value(value).expect("manifest");
        let families: Vec<String> = families.iter().map(|f| (*f).to_owned()).collect();
        live_run::run(
            repo,
            &manifest,
            "f".repeat(64).as_str(),
            secrets,
            operator,
            self.evidence(),
            &families,
        )
        .await
    }
}

pub fn receipt(repo: &Repo, family: &str, check: &str) -> CheckReceipt {
    let path = repo.root().join(format!(
        "docs/planning/evidence/C25/live/{family}/{check}.json"
    ));
    serde_json::from_slice(&std::fs::read(&path).unwrap_or_else(|_| panic!("receipt {check}")))
        .expect("receipt json")
}

pub fn outcomes(record: &RunRecord, family: &str) -> Vec<String> {
    record
        .families
        .iter()
        .find(|f| f.id == family)
        .map(|f| f.checks.clone())
        .unwrap_or_default()
}

fn every_file(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            every_file(&path, out);
        } else {
            out.push(path);
        }
    }
}

pub fn assert_no_secret(repo: &Repo, secrets: &[&str]) {
    let mut files = Vec::new();
    every_file(repo.root(), &mut files);
    assert!(!files.is_empty());
    for file in files {
        let text = std::fs::read_to_string(&file).expect("text");
        for secret in secrets {
            assert!(!text.contains(secret), "{} holds a secret", file.display());
        }
    }
}

pub fn raw(status: u16, body: &Value) -> Reply {
    let body = serde_json::to_vec(body).expect("json");
    Reply::Raw {
        head: format!(
            "HTTP/1.1 {status} Status\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close",
            body.len()
        ),
        body,
    }
}

/// The request's method, path and query parameters.
pub fn parts(seen: &Seen) -> (String, String, BTreeMap<String, String>) {
    let mut words = seen.request_line.split(' ');
    let method = words.next().unwrap_or("").to_owned();
    let target = words.next().unwrap_or("");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let query = url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();
    (method, path.to_owned(), query)
}

/// A stateful router shared with the test.
pub type Router<S> = Arc<Mutex<S>>;

pub fn routed<S: Send + 'static>(state: &Router<S>, route: fn(&mut S, &Seen) -> Reply) -> Reply {
    let state = Arc::clone(state);
    Reply::Route(Arc::new(move |seen| {
        route(&mut state.lock().expect("state"), seen)
    }))
}

pub const CLIENT_ID: &str = "c25-client@1234567890.iam.panserviceaccount.com";
pub const CLIENT_SECRET: &str = "synthetic-client-secret-c25";
pub const RESTRICTED_ID: &str = "c25-restricted@1234567890.iam.panserviceaccount.com";
pub const RESTRICTED_SECRET: &str = "synthetic-restricted-secret-c25";
pub const TSG: &str = "1234567890";

/// The management family entries every management-backed family shares.
pub fn management_family(extra: Value) -> Value {
    let mut family = json!({
        "credentials": {"client_secret": keychain("client"), "restricted_client_secret": keychain("restricted")},
        "nonsecret": {"client_id": CLIENT_ID, "restricted_client_id": RESTRICTED_ID, "tsg_id": TSG},
    });
    if let Value::Object(extra) = extra {
        for (k, v) in extra {
            if k == "nonsecret" || k == "credentials" {
                if let (Some(base), Value::Object(more)) = (family[&k].as_object_mut(), v) {
                    base.extend(more);
                }
            } else {
                family[k] = v;
            }
        }
    }
    family
}

pub fn management_secrets(extra: &[(&str, &str)]) -> Secrets {
    let mut pairs = vec![("client", CLIENT_SECRET), ("restricted", RESTRICTED_SECRET)];
    pairs.extend_from_slice(extra);
    secrets(&pairs)
}

/// The OAuth grant for the designated client, or a refusal; the restricted
/// client is granted too (its reads are denied by the service).
pub fn token(seen: &Seen) -> Option<Reply> {
    let (_, path, _) = parts(seen);
    if path != "/oauth2/access_token" {
        return None;
    }
    let auth = seen.header("authorization").unwrap_or_default();
    let restricted = auth.contains(&b64(&format!(
        "{}:{RESTRICTED_SECRET}",
        RESTRICTED_ID.replace('@', "%40")
    )));
    let main = auth.contains(&b64(&format!(
        "{}:{CLIENT_SECRET}",
        CLIENT_ID.replace('@', "%40")
    )));
    Some(if main {
        raw(
            200,
            &json!({"access_token": "c25-access-main", "token_type": "Bearer", "expires_in": 3600}),
        )
    } else if restricted {
        raw(
            200,
            &json!({"access_token": "c25-access-restricted", "token_type": "Bearer", "expires_in": 3600}),
        )
    } else {
        raw(401, &json!({"error": "invalid_client"}))
    })
}

/// Whether the request carries the restricted principal's token.
pub fn restricted(seen: &Seen) -> bool {
    seen.header("authorization") == Some("Bearer c25-access-restricted")
}

pub fn b64(input: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = input.as_bytes();
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Waits up to 10 s for `condition` (revocations settle after the run).
pub async fn within(mut condition: impl FnMut() -> bool) -> bool {
    for _ in 0..1000 {
        if condition() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    condition()
}
