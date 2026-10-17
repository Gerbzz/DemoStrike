use rusqlite::{Connection, OpenFlags, params};
use serde::Serialize;
use std::collections::HashMap;
use std::fmt;
use std::path::Path;

#[derive(Debug)]
pub enum LibraryError {
    MissingFile,
    InvalidSchema(String),
    MatchNotFound(i64),
    Database(rusqlite::Error),
}

impl fmt::Display for LibraryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingFile => write!(
                f,
                "The library file was not found. Check the path and try again."
            ),
            Self::InvalidSchema(detail) => {
                write!(f, "This is not a compatible DemoStrike library: {detail}")
            }
            Self::MatchNotFound(id) => write!(
                f,
                "Match {id} is no longer in this library. Refresh the match list."
            ),
            Self::Database(error) => write!(f, "Could not read the library: {error}"),
        }
    }
}

impl From<rusqlite::Error> for LibraryError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

/// Rust equivalent of the upstream score comparison used by TeamScore.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ScoreRelation {
    Tied,
    Leading,
    Trailing,
}

pub fn score_relation(score: i64, opposite_score: i64) -> ScoreRelation {
    use std::cmp::Ordering;
    match score.cmp(&opposite_score) {
        Ordering::Equal => ScoreRelation::Tied,
        Ordering::Greater => ScoreRelation::Leading,
        Ordering::Less => ScoreRelation::Trailing,
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchSummary {
    pub id: i64,
    pub file_name: String,
    pub demo_file_path: Option<String>,
    pub map_name: String,
    pub score_ct: i64,
    pub score_t: i64,
    pub score_ct_relation: ScoreRelation,
    pub score_t_relation: ScoreRelation,
    pub imported_at: String,
    pub parser_capability: String,
    pub source_sha256: Option<String>,
    pub tick_rate: Option<f64>,
    pub duration_ticks: Option<i64>,
    pub player_count: i64,
    pub round_count: i64,
    pub utility_count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySnapshot {
    pub path: String,
    pub matches: Vec<MatchSummary>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundSummary {
    pub round_number: i64,
    pub winner: Option<String>,
    pub start_tick: Option<i64>,
    pub end_tick: Option<i64>,
    pub combat: Option<RoundCombat>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundCombat {
    pub ct_players: i64,
    pub t_players: i64,
    pub unassigned_players: i64,
    pub enemy_kills: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerSummary {
    pub steamid: String,
    pub name: String,
    pub combat: Option<PlayerCombat>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerCombat {
    pub enemy_kills: Option<i64>,
    pub deaths: i64,
    pub enemy_assists: Option<i64>,
    pub enemy_health_damage: Option<i64>,
    pub rounds: Vec<PlayerRoundCombat>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerRoundCombat {
    pub round_number: i64,
    pub side: Option<String>,
    pub enemy_kills: Option<i64>,
    pub deaths: i64,
    pub enemy_assists: Option<i64>,
    pub enemy_health_damage: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UtilityEvent {
    pub id: i64,
    pub round_number: i64,
    pub utility_type: String,
    pub thrower_name: String,
    pub thrower_team: String,
    pub throw_tick: i64,
    pub enemies_blinded: Option<i64>,
    pub damage_enemy: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UtilityCount {
    pub utility_type: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchDetail {
    pub summary: MatchSummary,
    pub rounds: Vec<RoundSummary>,
    pub players: Vec<PlayerSummary>,
    pub utility_events: Vec<UtilityEvent>,
    pub utility_breakdown: Vec<UtilityCount>,
    pub full_combat_coverage: bool,
}

fn has_full_combat(connection: &Connection, capability: &str) -> Result<bool, LibraryError> {
    if capability != "native-combat-full-v2" {
        return Ok(false);
    }
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version < 7 {
        return Ok(false);
    }
    let mut statement = connection.prepare("PRAGMA table_info(player_round_combat)")?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok([
        "match_id",
        "round_number",
        "steamid",
        "side",
        "enemy_kills",
        "deaths",
        "enemy_assists",
        "enemy_health_damage",
    ]
    .iter()
    .all(|required| columns.iter().any(|column| column == required)))
}

fn open_library(path: &str) -> Result<Connection, LibraryError> {
    if !Path::new(path).is_file() {
        return Err(LibraryError::MissingFile);
    }
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    for (table, columns) in [
        (
            "matches",
            &[
                "id",
                "file_name",
                "map_name",
                "score_ct",
                "score_t",
                "imported_at",
                "parser_capability",
            ][..],
        ),
        ("players", &["match_id", "steamid", "name"][..]),
        (
            "rounds",
            &[
                "match_id",
                "round_number",
                "winner",
                "start_tick",
                "end_tick",
            ][..],
        ),
        (
            "utility_events",
            &[
                "id",
                "match_id",
                "round_number",
                "utility_type",
                "thrower_name",
                "thrower_team",
                "throw_tick",
            ][..],
        ),
    ] {
        let exists: i64 = connection.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |row| row.get(0),
        )?;
        if exists != 1 {
            return Err(LibraryError::InvalidSchema(format!(
                "missing {table} table"
            )));
        }
        let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
        let names = statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?;
        for column in columns {
            if !names.iter().any(|name| name == column) {
                return Err(LibraryError::InvalidSchema(format!(
                    "missing {table}.{column} column"
                )));
            }
        }
    }
    Ok(connection)
}

/// Earlier imports can retain stored effect totals without verified attribution.
/// Only the v6+ public view can expose those totals as measured facts.
fn has_verified_effect_view(connection: &Connection) -> Result<bool, LibraryError> {
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version < 6 {
        return Ok(false);
    }
    let mut statement = connection.prepare("PRAGMA table_info(utility_events_supported)")?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok([
        "effect_attribution_version",
        "blind_attribution_complete",
        "damage_attribution_complete",
        "enemies_blinded",
        "damage_enemy",
    ]
    .iter()
    .all(|column| columns.iter().any(|name| name == column)))
}

fn read_matches(connection: &Connection) -> Result<Vec<MatchSummary>, LibraryError> {
    let mut columns = connection.prepare("PRAGMA table_info(matches)")?;
    let available = columns
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    let optional = |name: &str| {
        if available.iter().any(|column| column == name) {
            format!("m.{name}")
        } else {
            "NULL".to_owned()
        }
    };
    let query = format!(
        "SELECT m.id, m.file_name, m.map_name, m.score_ct, m.score_t, m.imported_at,
                m.parser_capability,
                (SELECT COUNT(*) FROM players p WHERE p.match_id = m.id),
                (SELECT COUNT(*) FROM rounds r WHERE r.match_id = m.id),
                (SELECT COUNT(*) FROM utility_events u WHERE u.match_id = m.id),
                {}, {}, {}, {}
         FROM matches m ORDER BY m.imported_at DESC, m.id DESC",
        optional("file_path"),
        optional("source_sha256"),
        optional("tick_rate"),
        optional("duration_ticks")
    );
    let mut statement = connection.prepare(&query)?;
    let matches = statement
        .query_map([], |row| {
            let score_ct: i64 = row.get(3)?;
            let score_t: i64 = row.get(4)?;
            Ok(MatchSummary {
                id: row.get(0)?,
                file_name: row.get(1)?,
                demo_file_path: row.get(10)?,
                map_name: row.get(2)?,
                score_ct,
                score_t,
                score_ct_relation: score_relation(score_ct, score_t),
                score_t_relation: score_relation(score_t, score_ct),
                imported_at: row.get(5)?,
                parser_capability: row.get(6)?,
                source_sha256: row.get(11)?,
                tick_rate: row.get(12)?,
                duration_ticks: row.get(13)?,
                player_count: row.get(7)?,
                round_count: row.get(8)?,
                utility_count: row.get(9)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(matches)
}

pub fn list_matches(path: &str) -> Result<LibrarySnapshot, LibraryError> {
    let connection = open_library(path)?;
    Ok(LibrarySnapshot {
        path: path.to_owned(),
        matches: read_matches(&connection)?,
    })
}

pub fn get_match_detail(path: &str, match_id: i64) -> Result<MatchDetail, LibraryError> {
    let connection = open_library(path)?;
    let summary = read_matches(&connection)?
        .into_iter()
        .find(|item| item.id == match_id)
        .ok_or(LibraryError::MatchNotFound(match_id))?;
    let full_combat_coverage = has_full_combat(&connection, &summary.parser_capability)?;
    let mut combat_by_player: HashMap<String, Vec<PlayerRoundCombat>> = HashMap::new();
    let mut combat_by_round: HashMap<i64, Vec<PlayerRoundCombat>> = HashMap::new();
    if full_combat_coverage {
        let mut query = connection.prepare(
            "SELECT round_number, steamid, side, enemy_kills, deaths,
                    enemy_assists, enemy_health_damage
             FROM player_round_combat WHERE match_id = ?1 ORDER BY round_number, steamid",
        )?;
        let records = query.query_map([match_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                PlayerRoundCombat {
                    round_number: row.get(0)?,
                    side: row.get(2)?,
                    enemy_kills: row.get(3)?,
                    deaths: row.get(4)?,
                    enemy_assists: row.get(5)?,
                    enemy_health_damage: row.get(6)?,
                },
            ))
        })?;
        for record in records {
            let (round, steamid, combat) = record?;
            combat_by_round
                .entry(round)
                .or_default()
                .push(combat.clone());
            combat_by_player.entry(steamid).or_default().push(combat);
        }
    }

    let mut rounds_query = connection.prepare(
        "SELECT round_number, winner, start_tick, end_tick FROM rounds
         WHERE match_id = ?1 ORDER BY round_number",
    )?;
    let rounds = rounds_query
        .query_map([match_id], |row| {
            Ok(RoundSummary {
                round_number: row.get(0)?,
                winner: row.get(1)?,
                start_tick: row.get(2)?,
                end_tick: row.get(3)?,
                combat: combat_by_round
                    .get(&row.get::<_, i64>(0)?)
                    .map(|records| RoundCombat {
                        ct_players: records
                            .iter()
                            .filter(|item| item.side.as_deref() == Some("CT"))
                            .count() as i64,
                        t_players: records
                            .iter()
                            .filter(|item| item.side.as_deref() == Some("T"))
                            .count() as i64,
                        unassigned_players: records
                            .iter()
                            .filter(|item| item.side.is_none())
                            .count() as i64,
                        enemy_kills: records
                            .iter()
                            .try_fold(0_i64, |sum, item| Some(sum + item.enemy_kills?)),
                    }),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut players_query = connection.prepare(
        "SELECT steamid, name FROM players WHERE match_id = ?1 ORDER BY name COLLATE NOCASE",
    )?;
    let players = players_query
        .query_map([match_id], |row| {
            Ok(PlayerSummary {
                steamid: row.get(0)?,
                name: row.get(1)?,
                combat: combat_by_player
                    .get(&row.get::<_, String>(0)?)
                    .map(|records| PlayerCombat {
                        enemy_kills: records
                            .iter()
                            .try_fold(0_i64, |sum, item| Some(sum + item.enemy_kills?)),
                        deaths: records.iter().map(|item| item.deaths).sum(),
                        enemy_assists: records
                            .iter()
                            .try_fold(0_i64, |sum, item| Some(sum + item.enemy_assists?)),
                        enemy_health_damage: records
                            .iter()
                            .try_fold(0_i64, |sum, item| Some(sum + item.enemy_health_damage?)),
                        rounds: records.clone(),
                    }),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let events_sql = if has_verified_effect_view(&connection)? {
        "SELECT e.id, e.round_number, e.utility_type, e.thrower_name, e.thrower_team,
                e.throw_tick, v.enemies_blinded, v.damage_enemy
         FROM utility_events e LEFT JOIN utility_events_supported v ON v.id = e.id
         WHERE e.match_id = ?1 ORDER BY e.round_number, e.throw_tick, e.id"
    } else {
        "SELECT id, round_number, utility_type, thrower_name, thrower_team,
                throw_tick, NULL AS enemies_blinded, NULL AS damage_enemy
         FROM utility_events WHERE match_id = ?1 ORDER BY round_number, throw_tick, id"
    };
    let mut events_query = connection.prepare(events_sql)?;
    let utility_events = events_query
        .query_map([match_id], |row| {
            Ok(UtilityEvent {
                id: row.get(0)?,
                round_number: row.get(1)?,
                utility_type: row.get(2)?,
                thrower_name: row.get(3)?,
                thrower_team: row.get(4)?,
                throw_tick: row.get(5)?,
                enemies_blinded: row.get(6)?,
                damage_enemy: row.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut breakdown_query = connection.prepare(
        "SELECT utility_type, COUNT(*) FROM utility_events
         WHERE match_id = ?1 GROUP BY utility_type ORDER BY COUNT(*) DESC, utility_type",
    )?;
    let utility_breakdown = breakdown_query
        .query_map(params![match_id], |row| {
            Ok(UtilityCount {
                utility_type: row.get(0)?,
                count: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(MatchDetail {
        summary,
        rounds,
        players,
        utility_events,
        utility_breakdown,
        full_combat_coverage,
    })
}
