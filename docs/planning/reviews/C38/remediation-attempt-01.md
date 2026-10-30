# C38 remediation after attempt-01 (9/8/9/8/8, fail: one required fix, two evidence gaps)

Attempt-01 passes the chunk bar (8 in every dimension, no blockers); its findings are remediated before acceptance rather than carried as accepted failures.

| Finding | Remediation | Where |
|---|---|---|
| R1 [P2] the aside's MCP summary said "0 of 0" for an unread, loading or denied catalog, hid partial coverage, and silently cut the list at eight (a ninth server that was on and signed in was omitted) | The summary states the catalog's own state: "not read yet", "reading…", "could not be read: <the sanitized failure>", and for a read catalog "n of m" with "· only the first 500 servers were read" when the read was partial. The rows are the servers that are on first (every one of them is listed), then the others, eight in all; the rest are counted ("k more in the setup below"). A component test covers each state and the nine-server case. | `apps/desktop/src/v2/harness/harness.tsx`, `copy.ts` (`mcpUnread`, `mcpReading`, `mcpFailed`, `mcpPartial`, `mcpMore`), `harness.test.tsx` |
| E1 `parity/bundle.json` named the first capture's bundle while the composites came from the native script's run | The native script rewrites `parity/bundle.json` with every capture (bundle path, executable hash, the script and the time), so the record is bound to the run that produced the composites. | `tools/native-acceptance/acceptance_c38.py` (`composite`) |
| E2 the v2 setup panels (project, company sign-in, workspace MCP servers) and the saved conversations were not exercised through their v2 imports | Four component tests drive the v2 panels through the setup disclosure and the aside: the project folder (choose, a refused folder named, allow changes, remove, a refused delivery said), company sign-in (set up and saved, chosen, signed in through the browser, cancelled, a failed token named, signed out, a refused save named with nothing chosen), the MCP catalog (read, turned on, signed in and out, unsupported and stale/expired/failed servers named, the eight-server cap, partial coverage, a refused enable said) and the saved conversations (listed, opened with focus, continued, exported, deleted after asking, not continuable named, an unreadable one deleted after asking, a locked store named). The legacy `harness-view.test.tsx` keeps covering the legacy screen until C44 removes it. | `apps/desktop/src/v2/harness/harness.test.tsx` (60 tests) |

## Commands and results

Filled in from the attempt-02 run (rebuild, native, the check run and the verdict under `reviews/C38/attempt-02/`).
