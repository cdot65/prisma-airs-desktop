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
    ("Import from v1", "Import from v1", None),
    ("Settings", "Settings", None),
]
SECRETS = (c8.SCAN_KEY, c8.SECRET)


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
    size_window(app, pid, 1440, 900)
    os.makedirs(os.path.join(PARITY, "app"), exist_ok=True)
    with open(os.path.join(PARITY, "bundle.json"), "w") as f:
        json.dump({"bundle": prepared["bundle"], "executable_sha256": prepared["executable_sha256"], "from": "native-prepare.json",
                   "captured_by": "tools/native-acceptance/acceptance_c44.py", "captured_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}, f, indent=2)
        f.write("\n")
    secret_hits = []
    for item, head, product in DESTINATIONS:
        d = {}
        app.menu("Go", item)
        tree, d["opened"] = app.wait_for(lambda t: heading(t, head) is not None, 20)
        if product:
            tree, d["product_opened"] = app.wait_for(lambda t: any(n.get("role") == "AXButton" and (n.get("label") or "").startswith(product) and n.get("value") in (1, "1", True) for n in t) or any((n.get("label") or "") == f"Collapse {product}" for n in t) or app.has_text(t, product), 10)
        d["no_features_rail"] = c29.nav(tree, "Features") is None
        d["destinations_rail"] = c29.nav(tree, "Destinations") is not None
        d["no_interface_switch"] = fx.App.node(tree, "AXCheckBox", "Use the new interface") is None
        key = item.lower().replace(" & ", "-").replace(" ", "-")
        d["tree"] = c29.save_tree(f"all-{key}", tree)
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
    obs["n1"] = n1
    app.quit()

    run_path = os.path.join(NATIVE, "observations.json")
    with open(run_path, "w") as f:
        json.dump(json.loads(json.dumps({"observations": obs, "log": LOG}).replace(rel(NATIVE), rel(FINAL))), f, indent=1)
    run_rel = rel(run_path)
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C44"]}
    checks = []
    dest = obs["destinations"]
    all_ok = all(d["opened"] and d.get("product_opened", True) and d["no_features_rail"] and d["destinations_rail"] and d["no_interface_switch"] and all(d["captures"].values()) for d in dest.values())
    passed = n1["first_run_is_wizard"] and n1["no_features_rail_at_start"] and all_ok and not secret_hits and len(dest) == len(DESTINATIONS)
    checks.append({"id": "C44-N01", "description": inventory["C44-N01"], "result": "pass" if passed else "fail",
                   "observation": f"first run is the wizard {n1['first_run_is_wizard']} (no Features rail {n1['no_features_rail_at_start']}); "
                   + "; ".join(f"{k}: opened {v['opened']}" + (f", product {v.get('product_opened')}" if 'product_opened' in v else "") + f", no legacy rail {v['no_features_rail']}, Destinations {v['destinations_rail']}, no switch {v['no_interface_switch']}, captures {sorted(v['captures'])}" for k, v in dest.items())
                   + f"; plaintext secrets in any tree: {secret_hits}",
                   "evidence": [run_rel] + [v["tree"] for v in dest.values()] + [c for v in dest.values() for c in v["captures"].values() if c]})

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
            "Synthetic scan and management endpoints only; the Academy destination is opened as a guest (no Academy endpoint is staged); Sessions & Reports and Import from v1 are opened, not exercised (their drivers are C11 and C23).",
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
