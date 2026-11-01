#!/usr/bin/env python3
"""C44 native acceptance (P09 closure): one presentation. The fixture app's
first run is the wizard; after it, every destination the catalogue lists
(the sidebar's seven, the product entries that open Administration's
products, Sessions & Reports, Import from v1 and Settings) is opened from
the Go menu and captured in light and dark at 1440×900: the whole-app
screenshot set. The legacy shell is gone (no Features rail, no "Use the new
interface" switch) and no capture's tree carries a plaintext secret.

Run from the repository root after `xtask native-prepare --chunk C44`.
Requires the macOS Accessibility grant. Synthetic services only; the fixture
data root is removed at the end.

Checks (docs/architecture/native-checks.json, C44): C44-N01 cumulative
acceptance of every destination.
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
import acceptance_c28 as c28  # noqa: E402
import acceptance_c29 as c29  # noqa: E402
import v2_setup as v2s  # noqa: E402
import acceptance_c24 as c24  # noqa: E402  (the startup network sample and the Node-free sweep, carried over)
import legacy_assertions as la  # noqa: E402

ROOT = os.getcwd()
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence/C44")
NATIVE = os.path.join(EVIDENCE, "native")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json")
PARITY = os.path.join(EVIDENCE, "parity")
LOG = []
TITLE_BAR = 28
# Go menu item → the heading the destination shows (the product entries open Administration on their product;
# Sessions & Reports and Import from v1 keep their C11/C23 page titles under the v2 frame). The menu title keeps
# its ampersand (menu.rs doubles it for the menu library); the first run showed "Sessions  Reports" and no navigation.
DESTINATIONS = [
    ("Home", "Home", None),
    ("Inbox", "Inbox", None),
    ("Harness", "Harness", None),
    ("Academy", "AI Security Academy", None),
    ("Test Cases", "Test Cases", None),
    ("API Client", "API Client", None),
    ("Administration", "Administration", None),
    ("Runtime Security", "Administration", "AI Runtime Security"),
    ("AI Gateway", "Administration", "AI Gateway"),
    ("Red Team & Models", "Administration", "AI Red Teaming"),
    ("Sessions & Reports", "Investigate sessions", None),
    ("Settings", "Settings", None),
]
SECRETS = (c8.SCAN_KEY, c8.SECRET)
# The drivers whose records on this bundle the cumulative gate requires (legacy-script-map.md).
# The pre-P09 drivers that run on the one presentation through legacy_v2 (legacy_assertions: kind s) and the v2 drivers.
CUMULATIVE = sorted(({t.split("-")[0] for k, ts, _ in la.ASSERTIONS.values() if k in (la.X, la.S, la.P) for t in ts} - {"C44"})
                    | {"C10", "C11", "C35", "C36", "C37", "C38", "C39", "C40", "C41", "C42", "C43"})
# Learner chrome that must not appear outside Academy (C01-N12, carried over).
LEARNER_WORDS = ("XP", "streak", "credits", "earned skins", "badge")


def rel(path):
    return os.path.relpath(path, ROOT)


def heading(tree, label):
    return next((n for n in tree if n.get("role") == "AXHeading" and (n.get("label") or "").startswith(label)), None)


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
    work = tempfile.mkdtemp(prefix="airs-v2-c44-acceptance-")
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
    try:
        return exercise(app, endpoint, mgmt, prepared, FINAL)
    finally:
        endpoint.stop()
        mgmt.stop()
        shutil.rmtree(work, ignore_errors=True)


def exercise(app, endpoint, mgmt, prepared, FINAL):
    global NATIVE
    obs = {"captures": {}, "destinations": {}}
    app.launch()
    pid = app.pid
    # The first run is the wizard (no interface switch exists): walked once, then the driver's tenant through Settings.
    tree, _ = app.wait_for(lambda t: heading(t, v2s.WELCOME) is not None, 15)
    n1 = {"first_run_is_wizard": heading(tree, v2s.WELCOME) is not None, "no_features_rail_at_start": c29.nav(tree, "Features") is None}
    v2s.tenant_on_home(app, endpoint.origin)
    frame = size_window(app, pid, 1440, 900)
    geo = geometry(app.tree())
    # The window is asked for 1440×900 and gets what the display allows (the menu bar and the title bar take their
    # share); the record states the viewport and window actually captured.
    obs["geometry"] = {"requested": [1440, 900], "window": frame, "viewport": geo["viewport"], "web": geo["web"]}
    os.makedirs(os.path.join(PARITY, "app"), exist_ok=True)
    with open(os.path.join(PARITY, "bundle.json"), "w") as f:
        json.dump({"bundle": prepared["bundle"], "executable_sha256": prepared["executable_sha256"], "from": "native-prepare.json",
                   "captured_by": "tools/native-acceptance/acceptance_c44.py", "captured_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}, f, indent=2)
        f.write("\n")
    # ⌘-shortcuts through the native menu (C01-N05, carried over): ⌘2 opens the Inbox and ⌘, Settings; Go › Home after.
    app.keycode(19, "command down")
    _, n1["shortcut_inbox"] = app.wait_for(lambda t: heading(t, "Inbox") is not None, 10)
    app.keycode(43, "command down")
    _, n1["shortcut_settings"] = app.wait_for(lambda t: heading(t, "Settings") is not None, 10)
    app.menu("Go", "Home")
    app.wait_for(lambda t: heading(t, "Home") is not None, 10)
    # The built stylesheet carries the shared prefers-reduced-motion clamp (C01-N11, carried over; the same build the
    # bundle embeds): no v2 control depends on motion.
    css_dir = os.path.join(ROOT, "apps/desktop/dist/assets")
    css = "".join(open(os.path.join(css_dir, f)).read() for f in os.listdir(css_dir) if f.endswith(".css")) if os.path.isdir(css_dir) else ""
    n1["reduced_motion_clamp"] = "prefers-reduced-motion" in css and "animation" in css
    # The prepared bundle is Node-free and no Node, Bun or Deno process runs beside the app (C15-N09, C24-N05).
    n1["node_free"] = c24.node_free(prepared["bundle"])
    secret_hits = []
    learner_hits = []
    for item, head, product in DESTINATIONS:
        d = {}
        app.menu("Go", item)
        tree, d["opened"] = app.wait_for(lambda t: heading(t, head) is not None, 20)
        if product:
            # The product's card is open: its header button (the card's own toggle, labelled with the product and its
            # description) is followed by its tab strip, the AXTabGroup "<product> tabs", holding an Overview tab. A
            # collapsed card draws no tab group, and the product's name alone (every card's header) does not pass.
            # (The first attempt-02 run looked for a "Collapse <product>" control the card never labels that way.)
            def product_open(t):
                strip = next((n for n in t if n.get("role") == "AXTabGroup" and (n.get("label") or "") == f"{product} tabs"), None)
                header = next((n for n in t if n.get("role") == "AXButton" and (n.get("label") or "").startswith(product)), None)
                if strip is None or header is None:
                    return False
                i = t.index(strip)
                for m in t[i + 1:i + 60]:
                    if m.get("role") in ("AXRadioButton", "AXTab") and (m.get("label") or "") == "Overview":
                        return True
                return False
            tree, d["product_opened"] = app.wait_for(product_open, 10)
        d["no_features_rail"] = c29.nav(tree, "Features") is None
        d["destinations_rail"] = c29.nav(tree, "Destinations") is not None
        d["no_interface_switch"] = fx.App.node(tree, "AXCheckBox", "Use the new interface") is None
        key = item.lower().replace(" & ", "-").replace(" ", "-")
        d["tree"] = c29.save_tree(f"all-{key}", tree)
        if item != "Academy":
            words = " ".join(app.texts(tree))
            learner_hits += [f"{item}: {w}" for w in LEARNER_WORDS if f" {w} " in f" {words} " or words.startswith(w + " ") or words.endswith(" " + w)]
        for s in SECRETS:
            if app.has_text(tree, s):
                secret_hits.append((item, s[:4] + "…"))
        d["captures"] = {}
        for theme in ("Light", "Dark"):
            c29.appearance(app, theme)
            time.sleep(0.7)
            cap = c29.capture(app, f"all-{key}-{theme.lower()}")
            d["captures"][theme.lower()] = cap
            if cap:
                shutil.copy(os.path.join(ROOT, cap), os.path.join(PARITY, "app", f"{key}-{theme.lower()}.png"))
        c29.appearance(app, "Light")
        time.sleep(0.4)
        obs["destinations"][item] = d
        obs["captures"][f"all-{key}"] = dict(d["captures"])
    n1["secret_hits"] = secret_hits
    n1["learner_hits"] = learner_hits
    # No Node, Bun or Deno process runs beside the app: judged on the app's own descendants (the Mac may run other
    # tools' node processes; the legacy C15-N09 asserted the app spawned none).
    ps = [l.split(None, 2) for l in fx.sh(["ps", "-axo", "pid=,ppid=,comm="]).stdout.splitlines() if l.strip()]
    children = {}
    for pid_s, ppid_s, comm in ps:
        children.setdefault(int(ppid_s), []).append((int(pid_s), comm))
    family, stack = [], [app.pid]
    while stack:
        for cpid, comm in children.get(stack.pop(), []):
            family.append((cpid, comm)); stack.append(cpid)
    n1["runtime_processes"] = [f"{cpid} {comm}" for cpid, comm in family if os.path.basename(comm) in ("node", "bun", "deno")]
    n1["app_descendants"] = [os.path.basename(comm) for _, comm in family]
    # Persistence and startup (C02-N03, C08-N10, C01-N13, C24-N03 carried over): quit, relaunch with the configured
    # tenant, sample the app's and its WebKit processes' internet sockets for 15 s, then the tenant is still active
    # with its client (Administration reads again) and no wizard shows.
    app.quit()
    webkit_before = c24.webkit_pids()
    app.launch()
    sockets, lines, sampled = c24.network(app, webkit_before, seconds=15)
    net_path = os.path.join(NATIVE, "startup-network-samples.log")
    with open(net_path, "w") as f:
        f.write("\n".join([f"sampled pid={p} ({n})" for p, n in sorted(sampled.items())] + lines) + "\n")
    tree, n1["relaunch_home"] = app.wait_for(lambda t: heading(t, "Home") is not None and heading(t, v2s.WELCOME) is None, 20)
    n1["relaunch_tenant"] = any((n.get("label") or "").startswith("Switch tenant: Northwind Health") for n in tree)
    app.menu("Go", "Administration")
    tree, n1["relaunch_admin"] = app.wait_for(lambda t: heading(t, "Administration") is not None and app.has_text(t, "Connected"), 25)
    obs["tree_relaunch"] = c29.save_tree("relaunch-administration", tree)
    n1["startup"] = {"sockets": sockets, "webkit_sampled": sum(1 for n in sampled.values() if "WebKit" in n), "log": rel(net_path)}
    obs["n1"] = n1
    app.quit()
    # The cumulative gate: every mapped driver's record on this same bundle (tools/native-acceptance/run_on_v2.py
    # files them under native/cumulative/<CNN>/), bound to the prepared executable; a missing, stale or failing one
    # fails C44-N01. The set is the drivers the legacy-script map names as replacements and the pre-P09 drivers it
    # keeps running on the one presentation (C10, C11; C23 retired by D060).
    cumulative = {}
    for chunk in CUMULATIVE:
        path = os.path.join(EVIDENCE, "native", "cumulative", chunk, "native-acceptance.json")
        try:
            rec = json.load(open(path))
            cumulative[chunk] = {"record": rel(path), "bound": rec["app"]["executable_sha256"] == prepared["executable_sha256"],
                                 "checks": {c["id"]: c["result"] for c in rec["checks"]}, "recorded_utc": rec.get("recorded_utc")}
        except (OSError, ValueError, KeyError) as e:
            cumulative[chunk] = {"record": rel(path), "bound": False, "checks": {}, "missing": str(e)}
    obs["cumulative"] = cumulative

    run_path = os.path.join(NATIVE, "observations.json")
    with open(run_path, "w") as f:
        json.dump(json.loads(json.dumps({"observations": obs, "log": LOG}).replace(rel(NATIVE), rel(FINAL))), f, indent=1)
    run_rel = rel(run_path)
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C44"]}
    checks = []
    dest = obs["destinations"]
    all_ok = all(d["opened"] and d.get("product_opened", True) and d["no_features_rail"] and d["destinations_rail"] and d["no_interface_switch"] and all(d["captures"].values()) for d in dest.values())
    # A record must be present and bound to this executable. Every check of a v2 driver must pass; of a self-run legacy
    # driver the checks the map keeps (its targets) must pass — a legacy check the map retires or supersedes (C24-N06,
    # the legacy cumulative matrix, is this check) is reported but not required.
    mapped_targets = {t for k, ts, _ in la.ASSERTIONS.values() if k in (la.X, la.S, la.P) for t in ts}
    def record_ok(chunk, v):
        if not (v["bound"] and v["checks"]):
            return False
        required = [i for i in v["checks"] if i in mapped_targets] if chunk < "C28" else list(v["checks"])
        return bool(required) and all(v["checks"][i] == "pass" for i in required)
    cumulative_ok = all(record_ok(k, v) for k, v in cumulative.items())
    carried = (n1["shortcut_inbox"] and n1["shortcut_settings"] and n1["reduced_motion_clamp"] and not n1["node_free"] and not n1["runtime_processes"]
               and not learner_hits and n1["relaunch_home"] and n1["relaunch_tenant"] and n1["relaunch_admin"]
               and n1["startup"]["sockets"] == 0 and n1["startup"]["webkit_sampled"] > 0)
    # Every legacy assertion the map keeps (exercised, self-run or ported) has its targets passing on this bundle;
    # a retirement carries its reason. C44-N01's own targets are this check's other facts.
    results_by_id = {i: r for v in cumulative.values() for i, r in v["checks"].items()}
    own_ok = n1["first_run_is_wizard"] and n1["no_features_rail_at_start"] and all_ok and not secret_hits and len(dest) == len(DESTINATIONS) and cumulative_ok and carried
    results_by_id["C44-N01"] = "pass" if own_ok else "fail"
    uncovered = []
    for lid, (kind, targets, note) in la.ASSERTIONS.items():
        if kind == la.R:
            if not note:
                uncovered.append(f"{lid} (retired without a reason)")
        elif not targets or any(results_by_id.get(t) != "pass" for t in targets):
            uncovered.append(lid + " → " + ", ".join(t + " " + str(results_by_id.get(t, "not run")) for t in targets))
    from collections import Counter
    kinds = Counter(k for k, _, _ in la.ASSERTIONS.values())
    coverage_text = f"legacy assertions mapped: {len(la.ASSERTIONS)} ({kinds.get(la.X, 0)} exercised, {kinds.get(la.S, 0)} self-run, {kinds.get(la.P, 0)} ported, {kinds.get(la.R, 0)} retired with reasons); without a passing target on this bundle: {uncovered or 'none'}"
    obs["coverage"] = {"uncovered": uncovered, "kinds": dict(kinds)}
    cumulative_text = "; ".join(f"{k}: " + ("not run" if not v["checks"] else ("stale bundle; " if not v["bound"] else "") + ", ".join(f"{i} {r}" + ("" if i in mapped_targets or k >= "C28" else " (not required: retired or superseded in the map)") for i, r in v["checks"].items())) for k, v in cumulative.items())
    passed = own_ok and not uncovered
    checks.append({"id": "C44-N01", "description": inventory["C44-N01"], "result": "pass" if passed else "fail",
                   "observation": f"window requested 1440×900, got window {obs['geometry']['window']} viewport {obs['geometry']['viewport']} (the display's menu bar and the title bar take the rest); first run is the wizard {n1['first_run_is_wizard']} (no Features rail {n1['no_features_rail_at_start']}); "
                   + "; ".join(f"{k}: opened {v['opened']}" + (f", product {v.get('product_opened')}" if 'product_opened' in v else "") + f", no legacy rail {v['no_features_rail']}, Destinations {v['destinations_rail']}, no switch {v['no_interface_switch']}, captures {sorted(v['captures'])}" for k, v in dest.items())
                   + f"; plaintext secrets in any tree: {secret_hits}; learner chrome outside Academy: {learner_hits}; ⌘2 opened the Inbox {n1['shortcut_inbox']} and ⌘, Settings {n1['shortcut_settings']}; the built stylesheet carries the reduced-motion clamp {n1['reduced_motion_clamp']}; Node or JavaScript runtime files in the bundle: {n1['node_free']}; runtime processes beside the app: {n1['runtime_processes']}; after a relaunch Home with no wizard {n1['relaunch_home']}, the tenant active {n1['relaunch_tenant']}, Administration reading with the kept client {n1['relaunch_admin']}, internet sockets over 15 s of startup {n1['startup']['sockets']} (WebKit processes sampled {n1['startup']['webkit_sampled']}); cumulative records on this bundle: {cumulative_text}; {coverage_text}",
                   "evidence": [run_rel, obs["tree_relaunch"], n1["startup"]["log"]] + [v["tree"] for v in dest.values()] + [c for v in dest.values() for c in v["captures"].values() if c] + [v["record"] for v in cumulative.values() if v["checks"]]})

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1", "subject": "C44", "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (System Events and the ax helper) with fixture WebView self-capture; tools/native-acceptance/acceptance_c44.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "Pixels come from the fixture build's own WKWebView snapshot; the whole-app set under evidence/C44/parity/app is the app alone (C44 has no prototype board of its own).",
            "Synthetic scan and management endpoints only; the Academy destination is opened as a guest here (no Academy endpoint is staged for this driver; the Academy drivers C04–C07 stage their own and run in the cumulative set); Sessions & Reports is opened, not exercised here (acceptance_c11.py exercises it in the cumulative set); the alpha import is gone (D060).",
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
    # The cumulative drivers' records (run_on_v2.py, before this driver) live under native/cumulative: they move
    # into the staged folder so the swap keeps them (the first cumulative run lost them here).
    kept = os.path.join(FINAL, "cumulative")
    if os.path.isdir(kept):
        shutil.move(kept, os.path.join(NATIVE, "cumulative"))
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
