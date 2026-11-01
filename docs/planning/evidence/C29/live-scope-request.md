# C29 live row: what the owner must designate

C29's required acceptance includes one live read: "verify against the
designated read-only tenant (Vulture) under the owner scope, recorded as a
redacted receipt; no writes" (`chunks/C29.md`). The approved execution
manifest (`~/airs-v2-live.json`, `airs-v2/live-manifest/1`, approved
2026-10-25) designates Vulture (TSG 1852583913, environment test-b) for the
L18 scan family only. No family permits a management read on Vulture, so the
wizard's verify read (`management · verify`, the same read L02 runs on
Redtail) has no designated scope there.

A Keychain item `mgmt-vulture-client-secret` exists under the live service,
but a stored secret is not a designation. Per the release contract a family
without designated access is recorded **blocked**, never passed and never
run on an undesignated credential.

## What unblocks it

Add one family to the private manifest and re-run `cargo run -p xtask --
live-scope --scope <path>`:

```json
"L19": {
  "status": "designated",
  "reason": "",
  "environment": "test-b",
  "credentials": {
    "client_secret": { "kind": "keychain", "service": "io.cdot.prisma-airs-desktop.v2.live", "account": "mgmt-vulture-client-secret" }
  },
  "nonsecret": { "client_id": "<Vulture management client id>", "tsg_id": "1852583913", "region": "us" },
  "permitted_operations": ["one management verify read through the first-run wizard (C29); no inventories, no writes"],
  "disposable_prefix": null,
  "budget": { "reads": 2 },
  "cleanup": [],
  "irreversible_exclusions": ["no changes of any kind on Vulture"]
}
```

The run then goes through the real wizard on the fixture build with the
Keychain reference, and the receipt lands in `live/L19-verify.json` with the
client id and TSG only (no token, no secret). Until then `live/blocked.json`
is the row's record.

## Preparation update — 2026-11-01

The owner approved adding read-only L19 with the existing Keychain reference,
a maximum of two reads and no inventory browsing or changes. The public client
ID was supplied through the owner's local CLI tenant configuration; its secret
field was neither copied nor used. Private-manifest activation follows offline
validation of the new family.

Inspection of the actual management verifier found that its credential probe
reads one latest security-profile entry (`limit=1`, `latest=true`) and discards
the response contents. It does not merely obtain a token. The owner explicitly
approved this single-profile probe, retaining only verification status and call
metadata, within the two-read maximum and with no other inventory browsing or
changes. The earlier wording above must not be used to imply that it is token-only.
The exact bounded operation and input restrictions are now documented under
L19 in `docs/planning/live-checks.md` and validated by `xtask::live`.

The offline regression first failed because L19 was an unknown family; it now
accepts an optional complete designation and rejects scope expansion. This is
tooling preparation, not a live receipt. The historical `live/blocked.json`
remains unchanged until an approved actual wizard verification has a separate
source-bound receipt.
