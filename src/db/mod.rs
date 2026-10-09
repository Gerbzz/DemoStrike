//! SQLite persistence. Versioned migrations via PRAGMA user_version.

pub mod queries;

use crate::error::AppResult;
use rusqlite::Connection;
use std::path::Path;

const MIGRATIONS: &[&str] = &[
    // v1 — initial schema
    r#"
    CREATE TABLE matches (
        id INTEGER PRIMARY KEY,
        file_path TEXT NOT NULL UNIQUE,
        file_name TEXT NOT NULL,
        map_name TEXT NOT NULL,
        tick_rate REAL NOT NULL,
        duration_ticks INTEGER NOT NULL DEFAULT 0,
        rounds_count INTEGER NOT NULL DEFAULT 0,
        score_ct INTEGER NOT NULL DEFAULT 0,
        score_t INTEGER NOT NULL DEFAULT 0,
        imported_at TEXT NOT NULL
    );

    CREATE TABLE players (
        match_id INTEGER NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
        steamid TEXT NOT NULL,
        name TEXT NOT NULL,
        PRIMARY KEY (match_id, steamid)
    );

    CREATE TABLE rounds (
        match_id INTEGER NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
        round_number INTEGER NOT NULL,
        start_tick INTEGER,
        freeze_end_tick INTEGER,
        end_tick INTEGER,
        winner TEXT,
        end_reason TEXT,
        ct_equip_value INTEGER,
        t_equip_value INTEGER,
        PRIMARY KEY (match_id, round_number)
    );

    CREATE TABLE utility_events (
        id INTEGER PRIMARY KEY,
        match_id INTEGER NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
        round_number INTEGER NOT NULL,
        utility_type TEXT NOT NULL,
        thrower_steamid TEXT NOT NULL,
        thrower_name TEXT NOT NULL,
        thrower_team TEXT NOT NULL,
        throw_tick INTEGER NOT NULL,
        throw_time REAL NOT NULL DEFAULT 0,
        throw_x REAL NOT NULL, throw_y REAL NOT NULL, throw_z REAL NOT NULL,
        detonate_tick INTEGER,
        detonate_x REAL, detonate_y REAL, detonate_z REAL,
        land_x REAL, land_y REAL, land_z REAL,
        trajectory TEXT NOT NULL DEFAULT '[]',
        bounces TEXT NOT NULL DEFAULT '[]',
        blind_enemy_duration REAL NOT NULL DEFAULT 0,
        blind_team_duration REAL NOT NULL DEFAULT 0,
        enemies_blinded INTEGER NOT NULL DEFAULT 0,
        teammates_blinded INTEGER NOT NULL DEFAULT 0,
        damage_enemy INTEGER NOT NULL DEFAULT 0,
        damage_team INTEGER NOT NULL DEFAULT 0,
        kills INTEGER NOT NULL DEFAULT 0,
        flash_assists INTEGER NOT NULL DEFAULT 0,
        nearby_players TEXT NOT NULL DEFAULT '[]',
        round_winner TEXT,
        favorite INTEGER NOT NULL DEFAULT 0
    );
    CREATE INDEX idx_events_match ON utility_events(match_id);
    CREATE INDEX idx_events_type ON utility_events(utility_type);
    CREATE INDEX idx_events_thrower ON utility_events(thrower_steamid);
    CREATE INDEX idx_events_round ON utility_events(round_number);
    CREATE INDEX idx_events_favorite ON utility_events(favorite);

    CREATE TABLE tags (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL UNIQUE COLLATE NOCASE
    );
    CREATE TABLE event_tags (
        event_id INTEGER NOT NULL REFERENCES utility_events(id) ON DELETE CASCADE,
        tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
        PRIMARY KEY (event_id, tag_id)
    );

    CREATE TABLE collections (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        created_at TEXT NOT NULL
    );
    CREATE TABLE collection_events (
        collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
        event_id INTEGER NOT NULL REFERENCES utility_events(id) ON DELETE CASCADE,
        position INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (collection_id, event_id)
    );

    CREATE TABLE render_jobs (
        id INTEGER PRIMARY KEY,
        status TEXT NOT NULL DEFAULT 'queued',
        progress REAL NOT NULL DEFAULT 0,
        settings TEXT NOT NULL,
        output_dir TEXT NOT NULL,
        error TEXT,
        created_at TEXT NOT NULL
    );
    CREATE TABLE render_job_events (
        job_id INTEGER NOT NULL REFERENCES render_jobs(id) ON DELETE CASCADE,
        event_id INTEGER NOT NULL REFERENCES utility_events(id) ON DELETE CASCADE,
        position INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (job_id, event_id)
    );
    "#,
    // v2 — real CS2 POV capture needs the spectator slot from the demo.
    r#"
    ALTER TABLE utility_events ADD COLUMN observer_slot INTEGER;
    "#,
    // v3 - a grenade-only import must never masquerade as full combat/economy facts.
    r#"
    ALTER TABLE matches ADD COLUMN parser_capability TEXT NOT NULL DEFAULT 'reference-raw-facts';
    "#,
];

pub fn open(path: &Path) -> AppResult<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

#[cfg(test)]
pub fn open_in_memory() -> AppResult<Connection> {
    let conn = Connection::open_in_memory()?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> AppResult<()> {
    // journal_mode returns a row, so it can't go through execute_batch.
    let _mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
    conn.execute_batch("PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON;")?;
    Ok(())
}

fn migrate(conn: &Connection) -> AppResult<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate() {
        let target = (i + 1) as i64;
        if version < target {
            conn.execute_batch(sql)?;
            conn.execute_batch(&format!("PRAGMA user_version = {target};"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod capability_migration_tests {
    use super::*;

    #[test]
    fn v2_match_and_annotation_survive_capability_migration() {
        let conn = Connection::open_in_memory().unwrap();
        configure(&conn).unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.execute_batch(MIGRATIONS[1]).unwrap();
        conn.execute_batch("PRAGMA user_version = 2;").unwrap();
        conn.execute("INSERT INTO matches (id,file_path,file_name,map_name,tick_rate,imported_at) VALUES (7,'C:/old.dem','old.dem','de_ancient',64,'2026-10-08')", []).unwrap();
        conn.execute("INSERT INTO utility_events (id,match_id,round_number,utility_type,thrower_steamid,thrower_name,thrower_team,throw_tick,throw_x,throw_y,throw_z,favorite) VALUES (9,7,1,'smoke','123','player','CT',100,1,2,3,1)", []).unwrap();
        migrate(&conn).unwrap();
        let capability: String = conn.query_row("SELECT parser_capability FROM matches WHERE id=7", [], |r| r.get(0)).unwrap();
        let favorite: i64 = conn.query_row("SELECT favorite FROM utility_events WHERE id=9", [], |r| r.get(0)).unwrap();
        assert_eq!(capability, "reference-raw-facts");
        assert_eq!(favorite, 1);
        let events = queries::query_events(&conn, &Default::default()).unwrap();
        assert_eq!(events[0].parser_capability, crate::domain::ParserCapabilities::ReferenceRawFacts);
        assert_eq!(events[0].damage_enemy, Some(0));
    }
}
