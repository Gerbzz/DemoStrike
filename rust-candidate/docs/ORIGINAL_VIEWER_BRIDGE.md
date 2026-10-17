# Original CSDM 2D viewer bridge

Source pin: `akiver/cs-demo-manager@7270bc95c8cab87b7f9ddd490fea13c6eda3ace7` (MIT). The original source snapshot is left intact under `rust-candidate/upstream/cs-demo-manager`.

## Original components now mounted

The `viewer-2d` route mounts the pinned `Viewer2D` component. It retains the original `FullscreenProvider`, `useInteractiveMapCanvas` zoom and pan behavior, map coordinate transforms, player and grenade drawing hooks, death markers, drawing canvas, timer, kill feed, `RoundsBar`, `PlaybackBar`, speed and round controls, and `Timeline`. The 2D Viewer tab remains nested under the original match route.

## Native bridge

- `native-original-viewer-2d.tsx` fetches the persisted v10 round stream in bounded 512-tick IPC chunks and adapts recorded fields into the original viewer context. Source ticks, world coordinates, player identity, side, health, yaw, alive state, projectile identity/type, and raw deaths are retained. Position rows are not synthetic. The adapter loads one round at a time, with visible progress and recoverable errors.
- `native-original-viewer-events.ts` maps persisted, position-backed smoke, flash, HE and decoy detonations into the original drawing hooks. It links a projectile only when recorded entity, thrower and tick window agree. A separate seekable event list exposes exact grenade, damage, death and bomb ticks without inventing an effect when coordinates or identity are missing.
- Native v11 imports retain Source 2 bomb planted, defused and exploded game-event ticks, nullable actors, raw site IDs and observed positions. Original bomb timeline markers appear only for a single observed plant with complete coordinates. Unknown A/B bombsite names display as `?`; old imports report the bomb stream unavailable until reimport.
- If Source 2 `m_bPawnIsAlive` is missing or the wrong type, the native stream keeps a nullable state. The original living-player hook draws confirmed living positions; a ported wrapper draws unknown-state positions as neutral hollow markers, and the roster says `Alive unavailable`. A same-tick death event still overrides stale alive data.
- `native-original-viewer-match.ts` supplies the original components' consumed match and round fields. Team A/B scores are derived only from earlier rounds with known side and winner; ambiguity displays `?`. An unobserved official end tick is not claimed separately from the native recorded end tick.
- `native-original-viewer-radar.tsx` replaces only the upstream Electron `file://` radar loader. The user selects a local 1024 x 1024 PNG, held in WebView memory. The grid is a truthful coordinate fallback. The original canvas and transforms draw the positions.
- `native-original-team-panel.tsx` replaces the upstream panel because v10 has no per-frame money, armor, weapon, bomb carrier or kit observations. It displays only recorded side, name, alive state and health. The original audio selector is hidden because the voice extraction backend and audio bytes are unported.

## Boundaries

The native parser retains its two-million-player-sample cap. If exceeded, the match import and prior utility attribution remain intact, but incomplete replay is unavailable. Old imports remain readable and need reimport for positions and phase provenance. Unknown GameRules phase, match-start context, and side switches keep Team A/B attribution unavailable instead of guessing. Mirage and Ancient public fixtures end before halftime; side-switch tests are synthetic.

The native stream does not yet supply shots, inferno cells, chicken/hostage positions, equipment, audio, flash duration, scope state, or advanced kill flags. Bomb carrier state, bombsite A/B mapping and smoke expiration are unavailable; a recorded detonation is not treated as a complete smoke or fire area. The original rendering hooks remain mounted, but those effects have no native facts to draw. The viewer and recorded-event list name the unavailable groups. The full upstream Electron/WebSocket/PostgreSQL data pipeline and original populated match model are not ported. Do not treat this bridge as full upstream feature parity.

No Valve radar image is committed. Hosted visual tests fetch the pinned public CSDM Mirage radar into runner temporary storage, check its SHA-256, and capture only the running Tauri WebView2 application.
