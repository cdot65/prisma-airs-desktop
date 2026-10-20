# C15 follow-ups for the P04 push to 9/10 (D034)

C15 completed at 8/10 under D034 (6d78143). Codex attempt-02 scored
9/8/8/8/8 with no blockers, after attempt-01's findings were remediated
(`remediation-attempt-01.md`). The remaining findings are remediated in the
tree re-reviewed as C15 at the P04 push; D041 and DESIGN.md's C15 section
record the rules.

- R2 (remaining), the effective header set:
  - Admission reads the headers a request would actually send: the
    editor's plus the one its authentication generates (`Authorization`
    for Basic, Bearer and OAuth 2, the API-key header's own name).
  - Host and forwarding overrides from either source are refused for
    managed and registered-origin requests, so an `[auth] kind="api-key",
    name="Host"` is `ambiguousCredential` before any secret is read.
  - At a registered origin, method overrides (X-HTTP-Method-Override,
    X-HTTP-Method, X-Method-Override) are refused, literal or generated,
    and so is a second credential: Authorization, Proxy-Authorization,
    Cookie or x-pan-token headers from any source, or an API key in the
    query.
  - One manual Authorization still goes as written, and unregistered HTTP
    is unchanged.
  - Tests:
    - the production-interface
      `generated_overrides_and_extra_credentials_never_reach_a_registered_origin`:
      a generated Host header, a generated and a literal method override,
      and two Authorization sources are refused with no secret read and
      nothing sent. A single manual Authorization is delivered, and
      ordinary HTTP with an override and a generated Host header still
      goes.
    - the unit test
      `generated_headers_overrides_and_extra_credentials_are_refused_at_registered_origins`.
- R5, reviews bound to the credentials sent:
  - The review digest binds the authentication in full: the Basic
    username and secret reference, and the Bearer and API-key references.
  - It binds each workbench secret's generation this session. Set secret
    starts a new generation before the Keychain record changes.
  - It binds the hash of the Cookie header the collection's jar gives the
    request, taken when it is planned.
  - Apply re-plans after the native confirmation, so a change made while
    the sheet is held makes it stale. A reviewed request re-checks these
    before it is sent, and its first hop carries the admitted Cookie
    header, never a later jar.
  - The context and credential gate now applies before the request itself
    is sent; a redirect's later hops follow a request already sent, so they
    are never reported as "nothing was sent".
  - Test: `reviews_are_bound_to_the_credentials_and_cookies_actually_sent`.
    A changed Basic username or Bearer reference, a secret replaced after
    review or while the confirmation is held, a new session cookie and a
    cleared jar each return `reviewStale` with no DELETE sent. Unchanged
    reviews then go once, with the admitted cookie and the reviewed Basic
    credential.
- Probe "tenant redirects are followed": the managed redirect fixture now
  points to an unregistered loopback service (`?to=`). The test asserts
  that service is never reached, so the probe isolates the managed
  no-follow rule from the registered-origin guard. Nine new probes cover R2
  and R5, and the three whose anchors moved are retargeted.
