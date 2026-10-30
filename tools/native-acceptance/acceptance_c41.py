#!/usr/bin/env python3
"""C41 native acceptance (D059): the API Client as the prototype board draws
it (R2App start=api), driven through the real fixture window against the
synthetic scan and management endpoints and a plain-HTTP loopback service
the collection's requests go to.

Run from the repository root after `xtask native-prepare --chunk C41`.
Requires the macOS Accessibility grant. Synthetic services only; the fixture
data root is removed at the end.

Checks (docs/architecture/native-checks.json, C41): the board (title row
with the chips, the request tree, the open requests' tabs, the request
line, the sub-tabs, the response pane), Send's completion in the status
region with the body outside it, a later send reflecting the service's
change while History keeps the first response exactly, a new request from
+, a Params row written as a line and read by Preview, the collection
menu's docked sheet (N01); keyboard reach, the focus ring and the measured
layouts in both themes (N02). The parity composites are captured from the
sent state.
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
import acceptance_c15 as c15  # noqa: E402
import acceptance_c28 as c28  # noqa: E402
import acceptance_c29 as c29  # noqa: E402
import acceptance_c31 as c31  # noqa: E402
import native_sheets as sheets  # noqa: E402

ROOT = os.getcwd()
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence/C41")
NATIVE = os.path.join(EVIDENCE, "native")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json")
PARITY = os.path.join(EVIDENCE, "parity")
DC = os.path.join(ROOT, "tools/parity")
LOG = []
TITLE_BAR = 28
FIRST_BODY = '{"custom_topics":["Finance"]}'
TABS = ("AXRadioButton", "AXButton", "AXCheckBox")

ANY = ("AXButton", "AXPopUpButton", "AXCheckBox", "AXRadioButton")


def node_any(tree, label):
    return next((n for n in tree if (n.get("label") or "") == label and n.get("role") in ANY), None)


def press_any(app, label, timeout=8):
    """Presses the control with this exact label whatever role AX gives it: a button with aria-haspopup is an
    AXPopUpButton, one with aria-pressed an AXCheckBox."""
    tree, _ = app.wait_for(lambda t: node_any(t, label) is not None, timeout)
    node = node_any(tree, label)
    if node is None:
        raise RuntimeError(f"no control labelled {label}")
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
             and n.get("role") in ("AXButton", "AXStaticText", "AXTextField", "AXTextArea", "AXSearchField", "AXCheckBox", "AXPopUpButton", "AXComboBox", "AXLink", "AXHeading", "AXRadioButton")]
    right = max(n["pos"][0] + n["size"][0] for n in nodes) if nodes and win else None
    hbars = [n for n in tree if n.get("role") == "AXScrollBar" and (n.get("orientation") == "horizontal" or (n.get("size") and n["size"][0] > n["size"][1]))]
    return {"overflow_px": max(0, right - (win["pos"][0] + win["size"][0])) if right is not None else None, "horizontal_scrollbars": len(hbars)}, tree


def board_usable(tree, zoom):
    """The board at this zoom: the tree's search, the URL field, Send (at least 32 CSS px) and the response pane's tabs."""
    find = next((n for n in tree if n.get("role") in ("AXTextField", "AXSearchField") and (n.get("label") or "") == "Find request"), None)
    url = fx.App.node(tree, "AXTextField", "URL")
    send = fx.App.node(tree, "AXButton", "Send")
    body_tab = next((n for n in tree if n.get("role") in TABS and (n.get("label") or "") == "Body" and n.get("pos") and n.get("size")), None)
    out = {"find": find is not None, "url": url is not None, "send_size": send.get("size") if send else None,
           "send_ok": bool(send and send["size"][0] >= 32 * zoom - 1 and send["size"][1] >= 32 * zoom - 1), "response_tabs": body_tab is not None, "zoom": zoom}
    out["ok"] = out["find"] and out["url"] and out["send_ok"] and out["response_tabs"]
    return out


def composite(app, theme, web_size, prepared):
    name = f"api-{theme.lower()}"
    cap = c29.capture(app, f"parity-{name}")
    if not cap:
        raise SystemExit(f"no capture for {name}")
    os.makedirs(os.path.join(PARITY, "app"), exist_ok=True)
    os.makedirs(os.path.join(PARITY, "proto"), exist_ok=True)
    with open(os.path.join(PARITY, "bundle.json"), "w") as f:
        json.dump({"bundle": prepared["bundle"], "executable_sha256": prepared["executable_sha256"], "from": "native-prepare.json",
                   "captured_by": "tools/native-acceptance/acceptance_c41.py", "captured_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}, f, indent=2)
        f.write("\n")
    app_png = os.path.join(PARITY, "app", f"{name}.png")
    shutil.copy(os.path.join(ROOT, cap), app_png)
    web_w, web_h = web_size
    proto_png = os.path.join(PARITY, "proto", f"{name}.png")
    subprocess.run([sys.executable, "-I", os.path.join(DC, "dc_render.py"), "--board", "R2App", "--prop", "start=api",
                    "--prop", f"theme={theme.lower()}", "--width", str(web_w), "--height", str(web_h), "--out", proto_png], check=True, capture_output=True)
    out = os.path.join(PARITY, f"{name}.png")
    subprocess.run([sys.executable, "-I", os.path.join(DC, "compose.py"), "--left", proto_png, "--right", app_png,
                    "--out", out, "--title", f"API Client · {theme.lower()} · {web_w}×{web_h}"], check=True, capture_output=True)
    return rel(out)


def stage_collection(work, origin):
    """A collection in a Git repository: a folder with one request to the loopback topics route, and a root request."""
    repo = os.path.realpath(os.path.join(work, "repo"))
    root = os.path.join(repo, "api")
    os.makedirs(os.path.join(root, "topics"))
    with open(os.path.join(root, "collection.toml"), "w") as f:
        f.write(f'format_version = 1\nid = "c41"\nname = "Topics API"\n\n[variables]\nbase = "{origin}"\n')
    with open(os.path.join(root, "topics", "list.request.toml"), "w") as f:
        f.write(c15.request("list", "GET", "{{base}}/topics"))
    with open(os.path.join(root, "health.request.toml"), "w") as f:
        f.write(c15.request("health", "GET", "{{base}}/health"))
    env = {"GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": "/dev/null", "HOME": repo}
    for args in (["init", "-q", "-b", "main"], ["add", "-A"],
                 ["-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "commit", "-q", "-m", "fixture"]):
        subprocess.run(["/usr/bin/git", "-C", repo, *args], capture_output=True, text=True, env=env)
    os.makedirs(fx.DATA, exist_ok=True)
    with open(os.path.join(fx.DATA, "fixture-open-folder"), "w") as f:
        f.write(root + "\n")
    return root


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
    work = tempfile.mkdtemp(prefix="airs-v2-c41-acceptance-")
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
    api_dir = os.path.join(work, "api-endpoint")
    os.makedirs(api_dir)
    with open(os.path.join(api_dir, "topics.json"), "w") as f:
        json.dump(["Finance"], f)
    api = c15.Workbench(api_dir)
    try:
        return exercise(app, endpoint, mgmt, api, api_dir, prepared, work, FINAL)
    finally:
        api.stop()
        endpoint.stop()
        mgmt.stop()
        shutil.rmtree(work, ignore_errors=True)


def exercise(app, endpoint, mgmt, api, api_dir, prepared, work, FINAL):
    global NATIVE
    obs = {"captures": {}}
    collection_root = stage_collection(work, api.origin)
    sheets.enable()
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

    # ---- N01: the board ----
    n1 = {}
    app.menu("Go", "API Client")
    tree, n1["screen"] = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "API Client") is not None, 15)
    n1["start"] = app.has_text(tree, "No collection is open.")
    obs["tree_start"] = c29.save_tree("api-start", tree)
    c29.press(app, "AXButton", "Open collection…")
    if sheets.wait_sheet(app, timeout=4):
        sheets.open_file(app, collection_root)
    tree, n1["opened"] = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Open GET topics/list.request.toml") is not None, 15)
    n1["chip"] = node_any(tree, "Collection Topics API") is not None
    n1["environment_chip"] = any(n.get("role") in ("AXPopUpButton", "AXComboBox") and (n.get("label") or "") == "Environment" for n in tree)
    n1["lead"] = any("Collection Topics API · api · Northwind Health" in x for x in app.texts(tree))
    n1["folder"] = fx.App.node(tree, "AXButton", "Collapse folder topics") is not None
    n1["root_request"] = fx.App.node(tree, "AXButton", "Open GET health.request.toml") is not None
    n1["find"] = any(n.get("role") in ("AXTextField", "AXSearchField") and (n.get("label") or "") == "Find request" for n in tree)
    n1["plus"] = node_any(tree, "New request") is not None
    n1["select_prompt"] = app.has_text(tree, "Select a request, or create one with New request.")
    obs["tree_opened"] = c29.save_tree("api-opened", tree)
    # Open the folder's request: a tab, the request line, the sub-tabs, the response pane.
    app.ax("press", "AXButton", "Open GET topics/list.request.toml")
    tree, n1["request"] = app.wait_for(lambda t: fx.App.node(t, "AXTextField", "URL") is not None, 10)
    n1["tab"] = any(n.get("role") in TABS and (n.get("label") or "") == "GET list" for n in tree)
    n1["method"] = any(n.get("role") in ("AXPopUpButton", "AXComboBox") and (n.get("label") or "") == "Method" for n in tree)
    n1["url_value"] = (fx.App.node(tree, "AXTextField", "URL") or {}).get("value")
    n1["send"] = fx.App.node(tree, "AXButton", "Send") is not None
    n1["save"] = fx.App.node(tree, "AXButton", "Save") is not None
    n1["saved_state"] = any(r == "Saved" for r in c31.status_texts(tree))
    n1["sub_tabs"] = [l for l in ("Params", "Headers", "Body", "Auth", "Variables", "Preview") if any(n.get("role") in TABS and (n.get("label") or "").startswith(l) for n in tree)]
    n1["response_tabs"] = [l for l in ("Body", "Headers", "Details", "History") if any(n.get("role") in TABS and (n.get("label") or "").startswith(l) for n in tree)]
    n1["no_response"] = app.has_text(tree, "Send to see the response here.") and app.has_text(tree, "No response yet")
    obs["tree_request"] = c29.save_tree("api-request", tree)
    # Send: the completion announced in the status region, the body outside it, exact under Raw.
    c29.press(app, "AXButton", "Send")
    tree, n1["status"] = app.wait_for(lambda t: any("list · 200 · " in r for r in c31.status_texts(t)), 20)
    regions = c31.status_texts(tree)
    n1["status_text"] = next((r for r in regions if "list · 200" in r), None)
    n1["body_shown"] = app.has_text(tree, '"custom_topics"') and not any("custom_topics" in r for r in regions)
    n1["status_pill"] = app.has_text(tree, "200 OK")
    obs["tree_sent"] = c29.save_tree("api-sent", tree)
    obs["captures"]["api-sent"] = c29.matrix(app, "api-sent")
    # The parity composites, from this state, in both themes.
    app.ax("focus", "AXHeading", "API Client")
    time.sleep(0.3)
    n1["parity"] = {}
    for theme in ("Light", "Dark"):
        c29.appearance(app, theme)
        time.sleep(0.7)
        n1["parity"][theme.lower()] = composite(app, theme, geometry(app.tree())["web"], prepared)
    c29.appearance(app, "Light")
    time.sleep(0.5)
    press_any(app, "Raw")
    tree, n1["raw_exact"] = app.wait_for(lambda t: app.has_text(t, FIRST_BODY), 6)
    headers_tab = next((l for l in labels(tree, TABS, "Headers") if l != "Headers"), None) or "Headers"
    role = next((n["role"] for n in tree if (n.get("label") or "") == headers_tab and n.get("role") in TABS and n is not None and (n.get("pos") or [0])[0] > 600), None)
    # The response pane's Headers tab is the one right of the request's columns.
    cands = [n for n in tree if n.get("role") in TABS and (n.get("label") or "").startswith("Headers")]
    resp_headers = max(cands, key=lambda n: (n.get("pos") or [0])[0]) if cands else None
    if resp_headers:
        app.ax("press", resp_headers["role"], resp_headers["label"], sum(1 for n in cands[:cands.index(resp_headers)] if n["label"] == resp_headers["label"]))
        time.sleep(0.5)
    tree = app.tree()
    n1["response_headers"] = any(n.get("role") == "AXTable" and (n.get("label") or "") == "Response headers" for n in tree) and app.has_text(tree, "content-type")
    del role
    # The service changes; a new send reflects it; History lists both and the older one is exactly the first.
    with open(os.path.join(api_dir, "topics.json"), "w") as f:
        json.dump(["Finance", "Travel"], f)
    c29.press(app, "AXButton", "Send")
    tree, n1["second"] = app.wait_for(lambda t: app.has_text(t, "Travel"), 20)
    hist = next((n for n in tree if n.get("role") in TABS and (n.get("label") or "") == "History"), None)
    app.ax("press", hist["role"], "History")
    tree, n1["history"] = app.wait_for(lambda t: len([n for n in t if n.get("role") == "AXButton" and (n.get("label") or "").startswith("GET topics/list · HTTP 200")]) >= 2, 15)
    entries = [n for n in tree if n.get("role") == "AXButton" and (n.get("label") or "").startswith("GET topics/list · HTTP 200")]
    n1["entries"] = len(entries)
    if entries:
        oldest = entries[-1]
        app.ax("press", "AXButton", oldest["label"], sum(1 for n in entries[:-1] if n["label"] == oldest["label"]))
        time.sleep(1)
        press_any(app, "Raw")
        time.sleep(0.5)
    tree = app.tree()
    n1["old_unchanged"] = app.has_text(tree, FIRST_BODY) and not app.has_text(tree, "Travel")
    obs["tree_history"] = c29.save_tree("api-history", tree)
    obs["captures"]["api-history"] = {"light": c29.capture(app, "api-history-light")}
    n1["api_requests"] = len(api.requests("/topics"))
    # A Params row, added in the table and read by Preview as the query.
    params = next((n for n in tree if n.get("role") in TABS and (n.get("label") or "").startswith("Params")), None)
    app.ax("press", params["role"], params["label"])
    time.sleep(0.4)
    c29.press(app, "AXButton", "+ Add parameter")
    app.type_into("AXTextField", "Name 1", "page")
    app.type_into("AXTextField", "Value 1", "1")
    tree, n1["unsaved"] = app.wait_for(lambda t: any(r == "Unsaved changes" for r in c31.status_texts(t)), 6)
    n1["unsaved_seen"] = c31.status_texts(tree)[:4]
    c29.press(app, "AXButton", "Save")
    tree, n1["saved"] = app.wait_for(lambda t: any(r == "Saved" for r in c31.status_texts(t)), 10)
    preview = next((n for n in tree if n.get("role") in TABS and (n.get("label") or "") == "Preview"), None)
    app.ax("press", preview["role"], "Preview")
    tree, n1["preview_query"] = app.wait_for(lambda t: any("/topics?page=1" in x for x in app.texts(t)), 10)
    obs["tree_preview"] = c29.save_tree("api-preview", tree)
    with open(os.path.join(collection_root, "topics", "list.request.toml")) as f:
        text = f.read()
        n1["file_has_row"] = "[[query]]" in text and 'name = "page"' in text and 'value = "1"' in text
    # + creates a request draft: a new tab, "New · not saved yet".
    press_any(app, "New request")
    tree, n1["new_dialog"] = app.wait_for(lambda t: dialog(t, "New request") is not None, 8)
    app.type_into("AXTextField", "Path (folder/name)", "topics/create")
    c29.press(app, "AXButton", "Create")
    tree, n1["new_tab"] = app.wait_for(lambda t: any(n.get("role") in TABS and (n.get("label") or "").startswith("GET create") for n in t) and any(r == "New · not saved yet" for r in c31.status_texts(t)), 10)
    obs["tree_new"] = c29.save_tree("api-new", tree)
    # The Collection chip's menu opens Environments in a docked sheet.
    press_any(app, "Collection Topics API")
    tree, n1["menu"] = app.wait_for(lambda t: any(n.get("role") in ("AXMenuItem", "AXButton") and (n.get("label") or "") == "Environments…" for n in t), 6)
    item = next((n for n in tree if (n.get("label") or "") == "Environments…"), None)
    if item:
        app.ax("press", item["role"], "Environments…")
    tree, n1["sheet"] = app.wait_for(lambda t: dialog(t, "Environments") is not None, 8)
    sheet = dialog(tree, "Environments")
    web = next((n for n in tree if n.get("role") == "AXWebArea" and n.get("size")), None)
    n1["sheet_docked"] = bool(sheet and web and abs((sheet["pos"][0] + sheet["size"][0]) - (web["pos"][0] + web["size"][0])) <= 2 and abs(sheet["size"][0] - 620) <= 2)
    obs["tree_sheet"] = c29.save_tree("api-sheet", tree)
    obs["captures"]["api-sheet"] = {"light": c29.capture(app, "api-sheet-light")}
    c29.press(app, "AXButton", "Close Environments")
    app.wait_for(lambda t: dialog(t, "Environments") is None, 6)
    # Back on the sent request for the layouts: nothing was sent by the view itself.
    n1["sent_total"] = len(api.requests("/topics"))
    obs["n1"] = n1

    # ---- N02: keyboard, ring, layouts ----
    n2 = {}
    # Back to the list request's tab (the + step left its draft, URL https://example.com/, as the active tab; the
    # first run sent that draft from the keyboard and was refused as not loopback). Only the active tab's editor
    # is in the tree, so the URL field and Send below are the list request's once its tab is selected.
    tree = app.tree()
    tab = next((n for n in tree if n.get("role") in TABS and (n.get("label") or "") == "GET list"), None)
    if tab:
        app.ax("press", tab["role"], "GET list")
    tree, n2["list_tab"] = app.wait_for(lambda t: (fx.App.node(t, "AXTextField", "URL") or {}).get("value", "").endswith("/topics"), 10)
    if not n2["list_tab"]:
        raise RuntimeError(f"the list request's tab did not come back: URL {(fx.App.node(tree, 'AXTextField', 'URL') or {}).get('value')!r}")
    app.ax("focus", "AXTextField", "URL")
    walk = []
    sent_by_key = False
    before = len(api.requests("/topics"))
    for _ in range(4):
        app.keycode(48)
        f = fx.App.focused(app.tree()) or {}
        walk.append(f.get("label") or "")
        if (f.get("label") or "") == "Send":
            app.keycode(49)
            sent_by_key = True
            break
    n2["walk"] = walk
    tree, n2["sent_by_key"] = app.wait_for(lambda t: len(api.requests("/topics")) == before + 1 and any("list · 200 · " in r for r in c31.status_texts(t)), 15)
    n2["sent_by_key"] = n2["sent_by_key"] and sent_by_key
    n2["key_status"] = [r for r in c31.status_texts(tree) if "list ·" in r or "not send" in r or "not done" in r][:3]
    plain = c29.capture(app, "api-ring-plain")
    n2["ring"] = c31.ring_on(app, "Send", plain, "api-ring")
    layouts = {}
    size_window(app, pid, 1440, 900)
    for theme in ("Light", "Dark"):
        c29.appearance(app, theme)
        time.sleep(0.4)
        o, tree = overflow(app)
        g = geometry(tree)
        o["geometry"] = g
        o["width_1440"] = bool(g["web"] and g["web"][0] == 1440)
        o["board"] = board_usable(tree, 1)
        name = f"api-{g['web'][0]}x{g['web'][1]}-{theme.lower()}" if g["web"] else f"api-unknown-{theme.lower()}"
        o["capture"] = c29.capture(app, name)
        o["tree"] = c29.save_tree(name, tree)
        layouts[name] = o
    size_window(app, pid, 900, 600)
    c29.zoom(app, 10)
    for theme in ("Light", "Dark"):
        c29.appearance(app, theme)
        time.sleep(0.4)
        app.ax("focus", "AXButton", "Send")
        time.sleep(0.4)
        o, tree = overflow(app)
        g = geometry(tree)
        vp = g["viewport"] or g["web"]
        o["geometry"] = g
        o["window_900x600"] = bool(vp and vp[0] == 900 and vp[1] == 600 and g["web"] and g["web"][0] == 900)
        o["board"] = board_usable(tree, 2)
        name = f"api-{vp[0]}x{vp[1]}-200-{theme.lower()}" if vp else f"api-unknown-200-{theme.lower()}"
        o["capture"] = c29.capture(app, name)
        o["tree"] = c29.save_tree(name, tree)
        layouts[name] = o
    c29.zoom(app, -10)
    c29.appearance(app, "Match System")
    size_window(app, pid, 1440, 900)
    n2["layouts"] = layouts
    obs["captures"]["layouts"] = {k: v["capture"] for k, v in layouts.items()}
    obs["captures"]["api-ring"] = {"plain": n2["ring"].get("plain"), "focused": n2["ring"].get("capture")}
    obs["n2"] = n2
    app.quit()

    run_path = os.path.join(NATIVE, "observations.json")
    with open(run_path, "w") as f:
        json.dump(json.loads(json.dumps({"observations": obs, "log": LOG}).replace(rel(NATIVE), rel(FINAL))), f, indent=1)
    run_rel = rel(run_path)
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C41"]}
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail", "observation": observation, "evidence": [e for e in evidence if e]})

    caps = lambda name: [v for v in obs["captures"].get(name, {}).values() if v]  # noqa: E731
    n1_ok = (all(n1.get(k) for k in ("screen", "start", "opened", "chip", "environment_chip", "lead", "folder", "root_request", "find", "plus", "select_prompt", "request", "tab", "method", "send", "save", "saved_state",
                                     "no_response", "status", "body_shown", "status_pill", "raw_exact", "response_headers", "second", "history", "old_unchanged", "unsaved", "saved", "preview_query", "file_has_row",
                                     "new_dialog", "new_tab", "menu", "sheet", "sheet_docked"))
             and n1.get("url_value", "").endswith("/topics") and len(n1["sub_tabs"]) == 6 and len(n1["response_tabs"]) == 4 and n1["entries"] >= 2 and n1["api_requests"] == 2 and n1["sent_total"] == 2 and len(n1["parity"]) == 2)
    add("C41-N01", n1_ok,
        f"screen {n1['screen']} with the start panel {n1['start']}; opened through the sheet {n1['opened']}: chip {n1['chip']}, environment chip {n1['environment_chip']}, lead {n1['lead']}, folder {n1['folder']}, root request {n1['root_request']}, Find {n1['find']}, + {n1['plus']}, prompt {n1['select_prompt']}; "
        f"request opened {n1['request']}: tab {n1['tab']}, Method {n1['method']}, URL {n1.get('url_value')!r}, Send {n1['send']}, Save {n1['save']}, state Saved {n1['saved_state']}, sub-tabs {n1['sub_tabs']}, response tabs {n1['response_tabs']}, no response yet {n1['no_response']}; "
        f"Send announced {n1.get('status_text')!r} in a status region {n1['status']} with the body outside it {n1['body_shown']} (pill {n1['status_pill']}), exact under Raw {n1['raw_exact']}, Headers {n1['response_headers']}; after the service changed a new send reflected it {n1['second']}; History listed {n1['entries']} runs and the older one showed exactly the first response {n1['old_unchanged']}; "
        f"a Params row unsaved {n1['unsaved']} (status {n1.get('unsaved_seen')}) then saved {n1['saved']}, Preview read ?page=1 {n1['preview_query']}, the file holds the line {n1['file_has_row']}; + opened the dialog {n1['new_dialog']} and a new tab {n1['new_tab']}; the chip menu {n1['menu']} opened Environments {n1['sheet']} docked at 620px {n1['sheet_docked']}; {n1['sent_total']} requests reached the loopback service; composites {sorted(n1['parity'].values())}",
        [run_rel, obs["tree_start"], obs["tree_opened"], obs["tree_request"], obs["tree_sent"], obs["tree_history"], obs["tree_preview"], obs["tree_new"], obs["tree_sheet"]] + caps("api-sent") + caps("api-history") + caps("api-sheet") + sorted(n1["parity"].values()))
    lay = n2["layouts"]
    ring_ok = (n2["ring"].get("diff") or 0) > 40 and n2["ring"].get("focused") == "Send"
    layouts_ok = len(lay) == 4 and all(v["overflow_px"] == 0 and v["horizontal_scrollbars"] == 0 and v["board"]["ok"] for v in lay.values()) \
        and sum(1 for v in lay.values() if v.get("width_1440")) == 2 and sum(1 for v in lay.values() if v.get("window_900x600")) == 2
    add("C41-N02", n2["sent_by_key"] and ring_ok and layouts_ok,
        f"on the list request's tab {n2['list_tab']}, Tab from URL reached {n2['walk']} and Space on Send sent and announced it {n2['sent_by_key']} ({n2['key_status']}); ring on Send {n2['ring'].get('diff')} px; "
        + "; ".join(f"{k}: web {v['geometry']['web']} viewport {v['geometry']['viewport']} overflow {v['overflow_px']} px, scroll bars {v['horizontal_scrollbars']}, board {v['board']}" for k, v in lay.items()),
        [run_rel] + [v["tree"] for v in lay.values()] + caps("layouts") + caps("api-ring"))

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1", "subject": "C41", "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (System Events and the ax helper) with fixture WebView self-capture; tools/native-acceptance/acceptance_c41.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "Pixels come from the fixture build's own WKWebView snapshot; the parity composites put the prototype board (rendered at the WebView's measured size) beside it; the native file sheet is driven through the fixture sheet adapter.",
            "Synthetic scan and management endpoints and a plain-HTTP loopback service for the collection's requests only; no tenant-credential request is sent.",
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
