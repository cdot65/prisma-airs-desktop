# C43 parity notes — Wizard (R2Onboard, every step; mode=add)

Composites: prototype board left, fixture app right, both at the window's measured web area (1440 wide on this Mac), light and dark, captured by `tools/native-acceptance/acceptance_c43.py` per first-run step (`filled=true` on the board) and on the add sheet's Tenant step, against the bundle in `native-prepare.json`.

The board is the onboarding wizard: the 320px rail (244px in add mode) with the brand tile, the numbered steps turning to checks, the Optional/Skipped tags, the privacy note and the appearance button; the pinned footer (Cancel in add mode, Back, the hint, Step n of m, Skip for now, the primary); section cards with a 2-column field grid, the Environment segmented control, switches on the optional cards; the product cards, the Checks card, the starter-case rows, the Academy cards, the summary and next-up cards. C43 re-ports that presentation over the C29 state machine and intents, unchanged: everything durable stays in the core's setup draft, and the view only walks the steps and sends intents.

## Deviations from the board, and why

- **Required and Optional follow the core.** The prototype tags the management credentials Required and the scan key Optional; the desktop's first run requires the scan key (`scanRequired`), so its card is tagged Required on first run and Optional on the add sheet, and the management card Optional. The verify step says which products need the client.
- **Create tenant and the credential buttons** (Create tenant / Save changes / Revert, Save scan key, Save management client, Read workspaces, Use this workspace, Save workspace key) live inside the prototype's cards: the prototype fills its fields live, the desktop drafts the tenant and attaches each credential to the draft through its own intent, read once from its field and never echoed. The footer's primary names what is missing (Create the tenant to continue, Save the scan key to continue).
- **The import box** on Welcome is live (Import from v1…), and the Tenant step keeps a small link to it for the add sheet, which has no Welcome.
- **Products** are the core's four admitted products plus the two unavailable ones, with the reads-with state and the last verify result as pills; the prototype's "uses" line is scripted.
- **Verify** has no per-check timing: the rows carry the core's reason text; the banner states the run's outcome and offers Edit credentials after a failure. The rows before a run say Not run yet.
- **Starter cases** are the bundled packs for the chosen products (one per product with its count); the prototype scripts seven packs.
- **Academy** keeps the reviewed account component (Sign in to Academy through the device flow in Settings' terms) in the Your account card; the prototype's email/name fields and "Start with" paths are scripted. The right card states what is sent to Academy.
- **Done** adds the Academy row and says what was skipped; Change links return to the step when it is reachable (an unsaved profile or a stale run keeps later steps out of reach, R4/R7).
- **The appearance button** sets the Rust-owned appearance (light/dark) rather than a local theme.
- **The add sheet** is 1160 wide and 780 tall without the modal's padding (min(780px, 84vh) on short displays).

## Owner's judgement

Recorded 2026-10-30: the owner directed the gates to proceed without a per-screen pause ("go go gog og o go gog o. stop stopping. go go go go go until we are 100% complete. stop stopping. go go go go go", then "please continue my guy", "done. do it", and "no matter what autonomy I give you, you keep stopping"). The composites are posted with these deviations for the record, and that direction is the judgement this gate runs on; the owner's own side-by-side look remains invited, and an objection reopens the chunk.
