# Interview decisions

## D001 — Distribution and billing

Accepted in interview round 1, Q1.

The product should support public distribution. The owner explicitly has no
commercial requirement and expects all billing constructs to be removed.
Do not carry the earlier recommendation for future commercial flexibility into
the requirements as an accepted goal.

This is a product requirement, not a conclusion that every upstream module may
be copied or that a fork-retention condition can be removed. D008 settles
selective adaptation; individual imports still require provenance evaluation.
Preserve the learning fork's license and historical rebrand evidence.

## D002 — Functional replacement

Accepted in interview round 1, Q2.

V2 should ultimately cover every working v1 workflow, including Harness, Academy,
tenant administration, scanning, Gateway, reporting, and saved cases. Deliver
through prioritized, testable slices. Explicitly classify existing previews and
unfinished functionality rather than silently treating them as production work.

The owner proposed bringing the React Academy presentation into the application.
Round 2 clarified that this means the same remote learning interactions with
portable React presentation, rather than a rewrite of the hosted learning backend.
See D004–D006 for the clarified scope and UI direction.

## D003 — First release platform

Accepted in interview round 1, Q3.

The first supported release targets macOS Apple Silicon and ships as a signed,
notarized DMG. Intel Mac, Windows, and Linux belong to explicit later milestones.
Keep portability in the design without representing untested targets as supported.

## D004 — Academy learning integration

Accepted/clarified in interview round 2, responses numbered 4 and 5.

The current desktop app already interacts with the remote Academy. V2 should
preserve those remote interactions while sharing the existing React learner
presentation. Academy remains the source of truth for its learner presentation
and hosted learning backend. Desktop authentication and navigation need adapters;
the exact packaging/integration mechanism remains to be designed and reviewed.
Hosted grading, assessment timing, awards, and access authority remain hosted.

The response numbered 4 clarified Academy integration. It did not independently
settle the previous question about the extent of cdot-framework backend code reuse;
that decision remained open until D008 rather than being inferred from an
unrelated answer.

## D005 — Shared desktop visual foundation

Direction accepted in interview round 2, response numbered 5.

Academy's React components, themes, and shell layout are the preferred UI/UX
foundation for the broader desktop application. Verify actual reusability during
the architecture review. Do not create a parallel, diverging copy of its learner
presentation merely to achieve a shared appearance.

This direction does not approve importing the web authenticated entry point,
manager routes, browser session assumptions, or learning-specific global state
into the desktop root. The plan must distinguish shared presentation modules from
Academy feature behavior.

## D006 — Gamification is Academy-local

Accepted in interview round 2, response numbered 6.

Remove financial billing constructs from v2. Preserve the Academy's learning
gamification as part of that feature; do not apply XP, earned cosmetic credits,
shops, awards, or progression to other desktop features. Academy is one feature of
the broader application. Sharing its shell, theme, or controls does not justify
mounting its gamification state or account effects across the application.

## D007 — Desktop delivery scope

Accepted in interview round 2, response numbered 7.

Desktop is the scope of v2. No browser application or speculative web adapter is
an implementation deliverable. Do not introduce a new hosted AIRS application
merely because the presentation is React. Existing remote Academy and Prisma
product interactions remain required where their workflows need them.

## D008 — Focused Tauri/Rust desktop foundation

Accepted in interview round 3, Q8 (owner: "yes to all").

Use a Tauri Rust host with a focused Rust application core and direct calls to
`scm-sdk-rs`. Selectively adapt useful cdot-framework modules and architecture
patterns after suitability/provenance checks. Do not ship its media generation,
billing, MySQL, Redis, or inherited application server as desktop dependencies.
This settles the previously open backend foundation direction.

## D009 — First complete workflow

Accepted in interview round 3, Q9.

Prove an Academy-derived desktop shell, explicit tenant credential configuration,
a Runtime Security scan through the Rust SDK, and verdict presentation together.
Acceptance includes success, denied access, timeout, cancellation, and tenant
switching during a request. Select an accepted SDK capability/revision for this
slice; do not depend on unrelated SDK families being complete.

## D010 — No bundled Node/Bun backend runtime

Accepted in interview round 3, Q10.

The shipped application contains no Node/Bun runtime or TypeScript backend
workers, including Academy, Harness, and MCP authentication workers. React's
JavaScript runs in the WebView. Build-time JavaScript tooling is distinct from a
shipped backend runtime.

Preserve the existing native Rust Harness engine and evaluate whether the managed
CLI can be omitted after replacing host workers and resealing the package. Do not
infer that a full CLI or agent-engine rewrite is necessary. User-approved external
development tools are a separate concern, not permission to smuggle a required
backend runtime into the distribution.

## D011 — V1 coexistence and reviewed import

Accepted in interview round 3, Q11.

V2 installs alongside v1 with a separate application identity, protected credential
entries, and local data. Offer explicit reviewed imports rather than sharing
mutable state. Preserve v1 throughout validation and exercise import rollback.
Academy uses the same remote learner account and progress. D015 settles import
categories and fresh-sign-in direction; exact identity strings and history
compatibility still require specification and qualification.

## D012 — Dedicated v2 repository

Accepted in interview round 4, Q12, with exact owner-specified destination:

- Repository: `ssh://git@git-ssh.cdot.io/cdot/prisma-airs-desktop-v2.git`.
- Checkout: `/Users/cdot/development/redtail/prisma-airs-desktop-v2`.

Keep v1 intact as behavioral reference. Academy, the SDK, native Harness, and
cdot-framework remain independently maintained inputs, consumed at pinned
revisions or through reviewed shared presentation modules.

Read-only preflight found the new checkout already initialized on `main`, with
the exact origin and no worktree files. The remote advertised no branches.
Planning records were moved here from the framework checkout. This does not
authorize application implementation before the agreed planning/architecture gates.

## D013 — Scan-key-first onboarding

Accepted in interview round 4, Q13.

The first scan supports an explicit scanning API key without requiring management
OAuth credentials. Support scan-only tenants. Management credentials are separate
and added for administration workflows; a management registration does not
implicitly supply scan authentication. Additional scan credential modes must be
specified and tested when needed, not inferred from an SDK constructor.

## D014 — Rust application authority

Accepted in interview round 4, Q14.

Rust owns tenant context, credential access, request lifecycles, and permission
to apply configuration changes. React presents drafts, reviews, and results.
Configuration review binds the tenant, credentials, resource, and draft; changed
context invalidates it. Harness proposals enter the same operator review flow
and cannot authorize Apply. Initially preserve the existing Harness project/tool
restrictions; expansion would require a separate explicit design decision.

## D015 — Import work, reestablish authentication

Accepted in interview round 4, Q15.

Import selected user-authored work and settings, including saved cases, reports,
Academy drafts, API Client projects, and historical conversations. Require fresh
credential setup and sign-in. Regenerate disposable caches/downloads. Imported
conversations are inspectable history until resumption compatibility is validated;
old pending approvals and authentication sessions never transfer.

Reading protected source work for an explicitly selected import is distinct from
transferring its old authentication credentials. Define restartable import
receipts, version/account checks, rollback, and source-preservation tests.

## D016 — Explicit offline behavior

Accepted in interview round 4, Q16.

Offline mode supports local drafts, saved work, history, and valid downloaded
Academy content. Online operations require explicit retry after reconnection.
Do not silently replay configuration changes, scans, grading submissions, or
other remote actions. Preserve unknown/uncertain outcomes rather than asserting
that cancellation or disconnection undid remote work.

## D017 — Operational usage/cost administration

Accepted in interview round 4, Q17 (owner: "absolutely").

Retain Gateway usage, cost, budget, and limit views as Prisma AIRS administrative
functionality. These describe the systems the operator manages. They do not add
desktop checkout, invoices, paid subscriptions, or monetization machinery.

## D018 — Ordered capability delivery

Accepted in interview round 5, Q18.

After the scan proof, deliver Academy; Runtime/DLP/Sessions/Library; existing Red
Team/Model Security reads and API Client; Gateway; Harness/MCP; imports and final
package qualification. Split groups into small independently testable/reviewable
chunks with explicit SDK prerequisites. The implementation plan assigns C01–C26.

## D019 — Independent multidimensional review

Accepted in interview round 5, Q19.

Actual independent Claude Code review must score at least 9/10 overall and in
architecture, behavior, authority/security, and validation. No required fixes or
missing mandatory evidence. Preserve reviewed revisions/dependency pins, raw
verdicts, commands/results, remediation and review attempts. No aggregate score
compensates for a weak critical dimension. Scores are not executable evidence.
D026 changes the reviewer identity from C01 onward and adds phase reviews; the
thresholds and evidence requirements of this decision are unchanged.

## D020 — Native quality and measured budgets

Accepted in interview round 5, Q20.

macOS menus/shortcuts, keyboard navigation, focus, VoiceOver, and shared Academy
themes are release requirements. Measure startup, responsiveness, memory, and
package size. Establish named workload-specific numeric budgets during the first
complete slice before broad feature rollout. Do not invent passing measurements.

## D021 — Local redacted diagnostics

Accepted in interview round 5, Q21.

Desktop diagnostics are local/redacted with explicit export; no automatic analytics
or crash upload. Credentials/raw customer content stay out of diagnostic logs.
Protected conversation/case storage is intentional product behavior, distinct
from diagnostics. Existing hosted operations keep their normal server records.

## D022 — Stable release and preview distinction

Accepted in interview round 5, Q22.

Stable v2 requires agreed working-v1 parity, controlled live acceptance, verified
Node-free installed application, signing/notarization, coexistence/import/rollback
checks and no missing mandatory evidence. Earlier preview DMGs may demonstrate
reviewed milestones with incomplete scope clearly identified. Keep v1 available.

## D023 — Manual signed updates first

Accepted in interview round 5, Q23.

The initial release uses manually installed signed DMG updates with documented
rollback. An in-app updater is a separate later design/signing/validation milestone.

## D024 — New-source distribution terms

Accepted on 2026-10-16 through the architecture-work question:
"Apache-2.0: public source with upstream notices retained (Recommended)".
Apply Apache-2.0 to newly authored owner-controlled v2 material. Upstream code
retains its own terms/notices; this does not relicense Academy or the framework.
The product still has no billing/monetization requirement.

## D025 — Architecture selection and report rendering

Accepted on 2026-10-16: "All four: Rust authority → shared presentation → native
Harness → workbench admission (Recommended)". The owner also confirmed "Yes,
it renders" for the four candidate cards and before/after diagrams. This is
owner visual confirmation; browser automation remained unable to inspect local
HTML. Record implementation recommendations and independent review separately;
candidate selection is not a passing implementation/package test.

## D026 — Codex independent review of every chunk and phase

Accepted on 2026-10-17 by explicit owner direction at the start of
implementation: "Claude Code implements. Codex independently reviews every
implementation chunk. Codex also performs a cumulative review after every phase.
Every review must score at least 9/10 in all five dimensions."

- Applies from C01 onward. It amends D019's reviewer identity only; D019's
  thresholds, no-required-fix rule and evidence requirements are unchanged.
- Every chunk C01–C26 needs an actual independent Codex verdict scoring at
  least 9 in architecture, behavior, authority_security, validation and overall,
  with empty required_fixes, blockers and evidence_gaps.
- Every phase P01–P08 additionally needs a cumulative Codex review of the
  integrated phase and the earlier features it affects, under the same
  threshold. A collection of passing chunk verdicts is not a phase verdict.
- The implementer never scores itself, edits a verdict, pressures the reviewer
  or infers approval from a process exit status or queue acknowledgement.
- Historical G00/G01 Claude reviews remain unchanged. Their fingerprints cover
  only their recorded inputs, not this policy change. The C01 Codex review
  covers this decision and the updated reviewer guidance, prompts and schemas.

Transport is recorded per attempt. The preferred route is the owner-designated
existing Codex thread `01a141e8-0412-7b30-8ebc-db2ade105d6d`, addressed only by
that exact ID through `codex queue` (never `--last`) after a reviewer-authored
handshake file confirms delivery. Queue acknowledgement is submission, not
review. That thread runs under its own sandbox, so the read-only rule is an
instruction there; the controller enforces it by verifying the frozen source,
dependency and evidence fingerprints before and after the review. If delivery
cannot be confirmed, a fresh ephemeral `codex exec` read-only reviewer is used,
recorded as a fresh reviewer under a new attempt ID; the earlier attempt is
expired so a late answer cannot advance the gate.

## D027 — Published scm-sdk crate as the scan SDK input

Accepted on 2026-10-17 by owner direction during C01: "our rust sdk is done.
https://crates.io/crates/scm-sdk".

- From C02 the scan SDK input is the published crates.io package
  `scm-sdk = "=0.1.0"`, sha256
  `6d555d955c3734dd5ee78f7173569bdecd4426095b2ffedc4b77d8f0f2c2e8d9` (equal to
  the crates.io index checksum and recorded in Cargo.lock), built from SDK
  commit `4f65dc48686a3b701ed0585812fcca46859f7e5a` (`.cargo_vcs_info.json`), a
  descendant of the earlier source pin `ec6230d2`. It replaces that source pin
  for the scan family; later families stay gated at their owning chunks.
- Depend with `default-features = false, features = ["scan"]` and inject the
  app-owned transport; the SDK's bundled reqwest transport is not compiled.
- The pin is exact (`=`), lock-verified, and never a path or git dependency on
  an active SDK worktree. C02 runs the package's own unit tests natively from
  the checksum-verified `.crate` and its shipped Cargo.lock, separately from
  the desktop adapter contracts; it re-evaluates the scan contracts against
  this version (its error and engine code changed after `ec6230d2`) rather than
  reusing the earlier evaluation.
- Changing the version is a reviewed dependency change under the same rules.

## D028 — Academy device sign-in over the app-owned client, not oauth2/openidconnect

Decided in C04 (implementer decision, recorded for review).

- The qualification table's initial OIDC/OAuth selection (openidconnect 4.0.1,
  oauth2 5.0.0) is not adopted for C04. Academy's desktop sign-in is a public
  client RFC 8628 device flow whose endpoints come from the issuer's RFC 8414
  metadata (`/.well-known/oauth-authorization-server`, as working v1 uses),
  not OIDC discovery; it needs no ID-token validation (the account is bound
  through the learner `desktop.me` procedure, as v1 does).
- oauth2 5.0.0's device flow owns the polling loop and timing, which the
  authority must own (cancellation, supersession by flow number, revocation
  of a grant that lands after cancel), and its bundled HTTP integration
  targets reqwest 0.12, while the desktop's trust/proxy/redirect policy lives
  in the app-owned reqwest 0.13.5 client (C02).
- C04 therefore implements the small, fully bounded request/response set
  directly (`crates/desktop-core/src/academy.rs`) with the validations of
  working v1's `bridge/src/academy-auth.ts` (74754c02): HTTPS, issuer-origin
  endpoints, client ID shape, required `academy:learn` and `offline_access`
  scopes, Bearer token type, 128 KiB replies, redirects refused, transient
  5xx as network, RFC 8628 `slow_down` (+5 s).
- C20/C22 (company SSO, MCP) re-evaluate openidconnect/oauth2 for their own
  contracts; this decision covers Academy device sign-in only.

## Directions explicitly requested before round 1

- Fresh application implementation; the existing GPUI codebase is primarily a
  reference point.
- React presentation, Rust application implementation, and the new Rust SCM SDK.
- Tauri was initially acceptable if needed and is now selected by D008.
- cdot-framework informs the backend foundation through D008's selective reuse
  and architecture direction; individual imports still require evaluation.
- Plan first, architecture improvement review second, implementation and package
  qualification after that.
- Independent numeric review gates for testable chunks; earlier requirement:
  scores below 9/10 require remediation and another actual review.

## Interview exit and remaining evidence

The product/process frontier through Q23 is settled; D024/D025 subsequently
settle source terms and all four architecture candidates. Continue the requested
architecture work from those decisions. The prior extra blanket scope-confirmation
request must not prevent already authorized design/research. G01 still requires
concrete interfaces/commands and independent review before implementation.
Exact identity strings, dependency pins, import compatibility, named numeric
budgets, and designated live environments require explicit evidence/specification
at their declared gates. They are not secretly assumed complete or reasons to
repeat settled product questions. No application implementation has started.
