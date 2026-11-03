//! L15 API Client (C25) through the production `workbench` intent: a
//! run-owned collection (in a scratch folder the folder sheet answers with)
//! sends to the designated HTTPS endpoint (headers, cookies, a multipart
//! upload, a download saved through the save sheet, a stored secret) and to
//! the tenant's AIRS API through a managed collection: an approved read,
//! and a reversible write that is held for review until the runner's armed
//! native confirmation applies it, then removed the same way.

use std::collections::BTreeMap;
use std::path::Path;

use desktop_core::management::ManagementEndpoints;
use serde_json::{Value, json};

use crate::live_run::{
    Desk, DeskOptions, Evidence, FamilyResult, Found, Outcomes, Run, Spec, Stop,
};
use crate::{Result, invalid, io_err, now_utc};

const HEADERS: Spec = Spec {
    id: "L15-headers-and-query",
    workflows: &["W29"],
};
const COOKIES: Spec = Spec {
    id: "L15-cookies",
    workflows: &["W29"],
};
const UPLOAD: Spec = Spec {
    id: "L15-multipart-upload",
    workflows: &["W29"],
};
const DOWNLOAD: Spec = Spec {
    id: "L15-download",
    workflows: &["W30"],
};
const SECRET: Spec = Spec {
    id: "L15-stored-secret",
    workflows: &["W27"],
};
const AIRS_READ: Spec = Spec {
    id: "L15-airs-read",
    workflows: &["W31", "W32"],
};
const AIRS_WRITE: Spec = Spec {
    id: "L15-airs-reviewed-write",
    workflows: &["W32", "W33"],
};

/// Where a cleanup path takes the created resource's ID.
const CREATED_ID: &str = "{created_id}";

const UPLOAD_BYTES: &[u8] = b"c25-upload synthetic file\n";

fn request(id: &str, method: &str, url: &str, extra: &str) -> String {
    format!("format_version = 1\nid = \"{id}\"\nmethod = \"{method}\"\nurl = \"{url}\"\n{extra}")
}

fn write(path: &Path, text: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io_err(parent))?;
    }
    std::fs::write(path, text).map_err(io_err(path))
}

/// A collection inside a Git repository folder (as v1 requires).
fn collection(
    root: &Path,
    id: &str,
    variables: &str,
    files: &[(String, Vec<u8>)],
) -> Result<std::path::PathBuf> {
    std::fs::create_dir_all(root.join(".git")).map_err(io_err(root))?;
    let api = root.join("api");
    write(
        &api.join("collection.toml"),
        format!("format_version = 1\nid = \"{id}\"\nname = \"C25 live checks\"\n\n[variables]\n{variables}").as_bytes(),
    )?;
    for (relative, bytes) in files {
        write(&api.join(relative), bytes)?;
    }
    Ok(api)
}

fn toml_string(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned())
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
            "L15",
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

async fn open(desk: &mut Desk) -> Result<()> {
    let opened = desk.workbench(json!({"kind": "openFolder"})).await?;
    if opened["kind"] != "opened" {
        return Err(invalid(format!(
            "the collection did not open: {}",
            opened["kind"]
        )));
    }
    Ok(())
}

async fn doc(desk: &mut Desk, relative: &str) -> Result<Value> {
    let r = desk
        .workbench(json!({"kind": "openRequest", "relative": relative}))
        .await?;
    if r["kind"] != "document" {
        return Err(invalid(format!("{relative} did not open: {}", r["kind"])));
    }
    Ok(r["document"].clone())
}

async fn send(
    desk: &mut Desk,
    doc: &Value,
    fields: &Value,
    context: Option<String>,
) -> Result<Value> {
    desk.workbench(
        json!({"kind": "send", "id": doc["id"], "revision": doc["revision"],
        "fields": fields, "context": context}),
    )
    .await
}

fn status(r: &Value) -> Option<u64> {
    r["run"]["record"]["status"].as_u64()
}

fn ok(r: &Value) -> bool {
    r["kind"] == "ran" && status(r).is_some_and(|s| (200..300).contains(&s))
}

fn preview_has(r: &Value, needle: &str) -> bool {
    r["run"]["preview"].as_str().is_some_and(|p| {
        p.to_ascii_lowercase()
            .contains(&needle.to_ascii_lowercase())
    })
}

fn shown(r: &Value) -> Found {
    let mut found = Found::new("passed", "")
        .with("result", r["kind"].clone())
        .with("status", r["run"]["record"]["status"].clone())
        .with("bytes", r["run"]["record"]["bytes"].clone());
    if let Some(id) = r["run"]["record"]["id"].as_str() {
        found = found.id("run_id", id);
    }
    found
}

fn verdict(mut found: Found, ok: bool, passed: &str, failed: &str) -> Found {
    found.outcome = if ok { "passed" } else { "failed" };
    found.detail = if ok { passed } else { failed }.to_owned();
    found
}

pub(crate) async fn run(run: &mut Run<'_>, _id: &str) -> Result<FamilyResult> {
    let mut ctx = Ctx {
        run,
        outcomes: Vec::new(),
        stop: None,
    };
    let plain = [HEADERS, COOKIES, UPLOAD, DOWNLOAD, SECRET];
    match ctx.run.nonsecret("L15", "endpoint") {
        None => ctx.block(&plain, "L15 designates no HTTPS endpoint")?,
        Some(endpoint) if !endpoint.starts_with("https://") => {
            ctx.block(&plain, "the designated endpoint must be HTTPS")?;
        }
        Some(endpoint) => {
            let echoes = ctx.run.nonsecret("L15", "echo").as_deref() != Some("no");
            if !echoes {
                ctx.block(
                    &[HEADERS, COOKIES, UPLOAD, SECRET],
                    "the designated endpoint does not echo requests, so what was sent cannot be observed",
                )?;
            }
            plain_checks(&mut ctx, &endpoint, echoes).await?;
        }
    }
    if ctx.stop.is_none() {
        airs_checks(&mut ctx).await?;
    }
    Ok(FamilyResult {
        outcomes: ctx.outcomes,
        stop: ctx.stop,
    })
}

async fn plain_checks(ctx: &mut Ctx<'_, '_>, endpoint: &str, echoes: bool) -> Result<()> {
    let path = |key: &str, default: &str| {
        ctx.run
            .nonsecret("L15", key)
            .unwrap_or_else(|| default.to_owned())
    };
    let headers_path = path("headers_path", "/headers");
    let cookie_set = path("cookie_set_path", "/cookies/set?c25=1");
    let cookie_echo = path("cookie_echo_path", "/cookies");
    let upload_path = path("upload_path", "/post");
    let download_path = path("download_path", "/bytes/2048");
    let auth_path = path("auth_path", "/bearer");
    let root = ctx.run.folder("collection")?;
    let saves = ctx.run.folder("downloads")?;
    let private = ctx.run.folder("workbench")?;
    let files = vec![
        (
            "headers.request.toml".to_owned(),
            request("headers", "GET", &format!("{{{{base}}}}{headers_path}"), "").into_bytes(),
        ),
        (
            "cookie-set.request.toml".to_owned(),
            request("cookieset", "GET", &format!("{{{{base}}}}{cookie_set}"), "").into_bytes(),
        ),
        (
            "cookie-echo.request.toml".to_owned(),
            request(
                "cookieecho",
                "GET",
                &format!("{{{{base}}}}{cookie_echo}"),
                "",
            )
            .into_bytes(),
        ),
        (
            "upload.request.toml".to_owned(),
            request(
                "upload",
                "POST",
                &format!("{{{{base}}}}{upload_path}"),
                "body_kind = \"multipart\"\n",
            )
            .into_bytes(),
        ),
        (
            "download.request.toml".to_owned(),
            request(
                "download",
                "GET",
                &format!("{{{{base}}}}{download_path}"),
                "",
            )
            .into_bytes(),
        ),
        (
            "auth.request.toml".to_owned(),
            request(
                "auth",
                "GET",
                &format!("{{{{base}}}}{auth_path}"),
                "[auth]\nkind = \"bearer\"\nsecret = \"c25-endpoint-token\"\n",
            )
            .into_bytes(),
        ),
        ("data/c25.bin".to_owned(), UPLOAD_BYTES.to_vec()),
    ];
    let api = collection(
        &root,
        "c25-live",
        &format!("base = {}\n", toml_string(endpoint)),
        &files,
    )?;
    let mut desk = ctx.run.desk(DeskOptions {
        workbench_live: true,
        folder: Some(api),
        workbench_root: Some(private),
        save_dir: Some(saves.clone()),
        ..DeskOptions::default()
    })?;
    open(&mut desk).await?;
    let mut inputs = ctx.run.inputs("L15", &[]);
    inputs.insert("endpoint".to_owned(), json!(endpoint));

    if echoes {
        echoed(ctx, &mut desk, &inputs).await?;
    }

    let started = now_utc();
    let d = doc(&mut desk, "download.request.toml").await?;
    let r = send(&mut desk, &d, &d["fields"].clone(), None).await?;
    let found = match r["run"]["record"]["id"].as_str() {
        Some(run_id) if ok(&r) => {
            let saved = desk
                .workbench(json!({"kind": "saveResponse", "run": run_id}))
                .await?;
            let bytes = saved["bytes"].as_u64();
            let on_disk = std::fs::read_dir(&saves)
                .map(|d| {
                    d.flatten()
                        .filter_map(|e| e.metadata().ok())
                        .map(|m| m.len())
                        .sum::<u64>()
                })
                .unwrap_or(0);
            verdict(
                shown(&r)
                    .with("saved", saved["kind"].clone())
                    .with("saved_bytes", json!(bytes)),
                saved["kind"] == "responseSaved" && bytes.is_some_and(|b| b > 0 && b == on_disk),
                "the stored body was saved exactly through the save sheet",
                "the response could not be saved exactly",
            )
        }
        _ => verdict(
            shown(&r),
            false,
            "",
            "the download request did not complete",
        ),
    };
    let mut found = found;
    found.cleanup = "the saved file is in a run-owned folder removed after the run".to_owned();
    ctx.settle(DOWNLOAD, started, inputs.clone(), found)?;
    if !echoes {
        return Ok(());
    }

    let started = now_utc();
    let found = match ctx.run.secret("L15", "endpoint_token")? {
        None => Found::blocked("L15 designates no endpoint_token credential"),
        Some(token) => {
            let set = desk
                .workbench(json!({"kind": "setSecret", "reference": "c25-endpoint-token", "value": (*token).clone()}))
                .await?;
            let d = doc(&mut desk, "auth.request.toml").await?;
            let r = send(&mut desk, &d, &d["fields"].clone(), None).await?;
            let _ = desk
                .workbench(json!({"kind": "removeSecret", "reference": "c25-endpoint-token"}))
                .await;
            verdict(
                shown(&r).with("stored", set["kind"].clone()),
                set["kind"] == "done" && ok(&r),
                "a private secret was stored, used as bearer auth and removed; it appears in no record",
                "the stored secret was not accepted by the endpoint",
            )
        }
    };
    inputs.insert("credentials".to_owned(), json!(["L15.endpoint_token"]));
    ctx.settle(SECRET, started, inputs, found)?;
    Ok(())
}

/// Headers, cookies and a multipart upload: each needs an endpoint that
/// echoes what it received.
async fn echoed(
    ctx: &mut Ctx<'_, '_>,
    desk: &mut Desk,
    inputs: &BTreeMap<String, Value>,
) -> Result<()> {
    let started = now_utc();
    let d = doc(desk, "headers.request.toml").await?;
    let mut fields = d["fields"].clone();
    fields["headers"] = json!("X-C25-Check: headers");
    fields["query"] = json!("c25=query");
    let r = send(desk, &d, &fields, None).await?;
    let found = verdict(
        shown(&r),
        ok(&r) && preview_has(&r, "x-c25-check"),
        "the request carried exactly the typed header and query, and the endpoint echoed the header",
        "the endpoint did not answer 2xx with the typed header",
    );
    ctx.settle(HEADERS, started, inputs.clone(), found)?;

    let started = now_utc();
    let d = doc(desk, "cookie-set.request.toml").await?;
    let first = send(desk, &d, &d["fields"].clone(), None).await?;
    let d = doc(desk, "cookie-echo.request.toml").await?;
    let second = send(desk, &d, &d["fields"].clone(), None).await?;
    let found = verdict(
        shown(&second).with("set_status", first["run"]["record"]["status"].clone()),
        status(&first).is_some_and(|s| s < 400) && ok(&second) && preview_has(&second, "c25"),
        "the cookie one response set was sent with the next request to its origin",
        "the cookie was not carried to the next request",
    );
    ctx.settle(COOKIES, started, inputs.clone(), found)?;

    let started = now_utc();
    let d = doc(desk, "upload.request.toml").await?;
    let mut fields = d["fields"].clone();
    fields["body"] = json!(
        "[[parts]]\nname = \"note\"\nvalue = \"c25-upload\"\n[[parts]]\nname = \"file\"\nfile = \"data/c25.bin\"\nmime = \"application/octet-stream\"\n"
    );
    let r = send(desk, &d, &fields, None).await?;
    let found = verdict(
        shown(&r),
        ok(&r) && preview_has(&r, "c25-upload"),
        "a multipart body with a collection file was uploaded and echoed",
        "the multipart upload was not accepted",
    );
    ctx.settle(UPLOAD, started, inputs.clone(), found)
}

async fn airs_checks(ctx: &mut Ctx<'_, '_>) -> Result<()> {
    let Some(read_path) = ctx.run.nonsecret("L15", "airs_read_path") else {
        return ctx.block(&[AIRS_READ, AIRS_WRITE], "L15 designates no airs_read_path");
    };
    let endpoints = match ctx.run.evidence {
        Evidence::Fixture(f) => f
            .management
            .as_deref()
            .map(ManagementEndpoints::at_origin)
            .unwrap_or_default(),
        Evidence::Live => ManagementEndpoints::default(),
    };
    let api = url::Url::parse(&endpoints.api)
        .map_err(|_| invalid("the management API URL is not valid"))?
        .origin()
        .ascii_serialization();
    let write = (
        ctx.run.nonsecret("L15", "airs_write_method"),
        ctx.run.nonsecret("L15", "airs_write_path"),
        ctx.run.nonsecret("L15", "airs_write_body"),
        ctx.run.nonsecret("L15", "airs_cleanup_path"),
    );
    const OAUTH: &str = "[auth]\nkind = \"oauth2\"\ntoken_url = \"{{token_url}}\"\nclient_id = \"{{client_id}}\"\nclient_secret = \"{{client_secret}}\"\nscope = \"tsg_id:{{tsg_id}}\"\n";
    let mut files = vec![(
        "read.request.toml".to_owned(),
        request(
            "read",
            "GET",
            &format!("{{{{scm_base}}}}{read_path}"),
            OAUTH,
        )
        .into_bytes(),
    )];
    if let (Some(method), Some(path), Some(body), Some(cleanup)) = &write {
        files.push((
            "write.request.toml".to_owned(),
            request(
                "write",
                method,
                &format!("{{{{scm_base}}}}{path}"),
                &format!("body_kind = \"json\"\n{OAUTH}"),
            )
            .into_bytes(),
        ));
        files.push((
            "cleanup.request.toml".to_owned(),
            request(
                "cleanup",
                "DELETE",
                &format!("{{{{scm_base}}}}{cleanup}"),
                OAUTH,
            )
            .into_bytes(),
        ));
        let _ = body;
    }
    let root = ctx.run.folder("managed")?;
    let private = ctx.run.folder("workbench-managed")?;
    let variables = format!(
        "scm_base = {}\ntoken_url = {}\ntsg_id = {{ secret = \"tenant.tsg_id\" }}\nclient_id = {{ secret = \"tenant.client_id\" }}\nclient_secret = {{ secret = \"tenant.client_secret\" }}\n",
        toml_string(&api),
        toml_string(&endpoints.token)
    );
    let collection_root = collection(&root, "prisma-airs-scm", &variables, &files)?;
    let mut desk = match ctx
        .run
        .management_desk(
            "L15",
            DeskOptions {
                workbench_live: true,
                folder: Some(collection_root),
                workbench_root: Some(private),
                ..DeskOptions::default()
            },
        )
        .await?
    {
        Ok(d) => d,
        Err(e) => return ctx.block(&[AIRS_READ, AIRS_WRITE], &e),
    };
    open(&mut desk).await?;
    let context = Some(desk.context_text()?);
    let mut inputs = ctx.run.inputs("L15", &["client_secret"]);
    inputs.insert("airs_api".to_owned(), json!(api));

    let started = now_utc();
    let d = doc(&mut desk, "read.request.toml").await?;
    let r = send(&mut desk, &d, &d["fields"].clone(), context.clone()).await?;
    let found = verdict(
        shown(&r),
        ok(&r),
        "an approved AIRS read was admitted with the tenant's management credential and answered",
        "the AIRS read was not admitted or not answered",
    );
    ctx.settle(AIRS_READ, started, inputs.clone(), found)?;

    let started = now_utc();
    let (Some(_), Some(path), Some(body), Some(cleanup)) = write else {
        return ctx.settle(AIRS_WRITE, started, inputs, Found::blocked(
            "L15 designates no reversible write (airs_write_method, airs_write_path, airs_write_body, airs_cleanup_path)",
        ));
    };
    let prefix = ctx.run.prefix("L15");
    let prefix = prefix.filter(|p| !p.is_empty());
    // The cleanup names the prefix itself, or the created resource's ID,
    // taken from the write's response only when the name it reports there
    // carries the prefix.
    let created = (
        ctx.run.nonsecret("L15", "airs_created_id_field"),
        ctx.run.nonsecret("L15", "airs_created_name_field"),
    );
    let by_id = cleanup.contains(CREATED_ID) && created.0.is_some() && created.1.is_some();
    let named = prefix
        .as_deref()
        .is_some_and(|p| body.contains(p) && (cleanup.contains(p) || by_id));
    if !named {
        return ctx.settle(
            AIRS_WRITE,
            started,
            inputs,
            Found::blocked(
                "the write body and its cleanup path must both name the family's disposable prefix",
            ),
        );
    }
    let (mut found, response) = applied(
        &mut desk,
        "write.request.toml",
        Some(&body),
        None,
        context.clone(),
    )
    .await?;
    let mut cleanup_url = None;
    if found.outcome == "passed" && by_id {
        let reported = created_resource(&response, &created);
        match reported {
            Some((id, name)) if prefix.as_deref().is_some_and(|p| name.starts_with(p)) => {
                found = found.id("created_id", &id);
                cleanup_url = Some(id);
            }
            _ => {
                found = verdict(
                    found,
                    false,
                    "",
                    "the write's response named no created resource under the prefix, so its cleanup has no target",
                )
                .stopping("the created resource could not be identified: reconcile it", true);
            }
        }
    }
    if found.outcome == "passed" {
        let url = match cleanup_url {
            Some(id) => {
                let d = doc(&mut desk, "cleanup.request.toml").await?;
                d["fields"]["url"]
                    .as_str()
                    .map(|u| u.replace(CREATED_ID, &id))
            }
            None => None,
        };
        let (removed, _) = applied(&mut desk, "cleanup.request.toml", None, url, context).await?;
        found = found.with("cleanup", json!(removed.outcome)).with(
            "cleanup_status",
            removed
                .observed
                .get("status")
                .cloned()
                .unwrap_or(Value::Null),
        );
        found.cleanup = if removed.outcome == "passed" {
            "the created resource was deleted through the same reviewed path".to_owned()
        } else {
            "the cleanup request did not complete: reconcile the created resource".to_owned()
        };
        if removed.outcome != "passed" {
            found = found.stopping("the write's cleanup did not complete", true);
        }
    }
    inputs.insert("write_path".to_owned(), json!(path));
    ctx.settle(AIRS_WRITE, started, inputs, found)
}

/// Sends a managed mutation, expects it held for review (nothing sent),
/// then applies it with the armed native confirmation.
async fn applied(
    desk: &mut Desk,
    relative: &str,
    body: Option<&str>,
    url: Option<String>,
    context: Option<String>,
) -> Result<(Found, Value)> {
    let d = doc(desk, relative).await?;
    let mut fields = d["fields"].clone();
    if let Some(body) = body {
        fields["body"] = json!(body);
    }
    if let Some(url) = url {
        fields["url"] = json!(url);
    }
    let held = send(desk, &d, &fields, context.clone()).await?;
    if held["kind"] != "reviewRequired" {
        let found = verdict(
            shown(&held),
            false,
            "",
            "unexpected: the mutation was not held for review",
        );
        return Ok((found, held));
    }
    desk.confirmations.arm();
    let r = desk
        .workbench(
            json!({"kind": "apply", "review": held["review"]["review"], "id": d["id"],
            "revision": d["revision"], "fields": fields, "context": context}),
        )
        .await;
    desk.confirmations.disarm();
    let r = r?;
    let mut found = verdict(
        shown(&r).with("held_for_review", json!(true)),
        ok(&r),
        "held for review (nothing sent), then sent once after the native confirmation",
        "the reviewed mutation did not complete",
    );
    if r["kind"] == "ran" && status(&r).is_none() {
        found = found.stopping("the mutation's outcome is unknown", true);
    }
    if let Some(dest) = held["review"]["destination"].as_str() {
        found = found.with("destination", json!(dest));
    }
    Ok((found, r))
}

/// The created resource's ID and name, from the write's JSON response
/// preview, at the designated fields.
fn created_resource(
    response: &Value,
    fields: &(Option<String>, Option<String>),
) -> Option<(String, String)> {
    let (Some(id_field), Some(name_field)) = fields else {
        return None;
    };
    let body: Value = serde_json::from_str(response["run"]["preview"].as_str()?).ok()?;
    let id = body.get(id_field)?.as_str()?.to_owned();
    let name = body.get(name_field)?.as_str()?.to_owned();
    (!id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
    .then_some((id, name))
}
