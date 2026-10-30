# C42 parity notes — Academy (R2App start=academy)

Composites: prototype board left, fixture app right, both at the window's measured web area (1440 wide on this Mac), light and dark, captured by `tools/native-acceptance/acceptance_c42.py` against the bundle in `native-prepare.json` after signing in to the synthetic Academy, on its catalog.

The board is the Academy: the title row with the learner's pills, the 1.4fr/1fr frame and the Learning paths row of cards. C42 draws that chrome around Academy's own learner pages (the hosted embed, `@ai-security-academy/learner`, over the native learner transport) with the Rust-owned theme; the composite is judged on the chrome, the embed's content is the Academy's own (P19). `AcademyView` is not imported; the learner provider and the offline work (infrastructure, not views) are shared with the legacy destination until C44 removes it.

## Deviations from the board, and why

- **Pills.** The prototype scripts a streak and XP (Academy gamification stays in the Academy feature); the port's pills say who is learning (Browsing as a guest, or the signed-in name), whether Academy can be reached, and how many courses are kept offline.
- **The left card** is Academy's current page (the catalog, a path, a course, a unit) rather than the prototype's scripted "Continue · Unit 3 of 8" card: continuing a lesson is the embed's own content.
- **The right card** ("Your place") holds the learner's state and Academy's sections (Learn for a guest; Home · Learn · Program · Downloads · Notifications · Settings signed in) and the offline notice, instead of the prototype's scripted quick check.
- **Learning paths** are the paths Academy's catalog lists for this generation (`paths.list`, the same public query the catalog page makes), with their units done and minutes; a card opens its path page in the left card. The prototype scripts six paths with percentages.
- **Narrow windows.** The path cards reflow to as many columns as fit (the prototype fixes three); the frame keeps two columns.
- **Sign-in** stays in Settings (Open Settings), as the legacy destination does; the learner chrome (trail, status, link notice) stays inside the Academy region.

## Owner's judgement

Recorded 2026-10-30: the owner directed the gates to proceed without a per-screen pause ("go go gog og o go gog o. stop stopping. go go go go go until we are 100% complete. stop stopping. go go go go go", then "please continue my guy", "done. do it", and "no matter what autonomy I give you, you keep stopping"). The composites are posted with these deviations for the record, and that direction is the judgement this gate runs on; the owner's own side-by-side look remains invited, and an objection reopens the chunk.
