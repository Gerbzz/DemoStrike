# CSDM import cancellation slice

The preserved CSDM renderer remains mounted in Tauri WebView2. Its small
`ported/` controls send `csdm_enqueue_demo`, `csdm_list_import_jobs`, and
`csdm_cancel_import` to the Rust backend. Parsing, the durable queue, source
verification, and the SQLite match transaction remain in Rust.

The queue accepts cancellation of a queued job or an active job before the
existing `ImportControl` database-commit boundary. A queued job is skipped;
an active job receives the same atomic cancellation signal already used by
the native importer. Cancelled jobs remain in history and are not retried
after restart. Once finalization starts, cancellation is rejected and the UI
continues polling for the committed result. The old queue schema is migrated
in a transaction, preserving job IDs, receipts, errors, and recovery flags.
The source demo is never deleted or altered by this control.

The library commit and queue receipt are separate SQLite transactions. At
startup, a job interrupted after its library commit is reconciled to the saved
match receipt before it can be retried or cancelled. A recovered job also
checks for that receipt at cancellation time; an already saved match is
reported as completed, never as cancelled. Cancellation and worker completion
hold one queue transition lock, so an accepted active cancellation cannot be
persisted as a failure by the worker. A library that exists but cannot be
read stops reconciliation with an error instead of claiming a false outcome.

Hosted unit tests cover old-schema migration, queued cancellation, active
cancellation, a coordinated finish/cancel race, a crash between library commit
and queue receipt, restart persistence, and the importer's existing rollback
and too-late boundary. The hosted synthetic WebView2 smoke holds only the
smoke-feature parser long enough to click Cancel in the real CSDM window,
then checks that no match was saved and a later import still works. The
feature-free build has no parser delay. A live CS2 cancellation click and
the original upstream multi-demo Analyses route remain unverified.

This is a bounded Rust behavior port. React/TypeScript still mounts the
original CSDM shell and routes, and thin `ported/` components still display
the queue. No upstream source, license text, assets, or donor code was changed.
