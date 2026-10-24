# Native Rust migration: match review milestone

The migration target is a Rust application, not a repaired React adapter. The
retained CSDM TypeScript application remains a behavioral reference and an
incomplete fallback; it is **not** evidence of Rust parity.

## Direct Rust path

`demostrike --review-library ABSOLUTE.sqlite --review-match 1 --review-round 3`
opens the root `eframe`/`egui` application directly. It does not open a WebView,
React route, JavaScript canvas or TypeScript playback hook. Existing import,
FilmCraft editor, capture tools and project navigation remain available.

- `src/match_review.rs`: shared read-only SQLite match/round/player/combat/utility
  and bounded position readers. The Tauri Rust library reexports this source
  instead of maintaining a divergent copy.
- `src/native_replay.rs`: indexed exact-tick scenes, source-ordered event seek,
  observed projectile trajectories and planted/defused/exploded bomb states;
  bounded fractional-tick playback, pause, seek and speed.
- `src/native_review_ui.rs`: real native match, round, player and utility tabs,
  docked roster/radar/event viewer, round navigation, event seek, player focus,
  calibrated map coordinates, observed facing, and an optional user-selected
  radar PNG. Missing facts remain explicitly unavailable, not invented zeros.
- `src/ui.rs`: existing Rust import/editor/capture workspace integration. Import
  completion and capture requests retain their original library/match identity.
- `src/bin/demostrike-rust-review-proof.rs`: explicitly headless real-demo parser,
  database, reader and replay verification. It cannot prove a native window.
- `tests/native_rust_review.rs`: separate Windows desktop/WGPU acceptance using
  the actual application, genuine screenshot receipt, source-bound painted
  viewer identity, exact source sample counts and PNG/database hashes. This is
  only accepted after the hosted job executes, not from headless tests alone.

The real-demo fixture and pinned upstream radar are downloaded into temporary
validation storage; neither they nor generated databases are added to source.
The optional `--review-radar ABSOLUTE.png` accepts a calibrated 1024×1024 PNG.
`--capture ABSOLUTE.png --review-capture-receipt ABSOLUTE.json` provides the
native-review-only validation receipt without overwriting existing evidence.
The pre-existing capture clock is unchanged by this migration work.

## Verification boundary

Synthetic egui interaction tests execute real layout/input/paint commands, but
are not GPU screenshots. Real-demo headless proof checks all recorded rounds
against SQLite source rows, but is not a desktop test. Hosted native acceptance
must independently paint Round 1 (31 events) and Round 3 (61 events), preserve the
database, and pass existing native workspace/FilmCraft/navigation gates.

## Remaining parity, not claimed complete

The original CSDM React viewer/routes/canvas/hooks remain under
`rust-candidate/upstream` and `rust-candidate/ported`. They are not yet removed.
Their staging ledger is not relabeled as ported by this milestone.

Remaining work includes keyboard controls, automatic next-round policy, map
panning/drawing, wider map/elevation support, additional match/statistics views,
demo compatibility and missing equipment/carrier/effect facts, and end-to-end
CS2/HLAE media recording, camera review, editor export and release packaging.
The native FilmCraft editor and Rust capture pipeline already present are
retained, not rewritten or declared newly verified by a 2D review screenshot.
