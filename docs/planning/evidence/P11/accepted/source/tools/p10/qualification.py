"""Explicit C45/P10 evidence accounting. Does not execute checks or assign scores.

The existing runners produce raw records; this consumer rejects omitted coverage,
changed evidence, unsupported retirement and affected checks carried as unchanged.
Historical results always retain their original candidate and execution bindings.
"""
from pathlib import Path
import hashlib
import json


def require(value, message):
    if not value:
        raise ValueError(message)


def read(path):
    return json.loads(Path(path).read_text())


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def digest(value):
    return hashlib.sha256((json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()).hexdigest()


def bound(root, reference):
    path = root / reference["path"]
    require(path.resolve().is_relative_to(root.resolve()), "evidence escapes checkout")
    require(sha(path) == reference["sha256"], "evidence hash drift: " + str(path))
    return path


EXACT_FIELDS = ("executable_sha256", "bundle_sha256", "build_flags", "conditions")


def verify_raw(root, needed, execution, origin):
    require(execution["argv"] == needed["argv"], "execution command differs from approved plan")
    require(execution["cwd"] == origin["execution_root"], "execution cwd differs from original candidate")
    raw_path = bound(root, execution["raw"])
    raw = read(raw_path)
    if needed["kind"] == "native":
        require(raw["schema"] == "airs-v2/native-acceptance/1" and raw["app"]["executable_sha256"] == origin["executable_sha256"], "raw native candidate mismatch")
        checks = [check for check in raw["checks"] if check["id"] == needed["id"]]
        require(len(checks) == 1 and checks[0]["result"] == "pass" and checks[0].get("observation") and checks[0].get("evidence"), "raw native assertion not passing")
        for name in checks[0]["evidence"]:
            require(name in execution["evidence"], "native evidence missing from execution inventory")
            bound(root, {"path":name,"sha256":execution["evidence"][name]})
    elif needed["kind"] == "command":
        require(raw["schema"] == "airs-v2/run/1" and raw["status"] == "passed" and raw["failure"] is None and raw["inputs_before"] == raw["inputs_after"], "raw runner failed or inputs changed")
        require(raw["inputs_before"] == origin["runner_inputs"], "runner input fingerprint does not bind original candidate")
        commands = [command for command in raw["commands"] if command["label"] == needed["label"]]
        require(len(commands) == 1, "raw runner command missing/duplicated")
        command = commands[0]
        require(command["status"] == "passed" and command["exit_code"] == 0 and command["argv"] == needed["argv"] and command["cwd"] in [".", origin["execution_root"]] and not command["note"], "raw runner command failed/differs")
        require(command["output_sha256"] == execution["log"]["sha256"] and str((raw_path.parent/command["log"]).relative_to(root)) == execution["log"]["path"], "raw runner log binding")
        if needed.get("expects_tests"):
            require(command["tests"] and command["tests"]["passed"] > 0 and command["tests"]["failed"] == 0, "test command did not execute passing tests")
    else:
        raise ValueError("unsupported required raw execution kind")


def verify(root, record, historical=None):
    require(record["schema"] == "airs-desktop/qualified-coverage/1", "coverage schema")
    plan = read(bound(root, record["plan"]))
    require(plan["schema"] == "airs-desktop/required-coverage/1", "plan schema")
    requirements = plan["requirements"]
    ids = [item["id"] for item in requirements]
    require(len(ids) == len(set(ids)) and ids, "duplicate/empty required coverage")
    rows = record["coverage"]
    require(len(rows) == len(ids) and {r["id"] for r in rows} == set(ids), "missing/duplicate/unknown coverage")
    by_id = {item["id"]: item for item in requirements}
    candidate = read(bound(root, record["candidate"]))
    require(all(candidate.get(k) for k in EXACT_FIELDS) and candidate.get("helper_closure"), "candidate binding incomplete")
    require(candidate.get("source_inventory_sha256") == record["source_inventory_sha256"], "candidate source binding")
    for row in rows:
        needed = by_id[row["id"]]
        mode = row["disposition"]
        require(mode in ["executed", "reused_exact", "carried_unaffected", "not_applicable"], "unsupported coverage disposition")
        if mode == "not_applicable":
            decision = needed.get("retirement")
            require(decision and row.get("reason") and row.get("decision") == decision, "unapproved assertion retirement")
            bound(root, decision)
            continue
        original = read(bound(root, row["result"]))
        require(original.get("status") == "pass" and original.get("exit_code") == 0, "result is not an actual passing execution")
        require(original.get("execution_id") and original.get("argv") and original.get("cwd") and original.get("started_utc") and original.get("finished_utc"), "execution provenance missing")
        require(row["id"] in original.get("required_ids", []), "result does not cover required assertion")
        bound(root, original["log"])
        origin = read(bound(root, original["candidate"]))
        require(all(origin.get(k) for k in EXACT_FIELDS) and origin.get("helper_closure"), "original candidate binding incomplete")
        verify_raw(root, needed, original, origin)
        if mode in ["executed", "reused_exact"]:
            if mode == "reused_exact":
                require(not needed.get("must_execute", needed.get("affected", True)), "changed check must execute")
            helpers = needed.get("helper_paths")
            if helpers is None:
                require(origin["helper_closure"] == candidate["helper_closure"], "relevant helper closure differs")
            else:
                require(all(name in origin["helper_closure"] and name in candidate["helper_closure"] and origin["helper_closure"][name] == candidate["helper_closure"][name] for name in helpers), "relevant helper closure differs")
            require(all(origin[k] == candidate[k] for k in EXACT_FIELDS), "artifact/helper/flags/conditions differ")
            if mode == "executed":
                require(origin["source_inventory_sha256"] == record["source_inventory_sha256"], "execution is historical, not current")
        else:
            require(not needed.get("affected", True), "affected check cannot be carried unchanged")
            rationale = row.get("impact")
            require(rationale and rationale.get("reason") and rationale.get("dependencies") == needed.get("dependencies") and rationale.get("original_candidate_sha256") == original["candidate"]["sha256"], "unchanged-impact proof missing")
            proof = read(bound(root, rationale["proof"]))
            bindings = proof.get("file_bindings", {})
            require(set(bindings) == set(needed["dependencies"]) and bindings, "dependency membership proof missing")
            current_files=historical.source(record['source_inventory_sha256']) if historical else root
            original_files=historical.source(origin['source_inventory_sha256']) if historical else root
            for dependency, binding in bindings.items():
                require(binding["original_sha256"] == binding["current_sha256"] == sha(current_files/dependency), "dependency changed: " + dependency)
                original_source = original_files/dependency if historical else bound(root, binding["original_file"])
                require(sha(original_source) == binding["original_sha256"], "original dependency proof mismatch")
            require(proof.get("unchanged_dependencies") == needed["dependencies"] and proof.get("current_source_inventory_sha256") == record["source_inventory_sha256"] and proof.get("original_candidate_sha256") == original["candidate"]["sha256"], "impact proof does not bind current and original inputs")
    return {"qualified": True, "required": len(ids), "dispositions": {m: sum(r["disposition"] == m for r in rows) for m in sorted({r["disposition"] for r in rows})}, "candidate_sha256": record["candidate"]["sha256"], "source_inventory_sha256": record["source_inventory_sha256"]}
