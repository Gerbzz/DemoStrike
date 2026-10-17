"""Inventory a pinned CSDM analyzer export beside DemoStrike's public import.

This discovery step does not approve or regenerate expected values. A later
comparison must select shared facts and review every mismatch explicitly.
"""

import json
import os
from pathlib import Path
import sqlite3
from collections import Counter


root = Path(os.environ.get("ORACLE_RUN_ROOT") or os.environ["RUNNER_TEMP"])
exports = list((root / "csdm-oracle").rglob("*.json"))
if len(exports) != 1:
    raise RuntimeError(f"Expected one analyzer JSON export, found {len(exports)}")
oracle_file = exports[0]
if oracle_file.stat().st_size > 20_000_000:
    raise RuntimeError("Analyzer JSON exceeded the 20 MB discovery cap")
oracle = json.loads(oracle_file.read_text(encoding="utf-8"))


def shape(value):
    if isinstance(value, dict):
        return {"type": "object", "keys": sorted(value), "fields": {
            key: shape(child) for key, child in value.items()
            if isinstance(child, (list, dict))
        }}
    if isinstance(value, list):
        return {"type": "array", "length": len(value), "first": shape(value[0]) if value else None}
    return {"type": type(value).__name__}


database = root / "oracle-demostrike.sqlite"
with sqlite3.connect(f"file:{database.as_posix()}?mode=ro", uri=True) as connection:
    counts = {
        table: connection.execute(f"SELECT COUNT(*) FROM {table}").fetchone()[0]
        for table in (
            "matches", "rounds", "players", "player_round_combat",
            "raw_combat_hurts", "raw_combat_deaths", "utility_events",
        )
    }
    rounds = [
        dict(zip(("number", "winner", "start", "end", "reason"), row))
        for row in connection.execute(
            "SELECT round_number, winner, start_tick, end_tick, end_reason "
            "FROM rounds ORDER BY round_number"
        )
    ]
    native_hurts = [
        dict(zip(("roundNumber", "tick", "attackerSteamId", "victimSteamId", "healthDamage"), row))
        for row in connection.execute(
            "SELECT round_number, tick, attacker_steamid, victim_steamid, raw_dmg_health "
            "FROM raw_combat_hurts ORDER BY source_index"
        )
    ]
    native_deaths = [
        dict(zip(("roundNumber", "tick", "killerSteamId", "victimSteamId", "assisterSteamId"), row))
        for row in connection.execute(
            "SELECT round_number, tick, attacker_steamid, victim_steamid, assister_steamid "
            "FROM raw_combat_deaths ORDER BY source_index"
        )
    ]
    native_players = {
        steamid: dict(zip(("enemyKills", "enemyAssists", "deaths", "enemyHealthDamage"), row[1:]))
        for row in connection.execute(
            "SELECT steamid, SUM(enemy_kills), SUM(enemy_assists), SUM(deaths), "
            "SUM(enemy_health_damage) FROM player_round_combat GROUP BY steamid"
        ) for steamid in [row[0]]
    }


def key(event, fields):
    return tuple(str(event.get(field) or "") for field in fields)


def event_differences(oracle_events, native_events, fields):
    upstream = Counter(key(event, fields) for event in oracle_events)
    native = Counter(key(event, fields) for event in native_events)
    return {
        "upstreamCount": len(oracle_events),
        "nativeCount": len(native_events),
        "upstreamOnlyCount": sum((upstream - native).values()),
        "nativeOnlyCount": sum((native - upstream).values()),
        "upstreamOnlySamples": list((upstream - native).elements())[:12],
        "nativeOnlySamples": list((native - upstream).elements())[:12],
        "fields": fields,
    }


round_fields = (
    ("number", "number"), ("startTick", "start"), ("endTick", "end"),
)
round_comparison = []
for upstream, native in zip(oracle.get("rounds", []), rounds):
    differences = {
        original: {"upstream": upstream.get(original), "native": native.get(local)}
        for original, local in round_fields
        if str(upstream.get(original)) != str(native.get(local))
    }
    if differences:
        round_comparison.append({"round": native["number"], "differences": differences})

# Original analyzer exports Source 2 enum numbers. Keep the mapping explicit,
# sourced from the pinned analyzer and GameRules round outcome contract.
side_names = {2: "T", 3: "CT"}
reason_names = {1: "bomb_exploded", 7: "bomb_defused", 8: "t_killed",
                9: "ct_killed", 12: "time_ran_out", 17: "t_surrender",
                18: "ct_surrender"}
for upstream, native in zip(oracle.get("rounds", []), rounds):
    differences = {}
    for source, target, mapping in (("winnerSide", "winner", side_names),
                                    ("endReason", "reason", reason_names)):
        translated = mapping.get(upstream.get(source), "unmapped")
        if translated != native.get(target):
            differences[source] = {"upstream": upstream.get(source),
                                   "mapped": translated, "native": native.get(target)}
    if differences:
        item = next((item for item in round_comparison if item["round"] == native["number"]), None)
        if item is None:
            round_comparison.append({"round": native["number"], "differences": differences})
        else:
            item["differences"].update(differences)

damage_fields = ("roundNumber", "tick", "attackerSteamId", "victimSteamId")
upstream_damage = {}
native_damage = {}
for target, events in ((upstream_damage, oracle.get("damages", [])),
                       (native_damage, native_hurts)):
    for event in events:
        target.setdefault(key(event, damage_fields), []).append(event.get("healthDamage"))
damage_amount_differences = [
    {"event": identity, "upstreamEffective": sorted(upstream_damage[identity]),
     "nativeRaw": sorted(native_damage[identity])}
    for identity in upstream_damage.keys() & native_damage.keys()
    if sorted(upstream_damage[identity]) != sorted(native_damage[identity])
]
damage_amount_differences.sort(key=lambda item: item["event"])

# The native v8 ledger intentionally retains all raw deaths. Its displayed
# player-round totals now assign inter-round deaths to the preceding completed
# round and omit post-match events; compare that equivalent match scope.
def match_round(event):
    round_number = event["roundNumber"]
    current = next((item for item in rounds if item["number"] == round_number), None)
    if current is None:
        return None
    if current["start"] is not None and event["tick"] < current["start"]:
        earlier = [item for item in rounds if item["number"] < round_number
                   and item["end"] is not None and item["end"] <= event["tick"]]
        return max(earlier, key=lambda item: item["end"])["number"] if earlier else None
    return round_number

in_match_deaths = [dict(event, roundNumber=match_round(event))
                   for event in native_deaths if match_round(event) is not None]

player_comparison = []
for steamid, upstream in sorted(oracle.get("players", {}).items()):
    native = native_players.get(steamid, {})
    player_comparison.append({
        "steamId": steamid,
        "name": upstream.get("name"),
        "upstream": {name: upstream.get(name) for name in ("killCount", "assistCount", "deathCount", "healthDamage")},
        "native": native,
    })

comparison = {
    "status": "unreviewed independent comparison; differences do not update any expected fixture",
    "roundFieldDifferences": round_comparison,
    "damageEventIdentityWithoutAmount": event_differences(
        oracle.get("damages", []), native_hurts,
        damage_fields,
    ),
    "damageAmountSemantics": {"upstream": "effective health removed",
                              "native": "raw dmg_health including overkill",
                              "differentEventCount": len(damage_amount_differences),
                              "samples": damage_amount_differences[:12]},
    "killVsInMatchDeathIdentity": event_differences(
        oracle.get("kills", []), in_match_deaths,
        ("roundNumber", "tick", "killerSteamId", "victimSteamId", "assisterSteamId"),
    ),
    "retainedPostMatchRawDeaths": len(native_deaths) - len(in_match_deaths),
    "playerTotals": player_comparison,
}

inventory = {
    "status": "discovery-only: no expected values were regenerated or approved",
    "oracleSource": "akiver/cs-demo-analyzer@61a1208e71a00255846a7e0e829405052c11c7cd (v1.11.0)",
    "oracleExportBytes": oracle_file.stat().st_size,
    "oracleShape": shape(oracle),
    "demoStrikeCounts": counts,
    "demoStrikeRounds": rounds,
}
output = root / "csdm-oracle-inventory.json"
output.write_text(json.dumps(inventory, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
comparison_output = root / "csdm-oracle-comparison.json"
comparison_output.write_text(json.dumps(comparison, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
print(json.dumps({"inventory": {
    "upstreamRoundCount": len(oracle.get("rounds", [])),
    "upstreamKillCount": len(oracle.get("kills", [])),
    "upstreamDamageCount": len(oracle.get("damages", [])),
    "demoStrikeCounts": counts,
}, "comparison": comparison}, indent=2, ensure_ascii=False))
