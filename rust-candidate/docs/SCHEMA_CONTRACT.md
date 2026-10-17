# Read-only DemoStrike library contract

The candidate reads an existing DemoStrike SQLite import. It does not parse a `.dem` file or create/update the database. `src-tauri/src/library.rs` opens the selected file with `SQLITE_OPEN_READ_ONLY`, rejects missing files and missing required columns, and runs the following bounded queries.

| Table | Required columns | Used for |
| --- | --- | --- |
| `matches` | `id`, `file_name`, `map_name`, `score_ct`, `score_t`, `imported_at`, `parser_capability` | Match list and selected overview, ordered by import time and ID. |
| `players` | `match_id`, `steamid`, `name` | Player count and selected match's player list. |
| `rounds` | `match_id`, `round_number`, `winner`, `start_tick`, `end_tick` | Round count and ordered round list. A null winner is shown as unknown. |
| `utility_events` | `id`, `match_id`, `round_number`, `utility_type`, `thrower_name`, `thrower_team`, `throw_tick`, `enemies_blinded`, `damage_enemy` | Event count, selected match's chronological utility list, and type breakdown. |

The adapter makes no claim about every current or future DemoStrike import schema. The local read-only check exercised its six SQL strings against one verified older-CLI Cache import without changing the file. Hosted Rust tests exercise small synthetic schema variants; the WebView2 smoke test generates a separate deterministic synthetic fixture. Neither validates a new parser, a live game, HLAE capture, or full CS Demo Manager parity.

The synthetic fixture's two names, scores and events are invented solely to test the UI and IPC. They must always be labeled **synthetic** in screenshots and reports. The smoke test hashes the fixture before and after interacting with the app and checks that the bytes are unchanged.
