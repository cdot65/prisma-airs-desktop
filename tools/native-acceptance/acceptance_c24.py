#!/usr/bin/env python3
"""C24 native acceptance: cumulative privacy, diagnostics, network and
offline closure through the real fixture window, and the cumulative T09
matrix (every earlier chunk's native acceptance on this bundle).

A secret (a scan key) and content (a prompt) are put through the window;
the diagnostics are exported and cleared; the startup network is sampled;
work imported from a staged v1 (cases, FP/FN reports, a receipt) is read in
an offline launch and nothing is sent when the synthetic services come back.

Run from the repository root after `xtask native-prepare --chunk C24` and
after the C01–C23 drivers ran on the same bundle with AIRS_NATIVE_SUBJECT=C24.
Requires the Accessibility grant. Synthetic data only; the fixture data root
(with its keychains, exports and logs) is removed at the end; the real v1
folder and the login keychain are untouched.
"""

import json
import os
import shutil
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fixture_app as fx  # noqa: E402
import acceptance_c08 as c8  # noqa: E402
import acceptance_c23 as c23  # noqa: E402
import native_sheets as ns  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C24")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C24" else "native-C24")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C24" else "native-acceptance-C24.json")
SECRET = "SYNTHETIC-C24-NATIVE-SECRET-scan-key"
CONTENT = "SYNTHETIC-C24-NATIVE-CONTENT-prompt"
EXPORTS = os.path.join(fx.DATA, "fixture-exports")
LOGS = os.path.join(fx.DATA, "fixture-logs")
EARLIER = [f"C{n:02d}" for n in range(1, 24)]
LOG = []


def rel(path):
    return os.path.relpath(path, ROOT)


def save_tree(name, tree):
    path = os.path.join(NATIVE, f"tree-{name}.json")
    with open(path, "w") as f:
        json.dump(tree, f, indent=0)
    return rel(path)


def files(base):
    out = []
    for d, _, names in os.walk(base):
        out += [os.path.join(d, n) for n in names]
    return out


def holding(base, needle, skip=()):
    hits = []
    for p in files(base):
        if any(s in p for s in skip):
            continue
        try:
            if needle.encode() in open(p, "rb").read():
                hits.append(os.path.relpath(p, base))
        except OSError:
            continue
    return hits


def webkit_pids():
    return {int(p) for p in fx.sh(["pgrep", "-f", "com.apple.WebKit"]).stdout.split()}


def network(app, before, seconds=15):
    """Internet sockets of the app and of every WebKit process that appeared
    with its launch (`before`: the WebKit processes running until then),
    sampled every 0.5 s. Each line names its process; the processes sampled
    are returned so that an empty WebKit set cannot pass unnoticed."""
    sockets, lines, sampled = 0, [], {}
    for i in range(seconds * 2):
        procs = ([app.pid] if app.pid else []) + sorted(webkit_pids() - before)
        for p in procs:
            if p not in sampled:
                sampled[p] = fx.sh(["ps", "-o", "comm=", "-p", str(p)]).stdout.strip().rsplit("/", 1)[-1]
            out = fx.sh(["lsof", "-nP", "-a", "-p", str(p), "-i"]).stdout.splitlines()[1:]
            sockets += len(out)
            lines += [f"t={i / 2:.1f} pid={p} ({sampled[p]}) {line}" for line in out]
        time.sleep(0.5)
    return sockets, lines, sampled


def node_free(bundle):
    names = {"node", "bun", "npm", "npx", "airs-cli"}
    found = []
    for d, dirs, fs in os.walk(bundle):
        found += [os.path.join(d, x) for x in dirs if x == "node_modules"]
        found += [os.path.join(d, x) for x in fs if x in names or x.endswith((".mjs", ".cjs", ".js.map", ".map"))]
    return [os.path.relpath(p, bundle) for p in found]


def main():
    try:
        return run()
    finally:
        fx.sh(["pkill", "-f", fx.PROCESS])
        if os.path.exists(c23.V1_KEYCHAIN):
            fx.sh(["security", "delete-keychain", c23.V1_KEYCHAIN])
        fx.clean_fixture_root()


def run():
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")
    if c8.sha256(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c24-acceptance-")
    if os.path.isdir(NATIVE):
        shutil.rmtree(NATIVE)
    os.makedirs(NATIVE)
    app = fx.App(bundle, work, LOG)
    app.quit()
    fx.clean_fixture_root()
    v1_before = fx.v1_metadata()
    endpoint = fx.ScanEndpoint(os.path.join(work, "endpoint"), SECRET)
    endpoint.install_roots()
    endpoint.start()
    mgmt = c8.MgmtEndpoint(os.path.join(work, "endpoint"), os.path.join(work, "mgmt"))
    mgmt.install()
    mgmt.start()
    mgmt.mode()
    os.makedirs(fx.DATA, exist_ok=True)
    # A staged v1 (C23's fixture): imported below as this run's local work.
    c23.stage()
    obs = {}

    # ---- N02 (seeding): a secret and content through the window ----
    app.launch()
    app.open_settings()
    app.type_into("AXTextField", "TSG ID (optional)", c23.PROD_TSG)
    app.create_custom_tenant("Canary Tenant", endpoint.origin)
    _, stored = app.save_key(SECRET)
    app.make_active("Canary Tenant")
    app.open_runtime()
    app.type_into("AXTextField", "Profile name or ID", c8.PROFILE)
    app.type_into("AXTextArea", "Prompt", CONTENT)
    before = len(endpoint.requests())
    app.ax("press", "AXButton", "Scan", 0)
    tree, scanned = c8.text(app, "Allowed", 15)
    obs["seed"] = {"stored": stored, "scanned": scanned and len(endpoint.requests()) == before + 1}
    obs["tree_scan"] = save_tree("scan", tree)

    # ---- N04 (staging): import the v1 work ----
    app.ax("press", "AXButton", "Import from v1")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Import from v1") is not None, 10)
    c23.press(app, "Look for v1 data")
    c8.text(app, "v1’s data was found.", 12)
    for label in ("Tenants (names and TSG IDs)", "Saved cases", "FP/FN reports"):
        c23.press(app, label)
    c23.press(app, "Review what would be imported")
    tree, _ = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Review") is not None, 20)
    importer = next(l for l in c23.labelled(tree, "AXButton", "Import ") if l != "Import from v1")
    c23.press(app, importer)
    tree, imported = c8.text(app, "Imported ·", 30)
    obs["staged"] = {"imported": imported}

    # ---- N01: diagnostics in Settings ----
    app.open_settings()
    tree, stated = c8.text(app, "Kept only on this Mac for 14 days and never sent anywhere", 10)
    counted = any((n.get("label") or "").endswith("events kept.") or "events kept." in str(n.get("value") or "")
                  for n in tree) or app.has_text(tree, "events kept.")
    c23.press(app, "Export diagnostics…")
    tree, saved = c8.text(app, "Diagnostics saved.", 15)
    exported = [p for p in files(EXPORTS) if p.endswith("diagnostics.json")]
    document = json.load(open(exported[0])) if exported else {}
    obs["tree_diagnostics"] = save_tree("diagnostics", tree)
    obs["diagnostics"] = {
        "stated": stated,
        "counted": counted,
        "saved": saved and bool(exported),
        "schema": document.get("schema"),
        "events": len(document.get("events", [])),
        "kinds_only": all(set(e.get("event", {})) <= {"type", "kind", "refusal", "version", "fixture", "health",
                                                      "outcome", "file", "line"} for e in document.get("events", [])),
    }

    # ---- N02 (search): the canaries ----
    trees = [os.path.join(NATIVE, n) for n in os.listdir(NATIVE) if n.startswith("tree-")]
    obs["canaries"] = {
        "secret_in_files": holding(fx.DATA, SECRET, skip=(".keychain-db",)),
        "secret_in_trees": [rel(p) for p in trees if SECRET in open(p).read()],
        "content_in_diagnostics": holding(LOGS, CONTENT) + [rel(p) for p in exported if CONTENT in open(p).read()],
        "secret_in_diagnostics": holding(LOGS, SECRET) + [rel(p) for p in exported if SECRET in open(p).read()],
    }
    c23.press(app, "Clear diagnostics")
    time.sleep(1.0)
    lines = sum(len(open(p).read().splitlines()) for p in files(LOGS))
    obs["diagnostics"]["cleared"] = lines <= 2

    # ---- N07: Diagnostics by keyboard, through the production NSSavePanel ----
    # No AX tree is saved while a panel lists folders.
    out = os.path.join(work, "panel-exports")
    os.makedirs(out)
    ns.enable()
    app.open_settings()
    reached = c8.reach(app, 1, "Export diagnostics…")
    app.keycode(49)  # Space presses the focused button
    panel = ns.wait_sheet(app)
    saved_panel = ns.save_as(app, out)
    tree, said = c8.text(app, "Diagnostics saved.", 10)
    focus_saved = fx.App.focused(tree)
    target = os.path.join(out, "prisma-airs-desktop-diagnostics.json")
    try:
        exported_doc = json.load(open(target))
    except (OSError, ValueError):
        exported_doc = {}
    app.keycode(49)  # still on Export: the panel again, then Cancel
    cancelled = ns.cancel(app)
    tree, cancel_said = c8.text(app, "Nothing was saved.", 10)
    focus_cancelled = fx.App.focused(tree)
    obs["tree_panel_cancelled"] = save_tree("diagnostics-panel-cancelled", tree)
    ns.disable()
    clear = c8.reach(app, 1, "Clear diagnostics")
    app.keycode(49)
    tree, cleared_said = c8.text(app, "Diagnostics cleared.", 10)
    obs["tree_cleared"] = save_tree("diagnostics-cleared", tree)
    status = [n for n in tree if n.get("role") == "AXStaticText" and n.get("label") == "Diagnostics cleared."]
    for _ in range(10):
        app.keys("=", "command down")
    zoomed = c8.reach(app, 2, "Clear diagnostics")
    app.keys("0", "command down")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{900, 600}}'])
    time.sleep(0.8)
    smallest = c8.reach(app, 1, "Export diagnostics…")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{1280, 820}}'])
    # Focus comes back to the control that opened the panel, not just the window.
    on_export = lambda f: bool(f) and f.get("role") == "AXButton" and f.get("label") == "Export diagnostics…"  # noqa: E731
    obs["keyboard"] = {
        "export_reached": bool(reached["found"]),
        "panel_shown": panel,
        "saved": saved_panel and said and exported_doc.get("schema") == "prisma-airs-desktop-v2/diagnostics/1",
        "focus_after_save": (focus_saved or {}).get("label"),
        "focus_after_save_on_export": on_export(focus_saved),
        "cancelled": cancelled and cancel_said and sorted(os.listdir(out)) == ["prisma-airs-desktop-diagnostics.json"],
        "focus_after_cancel": (focus_cancelled or {}).get("label"),
        "focus_after_cancel_on_export": on_export(focus_cancelled),
        "clear_reached": bool(clear["found"]),
        "cleared_announced": cleared_said and bool(status),
        "zoom_200": zoomed,
        "minimum_size": smallest,
    }

    # ---- N03: startup network with a configured tenant ----
    app.quit()
    before = webkit_pids()
    app.launch(settle=0)
    sockets, net_lines, sampled = network(app, before)
    path = os.path.join(NATIVE, "startup-network-samples.log")
    with open(path, "w") as f:
        f.write("\n".join([f"sampled pid={p} ({n})" for p, n in sorted(sampled.items())] + net_lines) + "\n")
    obs["network"] = {"startup_sockets": sockets, "log": rel(path),
                      "processes": sorted(set(sampled.values())),
                      "webkit_sampled": sum(1 for n in sampled.values() if "WebKit" in n)}

    # ---- N04: offline launch, then reconnect ----
    app.quit()
    endpoint.stop()
    mgmt.stop()
    before = webkit_pids()
    app.launch()
    app.open_settings()
    # v1’s Prod shares Canary Tenant’s TSG ID, so its work was imported there.
    app.make_active("Canary Tenant")
    app.ax("press", "AXButton", "Case Library")
    tree, cases = c8.text(app, "Injection", 15)
    obs["tree_offline_library"] = save_tree("offline-library", tree)
    app.ax("press", "AXButton", "Sessions & Reports")
    tree, reports = c8.text(app, "FP/FN reports (1)", 15)
    app.ax("press", "AXButton", "Import from v1")
    tree, receipt = c8.text(app, "Imported ·", 15)
    app.open_settings()
    tree, diag = c8.text(app, "events kept.", 10)
    obs["tree_offline_settings"] = save_tree("offline-settings", tree)
    sent_before = len(endpoint.requests()) + len(mgmt.requests())
    endpoint.start()
    mgmt.start()
    mgmt.mode()
    for label in ("Case Library", "Sessions & Reports", "Runtime Security", "Import from v1"):
        app.ax("press", "AXButton", label)
        time.sleep(2.0)
    # The services are reachable again; the app and its WebKit processes
    # are sampled while nothing is asked of them.
    re_sockets, re_lines, re_sampled = network(app, before, seconds=8)
    path = os.path.join(NATIVE, "reconnect-network-samples.log")
    with open(path, "w") as f:
        f.write("\n".join([f"sampled pid={p} ({n})" for p, n in sorted(re_sampled.items())] + re_lines) + "\n")
    obs["offline"] = {"cases": cases, "reports": reports, "receipt": receipt, "diagnostics": diag,
                      "sent_on_reconnect": len(endpoint.requests()) + len(mgmt.requests()) - sent_before,
                      "reconnect_sockets": re_sockets, "reconnect_log": rel(path),
                      "reconnect_webkit_sampled": sum(1 for n in re_sampled.values() if "WebKit" in n)}
    app.quit()

    # ---- N05: Node-free ----
    obs["node_free"] = node_free(bundle)

    # ---- N06: the earlier chunks on this bundle ----
    earlier = {}
    for chunk in EARLIER:
        record = os.path.join(EVIDENCE, f"native-acceptance-{chunk}.json")
        try:
            data = json.load(open(record))
            earlier[chunk] = (data["app"]["executable_sha256"] == prepared["executable_sha256"]
                              and all(c["result"] == "pass" for c in data["checks"]))
        except (OSError, KeyError, ValueError):
            earlier[chunk] = False
    obs["earlier"] = earlier

    endpoint.stop()
    mgmt.stop()
    obs["v1_unchanged"] = fx.v1_metadata() == v1_before
    shutil.rmtree(work, ignore_errors=True)
    with open(os.path.join(NATIVE, "observations.json"), "w") as f:
        json.dump({"observations": obs, "log": LOG}, f, indent=1)
    return evaluate(prepared, obs)


def evaluate(prepared, o):
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C24"]}
    obs_path = rel(os.path.join(NATIVE, "observations.json"))
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [obs_path] + [e for e in evidence if e]})

    d = o["diagnostics"]
    add("C24-N01", d["stated"] and d["counted"] and d["saved"] and d["schema"] == "prisma-airs-desktop-v2/diagnostics/1"
        and d["events"] > 0 and d["kinds_only"] and d["cleared"], f"diagnostics {d}", [o["tree_diagnostics"]])
    c = o["canaries"]
    add("C24-N02", o["seed"]["stored"] and o["seed"]["scanned"] and not any(c.values()),
        f"seeded {o['seed']}; found {c}", [o["tree_scan"]])
    n = o["network"]
    add("C24-N03", n["startup_sockets"] == 0 and n["webkit_sampled"] > 0 and o["offline"]["sent_on_reconnect"] == 0
        and o["offline"]["reconnect_sockets"] == 0 and o["offline"]["reconnect_webkit_sampled"] > 0,
        f"startup internet sockets over 15 s of the app and its WebKit processes ({n['processes']}, WebKit sampled: "
        f"{n['webkit_sampled']}): {n['startup_sockets']}; after reconnect: requests {o['offline']['sent_on_reconnect']}, "
        f"sockets over 8 s {o['offline']['reconnect_sockets']} (WebKit sampled: {o['offline']['reconnect_webkit_sampled']})",
        [n["log"]])
    f = o["offline"]
    add("C24-N04", o["staged"]["imported"] and f["cases"] and f["reports"] and f["receipt"] and f["diagnostics"]
        and f["sent_on_reconnect"] == 0, f"offline {f}", [o["tree_offline_library"], o["tree_offline_settings"]])
    add("C24-N05", not o["node_free"], f"Node or JavaScript runtime files in the bundle: {o['node_free']}", [])
    e = o["earlier"]
    k = o["keyboard"]
    add("C24-N07", k["export_reached"] and k["panel_shown"] and k["saved"] and k["focus_after_save_on_export"]
        and k["cancelled"] and k["focus_after_cancel_on_export"] and k["clear_reached"] and k["cleared_announced"]
        and k["zoom_200"]["clear"] and k["minimum_size"]["clear"],
        f"Export reached by Tab and pressed with Space: {k['export_reached']}; production save panel shown: {k['panel_shown']}; "
        f"saved and announced: {k['saved']}; focus after saving: {k['focus_after_save']}; cancelled, nothing saved and announced: "
        f"{k['cancelled']}; focus after cancelling: {k['focus_after_cancel']}; Clear reached and its outcome announced: "
        f"{k['clear_reached'] and k['cleared_announced']}; 200% zoom: {k['zoom_200']['found']} clear {k['zoom_200']['clear']}; "
        f"minimum size: {k['minimum_size']['found']} clear {k['minimum_size']['clear']}",
        [o["tree_panel_cancelled"], o["tree_cleared"]])
    add("C24-N06", all(e.values()), f"earlier chunks passing on this bundle: {sorted(k for k, v in e.items() if v)}; not: {sorted(k for k, v in e.items() if not v)}",
        [rel(os.path.join(EVIDENCE, f"native-acceptance-{k}.json")) for k in sorted(e)])

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1",
        "subject": "C24",
        "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "method": "cua",
        "performed_by": "Claude Code driving the real fixture window and the production NSSavePanel (fixture switch fixture-native-sheets) through macOS Accessibility (tools/native-acceptance/ax.swift, System Events keystrokes); tools/native-acceptance/acceptance_c24.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "Spoken VoiceOver output is not recorded (no operator listened); Accessibility roles, names, order and focus are inspected by every earlier chunk's driver on this bundle.",
            "Network evidence is sampled lsof output (0.5 s), not a packet capture; DNS through mDNSResponder is not attributed. Reconnect is the synthetic services becoming reachable again.",
            "The offline launch covers the categories this driver stages (imported cases, FP/FN reports, an import receipt, diagnostics); Academy, API Client, Gateway and Harness offline work is covered by C07, C14, C15, C17, C18, C20 and C21's drivers on this bundle (C24-N06).",
            "The fixture build keeps diagnostics under its data root (fixture-logs/); production builds keep them in macOS's logs folder for the app (D051).",
            "N01 exports without a sheet into the fixture data root; N07 drives the production NSSavePanel into a driver-owned temporary folder. No Accessibility tree is saved while a panel lists folders.",
            "WebKit processes are those that appeared with the launch (pgrep com.apple.WebKit before and after); another app's WebKit process started meanwhile would be sampled too, which can only add sockets, never hide one.",
        ],
    }
    with open(RECORD, "w") as f:
        json.dump(record, f, indent=2)
        f.write("\n")
    for ch in checks:
        print(ch["result"].upper(), ch["id"], ch["observation"][:220])
    return 0 if all(ch["result"] == "pass" for ch in checks) else 1


if __name__ == "__main__":
    sys.exit(main())
