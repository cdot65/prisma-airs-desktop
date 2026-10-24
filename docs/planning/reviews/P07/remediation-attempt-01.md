# P07 phase attempt-01 remediation

Codex phase attempt-01 scored 9/9/9/8/8, with one required fix (P07-F1) and
one evidence gap (P07-G1). Both concern C23-N08: its driver reached only
Look and Forget by Tab, but the record claimed the whole import works by
keyboard. The review established no application defect. Driving the import
by keyboard found three focus defects, which are fixed below.

## P07-F1 / P07-G1: the import, driven by keyboard alone

`tools/native-acceptance/acceptance_c23.py` adds a keyboard pass after the
existing checks. Each control is reached by Tab and operated by Space, the
arrow keys or Return. Space is pressed only once Tab has reached the named
control. Each step's outcome and the focus that follows are read from the
window:

- **Import view.** Opened from the sidebar.
- **Look for v1 data.** v1's data is found and focus stays on Look again.
- **Categories.** Tenants and Saved cases are checked with Space.
- **Scratch's target.** Its pop-up opens with Space; Skip this tenant is
  chosen with the arrow keys and Return.
- **Review what would be imported.** The review appears and focus moves to
  its heading. Scratch's items say they cannot be imported, because their
  tenant is skipped.
- **Reviewed item.** Saved cases is turned off and on with Space, and the
  Import button's count goes from 2 to 1 and back.
- **Import 2 items.** The fixture crash hook, read at launch, stops the
  import after its first item. The driver ends the app and relaunches it.
  The receipt reads "Did not finish, so it was undone".
- **Import again.** The import finishes, and focus moves to the Imports
  heading.
- **Roll back… (cancel).** The destructive sheet opens. Return cancels it,
  and focus returns to Roll back….
- **Roll back… (confirm).** Tab moves from Cancel to Roll Back, and Space
  confirms. The rollback finishes and focus moves to the Imports heading.
  Mine is kept, Prod is removed, and Scratch was never created.

AppKit reaches a sheet's buttons by Tab only when macOS Keyboard navigation
is on, and it reads that from the global preference; a per-app setting is
ignored. A trial showed this: with the fixture app's own domain set, focus
stayed on the sheet itself. The keyboard pass therefore turns Keyboard
navigation on for its duration. It records the operator's own value and
restores it afterwards (also on failure), and the record asserts the
restore. This is a C23-N08 limitation in the record.

C23-N08 now fails unless every step above passes. Its definition in
`native-checks.json` names the relaunch, the sheet's keys and the focus
moves.

Two other driver changes were needed for the longer run:

- The staged v1 keychain locks itself after a while (its default
  settings), so the v1 fingerprint read every item as missing. The driver
  now unlocks it before reading, which changes no item; with that, the
  fingerprints match.
- One receipt, not every receipt, is damaged for C23-N09, since the
  keyboard pass adds a second.

## Product: focus on the keyboard path

These are fixed in `apps/desktop/src/features/import-v1/import-view.tsx`:

- **The review.** It replaces the form whose button asked for it, so
  nothing had focus. Focus now moves to the Review heading. Cancel replaces
  the review in turn, and focus now returns to Look again.
- **A confirmed rollback.** Focus was lost when the snapshots went from
  confirming straight to settled, skipping rollingBack, which is what the
  native run showed. The view tried to return focus to the Roll back
  button, which no longer exists. A cancelled confirmation still returns
  focus to Roll back…. A confirmed one now moves focus to the Imports
  heading, like any other import or rollback that settles.

React tests (`import-view.test.tsx`):

- `moves focus to the review when it replaces the form, and back to Look
  again when it is cancelled` (new).
- `imports only ready items, states every status as text and focuses the
  result`, extended with a confirmation that is published straight from
  confirming to rolled back.

C23 probes added:

- `the review does not take focus`;
- `Cancel leaves focus nowhere`;
- `a confirmed rollback leaves focus nowhere`.

## Optional suggestions

- `docs/planning/evidence/C24/precursor.json` now records the version
  0.2.0 snapshot:
  - the source commit, 582ae068, and its review binding;
  - the hashes of the three version files and the lockfiles;
  - the hashes of the unsigned production build footprint measured.
- The phase's probe runs now write to
  `evidence/P07/mutation-probes/<chunk>/` (results, log and baselines). The
  chunk folders' historical baselines are restored to their committed
  bytes.
