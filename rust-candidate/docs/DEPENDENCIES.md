# Direct dependency and notice register

This is a source-only candidate. The declarations in `package.json` and `src-tauri/Cargo.toml` are not an approved distributable dependency set. Public npm and crates.io version metadata were checked on 17 October 2026 for the direct packages below. The hosted workflow also uploads a resolved dependency metadata manifest for review. A release needs pinned lockfiles, transitive notice review, and runtime/package testing.

| Direct JavaScript package | Declared range | License metadata |
| --- | --- | --- |
| `@tauri-apps/api`, `@tauri-apps/plugin-dialog`, `@tauri-apps/cli` | `^2.0.0` | Apache-2.0 OR MIT |
| `react`, `react-dom` | `^19.0.0` | MIT |
| `@types/react`, `@types/react-dom` | `^19.0.0` | MIT |
| `@vitejs/plugin-react` | `^5.0.0` | MIT |
| `vite` | `^7.0.0` | MIT |
| `typescript` | `^5.9.0` | Apache-2.0 |
| `playwright-core` | `^1.56.0` | Apache-2.0; CI-only WebView2 automation, no browser binary is bundled. |

| Direct Rust crate | Declared range | License metadata |
| --- | --- | --- |
| `tauri`, `tauri-plugin-dialog`, `tauri-build` | `2` | Apache-2.0 OR MIT |
| `rusqlite` | `0.32` | MIT |
| `serde` | `1` | MIT OR Apache-2.0 |
| `tempfile` | `3` | MIT OR Apache-2.0; tests only. |

The copied CS Demo Manager match-score source has its separate, exact MIT notice in `LICENSE.upstream`. The approved DemoStrike icon is derived from the repository's own transparent logo, not from an upstream game asset. The public sources for dependency metadata are [npm registry](https://registry.npmjs.org/), [crates.io](https://crates.io/), and [Microsoft Playwright's license](https://github.com/microsoft/playwright/blob/main/LICENSE). No third-party logo, screenshot, demo, SDK, executable or game content is included.
