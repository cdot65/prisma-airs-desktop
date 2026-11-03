# 1. Native operation authority owns coordination

Date: 2026-10-16. Status: selected following owner candidate selection; independent G01 verdict
and implementation readiness are tracked in planning/status.md. Implements D008,D009,D013,D014,D016,D019.

V1 already has deep domain/request modules, but GPUI callers still coordinate
slots, protected-key IO and ordered effects. React must not inherit that authority.

Choose a principal-bound Rust actor with dispatch(Intent) and observe(ViewScope).
Keep feature transitions domain-owned inside one initial core crate. Native
bootstrap injects adapters and creates operator/Harness handles; JSON cannot
select the caller principal. Finite domain instructions exclude arbitrary SDK
method strings and any Harness Apply capability.

The interface includes atomic admission/context retirement, per-source supersession,
separate read/review/write/stream domains, priority/deadline/rate-limit semantics,
bounded queues, bounded delivery-sequence deduplication with safe session rotation,
single-use settlement and truthful Remote outcome. No remote IO
or blocking vault call occurs while the authoritative transition is locked.
Admitted work runs on the host's one async runtime; native adapters execute effects.

React owns raw editor text/undo. Rust owns immutable captured drafts/reviews.
Review/Apply/Save/Send capture fresh input in their actual production handlers;
Apply compares fresh input with the stored reviewed operation and executes only
that stored operation. Invalid/lossy input, changed context/draft/resource or a
consumed review refuses dispatch. Never rely only on a debounced revision.
RequestApply uses a native operator confirmation; actual Apply completion is
private, single-use and revalidated after the modal editor/context lease.
Every scoped intent/observation carries native-issued context and principal
admission. Async vault completions are generation/lease-bound; obsolete writes
receive orphan cleanup rather than reactivating credentials. Explicit one-time
secret reveal/copy stays in a scoped transient native dialog, never snapshots.

Alternatives considered under the design-it-twice skill:

| Design | Depth/locality | Cost |
|---|---|---|
| Actor dispatch/observe | Callers learn one authority seam; context and settlement stay together | Finite unions and observation backpressure need discipline |
| Typed scoped handles | Strong domain methods and revoked scopes; flexible internal composition | More handles/methods and IPC registries for callers to coordinate |
| Task-oriented scan board | Trivial first scan workflow | Public task surface grows across all parity features |

Recommend the actor with domain-specific types and a small generated presentation
facade. Retain the alternatives' explicit scoped validation and effortless scan
usage internally. Do not combine all their public surfaces or add pass-through
modules merely to obtain two method names.

Two real execution adapters (SDK/scripted), storage adapters (Keychain/session/
fault-injectable) and clocks justify internal seams. The interface is the test
surface: switch/replace/cancel while queued/dispatched, old/duplicate reply,
fresh-input races, double Apply, denied Harness Apply, unknown outcome/no replay,
priority/deadline/rate-limit containment and observation resynchronization.

See [selected design](../architecture/DESIGN.md) for ownership/wire details.
No app code has been written and no native/SDK test is claimed by this ADR.
