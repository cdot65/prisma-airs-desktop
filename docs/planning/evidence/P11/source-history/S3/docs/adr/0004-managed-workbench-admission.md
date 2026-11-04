# 4. Managed HTTP enters native configuration authority

Date: 2026-10-16. Status: selected following owner candidate selection; independent G01 verdict
and implementation readiness are tracked in planning/status.md. Implements D014,D016 and C14/C15 requirements.

V1's workbench has real request-document and transport depth. Its redactions-based
redirect guard is not explicit tenant-credential classification, Live mode lacks
application destination binding, and its post-401 retry can resend a mutation.
The stricter v2 review/no-replay policy must be enforced below every Send path.

Keep RequestDocument reconciliation separate. Native execution admission captures
a fresh immutable effective request and explicitly distinguishes tenant-managed,
workbench-owned and anonymous credentials. Known managed reads need capability/
destination admission. Known mutations need exact Configuration review. Unknown
managed methods/paths are denied, even if presented as a read or generic draft.
Ordinary workbench-owned/anonymous HTTP remains supported.

Replace the editable SCM collection-ID credential gate with native-owned
capability registration. Imported IDs/references cannot authorize tenant secret
resolution. An edited token_url/request URL is admitted against the explicit
credential destination before secrets are obtained; an ID change never creates
a capability. Preserve operator-reviewed endpoint configuration separately.

Bind review to expanded URL/method/headers/body and file hashes, variables,
cookies, credential/context/destination and resource classification. Recheck
before dispatch; changed input/context/files require fresh review. Rust constructs
the actual request; renderer fields cannot mark it preapproved.

Managed redirects are disabled initially: a 3xx is shown, and following it is a
new prepared/admitted request. Generic redirects preserve bounded cookie/credential
origin policy. No automatic workbench resend follows HTTP401, refresh, reconnect,
lost response, import or history replay. Refresh can precede a new explicit Send.

The built-in catalogue has 193 request specifications, 81 with mutation methods;
this is metadata parity, not a claim of dedicated/live-qualified product support.
Tests cover imported/edited requests, unknown managed endpoints, direct IPC bypass,
ambiguous paths/auth headers, same/cross-origin redirects, stale cookie/file/
variable/context bindings, double Apply and post-401/lost-response no resend.

This is a stricter v2 product contract, not an allegation that v1 failed its own
contract. See [selected design](../architecture/DESIGN.md).
