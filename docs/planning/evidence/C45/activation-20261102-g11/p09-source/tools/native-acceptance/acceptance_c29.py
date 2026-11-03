#!/usr/bin/env python3
"""C29 native acceptance: the first-run wizard and the add-tenant sheet in the
v2 shell, driven through the real fixture window.

Run from the repository root after `xtask native-prepare --chunk C29`.
Requires the macOS Accessibility grant. Synthetic services only (the scan
endpoint for key custody, the management endpoint for the verify read); the
fixture data root is removed at the end.

Checks (docs/architecture/native-checks.json, C29): first run opens the wizard
(N01); the tenant step drafts in the core, attaches credentials to the draft
and edits the draft (N02); products, the core-owned verify run with fail,
fix, pass and stale paths (N03); starter cases, Academy, completion and
relaunch (N04); the add-tenant sheet with Cancel discarding the draft and
Finish committing it (N05); the T09 matrix: every step captured in light and
dark at 100% and 200%, the minimum window size, keyboard reach (N06).
"""

import glob
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fixture_app as fx  # noqa: E402
import acceptance_c08 as c8  # noqa: E402  (synthetic management endpoint)

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C29")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C29" else "native-C29")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C29" else "native-acceptance-C29.json")
CAPTURES = os.path.join(subprocess.run(["getconf", "DARWIN_USER_TEMP_DIR"], capture_output=True, text=True).stdout.strip(),
                        "prisma-airs-v2-fixture-captures")
SCAN_KEY = "SYNTHETIC-C29-NATIVE-SCAN-KEY-abcdef"
# The synthetic management endpoint accepts only its own synthetic client (acceptance_c08).
CLIENT_ID = c8.CLIENT_ID
CLIENT_SECRET = c8.SECRET
TSG = c8.TSG
LOG = []


def sh(argv, **kw):
    return subprocess.run(argv, capture_output=True, text=True, **kw)


def sha256(path):
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def rel(path):
    return os.path.relpath(path, ROOT)


def save_tree(name, tree):
    path = os.path.join(NATIVE, f"tree-{name}.json")
    with open(path, "w") as f:
        json.dump(tree, f, indent=0)
    return rel(path)


def capture(app, name):
    before = set(glob.glob(os.path.join(CAPTURES, "capture-*.png")))
    app.menu("Fixture", "Capture Window")
    for _ in range(20):
        time.sleep(0.3)
        new = sorted(set(glob.glob(os.path.join(CAPTURES, "capture-*.png"))) - before)
        if new:
            dest = os.path.join(NATIVE, "captures", f"{name}.png")
            os.makedirs(os.path.dirname(dest), exist_ok=True)
            shutil.copy(new[-1], dest)
            return rel(dest)
    return None


def nav(tree, label):
    return next((n for n in tree if n.get("subrole") == "AXLandmarkNavigation" and n.get("label") == label), None)


def press(app, role, label, timeout=8):
    app.wait_for(lambda t: fx.App.node(t, role, label) is not None, timeout)
    app.ax("press", role, label)


def heading(app, title, timeout=10):
    return app.wait_for(lambda t: fx.App.node(t, "AXHeading", title) is not None, timeout)


def holding(base, needle, skip=()):
    out = []
    for d, _, names in os.walk(base):
        for n in names:
            p = os.path.join(d, n)
            if p.endswith(tuple(skip)):
                continue
            try:
                if needle.encode() in open(p, "rb").read():
                    out.append(rel(p))
            except OSError:
                pass
    return out


def main():
    try:
        return run()
    except Exception:
        try:
            with open(os.path.join(NATIVE, "tree-failure.json"), "w") as f:
                json.dump(_last_app_tree(), f, indent=0)
        except Exception as e:  # noqa: BLE001
            print("no failure tree:", e)
        raise
    finally:
        fx.sh(["pkill", "-f", fx.PROCESS])
        fx.clean_fixture_root()


_APP = None


def _last_app_tree():
    return _APP.tree() if _APP else []


def set_window_size(pid, w, h):
    sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {pid}) to set size of window 1 to {{{w}, {h}}}'])
    time.sleep(0.8)


def window_frame(pid):
    out = sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {pid}) to get {{position, size}} of window 1']).stdout.strip()
    nums = [int(x) for x in out.replace("{", "").replace("}", "").split(",") if x.strip().lstrip("-").isdigit()]
    return nums if len(nums) == 4 else None


def inside(frame, node):
    if not frame or not node or not node.get("pos") or not node.get("size"):
        return False
    x, y, w, h = frame
    px, py = node["pos"]
    pw, ph = node["size"]
    return x <= px and px + pw <= x + w and y <= py and py + ph <= y + h


def zoom(app, steps):
    for _ in range(abs(steps)):
        app.menu("View", "Zoom In" if steps > 0 else "Zoom Out")
        time.sleep(0.15)
    time.sleep(0.8)


def appearance(app, which):
    app.menu("View", "Appearance", which)
    time.sleep(1.0)


def matrix(app, name):
    """Four captures of the current step: light/dark at 100% and 200% (E1), each
    theme selected explicitly (the system appearance is never assumed)."""
    appearance(app, "Light")
    out = {"light-100": capture(app, f"{name}-light-100")}
    appearance(app, "Dark")
    out["dark-100"] = capture(app, f"{name}-dark-100")
    zoom(app, 10)
    out["dark-200"] = capture(app, f"{name}-dark-200")
    appearance(app, "Light")
    out["light-200"] = capture(app, f"{name}-light-200")
    zoom(app, -10)
    appearance(app, "Match System")
    return out


def focus_walk(app, steps=8):
    """Labels reached by Tab from the step heading, proving keyboard reach of a step."""
    return [((f or {}).get("role"), ((f or {}).get("label") or "")[:50]) for f in app.tab_walk(steps)]


def current_step(tree):
    return next(((n.get("label") or "") for n in tree if n.get("current") == "step"), None)


def switcher(tree):
    return fx.App.node(tree, "AXPopUpButton", "Switch tenant") or {}


def open_switcher(app):
    app.menu("Go", "Switch Tenant…")
    t, ok = app.wait_for(lambda t: any(n.get("role") == "AXGroup" and (n.get("label") or "") == "Switch tenant" for n in t), 8)
    time.sleep(0.4)
    return app.tree() if ok else t


def tenant_rows(tree):
    return [n.get("label") for n in tree if n.get("role") in ("AXButton", "AXCheckBox") and "TSG" in (n.get("label") or "")]


def dialog(tree):
    return next((n for n in tree if (n.get("label") or "") == "Add a tenant" and n.get("subrole") in ("AXDialog", "AXApplicationDialog")), None)


def run():
    global NATIVE
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")
    if sha256(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c29-acceptance-")
    FINAL = NATIVE
    NATIVE = NATIVE + ".staging"
    if os.path.isdir(NATIVE):
        shutil.rmtree(NATIVE)
    os.makedirs(NATIVE)
    if os.path.isdir(CAPTURES):
        shutil.rmtree(CAPTURES)
    app = fx.App(bundle, work, LOG)
    global _APP
    _APP = app
    app.quit()
    fx.clean_fixture_root()
    endpoint = fx.ScanEndpoint(os.path.join(work, "endpoint"), SCAN_KEY)
    endpoint.install_roots()
    endpoint.start()
    mgmt = c8.MgmtEndpoint(os.path.join(work, "endpoint"), os.path.join(work, "mgmt"))
    mgmt.install()
    mgmt.start()
    mgmt.mode()
    os.makedirs(fx.DATA, exist_ok=True)
    obs = {"captures": {}, "walks": {}}

    # ---- N01: a first run in v2 opens the wizard ----
    app.launch()
    pid = app.pid
    app.open_settings()
    press(app, "AXCheckBox", "Use the new interface")
    tree, wiz = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Built on the public APIs") is not None, 12)
    obs["tree_welcome"] = save_tree("welcome", tree)
    obs["welcome"] = {"shown": wiz, "framing": app.has_text(tree, "built entirely on Palo Alto Networks’ public APIs"),
                      "cards": all(app.has_text(tree, c) for c in ("Built on the public APIs", "See the calls as you go", "Learn by doing")),
                      "no_shell": nav(tree, "Destinations") is None,
                      "no_back": fx.App.node(tree, "AXButton", "Back") is None}
    obs["captures"]["welcome"] = matrix(app, "welcome")
    obs["walks"]["welcome"] = focus_walk(app, 4)
    app.launch()
    pid = app.pid
    _, again = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Built on the public APIs") is not None, 12)
    obs["welcome"]["after_relaunch"] = again
    press(app, "AXButton", "Set up a tenant")
    tree, tenant_step = heading(app, "Your tenant")
    f = fx.App.focused(tree) or {}
    obs["tenant_step"] = {"shown": tenant_step, "heading_focused": f.get("role") == "AXHeading" and f.get("label") == "Your tenant",
                          "rail_current": (current_step(tree) or "").endswith("Tenant"),
                          "continue_names_missing": (fx.App.node(tree, "AXButton", "Create the tenant to continue") or {}).get("enabled") is False}

    # ---- N02: validation, the draft, credentials on the draft, editing the draft ----
    app.type_into("AXTextField", "TSG ID", TSG[:9])
    press(app, "AXButton", "Create tenant")
    tree, invalid = app.wait_for(lambda t: app.has_text(t, "Enter a display name.") and app.has_text(t, "The TSG ID is exactly 10 digits."), 8)
    obs["tree_invalid"] = save_tree("tenant-invalid", tree)
    obs["validation"] = {"messages": invalid, "no_draft_yet": not any("TSG" in x and "·" in x for x in app.texts(tree))}
    app.type_into("AXTextField", "Display name", "Northwind Health")
    app.type_into("AXTextField", "TSG ID", TSG)
    press(app, "AXButton", "Create tenant")
    tree, drafted = app.wait_for(lambda t: app.has_text(t, "Northwind Health is drafted. Add its scan key below."), 10)
    obs["drafted"] = {"status": drafted, "rail_context": any(x == f"Northwind Health · Production · TSG {TSG}" for x in app.texts(tree)),
                      "save_changes_offered": fx.App.node(tree, "AXButton", "Save changes") is not None,
                      "continue_needs_key": (fx.App.node(tree, "AXButton", "Save the scan key to continue") or {}).get("enabled") is False}
    app.type_into("AXTextField", "Scan API key", SCAN_KEY, secure=True)
    press(app, "AXButton", "Save scan key")
    _, saved = app.wait_for(lambda t: app.has_text(t, "Scan key saved in the Keychain.") and fx.App.node(t, "AXButton", "Replace scan key") is not None, 12)
    time.sleep(0.8)  # the accessibility tree fills in behind the render
    tree = app.tree()
    obs["tree_key_saved"] = save_tree("tenant-key-saved", tree)
    obs["key"] = {"saved": saved, "continue_enabled": (fx.App.node(tree, "AXButton", "Continue") or {}).get("enabled") is not False,
                  "remove_offered": fx.App.node(tree, "AXButton", "Remove scan key") is not None}
    app.type_into("AXTextField", "Client ID", CLIENT_ID)
    app.type_into("AXTextField", "Client secret", CLIENT_SECRET, secure=True)
    press(app, "AXButton", "Save management client")
    tree, client_saved = app.wait_for(lambda t: app.has_text(t, "Management client saved.") and fx.App.node(t, "AXButton", "Replace management client") is not None, 12)
    obs["client_saved"] = client_saved
    # Gateway section: workspaces are read through the client, one is chosen, its key saved.
    press(app, "AXCheckBox", "Set up an AI Gateway workspace now")
    press(app, "AXButton", "Read workspaces")
    tree, gw_read = app.wait_for(lambda t: fx.App.node(t, "AXPopUpButton", "Workspace") is not None or app.has_text(t, "No workspace was readable in this tenant."), 15)
    obs["tree_gateway_section"] = save_tree("tenant-gateway-section", tree)
    obs["gateway_section"] = {"read": gw_read, "chooser": fx.App.node(tree, "AXPopUpButton", "Workspace") is not None, "requests_after_list": len(mgmt.requests()),
                              "popups": [n.get("label") for n in tree if n.get("role") == "AXPopUpButton"], "notes": [x for x in app.texts(tree) if "workspace" in x.lower()][:6]}
    if obs["gateway_section"]["chooser"]:
        press(app, "AXButton", "Use this workspace")
        tree, chosen = app.wait_for(lambda t: any(x.startswith("Workspace ") and x.endswith(" chosen.") for x in app.texts(t)), 12)
        obs["gateway_section"]["chosen"] = chosen
        app.type_into("AXTextField", "Inference endpoint", mgmt.origin + "/v1")
        app.type_into("AXTextField", "Workspace inference key", "SYNTHETIC-C29-WORKSPACE-KEY-qrstuv", secure=True)
        press(app, "AXButton", "Save workspace key")
        tree, wk = app.wait_for(lambda t: app.has_text(t, "Workspace key saved in the Keychain."), 12)
        obs["gateway_section"]["key_saved"] = wk
        obs["tree_gateway_key"] = save_tree("tenant-gateway-key", tree)
    else:
        obs["gateway_section"]["chosen"] = obs["gateway_section"]["key_saved"] = False
    press(app, "AXCheckBox", "Set up an AI Gateway workspace now")
    # Editing the draft: the profile stays editable; the rail follows.
    app.type_into("AXTextField", "Display name", "Northwind Health EU")
    press(app, "AXButton", "Save changes")
    tree, edited = app.wait_for(lambda t: app.has_text(t, "Tenant details saved.") and any(x == f"Northwind Health EU · Production · TSG {TSG}" for x in app.texts(t)), 10)
    obs["edited"] = {"saved": edited, "key_kept": fx.App.node(tree, "AXButton", "Replace scan key") is not None}
    obs["tree_tenant_edited"] = save_tree("tenant-edited", tree)
    obs["captures"]["tenant"] = matrix(app, "tenant")
    obs["walks"]["tenant"] = focus_walk(app, 8)
    # Secret canary over everything saved so far.
    obs["secrets"] = {"in_trees": [rel(p) for p in glob.glob(os.path.join(NATIVE, "tree-*.json")) if SCAN_KEY in open(p).read() or CLIENT_SECRET in open(p).read()],
                      "in_files": holding(fx.DATA, SCAN_KEY, skip=(".keychain-db",)) + holding(fx.DATA, CLIENT_SECRET, skip=(".keychain-db",)) + holding(fx.DATA, "SYNTHETIC-C29-WORKSPACE-KEY-qrstuv", skip=(".keychain-db",))}
    press(app, "AXButton", "Continue")

    # ---- N03: products, verify fail → fix → pass → stale → clean ----
    tree, products = heading(app, "What to read")
    obs["tree_products"] = save_tree("products", tree)
    boxes = [n for n in tree if n.get("role") == "AXCheckBox"]
    runtime = next((n for n in boxes if "AI Runtime Security" in (n.get("label") or "")), None)
    ais = next((n for n in boxes if "Agentic Identity" in (n.get("label") or "")), None)
    aes = next((n for n in boxes if "Agentic Endpoint" in (n.get("label") or "")), None)
    obs["products"] = {"shown": products,
                       "runtime_always_on": bool(runtime) and runtime.get("enabled") is False and str(runtime.get("value")) in ("1", "True", "true"),
                       "unavailable_disabled": bool(ais and aes) and ais.get("enabled") is False and aes.get("enabled") is False,
                       "unavailable_text": sum(1 for x in app.texts(tree) if x == "Not available in this release") >= 2,
                       "licence_note": app.has_text(tree, "Licence state is not readable through the public APIs")}
    press(app, "AXCheckBox", "AI Gateway")
    time.sleep(0.6)
    obs["captures"]["products"] = matrix(app, "products")
    # Back to Tenant: replace the client with a secret the service rejects.
    press(app, "AXButton", "Back")
    heading(app, "Your tenant")
    app.type_into("AXTextField", "Client ID", CLIENT_ID)
    app.type_into("AXTextField", "Client secret", "WRONG-SYNTHETIC-SECRET-000000", secure=True)
    press(app, "AXButton", "Replace management client")
    app.wait_for(lambda t: app.has_text(t, "Management client saved."), 12)
    press(app, "AXButton", "Continue")
    heading(app, "What to read")
    obs["walks"]["products"] = focus_walk(app, 6)
    press(app, "AXButton", "Continue")
    tree, _ = heading(app, "Verify")
    time.sleep(0.6)
    tree = app.tree()
    obs["tree_verify_planned"] = save_tree("verify-planned", tree)
    obs["verify_planned"] = sum(1 for x in app.texts(tree) if x == "Not run yet") >= 3 and (fx.App.node(tree, "AXButton", "Continue") or {}).get("enabled") is False
    # The fail path: the client was replaced with a secret the service rejects
    # (an optional service failing: continuable as a warning, or fixable).
    press(app, "AXButton", "Run the checks")
    tree, failed = app.wait_for(lambda t: app.has_text(t, "An optional check failed.") and fx.App.node(t, "AXButton", "Fix and verify again") is not None, 30)
    obs["tree_verify_fail"] = save_tree("verify-fail", tree)
    obs["verify_fail"] = {"failed": failed, "not_accepted": app.has_text(tree, "The management client was not accepted."),
                          "continue_with_warnings_offered": (fx.App.node(tree, "AXButton", "Continue with warnings") or {}).get("enabled") is not False and fx.App.node(tree, "AXButton", "Continue with warnings") is not None,
                          "scan_stored_not_pass": app.has_text(tree, "Stored") and not any(x == "Pass" for x in app.texts(tree)),
                          "unavailable_rows": sum(1 for x in app.texts(tree) if x == "Not available") >= 2}
    press(app, "AXButton", "Fix and verify again")
    tree, _ = heading(app, "Your tenant")
    obs["fix_returns_to_editable_tenant"] = fx.App.node(tree, "AXButton", "Save changes") is not None and fx.App.node(tree, "AXButton", "Replace management client") is not None
    # Correct the client: a fresh binding must authenticate with the new secret.
    app.type_into("AXTextField", "Client ID", CLIENT_ID)
    app.type_into("AXTextField", "Client secret", CLIENT_SECRET, secure=True)
    press(app, "AXButton", "Replace management client")
    app.wait_for(lambda t: app.has_text(t, "Management client saved."), 12)
    # The client change retired the draft's context and the choice under it:
    # reading the workspaces again restores the remembered one.
    tree = app.tree()
    if str((fx.App.node(tree, "AXCheckBox", "Set up an AI Gateway workspace now") or {}).get("value")) not in ("1", "True", "true"):
        press(app, "AXCheckBox", "Set up an AI Gateway workspace now")
    press(app, "AXButton", "Read workspaces")
    tree, rechosen = app.wait_for(lambda t: any(x.startswith("Workspace ") and x.endswith(" chosen.") for x in app.texts(t)) or fx.App.node(t, "AXButton", "Use this workspace") is not None, 15)
    if not any(x.startswith("Workspace ") and x.endswith(" chosen.") for x in app.texts(tree)):
        press(app, "AXButton", "Use this workspace")
        tree, rechosen = app.wait_for(lambda t: any(x.startswith("Workspace ") and x.endswith(" chosen.") for x in app.texts(t)), 12)
    obs["workspace_rechosen_after_fix"] = rechosen
    press(app, "AXButton", "Continue")
    heading(app, "What to read")
    press(app, "AXButton", "Continue")
    tree, _ = heading(app, "Verify")
    # The client change retired the run: a fresh "Run the checks" is offered (the failed
    # run itself offered Verify again beside Fix and verify again).
    obs["rerun_offered_after_fail"] = (fx.App.node(obs_tree_fail, "AXButton", "Verify again") is not None) if (obs_tree_fail := json.load(open(os.path.join(NATIVE, "tree-verify-fail.json")))) else False
    press(app, "AXButton", "Run the checks" if fx.App.node(tree, "AXButton", "Run the checks") else "Verify again")
    tree, passed = app.wait_for(lambda t: app.has_text(t, "The management client can read this tenant.") and (app.has_text(t, "Checks finished; nothing failed.") or app.has_text(t, "Checks finished with warnings.")), 30)
    obs["tree_verify_pass"] = save_tree("verify-pass", tree)
    obs["verify_pass"] = {"passed": passed, "scan_stored": app.has_text(tree, "the first scan proves it"),
                          "gateway_line": next((x for x in app.texts(tree) if x.startswith("Workspace ") and "readable" in x), None),
                          "gateway_pass": any(n.get("label") == "Pass" for n in tree if n.get("role") == "AXStaticText") and next((x for x in app.texts(tree) if x.startswith("Workspace ") and "route(s)" in x), None) is not None,
                          "continue_enabled": any((fx.App.node(tree, "AXButton", l) or {}).get("enabled") is not False and fx.App.node(tree, "AXButton", l) for l in ("Continue", "Continue with warnings")),
                          "requests": len(mgmt.requests())}
    obs["captures"]["verify"] = matrix(app, "verify")
    obs["walks"]["verify"] = focus_walk(app, 6)
    # Stale: an edit on Products invalidates the run until it is repeated.
    press(app, "AXButton", "Back")
    heading(app, "What to read")
    press(app, "AXCheckBox", "AI Gateway")
    time.sleep(0.5)
    press(app, "AXButton", "Continue")
    tree, stale = app.wait_for(lambda t: app.has_text(t, "The inputs changed since this run. Verify again before continuing."), 10)
    obs["stale"] = {"shown": stale, "continue_disabled": (fx.App.node(tree, "AXButton", "Continue") or {}).get("enabled") is False,
                    "done_unreachable": (next((n for n in tree if n.get("role") == "AXButton" and (n.get("label") or "").endswith("Done")), {}) or {}).get("enabled") is False}
    obs["tree_verify_stale"] = save_tree("verify-stale", tree)
    press(app, "AXButton", "Verify again")
    tree, clean = app.wait_for(lambda t: app.has_text(t, "Checks finished; nothing failed.") and (fx.App.node(t, "AXButton", "Continue") or {}).get("enabled") is not False, 30)
    obs["stale"]["clean_after_rerun"] = clean
    press(app, "AXButton", "Continue")

    # ---- N04: starter cases, Academy, done, relaunch ----
    tree, packs = heading(app, "Starter cases")
    pack_box = next((n for n in tree if n.get("role") == "AXCheckBox" and (n.get("label") or "").startswith("AI Runtime Security ·")), None)
    obs["packs"] = {"shown": packs, "runtime_pack": (pack_box or {}).get("label"), "checked": str((pack_box or {}).get("value")) in ("1", "True", "true")}
    obs["captures"]["packs"] = matrix(app, "packs")
    obs["walks"]["packs"] = focus_walk(app, 4)
    press(app, "AXButton", "Continue")
    tree, academy = heading(app, "Academy account")
    obs["academy"] = {"shown": academy, "sync_copy": app.has_text(tree, "syncs your Academy progress only"), "sign_in_offered": fx.App.node(tree, "AXButton", "Sign in to Academy") is not None}
    obs["captures"]["academy"] = matrix(app, "academy")
    obs["walks"]["academy"] = focus_walk(app, 4)
    press(app, "AXButton", "Skip for now")
    tree, done = heading(app, "Ready")
    obs["done"] = {"shown": done, "summary": app.has_text(tree, "Northwind Health EU is ready to become a tenant."),
                   "products_line": next((x for x in app.texts(tree) if x.startswith("Products:")), None)}
    obs["captures"]["done"] = matrix(app, "done")
    obs["walks"]["done"] = focus_walk(app, 4)
    press(app, "AXButton", "Open Prisma AIRS")
    tree, home = app.wait_for(lambda t: nav(t, "Destinations") is not None and fx.App.node(t, "AXHeading", "Home") is not None, 20)
    obs["home"] = {"shown": home, "switcher_names_tenant": "Northwind Health EU" in str(switcher(tree).get("label") or "")}
    obs["captures"]["home"] = {"light-100": capture(app, "home-after-wizard")}
    app.menu("Go", "Test Cases")
    tree, lib = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Test case library") is not None, 12)
    time.sleep(1.5)
    tree = app.tree()
    obs["library"] = {"shown": lib, "rows": sum(1 for n in tree if n.get("role") == "AXRow"), "mentions_injection": any("injection" in x.lower() for x in app.texts(tree))}
    obs["tree_library"] = save_tree("library", tree)
    app.launch()
    pid = app.pid
    tree, no_wizard = app.wait_for(lambda t: nav(t, "Destinations") is not None and fx.App.node(t, "AXHeading", "Home") is not None, 20)
    obs["relaunch_home"] = no_wizard and fx.App.node(tree, "AXHeading", "Built on the public APIs") is None

    # ---- N05: the add-tenant sheet ----
    t_open = open_switcher(app)
    before_rows = tenant_rows(t_open)
    active_before = str(switcher(t_open).get("label") or "")
    press(app, "AXButton", "Add a tenant")
    tree, sheet = app.wait_for(lambda t: dialog(t) is not None, 10)
    obs["tree_sheet"] = save_tree("add-tenant-sheet", tree)
    steps = [n.get("label") for n in tree if n.get("role") == "AXButton" and (n.get("label") or "")[:1].isdigit()]
    rail = next((n for n in tree if n.get("subrole") == "AXLandmarkNavigation" and n.get("label") == "Setup steps"), None)
    obs["sheet"] = {"shown": sheet, "steps": steps, "no_welcome_or_academy": not any("Welcome" in (s or "") or "Academy" in (s or "") for s in steps),
                    "tenant_heading": fx.App.node(tree, "AXHeading", "Your tenant") is not None, "rail_width": rail and rail["size"][0],
                    "no_back": fx.App.node(tree, "AXButton", "Back") is None}
    contained = []
    for _ in range(4):
        app.keycode(48)
        t = app.tree()
        f = fx.App.focused(t) or {}
        d = dialog(t)
        contained.append(bool(d and f.get("pos") and d["pos"][0] <= f["pos"][0] <= d["pos"][0] + d["size"][0] and d["pos"][1] <= f["pos"][1] <= d["pos"][1] + d["size"][1]))
    obs["sheet"]["contained"] = all(contained)
    obs["captures"]["sheet"] = matrix(app, "add-tenant-sheet")
    # Cancel before a draft: nothing to discard; the shell is unchanged.
    press(app, "AXButton", "Cancel")
    app.wait_for(lambda t: dialog(t) is None, 8)
    t2 = open_switcher(app)
    obs["sheet"]["rows_after_cancel"] = tenant_rows(t2)
    # Draft, key it, Cancel: the draft and its key go; the shell is unchanged.
    press(app, "AXButton", "Add a tenant")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Your tenant") is not None, 10)
    app.type_into("AXTextField", "Display name", "Meridian Retail")
    app.type_into("AXTextField", "TSG ID", "1001464285")
    press(app, "AXButton", "Create tenant")
    tree, _ = app.wait_for(lambda t: app.has_text(t, "Meridian Retail is drafted."), 10)
    obs["sheet"]["optional_key_copy"] = app.has_text(tree, "Optional here; without it scans stay unavailable")
    items_before_second = fx.keychain_accounts(c8.SCAN_SERVICE)
    app.type_into("AXTextField", "Scan API key", "SYNTHETIC-C29-SECOND-KEY-zyxwvu", secure=True)
    press(app, "AXButton", "Save scan key")
    tree, second_saved = app.wait_for(lambda t: app.has_text(t, "Scan key saved in the Keychain."), 12)
    obs["sheet"]["second_key_saved"] = second_saved
    time.sleep(1.0)
    items_with_second = fx.keychain_accounts(c8.SCAN_SERVICE)
    obs["sheet"]["keychain_items_before_second"] = items_before_second
    obs["sheet"]["keychain_items_with_second"] = items_with_second
    # Minimum size: the sheet's actions stay reachable (scrolled into view on focus).
    set_window_size(pid, 900, 600)
    app.ax("focus", "AXButton", "Cancel")
    time.sleep(0.5)
    t_small = app.tree()
    obs["sheet"]["cancel_visible_at_900x600"] = inside(window_frame(pid), fx.App.node(t_small, "AXButton", "Cancel"))
    appearance(app, "Light")
    obs["captures"]["sheet-900"] = {"light-100": capture(app, "add-tenant-sheet-900x600")}
    appearance(app, "Match System")
    set_window_size(pid, 1280, 820)
    press(app, "AXButton", "Cancel")
    app.wait_for(lambda t: dialog(t) is None, 8)
    time.sleep(0.8)
    t3 = open_switcher(app)
    obs["sheet"]["rows_after_keyed_cancel"] = tenant_rows(t3)
    obs["sheet"]["active_after_keyed_cancel"] = str(switcher(t3).get("label") or "") == active_before
    obs["secrets"]["second_key_in_files_after_cancel"] = holding(fx.DATA, "SYNTHETIC-C29-SECOND-KEY-zyxwvu", skip=(".keychain-db",))
    # The draft's Keychain record itself is removed by the cleanup (attributes only; no secret is read).
    removed = False
    for _ in range(30):
        items_after_cancel = fx.keychain_accounts(c8.SCAN_SERVICE)
        removed = items_after_cancel == items_before_second
        if removed:
            break
        time.sleep(0.5)
    obs["sheet"]["keychain_items_after_cancel"] = items_after_cancel
    obs["sheet"]["keychain_record_removed_after_cancel"] = removed and len(items_with_second) == len(items_before_second) + 1
    # Draft again, no key (optional), verify with the warning, Finish: one commit, one switch.
    press(app, "AXButton", "Add a tenant")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Your tenant") is not None, 10)
    app.type_into("AXTextField", "Display name", "Meridian Retail")
    app.type_into("AXTextField", "TSG ID", "1001464285")
    press(app, "AXButton", "Create tenant")
    app.wait_for(lambda t: app.has_text(t, "Meridian Retail is drafted."), 10)
    press(app, "AXButton", "Continue")
    heading(app, "What to read")
    press(app, "AXButton", "Continue")
    heading(app, "Verify")
    press(app, "AXButton", "Run the checks")
    tree, warned = app.wait_for(lambda t: app.has_text(t, "Checks finished with warnings.") and fx.App.node(t, "AXButton", "Continue with warnings") is not None, 20)
    obs["sheet"]["no_key_is_warning"] = warned and app.has_text(tree, "No scan key saved; scans stay unavailable")
    press(app, "AXButton", "Continue with warnings")
    heading(app, "Starter cases")
    press(app, "AXButton", "Skip")
    heading(app, "Ready")
    press(app, "AXButton", "Finish")
    tree, closed = app.wait_for(lambda t: dialog(t) is None and app.has_text(t, "Meridian Retail is set up and active."), 15)
    obs["sheet"]["finished"] = closed
    obs["sheet"]["active_after_finish"] = "Meridian Retail" in str(switcher(app.tree()).get("label") or "")
    t4 = open_switcher(app)
    obs["sheet"]["rows_after_finish"] = tenant_rows(t4)
    obs["tree_switcher_after"] = save_tree("switcher-after-sheet", t4)
    app.keycode(53)
    app.quit()
    endpoint.stop()
    mgmt.stop()

    run_path = os.path.join(NATIVE, "observations.json")
    with open(run_path, "w") as f:
        json.dump(json.loads(json.dumps({"observations": obs, "log": LOG}).replace(rel(NATIVE), rel(FINAL))), f, indent=1)
    run_rel = rel(run_path)
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C29"]}
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [e for e in evidence if e]})

    caps = lambda name: list(obs["captures"].get(name, {}).values())  # noqa: E731
    w, ts = obs["welcome"], obs["tenant_step"]
    add("C29-N01", w["shown"] and w["framing"] and w["cards"] and w["no_shell"] and w["no_back"] and w["after_relaunch"] and ts["shown"] and ts["heading_focused"] and ts["rail_current"] and ts["continue_names_missing"],
        f"wizard on first v2 launch {w['shown']} (framing {w['framing']}, three cards {w['cards']}, no shell {w['no_shell']}, no Back {w['no_back']}), again after relaunch {w['after_relaunch']}; Set up a tenant → Tenant heading focused {ts['heading_focused']}, rail current {ts['rail_current']}, Continue names what is missing {ts['continue_names_missing']}",
        [run_rel, obs["tree_welcome"]] + caps("welcome"))
    v, d, k, e, g, sc = obs["validation"], obs["drafted"], obs["key"], obs["edited"], obs["gateway_section"], obs["secrets"]
    add("C29-N02", v["messages"] and v["no_draft_yet"] and d["status"] and d["rail_context"] and d["save_changes_offered"] and d["continue_needs_key"] and k["saved"] and k["continue_enabled"] and k["remove_offered"] and obs["client_saved"] and g["read"] and g["chosen"] and g["key_saved"] and e["saved"] and e["key_kept"] and not sc["in_trees"] and not sc["in_files"],
        f"invalid fields: messages {v['messages']}, nothing drafted {v['no_draft_yet']}; valid create → draft {d['status']} with rail context {d['rail_context']}, profile stays editable {d['save_changes_offered']}, Continue says 'Save the scan key to continue' {d['continue_needs_key']}; key saved on the draft {k['saved']} (Replace/Remove offered {k['remove_offered']}), Continue enabled {k['continue_enabled']}; management client saved {obs['client_saved']}; Gateway section read workspaces through the client {g['read']}, chose one {g['chosen']} and saved its key {g['key_saved']}; edited name saved {e['saved']} keeping the key {e['key_kept']}; secrets in AX trees {sc['in_trees']}, in data files outside the Keychain {sc['in_files']}",
        [run_rel, obs["tree_invalid"], obs["tree_key_saved"], obs["tree_tenant_edited"]] + caps("tenant"))
    p, vf, vp, st = obs["products"], obs["verify_fail"], obs["verify_pass"], obs["stale"]
    add("C29-N03", p["shown"] and p["runtime_always_on"] and p["unavailable_disabled"] and p["unavailable_text"] and p["licence_note"] and obs["verify_planned"] and vf["failed"] and vf["not_accepted"] and vf["continue_with_warnings_offered"] and vf["scan_stored_not_pass"] and vf["unavailable_rows"] and obs["fix_returns_to_editable_tenant"] and obs["rerun_offered_after_fail"] and vp["passed"] and vp["scan_stored"] and vp["gateway_pass"] and vp["continue_enabled"] and st["shown"] and st["continue_disabled"] and st["done_unreachable"] and st["clean_after_rerun"],
        f"products: Runtime always on {p['runtime_always_on']}, AIS/AES disabled {p['unavailable_disabled']} with the notice {p['unavailable_text']}, licence note {p['licence_note']}; verify planned rows with Continue closed {obs['verify_planned']}; a rejected client: optional failure {vf['failed']} ('not accepted' {vf['not_accepted']}), Continue with warnings offered {vf['continue_with_warnings_offered']}, scan key Stored not Pass {vf['scan_stored_not_pass']}, AIS/AES listed Not available {vf['unavailable_rows']}; Fix and verify again lands on the editable tenant {obs['fix_returns_to_editable_tenant']} where the client is replaced, and the failed run still offers Verify again {obs['rerun_offered_after_fail']}; corrected client: management Pass {vp['passed']}, chosen workspace prerequisites Pass {vp['gateway_pass']} ('{vp['gateway_line']}'), Continue open {vp['continue_enabled']} ({vp['requests']} synthetic requests); changing products afterwards: stale {st['shown']}, Continue closed {st['continue_disabled']}, Done unreachable {st['done_unreachable']}, clean after a rerun {st['clean_after_rerun']}",
        [run_rel, obs["tree_products"], obs["tree_verify_fail"], obs["tree_verify_pass"], obs["tree_verify_stale"]] + caps("products") + caps("verify"))
    pk, ac, dn, hm, lb = obs["packs"], obs["academy"], obs["done"], obs["home"], obs["library"]
    add("C29-N04", pk["shown"] and bool(pk["runtime_pack"]) and pk["checked"] and ac["shown"] and ac["sync_copy"] and ac["sign_in_offered"] and dn["shown"] and dn["summary"] and hm["shown"] and hm["switcher_names_tenant"] and lb["shown"] and lb["rows"] > 0 and obs["relaunch_home"],
        f"starter pack per product with count '{pk['runtime_pack']}' checked {pk['checked']}; Academy step through the account component (sign-in offered {ac['sign_in_offered']}, sync disclosure {ac['sync_copy']}); Done summary {dn['summary']} ({dn['products_line']}); Open Prisma AIRS → Home naming the tenant {hm['switcher_names_tenant']}; Test Cases lists {lb['rows']} rows; relaunch shows Home, not the wizard: {obs['relaunch_home']}",
        [run_rel, obs["tree_library"]] + caps("packs") + caps("academy") + caps("done") + caps("home"))
    s = obs["sheet"]
    add("C29-N05", s["shown"] and s["no_welcome_or_academy"] and s["tenant_heading"] and (s["rail_width"] or 0) == 244 and s["no_back"] and s["contained"] and s["rows_after_cancel"] == before_rows and s["optional_key_copy"] and s["second_key_saved"] and s["rows_after_keyed_cancel"] == before_rows and s["active_after_keyed_cancel"] and not obs["secrets"]["second_key_in_files_after_cancel"] and s["keychain_record_removed_after_cancel"] and s["no_key_is_warning"] and s["finished"] and s["active_after_finish"] and len(s["rows_after_finish"]) == 2,
        f"sheet as a dialog {s['shown']} with steps {s['steps']} (rail {s['rail_width']}px, no Back {s['no_back']}); Tab stayed inside {s['contained']}; Cancel before a draft left the tenants {s['rows_after_cancel']}; a keyed draft left on Cancel: tenants {s['rows_after_keyed_cancel']}, active unchanged {s['active_after_keyed_cancel']}, no plaintext in files {not obs['secrets']['second_key_in_files_after_cancel']}, its fixture-Keychain record removed by the cleanup {s['keychain_record_removed_after_cancel']} (items {len(s['keychain_items_before_second'])} → {len(s['keychain_items_with_second'])} → {len(s['keychain_items_after_cancel'])}); a draft without a key verifies with a warning {s['no_key_is_warning']}; Finish committed it once {s['finished']}, made it active {s['active_after_finish']} and listed {len(s['rows_after_finish'])} tenants",
        [run_rel, obs["tree_sheet"], obs["tree_switcher_after"]] + caps("sheet") + caps("sheet-900"))
    all_caps = {k: v for k, v in obs["captures"].items()}
    complete = all(len(all_caps.get(n, {})) == 4 and all(all_caps[n].values()) for n in ("welcome", "tenant", "products", "verify", "packs", "academy", "done", "sheet"))
    walks_ok = all(len([x for x in obs["walks"][n] if x[0]]) >= 2 for n in obs["walks"]) and set(obs["walks"]) >= {"welcome", "tenant", "products", "verify", "packs", "academy", "done"}
    add("C29-N06", complete and walks_ok and s["cancel_visible_at_900x600"],
        f"every step captured light/dark at 100%/200% {complete}; Tab reaches controls on each step {walks_ok} ({ {n: len(w) for n, w in obs['walks'].items()} }); at 900×600 the sheet's Cancel is inside the window once focused {s['cancel_visible_at_900x600']}",
        [run_rel] + [c for n in all_caps for c in all_caps[n].values()])

    hw = sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1", "subject": "C29", "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (System Events and the ax helper) with fixture WebView self-capture; tools/native-acceptance/acceptance_c29.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "Pixels come from the fixture build's own WKWebView snapshot; menus and dialogs are verified through System Events and the accessibility tree.",
            "The verify reads run against the synthetic management endpoint (tools/native-acceptance/mgmt_server.py), whose Gateway lists two synthetic workspaces; the fail path replaces the management client with a secret that endpoint rejects, then corrects it.",
            "The Academy step is exercised as skipped; the hosted device flow is the reviewed C04 component and its acceptance.",
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
        print(c["result"].upper(), c["id"], c["observation"][:200])
    return 0 if all(c["result"] == "pass" for c in checks) else 1


if __name__ == "__main__":
    sys.exit(main())
