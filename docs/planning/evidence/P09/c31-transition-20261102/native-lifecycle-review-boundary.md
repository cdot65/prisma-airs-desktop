# Native lifecycle inspection boundary for the independent C31 review

The four focused regressions exercise actual Host/Tauri IPC and the production
core with scripted native adapters. They establish reload before reveal, after
a native confirmation is held, during delayed issuance, and the delivery/other
window control. They do not exercise AppKit event-task scheduling or a
RevealSecret effect concurrent with native page teardown.

The independent reviewer should inspect that remaining production boundary:
`authority.rs` dispatch releases the state lock before running effects, including
`Effect::RevealSecret`. `NativeSecrets::reveal` queues an AppKit task; its reveal
ownership generation is allocated inside that task. `NativeSecrets::dismiss`
ends the current sheet. `NativeConfirmation::confirm` also queues presentation;
`present` constructs a sheet after obtaining its main window. The pinned local
Tauri runtime-wry 2.12.2 implementation of `Context::send_user_message` executes
a task inline on its main thread and enqueues it from other threads.

These are static observations, not an executed race reproduction or an
independent verdict. No additional native UI action was performed during the
sealed fixture queue. Determine whether the production ordering preserves the
required page-exit ownership under queued/admitted callbacks, and report any
actual remaining fix honestly. Neither a favorable score nor the current four
focused results should be represented as coverage of an unexecuted AppKit race.
This concerns C31 only; it does not reopen C30.
