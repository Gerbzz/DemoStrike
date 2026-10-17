# Native UI previews

## Current draft workspace - 17 October 2026

The [current README image](../assets/screenshots/demostrike-empty-native-pr2-2026-10-17.png) is an unedited 1024 × 768 PNG captured by DemoStrike's native eframe/egui renderer in a hosted Windows run. It shows the revised empty workspace with its import action, saved-match navigation, and optional configuration panels. The capture used an isolated empty `LOCALAPPDATA` directory; no demo or media was loaded.

- Source: draft PR [#2](https://github.com/Gerbzz/DemoStrike/pull/2), commit `f73f62a043a5e34245aacf4bfd2a924888c83050`.
- Hosted test, native build, and capture: [Actions run 38989213660](https://github.com/Gerbzz/DemoStrike/actions/runs/38989213660), completed successfully for that exact source commit.
- PNG SHA-256: `1E705B74DEFA43D6028E5DB7CD5928788EB8B974EBE64BFBF82A9A903351F82A` (64,773 bytes).

This capture verifies the empty native window only. The later saved-match empty-state text and import action require interactive review. Demo import, event analysis, capture quality, FilmCraft editing, and export are outside this image's scope.

## Earlier baseline - 15 October 2026

The [earlier baseline image](../assets/screenshots/demostrike-empty-native-current.png) is a 1440 × 900 PNG produced by DemoStrike's own eframe/egui WGPU renderer on the Office PC. It shows the empty analysis workspace before any demo is imported. The yellow notice says no capture media has been verified in this view; this is expected for an empty workspace.

- Preview source: PR [#1](https://github.com/Gerbzz/DemoStrike/pull/1), commit `c74e381e015e637cea08b9a916a595d69610dcfa`. The initial screenshot-only main commit `6bbcb060bca4321d56bbf10770972996020c9928` did not contain the screenshot command.
- Hosted Windows test/build: [Actions run 38703871824](https://github.com/Gerbzz/DemoStrike/actions/runs/38703871824), completed successfully for that exact commit. It ran `cargo test --locked --features native-ui --bin demostrike` and `cargo build --locked --features native-ui --bin demostrike`.
- Executable artifact SHA-256: `3B796AC73F7A39049271687423DC5D22D2B2BD1ED98F66231FBEDC689D8D8A01` (119,061,504 bytes). The downloaded executable matched the hosted SHA manifest.
- The executable ran with `--capture` and an absolute output PNG path. Its `LOCALAPPDATA` and capture workspace pointed to empty isolated directories. It exited successfully after writing the screenshot.
- Published PNG SHA-256: `BA4F746464BD3FEAACA83705C827D9263F33C962D475D7921E70EADE37A288E2` (85,178 bytes).

No CS2 demo, HLAE session, capture package, match database, user recording, or external media was loaded. This image verifies the initial native window rendered on that draft executable. It does not verify demo import, event analysis, capture quality, editor export, or release readiness. The executable and isolated runtime data are not part of this repository.
