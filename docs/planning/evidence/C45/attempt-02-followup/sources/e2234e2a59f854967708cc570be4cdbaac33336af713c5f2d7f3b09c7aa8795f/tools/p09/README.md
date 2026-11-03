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
- Use a fresh P09 check attempt on unchanged inputs. Kirk `attempt-02` is
  historical; Worf `attempt-03` failed 17/18 with retained fixture failures.
  Worf `attempt-04` also finished 17/18: the full fixture suite passed, but
  fixture Clippy rejected the observer's placement after a test module. The
  unchanged fixture-only observer was moved before that module; the exact
  Clippy command and formatting check passed. Generation 04 requalifies this
  source ordering; generation 04's 19 natives and check `attempt-05` then passed.
  C29 attempt-08 passed 9/9/9/9/9. C30 attempt-10 returned 9/8/8/8/8 with
  blocker B1 and was not accepted: stale destructive switch retries and missing
  current switch-latency evidence require remediation. The current candidate
  binds native guard references and retires renderer retries. Generation 05,
  current performance acceptance, fresh check `attempt-06` and changed-source
  C29/C30 re-reviews remain required; no old score transfers to changed inputs.
  G6 then passed all19 natives/current C30p95=25.065583ms/check06all18,
  and C29attempt09 passed full9. C30attempt11 returned9/8/9/8/8 without
  blockers, accepted underD034 with R1/E1: a definite tenant-state save
  refusal could occur after conversation resolution. Both switch paths now
  persist staged Data before resolution/history/context retirement; all4
  real-Harness fault/recovery cases fail originalG6 and pass corrected code.
  Broader22Harness/10onboarding/5setup-read preflight and strictClippy/fmt pass.
  G7 preparation/all19/currentperformance/check07 and fresh C29attempt10 ->
  C30attempt12 remain required before C31/C32/phase. No score transfers.
  The current audit requires every compiled input to match the fresh native
  build's captured before/after seal (`P09/native-build-inputs.json` and its
  generation under `P09/preparations/`). There is no source-exemption list.
  The old attempt-01 comparison remains provenance only. Failed Worf generations,
  scoped AX corrections and fixture-lifecycle remediation remain preserved.
- The packet needs the passing L19 receipt and the preceding chunk's full gate.
  The original two live reads plus the separately approved third remain charged;
  these helpers authorize no additional live calls.

Example after the fresh fixture/native prerequisites are satisfied (choose
unused attempt IDs if these already exist):

```sh
bash docs/planning/evidence/P09/runs/run-checks.sh attempt-07
python3 tools/p09/audit-p09-checks.py attempt-07
python3 tools/p09/make-remedial-packet.py C29 attempt-10 attempt-09 attempt-07
python3 tools/p09/launch-remedial-review.py C29 attempt-10
python3 tools/p09/audit-remedial-review.py C29 attempt-10
```

The packet arguments are subject, new review attempt, prior review attempt and
check attempt (default `attempt-02`). The launcher accepts an optional third
argument naming the review model; otherwise it reads the top-level `model` in
`~/.codex/config.toml` and fails clearly if none is available. It never invents
a model. Reading that TOML requires Python 3.11 or newer; an explicit model
argument also works with the older Python versions supported by these scripts.
The fresh Codex session runs in process with `--no-daemon`, read-only sandbox,
approval policy `never`, web search disabled and captured JSONL output. It uses the retained strict
verdict schema. Hooks, plugins/apps, browser/UI tools, delegated agents and
unbounded connection retries are explicitly disabled for the reviewer process;
an actual CLI feature probe, invocation metadata and final-message binding are
audited. Failed launch logs are never overwritten; use a fresh attempt.
Preserve every verdict and use a new attempt for another run.

Temporary comparison inventories go under the ignored
`.evidence-private/p09-review/` directory, which the helpers create. Check and
review records remain under their existing repository evidence paths. Keep
scoped inputs unchanged until the post-review audit is complete, report the
actual verdict scores, and apply the standing D034 acceptance/remediation rules.

Start with [the Worf continuation guide](../../docs/planning/WORF-START-HERE.md).
`verify-migration.py` checks the captured clone checkpoint; intentional later
evidence/source changes are not reset to satisfy that historical inventory.

The former disabled phase draft is preserved under P09/phase-tooling-history.
`make-phase-packet.py` now uses `phase_inputs.py` before creating any attempt:
all 19 current native drivers on the exact executable, complete C43 coverage,
all 18 current check logs/frozen inputs, actual full-9 member and prior-phase
verdict bindings, and C29-C32 post-review audits. C41's explicit override and
C33's supersession remain distinct. No passing phase or controller receipt is
inferred from input verification. After those prerequisites:

```sh
python3 -I tools/p09/test_phase_inputs.py
python3 -I tools/p09/make-phase-packet.py attempt-01 docs/planning/evidence/P09/runs/run-attempt-07.json
python3 tools/p09/launch-remedial-review.py P09 attempt-01
python3 -I tools/p09/audit-phase-review.py attempt-01
```

Use fresh IDs. The phase packet binds file modes and symlink targets, source,
current native evidence, failed Worf generations, check logs and predecessor
records in separate self-excluding inventories. The post-review audit rechecks
both inventories and the complete current gates. Phase completion still needs
an actual independent verdict at 9 in every dimension with no fixes/gaps/blockers.


C30's current acceptance measurement is mandatory before cumulative C44 runs:

```sh
AIRS_NATIVE_SUBJECT=P09 python3 -I tools/perf/measure_c30.py --label acceptance --attempt attempt-03
```

Run it as the sole UI writer after C36/C43 and before C44, on the newly prepared
fixture. It preserves the original budget/registration and refuses existing
acceptance/attempt paths. Method2's presentation/guest rationale is recorded
before measurement under P09/performance/C30. All12 raw trials, fixed2warmups,
10measured switches, actual AX observations, unchanged sources/inputs and the
prepared executable/probe are bound. The phase input verifier recomputes the
100ms p95 gate and checks chronological nonoverlap with native drivers. A failed
measurement remains failed evidence and must be archived before a fresh attempt;
no result exists until this real driver runs. Fresh C30 review must assess the
method/guest rationale. The C30 failed review and production-interface regression
logs/original sources remain in evidence/C30/remediation-attempt-10.

The additive `*_c31_transition.py` verifiers and `*-c31-transition*.py`
entrypoints handle the explicit 2026-11-02 C31 remediation transition. They
preserve the original G7 owner-closure verifiers and raw verdicts. The baseline
source inventory, exact declared fix commits, complete before/after source delta
and focused preflight are sealed by `seal-c31-transition.py`. Source outside the
declared C31 and named administrative paths must remain unchanged. Fresh current
preparation, 19 mapped native drivers and 18 checks qualify changed application
bytes; G7 results are historical. No new C30 qualification is performed. C30's
bound owner all-nine score continues; its actual low independent verdict remains
unchanged. C29's actual full-nine G7 review stays historical. Current C31, C32
and cumulative P09 require actual independent full-nine reviews and audits.

G8's first C31 driver stopped before acceptance because Keys requires an
explicit Gateway workspace. The driver-only amendment preserves that failed
attempt and the first source seal. `verify_driver_amendment` binds the original
G8 preparation, exact executable and complete bundle, proves the compiled-input
delta contains only the C31 driver and its test, and checks that runtime changes
are confined to `gateway_key_checks` (called only by C31/C39). The ten completed
G8 records/logs and direct driver hashes stay immutable. C31/C39/C44 and all
unrun drivers execute freshly; the 18 current checks run after continuation.
The new source seal is `transition-driver-01.json`; the original `transition.json`
and original actual build record are retained unchanged. This is an explicit
same-application continuation, with no G7 result relabeled as current and no
new C30 qualification.

The final attempt-06 remediation uses `transition-final-01.json` and a fresh
current build/qualification. G8's original seal, explicit driver continuation,
all recorded results and exact full application remain historical and immutable.
The additive LFS rules retain its two large app executables without changing
archived bytes. G9 never reuses a G8 application/native pass.

`c31_owner_completion.py` implements the committed conditional owner directive.
It accepts only the first completed actual numeric C31 verdict after attempt-06,
bound to the final source, evidence, transport, current qualification and audit.
Any score below nine activates separate owner all-nine completion, including
when blockers remain. Raw verdict/receipt/audit scores, arrays and false full-gate
flags stay unchanged. Attempt-06, a missing verdict or a transport error cannot
activate it. C32 predecessor, P09 member and C45 activation use that same binding;
C32/P09 still need actual independent full-nine passes. Once activated, no further
C31 score-chasing work or reopening those accepted findings through P09 is
allowed. The directive itself is an explicit administrative source-seal path.
