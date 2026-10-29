#!/usr/bin/env python3
"""C35 native acceptance: the ported shell (R2App) driven through the real
fixture window against the synthetic scan and management endpoints.

Run from the repository root after `xtask native-prepare --chunk C35`.
Requires the macOS Accessibility grant. Synthetic services only; the fixture
data root is removed at the end.

Checks (docs/architecture/native-checks.json, C35): the rail, status block
and header with their controls bound to the core's preferences (N01); the
tenant menu and the palette at the prototype's widths (N02); keyboard reach,
the focus ring, and the 1440×900 and 900×600 / 200% layouts in both themes
(N03).
"""

import json
import os
import shutil
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fixture_app as fx  # noqa: E402
import acceptance_c01 as c1  # noqa: E402  (png pixels)
import acceptance_c08 as c8  # noqa: E402
import acceptance_c28 as c28  # noqa: E402
import acceptance_c29 as c29  # noqa: E402
import acceptance_c31 as c31  # noqa: E402  (focus ring helper)

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C35")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C35" else "native-C35")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C35" else "native-acceptance-C35.json")
LOG = []
TITLE_BAR = 32
NAV = ["Home", "Inbox", "Harness", "Academy", "Test Cases", "API Client", "Administration"]
PRODUCTS = ["AI Runtime Security", "AI Gateway", "AI Red Teaming", "Model and skill scans", "Agentic Identity Security", "Agentic Endpoint Security"]
CONTROLS = ("AXButton", "AXCheckBox", "AXPopUpButton", "AXMenuButton", "AXSwitch", "AXToggle")


def rel(path):
    return os.path.relpath(path, ROOT)


def labelled(tree, roles, label):
    return next((n for n in tree if n.get("role") in roles and (n.get("label") or "").strip() == label), None)


def starts(tree, roles, prefix):
    return next((n for n in tree if n.get("role") in roles and (n.get("label") or "").startswith(prefix)), None)


def group(tree, label):
    return next((n for n in tree if n.get("role") == "AXGroup" and (n.get("label") or "") == label), None)


def within(tree, node):
    """The nodes below `node` in tree order (the tree is depth-first with a depth field)."""
    if not node:
        return []
    i = tree.index(node)
    out = []
    for m in tree[i + 1:]:
        if m.get("depth", 0) <= node.get("depth", 0):
            break
        out.append(m)
    return out


def luminance(capture):
    """Mean luminance of a capture over a coarse grid, 0 (black) to 255."""
    width, height, bpp, rows = c1.png_pixels(os.path.join(ROOT, capture))
    total, count = 0, 0
    for y in range(0, height, max(1, height // 40)):
        row = rows[y]
        for x in range(0, width, max(1, width // 40)):
            r, g, b = row[x * bpp], row[x * bpp + 1], row[x * bpp + 2]
            total += (r * 299 + g * 587 + b * 114) // 1000
            count += 1
    return total / max(count, 1)


def size_window(app, pid, w, h):
    """Places the window at the screen's origin, asks for w×h plus the title bar, and
    returns the WebView's actual CSS size from the accessibility tree (R5): what a
    matrix row records and asserts, never the requested size."""
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
    work = tempfile.mkdtemp(prefix="airs-v2-c35-acceptance-")
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
        return exercise(app, endpoint, prepared, FINAL)
    finally:
        endpoint.stop()
        mgmt.stop()
        shutil.rmtree(work, ignore_errors=True)


def exercise(app, endpoint, prepared, FINAL):
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
    geometry = size_window(app, pid, 1440, 900)
    time.sleep(0.6)

    # ---- N01: the rail, status block and header ------------------------------------
    n1 = {}
    app.menu("Go", "Home")
    c29.heading(app, "Home")
    # The N01 captures are named by the appearance they show: it is selected, never assumed.
    c29.appearance(app, "Light")
    time.sleep(0.8)
    tree = app.tree()
    obs["tree_home"] = c29.save_tree("home", tree)
    rail = within(tree, c29.nav(tree, "Destinations"))
    rail_labels = [(n.get("label") or "") for n in rail if n.get("role") in CONTROLS]
    n1["nav"] = [next((l for l in rail_labels if l.startswith(name)), None) for name in NAV]
    n1["nav_hints"] = all(l and f"Command {i + 1}" in l for i, l in enumerate(n1["nav"]))
    n1["products"] = [next((l for l in rail_labels if l.startswith(p)), None) for p in PRODUCTS]
    n1["products_listed"] = all(n1["products"])
    n1["new_tags"] = sum(1 for l in n1["products"] if l and l.endswith(" New"))
    toggle = labelled(tree, CONTROLS, "Show or hide products")
    n1["products_toggle"] = toggle is not None
    if toggle:
        app.ax("press", toggle["role"], "Show or hide products")
        t, hidden = app.wait_for(lambda t: starts(within(t, c29.nav(t, "Destinations")), CONTROLS, "AI Gateway") is None, 8)
        n1["products_hidden"] = hidden
        app.ax("press", toggle["role"], "Show or hide products")
        t, shown = app.wait_for(lambda t: starts(within(t, c29.nav(t, "Destinations")), CONTROLS, "AI Gateway") is not None, 8)
        n1["products_shown_again"] = shown
    rail_text = " ".join((n.get("label") or n.get("value") or "") for n in rail if n.get("role") == "AXStaticText")
    n1["status_rows"] = {k: k in rail_text for k in ("Harness", "Keychain", "Jobs")}
    connected = starts(rail, CONTROLS, "Connected") or starts(rail, ("AXButton",), "Connected")
    n1["connected"] = connected is not None or "Connected" in rail_text
    # The header: the title, the search button's size, the switch, the Harness button.
    web = next((n for n in tree if n.get("role") == "AXWebArea"), None)
    search = starts(tree, CONTROLS, "Search or go to")
    n1["search"] = {"size": search.get("size") if search else None, "top": (search["pos"][1] - web["pos"][1]) if search and web else None}
    n1["search_260"] = bool(search and 255 <= search["size"][0] <= 265 and 30 <= search["size"][1] <= 34)
    n1["search_in_header"] = bool(n1["search"]["top"] is not None and 0 <= n1["search"]["top"] <= 56)
    titles = [n for n in tree if n.get("role") == "AXStaticText" and (n.get("label") or n.get("value") or "") == "Home" and web and n.get("pos") and n["pos"][1] - web["pos"][1] <= 56]
    n1["header_title"] = len(titles) >= 1
    explain = labelled(tree, CONTROLS, "Show the calls")
    n1["explain"] = explain is not None
    n1["explain_role"] = explain.get("role") if explain else None
    ask = starts(tree, ("AXButton",), "Ask Harness")
    n1["ask_harness"] = ask is not None
    # The Explain switch is the show-the-calls preference: off hides the calls panel, on shows it.
    calls_panel = lambda t: fx.App.node(t, "AXButton", "Hide the calls")  # noqa: E731  (the panel's own control)
    n1["calls_before"] = calls_panel(tree) is not None
    if explain:
        app.ax("press", explain["role"], "Show the calls")
        t, flipped = app.wait_for(lambda t: (calls_panel(t) is not None) != n1["calls_before"], 10)
        n1["calls_after_first"] = calls_panel(t) is not None
        n1["explain_flipped"] = flipped
        app.ax("press", explain["role"], "Show the calls")
        t, back = app.wait_for(lambda t: (calls_panel(t) is not None) == n1["calls_before"], 10)
        n1["explain_back"] = back
    obs["captures"]["home"] = {"light": c29.capture(app, "home-light")}
    # The theme toggle switches the shown appearance.
    before = luminance(obs["captures"]["home"]["light"])
    theme = labelled(tree, CONTROLS, "Switch light or dark appearance")
    n1["theme_toggle"] = theme is not None
    if theme:
        app.ax("press", theme["role"], "Switch light or dark appearance")
        time.sleep(1.2)
        obs["captures"]["home"]["dark"] = c29.capture(app, "home-dark")
        after = luminance(obs["captures"]["home"]["dark"])
        n1["theme"] = {"before": round(before), "after": round(after)}
        n1["theme_switched"] = abs(after - before) > 40
        app.ax("press", theme["role"], "Switch light or dark appearance")
        time.sleep(1.0)
        obs["captures"]["home"]["light-again"] = c29.capture(app, "home-light-again")
        n1["theme_back"] = abs(luminance(obs["captures"]["home"]["light-again"]) - before) < 20
    c29.appearance(app, "Match System")
    # Ask Harness opens the Harness; Settings ⌘, opens Settings.
    if ask:
        app.ax("press", "AXButton", "Ask Harness")
        _, n1["harness_opened"] = c29.heading(app, "Harness")
    settings = starts(tree, ("AXButton",), "Settings")
    n1["settings_button"] = settings is not None
    if settings:
        app.menu("Go", "Home")
        c29.heading(app, "Home")
        app.ax("press", "AXButton", settings["label"])
        _, n1["settings_opened"] = c29.heading(app, "Settings")
        tree = app.tree()
        n1["classic_in_settings"] = labelled(tree, ("AXCheckBox",), "Use the new interface") is not None
        n1["classic_not_in_rail"] = starts(within(tree, c29.nav(tree, "Destinations")), CONTROLS, "Return to the classic") is None
    obs["n1"] = n1

    # ---- N02: the tenant menu and the palette -------------------------------------
    n2 = {}
    app.menu("Go", "Home")
    c29.heading(app, "Home")
    time.sleep(0.5)
    trigger = starts(app.tree(), CONTROLS, "Switch tenant")
    n2["trigger"] = trigger.get("label") if trigger else None
    if trigger:
        app.ax("press", trigger["role"], trigger["label"])
    t, opened = app.wait_for(lambda t: group(t, "Tenants") is not None, 8)
    time.sleep(0.5)
    tree = app.tree()
    obs["tree_menu"] = c29.save_tree("tenant-menu", tree)
    menu = group(tree, "Tenants")
    n2["menu_open"] = menu is not None
    n2["menu_size"] = menu.get("size") if menu else None
    n2["menu_360"] = bool(menu and 350 <= menu["size"][0] <= 370)
    inside = within(tree, menu)
    texts = " ".join((n.get("label") or n.get("value") or "") for n in inside)
    n2["menu_rows"] = {k: any((n.get("label") or "").startswith(k) for n in inside if n.get("role") in CONTROLS) for k in ("Tenant settings and credentials", "Change Gateway workspace", "Add a tenant")}
    n2["identity"] = "Northwind Health" in texts and "Production" in texts and f"TSG {c8.TSG}" in texts
    rows = [n for n in within(tree, group(tree, "Switch tenant")) if n.get("role") in CONTROLS]
    n2["tenant_rows"] = [n.get("label") for n in rows]
    n2["current_checked"] = bool(rows and str(rows[0].get("value", "")).strip() in ("1", "True", "true", "checked"))
    obs["captures"]["tenant-menu"] = {"light": c29.capture(app, "tenant-menu-light")}
    # The rows' ⌃n hints are drawn inside named buttons (not separate accessible text); the
    # hint is proven by its key: ⌃1 picks the first (current) tenant and closes the menu.
    app.keycode(18, "control down")
    _, n2["hint"] = app.wait_for(lambda t: group(t, "Tenants") is None, 8)
    time.sleep(0.4)
    app.ax("press", trigger["role"], trigger["label"])
    app.wait_for(lambda t: group(t, "Tenants") is not None, 8)
    time.sleep(0.4)
    app.keycode(53)
    t, closed = app.wait_for(lambda t: group(t, "Tenants") is None, 8)
    n2["menu_closed"] = closed
    time.sleep(0.3)
    f = fx.App.focused(app.tree()) or {}
    n2["focus_back"] = (f.get("label") or "").startswith("Switch tenant")
    # The palette from the header's search button.
    app.ax("press", "AXButton", starts(app.tree(), ("AXButton",), "Search or go to")["label"])
    t, popen = app.wait_for(lambda t: c29.dialog(t) is None and any((n.get("label") or "") == "Command palette" and n.get("subrole") in ("AXDialog", "AXApplicationDialog") for n in t), 8)
    time.sleep(0.5)
    tree = app.tree()
    obs["tree_palette"] = c29.save_tree("palette", tree)
    dialog = next((n for n in tree if (n.get("label") or "") == "Command palette" and n.get("subrole") in ("AXDialog", "AXApplicationDialog")), None)
    n2["palette_open"] = dialog is not None
    n2["palette_size"] = dialog.get("size") if dialog else None
    n2["palette_640"] = bool(dialog and 630 <= dialog["size"][0] <= 650)
    inside = within(tree, dialog)
    texts = " ".join((n.get("label") or n.get("value") or "") for n in inside)
    # The Tenants group lists the other tenants; with one tenant there is none to list.
    n2["groups"] = {k: k in texts for k in ("Go to", "Actions")}
    n2["tenants_group"] = "Tenants" in texts
    n2["footer"] = "move" in texts and "open" in texts
    options = [n for n in inside if n.get("role") in ("AXStaticText", "AXMenuItem", "AXCell", "AXRow", "AXGroup") and (n.get("label") or "").startswith(("Home", "Inbox"))]
    n2["options"] = len(options) >= 2
    n2["hints"] = "⌘1" in texts
    obs["captures"]["palette"] = {"light": c29.capture(app, "palette-light")}
    app.keys("inbox")
    time.sleep(0.5)
    tree = app.tree()
    inside = within(tree, next((n for n in tree if (n.get("label") or "") == "Command palette" and n.get("subrole") in ("AXDialog", "AXApplicationDialog")), None))
    n2["narrowed"] = not any((n.get("label") or "") == "Harness" for n in inside)
    app.keycode(36)
    _, n2["enter_navigated"] = c29.heading(app, "Inbox")
    n2["palette_closed"] = not any((n.get("label") or "") == "Command palette" for n in app.tree())
    obs["n2"] = n2

    # ---- N03: keyboard reach, the focus ring and the layouts ------------------------
    n3 = {}
    app.menu("Go", "Home")
    c29.heading(app, "Home")
    time.sleep(0.5)
    trigger = starts(app.tree(), CONTROLS, "Switch tenant")
    app.ax("focus", trigger["role"], trigger["label"])
    time.sleep(0.3)
    walk = [(((f or {}).get("label")) or "")[:40] for f in app.tab_walk(24)]
    n3["walk"] = walk
    order = []
    for key in ("Home", "Administration", "Connected", "Settings", "Search or go to", "Show the calls", "Ask Harness"):
        idx = next((i for i, l in enumerate(walk) if l.startswith(key)), None)
        order.append((key, idx))
    n3["order"] = order
    n3["reach_in_order"] = all(i is not None for _, i in order) and [i for _, i in order] == sorted(i for _, i in order)
    plain = c29.capture(app, "search-ring-plain")
    n3["ring"] = c31.ring_on(app, starts(app.tree(), ("AXButton",), "Search or go to")["label"], plain, "search-ring")
    ring_ok = (n3["ring"].get("diff") or 0) > 40
    explain = labelled(app.tree(), CONTROLS, "Show the calls")
    app.ax("focus", explain["role"], "Show the calls")
    time.sleep(0.3)
    before_calls = calls_panel(app.tree()) is not None
    app.keycode(49)
    t, flipped = app.wait_for(lambda t: (calls_panel(t) is not None) != before_calls, 10)
    n3["space_flips_explain"] = flipped
    app.keycode(49)
    app.wait_for(lambda t: (calls_panel(t) is not None) == before_calls, 10)
    # Every matrix row is named by the WebView's measured CSS size, not the request: this
    # display cannot give a 900px-high web area, and a clamped resize must never pass as one.
    layouts = {}
    for screen, menu_item in (("home", "Home"), ("inbox", "Inbox"), ("admin", "Administration")):
        app.menu("Go", menu_item)
        c29.heading(app, menu_item)
        wide = size_window(app, pid, 1440, 900)
        for theme in ("Light", "Dark"):
            c29.appearance(app, theme)
            o, tree = overflow(app)
            o["geometry"] = wide
            o["width_1440"] = bool(wide["web"] and wide["web"][0] == 1440)
            o["capture"] = c29.capture(app, f"{screen}-{wide['web'][0]}x{wide['web'][1]}-{theme.lower()}")
            o["tree"] = c29.save_tree(f"{screen}-{wide['web'][0]}x{wide['web'][1]}-{theme.lower()}", tree)
            layouts[f"{screen}-{wide['web'][0]}x{wide['web'][1]}-{theme.lower()}"] = o
        small = size_window(app, pid, 900, 600)
        c29.zoom(app, 10)
        for theme in ("Light", "Dark"):
            c29.appearance(app, theme)
            o, tree = overflow(app)
            o["geometry"] = small
            o["window_900x600"] = bool(small["web"] and small["web"][0] == 900 and small["web"][1] == 600)
            o["capture"] = c29.capture(app, f"{screen}-{small['web'][0]}x{small['web'][1]}-200-{theme.lower()}")
            o["tree"] = c29.save_tree(f"{screen}-{small['web'][0]}x{small['web'][1]}-200-{theme.lower()}", tree)
            layouts[f"{screen}-{small['web'][0]}x{small['web'][1]}-200-{theme.lower()}"] = o
        c29.zoom(app, -10)
        c29.appearance(app, "Match System")
        size_window(app, pid, 1440, 900)
    n3["layouts"] = layouts
    n3["geometry"] = geometry
    obs["captures"]["layouts"] = {k: v["capture"] for k, v in layouts.items()}
    obs["n3"] = n3
    app.quit()

    run_path = os.path.join(NATIVE, "observations.json")
    with open(run_path, "w") as f:
        json.dump(json.loads(json.dumps({"observations": obs, "log": LOG}).replace(rel(NATIVE), rel(FINAL))), f, indent=1)
    run_rel = rel(run_path)
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C35"]}
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [e for e in evidence if e]})

    caps = lambda name: [v for v in obs["captures"].get(name, {}).values() if v]  # noqa: E731
    n1_ok = (all(n1["nav"]) and n1["nav_hints"] and n1["products_listed"] and n1["new_tags"] >= 2 and n1["products_toggle"] and n1.get("products_hidden") and n1.get("products_shown_again")
             and all(n1["status_rows"].values()) and n1["connected"] and n1["search_260"] and n1["search_in_header"] and n1["header_title"] and n1["explain"] and n1["ask_harness"]
             and n1.get("explain_flipped") and n1.get("explain_back") and n1["theme_toggle"] and n1.get("theme_switched") and n1.get("theme_back")
             and n1.get("harness_opened") and n1["settings_button"] and n1.get("settings_opened") and n1.get("classic_in_settings") and n1.get("classic_not_in_rail"))
    add("C35-N01", n1_ok,
        f"rail listed {n1['nav']} with ⌘ hints {n1['nav_hints']}; products {n1['products']} with {n1['new_tags']} New tags, hidden by the chevron {n1.get('products_hidden')} and shown again {n1.get('products_shown_again')}; status rows {n1['status_rows']} and Connected {n1['connected']}; header: title {n1['header_title']}, search button {n1['search']['size']} at top {n1['search']['top']} ({n1['search_260'] and n1['search_in_header']}), Explain as {n1['explain_role']} flipping the calls panel {n1.get('explain_flipped')} and back {n1.get('explain_back')}, Ask Harness opened the Harness {n1.get('harness_opened')}; theme toggle luminance {n1.get('theme')} switched {n1.get('theme_switched')} and back {n1.get('theme_back')}; Settings ⌘, opened Settings {n1.get('settings_opened')} where the classic switch lives {n1.get('classic_in_settings')} and not in the rail {n1.get('classic_not_in_rail')}",
        [run_rel, obs["tree_home"]] + caps("home"))
    n2_ok = (n2["menu_open"] and n2["menu_360"] and all(n2["menu_rows"].values()) and n2["identity"] and len(n2["tenant_rows"]) >= 1 and n2["hint"] and n2["menu_closed"] and n2["focus_back"]
             and n2["palette_open"] and n2["palette_640"] and all(n2["groups"].values()) and n2["footer"] and n2["options"] and n2["hints"] and n2["narrowed"] and n2["enter_navigated"] and n2["palette_closed"])
    add("C35-N02", n2_ok,
        f"tenant menu {n2['menu_size']} ({n2['menu_360']}) named the tenant with environment and TSG {n2['identity']}, rows {n2['menu_rows']}, tenants {n2['tenant_rows']} (current checked {n2['current_checked']}), ⌃1 picked the current tenant and closed the menu {n2['hint']}; Escape closed it {n2['menu_closed']} and focus returned {n2['focus_back']}; palette {n2['palette_size']} ({n2['palette_640']}) with groups {n2['groups']}, ⌘ hints {n2['hints']} and the key-hint footer {n2['footer']}; typing narrowed the rows {n2['narrowed']}, Enter opened the Inbox {n2['enter_navigated']} and closed it {n2['palette_closed']}",
        [run_rel, obs["tree_menu"], obs["tree_palette"]] + caps("tenant-menu") + caps("palette"))
    layout_ok = all(v["overflow_px"] == 0 and v["horizontal_scrollbars"] == 0 for v in layouts.values())
    geometry_ok = all(v.get("width_1440", v.get("window_900x600", False)) for v in layouts.values())
    sizes_text = ", ".join(f"{k}: {v['geometry']['web']}" for k, v in layouts.items())
    overflow_text = ", ".join(f"{k}: {v['overflow_px']}" for k, v in layouts.items())
    add("C35-N03", n3["reach_in_order"] and ring_ok and n3["space_flips_explain"] and layout_ok and geometry_ok,
        f"Tab from the tenant button reached {n3['order']} in order {n3['reach_in_order']}; ring on the search button {n3['ring'].get('diff')} px ({ring_ok}); Space on Explain flipped the preference {n3['space_flips_explain']}; measured WebView sizes by row {sizes_text} (1440 wide and 900×600 asserted from the accessibility tree: {geometry_ok}); overflow px by layout {overflow_text}, horizontal scroll bars {[v['horizontal_scrollbars'] for v in layouts.values()]}",
        [run_rel, n3["ring"].get("capture"), n3["ring"].get("plain")] + [v["capture"] for v in layouts.values()] + [v.get("tree") for v in layouts.values()])

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1", "subject": "C35", "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (System Events and the ax helper) with fixture WebView self-capture; tools/native-acceptance/acceptance_c35.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "Pixels come from the fixture build's own WKWebView snapshot; widths are read from the accessibility tree at 100% zoom.",
            "Window geometry is measured, not assumed: this Mac's display (1470×956 points, menu bar and title bar) gives a 1440-wide WebView of about 800 CSS px; the wide rows record that measured size and assert the width, the small rows assert exactly 900×600.",
            "Scan and management endpoints are synthetic services on this Mac (scan_server.py, mgmt_server.py).",
            "Visual parity with the prototype is the owner's judgement from the composites under evidence/C35/parity, not a check here.",
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
        print(c["id"], c["result"], "-", c["observation"][:160])
    return 0 if all(c["result"] == "pass" for c in checks) else 1


if __name__ == "__main__":
    sys.exit(main())
