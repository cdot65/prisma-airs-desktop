# C01 remediation of attempt-02

Attempt-02 (thread transport) scored architecture 8, behavior 8,
authority_security 8, validation 7, overall 8 with seven required fixes and one
evidence gap. Its packet, verdict, thread reply and receipt are unchanged in
attempt-02/. Two of the findings exposed attempt-01 edits that had silently not
applied (scripted replacements whose target text rustfmt had reformatted); all
remediation edits are now made with assertions that the target text exists and
were audited afterwards.

| Finding | Cause | Change | Acceptance |
|---|---|---|---|
| R1 shared package typecheck ran nothing | `--filter ./packages/*` is resolved against the invoking directory, so it matched no project while exiting 0 | shared/5 names all six Academy packages with `--fail-if-no-match` and runs their `typecheck` scripts (each appears in the log); the runner also fails any `--filter` command whose output reports "No projects matched" | fresh check runs (main and clean checkout) show six package typechecks; mutation probe: a type error in a package-only file fails shared/5 |
| R2 web consumer scratch not bound | only the presentation package and its installed copy were fingerprinted | the dependency fingerprint adds `shared/web-scratch`: the whole `.work/academy` tree excluding node_modules, `.env*` and the documented generated outputs (`dist/`, `public/excalidraw-assets/`, `packages/shared/tsconfig.tsbuildinfo`, `stats.html` written by the build's bundle visualizer); verification requires it to equal the locked input under the same exclusions | `edits_to_the_web_consumer_scratch_tree_are_detected` (shell, test config and added files) |
| R3 destructive staging not confined | the confinement edit to `prepare` had not applied | `prepare` confines both paths before reading the lock or deleting anything and refuses `.work` itself | `shared_prepare_rejects_escapes_before_touching_anything` (parent components, absolute, symlinked ancestor, wrong roots; sentinels outside scratch untouched; `.env` never staged) |
| R4 bundle identity | only the executable was re-hashed | native-prepare runs record a `fixture-bundle` artifact; completion re-hashes the whole bundle tree and reads its live `CFBundleIdentifier`; the acceptance record must name the prepared bundle path | `prepared_bundle_bytes_path_and_identifier_are_bound` |
| R5 phase native evidence | phases required only check/test | every chunk and phase requires check, test and native-prepare runs; `phase_native_prepare` builds the integrated fixture; a phase's native record must cover every member chunk's checks; phases fail closed until each member has a native inventory | `phases_require_integrated_native_runs_and_acceptance` |
| R6 bootstrap schema equivalence | Python validator lacked record shapes | strict key sets and schema strings for receipt, transport, packet, fingerprints, run, command, native, app and check records; bundle path/hash/identifier checks; new resealed cases (schema changes, unknown and missing fields, other bundle) for 36 total | bootstrap result for the attempt (both validators agree on every case) |
| R7 observation lifecycle | invalid initial snapshots escaped cleanup; at-cap admission evicted silently | the facade releases both sides when the initial snapshot is invalid; host (8 per webview) and core (32) refuse new observations with `busy` instead of evicting live ones | invoke.test invalid-snapshot cleanup; `observations_per_webview_are_bounded_without_silent_eviction`; core `observers_beyond_the_cap_are_refused_not_evicted` |

Also: desktop-core unit tests now run in every check; the thread wait requires
a completed turn (the earlier edit had not applied; `has_completed_turn` test);
the native driver operates Settings radios by keyboard at 200% zoom and at the
minimum window size with scrolled captures; dated status notes added to the
attempt-01 record and dependencies.md. Mutation probes were rerun against the
current code.

## Found during native acceptance after remediation (2026-10-17)

Three native runs were superseded before the passing one; each is retained
with its cause under `evidence/C01/native-failed/`.

| Finding | Cause | Change | Acceptance |
|---|---|---|---|
| Driver could not reach a named radio | a radio group is one Tab stop on its checked radio, and WebKit does not wrap arrow keys | the driver tabs into the group and moves with Up/Down by tree order before Space, recording the keys; the evaluator requires the operated radio to be the requested one | N09/N10 observations list the keys (`Tab, Tab, Up, Space`; `Tab, Tab, Down, Space`) |
| Focused radio not scrolled into view at 200% zoom (app defect) | WebKit does not scroll when arrow keys move focus within a radio group; the shared frame's fixed 60px bottom fade covered it | the desktop shell scrolls every newly focused control into view (`block: 'nearest'`); root scroll padding keeps it clear of the sticky 56px topbar and the fade | Vitest `scrolls a control into view when arrow keys move focus within a radio group`; mutation probe removing the handler fails it; N10 frame [649, 645] size [652, 88] ends above the fade |
| Last rail destination under the fade at 200% zoom (app defect) | below 900 CSS px the shared fade spans the rail; the evaluator only required "inside the window", so a 13/13 run passed while the capture showed "Settings" dimmed | below 900 CSS px the rail has 82px bottom padding and scroll padding; the evaluator now requires focused controls to end above the fade (60 CSS px × zoom) for N09 and N10 | N10 last destination [127, 611] size [414, 82], clear of the fade; applied to the superseded run the new rule fails it |
