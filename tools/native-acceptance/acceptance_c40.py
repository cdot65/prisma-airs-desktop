#!/usr/bin/env python3
"""C40 native acceptance (D059): Test Cases as the prototype board draws it
(R2App start=cases), driven through the real fixture window against the
synthetic scan and management endpoints.

Run from the repository root after `xtask native-prepare --chunk C40`.
Requires the macOS Accessibility grant. Synthetic services only; the fixture
data root is removed at the end.

Checks (docs/architecture/native-checks.json, C40): the board (title row,
Load starter packs… dialog, New case dialog, the table card with search and
product pills, the result pills, Inspect's detail sheet, the ⋯ menu's Remove
and Copy, Open in Playground and the scan run there recording the result,
Import… and Export… through the native sheets) (N01); keyboard reach, the
focus ring and the measured layouts in both themes (N02). The parity
composites are captured from the populated list after the starter packs
are loaded.
"""

import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fixture_app as fx  # noqa: E402
import acceptance_c08 as c8  # noqa: E402
import acceptance_c28 as c28  # noqa: E402
import acceptance_c29 as c29  # noqa: E402
import acceptance_c31 as c31  # noqa: E402
import native_sheets as sheets  # noqa: E402

ROOT = os.getcwd()
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence/C40")
NATIVE = os.path.join(EVIDENCE, "native")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json")
PARITY = os.path.join(EVIDENCE, "parity")
DC = os.path.join(ROOT, "tools/parity")
LOG = []
TITLE_BAR = 28
RED_TEAM_RUNS = "AI Red Teaming: hand the spec to a scan."
PILLS = ("AXCheckBox", "AXButton", "AXRadioButton")
BTN = ("AXButton", "AXPopUpButton")


def ring_any(app, label, plain_cap, name):
    """c31.ring_on for a control whatever its AX role (New case opens a menu: an AXPopUpButton)."""
    node = next((n for n in app.tree() if (n.get("label") or "") == label and n.get("role") in BTN), None)
    app.ax("focus", node["role"] if node else "AXButton", label)
    time.sleep(0.5)
    focused_cap = c29.capture(app, name)
    snap = app.tree()
    web = next((n for n in snap if n.get("role") == "AXWebArea"), None)
    btn = next((n for n in snap if (n.get("label") or "") == label and n.get("role") in BTN), None)
    if not (web and btn and plain_cap and focused_cap):
        return {"diff": 0, "focused": (fx.App.focused(snap) or {}).get("label")}
    img_f = c31.c1.png_pixels(os.path.join(ROOT, focused_cap))
    img_p = c31.c1.png_pixels(os.path.join(ROOT, plain_cap))
    scale = img_f[0] / web["size"][0]
    box = [btn["pos"][0] - web["pos"][0], btn["pos"][1] - web["pos"][1], btn["size"][0], btn["size"][1]]
    return {"diff": c28.ring_diff(img_f, img_p, box, scale), "focused": (fx.App.focused(snap) or {}).get("label"), "capture": focused_cap, "plain": plain_cap}


def press_any(app, label, timeout=8):
    """Presses the control with this exact label whatever role AX gives it (a button with aria-haspopup is an AXPopUpButton)."""
    tree, _ = app.wait_for(lambda t: any((n.get("label") or "") == label and n.get("role") in BTN for n in t), timeout)
    node = next((n for n in tree if (n.get("label") or "") == label and n.get("role") in BTN), None)
    if node is None:
        raise RuntimeError(f"no button labelled {label}")
    app.ax("press", node["role"], label)
    time.sleep(0.5)



def rel(path):
    return os.path.relpath(path, ROOT)


def labels(tree, roles, prefix):
    roles = (roles,) if isinstance(roles, str) else roles
    out = []
    for n in tree:
        label = n.get("label") or ""
        if n.get("role") in roles and label.startswith(prefix) and label not in out:
            out.append(label)
    return out


def dialog(tree, label):
    return next((n for n in tree if (n.get("label") or "") == label and n.get("subrole") in ("AXDialog", "AXApplicationDialog")), None)


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


def frame(n):
    return (n["pos"][0], n["pos"][1], n["pos"][0] + n["size"][0], n["pos"][1] + n["size"][1]) if n and n.get("pos") and n.get("size") else None


def contained(n, outer, slack=1):
    a, b = frame(n), frame(outer)
    return bool(a and b) and a[0] >= b[0] - slack and a[1] >= b[1] - slack and a[2] <= b[2] + slack and a[3] <= b[3] + slack


def geometry(tree):
    web = next((n for n in tree if n.get("role") == "AXWebArea" and n.get("size")), None)
    scroll = next((n for n in tree if n.get("role") == "AXScrollArea" and n.get("size")), None)
    win = next((n for n in tree if n.get("role") == "AXWindow"), None)
    return {"web": [int(web["size"][0]), int(web["size"][1])] if web else None,
            "viewport": [int(scroll["size"][0]), int(scroll["size"][1])] if scroll else None,
            "window": win.get("size") if win else None}


def size_window(app, pid, w, h):
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {pid}) to set position of window 1 to {{0, 0}}'])
    requested = [w, h + TITLE_BAR]
    for _ in range(3):
        c28.set_window_size(pid, requested[0], requested[1])
        time.sleep(0.8)
        g = geometry(app.tree())
        vp = g["viewport"] or g["web"]
        if not vp or vp[1] == h:
            break
        requested = [requested[0], requested[1] + (h - vp[1])]
    return c29.window_frame(pid)


def overflow(app):
    tree = app.tree()
    win = next((n for n in tree if n.get("role") == "AXWindow"), None)
    nodes = [n for n in tree if n.get("pos") and n.get("size") and n["size"][0] > 0
             and n.get("role") in ("AXButton", "AXStaticText", "AXTextField", "AXTextArea", "AXSearchField", "AXCheckBox", "AXPopUpButton", "AXLink", "AXHeading", "AXRadioButton")]
    right = max(n["pos"][0] + n["size"][0] for n in nodes) if nodes and win else None
    hbars = [n for n in tree if n.get("role") == "AXScrollBar" and (n.get("orientation") == "horizontal" or (n.get("size") and n["size"][0] > n["size"][1]))]
    return {"overflow_px": max(0, right - (win["pos"][0] + win["size"][0])) if right is not None else None, "horizontal_scrollbars": len(hbars)}, tree


def table_usable(tree, zoom):
    """The cases table at this zoom: the header row, the first row's action and the footer inside the card, the
    row's action control at least 32 CSS px."""
    table = next((n for n in tree if n.get("role") == "AXTable" and (n.get("label") or "") == "Test Cases"), None)
    card = next((n for n in tree if n.get("role") == "AXGroup" and n.get("pos") and n.get("size") and table and contained(table, n) and n["size"][1] > table["size"][1] - 1 and n["size"][0] >= table["size"][0] - 1), None)
    action = next((n for n in tree if n.get("role") in BTN and (n.get("label") or "").startswith(("Open in Playground:", "Inspect:"))), None)
    foot = next((n for n in tree if n.get("role") == "AXButton" and (n.get("label") or "") == "Next page"), None)
    out = {"table": table is not None, "action_size": action.get("size") if action else None, "action_ok": bool(action and action["size"][0] >= 32 * zoom - 1 and action["size"][1] >= 32 * zoom - 1),
           "foot": foot is not None, "zoom": zoom}
    if table and action:
        out["action_inside"] = contained(action, table) or (card is not None and contained(action, card))
    else:
        out["action_inside"] = False
    out["ok"] = out["table"] and out["action_ok"] and out["foot"] and out["action_inside"]
    return out


def composite(app, theme, web_size, prepared):
    name = f"cases-{theme.lower()}"
    cap = c29.capture(app, f"parity-{name}")
    if not cap:
        raise SystemExit(f"no capture for {name}")
    os.makedirs(os.path.join(PARITY, "app"), exist_ok=True)
    os.makedirs(os.path.join(PARITY, "proto"), exist_ok=True)
    with open(os.path.join(PARITY, "bundle.json"), "w") as f:
        json.dump({"bundle": prepared["bundle"], "executable_sha256": prepared["executable_sha256"], "from": "native-prepare.json",
                   "captured_by": "tools/native-acceptance/acceptance_c40.py", "captured_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}, f, indent=2)
        f.write("\n")
    app_png = os.path.join(PARITY, "app", f"{name}.png")
    shutil.copy(os.path.join(ROOT, cap), app_png)
    web_w, web_h = web_size
    proto_png = os.path.join(PARITY, "proto", f"{name}.png")
    subprocess.run([sys.executable, "-I", os.path.join(DC, "dc_render.py"), "--board", "R2App", "--prop", "start=cases",
                    "--prop", f"theme={theme.lower()}", "--width", str(web_w), "--height", str(web_h), "--out", proto_png], check=True, capture_output=True)
    out = os.path.join(PARITY, f"{name}.png")
    subprocess.run([sys.executable, "-I", os.path.join(DC, "compose.py"), "--left", proto_png, "--right", app_png,
                    "--out", out, "--title", f"Test Cases · {theme.lower()} · {web_w}×{web_h}"], check=True, capture_output=True)
    return rel(out)


def main():
    try:
        return run()
    finally:
        sheets.disable()
        fx.sh(["pkill", "-f", fx.PROCESS])
        fx.clean_fixture_root()


def run():
    global NATIVE
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")
    if c8.sha256(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c40-acceptance-")
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
    time.sleep(1.5)
    endpoint = fx.ScanEndpoint(os.path.join(work, "endpoint"), c8.SCAN_KEY)
    endpoint.install_roots()
    endpoint.start()
    endpoint.mode("block")
    mgmt = c8.MgmtEndpoint(os.path.join(work, "endpoint"), os.path.join(work, "mgmt"))
    mgmt.install()
    mgmt.start()
    mgmt.mode()
    try:
        return exercise(app, endpoint, mgmt, prepared, work, FINAL)
    finally:
        endpoint.stop()
        mgmt.stop()
        shutil.rmtree(work, ignore_errors=True)


def exercise(app, endpoint, mgmt, prepared, work, FINAL):
    global NATIVE
    obs = {"captures": {}}
    sheets.enable()
    app.launch()
    pid = app.pid
    app.open_settings()
    app.type_into("AXTextField", "TSG ID (optional)", c8.TSG)
    app.create_custom_tenant("Northwind Health", endpoint.origin)
    app.save_key(c8.SCAN_KEY)
    c8.save_client(app)
    # A second tenant, the destination of an explicit copy.
    app.type_into("AXTextField", "TSG ID (optional)", c8.TSG)
    app.create_custom_tenant("Tenant B", endpoint.origin)
    app.make_active("Northwind Health")
    c29.press(app, "AXCheckBox", "Use the new interface")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Home") is not None and c29.nav(t, "Destinations") is not None, 15)
    size_window(app, pid, 1440, 900)

    # ---- N01: the board ----
    n1 = {}
    app.menu("Go", "Test Cases")
    tree, n1["screen"] = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Test Cases") is not None, 15)
    n1["empty_line"] = app.has_text(tree, "0 cases in Northwind Health · sealed on this Mac, per tenant")
    n1["empty_state"] = app.has_text(tree, "No cases yet.")
    obs["tree_empty"] = c29.save_tree("cases-empty", tree)
    # Load starter packs… : the dialog with the bundled packs checked; Load selected adds them.
    press_any(app, "Load starter packs…", 15)
    tree, n1["packs_dialog"] = app.wait_for(lambda t: dialog(t, "Load starter packs") is not None, 10)
    inside = within(tree, dialog(tree, "Load starter packs"))
    n1["packs_options"] = [(n.get("label"), n.get("value")) for n in inside if n.get("role") == "AXCheckBox"]
    n1["packs_transfer"] = any((n.get("label") or "") in ("Import…", "Export…") for n in inside)
    obs["tree_packs"] = c29.save_tree("cases-packs", tree)
    obs["captures"]["cases-packs"] = {"light": c29.capture(app, "cases-packs-light")}
    c29.press(app, "AXButton", "Load selected")
    tree, n1["packs"] = app.wait_for(lambda t: dialog(t, "Load starter packs") is None and len(labels(t, "AXButton", "Open in Playground:")) >= 1 and len(labels(t, BTN, "Inspect:")) >= 1, 20)
    n1["pack_named"] = app.has_text(tree, "Runtime Security starter pack")
    n1["not_run"] = app.has_text(tree, "Not run")
    n1["runtime_rows"] = len(labels(tree, "AXButton", "Open in Playground:"))
    n1["spec_rows"] = len(labels(tree, BTN, "Inspect:"))
    n1["count_line"] = next((x for x in app.texts(tree) if re.match(r"\d+ cases in Northwind Health · sealed on this Mac, per tenant", x)), None)
    n1["pills"] = labels(tree, PILLS, "All ") + labels(tree, PILLS, "Runtime Security ") + labels(tree, PILLS, "AI Red Teaming ")
    n1["range"] = next((x for x in app.texts(tree) if re.match(r"1–\d+ of \d+", x)), None)
    obs["tree_cases"] = c29.save_tree("cases", tree)
    obs["captures"]["cases"] = c29.matrix(app, "cases")
    # The parity composites, from this state, in both themes.
    app.ax("focus", "AXHeading", "Test Cases")
    time.sleep(0.3)
    n1["parity"] = {}
    for theme in ("Light", "Dark"):
        c29.appearance(app, theme)
        time.sleep(0.7)
        n1["parity"][theme.lower()] = composite(app, theme, geometry(app.tree())["web"], prepared)
    c29.appearance(app, "Light")
    time.sleep(0.5)
    # Filter by product (the pills carry their counts) and search.
    red = next((l for l in labels(tree, PILLS, "AI Red Teaming ")), None)
    n1["filter_label"] = red
    if red:
        role = next(n["role"] for n in tree if (n.get("label") or "") == red and n.get("role") in PILLS)
        app.ax("press", role, red)
        time.sleep(0.6)
        t2 = app.tree()
        n1["filtered_runtime_rows"] = len(labels(t2, "AXButton", "Open in Playground:"))
        n1["filtered_spec_rows"] = len(labels(t2, BTN, "Inspect:"))
        all_label = next((l for l in labels(t2, PILLS, "All ")), None)
        if all_label:
            app.ax("press", role, all_label)
            time.sleep(0.4)
    app.type_into("AXTextField", "Search cases", "zzz-no-such-case")
    tree, n1["search_empty"] = app.wait_for(lambda t: app.has_text(t, "No case matches.") and fx.App.node(t, "AXButton", "Clear search and filters") is not None, 8)
    c29.press(app, "AXButton", "Clear search and filters")
    app.wait_for(lambda t: len(labels(t, BTN, "Inspect:")) >= 1, 8)
    # Inspect a Red Team spec: the detail sheet (520px, docked right), and no scan request at all.
    scans_before = len(endpoint.requests())
    tree = app.tree()
    red_inspect = None
    for label in labels(tree, BTN, "Inspect:"):
        press_any(app, label)
        t3, opened = app.wait_for(lambda t: dialog(t, "Case details") is not None, 6)
        if opened and app.has_text(t3, RED_TEAM_RUNS):
            red_inspect = label
            tree = t3
            break
        if opened:
            press_any(app, "Close details")
            app.wait_for(lambda t: dialog(t, "Case details") is None, 6)
    sheet = dialog(tree, "Case details")
    web = next((n for n in tree if n.get("role") == "AXWebArea" and n.get("size")), None)
    n1["inspect_sheet"] = sheet is not None
    n1["sheet_docked"] = bool(sheet and web and abs((sheet["pos"][0] + sheet["size"][0]) - (web["pos"][0] + web["size"][0])) <= 2 and abs(sheet["size"][0] - 520) <= 2)
    n1["red_team_runs_from"] = red_inspect is not None and app.has_text(tree, RED_TEAM_RUNS)
    n1["never_run_note"] = app.has_text(tree, "This screen never runs a case")
    obs["tree_inspect"] = c29.save_tree("cases-inspect", tree)
    obs["captures"]["cases-inspect"] = {"light": c29.capture(app, "cases-inspect-light")}
    if sheet is not None:
        press_any(app, "Close details")
        app.wait_for(lambda t: dialog(t, "Case details") is None, 6)
    time.sleep(0.5)
    n1["inspect_scans"] = len(endpoint.requests()) - scans_before
    # Open the first Runtime case in the Playground; the operator runs it there; the result is recorded.
    runtime_label = labels(app.tree(), "AXButton", "Open in Playground:")[0]
    case_name = runtime_label.split(": ", 1)[1]
    app.ax("press", "AXButton", runtime_label)
    tree, n1["playground"] = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Administration") is not None and app.has_text(t, f"{case_name} is loaded"), 20)
    n1["no_scan_on_open"] = len(endpoint.requests()) == scans_before
    c29.press(app, "AXButton", "Scan", 10)
    tree, n1["scanned"] = app.wait_for(lambda t: len(endpoint.requests()) == scans_before + 1 and app.has_text(t, "Blocked"), 30)
    time.sleep(1)
    app.menu("Go", "Test Cases")
    tree, n1["result_recorded"] = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Test Cases") is not None and app.has_text(t, "Blocked"), 15)
    obs["tree_result"] = c29.save_tree("cases-result", tree)
    obs["captures"]["cases-result"] = {"light": c29.capture(app, "cases-result-light")}
    n1["scans_total"] = len(endpoint.requests()) - scans_before
    # New case: the dialog; a Runtime case saved; a specification saved from the Red Team filter.
    scans5 = len(endpoint.requests())

    def notice(text, timeout=12):
        return app.wait_for(lambda t: app.has_text(t, text), timeout)

    press_any(app, "New case")
    tree, n1["new_dialog"] = app.wait_for(lambda t: dialog(t, "New test case") is not None and fx.App.node(t, "AXTextField", "Security profile") is not None, 10)
    app.type_into("AXTextField", "Name", "Native runtime case")
    app.type_into("AXTextField", "Security profile", "Production guardrails")
    app.type_into("AXTextField", "Application", "Support bot")
    app.type_into("AXTextArea", "Input or test specification", "Native input")
    c29.press(app, "AXButton", "Save case")
    _, n1["runtime_saved"] = notice("Saved Native runtime case to this tenant's cases.")
    n1["new_closed"] = dialog(app.tree(), "New test case") is None
    n1["runtime_listed"] = fx.App.node(app.tree(), "AXButton", "Open in Playground: Native runtime case") is not None
    t5 = app.tree()
    red5 = next((l for l in labels(t5, PILLS, "AI Red Teaming ")), None)
    role5 = next(n["role"] for n in t5 if (n.get("label") or "") == red5 and n.get("role") in PILLS)
    app.ax("press", role5, red5)
    time.sleep(0.5)
    press_any(app, "New case")
    app.wait_for(lambda t: dialog(t, "New test case") is not None and fx.App.node(t, "AXTextField", "Name") is not None, 10)
    n1["spec_form"] = fx.App.node(app.tree(), "AXTextField", "Security profile") is None
    app.type_into("AXTextField", "Name", "Native red team spec")
    app.type_into("AXTextArea", "Input or test specification", "Attack the support bot.")
    c29.press(app, "AXButton", "Save case")
    _, n1["spec_saved"] = notice("Saved Native red team spec to this tenant's cases.")
    n1["spec_inspect_only"] = any(n.get("role") in BTN and (n.get("label") or "") == "Inspect: Native red team spec" for n in app.tree()) and fx.App.node(app.tree(), "AXButton", "Open in Playground: Native red team spec") is None
    all5 = next((l for l in labels(app.tree(), PILLS, "All ")), None)
    app.ax("press", role5, all5)
    time.sleep(0.5)
    # The ⋯ menu: Remove, and Copy to Tenant B.
    press_any(app, "More actions for Native runtime case")
    tree, n1["menu"] = app.wait_for(lambda t: any(n.get("role") in ("AXMenuItem", "AXButton") and (n.get("label") or "") == "Remove case Native runtime case" for n in t), 6)
    item = next(n for n in tree if (n.get("label") or "") == "Remove case Native runtime case")
    app.ax("press", item["role"], "Remove case Native runtime case")
    _, n1["removed"] = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Open in Playground: Native runtime case") is None, 10)
    press_any(app, "More actions for Native red team spec")
    tree, _ = app.wait_for(lambda t: any((n.get("label") or "") == "Copy Native red team spec to Tenant B" for n in t), 6)
    item = next(n for n in tree if (n.get("label") or "") == "Copy Native red team spec to Tenant B")
    app.ax("press", item["role"], "Copy Native red team spec to Tenant B")
    _, n1["copied"] = notice("Copied as a new case into Tenant B.")
    # Export and import through the packs dialog and the native sheets.
    out = os.path.join(work, "exports")
    os.makedirs(out)
    press_any(app, "Load starter packs…")
    app.wait_for(lambda t: dialog(t, "Load starter packs") is not None, 10)
    c29.press(app, "AXButton", "Export…")
    sheets.save_as(app, out)
    _, n1["export_saved"] = notice("Saved prisma-airs-test-cases.json.")
    n1["export_file"] = os.path.exists(os.path.join(out, "prisma-airs-test-cases.json"))
    c29.press(app, "AXButton", "Export…")
    sheets.cancel(app)
    _, n1["export_cancelled"] = notice("Export cancelled. Nothing was saved.")
    c29.press(app, "AXButton", "Import…")
    sheets.open_file(app, os.path.join(out, "prisma-airs-test-cases.json"))
    _, n1["imported"] = notice("Every case in that file is already in the library.")
    c29.press(app, "AXButton", "Cancel")
    app.wait_for(lambda t: dialog(t, "Load starter packs") is None, 6)
    n1["no_scan"] = len(endpoint.requests()) == scans5
    obs["tree_library"] = c29.save_tree("cases-library", app.tree())
    obs["n1"] = n1

    # ---- N02: keyboard, ring, layouts ----
    n2 = {}
    packs_btn = next((n for n in app.tree() if n.get("role") in BTN and (n.get("label") or "") == "Load starter packs…"), None)
    app.ax("focus", packs_btn["role"] if packs_btn else "AXButton", "Load starter packs…")
    time.sleep(0.3)
    walk = []
    for _ in range(8):
        app.keycode(48)
        time.sleep(0.15)
        f = fx.App.focused(app.tree()) or {}
        walk.append(f.get("label") or "")
    n2["walk"] = walk
    keys = ["New case", "Search cases", "All ", "Open in Playground:"]
    idx = [next((i for i, l in enumerate(walk) if l.startswith(k)), None) for k in keys]
    n2["order"] = list(zip(keys, idx))
    n2["reach_in_order"] = all(i is not None for i in idx[:3]) and idx[:3] == sorted(idx[:3])
    app.ax("focus", packs_btn["role"] if packs_btn else "AXButton", "Load starter packs…")
    time.sleep(0.4)
    plain = c29.capture(app, "cases-ring-plain")
    n2["ring"] = ring_any(app, "New case", plain, "cases-ring-new")
    layouts = {}
    size_window(app, pid, 1440, 900)
    for theme in ("Light", "Dark"):
        c29.appearance(app, theme)
        time.sleep(0.4)
        o, tree = overflow(app)
        g = geometry(tree)
        o["geometry"] = g
        o["width_1440"] = bool(g["web"] and g["web"][0] == 1440)
        o["table"] = table_usable(tree, 1)
        name = f"cases-{g['web'][0]}x{g['web'][1]}-{theme.lower()}" if g["web"] else f"cases-unknown-{theme.lower()}"
        o["capture"] = c29.capture(app, name)
        o["tree"] = c29.save_tree(name, tree)
        layouts[name] = o
    size_window(app, pid, 900, 600)
    c29.zoom(app, 10)
    for theme in ("Light", "Dark"):
        c29.appearance(app, theme)
        time.sleep(0.4)
        first = next((n for n in app.tree() if n.get("role") in BTN and (n.get("label") or "").startswith(("Open in Playground:", "Inspect:"))), None)
        if first:
            app.ax("focus", first["role"], first["label"][:60])
            time.sleep(0.4)
        o, tree = overflow(app)
        g = geometry(tree)
        vp = g["viewport"] or g["web"]
        o["geometry"] = g
        o["window_900x600"] = bool(vp and vp[0] == 900 and vp[1] == 600 and g["web"] and g["web"][0] == 900)
        o["table"] = table_usable(tree, 2)
        name = f"cases-{vp[0]}x{vp[1]}-200-{theme.lower()}" if vp else f"cases-unknown-200-{theme.lower()}"
        o["capture"] = c29.capture(app, name)
        o["tree"] = c29.save_tree(name, tree)
        layouts[name] = o
    c29.zoom(app, -10)
    c29.appearance(app, "Match System")
    size_window(app, pid, 1440, 900)
    n2["layouts"] = layouts
    obs["captures"]["layouts"] = {k: v["capture"] for k, v in layouts.items()}
    obs["captures"]["cases-ring"] = {"plain": n2["ring"].get("plain"), "focused": n2["ring"].get("capture")}
    obs["n2"] = n2
    app.quit()

    run_path = os.path.join(NATIVE, "observations.json")
    with open(run_path, "w") as f:
        json.dump(json.loads(json.dumps({"observations": obs, "log": LOG}).replace(rel(NATIVE), rel(FINAL))), f, indent=1)
    run_rel = rel(run_path)
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C40"]}
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail", "observation": observation, "evidence": [e for e in evidence if e]})

    caps = lambda name: [v for v in obs["captures"].get(name, {}).values() if v]  # noqa: E731
    n1_ok = (all(n1.get(k) for k in ("screen", "empty_line", "empty_state", "packs_dialog", "packs_transfer", "packs", "pack_named", "not_run", "search_empty", "inspect_sheet", "sheet_docked", "red_team_runs_from", "never_run_note",
                                     "playground", "no_scan_on_open", "scanned", "result_recorded", "new_dialog", "runtime_saved", "new_closed", "runtime_listed", "spec_form", "spec_saved", "spec_inspect_only", "menu", "removed", "copied",
                                     "export_saved", "export_file", "export_cancelled", "imported", "no_scan"))
             and len(n1.get("packs_options", [])) == 2 and n1["runtime_rows"] >= 1 and n1["spec_rows"] >= 1 and bool(n1.get("count_line")) and len(n1.get("pills", [])) >= 3 and bool(n1.get("range"))
             and n1.get("filtered_runtime_rows") == 0 and (n1.get("filtered_spec_rows") or 0) >= 1 and n1["inspect_scans"] == 0 and n1["scans_total"] == 1 and len(n1["parity"]) == 2)
    add("C40-N01", n1_ok,
        f"screen {n1['screen']} with the count line {n1.get('count_line')!r}; empty first {n1['empty_state']}; the packs dialog {n1['packs_dialog']} with {n1.get('packs_options')} and Import…/Export… {n1['packs_transfer']}; loaded {n1['packs']} ({n1['runtime_rows']} runtime, {n1['spec_rows']} spec rows, pack named {n1['pack_named']}, Not run {n1['not_run']}); pills {n1.get('pills')}, range {n1.get('range')}; "
        f"filter {n1.get('filter_label')} → {n1.get('filtered_runtime_rows')}/{n1.get('filtered_spec_rows')}; search empty with Clear {n1['search_empty']}; Inspect opened the sheet {n1['inspect_sheet']} docked at 520px {n1['sheet_docked']} naming Red Teaming {n1['red_team_runs_from']} and the never-run note {n1['never_run_note']}, {n1['inspect_scans']} scan requests; "
        f"Open in Playground {n1['playground']} without a scan {n1['no_scan_on_open']}, scanned there {n1['scanned']} and recorded {n1['result_recorded']} ({n1['scans_total']} request); New case dialog {n1['new_dialog']}: runtime saved {n1['runtime_saved']} and listed {n1['runtime_listed']}, spec form {n1['spec_form']} saved {n1['spec_saved']} inspect-only {n1['spec_inspect_only']}; ⋯ menu {n1['menu']}: removed {n1['removed']}, copied to Tenant B {n1['copied']}; "
        f"export saved {n1['export_saved']} (file {n1['export_file']}) / cancelled {n1['export_cancelled']}, import of the same file {n1['imported']}; no scan from the library {n1['no_scan']}; composites {sorted(n1['parity'].values())}",
        [run_rel, obs["tree_empty"], obs["tree_packs"], obs["tree_cases"], obs["tree_inspect"], obs["tree_result"], obs["tree_library"]] + caps("cases-packs") + caps("cases") + caps("cases-inspect") + caps("cases-result") + sorted(n1["parity"].values()))
    lay = n2["layouts"]
    ring_ok = (n2["ring"].get("diff") or 0) > 40 and n2["ring"].get("focused") == "New case"
    layouts_ok = len(lay) == 4 and all(v["overflow_px"] == 0 and v["horizontal_scrollbars"] == 0 and v["table"]["ok"] for v in lay.values()) \
        and sum(1 for v in lay.values() if v.get("width_1440")) == 2 and sum(1 for v in lay.values() if v.get("window_900x600")) == 2
    add("C40-N02", n2["reach_in_order"] and ring_ok and layouts_ok,
        f"Tab from Load starter packs… reached {n2['order']} in order {n2['reach_in_order']} (walk {n2['walk']}); ring on New case {n2['ring'].get('diff')} px; "
        + "; ".join(f"{k}: web {v['geometry']['web']} viewport {v['geometry']['viewport']} overflow {v['overflow_px']} px, scroll bars {v['horizontal_scrollbars']}, table {v['table']}" for k, v in lay.items()),
        [run_rel] + [v["tree"] for v in lay.values()] + caps("layouts") + caps("cases-ring"))

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1", "subject": "C40", "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (System Events and the ax helper) with fixture WebView self-capture; tools/native-acceptance/acceptance_c40.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "Pixels come from the fixture build's own WKWebView snapshot; the parity composites put the prototype board (rendered at the WebView's measured size) beside it; the native file sheets are driven through the fixture sheet adapter.",
            "Synthetic scan and management endpoints only; the starter packs are the bundled ones (the public catalog is not fetched natively).",
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
