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

## D029 — Desktop learner host: Academy's own pages with declared host capabilities

Decided in C05 (implementer decision, recorded for review).

- The desktop hosts Academy's learner pages unchanged and declares what its
  host cannot do through `LearnerHost` props that Academy implements and
  Academy web leaves at their defaults: `labs: 'preview'` (lab runs use
  procedures outside the desktop account allowlist; a previewed lab starts
  no sandbox and says so) and `diagrams: 'prerendered'` (the WebView keeps
  Tauri's `freezePrototype` and its style CSP, under which mermaid's
  runtime cannot load; Academy's pipeline already ships light/dark SVG
  artifacts, shown as images). Loosening `freezePrototype` or the CSP for
  a diagram renderer is rejected.
- Lesson links never navigate or open other sites from lesson content:
  opening external pages would give server-authored content a browser
  hand-off the desktop does not otherwise grant (C04 opens only the issuer's
  verification page). The learner is told where the link points.
- Procedures the shipped learner modules call but the desktop refuses are
  listed with their reason in `tools/academy/truth-graph.mjs`; a new refused
  procedure without a reason fails the C05 gate.
- C06 revisits labs (guided lab submission through the desktop attempt
  contracts) and gamification; C07 offline content.

## D030 — Graded workflows through Academy's contracts; host capabilities for what the desktop leaves to the web

Decided in C06 (implementer decision, recorded for review).

- Guided labs run through Academy's desktop attempt contract
  (`desktop.attempt.*`, `desktop.draft.*`; `LearnerHost labs="desktop"`),
  not the web lab runtime: Academy owns attempts, versioned drafts,
  idempotency and grading, and the desktop never sees an answer key.
  Labs without an authored desktop adapter (Academy's `PRECONDITION_FAILED`)
  and drawing steps are previewed.
- The desktop leaves to Academy web, by declared `LearnerHost`
  capabilities, what its account token does not carry: assessor review
  (`assessorTools=false`) and away commendation and event claims
  (`awardPresenters=false`). Each is a page that says where the action
  happens, never a control that fails.
- Learner workflows v1 kept are retained even where Academy web dropped
  their trigger (C06 review RF03): course reset (`courseReset`), a
  completed lesson's rating (`lessonFeedback`) and the avatar. The avatar
  is uploaded and read back by the core (`UploadAvatar`,
  `LoadAcademyImage`) because the WebView admits no remote network or
  images; uploads go only to reviewed destinations (Academy's object store
  or origin), and only Academy's public avatar address is read. As in v1,
  the core decodes, orients and re-encodes every photo it sends or shows
  (`image` 0.25.10, decoders for JPEG, PNG and WebP only, bounded), and an
  upload that may have reached Academy's per-user key is never reported as
  unchanged.
- Native host actions are intents with no effect on Academy state:
  `SignOutAcademy` for the account page's sign-out and `PrintPage` for
  certificate downloads (the native print sheet; "Save as PDF" is the
  system's), which reports whether the sheet opened. Public certificate links use Academy web's origin
  (`ShellSnapshot.academyOrigin`).
- Frozen built-ins stay frozen (D029): Academy modules that cannot load
  under `freezePrototype` are kept off the desktop's load path in Academy
  (lazy web lab runtimes) or replaced there (DOMPurify for the diagram
  sanitizer), not by loosening the WebView.
- C07 takes offline lab work and content; the desktop never grades offline.

## D031 — Offline work sealed per account with a lazily created Keychain key

Decided in C07 (implementer decision, recorded for review).

- Offline records are sealed with XChaCha20-Poly1305 from
  `chacha20poly1305` 0.10.1, the version v1 qualified and the one in the
  offline registry; 0.11.0 is not adopted here (no cached, reviewed
  source). The 24-byte random nonce removes nonce bookkeeping across
  restarts and crashes.
- One random key per account, in the learner Keychain namespace under a
  deterministic record (`cr-` and a digest of the account), so the key is
  found again after restart without an index on disk and reset removes it
  with the learner credentials. It is created on the first download or
  the first lab work kept on this Mac (unconfirmed lab work is kept
  shortly after each edit), never at sign-in: an account that never
  downloads a course or edits a lab adds nothing to the Keychain. Sign-out
  keeps the key with the account's locked work; reset removes both.
- Course content is kept only as Academy's bundle for the signed-in account
  under Academy's lease; it is read only when Academy is unreachable, and
  never fills a gap with content Academy did not send (media and videos are
  disclosed as needing Academy).
- Offline lab work is Academy's draft, kept by the host through a declared
  `LearnerHost` capability (`localDrafts`); Academy's versioned save and
  conflict contract decides what happens when it is sent, and it is sent
  only by the learner.
- The host closes the store on quit (`Core::shutdown`, run on
  `RunEvent::Exit`) so the per-account lock is released before a relaunch.
- Cached lab attempts carry Academy's authority with them: shown offline
  only within the lease Academy gave them and while their course is not
  withdrawn, re-read on reconnect (a read) so Academy's lock replaces an
  authored definition; locked, they keep only the learner's own work.
- Offline store work shares reset's write ledger: a retired generation or
  a reset in progress admits none, and reset waits for admitted work.
- A key is proven (sealed key check) before the store reads or changes
  anything; a wrong key never sets records aside.
- Kept records change only by results observed after them (observation
  stamps taken when a read of Academy starts); confirmed drafts move kept
  attempts forward without extending their lease.

## D032 — Owner acceptance of a review below 9/10

Decided by the owner on 2026-10-18 during C07 ("if the next review comes back
8/10, allow it and proceed. we need to push forward"); recorded by the
implementer.

- D026's threshold (9/10 in every score, no required fixes) stays the gate.
  The owner may accept a review below it: `xtask complete --owner-accept
  REASON` completes on the newest receipt only when every score is at least
  8, there are no blockers, and nothing fails other than the threshold
  itself (scores, required fixes, evidence gaps, the recommendation); the
  packet, verdict, transport, fingerprints, runs and native evidence must
  all still check. The owner widened it to evidence gaps on 2026-10-18 ("hey
  it's good enough, push on forward") after C07 attempt-05 (9/9/9/8/8, one
  evidence gap, no required fixes).
- The completion records the decision, the owner's reason, the minimum and
  every accepted failure (`owner_acceptance`); later completion checks honor
  an 8/10 receipt only with that record. Verdicts are never edited, and the
  accepted review's required fixes remain recorded in its attempt directory
  as known follow-ups.

## D033 — Management reads through the published SDK's `management` feature

Decided in C08 (implementer decision, recorded for review).

- The published `scm-sdk =0.1.0` (D027, built from `4f65dc48`) contains the
  reviewed Phase4 management, DLP and bounded-pagination commit `32cbcb6d`
  (an ancestor of `4f65dc48`, checked with `git merge-base` in the read-only
  SDK repository). C08 enables its `management` feature next to `scan`; no
  version, checksum or source changes, and the app-owned transport is still
  the only transport compiled.
- The management credential is its own class (`CredentialClass::Management`)
  beside the scan key: the client secret in the Keychain or session vault,
  the client ID in its stored credential record, the TSG from the tenant profile. The
  secret is bound to the token origin it was stored for and sent only there
  (`OriginPolicy` over the token, AIRS and DLP origins); a different
  configured token origin refuses admission. Scanning never consults it and
  management never consults the scan key.
- Production builds use Palo Alto Networks' default token, AIRS management
  and DLP endpoints only. Custom management endpoints are deferred (not
  offered in C08); the fixture build reads a management origin
  file in its own data root for synthetic acceptance only.
- Every read is one explicit runtime operation with retries off, a 60 s
  deadline and a 100-item page limit, observed once. Pages and details
  carry SDK-serialized JSON with secret fields removed recursively
  (`api_key`, `auth_code`, `client_secret`, `secret`, `token`,
  `access_token`, `refresh_token`, `password`, `private_key`,
  `api_key_value`) and unknown values kept. Completeness is claimed only
  from the service's own signal (v1's paging rules); a missing page never
  proves absence.
- Clients and their token cache live per credential generation (an SDK
  session and a lazily built binding); a replaced or removed credential, a
  tenant switch or reset cancels that generation's reads and never shows
  their results.

## D034 — Chunk reviews at 8/10; phase sign-off at 9/10 with every chunk raised

Decided by the owner on 2026-10-18 during C08 ("from now on the chunk reviews
just need to clear a 8/10, but the phases themselves must continue to achieve a
9/10 or higher ... each chunk will be pushed to achieve a 9/10 or higher before
the phase is signed off on"); recorded by the implementer. Supersedes D032's
case-by-case acceptance for chunks.

- A chunk completes on a Codex review whose five scores are all at least 8
  with no blockers, through `xtask complete --owner-accept REASON`; required
  fixes, evidence gaps and the fail recommendation may remain and are
  recorded (`owner_acceptance.decision = "D034"`). Everything else (packet,
  verdict, transport, fingerprints, runs, native evidence) must still check.
- A phase review is never owner-accepted: it needs 9/10 in all five scores
  with empty required fixes, blockers and evidence gaps (`xtask complete`
  refuses `--owner-accept` for a phase).
- Before the phase review, every chunk in the phase whose current completion
  was owner-accepted is remediated and re-reviewed until it passes the full
  9/10 gate, then completed as a new generation without reopening later
  chunks. `xtask complete` refuses a phase while any member chunk's current
  completion is owner-accepted.

## D035 — Reviewed Runtime writes with an AppKit confirmation and secret sheet

Decided in C09 (implementer decision, recorded for review).

- C09 implements exactly v1's Runtime write subset (`bridge/src/protocol.ts`
  allowed matrix at 74754c02): profiles and custom topics create, update and
  delete; applications update and delete; API keys create, version, rotate
  and delete; never force delete. v1's validation, authorship (the
  management client ID), key-creation checks (application and key names
  case-insensitively unique over a 40×25 walk, the one deployment profile's
  auth code injected by the desktop), baseline and review digests, typed
  confirmation ("CREATE" or the item name) and impact text are kept. Request
  bodies are normalized with the SDK's own request contracts
  (`scm_sdk::validation::parse`), the Rust counterpart of v1's zod parse.
- The SDK has no conditional-write precondition, so a change re-reads its
  baseline immediately before writing and refuses (not sent) when the
  review digest differs, as v1 did; a 409 is a conflict. An external writer
  can still race the final read; atomic conflict prevention is not claimed.
  Writes are never retried; a transport failure's outcome stays unknown.
  After a write the item is read back by the operation (v1 relied on the
  write reply), and the readback never changes the write's outcome.
- RequestApply opens an `NSAlert` sheet on the main window built on the main
  thread with `objc2` 0.6.5, `objc2-app-kit` 0.3.2, `objc2-foundation` 0.3.2
  and `block2` 0.6.2 (the lock's versions; DESIGN named objc2 0.6.3 before
  the pin moved). These become shipping macOS dependencies limited to the
  NSAlert, NSWindow, NSButton, NSTextField and NSPasteboard classes; WebKit
  capture stays fixture-only. The sheet's completion resolves a private,
  nonce-bound future; no wire intent can carry an approval. Destructive
  changes (delete, rotate) make Cancel the Return default.
- One-time keys (`api_key` in create/version/rotate replies; the SDK models
  it as a plain String) move into a zeroizing buffer at once and are never
  serialized to a snapshot, log or store; the native secret sheet shows one
  on request, ends itself after 60 s, and Copy and Close writes it with a
  random owner marker and the nspasteboard.org concealed/transient hints,
  clearing it after 60 s only if the marker and change count are still
  ours. The text field's own string copy cannot be zeroized; it is cleared
  when the sheet ends.
- v1's directional and customized prompt-injection options were local
  previews with no API fields; they are not part of any draft.

## D036 — DLP authoring subset with staged keyword files

Decided in C10 (implementer decision, recorded for review).

- C10 implements exactly v1's DLP write subset (`bridge/src/protocol.ts`
  allowed matrix at 74754c02): data patterns create, replace and delete;
  data profiles create and replace; data filtering profiles replace;
  dictionaries create, replace and delete. There is no filtering profile
  create or delete, no data profile delete, and the SDK's merge-patch
  methods are not used; deployment profiles and the AIRS DLP catalog stay
  read-only. Unsupported variants fail review before any request.
- The same review authority as C09 applies (fresh input, typed
  confirmation, native confirmation, baseline re-read and digest, no
  retries, faithful outcomes); DLP baselines and readback use each item's
  own get, where a 404 is absent. v1 added no authorship to DLP writes;
  neither does C10, and the SDK's DLP deletes send no body.
- Bodies are normalized with the SDK's request contracts; data profiles
  first drop the response-only `advance_data_patterns_rule_request` and
  null fields and must pass v1's `validDataProfile` (ported exactly, UTF-16
  name length). Dictionaries are exactly `{metadata, file}` with the file
  as non-empty text of at most 64 Ki UTF-16 units (v1), metadata checked by
  `DictionaryRequestSchema` and an `original_file_name` the SDK's multipart
  rules accept; uploads are the SDK's multipart (`json` metadata part,
  `file` part named by the file name, text/plain).
- v1 entered keywords only as text. C10's milestone asks for file
  selection, so a dictionary may also use a keyword file chosen in a native
  sheet (rfd/NSOpenPanel; the fixture build reads a file staged in its data
  root). Rust reads it once (bounded to 256 KiB, then UTF-8, a byte-order
  mark dropped, non-empty, at most 64 Ki UTF-16 units), never writes it,
  and stages it for the active context under a random token; a draft
  refers to it as `fileRef`. The renderer never sees a path or the content;
  a different file chosen after review changes the draft Apply compares
  and is refused.

## D037 — Sessions investigations, the protected cache and metadata-only reports

Decided in C11 (implementer decision, recorded for review).

- Sessions are read through the tenant's management binding (C08) with the
  SDK's raw dashboard reads, leniently as v1 read them: the inventory is the
  last 24 hours (`time_interval=1`, `time_unit=day`) in pages of 25; a
  session is one page of 25 actions over 30 days ("at least 25" when full);
  a transaction and a scan's content are read by scan and sub-request.
  Secret fields are redacted and prompt/response text is removed from every
  read except Content, which runs only on an explicit reveal.
- Searches keep v1's classification and bounds (`bridge/src/runtime.ts`): a
  session ID walks 30 days in pages of 100 for at most 40 pages; a scan ID is
  one scan report whose metadata (never content) is returned, a 404 proving
  absence; a detector walks the last day for at most 1,000 sessions and
  stops after the page that reaches 100 matches. Completeness is claimed
  only when the window ended (and, for a detector, with no more than 100
  matches); the presentation words each result by what was read.
- The investigation cache belongs to one credential generation and never
  authorizes anything: every read, cached or not, first binds the current
  credential. It keeps v1's 50 entries per category and 4 MiB snapshot
  bound. For a Keychain credential it is sealed (XChaCha20-Poly1305, AAD
  bound to the credential) under a random key in the credential namespace
  whose record is derived from the credential's record; the sealed file and
  key are removed with the credential through the cleanup ledger and by a
  reset. A session credential's cache stays in memory. Unlike v1, the
  snapshot is a 0600 file rather than a Keychain item (one key item per
  credential, any size); a snapshot that does not open is discarded. Clear
  empties it; Export writes its session and transaction metadata only.
- FP/FN reports keep v1's model, limits, upsert/remove/undo and CSV. Unlike
  v1, a new report is built in Rust from the scan's action row in this
  credential's investigation (the operator supplies only kind, category and
  reason, the category being one of v1's quick picks), so a report can
  never carry content or invented metadata; an existing report can be
  edited without its session. Reports are a 0600 JSON file per tenant
  (named by a digest of the tenant ID), loaded off the lock, written
  newest-generation-only, removed with the tenant and by reset. Exports go
  through the native save sheet (rfd/NSSavePanel, owner-only atomic write;
  the fixture build writes into its data root without replacing a file).
  Nothing is written to the tenant.
- Exception Patterns stays v1's inert synthetic preview.

## D038 — Case Library storage and Runtime Home/Inbox scope

Decided in C12 (implementer decision, recorded for review).

- The case Library keeps v1's model and rules exactly (`settings.rs`,
  `case_library.rs` at 74754c02): five products, 32 cases and 256 KiB per
  tenant, v1's field limits, Runtime cases needing a profile and application
  and never a payload, specifications never a profile or application,
  `expected` a free-text goal that is never compared with a verdict (v1
  never compared it), export file version 2, imports of version 1 or 2 for
  the same tenant merged by identifier with nothing added past 32, the
  bundled catalog's eleven examples and the catalog's version, kind,
  deduplication and URL rules.
- v1 kept a protected tenant's cases in a Keychain item and a session
  tenant's in memory. v2 tenants have no tenant-level storage mode, so every
  tenant's library is sealed on this Mac (XChaCha20-Poly1305, AAD bound to
  the tenant) under a random key in the credential Keychain namespace whose
  record is derived from the tenant; it is removed with the tenant and by
  reset, written newest-generation-only by the local-data writer that tenant
  deletion and reset also take. A library that does not open is set aside,
  never overwritten. Imports, catalog imports and copies are saved when
  accepted (v1 kept imports in memory until the next save; v2 has no
  separate save step). A Runtime case saved from the scan form opens the
  library if needed and is added when it opens.
- v1's only case actions are kept (create, load, inspect, delete, import,
  export, catalog import, copy); there is no edit or duplicate. Loading a
  Runtime case or starter fills the scan form in the presentation and never
  scans. Copy takes any case (v1 copied only the last) and always makes a
  new case in the destination.
- The public catalog is read once on request through the app transport
  (HTTPS, 15 s, at most 256 KiB, no redirects, no credential) from the
  configured catalog URL, which must pass v1's URL rule; fixture builds point
  it at a local snapshot server (C12's "approved fixture/catalog snapshots").
- Home/Inbox is Runtime-only here, shown as the Runtime destination's
  Overview & Inbox section (no new navigation entry): one overview read takes
  the last day's sessions chart and the API key inventory's first page,
  summarized by v1's rules without fabricated zeros (unknown benign or
  detector counts stay unknown), Inbox items (key expiry within 30 days
  inclusive, Runtime activity), gaps, freshness (10 minutes) and Needs
  attention faces and holds (acknowledge until worse, snooze 24 h, restore;
  memory only, like v1). A refresh in which every source fails keeps the
  last read, marked with the failure. Automatic reads happen only on
  opening when nothing fresh is held and not within ten minutes of a failed
  read; v1's idle polling, the encrypted metric history and baselines, and
  every Gateway, Red Teaming and Supply Chain source belong to C16-C18.
- P03 push amendments: a Runtime case never carries a payload (refused on
  import, catalog and load; v1's validator allowed one, but v1 authoring
  never produced one); the library file is named after its key record so the
  durable credential cleanup ledger removes a deleted tenant's key and file
  together; library operations report success only when their write
  settles, failures go to the operation's originator and `SaveLibraryAgain`
  retries; a deadline ticker publishes freshness and snooze transitions.

## D039 — Red Team and Model/skill observation scope and reads

Decided in C13 (implementer decision, recorded for review).

- Only v1's live reads are reproduced (74754c02, `bridge/src/observability.ts`
  and the Red Teaming/Supply Chain Overview pages): AI Red Teaming scan
  statistics for a 7, 15 or 30-day window (`date_range=LAST_N_DAYS`, default
  30), the first 100 scans (`skip=0&limit=100`), a scan's attack results 100
  per page up to ten pages (1,000), Model Security scans
  (`skip=0&limit=100&isBackgroundRefresh=false`), AgentGuard 30-day skill
  statistics (`time_period=30_DAYS`) and skill scans. Every other v1 Red
  Teaming and Supply Chain tab was a preview or stub that sent nothing; v2
  shows no control for them and states that scans, ASR verification,
  targets, adapters, channels, prompt sets, model scans, security groups and
  model sources are not available. ASR is never computed: attack results
  count only rows the service rated as succeeded (`threat`).
- The reads go through the pinned SDK's Red Team, Model Security and
  AgentGuard clients (features `red-team`, `model-security`, `agentguard`)
  built from the tenant's management OAuth client, with the same token, origin
  policy, limits and no retries, and the tenant header only where the SDK's
  family sends it (AgentGuard). They use the SDK's raw read on those clients
  at the typed operations' paths so v1's tolerance applies: the typed models
  reject a missing total (statistics, AgentGuard lists) that v1 read as zero
  when nothing contradicted it. Endpoints default to the SDK's
  (`api.apps.paloaltonetworks.com` `/ai-red-teaming/{data-plane,mgmt-plane}`,
  `/aims/{data,mgmt}`, `/aiag/{data,mgmt}`); v1's bridge used its own SDK's
  defaults, and C25 qualifies the live hosts.
- v1's rules are kept: a reply reporting a failure (`success: false`, an
  `error` or `errors` field) is invalid, and statistics must carry one of
  their known fields; a missing total is zero only when every reported
  count, totals and every breakdown entry or rated target, is zero
  (AgentGuard: both counts zero and no top vulnerability), otherwise the
  reply is invalid; a 404 for statistics is "no scans" only when a one-row
  scan list is complete, empty and totals zero; a list is complete only
  when the count is not capped and the total is no larger than the rows
  actually received, or without a total the page is short, and a total
  below a page's rows is invalid; partial and capped counts are lower
  bounds, and a capped total says so ("≥ N"); reported rated-target
  groups are validated (arrays of at most 64 objects with whole,
  non-negative counts; an absent or null group is unreported, where v1
  rejected null) before a source counts as quiet, and a list needs its pagination object
  and a boolean `count_capped` when present, so a malformed page never
  proves "no scans"; breakdown labels from v1's
  list for each breakdown are upper-cased with `_` read as a space and any
  other label becomes OTHER (more than 64 entries make the reply invalid);
  each source keeps its own state and a read in which no source answered
  fails. (P04 push: these were tightened after the C13 review.)
- Privacy follows v1's worker seam: only identifiers, statuses, target
  types, outcomes, counts and times leave the core. Scan, target, model and
  skill names, model URIs, security groups, findings (including AgentGuard's
  top vulnerability), attack prompts, replies and unknown fields never do. A
  skill is shown by the first eight hex digits of SHA-256 over a per-read
  random salt and its name.
- Opening a scan's attack results reads its pages one after another until
  the service's total is covered, a page comes back short, a read fails or
  the ten-page cap is reached (v1 read two pages and offered Load more for
  the rest); each page is the same bounded read carrying the rows received
  before it, from which the core judges coverage, and closing the results
  stops the walk. "No attacks" is shown only for a complete read.
- Reads happen when a tab is first shown with nothing held, on Refresh and
  on a window change; the view is keyed by context (a tenant or credential
  change starts with nothing held), keeps the last read dated when a refresh
  fails and drops a superseded reply. v1's idle polling, encrypted metric
  history, cached snapshots across sessions and Home tiles for these
  products are not reproduced here (C16-C18 own Home/observation history).

## D040 — API Client projects and documents (local-first authoring)

Decided in C14 (implementer decision, recorded for review).

- **Format.** v1's portable project files are kept byte-compatible: a
  collection is a folder with `collection.toml` (`format_version = 1`, `id`,
  `name`, `[variables]`), `*.request.toml` request files (`method`, `url`,
  `[[headers]]`, `[[query]]`, `body`/`schema` sidecar references,
  `body_kind`, `[auth]`, `[variables]`), shared `environments/*.toml` and
  JSON/YAML schema text. Edits go through `toml_edit` 0.25.17 (already in the
  lock), so comments, row decoration, extension keys and CRLF survive a Save.
  A new collection is created only inside a Git repository (v1).
- **Inventory.** Discovery is read-only and bounded (1 MiB metadata, 2,000
  requests, 20,000 entries, depth 24, 512-byte paths; a directory is read
  no further than the entry budget, and an unreadable entry makes the
  inventory incomplete); it never follows a
  link, enters `.git` or a nested collection; invalid files and every copy of
  a duplicate identity are reported and left out, and an incomplete
  inventory refuses every write (v1). Writes are atomic and refuse links,
  `.git`, nested collections, absolute, hidden-on-create and `..` paths.
- **Revisions and fresh input.** Rust holds each open document's baseline
  (the SHA-256 of every file it read) and names it by a revision. Save,
  Protect and Preview carry the editor's full snapshot and the revision it
  was edited from; a different revision is refused (`stale`, the editor
  reloads), a snapshot that cannot be written faithfully is refused with the
  field and row ("Fix to save") and nothing is written, and files that
  changed since the baseline are a conflict, never overwritten. A body is
  written only when no one else owns the file it reaches: not another
  request's body, not a schema a request associates, and not a project file
  (request files, `collection.toml`, environments, standalone schemas; a
  `.json` or YAML file a request names as its body is that request's body,
  so the built-in catalogue's JSON sidecars save), compared by the file itself (the same device and
  inode) or, for a file not yet written, its case-folded name, so case or
  Unicode-normalization aliases on the Mac's volumes are refused
  (`sharedBody`; P04 push, attempt-02 R5 and attempt-05 R6). The editor
  captures the fields it holds at the click (not a debounced copy) and keeps
  text typed while a save was in flight, protecting it against the new
  revision. A Save whose snapshot equals the saved version is `unchanged`
  and supersedes any older protected draft: the draft is removed and that
  removal is stored before the Save is acknowledged, so a restart never
  recovers it (P04 push, attempt-04 R1).
- **Saves and interruption.** A save writes its journal (each file's before
  and after hash, sealed) first, then the body, then the metadata, then
  clears the journal. On opening, a journal whose files hold neither the old
  nor the new content marks the request interrupted: Save is refused until
  the operator chooses Use disk or Keep mine. Nothing is written again by
  itself after a restart.
- **Conflicts.** Use disk and Keep mine archive both versions privately
  first (at most 16 per collection); Keep mine rebases the edits onto the
  files on disk and the next Save writes them.
- **Private profile.** Recent collections (8), the value-free secret index,
  the built-in collection's write record and, per collection, tabs, the
  selected environment, private environments, protected drafts (64),
  archives and the journal are sealed (XChaCha20-Poly1305) under one key in
  the credential Keychain namespace in the app's `api-client` folder; reset
  removes both, never the operator's collections. Drafts are protected 400
  ms after typing pauses and when leaving a document; a closed tab keeps its
  draft (v1).
- **Environments and secrets.** v1's precedence (collection defaults, private
  `_defaults`, the shared environment, the private environment of the same
  name, the request's `[variables]`) and `{{name}}` one-level substitution are
  kept for the preview, which masks every secret and lists unresolved
  variables and secrets without a value. Workbench secret values are entered
  in a secure field and stored as Keychain records derived from the
  collection and reference; `tenant.*` references are reserved for native
  tenant authority and are never stored, resolved or sent here.
- **Imports.** cURL (v1's lexer and option list; credentials become secret
  references; credential-named query, JSON or form fields are refused) and
  Insomnia v5 / schema 5.1 exports (YAML or JSON through `serde_yaml_ng`
  0.10.0, bounded to 1 MiB, depth 64, 50,000 nodes and 16 MiB expanded;
  scripts, template expressions, path parameters and non-default settings
  refuse that request). Imported requests are unsaved drafts; imported
  environments are new shared files and never replace one; the source is
  only read.
- **Git view.** Read-only branch, HEAD, upstream and changed paths from an
  absolute `git` (never PATH or the `/usr/bin` shim), with an empty
  environment, no system or global configuration, fsmonitor and hooks
  disabled, no optional locks, 5 s and 1 MiB. v1's hosting links and saved
  diff are not reproduced here.
- **Profile adoption.** v1's API Client profile (recent collections and
  their private environments) is previewed and imported on request; the
  source is only read; secret values, drafts, history and cookies do not
  transfer (D015). v1's separate "Import Rusty profile" migration is not
  reproduced.
- **Built-in collection.** v1's sanitized catalog (193 request
  specifications, `src/api_client/scm/catalog.json` at 74754c02, SHA-256
  914a9ee7…037a8c) is written to the private folder and opened as an
  ordinary collection; a refresh replaces or removes a file only while it is
  exactly as last written. These are generic specifications, not dedicated
  workflows or live qualification.
- **Close guard.** Quitting (the app's own Quit item, since AppKit's
  predefined one bypasses the event loop) or closing the window first waits
  300 ms for intents already sent, then, while edits are not protected,
  asks in a native sheet (Quit anyway / Cancel). File > Save (Cmd+S) is a
  native menu item, because WKWebView receives Command shortcuts only
  through menus.
- **Not done here.** Nothing is sent: execution, history, cookies,
  downloads, OAuth tokens and admission of managed requests arrive in C15.

## D041 — API Client execution and managed-request admission (ADR 0004)

Decided in C15 (implementer decision, recorded for review).

- **Fresh input, Rust-built requests.** Send and Apply carry the editor's
  full snapshot and the revision it was edited from (a different revision is
  refused, as Save). Rust renders it against the current environment layers
  (v1's precedence, one-level `{{name}}`, `{{{` escape; unresolved, unclosed
  and nested names refused), builds the actual request itself and sends it;
  the presentation never supplies a prepared or approved request. Bodies are
  v1's text, JSON (application/json unless set), form, multipart (64 MiB per
  file, 128 MiB in all, an explicit Content-Type refused) and stored files
  (64 MiB), streamed from the collection.
- **Credential classes.** Decided from the secret references the rendered
  request would use, never from a collection or request identity:
  anonymous, workbench (Keychain secrets of the collection) or managed (a
  native tenant credential). Managed references are `tenant.client_id`,
  `tenant.client_secret`, `tenant.api_key` and `tenant.tsg_id` (filled in as
  text); any other `tenant.*` reference is refused, and workbench secrets
  never ride with tenant credentials. The built-in collection now names
  these instead of v1's per-tenant `tenant.select` environments.
- **Admission before any secret is read.** The OAuth client pair goes only
  into an `oauth2` grant whose token URL is exactly the management
  capability's token URL (or the catalogue's `/auth/v1/oauth2/access_token`
  at the same, stored token origin); the scan key only as the `x-pan-token`
  header to the tenant's stored scan origin. Requests with tenant
  credentials use HTTPS, an admitted API origin (the configured management
  endpoints plus `api.sase.paloaltonetworks.com`, the catalogue's host) and
  no Authorization, Cookie, x-pan-token or method-override headers of the
  operator's. Their method and path must be one of the 193 built-in
  catalogue operations (variables are single path segments; encoded
  separators or dots, empty, dot and parameter segments are refused before
  normalization). Known reads are sent; known mutations (81) need a review;
  any other method or path is denied, whatever it is presented as.
- **Registered Prisma origins** (attempt-01 R1, R2). The configured
  management, token and workbench API origins, the AIRS scan regions and
  every tenant's scan destination are registered by host and port with or
  without an active tenant or ready credential. Requests there are
  classified whatever their credential: a manual Authorization header, a
  collection secret or none still needs a review for a known mutation, and
  any unknown operation is denied. Generic redirects never enter a
  registered origin (the 3xx is the result). URL user-info and Host,
  Forwarded, X-Forwarded-Host, X-Original-URL and X-Rewrite-URL headers are
  refused for managed and registered-origin requests before any secret,
  whether the editor wrote them or the authentication generates them (an
  API-key header's name). At a registered origin a method override
  (X-HTTP-Method-Override, X-HTTP-Method, X-Method-Override) and more than
  one credential are refused as well. Credentials are the editor's
  Authorization, Proxy-Authorization, Cookie or x-pan-token headers, the
  request's authentication (whatever header it generates, or an API key in
  the query) and the collection jar's Cookie header for the URL. The jar is
  read when the request is planned, before any secret, and the Cookie it
  gives is the one the first hop sends. One credential still goes: a
  manual Authorization, or the session cookie alone (P04 push, attempt-02
  R2 and attempt-03).
- **Context binding** (attempt-01 R3). Every context change retires the
  pending review permanently; Apply also checks that its context is the
  active one; managed or reviewed requests re-check their context and the
  tenant's credential records before secrets are read, before a token is
  minted and before the request is sent, and end `retired` (nothing sent)
  otherwise; a redirect's later hops follow a request already sent.
- **Credential binding** (P04 push, attempt-02 R5). A review is also bound
  to the credentials it sends: the authentication in full (Basic username
  and secret reference, Bearer and API-key references), each workbench
  secret's generation this session (replacing or removing a secret starts
  a new generation before its Keychain record changes) and the hash of the
  Cookie header the collection's jar gives the request. Any change, also
  while the native confirmation is held, makes Apply stale and sends
  nothing; a reviewed request re-checks them before it is sent and carries
  the admitted Cookie header on its first hop, never a later jar.
- **Reviews.** A known change returns a masked review bound to the tenant,
  client and capability records, context, catalogue operation, the exact
  expanded method, URL, query, headers, auth and body (secrets by
  reference, files by content), the variables used and the request
  identity. Apply re-renders the same snapshot against the current files
  and environment, must reproduce the digest, takes the native AppKit
  confirmation (destructive for DELETE), is consumed with admission and
  sends once (each body file pinned to the content it was matched with: a
  file that changes while streaming never sends its last piece); a changed snapshot, file, environment or context, a forged,
  expired or consumed review, or a declined sheet sends nothing.
- **Transport.** The native transport policy's proxy and reviewed roots;
  no redirects or retries inside the client, no automatic decompression,
  HTTP allowed for ordinary requests; a 10 s connect and 120 s idle limit.
  Fixture builds send to loopback only (literal loopback addresses, or
  `localhost` pinned to 127.0.0.1) without any proxy; production sends
  anywhere. Redirects: never followed with tenant credentials (the 3xx is
  the result), within the origin with workbench secrets, up to 10 hops
  otherwise; credentials and cookies never cross origins.
- **Wire credentials are redacted** (attempt-01 R4): response headers and
  redirect targets lose the secrets used and their wire forms (minted
  bearer tokens, Basic encodings) before display or history; response
  bodies stay intact in sealed storage, as downloads need.
- **No replay.** OAuth tokens are minted after admission (Basic client
  auth, RFC 6749 2.3.1 encoding, no redirects, 30 s, 64 KiB), cached in
  memory, dropped after a 401; nothing is ever resent by itself: not after a
  401, a lost response, a timeout, a cancel, a restart or from history.
  A mutation without a response is shown as possibly delivered.
- **Storage.** Responses stream into sealed frames (1 MiB,
  XChaCha20-Poly1305 under the profile key, bound to the run, position and
  last frame) in the private folder, 512 MiB at most; 256 KiB is kept for
  display. History is a sealed index per collection (200 runs, 30 days,
  1 GiB of bodies) recorded as running before sending, so a stop is found
  as interrupted. Save response writes through the native save sheet beside
  the destination and renames, never into the private folder, `.git` or a
  non-file. Cookies: one jar per collection for requests without tenant
  credentials; host-only (no public-suffix list is needed: a Domain other
  than the responding host is refused, so no cookie reaches a sibling or
  parent host, narrower than v1), Secure over HTTPS only, 1,000 cookies,
  4 KiB per Set-Cookie, persistent ones sealed (8 MiB). The `cookie` 0.18.2
  parser was already locked.
- **Concurrency.** At most four requests run at once, off the workbench
  lane (editing continues), each cancellable; reset cancels them and forgets
  tokens, cookies, reviews and histories. Reset also retires them for good
  (P04 phase attempt-01 R1). Each send is planned in a reset epoch that
  reset advances when it is admitted. Every private-storage step of a send
  holds a shared fence and runs only while its epoch is current: history
  and cookie loads and writes, the response body file, and body pruning.
  Reset holds the fence exclusively from before its purge to its end, so
  a step either finishes before the purge or never runs. A retired send
  neither recreates the private folder, its key, history, bodies or
  cookies, nor republishes its record; it ends `noCollection`. A send's
  epoch is the one it was admitted in, sampled before its first await and
  kept through planning (P04 attempt-02 R1), so a reset during planning
  retires it too: its jar is not loaded, and no review is published and
  no run registered for it.
- **Workload.** The large HTTP bodies and downloads budgets were registered
  before implementation (`evidence/C15/workload-budget.md`, 290a069).
- **Not done here.** The catalogue's 193 requests are specifications;
  live qualification of any operation is C25.


## D042 — Gateway workspace and configuration reads

Decided in C16 (implementer decision, recorded for review).

- **Dependency.** desktop-core enables the pinned `scm-sdk =0.1.0` crate's
  `gateway` feature (it implies `models` and `iam`). Nothing new is
  resolved: the only Cargo.lock change is `getrandom 0.4.3`, already
  pinned, listed among scm-sdk's dependencies; the crate's checksum is the
  one D033 verified.
- **One credential.** Gateway reads use the tenant's management OAuth
  client (the SDK's `AIGatewayClient` over the management binding's
  client; v1 used the same client). The data, admin and IAM planes are the
  SDK's defaults at the management origin (`/ai_gw/v2`,
  `/ai_gw/admin/v2`, `/iam/v1`); fixture builds place them at the
  synthetic origin, and the binding's origin policy admits exactly them.
  The scan key and workbench secrets are never consulted, and no Gateway
  inference credential exists yet (C18).
- **Reads advertised** (each through the SDK's typed operation; replies
  meet its contract, so an inventory without `total` is invalid where v1
  tolerated it): workspaces (data plane, active ones as the service
  returns by default) and a workspace; within the chosen workspace
  providers, routes (configs), guardrails, service and user keys (each
  `?workspace_id=`) and their details; tenant-wide integrations (admin),
  an integration's models; the workspace's IAM scope. Deferred: rate- and
  usage-limit policies, telemetry, logs, cost and pricing (C18), admin
  workspace listing and provisioning (C17), MCP servers and integrations
  (C22), secret references, IAM scope listing and every write.
- **Projections.** Items leave the core only as v1's allow-list
  (`projectGateway`): base scalar fields, limits' scalar fields, defaults
  and string scopes. Provider keys and model configuration, key values,
  masked keys, integration secrets and every credential-bearing routing or
  guardrail value stay in Rust; a route shows its public document, whether
  it carries credentials and where (paths and shapes, never values); a
  guardrail with credentials shows only that it has them. v1's secret-name
  rule is used; separators are ignored, which only widens it.
- **Completeness.** At most 200 rows. `has_more` decides when present;
  otherwise a list is complete only when its total is no more than the
  rows received. A total below the rows, a repeated ID or a scoped row
  naming another workspace makes the reply invalid. A partial inventory is
  shown as part of a larger one and never as empty.
- **Details belong to the chosen workspace.** A reply must carry the asked
  ID; a workspace item naming another workspace is invalid; one naming
  none is shown only when the workspace's inventory lists it (absent only
  from a complete inventory, otherwise nothing is claimed).
- **Workspace choice is native.** A Workspaces listing settled under the
  current tenant context records the workspaces; `SelectGatewayWorkspace`
  admits only a listed one and issues a `WorkspaceRef` (`gw-`), saved per
  tenant (a failed save refuses and leaves the previous choice). Scoped
  reads carry the reference, are admitted only with the current one and
  read the workspace it names (never one the presentation supplies), under
  the reference as their operations domain: choosing again retires them,
  and a result settled under an earlier choice is withdrawn when observed.
  A tenant context change (switch, credential replacement, reset) clears
  the listing and the choice; the remembered workspace is restored only
  when a listing under the new context lists it, and a complete listing
  without it clears it and says so. A choice never touches the tenant
  context, so Runtime reviews stay protected.
- **Prerequisites.** One read of the chosen workspace's routes, providers,
  service keys and the tenant's integrations, with the IAM scope named by
  the workspace: each source keeps its data or its sanitized failure; the
  scope binds the workspace, does not, is absent, is denied or failed;
  each plane is readable, not allowed or failed; the count of service keys
  with `completions.write`. The management client is never offered for
  inference, and identity administration is not provided.

## D043 — Reviewed Gateway changes and provisioning outcomes

Decided in C17 (implementer decision, recorded for review).

- **One change path.** Gateway changes reuse C09's reviewed-change flow
  (ReviewChange, RequestApply, the native confirmation, single-use apply,
  one-time secrets) with seven Gateway kinds and a `models` action; their
  rules are `gateway_changes`. No second review mechanism exists.
- **Matrix (v1 → SDK `=0.1.0`, each the SDK's typed call, retries off).**

  | Kind | Create | Update | Delete | Other |
  |---|---|---|---|---|
  | Workspaces (admin) | provisioning: IAM scope, workspace, binding | `workspaces().update` | archive (`delete`, soft) | — |
  | Providers, routes, guardrails (data) | `create` in the chosen workspace | `update` | `delete` | — |
  | Service and user keys (data) | `create_service`/`create_user` | `update_*` | `delete_*` | `rotate_*` |
  | Integrations (admin, tenant-wide) | `create` (no credentials) | `update` | `delete(id, tsg)` | `set_models` |

  Deferred: integration credential entry (`key`, `secret_mappings`) to a
  protected native input, integration workspace bindings, v1's protected
  routing-credential merge, rate/usage-limit policies (C18), MCP (C22).
- **Scope is the core's.** A change inside a workspace carries the
  selection's `WorkspaceRef`; the core admits it only with the current
  reference and places a create there (`workspace_id`), refusing a draft
  that names another (`workspaceMismatch`). An item must belong to the
  chosen workspace (its `workspace_id`, or its complete inventory). A
  workspace's own update or archive needs a listed workspace; provisioning
  and integration changes are tenant-wide. The review is bound to the
  reference: choosing another workspace ends it and its native sheet. The
  review and the confirmation name the workspace, its slug and IAM scope,
  or the IAM scope a provisioning creates or reuses.
- **Organisation.** Key, guardrail and integration creates belong to the
  management credential's tenant (`organisation_id`), filled by the core;
  another is a contract problem.
- **Credentials never travel in a draft.** Integration keys and secret
  mappings, and any credential-bearing field (v1's secret-name rule) are
  refused before any request (`credentialPolicy`). A change that would
  replace or lose stored credentials (a route document, guardrail checks
  or integration configuration that holds some) is refused after the read
  (`protectedConfiguration`).
- **Validation.** The SDK request schemas (strict); provisioning names its
  IAM scope explicitly (a generated name would differ between review and
  apply) and the review checks it is absent, or present when reused;
  guardrail checks must name a catalog evaluator and meet its parameter
  schema (v1 `validateParameters`; references, combinators, conditionals
  and patterns fail closed), and a Prisma AIRS check needs one usable AIRS
  connector; an integration's configuration meets its provider family's
  concrete schema.
- **Baseline and conflict.** The review reads the item (unprojected, kept
  in the core) and binds tenant, client, kind, action, target, body,
  workspace, references and the item without traffic counters and server
  timestamps (v1 `stableConfiguration`). Apply re-reads all of it; any
  difference is a conflict, nothing sent.
- **References block deletes (v1 `deletionImpact`).** Providers and
  guardrails: routes naming them; routes: other routes, keys and the
  workspace defaults; integrations: providers in every listed workspace
  and the workspaces it is granted to; workspaces (archive): any route,
  provider, guardrail or key. The review lists them and Apply is refused
  while any remain; an inventory that cannot be read completely refuses
  the review.
- **Write and readback.** Exactly one write. A one-time key (`key`) is
  taken into a zeroizing buffer before anything else, then the reply is
  stripped of credential-bearing fields. Readback by identifier (the
  reply's for a create) as the allow-listed projection; absent from a
  complete inventory after a delete or archive. A routing document must
  read back as reviewed under a new version, and model enablement (and the
  all-models setting, when the draft sets it) as set; otherwise the
  readback `differs` (never claimed applied as reviewed).
- **Provisioning keeps every step.** The SDK's bounded `provision`: scope
  create (unless reused), workspace create, binding; never retried. Each
  step reports not started, not sent, rejected, unknown, done, existing
  or deleted (a scope created for a workspace that was definitely refused
  is deleted by the SDK; an uncertain create keeps it). That cleanup is
  reported (not needed, kept, deleted, refused, unknown); a reused scope is
  never this attempt's to remove. The outcome is unknown when any step is,
  completed (partially) when something it created remains, otherwise the
  failure's. Afterwards the tenant is read back: this attempt's workspace in
  the admin inventory, by the identity the service returned or else the one
  workspace with the requested name and scope that did not use a reused
  scope before (read just before the write); another workspace sharing the
  scope is never attributed to the attempt, and when none can be told apart
  the result says so (`ambiguous`). Then the IAM scope, and whether it lists
  that workspace. Only a provisioning whose every step completed and whose
  workspace, scope and binding all read back is summarized as verified.
- **A workspace switch retires waiting applies.** Each apply shares its
  progress with its record. Choosing another workspace (or losing the
  selection or the tenant context) marks every change bound to another
  reference that has not dispatched its write as retired, under the lock
  the write's dispatch takes: it settles not sent (`workspaceChanged`)
  without writing, even if the operator chooses the first workspace again
  (a new reference). A write already dispatched keeps its own outcome.
- **Stopping is not rollback.** `CancelChange` stops an applying change of
  the active context: before its write it is not sent; after an answered
  write it keeps that outcome and any one-time key, with its readback
  `stopped`; with the write unanswered, unknown, and a provisioning is read
  back as above. Nothing sent is undone. (C09 changes share this.)
- **T23: work kept on this Mac.** Each tenant's Gateway drafts and uncertain
  results are kept in one owner-only file under the sessions root (removed
  with the tenant and by a reset). The workbench keeps edited text once
  typing pauses, labelled by the core with the workspace a review would
  apply to. Only text that reads as a complete document is kept, and its
  credential fields are refused as in review (at any depth, escaped or
  not): text still being typed stays in the editor, and the last complete
  text is what is kept. A change that sent
  something drops its draft; one that settled unknown, partially, or
  without a confirming readback is recorded with the reviewed text, its
  outcome, readback and steps. Neither carries a review, confirmation,
  reply or key: reviews, confirmations, results and keys remain memory
  only. After a restart (offline too) both are read and drafts edited
  locally; records say the service has not been read since. Reconnecting
  sends nothing; a retry is a fresh review of the text under the workspace
  chosen from a fresh listing, and a record goes only when dismissed. A
  change whose tenant is left or whose credential is replaced while its
  write may be in flight leaves the screen with its context (its review,
  reply and key withdrawn, never replayed) but is still recorded under its
  own tenant and workspace when it settles. Nothing is kept for a tenant
  that was deleted. A reset drops every book, queued settlement, pending
  read and write; reads take their turn with writes, and every book change
  takes a new generation from the authority's counter, so work admitted
  before a reset finds nothing to read or write after it.

## D044 — Gateway inference, observation and Home/Inbox

Decided in C18 (implementer decision, recorded for review).

- **Dependency.** No new dependency or SDK feature: the pinned `scm-sdk
  =0.1.0` `gateway` feature already provides `RuntimeClient`, the
  telemetry, usage- and rate-limit clients and `ContractStream`. Cargo.lock
  is unchanged.
- **Inference key.** A third credential class, `Inference`, holds one
  workspace inference key per tenant. It is entered (masked) in the
  Playground and passed once to the core, like the management secret. It
  is bound to the workspace chosen when it was stored (named natively by
  the current `WorkspaceRef`, never by the presentation) and to the
  runtime endpoint entered with it: an `https` URL with a host and no
  credentials, query or fragment (v1's rule). It is stored in the Keychain
  or for the session, with the same write-ahead ledger, cleanup, tenant
  deletion and reset as the other classes. The binding (workspace and
  endpoint, never the key) is saved with the tenant. Storing or removing it
  is not a tenant-context change: it stops only the run that used the old
  key, and reviews, reads and the workspace choice stay. The management
  client is never offered for inference, and the inference key is never
  offered for management.
- **Runs.** `RunInference` admits one Playground run under the current
  context and workspace reference with the key stored for that workspace.
  One run is admitted at a time. It is the SDK's streamed chat completion:
  v1's shape (system and user messages, `stream: true`, `max_tokens`
  256), with the route as `x-portkey-config` when one is chosen and a fresh
  `airs-` trace as `x-portkey-trace-id`.
  - **Bounds.** At most 32 KiB of input and 64 KiB of output; once the
    output reaches its bound, the stream is closed and the run is
    `truncated`. A stream event may be at most 64 KiB; the stream may be
    idle for at most 30 s; the whole run may take at most 120 s.
  - **No retry, no replay.** Retries are 0 and replay stays the SDK's
    default `Never`. A failed route never falls back to a direct model.
  - **Binding.** The run's operation lives in the workspace reference's
    domain. Choosing another workspace or changing the tenant context stops
    it; replacing or removing the key does too. Its output is shown only
    while it is still bound, and a late chunk of a retired run is never
    shown. A key replacement or removal withdraws the run from the very
    snapshot that admits it, before its cancellation has finished; its end
    is still recorded (below).
  - **Outcomes.** Not sent; rejected; completed (the Gateway answered, even
    if a later chunk was malformed); unknown (stopped or lost after it was
    sent). When the stream cannot be opened, the outcome is the SDK's own
    `RemoteOutcome`, never one derived from the failure's label: a 4xx is
    rejected (446 named `blocked`), while a 5xx, a lost handshake or a
    malformed successful one is unknown, since the Gateway may have run it.
    Stopping ends the stream on this Mac. It never establishes that nothing
    was charged.
  - **Publishing.** Snapshots follow a streaming run at most every 40 ms.
- **Observation reads.** These are scoped Gateway reads
  (`GatewayRead::Observe`) through the SDK's telemetry and policy clients
  over the management client, for the chosen workspace (its slug for
  telemetry, its ID for policies). Each source keeps its data or its
  sanitized failure, and a read fails only when no source answered.
  - **Usage** (1, 7 or 30 days): the requests, cost, tokens and latency
    series (at most 2,048 points each) and the busiest models (at most 100
    of at most 200 rows).
  - **Logs:** a 25-row page, at most 40 pages, filterable by trace or
    status. Every row must name the chosen workspace and match the filter,
    and IDs are unique. The last page that may be read, with more reported
    (or a full page without a total), is `capped`: the list says more exist
    and never "all read". A further page continues only the query of the
    pages shown (its own window and filters, never the form's edited ones),
    and a reply for any other query or an earlier read is dropped.
  - **Limits:** the usage- and rate-limit policies naming the chosen
    workspace.
  - **Totals.** A total the service did not report is unknown. It is zero
    only for an empty, complete answer. A window over the service's quota is
    partial and claims no total.
  - **What is never shown.** Log metadata, user identities, request URLs
    and policy conditions stay in the core. Limit policies are read, not
    changed.
- **Home and Inbox.** An overview read also reads the chosen workspace's
  last day: requests, errors and cost, plus how many policies apply. It is
  labelled with that workspace, and none is read when no workspace is
  chosen. Errors in the day make a Gateway Inbox item: bad at one error in
  ten, otherwise a warning. A failed or partial Gateway source, or a day
  whose requests or errors were not reported, is a gap, so the face is
  never "All clear". A Gateway item's hold (acknowledged or snoozed) ends
  only when a read of its own workspace established both the requests and
  the errors and no longer reports it: a read of another workspace, or an
  unknown day, keeps it. An overview whose Runtime sources failed but whose
  Gateway answered is kept.
- **Work kept on this Mac (T23).** The tenant's Gateway book also keeps:
  - one Playground draft per workspace (model, route, system text and
    prompt; saved as typed, within the run bounds);
  - the record of every run that was sent and did not complete (stopped,
    or failed after it was sent): workspace, model, route, trace, how it
    ended, its outcome and token counts, never its output.
  Both are listed under "Kept on this Mac" (offline too). A kept draft is
  edited there by its local ID (`SavePlaygroundDraft` with the draft's
  `id` and no workspace), which needs no listing or workspace reference
  and so works offline. A pending edit is kept when the form loses focus
  or closes, not only after the pause in typing. Nothing is replayed on
  relaunch or reconnect, and a run is always an explicit press from the
  chosen workspace with its stored key.
- **Not done here.** File operations beyond D045's subset, the model catalogue
  (`/models`), log exports, audit logs, pricing, limit-policy writes and
  entity resets are not provided. Spoken VoiceOver output is not recorded.
  No real endpoint is called before C25, and no live inference budget is
  used.

## D045 — Playground APIs and Gateway files: chat completions, Responses and a bounded file subset

Decided in C18 remediation (implementer decision for the C18 milestone
"supported response/file workflows", recorded for review). Corrected after
review attempt 2: an earlier version of this decision said the pinned SDK
could not upload files. That was wrong. `RuntimeClient::create_file`
(`scm-sdk-0.1.0/src/airs/gateway/runtime_audio.rs`, multipart `POST /files`,
compiled under the `gateway` feature this build already enables) uploads a
file, so the file requirement is met here with a bounded subset.

The pinned SDK's `RuntimeClient` and v1 were compared operation by
operation. v1's Playground streamed chat completions only
(`bridge/src/gateway-stream.ts`, `createChatCompletion`). It had no
Responses or file workflow.

| Operation (pinned `scm-sdk 0.1.0`, `RuntimeClient`) | v1 | C18 |
| --- | --- | --- |
| `create_chat_completion_stream` | Playground | Supported (v1 parity). |
| `create_response_stream` | None | Supported: a Playground run with the Responses API, optionally attaching one listed file. |
| `create_response`, `create_completion(_stream)`, prompt completions/renders, embeddings | None | Not offered: the Playground streams, and v1 had none of them. |
| `get_response`, `list_input_items`, `delete_response` | None | Not offered: Responses runs are sent with `store: false`, so the Gateway keeps no response to read back, continue or delete. |
| `list_files` | None | Supported: the chosen workspace's files (metadata only; the first 200, more reported). An upload adds its file at the top and keeps the bound: a file pushed past it is reported as more. A creation time no date can render is shown as unknown, not refused. |
| `create_file` (multipart) | None | Supported: one file chosen in the native sheet, at most 4 MiB, purpose `assistants`. |
| `delete_file` | None | Supported: one listed file, after an in-app confirmation. The answer counts only if it names that file: `deleted: true` completes, `deleted: false` is a refusal, and an answer naming another file is uncertain (kept with its trace, the file still listed, never resent). |
| `retrieve_file` | None | Not needed: the list already gives each file's metadata, and the upload's answer gives the new file's. |
| `download_file` (content) | None | Not offered: the app never reads a file's content back (retention below). |
| Purposes `batch`, `fine-tune`, `vision` | None | Not offered: this app has no batch, fine-tuning or image workflow to use them. |
| Vector stores and their files, batches, fine-tuning jobs | None | Not offered: no v1 workflow and none in the plan. |

- **Responses runs.** These follow D044's rules for runs: the same key,
  endpoint, workspace binding, trace and route headers, bounds, single
  send, outcomes and records. The request is `input` (the prompt) with the
  system text as `instructions`, `max_output_tokens` 256, `stream: true`
  and `store: false`. With a file attached, `input` is one user message of
  an `input_text` (the prompt) and an `input_file` (the file's ID). The SDK
  ends the stream at the completed, incomplete or failed event. Text and
  refusal deltas are output. The completed or incomplete event gives the
  response's ID, served model, token counts and end. A failed or error
  event ends the run as failed, named `modelFailed`; its outcome is
  completed, since the Gateway answered. Tool, audio and search events are
  not asked for and add no output.
- **Cut short (both APIs).** A Responses `response.incomplete`, or a chat
  finish of `length` or `content_filter`, ends the run as `incomplete`:
  the Gateway answered, so the outcome is completed and nothing is retried
  or kept as uncertain. The status says the answer is incomplete and why
  (the sanitized reason, for example `max_output_tokens`), and the partial
  output, tokens and trace stay shown. It is never shown as "Completed."
- **Choice.** The Playground offers the API as a choice (Chat completions
  by default). It is kept with the draft, shown with the run and kept in
  an uncertain run's record.
- **Files: binding.** Every file request uses the workspace inference key
  and goes only to the runtime endpoint stored with it, admitted through
  the same check as a run (current context, current workspace reference,
  the key stored for that workspace). One file request runs at a time, in
  the reference's operations domain, so another workspace or context stops
  it. Replacing or removing the key withdraws the files view at once and
  drops a chosen file. Requests carry a fresh trace header and no route.
- **Files: choosing and uploading.** A file is chosen in the native sheet.
  Rust reads it once, up to 4 MiB (a larger or empty file is refused with
  its reason before anything is sent), and holds it in memory under a
  random token. The presentation sees only its name and size, never its
  path or bytes. One upload takes the chosen file; uploading again needs a
  new choice. The multipart name drops the characters a part name may not
  carry.
- **Files: outcomes.** Each request is sent once and never retried. A
  failed request keeps the SDK's finding (`remote_outcome`): a refusal (4xx)
  changed nothing; a 5xx, a lost answer, or an answer that is not a file
  may have stored or deleted it. An upload or delete with an unknown result
  is kept on this Mac (tenant book, D044) with its action, file name, size,
  file ID (for a delete), trace and failure. It is never sent again and
  is shown offline until dismissed; reading the files shows what exists.
- **Files: attachment.** A Responses run may attach one file, and only one
  that the files last read for that workspace with that key listed. The
  Playground offers those files; a draft keeps the chosen file's ID, and a
  run with a file no longer listed (another key, another workspace, not
  read since a relaunch) is refused before anything is sent.
- **Files: retention.** An uploaded file stays in the Gateway until it is
  deleted, here or elsewhere. The app never deletes one by itself, keeps no
  copy of its content (not in the tenant book, logs or snapshots) and never
  downloads one. Deleting asks first and names the file and workspace.

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
