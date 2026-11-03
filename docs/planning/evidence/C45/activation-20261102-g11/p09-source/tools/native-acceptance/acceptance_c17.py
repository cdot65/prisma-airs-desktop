#!/usr/bin/env python3
"""C17 native acceptance: reviewed AI Gateway changes and workspace
provisioning through the real fixture window against the synthetic Gateway
planes (mgmt_server.py: its Gateway write overlay), with the native
confirmation and secret sheets.

Run from the repository root after `xtask native-prepare --chunk C17`.
Requires the Accessibility grant. Synthetic data only; the fixture data
root is removed at the end; the clipboard is restored; v1 data and the
login keychain are untouched.
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
import acceptance_c09 as c9  # noqa: E402
import acceptance_c16 as c16  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C17")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C17" else "native-C17")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C17" else "native-acceptance-C17.json")
GW_A = c16.GW_A
PROVIDER_1 = f"7e3a0000-0000-4000-8000-{GW_A[-4:]}00000001"
PROVIDER_2 = f"7e3a0000-0000-4000-8000-{GW_A[-4:]}00000002"
SCOPE_LINE = "Applies to workspace Team A (team-a), IAM scope scope-team-a."
SHEET_SCOPE = "Workspace: Team A (team-a), IAM scope scope-team-a."
PRIVATE = c16.PRIVATE + ("GW-ONE-TIME-",)
LOG = []


def rel(path):
    return os.path.relpath(path, ROOT)


def save_tree(name, tree):
    path = os.path.join(NATIVE, f"tree-{name}.json")
    with open(path, "w") as f:
        json.dump(tree, f, indent=0)
    return rel(path)


def leaks(app, tree):
    return sorted({p for p in PRIVATE if app.has_text(tree, p)})


def gateway_writes(mgmt):
    return [r for r in mgmt.requests()
            if r["method"] != "GET" and r["path"].startswith(("/ai_gw/", "/iam/v1/"))]


def lines(requests):
    return [f"{r['method']} {r['path']}" for r in requests]


def sheet_text(app, tree):
    return " ".join(str(n.get("label") or "") + " " + str(n.get("value") or "") for n in c9.sheet(app, tree) or [])


def open_item(app, details, action):
    app.ax("press", "AXButton", details)
    app.wait_for(lambda t: fx.App.node(t, "AXButton", action) is not None, 12)
    app.ax("press", "AXButton", action)
    return app.wait_for(lambda t: fx.App.node(t, "AXButton", "Review change") is not None, 10)


def heading_focused(app, title):
    f = fx.App.focused(app.tree())
    return bool(f) and f.get("role") == "AXHeading" and f.get("label") == title


def confirm_and_apply(app, typed):
    app.type_into("AXTextField", f"Type {typed} to confirm", typed)
    app.ax("press", "AXButton", "Apply…")
    return c9.wait_sheet(app, 12)


def provision(app, name, scope):
    c9.close_workbench(app)
    app.ax("press", "AXButton", "Provision workspace…")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Review change") is not None, 10)
    focused = heading_focused(app, "Provision a workspace")
    app.type_into("AXTextArea", "workspace draft (JSON)",
                  json.dumps({"name": name, "scope_name": scope, "existing_scope": False, "description": ""}))
    tree, reviewed = c9.review(app, "Provisions a workspace in three separate requests")
    tree, _ = confirm_and_apply(app, "CREATE")
    return focused, reviewed, sheet_text(app, tree)


def main():
    saved_clipboard = c9.clipboard()
    try:
        return run()
    finally:
        c9.set_clipboard(saved_clipboard)
        fx.sh(["pkill", "-f", fx.PROCESS])
        fx.clean_fixture_root()


def run():
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")
    if c8.sha256(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c17-acceptance-")
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

    # ---- N01: a reviewed edit inside the chosen workspace, applied once ----
    open_item(app, "Details for team-a-provider-1", "Edit team-a-provider-1")
    focused = heading_focused(app, "Edit provider team-a-provider-1")
    app.type_into("AXTextArea", "provider draft (JSON)", '{"name":"team-a-primary"}')
    tree, reviewed = c9.review(app, SCOPE_LINE)
    review_leaks = leaks(app, tree)
    before_writes = len(gateway_writes(mgmt))
    tree, _ = confirm_and_apply(app, "team-a-provider-1")
    sheet = sheet_text(app, tree)
    obs["tree_edit_sheet"] = save_tree("edit-sheet", tree)
    app.keycode(36)  # Return: Apply (non-destructive default)
    tree, applied = c8.text(app, "Applied. The item was read back.", 15)
    puts = [r for r in gateway_writes(mgmt) if r["method"] == "PUT"]
    obs["edit"] = {
        "heading_focused": focused,
        "reviewed": reviewed,
        "nothing_before_confirm": before_writes == 0,
        "sheet_scope": SHEET_SCOPE in sheet,
        "sheet_title": "Update Gateway provider" in sheet,
        "applied": applied and app.has_text(tree, "team-a-primary"),
        "puts": [{"path": r["path"], "tsg": r.get("xTsgId"), "fields": r.get("fields"), "auth": r.get("auth")} for r in puts],
        "leaks": sorted(set(review_leaks + leaks(app, tree))),
    }
    obs["tree_edit"] = save_tree("edit-applied", tree)

    # ---- N02: choosing another workspace ends a prepared change ----
    c9.close_workbench(app)
    c8.text(app, "team-a-primary", 12)
    open_item(app, "Details for team-a-primary", "Edit team-a-primary")
    app.type_into("AXTextArea", "provider draft (JSON)", '{"note":"prepared in Team A"}')
    tree, ready = c9.review(app, SCOPE_LINE)
    app.ax("press", "AXRadioButton", "Team B")
    tree, switched = c8.text(app, "1 of 5+ providers read", 12)
    closed = fx.App.node(tree, "AXButton", "Review again") is None and fx.App.node(tree, "AXButton", "Apply…") is None
    app.ax("press", "AXRadioButton", "Team A")
    tree, _ = c8.text(app, "2 providers · complete", 12)
    obs["switch"] = {"ready": ready, "switched": switched, "closed": closed,
                     "still_closed": fx.App.node(tree, "AXButton", "Apply…") is None,
                     "writes": len(gateway_writes(mgmt))}
    obs["tree_switch"] = save_tree("switch", tree)

    # ---- N03: a delete blocked by a reference; an unreferenced one applies ----
    open_item(app, "Details for team-a-primary", "Delete team-a-primary")
    tree, listed = c9.review(app, "This cannot be applied while other configuration references it")
    apply_button = fx.App.node(tree, "AXButton", "Apply…") or {}
    blocked = {"listed": listed, "route": app.has_text(tree, "Route: team-a-route"),
               "apply_disabled": apply_button.get("enabled") is False}
    obs["tree_blocked"] = save_tree("delete-blocked", tree)
    c9.close_workbench(app)
    open_item(app, "Details for team-a-provider-2", "Delete team-a-provider-2")
    c9.review(app, "Deletes this item from the workspace")
    tree, _ = confirm_and_apply(app, "team-a-provider-2")
    delete_sheet = sheet_text(app, tree)
    app.keycode(36)  # Return cancels a destructive change
    c9.wait_no_sheet(app)
    time.sleep(1)
    cancelled = not [r for r in gateway_writes(mgmt) if r["method"] == "DELETE"]
    app.ax("press", "AXButton", "Apply…")
    c9.wait_sheet(app)
    c9.press_exact(app, "Delete")
    tree, deleted = c8.text(app, "Applied. The item is no longer in the inventory.", 15)
    blocked.update({"delete_sheet": "Delete Gateway provider" in delete_sheet and SHEET_SCOPE in delete_sheet,
                    "return_cancelled": cancelled, "deleted": deleted,
                    "deletes": lines([r for r in gateway_writes(mgmt) if r["method"] == "DELETE"])})
    obs["delete"] = blocked
    obs["tree_deleted"] = save_tree("deleted", tree)

    # ---- N04: a new service key, held once and shown in the native sheet ----
    c9.close_workbench(app)
    c16.tab(app, "Service keys")
    c8.text(app, "2 service keys · complete", 12)
    app.ax("press", "AXButton", "New service key…")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Review change") is not None, 10)
    app.type_into("AXTextArea", "service key draft (JSON)",
                  '{"name":"native-key","description":"","scopes":["completions.write"],"type":"service"}')
    c9.review(app, SCOPE_LINE)
    tree, _ = confirm_and_apply(app, "CREATE")
    app.keycode(36)
    tree, held = c8.text(app, "A new Gateway key is held in memory only", 15)
    held_leaks = leaks(app, tree)
    obs["tree_key_held"] = save_tree("key-held", tree)
    posts = [r for r in gateway_writes(mgmt) if r["path"] == "/ai_gw/v2/api-keys/service"]
    issued = [p.get("issuedSha256") for p in posts]
    app.ax("press", "AXButton", "Show new key…")
    tree, _ = c9.wait_sheet(app)
    secret = next((v for n in c9.sheet(app, tree) or [] for v in (n.get("label"), n.get("value"))
                   if isinstance(v, str) and v.startswith("GW-ONE-TIME-")), None)
    c9.press_exact(app, "Copy and Close")
    c9.wait_no_sheet(app)
    tree, once = c8.text(app, "The new key was shown once and is no longer held by the desktop.", 10)
    c9.close_workbench(app)
    time.sleep(0.6)
    back = fx.App.focused(app.tree())
    obs["key"] = {
        "held": held,
        "body": posts[0].get("gatewayBody") if posts else None,
        "revealed_matches_issued": bool(secret) and hashlib.sha256(secret.encode()).hexdigest() in issued,
        "shown_once": once and fx.App.node(tree, "AXButton", "Show new key…") is None,
        "focus_returned": bool(back) and back.get("label") == "New service key…",
        "leaks": held_leaks,
    }

    # ---- N05: provisioning, each step and its readback ----
    focused, reviewed, sheet = provision(app, "Team C", "scope-team-c")
    app.keycode(36)
    tree, done = c8.text(app, "Provisioned. The workspace, its IAM scope and the scope’s binding to it were read back.", 20)
    tree, relisted = c8.text(app, "3 workspaces", 15)
    obs["provision"] = {
        "heading_focused": focused,
        "reviewed": reviewed,
        "sheet_scope": "IAM scope: scope-team-c." in sheet,
        "done": done,
        "steps": app.has_text(tree, "IAM scope scope-team-c"),
        "relisted": relisted and app.has_text(tree, "Team C"),
        "writes": lines(gateway_writes(mgmt))[-3:],
    }
    obs["tree_provision"] = save_tree("provisioned", tree)

    # ---- N06: a lost workspace reply: unknown, never retried or undone ----
    mgmt.mode(gwLose="POST /ai_gw/admin/v2/workspaces")
    writes_before = len(gateway_writes(mgmt))
    provision(app, "Team D", "scope-team-d")
    app.keycode(36)
    tree, unknown = c8.text(app, "Not every step’s outcome is known. The IAM scope was kept because the workspace exists or may exist.", 25)
    mgmt.mode()
    after = gateway_writes(mgmt)[writes_before:]
    obs["lost"] = {
        "unknown": unknown,
        "workspace_unknown": app.has_text(tree, "outcome unknown"),
        "found_by_readback": app.has_text(tree, "workspace found (team-d)"),
        "writes": lines(after),
        "no_cleanup": not any(r["method"] == "DELETE" for r in after),
    }
    obs["tree_lost"] = save_tree("lost-reply", tree)

    # ---- N07: stopping a hung provisioning claims no rollback ----
    mgmt.mode(gwHang="POST /ai_gw/admin/v2/workspaces")
    writes_before = len(gateway_writes(mgmt))
    provision(app, "Team E", "scope-team-e")
    app.keycode(36)
    tree, waiting = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Stop waiting") is not None, 15)
    told = app.has_text(tree, "Stopping does not undo anything already sent")
    time.sleep(1.5)
    app.ax("press", "AXButton", "Stop waiting")
    tree, stopped = c8.text(app, "The desktop stopped waiting for this change.", 25)
    mgmt.mode()
    after = gateway_writes(mgmt)[writes_before:]
    obs["stop"] = {
        "offered": waiting and told,
        "stopped": stopped,
        "nothing_undone": app.has_text(tree, "Whether the IAM scope this attempt created was deleted is not known")
        and not app.has_text(tree, "Nothing was undone"),
        "scope_reported": app.has_text(tree, "Scope found"),
        "writes": lines(after),
        "no_cleanup": not any(r["method"] == "DELETE" for r in after),
    }
    obs["tree_stop"] = save_tree("stopped", tree)

    # ---- N08: the chosen workspace's own settings and limits ----
    c9.close_workbench(app)
    c16.tab(app, "Workspace")
    tree, shown = c8.text(app, "Settings of Team A as read from the service.", 12)
    reads = [r for r in mgmt.requests() if r["method"] == "GET" and r["path"] == f"/ai_gw/v2/workspaces/{GW_A}"]
    obs["workspace"] = {
        "shown": shown,
        "limits": app.has_text(tree, "rpm") and app.has_text(tree, "credit_limit"),
        "defaults": app.has_text(tree, "allow_config_override"),
        "actions": fx.App.node(tree, "AXButton", "Edit workspace…") is not None
        and fx.App.node(tree, "AXButton", "Archive workspace…") is not None,
        "read": len(reads),
        "leaks": leaks(app, tree),
    }
    obs["tree_workspace"] = save_tree("workspace", tree)

    # ---- N09: keyboard reach at 200% zoom and the minimum size ----
    for _ in range(10):
        app.keys("=", "command down")
    obs["zoom_provision"] = c8.reach(app, 2, "Provision workspace…")
    app.keys("0", "command down")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{900, 600}}'])
    time.sleep(0.8)
    obs["min_edit_workspace"] = c8.reach(app, 1, "Edit workspace…")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{1280, 820}}'])

    # ---- N11: drafts and uncertain results kept on this Mac, offline ----
    c16.tab(app, "Providers")
    c8.text(app, "team-a-primary", 12)
    open_item(app, "Details for team-a-primary", "Edit team-a-primary")
    app.type_into("AXTextArea", "provider draft (JSON)", '{"note":"kept on this Mac"}')
    tree, kept_before = c8.text(app, f"Draft: Update provider {PROVIDER_1} in Team A", 12)
    c9.close_workbench(app)
    records_before = app.has_text(tree, "Its outcome was not known when it settled")
    writes_before = len(gateway_writes(mgmt))
    requests_before = len(mgmt.requests())
    app.quit()
    mgmt.stop()
    app.launch()
    c16.open_gateway(app)
    tree, offline_listed = c8.text(app, "The workspaces could not be read", 25)
    tree, kept = c8.text(app, "Kept on this Mac", 15)
    draft_label = f"Open draft: Update provider {PROVIDER_1} in Team A"
    offline = {
        "kept_before_quit": kept_before and records_before,
        "listing_failed": offline_listed,
        "kept": kept,
        "draft_listed": fx.App.node(tree, "AXButton", draft_label) is not None,
        "records": sum(1 for n in tree if n.get("role") == "AXButton" and (n.get("label") or "").startswith("Open to retry: Provision workspace Team ")),
        "stale_said": app.has_text(tree, "The service has not been read since"),
        "nothing_reviewed": fx.App.node(tree, "AXButton", "Apply…") is None,
    }
    obs["tree_offline_kept"] = save_tree("offline-kept", tree)
    app.ax("press", "AXButton", draft_label)
    tree, _ = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Review change") is not None, 10)
    review_button = fx.App.node(tree, "AXButton", "Review change") or {}
    editor = fx.App.node(tree, "AXTextArea", "provider draft (JSON)") or {}
    offline.update({
        "heading_focused": heading_focused(app, f"Edit provider {PROVIDER_1}"),
        "text_restored": "kept on this Mac" in str(editor.get("value") or ""),
        "review_blocked": app.has_text(tree, "This draft is for workspace Team A. Choose that workspace above to review it.")
        and review_button.get("enabled") is False,
    })
    obs["tree_offline_draft"] = save_tree("offline-draft", tree)
    app.type_into("AXTextArea", "provider draft (JSON)", '{"note":"edited offline"}')
    time.sleep(2)
    c9.close_workbench(app)
    back = fx.App.focused(app.tree())
    offline["focus_returned"] = bool(back) and back.get("label") == draft_label
    offline["requests"] = len(mgmt.requests()) - requests_before
    obs["offline"] = offline

    # ---- N12: reconnecting sends nothing; kept work stays until dismissed ----
    app.quit()
    mgmt.start()
    app.launch()
    c16.open_gateway(app)
    # The service lists its workspaces again (its own state restarted with it).
    tree, relisted = app.wait_for(lambda t: any(app.has_text(t, f"{n} workspaces") for n in (2, 3, 4, 5)), 25)
    time.sleep(3)
    tree = app.tree()
    reconnect = {
        "listed": relisted,
        "writes_after_reconnect": lines(gateway_writes(mgmt)[writes_before:]),
        "draft_listed": fx.App.node(tree, "AXButton", draft_label) is not None,
        "records": sum(1 for n in tree if n.get("role") == "AXButton" and (n.get("label") or "").startswith("Open to retry: Provision workspace Team ")),
        "reach_open_draft": c8.reach(app, 1, draft_label),
    }
    app.ax("press", "AXButton", draft_label)
    tree, _ = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Review change") is not None, 10)
    editor = fx.App.node(tree, "AXTextArea", "provider draft (JSON)") or {}
    reconnect["edit_kept"] = "edited offline" in str(editor.get("value") or "")
    c9.close_workbench(app)
    app.ax("press", "AXButton", "Dismiss record: Provision workspace Team D")
    tree, _ = app.wait_for(
        lambda t: sum(1 for n in t if n.get("role") == "AXButton" and (n.get("label") or "").startswith("Open to retry: Provision workspace Team ")) == reconnect["records"] - 1,
        10)
    reconnect["dismissed_one"] = sum(1 for n in tree if n.get("role") == "AXButton"
                                     and (n.get("label") or "").startswith("Open to retry: Provision workspace Team ")) == reconnect["records"] - 1
    reconnect["team_d_gone"] = fx.App.node(tree, "AXButton", "Dismiss record: Provision workspace Team D") is None
    reconnect["writes_total"] = lines(gateway_writes(mgmt)[writes_before:])
    obs["reconnect"] = reconnect
    obs["tree_reconnect"] = save_tree("reconnect", tree)

    app.quit()
    obs["gateway_writes"] = lines(gateway_writes(mgmt))
    trees = [os.path.join(NATIVE, n) for n in os.listdir(NATIVE) if n.startswith("tree-")]
    secrets = (c8.SECRET, c8.SCAN_KEY) + PRIVATE + ((secret,) if secret else ())
    obs["secret_in_trees"] = [rel(p) for p in trees if any(s in open(p).read() for s in secrets)]
    endpoint.stop()
    mgmt.stop()
    obs["v1_unchanged"] = fx.v1_metadata() == v1_before
    shutil.rmtree(work, ignore_errors=True)
    with open(os.path.join(NATIVE, "observations.json"), "w") as f:
        json.dump({"observations": obs, "log": LOG}, f, indent=1)
    return evaluate(prepared, obs)


EXPECTED_WRITES = [
    f"PUT /ai_gw/v2/providers/{PROVIDER_1}",
    f"DELETE /ai_gw/v2/providers/{PROVIDER_2}",
    "POST /ai_gw/v2/api-keys/service",
    "POST /iam/v1/scopes", "POST /ai_gw/admin/v2/workspaces", "PUT /iam/v1/scopes/scope-team-c",
    "POST /iam/v1/scopes", "POST /ai_gw/admin/v2/workspaces",
    "POST /iam/v1/scopes", "POST /ai_gw/admin/v2/workspaces",
]


def evaluate(prepared, o):
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C17"]}
    obs_path = rel(os.path.join(NATIVE, "observations.json"))
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [obs_path] + [e for e in evidence if e]})

    e = o["edit"]
    add("C17-N01", e["heading_focused"] and e["reviewed"] and e["nothing_before_confirm"] and e["sheet_scope"]
        and e["sheet_title"] and e["applied"] and not e["leaks"]
        and [(p["path"], p["tsg"], p["fields"], p["auth"]) for p in e["puts"]]
        == [(f"/ai_gw/v2/providers/{PROVIDER_1}", c8.TSG, ["name"], "bearer-issued")],
        f"edit {e}", [o["tree_edit_sheet"], o["tree_edit"]])
    s = o["switch"]
    add("C17-N02", s["ready"] and s["switched"] and s["closed"] and s["still_closed"] and s["writes"] == 1,
        f"switch {s}", [o["tree_switch"]])
    d = o["delete"]
    add("C17-N03", d["listed"] and d["route"] and d["apply_disabled"] and d["delete_sheet"] and d["return_cancelled"]
        and d["deleted"] and d["deletes"] == [f"DELETE /ai_gw/v2/providers/{PROVIDER_2}"],
        f"delete {d}", [o["tree_blocked"], o["tree_deleted"]])
    k = o["key"]
    body = k["body"] or {}
    add("C17-N04", k["held"] and k["revealed_matches_issued"] and k["shown_once"] and k["focus_returned"]
        and body.get("workspace_id") == GW_A and body.get("organisation_id") == c8.TSG and not k["leaks"],
        f"key {k}", [o["tree_key_held"]])
    p = o["provision"]
    add("C17-N05", p["heading_focused"] and p["reviewed"] and p["sheet_scope"] and p["done"] and p["steps"]
        and p["relisted"] and p["writes"] == ["POST /iam/v1/scopes", "POST /ai_gw/admin/v2/workspaces",
                                              "PUT /iam/v1/scopes/scope-team-c"],
        f"provision {p}", [o["tree_provision"]])
    lo = o["lost"]
    add("C17-N06", lo["unknown"] and lo["workspace_unknown"] and lo["found_by_readback"] and lo["no_cleanup"]
        and lo["writes"] == ["POST /iam/v1/scopes", "POST /ai_gw/admin/v2/workspaces"],
        f"lost reply {lo}", [o["tree_lost"]])
    st = o["stop"]
    add("C17-N07", st["offered"] and st["stopped"] and st["nothing_undone"] and st["scope_reported"] and st["no_cleanup"]
        and st["writes"] == ["POST /iam/v1/scopes", "POST /ai_gw/admin/v2/workspaces"],
        f"stop {st}", [o["tree_stop"]])
    w = o["workspace"]
    add("C17-N08", w["shown"] and w["limits"] and w["defaults"] and w["actions"] and w["read"] >= 1 and not w["leaks"],
        f"workspace {w}", [o["tree_workspace"]])
    off = o["offline"]
    add("C17-N11", off["kept_before_quit"] and off["listing_failed"] and off["kept"] and off["draft_listed"]
        and off["records"] == 2 and off["stale_said"] and off["nothing_reviewed"] and off["heading_focused"]
        and off["text_restored"] and off["review_blocked"] and off["focus_returned"] and off["requests"] == 0,
        f"offline {off}", [o["tree_offline_kept"], o["tree_offline_draft"]])
    rc = o["reconnect"]
    add("C17-N12", rc["listed"] and rc["writes_after_reconnect"] == [] and rc["draft_listed"] and rc["records"] == 2
        and rc["edit_kept"] and rc["dismissed_one"] and rc["team_d_gone"] and rc["writes_total"] == [] and rc["reach_open_draft"]["clear"],
        f"reconnect {rc}", [o["tree_reconnect"]])
    reach_ok = o["zoom_provision"]["clear"] and o["min_edit_workspace"]["clear"]
    add("C17-N09", reach_ok, f"200% zoom: {o['zoom_provision']['found']}; minimum size: {o['min_edit_workspace']['found']}", [])
    add("C17-N10", o["gateway_writes"] == EXPECTED_WRITES and not o["secret_in_trees"] and o["v1_unchanged"],
        f"Gateway and IAM writes {o['gateway_writes']}; secrets or private markers in AX snapshots: "
        f"{o['secret_in_trees']}; v1 unchanged: {o['v1_unchanged']}", [])

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1",
        "subject": "C17",
        "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (tools/native-acceptance/ax.swift, System Events keystrokes) against tools/native-acceptance/mgmt_server.py; tools/native-acceptance/acceptance_c17.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "The AI Gateway data, admin and IAM planes are a synthetic local TLS endpoint at the production paths; no product endpoint is called before C25.",
            "The synthetic replies follow the SDK's Gateway contract shapes; a lost reply and a hung step are simulated by the endpoint. Live Gateway qualification (C25) is not claimed.",
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
