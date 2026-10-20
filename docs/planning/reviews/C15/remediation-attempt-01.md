# C15 remediation after attempt-01

Verdict attempt-01 scored architecture 8, behavior 8, authority/security 7,
validation 8 and overall 7 with four required fixes, two evidence gaps and
no blockers. D041 and the DESIGN C15 section record the decisions below.

## R1: registered Prisma origins whatever the credential

`Destinations.registered` now holds every registered Prisma origin by host
and port: the configured management, DLP, Red Teaming, Model Security,
AgentGuard and workbench API origins, the token origin, the four AIRS scan
regions and every tenant's scan destination. It is built natively for every
Send, with or without a context and whether or not a credential is ready.
A request there without tenant credentials (a literal Authorization header,
a collection Keychain secret or none) is classified against the catalogue:
known mutations need the same exact native review, unknown operations and
ambiguous spellings are denied. A generic redirect is never followed into a
registered origin (the 3xx is the result). Ordinary HTTP elsewhere is
unchanged. Tests: `registered_origins_are_classified_whatever_the_credential`
(unit, with and without tenant destinations) and
`registered_prisma_origins_are_classified_whatever_the_credential`
(integration: manual, collection-Keychain and bare DELETEs need review with
and without a context; unknown and plain-HTTP spellings denied; the manual
review confirmed is sent once; a 307 into the origin is not followed).

## R2: Host overrides and URL user-info

Managed and registered-origin requests refuse URL user-info and Host,
Forwarded, X-Forwarded-Host, X-Original-URL and X-Rewrite-URL headers (any
case) as `ambiguousCredential`, before any secret is read or token minted.
Tests: `host_overrides_and_url_user_info_are_refused` (unit) and two new
cases in `tenant_credentials_are_admitted_before_any_secret_is_read`
(nothing read, nothing sent).

## R3: context retirement covers workbench sends

Every context reissue (tenant switch, credential replacement) and reset
retires the pending review permanently (its ID joins the consumed set).
Apply's review check also requires the review's context to be the active
one. A managed or reviewed request re-checks, through `still_admitted`, that
its context is active and the tenant's credential records are the ones it
was planned with: before its secrets are read, before a token is minted and
before each send. Otherwise the run ends `retired` and nothing is sent.
Test: `a_tenant_change_retires_reviews_and_unsent_requests`. It holds a
managed read while its token is minted (the fixture token endpoint waits 3 s)
and switches tenant: the run ends `retired` and the API request never
arrives. It holds an Apply at the native confirmation and switches: the
review is gone at once, the Apply is refused, and after switching back the
review stays consumed. It replaces the credential: the review is retired.
No DELETE ever arrives.

## R4: wire credentials redacted

Redaction now includes the credentials derived on the wire: each Basic
encoding (request auth and the OAuth client authentication) and the minted
bearer token. They are removed from response headers and redirect targets
before the result, history, ShowRun or anything stored. Response bodies stay
in sealed storage for download, as before. Test:
`wire_credentials_are_redacted_wherever_runs_are_kept`. A service echoes
the minted bearer and a collection-Keychain Basic credential; the immediate
result, history page, ShowRun, every private file and the history after a
restart contain neither.

## Evidence gap: T23 offline to reconnected

`offline_then_reconnected_sends_nothing_by_itself`: a registered-origin
DELETE is sent once and its response lost (uncertain). The port then goes
offline: a read fails `connect`, not uncertain, and a new review can still
be prepared. The port comes back: nothing arrives within 1.5 s and the lost
run stays uncertain in history. The spent review is refused
(`reviewConsumed`), sending nothing. Only the fresh, explicit Apply and Send
reach the service, once each.

## Evidence gap: responsiveness target

`probe.swift activate-field` times from AXPress on the other tab to that
tab's URL text field exposing its URL (`{{base}}/items`), the registered
target. `measure_c15.py` uses it, and the workload is re-measured in this
attempt. The earlier heading-based samples are superseded, not relabelled.

## Probes

Ten probes are added: registered classification, registered origins without
a tenant, registered spellings, redirects into registered origins, Host
overrides, URL user-info, review retirement, the dispatch gate, bearer
redaction and Basic redaction. Three existing probes were retargeted at the
changed lines. The C15 set is now 40.
