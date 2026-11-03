#!/usr/bin/env python3
"""C13 native acceptance: Red Team & Models through the real fixture window
against the synthetic product endpoints (mgmt_server.py: AI Red Teaming
statistics, scans and attack results, Model Security scans, AgentGuard
statistics and skill scans).

Run from the repository root after `xtask native-prepare --chunk C13`.
Requires the Accessibility grant. Synthetic data only; the fixture data
root is removed at the end; v1 data and the login keychain are untouched.
"""

import json
import os
import shutil
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fixture_app as fx  # noqa: E402
import acceptance_c08 as c8  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C13")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C13" else "native-C13")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C13" else "native-acceptance-C13.json")
# Markers the synthetic services put in names, URIs, findings, prompts,
# replies and error bodies; none may ever reach the window.
PRIVATE = ("RT-PRIVATE", "MS-PRIVATE", "AG-PRIVATE", "SERVER-TEXT-NEVER-SHOWN")
# v1's preview workflows; no control may start one.
WORKFLOWS = ("New scan", "Start scan", "Re-run scan", "Verify ASR", "Yes, verify", "New target", "Create target",
             "New adapter", "Save adapter", "New network channel", "Create account", "New prompt set",
             "Create prompt set", "Scan a model", "New security group")
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


def buttons(tree, prefix):
    """Distinct button labels (WebKit can expose an element twice)."""
    return sorted({n["label"] for n in tree if n.get("role") == "AXButton" and (n.get("label") or "").startswith(prefix)})


def press(app, label, roles=("AXRadioButton", "AXButton", "AXTab")):
    tree = app.tree()
    for role in roles:
        matches = [n for n in tree if n.get("role") == role and (n.get("label") or "").startswith(label)]
        for nth, n in enumerate(matches):
            if n.get("label") == label:
                app.ax("press", role, label, nth)
                return role
    raise RuntimeError(f"no element labelled {label!r}")


def open_red_team(app):
    app.ax("press", "AXButton", "Red Team & Models")
    return app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Red Teaming and model observation") is not None)


def main():
    try:
        return run()
    finally:
        fx.stop_app()
        fx.clean_fixture_root()


def run():
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop")
    if c8.sha256(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c13-acceptance-")
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

    # ---- N08: no management client, nothing read ----
    open_red_team(app)
    tree, asked = c8.text(app, "Configure one in Settings to see them", 8)
    obs["no_client"] = {"asked": asked, "requests": len(mgmt.requests())}
    app.open_settings()
    c8.save_client(app)

    # ---- N01: Red Teaming on opening ----
    open_red_team(app)
    tree, read = c8.text(app, "Scans · 100 read · complete", 15)
    rt = [r for r in mgmt.requests() if r["path"].startswith("/ai-red-teaming/")]
    stats = [r for r in rt if r["path"].endswith("/scan-statistics")]
    lists = [r for r in rt if r["path"].endswith("/v1/scan")]
    attack_buttons = buttons(tree, "Open attack results for scan ")
    obs["red_team"] = {
        "read": read,
        "statistics": app.has_text(tree, "Statistics · Read"),
        "tiles": all(app.has_text(tree, t) for t in ("Scans", "Targets scanned", "Active scans", "Failed scans")),
        "lists": all(app.has_text(tree, t) for t in ("Scans by status", "Targets by type", "Risk profile")),
        "rows_with_attacks": len(attack_buttons),
        "window_query": [r["query"].get("date_range") for r in stats],
        "list_query": [(r["query"].get("skip"), r["query"].get("limit")) for r in lists],
        "tsg_header": [r.get("xTsgId") for r in rt],
        "leaks": leaks(app, tree),
    }
    obs["tree_red_team"] = save_tree("red-team", tree)

    # ---- N02: another window ----
    app.ax("press", "AXRadioButton", "Last 7 days")
    tree, windowed = c8.text(app, "last 7 days", 12)
    stats = [r for r in mgmt.requests() if r["path"].endswith("/scan-statistics")]
    obs["window"] = {"shown": windowed, "query": stats[-1]["query"].get("date_range") if stats else None}

    # ---- N03: attack results ----
    first = attack_buttons[0] if attack_buttons else "Open attack results for scan 00000000"
    app.ax("press", "AXButton", first)
    tree, opened = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Attack results · scan ") is not None, 8)
    focus = fx.App.focused(tree)
    heading_focus = bool(focus) and (focus.get("label") or "").startswith("Attack results · scan ")
    tree, complete = c8.text(app, "63 of 250 succeeded · read 100 per page · all attacks read", 15)
    attack_reads = [r for r in mgmt.requests() if r["path"].endswith("/list-attacks")]
    obs["attacks"] = {
        "opened": opened,
        "heading_focused": heading_focus,
        "complete": complete,
        # Informational: the Accessibility dump does not reach every row of
        # a long table; the statement and the page reads prove 250 read.
        "rows_in_snapshot": len({t for t in fx.App.texts(tree) if t.startswith("#") and t[1:].isdigit()}),
        "first_row": app.has_text(tree, "#001"),
        "skips": [r["query"].get("skip") for r in attack_reads],
        "prompts_kept_out": app.has_text(tree, "Attack prompts and the target's replies stay in the Red Teaming report."),
        "leaks": leaks(app, tree),
    }
    obs["tree_attacks"] = save_tree("attacks", tree)
    app.ax("press", "AXButton", "Close attack results")

    # ---- N06: no control starts a workflow (Red Teaming) ----
    tree = app.tree()
    obs["workflows_red_team"] = {
        "disclosed": app.has_text(tree, "Starting or re-running Red Teaming scans, verifying ASR"),
        "controls": [b for w in WORKFLOWS for b in buttons(tree, w)],
    }

    # ---- N04: Models & skills ----
    press(app, "Models & skills")
    tree, models = c8.text(app, "Model scans · Partial sample · 100 of 140", 15)
    product = [r for r in mgmt.requests() if r["path"].startswith(("/aims/", "/aiag/"))]
    obs["models"] = {
        "read": models,
        "skills": app.has_text(tree, "Skill scans · 100 read · complete"),
        "statistics": app.has_text(tree, "Skill statistics · Read · last 30 days"),
        "pseudonyms": len({t for t in fx.App.texts(tree) if t.endswith("(pseudonym for this read)")}),
        "background": sorted({r["query"].get("isBackgroundRefresh") for r in product if r["path"].endswith("/v1/scans")}),
        "period": [r["query"].get("time_period") for r in product if r["path"].endswith("/stats/scans")],
        "agentguard_tsg": sorted({r.get("xTsgId") for r in product if r["path"].startswith("/aiag/")}),
        "model_tsg": sorted({str(r.get("xTsgId")) for r in product if r["path"].startswith("/aims/")}),
        "leaks": leaks(app, tree),
    }
    obs["tree_models"] = save_tree("models", tree)
    obs["workflows_models"] = {
        "disclosed": app.has_text(tree, "Starting model scans and managing security groups or model sources are not available here"),
        "controls": [b for w in WORKFLOWS for b in buttons(tree, w)],
    }

    # ---- N05: failed sources are stated, never zero ----
    mgmt.mode(modelsStatus=403)
    app.ax("press", "AXButton", "Refresh model observation")
    tree, denied = c8.text(app, "Model scans · Unavailable:", 12)
    one = {"denied": denied, "others": app.has_text(tree, "Skill scans · 100 read · complete"),
           "tile": app.has_text(tree, "Unavailable")}
    mgmt.mode(modelsStatus=503, skillStatsStatus=503, skillsStatus=503)
    app.ax("press", "AXButton", "Refresh model observation")
    tree, failed = c8.text(app, "The last read failed", 12)
    kept = {"failed": failed, "kept": app.has_text(tree, "Showing what was read at")}
    obs["tree_failed"] = save_tree("models-failed", tree)
    mgmt.mode(rtStatsStatus=404)
    press(app, "Red Teaming")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Refresh Red Teaming") is not None)
    app.ax("press", "AXButton", "Refresh Red Teaming")
    tree, absent = c8.text(app, "Statistics · Unavailable: The service did not find this resource.", 12)
    obs["failures"] = {"one_source": one, "all_sources": kept, "statistics_absent_with_scans": absent,
                       "leaks": leaks(app, tree)}

    # ---- N07: keyboard reach at 200% zoom and the minimum size (ten scans,
    # so the tab order is short enough to walk) ----
    mgmt.mode(rtScans=10)
    app.ax("press", "AXButton", "Refresh Red Teaming")
    c8.text(app, "Scans · 10 read · complete", 12)
    for _ in range(10):
        app.keys("=", "command down")
    obs["zoom_refresh"] = c8.reach(app, 2, "Refresh Red Teaming")
    app.keys("0", "command down")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{900, 600}}'])
    time.sleep(0.8)
    obs["min_attacks"] = c8.reach(app, 1, first)
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{1280, 820}}'])

    app.quit()
    requests = mgmt.requests()
    obs["writes"] = [{k: r.get(k) for k in ("method", "path")} for r in requests
                     if r["method"] != "GET" and r["path"] != "/oauth2/access_token"]
    trees = [os.path.join(NATIVE, n) for n in os.listdir(NATIVE) if n.startswith("tree-")]
    obs["secret_in_trees"] = [rel(p) for p in trees if any(s in open(p).read() for s in (c8.SECRET, c8.SCAN_KEY) + PRIVATE)]
    endpoint.stop()
    mgmt.stop()
    obs["v1_unchanged"] = fx.v1_metadata() == v1_before
    shutil.rmtree(work, ignore_errors=True)
    with open(os.path.join(NATIVE, "observations.json"), "w") as f:
        json.dump({"observations": obs, "log": LOG}, f, indent=1)
    return evaluate(prepared, obs)


def evaluate(prepared, o):
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C13"]}
    obs_path = rel(os.path.join(NATIVE, "observations.json"))
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [obs_path] + [e for e in evidence if e]})

    rt = o["red_team"]
    add("C13-N01", rt["read"] and rt["statistics"] and rt["tiles"] and rt["lists"] and rt["rows_with_attacks"] == 95
        and rt["window_query"][:1] == ["LAST_30_DAYS"] and ("0", "100") in rt["list_query"]
        and all(h is None for h in rt["tsg_header"]) and not rt["leaks"],
        f"Red Teaming {rt}", [o["tree_red_team"]])
    w = o["window"]
    add("C13-N02", w["shown"] and w["query"] == "LAST_7_DAYS", f"window {w}", [])
    at = o["attacks"]
    add("C13-N03", at["opened"] and at["heading_focused"] and at["complete"] and at["first_row"]
        and at["skips"] == ["0", "100", "200"] and at["prompts_kept_out"] and not at["leaks"],
        f"attack results {at}", [o["tree_attacks"]])
    m = o["models"]
    add("C13-N04", m["read"] and m["skills"] and m["statistics"] and m["pseudonyms"] == 100 and m["background"] == ["false"]
        and m["period"] == ["30_DAYS"] and m["agentguard_tsg"] == [c8.TSG] and m["model_tsg"] == ["None"] and not m["leaks"],
        f"models and skills {m}", [o["tree_models"]])
    fl = o["failures"]
    add("C13-N05", fl["one_source"]["denied"] and fl["one_source"]["others"] and fl["all_sources"]["failed"]
        and fl["all_sources"]["kept"] and fl["statistics_absent_with_scans"] and not fl["leaks"],
        f"failures {fl}", [o["tree_failed"]])
    wr, wm = o["workflows_red_team"], o["workflows_models"]
    add("C13-N06", wr["disclosed"] and wm["disclosed"] and not wr["controls"] and not wm["controls"] and not o["writes"],
        f"Red Teaming {wr}; models {wm}; non-read requests: {o['writes']}", [])
    reach_ok = o["zoom_refresh"]["clear"] and o["min_attacks"]["clear"]
    add("C13-N07", reach_ok, f"200% zoom: {o['zoom_refresh']['found']}; minimum size: {o['min_attacks']['found']}", [])
    nc = o["no_client"]
    add("C13-N08", nc["asked"] and nc["requests"] == 0 and not o["secret_in_trees"] and o["v1_unchanged"],
        f"without a management client: {nc}; secrets or private markers in AX snapshots: {o['secret_in_trees']}; v1 unchanged: {o['v1_unchanged']}", [])

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1",
        "subject": "C13",
        "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (tools/native-acceptance/ax.swift, System Events keystrokes) against tools/native-acceptance/mgmt_server.py; tools/native-acceptance/acceptance_c13.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "The AI Red Teaming, Model Security and AgentGuard services are synthetic local TLS endpoints at the production paths; no product endpoint is called before C25.",
            "The local status filter is covered by the production App tests; the driver checks the window, attack paging, source states and privacy natively.",
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
