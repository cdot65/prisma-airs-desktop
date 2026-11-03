#!/usr/bin/env python3
"""C05 measurement of the Academy hosted content and learner sync workload
against the budgets C04 registered before C05 existed
(docs/planning/evidence/C04/workload-budget.md, registration in
docs/planning/evidence/C04/workload-budget-registration.json).

  measure_c05.py --label calibration|acceptance

Run from the repository root on the prepared C05 fixture bundle. Only an
acceptance run writes docs/planning/evidence/<subject>/workload-perf.json,
and it refuses to start unless the budget table matches its registration.
Raw samples are recorded; `xtask native` recomputes every statistic.

Workloads, on one signed-in launch against the synthetic Academy endpoint
(academy_server.py serving academy_catalog.py), which answers immediately
and logs its own handling time per request:

- learner-call-overhead: the fixture-only learner call probe in the Academy
  destination issues 100 sequential `courses.list` queries through the
  production desktop tRPC link and native transport; each sample is the
  dispatch-to-result time in the link minus that request's handling time in
  the endpoint log.
- catalog-to-lesson: 20 distinct 50 KiB lessons, each activated from the
  course page's module list; from the AXPress to the lesson heading and its
  first paragraph existing in the Accessibility tree (tools/perf/probe.swift
  activate, 2 ms polling).
- academy-process-tree-resident: after those lessons (more than ten, in
  sequence), the app plus its WebKit processes' RSS sampled once per second
  for 20 s.
"""

import argparse
import hashlib
import json
import os
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "native-acceptance"))
import academy_catalog  # noqa: E402
import fixture_app as fx  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C05")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
REGISTRATION = os.path.join(ROOT, "docs/planning/evidence/C04/workload-budget-registration.json")
COURSE = "Runtime Security Foundations"
CATALOG = "What will you take up next?"
LESSONS = ([("m-rt-1", n) for n in range(1, 13)] + [("m-rt-2", n) for n in range(1, 9)])


def sha256_file(path):
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def utc():
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def webkit_pids():
    return set(int(p) for p in fx.sh(["pgrep", "-f", "com.apple.WebKit"]).stdout.split())


def rss_mib(pids):
    out = fx.sh(["ps", "-o", "rss=", "-p", ",".join(str(p) for p in pids)]).stdout.split()
    return sum(int(v) for v in out) / 1024


def heading(tree, title, level=None):
    return next((n for n in tree if n.get("role") == "AXHeading" and n.get("label") == title
                 and (level is None or n.get("value") == level)), None)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--label", choices=["calibration", "acceptance"], required=True)
    label = parser.parse_args().label
    registration = json.load(open(REGISTRATION))
    table = os.path.join(ROOT, registration["budgets"])
    table_sha = sha256_file(table)
    if table_sha != registration["budgets_sha256"]:
        raise SystemExit("the workload budget table does not match its C04 registration")
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")
    if sha256_file(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c05-perf-")
    probe = os.path.join(work, "probe")
    built = fx.sh(["swiftc", "-O", os.path.join(HERE, "probe.swift"), "-o", probe])
    if built.returncode != 0:
        raise SystemExit(f"probe did not build: {built.stderr}")
    started = utc()
    app = fx.App(bundle, work, [])
    app.quit()
    fx.clean_fixture_root()
    endpoint = fx.AcademyEndpoint(os.path.join(work, "academy"))
    with open(os.path.join(endpoint.dir, "catalog.json"), "w") as f:
        json.dump(academy_catalog.build(ROOT), f)
    endpoint.install()
    endpoint.start()
    endpoint.mode(approval="granted", displayName="Ada Lovelace")
    metrics, details = {}, {}
    try:
        before = webkit_pids()
        app.launch()
        time.sleep(2)
        pids = [app.pid, *sorted(webkit_pids() - before)]
        details["process_tree"] = {"app": app.pid, "webkit": pids[1:],
                                   "commands": [fx.sh(["ps", "-o", "comm=", "-p", str(p)]).stdout.strip() for p in pids[1:]]}
        app.open_settings()
        app.ax("press", "AXButton", "Sign in to Academy")
        _, ok = app.wait_for(lambda t: app.has_text(t, "Signed in to Academy as"), 15)
        if not ok:
            raise RuntimeError("the measurement learner did not sign in")
        app.ax("press", "AXButton", "Academy")
        app.wait_for(lambda t: heading(t, CATALOG) is not None)
        time.sleep(2)

        # ---- learner-call-overhead ----
        mark = len(endpoint.requests())
        app.ax("press", "AXButton", "Run learner call probe")
        deadline = time.time() + 120
        samples = None
        while time.time() < deadline and samples is None:
            time.sleep(1)
            tree = app.tree()
            for i, n in enumerate(tree):
                if n.get("label") == "Learner call probe samples":
                    for m in tree[i:i + 4]:
                        value = m.get("value")
                        if isinstance(value, str) and value.startswith("["):
                            samples = json.loads(value)
                            break
                    break
        if samples is None:
            raise RuntimeError("the learner call probe produced no samples")
        handled = [r["handlingMs"] for r in endpoint.requests()[mark:] if r.get("procedure") == "courses.list"]
        if len(handled) != len(samples):
            raise RuntimeError(f"{len(samples)} probe samples but {len(handled)} logged courses.list requests")
        metrics["learner-call-overhead"] = {"unit": "ms", "samples": [round(s - h, 3) for s, h in zip(samples, handled)]}
        details["learner_call"] = {"link_ms": samples, "endpoint_handling_ms": handled}

        # ---- catalog-to-lesson ----
        app.ax("press", "AXCheckBox", "Courses")
        app.wait_for(lambda t: app.node(t, "AXLink", f"COURSE {COURSE}") is not None, 5)
        app.ax("press", "AXLink", f"COURSE {COURSE}")
        app.wait_for(lambda t: heading(t, COURSE, 1) is not None)
        timings, runs = [], []
        for module, n in LESSONS:
            title = academy_catalog.lesson_title(module, n)
            if heading(app.tree(), COURSE, 1) is None:
                app.ax("press", "AXLink", COURSE)
                app.wait_for(lambda t: heading(t, COURSE, 1) is not None)
            if app.node(app.tree(), "AXLink", f"{title} 5 min") is None:
                index = [m for m, _ in academy_catalog.MODULES].index(module) + 1
                app.ax("press", "AXButton", f"{index}\n")
                app.wait_for(lambda t, title=title: app.node(t, "AXLink", f"{title} 5 min") is not None, 5)
            time.sleep(0.5)
            paragraph = f"Lesson {n} of {dict(academy_catalog.MODULES)[module]} opens here"
            out = fx.sh([probe, "activate", str(app.pid), "AXLink", f"{title} 5 min", title, paragraph, "10000"])
            data = json.loads(out.stdout or "{}")
            timings.append(data.get("ms"))
            runs.append({"lesson": title, **data})
            time.sleep(1)
        if any(t is None for t in timings):
            raise RuntimeError(f"a lesson did not appear in time: {runs}")
        metrics["catalog-to-lesson"] = {"unit": "ms", "samples": timings}
        details["catalog_to_lesson"] = runs

        # ---- academy-process-tree-resident ----
        resident = []
        for _ in range(20):
            resident.append(round(rss_mib(pids), 1))
            time.sleep(1)
        metrics["academy-process-tree-resident"] = {"unit": "MiB", "samples": resident}
        details["requests"] = len(endpoint.requests())
    finally:
        app.quit()
        endpoint.stop()
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
    # Recorded for another subject (a phase, or C24's cumulative closure)
    # under this chunk's name.
    own = "workload-perf.json" if SUBJECT == "C05" else "workload-perf-C05.json"
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
