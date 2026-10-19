#!/usr/bin/env python3
"""C12 native acceptance: the case Library and Runtime Overview & Inbox
through the real fixture window against the synthetic management endpoint
(mgmt_server.py: sessions chart, API keys and a public example catalog
snapshot at the fixture catalog URL).

Run from the repository root after `xtask native-prepare --chunk C12`.
Requires the Accessibility grant. Synthetic data only; the fixture data
root is removed at the end; v1 data and the login keychain are untouched.
"""

import json
import os
import shutil
import stat
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fixture_app as fx  # noqa: E402
import acceptance_c08 as c8  # noqa: E402
import native_sheets as ns  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C12")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C12" else "native-C12")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C12" else "native-acceptance-C12.json")
EXPORTS = os.path.join(fx.DATA, "fixture-exports")
STAGE = os.path.join(fx.DATA, "fixture-case-files")
LIBRARY = os.path.join(fx.DATA, "library")
CATALOG_FILE = os.path.join(fx.DATA, "fixture-case-catalog")
CATALOG_PATH = "/cdot65/prisma-airs-test-cases/main/catalog.json"
PROMPT = "SYNTHETIC-C12-CASE-PROMPT"
LOG = []


def rel(path):
    return os.path.relpath(path, ROOT)


def save_tree(name, tree):
    path = os.path.join(NATIVE, f"tree-{name}.json")
    with open(path, "w") as f:
        json.dump(tree, f, indent=0)
    return rel(path)


def press(app, label, roles=("AXRadioButton", "AXButton", "AXCheckBox")):
    tree = app.tree()
    for role in roles:
        matches = [n for n in tree if n.get("role") == role and (n.get("label") or "").startswith(label)]
        for nth, n in enumerate(matches):
            if n.get("label") == label:
                app.ax("press", role, label, nth)
                return role
    raise RuntimeError(f"no element labelled {label!r}")


def sealed():
    return sorted(os.path.join(LIBRARY, n) for n in os.listdir(LIBRARY) if n.endswith(".sealed")) if os.path.isdir(LIBRARY) else []


def stage(name, data):
    if os.path.isdir(STAGE):
        shutil.rmtree(STAGE)
    os.makedirs(STAGE)
    with open(os.path.join(STAGE, name), "wb") as f:
        f.write(data)


def rows(tree):
    return sorted({n["label"] for n in tree if n.get("role") == "AXButton"
                   and ((n.get("label") or "").startswith("Load ") or (n.get("label") or "").startswith("Inspect "))})


def open_library(app):
    app.ax("press", "AXButton", "Case Library")
    return app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Test case library") is not None)


def value_of(tree, role, label):
    node = fx.App.node(tree, role, label)
    return node.get("value") if node else None


def main():
    try:
        return run()
    finally:
        fx.sh(["pkill", "-f", fx.PROCESS])
        fx.clean_fixture_root()


def run():
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")
    if c8.sha256(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c12-acceptance-")
    if os.path.isdir(NATIVE):
        shutil.rmtree(NATIVE)
    os.makedirs(NATIVE)
    app = fx.App(bundle, work, LOG)
    app.quit()
    fx.clean_fixture_root()
    v1_before = fx.v1_metadata()
    endpoint = fx.ScanEndpoint(os.path.join(work, "endpoint"), c8.SCAN_KEY)
    endpoint.install_roots()
    endpoint.start()
    mgmt = c8.MgmtEndpoint(os.path.join(work, "endpoint"), os.path.join(work, "mgmt"))
    mgmt.install()
    mgmt.start()
    mgmt.mode(keyExpiresInDays=5.5)
    with open(CATALOG_FILE, "w") as f:
        f.write(mgmt.origin + CATALOG_PATH + "\n")
    obs = {}

    app.launch()
    app.open_settings()
    app.type_into("AXTextField", "TSG ID (optional)", c8.TSG)
    app.create_custom_tenant("Tenant A", endpoint.origin)
    app.save_key(c8.SCAN_KEY)
    c8.save_client(app)

    # ---- N05: starters in an empty library ----
    tree, _ = open_library(app)
    tree, starters = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Try Sensitive data in a reply in Playground") is not None, 10)
    obs["starters"] = starters and all(fx.App.node(tree, "AXButton", f"Try {n} in Playground") is not None
                                       for n in ("Benign support question", "Prompt injection attempt", "Sensitive data in a reply"))
    scans_before = len(endpoint.requests())
    app.ax("press", "AXButton", "Try Sensitive data in a reply in Playground")
    tree, loaded = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Scan a prompt or response") is not None, 10)
    response = value_of(tree, "AXTextArea", "Response") or ""
    obs["starter_loaded"] = loaded and "123-45-6789" in response and len(endpoint.requests()) == scans_before
    obs["tree_starter"] = save_tree("starter-loaded", tree)
    app.type_into("AXTextField", "Profile name or ID", c8.PROFILE)
    app.ax("press", "AXButton", "Save this case")
    tree, saved_starter = c8.text(app, "Saved to this tenant’s case Library.", 10)
    obs["starter_saved"] = saved_starter

    # ---- N01: authoring a Runtime case and a specification ----
    tree, _ = open_library(app)
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Load Sensitive data in a reply") is not None, 10)
    app.ax("press", "AXButton", "Create a case")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Save case") is not None)
    app.type_into("AXTextField", "Name", "Benign question")
    app.type_into("AXTextField", "Security profile", c8.PROFILE)
    app.type_into("AXTextField", "Application", "Desktop examples")
    app.type_into("AXTextArea", "Input or test specification", PROMPT)
    app.ax("press", "AXButton", "Save case")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Load Benign question") is not None, 10)
    press(app, "Model Security")
    app.ax("press", "AXButton", "Create a case")
    tree, spec_form = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Save case") is not None
                                   and fx.App.node(t, "AXTextField", "Security profile") is None, 8)
    app.type_into("AXTextField", "Name", "Provenance checklist")
    app.type_into("AXTextArea", "Input or test specification", "Check the model artifact provenance")
    app.ax("press", "AXButton", "Save case")
    tree, spec_saved = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Inspect Provenance checklist") is not None, 10)
    press(app, "All products")
    time.sleep(0.8)
    files = sealed()
    blob = open(files[0], "rb").read() if files else b""
    obs["authored"] = {"spec_form_without_profile": spec_form, "spec_saved": spec_saved,
                       "rows": rows(app.tree()), "sealed": len(files),
                       "plaintext": PROMPT.encode() in blob,
                       "mode": oct(stat.S_IMODE(os.stat(files[0]).st_mode)) if files else None}
    app.quit()
    app.launch()
    tree, _ = open_library(app)
    tree, again = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Load Benign question") is not None
                               and fx.App.node(t, "AXButton", "Inspect Provenance checklist") is not None, 12)
    obs["after_relaunch"] = again
    obs["tree_library"] = save_tree("library", tree)

    # ---- N02: Load and Inspect ----
    obs["no_load_for_spec"] = fx.App.node(tree, "AXButton", "Load Provenance checklist") is None
    app.ax("press", "AXButton", "Inspect Provenance checklist")
    tree, inspected = c8.text(app, "Execution belongs to that product workflow.", 8)
    obs["inspected"] = inspected
    scans_before = len(endpoint.requests())
    app.ax("press", "AXButton", "Load Benign question")
    tree, loaded = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Scan a prompt or response") is not None, 10)
    obs["case_loaded"] = {
        "prompt": value_of(tree, "AXTextArea", "Prompt") == PROMPT,
        "profile": value_of(tree, "AXTextField", "Profile name or ID") == c8.PROFILE,
        "app": value_of(tree, "AXTextField", "Application name") == "Desktop examples",
        "nothing_scanned": app.has_text(tree, "nothing was scanned") and len(endpoint.requests()) == scans_before,
    }
    obs["tree_loaded"] = save_tree("case-loaded", tree)

    # ---- N03: catalogs ----
    open_library(app)
    app.ax("press", "AXButton", "Import bundled")
    tree, bundled = c8.text(app, "Added 11 examples. Examples are never run.", 10)
    app.ax("press", "AXButton", "Import bundled")
    tree, already = c8.text(app, "These examples are already in this tenant's library.", 10)
    app.ax("press", "AXButton", "Import public examples")
    tree, public = c8.text(app, "Added 1 example. Examples are never run.", 15)
    mgmt.mode(keyExpiresInDays=5.5, catalog="bad")
    app.ax("press", "AXButton", "Import public examples")
    tree, malformed = c8.text(app, "Choose a version 1 example catalog", 15)
    mgmt.mode(keyExpiresInDays=5.5)
    catalog_reads = [r for r in mgmt.requests() if r["path"] == CATALOG_PATH]
    obs["catalogs"] = {"bundled": bundled, "already": already, "public": public, "malformed": malformed,
                       "catalog_reads": len(catalog_reads),
                       "credentialless": all(r.get("auth") == "none" for r in catalog_reads)}
    obs["tree_catalogs"] = save_tree("catalogs", tree)

    # ---- N04: export, remove, import, foreign file ----
    app.ax("press", "AXButton", "Export library…")
    tree, exported = c8.text(app, "Saved prisma-airs-test-cases.json.", 10)
    path = os.path.join(EXPORTS, "prisma-airs-test-cases.json")
    data = open(path, "rb").read() if os.path.exists(path) else b""
    document = json.loads(data or b"{}")
    obs["export"] = {"saved": exported, "version": document.get("version"), "cases": len(document.get("cases", [])),
                     "mode": oct(stat.S_IMODE(os.stat(path).st_mode)) if data else None}
    app.ax("press", "AXButton", "Remove case Benign question")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Load Benign question") is None, 8)
    stage("cases.json", data)
    app.ax("press", "AXButton", "Import cases…")
    tree, restored = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Load Benign question") is not None
                                  and app.has_text(t, "Imported 1 case."), 10)
    foreign = dict(document, tenant="tn-ffffffffffffffff")
    stage("foreign.json", json.dumps(foreign).encode())
    app.ax("press", "AXButton", "Import cases…")
    tree, refused = c8.text(app, "Import must belong to this tenant", 10)
    obs["import"] = {"restored": restored, "foreign_refused": refused}
    obs["tree_import"] = save_tree("import", tree)

    # ---- N10: the production panels ----
    # No AX tree is saved while a panel lists folders.
    out = os.path.join(work, "panel-library")
    os.makedirs(out)
    big = os.path.join(out, "too-big.json")
    with open(big, "w") as f:
        f.write(" " * (300 * 1024))
    ns.enable()
    app.ax("press", "AXButton", "Export library…")
    panel = ns.wait_sheet(app)
    saved_panel = ns.save_as(app, out, name="library-panel")
    tree, said = c8.text(app, "Saved library-panel.json.", 10)
    exported_file = os.path.join(out, "library-panel.json")
    mode = oct(stat.S_IMODE(os.stat(exported_file).st_mode)) if os.path.exists(exported_file) else None
    app.ax("press", "AXButton", "Remove case Benign question")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Load Benign question") is None, 8)
    app.ax("press", "AXButton", "Import cases…")
    opened = ns.open_file(app, exported_file)
    tree, restored_panel = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Load Benign question") is not None, 10)
    focus = fx.App.focused(tree)
    count = len(rows(tree))
    app.ax("press", "AXButton", "Import cases…")
    cancelled = ns.cancel(app)
    time.sleep(1)
    unchanged = len(rows(app.tree())) == count
    app.ax("press", "AXButton", "Import cases…")
    ns.open_file(app, big)
    tree, refused = c8.text(app, "That file could not be read. Nothing was changed.", 10)
    obs["panel_library"] = {"panel_shown": panel, "saved": saved_panel and said, "mode": mode,
                            "imported": opened and restored_panel, "cancelled": cancelled and unchanged,
                            "too_large_refused": refused,
                            "focus_in_window": bool(focus) and focus.get("role") not in ("AXSheet", None)}
    obs["tree_panel_library"] = save_tree("panel-library", tree)
    ns.disable()

    # ---- N06: Overview & Inbox ----
    app.open_runtime()
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Scan a prompt or response") is not None)
    press(app, "Overview & Inbox")
    tree, summary = c8.text(app, "20 sessions · 3 violating · 17 benign · 3 detection events", 15)
    key_item = "Runtime expires in 5 days"
    obs["overview"] = {"summary": summary, "key_item": app.has_text(tree, key_item),
                       "attention": app.has_text(tree, "0 critical · 2 warn")}
    obs["tree_overview"] = save_tree("overview", tree)
    app.ax("press", "AXButton", "Acknowledge")
    tree, held = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Restore ") is not None, 8)
    restore = fx.App.node(tree, "AXButton", "Restore ")
    obs["held"] = held
    if restore:
        app.ax("press", "AXButton", restore["label"])
    tree, reopened = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Restore ") is None
                                  and app.has_text(t, "0 critical · 2 warn"), 8)
    obs["restored"] = reopened
    app.ax("press", "AXButton", "Inspect sessions")
    tree, sessions = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Investigate sessions") is not None, 10)
    obs["to_sessions"] = sessions

    # ---- N07: a failed chart, then an unreachable service ----
    app.open_runtime()
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Scan a prompt or response") is not None)
    press(app, "Overview & Inbox")
    mgmt.mode(keyExpiresInDays=5.5, chartStatus=503)
    app.ax("press", "AXButton", "Refresh Runtime insights")
    tree, unavailable = c8.text(app, "Runtime activity unavailable:", 15)
    obs["chart_failed"] = unavailable and not app.has_text(tree, "0 sessions") and not app.has_text(tree, "SERVER-TEXT")
    obs["tree_chart_failed"] = save_tree("chart-failed", tree)
    mgmt.mode(keyExpiresInDays=5.5)
    mgmt.stop()
    app.ax("press", "AXButton", "Refresh Runtime insights")
    tree, failed = c8.text(app, "The last refresh failed", 20)
    obs["refresh_failed"] = failed and app.has_text(tree, "Showing what was read at")
    obs["tree_refresh_failed"] = save_tree("refresh-failed", tree)
    before = len(mgmt.requests())
    mgmt.start()
    time.sleep(6)
    obs["offline_auto"] = len(mgmt.requests()) - before

    # ---- N09: zoom and minimum size ----
    press(app, "Playground")
    open_library(app)
    for _ in range(10):
        app.keys("=", "command down")
    obs["zoom_create"] = c8.reach(app, 2, "Create a case")
    app.keys("0", "command down")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{900, 600}}'])
    time.sleep(0.8)
    obs["min_create"] = c8.reach(app, 1, "Create a case")
    app.open_runtime()
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Scan a prompt or response") is not None)
    press(app, "Overview & Inbox")
    time.sleep(0.8)
    obs["min_refresh"] = c8.reach(app, 1, "Refresh Runtime insights")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{1280, 820}}'])
    for _ in range(10):
        app.keys("=", "command down")
    obs["zoom_refresh"] = c8.reach(app, 2, "Refresh Runtime insights")
    app.keys("0", "command down")

    # ---- N08: tenant deletion; nothing but reads ----
    app.open_settings()
    app.ax("press", "AXButton", "Delete Tenant A")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Confirm deleting Tenant A") is not None, 8)
    app.ax("press", "AXButton", "Confirm deleting Tenant A")
    deadline = time.time() + 10
    while sealed() and time.time() < deadline:
        time.sleep(0.3)
    obs["library_removed"] = not sealed()
    app.quit()
    requests = mgmt.requests()
    obs["writes"] = [{k: r.get(k) for k in ("method", "path")} for r in requests
                     if r["method"] != "GET" and r["path"] != "/oauth2/access_token"]
    trees = [os.path.join(NATIVE, n) for n in os.listdir(NATIVE) if n.startswith("tree-")]
    obs["secret_in_trees"] = [rel(p) for p in trees if any(s in open(p).read() for s in (c8.SECRET, c8.SCAN_KEY, "SYNTHETIC-C08-KEY-VALUE"))]
    endpoint.stop()
    mgmt.stop()
    obs["v1_unchanged"] = fx.v1_metadata() == v1_before
    shutil.rmtree(work, ignore_errors=True)
    with open(os.path.join(NATIVE, "observations.json"), "w") as f:
        json.dump({"observations": obs, "log": LOG}, f, indent=1)
    return evaluate(prepared, obs)


def evaluate(prepared, o):
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C12"]}
    obs_path = rel(os.path.join(NATIVE, "observations.json"))
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [obs_path] + [e for e in evidence if e]})

    a = o["authored"]
    add("C12-N01", a["spec_form_without_profile"] and a["spec_saved"] and "Load Benign question" in a["rows"]
        and "Inspect Provenance checklist" in a["rows"] and a["sealed"] == 1 and not a["plaintext"] and a["mode"] == "0o600" and o["after_relaunch"],
        f"authored {a}; listed after relaunch: {o['after_relaunch']}", [o["tree_library"]])
    loaded = o["case_loaded"]
    add("C12-N02", all(loaded.values()) and o["no_load_for_spec"] and o["inspected"],
        f"Runtime case loaded {loaded}; specification without Load: {o['no_load_for_spec']}; inspected: {o['inspected']}", [o["tree_loaded"]])
    cat = o["catalogs"]
    add("C12-N03", cat["bundled"] and cat["already"] and cat["public"] and cat["malformed"] and cat["catalog_reads"] == 2 and cat["credentialless"],
        f"catalogs {cat}", [o["tree_catalogs"]])
    exp, imp = o["export"], o["import"]
    add("C12-N04", exp["saved"] and exp["version"] == 2 and exp["mode"] == "0o600" and exp["cases"] >= 2 and imp["restored"] and imp["foreign_refused"],
        f"export {exp}; import {imp}", [o["tree_import"]])
    add("C12-N05", o["starters"] and o["starter_loaded"] and o["starter_saved"],
        f"three starters offered: {o['starters']}; loaded without scanning: {o['starter_loaded']}; saved as a case: {o['starter_saved']}", [o["tree_starter"]])
    ov = o["overview"]
    add("C12-N06", all(ov.values()) and o["held"] and o["restored"] and o["to_sessions"],
        f"overview {ov}; acknowledged: {o['held']}; restored: {o['restored']}; opened Sessions: {o['to_sessions']}", [o["tree_overview"]])
    add("C12-N07", o["chart_failed"] and o["refresh_failed"] and o["offline_auto"] == 0,
        f"chart failure stated without zeros: {o['chart_failed']}; refresh failure with the last read kept: {o['refresh_failed']}; requests within 6 s: {o['offline_auto']}",
        [o["tree_chart_failed"], o["tree_refresh_failed"]])
    add("C12-N08", o["library_removed"] and not o["writes"] and not o["secret_in_trees"],
        f"sealed library removed with the tenant: {o['library_removed']}; non-read requests: {o['writes']}; secrets in AX snapshots: {o['secret_in_trees']}", [])
    pl = o["panel_library"]
    add("C12-N10", pl["panel_shown"] and pl["saved"] and pl["mode"] == "0o600" and pl["imported"] and pl["cancelled"]
        and pl["too_large_refused"] and pl["focus_in_window"],
        f"production panels: {pl}", [o["tree_panel_library"]])
    zoom_ok = all(r["clear"] for r in (o["zoom_create"], o["min_create"], o["zoom_refresh"], o["min_refresh"]))
    add("C12-N09", zoom_ok,
        f"200% zoom: {o['zoom_create']['found']} / {o['zoom_refresh']['found']}; minimum size: {o['min_create']['found']} / {o['min_refresh']['found']}; all clear: {zoom_ok}", [])

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1",
        "subject": "C12",
        "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (tools/native-acceptance/ax.swift, System Events keystrokes) against tools/native-acceptance/mgmt_server.py; tools/native-acceptance/acceptance_c12.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "N04 answers the case-file sheet with a staged file and saves exports into the fixture data root without a sheet; N10 drives the production rfd NSOpenPanel/NSSavePanel (the fixture switch fixture-native-sheets) with driver-owned temporary files. No Accessibility tree is saved while a panel lists folders.",
            "The public example catalog is a local snapshot served at the fixture catalog URL; the production URL is raw.githubusercontent.com.",
            "Spoken VoiceOver output was not recorded; AX roles, names and focus are inspected.",
        ],
    }
    with open(RECORD, "w") as f:
        json.dump(record, f, indent=2)
        f.write("\n")
    for c in checks:
        print(c["result"].upper(), c["id"], c["observation"][:220])
    return 0 if all(c["result"] == "pass" for c in checks) else 1


if __name__ == "__main__":
    sys.exit(main())
