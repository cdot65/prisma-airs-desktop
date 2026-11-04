# Shared P11 qualification and actual reviews

The accepted D063 plan selects the changed C46–C49 flows. P09/P10 and RC1 retain
their original results and artifacts. One actual P11 Check/Test run and fixture
candidate serve all four chunk reviews and the phase review. No phase run is
copied, renamed or reported as a newly executed chunk run. The standard ledger
remains unchanged (`controller_complete:false`), as with the accepted P10 manual
review fallback.

Use the existing Rust Runner's registered P11 focused command plans and one
`xtask native-prepare --phase P11 --target aarch64-apple-darwin --features
test-fixtures`. Finish source preflight before that build. Real source failures
get causal checks; driver-only fixes retain the same verified app bytes. Keep
all failed attempts. Do not rerun unrelated Rust/native suites for provenance.

`acceptance_p11.py` requires the explicit fixture bundle, owned processes and
local synthetic endpoints. It records measured facts and capture hashes first.
Its no-UI finalizer requires an actual source-bound P11 test RunRecord and an
explicit, capture-specific visual review. No pixels or invisible authority
boundary are inferred from a test count, successful script exit or AX absence.

`proof.py make` binds the actual Check/Test RunRecords, current metadata-only
fingerprint, prepare, finalized native record, original observation manifest and
build receipt into one immutable qualification folder. Its CLI lists the exact
required inputs. It reuses P10's raw evidence validator and rejects missing or
changed command/native membership, logs, current source/dependencies, candidate,
helpers, incomplete build inputs and failed observation exits. The Python source
hash recomputation was checked against the actual Rust fingerprint algorithm.

For the two reviewed conditional-style fixes, `--check` may name an explicit
`airs-desktop/p11-check-selection/1` receipt. It binds the original failed Check
run, exact before/current source inventories, the fixed module's exact hashes,
and a current `airs-desktop/p11-check-supplement/1` containing only affected or
previously blocked commands. Every registered command is accounted in order.
Passing setup/bindings/typecheck/compile results may be `carried_unaffected`
with a reason; their original source, timestamps and logs remain bound. Format
and all affected/blocked lint/notice checks execute. This receipt is never a
new passing full-run record. Tests and native assertions still execute on the
shared current candidate, and archived standing uses this same strict consumer.
The dated test-only `unwrap` to descriptive `expect` correction is also bound
by its exact old/new hashes. `airs-desktop/rust-test-build-carry/1` retains the
original build/prepare and binds the current Check selection and Test run; only
that Rust test file may differ. It requires no frontend or native rebuild and
does not claim either was executed. Executable and complete bundle verification
remain independent requirements.

For a frontend test-only source change, `--build-input-carry` requires the
original native build and a successful current-source frontend rebuild whose
complete output is byte-identical. Its command, input hashes and raw log remain
bound; the retained native build is never reported as newly executed.

`review.py make C46 attempt-01 QUALIFICATION` (likewise C47–C49) writes a frozen
request and inventories. Launch it with the retained
`tools/p09/launch-remedial-review.py C46 attempt-01 gpt-6.1-sol`, then use
`review.py audit C46 attempt-01`. The launcher requests a fresh, ephemeral,
read-only reviewer with hooks/plugins/network/UI features disabled. Every actual
score and finding is retained. After all four pass the full gate, make and audit
the P11 request against exactly the same qualification. Chunk acceptance at8
follows D034; P11 still requires every member and phase at9 with empty findings.

`standing.finalize(root, qualification, attempts)` runs only after those five
actual reviews. It refuses existing outputs, verifies the three explicit LFS
paths, and archives the exact source, fixture app and proof/evidence membership.
`standing.py release-prerequisites` subsequently verifies that archived standing
without a current `target` directory, installed app, host toolchain or matching
hostname. Production promotion requires both the original P09/P10 proof and this
new P11 standing. Stable installed/live acceptance and Linux qualification remain
separate. These tools perform no signing, publication or live product calls.

## VM affected verification after the Modal fix

The dedicated VM uses the unchanged registered command plan. Run selected labels
with the existing Runner through the small build-only example:

    cargo run --locked -p xtask --example p11_affected -- check C46/check/1 C46/check/4
    cargo run --locked -p xtask --example p11_affected -- test <all 12 registered frontend labels, in order>

This produces ordinary subset RunRecords. It refuses setup, unknown, duplicate
and reordered labels; it does not create full-suite or controller completion.
The source checks originally run on Kirk retain their original logs and inputs.

The optional proof.py make --impact-selection FILE keeps --check and --tests
pointing to the original D2 records. FILE has schema
airs-desktop/p11-impact-selection/1, current runner_inputs, a nonempty reason,
and these bound references: baseline_source, baseline_candidate,
baseline_check, baseline_test, current_source, tool_transition, and
current_runs (check/test). Both source records contain files and git_config.
It also lists exact changed_paths, dependencies from carry_dependencies,
and affected labels: the two checks above and all registered frontend tests.

D2 is source-check context, with no native qualification claimed. Its permanent
archive is docs/planning/evidence/P11/source-history/D2. The original failed
Check and its two repairs stay intact. Current format/typecheck and frontend
results execute in the VM; eight unchanged Rust test commands and seven other
checks are carried with explicit reasons. Dependencies include all desktop-core,
xtask library, contracts, resources, architecture inputs and root lock/toolchain
files. The tool audit must bind exact original/current compiler, Cargo, Node and
pnpm bytes; only the rustup shims may differ. No whole dependency hash is relabelled.

The P11 wrapper validates Check provenance at its original source context. The
unchanged P10 generic consumer validates carried Rust results against their
original candidate, raw runs/logs and exact dependency proofs. Every registered
requirement remains present. Final standing includes both source contexts and
verifies without a target directory or the original host. A changed production
dependency, omitted command, failed/zero-test result or changed compiler fails
the consumer. New native assertions still require actual current VM evidence.

## Native helper continuation without replaying source QA

For helper/proof-only fixes after the VM source checks, --helper-source-carry
binds schema airs-desktop/p11-helper-source-carry/1. It names baseline_source
(the archived S2 source seal), baseline_candidate (S2 app/VM context),
impact_selection (the original D2/S2 selection), current_source (S3), exact
changed_paths, current runner_inputs, unchanged conditions, a reason and qa refs.
The complete application input identities and dependency/build fingerprints must
match. Only native/P11 helpers, their build-only example and owner execution
documentation/attributes may change; an application edit is rejected.

Each QA ref has schema airs-desktop/p11-helper-qa/1, actual argv/cwd, exit_code,
positive passing tests, a bound log and helper_files mapping names to current
hashes. The records must cover every changed Python helper. Their relevant
helper hashes bind validity; unrelated source edits do not force repeated QA.

The fourteen S2 frontend/check results retain their actual RunRecords and become
reused_exact against the same app, flags and VM conditions with empty helper
dependencies. The eight D2 Rust results remain carried_unaffected. Original
failed Check evidence stays unchanged. Both D2 and S2 source archives are in
the durable historical context; no current target or original host is needed.

A native continuation names a native-selection/1 manifest and finalized standard
acceptance. Per-row origins retain the failed original process status separately
from completed assertion status. C46 uses its original completed observations
and unchanged relevant helpers; remaining rows use the successful continuation.
The native provenance validator binds actual per-row logs, arguments, times,
capture reviews and app identities. A failed whole process is never relabelled
as a newly passing process.

### Review remediation without replaying unaffected flows

`remediation.py RECEIPT --out FRESH_FOLDER` consumes the dated C46/C47/C48
review fixes. `proof.py verify FRESH_FOLDER/qualification.json` remains the
review entrypoint. This path preserves qualification02 and all original raw
outcomes; it does not turn a failed contributing process into a passing run.

The receipt (`airs-desktop/p11-review-remediation/1`) binds the original
qualification, D2/S2/S3 source history, the preserved original app tar, current
source/fingerprint/build/prepare, the actual nine Rust integration consumers,
C48 source selectors, the old-artifact drawer-scroll supplement, current-artifact
recovery, helper QA, and a reason for every carried requirement. Exactly four
frontend files and two Rust fixture files have pinned before/after hashes. Other
application, dependency, native-helper or VM changes cannot silently use this
review decision. The versioned native build example is a build tool, not a
production-source exemption.

Unchanged rows retain their original result and candidate references. Explicit
`carried_unaffected` dependency proofs include every resource and the relevant
native helpers. Source selectors replace only affected verification goals;
the original registered command remains visible. These custom selector records
retain actual commands, logs, source inputs and known timing limitations rather
than manufacturing a complete `airs-v2/run/1` or claiming a whole command reran.
C48's new recovery slice retains unrelated first-use/save facts as old subfact
origins. It never presents the original 099/eb9 artifact as the new app.

Historical finalization archives the accepted source/app and retains all three
prior source contexts. P10's generic coverage and historical consumers are
unchanged. This path does not assign review scores or complete a controller.

### Retain unaffected full-nine members after the C48 retry fix

The dated `airs-desktop/p11-member-review-carry/1` receipt retains exactly
C46/C47/C49 attempt-02 on qualification03. It binds their original actual
request/verdict/audit/receipt objects, source S4 and complete old f558 application,
the current qualification/source, the pinned three-file C48 retry-only delta,
per-member reasons and conservative dependency identities including all resources.
C48 and P11 cannot be carried. Unexpected production/configuration changes,
changed dependencies, another qualification or a non-full-nine review reject.

Make C48 attempt-03 against qualification04 normally. After its actual full-nine
audit, make the phase packet with:

```
python3 tools/p11/review.py make P11 attempt-01 QUALIFICATION04 --review-carry docs/planning/evidence/P11/member-review-carry-01/receipt.json
```

The phase request explicitly distinguishes three retained original qualification03
members from current C48; it reviews all current source, integration and the carry
logic. Its audit reads the bound carry reference from that request. Old review
scores/fingerprints/qualification bindings are never rewritten. The current
consumer is not recursively applied to old qualification03 after its one-off
application hashes advance: actual old audits plus exact archived source/app
preserve that original standing. Final historical context includes S4 and the
old f558 application alongside the accepted current source/app. P10 remains
unchanged.
