//! Adversarial tests for the C25 controlled live scope.
//!
//! A complete owner-approved manifest passes. Each case breaks one rule, so
//! every rejection below is caused by the single mutation it names. No test
//! resolves a secret reference or calls a service.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};
use xtask::Repo;
use xtask::ledger::Ledger;
use xtask::live::{self, LiveManifest};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn repo() -> Repo {
    Repo::discover()
}

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "airs-v2-controlled-live-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn keychain(account: &str) -> Value {
    json!({"kind": "keychain", "service": "io.cdot.prisma-airs-desktop.v2.c25", "account": account})
}

/// Every family designated, the mutating ones with names and cleanup, the
/// consuming ones with budgets, L18 in its own environment.
fn baseline() -> Value {
    let mut families = serde_json::Map::new();
    for id in live::families() {
        let isolated = id == "L18";
        let mut family = json!({
            "status": "designated",
            "environment": if isolated { "test-b" } else { "staging-a" },
            "credentials": {"primary": keychain(&format!("{id}-{}", if isolated { "b" } else { "a" }))},
            "nonsecret": {"endpoint": "https://service.example.test/api", "tsg_id": "1234567890", "client_id": "0b4f6a3e-2c1d-4e5f-8a9b-7c6d5e4f3a2b"},
            "permitted_operations": ["read"],
            "irreversible_exclusions": ["production tenants"]
        });
        if live::MUTATING.contains(&id.as_str()) {
            family["disposable_prefix"] = json!("airs-v2-c25-");
            family["cleanup"] =
                json!(["delete every airs-v2-c25- resource and read back its absence"]);
        }
        if live::CONSUMING.contains(&id.as_str()) {
            family["budget"] = json!({"requests": 50});
        }
        families.insert(id, family);
    }
    json!({
        "schema": live::SCHEMA,
        "owner_approval": {"approved_by": "owner", "approved_utc": "2026-10-25T00:00:00Z", "decision": "D0xx"},
        "environments": {"staging-a": {"kind": "staging"}, "test-b": {"kind": "test"}},
        "families": families
    })
}

fn failures(value: &Value) -> Vec<String> {
    let manifest: LiveManifest = serde_json::from_value(value.clone()).expect("manifest parses");
    live::check(&repo(), &manifest)
}

fn rejected(edit: impl FnOnce(&mut Value), needle: &str) {
    let mut value = baseline();
    edit(&mut value);
    let found = failures(&value);
    assert!(
        found.iter().any(|f| f.contains(needle)),
        "expected a failure containing {needle:?}, got {found:?}"
    );
}

fn write_private(dir: &Path, value: &Value, mode: u32) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("live-manifest.json");
    std::fs::write(&path, serde_json::to_vec_pretty(value).expect("fixture")).expect("fixture");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).expect("fixture");
    path
}

fn ledger() -> Ledger {
    let path = repo().join("docs/architecture/workflow-ledger.json");
    serde_json::from_slice(&std::fs::read(path).expect("fixture")).expect("fixture")
}

#[test]
fn a_complete_private_manifest_passes() {
    assert_eq!(failures(&baseline()), Vec::<String>::new());
    let path = write_private(&scratch(), &baseline(), 0o600);
    let (_, sha) = live::load(&repo(), &path).expect("baseline loads");
    assert_eq!(sha.len(), 64);
}

#[test]
fn the_manifest_must_be_approved_and_current() {
    rejected(
        |v| v["owner_approval"]["approved_by"] = json!(""),
        "owner approval is incomplete",
    );
    rejected(
        |v| v["owner_approval"]["decision"] = json!(" "),
        "owner approval is incomplete",
    );
    rejected(
        |v| v["schema"] = json!("airs-v2/live-manifest/0"),
        "is not airs-v2/live-manifest/1",
    );
}

#[test]
fn production_is_never_an_environment() {
    rejected(
        |v| v["environments"]["staging-a"]["kind"] = json!("production"),
        "only staging or test",
    );
}

#[test]
fn every_family_is_present_and_a_blocked_one_says_why() {
    rejected(
        |v| {
            v["families"]
                .as_object_mut()
                .expect("fixture")
                .remove("L05");
        },
        "L05 is missing",
    );
    rejected(
        |v| v["families"]["L19"] = v["families"]["L01"].clone(),
        "L19 is not a live family",
    );
    rejected(
        |v| v["families"]["L06"] = json!({"status": "blocked", "reason": ""}),
        "L06 is blocked without a reason",
    );
    rejected(
        |v| v["families"]["L06"]["status"] = json!("skipped"),
        "L06 status skipped",
    );
    let mut value = baseline();
    value["families"]["L06"] = json!({"status": "blocked", "reason": "no seeded Red Team records"});
    assert_eq!(failures(&value), Vec::<String>::new());
}

#[test]
fn a_designated_family_names_its_environment_and_operations() {
    rejected(
        |v| v["families"]["L02"]["environment"] = json!("nowhere"),
        "L02 names unknown environment",
    );
    rejected(
        |v| v["families"]["L07"]["permitted_operations"] = json!([]),
        "L07 is designated with no permitted operations",
    );
}

#[test]
fn mutating_families_need_disposable_names_and_cleanup() {
    rejected(
        |v| {
            v["families"]["L03"]
                .as_object_mut()
                .expect("fixture")
                .remove("disposable_prefix");
        },
        "L03 changes remote state",
    );
    rejected(
        |v| v["families"]["L04"]["disposable_prefix"] = json!(" "),
        "L04 changes remote state",
    );
    rejected(
        |v| v["families"]["L08"]["cleanup"] = json!([]),
        "L08 changes remote state",
    );
    rejected(
        |v| v["families"]["L17"]["cleanup"] = json!([]),
        "L17 changes remote state",
    );
}

#[test]
fn consuming_families_need_a_budget() {
    for id in live::CONSUMING {
        rejected(
            |v| v["families"][id]["budget"] = json!({}),
            &format!("{id} consumes service capacity"),
        );
    }
}

#[test]
fn isolation_needs_its_own_environment_and_credentials() {
    rejected(
        |v| v["families"]["L18"]["environment"] = json!("staging-a"),
        "L18 shares L01's environment",
    );
    rejected(
        |v| v["families"]["L18"]["credentials"]["primary"] = keychain("L01-a"),
        "L18 shares a credential reference with L01",
    );
}

#[test]
fn secrets_are_referenced_never_written() {
    rejected(
        |v| v["families"]["L01"]["nonsecret"]["note"] = json!("-----BEGIN PRIVATE KEY-----"),
        "L01 nonsecret note looks like a secret value",
    );
    rejected(
        |v| {
            v["families"]["L01"]["nonsecret"]["note"] =
                json!("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ4In0.c2ln")
        },
        "L01 nonsecret note looks like a secret value",
    );
    rejected(
        |v| {
            v["families"]["L01"]["nonsecret"]["note"] =
                json!("Zk9xR2pLbTNuQjV2QzhYejFXcTRUeTdVaTBPcA")
        },
        "L01 nonsecret note looks like a secret value",
    );
    rejected(
        |v| v["families"]["L02"]["nonsecret"]["client_secret"] = json!("x"),
        "L02 nonsecret client_secret names a secret",
    );
    rejected(
        |v| v["families"]["L01"]["nonsecret"]["api-key"] = json!("x"),
        "L01 nonsecret api-key names a secret",
    );
}

#[test]
fn secret_references_stay_in_v2_test_storage_outside_the_repository() {
    rejected(
        |v| {
            v["families"]["L01"]["credentials"]["primary"] = json!({"kind": "keychain", "service": "io.cdot.prisma-airs-desktop", "account": "x"})
        },
        "L01 credential primary names Keychain service io.cdot.prisma-airs-desktop;",
    );
    rejected(
        |v| {
            v["families"]["L01"]["credentials"]["primary"] =
                json!({"kind": "keychain", "service": "com.example.other", "account": "x"})
        },
        "only io.cdot.prisma-airs-desktop.v2.* test items",
    );
    rejected(
        |v| v["families"]["L01"]["credentials"]["primary"]["account"] = json!(""),
        "L01 credential primary has no Keychain account",
    );
    let inside = repo().join("docs/secret.key").display().to_string();
    rejected(
        |v| {
            v["families"]["L01"]["credentials"]["primary"] = json!({"kind": "file", "path": inside})
        },
        "L01 credential primary file is inside the repository",
    );
    rejected(
        |v| {
            v["families"]["L01"]["credentials"]["primary"] =
                json!({"kind": "file", "path": "relative/secret"})
        },
        "file path is not absolute",
    );
    let mut value = baseline();
    value["families"]["L01"]["credentials"]["primary"] =
        json!({"kind": "file", "path": "/Users/operator/private/airs-l01-scan-key"});
    assert_eq!(failures(&value), Vec::<String>::new());
}

#[test]
fn the_manifest_file_is_private_and_strict() {
    let dir = scratch();
    let shared = write_private(&dir, &baseline(), 0o644);
    let err = live::load(&repo(), &shared)
        .expect_err("rejected")
        .to_string();
    assert!(err.contains("readable by group or others"), "{err}");

    let inside = repo().join("target/controlled-live-test-manifest.json");
    std::fs::create_dir_all(inside.parent().expect("fixture")).expect("fixture");
    std::fs::write(&inside, serde_json::to_vec(&baseline()).expect("fixture")).expect("fixture");
    let err = live::load(&repo(), &inside)
        .expect_err("rejected")
        .to_string();
    std::fs::remove_file(&inside).ok();
    assert!(err.contains("inside the repository"), "{err}");

    let mut unknown = baseline();
    unknown["families"]["L01"]["value"] = json!("x");
    let path = write_private(&scratch(), &unknown, 0o600);
    let err = live::load(&repo(), &path)
        .expect_err("rejected")
        .to_string();
    assert!(err.contains("unknown field"), "{err}");

    let mut wrong = baseline();
    wrong["environments"]["staging-a"]["kind"] = json!("production");
    let path = write_private(&scratch(), &wrong, 0o600);
    let err = live::load(&repo(), &path)
        .expect_err("rejected")
        .to_string();
    assert!(err.contains("the live manifest is not acceptable"), "{err}");
}

#[test]
fn the_template_parses_but_is_not_approved() {
    let template: Value = serde_json::from_slice(
        &std::fs::read(repo().join("docs/architecture/live-manifest.template.json"))
            .expect("fixture"),
    )
    .expect("fixture");
    let body = template.clone();
    let found = failures(&body);
    assert_eq!(found, vec!["the owner approval is incomplete".to_owned()]);
    let manifest: LiveManifest = serde_json::from_value(body).expect("fixture");
    assert!(manifest.families.values().all(|f| f.status == "blocked"));
    assert_eq!(manifest.families.len(), 18);
}

#[test]
fn every_required_row_reports_its_access() {
    let ledger = ledger();
    let required = ledger
        .rows
        .iter()
        .filter(|r| r.live.requirement == "required")
        .count();

    let none = live::report(&ledger, None);
    assert_eq!(none.summary.families_blocked, 18);
    assert_eq!(none.summary.rows_required, required);
    assert_eq!(none.summary.rows_owned_by_c26, 1);
    assert_eq!(none.summary.rows_blocked, required - 1);
    assert!(none.families.iter().all(|f| f.reason == live::NO_MANIFEST));
    assert!(none.manifest_sha256.is_none() && none.owner_approval.is_none());
    let w63 = none.rows.iter().find(|r| r.id == "W63").expect("fixture");
    assert_eq!(
        (w63.access_status.as_str(), w63.owner.as_deref()),
        ("final-artifact", Some("C26"))
    );

    let all: LiveManifest = serde_json::from_value(baseline()).expect("fixture");
    let full = live::report(&ledger, Some((&all, "abc")));
    assert_eq!(full.summary.families_designated, 18);
    assert_eq!(full.summary.rows_designated, required - 1);
    assert_eq!(full.summary.rows_blocked, 0);
    assert_eq!(full.manifest_sha256.as_deref(), Some("abc"));
    let l01 = full
        .families
        .iter()
        .find(|f| f.id == "L01")
        .expect("fixture");
    assert_eq!(l01.environment_kind.as_deref(), Some("staging"));
    assert_eq!(
        l01.credentials[0].reference,
        "keychain:io.cdot.prisma-airs-desktop.v2.c25/L01-a"
    );

    let mut partial = baseline();
    partial["families"]["L09"] =
        json!({"status": "blocked", "reason": "no inference budget designated"});
    let partial: LiveManifest = serde_json::from_value(partial).expect("fixture");
    let report = live::report(&ledger, Some((&partial, "abc")));
    let w40 = report.rows.iter().find(|r| r.id == "W40").expect("fixture");
    assert_eq!(w40.access_status, "blocked-by-access");
    assert_eq!(w40.blocked_families, vec!["L09".to_owned()]);
    let w41 = report.rows.iter().find(|r| r.id == "W41").expect("fixture");
    assert_eq!(w41.access_status, "designated");
    assert_eq!(report.summary.rows_blocked, 1);
}
