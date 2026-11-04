#!/usr/bin/env python3
"""C22 measurement of the MCP server session workload against the budgets
registered before C22 was built into the app
(docs/planning/evidence/C22/workload-budget.md, registration in
docs/planning/evidence/C22/workload-budget-registration.json).

  measure_c22.py --label calibration|acceptance

Run from the repository root on the prepared C22 fixture bundle. Only an
acceptance run writes docs/planning/evidence/<subject>/workload-perf.json
(in a phase, workload-perf-C22.json), and it refuses to start unless the
budget table matches its registration. Raw samples are recorded; `xtask
native` recomputes every statistic.

Every timing is tools/perf/probe.swift `activate`: from AXPress on the
named control to the named heading and a static text with the named prefix
existing in the accessibility tree (2 ms polling). The synthetic Gateway
plays the model (mode "harnessMcp"), and its answers are numbered.

- harness-mcp-catalog-listed: from "Read the servers again" to the panel
  heading and Tools' row named with that read's tag (mode "mcpTag").
- harness-mcp-session-ready: from "Start conversation" to "MCP servers:
  tools (ready)"; each conversation ended and cleared before the next.
- harness-mcp-approval-shown: from Send to "Run a workspace MCP tool".
- harness-mcp-call-answered: from "Approve once" to the numbered answer.
- harness-mcp-rotation-first-output: six-second MCP tokens; each Send 4 s
  after the previous answer, to the numbered answer.
- harness-mcp-process-tree-resident: RSS of the app, its WebKit processes
  and the engine, once a second for 20 s spanning a session start, two
  approved calls and one rotation.
"""

import argparse
import json
import os
import ssl
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "native-acceptance"))
import fixture_app as fx  # noqa: E402
import acceptance_c08 as c8  # noqa: E402
import acceptance_c09 as c9  # noqa: E402
import acceptance_c16 as c16  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C22")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
REGISTRATION = os.path.join(ROOT, "docs/planning/evidence/C22/workload-budget-registration.json")
KEY = "SYNTHETIC-C22-PERF-KEY"
HEADING = "Conversation in Team A"
PANEL = "Workspace MCP servers"
ENGINE = "Fixture.app/Contents/Resources/harness/airs-harness"
SERVER = "Tools tools"
MCP_CALL = {"server": "tools", "tool": "lookup", "arguments": json.dumps({"query": "record-1"})}


def sha256_file(path):
    import hashlib
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def utc():
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def webkit_pids():
    return set(int(p) for p in fx.sh(["pgrep", "-f", "com.apple.WebKit"]).stdout.split())


def engines():
    return [int(p) for p in fx.sh(["pgrep", "-f", ENGINE]).stdout.split()]


def rss_mib(pids):
    pids = [p for p in pids if p]
    out = fx.sh(["ps", "-o", "rss=", "-p", ",".join(str(p) for p in pids)]).stdout.split()
    return sum(int(v) for v in out) / 1024


def probe_activate(probe, app, role, label, heading, text, timeout=60000):
    out = fx.sh([probe, "activate", str(app.pid), role, label, heading, text, str(timeout)])
    return json.loads(out.stdout or "{}")


def requests(mgmt):
    """Gateway inference requests so far (each numbers the next answer)."""
    return sum(1 for r in mgmt.requests() if r["path"] in ("/rt/v1/responses", "/rt/v1/chat/completions")
               and not r.get("closedByClient"))


def type_prompt(app, text):
    app.type_into("AXTextArea", "Message", text)
    app.wait_for(lambda t: (fx.App.node(t, "AXButton", "Send") or {}).get("enabled") is not False, 5)


def settled(app, serial, timeout=40):
    return app.wait_for(lambda t: app.has_text(t, f"Answer {serial}:") and app.has_text(t, "Answered.")
                        and fx.App.node(t, "AXButton", "Approve once") is None, timeout)[1]


def end_and_clear(app):
    if fx.App.node(app.tree(), "AXButton", "End conversation"):
        app.ax("press", "AXButton", "End conversation")
        app.wait_for(lambda t: app.has_text(t, "You ended this conversation."), 10)
    deadline = time.time() + 10
    while engines() and time.time() < deadline:
        time.sleep(0.05)
    if fx.App.node(app.tree(), "AXButton", "Clear conversation"):
        app.ax("press", "AXButton", "Clear conversation")
        app.wait_for(lambda t: fx.App.node(t, "AXButton", "Start conversation") is not None, 10)


def sign_in(app, work, again):
    """Signs in to Tools, playing the browser."""
    before = len(fx.opened_urls())
    c9.press_exact(app, f"Sign in to {SERVER} again" if again else f"Sign in to {SERVER}")
    app.wait_for(lambda t: len(fx.opened_urls()) > before, 20)
    url = fx.opened_urls()[before]
    context = ssl.create_default_context(cafile=os.path.join(work, "endpoint", "ca.pem"))

    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, *args, **kwargs):
            return None

    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), urllib.request.HTTPSHandler(context=context), NoRedirect)
    try:
        opener.open(url, timeout=10)
        raise RuntimeError("the authorization was not redirected")
    except urllib.error.HTTPError as e:
        location = e.headers.get("location")
    urllib.request.build_opener(urllib.request.ProxyHandler({})).open(location, timeout=10)
    _, ok = app.wait_for(lambda t: app.has_text(t, "Signed in."), 20)
    if not ok:
        raise RuntimeError("MCP sign-in did not complete")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--label", choices=["calibration", "acceptance"], required=True)
    label = parser.parse_args().label
    registration = json.load(open(REGISTRATION))
    table = os.path.join(ROOT, registration["budgets"])
    table_sha = sha256_file(table)
    if table_sha != registration["budgets_sha256"]:
        raise SystemExit("the workload budget table does not match its C22 registration")
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")
    if sha256_file(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c22-perf-")
    probe = os.path.join(work, "probe")
    built = fx.sh(["swiftc", "-O", os.path.join(HERE, "probe.swift"), "-o", probe])
    if built.returncode != 0:
        raise SystemExit(f"probe did not build: {built.stderr}")
    started = utc()
    app = fx.App(bundle, work, [])
    app.quit()
    fx.sh(["pkill", "-9", "-f", ENGINE])
    fx.clean_fixture_root()
    endpoint = fx.ScanEndpoint(os.path.join(work, "endpoint"), c8.SCAN_KEY)
    endpoint.install_roots()
    endpoint.start()
    mgmt = c8.MgmtEndpoint(os.path.join(work, "endpoint"), os.path.join(work, "mgmt"))
    mgmt.install()
    mgmt.start()
    mgmt.mode()
    metrics, details = {}, {}
    try:
        app.launch()
        app.open_settings()
        app.type_into("AXTextField", "TSG ID (optional)", c8.TSG)
        app.create_custom_tenant("Tenant A", endpoint.origin)
        app.save_key(c8.SCAN_KEY)
        c8.save_client(app)
        c16.open_gateway(app)
        c8.text(app, "2 workspaces", 25)
        app.ax("press", "AXRadioButton", "Team A")
        c8.text(app, "2 providers · complete", 15)
        c16.tab(app, "Playground")
        app.type_into("AXTextField", "Runtime endpoint", f"{mgmt.origin}/rt/v1")
        app.type_into("AXTextField", "Workspace API key with completions access", KEY, secure=True)
        app.ax("press", "AXButton", "Store key for Team A")
        c8.text(app, "A key for Team A is stored", 15)
        app.ax("press", "AXButton", "Harness")
        app.wait_for(lambda t: fx.App.node(t, "AXCheckBox", f"Use {SERVER} in new conversations") is not None, 20)
        app.ax("press", "AXCheckBox", f"Use {SERVER} in new conversations")
        sign_in(app, work, again=False)

        # ---- harness-mcp-catalog-listed ----
        timings, runs = [], []
        for n in range(10):
            tag = f"R{n + 1}"
            mgmt.mode(mcpTag=tag)
            data = probe_activate(probe, app, "AXButton", "Read the servers again", PANEL, f"Use {SERVER} {tag}", 30000)
            timings.append(data.get("ms"))
            runs.append({"sample": n + 1, "tag": tag, **data})
        # The untagged catalog again (the names above carry the last tag).
        mgmt.mode()
        app.ax("press", "AXButton", "Read the servers again")
        _, back = app.wait_for(lambda t: fx.App.node(t, "AXCheckBox", f"Use {SERVER} in new conversations") is not None, 20)
        if not back:
            raise RuntimeError("the untagged catalog was not read again")
        if any(t is None for t in timings):
            raise RuntimeError(f"the catalog was not listed: {runs}")
        metrics["harness-mcp-catalog-listed"] = {"unit": "ms", "samples": timings}
        details["catalog"] = runs

        # ---- harness-mcp-session-ready ----
        timings, runs = [], []
        for n in range(10):
            data = probe_activate(probe, app, "AXButton", "Start", HEADING, "MCP servers: tools (ready)", 30000)
            timings.append(data.get("ms"))
            runs.append({"sample": n + 1, **data})
            end_and_clear(app)
        if any(t is None for t in timings):
            raise RuntimeError(f"a session did not become ready: {runs}")
        metrics["harness-mcp-session-ready"] = {"unit": "ms", "samples": timings}
        details["ready"] = runs

        # ---- harness-mcp-approval-shown and harness-mcp-call-answered ----
        mgmt.mode(harnessMcp=MCP_CALL)
        app.ax("press", "AXButton", "Start")
        app.wait_for(lambda t: app.has_text(t, "MCP servers: tools (ready)"), 30)
        shown, done, runs = [], [], []
        for n in range(10):
            serial = requests(mgmt) + 2
            type_prompt(app, f"Look it up {n + 1}")
            first = probe_activate(probe, app, "AXButton", "Send", "Waiting for your approval", "Run a workspace MCP tool", 30000)
            second = probe_activate(probe, app, "AXButton", "Approve once", HEADING, f"Answer {serial}:", 30000)
            ok = settled(app, serial)
            shown.append(first.get("ms"))
            done.append(second.get("ms"))
            runs.append({"sample": n + 1, "answered": ok, "shown": first, "done": second})
        if any(t is None for t in shown + done) or not all(r["answered"] for r in runs):
            raise RuntimeError(f"an MCP call was not approved and answered: {runs}")
        metrics["harness-mcp-approval-shown"] = {"unit": "ms", "samples": shown}
        metrics["harness-mcp-call-answered"] = {"unit": "ms", "samples": done}
        details["calls"] = runs
        end_and_clear(app)

        # ---- harness-mcp-rotation-first-output ----
        mgmt.mode(mcpLifetime=6)
        sign_in(app, work, again=True)
        app.ax("press", "AXButton", "Start")
        app.wait_for(lambda t: app.has_text(t, "MCP servers: tools (ready)"), 30)
        timings, runs = [], []
        for n in range(10):
            time.sleep(4)
            serial = requests(mgmt) + 1
            type_prompt(app, f"Rotated {n + 1}")
            data = probe_activate(probe, app, "AXButton", "Send", HEADING, f"Answer {serial}:", 30000)
            ok = settled(app, serial)
            timings.append(data.get("ms"))
            runs.append({"sample": n + 1, "answered": ok, **data})
        refreshes = sum(1 for r in mgmt.requests() if r.get("mcpPlane") == "/mcp-oauth/token" and r.get("grantType") == "refresh_token")
        details["refreshes"] = refreshes
        if any(t is None for t in timings) or not all(r["answered"] for r in runs) or refreshes < 10:
            raise RuntimeError(f"a rotated answer was not measured: {runs}, refreshes {refreshes}")
        metrics["harness-mcp-rotation-first-output"] = {"unit": "ms", "samples": timings}
        details["rotation"] = runs
        end_and_clear(app)

        # ---- harness-mcp-process-tree-resident ----
        app.quit()
        before = webkit_pids()
        app.launch()
        time.sleep(2)
        pids = [app.pid, *sorted(webkit_pids() - before)]
        details["process_tree"] = {"app": app.pid, "webkit": pids[1:]}
        c16.open_gateway(app)
        c8.text(app, "2 workspaces", 25)
        app.ax("press", "AXRadioButton", "Team A")
        c8.text(app, "2 providers · complete", 15)
        app.ax("press", "AXButton", "Harness")
        app.wait_for(lambda t: fx.App.node(t, "AXCheckBox", f"Use {SERVER} in new conversations") is not None, 20)
        mgmt.mode(mcpLifetime=6, harnessMcp=MCP_CALL)
        resident = []
        stop = threading.Event()

        def sample():
            for _ in range(20):
                resident.append(round(rss_mib(pids + engines()), 1))
                if stop.wait(1):
                    break

        sampler = threading.Thread(target=sample)
        sampler.start()
        steps = []
        app.ax("press", "AXButton", "Start")
        steps.append(("ready", app.wait_for(lambda t: app.has_text(t, "MCP servers: tools (ready)"), 30)[1]))
        for n in range(2):
            serial = requests(mgmt) + 2
            type_prompt(app, f"Resident call {n + 1}")
            app.ax("press", "AXButton", "Send")
            app.wait_for(lambda t: fx.App.node(t, "AXButton", "Approve once") is not None, 20)
            c9.press_exact(app, "Approve once")
            steps.append(("call", settled(app, serial)))
        time.sleep(4)
        mgmt.mode(mcpLifetime=6)
        serial = requests(mgmt) + 1
        type_prompt(app, "Resident rotation")
        app.ax("press", "AXButton", "Send")
        steps.append(("rotation", settled(app, serial)))
        sampler.join()
        end_and_clear(app)
        if len(resident) != 20 or not all(ok for _, ok in steps):
            raise RuntimeError(f"the resident workload did not complete: {steps}, {len(resident)} samples")
        metrics["harness-mcp-process-tree-resident"] = {"unit": "MiB", "samples": resident}
        details["resident_steps"] = steps
    finally:
        app.quit()
        fx.sh(["pkill", "-9", "-f", ENGINE])
        endpoint.stop()
        mgmt.stop()
        fx.clean_fixture_root()
    record = {
        "schema": "airs-v2-workload-perf/1",
        "label": label,
        "registration_sha256": table_sha,
        "started_utc": started,
        "finished_utc": utc(),
        "executable_sha256": prepared["executable_sha256"],
        "host": fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip(),
        "metrics": metrics,
        "details": details,
    }
    own = "workload-perf.json" if SUBJECT == "C22" else "workload-perf-C22.json"
    name = own if label == "acceptance" else f"workload-perf-calibration-{started.replace(':', '')}.json"
    path = os.path.join(EVIDENCE, name)
    with open(path, "w") as f:
        json.dump(record, f, indent=1)
        f.write("\n")
    for key, metric in metrics.items():
        ordered = sorted(metric["samples"])
        p95 = ordered[max(1, -(-95 * len(ordered) // 100)) - 1]
        print(f"{key}: n={len(ordered)} p95={p95} max={ordered[-1]} {metric['unit']}")
    print(os.path.relpath(path, ROOT))
    return 0


if __name__ == "__main__":
    sys.exit(main())
