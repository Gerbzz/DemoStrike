# Candidate evidence, 2026-10-17

Local preparation was on branch `impeccable/tauri-parity-candidate` in a separate repository under the task workspace. The reviewed source was later published as the isolated `rust-candidate/` directory on [DemoStrike draft PR #3](https://github.com/Gerbzz/DemoStrike/pull/3). The original DemoStrike, AnimateStrike, MapStrike and SkinStrike working trees were checked clean.

## Passed without a build

- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`: pass. This parses and formats Rust source; it does not compile dependencies or execute tests.
- `python tests/verify_cache_fixture.py --db <local-fixture.sqlite>`: pass against the isolated older-CLI Cache SQLite import. The private fixture path is passed by the caller and is not stored here. It executed all six static SQL strings from `src-tauri/src/library.rs`, checked `de_cache`, recorded score 10-5, 10 players, 16 round rows including one unknown winner, 253 utility events and type totals, SQLite integrity and unchanged database bytes.
- Four JSON configuration files parsed successfully.
- A no-emit TypeScript program check returned zero diagnostics using already installed CsThrowBack `node_modules` via temporary in-memory path mapping. The pinned upstream score helper passed tie/lead/trail tests, and the adapted `TeamScore` rendered the expected label, number, team name and lead color into static HTML. These are source tests, not native GUI acceptance. Candidate-specific dependency versions remain unverified because no install occurred.
- Pinned upstream ledger contains 3,168 unique tracked entries, SHA-256 `39CBACA23A15D96BBBE85AE1015D3A877BC7AB63277E3BBD9B59B5B79357F006`; two score files are `adapted_partial`, 3,126 entries remain `unported`.

## Hosted validation after publication

[Windows Actions run 39009345965](https://github.com/Gerbzz/DemoStrike/actions/runs/39009345965) passed at commit `5d9d80a`: candidate dependency install, frontend typecheck/build, adapted upstream score test, Rust formatting, Rust library and contract tests, and release-mode Tauri native compilation. The built executable remained on the hosted runner; the workflow publishes no executable, installer, image or other artifact. The initial run at `f235abd` failed in the Tauri build script because its Windows icon resource was absent. Commit `5d9d80a` added an `.ico` encoding of the existing approved transparent DemoStrike image, with provenance in `REUSE_AUDIT.md`, and the rerun passed.

`src-tauri/tests/library_contract.rs` tests read-only bytes, rows, missing/wrong schema and empty libraries. `src-tauri/src/capture_provider.rs` tests source hash/tick/camera validation and deterministic tick ordering. These passed on the hosted runner. The private real-demo cache fixture test ran only in local read-only source verification and was never uploaded.

At the `5d9d80a` build, Tauri launch, packaged bridge, live CS2/HLAE capture and native UI inspection remained unverified, with no candidate screenshot. A newer workflow gate is designed to launch the actual Tauri WebView2 window against an invented, sanitized SQLite fixture and publish app-content captures if its IPC assertions succeed; check its run and inspect the images before making a runtime or visual claim. The real Cache import was produced by an older DemoStrike CLI, not by this candidate. Existing DemoStrike native screenshots are not evidence for this candidate's UI.

The repository-root `.github/workflows/rust-candidate-verify.yml` is the active pull request workflow. It has read-only repository permissions and does not upload artifacts or releases. The workflow under `rust-candidate/.github/` is the candidate's original standalone preparation file and does not trigger from this repository layout.
