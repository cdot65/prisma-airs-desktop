#!/usr/bin/env python3
"""C19 native acceptance: the native Harness through the real fixture window
and the bundled, pinned engine (Contents/Resources/harness/airs-harness)
against the synthetic Gateway (mgmt_server.py streams Responses the way the
engine expects). A conversation is started, answered, stopped, ended, retired
by another workspace and by sleep, survives nothing across a quit, and a
crashed engine is named; every engine process is checked gone and its
runtime home removed.

Run from the repository root after `xtask native-prepare --chunk C19`.
Requires the Accessibility grant. Synthetic data only; the fixture data
root is removed at the end; v1 data and the login keychain are untouched.
"""

import hashlib
import json
import os
import shutil
import signal
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fixture_app as fx  # noqa: E402
import acceptance_c08 as c8  # noqa: E402
import acceptance_c16 as c16  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C19")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C19" else "native-C19")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C19" else "native-acceptance-C19.json")
INFERENCE_KEY = "SYNTHETIC-C19-HARNESS-KEY"
KEY_SHA = hashlib.sha256(INFERENCE_KEY.encode()).hexdigest()
PROMPT = "Say hello to the C19 canary."
ANSWER = "Hello from the synthetic Gateway."
RUNTIME_HOMES = os.path.join(fx.DATA, "harness-runtime")
ENGINE = "Fixture.app/Contents/Resources/harness/airs-harness"
PIN = json.load(open(os.path.join(ROOT, "resources/harness/pin.json")))
LOG = []


def rel(path):
    return os.path.relpath(path, ROOT)


def save_tree(name, tree):
    path = os.path.join(NATIVE, f"tree-{name}.json")
    with open(path, "w") as f:
        json.dump(tree, f, indent=0)
    return rel(path)


def engines():
    """The bundled engine's processes (the engine and its own supervisor)."""
    out = fx.sh(["pgrep", "-f", ENGINE]).stdout.split()
    return [int(p) for p in out]


def leader():
    """The engine process whose parent is the app."""
    app = fx.App.running_pid()
    for pid in engines():
        ppid = fx.sh(["ps", "-o", "ppid=", "-p", str(pid)]).stdout.strip()
        if app and ppid == str(app):
            return pid
    return None


def ps(pid, fields):
    return fx.sh(["ps", "-o", f"{fields}=", "-p", str(pid)]).stdout.strip()


def homes():
    if not os.path.isdir(RUNTIME_HOMES):
        return []
    return sorted(n for n in os.listdir(RUNTIME_HOMES) if n.startswith("run-"))


def wait(predicate, timeout=10):
    deadline = time.time() + timeout
    while time.time() < deadline:
        if predicate():
            return True
        time.sleep(0.05)
    return predicate()


def harness_runs(mgmt):
    return [r for r in mgmt.requests() if r["path"] == "/rt/v1/responses" and r.get("harness") and not r.get("closedByClient")]


def closed(mgmt):
    return [r for r in mgmt.requests() if r.get("closedByClient") and r.get("harness")]


def open_harness(app):
    app.ax("press", "AXButton", "Harness")
    return app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Harness") is not None, 10)


def button(tree, label):
    return fx.App.node(tree, "AXButton", label)


def enabled(tree, label):
    b = button(tree, label) or {}
    return bool(b) and b.get("enabled") is not False


def status_node(tree):
    """The conversation's status line (role="status": AXApplicationStatus)."""
    return next((n for n in tree if n.get("subrole") == "AXApplicationStatus"), None)


def start(app, timeout=30):
    app.ax("press", "AXButton", "Start")
    return app.wait_for(lambda t: fx.App.node(t, "AXTextArea", "Message") is not None
                        and app.has_text(t, "Ready."), timeout)


def say(app, text):
    app.type_into("AXTextArea", "Message", text)
    app.ax("focus", "AXTextArea", "Message")
    app.keycode(36, "command down")


def scan_for(paths, needles):
    """Files under `paths` holding any of `needles`."""
    hits = []
    for base in paths:
        for d, _, files in os.walk(base):
            for f in files:
                p = os.path.join(d, f)
                try:
                    data = open(p, "rb").read()
                except OSError:
                    continue
                if any(n.encode() in data for n in needles):
                    hits.append(os.path.relpath(p, base))
    return hits


def main():
    try:
        return run()
    finally:
        fx.sh(["pkill", "-f", fx.PROCESS])
        fx.sh(["pkill", "-9", "-f", ENGINE])
        fx.clean_fixture_root()


def run():
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")
    if c8.sha256(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c19-acceptance-")
    if os.path.isdir(NATIVE):
        shutil.rmtree(NATIVE)
    os.makedirs(NATIVE)
    app = fx.App(bundle, work, LOG)
    app.quit()
    fx.sh(["pkill", "-9", "-f", ENGINE])
    fx.clean_fixture_root()
    v1_before = fx.v1_metadata()
    endpoint = fx.ScanEndpoint(os.path.join(work, "endpoint"), c8.SCAN_KEY)
    endpoint.install_roots()
    endpoint.start()
    mgmt = c8.MgmtEndpoint(os.path.join(work, "endpoint"), os.path.join(work, "mgmt"))
    mgmt.install()
    mgmt.start()
    mgmt.mode()
    runtime = f"{mgmt.origin}/rt/v1"
    obs = {}

    # ---- N01: the bundled engine, sealed and Node-free ----
    resources = os.path.join(bundle, "Contents/Resources")
    engine_path = os.path.join(resources, "harness", PIN["executable"])
    names = []
    for d, _, files in os.walk(bundle):
        names += [os.path.relpath(os.path.join(d, f), bundle) for f in files]
    js_runtime = [n for n in names if os.path.basename(n) in ("node", "bun", "npm", "npx", "airs-cli")
                  or "node_modules" in n or n.endswith((".mjs", ".cjs"))]
    manifest = json.load(open(os.path.join(resources, "harness", "MANIFEST.json")))
    obs["bundle"] = {"engine_sha": c8.sha256(engine_path) == PIN["binarySha256"],
                     "manifest_pin": manifest["pin"]["binarySha256"] == PIN["binarySha256"]
                     and manifest["pin"]["resourcesSha256"] == PIN["resourcesSha256"],
                     "js_runtime": js_runtime[:10], "files": len(names)}

    app.launch()
    tree, heading = open_harness(app)
    tree, shown = c8.text(app, f"Engine {PIN['runtimeVersion']} · {PIN['engineCommit'][:12]} · airs-desktop/1", 10)
    obs["offer"] = {"heading": heading, "engine_line": shown,
                    "needs_tenant": app.has_text(tree, "A tenant chosen") and fx.App.node(tree, "AXButton", "Go to Settings") is not None,
                    "start_disabled": not enabled(tree, "Start conversation")}
    obs["tree_offer"] = save_tree("offer", tree)
    # A tenant, its management client, a chosen workspace and its key.
    app.open_settings()
    app.type_into("AXTextField", "TSG ID (optional)", c8.TSG)
    app.create_custom_tenant("Tenant A", endpoint.origin)
    app.save_key(c8.SCAN_KEY)
    c8.save_client(app)
    c16.open_gateway(app)
    c8.text(app, "2 workspaces", 15)
    app.ax("press", "AXRadioButton", "Team A")
    c8.text(app, "2 providers · complete", 15)
    open_harness(app)
    # C20 adds company sign-in as the other way to answer.
    tree, wants_key = c8.text(app, "Store an inference key for Team A in the AI Gateway Playground", 10)
    obs["offer"]["needs_key"] = wants_key and not enabled(tree, "Start conversation")
    c16.open_gateway(app)
    c16.tab(app, "Playground")
    app.type_into("AXTextField", "Runtime endpoint", runtime)
    app.type_into("AXTextField", "Workspace API key with completions access", INFERENCE_KEY, secure=True)
    app.ax("press", "AXButton", "Store key for Team A")
    c8.text(app, f"A key for Team A is stored (Keychain). It is sent only to {runtime}.", 15)
    tree, _ = open_harness(app)
    tree, bound = c8.text(app, runtime, 10)
    obs["offer"]["bound"] = bound and app.has_text(tree, "Team A") and enabled(tree, "Start conversation")

    # ---- N02: one engine, launched with nothing but its bootstrap ----
    before = engines()
    tree, ready = start(app)
    pid = leader()
    group = ps(pid, "pgid") if pid else None
    environment = fx.sh(["ps", "eww", "-o", "command=", "-p", str(pid)]).stdout if pid else ""
    obs["start"] = {
        "ready": ready, "none_before": before == [], "leader": pid is not None,
        "own_group": bool(pid) and group == str(pid),
        "args": ps(pid, "args") == f"{engine_path} desktop-server" if pid else None,
        "key_in_environment": INFERENCE_KEY in environment,
        "path": "PATH=/usr/bin:/bin:/usr/sbin:/sbin" in environment,
        "home": f"AIRS_HARNESS_HOME={RUNTIME_HOMES}/run-" in environment,
        "homes": len(homes()),
        "focus_message": (app.focused(tree) or {}).get("role") == "AXTextArea",
        "processes": len(engines()),
    }
    obs["tree_ready"] = save_tree("ready", tree)

    # ---- N03: an answer streamed through the engine with the workspace key ----
    say(app, PROMPT)
    tree, answered = c8.text(app, ANSWER, 20)
    tree, done = c8.text(app, "Answered.", 10)
    sent = harness_runs(mgmt)
    obs["answer"] = {"answered": answered, "done": done, "sent": len(sent),
                     "key": bool(sent) and sent[0].get("inferenceKeySha256") == KEY_SHA,
                     "sandbox": sent[0].get("sandboxMode") if sent else None,
                     "stream": sent[0].get("stream") if sent else None,
                     "you": app.has_text(tree, PROMPT), "tokens": app.has_text(tree, "Tokens so far: 10 in, 6 out.")}
    obs["tree_answer"] = save_tree("answer", tree)

    # ---- N04: stopping an answer; the conversation goes on ----
    mgmt.mode(harness="paced")
    say(app, "Tell a long story.")
    tree, streaming = c8.text(app, "thirty-two bytes of paced text.", 20)
    stop_offered = enabled(tree, "Stop answer")
    app.ax("press", "AXButton", "Stop answer")
    tree, stopped = c8.text(app, "The answer was stopped.", 10)
    gone_stream = wait(lambda: len(closed(mgmt)) >= 1, 10)
    mgmt.mode()
    say(app, "Short this time.")
    tree, again = app.wait_for(lambda t: app.has_text(t, "Answered.") and sum(ANSWER in x for x in app.texts(t)) >= 2, 20)
    obs["interrupt"] = {"streaming": streaming, "stop_offered": stop_offered, "stopped": stopped,
                        "note": app.has_text(tree, "(stopped before it finished)"), "closed_by_client": gone_stream,
                        "continued": again, "sent": len(harness_runs(mgmt))}
    obs["tree_interrupt"] = save_tree("interrupt", tree)

    # ---- N05: ending the conversation reaps the engine and its home ----
    app.ax("press", "AXButton", "End conversation")
    tree, ended = c8.text(app, "You ended this conversation.", 10)
    reaped = wait(lambda: engines() == [], 10)
    tree, offered = app.wait_for(lambda t: enabled(t, "Start a new conversation"), 5)
    holder = next((n for n in tree if n.get("focused") and n.get("role") not in ("AXWindow", "AXWebArea", "AXScrollArea")), None) or {}
    obs["end"] = {"ended": ended, "reaped": reaped, "homes": homes(),
                  "focus": {k: holder.get(k) for k in ("role", "subrole", "label")},
                  "status_focused": "ended" in f"{holder.get('label') or ''}{holder.get('value') or ''}"
                  or str(holder.get("subrole") or "").endswith("Status"),
                  "new_offered": offered}
    obs["tree_ended"] = save_tree("ended", tree)

    # ---- N06: another workspace, or sleep, retires the conversation ----
    tree, _ = start(app)
    pid_ws = leader()
    c16.open_gateway(app)
    app.ax("press", "AXRadioButton", "Team B")
    time.sleep(1.0)
    retired_ws = wait(lambda: engines() == [], 10)
    tree, _ = open_harness(app)
    obs["retire"] = {"workspace_engine_gone": retired_ws, "had_engine": pid_ws is not None,
                     "view_withdrawn": not app.has_text(tree, "Conversation in Team A"),
                     "homes_after_workspace": homes()}
    obs["tree_retired"] = save_tree("retired", tree)
    c16.open_gateway(app)
    app.ax("press", "AXRadioButton", "Team A")
    c8.text(app, "2 providers · complete", 15)
    open_harness(app)
    tree, _ = start(app)
    pid_sleep = leader()
    app.menu("Fixture", "Post Will Sleep")
    tree, slept = c8.text(app, "This conversation ended when the Mac went to sleep.", 10)
    obs["retire"].update({"sleep_shown": slept, "sleep_engine_gone": wait(lambda: engines() == [], 10),
                          "had_sleep_engine": pid_sleep is not None, "homes_after_sleep": homes()})
    obs["tree_sleep"] = save_tree("sleep", tree)

    # ---- N10: a crashed engine is named, and a new conversation starts ----
    tree, _ = start(app)
    victim = leader()
    if victim:
        os.kill(victim, signal.SIGKILL)
    tree, crashed = c8.text(app, "The engine stopped unexpectedly.", 10)
    crash_gone = wait(lambda: engines() == [], 10)
    wait(lambda: homes() == [], 5)
    crash_homes = homes()
    obs["tree_crash"] = save_tree("crash", tree)
    tree, restarted = start(app)
    obs["crash"] = {"killed": victim is not None, "named": crashed, "gone": crash_gone,
                    "homes": crash_homes, "restarted": restarted, "new_engine": leader() not in (None, victim)}

    # ---- N08: keyboard and accessibility ----
    say(app, PROMPT)
    tree, _ = c8.text(app, "Answered.", 20)
    tree, _ = app.wait_for(lambda t: any(n.get("role") == "AXList" and n.get("label") == "Messages" for n in t), 5)
    transcript = next((n for n in tree if n.get("role") == "AXList" and n.get("label") == "Messages"), None)
    obs["a11y"] = {"transcript_named": transcript is not None,
                   "message_labelled": fx.App.node(tree, "AXTextArea", "Message") is not None,
                   "status_role": status_node(tree) is not None,
                   "send_keyboard": len(harness_runs(mgmt)) >= 4}
    app.ax("focus", "AXTextArea", "Message")
    reached = None
    for _ in range(40):
        app.keycode(48)
        f = app.focused(app.tree())
        if f and f.get("role") == "AXButton" and f.get("label") == "End conversation":
            reached = f
            break
    if reached:
        app.keycode(49)
    tree, ended_kb = c8.text(app, "You ended this conversation.", 10)
    obs["a11y"].update({"end_reached_by_tab": reached is not None, "ended_by_space": ended_kb,
                        "engine_gone": wait(lambda: engines() == [], 10)})
    obs["tree_a11y"] = save_tree("a11y", tree)

    # ---- N07: quitting with a conversation open leaves nothing running or kept ----
    tree, _ = start(app)
    say(app, PROMPT)
    c8.text(app, "Answered.", 20)
    pid_quit = leader()
    app.quit()
    quit_gone = wait(lambda: engines() == [], 10)
    homes_after_quit = homes()
    leaks = scan_for([fx.DATA], [INFERENCE_KEY, PROMPT, ANSWER, "Tell a long story."])
    app.launch()
    tree, _ = open_harness(app)
    tree, offered = c8.text(app, "Start conversation", 15)
    obs["quit"] = {"had_engine": pid_quit is not None, "engine_gone": quit_gone, "homes": homes_after_quit,
                   # Since C20 a saved conversation is listed by its title
                   # (sealed on disk); none is running or reopened.
                   "plaintext_on_disk": leaks, "nothing_restored": fx.App.node(tree, "AXTextArea", "Message") is None
                   and not app.has_text(tree, "Conversation in"), "start_offered": offered}
    obs["tree_relaunch"] = save_tree("relaunch", tree)
    # The workspace listing is read again before a workspace can be chosen.
    c16.open_gateway(app)
    c8.text(app, "2 workspaces", 25)
    app.ax("press", "AXRadioButton", "Team A")
    c8.text(app, "2 providers · complete", 15)
    open_harness(app)
    tree, relaunched = start(app)
    obs["quit"]["new_conversation"] = relaunched
    app.ax("press", "AXButton", "End conversation")
    c8.text(app, "You ended this conversation.", 10)
    wait(lambda: engines() == [], 10)

    app.quit()
    trees = [os.path.join(NATIVE, n) for n in os.listdir(NATIVE) if n.startswith("tree-")]
    secrets = (c8.SECRET, c8.SCAN_KEY, INFERENCE_KEY)
    obs["secret_in_trees"] = [rel(p) for p in trees if any(s in open(p).read() for s in secrets)]
    log_text = json.dumps(mgmt.requests())
    obs["prompt_in_log"] = PROMPT in log_text or INFERENCE_KEY in log_text
    engine_paths = sorted({r["path"] for r in mgmt.requests() if r.get("harness")})
    obs["engine_paths"] = engine_paths
    obs["harness_requests"] = len(harness_runs(mgmt))
    endpoint.stop()
    mgmt.stop()
    obs["v1_unchanged"] = fx.v1_metadata() == v1_before
    obs["engines_left"] = engines()
    shutil.rmtree(work, ignore_errors=True)
    with open(os.path.join(NATIVE, "observations.json"), "w") as f:
        json.dump({"observations": obs, "log": LOG}, f, indent=1)
    return evaluate(prepared, obs)


def evaluate(prepared, o):
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C19"]}
    obs_path = rel(os.path.join(NATIVE, "observations.json"))
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [obs_path] + [e for e in evidence if e]})

    b, f = o["bundle"], o["offer"]
    add("C19-N01", b["engine_sha"] and b["manifest_pin"] and not b["js_runtime"] and f["heading"] and f["engine_line"]
        and f["needs_tenant"] and f["start_disabled"] and f["needs_key"] and f["bound"],
        f"bundle {b}; offer {f}", [o["tree_offer"]])
    s = o["start"]
    add("C19-N02", s["ready"] and s["none_before"] and s["leader"] and s["own_group"] and s["args"] is True
        and not s["key_in_environment"] and s["path"] and s["home"] and s["homes"] == 1 and s["focus_message"],
        f"start {s}", [o["tree_ready"]])
    a = o["answer"]
    add("C19-N03", a["answered"] and a["done"] and a["sent"] == 1 and a["key"] and a["sandbox"] == "read-only"
        and a["stream"] is True and a["you"] and a["tokens"], f"answer {a}", [o["tree_answer"]])
    i = o["interrupt"]
    add("C19-N04", i["streaming"] and i["stop_offered"] and i["stopped"] and i["note"] and i["closed_by_client"]
        and i["continued"] and i["sent"] == 3, f"interrupt {i}", [o["tree_interrupt"]])
    e = o["end"]
    add("C19-N05", e["ended"] and e["reaped"] and e["homes"] == [] and e["status_focused"] and e["new_offered"],
        f"end {e}", [o["tree_ended"]])
    r = o["retire"]
    add("C19-N06", r["had_engine"] and r["workspace_engine_gone"] and r["view_withdrawn"] and r["homes_after_workspace"] == []
        and r["had_sleep_engine"] and r["sleep_shown"] and r["sleep_engine_gone"] and r["homes_after_sleep"] == [],
        f"retire {r}", [o["tree_retired"], o["tree_sleep"]])
    q = o["quit"]
    add("C19-N07", q["had_engine"] and q["engine_gone"] and q["homes"] == [] and q["plaintext_on_disk"] == []
        and q["nothing_restored"] and q["start_offered"] and q["new_conversation"], f"quit {q}", [o["tree_relaunch"]])
    y = o["a11y"]
    add("C19-N08", y["transcript_named"] and y["message_labelled"] and y["status_role"] and y["send_keyboard"]
        and y["end_reached_by_tab"] and y["ended_by_space"] and y["engine_gone"], f"keyboard and accessibility {y}",
        [o["tree_a11y"]])
    add("C19-N09", not o["secret_in_trees"] and not o["prompt_in_log"] and o["engine_paths"] == ["/rt/v1/responses"]
        and o["v1_unchanged"] and o["engines_left"] == [],
        f"engine requests: {o['harness_requests']} to {o['engine_paths']}; secrets in AX snapshots: {o['secret_in_trees']}; "
        f"prompt or key in the server log: {o['prompt_in_log']}; engines left: {o['engines_left']}; v1 unchanged: {o['v1_unchanged']}", [])
    c = o["crash"]
    add("C19-N10", c["killed"] and c["named"] and c["gone"] and c["homes"] == [] and c["restarted"] and c["new_engine"],
        f"crash {c}", [o["tree_crash"]])

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1",
        "subject": "C19",
        "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (tools/native-acceptance/ax.swift, System Events keystrokes) and the bundled engine's processes (ps, pgrep) against tools/native-acceptance/mgmt_server.py; tools/native-acceptance/acceptance_c19.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "The engine is the pinned build (9331a58) from the sealed archive; its inference goes to a synthetic local TLS Gateway, so no product endpoint is called before C25 and no live inference budget is used.",
            "Sleep is exercised by posting NSWorkspace's will-sleep notification in the app (a fixture menu item), which runs the production observer; the Mac does not actually sleep.",
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
