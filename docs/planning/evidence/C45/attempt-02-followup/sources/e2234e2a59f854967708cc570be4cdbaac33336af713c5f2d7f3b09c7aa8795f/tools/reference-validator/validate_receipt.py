#!/usr/bin/env python3
"""Independent one-off reference validator for the C01 bootstrap receipt.

C01 builds the controller that gates every later chunk, so the controller
cannot certify its own first receipt. This validator shares no code with
crates/xtask. From the documented contracts it:

* validates the verdict against docs/architecture/review-verdict.schema.json
  with its own JSON-Schema subset interpreter, applies the D019/D026 threshold
  and requires receipt scores to equal the verdict's;
* re-derives packet, verdict, raw-output, run, log, native and predecessor
  hashes and requires the reviewer's completed final output to equal
  verdict.json;
* rebuilds the approved command plan from docs/architecture/checks.json and
  requires each run record to match it exactly (labels, argv, exit status,
  executed tests, logs, inputs);
* checks native acceptance against docs/architecture/native-checks.json, the
  fixture identity, hash-bound evidence and the prepared executable as it
  exists now;
* recomputes the current source, dependency, build-flag and evidence
  fingerprints from the repository and toolchain.

Usage:
  validate_receipt.py validate <repo> <receipt.json>
  validate_receipt.py bootstrap <repo> <attempt-dir> <xtask-binary>

`bootstrap` runs this validator and `xtask validate-receipt --gate-only` on the
real receipt and on resealed adversarial copies, requires both to accept the
real receipt and both to reject every mutation, and writes
<attempt-dir>/bootstrap/result.json.
"""

import copy
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile

DIMENSIONS = ["architecture", "behavior", "authority_security", "validation", "overall"]
EXCLUDED = ["docs/planning/status.md", "docs/planning/reviews/", "docs/planning/evidence/"]
RUN_SCHEMA = "airs-v2/run/1"


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def read(path):
    with open(path, "rb") as f:
        return f.read()


def canonical(obj):
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


# ---------- minimal JSON Schema interpreter ----------

def schema_errors(value, schema, where="$"):
    kind = schema.get("type")
    types = {"object": dict, "array": list, "string": str, "boolean": bool}
    if kind == "integer":
        if isinstance(value, bool) or not isinstance(value, int):
            return [f"{where}: expected integer"]
    elif kind in types and not isinstance(value, types[kind]):
        return [f"{where}: expected {kind}"]
    errors = []
    if "enum" in schema and value not in schema["enum"]:
        errors.append(f"{where}: not in enum")
    if isinstance(value, int) and not isinstance(value, bool):
        if "minimum" in schema and value < schema["minimum"]:
            errors.append(f"{where}: below minimum")
        if "maximum" in schema and value > schema["maximum"]:
            errors.append(f"{where}: above maximum")
    if kind == "object":
        props = schema.get("properties", {})
        errors += [f"{where}.{n}: repeated" for n in getattr(value, "dups", ())]
        errors += [f"{where}.{n}: missing" for n in schema.get("required", []) if n not in value]
        if schema.get("additionalProperties") is False:
            errors += [f"{where}.{n}: not allowed" for n in value if n not in props]
        for name, sub in props.items():
            if name in value:
                errors += schema_errors(value[name], sub, f"{where}.{name}")
    if kind == "array" and "items" in schema:
        for i, item in enumerate(value):
            errors += schema_errors(item, schema["items"], f"{where}[{i}]")
    return errors


# ---------- independent fingerprints ----------

def file_line(root, rel):
    full = os.path.join(root, rel)
    try:
        st = os.lstat(full)
    except FileNotFoundError:
        return f"{rel}\0deleted\0\n"
    if os.path.islink(full):
        return f"{rel}\0symlink\0{sha256(os.fsencode(os.readlink(full)))}\n"
    mode = "100755" if st.st_mode & 0o111 else "100644"
    return f"{rel}\0{mode}\0{sha256(read(full))}\n"


def excluded(path):
    return any(path.startswith(e) if e.endswith("/") else path == e for e in EXCLUDED)


def source_fingerprint(root):
    out = subprocess.run(
        ["/usr/bin/git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
        cwd=root, capture_output=True, check=True,
        env={"PATH": "/usr/bin:/bin", "HOME": os.environ.get("HOME", "")},
    ).stdout
    paths = sorted({p.decode() for p in out.split(b"\0") if p and not excluded(p.decode())})
    return sha256(("".join(file_line(root, p) for p in paths) + file_line(root, ".git/config")).encode())


def tree_hash(directory, skip=()):
    rels = []
    for current, dirs, files in os.walk(directory):
        keep = []
        for d in dirs:
            full = os.path.join(current, d)
            if os.path.islink(full):
                rels.append(os.path.relpath(full, directory))
            elif d not in skip:
                keep.append(d)
        dirs[:] = keep
        rels += [os.path.relpath(os.path.join(current, f), directory) for f in files]
    return sha256("".join(file_line(directory, r) for r in sorted(rels)).encode())


ACADEMY_GENERATED = ["dist", "public/excalidraw-assets", "packages/shared/tsconfig.tsbuildinfo", "stats.html"]


def academy_consumer_hash(directory):
    rels = []
    for current, dirs, files in os.walk(directory):
        keep = []
        for d in dirs:
            full = os.path.join(current, d)
            if os.path.islink(full):
                rels.append(os.path.relpath(full, directory))
            elif d != "node_modules":
                keep.append(d)
        dirs[:] = keep
        rels += [os.path.relpath(os.path.join(current, f), directory) for f in files]
    def kept(r):
        name = r.rsplit("/", 1)[-1]
        generated = any(r == g or r.startswith(g + "/") for g in ACADEMY_GENERATED)
        return not generated and name != ".env" and not name.startswith(".env.")
    return sha256("".join(file_line(directory, r) for r in sorted(filter(kept, rels))).encode())


def toolchain_hash(root):
    spec = json.loads(read(os.path.join(root, "docs/architecture/toolchain.json")))
    home = os.environ["HOME"]
    dirs = [d.replace("$HOME", home).replace("{repo}", root) for d in spec["path_dirs"]]
    env = {k: os.environ[k] for k in spec["preserve_unchanged"] if k in os.environ}
    env.update(spec["declared_env"])
    env["PATH"] = ":".join(dirs)

    def resolve(program):
        for d in dirs:
            if os.path.isfile(os.path.join(d, program)):
                return os.path.join(d, program)
        raise RuntimeError(f"{program} not found")

    lines = []
    for tool in spec["tools"]:
        out = subprocess.run([resolve(tool["argv"][0])] + tool["argv"][1:], cwd=root, env=env, capture_output=True, text=True)
        lines.append(f"{tool['name']}\0{out.stdout.strip()}\0{sha256(read(resolve(tool['argv'][0])))}\n")
    for name in ("rustc", "cargo"):
        path = subprocess.run([resolve("rustup"), "which", name], cwd=root, env=env, capture_output=True, text=True).stdout.strip()
        lines.append(f"{name}-toolchain\0\0{sha256(read(path))}\n")
    return sha256("".join(lines).encode())


def dependencies_fingerprint(root):
    parts = {}
    for f in ["Cargo.lock", "pnpm-lock.yaml", "rust-toolchain.toml", ".npmrc", "docs/architecture/inputs.lock.json"]:
        p = os.path.join(root, f)
        parts[f] = sha256(read(p)) if os.path.exists(p) else "absent"
    inputs = os.path.join(root, ".inputs")
    if os.path.isdir(inputs):
        for name in sorted(os.listdir(inputs)):
            if os.path.isdir(os.path.join(inputs, name)):
                parts[f".inputs/{name}"] = tree_hash(os.path.join(inputs, name))
    web = os.path.join(root, ".work/academy")
    parts["shared/web-scratch"] = academy_consumer_hash(web) if os.path.isdir(web) else "absent"
    installed = os.path.realpath(os.path.join(root, "apps/desktop/node_modules/@ai-security-academy/presentation"))
    for label, path in [("shared/input", os.path.join(root, ".inputs/academy/packages/presentation")),
                        ("shared/scratch", os.path.join(root, ".work/academy/packages/presentation")),
                        ("shared/installed", installed)]:
        parts[label] = tree_hash(path, skip=("node_modules",)) if os.path.isdir(path) else "absent"
    parts["toolchain"] = toolchain_hash(root)
    return sha256(canonical(parts).encode())


def build_flags_fingerprint(root, manifest, subject):
    parts = {}
    entry = manifest["chunks"][subject]
    parts[f"chunk/{subject}"] = {"fixture": entry["fixture_command"], "frontend": entry.get("frontend_commands", []),
                                 "host": entry.get("host_commands", []), "native_prepare": entry["native_prepare"]}
    parts["setup"] = manifest["setup_commands"]
    parts["phase_native"] = manifest["phase_native_prepare"]
    parts["common"] = manifest["common_commands"]
    parts["shared"] = manifest["shared_source_commands"]
    parts["artifact"] = manifest["artifact_build"]
    parts["target"] = manifest["target"]
    parts["fixture_only"] = manifest["fixture_only_features"]
    parts["production_features"] = manifest["requested_production_features"]
    for f in ["apps/desktop/src-tauri/tauri.conf.json", "apps/desktop/src-tauri/tauri.fixture.conf.json",
              "apps/desktop/src-tauri/capabilities/main.json", "apps/desktop/src-tauri/Cargo.toml", "Cargo.toml",
              "docs/architecture/native-checks.json"]:
        p = os.path.join(root, f)
        parts[f] = sha256(read(p)) if os.path.exists(p) else "absent"
    return sha256(canonical(parts).encode())


def evidence_fingerprint(root, subject):
    d = os.path.join(root, "docs/planning/evidence", subject)
    return tree_hash(d) if os.path.isdir(d) else sha256(b"")


# ---------- approved plan (chunk subjects) ----------

def runs_tests(argv):
    return "vitest" in argv or argv[:2] == ["cargo", "test"]


def plan(manifest, subject):
    sub = lambda argv: [subject if a in ("{chunk}", "{phase}") else a for a in argv]
    entry = manifest["chunks"][subject]
    check = [(f"common/{i + 1}", sub(a), runs_tests(a)) for i, a in enumerate(manifest["common_commands"])]
    if subject in manifest["shared_source_required_chunks"]:
        check += [(f"shared/{i + 1}", sub(a), runs_tests(a)) for i, a in enumerate(manifest["shared_source_commands"])]
    test = [(f"{subject}/fixture", sub(entry["fixture_command"]), True)]
    test += [(f"{subject}/host/{i + 1}", sub(a), True) for i, a in enumerate(entry.get("host_commands", []))]
    test += [(f"{subject}/frontend/{i + 1}", sub(a), True) for i, a in enumerate(entry.get("frontend_commands", []))]
    setup = [(f"setup/{i + 1}", sub(a), False) for i, a in enumerate(manifest["setup_commands"])]
    native = [("native-prepare", sub(entry["native_prepare"]), False)]
    return {"check": setup + check, "test": setup + test, "native-prepare": setup + native}


def json_objects(text):
    found, decoder = [], json.JSONDecoder()
    for i, ch in enumerate(text):
        if ch == "{":
            try:
                value, _ = decoder.raw_decode(text[i:])
                if isinstance(value, dict):
                    found.append(value)
            except ValueError:
                pass
    return found


# ---------- strict typed records (mirroring the controller's serde types) ----------

class Obj(dict):
    """A decoded JSON object that remembers keys it saw more than once."""
    dups = ()


def _pairs(pairs):
    obj, seen, dups = Obj(), set(), []
    for k, v in pairs:
        if k in seen:
            dups.append(k)
        seen.add(k)
        obj[k] = v
    obj.dups = tuple(dups)
    return obj


def _no_constant(name):
    raise ValueError(f"non-JSON number {name}")


def loads(data):
    """Strict JSON: UTF-8 only, no NaN/Infinity, duplicate keys recorded."""
    text = data.decode("utf-8") if isinstance(data, bytes) else data
    return json.loads(text, object_pairs_hook=_pairs, parse_constant=_no_constant)


def integer(lo, hi):
    return ("int", lo, hi)


U8, I32, U64 = integer(0, 2**8 - 1), integer(-2**31, 2**31 - 1), integer(0, 2**64 - 1)
USIZE, U128 = integer(0, 2**64 - 1), integer(0, 2**128 - 1)
STR, BOOL = "str", "bool"


def opt(t):
    return ("opt", t)


def seq(t):
    return ("list", t)


def rec(name):
    return ("rec", name)


def enum(*values):
    return ("enum", frozenset(values))


RUN_KIND = enum("check", "test", "native-prepare")
STATUS = enum("passed", "failed", "blocked")
HASHED = rec("hashed")

# name -> (fields, deny_unknown_fields, fields that may be absent). Option
# fields may also be absent (serde treats a missing Option as None).
RECORDS = {
    "receipt": ({"schema": STR, "receipt_id": STR, "subject": STR, "request_id": STR, "attempt_id": STR,
                 "transport": rec("transport"), "packet_sha256": STR, "verdict_sha256": STR,
                 "fingerprints": rec("fingerprints"), "fingerprints_after": rec("fingerprints"), "scores": rec("scores"),
                 "required_fixes": USIZE, "blockers": USIZE, "evidence_gaps": USIZE, "passed": BOOL,
                 "failures": seq(STR), "created_utc": STR}, True, ()),
    "transport": ({"kind": STR, "thread": opt(STR), "codex_path": STR, "codex_version": STR, "codex_sha256": STR,
                   "submitted_utc": STR, "completed_utc": STR, "exit_code": opt(I32), "timed_out": BOOL,
                   "raw_output": opt(HASHED), "model": opt(STR)}, True, ()),
    "hashed": ({"path": STR, "sha256": STR}, True, ()),
    "scores": ({d: U8 for d in DIMENSIONS}, True, ()),
    "fingerprints": ({"source": STR, "dependencies": STR, "build_flags": STR, "evidence": STR}, True, ()),
    "packet": ({"schema": STR, "request_id": STR, "attempt_id": STR, "subject": STR, "kind": STR, "created_utc": STR,
                "repository": STR, "base_commit": STR, "scope": STR, "instructions": seq(STR),
                "predecessors": seq(rec("predecessor")), "fingerprints": rec("fingerprints"), "source_inventory": HASHED,
                "runs": seq(rec("run_ref")), "evidence_files": seq(HASHED), "native_acceptance": opt(HASHED),
                "native_executable_sha256": opt(STR), "diff": HASHED, "limitations": seq(STR), "verdict_schema": HASHED,
                "verdict_path": STR, "review_instructions": STR}, True, ()),
    "predecessor": ({"subject": STR, "receipt": HASHED}, True, ()),
    "run_ref": ({"kind": RUN_KIND, "run_id": STR, "path": STR, "sha256": STR}, True, ()),
    "run": ({"schema": STR, "run_id": STR, "subject": STR, "kind": RUN_KIND, "started_utc": STR, "finished_utc": STR,
             "toolchain": seq(rec("tool")), "env": ("map", STR), "inputs_before": rec("inputs"),
             "inputs_after": rec("inputs"), "commands": seq(rec("command")), "status": STATUS, "failure": opt(STR),
             "artifacts": seq(rec("artifact"))}, True, ("artifacts",)),
    "tool": ({"name": STR, "path": STR, "version": STR, "sha256": STR}, False, ()),
    "inputs": ({"source": STR, "dependencies": STR, "build_flags": STR}, True, ()),
    "artifact": ({"name": STR, "path": STR, "sha256": STR}, True, ()),
    "command": ({"label": STR, "argv": seq(STR), "cwd": STR, "exit_code": opt(I32), "duration_ms": U128,
                 "output_sha256": STR, "log": STR, "tests": opt(rec("tests")), "status": STATUS, "note": opt(STR)},
                True, ()),
    "tests": ({"passed": U64, "failed": U64}, True, ()),
    "native": ({"schema": STR, "subject": STR, "recorded_utc": STR, "method": enum("cua", "operator"),
                "performed_by": STR, "host": STR, "app": rec("app"), "checks": seq(rec("check")),
                "limitations": seq(STR)}, True, ()),
    "app": ({"bundle": STR, "identifier": STR, "executable_sha256": STR, "fixture": BOOL}, True, ()),
    "check": ({"id": STR, "description": STR, "result": enum("pass", "fail", "blocked"), "observation": STR,
               "evidence": seq(STR)}, True, ()),
}
SCHEMAS = {"receipt": "airs-v2/review-receipt/1", "packet": "airs-v2/review-request/1", "run": RUN_SCHEMA,
           "native": "airs-v2/native-acceptance/1"}


def type_errors(value, t, where):
    if t == STR:
        if not isinstance(value, str):
            return [f"{where} is not a string"]
        if any(0xD800 <= ord(c) <= 0xDFFF for c in value):
            return [f"{where} contains a lone surrogate"]
        return []
    if t == BOOL:
        return [] if isinstance(value, bool) else [f"{where} is not a boolean"]
    kind = t[0]
    if kind == "int":
        if isinstance(value, bool) or not isinstance(value, int):
            return [f"{where} is not an integer"]
        return [] if t[1] <= value <= t[2] else [f"{where} is out of range"]
    if kind == "enum":
        return [] if isinstance(value, str) and value in t[1] else [f"{where} is not one of {sorted(t[1])}"]
    if kind == "opt":
        return [] if value is None else type_errors(value, t[1], where)
    if kind == "list":
        if not isinstance(value, list):
            return [f"{where} is not an array"]
        return [e for i, item in enumerate(value) for e in type_errors(item, t[1], f"{where}[{i}]")]
    if kind == "map":
        if not isinstance(value, dict):
            return [f"{where} is not an object"]
        return [e for k, v in value.items() for e in type_errors(v, t[1], f"{where}.{k}")]
    if kind == "rec":
        return record_errors(t[1], value, where)
    raise AssertionError(t)


def record_errors(name, value, where):
    if not isinstance(value, dict):
        return [f"{where} is not an object"]
    fields, deny, absent_ok = RECORDS[name]
    errors = [f"{where} repeats field {k}" for k in getattr(value, "dups", ()) if k in fields or deny]
    for k, t in fields.items():
        if k in value:
            errors += type_errors(value[k], t, f"{where}.{k}")
        elif k not in absent_ok and not (isinstance(t, tuple) and t[0] == "opt"):
            errors.append(f"{where} missing {k}")
    if deny:
        errors += [f"{where} unknown field {k}" for k in sorted(set(value) - set(fields))]
    return errors


def shape_errors(name, value):
    errors = record_errors(name, value, name)
    if not errors and name in SCHEMAS and value["schema"] != SCHEMAS[name]:
        errors.append(f"{name} has the wrong schema")
    return errors


def load_json(data):
    try:
        return loads(data), None
    except ValueError as e:
        return None, f"not strict JSON ({e})"


# ---------- validation ----------

def load(root, path):
    p = path if os.path.isabs(path) else os.path.join(root, path)
    return read(p) if os.path.exists(p) else None


def validate(root, receipt_path, current=None):
    reasons = []
    attempt = os.path.dirname(receipt_path)
    receipt_b, packet_b = read(receipt_path), read(os.path.join(attempt, "request.json"))
    (receipt, receipt_err), (packet, packet_err) = load_json(receipt_b), load_json(packet_b)
    if receipt_err or packet_err:
        return [f"receipt {receipt_err}" if receipt_err else f"packet {packet_err}"]
    manifest = json.loads(read(os.path.join(root, "docs/architecture/checks.json")))
    shape = shape_errors("receipt", receipt) + shape_errors("packet", packet)
    if shape:
        return reasons + [f"record shape: {e}" for e in shape]
    subject = packet["subject"]
    if os.path.exists(os.path.join(attempt, "expired.json")):
        reasons.append("attempt expired")
    if sha256(packet_b) != receipt["packet_sha256"]:
        reasons.append("packet hash mismatch")
    for key in ("request_id", "attempt_id", "subject"):
        if receipt[key] != packet[key]:
            reasons.append(f"receipt {key} mismatch")
    verdict_b = load(root, packet["verdict_path"])
    if verdict_b is None:
        return reasons + ["no verdict"]
    if sha256(verdict_b) != receipt["verdict_sha256"]:
        reasons.append("verdict hash mismatch")
    verdict, verdict_err = load_json(verdict_b)
    if verdict_err:
        return reasons + [f"verdict {verdict_err}"]
    schema_path = os.path.join(root, "docs/architecture/review-verdict.schema.json")
    if sha256(read(schema_path)) != packet["verdict_schema"]["sha256"]:
        reasons.append("verdict schema changed since packet")
    errors = schema_errors(verdict, json.loads(read(schema_path)))
    reasons += [f"schema: {e}" for e in errors]
    if not errors:
        for key in ("request_id", "attempt_id", "subject"):
            if verdict[key] != packet[key]:
                reasons.append(f"verdict {key} mismatch")
        if verdict["fingerprints"] != packet["fingerprints"]:
            reasons.append("verdict fingerprints differ from packet")
        if verdict["scores"] != receipt["scores"]:
            reasons.append("receipt scores differ from verdict")
        reasons += [f"{d} score {verdict['scores'][d]} < 9" for d in DIMENSIONS if verdict["scores"][d] < 9]
        reasons += [f"{k} not empty" for k in ("required_fixes", "blockers", "evidence_gaps") if verdict[k]]
        if verdict["recommendation"] != "pass":
            reasons.append("recommendation fail")
    if receipt["fingerprints"] != packet["fingerprints"]:
        reasons.append("receipt fingerprints differ from packet")
    if receipt["fingerprints_after"] != packet["fingerprints"]:
        reasons.append("inputs changed during review")
    current = current or {
        "source": source_fingerprint(root), "dependencies": dependencies_fingerprint(root),
        "build_flags": build_flags_fingerprint(root, manifest, subject), "evidence": evidence_fingerprint(root, subject)}
    for key in ("source", "dependencies", "build_flags", "evidence"):
        if current[key] != packet["fingerprints"][key]:
            reasons.append(f"current {key} differs from packet")

    # transport and final output
    t = receipt["transport"]
    raw_ref = t.get("raw_output")
    raw = load(root, raw_ref["path"]) if raw_ref else None
    if raw is None:
        reasons.append("raw reviewer output missing")
    else:
        if sha256(raw) != raw_ref["sha256"]:
            reasons.append("raw reviewer output hash mismatch")
        if t.get("exit_code") != 0 or t.get("timed_out"):
            reasons.append("transport did not complete successfully")
        finals = []
        for line in raw.decode("utf-8", "replace").splitlines():
            try:
                event = json.loads(line)
            except ValueError:
                continue
            if t["kind"] == "codex-thread":
                if event.get("payload", {}).get("type") == "task_complete" and event.get("timestamp", "") >= t["submitted_utc"]:
                    finals.append(event["payload"].get("last_agent_message") or "")
            elif t["kind"] == "codex-exec-fresh":
                item = event.get("item", {})
                if item.get("type") in ("mcp_tool_call", "web_search", "file_change"):
                    reasons.append("forbidden reviewer tool use")
                if event.get("type") in ("turn.failed", "error"):
                    reasons.append("fresh reviewer turn failed")
        if t["kind"] == "codex-exec-fresh":
            events = [json.loads(l) for l in raw.decode().splitlines() if l.strip().startswith("{")]
            if not any(e.get("type") == "turn.completed" for e in events):
                reasons.append("fresh reviewer turn never completed")
            msgs = [e["item"]["text"] for e in events if e.get("type") == "item.completed" and e.get("item", {}).get("type") == "agent_message"]
            finals += msgs[-1:]
        if not finals:
            reasons.append("no completed reviewer turn")
        elif not any(obj == verdict for f in finals for obj in json_objects(f)):
            reasons.append("final reviewer output differs from verdict.json")

    # predecessors
    for pred in packet["predecessors"]:
        b = load(root, pred["receipt"]["path"])
        if b is None or sha256(b) != pred["receipt"]["sha256"]:
            reasons.append(f"predecessor {pred['subject']} receipt missing or changed")

    # runs against the approved plan
    expected = plan(manifest, subject)
    for kind in expected:
        if sum(1 for r in packet["runs"] if r["kind"] == kind) != 1:
            reasons.append(f"expected exactly one {kind} run")
    for ref in packet["runs"]:
        b = load(root, ref["path"])
        if b is None or sha256(b) != ref["sha256"]:
            reasons.append(f"run {ref['run_id']} missing or edited")
            continue
        rec, rec_err = load_json(b)
        shape = [rec_err] if rec_err else shape_errors("run", rec)
        if shape:
            reasons += [f"run {ref['run_id']} shape: {e}" for e in shape]
            continue
        if (rec.get("schema") != RUN_SCHEMA or rec["subject"] != subject or rec["kind"] != ref["kind"] or rec["run_id"] != ref["run_id"]
                or ref["path"] != f"docs/planning/evidence/{subject}/runs/{rec['run_id']}/run.json"):
            reasons.append(f"run {ref['run_id']} identity mismatch")
        if rec["status"] != "passed" or rec.get("failure"):
            reasons.append(f"run {ref['run_id']} not passed")
        want = expected.get(rec["kind"], [])
        if len(rec["commands"]) != len(want):
            reasons.append(f"run {ref['run_id']} command count {len(rec['commands'])} != {len(want)}")
        for cmd, (label, argv, tests) in zip(rec["commands"], want):
            if cmd["label"] != label or cmd["argv"] != argv:
                reasons.append(f"run {ref['run_id']} command {label} not approved")
            if cmd["status"] != "passed" or cmd["exit_code"] != 0:
                reasons.append(f"run {ref['run_id']} command {label} failed")
            if tests and not (cmd.get("tests") and cmd["tests"]["passed"] > 0 and cmd["tests"]["failed"] == 0):
                reasons.append(f"run {ref['run_id']} command {label} ran no passing tests")
            log = load(root, os.path.join(os.path.dirname(ref["path"]), cmd["log"]))
            if log is None or sha256(log) != cmd["output_sha256"]:
                reasons.append(f"run {ref['run_id']} log {label} missing or edited")
        fp = packet["fingerprints"]
        for state in (rec["inputs_before"], rec["inputs_after"]):
            if (state["source"], state["dependencies"], state["build_flags"]) != (fp["source"], fp["dependencies"], fp["build_flags"]):
                reasons.append(f"run {ref['run_id']} used other inputs")
                break
        if rec["kind"] == "native-prepare":
            art = {a["name"]: a["sha256"] for a in rec.get("artifacts", [])}
            if art.get("fixture-executable") != packet.get("native_executable_sha256"):
                reasons.append("native-prepare run does not record the packet executable")
            prep = load(root, f"docs/planning/evidence/{subject}/native-prepare.json")
            if prep is None or art.get("fixture-bundle") != json.loads(prep)["bundle_sha256"]:
                reasons.append("native-prepare run does not record the prepared bundle")

    # native acceptance
    nref = packet.get("native_acceptance")
    if not nref:
        reasons.append("no native acceptance")
    else:
        b = load(root, nref["path"])
        if b is None or sha256(b) != nref["sha256"]:
            reasons.append("native record missing or edited")
        else:
            rec, rec_err = load_json(b)
            shape = [rec_err] if rec_err else shape_errors("native", rec)
            if shape:
                return reasons + [f"native shape: {e}" for e in shape]
            inventory = json.loads(read(os.path.join(root, "docs/architecture/native-checks.json")))
            required = [c["id"] for c in inventory["chunks"].get(subject, [])]
            ids = [c["id"] for c in rec["checks"]]
            bound = {e["path"] for e in packet["evidence_files"]}
            if not required:
                reasons.append("no native inventory")
            if sorted(ids) != sorted(set(ids)) or set(ids) != set(required):
                reasons.append("native checks do not match the required inventory")
            for c in rec["checks"]:
                if c["result"] != "pass" or not c["observation"].strip() or not c["evidence"] or not set(c["evidence"]) <= bound:
                    reasons.append(f"native check {c['id']} incomplete")
            if rec["subject"] != subject or not rec["app"]["fixture"] or rec["app"]["identifier"] != inventory["fixture_identifier"]:
                reasons.append("native record identity mismatch")
            if rec["app"]["executable_sha256"] != packet.get("native_executable_sha256"):
                reasons.append("native record names other bytes")
            prepared = load(root, f"docs/planning/evidence/{subject}/native-prepare.json")
            exe = None
            if prepared:
                prep = json.loads(prepared)
                bundle = prep["bundle"]
                exe = load(root, os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop"))
                bundle_path = os.path.join(root, bundle)
                if rec["app"]["bundle"] != bundle:
                    reasons.append("native record names another bundle")
                if not os.path.isdir(bundle_path) or tree_hash(bundle_path) != prep["bundle_sha256"]:
                    reasons.append("prepared bundle changed or missing")
                ident = subprocess.run(["/usr/bin/plutil", "-extract", "CFBundleIdentifier", "raw", "-o", "-",
                                        os.path.join(bundle_path, "Contents/Info.plist")], capture_output=True, text=True).stdout.strip()
                if ident != inventory["fixture_identifier"] or prep["identifier"] != ident:
                    reasons.append("prepared bundle identifier is not the fixture identity")
            else:
                reasons.append("native-prepare record missing")
            if exe is None or sha256(exe) != packet.get("native_executable_sha256"):
                reasons.append("prepared executable changed or missing")
    return reasons


# ---------- bootstrap: agreement with the controller ----------

def controller_accepts(root, xtask, receipt_path):
    env = {k: os.environ[k] for k in ("HOME", "USER", "LOGNAME", "TMPDIR", "LANG") if k in os.environ}
    env["PATH"] = "/usr/bin:/bin"
    out = subprocess.run([xtask, "validate-receipt", "--receipt", receipt_path, "--chunk", "C01", "--gate-only"],
                         cwd=root, env=env, capture_output=True, text=True)
    return out.returncode == 0, (out.stdout + out.stderr).strip()


def make_copy(root, attempt, tmp, name, mutate):
    d = os.path.join(tmp, name)
    os.makedirs(d)
    for f in ("request.json", "verdict.json", "receipt.json", "thread-reply.jsonl", "stdout.jsonl"):
        if os.path.exists(os.path.join(attempt, f)):
            shutil.copy(os.path.join(attempt, f), d)
    packet = json.loads(read(os.path.join(d, "request.json")))
    receipt = json.loads(read(os.path.join(d, "receipt.json")))
    verdict = json.loads(read(os.path.join(d, "verdict.json")))
    packet["verdict_path"] = os.path.join(d, "verdict.json")
    raw_name = os.path.basename(receipt["transport"]["raw_output"]["path"])
    receipt["transport"]["raw_output"]["path"] = os.path.join(d, raw_name)
    raw = read(os.path.join(d, raw_name))
    state = {"packet": packet, "receipt": receipt, "verdict": verdict, "raw": raw, "dir": d, "root": root}
    mutate(state)
    vb = json.dumps(state["verdict"], indent=2).encode()
    with open(os.path.join(d, "verdict.json"), "wb") as f:
        f.write(vb)
    with open(os.path.join(d, raw_name), "wb") as f:
        f.write(state["raw"])
    pb = json.dumps(state["packet"], indent=2).encode()
    with open(os.path.join(d, "request.json"), "wb") as f:
        f.write(pb)
    r = state["receipt"]
    r["verdict_sha256"], r["packet_sha256"] = sha256(vb), sha256(pb)
    r["transport"]["raw_output"]["sha256"] = sha256(state["raw"])
    text = json.dumps(r, indent=2)
    if "receipt_text" in state:
        text = state["receipt_text"](text)
    with open(os.path.join(d, "receipt.json"), "w") as f:
        f.write(text)
    return os.path.join(d, "receipt.json")


def final_reply(state):
    """Rewrites the thread completion so it carries the (mutated) verdict."""
    lines = []
    for line in state["raw"].decode().splitlines():
        event = json.loads(line)
        if event.get("payload", {}).get("type") == "task_complete":
            event["payload"]["last_agent_message"] = json.dumps(state["verdict"], indent=2)
        lines.append(json.dumps(event))
    state["raw"] = ("\n".join(lines) + "\n").encode()


def verdict_change(fn):
    def apply(state):
        fn(state["verdict"])
        state["receipt"]["scores"] = state["verdict"]["scores"] if isinstance(state["verdict"].get("scores"), dict) else state["receipt"]["scores"]
        final_reply(state)
    return apply


def set_fp(key):
    def apply(state):
        for holder in (state["packet"], state["verdict"]):
            holder["fingerprints"][key] = "0" * 64
        state["receipt"]["fingerprints"][key] = "0" * 64
        state["receipt"]["fingerprints_after"][key] = "0" * 64
        final_reply(state)
    return apply


def native_copy(edit):
    def apply(state):
        ref = state["packet"]["native_acceptance"]
        rec = json.loads(read(os.path.join(state["root"], ref["path"])))
        edit(rec)
        path = os.path.join(state["dir"], "native-acceptance.json")
        b = json.dumps(rec, indent=2).encode()
        with open(path, "wb") as f:
            f.write(b)
        state["packet"]["native_acceptance"] = {"path": path, "sha256": sha256(b)}
    return apply


def run_copy(kind, edit):
    """Copies the packet's `kind` run record, edits it and reseals its packet reference."""
    def apply(state):
        ref = next(r for r in state["packet"]["runs"] if r["kind"] == kind)
        rec = json.loads(read(os.path.join(state["root"], ref["path"])))
        edit(rec)
        path = os.path.join(state["dir"], f"{kind}-run.json")
        b = json.dumps(rec, indent=2).encode()
        with open(path, "wb") as f:
            f.write(b)
        ref["path"], ref["sha256"] = path, sha256(b)
    return apply


def receipt_float_score(state):
    scores = state["receipt"]["scores"]
    scores["overall"] = float(scores["overall"])


def receipt_duplicate_passed(state):
    state["receipt_text"] = lambda text: text.replace('"passed": ', '"passed": false,\n  "passed": ', 1)


def first_tested(rec):
    return next(c for c in rec["commands"] if c.get("tests"))


# Typed-record cases: each value keeps Python truthiness/equality with the
# real one, so only boundary type checking rejects it (as serde does).
TYPED_MUTATIONS = [
    ("transport timed_out integer 0", lambda s: s["receipt"]["transport"].__setitem__("timed_out", 0)),
    ("transport exit_code float 0.0", lambda s: s["receipt"]["transport"].__setitem__("exit_code", 0.0)),
    ("receipt passed string", lambda s: s["receipt"].__setitem__("passed", "yes")),
    ("receipt required_fixes string", lambda s: s["receipt"].__setitem__("required_fixes", "0")),
    ("receipt score float", receipt_float_score),
    ("receipt duplicate field", receipt_duplicate_passed),
    ("packet run kind outside enum", lambda s: s["packet"]["runs"][0].__setitem__("kind", "Check")),
    ("packet executable hash integer", lambda s: s["packet"].__setitem__("native_executable_sha256", 1)),
    ("packet nested unknown field", lambda s: s["packet"]["diff"].__setitem__("approved", True)),
    ("native method invented", native_copy(lambda r: r.__setitem__("method", "invented"))),
    ("native app fixture integer 1", native_copy(lambda r: r["app"].__setitem__("fixture", 1))),
    ("native check result wrong case", native_copy(lambda r: r["checks"][0].__setitem__("result", "PASS"))),
    ("run status wrong case", run_copy("check", lambda r: r.__setitem__("status", "Passed"))),
    ("run command exit_code string", run_copy("check", lambda r: r["commands"][0].__setitem__("exit_code", "0"))),
    ("run tests passed boolean", run_copy("test", lambda r: first_tested(r)["tests"].__setitem__("passed", True))),
    ("run env value integer", run_copy("check", lambda r: r["env"].__setitem__("CI", 1))),
]


def drop_optional_model(state):
    transport = state["receipt"]["transport"]
    transport.pop("model", None)
    state["receipt"]["transport"] = dict(reversed(list(transport.items())))


# Resealed equivalents both validators must accept: serde treats a missing
# Option as None and does not depend on key order.
ACCEPTANCE_CONTROLS = [
    ("unchanged", lambda s: None),
    ("transport model omitted and keys reordered", drop_optional_model),
]

MUTATIONS = (
    [(f"score {d}=8", verdict_change(lambda v, d=d: v["scores"].__setitem__(d, 8))) for d in DIMENSIONS] + [
        ("required fix with nines", verdict_change(lambda v: v["required_fixes"].append("fix"))),
        ("blocker", verdict_change(lambda v: v["blockers"].append("blocker"))),
        ("evidence gap", verdict_change(lambda v: v["evidence_gaps"].append("gap"))),
        ("recommendation fail", verdict_change(lambda v: v.__setitem__("recommendation", "fail"))),
        ("extra verdict field", verdict_change(lambda v: v.__setitem__("approved", True))),
        ("score 11", verdict_change(lambda v: v["scores"].__setitem__("overall", 11))),
        ("request id mismatch", verdict_change(lambda v: v.__setitem__("request_id", "rq-other"))),
        ("receipt score mismatch", lambda s: s["receipt"]["scores"].__setitem__("overall", 10)),
        ("current source differs", set_fp("source")),
        ("current dependencies differ", set_fp("dependencies")),
        ("current build flags differ", set_fp("build_flags")),
        ("current evidence differs", set_fp("evidence")),
        ("final output disagrees", lambda s: (s["verdict"]["optional_suggestions"].append("tamper"))),
        ("transport exit nonzero", lambda s: s["receipt"]["transport"].__setitem__("exit_code", 1)),
        ("transport timed out", lambda s: s["receipt"]["transport"].__setitem__("timed_out", True)),
        ("no completed turn", lambda s: s.__setitem__("raw", b'{"payload":{"type":"agent_message","message":"progress"}}\n')),
        ("inputs changed during review", lambda s: s["receipt"]["fingerprints_after"].__setitem__("source", "1" * 64)),
        ("test run missing", lambda s: s["packet"].__setitem__("runs", [r for r in s["packet"]["runs"] if r["kind"] != "test"])),
        ("predecessor hash changed", lambda s: s["packet"]["predecessors"][0]["receipt"].__setitem__("sha256", "0" * 64)),
        ("native check removed", native_copy(lambda r: r["checks"].pop())),
        ("native duplicate check", native_copy(lambda r: r["checks"].append(copy.deepcopy(r["checks"][0])))),
        ("native wrong identity", native_copy(lambda r: r["app"].__setitem__("identifier", "io.cdot.prisma-airs-desktop.v2"))),
        ("native blank observation", native_copy(lambda r: r["checks"][0].__setitem__("observation", " "))),
        ("native other bundle", native_copy(lambda r: r["app"].__setitem__("bundle", "target/other.app"))),
        ("native unknown field", native_copy(lambda r: r.__setitem__("approved", True))),
        ("receipt schema changed", lambda s: s["receipt"].__setitem__("schema", "airs-v2/review-receipt/0")),
        ("receipt unknown field", lambda s: s["receipt"].__setitem__("approved", True)),
        ("transport unknown field", lambda s: s["receipt"]["transport"].__setitem__("approved", True)),
        ("packet schema changed", lambda s: s["packet"].__setitem__("schema", "airs-v2/review-request/0")),
        ("packet unknown field", lambda s: s["packet"].__setitem__("approved", True)),
        ("packet missing field", lambda s: s["packet"].pop("diff")),
    ] + TYPED_MUTATIONS)


def bootstrap(root, attempt, xtask):
    receipt_path = os.path.join(attempt, "receipt.json")
    real_ref = validate(root, receipt_path)
    real_ctl, ctl_out = controller_accepts(root, xtask, receipt_path)
    current = {"source": source_fingerprint(root), "dependencies": dependencies_fingerprint(root),
               "build_flags": build_flags_fingerprint(root, json.loads(read(os.path.join(root, "docs/architecture/checks.json"))), "C01"),
               "evidence": evidence_fingerprint(root, "C01")}
    cases = []
    with tempfile.TemporaryDirectory() as tmp:
        for i, (name, mutate) in enumerate(MUTATIONS):
            copy_receipt = make_copy(root, attempt, tmp, f"case-{i}", mutate)
            ref = validate(root, copy_receipt, current)
            ctl, out = controller_accepts(root, xtask, copy_receipt)
            cases.append({"case": name, "reference_rejected": bool(ref), "reference_reasons": ref,
                          "controller_rejected": not ctl, "controller_output": out})
        controls = []
        for i, (name, mutate) in enumerate(ACCEPTANCE_CONTROLS):
            control = make_copy(root, attempt, tmp, f"control-{i}", mutate)
            control_ref = validate(root, control, current)
            control_ctl, control_out = controller_accepts(root, xtask, control)
            controls.append({"control": name, "reference_accepted": not control_ref, "reference_reasons": control_ref,
                             "controller_accepted": control_ctl, "controller_output": control_out})
    agreement = all(c["reference_rejected"] and c["controller_rejected"] for c in cases)
    controls_ok = all(c["reference_accepted"] and c["controller_accepted"] for c in controls)
    result = {
        "schema": "airs-v2/bootstrap/1",
        "receipt_sha256": sha256(read(receipt_path)),
        "reference_accepted": not real_ref,
        "reference_reasons": real_ref,
        "controller_accepted": real_ctl,
        "controller_output": ctl_out,
        "resealed_controls_accepted_by_both": controls_ok,
        "adversarial_cases": len(cases),
        "typed_cases": len(TYPED_MUTATIONS),
        "adversarial_agreement": agreement and controls_ok,
        "controls": controls,
        "cases": cases,
        "validator_sha256": sha256(read(os.path.abspath(__file__))),
    }
    os.makedirs(os.path.join(attempt, "bootstrap"), exist_ok=True)
    with open(os.path.join(attempt, "bootstrap", "result.json"), "w") as f:
        json.dump(result, f, indent=2)
    return result


def main():
    if len(sys.argv) == 4 and sys.argv[1] == "validate":
        reasons = validate(os.path.abspath(sys.argv[2]), os.path.abspath(sys.argv[3]))
        print(json.dumps({"accepted": not reasons, "reasons": reasons}, indent=2))
        return 0 if not reasons else 1
    if len(sys.argv) == 5 and sys.argv[1] == "bootstrap":
        r = bootstrap(os.path.abspath(sys.argv[2]), os.path.abspath(sys.argv[3]), os.path.abspath(sys.argv[4]))
        print(json.dumps({k: v for k, v in r.items() if k != "cases"}, indent=2))
        ok = r["reference_accepted"] and r["controller_accepted"] and r["adversarial_agreement"]
        return 0 if ok else 1
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main())
