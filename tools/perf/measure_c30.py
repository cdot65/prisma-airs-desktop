#!/usr/bin/env python3
"""C30 current-presentation cached-switch measurement; synthetic services only.

AIRS_NATIVE_SUBJECT=P09 python3 -I tools/perf/measure_c30.py --label acceptance --attempt attempt-01

The immutable original C30 budget/registration stays binding. Method 2 and its
VM/presentation rationale are recorded before measuring in P09/performance/C30.
Every raw trial is retained, two fixed warmups then ten alternating samples.
An existing canonical acceptance or attempt directory is never overwritten.
"""

import argparse
import datetime
import hashlib
import json
import os
import math
import platform
import shutil
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "native-acceptance"))
import fixture_app as fx  # noqa: E402
import acceptance_c08 as c8  # noqa: E402
import v2_setup as v2s  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C30")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
REGISTRATION = os.path.join(ROOT, "docs/planning/evidence/C30/workload-budget-registration.json")
METHOD = os.path.join(ROOT, "docs/planning/evidence/P09/performance/C30/method-02.md")


def sha256_file(path):
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def utc():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def open_switcher(app):
    app.menu("Go", "Switch Tenant…")
    _, shown = app.wait_for(lambda t: any(n.get("role") == "AXGroup" and (n.get("label") or "") == "Switch tenant" for n in t), 8)
    if not shown:
        raise RuntimeError("the current tenant switcher did not open")
    time.sleep(0.3)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--label", choices=["calibration", "acceptance"], required=True)
    parser.add_argument("--attempt", default="attempt-01")
    args = parser.parse_args()
    label = args.label
    if not args.attempt.startswith("attempt-") or not args.attempt[8:].isdigit():
        raise SystemExit("choose a fresh numbered attempt")
    own = "workload-perf.json" if SUBJECT == "C30" else "workload-perf-C30.json"
    canonical = os.path.join(EVIDENCE, own)
    if label == "acceptance" and os.path.exists(canonical):
        raise SystemExit("preserve/archive the existing performance record before another acceptance")
    output = os.path.join(EVIDENCE, "performance", "C30", args.attempt)
    os.makedirs(output, exist_ok=False)
    source_paths = [os.path.join(HERE, "measure_c30.py"), os.path.join(HERE, "probe.swift"), METHOD]
    source_hashes = {path: sha256_file(path) for path in source_paths}
    procedure = os.path.join(ROOT, "docs/planning/evidence/P09/runs/hash-inputs.sh")
    before_file = os.path.join(output, "inputs-before.sha256")
    before = subprocess.check_output(["bash", procedure, ROOT, before_file], text=True).strip()
    seal_path = os.path.join(EVIDENCE, "native-build-inputs.json")
    seal = json.load(open(seal_path))
    if before != seal["inputs"]["before_digest"]:
        raise SystemExit("current compiled inputs differ from prepared native build")
    registration = json.load(open(REGISTRATION))
    table = os.path.join(ROOT, registration["budgets"])
    table_sha = sha256_file(table)
    if table_sha != registration["budgets_sha256"]:
        raise SystemExit("the workload budget table does not match its C30 registration")
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    prepare_hash = sha256_file(os.path.join(EVIDENCE, "native-prepare.json"))
    build_hash = sha256_file(seal_path)
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")
    if sha256_file(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    with open(os.path.join(output, "attempt-start.json"), "x") as f:
        json.dump({"started_utc": utc(), "command": list(sys.orig_argv), "source_hashes": source_hashes,
                   "prepare_sha256": prepare_hash, "build_inputs_sha256": build_hash,
                   "executable_sha256": prepared["executable_sha256"], "input_digest": before}, f, indent=2)
    work = tempfile.mkdtemp(prefix="airs-v2-c30-perf-")
    probe = os.path.join(work, "probe")
    built = fx.sh(["swiftc", "-O", os.path.join(HERE, "probe.swift"), "-o", probe])
    with open(os.path.join(output, "probe-build.json"), "x") as f:
        json.dump({"argv": ["swiftc", "-O", os.path.join(HERE, "probe.swift"), "-o", probe],
                   "exit": built.returncode, "stdout": built.stdout, "stderr": built.stderr}, f, indent=2)
    if built.returncode != 0:
        raise SystemExit(f"probe did not build: {built.stderr}")
    started = utc()
    app = fx.App(bundle, work, [])
    app.quit()
    fx.clean_fixture_root()
    endpoint = fx.ScanEndpoint(os.path.join(work, "endpoint"), c8.SCAN_KEY)
    endpoint.install_roots()
    endpoint.start()
    mgmt = c8.MgmtEndpoint(os.path.join(work, "endpoint"), os.path.join(work, "mgmt"))
    mgmt.install()
    mgmt.start()
    mgmt.mode()
    shutil.copy2(probe, os.path.join(output, "timing-probe.bin"))
    metrics, details = {}, {"raw_switches": []}
    failure = None
    conditions = {"platform": platform.platform(), "machine": platform.machine(),
                  "macos": fx.sh(["sw_vers", "-productVersion"]).stdout.strip(),
                  "swiftc": fx.sh(["swiftc", "--version"]).stdout.strip(),
                  "load_average": list(os.getloadavg()),
                  "execution_environment": "owner-authorized Worf macOS guest; guest-level observations only"}
    try:
        app.launch()
        conditions["fixture_pid"] = app.pid
        # The one current presentation: actual first-run wizard, then the
        # available tenant Settings form. No removed variant switch is required.
        _, home = v2s.tenant_on_home(app, endpoint.origin, label="Tenant A", extra=("Tenant B",))
        if not home:
            raise RuntimeError("current presentation setup did not reach Home")
        app.ax("press", "AXButton", "Refresh")
        tree, read = app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Home") is not None and app.has_text(t, "Read finished at"), 30)
        if not read:
            raise RuntimeError("Tenant A's single read did not settle")
        details["a_read"] = True
        conditions["window"] = next(({k: n.get(k) for k in ("pos", "size")} for n in tree if n.get("role") == "AXWindow"), None)
        details["management_requests_before"] = len(mgmt.requests())
        samples = []
        current = "Tenant A"
        for index in range(12):
            other = "Tenant B" if current == "Tenant A" else "Tenant A"
            open_switcher(app)
            # Shown means the destination's Home with its own cached read status: Tenant A's
            # finished read, Tenant B's first read still pending (never the other's).
            status = "Read finished at" if other == "Tenant A" else "First read pending"
            app.front()
            argv = [probe, "switch-home", str(app.pid), other, status, "10000", exe]
            out = fx.sh(argv)
            result = json.loads(out.stdout or "{}")
            details["raw_switches"].append({"index": index, "from": current, "to": other,
                "warmup": index < 2, "exit": out.returncode, "probe": result})
            with open(os.path.join(output, f"probe-{index:02}.json"), "x") as f:
                json.dump({"argv": argv, "exit": out.returncode, "stdout": out.stdout, "stderr": out.stderr}, f, indent=2)
            measured = result.get("ms")
            if out.returncode != 0 or type(measured) not in (int, float) or not math.isfinite(measured) or measured <= 0:
                raise RuntimeError(f"switch {index} did not produce a valid native timing")
            samples.append(measured)
            after_tree = app.tree()
            with open(os.path.join(output, f"switch-{index:02}-ax.json"), "x") as f:
                json.dump(after_tree, f)
            details["raw_switches"][-1]["tree"] = os.path.relpath(os.path.join(output, f"switch-{index:02}-ax.json"), ROOT)
            details["raw_switches"][-1]["tree_sha256"] = sha256_file(os.path.join(output, f"switch-{index:02}-ax.json"))
            current = other
            time.sleep(0.5)
        metrics["tenant-switch-home"] = {"unit": "ms", "samples": samples[2:]}
        details["switches"] = len(samples)
        details["management_requests_after"] = len(mgmt.requests())
        if details["management_requests_after"] != details["management_requests_before"]:
            raise RuntimeError("cached switches unexpectedly made synthetic management requests")
    except Exception as error:
        failure = f"{type(error).__name__}: {error}"
    finally:
        app.quit()
        endpoint.stop()
        mgmt.stop()
        fx.clean_fixture_root()
    after_file = os.path.join(output, "inputs-after.sha256")
    after = subprocess.check_output(["bash", procedure, ROOT, after_file], text=True).strip()
    if before != after or sha256_file(exe) != prepared["executable_sha256"]:
        failure = "compiled inputs or prepared executable changed during measurement"
    sources_unchanged = all(sha256_file(path) == digest for path, digest in source_hashes.items())
    if not sources_unchanged or sha256_file(os.path.join(EVIDENCE, "native-prepare.json")) != prepare_hash or sha256_file(seal_path) != build_hash:
        failure = "timing sources or prepared records changed during measurement"
    observed = metrics.get("tenant-switch-home", {}).get("samples", [])
    p95 = sorted(observed)[-1] if len(observed) == 10 else None
    if failure is None and p95 is not None and p95 > 100:
        failure = f"registered p95 budget exceeded: {p95} ms > 100 ms"
    passed = failure is None and details.get("switches") == 12 and p95 is not None and p95 <= 100
    record = {
        "schema": "airs-v2-workload-perf/1",
        "label": label,
        "status": "pass" if passed else "fail",
        "failure": failure,
        "method": "c30-cached-switch-v2",
        "method_sha256": source_hashes[METHOD],
        "registration_record_sha256": sha256_file(REGISTRATION),
        "subject": SUBJECT,
        "attempt": args.attempt,
        "command": list(sys.orig_argv),
        "conditions": conditions,
        "prepare_sha256": prepare_hash,
        "build_inputs_sha256": build_hash,
        "sources_unchanged": sources_unchanged,
        "input_digest": before,
        "inputs_unchanged": before == after,
        "source_root": ROOT,
        "source_head": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
        "driver_sha256": source_hashes[os.path.join(HERE, "measure_c30.py")],
        "probe_source_sha256": source_hashes[os.path.join(HERE, "probe.swift")],
        "probe_executable_sha256": sha256_file(os.path.join(output, "timing-probe.bin")),
        "registration_sha256": table_sha,
        "started_utc": started,
        "finished_utc": utc(),
        "executable_sha256": prepared["executable_sha256"],
        "host": fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip(),
        "metrics": metrics,
        "details": details,
    }
    path = os.path.join(output, "workload-perf.json")
    payload = json.dumps(record, indent=2) + "\n"
    with open(path, "x") as f:
        f.write(payload)
    if label == "acceptance":
        with open(canonical, "x") as f:
            f.write(payload)
    for key, metric in metrics.items():
        ordered = sorted(metric["samples"])
        if ordered:
            p95 = ordered[max(1, -(-95 * len(ordered) // 100)) - 1]
            print(f"{key}: n={len(ordered)} p95={p95} max={ordered[-1]} {metric['unit']}")
    print(os.path.relpath(path, ROOT))
    shutil.rmtree(work)
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
