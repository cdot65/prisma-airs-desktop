# C41 parity notes — API Client (R2App start=api)

Composites: prototype board left, fixture app right, both at the window's measured web area (1440 wide on this Mac), light and dark, captured by `tools/native-acceptance/acceptance_c41.py` against the bundle in `native-prepare.json` after a collection was opened through the native sheet and its request sent to the loopback service.

The board is the API Client: the title row with the Collection and Environment chips, the 286px request tree (Find request, +, folders with counts, requests with their methods), the open requests' tabs, the request line (method, URL, Send, the save state), the Params / Headers / Body / Auth / Variables / Preview sub-tabs over the request's columns and the response pane (status, Body / Headers / Details / History) at 40% on the right, the footer. C41 ports it as new components over the workbench intents; `WorkbenchView` and the legacy editor are not imported. The C14/C15 behaviour (local-first collections, drafts protected on this Mac, Save and Send taking the fields the editor holds, reviewed tenant-credential changes, immutable history) is the legacy editor's logic, ported.

## Deviations from the board, and why

- **Collection and environment are the core's.** The prototype scripts the built-in `scm` collection and an environment that "follows the tenant"; the port shows the collection the operator opened (name, folder or built-in) and the environment chosen from the collection's own (a select in the chip), since the core's environments are the collection's files and the private ones on this Mac.
- **The Collection chip opens a menu** (Refresh, Environments…, Secrets…, Schemas…, Import…, Git…, Close collection): the prototype's chip is a label. The C14 tools have no other home on the board; they open in a 620px docked sheet.
- **+ creates a request or a folder** through a small dialog (path under the collection); the prototype's + is disabled.
- **Request rows are editable** (the prototype's are read-only): Params and Headers are tables over the file's lines with a Text toggle; Body, Auth (the `[auth]`/`[variables]` TOML) and the Schema association are edited as text; Variables and Preview are read from the core's preview of the request as typed. The prototype's Auth and Variables tables script tenant credentials; the port shows what the file and the environment hold, never a value.
- **The URL is one field** with the `{{variable}}` prefix highlighted beside it rather than inside it.
- **The save state** sits in the request line's status region (Saved / Unsaved changes / New · not saved yet, and Send's completion "<request> · <code> · <duration>"), as the C33 tests require; the prototype shows a static "Saved".
- **Response body** is pretty-printed JSON with numbered lines by default and the exact text under Raw; Copy and Save response… are live (the prototype's Save is disabled).
- **History** lists the collection's runs newest first with the status pill, the request, the time and the duration; choosing one shows it in the pane as recorded (the prototype scripts two entries).
- **Footer** says Ready / Working…, the request count and the archives kept privately rather than "Saved · Shortcuts ⌘/".
- **Narrow windows.** Below 1180px the response pane drops under the request's columns; the prototype is fixed-width.

## Owner's judgement

Recorded 2026-10-30: the owner directed the gates to proceed without a per-screen pause ("go go gog og o go gog o. stop stopping. go go go go go until we are 100% complete. stop stopping. go go go go go", then "please continue my guy", then "done. do it"). The composites are posted with these deviations for the record, and that direction is the judgement this gate runs on; the owner's own side-by-side look remains invited, and an objection reopens the chunk.
