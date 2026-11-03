#!/usr/bin/env python3
"""C18 native acceptance: the AI Gateway Playground (a workspace inference
key, streamed chat and Responses runs, complete, cut short or failed,
stopping, refusals, a workspace switch mid-stream), the workspace's Files
(read, upload, attach to a Responses run, delete, an uncertain upload),
the Usage, Logs and Limits tabs, the Gateway on Home/Inbox, and the
Playground's work kept on this Mac across an offline relaunch, through the
real fixture window against the synthetic Gateway (mgmt_server.py: its
telemetry, policies, files and streaming runtime endpoint).

Run from the repository root after `xtask native-prepare --chunk C18`.
Requires the Accessibility grant. Synthetic data only; the fixture data
root is removed at the end; v1 data and the login keychain are untouched.
"""

import hashlib
import json
import os
import shutil
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fixture_app as fx  # noqa: E402
import acceptance_c08 as c8  # noqa: E402
import acceptance_c16 as c16  # noqa: E402

STAGE = os.path.join(fx.DATA, "fixture-gateway-files")
UPLOAD = "notes-c18.txt"
UPLOAD_BODY = b"C18 synthetic upload: never shown, never logged.\n"

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C18")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C18" else "native-C18")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C18" else "native-acceptance-C18.json")
INFERENCE_KEY = "SYNTHETIC-C18-INFERENCE-KEY"
KEY_SHA = hashlib.sha256(INFERENCE_KEY.encode()).hexdigest()
PROMPT = "Say hello from the synthetic Gateway."
EDITED = "Edited offline on this Mac."
MODEL = "@openai-prod/gpt-4o"
LOG = []


def rel(path):
    return os.path.relpath(path, ROOT)


def save_tree(name, tree):
    path = os.path.join(NATIVE, f"tree-{name}.json")
    with open(path, "w") as f:
        json.dump(tree, f, indent=0)
    return rel(path)


def runs(mgmt):
    return [r for r in mgmt.requests() if r["path"] == "/rt/v1/chat/completions" and not r.get("closedByClient")]


def response_runs(mgmt):
    return [r for r in mgmt.requests() if r["path"] == "/rt/v1/responses" and not r.get("closedByClient")]


def file_requests(mgmt, method):
    return [r for r in mgmt.requests() if r["path"].startswith("/rt/v1/files") and r["method"] == method]


def stage_upload():
    if os.path.isdir(STAGE):
        shutil.rmtree(STAGE)
    os.makedirs(STAGE)
    with open(os.path.join(STAGE, UPLOAD), "wb") as f:
        f.write(UPLOAD_BODY)


def closed(mgmt):
    return [r for r in mgmt.requests() if r.get("closedByClient")]


def button(tree, label):
    return fx.App.node(tree, "AXButton", label)


def enabled(tree, label):
    b = button(tree, label) or {}
    return b.get("enabled") is not False and bool(b)


def reach(app, zoom, label, roles=("AXButton", "AXDisclosureTriangle")):
    """c8.reach for any of `roles` (a <summary> is an AXDisclosureTriangle)."""
    window = next(n for n in app.tree() if n.get("role") == "AXWindow")
    bottom = window["pos"][1] + window["size"][1]
    found = None
    for _ in range(110):
        app.keycode(48)
        f = app.focused(app.tree())
        if f and f.get("role") in roles and f.get("label") == label:
            found = {"role": f["role"], "label": f["label"], "pos": f["pos"], "size": f["size"]}
            break
    clear = bool(found) and found["pos"][1] >= window["pos"][1] and found["pos"][1] + found["size"][1] <= bottom - 60 * zoom
    return {"zoom": zoom, "window": {"pos": window["pos"], "size": window["size"]}, "found": found, "clear": clear}


def run_prompt(app):
    app.ax("press", "AXButton", "Run prompt")


def clear(app):
    tree = app.tree()
    if button(tree, "Clear response"):
        app.ax("press", "AXButton", "Clear response")
        app.wait_for(lambda t: button(t, "Clear response") is None, 8)


def main():
    try:
        return run()
    finally:
        fx.sh(["pkill", "-f", fx.PROCESS])
        fx.clean_fixture_root()


def run():
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")
    if c8.sha256(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c18-acceptance-")
    if os.path.isdir(NATIVE):
        shutil.rmtree(NATIVE)
    os.makedirs(NATIVE)
    app = fx.App(bundle, work, LOG)
    app.quit()
    fx.clean_fixture_root()
    v1_before = fx.v1_metadata()
    endpoint = fx.ScanEndpoint(os.path.join(work, "endpoint"), c8.SCAN_KEY)
    endpoint.install_roots()
    endpoint.start()
    mgmt = c8.MgmtEndpoint(os.path.join(work, "endpoint"), os.path.join(work, "mgmt"))
    mgmt.install()
    mgmt.start()
    mgmt.mode()
    runtime = f"{mgmt.origin}/rt/v1"
    obs = {}

    app.launch()
    app.open_settings()
    app.type_into("AXTextField", "TSG ID (optional)", c8.TSG)
    app.create_custom_tenant("Tenant A", endpoint.origin)
    app.save_key(c8.SCAN_KEY)
    c8.save_client(app)
    c16.open_gateway(app)
    c8.text(app, "2 workspaces", 15)
    app.ax("press", "AXRadioButton", "Team A")
    c8.text(app, "2 providers · complete", 15)

    # ---- N01: the inference key is stored for the chosen workspace ----
    c16.tab(app, "Playground")
    tree, offered = c8.text(app, "No inference key is stored for this tenant. The management client is never used for inference.", 12)
    run_disabled = not enabled(tree, "Run prompt")
    app.type_into("AXTextField", "Runtime endpoint", runtime)
    app.type_into("AXTextField", "Workspace API key with completions access", INFERENCE_KEY, secure=True)
    app.ax("press", "AXButton", "Store key for Team A")
    tree, stored = c8.text(app, f"A key for Team A is stored (Keychain). It is sent only to {runtime}.", 15)
    obs["key"] = {"offered": offered, "run_disabled_without_key": run_disabled, "stored": stored,
                  "shown": app.has_text(tree, INFERENCE_KEY), "runs": len(runs(mgmt))}
    obs["tree_key"] = save_tree("key-stored", tree)

    # ---- N02: one streamed run, sent once with the inference key ----
    app.type_into("AXTextField", "Model", MODEL)
    app.type_into("AXTextArea", "Prompt", PROMPT)
    tree, warned = c8.text(app, "It is sent once and never retried.", 8)
    before = len(runs(mgmt))
    nothing_before = before == 0
    run_prompt(app)
    tree, done = c8.text(app, "Completed.", 15)
    sent = runs(mgmt)[before:]
    trace = sent[0].get("trace") if sent else None
    obs["run"] = {
        "warned": warned, "nothing_before_run": nothing_before, "completed": done,
        "output": app.has_text(tree, "Hello from the synthetic Gateway."),
        "sent": len(sent), "key_sha_matches": bool(sent) and sent[0].get("inferenceKeySha256") == KEY_SHA,
        "no_authorization": bool(sent) and sent[0].get("authorizationHeader") is False,
        "model": sent[0].get("model") if sent else None, "stream": sent[0].get("stream") if sent else None,
        "max_tokens": sent[0].get("maxTokens") if sent else None,
        "trace_shown": bool(trace) and app.has_text(tree, trace),
        "tokens": app.has_text(tree, "19 (7 in, 12 out)"),
    }
    obs["tree_run"] = save_tree("run-completed", tree)

    # ---- N03: stopping a run mid-stream; it is kept as uncertain ----
    clear(app)
    mgmt.mode(chat="hang")
    before = len(runs(mgmt))
    run_prompt(app)
    tree, partial = c8.text(app, "Synthetic partial answer", 15)
    stop_offered = enabled(tree, "Stop run")
    app.ax("press", "AXButton", "Stop run")
    tree, stopped = c8.text(app, "Stopped. The Gateway may have run it and charged for it: find it in the logs by its trace.", 12)
    tree, kept = c8.text(app, "was stopped after it was sent", 12)
    time.sleep(1)
    obs["stop"] = {"partial": partial, "stop_offered": stop_offered, "stopped": stopped, "kept": kept,
                   "output_kept": app.has_text(tree, "Its output is not kept."),
                   "closed_by_client": len(closed(mgmt)) >= 1, "sent": len(runs(mgmt)) - before}
    obs["tree_stop"] = save_tree("run-stopped", tree)

    # ---- N04: a guardrail refusal is named and never retried ----
    clear(app)
    mgmt.mode(chat="status:446")
    before = len(runs(mgmt))
    run_prompt(app)
    tree, blocked = c8.text(app, "A Gateway guardrail blocked the request or its response.", 15)
    time.sleep(1.5)
    obs["blocked"] = {"named": blocked, "sent": len(runs(mgmt)) - before}
    obs["tree_blocked"] = save_tree("run-blocked", tree)
    # A Gateway failure (503) is the SDK's unknown: it may have been run.
    clear(app)
    mgmt.mode(chat="status:503")
    before = len(runs(mgmt))
    run_prompt(app)
    tree, failed = c8.text(app, "Failed. The Gateway or its provider failed. The Gateway may have run it and charged for it: find it in the logs by its trace.", 15)
    tree, kept503 = c8.text(app, "failed after it was sent: the Gateway may have run it and charged for it.", 12)
    time.sleep(1.5)
    sent503 = runs(mgmt)[before:]
    obs["blocked"].update({"unavailable_named": failed, "unavailable_kept": kept503,
                           "unavailable_trace_kept": bool(sent503) and app.has_text(tree, f"Trace {sent503[0].get('trace')}"),
                           "unavailable_sent": len(sent503)})
    obs["tree_unavailable"] = save_tree("run-unavailable", tree)

    # ---- N12: a Responses run ----
    clear(app)
    mgmt.mode()
    app.ax("press", "AXRadioButton", "Responses")
    before_chat = len(runs(mgmt))
    before = len(response_runs(mgmt))
    run_prompt(app)
    tree, done = c8.text(app, "Completed.", 15)
    sent = response_runs(mgmt)[before:]
    obs["responses"] = {
        "completed": done, "output": app.has_text(tree, "Responses from the synthetic Gateway."),
        "sent": len(sent), "chat_sent": len(runs(mgmt)) - before_chat,
        "key_sha_matches": bool(sent) and sent[0].get("inferenceKeySha256") == KEY_SHA,
        "no_authorization": bool(sent) and sent[0].get("authorizationHeader") is False,
        "store": sent[0].get("store") if sent else None, "max_output_tokens": sent[0].get("maxOutputTokens") if sent else None,
        "stream": sent[0].get("stream") if sent else None,
        "api_shown": app.has_text(tree, "Responses"), "served": app.has_text(tree, "gpt-4o-2024"),
        "tokens": app.has_text(tree, "14 (5 in, 9 out)"),
    }
    obs["tree_responses"] = save_tree("responses", tree)
    # Cut short at the output token limit: incomplete, with its reason.
    clear(app)
    mgmt.mode(responses="incomplete")
    before = len(response_runs(mgmt))
    run_prompt(app)
    tree, cut = c8.text(app, "Incomplete: the Gateway ended the answer before it was finished.", 15)
    obs["responses"].update({
        "incomplete": cut, "reason": app.has_text(tree, "It reached the output token limit (max_output_tokens)."),
        "finish_shown": app.has_text(tree, "max_output_tokens (incomplete)"),
        "partial": app.has_text(tree, "Partial from the"), "incomplete_tokens": app.has_text(tree, "14 (5 in, 9 out)"),
        "not_completed": not app.has_text(tree, "Completed."), "incomplete_sent": len(response_runs(mgmt)) - before,
    })
    obs["tree_responses_incomplete"] = save_tree("responses-incomplete", tree)
    # The model failed: failed, never incomplete or completed.
    clear(app)
    mgmt.mode(responses="failed")
    before = len(response_runs(mgmt))
    run_prompt(app)
    tree, failed = c8.text(app, "Failed. The model failed while answering.", 15)
    obs["responses"].update({"failed": failed, "failed_not_incomplete": not app.has_text(tree, "Incomplete:"),
                             "failed_sent": len(response_runs(mgmt)) - before})
    obs["tree_responses_failed"] = save_tree("responses-failed", tree)
    clear(app)
    mgmt.mode()

    # ---- N13: the workspace's files: read, upload, attach, delete ----
    stage_upload()
    c16.tab(app, "Files")
    tree, heading = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Files in Team A") is not None, 10)
    app.ax("press", "AXButton", "Read files")
    tree, listed = c8.text(app, "seed-notes.txt", 15)
    reads = file_requests(mgmt, "GET")
    files = {"heading": heading, "listed": listed, "retention": app.has_text(tree, "this Mac keeps no copy and never reads one back"),
             "read_key": bool(reads) and reads[-1].get("inferenceKeySha256") == KEY_SHA and reads[-1].get("route") is None}
    app.ax("press", "AXButton", "Choose file…")
    tree, chosen = c8.text(app, f"{UPLOAD} · {len(UPLOAD_BODY)} bytes", 15)
    files["chosen"] = chosen
    files["path_hidden"] = not app.has_text(tree, STAGE) and not app.has_text(tree, "never shown, never logged")
    app.ax("focus", "AXButton", f"Upload {UPLOAD} to Team A")
    app.keycode(49)
    tree, uploaded = c8.text(app, f"Uploaded {UPLOAD} as file-up", 15)
    posts = file_requests(mgmt, "POST")
    files.update({
        "uploaded": uploaded, "upload_sent": len(posts),
        "upload_body": bool(posts) and posts[0].get("filename") == UPLOAD and posts[0].get("purpose") == "assistants"
        and posts[0].get("bytes") == len(UPLOAD_BODY) and posts[0].get("contentSha256") == hashlib.sha256(UPLOAD_BODY).hexdigest(),
        "upload_key": bool(posts) and posts[0].get("inferenceKeySha256") == KEY_SHA,
        "upload_trace_shown": bool(posts) and bool(posts[0].get("trace")) and app.has_text(tree, f"Trace {posts[0].get('trace')}"),
    })
    # The status line takes focus (a role="status" paragraph: an AXGroup
    # whose subrole is AXApplicationStatus; only the action status is
    # focusable). Containers of the focus are skipped.
    holder = next((n for n in tree if n.get("focused")
                   and n.get("role") not in ("AXWindow", "AXWebArea", "AXScrollArea")), None) or {}
    files["focus_node"] = {k: holder.get(k) for k in ("role", "subrole", "label", "value")}
    files["status_focused"] = ("Uploaded" in f"{holder.get('label') or ''}{holder.get('value') or ''}"
                               or str(holder.get("subrole") or "").endswith("Status"))
    obs["tree_files_uploaded"] = save_tree("files-uploaded", tree)
    # Attach it to a Responses run.
    c16.tab(app, "Playground")
    app.ax("press", "AXRadioButton", "Responses")
    tree, _ = app.wait_for(lambda t: fx.App.node(t, "AXPopUpButton", "Attached file") is not None, 10)
    app.ax("focus", "AXPopUpButton", "Attached file")
    app.keys("notes")
    tree, picked = app.wait_for(lambda t: str((fx.App.node(t, "AXPopUpButton", "Attached file") or {}).get("value") or "").startswith(UPLOAD), 8)
    before = len(response_runs(mgmt))
    run_prompt(app)
    tree, done = c8.text(app, "Completed.", 15)
    attached = response_runs(mgmt)[before:]
    created = (attached[0].get("inputFiles") or [None])[0] if attached else None
    files.update({"picked": picked, "attached_run": done and len(attached) == 1,
                  "attached_id": bool(created) and str(created).startswith("file-up") and app.has_text(tree, str(created))})
    obs["tree_files_attached"] = save_tree("files-attached", tree)
    clear(app)
    app.ax("press", "AXRadioButton", "Chat completions")
    time.sleep(1.2)
    # Delete asks first; Cancel sends nothing and returns to the row.
    c16.tab(app, "Files")
    tree, _ = c8.text(app, "seed-notes.txt", 10)
    app.ax("press", "AXButton", "Delete seed-notes.txt (file-seed)")
    tree, asked = c8.text(app, "Delete seed-notes.txt (file-seed) from the Gateway?", 10)
    confirm_focus = (app.focused(tree) or {}).get("label") == "Delete file"
    app.ax("press", "AXButton", "Cancel")
    tree, _ = app.wait_for(lambda t: (app.focused(t) or {}).get("label") == "Delete seed-notes.txt (file-seed)", 8)
    cancel_focus = (app.focused(tree) or {}).get("label") == "Delete seed-notes.txt (file-seed)"
    nothing_deleted = len(file_requests(mgmt, "DELETE")) == 0
    app.ax("press", "AXButton", "Delete seed-notes.txt (file-seed)")
    c8.text(app, "from the Gateway?", 10)
    app.keycode(49)
    tree, deleted = c8.text(app, "Deleted seed-notes.txt from the Gateway.", 15)
    deletes = file_requests(mgmt, "DELETE")
    files.update({"asked": asked, "confirm_focus": confirm_focus, "cancel_focus": cancel_focus, "nothing_on_cancel": nothing_deleted,
                  "deleted": deleted, "delete_sent": len(deletes), "delete_target": bool(deletes) and deletes[0].get("file") == "file-seed"})
    obs["tree_files_deleted"] = save_tree("files-deleted", tree)
    # An upload whose answer is a 503 may have been stored: kept, not resent.
    mgmt.mode(files="status:503")
    app.ax("press", "AXButton", "Choose file…")
    c8.text(app, f"{UPLOAD} · {len(UPLOAD_BODY)} bytes", 15)
    app.ax("press", "AXButton", f"Upload {UPLOAD} to Team A")
    tree, uncertain = c8.text(app, f"The upload of {UPLOAD} may have been stored: its result is not known.", 15)
    tree, kept_file = c8.text(app, f"Upload of {UPLOAD} (", 12)
    time.sleep(1.5)
    files.update({"uncertain": uncertain, "kept": kept_file and app.has_text(tree, "was sent without a known result"),
                  "uncertain_sent": len(file_requests(mgmt, "POST")) - 1})
    obs["tree_files_uncertain"] = save_tree("files-uncertain", tree)
    # A delete answered for another file claims nothing: it is uncertain,
    # kept with its trace, the file stays listed, and it is not resent.
    mgmt.mode(files="another")
    # The uploaded file's row (its ID comes from the loopback service).
    target = next((n.get("label") for n in app.tree() if n.get("role") == "AXButton"
                   and str(n.get("label") or "").startswith(f"Delete {UPLOAD} (file-up")), f"Delete {UPLOAD} (file-up?)")
    app.ax("press", "AXButton", target)
    c8.text(app, "from the Gateway?", 10)
    app.ax("press", "AXButton", "Delete file")
    tree, mismatch = c8.text(app, f"{UPLOAD} may have been deleted: its result is not known.", 15)
    tree, mismatch_kept = c8.text(app, f"Delete of {UPLOAD}", 12)
    time.sleep(1.5)
    tree = app.tree()
    files.update({"mismatch_uncertain": mismatch and app.has_text(tree, "did not describe that file"),
                  "mismatch_kept": mismatch_kept, "mismatch_listed": fx.App.node(tree, "AXButton", target) is not None,
                  "mismatch_sent": len(file_requests(mgmt, "DELETE")) - 1})
    obs["tree_files_mismatch"] = save_tree("files-mismatch", tree)
    # A file whose time no date can render is listed as unknown time, and
    # the view stays usable.
    mgmt.mode(filesLate=True)
    app.ax("press", "AXButton", "Read files")
    tree, late = c8.text(app, "late-notes.txt", 15)
    files.update({"late_listed": late and app.has_text(tree, "Unknown time")
                  and fx.App.node(tree, "AXButton", "Delete late-notes.txt (file-late)") is not None})
    obs["tree_files_late"] = save_tree("files-late", tree)
    obs["files"] = files
    mgmt.mode()
    shutil.rmtree(STAGE, ignore_errors=True)
    c16.tab(app, "Playground")
    time.sleep(1.0)

    # ---- N05: another workspace mid-stream stops it; nothing of it stays ----
    clear(app)
    mgmt.mode(chat="paced")
    closed_before = len(closed(mgmt))
    before = len(runs(mgmt))
    run_prompt(app)
    tree, streaming = c8.text(app, "C18-STREAM-", 15)
    app.ax("press", "AXRadioButton", "Team B")
    tree, switched = c8.text(app, "Playground for Team B", 15)
    app.wait_for(lambda t: len(closed(mgmt)) > closed_before, 10)
    app.ax("press", "AXRadioButton", "Team A")
    c8.text(app, "2 providers · complete", 15)
    c16.tab(app, "Playground")
    time.sleep(1.5)
    tree = app.tree()
    obs["switch"] = {"streaming": streaming, "switched": switched,
                     "closed_by_client": len(closed(mgmt)) > closed_before,
                     "gone": not app.has_text(tree, "C18-STREAM-"), "sent": len(runs(mgmt)) - before}
    obs["tree_switch"] = save_tree("switch", tree)
    mgmt.mode()

    # ---- N06: usage with each source's own state ----
    mgmt.mode(gwTelemetry="fail")
    c16.tab(app, "Usage")
    tree, usage = c8.text(app, "Total: 120", 15)
    obs["usage"] = {
        "requests": usage,
        "chart_named": any(n.get("role") == "AXImage" and (n.get("label") or "").startswith("Requests: Total 120")
                           for n in tree),
        "cost_failed": app.has_text(tree, "Not read: The management service is unavailable"),
        "errors": app.has_text(tree, "Errors in the window: 24"),
        "models": app.has_text(tree, "model-000"),
        "reach_values": reach(app, 1, "Show requests values"),
    }
    obs["tree_usage"] = save_tree("usage", tree)
    mgmt.mode()

    # ---- N07: a run's trace finds it in the logs; limits are read ----
    c16.tab(app, "Playground")
    # The run stopped by the switch is gone; a fresh run is found by its trace.
    tree, _ = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Run prompt") is not None, 10)
    run_prompt(app)
    c8.text(app, "Completed.", 15)
    app.ax("press", "AXButton", "Find in logs")
    tree, logs = c8.text(app, "1 request shown of 1", 15)
    log_reads = [r for r in mgmt.requests() if r["path"] == "/ai_gw/v2/logs"]
    c16.tab(app, "Limits")
    tree2, limits = c8.text(app, "Monthly budget", 15)
    obs["logs"] = {"filtered": logs, "trace_query": bool(log_reads) and bool(log_reads[-1]["query"].get("traceId")),
                   "slug": bool(log_reads) and log_reads[-1]["query"].get("workspaceSlug") == "team-a",
                   "limits": limits and app.has_text(tree2, "$50.00 · alert at 80%") and app.has_text(tree2, "60 per rpm")}
    obs["tree_logs"] = save_tree("logs", tree)
    obs["tree_limits"] = save_tree("limits", tree2)

    # ---- N08: Home/Inbox shows the chosen workspace's Gateway day ----
    app.open_runtime()
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Scan a prompt or response") is not None)
    c16.tab(app, "Overview & Inbox")
    tree, day = c8.text(app, "Last day: 120 requests · 24 errors · $7.20 cost", 20)
    item = app.has_text(tree, "Gateway errors in Team A")
    obs["home"] = {"day": day, "item": item, "heading": fx.App.node(tree, "AXHeading", "AI Gateway · Team A") is not None,
                   "policies": app.has_text(tree, "2 limit policies apply to this workspace.")}
    obs["tree_home"] = save_tree("home", tree)

    # ---- N10: keyboard reach at 200% zoom and the minimum size ----
    c16.open_gateway(app)
    c16.tab(app, "Playground")
    c8.text(app, "Prompt", 10)
    for _ in range(10):
        app.keys("=", "command down")
    obs["zoom_run"] = reach(app, 2, "Run prompt")
    app.keys("0", "command down")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{900, 600}}'])
    time.sleep(0.8)
    obs["min_store"] = reach(app, 1, "Remove key")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{1280, 820}}'])

    # ---- N09: offline relaunch keeps the draft and the uncertain run ----
    time.sleep(1.5)
    sent_before = len(runs(mgmt)) + len(response_runs(mgmt)) + len(file_requests(mgmt, "POST")) + len(file_requests(mgmt, "DELETE"))
    requests_before = len(mgmt.requests())
    app.quit()
    mgmt.stop()
    app.launch()
    c16.open_gateway(app)
    tree, failed = c8.text(app, "The workspaces could not be read", 25)
    tree, kept = c8.text(app, "Kept on this Mac", 15)
    offline = {
        "listing_failed": failed, "kept": kept,
        "draft": app.has_text(tree, "Playground draft for Team A") and app.has_text(tree, PROMPT),
        "run_kept": app.has_text(tree, "was stopped after it was sent") and app.has_text(tree, "failed after it was sent"),
        "file_kept": app.has_text(tree, f"Upload of {UPLOAD} (") and app.has_text(tree, "the file may have been stored"),
        "no_response": not app.has_text(tree, "Synthetic partial answer"),
    }
    obs["tree_offline"] = save_tree("offline", tree)
    # Edit the kept draft offline, by keyboard: Edit, the prompt, Done.
    app.ax("press", "AXButton", "Edit Playground draft for Team A")
    tree, editing = app.wait_for(lambda t: fx.App.node(t, "AXTextArea", "Draft prompt") is not None, 10)
    app.type_into("AXTextArea", "Draft prompt", EDITED)
    app.ax("focus", "AXButton", "Done editing")
    app.keycode(49)
    tree, done = app.wait_for(lambda t: fx.App.node(t, "AXTextArea", "Draft prompt") is None, 10)
    focus = app.focused(app.tree()) or {}
    offline.update({"editor": editing is not None and bool(editing), "done": bool(done),
                    "focus_returned": focus.get("label") == "Edit Playground draft for Team A"})
    obs["tree_offline_edited"] = save_tree("offline-edited", app.tree())
    time.sleep(1.5)
    app.quit()
    app.launch()
    c16.open_gateway(app)
    tree, again = c8.text(app, EDITED, 25)
    offline["edit_kept_offline"] = again and not app.has_text(tree, PROMPT)
    offline["requests"] = len(mgmt.requests()) - requests_before
    app.quit()
    mgmt.start()
    app.launch()
    c16.open_gateway(app)
    c8.text(app, "2 workspaces", 25)
    app.ax("press", "AXRadioButton", "Team A")
    c8.text(app, "2 providers · complete", 15)
    c16.tab(app, "Playground")
    tree, _ = app.wait_for(lambda t: fx.App.node(t, "AXTextArea", "Prompt") is not None, 15)
    offline["opens_online"] = str((app.field("AXTextArea", "Prompt") or {}).get("value") or "") == EDITED
    time.sleep(3)
    offline["reconnect_sent"] = (len(runs(mgmt)) + len(response_runs(mgmt)) + len(file_requests(mgmt, "POST"))
                                 + len(file_requests(mgmt, "DELETE")) - sent_before)
    obs["offline"] = offline

    app.quit()
    obs["inference_requests"] = [{"trace": r.get("trace"), "status": r.get("status")} for r in runs(mgmt)]
    obs["responses_requests"] = [{"trace": r.get("trace"), "status": r.get("status")} for r in response_runs(mgmt)]
    trees = [os.path.join(NATIVE, n) for n in os.listdir(NATIVE) if n.startswith("tree-")]
    secrets = (c8.SECRET, c8.SCAN_KEY, INFERENCE_KEY)
    obs["secret_in_trees"] = [rel(p) for p in trees if any(s in open(p).read() for s in secrets)]
    log_text = json.dumps(mgmt.requests())
    obs["prompt_in_log"] = PROMPT in log_text or INFERENCE_KEY in log_text or "never shown, never logged" in log_text
    obs["upload_in_trees"] = [rel(p) for p in trees if "never shown, never logged" in open(p).read() or STAGE in open(p).read()]
    endpoint.stop()
    mgmt.stop()
    obs["v1_unchanged"] = fx.v1_metadata() == v1_before
    shutil.rmtree(work, ignore_errors=True)
    with open(os.path.join(NATIVE, "observations.json"), "w") as f:
        json.dump({"observations": obs, "log": LOG}, f, indent=1)
    return evaluate(prepared, obs)


def evaluate(prepared, o):
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C18"]}
    obs_path = rel(os.path.join(NATIVE, "observations.json"))
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [obs_path] + [e for e in evidence if e]})

    k = o["key"]
    add("C18-N01", k["offered"] and k["run_disabled_without_key"] and k["stored"] and not k["shown"] and k["runs"] == 0,
        f"key {k}", [o["tree_key"]])
    r = o["run"]
    add("C18-N02", r["warned"] and r["nothing_before_run"] and r["completed"] and r["output"] and r["sent"] == 1
        and r["key_sha_matches"] and r["no_authorization"] and r["model"] == MODEL and r["stream"] is True
        and r["max_tokens"] == 256 and r["trace_shown"] and r["tokens"],
        f"run {r}", [o["tree_run"]])
    s = o["stop"]
    add("C18-N03", s["partial"] and s["stop_offered"] and s["stopped"] and s["kept"] and s["output_kept"]
        and s["closed_by_client"] and s["sent"] == 1, f"stop {s}", [o["tree_stop"]])
    b = o["blocked"]
    add("C18-N04", b["named"] and b["sent"] == 1 and b["unavailable_named"] and b["unavailable_kept"]
        and b["unavailable_trace_kept"] and b["unavailable_sent"] == 1, f"refusals {b}",
        [o["tree_blocked"], o["tree_unavailable"]])
    w = o["switch"]
    add("C18-N05", w["streaming"] and w["switched"] and w["closed_by_client"] and w["gone"] and w["sent"] == 1,
        f"switch {w}", [o["tree_switch"]])
    u = o["usage"]
    add("C18-N06", u["requests"] and u["chart_named"] and u["cost_failed"] and u["errors"] and u["models"]
        and u["reach_values"]["clear"], f"usage {u}", [o["tree_usage"]])
    lg = o["logs"]
    add("C18-N07", lg["filtered"] and lg["trace_query"] and lg["slug"] and lg["limits"], f"logs {lg}",
        [o["tree_logs"], o["tree_limits"]])
    h = o["home"]
    add("C18-N08", h["day"] and h["item"] and h["heading"] and h["policies"], f"home {h}", [o["tree_home"]])
    off = o["offline"]
    add("C18-N09", off["listing_failed"] and off["kept"] and off["draft"] and off["run_kept"] and off["file_kept"] and off["no_response"]
        and off["editor"] and off["done"] and off["focus_returned"] and off["edit_kept_offline"]
        and off["opens_online"] and off["requests"] == 0 and off["reconnect_sent"] == 0, f"offline {off}",
        [o["tree_offline"], o["tree_offline_edited"]])
    rs = o["responses"]
    add("C18-N12", rs["completed"] and rs["output"] and rs["sent"] == 1 and rs["chat_sent"] == 0 and rs["key_sha_matches"]
        and rs["no_authorization"] and rs["store"] is False and rs["max_output_tokens"] == 256 and rs["stream"] is True
        and rs["api_shown"] and rs["served"] and rs["tokens"]
        and rs["incomplete"] and rs["reason"] and rs["finish_shown"] and rs["partial"] and rs["incomplete_tokens"]
        and rs["not_completed"] and rs["incomplete_sent"] == 1 and rs["failed"] and rs["failed_not_incomplete"]
        and rs["failed_sent"] == 1, f"responses {rs}",
        [o["tree_responses"], o["tree_responses_incomplete"], o["tree_responses_failed"]])
    fl = o["files"]
    add("C18-N13", fl["heading"] and fl["listed"] and fl["retention"] and fl["read_key"] and fl["chosen"] and fl["path_hidden"]
        and fl["uploaded"] and fl["upload_sent"] == 1 and fl["upload_body"] and fl["upload_key"] and fl["upload_trace_shown"]
        and fl["status_focused"] and fl["picked"] and fl["attached_run"] and fl["attached_id"] and fl["asked"]
        and fl["confirm_focus"] and fl["cancel_focus"] and fl["nothing_on_cancel"] and fl["deleted"] and fl["delete_sent"] == 1
        and fl["delete_target"] and fl["uncertain"] and fl["kept"] and fl["uncertain_sent"] == 1
        and fl["mismatch_uncertain"] and fl["mismatch_kept"] and fl["mismatch_listed"] and fl["mismatch_sent"] == 1
        and fl["late_listed"], f"files {fl}",
        [o["tree_files_uploaded"], o["tree_files_attached"], o["tree_files_deleted"], o["tree_files_uncertain"],
         o["tree_files_mismatch"], o["tree_files_late"]])
    add("C18-N10", o["zoom_run"]["clear"] and o["min_store"]["clear"],
        f"200% zoom: {o['zoom_run']['found']}; minimum size: {o['min_store']['found']}", [])
    add("C18-N11", not o["secret_in_trees"] and not o["prompt_in_log"] and not o["upload_in_trees"] and o["v1_unchanged"]
        and [x["status"] for x in o["inference_requests"]] == [200, 200, 446, 503, 200, 200]
        and len(o["responses_requests"]) == 4,
        f"inference requests {o['inference_requests']}; responses {len(o['responses_requests'])}; "
        f"secrets in AX snapshots: {o['secret_in_trees']}; upload content or path in AX snapshots: {o['upload_in_trees']}; "
        f"prompt or key in the server log: {o['prompt_in_log']}; v1 unchanged: {o['v1_unchanged']}", [])

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1",
        "subject": "C18",
        "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (tools/native-acceptance/ax.swift, System Events keystrokes) against tools/native-acceptance/mgmt_server.py; tools/native-acceptance/acceptance_c18.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "The AI Gateway planes and the runtime endpoint are a synthetic local TLS endpoint; no product endpoint is called before C25 and no live inference budget is used.",
            "The synthetic replies follow the SDK's Gateway contract shapes; a hung stream and a refusal are simulated by the endpoint.",
            "Spoken VoiceOver output was not recorded; AX roles, names and focus are inspected.",
        ],
    }
    with open(RECORD, "w") as f:
        json.dump(record, f, indent=2)
        f.write("\n")
    for c in checks:
        print(c["result"].upper(), c["id"], c["observation"][:220])
    return 0 if all(c["result"] == "pass" for c in checks) else 1


if __name__ == "__main__":
    sys.exit(main())
