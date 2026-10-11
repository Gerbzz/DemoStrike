# DemoStrike

**A native Rust workbench for reviewing CS2 utility events and preparing editable clip handoffs.** DemoStrike turns supported demo data into a searchable local match database, then keeps event context available while a creator captures, reviews, and imports recorded takes. Its native parser has a bounded grenade, combat, and flash subset; it does not claim general CS2 demo support.

> **Development status:** This is an alpha source build. A local source checkpoint passed 71 non-UI and 75 native-feature tests; independent probes covered media timing and project import, and one bounded reimport, edit, and shared-compositor export decoded successfully. This public packaging uses a pinned public ClipStrike dependency and synthetic test metadata; the native window has not had pointer, visible-layout, or audible-speaker acceptance. No demo, match database, footage, private project, installer, or finished guide is distributed here.

## From demo to editable clips

1. Import a supported CS2 demo with the Rust parser. It validates stream framing, event schemas, source identity, and coverage before committing a match.
2. Review grenade releases, trajectories, detonations, round transitions, nearby-player positions, and supported blind, utility-hurt, kill, and assist facts. Imported events carry a capability marker so unavailable or ambiguous fields are not shown as measured zeroes.
3. Keep results in a local SQLite match database. The domain and query layer includes event detail, filters, favorites, tags, collections, and render-job records; the current UI concentrates on browsing analyzed utility events and their detail.
4. Plan a source-bound capture request, record separate camera takes with external CS2/HLAE tools, and seal a reviewer-approved package with source, identity, video, and audio checks.
5. Import an accepted package into a new editable `.cstrike` project. The same native window can open the shared ClipStrike editor for edits, preview, save, and export.

DemoStrike does not record the game, replace HLAE, or manufacture footage from a demo.

## Implemented capabilities

| Area | Current behavior | Limit |
| --- | --- | --- |
| Native demo parser | Rust <code>source2-demo</code> 0.5.9 adapter with framing, schema, coverage, and source checks | The tested corpus is bounded; unsupported layouts fail, and economy is unavailable |
| Local analysis | Utility, supported combat and flash facts, nearby players, round information, SQLite persistence, and transactional import | Ambiguous effect attribution stays unavailable rather than becoming a total |
| Desktop UI | First-party Rust egui/eframe window with import progress, cancellation, event review, capture requests, project import, and same-window shared editor | Native pointer, visible-layout, and audible playback acceptance are open |
| Capture handoff | Source-bound request, external three-role recording pilot, reviewed receipt, hash/identity/full-decode checks, and versioned package | Capture is not a supervised background worker in the normal UI; stream checks do not replace human camera review |
| Editable project | Accepted package becomes a new ClipStrike project with independently checked video coverage | Requires integer constant-frame-rate video and audio covering the full picture; prior projects need reimport or manual trim |
| CLI | Native demo import, capture planning/preflight/pilot/seal, media probing, package import, and older raw-facts tools | User supplies demos, media, CS2/HLAE, FFmpeg/FFprobe, and the configured native SDK |

The ordinary package command requires a muxed MP4 with video and audio stream headers and invokes local `ffprobe`. `package-structural` is a metadata-only handoff experiment, not accepted media. The source-bound capture route additionally requires a reviewed receipt, matching live identity/tick evidence, full media decoding, and visual review of each role. See the [capture package v1 specification](docs/CAPTURE_PACKAGE_V1.md).

## Why the native parser has a gate

The parser reads Source 2 demo bytes through a pinned crates.io library and builds its own facts. It does not substitute a saved sidecar for the demo. Before import it checks PBDEMS2 framing, file-info offset, replay timing, event field schemas, owned projectile paths and detonations, round outcomes, and finite coordinates. Unknown or incomplete layouts fail without committing a plausible-looking partial match. Three original hashes remain regression anchors, not an allowlist for every file the parser can read.

Native results include release and detonation events, sampled flight paths, nearby-player positions, supported round transitions, and a bounded set of blind, utility hurt, relevant death, and flash-assist facts. The importer records `native-combat-flash-v1` in SQLite. Attribution is unique-or-unavailable; older `grenade-only-v1` rows retain unavailable combat/flash fields. Economy remains unavailable. The wider raw-facts schema is retained for compatibility with the separate reference path.

Import work runs in a background worker with progress updates and cancellation. The SQLite write is transactional, and an already imported canonical source path is rejected so a repeat import does not replace prior event IDs or annotations.

## Capture-package safety

A package is assembled from a draft that points to existing recorded takes. Each clip gets a hash and provenance fields such as demo fingerprint, event ID, map, round, tick range, camera role, dimensions, frame rate, audio declaration, and quality-control status. The package writer stages incomplete work separately, can resume a recognized partial copy, and refuses to overwrite an unrelated destination.

Stream probing checks headers before and after copying a normal package. It cannot hear the audio, establish where a video was recorded, or approve a take for publication. A structural package performs weaker checks and is labeled accordingly. The receiving editor must keep the media files available because the handoff links to them.

For source-bound capture, a request freezes the selected event and three planned camera roles. Local `preflight-capture` checks supplied CS2, HLAE, FFmpeg, and FFprobe paths without launching them; CLI pilot commands can stage and launch a bounded offline recording with a task-owned runtime. The reviewer must verify actual camera identity and pixels before `seal-capture` will accept a receipt. DemoStrike does not bundle or install those external tools or automate a finished narrated guide.

`import-capture-package` uses FFprobe to inspect independent stream durations and every video-frame timestamp before writing a new editable project. It currently accepts a continuous single video stream at constant integer FPS and one audio stream that covers the full picture. Fractional FPS, timestamp gaps, short audio, missing durations, and unsupported stream layouts fail explicitly. Editor clips end at the validated picture endpoint while the package's original capture facts remain intact. Projects imported before this video-coverage rule need reimport or a deliberate manual trim; they are not silently rewritten.

## Build and run

This is a Windows-tested source checkout. Use a Rust toolchain with edition 2024 support. The desktop app uses native Rust UI crates and the GStreamer MSVC SDK required by its ClipStrike editor integration; see [ClipStrike local setup](https://github.com/Gerbzz/Clipstrike/blob/fdf2f6ac8828374b803577ae1f6b310b73860075/docs/LOCAL_SETUP.md). Cargo pins the public ClipStrike core, UI, and media crates to the same exact Git commit in `Cargo.toml` and `Cargo.lock`, with no sibling checkout or absolute source path required. Capture and package commands additionally need user-supplied CS2/HLAE and FFmpeg/FFprobe tools. No installer or clean-machine build is verified.

~~~powershell
cargo test --locked --features native-ui
cargo build --locked --features native-ui --bins
cargo run --locked --features native-ui --bin demostrike
~~~

The CLI is a separate binary:

~~~powershell
cargo run --locked --bin demostrike-cli -- probe-demo <demo.dem>
cargo run --locked --bin demostrike-cli -- import-demo <supported-demo.dem> <matches.sqlite>
cargo run --locked --bin demostrike-cli -- plan-capture <matches.sqlite> <event-id> <new-request.json>
cargo run --locked --bin demostrike-cli -- import-capture-package <accepted-package-folder> <new-project.cstrike>
~~~

The first command parses a demo without saving a match; the second imports only when its supported schemas and coverage validate. No evaluation demo or raw reference facts are included. Run the CLI without arguments for the remaining preflight, pilot, seal, and probe commands.

## Data and interoperability

The match database is SQLite and defaults in the desktop UI to the user's local application-data folder. A capture package contains <code>manifest.json</code> plus per-take files under <code>clips/</code>. It keeps demo/event provenance and media hashes, while the actual recorded files remain separate from the database.

The package and embedded editor use public [ClipStrike](https://github.com/Gerbzz/Clipstrike) at commit `fdf2f6ac8828374b803577ae1f6b310b73860075`. One bounded accepted-package reimport, saved/reopened chronological edit, and 6.2-second shared-compositor H.264/AAC export produced a fully decoded file with SHA-256 `E32FB0F447925F42F16E791F38C402E2FE75D687A04D929901B640836419DD01`. Neither that media nor its private project is in this repository; the result does not establish broad capture, editing, or audible quality.

## Advanced reference import

A separate reference-import adapter remains available for local comparison with the earlier raw-facts workflow. It requires a user-supplied Node runtime, extractor script, and matching <code>@laihoe/demoparser2</code> module. The desktop UI can take the paths through <code>DEMOSTRIKE_NODE_PATH</code> and <code>DEMOSTRIKE_EXTRACTOR_SCRIPT</code>. None of that extractor code or its output is bundled in this repository, and it is not required for the native Rust grenade path. [Source attribution](ATTRIBUTION.md) records the private Rust-module provenance.

## What remains to build

The parser needs a broader redistributable corpus before it can claim general matches. Economy extraction, a supervised in-window capture worker, native pointer and visible-layout review, audible speaker review, repeated export reliability, and clean-machine packaging remain open. This source does not produce a finished automatic narrated guide.

The project source has no public license grant. Third-party dependencies retain their own licenses; see [attribution](ATTRIBUTION.md) for reused project code.
