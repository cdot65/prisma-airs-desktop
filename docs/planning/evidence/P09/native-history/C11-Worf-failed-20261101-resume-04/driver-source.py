#!/usr/bin/env python3
"""C11 native acceptance: Sessions investigations and metadata-only FP/FN
reports through the real fixture window against the synthetic management
endpoint (mgmt_server.py: a generated 24-hour sessions inventory, sessions,
transactions and scan content, content reads logged by scan ID only).

Run from the repository root after `xtask native-prepare --chunk C11`.
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
import mgmt_server  # noqa: E402
import native_sheets as ns  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C11")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C11" else "native-C11")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C11" else "native-acceptance-C11.json")
EXPORTS = os.path.join(fx.DATA, "fixture-exports")
SEALED = os.path.join(fx.DATA, "sessions", "investigations")
PROMPT = mgmt_server.CONTENT_PROMPT
RESPONSE = mgmt_server.CONTENT_RESPONSE
SCAN = "scn-ses-00000-0"
LOG = []
CSV_HEADER = ("tenant,tsg_id,kind,scan_id,scan_sub_req_id,session_id,application,application_id,"
              "verdict,action,detectors,profile,reason_category,reason,reported_at,exported_at")


def rel(path):
    return os.path.relpath(path, ROOT)


def save_tree(name, tree):
    path = os.path.join(NATIVE, f"tree-{name}.json")
    with open(path, "w") as f:
        json.dump(tree, f, indent=0)
    return rel(path)


def press(app, label, roles=("AXRadioButton", "AXButton", "AXCheckBox")):
    """Presses the element labelled exactly `label` (tabs are radio buttons
    in WebKit's tree, toggles check boxes)."""
    tree = app.tree()
    for role in roles:
        matches = [n for n in tree if n.get("role") == role and (n.get("label") or "").startswith(label)]
        for nth, n in enumerate(matches):
            if n.get("label") == label:
                app.ax("press", role, label, nth)
                return role
    raise RuntimeError(f"no element labelled {label!r}")


def reads(mgmt, path):
    return [r for r in mgmt.requests() if r["path"] == path]


def overview(mgmt):
    return reads(mgmt, "/aisec/v1/mgmt/dashboard/v2/sessions/sessionsoverview")


def contents(mgmt):
    return reads(mgmt, "/aisec/v1/mgmt/reports/scancontent")


def sealed_files():
    return sorted(os.path.join(SEALED, n) for n in os.listdir(SEALED)) if os.path.isdir(SEALED) else []


def exported(prefix):
    if not os.path.isdir(EXPORTS):
        return None
    names = sorted(n for n in os.listdir(EXPORTS) if n.startswith(prefix))
    return os.path.join(EXPORTS, names[-1]) if names else None


def open_sessions(app):
    app.ax("press", "AXButton", "Sessions &")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Investigate sessions") is not None)
    return c8.text(app, "Sessions 1–25 of 60", 15)


def open_session(app, session):
    app.ax("press", "AXButton", f"Open session {session}")
    return c8.text(app, "3 scans", 12)


def search(app, query, expect):
    app.type_into("AXTextField", "Search by session ID, scan ID or detector", query)
    app.ax("press", "AXButton", "Search")
    return c8.text(app, expect, 20)


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
    work = tempfile.mkdtemp(prefix="airs-v2-c11-acceptance-")
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
    mgmt.mode(sessionCount=60, detectorEvery=7)
    obs = {}

    app.launch()
    app.open_settings()
    app.type_into("AXTextField", "TSG ID (optional)", c8.TSG)
    app.create_custom_tenant("Tenant A", endpoint.origin)
    app.save_key(c8.SCAN_KEY)
    c8.save_client(app)

    # ---- N01: the 24-hour inventory, paging and a page filter ----
    tree, first = open_sessions(app)
    app.ax("press", "AXButton", "Next page")
    tree, second = c8.text(app, "Sessions 26–50 of 60", 15)
    press(app, "Malicious 4")
    time.sleep(0.6)
    tree = app.tree()
    opened = sorted({n["label"] for n in tree if n.get("role") == "AXButton" and (n.get("label") or "").startswith("Open session ")})
    obs["paging"] = first and second
    obs["malicious_rows"] = opened
    obs["overview_queries"] = [{k: r["query"].get(k) for k in ("time_interval", "time_unit", "limit", "offset")} for r in overview(mgmt)]
    obs["tree_filter"] = save_tree("filter", tree)
    press(app, "All 25")
    app.ax("press", "AXButton", "First page")
    c8.text(app, "Sessions 1–25 of 60", 15)

    # ---- N02: drill-down, explicit reveal, Hide, leaving ----
    tree, listed = open_session(app, "ses-00000")
    obs["listed_scans"] = listed and app.has_text(tree, SCAN)
    obs["no_content_before"] = not contents(mgmt) and not app.has_text(tree, PROMPT)
    obs["tree_session"] = save_tree("session", tree)
    app.ax("press", "AXButton", f"Show content of scan {SCAN}")
    tree, shown = c8.text(app, PROMPT, 12)
    obs["revealed"] = shown and app.has_text(tree, RESPONSE) and len(contents(mgmt)) == 1
    obs["tree_revealed"] = save_tree("revealed", tree)
    app.ax("press", "AXButton", f"Hide content of scan {SCAN}")
    tree, hidden = app.wait_for(lambda t: not app.has_text(t, PROMPT), 8)
    obs["hidden"] = hidden
    app.ax("press", "AXButton", "Administration")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Tenant inventories") is not None)
    tree, _ = open_sessions(app)
    obs["gone_after_leaving"] = not app.has_text(tree, PROMPT)
    obs["tree_returned"] = save_tree("returned", tree)

    # ---- N03: bounded searches and their coverage ----
    tree, detector = search(app, "prompt_injection",
                            "9 sessions reporting prompt_injection among 60 sessions read from the last 24 hours. That is every session of the last 24 hours.")
    obs["detector"] = detector
    obs["tree_detector"] = save_tree("search-detector", tree)
    tree, absent = search(app, "ses-99999", "No session with this ID in the last 30 days (60 sessions read).")
    obs["session_absent"] = absent and any(r["query"].get("time_interval") == "30" and r["query"].get("limit") == "100" for r in overview(mgmt))
    before = len(contents(mgmt))
    tree, scan_found = search(app, "scn-ses-00001-0", "Scan found. Its metadata is shown; content stays hidden.")
    obs["scan_metadata"] = scan_found and app.has_text(tree, "R-C11") and not app.has_text(tree, PROMPT) and not app.has_text(tree, RESPONSE)
    obs["scan_lookup_reads"] = len(contents(mgmt)) - before
    obs["tree_scan"] = save_tree("search-scan", tree)
    app.ax("press", "AXButton", "Clear search")

    # ---- N04: a report, remove/undo and the CSV export ----
    open_session(app, "ses-00000")
    app.ax("press", "AXButton", f"Report scan {SCAN}")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Save report") is not None)
    app.type_into("AXTextArea", "Why is this verdict wrong?", "Order numbers look like card numbers")
    app.ax("press", "AXButton", "Save report")
    app.wait_for(lambda t: fx.App.node(t, "AXRadioButton", "FP/FN reports (1)") is not None
                 or fx.App.node(t, "AXButton", "FP/FN reports (1)") is not None, 10)
    press(app, "FP/FN reports (1)")
    tree, listed = c8.text(app, "Order numbers look like card numbers", 10)
    obs["report_listed"] = listed and app.has_text(tree, SCAN) and app.has_text(tree, "prompt_injection")
    obs["tree_reports"] = save_tree("reports", tree)
    app.ax("press", "AXButton", f"Remove report for {SCAN}")
    tree, removed = c8.text(app, f"Removed the report for {SCAN}.", 10)
    gone = not app.has_text(tree, "Order numbers look like card numbers")
    app.ax("press", "AXButton", "Undo")
    tree, restored = app.wait_for(lambda t: app.has_text(t, "Order numbers look like card numbers")
                                  and fx.App.node(t, "AXButton", "Undo") is None, 10)
    obs["remove_undo"] = removed and gone and restored
    obs["remove_undo_parts"] = {"removed": removed, "gone": gone, "restored": restored}
    app.ax("press", "AXButton", "Export CSV…")
    tree, saved = c8.text(app, "Saved fpfn-reports-tenant-a-", 15)
    path = exported("fpfn-reports-tenant-a-")
    data = open(path, "rb").read() if path else b""
    lines = data.decode("utf-8", "replace").split("\r\n")
    obs["csv"] = {
        "saved": saved, "file": os.path.basename(path) if path else None,
        "mode": oct(stat.S_IMODE(os.stat(path).st_mode)) if path else None,
        "header": lines[0] if lines else None, "rows": len([line for line in lines[1:] if line]),
        "crlf": data.endswith(b"\r\n") and b"\n" not in data.replace(b"\r\n", b""),
        "content": PROMPT.encode() in data or RESPONSE.encode() in data,
    }
    obs["tree_export"] = save_tree("reports-exported", tree)

    # ---- N10: the production NSSavePanel ----
    # No AX tree is saved while a panel lists folders.
    out = os.path.join(work, "panel-exports")
    os.makedirs(out)
    ns.enable()
    app.ax("press", "AXButton", "Export CSV…")
    panel = ns.wait_sheet(app)
    first = ns.save_as(app, out, name="reports-panel")
    tree, said = c8.text(app, "Saved reports-panel.csv.", 10)
    target = os.path.join(out, "reports-panel.csv")
    first_mode = oct(stat.S_IMODE(os.stat(target).st_mode)) if os.path.exists(target) else None
    with open(target, "ab") as f:
        f.write(b"stale tail")
    app.ax("press", "AXButton", "Export CSV…")
    replaced = ns.save_as(app, out, name="reports-panel", replace=True)
    tree, said_again = c8.text(app, "Saved reports-panel.csv.", 10)
    data = open(target, "rb").read() if os.path.exists(target) else b""
    app.ax("press", "AXButton", "Export CSV…")
    cancelled = ns.cancel(app)
    tree, cancel_said = c8.text(app, "Export cancelled. Nothing was saved.", 10)
    focus = fx.App.focused(tree)
    obs["panel_export"] = {
        "panel_shown": panel, "saved": first and said, "mode": first_mode,
        "replaced": replaced and said_again and not data.endswith(b"stale tail") and data.startswith(CSV_HEADER.encode()),
        "replaced_mode": oct(stat.S_IMODE(os.stat(target).st_mode)) if data else None,
        "cancelled": cancelled and cancel_said and sorted(os.listdir(out)) == ["reports-panel.csv"],
        "focus_in_window": bool(focus) and focus.get("role") not in ("AXSheet", None),
    }
    obs["tree_panel_export"] = save_tree("panel-export-cancelled", tree)
    ns.disable()

    # ---- N06: export metadata, then clear ----
    press(app, "Sessions")
    tree, _ = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Export metadata…") is not None, 10)
    app.ax("press", "AXButton", "Export metadata…")
    tree, meta_saved = c8.text(app, "Saved investigation-tenant-a-", 15)
    meta = exported("investigation-tenant-a-")
    meta_text = open(meta).read() if meta else ""
    obs["metadata_export"] = {"saved": meta_saved, "file": os.path.basename(meta) if meta else None,
                              "sessions": "ses-00000" in meta_text, "content": PROMPT in meta_text or RESPONSE in meta_text,
                              "mode": oct(stat.S_IMODE(os.stat(meta).st_mode)) if meta else None}
    # ---- N05: sealed, restored after relaunch ----
    files = sealed_files()
    blob = open(files[0], "rb").read() if files else b""
    obs["sealed"] = {"files": len(files), "plaintext": any(s.encode() in blob for s in (PROMPT, "ses-00000", SCAN)),
                     "mode": oct(stat.S_IMODE(os.stat(files[0]).st_mode)) if files else None}
    app.quit()
    app.launch()
    open_sessions(app)
    open_session(app, "ses-00000")
    reads_before = len(contents(mgmt))
    app.ax("press", "AXButton", f"Show content of scan {SCAN}")
    tree, again = c8.text(app, PROMPT, 12)
    obs["restored_from_seal"] = again and len(contents(mgmt)) == reads_before
    obs["tree_relaunch"] = save_tree("relaunch-revealed", tree)
    app.ax("press", "AXButton", "Clear cache")
    tree, cleared = c8.text(app, "0 sessions, 0 transactions and 0 revealed scans", 10)
    time.sleep(1)
    obs["cleared"] = cleared and not sealed_files()
    app.ax("press", "AXButton", f"Hide content of scan {SCAN}")
    app.wait_for(lambda t: not app.has_text(t, PROMPT), 8)
    app.ax("press", "AXButton", f"Show content of scan {SCAN}")
    c8.text(app, PROMPT, 12)
    obs["read_after_clear"] = len(contents(mgmt)) == reads_before + 1
    app.wait_for(lambda t: bool(sealed_files()), 8)
    obs["resealed"] = bool(sealed_files())
    obs["tree_cleared"] = save_tree("cleared", tree)

    # ---- N08: the exception patterns preview ----
    count = len(mgmt.requests())
    press(app, "Exception patterns")
    tree, synthetic = c8.text(app, "Synthetic data · Memory only · No live integrations", 8)
    time.sleep(1)
    obs["exceptions"] = synthetic and len(mgmt.requests()) == count
    obs["tree_exceptions"] = save_tree("exceptions", tree)
    press(app, "Sessions")
    c8.text(app, "Sessions 1–25 of 60", 15)

    # ---- N09: zoom and minimum size ----
    for _ in range(10):
        app.keys("=", "command down")
    obs["zoom_refresh"] = c8.reach(app, 2, "Refresh")
    obs["zoom_next"] = c8.reach(app, 2, "Next page")
    app.keys("0", "command down")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{900, 600}}'])
    time.sleep(0.8)
    obs["min_refresh"] = c8.reach(app, 1, "Refresh")
    obs["min_next"] = c8.reach(app, 1, "Next page")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{1280, 820}}'])

    # ---- N11: another tenant sees none of A's content or reports ----
    press(app, "Sessions")
    c8.text(app, "Sessions 1–25 of 60", 15)
    open_session(app, "ses-00000")
    app.ax("press", "AXButton", f"Show content of scan {SCAN}")
    tree, a_shown = c8.text(app, PROMPT, 12)
    app.open_settings()
    app.create_custom_tenant("Tenant B", endpoint.origin)
    tree, b_active = app.make_active("Tenant B")
    app.ax("press", "AXButton", "Sessions &")
    tree, _ = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Investigate sessions") is not None, 10)
    time.sleep(1)
    tree = app.tree()
    b_sessions = {"needs_client": app.has_text(tree, "has no ready management client"),
                  "no_content": not app.has_text(tree, PROMPT) and not app.has_text(tree, RESPONSE),
                  "no_rows": not app.has_text(tree, "ses-00000")}
    obs["tree_tenant_b_sessions"] = save_tree("tenant-b-sessions", tree)
    press(app, "FP/FN reports")
    tree, _ = app.wait_for(lambda t: app.has_text(t, "No reports yet"), 8)
    b_reports = {"empty": app.has_text(tree, "No reports yet"),
                 "no_a_report": not app.has_text(tree, "Order numbers look like card numbers") and not app.has_text(tree, SCAN)}
    obs["tree_tenant_b_reports"] = save_tree("tenant-b-reports", tree)
    app.open_settings()
    app.make_active("Tenant A")
    app.ax("press", "AXButton", "Sessions &")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Investigate sessions") is not None, 10)
    tree, back = app.wait_for(lambda t: fx.App.node(t, "AXRadioButton", "FP/FN reports (1)") is not None
                              or fx.App.node(t, "AXButton", "FP/FN reports (1)") is not None, 10)
    press(app, "FP/FN reports (1)")
    tree, a_report = c8.text(app, "Order numbers look like card numbers", 8)
    obs["tenant_switch"] = {"a_content_shown": a_shown, "b_active": b_active, **b_sessions, **b_reports,
                            "a_report_back": back and a_report, "a_content_not_restored": not app.has_text(tree, PROMPT)}

    # ---- N05 (end): removing the management client removes the seal ----
    app.open_settings()
    app.ax("press", "AXButton", "Remove management client")
    tree, gone = c8.text(app, "No management client. Scans do not need one", 10)
    _, unsealed = app.wait_for(lambda t: not sealed_files(), 10)
    obs["removed_seal"] = gone and unsealed
    app.quit()

    # ---- N07: nothing but reads ----
    requests = mgmt.requests()
    obs["writes"] = [{k: r.get(k) for k in ("method", "path")} for r in requests
                     if r["method"] != "GET" and r["path"] != "/oauth2/access_token"]
    obs["requests"] = len(requests)
    trees = [os.path.join(NATIVE, n) for n in os.listdir(NATIVE) if n.startswith("tree-")]
    obs["secret_in_trees"] = [rel(p) for p in trees if c8.SECRET in open(p).read() or c8.SCAN_KEY in open(p).read()]
    endpoint.stop()
    mgmt.stop()
    obs["v1_unchanged"] = fx.v1_metadata() == v1_before
    shutil.rmtree(work, ignore_errors=True)
    with open(os.path.join(NATIVE, "observations.json"), "w") as f:
        json.dump({"observations": obs, "log": LOG}, f, indent=1)
    return evaluate(prepared, obs)


def evaluate(prepared, o):
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C11"]}
    obs_path = rel(os.path.join(NATIVE, "observations.json"))
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [obs_path] + [e for e in evidence if e]})

    queries = o["overview_queries"]
    inventory_reads = [q for q in queries if q["limit"] == "25"]
    add("C11-N01", o["paging"] and o["malicious_rows"] == ["Open session ses-00028", "Open session ses-00035", "Open session ses-00042", "Open session ses-00049"]
        and len(inventory_reads) >= 2 and all(q["time_interval"] == "1" and q["time_unit"] == "day" for q in inventory_reads),
        f"pages 1–25 and 26–50 of 60: {o['paging']}; Malicious on page 2: {o['malicious_rows']}; inventory queries {inventory_reads[:3]}",
        [o["tree_filter"]])
    add("C11-N02", o["listed_scans"] and o["no_content_before"] and o["revealed"] and o["hidden"] and o["gone_after_leaving"],
        f"scans listed: {o['listed_scans']}; no content read or shown before reveal: {o['no_content_before']}; one read and shown on reveal: "
        f"{o['revealed']}; Hide removed it: {o['hidden']}; none after leaving and returning: {o['gone_after_leaving']}",
        [o["tree_session"], o["tree_revealed"], o["tree_returned"]])
    add("C11-N03", o["detector"] and o["session_absent"] and o["scan_metadata"],
        f"detector coverage statement: {o['detector']}; session ID absent among 60 over 30 days: {o['session_absent']}; scan metadata only: "
        f"{o['scan_metadata']} ({o['scan_lookup_reads']} lookup read)", [o["tree_detector"], o["tree_scan"]])
    csv = o["csv"]
    add("C11-N04", o["report_listed"] and o["remove_undo"] and csv["saved"] and csv["mode"] == "0o600" and csv["header"] == CSV_HEADER
        and csv["rows"] == 1 and csv["crlf"] and not csv["content"],
        f"listed with metadata: {o['report_listed']}; remove/undo: {o['remove_undo']}; CSV {csv}", [o["tree_reports"], o["tree_export"]])
    sealed = o["sealed"]
    add("C11-N05", sealed["files"] == 1 and not sealed["plaintext"] and sealed["mode"] == "0o600" and o["restored_from_seal"] and o["removed_seal"],
        f"sealed {sealed}; shown after relaunch without a content read: {o['restored_from_seal']}; removed with the client: {o['removed_seal']}",
        [o["tree_relaunch"]])
    meta = o["metadata_export"]
    add("C11-N06", meta["saved"] and meta["sessions"] and not meta["content"] and meta["mode"] == "0o600" and o["cleared"] and o["read_after_clear"],
        f"metadata export {meta}; cleared with the seal removed: {o['cleared']}; read again after clearing: {o['read_after_clear']} (resealed: {o['resealed']})",
        [o["tree_cleared"]])
    add("C11-N07", not o["writes"] and not o["secret_in_trees"],
        f"{o['requests']} requests; non-read requests: {o['writes']}; secrets in AX snapshots: {o['secret_in_trees']}", [])
    add("C11-N08", o["exceptions"], f"synthetic label shown and no request sent: {o['exceptions']}", [o["tree_exceptions"]])
    pe = o["panel_export"]
    add("C11-N10", pe["panel_shown"] and pe["saved"] and pe["mode"] == "0o600" and pe["replaced"] and pe["replaced_mode"] == "0o600"
        and pe["cancelled"] and pe["focus_in_window"],
        f"production save panel: {pe}", [o["tree_panel_export"]])
    ts = o["tenant_switch"]
    add("C11-N11", all(ts.values()), f"switch to tenant B and back: {ts}",
        [o["tree_tenant_b_sessions"], o["tree_tenant_b_reports"]])
    zoom_ok = all(r["clear"] for r in (o["zoom_refresh"], o["zoom_next"], o["min_refresh"], o["min_next"]))
    add("C11-N09", zoom_ok,
        f"200% zoom: {o['zoom_refresh']['found']} / {o['zoom_next']['found']}; minimum size: {o['min_refresh']['found']} / {o['min_next']['found']}; "
        f"all clear: {zoom_ok}", [])

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1",
        "subject": "C11",
        "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (tools/native-acceptance/ax.swift, System Events keystrokes) against tools/native-acceptance/mgmt_server.py; tools/native-acceptance/acceptance_c11.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "N04/N06 save exports into the fixture data root without a sheet (never replacing a file); N10 drives the production rfd/NSSavePanel sheet (the fixture switch fixture-native-sheets) into a driver-owned temporary folder, including a confirmed Replace and Cancel. No Accessibility tree is saved while a panel lists folders.",
            "The dashboard endpoint is a local synthetic fixture; live sessions reads are qualified in C25.",
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
