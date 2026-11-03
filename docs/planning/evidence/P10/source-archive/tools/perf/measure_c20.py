#!/usr/bin/env python3
"""C20 measurement of the protected Harness history workload against the
budgets registered before C20 was built into the app
(docs/planning/evidence/C20/workload-budget.md, registration in
docs/planning/evidence/C20/workload-budget-registration.json).

  measure_c20.py --label calibration|acceptance

Run from the repository root on the prepared C20 fixture bundle. Only an
acceptance run writes docs/planning/evidence/<subject>/workload-perf.json
(in a phase, workload-perf-C20.json), and it refuses to start unless the
budget table matches its registration. Raw samples are recorded; `xtask
native` recomputes every statistic.

Every timing is tools/perf/probe.swift `activate`: from AXPress on the
named control to the named heading and a static text with the named prefix
existing in the accessibility tree (2 ms polling). Answers are numbered by
the synthetic server ("Answer <n>: ..."), so a sample waits for its own.

- harness-history-saved: long conversation A (1 KiB answers), turns 41-50:
  from Send to the Saved conversations list showing "<2k> messages".
- harness-history-listed: full history (A, B and 14 one-turn
  conversations); after each relaunch, with Team A chosen again, from the
  Harness destination to "Saved conversations" and A's title.
- harness-history-opened: alternating A and B, from "Open <title>" to the
  title's heading and its last answer.
- harness-history-resume-ready: A opened, from "Continue this
  conversation" to "Conversation in Team A" and "Ready."; ended and cleared.
- harness-refresh-restart-first-output: company sign-in with 4 s tokens;
  each Send 2.5 s after the previous one, to the answer's first text.
- harness-history-process-tree-resident: RSS of the app, its WebKit
  processes and the engine, once a second for 20 s while the list is shown,
  A and B are opened and B is continued with three short answers.
"""

import argparse
import hashlib
import json
import os
import sys
import tempfile
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "native-acceptance"))
import fixture_app as fx  # noqa: E402
import acceptance_c08 as c8  # noqa: E402
import acceptance_c16 as c16  # noqa: E402
import acceptance_c20 as c20  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C20")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
REGISTRATION = os.path.join(ROOT, "docs/planning/evidence/C20/workload-budget-registration.json")
KEY = "SYNTHETIC-C20-PERF-KEY"
HEADING = "Conversation in Team A"
SAVED = "Saved conversations"
ENGINE = "Fixture.app/Contents/Resources/harness/airs-harness"
TITLE_A = "Conversation A turn 1"
TITLE_B = "Conversation B turn 1"


def sha256_file(path):
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


def answered(mgmt):
    return sum(1 for r in mgmt.requests() if r["path"] == "/rt/v1/responses" and not r.get("closedByClient"))


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


def start(app):
    app.ax("press", "AXButton", "Start conversation")
    _, ok = app.wait_for(lambda t: app.has_text(t, "Ready."), 30)
    return ok


def type_prompt(app, text):
    app.type_into("AXTextArea", "Message", text)
    app.wait_for(lambda t: (fx.App.node(t, "AXButton", "Send") or {}).get("enabled") is not False, 5)


def turn(app, mgmt, text):
    """Sends one message and waits for its own numbered answer and save."""
    serial = answered(mgmt) + 1
    type_prompt(app, text)
    app.ax("press", "AXButton", "Send")
    _, ok = app.wait_for(lambda t: app.has_text(t, f"Answer {serial}:") and app.has_text(t, "Answered."), 30)
    if not ok:
        raise RuntimeError(f"no answer {serial} to {text!r}")
    return serial


def choose_team_a(app):
    c16.open_gateway(app)
    c8.text(app, "2 workspaces", 25)
    app.ax("press", "AXRadioButton", "Team A")
    c8.text(app, "2 providers · complete", 15)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--label", choices=["calibration", "acceptance"], required=True)
    label = parser.parse_args().label
    registration = json.load(open(REGISTRATION))
    table = os.path.join(ROOT, registration["budgets"])
    table_sha = sha256_file(table)
    if table_sha != registration["budgets_sha256"]:
        raise SystemExit("the workload budget table does not match its C20 registration")
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")
    if sha256_file(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c20-perf-")
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
        choose_team_a(app)
        c16.tab(app, "Playground")
        app.type_into("AXTextField", "Runtime endpoint", f"{mgmt.origin}/rt/v1")
        app.type_into("AXTextField", "Workspace API key with completions access", KEY, secure=True)
        app.ax("press", "AXButton", "Store key for Team A")
        c8.text(app, "A key for Team A is stored", 15)
        app.ax("press", "AXButton", "Harness")
        app.wait_for(lambda t: fx.App.node(t, "AXButton", "Start conversation") is not None, 10)

        # ---- harness-history-saved (and long conversation A) ----
        mgmt.mode(harness="kib")
        if not start(app):
            raise RuntimeError("conversation A did not start")
        last = {}
        for k in range(1, 41):
            last["A"] = turn(app, mgmt, f"Conversation A turn {k}")
        timings, runs = [], []
        for k in range(41, 51):
            serial = answered(mgmt) + 1
            type_prompt(app, f"Conversation A turn {k}")
            data = probe_activate(probe, app, "AXButton", "Send", SAVED, f"{2 * k} messages", 30000)
            _, ok = app.wait_for(lambda t: app.has_text(t, f"Answer {serial}:") and app.has_text(t, "Answered."), 15)
            last["A"] = serial
            timings.append(data.get("ms"))
            runs.append({"turn": k, "answered": ok, **data})
        if any(t is None for t in timings) or not all(r["answered"] for r in runs):
            raise RuntimeError(f"a turn was not saved: {runs}")
        metrics["harness-history-saved"] = {"unit": "ms", "samples": timings}
        details["saved"] = runs
        end_and_clear(app)

        # ---- the rest of the full history: B and fourteen one-turn ----
        if not start(app):
            raise RuntimeError("conversation B did not start")
        for k in range(1, 51):
            last["B"] = turn(app, mgmt, f"Conversation B turn {k}")
        end_and_clear(app)
        mgmt.mode()
        for n in range(14):
            if not start(app):
                raise RuntimeError(f"short conversation {n} did not start")
            turn(app, mgmt, f"Short conversation {n + 1}")
            end_and_clear(app)
        _, full = app.wait_for(lambda t: app.has_text(t, "This workspace already keeps 16 saved conversations."), 10)
        details["full_history"] = full
        if not full:
            raise RuntimeError("the full history was not made")

        # ---- harness-history-listed ----
        timings, runs = [], []
        for n in range(10):
            app.quit()
            app.launch()
            choose_team_a(app)
            data = probe_activate(probe, app, "AXButton", "Harness", SAVED, TITLE_A, 30000)
            timings.append(data.get("ms"))
            runs.append({"sample": n + 1, **data})
        if any(t is None for t in timings):
            raise RuntimeError(f"the history was not listed: {runs}")
        metrics["harness-history-listed"] = {"unit": "ms", "samples": timings}
        details["listed"] = runs

        # ---- harness-history-opened ----
        timings, runs = [], []
        for n in range(10):
            title, which = (TITLE_A, "A") if n % 2 == 0 else (TITLE_B, "B")
            data = probe_activate(probe, app, "AXButton", f"Open {title}", title, f"Answer {last[which]}:", 30000)
            timings.append(data.get("ms"))
            runs.append({"sample": n + 1, "conversation": which, **data})
        if any(t is None for t in timings):
            raise RuntimeError(f"a conversation did not open: {runs}")
        metrics["harness-history-opened"] = {"unit": "ms", "samples": timings}
        details["opened"] = runs

        # ---- harness-history-resume-ready ----
        timings, runs = [], []
        for n in range(10):
            app.ax("press", "AXButton", f"Open {TITLE_A}")
            app.wait_for(lambda t: fx.App.node(t, "AXButton", "Continue this conversation") is not None, 15)
            data = probe_activate(probe, app, "AXButton", "Continue this conversation", HEADING, "Ready.", 30000)
            timings.append(data.get("ms"))
            runs.append({"sample": n + 1, "engines": len(engines()), **data})
            end_and_clear(app)
        if any(t is None for t in timings):
            raise RuntimeError(f"a conversation did not continue: {runs}")
        metrics["harness-history-resume-ready"] = {"unit": "ms", "samples": timings}
        details["resume"] = runs

        # ---- harness-history-process-tree-resident ----
        app.quit()
        before = webkit_pids()
        app.launch()
        time.sleep(2)
        pids = [app.pid, *sorted(webkit_pids() - before)]
        details["process_tree"] = {"app": app.pid, "webkit": pids[1:]}
        choose_team_a(app)
        resident = []
        stop = threading.Event()

        def sample():
            for _ in range(20):
                resident.append(round(rss_mib(pids + engines()), 1))
                if stop.wait(1):
                    break

        sampler = threading.Thread(target=sample)
        sampler.start()
        app.ax("press", "AXButton", "Harness")
        steps = [("listed", app.wait_for(lambda t: app.has_text(t, TITLE_A), 15)[1])]
        for title in (TITLE_A, TITLE_B):
            app.ax("press", "AXButton", f"Open {title}")
            steps.append((f"open {title}", app.wait_for(lambda t: fx.App.node(t, "AXHeading", title) is not None, 15)[1]))
        app.ax("press", "AXButton", "Continue this conversation")
        steps.append(("continue", app.wait_for(lambda t: app.has_text(t, "Ready.") and fx.App.node(t, "AXHeading", HEADING) is not None, 30)[1]))
        for n in range(3):
            turn(app, mgmt, f"Resident {n + 1}")
            steps.append(("short", True))
        sampler.join()
        end_and_clear(app)
        if len(resident) != 20 or not all(ok for _, ok in steps):
            raise RuntimeError(f"the resident workload did not complete: {steps}, {len(resident)} samples")
        metrics["harness-history-process-tree-resident"] = {"unit": "MiB", "samples": resident}
        details["resident_steps"] = steps

        # ---- harness-refresh-restart-first-output ----
        mgmt.mode(idpLifetime=4)
        issuer = f"{mgmt.origin}/idp"
        app.ax("press", "AXButton", "Set up company sign-in")
        app.wait_for(lambda t: fx.App.node(t, "AXTextField", "Identity provider") is not None, 10)
        for label_, value in (("Gateway endpoint", f"{mgmt.origin}/rt/v1"), ("Identity provider", issuer),
                              ("Client ID", c20.CLIENT), ("Gateway audience", c20.AUDIENCE)):
            app.type_into("AXTextField", label_, value)
        app.ax("press", "AXButton", "Save company sign-in")
        c8.text(app, "Sign in with your browser", 10)
        _, step = c20.sign_in(app, work, issuer)
        if not step.get("signed_in"):
            raise RuntimeError(f"company sign-in did not complete: {step}")
        if not start(app):
            raise RuntimeError("the signed-in conversation did not start")
        timings, runs = [], []
        sent_at = time.monotonic()
        for n in range(10):
            serial = answered(mgmt) + 1
            type_prompt(app, f"Renewed {n + 1}")
            time.sleep(max(0.0, 2.5 - (time.monotonic() - sent_at)))
            sent_at = time.monotonic()
            data = probe_activate(probe, app, "AXButton", "Send", HEADING, f"Answer {serial}:", 30000)
            _, ok = app.wait_for(lambda t: app.has_text(t, "Answered.") and app.has_text(t, f"Answer {serial}:"), 15)
            timings.append(data.get("ms"))
            runs.append({"sample": n + 1, "answered": ok, **data})
        refreshes = sum(1 for r in mgmt.requests() if r.get("idp") == "/token" and r.get("grantType") == "refresh_token")
        details["refreshes"] = refreshes
        if any(t is None for t in timings) or not all(r["answered"] for r in runs) or refreshes < 10:
            raise RuntimeError(f"a renewed answer was not measured: {runs}, refreshes {refreshes}")
        metrics["harness-refresh-restart-first-output"] = {"unit": "ms", "samples": timings}
        details["refresh"] = runs
        end_and_clear(app)
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
    own = "workload-perf.json" if SUBJECT == "C20" else "workload-perf-C20.json"
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
