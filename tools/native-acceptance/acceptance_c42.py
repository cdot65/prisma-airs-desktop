#!/usr/bin/env python3
"""C42 native acceptance (D059): the Academy as the prototype board draws it
(R2App start=academy) around Academy's own learner pages, driven through
the real fixture window against the synthetic Academy endpoint
(academy_server.py with academy_catalog.py) and the synthetic scan and
management endpoints.

Run from the repository root after `xtask native-prepare --chunk C42`.
Requires the macOS Accessibility grant. Synthetic services only; the fixture
data root is removed at the end.

Checks (docs/architecture/native-checks.json, C42): the board (title row
with the account pill, the frame with the learner pages and Your place,
the Learning paths row from the catalog, a path card opening its page) as
a guest and signed in, the learner chrome inside the region, the theme
following the appearance (N01); keyboard reach, the focus ring and the
measured layouts in both themes (N02). The parity composites are captured
from the signed-in catalog state.
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
import academy_catalog  # noqa: E402
import acceptance_c01 as c1  # noqa: E402
import acceptance_c08 as c8  # noqa: E402
import acceptance_c28 as c28  # noqa: E402
import acceptance_c29 as c29  # noqa: E402
import acceptance_c31 as c31  # noqa: E402

ROOT = os.getcwd()
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence/C42")
NATIVE = os.path.join(EVIDENCE, "native")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json")
PARITY = os.path.join(EVIDENCE, "parity")
DC = os.path.join(ROOT, "tools/parity")
LOG = []
TITLE_BAR = 28
CATALOG = "What will you take up next?"
GUEST_NOTE = "Browsing as a guest"
# Learner chrome that must never appear outside the Academy region.
LEARNER_MARKERS = [GUEST_NOTE, "Reading progress", "Complete & continue", "Academy location", "Start knowledge check"]


def rel(path):
    return os.path.relpath(path, ROOT)


def heading(tree, label, level=None):
    return next((n for n in tree if n.get("role") == "AXHeading" and (n.get("label") or "") == label and (level is None or n.get("value") == level)), None)


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


def links(nodes, prefix=""):
    out = []
    for n in nodes:
        label = n.get("label") or ""
        if n.get("role") == "AXLink" and label.startswith(prefix) and label not in out:
            out.append(label)
    return out


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
             and n.get("role") in ("AXButton", "AXStaticText", "AXTextField", "AXTextArea", "AXLink", "AXHeading", "AXCheckBox", "AXRadioButton")]
    right = max(n["pos"][0] + n["size"][0] for n in nodes) if nodes and win else None
    hbars = [n for n in tree if n.get("role") == "AXScrollBar" and (n.get("orientation") == "horizontal" or (n.get("size") and n["size"][0] > n["size"][1]))]
    return {"overflow_px": max(0, right - (win["pos"][0] + win["size"][0])) if right is not None else None, "horizontal_scrollbars": len(hbars)}, tree


def board_usable(tree, zoom):
    """The board at this zoom: the two frame cards, the Learning paths heading and a path card at least 32 CSS px tall."""
    pages = group(tree, "Academy pages")
    place = group(tree, "Your place")
    paths_h = heading(tree, "Learning paths")
    card = next((n for n in tree if n.get("role") == "AXLink" and (n.get("label") or "").startswith("Open ") and n.get("size")), None)
    out = {"pages": pages is not None, "place": place is not None, "paths": paths_h is not None, "card_size": card.get("size") if card else None,
           "card_ok": bool(card and card["size"][1] >= 32 * zoom - 1), "zoom": zoom}
    out["ok"] = out["pages"] and out["place"] and out["paths"] and out["card_ok"]
    return out


def ring_on_link(app, label, plain_cap, name):
    """Ring pixels around link `label` focused, against a capture with it unfocused (c31.ring_on for links)."""
    app.ax("focus", "AXLink", label)
    time.sleep(0.5)
    focused_cap = c29.capture(app, name)
    snap = app.tree()
    web = next((n for n in snap if n.get("role") == "AXWebArea"), None)
    link = next((n for n in snap if n.get("role") == "AXLink" and (n.get("label") or "") == label and n.get("pos") and n.get("size")), None)
    if not (web and link and plain_cap and focused_cap):
        return {"diff": 0, "focused": (fx.App.focused(snap) or {}).get("label")}
    img_f = c1.png_pixels(os.path.join(ROOT, focused_cap))
    img_p = c1.png_pixels(os.path.join(ROOT, plain_cap))
    scale = img_f[0] / web["size"][0]
    box = [link["pos"][0] - web["pos"][0], link["pos"][1] - web["pos"][1], link["size"][0], link["size"][1]]
    return {"diff": c28.ring_diff(img_f, img_p, box, scale), "focused": (fx.App.focused(snap) or {}).get("label"), "plain": plain_cap, "capture": focused_cap}


def composite(app, theme, web_size, prepared):
    name = f"academy-{theme.lower()}"
    cap = c29.capture(app, f"parity-{name}")
    if not cap:
        raise SystemExit(f"no capture for {name}")
    os.makedirs(os.path.join(PARITY, "app"), exist_ok=True)
    os.makedirs(os.path.join(PARITY, "proto"), exist_ok=True)
    with open(os.path.join(PARITY, "bundle.json"), "w") as f:
        json.dump({"bundle": prepared["bundle"], "executable_sha256": prepared["executable_sha256"], "from": "native-prepare.json",
                   "captured_by": "tools/native-acceptance/acceptance_c42.py", "captured_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}, f, indent=2)
        f.write("\n")
    app_png = os.path.join(PARITY, "app", f"{name}.png")
    shutil.copy(os.path.join(ROOT, cap), app_png)
    web_w, web_h = web_size
    proto_png = os.path.join(PARITY, "proto", f"{name}.png")
    subprocess.run([sys.executable, "-I", os.path.join(DC, "dc_render.py"), "--board", "R2App", "--prop", "start=academy",
                    "--prop", f"theme={theme.lower()}", "--width", str(web_w), "--height", str(web_h), "--out", proto_png], check=True, capture_output=True)
    out = os.path.join(PARITY, f"{name}.png")
    subprocess.run([sys.executable, "-I", os.path.join(DC, "compose.py"), "--left", proto_png, "--right", app_png,
                    "--out", out, "--title", f"Academy · {theme.lower()} · {web_w}×{web_h}"], check=True, capture_output=True)
    return rel(out)


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
    work = tempfile.mkdtemp(prefix="airs-v2-c42-acceptance-")
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
    academy = fx.AcademyEndpoint(os.path.join(work, "academy"))
    with open(os.path.join(academy.dir, "catalog.json"), "w") as f:
        json.dump(academy_catalog.build(ROOT), f)
    academy.install()
    academy.start()
    try:
        return exercise(app, endpoint, mgmt, academy, prepared, work, FINAL)
    finally:
        academy.stop()
        endpoint.stop()
        mgmt.stop()
        shutil.rmtree(work, ignore_errors=True)


def exercise(app, endpoint, mgmt, academy, prepared, work, FINAL):
    global NATIVE
    obs = {"captures": {}, "origin": academy.origin}
    app.launch()
    pid = app.pid
    app.open_settings()
    app.type_into("AXTextField", "TSG ID (optional)", c8.TSG)
    app.create_custom_tenant("Northwind Health", endpoint.origin)
    app.save_key(c8.SCAN_KEY)
    c8.save_client(app)
    c29.press(app, "AXCheckBox", "Use the new interface")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Home") is not None and c29.nav(t, "Destinations") is not None, 15)
    size_window(app, pid, 1440, 900)

    def go_academy():
        app.menu("Go", "Academy")
        return app.wait_for(lambda t: heading(t, "AI Security Academy") is not None and heading(t, CATALOG) is not None, 20)

    # ---- N01: the board, as a guest ----
    n1 = {}
    tree, n1["guest_screen"] = go_academy()
    n1["lead"] = app.has_text(tree, "Learn each product with your own tenant beside you")
    n1["guest_pill"] = app.has_text(tree, GUEST_NOTE)
    pages = group(tree, "Academy pages")
    place = group(tree, "Your place")
    n1["frame"] = pages is not None and place is not None
    n1["catalog_in_pages"] = heading(within(tree, pages), CATALOG) is not None
    n1["guest_note"] = any("Sign in from Settings to track progress" in (x.get("label") or "") for x in within(tree, place))
    n1["guest_open_settings"] = any(x.get("role") == "AXButton" and (x.get("label") or "") == "Open Settings" for x in within(tree, place))
    sections = group(tree, "Academy sections")
    n1["guest_sections"] = links(within(tree, sections)) if sections else []
    paths_h = heading(tree, "Learning paths")
    n1["paths_heading"] = paths_h is not None
    n1["path_cards"] = links(tree, "Open ")
    n1["path_units"] = any(" units" in (x.get("label") or "") for x in tree)
    # A guest reads the catalog with no credential.
    guest_calls = [r for r in academy.requests() if r.get("procedure")]
    n1["guest_calls"] = sorted({c["procedure"] for c in guest_calls})
    n1["guest_no_credential"] = bool(guest_calls) and all(not c.get("authorization") for c in guest_calls)
    n1["paths_list_read"] = any(c["procedure"] == "paths.list" for c in guest_calls)
    # Learner chrome stays inside the region.
    region = group(tree, "Academy")
    inside = within(tree, region) if region else []
    n1["chrome_inside"] = region is not None and all(not any(m in (x.get("label") or "") for m in LEARNER_MARKERS) for x in tree if x not in inside and x is not region)
    obs["tree_guest"] = c29.save_tree("academy-guest", tree)
    obs["captures"]["academy-guest"] = {"light": c29.capture(app, "academy-guest-light")}
    # A path card opens its page within the destination.
    if n1["path_cards"]:
        app.ax("press", "AXLink", n1["path_cards"][0])
        tree, n1["path_opened"] = app.wait_for(lambda t: heading(t, CATALOG) is None and group(t, "Academy pages") is not None and heading(t, "AI Security Academy") is not None, 15)
        trail = group(tree, "Academy location")
        n1["trail"] = trail is not None
        obs["tree_path"] = c29.save_tree("academy-path", tree)
    else:
        n1["path_opened"] = False
        n1["trail"] = False
    # ---- sign in from Settings; the account generation ----
    academy.mode(approval="granted", displayName="Ada Lovelace")
    app.open_settings()
    app.ax("press", "AXButton", "Sign in to Academy")
    _, n1["signed_in"] = app.wait_for(lambda t: app.has_text(t, "Signed in to Academy as"), 20)
    mark = len(academy.requests())
    app.menu("Go", "Home")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Home") is not None, 10)
    tree, n1["account_screen"] = go_academy()
    time.sleep(1)
    tree = app.tree()
    n1["account_pill"] = app.has_text(tree, "Ada Lovelace")
    n1["no_guest_pill"] = not app.has_text(tree, GUEST_NOTE)
    place = group(tree, "Your place")
    n1["signed_in_note"] = any("Signed in as Ada Lovelace" in (x.get("label") or "") for x in within(tree, place))
    sections = group(tree, "Academy sections")
    n1["account_sections"] = links(within(tree, sections)) if sections else []
    account_calls = [r for r in academy.requests()[mark:] if r.get("procedure")]
    n1["account_calls"] = sorted({c["procedure"] for c in account_calls})
    n1["account_credential"] = any(c.get("authorization") for c in account_calls)
    obs["tree_account"] = c29.save_tree("academy-account", tree)
    obs["captures"]["academy-account"] = c29.matrix(app, "academy-account")
    # The parity composites, from this state, in both themes (the theme follows the appearance).
    app.ax("focus", "AXHeading", "AI Security Academy")
    time.sleep(0.3)
    n1["parity"] = {}
    for theme in ("Light", "Dark"):
        c29.appearance(app, theme)
        time.sleep(0.9)
        n1["parity"][theme.lower()] = composite(app, theme, geometry(app.tree())["web"], prepared)
    c29.appearance(app, "Light")
    time.sleep(0.5)
    obs["n1"] = n1

    # ---- N02: keyboard, ring, layouts ----
    n2 = {}
    tree = app.tree()
    sections = group(tree, "Academy sections")
    first = links(within(tree, sections))[0] if sections and links(within(tree, sections)) else None
    walk = []
    if first:
        app.ax("focus", "AXLink", first)
        time.sleep(0.3)
        for _ in range(8):
            app.keycode(48)
            time.sleep(0.15)
            f = fx.App.focused(app.tree()) or {}
            walk.append(f.get("label") or "")
    n2["walk"] = walk
    n2["reach"] = {"section": any(w in n1["account_sections"][1:] for w in walk), "card": any(w.startswith("Open ") for w in walk)}
    if first:
        app.ax("focus", "AXLink", first)
        time.sleep(0.4)
    plain = c29.capture(app, "academy-ring-plain")
    n2["ring"] = ring_on_link(app, n1["account_sections"][1] if len(n1["account_sections"]) > 1 else first, plain, "academy-ring") if first else {}
    layouts = {}
    size_window(app, pid, 1440, 900)
    for theme in ("Light", "Dark"):
        c29.appearance(app, theme)
        time.sleep(0.5)
        o, tree = overflow(app)
        g = geometry(tree)
        o["geometry"] = g
        o["width_1440"] = bool(g["web"] and g["web"][0] == 1440)
        o["board"] = board_usable(tree, 1)
        name = f"academy-{g['web'][0]}x{g['web'][1]}-{theme.lower()}" if g["web"] else f"academy-unknown-{theme.lower()}"
        o["capture"] = c29.capture(app, name)
        o["tree"] = c29.save_tree(name, tree)
        layouts[name] = o
    size_window(app, pid, 900, 600)
    c29.zoom(app, 10)
    for theme in ("Light", "Dark"):
        c29.appearance(app, theme)
        time.sleep(0.5)
        o, tree = overflow(app)
        g = geometry(tree)
        vp = g["viewport"] or g["web"]
        o["geometry"] = g
        o["window_900x600"] = bool(vp and vp[0] == 900 and vp[1] == 600 and g["web"] and g["web"][0] == 900)
        o["board"] = board_usable(tree, 2)
        name = f"academy-{vp[0]}x{vp[1]}-200-{theme.lower()}" if vp else f"academy-unknown-200-{theme.lower()}"
        o["capture"] = c29.capture(app, name)
        o["tree"] = c29.save_tree(name, tree)
        layouts[name] = o
    c29.zoom(app, -10)
    c29.appearance(app, "Match System")
    size_window(app, pid, 1440, 900)
    n2["layouts"] = layouts
    obs["captures"]["layouts"] = {k: v["capture"] for k, v in layouts.items()}
    obs["captures"]["academy-ring"] = {"plain": n2["ring"].get("plain"), "focused": n2["ring"].get("capture")}
    obs["n2"] = n2
    app.quit()

    run_path = os.path.join(NATIVE, "observations.json")
    with open(run_path, "w") as f:
        json.dump(json.loads(json.dumps({"observations": obs, "log": LOG}).replace(rel(NATIVE), rel(FINAL))), f, indent=1)
    run_rel = rel(run_path)
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C42"]}
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail", "observation": observation, "evidence": [e for e in evidence if e]})

    caps = lambda name: [v for v in obs["captures"].get(name, {}).values() if v]  # noqa: E731
    n1_ok = (all(n1.get(k) for k in ("guest_screen", "lead", "guest_pill", "frame", "catalog_in_pages", "guest_note", "guest_open_settings", "paths_heading", "path_units", "guest_no_credential", "paths_list_read", "chrome_inside",
                                     "path_opened", "signed_in", "account_screen", "account_pill", "no_guest_pill", "signed_in_note", "account_credential"))
             and n1["guest_sections"] == ["Learn"] and len(n1["path_cards"]) >= 1 and n1["account_sections"] == ["Home", "Learn", "Program", "Downloads", "Notifications", "Settings"] and len(n1["parity"]) == 2)
    add("C42-N01", n1_ok,
        f"guest: screen {n1['guest_screen']} lead {n1['lead']} pill {n1['guest_pill']}; frame {n1['frame']} with the catalog in the pages card {n1['catalog_in_pages']}; Your place: note {n1['guest_note']}, Open Settings {n1['guest_open_settings']}, sections {n1['guest_sections']}; Learning paths {n1['paths_heading']} cards {n1['path_cards']} with units {n1['path_units']}; "
        f"guest calls {n1['guest_calls']} without a credential {n1['guest_no_credential']} (paths.list {n1['paths_list_read']}); learner chrome inside the region {n1['chrome_inside']}; a card opened its page {n1['path_opened']} with the trail {n1['trail']}; "
        f"signed in {n1['signed_in']}: pill {n1['account_pill']} (guest pill gone {n1['no_guest_pill']}), note {n1['signed_in_note']}, sections {n1['account_sections']}, calls {n1['account_calls']} with the credential {n1['account_credential']}; composites {sorted(n1['parity'].values())}",
        [run_rel, obs["tree_guest"], obs.get("tree_path"), obs["tree_account"]] + caps("academy-guest") + caps("academy-account") + sorted(n1["parity"].values()))
    lay = n2["layouts"]
    ring_ok = (n2["ring"].get("diff") or 0) > 40
    layouts_ok = len(lay) == 4 and all(v["overflow_px"] == 0 and v["horizontal_scrollbars"] == 0 and v["board"]["ok"] for v in lay.values()) \
        and sum(1 for v in lay.values() if v.get("width_1440")) == 2 and sum(1 for v in lay.values() if v.get("window_900x600")) == 2
    add("C42-N02", n2["reach"]["section"] and n2["reach"]["card"] and ring_ok and layouts_ok,
        f"Tab from the first section reached {n2['walk']} (a section {n2['reach']['section']}, a path card {n2['reach']['card']}); ring on a section link {n2['ring'].get('diff')} px; "
        + "; ".join(f"{k}: web {v['geometry']['web']} viewport {v['geometry']['viewport']} overflow {v['overflow_px']} px, scroll bars {v['horizontal_scrollbars']}, board {v['board']}" for k, v in lay.items()),
        [run_rel] + [v["tree"] for v in lay.values()] + caps("layouts") + caps("academy-ring"))

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1", "subject": "C42", "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (System Events and the ax helper) with fixture WebView self-capture; tools/native-acceptance/acceptance_c42.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "Pixels come from the fixture build's own WKWebView snapshot; the parity composites put the prototype board (rendered at the WebView's measured size) beside it; the embed's content is the synthetic Academy catalog's, judged as Academy's own (P19).",
            "Synthetic Academy, scan and management endpoints only; the device sign-in is the synthetic endpoint's granted approval.",
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
