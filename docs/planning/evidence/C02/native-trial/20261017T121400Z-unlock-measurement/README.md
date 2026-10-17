# Trial native acceptance run (retained)

Third trial (2026-10-17, fixture executable 735c314e…), after locking moved
to the app's own keychain reference. 11/12 passed. C02-N07's app behavior
held throughout: the locked keychain was reported ("The Keychain is
locked."), no prompt or extra window appeared, nothing was written while
locked, and after the Fixture menu unlocked the app's keychain the save
succeeded and the retired record was cleaned up. The check failed only on
its unlock measurement, which asked another process (`security
show-keychain-info`), whose view does not reflect the running app's own
lock state (see the external-lock trial). The criterion now judges the
unlock by the app's successful save and keeps the other process's view as
information only.
