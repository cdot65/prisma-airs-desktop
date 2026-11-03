//! L17 Workspace MCP (C25) through the production intents: the selected
//! workspace's catalog (`listMcpServers`), the desktop's own OAuth sign-in
//! to a designated server (`signInMcpServer`; the operator completes the
//! authorization page, and the core's loopback callback finishes it), a
//! scope or grant change the operator makes (the catalog then shows the
//! sign-in stale), and revocation (`signOutMcpServer`). Tool calls, their
//! approval and denial, and token refresh happen inside the native Harness
//! engine and are recorded as needing the desktop app.

use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::{Value, json};

use crate::live_run::{
    Desk, DeskOptions, FamilyResult, Found, Outcomes, Run, Spec, Stop, operator_step,
};
use crate::{Result, now_utc};

const DISCOVERY: Spec = Spec {
    id: "L17-discovery",
    workflows: &["W47"],
};
const SIGN_IN: Spec = Spec {
    id: "L17-sign-in",
    workflows: &["W47"],
};
const SCOPE: Spec = Spec {
    id: "L17-scope-change",
    workflows: &["W48"],
};
const REVOKE: Spec = Spec {
    id: "L17-revocation",
    workflows: &["W48"],
};
const APPROVED: Spec = Spec {
    id: "L17-approved-read-call",
    workflows: &["W48"],
};
const DENIED: Spec = Spec {
    id: "L17-denied-call",
    workflows: &["W48"],
};
const REFRESH: Spec = Spec {
    id: "L17-refresh",
    workflows: &["W48"],
};

fn view(j: &Value) -> Value {
    j["harness"]["mcp"].clone()
}

fn server<'a>(v: &'a Value, id: &str) -> &'a Value {
    v["servers"]
        .as_array()
        .and_then(|s| s.iter().find(|s| s["id"] == id))
        .unwrap_or(&Value::Null)
}

async fn list(desk: &mut Desk, reference: &str) -> Result<Value> {
    let context = desk.context_text()?;
    desk.send_json(json!({"kind": "listMcpServers", "context": context, "workspace": reference}))
        .await?;
    Ok(desk
        .until(desk.settle, |j| view(j)["state"] != "loading")
        .await
        .map(|j| view(&j))
        .unwrap_or_default())
}

struct Ctx<'r, 'a> {
    run: &'r mut Run<'a>,
    outcomes: Outcomes,
    stop: Option<Stop>,
}

impl Ctx<'_, '_> {
    fn settle(
        &mut self,
        spec: Spec,
        started: String,
        inputs: BTreeMap<String, Value>,
        found: Found,
    ) -> Result<()> {
        let stop = self.run.settle(
            "L17",
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

    fn block(&mut self, specs: &[Spec], reason: &str) -> Result<()> {
        for spec in specs {
            self.settle(*spec, now_utc(), BTreeMap::new(), Found::blocked(reason))?;
        }
        Ok(())
    }
}

pub(crate) async fn run(run: &mut Run<'_>, _id: &str) -> Result<FamilyResult> {
    let mut ctx = Ctx {
        run,
        outcomes: Vec::new(),
        stop: None,
    };
    let remote = [DISCOVERY, SIGN_IN, SCOPE, REVOKE];
    for spec in [APPROVED, DENIED, REFRESH] {
        let found = Found::native(
            "MCP tool calls, their native approval or denial, and token refresh run inside the \
             Harness engine, which only the desktop app launches; run this with the native \
             acceptance driver against the designated server",
        );
        ctx.settle(spec, now_utc(), BTreeMap::new(), found)?;
    }
    let designated_server = ctx.run.nonsecret("L17", "mcp_server_id");
    let server_name = ctx.run.nonsecret("L17", "mcp_server_name");
    let (Some(workspace), true, Some(endpoint)) = (
        ctx.run.nonsecret("L17", "workspace_id"),
        designated_server.is_some() || server_name.is_some(),
        ctx.run.nonsecret("L17", "inference_endpoint"),
    ) else {
        ctx.block(
            &remote,
            "L17 needs workspace_id, mcp_server_id (or mcp_server_name) and inference_endpoint",
        )?;
        return Ok(FamilyResult {
            outcomes: ctx.outcomes,
            stop: None,
        });
    };
    let mut desk = match ctx
        .run
        .management_desk("L17", DeskOptions::default())
        .await?
    {
        Ok(d) => d,
        Err(e) => {
            ctx.block(&remote, &e)?;
            return Ok(FamilyResult {
                outcomes: ctx.outcomes,
                stop: None,
            });
        }
    };
    let reference = match desk.select_workspace(&workspace).await {
        Ok(r) => r,
        Err(e) => {
            ctx.block(&remote, &e.to_string())?;
            return Ok(FamilyResult {
                outcomes: ctx.outcomes,
                stop: None,
            });
        }
    };
    let Some(key) = ctx.run.secret("L17", "inference_key")? else {
        ctx.block(&remote, "L17 designates no inference_key credential")?;
        return Ok(FamilyResult {
            outcomes: ctx.outcomes,
            stop: None,
        });
    };
    let context = desk.context_text()?;
    desk.send_json(
        json!({"kind": "configureInferenceCredential", "context": context,
        "mode": "session", "workspace": reference, "endpoint": endpoint, "secret": (*key).clone()}),
    )
    .await?;
    let _ = desk
        .until(desk.settle, |j| {
            j["tenants"]
                .as_array()
                .is_some_and(|t| t.iter().any(|t| t["inference"]["state"] == "ready"))
        })
        .await;
    let mut inputs = ctx.run.inputs("L17", &["client_secret", "inference_key"]);
    inputs.insert("endpoint".to_owned(), json!(endpoint));

    let started = now_utc();
    let v = list(&mut desk, &reference).await?;
    // The designated server, or the one catalog server whose name contains
    // `mcp_server_name` (case-insensitive); more than one match is refused.
    let (server_id, discovered) = match designated_server {
        Some(id) => (id, None),
        None => {
            let needle = server_name.clone().unwrap_or_default().to_lowercase();
            let matches: Vec<&Value> = v["servers"]
                .as_array()
                .map(|s| {
                    s.iter()
                        .filter(|s| {
                            s["name"]
                                .as_str()
                                .is_some_and(|n| n.to_lowercase().contains(&needle))
                        })
                        .collect()
                })
                .unwrap_or_default();
            match matches.as_slice() {
                [only] => (
                    only["id"].as_str().unwrap_or_default().to_owned(),
                    Some(json!({"by": "mcp_server_name", "name": only["name"]})),
                ),
                other => {
                    let names: Vec<Value> = v["servers"]
                        .as_array()
                        .map(|s| s.iter().map(|s| s["name"].clone()).collect())
                        .unwrap_or_default();
                    (
                        String::new(),
                        Some(json!({"by": "mcp_server_name", "matches": other.len(),
                            "catalog_names": names})),
                    )
                }
            }
        }
    };
    let listed = !server_id.is_empty() && !server(&v, &server_id).is_null();
    let mut found = Found::check(
        v["state"] == "ready" && listed,
        "the workspace's MCP catalog was read and lists the designated server (its upstream URL is never kept)",
        "the catalog was not read or does not list exactly one designated server",
    )
    .with("state", v["state"].clone())
    .with("servers", json!(v["servers"].as_array().map(Vec::len)));
    if let Some(d) = discovered {
        found = found.with("discovered", d);
    }
    if !server_id.is_empty() {
        found = found.id("mcp_server_id", &server_id);
    }
    ctx.settle(DISCOVERY, started, inputs.clone(), found)?;
    if !listed {
        ctx.block(
            &[SIGN_IN, SCOPE, REVOKE],
            "the designated server is not in the catalog",
        )?;
        return Ok(FamilyResult {
            outcomes: ctx.outcomes,
            stop: ctx.stop,
        });
    }
    // `operator_present = no`: the browser and console steps wait for a run
    // with the operator; they are recorded blocked, never attempted.
    if ctx.run.nonsecret("L17", "operator_present").as_deref() == Some("no") {
        ctx.block(
            &[SIGN_IN, SCOPE, REVOKE],
            "the operator was not present for the sign-in; run L17 again with the operator",
        )?;
        return Ok(FamilyResult {
            outcomes: ctx.outcomes,
            stop: ctx.stop,
        });
    }

    let started = now_utc();
    let context = desk.context_text()?;
    desk.send_json(
        json!({"kind": "signInMcpServer", "context": context, "workspace": reference,
        "server": server_id}),
    )
    .await?;
    let bound = ctx.run.operator_timeout("L17");
    let (done, step) = {
        let desk = &desk;
        let id = server_id.clone();
        operator_step(
            ctx.run.operator.as_ref(),
            "mcp-sign-in",
            "authorize the desktop for the designated MCP server with the designated test identity",
            None,
            bound,
            async || {
                desk.json()
                    .map(|j| server(&view(&j), &id)["signedIn"] == true)
                    .unwrap_or(false)
            },
        )
        .await
    };
    let mut found = Found::check(
        done,
        "the operator authorized the server; the desktop exchanged its own code and holds the grant (memory and v2 Keychain only)",
        "the MCP sign-in was not completed within the bound",
    );
    found.operator_step = Some(step);
    ctx.settle(SIGN_IN, started, inputs.clone(), found)?;
    if !done {
        ctx.block(&[SCOPE, REVOKE], "the server was not signed in")?;
        return Ok(FamilyResult {
            outcomes: ctx.outcomes,
            stop: ctx.stop,
        });
    }

    let started = now_utc();
    let found = if ctx.run.nonsecret("L17", "scope_change").as_deref() != Some("designated") {
        Found::blocked("L17 designates no scope_change the operator can make")
    } else {
        let bound = ctx.run.operator_timeout("L17");
        let (done, step) = {
            let desk = &mut desk;
            let id = server_id.clone();
            let reference = reference.clone();
            operator_step(
                ctx.run.operator.as_ref(),
                "mcp-scope-change",
                "change the designated MCP server's scope or grant in the Gateway now",
                None,
                bound,
                async || {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    match list(desk, &reference).await {
                        Ok(v) => {
                            let s = server(&v, &id);
                            s["stale"] == true || s["signedIn"] == false
                        }
                        Err(_) => false,
                    }
                },
            )
            .await
        };
        let mut found = Found::check(
            done,
            "after the operator's change, the catalog showed the sign-in stale and it no longer counted",
            "the changed scope or grant was not noticed",
        );
        found.operator_step = Some(step);
        found
    };
    ctx.settle(SCOPE, started, inputs.clone(), found)?;

    let started = now_utc();
    let context = desk.context_text()?;
    desk.send_json(
        json!({"kind": "signOutMcpServer", "context": context, "workspace": reference,
        "server": server_id}),
    )
    .await?;
    let out = desk
        .until(desk.settle, |j| {
            server(&view(j), &server_id)["signedIn"] == false
        })
        .await
        .is_some();
    let mut found = Found::check(
        out,
        "signed out: the desktop revoked the tokens and deleted their item",
        "the sign-out did not take effect",
    );
    found.cleanup = "the grant was revoked".to_owned();
    ctx.settle(REVOKE, started, inputs, found)?;
    Ok(FamilyResult {
        outcomes: ctx.outcomes,
        stop: ctx.stop,
    })
}
