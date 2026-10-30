#!/usr/bin/env python3
"""C38 native acceptance (D059): the Harness as the prototype board draws it
(R2App start=harness), driven through the real fixture window with the real
pinned engine against the synthetic Gateway service that plays the model.

Run from the repository root after `xtask native-prepare --chunk C38`.
Requires the macOS Accessibility grant. Synthetic services only; the fixture
data root is removed at the end.

Checks (docs/architecture/native-checks.json, C38): the board over a running
conversation with a decision held in the aside, decided there (N01); keyboard
reach, the focus ring on Send, and the measured layouts at 1440 wide and at
900×600 with 200% zoom in both themes (N02). The parity composites
(docs/planning/evidence/C38/parity) are captured from the N01 state: the
conversation with its decision waiting, as the board shows it.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fixture_app as fx  # noqa: E402
import acceptance_c08 as c8  # noqa: E402
import acceptance_c16 as c16  # noqa: E402
import acceptance_c19 as c19  # noqa: E402
import acceptance_c28 as c28  # noqa: E402
import acceptance_c29 as c29  # noqa: E402
import acceptance_c31 as c31  # noqa: E402
import native_sheets as sheets  # noqa: E402

ROOT = os.getcwd()
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence/C38")
NATIVE = os.path.join(EVIDENCE, "native")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json")
PARITY = os.path.join(EVIDENCE, "parity")
DC = os.path.join(ROOT, "tools/parity")
INFERENCE_KEY = "SYNTHETIC-C38-HARNESS-KEY"
MARKERS = ["Profile 0", "WROTE", "Operation not permitted", "fork", "Failed to write file", "unavailable", "unsupported"]
FIRST = "Why were 14 prompts blocked on claims-assistant overnight? Is it one actor?"
SECOND = "Which MCP servers does this workspace have?"
TITLE_BAR = 28
LOG = []


def rel(path):
    return os.path.relpath(path, ROOT)


def ask(mgmt, name, arguments, namespace="airs_desktop"):
    """The next turn's model asks for this one tool call."""
    tool = {"name": name, "arguments": json.dumps(arguments)}
    if namespace:
        tool["namespace"] = namespace
    mgmt.mode(harnessTool=tool, toolMarkers=MARKERS)


def plain(mgmt):
    mgmt.mode(harnessTool=None, toolMarkers=MARKERS)


def send(app, text):
    app.type_into("AXTextArea", "Message", text)
    c29.press(app, "AXButton", "Send")


def answers(app, tree=None):
    return sum(c19.ANSWER in x for x in app.texts(tree if tree is not None else app.tree()))


def landmark(tree, subrole, label):
    return next((n for n in tree if n.get("subrole") == subrole and (n.get("label") or "") == label), None)


def within(tree, node):
    if not node:
        return []
    i = tree.index(node)
    out = []
    for m in tree[i + 1:]:
        if m.get("depth", 0) <= node.get("depth", 0):
            break
        out.append(m)
    return out


def texts(nodes):
    return " ".join((n.get("label") or n.get("value") or "") for n in nodes)


def size_window(app, pid, w, h):
    """Places the window at the screen's origin, asks for w×h plus the title bar, and
    returns the WebView's measured CSS size (named and asserted, never the request)."""
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {pid}) to set position of window 1 to {{0, 0}}'])
    requested = [w, h + TITLE_BAR]
    for _ in range(3):
        c28.set_window_size(pid, requested[0], requested[1])
        time.sleep(0.8)
        g = geometry(app.tree())
        vp = g["viewport"] or g["web"]
        if not vp or vp[1] == h:
            break
        # The title bar's height is the window's to decide: the viewport is measured and the
        # request corrected by the difference, so the row is named by what was measured.
        requested = [requested[0], requested[1] + (h - vp[1])]
    frame = c29.window_frame(pid)
    web = next((n for n in app.tree() if n.get("role") == "AXWebArea" and n.get("size")), None)
    return {"requested": [w, h], "window": frame, "web": [int(web["size"][0]), int(web["size"][1])] if web else None}


def geometry(tree):
    web = next((n for n in tree if n.get("role") == "AXWebArea" and n.get("size")), None)
    scroll = next((n for n in tree if n.get("role") == "AXScrollArea" and n.get("size")), None)
    win = next((n for n in tree if n.get("role") == "AXWindow"), None)
    return {"web": [int(web["size"][0]), int(web["size"][1])] if web else None,
            "viewport": [int(scroll["size"][0]), int(scroll["size"][1])] if scroll else None,
            "window": win.get("size") if win else None}


def overflow(app):
    tree = app.tree()
    win = next((n for n in tree if n.get("role") == "AXWindow"), None)
    nodes = [n for n in tree if n.get("pos") and n.get("size") and n["size"][0] > 0
             and n.get("role") in ("AXButton", "AXStaticText", "AXTextField", "AXTextArea", "AXSearchField", "AXCheckBox", "AXPopUpButton", "AXLink", "AXHeading")]
    right = max(n["pos"][0] + n["size"][0] for n in nodes) if nodes and win else None
    hbars = [n for n in tree if n.get("role") == "AXScrollBar" and (n.get("orientation") == "horizontal" or (n.get("size") and n["size"][0] > n["size"][1]))]
    return {"overflow_px": max(0, right - (win["pos"][0] + win["size"][0])) if right is not None else None, "horizontal_scrollbars": len(hbars)}, tree


def columns(tree, zoom):
    """The three columns' usability at this zoom: the rail (navigation 'Conversations'), the
    composer's Send and the aside's Approve once keep their 32 CSS px; at 200% the rail, the
    composer and the aside stack (each below the previous), at 100% they sit side by side."""
    rail = landmark(tree, "AXLandmarkNavigation", "Conversations")
    aside = landmark(tree, "AXLandmarkComplementary", "Decisions")
    message = fx.App.node(tree, "AXTextArea", "Message")
    send_btn = fx.App.node(tree, "AXButton", "Send")
    approve = fx.App.node(tree, "AXButton", "Approve once")
    sizes = {k: (n.get("size") if n else None) for k, n in (("send", send_btn), ("approve", approve), ("new", fx.App.node(tree, "AXButton", "New investigation")))}
    controls_ok = all(s and s[0] >= 32 * zoom - 1 and s[1] >= 32 * zoom - 1 for s in sizes.values())
    pos = {k: (n.get("pos") if n else None) for k, n in (("rail", rail), ("message", message), ("aside", aside))}
    if not all(pos.values()):
        return {"ok": False, "sizes": sizes, "pos": pos, "zoom": zoom}
    if zoom == 1:
        arranged = pos["rail"][0] < pos["message"][0] < pos["aside"][0]
    else:
        arranged = pos["rail"][1] < pos["message"][1] < pos["aside"][1]
    return {"ok": controls_ok and arranged, "controls_ok": controls_ok, "arranged": arranged, "sizes": sizes, "pos": pos, "zoom": zoom}


def composite(app, theme, web_size, prepared=None):
    """The parity composite for this theme from the current capture (prototype left, app right); the
    record of the bundle the captures came from is rewritten with every capture (attempt-01 E1)."""
    name = f"harness-{theme.lower()}"
    if prepared:
        os.makedirs(PARITY, exist_ok=True)
        with open(os.path.join(PARITY, "bundle.json"), "w") as f:
            json.dump({"bundle": prepared["bundle"], "executable_sha256": prepared["executable_sha256"], "from": "native-prepare.json",
                       "captured_by": "tools/native-acceptance/acceptance_c38.py", "captured_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}, f, indent=2)
            f.write("\n")
    cap = c29.capture(app, f"parity-{name}")
    if not cap:
        raise SystemExit(f"no capture for {name}")
    os.makedirs(os.path.join(PARITY, "app"), exist_ok=True)
    os.makedirs(os.path.join(PARITY, "proto"), exist_ok=True)
    app_png = os.path.join(PARITY, "app", f"{name}.png")
    shutil.copy(os.path.join(ROOT, cap), app_png)
    web_w, web_h = web_size
    proto_png = os.path.join(PARITY, "proto", f"{name}.png")
    subprocess.run([sys.executable, "-I", os.path.join(DC, "dc_render.py"), "--board", "R2App", "--prop", "start=harness",
                    "--prop", f"theme={theme.lower()}", "--width", str(web_w), "--height", str(web_h), "--out", proto_png], check=True, capture_output=True)
    out = os.path.join(PARITY, f"{name}.png")
    subprocess.run([sys.executable, "-I", os.path.join(DC, "compose.py"), "--left", proto_png, "--right", app_png,
                    "--out", out, "--title", f"Harness · {theme.lower()} · {web_w}×{web_h}"], check=True, capture_output=True)
    return rel(out)


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
    work = tempfile.mkdtemp(prefix="airs-v2-c38-acceptance-")
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
    time.sleep(1.5)
    endpoint = fx.ScanEndpoint(os.path.join(work, "endpoint"), c8.SCAN_KEY)
    endpoint.install_roots()
    endpoint.start()
    mgmt = c8.MgmtEndpoint(os.path.join(work, "endpoint"), os.path.join(work, "mgmt"))
    mgmt.install()
    mgmt.start()
    mgmt.mode()
    try:
        return exercise(app, endpoint, mgmt, prepared, FINAL)
    finally:
        endpoint.stop()
        mgmt.stop()
        shutil.rmtree(work, ignore_errors=True)


def exercise(app, endpoint, mgmt, prepared, FINAL):
    global NATIVE
    runtime = f"{mgmt.origin}/rt/v1"
    obs = {"captures": {}}
    sheets.enable()
    app.launch()
    pid = app.pid
    # One tenant with the Gateway workspace Team A chosen and its inference key stored, then v2.
    app.open_settings()
    app.type_into("AXTextField", "TSG ID (optional)", c8.TSG)
    app.create_custom_tenant("Northwind Health", endpoint.origin)
    app.save_key(c8.SCAN_KEY)
    c8.save_client(app)
    c29.press(app, "AXCheckBox", "Use the new interface")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Home") is not None and c29.nav(t, "Destinations") is not None, 15)
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

    # ---- N01: the board over a running conversation; the decision held and decided in the aside ----
    n1 = {}
    app.menu("Go", "Harness")
    tree, n1["screen"] = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Harness") is not None
                                      and fx.App.node(t, "AXTextArea", "Message") is not None, 15)
    n1["status_line"] = app.has_text(tree, "Northwind Health · Engine") and app.has_text(tree, "Team A")
    rail = landmark(tree, "AXLandmarkNavigation", "Conversations")
    n1["rail_label"] = app.has_text(tree, "Saved conversations · encrypted")
    n1["rail_new"] = any(n.get("role") == "AXButton" and (n.get("label") or "") == "New investigation" for n in within(tree, rail))
    aside = landmark(tree, "AXLandmarkComplementary", "Decisions")
    n1["aside_empty"] = aside is not None and "Nothing to decide yet" in texts(within(tree, aside))
    n1["no_end_before"] = fx.App.node(tree, "AXButton", "End conversation") is None
    # The aside's summary: the workspace catalog (two servers from the synthetic plane), each not signed in.
    tree, n1["mcp_summary"] = app.wait_for(lambda t: app.has_text(t, "Workspace MCP servers · 0 of 2"), 20)
    aside_text = texts(within(tree, landmark(tree, "AXLandmarkComplementary", "Decisions")))
    n1["mcp_rows"] = [name for name in ("Tools tools", "Docs tools") if name in aside_text]
    n1["mcp_standing"] = aside_text.count("Not signed in") >= 2
    obs["tree_empty"] = c29.save_tree("harness-empty", tree)
    obs["captures"]["harness-empty"] = c29.matrix(app, "harness-empty")
    runs_before = len(c19.harness_runs(mgmt))
    plain(mgmt)
    send(app, FIRST)
    tree, n1["answered"] = app.wait_for(lambda t: answers(app, t) >= 1 and fx.App.node(t, "AXButton", "End conversation") is not None, 60)
    n1["model_turns"] = len(c19.harness_runs(mgmt)) - runs_before
    n1["operator_shown"] = sum(FIRST in x for x in app.texts(tree)) >= 2  # the rail row and the transcript bubble
    band_end = fx.App.node(tree, "AXButton", "End conversation")
    heading = fx.App.node(tree, "AXHeading", "Harness")
    n1["end_in_band"] = bool(band_end and heading and band_end.get("pos") and heading.get("pos") and abs(band_end["pos"][1] - heading["pos"][1]) < 60)
    n1["rail_live"] = any(FIRST[:20] in (n.get("label") or n.get("value") or "") for n in within(tree, landmark(tree, "AXLandmarkNavigation", "Conversations")))
    obs["tree_first"] = c29.save_tree("harness-first-send", tree)
    # The decision: a read-only desktop read asked in the aside, the pending line in the transcript.
    ask(mgmt, "mcpServers", {})
    send(app, SECOND)
    tree, n1["asked"] = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Approve once") is not None
                                     and fx.App.node(t, "AXButton", "Decline") is not None, 60)
    aside = landmark(tree, "AXLandmarkComplementary", "Decisions")
    inside = within(tree, aside)
    n1["decision_in_aside"] = any(n.get("role") == "AXButton" and (n.get("label") or "") == "Approve once" for n in inside) and "airs_desktop" in texts(inside) and "mcpServers" in texts(inside)
    n1["read_only_shown"] = "Read-only" in texts(inside)
    n1["pending_line"] = app.has_text(tree, "Your OK is needed for a tool call")
    n1["aside_not_empty"] = "Nothing to decide yet" not in texts(inside)
    obs["tree_decision"] = c29.save_tree("harness-decision", tree)
    obs["captures"]["harness-decision"] = c29.matrix(app, "harness-decision")
    # The parity composites, from this state, at the board's size in both themes (D059).
    plain(mgmt)
    g = size_window(app, pid, 1440, 900)
    # Focus leaves the composer (the heading takes it, ringless), so the capture is the board at rest.
    app.ax("focus", "AXHeading", "Harness")
    time.sleep(0.3)
    parity = {}
    for theme in ("Light", "Dark"):
        c29.appearance(app, theme)
        time.sleep(0.7)
        web = geometry(app.tree())["web"] or g["web"]
        parity[theme.lower()] = composite(app, theme, web, prepared)
    c29.appearance(app, "Match System")
    n1["parity"] = parity
    # Allowed in the aside: the catalog is read once and the answer follows; the aside empties.
    reads_before = len([r for r in mgmt.requests() if "mcp-servers" in r.get("path", "")])
    answers_before = answers(app)
    c29.press(app, "AXButton", "Approve once")
    tree, n1["read_done"] = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Approve once") is None
                                         and len([r for r in mgmt.requests() if "mcp-servers" in r.get("path", "")]) > reads_before
                                         and answers(app, t) > answers_before, 60)
    n1["reads"] = len([r for r in mgmt.requests() if "mcp-servers" in r.get("path", "")]) - reads_before
    n1["aside_empty_again"] = "Nothing to decide yet" in texts(within(tree, landmark(tree, "AXLandmarkComplementary", "Decisions")))
    obs["tree_approved"] = c29.save_tree("harness-approved", tree)
    obs["n1"] = n1

    # ---- N02: keyboard reach with a decision waiting; the ring on Send; the layouts ----
    n2 = {}
    ask(mgmt, "mcpServers", {})
    send(app, "And their tools?")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Approve once") is not None, 60)
    plain(mgmt)
    time.sleep(0.5)
    app.ax("focus", "AXButton", "End conversation")
    time.sleep(0.3)
    walk = []
    for _ in range(8):
        app.keycode(48)
        time.sleep(0.15)
        f = fx.App.focused(app.tree()) or {}
        walk.append((f.get("role") or "", f.get("label") or ""))
    n2["walk"] = walk
    labels = [l for _, l in walk]
    # While the engine's turn waits on the decision, Send is disabled (nothing can be sent) and the
    # composer's reachable action is Stop answering; Send is the action once the turn is over.
    action = "Stop answering" if "Stop answering" in labels else "Send"
    n2["composer_action"] = action
    order = [labels.index(x) if x in labels else -1 for x in ("New investigation", "Message", action, "Approve once", "Decline")]
    n2["walk_in_order"] = all(i >= 0 for i in order) and order == sorted(order)
    app.ax("focus", "AXTextArea", "Message")
    time.sleep(0.5)
    ring_plain = c29.capture(app, "harness-ring-plain")
    n2["ring"] = c31.ring_on(app, action, ring_plain, "harness-ring-action")
    layouts = {}
    size_window(app, pid, 1440, 900)
    for theme in ("Light", "Dark"):
        c29.appearance(app, theme)
        time.sleep(0.5)
        o, tree = overflow(app)
        o["decision_populated"] = fx.App.node(tree, "AXButton", "Approve once") is not None
        o["columns"] = columns(tree, 1)
        gm = geometry(tree)
        o["geometry"] = gm
        o["width_1440"] = bool(gm["web"] and gm["web"][0] == 1440)
        name = f"harness-{gm['web'][0]}x{gm['web'][1]}-{theme.lower()}" if gm["web"] else f"harness-unknown-{theme.lower()}"
        o["capture"] = c29.capture(app, name)
        o["tree"] = c29.save_tree(name, tree)
        layouts[name] = o
    size_window(app, pid, 900, 600)
    c29.zoom(app, 10)
    for theme in ("Light", "Dark"):
        c29.appearance(app, theme)
        time.sleep(0.5)
        # The decision card is measured where it stands: scrolled into view by focusing Approve once.
        app.ax("focus", "AXButton", "Approve once")
        time.sleep(0.4)
        o, tree = overflow(app)
        o["decision_populated"] = fx.App.node(tree, "AXButton", "Approve once") is not None
        o["columns"] = columns(tree, 2)
        gm = geometry(tree)
        o["geometry"] = gm
        vp = gm["viewport"] or gm["web"]
        o["window_900x600"] = bool(vp and vp[0] == 900 and vp[1] == 600 and gm["web"] and gm["web"][0] == 900)
        name = f"harness-{vp[0]}x{vp[1]}-200-{theme.lower()}" if vp else f"harness-unknown-200-{theme.lower()}"
        o["capture"] = c29.capture(app, name)
        o["tree"] = c29.save_tree(name, tree)
        layouts[name] = o
    c29.zoom(app, -10)
    c29.appearance(app, "Match System")
    size_window(app, pid, 1440, 900)
    n2["layouts"] = layouts
    obs["captures"]["layouts"] = {k: v["capture"] for k, v in layouts.items()}
    obs["captures"]["harness-ring"] = {"plain": n2["ring"].get("plain"), "focused": n2["ring"].get("capture")}
    # Declined and ended: nothing more runs; the engine goes.
    c29.press(app, "AXButton", "Decline")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Approve once") is None, 40)
    c29.press(app, "AXButton", "End conversation")
    _, n2["ended"] = app.wait_for(lambda t: app.has_text(t, "You ended this conversation"), 30)
    n2["engine_gone"] = c19.wait(lambda: not c19.engines(), 20)
    obs["n2"] = n2
    app.quit()

    run_path = os.path.join(NATIVE, "observations.json")
    with open(run_path, "w") as f:
        json.dump(json.loads(json.dumps({"observations": obs, "log": LOG}).replace(rel(NATIVE), rel(FINAL))), f, indent=1)
    run_rel = rel(run_path)
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C38"]}
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [e for e in evidence if e]})

    caps = lambda name: list(obs["captures"].get(name, {}).values())  # noqa: E731
    n1_ok = all(n1[k] for k in ("screen", "status_line", "rail_label", "rail_new", "aside_empty", "no_end_before", "mcp_summary", "mcp_standing", "answered",
                                 "operator_shown", "end_in_band", "rail_live", "asked", "decision_in_aside", "read_only_shown", "pending_line", "aside_not_empty",
                                 "read_done", "aside_empty_again")) and len(n1["mcp_rows"]) == 2 and n1["model_turns"] >= 1 and n1["reads"] == 1 and len(n1["parity"]) == 2
    add("C38-N01", n1_ok,
        f"board {n1['screen']} with the tenant · engine · workspace line {n1['status_line']}; rail labelled and with New investigation {n1['rail_label'] and n1['rail_new']}; aside 'Nothing to decide yet' {n1['aside_empty']} listing the catalog {n1['mcp_rows']} each not signed in {n1['mcp_standing']}; "
        f"the first send answered {n1['answered']} ({n1['model_turns']} model turn(s)) with the operator's message in the rail and the transcript {n1['operator_shown']}, End conversation in the band {n1['end_in_band']}, the live row in the rail {n1['rail_live']}; "
        f"the read-only desktop read held in the aside {n1['decision_in_aside']} marked Read-only {n1['read_only_shown']} with the transcript's pending line {n1['pending_line']}; allowed there it was read {n1['reads']} time(s) and answered {n1['read_done']}, the aside empty again {n1['aside_empty_again']}; "
        f"parity composites {sorted(n1['parity'].values())}",
        [run_rel, obs["tree_empty"], obs["tree_first"], obs["tree_decision"], obs["tree_approved"]] + caps("harness-empty") + caps("harness-decision") + sorted(n1["parity"].values()))
    lay = n2["layouts"]
    layouts_ok = len(lay) == 4 and all(v["overflow_px"] == 0 and v["horizontal_scrollbars"] == 0 and v["decision_populated"] and v["columns"]["ok"] for v in lay.values()) \
        and sum(1 for v in lay.values() if v.get("width_1440")) == 2 and sum(1 for v in lay.values() if v.get("window_900x600")) == 2
    ring_ok = n2["ring"].get("diff", 0) > 150 and n2["ring"].get("focused") == n2["composer_action"]
    add("C38-N02", n2["walk_in_order"] and ring_ok and layouts_ok and n2["ended"] and n2["engine_gone"],
        f"Tab from End conversation reached {[l for _, l in n2['walk']]} in order {n2['walk_in_order']} (the composer's action {n2['composer_action']}; Send is disabled while the turn waits on the decision); ring on {n2['composer_action']} {n2['ring'].get('diff')} px; "
        + "; ".join(f"{k}: web {v['geometry']['web']} viewport {v['geometry']['viewport']} overflow {v['overflow_px']} px, scroll bars {v['horizontal_scrollbars']}, columns {v['columns'].get('arranged')} controls {v['columns'].get('sizes')}" for k, v in lay.items())
        + f"; declined and ended {n2['ended']}, engine gone {n2['engine_gone']}",
        [run_rel] + [v["tree"] for v in lay.values()] + caps("layouts") + caps("harness-ring"))

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1", "subject": "C38", "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (System Events and the ax helper) with fixture WebView self-capture; tools/native-acceptance/acceptance_c38.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "Pixels come from the fixture build's own WKWebView snapshot; the parity composites put the prototype board (rendered at the WebView's measured size) beside it.",
            "The model is the synthetic Gateway service (tools/native-acceptance/mgmt_server.py); the real pinned engine runs against it. The lifecycle fixtures and the setup panels are exercised in the component suite.",
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
