# C15 review follow-ups

C15 completed at 8/10 under D034 (6d78143). Codex attempt-02 scored
9/8/8/8/8 with no blockers, after attempt-01's findings were remediated
(`remediation-attempt-01.md`). These findings stay open until C15 is raised
to a passing review before the P04 phase sign-off.

## Attempt-02 (open)

- R2 (remaining): the Host/forwarding refusal reads only the editor's
  headers. A generated API-key header (`[auth] kind="api-key", name="Host"`)
  bypasses it at a registered Prisma origin, and the non-managed branch
  admits method-override headers (`X-HTTP-Method-Override: DELETE` on a
  catalogued GET) and conflicting or duplicate Authorization sources.
  Plan: admit the effective header set, editor headers plus generated auth
  headers, before secrets are resolved. Refuse Host/forwarding and
  method-override headers from either source, and refuse duplicate
  authentication at registered origins. Keep one valid manual Authorization
  and ordinary unregistered HTTP working. Add production-interface fixtures
  for a generated Host header and a method override.
- R5: a registered-origin mutation review is not bound to the
  authentication and cookies actually sent. The digest reduces Basic and
  Bearer auth to their kind (no username, no secret reference). Collection
  secrets are replaced under the same record ID with no revision. Cookies
  are read from the live jar at send time. Plan: bind the auth username,
  the secret references, each referenced secret's revision and a digest of
  the effective cookie snapshot into the review, without exposing values.
  Recheck them before the send, also while the native confirmation is held,
  and send the admitted snapshot rather than re-reading mutable state.
- Probe "tenant redirects are followed" survived because the
  registered-origin redirect guard also blocks its same-origin target.
  Plan: retarget it at an unregistered loopback destination.
