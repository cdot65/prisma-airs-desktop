#!/usr/bin/env python3
"""C30 native acceptance: the Inbox, reads, the tenant-switch guard and Home
attention in the v2 shell, driven through the real fixture window.

Run from the repository root after `xtask native-prepare --chunk C30`.
Requires the macOS Accessibility grant. Synthetic services only (the scan
endpoint, the synthetic management and Gateway service that also plays the
Harness model); the real pinned engine runs against it. The fixture data root
and the project folder are removed at the end.

Checks (docs/architecture/native-checks.json, C30): the guard through the
switcher, the palette and the Go menu while a Harness approval is pending
(N01); a never-read tenant's pending state, one read, the Inbox and its
contextual action (N02); snooze parity across Home, the Inbox row, the detail
pane and the toast, acknowledge and unsnooze, with the capture matrix (N03);
the accessibility additions on the changed UI (DESIGN-V2 §8, T09): keyboard
reach of every Inbox control, the guard as the one topmost modal with focus
return, a toast persisting under Reduce Motion, and the Inbox usable at the
900×600 minimum and 200% zoom in both themes (N04).
"""

import glob
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
import acceptance_c28 as c28  # noqa: E402  (window size, Reduce Motion)
import acceptance_c29 as c29  # noqa: E402  (shell helpers: captures, matrix, switcher)
import native_sheets as sheets  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C30")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C30" else "native-C30")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C30" else "native-acceptance-C30.json")
INFERENCE_KEY = "SYNTHETIC-C30-HARNESS-KEY"
MARKERS = ["Profile 0", "WROTE", "Operation not permitted", "fork", "Failed to write file", "unavailable", "unsupported"]
LOG = []
GUARD = "A Harness decision is waiting"


def rel(path):
    return os.path.relpath(path, ROOT)


def dialog(tree, label):
    return next((n for n in tree if (n.get("label") or "") == label and n.get("subrole") in ("AXDialog", "AXApplicationDialog")), None)


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
    work = tempfile.mkdtemp(prefix="airs-v2-c30-acceptance-")
    base = os.path.join(ROOT, "target", f"native-c30-{secrets.token_hex(6)}")
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
    # Two tenants (A fully configured for the Harness, B never read), then v2.
    app.open_settings()
    app.type_into("AXTextField", "TSG ID (optional)", c8.TSG)
    app.create_custom_tenant("Tenant A", endpoint.origin)
    app.save_key(c8.SCAN_KEY)
    c8.save_client(app)
    app.type_into("AXTextField", "TSG ID (optional)", c8.TSG)
    app.create_custom_tenant("Tenant B", endpoint.origin)
    app.make_active("Tenant A")
    c29.press(app, "AXCheckBox", "Use the new interface")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Home") is not None and c29.nav(t, "Destinations") is not None, 15)
    # Gateway workspace and inference key for A, through the same destinations the legacy shell renders.
    app.menu("Go", "AI Gateway")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", c16.HEADING) is not None, 12)
    c8.text(app, "2 workspaces", 15)
    app.ax("press", "AXRadioButton", "Team A")
    c8.text(app, "Team A", 10)
    c16.tab(app, "Playground")
    app.type_into("AXTextField", "Runtime endpoint", runtime)
    app.type_into("AXTextField", "Workspace API key with completions access", INFERENCE_KEY, secure=True)
    app.ax("press", "AXButton", "Store key for Team A")
    c8.text(app, "stored", 12)
    # The Harness with a project folder: a command needs the operator's approval.
    app.menu("Go", "Harness")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Harness") is not None, 10)
    app.ax("press", "AXButton", "Choose a folder…")
    sheets.open_file(app, project)
    app.wait_for(lambda t: app.has_text(t, project), 10)
    tree, ready = c19.start(app)
    obs["harness_ready"] = ready
    mgmt.mode(harnessTool={"name": "exec_command", "arguments": json.dumps({"cmd": "ls", "yield_time_ms": 1000, "login": False})}, toolMarkers=MARKERS)
    c19.say(app, "List the files.")
    tree, pending = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Decline") is not None, 40)
    obs["approval_pending"] = pending
    obs["tree_approval"] = c29.save_tree("harness-approval", tree)

    # ---- N01: the guard through the switcher, the palette and the Go menu ----
    def guard_shown(timeout=10):
        t, ok = app.wait_for(lambda t: dialog(t, GUARD) is not None, timeout)
        return t, ok

    t_sw = c29.open_switcher(app)
    c29.press(app, "AXCheckBox", "Tenant B")
    tree, g1 = guard_shown()
    obs["guard_switcher"] = {"shown": g1, "body_names_target": app.has_text(tree, "Switching to Tenant B ends it"),
                             "buttons": [l for l in ("Stay", "Discard and switch", "Archive and switch") if fx.App.node(tree, "AXButton", l)]}
    obs["tree_guard"] = c29.save_tree("guard-switcher", tree)
    # One topmost modal (I8): the rail (its switcher) and the page (the Harness
    # heading, the Decline button) are hidden beneath the guard; the dialog is
    # modal and holds the initial focus on Stay, the safe default (Enter on
    # arrival never ends the conversation).
    gd = dialog(tree, GUARD) or {}
    obs["guard_switcher"]["siblings_hidden"] = c29.switcher(tree) == {} and fx.App.node(tree, "AXHeading", "Harness") is None and fx.App.node(tree, "AXButton", "Decline") is None
    obs["guard_switcher"]["modal"] = bool(gd.get("modal")) or gd.get("subrole") in ("AXDialog", "AXApplicationDialog")
    obs["guard_switcher"]["initial_focus"] = ((fx.App.focused(tree) or {}).get("label") or "")
    obs["guard_switcher"]["initial_focus_stay"] = obs["guard_switcher"]["initial_focus"] == "Stay"
    obs["captures"]["guard"] = c29.matrix(app, "guard")
    # Escape is the same Stay; the switcher that asked gets focus back.
    app.keycode(53)
    tree, closed = app.wait_for(lambda t: dialog(t, GUARD) is None, 8)
    time.sleep(0.5)
    tree = app.tree()
    obs["guard_switcher"]["stay_closed"] = closed
    obs["guard_switcher"]["focus_after"] = ((fx.App.focused(tree) or {}).get("label") or "")
    obs["guard_switcher"]["focus_returned"] = (fx.App.focused(tree) or {}).get("role") == "AXPopUpButton" and "Tenant A" in obs["guard_switcher"]["focus_after"]
    obs["guard_switcher"]["siblings_back"] = c29.switcher(tree) != {} and fx.App.node(tree, "AXButton", "Decline") is not None
    obs["guard_switcher"]["still_tenant_a"] = "Tenant A" in str(c29.switcher(app.tree()).get("label") or "")
    obs["guard_switcher"]["approval_kept"] = fx.App.node(app.tree(), "AXButton", "Decline") is not None
    # Palette: "Switch to Tenant B" entry.
    app.keys("k", "command down")
    app.wait_for(lambda t: fx.App.node(t, "AXComboBox", "") is not None or any(n.get("role") == "AXComboBox" for n in t), 8)
    app.keys("Switch to Tenant B")
    time.sleep(0.5)
    app.keycode(36)
    tree, g2 = guard_shown()
    obs["guard_palette"] = {"shown": g2}
    c29.press(app, "AXButton", "Stay")
    app.wait_for(lambda t: dialog(t, GUARD) is None, 8)
    # Go menu: the menu's tenant path is Switch Tenant…, which is the switcher.
    app.menu("Go", "Switch Tenant…")
    app.wait_for(lambda t: any(n.get("role") == "AXGroup" and (n.get("label") or "") == "Switch tenant" for n in t), 8)
    time.sleep(0.3)
    c29.press(app, "AXCheckBox", "Tenant B")
    tree, g3 = guard_shown()
    obs["guard_menu"] = {"shown": g3}
    # Archive and switch: B becomes active; the conversation ended, the decision untaken.
    runs_before = len(c19.harness_runs(mgmt))
    c29.press(app, "AXButton", "Archive and switch")
    tree, switched = app.wait_for(lambda t: "Tenant B" in str(c29.switcher(t).get("label") or "") and fx.App.node(t, "AXHeading", "Home") is not None, 15)
    obs["archive_switch"] = {"switched": switched, "lands_on_home": fx.App.node(tree, "AXHeading", "Home") is not None,
                             "runs_after": len(c19.harness_runs(mgmt)) - runs_before,
                             "engine_gone": c19.wait(lambda: not c19.engines(), 20)}

    # ---- N02: a never-read tenant, one read, the Inbox ----
    tree = app.tree()
    obs["tree_home_fresh"] = c29.save_tree("home-fresh", tree)
    obs["fresh"] = {"home_pending": app.has_text(tree, "First read pending"), "no_attention": not any(n.get("role") == "AXList" and (n.get("label") or "") == "Inbox items" for n in tree)}
    obs["captures"]["home-fresh"] = c29.matrix(app, "home-fresh")
    app.menu("Go", "Inbox")
    tree, inbox = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Inbox") is not None, 10)
    obs["fresh"]["inbox_pending"] = app.has_text(tree, "First read pending")
    obs["fresh"]["inbox_empty"] = not any(n.get("role") == "AXList" and (n.get("label") or "") == "Inbox items" for n in tree)
    obs["tree_inbox_fresh"] = c29.save_tree("inbox-fresh", tree)
    # B has no management client yet: add one (Administration reads need it), then read.
    app.menu("Go", "Settings")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Settings") is not None, 10)
    c8.save_client(app)
    app.menu("Go", "Home")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Home") is not None, 10)
    c29.press(app, "AXButton", "Read now")
    tree, read_done = app.wait_for(lambda t: any(x.startswith("Read finished at") for x in app.texts(t)), 30)
    status_line = next((x for x in app.texts(tree) if x.startswith("Read finished at")), None)
    # The toast states the same completion line as the status (one text each).
    toast_line = status_line if sum(1 for x in app.texts(tree) if x == status_line) >= 2 else None
    obs["read"] = {"done": read_done, "status": status_line, "toast": toast_line,
                   "requests": len([r for r in mgmt.requests() if r["path"].startswith("/aisec/")])}
    obs["tree_home_read"] = c29.save_tree("home-read", tree)
    obs["captures"]["home-read"] = c29.matrix(app, "home-read")
    app.menu("Go", "Inbox")
    tree, _ = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Inbox") is not None and any(n.get("role") == "AXList" and (n.get("label") or "") == "Inbox items" for n in t), 12)
    tabs = [n.get("label") for n in tree if n.get("role") in ("AXRadioButton", "AXTab", "AXButton") and (n.get("label") or "").split(" ")[0] in ("All", "Needs", "Updates", "Snoozed")]
    rows = [n for n in tree if n.get("role") == "AXButton" and n.get("label") and any(k in n["label"] for k in ("sessions", "expires", "quota", "setup warning"))]
    obs["inbox"] = {"tabs": tabs, "rows": [r["label"][:60] for r in rows]}
    obs["tree_inbox"] = c29.save_tree("inbox", tree)
    obs["captures"]["inbox"] = c29.matrix(app, "inbox")
    if rows:
        app.ax("press", "AXButton", rows[0]["label"][:40])
        tree, detail = app.wait_for(lambda t: any(n.get("role") == "AXHeading" and n.get("label") == rows[0]["label"].split(" · ")[0] for n in t) or app.has_text(t, "Why this matters"), 8)
        obs["inbox"]["detail"] = detail
        # The detail pane's contextual action: the first button inside the Details region
        # that is not a hold action.
        pane = next((i for i, n in enumerate(tree) if n.get("role") in ("AXGroup", "AXLandmarkComplementary") and (n.get("label") or "") == "Details"), None)
        if pane is None:
            pane = next((i for i, n in enumerate(tree) if n.get("subrole") == "AXLandmarkComplementary"), None)
        candidates = [n for n in (tree[pane + 1:] if pane is not None else tree) if n.get("role") == "AXButton" and n.get("label") and not (n["label"].startswith(("Acknowledge", "Snooze", "Unsnooze", "Mark as")))]
        action = candidates[0] if candidates else None
        obs["inbox"]["action"] = action and action.get("label")
        if action:
            app.ax("press", "AXButton", action["label"])
            tree, navigated = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Inbox") is None and c29.nav(t, "Destinations") is not None, 10)
            obs["inbox"]["navigated"] = navigated
            obs["inbox"]["destination_heading"] = next((n.get("label") for n in tree if n.get("role") == "AXHeading"), None)

    # ---- N03: snooze parity, acknowledge, unsnooze ----
    app.menu("Go", "Home")
    tree, _ = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Home") is not None, 10)
    snooze = next((n for n in tree if n.get("role") == "AXButton" and (n.get("label") or "").startswith("Snooze 24 hours: ")), None)
    obs["snooze"] = {"offered_on_home": snooze is not None}
    if snooze:
        title = snooze["label"][len("Snooze 24 hours: "):]
        app.ax("press", "AXButton", snooze["label"])
        tree, toasted = app.wait_for(lambda t: any(x.startswith(f"{title} snoozed · returns ") or f"{title} snoozed · returns " in x for x in app.texts(t)), 10)
        toast_text = next((x for x in app.texts(tree) if f"{title} snoozed · returns " in x), "")
        returns_toast = toast_text.split("returns ", 1)[1].rstrip("×").strip() if "returns " in toast_text else None
        obs["snooze"].update({"title": title, "toast": toasted, "returns_toast": returns_toast,
                              "off_home": not any(n.get("role") == "AXButton" and (n.get("label") or "") == snooze["label"] for n in tree)})
        app.menu("Go", "Inbox")
        app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Inbox") is not None, 10)
        c29.press(app, "AXRadioButton", "Snoozed") if fx.App.node(app.tree(), "AXRadioButton", "Snoozed") else c29.press(app, "AXButton", "Snoozed")
        tree, _ = app.wait_for(lambda t: any(n.get("role") == "AXButton" and title in (n.get("label") or "") for n in t), 8)
        row = next((n for n in tree if n.get("role") == "AXButton" and title in (n.get("label") or "")), None)
        row_returns = row and row["label"].split("Returns ", 1)[1].strip() if row and "Returns " in row["label"] else None
        app.ax("press", "AXButton", row["label"][:40]) if row else None
        tree, _ = app.wait_for(lambda t: any((n.get("label") or "").startswith("Returns ") for n in t if n.get("role") == "AXStaticText"), 8)
        pane_returns = next(((n.get("label") or "")[len("Returns "):] for n in tree if n.get("role") == "AXStaticText" and (n.get("label") or "").startswith("Returns ")), None)
        obs["snooze"].update({"returns_row": row_returns, "returns_pane": pane_returns,
                              "all_equal": bool(returns_toast) and returns_toast == row_returns == pane_returns})
        obs["tree_snoozed"] = c29.save_tree("inbox-snoozed", tree)
        obs["captures"]["inbox-snoozed"] = c29.matrix(app, "inbox-snoozed")
        c29.press(app, "AXButton", "Unsnooze")
        tree, back = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Snooze 24 hours: " + title) is not None, 8)
        obs["snooze"]["unsnoozed"] = back
        # Acknowledge from the All tab, where the acknowledged row stays listed with its mark.
        c29.press(app, "AXRadioButton", "All") if fx.App.node(app.tree(), "AXRadioButton", "All") else c29.press(app, "AXButton", "All")
        time.sleep(0.4)
        c29.press(app, "AXButton", "Acknowledge")
        tree, acked = app.wait_for(lambda t: app.has_text(t, "Acknowledged") and fx.App.node(t, "AXButton", "Mark as needing action") is not None, 8)
        obs["snooze"]["acknowledged"] = acked
        app.menu("Go", "Home")
        tree, _ = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Home") is not None, 10)
        obs["snooze"]["ack_off_home"] = not any(n.get("role") == "AXButton" and (n.get("label") or "") == "Snooze 24 hours: " + title for n in tree)

    # ---- N04: accessibility additions on the changed UI (DESIGN-V2 §8, T09) ----
    def window(tree):
        return next((n for n in tree if n.get("role") == "AXWindow"), None)

    def inside(node, win, slack=1):
        return bool(node and node.get("pos") and win and win.get("pos")
                    and node["pos"][0] >= win["pos"][0] - slack and node["pos"][1] >= win["pos"][1] - slack
                    and node["pos"][0] + node["size"][0] <= win["pos"][0] + win["size"][0] + slack
                    and node["pos"][1] + node["size"][1] <= win["pos"][1] + win["size"][1] + slack)

    def inside_x(node, win, slack=1):
        """Horizontally within the window: the page scrolls vertically by design, never sideways."""
        return bool(node and node.get("pos") and win and win.get("pos")
                    and node["pos"][0] >= win["pos"][0] - slack
                    and node["pos"][0] + node["size"][0] <= win["pos"][0] + win["size"][0] + slack)

    def extent(tree):
        win = window(tree)
        nodes = [n for n in tree if n.get("pos") and n.get("size") and n.get("role") != "AXWindow"]
        if not win or not nodes:
            return None
        right = max(n["pos"][0] + n["size"][0] for n in nodes)
        return {"window_right": win["pos"][0] + win["size"][0], "content_right": right, "overflow_px": max(0, right - (win["pos"][0] + win["size"][0]))}

    a11y = {}
    # Keyboard: from Read now, Tab reaches the tabs, the rows and the detail actions, every focused frame inside the window.
    app.menu("Go", "Inbox")
    tree, _ = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Inbox") is not None and any(n.get("role") == "AXList" and (n.get("label") or "") == "Inbox items" for n in t), 12)
    c29.press(app, "AXRadioButton", "All") if fx.App.node(tree, "AXRadioButton", "All") else c29.press(app, "AXButton", "All")
    time.sleep(0.4)
    app.ax("focus", "AXButton", "Read now")
    walk = []
    for _ in range(16):
        app.keycode(48)
        tt = app.tree()
        f = fx.App.focused(tt) or {}
        walk.append({"role": f.get("role"), "label": (f.get("label") or "")[:60], "inside": inside(f, window(tt))})
    labels = [w["label"] for w in walk]
    a11y["walk"] = walk
    a11y["keyboard"] = {
        "tabs": any(l.startswith("Needs action") for l in labels) and any(l.startswith("Snoozed") for l in labels),
        "row": any(any(k in l for k in ("sessions", "expires", "quota", "setup warning")) for l in labels),
        "detail_actions": any(l.startswith(("Acknowledge", "Mark as needing action")) for l in labels) and any(l.startswith(("Snooze 24 hours", "Unsnooze")) for l in labels),
        "all_inside": all(w["inside"] for w in walk if w["role"]),
    }
    # Enter on a focused row opens its detail (the pane's heading names it).
    app.ax("focus", "AXButton", "Read now")
    for _ in range(6):
        app.keycode(48)
        f = fx.App.focused(app.tree()) or {}
        if f.get("role") == "AXButton" and any(k in (f.get("label") or "") for k in ("sessions", "expires", "quota", "setup warning")):
            break
    row_label = (f.get("label") or "")
    app.keycode(36)
    tree, opened = app.wait_for(lambda t: any(n.get("role") == "AXHeading" and n.get("label") and n["label"] in row_label and n.get("label") != "Inbox" for n in t), 6)
    a11y["keyboard"]["enter_opens_detail"] = opened
    # Read status is a live region: its line is in the tree as status text.
    app.menu("Go", "Home")
    tree, _ = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Home") is not None, 10)
    a11y["status_line_present"] = any(x.startswith("Read finished at") for x in app.texts(tree))
    # Minimum window and 200% zoom in both themes: the Inbox columns stack; rows, detail text, facts and every action inside the window, no horizontal scroll.
    base = c28.window_size(pid)
    c28.set_window_size(pid, 900, 600)
    app.menu("Go", "Inbox")
    tree, _ = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Inbox") is not None, 10)
    rows = [n for n in tree if n.get("role") == "AXButton" and n.get("label") and any(k in n["label"] for k in ("sessions", "expires", "quota", "setup warning"))]
    if rows:
        app.ax("press", "AXButton", rows[0]["label"][:40])
        app.wait_for(lambda t: app.has_text(t, "Why this matters"), 6)
    c29.zoom(app, 10)
    zoomed = {}
    for theme in ("Light", "Dark"):
        c29.appearance(app, theme)
        tree = app.tree()
        win = window(tree)
        pane = next((n for n in tree if n.get("subrole") == "AXLandmarkComplementary" or (n.get("role") == "AXGroup" and (n.get("label") or "") == "Details")), None)
        actions = [n for n in tree if n.get("role") == "AXButton" and (n.get("label") or "").startswith(("Acknowledge", "Mark as needing action", "Snooze 24 hours", "Unsnooze", "Open "))]
        hbars = [n for n in tree if n.get("role") == "AXScrollBar" and (n.get("orientation") == "horizontal" or (n.get("size") and n["size"][0] > n["size"][1]))]
        texts = [n for n in tree if n.get("role") == "AXStaticText" and n.get("pos") and n.get("size") and n["size"][0] > 0]
        # Every action is reached by focusing it (WebKit scrolls it into view): then its whole frame is inside the window.
        reached = []
        for a in actions:
            app.ax("focus", "AXButton", a["label"][:40])
            time.sleep(0.3)
            ta = app.tree()
            fa = fx.App.node(ta, "AXButton", a["label"][:40])
            reached.append({"label": a["label"][:40], "inside_x": inside_x(a, win), "inside_when_focused": inside(fa, window(ta))})
        zoomed[theme.lower()] = {
            "window": c28.window_size(pid), "extent": extent(tree), "horizontal_scrollbars": len(hbars),
            "pane_inside": inside_x(pane, win), "pane_right": pane and pane["pos"][0] + pane["size"][0], "window_right": win and win["pos"][0] + win["size"][0],
            "actions": reached,
            "texts_inside": all(inside_x(t, win) for t in texts), "why_present": app.has_text(tree, "Why this matters"),
            "rows_inside": all(inside_x(r, win) for r in tree if r.get("role") == "AXButton" and r.get("label") and any(k in r["label"] for k in ("sessions", "expires", "quota", "setup warning"))),
        }
        zoomed[theme.lower()]["capture"] = c29.capture(app, f"inbox-min-200-{theme.lower()}")
        zoomed[theme.lower()]["tree"] = c29.save_tree(f"inbox-min-200-{theme.lower()}", tree)
    c29.zoom(app, -10)
    c29.appearance(app, "Match System")
    if base:
        c28.set_window_size(pid, base[0], base[1])
    a11y["zoom"] = zoomed
    obs["captures"]["inbox-min-200"] = {k: v["capture"] for k, v in zoomed.items()}
    # Reduce Motion: with the system setting on and the app relaunched, a snooze toast persists until dismissed; the setting is restored.
    had_key = fx.sh(["defaults", "read", "com.apple.universalaccess", "reduceMotion"]).returncode == 0
    prior = fx.sh(["defaults", "read", "com.apple.universalaccess", "reduceMotion"]).stdout.strip()
    rm = {}
    try:
        fx.sh(["defaults", "write", "com.apple.universalaccess", "reduceMotion", "-bool", "true"])
        time.sleep(1)
        rm["active"] = c28.reduce_motion()
        pid = app.launch()
        app.wait_for(lambda t: c29.nav(t, "Destinations") is not None, 15)
        app.menu("Go", "Home")
        app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Home") is not None, 10)
        # Reads and holds live for the session: the relaunched desktop reads again first.
        c29.press(app, "AXButton", "Read now")
        tree, rm["read_again"] = app.wait_for(lambda t: any(x.startswith("Read finished at") for x in app.texts(t)), 30)
        # The attention list follows the read; the AX tree fills in a moment later.
        tree, offered = app.wait_for(lambda t: any(n.get("role") == "AXButton" and (n.get("label") or "").startswith("Snooze 24 hours: ") for n in t), 10)
        snooze2 = next((n for n in tree if n.get("role") == "AXButton" and (n.get("label") or "").startswith("Snooze 24 hours: ")), None)
        rm["offered"] = snooze2 is not None
        rm["tree"] = c29.save_tree("reduce-motion-home", tree)
        if snooze2 is None:
            # Nothing on Home needs attention: snooze from the Inbox detail pane instead (the same intent, I11).
            app.menu("Go", "Inbox")
            app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Inbox") is not None, 10)
            c29.press(app, "AXRadioButton", "All") if fx.App.node(app.tree(), "AXRadioButton", "All") else c29.press(app, "AXButton", "All")
            ti, _ = app.wait_for(lambda t: any(n.get("role") == "AXList" and (n.get("label") or "") == "Inbox items" for n in t), 8)
            rows_i = [n for n in ti if n.get("role") == "AXButton" and n.get("label") and any(k in n["label"] for k in ("sessions", "expires", "quota", "setup warning"))]
            if rows_i:
                app.ax("press", "AXButton", rows_i[0]["label"][:40])
                ti, _ = app.wait_for(lambda t: any(n.get("role") == "AXButton" and (n.get("label") or "").startswith("Snooze 24 hours: ") for n in t), 8)
                snooze2 = next((n for n in ti if n.get("role") == "AXButton" and (n.get("label") or "").startswith("Snooze 24 hours: ")), None)
            rm["offered_in_inbox"] = snooze2 is not None
        if snooze2:
            title2 = snooze2["label"][len("Snooze 24 hours: "):]
            app.ax("press", "AXButton", snooze2["label"])
            _, shown = app.wait_for(lambda t: any(f"{title2} snoozed · returns " in x for x in app.texts(t)), 10)
            time.sleep(8)
            rm["persists"] = shown and any(f"{title2} snoozed · returns " in x for x in app.texts(app.tree()))
            c29.press(app, "AXButton", "Dismiss")
            time.sleep(0.6)
            rm["dismissed"] = not any(f"{title2} snoozed · returns " in x for x in app.texts(app.tree()))
            # Back as it was for the record: the item returns to Home.
            app.menu("Go", "Inbox")
            app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Inbox") is not None, 10)
            c29.press(app, "AXRadioButton", "Snoozed") if fx.App.node(app.tree(), "AXRadioButton", "Snoozed") else c29.press(app, "AXButton", "Snoozed")
            tr, _ = app.wait_for(lambda t: any(n.get("role") == "AXButton" and title2 in (n.get("label") or "") for n in t), 8)
            row2 = next((n for n in tr if n.get("role") == "AXButton" and title2 in (n.get("label") or "")), None)
            if row2:
                app.ax("press", "AXButton", row2["label"][:40])
                c29.press(app, "AXButton", "Unsnooze")
    finally:
        if had_key:
            fx.sh(["defaults", "write", "com.apple.universalaccess", "reduceMotion", "-bool", "true" if prior == "1" else "false"])
        else:
            fx.sh(["defaults", "delete", "com.apple.universalaccess", "reduceMotion"])
        time.sleep(1)
    rm["restored"] = c28.reduce_motion() == (had_key and prior == "1")
    a11y["reduce_motion"] = rm
    obs["a11y"] = a11y
    app.quit()

    run_path = os.path.join(NATIVE, "observations.json")
    with open(run_path, "w") as f:
        json.dump(json.loads(json.dumps({"observations": obs, "log": LOG}).replace(rel(NATIVE), rel(FINAL))), f, indent=1)
    run_rel = rel(run_path)
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C30"]}
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [e for e in evidence if e]})

    caps = lambda name: list(obs["captures"].get(name, {}).values())  # noqa: E731
    g, gp, gm, a = obs["guard_switcher"], obs["guard_palette"], obs["guard_menu"], obs["archive_switch"]
    add("C30-N01", obs["harness_ready"] and obs["approval_pending"] and g["shown"] and g["body_names_target"] and set(g["buttons"]) == {"Stay", "Discard and switch", "Archive and switch"} and g["stay_closed"] and g["still_tenant_a"] and g["approval_kept"] and gp["shown"] and gm["shown"] and a["switched"] and a["runs_after"] == 0,
        f"Harness ready {obs['harness_ready']}, approval pending {obs['approval_pending']}; guard via switcher {g['shown']} naming the target {g['body_names_target']} with {g['buttons']}; Stay closed it {g['stay_closed']}, Tenant A kept {g['still_tenant_a']}, approval kept {g['approval_kept']}; via palette {gp['shown']}; via Go › Switch Tenant… {gm['shown']}; Archive and switch → Tenant B {a['switched']}, no further model turns {a['runs_after'] == 0}, engine gone {a['engine_gone']}",
        [run_rel, obs["tree_approval"], obs["tree_guard"]] + caps("guard"))
    fr, rd, ib = obs["fresh"], obs["read"], obs["inbox"]
    add("C30-N02", fr["home_pending"] and fr["inbox_pending"] and fr["inbox_empty"] and rd["done"] and bool(rd["status"]) and bool(rd["toast"]) and len(ib["rows"]) > 0 and ib.get("detail") and ib.get("navigated"),
        f"fresh tenant: Home pending {fr['home_pending']}, Inbox pending {fr['inbox_pending']} and empty {fr['inbox_empty']}; Read now → '{rd['status']}', toast '{rd['toast']}' ({rd['requests']} synthetic reads); Inbox tabs {ib['tabs']} rows {ib['rows']}; detail {ib.get('detail')}; action '{ib.get('action')}' navigated {ib.get('navigated')} to '{ib.get('destination_heading')}'",
        [run_rel, obs["tree_home_fresh"], obs["tree_inbox_fresh"], obs["tree_home_read"], obs["tree_inbox"]] + caps("home-fresh") + caps("home-read") + caps("inbox"))
    s = obs["snooze"]
    add("C30-N03", s["offered_on_home"] and s.get("toast") and s.get("off_home") and s.get("all_equal") and s.get("unsnoozed") and s.get("acknowledged") and s.get("ack_off_home"),
        f"snooze offered on Home {s['offered_on_home']} for '{s.get('title')}'; toast {s.get('toast')} returns '{s.get('returns_toast')}', row '{s.get('returns_row')}', pane '{s.get('returns_pane')}' all equal {s.get('all_equal')}; off Home {s.get('off_home')}; Unsnooze {s.get('unsnoozed')}; Acknowledge {s.get('acknowledged')} and off Home {s.get('ack_off_home')}",
        [run_rel, obs.get("tree_snoozed")] + caps("inbox-snoozed"))
    ay = obs.get("a11y") or {}
    kb, zm, rm = ay.get("keyboard") or {}, ay.get("zoom") or {}, ay.get("reduce_motion") or {}
    zoom_ok = bool(zm) and all(
        v["extent"] and v["extent"]["overflow_px"] == 0 and v["horizontal_scrollbars"] == 0 and v["pane_inside"] and v["texts_inside"] and v["rows_inside"] and v["why_present"] and v["actions"] and all(a["inside_x"] and a["inside_when_focused"] for a in v["actions"])
        for v in zm.values())
    add("C30-N04", kb.get("tabs") and kb.get("row") and kb.get("detail_actions") and kb.get("all_inside") and kb.get("enter_opens_detail") and ay.get("status_line_present") and g.get("siblings_hidden") and g.get("modal") and g.get("initial_focus_stay") and g.get("focus_returned") and g.get("siblings_back") and zoom_ok and rm.get("active") and rm.get("read_again") and rm.get("persists") and rm.get("dismissed") and rm.get("restored"),
        f"keyboard: Tab from Read now reached the tabs {kb.get('tabs')}, a row {kb.get('row')} and the detail actions {kb.get('detail_actions')}, every focused frame inside the window {kb.get('all_inside')}, Enter opened the row's detail {kb.get('enter_opens_detail')}; read status in the tree {ay.get('status_line_present')}; guard: siblings hidden beneath it {g.get('siblings_hidden')}, modal {g.get('modal')}, initial focus '{g.get('initial_focus')}' (Stay, the safe default: {g.get('initial_focus_stay')}), Escape returned focus to '{g.get('focus_after')}' ({g.get('focus_returned')}), siblings back {g.get('siblings_back')}; at {(zm.get('light') or {}).get('window')} and 200%: light overflow {((zm.get('light') or {}).get('extent') or {}).get('overflow_px')} px, dark {((zm.get('dark') or {}).get('extent') or {}).get('overflow_px')} px, pane right {(zm.get('light') or {}).get('pane_right')} vs window {(zm.get('light') or {}).get('window_right')}, actions within the width and fully inside once focused {[(a['inside_x'], a['inside_when_focused']) for a in (zm.get('light') or {}).get('actions') or []]}; Reduce Motion active {rm.get('active')}: snooze toast persisted past 8 s {rm.get('persists')}, dismissed on request {rm.get('dismissed')}, setting restored {rm.get('restored')}",
        [run_rel, obs.get("tree_guard"), rm.get("tree")] + [v.get("tree") for v in zm.values()] + caps("inbox-min-200"))

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1", "subject": "C30", "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (System Events and the ax helper) with fixture WebView self-capture; tools/native-acceptance/acceptance_c30.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "Pixels come from the fixture build's own WKWebView snapshot; menus and dialogs are verified through System Events and the accessibility tree.",
            "Reads run against the synthetic management endpoint (tools/native-acceptance/mgmt_server.py); the Harness approval is raised by the real pinned engine against the same synthetic Gateway playing the model.",
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
        print(c["result"].upper(), c["id"], c["observation"][:200])
    return 0 if all(c["result"] == "pass" for c in checks) else 1


if __name__ == "__main__":
    sys.exit(main())
