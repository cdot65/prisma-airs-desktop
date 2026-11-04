# P11 verification plan

This plan applies D062 to the owner-approved refinements. It does not reopen or
reinterpret historical P09/P10 results. Select checks from changed behavior,
dependencies and authority boundaries; no full Rust/native replay per commit.

| Stream | Changed boundaries | Focused proof | Affected native proof |
| --- | --- | --- | --- |
| C46 connection/diagnostics | OAuth, draft commitment, credential/context retirement, warning persistence and Inbox arrival | Real synthetic TLS issuer vs product permissions; completion bypass, cancellation, persistence and stale replies; equivalent and legacy warning rechecks; strict wire validation | New tenant with required OAuth and optional Scan Key; existing tenant warnings, source reason, recheck and retained diagnostic; correct Supply Chain arrival |
| C47 shell | Shared page heading/actions, drawer focus and tenant retirement | Header/Explain navigation, pending preference race, modal priority, all PageIntro consumers | Interactive overlay without page shrink; navigation, close/Escape/focus; representative pages at 100%/200%, light/dark and narrow widths |
| C48 API | Editable token overlays, response/history status ownership, file suggestion, collection startup persistence | Exact strings/large integers/invalid JSON; late cross-tab/collection replies; startup/close/custom-root recovery; picker suggestion vs selected path | JSON/URL input, cursor/selection/copy, Save exactly once, tabs scroll, footer/Cancel ownership, native Save Response picker |
| C49 Settings/themes | Tenant credential cards, metadata edit, additive storage preference, desktop/Academy CSS | Isolated Add draft, edit/delete target, secret cleanup; old-state defaults and alternate-theme persistence; four-palette contrast and Academy isolation | Cards/controls and keyboard navigation; both themes in light/dark; learner/portal isolation; secure/session credential controls |

Use Rust 1.99.0 and Node 24.21.0 with the locked dependency set. All local network
fixtures use generated credentials and loopback endpoints. No real tenant or
Keychain content is inspected. Genuine remote warning causes remain unknown
until separately designated read-only scope exists.

Keep immutable raw red/green outcomes and command/source bindings. A shared Rust
target exposed cross-worktree workspace metadata during fixture migration.
Those raw infrastructure failures remain recorded. Workspace outputs were removed
only from private CoW target copies; public dependency artifacts were retained
with a hash/inode check and the shared cache was left untouched. Re-establish
affected Rust results with private source-bound outputs. Frontend checks are
unaffected. This is a bounded cache correction, not a full suite rerun trigger.

Finish focused checks and cross-consumer review before building the native
candidate. Use one final fixture candidate for affected Mac checks and all P11
reviews when valid. Test doubles and DOM results do not prove native focus,
WebKit layout, picker or Keychain behavior. Linux native/platform qualification
remains separate; an available Linux runner is not product qualification.

Unchanged code paths may retain their original evidence with an explicit impact
rationale and original artifact binding. Changed bundle bytes cannot claim exact
artifact equivalence to RC1. Full end-to-end testing is reserved for release
qualification or an impact that cannot be bounded; neither a new commit nor a
review score alone triggers it. Every new assertion is accounted explicitly.

Independent chunk reviews use actual scores under D034; phase P11 requires 9
in all dimensions and no required fixes, blockers or evidence gaps. Do not infer
completion from a source test, an agent exit or a queue acknowledgement.
