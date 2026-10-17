# First slice specification

Baseline: upstream CS Demo Manager commit `7270bc95c8cab87b7f9ddd490fea13c6eda3ace7` (3.20.1), MIT. Candidate project is isolated from all existing repositories. Preserve the MIT notice with upstream derived work. Third party submodules and binaries require separate license review before reuse.

## Implemented-slice acceptance

1. User supplies a path to an existing DemoStrike SQLite database. Opening must use SQLite read-only mode and reject missing files, malformed data, and unexpected schema with actionable errors.
2. Match library lists imported matches, player/round/utility counts, map, score, and import time. Search filters visible rows without changing data.
3. Selecting a match shows overview, rounds, players, and utility events. A round filter narrows events. Refresh rereads source data.
4. Empty library, no search matches, loading, and error states are explicit and recoverable. All controls are keyboard reachable and visibly focused.
5. The UI must never imply that this read-only adapter performs upstream demo analysis, PostgreSQL management, tick navigation, game launch, recording, or video editing.

## Required later parity

Port or explicitly bridge upstream analyzer, PostgreSQL store, Steam Game Coordinator helper, CS2/CSGO plugins, preload APIs, UUID-correlated WebSocket replies and push events, all upstream routes, and game/capture lifecycle. `docs/MIGRATION_LEDGER.csv` records each upstream source entry. Tick actions, playback, capture, and crash restoration require real-game acceptance and are outside this slice.

## Test boundary

Rust unit tests define read-only and data-shape behavior. A Python read-only verifier checks the real existing Cache import without invoking a build. Rust, TypeScript, native window, and visual tests remain unrun until an authorized environment can build without crossing the 20 GiB free-space floor.
