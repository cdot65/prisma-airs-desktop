# 2. Share Academy presentation through explicit composition

Date: 2026-10-16. Status: selected following owner candidate selection; independent G01 verdict
and implementation readiness are tracked in planning/status.md. Implements D004,D005,D006,D007,D010.

The current UI package re-exports web app source. Shell palette/mobile navigation
and theme persistence require tRPC/Clerk/router context; the authenticated web
entry also starts learner and manager behavior. A whole-app import would spread
that authority into unrelated desktop features.

Keep one Academy-owned presentation implementation. Create explicit provider-free
controls/theme/shell exports while preserving the existing tooling entry. Both
Academy web and desktop consume the extracted source. Composition adapters supply
brand/navigation/account/header/palette/mobile slots; the shared module owns
visual semantics, responsive layout and focus. No universal plugin framework.

Desktop global theme is an app preference stored through Rust, not hosted
settings.update. Academy web keeps its learner-account theme behavior. Learner
notifications, gamification and account effects mount only in Academy composition.

Extract learner views separately when C05/C06 require them. Retain React Query/
tRPC hooks behind a transport/navigation provider; type-only learner contracts
build without the Express/DB runtime. Web uses existing HTTP authentication;
desktop uses a terminating link into the allowlisted Rust Academy transport.
Tokens stay native. Hosted learning/access/grading/award truth and client-safe
bundle/draft contracts remain authoritative. Never bundle raw answer content.

Directly embedding the authenticated web root is rejected because its interface
contains unrelated account/manager assumptions. Maintaining a desktop UI copy
is rejected because appearance and learner behavior would diverge.

Tests render shared presentation without hosted identity/query providers, run
both consumers' regressions, verify native keyboard/focus/themes, and prove no
learner mutation/gamification effect from general desktop screens. Learner tests
cover native link envelopes/allowlist, account cache retirement and hosted authority.

Dependency/source/license pins are extraction prerequisites. This ADR changes
no Academy checkout, package or hosted registration. See
[selected design](../architecture/DESIGN.md).
