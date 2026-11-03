#!/usr/bin/env python3
"""C31 native acceptance: Administration, reviews and Runtime key rotation in
the v2 shell, driven through the real fixture window against the synthetic
management service, with the native AppKit confirmation and secret sheets.

Run from the repository root after `xtask native-prepare --chunk C31`.
Requires the macOS Accessibility grant. Synthetic services only; the fixture
data root is removed at the end.

Checks (docs/architecture/native-checks.json, C31): the Administration
inventory (N01), a topic review applied only with the typed name through the
native confirmation and read back (N02), the staged key rotation with the
host-only reveal, the Inbox reminder and the two labelled timestamps (N03),
and the accessibility additions on the changed UI (N04).
"""

import json
import os
import shutil
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fixture_app as fx  # noqa: E402
import acceptance_c01 as c1  # noqa: E402
import acceptance_c08 as c8  # noqa: E402
import acceptance_c09 as c9  # noqa: E402
import acceptance_c28 as c28  # noqa: E402
import acceptance_c29 as c29  # noqa: E402
import administration_coverage as coverage  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C31")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C31" else "native-C31")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C31" else "native-acceptance-C31.json")
LOG = []
ROTATE = "Rotate the key of Support bot"
VERSION_BODY = ('{"api_key_name":"Runtime 2","cust_app":"Support bot","cust_env":"production","cust_cloud_provider":"aws",'
                '"dp_name":"dp-0","revoked":false,"rotation_time_interval":30,"rotation_time_unit":"days"}')


def rel(path):
    return os.path.relpath(path, ROOT)


def dialog(tree, label):
    return next((n for n in tree if (n.get("label") or "") == label and n.get("subrole") in ("AXDialog", "AXApplicationDialog")), None)


def button(tree, label):
    return next((n for n in tree if n.get("role") == "AXButton" and (n.get("label") or "") == label), None)


def tab(tree, label):
    return next((n for n in tree if n.get("role") in ("AXRadioButton", "AXTab") and (n.get("label") or "") == label), None)


def status_texts(tree):
    """The text inside each polite status region (role=status, AXApplicationStatus), by tree depth."""
    regions = []
    for i, n in enumerate(tree):
        if n.get("subrole") != "AXApplicationStatus":
            continue
        texts = []
        for m in tree[i + 1:]:
            if m.get("depth", 0) <= n.get("depth", 0):
                break
            texts.append(str(m.get("label") or m.get("value") or ""))
        regions.append(" ".join(t for t in texts if t))
    return regions


def ring_on(app, label, plain_cap, name):
    """Ring pixels around button `label` focused, against a capture with it unfocused."""
    app.ax("focus", "AXButton", label)
    time.sleep(0.5)
    focused_cap = c29.capture(app, name)
    snap = app.tree()
    web = next((n for n in snap if n.get("role") == "AXWebArea"), None)
    btn = button(snap, label)
    if not (web and btn and plain_cap and focused_cap):
        return {"diff": 0, "focused": (fx.App.focused(snap) or {}).get("label")}
    img_f = c1.png_pixels(os.path.join(ROOT, focused_cap))
    img_p = c1.png_pixels(os.path.join(ROOT, plain_cap))
    scale = img_f[0] / web["size"][0]
    box = [btn["pos"][0] - web["pos"][0], btn["pos"][1] - web["pos"][1], btn["size"][0], btn["size"][1]]
    return {"diff": c28.ring_diff(img_f, img_p, box, scale), "focused": (fx.App.focused(snap) or {}).get("label"), "capture": focused_cap, "plain": plain_cap}


def press_tab(app, label):
    app.wait_for(lambda t: tab(t, label) is not None, 8)
    node = tab(app.tree(), label)
    app.ax("press", node["role"], label)
    time.sleep(0.6)



def gateway_key_checks(app):
    """Inspect both Gateway key kinds and every returned detail, on this app.

    Card-scoped selectors prevent an open Runtime detail or the navigation
    rail from standing in for Gateway facts. Keep a tree and capture for each
    list/detail, including failures; the caller enforces gateway_keys_complete.
    """
    # A fresh tenant has no Gateway workspace until the operator chooses one.
    # Keys shows that chooser, not key-kind controls, until its binding settles.
    def first_kind(tree):
        return next((n for n in coverage.product_nodes(tree, "AI Gateway")
                     if n.get("role") in ("AXRadioButton", "AXTab")
                     and n.get("label") == "Service keys"), None)
    def workspace(tree):
        return next((n for n in coverage.product_nodes(tree, "AI Gateway")
                     if n.get("role") == "AXRadioButton"
                     and (n.get("label") or "").startswith("Team A")), None)
    tree, ready = app.wait_for(lambda t: first_kind(t) is not None or workspace(t) is not None, 20)
    if not ready:
        raise ValueError("Gateway Keys has neither its workspace chooser nor key-kind controls")
    if first_kind(tree) is None:
        coverage.act_on_node(app, tree, workspace(tree))
        tree, ready = app.wait_for(lambda t: first_kind(t) is not None, 20)
        if not ready:
            raise ValueError("Gateway Keys workspace did not settle")
    result = {"kinds": {}}
    for kind in ("Service keys", "User keys"):
        tree, ready = app.wait_for(lambda t: any(n.get("role") in ("AXRadioButton", "AXTab")
                                   and n.get("label") == kind
                                   for n in coverage.product_nodes(t, "AI Gateway")), 20)
        if not ready:
            raise ValueError("Gateway key-kind control did not settle: " + kind)
        nodes = coverage.product_nodes(tree, "AI Gateway")
        choice = next((n for n in nodes if n.get("role") in ("AXRadioButton", "AXTab")
                       and n.get("label") == kind), None)
        coverage.act_on_node(app, tree, choice)
        tree, _ = app.wait_for(lambda t: coverage.gateway_key_state(t, kind)["table"], 20)
        state = coverage.gateway_key_state(tree, kind)
        slug = kind.lower().replace(" ", "-")
        state.update(tree=c29.save_tree("gateway-" + slug, tree),
                     capture=c29.capture(app, "gateway-" + slug))
        rows = sorted({n["label"] for n in coverage.product_nodes(tree, "AI Gateway")
                       if n.get("role") == "AXButton" and (n.get("label") or "").startswith("Details for ")})
        observed = {"list": state, "row_labels": rows, "details": []}
        for index, label in enumerate(rows):
            tree = app.tree()
            node = next((n for n in coverage.product_nodes(tree, "AI Gateway")
                         if n.get("role") == "AXButton" and n.get("label") == label), None)
            coverage.act_on_node(app, tree, node)
            tree, _ = app.wait_for(lambda t: coverage.gateway_key_state(t, kind, require_detail=True)["detail_success"], 20)
            detail = coverage.gateway_key_state(tree, kind, require_detail=True)
            detail.update(row=label, tree=c29.save_tree(f"gateway-{slug}-detail-{index}", tree),
                          capture=c29.capture(app, f"gateway-{slug}-detail-{index}"))
            observed["details"].append(detail)
            close = coverage.button(coverage.detail_nodes(tree, "AI Gateway"), "Close details")
            coverage.act_on_node(app, tree, close)
            app.wait_for(lambda t: not coverage.detail_nodes(t, "AI Gateway"), 8)
        result["kinds"][kind] = observed
    return result


def gateway_key_evidence(value):
    return [row[key] for kind in value.get("kinds", {}).values()
            for row in [kind["list"], *kind["details"]]
            for key in ("tree", "capture") if row.get(key)]


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
    work = tempfile.mkdtemp(prefix="airs-v2-c31-acceptance-")
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
    try:
        return exercise(app, mgmt, endpoint, prepared, FINAL)
    finally:
        endpoint.stop()
        mgmt.stop()
        shutil.rmtree(work, ignore_errors=True)


def exercise(app, mgmt, endpoint, prepared, FINAL):
    global NATIVE
    obs = {"captures": {}}
    app.launch()
    pid = app.pid
    app.open_settings()
    app.type_into("AXTextField", "TSG ID (optional)", c8.TSG)
    app.create_custom_tenant("Tenant A", endpoint.origin)
    app.save_key(c8.SCAN_KEY)
    c8.save_client(app)
    c29.press(app, "AXCheckBox", "Use the new interface")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Home") is not None and c29.nav(t, "Destinations") is not None, 15)
    app.menu("Go", "Administration")
    tree, _ = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Administration") is not None and tab(t, "Custom topics") is not None, 15)

    # ---- N01: the inventory ----
    n1 = {}
    n1["gates"] = all(coverage.gated_product(tree, name, "needs an admitted " + name + " service")
                       for name in ("Agentic Identity Security", "Agentic Endpoint Security"))
    n1["runtime_tabs"] = [n.get("label") for n in tree if n.get("role") in ("AXRadioButton", "AXTab")]
    c8.text(app, "Details for Finance", 12)
    n1["new_topic"] = button(app.tree(), "New topic…") is not None
    press_tab(app, "Applications")
    tree, _ = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Details for Support bot") is not None, 12)
    n1["no_new_application"] = not any(n.get("role") == "AXButton" and (n.get("label") or "").startswith("New ")
                                           for n in coverage.product_nodes(tree, "AI Runtime Security"))
    press_tab(app, "API keys")
    tree, _ = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Details for Runtime") is not None, 12)
    n1["new_api_key"] = button(tree, "New API key…") is not None
    app.ax("press", "AXButton", "Details for Runtime")
    tree, _ = app.wait_for(lambda t: button(t, "Issue additional key…") is not None, 10)
    n1["key_actions"] = [n.get("label") for n in coverage.detail_nodes(tree, "AI Runtime Security")
                         if n.get("role") == "AXButton" and n.get("label") != "Close details"]
    obs["tree_admin"] = c29.save_tree("admin-keys", tree)
    obs["captures"]["admin"] = c29.matrix(app, "admin")
    # Open the actual Gateway card, not its navigation-rail namesake. Runtime
    # may remain open: its legitimate rotation actions must not taint Gateway.
    tree = app.tree()
    coverage.act_on_node(app, tree, coverage.product_header(tree, "AI Gateway"))
    tree, _ = app.wait_for(lambda t: tab(coverage.product_nodes(t, "AI Gateway"), "Keys") is not None, 10)
    n1["gateway_tabs"] = [n.get("label") for n in coverage.product_nodes(tree, "AI Gateway")
                          if n.get("role") in ("AXRadioButton", "AXTab")]
    coverage.act_on_node(app, tree, tab(coverage.product_nodes(tree, "AI Gateway"), "Keys"))
    n1["gateway_keys"] = gateway_key_checks(app)
    n1["gateway_no_version"] = coverage.gateway_keys_complete(n1["gateway_keys"])
    tree = app.tree()
    coverage.act_on_node(app, tree, coverage.product_header(tree, "AI Gateway"))
    app.wait_for(lambda t: tab(coverage.product_nodes(t, "AI Gateway"), "Keys") is None, 8)
    c29.press(app, "AXButton", "Model and skill scans")
    press_tab(app, "Codebase scans")
    tree = app.tree()
    n1["codebase_unavailable"] = app.has_text(tree, "needs an admitted codebase-scan read")
    obs["n1"] = n1

    # ---- N02: a topic review applied only with the typed name ----
    n2 = {}
    c29.press(app, "AXButton", "Model and skill scans")
    c29.press(app, "AXButton", "AI Runtime Security")
    press_tab(app, "Custom topics")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Details for Finance") is not None, 12)
    app.ax("press", "AXButton", "Details for Finance")
    app.wait_for(lambda t: button(t, "Edit…") is not None, 10)
    app.ax("press", "AXButton", "Edit…")
    tree, n2["sheet"] = app.wait_for(lambda t: dialog(t, "Review a change") is not None, 10)
    app.type_into("AXTextArea", "custom topic draft (JSON)", '{"topic_name":"Finance","description":"Money and payments","examples":[],"revision":1}')
    c9.review(app, "Type Finance to confirm")
    # Focus ring on the review sheet's controls (E3): Close focused vs the confirmation field focused.
    app.ax("focus", "AXTextField", "Type Finance to confirm")
    time.sleep(0.5)
    plain = c29.capture(app, "review-ring-plain")
    n2["ring"] = ring_on(app, "Close", plain, "review-ring-close")
    tree = app.tree()
    apply = button(tree, "Apply…") or {}
    n2["disabled_empty"] = apply.get("enabled") is False
    app.type_into("AXTextField", "Type Finance to confirm", "finance")
    time.sleep(0.4)
    n2["disabled_wrong"] = (button(app.tree(), "Apply…") or {}).get("enabled") is False
    app.type_into("AXTextField", "Type Finance to confirm", "Finance")
    time.sleep(0.4)
    n2["enabled_match"] = (button(app.tree(), "Apply…") or {}).get("enabled") is not False
    writes_before = len(c9.writes(mgmt, "PUT"))
    app.ax("press", "AXButton", "Apply…")
    tree, n2["native_confirmation"] = c9.wait_sheet(app)
    nodes = c9.sheet(app, tree) or []
    n2["confirmation_title"] = any("Update custom topic “Finance”?" in ((n.get("label") or "") + str(n.get("value") or "")) for n in nodes)
    app.keycode(36)
    tree, n2["live"] = c8.text(app, "Live. The item was read back.", 15)
    # The applied result is announced: it sits in a polite status region (DESIGN-V2 §8).
    n2["status_regions"] = status_texts(tree)
    n2["announced"] = any("Live. The item was read back." in r for r in n2["status_regions"])
    n2["writes"] = len(c9.writes(mgmt, "PUT")) - writes_before
    obs["tree_applied"] = c29.save_tree("review-applied", tree)
    obs["captures"]["review-applied"] = c29.matrix(app, "review-applied")
    c29.press(app, "AXButton", "Done")
    tree, n2["closed"] = app.wait_for(lambda t: dialog(t, "Review a change") is None, 8)
    obs["n2"] = n2

    # ---- N03: the staged rotation ----
    n3 = {}
    press_tab(app, "API keys")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Details for Runtime") is not None, 12)
    app.ax("press", "AXButton", "Details for Runtime")
    app.wait_for(lambda t: button(t, "Issue additional key…") is not None, 10)
    app.ax("press", "AXButton", "Issue additional key…")
    tree, n3["sheet"] = app.wait_for(lambda t: dialog(t, ROTATE) is not None, 10)
    app.type_into("AXTextArea", "API key draft (JSON)", VERSION_BODY)
    c9.review(app, "Type CREATE to confirm")
    app.type_into("AXTextField", "Type CREATE to confirm", "CREATE")
    time.sleep(0.4)
    posts_before = len(c9.writes(mgmt, "POST", "/aisec/v1/mgmt/apikey"))
    app.ax("press", "AXButton", "Apply…")
    c9.wait_sheet(app)
    app.keycode(36)
    tree, n3["issued"] = c8.text(app, "Additional key issued · old key still active", 15)
    posts = c9.writes(mgmt, "POST", "/aisec/v1/mgmt/apikey")
    n3["one_post"] = len(posts) - posts_before == 1
    issued = [p.get("issuedSha256") for p in posts]
    obs["tree_issued"] = c29.save_tree("rotation-issued", tree)
    obs["captures"]["rotation-issued"] = c29.matrix(app, "rotation-issued")
    c29.press(app, "AXButton", "Reveal new key…")
    tree, n3["host_sheet"] = c9.wait_sheet(app)
    secret = c9.secret_in(c9.sheet(app, tree))
    n3["revealed_matches_issued"] = bool(secret) and c9.sha(secret) in issued
    c9.press_exact(app, "Close")
    c9.wait_no_sheet(app)
    # Escape is a review exit: the held key is forgotten (I6).
    app.keycode(53)
    tree, n3["escaped"] = app.wait_for(lambda t: dialog(t, ROTATE) is None, 8)
    # The reminder is a distinct Inbox item.
    app.menu("Go", "Inbox")
    tree, n3["reminder"] = app.wait_for(lambda t: app.has_text(t, "Update Support bot with the new key, then remove the old key"), 12)
    obs["tree_reminder"] = c29.save_tree("rotation-reminder", tree)
    app.menu("Go", "Administration")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Administration") is not None, 10)
    press_tab(app, "API keys")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Details for Runtime") is not None, 12)
    app.ax("press", "AXButton", "Details for Runtime")
    tree, _ = app.wait_for(lambda t: button(t, "Finish rotation…") is not None, 10)
    app.ax("press", "AXButton", "Finish rotation…")
    tree, n3["resumed"] = app.wait_for(lambda t: app.has_text(t, "Rotation in progress · secret no longer available"), 10)
    n3["resumed_no_reveal"] = button(tree, "Reveal new key…") is None
    c29.press(app, "AXButton", "Application updated")
    tree, n3["acknowledged"] = app.wait_for(lambda t: app.has_text(t, "Deployment acknowledged") and fx.App.node(t, "AXButton", "Review change") is not None, 10)
    c9.review(app, "Type Runtime to confirm")
    app.type_into("AXTextField", "Type Runtime to confirm", "Runtime")
    time.sleep(0.4)
    deletes_before = len(c9.writes(mgmt, "DELETE", "/aisec/v1/mgmt/apikey/delete/"))
    app.ax("press", "AXButton", "Apply…")
    c9.wait_sheet(app)
    c9.press_exact(app, "Delete")
    tree, n3["complete"] = c8.text(app, "Rotation complete · old key removed", 15)
    n3["one_delete"] = len(c9.writes(mgmt, "DELETE", "/aisec/v1/mgmt/apikey/delete/")) - deletes_before == 1
    n3["timestamps"] = app.has_text(tree, "Deployment acknowledged (your statement)") and app.has_text(tree, "Old key removed (read back)")
    tree, _ = app.wait_for(lambda t: any("Rotation of Support bot complete" in r for r in status_texts(t)), 8)
    n3["status_regions"] = status_texts(tree)
    n3["announced"] = any("Rotation of Support bot complete. The old key was read back gone." in r for r in n3["status_regions"])
    obs["tree_complete"] = c29.save_tree("rotation-complete", tree)
    obs["captures"]["rotation-complete"] = c29.matrix(app, "rotation-complete")
    obs["n3"] = n3

    # ---- N04: accessibility additions ----
    n4 = {}
    tree = app.tree()
    n4["modal_count"] = sum(n.get("subrole") in ("AXDialog", "AXApplicationDialog") for n in tree)
    n4["sheet_topmost"] = c29.switcher(tree) == {} and dialog(tree, ROTATE) is not None and n4["modal_count"] == 1
    obs["tree_topmost"] = c29.save_tree("rotation-modal-topmost", tree)
    app.keycode(53)
    tree, n4["sheet_closed"] = app.wait_for(lambda t: dialog(t, ROTATE) is None, 8)
    time.sleep(0.5)
    focused = fx.App.focused(app.tree()) or {}
    n4["focus_after"] = focused.get("label")
    obs["tree_focus_returned"] = c29.save_tree("rotation-focus-returned", app.tree())
    # The opener ("Finish rotation…") is gone once the rotation completed:
    # focus goes to the page heading, never nowhere.
    n4["focus_returned"] = focused.get("role") == "AXHeading" and (focused.get("label") or "") == "Administration"
    n4["reminder_cleared"] = True
    app.menu("Go", "Inbox")
    tree, _ = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Inbox") is not None, 10)
    n4["reminder_cleared"] = not app.has_text(tree, "Update Support bot with the new key")
    app.menu("Go", "Administration")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Administration") is not None, 10)
    # Keyboard: from the Runtime header, Tab reaches its tabs.
    tree = app.tree()
    if tab(coverage.product_nodes(tree, "AI Runtime Security"), "Custom topics") is None:
        coverage.act_on_node(app, tree, coverage.product_header(tree, "AI Runtime Security"))
        tree, _ = app.wait_for(lambda t: tab(coverage.product_nodes(t, "AI Runtime Security"), "Custom topics") is not None, 10)
    coverage.act_on_node(app, tree, coverage.product_header(tree, "AI Runtime Security"), "focus")
    walk = []
    n4["keyboard_trees"] = []
    for index in range(10):
        app.keycode(48)
        tree = app.tree()
        f = fx.App.focused(tree) or {}
        walk.append(f.get("label") or "")
        n4["keyboard_trees"].append(c29.save_tree(f"administration-keyboard-{index}", tree))
    n4["walk"] = walk
    n4["tabs_reached"] = any(l in ("Overview", "Playground", "Security profiles", "Custom topics") for l in walk)
    # 900×600 and 200% in both themes: nothing past the window's right edge.
    base = c28.window_size(pid)
    c28.set_window_size(pid, 900, 600)
    c29.zoom(app, 10)
    zoom = {}
    for theme in ("Light", "Dark"):
        c29.appearance(app, theme)
        tree = app.tree()
        win = next((n for n in tree if n.get("role") == "AXWindow"), None)
        # What a reader or a pointer reaches: controls, text, tabs and cells
        # (container groups can carry stale frames in WebKit's tree).
        nodes = [n for n in tree if n.get("pos") and n.get("size") and n["size"][0] > 0
                 and n.get("role") in ("AXButton", "AXStaticText", "AXRadioButton", "AXTab", "AXTextField", "AXTextArea", "AXCell", "AXCheckBox", "AXPopUpButton", "AXLink", "AXHeading")]
        right = max(n["pos"][0] + n["size"][0] for n in nodes) if nodes and win else None
        hbars = [n for n in tree if n.get("role") == "AXScrollBar" and (n.get("orientation") == "horizontal" or (n.get("size") and n["size"][0] > n["size"][1]))]
        zoom[theme.lower()] = {"overflow_px": max(0, right - (win["pos"][0] + win["size"][0])) if right is not None else None, "horizontal_scrollbars": len(hbars),
                               "capture": c29.capture(app, f"admin-min-200-{theme.lower()}"), "tree": c29.save_tree(f"admin-min-200-{theme.lower()}", tree)}
    c29.zoom(app, -10)
    c29.appearance(app, "Match System")
    if base:
        c28.set_window_size(pid, base[0], base[1])
    n4["zoom"] = zoom
    obs["captures"]["admin-min-200"] = {k: v["capture"] for k, v in zoom.items()}
    # Reduce Motion (E3): the applied change's toast, bound to this tenant,
    # stays until dismissed instead of expiring after 6 s.
    had_key = fx.sh(["defaults", "read", "com.apple.universalaccess", "reduceMotion"]).returncode == 0
    prior = fx.sh(["defaults", "read", "com.apple.universalaccess", "reduceMotion"]).stdout.strip()
    rm = {}
    try:
        fx.sh(["defaults", "write", "com.apple.universalaccess", "reduceMotion", "-bool", "true"])
        time.sleep(1)
        rm["active"] = c28.reduce_motion()
        app.quit()
        app.launch()
        c29._APP = app
        app.wait_for(lambda t: c29.nav(t, "Destinations") is not None, 15)
        app.menu("Go", "Administration")
        app.wait_for(lambda t: fx.App.node(t, "AXButton", "Details for Finance") is not None, 15)
        app.ax("press", "AXButton", "Details for Finance")
        app.wait_for(lambda t: button(t, "Edit…") is not None, 10)
        app.ax("press", "AXButton", "Edit…")
        app.wait_for(lambda t: dialog(t, "Review a change") is not None, 10)
        app.type_into("AXTextArea", "custom topic draft (JSON)", '{"topic_name":"Finance","description":"Money, payments and wires","examples":[],"revision":2}')
        c9.review(app, "Type Finance to confirm")
        app.type_into("AXTextField", "Type Finance to confirm", "Finance")
        time.sleep(0.4)
        app.ax("press", "AXButton", "Apply…")
        c9.wait_sheet(app)
        app.keycode(36)
        _, rm["applied"] = c8.text(app, "Live. The item was read back.", 15)
        c29.press(app, "AXButton", "Done")
        tree, rm["toast"] = app.wait_for(lambda t: app.has_text(t, "Applied and read back: Edit Finance"), 8)
        time.sleep(8)
        tree = app.tree()
        rm["persists"] = app.has_text(tree, "Applied and read back: Edit Finance")
        rm["tree"] = c29.save_tree("reduce-motion-toast", tree)
        rm["capture"] = c29.capture(app, "reduce-motion-toast")
        # Dismiss is pressed when the toast is still there; a toast that left on its own is recorded by `persists`
        # (false) rather than aborting the run (the C44 cumulative run met that once).
        if fx.App.node(app.tree(), "AXButton", "Dismiss") is not None:
            c29.press(app, "AXButton", "Dismiss")
        time.sleep(0.6)
        rm["dismissed"] = not app.has_text(app.tree(), "Applied and read back: Edit Finance")
    finally:
        if had_key:
            fx.sh(["defaults", "write", "com.apple.universalaccess", "reduceMotion", "-bool", "true" if prior == "1" else "false"])
        else:
            fx.sh(["defaults", "delete", "com.apple.universalaccess", "reduceMotion"])
        time.sleep(1)
    rm["restored"] = c28.reduce_motion() == (had_key and prior == "1")
    n4["reduce_motion"] = rm
    obs["captures"]["reduce-motion"] = {"toast": rm.get("capture")}
    obs["captures"]["review-ring"] = {"plain": n2["ring"].get("plain"), "close": n2["ring"].get("capture")}
    obs["n4"] = n4
    app.quit()

    run_path = os.path.join(NATIVE, "observations.json")
    with open(run_path, "w") as f:
        json.dump(json.loads(json.dumps({"observations": obs, "log": LOG}).replace(rel(NATIVE), rel(FINAL))), f, indent=1)
    run_rel = rel(run_path)
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C31"]}
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [e for e in evidence if e]})

    caps = lambda name: list(obs["captures"].get(name, {}).values())  # noqa: E731
    add("C31-N01", n1["gates"] and n1["new_topic"] and n1["no_new_application"] and n1["new_api_key"] and set(n1["key_actions"]) == {"Issue additional key…", "Regenerate…", "Remove…"} and "Limits" not in n1["gateway_tabs"] and n1["gateway_no_version"] and n1["codebase_unavailable"],
        f"AIS/AES gates {n1['gates']}; New topic… {n1['new_topic']}; no New application… {n1['no_new_application']}; New API key… {n1['new_api_key']}; key actions {n1['key_actions']}; Gateway tabs {n1['gateway_tabs']}; Gateway keys never offer a version {n1['gateway_no_version']}; Codebase scans unavailable {n1['codebase_unavailable']}",
        [run_rel, obs["tree_admin"]] + gateway_key_evidence(n1["gateway_keys"]) + caps("admin"))
    add("C31-N02", n2["sheet"] and n2["disabled_empty"] and n2["disabled_wrong"] and n2["enabled_match"] and n2["native_confirmation"] and n2["confirmation_title"] and n2["live"] and n2["writes"] == 1 and n2["closed"],
        f"review sheet {n2['sheet']}; Apply disabled empty {n2['disabled_empty']} and wrong-case {n2['disabled_wrong']}, enabled on the exact name {n2['enabled_match']}; native confirmation {n2['native_confirmation']} titled for Finance {n2['confirmation_title']}; applied panel 'Live. The item was read back.' {n2['live']}; PUT writes {n2['writes']}; Done closed {n2['closed']}",
        [run_rel, obs["tree_applied"]] + caps("review-applied"))
    add("C31-N03", n3["sheet"] and n3["issued"] and n3["one_post"] and n3["host_sheet"] and n3["revealed_matches_issued"] and n3["escaped"] and n3["reminder"] and n3["resumed"] and n3["resumed_no_reveal"] and n3["acknowledged"] and n3["complete"] and n3["one_delete"] and n3["timestamps"],
        f"additional key issued {n3['issued']} with one POST {n3['one_post']}; Reveal opened the host sheet {n3['host_sheet']} showing the issued key {n3['revealed_matches_issued']}; Escape closed {n3['escaped']}; Inbox reminder {n3['reminder']}; Finish rotation… resumed {n3['resumed']} with no Reveal {n3['resumed_no_reveal']}; Application updated acknowledged {n3['acknowledged']}; old key removed through its review with one DELETE {n3['one_delete']} → complete {n3['complete']}; both labelled timestamps {n3['timestamps']}",
        [run_rel, obs["tree_issued"], obs["tree_reminder"], obs["tree_complete"]] + caps("rotation-issued") + caps("rotation-complete"))
    zoom_ok = all(v["overflow_px"] == 0 and v["horizontal_scrollbars"] == 0 for v in n4["zoom"].values())
    rmo = n4["reduce_motion"]
    ring_ok = n2["ring"].get("diff", 0) > 150 and n2["ring"].get("focused") == "Close"
    rm_ok = rmo.get("active") and rmo.get("applied") and rmo.get("toast") and rmo.get("persists") and rmo.get("dismissed") and rmo.get("restored")
    add("C31-N04", n4["sheet_topmost"] and n4["sheet_closed"] and n4["focus_returned"] and n4["reminder_cleared"] and n4["tabs_reached"] and zoom_ok
        and ring_ok and n2["announced"] and n3["announced"] and rm_ok,
        f"rotation sheet topmost (rail hidden) {n4['sheet_topmost']}; Escape closed {n4['sheet_closed']} returning focus to '{n4['focus_after']}' ({n4['focus_returned']}); reminder cleared after completion {n4['reminder_cleared']}; Tab from the Runtime header reached its tabs {n4['tabs_reached']}; at 900×600 and 200% overflow light {n4['zoom']['light']['overflow_px']} px, dark {n4['zoom']['dark']['overflow_px']} px; "
        f"focus ring on the review sheet's Close {n2['ring'].get('diff')} px changed ({ring_ok}); applied result in a status region {n2['announced']}; rotation completion announced in the panel's status region {n3['announced']}; "
        f"Reduce Motion active {rmo.get('active')}: the applied toast shown {rmo.get('toast')}, still present after 8 s {rmo.get('persists')}, dismissed {rmo.get('dismissed')}, setting restored {rmo.get('restored')}",
        [run_rel, rmo.get("tree"), obs["tree_topmost"], obs["tree_focus_returned"]] + n4["keyboard_trees"] + [v["tree"] for v in n4["zoom"].values()] + caps("admin-min-200") + caps("review-ring") + caps("reduce-motion"))

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1", "subject": "C31", "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (System Events and the ax helper) with fixture WebView self-capture; tools/native-acceptance/acceptance_c31.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "Pixels come from the fixture build's own WKWebView snapshot; the native confirmation and secret sheets are verified through the accessibility tree.",
            "Changes run against the synthetic management endpoint (tools/native-acceptance/mgmt_server.py); the live topic review row is recorded separately under evidence/C31/live.",
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
