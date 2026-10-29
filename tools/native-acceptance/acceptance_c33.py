#!/usr/bin/env python3
"""C33 native acceptance: Test Cases, the API Client and "Show the calls" in
the v2 shell, driven through the real fixture window against the synthetic
scan, management and loopback API endpoints.

Run from the repository root after `xtask native-prepare --chunk C33`.
Requires the macOS Accessibility grant. Synthetic services only; the fixture
data root and the staged collection are removed at the end.

Checks (docs/architecture/native-checks.json, C33): Test Cases with starter
packs, filter, search and Inspect dispatching no scan, and a Runtime case
opened in the Playground, run there by the operator, recording its result
(N01); the API Client's completion status region after Send with the body
outside it, and a history entry unchanged after a topic is applied while a
new send reflects it (N02); the calls panel shown only while chosen, naming
each call's request line and credential kind and never a secret (N03);
keyboard reach, the focus ring, and the 900x600 / 200% layout in both themes
(N04).
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
import acceptance_c15 as c15  # noqa: E402  (the loopback API endpoint)
import acceptance_c28 as c28  # noqa: E402
import acceptance_c29 as c29  # noqa: E402
import acceptance_c31 as c31  # noqa: E402  (focus ring and status-region helpers)
import native_sheets as sheets  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C33")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C33" else "native-C33")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C33" else "native-acceptance-C33.json")
LOG = []
RED_TEAM_RUNS = "AI Red Teaming: hand the spec to a scan."


def rel(path):
    return os.path.relpath(path, ROOT)


def labels(tree, role, prefix):
    """Distinct labels (the tree can list a control more than once), in order."""
    out = []
    for n in tree:
        label = n.get("label") or ""
        if n.get("role") == role and label.startswith(prefix) and label not in out:
            out.append(label)
    return out


def stage_collection(work, origin):
    """A collection in a Git repository with one request to the loopback topics route."""
    repo = os.path.realpath(os.path.join(work, "repo"))
    root = os.path.join(repo, "api")
    os.makedirs(root)
    with open(os.path.join(root, "collection.toml"), "w") as f:
        f.write(f'format_version = 1\nid = "c33"\nname = "Topics API"\n\n[variables]\nbase = "{origin}"\n')
    with open(os.path.join(root, "topics.request.toml"), "w") as f:
        f.write(c15.request("topics", "GET", "{{base}}/topics"))
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
    work = tempfile.mkdtemp(prefix="airs-v2-c33-acceptance-")
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
    endpoint.mode("block")
    mgmt = c8.MgmtEndpoint(os.path.join(work, "endpoint"), os.path.join(work, "mgmt"))
    mgmt.install()
    mgmt.start()
    mgmt.mode()
    api_dir = os.path.join(work, "api-endpoint")
    os.makedirs(api_dir)
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
    app.create_custom_tenant("Tenant A", endpoint.origin)
    app.save_key(c8.SCAN_KEY)
    c8.save_client(app)
    # A second tenant, the destination of an explicit copy.
    app.type_into("AXTextField", "TSG ID (optional)", c8.TSG)
    app.create_custom_tenant("Tenant B", endpoint.origin)
    app.make_active("Tenant A")
    c29.press(app, "AXCheckBox", "Use the new interface")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Home") is not None and c29.nav(t, "Destinations") is not None, 15)

    # ---- N01: Test Cases; Inspect never scans; Open in Playground, run there, result recorded ----
    n1 = {}
    tree = app.tree()
    n1["calls_hidden_by_default"] = not app.has_text(tree, "The calls")
    app.menu("Go", "Test Cases")
    tree, n1["screen"] = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Test Cases") is not None, 15)
    obs["tree_cases_empty"] = c29.save_tree("cases-empty", tree)
    c29.press(app, "AXButton", "Add starter packs", 15)
    tree, n1["packs"] = app.wait_for(lambda t: len(labels(t, "AXButton", "Open in Playground:")) >= 1 and len(labels(t, "AXButton", "Inspect:")) >= 1, 20)
    n1["pack_named"] = app.has_text(tree, "Runtime Security starter pack")
    n1["not_run"] = app.has_text(tree, "Not run")
    n1["runtime_rows"] = len(labels(tree, "AXButton", "Open in Playground:"))
    n1["spec_rows"] = len(labels(tree, "AXButton", "Inspect:"))
    obs["tree_cases"] = c29.save_tree("cases", tree)
    obs["captures"]["cases"] = c29.matrix(app, "cases")
    # Filter and search.
    # The filter's toggle buttons (aria-pressed) are exposed as check boxes.
    filter_role = "AXCheckBox" if labels(tree, "AXCheckBox", "AI Red Teaming (") else "AXButton"
    red = next((l for l in labels(tree, filter_role, "AI Red Teaming (")), None)
    n1["filter_label"] = red
    if red:
        app.ax("press", filter_role, red)
        time.sleep(0.6)
        tree = app.tree()
        n1["filtered_runtime_rows"] = len(labels(tree, "AXButton", "Open in Playground:"))
        n1["filtered_spec_rows"] = len(labels(tree, "AXButton", "Inspect:"))
        all_label = next((l for l in labels(tree, filter_role, "All (")), None)
        if all_label:
            app.ax("press", filter_role, all_label)
            time.sleep(0.4)
    app.type_into("AXTextField", "Search cases", "zzz-no-such-case")
    tree, n1["search_empty"] = app.wait_for(lambda t: app.has_text(t, "No case matches."), 8)
    app.ax("focus", "AXTextField", "Search cases")
    app.keys("a", "command down")
    app.keycode(51)
    app.wait_for(lambda t: len(labels(t, "AXButton", "Inspect:")) >= 1, 8)
    # Inspect a Red Team spec: details, and no scan request at all.
    scans_before = len(endpoint.requests())
    tree = app.tree()
    inspect = next((l for l in labels(tree, "AXButton", "Inspect:")), None)
    red_inspect = None
    for label in labels(tree, "AXButton", "Inspect:"):
        app.ax("press", "AXButton", label)
        time.sleep(0.5)
        if app.has_text(app.tree(), RED_TEAM_RUNS):
            red_inspect = label
            break
        app.ax("press", "AXButton", label)
        time.sleep(0.3)
    tree = app.tree()
    n1["inspect"] = inspect is not None
    n1["red_team_runs_from"] = red_inspect is not None and app.has_text(tree, RED_TEAM_RUNS)
    n1["never_run_note"] = app.has_text(tree, "This screen never runs a case")
    obs["tree_inspect"] = c29.save_tree("cases-inspect", tree)
    obs["captures"]["cases-inspect"] = {"light-100": c29.capture(app, "cases-inspect")}
    time.sleep(1)
    n1["inspect_scans"] = len(endpoint.requests()) - scans_before
    # Open the first Runtime case in the Playground; the operator runs it there.
    runtime_label = labels(app.tree(), "AXButton", "Open in Playground:")[0]
    case_name = runtime_label.split(": ", 1)[1]
    app.ax("press", "AXButton", runtime_label)
    tree, n1["playground"] = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Administration") is not None and app.has_text(t, f"{case_name} is loaded"), 20)
    n1["no_scan_on_open"] = len(endpoint.requests()) == scans_before
    obs["tree_playground"] = c29.save_tree("cases-playground", tree)
    obs["captures"]["cases-playground"] = {"light-100": c29.capture(app, "cases-playground")}
    c29.press(app, "AXButton", "Scan", 10)
    tree, n1["scanned"] = app.wait_for(lambda t: len(endpoint.requests()) == scans_before + 1 and (app.has_text(t, "Blocked") or app.has_text(t, "Block")), 30)
    time.sleep(1)
    app.menu("Go", "Test Cases")
    tree, n1["result_recorded"] = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Test Cases") is not None and app.has_text(t, "Blocked"), 15)
    obs["tree_cases_result"] = c29.save_tree("cases-result", tree)
    obs["captures"]["cases-result"] = {"light-100": c29.capture(app, "cases-result")}
    n1["scans_total"] = len(endpoint.requests()) - scans_before
    obs["n1"] = n1

    # ---- N05: the library's workflows, natively (attempt-02 E2) ----
    n5 = {}
    scans5 = len(endpoint.requests())

    def notice(text, timeout=12):
        return app.wait_for(lambda t: app.has_text(t, text), timeout)

    # A Runtime case, typed into the form.
    c29.press(app, "AXButton", "New case")
    app.wait_for(lambda t: fx.App.node(t, "AXTextField", "Security profile") is not None, 10)
    app.type_into("AXTextField", "Name", "Native runtime case")
    app.type_into("AXTextField", "Security profile", "Production guardrails")
    app.type_into("AXTextField", "Application", "Support bot")
    app.type_into("AXTextArea", "Input or test specification", "Native input")
    c29.press(app, "AXButton", "Save case")
    _, n5["runtime_saved"] = notice("Saved Native runtime case to this tenant's cases.")
    n5["runtime_listed"] = fx.App.node(app.tree(), "AXButton", "Open in Playground: Native runtime case") is not None
    # A specification: with the Red Team filter on, the form starts on that product.
    role5 = "AXCheckBox" if labels(app.tree(), "AXCheckBox", "AI Red Teaming (") else "AXButton"
    app.ax("press", role5, labels(app.tree(), role5, "AI Red Teaming (")[0])
    time.sleep(0.5)
    c29.press(app, "AXButton", "New case")
    app.wait_for(lambda t: fx.App.node(t, "AXTextField", "Name") is not None, 10)
    n5["spec_form"] = fx.App.node(app.tree(), "AXTextField", "Security profile") is None
    app.type_into("AXTextField", "Name", "Native red team spec")
    app.type_into("AXTextArea", "Input or test specification", "Attack the support bot.")
    c29.press(app, "AXButton", "Save case")
    _, n5["spec_saved"] = notice("Saved Native red team spec to this tenant's cases.")
    n5["spec_inspect_only"] = fx.App.node(app.tree(), "AXButton", "Inspect: Native red team spec") is not None and fx.App.node(app.tree(), "AXButton", "Open in Playground: Native red team spec") is None
    app.ax("press", role5, labels(app.tree(), role5, "All (")[0])
    time.sleep(0.5)
    # Remove one.
    app.ax("press", "AXButton", "Remove case Native runtime case")
    _, n5["removed"] = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Open in Playground: Native runtime case") is None, 10)
    # Export: saved, cancelled, failed.
    out = os.path.join(work, "exports")
    os.makedirs(out)
    c29.press(app, "AXButton", "Export…")
    sheets.save_as(app, out)
    _, n5["export_saved"] = notice("Saved prisma-airs-test-cases.json.")
    n5["export_file"] = os.path.exists(os.path.join(out, "prisma-airs-test-cases.json"))
    c29.press(app, "AXButton", "Export…")
    sheets.cancel(app)
    _, n5["export_cancelled"] = notice("Export cancelled. Nothing was saved.")
    locked = os.path.join(work, "locked")
    os.makedirs(locked)
    os.chmod(locked, 0o555)
    try:
        c29.press(app, "AXButton", "Export…")
        sheets.save_as(app, locked)
        _, n5["export_failed"] = notice("The export could not be saved.")
    finally:
        os.chmod(locked, 0o755)
    # Import the exported file: its cases are already here.
    c29.press(app, "AXButton", "Import…")
    sheets.open_file(app, os.path.join(out, "prisma-airs-test-cases.json"))
    _, n5["imported"] = notice("Every case in that file is already in the library.")
    # Copy explicitly to Tenant B.
    app.ax("focus", "AXPopUpButton", "Copy cases to")
    app.keys("Tenant B")
    _, n5["copy_target"] = app.wait_for(lambda t: str((fx.App.node(t, "AXPopUpButton", "Copy cases to") or {}).get("value") or "") == "Tenant B", 8)
    copy_label = next((l for l in labels(app.tree(), "AXButton", "Copy ") if l.endswith(" to Tenant B")), None)
    if copy_label:
        app.ax("press", "AXButton", copy_label)
    _, n5["copied"] = notice("Copied as a new case into Tenant B.")
    tree = app.tree()
    obs["tree_library"] = c29.save_tree("cases-library", tree)
    obs["captures"]["cases-library"] = {"light-100": c29.capture(app, "cases-library")}
    n5["no_scan"] = len(endpoint.requests()) == scans5
    obs["n5"] = n5

    # ---- N02: API Client; completion status region; immutable history ----
    n2 = {}
    app.menu("Go", "API Client")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "API Client") is not None, 15)
    c15.press(app, "Open collection…")
    # With the native sheets on (N05), the folder is chosen in the open panel.
    if sheets.wait_sheet(app, timeout=4):
        sheets.open_file(app, collection_root)
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Open GET topics.request.toml") is not None, 15)
    app.ax("press", "AXButton", "Open GET topics.request.toml")
    app.wait_for(lambda t: fx.App.node(t, "AXTextField", "URL") is not None, 10)
    c29.press(app, "AXButton", "Send")
    tree, n2["status"] = app.wait_for(lambda t: any("topics · 200 · " in r for r in c31.status_texts(t)), 20)
    regions = c31.status_texts(tree)
    n2["status_text"] = next((r for r in regions if "topics · 200" in r), None)
    n2["body_outside"] = app.has_text(tree, '"custom_topics"') and not any("custom_topics" in r for r in regions)
    obs["tree_api_sent"] = c29.save_tree("api-sent", tree)
    obs["captures"]["api-sent"] = c29.matrix(app, "api-sent")
    # A topic is applied in the "service"; a new send reflects it.
    with open(os.path.join(api_dir, "topics.json"), "w") as f:
        json.dump(["Finance", "Travel"], f)
    c29.press(app, "AXButton", "Send")
    tree, n2["second"] = app.wait_for(lambda t: app.has_text(t, "Travel"), 20)
    c15.press(app, "History")
    tree, n2["history"] = app.wait_for(lambda t: len(labels(t, "AXButton", "GET topics · HTTP 200")) >= 2, 15)
    entries = labels(tree, "AXButton", "GET topics · HTTP 200")
    n2["entries"] = len(entries)
    # The older entry (listed last) shows exactly the first response.
    nodes = [n for n in tree if n.get("role") == "AXButton" and (n.get("label") or "").startswith("GET topics · HTTP 200")]
    if nodes:
        older = nodes[-1]["label"]
        app.ax("press", "AXButton", older, sum(1 for n in nodes[:-1] if n["label"] == older))
        time.sleep(1)
    tree = app.tree()
    n2["old_unchanged"] = app.has_text(tree, '{"custom_topics":["Finance"]}')
    obs["tree_api_history"] = c29.save_tree("api-history", tree)
    obs["captures"]["api-history"] = {"light-100": c29.capture(app, "api-history")}
    n2["api_requests"] = len(api.requests("/topics"))
    obs["n2"] = n2

    # ---- N03: "Show the calls" ----
    n3 = {}
    tree = app.tree()
    n3["off_before"] = not app.has_text(tree, "The calls")
    c29.press(app, "AXCheckBox", "Show the calls")
    tree, n3["shown"] = app.wait_for(lambda t: app.has_text(t, "The calls") and app.has_text(t, "/v1/scan/sync/request"), 10)
    texts = " ".join(app.texts(tree))
    n3["scan_line"] = "POST" in texts and "/v1/scan/sync/request" in texts
    n3["credential_kind"] = "scan API key (attached by the core)" in texts
    n3["management"] = "management client token (attached by the core)" in texts
    n3["no_secret"] = c8.SCAN_KEY not in texts and c8.SECRET not in texts
    obs["tree_calls"] = c29.save_tree("calls", tree)
    obs["captures"]["calls"] = c29.matrix(app, "calls")
    c29.press(app, "AXButton", "Hide the calls")
    tree, n3["hidden"] = app.wait_for(lambda t: not app.has_text(t, "The calls"), 10)
    obs["n3"] = n3

    # ---- N04: keyboard, focus ring, 900x600 / 200% ----
    n4 = {}
    app.menu("Go", "Test Cases")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Test Cases") is not None, 15)
    app.ax("focus", "AXButton", "Add starter packs")
    walk = []
    for _ in range(8):
        app.keycode(48)
        f = fx.App.focused(app.tree()) or {}
        walk.append(f.get("label") or "")
    n4["walk"] = walk
    n4["reached_search"] = any("Search cases" in l for l in walk) or any(l.startswith("All (") for l in walk)
    n4["reached_filter"] = any(l.startswith(("All (", "Runtime Security (", "AI Gateway (")) for l in walk)
    ring_plain = c29.capture(app, "cases-ring-plain")
    n4["ring"] = c31.ring_on(app, "Add starter packs", ring_plain, "cases-ring")
    base = c28.window_size(pid)
    c28.set_window_size(pid, 900, 600)
    c29.zoom(app, 10)
    zoom = {}
    for theme in ("Light", "Dark"):
        c29.appearance(app, theme)
        tree = app.tree()
        win = next((n for n in tree if n.get("role") == "AXWindow"), None)
        nodes = [n for n in tree if n.get("pos") and n.get("size") and n["size"][0] > 0
                 and n.get("role") in ("AXButton", "AXStaticText", "AXTextField", "AXSearchField", "AXCheckBox", "AXPopUpButton", "AXLink", "AXHeading")]
        right = max(n["pos"][0] + n["size"][0] for n in nodes) if nodes and win else None
        hbars = [n for n in tree if n.get("role") == "AXScrollBar" and (n.get("orientation") == "horizontal" or (n.get("size") and n["size"][0] > n["size"][1]))]
        zoom[theme.lower()] = {"overflow_px": max(0, right - (win["pos"][0] + win["size"][0])) if right is not None else None,
                               "horizontal_scrollbars": len(hbars),
                               "capture": c29.capture(app, f"cases-min-200-{theme.lower()}"),
                               "tree": c29.save_tree(f"cases-min-200-{theme.lower()}", tree)}
    c29.zoom(app, -10)
    c29.appearance(app, "Match System")
    if base:
        c28.set_window_size(pid, base[0], base[1])
    n4["zoom"] = zoom
    obs["captures"]["cases-min-200"] = {k: v["capture"] for k, v in zoom.items()}
    obs["captures"]["cases-ring"] = {"plain": n4["ring"].get("plain"), "focused": n4["ring"].get("capture")}

    # The API Client by keyboard (E2): from the URL, Tab reaches Send; Space sends; the completion is announced.
    app.menu("Go", "API Client")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "API Client") is not None, 15)
    if fx.App.node(app.tree(), "AXTextField", "URL") is None and fx.App.node(app.tree(), "AXButton", "Open GET topics.request.toml") is not None:
        app.ax("press", "AXButton", "Open GET topics.request.toml")
    app.wait_for(lambda t: fx.App.node(t, "AXTextField", "URL") is not None, 10)
    app.ax("focus", "AXTextField", "URL")
    api_walk = []
    sent_by_key = False
    before = len(api.requests("/topics"))
    for _ in range(4):
        app.keycode(48)
        f = fx.App.focused(app.tree()) or {}
        api_walk.append(f.get("label") or "")
        if (f.get("label") or "") == "Send":
            app.keycode(49)
            sent_by_key = True
            break
    n4["api_walk"] = api_walk
    tree, n4["api_sent_by_key"] = app.wait_for(lambda t: len(api.requests("/topics")) == before + 1 and any("topics · 200 · " in r for r in c31.status_texts(t)), 15)
    n4["api_sent_by_key"] = n4["api_sent_by_key"] and sent_by_key
    api_plain = c29.capture(app, "api-ring-plain")
    n4["api_ring"] = c31.ring_on(app, "Send", api_plain, "api-ring")
    c28.set_window_size(pid, 900, 600)
    c29.zoom(app, 10)
    api_zoom = {}
    for theme in ("Light", "Dark"):
        c29.appearance(app, theme)
        tree = app.tree()
        win = next((n for n in tree if n.get("role") == "AXWindow"), None)
        nodes = [n for n in tree if n.get("pos") and n.get("size") and n["size"][0] > 0
                 and n.get("role") in ("AXButton", "AXStaticText", "AXTextField", "AXTextArea", "AXComboBox", "AXCheckBox", "AXPopUpButton", "AXHeading")]
        right = max(n["pos"][0] + n["size"][0] for n in nodes) if nodes and win else None
        hbars = [n for n in tree if n.get("role") == "AXScrollBar" and (n.get("orientation") == "horizontal" or (n.get("size") and n["size"][0] > n["size"][1]))]
        api_zoom[theme.lower()] = {"overflow_px": max(0, right - (win["pos"][0] + win["size"][0])) if right is not None else None,
                                   "horizontal_scrollbars": len(hbars),
                                   "capture": c29.capture(app, f"api-min-200-{theme.lower()}"),
                                   "tree": c29.save_tree(f"api-min-200-{theme.lower()}", tree)}
    c29.zoom(app, -10)
    c29.appearance(app, "Match System")
    if base:
        c28.set_window_size(pid, base[0], base[1])
    n4["api_zoom"] = api_zoom
    obs["captures"]["api-min-200"] = {k: v["capture"] for k, v in api_zoom.items()}
    obs["captures"]["api-ring"] = {"plain": n4["api_ring"].get("plain"), "focused": n4["api_ring"].get("capture")}
    # The calls panel by keyboard: its control is reachable, takes the ring, and Space hides the panel.
    c29.press(app, "AXCheckBox", "Show the calls")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Hide the calls") is not None, 10)
    calls_plain = c29.capture(app, "calls-ring-plain")
    n4["calls_ring"] = c31.ring_on(app, "Hide the calls", calls_plain, "calls-ring")
    app.keycode(48)
    n4["calls_next"] = (fx.App.focused(app.tree()) or {}).get("role")
    app.ax("focus", "AXButton", "Hide the calls")
    time.sleep(0.4)
    app.keycode(49)
    _, n4["calls_hidden_by_key"] = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Hide the calls") is None, 10)
    obs["captures"]["calls-ring"] = {"plain": n4["calls_ring"].get("plain"), "focused": n4["calls_ring"].get("capture")}
    obs["n4"] = n4
    app.quit()

    run_path = os.path.join(NATIVE, "observations.json")
    with open(run_path, "w") as f:
        json.dump(json.loads(json.dumps({"observations": obs, "log": LOG}).replace(rel(NATIVE), rel(FINAL))), f, indent=1)
    run_rel = rel(run_path)
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C33"]}
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [e for e in evidence if e]})

    caps = lambda name: [v for v in obs["captures"].get(name, {}).values() if v]  # noqa: E731
    add("C33-N01", n1["screen"] and n1["packs"] and n1["pack_named"] and n1["not_run"] and n1.get("filtered_runtime_rows") == 0 and (n1.get("filtered_spec_rows") or 0) >= 1
        and n1["search_empty"] and n1["inspect"] and n1["red_team_runs_from"] and n1["never_run_note"] and n1["inspect_scans"] == 0
        and n1["playground"] and n1["no_scan_on_open"] and n1["scanned"] and n1["result_recorded"] and n1["scans_total"] == 1,
        f"Test Cases screen {n1['screen']}; Add starter packs listed {n1['runtime_rows']} Runtime rows (Open in Playground) and {n1['spec_rows']} specification rows (Inspect), pack named {n1['pack_named']}, Not run {n1['not_run']}; filter {n1.get('filter_label')} showed {n1.get('filtered_spec_rows')} rows, none Runtime ({n1.get('filtered_runtime_rows')}); search with no match said so {n1['search_empty']}; Inspect showed the Red Team spec's 'hand the spec to a scan' {n1['red_team_runs_from']} and the never-run note {n1['never_run_note']} with {n1['inspect_scans']} scan requests; Open in Playground opened Administration's Playground with the case loaded {n1['playground']} and no scan {n1['no_scan_on_open']}; the operator's Scan sent one request {n1['scanned']}; Test Cases then showed its result {n1['result_recorded']} ({n1['scans_total']} scan in all)",
        [run_rel, obs["tree_cases"], obs["tree_inspect"], obs["tree_playground"], obs["tree_cases_result"]] + caps("cases") + caps("cases-inspect") + caps("cases-playground") + caps("cases-result"))
    add("C33-N02", n2["status"] and n2["body_outside"] and n2["second"] and n2["history"] and n2["entries"] >= 2 and n2["old_unchanged"] and n2["api_requests"] == 2,
        f"Send announced '{n2['status_text']}' in a status region {n2['status']} with the body outside it {n2['body_outside']}; after the topic was applied a new send reflected it {n2['second']}; History listed {n2['entries']} entries and the older one showed exactly the first response {n2['old_unchanged']}; {n2['api_requests']} requests reached the loopback endpoint, nothing resent",
        [run_rel, obs["tree_api_sent"], obs["tree_api_history"]] + caps("api-sent") + caps("api-history"))
    add("C33-N03", n1["calls_hidden_by_default"] and n3["off_before"] and n3["shown"] and n3["scan_line"] and n3["credential_kind"] and n3["no_secret"] and n3["hidden"],
        f"hidden by default once a tenant exists {n1['calls_hidden_by_default'] and n3['off_before']}; Show the calls listed the scan's request line {n3['scan_line']} with its credential by kind {n3['credential_kind']} (management calls {n3['management']}); no secret anywhere in the window {n3['no_secret']}; Hide the calls removed the panel {n3['hidden']}",
        [run_rel, obs["tree_calls"]] + caps("calls"))
    n5 = obs["n5"]
    add("C33-N05", all(n5.get(k) for k in ("runtime_saved", "runtime_listed", "spec_form", "spec_saved", "spec_inspect_only", "removed", "export_saved", "export_file", "export_cancelled", "export_failed", "imported", "copy_target", "copied", "no_scan")),
        f"New case saved a Runtime case {n5['runtime_saved']} listed with Open in Playground {n5['runtime_listed']}, and with the Red Team filter a specification {n5['spec_saved']} (no profile field {n5['spec_form']}, Inspect only {n5['spec_inspect_only']}); Remove removed one {n5['removed']}; Export saved the file {n5['export_saved'] and n5['export_file']}, cancelled said so {n5['export_cancelled']}, a read-only folder failed and said so {n5['export_failed']}; Import of the exported file reported its cases already present {n5['imported']}; Copy cases to Tenant B {n5['copy_target']} copied one {n5['copied']}; no scan request throughout {n5['no_scan']}",
        [run_rel, obs["tree_library"]] + caps("cases-library"))
    zoom_ok = all(v["overflow_px"] == 0 and v["horizontal_scrollbars"] == 0 for v in n4["zoom"].values())
    api_zoom_ok = all(v["overflow_px"] == 0 and v["horizontal_scrollbars"] == 0 for v in n4["api_zoom"].values())
    ring_ok = n4["ring"].get("diff", 0) > 150 and n4["ring"].get("focused") == "Add starter packs"
    api_ring_ok = n4["api_ring"].get("diff", 0) > 150 and n4["api_ring"].get("focused") == "Send"
    calls_ring_ok = n4["calls_ring"].get("diff", 0) > 150 and n4["calls_ring"].get("focused") == "Hide the calls"
    add("C33-N04", n4["reached_search"] and n4["reached_filter"] and zoom_ok and ring_ok
        and n4["api_sent_by_key"] and api_ring_ok and api_zoom_ok and calls_ring_ok and n4["calls_hidden_by_key"],
        f"Test Cases: Tab from Add starter packs reached {n4['walk']} (search {n4['reached_search']}, filter {n4['reached_filter']}); ring {n4['ring'].get('diff')} px ({ring_ok}); 900×600 at 200% overflow light {n4['zoom']['light']['overflow_px']} px, dark {n4['zoom']['dark']['overflow_px']} px, scroll bars {[v['horizontal_scrollbars'] for v in n4['zoom'].values()]}. API Client: Tab from URL reached {n4['api_walk']} and Space on Send sent and announced it {n4['api_sent_by_key']}; ring on Send {n4['api_ring'].get('diff')} px ({api_ring_ok}); 900×600 at 200% overflow light {n4['api_zoom']['light']['overflow_px']} px, dark {n4['api_zoom']['dark']['overflow_px']} px, scroll bars {[v['horizontal_scrollbars'] for v in n4['api_zoom'].values()]}. Calls panel: ring on Hide the calls {n4['calls_ring'].get('diff')} px ({calls_ring_ok}), Tab moved on to {n4['calls_next']}, Space hid it {n4['calls_hidden_by_key']}",
        [run_rel] + [v["tree"] for v in n4["zoom"].values()] + [v["tree"] for v in n4["api_zoom"].values()] + caps("cases-min-200") + caps("cases-ring") + caps("api-min-200") + caps("api-ring") + caps("calls-ring"))

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1", "subject": "C33", "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (System Events and the ax helper) with fixture WebView self-capture; tools/native-acceptance/acceptance_c33.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "Pixels come from the fixture build's own WKWebView snapshot; the collection folder is chosen through the fixture folder adapter.",
            "Scan, management and API endpoints are synthetic services on this Mac (scan_server.py, mgmt_server.py, workbench_server.py).",
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
