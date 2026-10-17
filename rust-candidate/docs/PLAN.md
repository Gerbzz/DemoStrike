# Migration plan

1. Inventory every tracked upstream file and separately identify external tools/submodules and licenses.
2. Establish a Tauri command boundary and an adapted Matches → Match Overview screen backed by a read-only existing DemoStrike SQLite import. Validate counts against a real import.
3. Port upstream data and application services behind typed adapters, keeping temporary dependencies labelled. Build contract tests for success, errors, identity, filtering, and preservation.
4. Port remaining routes in workflow order: demos/import, match detail, players/teams, downloads, 2D viewer, playback, video/capture, settings and maintenance. Update each ledger row only when tested.
5. Run hosted compile/unit/UI tests where allowed. Native game, capture, and crash-restoration tests need a suitable machine and explicit runtime acceptance. Review independently before any merge.

The HLAE-to-Rust provider track is documented in `HLAE_RUST_PARITY.md`. It begins with pure source/tick/camera contracts and keeps the current HLAE path as a supported provider until a replacement is genuinely verified.

No step authorizes deletion, downloads, local builds, production changes, or merging.
