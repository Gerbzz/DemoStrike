# DemoStrike source preview

DemoStrike is a local Rust workbench for importing supported CS2 demos, reviewing utility/combat/flash events, and preparing editable clip handoffs. The native parser copies a demo to a private byte snapshot, validates supported facts, and commits an import transactionally. Unsupported schemas and missing facts are rejected rather than guessed. It stores searchable events, favorites and tags in a local database. No Node extractor or `.raw.json` sidecar is needed for the normal Rust import.

**Status: alpha source.** The verified local foundation passed 91 no-UI and 112 native-UI library tests (with opt-in media tests ignored), linked the native executable, and passed a generated three-take technical QC/mux exercise. An earlier retained-media editor proof imported an accepted capture, saved/reopened an editable project and decoded a rendered export. The source-publication candidate removes match-specific proof tests and identifiers; it has not undergone another heavy build. Native pointer/keyboard use, human listening, a new CS2/HLAE capture, broad demo compatibility and release packaging remain unverified. No binary is published here.

## Capture and editor flow

The in-app path can stage a supervised offline pilot, launch owned CS2/HLAE processes in a bounded Windows Job Object, inspect three camera roles, preview muxed media in the embedded FilmCraft editor, and author a reviewed receipt. Technical QC and media decoding do not mark image, motion, POV or audio as human-approved. Sealing checks source hashes, runtime identity, proof and receipt relationships; the new package sidecar can be verified after relocation. Raw HLAE WAVs, logs and proof PNGs are retained separately for an independent evidence audit. The app does not create footage without external CS2/HLAE tools.

The editor is based on public [Gerbzz/Clipstrike FilmCraft source](https://github.com/Gerbzz/Clipstrike/commit/4be130f016fe7a0a3b7ae5d59a037c299594aecb). It is included here only where adapted for DemoStrike host branding; unchanged core crates remain pinned to that exact public commit. Earlier ClipStrike capture crates remain pinned to the preceding public commit `fdf2f6ac8828374b803577ae1f6b310b73860075`. Both revisions remain in ordinary Git history.

## Source and licensing boundary

This repository contains source and synthetic tests. It excludes demos, captured media, user databases, runtime logs, local paths, compiled binaries, credentials and the private CsThrowBack repository. [SOURCE_SCOPE.md](SOURCE_SCOPE.md) lists the exact boundary. [ATTRIBUTION.md](ATTRIBUTION.md) identifies reused CsThrowBack, FilmCraft and older ClipStrike code and their distinct license status.

There is **no root license grant for first-party DemoStrike source**. Public visibility alone does not set reuse terms. The original FilmCraft MIT/Apache texts and notices are preserved for inherited FilmCraft code; the older public ClipStrike MIT text is preserved for its pinned legacy crates. Those files do not automatically license every DemoStrike adaptation. First-party ownership and redistribution terms require an explicit owner decision before any compiled release.

## Reproduce the source checks

Use a reviewed Windows Rust/Cargo toolchain and the committed lockfile. Populate the Cargo cache with `cargo fetch --locked`, then run `cargo test --locked --offline --lib` and `cargo test --locked --offline --features native-ui --lib` with an isolated target and one Cargo job. The full FilmCraft editor and media graph is substantial; allow enough disk headroom. Real-media tests are intentionally absent from this public source boundary. The normal desktop binary requires the `native-ui` feature.
