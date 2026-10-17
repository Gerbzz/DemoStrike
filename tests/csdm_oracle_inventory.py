"""Inventory a pinned CSDM analyzer export beside DemoStrike's public import.

This discovery step does not approve or regenerate expected values. A later
comparison must select shared facts and review every mismatch explicitly.
"""

import json
import os
from pathlib import Path
import sqlite3


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
print(json.dumps(inventory, indent=2, ensure_ascii=False))
