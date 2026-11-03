//! L09 Gateway inference (C25): bounded streamed runs through the
//! production Playground (`configureInferenceCredential`, `runInference`,
//! `stopInference`) in the designated workspace, with its own inference key
//! (never the management client). Each run is charged against the `runs`
//! budget before it starts; the tokens the Gateway reports are charged to
//! `tokens` afterwards and a run over the cap stops the family. The core
//! sends only to the configured Gateway endpoint; there is no upstream
//! fallback for the runner to take.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::live_run::{Desk, DeskOptions, FamilyResult, Found, Outcomes, Run, Spec, Stop};
use crate::{Result, now_utc};

const STREAM: Spec = Spec {
    id: "L09-stream",
    workflows: &["W40"],
};
const CANCEL: Spec = Spec {
    id: "L09-cancel",
    workflows: &["W40"],
};
const ERROR: Spec = Spec {
    id: "L09-error-recovery",
    workflows: &["W40"],
};

/// A short synthetic prompt (bounded output).
const PROMPT: &str = "Reply with the single word: ready.";
/// A prompt that streams long enough to be stopped.
const LONG: &str = "Count from 1 to 200, one number per line.";

/// `prompt` made unique to this run, so a Gateway response cache cannot
/// answer it: a cached answer is replayed as one chunk without
/// `finish_reason`, which the SDK's stream contract refuses (C25).
fn fresh(prompt: &str) -> String {
    format!("{prompt} (C25 run {})", now_utc())
}

async fn start(desk: &mut Desk, reference: &str, run: Value) -> Result<Value> {
    let context = desk.context_text()?;
    desk.admit(
        json!({"kind": "runInference", "context": context, "workspace": reference, "run": run}),
    )
    .await
    .map(|op| json!(op))
}

fn playground(j: &Value) -> Value {
    j["gateway"]["playground"].clone()
}

async fn ended(desk: &Desk) -> Value {
    desk.until(desk.settle, |j| {
        playground(j)["state"]
            .as_str()
            .is_some_and(|s| s != "running")
    })
    .await
    .map(|j| playground(&j))
    .unwrap_or_else(|| json!({"state": "running"}))
}

fn shown(view: &Value) -> Found {
    Found::new("passed", "")
        .with("state", view["state"].clone())
        .with("outcome", view["outcome"].clone())
        .with("failure", view["failure"].clone())
        .with("sent", view["sent"].clone())
        .with("answered", view["answered"].clone())
        .with("served_model", view["servedModel"].clone())
        .with("finish", view["finish"].clone())
        .with("usage", view["usage"].clone())
        .with("output_chars", json!(view["output"].as_str().map(str::len)))
}

fn ids(mut found: Found, view: &Value) -> Found {
    if let Some(trace) = view["trace"].as_str() {
        found = found.id("trace", trace);
    }
    if let Some(response) = view["responseId"].as_str() {
        found = found.id("response_id", response);
    }
    found
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
            "L09",
            spec,
            started,
            inputs,
            found,
            Some("L09"),
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

    /// Charges one run; a run over the cap is never started.
    fn charge(&mut self) -> std::result::Result<(), String> {
        self.run.guard.spend("L09", "runs", 1)
    }

    /// Charges the tokens the Gateway reported; over the cap stops.
    fn tokens(&mut self, view: &Value) -> Option<String> {
        let total = view["usage"]["total"].as_u64()?;
        self.run.guard.report("L09", "tokens", total).err()
    }
}

pub(crate) async fn run(run: &mut Run<'_>, _id: &str) -> Result<FamilyResult> {
    let mut ctx = Ctx {
        run,
        outcomes: Vec::new(),
        stop: None,
    };
    let all = [STREAM, CANCEL, ERROR];
    let (Some(workspace), Some(endpoint), Some(model)) = (
        ctx.run.nonsecret("L09", "workspace_id"),
        ctx.run.nonsecret("L09", "inference_endpoint"),
        ctx.run.nonsecret("L09", "model"),
    ) else {
        ctx.block(&all, "L09 needs workspace_id, inference_endpoint and model")?;
        return Ok(FamilyResult {
            outcomes: ctx.outcomes,
            stop: None,
        });
    };
    let route = ctx.run.nonsecret("L09", "route");
    let mut desk = match ctx
        .run
        .management_desk("L09", DeskOptions::default())
        .await?
    {
        Ok(d) => d,
        Err(e) => {
            ctx.block(&all, &e)?;
            return Ok(FamilyResult {
                outcomes: ctx.outcomes,
                stop: None,
            });
        }
    };
    let reference = match desk.select_workspace(&workspace).await {
        Ok(r) => r,
        Err(e) => {
            ctx.block(&all, &e.to_string())?;
            return Ok(FamilyResult {
                outcomes: ctx.outcomes,
                stop: None,
            });
        }
    };
    let Some(key) = ctx.run.secret("L09", "inference_key")? else {
        ctx.block(&all, "L09 designates no inference_key credential")?;
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
    if desk
        .until(desk.settle, |j| {
            j["tenants"]
                .as_array()
                .is_some_and(|t| t.iter().any(|t| t["inference"]["state"] == "ready"))
        })
        .await
        .is_none()
    {
        ctx.block(
            &all,
            "the inference key could not be stored for this session",
        )?;
        return Ok(FamilyResult {
            outcomes: ctx.outcomes,
            stop: None,
        });
    }
    let mut inputs = ctx.run.inputs("L09", &["client_secret", "inference_key"]);
    inputs.insert("endpoint".to_owned(), json!(endpoint));

    // A complete streamed answer.
    let started = now_utc();
    let found = match ctx.charge() {
        Err(e) => Found::blocked(e.clone()).stopping(format!("unexpected cost: {e}"), false),
        Ok(()) => {
            start(
                &mut desk,
                &reference,
                json!({"model": model, "route": route, "prompt": fresh(PROMPT)}),
            )
            .await?;
            let view = ended(&desk).await;
            let mut found = shown(&view);
            let complete = view["state"] == "completed" && view["outcome"] == "completed";
            found.outcome = if complete { "passed" } else { "failed" };
            found.detail = if complete {
                "a streamed answer through the designated Gateway route, with the model it served"
                    .to_owned()
            } else {
                "the run did not complete".to_owned()
            };
            if view["state"] == "running" {
                found = found.stopping(
                    "the run did not end in time; it may still be consuming",
                    true,
                );
            } else if let Some(over) = ctx.tokens(&view) {
                found = found.stopping(format!("unexpected cost: {over}"), false);
            }
            ids(found, &view)
        }
    };
    ctx.settle(STREAM, started, inputs.clone(), found)?;
    if ctx.stop.is_some() {
        return Ok(FamilyResult {
            outcomes: ctx.outcomes,
            stop: ctx.stop,
        });
    }

    // Stopped after the first output arrives.
    let started = now_utc();
    let found = match ctx.charge() {
        Err(e) => Found::blocked(e.clone()).stopping(format!("unexpected cost: {e}"), false),
        Ok(()) => {
            start(
                &mut desk,
                &reference,
                json!({"model": model, "route": route, "prompt": fresh(LONG)}),
            )
            .await?;
            let streaming = desk
                .until(desk.settle, |j| {
                    let p = playground(j);
                    p["output"].as_str().is_some_and(|o| !o.is_empty()) || p["state"] != "running"
                })
                .await
                .map(|j| playground(&j))
                .unwrap_or_default();
            if streaming["state"] == "running" {
                desk.send_json(
                    json!({"kind": "stopInference", "operation": streaming["operation"]}),
                )
                .await?;
            }
            let view = ended(&desk).await;
            let mut found = shown(&view);
            let stopped = view["state"] == "stopped";
            found.outcome = if stopped { "passed" } else { "failed" };
            found.detail = if stopped {
                "the stream was stopped by the operator's Stop; whether it was charged is reported as unknown, never assumed".to_owned()
            } else {
                "the run ended before it could be stopped".to_owned()
            };
            if let Some(over) = ctx.tokens(&view) {
                found = found.stopping(format!("unexpected cost: {over}"), false);
            }
            ids(found, &view)
        }
    };
    ctx.settle(CANCEL, started, inputs.clone(), found)?;
    if ctx.stop.is_some() {
        return Ok(FamilyResult {
            outcomes: ctx.outcomes,
            stop: ctx.stop,
        });
    }

    // A model the route does not serve: refused, nothing sent elsewhere,
    // and the Playground recovers for the next run.
    let started = now_utc();
    let missing = ctx
        .run
        .nonsecret("L09", "unavailable_model")
        .unwrap_or_else(|| "c25-unavailable-model".to_owned());
    let found = match ctx.charge() {
        Err(e) => Found::blocked(e.clone()).stopping(format!("unexpected cost: {e}"), false),
        Ok(()) => {
            start(
                &mut desk,
                &reference,
                json!({"model": missing, "prompt": PROMPT}),
            )
            .await?;
            let view = ended(&desk).await;
            let mut found = shown(&view);
            let refused = view["state"] == "failed" && view["servedModel"].is_null();
            let context = desk.context_text()?;
            let cleared = desk
                .send_json(json!({"kind": "clearInference", "operation": view["operation"]}))
                .await
                .is_ok();
            found.outcome = if refused && cleared {
                "passed"
            } else {
                "failed"
            };
            found.detail = if refused {
                "the Gateway refused the unknown model; no answer and no other model were shown, and the run was cleared".to_owned()
            } else {
                "an unknown model did not fail as expected".to_owned()
            };
            let _ = context;
            ids(found, &view)
        }
    };
    ctx.settle(ERROR, started, inputs, found)?;
    Ok(FamilyResult {
        outcomes: ctx.outcomes,
        stop: ctx.stop,
    })
}
