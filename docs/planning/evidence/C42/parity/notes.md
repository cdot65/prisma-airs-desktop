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

## Judgement of the composites

Looked at side by side on 2026-10-30 (`academy-light.png`, `academy-dark.png`; prototype left, app right, both 1440 wide):

- **The frame matches the board.** The sidebar, the title bar with Search or go to… ⌘K, Explain and Ask Harness, the title row (AI Security Academy / Learn each product with your own tenant beside you), the pill at the right of the title row, the two-column card frame with the wider card on the left and the YOUR PLACE card on the right (its eyebrow, its state line, its section chips) sit where the board puts them, at the same weights and radii, in both themes. The dark theme keeps the board's surfaces and the orange accent.
- **The pills differ by design.** The board's streak and XP pills are not drawn (gamification stays in Academy); the app draws the learner's pill (Ada Lovelace signed in; Browsing as a guest otherwise). Accepted deviation, recorded above.
- **The left card is the embed, not the board's scripted CONTINUE card.** The composite shows Academy's catalog page (Learn, What will you take up next?, the search, the Paths/Courses/Labs chips, the Prisma AIRS family and its Runtime Security Path card) in the app's card where the board scripts a Continue lesson card. The embed's own typography (its 40px display heading) is larger than the board's card title; that is Academy's content (P19) and is not restyled by this chunk.
- **The Learning paths row is below the fold in the composite.** The board shows six path cards under the frame at 1440×814; the app's row is drawn (the native tree records the Learning paths heading and the Open Runtime Security Path card, checks N01/N02) but the embedded catalog is taller than the board's card, so at this window height the row starts below the captured viewport. The row's cards follow the board's card layout (title, description, progress bar, units line) with the catalog's one path instead of six. This is the one visible gap: a shorter left card would surface the row; it is a consequence of hosting the catalog page itself and is left as is.
- **No other differences** in spacing, colour or type were found in either theme.

Verdict for this gate: the chrome is at parity with the board; the two content deviations (pills, the embed in the left card) are the chunk's design; the row below the fold is noted as the remaining gap. The owner's standing direction of 2026-10-30 ("go go go until we are 100% complete, stop stopping") is why the gate did not pause for a per-screen sign-off; the owner's own side-by-side look remains invited, and an objection reopens the chunk.
