# Trial native acceptance run (retained)

Second trial of `tools/native-acceptance/acceptance_c02.py` on 2026-10-17
(07:02–07:07 CDT, fixture executable 9707e3a6…). 10/12 passed.

- C02-N07: the fixture keychain was locked from another process with
  `security lock-keychain` (the CLI then reported it locked), yet the running
  app's next scan-key save succeeded: a new record was written and the
  previous one deleted (`items_before_lock` → `items_after_unlock` in
  `native/observations.json`). A follow-up probe on the same build
  reproduced it, while the same external lock does make writes fail from a
  separate test process (`keychain_access`). The running app does not observe
  another process's lock on macOS 27, so acceptance now locks through the
  app's own keychain reference (fixture-only Fixture menu items calling
  `SecKeychainLock`/`SecKeychainUnlock`), which exercises the app's real
  locked-store path.
- C02-N10: the expectation named "Save key" while the active tenant already
  had a key, where the button reads "Replace key" (test expectation error).

A first trial (not retained) stopped early when keystrokes were dropped
while typing into a freshly focused field; the script now verifies every
typed value and retypes over a selection.
