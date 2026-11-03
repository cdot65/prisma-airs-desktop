#!/usr/bin/env python3
"""C36 native acceptance: Home as the prototype draws it (R2App start=home),
driven through the real fixture window against the synthetic scan and
management endpoints.

Run from the repository root after `xtask native-prepare --chunk C36`.
Requires the macOS Accessibility grant. Synthetic services only; the fixture
data root is removed at the end.

Checks (docs/architecture/native-checks.json, C36): the heading, read
summary, Refresh, Explain banner, the six cards with their pills, figures and
last lines, a card opening its product, Needs attention with its controls or
its empty state, and the quick actions (N01); keyboard reach, the focus ring,
Enter on a card, and the measured 1440-wide and 900×600 / 200% layouts in
both themes (N02).
"""

import json
import os
import re
import shutil
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fixture_app as fx  # noqa: E402
import acceptance_c08 as c8  # noqa: E402
import acceptance_c28 as c28  # noqa: E402
import acceptance_c29 as c29  # noqa: E402
import v2_setup as v2s  # noqa: E402
import acceptance_c31 as c31  # noqa: E402  (focus ring helper)

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C36")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C36" else "native-C36")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C36" else "native-acceptance-C36.json")
LOG = []
TITLE_BAR = 32
PRODUCTS = ["AI Runtime Security", "AI Gateway", "AI Red Teaming", "Model and skill scans", "Agentic Identity Security", "Agentic Endpoint Security"]
PILLS = ("Connected", "Partial", "Needs a role", "Connecting", "Failed", "Not available")
CONTROLS = ("AXButton", "AXCheckBox", "AXPopUpButton", "AXMenuButton", "AXSwitch", "AXToggle")


def rel(path):
    return os.path.relpath(path, ROOT)


def labelled(tree, roles, label):
    return next((n for n in tree if n.get("role") in roles and (n.get("label") or "").strip() == label), None)


def starts(tree, roles, prefix):
    return next((n for n in tree if n.get("role") in roles and (n.get("label") or "").startswith(prefix)), None)


def region(tree, label):
    return next((n for n in tree if n.get("subrole") == "AXLandmarkRegion" and (n.get("label") or "") == label), None)


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
    c28.set_window_size(pid, w, h + TITLE_BAR)
    time.sleep(0.8)
    frame = c29.window_frame(pid)
    web = next((n for n in app.tree() if n.get("role") == "AXWebArea" and n.get("size")), None)
    return {"requested": [w, h], "window": frame, "web": [int(web["size"][0]), int(web["size"][1])] if web else None}


def overflow(app):
    tree = app.tree()
    win = next((n for n in tree if n.get("role") == "AXWindow"), None)
    nodes = [n for n in tree if n.get("pos") and n.get("size") and n["size"][0] > 0
             and n.get("role") in ("AXButton", "AXStaticText", "AXTextField", "AXSearchField", "AXCheckBox", "AXPopUpButton", "AXLink", "AXHeading")]
    right = max(n["pos"][0] + n["size"][0] for n in nodes) if nodes and win else None
    hbars = [n for n in tree if n.get("role") == "AXScrollBar" and (n.get("orientation") == "horizontal" or (n.get("size") and n["size"][0] > n["size"][1]))]
    return {"overflow_px": max(0, right - (win["pos"][0] + win["size"][0])) if right is not None else None, "horizontal_scrollbars": len(hbars)}, tree


def cards(tree):
    """The six card buttons by their accessible name ("<product>: <pill>, <value> <metric>, <sub>")."""
    out = {}
    for n in tree:
        label = n.get("label") or ""
        if n.get("role") == "AXButton":
            for p in PRODUCTS:
                if label.startswith(p + ": ") and p not in out:
                    out[p] = label
    return out


def main():
    try:
        return run()
    finally:
        fx.stop_app()
        fx.clean_fixture_root()


def run():
    global NATIVE
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop")
    if c8.sha256(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c36-acceptance-")
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
    fx.clean_fixture_root()
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
    obs = {"captures": {}}
    app.launch()
    pid = app.pid
    v2s.tenant_on_home(app, endpoint.origin)
    app.wait_for(lambda t: c29.nav(t, "Destinations") is not None, 15)
    geometry = size_window(app, pid, 1440, 900)
    app.menu("Go", "Home")
    c29.heading(app, "Home")
    c29.appearance(app, "Light")
    time.sleep(0.8)

    # ---- N01: the board's regions from one read ------------------------------------
    n1 = {}
    tree = app.tree()
    n1["heading"] = fx.App.node(tree, "AXHeading", "Home") is not None
    status = next((n for n in tree if n.get("subrole") == "AXApplicationStatus"), None)
    n1["sub_before"] = texts(within(tree, status)) if status else ""
    n1["cards_before"] = cards(tree)
    # The read: Refresh sends readTenant; the synthetic plane answers; the Runtime card gains its figure.
    refresh = starts(tree, ("AXButton",), "Refresh")
    n1["refresh"] = refresh is not None
    if refresh:
        app.ax("press", "AXButton", "Refresh")
        app.wait_for(lambda t: starts(t, ("AXButton",), "Reading") is not None or "Read finished" in texts(t), 10)
        _, n1["read_landed"] = app.wait_for(lambda t: "Read finished" in texts(t) and starts(t, ("AXButton",), "Refresh") is not None, 40)
        time.sleep(0.8)
    tree = app.tree()
    obs["tree_home"] = c29.save_tree("home", tree)
    status = next((n for n in tree if n.get("subrole") == "AXApplicationStatus"), None)
    n1["sub"] = texts(within(tree, status)) if status else ""
    n1["sub_ok"] = bool(re.search(r"6 products · Read finished at \d\d:\d\d", n1["sub"]))
    n1["cards"] = cards(tree)
    n1["six_cards"] = len(n1["cards"]) == 6 and all(p in n1["cards"] for p in PRODUCTS)
    n1["pills"] = {p: next((pill for pill in PILLS if f": {pill}," in label), None) for p, label in n1["cards"].items()}
    n1["pills_ok"] = all(n1["pills"].values())
    rt = n1["cards"].get("AI Runtime Security", "")
    m = re.search(r": [A-Za-z ]+, ([\d,]+|—) ([^,]+),", rt)
    n1["runtime_value"] = m.group(1) if m else None
    n1["runtime_metric"] = m.group(2) if m else None
    n1["runtime_figure"] = bool(m and m.group(1) != "—" and "detection events" in (m.group(2) or ""))
    n1["gated"] = {p: ("Not available" in n1["cards"].get(p, "")) for p in PRODUCTS[4:]}
    n1["gated_ok"] = all(n1["gated"].values())
    # The card is one named button: its name ends with "New" for the gated products.
    n1["new_tags"] = sum(1 for p in PRODUCTS[4:] if n1["cards"].get(p, "").endswith("New"))
    explain = any("Each card is one product" in (n.get("label") or n.get("value") or "") for n in tree)
    n1["explain_with_calls"] = explain
    sw = labelled(tree, CONTROLS, "Show the calls")
    if sw:
        app.ax("press", sw["role"], "Show the calls")
        time.sleep(0.8)
        t2 = app.tree()
        explain2 = any("Each card is one product" in (n.get("label") or n.get("value") or "") for n in t2)
        n1["explain_flips"] = explain2 != explain
        app.ax("press", sw["role"], "Show the calls")
        time.sleep(0.6)
    obs["captures"]["home"] = {"light": c29.capture(app, "home-light")}
    c29.appearance(app, "Dark")
    obs["captures"]["home"]["dark"] = c29.capture(app, "home-dark")
    c29.appearance(app, "Light")
    # Needs attention, populated by the read: the rows carry a severity word, the entry's action, Acknowledge and Snooze.
    tree = app.tree()
    att = region(tree, "Needs attention")
    inside = within(tree, att)
    n1["attention_region"] = att is not None
    acks = [n for n in inside if n.get("role") == "AXButton" and (n.get("label") or "").startswith("Acknowledge: ")]
    snoozes = [n for n in inside if n.get("role") == "AXButton" and (n.get("label") or "").startswith("Snooze")]
    n1["attention_rows"] = len(acks)
    n1["attention_text"] = texts(inside)[:400]
    # The row's second line starts with the severity word (I9); the accessibility tree folds the line into one text.
    n1["severity_word"] = any(n.get("role") == "AXStaticText" and (n.get("label") or n.get("value") or "").startswith(("Critical", "Warning", "Info")) for n in inside)
    n1["attention_empty_names_read"] = len(acks) == 0 and ("Read finished" in n1["attention_text"] or "Nothing needs action" in n1["attention_text"] or "read" in n1["attention_text"].lower())
    n1["open_inbox"] = labelled(inside, ("AXButton",), "Open the Inbox") is not None
    obs["n1"] = n1

    # ---- N02 (part 1, while the attention row is populated): keyboard reach, ring, Enter ----
    n2 = {}
    app.ax("focus", "AXHeading", "Home")
    time.sleep(0.3)
    walk = [(((f or {}).get("label")) or "")[:44] for f in app.tab_walk(22)]
    n2["walk"] = walk
    order = []
    # Every card, the attention row's controls and every quick action, in the board's order.
    own_action = next(((n.get("label") or "") for n in inside if n.get("role") == "AXButton" and not (n.get("label") or "").startswith(("Acknowledge", "Snooze", "Open the Inbox"))), None)
    keys = ["Refresh", "AI Runtime Security", "AI Gateway", "AI Red Teaming", "Model and skill scans", "Agentic Identity Security", "Agentic Endpoint Security", "Open the Inbox"] + ([own_action] if own_action else []) + (["Acknowledge: ", "Snooze"] if acks else []) + ["Test a prompt", "Ask the Harness", "Send a request", "Keep learning"]
    for key in keys:
        idx = next((i for i, l in enumerate(walk) if l.startswith(key)), None)
        order.append((key, idx))
    n2["order"] = order
    n2["reach_in_order"] = all(i is not None for _, i in order) and [i for _, i in order] == sorted(i for _, i in order)
    plain = c29.capture(app, "card-ring-plain")
    rt_label = cards(app.tree()).get("AI Runtime Security")
    n2["ring"] = c31.ring_on(app, rt_label, plain, "card-ring") if rt_label else {"diff": 0}
    app.ax("focus", "AXButton", rt_label)
    time.sleep(0.3)
    app.keycode(36)
    _, n2["enter_opens"] = c29.heading(app, "Administration")
    app.menu("Go", "Home")
    c29.heading(app, "Home")
    time.sleep(0.5)

    # ---- N01 (continued): the holds, the quick actions, a card ----------------------------
    # ---- N02 (part 2), still with the attention row populated (attempt-02 R1): the layouts ----
    def row_layout(tree, acks, zoom):
        """The populated row's usability at this zoom (attempt-03 R1/R2): the Acknowledge and Snooze
        controls keep their 32 CSS px, and the row's text keeps at least 200 CSS px of width (so words
        are not split into a narrow column); measured from the tree in points, zoom times CSS px."""
        if not acks:
            return {"ok": False, "reason": "no attention row"}
        att = region(tree, "Needs attention")
        inside = within(tree, att)
        buttons = [n for n in inside if n.get("role") == "AXButton" and (n.get("label") or "").startswith(("Acknowledge: ", "Snooze 24 hours: "))]
        title = next((n for n in inside if n.get("role") == "AXStaticText" and (n.get("label") or n.get("value") or "") == acks[0]["label"].split(": ", 1)[1]), None)
        sizes = [n.get("size") for n in buttons]
        controls_ok = len(buttons) >= 2 and all(s and s[0] >= 32 * zoom - 1 and s[1] >= 32 * zoom - 1 for s in sizes)
        text_ok = bool(title and title.get("size") and title["size"][0] >= 200 * zoom - 2)
        return {"ok": controls_ok and text_ok, "controls": sizes, "title_width": title.get("size") if title else None, "zoom": zoom}
    def row(tree):
        web = next((n for n in tree if n.get("role") == "AXWebArea" and n.get("size")), None)
        scroll = next((n for n in tree if n.get("role") == "AXScrollArea" and n.get("size")), None)
        win = next((n for n in tree if n.get("role") == "AXWindow"), None)
        return {"web": [int(web["size"][0]), int(web["size"][1])] if web else None,
                "viewport": [int(scroll["size"][0]), int(scroll["size"][1])] if scroll else None,
                "window": win.get("size") if win else None}
    layouts = {}
    size_window(app, pid, 1440, 900)
    for theme in ("Light", "Dark"):
        c29.appearance(app, theme)
        o, tree = overflow(app)
        o["attention_populated"] = fx.App.node(tree, "AXButton", acks[0]["label"]) is not None if acks else False
        o["row_layout"] = row_layout(tree, acks, 1)
        g = row(tree)
        o["geometry"] = g
        o["width_1440"] = bool(g["web"] and g["web"][0] == 1440)
        name = f"home-{g['web'][0]}x{g['web'][1]}-{theme.lower()}" if g["web"] else f"home-unknown-{theme.lower()}"
        o["capture"] = c29.capture(app, name)
        o["tree"] = c29.save_tree(name, tree)
        layouts[name] = o
    size_window(app, pid, 900, 600)
    c29.zoom(app, 10)
    for theme in ("Light", "Dark"):
        c29.appearance(app, theme)
        # The populated attention row is measured and captured too: scrolled into view by focusing its Acknowledge control.
        if acks:
            app.ax("focus", "AXButton", acks[0]["label"])
            time.sleep(0.4)
        o, tree = overflow(app)
        o["attention_populated"] = fx.App.node(tree, "AXButton", acks[0]["label"]) is not None if acks else False
        o["row_layout"] = row_layout(tree, acks, 2)
        g = row(tree)
        o["geometry"] = g
        # At 200% the document's accessibility area can exceed the viewport by its scroll extent:
        # the viewport (scroll area, else the window's content) is what must be 900×600.
        vp = g["viewport"] or g["web"]
        o["window_900x600"] = bool(vp and vp[0] == 900 and vp[1] == 600 and g["web"] and g["web"][0] == 900)
        name = f"home-{vp[0]}x{vp[1]}-200-{theme.lower()}" if vp else f"home-unknown-200-{theme.lower()}"
        o["capture"] = c29.capture(app, name)
        o["tree"] = c29.save_tree(name, tree)
        layouts[name] = o
    c29.zoom(app, -10)
    c29.appearance(app, "Match System")
    size_window(app, pid, 1440, 900)
    n2["layouts"] = layouts

    if acks:
        title = acks[0]["label"].split(": ", 1)[1]
        n1["attention_item"] = title
        # Snooze first: the core's return time is announced and the row leaves the list.
        app.ax("press", "AXButton", snoozes[0]["label"])
        _, n1["snooze_toast"] = app.wait_for(lambda t: any("snoozed" in (n.get("label") or n.get("value") or "").lower() and "returns" in (n.get("label") or n.get("value") or "").lower() for n in t), 10)
        _, n1["snooze_removed_row"] = app.wait_for(lambda t: fx.App.node(t, "AXButton", acks[0]["label"]) is None, 10)
        # Unsnooze it in the Inbox (C30), so Acknowledge can be exercised on Home.
        app.menu("Go", "Inbox")
        c29.heading(app, "Inbox")
        time.sleep(0.6)
        # The Inbox's tabs expose as radio buttons (or tabs) whose names carry their counts ("Snoozed 1").
        snoozed_tab = next((n for n in app.tree() if n.get("role") in ("AXRadioButton", "AXTab", "AXButton") and (n.get("label") or "").startswith("Snoozed")), None)
        if snoozed_tab:
            app.ax("press", snoozed_tab["role"], snoozed_tab["label"])
        n1["snoozed_tab"] = snoozed_tab is not None
        time.sleep(0.5)
        # The Inbox row's name starts with its severity word (I9); the title is inside it.
        row = next((n for n in app.tree() if n.get("role") == "AXButton" and title in (n.get("label") or "") and not (n.get("label") or "").startswith(("Acknowledge", "Snooze", "Unsnooze"))), None)
        n1["snoozed_row_listed"] = row is not None
        if row:
            app.ax("press", "AXButton", row["label"][:60])
            time.sleep(0.5)
            unsnooze = fx.App.node(app.tree(), "AXButton", "Unsnooze")
            n1["unsnooze_offered"] = unsnooze is not None
            if unsnooze:
                app.ax("press", "AXButton", "Unsnooze")
                time.sleep(0.6)
        app.menu("Go", "Home")
        c29.heading(app, "Home")
        _, n1["row_back"] = app.wait_for(lambda t: fx.App.node(t, "AXButton", acks[0]["label"]) is not None, 10)
        # Acknowledge: the held entry leaves Needs attention (the core's word, read back).
        if n1["row_back"]:
            app.ax("press", "AXButton", acks[0]["label"])
            _, n1["ack_applied"] = app.wait_for(lambda t: fx.App.node(t, "AXButton", acks[0]["label"]) is None, 10)
        else:
            n1["ack_applied"] = False
        time.sleep(0.5)
    tree = app.tree()
    qa = region(tree, "Quick actions")
    qinside = within(tree, qa)
    n1["quick"] = [n.get("label") for n in qinside if n.get("role") == "AXButton"]
    n1["quick_ok"] = len(n1["quick"]) == 4 and all(any(l and l.startswith(k) for l in n1["quick"]) for k in ("Test a prompt", "Ask the Harness", "Send a request", "Keep learning"))
    n1["quick_destinations"] = {}
    if n1["quick_ok"]:
        for key, heading, extra in (("Test a prompt", "Administration", "Playground"), ("Ask the Harness", "Harness", None), ("Send a request", "API Client", None), ("Keep learning", "Academy", None)):
            app.ax("press", "AXButton", next(l for l in n1["quick"] if l.startswith(key)))
            # The destination is the shell header's title (the catalogue's label) or the page's own heading:
            # Academy, the legacy screen until C42, heads its sign-in page "Sign in to continue".
            def arrived(t, heading=heading):
                web = next((n for n in t if n.get("role") == "AXWebArea" and n.get("pos")), None)
                in_header = lambda n: web is not None and n.get("pos") is not None and 0 <= n["pos"][1] - web["pos"][1] <= 56  # noqa: E731
                return any(n.get("role") == "AXHeading" and heading in (n.get("label") or "") for n in t) or any(
                    n.get("role") == "AXStaticText" and (n.get("label") or n.get("value") or "") == heading and in_header(n) for n in t)
            _, opened = app.wait_for(arrived, 10)
            ok = opened
            if opened and extra:
                time.sleep(0.8)
                t = app.tree()
                tab = next((n for n in t if (n.get("label") or "") == extra and n.get("role") in ("AXTab", "AXRadioButton", "AXButton")), None)
                ok = bool(tab and str(tab.get("value", "")).lower() in ("1", "true", "selected") or (tab and tab.get("selected")))
                if not ok:
                    # The selected tab exposes its panel: the Playground's own controls are on screen.
                    ok = any((n.get("label") or "").startswith(("Prompt", "Scan")) for n in t) and tab is not None
                obs["tree_playground"] = c29.save_tree("playground-from-quick-action", t)
            n1["quick_destinations"][key] = ok
            app.menu("Go", "Home")
            c29.heading(app, "Home")
            time.sleep(0.4)
    n1["quick_all"] = n1["quick_ok"] and all(n1["quick_destinations"].values()) and len(n1["quick_destinations"]) == 4
    gw = n1["cards"].get("AI Gateway")
    if gw:
        app.ax("press", "AXButton", gw)
        _, n1["card_opened_admin"] = c29.heading(app, "Administration")
        time.sleep(0.8)
        t = app.tree()
        n1["gateway_expanded"] = any(n.get("role") == "AXButton" and (n.get("label") or "").startswith("AI Gateway") and str(n.get("expanded", "")).lower() in ("1", "true") for n in t) or any((n.get("label") or "") == "Workspaces" for n in t)
        obs["tree_admin"] = c29.save_tree("admin-from-card", t)
        app.menu("Go", "Home")
        c29.heading(app, "Home")
    obs["n1"] = n1

    n2["geometry"] = geometry
    obs["captures"]["layouts"] = {k: v["capture"] for k, v in layouts.items()}
    obs["n2"] = n2
    app.quit()

    run_path = os.path.join(NATIVE, "observations.json")
    with open(run_path, "w") as f:
        json.dump(json.loads(json.dumps({"observations": obs, "log": LOG}).replace(rel(NATIVE), rel(FINAL))), f, indent=1)
    run_rel = rel(run_path)
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C36"]}
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [e for e in evidence if e]})

    caps = lambda name: [v for v in obs["captures"].get(name, {}).values() if v]  # noqa: E731
    holds_ok = n1["attention_rows"] > 0 and n1.get("snooze_toast") and n1.get("snooze_removed_row") and n1.get("row_back") and n1.get("ack_applied") and n1["severity_word"]
    attention_ok = n1["attention_region"] and n1["open_inbox"] and (holds_ok or (n1["attention_rows"] == 0 and n1["attention_empty_names_read"]))
    n1_ok = (n1["heading"] and n1["refresh"] and n1.get("read_landed") and n1["sub_ok"] and n1["six_cards"] and n1["pills_ok"] and n1["runtime_figure"]
             and n1["gated_ok"] and n1["new_tags"] >= 2 and n1.get("explain_flips") and attention_ok and n1["quick_all"]
             and n1.get("card_opened_admin") and n1.get("gateway_expanded"))
    add("C36-N01", bool(n1_ok),
        f"heading {n1['heading']}; Refresh {n1['refresh']} read landed {n1.get('read_landed')}; summary '{n1['sub']}' ({n1['sub_ok']}); six cards {n1['six_cards']} with pills {n1['pills']}; Runtime figure {n1['runtime_value']} {n1['runtime_metric']} ({n1['runtime_figure']}); gated {n1['gated']} with {n1['new_tags']} New tags in their names; Explain banner shown at first {n1['explain_with_calls']} (the calls start hidden once a tenant exists) and flipping with the Explain switch {n1.get('explain_flips')}; Needs attention: {n1['attention_rows']} row(s) with a severity word {n1['severity_word']}; Snooze announced the return time {n1.get('snooze_toast')} and removed the row {n1.get('snooze_removed_row')}, Unsnooze in the Inbox brought it back {n1.get('row_back')}, Acknowledge removed it {n1.get('ack_applied')}; Open the Inbox {n1['open_inbox']}; quick actions {n1['quick']} opened their destinations {n1.get('quick_destinations')}; the Gateway card opened Administration {n1.get('card_opened_admin')} with the product expanded {n1.get('gateway_expanded')}",
        [run_rel, obs["tree_home"], obs.get("tree_admin"), obs.get("tree_playground")] + caps("home"))
    layout_ok = all(v["overflow_px"] == 0 and v["horizontal_scrollbars"] == 0 for v in layouts.values())
    geometry_ok = all(v.get("width_1440", v.get("window_900x600", False)) for v in layouts.values())
    populated_ok = all(v.get("attention_populated") for v in layouts.values())
    rows_ok = all((v.get("row_layout") or {}).get("ok") for v in layouts.values())
    rows_text = ", ".join(f"{k}: controls {(v.get('row_layout') or {}).get('controls')} title width {(v.get('row_layout') or {}).get('title_width')}" for k, v in layouts.items())
    sizes_text = ", ".join(f"{k}: web {v['geometry']['web']} viewport {v['geometry']['viewport']}" for k, v in layouts.items())
    overflow_text = ", ".join(f"{k}: {v['overflow_px']}" for k, v in layouts.items())
    ring_ok = (n2["ring"].get("diff") or 0) > 40
    add("C36-N02", bool(n2["reach_in_order"] and ring_ok and n2.get("enter_opens") and layout_ok and geometry_ok and populated_ok and rows_ok),
        f"Tab from the heading, with the attention row populated, reached {n2['order']} in order {n2['reach_in_order']}; ring on the Runtime card {n2['ring'].get('diff')} px ({ring_ok}); Enter on the card opened Administration {n2.get('enter_opens')}; each row's geometry from its own saved tree: {sizes_text}; attention row populated in every layout tree {populated_ok}, with 32px controls and at least 200px of text at each zoom {rows_ok} ({rows_text}) (asserted {geometry_ok}); overflow px {overflow_text}, horizontal scroll bars {[v['horizontal_scrollbars'] for v in layouts.values()]}",
        [run_rel, n2["ring"].get("capture"), n2["ring"].get("plain")] + [v["capture"] for v in layouts.values()] + [v.get("tree") for v in layouts.values()])

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1", "subject": "C36", "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (System Events and the ax helper) with fixture WebView self-capture; tools/native-acceptance/acceptance_c36.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "Pixels come from the fixture build's own WKWebView snapshot; the appearance is selected before each capture.",
            "Scan and management endpoints are synthetic services on this Mac (scan_server.py, mgmt_server.py): the read's figures are the synthetic plane's.",
            "Window geometry is measured from the accessibility tree, never assumed; this Mac's display gives a 1440-wide WebView of about 800 CSS px.",
            "Visual parity with the prototype is the owner's judgement from the composites under evidence/C36/parity, not a check here.",
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
        print(c["id"], c["result"], "-", c["observation"][:200])
    return 0 if all(c["result"] == "pass" for c in checks) else 1


if __name__ == "__main__":
    sys.exit(main())
