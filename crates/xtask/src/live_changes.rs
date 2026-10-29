//! Mutating families (C25): L03 Runtime changes, L04 DLP and L08 Gateway
//! changes, each through the production change review: `reviewChange`
//! (Rust reads the baseline and normalizes the draft), `requestApply` with
//! the exact typed confirmation, and the native confirmation, which the
//! runner arms for that one reviewed change only. Every created resource is
//! named under the family's disposable prefix, every update or delete
//! targets a resource this run created (or, for a replace, a designated one
//! written back with its own current body), one-time secrets are forgotten
//! unseen, and every creation is deleted again and read back as absent. An
//! outcome that may or may not have taken effect stops the run and leaves
//! what was created for reconciliation instead of guessing.

use std::collections::BTreeMap;

use desktop_core::management::ManagementResult;
use serde_json::{Value, json};

use crate::live_run::{
    Desk, DeskOptions, FamilyResult, Found, Outcomes, Run, Spec, Stop, disposable, sanitize_id,
};
use crate::{Result, invalid, now_utc};

/// Names in a body that must sit under the disposable prefix (an IAM
/// `scope_name` is checked against its underscore form in `names_ok`).
const NAME_FIELDS: [&str; 5] = [
    "topic_name",
    "profile_name",
    "api_key_name",
    "cust_app",
    "name",
];

fn id_field(kind: &str) -> &'static str {
    match kind {
        "topics" => "topic_id",
        "profiles" => "profile_id",
        "apiKeys" => "api_key_id",
        "applications" => "customer_appId",
        _ => "id",
    }
}

/// A created resource the run must delete again.
#[derive(Clone, Debug)]
struct Created {
    label: &'static str,
    kind: &'static str,
    id: String,
    scoped: bool,
}

/// What one applied change produced.
pub(crate) enum Applied {
    /// The review failed; nothing was sent.
    Review(Value),
    Settled {
        result: Value,
        confirmation: Option<String>,
    },
}

/// Reviews `draft`, applies it with the review's own confirmation text and
/// the armed native confirmation, and waits for the result to settle.
pub(crate) async fn apply(desk: &mut Desk, draft: &Value) -> Result<Applied> {
    let context = desk.context_text()?;
    desk.send_json(json!({"kind": "reviewChange", "context": context, "draft": draft}))
        .await?;
    let snap = desk
        .until(desk.settle, |j| {
            matches!(
                j["changes"]["review"]["state"].as_str(),
                Some("ready" | "failed")
            )
        })
        .await
        .ok_or_else(|| invalid("the review did not settle"))?;
    let review = snap["changes"]["review"].clone();
    if review["state"] == "failed" {
        return Ok(Applied::Review(review["failure"].clone()));
    }
    let typed = review["confirmation"]
        .as_str()
        .unwrap_or("CREATE")
        .to_owned();
    let before = snap["changes"]["results"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    desk.confirmations.arm();
    let sent = desk
        .send_json(
            json!({"kind": "requestApply", "review": review["review"], "draft": draft,
            "confirmation": typed}),
        )
        .await;
    if let Err(e) = sent {
        desk.confirmations.disarm();
        return Err(e);
    }
    let settled = desk
        .until(desk.settle, |j| {
            j["changes"]["results"]
                .as_array()
                .is_some_and(|r| r.len() > before && r[0]["settled"] == true)
        })
        .await;
    desk.confirmations.disarm();
    let confirmation = desk.confirmations.shown().last().map(|(t, _)| t.clone());
    match settled {
        Some(j) => Ok(Applied::Settled {
            result: j["changes"]["results"][0].clone(),
            confirmation,
        }),
        None => Ok(Applied::Settled {
            result: json!({"outcome": "unknown", "settled": false}),
            confirmation,
        }),
    }
}

/// Forgets an applied change's one-time secret, unseen.
async fn forget(desk: &mut Desk, result: &Value) -> Result<Value> {
    if result["secret"] != "held" {
        return Ok(result["secret"].clone());
    }
    desk.send_json(json!({"kind": "forgetSecret", "operation": result["operation"]}))
        .await?;
    let operation = result["operation"].clone();
    let forgotten = desk
        .until(desk.settle, |j| {
            j["changes"]["results"].as_array().is_some_and(|r| {
                r.iter()
                    .any(|x| x["operation"] == operation && x["secret"] == "forgotten")
            })
        })
        .await
        .is_some();
    Ok(json!(if forgotten {
        "held, then forgotten unseen"
    } else {
        "still held"
    }))
}

fn current(result: &Value) -> Value {
    result["current"]
        .as_str()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(Value::Null)
}

fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

struct Ctx<'r, 'a> {
    run: &'r mut Run<'a>,
    id: &'static str,
    prefix: Option<String>,
    outcomes: Outcomes,
    created: Vec<Created>,
    stop: Option<Stop>,
}

/// One planned change.
struct Change {
    spec: Spec,
    kind: &'static str,
    action: &'static str,
    /// The created resource's label (update, delete, rotate), or none.
    target: Option<&'static str>,
    body: Value,
    /// Within the selected Gateway workspace.
    scoped: bool,
    expect: &'static str,
    /// Remember what this creates under this label.
    creates: Option<&'static str>,
}

impl Ctx<'_, '_> {
    fn stopped(&self) -> bool {
        self.stop.is_some()
    }

    fn settle(
        &mut self,
        spec: Spec,
        started: String,
        inputs: BTreeMap<String, Value>,
        found: Found,
    ) -> Result<()> {
        let stop = self.run.settle(
            self.id,
            spec,
            started,
            inputs,
            found,
            None,
            &mut self.outcomes,
        )?;
        if self.stop.is_none() {
            self.stop = stop;
        }
        Ok(())
    }

    fn block(&mut self, spec: Spec, reason: &str) -> Result<()> {
        self.settle(spec, now_utc(), BTreeMap::new(), Found::blocked(reason))
    }

    /// Names in a create body must be under the prefix.
    fn names_ok(&self, body: &Value) -> std::result::Result<(), String> {
        let mut names = Vec::new();
        for field in NAME_FIELDS {
            if let Some(n) = body.get(field).and_then(Value::as_str) {
                names.push(n.to_owned());
            }
        }
        if let Some(n) = body["metadata"]["name"].as_str() {
            names.push(n.to_owned());
        }
        if names.is_empty() {
            return Err("a create names nothing to check against the prefix".to_owned());
        }
        for name in names {
            disposable(self.prefix.as_deref(), &name)?;
        }
        // An IAM scope name allows only letters, digits and underscores
        // (the service refuses hyphens), so it carries the prefix with its
        // hyphens as underscores.
        if let Some(scope) = body.get("scope_name").and_then(Value::as_str) {
            let prefix = self.prefix.as_deref().unwrap_or_default().replace('-', "_");
            if prefix.is_empty() || !scope.starts_with(&prefix) {
                return Err(format!(
                    "the IAM scope {scope} does not start with {prefix}"
                ));
            }
        }
        Ok(())
    }

    fn target(&self, label: &str) -> Option<Created> {
        self.created
            .iter()
            .rev()
            .find(|c| c.label == label)
            .cloned()
    }

    /// Applies one change and records it.
    async fn change(&mut self, desk: &mut Desk, change: Change) -> Result<Option<Value>> {
        if self.stopped() {
            return Ok(None);
        }
        let started = now_utc();
        let target = match change.target {
            None => String::new(),
            Some(label) => match self.target(label) {
                Some(c) => c.id,
                None => {
                    self.block(
                        change.spec,
                        &format!("nothing labelled {label} was created in this run"),
                    )?;
                    return Ok(None);
                }
            },
        };
        if change.action == "create"
            && let Err(e) = self.names_ok(&change.body)
        {
            self.settle(change.spec, started, BTreeMap::new(), Found::blocked(e))?;
            return Ok(None);
        }
        let mut draft = json!({"kind": change.kind, "action": change.action, "target": target,
            "body": change.body.to_string()});
        if change.scoped {
            draft["workspace"] = json!(desk.workspace_ref());
        }
        let mut inputs = BTreeMap::new();
        inputs.insert("kind".to_owned(), json!(change.kind));
        inputs.insert("action".to_owned(), json!(change.action));
        if !target.is_empty() {
            inputs.insert("target".to_owned(), json!(sanitize_id(&target)));
        }
        inputs.insert("body".to_owned(), change.body.clone());
        let (result, confirmation) = match apply(desk, &draft).await? {
            // A cleanup whose target the review's complete inventory walk
            // did not find is already absent (an application goes with its
            // last key); an incomplete walk reports a different problem.
            Applied::Review(failure)
                if change.action == "delete"
                    && change.expect == "absent"
                    && failure["problem"] == "notFound" =>
            {
                let mut found = Found::passed(
                    "already absent: the review's complete inventory walk found nothing to delete",
                )
                .with("review_failure", failure);
                found.readback = "absent".to_owned();
                self.settle(change.spec, started, inputs, found)?;
                return Ok(None);
            }
            Applied::Review(failure) => {
                let found = Found::failed("the review refused the draft; nothing was sent")
                    .with("review_failure", failure);
                self.settle(change.spec, started, inputs, found)?;
                return Ok(None);
            }
            Applied::Settled {
                result,
                confirmation,
            } => (result, confirmation),
        };
        let outcome = result["outcome"].as_str().unwrap_or("unknown");
        let readback = result["readback"].as_str().unwrap_or("none").to_owned();
        let mut found = match outcome {
            "completed" if readback == change.expect => Found::passed(format!(
                "applied once after the native confirmation; read back {readback}"
            )),
            "completed" => Found::failed(format!(
                "the write completed but its readback was {readback}, not {}",
                change.expect
            ))
            .stopping("the resulting state is not verified", true),
            "unknown" => Found::failed("the write may or may not have taken effect")
                .stopping("an uncertain outcome", true),
            other => Found::failed(format!(
                "the service did not apply it ({other}); nothing changed"
            )),
        }
        .with("outcome", json!(outcome))
        .with("failure", result["failure"].clone())
        .with("confirmation_title", json!(confirmation));
        found.readback = readback;
        if let Some(steps) = result.get("provisioning").filter(|p| !p.is_null()) {
            // Every step of a multi-step provisioning must have settled and
            // been read back; otherwise what exists must be reconciled.
            let settled = ["scope", "workspace", "binding"]
                .iter()
                .all(|k| steps[*k] == "completed")
                && steps["scopeRead"] == "found"
                && steps["workspaceRead"] == "found"
                && steps["boundRead"] == true;
            if !settled && found.outcome == "passed" {
                found = Found::failed("the provisioning did not settle every step")
                    .stopping("a partially provisioned workspace must be reconciled", true);
            }
            found = found.with("provisioning", steps.clone());
        }
        if result["secret"] != "none" && !result["secret"].is_null() {
            let secret = forget(desk, &result).await?;
            found = found.with("one_time_secret", secret);
        }
        let mut created_id = None;
        if outcome == "completed"
            && let Some(label) = change.creates
        {
            let now = current(&result);
            let id = text(&now[id_field(change.kind)])
                .or_else(|| text(&result["provisioning"]["workspaceId"]));
            if let Some(id) = id {
                found = found.id("created_id", &id);
                self.created.push(Created {
                    label,
                    kind: change.kind,
                    id: id.clone(),
                    scoped: change.scoped,
                });
                created_id = Some(id);
            }
        }
        found.cleanup = if change.creates.is_some() {
            "deleted at the end of the family and read back as absent".to_owned()
        } else if change.action == "delete" {
            "this is the cleanup".to_owned()
        } else {
            "nothing created".to_owned()
        };
        self.settle(change.spec, started, inputs, found)?;
        let _ = created_id;
        Ok((outcome == "completed").then(|| current(&result)))
    }

    /// Deletes every created resource, newest first, unless the run stopped
    /// on an uncertain outcome (then they are left for reconciliation).
    async fn cleanup(&mut self, desk: &mut Desk, specs: &[(&'static str, Spec)]) -> Result<()> {
        let created = std::mem::take(&mut self.created);
        if let Some(stop) = &self.stop
            && stop.reconcile
        {
            let left: Vec<String> = created
                .iter()
                .map(|c| format!("{} {}", c.kind, sanitize_id(&c.id)))
                .collect();
            for (_, spec) in specs {
                let found =
                    Found::blocked("not run: an uncertain outcome must be reconciled first")
                        .with("left_for_reconciliation", json!(left));
                self.run.settle(
                    self.id,
                    *spec,
                    now_utc(),
                    BTreeMap::new(),
                    found,
                    None,
                    &mut self.outcomes,
                )?;
            }
            return Ok(());
        }
        for (label, spec) in specs {
            let Some(item) = created.iter().rev().find(|c| c.label == *label).cloned() else {
                self.run.settle(
                    self.id,
                    *spec,
                    now_utc(),
                    BTreeMap::new(),
                    Found::blocked(format!("nothing labelled {label} was created")),
                    None,
                    &mut self.outcomes,
                )?;
                continue;
            };
            self.created = vec![item.clone()];
            // A key change carries its rotation policy, deletes included
            // (the review checks it on every key change, as v1 did).
            let body = if item.kind == "apiKeys" {
                json!({"rotation_time_interval": 30, "rotation_time_unit": "days"})
            } else {
                json!({})
            };
            self.change(
                desk,
                Change {
                    spec: *spec,
                    kind: item.kind,
                    action: "delete",
                    target: Some(item.label),
                    body,
                    scoped: item.scoped,
                    expect: "absent",
                    creates: None,
                },
            )
            .await?;
        }
        self.created.clear();
        Ok(())
    }

    /// Replaces a designated resource with its own current body: the write
    /// path is exercised and nothing changes.
    async fn replace_same(
        &mut self,
        desk: &mut Desk,
        spec: Spec,
        kind: &'static str,
        resource: &str,
        key: &str,
    ) -> Result<()> {
        // `<resource>_blocked` (a reason) records the write-back as blocked.
        let blocked = format!("{}_blocked", key.trim_end_matches("_id"));
        if let Some(reason) = self.run.nonsecret(self.id, &blocked) {
            return self.block(spec, &reason);
        }
        let Some(id) = self.run.nonsecret(self.id, key) else {
            return self.block(spec, &format!("{} designates no {key}", self.id));
        };
        let detail = desk
            .read(json!({"kind": "detail", "resource": resource, "id": id}))
            .await?;
        let body = match &detail {
            ManagementResult::Detail { detail } => serde_json::to_value(detail)
                .ok()
                .and_then(|d| {
                    d["item"]
                        .as_str()
                        .and_then(|s| serde_json::from_str::<Value>(s).ok())
                })
                .map(|mut item| {
                    if let Some(o) = item.as_object_mut() {
                        for server in [
                            "id",
                            "created_at",
                            "updated_at",
                            "created_by",
                            "updated_by",
                            "status",
                        ] {
                            o.remove(server);
                        }
                    }
                    item
                }),
            _ => None,
        };
        let Some(body) = body else {
            return self.settle(
                spec,
                now_utc(),
                BTreeMap::new(),
                Found::failed("the designated resource could not be read to write it back"),
            );
        };
        self.created.push(Created {
            label: "designated",
            kind,
            id,
            scoped: false,
        });
        self.change(
            desk,
            Change {
                spec,
                kind,
                action: "update",
                target: Some("designated"),
                body,
                scoped: false,
                expect: "found",
                creates: None,
            },
        )
        .await?;
        self.created.retain(|c| c.label != "designated");
        Ok(())
    }
}

pub(crate) async fn run(run: &mut Run<'_>, id: &str) -> Result<FamilyResult> {
    let id: &'static str = match id {
        "L03" => "L03",
        "L04" => "L04",
        _ => "L08",
    };
    let prefix = run.prefix(id);
    let mut ctx = Ctx {
        run,
        id,
        prefix,
        outcomes: Vec::new(),
        created: Vec::new(),
        stop: None,
    };
    let mut desk = match ctx.run.management_desk(id, DeskOptions::default()).await? {
        Ok(d) => d,
        Err(e) => {
            ctx.block(
                Spec {
                    id: leak(format!("{id}-setup")),
                    workflows: &[],
                },
                &e,
            )?;
            return Ok(FamilyResult {
                outcomes: ctx.outcomes,
                stop: None,
            });
        }
    };
    match id {
        "L03" if ctx.run.subject == crate::subject::Subject::Chunk(31) => {
            c31_topic(&mut ctx, &mut desk).await?
        }
        "L03" => l03(&mut ctx, &mut desk).await?,
        "L04" => l04(&mut ctx, &mut desk).await?,
        _ => l08(&mut ctx, &mut desk).await?,
    }
    Ok(FamilyResult {
        outcomes: ctx.outcomes,
        stop: ctx.stop,
    })
}

/// A setup check's name lives for the run (one per family at most).
fn leak(name: String) -> &'static str {
    Box::leak(name.into_boxed_str())
}

fn spec(id: &'static str, workflows: &'static [&'static str]) -> Spec {
    Spec { id, workflows }
}

async fn l03(ctx: &mut Ctx<'_, '_>, desk: &mut Desk) -> Result<()> {
    let p = ctx.prefix.clone().unwrap_or_default();
    let topic = json!({"topic_name": format!("{p}topic"), "description": "C25 disposable topic",
        "examples": ["c25 synthetic example"]});
    let created = ctx
        .change(
            desk,
            Change {
                spec: spec("L03-topic-create", &["W08"]),
                kind: "topics",
                action: "create",
                target: None,
                body: topic.clone(),
                scoped: false,
                expect: "found",
                creates: Some("topic"),
            },
        )
        .await?;
    // The update names the revision the service read back.
    let revision = created
        .as_ref()
        .map(|c| c["revision"].clone())
        .filter(|r| !r.is_null())
        .unwrap_or(json!(1));
    let mut update = topic.clone();
    update["description"] = json!("C25 disposable topic, updated");
    update["revision"] = revision;
    ctx.change(
        desk,
        Change {
            spec: spec("L03-topic-update", &["W08"]),
            kind: "topics",
            action: "update",
            target: Some("topic"),
            body: update,
            scoped: false,
            expect: "found",
            creates: None,
        },
    )
    .await?;
    // The service requires a default policy: the new profile copies the
    // policy of a designated template profile (`profile_template`, else
    // L01's scan profile), as the editor's duplicate does.
    let template = ctx
        .run
        .nonsecret("L03", "profile_template")
        .or_else(|| ctx.run.nonsecret("L01", "profile_name"));
    let policy = match &template {
        None => None,
        Some(name) => {
            let page = desk
                .read(json!({"kind": "page", "resource": "profiles", "offset": 0, "limit": 100}))
                .await?;
            match &page {
                ManagementResult::Page { page } => serde_json::from_str::<Vec<Value>>(&page.items)
                    .unwrap_or_default()
                    .into_iter()
                    .find(|p| p["profile_name"].as_str() == Some(name.as_str()))
                    .map(|p| p["policy"].clone())
                    .filter(|p| p.is_object()),
                _ => None,
            }
        }
    };
    match policy {
        None => ctx.block(
            spec("L03-profile-create", &["W07"]),
            "no template profile's policy could be read (designate profile_template)",
        )?,
        Some(policy) => {
            ctx.change(
                desk,
                Change {
                    spec: spec("L03-profile-create", &["W07"]),
                    kind: "profiles",
                    action: "create",
                    target: None,
                    // Active: the service's inventory omits inactive
                    // profiles, so an inactive one could not be read back
                    // or cleaned up (C25 live finding).
                    body: json!({"profile_name": format!("{p}profile"), "active": true,
                    "policy": policy}),
                    scoped: false,
                    expect: "found",
                    creates: Some("profile"),
                },
            )
            .await?;
        }
    }
    match ctx.run.nonsecret("L03", "deployment_profile") {
        None => {
            ctx.block(
                spec("L03-key-create", &["W10", "W11"]),
                "L03 designates no deployment_profile",
            )?;
            ctx.block(
                spec("L03-key-rotate", &["W10", "W11"]),
                "no key was created",
            )?;
            ctx.block(
                spec("L03-application-update", &["W09"]),
                "no application was created",
            )?;
        }
        Some(dp) => {
            let app = format!("{p}app");
            let env = ctx
                .run
                .nonsecret("L03", "app_environment")
                .unwrap_or_else(|| "staging".to_owned());
            let cloud = ctx
                .run
                .nonsecret("L03", "app_cloud_provider")
                .unwrap_or_else(|| "aws".to_owned());
            ctx.change(
                desk,
                Change {
                    spec: spec("L03-key-create", &["W10", "W11"]),
                    kind: "apiKeys",
                    action: "create",
                    target: None,
                    body: json!({"cust_app": app, "cust_env": env, "cust_cloud_provider": cloud,
                    "api_key_name": format!("{p}key"), "dp_name": dp, "revoked": false,
                    "rotation_time_interval": 30, "rotation_time_unit": "days"}),
                    scoped: false,
                    expect: "found",
                    creates: Some("key"),
                },
            )
            .await?;
            ctx.change(
                desk,
                Change {
                    spec: spec("L03-key-rotate", &["W10", "W11"]),
                    kind: "apiKeys",
                    action: "rotate",
                    target: Some("key"),
                    body: json!({"rotation_time_interval": 90, "rotation_time_unit": "days"}),
                    scoped: false,
                    expect: "found",
                    creates: None,
                },
            )
            .await?;
            // Rotation replaces the key's ID: find it again by name so its
            // cleanup names the current key.
            if !ctx.stopped() {
                let page = desk
                    .read(json!({"kind": "page", "resource": "apiKeys", "offset": 0, "limit": 100}))
                    .await?;
                let key_name = format!("{p}key");
                let current = match &page {
                    ManagementResult::Page { page } => {
                        serde_json::from_str::<Vec<Value>>(&page.items)
                            .unwrap_or_default()
                            .into_iter()
                            .find(|k| k["api_key_name"].as_str() == Some(key_name.as_str()))
                            .and_then(|k| text(&k["api_key_id"]))
                    }
                    _ => None,
                };
                if let Some(id) = current
                    && let Some(key) = ctx.created.iter_mut().rev().find(|c| c.label == "key")
                {
                    key.id = id;
                }
            }
            // The key's creation made its application; find it by name.
            if !ctx.stopped() {
                let page = desk
                    .read(json!({"kind": "page", "resource": "applications", "offset": 0, "limit": 100}))
                    .await?;
                let app_id = match &page {
                    ManagementResult::Page { page } => {
                        serde_json::from_str::<Vec<Value>>(&page.items)
                            .unwrap_or_default()
                            .into_iter()
                            .find(|a| a["app_name"].as_str() == Some(app.as_str()))
                            .and_then(|a| text(&a["customer_appId"]))
                    }
                    _ => None,
                };
                match app_id {
                    Some(id) => {
                        ctx.created.push(Created {
                            label: "application",
                            kind: "applications",
                            id,
                            scoped: false,
                        });
                        ctx.change(desk, Change {
                            spec: spec("L03-application-update", &["W09"]),
                            kind: "applications", action: "update", target: Some("application"),
                            // A real change: the environment moves off the
                            // one the key's creation recorded.
                            body: json!({"app_name": app, "cloud_provider": cloud,
                                "environment": if env == "development" { "staging" } else { "development" }}),
                            scoped: false, expect: "found", creates: None,
                        })
                        .await?;
                    }
                    None => ctx.settle(
                        spec("L03-application-update", &["W09"]),
                        now_utc(),
                        BTreeMap::new(),
                        Found::failed("the key's application is not listed"),
                    )?,
                }
            }
        }
    }
    ctx.cleanup(
        desk,
        &[
            ("key", spec("L03-cleanup-key", &["W10"])),
            ("application", spec("L03-cleanup-application", &["W09"])),
            ("profile", spec("L03-cleanup-profile", &["W07"])),
            ("topic", spec("L03-cleanup-topic", &["W08"])),
        ],
    )
    .await
}

/// C31's live row, inside L03's designation (create, update and delete
/// disposable `airs-v2-c25-*` topics): one topic review applied after the
/// native confirmation and read back, then removed and read back absent.
async fn c31_topic(ctx: &mut Ctx<'_, '_>, desk: &mut Desk) -> Result<()> {
    let p = ctx.prefix.clone().unwrap_or_default();
    let topic = json!({"topic_name": format!("{p}c31-topic"), "description": "C31 review apply",
        "examples": ["c31 synthetic example"]});
    let created = ctx
        .change(
            desk,
            Change {
                spec: spec("C31-topic-create", &["W08"]),
                kind: "topics",
                action: "create",
                target: None,
                body: topic.clone(),
                scoped: false,
                expect: "found",
                creates: Some("topic"),
            },
        )
        .await?;
    let revision = created
        .as_ref()
        .map(|c| c["revision"].clone())
        .filter(|r| !r.is_null())
        .unwrap_or(json!(1));
    let mut update = topic;
    update["description"] = json!("C31 review apply, updated through the review");
    update["revision"] = revision;
    ctx.change(
        desk,
        Change {
            spec: spec("C31-topic-review-apply", &["W08"]),
            kind: "topics",
            action: "update",
            target: Some("topic"),
            body: update,
            scoped: false,
            expect: "found",
            creates: None,
        },
    )
    .await?;
    ctx.cleanup(desk, &[("topic", spec("C31-cleanup-topic", &["W08"]))])
        .await
}

async fn l04(ctx: &mut Ctx<'_, '_>, desk: &mut Desk) -> Result<()> {
    let p = ctx.prefix.clone().unwrap_or_default();
    // The service needs the regex rules and confidence levels a custom
    // pattern detects with (the shape of the tenant's own custom patterns).
    let pattern = json!({"name": format!("{p}pattern"), "type": "custom",
        "description": "C25 disposable pattern",
        "detection_config": {"technique": "regex", "supported_confidence_levels": ["high", "low"]},
        "matching_rules": {"proximity_distance": 200,
            "regexes": [{"regex": "airs-v2-c25-[0-9]{6}", "weight": 1}]}});
    ctx.change(
        desk,
        Change {
            spec: spec("L04-pattern-create", &["W13"]),
            kind: "dataPatterns",
            action: "create",
            target: None,
            body: pattern.clone(),
            scoped: false,
            expect: "found",
            creates: Some("pattern"),
        },
    )
    .await?;
    let mut replaced = pattern;
    replaced["description"] = json!("C25 disposable pattern, replaced");
    ctx.change(
        desk,
        Change {
            spec: spec("L04-pattern-replace", &["W13"]),
            kind: "dataPatterns",
            action: "update",
            target: Some("pattern"),
            body: replaced,
            scoped: false,
            expect: "found",
            creates: None,
        },
    )
    .await?;
    let category = ctx
        .run
        .nonsecret("L04", "dictionary_category")
        .unwrap_or_else(|| "Confidential".to_owned());
    let region = ctx
        .run
        .nonsecret("L04", "dictionary_region")
        .unwrap_or_else(|| "United States".to_owned());
    let metadata = |file: &str| {
        json!({"name": format!("{p}dictionary"), "original_file_name": file,
        "category": category, "region_name": region})
    };
    ctx.change(desk, Change {
        spec: spec("L04-dictionary-create", &["W16"]),
        kind: "dictionaries", action: "create", target: None,
        body: json!({"metadata": metadata("c25-keywords.txt"), "file": "c25-synthetic-keyword\n"}),
        scoped: false, expect: "found", creates: Some("dictionary"),
    })
    .await?;
    ctx.change(desk, Change {
        spec: spec("L04-dictionary-replace", &["W16"]),
        kind: "dictionaries", action: "update", target: Some("dictionary"),
        body: json!({"metadata": metadata("c25-keywords-2.txt"), "file": "c25-synthetic-keyword-2\n"}),
        scoped: false, expect: "found", creates: None,
    })
    .await?;
    if !ctx.stopped() {
        ctx.replace_same(
            desk,
            spec("L04-data-profile-replace", &["W14"]),
            "dataProfiles",
            "dataProfiles",
            "data_profile_id",
        )
        .await?;
    }
    if !ctx.stopped() {
        ctx.replace_same(
            desk,
            spec("L04-filtering-profile-replace", &["W15"]),
            "filteringProfiles",
            "filteringProfiles",
            "filtering_profile_id",
        )
        .await?;
    }
    if !ctx.stopped() {
        let started = now_utc();
        let page = desk
            .read(
                json!({"kind": "page", "resource": "deploymentProfiles", "offset": 0, "limit": 25}),
            )
            .await?;
        let found = Found::check(
            matches!(page, ManagementResult::Page { .. }),
            "the deployment profile catalog was read (catalog-only, no deployment writes)",
            "the deployment catalog could not be read",
        )
        .with("page", crate::live_run::summarize(&page));
        ctx.settle(
            spec("L04-deployment-catalog", &["W12"]),
            started,
            BTreeMap::new(),
            found,
        )?;
    }
    ctx.cleanup(
        desk,
        &[
            ("dictionary", spec("L04-cleanup-dictionary", &["W16"])),
            ("pattern", spec("L04-cleanup-pattern", &["W13"])),
        ],
    )
    .await
}

async fn l08(ctx: &mut Ctx<'_, '_>, desk: &mut Desk) -> Result<()> {
    let p = ctx.prefix.clone().unwrap_or_default();
    // `provision_blocked` (a reason) records provisioning as blocked and
    // runs the route and key changes in the designated workspace instead.
    let workspace = match ctx.run.nonsecret("L08", "provision_blocked") {
        Some(reason) => {
            ctx.block(spec("L08-provision-workspace", &["W37", "W39"]), &reason)?;
            ctx.run.nonsecret("L08", "workspace_id")
        }
        None => {
            ctx.change(
                desk,
                Change {
                    spec: spec("L08-provision-workspace", &["W37", "W39"]),
                    kind: "gatewayWorkspaces",
                    action: "create",
                    target: None,
                    body: json!({"name": format!("{p}ws"),
                    "scope_name": format!("{}scope", p.replace('-', "_")),
                    "existing_scope": false}),
                    scoped: false,
                    expect: "found",
                    creates: Some("workspace"),
                },
            )
            .await?;
            ctx.target("workspace").map(|c| c.id)
        }
    };
    let selected = match workspace {
        Some(ws) if !ctx.stopped() => desk.select_workspace(&ws).await.is_ok(),
        _ => false,
    };
    if selected {
        match ctx.run.nonsecret("L08", "route_provider") {
            None => {
                ctx.block(
                    spec("L08-route-create", &["W37"]),
                    "L08 designates no route_provider",
                )?;
                ctx.block(spec("L08-route-update", &["W37"]), "no route was created")?;
            }
            Some(provider) => {
                // A single-target routing config naming the integration as
                // `@slug` (the Gateway's model catalog form).
                let target = if provider.starts_with('@') {
                    provider.clone()
                } else {
                    format!("@{provider}")
                };
                let config = json!({"strategy": {"mode": "single"},
                    "targets": [{"provider": target}]});
                ctx.change(
                    desk,
                    Change {
                        spec: spec("L08-route-create", &["W37"]),
                        kind: "gatewayRoutes",
                        action: "create",
                        target: None,
                        body: json!({"name": format!("{p}route"), "config": config}),
                        scoped: true,
                        expect: "found",
                        creates: Some("route"),
                    },
                )
                .await?;
                ctx.change(
                    desk,
                    Change {
                        spec: spec("L08-route-update", &["W37"]),
                        kind: "gatewayRoutes",
                        action: "update",
                        target: Some("route"),
                        body: json!({"name": format!("{p}route-2")}),
                        scoped: true,
                        expect: "found",
                        creates: None,
                    },
                )
                .await?;
            }
        }
        ctx.change(desk, Change {
            spec: spec("L08-service-key-create", &["W38"]),
            kind: "gatewayServiceKeys", action: "create", target: None,
            body: json!({"name": format!("{p}key"), "scopes": ["completions.write"], "type": "service"}),
            scoped: true, expect: "found", creates: Some("key"),
        })
        .await?;
        ctx.change(
            desk,
            Change {
                spec: spec("L08-service-key-rotate", &["W38"]),
                kind: "gatewayServiceKeys",
                action: "rotate",
                target: Some("key"),
                body: json!({"key_transition_period_ms": 3_600_000}),
                scoped: true,
                expect: "found",
                creates: None,
            },
        )
        .await?;
    } else {
        for name in [
            "L08-route-create",
            "L08-route-update",
            "L08-service-key-create",
            "L08-service-key-rotate",
        ] {
            ctx.block(
                spec(name, &["W37", "W38"]),
                "the provisioned workspace could not be selected",
            )?;
        }
    }
    ctx.cleanup(
        desk,
        &[
            ("key", spec("L08-cleanup-service-key", &["W38"])),
            ("route", spec("L08-cleanup-route", &["W37"])),
            ("workspace", spec("L08-cleanup-workspace", &["W39"])),
        ],
    )
    .await
}
