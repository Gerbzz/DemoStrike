# Hosted CI handoff

An earlier handwritten semantic duplicate was rejected as a differential
oracle and is excluded from this handoff. `ci/run-differential.sh` compares
only the pinned original source slices with the Rust port.

The corrected harness fetches only `shared/CommandSystem.cpp` and `.h` from
the pinned advancedfx commit and copies the original scheduling method bodies
and inline tree code verbatim into a standalone C++ translation unit. The
generator records input and slice SHA-256 values in
`reference/generated_manifest.json`. Test-only stubs replace game getters,
the command sink, warnings, formatted curve data, and range insertion.

## Exact source boundary

The generator retains these text slices (normalizing CRLF to LF only):

| File | Start anchor | End anchor (excluded) | Contents |
| --- | --- | --- | --- |
| `CommandSystem.h` | `class IExecuteClientCmdForCommandSystem {` | `class CommandSystem` | Original three interfaces |
| `CommandSystem.h` | `struct Interval {` | `double m_LastTime;` | Original comparator, interval tree, overlap predicate, maps, queue, and tree rebuild methods |
| `CommandSystem.cpp` | `CommandSystem::CommandSystem(` | `namespace CommandSystemXML {` | Original constructor, add, start shifts, remove, and clear |
| `CommandSystem.cpp` | `void CommandSystem::OnExecuteCommands(void)` | `void CommandSystem::AddCurves(` | Original frame collection, level reset, hook checks, and `CCommand::DoCommand` |

The wrapper supplies a minimal `CommandSystem` declaration and unformatted
`CCommand` fields. The **original** `DoCommand` body handles the plain-string
path; its curve objects are stubs, so formatted curve output is not verified.
`AddRange` is a test-only insertion seam for range fixtures because the
original paths for creating ranges require the omitted XML and console layers.
The concrete float clocks, inert output sink, warning stub, `_snprintf_s`
compatibility shim, and scenario driver are new code. None makes overlap,
cursor, traversal, or queue decisions. The old handwritten model is unused.

On a hosted Ubuntu runner with `g++` and Rust, from this crate root run:

```sh
CARGO_INCREMENTAL=0 cargo test --offline
bash ci/run-differential.sh
```

The second command fetches two pinned MIT source text files, without cloning
submodules, then compiles the generated source-based harness. It compares 17
labeled frame traces from original C++ and Rust using `diff -u`; exit code zero
and the `Pinned original C++/Rust scheduler traces match` line are acceptance
evidence. Hosted CI needs outbound access to `raw.githubusercontent.com`.

The corrected harness has **not** been generated, compiled, or compared in
this constrained local environment. If generation or compilation fails on CI,
report that failure; do not fall back to the handwritten model.

Differential traces do not yet cover formatted curves, XML persistence,
console commands, missing clock hooks, non-finite samples, reentrant callback
calls into `OnExecuteCommands`, or large degenerate trees. The Rust unit tests
cover more tick-zero, precision, and worker-adapter cases than this C++ trace;
those cases still need a source-based differential comparison for parity.

If the crate is copied under a larger DemoStrike repository, run these
commands from the crate directory and use the worker's own test command
separately. No service or game process is started by this script.

