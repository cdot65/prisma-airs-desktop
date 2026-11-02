Recorded UTC 2026-11-02T05:23:46.498668+00:00

# Optional complete-bundle predicate and standalone inspection

Supplementary root advisory; not a numeric verdict, applied fix, full native audit or phase pass. The active application, native drivers/helpers, tools/p09 sources, sealed inputs and current resume04 execution were not changed by this work. G6 continues serially. No exception to current checks/reviews or retroactive verification-reuse policy is asserted.

The existing manual phase fallback rehashes the live executable and compares declared bundle hashes, without independently recomputing the complete live app tree/CFBundleIdentifier. Native Rust current_state already has the canonical whole-bundle/identifier semantics. A current final artifact comparison closes this boundary; no actual bundle drift was observed.

## Tested optional proposal

proposal-artifacts/proposal.patch changes only tools/p09/phase_inputs.py in scratch, preserving return shape/native helpers. It reproduces the Rust inventory (relative paths, execute bit normalization, symlink target bytes, sorted NUL/newline-delimited entries) and uses stock plutil with empty environment for fixture identity. Python directory iteration can fail closed where Rust skips individual enumeration errors; README discloses that stricter behavior.

Seven stdlib test methods pass, including valid bundle, identical-executable resource drift, changed symlink target, changed executable bit, wrong identifier despite rebound tree hash, independent literal inventory vector, and eight control-character prefix/suffix subcases. Original proposal failed all eight new control subcases; revision02 rejects U+001C-U+001F before Python strip, resolving the Rust normalization difference. Raw red/green logs, base hashes, full/delta patches and results are preserved. Independent read-only proposal review found no material issue after correction; no numeric score assigned.

Helper-only actual G6 comparison passed: executable fd51e81755a555f0daeee52168f134254c569e84b8ae82f684c3f3f839f83d56, complete bundle7d5a2dd4700bbea8f24f34eb97e6eb64a77642d6b0c19ae32b3f76aaf6bad909, actual/prepared/registered fixture identity matched. This was an instantaneous comparison during qualification, NOT final acceptance; repeat at final acceptance. actual-bundle-check.json records method/source/expected/actual values.

## Main-implementer disposition

Choose a standalone read-only final comparison with current resume04 unchanged, or integrate the optional predicate/tests at a suitable authorized boundary under the existing workflow (or future P10 policy). This proposal creates no new approval wait and requires no interruption. Do not count unapplied code or isolated test results as current project checks; do not relabel any original result. Current native/check/member/phase reviews remain required.

External originals: /Users/admin/.agent-workflows/airs/2026-11-01-worf-start/p09-bundle-predicate-proposal/revision-02/. Full patch SHA25616b5db318b1cadcbd69005c5fc7ba7727bb1a44336f52ec2e8a7e21ab69af8e6. Root authoring agent audit_early_worktrees; independent advisory reviewer audit_main_pending. Official reviews remain the fresh read-only Codex process.
