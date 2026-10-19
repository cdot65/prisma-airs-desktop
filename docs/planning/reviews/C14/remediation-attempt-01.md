# C14 remediation after attempt-01

Verdict attempt-01 scored architecture 9, behavior 7, authority/security 9,
validation 8 and overall 7 with five required fixes, no blockers and one
evidence gap.

## R1: unprotected edit generations

The editor numbers every edit. `editing { id, edit }` records the newest
generation in Rust; `protect` and `save` carry the generation their snapshot
covers and clear the close guard's mark only when it covers the newest one.
The editor re-reports editing whenever a submitted snapshot may have cleared
the mark (after a Save, and while a Protect is outstanding), so edit → Save →
edit and typing during an outstanding Protect stay guarded.
Tests: `an_older_acknowledgement_never_clears_newer_unprotected_edits`
(Rust, both orders of acknowledgement and newer editing) and "re-arms the
close guard after a save and while an older protection is outstanding"
(actual DocumentEditor).

## R2: closing before protection

`closeTab` and `closeCollection` carry `discard` and are refused with
`unprotected` while Rust holds an unprotected mark. The workbench awaits each
editor's flush (protect what is not yet acknowledged) before closing; if that
fails or Rust still refuses, it asks: Keep editing, or Close and discard
edits (which abandons the editor's pending protection). Tests:
`closing_with_unprotected_edits_waits_for_protection_or_an_explicit_discard`
(invalid text survives a restart; explicit discard) and the React tests
"protects the latest typing before a tab closes" and "asks before closing
the collection when the latest typing cannot be kept" (private-store
failure).

## R3: authoritative fields from fresh metadata

`document::open` reads method and URL from the metadata it hashes for the
baseline (and refuses a file whose identity changed); the inventory's
abbreviated method/URL are navigator text only. Test:
`documents_open_from_their_own_metadata_not_the_inventory` (a 3,000-character
URL survives a header-only Save; a file changed after discovery opens as it
is now).

## R4: the Save snapshot is protected before any write

Save seals the submitted snapshot as the document's draft before the journal
and the first project write; a failure to seal refuses the Save with nothing
written. A fully written save leaves a draft equal to its files, which the
next opening drops. The new fixture `crash_next_save` stops after a project
write without running any failure handling. Test:
`a_save_stopped_between_writes_restores_every_submitted_field`.

## R5: duplicate identities and body ownership

Discovery records every body file named by any parseable request file
(duplicates and invalid ones included); a body write is refused when another
request file names it, and a document whose file is part of a duplicate pair
cannot be saved. The collection bar now lists issues even when the inventory
is complete. Test: `a_duplicate_identity_pair_still_owns_its_body`.

## E1: Use disk natively

C14-N06 now also drives a separate conflict settled with Use disk: the disk
version is shown, the project file stays as the other writer left it, and
the collection's count of privately kept versions (now shown in the
collection bar) rises.

## Optional suggestion

Insomnia parsing has direct boundary fixtures just inside and past MAX_DEPTH
and MAX_NODES in YAML the parser otherwise accepts. The parent-path probe now
removes both of its guards (the `..` check and the final normal-component
check). Nine new probes cover R1–R5; the probe set is 40.
