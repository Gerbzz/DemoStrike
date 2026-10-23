<p align="center">
  <img src="assets/branding/DemoStrike-geometric.svg" alt="DemoStrike: an event diamond locks onto a segmented timeline" width="160" height="160">
</p>

<h1 align="center">DemoStrike</h1>

<p align="center">
  <a href="https://prowebdevsolutions.com"><picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/branding/PWDS-logo-orange.svg">
    <img src="assets/branding/PWDS-logo.png" alt="PWDS company logo" width="120">
  </picture></a>
</p>

**Find the moments worth clipping.**

DemoStrike turns supported CS2 demos into searchable match events. Review combat, utility, and flashes; mark favorites and tags; then prepare a supervised capture and editable handoff for selected moments.

![DemoStrike native desktop app showing its empty analysis workspace](assets/screenshots/demostrike-empty-native-pr2-2026-10-17.png)

*Historical native empty workspace, captured on 17 October 2026 from source `f73f62a` in [PR #2](https://github.com/Gerbzz/DemoStrike/pull/2). PR #2 and its [integration PR #5](https://github.com/Gerbzz/DemoStrike/pull/5) are now merged; this is not a capture of current `main`. No demo, match events, capture footage, or media were loaded. [Capture provenance and limits](docs/NATIVE_UI_PREVIEW.md).*

## Highlights

- Find and organize events in a local match database.
- Track useful moments with favorites and tags.
- Prepare reviewed capture packages for an editor workflow.

## Current status

Alpha source. [PR #2](https://github.com/Gerbzz/DemoStrike/pull/2) and [PR #5](https://github.com/Gerbzz/DemoStrike/pull/5) are merged. The earlier PR #2 source passed a hosted Windows native build and focused binary test, and its executable produced the historical image above. No source-matched screenshot of current `main` is recorded here. Broad demo compatibility, a new CS2/HLAE capture, human media review, and release packaging remain unverified.

## Capture and editor flow

The in-app path can stage a supervised offline pilot, launch owned CS2/HLAE processes in a bounded Windows Job Object, inspect three camera roles, preview muxed media in the embedded FilmCraft editor, and author a reviewed receipt. Technical QC and media decoding do not mark image, motion, POV or audio as human-approved. Sealing checks source hashes, runtime identity, proof and receipt relationships; the new package sidecar can be verified after relocation. Raw HLAE WAVs, logs and proof PNGs are retained separately for an independent evidence audit. The app does not create footage without external CS2/HLAE tools.

The editor is based on public [Gerbzz/Clipstrike FilmCraft source](https://github.com/Gerbzz/Clipstrike/commit/4be130f016fe7a0a3b7ae5d59a037c299594aecb). It is included here only where adapted for DemoStrike host branding; unchanged core crates remain pinned to that exact public commit. Earlier ClipStrike capture crates remain pinned to the preceding public commit `fdf2f6ac8828374b803577ae1f6b310b73860075`. Both revisions remain in ordinary Git history.

## Source and licensing boundary

This repository contains source and synthetic tests. It excludes demos, captured media, user databases, runtime logs, local paths, compiled binaries, credentials and the private CsThrowBack repository. [SOURCE_SCOPE.md](SOURCE_SCOPE.md) lists the exact boundary. [ATTRIBUTION.md](ATTRIBUTION.md) identifies reused CsThrowBack, FilmCraft and older ClipStrike code and their distinct license status.

There is **no root license grant for first-party DemoStrike source**. Public visibility alone does not set reuse terms. The original FilmCraft MIT/Apache texts and notices are preserved for inherited FilmCraft code; the older public ClipStrike MIT text is preserved for its pinned legacy crates. Those files do not automatically license every DemoStrike adaptation. First-party ownership and redistribution terms require an explicit owner decision before any compiled release.

## Reproduce the source checks

Use a reviewed Windows Rust/Cargo toolchain and the committed lockfile. Populate the Cargo cache with `cargo fetch --locked`, then run `cargo test --locked --offline --lib` and `cargo test --locked --offline --features native-ui --lib` with an isolated target and one Cargo job. The full FilmCraft editor and media graph is substantial; allow enough disk headroom. Real-media tests are intentionally absent from this public source boundary. The normal desktop binary requires the `native-ui` feature.

## Verification history

The earlier local foundation passed 91 no-UI and 112 native-UI library tests, linked the native executable, and passed a generated three-take technical QC/mux exercise. A retained-media editor proof imported a capture, saved and reopened an editable project, and decoded a rendered export. The public source candidate removes match-specific proof tests and identifiers and has not been rebuilt. These checks do not establish broad demo support or human approval of capture quality.

## PWDS brand artwork

The company mark in this masthead is the existing [official PWDS logo](https://github.com/Gerbzz/PWDS-Website/blob/main/public/prowebdevsolutions-logo.png), with the [official orange display asset](https://prowebdevsolutions.com/images/prowebdevsolutions-logo-orange.svg) for dark backgrounds. Any app logo above it remains the original Strike suite artwork.
