# DemoStrike source provenance and license scope

## First-party and private-source history

The earlier public DemoStrike source already reused these selected Rust modules from Neil's private `Gerbzz/csthrowback` commit `9559eb710d563aaa1d9262b2df9445432429d394`:

- `src-tauri/src/analysis/mod.rs` -> `src/analysis/mod.rs`
- `src-tauri/src/domain/mod.rs` -> `src/domain/mod.rs`
- `src-tauri/src/parser/raw.rs` -> `src/parser/raw.rs`
- `src-tauri/src/db/mod.rs` -> `src/db/mod.rs`
- `src-tauri/src/db/queries.rs` -> `src/db/queries.rs`
- `src-tauri/src/error.rs` -> `src/error.rs`

`src/demo_import.rs` adapts that project's parser and staged-import flow. The private repository has no root LICENSE or NOTICE in the reviewed checkout. Its full source, Tauri/React UI, Node extractor and demo corpus are not published here. The DemoStrike Rust UI, capture integration and other first-party adaptations have **no new license grant in this candidate**. Publication as source does not itself decide reuse terms or transfer ownership.

## Public FilmCraft and ClipStrike components

The embedded editor is adapted from public [Gerbzz/Clipstrike commit `4be130f016fe7a0a3b7ae5d59a037c299594aecb`](https://github.com/Gerbzz/Clipstrike/commit/4be130f016fe7a0a3b7ae5d59a037c299594aecb). The copied source under `third_party/filmcraft_source` retains its `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`, `ATTRIBUTION.md`, and font OFL notices. `third_party/filmcraft` also retains the donor license/notice texts. `src/filmcraft_audio.rs` copies that commit's `apps/filmcraft/src/audio.rs`; `src/filmcraft_legacy.rs` adapts its legacy importer, and the local UI crate adds DemoStrike host branding. Its public synthetic fixture and attribution sidecar are in `tests/fixtures`.

The selected FilmCraft donor source is MIT OR Apache-2.0, but those inherited texts do not establish terms for first-party ClipStrike or DemoStrike modifications. The earlier public ClipStrike crates are pinned to [commit `fdf2f6ac8828374b803577ae1f6b310b73860075`](https://github.com/Gerbzz/Clipstrike/commit/fdf2f6ac8828374b803577ae1f6b310b73860075); `third_party/clipstrike_legacy/LICENSE-MIT` preserves that public repository's Gerbzz MIT text. Neither historical license file is presented as a blanket license for this combined repository.

The `.cstrike` name and ClipStrike strings in migrated source-format metadata are historical provenance. DemoStrike supplies its own UI host brand. No raw match data, donor ArtCraft marks, original private source checkout or contributor personal metadata is included.

## Approved full-page icon exports — 2026-10-25

Original approved artwork; exact master and derived hashes are recorded in `assets/branding/platform/manifest.json`. No inherited asset or licence is replaced.

| `assets/branding/DemoStrike-logo.svg` | DemoStrike contributors | Derived from approved `assets/branding/DemoStrike-logo.svg`, unchanged page geometry | Existing original Strike artwork terms; no new licence grant | Runtime/package branding; native acceptance unknown |
| `assets/branding/platform/DemoStrike.png` | DemoStrike contributors | Derived from approved `assets/branding/DemoStrike-logo.svg`, unchanged page geometry | Existing original Strike artwork terms; no new licence grant | Runtime/package branding; native acceptance unknown |
| `assets/branding/platform/DemoStrike.ico` | DemoStrike contributors | Derived from approved `assets/branding/DemoStrike-logo.svg`, unchanged page geometry | Existing original Strike artwork terms; no new licence grant | Runtime/package branding; native acceptance unknown |
| `rust-candidate/src-tauri/icons/demostrike-approved.ico` | DemoStrike contributors | Derived from approved `assets/branding/DemoStrike-logo.svg`, unchanged page geometry | Existing original Strike artwork terms; no new licence grant | Runtime/package branding; native acceptance unknown |
