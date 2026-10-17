"""Inventory a pinned CSDM analyzer export beside DemoStrike's public import.

This discovery step does not approve or regenerate expected values. A later
comparison must select shared facts and review every mismatch explicitly.
"""

import json
import os
from pathlib import Path
import sqlite3
from collections import Counter


root = Path(os.environ["RUNNER_TEMP"])
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
    ("number", "number"), ("winnerSide", "winner"),
    ("startTick", "start"), ("endTick", "end"), ("endReason", "reason"),
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
    "damageEventIdentity": event_differences(
        oracle.get("damages", []), native_hurts,
        ("roundNumber", "tick", "attackerSteamId", "victimSteamId", "healthDamage"),
    ),
    "killVsRawDeathIdentity": event_differences(
        oracle.get("kills", []), native_deaths,
        ("roundNumber", "tick", "killerSteamId", "victimSteamId", "assisterSteamId"),
    ),
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
