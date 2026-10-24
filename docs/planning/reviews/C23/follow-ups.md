# C23 follow-ups for the P07 push to 9/10 (D034)

C23 completed at 8/10 under D034 (896b1d2b). Codex attempt-02 scored
8/8/8/8/8 with no blockers, three required fixes (F1–F3) and one evidence
gap (G1). All are remediated in the P07 push. Each fix has a
production-interface test in `crates/desktop-core/tests/imports_rollback/
held.rs` (every intent goes through `AuthorityHandle::dispatch`, every
relaunch through `Core::start`) and mutation probes in
`docs/planning/evidence/C23/mutation-probes/probes.py` (section "P07
follow-ups"). D050 records the resulting rules.

## F1: a rollback that cannot finish keeps what the import writes waiting

- Code (`authority/import_v1_domain.rs`, `authority.rs`):
  - `v1_import_holds` is the write gate: an import or rollback running,
    or any receipt that has not settled (`applied` or `rolledBack`), or a
    receipt that cannot be read.
  - `AuthorityHandle::dispatch` refuses `busy` every intent
    `waits_for_import` lists while it holds, not only while a job runs. A
    launch rollback that fails (its owner refused) settles the activity but
    leaves the receipt unsettled, so the gate stays closed until a retried
    rollback settles it.
  - A new import (`confirmV1Import`) and Import Again wait the same way.
  - Because nothing else can write an entry an unsettled receipt names,
    treating an item that stopped while `writing` as having written its
    entries never claims another writer's work (D050).
- Test `a_rollback_that_cannot_finish_keeps_what_the_import_writes_waiting`:
  - The import stops with its cases noted `writing`, before the library
    write, into a tenant whose library is sealed.
  - It is relaunched with the v2 Keychain locked: the launch rollback cannot
    read the library and reports a problem; the receipt stays `rollingBack`.
  - With the Keychain back, a case save and a tenant creation are refused
    `busy`.
  - The retried rollback settles; then the operator's case, saved through
    the ordinary case save under v1's case name, and a new tenant go ahead,
    and the settled receipt cannot be rolled back again.
- Probe: `writes go ahead once a rollback that could not finish stops
  running`.

## F2: a paused import grants no authority over what it staged

- Code:
  - `selectTenant` refuses (`invalidInput`) a tenant an unsettled receipt
    names; the snapshot's `active` never shows one; and the import's TSG
    matches (existing targets) leave staged tenants out.
  - `openHarnessHistory` refuses a conversation an unsettled receipt names
    before reading it, `resumeHarness` refuses it, and the history view
    never publishes an opened conversation that is staged.
- Test `a_paused_import_grants_no_authority_over_what_it_staged`:
  - The import is paused after every item is written and noted `applied`,
    before the commit: a new tenant (Scratch) and a conversation in the
    operator's tenant's Gateway workspace are on disk.
  - The staged tenant is not listed; selecting it is refused and the
    operator's tenant stays active.
  - Opening the staged conversation by its ID is refused and nothing is
    opened.
  - Relaunched, the import is undone and the operator's tenant is still
    the active one.
- Probes: `a staged tenant can be selected`, `a staged conversation can be
  opened`.

## F3: a receipt that cannot be read fails closed

- Code (`import_v1/receipt.rs`):
  - Every receipt is written in two copies, `imports/<id>.json` and then
    `imports.copy/<id>.json`, each synced with its directory; a note is
    recorded only once both are.
  - `load_all` reads each receipt from its first copy, or from its second
    when the first is damaged or its folder cannot be listed. A missing
    folder holds none; one that cannot be listed is not taken for empty.
  - It reports receipts readable in neither copy by ID, and whether
    neither folder can be listed.
- Code (authority and presentation):
  - Either condition holds the write gate (F1), refuses a new import, and
    is shown: the view names each lost record, or says the folders cannot
    be read and to restore access and reopen the app.
  - Only the operator releases a lost record (`forgetV1ImportReceipt` with
    its ID, "Remove damaged record …"), which removes both copies and
    leaves what that import wrote as it is.
  - In that double fault the entries cannot be hidden, because nothing
    names them; they are held. D050 states this.
- Test `a_receipt_is_read_from_its_second_copy_and_one_lost_in_both_holds_writes`,
  each after an import paused once its first tenant is written:
  - first copy damaged: relaunched, the receipt is read from its second
    copy and the tenant removed;
  - first folder unlistable (mode 000): the same;
  - both copies damaged: the record is named, a tenant creation and a new
    import are refused `busy`, and removing the record releases them and
    removes both files;
  - neither folder listable: the view says so and writes wait; with access
    restored, the relaunch undoes the import.
- React tests: `says a record lost in both copies holds what imports write,
  and releases it only when asked`; `says when the import records cannot be
  read at all`.
- Probes: `a receipt's damaged first copy is not read from its second`, `a
  receipt is recorded in one copy`, `a folder that cannot be listed is taken
  for empty`, `a lost receipt holds nothing`, `a damaged first copy is
  reported (unit)`, `a lost record is not named`, `unreadable records are
  not said`.

## G1: the failure matrix covers these paths

The three tests above are the requested regressions: failed recovery
followed by an unlock and an ordinary write; native selection and direct
history reads of staged work; and persisted staged data with a damaged or
unreadable receipt on relaunch. They wait on observable state (the tenant
or conversation on disk, the receipt's `writing` note), not on fixed
delays, and the `imports_rollback` target passes three consecutive runs.

## Attempt-03 (R3-F1, R3-F2, R3-G1, R3-G2)

Codex attempt-03 scored 8/8/8/8/8 on the remediation above. Its findings
are remediated here.

### R3-F1: a commit binding that outlives both receipt files

- Code:
  - While a receipt is unsettled, `store_durably` also writes its copy into
    the saved document (`PersistedState.openImports`), after both files; a
    note is recorded only once that save is confirmed. Settling removes the
    copy.
  - `load_v1_receipts` takes an unsettled receipt its files no longer give
    from the document (so it is staged and rolled back) and drops a
    document copy whose files say it settled.
  - Losing the saved document makes local storage unavailable, which
    already shows and writes nothing local. So no single loss publishes
    what an unsettled import wrote.
- D050's earlier double-fault exception is withdrawn.

### R3-F2: an incomplete inventory is never taken for empty

- `receipt::load_all` lists each folder as missing, listed or unreadable;
  an entry that cannot be read makes its folder unreadable.
- An unreadable folder is stood in for only by the other one listed, never
  by a missing one (as with a receipt from before the second copy existed).
- After the launch rollback, whose writes restore its folders'
  permissions, the inventory is read again.

### R3-G1: the tests cover the double faults and their acceptance

`what_an_unsettled_import_wrote_is_never_published_whatever_its_receipt_files`
(it replaces the attempt-02 F3 test) covers:
- the first file damaged: read from the second and undone;
- both files damaged, the rollback unable to record (a directory-sync
  fault): the receipt is read from the document, the tenant is on disk but
  not listed, not active and not selectable, writes wait, and the retried
  rollback undoes it;
- neither folder listable, the rollback unable to record: the same;
- the first folder unlistable and the second file never written:
  recovered from the document and undone;
- a committed import's first file damaged: a settled receipt has no
  document copy, so this is the case that shows the second file is read
  (the unsettled case above is recovered from the document either way);
  its receipt is listed, applied, nothing is named lost and writes go
  ahead;
- a committed import's receipt lost in both files: its tenants stay shown,
  writes and a new import wait, the record is named, and removing it
  releases them (both files gone);
- a committed import's first folder unlistable with no second folder: the
  view says the records cannot be read and writes wait until access
  returns; then the record is back and writes go ahead.

Probes guard the document binding, its load merge and the listing
classification.

### R3-G2: current native evidence for the changed states

- C23-N09 (new) drives the lost-record state natively:
  - a receipt damaged in both files is named by alert;
  - "Remove damaged record …" is reached by Tab and pressed with Space;
  - both files go, the alert clears and focus moves to the Imports heading
    (the removed button had it; Forget this record does the same);
  - with neither folder readable, the view says so.
- The C01–C22 drivers run again on this attempt's C23 bundle, so every
  regression record names the executable under review.

### Verification notes for this attempt

- The probe `a receipt's damaged first copy is not read from its second`
  survived the first run of this attempt: once the document copy existed,
  the unsettled scenario recovered the receipt from the document even
  without the second file. The committed scenario above was added for it,
  and the relaunch checks in this test now poll for the state they assert
  instead of waiting a fixed 200 ms.
- C20-N10 failed once in the C20 regression run and passed on retry. Its
  log check looked for the bare text `eyJ`, which also occurs by chance in
  the random PKCE challenge, state and nonce that the fake provider logs
  with each authorization request. The failed run's request log was
  overwritten by the retry, so that cause is the likely one, not a
  confirmed one. The C20 driver now looks for a signed token (a JSON
  header followed by JSON claims), keeps the literal secret and canary
  checks, and names each logged request that holds one, so a recurrence
  can be diagnosed.

## Attempt-04 (R4-F1)

Codex attempt-04 scored 9/8/8/8/8 with one required fix and no gaps.

### R4-F1: the document's receipt copies are validated before use

- Code:
  - `PersistedState::is_consistent` (`store.rs`) now checks
    `openImports`. Each copy must pass `receipt::binding_ok`: the current
    receipt schema, a valid receipt ID, and not settled (applied or
    rolled back), since only unsettled receipts are kept there. IDs must
    be unique, and there are at most `MAX_RECEIPTS` copies.
  - `FileStore::load` already refuses an inconsistent document as
    `corrupt`, so an invalid copy is never merged by `load_v1_receipts`,
    adopted, rolled back or saved. Corrupt storage loads nothing, and every
    save is refused.
- Test `a_document_copy_of_a_receipt_is_checked_before_it_drives_anything`:
  - It relaunches through the production `FileStore`; the test writes the
    document with `FileStore::save`.
  - Each case starts from an interrupted import with both receipt files
    damaged.
  - A current copy still drives the recovery: storage is ready, the
    receipt is read from the document and rolled back, and the saved
    document no longer holds the tenant or the copy.
  - A copy of another schema, or one claiming `applied`:
    - storage reads `corrupt` and no tenant is shown;
    - no receipt is adopted, and the lost record is named;
    - a write is refused;
    - the document file is byte-for-byte unchanged and still holds the
      import's tenant, and both receipt files are untouched.
- Probes:
  - `the document's receipt copies are not checked`;
  - `a document copy of another schema is accepted`;
  - `a settled document copy is accepted`.
- D050 states the rule.
