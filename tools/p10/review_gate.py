"""Read-only actual-review proof checks for C45/P10 and production prerequisites.

No score assignment, evidence execution, controller completion or release action.
"""
from pathlib import Path
import json
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "p09"))
from phase_packet import file_identity, digest, validate
from phase_inputs import full_gate, require, read, sha, REVIEW_DISABLED


def transport(root, out, request):
    invocation = read(out/"invocation.json")
    require(invocation["request_sha256"] == sha(out/"request.json"), "transport request drift")
    schema_path = root/request.get("schema_archive", "docs/architecture/review-verdict.schema.json")
    require(invocation["schema_sha256"] == sha(schema_path), "transport schema drift")
    args = invocation["command_without_prompt"]
    require(args[0] == "codex" and args.count("exec") == 1, "not fresh Codex exec")
    for flag in ["--no-daemon", "--ephemeral", "--ignore-user-config", "--ignore-rules", "--json"]:
        require(args.count(flag) == 1, "transport isolation " + flag)
    for flag, value in [("-a", "never"), ("-s", "read-only"), ("-m", "gpt-6.1-sol")]:
        require(args.count(flag) == 1 and args[args.index(flag)+1] == value, "transport isolation argument " + flag)
    require(args.count("--output-schema") == 1 and args[args.index("--output-schema")+1] == "docs/architecture/review-verdict.schema.json", "transport output schema")
    expected_out = Path(request.get("execution_root", root))/"docs/planning/reviews"/request["subject"]/request["attempt_id"]/"verdict-returned.json"
    require(args.count("-o") == 1 and args[args.index("-o")+1] == str(expected_out), "transport output path")
    require(invocation["model_requested"] == "gpt-6.1-sol", "configured review model changed")
    require(not any("dangerously" in arg or arg in ["resume", "fork", "--last", "--approve-for-me"] for arg in args), "review resume/bypass")
    disabled = [args[i+1] for i,a in enumerate(args) if a == "--disable"]
    require(set(disabled) == REVIEW_DISABLED and len(disabled) == len(REVIEW_DISABLED), "restricted review features")
    overrides = [args[i+1] for i,a in enumerate(args) if a == "-c"]
    require(set(overrides) == {'web_search="disabled"', 'model_reasoning_effort="xhigh"'} and len(overrides) == 2, "review overrides")
    require(invocation["feature_probe_sha256"] == sha(out/"feature-probe.log"), "feature probe drift")
    observed = {line.split()[0]:line.split()[-1] for line in (out/"feature-probe.log").read_text().splitlines() if len(line.split()) >= 3}
    require(all(observed.get(k) == invocation["restricted_features_observed"].get(k) == "false" for k in REVIEW_DISABLED), "feature isolation not observed")
    messages=[];sessions=[]
    for line in (out/"thread-reply.log").read_text().splitlines():
        try:event=json.loads(line)
        except json.JSONDecodeError:continue
        if not isinstance(event,dict):continue
        item=event.get("item",{})
        if not isinstance(item,dict):item={}
        require(event.get("type") not in ["hook.started","hook.completed"] and item.get("type") not in ["mcp_tool_call","hook_execution"], "unexpected review side effect")
        if event.get("type") == "thread.started":sessions.append(event.get("thread_id"))
        if event.get("type") == "item.completed" and item.get("type") in ["agent_message","assistant_message"]:messages.append(item.get("text",""))
    require(len(sessions) == 1 and messages and json.loads(messages[-1]) == read(out/"verdict-returned.json"), "actual independent returned verdict missing/mismatched")
    return dict(invocation_sha256=sha(out/"invocation.json"),thread_reply_sha256=sha(out/"thread-reply.log"),feature_probe_sha256=sha(out/"feature-probe.log"),reviewer_sessions_observed=sessions)


def actual_review(root, subject, attempt, source_root=None, evidence=True):
    out=root/"docs/planning/reviews"/subject/attempt
    request=read(out/"request.json"); verdict=read(out/"verdict-returned.json")
    schema_path=(root/request["schema_archive"] if request.get("schema_archive")
                 else (source_root or root)/"docs/architecture/review-verdict.schema.json")
    # Historical P09 schema is preserved independently of future descriptions.
    if subject == "P09":schema_path=root/"docs/planning/evidence/C45/activation-20261102-g11/p09-source/docs/architecture/review-verdict.schema.json"
    validate(verdict,read(schema_path))
    require(verdict["subject"] == subject and request["subject"] == subject and verdict["attempt_id"] == request["attempt_id"] == attempt and verdict["request_id"] == request["request_id"] and verdict["fingerprints"] == request["fingerprints"], "actual verdict request/fingerprint binding")
    full_gate(verdict)
    # Preserve request bytes: the archive path is verification context, never an edit.
    transport_request=dict(request)
    # A later schema description must not change an archived review's input.
    # Supply verification context only; request/invocation bytes stay immutable.
    if source_root is not None:
        transport_request["schema_archive"]=str(schema_path.relative_to(root))
    if subject == "P09":
        transport_request["schema_archive"]=str(schema_path.relative_to(root))
        transport_request["execution_root"]=read(root/"docs/planning/evidence/C45/activation-20261102-g11/p09-proof.json")["source_checkout"]
    observed=transport(root,out,transport_request)
    audit=read(out/"post-review-audit.json");receipt=read(out/"receipt.json")
    require(audit["full_gate_pass"] is True and receipt["full_gate_pass"] is True and receipt["source_frozen"] is True, "actual full-gate audit missing")
    for record in [audit,receipt]:
        require(record["request_sha256"] == sha(out/"request.json") and record.get("raw_verdict_sha256",record.get("verdict_sha256")) == sha(out/"verdict-returned.json") and record["scores"] == verdict["scores"], "review audit/receipt binding")
    require(receipt["post_review_audit_sha256"] == sha(out/"post-review-audit.json") and receipt["fingerprints"] == verdict["fingerprints"], "receipt audit/fingerprints")
    require(receipt["invocation_sha256"] == observed["invocation_sha256"] and receipt["thread_reply_sha256"] == observed["thread_reply_sha256"], "receipt transport binding")
    for name,key,base in [("source-inventory.json","source",source_root or root),("evidence-inventory.json","evidence",root)]:
        inventory=read(out/name)["files"]
        require(digest(inventory) == request["fingerprints"][key], "inventory fingerprint mismatch")
        if evidence or key == "source":
            locations = read(root/"docs/planning/evidence/C45/activation-20261102-g11/p09-extra-evidence/locations.json") if subject == "P09" and key == "evidence" else {}
            for relative,identity in inventory.items():
                path = root/locations[relative] if relative in locations else base/relative
                require(file_identity(path) == identity, "reviewed input drift: " + relative)
    return dict(subject=subject,attempt=attempt,request_sha256=sha(out/"request.json"),verdict_sha256=sha(out/"verdict-returned.json"),audit_sha256=sha(out/"post-review-audit.json"),receipt_sha256=sha(out/"receipt.json"),scores=verdict["scores"],fingerprints=verdict["fingerprints"])


def release_prerequisites(root):
    archive=root/"docs/planning/evidence/C45/activation-20261102-g11/p09-source"
    p09=actual_review(root,"P09","attempt-02",archive)
    standing=read(root/"docs/planning/evidence/P10/completion-proof.json")
    require(standing["schema"] == "airs-desktop/p10-completion-proof/1" and standing["p09"] == p09,"P09 prerequisite binding")
    # P10's exact reviewed source is archived on completion. Later C26 release
    # implementation gets its own checks/review; it does not relabel P10 inputs.
    archived=root/standing["source_archive"]
    require(archived.resolve().is_relative_to((root/"docs/planning/evidence/P10").resolve()),"P10 archive escapes evidence")
    p10=actual_review(root,"P10",standing["attempt"],archived)
    require(standing["p10"] == p10,"P10 completion proof binding")
    c45=actual_review(root,"C45",standing["c45"]["attempt"],root/standing["c45_source_archive"])
    require(standing["c45"] == c45,"C45 member proof binding")
    from packet import qualification
    from historical import Historical
    historical=Historical(root,standing["historical_context"])
    qualified=qualification(root,root/standing["qualification"]["record"],current=False,historical=historical)
    request=read(root/"docs/planning/reviews/P10"/standing["attempt"]/"request.json")
    require(qualified == standing["qualification"] == request["qualification"] and request["predecessors"]["C45"] == c45,"P10 qualified candidate/member binding")
    return {"p09":p09,"p10":p10,"c45":c45,"controller_complete":False,"c26_required_here":False}


if __name__ == "__main__":
    require(len(sys.argv) == 2 and sys.argv[1] == "release-prerequisites", "usage: review_gate.py release-prerequisites")
    print(json.dumps(release_prerequisites(Path.cwd().resolve()),indent=2))
