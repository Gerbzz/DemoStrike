# DemoStrike source provenance

This local prototype reuses the following Rust modules from Neil's private
`Gerbzz/csthrowback` repository at commit
`9559eb710d563aaa1d9262b2df9445432429d394`:

- `src-tauri/src/analysis/mod.rs` -> `src/analysis/mod.rs`
- `src-tauri/src/domain/mod.rs` -> `src/domain/mod.rs`
- `src-tauri/src/parser/raw.rs` -> `src/parser/raw.rs`
- `src-tauri/src/db/mod.rs` -> `src/db/mod.rs`
- `src-tauri/src/db/queries.rs` -> `src/db/queries.rs`
- `src-tauri/src/error.rs` -> `src/error.rs`

The source repository has no root LICENSE or NOTICE file in this checkout.
This repository is private. No license is inferred for copied project
code. Audit source ownership, dependency and asset licenses before any
repository migration or distribution. The original private repository remains the provenance reference.

The DemoStrike Rust UI is new code. The original Tauri/React/TypeScript UI
has not been copied into it.
`src/demo_import.rs` adapts the parser contract and staged import flow from
CsThrowBack's `src-tauri/src/parser/mod.rs` and `src-tauri/src/commands/mod.rs`.
It does not copy or bundle the Node extractor script or `demoparser2` module;
the original source checkout remains the provenance reference.
