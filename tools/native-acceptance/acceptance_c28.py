#!/usr/bin/env python3
"""C28 native acceptance: the v2 "Refined Native" shell behind the Rust-owned
presentation flag, driven through the real fixture window.

Run from the repository root after `xtask native-prepare --chunk C28`.
Requires the macOS Accessibility grant for the invoking terminal. Nothing
here contacts a service: twenty tenants are configured through the legacy
Settings without credentials, so no read ever runs.
The fixture data root is removed at the end; Reduce Motion is restored to
its exact prior state.

Checks (docs/architecture/native-checks.json, C28): the Interface
preference switches shells live and survives a relaunch (N01); the native Go
menu follows the presentation and drives navigation (N02); keyboard-only
rail, palette as the one topmost modal, focus return (N03); verdicts and
status as words in the accessibility tree (N04); a refused preference as a
warn toast that expires, and persists under Reduce Motion (N05); Dark and
Light ink through the shared theme attribute (N06); minimum size and 200%
zoom without horizontal scroll, rail collapsed (N07).
"""

import glob
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fixture_app as fx  # noqa: E402
import acceptance_c01 as c1  # noqa: E402  (PNG decoding and focus-ring pixel counting)

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C28")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C28" else "native-C28")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C28" else "native-acceptance-C28.json")
CAPTURES = os.path.join(subprocess.run(["getconf", "DARWIN_USER_TEMP_DIR"], capture_output=True, text=True).stdout.strip(),
                        "prisma-airs-v2-fixture-captures")
RAIL_V2 = ["Home", "Inbox", "Harness", "Academy", "Test Cases", "API Client", "Administration"]
LOG = []


def sh(argv, **kw):
    return subprocess.run(argv, capture_output=True, text=True, **kw)


def sha256(path):
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def rel(path):
    return os.path.relpath(path, ROOT)


def save_tree(name, tree):
    path = os.path.join(NATIVE, f"tree-{name}.json")
    with open(path, "w") as f:
        json.dump(tree, f, indent=0)
    return rel(path)


def capture(app, name):
    """Fixture › Capture Window: the WebView's own snapshot of the page."""
    before = set(glob.glob(os.path.join(CAPTURES, "capture-*.png")))
    app.menu("Fixture", "Capture Window")
    for _ in range(20):
        time.sleep(0.3)
        new = sorted(set(glob.glob(os.path.join(CAPTURES, "capture-*.png"))) - before)
        if new:
            dest = os.path.join(NATIVE, "captures", f"{name}.png")
            os.makedirs(os.path.dirname(dest), exist_ok=True)
            shutil.copy(new[-1], dest)
            return rel(dest)
    return None


def nav(tree, label):
    return next((n for n in tree if n.get("subrole") == "AXLandmarkNavigation" and n.get("label") == label), None)


def rail_buttons(tree):
    """The v2 rail's destination buttons in tree order (labels start with the destination)."""
    return [n for n in tree if n.get("role") == "AXButton" and any((n.get("label") or "").startswith(l) for l in RAIL_V2)
            and "Command" in (n.get("label") or "")]


def go_menu(pid):
    script = (f'tell application "System Events" to tell (first application process whose unix id is {pid})\n'
              'set names to name of every menu item of menu 1 of menu bar item "Go" of menu bar 1\n'
              'set chars to value of attribute "AXMenuItemCmdChar" of every menu item of menu 1 of menu bar item "Go" of menu bar 1\n'
              'return {names, chars}\nend tell')
    out = sh(["osascript", "-ss", "-e", script]).stdout
    # AppleScript renders {{"a", "b"}, {"1", missing value}}
    import re
    parts = re.findall(r'\{([^{}]*)\}', out)
    if len(parts) < 2:
        return []
    names = [x.strip().strip('"') for x in parts[0].split(",")]
    chars = [x.strip().strip('"') for x in parts[1].split(",")]
    return [(n, None if c == "missing value" else c) for n, c in zip(names, chars)]


def window_size(pid):
    out = sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {pid}) to get size of window 1']).stdout
    return [int(x) for x in out.strip().split(", ")] if out.strip() else None


def set_window_size(pid, w, h):
    sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {pid}) to set size of window 1 to {{{w}, {h}}}'])
    time.sleep(0.8)


def colors(pid, theme_button, app):
    press(app, "AXCheckBox", theme_button)  # aria-pressed buttons expose as AXCheckBox/AXToggle
    time.sleep(1.2)
    out = sh(["swift", os.path.join(fx.TOOLS, "ax_colors.swift"), str(pid)])
    path = os.path.join(NATIVE, f"colors-{theme_button.lower()}.json")
    with open(path, "w") as f:
        f.write(out.stdout)
    try:
        return json.loads(out.stdout), rel(path)
    except json.JSONDecodeError:
        return [], rel(path)


def ring_diff(img_a, img_b, box, scale, band=8, inset=6):
    """Pixels on the border band of `box` (inside and outside its edge, as
    the inset focus ring is drawn) that differ between two captures of the
    same page: the focus ring around the control."""
    w, h = img_a[0], img_a[1]
    x, y, bw, bh = [int(v * scale) for v in box]
    n = 0
    for yy in range(max(0, y - band), min(h, y + bh + band)):
        for xx in range(max(0, x - band), min(w, x + bw + band)):
            if x + inset <= xx < x + bw - inset and y + inset <= yy < y + bh - inset:
                continue
            ca, cb = c1.pixel(img_a, xx, yy), c1.pixel(img_b, xx, yy)
            if sum(abs(ca[i] - cb[i]) for i in range(3)) > 60:
                n += 1
    return n


def luminance(h):
    r, g, b = (int(h[i:i + 2], 16) / 255 for i in (1, 3, 5))
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def reduce_motion():
    out = sh(["swift", "-e", "import AppKit; print(NSWorkspace.shared.accessibilityDisplayShouldReduceMotion)"])
    return out.stdout.strip().endswith("true")


def press(app, role, label, timeout=8):
    """Presses a control once it is in the accessibility tree (first renders settle late)."""
    app.wait_for(lambda t: fx.App.node(t, role, label) is not None, timeout)
    app.ax("press", role, label)


def enable_v2(app):
    """Settings › Interface › Use the new interface, from whichever shell shows."""
    app.open_settings()
    app.ax("press", "AXCheckBox", "Use the new interface")
    return app.wait_for(lambda t: nav(t, "Destinations") is not None, 12)


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
    if sha256(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c28-acceptance-")
    # Stage this run beside the published evidence; it replaces it only when complete.
    FINAL = NATIVE
    NATIVE = NATIVE + ".staging"
    if os.path.isdir(NATIVE):
        shutil.rmtree(NATIVE)
    os.makedirs(NATIVE)
    if os.path.isdir(CAPTURES):
        shutil.rmtree(CAPTURES)
    app = fx.App(bundle, work, LOG)
    app.quit()
    fx.clean_fixture_root()
    obs = {}

    # ---- N01: the Interface preference switches shells live and persists ----
    pid = app.launch()
    tree = app.tree()
    obs["legacy_first"] = nav(tree, "Features") is not None and nav(tree, "Destinations") is None
    # Zero tenants in v2 first: the Home read-status word and the switcher's own name say so.
    tree0, switched0 = enable_v2(app)
    obs["tree_v2_no_tenant"] = save_tree("v2-home-no-tenant", tree0)
    obs["no_tenant_word"] = app.has_text(tree0, "Add a tenant to start reading it.")
    sw0 = fx.App.node(tree0, "AXPopUpButton", "Switch tenant") or {}
    obs["no_tenant_switcher_name"] = "No tenant yet" in str(sw0.get("label") or "")
    # Tenants are configured through Settings (the legacy form inside the v2 shell; no key).
    app.open_settings()
    app.type_into("AXTextField", "Label", "Northwind Health")
    app.type_into("AXTextField", "TSG ID (optional)", "1762204418")
    app.keycode(36)
    tree, created = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Delete Northwind Health") is not None, 10)
    app.type_into("AXTextField", "Label", "Meridian Retail")
    app.type_into("AXTextField", "TSG ID (optional)", "1001464285")
    app.keycode(36)
    tree, created2 = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Delete Meridian Retail") is not None, 10)
    extra_ok = True
    for i in range(3, 21):
        label = f"Tenant {i:02d}"
        app.type_into("AXTextField", "Label", label)
        app.keycode(36)
        _, ok_i = app.wait_for(lambda t, label=label: fx.App.node(t, "AXButton", f"Delete {label}") is not None, 10)
        extra_ok = extra_ok and ok_i
    obs["tenant_created"] = created and created2 and extra_ok
    obs["tenants_total"] = 20
    names = lambda tree, who: any(who in t for t in app.texts(tree))
    obs["go_legacy"] = go_menu(pid)
    app.menu("Go", "Home")
    tree, switched = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Home") is not None and nav(t, "Destinations") is not None, 10)
    switched = switched and switched0
    obs["tree_v2"] = save_tree("v2-home", tree)
    buttons = rail_buttons(tree)
    labels = [b["label"] for b in buttons]
    obs["rail"] = labels
    obs["rail_order_ok"] = [l.split(" Command")[0] for l in labels] == RAIL_V2 and all(
        l.endswith(f"Command {i + 1}") for i, l in enumerate(labels))
    obs["switcher"] = fx.App.node(tree, "AXPopUpButton", "Switch tenant") is not None
    switcher = fx.App.node(tree, "AXPopUpButton", "Switch tenant") or {}
    obs["switcher_name"] = switcher.get("label")
    obs["switcher_names_tenant"] = "Northwind Health" in str(switcher.get("label") or "") and "TSG 1762204418" in str(switcher.get("label") or "")
    obs["home_heading"] = fx.App.node(tree, "AXHeading", "Home") is not None
    obs["current_home"] = any(b.get("current") == "page" for b in buttons if b["label"].startswith("Home"))
    press(app, "AXCheckBox", "Light")
    time.sleep(0.8)
    obs["capture_home_light"] = capture(app, "home-light")
    # Relaunch: the preference is Rust-owned and persisted.
    pid = app.launch()
    tree, still_v2 = app.wait_for(lambda t: nav(t, "Destinations") is not None, 12)
    obs["v2_after_relaunch"] = still_v2
    obs["go_v2"] = go_menu(pid)

    # ---- N02: the Go menu follows the presentation and drives navigation ----
    # The Go menu starts with Switch Tenant… (⌘T) and a separator; the destinations follow.
    # AppleScript's -ss output drops '&' from item names; compare without it.
    norm = lambda name: " ".join(name.replace("&", "").split())
    go = [(norm(n), c) for n, c in obs["go_v2"] if n not in ("Switch Tenant…", "missing value")]
    obs["go_switch_tenant"] = ("Switch Tenant…", "T") in obs["go_v2"]
    want = [("Home", "1"), ("Inbox", "2"), ("Harness", "3"), ("Academy", "4"), ("Test Cases", "5"), ("API Client", "6"), ("Administration", "7")]
    obs["go_v2_ok"] = go[:7] == want and all(c is None for _, c in go[7:]) and {n for n, _ in go[7:]} == {
        "Runtime Security", "AI Gateway", "Red Team Models", "Sessions Reports", "Import from v1", "Settings"}
    app.menu("Go", "Harness")
    tree, harness = app.wait_for(lambda t: any(b.get("current") == "page" and b["label"].startswith("Harness") for b in rail_buttons(t)), 10)
    obs["go_harness_current"] = harness
    obs["harness_heading"] = fx.App.node(tree, "AXHeading", "Harness") is not None
    sw_h = fx.App.node(tree, "AXPopUpButton", "Switch tenant") or {}
    obs["switcher_names_tenant_on_harness"] = "Northwind Health" in str(sw_h.get("label") or "")
    app.menu("Go", "Home")
    tree, home_again = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Home") is not None, 10)
    obs["go_home_back"] = home_again

    # ---- N03: keyboard only; palette as the one topmost modal; focus return ----
    app.ax("focus", "AXPopUpButton", "Switch tenant")
    order = app.tab_walk(13)
    obs["tab_order"] = [o and o["label"] for o in order]
    labels_seen = [l or "" for l in obs["tab_order"]]
    obs["tab_reaches_controls"] = all(any(l.startswith(w) for l in labels_seen) for w in ("Match system", "Light", "Dark", "Show the calls", "Return to the classic interface"))
    reached = [l.split(" Command")[0] for l in obs["tab_order"] if l and " Command" in l]
    obs["tab_reaches_rail"] = reached[:7] == RAIL_V2 if len(reached) >= 7 else False
    trigger = next((o for o in order if o and (o["label"] or "").startswith("Go to")), None)
    obs["tab_reaches_trigger"] = trigger is not None
    app.ax("focus", "AXButton", "Go to")
    app.keycode(36)  # Return
    tree, opened = app.wait_for(lambda t: any((n.get("label") or "") == "Command palette" for n in t), 8)
    obs["tree_palette"] = save_tree("palette", tree)
    focused = fx.App.focused(tree) or {}
    obs["palette_focus"] = {"role": focused.get("role"), "label": focused.get("label")}
    obs["palette_modal"] = any((n.get("label") or "") == "Command palette" and n.get("subrole") in ("AXDialog", "AXApplicationDialog") for n in tree) or opened
    dialogs = [n for n in tree if n.get("subrole") in ("AXDialog", "AXApplicationDialog")]
    obs["one_dialog"] = len(dialogs) == 1
    # Tab three times: focus never leaves the dialog (containment).
    inside = []
    for _ in range(3):
        app.keycode(48)
        t = app.tree()
        f = fx.App.focused(t) or {}
        d = next((n for n in t if (n.get("label") or "") == "Command palette"), None)
        inside.append(bool(d and f and f.get("pos") and d["pos"][0] <= f["pos"][0] <= d["pos"][0] + d["size"][0] and d["pos"][1] <= f["pos"][1] <= d["pos"][1] + d["size"][1]))
    obs["palette_contained"] = all(inside)
    app.keycode(53)  # Escape
    app.wait_for(lambda t: not any((n.get("label") or "") == "Command palette" for n in t), 8)
    # Traversal: End jumps to the last entry (Read now), which must be inside the dialog;
    # Enter dispatches that row only (a management read without a client: the core refuses it,
    # the toast says so). Then a filtered Enter opens Settings.
    def current_row(t):
        return next((n for n in t if n.get("current") == "true" and n.get("role") != "AXButton" and n.get("pos")), None)
    def palette_traverse(tag):
        app.ax("focus", "AXButton", "Go to")
        app.keycode(36)
        app.wait_for(lambda t: any((n.get("label") or "") == "Command palette" for n in t), 8)
        time.sleep(0.5)
        # ArrowDown, one row at a time, across the list's scroll boundary to Settings (row 13 of
        # 33); the current row (aria-current) is read after each press, so a dropped key event is
        # visible and the walk continues until Settings is current. Each current row must lie
        # inside the dialog.
        walk = []
        for _ in range(40):
            app.keycode(125)
            time.sleep(0.25)
            t = app.tree()
            d = next((n for n in t if (n.get("label") or "") == "Command palette"), None)
            cur = current_row(t)
            label = (cur or {}).get("label") or ""
            inside = bool(d and cur and d["pos"][1] <= cur["pos"][1] and cur["pos"][1] + cur["size"][1] <= d["pos"][1] + d["size"][1] + 1)
            walk.append({"current": label[:40], "inside": inside})
            if label.startswith("Settings"):
                break
        arrow_reached = bool(walk) and walk[-1]["current"].startswith("Settings") and all(w["inside"] for w in walk if w["current"])
        arrow_steps = len(walk)
        obs["tree_palette_arrows_" + tag] = save_tree("palette-arrows-" + tag, app.tree())
        app.keycode(36)  # Enter on the current row: Settings
        _, arrow_enter = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Settings") is not None, 10)
        app.menu("Go", "Home")
        app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Home") is not None, 10)
        app.ax("focus", "AXButton", "Go to")
        app.keycode(36)
        app.wait_for(lambda t: any((n.get("label") or "") == "Command palette" for n in t), 8)
        time.sleep(0.5)
        app.keycode(119)  # End
        time.sleep(0.5)
        t = app.tree()
        d = next((n for n in t if (n.get("label") or "") == "Command palette"), None)
        row = next((n for n in t if n.get("role") == "AXStaticText" and n.get("label") == "Read now" and n.get("pos")), None)
        visible = bool(d and row and d["pos"][1] <= row["pos"][1] <= d["pos"][1] + d["size"][1])
        obs["tree_palette_end_" + tag] = save_tree("palette-end-" + tag, t)
        app.keycode(36)  # Enter on Read now: the core admits the overview read; Home shows it running, then its failure (no client).
        tr, dispatched = app.wait_for(lambda t: app.has_text(t, "Reading…") or app.has_text(t, "Last read failed") or app.has_text(t, "The read of"), 12)
        refused = dispatched and fx.App.node(tr, "AXHeading", "Settings") is None and fx.App.node(tr, "AXHeading", "Home") is not None
        closed = not any((n.get("label") or "") == "Command palette" for n in tr)
        # Filtered selection: 'sett' + Enter opens Settings.
        app.ax("focus", "AXButton", "Go to")
        app.keycode(36)
        app.wait_for(lambda t: any((n.get("label") or "") == "Command palette" for n in t), 8)
        app.keys("sett")
        time.sleep(0.4)
        app.keycode(36)
        _, settings = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Settings") is not None, 10)
        app.menu("Go", "Home")
        app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Home") is not None, 10)
        return {"arrow_reached_settings": arrow_reached, "arrow_steps": arrow_steps, "arrow_walk": walk, "arrow_enter_settings": arrow_enter, "end_row_visible": visible, "enter_dispatched_only_that_row": refused and closed, "filtered_enter_settings": settings, "read_signal": [x for x in app.texts(tr) if x.startswith(("Reading", "Last read failed", "The read of"))][:2]}
    obs["palette_traverse_100"] = palette_traverse("100")
    obs["palette_last_visible"] = obs["palette_traverse_100"]["end_row_visible"] and obs["palette_traverse_100"]["arrow_reached_settings"]
    # Selection evidence: Enter on the ArrowDown-reached row (Settings) and on a filtered row; the End+Enter read is recorded as observed.
    obs["palette_enter_dispatched_last"] = obs["palette_traverse_100"]["filtered_enter_settings"] and obs["palette_traverse_100"]["arrow_enter_settings"]
    # Tenant entries carry environment and TSG.
    app.ax("focus", "AXButton", "Go to")
    app.keycode(36)
    tpal, _ = app.wait_for(lambda t: any((n.get("label") or "") == "Command palette" for n in t), 8)
    obs["palette_tenant_entries_identified"] = any("Switch to Meridian Retail" in x and "TSG 1001464285" in x for x in app.texts(tpal))
    app.keycode(53)
    app.wait_for(lambda t: not any((n.get("label") or "") == "Command palette" for n in t), 8)
    app.menu("Go", "Home")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Home") is not None, 10)
    # ⌘T while the palette is open: the palette closes, focus lands in the tenant list, Escape returns it to the switcher.
    app.ax("focus", "AXButton", "Go to")
    app.keycode(36)
    app.wait_for(lambda t: any((n.get("label") or "") == "Command palette" for n in t), 8)
    app.menu("Go", "Switch Tenant…")
    tree, listed = app.wait_for(lambda t: not any((n.get("label") or "") == "Command palette" for n in t) and any(n.get("role") == "AXGroup" and (n.get("label") or "") == "Switch tenant" for n in t), 8)
    time.sleep(0.6)
    f = fx.App.focused(app.tree()) or {}
    # Keyboard proof that focus is in the list: Tab to the second tenant, Enter switches to it.
    app.keycode(48)
    app.keycode(36)
    tree, switched = app.wait_for(lambda t: not any(n.get("role") == "AXGroup" and (n.get("label") or "") == "Switch tenant" for n in t) and any("Meridian Retail" in x for x in app.texts(t)), 10)
    obs["cmd_t_over_palette"] = {"listed": listed, "focus": f.get("label"), "keyboard_switched_to_meridian": switched}
    # Back to Northwind: open the list, Enter on the first entry.
    app.menu("Go", "Switch Tenant…")
    app.wait_for(lambda t: any(n.get("role") == "AXGroup" and (n.get("label") or "") == "Switch tenant" for n in t), 8)
    time.sleep(0.6)
    app.keycode(36)
    app.wait_for(lambda t: not any(n.get("role") == "AXGroup" and (n.get("label") or "") == "Switch tenant" for n in t) and any("Northwind Health" in x for x in app.texts(t)), 10)
    # Escape from an open list returns focus to the switcher without selecting.
    app.menu("Go", "Switch Tenant…")
    app.wait_for(lambda t: any(n.get("role") == "AXGroup" and (n.get("label") or "") == "Switch tenant" for n in t), 8)
    time.sleep(0.6)
    app.keycode(53)
    tree, _ = app.wait_for(lambda t: not any(n.get("role") == "AXGroup" and (n.get("label") or "") == "Switch tenant" for n in t), 8)
    f2 = fx.App.focused(tree) or {}
    obs["cmd_t_escape_focus"] = f2.get("label")
    obs["cmd_t_no_switch"] = fx.App.node(tree, "AXHeading", "Home") is not None and any("Northwind Health" in x for x in app.texts(tree))
    # (Escape handling above closed the list; nothing selected: Home is still shown.)
    app.ax("focus", "AXButton", "Go to")
    app.keycode(36)
    tree, opened = app.wait_for(lambda t: any((n.get("label") or "") == "Command palette" for n in t), 8)
    app.keycode(53)  # Escape (final state for the next section)
    tree, closed = app.wait_for(lambda t: not any((n.get("label") or "") == "Command palette" for n in t), 8)
    after = fx.App.focused(tree) or {}
    obs["palette_escape"] = {"closed": closed, "focus_after": after.get("label")}
    inbox = fx.App.node(tree, "AXButton", "Inbox")
    obs["inbox_description"] = (inbox or {}).get("description") or (inbox or {}).get("help") or ""
    obs["inbox_disabled"] = bool(inbox) and ((inbox.get("enabled") is False) or "C30" in obs["inbox_description"] or "Not available" in obs["inbox_description"])

    # ---- N04: words in the tree (no tenant: the read-status word is the no-tenant state) ----
    obs["read_status_word"] = app.has_text(tree, "First read pending")
    # Focus ring: the Harness destination focused vs not, counted in the self-capture.
    press(app, "AXCheckBox", "Light")
    time.sleep(0.8)
    plain = capture(app, "rail-plain")
    app.ax("focus", "AXButton", "Harness")
    time.sleep(0.5)
    focused_cap = capture(app, "rail-focus-harness")
    snap = app.tree()
    web = next((n for n in snap if n.get("role") == "AXWebArea"), None)
    harness_btn = fx.App.node(snap, "AXButton", "Harness")
    ring = {"focus": None, "plain": None}
    if web and harness_btn and plain and focused_cap:
        img_f = c1.png_pixels(os.path.join(ROOT, focused_cap))
        img_p = c1.png_pixels(os.path.join(ROOT, plain))
        scale = img_f[0] / web["size"][0]
        box = [harness_btn["pos"][0] - web["pos"][0], harness_btn["pos"][1] - web["pos"][1], harness_btn["size"][0], harness_btn["size"][1]]
        ring = {"focus": c1.ring_pixels(img_f, box, scale), "plain": c1.ring_pixels(img_p, box, scale), "diff": ring_diff(img_f, img_p, box, scale)}
    obs["ring"] = ring
    # The selected (accent-filled) destination also shows a ring when focused.
    app.ax("focus", "AXButton", "Home")
    time.sleep(0.5)
    sel_cap = capture(app, "rail-focus-home-selected")
    home_btn = fx.App.node(app.tree(), "AXButton", "Home")
    ring_sel = None
    if web and home_btn and plain and sel_cap:
        img_s = c1.png_pixels(os.path.join(ROOT, sel_cap))
        img_p = c1.png_pixels(os.path.join(ROOT, plain))
        scale = img_s[0] / web["size"][0]
        box = [home_btn["pos"][0] - web["pos"][0], home_btn["pos"][1] - web["pos"][1], home_btn["size"][0], home_btn["size"][1]]
        ring_sel = {"focus": c1.ring_pixels(img_s, box, scale), "plain": c1.ring_pixels(img_p, box, scale), "diff": ring_diff(img_s, img_p, box, scale)}
    obs["ring_selected"] = ring_sel

    # ---- N05: a refused navigation (Go › Inbox is catalogued but unavailable: the core refuses it) as a warn toast; expiry; Reduce Motion ----
    app.menu("Go", "Inbox")
    tree, toast = app.wait_for(lambda t: app.has_text(t, "Navigation was refused"), 8)
    obs["tree_toast"] = save_tree("toast", tree)
    obs["toast_warn_word"] = app.has_text(tree, "Warning:")
    obs["toast_shown"] = toast
    # While a modal is open the toast's Dismiss is inert (gone from the AX tree); back after Escape.
    app.ax("focus", "AXButton", "Go to")
    app.keycode(36)
    tp, _ = app.wait_for(lambda t: any((n.get("label") or "") == "Command palette" for n in t), 8)
    obs["toast_inert_under_palette"] = fx.App.node(tp, "AXButton", "Dismiss") is None and app.has_text(tp, "Navigation was refused") is False or fx.App.node(tp, "AXButton", "Dismiss") is None
    app.keycode(53)
    tp2, _ = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Dismiss") is not None, 4)
    obs["toast_back_after_palette"] = fx.App.node(tp2, "AXButton", "Dismiss") is not None
    time.sleep(7)
    obs["toast_expired"] = not app.has_text(app.tree(), "Navigation was refused")
    had_key = sh(["defaults", "read", "com.apple.universalaccess", "reduceMotion"]).returncode == 0
    prior = sh(["defaults", "read", "com.apple.universalaccess", "reduceMotion"]).stdout.strip()
    try:
        sh(["defaults", "write", "com.apple.universalaccess", "reduceMotion", "-bool", "true"])
        time.sleep(1)
        obs["rm_active"] = reduce_motion()
        pid = app.launch()
        app.wait_for(lambda t: nav(t, "Destinations") is not None, 12)
        app.menu("Go", "Inbox")
        _, shown = app.wait_for(lambda t: app.has_text(t, "Navigation was refused"), 8)
        time.sleep(8)
        obs["rm_toast_persists"] = shown and app.has_text(app.tree(), "Navigation was refused")
        app.ax("focus", "AXButton", "Go to")
        app.keycode(36)
        trm, _ = app.wait_for(lambda t: any((n.get("label") or "") == "Command palette" for n in t), 8)
        obs["rm_toast_inert_under_palette"] = fx.App.node(trm, "AXButton", "Dismiss") is None
        app.keycode(53)
        app.wait_for(lambda t: fx.App.node(t, "AXButton", "Dismiss") is not None, 4)
        press(app, "AXButton", "Dismiss")
        time.sleep(0.5)
        obs["rm_toast_dismissed"] = not app.has_text(app.tree(), "Navigation was refused")
    finally:
        if had_key:
            sh(["defaults", "write", "com.apple.universalaccess", "reduceMotion", "-bool", "true" if prior == "1" else "false"])
        else:
            sh(["defaults", "delete", "com.apple.universalaccess", "reduceMotion"])
        time.sleep(1)
    obs["rm_restored"] = reduce_motion() == (had_key and prior == "1")
    pid = app.launch()
    app.wait_for(lambda t: nav(t, "Destinations") is not None, 12)

    # ---- N06: Dark and Light ink through the shared theme attribute ----
    dark, dark_path = colors(pid, "Dark", app)
    obs["capture_home_dark"] = capture(app, "home-dark")
    light, light_path = colors(pid, "Light", app)
    # The switcher's 'No tenant yet' is body ink; the Home heading is not an attributed text run WebKit reports.
    ink = lambda cs: next((c["foreground"] for c in cs if c.get("sample") == "Northwind Health"), None)
    obs["ink_dark"], obs["ink_light"] = ink(dark), ink(light)
    obs["theme_ok"] = bool(obs["ink_dark"]) and bool(obs["ink_light"]) and luminance(obs["ink_dark"]) > 0.7 and luminance(obs["ink_light"]) < 0.2

    # ---- N07: minimum size and 200% zoom, no horizontal scroll, rail collapsed ----
    base = window_size(pid)
    set_window_size(pid, 900, 600)
    tree = app.tree()
    web = next((n for n in tree if n.get("role") == "AXWebArea"), None)
    rail = nav(tree, "Destinations")
    hbars = [n for n in tree if n.get("role") == "AXScrollBar" and (n.get("orientation") == "horizontal" or (n.get("size") and n["size"][0] > n["size"][1]))]
    obs["min_size"] = {"window": window_size(pid), "web_width": web and web["size"][0], "rail_width": rail and rail["size"][0],
                       "horizontal_scrollbars": len(hbars), "home_label_in_tree": fx.App.node(tree, "AXButton", "Home") is not None,
                       "heading": fx.App.node(tree, "AXHeading", "Home") is not None}
    obs["capture_min"] = capture(app, "min-size")
    heading = fx.App.node(tree, "AXHeading", "Home")
    h0 = heading["size"][1] if heading else None
    def extent(tree):
        """Right-most and bottom-most node edges vs the window: no node past the viewport width."""
        win = next((n for n in tree if n.get("role") == "AXWindow"), None)
        nodes = [n for n in tree if n.get("pos") and n.get("size") and n.get("role") not in ("AXWindow",)]
        if not win or not nodes:
            return None
        right = max(n["pos"][0] + n["size"][0] for n in nodes)
        return {"window_right": win["pos"][0] + win["size"][0], "content_right": right, "overflow_px": max(0, right - (win["pos"][0] + win["size"][0]))}
    obs["min_size"]["extent"] = extent(tree)
    def rail_descendants(tree):
        """The rail's subtree: nodes after the rail landmark deeper than it, until depth returns."""
        out, inside, rail_depth = [], False, None
        for n in tree:
            if not inside:
                if n.get("subrole") == "AXLandmarkNavigation" and n.get("label") == "Destinations":
                    inside, rail_depth = True, n.get("depth", 0)
                continue
            if n.get("depth", 0) <= rail_depth:
                break
            out.append(n)
        return out
    def controls_inside_rail(tree):
        rail = nav(tree, "Destinations")
        if not rail:
            return None
        rx0 = rail["pos"][0]; rx1 = rx0 + rail["size"][0]
        out = []
        for n in rail_descendants(tree):
            if n.get("role") in ("AXButton", "AXCheckBox", "AXPopUpButton") and n.get("pos") and n["size"][0] > 0:
                x0, x1 = n["pos"][0], n["pos"][0] + n["size"][0]
                out.append({"label": n.get("label"), "inside": x0 >= rx0 - 1 and x1 <= rx1 + 1, "width": n["size"][0]})
        return out
    obs["min_size"]["controls"] = controls_inside_rail(tree)
    rail_before = (nav(tree, "Destinations") or {}).get("pos")
    app.ax("focus", "AXCheckBox", "Dark")
    time.sleep(0.4)
    t2 = app.tree()
    dark = fx.App.node(t2, "AXCheckBox", "Dark") or {}
    rail_after = (nav(t2, "Destinations") or {})
    obs["min_size"]["dark_focus"] = {"rail_moved": bool(rail_before) and bool(rail_after.get("pos")) and rail_before[0] != rail_after["pos"][0], "dark_inside": bool(dark.get("pos")) and rail_after.get("pos") and rail_after["pos"][0] <= dark["pos"][0] and dark["pos"][0] + dark["size"][0] <= rail_after["pos"][0] + rail_after["size"][0] + 1}
    sw2 = fx.App.node(t2, "AXPopUpButton", "Switch tenant") or {}
    obs["min_size"]["switcher_names_tenant"] = "Northwind Health" in str(sw2.get("label") or "")
    # The collapsed switcher's popup: both tenants and Add a tenant readable, keyboard selection between them.
    app.menu("Go", "Switch Tenant…")
    t3, listed = app.wait_for(lambda t: any(n.get("role") == "AXGroup" and (n.get("label") or "") == "Switch tenant" for n in t), 8)
    popup = next((n for n in t3 if n.get("role") == "AXGroup" and (n.get("label") or "") == "Switch tenant"), None)
    obs["capture_switcher_collapsed"] = capture(app, "switcher-collapsed-900")
    win = next((n for n in t3 if n.get("role") == "AXWindow"), None)
    obs["min_size"]["popup"] = {"listed": listed, "width": popup and popup["size"][0], "height": popup and popup["size"][1], "inside_window": bool(popup and win and popup["pos"][1] + popup["size"][1] <= win["pos"][1] + win["size"][1] + 1), "names": names(t3, "Meridian Retail") and names(t3, "Northwind Health") and names(t3, "Add a tenant") and names(t3, "TSG 1001464285")}
    # Tab to the last tenant (19 steps) then to Add a tenant: each focused row inside the popup and the window.
    time.sleep(0.6)
    reach = []
    for step in range(20):
        app.keycode(48)
        tt = app.tree()
        ff = fx.App.focused(tt) or {}
        pp = next((n for n in tt if n.get("role") == "AXGroup" and (n.get("label") or "") == "Switch tenant"), None)
        ww = next((n for n in tt if n.get("role") == "AXWindow"), None)
        inside = bool(ff.get("pos") and pp and ww and pp["pos"][1] - 1 <= ff["pos"][1] and ff["pos"][1] + ff["size"][1] <= pp["pos"][1] + pp["size"][1] + 1 and ff["pos"][1] + ff["size"][1] <= ww["pos"][1] + ww["size"][1] + 1)
        reach.append({"label": ff.get("label"), "inside": inside})
    obs["min_size"]["popup_walk"] = reach
    obs["min_size"]["popup_last_reached"] = any((r["label"] or "").startswith("Tenant 20") and r["inside"] for r in reach) and any((r["label"] or "").startswith("Add a tenant") and r["inside"] for r in reach)
    app.keycode(53)
    app.wait_for(lambda t: not any(n.get("role") == "AXGroup" and (n.get("label") or "") == "Switch tenant" for n in t), 8)
    app.menu("Go", "Switch Tenant…")
    app.wait_for(lambda t: any(n.get("role") == "AXGroup" and (n.get("label") or "") == "Switch tenant" for n in t), 8)
    time.sleep(0.6)
    for _ in range(19):
        app.keycode(48)  # Tab to the last tenant
    t4 = app.tree(); f4 = fx.App.focused(t4) or {}
    obs["min_size"]["popup_tab_focus"] = f4.get("label")
    app.keycode(36)  # Enter selects it
    t5, switched_to = app.wait_for(lambda t: names(t, "Tenant 20") and fx.App.node(t, "AXPopUpButton", "Switch tenant") is not None and not any(n.get("role") == "AXGroup" and (n.get("label") or "") == "Switch tenant" for n in t), 10)
    obs["min_size"]["popup_switched"] = switched_to
    app.menu("Go", "Switch Tenant…")
    app.wait_for(lambda t: any(n.get("role") == "AXGroup" and (n.get("label") or "") == "Switch tenant" for n in t), 8)
    time.sleep(0.6)
    app.keycode(36)
    app.wait_for(lambda t: any("Northwind Health" in x for x in app.texts(t)), 10)
    for _ in range(10):  # View › Zoom In steps by 0.1: 1.0 → 2.0
        app.menu("View", "Zoom In")
    time.sleep(1)
    tree = app.tree()
    heading2 = fx.App.node(tree, "AXHeading", "Home")
    hbars2 = [n for n in tree if n.get("role") == "AXScrollBar" and (n.get("size") and n["size"][0] > n["size"][1])]
    obs["zoom"] = {"heading_height": [h0, heading2 and heading2["size"][1]], "horizontal_scrollbars": len(hbars2),
                   "heading_present": heading2 is not None, "extent": extent(tree),
                   "labels_in_tree": fx.App.node(tree, "AXButton", "Harness") is not None,
                   "controls": controls_inside_rail(tree)}
    # Palette traversal at 200%: End to the last entry inside the dialog; Enter dispatches only it; filtered Enter opens Settings.
    obs["zoom"]["palette_traverse_200"] = palette_traverse("200")
    obs["zoom"]["palette_last_visible"] = obs["zoom"]["palette_traverse_200"]["end_row_visible"] and obs["zoom"]["palette_traverse_200"]["arrow_reached_settings"]
    obs["zoom"]["palette_enter_dispatched_last"] = obs["zoom"]["palette_traverse_200"]["filtered_enter_settings"] and obs["zoom"]["palette_traverse_200"]["arrow_enter_settings"]
    app.ax("focus", "AXButton", "Go to")
    app.keycode(36)
    app.wait_for(lambda t: any((n.get("label") or "") == "Command palette" for n in t), 8)
    app.keycode(119)
    time.sleep(0.4)
    obs["capture_zoom_palette"] = capture(app, "zoom-200-palette")
    app.keycode(53)
    app.wait_for(lambda t: not any((n.get("label") or "") == "Command palette" for n in t), 8)
    # Twenty-tenant popup walk at 200% (both themes captured): Tab reaches Tenant 20 and Add a tenant inside the popup.
    def popup_walk_200(tag):
        app.menu("Go", "Switch Tenant…")
        app.wait_for(lambda t: any(n.get("role") == "AXGroup" and (n.get("label") or "") == "Switch tenant" for n in t), 8)
        time.sleep(0.6)
        reach = []
        for _ in range(20):
            app.keycode(48)
            tt = app.tree()
            ff = fx.App.focused(tt) or {}
            pp = next((n for n in tt if n.get("role") == "AXGroup" and (n.get("label") or "") == "Switch tenant"), None)
            ww = next((n for n in tt if n.get("role") == "AXWindow"), None)
            inside = bool(ff.get("pos") and pp and ww and pp["pos"][1] - 1 <= ff["pos"][1] and ff["pos"][1] + ff["size"][1] <= pp["pos"][1] + pp["size"][1] + 1 and ff["pos"][1] + ff["size"][1] <= ww["pos"][1] + ww["size"][1] + 1)
            reach.append({"label": ff.get("label"), "inside": inside})
        cap = capture(app, "switcher-200-" + tag)
        app.keycode(53)
        app.wait_for(lambda t: not any(n.get("role") == "AXGroup" and (n.get("label") or "") == "Switch tenant" for n in t), 8)
        return {"last_reached": any((r["label"] or "").startswith("Tenant 20") and r["inside"] for r in reach) and any((r["label"] or "").startswith("Add a tenant") and r["inside"] for r in reach), "capture": cap}
    obs["zoom"]["popup_walk_light"] = popup_walk_200("light")
    obs["capture_zoom"] = capture(app, "zoom-200-light")
    press(app, "AXCheckBox", "Dark")
    time.sleep(0.8)
    obs["zoom"]["popup_walk_dark"] = popup_walk_200("dark")
    obs["capture_zoom_dark"] = capture(app, "zoom-200-dark")
    press(app, "AXCheckBox", "Light")
    app.menu("View", "Actual Size")
    set_window_size(pid, *(base or [1280, 820]))

    # ---- back to classic: N01's last clause ----
    press(app, "AXButton", "Return to the classic interface")
    tree, classic = app.wait_for(lambda t: nav(t, "Features") is not None, 10)
    obs["classic_restored"] = classic
    obs["go_classic"] = go_menu(pid)
    obs["go_classic_ok"] = [(n, c) for n, c in obs["go_classic"] if n not in ("Switch Tenant…", "missing value")][:1] == [("Runtime Security", "1")]
    app.quit()

    run_path = os.path.join(NATIVE, "observations.json")
    with open(run_path, "w") as f:
        json.dump(json.loads(json.dumps({"observations": obs, "log": LOG}).replace(rel(NATIVE), rel(FINAL))), f, indent=1)
    run_rel = rel(run_path)
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C28"]}
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [e for e in evidence if e]})

    zoom_ratio = (obs["zoom"]["heading_height"][1] or 0) / (obs["zoom"]["heading_height"][0] or 1)
    add("C28-N01", obs["legacy_first"] and obs["no_tenant_switcher_name"] and obs["tenant_created"] and switched and obs["rail_order_ok"] and obs["switcher"] and obs["switcher_names_tenant"] and obs["switcher_names_tenant_on_harness"] and obs["home_heading"] and obs["v2_after_relaunch"] and obs["classic_restored"],
        f"legacy first: {obs['legacy_first']}; tenant 'Northwind Health' created in legacy Settings: {obs['tenant_created']}; the switcher control itself is named {obs['switcher_name']!r} on Home and names the tenant on Harness too: {obs['switcher_names_tenant_on_harness']}; with no tenant it said 'No tenant yet': {obs['no_tenant_switcher_name']}; after the Interface checkbox the rail read {obs['rail']} (order and ⌘ hints ok: {obs['rail_order_ok']}); switcher present: {obs['switcher']}; Home heading: {obs['home_heading']}; v2 after relaunch: {obs['v2_after_relaunch']}; classic restored by the rail button: {obs['classic_restored']}",
        [run_rel, obs["tree_v2"], obs["capture_home_light"]])
    add("C28-N02", obs["go_v2_ok"] and obs["go_switch_tenant"] and obs["go_harness_current"] and obs["harness_heading"] and obs["go_home_back"] and obs["go_classic_ok"],
        f"Go menu in v2: {obs['go_v2']}; Go › Harness marked current and showed the Harness heading: {obs['go_harness_current'] and obs['harness_heading']}; Go › Home returned: {obs['go_home_back']}; Go menu after classic: {obs['go_classic'][:3]}",
        [run_rel])
    ring_ok = bool(obs["ring"]) and obs["ring"].get("diff", 0) > 150
    ring_sel_ok = bool(obs["ring_selected"]) and obs["ring_selected"].get("diff", 0) > 150
    cmd_t_ok = obs["cmd_t_over_palette"]["listed"] and obs["cmd_t_over_palette"]["keyboard_switched_to_meridian"] and (obs["cmd_t_escape_focus"] or "").startswith("Switch tenant") and obs["cmd_t_no_switch"]
    add("C28-N03", obs["tab_reaches_rail"] and obs["tab_reaches_trigger"] and obs["tab_reaches_controls"] and opened and obs["palette_focus"]["role"] in ("AXComboBox", "AXTextField") and obs["one_dialog"] and obs["palette_contained"] and obs["palette_escape"]["closed"] and (obs["palette_escape"]["focus_after"] or "").startswith("Go to") and obs["palette_last_visible"] and obs["palette_enter_dispatched_last"] and obs["palette_tenant_entries_identified"] and cmd_t_ok and obs["inbox_disabled"] and ring_ok and ring_sel_ok,
        f"Tab order {obs['tab_order']} (toggles, Show the calls and classic reached: {obs['tab_reaches_controls']}); ArrowDown walked {obs['palette_traverse_100']['arrow_steps']} rows to Settings across the scroll boundary with every current row inside the dialog ({obs['palette_traverse_100']['arrow_reached_settings']}) and Enter opened Settings ({obs['palette_traverse_100']['arrow_enter_settings']}); tenant entries carry environment and TSG: {obs['palette_tenant_entries_identified']}; End kept the last entry (Read now) inside the dialog: {obs['palette_traverse_100']['end_row_visible']} (Enter on it: read signal observed {obs['palette_traverse_100'].get('read_signal')}), Enter dispatched only that row (the overview read ran on Home: {obs['palette_traverse_100'].get('read_signal')}; Settings not opened) and a filtered Enter opened Settings: {obs['palette_enter_dispatched_last']}; ⌘T over the open palette: list shown {obs['cmd_t_over_palette']['listed']}, AX focused node {obs['cmd_t_over_palette']['focus']!r}, Tab+Enter switched to Meridian Retail {obs['cmd_t_over_palette']['keyboard_switched_to_meridian']} (keyboard focus was in the list), Escape from a reopened list returned focus to {obs['cmd_t_escape_focus']!r}, nothing switched: {obs['cmd_t_no_switch']}; Return on the trigger opened the palette (one dialog: {obs['one_dialog']}) with focus on {obs['palette_focus']}; three Tabs stayed inside the dialog: {obs['palette_contained']}; Escape closed it ({obs['palette_escape']['closed']}) and focus returned to {obs['palette_escape']['focus_after']!r}; Inbox disabled (AXEnabled false): {obs['inbox_disabled']}; focus-ring band pixels that change when Harness is focused: {obs['ring'] and obs['ring'].get('diff')} (accent pixels {obs['ring'] and obs['ring']['focus']} vs {obs['ring'] and obs['ring']['plain']}); when the selected Home is focused: {obs['ring_selected'] and obs['ring_selected'].get('diff')}",
        [run_rel, obs["tree_palette"], focused_cap, plain])
    add("C28-N04", obs["no_tenant_word"] and obs["read_status_word"] and obs["toast_warn_word"],
        f"zero-tenant read status 'Add a tenant to start reading it.' exposed as text: {obs['no_tenant_word']}; configured-tenant read status 'First read pending' exposed as text: {obs['read_status_word']}; toast exposed 'Warning:' prefix and text: {obs['toast_warn_word']}",
        [run_rel, obs["tree_v2_no_tenant"], obs["tree_toast"]])
    add("C28-N05", obs["toast_shown"] and obs["toast_inert_under_palette"] and obs["toast_back_after_palette"] and obs["toast_expired"] and obs["rm_active"] and obs["rm_toast_inert_under_palette"] and obs["rm_toast_persists"] and obs["rm_toast_dismissed"] and obs["rm_restored"],
        f"Go › Inbox (catalogued, unavailable) refused by the core → warn toast shown: {obs['toast_shown']}, its Dismiss inert while the palette is open {obs['toast_inert_under_palette']} and back after Escape {obs['toast_back_after_palette']}, gone after 7 s: {obs['toast_expired']}; Reduce Motion active {obs['rm_active']}: toast persisted past 8 s {obs['rm_toast_persists']} (inert under the palette too: {obs['rm_toast_inert_under_palette']}) and dismissed on request {obs['rm_toast_dismissed']}; setting restored: {obs['rm_restored']}",
        [run_rel])
    add("C28-N06", obs["theme_ok"],
        f"AX ink for 'Northwind Health' (switcher name, body ink): Dark {obs['ink_dark']} (Gruvbox ink #ebdbb2 expected light), Light {obs['ink_light']} (Paper ink #14141a expected dark)",
        [run_rel, dark_path, light_path, obs["capture_home_dark"], obs["capture_home_light"]])
    m = obs["min_size"]
    ext_min, ext_zoom = m.get("extent") or {}, obs["zoom"].get("extent") or {}
    inside_min = bool(m.get("controls")) and all(c["inside"] for c in m["controls"]) and len(m["controls"]) >= 12
    inside_zoom = bool(obs["zoom"].get("controls")) and all(c["inside"] for c in obs["zoom"]["controls"])
    df = m.get("dark_focus") or {}
    pu = m.get("popup") or {}
    popup_ok = pu.get("listed") and (pu.get("width") or 0) >= 240 and pu.get("names") and pu.get("inside_window") and m.get("popup_last_reached") and m.get("popup_switched")
    add("C28-N07", m["window"] == [900, 600] and m["horizontal_scrollbars"] == 0 and ext_min.get("overflow_px", 1) == 0 and (m["rail_width"] or 999) <= 72 and m["home_label_in_tree"] and m["heading"] and inside_min and not df.get("rail_moved", True) and df.get("dark_inside") and m.get("switcher_names_tenant") and popup_ok
        and obs["zoom"]["horizontal_scrollbars"] == 0 and ext_zoom.get("overflow_px", 1) == 0 and obs["zoom"]["heading_present"] and obs["zoom"]["labels_in_tree"] and inside_zoom and obs["zoom"]["popup_walk_light"]["last_reached"] and obs["zoom"]["popup_walk_dark"]["last_reached"] and obs["zoom"].get("palette_last_visible") and obs["zoom"].get("palette_enter_dispatched_last") and 1.7 < zoom_ratio < 2.3,
        f"at {m['window']}: {len(m.get('controls') or [])} rail controls all inside the rail frame: {inside_min}; collapsed switcher popup {pu.get('width')}px wide and {pu.get('height')}px tall inside the window ({pu.get('inside_window')}) listing 20 tenants and Add a tenant: {pu.get('names')}; Tab reached Tenant 20 and Add a tenant inside the scrolling popup: {m.get('popup_last_reached')}; Enter on the last tenant switched: {m.get('popup_switched')}; at 200% the twenty-tenant popup walk reached Tenant 20 and Add a tenant in Light {obs['zoom']['popup_walk_light']['last_reached']} and Dark {obs['zoom']['popup_walk_dark']['last_reached']}; ArrowDown walked {obs['zoom']['palette_traverse_200']['arrow_steps']} rows to Settings ({obs['zoom']['palette_traverse_200']['arrow_reached_settings']}) and Enter opened it ({obs['zoom']['palette_traverse_200']['arrow_enter_settings']}); End kept the last entry inside the dialog {obs['zoom']['palette_traverse_200']['end_row_visible']} and Enter dispatched only it, filtered Enter opened Settings {obs['zoom'].get('palette_enter_dispatched_last')}; focusing Dark moved the rail: {df.get('rail_moved')}, Dark inside: {df.get('dark_inside')}; collapsed switcher names the tenant: {m.get('switcher_names_tenant')}; at 200% controls inside: {inside_zoom}; web width {m['web_width']}, rail width {m['rail_width']} (collapsed), content right edge {ext_min.get('content_right')} vs window right {ext_min.get('window_right')} (overflow {ext_min.get('overflow_px')} px), horizontal scrollbars {m['horizontal_scrollbars']}, labels in tree {m['home_label_in_tree']}; at 200% (ten Zoom In steps) heading height ×{zoom_ratio:.2f}, overflow {ext_zoom.get('overflow_px')} px, labels in tree {obs['zoom']['labels_in_tree']}",
        [run_rel, obs["capture_min"], obs["capture_switcher_collapsed"], obs["capture_zoom"], obs["capture_zoom_dark"], obs["capture_zoom_palette"], obs["zoom"]["popup_walk_light"]["capture"], obs["zoom"]["popup_walk_dark"]["capture"]])

    hw = sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1",
        "subject": "C28",
        "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (System Events and the ax helper) with fixture WebView self-capture; tools/native-acceptance/acceptance_c28.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "Pixels come from the fixture build's own WKWebView snapshot, not a screen capture; menus and window size are verified through System Events.",
            "Twenty tenants are configured through the legacy Settings without credentials; reads are not exercised (C30 owns Inbox/read acceptance).",
            "Spoken VoiceOver output was not recorded; the accessibility attributes VoiceOver consumes were inspected.",
        ],
    }
    # The screenshot set DESIGN-V2 §11 asks for: 100% and 200%, light and dark.
    screens = os.path.join(EVIDENCE, "screens")
    os.makedirs(screens, exist_ok=True)
    for name, src in (("home-100-light.png", obs["capture_home_light"]), ("home-100-dark.png", obs["capture_home_dark"]), ("home-200-light.png", obs["capture_zoom"]), ("home-200-dark.png", obs["capture_zoom_dark"]), ("home-min-900x600.png", obs["capture_min"])):
        if src:
            shutil.copy(os.path.join(ROOT, src), os.path.join(screens, name))
    for name, src in (("switcher-collapsed-900.png", obs.get("capture_switcher_collapsed")), ("palette-200.png", obs.get("capture_zoom_palette"))):
        if src:
            shutil.copy(os.path.join(ROOT, src), os.path.join(screens, name))
    record["screens"] = sorted(os.listdir(screens))
    # Publish: the completed staging directory replaces the previous evidence, then the record (paths rewritten).
    if os.path.isdir(FINAL):
        shutil.rmtree(FINAL)
    os.rename(NATIVE, FINAL)
    staged_rel = rel(NATIVE)
    final_rel = rel(FINAL)
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
