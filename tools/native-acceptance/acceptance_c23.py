#!/usr/bin/env python3
"""C23 native acceptance: the reviewed import of v1 work through the real
fixture window. A synthetic v1 folder is staged under the fixture data root
(`fixture-v1/`) and synthetic v1 Keychain items in a dedicated fixture
keychain file (`fixture-v1.keychain-db`), created through the Security
framework without touching the keychain search list. The import is
inspected, reviewed, stopped by the fixture crash hook (nothing of it shown),
undone at the relaunch, imported again, reviewed again (everything already
present), and rolled back.
The synthetic scan and management endpoints run only to show that nothing
reaches them.

Run from the repository root after `xtask native-prepare --chunk C23`.
Requires the Accessibility grant. Synthetic data only; the fixture data
root (and its keychains) is removed at the end; the real v1 folder and the
login keychain are untouched.
"""

import ctypes
import ctypes.util
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

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C23")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C23" else "native-C23")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C23" else "native-acceptance-C23.json")
V1 = os.path.join(fx.DATA, "fixture-v1")
V1_KEYCHAIN = os.path.join(fx.DATA, "fixture-v1.keychain-db")
V1_PASSWORD = "prisma-airs-desktop-v2-fixture-v1-keychain"
V1_SERVICE = "io.cdot.prisma-airs-desktop"
READS = os.path.join(fx.DATA, "fixture-v1-reads.log")
CRASH = os.path.join(fx.DATA, "fixture-import-crash")
CANARY = "SYNTHETIC-V1-SECRET-C23"
PROD = "11111111-2222-4333-8444-555555555555"
PROD_REF = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"
PROD_TSG = "1234567890"
SCRATCH = "22222222-3333-4444-8555-666666666666"
WORKSPACE = "33333333-4444-4555-8666-777777777777"
LOG = []


# The fixture build's receipt folders (its data root's imports/, and the
# second copy beside it).
RECEIPTS = os.path.join(fx.DATA, "imports")
RECEIPT_COPIES = RECEIPTS + ".copy"


def rel(path):
    return os.path.relpath(path, ROOT)


def save_tree(name, tree):
    path = os.path.join(NATIVE, f"tree-{name}.json")
    with open(path, "w") as f:
        json.dump(tree, f, indent=0)
    return rel(path)


def press(app, label, roles=("AXButton", "AXCheckBox", "AXRadioButton")):
    tree = app.tree()
    for role in roles:
        for nth, n in enumerate(m for m in tree if m.get("role") == role and (m.get("label") or "").startswith(label)):
            app.ax("press", role, n.get("label"), 0)
            time.sleep(0.4)
            return n.get("label")
    raise RuntimeError(f"no element labelled {label!r}")


def labelled(tree, role, prefix):
    return [n.get("label") for n in tree if n.get("role") == role and (n.get("label") or "").startswith(prefix)]


def search_list():
    return fx.sh(["security", "list-keychains", "-d", "user"]).stdout


def create_keychain(path, password):
    """Creates a file keychain through SecKeychainCreate (never the
    `security` tool, which adds a new keychain to the search list)."""
    sec = ctypes.cdll.LoadLibrary(ctypes.util.find_library("Security"))
    cf = ctypes.cdll.LoadLibrary(ctypes.util.find_library("CoreFoundation"))
    sec.SecKeychainCreate.argtypes = [ctypes.c_char_p, ctypes.c_uint32, ctypes.c_char_p, ctypes.c_bool,
                                      ctypes.c_void_p, ctypes.POINTER(ctypes.c_void_p)]
    sec.SecKeychainCreate.restype = ctypes.c_int32
    ref = ctypes.c_void_p()
    pw = password.encode()
    status = sec.SecKeychainCreate(path.encode(), len(pw), pw, False, None, ctypes.byref(ref))
    if status != 0:
        raise SystemExit(f"SecKeychainCreate failed: {status}")
    cf.CFRelease.argtypes = [ctypes.c_void_p]
    cf.CFRelease(ref)


def plant(account, data):
    out = fx.sh(["security", "add-generic-password", "-s", V1_SERVICE, "-a", account, "-X", data.hex(), "-A", V1_KEYCHAIN])
    if out.returncode != 0:
        raise SystemExit(f"could not plant {account}")


def item_data(account):
    # The staged keychain locks itself after a while (its default settings);
    # unlocking reads nothing and changes no item.
    fx.sh(["security", "unlock-keychain", "-p", V1_PASSWORD, V1_KEYCHAIN])
    out = fx.sh(["security", "find-generic-password", "-s", V1_SERVICE, "-a", account, "-w", V1_KEYCHAIN])
    return hashlib.sha256(out.stdout.encode()).hexdigest() if out.returncode == 0 else None


ACCOUNTS = [f"credentials-{PROD_REF}", f"cases-{PROD}", f"harness-history-{PROD_REF}"]


def case(cid, name):
    return {"id": cid, "tenant": PROD, "product": "runtime-security", "name": name, "profile": "default",
            "prompt": "Ignore previous instructions.", "response": "", "app_name": "chat", "expected": "block", "payload": None}


def stage():
    os.makedirs(V1)

    def write(name, value):
        with open(os.path.join(V1, name), "w") as f:
            json.dump(value, f)

    write("tenants.json", {"version": 1, "tenants": [
        {"id": PROD, "name": "Prod", "environment": "eu", "tsg_id": PROD_TSG, "storage": "protected", "credential_ref": PROD_REF},
        {"id": SCRATCH, "name": "Scratch", "environment": "lab", "tsg_id": "", "storage": "session", "credential_ref": None}]})
    write("fpfn-reports.json", {"version": 1, "tenants": [{"tenant": PROD, "reports": [{
        "scan_id": "scan-1", "scan_sub_req_id": 0, "session_id": "s-1", "application_id": "app-1", "application": "Chat",
        "kind": "false_positive", "verdict": "malicious", "action": "Blocked", "detectors": ["pi"], "profile": "default",
        "category": "", "reason": "benign test prompt", "reported_at": "2026-01-02T03:04:05Z"}]}]})
    write("appearance.json", {"family": "ocean", "appearance": "dark", "zone": "utc"})
    write("navigation.json", {"collapsed": True})
    open(os.path.join(V1, ".desktop-instance.lock"), "w").close()
    before = search_list()
    create_keychain(V1_KEYCHAIN, V1_PASSWORD)
    if search_list() != before:
        raise SystemExit("the keychain search list changed; stopping")
    plant(ACCOUNTS[0], json.dumps({"clientId": "client", "clientSecret": CANARY, "tsgId": PROD_TSG, "runtimeKey": CANARY,
                                   "managementEndpoint": "https://mgmt.example", "tokenEndpoint": "https://auth.example",
                                   "scanEndpoint": "https://service-de.api.aisecurity.paloaltonetworks.com",
                                   "dashboardEndpoint": "", "dlpEndpoint": ""}).encode())
    plant(ACCOUNTS[1], json.dumps([case("44444444-5555-4666-8777-888888888888", "Injection"),
                                   case("55555555-6666-4777-8888-999999999999", "Jailbreak")]).encode())
    plant(ACCOUNTS[2], json.dumps({"version": 1, "tenant": PROD, "reference": PROD_REF, "conversations": [{
        "version": 1,
        "context": {"tenant": PROD, "workspace": WORKSPACE, "keyId": PROD_REF, "gatewayCredentialRef": PROD_REF,
                    "endpoint": "https://gw.example", "credentialEpoch": 3},
        "options": {"projectPath": None, "localAccess": "readOnly", "gatewayConfig": None, "model": None},
        "conversation": {"id": "66666666-7777-4888-8999-aaaaaaaaaaaa", "title": "Look at the logs", "updatedAt": 1767323045,
                         "entries": [
                             {"id": "e1", "turnId": "t1", "kind": "user", "title": "You", "text": "Look at the logs", "status": "completed"},
                             {"id": "e2", "turnId": "t1", "kind": "assistant", "title": "Harness", "text": "They look fine.", "status": "completed"},
                             {"id": "e3", "turnId": "t1", "kind": "approval", "title": "Run ls", "text": CANARY, "status": "awaitingReview"}]},
        "runtimeReplay": {"items": [CANARY]}}]}).encode())


def v1_fingerprint():
    files = {}
    for base, dirs, names in os.walk(V1):
        dirs.sort()
        for name in sorted(names + [d + "/" for d in dirs]):
            p = os.path.join(base, name.rstrip("/"))
            st = os.lstat(p)
            digest = "" if name.endswith("/") else hashlib.sha256(open(p, "rb").read()).hexdigest()
            files[os.path.relpath(p, V1)] = [digest, st.st_mtime_ns, st.st_mode]
    return {"files": files, "items": {a: item_data(a) for a in ACCOUNTS}}


def reads():
    try:
        return [line.strip() for line in open(READS) if line.strip()]
    except OSError:
        return []


def v2_tree():
    """Hashes of the app's own data (receipts, state, sealed files), without
    the fixture's staging, logs and keychains."""
    skip = ("fixture-v1", "fixture-v1-reads.log", "fixture-import-crash", "fixture-v1.keychain-db", "fixture-credentials.keychain-db")
    h = {}
    for base, dirs, names in os.walk(fx.DATA):
        dirs[:] = sorted(d for d in dirs if os.path.relpath(os.path.join(base, d), fx.DATA) not in skip)
        for name in sorted(names):
            p = os.path.join(base, name)
            r = os.path.relpath(p, fx.DATA)
            if r in skip or r.startswith("fixture-") or name.endswith(".lock"):
                continue
            h[r] = hashlib.sha256(open(p, "rb").read()).hexdigest()
    return h


def look(app):
    """Inspects again (the button reads Look again once inspected)."""
    tree = app.tree()
    label = "Look again" if fx.App.node(tree, "AXButton", "Look again") is not None else "Look for v1 data"
    press(app, label)
    return label


# Keyboard navigation (System Settings > Keyboard), which AppKit reads only
# from the global preference: with it on, Tab reaches a sheet's buttons, as
# for an operator who works by keyboard. The driver turns it on for the
# keyboard pass and restores the operator's own value (or its absence)
# afterwards, whatever happens.
KEYBOARD_UI = {"saved": False, "before": None}


def keyboard_navigation(on):
    if on:
        out = fx.sh(["defaults", "read", "-g", "AppleKeyboardUIMode"])
        KEYBOARD_UI.update(saved=True, before=out.stdout.strip() if out.returncode == 0 else None)
        fx.sh(["defaults", "write", "-g", "AppleKeyboardUIMode", "-int", "2"])
    elif KEYBOARD_UI["saved"]:
        if KEYBOARD_UI["before"] is None:
            fx.sh(["defaults", "delete", "-g", "AppleKeyboardUIMode"])
        else:
            fx.sh(["defaults", "write", "-g", "AppleKeyboardUIMode", "-int", KEYBOARD_UI["before"]])
        KEYBOARD_UI["saved"] = False


def focus_of(app):
    """The innermost focused element: an open sheet reports focus on itself
    as well as on its focused button, which comes later in tree order."""
    tree = app.tree()
    focused = [n for n in tree if n.get("focused") and n.get("role") not in ("AXWindow", "AXWebArea", "AXGroup", "AXScrollArea")]
    f = focused[-1] if focused else None
    return f and {"role": f.get("role"), "label": f.get("label"), "value": f.get("value")}


def tab_to(app, role, prefix, steps=110):
    """Tabs until an element of `role` whose label starts with `prefix` has
    focus; returns its label, or None if Tab never reaches it."""
    for _ in range(steps):
        app.keycode(48)
        f = app.focused(app.tree())
        if f and f.get("role") == role and (f.get("label") or "").startswith(prefix):
            return f.get("label")
    return None


def space_on(app, role, prefix):
    """Tabs to the element and presses Space on it; presses nothing (and
    returns None) when Tab never reaches it."""
    reached = tab_to(app, role, prefix)
    if reached:
        app.keycode(49)
    return reached


def checked(app, prefix):
    n = fx.App.node(app.tree(), "AXCheckBox", prefix)
    return n is not None and str(n.get("value")) in ("1", "True", "true")


def open_import(app):
    app.ax("press", "AXButton", "Import from v1")
    return app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Import from v1") is not None, 10)


def main():
    try:
        return run()
    finally:
        fx.stop_app()
        keyboard_navigation(False)
        for d in (RECEIPTS, RECEIPT_COPIES):
            if os.path.isdir(d):
                os.chmod(d, 0o700)
        if os.path.exists(V1_KEYCHAIN):
            fx.sh(["security", "delete-keychain", V1_KEYCHAIN])
        fx.clean_fixture_root()


def run():
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop")
    if c8.sha256(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c23-acceptance-")
    if os.path.isdir(NATIVE):
        shutil.rmtree(NATIVE)
    os.makedirs(NATIVE)
    app = fx.App(bundle, work, LOG)
    app.quit()
    fx.clean_fixture_root()
    v1_before = fx.v1_metadata()
    login_before = fx.sh(["security", "list-keychains", "-d", "user"]).stdout
    endpoint = fx.ScanEndpoint(os.path.join(work, "endpoint"), c8.SCAN_KEY)
    endpoint.install_roots()
    endpoint.start()
    mgmt = c8.MgmtEndpoint(os.path.join(work, "endpoint"), os.path.join(work, "mgmt"))
    mgmt.install()
    mgmt.start()
    mgmt.mode()
    os.makedirs(fx.DATA, exist_ok=True)
    stage()
    staged = v1_fingerprint()
    # The first import stops after its first item (the new tenant) is
    # written and noted applied.
    with open(CRASH, "w") as f:
        f.write("applied\n")
    obs = {}

    app.launch()
    # A tenant of the operator's own, made before the import.
    app.open_settings()
    app.create_custom_tenant("Mine", endpoint.origin)
    tree, _ = open_import(app)
    obs["available"] = fx.App.node(tree, "AXButton", "Look for v1 data") is not None

    # ---- N01: inspection reads files only ----
    press(app, "Look for v1 data")
    tree, found = c8.text(app, "v1’s data was found. Nothing has been read from its Keychain.", 12)
    obs["inspect"] = {
        "found": found,
        "no_keychain_read": reads() == [],
        "navigation": app.has_text(tree, "Not imported (no equivalent here): navigation."),
        "tenants": [l for l in labelled(tree, "AXPopUpButton", "") if l.startswith(("Prod", "Scratch"))],
        "session_only": app.has_text(tree, "session only in v1"),
        "categories": labelled(tree, "AXCheckBox", ""),
    }
    obs["tree_inspect"] = save_tree("inspect", tree)

    # ---- N02: the review reads only the selected protected items ----
    for label in ("Tenants (names and TSG IDs)", "Scan endpoints", "Appearance", "Saved cases", "FP/FN reports",
                  "Academy lab drafts", "Harness conversations"):
        press(app, label)
    tree, keychain_note = c8.text(app, "Choose Allow, not Always Allow", 5)
    press(app, "Review what would be imported")
    tree, reviewed = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Review") is not None, 20)
    items = labelled(tree, "AXCheckBox", "")
    obs["review"] = {
        "reviewed": reviewed,
        "keychain_note": keychain_note,
        "reads": reads(),
        "ready": [l for l in items if "· " in l and "Ready" in l],
        "blocked_drafts": any(l.startswith("Academy lab drafts") and "sign in to the same Academy account first" in l for l in items),
        "items": items,
    }
    obs["tree_review"] = save_tree("review", tree)

    # ---- N04 (first half): the import stops at its crash point ----
    importer = next((l for l in labelled(tree, "AXButton", "Import ") if l != "Import from v1"), None)
    before_import = v2_tree()
    press(app, importer)
    time.sleep(3.0)
    stopped = v2_tree()
    # Written to the app's data, yet not shown: it has not committed.
    app.open_settings()
    tree = app.tree()
    obs["interrupted"] = {
        "written": stopped != before_import,
        "hidden_while_stopped": app.has_text(tree, "Mine")
        and not any((n.get("label") or "") == "Prod" for n in tree),
    }
    obs["tree_stopped"] = save_tree("stopped", tree)
    app.kill()
    os.remove(CRASH)
    app.launch()
    tree, _ = open_import(app)
    tree, undone = c8.text(app, "Did not finish, so it was undone", 20)
    obs["interrupted"]["undone"] = undone
    obs["interrupted"]["import_again"] = fx.App.node(tree, "AXButton", "Import again") is not None
    obs["tree_interrupted"] = save_tree("interrupted", tree)
    app.open_settings()
    tree = app.tree()
    obs["interrupted"]["gone_after_relaunch"] = app.has_text(tree, "Mine") and not any(
        (n.get("label") or "") == "Prod" for n in tree)
    tree, _ = open_import(app)
    press(app, "Import again")
    tree, resumed = app.wait_for(lambda t: app.has_text(t, "Imported ·"), 30)
    obs["interrupted"]["imported_again"] = resumed

    # ---- N03: what the import wrote ----
    focus = app.focused(tree)
    obs["import"] = {"receipt": resumed, "focus": focus and focus.get("label")}
    app.open_settings()
    app.make_active("Prod")
    tree, _ = c8.text(app, "Active tenant: Prod", 10)
    obs["import"]["tenant_tsg"] = app.has_text(tree, PROD_TSG)
    obs["import"]["needs_key"] = app.has_text(tree, "Scan key") or app.has_text(tree, "No scan key")
    app.ax("press", "AXButton", "Case Library")
    tree, cases = c8.text(app, "Injection", 12)
    obs["import"]["cases"] = cases and app.has_text(tree, "Jailbreak")
    app.ax("press", "AXButton", "Sessions & Reports")
    tree, reports = c8.text(app, "FP/FN reports (1)", 12)
    obs["import"]["reports"] = reports
    obs["tree_imported"] = save_tree("imported", tree)

    # ---- N02 (second half): everything is already present now ----
    tree, _ = open_import(app)
    look(app)
    c8.text(app, "v1’s data was found.", 12)
    for label in ("Saved cases", "FP/FN reports"):
        press(app, label)
    press(app, "Review what would be imported")
    tree, _ = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Review") is not None, 20)
    again = labelled(tree, "AXCheckBox", "")
    obs["review"]["already_present"] = sum("Already here" in l for l in again) >= 2
    obs["tree_review_again"] = save_tree("review-again", tree)
    press(app, "Cancel")

    # ---- N05: rollback asks, Cancel changes nothing, confirming removes ----
    press(app, "Roll back…")
    tree, sheet = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Roll Back") is not None, 10)
    app.ax("press", "AXButton", "Cancel")
    time.sleep(1.0)
    tree = app.tree()
    f = app.focused(tree)
    obs["rollback"] = {"asked": sheet, "cancel_focus": f and f.get("label"), "kept_after_cancel": app.has_text(tree, "Imported ·")}
    press(app, "Roll back…")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Roll Back") is not None, 10)
    app.ax("press", "AXButton", "Roll Back")
    tree, rolled = c8.text(app, "Rolled back ·", 30)
    obs["rollback"]["rolled_back"] = rolled
    app.open_settings()
    tree = app.tree()
    obs["rollback"]["mine_kept"] = app.has_text(tree, "Mine")
    obs["rollback"]["prod_removed"] = not any((n.get("label") or "") == "Prod" for n in tree)
    obs["tree_rolled_back"] = save_tree("rolled-back", tree)

    # ---- N08: the import by keyboard alone ----
    # Inspect, choose categories and a tenant's target, review and change
    # the selection, import (stopped by the crash hook, then Import again
    # after a relaunch), and roll back with the confirmation sheet: every
    # control is reached by Tab and operated by Space, the arrow keys and
    # Return, and each step's outcome and focus is read.
    keyboard_navigation(True)
    app.quit()
    # The crash hook is read at launch: this import stops after its first
    # item is written and noted applied.
    with open(CRASH, "w") as f:
        f.write("applied\n")
    app.launch()
    kb = {}
    kb["nav"] = space_on(app, "AXButton", "Import from v1")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Import from v1") is not None, 10)
    kb["look"] = space_on(app, "AXButton", "Look")
    tree, kb["inspected"] = c8.text(app, "v1’s data was found.", 12)
    kb["focus_after_look"] = focus_of(app)
    picked = {}
    for label in ("Tenants (names and TSG IDs)", "Saved cases"):
        reached = space_on(app, "AXCheckBox", label)
        time.sleep(0.4)
        picked[label] = bool(reached) and checked(app, label)
    kb["categories_checked_by_space"] = picked
    # Scratch's target: its pop-up opened with Space, Skip chosen with the
    # arrow keys and Return.
    kb["target_reached"] = space_on(app, "AXPopUpButton", "Scratch")
    time.sleep(0.8)
    for _ in range(3):
        app.keycode(125)
        time.sleep(0.2)
    app.keycode(36)
    time.sleep(0.6)
    target = fx.App.node(app.tree(), "AXPopUpButton", "Scratch")
    kb["target_value"] = target and target.get("value")
    kb["review_reached"] = space_on(app, "AXButton", "Review what would be imported")
    tree, kb["reviewed"] = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Review") is not None, 20)
    kb["focus_after_review"] = focus_of(app)
    items = labelled(tree, "AXCheckBox", "")
    kb["review_items"] = items
    kb["scratch_skipped"] = not any("Scratch" in l and "→" in l and "Skip" not in l and "not imported" not in l
                                    for l in items if l.startswith("Tenants"))
    importer = next((l for l in labelled(tree, "AXButton", "Import ") if l != "Import from v1"), "")
    # One reviewed item turned off and on again with Space: the import's
    # count follows.
    item = next((l for l in items if l.startswith("Saved cases")), None)
    toggled = space_on(app, "AXCheckBox", item) if item else None
    time.sleep(0.4)
    fewer = next((l for l in labelled(app.tree(), "AXButton", "Import ") if l != "Import from v1"), "")
    if toggled:
        app.keycode(49)
    time.sleep(0.4)
    again = next((l for l in labelled(app.tree(), "AXButton", "Import ") if l != "Import from v1"), "")
    kb["item_toggled"] = {"item": toggled, "before": importer, "off": fewer, "on": again}
    kb["import_reached"] = space_on(app, "AXButton", importer) if importer else None
    time.sleep(3.0)
    app.kill()
    os.remove(CRASH)
    app.launch()
    time.sleep(3.0)  # the launch rollback runs first
    kb["nav_after_relaunch"] = space_on(app, "AXButton", "Import from v1")
    tree, kb["import_view_after_relaunch"] = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Import from v1") is not None, 10)
    tree, kb["undone_at_relaunch"] = c8.text(app, "Did not finish, so it was undone", 20)
    kb["import_again_reached"] = space_on(app, "AXButton", "Import again")
    tree, kb["imported_again"] = app.wait_for(lambda t: app.has_text(t, "Imported ·"), 30)
    kb["focus_after_import"] = focus_of(app)
    obs["tree_keyboard_imported"] = save_tree("keyboard-imported", tree)
    # Roll back: Return cancels the destructive sheet and focus returns to
    # Roll back…; then Tab reaches Roll Back in the sheet and Space confirms.
    kb["roll_back_reached"] = space_on(app, "AXButton", "Roll back…")
    tree, kb["sheet_shown"] = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Roll Back") is not None, 10)
    app.keycode(36)
    tree, _ = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Roll Back") is None, 10)
    time.sleep(0.8)
    kb["return_cancelled"] = app.has_text(app.tree(), "Imported ·")
    kb["focus_after_cancel"] = focus_of(app)
    if (kb["focus_after_cancel"] or {}).get("label") == "Roll back…":
        app.keycode(49)
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Roll Back") is not None, 10)
    tabs = 0
    kb["sheet_focus_trace"] = [focus_of(app)]
    while tabs < 4 and (focus_of(app) or {}).get("label") != "Roll Back":
        app.keycode(48)
        tabs += 1
        kb["sheet_focus_trace"].append(focus_of(app))
    kb["confirm_focused_after_tabs"] = tabs if (focus_of(app) or {}).get("label") == "Roll Back" else None
    if kb["confirm_focused_after_tabs"] is not None:
        app.keycode(49)
    tree, kb["rolled_back"] = app.wait_for(lambda t: len(labelled(t, "AXStaticText", "Rolled back ·")) >= 2, 30)
    app.wait_for(lambda t: any(n.get("focused") and n.get("label") == "Imports" for n in t), 5)
    kb["focus_after_roll_back"] = focus_of(app)
    obs["tree_keyboard_rolled_back"] = save_tree("keyboard-rolled-back", tree)
    app.open_settings()
    tree = app.tree()
    kb["after"] = {"mine_kept": app.has_text(tree, "Mine"),
                   "prod_removed": not any((n.get("label") or "") == "Prod" for n in tree),
                   "scratch_never_made": not any((n.get("label") or "") == "Scratch" for n in tree)}
    keyboard_navigation(False)
    kb["keyboard_navigation_restored"] = (fx.sh(["defaults", "read", "-g", "AppleKeyboardUIMode"]).stdout.strip() or None) == KEYBOARD_UI["before"]
    obs["keyboard_import"] = kb

    # ---- N08: keyboard and reach ----
    open_import(app)
    order = app.tab_walk(40)
    labels = [x and x.get("label") for x in order]
    obs["keyboard"] = {"look": "Look again" in labels or "Look for v1 data" in labels,
                       "forget": any((l or "").startswith("Forget this record") for l in labels)}
    for _ in range(10):
        app.keys("=", "command down")
    obs["zoom"] = c8.reach(app, 2, "Forget this record")
    app.keys("0", "command down")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{900, 600}}'])
    time.sleep(0.8)
    obs["min"] = c8.reach(app, 1, "Forget this record")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{1280, 820}}'])

    # ---- N09: a record lost in both files, then folders that cannot be read ----
    app.quit()
    # One record is lost in both files; the others stay readable.
    ids = sorted(n[:-5] for n in os.listdir(RECEIPTS) if n.endswith(".json"))[:1]
    for d in (RECEIPTS, RECEIPT_COPIES):
        for i in ids:
            with open(os.path.join(d, f"{i}.json"), "w") as f:
                f.write("{")
    app.launch()
    open_import(app)
    tree, named = c8.text(app, "damaged in both of", 10)
    obs["tree_lost"] = save_tree("record-lost", tree)
    remove = f"Remove damaged record {ids[0]}" if ids else "Remove damaged record"
    reached = c8.reach(app, 1, remove)
    app.keycode(49)  # Space presses the focused button
    tree, released = app.wait_for(lambda t: not app.has_text(t, "damaged in both of"), 10)
    focus = app.focused(tree)
    obs["lost"] = {
        "records": len(ids),
        "named": named,
        "remove_reached": bool(reached["found"]),
        "released": released and not any(os.path.exists(os.path.join(d, f"{i}.json"))
                                          for d in (RECEIPTS, RECEIPT_COPIES) for i in ids),
        "focus_in_window": bool(focus) and focus.get("role") not in ("AXSheet", None),
        "focus": focus and focus.get("label"),
    }
    obs["tree_released"] = save_tree("record-released", tree)
    app.quit()
    for d in (RECEIPTS, RECEIPT_COPIES):
        os.chmod(d, 0)
    try:
        app.launch()
        open_import(app)
        tree, said = c8.text(app, "The import records can", 10)
        obs["unreadable"] = {"said": said, "alert": app.has_text(tree, "Restore access to the app")}
        obs["tree_unreadable"] = save_tree("records-unreadable", tree)
    finally:
        for d in (RECEIPTS, RECEIPT_COPIES):
            os.chmod(d, 0o700)

    app.quit()
    # ---- N06 / N07: preservation and no credential, no network ----
    trees = [os.path.join(NATIVE, n) for n in os.listdir(NATIVE) if n.startswith("tree-")]
    obs["canary_in_trees"] = [rel(p) for p in trees if CANARY in open(p).read()]
    after = v1_fingerprint()
    obs["v1_items_after"] = {a: (after["items"].get(a) or "none")[:12] for a in ACCOUNTS}
    obs["v1_changed"] = sorted([k for k in set(staged["files"]) | set(after["files"]) if staged["files"].get(k) != after["files"].get(k)]
                               + [a for a in ACCOUNTS if staged["items"].get(a) != after["items"].get(a)])
    obs["preserved"] = {"v1": after == staged, "real_v1": fx.v1_metadata() == v1_before,
                        "search_list": fx.sh(["security", "list-keychains", "-d", "user"]).stdout == login_before}
    obs["network"] = len(mgmt.requests()) + len(endpoint.requests())
    endpoint.stop()
    mgmt.stop()
    shutil.rmtree(work, ignore_errors=True)
    with open(os.path.join(NATIVE, "observations.json"), "w") as f:
        json.dump({"observations": obs, "log": LOG}, f, indent=1)
    return evaluate(prepared, obs)


def evaluate(prepared, o):
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C23"]}
    obs_path = rel(os.path.join(NATIVE, "observations.json"))
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [obs_path] + [e for e in evidence if e]})

    i = o["inspect"]
    add("C23-N01", o["available"] and i["found"] and i["no_keychain_read"] and i["navigation"] and i["session_only"]
        and len(i["tenants"]) == 2, f"inspect {i}", [o["tree_inspect"]])
    r = o["review"]
    selected_reads = sorted(set(r["reads"])) == sorted(ACCOUNTS)
    add("C23-N02", r["reviewed"] and r["keychain_note"] and selected_reads and len(r["ready"]) >= 4 and r["blocked_drafts"]
        and r["already_present"] and not o["canary_in_trees"],
        f"reads {r['reads']}; ready {r['ready']}; drafts blocked {r['blocked_drafts']}; already present {r['already_present']}; secret in AX: {o['canary_in_trees']}",
        [o["tree_review"], o["tree_review_again"]])
    m = o["import"]
    add("C23-N03", m["receipt"] and m["tenant_tsg"] and m["cases"] and m["reports"], f"import {m}", [o["tree_imported"]])
    n = o["interrupted"]
    add("C23-N04", all(n.values()), f"interrupted {n}", [o["tree_stopped"], o["tree_interrupted"]])
    b = o["rollback"]
    add("C23-N05", b["asked"] and b["kept_after_cancel"] and (b["cancel_focus"] or "").startswith("Roll back") and b["rolled_back"]
        and b["mine_kept"] and b["prod_removed"], f"rollback {b}", [o["tree_rolled_back"]])
    p = o["preserved"]
    add("C23-N06", all(p.values()), f"preserved {p}", [])
    add("C23-N07", o["network"] == 0 and m["needs_key"] and r["blocked_drafts"],
        f"requests to the synthetic services: {o['network']}; imported tenant asks for a key: {m['needs_key']}; drafts need sign-in: {r['blocked_drafts']}", [])
    k = o["keyboard"]
    kb = o["keyboard_import"]
    label = lambda f: (f or {}).get("label")  # noqa: E731
    t = kb["item_toggled"]
    steps = {
        "Import view opened": bool(kb["nav"]),
        "inspected by Space, focus kept on Look again": bool(kb["look"]) and kb["inspected"] and label(kb["focus_after_look"]) == "Look again",
        "categories checked by Space": len(kb["categories_checked_by_space"]) == 2 and all(kb["categories_checked_by_space"].values()),
        "Scratch's target set to Skip with the arrow keys": bool(kb["target_reached"]) and kb["target_value"] == "Skip this tenant",
        "reviewed, focus on Review": bool(kb["review_reached"]) and kb["reviewed"] and label(kb["focus_after_review"]) == "Review"
        and kb["scratch_skipped"],
        "an item turned off and on by Space": bool(t["item"]) and t["off"] != t["before"] and t["on"] == t["before"],
        "imported, stopped, undone at relaunch": bool(kb["import_reached"]) and bool(kb["nav_after_relaunch"]) and kb["undone_at_relaunch"],
        "Import again, focus on Imports": bool(kb["import_again_reached"]) and kb["imported_again"] and label(kb["focus_after_import"]) == "Imports",
        "Return cancelled the rollback, focus on Roll back": bool(kb["roll_back_reached"]) and kb["sheet_shown"] and kb["return_cancelled"]
        and label(kb["focus_after_cancel"]) == "Roll back…",
        "Roll Back reached by Tab and confirmed by Space, focus on Imports": kb["confirm_focused_after_tabs"] is not None and kb["rolled_back"]
        and label(kb["focus_after_roll_back"]) == "Imports",
        "rolled back: Mine kept, Prod removed, Scratch never made": all(kb["after"].values()),
        "Keyboard navigation restored": kb["keyboard_navigation_restored"],
    }
    add("C23-N08", all(steps.values()) and all(k.values()) and o["zoom"]["clear"] and o["min"]["clear"],
        f"by keyboard: {steps}; Tab reached {k}; 200% zoom: {o['zoom']['found']}; minimum size: {o['min']['found']}",
        [o["tree_keyboard_imported"], o["tree_keyboard_rolled_back"]])
    lost, unreadable = o["lost"], o["unreadable"]
    add("C23-N09", lost["records"] >= 1 and lost["named"] and lost["remove_reached"] and lost["released"]
        and lost["focus_in_window"] and unreadable["said"] and unreadable["alert"],
        f"a record damaged in both files: named {lost['named']}, Remove reached by Tab {lost['remove_reached']}, released by Space "
        f"(both files gone) {lost['released']}, focus after {lost['focus']}; folders that cannot be read: said {unreadable['said']}, "
        f"what to do {unreadable['alert']}",
        [o["tree_lost"], o["tree_released"], o["tree_unreadable"]])

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1",
        "subject": "C23",
        "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (tools/native-acceptance/ax.swift, System Events keystrokes); tools/native-acceptance/acceptance_c23.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "The v1 source is a synthetic fixture folder and a staged file keychain read without prompting; the production source (v1's folder and the login Keychain, where macOS asks before each item is read) is the same read-only adapter's other branch and is not driven here.",
            "The interruption is the fixture crash hook (the import stops after an item is noted writing) followed by ending the app; every other commit point and rollback interruption is exercised by the imports_rollback suite.",
            "Spoken VoiceOver output was not recorded; AX roles, names and focus are inspected.",
            "C23-N08's keyboard pass turns on macOS Keyboard navigation (the global AppleKeyboardUIMode preference, which AppKit reads for a sheet's buttons) for its duration and restores the operator's own value afterwards; without it no app's destructive confirmation button is reachable by Tab, and Return cancels.",
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
