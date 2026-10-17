use demostrike_rust_candidate::library::{
    ScoreRelation, get_match_detail, list_matches, score_relation,
};
use rusqlite::Connection;
use tempfile::tempdir;

fn fixture() -> (tempfile::TempDir, String) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("library.sqlite");
    let db = Connection::open(&path).unwrap();
    db.execute_batch(
        "CREATE TABLE matches (id INTEGER PRIMARY KEY, file_name TEXT, map_name TEXT, score_ct INTEGER, score_t INTEGER, imported_at TEXT, parser_capability TEXT);
         CREATE TABLE players (match_id INTEGER, steamid TEXT, name TEXT);
         CREATE TABLE rounds (match_id INTEGER, round_number INTEGER, winner TEXT, start_tick INTEGER, end_tick INTEGER);
         CREATE TABLE utility_events (id INTEGER PRIMARY KEY, match_id INTEGER, round_number INTEGER, utility_type TEXT, thrower_name TEXT, thrower_team TEXT, throw_tick INTEGER, enemies_blinded INTEGER, damage_enemy INTEGER);
         INSERT INTO matches VALUES (1, 'cache.dem', 'de_cache', 10, 5, '2026-10-17T00:00:00Z', 'native-combat-flash-v1');
         INSERT INTO players VALUES (1, '7656', 'Tester');
         INSERT INTO rounds VALUES (1, 1, 'CT', 100, 200);
         INSERT INTO utility_events VALUES (1, 1, 1, 'smoke', 'Tester', 'CT', 140, 0, 0);",
    )
    .unwrap();
    (dir, path.to_string_lossy().into_owned())
}

#[test]
fn lists_real_schema_rows_without_writing() {
    let (_dir, path) = fixture();
    let before = std::fs::read(&path).unwrap();
    let snapshot = list_matches(&path).unwrap();
    assert_eq!(snapshot.matches.len(), 1);
    assert_eq!(snapshot.matches[0].utility_count, 1);
    assert_eq!(snapshot.matches[0].player_count, 1);
    assert_eq!(
        snapshot.matches[0].score_ct_relation,
        ScoreRelation::Leading
    );
    assert_eq!(
        snapshot.matches[0].score_t_relation,
        ScoreRelation::Trailing
    );
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[test]
fn preserves_upstream_team_score_relation_for_all_three_cases() {
    assert_eq!(score_relation(5, 5), ScoreRelation::Tied);
    assert_eq!(score_relation(10, 5), ScoreRelation::Leading);
    assert_eq!(score_relation(5, 10), ScoreRelation::Trailing);
}

#[test]
fn loads_rounds_players_and_utility_for_selected_match() {
    let (_dir, path) = fixture();
    let detail = get_match_detail(&path, 1).unwrap();
    assert_eq!(detail.summary.map_name, "de_cache");
    assert_eq!(detail.rounds.len(), 1);
    assert_eq!(detail.players[0].name, "Tester");
    assert_eq!(detail.utility_events[0].utility_type, "smoke");
    assert_eq!(detail.utility_breakdown[0].count, 1);
}

#[test]
fn rejects_missing_file_and_unknown_schema() {
    let dir = tempdir().unwrap();
    assert!(list_matches(&dir.path().join("missing.sqlite").to_string_lossy()).is_err());
    let path = dir.path().join("wrong.sqlite");
    Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TABLE unrelated (id INTEGER);")
        .unwrap();
    assert!(list_matches(&path.to_string_lossy()).is_err());
}

#[test]
fn empty_valid_library_is_valid_and_missing_match_is_an_error() {
    let (_dir, path) = fixture();
    let db = Connection::open(&path).unwrap();
    db.execute("DELETE FROM utility_events", []).unwrap();
    db.execute("DELETE FROM rounds", []).unwrap();
    db.execute("DELETE FROM players", []).unwrap();
    db.execute("DELETE FROM matches", []).unwrap();
    assert!(list_matches(&path).unwrap().matches.is_empty());
    assert!(get_match_detail(&path, 1).is_err());
}
