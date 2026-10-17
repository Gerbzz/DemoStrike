# Source and asset reuse audit

Baseline reviewed: public `akiver/cs-demo-manager` commit `7270bc95c8cab87b7f9ddd490fea13c6eda3ace7`. The pinned root license is MIT with Copyright (c) 2014-present AkiVer; its full notice is preserved in `LICENSE.upstream`.

| Candidate content | Source relationship | Actual reuse |
| --- | --- | --- |
| `src/App.tsx` | Workflow and information hierarchy inspected in upstream `src/ui/matches/matches.tsx`, `matches/table/matches-table.tsx`, and `match/overview/match-overview.tsx`. | New React/TypeScript implementation against the candidate's narrower SQLite command contract. No upstream component file is copied verbatim. Upstream Redux, PostgreSQL, WebSocket, table framework, routing and match statistics are not ported. |
| `src/styles.css`, `DESIGN.md` | Upstream `src/ui/styles/variables.css` and `base.css` informed density and state coverage. User requested a softer VS Code Dark Modern-like palette. | New CSS. No upstream stylesheet, font, icon, logo, map art or screenshot copied. |
| `src/bridge.ts` | Upstream `src/preload/preload.ts` and `src/ui/web-socket-client.ts` establish future behavior to migrate. | New two-command Tauri adapter. It does not implement or emulate `window.csdm` or UUID-correlated WebSocket messages. |
| `src-tauri/src/library.rs` | Reads the current DemoStrike SQLite schema. | New read-only adapter; no CS Demo Manager PostgreSQL or Go analyzer code copied. |
| `src-tauri/src/capture_provider.rs` | Inspected current CsThrowBack HLAE capture behavior and independent HLAE license boundaries. | New pure data contract and semantic tick planner. No HLAE executable, SDK, submodule, camera engine, game hook or code copied. |
| `src/upstream/get-team-score-class-name.ts`, `TeamScore.tsx` | Pinned upstream match score helper and component; exact Git blobs and changes recorded in `UPSTREAM_ADAPTATIONS.md`. | The helper function is copied intact under the upstream MIT notice. `TeamScore` is adapted from the actual upstream JSX and rendered in the candidate match table and overview. Rust returns the matching three-way score relation. |
| `LICENSE.upstream` | Exact upstream root MIT notice. | Verbatim notice included for attribution and any future adapted source distribution. |
| `src-tauri/icons/icon.ico` | Existing approved `DemoStrike/assets/branding/DemoStrike.png` in this repository (SHA-256 `D039582241B04A74D6E8124A0021D22DF91A5ACB1641CCC0F873B72376E4320B`). | Windows icon resource encoded at 256 × 256 from the same art, preserving its transparent corners. The source PNG is unchanged. |

The two score files are the only actual upstream application source slice in this candidate; the larger match screens remain structural adaptations and remain marked unported in the file-by-file ledger. The score rows are marked `adapted_partial` until the Rust tests and native UI are verified. The upstream MIT root does not grant rights to every submodule, game asset, bundled binary, font or third-party dependency. Those are excluded from this candidate. No DemoDesk/AGPL source is included.

Candidate dependencies are declared but not installed or bundled here: Tauri, its dialog plugin, React, Vite, TypeScript, rusqlite, serde and tempfile. Their exact resolved versions and transitive notices require a generated lockfile and a hosted dependency/license audit before distribution. The approved DemoStrike source PNG remains at its existing repository path; only the Windows icon encoding is added to this candidate.

The local real-demo fixture is outside this repository. `tests/verify_cache_fixture.py` accepts its path as an argument; the candidate contains only aggregate test expectations. A targeted source scan found no recognizable credentials, tokens, personal absolute paths, original `.dem`, SQLite database or native screenshots in tracked candidate files. This is a source audit, not a claim of a completed binary dependency audit.
