//! Academy families (C25), sharing one learner sign-in: L11 identity and
//! access, L12 learning, L13 assessment and account, L14 local work.
//!
//! The device sign-in is the operator's: the core starts it and opens
//! Academy's verification page (through the host's browser adapter); the
//! operator approves it with the designated test identity, and the runner
//! only waits, bounded, for the core to show the account signed in. Learner
//! calls go through the production `academyCall` intent and the offline
//! intents; which procedures a family calls, with which synthetic inputs,
//! is the family's `call.*` entries, and a mutation must also be listed in
//! its permitted operations. No credential of a learner is ever seen here.

use std::collections::BTreeMap;

use desktop_core::learner::{LEARNER_PROCEDURES, ProcedureKind};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::live_run::{Desk, DeskOptions, Found, Outcomes, Run, Spec, Stop, operator_step};
use crate::{Result, now_utc};

pub const FAMILIES: [&str; 4] = ["L11", "L12", "L13", "L14"];

const SIGN_IN: Spec = Spec {
    id: "L11-device-sign-in",
    workflows: &["W49"],
};
const ADMISSION: Spec = Spec {
    id: "L11-protected-admission",
    workflows: &["W49"],
};
const SWITCH: Spec = Spec {
    id: "L11-account-switch",
    workflows: &["W49"],
};
const SIGN_OUT: Spec = Spec {
    id: "L11-sign-out-revoke",
    workflows: &["W49"],
};
const AVATAR: Spec = Spec {
    id: "L13-avatar",
    workflows: &["W54"],
};

/// The workflows each family's calls may claim.
fn claimable(family: &str) -> &'static [&'static str] {
    match family {
        "L12" => &["W50", "W51", "W52", "W65"],
        "L13" => &["W52", "W53", "W54", "W65", "W66"],
        "L14" => &["W55", "W56"],
        _ => &[],
    }
}
const DOWNLOAD: Spec = Spec {
    id: "L14-download-lease",
    workflows: &["W55"],
};
const DRAFT: Spec = Spec {
    id: "L14-local-draft",
    workflows: &["W55"],
};
const DENIAL: Spec = Spec {
    id: "L14-access-change-denied",
    workflows: &["W56"],
};

/// An 8×8 PNG: the synthetic avatar (re-encoded by the core before upload).
const AVATAR_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x08, 0x08, 0x02, 0x00, 0x00, 0x00, 0x4B, 0x6D, 0x29,
    0xDC, 0x00, 0x00, 0x00, 0x11, 0x49, 0x44, 0x41, 0x54, 0x78, 0xDA, 0x63, 0xD0, 0xCE, 0xD9, 0x80,
    0x15, 0x31, 0x0C, 0x2D, 0x09, 0x00, 0xD0, 0x59, 0x51, 0xC1, 0x19, 0x34, 0xF9, 0xFA, 0x00, 0x00,
    0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

pub(crate) struct Shared {
    pub families: Vec<(String, Outcomes)>,
    pub stop: Option<Stop>,
}

struct Ctx<'r, 'a> {
    run: &'r mut Run<'a>,
    outcomes: BTreeMap<String, Outcomes>,
    stop: Option<Stop>,
}

impl Ctx<'_, '_> {
    fn settle(
        &mut self,
        family: &str,
        spec: Spec,
        started: String,
        inputs: BTreeMap<String, Value>,
        found: Found,
    ) -> Result<()> {
        let outcomes = self.outcomes.entry(family.to_owned()).or_default();
        let stop = self
            .run
            .settle(family, spec, started, inputs, found, None, outcomes)?;
        if self.stop.is_none() {
            self.stop = stop;
        }
        Ok(())
    }

    fn block(&mut self, family: &str, spec: Spec, reason: &str) -> Result<()> {
        self.settle(
            family,
            spec,
            now_utc(),
            BTreeMap::new(),
            Found::blocked(reason),
        )
    }
}

fn state(j: &Value) -> String {
    j["academy"]["state"].as_str().unwrap_or("").to_owned()
}

fn account_hash(j: &Value) -> Option<String> {
    j["academy"]["accountId"].as_str().map(|a| {
        let digest = Sha256::digest(a.as_bytes());
        hex::encode(&digest[..8])
    })
}

fn learner(j: &Value) -> Value {
    j["learner"].clone()
}

/// Starts the device sign-in and waits for the operator to approve it.
async fn sign_in(run: &Run<'_>, desk: &mut Desk, family: &str) -> Result<(Found, Option<String>)> {
    desk.send_json(json!({"kind": "startAcademySignIn"}))
        .await?;
    let awaiting = desk
        .until(desk.settle, |j| {
            matches!(state(j).as_str(), "awaitingApproval" | "failed")
        })
        .await;
    match awaiting {
        Some(j) if state(&j) == "awaitingApproval" => {}
        Some(j) => {
            return Ok((
                Found::failed("the sign-in could not start")
                    .with("academy", j["academy"]["failure"].clone()),
                None,
            ));
        }
        None => return Ok((Found::failed("the sign-in did not start in time"), None)),
    }
    // The core opens the verification page through the host; the operator
    // completes it there.
    desk.send_json(json!({"kind": "openAcademyVerification"}))
        .await?;
    let bound = run.operator_timeout(family);
    let (completed, step) = {
        let desk = &*desk;
        operator_step(
            run.operator.as_ref(),
            "academy-device-sign-in",
            "approve the Academy device sign-in with the designated test learner",
            None,
            bound,
            // A failed sign-in ends the wait; the core's state judges it.
            async || {
                desk.json()
                    .map(|j| matches!(state(&j).as_str(), "signedIn" | "failed"))
                    .unwrap_or(false)
            },
        )
        .await
    };
    let j = desk.json()?;
    let signed_in = completed && state(&j) == "signedIn";
    let mut found = Found::check(
        signed_in,
        "the operator approved the device sign-in and the core shows the learner signed in",
        if completed {
            "the device sign-in failed"
        } else {
            "the device sign-in was not completed within the bound"
        },
    )
    .with("state", json!(state(&j)))
    .with("failure", j["academy"]["failure"].clone())
    .with("vault", j["academy"]["vault"].clone());
    found.operator_step = Some(step);
    let hash = account_hash(&j);
    if let Some(h) = &hash {
        found = found.with("account_sha256_prefix", json!(h));
    }
    Ok((found, hash))
}

/// One designated learner call: its label, procedure, kind and input.
struct Call {
    label: String,
    /// The workflows this call is evidence for, as its entry names them.
    workflows: &'static [&'static str],
    procedure: String,
    kind: ProcedureKind,
    input: Option<String>,
}

/// Parses the family's `call.*` entries, each `<workflows> <procedure>
/// [<input>]`: the workflow IDs (comma-separated, among those the family may
/// claim) the call is evidence for, the procedure, and its input. A call
/// counts only for the workflows it names.
fn program(run: &Run<'_>, family: &str) -> std::result::Result<Vec<Call>, String> {
    let Ok(f) = run.family(family) else {
        return Ok(Vec::new());
    };
    let allowed = claimable(family);
    let mut calls = Vec::new();
    for (key, value) in &f.nonsecret {
        let Some(label) = key.strip_prefix("call.") else {
            continue;
        };
        let (tags, rest) = value
            .split_once(' ')
            .ok_or_else(|| format!("{key} names no workflow and procedure"))?;
        let mut workflows = Vec::new();
        for tag in tags.split(',') {
            let w = allowed
                .iter()
                .find(|w| **w == tag)
                .ok_or_else(|| format!("{key}: {tag} is not a workflow {family} may claim"))?;
            workflows.push(*w);
        }
        let (procedure, input) = match rest.trim().split_once(' ') {
            Some((p, i)) => (p.to_owned(), Some(i.trim().to_owned())),
            None => (rest.trim().to_owned(), None),
        };
        let kind = LEARNER_PROCEDURES
            .iter()
            .find(|(p, _)| *p == procedure)
            .map(|(_, k)| *k)
            .ok_or_else(|| format!("{procedure} is not a learner procedure"))?;
        if kind == ProcedureKind::Mutation && !f.permitted_operations.contains(&procedure) {
            return Err(format!(
                "{procedure} changes learner state but is not a permitted operation of {family}"
            ));
        }
        if let Some(input) = &input
            && serde_json::from_str::<Value>(input).is_err()
        {
            return Err(format!("{key}: the input is not JSON"));
        }
        calls.push(Call {
            label: label.to_owned(),
            workflows: Box::leak(workflows.into_boxed_slice()),
            procedure,
            kind,
            input,
        });
    }
    Ok(calls)
}

fn result_found(result: &Value) -> Found {
    let kind = result["kind"].as_str().unwrap_or("");
    let mut found = Found::check(
        kind == "data",
        "the hosted procedure answered",
        "the hosted procedure did not answer with data",
    )
    .with("result", json!(kind))
    .with("http_status", result["httpStatus"].clone())
    .with("failure", result["failure"].clone())
    .with("outcome", result["outcome"].clone());
    if kind == "error" && result["outcome"] == "unknown" {
        found = found.stopping("a learner mutation may or may not have taken effect", true);
    }
    found
}

async fn call(
    desk: &mut Desk,
    procedure: &str,
    kind: ProcedureKind,
    input: Option<&str>,
) -> Result<Value> {
    let learner = learner(&desk.json()?);
    desk.academy(
        json!({"kind": "academyCall", "learner": learner, "procedure": procedure,
        "procedureKind": if kind == ProcedureKind::Mutation { "mutation" } else { "query" },
        "input": input}),
    )
    .await
}

fn leak(name: String) -> &'static str {
    Box::leak(name.into_boxed_str())
}

pub(crate) async fn run(run: &mut Run<'_>, families: &[String]) -> Result<Shared> {
    let mut ctx = Ctx {
        run,
        outcomes: BTreeMap::new(),
        stop: None,
    };
    let wants = |f: &str| families.iter().any(|x| x == f);
    let origin = FAMILIES
        .iter()
        .filter(|f| wants(f))
        .find_map(|f| ctx.run.nonsecret(f, "academy_origin"));
    let lead = FAMILIES.iter().copied().find(|f| wants(f)).unwrap_or("L11");
    let offline = if wants("L14") {
        Some(ctx.run.folder("offline")?)
    } else {
        None
    };
    let mut desk = ctx.run.desk(DeskOptions {
        offline_root: offline,
        academy_origin: origin.filter(|o| o.starts_with("https://")),
        image: wants("L13").then(|| AVATAR_PNG.to_vec()),
        ..DeskOptions::default()
    })?;

    let started = now_utc();
    let (found, account) = sign_in(ctx.run, &mut desk, lead).await?;
    let signed_in = found.outcome == "passed";
    let sign_in_family = if wants("L11") { "L11" } else { lead };
    ctx.settle(
        sign_in_family,
        SIGN_IN,
        started,
        ctx.run.inputs(sign_in_family, &[]),
        found,
    )?;
    if !signed_in {
        for family in families {
            let spec = Spec {
                id: leak(format!("{family}-after-sign-in")),
                workflows: &[],
            };
            ctx.block(family, spec, "the learner was not signed in")?;
        }
        return Ok(finish(ctx, families));
    }

    if wants("L11") {
        let started = now_utc();
        let me = call(&mut desk, "desktop.me", ProcedureKind::Query, None).await?;
        // Academy's access check takes the courses to check (the designated
        // course, when there is one).
        let courses: Vec<String> = ctx.run.nonsecret("L14", "course_id").into_iter().collect();
        let input = json!({"json": {"courseIds": courses}}).to_string();
        let access = call(
            &mut desk,
            "desktop.access",
            ProcedureKind::Query,
            Some(&input),
        )
        .await?;
        let found = Found::check(
            me["kind"] == "data" && access["kind"] == "data",
            "protected learner procedures were admitted with the signed-in learner's token",
            "a protected learner procedure was not admitted",
        )
        .with("desktop.me", me["kind"].clone())
        .with("desktop.access", access["kind"].clone());
        ctx.settle("L11", ADMISSION, started, ctx.run.inputs("L11", &[]), found)?;
    }

    for family in ["L12", "L13", "L14"] {
        if !wants(family) || ctx.stop.is_some() {
            continue;
        }
        match program(ctx.run, family) {
            Err(e) => {
                let spec = Spec {
                    id: leak(format!("{family}-calls")),
                    workflows: &[],
                };
                ctx.block(family, spec, &e)?;
            }
            Ok(calls) => {
                if calls.is_empty() && family != "L14" {
                    let spec = Spec {
                        id: leak(format!("{family}-calls")),
                        workflows: &[],
                    };
                    ctx.block(
                        family,
                        spec,
                        &format!("{family} designates no call.* procedures"),
                    )?;
                }
                for Call {
                    label,
                    workflows,
                    procedure,
                    kind,
                    input,
                } in calls
                {
                    if ctx.stop.is_some() {
                        break;
                    }
                    let spec = Spec {
                        id: leak(format!("{family}-call-{label}")),
                        workflows,
                    };
                    let started = now_utc();
                    let mut inputs = BTreeMap::new();
                    inputs.insert("procedure".to_owned(), json!(procedure));
                    inputs.insert("input".to_owned(), json!(input));
                    // Charged before it is sent: an exhausted budget sends nothing.
                    if let Err(e) = ctx.run.guard.spend(family, "calls", 1) {
                        let found = Found::blocked(e.clone())
                            .stopping(format!("unexpected cost: {e}"), false);
                        ctx.settle(family, spec, started, inputs, found)?;
                        break;
                    }
                    let result = call(&mut desk, &procedure, kind, input.as_deref()).await?;
                    let mut found = result_found(&result);
                    if kind == ProcedureKind::Mutation {
                        found.cleanup =
                            "synthetic coursework on a test account (owner-designated)".to_owned();
                    }
                    ctx.settle(family, spec, started, inputs, found)?;
                }
            }
        }
    }

    if wants("L13") && ctx.stop.is_none() {
        let started = now_utc();
        let permitted = ctx
            .run
            .family("L13")
            .map(|f| {
                f.permitted_operations
                    .iter()
                    .any(|o| o == "settings.removeAvatar")
            })
            .unwrap_or(false);
        let found = if !permitted {
            Found::blocked(
                "an avatar upload needs settings.removeAvatar among L13's permitted operations, to undo it",
            )
        } else if let Err(e) = ctx.run.guard.spend("L13", "calls", 2) {
            // The upload and its removal, charged before either is sent.
            Found::blocked(e.clone()).stopping(format!("unexpected cost: {e}"), false)
        } else {
            let learner = learner(&desk.json()?);
            let uploaded = desk
                .academy(json!({"kind": "uploadAvatar", "learner": learner}))
                .await?;
            let removed = call(
                &mut desk,
                "settings.removeAvatar",
                ProcedureKind::Mutation,
                None,
            )
            .await?;
            let mut found = Found::check(
                uploaded["kind"] == "data" && removed["kind"] == "data",
                "a synthetic avatar was bounded, re-encoded and uploaded, then removed",
                "the avatar upload or its removal did not complete",
            )
            .with("upload", uploaded["kind"].clone())
            .with("upload_failure", uploaded["failure"].clone())
            .with("remove", removed["kind"].clone());
            found.cleanup = "removed with settings.removeAvatar".to_owned();
            found
        };
        ctx.settle("L13", AVATAR, started, BTreeMap::new(), found)?;
    }

    if wants("L14") && ctx.stop.is_none() {
        l14(&mut ctx, &mut desk).await?;
    }

    if wants("L11") && ctx.stop.is_none() {
        let started = now_utc();
        let before = learner(&desk.json()?);
        desk.send_json(json!({"kind": "signOutAcademy"})).await?;
        let out = desk
            .until(desk.settle, |j| state(j) == "signedOut")
            .await
            .is_some();
        // The retired learner reference is refused by the core afterwards.
        let refused = desk
            .send_json(
                json!({"kind": "academyCall", "learner": before, "procedure": "desktop.me",
                "procedureKind": "query", "input": null}),
            )
            .await
            .is_err();
        let found = Found::check(
            out && refused,
            "signed out: the session was deleted, its refresh token revoked, and the old learner is refused",
            "sign-out did not retire the learner",
        );
        ctx.settle("L11", SIGN_OUT, started, BTreeMap::new(), found)?;

        let started = now_utc();
        let found = if ctx.run.nonsecret("L11", "second_account").as_deref() == Some("designated") {
            let (found, second) = sign_in(ctx.run, &mut desk, "L11").await?;
            let switched = found.outcome == "passed" && second.is_some() && second != account;
            let mut found = Found {
                outcome: if switched { "passed" } else { "failed" },
                detail: if switched {
                    "a second test learner signed in; the account changed and nothing of the first carried over".to_owned()
                } else {
                    "the second sign-in did not show a different account".to_owned()
                },
                ..found
            };
            desk.send_json(json!({"kind": "signOutAcademy"})).await?;
            found.cleanup = "signed out again".to_owned();
            found
        } else {
            Found::blocked("L11 designates no second_account for the account switch")
        };
        ctx.settle("L11", SWITCH, started, BTreeMap::new(), found)?;
    } else if ctx.stop.is_none() {
        desk.send_json(json!({"kind": "signOutAcademy"})).await?;
    }
    Ok(finish(ctx, families))
}

async fn l14(ctx: &mut Ctx<'_, '_>, desk: &mut Desk) -> Result<()> {
    let Some(course) = ctx.run.nonsecret("L14", "course_id") else {
        for spec in [DOWNLOAD, DRAFT, DENIAL] {
            ctx.block("L14", spec, "L14 designates no course_id")?;
        }
        return Ok(());
    };
    let started = now_utc();
    let learner_ref = learner(&desk.json()?);
    let downloaded = desk
        .academy(json!({"kind": "downloadCourse", "learner": learner_ref, "courseId": course}))
        .await?;
    let mut found = result_found(&downloaded);
    found.detail = if found.outcome == "passed" {
        "the client-safe course was downloaded under its lease and sealed for this account"
            .to_owned()
    } else {
        "the course could not be downloaded".to_owned()
    };
    found.cleanup = "removed at the end of the family".to_owned();
    let mut inputs = BTreeMap::new();
    inputs.insert("course_id".to_owned(), json!(course));
    ctx.settle("L14", DOWNLOAD, started, inputs.clone(), found)?;

    let started = now_utc();
    let found = match ctx.run.nonsecret("L14", "attempt_id") {
        None => Found::blocked("L14 designates no attempt_id"),
        Some(attempt) => {
            let base: u32 = ctx
                .run
                .nonsecret("L14", "base_version")
                .and_then(|b| b.parse().ok())
                .unwrap_or(0);
            let draft = "C25 synthetic local draft";
            // Lab work is a JSON object, as the learner page keeps it.
            let saved = desk
                .academy(
                    json!({"kind": "saveLocalDraft", "learner": learner_ref, "attemptId": attempt,
                    "baseVersion": base, "draft": json!({"c25": draft}).to_string()}),
                )
                .await?;
            let loaded = desk
                .academy(
                    json!({"kind": "loadLocalDraft", "learner": learner_ref, "attemptId": attempt}),
                )
                .await?;
            let kept = loaded["data"].as_str().is_some_and(|d| d.contains(draft));
            let _ = desk
                .academy(json!({"kind": "clearLocalDraft", "learner": learner_ref, "attemptId": attempt}))
                .await;
            let mut found = Found::check(
                saved["kind"] == "data" && kept,
                "the draft was sealed locally for this account and read back; nothing was submitted",
                "the local draft was not kept",
            )
            .with("save", json!({"kind": saved["kind"], "failure": saved["failure"]}))
            .with("load", json!({"kind": loaded["kind"], "failure": loaded["failure"]}));
            found.cleanup = "cleared from this Mac".to_owned();
            found
        }
    };
    ctx.settle("L14", DRAFT, started, inputs.clone(), found)?;

    // The operator changes the account's access; the core must then refuse.
    let started = now_utc();
    let found = if ctx.run.nonsecret("L14", "access_change").as_deref() != Some("designated") {
        Found::blocked("L14 designates no access_change the operator can make")
    } else {
        let bound = ctx.run.operator_timeout("L14");
        let course = course.clone();
        let denied = {
            let desk_ref = &mut *desk;
            let learner = learner_ref.clone();
            let mut last = Value::Null;
            let (done, step) = operator_step(
                ctx.run.operator.as_ref(),
                "academy-access-change",
                "remove the designated test learner's access to the designated course now",
                None,
                bound,
                async || {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    match desk_ref
                        .academy(json!({"kind": "downloadCourse", "learner": learner, "courseId": course}))
                        .await
                    {
                        Ok(r) => {
                            // Academy withdrew the course (no longer
                            // downloadable) or refused the learner.
                            let refused = (r["kind"] == "failed"
                                && r["failure"] == "notDownloadable")
                                || (r["kind"] == "error"
                                    && matches!(r["httpStatus"].as_u64(), Some(401 | 403 | 404)));
                            last = r;
                            refused
                        }
                        Err(_) => false,
                    }
                },
            )
            .await;
            (done, step, last)
        };
        let (done, step, last) = denied;
        let mut found = Found::check(
            done,
            "after the operator's access change, Academy refused the course and nothing stale was served",
            "Academy kept serving the course after the access change",
        )
        .with("last", json!({"kind": last["kind"], "failure": last["failure"], "http_status": last["httpStatus"]}));
        found.operator_step = Some(step);
        found
    };
    ctx.settle("L14", DENIAL, started, inputs, found)?;
    let _ = desk
        .academy(json!({"kind": "removeDownload", "learner": learner_ref, "courseId": course}))
        .await;
    Ok(())
}

fn finish(ctx: Ctx<'_, '_>, families: &[String]) -> Shared {
    let Ctx {
        mut outcomes, stop, ..
    } = ctx;
    Shared {
        families: families
            .iter()
            .map(|f| (f.clone(), outcomes.remove(f).unwrap_or_default()))
            .collect(),
        stop,
    }
}
