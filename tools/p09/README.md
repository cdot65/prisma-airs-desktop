# Portable P09 review helpers

These are the owner-authorized C29–C32 manual review helpers formerly kept in
Kirk's temporary queue. Run them from the repository root. They preserve the
existing gates; copying them does not run a review, native test or live probe.

Prerequisites:

- Install the repository's pinned toolchain/dependencies and fetch Git LFS
  objects (`git lfs pull`). In particular, the retained L19 qualification
  executable is `docs/planning/evidence/C29/live/L19/build-02/l19-qualification-executable.bin`;
  its location record is alongside it. The audit prefers the original executable
  if the historical absolute path in `build.json` still exists, otherwise it
  checks the archived executable. Its SHA must match the original live receipt
  in either case. The historical build/receipt paths are not rewritten.
- Rebuild the P09 fixture and run fresh native qualification in the Worf VM;
  retain the earlier build/evidence and record the new preparation. Mapped
  native checks must pass on that exact prepared executable. C43 must include
  all 24 retained behaviors and 48 layout/keyboard rows with matching capture
  hashes. Historical C44/C43 smoke evidence cannot substitute.
- Use a fresh P09 check attempt on unchanged inputs. The retained Kirk run is
  `attempt-02` (18 checks passed); its logs are history, not a Worf run. The
  check audit retains the existing comparison against the `attempt-01` build
  input inventory, allowing only the three recorded native-tool changes.
- The packet needs the passing L19 receipt and the preceding chunk's full gate.
  The original two live reads plus the separately approved third remain charged;
  these helpers authorize no additional live calls.

Example after the fresh fixture/native prerequisites are satisfied (choose
unused attempt IDs if these already exist):

```sh
bash docs/planning/evidence/P09/runs/run-checks.sh attempt-03
python3 tools/p09/audit-p09-checks.py attempt-03
python3 tools/p09/make-remedial-packet.py C29 attempt-08 attempt-07 attempt-03
python3 tools/p09/launch-remedial-review.py C29 attempt-08
python3 tools/p09/audit-remedial-review.py C29 attempt-08
```

The packet arguments are subject, new review attempt, prior review attempt and
check attempt (default `attempt-02`). The launcher accepts an optional third
argument naming the review model; otherwise it reads the top-level `model` in
`~/.codex/config.toml` and fails clearly if none is available. It never invents
a model. Reading that TOML requires Python 3.11 or newer; an explicit model
argument also works with the older Python versions supported by these scripts.
The fresh Codex session is read-only and uses the retained strict
verdict schema. Preserve every verdict and use a new attempt for another run.

Temporary comparison inventories go under the ignored
`.evidence-private/p09-review/` directory, which the helpers create. Check and
review records remain under their existing repository evidence paths. Keep
scoped inputs unchanged until the post-review audit is complete, report the
actual verdict scores, and apply the standing D034 acceptance/remediation rules.

Start with [the Worf continuation guide](../../docs/planning/WORF-START-HERE.md). `verify-migration.py` checks the captured source/worktree checkpoint after cloning. `make-phase-packet.py` is an explicitly disabled historical draft; enable it only after implementing its current-P09 checks.
