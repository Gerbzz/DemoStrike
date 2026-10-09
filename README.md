# DemoStrike

Demo and grenade analysis prototype. The native Rust parser currently accepts three hash-validated evaluation demos and reports grenade-only capabilities. No demo files or extracted facts are included. The optional reference extractor needs a separately configured local Node runtime and CsThrowBack extractor.

This repository uses the public `clipstrike-core` crate from Clipstrike commit `f5dd0a3dceeb5ee1196e654210cc7af0ca5ee97a` for the capture package handoff. See `ATTRIBUTION.md` for source provenance.
The [capture package v1 specification](docs/CAPTURE_PACKAGE_V1.md) describes the handoff and its validation limits.

## Build and test

On Windows with a Rust 2024 toolchain:

```powershell
cargo test --locked
cargo build --locked --features native-ui --bins
```

The code is a local prototype. Native windows, installers, and full production workflows have not passed clean-machine acceptance. Project source in this private repository has no public license grant. Third-party crates retain their own licenses.
