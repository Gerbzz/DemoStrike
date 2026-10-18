# DemoStrike HLAE scheduler port

This is an isolated, dependency-free Rust crate for the **core scheduler** in
advancedfx `CommandSystem.cpp` and `.h` at commit
`7567c24ec2501a7b07cda6213ece4540e57b5039`. It is a source handoff for
DemoStrike, not a full HLAE rewrite or a change to the active DemoStrike worker.
See [SOURCE_INVENTORY.md](SOURCE_INVENTORY.md) and
[LICENSE-ADVANCEDFX](LICENSE-ADVANCEDFX).

The crate accepts offline `f32` tick/time samples and returns inert `String`
batches. A sample tick of zero is ignored, time zero is processed, both cursors
start at `-1`, and a level reset rewinds cursors while retaining commands.
Disabled frames advance cursors. Tick matches are collected before time
matches. `dispatch_frame` collects the entire batch before invoking callbacks.

The pinned scheduler's open frame upper edge means a point at tick 10 fires
when the clock *leaves* 10, not when it first reaches 10. Its interval tree
visits only one branch at each node and can miss eligible commands; this port
keeps that observable quirk. Interval and clock values should be finite.

This crate ports scheduling, cursor handling, interval overlap, tree order,
list removal, clearing, and start shifting. It excludes console parsing,
RapidXML save/load, formatted curve interpolation, game hooks, and command
execution. No submodule code is included.

## Local verification

```text
CARGO_INCREMENTAL=0 cargo test --offline
CARGO_INCREMENTAL=0 cargo run --offline --example reference_trace
```

On a hosted runner with `g++`, Rust, and access to the two pinned source text
files, `bash ci/run-differential.sh` generates a harness from original C++
method bodies and compares its trace with the Rust example. See
[ci/HANDOFF.md](ci/HANDOFF.md) for retained code and stub boundaries. It has
not been generated, compiled, or compared locally.

## Worker handoff

Copy the crate into a reviewed path in the DemoStrike repository. Add the
path dependency shown in [worker-integration.toml](integration/worker-integration.toml)
and adapt [worker_adapter.rs](integration/worker_adapter.rs) to the worker's
frame and level-init calls. Feed it the existing offline demo clock samples.
Keep ownership of any command consumption in the worker; no command runner
is part of this crate. Run worker tests and the differential script before
merging. Nothing here has been published or merged.
