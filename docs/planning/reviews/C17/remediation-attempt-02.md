# C17 remediation after attempt-02

Verdict attempt-02 scored architecture 9, behavior 8, authority/security 7,
validation 8 and overall 8 with three required fixes, one evidence gap and
no blockers. Authority/security is below the D034 chunk floor of 8, so the
chunk is remediated before completion. D043 (T23) and the DESIGN C17
section record the decisions below.

## R1: credential checking fails closed on incomplete text

`gateway_local::check` now keeps only text that reads as a complete
document (or is blank). Text that does not parse is refused with the new
problem `incomplete`, so the credential check always sees every field.
That check covers top-level `key` and `secret_mappings` and credential
fields at any depth, escaped or not. Text still being typed stays in the
editor and is never written to this Mac or published in a snapshot; the
last complete text is what is kept. A file holding such a draft fails the
read check and is never restored. The size bound is unchanged. The
workbench note says why the edits are not kept yet.

Tests:
- Unit `credential_text_is_never_kept_whole_truncated_nested_or_escaped`:
  complete, truncated, nested and escaped canaries, and a planted file that
  is refused on read.
- Integration `credential_text_never_reaches_this_mac_even_while_it_is_being_typed`:
  nine canary bodies, both as new drafts and as edits of a kept draft. It
  checks the snapshot, the file and an offline restart, and that the size
  bound still holds.
- The App test asserts the `incomplete` note.
- Contract validation accepts `incomplete`.

## R2: a reset fences Gateway local work

Reset admission and a completed reset both drop every book, queued
settlement, pending write and detached change (`forget_all_gateway_local`).
Reads now take their turn on the same writer lock as writes, tenant
deletion and reset, so a read admitted before a reset finds its lease gone.
Every book change takes a new generation from the authority's lease
counter, so a write admitted for an earlier book can never match a later
one. A write or settlement for a tenant that no longer exists does
nothing, and no settlement is accepted while resetting.

Test: `local_work_held_across_a_reset_never_reappears`. With every file
read and write held, it leaves tenant A's edit waiting to be written and
tenant B's unread book waiting for a read (with an uncertain settlement
queued). It then admits a reset and releases the writer: the sessions root
is purged and stays absent, neither file reappears, and the local view is
idle.

## R3: a change in flight keeps its record under its own tenant

When the active context is retired (tenant switch, credential replacement,
tenant deletion, reset), an unsettled Gateway change with a local origin
leaves the active results but is held in `detached_changes`. It has no
review or confirmation and is never shown. When its operation settles, its
reply, current item and one-time key are dropped unread, and its local
settlement is applied under the originating tenant and workspace. That
settlement drops the draft once something was sent and records an
uncertain outcome. A deleted tenant or a reset keeps nothing, and nothing
is replayed.

Test: `a_write_in_flight_when_the_context_changes_is_kept_under_its_own_tenant`
covers three cases:
1. A provider update awaiting its reply when the credential is replaced:
   it is recorded unknown under tenant A in workspace Team A, with the
   results withdrawn.
2. A provisioning awaiting its workspace create when tenant B is chosen:
   it is recorded in A's file while B shows only its own work. The file
   holds no canary, and an offline restart in A restores both records with
   nothing sent.
3. A tenant deleted while its write awaits a reply: no file is kept for it.

## G2

The regressions above use the production interface. Four new mutation
probes guard them:
- text still being typed kept as a draft;
- a reset that leaves Gateway local work to be read and written;
- a change in flight dropped with its context;
- a deleted tenant's change still kept.

The credential probe's anchor moved with the check.
