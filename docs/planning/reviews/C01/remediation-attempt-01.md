# C01 remediation of attempt-01

Attempt-01 (thread transport, request rq-C01-20261017T010643Z-5733bda1) was an
explicitly early review with native acceptance missing. Codex scored
architecture 8, behavior 8, authority_security 7, validation 6, overall 7 and
recommended fail. Its raw verdict, packet and receipt remain unchanged in
attempt-01/. Each finding, its cause, the change and the acceptance evidence:

| Finding | Cause | Change | Acceptance |
|---|---|---|---|
| R1 run coverage | gate checked statuses/inputs only | `Manifest::plan` is the single approved plan; the gate requires exactly one run per required kind whose schema/subject/kind/run_id/path match its reference, commands equal the plan (labels and argv, in order), exit 0, executed passing tests where expected, logs present with matching hashes, and the native-prepare artifact equal to the packet executable | shell_controller `run_records_must_cover_the_exact_approved_plan`, `run_identity_exit_status_tests_and_logs_are_verified`, `run_ids_must_match_their_references` |
| R2 native contract | any nonempty all-pass list accepted; identity/bytes trusted | versioned `docs/architecture/native-checks.json` (C01-N01..N13 incl. zoom and Reduce Motion); exact ID set, no duplicates, observations, evidence bound by the packet, fixture identifier, and the prepared executable re-hashed at completion | `native_record_must_match_the_required_inventory_and_identity`, `changed_or_missing_prepared_bytes_invalidate_native_acceptance` |
| R3 transport provenance | exit/timeout not recorded; raw output only had to name the request | TransportRecord records exit_code and timed_out; thread requires a `task_complete` after submission whose final message contains JSON equal to verdict.json; fresh exec requires `turn.completed`, no failed turn and a final agent message equal to verdict.json | `failed_or_incomplete_reviewer_turns_cannot_complete`, `fresh_exec_requires_a_completed_turn_whose_final_message_is_the_verdict` |
| R4 predecessors/generations | presence of ledger lines sufficed; completion permanent | ledger entries carry generations; commits bind the completion's receipt ID; `reopen` appends invalidations for the subject and every later subject; completion requires an unrevoked latest generation with a matching commit and an intact passing receipt; packets' predecessor receipts are re-hashed by the gate | `ledger_generations_invalidation_and_receipt_integrity`, on-disk predecessor tamper/missing cases |
| R5 consumed shared source | .work and installed package not fingerprinted | the dependency fingerprint hashes the locked input subtree, the staged scratch package and the installed node_modules copy; check/native/review verify both copies equal the locked input | `edits_to_consumed_shared_source_are_detected` |
| R6 bootstrap validator | partial checks; adversarial edits tripped hashes only | the reference validator independently recomputes all four fingerprints (verified equal to xtask on the current tree), the plan, native inventory, transport/final output and predecessors; its bootstrap harness reseals hashes per mutation and requires both validators to reject each of 28 cases and accept the real receipt and a resealed control; the result is stored in the attempt's bootstrap/ directory and required by `complete` for C01 | bootstrap result for the passing attempt |
| R7 observation resources | JS handler replaced only; native tasks unbounded | `closeShell` wire scope; host registry per webview (cap 8, FIFO retirement), close on resync/stop/failed observe, retirement on page reload and window destruction; core prunes closed subscribers with a 32-subscriber cap; the facade unregisters the Tauri callback | ipc_admission `observations_are_native_owned_closable_and_retired_with_the_webview`, `observations_per_webview_are_bounded`; invoke.test `closes replaced, failed and stopped observations on both sides` |
| R8 modal focus | native navigate left the palette open | native navigation closes the palette before moving focus to the heading | desktop-shell test (palette open + native navigate) and C01-N05 |

Suggestions taken: stricter runtime validators (feature IDs, availability
shape, u64 revision bound) with tests; canonicalized confinement for shared
staging including chained symlinks; act-clean appearance test; real rustc/cargo
executables bound alongside rustup shims; precise VoiceOver wording; appearance
persistence assigned to C02 (C01 writes no user data).

Status as of 2026-10-17 01:40Z, when attempt-02 was prepared (superseded
facts are kept for history): the Academy branch was later pushed with owner
approval, a clean-checkout reproduction was recorded, and native acceptance was
completed through macOS Accessibility once the owner granted it.

Evidence gaps: the bootstrap result is now produced per attempt outside the
hash-bound evidence (see R6). Clean-checkout reproducibility depends on making
the Academy input commit reachable from an accessible source; that requires the
owner's decision to push branch `v2/desktop-presentation` to the Academy
origin (or another approved artifact source) and remains open until then.
Native acceptance remains blocked on macOS Accessibility/Screen Recording
grants or an operator-performed checklist.
