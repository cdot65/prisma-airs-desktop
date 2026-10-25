//! L01 Runtime scan and L18 scope isolation (C25), through the `scan`
//! intent of the production core with a session-only scan key.

use std::collections::BTreeMap;

use desktop_core::ScanRecordView;
use desktop_core::runtime::Outcome;
use desktop_core::scanning::{ScanFailureKind, Verdict};
use desktop_core::tenants::Destination;
use serde_json::{Value, json};

use crate::live_run::{DeskOptions, FamilyResult, Found, Run, Spec, destination};
use crate::{Result, invalid, now_utc};

/// A synthetic, benign prompt: no customer or operator content is scanned.
const DEFAULT_PROMPT: &str =
    "Prisma AIRS Desktop v2 controlled live check: a synthetic benign prompt.";

const L01_SCAN: Spec = Spec {
    id: "L01-scan",
    workflows: &["W01", "W02", "W04", "W06", "W22"],
};
const L01_DENIED: Spec = Spec {
    id: "L01-denied",
    workflows: &["W02", "W05"],
};
const L18_SCAN: Spec = Spec {
    id: "L18-own-scope",
    workflows: &["W03", "W32"],
};
const L18_OWN_ID: Spec = Spec {
    id: "L18-own-profile-id",
    workflows: &["W03", "W32"],
};
const L18_CROSS: Spec = Spec {
    id: "L18-cross-scope-denied",
    workflows: &["W03", "W32"],
};

/// One planned scan: whose key goes toward whose destination, whether the
/// service must refuse it, and which budget pays for it. `by_profile_id`
/// names the destination's profile by the ID its own scan answered with,
/// not by the configured name: tenants in one region share an endpoint and
/// may each have a profile of the same name, so only the ID identifies the
/// other scope's profile.
#[derive(Clone, Copy)]
struct Step {
    spec: Spec,
    key_family: &'static str,
    key_name: &'static str,
    dest_family: &'static str,
    expect_denied: bool,
    by_profile_id: bool,
    budget: &'static str,
}

fn profile(run: &Run<'_>, family: &str) -> std::result::Result<Value, String> {
    match (
        run.nonsecret(family, "profile_name"),
        run.nonsecret(family, "profile_id"),
    ) {
        (Some(name), None) => Ok(json!({"kind": "name", "name": name})),
        (None, Some(id)) => Ok(json!({"kind": "id", "id": id})),
        _ => Err("exactly one of profile_name or profile_id is required".to_owned()),
    }
}

/// What a settled scan shows, as recorded.
fn observe(record: &ScanRecordView, mut found: Found) -> Found {
    found = found.with("outcome", json!(record.outcome));
    if let Some(result) = &record.result {
        found = found
            .with("verdict", json!(result.verdict))
            .with("action", json!(result.action))
            .with("category", json!(result.category))
            .with("timeout", json!(result.timeout))
            .with("service_error", json!(result.service_error))
            .with("profile_name", json!(result.profile_name))
            .id("scan_id", &result.scan_id)
            .id("report_id", &result.report_id);
        if let Some(tr) = &result.tr_id {
            found = found.id("tr_id", tr);
        }
        if let Some(p) = &result.profile_id {
            found = found.id("profile_id", p);
        }
    }
    if let Some(failure) = &record.failure {
        found = found
            .with("failure", json!(failure.kind))
            .with("status", json!(failure.status));
    }
    found
}

/// A settled scan that may have reached the service without a reliable
/// outcome: its consumption is uncertain.
fn uncertain(record: &ScanRecordView) -> bool {
    record.outcome == Some(Outcome::Unknown) || record.discarded
}

/// Another scope's key refused for this scope's profile, with no result. A
/// key outside the profile's tenant is told the profile does not exist
/// (400) on a shared regional endpoint, or refused as a credential.
fn refused(record: &ScanRecordView) -> bool {
    denied(record)
        || (record.result.is_none()
            && record.failure.as_ref().is_some_and(|f| {
                matches!(
                    f.kind,
                    ScanFailureKind::BadRequest | ScanFailureKind::Unprocessable
                )
            }))
}

fn denied(record: &ScanRecordView) -> bool {
    record.result.is_none()
        && record.failure.as_ref().is_some_and(|f| {
            matches!(
                f.kind,
                ScanFailureKind::Unauthorized
                    | ScanFailureKind::Forbidden
                    | ScanFailureKind::CredentialDenied
            )
        })
}

async fn scan(desk: &mut crate::live_run::Desk, draft: Value) -> Result<ScanRecordView> {
    let context = desk.context_text()?;
    let operation = desk
        .admit(json!({"kind": "scan", "context": context, "draft": draft}))
        .await?;
    let snap = desk
        .wait(|s| {
            s.scans.iter().any(|r| {
                r.operation == operation && r.progress == desktop_core::ScanProgress::Settled
            })
        })
        .await?;
    snap.scans
        .into_iter()
        .find(|r| r.operation == operation)
        .ok_or_else(|| invalid("the settled scan is not listed"))
}

/// A check's finding, its inputs and the profile ID its scan answered with.
type Stepped = (Found, BTreeMap<String, Value>, Option<String>);

async fn step(run: &mut Run<'_>, step: Step, target: Option<&str>) -> Result<Stepped> {
    let dest: Destination = match destination(run.family(step.dest_family)?, false) {
        Ok(d) => d,
        Err(e) => return Ok((Found::blocked(e), BTreeMap::new(), None)),
    };
    let mut inputs = run.inputs(step.dest_family, &[]);
    inputs.insert("endpoint".to_owned(), json!(dest.origin()));
    inputs.insert(
        "credential".to_owned(),
        json!(format!("{}.{}", step.key_family, step.key_name)),
    );
    inputs
        .entry("prompt".to_owned())
        .or_insert_with(|| json!(DEFAULT_PROMPT));
    let profile = if step.by_profile_id {
        let Some(id) = target else {
            return Ok((
                Found::blocked(format!(
                    "{}'s own scan answered with no profile ID to aim at",
                    step.dest_family
                )),
                inputs,
                None,
            ));
        };
        inputs.insert("profile_id".to_owned(), json!(id));
        inputs.remove("profile_name");
        json!({"kind": "id", "id": id})
    } else {
        match profile(run, step.dest_family) {
            Ok(p) => p,
            Err(e) => return Ok((Found::blocked(e), inputs, None)),
        }
    };
    let Some(key) = run.secret(step.key_family, step.key_name)? else {
        return Ok((
            Found::blocked(format!(
                "{} designates no {} credential",
                step.key_family, step.key_name
            )),
            inputs,
            None,
        ));
    };
    if let Err(e) = run.guard.spend(step.budget, "scans", 1) {
        return Ok((
            Found::blocked(e.clone()).stopping(format!("unexpected cost: {e}"), false),
            inputs,
            None,
        ));
    }
    let mut desk = run.desk(DeskOptions::default())?;
    desk.tenant(&format!("C25 {}", step.spec.id), None, &dest)
        .await?;
    desk.scan_key(&key).await?;
    let draft = json!({
        "profile": profile,
        "prompt": run.nonsecret(step.dest_family, "prompt").unwrap_or_else(|| DEFAULT_PROMPT.to_owned()),
        "response": "",
        "appName": run.nonsecret(step.dest_family, "app_name"),
    });
    let record = scan(&mut desk, draft).await?;
    let answered = record.result.as_ref().and_then(|r| r.profile_id.clone());
    let management = desk.management_configured()?;
    drop(desk);
    let cleanup = "a local session-only tenant; removed with the run's core, no remote resource";
    if uncertain(&record) {
        let mut found = observe(
            &record,
            Found::failed(
                "the scan may have reached the service without a reliable outcome; its \
                 consumption is uncertain",
            ),
        )
        .stopping("an uncertain outcome", true);
        found.cleanup = cleanup.to_owned();
        return Ok((found, inputs, answered));
    }
    let mut found = if step.expect_denied {
        if record.result.is_some() {
            Found::failed("unexpected authority: a credential that must be denied completed a scan")
                .stopping("unexpected authority", false)
        } else {
            if step.by_profile_id {
                Found::check(
                    refused(&record),
                    "the service refused this key for the other scope's profile and nothing \
                     was shown as a result",
                    "the request failed, but not as a refusal of the other scope's profile",
                )
            } else {
                Found::check(
                    denied(&record),
                    "the service refused the credential and nothing was shown as a result",
                    "the request failed, but not as a credential denial",
                )
            }
        }
    } else {
        match &record.result {
            None => Found::failed("the designated key did not complete a scan"),
            Some(result) => {
                let expected = if step.by_profile_id {
                    target.map(str::to_owned)
                } else {
                    run.nonsecret(step.dest_family, "expected_profile_id")
                };
                if let Some(expected) = expected
                    && result.profile_id.as_deref() != Some(expected.as_str())
                {
                    Found::failed(format!(
                        "unexpected authority: the service answered for another profile than {expected}"
                    ))
                    .stopping("unexpected tenant or authority", false)
                } else {
                    let complete = record.outcome == Some(Outcome::Completed)
                        && !result.timeout
                        && !result.service_error
                        && result.verdict != Verdict::Unknown;
                    Found::check(
                        complete,
                        "a complete verdict for the explicit profile, with no management capability configured",
                        "the scan settled incomplete (timeout, service error or unknown verdict)",
                    )
                }
            }
        }
        .with("management_configured", json!(management))
    };
    found = observe(&record, found);
    found.cleanup = cleanup.to_owned();
    Ok((found, inputs, answered))
}

pub(crate) async fn run(run: &mut Run<'_>, id: &str) -> Result<FamilyResult> {
    let s = |spec, key_family, key_name, dest_family, expect_denied, by_profile_id, budget| Step {
        spec,
        key_family,
        key_name,
        dest_family,
        expect_denied,
        by_profile_id,
        budget,
    };
    let mut plan = Vec::new();
    let mut blocked = Vec::new();
    match id {
        "L01" => {
            plan.push(s(L01_SCAN, "L01", "scan_key", "L01", false, false, "L01"));
            plan.push(s(
                L01_DENIED,
                "L01",
                "denied_scan_key",
                "L01",
                true,
                false,
                "L01",
            ));
        }
        _ => {
            let budget = if run.family("L18")?.budget.contains_key("scans") {
                "L18"
            } else {
                "L01"
            };
            plan.push(s(L18_SCAN, "L18", "scan_key", "L18", false, false, budget));
            // L18's own key names its profile by the ID it answered with, so
            // that request shape is shown to work before L01's key tries it.
            plan.push(s(L18_OWN_ID, "L18", "scan_key", "L18", false, true, budget));
            if run.designated("L01") {
                // L01's key aimed at L18's profile ID must be refused.
                plan.push(s(L18_CROSS, "L01", "scan_key", "L18", true, true, budget));
            } else {
                blocked.push(L18_CROSS);
            }
        }
    }
    let mut outcomes = Vec::new();
    // The profile ID L18's own scan answered with.
    let mut target: Option<String> = None;
    for planned in plan {
        let started = now_utc();
        let (found, inputs, answered) = step(run, planned, target.as_deref()).await?;
        if planned.spec.id == L18_SCAN.id {
            target = answered;
        }
        if let Some(stop) = run.settle(
            id,
            planned.spec,
            started,
            inputs,
            found,
            Some(planned.budget),
            &mut outcomes,
        )? {
            return Ok(FamilyResult {
                outcomes,
                stop: Some(stop),
            });
        }
    }
    for spec in blocked {
        let found = Found::blocked("L01 is not designated, so no other scope's key can be tried");
        run.settle(
            id,
            spec,
            now_utc(),
            BTreeMap::new(),
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
