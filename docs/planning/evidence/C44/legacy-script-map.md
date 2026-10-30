# Legacy native scripts → their replacement drivers (C44)

Every native acceptance script under `tools/native-acceptance/` written before P09 (`acceptance_c01.py` … `acceptance_c33.py`) drove the legacy shell. After C44 the legacy shell is gone; the scripts stay in the tree because the v2 drivers import their helpers (endpoints, AX driving, capture and pixel helpers), but no gate runs them against this build: their assertions about the legacy shell no longer hold and are superseded by the v2 drivers below. The one shared setup is `v2_setup.py` (the first-run wizard, then Settings' tenant form for custom endpoints).

| Legacy script | Native checks it recorded | After C44 | Replacement driver and check ids |
|---|---|---|---|
| `acceptance_c01.py` | C01-N01, C01-N02, C01-N03, C01-N04, C01-N05, C01-N06, C01-N07, C01-N08, C01-N09, C01-N10, C01-N11, C01-N12, C01-N13 | kept (helper library: AX driver, capture, pixel helpers used by every later script) | C35-N01..N03 (the ported shell), C44-N01 (every destination) |
| `acceptance_c02.py` | C02-N01, C02-N02, C02-N03, C02-N04, C02-N05, C02-N06, C02-N07, C02-N08, C02-N09, C02-N10, C02-N11, C02-N12 | kept (tenant and Settings helpers) | C43-N01..N02 (the wizard), C35-N02 (the tenant menu), C44-N01 |
| `acceptance_c03.py` | C03-N01, C03-N02, C03-N03, C03-N04, C03-N05, C03-N06, C03-N07, C03-N08, C03-N09, C03-N10, C03-N11, C03-N12, C03-N13 | kept (scan endpoint helpers) | C39-N01 (the Playground), C40-N01 (a case run in the Playground) |
| `acceptance_c04.py` | C04-N01, C04-N02, C04-N03, C04-N04, C04-N05, C04-N06, C04-N07, C04-N08, C04-N09, C04-N10, C04-N11, C04-N12 | kept (Academy sign-in helpers) | C42-N01 (sign in from Settings, the account generation) |
| `acceptance_c05.py` | C05-N01, C05-N02, C05-N03, C05-N04, C05-N05, C05-N06, C05-N07, C05-N08, C05-N09, C05-N10, C05-N11, C05-N12, C05-N13, C05-N14, C05-N15 | kept (Academy catalog/lesson helpers) | C42-N01..N02 (the Academy chrome around the learner pages) |
| `acceptance_c06.py` | C06-N01, C06-N02, C06-N03, C06-N04, C06-N05, C06-N06, C06-N07, C06-N08, C06-N09, C06-N10, C06-N11, C06-N12, C06-N13, C06-N14, C06-N15 | kept (Academy graded workflows helpers) | C42-N01 (the embed is the Academy's own content, P19) |
| `acceptance_c07.py` | C07-N01, C07-N02, C07-N03, C07-N04, C07-N05, C07-N06, C07-N07, C07-N08, C07-N09, C07-N10 | kept (Academy offline helpers) | C42-N01 (offline pill and notice) |
| `acceptance_c08.py` | C08-N01, C08-N02, C08-N03, C08-N04, C08-N05, C08-N06, C08-N07, C08-N08, C08-N09, C08-N10, C08-N11, C08-N12 | kept (management endpoint, tenant/client helpers used by every v2 script) | C39-N01..N04 (Administration), C44-N01 |
| `acceptance_c09.py` | C09-N01, C09-N02, C09-N03, C09-N04, C09-N05, C09-N06, C09-N07, C09-N08, C09-N09, C09-N10, C09-N11, C09-N12 | kept (Runtime change helpers) | C39-N02 (the review sheet, Runtime changes) |
| `acceptance_c10.py` | C10-N01, C10-N02, C10-N03, C10-N04, C10-N05, C10-N06, C10-N07, C10-N08, C10-N09, C10-N10 | kept (DLP authoring helpers) | C39-N01 (DLP inventories in the Runtime card) |
| `acceptance_c11.py` | C11-N01, C11-N02, C11-N03, C11-N04, C11-N05, C11-N06, C11-N07, C11-N08, C11-N09, C11-N10, C11-N11 | kept (Sessions helpers) | C44-N01 (Sessions & Reports stays a framed legacy destination) |
| `acceptance_c12.py` | C12-N01, C12-N02, C12-N03, C12-N04, C12-N05, C12-N06, C12-N07, C12-N08, C12-N09, C12-N10, C12-N11 | kept (library helpers) | C40-N01..N02 (Test Cases) |
| `acceptance_c13.py` | C13-N01, C13-N02, C13-N03, C13-N04, C13-N05, C13-N06, C13-N07, C13-N08 | kept (Red Team / models helpers) | C39-N03 (the observation products' tabs) |
| `acceptance_c14.py` | C14-N01, C14-N02, C14-N03, C14-N04, C14-N05, C14-N06, C14-N07, C14-N08, C14-N09, C14-N10, C14-N11, C14-N12, C14-N13 | kept (API Client project helpers) | C41-N01..N02 (the API Client) |
| `acceptance_c15.py` | C15-N01, C15-N02, C15-N03, C15-N04, C15-N05, C15-N06, C15-N07, C15-N08, C15-N09 | kept (API Client execution helpers; the loopback service) | C41-N01..N02 |
| `acceptance_c16.py` | C16-N01, C16-N02, C16-N03, C16-N04, C16-N05, C16-N06, C16-N07, C16-N08, C16-N09, C16-N10, C16-N11 | kept (Gateway helpers) | C39-N03 (the Gateway card's tabs) |
| `acceptance_c17.py` | C17-N01, C17-N02, C17-N03, C17-N04, C17-N05, C17-N06, C17-N07, C17-N08, C17-N09, C17-N10, C17-N11, C17-N12 | kept (Gateway change helpers) | C39-N02 (the review sheet, Gateway changes) |
| `acceptance_c18.py` | C18-N01, C18-N02, C18-N03, C18-N04, C18-N05, C18-N06, C18-N07, C18-N08, C18-N09, C18-N10, C18-N11, C18-N12, C18-N13 | kept (Gateway Playground helpers) | C39-N03 (the Gateway card's Playground tab) |
| `acceptance_c19.py` | C19-N01, C19-N02, C19-N03, C19-N04, C19-N05, C19-N06, C19-N07, C19-N08, C19-N09, C19-N10 | kept (Harness engine helpers) | C38-N01..N02 (the Harness) |
| `acceptance_c20.py` | C20-N01, C20-N02, C20-N03, C20-N04, C20-N05, C20-N06, C20-N07, C20-N08, C20-N09, C20-N10, C20-N11, C20-N12 | kept (Harness sign-in helpers) | C38-N01 (the Decisions aside, sign-in panel) |
| `acceptance_c21.py` | C21-N01, C21-N02, C21-N03, C21-N04, C21-N05, C21-N06, C21-N07, C21-N08, C21-N09 | kept (Harness tools/project helpers) | C38-N01 (the project panel, decisions) |
| `acceptance_c22.py` | C22-N01, C22-N02, C22-N03, C22-N04, C22-N05, C22-N06, C22-N07 | kept (MCP server helpers) | C38-N01 (the MCP summary) |
| `acceptance_c23.py` | C23-N01, C23-N02, C23-N03, C23-N04, C23-N05, C23-N06, C23-N07, C23-N08, C23-N09 | kept (v1 import helpers) | C44-N01 (Import from v1 stays a framed reviewed destination) |
| `acceptance_c24.py` | C24-N01, C24-N02, C24-N03, C24-N04, C24-N05, C24-N06, C24-N07 | kept (privacy/diagnostics helpers) | C44-N01 (cumulative: no plaintext secret in any capture) |
| `acceptance_c25.py` | C25-N01, C25-N02, C25-N03, C25-N04 | kept (controlled live scope driver; owner-run) | the C29 live Vulture row (owner) |
| `acceptance_c28.py` | C28-N01, C28-N02, C28-N03, C28-N04, C28-N05, C28-N06, C28-N07 | kept (v2 shell helpers used by every v2 script) | C35-N01..N03 |
| `acceptance_c29.py` | C29-N01, C29-N02, C29-N03, C29-N04, C29-N05, C29-N06 | kept (wizard helpers used by every v2 script) | C43-N01..N03 |
| `acceptance_c30.py` | C30-N01, C30-N02, C30-N03, C30-N04 | kept (Inbox/guard helpers) | C37-N01..N03 (the Inbox), C36-N01..N02 (Home) |
| `acceptance_c31.py` | C31-N01, C31-N02, C31-N03, C31-N04 | kept (Administration/review helpers used by every v2 script) | C39-N01..N04 |
| `acceptance_c32.py` | C32-N01, C32-N02, C32-N03, C32-N04 | superseded by C38 (the Harness board); kept as a helper | C38-N01..N02 |
| `acceptance_c33.py` | C33-N01, C33-N02, C33-N03, C33-N04, C33-N05 | superseded by C40 and C41; kept as a helper | C40-N01..N02, C41-N01..N02 |
| `acceptance_c34.py` | — | never written (C34 was re-cut into C35–C44) | — |

## What the legacy shell asserted and where it lives now

- **The shell itself** (C01 rail, topbar, palette, Settings tabs): C35 (`acceptance_c35.py`, C35-N01..N03) and C44-N01.
- **Settings' "Use the new interface" switch** (C28, C35): removed; C44-N01 asserts its absence. The core accepts `setUiVariant` and ignores it (`ui_v2_closure`).
- **Runtime Security, AI Gateway, Red Team & Models as pages** (C03, C09, C10, C13, C16–C18): Administration's product cards (C39); the Go/palette entries open the product in Administration (`ui_v2_closure`, C44-N01).
- **Sessions & Reports** (C11) and **Import from v1** (C23): framed by the v2 shell as before; no board of their own in P09. C44-N01 opens each.
- **The Academy destination** (C04–C07): the v2 Academy chrome around the same learner pages (C42); the learner suites under `features/academy/*.test.tsx` now exercise the v2 component.
- **The API Client** (C14, C15) and **the Library** (C12): C41 and C40.
- **The Harness** (C19–C22): C38.
- **Privacy, diagnostics and network** (C24): C44-N01 repeats the no-plaintext-secret sweep over the whole-app capture set.

## Files removed in C44

`apps/desktop/src/shell/desktop-shell.tsx` (+test), `command-palette.tsx`, `icons.ts`; `features/academy/academy-view.tsx` (+test), `features/administration/administration-view.tsx`, `features/red-team/red-team-view.tsx` (+test), `features/runtime/runtime-view.tsx`, `features/runtime/overview-inbox.tsx` (+test); the `Interface` fieldset of Settings; the legacy branch of `app.tsx` and `feature-body.tsx`.

## Files kept although no v2 screen imports them

The owner's uncommitted work (docs/planning/evidence/C28/owner-work.json) touches `features/gateway/gateway-view.tsx`, `playground.tsx`, `features/harness/harness-view.tsx`, `sign-in-panel.tsx`, `prerequisites.tsx`, `features/library/library-view.tsx`, `features/workbench/workbench-view.tsx` and their tests; deleting them on this branch would collide with that work, so they and the components only they import (`gateway-files.tsx`, `gateway-observe.tsx`, the Harness panels, `document-editor.tsx`, `request-send.tsx`) stay as unreferenced code until the owner's work lands. Their tests still run.
