#!/usr/bin/env python3
"""C14 native acceptance: API Client projects and documents through the real
fixture window. A synthetic v1 collection inside a temporary Git repository
is opened through the folder sheet (the fixture build answers it with the
staged folder), edited, saved, protected, conflicted, imported into and
inspected; a staged v1 API Client profile is adopted. The synthetic scan and
management endpoints run only to show that nothing reaches the network.

Run from the repository root after `xtask native-prepare --chunk C14`.
Requires the Accessibility grant. Synthetic data only; the fixture data
root is removed at the end; v1 data and the login keychain are untouched.
"""

import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fixture_app as fx  # noqa: E402
import acceptance_c08 as c8  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C14")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C14" else "native-C14")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C14" else "native-acceptance-C14.json")
SECRET_VALUE = "SYNTHETIC-WORKBENCH-SECRET-C14"
CURL_SECRET = "curl-literal-secret-c14"
GIT = "/Library/Developer/CommandLineTools/usr/bin/git"
LOG = []

REQUEST = """# Lists items (kept)
format_version = 1
id = "list"
method = "GET"
url = "https://api.example.com/items" # trailing
body = "items/list.body.txt"
x-extension = true

[[headers]]
name = "Accept"
value = "application/json"

[[headers]]
name = "Authorization"
value = "Bearer {{token}}"
"""

EXPORT = """type: collection.insomnia.rest/5.0
schema_version: "5.1"
name: Sample
collection:
  - name: List
    method: GET
    url: "{{ _.base }}/items"
  - name: Script
    method: GET
    url: https://api.example.com/
    scripts:
      preRequest: "alert(1)"
environments:
  name: Base
  data:
    base: https://api.example.com
"""


def rel(path):
    return os.path.relpath(path, ROOT)


def sha(path):
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def tree_hash(root):
    h = hashlib.sha256()
    for base, dirs, files in sorted(os.walk(root)):
        dirs[:] = sorted(d for d in dirs if d != ".git")
        for name in sorted(files):
            p = os.path.join(base, name)
            h.update(os.path.relpath(p, root).encode())
            h.update(open(p, "rb").read())
    return h.hexdigest()


def save_tree(name, tree):
    path = os.path.join(NATIVE, f"tree-{name}.json")
    with open(path, "w") as f:
        json.dump(tree, f, indent=0)
    return rel(path)


def press(app, label, roles=("AXButton", "AXRadioButton", "AXTab", "AXCheckBox")):
    tree = app.tree()
    for role in roles:
        matches = [n for n in tree if n.get("role") == role and (n.get("label") or "").startswith(label)]
        for nth, n in enumerate(matches):
            if n.get("label") == label:
                app.ax("press", role, label, nth)
                time.sleep(0.4)
                return role
    raise RuntimeError(f"no element labelled {label!r}")


def value(app, role, label):
    node = app.field(role, label)
    return None if node is None else str(node.get("value") or "")


def replace(app, role, label, text):
    """Replaces a field's whole text and verifies what arrived."""
    for _ in range(3):
        app.ax("focus", role, label)
        time.sleep(0.3)
        app.keys("a", "command down")
        app.keycode(51)
        app.keys(text)
        if value(app, role, label) == text:
            return True
    return False


def osa(app, script):
    """Runs keystrokes in one System Events script (no pause between them),
    only while the fixture app is frontmost."""
    app.front()
    fx.sh(["osascript", "-e", f'tell application "System Events"\n{script}\nend tell'])
    time.sleep(0.5)


def quote(text):
    return text.replace("\\", "\\\\").replace('"', '\\"')


def archived(tree):
    """The collection's count of privately kept versions (0 when none)."""
    for text in fx.App.texts(tree):
        found = re.search(r"(\d+) earlier versions kept privately", text)
        if found:
            return int(found.group(1))
    return 0


def open_client(app):
    app.ax("press", "AXButton", "API Client")
    return app.wait_for(lambda t: fx.App.node(t, "AXHeading", "API Client") is not None)


def open_list(app):
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Open GET items/list.request.toml") is not None)
    app.ax("press", "AXButton", "Open GET items/list.request.toml")
    return app.wait_for(lambda t: fx.App.node(t, "AXTextField", "URL") is not None)


def git(repo, *args):
    env = {"GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": "/dev/null", "HOME": repo}
    return subprocess.run([GIT, "-C", repo, *args], capture_output=True, text=True, env=env)


def stage(work):
    """A v1 collection in a Git repository, the Insomnia export and a v1
    API Client profile naming the collection."""
    repo = os.path.realpath(os.path.join(work, "repo"))
    root = os.path.join(repo, "api")
    os.makedirs(os.path.join(root, "items"))
    os.makedirs(os.path.join(root, "environments"))
    with open(os.path.join(root, "collection.toml"), "w") as f:
        f.write('format_version = 1\nid = "c14"\nname = "Items API"\n\n[variables]\nhost = "api.example.com"\n'
                'token = { secret = "dev-token" }\nmanaged = { secret = "tenant.example" }\n')
    with open(os.path.join(root, "items/list.request.toml"), "w") as f:
        f.write(REQUEST)
    open(os.path.join(root, "items/list.body.txt"), "w").close()
    with open(os.path.join(root, "bad.request.toml"), "w") as f:
        f.write("this is = [not toml")
    with open(os.path.join(root, "environments/dev.toml"), "w") as f:
        f.write('format_version = 1\n\n[variables]\nhost = "dev.example.com"\n')
    git(repo, "init", "-q", "-b", "main")
    git(repo, "add", "-A")
    git(repo, "-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "commit", "-q", "-m", "fixture")
    marker = os.path.join(work, "fsmonitor-ran")
    script = os.path.join(work, "monitor.sh")
    with open(script, "w") as f:
        f.write(f"#!/bin/sh\ntouch {marker}\n")
    os.chmod(script, 0o755)
    git(repo, "config", "core.fsmonitor", script)
    os.makedirs(fx.DATA, exist_ok=True)
    with open(os.path.join(fx.DATA, "fixture-open-folder"), "w") as f:
        f.write(root + "\n")
    os.makedirs(os.path.join(fx.DATA, "fixture-workbench-files"))
    export = os.path.join(fx.DATA, "fixture-workbench-files", "export.yaml")
    with open(export, "w") as f:
        f.write(EXPORT)
    profile = os.path.join(fx.DATA, "fixture-v1-api-client")
    scope = hashlib.sha256(f"{root}\0c14".encode()).hexdigest()
    os.makedirs(os.path.join(profile, "environments", scope))
    with open(os.path.join(profile, "recent-collections.json"), "w") as f:
        json.dump([root], f)
    with open(os.path.join(profile, "environments", scope, "staging.toml"), "w") as f:
        f.write('format_version = 1\n\n[variables]\nhost = "staging.example.com"\nkey = { secret = "staging-key" }\n')
    return {"repo": repo, "root": root, "export": export, "profile": profile, "marker": marker}


def main():
    try:
        return run()
    finally:
        fx.stop_app()
        fx.clean_fixture_root()


def run():
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop")
    if c8.sha256(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c14-acceptance-")
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
    mgmt.mode()
    s = stage(work)
    request_file = os.path.join(s["root"], "items/list.request.toml")
    profile_before = tree_hash(s["profile"])
    export_before = sha(s["export"])
    obs = {}

    app.launch()
    open_client(app)

    # ---- N13: adoption of the staged v1 profile ----
    press(app, "Import v1 API Client profile…")
    tree, listed = c8.text(app, "Import your v1 API Client profile", 10)
    box = next((n["label"] for n in tree if n.get("role") == "AXCheckBox" and (n.get("label") or "").startswith("Items API")), None)
    if box:
        press(app, box)
    press(app, "Import selected")
    tree, recent = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Items API (api)") is not None, 10)
    obs["adoption"] = {"listed": listed, "candidate": box, "recent": recent, "source_unchanged": tree_hash(s["profile"]) == profile_before}
    obs["tree_adoption"] = save_tree("adoption", tree)

    # ---- N01: open the collection through the folder sheet ----
    press(app, "Open collection…")
    tree, opened = c8.text(app, "Items API", 10)
    tree, _ = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Open GET items/list.request.toml") is not None, 10)
    obs["open"] = {
        "opened": opened,
        "listed": fx.App.node(tree, "AXButton", "Open GET items/list.request.toml") is not None,
        "problem": app.has_text(tree, "bad.request.toml: invalid"),
        "bad_hidden": fx.App.node(tree, "AXButton", "Open GET bad") is None,
    }
    tree, _ = open_list(app)
    obs["open"]["fields"] = {
        label: fx.App.node(tree, role, label) is not None
        for role, label in (("AXTextField", "Method"), ("AXTextField", "URL"), ("AXTextArea", "Params"), ("AXTextArea", "Headers"),
                            ("AXTextArea", "Body"), ("AXTextArea", "Auth and variables (TOML)"))
    }
    obs["open"]["url"] = value(app, "AXTextField", "URL")
    obs["tree_open"] = save_tree("open", tree)
    # An incomplete inventory refuses every Save (v1); the invalid file is
    # removed and the collection read again.
    os.remove(os.path.join(s["root"], "bad.request.toml"))
    press(app, "Refresh")
    _, complete = app.wait_for(lambda t: not app.has_text(t, "This collection has problems to fix"), 10)
    obs["open"]["complete_after_fix"] = complete

    # ---- N02: Cmd+S inside the protection debounce ----
    app.ax("focus", "AXTextField", "URL")
    time.sleep(0.3)
    app.keys("a", "command down")
    osa(app, f'keystroke "{quote("https://api.example.com/v2/items")}"\nkeystroke "s" using {{command down}}')
    tree, saved = c8.text(app, "Saved", 10)
    text = open(request_file).read()
    obs["save"] = {
        "saved": saved and app.has_text(tree, "Saved"),
        "written": 'url = "https://api.example.com/v2/items" # trailing' in text,
        "kept": text.startswith("# Lists items (kept)\n") and "x-extension = true" in text,
    }

    # ---- N03: invalid input refused, file unchanged ----
    before = sha(request_file)
    app.ax("focus", "AXTextArea", "Headers")
    time.sleep(0.3)
    app.keycode(119)  # End: after the last row
    app.keys("a", "command down")
    app.keycode(124)  # Right: caret at the end
    app.keycode(36)
    app.keys("not a header")
    app.keys("s", "command down")
    tree, refused = c8.text(app, "Fix to save. Headers: use Name: value (line 3).", 10)
    obs["invalid"] = {
        "refused": refused,
        "unchanged": sha(request_file) == before,
        "kept": (value(app, "AXTextArea", "Headers") or "").endswith("not a header"),
    }
    replace(app, "AXTextArea", "Headers", "Accept: application/json\nAuthorization: Bearer {{token}}")

    # ---- N04: a protected draft survives a relaunch ----
    replace(app, "AXTextField", "URL", "https://api.example.com/v2/draft")
    time.sleep(1.5)
    app.quit()
    quit_cleanly = app.running_pid() is None
    app.launch()
    open_client(app)
    press(app, "Items API (api)")
    tree, recovered = c8.text(app, "Recovered draft", 12)
    draft_url = value(app, "AXTextField", "URL")
    press(app, "Discard draft")
    _, discarded = app.wait_for(lambda t: str((fx.App.node(t, "AXTextField", "URL") or {}).get("value")) == "https://api.example.com/v2/items", 8)
    obs["draft"] = {"quit_cleanly": quit_cleanly, "recovered": recovered, "draft_url": draft_url, "discarded": discarded}
    obs["tree_recovered"] = save_tree("recovered", tree)

    # ---- N05: the close guard ----
    app.ax("focus", "AXTextField", "URL")
    time.sleep(0.3)
    app.keycode(124)
    osa(app, 'keystroke "/guard"\nkeystroke "q" using {command down}')
    tree, sheet = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Quit anyway") is not None, 8)
    asked = app.has_text(tree, "Quit with unprotected API Client edits?")
    if sheet:
        app.ax("press", "AXButton", "Cancel")
        time.sleep(1.0)
    still = app.running_pid() is not None
    kept = value(app, "AXTextField", "URL") == "https://api.example.com/v2/items/guard"
    time.sleep(1.5)
    app.keys("q", "command down")
    gone = False
    for _ in range(30):
        time.sleep(0.3)
        if app.running_pid() is None:
            gone = True
            break
    obs["guard"] = {"sheet": sheet, "asked": asked, "cancel_kept_app": still, "text_kept": kept, "protected_quit": gone}
    app.pid = None

    # ---- N06: a conflict is never overwritten ----
    app.launch()
    open_client(app)
    press(app, "Items API (api)")
    app.wait_for(lambda t: fx.App.node(t, "AXTextField", "URL") is not None, 12)
    press(app, "Discard draft")
    app.wait_for(lambda t: str((fx.App.node(t, "AXTextField", "URL") or {}).get("value")) == "https://api.example.com/v2/items", 8)
    with open(request_file) as f:
        theirs = f.read().replace("v2/items", "theirs")
    with open(request_file, "w") as f:
        f.write(theirs)
    replace(app, "AXTextField", "URL", "https://api.example.com/v2/mine")
    app.keys("s", "command down")
    tree, conflict = c8.text(app, "Changed on disk", 10)
    untouched = open(request_file).read() == theirs
    press(app, "Keep mine")
    _, rebased = c8.text(app, "Your edits now apply to the files on disk. Save to write them.", 10)
    app.keys("s", "command down")
    _, written = app.wait_for(lambda t: "v2/mine" in open(request_file).read(), 10)
    obs["conflict"] = {"reported": conflict, "untouched": untouched, "rebased": rebased, "written": written}
    obs["tree_conflict"] = save_tree("conflict", tree)

    # A separate conflict settled with Use disk: the disk version is shown,
    # the project file stays as the other writer left it, and the operator's
    # version joins the privately kept ones.
    time.sleep(1.0)
    kept_before = archived(app.tree())
    with open(request_file) as f:
        disk = f.read().replace("v2/mine", "v2/disk")
    with open(request_file, "w") as f:
        f.write(disk)
    replace(app, "AXTextField", "URL", "https://api.example.com/v2/mine-again")
    app.keys("s", "command down")
    _, conflict2 = c8.text(app, "Changed on disk", 10)
    press(app, "Use disk")
    tree, used = c8.text(app, "Using the files on disk. Your version was kept privately.", 10)
    _, shown = app.wait_for(lambda t: str((fx.App.node(t, "AXTextField", "URL") or {}).get("value")) == "https://api.example.com/v2/disk", 8)
    tree, more = app.wait_for(lambda t: archived(t) > kept_before, 8)
    obs["use_disk"] = {"reported": conflict2, "used": used, "disk_shown": shown,
                       "file_unchanged": open(request_file).read() == disk,
                       "kept_privately": more and archived(tree) - kept_before >= 1,
                       "archives": [kept_before, archived(tree)]}
    obs["tree_use_disk"] = save_tree("use-disk", tree)

    # ---- N07: secrets ----
    press(app, "Secrets")
    tree, listed = c8.text(app, "dev-token (no value)", 10)
    app.type_into("AXTextField", "dev-token (no value)", SECRET_VALUE, secure=True)
    press(app, "Store value for dev-token")
    tree, stored = c8.text(app, "dev-token (value stored)", 10)
    managed = app.has_text(tree, "managed by the tenant in Settings") and fx.App.node(tree, "AXButton", "Store value for tenant.example") is None
    obs["tree_secrets"] = save_tree("secrets", tree)
    press(app, "Requests")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Preview") is not None, 8)
    press(app, "Preview")
    tree, masked = c8.text(app, "Authorization: Bearer ••••••", 10)
    obs["secrets"] = {"listed": listed, "stored": stored, "managed": managed, "masked": masked,
                      "missing_line": app.has_text(tree, "Secrets without a value")}
    obs["tree_preview"] = save_tree("preview", tree)

    # ---- N11: keyboard and reach ----
    order = app.tab_walk(60)
    labels = [f and f.get("label") for f in order]
    obs["keyboard"] = {
        "tools": any(l == "Requests" for l in labels),
        "find": any(l == "Find a request" for l in labels),
        "tab": any((l or "").startswith("GET list") for l in labels),
        "url": any(l == "URL" for l in labels),
        "save": any(l == "Save" for l in labels),
    }
    for _ in range(10):
        app.keys("=", "command down")
    obs["zoom_save"] = c8.reach(app, 2, "Save")
    app.keys("0", "command down")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{900, 600}}'])
    time.sleep(0.8)
    obs["min_save"] = c8.reach(app, 1, "Save")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{1280, 820}}'])
    time.sleep(0.6)

    # ---- N12: Git view ----
    press(app, "Git")
    press(app, "Read Git status")
    tree, branch = c8.text(app, "Branch main", 12)
    obs["git"] = {"branch": branch, "change": app.has_text(tree, "api/items/list.request.toml"),
                  "no_program_ran": not os.path.exists(s["marker"])}

    # ---- N08: imports ----
    press(app, "Import")
    curl = f"curl -u alice:{CURL_SECRET} https://api.example.com/v1/things -H 'X-Api-Key: {CURL_SECRET}'"
    app.type_into("AXTextArea", "cURL command", curl)
    press(app, "Create draft")
    tree, report = c8.text(app, "store its value in Secrets as import-", 10)
    obs["curl"] = {"report": report, "no_file": not os.path.exists(os.path.join(s["root"], "imported")),
                   "no_secret": not app.has_text(tree, CURL_SECRET)}
    press(app, "Import")
    press(app, "Choose Insomnia export…")
    tree, previewed = c8.text(app, "Conversion report", 10)
    refused_script = app.has_text(tree, "Request Script · not converted (fields)")
    press(app, "Select all supported")
    press(app, "Create selected")
    tree, created = c8.text(app, "Created 1 request drafts and 1 environments; 0 skipped.", 10)
    obs["insomnia"] = {"previewed": previewed, "refused_script": refused_script, "created": created,
                       "source_unchanged": sha(s["export"]) == export_before}
    obs["tree_import"] = save_tree("import", tree)

    # ---- N10 (part): no replay after a relaunch ----
    time.sleep(1.5)
    files_before = tree_hash(s["root"])
    app.quit()
    if app.running_pid() is not None:
        # A guard sheet would block; record it and quit anyway.
        LOG.append({"step": "guard-at-final-quit"})
        app.kill()
    app.launch()
    open_client(app)
    press(app, "Items API (api)")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Close collection") is not None, 12)
    time.sleep(1.0)
    replayed = tree_hash(s["root"]) != files_before

    # ---- N09: the built-in collection ----
    press(app, "Close collection")
    press(app, "Open Strata Cloud Manager")
    tree, scm = c8.text(app, "Built-in collection · 193 requests", 15)
    first = next((n["label"] for n in tree if n.get("role") == "AXButton" and (n.get("label") or "").startswith("Open GET ")), None)
    if first:
        app.ax("press", "AXButton", first)
        app.wait_for(lambda t: fx.App.node(t, "AXButton", "Preview") is not None, 10)
        press(app, "Preview")
        tree, oauth = c8.text(app, "Authentication: oauth2", 10)
    else:
        oauth = False
    obs["scm"] = {"opened": scm, "request": first, "oauth": oauth, "nothing_missing": not app.has_text(tree, "Secrets without a value"),
                  "private": os.path.isdir(os.path.join(fx.DATA, "api-client", "scm"))}
    obs["tree_scm"] = save_tree("scm", tree)

    app.quit()
    requests = mgmt.requests() + endpoint.requests()
    obs["network"] = {"requests": len(requests), "replayed": replayed}
    trees = [os.path.join(NATIVE, n) for n in os.listdir(NATIVE) if n.startswith("tree-")]
    obs["secret_in_trees"] = [rel(p) for p in trees if any(x in open(p).read() for x in (SECRET_VALUE, CURL_SECRET))]
    plain = []
    for base in (fx.DATA, s["repo"]):
        for d, _, files in os.walk(base):
            for name in files:
                p = os.path.join(d, name)
                try:
                    data = open(p, "rb").read()
                except OSError:
                    continue
                if SECRET_VALUE.encode() in data or CURL_SECRET.encode() in data:
                    plain.append(os.path.relpath(p, base))
    obs["secret_in_files"] = plain
    endpoint.stop()
    mgmt.stop()
    obs["v1_unchanged"] = fx.v1_metadata() == v1_before
    shutil.rmtree(work, ignore_errors=True)
    with open(os.path.join(NATIVE, "observations.json"), "w") as f:
        json.dump({"observations": obs, "log": LOG}, f, indent=1)
    return evaluate(prepared, obs)


def evaluate(prepared, o):
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C14"]}
    obs_path = rel(os.path.join(NATIVE, "observations.json"))
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [obs_path] + [e for e in evidence if e]})

    op = o["open"]
    add("C14-N01", op["opened"] and op["listed"] and op["problem"] and op["bad_hidden"] and op["complete_after_fix"] and all(op["fields"].values())
        and op["url"] == "https://api.example.com/items", f"open {op}", [o["tree_open"]])
    sv = o["save"]
    add("C14-N02", all(sv.values()), f"Cmd+S {sv}", [])
    iv = o["invalid"]
    add("C14-N03", all(iv.values()), f"invalid {iv}", [])
    dr = o["draft"]
    add("C14-N04", dr["quit_cleanly"] and dr["recovered"] and dr["draft_url"] == "https://api.example.com/v2/draft" and dr["discarded"],
        f"draft {dr}", [o["tree_recovered"]])
    gd = o["guard"]
    add("C14-N05", all(gd.values()), f"close guard {gd}", [])
    cf = o["conflict"]
    ud = o["use_disk"]
    use_disk = all(v for k, v in ud.items() if k != "archives")
    add("C14-N06", all(cf.values()) and use_disk, f"keep mine {cf}; use disk {ud}",
        [o["tree_conflict"], o["tree_use_disk"]])
    sc = o["secrets"]
    add("C14-N07", sc["listed"] and sc["stored"] and sc["managed"] and sc["masked"] and not sc["missing_line"]
        and not o["secret_in_trees"] and not o["secret_in_files"],
        f"secrets {sc}; value in AX snapshots: {o['secret_in_trees']}; in files: {o['secret_in_files']}", [o["tree_secrets"], o["tree_preview"]])
    cu, ins = o["curl"], o["insomnia"]
    add("C14-N08", all(cu.values()) and all(ins.values()), f"cURL {cu}; Insomnia {ins}", [o["tree_import"]])
    sm = o["scm"]
    add("C14-N09", sm["opened"] and bool(sm["request"]) and sm["oauth"] and sm["nothing_missing"] and sm["private"], f"built-in {sm}", [o["tree_scm"]])
    nw = o["network"]
    add("C14-N10", nw["requests"] == 0 and not nw["replayed"], f"network requests {nw['requests']}; files changed by relaunch: {nw['replayed']}", [])
    kb = o["keyboard"]
    add("C14-N11", all(kb.values()) and o["zoom_save"]["clear"] and o["min_save"]["clear"],
        f"Tab reached {kb}; 200% zoom: {o['zoom_save']['found']}; minimum size: {o['min_save']['found']}", [])
    gt = o["git"]
    add("C14-N12", all(gt.values()), f"git {gt}", [])
    ad = o["adoption"]
    add("C14-N13", ad["listed"] and bool(ad["candidate"]) and ad["recent"] and ad["source_unchanged"] and o["v1_unchanged"],
        f"adoption {ad}; v1 unchanged: {o['v1_unchanged']}", [o["tree_adoption"]])

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1",
        "subject": "C14",
        "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (tools/native-acceptance/ax.swift, System Events keystrokes); tools/native-acceptance/acceptance_c14.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "The fixture build answers the collection folder sheets and the Insomnia file sheet with staged paths; the production rfd sheets are the same adapters' native branch (flag fixture-native-sheets) and are not driven here.",
            "Interrupted saves are exercised by the workbench_documents suite (a fixture fault between file writes and a restart); the window is not killed mid-write here.",
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
