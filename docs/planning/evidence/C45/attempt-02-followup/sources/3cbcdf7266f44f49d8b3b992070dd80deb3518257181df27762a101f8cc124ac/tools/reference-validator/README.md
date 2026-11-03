# One-off C01 bootstrap reference validator

C01 builds the controller that gates every later chunk, so the controller
cannot certify its own first receipt. `validate_receipt.py` is an independent
Python (standard library only) re-implementation of the receipt checks. It
shares no code with `crates/xtask`; it works from the documented contracts:
the verdict schema, `checks.json` (approved command plan), `native-checks.json`,
`toolchain.json`, the fingerprint algorithms (`path\0mode\0sha256\n` inventories,
canonical sorted compact JSON for dependency and build-flag parts) and the
packet/receipt formats. Every record it reads (receipt, transport, packet,
run and command, native acceptance) is checked against typed definitions that
mirror the controller's serde types: primitive types with Rust integer ranges
(booleans and floats are not integers), enums, nested records, nullable fields
(a missing `Option` is accepted, as serde does), unknown fields where serde
denies them, and strict JSON (UTF-8 only, no NaN/Infinity, no repeated struct
fields, no lone surrogates). It recomputes the current source, dependency, build-flag
and evidence fingerprints itself; on the C01 tree they equal xtask's.

```text
python3 -I -B tools/reference-validator/validate_receipt.py validate . docs/planning/reviews/C01/attempt-NN/receipt.json
python3 -I -B tools/reference-validator/validate_receipt.py bootstrap . docs/planning/reviews/C01/attempt-NN target/debug/xtask
```

`bootstrap` requires both validators to accept the real receipt and resealed
acceptance controls (an unmodified copy, and one omitting the optional
transport model with reordered keys), and both to reject each resealed single
mutation
(each score at 8, required fix/blocker/gap/fail recommendation despite nines,
schema violations, request/receipt-score mismatches, each current fingerprint
differing, final output disagreeing with verdict.json, nonzero or timed-out
transport, no completed turn, inputs changed during review, a missing test run,
a changed predecessor receipt, incomplete or misidentified native records, and
typed-record cases whose values keep Python truthiness/equality with the real
ones: integer `timed_out`, float `exit_code`, string `passed` and
`required_fixes`, a float score, a repeated receipt field, enum values outside
or in the wrong case (run kind, native method and check result, run status),
integer native `fixture` and executable hash, a nested unknown field, string
command exit code, boolean test count and integer environment value).
Hashes are recomputed after each mutation so the semantic guard, not the tamper
check, is what rejects it. The result is written to
`<attempt>/bootstrap/result.json`, outside the hash-bound subject evidence, and
`xtask complete --chunk C01` requires it to name the exact receipt. After C01
completes, the controller gates later work; this validator is not in that path.
