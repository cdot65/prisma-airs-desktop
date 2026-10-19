#!/usr/bin/env python3
"""C09 native acceptance: reviewed Runtime changes through the real fixture
window, the native AppKit confirmation and secret sheets and the clipboard,
against the synthetic management endpoint (mgmt_server.py, which now keeps
an in-memory inventory and issues synthetic one-time keys).

Run from the repository root after `xtask native-prepare --chunk C09`.
Requires the Accessibility grant for the invoking terminal. It starts from
an empty fixture data root, uses only synthetic credentials and keys, never
touches the login keychain or v1 data, saves and restores the operator's
clipboard text around the clipboard checks (never logging it), and removes
the fixture root at the end.
"""

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
import acceptance_c08 as c8  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C09")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C09" else "native-C09")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C09" else "native-acceptance-C09.json")
AUTH_CODE = "SYNTHETIC-C08-DP-CODE"
LOG = []
SEEN_SECRETS = []


def rel(path):
    return os.path.relpath(path, ROOT)


def save_tree(name, tree):
    path = os.path.join(NATIVE, f"tree-{name}.json")
    with open(path, "w") as f:
        json.dump(tree, f, indent=0)
    return rel(path)


def sha(text):
    return hashlib.sha256(text.encode()).hexdigest()


def clipboard():
    return subprocess.run(["pbpaste"], capture_output=True, text=True).stdout


def set_clipboard(text):
    subprocess.run(["pbcopy"], input=text, text=True)


def writes(mgmt, method=None, prefix=""):
    return [r for r in mgmt.requests()
            if r["method"] != "GET" and r["path"] != "/oauth2/access_token"
            and (method is None or r["method"] == method) and r["path"].startswith(prefix)]


def sheet(app, tree=None):
    """The open AppKit sheet's nodes (AXSheet subtree), or None."""
    tree = tree if tree is not None else app.tree()
    for i, n in enumerate(tree):
        if n.get("role") == "AXSheet":
            inside = [n]
            for m in tree[i + 1:]:
                if m["depth"] <= n["depth"]:
                    break
                inside.append(m)
            return inside
    return None


def press_exact(app, label):
    """Presses the AXButton whose label is exactly `label` (the page may
    hold buttons that merely start with it)."""
    tree = app.tree()
    matches = [n for n in tree if n.get("role") == "AXButton" and (n.get("label") or "").startswith(label)]
    for nth, n in enumerate(matches):
        if n.get("label") == label:
            app.ax("press", "AXButton", label, nth)
            return True
    return False


def wait_sheet(app, timeout=8):
    return app.wait_for(lambda t: sheet(app, t) is not None, timeout)


def wait_no_sheet(app, timeout=8):
    return app.wait_for(lambda t: sheet(app, t) is None, timeout)


def open_change(app, inventory, details, action):
    app.ax("press", "AXButton", inventory)
    app.wait_for(lambda t: fx.App.node(t, "AXButton", details) is not None)
    app.ax("press", "AXButton", details)
    app.wait_for(lambda t: fx.App.node(t, "AXButton", action) is not None)
    app.ax("press", "AXButton", action)
    return app.wait_for(lambda t: fx.App.node(t, "AXButton", "Review change") is not None)


def review(app, ready_text):
    label = "Review again" if fx.App.node(app.tree(), "AXButton", "Review again") else "Review change"
    app.ax("press", "AXButton", label)
    return c8.text(app, ready_text)


def close_workbench(app):
    press_exact(app, "Close")
    time.sleep(0.6)


def secret_in(nodes):
    for n in nodes or []:
        for value in (n.get("label"), n.get("value")):
            if isinstance(value, str) and value.startswith("SYNTHETIC-C09-"):
                return value
    return None


def main():
    saved_clipboard = clipboard()
    try:
        return run()
    finally:
        set_clipboard(saved_clipboard)
        fx.sh(["pkill", "-f", fx.PROCESS])
        fx.clean_fixture_root()


def run():
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")
    if c8.sha256(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c09-acceptance-")
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

    # ---- setup: tenant, scan key, management client ----
    app.launch()
    app.open_settings()
    app.type_into("AXTextField", "TSG ID (optional)", c8.TSG)
    app.create_custom_tenant("Tenant A", endpoint.origin)
    app.save_key(c8.SCAN_KEY)
    c8.save_client(app)
    c8.open_admin(app)
    c8.text(app, "Items 1–25 of 25+")

    # ---- N01: edit and review ----
    open_change(app, "Custom topics", "Details for Finance", "Edit…")
    draft = app.field("AXTextArea", "custom topic draft (JSON)") or {}
    obs["editor_prefilled"] = "Money talk" in str(draft.get("value") or "")
    edited = '{"topic_name":"Finance","description":"Money and payments","examples":[],"revision":1}'
    app.type_into("AXTextArea", "custom topic draft (JSON)", edited)
    tree, reviewed = review(app, "Applies only to this tenant")
    obs["reviewed"] = reviewed and app.has_text(tree, "Money and payments") and app.has_text(tree, "Money talk") \
        and app.has_text(tree, "Type Finance to confirm")
    obs["review_writes"] = len(writes(mgmt))
    obs["tree_review"] = save_tree("review", tree)

    # ---- N02: the native sheet; Escape cancels; Return applies ----
    app.type_into("AXTextField", "Type Finance to confirm", "Finance")
    app.ax("press", "AXButton", "Apply…")
    tree, shown = wait_sheet(app)
    nodes = sheet(app, tree) or []
    obs["sheet_shown"] = shown
    obs["sheet_title"] = any("Update custom topic “Finance”?" in (n.get("label") or "") or
                             "Update custom topic “Finance”?" in str(n.get("value") or "") for n in nodes)
    obs["sheet_tenant"] = any("Tenant A" in (n.get("label") or "") + str(n.get("value") or "") for n in nodes)
    obs["sheet_in_webview"] = any(n.get("role") == "AXWebArea" for n in nodes)
    obs["tree_sheet"] = save_tree("sheet", tree)
    app.keycode(53)  # Escape
    _, gone = wait_no_sheet(app)
    time.sleep(1)
    obs["escape_cancelled"] = gone and len(writes(mgmt)) == 0
    app.ax("press", "AXButton", "Apply…")
    wait_sheet(app)
    app.keycode(36)  # Return: Apply (non-destructive default)
    tree, applied = c8.text(app, "Applied. The item was read back.", 15)
    puts = writes(mgmt, "PUT")
    obs["applied"] = applied
    obs["puts"] = [{k: p.get(k) for k in ("path", "author", "description", "fields")} for p in puts]
    obs["tree_applied"] = save_tree("applied", tree)

    # ---- N03: conflict and edited-after-review ----
    close_workbench(app)
    app.ax("press", "AXButton", "Edit…")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Review change") is not None)
    app.type_into("AXTextArea", "custom topic draft (JSON)",
                  '{"topic_name":"Finance","description":"Second edit","examples":[],"revision":2}')
    review(app, "Applies only to this tenant")
    app.type_into("AXTextField", "Type Finance to confirm", "Finance")
    mgmt.mode(topicDescription="Changed elsewhere")
    app.ax("press", "AXButton", "Apply…")
    wait_sheet(app)
    app.keycode(36)
    tree, conflict = c8.text(app, "The item changed since it was reviewed", 15)
    obs["conflict"] = conflict and len(writes(mgmt, "PUT")) == 1
    obs["tree_conflict"] = save_tree("conflict", tree)
    mgmt.mode()
    # Edited after Review: refused before any sheet.
    close_workbench(app)
    app.ax("press", "AXButton", "Edit…")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Review change") is not None)
    review(app, "Applies only to this tenant")
    app.type_into("AXTextField", "Type Finance to confirm", "Finance")
    # A real edit (formatting alone normalizes to the same draft).
    app.type_into("AXTextArea", "custom topic draft (JSON)",
                  '{"topic_name":"Finance","description":"Edited after review","examples":[],"revision":2}')
    app.ax("press", "AXButton", "Apply…")
    tree, refused = c8.text(app, "The draft changed since it was reviewed")
    obs["edited_refused"] = refused and sheet(app, tree) is None and len(writes(mgmt, "PUT")) == 1
    obs["tree_edited"] = save_tree("edited-after-review", tree)

    # ---- N04: destructive delete ----
    close_workbench(app)
    app.ax("press", "AXButton", "Delete…")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Review change") is not None)
    review(app, "Removes this resource")
    app.type_into("AXTextField", "Type Finance to confirm", "Finance")
    app.ax("press", "AXButton", "Apply…")
    tree, _ = wait_sheet(app)
    obs["delete_sheet"] = any("Delete custom topic" in (n.get("label") or "") + str(n.get("value") or "")
                              for n in sheet(app, tree) or [])
    app.keycode(36)  # Return: Cancel for a destructive change
    wait_no_sheet(app)
    time.sleep(1)
    obs["return_cancelled_delete"] = len(writes(mgmt, "DELETE")) == 0
    app.ax("press", "AXButton", "Apply…")
    wait_sheet(app)
    obs["pressed_delete"] = press_exact(app, "Delete")
    tree, deleted = c8.text(app, "Applied. The item is no longer in the inventory.", 15)
    obs["deleted"] = deleted and len(writes(mgmt, "DELETE")) == 1
    obs["tree_deleted"] = save_tree("deleted", tree)

    # ---- N05/N06: key creation and the secret sheet ----
    close_workbench(app)
    app.ax("press", "AXButton", "API keys")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Create API key") is not None)
    app.ax("press", "AXButton", "Create API key")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Review change") is not None)
    app.type_into("AXTextArea", "API key draft (JSON)",
                  '{"api_key_name":"Native key","cust_app":"Native app","cust_env":"production","cust_cloud_provider":"aws",'
                  '"dp_name":"dp-0","revoked":false,"rotation_time_interval":30,"rotation_time_unit":"days"}')
    tree, _ = review(app, "Applies only to this tenant")
    obs["auth_code_shown"] = app.has_text(tree, AUTH_CODE)
    app.type_into("AXTextField", "Type CREATE to confirm", "CREATE")
    app.ax("press", "AXButton", "Apply…")
    wait_sheet(app)
    app.keycode(36)
    tree, held = c8.text(app, "held in memory only", 15)
    posts = writes(mgmt, "POST", "/aisec/v1/mgmt/apikey")
    obs["key_created"] = held
    obs["key_post"] = [{k: p.get(k) for k in ("author", "authCodeMatches", "fields")} for p in posts]
    issued = [p.get("issuedSha256") for p in posts]
    obs["tree_key_held"] = save_tree("key-held", tree)
    app.ax("press", "AXButton", "Show new key…")
    tree, _ = wait_sheet(app)
    secret = secret_in(sheet(app, tree))
    if secret:
        SEEN_SECRETS.append(secret)
    obs["revealed_matches_issued"] = bool(secret) and sha(secret) in issued
    press_exact(app, "Copy and Close")
    wait_no_sheet(app)
    copied_at = time.time()
    obs["copied_matches"] = bool(secret) and sha(clipboard()) == sha(secret)
    # Shown once: the desktop no longer holds it, so it cannot be shown again.
    tree, shown_once = c8.text(app, "The new key was shown once and is no longer held by the desktop.")
    obs["shown_once"] = shown_once and fx.App.node(tree, "AXButton", "Show new key…") is None
    first_secret = secret

    # ---- N07: rotation; the rotated key's sheet left open ends itself ----
    app.ax("press", "AXButton", "Refresh")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Details for Native key") is not None)
    close_workbench(app)
    app.ax("press", "AXButton", "Details for Native key")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Rotate…") is not None)
    app.ax("press", "AXButton", "Rotate…")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Review change") is not None)
    review(app, "Replaces this key")
    app.type_into("AXTextField", "Type Native key to confirm", "Native key")
    app.ax("press", "AXButton", "Apply…")
    wait_sheet(app)
    app.keycode(36)  # Return cancels a destructive rotation
    wait_no_sheet(app)
    time.sleep(1)
    obs["return_cancelled_rotate"] = len(writes(mgmt, "POST", "/aisec/v1/mgmt/apikey/regenerate")) == 0
    app.ax("press", "AXButton", "Apply…")
    wait_sheet(app)
    press_exact(app, "Rotate key")
    c8.text(app, "held in memory only", 15)
    rotated = writes(mgmt, "POST", "/aisec/v1/mgmt/apikey/regenerate")
    obs["rotated"] = len(rotated) == 1 and rotated[0].get("fields") == ["rotation_time_interval", "rotation_time_unit", "updated_by"]
    app.ax("press", "AXButton", "Show new key…")
    tree, _ = wait_sheet(app)
    secret = secret_in(sheet(app, tree))
    if secret:
        SEEN_SECRETS.append(secret)
    shown_at = time.time()
    # Left open, the sheet ends itself after 60 s; by then the first key
    # copied above (nothing else was copied) has been cleared.
    time.sleep(max(0, 62 - (time.time() - shown_at)))
    tree = app.tree()
    obs["reveal_expired"] = sheet(app, tree) is None
    obs["clipboard_cleared"] = bool(first_secret) and clipboard() != first_secret and time.time() - copied_at >= 60

    # ---- Clear key forgets a held key; a later copy keeps foreign text ----
    def create_key(name, app_name):
        close_workbench(app)
        app.ax("press", "AXButton", "Create API key")
        app.wait_for(lambda t: fx.App.node(t, "AXButton", "Review change") is not None)
        app.type_into("AXTextArea", "API key draft (JSON)",
                      '{"api_key_name":"%s","cust_app":"%s","cust_env":"production","cust_cloud_provider":"aws",'
                      '"dp_name":"dp-0","revoked":false,"rotation_time_interval":30,"rotation_time_unit":"days"}' % (name, app_name))
        tree, ready = review(app, "Applies only to this tenant")
        if not ready or fx.App.node(tree, "AXTextField", "Type CREATE to confirm") is None:
            obs.setdefault("create_key_problems", []).append(
                [t for t in app.texts(tree) if any(w in t for w in ("already", "could not", "not ", "Reading", "draft", "inventory"))][:12])
            save_tree(f"create-{name.replace(' ', '-')}", tree)
        app.type_into("AXTextField", "Type CREATE to confirm", "CREATE")
        app.ax("press", "AXButton", "Apply…")
        wait_sheet(app)
        app.keycode(36)
        return app.wait_for(lambda t: fx.App.node(t, "AXButton", "Show new key…") is not None, 15)

    create_key("Native key 2", "Native app 2")
    app.ax("press", "AXButton", "Clear key")
    tree, cleared = c8.text(app, "The new key was cleared from memory.")
    obs["key_cleared"] = cleared and fx.App.node(tree, "AXButton", "Show new key…") is None
    create_key("Native key 3", "Native app 3")
    app.ax("press", "AXButton", "Show new key…")
    tree, _ = wait_sheet(app)
    secret = secret_in(sheet(app, tree))
    if secret:
        SEEN_SECRETS.append(secret)
    press_exact(app, "Copy and Close")
    wait_no_sheet(app)
    set_clipboard("operator text c09")
    time.sleep(62)
    obs["foreign_clipboard_kept"] = clipboard() == "operator text c09"

    # ---- N08: T23 offline review, return, explicit retry ----
    close_workbench(app)
    app.ax("press", "AXButton", "Custom topics")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "New custom topic") is not None)
    mgmt.stop()
    app.ax("press", "AXButton", "New custom topic")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Review change") is not None)
    app.type_into("AXTextArea", "custom topic draft (JSON)", '{"topic_name":"Offline topic","description":"t","examples":[]}')
    # A create needs no baseline read, so it reviews offline; applying it
    # fails to reach the service and is reported, never queued.
    review(app, "Applies only to this tenant")
    app.type_into("AXTextField", "Type CREATE to confirm", "CREATE")
    app.ax("press", "AXButton", "Apply…")
    wait_sheet(app)
    app.keycode(36)
    tree, offline = c8.text(app, "The management service could not be reached.", 20)
    # A transport failure's outcome is unknown (C03 semantics): the window
    # must not claim it applied, and must say to refresh before retrying.
    obs["offline_review"] = offline and app.has_text(
        tree, "The management service could not be reached. The change may or may not have been applied. Refresh the inventory")
    obs["offline_texts"] = [t for t in app.texts(tree) if "service" in t][:6]
    obs["tree_offline_review"] = save_tree("offline-apply", tree)
    before = len(mgmt.requests())
    mgmt.start()
    time.sleep(6)
    obs["offline_auto"] = len(mgmt.requests()) - before
    tree, retried = review(app, "Applies only to this tenant")
    obs["offline_retry"] = retried and len(writes(mgmt, "POST", "/aisec/v1/mgmt/topic")) == 0
    obs["tree_offline"] = save_tree("offline-retry", tree)

    # ---- N10: zoom and minimum size (before quitting) ----
    for _ in range(10):
        app.keys("=", "command down")
    obs["zoom"] = c8.reach(app, 2, "Review again")
    app.keys("0", "command down")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{900, 600}}'])
    time.sleep(0.8)
    obs["min_size"] = c8.reach(app, 1, "Review again")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{1280, 820}}'])

    # ---- N09: quit with a confirmed create in flight; relaunch ----
    mgmt.mode(delayMs={"/aisec/v1/mgmt/topic": 5000})
    app.type_into("AXTextField", "Type CREATE to confirm", "CREATE")
    app.ax("press", "AXButton", "Apply…")
    wait_sheet(app)
    app.keycode(36)
    time.sleep(1.5)
    app.quit()
    time.sleep(5)
    mgmt.mode()
    in_flight = len(writes(mgmt, "POST", "/aisec/v1/mgmt/topic"))
    app.launch()
    tree = c8.open_admin(app)
    time.sleep(6)
    tree = app.tree()
    obs["relaunch_clean"] = fx.App.node(tree, "AXHeading", "Applied changes") is None and not app.has_text(tree, "Applies only to this tenant")
    obs["in_flight_posts"] = in_flight
    obs["relaunch_posts"] = len(writes(mgmt, "POST", "/aisec/v1/mgmt/topic"))
    obs["tree_relaunch"] = save_tree("relaunch", tree)

    # ---- N11: retiring the context ends an open confirmation and secret sheet ----
    app.open_settings()
    app.create_custom_tenant("Tenant B", endpoint.origin)
    app.make_active("Tenant A")
    c8.open_admin(app)
    c8.text(app, "Items 1–25 of 25+")
    open_change(app, "Custom topics", "Details for Finance", "Edit…")
    app.type_into("AXTextArea", "custom topic draft (JSON)",
                  '{"topic_name":"Finance","description":"Retired mid-confirmation","examples":[],"revision":1}')
    review(app, "Applies only to this tenant")
    app.type_into("AXTextField", "Type Finance to confirm", "Finance")
    puts_before = len(writes(mgmt, "PUT"))
    app.ax("press", "AXButton", "Apply…")
    _, confirm_shown = wait_sheet(app)
    # The sheet is window-modal; the switch is made through Accessibility.
    app.ax("press", "AXButton", "Settings")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Make Tenant B active") is not None, 8)
    app.ax("press", "AXButton", "Make Tenant B active")
    _, confirm_gone = wait_no_sheet(app, 10)
    app.keycode(36)  # a late Return applies nothing
    time.sleep(2)
    tree = app.tree()
    obs["retire_confirmation"] = {"shown": confirm_shown, "ended": confirm_gone,
                                  "no_write": len(writes(mgmt, "PUT")) == puts_before,
                                  "tenant_b_active": app.has_text(tree, "Active tenant: Tenant B") or app.has_text(tree, "Tenant B (active)")}
    obs["tree_retired_confirmation"] = save_tree("retired-confirmation", tree)
    app.make_active("Tenant A")
    c8.open_admin(app)
    app.ax("press", "AXButton", "API keys")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Create API key") is not None)
    app.ax("press", "AXButton", "Create API key")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Review change") is not None)
    app.type_into("AXTextArea", "API key draft (JSON)",
                  '{"api_key_name":"Retired sheet key","cust_app":"Native app","cust_env":"production","cust_cloud_provider":"aws",'
                  '"dp_name":"dp-0","revoked":false,"rotation_time_interval":30,"rotation_time_unit":"days"}')
    review(app, "Applies only to this tenant")
    app.type_into("AXTextField", "Type CREATE to confirm", "CREATE")
    app.ax("press", "AXButton", "Apply…")
    wait_sheet(app)
    app.keycode(36)
    c8.text(app, "held in memory only", 15)
    marker = "c09-retire-clipboard-marker"
    set_clipboard(marker)
    app.ax("press", "AXButton", "Show new key…")
    tree, secret_shown = wait_sheet(app)
    secret = secret_in(sheet(app, tree))
    if secret:
        SEEN_SECRETS.append(secret)
    app.ax("press", "AXButton", "Settings")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Make Tenant B active") is not None, 8)
    app.ax("press", "AXButton", "Make Tenant B active")
    _, secret_gone = wait_no_sheet(app, 10)
    time.sleep(1)
    app.make_active("Tenant A")
    tree = c8.open_admin(app)
    time.sleep(1)
    tree = app.tree()
    obs["retire_secret"] = {"shown": secret_shown and bool(secret), "ended": secret_gone,
                            "clipboard_untouched": clipboard() == marker,
                            "not_offered_again": fx.App.node(tree, "AXButton", "Show new key…") is None}
    obs["tree_retired_secret"] = save_tree("retired-secret", tree)

    # ---- N12: an older reveal's expiry never ends a newer reveal ----
    for name in ("Ownership key one", "Ownership key two"):
        app.ax("press", "AXButton", "API keys")
        app.wait_for(lambda t: fx.App.node(t, "AXButton", "Create API key") is not None)
        app.ax("press", "AXButton", "Create API key")
        app.wait_for(lambda t: fx.App.node(t, "AXButton", "Review change") is not None)
        app.type_into("AXTextArea", "API key draft (JSON)",
                      '{"api_key_name":"' + name + '","cust_app":"' + name + ' app","cust_env":"production","cust_cloud_provider":"aws",'
                      '"dp_name":"dp-0","revoked":false,"rotation_time_interval":30,"rotation_time_unit":"days"}')
        tree, ready = review(app, "Applies only to this tenant")
        if not ready:
            save_tree("n12-review-" + name.split()[-1], tree)
            raise RuntimeError(f"{name} review did not become ready: " + " | ".join(
                str(n.get("value") or n.get("label")) for n in tree if n.get("role") == "AXStaticText")[-600:])
        app.type_into("AXTextField", "Type CREATE to confirm", "CREATE")
        app.ax("press", "AXButton", "Apply…")
        wait_sheet(app)
        app.keycode(36)
        c8.text(app, "held in memory only", 15)
        close_workbench(app)
    app.ax("press", "AXButton", "Show new key…", 0)
    first_at = time.time()
    tree, _ = wait_sheet(app)
    first = secret_in(sheet(app, tree))
    if first:
        SEEN_SECRETS.append(first)
    press_exact(app, "Close")
    wait_no_sheet(app)
    time.sleep(max(0.0, first_at + 40 - time.time()))
    app.ax("press", "AXButton", "Show new key…", 0)
    tree, second_shown = wait_sheet(app)
    second = secret_in(sheet(app, tree))
    if second:
        SEEN_SECRETS.append(second)
    # The first reveal's 60 s expiry passes while the second is shown.
    time.sleep(max(0.0, first_at + 66 - time.time()))
    tree = app.tree()
    still = secret_in(sheet(app, tree))
    obs["reveal_ownership"] = {"second_shown": second_shown and bool(second) and second != first,
                               "survives_older_expiry": still == second and still is not None}
    press_exact(app, "Close")
    _, closed = wait_no_sheet(app)
    obs["reveal_ownership"]["closed_by_operator"] = closed
    app.quit()

    trees = [os.path.join(NATIVE, n) for n in os.listdir(NATIVE) if n.startswith("tree-")]
    obs["secret_in_trees"] = [rel(p) for p in trees if any(s in open(p).read() for s in SEEN_SECRETS + [AUTH_CODE])]
    hits = []
    for base_dir in [fx.DATA] + c8.WEBKIT_DIRS:
        for base, _, names in os.walk(base_dir):
            for name in names:
                path = os.path.join(base, name)
                if path == fx.KEYCHAIN:
                    continue
                try:
                    data = open(path, "rb").read()
                except OSError:
                    continue
                if any(s.encode() in data for s in SEEN_SECRETS + [AUTH_CODE]):
                    hits.append(path.replace(fx.HOME, "~"))
    obs["persisted_secret_files"] = hits
    obs["secrets_seen"] = len(SEEN_SECRETS)
    endpoint.stop()
    mgmt.stop()
    obs["v1_unchanged"] = fx.v1_metadata() == v1_before
    obs["mgmt_writes"] = [{k: r.get(k) for k in ("method", "path", "author", "authCodeMatches", "fields", "status")} for r in writes(mgmt)]
    SEEN_SECRETS.clear()
    shutil.rmtree(work, ignore_errors=True)
    with open(os.path.join(NATIVE, "observations.json"), "w") as f:
        json.dump({"observations": obs, "log": LOG}, f, indent=1)
    return evaluate(prepared, obs)


def evaluate(prepared, o):
    o.setdefault("retire_confirmation", {"missing": False})
    o.setdefault("retire_secret", {"missing": False})
    o.setdefault("reveal_ownership", {"missing": False})
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C09"]}
    obs_path = rel(os.path.join(NATIVE, "observations.json"))
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [obs_path] + [e for e in evidence if e]})

    add("C09-N01", o["editor_prefilled"] and o["reviewed"] and o["review_writes"] == 0,
        f"editor prefilled with the item: {o['editor_prefilled']}; review shows current and expected JSON with the impact and 'Type Finance to confirm': "
        f"{o['reviewed']}; writes while reviewing: {o['review_writes']}", [o["tree_review"]])
    puts = o["puts"]
    put_ok = len(puts) == 1 and puts[0]["author"] == c8.CLIENT_ID and puts[0]["description"] == "Money and payments"
    add("C09-N02", o["sheet_shown"] and o["sheet_title"] and o["sheet_tenant"] and not o["sheet_in_webview"] and o["escape_cancelled"]
        and o["applied"] and put_ok,
        f"AppKit sheet shown: {o['sheet_shown']} (title from Rust's summary: {o['sheet_title']}, tenant named: {o['sheet_tenant']}, no web area inside: "
        f"{not o['sheet_in_webview']}); Escape cancelled with no write: {o['escape_cancelled']}; Return applied and read back: {o['applied']}; writes {puts}",
        [o["tree_sheet"], o["tree_applied"]])
    add("C09-N03", o["conflict"] and o["edited_refused"],
        f"changed elsewhere → 'changed since it was reviewed' and no second PUT: {o['conflict']}; draft edited after Review refused without a sheet: "
        f"{o['edited_refused']}", [o["tree_conflict"], o["tree_edited"]])
    add("C09-N04", o["delete_sheet"] and o["return_cancelled_delete"] and o["pressed_delete"] and o["deleted"],
        f"destructive delete sheet: {o['delete_sheet']}; Return chose Cancel (no DELETE): {o['return_cancelled_delete']}; Delete pressed: "
        f"{o['pressed_delete']}; one DELETE and 'no longer in the inventory': {o['deleted']}", [o["tree_deleted"]])
    kp = o["key_post"]
    add("C09-N05", o["key_created"] and len(kp) == 1 and kp[0]["authCodeMatches"] and kp[0]["author"] == c8.CLIENT_ID
        and not o["auth_code_shown"] and not o["secret_in_trees"] and not o["persisted_secret_files"],
        f"'held in memory only': {o['key_created']}; POST {kp}; auth code shown in the window: {o['auth_code_shown']}; AX snapshots with a key or the code: "
        f"{o['secret_in_trees']}; data-root files with one: {o['persisted_secret_files']}", [o["tree_key_held"]])
    add("C09-N06", o["revealed_matches_issued"] and o["copied_matches"] and o["shown_once"] and o["reveal_expired"] and o["clipboard_cleared"],
        f"sheet showed the issued key (hash): {o['revealed_matches_issued']}; Copy and Close put it on the clipboard: {o['copied_matches']}; then shown "
        f"once and no longer offered: {o['shown_once']}; another key's sheet left open ended after 60 s: {o['reveal_expired']}; the copied key cleared "
        f"from the clipboard after 60 s: {o['clipboard_cleared']}", [])
    add("C09-N07", o["return_cancelled_rotate"] and o["rotated"] and o["foreign_clipboard_kept"] and o["key_cleared"],
        f"Return cancelled the destructive rotation: {o['return_cancelled_rotate']}; one regenerate with the reduced body: {o['rotated']}; operator's later "
        f"clipboard text kept after 60 s: {o['foreign_clipboard_kept']}; Clear key forgot the held key: {o['key_cleared']}", [])
    add("C09-N08", o["offline_review"] and o["offline_auto"] == 0 and o["offline_retry"],
        f"offline apply reported unreachable and not applied: {o['offline_review']} ({o['offline_texts']}); requests within 6 s of return: "
        f"{o['offline_auto']}; explicit Review prepared it again with nothing sent: {o['offline_retry']}", [o["tree_offline_review"], o["tree_offline"]])
    add("C09-N09", o["relaunch_clean"] and o["in_flight_posts"] == 1 and o["relaunch_posts"] == 1,
        f"after relaunch no review or result: {o['relaunch_clean']}; topic POSTs in flight before quitting: {o['in_flight_posts']}; after relaunch + 6 s: "
        f"{o['relaunch_posts']}", [o["tree_relaunch"]])
    add("C09-N10", o["zoom"]["clear"] and o["min_size"]["clear"],
        f"200% zoom: {o['zoom']['found']} in window {o['zoom']['window']}, clear: {o['zoom']['clear']}; minimum size: {o['min_size']['found']} in window "
        f"{o['min_size']['window']}, clear: {o['min_size']['clear']}", [])

    ro = o["reveal_ownership"]
    add("C09-N12", all(ro.values()),
        f"a second key revealed 40 s after the first was closed stays shown past the first reveal's 60 s expiry: {ro}", [])
    rc, rs = o["retire_confirmation"], o["retire_secret"]
    add("C09-N11", all(rc.values()) and all(rs.values()),
        f"confirmation sheet retired by a tenant switch: {rc}; secret sheet retired: {rs}",
        [o.get("tree_retired_confirmation"), o.get("tree_retired_secret")])
    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1",
        "subject": "C09",
        "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "method": "cua",
        "performed_by": "Claude Code driving the real fixture window, its AppKit sheets and the general pasteboard through macOS Accessibility (tools/native-acceptance/ax.swift, System Events keystrokes, pbcopy/pbpaste) against tools/native-acceptance/mgmt_server.py; tools/native-acceptance/acceptance_c09.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "The management endpoint is a local synthetic fixture; live writes and one-time keys are qualified in C25.",
            "The clipboard checks use the general pasteboard (the operator's text is saved and restored, never logged); concealment hints are written but their effect on clipboard managers is not observed.",
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
