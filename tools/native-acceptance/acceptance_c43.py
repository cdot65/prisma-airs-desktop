#!/usr/bin/env python3
"""C43 native acceptance (D059): the first-run wizard and the add-tenant
sheet as the prototype board draws them (R2Onboard, every step, mode=add),
driven through the real fixture window against the synthetic scan and
management endpoints.

Run from the repository root after `xtask native-prepare --chunk C43`.
Requires the macOS Accessibility grant. Synthetic services only; the fixture
data root is removed at the end.

Checks (docs/architecture/native-checks.json, C43): every first-run step as
the board draws it, the rail, the pinned footer and the C29 behaviour
through to completion (N01); the add-tenant sheet (N02); keyboard reach,
the focus ring and the measured layouts in both themes (N03). The parity
composites are captured per step.
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
import acceptance_c28 as c28  # noqa: E402
import acceptance_c29 as c29  # noqa: E402
import acceptance_c31 as c31  # noqa: E402

ROOT = os.getcwd()
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence/C43")
NATIVE = os.path.join(EVIDENCE, "native")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json")
PARITY = os.path.join(EVIDENCE, "parity")
DC = os.path.join(ROOT, "tools/parity")
LOG = []
TITLE_BAR = 28
TAGS = ("AXRadioButton", "AXCheckBox", "AXButton")
STEP_BOARDS = {"welcome": "welcome", "tenant": "tenant", "products": "products", "verify": "verify", "packs": "cases", "academy": "academy", "done": "done"}


def rel(path):
    return os.path.relpath(path, ROOT)


def heading(tree, label):
    return next((n for n in tree if n.get("role") == "AXHeading" and (n.get("label") or "") == label), None)


def heading_like(tree, prefix):
    return next((n for n in tree if n.get("role") == "AXHeading" and (n.get("label") or "").startswith(prefix)), None)


def group(tree, label):
    return next((n for n in tree if n.get("role") == "AXGroup" and (n.get("label") or "") == label), None)


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


def labels(nodes, roles, prefix=""):
    roles = (roles,) if isinstance(roles, str) else roles
    out = []
    for n in nodes:
        label = n.get("label") or ""
        if n.get("role") in roles and label.startswith(prefix) and label not in out:
            out.append(label)
    return out


def press(app, role, label, timeout=8):
    app.wait_for(lambda t: fx.App.node(t, role, label) is not None, timeout)
    app.ax("press", role, label)
    time.sleep(0.5)


def press_any(app, label, timeout=8):
    """Presses the control with this label whatever role AX gives it (a switch is a checkbox, a radio a radio button)."""
    tree, _ = app.wait_for(lambda t: any((n.get("label") or "") == label and n.get("role") in TAGS for n in t), timeout)
    node = next((n for n in tree if (n.get("label") or "") == label and n.get("role") in TAGS), None)
    if node:
        app.ax("press", node["role"], label)
        time.sleep(0.5)
    return node is not None


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
             and n.get("role") in ("AXButton", "AXStaticText", "AXTextField", "AXTextArea", "AXLink", "AXHeading", "AXCheckBox", "AXRadioButton", "AXPopUpButton")]
    right = max(n["pos"][0] + n["size"][0] for n in nodes) if nodes and win else None
    hbars = [n for n in tree if n.get("role") == "AXScrollBar" and (n.get("orientation") == "horizontal" or (n.get("size") and n["size"][0] > n["size"][1]))]
    return {"overflow_px": max(0, right - (win["pos"][0] + win["size"][0])) if right is not None else None, "horizontal_scrollbars": len(hbars)}, tree


def primary_usable(tree, zoom):
    """The footer's primary inside the window and at least 32 CSS px."""
    win = next((n for n in tree if n.get("role") == "AXWindow"), None)
    labels_ = ("Create the tenant to continue", "Save the scan key to continue", "Continue", "Continue with warnings", "Set up a tenant", "Open Prisma AIRS", "Finish")
    btn = next((n for n in tree if n.get("role") == "AXButton" and (n.get("label") or "") in labels_ and n.get("pos") and n.get("size")), None)
    inside = bool(btn and win and btn["pos"][0] >= win["pos"][0] and btn["pos"][0] + btn["size"][0] <= win["pos"][0] + win["size"][0] + 1 and btn["pos"][1] + btn["size"][1] <= win["pos"][1] + win["size"][1] + 1)
    return {"label": btn.get("label") if btn else None, "size": btn.get("size") if btn else None, "inside": inside,
            "size_ok": bool(btn and btn["size"][1] >= 32 * zoom - 1), "ok": inside and bool(btn and btn["size"][1] >= 32 * zoom - 1), "zoom": zoom}


def composite(app, step, theme, web_size, prepared, mode="firstRun"):
    board = STEP_BOARDS[step]
    name = f"{'add-' if mode == 'add' else ''}{board}-{theme.lower()}"
    cap = c29.capture(app, f"parity-{name}")
    if not cap:
        raise SystemExit(f"no capture for {name}")
    os.makedirs(os.path.join(PARITY, "app"), exist_ok=True)
    os.makedirs(os.path.join(PARITY, "proto"), exist_ok=True)
    with open(os.path.join(PARITY, "bundle.json"), "w") as f:
        json.dump({"bundle": prepared["bundle"], "executable_sha256": prepared["executable_sha256"], "from": "native-prepare.json",
                   "captured_by": "tools/native-acceptance/acceptance_c43.py", "captured_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}, f, indent=2)
        f.write("\n")
    app_png = os.path.join(PARITY, "app", f"{name}.png")
    shutil.copy(os.path.join(ROOT, cap), app_png)
    web_w, web_h = web_size
    proto_png = os.path.join(PARITY, "proto", f"{name}.png")
    props = ["--prop", f"step={board}", "--prop", f"theme={theme.lower()}", "--prop", "filled=true"]
    if mode == "add":
        props += ["--prop", "mode=add"]
    subprocess.run([sys.executable, "-I", os.path.join(DC, "dc_render.py"), "--board", "R2Onboard", *props, "--width", str(web_w), "--height", str(web_h), "--out", proto_png], check=True, capture_output=True)
    out = os.path.join(PARITY, f"{name}.png")
    subprocess.run([sys.executable, "-I", os.path.join(DC, "compose.py"), "--left", proto_png, "--right", app_png,
                    "--out", out, "--title", f"Wizard · {board}{' · add' if mode == 'add' else ''} · {theme.lower()} · {web_w}×{web_h}"], check=True, capture_output=True)
    return rel(out)


def composites_for(app, step, prepared, mode="firstRun"):
    out = {}
    for theme in ("Light", "Dark"):
        c29.appearance(app, theme)
        time.sleep(0.8)
        out[theme.lower()] = composite(app, step, theme, geometry(app.tree())["web"], prepared, mode)
    c29.appearance(app, "Light")
    time.sleep(0.5)
    return out


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
    work = tempfile.mkdtemp(prefix="airs-v2-c43-acceptance-")
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
    mgmt = c8.MgmtEndpoint(os.path.join(work, "endpoint"), os.path.join(work, "mgmt"))
    mgmt.install()
    mgmt.start()
    mgmt.mode()
    os.makedirs(fx.DATA, exist_ok=True)
    try:
        return exercise(app, endpoint, mgmt, prepared, FINAL)
    finally:
        endpoint.stop()
        mgmt.stop()
        shutil.rmtree(work, ignore_errors=True)


def exercise(app, endpoint, mgmt, prepared, FINAL):
    global NATIVE
    obs = {"captures": {}, "parity": {}}
    app.launch()
    pid = app.pid
    # C44: the first run opens the wizard; there is no interface switch.
    tree, shown = app.wait_for(lambda t: heading(t, "Welcome to Prisma AIRS Desktop") is not None, 15)
    size_window(app, pid, 1440, 900)
    n1 = {"welcome": shown}

    def rail(tree):
        nav = group(tree, "Setup steps")
        return within(tree, nav) if nav else []

    def step_labels(tree):
        return [n.get("label") for n in rail(tree) if n.get("role") == "AXButton"]

    def status_texts(tree):
        return c31.status_texts(tree)

    # ---- N01: every first-run step ----
    tree = app.tree()
    n1["rail"] = {"brand": any("Prisma AIRS" in (n.get("label") or "") for n in rail(tree)), "steps": step_labels(tree),
                  "optional_tags": sum(1 for n in rail(tree) if (n.get("label") or "") == "Optional"), "privacy": any("Secrets go straight to the macOS Keychain" in (n.get("label") or "") for n in rail(tree)),
                  "theme_button": any((n.get("label") or "").endswith(" appearance") for n in rail(tree))}
    n1["welcome_board"] = {"cards": all(app.has_text(tree, c) for c in ("Built on the public APIs", "See the calls as you go", "Learn by doing")),
                           "no_import_box": fx.App.node(tree, "AXButton", "Import from v1…") is None, "no_back": fx.App.node(tree, "AXButton", "Back") is None,
                           "progress": app.has_text(tree, "Step 1 of 7"), "primary": fx.App.node(tree, "AXButton", "Set up a tenant") is not None}
    obs["tree_welcome"] = c29.save_tree("wizard-welcome", tree)
    obs["parity"]["welcome"] = composites_for(app, "welcome", prepared)
    press(app, "AXButton", "Set up a tenant")
    tree, ok = app.wait_for(lambda t: heading(t, "Connect your tenant") is not None, 10)
    f = fx.App.focused(tree) or {}
    n1["tenant"] = {"shown": ok, "heading_focused": f.get("role") == "AXHeading" and f.get("label") == "Connect your tenant",
                    "card": app.has_text(tree, "Tenant") and app.has_text(tree, "Required"),
                    "environment_control": any(n.get("role") == "AXRadioButton" and (n.get("label") or "") == "Staging" for n in tree),
                    "region": any(n.get("role") in ("AXPopUpButton", "AXComboBox") and (n.get("label") or "") == "Region" for n in tree),
                    "continue_names_missing": (fx.App.node(tree, "AXButton", "Create the tenant to continue") or {}).get("enabled") is False,
                    "progress": app.has_text(tree, "Step 2 of 7"), "back": fx.App.node(tree, "AXButton", "Back") is not None}
    # Validation, then the draft.
    press(app, "AXButton", "Create tenant")
    tree, invalid = app.wait_for(lambda t: app.has_text(t, "Enter a display name.") and app.has_text(t, "The TSG ID is exactly 10 digits."), 6)
    n1["tenant"]["validation"] = invalid
    app.type_into("AXTextField", "Display name", "Northwind Health")
    app.type_into("AXTextField", "TSG ID", "1762204418")
    press_any(app, "Staging")
    press(app, "AXButton", "Create tenant")
    tree, drafted = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Save scan key") is not None and app.has_text(t, "Northwind Health · Staging · TSG 1762204418"), 12)
    n1["tenant"]["drafted"] = drafted
    n1["tenant"]["scan_required_tag"] = any((n.get("label") or "") == "Required" for n in within(tree, next((n for n in tree if n.get("role") == "AXGroup" and (n.get("label") or "") == ""), None))) or app.has_text(tree, "Required on first run.")
    n1["tenant"]["management_card"] = app.has_text(tree, "Management credentials · OAuth 2.0") and fx.App.node(tree, "AXButton", "Where do I find these?") is not None
    n1["tenant"]["gateway_switch"] = any(n.get("role") == "AXCheckBox" and (n.get("label") or "") == "Set up an AI Gateway workspace now" for n in tree)
    n1["tenant"]["continue_needs_key"] = (fx.App.node(tree, "AXButton", "Save the scan key to continue") or {}).get("enabled") is False
    press(app, "AXButton", "Where do I find these?")
    tree, help_open = app.wait_for(lambda t: app.has_text(t, "Service accounts"), 6)
    n1["tenant"]["help"] = help_open
    app.type_into("AXTextField", "Scan API key", c8.SCAN_KEY, secure=True)
    press(app, "AXButton", "Save scan key")
    tree, saved = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Replace scan key") is not None and any("Scan key saved in the Keychain." in r for r in status_texts(t)), 12)
    n1["tenant"]["key_saved"] = saved
    n1["tenant"]["hint_in_footer"] = any("Scan key saved in the Keychain." in r for r in status_texts(tree))
    app.type_into("AXTextField", "Client ID", c8.CLIENT_ID)
    app.type_into("AXTextField", "Client secret", c8.SECRET, secure=True)
    press(app, "AXButton", "Save management client")
    tree, client_saved = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Replace management client") is not None, 12)
    n1["tenant"]["client_saved"] = client_saved
    obs["tree_tenant"] = c29.save_tree("wizard-tenant", tree)
    obs["parity"]["tenant"] = composites_for(app, "tenant", prepared)
    press(app, "AXButton", "Continue")
    tree, ok = app.wait_for(lambda t: heading(t, "Which products will you manage?") is not None, 10)
    cards = [n for n in tree if n.get("role") == "AXCheckBox" and (n.get("label") or "") in ("AI Runtime Security", "AI Gateway", "AI Red Teaming", "Model and skill scans", "Agentic Identity Security", "Agentic Endpoint Security")]
    n1["products"] = {"shown": ok, "cards": [c.get("label") for c in cards], "runtime_always": app.has_text(tree, "Always on"), "unavailable": app.has_text(tree, "Not available in this release"),
                      "rail_done_mark": any(n.get("role") == "AXButton" and (n.get("label") or "") == "Tenant" for n in rail(tree)), "progress": app.has_text(tree, "Step 3 of 7")}
    press_any(app, "AI Gateway")
    tree, chosen = app.wait_for(lambda t: any(n.get("role") == "AXCheckBox" and (n.get("label") or "") == "AI Gateway" and n.get("value") in (1, "1", True) for n in t), 8)
    n1["products"]["gateway_chosen"] = chosen
    obs["tree_products"] = c29.save_tree("wizard-products", tree)
    obs["parity"]["products"] = composites_for(app, "products", prepared)
    press(app, "AXButton", "Continue")
    tree, ok = app.wait_for(lambda t: heading(t, "Check the connection") is not None, 10)
    n1["verify"] = {"shown": ok, "banner_not_checked": app.has_text(tree, "Not checked yet"), "rows": sum(1 for x in app.texts(tree) if x == "Not run yet"),
                    "run_offered": fx.App.node(tree, "AXButton", "Run the checks") is not None, "continue_closed": (fx.App.node(tree, "AXButton", "Continue") or {}).get("enabled") is False,
                    "progress": app.has_text(tree, "Step 4 of 7")}
    press(app, "AXButton", "Run the checks")
    tree, finished = app.wait_for(lambda t: any(r in ("Checks finished; nothing failed.", "Checks finished with warnings.", "An optional check failed. You can fix it and verify again, or continue with it recorded as a warning.") for r in app.texts(t)), 40)
    n1["verify"]["finished"] = finished
    n1["verify"]["continue_open"] = any((fx.App.node(tree, "AXButton", l) or {}).get("enabled") is not False and fx.App.node(tree, "AXButton", l) is not None for l in ("Continue", "Continue with warnings"))
    n1["verify"]["again_offered"] = fx.App.node(tree, "AXButton", "Verify again") is not None
    obs["tree_verify"] = c29.save_tree("wizard-verify", tree)
    obs["parity"]["verify"] = composites_for(app, "verify", prepared)
    press(app, "AXButton", "Continue with warnings" if fx.App.node(tree, "AXButton", "Continue with warnings") else "Continue")
    tree, ok = app.wait_for(lambda t: heading(t, "Load starter test cases") is not None, 10)
    packs = [n for n in tree if n.get("role") == "AXCheckBox" and " · " in (n.get("label") or "") and "case" in (n.get("label") or "")]
    n1["packs"] = {"shown": ok, "rows": [p.get("label") for p in packs], "summary": any("will be imported" in x for x in app.texts(tree)),
                   "skip": fx.App.node(tree, "AXButton", "Skip for now") is not None, "progress": app.has_text(tree, "Step 5 of 7")}
    obs["tree_packs"] = c29.save_tree("wizard-packs", tree)
    obs["parity"]["packs"] = composites_for(app, "packs", prepared)
    press(app, "AXButton", "Continue")
    tree, ok = app.wait_for(lambda t: heading(t, "Join the AI Security Academy") is not None, 10)
    n1["academy"] = {"shown": ok, "account_card": app.has_text(tree, "Your account") and fx.App.node(tree, "AXButton", "Sign in to Academy") is not None,
                     "syncs_card": app.has_text(tree, "What is sent to Academy"), "skip": fx.App.node(tree, "AXButton", "Skip for now") is not None, "progress": app.has_text(tree, "Step 6 of 7")}
    obs["tree_academy"] = c29.save_tree("wizard-academy", tree)
    obs["parity"]["academy"] = composites_for(app, "academy", prepared)
    press(app, "AXButton", "Skip for now")
    tree, ok = app.wait_for(lambda t: heading(t, "Northwind Health is ready") is not None, 10)
    n1["done"] = {"shown": ok, "skipped_tag": any((n.get("label") or "") == "Skipped" for n in rail(tree)), "summary": all(app.has_text(tree, k) for k in ("Scan key", "Management client", "Products", "Starter cases", "Academy")),
                  "change": len(labels(tree, "AXButton", "Change ")) >= 5, "next_up": all(app.has_text(tree, k) for k in ("Home", "Inbox", "Ask the Harness")), "progress": app.has_text(tree, "Step 7 of 7"),
                  "open": fx.App.node(tree, "AXButton", "Open Prisma AIRS") is not None}
    obs["tree_done"] = c29.save_tree("wizard-done", tree)
    obs["parity"]["done"] = composites_for(app, "done", prepared)
    press(app, "AXButton", "Open Prisma AIRS")
    tree, home = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Home") is not None and c29.nav(t, "Destinations") is not None, 20)
    n1["done"]["completed"] = home and any("Northwind Health" in (n.get("label") or "") for n in tree)
    obs["n1"] = n1

    # ---- N02: the add-tenant sheet ----
    n2 = {}
    c29.open_switcher(app) if hasattr(c29, "open_switcher") else press(app, "AXButton", "Switch tenant")
    press(app, "AXButton", "Add a tenant")
    tree, sheet = app.wait_for(lambda t: c29.dialog(t) is not None, 10)
    d = c29.dialog(tree)
    web = next((n for n in tree if n.get("role") == "AXWebArea" and n.get("size")), None)
    n2["sheet"] = sheet
    n2["size"] = d.get("size") if d else None
    n2["fixed_1160"] = bool(d and abs(d["size"][0] - 1160) <= 2)
    nav = group(tree, "Setup steps")
    n2["rail_244"] = bool(nav and abs(nav["size"][0] - 244) <= 2)
    n2["steps"] = step_labels(tree)
    n2["no_back"] = fx.App.node(tree, "AXButton", "Back") is None
    n2["cancel"] = fx.App.node(tree, "AXButton", "Cancel") is not None
    n2["cancel_note"] = any("Cancel discards this draft" in r for r in status_texts(tree))
    obs["tree_add"] = c29.save_tree("wizard-add", tree)
    obs["parity"]["add"] = {}
    for theme in ("Light", "Dark"):
        c29.appearance(app, theme)
        time.sleep(0.8)
        obs["parity"]["add"][theme.lower()] = composite(app, "tenant", theme, geometry(app.tree())["web"], prepared, mode="add")
    c29.appearance(app, "Light")
    time.sleep(0.5)
    press(app, "AXButton", "Cancel")
    tree, closed = app.wait_for(lambda t: c29.dialog(t) is None, 8)
    n2["cancel_before_draft_closes"] = closed
    n2["tenants_after_cancel"] = len(c29.tenant_rows(tree)) if hasattr(c29, "tenant_rows") else None
    # Drafted: the scan card is Optional and Continue is open; Cancel discards the draft.
    press(app, "AXButton", "Add a tenant")
    app.wait_for(lambda t: c29.dialog(t) is not None, 10)
    app.type_into("AXTextField", "Display name", "Coastal Bank")
    app.type_into("AXTextField", "TSG ID", "1001464285")
    press(app, "AXButton", "Create tenant")
    tree, drafted = app.wait_for(lambda t: app.has_text(t, "Optional here; without it scans stay unavailable"), 12)
    n2["drafted_optional"] = drafted
    n2["continue_open"] = (fx.App.node(tree, "AXButton", "Continue") or {}).get("enabled") is not False and fx.App.node(tree, "AXButton", "Continue") is not None
    press(app, "AXButton", "Cancel")
    tree, discarded = app.wait_for(lambda t: c29.dialog(t) is None, 10)
    n2["cancel_discards"] = discarded and not app.has_text(tree, "Coastal Bank")
    obs["n2"] = n2

    # ---- N03: keyboard, ring, layouts (first-run steps are gone: use the add sheet's Tenant and Verify) ----
    n3 = {}
    press(app, "AXButton", "Add a tenant")
    tree, _ = app.wait_for(lambda t: c29.dialog(t) is not None, 10)
    app.ax("focus", "AXButton", "Tenant")
    time.sleep(0.3)
    walk = []
    for _ in range(12):
        app.keycode(48)
        time.sleep(0.12)
        f = fx.App.focused(app.tree()) or {}
        walk.append(f.get("label") or "")
    n3["walk"] = walk
    n3["reach"] = {"field": "Display name" in walk, "primary": any(w in ("Create the tenant to continue", "Create tenant") for w in walk)}
    app.ax("focus", "AXTextField", "Display name")
    time.sleep(0.4)
    plain = c29.capture(app, "wizard-ring-plain")
    n3["ring"] = c31.ring_on(app, "Create tenant", plain, "wizard-ring")
    layouts = {}
    for name_, (w, h, zoom) in {"1440x900": (1440, 900, 1), "900x600-200": (900, 600, 2)}.items():
        size_window(app, pid, w, h)
        if zoom == 2:
            c29.zoom(app, 10)
        for theme in ("Light", "Dark"):
            c29.appearance(app, theme)
            time.sleep(0.5)
            o, tree = overflow(app)
            g = geometry(tree)
            o["geometry"] = g
            o["primary"] = primary_usable(tree, zoom)
            o["dialog"] = c29.dialog(tree) is not None
            key = f"wizard-add-{name_}-{theme.lower()}"
            o["capture"] = c29.capture(app, key)
            o["tree"] = c29.save_tree(key, tree)
            layouts[key] = o
        if zoom == 2:
            c29.zoom(app, -10)
    c29.appearance(app, "Match System")
    size_window(app, pid, 1440, 900)
    n3["layouts"] = layouts
    obs["captures"]["layouts"] = {k: v["capture"] for k, v in layouts.items()}
    obs["captures"]["wizard-ring"] = {"plain": n3["ring"].get("plain"), "focused": n3["ring"].get("capture")}
    obs["n3"] = n3
    press(app, "AXButton", "Cancel")
    app.wait_for(lambda t: c29.dialog(t) is None, 8)
    app.quit()

    run_path = os.path.join(NATIVE, "observations.json")
    with open(run_path, "w") as f:
        json.dump(json.loads(json.dumps({"observations": obs, "log": LOG}).replace(rel(NATIVE), rel(FINAL))), f, indent=1)
    run_rel = rel(run_path)
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C43"]}
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail", "observation": observation, "evidence": [e for e in evidence if e]})

    parity_files = sorted(p for step in obs["parity"].values() for p in step.values())
    r, t_, p_, v_, k_, a_, d_ = n1["rail"], n1["tenant"], n1["products"], n1["verify"], n1["packs"], n1["academy"], n1["done"]
    n1_ok = (n1["welcome"] and r["brand"] and r["optional_tags"] >= 2 and r["privacy"] and r["theme_button"] and len(r["steps"]) >= 7
             and all(n1["welcome_board"].values()) and all(t_.get(k) for k in ("shown", "heading_focused", "card", "environment_control", "region", "continue_names_missing", "progress", "validation", "drafted", "management_card", "gateway_switch", "continue_needs_key", "help", "key_saved", "hint_in_footer", "client_saved"))
             and p_["shown"] and len(p_["cards"]) == 6 and p_["runtime_always"] and p_["unavailable"] and p_["gateway_chosen"] and p_["progress"]
             and v_["shown"] and v_["banner_not_checked"] and v_["rows"] >= 3 and v_["run_offered"] and v_["continue_closed"] and v_["finished"] and v_["continue_open"] and v_["again_offered"]
             and k_["shown"] and len(k_["rows"]) >= 1 and k_["summary"] and k_["skip"] and a_["shown"] and a_["account_card"] and a_["syncs_card"] and a_["skip"]
             and all(d_.get(k) for k in ("shown", "skipped_tag", "summary", "change", "next_up", "progress", "open", "completed")) and len(parity_files) == 14)
    add("C43-N01", n1_ok,
        f"welcome {n1['welcome']} rail {r}; welcome board {n1['welcome_board']}; tenant {t_}; products {p_}; verify {v_}; packs {k_}; academy {a_}; done {d_}; composites {parity_files}",
        [run_rel, obs["tree_welcome"], obs["tree_tenant"], obs["tree_products"], obs["tree_verify"], obs["tree_packs"], obs["tree_academy"], obs["tree_done"]] + parity_files)
    n2_ok = n2["sheet"] and n2["fixed_1160"] and n2["rail_244"] and n2["steps"][:5] == ["Tenant", "Products", "Verify", "Starter cases", "Done"] and n2["no_back"] and n2["cancel"] and n2["cancel_note"] and n2["cancel_before_draft_closes"] and n2["drafted_optional"] and n2["continue_open"] and n2["cancel_discards"] and len(obs["parity"]["add"]) == 2
    add("C43-N02", n2_ok, f"sheet {n2} composites {sorted(obs['parity']['add'].values())}", [run_rel, obs["tree_add"]] + sorted(obs["parity"]["add"].values()))
    lay = n3["layouts"]
    ring_ok = (n3["ring"].get("diff") or 0) > 40 and n3["ring"].get("focused") == "Create tenant"
    layouts_ok = len(lay) == 4 and all(v["overflow_px"] == 0 and v["horizontal_scrollbars"] == 0 and v["primary"]["ok"] and v["dialog"] for v in lay.values())
    add("C43-N03", n3["reach"]["field"] and n3["reach"]["primary"] and ring_ok and layouts_ok,
        f"Tab from the rail reached {n3['walk']} (field {n3['reach']['field']}, primary {n3['reach']['primary']}); ring on Create tenant {n3['ring'].get('diff')} px; "
        + "; ".join(f"{k}: web {v['geometry']['web']} viewport {v['geometry']['viewport']} overflow {v['overflow_px']} px, scroll bars {v['horizontal_scrollbars']}, primary {v['primary']}" for k, v in lay.items()),
        [run_rel] + [v["tree"] for v in lay.values()] + [v for v in obs["captures"]["layouts"].values() if v] + [v for v in obs["captures"]["wizard-ring"].values() if v])

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1", "subject": "C43", "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (System Events and the ax helper) with fixture WebView self-capture; tools/native-acceptance/acceptance_c43.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "Pixels come from the fixture build's own WKWebView snapshot; the parity composites put the prototype board (rendered at the WebView's measured size, filled=true) beside each step.",
            "Synthetic scan and management endpoints only; the Academy step is skipped (no Academy endpoint is staged).",
            "The first-run layouts are measured on the add sheet's Tenant step after the first run completed (the first-run window is gone once the tenant exists).",
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
