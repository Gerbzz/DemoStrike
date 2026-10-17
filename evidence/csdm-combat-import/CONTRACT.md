# Bounded CSDM combat import contract

The pinned upstream CSDM analyzer writes `_teams.csv`, `_rounds.csv`, and
`_players.csv`; `src/node/database/matches/insert-match.ts` maps them into
the typed `TeamTable`, `RoundTable`, and `MatchPlayerTable`. Those rows include
stable team names, both half scores, round economy, full player ratings,
clutches, and other measurements. DemoStrike's native `source2-demo` parser
does not currently produce that entire analyzer model. This migration keeps
the upstream React route, table, and tab foundations, and supplies only the
bounded fields below through the existing native DemoStrike import pipeline.

| UI fact | Native source | Rule |
| --- | --- | --- |
| Round winner and ticks | GameRules round transitions | Preserve observed CT/T winner and tick boundaries. |
| Observed CT/T players | Controller identity at freeze end and combat events | Side is per round; never promoted to stable Team A/B identity. Conflicts become unavailable. |
| Enemy eliminations | All non-warmup `player_death` events | Count only distinct attacker/victim identities with different observed sides. Unknown side makes that player's total unavailable. |
| Deaths | Victim in all non-warmup `player_death` events | World and self deaths still count for the victim. |
| Enemy assists | Resolved assister in non-warmup `player_death` | Count only a distinct assister on the opposite observed side from the victim. |
| Raw enemy damage | All non-warmup `player_hurt` events | Sum raw `dmg_health` only for distinct players with different observed sides. This can include overkill and is not CSDM's capped effective health damage. Unknown side makes that player's total unavailable. |

The versioned `player_round_combat` table is written in the same transaction
as match, round, player, and utility data. Its nullable metrics preserve
uncertainty. The read-only Tauri view exposes these rows only for schema v7+
and parser capability `native-combat-full-v2`; historical
`native-combat-flash-v1` and other imports remain explicitly unavailable.
Hosted synthetic facts instead use `test-synthetic-combat-v1`; they remain
readable for UI contract tests but never claim native decoder provenance.
The original utility attribution view remains independent of these player
combat totals. The import command checks source identity, snapshots the demo,
parses it with the native parser, and commits the managed library without
replacing earlier imports or annotations.

Stable team identities, official CSDM `kill_count`/rating formulas, economy,
headshots, clutch statistics, and player score are not inferred from these
events. The UI names its narrower enemy-event counts accordingly. The native
parser may reject an unsupported combat identity/schema before saving; the
error leaves the source and existing match rows intact.

The checked-in JSON and tiny PBDEMS2-header test input are public synthetic
fixtures. The hosted native smoke compiles a `synthetic-import-smoke` feature
that supplies those facts to the **same UI command and core transaction**.
The normal app build has no synthetic parser feature and uses the native CS2
decoder. Passing synthetic smoke proves the import wiring and UI contract,
not that this new parser version successfully decodes Neil's private demo.

A private read-only inspection of the already available v6 library found one
`native-combat-flash-v1` match, 10 players, 16 rounds, and 253 utility events,
with its source `.dem` still present. It was not migrated or uploaded. A
current-build private native GUI import remains unverified under the task's
no-local-build and no-executable-download constraint.
