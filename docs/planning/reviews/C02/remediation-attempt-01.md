# C02 remediation after Codex review attempt-01

Attempt-01 (thread transport) scored architecture 8, behavior 7,
authority_security 7, validation 8, overall 7 with four required fixes and one
evidence gap. Its packet, verdict, thread reply and receipt are unchanged in
attempt-01/.

| Finding | Cause | Change | Acceptance |
|---|---|---|---|
| RF01 reset vs outstanding writes | reset purged and removed the document without waiting for admitted Keychain writes; a late write could land after the purge and its ledger entry be erased or only in memory | reset admission retires every credential binding (Keychain records into the cleanup ledger) and saves that before deleting anything; no new write can start while resetting; the reset waits for `writes_in_flight` to drain before purging, then removes the document; cleanup never deletes a record whose write is in flight | `reset_waits_for_in_flight_writes_and_retires_authority_at_admission` (held write: no purge while held, retirement saved first, write settles before purge, nothing survives); `a_failed_purge_retires_keys_durably_and_cleanup_finishes_after_restart`; probes "reset purges while a credential write is in flight", "cleanup deletes a record whose write is in flight" |
| RF02 authority during reset | admission cancelled operations but left the context reference and bindings valid; a failed reset kept the old reference | admission retires every context reference and operation (`retire_all_contexts`) and `check_context` refuses while resetting; credential reads and operation admission are `StaleContext`; success or failure issues a fresh context, so no earlier reference revives | same reset test (old and current references: reads and admissions refused while resetting); failed-purge test (old reference refused, fresh reference issued); probe "reset leaves the prior context usable" |
| RF03 committed vs uncommitted storage failure | the store renamed before a fallible directory sync and every error rolled memory back and refused | `StoreError::Unavailable` now means nothing changed (failure before the rename; memory restored, intent refused); `StoreError::Unconfirmed` means the document was replaced but the directory sync failed: the change is kept, admitted and reported (`durabilityUnconfirmed`) until a later save confirms | FileStore `failures_before_the_rename_change_nothing_and_after_it_are_unconfirmed` (injected faults on the real file store); `saves_are_refused_only_when_nothing_was_written` (refused change absent after restart; unconfirmed change kept and reported, cleared by the next confirmed save); probe "an unconfirmed (written) save is rolled back and refused" |
| RF04 partial reset reporting | purge preceded a fallible document removal, could delete some items before failing, and the UI said "Your data was kept" | failures report their stage (`credentials` or `document`) with the reason; keys are already retired (never shown as saved) and the durable ledger finishes deletion, including after restart; the UI explains exactly what was and was not done | `a_partial_purge_and_a_failed_document_removal_report_their_stage` (one of two keys deleted then failure; ledger completes; document-removal failure after a clean purge; retry completes); `reset.test.tsx` stage texts never claim keys or data were kept; probe "reset keeps credential bindings until it succeeds" |
| Evidence gap T06 interruption | the above orderings were untested | the tests above cross the production dispatch interface with deterministic held writes, partial purges, failing document removal and before/after-rename store faults, plus restart | fresh check/test/native runs and mutation probes bound to this source |

Optional suggestion taken: the snapshot validator now checks integer, non-negative
import counts and candidate indices, the endpoint review's tenant ID, the reset
failure shape and the durability flag; `snapshot-validation.test.ts` covers
malformed cases (probe "snapshot validator accepts missing import counts").

Also: an unavailable store (possibly another instance's data root) now refuses
reset instead of purging that instance's Keychain namespace
(`an_unavailable_store_refuses_reset_and_touches_nothing`).
