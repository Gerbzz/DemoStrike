"""Read-only parity check of the older-CLI Cache import used by this candidate.

This verifies source data and candidate query shapes, not a compiled Rust/Tauri app.
"""

import argparse
import hashlib
import re
import sqlite3
from pathlib import Path


RUST_ADAPTER = Path(__file__).resolve().parents[1] / "src-tauri/src/library.rs"


def main() -> None:
    parser = argparse.ArgumentParser(description="Verify a caller supplied read-only Cache import fixture")
    parser.add_argument("--db", required=True, type=Path, help="Path to an existing local SQLite import")
    db_path = parser.parse_args().db.resolve(strict=True)
    before = hashlib.sha256(db_path.read_bytes()).hexdigest()
    with sqlite3.connect(f"file:{db_path.as_posix()}?mode=ro&immutable=1", uri=True) as db:
        # Execute the actual static SELECT strings from the Rust adapter against
        # this import. This catches SQL/schema drift without compiling a binary.
        adapter_sql = re.findall(r'"(SELECT[^\"]+)"', RUST_ADAPTER.read_text(encoding="utf-8"))
        assert len(adapter_sql) == 6, len(adapter_sql)
        for query in adapter_sql:
            parameters = ("matches",) if "sqlite_master" in query else (() if "FROM matches m" in query else (1,))
            db.execute(query, parameters).fetchall()
        match = db.execute(
            "SELECT id, file_name, map_name, score_ct, score_t, parser_capability FROM matches"
        ).fetchone()
        assert match is not None
        match_id, file_name, map_name, score_ct, score_t, parser = match
        assert file_name.lower().endswith(".dem")
        assert (map_name, score_ct, score_t) == ("de_cache", 10, 5)
        assert parser == "native-combat-flash-v1"
        counts = {
            table: db.execute(f"SELECT COUNT(*) FROM {table} WHERE match_id = ?", (match_id,)).fetchone()[0]
            for table in ("players", "rounds", "utility_events")
        }
        assert counts == {"players": 10, "rounds": 16, "utility_events": 253}, counts
        missing_winners = db.execute(
            "SELECT COUNT(*) FROM rounds WHERE match_id = ? AND winner IS NULL", (match_id,)
        ).fetchone()[0]
        assert missing_winners == 1, missing_winners
        utility = dict(db.execute(
            "SELECT utility_type, COUNT(*) FROM utility_events WHERE match_id = ? GROUP BY utility_type",
            (match_id,),
        ))
        assert utility == {
            "smoke": 89, "flash": 69, "he": 37, "molotov": 32, "incendiary": 24, "decoy": 2
        }, utility
        assert db.execute("PRAGMA integrity_check").fetchone()[0] == "ok"
    assert hashlib.sha256(db_path.read_bytes()).hexdigest() == before
    print("PASS: Cache fixture, six Rust SQL queries, unknown winner, utility breakdown, integrity, unchanged bytes")


if __name__ == "__main__":
    main()
