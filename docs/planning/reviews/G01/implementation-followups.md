# Nonblocking selected-design followups

Source: actual G01 attempt05 verdict. These suggestions do not invalidate its
passing design receipt; evaluate them in the named implementation chunks and
bind any resulting source changes/tests to that chunk's independent review.

- C20/C21: assert Gateway refresh asks for write confirmation only when the
  conversation currently holds WorkspaceWrite; read-only refresh needs no prompt.
- C22: preserve the explicit MCP-only native continuity exception for current
  project access; retire old run approvals and callbacks. Test Send when a catalog
  refresh fails mid-session under the declared fail-closed remote admission rule.
- C20–C22: consider production composer suites for preserved drafts, MCP-only
  restart/no extra prompt, and SignInExpired/tools-unavailable presentation.
- C01: narrow shared-package typechecking to consumed presentation packages if
  unrelated packages add failures; record the reviewed filter/revision change.
- C03: pass and record an explicit Limits struct (8MiB request,16MiB decoded
  response,60s deadline); do not infer the response ceiling from the SDK's8MiB
  default. Qualify working-v1 large-input parity before acceptance.
