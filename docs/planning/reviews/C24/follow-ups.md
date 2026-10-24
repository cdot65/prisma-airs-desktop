# C24 follow-ups for the P07 push to 9/10 (D034)

C24 completed at 8/10 under D034 (4e5a9e6c). Codex attempt-01 scored
8/8/8/8/8 with no blockers, two required fixes (F1, F2) and three evidence
gaps (G1–G3), and acknowledged budgets.json version 4. All are remediated
in the P07 push. Each fix has a test through the authority
(`crates/desktop-core/tests/native_quality`) or the presentation, and
mutation probes in `docs/planning/evidence/C24/mutation-probes/probes.py`
(section "P07 follow-ups"). D051 records the resulting rules.

## F1: diagnostic values are the app's own, not a character set

- Code (`diagnostics.rs`, `authority.rs`):
  - `Event::fields_ok` checks each field against what the app records for
    that event: a request kind in `INTENT_KINDS`, generated with
    `Intent::kind` from one table (so the two cannot differ); a refusal
    code, storage health or reset outcome among those enums' serialized
    names (each list sits beside an exhaustive match, so a new variant does
    not build until it is listed); a `major.minor.patch` version; a panic
    location that is `external` or a lower-case Rust source file of this
    repository.
  - The same check runs when an event is recorded and when a stored line is
    read back, counted or exported.
  - `record_panic` normalizes every other location (a dependency's, the
    standard library's) to `external` with line 0.
- Test `only_values_the_app_records_are_exported`:
  - Valid JSON lines are planted with a token-shaped canary and single
    ordinary words in every string field (storage health, request kind,
    refusal, reset outcome, version, panic file).
  - Panics are recorded at a registry path, a standard-library path and a
    repository file.
  - The export holds none of the canaries or words, keeps the app's own
    events, and has exactly the repository panic plus two `external` ones.
- Probes: `a token-shaped storage health is exported`, `a single-word
  request kind is exported`, `a single-word refusal is exported`, `any
  reset outcome is exported`, `any version is exported`, `a standard
  library panic path is recorded`, and the re-anchored `an absolute panic
  path is recorded`.

## F2: a clearing that cannot remove the files says so

- Code:
  - `Diagnostics::clear` returns whether both files were removed. A file
    that could not be removed is counted again from disk, and no clearing is
    recorded; a later clearing that can removes it.
  - The view gains `clear: cleared | failed` (contract-validated). Settings
    announces a clearing as a status, and a failed one as an alert saying
    the events counted are still kept.
- Test `a_clearing_that_cannot_remove_the_files_says_so_and_keeps_counting_them`:
  - The logs folder is made non-writable and Clear is sent.
  - The view reads `failed`; no clearing is recorded; the count equals the
    lines still on disk; an export still holds them.
  - Retried, the clearing removes them and reads `cleared`.
- React test `announces a clearing, and says when one could not remove
  everything` (including a keyboard retry); contract tests for `clear`.
- Probes: `a failed clearing counts nothing`, `a failed clearing is
  recorded as cleared`, `a failed clearing reads cleared`, `a failed
  clearing is not said`, `an unknown clearing passes the contract`, and the
  re-anchored `clearing keeps the lines`.

## G1: the network inventory covers the WebKit processes

- `acceptance_c24.py` samples the app and every WebKit process that
  appeared with its launch, at startup (15 s) and again after reconnect (8 s,
  with nothing asked). Each line names its process ID and name, and the
  processes sampled are listed in the logs.
- C24-N03 fails if no WebKit process was sampled in either window, so
  WebKit activity cannot be left out silently. The stated limits (sampled
  lsof, DNS not attributed) are kept.

## G2: Diagnostics by keyboard, through the real save sheet

- New check C24-N07 (`native-checks.json`), driven with the fixture switch
  `fixture-native-sheets` so the production NSSavePanel is shown:
  - Export diagnostics… is reached by Tab and pressed with Space;
  - the panel saves into a driver-owned folder, and "Diagnostics saved." is
    announced;
  - a second export cancelled in the panel saves nothing, and "Nothing was
    saved." is announced;
  - focus returns to the window both times;
  - Clear diagnostics is reached and pressed by keyboard, and its outcome
    announced;
  - both controls are reachable and clear of the bottom fade at 200% zoom
    and at the minimum window size.
- No Accessibility tree is saved while a panel lists folders.
- C24-N01's description now says its export runs without a sheet in the
  fixture build.

## G3: the complete package is measured and its budgets resolved

- `xtask footprint` unpacks the sealed Harness engine and builds the
  production app with `tauri.harness.conf.json`, as `native-prepare` does
  for the fixture build, so the footprint is that of the app that ships.
- budgets.json version 5 (registered before that package was measured;
  `evidence/C03/budget-registration.json`, version 4 archived) keeps the
  first slice's allowance and adds exactly the engine's pinned sizes from
  `resources/harness/pin.json`:
  - app bundle: 150 + 328.4 = 479 MiB;
  - DMG: 100 + 108.2 = 209 MiB.
- Prior results are kept: `footprint-v4-without-engine-20261023.json`.
- The governance needs owner or independent reviewer acknowledgement of
  version 5; the review is asked for it. Signed and notarized packaging
  stays C26's.
