#!/usr/bin/env python3
"""C15 native acceptance: API Client execution through the real fixture
window. A synthetic collection is opened through the folder sheet (staged);
requests go to a plain-HTTP loopback endpoint (workbench_server.py) and,
with the tenant's native credentials, to the synthetic management endpoint
(mgmt_server.py). Covers Send of the fresh typed input from the keyboard,
cookies, a 64 MiB response streamed and saved through the save sheet
(staged), cancel, a lost response, tenant-credential admission, a reviewed
change through the native AppKit confirmation sheet, history across a
relaunch with nothing replayed, and no Node/Bun in the process tree.

Run from the repository root after `xtask native-prepare --chunk C15`.
Requires the Accessibility grant. Synthetic data only; the fixture data
root is removed at the end; v1 data and the login keychain are untouched.
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
import acceptance_c09 as c9  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C15")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C15" else "native-C15")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C15" else "native-acceptance-C15.json")
BIG = 64 * 1024 * 1024
LOG = []

OAUTH = ('[auth]\nkind = "oauth2"\ntoken_url = "{{token_url}}"\nclient_id = "{{client_id}}"\n'
         'client_secret = "{{client_secret}}"\nscope = "tsg_id:{{tsg_id}}"\n')


def rel(path):
    return os.path.relpath(path, ROOT)


def sha(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for piece in iter(lambda: f.read(1 << 20), b""):
            h.update(piece)
    return h.hexdigest()


def pattern(n):
    return bytes(i % 251 for i in range(n))


def pattern_sha(n):
    h = hashlib.sha256()
    chunk = pattern(251 * 4096)
    left = n
    while left:
        piece = chunk[: min(len(chunk), left)]
        h.update(piece)
        left -= len(piece)
    return h.hexdigest()


def save_tree(name, tree):
    path = os.path.join(NATIVE, f"tree-{name}.json")
    with open(path, "w") as f:
        json.dump(tree, f, indent=0)
    return rel(path)


def press(app, label, roles=("AXButton", "AXRadioButton", "AXTab")):
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
    for _ in range(3):
        app.ax("focus", role, label)
        time.sleep(0.3)
        app.keys("a", "command down")
        app.keycode(51)
        app.keys(text)
        if value(app, role, label) == text:
            return True
    return False


class Workbench:
    """The plain-HTTP loopback endpoint (workbench_server.py)."""

    def __init__(self, directory):
        self.dir = directory
        self.proc = subprocess.Popen(["python3", "-I", os.path.join(fx.TOOLS, "workbench_server.py"), "--dir", directory],
                                     stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        line = self.proc.stdout.readline().split()
        if not line or line[0] != "ready":
            raise SystemExit("the synthetic workbench endpoint did not start")
        self.port = int(line[1])

    @property
    def origin(self):
        return f"http://127.0.0.1:{self.port}"

    def requests(self, path=None, ends=False):
        """Requests received (or, with `ends`, streams finished)."""
        p = os.path.join(self.dir, "requests.jsonl")
        if not os.path.exists(p):
            return []
        with open(p) as f:
            rows = [json.loads(line) for line in f if line.strip()]
        return [r for r in rows if (r["method"] == "END") == ends and (path is None or r["path"].split("?")[0] == path)]

    def stop(self):
        self.proc.terminate()
        self.proc.wait(timeout=5)


def request(id_, method, url, extra=""):
    return f'format_version = 1\nid = "{id_}"\nmethod = "{method}"\nurl = "{url}"\n{extra}'


def stage(work, server, mgmt):
    repo = os.path.realpath(os.path.join(work, "repo"))
    root = os.path.join(repo, "api")
    os.makedirs(os.path.join(repo, ".git"))
    os.makedirs(root)
    with open(os.path.join(root, "collection.toml"), "w") as f:
        f.write(f'format_version = 1\nid = "c15"\nname = "Exec"\n\n[variables]\nbase = "{server.origin}"\n'
                f'scm_base = "{mgmt.origin}"\ntoken_url = "{mgmt.origin}/oauth2/access_token"\n'
                'tsg_id = { secret = "tenant.tsg_id" }\nclient_id = { secret = "tenant.client_id" }\n'
                'client_secret = { secret = "tenant.client_secret" }\n')
    files = {
        "items": request("items", "GET", "{{base}}/items"),
        "login": request("login", "GET", "{{base}}/login"),
        "needs": request("needs", "GET", "{{base}}/needs-cookie"),
        "big": request("big", "GET", f"{{{{base}}}}/big?bytes={BIG}"),
        "hang": request("hang", "GET", "{{base}}/hang"),
        "lost": request("lost", "POST", "{{base}}/lost"),
        "policies": request("policies", "GET", "{{scm_base}}/iam/v1/access_policies", OAUTH),
        "unknown": request("unknown", "GET", "{{scm_base}}/iam/v1/not-a-catalogue-operation", OAUTH),
        # Its token URL edited away from the credential's token address.
        "retoken": request("retoken", "GET", "{{scm_base}}/iam/v1/access_policies",
                           OAUTH.replace("{{token_url}}", f"{mgmt.origin}/elsewhere")),
        "create": request("create", "POST", "{{scm_base}}/iam/v1/access_policies", OAUTH),
    }
    for name, text in files.items():
        with open(os.path.join(root, f"{name}.request.toml"), "w") as f:
            f.write(text)
    os.makedirs(fx.DATA, exist_ok=True)
    with open(os.path.join(fx.DATA, "fixture-open-folder"), "w") as f:
        f.write(root + "\n")
    saved = os.path.join(work, "saved", "big-response.bin")
    os.makedirs(os.path.dirname(saved))
    with open(os.path.join(fx.DATA, "fixture-save-response"), "w") as f:
        f.write(saved + "\n")
    return {"repo": repo, "root": root, "saved": saved}


def open_client(app):
    app.ax("press", "AXButton", "API Client")
    return app.wait_for(lambda t: fx.App.node(t, "AXHeading", "API Client") is not None)


def open_request(app, name, method):
    label = f"Open {method} {name}.request.toml"
    shown = lambda t: any(n.get("role") == "AXHeading" and n.get("label") == name for n in t)  # noqa: E731
    for _ in range(3):
        app.wait_for(lambda t: fx.App.node(t, "AXButton", label) is not None, 10)
        app.ax("press", "AXButton", label)
        tree, ok = app.wait_for(shown, 6)
        if ok:
            return tree, ok
    return tree, False


def send(app):
    press(app, "Send")


def descendants(pid):
    out = fx.sh(["ps", "-A", "-o", "pid=,ppid=,comm="]).stdout.splitlines()
    rows = [line.split(None, 2) for line in out if len(line.split(None, 2)) == 3]
    children = {}
    for p, pp, comm in rows:
        children.setdefault(pp, []).append((p, comm))
    found, stack = [], [str(pid)]
    while stack:
        for p, comm in children.get(stack.pop(), []):
            found.append(os.path.basename(comm))
            stack.append(p)
    return found


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
    work = tempfile.mkdtemp(prefix="airs-v2-c15-acceptance-")
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
    server = Workbench(os.path.join(work, "workbench"))
    s = stage(work, server, mgmt)
    obs = {}

    app.launch()
    app.open_settings()
    app.type_into("AXTextField", "TSG ID (optional)", c8.TSG)
    app.create_custom_tenant("Tenant A", endpoint.origin)
    app.save_key(c8.SCAN_KEY)
    c8.save_client(app)
    open_client(app)
    press(app, "Open collection…")
    tree, opened = c8.text(app, "Exec", 10)
    obs["network_note"] = app.has_text(tree, "loopback addresses only")

    # ---- N01: the fresh typed input, sent from the keyboard ----
    open_request(app, "items", "GET")
    items_file = os.path.join(s["root"], "items.request.toml")
    before = sha(items_file)
    app.ax("focus", "AXTextField", "URL")
    time.sleep(0.3)
    app.keycode(119)  # End
    app.keys("?q=1")
    app.ax("focus", "AXButton", "Send")
    time.sleep(0.3)
    focused = app.focused(app.tree())
    app.keycode(49)  # Space activates the focused button
    tree, ran = c8.text(app, "HTTP 200 ·", 12)
    got = server.requests("/items")
    obs["fresh"] = {
        "send_focusable": bool(focused) and (focused.get("label") or "") == "Send",
        "ran": ran,
        "body_shown": app.has_text(tree, '{"items":[1,2,3]}'),
        "sent_as_typed": [r["path"] for r in got] == ["/items?q=1"],
        "file_unchanged": sha(items_file) == before,
        "response_region": any(n.get("label") == "Response" for n in tree),
    }
    obs["tree_fresh"] = save_tree("fresh", tree)

    # ---- N02: cookies follow the collection's jar ----
    open_request(app, "login", "GET")
    send(app)
    c8.text(app, "HTTP 200 ·", 10)
    open_request(app, "needs", "GET")
    send(app)
    tree, cookie = c8.text(app, '"cookie":true', 10)
    sent = {k.lower(): v for k, v in (server.requests("/needs-cookie") or [{}])[-1].get("headers", {}).items()}
    obs["cookies"] = {"returned": cookie, "header": sent.get("cookie")}

    # ---- N03: a 64 MiB response streamed, then saved exactly ----
    open_request(app, "big", "GET")
    started = time.time()
    send(app)
    tree, big = c8.text(app, "HTTP 200 · 67.1 MB", 90)
    obs["big"] = {"ran": big, "seconds": round(time.time() - started, 2),
                  "not_text": app.has_text(tree, "The body is not text; save it to open it elsewhere.")}
    press(app, "Save response…")
    tree, saved = c8.text(app, "Saved 67.1 MB as big-response.bin.", 60)
    obs["big"]["saved"] = saved
    obs["big"]["exact"] = os.path.exists(s["saved"]) and os.path.getsize(s["saved"]) == BIG and sha(s["saved"]) == pattern_sha(BIG)
    sample = pattern(4096)
    plain = []
    for d, _, names in os.walk(os.path.join(fx.DATA, "api-client")):
        for name in names:
            p = os.path.join(d, name)
            with open(p, "rb") as f:
                if sample in f.read():
                    plain.append(os.path.relpath(p, fx.DATA))
    obs["big"]["sealed"] = not plain
    obs["tree_big"] = save_tree("big", tree)

    # ---- N04: cancel a running request ----
    open_request(app, "hang", "GET")
    send(app)
    tree, running = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Cancel request") is not None, 10)
    obs["cancel"] = {"running": running, "progress": app.has_text(tree, "Sending GET…")}
    press(app, "Cancel request")
    tree, cancelled = c8.text(app, "Cancelled ·", 10)
    obs["cancel"].update({"cancelled": cancelled, "sent_once": len(server.requests("/hang")) == 1})

    # ---- N05: a lost response is never resent ----
    open_request(app, "lost", "POST")
    send(app)
    tree, uncertain = c8.text(app, "may have reached the server", 15)
    time.sleep(2)
    obs["lost"] = {"uncertain": uncertain, "sent_once": len(server.requests("/lost")) == 1,
                   "no_resend_control": not any((n.get("label") or "").lower() in ("retry", "resend") for n in tree)}

    # ---- N06: tenant credentials admitted before any is read ----
    base = len(mgmt.requests())
    open_request(app, "unknown", "GET")
    send(app)
    tree, denied = c8.text(app, "known Strata Cloud Manager and AIRS operations", 10)
    open_request(app, "retoken", "GET")
    send(app)
    tree, token_refused = c8.text(app, "not the tenant credential's token address", 10)
    nothing = len(mgmt.requests()) == base
    open_request(app, "policies", "GET")
    send(app)
    tree, read = c8.text(app, "HTTP ", 15)
    calls = mgmt.requests()[base:]
    token = [c for c in calls if c["path"] == "/oauth2/access_token"]
    reads = [c for c in calls if c["path"] == "/iam/v1/access_policies"]
    obs["managed"] = {
        "unknown_denied": denied,
        "token_url_refused": token_refused,
        "nothing_sent_before": nothing,
        "read": read,
        "token": len(token) == 1 and token[0]["auth"] == "basic-ok" and token[0]["scope"] == f"tsg_id:{c8.TSG}" and not token[0]["secretInBody"],
        "bearer": len(reads) == 1 and reads[0]["auth"] == "bearer-issued",
    }
    obs["tree_managed"] = save_tree("managed", tree)

    # ---- N07: a reviewed change through the native sheet ----
    open_request(app, "create", "POST")
    replace(app, "AXTextArea", "Body", '{"name": "synthetic-c15"}')
    send(app)
    tree, reviewed = c8.text(app, "Review this change to the tenant", 10)
    posts = lambda: [c for c in mgmt.requests() if c["method"] == "POST" and c["path"] == "/iam/v1/access_policies"]  # noqa: E731
    obs["review"] = {"shown": reviewed, "none_before": not posts(), "masked_url": app.has_text(tree, f"POST {mgmt.origin}/iam/v1/access_policies")}
    obs["tree_review"] = save_tree("review", tree)
    press(app, "Send to tenant…")
    tree, sheet = c9.wait_sheet(app)
    nodes = c9.sheet(app, tree) or []
    obs["review"]["sheet"] = sheet
    obs["review"]["sheet_title"] = any("Send POST to the tenant?" in (n.get("label") or "") + str(n.get("value") or "") for n in nodes)
    obs["tree_sheet"] = save_tree("sheet", tree)
    app.keycode(53)  # Escape
    c9.wait_no_sheet(app)
    tree, declined = c8.text(app, "Not sent.", 10)
    obs["review"]["escape_declined"] = declined and not posts()
    press(app, "Send to tenant…")
    c9.wait_sheet(app)
    app.keycode(36)  # Return: Send
    tree, applied = c8.text(app, "HTTP ", 15)
    time.sleep(1)
    obs["review"]["applied_once"] = applied and len(posts()) == 1
    obs["review"]["review_gone"] = fx.App.node(app.tree(), "AXButton", "Send to tenant…") is None

    # ---- N09: no Node or Bun beside the app ----
    obs["process_tree"] = sorted(set(descendants(app.running_pid() or 0)))

    # ---- N08: history across a relaunch; nothing replayed ----
    open_request(app, "hang", "GET")
    send(app)
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Cancel request") is not None, 10)
    hangs = len(server.requests("/hang"))
    app.quit()
    app.launch()
    open_client(app)
    press(app, "Exec (api)")
    app.wait_for(lambda t: fx.App.node(t, "AXRadioButton", "History") is not None or fx.App.node(t, "AXTab", "History") is not None, 10)
    press(app, "History")
    tree, interrupted = c8.text(app, "Interrupted (not sent again)", 12)
    time.sleep(2)
    obs["history"] = {
        "interrupted": interrupted,
        "earlier": app.has_text(tree, "GET items · HTTP 200"),
        "nothing_replayed": len(server.requests("/hang")) == hangs and len(posts()) == 1,
    }
    obs["tree_history"] = save_tree("history", tree)

    app.quit()
    server.stop()
    endpoint.stop()
    mgmt.stop()
    obs["v1_unchanged"] = fx.v1_metadata() == v1_before
    shutil.rmtree(work, ignore_errors=True)
    with open(os.path.join(NATIVE, "observations.json"), "w") as f:
        json.dump({"observations": obs, "log": LOG}, f, indent=1)
    return evaluate(prepared, obs)


def evaluate(prepared, o):
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C15"]}
    obs_path = rel(os.path.join(NATIVE, "observations.json"))
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [obs_path] + [e for e in evidence if e]})

    add("C15-N01", all(o["fresh"].values()) and o["network_note"], f"fresh input {o['fresh']}; loopback note {o['network_note']}", [o["tree_fresh"]])
    ck = o["cookies"]
    add("C15-N02", ck["returned"] and ck["header"] == "sid=synthetic-c15", f"cookies {ck}", [])
    bg = o["big"]
    add("C15-N03", bg["ran"] and bg["not_text"] and bg["saved"] and bg["exact"] and bg["sealed"], f"64 MiB {bg}", [o["tree_big"]])
    add("C15-N04", all(o["cancel"].values()), f"cancel {o['cancel']}", [])
    add("C15-N05", all(o["lost"].values()), f"lost response {o['lost']}", [])
    add("C15-N06", all(o["managed"].values()), f"tenant credentials {o['managed']}", [o["tree_managed"]])
    add("C15-N07", all(o["review"].values()), f"review {o['review']}", [o["tree_review"], o["tree_sheet"]])
    add("C15-N08", all(o["history"].values()), f"history {o['history']}", [o["tree_history"]])
    pt = o["process_tree"]
    add("C15-N09", not any(n in ("node", "bun", "deno") for n in pt) and o["v1_unchanged"],
        f"descendants {pt}; v1 unchanged {o['v1_unchanged']}", [])

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1",
        "subject": "C15",
        "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (tools/native-acceptance/ax.swift, System Events keystrokes); tools/native-acceptance/acceptance_c15.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "Fixture builds send API Client requests to loopback only (literal loopback addresses and localhost pinned to 127.0.0.1); no product endpoint is called.",
            "The fixture build answers the collection folder sheet and the save-response sheet with staged paths; the production rfd sheets are the same adapter's native branch (flag fixture-native-sheets) and are not driven here.",
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
