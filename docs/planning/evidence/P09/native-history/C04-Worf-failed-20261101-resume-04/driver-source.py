#!/usr/bin/env python3
"""C04 native acceptance: Academy device sign-in in the prepared fixture app,
driven through macOS Accessibility against the synthetic Academy + Clerk
endpoint (academy_server.py), evaluated against the C04 inventory
(docs/architecture/native-checks.json).

The fixture build trusts the disposable test CA, talks to the synthetic
endpoint named in its data root, records (does not launch) browser pages,
and keeps credentials in its dedicated fixture keychain. Keychain items are
inspected by attributes only. The fixture root is removed at the end.

Launches:
1. fresh: account group, keyboard sign-in, code and page, approval,
   tenants created and switched;
2. relaunch: still signed in; sign-out; cancel; declined; unreachable;
   zoom/minimum size; sign in again and reset.
"""

import json
import os
import shutil
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fixture_app as fx  # noqa: E402
import v2_setup as v2s  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C04")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C04" else "native-C04")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C04" else "native-acceptance-C04.json")
ACADEMY_SERVICE = "io.cdot.prisma-airs-desktop.v2.fixture.academy"
SCAN_SERVICE = "io.cdot.prisma-airs-desktop.v2.fixture.scan"
SECRETS = ["fixture-access-1", "fixture-access-2", "fixture-refresh-1", "fixture-device-code"]
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


def group(tree, label):
    """Nodes inside the first element labelled `label` (flat tree with depth)."""
    for i, n in enumerate(tree):
        if (n.get("label") or "") == label:
            inside = []
            for m in tree[i + 1:]:
                if m["depth"] <= n["depth"]:
                    break
                inside.append(m)
            return inside
    return []


def texts(nodes):
    return [n.get("label") or "" for n in nodes] + [str(n.get("value")) for n in nodes if n.get("value") is not None]


def shows(nodes, fragment):
    return any(fragment in t for t in texts(nodes))


def account(app):
    return group(app.tree(), "Academy account")


def wait_account(app, fragment, timeout=12):
    deadline = time.time() + timeout
    while time.time() < deadline:
        if shows(account(app), fragment):
            return True
        time.sleep(0.3)
    return False


def press(app, label):
    app.ax("press", "AXButton", label)


def data_root_hits():
    hits = []
    for base, _, names in os.walk(fx.DATA):
        for name in names:
            path = os.path.join(base, name)
            if path == fx.KEYCHAIN:
                continue
            try:
                data = open(path, "rb").read()
            except OSError:
                continue
            if any(s.encode() in data for s in SECRETS):
                hits.append(os.path.relpath(path, fx.DATA))
    return hits


def main():
    try:
        return run()
    finally:
        fx.sh(["pkill", "-f", fx.PROCESS])
        fx.clean_fixture_root()


def run():
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    if sha256(os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c04-acceptance-")
    if os.path.isdir(NATIVE):
        shutil.rmtree(NATIVE)
    os.makedirs(NATIVE)
    app = fx.App(bundle, work, LOG)
    app.quit()
    fx.clean_fixture_root()
    endpoint = fx.AcademyEndpoint(os.path.join(work, "academy"))
    endpoint.install()
    endpoint.start()
    obs = {"origin": endpoint.origin}

    # ---- launch 1 ----
    app.launch()
    tree = app.open_settings()
    obs["scan_items_before"] = fx.keychain_accounts(SCAN_SERVICE)
    acct = group(tree, "Academy account")
    obs["group"] = bool(acct) and shows(acct, "separate from AIRS tenants") and shows(acct, "Not signed in to Academy.")
    obs["tree_signed_out"] = save_tree("signed-out", tree)
    # Keyboard: Tab to the sign-in button and press Space.
    reached = False
    for _ in range(40):
        app.keycode(48)
        f = app.focused(app.tree()) or {}
        if f.get("role") == "AXButton" and f.get("label") == "Sign in to Academy":
            reached = True
            break
    obs["keyboard_reached_sign_in"] = reached
    if reached:
        app.keycode(49)
    else:
        press(app, "Sign in to Academy")
    obs["awaiting"] = wait_account(app, "WDJB-MJHT")
    tree = app.tree()
    acct = group(tree, "Academy account")
    obs["code_label"] = any((n.get("label") or "").startswith("Sign-in code W D J B") for n in acct)
    obs["page_shown"] = shows(acct, f"{endpoint.origin}/activate?user_code=WDJB-MJHT")
    obs["status_regions"] = sum(1 for n in acct if n.get("subrole") == "AXApplicationStatus")
    obs["tree_awaiting"] = save_tree("awaiting", tree)
    order = app.tab_walk(4)
    labels = [o and o.get("label") for o in order]
    obs["keyboard_open_and_cancel"] = "Open sign-in page" in labels and "Cancel sign-in" in labels
    press(app, "Open sign-in page")
    time.sleep(1)
    obs["opened"] = fx.opened_urls()
    endpoint.mode(approval="granted", displayName="Ada Lovelace")
    obs["signed_in"] = wait_account(app, "Signed in to Academy as") and shows(account(app), "Ada Lovelace")
    obs["requests_sign_in"] = endpoint.requests()
    obs["academy_items"] = fx.keychain_accounts(ACADEMY_SERVICE)
    # The Academy sign-in adds nothing to the scan namespace: compared with the items present before it (on the one
    # presentation the wizard's placeholder tenant stored its scan key first).
    obs["scan_items"] = fx.keychain_accounts(SCAN_SERVICE)
    obs["scan_items_added"] = sorted(set(obs["scan_items"]) - set(obs.get("scan_items_before", [])))
    tree = app.tree()
    obs["tree_signed_in"] = save_tree("signed-in", tree)
    # Tenants are independent of the learner account.
    app.create_custom_tenant("Tenant A", "https://airs-a.example.com")
    app.create_custom_tenant("Tenant B", "https://airs-b.example.com")
    app.make_active("Tenant B")
    time.sleep(0.5)
    obs["after_tenant_switch"] = shows(account(app), "Signed in to Academy as") and shows(account(app), "Ada Lovelace")
    obs["active_tenant_b"] = app.has_text(app.tree(), "Active tenant: Tenant B")
    app.quit()

    # ---- launch 2 ----
    app.launch()
    app.open_settings()
    obs["restored"] = wait_account(app, "Signed in to Academy as") and shows(account(app), "Ada Lovelace")
    obs["tree_restored"] = save_tree("restored", app.tree())
    before = len(endpoint.requests())
    press(app, "Sign out of Academy")
    obs["signed_out"] = wait_account(app, "Not signed in to Academy.")
    time.sleep(1.5)
    obs["academy_items_after_sign_out"] = fx.keychain_accounts(ACADEMY_SERVICE)
    obs["revocations"] = [r for r in endpoint.requests()[before:] if r["path"] == "/oauth/revoke"]

    # Cancel while waiting; a later grant does not sign in.
    endpoint.mode(approval="pending")
    press(app, "Sign in to Academy")
    wait_account(app, "WDJB-MJHT")
    press(app, "Cancel sign-in")
    obs["cancelled"] = wait_account(app, "Not signed in to Academy.")
    endpoint.mode(approval="granted")
    time.sleep(3)
    obs["cancel_stays_out"] = shows(account(app), "Not signed in to Academy.")
    obs["academy_items_after_cancel"] = fx.keychain_accounts(ACADEMY_SERVICE)

    # Declined in the browser.
    endpoint.mode(approval="denied")
    press(app, "Sign in to Academy")
    obs["denied"] = wait_account(app, "Sign-in was declined in the browser.") and any(
        n.get("role") == "AXButton" and n.get("label") == "Sign in again" for n in account(app))
    obs["academy_items_after_denied"] = fx.keychain_accounts(ACADEMY_SERVICE)
    obs["tree_denied"] = save_tree("denied", app.tree())

    # Unreachable endpoint.
    endpoint.stop()
    press(app, "Sign in again")
    obs["offline"] = wait_account(app, "Academy could not be reached", 20)
    obs["academy_items_after_offline"] = fx.keychain_accounts(ACADEMY_SERVICE)
    obs["tree_offline"] = save_tree("offline", app.tree())
    endpoint.start()

    # 200% zoom and minimum size.
    for _ in range(10):
        app.keys("=", "command down")
    obs["zoom"] = reach(app, 2)
    app.keys("0", "command down")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{900, 600}}'])
    time.sleep(0.8)
    obs["min_size"] = reach(app, 1)
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{1280, 820}}'])

    # Sign in again, then reset local data.
    endpoint.mode(approval="granted", displayName="Ada Lovelace")
    press(app, "Sign in again")
    obs["signed_in_again"] = wait_account(app, "Signed in to Academy as")
    obs["academy_items_before_reset"] = fx.keychain_accounts(ACADEMY_SERVICE)
    press(app, "Reset local data…")
    app.wait_for(lambda t: app.node(t, "AXButton", "Confirm reset") is not None)
    press(app, "Confirm reset")
    app.wait_for(lambda t: app.has_text(t, "No tenants yet"))
    # After Reset local data the one presentation shows the first-run wizard again (no Settings page to read the
    # account status from): the wizard showing is the signed-out state here.
    obs["reset_signed_out"] = wait_account(app, "Not signed in to Academy.") or v2s.wizard_showing(app, timeout=5)
    obs["academy_items_after_reset"] = fx.keychain_accounts(ACADEMY_SERVICE)
    trees = [os.path.join(NATIVE, n) for n in os.listdir(NATIVE) if n.startswith("tree-")]
    obs["secret_in_trees"] = [rel(p) for p in trees if any(s in open(p).read() for s in SECRETS)]
    obs["data_root_hits"] = data_root_hits()
    app.quit()
    endpoint.stop()
    obs["requests"] = endpoint.requests()
    shutil.rmtree(work, ignore_errors=True)
    with open(os.path.join(NATIVE, "observations.json"), "w") as f:
        json.dump({"observations": obs, "log": LOG}, f, indent=1)
    return evaluate(prepared, obs)


def reach(app, zoom):
    """Tabs until an Academy sign-in button has focus; it must lie inside the
    window and end above the 60 CSS px bottom fade."""
    window = next(n for n in app.tree() if n.get("role") == "AXWindow")
    bottom = window["pos"][1] + window["size"][1]
    found = None
    for _ in range(60):
        app.keycode(48)
        f = app.focused(app.tree())
        if f and f.get("role") == "AXButton" and f.get("label") in ("Sign in again", "Sign in to Academy"):
            found = {"label": f["label"], "pos": f["pos"], "size": f["size"]}
            break
    clear = bool(found) and found["pos"][1] >= window["pos"][1] and found["pos"][1] + found["size"][1] <= bottom - 60 * zoom
    return {"zoom": zoom, "window": {"pos": window["pos"], "size": window["size"]}, "found": found, "clear": clear}


def evaluate(prepared, o):
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C04"]}
    obs_path = rel(os.path.join(NATIVE, "observations.json"))
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [obs_path] + [e for e in evidence if e]})

    add("C04-N01", o["group"] and o["keyboard_reached_sign_in"],
        f"'Academy account' group with the tenant-independence text and 'Not signed in to Academy.': {o['group']}; Tab reached 'Sign in to Academy': {o['keyboard_reached_sign_in']}",
        [o["tree_signed_out"]])
    page = f"{o['origin']}/activate?user_code=WDJB-MJHT"
    add("C04-N02", o["awaiting"] and o["code_label"] and o["page_shown"] and o["opened"] == [page],
        f"code shown: {o['awaiting']}; accessible name 'Sign-in code W D J B …': {o['code_label']}; issuer page shown: {o['page_shown']}; pages handed to the browser: {o['opened']}",
        [o["tree_awaiting"]])
    reqs = o["requests_sign_in"]
    identity = [r for r in reqs if r["path"].startswith("/oauth") or r["path"] in ("/api/desktop/config", "/.well-known/oauth-authorization-server")]
    device = [r for r in reqs if r["path"] == "/oauth/device"]
    polls = [r for r in reqs if r["path"] == "/oauth/token"]
    me = [r for r in reqs if r["path"] == "/api/trpc/desktop.me"]
    public = (len(device) == 1 and device[0]["fields"] == ["client_id", "scope"] and device[0]["clientId"] == "desktop_public_client"
              and polls and all(p["grantType"] == "urn:ietf:params:oauth:grant-type:device_code" for p in polls)
              and not any(r["clientSecret"] or r["authorization"] for r in identity) and me and all(r["authorization"] for r in me))
    add("C04-N03", o["signed_in"] and public and len(o["academy_items"]) == 1 and o["scan_items_added"] == [] and not o["secret_in_trees"] and not o["data_root_hits"],
        f"'Signed in to Academy as Ada Lovelace': {o['signed_in']}; public-client device flow (device fields {device[0]['fields'] if device else None}, {len(polls)} polls, no secret or Authorization on identity requests, desktop.me with Authorization): {public}; "
        f"learner namespace items {o['academy_items']}; scan namespace items added by the sign-in {o['scan_items_added']} (present before: {o.get('scan_items_before')}); token in AX snapshots {o['secret_in_trees']}; in data root {o['data_root_hits']}",
        [o["tree_signed_in"]])
    add("C04-N04", o["restored"], f"after quit and relaunch 'Signed in to Academy as Ada Lovelace': {o['restored']}", [o["tree_restored"]])
    add("C04-N05", o["after_tenant_switch"] and o["active_tenant_b"],
        f"Tenant B active: {o['active_tenant_b']}; Academy still signed in as Ada Lovelace: {o['after_tenant_switch']}", [])
    revoked = [r for r in o["revocations"] if r["tokenHint"] == "refresh_token" and r["revokedToken"] == "fixture-refresh-1"]
    add("C04-N06", o["signed_out"] and o["academy_items_after_sign_out"] == [] and len(revoked) >= 1,
        f"'Not signed in to Academy.': {o['signed_out']}; learner namespace items {o['academy_items_after_sign_out']}; refresh-token revocations {len(revoked)}", [])
    add("C04-N07", o["denied"] and o["academy_items_after_denied"] == [],
        f"'Sign-in was declined in the browser.' with 'Sign in again': {o['denied']}; learner namespace items {o['academy_items_after_denied']}", [o["tree_denied"]])
    add("C04-N08", o["offline"] and o["academy_items_after_offline"] == [],
        f"'Academy could not be reached' with the endpoint stopped: {o['offline']}; learner namespace items {o['academy_items_after_offline']}", [o["tree_offline"]])
    add("C04-N09", o["cancelled"] and o["cancel_stays_out"] and o["academy_items_after_cancel"] == [],
        f"cancelled to 'Not signed in to Academy.': {o['cancelled']}; still signed out 3 s after a grant became available: {o['cancel_stays_out']}; learner namespace items {o['academy_items_after_cancel']}", [])
    add("C04-N10", o["keyboard_reached_sign_in"] and o["keyboard_open_and_cancel"] and o["awaiting"] and o["status_regions"] > 0,
        f"Tab reached 'Sign in to Academy' and Space started sign-in: {o['keyboard_reached_sign_in'] and o['awaiting']}; Tab reached 'Open sign-in page' and 'Cancel sign-in': {o['keyboard_open_and_cancel']}; live status regions in the group: {o['status_regions']}",
        [o["tree_awaiting"]])
    add("C04-N11", o["zoom"]["clear"] and o["min_size"]["clear"],
        f"200% zoom: {o['zoom']['found']} in window {o['zoom']['window']}, clear: {o['zoom']['clear']}; minimum size: {o['min_size']['found']} in window {o['min_size']['window']}, clear: {o['min_size']['clear']}", [])
    add("C04-N12", o["signed_in_again"] and len(o["academy_items_before_reset"]) == 1 and o["reset_signed_out"] and o["academy_items_after_reset"] == [],
        f"signed in before reset with {len(o['academy_items_before_reset'])} learner item(s); after reset 'Not signed in to Academy.': {o['reset_signed_out']}; learner namespace items {o['academy_items_after_reset']}", [])

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1",
        "subject": "C04",
        "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (tools/native-acceptance/ax.swift, System Events keystrokes) against the synthetic Academy + Clerk endpoint tools/native-acceptance/academy_server.py; tools/native-acceptance/acceptance_c04.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "The identity provider and Academy are a local synthetic fixture with a disposable test CA trusted only by the fixture build; the hosted Academy/Clerk registration is qualified in C25 (L11).",
            "Browser approval is simulated by switching the synthetic token endpoint from pending to granted or denied; the fixture build records the verification page instead of launching a browser. Production opens it with /usr/bin/open.",
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
