# Candidate evidence, 2026-10-17

Local branch: `impeccable/tauri-parity-candidate`. Separate repository under the task workspace; no remote and no push. Original DemoStrike, AnimateStrike, MapStrike and SkinStrike working trees were checked clean.

## Passed without a build

- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`: pass. This parses and formats Rust source; it does not compile dependencies or execute tests.
- `python tests/verify_cache_fixture.py --db <local-fixture.sqlite>`: pass against the isolated older-CLI Cache SQLite import. The private fixture path is passed by the caller and is not stored here. It executed all six static SQL strings from `src-tauri/src/library.rs`, checked `de_cache`, recorded score 10-5, 10 players, 16 round rows including one unknown winner, 253 utility events and type totals, SQLite integrity and unchanged database bytes.
- Four JSON configuration files parsed successfully.
- A no-emit TypeScript program check returned zero diagnostics using already installed CsThrowBack `node_modules` via temporary in-memory path mapping. The pinned upstream score helper passed tie/lead/trail tests, and the adapted `TeamScore` rendered the expected label, number, team name and lead color into static HTML. These are source tests, not native GUI acceptance. Candidate-specific dependency versions remain unverified because no install occurred.
- Pinned upstream ledger contains 3,168 unique tracked entries, SHA-256 `39CBACA23A15D96BBBE85AE1015D3A877BC7AB63277E3BBD9B59B5B79357F006`; two score files are `adapted_partial`, 3,126 entries remain `unported`.

## Authored, not executed

`src-tauri/tests/library_contract.rs` tests read-only bytes, rows, missing/wrong schema and empty libraries. `src-tauri/src/capture_provider.rs` includes pure plan tests for source hash/tick/camera validation and deterministic tick ordering. Rust tests, npm build, Tauri launch, packaged bridge and native UI testing were not run because Office had about 21.7 GiB free with a protected 20 GiB floor and local builds/executable downloads were prohibited.

The new `score_relation` Rust contract and its integration fixture tests are authored but unrun. The JavaScript score component test used already installed local dependencies; hosted CI is configured to run the same test with candidate-installed dependencies after publication is separately authorized.

No current candidate GUI screenshot exists. The real Cache import was produced by an older DemoStrike CLI, not by this candidate. Existing DemoStrike native screenshots are not evidence for this candidate's UI.

`.github/workflows/verify.yml` prepares frontend build and Rust tests on a hosted Windows runner with read-only repository permissions, triggered only by a pull request or manual dispatch. It has not run and does not upload artifacts or releases.
