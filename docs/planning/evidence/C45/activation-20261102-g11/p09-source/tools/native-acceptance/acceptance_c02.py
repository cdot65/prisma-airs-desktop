#!/usr/bin/env python3
"""C02 native acceptance: drive the prepared fixture app through macOS
Accessibility and evaluate the C02 inventory (docs/architecture/native-checks.json).

Run from the repository root after `xtask native --chunk C02` prepared the
fixture bundle. Requires the macOS Accessibility grant for the invoking
terminal. The fixture build keeps its data in its own Application Support
directory (bundle identifier io.cdot.prisma-airs-desktop.v2.fixture) and its
credentials in a dedicated fixture test keychain there; this script starts
from an empty fixture root and removes it at the end. It uses only synthetic
keys, never reads a stored secret back (Keychain items are inspected by
attributes only), and never touches the login keychain or v1 data (v1's
Application Support metadata is compared before and after, read-only).

Launches:
1. fresh: keyboard walk, create a tenant, save a Keychain key, set Dark;
2. relaunch (startup network sampled): persistence, a session-only key, the
   locked and unlocked fixture keychain;
3. relaunch: session key gone, reviewed endpoint change, native-sheet profile
   import, accessibility names/alert, 200% zoom and minimum size, reset;
4. relaunch with a corrupt state document: notice, refusal, reset.
Every check's result is computed from recorded observations.
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

ROOT = os.getcwd()
# A phase review runs every member chunk's checks into the phase's own
# evidence (AIRS_NATIVE_SUBJECT=Pxx); tools/native-acceptance/merge_phase.py
# then combines the per-chunk records.
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C02")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C02" else "native-C02")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C02" else "native-acceptance-C02.json")
TOOLS = os.path.join(ROOT, "tools/native-acceptance")
PROCESS = "Fixture.app/Contents/MacOS/prisma-airs-desktop-v2"
HOME = os.path.expanduser("~")
DATA = os.path.join(HOME, "Library/Application Support/io.cdot.prisma-airs-desktop.v2.fixture")
KEYCHAIN = os.path.join(DATA, "fixture-credentials.keychain-db")
KEYCHAIN_PASSWORD = "prisma-airs-desktop-v2-fixture-keychain"
SERVICE = "io.cdot.prisma-airs-desktop.v2.fixture.scan"
V1_DATA = os.path.join(HOME, "Library/Application Support/Prisma AIRS Desktop")
CAPTURES = os.path.join(subprocess.run(["getconf", "DARWIN_USER_TEMP_DIR"], capture_output=True, text=True).stdout.strip(),
                        "prisma-airs-v2-fixture-captures")
KEY_A = "synthetic-c02-native-key-alpha"
KEY_B = "synthetic-c02-native-key-bravo"
KEY_C = "synthetic-c02-native-key-charlie"
EMBEDDED = "synthetic-c02-embedded-secret"
CUSTOM = "https://airs-proxy.example.com:8443"
AX = None
LOG = []


def sh(argv, **kw):
    return subprocess.run(argv, capture_output=True, text=True, **kw)


def sha256(path):
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def rel(path):
    return os.path.relpath(path, ROOT)


def note(what, **data):
    LOG.append({"t": time.strftime("%H:%M:%S"), "step": what, **data})


# ---------- app control ----------

def app_pid():
    out = sh(["pgrep", "-f", PROCESS]).stdout.split()
    return int(out[0]) if out else None


def quit_app():
    pid = app_pid()
    if pid:
        try:
            keys("q", "command down", pid=pid)
        except RuntimeError:
            pass
        for _ in range(40):
            time.sleep(0.25)
            if not app_pid():
                break
    sh(["pkill", "-f", PROCESS])
    time.sleep(0.5)


def launch(bundle, sample_network=False):
    quit_app()
    before = webkit_pids()
    sh(["open", "-n", bundle])
    lines, sockets = [], 0
    for i in range(30 if sample_network else 12):
        pid = app_pid()
        if sample_network:
            new = sorted(webkit_pids() - before)
            lines.append(f"t={i / 2:.1f} app={pid} webkit={','.join(new)}")
            for p in ([str(pid)] if pid else []) + new:
                out = sh(["lsof", "-nP", "-a", "-p", p, "-i"]).stdout.splitlines()[1:]
                sockets += len(out)
                lines += out
        time.sleep(0.5)
    pid = app_pid()
    ax("tree", pid)  # announces the AX client so WebKit builds its tree
    time.sleep(1)
    return pid, (sockets, lines, len(webkit_pids() - before))


def webkit_pids():
    return set(sh(["pgrep", "-f", "com.apple.WebKit"]).stdout.split())


def ax(command, pid, *args):
    out = sh([AX, command, str(pid), *[str(a) for a in args]])
    if command == "tree":
        return json.loads(out.stdout or "[]")
    if out.returncode != 0:
        raise RuntimeError(f"ax {command} {args}: {out.stderr.strip()}")
    return out.stdout


def front(pid):
    """Brings the fixture app forward and confirms it is frontmost; keystrokes
    are never sent unless it is (they would reach another application)."""
    script = (f'tell application "System Events"\nset p to first application process whose unix id is {pid}\n'
              'set frontmost of p to true\ndelay 0.2\nreturn frontmost of p\nend tell')
    for _ in range(5):
        if sh(["osascript", "-e", script]).stdout.strip() == "true":
            return
        time.sleep(0.3)
    raise RuntimeError(f"the fixture app (pid {pid}) is not frontmost; refusing to send keystrokes")


def keys(text, modifiers=None, pid=None):
    if pid is None:
        raise RuntimeError("keystrokes need the fixture pid")
    front(pid)
    using = f" using {{{modifiers}}}" if modifiers else ""
    escaped = text.replace("\\", "\\\\").replace('"', '\\"')
    sh(["osascript", "-e", f'tell application "System Events" to keystroke "{escaped}"{using}'])
    time.sleep(0.4)


def keycode(code, pid=None, modifiers=None):
    if pid is None:
        raise RuntimeError("keystrokes need the fixture pid")
    front(pid)
    using = f" using {{{modifiers}}}" if modifiers else ""
    sh(["osascript", "-e", f'tell application "System Events" to key code {code}{using}'])
    time.sleep(0.35)


def menu(pid, *path):
    front(pid)
    chain = f'menu item "{path[-1]}"'
    for item in reversed(path[1:-1]):
        chain += f' of menu 1 of menu item "{item}"'
    chain += f' of menu 1 of menu bar item "{path[0]}" of menu bar 1'
    sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {pid}) to click {chain}'])
    time.sleep(0.8)


def texts(tree):
    return [n.get("label") or "" for n in tree] + [str(n.get("value")) for n in tree if n.get("value") is not None]


def has_text(tree, fragment):
    return any(fragment in t for t in texts(tree))


def wait_for(pid, predicate, timeout=12.0):
    deadline = time.time() + timeout
    while True:
        tree = ax("tree", pid)
        if predicate(tree):
            return tree, True
        if time.time() > deadline:
            return tree, False
        time.sleep(0.4)


def node(tree, role, prefix, nth=0):
    found = [n for n in tree if n.get("role") == role and (n.get("label") or "").startswith(prefix)]
    return found[nth] if nth < len(found) else None


def focused(tree):
    return next((n for n in tree if n.get("focused") and n.get("role") not in ("AXWindow", "AXWebArea", "AXGroup", "AXScrollArea")), None)


def save_tree(name, tree):
    path = os.path.join(NATIVE, f"tree-{name}.json")
    with open(path, "w") as f:
        json.dump(tree, f, indent=0)
    return rel(path)


def capture(pid, name):
    before = set(glob.glob(os.path.join(CAPTURES, "capture-*.png")))
    menu(pid, "Fixture", "Capture Window")
    time.sleep(1.2)
    new = sorted(set(glob.glob(os.path.join(CAPTURES, "capture-*.png"))) - before)
    if not new:
        return None
    dest = os.path.join(NATIVE, "captures", f"{name}.png")
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    shutil.copy(new[-1], dest)
    return rel(dest)


# ---------- fixture keychain and data (attributes/metadata only) ----------

def keychain_items():
    """Accounts of fixture-namespace items (attributes only; no secrets)."""
    if not os.path.exists(KEYCHAIN):
        return []
    out = sh(["security", "dump-keychain", KEYCHAIN]).stdout
    accounts, account, service = [], None, None
    for line in out.splitlines() + ["keychain:"]:
        line = line.strip()
        if line.startswith("keychain:") or line.startswith("class:"):
            if service == SERVICE and account:
                accounts.append(account)
            account, service = None, None
        if line.startswith('"acct"<blob>='):
            account = line.split("=", 1)[1].strip('"')
        if line.startswith('"svce"<blob>='):
            service = line.split("=", 1)[1].strip('"')
    return sorted(accounts)


def v1_metadata():
    entries = []
    for base, _, files in os.walk(V1_DATA):
        for name in files:
            path = os.path.join(base, name)
            try:
                st = os.lstat(path)
            except OSError:
                continue
            entries.append([os.path.relpath(path, V1_DATA), st.st_size, int(st.st_mtime)])
    return sorted(entries)


def clean_fixture_root():
    if os.path.exists(KEYCHAIN):
        sh(["security", "delete-keychain", KEYCHAIN])
    if os.path.isdir(DATA):
        shutil.rmtree(DATA)


# ---------- flows ----------

def open_settings(pid):
    keys(",", "command down", pid=pid)
    tree, _ = wait_for(pid, lambda t: node(t, "AXHeading", "Settings") is not None)
    return tree


def tab_walk(pid, steps=60):
    order = []
    for _ in range(steps):
        keycode(48, pid)
        f = focused(ax("tree", pid))
        order.append(None if f is None else {"role": f["role"], "subrole": f.get("subrole"), "label": f.get("label")})
    return order


def field_text(pid, role, label, nth=0):
    found = [n for n in ax("tree", pid) if n.get("role") == role and (n.get("label") or "").startswith(label)]
    return found[nth] if nth < len(found) else None


def type_into(pid, role, label, text, nth=0, secure=False):
    """Types `text` into a field and verifies what arrived (a secure field by
    its bullet count); retypes over a selection if keystrokes were dropped."""
    for attempt in range(3):
        ax("focus", pid, role, label, nth)
        time.sleep(0.4)
        if attempt:
            keys("a", "command down", pid=pid)
            keycode(51, pid)  # delete the selection
        keys(text, pid=pid)
        time.sleep(0.3)
        field = field_text(pid, role, label, nth) or {}
        value = str(field.get("value") or "")
        if (len(value) == len(text) and set(value) <= {"•"}) if secure else value == text:
            note("typed", field=label, attempts=attempt + 1)
            return field
    raise RuntimeError(f"could not type into {label} reliably")


def create_tenant(pid, label, tsg, region_letter):
    type_into(pid, "AXTextField", "Label", label)
    type_into(pid, "AXTextField", "TSG ID (optional)", tsg)
    ax("focus", pid, "AXPopUpButton", "Region")
    keys(region_letter, pid=pid)
    tree = ax("tree", pid)
    region = node(tree, "AXPopUpButton", "Region")
    ax("focus", pid, "AXTextField", "TSG ID (optional)")
    keycode(36, pid)  # Return submits the form
    tree, ok = wait_for(pid, lambda t: has_text(t, f"Active tenant:") and node(t, "AXButton", f"Delete {label}") is not None)
    return tree, ok, region.get("value") if region else None


def save_key(pid, text, session=False):
    if session:
        ax("press", pid, "AXRadioButton", "For this session only")
    secure = type_into(pid, "AXTextField", "AIRS API key", text, secure=True)
    keycode(36, pid)
    return secure


def main():
    try:
        return run()
    finally:
        # Never leave the fixture app running or its fixture data behind.
        sh(["pkill", "-f", PROCESS])
        if os.path.exists(KEYCHAIN):
            sh(["security", "delete-keychain", KEYCHAIN])
        if os.path.isdir(DATA):
            shutil.rmtree(DATA)


def run():
    global AX
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")
    if sha256(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c02-acceptance-")
    AX = os.path.join(work, "ax")
    built = sh(["swiftc", "-O", os.path.join(TOOLS, "ax.swift"), "-o", AX])
    if built.returncode != 0:
        raise SystemExit(f"ax helper did not build: {built.stderr}")
    if os.path.isdir(NATIVE):
        shutil.rmtree(NATIVE)
    os.makedirs(NATIVE)
    if os.path.isdir(CAPTURES):
        shutil.rmtree(CAPTURES)
    quit_app()
    clean_fixture_root()
    v1_before = v1_metadata()
    registry = os.path.join(work, "tenants.json")
    with open(registry, "w") as f:
        json.dump({"version": 1, "active": "prod", "tenants": [
            {"name": "Imported Prod", "configPath": os.path.join(work, "missing-credentials.json"), "tsgId": "1111111111"},
            {"name": "Imported Lab", "configPath": os.path.join(work, "lab.json"), "tsgId": "2222", "airsApiKey": EMBEDDED},
        ]}, f)
    obs = {}

    # ---- launch 1: fresh ----
    pid, _ = launch(bundle)
    tree = open_settings(pid)
    obs["empty_list"] = has_text(tree, "No tenants yet")
    obs["tab_order_empty"] = tab_walk(pid, 30)
    tree, created, region = create_tenant(pid, "Prod EU", "1234567890", "E")
    obs["created"] = created
    obs["region_selected"] = region
    obs["created_row"] = has_text(tree, "Prod EU (active)") and has_text(tree, "Europe (Germany) region · scan key missing")
    obs["tree_created"] = save_tree("created", tree)
    secure = save_key(pid, KEY_A)
    obs["secure_field"] = secure
    tree, saved = wait_for(pid, lambda t: has_text(t, "Scan key saved in the Keychain."))
    obs["keychain_saved"] = saved
    obs["items_after_save"] = keychain_items()
    menu(pid, "View", "Appearance", "Dark")
    obs["capture_saved"] = capture(pid, "keychain-key-saved")
    obs["tree_saved"] = save_tree("keychain-saved", tree)
    obs["key_a_in_tree"] = KEY_A in json.dumps(tree)
    note("launch 1 done", **{k: obs[k] for k in ("created", "keychain_saved", "items_after_save")})
    quit_app()

    # ---- launch 2: persistence, session key, locked store ----
    pid, network = launch(bundle, sample_network=True)
    obs["network"] = {"sockets": network[0], "webkit": network[2]}
    with open(os.path.join(NATIVE, "startup-network-samples.log"), "w") as f:
        f.write("\n".join(network[1]) + "\n")
    tree = open_settings(pid)
    tree, restored = wait_for(pid, lambda t: has_text(t, "Prod EU (active)") and has_text(t, "Scan key saved in the Keychain."))
    dark = node(tree, "AXRadioButton", "Dark")
    obs["restored"] = restored
    obs["dark_restored"] = bool(dark and dark.get("value") == 1)
    obs["tree_restored"] = save_tree("restored", tree)
    tree, created_b, _ = create_tenant(pid, "Lab Session", "42", "I")
    ax("press", pid, "AXButton", "Make Lab Session active")
    tree, switched = wait_for(pid, lambda t: has_text(t, "Active tenant: Lab Session"))
    obs["switched_to_b"] = created_b and switched
    items_before_session = keychain_items()
    save_key(pid, KEY_B, session=True)
    tree, held = wait_for(pid, lambda t: has_text(t, "Scan key held for this session only."))
    obs["session_held"] = held
    obs["items_after_session"] = keychain_items()
    obs["session_added_no_item"] = obs["items_after_session"] == items_before_session
    obs["key_b_in_tree"] = KEY_B in json.dumps(tree)
    obs["tree_session"] = save_tree("session-held", tree)
    obs["names"] = {
        "make_active": node(tree, "AXButton", "Make Prod EU active") is not None,
        "delete": node(tree, "AXButton", "Delete Lab Session") is not None and node(tree, "AXButton", "Delete Prod EU") is not None,
    }
    # Since C08 the active tenant's management client form sits between
    # the scan key and the destination, adding its stops to the walk.
    obs["tab_order_full"] = tab_walk(pid, 60)
    # Locked store: lock the fixture keychain externally, then try to save.
    ax("press", pid, "AXButton", "Make Prod EU active")
    wait_for(pid, lambda t: has_text(t, "Active tenant: Prod EU"))
    items_before_lock = keychain_items()
    menu(pid, "Fixture", "Lock Credential Store")
    obs["lock"] = "User interaction is not allowed" in sh(["security", "show-keychain-info", KEYCHAIN]).stderr
    save_key(pid, KEY_C)
    tree, locked = wait_for(pid, lambda t: has_text(t, "The Keychain is locked."))
    windows = [n for n in tree if n.get("role") in ("AXWindow", "AXSheet")]
    obs["locked_reported"] = locked
    obs["locked_no_prompt"] = len(windows) == 1
    obs["locked_tree"] = save_tree("locked", tree)
    obs["capture_locked"] = capture(pid, "keychain-locked")
    obs["items_while_locked"] = keychain_items()
    menu(pid, "Fixture", "Unlock Credential Store")
    # Informational only: another process's view of the lock state is not
    # what the running app sees (see the retained trial run).
    obs["cli_view_after_unlock"] = sh(["security", "show-keychain-info", KEYCHAIN]).returncode == 0
    save_key(pid, KEY_C)
    tree, resaved = wait_for(pid, lambda t: has_text(t, "Scan key saved in the Keychain."))
    time.sleep(1.5)
    obs["resaved_after_unlock"] = resaved
    obs["items_before_lock"] = items_before_lock
    obs["items_after_unlock"] = keychain_items()
    note("launch 2 done", restored=restored, held=held, locked=locked, resaved=resaved)
    quit_app()

    # ---- launch 3: session gone, endpoint, import, a11y, zoom, reset ----
    pid, _ = launch(bundle)
    tree = open_settings(pid)
    tree, _ = wait_for(pid, lambda t: has_text(t, "Lab Session"))
    obs["session_gone"] = has_text(tree, "India region · scan key missing")
    obs["prod_still_saved"] = has_text(tree, "Scan key saved in the Keychain.")
    obs["tree_relaunch_2"] = save_tree("after-second-relaunch", tree)
    items_before_endpoint = keychain_items()
    ax("press", pid, "AXButton", "Change destination…")
    wait_for(pid, lambda t: node(t, "AXButton", "Review change") is not None)
    ax("press", pid, "AXRadioButton", "Custom HTTPS endpoint", 0)
    tree, _ = wait_for(pid, lambda t: node(t, "AXTextField", "Endpoint origin") is not None)
    type_into(pid, "AXTextField", "Endpoint origin", CUSTOM)
    ax("press", pid, "AXButton", "Review change")
    tree, reviewed = wait_for(pid, lambda t: has_text(t, f"Review: scan requests will go to {CUSTOM}"))
    obs["review_shown"] = reviewed and has_text(tree, "instead of https://service-de.api.aisecurity.paloaltonetworks.com") and has_text(
        tree, "The current scan key will be removed")
    obs["tree_review"] = save_tree("endpoint-review", tree)
    obs["capture_review"] = capture(pid, "endpoint-review")
    ax("press", pid, "AXButton", "Change destination")
    tree, changed = wait_for(pid, lambda t: has_text(t, f"go only to {CUSTOM}") and has_text(t, "No scan key."))
    time.sleep(1.5)
    obs["endpoint_changed"] = changed
    obs["items_before_endpoint"] = items_before_endpoint
    obs["items_after_endpoint"] = keychain_items()
    # Import through the native sheet.
    ax("press", pid, "AXButton", "Choose profile file…")
    tree, sheet = wait_for(pid, lambda t: any(n.get("role") == "AXSheet" for n in t))
    obs["sheet_attached"] = sheet
    obs["capture_sheet_tree"] = save_tree("import-sheet", tree)
    keys("g", "command down, shift down", pid=pid)
    time.sleep(0.8)
    keys(registry, pid=pid)
    time.sleep(0.6)
    keycode(36, pid)
    time.sleep(1.0)
    keycode(36, pid)
    tree, previewed = wait_for(pid, lambda t: has_text(t, "tenants.json") and has_text(t, "Imported Lab"), timeout=15)
    obs["preview"] = previewed
    obs["preview_counts"] = has_text(tree, "Not imported: 1 secret field(s), 2 file reference(s), 0 other field(s).")
    obs["preview_secret_absent"] = EMBEDDED not in json.dumps(tree)
    obs["preview_path_absent"] = os.path.join(work, "lab.json") not in json.dumps(tree)
    obs["tree_preview"] = save_tree("import-preview", tree)
    obs["capture_preview"] = capture(pid, "import-preview")
    ax("press", pid, "AXButton", "Import 2 profiles")
    tree, imported = wait_for(pid, lambda t: node(t, "AXButton", "Delete Imported Lab") is not None and node(t, "AXButton", "Delete Imported Prod") is not None)
    obs["imported"] = imported
    obs["imported_keyless"] = sum("scan key missing" in t for t in texts(tree)) >= 3
    obs["tree_imported"] = save_tree("imported", tree)
    # A refusal is exposed as an alert (invalid custom origin when adding).
    ax("press", pid, "AXRadioButton", "Custom HTTPS endpoint", 0)
    tree, _ = wait_for(pid, lambda t: node(t, "AXTextField", "Endpoint origin") is not None)
    type_into(pid, "AXTextField", "Label", "Plain HTTP")
    type_into(pid, "AXTextField", "Endpoint origin", "http://plain.example.com")
    ax("focus", pid, "AXTextField", "Label")
    keycode(36, pid)
    tree, alerted = wait_for(pid, lambda t: has_text(t, "Check the entered values and try again."))
    alert_nodes = [n for n in tree if "Alert" in str(n.get("subrole")) or n.get("role") == "AXApplicationAlert"]
    obs["alert"] = alerted
    obs["alert_role"] = [(n.get("role"), n.get("subrole")) for n in alert_nodes][:3]
    obs["tree_alert"] = save_tree("refusal-alert", tree)
    # 200% zoom and minimum size: the scan-key field and Save action.
    for _ in range(10):
        keys("=", "command down", pid=pid)
    obs["zoom"] = reach_key_field(pid, 2)
    obs["capture_zoom"] = capture(pid, "zoom-200-scan-key")
    keys("0", "command down", pid=pid)
    sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {pid}) to set size of window 1 to {{900, 600}}'])
    time.sleep(0.8)
    obs["min_size"] = reach_key_field(pid, 1)
    obs["capture_min"] = capture(pid, "min-size-scan-key")
    sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {pid}) to set size of window 1 to {{1280, 820}}'])
    # Reset (two-step).
    ax("press", pid, "AXButton", "Reset local data…")
    wait_for(pid, lambda t: node(t, "AXButton", "Confirm reset") is not None)
    ax("press", pid, "AXButton", "Confirm reset")
    tree, reset = wait_for(pid, lambda t: has_text(t, "No tenants yet"))
    time.sleep(1)
    obs["reset"] = reset
    obs["items_after_reset"] = keychain_items()
    obs["state_after_reset"] = os.path.exists(os.path.join(DATA, "state.json"))
    obs["tree_reset"] = save_tree("reset", tree)
    quit_app()

    # ---- launch 4: corrupt state document ----
    corrupt = b"{not json at all"
    with open(os.path.join(DATA, "state.json"), "wb") as f:
        f.write(corrupt)
    pid, _ = launch(bundle)
    tree = open_settings(pid)
    tree, noticed = wait_for(pid, lambda t: has_text(t, "Local desktop data could not be read"))
    obs["corrupt_notice"] = noticed
    type_into(pid, "AXTextField", "Label", "Blocked")
    ax("focus", pid, "AXTextField", "Label")
    keycode(36, pid)
    tree, refused = wait_for(pid, lambda t: has_text(t, "Local desktop data or the credential store is not available."))
    obs["corrupt_refused"] = refused
    obs["corrupt_untouched"] = open(os.path.join(DATA, "state.json"), "rb").read() == corrupt
    obs["tree_corrupt"] = save_tree("corrupt", tree)
    obs["capture_corrupt"] = capture(pid, "corrupt-notice")
    ax("press", pid, "AXButton", "Reset local data…")
    wait_for(pid, lambda t: node(t, "AXButton", "Confirm reset") is not None)
    ax("press", pid, "AXButton", "Confirm reset")
    tree, recovered = wait_for(pid, lambda t: not has_text(t, "Local desktop data could not be read") and has_text(t, "No tenants yet"))
    obs["corrupt_recovered"] = recovered
    quit_app()
    v1_after = v1_metadata()
    obs["v1_unchanged"] = v1_before == v1_after
    obs["v1_entries"] = len(v1_before)
    shutil.rmtree(work, ignore_errors=True)

    with open(os.path.join(NATIVE, "observations.json"), "w") as f:
        json.dump({"observations": obs, "log": LOG}, f, indent=1)
    return evaluate(prepared, obs)


def reach_key_field(pid, zoom):
    """Tabs until the scan-key field has focus, then to Save key; both must lie
    inside the window and end above the 60 CSS px bottom fade."""
    window = next(n for n in ax("tree", pid) if n.get("role") == "AXWindow")
    bottom = window["pos"][1] + window["size"][1]
    found = {}
    for _ in range(80):
        keycode(48, pid)
        tree = ax("tree", pid)
        f = focused(tree)
        if f and f.get("label") == "AIRS API key" and "field" not in found:
            found["field"] = {"pos": f["pos"], "size": f["size"]}
        if f and f.get("role") == "AXButton" and (f.get("label") or "").startswith("Save key") and "field" in found:
            found["save"] = {"pos": f["pos"], "size": f["size"]}
            break
    ok = all(k in found and found[k]["pos"][1] >= window["pos"][1]
             and found[k]["pos"][1] + found[k]["size"][1] <= bottom - 60 * zoom for k in ("field", "save"))
    return {"zoom": zoom, "window": {"pos": window["pos"], "size": window["size"]}, "found": found, "clear": ok}


def evaluate(prepared, o):
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C02"]}
    obs_path = rel(os.path.join(NATIVE, "observations.json"))
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [obs_path] + [e for e in evidence if e]})

    labels = [s and s["label"] for s in o["tab_order_empty"]]
    want = ["Label", "TSG ID (optional)", "AIRS region", "Region", "Add tenant", "Choose profile file…", "Reset local data…"]
    seq = [w for w in want if w in labels]
    in_order = seq == want and [labels.index(w) for w in want] == sorted(labels.index(w) for w in want)
    add("C02-N01", o["empty_list"] and o["created"] and o["created_row"] and o["region_selected"] == "Europe (Germany)" and in_order,
        f"empty list shown: {o['empty_list']}; Tab order reached {seq} in order: {in_order}; tenant created by keyboard (Return in the form): {o['created']}; "
        f"region pop-up value '{o['region_selected']}'; list row 'Prod EU (active)' with 'Europe (Germany) region · scan key missing': {o['created_row']}",
        [o["tree_created"]])
    sf = o["secure_field"] or {}
    secure_ok = sf.get("subrole") == "AXSecureTextField" and KEY_A not in str(sf.get("value"))
    add("C02-N02", secure_ok and o["keychain_saved"] and len(o["items_after_save"]) == 1 and o["items_after_save"][0].startswith("cr-") and not o["key_a_in_tree"],
        f"field role {sf.get('role')}/{sf.get('subrole')} with AX value {sf.get('value')!r} (not the key); status 'Scan key saved in the Keychain.': {o['keychain_saved']}; "
        f"fixture-namespace items {o['items_after_save']} (attributes only); key absent from the AX snapshot: {not o['key_a_in_tree']}",
        [o["tree_saved"], o["capture_saved"]])
    add("C02-N03", o["restored"] and o["dark_restored"],
        f"after quit and relaunch: 'Prod EU (active)' with 'Scan key saved in the Keychain.': {o['restored']}; Dark appearance radio checked: {o['dark_restored']}",
        [o["tree_restored"]])
    add("C02-N04", o["switched_to_b"] and o["session_held"] and o["session_added_no_item"] and not o["key_b_in_tree"] and o["session_gone"],
        f"'Lab Session' created and made active: {o['switched_to_b']}; 'Scan key held for this session only.': {o['session_held']}; Keychain items unchanged {o['items_after_session']}: "
        f"{o['session_added_no_item']}; after relaunch 'India region · scan key missing': {o['session_gone']}",
        [o["tree_session"], o["tree_relaunch_2"]])
    endpoint_deleted = len(o["items_before_endpoint"]) == 1 and o["items_after_endpoint"] == []
    add("C02-N05", o["review_shown"] and o["endpoint_changed"] and endpoint_deleted,
        f"review names {CUSTOM} instead of the Europe origin and the key removal: {o['review_shown']}; after confirming, destination {CUSTOM} with 'No scan key.': "
        f"{o['endpoint_changed']}; Keychain items {o['items_before_endpoint']} → {o['items_after_endpoint']}",
        [o["tree_review"], o["capture_review"]])
    add("C02-N06", o["sheet_attached"] and o["preview"] and o["preview_counts"] and o["preview_secret_absent"] and o["preview_path_absent"] and o["imported"] and o["imported_keyless"],
        f"AXSheet attached to the main window: {o['sheet_attached']}; preview listed 'Imported Prod'/'Imported Lab' from tenants.json: {o['preview']}; "
        f"discarded counts '1 secret field(s), 2 file reference(s), 0 other field(s)': {o['preview_counts']}; embedded secret absent from AX tree: {o['preview_secret_absent']}; "
        f"referenced path absent: {o['preview_path_absent']}; imported keyless tenants: {o['imported'] and o['imported_keyless']}",
        [o["capture_sheet_tree"], o["tree_preview"], o["capture_preview"], o["tree_imported"]])
    rotated = len(o["items_after_unlock"]) == 1 and o["items_after_unlock"] != o["items_before_lock"]
    unchanged_while_locked = o["items_while_locked"] == o["items_before_lock"]
    add("C02-N07", o["lock"] and o["locked_reported"] and o["locked_no_prompt"] and unchanged_while_locked and o["resaved_after_unlock"] and rotated,
        f"fixture keychain locked by the app's own SecKeychainLock (Fixture menu; `security show-keychain-info` then reports it locked): {o['lock']}; 'The Keychain is locked.' reported: {o['locked_reported']}; no extra window or sheet (no prompt): {o['locked_no_prompt']}; "
        f"Keychain items unchanged while locked: {unchanged_while_locked}; after the app unlocked its keychain (Fixture menu) the save succeeded: {o['resaved_after_unlock']}; "
        f"items {o['items_before_lock']} → {o['items_after_unlock']} (retired record cleaned): {rotated}; another process's view after the unlock: unlocked={o['cli_view_after_unlock']} (informational)",
        [o["locked_tree"], o["capture_locked"]])
    add("C02-N08", o["reset"] and o["items_after_reset"] == [] and not o["state_after_reset"] and o["v1_unchanged"],
        f"'No tenants yet' after two-step reset: {o['reset']}; fixture-namespace items after reset {o['items_after_reset']}; state document present: {o['state_after_reset']}; "
        f"v1 Application Support metadata ({o['v1_entries']} files: path, size, mtime) unchanged: {o['v1_unchanged']}",
        [o["tree_reset"]])
    add("C02-N09", o["corrupt_notice"] and o["corrupt_refused"] and o["corrupt_untouched"] and o["corrupt_recovered"],
        f"notice shown: {o['corrupt_notice']}; Add tenant refused with the storage message: {o['corrupt_refused']}; document byte-identical: {o['corrupt_untouched']}; "
        f"recovered by Reset: {o['corrupt_recovered']}",
        [o["tree_corrupt"], o["capture_corrupt"]])
    full = [s and s["label"] for s in o["tab_order_full"]]
    # With a key already present the submit button reads "Replace key".
    reach = ["AIRS API key", "In the Keychain", "Replace key", "Change destination…", "Make Prod EU active", "Delete Prod EU", "Label"]
    reached = [r for r in reach if any(l and l.startswith(r) for l in full)]
    add("C02-N10", reached == reach and o["names"]["make_active"] and o["names"]["delete"] and o["alert"] and bool(o["alert_role"]),
        f"Tab reached {reached} of {reach}; per-tenant named actions present: {o['names']}; refusal message shown: {o['alert']} with AX alert roles {o['alert_role']}",
        [o["tree_session"], o["tree_alert"]])
    add("C02-N11", o["network"]["sockets"] == 0,
        f"{o['network']['sockets']} internet sockets across 30 samples over 15s of the second launch (tenants and a stored Keychain key present) for the app and "
        f"{o['network']['webkit']} new WebKit processes (0.5s polling; DNS via mDNSResponder not attributed).",
        [rel(os.path.join(NATIVE, "startup-network-samples.log"))])
    add("C02-N12", o["zoom"]["clear"] and o["min_size"]["clear"],
        f"200% zoom: {o['zoom']['found']} in window {o['zoom']['window']}, clear of the fade: {o['zoom']['clear']}; minimum size: {o['min_size']['found']} in window "
        f"{o['min_size']['window']}, clear: {o['min_size']['clear']}",
        [o["capture_zoom"], o["capture_min"]])

    hw = sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1",
        "subject": "C02",
        "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (tools/native-acceptance/ax.swift and System Events keystrokes) with fixture WebView self-capture; tools/native-acceptance/acceptance_c02.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "The fixture build stores credentials in a dedicated fixture test keychain (DESIGN); the production login-keychain path is qualified by keychain_access (read-only availability) and, with signing, in C26.",
            "Keychain items are inspected by attributes only (security dump-keychain without -d); stored secrets are never read back.",
            "v1 isolation is evidenced by unchanged Application Support metadata (path, size, mtime); the login keychain is not enumerated.",
            "Spoken VoiceOver output was not recorded; AX roles, names and focus are inspected.",
            "Startup network evidence is sampled lsof output, not a packet capture.",
            "That a referenced credential file is never opened is shown by the core tests and the absent file; file-system tracing was not recorded.",
            "A trial run locked the fixture keychain from another process (`security lock-keychain`); the running app did not observe that lock and its writes still succeeded (macOS 27; retained under native-trial/). C02-N07 therefore locks through the app's own keychain reference (fixture-only menu item), which exercises the app's real locked-store path.",
        ],
    }
    with open(RECORD, "w") as f:
        json.dump(record, f, indent=2)
        f.write("\n")
    for c in checks:
        print(c["result"].upper(), c["id"], c["observation"][:180])
    return 0 if all(c["result"] == "pass" for c in checks) else 1


if __name__ == "__main__":
    sys.exit(main())
