# C20 follow-ups for the P06 push to 9/10 (D034)

C20 completed at 8/10 under D034. Codex attempt-01 scored 9/8/8/8/8 with no
blockers, five required fixes (F1–F5) and two evidence gaps. All are
remediated in the P06 push. Each fix has a production-interface test and a
mutation probe in `docs/planning/evidence/C20/mutation-probes/probes.py`
(section "P06 follow-ups"). D047 records the resulting rules.

## F1: an older export never stands in for newer turns

- Code (`harness/session.rs`):
  - When a turn ends, `latest` is cleared. Only that turn's own export
    sets it again.
  - A renewal after a refused or unanswered export therefore has nothing
    to restore. It fails as `restoreFailed` without starting an engine or
    sending the message.
- Stand-in mode `history-refuse-later` exports only the first turn.
- Test `a_renewal_never_restores_an_export_older_than_the_conversation`
  (`harness_identity_history`): turn A exported, B's export refused, then a
  renewal for C. The conversation fails `restoreFailed`, keeps its four
  entries, launches no engine, and the old engine is reaped.
- Probe: `an older export is restored after a refused one`.

## F2: Keychain writes for sign-in and rotation are owned until they settle

- Code (`authority/harness_identity.rs`):
  - `begin_write` and `write` are now the shared confirmed write-ahead
    contract; C22's MCP grants use the same functions.
  - The record joins the ledger, and the save is confirmed durable (an
    unconfirmed ledger that cannot be confirmed writes nothing).
  - The record is marked in flight, so cleanup and reset wait for it.
  - It stays in flight until `settle_write` ends that under the same lock
    as the save that commits it or leaves it in the ledger (C22 review F4).
    A stale write stays in the ledger and is deleted; its token is revoked.
- Test `sign_in_writes_are_owned_until_they_settle_and_need_a_confirmed_ledger`:
  - An unconfirmed store makes the sign-in fail `storeFailed` with no Put.
  - An initial write held, then the sign-in cancelled.
  - A rotation write held, then signed out.
  - An initial write held, then reset.
  - Each time, no refresh-token item and no ledger entry is left, and
    nothing is signed in.
- Test `signing_out_during_a_sign_in_again_keeps_nothing`: a sign-in again's
  write is held while the account signs out. Neither token is kept, and the
  new one is revoked.
- Probes:
  - `a sign-in write needs no confirmed ledger`
  - `a Keychain write in flight is not owned`

## F3: re-sign-in and renewal never overwrite each other

- Code:
  - A sign-in again to the same account takes the live grant's token lock
    before its write and holds it until its tokens are installed. It
    renews that grant in place, so the grant and binding are unchanged and
    `revive` clears an expiry.
  - A rotation commits only while the stored record is still the one it
    started from (`grant_holds`).
- Test `a_renewal_and_a_sign_in_again_never_overwrite_each_other` covers
  both orders:
  - A rotation write held, then a sign-in again: the sign-in waits, writing
    nothing.
  - A sign-in again's write held, then a renewal: the renewal waits, then
    uses the new token with no refresh.
  - Each time, exactly one refresh-token item remains. It is the stored
    grant's, under the same grant ID, with an empty ledger, and the
    conversation continues.
- Fixture: the synthetic provider now names each refresh token uniquely.
  Earlier, `refresh-<count>` could reissue a revoked name and hide a
  rotation.
- Probe: `a sign-in again does not wait for a renewal`.

## F4: history saves are fenced against tenant removal and reset

- Code:
  - Each save is admitted under the authority lock (`admit_history_write`):
    storage ready, tenant present, scope noted. The tenant counts as writing
    and the scope key stays in flight until the save settles.
  - Tenant removal deletes the folder only after that tenant's saves
    settle, and cleanup deletes the key after them.
  - Reset waits for every save, then purges `harness-history` with the
    Keychain namespace. A failure is a reset failure.
  - A launch removes history folders of tenants that no longer exist.
- Test `history_saves_racing_tenant_removal_or_reset_never_outlive_them`:
  - A save held past admission while the tenant is removed, then released:
    no file, key or ledger entry remains.
  - A stray folder is removed at the next launch.
  - A save held while resetting: the history folder and keys are gone and
    no reset failure is reported.
- Probes:
  - `a history save is not fenced`
  - `reset leaves the history folder`
  - `a removed tenant's stray history survives a launch`

## F5: deleting an unreadable record asks first

- Code (`history-panel.tsx`): "Delete unreadable conversation…" opens a
  confirmation alert with "Delete permanently" and "Keep it". Nothing is
  deleted until it is confirmed.
- The App test (`says why a saved conversation cannot be continued, names
  unreadable ones and a full workspace`) covers three steps. The first
  activation deletes nothing. Keep it keeps the record. Only confirming
  dispatches the deletion.
- Native C20 checks the keyboard flow (see native-checks C20-N10).
- Probe: `an unreadable record is deleted without asking`.

## Evidence gaps

- **Stand-in failure modes:** test
  `refused_or_unanswered_exports_and_refused_restores_are_named` selects
  `history-refuse`, `history-silent` and `restore-refuse`:
  - a refused export is named `unavailable` and writes nothing;
  - an unanswered one fails the conversation (`timeout`) and reaps the
    engine;
  - a refused restore fails as `restoreFailed` and keeps the record.
  - Probes: `a refused export is not named`,
    `a refused restore is not named as one`.
- **T23 offline, reconnect, sign-out in flight, expiry after sleep:**
  - `offline_history_is_read_and_exported_and_nothing_is_replayed_on_reconnect`:
    - The app is relaunched with AI Gateway unreachable.
    - The last chosen workspace's history is listed from this Mac (`local`).
    - A record opens as `workspaceUnavailable` and exports as Markdown,
      while no engine starts.
    - Reconnected, nothing is resumed or replayed, and the record is
      continuable again only by the operator.
  - `signing_out_during_a_sign_in_again_keeps_nothing`: see F2.
  - `after_sleep_an_expired_sign_in_is_renewed_before_anything_is_sent`:
    after sleep, an expired, unrenewable sign-in. Continuing starts no
    engine, names `sessionExpired`, and signing in again succeeds.
  - Frontend: `reads and exports what is saved on this Mac when no
    workspace can be chosen (T23)`. The contract requires `local`.
  - Probes:
    - `saved history cannot be read offline`
    - `an offline record is continuable`
    - `offline history is not shown`
    - `an online list is shown as this Mac's`
    - `a history view without local passes the contract`

## Optional suggestions

- One JWKS refresh is now allowed per verification, so each later key
  rotation is followed (D047).
- DESIGN.md and qualification.md now point to D047/D049 for the bounded
  native flows and `aws_lc_rs`.

## Attempt-02 remediation

Attempt-02 scored 9/8/8/8/8. It had one required fix and no evidence gaps.
It found F1-F5 and both evidence gaps closed.

**R2-F1: a history scope is noted durably before its key is made.**
- Code (`authority/harness_history.rs`, `admit_history_write`): after
  noting the scope, an unconfirmed document is confirmed first, or the save
  is refused and named `unavailable` with nothing written. This holds
  whether the note is this save's or an earlier one known only in memory,
  so the known-scope branch is not a bypass. It is the rule
  `harness_identity::begin_write` already applies to sign-in writes.
- Test `a_history_key_is_made_only_once_its_scope_is_durably_noted`
  (`harness_identity_history`):
  - With saves and their confirmation unconfirmed, two turns make no Keychain
    Put and no sealed record, and each names the failure.
  - The last confirmed document (what a crash leaves) holds no scope.
  - Once confirmable, the next save confirms the note, then writes.
  - After a crash relaunch from the confirmed document, removing the tenant
    removes the record and its key.
- Probe: `a history key is made under an unconfirmed note`.

**Suggestions.**
- `after_sleep_an_expired_sign_in_is_renewed_before_anything_is_sent` now
  waits past the twelve-second token's own expiry (13 s), not only its
  renewal margin.
- F2's description above now says when a write leaves flight.
