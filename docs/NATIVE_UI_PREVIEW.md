# Native UI preview — 15 October 2026

The [README image](../assets/screenshots/demostrike-empty-native-current.png) is a 1440 × 900 PNG produced by DemoStrike's own eframe/egui WGPU renderer on the Office PC. It shows the empty analysis workspace before any demo is imported. The yellow notice says no capture media has been verified in this view; this is expected for an empty workspace.

- Preview source: draft PR [#1](https://github.com/Gerbzz/DemoStrike/pull/1), commit `c74e381e015e637cea08b9a916a595d69610dcfa`. The screenshot command is in that branch, not in this main revision.
- Hosted Windows test/build: [Actions run 38703871824](https://github.com/Gerbzz/DemoStrike/actions/runs/38703871824), completed successfully for that exact commit. It ran `cargo test --locked --features native-ui --bin demostrike` and `cargo build --locked --features native-ui --bin demostrike`.
- Executable artifact SHA-256: `3B796AC73F7A39049271687423DC5D22D2B2BD1ED98F66231FBEDC689D8D8A01` (119,061,504 bytes). The downloaded executable matched the hosted SHA manifest.
- The executable ran with `--capture` and an absolute output PNG path. Its `LOCALAPPDATA` and capture workspace pointed to empty isolated directories. It exited successfully after writing the screenshot.
- Published PNG SHA-256: `BA4F746464BD3FEAACA83705C827D9263F33C962D475D7921E70EADE37A288E2` (85,178 bytes).

No CS2 demo, HLAE session, capture package, match database, user recording, or external media was loaded. This image verifies the initial native window rendered on that draft executable. It does not verify demo import, event analysis, capture quality, editor export, or release readiness. The executable and isolated runtime data are not part of this repository.
