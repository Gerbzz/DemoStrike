# CS Demo Manager upstream inventory

This is the source inventory for the separate DemoStrike Rust candidate. It is a migration plan, not a claim that CS Demo Manager has been ported. The proposed architecture is a Tauri shell with a Rust service and an adapted React/TypeScript UI. The existing DemoStrike and CSthrowback capture and FilmCraft editor remain separate integration boundaries.

## Immutable upstream baseline

- Repository: [akiver/cs-demo-manager](https://github.com/akiver/cs-demo-manager)
- Pinned commit: [`7270bc95c8cab87b7f9ddd490fea13c6eda3ace7`](https://github.com/akiver/cs-demo-manager/commit/7270bc95c8cab87b7f9ddd490fea13c6eda3ace7), authored 2026-10-15.
- Application version at that commit: `3.20.1` in the [pinned package manifest](https://github.com/akiver/cs-demo-manager/blob/7270bc95c8cab87b7f9ddd490fea13c6eda3ace7/package.json).
- [Recursive Git tree](https://api.github.com/repos/akiver/cs-demo-manager/git/trees/7270bc95c8cab87b7f9ddd490fea13c6eda3ace7?recursive=1): 3,551 tree entries, including 3,168 tracked non-directory entries; GitHub reports `truncated: false`.
- [MIGRATION_LEDGER.csv](MIGRATION_LEDGER.csv) records every tracked non-directory entry in that exact tree with its Git object SHA (blob or submodule commit), category, parity track, proposed destination, port status, test evidence, gap, and license gate. SHA-256 of this ledger: `39CBACA23A15D96BBBE85AE1015D3A877BC7AB63277E3BBD9B59B5B79357F006`.

The ledger marks 3,126 entries `unported`, 40 `not_applicable`, and two upstream match-score source entries `adapted_partial`. A proposed path groups files by responsibility; it does not promise one Rust file per TypeScript file. Most React UI files point to a prospective UI tree; the two score files point to their actual `src/upstream` location. Node, server, preload, Electron, CLI, and shared logic point to prospective `src-tauri/src` modules. Generated files, binary artifacts, and third-party submodule pointers are not treated as application source to translate.

### Coverage by tracked-entry category

| Category | Entries | Meaning |
| --- | ---: | --- |
| Upstream UI source | 1,497 | React/TypeScript/CSS/HTML surfaces and support |
| Upstream Node source | 763 | Demo analysis orchestration, PostgreSQL, downloads, Steam, video, game integration |
| Upstream service source | 162 | Daemon, WebSocket, handlers, tasks |
| Upstream shared source | 160 | Shared types and logic |
| Upstream CLI source | 29 | Commands and terminal behavior |
| Upstream Electron source | 25 | Window, menu, updater, app lifecycle |
| Upstream preload source | 4 | Electron bridge |
| Upstream type source | 11 | Additional declarations |
| Upstream tests | 21 | Existing test source; no equivalent tests passed yet |
| Native plugin source | 18 | Separate CS2 and CSGO plugin tracks |
| Asset or fixture | 372 | Visual, map, and other data requiring provenance review |
| Compiled binary | 21 | Bundled platform/game binaries; no reuse planned |
| Git submodule pointer | 6 | External projects at pinned SHAs |
| Generated fixtures or snapshots | 12 | Recreate or compare as evidence |
| Localization catalog | 2 | Adapt and verify separately |
| Build, automation, manifest, lock, docs, configuration | 65 | Tooling or reference, not ported behavior |
| **Total** | **3,168** | Complete non-directory Git tree |

## Principal feature and transport tracks

| Upstream source | Prospective destination | Required parity evidence |
| --- | --- | --- |
| [`src/ui/renderer.tsx`](https://github.com/akiver/cs-demo-manager/blob/7270bc95c8cab87b7f9ddd490fea13c6eda3ace7/src/ui/renderer.tsx), `src/ui/bootstrap/`, `src/ui/router.tsx` | `ui/src` adapted for Tauri | Actual navigation, loading/error states, keyboard/focus and theme captures |
| `src/ui/demos/`, `src/ui/matches/`, `src/ui/match/` | `ui/src` library and match views | Import/list/detail/round/player/event workflows on real and malformed demos |
| `src/ui/styles/variables.css`, `base.css`, `index.css` | `ui/src/styles` | Softer VS Code Dark Modern-like hierarchy at real window sizes; contrast and focus checks |
| `src/node/demo-analyzer/`, `src/node/demo/`, external `@akiver/cs-demo-analyzer@1.11.0` | Rust parser/import service | Same demo parses with equivalent teams, 10 players, 16 rounds, 253 utility events, tick identities, and explicit negative cases |
| `src/node/database/` (406 tracked entries) and `pg@8.23.1` | Rust storage and migration layer | PostgreSQL schema/query/import behavior, rollback, corrupted/missing data and migration tests; SQLite read-only first slice is a distinct adapter |
| `src/node/boiler/`, external `@akiver/boiler-writter@1.7.0` | Steam Game Coordinator integration | Account/session boundary, demo lookup/download, errors and cancellation |
| `src/node/csgo-voice-extractor/`, external `@akiver/csgo-voice-extractor@3.1.6` | Separate voice extraction track | Verify output modes, timestamps and failure cases without assuming parser parity |
| `src/node/video/`, `src/ui/match/video/` | Existing CSthrowback/FilmCraft boundary plus Rust service adapter | Grenade choice, synchronized views, lineup holds, editable export project and cancel/error cases; no video render in this pass |
| `src/server/`, `src/electron-main/`, `src/preload/`, `src/ui/web-socket-client.ts` | Tauri commands/events and Rust service | Enumerate each IPC/WebSocket contract and preserve async state, errors, cancellation, and permissions |
| `src/cli/` | Rust CLI | Command compatibility and end-to-end demo workflows |
| `cs2-server-plugin/`, `csgo-server-plugin/`, `static/cs2/` | Separate native plugin and protocol evaluation | Game-version compatibility and explicit license/redistribution review before any use |

The first candidate slice consumes an isolated SQLite import made by an **older DemoStrike CLI**. It can demonstrate a read-only imported-match library and overview. It does **not** establish that the new candidate parses demos, reproduces CS Demo Manager's PostgreSQL service, or implements Game Coordinator, server plugins, export, or editor parity.

## Licensing and provenance gates

The pinned repository's [MIT license](https://github.com/akiver/cs-demo-manager/blob/7270bc95c8cab87b7f9ddd490fea13c6eda3ace7/LICENSE) names `Copyright (c) 2014-present AkiVer`. Copies or substantial portions of upstream source need that copyright and permission notice. Record copied files and preserve the notice in candidate distribution. The package manifest's `license: MIT` does not relicense third-party packages, fonts, game assets, or native binaries.

- The pinned [`package.json`](https://github.com/akiver/cs-demo-manager/blob/7270bc95c8cab87b7f9ddd490fea13c6eda3ace7/package.json) declares 92 direct devDependencies, including Electron, `pg`, `@akiver/cs-demo-analyzer`, `@akiver/boiler-writter`, and `@akiver/csgo-voice-extractor`. Repository metadata currently identifies those three AkiVer external projects as MIT, but their package tarballs and transitive dependencies have **not** been audited. Recheck exact versions before reuse.
- [`.gitmodules`](https://github.com/akiver/cs-demo-manager/blob/7270bc95c8cab87b7f9ddd490fea13c6eda3ace7/.gitmodules) references six plugin dependencies. `easywsclient` at `afc1d8cfc584e0f1f4a77e8c0ce3e979d9fe7ce2` has a [MIT COPYING notice](https://github.com/dhbaird/easywsclient/blob/afc1d8cfc584e0f1f4a77e8c0ce3e979d9fe7ce2/COPYING). `nlohmann/json` at `a03e65420ca9a14c97080b67036859aaac06700d` has a [MIT notice](https://github.com/nlohmann/json/blob/a03e65420ca9a14c97080b67036859aaac06700d/LICENSE.MIT). The pinned `hl2sdk` trees and their Valve/other bundled material need separate terms review; no code is copied from them.
- The repository tracks platform binaries under `static/cs2/`, `static/csdm.*`, and `build-assets/`, plus an already-built Node addon. Their redistribution rights and build provenance are not established here. The map/rank/source images, icon artwork, and bundled Inter font likewise require provenance review before copying into the candidate. Design may be recreated without carrying over uncertain assets.
- No DemoDesk/AGPL code or assets are included in this candidate inventory or proposed reuse.

## First acceptance fixtures and limits

The existing verified Cache CS2 demo was imported through an older DemoStrike CLI into an isolated SQLite database. Its verified counts are 10 players, 16 rounds, and 253 utility events. The candidate's first match-library slice must read that database **without modifying the source** and prove match selection and overview navigation. Follow with round/player/event details, duplicate imports, missing/corrupt paths, schema mismatch, incomplete analysis, and order-independent tests. Only a new Rust parser/importer running the same demo and matching recorded identities and counts can be marked parser parity.

This inventory was assembled from the GitHub REST tree and pinned text files. It is not a source checkout, build, runtime test, visual acceptance, or dependency license audit. The parent implementation owns code and test changes.
