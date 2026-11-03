# Risk-based verification strategy — owner direction, 2026-11-02

Status: future policy, including C45/P10. Record and implement it before the next phase's qualification. Do not interrupt, change or retroactively reinterpret the active P09 workflow, its sealed inputs, reviews or historical outcomes.

The owner wants materially faster iteration while preserving confidence in the backend/frontend migration. Full end-to-end qualification is reserved for changes that warrant it. The earlier 2026-11-01 exact-artifact verification-reuse decision remains in force.

## Select verification from the change

Before expensive work, write a short verification plan: changed behavior and components, authority/data/platform boundaries touched, affected tests/native flows, retained evidence and its justification, uncertainty, and the reason for any full run. Keep this a small record using the existing runner, not a new orchestration framework.

1. During implementation, run meaningful focused unit and production-interface integration regressions, relevant typechecks, and affected lint/format checks. Cover failure ordering, teardown and stale ownership at this layer first. Rust test-only or lint-only changes require their relevant targets, not an unrelated workspace or native suite.
2. For changed user-facing or native behavior, exercise the affected native flows and boundaries. Include related consumers of shared changed components. Use a small startup/navigation/critical-action smoke set when integration or packaging makes it useful.
3. Run the full end-to-end matrix when justified by broad architectural/runtime migration; wide native bridge, shared authority or lifecycle changes; application identity, protected storage or install changes; release-candidate qualification; or impact/evidence uncertainty that cannot be bounded confidently.
4. A new commit, worktree, merge, review attempt, review score, timestamp or source fingerprint does not by itself justify full re-execution. Decide from actual behavior, dependencies, artifact and execution-condition changes.

When a bug reveals a shared invariant, inspect all its relevant callers and add cross-consumer production-path regressions before sealing. For example, test native confirmation ownership at admission, queued presentation, shown presentation and retirement across applicable callers together. Catch the class of failure before another long UI run.

Validate native assertion maps and their positive/negative prerequisites cheaply. Loading, errors or incomplete screens cannot prove that a successful detail surface lacks forbidden actions.

## Preserve and reuse evidence honestly

- For exact reuse, verify the actual executable and complete canonical app bundle, relevant per-driver helper/import closure, build flags, and execution conditions against the original passing evidence. Run newly affected checks only. A source-input hash change alone must not force native or Rust suite repetition.
- When application behavior or relevant artifact bytes change, execute affected checks on the current candidate. Unaffected historical coverage may be carried with an explicit dependency/impact justification and original artifact binding; do not call it a current execution or assert byte equality that does not hold. Broaden testing when that justification is uncertain.
- For an isolated native driver/helper change, run its coverage validators and affected driver consumers. Retain other valid completed drivers when their own helper closure, app artifact and relevant conditions remain valid.
- Every required assertion needs accounted coverage. Distinguish executed now, reused with exact proof, carried unaffected with rationale, not applicable with reason, and explicit owner-accepted risk. An assertion cannot disappear through omission or a score change.
- Preserve raw logs, failures, verdicts, source inventories, hashes and original run records unchanged. New proof references them; it never rewrites their provenance or turns an unperformed check into a pass.
- Missing or inconsistent proof requires the affected verification. New requirement retirements and accepted risks retain their existing owner-decision rules.

## Coalesce expensive work

Finish focused checks, source/interface review and cross-consumer risk inspection before freezing a candidate. Resolve known lint, typing, contract and fixture/assertion problems before starting the long matrix.

Qualify one stable candidate at the justified integration boundary. Let chunk and phase reviews consume that same qualified artifact and evidence when validity is preserved. A formal review still examines the actual current source and evidence and reports its real scores; it does not automatically schedule another full test run.

For review fixes, execute the causal regression and affected checks, then update the impact/evidence record. Remediation or merge triggers another full run only when its actual risk warrants one. Do not reopen unaffected chunks or repeat unrelated checks merely to refresh source provenance.

Keep long native jobs durable and independent of model service lifetime, with one UI writer, ownership/source guards, per-step immutable results and a resumable queue. Recover from infrastructure failure at the first unfinished valid step. Do not restart completed valid steps.

## Apply concretely to C45/P10

C45's new application/Keychain/data identities, fresh-setup isolation and broad presentation cleanup warrant one complete current cumulative native baseline, including new identity/isolation assertions, after implementation and focused preflight settle. Preserve all P09 evidence.

After that baseline, use affected checks for C45 fixes. P10 may consume the same qualified candidate rather than automatically repeating the full C44/C45 matrix and Rust suite. Integration gets checks for actual merge changes; an unchanged qualified artifact and helper closure reuse the existing proof.

After P09 has passed, copy this policy into the isolated C45 worktree at docs/planning/verification-policy.md. Update the C45 brief's blanket rerun wording, repository guidance, test catalogue, review protocol and new P10 provenance/gates to implement this owner direction explicitly. Keep historical P09 verifiers and results unchanged. Keep implementation small: a change-impact/verification-plan record and strict validation of selected checks and evidence reuse.

Retain existing chunk/phase independent-review and release requirements, including separately recorded owner exceptions. This strategy changes verification scope and scheduling; it does not fabricate scores, bypass assertion coverage, grant live access, or authorize signing/publication.
