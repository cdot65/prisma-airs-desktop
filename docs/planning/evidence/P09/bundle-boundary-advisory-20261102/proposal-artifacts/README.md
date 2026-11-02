# Revision 02: P09 final prepared-bundle predicate proposal

This is an optional external proposal. It does not change the active checkout,
resume-04, native helpers, evidence, command results or any prior proposal file.

The only semantic correction from revision 01 is to reject U+001C, U+001D,
U+001E and U+001F in the raw decoded plutil output before Python stripping.
Python strip removes these four characters but Rust str::trim preserves them.
Rejecting them fails closed for the registered fixture identifier, without adding
general Unicode normalization machinery. Whole-bundle hashing and the original
return shape remain unchanged.

Validation:
- The original proposal was run against the expanded test file as an expected
  red control. All eight added binary-plist prefix/suffix subcases exposed the
  original normalization issue (7 test methods, 8 failing subtests; exit 1).
- Revision 02 passed all 7 test methods, including the 8 new control subcases,
  in 0.056 seconds. All original valid/resource/link/mode/identifier/canonical
  inventory cases remain passing. No failures, errors or skips.
- Both full proposal.patch and revision-delta.patch passed git apply --check
  against copies of their exact respective bases.
- The helper-only actual G6 comparison passed in 0.224160917 seconds. Its current
  executable fd51e817... and complete Rust-sealed bundle 7d5a2dd4... matched, as
  did the registered fixture identifier. C05 was the sole UI writer; no actual
  timing acceptance was running. The preparation and active source hashes did
  not change. This is not native/performance acceptance or a review score.

Canonical inventory: paths are globally sorted, directories are omitted,
symlinks are hashed by target bytes without traversal, any execute bit maps to
100755 (otherwise 100644), and entries use path-NUL-mode-NUL-SHA-newline.
The current G6 comparison confirms this Python inventory equals the stored
Rust native preparation hash for the actual readable bundle.

Directory error semantics are intentionally stricter: Python directory
iteration errors propagate and fail closed, whereas Rust collect_tree uses
read_dir(...).filter_map(|entry| entry.ok()) and can skip individual failed
directory entries. No claim of identical I/O-error recovery is made. For stable,
readable bundle contents, the inventory and identifiers follow the referenced
Rust implementation; an inaccessible directory must not be accepted by this
proposal. The separate second metadata lookup preserves Rust file_entry's
deleted-entry behavior if a listed file disappears.

Files:
- base/: original active-source and Rust-reference copies.
- previous-proposal/: unchanged revision-01 source copy for delta patch checking.
- base-hashes.json: both patch bases, proposal hashes and original-file hash map.
- proposal/tools/p09/phase_inputs.py: complete revised source.
- proposal.patch: full patch against active base a0088fea...c1c51.
- revision-delta.patch: small correction against original proposal 47c8b47a....
- test_bundle_predicate.py: stdlib unittest with real stock plutil extraction.
- tests-red-revision01.log, tests.log, test-executions.json: exact tiny runs.
- apply-check.log, delta-check.log: successful patch application checks.
- actual-bundle-check.json: helper-only current G6 comparison.
- results.json: result and preservation summary.

Run from this revision directory:

    /usr/bin/python3 -B test_bundle_predicate.py

All original proposal files, results and logs remain byte-identical to their
recorded values. No Rust build/suite, dependency install, native UI, performance
measurement, official gate or independent review was run. Main may use the
current final-artifact proof or choose future-only predicate adoption. Application
is not performed here. A mismatch remains a failure to investigate, never a
native rerun bypass or a relabeled historical result.
