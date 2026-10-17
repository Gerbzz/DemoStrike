# DemoStrike Rust candidate

An isolated Tauri 2 and React migration candidate for CS Demo Manager 3.20.1. The source baseline is [`akiver/cs-demo-manager` at `7270bc95c8cab87b7f9ddd490fea13c6eda3ace7`](https://github.com/akiver/cs-demo-manager/tree/7270bc95c8cab87b7f9ddd490fea13c6eda3ace7). The candidate does not replace the current DemoStrike, CSthrowback, ClipStrike, or FilmCraft applications.

The first implemented slice reads an existing DemoStrike SQLite import and shows the match library, overview, rounds, players, and utility events. It never imports a demo, writes to the database, launches Counter-Strike, or captures video. The UI adapts the upstream Matches and Match Overview information architecture to a smaller temporary data contract; see `docs/UPSTREAM_ADAPTATIONS.md`. This is not a completed rewrite or upstream feature parity.

## Intended development run

Install dependencies only in an environment with sufficient disk space, then run `npm install` and `npm run tauri dev`. Use Browse or the path field to open an existing DemoStrike `.sqlite` file. Local preparation did not install packages, compile Rust, run Tauri, or produce a native screenshot because the Office disk floor prohibits local builds and executable downloads. Hosted CI later built the source, but no release binary is provided.

## Source map

- `src-tauri/src/library.rs`: read-only, validated SQLite adapter.
- `src/upstream/`: the first bounded, pinned upstream match-score component and helper, rendered in the library and overview. Provenance and acceptance gaps are in `docs/UPSTREAM_ADAPTATIONS.md`.
- `src-tauri/src/capture_provider.rs`: pure request/provider contract for the future HLAE-to-Rust parity track; no runtime provider exists yet.
- `src-tauri/src/lib.rs`: typed Tauri commands for match list and detail.
- `src/bridge.ts`: typed frontend command adapter; future upstream `window.csdm` and WebSocket messages belong here.
- `src/App.tsx`, `src/components/`: match library and overview UI.
- `docs/MIGRATION_LEDGER.csv`: exhaustive upstream file ledger, with unported status unless proven otherwise.
- `docs/REUSE_AUDIT.md`: exact upstream influence, copied notice, license gates and source scan.
- `docs/INTEGRATION_PARITY.md`: integration boundaries and negative tests.
- `docs/SCHEMA_CONTRACT.md`: required read-only SQLite tables and columns, plus fixture limits.
- `docs/NATIVE_SMOKE.md`: real hosted WebView2 launch, IPC assertions, and synthetic capture provenance.
- `docs/DEPENDENCIES.md`: direct dependency license register and unresolved distribution gates.

The repository-root [GitHub Actions workflow](../.github/workflows/rust-candidate-verify.yml) ran successfully on [draft PR #3](https://github.com/Gerbzz/DemoStrike/pull/3) at commit `5d9d80a` on hosted Windows: frontend typecheck/build, score contract, Rust tests, and native executable compilation passed. That run did not launch or package the executable. A newer hosted gate is designed to launch its real WebView2 window, exercise Rust IPC with a sanitized synthetic library, and publish app-content captures if it succeeds; its result must be checked before claiming runtime or visual acceptance. The workflow has read-only repository permissions and no executable artifact or release step. Pinned lockfiles and transitive license review are still required before distribution.

The current Rust service contract is deliberately narrower than the upstream PostgreSQL, analyzer, Game Coordinator, and plugin contracts. Their gaps are tracked in the ledger and parity document.
