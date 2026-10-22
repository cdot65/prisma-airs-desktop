#!/usr/bin/env python3
"""C21 native acceptance: the Harness's desktop tools, project folder,
approvals and proposals through the real fixture window, the bundled pinned
engine and the synthetic Gateway and management service (mgmt_server.py).

The synthetic Gateway plays the model: mode "harnessTool" makes it ask the
engine for one tool call, and it answers in text once the engine sends the
call's output back (logged only as size, digest and named markers). The
project folder lives under the repository's target/ directory (the engine
refuses writes in system temporary folders) and is removed at the end.

Run from the repository root after `xtask native-prepare --chunk C21`.
Requires the Accessibility grant. Synthetic data only; the fixture data
root is removed at the end; v1 data and the login keychain are untouched.
"""

import json
import os
import secrets
import shutil
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fixture_app as fx  # noqa: E402
import acceptance_c08 as c8  # noqa: E402
import acceptance_c09 as c9  # noqa: E402
import acceptance_c16 as c16  # noqa: E402
import acceptance_c19 as c19  # noqa: E402
import native_sheets as sheets  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C21")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C21" else "native-C21")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C21" else "native-acceptance-C21.json")
INFERENCE_KEY = "SYNTHETIC-C21-HARNESS-KEY"
NAMESPACE = "airs_desktop"
OUTSIDE = "C21-OUTSIDE-CANARY"
MARKERS = ["Profile 0", "WROTE", "Operation not permitted", "fork", "Failed to write file", OUTSIDE,
           "unavailable", "unsupported"]
TOPIC = "11111111-1111-4111-8111-111111111111"
PROPOSED = "Money talk, proposed by the Harness"
LOG = []
c19.NATIVE = NATIVE


def rel(path):
    return os.path.relpath(path, ROOT)


def save_tree(name, tree):
    if sheets.sheets(tree):
        raise RuntimeError("refusing to save an AX tree while a panel is shown")
    return c19.save_tree(name, tree)


def runs(mgmt):
    return c19.harness_runs(mgmt)


def mgmt_requests(mgmt, since=0):
    return [r for r in mgmt.requests()[since:] if r["path"].startswith("/aisec/")]


def ask(mgmt, name, arguments, namespace=NAMESPACE):
    """The next turn's model asks for this one tool call."""
    tool = {"name": name, "arguments": json.dumps(arguments)}
    if namespace:
        tool["namespace"] = namespace
    mgmt.mode(harnessTool=tool, toolMarkers=MARKERS)


def command(mgmt, cmd):
    ask(mgmt, "exec_command", {"cmd": cmd, "yield_time_ms": 1000, "login": False}, namespace=None)


def turn(app, mgmt, prompt, decide=None, timeout=40):
    """Sends `prompt`; answers the approval it raises with `decide`
    ("Allow" or "Decline") if given. The tool output the engine returned."""
    before = len(runs(mgmt))
    c19.say(app, prompt)
    shown = None
    if decide:
        tree, shown = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Decline") is not None, timeout)
        shown = {"shown": shown, "texts": [x for x in app.texts(tree) if x.startswith(("Run a command", "Change files", "In /"))][:6]}
        c9.press_exact(app, decide)
    tree, done = app.wait_for(lambda t: len(runs(mgmt)) >= before + 2 and app.has_text(t, "Answered.")
                              and fx.App.node(t, "AXButton", "Decline") is None, timeout)
    output = next((r.get("toolOutput") for r in reversed(runs(mgmt)[before:]) if r.get("toolOutput")), None)
    return tree, {"answered": done, "approval": shown, "output": output, "requests": len(runs(mgmt)) - before}


def main():
    try:
        return run()
    finally:
        sheets.disable()
        fx.sh(["pkill", "-f", fx.PROCESS])
        fx.sh(["pkill", "-9", "-f", c19.ENGINE])
        fx.clean_fixture_root()


def run():
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")
    if c8.sha256(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c21-acceptance-")
    # The engine refuses writes in system temporary folders: the project
    # and the outside canary live under the repository's target/.
    base = os.path.join(ROOT, "target", f"native-c21-{secrets.token_hex(6)}")
    project = os.path.join(base, "agent-demo")
    os.makedirs(project)
    for n in range(3):
        with open(os.path.join(project, f"file-{n}.txt"), "w") as f:
            f.write(f"content {n}\n")
    with open(os.path.join(base, "outside.txt"), "w") as f:
        f.write(OUTSIDE + "\n")
    if os.path.isdir(NATIVE):
        shutil.rmtree(NATIVE)
    os.makedirs(NATIVE)
    app = fx.App(bundle, work, LOG)
    app.quit()
    fx.sh(["pkill", "-9", "-f", c19.ENGINE])
    fx.clean_fixture_root()
    v1_before = fx.v1_metadata()
    endpoint = fx.ScanEndpoint(os.path.join(work, "endpoint"), c8.SCAN_KEY)
    endpoint.install_roots()
    endpoint.start()
    mgmt = c8.MgmtEndpoint(os.path.join(work, "endpoint"), os.path.join(work, "mgmt"))
    mgmt.install()
    mgmt.start()
    mgmt.mode()
    try:
        return exercise(app, work, endpoint, mgmt, prepared, v1_before, base, project)
    finally:
        endpoint.stop()
        mgmt.stop()
        shutil.rmtree(work, ignore_errors=True)
        shutil.rmtree(base, ignore_errors=True)


def exercise(app, work, endpoint, mgmt, prepared, v1_before, base, project):
    runtime = f"{mgmt.origin}/rt/v1"
    obs = {}
    sheets.enable()
    app.launch()
    app.open_settings()
    app.type_into("AXTextField", "TSG ID (optional)", c8.TSG)
    app.create_custom_tenant("Tenant A", endpoint.origin)
    app.save_key(c8.SCAN_KEY)
    c8.save_client(app)
    c16.open_gateway(app)
    c8.text(app, "2 workspaces", 15)
    app.ax("press", "AXRadioButton", "Team A")
    c8.text(app, "2 providers · complete", 15)
    c16.tab(app, "Playground")
    app.type_into("AXTextField", "Runtime endpoint", runtime)
    app.type_into("AXTextField", "Workspace API key with completions access", INFERENCE_KEY, secure=True)
    app.ax("press", "AXButton", "Store key for Team A")
    c8.text(app, f"A key for Team A is stored (Keychain). It is sent only to {runtime}.", 15)

    # ---- N01: the folder from the native sheet; a system folder refused ----
    c19.open_harness(app)
    tree, offered = c8.text(app, "No folder.", 10)
    obs["tree_no_folder"] = save_tree("no-folder", tree)
    app.ax("press", "AXButton", "Choose a folder…")
    shown = sheets.wait_sheet(app)
    # Only read in memory: the panel lists the operator's folders.
    titled = any("Choose project folder" in (n.get("label") or "") + str(n.get("value") or "") for n in app.tree())
    opened = sheets.open_file(app, project)
    tree, chosen = app.wait_for(lambda t: app.has_text(t, project), 10)
    write = fx.App.node(tree, "AXCheckBox", "Allow changes in this folder") or {}
    obs["folder"] = {"offered": offered, "sheet": shown, "titled": titled, "opened": opened, "chosen": chosen,
                     "read_only": str(write.get("value")) in ("0", "False", "false"),
                     "read_only_text": app.has_text(tree, "Read-only: the Harness may ask to run commands here")}
    obs["tree_folder"] = save_tree("folder", tree)
    app.ax("press", "AXButton", "Choose another folder…")
    sheets.wait_sheet(app)
    sheets.open_file(app, "/usr")
    tree, refused = c8.text(app, "/usr cannot be used. Folders that hold system files, credentials or this app’s own data cannot be used.", 10)
    obs["folder"].update({"system_refused": refused, "kept": project in app.texts(tree)})
    obs["tree_refused"] = save_tree("refused", tree)
    sheets.disable()

    # ---- N02: a typed desktop read through the management client ----
    tree, ready = c19.start(app)
    obs["scope_read_only"] = app.has_text(tree, f"Folder {project} · read-only")
    since = len(mgmt.requests())
    ask(mgmt, "inventory", {"resource": "profiles", "limit": 5})
    tree, read = turn(app, mgmt, "Which security profiles does this tenant have?")
    reads = [r for r in mgmt_requests(mgmt, since) if r["method"] == "GET"]
    obs["read"] = {"ready": ready, **read, "management_reads": len(reads),
                   "tool_entry": app.has_text(tree, "App tool")}
    obs["tree_read"] = save_tree("read", tree)

    # ---- N03: commands wait for the operator; read-only cannot write ----
    command(mgmt, "echo kept > kept.txt && echo WROTE")
    tree, declined = turn(app, mgmt, "Write a file.", decide="Decline")
    obs["declined"] = {**declined, "entry_declined": app.has_text(tree, "(declined)"),
                       "nothing_written": not os.path.exists(os.path.join(project, "kept.txt"))}
    obs["tree_declined"] = save_tree("declined", tree)
    command(mgmt, "echo kept > kept.txt && echo WROTE")
    tree, read_only = turn(app, mgmt, "Write a file again.", decide="Allow")
    obs["read_only"] = {**read_only, "nothing_written": not os.path.exists(os.path.join(project, "kept.txt"))}

    # ---- N04: allowing changes is a new binding; the sandbox holds ----
    pid_ro = c19.leader()
    app.ax("press", "AXCheckBox", "Allow changes in this folder")
    tree, retired = app.wait_for(lambda t: c19.button(t, "Start") is not None and not app.has_text(t, "Conversation in Team A"), 15)
    obs["write"] = {"retired": retired, "engine_reaped": c19.wait(lambda: pid_ro not in c19.engines(), 10) and pid_ro is not None}
    tree, ready = c19.start(app)
    obs["write"]["ready"] = ready and app.has_text(tree, f"Folder {project} · changes allowed, each with your approval")
    command(mgmt, "echo kept > kept.txt && echo WROTE")
    tree, wrote = turn(app, mgmt, "Write a file.", decide="Allow")
    kept = os.path.join(project, "kept.txt")
    obs["write"]["inside"] = {**wrote, "file": open(kept).read() if os.path.exists(kept) else None}
    command(mgmt, f"cat {os.path.join(base, 'outside.txt')}")
    tree, outside = turn(app, mgmt, "Read the file next to the folder.", decide="Allow")
    obs["write"]["outside"] = outside
    command(mgmt, "sleep 1 & wait")
    tree, child = turn(app, mgmt, "Start a background job.", decide="Allow")
    obs["write"]["child"] = child
    patch = "*** Begin Patch\n*** Add File: added.txt\n+added by the engine\n*** End Patch"
    command(mgmt, f"apply_patch <<'EOF'\n{patch}\nEOF\n")
    tree, patched = turn(app, mgmt, "Add a file with a patch.", decide="Allow")
    obs["write"]["patch"] = {**patched, "file_change_shown": bool(patched["approval"]) and any(
        x.startswith("Change files") for x in patched["approval"]["texts"]),
        "file": os.path.exists(os.path.join(project, "added.txt")),
        "entry_failed": app.has_text(tree, "(failed)")}
    obs["tree_write"] = save_tree("write", tree)

    # ---- N06: forged or unknown calls run nothing ----
    forged = {}
    for label, (name, arguments) in {
        "unknown": ("deleteEverything", {}),
        "credential": ("inventory", {"resource": "profiles", "api_key": "x"}),
        "gateway": ("proposeChange", {"kind": "gatewayRoutes", "action": "create", "summary": "Add a route"}),
    }.items():
        since = len(mgmt.requests())
        ask(mgmt, name, arguments)
        tree, result = turn(app, mgmt, f"Try the {label} call.")
        forged[label] = {**result, "management": len(mgmt_requests(mgmt, since))}
    forged["no_proposal"] = not app.has_text(tree, "Proposed changes")
    obs["forged"] = forged
    obs["tree_forged"] = save_tree("forged", tree)

    # ---- N05: a proposal is staged, reviewed and applied only by the operator ----
    since = len(mgmt.requests())
    ask(mgmt, "proposeChange", {"kind": "topics", "action": "update", "target": TOPIC,
                                "body": {"topic_name": "Finance", "description": PROPOSED, "examples": [], "revision": 1},
                                "summary": "Clarify the Finance topic"})
    tree, staged = turn(app, mgmt, "Propose a clearer Finance topic.")
    obs["proposal"] = {**staged, "listed": app.has_text(tree, "Proposed changes") and app.has_text(tree, "Clarify the Finance topic"),
                       "writes_after_staging": len(c9.writes(mgmt, prefix="/aisec/"))}
    obs["tree_proposal"] = save_tree("proposal", tree)
    c9.press_exact(app, "Review")
    tree, in_admin = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Proposed by the Harness") is not None
                                  and app.has_text(t, "Type Finance to confirm"), 20)
    editor = fx.App.node(tree, "AXTextArea", "custom topic draft (JSON)") or {}
    obs["proposal"].update({"in_admin": in_admin, "exact_draft": PROPOSED in str(editor.get("value") or ""),
                            "writes_after_review": len(c9.writes(mgmt, prefix="/aisec/"))})
    obs["tree_review"] = save_tree("review", tree)
    app.type_into("AXTextField", "Type Finance to confirm", "Finance")
    app.ax("press", "AXButton", "Apply…")
    _, sheet = c9.wait_sheet(app)
    app.keycode(36)
    tree, applied = c8.text(app, "Applied. The item was read back.", 15)
    puts = c9.writes(mgmt, "PUT", "/aisec/")
    obs["proposal"].update({"sheet": sheet, "applied": applied, "puts": len(puts),
                            "put_description": PROPOSED in json.dumps(puts[-1]) if puts else False})
    obs["tree_applied"] = save_tree("applied", tree)
    c19.open_harness(app)
    tree, closed = c8.text(app, "Its review has ended.", 10)
    obs["proposal"]["closed"] = closed

    # ---- N07: keyboard and accessibility ----
    ask(mgmt, "proposeChange", {"kind": "topics", "action": "update", "target": TOPIC,
                                "body": {"topic_name": "Finance", "description": "Again", "examples": [], "revision": 2},
                                "summary": "Another clarification"})
    turn(app, mgmt, "Propose another.")
    command(mgmt, "ls")
    c19.say(app, "List the folder.")
    tree, waiting = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Decline") is not None, 30)
    obs["a11y"] = {"waiting": waiting,
                   "checkbox": fx.App.node(tree, "AXCheckBox", "Allow changes in this folder") is not None,
                   "regions": [h for h in ("Project folder", "Waiting for your approval", "Proposed changes")
                               if fx.App.node(tree, "AXHeading", h) is not None]}
    app.ax("focus", "AXCheckBox", "Allow changes in this folder")
    order = app.tab_walk(16)
    labels = [(o or {}).get("label") or "" for o in order]
    obs["a11y"]["tab_order"] = order
    obs["a11y"]["reached"] = [x for x in ("Choose another folder…", "Remove folder", "Allow", "Decline", "Review")
                              if any(lbl == x for lbl in labels)]
    obs["tree_a11y"] = save_tree("a11y", tree)
    c9.press_exact(app, "Decline")
    app.wait_for(lambda t: app.has_text(t, "Answered.") and fx.App.node(t, "AXButton", "Decline") is None, 30)

    # ---- N08: removing the folder ends the conversation; nothing is left ----
    pid = c19.leader()
    app.ax("press", "AXButton", "Remove folder")
    tree, removed = app.wait_for(lambda t: app.has_text(t, "No folder.") and not app.has_text(t, "Conversation in Team A"), 15)
    obs["removed"] = {"removed": removed, "engine_reaped": c19.wait(lambda: pid not in c19.engines(), 10) and pid is not None}
    obs["tree_removed"] = save_tree("removed", tree)
    app.quit()
    obs["removed"]["no_engine_after_quit"] = c19.wait(lambda: c19.engines() == [], 10)
    trees = [os.path.join(NATIVE, n) for n in os.listdir(NATIVE) if n.startswith("tree-")]
    secrets_ = (c8.SECRET, c8.SCAN_KEY, INFERENCE_KEY)
    obs["secret_in_trees"] = [rel(p) for p in trees if any(s in open(p).read() for s in secrets_)]
    log_text = json.dumps(mgmt.requests())
    obs["secret_in_log"] = any(s in log_text for s in (INFERENCE_KEY, c8.SECRET, OUTSIDE))
    obs["tool_output_secret"] = any((r.get("toolOutput") or {}).get("carriesSecret") for r in runs(mgmt))
    obs["project_files"] = sorted(os.listdir(project))
    obs["v1_unchanged"] = fx.v1_metadata() == v1_before
    obs["engines_left"] = c19.engines()
    with open(os.path.join(NATIVE, "observations.json"), "w") as f:
        json.dump({"observations": obs, "log": LOG}, f, indent=1)
    return evaluate(prepared, obs)


def marked(result, marker):
    return marker in ((result or {}).get("output") or {}).get("markers", [])


def evaluate(prepared, o):
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C21"]}
    obs_path = rel(os.path.join(NATIVE, "observations.json"))
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [obs_path] + [e for e in evidence if e]})

    f = o["folder"]
    add("C21-N01", f["offered"] and f["sheet"] and f["titled"] and f["opened"] and f["chosen"] and f["read_only"]
        and f["read_only_text"] and f["system_refused"] and f["kept"], f"folder {f}",
        [o["tree_no_folder"], o["tree_folder"], o["tree_refused"]])
    r = o["read"]
    add("C21-N02", r["ready"] and r["answered"] and r["tool_entry"] and r["management_reads"] >= 1 and marked(r, "Profile 0")
        and not r["output"]["carriesSecret"] and o["scope_read_only"], f"read {r}", [o["tree_read"]])
    d, ro = o["declined"], o["read_only"]
    add("C21-N03", d["answered"] and d["approval"]["shown"] and any(x.startswith("In /") for x in d["approval"]["texts"])
        and d["entry_declined"] and d["nothing_written"] and not marked(d, "WROTE")
        and ro["answered"] and ro["approval"]["shown"] and ro["nothing_written"] and not marked(ro, "WROTE"),
        f"declined {d}; read-only {ro}", [o["tree_declined"]])
    w = o["write"]
    add("C21-N04", w["retired"] and w["engine_reaped"] and w["ready"] and w["inside"]["file"] == "kept\n" and marked(w["inside"], "WROTE")
        and marked(w["outside"], "Operation not permitted") and not marked(w["outside"], OUTSIDE)
        and marked(w["child"], "fork") and w["patch"]["file_change_shown"] and w["patch"]["answered"]
        # The pinned engine's patch writer fails in its helper sandbox; shown as reported.
        and marked(w["patch"], "Failed to write file") and not w["patch"]["file"] and w["patch"]["entry_failed"],
        f"write {w}", [o["tree_write"]])
    p = o["proposal"]
    add("C21-N05", p["answered"] and p["listed"] and p["writes_after_staging"] == 0 and p["in_admin"] and p["exact_draft"]
        and p["writes_after_review"] == 0 and p["sheet"] and p["applied"] and p["puts"] == 1 and p["put_description"] and p["closed"],
        f"proposal {p}", [o["tree_proposal"], o["tree_review"], o["tree_applied"]])
    g = o["forged"]
    add("C21-N06", all(g[k]["answered"] and g[k]["management"] == 0 and (marked(g[k], "unavailable") or marked(g[k], "unsupported"))
                       for k in ("unknown", "credential", "gateway")) and g["no_proposal"], f"forged {g}", [o["tree_forged"]])
    y = o["a11y"]
    add("C21-N07", y["waiting"] and y["checkbox"] and len(y["regions"]) == 3 and set(y["reached"]) >= {"Allow", "Decline", "Review"},
        f"keyboard and accessibility {y}", [o["tree_a11y"]])
    m = o["removed"]
    add("C21-N08", m["removed"] and m["engine_reaped"] and m["no_engine_after_quit"] and not o["secret_in_trees"] and not o["secret_in_log"]
        and not o["tool_output_secret"] and o["project_files"] == ["file-0.txt", "file-1.txt", "file-2.txt", "kept.txt"]
        and o["v1_unchanged"] and o["engines_left"] == [],
        f"removed {m}; secrets in AX snapshots: {o['secret_in_trees']}; secret in the server log: {o['secret_in_log']}; "
        f"secret in a tool output: {o['tool_output_secret']}; folder: {o['project_files']}; engines left: {o['engines_left']}; "
        f"v1 unchanged: {o['v1_unchanged']}", [o["tree_removed"]])

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1",
        "subject": "C21",
        "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (tools/native-acceptance/ax.swift, System Events keystrokes) and the production NSOpenPanel, with the synthetic Gateway playing the model's tool calls, and inspecting the engine's processes, the project folder and the management log, against tools/native-acceptance/mgmt_server.py; tools/native-acceptance/acceptance_c21.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "The model is played by the synthetic Gateway (one scripted tool call per turn); no product model, Gateway or management endpoint is called before C25.",
            "The pinned engine's own patch writer (apply_patch) fails inside its narrower helper sandbox; the app shows the engine's result as reported, and allowed shell commands are what changed the folder. The engine is pinned and unchanged.",
            "The re-confirmation of changes after a company sign-in renewal is exercised by the core integration test (harness_identity_history) rather than this driver.",
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
