"""Compare sampled native viewer coordinates with pinned CSDM CSV positions.

The upstream export is kept only on the hosted runner. This report contains
counts and coordinate differences, never source demo data or a bulk CSV copy.
"""
import csv
import json
import os
from pathlib import Path
import sqlite3


root = Path(os.environ["RUNNER_TEMP"])
database = sqlite3.connect(root / "oracle-demostrike.sqlite")
database.row_factory = sqlite3.Row
rows = database.execute("""SELECT tick, steamid, x, y, z FROM viewer_player_positions
                           WHERE round_number IN (1, 2, 3)
                           ORDER BY tick, steamid""").fetchall()
assert len(rows) > 1000, "Native frame stream is too sparse for viewer acceptance"
stride = max(1, len(rows) // 600)
native = {(row["tick"], row["steamid"]): tuple(row[axis] for axis in "xyz")
          for row in rows[::stride]}
assert len(native) >= 100

oracle_dir = root / "csdm-position-oracle"
position_files = list(oracle_dir.rglob("*_positions.csv"))
def header(file):
    with file.open(newline="", encoding="utf-8") as handle:
        return set(next(csv.reader(handle)))

player_files = [file for file in position_files if {"steamid", "is alive", "health"} <= header(file)]
assert len(player_files) == 1, [str(file) for file in position_files]
matched = {}
with player_files[0].open(newline="", encoding="utf-8") as file:
    for row in csv.DictReader(file):
        key = (int(row["tick"]), row["steamid"])
        if key in native:
            expected = native[key]
            observed = tuple(float(row[axis]) for axis in "xyz")
            matched[key] = max(abs(a - b) for a, b in zip(expected, observed))

grenade_rows = database.execute("""SELECT tick,x,y,z FROM viewer_grenade_positions
                                   WHERE round_number IN (1,2,3) ORDER BY tick""").fetchall()
assert grenade_rows, "Native projectile stream is absent"
grenade_ticks = {row["tick"] for row in grenade_rows[::max(1, len(grenade_rows) // 300)]}
grenade_file = [file for file in position_files if {"grenade id", "projectile id"} <= header(file)]
assert len(grenade_file) == 1, [str(file) for file in position_files]
oracle_grenades = {}
with grenade_file[0].open(newline="", encoding="utf-8") as file:
    for row in csv.DictReader(file):
        tick = int(row["tick"])
        if tick in grenade_ticks:
            oracle_grenades.setdefault(tick, []).append(tuple(float(row[axis]) for axis in "xyz"))
grenade_matches = []
for row in grenade_rows:
    if row["tick"] not in oracle_grenades:
        continue
    xyz = tuple(row[axis] for axis in "xyz")
    grenade_matches.append(min(max(abs(a - b) for a, b in zip(xyz, observed))
                               for observed in oracle_grenades[row["tick"]]))

report = {
    "nativePlayerRowsFirstThreeRounds": len(rows),
    "sampledPlayerKeys": len(native),
    "sharedPlayerKeys": len(matched),
    "playerMaxAxisDifference": max(matched.values(), default=None),
    "playerWithin001Units": sum(value <= 0.001 for value in matched.values()),
    "nativeGrenadeRowsFirstThreeRounds": len(grenade_rows),
    "sharedGrenadeTicks": len(oracle_grenades),
    "grenadeComparisons": len(grenade_matches),
    "grenadeWithin16Units": sum(value <= 16 for value in grenade_matches),
    "grenadeMinAxisDifference": min(grenade_matches, default=None),
    "grenadeMaxAxisDifference": max(grenade_matches, default=None),
}
(root / "csdm-position-comparison.json").write_text(json.dumps(report, indent=2) + "\n")
print("CSDM_POSITION_COMPARISON", json.dumps(report))
assert len(matched) == len(native), "Native player tick/identity samples missing from CSDM"
assert report["playerWithin001Units"] == len(native), "Player world coordinates disagree with CSDM"
assert len(grenade_matches) >= 5, "Too few overlapping native and CSDM projectile ticks"
assert report["grenadeWithin16Units"] == len(grenade_matches), "Projectile world coordinates disagree with CSDM"
