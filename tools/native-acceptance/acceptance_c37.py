#!/usr/bin/env python3
"""C37 native acceptance: the Inbox as the prototype draws it (R2App
start=inbox), driven through the real fixture window against the synthetic
scan and management endpoints.

Run from the repository root after `xtask native-prepare --chunk C37`.
Requires the macOS Accessibility grant. Synthetic services only; the fixture
data root is removed at the end.

Checks (docs/architecture/native-checks.json, C37): the list card with its
filter tabs and counts, rows with severity word, title, product, source and
when, the footer's coverage line, the detail card filled from a row (severity,
title, body, facts, the entry's own action, Acknowledge, Snooze), the empty
states, and the C30 holds unchanged (N01); keyboard reach, the focus ring,
Enter on a row, and the measured layouts in both themes (N02).
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
import acceptance_c31 as c31  # noqa: E402  (focus ring helper)

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C37")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C37" else "native-C37")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C37" else "native-acceptance-C37.json")
LOG = []
TITLE_BAR = 32
TABS = ("All", "Needs action", "Updates", "Snoozed")
CONTROLS = ("AXButton", "AXCheckBox", "AXPopUpButton", "AXMenuButton", "AXSwitch", "AXToggle", "AXRadioButton", "AXTab")


def rel(path):
    return os.path.relpath(path, ROOT)


def labelled(tree, roles, label):
    return next((n for n in tree if n.get("role") in roles and (n.get("label") or "").strip() == label), None)


def starts(tree, roles, prefix):
    return next((n for n in tree if n.get("role") in roles and (n.get("label") or "").startswith(prefix)), None)


def region(tree, label):
    return next((n for n in tree if n.get("subrole") in ("AXLandmarkRegion", "AXLandmarkComplementary") and (n.get("label") or "") == label), None)


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


def tab_node(tree, label):
    return next((n for n in tree if n.get("role") in ("AXRadioButton", "AXTab", "AXButton") and (n.get("label") or "").startswith(label)), None)


def rows_of(tree, list_node):
    """The list's row buttons: every button in the list card that is not a tab."""
    return [n for n in within(tree, list_node) if n.get("role") == "AXButton" and not (n.get("label") or "").split(" ")[0] in ("All", "Needs", "Updates", "Snoozed", "Open")]


def size_window(app, pid, w, h):
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {pid}) to set position of window 1 to {{0, 0}}'])
    c28.set_window_size(pid, w, h + TITLE_BAR)
    time.sleep(0.8)
    return c29.window_frame(pid)


def geometry_of(tree):
    web = next((n for n in tree if n.get("role") == "AXWebArea" and n.get("size")), None)
    scroll = next((n for n in tree if n.get("role") == "AXScrollArea" and n.get("size")), None)
    return {"web": [int(web["size"][0]), int(web["size"][1])] if web else None,
            "viewport": [int(scroll["size"][0]), int(scroll["size"][1])] if scroll else None}


def overflow(app):
    tree = app.tree()
    win = next((n for n in tree if n.get("role") == "AXWindow"), None)
    nodes = [n for n in tree if n.get("pos") and n.get("size") and n["size"][0] > 0
             and n.get("role") in ("AXButton", "AXStaticText", "AXTextField", "AXSearchField", "AXCheckBox", "AXPopUpButton", "AXLink", "AXHeading", "AXRadioButton")]
    right = max(n["pos"][0] + n["size"][0] for n in nodes) if nodes and win else None
    hbars = [n for n in tree if n.get("role") == "AXScrollBar" and (n.get("orientation") == "horizontal" or (n.get("size") and n["size"][0] > n["size"][1]))]
    return {"overflow_px": max(0, right - (win["pos"][0] + win["size"][0])) if right is not None else None, "horizontal_scrollbars": len(hbars)}, tree


def main():
    try:
        return run()
    finally:
        fx.sh(["pkill", "-f", fx.PROCESS])
        fx.clean_fixture_root()


def run():
    global NATIVE
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")
    if c8.sha256(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c37-acceptance-")
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
    app.open_settings()
    app.type_into("AXTextField", "TSG ID (optional)", c8.TSG)
    app.create_custom_tenant("Northwind Health", endpoint.origin)
    app.save_key(c8.SCAN_KEY)
    c8.save_client(app)
    c29.press(app, "AXCheckBox", "Use the new interface")
    app.wait_for(lambda t: c29.nav(t, "Destinations") is not None, 15)
    size_window(app, pid, 1440, 900)
    app.menu("Go", "Inbox")
    c29.heading(app, "Inbox")
    c29.appearance(app, "Light")
    time.sleep(0.6)

    # ---- N01: the list card, the detail card, the empty states and the holds ------------
    n1 = {}
    tree = app.tree()
    n1["heading"] = fx.App.node(tree, "AXHeading", "Inbox") is not None
    n1["sub_before"] = texts([n for n in tree if n.get("subrole") == "AXApplicationStatus"] and within(tree, next(n for n in tree if n.get("subrole") == "AXApplicationStatus")))
    n1["empty_before"] = "First read pending" in texts(tree)
    obs["captures"]["inbox-empty"] = {"light": c29.capture(app, "inbox-empty-light")}
    # The read lands; rows appear newest first.
    c29.press(app, "AXButton", "Read now")
    _, n1["read_landed"] = app.wait_for(lambda t: app.has_text(t, "sources read at") or app.has_text(t, "All sources read"), 40)
    time.sleep(0.8)
    tree = app.tree()
    obs["tree_inbox"] = c29.save_tree("inbox", tree)
    status = next((n for n in tree if n.get("subrole") == "AXApplicationStatus"), None)
    n1["sub"] = texts(within(tree, status)) if status else ""
    n1["sub_ok"] = "Everything that needs you in Northwind Health · newest first" in n1["sub"]
    n1["tabs"] = {t: (tab_node(tree, t) or {}).get("label") for t in TABS}
    n1["tabs_ok"] = all(n1["tabs"].values()) and all(re.search(r"\d+$", (l or "")) for l in n1["tabs"].values())
    list_node = region(tree, "Inbox items")
    n1["list_region"] = list_node is not None
    rows = rows_of(tree, list_node)
    n1["rows"] = [r.get("label") for r in rows]
    n1["rows_ok"] = len(rows) >= 1
    n1["severity_words"] = all(any(w in (r.get("label") or "") for w in ("Critical", "Warning", "Info")) for r in rows)
    foot = next(((n.get("label") or n.get("value") or "") for n in within(tree, list_node) if n.get("role") == "AXStaticText" and ("sources read at" in (n.get("label") or n.get("value") or "") or "No source reads" in (n.get("label") or n.get("value") or ""))), None)
    n1["footer"] = foot
    # Select the first row: the detail card fills.
    app.ax("press", "AXButton", rows[0]["label"][:60]) if rows else None
    time.sleep(0.6)
    tree = app.tree()
    detail = region(tree, "Details")
    inside = within(tree, detail)
    n1["detail_region"] = detail is not None
    dtext = texts(inside)
    title = (rows[0]["label"] or "").split("\n")[0] if rows else ""
    n1["detail_title"] = any(n.get("role") == "AXHeading" and (n.get("label") or "") and (n.get("label") or "") in (rows[0]["label"] or "") for n in inside) if rows else False
    n1["detail_severity"] = any(w in dtext for w in ("Critical", "Warning", "Info"))
    facts = next((n for n in inside if (n.get("label") or "") == "Facts" and n.get("role") in ("AXGroup", "AXList")), None)
    n1["facts"] = facts is not None and len([n for n in within(tree, facts) if n.get("role") == "AXStaticText"]) >= 2
    n1["detail_actions"] = {k: starts(inside, ("AXButton",), k) is not None for k in ("Acknowledge", "Snooze 24 hours")}
    n1["detail_action_own"] = any(n.get("role") == "AXButton" and not (n.get("label") or "").startswith(("Acknowledge", "Snooze", "Unsnooze", "Mark as")) for n in inside)
    obs["tree_detail"] = c29.save_tree("inbox-detail", tree)
    obs["captures"]["inbox"] = {"light": c29.capture(app, "inbox-light")}
    c29.appearance(app, "Dark")
    obs["captures"]["inbox"]["dark"] = c29.capture(app, "inbox-dark")
    c29.appearance(app, "Light")
    # The C30 holds, from the detail card: snooze (the core's time in the row, the detail and the toast), unsnooze, acknowledge.
    snooze = starts(inside, ("AXButton",), "Snooze 24 hours")
    if snooze:
        app.ax("press", "AXButton", snooze["label"])
        tree, toasted = app.wait_for(lambda t: any("snoozed · returns" in x for x in app.texts(t)), 10)
        toast_text = next((x for x in app.texts(tree) if "snoozed · returns" in x), "")
        n1["snooze_toast"] = toasted
        returns_toast = toast_text.split("returns ", 1)[1].rstrip("×").strip() if "returns " in toast_text else None
        tab = tab_node(app.tree(), "Snoozed")
        app.ax("press", tab["role"], tab["label"]) if tab else None
        time.sleep(0.6)
        tree = app.tree()
        row = next((n for n in rows_of(tree, region(tree, "Inbox items")) if "Returns " in (n.get("label") or "")), None)
        row_returns = row["label"].split("Returns ", 1)[1].strip() if row and "Returns " in row["label"] else None
        app.ax("press", "AXButton", row["label"][:60]) if row else None
        time.sleep(0.5)
        tree = app.tree()
        pane_returns = next(((n.get("label") or "")[len("Returns "):] for n in within(tree, region(tree, "Details")) if n.get("role") == "AXStaticText" and (n.get("label") or "").startswith("Returns ")), None)
        n1["snooze_times"] = {"toast": returns_toast, "row": row_returns, "pane": pane_returns}
        n1["snooze_same"] = bool(returns_toast) and returns_toast == row_returns == pane_returns
        c29.press(app, "AXButton", "Unsnooze")
        _, n1["unsnoozed"] = app.wait_for(lambda t: starts(t, ("AXButton",), "Snooze 24 hours") is not None, 8)
        tab = tab_node(app.tree(), "All")
        app.ax("press", tab["role"], tab["label"]) if tab else None
        time.sleep(0.5)
        c29.press(app, "AXButton", "Acknowledge")
        _, n1["acknowledged"] = app.wait_for(lambda t: app.has_text(t, "Acknowledged") and fx.App.node(t, "AXButton", "Mark as needing action") is not None, 8)
        c29.press(app, "AXButton", "Mark as needing action")
        _, n1["unmarked"] = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Acknowledge") is not None, 8)
    obs["n1"] = n1

    # ---- N02: keyboard, ring, Enter on a row, layouts ------------------------------------
    n2 = {}
    app.ax("focus", "AXHeading", "Inbox")
    time.sleep(0.3)
    walk = [(((f or {}).get("label")) or "")[:44] for f in app.tab_walk(14)]
    n2["walk"] = walk
    order = []
    for key in ("Read now", "All", rows[0]["label"][:12] if rows else "", "Acknowledge"):
        idx = next((i for i, l in enumerate(walk) if key and l.startswith(key)), None)
        order.append((key, idx))
    n2["order"] = order
    n2["reach_in_order"] = all(i is not None for _, i in order) and [i for _, i in order] == sorted(i for _, i in order)
    plain = c29.capture(app, "row-ring-plain")
    n2["ring"] = c31.ring_on(app, rows[0]["label"][:60], plain, "row-ring") if rows else {"diff": 0}
    ring_ok = (n2["ring"].get("diff") or 0) > 40
    if rows:
        app.ax("focus", "AXButton", rows[0]["label"][:60])
        time.sleep(0.3)
        app.keycode(36)
        _, n2["enter_opens"] = app.wait_for(lambda t: region(t, "Details") is not None and any(n.get("role") == "AXHeading" and (n.get("label") or "") in (rows[0]["label"] or "") for n in within(t, region(t, "Details"))), 8)
    layouts = {}
    size_window(app, pid, 1440, 900)
    for theme in ("Light", "Dark"):
        c29.appearance(app, theme)
        o, tree = overflow(app)
        g = geometry_of(tree)
        o["geometry"] = g
        o["width_1440"] = bool(g["web"] and g["web"][0] == 1440)
        name = f"inbox-{g['web'][0]}x{g['web'][1]}-{theme.lower()}"
        o["capture"] = c29.capture(app, name)
        o["tree"] = c29.save_tree(name, tree)
        layouts[name] = o
    size_window(app, pid, 900, 600)
    c29.zoom(app, 10)
    for theme in ("Light", "Dark"):
        c29.appearance(app, theme)
        o, tree = overflow(app)
        g = geometry_of(tree)
        vp = g["viewport"] or g["web"]
        o["geometry"] = g
        o["window_900x600"] = bool(vp and vp[0] == 900 and vp[1] == 600)
        name = f"inbox-{vp[0]}x{vp[1]}-200-{theme.lower()}"
        o["capture"] = c29.capture(app, name)
        o["tree"] = c29.save_tree(name, tree)
        layouts[name] = o
    c29.zoom(app, -10)
    c29.appearance(app, "Match System")
    size_window(app, pid, 1440, 900)
    n2["layouts"] = layouts
    obs["captures"]["layouts"] = {k: v["capture"] for k, v in layouts.items()}
    obs["n2"] = n2
    app.quit()

    run_path = os.path.join(NATIVE, "observations.json")
    with open(run_path, "w") as f:
        json.dump(json.loads(json.dumps({"observations": obs, "log": LOG}).replace(rel(NATIVE), rel(FINAL))), f, indent=1)
    run_rel = rel(run_path)
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C37"]}
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail", "observation": observation, "evidence": [e for e in evidence if e]})

    caps = lambda name: [v for v in obs["captures"].get(name, {}).values() if v]  # noqa: E731
    holds_ok = n1.get("snooze_toast") and n1.get("snooze_same") and n1.get("unsnoozed") and n1.get("acknowledged") and n1.get("unmarked")
    n1_ok = (n1["heading"] and n1["empty_before"] and n1.get("read_landed") and n1["sub_ok"] and n1["tabs_ok"] and n1["list_region"] and n1["rows_ok"] and n1["severity_words"]
             and bool(n1["footer"]) and n1["detail_region"] and n1["detail_title"] and n1["detail_severity"] and n1["facts"] and all(n1["detail_actions"].values()) and n1["detail_action_own"] and holds_ok)
    add("C37-N01", n1_ok,
        f"heading {n1['heading']}; empty state before any read named it {n1['empty_before']}; Read now landed {n1.get('read_landed')}; line '{n1['sub']}' ({n1['sub_ok']}); tabs {n1['tabs']} ({n1['tabs_ok']}); rows {n1['rows']} with severity words {n1['severity_words']}; footer '{n1['footer']}'; detail: title {n1['detail_title']}, severity {n1['detail_severity']}, facts grid {n1['facts']}, own action {n1['detail_action_own']}, actions {n1['detail_actions']}; holds: snooze toast {n1.get('snooze_toast')} times {n1.get('snooze_times')} same {n1.get('snooze_same')}, unsnoozed {n1.get('unsnoozed')}, acknowledged {n1.get('acknowledged')}, unmarked {n1.get('unmarked')}",
        [run_rel, obs["tree_inbox"], obs["tree_detail"]] + caps("inbox-empty") + caps("inbox"))
    layout_ok = all(v["overflow_px"] == 0 and v["horizontal_scrollbars"] == 0 for v in layouts.values())
    geometry_ok = all(v.get("width_1440", v.get("window_900x600", False)) for v in layouts.values())
    sizes_text = ", ".join(f"{k}: web {v['geometry']['web']} viewport {v['geometry']['viewport']}" for k, v in layouts.items())
    overflow_text = ", ".join(f"{k}: {v['overflow_px']}" for k, v in layouts.items())
    add("C37-N02", n2["reach_in_order"] and ring_ok and n2.get("enter_opens") and layout_ok and geometry_ok,
        f"Tab from the heading reached {n2['order']} in order {n2['reach_in_order']}; ring on the first row {n2['ring'].get('diff')} px ({ring_ok}); Enter on the row opened its detail {n2.get('enter_opens')}; measured sizes by row {sizes_text} (asserted {geometry_ok}); overflow px {overflow_text}, horizontal scroll bars {[v['horizontal_scrollbars'] for v in layouts.values()]}",
        [run_rel, n2["ring"].get("capture"), n2["ring"].get("plain")] + [v["capture"] for v in layouts.values()] + [v.get("tree") for v in layouts.values()])

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1", "subject": "C37", "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (System Events and the ax helper) with fixture WebView self-capture; tools/native-acceptance/acceptance_c37.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "Pixels come from the fixture build's own WKWebView snapshot; the appearance is selected before each capture.",
            "Scan and management endpoints are synthetic services on this Mac (scan_server.py, mgmt_server.py): the read's entries are the synthetic plane's.",
            "Window geometry is measured from each row's own accessibility tree (WebView area and viewport), never assumed.",
            "Visual parity with the prototype is the owner's judgement from the composites under evidence/C37/parity, not a check here.",
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
