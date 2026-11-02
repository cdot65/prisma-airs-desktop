#!/usr/bin/env python3
"""C32 native acceptance: the Harness in the v2 shell, driven through the
real fixture window with the real pinned engine against the synthetic
Gateway service that plays the model.

Run from the repository root after `xtask native-prepare --chunk C32`.
Requires the macOS Accessibility grant. Synthetic services only; the fixture
data root and the project folder are removed at the end.

Checks (docs/architecture/native-checks.json, C32): the screen and a fresh
tenant's first send starting a user conversation (N01); a decision in the
page with the tenant-switch guard through the switcher, the palette and the
Go menu, then decline and approve (N02); a proposal followed up only as the
operator's review in Administration, ending the conversation, and the next
send starting a new one (N03); keyboard reach and the 900x600 / 200% layout
in both themes (N04).
"""

import json
import os
import secrets
import shutil
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fixture_app as fx  # noqa: E402
import acceptance_c08 as c8  # noqa: E402
import acceptance_c09 as c9  # noqa: E402
import acceptance_c16 as c16  # noqa: E402
import acceptance_c19 as c19  # noqa: E402
import acceptance_c28 as c28  # noqa: E402
import acceptance_c29 as c29  # noqa: E402
import acceptance_c31 as c31  # noqa: E402  (focus ring and status-region helpers)
import c32_coverage
import v2_setup as v2s
import acceptance_c38 as c38
import native_sheets as sheets  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C32")
EVIDENCE = os.path.join(ROOT, os.environ["AIRS_C32_EVIDENCE"])
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C32" else "native-C32")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C32" else "native-acceptance-C32.json")
INFERENCE_KEY = "SYNTHETIC-C32-HARNESS-KEY"
MARKERS = ["Profile 0", "WROTE", "Operation not permitted", "fork", "Failed to write file", "unavailable", "unsupported"]
TOPIC = "11111111-1111-4111-8111-111111111111"
FIRST = "Which profiles block prompt injection?"
LOG = []
GUARD = "A Harness decision is waiting"


def rel(path):
    return os.path.relpath(path, ROOT)


def dialog(tree, label):
    return next((n for n in tree if (n.get("label") or n.get("title") or "") == label and (n.get("subrole") in ("AXDialog", "AXApplicationDialog") or n.get("role") in ("AXDialog", "AXSheet", "AXGroup"))), None)


def ask(mgmt, name, arguments, namespace="airs_desktop"):
    """The next turn's model asks for this one tool call."""
    tool = {"name": name, "arguments": json.dumps(arguments)}
    if namespace:
        tool["namespace"] = namespace
    mgmt.mode(harnessTool=tool, toolMarkers=MARKERS)


def plain(mgmt):
    """The next turns answer with text only."""
    mgmt.mode(harnessTool=None, toolMarkers=MARKERS)


def send(app, text):
    app.type_into("AXTextArea", "Message", text)
    c29.press(app, "AXButton", "Send")


def answers(app, tree=None):
    return sum(c19.ANSWER in x for x in app.texts(tree if tree is not None else app.tree()))


def main():
    try:
        return run()
    finally:
        sheets.disable()
        fx.sh(["pkill", "-f", fx.PROCESS])
        fx.sh(["pkill", "-9", "-f", c19.ENGINE])
        fx.clean_fixture_root()


def run():
    global NATIVE
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")
    if c8.sha256(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c32-acceptance-")
    base = os.path.join(ROOT, "target", f"native-c32-{secrets.token_hex(6)}")
    project = os.path.join(base, "agent-demo")
    os.makedirs(project)
    for n in range(3):
        with open(os.path.join(project, f"file-{n}.txt"), "w") as f:
            f.write(f"content {n}\n")
    FINAL = NATIVE
    NATIVE = NATIVE + ".staging"
    if os.path.isdir(NATIVE):
        shutil.rmtree(NATIVE)
    os.makedirs(NATIVE)
    c29.NATIVE = NATIVE
    c19.NATIVE = NATIVE
    if os.path.isdir(c29.CAPTURES):
        shutil.rmtree(c29.CAPTURES)
    app = fx.App(bundle, work, LOG)
    c29._APP = app
    app.quit()
    fx.sh(["pkill", "-9", "-f", c19.ENGINE])
    fx.clean_fixture_root()
    endpoint = fx.ScanEndpoint(os.path.join(work, "endpoint"), c8.SCAN_KEY)
    endpoint.install_roots()
    endpoint.start()
    mgmt = c8.MgmtEndpoint(os.path.join(work, "endpoint"), os.path.join(work, "mgmt"))
    mgmt.install()
    mgmt.start()
    mgmt.mode()
    try:
        return exercise(app, endpoint, mgmt, prepared, project, FINAL)
    finally:
        endpoint.stop()
        mgmt.stop()
        shutil.rmtree(work, ignore_errors=True)
        shutil.rmtree(base, ignore_errors=True)


def exercise(app, endpoint, mgmt, prepared, project, FINAL):
    global NATIVE
    runtime = f"{mgmt.origin}/rt/v1"
    obs = {"captures": {}}
    sheets.enable()
    app.launch()
    pid = app.pid
    v2s.tenant_on_home(app, endpoint.origin, extra=("Tenant B",))
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Home") is not None and c29.nav(t, "Destinations") is not None, 15)
    app.menu("Go", "AI Gateway")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", c16.HEADING) is not None, 12)
    c8.text(app, "2 workspaces", 15)
    app.ax("press", "AXRadioButton", "Team A")
    c8.text(app, "Team A", 10)
    # On the one presentation (C44) the workspace's inference key is saved in the Gateway card's Workspaces tab (the
    # legacy Playground form is gone): the Workspaces tab, the endpoint, the key and Save workspace key.
    ws_tab = next((n for n in app.tree() if n.get("role") in ("AXRadioButton", "AXTab") and (n.get("label") or "").strip() == "Workspaces"), None)
    if ws_tab is None:
        raise RuntimeError(f"no Workspaces tab in the Gateway card [tree: {app.dump_tree('gateway-no-workspaces-tab')}]")
    app.ax("press", ws_tab["role"], "Workspaces")
    tree, shown = app.wait_for(lambda t: fx.App.node(t, "AXTextField", "Inference endpoint") is not None, 12)
    if not shown:
        raise RuntimeError(f"the workspace key form did not show for Team A [tree: {app.dump_tree('gateway-workspace-key')}]")
    app.type_into("AXTextField", "Inference endpoint", runtime)
    app.type_into("AXTextField", "Workspace inference key", INFERENCE_KEY, secure=True)
    app.ax("press", "AXButton", "Save workspace key")
    c8.text(app, "Workspace key saved in the Keychain.", 12)

    # ---- N01: the screen; a fresh tenant's first send starts a user conversation ----
    n1 = {}
    app.menu("Go", "Harness")
    tree, n1["screen"] = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Harness") is not None
                                      and fx.App.node(t, "AXTextArea", "Message") is not None, 15)
    n1["status_line"] = app.has_text(tree, "Northwind Health · Engine") and app.has_text(tree, "Team A")
    n1["no_checklist"] = not app.has_text(tree, "Before you start")
    n1["no_seeded"] = not any(n.get("role") == "AXButton" and (n.get("label") or "").endswith("Ended") for n in tree) and not app.has_text(tree, "claims-triage")
    n1["rail"] = any(n.get("role") == "AXNavigation" or (n.get("subrole") == "AXLandmarkNavigation" and (n.get("label") or "") == "Conversations") for n in tree) or app.has_text(tree, "New investigation")
    obs["tree_empty"] = c29.save_tree("harness-empty", tree)
    obs["captures"]["harness-empty"] = c29.matrix(app, "harness-empty")
    runs_before = len(c19.harness_runs(mgmt))
    plain(mgmt)
    send(app, FIRST)
    tree, n1["answered"] = app.wait_for(lambda t: answers(app, t) >= 1, 60)
    n1["model_turns"] = len(c19.harness_runs(mgmt)) - runs_before
    n1["titled"] = sum(FIRST in x for x in app.texts(tree)) >= 2  # the rail title and the transcript entry
    n1["engine"] = bool(c19.engines())
    obs["tree_first"] = c29.save_tree("harness-first-send", tree)
    obs["captures"]["harness-first"] = c29.matrix(app, "harness-first")
    obs["n1"] = n1

    # ---- N02: a decision in the page; the guard through every path; decline and approve ----
    n2 = {}
    c29.press(app, "AXButton", "Project, sign-in and workspace tools")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Choose a folder…") is not None, 10)
    app.ax("press", "AXButton", "Choose a folder…")
    sheets.open_file(app, project)
    tree, n2["project"] = app.wait_for(lambda t: app.has_text(t, project), 15)
    # A project applies to the next conversation: choosing it ends a running one (C21); the
    # next send starts a new one.
    time.sleep(1)
    if fx.App.node(app.tree(), "AXButton", "End conversation") is not None:
        c29.press(app, "AXButton", "End conversation")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "End conversation") is None, 30)
    ask(mgmt, "exec_command", {"cmd": "ls", "yield_time_ms": 1000, "login": False}, namespace=None)
    send(app, "List the files.")
    tree, n2["asked"] = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Approve once") is not None
                                     and fx.App.node(t, "AXButton", "Decline") is not None, 60)
    n2["names_call"] = app.has_text(tree, "exec_command")
    obs["tree_decision"] = c29.save_tree("harness-decision", tree)
    obs["captures"]["harness-decision"] = c29.matrix(app, "harness-decision")

    def guard_shown(timeout=10):
        return app.wait_for(lambda t: dialog(t, GUARD) is not None, timeout)

    c29.open_switcher(app)
    c29.press(app, "AXCheckBox", "Tenant B")
    tree, g1 = guard_shown()
    n2["guard_switcher"] = g1 and app.has_text(tree, "Switching to Tenant B ends it")
    n2["guard_buttons"] = [l for l in ("Stay", "Discard and switch", "Archive and switch") if fx.App.node(tree, "AXButton", l)]
    obs["tree_guard"] = c29.save_tree("guard-switcher", tree)
    # One topmost modal (I8): the rail and the page are hidden beneath it, focus starts on Stay.
    n2["guard_isolated"] = c29.switcher(tree) == {} and c38.landmark(tree, "AXLandmarkNavigation", "Conversations") is None and fx.App.node(tree, "AXButton", "Approve once") is None
    n2["guard_initial_focus"] = ((fx.App.focused(tree) or {}).get("label") or "")
    # Escape is Stay; focus returns to the switcher that asked.
    app.keycode(53)
    tree, _ = app.wait_for(lambda t: dialog(t, GUARD) is None, 8)
    time.sleep(0.5)
    focused = fx.App.focused(app.tree()) or {}
    n2["guard_focus_returned"] = focused.get("role") in ("AXPopUpButton", "AXButton") and "Northwind Health" in (focused.get("label") or "")
    n2["guard_page_back"] = fx.App.node(app.tree(), "AXButton", "Approve once") is not None
    app.keys("k", "command down")
    app.wait_for(lambda t: any(n.get("role") == "AXComboBox" for n in t), 8)
    app.keys("Switch to Tenant B")
    time.sleep(0.5)
    app.keycode(36)
    _, n2["guard_palette"] = guard_shown()
    c29.press(app, "AXButton", "Stay")
    app.wait_for(lambda t: dialog(t, GUARD) is None, 8)
    app.menu("Go", "Switch Tenant…")
    app.wait_for(lambda t: any(n.get("role") == "AXGroup" and (n.get("label") or "") == "Switch tenant" for n in t), 8)
    time.sleep(0.3)
    c29.press(app, "AXCheckBox", "Tenant B")
    _, n2["guard_menu"] = guard_shown()
    c29.press(app, "AXButton", "Stay")
    tree, _ = app.wait_for(lambda t: dialog(t, GUARD) is None, 8)
    n2["still_tenant_a"] = "Northwind Health" in str(c29.switcher(app.tree()).get("label") or "")
    n2["decision_kept"] = fx.App.node(app.tree(), "AXButton", "Approve once") is not None
    # Decline: recorded, nothing run.
    c29.press(app, "AXButton", "Decline")
    tree, n2["declined"] = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Approve once") is None and app.has_text(t, "Declined"), 40)
    # Approve once: the command runs and the turn completes.
    turns = len(c19.harness_runs(mgmt))
    ask(mgmt, "exec_command", {"cmd": "ls", "yield_time_ms": 1000, "login": False}, namespace=None)
    send(app, "List them again.")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Approve once") is not None, 60)
    c29.press(app, "AXButton", "Approve once")
    tree, n2["approved"] = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Approve once") is None
                                        and len(c19.harness_runs(mgmt)) >= turns + 2, 60)
    obs["tree_approved"] = c29.save_tree("harness-approved", tree)
    # With changes allowed, a command may change the folder: in-page consent alone never runs it.
    c29.press(app, "AXButton", "End conversation")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "End conversation") is None, 30)
    if fx.App.node(app.tree(), "AXCheckBox", "Allow changes in this folder") is None:
        c29.press(app, "AXButton", "Project, sign-in and workspace tools")
    c29.press(app, "AXCheckBox", "Allow changes in this folder")
    marker = os.path.join(project, "c32-change.txt")
    ask(mgmt, "exec_command", {"cmd": "echo c32 > c32-change.txt", "yield_time_ms": 1000, "login": False}, namespace=None)
    send(app, "Write a file.")
    tree, asked = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Approve…") is not None, 60)
    n2["may_change_shown"] = asked and app.has_text(tree, "May change things")
    obs["tree_may_change"] = c29.save_tree("harness-may-change", tree)
    c29.press(app, "AXButton", "Approve…")
    tree, n2["native_sheet"] = c9.wait_sheet(app)
    c9.press_exact(app, "Cancel")
    c9.wait_no_sheet(app)
    time.sleep(1.5)
    n2["refused_kept_pending"] = fx.App.node(app.tree(), "AXButton", "Approve…") is not None and not os.path.exists(marker)
    c29.press(app, "AXButton", "Approve…")
    c9.wait_sheet(app)
    c9.press_exact(app, "Allow once")
    c9.wait_no_sheet(app)
    _, n2["ran_after_native"] = app.wait_for(lambda t: os.path.exists(marker) and fx.App.node(t, "AXButton", "Approve…") is None, 60)
    obs["n2"] = n2

    # ---- N03: a desktop read decided in the page; a follow-up opens only the operator's review; ending; anew ----
    n3 = {}
    ask(mgmt, "mcpServers", {})
    send(app, "Which MCP servers does this workspace have?")
    tree, n3["read_asked"] = app.wait_for(lambda t: app.has_text(t, "mcpServers") and fx.App.node(t, "AXButton", "Approve once") is not None, 60)
    n3["read_only_shown"] = app.has_text(tree, "Read-only")
    obs["tree_read"] = c29.save_tree("harness-desktop-read", tree)
    reads_before = len([r for r in mgmt.requests() if "mcp-servers" in r.get("path", "")])
    c29.press(app, "AXButton", "Approve once")
    _, n3["read_done"] = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Approve once") is None
                                      and len([r for r in mgmt.requests() if "mcp-servers" in r.get("path", "")]) > reads_before, 60)
    n3["read_count"] = len([r for r in mgmt.requests() if "mcp-servers" in r.get("path", "")]) - reads_before
    n3["read_done"] = n3["read_done"] and n3["read_count"] == 1
    since = len(c9.writes(mgmt, prefix="/aisec/"))
    ask(mgmt, "proposeChange", {"kind": "topics", "action": "update", "target": TOPIC,
                                "body": {"topic_name": "Finance", "description": "Money talk, proposed in C32", "examples": [], "revision": 1},
                                "summary": "Clarify the Finance topic"})
    send(app, "Propose a clearer Finance topic.")
    tree, n3["staged"] = app.wait_for(lambda t: app.has_text(t, "Clarify the Finance topic")
                                      and fx.App.node(t, "AXButton", "Open the review") is not None, 60)
    n3["writes_after_staging"] = len(c9.writes(mgmt, prefix="/aisec/")) - since
    obs["tree_follow_up"] = c29.save_tree("harness-follow-up", tree)
    obs["captures"]["harness-follow-up"] = c29.matrix(app, "harness-follow-up")
    c29.press(app, "AXButton", "Open the review")
    tree, n3["review"] = app.wait_for(lambda t: dialog(t, "Review a change") is not None and app.has_text(t, "Type Finance to confirm"), 20)
    n3["proposed_title"] = app.has_text(tree, "Proposed by the Harness: update custom topic")
    apply = fx.App.node(tree, "AXButton", "Apply…")
    n3["apply_gated"] = apply is None or apply.get("enabled") is False
    n3["writes_after_review"] = len(c9.writes(mgmt, prefix="/aisec/")) - since
    obs["tree_review"] = c29.save_tree("harness-review", tree)
    app.keycode(53)
    app.wait_for(lambda t: dialog(t, "Review a change") is None, 10)
    app.menu("Go", "Harness")
    tree, n3["closed"] = app.wait_for(lambda t: app.has_text(t, "Review closed"), 15)
    c29.press(app, "AXButton", "End conversation")
    tree, n3["ended"] = app.wait_for(lambda t: app.has_text(t, "Transcript saved on this Mac") and fx.App.node(t, "AXButton", "End conversation") is None, 30)
    n3["rail_ended"] = app.has_text(tree, "Ended")
    n3["status_regions"] = c31.status_texts(tree)
    n3["ended_announced"] = any("ended" in r.lower() for r in n3["status_regions"])
    n3["engine_gone"] = c19.wait(lambda: not c19.engines(), 20)
    obs["tree_ended"] = c29.save_tree("harness-ended", tree)
    obs["captures"]["harness-ended"] = c29.matrix(app, "harness-ended")
    before = answers(app)
    plain(mgmt)
    send(app, "And yesterday?")
    tree, n3["new_conversation"] = app.wait_for(lambda t: app.has_text(t, "And yesterday?") and answers(app, t) >= 1 and fx.App.node(t, "AXButton", "End conversation") is not None, 60)
    obs["n3"] = n3

    # ---- N04: keyboard reach; 900x600 and 200% in both themes ----
    n4 = {}
    app.ax("focus", "AXTextArea", "Message")
    walk = []
    for _ in range(6):
        app.keycode(48)
        f = fx.App.focused(app.tree()) or {}
        walk.append(f.get("label") or "")
    n4["walk"] = walk
    n4["send_reached"] = any(l in ("Send", "End conversation") for l in walk)
    # Focus ring on a changed control: New investigation focused vs the message box focused.
    app.ax("focus", "AXTextArea", "Message")
    time.sleep(0.5)
    ring_plain = c29.capture(app, "harness-ring-plain")
    n4["ring"] = c31.ring_on(app, "New investigation", ring_plain, "harness-ring-new")
    base = c28.window_size(pid)
    c38.size_window(app, pid, 900, 600)
    c29.zoom(app, 10)
    zoom = {}
    for theme in ("Light", "Dark"):
        c29.appearance(app, theme)
        tree = app.tree()
        win = next((n for n in tree if n.get("role") == "AXWindow"), None)
        nodes = [n for n in tree if n.get("pos") and n.get("size") and n["size"][0] > 0
                 and n.get("role") in ("AXButton", "AXStaticText", "AXTextArea", "AXTextField", "AXCheckBox", "AXPopUpButton", "AXLink", "AXHeading")]
        right = max(n["pos"][0] + n["size"][0] for n in nodes) if nodes and win else None
        hbars = [n for n in tree if n.get("role") == "AXScrollBar" and (n.get("orientation") == "horizontal" or (n.get("size") and n["size"][0] > n["size"][1]))]
        zoom[theme.lower()] = {"overflow_px": max(0, right - (win["pos"][0] + win["size"][0])) if right is not None else None,
                               "horizontal_scrollbars": len(hbars),
                               "capture": c29.capture(app, f"harness-min-200-{theme.lower()}"),
                               "tree": c29.save_tree(f"harness-min-200-{theme.lower()}", tree)}
    c29.zoom(app, -10)
    c29.appearance(app, "Match System")
    if base:
        c28.set_window_size(pid, base[0], base[1])
    n4["zoom"] = zoom
    obs["captures"]["harness-min-200"] = {k: v["capture"] for k, v in zoom.items()}
    obs["captures"]["harness-ring"] = {"plain": n4["ring"].get("plain"), "focused": n4["ring"].get("capture")}
    # Reduce Motion: the ended conversation's toast, bound to this tenant, stays past 6 s until dismissed.
    had_key = fx.sh(["defaults", "read", "com.apple.universalaccess", "reduceMotion"]).returncode == 0
    prior = fx.sh(["defaults", "read", "com.apple.universalaccess", "reduceMotion"]).stdout.strip()
    rm = {}
    try:
        fx.sh(["defaults", "write", "com.apple.universalaccess", "reduceMotion", "-bool", "true"])
        time.sleep(1)
        rm["active"] = c28.reduce_motion()
        app.quit()
        fx.sh(["pkill", "-9", "-f", c19.ENGINE])
        app.launch()
        c29._APP = app
        app.wait_for(lambda t: c29.nav(t, "Destinations") is not None, 15)
        # A relaunch restores the workspace once AI Gateway's workspaces are read again.
        app.menu("Go", "AI Gateway")
        app.wait_for(lambda t: fx.App.node(t, "AXRadioButton", "Team A") is not None, 20)
        app.ax("press", "AXRadioButton", "Team A")
        c8.text(app, "Team A", 10)
        app.menu("Go", "Harness")
        _, rm["harness_ready"] = app.wait_for(lambda t: fx.App.node(t, "AXTextArea", "Message") is not None and not app.has_text(t, "Before you start"), 20)
        plain(mgmt)
        send(app, "One more question.")
        _, rm["answered"] = app.wait_for(lambda t: fx.App.node(t, "AXButton", "End conversation") is not None and answers(app, t) >= 1, 90)
        if fx.App.node(app.tree(), "AXButton", "End conversation") is not None:
            c29.press(app, "AXButton", "End conversation")
        tree, rm["toast"] = app.wait_for(lambda t: app.has_text(t, "Conversation ended."), 30)
        time.sleep(8)
        tree = app.tree()
        rm["persists"] = app.has_text(tree, "Conversation ended.")
        rm["tree"] = c29.save_tree("harness-reduce-motion-toast", tree)
        rm["capture"] = c29.capture(app, "harness-reduce-motion-toast")
        if fx.App.node(app.tree(), "AXButton", "Dismiss") is not None:
            c29.press(app, "AXButton", "Dismiss")
        time.sleep(0.6)
        rm["dismissed"] = rm.get("toast", False) and not app.has_text(app.tree(), "Conversation ended.")
    finally:
        if had_key:
            fx.sh(["defaults", "write", "com.apple.universalaccess", "reduceMotion", "-bool", "true" if prior == "1" else "false"])
        else:
            fx.sh(["defaults", "delete", "com.apple.universalaccess", "reduceMotion"])
        time.sleep(1)
    rm["restored"] = c28.reduce_motion() == (had_key and prior == "1")
    n4["reduce_motion"] = rm
    obs["captures"]["harness-reduce-motion"] = {"toast": rm.get("capture")}
    obs["n4"] = n4
    app.quit()

    run_path = os.path.join(NATIVE, "observations.json")
    with open(run_path, "w") as f:
        json.dump(json.loads(json.dumps({"observations": obs, "log": LOG}).replace(rel(NATIVE), rel(FINAL))), f, indent=1)
    run_rel = rel(run_path)
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C32"]}
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [e for e in evidence if e]})

    caps = lambda name: list(obs["captures"].get(name, {}).values())  # noqa: E731
    add("C32-N01", n1["screen"] and n1["status_line"] and n1["no_checklist"] and n1["no_seeded"] and n1["answered"] and n1["titled"] and n1["model_turns"] >= 1,
        f"Harness screen {n1['screen']} with its status line {n1['status_line']}; checklist hidden once satisfied {n1['no_checklist']}; no seeded conversation {n1['no_seeded']}; the first send started a conversation and was answered {n1['answered']} ({n1['model_turns']} model turns), titled from the message in the rail {n1['titled']}",
        [run_rel, obs["tree_empty"], obs["tree_first"]] + caps("harness-empty") + caps("harness-first"))
    add("C32-N02", n2["project"] and n2["asked"] and n2["names_call"] and n2["guard_switcher"] and set(n2["guard_buttons"]) == {"Stay", "Discard and switch", "Archive and switch"}
        and n2["guard_isolated"] and n2["guard_initial_focus"] == "Stay" and n2["guard_focus_returned"] and n2["guard_page_back"]
        and n2["guard_palette"] and n2["guard_menu"] and n2["still_tenant_a"] and n2["decision_kept"] and n2["declined"] and n2["approved"]
        and n2["may_change_shown"] and n2["native_sheet"] and n2["refused_kept_pending"] and n2["ran_after_native"],
        f"project chosen through the embedded panel {n2['project']}; a read-only command asked in the page {n2['asked']} naming the call {n2['names_call']}; guard via switcher {n2['guard_switcher']} with {n2['guard_buttons']}, the one topmost modal (rail and page hidden) {n2['guard_isolated']}, initial focus '{n2['guard_initial_focus']}', Escape returned focus to the switcher {n2['guard_focus_returned']} with the page back {n2['guard_page_back']}; via palette {n2['guard_palette']}, via Go › Switch Tenant… {n2['guard_menu']}; Stay kept Northwind Health {n2['still_tenant_a']} and the decision {n2['decision_kept']}; Decline recorded {n2['declined']}; Approve once ran it {n2['approved']}; with changes allowed the command was shown as May change things {n2['may_change_shown']}, Approve… opened the native sheet {n2['native_sheet']}, Cancel kept it pending and ran nothing {n2['refused_kept_pending']}, Allow once ran it {n2['ran_after_native']}",
        [run_rel, obs["tree_decision"], obs["tree_guard"], obs["tree_approved"], obs["tree_may_change"]] + caps("harness-decision"))
    add("C32-N03", n3["read_asked"] and n3["read_only_shown"] and n3["read_done"] and n3["staged"] and n3["writes_after_staging"] == 0 and n3["review"] and n3["proposed_title"]
        and n3["apply_gated"] and n3["writes_after_review"] == 0 and n3["closed"] and n3["ended"] and n3["rail_ended"] and n3["ended_announced"] and n3["engine_gone"] and n3["new_conversation"],
        f"the workspace MCP catalog read asked in the page as read-only {n3['read_asked'] and n3['read_only_shown']} and read once approved {n3['read_done']}; Apply gated by the typed name {n3['apply_gated']}; follow-up staged {n3['staged']} with {n3['writes_after_staging']} writes; Open the review showed the operator's typed review in Administration {n3['review']} titled for the proposal {n3['proposed_title']} ({n3['writes_after_review']} writes); leaving it closed the follow-up {n3['closed']}; End conversation sealed it {n3['ended']}, announced in a status region {n3['ended_announced']}, marked ended in the rail {n3['rail_ended']}, engine gone {n3['engine_gone']}; the next send started a new conversation {n3['new_conversation']}",
        [run_rel, obs["tree_read"], obs["tree_follow_up"], obs["tree_review"], obs["tree_ended"]] + caps("harness-follow-up") + caps("harness-ended"))
    zoom_ok = all(v["overflow_px"] == 0 and v["horizontal_scrollbars"] == 0 for v in n4["zoom"].values())
    rmo = n4["reduce_motion"]
    ring_ok = n4["ring"].get("diff", 0) > 150 and n4["ring"].get("focused") == "New investigation"
    rm_ok = rmo.get("active") and rmo.get("harness_ready") and rmo.get("answered") and rmo.get("toast") and rmo.get("persists") and rmo.get("dismissed") and rmo.get("restored")
    add("C32-N04", n4["send_reached"] and zoom_ok and ring_ok and rm_ok,
        f"Tab from the message reached {n4['walk']} (Send or End {n4['send_reached']}); at 900×600 and 200% overflow light {n4['zoom']['light']['overflow_px']} px, dark {n4['zoom']['dark']['overflow_px']} px, horizontal scroll bars {[v['horizontal_scrollbars'] for v in n4['zoom'].values()]}; focus ring on New investigation {n4['ring'].get('diff')} px changed ({ring_ok}); Reduce Motion active {rmo.get('active')}: the ended-conversation toast shown {rmo.get('toast')}, still present after 8 s {rmo.get('persists')}, dismissed {rmo.get('dismissed')}, setting restored {rmo.get('restored')}",
        [run_rel, rmo.get("tree")] + [v["tree"] for v in n4["zoom"].values()] + caps("harness-min-200") + caps("harness-ring") + caps("harness-reduce-motion"))

    coverage_errors = c32_coverage.problems(obs)
    for check in checks:
        if coverage_errors and check["id"] in ("C32-N02", "C32-N03", "C32-N04"):
            check["result"] = "fail"
            check["observation"] += "; mandatory coverage errors: " + str(coverage_errors)

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1", "subject": "C32", "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "method": "cua",
        "performed_by": "Codex gpt-6.1-sol orchestrating the real fixture window through macOS Accessibility (System Events and the ax helper) with fixture WebView self-capture; tools/native-acceptance/acceptance_c32_current.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "Pixels come from the fixture build's own WKWebView snapshot; the native file sheet is driven through the fixture sheet adapter.",
            "The model is the synthetic Gateway service (tools/native-acceptance/mgmt_server.py); the real pinned engine runs against it. The lifecycle fixtures (blocks, mcp, keys) are exercised in the component suite; natively the same decision, follow-up and end paths run with a command approval and a proposal the engine really raises.",
            "Spoken VoiceOver output was not recorded.",
        ],
    }
    screens = os.path.join(EVIDENCE, "screens")
    if os.path.isdir(screens):
        shutil.rmtree(screens)
    os.makedirs(screens, exist_ok=True)
    for name, variants in obs["captures"].items():
        for variant, src in variants.items():
            if src:
                shutil.copy(os.path.join(ROOT, src), os.path.join(screens, f"{name}-{variant}.png"))
    record["screens"] = sorted(os.listdir(screens))
    if os.path.isdir(FINAL):
        shutil.rmtree(FINAL)
    os.rename(NATIVE, FINAL)
    staged_rel, final_rel = rel(NATIVE), rel(FINAL)
    for c in checks:
        c["evidence"] = [e.replace(staged_rel, final_rel) for e in c["evidence"]]
    NATIVE = FINAL
    with open(RECORD, "w") as f:
        json.dump(record, f, indent=2)
        f.write("\n")
    for c in checks:
        print(c["result"].upper(), c["id"], c["observation"][:220])
    return 0 if all(c["result"] == "pass" for c in checks) else 1


if __name__ == "__main__":
    sys.exit(main())
