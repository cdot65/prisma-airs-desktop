#!/usr/bin/env python3
"""C03 native acceptance: drive the prepared fixture app through macOS
Accessibility against the synthetic localhost TLS scan endpoint
(scan_server.py) and evaluate the C03 inventory
(docs/architecture/native-checks.json).

Run from the repository root after `xtask native --chunk C03` prepared the
fixture bundle. Requires the Accessibility grant for the invoking terminal.
It starts from an empty fixture data root, installs the disposable test CA
as the fixture build's extra trust root, uses only synthetic keys, never
reads a stored secret back, never touches the login keychain or v1 data,
and removes the fixture root at the end.

Launches:
1. no endpoint: offline launch, edit, scan (unknown), endpoint returns, no
   automatic send, explicit Scan; then tenants, keyboard, literal request,
   verdicts, denial, cancel, tenant switch, lost response, v1-sized input;
2. in-flight scan, quit;
3. relaunch: no replay; zoom and minimum size; persistence check.
"""

import json
import os
import shutil
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fixture_app as fx  # noqa: E402
import v2_setup as v2s  # noqa: E402

ROOT = os.getcwd()
# A phase review runs every member chunk's checks into the phase's own
# evidence (AIRS_NATIVE_SUBJECT=Pxx); tools/native-acceptance/merge_phase.py
# then combines the per-chunk records.
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C03")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C03" else "native-C03")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C03" else "native-acceptance-C03.json")
KEY_A = "synthetic-c03-native-key-alpha"
PROFILE = "Production guardrails"
APP_NAME = "Desktop workbench"
MARK = "c03-marker"
LOG = []


def sha256(path):
    import hashlib
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def rel(path):
    return os.path.relpath(path, ROOT)


def save_tree(name, tree):
    path = os.path.join(NATIVE, f"tree-{name}.json")
    with open(path, "w") as f:
        json.dump(tree, f, indent=0)
    return rel(path)


def main():
    try:
        return run()
    finally:
        fx.stop_app()
        fx.clean_fixture_root()


def scan(app, prompt, profile=PROFILE, app_name=None):
    """Fills the form through the keyboard and presses Scan."""
    app.type_into("AXTextField", "Profile name or ID", profile)
    app.type_into("AXTextArea", "Prompt", prompt)
    if app_name is not None:
        app.type_into("AXTextField", "Application name (optional)", app_name)
    app.ax("press", "AXButton", "Scan", 0)


def settled(app, fragment, timeout=12):
    return app.wait_for(lambda t: app.has_text(t, fragment), timeout)


def panel(tree):
    """Nodes inside the 'Scan result' region (the flat tree keeps depth)."""
    for i, n in enumerate(tree):
        if (n.get("label") or "") == "Scan result":
            inside = []
            for m in tree[i + 1:]:
                if m["depth"] <= n["depth"]:
                    break
                inside.append(m)
            return inside
    return []


def labels(nodes):
    return {n.get("label") or "" for n in nodes} | {str(n.get("value")) for n in nodes if n.get("value") is not None}


def scan_enabled(tree):
    button = fx.App.node(tree, "AXButton", "Scan")
    return bool(button and button.get("label") == "Scan" and button.get("enabled"))


def run():
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop")
    if sha256(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c03-acceptance-")
    if os.path.isdir(NATIVE):
        shutil.rmtree(NATIVE)
    os.makedirs(NATIVE)
    app = fx.App(bundle, work, LOG)
    app.quit()
    fx.clean_fixture_root()
    v1_before = fx.v1_metadata()
    endpoint = fx.ScanEndpoint(os.path.join(work, "endpoint"), KEY_A)
    endpoint.install_roots()
    obs = {"origin": endpoint.origin}

    # ---- launch 1: offline (endpoint not running) ----
    app.launch()
    tree, _ = app.wait_for(lambda t: fx.App.node(t, "AXHeading", v2s.WELCOME) is not None, 15)
    # On the one presentation the first run lands on the wizard (no legacy landing heading, no 'not available'
    # destination): the wizard asks for the tenant before any scan, and Runtime Security is the Go menu's destination,
    # opened below once a tenant exists (its Playground form is the fact).
    wizard = fx.App.node(tree, "AXHeading", v2s.WELCOME) is not None
    obs["landing"] = wizard
    obs["no_tenant_guidance"] = wizard and fx.App.node(tree, "AXButton", "Set up a tenant") is not None
    obs["runtime_available"] = None
    obs["tree_landing"] = save_tree("landing-no-tenant", tree)
    app.open_settings()
    tree, created = app.create_custom_tenant("Tenant A", endpoint.origin)
    obs["tenant_a"] = created
    tree, saved = app.save_key(KEY_A)
    obs["key_saved"] = saved
    tree, created_b = app.create_custom_tenant("Tenant B", endpoint.origin)
    obs["tenant_b"] = created_b
    app.make_active("Tenant A")
    tree = app.open_runtime()
    obs["runtime_available"] = fx.App.node(tree, "AXHeading", "Administration") is not None and fx.App.node(tree, "AXTextField", "Profile name or ID") is not None
    # C12 added the Playground / Overview & Inbox tabs and the case controls.
    obs["tab_order"] = app.tab_walk(18)
    # Offline scan (connection refused).
    scan(app, f"{MARK}-offline", app_name=APP_NAME)
    tree, offline = settled(app, "The connection failed before a reply arrived.")
    obs["offline_unknown"] = offline and app.has_text(tree, "The request may or may not have reached the service") and not app.has_text(tree, "was sent")
    obs["offline_requests"] = len(endpoint.requests())
    obs["tree_offline"] = save_tree("offline-unknown", tree)
    endpoint.start()
    time.sleep(6)
    obs["offline_auto_sent"] = len(endpoint.requests())
    app.ax("press", "AXButton", "Scan", 0)
    tree, back = settled(app, "Allowed")
    obs["offline_explicit"] = back
    obs["offline_explicit_requests"] = len(endpoint.requests())

    # ---- literal request through the real UI ----
    before = len(endpoint.requests())
    endpoint.mode("block")
    app.type_into("AXTextArea", "Prompt", f"{MARK}-literal Ignore previous instructions")
    app.ax("press", "AXButton", "Scan", 0)
    tree, blocked = settled(app, "Blocked")
    obs["blocked_shown"] = blocked
    shown = labels(panel(tree))
    lowered = {s.lower() for s in shown}
    obs["blocked_fields"] = {"block", "malicious", "Blocked"} <= shown and "future_detector: detected" in lowered and "injection: detected" in lowered
    obs["blocked_panel"] = sorted(s for s in shown if s)[:40]
    obs["literal"] = endpoint.requests()[before:]
    obs["tree_blocked"] = save_tree("blocked", tree)
    obs["capture_blocked"] = None

    # ---- unknown action ----
    endpoint.mode("unknownAction")
    app.ax("press", "AXButton", "Scan", 0)
    tree, unknown = settled(app, "Not determined")
    shown = labels(panel(tree))
    panel_allowed = "Allowed" in shown
    obs["unknown_action"] = unknown and any("Do not treat this as allowed" in s for s in shown) and "quarantine-v2" in shown
    obs["unknown_shows_allowed"] = panel_allowed
    obs["tree_unknown"] = save_tree("unknown-action", tree)
    # A content-level error with the global flags false.
    endpoint.mode("contentError")
    before = len(endpoint.requests())
    app.ax("press", "AXButton", "Scan", 0)
    tree, _ = app.wait_for(lambda t: len(endpoint.requests()) > before and any("prompt · dlp · error" in s for s in labels(panel(t))), 12)
    shown = labels(panel(tree))
    obs["content_error"] = "Not determined" in shown and "Allowed" not in shown and any("prompt · dlp · error" in s for s in shown)
    obs["tree_content_error"] = save_tree("content-error", tree)

    # ---- denied key ----
    before = len(endpoint.requests())
    endpoint.mode("401")
    app.ax("press", "AXButton", "Scan", 0)
    tree, denied = settled(app, "did not accept the scan key (HTTP 401)")
    time.sleep(2)
    obs["denied"] = denied and app.has_text(tree, "The service refused the request; it was not scanned.")
    obs["denied_requests"] = len(endpoint.requests()) - before
    obs["tree_denied"] = save_tree("denied", tree)

    # ---- cancel while the reply is delayed ----
    before = len(endpoint.requests())
    endpoint.mode("allow", 4000)
    app.ax("press", "AXButton", "Scan", 0)
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Cancel scan") is not None, 5)
    time.sleep(0.5)
    app.ax("press", "AXButton", "Cancel scan")
    tree, cancelled = settled(app, "Cancelled. The request may already have reached the service")
    obs["cancelled"] = cancelled
    time.sleep(5)
    tree = app.tree()
    shown = labels(panel(tree))
    obs["cancel_no_verdict"] = "Allowed" not in shown and any("Cancelled. The request may already have reached the service" in s for s in shown)
    obs["cancel_scan_enabled"] = scan_enabled(tree)
    obs["cancel_requests"] = len(endpoint.requests()) - before
    obs["tree_cancelled"] = save_tree("cancelled", tree)

    # ---- tenant switch mid-request ----
    before = len(endpoint.requests())
    endpoint.mode("allow", 5000)
    app.type_into("AXTextArea", "Prompt", f"{MARK}-switch")
    app.ax("press", "AXButton", "Scan", 0)
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Cancel scan") is not None, 5)
    app.open_settings()
    app.make_active("Tenant B")
    tree = app.open_runtime()
    obs["b_sees_none"] = fx.App.node(tree, "AXHeading", "Recent scans") is None and not app.has_text(tree, "Scan result")
    obs["tree_tenant_b"] = save_tree("tenant-b", tree)
    time.sleep(6)
    app.open_settings()
    app.make_active("Tenant A")
    tree = app.open_runtime()
    shown = labels(panel(tree))
    obs["a_discarded"] = any("Any reply was discarded and is not shown." in s for s in shown) and "Allowed" not in shown
    obs["switch_requests"] = len(endpoint.requests()) - before
    obs["tree_tenant_a_back"] = save_tree("tenant-a-after-switch", tree)

    # ---- lost response ----
    before = len(endpoint.requests())
    endpoint.mode("close")
    app.type_into("AXTextField", "Profile name or ID", PROFILE)
    app.type_into("AXTextArea", "Prompt", f"{MARK}-lost")
    app.ax("press", "AXButton", "Scan", 0)
    tree, lost = settled(app, "The connection failed before a reply arrived.")
    obs["lost"] = lost and app.has_text(tree, "Press Scan to send it again only if a second scan is acceptable.")
    time.sleep(6)
    obs["lost_auto"] = len(endpoint.requests()) - before
    endpoint.mode("allow")
    app.ax("press", "AXButton", "Scan", 0)
    settled(app, "Allowed")
    obs["lost_explicit"] = len(endpoint.requests()) - before
    obs["tree_lost"] = save_tree("lost", tree)

    # ---- keyboard-only scan ----
    before = len(endpoint.requests())
    endpoint.mode("block")
    app.ax("focus", "AXTextField", "Profile name or ID")
    app.keys("a", "command down")
    app.keys("Keyboard profile")
    app.keycode(48)  # Tab to Prompt
    app.keys("a", "command down")
    app.keys(f"{MARK} keyboard")
    tree = app.tree()
    obs["keyboard_focus_prompt"] = (app.focused(tree) or {}).get("label") == "Prompt"
    for _ in range(3):
        app.keycode(48)
    tree = app.tree()
    focused = app.focused(tree) or {}
    obs["keyboard_focus_scan"] = focused.get("role") == "AXButton" and focused.get("label") == "Scan"
    app.keycode(49)  # Space activates the focused Scan button
    tree, announced = settled(app, "Blocked")
    status_nodes = [n for n in tree if n.get("subrole") == "AXApplicationStatus" or n.get("role") == "AXApplicationStatus"]
    obs["keyboard_verdict"] = announced
    obs["keyboard_status_regions"] = len(status_nodes)
    obs["keyboard_request"] = endpoint.requests()[before:]
    obs["tree_keyboard"] = save_tree("keyboard", tree)

    # ---- v1-sized input and the 2 MiB ceiling ----
    before = len(endpoint.requests())
    endpoint.mode("allow")
    prompt_file = os.path.join(work, "prompt.txt")
    response_file = os.path.join(work, "response.txt")
    with open(prompt_file, "w") as f:
        f.write("p" * (64 * 1024))
    with open(response_file, "w") as f:
        f.write("r" * (32 * 1024))
    obs["v1_typed"] = app.type_file("AXTextArea", "Prompt", prompt_file) and app.type_file("AXTextArea", "Response", response_file)
    app.ax("press", "AXButton", "Scan", 0)
    tree, v1_ok = settled(app, "Allowed", 20)
    obs["v1_verdict"] = v1_ok
    obs["v1_request"] = [{k: r.get(k) for k in ("promptChars", "responseChars", "bodyBytes", "tokenMatches", "payloadHashMatches")} for r in endpoint.requests()[before:]]
    obs["tree_v1"] = save_tree("v1-sized", tree)
    app.ax("focus", "AXTextArea", "Response")
    app.keys("a", "command down")
    app.keycode(51)

    # ---- launch 2: quit while a scan is in flight ----
    endpoint.mode("allow", 4000)
    app.type_into("AXTextArea", "Prompt", f"{MARK}-inflight")
    before = len(endpoint.requests())
    app.ax("press", "AXButton", "Scan", 0)
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Cancel scan") is not None, 5)
    obs["inflight_sent"] = len(endpoint.requests()) - before
    app.quit()
    time.sleep(5)
    after_quit = len(endpoint.requests())

    # ---- launch 3: no replay; zoom and minimum size ----
    endpoint.mode("allow")
    app.launch()
    time.sleep(6)
    tree = app.tree()
    obs["relaunch_history"] = fx.App.node(tree, "AXHeading", "Recent scans") is not None
    obs["relaunch_replayed"] = len(endpoint.requests()) - after_quit
    obs["tree_relaunch"] = save_tree("relaunch", tree)
    for _ in range(10):
        app.keys("=", "command down")
    obs["zoom"] = reach_scan(app, 2)
    app.keys("0", "command down")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{900, 600}}'])
    time.sleep(0.8)
    obs["min_size"] = reach_scan(app, 1)
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{1280, 820}}'])
    trees = [os.path.join(NATIVE, n) for n in os.listdir(NATIVE) if n.startswith("tree-")]
    obs["key_in_trees"] = [rel(p) for p in trees if KEY_A in open(p).read()]
    app.quit()
    endpoint.stop()
    obs["persisted_secrets"] = scan_data_root()
    obs["v1_unchanged"] = fx.v1_metadata() == v1_before
    obs["requests"] = endpoint.requests()
    shutil.rmtree(work, ignore_errors=True)

    with open(os.path.join(NATIVE, "observations.json"), "w") as f:
        json.dump({"observations": obs, "log": LOG}, f, indent=1)
    return evaluate(prepared, obs)


def reach_scan(app, zoom):
    """Tabs until the Prompt field, then the Scan button, have focus; both
    must lie inside the window and end above the 60 CSS px bottom fade."""
    window = next(n for n in app.tree() if n.get("role") == "AXWindow")
    bottom = window["pos"][1] + window["size"][1]
    found = {}
    app.ax("press", "AXButton", "Runtime Security")
    for _ in range(60):
        app.keycode(48)
        f = app.focused(app.tree())
        if f and f.get("label") == "Prompt" and "prompt" not in found:
            found["prompt"] = {"pos": f["pos"], "size": f["size"]}
        if f and f.get("role") == "AXButton" and f.get("label") == "Scan" and "prompt" in found:
            found["scan"] = {"pos": f["pos"], "size": f["size"]}
            break
    # Inside the window: the v2 shell has no bottom fade (the legacy 60 CSS px band).
    ok = all(k in found and found[k]["pos"][1] >= window["pos"][1]
             and found[k]["pos"][1] + found[k]["size"][1] <= bottom + 1 for k in ("prompt", "scan"))
    return {"zoom": zoom, "window": {"pos": window["pos"], "size": window["size"]}, "found": found, "clear": ok}


WEBKIT_DIRS = [os.path.join(fx.HOME, "Library/WebKit/io.cdot.airs-desktop.fixture"),
               os.path.join(fx.HOME, "Library/Caches/io.cdot.airs-desktop.fixture")]


def scan_data_root():
    """Files in the fixture data root (other than the fixture keychain) and
    the fixture app's WebKit data and caches that contain the synthetic key
    or a prompt marker."""
    hits, files = [], []
    for root in [fx.DATA] + WEBKIT_DIRS:
        for base, _, names in os.walk(root):
            for name in names:
                path = os.path.join(base, name)
                if path == fx.KEYCHAIN:
                    continue
                files.append(path.replace(fx.HOME, "~"))
                try:
                    data = open(path, "rb").read()
                except OSError:
                    continue
                if KEY_A.encode() in data or MARK.encode() in data:
                    hits.append(path.replace(fx.HOME, "~"))
    return {"files": sorted(files), "hits": hits}


def evaluate(prepared, o):
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C03"]}
    obs_path = rel(os.path.join(NATIVE, "observations.json"))
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [obs_path] + [e for e in evidence if e]})

    labels = [s and s["label"] for s in o["tab_order"]]
    want = ["By name", "Profile name or ID", "Prompt", "Response", "Application name (optional)", "Scan"]
    seq = [w for w in want if w in labels]
    in_order = seq == want and [labels.index(w) for w in want] == sorted(labels.index(w) for w in want)
    add("C03-N01", o["runtime_available"] and o["landing"] and o["no_tenant_guidance"] and in_order,
        f"Runtime Security opens the Runtime card's Playground (one presentation): {o['runtime_available']}; the first run lands on the wizard: {o['landing']}; the wizard asks for the tenant before any scan: "
        f"{o['no_tenant_guidance']}; Tab reached {seq} in order: {in_order}",
        [o["tree_landing"]])
    lit = o["literal"]
    r = lit[0] if lit else {}
    literal_ok = (len(lit) == 1 and r.get("method") == "POST" and r.get("path") == "/v1/scan/sync/request" and r.get("contentType") == "application/json"
                  and r.get("tokenMatches") and r.get("payloadHashMatches") and not r.get("authorization")
                  and r.get("profile") == {"profile_name": PROFILE} and r.get("metadata") == {"app_name": APP_NAME}
                  and str(r.get("promptHead", "")).startswith(f"{MARK}-literal"))
    add("C03-N02", literal_ok and o["blocked_shown"] and o["blocked_fields"],
        f"endpoint requests for the scan: {len(lit)}; {r.get('method')} {r.get('path')} content-type {r.get('contentType')}; token equals the synthetic key: {r.get('tokenMatches')}; "
        f"x-payload-hash is the HMAC of the received body: {r.get('payloadHashMatches')}; Authorization present: {r.get('authorization')}; profile {r.get('profile')}; metadata {r.get('metadata')}; "
        f"'Blocked' shown: {o['blocked_shown']}; action/category/detectors shown: {o['blocked_fields']}",
        [o["tree_blocked"]])
    add("C03-N03", o["unknown_action"] and not o["unknown_shows_allowed"] and o["content_error"],
        f"'Not determined' with 'Do not treat this as allowed' and the verbatim action 'quarantine-v2': {o['unknown_action']}; a bare 'Allowed' label present: {o['unknown_shows_allowed']}; "
        f"allow/benign with a prompt DLP content error and global flags false shown as 'Not determined' with the content error listed and no 'Allowed': {o['content_error']}",
        [o["tree_unknown"], o["tree_content_error"]])
    add("C03-N04", o["denied"] and o["denied_requests"] == 1,
        f"401 reported as not accepted and refused: {o['denied']}; endpoint requests: {o['denied_requests']}",
        [o["tree_denied"]])
    add("C03-N05", o["cancelled"] and o["cancel_no_verdict"] and o["cancel_scan_enabled"] and o["cancel_requests"] == 1,
        f"'Cancelled. The request may already have reached the service': {o['cancelled']}; no verdict after the delayed reply: {o['cancel_no_verdict']}; Scan enabled: {o['cancel_scan_enabled']}; "
        f"endpoint requests: {o['cancel_requests']}",
        [o["tree_cancelled"]])
    add("C03-N06", o["b_sees_none"] and o["a_discarded"] and o["switch_requests"] == 1,
        f"Tenant B's Runtime view shows no Tenant A scans: {o['b_sees_none']}; Tenant A's record says the reply was discarded: {o['a_discarded']}; endpoint requests: {o['switch_requests']}",
        [o["tree_tenant_b"], o["tree_tenant_a_back"]])
    add("C03-N07", o["lost"] and o["lost_auto"] == 1 and o["lost_explicit"] == 2,
        f"connection failure with unknown outcome and resend guidance: {o['lost']}; requests 6 s later: {o['lost_auto']}; after pressing Scan: {o['lost_explicit']}",
        [o["tree_lost"]])
    add("C03-N08", o["offline_unknown"] and o["offline_requests"] == 0 and o["offline_auto_sent"] == 0 and o["offline_explicit"] and o["offline_explicit_requests"] == 1,
        f"offline launch and scan reported unknown (connection failed; 'may or may not have reached the service', never 'was sent'): {o['offline_unknown']}; requests while offline: {o['offline_requests']}; 6 s after the endpoint returned: "
        f"{o['offline_auto_sent']}; explicit Scan: verdict {o['offline_explicit']}, requests {o['offline_explicit_requests']}",
        [o["tree_offline"]])
    add("C03-N09", o["inflight_sent"] == 1 and not o["relaunch_history"] and o["relaunch_replayed"] == 0,
        f"in-flight scan sent before quitting: {o['inflight_sent']}; history after relaunch: {o['relaunch_history']}; requests within 6 s of relaunch: {o['relaunch_replayed']}",
        [o["tree_relaunch"]])
    kr = o["keyboard_request"]
    add("C03-N10", o["keyboard_focus_prompt"] and o["keyboard_focus_scan"] and o["keyboard_verdict"] and o["keyboard_status_regions"] > 0 and len(kr) == 1
        and kr[0].get("profile") == {"profile_name": "Keyboard profile"},
        f"Tab reached Prompt: {o['keyboard_focus_prompt']}; then Scan: {o['keyboard_focus_scan']}; Space submitted and 'Blocked' shown: {o['keyboard_verdict']}; "
        f"AX status regions: {o['keyboard_status_regions']}; endpoint requests {len(kr)} with profile {kr[0].get('profile') if kr else None}",
        [o["tree_keyboard"]])
    v1 = o["v1_request"]
    add("C03-N11", o["v1_typed"] and o["v1_verdict"] and len(v1) == 1 and v1[0]["promptChars"] == 64 * 1024 and v1[0]["responseChars"] == 32 * 1024
        and v1[0]["payloadHashMatches"],
        f"64 KiB prompt and 32 KiB response typed and present in the fields: {o['v1_typed']}; request received {v1}; verdict shown: {o['v1_verdict']}",
        [o["tree_v1"]])
    add("C03-N12", o["zoom"]["clear"] and o["min_size"]["clear"],
        f"200% zoom: {o['zoom']['found']} in window {o['zoom']['window']}, clear: {o['zoom']['clear']}; minimum size: {o['min_size']['found']} in window {o['min_size']['window']}, "
        f"clear: {o['min_size']['clear']}",
        [])
    add("C03-N13", not o["persisted_secrets"]["hits"] and not o["key_in_trees"],
        f"data root files {o['persisted_secrets']['files']}; files containing the key or a prompt marker: {o['persisted_secrets']['hits']}; AX snapshots containing the key: {o['key_in_trees']}",
        [])

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1",
        "subject": "C03",
        "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (tools/native-acceptance/ax.swift, System Events keystrokes) against the synthetic TLS endpoint tools/native-acceptance/scan_server.py; tools/native-acceptance/acceptance_c03.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "The endpoint is a local synthetic fixture with a disposable test CA trusted only by the fixture build; live Prisma AIRS behavior is qualified in C25.",
            "v1-sized input is typed as key events posted to the app's process (20 characters per event) because System Events typing is too slow for 96 KiB and the WebView ignores accessibility value writes; smaller inputs are typed through System Events. The 2 MiB and 8 MiB refusals are covered by scan-submit.test.tsx and sync_scan, not natively.",
            "Offline is simulated by a stopped local endpoint (connection refused), not by disabling the host network.",
            "Spoken VoiceOver output was not recorded; AX roles, names, focus and live status regions are inspected.",
        ],
    }
    with open(RECORD, "w") as f:
        json.dump(record, f, indent=2)
        f.write("\n")
    for c in checks:
        print(c["result"].upper(), c["id"], c["observation"][:200])
    return 0 if all(c["result"] == "pass" for c in checks) else 1


if __name__ == "__main__":
    sys.exit(main())
