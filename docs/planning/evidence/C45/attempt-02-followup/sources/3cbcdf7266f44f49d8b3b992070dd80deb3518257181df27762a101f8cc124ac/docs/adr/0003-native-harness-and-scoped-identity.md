# 3. Retain native Harness and own scoped identity in Rust

Date: 2026-10-16. Status: selected following owner candidate selection; independent G01 verdict
and implementation readiness are tracked in planning/status.md. Implements D010,D011,D014–D016,D021–D023.

Desktop-server is already a Rust engine. V1 host workers and its package manifest
still require Node/managed CLI. Preserve engine depth and replace those host
requirements, rather than rewrite the agent. Source inspection alone does not
establish that a newly sealed installed bundle works without the old resources.

Native lifecycle owns sealed executable selection, bounded stdin bootstrap,
protocol supervision, process cleanup, scoped grants and protected history.
React only presents lifecycle state. Grant/tenant/project changes retire the
run; approvals bind actual call items. Preserve existing project/process/tool
restrictions, Gateway-only inference/MCP and no engine Apply.

Identity shares native discovery/HTTP/validation/storage mechanics while retaining
Academy learner, Harness inference and MCP grant policies. They are distinct
identities with distinct sign-out/refresh/context effects. Use public-client
PKCE/device flows and a no-redirect native transport; never embed a client secret
or trust renderer-selected issuer/token destinations.

Host signature/JWKS validation is a deliberate stricter v2 requirement; v1
omitted it. Validate ID/access token types according to their actual contracts,
not by decoding arbitrary JWT claims. Rotation/expiry/sleep/revocation fail closed.
Same-identity token refresh restarts the one-shot-bootstrap engine and restores
compatible in-session history; it re-confirms workspace-write permission and
never replays remote/tool effects. Declining workspace-write confirmation keeps
the draft/cancels Send without restart. Any changed ContextBinding (including
local access/model/configuration/MCP inventory) starts a new conversation; its
old history is inspect-only under the new binding. MCP-only token rotation also
rebootstraps/restores an identical binding, but retains working v1's current
project-access confirmation without an SSO refresh or another write prompt.
Native scope additionally binds per-server MCP grant IDs/catalog revisions absent
from engine equality; re-sign-in/sign-out/removal/revision change retires the
conversation. Expiry alone leaves inference usable with that non-required server
unavailable and native calls denied. This continuity is distinct from inspect-only
imported history. No token or refresh credential enters React, diagnostics, process arguments or
inherited environment. One-time operator-entered inputs remain transient.

Use independent v2 bundle/data/Keychain namespaces. Protected work is encrypted
and account/context-scoped; no plaintext fallback. Reviewed import creates a new
v2 data generation, never modifies v1, transfers no credentials/approvals and
retains a restartable receipt/rollback generation. Historical conversations are
inspect-only until fresh-context compatibility is qualified.

Tests cross native process and identity interfaces: malformed/oversized bootstrap,
queue pressure, wrong context, crash/EOF/graceful and forced cleanup, signature/
issuer/audience/key-rotation rejection, scoped sign-out, protected histories and
Node-free installed inventory. Native/live/final signed-byte proof remain separate.
See [selected design](../architecture/DESIGN.md) for identity/crypto proposals.
