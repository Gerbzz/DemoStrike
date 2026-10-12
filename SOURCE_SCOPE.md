# Public source boundary

Included: first-party Rust source, the native DemoStrike UI and CLI, the local FilmCraft UI adaptation and necessary public editor source, pinned public Cargo dependencies, the tested capture/QC source script, optional offline runtime staging helper, original third-party licenses/notices, and synthetic text fixtures.

Excluded: original private CsThrowBack checkout; real CS2 demos and decoded facts; HLAE/CS2 binaries; recorded MP4/WAV and proof PNG; runtime request/receipt/log/SQLite files; compiled executables and targets; credentials; machine-specific paths; opt-in tests that require retained real demos. The original private checkout, user project files, launcher and verified executable remain untouched.

The old ClipStrike revision stays reachable through public Git history. This candidate is a normal descendant of the current public DemoStrike branch; its source does not publish the private CsThrowBack repository or declare a new first-party license.
