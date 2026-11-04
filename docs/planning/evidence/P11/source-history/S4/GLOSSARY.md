# Prisma AIRS Desktop v2 domain vocabulary

These terms name the desktop product and its native authority concepts.
Mechanisms, wire formats and transition rules belong in the selected design.

## Language

**Tenant profile**: An application-owned configuration for a Prisma AIRS tenant,
including its label, endpoints, and protected credential references. New profiles
require successful management OAuth authentication before commitment; a Scan Key
can be added later. Existing profiles continue to load without being forced back
through onboarding when credentials expire or a session-only secret is lost.
A profile is not an
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

**Authority principal**: The native-established operator or restricted Harness
identity on whose behalf desktop work is requested. A claimed identity in a
request is not an Authority principal.

**Authority handle**: A principal-scoped entry to native desktop operations and
observations. Possessing an operation reference does not expand its authority.

**Intent**: A typed request for a specific desktop action under the caller's
current authority. It is distinct from a Harness proposal or an arbitrary remote
request.

**Receipt**: Native acknowledgement that an Intent was admitted, with its request
or operation reference. It is not evidence of operator confirmation or remote
completion.

**Refusal**: Native rejection of an Intent before admission under the requested
authority. A remote action's failure is a Remote outcome instead.

**ViewScope**: The product context a caller asks to observe, such as a learner
account, tenant workspace or Harness conversation. Native authority determines
which part is visible to that caller.

**Observation**: A scoped redacted view of native desktop state and operation
progress. It confers no execution or credential authority.

**ContextRef**: A native reference to the particular product context presented
to the caller. Returning to the same tenant or account does not revive retired
authority.

**Delivery session**: The current native-registered channel for one caller's
Intent deliveries. It is separate from tenant, learner or inference identity.

**Delivery sequence**: The order identity of an Intent within its Delivery session,
used to distinguish duplicate delivery from an explicit new action.
_Avoid_: Business retry, authorization token

**Native-owned payload reference**: A scoped reference to staged desktop input
whose ownership and content are known natively. It is not permission to read an
arbitrary filesystem path.

**Reviewed**: A Configuration review whose captured proposal remains eligible for
operator confirmation.

**Confirming**: A Configuration review currently awaiting the native operator's
answer for that specific proposal.

**Consumed**: A Configuration review already committed to its single admitted
operation. Failure of that operation does not make the review reusable.

**Queued**: An admitted desktop operation whose remote dispatch has not been
claimed.

**DispatchClaimed**: A desktop operation for which remote dispatch may have begun.
Local cancellation at this stage does not prove NotSent.

**Settled**: A desktop operation with a recorded terminal local result and its
current Remote outcome. Later reconciliation may refine knowledge of that outcome.

**Harness grant**: Native authority for one verified inference identity and its
approved tenant/workspace/origin context. Rotating that identity's token material
is distinct from replacing or revoking the Harness grant.

**Grant epoch**: The particular lifetime of a native grant's authority. Retired
authority stays retired even when an equivalent identity is selected again.

**One-time secret receipt**: The operator-scoped acknowledgement holding newly
issued secret material for its explicit transient reveal. It is separate from a
stored credential lookup.

**Editor lease**: The hold on the reviewed document while native operator
confirmation is pending. A changed document or context invalidates that hold.
