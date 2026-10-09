# DemoStrike

**A native Rust workbench for reviewing CS2 utility events and preparing clip handoffs.** DemoStrike turns supported demo data into a searchable local match database, then keeps the event context available while a creator selects and packages recorded takes. The current native parser is deliberately narrow: it analyzes grenade evidence from three hash-validated evaluation demos, rather than claiming general CS2 demo support.

> **Development status:** The native Rust grenade path and desktop UI compile and pass headless tests. No demo files, extracted match facts, footage, or installer are distributed here. Visible window review, a broad parser corpus, and production capture acceptance are still open.

## From demo to editable clips

1. Import one of the three validated Ancient/Nuke demos with the Rust parser. DemoStrike checks its hash before analysis.
2. Review grenade releases, trajectories, detonations, round transitions, and nearby-player positions. Imported events carry a capability marker so unavailable combat and economy data are not shown as zeroes.
3. Keep results in a local SQLite match database. The domain and query layer includes event detail, filters, favorites, tags, collections, and render-job records; the current UI concentrates on browsing analyzed utility events and their detail.
4. After recording separate takes elsewhere, prepare a capture draft and produce a versioned package. A compatible editor can import each packaged take as a separate editable clip.

DemoStrike does not record the game, replace HLAE, or manufacture footage from a demo.

## Implemented capabilities

| Area | Current behavior | Limit |
| --- | --- | --- |
| Native demo parser | Rust <code>source2-demo</code> 0.5.9 adapter with hash and coverage checks for three specific demos | Other demo hashes are rejected; combat damage, flash effects, kills, assists, and economy are unavailable |
| Local analysis | Utility events, trajectories, nearby players, round information, SQLite persistence, and transactional import | Coverage reflects the grenade-only parser capability |
| Desktop UI | First-party Rust egui/eframe window with import progress, cancellation, match/event review, and capture-draft controls | The visible native window has not passed acceptance review |
| Capture package | Separate media files, SHA-256 provenance, partial-stage recovery, and validation of a versioned manifest | Stream headers are not proof of audible sound or capture origin |
| CLI | Demo inspection/import, raw-facts analysis, media probing, package creation, and recovery | Some commands need separately supplied demos, footage, or external tools |

The normal package command requires a muxed MP4 with video and audio stream headers and invokes <code>ffprobe</code> on the local machine. <code>package-structural</code> is a metadata-only handoff experiment; its output is explicitly labeled structural and should not be treated as validated media. See the [capture package v1 specification](docs/CAPTURE_PACKAGE_V1.md).

## Why the native parser has a gate

The parser reads Source 2 demo bytes through a pinned crates.io library and builds its own grenade facts. It does not substitute a saved sidecar for the demo. The current validation set binds three exact file hashes to expected map and coverage counts. An unknown file returns an explicit unsupported-demo error rather than a plausible-looking partial analysis.

Grenade-only results include release and detonation events, sampled flight paths, selected nearby-player positions, and supported round transitions. The importer records <code>grenade-only-v1</code> in SQLite. Screens and queries can then distinguish “not available” from a measured zero for combat and economy fields. The wider raw-facts schema is retained for compatibility with the separate reference path, but the Rust mode does not claim that wider coverage.

Import work runs in a background worker with progress updates and cancellation. The SQLite write is transactional, and an already imported canonical source path is rejected so a repeat import does not replace prior event IDs or annotations.

## Capture-package safety

A package is assembled from a draft that points to existing recorded takes. Each clip gets a hash and provenance fields such as demo fingerprint, event ID, map, round, tick range, camera role, dimensions, frame rate, audio declaration, and quality-control status. The package writer stages incomplete work separately, can resume a recognized partial copy, and refuses to overwrite an unrelated destination.

Stream probing checks headers before and after copying a normal package. It cannot hear the audio, establish where a video was recorded, or approve a take for publication. A structural package performs weaker checks and is labeled accordingly. The receiving editor must keep the media files available because the handoff links to them.

## Build and run

This is a Windows-tested source checkout. Use a Rust toolchain with edition 2024 support. The desktop app uses native Rust UI crates; the optional media probe calls an external FFprobe executable. The public ClipStrike core dependency is pinned to an exact Git commit in both <code>Cargo.toml</code> and <code>Cargo.lock</code>.

~~~powershell
cargo test --locked --features native-ui
cargo build --locked --features native-ui --bins
cargo run --locked --features native-ui --bin demostrike
~~~

The CLI is a separate binary:

~~~powershell
cargo run --locked --bin demostrike-cli -- inspect-demo <demo.dem>
cargo run --locked --bin demostrike-cli -- import-grenades <validated-demo.dem> <matches.sqlite>
~~~

The first command checks a CS2 demo header; it does not parse events. The second imports only an exact supported demo. The three files and their raw reference facts are not included in this repository. For package and probe commands, run the CLI without arguments to see its complete usage.

## Data and interoperability

The match database is SQLite and defaults in the desktop UI to the user's local application-data folder. A capture package contains <code>manifest.json</code> plus per-take files under <code>clips/</code>. It keeps demo/event provenance and media hashes, while the actual recorded files remain separate from the database.

The package handoff uses <code>clipstrike-core</code> from public [ClipStrike](https://github.com/Gerbzz/Clipstrike) at commit <code>f5dd0a3dceeb5ee1196e654210cc7af0ca5ee97a</code>. ClipStrike can import the package into a <code>.cstrike</code> project. That relationship is a format handoff, not a promise of accepted video or audio quality.

## Advanced reference import

A separate reference-import adapter remains available for local comparison with the earlier raw-facts workflow. It requires a user-supplied Node runtime, extractor script, and matching <code>@laihoe/demoparser2</code> module. The desktop UI can take the paths through <code>DEMOSTRIKE_NODE_PATH</code> and <code>DEMOSTRIKE_EXTRACTOR_SCRIPT</code>. None of that extractor code or its output is bundled in this repository, and it is not required for the native Rust grenade path. [Source attribution](ATTRIBUTION.md) records the private Rust-module provenance.

## What remains to build

The parser needs validation on a broader, redistributable demo corpus before it can accept general matches. Combat, flash, kill, assist, and economy capabilities need their own independently validated native extraction. Capture assembly, audible audio review, visible UI review, and clean-machine packaging also remain open. These are planned directions, not features available in this checkout.

The project source is private and has no public license grant. Third-party dependencies retain their own licenses; see [attribution](ATTRIBUTION.md) for reused project code.
