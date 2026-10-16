# Failed-review remediation prompt

```text
Remediate the latest failed review of GATE_OR_CHUNK_ID. Read its unchanged raw
verdict, source/dependency/check manifests, prior attempts and canonical instructions.
For each required fix/evidence gap, record the causal defect, exact changed behavior,
and acceptance test/evidence that resolves it. Do not raise scores yourself, delete
failed attempts, broaden scope, or weaken tests/gates to force a pass.

Change only affected approved source/plan and supporting evidence. Rerun relevant
checks and regressions, freeze new hashes, then request a fresh actual independent
review of the same gate/chunk. Keep it active until every critical dimension and
overall score is >=9, no required fixes remain, and mandatory evidence is present.
```
