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
