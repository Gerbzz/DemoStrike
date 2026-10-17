"""Create a small, entirely synthetic DemoStrike-schema library for hosted UI checks."""

from __future__ import annotations

import hashlib
import os
from pathlib import Path
import sqlite3


def main() -> None:
    runner_temp = os.environ.get("RUNNER_TEMP")
    if not runner_temp:
        raise SystemExit("RUNNER_TEMP is required; this fixture is for hosted CI")
    output_dir = Path(runner_temp) / "demostrike-smoke"
    output_dir.mkdir(parents=True, exist_ok=True)
    db_path = output_dir / "synthetic-library.sqlite"
    if db_path.exists():
        raise SystemExit(f"Refusing to replace an existing fixture: {db_path}")

    with sqlite3.connect(db_path) as db:
        db.executescript(
            """
            CREATE TABLE matches (
                id INTEGER PRIMARY KEY, file_name TEXT NOT NULL,
                map_name TEXT NOT NULL, score_ct INTEGER NOT NULL,
                score_t INTEGER NOT NULL, imported_at TEXT NOT NULL,
                parser_capability TEXT NOT NULL
            );
            CREATE TABLE players (match_id INTEGER, steamid TEXT, name TEXT);
            CREATE TABLE rounds (
                match_id INTEGER, round_number INTEGER, winner TEXT,
                start_tick INTEGER, end_tick INTEGER
            );
            CREATE TABLE utility_events (
                id INTEGER PRIMARY KEY, match_id INTEGER, round_number INTEGER,
                utility_type TEXT, thrower_name TEXT, thrower_team TEXT,
                throw_tick INTEGER, enemies_blinded INTEGER, damage_enemy INTEGER
            );
            INSERT INTO matches VALUES
                (101, 'synthetic-alpha.dem', 'de_mirage', 13, 9,
                 '2026-01-02T12:00:00Z', 'synthetic-ci-only'),
                (102, 'synthetic-beta.dem', 'de_inferno', 6, 13,
                 '2026-01-01T12:00:00Z', 'synthetic-ci-only');
            INSERT INTO players VALUES
                (101, 'synthetic-ct-1', 'Synthetic CT'),
                (101, 'synthetic-t-1', 'Synthetic T');
            INSERT INTO rounds VALUES
                (101, 1, 'CT', 100, 200),
                (101, 2, 'T', 201, 300),
                (101, 3, NULL, 301, 400);
            INSERT INTO utility_events VALUES
                (1, 101, 1, 'smoke', 'Synthetic CT', 'CT', 130, 0, 0),
                (2, 101, 2, 'flash', 'Synthetic T', 'T', 240, 2, 0),
                (3, 101, 3, 'molotov', 'Synthetic CT', 'CT', 330, 0, 18);
            """
        )
        assert db.execute("PRAGMA integrity_check").fetchone()[0] == "ok"
    digest = hashlib.sha256(db_path.read_bytes()).hexdigest()
    print(f"Synthetic fixture: {db_path}; SHA-256: {digest}")


if __name__ == "__main__":
    main()
