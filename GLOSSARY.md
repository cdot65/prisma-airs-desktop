# Prisma AIRS Desktop v2 domain vocabulary

These terms describe accepted product concepts. Concrete Rust/TypeScript
interfaces and module decomposition remain architecture-review work.

**Tenant profile**: An application-owned configuration for a Prisma AIRS tenant,
including its label, endpoints, and protected credential references. A scan-only
profile does not require management OAuth credentials. A profile is not an
Academy learner account or a Gateway workspace.

**Credential capability**: The specific operations a credential can authorize,
such as scanning, management, inference, or workspace MCP. Possessing one
capability does not establish another; configuration keeps them explicit.

**Gateway workspace**: A remote AI Gateway administrative/inference context within
the selected tenant profile. It is distinct from a local filesystem project.

**Configuration review**: An operator-visible proposal for an AIRS configuration
change, bound to the current tenant, credential context, target resource, and
draft. Rust owns its validity. Displaying or importing a proposal does not grant
permission to apply it.

**Remote outcome**: What is known about an attempted remote action: not sent,
rejected, completed as established by evidence, or unknown. Local timeout,
cancellation, and disconnection do not establish remote rollback.

**Academy feature**: The learner experience within the broader desktop app.
Presentation is shared with Academy; remote learning contracts remain hosted.
Learning progression, XP, cosmetic credits, awards, and other gamification stay
within this feature.

**Academy learner account**: The remote learning identity owning progress,
assessments, certificates, and account-scoped local drafts/downloads. It is
independent of the selected Prisma AIRS tenant and Harness inference sign-in.

**Shared presentation**: React controls, themes, and shell layout reused from the
Academy-owned source. Sharing appearance does not share learner authority,
gamification effects, manager routes, or browser-session assumptions globally.

**Harness**: The assistant feature using the native Rust agent engine and
desktop-owned lifecycle, credential grants, reviews, and protected history.
The SCM SDK is a separate dependency, not the Harness engine.

**Project scope**: The explicitly selected local filesystem scope and approved
tool permissions available to a Harness conversation. Preserve the existing
restrictions initially; a transcript is not permission to restore approvals.

**MCP grant**: A desktop-owned authorization context for a specific remote MCP
binding. Inference sign-in is separate. Discovery does not establish execution;
execution evidence must correspond to the actual approved call.

**Historical conversation**: Saved user work that can be inspected after import.
Resumption requires separate context/protocol compatibility validation. It never
restores pending approvals or old authentication sessions.

**Billing**: Desktop commercial charging/monetization machinery, excluded from
v2. Remote Gateway usage/cost/budget/limit administration remains product
functionality. Academy's earned learning economy remains Academy-local.

**Shipped backend runtime**: Executables/workers required by the installed app's
backend. Node/Bun and TypeScript backend workers are excluded. Build-time JS
tooling and React JavaScript executing in the WebView are separate concepts.

**Parity**: Coverage of agreed working v1 behavior with new v2 evidence. V1 source
implementation, retained fixture/contract results, retained live evidence, and
v2 installed-artifact results are distinct. Replacing a preview/stub with a live
feature is scope expansion, not implicit parity work.
