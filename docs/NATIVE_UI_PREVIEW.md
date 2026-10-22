# Native UI previews

## Selected .DEM identity native capture - PR #14

The [README showcase](../assets/screenshots/demostrike-dem-tactical-native-pr14.png) is the original, unedited 1028 × 749 PNG from the actual hosted Windows Tauri/WebView2 window. It shows the ported CS Demo Manager renderer with the .DEM title-bar badge, original navigation and window controls, and an empty match library. The path fields contain placeholders; no personal path or match data is shown.

This is the separate source candidate under `rust-candidate/ported`, using the pinned full upstream application source with native adapters. The repository-root `demostrike` executable remains the legacy eframe launcher. This capture does not establish that the candidate is the default launcher, a released installer, or a completed port of every upstream feature.

- Source PR: [#14](https://github.com/Gerbzz/DemoStrike/pull/14), captured source head `854128ff8d45637ccc88c9f6634f7c90b7b22adb`.
- Actual checked-out native test merge: `b90697ea884104c0b288b9683cbd9a67aaccb91d`, combining that head with base `a1d3c12b8aa59e0630418a46d024958e5c504248`.
- Tested tree: `4c0df0acf2d296d6f212dc4c9b1faad8cf430238`. The later README, provenance and screenshot commit changes only documentation and this PNG; all application and branding paths remain identical to this captured source.
- Hosted capture: [run 39696300842, original-native job 119554636650](https://github.com/Gerbzz/DemoStrike/actions/runs/39696300842/job/119554636650). Its `demostrike-original-native-startup` artifact's `proof.json` binds the source commit, executable and screenshot hashes.
- Capture artifact: `demostrike-original-native-startup`, ID `12391542933`, uploaded at `2026-10-22T00:08:16Z`; ZIP SHA-256 `27c79f64dfced933e8f71c10c23272c036f928bd3e9f8ba313f2448dba812e16`. The artifact timestamp is distinct from the source commit date.
- Original member: `original-matches-empty.png`, copied byte for byte to the showcase path; 60305 bytes; SHA-256 `20739fb709e75434f36b116306c6d9ef56a9084f1a59f19ad6e4fa586f1f8925`.
- Captured executable SHA-256: `0d93eecbd51c78bdb4f2a30a38e9727de058f1a4905110733884801396c0011e`.

The same job verified all seven embedded icon payloads at Windows resource group 32512 and asserted that the title-bar image finished decoding and the page title was DemoStrike. The capture used the existing CI synthetic-import smoke build, whose parser is configured for synthetic fixtures; this empty frame shows no loaded fixture. That captured executable is not a production release executable or proof of real-demo decoding. Its broader native smoke exercises synthetic import and read-only navigation separately. The PNG alone does not demonstrate demo analysis, HLAE capture, FilmCraft editing/export, taskbar appearance, alternate DPI, or interactive Room acceptance.

The retained images below are historical eframe captures. They do not establish the appearance of the selected .DEM identity or a released installer.

## Historical eframe previews

### PR #2 empty workspace - 17 October 2026

The [retained historical image](../assets/screenshots/demostrike-empty-native-pr2-2026-10-17.png) is an unedited 1024 × 768 PNG captured by DemoStrike's native eframe/egui renderer in a hosted Windows run. It shows the revised empty workspace with its import action, saved-match navigation, and optional configuration panels. The capture used an isolated empty `LOCALAPPDATA` directory; no demo or media was loaded.

- Source: PR [#2](https://github.com/Gerbzz/DemoStrike/pull/2), commit `f73f62a043a5e34245aacf4bfd2a924888c83050`. PR #2 and its [integration PR #5](https://github.com/Gerbzz/DemoStrike/pull/5) are merged; those merges do not change this image's source provenance.
- Hosted test, native build, and capture: [Actions run 38989213660](https://github.com/Gerbzz/DemoStrike/actions/runs/38989213660), completed successfully for that exact source commit.
- PNG SHA-256: `1E705B74DEFA43D6028E5DB7CD5928788EB8B974EBE64BFBF82A9A903351F82A` (64,773 bytes).

This capture verifies the historical empty native window only; it does not depict the selected Tauri renderer or current main. The later saved-match empty-state text and import action require interactive review. Demo import, event analysis, capture quality, FilmCraft editing, and export are outside this image's scope.

### Earlier baseline - 15 October 2026

The [earlier baseline image](../assets/screenshots/demostrike-empty-native-current.png) is a 1440 × 900 PNG produced by DemoStrike's own eframe/egui WGPU renderer on the Office PC. It shows the empty analysis workspace before any demo is imported. The yellow notice says no capture media has been verified in this view; this is expected for an empty workspace.

- Preview source: PR [#1](https://github.com/Gerbzz/DemoStrike/pull/1), commit `c74e381e015e637cea08b9a916a595d69610dcfa`. The initial screenshot-only main commit `6bbcb060bca4321d56bbf10770972996020c9928` did not contain the screenshot command.
- Hosted Windows test/build: [Actions run 38703871824](https://github.com/Gerbzz/DemoStrike/actions/runs/38703871824), completed successfully for that exact commit. It ran `cargo test --locked --features native-ui --bin demostrike` and `cargo build --locked --features native-ui --bin demostrike`.
- Executable artifact SHA-256: `3B796AC73F7A39049271687423DC5D22D2B2BD1ED98F66231FBEDC689D8D8A01` (119,061,504 bytes). The downloaded executable matched the hosted SHA manifest.
- The executable ran with `--capture` and an absolute output PNG path. Its `LOCALAPPDATA` and capture workspace pointed to empty isolated directories. It exited successfully after writing the screenshot.
- Published PNG SHA-256: `BA4F746464BD3FEAACA83705C827D9263F33C962D475D7921E70EADE37A288E2` (85,178 bytes).

No CS2 demo, HLAE session, capture package, match database, user recording, or external media was loaded. This image verifies the initial native window rendered on that draft executable. It does not verify demo import, event analysis, capture quality, editor export, or release readiness. The executable and isolated runtime data are not part of this repository.

## Historical Windows audio diagnostic

A historical full serial native library run ended with Windows access violation `0xc0000005` as `ui::filmcraft_audio::tests::stale_ready_generation_cannot_activate_or_become_the_clock` starts. [Hosted comparison run 38992294811](https://github.com/Gerbzz/DemoStrike/actions/runs/38992294811) reproduced it on the historical base and the unchanged PR #2 audio source; the isolated test passed on both. The root native call is unknown.

The [manual historical audio workflow](../.github/workflows/historical-audio-diagnostic.yml) retains the isolated and full serial tests on both the pinned historical base and the selected ref. The main native-preview workflow still gates the isolated audio invariant, import and FilmCraft navigation tests, CLI and binary tests, native build, and native screenshot. A new failure in those focused gates remains blocking.
