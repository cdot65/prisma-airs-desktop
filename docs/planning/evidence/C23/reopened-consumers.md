# C23 reopened-consumer review ledger

C23's import writes through the owners earlier chunks built, and its
remediation of review attempt-01 (D050: the receipt commits the import,
nothing it names is shown before that, and what it writes waits meanwhile)
reaches code those chunks own. Each consumer is listed with what changed,
why its accepted behavior holds, and the evidence re-run on C23's tree and
bundle. Review-protocol.md requires that a change reopen affected consumers
rather than relying on historical scores.

None of these consumers is reopened with `xtask reopen`, which would
invalidate the reopened subject and every later completed subject. With no
import unsettled, every view and admission below behaves exactly as before,
and each is re-verified on this tree by the evidence below, which the C23
review receives.

| Consumer (owner) | What C23 changed | Why its behavior holds | Re-run on C23 |
|---|---|---|---|
| Intent admission (C01, every chunk) | While an import or its rollback runs, intents that write or export what it writes (tenants, appearance, cases, reports, the API Client, Harness history deletion and export, profile import) are refused `busy` | The check applies only while the v1 import is busy (importing, confirming or rolling back, including the launch rollback); admission, sequencing and effects are otherwise unchanged | full desktop-core suite; C01–C22 native drivers on the C23 bundle |
| The shell snapshot (C01, C02) | Tenants an unsettled import wrote are not listed; the appearance shown is the one before it | With no unsettled receipt the staged set is empty and both fields are what they were | `snapshot-validation.test.ts`, `desktop-shell.test.tsx`; C01/C02 native drivers |
| Academy offline drafts (C07) | `loadLocalDraft` answers none, and the kept-drafts list omits, a draft an unsettled import wrote for the signed-in account; the import's drafts carry the receipt's ID beside the fields Academy reads | `load_draft` returns the same fields as before; another account's drafts and every draft outside an unsettled import are unaffected | `academy_offline` suite; C07 native driver |
| Sessions reports (C11) | The open book's view omits reports an unsettled import wrote; a book that finished opening while an import (or its rollback) wrote its file gets the same change in memory, and its newest generation is written | Loads, the book's contents, Undo and writes are unchanged; a held write still lets other tenants' books open (`held_report_writes_keep_each_tenants_own_latest_changes`) | `sessions_reports` suite; C11 native driver |
| Case Library (C12) | The open library's view omits cases an unsettled import wrote; a library that finished opening while an import (or its rollback) wrote its file gets the same change in memory, and its newest generation is written | Loads, saves and their outcomes are unchanged; the change made again finds nothing to do when the library read the file after the write | `library_observation` suite and the library unit tests, the re-anchored C12 probe; C12 native driver |
| API Client recent list (C14) | `add_v1_recents` notes what it is about to add, on the API Client's lane, before the profile is written; the recent list omits collections an unsettled import added | Adopt from v1 (C14) does not use this function; the list and its indices are unchanged otherwise | `workbench_documents` and `workbench_execution` suites; C14 native driver |
| Harness history (C20) | The history list omits conversations an unsettled import saved; `Scope::ids` is visible to the crate | Saves, reads, deletion and export of history are unchanged; imported conversations stay inspect-only | `harness_identity_history` suite; C20 native driver |

C23's native regressions also changed one earlier chunk's driver:

| Consumer (owner) | What C23 changed | Why its behavior holds | Re-run on C23 |
|---|---|---|---|
| C09 native driver, C09-N07 (C09) | The check that the operator's later clipboard text is kept past the 60 s clear repeats its window, with a newly issued key, when another writer changed the pasteboard during it; only a window no other writer touched decides the check, and the record states how many windows were repeated | During C23's regressions Universal Clipboard from another device (a type marked `com.apple.is-remote-clipboard`) replaced the driver's text 13 s into the window, three runs in a row; a change-count watch showed the app wrote nothing then. The app's code and the check's pass condition are unchanged | C09 native driver on the C23 bundle |

The P07 remediation of review attempt-02 reaches these consumers too:

| Consumer (owner) | What changed | Why its behavior holds | Re-run |
|---|---|---|---|
| Intent admission (C01, every chunk) | The import write gate holds until every receipt has settled and while a receipt cannot be read, not only while an import or rollback runs | With every receipt settled and readable (always, unless an import was interrupted or a receipt lost), admission is exactly as before | full desktop-core suite; C23 native driver |
| Tenant selection and the snapshot's active tenant (C02) | `selectTenant` refuses a tenant an unsettled receipt names; the active tenant shown is never one | Tenants outside an unsettled import select and show as before | `context_credentials` suite; C02 native driver in the C24 cumulative run |
| Harness history open and resume (C20) | Opening or resuming a conversation an unsettled receipt names is refused; such an opened conversation is not published | Every other conversation opens and resumes as before | `harness_identity_history` suite; C20 native driver in the C24 cumulative run |
| The saved document (C01, C02 and every chunk that saves state) | `PersistedState` gains `openImports`, the unsettled receipts (C23's commit binding) | Serialized only while an import is unsettled (`skip_serializing_if` empty), so every saved document otherwise is byte-for-byte as before and every earlier build reads it; a reset clears it with the rest of the data | `context_credentials` store tests; full desktop-core suite |
