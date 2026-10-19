#!/usr/bin/env python3
"""C10 native acceptance: DLP authoring through the real fixture window, the
native confirmation sheet and the fixture build's keyword-file sheet,
against the synthetic management endpoint (mgmt_server.py: stateful DLP
collections, multipart dictionary uploads logged by part with SHA-256s).

Run from the repository root after `xtask native-prepare --chunk C10`.
Requires the Accessibility grant. Synthetic data only; the fixture data
root is removed at the end; v1 data and the login keychain are untouched.
"""

import hashlib
import json
import os
import shutil
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fixture_app as fx  # noqa: E402
import acceptance_c08 as c8  # noqa: E402
import acceptance_c09 as c9  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C10")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C10" else "native-C10")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C10" else "native-acceptance-C10.json")
STAGE = os.path.join(fx.DATA, "fixture-dictionary-files")
LOG = []
TYPED = "alphabeta"
FILE_TEXT = "gamma\ndelta\nepsilon\n"


def rel(path):
    return os.path.relpath(path, ROOT)


def save_tree(name, tree):
    path = os.path.join(NATIVE, f"tree-{name}.json")
    with open(path, "w") as f:
        json.dump(tree, f, indent=0)
    return rel(path)


def sha(data):
    return hashlib.sha256(data if isinstance(data, bytes) else data.encode()).hexdigest()


def stage(name, data):
    if os.path.isdir(STAGE):
        shutil.rmtree(STAGE)
    if name:
        os.makedirs(STAGE)
        path = os.path.join(STAGE, name)
        with open(path, "wb") as f:
            f.write(data)
        return path
    os.makedirs(STAGE)
    return None


def dlp_writes(mgmt, method=None, prefix="/dlp/v2/api/"):
    return [r for r in c9.writes(mgmt, method, prefix)]


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
    work = tempfile.mkdtemp(prefix="airs-v2-c10-acceptance-")
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
    obs = {}

    app.launch()
    app.open_settings()
    app.type_into("AXTextField", "TSG ID (optional)", c8.TSG)
    app.create_custom_tenant("Tenant A", endpoint.origin)
    app.save_key(c8.SCAN_KEY)
    c8.save_client(app)
    c8.open_admin(app)
    c8.text(app, "Items 1–25 of 25+")

    # ---- N01: edit a data pattern ----
    c9.open_change(app, "Data patterns", "Details for Card numbers", "Edit…")
    app.type_into("AXTextArea", "data pattern draft (JSON)",
                  '{"name":"Card numbers v2","type":"custom","detection_config":{"technique":"regex"}}')
    c9.review(app, "Applies only to this tenant")
    app.type_into("AXTextField", "Type Card numbers to confirm", "Card numbers")
    app.ax("press", "AXButton", "Apply…")
    c9.wait_sheet(app)
    app.keycode(36)
    tree, applied = c8.text(app, "Applied. The item was read back.", 15)
    puts = dlp_writes(mgmt, "PUT")
    obs["pattern_applied"] = applied
    obs["pattern_puts"] = [{k: p.get(k) for k in ("path", "dlpBody")} for p in puts]
    obs["tree_pattern"] = save_tree("pattern-applied", tree)

    # ---- N05: unavailable operations ----
    c9.close_workbench(app)
    app.ax("press", "AXButton", "Data profiles")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Details for PCI") is not None)
    app.ax("press", "AXButton", "Details for PCI")
    tree, _ = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Edit…") is not None)
    obs["profile_no_delete"] = fx.App.node(tree, "AXButton", "Delete…") is None and fx.App.node(tree, "AXButton", "New data profile") is not None
    app.ax("press", "AXButton", "Data filtering profiles")
    tree, _ = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Details for Default") is not None)
    obs["filters_no_new"] = not any((n.get("label") or "").startswith("New ") for n in tree if n.get("role") == "AXButton")
    app.ax("press", "AXButton", "Deployment profiles")
    tree, read_only = c8.text(app, "Read only. Deployment profiles")
    obs["deployments_read_only"] = read_only
    obs["tree_unavailable"] = save_tree("unavailable", tree)

    # ---- N02: dictionary from typed keywords ----
    app.ax("press", "AXButton", "Dictionaries")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "New dictionary") is not None)
    app.ax("press", "AXButton", "New dictionary")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Review change") is not None)
    app.type_into("AXTextArea", "Dictionary keywords, one per line", TYPED)
    c9.review(app, "Applies only to this tenant")
    app.type_into("AXTextField", "Type CREATE to confirm", "CREATE")
    app.ax("press", "AXButton", "Apply…")
    c9.wait_sheet(app)
    app.keycode(36)
    tree, typed_ok = c8.text(app, "Applied. The item was read back.", 15)
    posts = dlp_writes(mgmt, "POST", "/dlp/v2/api/dictionaries")
    obs["typed_applied"] = typed_ok
    obs["typed_upload"] = [{k: p.get(k) for k in ("contentType", "boundaryPrefix", "parts")} for p in posts]
    obs["typed_sha"] = sha(TYPED)
    obs["tree_typed"] = save_tree("dictionary-typed", tree)

    # ---- N04: a non-text file; a cancelled sheet ----
    c9.close_workbench(app)
    app.ax("press", "AXButton", "New dictionary")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Choose keyword file…") is not None)
    stage("binary.txt", bytes([0xff, 0xfe, 0x00, 0x01]))
    before = len(mgmt.requests())
    app.ax("press", "AXButton", "Choose keyword file…")
    tree, not_text = c8.text(app, "That file is not UTF-8 text.")
    obs["not_text"] = not_text and len(mgmt.requests()) == before
    stage(None, b"")
    app.ax("press", "AXButton", "Choose keyword file…")
    time.sleep(1.5)
    tree = app.tree()
    obs["cancelled_nothing"] = app.has_text(tree, "A file is read once by the desktop") and len(mgmt.requests()) == before
    obs["tree_not_text"] = save_tree("not-text", tree)

    # ---- N03: native file selection ----
    path = stage("keywords-file.txt", FILE_TEXT.encode())
    stat_before = (os.stat(path).st_mtime_ns, sha(open(path, "rb").read()))
    app.ax("press", "AXButton", "Choose keyword file…")
    tree, chosen = c8.text(app, "Chosen: keywords-file.txt")
    obs["file_chosen"] = chosen and app.has_text(tree, "3 lines")
    app.ax("press", "AXRadioButton", "Use the chosen file")
    time.sleep(0.5)
    c9.review(app, "Applies only to this tenant")
    app.type_into("AXTextField", "Type CREATE to confirm", "CREATE")
    app.ax("press", "AXButton", "Apply…")
    c9.wait_sheet(app)
    app.keycode(36)
    before_posts = len(posts)
    tree, file_ok = c8.text(app, "Applied. The item was read back.", 15)
    app.wait_for(lambda t: len(dlp_writes(mgmt, "POST", "/dlp/v2/api/dictionaries")) > before_posts, 10)
    file_posts = dlp_writes(mgmt, "POST", "/dlp/v2/api/dictionaries")[before_posts:]
    obs["file_applied"] = file_ok
    obs["file_upload"] = [{k: p.get(k) for k in ("contentType", "parts")} for p in file_posts]
    obs["file_sha"] = sha(FILE_TEXT)
    obs["source_unchanged"] = (os.stat(path).st_mtime_ns, sha(open(path, "rb").read())) == stat_before
    obs["tree_file"] = save_tree("dictionary-file", tree)

    # ---- N09: zoom and minimum size in the dictionary workbench ----
    for _ in range(10):
        app.keys("=", "command down")
    obs["zoom_choose"] = c8.reach(app, 2, "Choose keyword file…")
    obs["zoom_review"] = c8.reach(app, 2, "Review change")
    app.keys("0", "command down")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{900, 600}}'])
    time.sleep(0.8)
    obs["min_choose"] = c8.reach(app, 1, "Choose keyword file…")
    obs["min_review"] = c8.reach(app, 1, "Review change")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{1280, 820}}'])

    # ---- N07: denied upload ----
    c9.close_workbench(app)
    mgmt.mode(dlpDeny="dictionaries")
    app.ax("press", "AXButton", "New dictionary")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Review change") is not None)
    c9.review(app, "Applies only to this tenant")
    app.type_into("AXTextField", "Type CREATE to confirm", "CREATE")
    app.ax("press", "AXButton", "Apply…")
    c9.wait_sheet(app)
    app.keycode(36)
    tree, denied = c8.text(app, "not allowed to read this", 15)
    obs["denied"] = denied and app.has_text(tree, "Nothing was changed.") and not app.has_text(tree, "SERVER-TEXT")
    obs["tree_denied"] = save_tree("denied", tree)
    mgmt.mode()

    # ---- N06: concurrent edit on a pattern ----
    c9.close_workbench(app)
    app.ax("press", "AXButton", "Data patterns")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Details for Card numbers v2") is not None)
    app.ax("press", "AXButton", "Details for Card numbers v2")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Edit…") is not None)
    app.ax("press", "AXButton", "Edit…")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Review change") is not None)
    app.type_into("AXTextArea", "data pattern draft (JSON)",
                  '{"name":"Card numbers v3","type":"custom","detection_config":{"technique":"regex"}}')
    c9.review(app, "Applies only to this tenant")
    app.type_into("AXTextField", "Type Card numbers v2 to confirm", "Card numbers v2")
    mgmt.mode(patternName="Renamed elsewhere")
    app.ax("press", "AXButton", "Apply…")
    c9.wait_sheet(app)
    app.keycode(36)
    tree, conflict = c8.text(app, "The item changed since it was reviewed", 15)
    obs["conflict"] = conflict and len(dlp_writes(mgmt, "PUT")) == 1
    obs["tree_conflict"] = save_tree("conflict", tree)
    mgmt.mode()

    # ---- N08: T23 offline create ----
    c9.close_workbench(app)
    app.ax("press", "AXButton", "New data pattern")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Review change") is not None)
    app.type_into("AXTextArea", "data pattern draft (JSON)",
                  '{"name":"Offline pattern","type":"custom","detection_config":{"technique":"regex"}}')
    mgmt.stop()
    c9.review(app, "Applies only to this tenant")
    app.type_into("AXTextField", "Type CREATE to confirm", "CREATE")
    app.ax("press", "AXButton", "Apply…")
    c9.wait_sheet(app)
    app.keycode(36)
    tree, offline = c8.text(app, "The management service could not be reached. The change may or may not have been applied.", 20)
    obs["offline"] = offline
    before = len(mgmt.requests())
    mgmt.start()
    time.sleep(6)
    obs["offline_auto"] = len(mgmt.requests()) - before
    obs["tree_offline"] = save_tree("offline", tree)
    app.quit()

    trees = [os.path.join(NATIVE, n) for n in os.listdir(NATIVE) if n.startswith("tree-")]
    obs["path_in_trees"] = [rel(p) for p in trees if STAGE in open(p).read() or fx.HOME in open(p).read()]
    endpoint.stop()
    mgmt.stop()
    obs["v1_unchanged"] = fx.v1_metadata() == v1_before
    obs["dlp_writes"] = [{k: r.get(k) for k in ("method", "path", "status")} for r in dlp_writes(mgmt)]
    shutil.rmtree(work, ignore_errors=True)
    with open(os.path.join(NATIVE, "observations.json"), "w") as f:
        json.dump({"observations": obs, "log": LOG}, f, indent=1)
    return evaluate(prepared, obs)


def upload_ok(uploads, file_sha, file_name):
    if len(uploads) != 1:
        return False
    parts = uploads[0].get("parts") or []
    if len(parts) != 2:
        return False
    meta, file = parts
    return (meta["name"], meta["filename"], meta["contentType"]) == ("json", "metadata.json", "application/json") \
        and (file["name"], file["filename"], file["contentType"]) == ("file", file_name, "text/plain") \
        and file["sha256"] == file_sha


def evaluate(prepared, o):
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C10"]}
    obs_path = rel(os.path.join(NATIVE, "observations.json"))
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [obs_path] + [e for e in evidence if e]})

    puts = o["pattern_puts"]
    add("C10-N01", o["pattern_applied"] and len(puts) == 1 and puts[0]["path"] == "/dlp/v2/api/data-patterns/pattern-1"
        and puts[0]["dlpBody"] == {"name": "Card numbers v2", "type": "custom", "detection_config": {"technique": "regex"}},
        f"applied and read back: {o['pattern_applied']}; writes {puts}", [o["tree_pattern"]])
    typed = o["typed_upload"]
    add("C10-N02", o["typed_applied"] and upload_ok(typed, o["typed_sha"], "keywords.txt")
        and typed[0]["contentType"] == "multipart/form-data" and typed[0]["boundaryPrefix"],
        f"applied: {o['typed_applied']}; upload {typed}; expected file sha {o['typed_sha']}", [o["tree_typed"]])
    add("C10-N03", o["file_chosen"] and o["file_applied"] and upload_ok(o["file_upload"], o["file_sha"], "keywords.txt")
        and o["source_unchanged"] and not o["path_in_trees"],
        f"chosen file shown with its line count: {o['file_chosen']}; applied: {o['file_applied']}; upload {o['file_upload']} (expected sha "
        f"{o['file_sha']}); source unchanged: {o['source_unchanged']}; AX snapshots with a path: {o['path_in_trees']}", [o["tree_file"]])
    add("C10-N04", o["not_text"] and o["cancelled_nothing"],
        f"non-UTF-8 file reported and nothing sent: {o['not_text']}; cancelled sheet staged nothing: {o['cancelled_nothing']}", [o["tree_not_text"]])
    add("C10-N05", o["profile_no_delete"] and o["filters_no_new"] and o["deployments_read_only"],
        f"data profiles Edit but no Delete (and New offered): {o['profile_no_delete']}; filtering profiles without New: {o['filters_no_new']}; "
        f"deployment profiles read only: {o['deployments_read_only']}", [o["tree_unavailable"]])
    add("C10-N06", o["conflict"], f"changed elsewhere → 'changed since it was reviewed' and no second PUT: {o['conflict']}", [o["tree_conflict"]])
    add("C10-N07", o["denied"], f"403 upload reported as not allowed, nothing changed, no server text: {o['denied']}", [o["tree_denied"]])
    add("C10-N08", o["offline"] and o["offline_auto"] == 0,
        f"offline create reported unreachable with an unknown outcome: {o['offline']}; requests within 6 s of return: {o['offline_auto']}",
        [o["tree_offline"]])
    zoom_ok = all(r["clear"] for r in (o["zoom_choose"], o["zoom_review"], o["min_choose"], o["min_review"]))
    add("C10-N09", zoom_ok,
        f"200% zoom: {o['zoom_choose']['found']} / {o['zoom_review']['found']}; minimum size: {o['min_choose']['found']} / {o['min_review']['found']}; "
        f"all clear: {zoom_ok}", [])

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1",
        "subject": "C10",
        "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "method": "cua",
        "performed_by": "Claude Code driving the real fixture window and its AppKit sheet through macOS Accessibility (tools/native-acceptance/ax.swift, System Events keystrokes) against tools/native-acceptance/mgmt_server.py; tools/native-acceptance/acceptance_c10.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "The fixture build answers the keyword-file sheet with the file the driver staged in its data root (the production build opens the rfd/NSOpenPanel sheet); the file is only read.",
            "The DLP endpoint is a local synthetic fixture; live DLP writes are qualified in C25.",
            "Spoken VoiceOver output was not recorded; AX roles, names, the sheet subtree and focus are inspected.",
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
