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
