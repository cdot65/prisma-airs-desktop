#!/usr/bin/env python3
"""C08 native acceptance: drive the prepared fixture app through macOS
Accessibility against the synthetic localhost TLS scan endpoint
(scan_server.py) and the synthetic management endpoint (mgmt_server.py),
and evaluate the C08 inventory (docs/architecture/native-checks.json).

Run from the repository root after `xtask native-prepare --chunk C08`
prepared the fixture bundle. Requires the Accessibility grant for the
invoking terminal. It starts from an empty fixture data root, installs the
disposable test CA as the fixture build's extra trust root and the
management endpoint as the fixture build's management origin, uses only
synthetic credentials, never reads a stored secret back, never touches the
login keychain or v1 data, and removes the fixture root at the end.

Launches:
1. Administration without a tenant and without a management client; a
   scan-only scan; management onboarding; a refused grant (scans still
   work); verification; later pages; details and unknown values; denial,
   empty and refresh denial with stale rows; a credential replaced while a
   later page is in flight; zoom and minimum size;
2. relaunch: the Keychain client still reads; removal leaves the scan key.
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

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C08")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C08" else "native-C08")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C08" else "native-acceptance-C08.json")
SCAN_KEY = "synthetic-c08-native-scan-key"
CLIENT_ID = "c08-native@1234567890.iam.panserviceaccount.com"
SECRET = "synthetic-c08-native-client-secret"
TSG = "1234567890"
PROFILE = "Production guardrails"
SCAN_SERVICE = "io.cdot.prisma-airs-desktop.v2.fixture.scan"
MGMT_ORIGIN_FILE = os.path.join(fx.DATA, "fixture-management-origin")
REDACTED = ["SYNTHETIC-C08-KEY-VALUE", "SYNTHETIC-C08-AUTH-CODE", "SYNTHETIC-C08-DP-CODE"]
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


class MgmtEndpoint:
    """The synthetic management endpoint (mgmt_server.py)."""

    def __init__(self, certs, directory):
        self.certs, self.dir = certs, directory
        os.makedirs(directory, exist_ok=True)
        self.port = fx.free_port()
        self.proc = None
        self.mode()

    @property
    def origin(self):
        return f"https://localhost:{self.port}"

    def install(self):
        os.makedirs(fx.DATA, exist_ok=True)
        with open(MGMT_ORIGIN_FILE, "w") as f:
            f.write(self.origin + "\n")

    def start(self):
        self.proc = subprocess.Popen(
            ["python3", "-I", os.path.join(fx.TOOLS, "mgmt_server.py"), "--certs", self.certs, "--dir", self.dir,
             "--port", str(self.port), "--client-id", CLIENT_ID, "--secret", SECRET, "--tsg", TSG],
            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        if self.proc.stdout.readline().strip() != "ready":
            raise SystemExit("the synthetic management endpoint did not start")

    def stop(self):
        if self.proc:
            self.proc.terminate()
            self.proc.wait(timeout=5)
            self.proc = None

    def mode(self, **values):
        with open(os.path.join(self.dir, "mode.json"), "w") as f:
            json.dump(values, f)

    def requests(self):
        path = os.path.join(self.dir, "requests.jsonl")
        if not os.path.exists(path):
            return []
        with open(path) as f:
            return [json.loads(line) for line in f if line.strip()]


def main():
    try:
        return run()
    finally:
        fx.sh(["pkill", "-f", fx.PROCESS])
        fx.clean_fixture_root()


def open_admin(app):
    app.ax("press", "AXButton", "Administration")
    tree, _ = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Tenant inventories") is not None)
    return tree


def text(app, fragment, timeout=12):
    return app.wait_for(lambda t: app.has_text(t, fragment), timeout)


def scan(app, endpoint, marker):
    before = len(endpoint.requests())
    app.open_runtime()
    app.type_into("AXTextField", "Profile name or ID", PROFILE)
    app.type_into("AXTextArea", "Prompt", marker)
    app.ax("press", "AXButton", "Scan", 0)
    tree, allowed = text(app, "Allowed")
    sent = endpoint.requests()[before:]
    return allowed and len(sent) == 1 and sent[0].get("tokenMatches") and not sent[0].get("authorization"), tree


def save_client(app):
    app.type_into("AXTextField", "Client ID", CLIENT_ID)
    app.type_into("AXTextField", "Client secret", SECRET, secure=True)
    app.keycode(36)
    return text(app, "Management client secret saved in the Keychain.")


def reach(app, zoom, label):
    """Tabs until `label` has focus; it must lie inside the window and end
    above the 60 CSS px bottom fade."""
    window = next(n for n in app.tree() if n.get("role") == "AXWindow")
    bottom = window["pos"][1] + window["size"][1]
    found = None
    for _ in range(110):
        app.keycode(48)
        f = app.focused(app.tree())
        if f and f.get("role") == "AXButton" and f.get("label") == label:
            found = {"label": f["label"], "pos": f["pos"], "size": f["size"]}
            break
    clear = bool(found) and found["pos"][1] >= window["pos"][1] and found["pos"][1] + found["size"][1] <= bottom - 60 * zoom
    return {"zoom": zoom, "window": {"pos": window["pos"], "size": window["size"]}, "found": found, "clear": clear}


def run():
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")
    if sha256(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c08-acceptance-")
    if os.path.isdir(NATIVE):
        shutil.rmtree(NATIVE)
    os.makedirs(NATIVE)
    app = fx.App(bundle, work, LOG)
    app.quit()
    fx.clean_fixture_root()
    v1_before = fx.v1_metadata()
    endpoint = fx.ScanEndpoint(os.path.join(work, "endpoint"), SCAN_KEY)
    endpoint.install_roots()
    endpoint.start()
    mgmt = MgmtEndpoint(os.path.join(work, "endpoint"), os.path.join(work, "mgmt"))
    mgmt.install()
    mgmt.start()
    obs = {"scan_origin": endpoint.origin, "management_origin": mgmt.origin}

    # ---- launch 1 ----
    app.launch()
    tree = app.tree()
    obs["admin_available"] = fx.App.node(tree, "AXButton", "Administration") is not None and not any(
        (n.get("label") or "").startswith("Administration") and "Not available" in (n.get("label") or "") for n in tree)
    tree = open_admin(app)
    obs["admin_no_tenant"] = app.has_text(tree, "Add a tenant in Settings to read its inventories.")
    obs["tree_no_tenant"] = save_tree("admin-no-tenant", tree)

    app.open_settings()
    app.type_into("AXTextField", "TSG ID (optional)", TSG)
    tree, created = app.create_custom_tenant("Tenant A", endpoint.origin)
    obs["tenant"] = created
    tree, saved = app.save_key(SCAN_KEY)
    obs["scan_key_saved"] = saved
    obs["scan_items_before"] = fx.keychain_accounts(SCAN_SERVICE)
    tree = open_admin(app)
    obs["admin_not_configured"] = app.has_text(tree, "has no ready management client") and fx.App.node(tree, "AXButton", "Open Settings") is not None
    obs["tree_not_configured"] = save_tree("admin-not-configured", tree)
    obs["scan_only"], tree = scan(app, endpoint, "c08-marker-scan-only")
    obs["mgmt_requests_scan_only"] = len(mgmt.requests())

    # ---- onboarding (keyboard: typed fields, Return submits) ----
    app.open_settings()
    tree = app.tree()
    obs["no_client_status"] = app.has_text(tree, "No management client. Scans do not need one")
    tree, client_saved = save_client(app)
    obs["client_saved"] = client_saved and app.has_text(tree, f"Client ID: {CLIENT_ID}")
    obs["secret_field_cleared"] = str((fx.App.node(tree, "AXTextField", "Client secret") or {}).get("value") or "") == ""
    obs["scan_items_after_client"] = fx.keychain_accounts(SCAN_SERVICE)
    obs["scan_key_still_saved"] = app.has_text(tree, "Scan key saved in the Keychain.")
    obs["tree_client_saved"] = save_tree("client-saved", tree)

    # ---- a refused grant never affects scanning ----
    mgmt.mode(grant="deny")
    before = len(mgmt.requests())
    app.ax("press", "AXButton", "Verify")
    tree, refused = text(app, "The management client was not accepted.")
    obs["grant_refused"] = refused and not app.has_text(tree, "SERVER-TEXT")
    obs["grant_refused_requests"] = mgmt.requests()[before:]
    obs["tree_grant_refused"] = save_tree("grant-refused", tree)
    obs["scan_after_refusal"], _ = scan(app, endpoint, "c08-marker-after-refusal")

    # ---- verification ----
    mgmt.mode()
    app.open_settings()
    before = len(mgmt.requests())
    app.ax("press", "AXButton", "Verify")
    tree, verified = text(app, "The management client can read this tenant.")
    obs["verified"] = verified
    obs["verify_requests"] = mgmt.requests()[before:]
    obs["tree_verified"] = save_tree("verified", tree)

    # ---- later pages; completeness only from the service ----
    before = len(mgmt.requests())
    tree = open_admin(app)
    tree, first = text(app, "Items 1–25 of 25+")
    obs["page1"] = first and app.has_text(tree, "Profile 00") and app.has_text(tree, "Profile 24") and not app.has_text(tree, "Profile 25")
    obs["page1_partial"] = app.has_text(tree, "Rows not yet read are not shown and are not assumed absent.")
    obs["tree_page1"] = save_tree("page1", tree)
    app.ax("press", "AXButton", "Next page")
    tree, second = text(app, "Items 26–30 of 30")
    next_button = fx.App.node(tree, "AXButton", "Next page") or {}
    obs["page2"] = second and app.has_text(tree, "Profile 29") and not app.has_text(tree, "Profile 00")
    obs["page2_next_disabled"] = next_button.get("enabled") is False
    obs["page2_partial_gone"] = not app.has_text(tree, "Rows not yet read")
    obs["tree_page2"] = save_tree("page2", tree)
    obs["page_requests"] = [r for r in mgmt.requests()[before:] if "/profiles/" in r["path"]]

    # ---- details, unknown values, redacted secret fields ----
    app.ax("press", "AXButton", "First page")
    text(app, "Items 1–25 of 25+")
    app.ax("press", "AXButton", "Details for Profile 03")
    tree, detail = text(app, "00000000-0000-4000-8000-000000000003")
    obs["profile_detail"] = detail and app.has_text(tree, "profile_name") and app.has_text(tree, "local previews in the previous desktop")
    obs["tree_profile_detail"] = save_tree("profile-detail", tree)
    app.ax("press", "AXButton", "API keys")
    tree, keys = text(app, "Items 1–1 of 1")
    app.ax("press", "AXButton", "Details for Runtime")
    tree, key_detail = text(app, "FUTURE_STATUS")
    obs["api_key_detail"] = keys and key_detail and app.has_text(tree, "abcd1234")
    obs["redacted_in_tree"] = [s for s in REDACTED if app.has_text(tree, s)]
    obs["tree_api_key_detail"] = save_tree("api-key-detail", tree)
    app.ax("press", "AXButton", "Deployment profiles")
    tree, dps = text(app, "Items 1–3 of 3")
    obs["deployment_profiles"] = dps and app.has_text(tree, "dp-2")
    app.ax("press", "AXButton", "Data patterns")
    tree, patterns = text(app, "Items 1–1 of 2")
    obs["data_patterns"] = patterns and app.has_text(tree, "Card numbers")
    obs["tree_data_patterns"] = save_tree("data-patterns", tree)

    # ---- denial and the service's empty inventory ----
    mgmt.mode(topics="deny")
    app.ax("press", "AXButton", "Custom topics")
    tree, denied = text(app, "This management client is not allowed to read this.")
    obs["topics_denied"] = denied and not app.has_text(tree, "SERVER-TEXT") and not app.has_text(tree, "reports no custom topics")
    obs["tree_topics_denied"] = save_tree("topics-denied", tree)
    mgmt.mode(topics="empty")
    app.ax("press", "AXButton", "Refresh")
    tree, empty = text(app, "The service reports no custom topics in this tenant.")
    obs["topics_empty"] = empty and not app.has_text(tree, "not allowed to read this")
    obs["tree_topics_empty"] = save_tree("topics-empty", tree)

    # ---- refresh denial keeps rows with their age ----
    mgmt.mode()
    app.ax("press", "AXButton", "Security profiles")
    text(app, "Items 1–25 of 25+")
    mgmt.mode(profiles="deny")
    app.ax("press", "AXButton", "Refresh")
    tree, stale = text(app, "They may be out of date.")
    obs["refresh_denied"] = stale and app.has_text(tree, "not allowed to read this") and app.has_text(tree, "Showing rows read at") and app.has_text(tree, "Profile 00")
    obs["tree_refresh_denied"] = save_tree("refresh-denied", tree)
    mgmt.mode()
    app.ax("press", "AXButton", "Refresh")
    text(app, "Items 1–25 of 25+")

    # ---- credential replaced while a later page is in flight ----
    grants_before = len([r for r in mgmt.requests() if r["path"] == "/oauth2/access_token"])
    mgmt.mode(delayMs={"/aisec/v1/mgmt/profiles": 5000})
    app.ax("press", "AXButton", "Next page")
    app.wait_for(lambda t: app.has_text(t, "Reading security profiles"), 5)
    mgmt.mode()
    app.open_settings()
    app.type_into("AXTextField", "Client secret", SECRET, secure=True)
    app.keycode(36)
    tree, replaced = text(app, "Management client secret saved in the Keychain.")
    tree = open_admin(app)
    tree, restarted = text(app, "Items 1–25 of 25+")
    time.sleep(6)
    tree = app.tree()
    obs["replaced"] = replaced
    obs["restarted_first_page"] = restarted and app.has_text(tree, "Items 1–25 of 25+") and not app.has_text(tree, "Items 26–30")
    obs["no_alert_after_late_reply"] = not app.has_text(tree, "The read was stopped") and not app.has_text(tree, "not allowed")
    obs["grants_after_replace"] = len([r for r in mgmt.requests() if r["path"] == "/oauth2/access_token"]) - grants_before
    obs["tree_replaced"] = save_tree("replaced-mid-pagination", tree)

    # ---- zoom and minimum size ----
    for _ in range(10):
        app.keys("=", "command down")
    obs["zoom"] = reach(app, 2, "Next page")
    app.keys("0", "command down")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{900, 600}}'])
    time.sleep(0.8)
    obs["min_size"] = reach(app, 1, "Next page")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{1280, 820}}'])
    app.quit()

    # ---- launch 2: the Keychain client still reads; removal ----
    app.launch()
    tree = open_admin(app)
    tree, reread = text(app, "Items 1–25 of 25+")
    obs["relaunch_reads"] = reread and app.has_text(tree, "Profile 00")
    obs["tree_relaunch"] = save_tree("relaunch", tree)
    app.open_settings()
    app.ax("press", "AXButton", "Remove management client")
    tree, removed = text(app, "No management client. Scans do not need one")
    obs["removed"] = removed and app.has_text(tree, "Scan key saved in the Keychain.")
    obs["scan_items_after_remove"] = fx.keychain_accounts(SCAN_SERVICE)
    tree = open_admin(app)
    obs["admin_after_remove"] = app.has_text(tree, "has no ready management client")
    obs["scan_after_remove"], _ = scan(app, endpoint, "c08-marker-after-remove")
    trees = [os.path.join(NATIVE, n) for n in os.listdir(NATIVE) if n.startswith("tree-")]
    obs["secret_in_trees"] = [rel(p) for p in trees if SECRET in open(p).read() or SCAN_KEY in open(p).read()]
    obs["redacted_in_trees"] = [rel(p) for p in trees if any(s in open(p).read() for s in REDACTED)]
    app.quit()
    endpoint.stop()
    mgmt.stop()
    obs["persisted_secrets"] = scan_data_root()
    obs["v1_unchanged"] = fx.v1_metadata() == v1_before
    obs["mgmt_requests"] = mgmt.requests()
    obs["scan_requests"] = endpoint.requests()
    shutil.rmtree(work, ignore_errors=True)

    with open(os.path.join(NATIVE, "observations.json"), "w") as f:
        json.dump({"observations": obs, "log": LOG}, f, indent=1)
    return evaluate(prepared, obs)


WEBKIT_DIRS = [os.path.join(fx.HOME, "Library/WebKit/io.cdot.prisma-airs-desktop.v2.fixture"),
               os.path.join(fx.HOME, "Library/Caches/io.cdot.prisma-airs-desktop.v2.fixture")]


def scan_data_root():
    """Files in the fixture data root (other than the fixture keychain) and
    the fixture app's WebKit data and caches that contain a synthetic
    secret or a redacted secret field value."""
    hits, files = [], []
    needles = [SECRET.encode(), SCAN_KEY.encode()] + [s.encode() for s in REDACTED]
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
                if any(n in data for n in needles):
                    hits.append(path.replace(fx.HOME, "~"))
    return {"files": sorted(files), "hits": hits}


def evaluate(prepared, o):
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C08"]}
    obs_path = rel(os.path.join(NATIVE, "observations.json"))
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [obs_path] + [e for e in evidence if e]})

    add("C08-N01", o["admin_available"] and o["admin_no_tenant"] and o["admin_not_configured"] and o["scan_only"] and o["mgmt_requests_scan_only"] == 0,
        f"Administration available without 'Not available': {o['admin_available']}; no-tenant guidance: {o['admin_no_tenant']}; scan-only tenant shows the "
        f"no-management-client guidance with Open Settings: {o['admin_not_configured']}; scan-only scan allowed with the scan key and no Authorization: "
        f"{o['scan_only']}; management endpoint requests so far: {o['mgmt_requests_scan_only']}",
        [o["tree_no_tenant"], o["tree_not_configured"]])
    before, after = o["scan_items_before"], o["scan_items_after_client"]
    add("C08-N02", o["no_client_status"] and o["client_saved"] and o["secret_field_cleared"] and len(before) == 1 and len(after) == 2
        and set(before) <= set(after) and o["scan_key_still_saved"],
        f"status before: no management client (scans do not need one): {o['no_client_status']}; client typed and saved with Return, status and Client ID shown: "
        f"{o['client_saved']}; secret field empty after saving: {o['secret_field_cleared']}; fixture credential items {len(before)} → {len(after)} with the scan "
        f"key's item kept: {set(before) <= set(after)}; scan key still saved: {o['scan_key_still_saved']}",
        [o["tree_client_saved"]])
    vr = o["verify_requests"]
    grants = [r for r in vr if r["path"] == "/oauth2/access_token"]
    reads = [r for r in vr if r["path"] != "/oauth2/access_token"]
    grant_ok = len(grants) == 1 and grants[0]["method"] == "POST" and grants[0]["auth"] == "basic-ok" and grants[0]["grantType"] == "client_credentials" \
        and TSG in str(grants[0].get("scope")) and not grants[0]["secretInBody"]
    read_ok = len(reads) == 1 and reads[0]["path"] == f"/aisec/v1/mgmt/profiles/tsg/{TSG}" and reads[0]["auth"] == "bearer-issued" \
        and reads[0]["query"].get("limit") == "1" and not reads[0]["scanToken"]
    add("C08-N03", o["verified"] and grant_ok and read_ok,
        f"'can read this tenant' shown: {o['verified']}; token requests {[(g['method'], g['auth'], g['grantType'], g['scope'], g['secretInBody']) for g in grants]}; "
        f"reads {[(r['path'], r['query'], r['auth'], r['scanToken']) for r in reads]}",
        [o["tree_verified"]])
    pr = o["page_requests"]
    offsets = [(r["query"].get("offset"), r["query"].get("limit"), r["auth"]) for r in pr]
    add("C08-N04", o["page1"] and o["page1_partial"] and o["page2"] and o["page2_next_disabled"] and o["page2_partial_gone"]
        and offsets == [("0", "25", "bearer-issued"), ("25", "25", "bearer-issued")],
        f"page 1 rows 00–24 with 'Items 1–25 of 25+': {o['page1']}; partial note: {o['page1_partial']}; page 2 rows to 29 with 'Items 26–30 of 30': {o['page2']}; "
        f"Next page disabled: {o['page2_next_disabled']}; partial note gone: {o['page2_partial_gone']}; profile reads (offset, limit, auth): {offsets}",
        [o["tree_page1"], o["tree_page2"]])
    add("C08-N05", o["profile_detail"] and o["api_key_detail"] and not o["redacted_in_tree"] and o["deployment_profiles"] and o["data_patterns"],
        f"profile detail with fields and the local-preview classification: {o['profile_detail']}; API key detail with the unknown status 'FUTURE_STATUS' and last8: "
        f"{o['api_key_detail']}; secret field values in the tree: {o['redacted_in_tree']}; deployment profiles 1–3 of 3: {o['deployment_profiles']}; "
        f"DLP data patterns 'Items 1–1 of 2': {o['data_patterns']}",
        [o["tree_profile_detail"], o["tree_api_key_detail"], o["tree_data_patterns"]])
    add("C08-N06", o["topics_denied"] and o["topics_empty"],
        f"403 shown as not allowed, without server text and not as empty: {o['topics_denied']}; the service's empty list shown as reported empty: {o['topics_empty']}",
        [o["tree_topics_denied"], o["tree_topics_empty"]])
    add("C08-N07", o["refresh_denied"],
        f"a denied refresh shows the denial and keeps the earlier rows marked with their read time: {o['refresh_denied']}",
        [o["tree_refresh_denied"]])
    add("C08-N08", o["replaced"] and o["restarted_first_page"] and o["no_alert_after_late_reply"] and o["grants_after_replace"] == 1,
        f"client replaced while page 2 was in flight: {o['replaced']}; Administration restarted at 'Items 1–25 of 25+' and still there after the late reply: "
        f"{o['restarted_first_page']}; no alert from the retired read: {o['no_alert_after_late_reply']}; new token grants: {o['grants_after_replace']}",
        [o["tree_replaced"]])
    refused = o["grant_refused_requests"]
    add("C08-N09", o["grant_refused"] and o["scan_after_refusal"] and refused and all(r["path"] == "/oauth2/access_token" for r in refused),
        f"refused grant shown as not accepted without server text: {o['grant_refused']}; management requests {[(r['path'], r['status']) for r in refused]}; "
        f"scan afterwards allowed with the scan key: {o['scan_after_refusal']}",
        [o["tree_grant_refused"]])
    add("C08-N10", o["relaunch_reads"] and o["removed"] and o["scan_items_after_remove"] == before and o["admin_after_remove"] and o["scan_after_remove"],
        f"after relaunch the Keychain client reads page 1: {o['relaunch_reads']}; removal shows no management client and keeps the scan key: {o['removed']}; "
        f"fixture credential items after removal equal the scan key's alone: {o['scan_items_after_remove'] == before}; Administration not configured: "
        f"{o['admin_after_remove']}; scan still allowed: {o['scan_after_remove']}",
        [o["tree_relaunch"]])
    add("C08-N11", o["zoom"]["clear"] and o["min_size"]["clear"],
        f"200% zoom: {o['zoom']['found']} in window {o['zoom']['window']}, clear: {o['zoom']['clear']}; minimum size: {o['min_size']['found']} in window "
        f"{o['min_size']['window']}, clear: {o['min_size']['clear']}",
        [])
    no_scan_token = all(not r["scanToken"] for r in o["mgmt_requests"])
    no_bearer_on_scan = all(not r.get("authorization") for r in o["scan_requests"])
    add("C08-N12", not o["persisted_secrets"]["hits"] and not o["secret_in_trees"] and not o["redacted_in_trees"] and no_scan_token and no_bearer_on_scan
        and o["v1_unchanged"],
        f"data root files {o['persisted_secrets']['files']}; files with a secret or redacted value: {o['persisted_secrets']['hits']}; AX snapshots with a secret: "
        f"{o['secret_in_trees']}; with a redacted value: {o['redacted_in_trees']}; management requests carrying the scan key: {not no_scan_token}; scan requests with "
        f"Authorization: {not no_bearer_on_scan}; v1 data unchanged: {o['v1_unchanged']}",
        [])

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1",
        "subject": "C08",
        "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (tools/native-acceptance/ax.swift, System Events keystrokes) against the synthetic TLS endpoints tools/native-acceptance/scan_server.py and tools/native-acceptance/mgmt_server.py; tools/native-acceptance/acceptance_c08.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "The endpoints are local synthetic fixtures with a disposable test CA and a management origin trusted only by the fixture build; live management behavior is qualified in C25.",
            "Keychain items are inspected by attributes only (security dump-keychain without -d); stored secrets are never read back.",
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
