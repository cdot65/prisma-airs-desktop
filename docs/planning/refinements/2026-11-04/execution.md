# Refinement execution status

Status: all four implementation streams are integrated. Shared P11 qualification
and independent scored reviews are pending. The owner-approved scope is in [plan.md](plan.md); this record makes
no stable-release or installed-acceptance claim.

Base: Forgejo main `f1e58da5cdeb1c67eed650b186081661cefc2fa2`. Isolated
`refinement/20261104-*` worktrees preserve all existing release/Linux branches.

| Stream | Current implementation | Outstanding |
| --- | --- | --- |
| C46 | Required OAuth-only authentication before new-tenant commit; optional Scan Key; no shipping CreateTenant bypass; label/environment-only Edit; source-specific warning history/rechecks and status-aware OAuth classification | Shared final focused and native checks; review |
| C47 | Header H1, PageIntro consumers, removed Ask Harness, nonmodal Explain with tenant retirement, improved sun glyph | Integrated native focus/layout/theme checks and review |
| C48 | JSON/URL token presentation, scoped footer, scrolling tabs, startup/closed/custom selection, hyphenated suggestions; redirected custom root refused | Integrated native editing/picker/status checks and review |
| C49 | Compact Settings cards, isolated Add flow, Appearance/Theme selection, old-state default and Academy isolation | Integrated native controls/theme checks and review |

Private raw evidence is retained outside the repository at the owner's queue
`refinement-20261104/evidence` and `refinement-20261104/tooling`. Public summaries
will reference final source/artifact bindings without copying private state.

Current focused results include source-bound frontend consumer regressions and
typechecks, and a private-target Rust run of tenant metadata (3), color theme (3),
collection startup/root recovery (3), response filename (1) and old collection
profile default (1): all passed, with source unchanged during the run. Original
failing regressions and the aborted shared-cache attempt remain unchanged.
These counts are separate from forthcoming integrated validation and do not
stand in for native qualification.

No real product API calls, remote IAM changes, owner credential reads, historical
seal edits or RC1 asset changes were performed for this refinement work.

The fixture candidate is built. Its executable SHA-256 is
`3239facd680ed48306306d6fc8931b9bbe164a73a59042eaf841c92c48d405a8`.
The original build receipts remain under `evidence/P11/native-build-01` and
`native-build-02`. The later API footer test-only timing fix retains this build;
an exact frontend output comparison and explicit build-input carry record the
unchanged application bytes. This does not report another native/Rust build.

Native attempts 01–03 failed before any product assertion. The owned-window
fallback was verified on the real fixture window, but background keystrokes did
not populate the draft. Their failures and zero endpoint calls are retained.
Attempt 04 is prepared for a local Terminal launch; it has not started. No native
qualification pass or independent review score has been recorded.

The separate Linux integration branch preserves its platform adapters and pins;
its typechecks and focused frontend checks passed. Its integration record is
`docs/planning/refinements/2026-11-04/linux-integration.md` on the separate
`refinement/20261104-linux` branch. Linux native/AppImage/installed acceptance remains
separate and unqualified; no Linux job or release was triggered by this merge.
