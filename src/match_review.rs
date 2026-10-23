//! Read-only match and replay data shared by native Rust application surfaces.
//!
//! This is the existing Rust SQLite reader extracted from the candidate host,
//! not a translated TypeScript adapter. It has no Tauri, React, or IPC dependency.
//! Nullable facts remain unavailable, sample reads use exact observed ticks,
//! and review never migrates or writes the user's database.

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::Path;

#[derive(Debug)]
pub enum LibraryError {
    MissingFile,
    InvalidSchema(String),
    InvalidRequest(String),
    MatchNotFound(i64),
    RoundNotFound(i64),
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
            Self::InvalidRequest(detail) => write!(f, "Invalid viewer request: {detail}"),
            Self::MatchNotFound(id) => write!(
                f,
                "Match {id} is no longer in this library. Refresh the match list."
            ),
            Self::RoundNotFound(number) => write!(f, "Round {number} is not in this match."),
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
    pub freeze_end_tick: Option<i64>,
    pub end_tick: Option<i64>,
    pub end_reason: Option<String>,
    pub team_a_side: Option<String>,
    pub team_b_side: Option<String>,
    pub ct_equip_value: Option<i64>,
    pub t_equip_value: Option<i64>,
    pub combat: Option<RoundCombat>,
    /// None means this import has no verified ordered combat stream. An
    /// empty array means the stream exists but this round has no deaths.
    pub deaths: Option<Vec<RoundDeath>>,
    /// Raw damage events are absent for older imports; zero means a verified empty round.
    pub hurts: Option<Vec<RoundHurt>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundDeath {
    pub source_index: i64,
    pub event_order: Option<i64>,
    pub tick: i64,
    pub weapon: Option<String>,
    pub attacker_steamid: Option<String>,
    pub attacker_side: Option<String>,
    pub victim_steamid: String,
    pub victim_side: Option<String>,
    pub assister_steamid: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundHurt {
    pub source_index: i64,
    pub event_order: Option<i64>,
    pub tick: i64,
    pub weapon: Option<String>,
    pub attacker_steamid: Option<String>,
    pub attacker_side: Option<String>,
    pub victim_steamid: String,
    pub victim_side: Option<String>,
    /// Demo event raw_dmg_health, not CSDM's effective health damage.
    pub raw_damage: i64,
    /// Pinned CSDM HealthDamageTaken; unavailable for earlier imports.
    pub effective_damage: Option<i64>,
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
    pub stable_team: Option<String>,
    pub enemy_kills: Option<i64>,
    pub deaths: i64,
    pub enemy_assists: Option<i64>,
    pub enemy_health_damage: Option<i64>,
    pub effective_enemy_health_damage: Option<i64>,
    pub rounds: Vec<PlayerRoundCombat>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerRoundCombat {
    pub round_number: i64,
    pub side: Option<String>,
    pub stable_team: Option<String>,
    pub enemy_kills: Option<i64>,
    pub deaths: i64,
    pub enemy_assists: Option<i64>,
    pub enemy_health_damage: Option<i64>,
    pub effective_enemy_health_damage: Option<i64>,
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
pub struct ObservedDuelEvent {
    pub round_number: i64,
    pub death: RoundDeath,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedDuelPair {
    pub attacker_steamid: String,
    pub victim_steamid: String,
    pub events: Vec<ObservedDuelEvent>,
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
    /// None means the ordered death stream was unavailable for this import.
    pub observed_duels: Option<Vec<ObservedDuelPair>>,
}

/// The viewer keeps the demo's absolute tick domain. Older imports report
/// unavailable instead of interpreting absent rows as an empty replay.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewerRoundMeta {
    pub round_number: i64,
    pub start_tick: Option<i64>,
    pub end_tick: Option<i64>,
    pub tick_rate: Option<f64>,
    pub map_name: String,
    pub positions_available: bool,
    pub player_sample_count: i64,
    pub grenade_sample_count: i64,
    pub utility_events: Vec<ViewerUtilityEvent>,
    pub bomb_events: Vec<ViewerBombEvent>,
    pub bomb_events_available: bool,
    /// None means this import has no verified bomb event stream.
    pub observed_bomb_selection: Option<ObservedBombSelection>,
}

/// Detonation facts persist with the imported demo. Missing coordinates and
/// projectile identity stay nullable; the viewer must not infer an effect.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewerUtilityEvent {
    pub id: i64,
    pub utility_type: String,
    pub throw_tick: i64,
    pub detonate_tick: Option<i64>,
    pub detonate_x: Option<f64>,
    pub detonate_y: Option<f64>,
    pub detonate_z: Option<f64>,
    pub projectile_entity_id: Option<i64>,
    /// Native match for the original viewer projectileId. None is unmatched or ambiguous.
    pub observed_trajectory_index: Option<i64>,
    pub thrower_steamid: String,
    pub thrower_name: String,
    pub thrower_side: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinderThrow {
    pub id: i64,
    pub round_number: i64,
    pub utility_type: String,
    pub thrower_steamid: String,
    pub thrower_name: String,
    pub thrower_side: String,
    pub throw_tick: i64,
    pub throw_position: [f64; 3],
    pub detonate_tick: Option<i64>,
    pub detonate_position: Option<[f64; 3]>,
    pub trajectory: Vec<[f64; 4]>,
}

/// A read-only native edge for the original grenade finder. Every coordinate
/// comes from the imported event; invalid or absent paths remain unavailable.
pub fn get_utility_finder(path: &str, match_id: i64) -> Result<Vec<FinderThrow>, LibraryError> {
    let connection = open_library(path)?;
    if !read_matches(&connection)?
        .iter()
        .any(|item| item.id == match_id)
    {
        return Err(LibraryError::MatchNotFound(match_id));
    }
    let mut query = connection.prepare(
        "SELECT id,round_number,utility_type,thrower_steamid,thrower_name,thrower_team,
                throw_tick,throw_x,throw_y,throw_z,detonate_tick,
                detonate_x,detonate_y,detonate_z,trajectory
         FROM utility_events WHERE match_id=?1 ORDER BY round_number,throw_tick,id",
    )?;
    let throws = query
        .query_map([match_id], |row| {
            let detonate = (
                row.get::<_, Option<f64>>(11)?,
                row.get::<_, Option<f64>>(12)?,
                row.get::<_, Option<f64>>(13)?,
            );
            let raw_path: String = row.get(14)?;
            let trajectory: Vec<[f64; 4]> = serde_json::from_str::<Vec<[f64; 4]>>(&raw_path)
                .unwrap_or_default()
                .into_iter()
                .filter(|point| point.iter().all(|value| value.is_finite()))
                .collect();
            Ok(FinderThrow {
                id: row.get(0)?,
                round_number: row.get(1)?,
                utility_type: row.get(2)?,
                thrower_steamid: row.get(3)?,
                thrower_name: row.get(4)?,
                thrower_side: row.get(5)?,
                throw_tick: row.get(6)?,
                throw_position: [row.get(7)?, row.get(8)?, row.get(9)?],
                detonate_tick: row.get(10)?,
                detonate_position: match detonate {
                    (Some(x), Some(y), Some(z))
                        if [x, y, z].iter().all(|value| value.is_finite()) =>
                    {
                        Some([x, y, z])
                    }
                    _ => None,
                },
                trajectory,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(throws)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewerBombEvent {
    pub source_index: i64,
    pub kind: String,
    pub tick: i64,
    pub actor_steamid: Option<String>,
    pub actor_name: Option<String>,
    pub site_id: Option<i64>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub z: Option<f64>,
}

/// Indexes refer to the ordered bomb_events array returned in the same IPC
/// response. A verified stream can have no uniquely usable plant.
#[derive(Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedBombSelection {
    pub plant_index: Option<usize>,
    pub explosion_index: Option<usize>,
    pub defusal_index: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewerPlayerFrame {
    pub tick: i64,
    pub steamid: String,
    pub name: String,
    pub side: Option<String>,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub is_alive: Option<bool>,
    pub health: Option<i64>,
    pub yaw: Option<f64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewerGrenadeFrame {
    pub tick: i64,
    pub trajectory_index: i64,
    pub entity_id: i64,
    pub grenade_type: Option<String>,
    pub thrower_steamid: Option<String>,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewerFrameChunk {
    pub start_tick: i64,
    pub end_tick: i64,
    pub players: Vec<ViewerPlayerFrame>,
    pub grenades: Vec<ViewerGrenadeFrame>,
}

fn has_full_combat(connection: &Connection, capability: &str) -> Result<bool, LibraryError> {
    if capability != "native-combat-full-v2" && capability != "test-synthetic-combat-v1" {
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

fn has_ordered_combat_deaths(
    connection: &Connection,
    capability: &str,
) -> Result<bool, LibraryError> {
    if capability != "native-combat-full-v2" {
        return Ok(false);
    }
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version < 8 {
        return Ok(false);
    }
    let mut statement = connection.prepare("PRAGMA table_info(raw_combat_deaths)")?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok([
        "match_id",
        "source_index",
        "event_order",
        "tick",
        "round_number",
        "weapon",
        "attacker_steamid",
        "attacker_side",
        "victim_steamid",
        "victim_side",
        "assister_steamid",
    ]
    .iter()
    .all(|column| columns.iter().any(|name| name == column)))
}

fn has_ordered_combat_hurts(
    connection: &Connection,
    capability: &str,
) -> Result<bool, LibraryError> {
    if capability != "native-combat-full-v2" {
        return Ok(false);
    }
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version < 8 {
        return Ok(false);
    }
    let mut statement = connection.prepare("PRAGMA table_info(raw_combat_hurts)")?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok([
        "match_id",
        "source_index",
        "event_order",
        "tick",
        "round_number",
        "weapon",
        "attacker_steamid",
        "attacker_side",
        "victim_steamid",
        "victim_side",
        "raw_dmg_health",
    ]
    .iter()
    .all(|column| columns.iter().any(|name| name == column)))
}

/// Source 2 can advance its raw round counter before the next live round.
/// Assign events in that gap to the last completed earlier round. Raw rows
/// remain unchanged, as with the persisted per-player combat attribution.
fn observed_combat_side(side: Option<String>) -> Option<String> {
    match side.as_deref() {
        Some("CT") => Some("CT".to_owned()),
        Some("T" | "TERRORIST") => Some("T".to_owned()),
        _ => None,
    }
}

fn effective_event_round(
    raw_round: i64,
    tick: i64,
    boundaries: &[(i64, Option<i64>, Option<i64>)],
) -> Option<i64> {
    let (_, start, _) = boundaries
        .iter()
        .find(|(number, _, _)| *number == raw_round)?;
    if start.is_some_and(|value| tick < value) {
        return boundaries
            .iter()
            .filter(|(number, _, end)| {
                *number < raw_round && end.is_some_and(|value| value <= tick)
            })
            .max_by_key(|(_, _, end)| *end)
            .map(|(number, _, _)| *number);
    }
    Some(raw_round)
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

fn unique_bomb_event_index(
    events: &[ViewerBombEvent],
    kind: &str,
    earliest_tick: i64,
) -> Option<usize> {
    let mut matches = events
        .iter()
        .enumerate()
        .filter(|(_, event)| event.kind == kind && event.tick >= earliest_tick);
    let (index, _) = matches.next()?;
    matches.next().is_none().then_some(index)
}

fn select_observed_bomb_events(events: &[ViewerBombEvent]) -> ObservedBombSelection {
    let mut selected = ObservedBombSelection::default();
    let mut plants = events
        .iter()
        .enumerate()
        .filter(|(_, event)| event.kind == "bomb_planted");
    let Some((plant_index, plant)) = plants.next() else {
        return selected;
    };
    if plants.next().is_some()
        || ![plant.x, plant.y, plant.z]
            .into_iter()
            .all(|coordinate| coordinate.is_some_and(|value| value.is_finite()))
    {
        return selected;
    }

    selected.plant_index = Some(plant_index);
    selected.explosion_index = unique_bomb_event_index(events, "bomb_exploded", plant.tick);
    selected.defusal_index = unique_bomb_event_index(events, "bomb_defused", plant.tick);
    selected
}

#[derive(Debug)]
struct ProjectileSample {
    tick: i64,
    trajectory_index: i64,
    entity_id: i64,
    thrower_steamid: Option<String>,
}

/// Match the previous viewer adapter's nearest sample rule. A tied nearest
/// tick across different trajectories stays unknown instead of inventing a
/// projectile identity. Samples arrive in the same tick/trajectory SQL order
/// as get_viewer_frames.
fn select_projectile_trajectory(
    event: &ViewerUtilityEvent,
    samples: &[ProjectileSample],
) -> Option<i64> {
    let entity_id = event.projectile_entity_id?;
    let detonate_tick = event.detonate_tick?;
    let lower = event.throw_tick.saturating_sub(2);
    let upper = detonate_tick.saturating_add(2);
    let candidates = samples.iter().filter(|sample| {
        sample.entity_id == entity_id
            && sample.tick >= lower
            && sample.tick <= upper
            && (sample.thrower_steamid.is_none()
                || sample.thrower_steamid.as_deref() == Some(event.thrower_steamid.as_str()))
    });
    let nearest = candidates.min_by_key(|sample| sample.tick.abs_diff(event.throw_tick))?;
    let distance = nearest.tick.abs_diff(event.throw_tick);
    if samples.iter().any(|sample| {
        sample.entity_id == entity_id
            && sample.tick >= lower
            && sample.tick <= upper
            && (sample.thrower_steamid.is_none()
                || sample.thrower_steamid.as_deref() == Some(event.thrower_steamid.as_str()))
            && sample.trajectory_index != nearest.trajectory_index
            && sample.tick.abs_diff(event.throw_tick) == distance
    }) {
        return None;
    }
    Some(nearest.trajectory_index)
}

/// Hard limit shared by metadata, trajectory linking and 512-tick chunk reads.
/// Over-limit input is rejected; successful reads never silently drop samples.
const MAX_VIEWER_SAMPLES_PER_ROUND: usize = 2_000_000;

fn sample_limit_with_sentinel(budget: usize) -> Result<i64, LibraryError> {
    if budget > MAX_VIEWER_SAMPLES_PER_ROUND {
        return Err(sample_budget_exceeded());
    }
    Ok((budget + 1) as i64)
}

fn sample_budget_exceeded() -> LibraryError {
    LibraryError::InvalidRequest(format!(
        "round position stream exceeds the {MAX_VIEWER_SAMPLES_PER_ROUND}-sample safety bound"
    ))
}

fn collect_sample_rows<T>(
    rows: impl Iterator<Item = rusqlite::Result<T>>,
    budget: usize,
) -> Result<Vec<T>, LibraryError> {
    let mut samples = Vec::new();
    for row in rows {
        // The sentinel row proves overflow. Never return a truncated success.
        if samples.len() == budget {
            return Err(sample_budget_exceeded());
        }
        samples.push(row?);
    }
    Ok(samples)
}

fn read_projectile_link_samples(
    connection: &Connection,
    match_id: i64,
    round_number: i64,
    start_tick: i64,
    end_tick: i64,
    sample_budget: usize,
) -> Result<Vec<ProjectileSample>, LibraryError> {
    let limit = sample_limit_with_sentinel(sample_budget)?;
    let mut query = connection.prepare(
        "SELECT tick,trajectory_index,entity_id,thrower_steamid
         FROM viewer_grenade_positions
         WHERE match_id=?1 AND round_number=?2 AND tick BETWEEN ?3 AND ?4
         ORDER BY tick,trajectory_index LIMIT ?5",
    )?;
    let rows = query.query_map(
        params![match_id, round_number, start_tick, end_tick, limit],
        |row| {
            Ok(ProjectileSample {
                tick: row.get(0)?,
                trajectory_index: row.get(1)?,
                entity_id: row.get(2)?,
                thrower_steamid: row.get(3)?,
            })
        },
    )?;
    collect_sample_rows(rows, sample_budget)
}

pub fn get_viewer_round(
    path: &str,
    match_id: i64,
    round_number: i64,
) -> Result<ViewerRoundMeta, LibraryError> {
    get_viewer_round_inner(path, match_id, round_number, true)
}

fn get_viewer_round_inner(
    path: &str,
    match_id: i64,
    round_number: i64,
    include_utility_links: bool,
) -> Result<ViewerRoundMeta, LibraryError> {
    let connection = open_library(path)?;
    let summary = read_matches(&connection)?
        .into_iter()
        .find(|item| item.id == match_id)
        .ok_or(LibraryError::MatchNotFound(match_id))?;
    let bounds: Option<(Option<i64>, Option<i64>)> = connection
        .query_row(
            "SELECT start_tick,end_tick FROM rounds WHERE match_id=?1 AND round_number=?2",
            params![match_id, round_number],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let (start_tick, end_tick) = bounds.ok_or(LibraryError::RoundNotFound(round_number))?;
    let schema_version: i64 =
        connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let mut player_sample_count = 0;
    let mut grenade_sample_count = 0;
    let mut utility_events = Vec::new();
    let mut bomb_events = Vec::new();
    let mut bomb_events_available = false;
    let mut version = None;
    if schema_version >= 10 && has_column(&connection, "matches", "viewer_position_version")? {
        version = connection.query_row(
            "SELECT viewer_position_version FROM matches WHERE id=?1",
            [match_id],
            |row| row.get::<_, Option<i64>>(0),
        )?;
        if version == Some(1) {
            // Bound even the COUNT scan: a malformed or concurrently enlarged
            // library must not materialize a round before the UI can reject it.
            let limit = sample_limit_with_sentinel(MAX_VIEWER_SAMPLES_PER_ROUND)?;
            player_sample_count = connection.query_row(
                "SELECT COUNT(*) FROM (SELECT 1 FROM viewer_player_positions
                 WHERE match_id=?1 AND round_number=?2 LIMIT ?3)",
                params![match_id, round_number, limit],
                |row| row.get::<_, i64>(0),
            )?;
            grenade_sample_count = connection.query_row(
                "SELECT COUNT(*) FROM (SELECT 1 FROM viewer_grenade_positions
                 WHERE match_id=?1 AND round_number=?2 LIMIT ?3)",
                params![match_id, round_number, limit],
                |row| row.get::<_, i64>(0),
            )?;
            if player_sample_count
                .checked_add(grenade_sample_count)
                .is_none_or(|total| total > MAX_VIEWER_SAMPLES_PER_ROUND as i64)
            {
                return Err(sample_budget_exceeded());
            }
            if has_column(&connection, "utility_events", "projectile_entity_id")?
                && has_column(&connection, "utility_events", "detonate_tick")?
                && has_column(&connection, "utility_events", "thrower_steamid")?
            {
                let mut query = connection.prepare(
                    "SELECT id,utility_type,throw_tick,detonate_tick,
                    detonate_x,detonate_y,detonate_z,projectile_entity_id,
                    thrower_steamid,thrower_name,thrower_team
                 FROM utility_events WHERE match_id=?1 AND round_number=?2
                 ORDER BY throw_tick,id",
                )?;
                utility_events = query
                    .query_map(params![match_id, round_number], |row| {
                        Ok(ViewerUtilityEvent {
                            id: row.get(0)?,
                            utility_type: row.get(1)?,
                            throw_tick: row.get(2)?,
                            detonate_tick: row.get(3)?,
                            detonate_x: row.get(4)?,
                            detonate_y: row.get(5)?,
                            detonate_z: row.get(6)?,
                            projectile_entity_id: row.get(7)?,
                            observed_trajectory_index: None,
                            thrower_steamid: row.get(8)?,
                            thrower_name: row.get(9)?,
                            thrower_side: row.get(10)?,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
            }
            // Chunk reads need bounds and coverage, not materialized trajectory
            // links. Keep link allocation out of every 512-tick chunk request.
            if let Some((start, end)) = start_tick.zip(end_tick).filter(|(start, end)| start < end)
            {
                if include_utility_links && !utility_events.is_empty() && grenade_sample_count > 0 {
                    let samples = read_projectile_link_samples(
                        &connection,
                        match_id,
                        round_number,
                        start,
                        end,
                        grenade_sample_count as usize,
                    )?;
                    for event in &mut utility_events {
                        event.observed_trajectory_index =
                            select_projectile_trajectory(event, &samples);
                    }
                }
            }
            if schema_version >= 11
                && has_column(&connection, "matches", "viewer_bomb_version")?
                && has_column(&connection, "viewer_bomb_events", "kind")?
            {
                bomb_events_available = connection.query_row(
                    "SELECT viewer_bomb_version FROM matches WHERE id=?1",
                    [match_id],
                    |row| row.get::<_, Option<i64>>(0),
                )? == Some(1);
            }
            if bomb_events_available {
                let mut query = connection.prepare(
                    "SELECT source_index,kind,tick,actor_steamid,actor_name,site_id,x,y,z
                     FROM viewer_bomb_events WHERE match_id=?1 AND round_number=?2
                     ORDER BY tick,source_index",
                )?;
                bomb_events = query
                    .query_map(params![match_id, round_number], |row| {
                        Ok(ViewerBombEvent {
                            source_index: row.get(0)?,
                            kind: row.get(1)?,
                            tick: row.get(2)?,
                            actor_steamid: row.get(3)?,
                            actor_name: row.get(4)?,
                            site_id: row.get(5)?,
                            x: row.get(6)?,
                            y: row.get(7)?,
                            z: row.get(8)?,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
            }
        }
    }
    let observed_bomb_selection =
        bomb_events_available.then(|| select_observed_bomb_events(&bomb_events));
    Ok(ViewerRoundMeta {
        round_number,
        start_tick,
        end_tick,
        tick_rate: summary
            .tick_rate
            .filter(|rate| rate.is_finite() && *rate > 0.0),
        map_name: summary.map_name,
        positions_available: version == Some(1)
            && player_sample_count > 0
            && matches!((start_tick,end_tick), (Some(start),Some(end)) if start < end),
        player_sample_count,
        grenade_sample_count,
        utility_events,
        bomb_events,
        bomb_events_available,
        observed_bomb_selection,
    })
}

/// Return at most eight seconds at 64 ticks/s per IPC call. Seek and playback
/// share the same absolute tick; the service never fabricates intermediate
/// coordinates or continues a projectile after its last observed frame.
pub fn get_viewer_frames(
    path: &str,
    match_id: i64,
    round_number: i64,
    start_tick: i64,
    end_tick: i64,
) -> Result<ViewerFrameChunk, LibraryError> {
    let meta = get_viewer_round_inner(path, match_id, round_number, false)?;
    if !meta.positions_available {
        return Err(LibraryError::InvalidRequest(
            "this round has no verified native position stream".into(),
        ));
    }
    if start_tick < meta.start_tick.unwrap()
        || end_tick > meta.end_tick.unwrap()
        || end_tick < start_tick
        || end_tick - start_tick > 511
    {
        return Err(LibraryError::InvalidRequest(
            "window must be within one recorded round and at most 512 ticks".into(),
        ));
    }
    let connection = open_library(path)?;
    read_viewer_frame_chunk(
        &connection,
        match_id,
        round_number,
        start_tick,
        end_tick,
        MAX_VIEWER_SAMPLES_PER_ROUND,
    )
}

fn read_viewer_frame_chunk(
    connection: &Connection,
    match_id: i64,
    round_number: i64,
    start_tick: i64,
    end_tick: i64,
    sample_budget: usize,
) -> Result<ViewerFrameChunk, LibraryError> {
    let player_limit = sample_limit_with_sentinel(sample_budget)?;
    let mut query = connection.prepare(
        "SELECT tick,steamid,name,side,x,y,z,is_alive,health,yaw
        FROM viewer_player_positions WHERE match_id=?1 AND round_number=?2
        AND tick BETWEEN ?3 AND ?4 ORDER BY tick,steamid LIMIT ?5",
    )?;
    let rows = query.query_map(
        params![match_id, round_number, start_tick, end_tick, player_limit],
        |row| {
            let is_alive: Option<i64> = row.get(7)?;
            Ok(ViewerPlayerFrame {
                tick: row.get(0)?,
                steamid: row.get(1)?,
                name: row.get(2)?,
                side: row.get(3)?,
                x: row.get(4)?,
                y: row.get(5)?,
                z: row.get(6)?,
                is_alive: is_alive.map(|value| value != 0),
                health: row.get(8)?,
                yaw: row.get(9)?,
            })
        },
    )?;
    let players = collect_sample_rows(rows, sample_budget)?;
    let remaining_budget = sample_budget - players.len();
    let grenade_limit = sample_limit_with_sentinel(remaining_budget)?;
    let mut query = connection.prepare(
        "SELECT tick,trajectory_index,entity_id,grenade_type,
        thrower_steamid,x,y,z FROM viewer_grenade_positions
        WHERE match_id=?1 AND round_number=?2 AND tick BETWEEN ?3 AND ?4
        ORDER BY tick,trajectory_index LIMIT ?5",
    )?;
    let rows = query.query_map(
        params![match_id, round_number, start_tick, end_tick, grenade_limit],
        |row| {
            Ok(ViewerGrenadeFrame {
                tick: row.get(0)?,
                trajectory_index: row.get(1)?,
                entity_id: row.get(2)?,
                grenade_type: row.get(3)?,
                thrower_steamid: row.get(4)?,
                x: row.get(5)?,
                y: row.get(6)?,
                z: row.get(7)?,
            })
        },
    )?;
    let grenades = collect_sample_rows(rows, remaining_budget)?;
    Ok(ViewerFrameChunk {
        start_tick,
        end_tick,
        players,
        grenades,
    })
}

fn has_column(connection: &Connection, table: &str, column: &str) -> Result<bool, LibraryError> {
    let mut query = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = query
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(columns.iter().any(|name| name == column))
}

fn aggregate_observed_duels(rounds: &[RoundSummary]) -> Option<Vec<ObservedDuelPair>> {
    if !rounds.iter().any(|round| round.deaths.is_some()) {
        return None;
    }

    let mut pairs: BTreeMap<(String, String), Vec<ObservedDuelEvent>> = BTreeMap::new();
    for round in rounds {
        for death in round.deaths.iter().flatten() {
            let Some(attacker) = death.attacker_steamid.as_deref() else {
                continue;
            };
            if attacker == death.victim_steamid.as_str() {
                continue;
            }
            let opposing_sides = matches!(
                (death.attacker_side.as_deref(), death.victim_side.as_deref()),
                (Some("CT"), Some("T")) | (Some("T"), Some("CT"))
            );
            if !opposing_sides {
                continue;
            }
            pairs
                .entry((attacker.to_owned(), death.victim_steamid.clone()))
                .or_default()
                .push(ObservedDuelEvent {
                    round_number: round.round_number,
                    death: death.clone(),
                });
        }
    }

    Some(
        pairs
            .into_iter()
            .map(
                |((attacker_steamid, victim_steamid), events)| ObservedDuelPair {
                    attacker_steamid,
                    victim_steamid,
                    events,
                },
            )
            .collect(),
    )
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
        let effective = if has_column(
            &connection,
            "player_round_combat",
            "effective_enemy_health_damage",
        )? {
            "effective_enemy_health_damage"
        } else {
            "NULL"
        };
        let stable = if has_column(&connection, "player_round_combat", "stable_team")? {
            "stable_team"
        } else {
            "NULL"
        };
        let sql = format!(
            "SELECT round_number, steamid, side, enemy_kills, deaths,
                    enemy_assists, enemy_health_damage, {effective}, {stable}
             FROM player_round_combat WHERE match_id = ?1 ORDER BY round_number, steamid"
        );
        let mut query = connection.prepare(&sql)?;
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
                    effective_enemy_health_damage: row.get(7)?,
                    stable_team: row.get(8)?,
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

    let mut boundary_query = connection.prepare(
        "SELECT round_number, start_tick, end_tick FROM rounds WHERE match_id = ?1 ORDER BY round_number",
    )?;
    let boundaries = boundary_query
        .query_map([match_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<Result<Vec<(i64, Option<i64>, Option<i64>)>, _>>()?;

    let mut deaths_by_round = None;
    if full_combat_coverage && has_ordered_combat_deaths(&connection, &summary.parser_capability)? {
        let mut query = connection.prepare(
            "SELECT source_index, event_order, tick, round_number, weapon,
                    attacker_steamid, attacker_side, victim_steamid, victim_side, assister_steamid
             FROM raw_combat_deaths WHERE match_id = ?1
             ORDER BY tick, event_order, source_index",
        )?;
        let records = query.query_map([match_id], |row| {
            Ok((
                row.get::<_, i64>(3)?,
                RoundDeath {
                    source_index: row.get(0)?,
                    event_order: row.get(1)?,
                    tick: row.get(2)?,
                    weapon: row.get(4)?,
                    attacker_steamid: row.get(5)?,
                    attacker_side: observed_combat_side(row.get(6)?),
                    victim_steamid: row.get(7)?,
                    victim_side: observed_combat_side(row.get(8)?),
                    assister_steamid: row.get(9)?,
                },
            ))
        })?;
        let mut grouped: HashMap<i64, Vec<RoundDeath>> = HashMap::new();
        for record in records {
            let (round, death) = record?;
            if let Some(effective) = effective_event_round(round, death.tick, &boundaries) {
                grouped.entry(effective).or_default().push(death);
            }
        }
        // An older match can be migrated to schema v8 without ever having
        // recorded raw events. Do not turn that absence into a measured zero.
        if !grouped.is_empty() {
            deaths_by_round = Some(grouped);
        }
    }

    let mut hurts_by_round = None;
    if full_combat_coverage && has_ordered_combat_hurts(&connection, &summary.parser_capability)? {
        let effective = if has_column(&connection, "raw_combat_hurts", "effective_health_damage")? {
            "effective_health_damage"
        } else {
            "NULL"
        };
        let sql = format!(
            "SELECT source_index, event_order, tick, round_number, weapon,
                    attacker_steamid, attacker_side, victim_steamid, victim_side, raw_dmg_health,
                    {effective}
             FROM raw_combat_hurts WHERE match_id = ?1
             ORDER BY tick, event_order, source_index"
        );
        let mut query = connection.prepare(&sql)?;
        let records = query.query_map([match_id], |row| {
            Ok((
                row.get::<_, i64>(3)?,
                RoundHurt {
                    source_index: row.get(0)?,
                    event_order: row.get(1)?,
                    tick: row.get(2)?,
                    weapon: row.get(4)?,
                    attacker_steamid: row.get(5)?,
                    attacker_side: observed_combat_side(row.get(6)?),
                    victim_steamid: row.get(7)?,
                    victim_side: observed_combat_side(row.get(8)?),
                    raw_damage: row.get(9)?,
                    effective_damage: row.get(10)?,
                },
            ))
        })?;
        let mut grouped: HashMap<i64, Vec<RoundHurt>> = HashMap::new();
        for record in records {
            let (round, hurt) = record?;
            if let Some(effective) = effective_event_round(round, hurt.tick, &boundaries) {
                grouped.entry(effective).or_default().push(hurt);
            }
        }
        if !grouped.is_empty() {
            hurts_by_round = Some(grouped);
        }
    }

    let mut columns_query = connection.prepare("PRAGMA table_info(rounds)")?;
    let round_columns = columns_query
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    let optional_round = |name: &str| {
        if round_columns.iter().any(|column| column == name) {
            name.to_owned()
        } else {
            "NULL".to_owned()
        }
    };

    let rounds_sql = format!(
        "SELECT round_number, winner, start_tick, end_tick, {}, {}, {}, {}, {}, {}
         FROM rounds WHERE match_id = ?1 ORDER BY round_number",
        optional_round("freeze_end_tick"),
        optional_round("end_reason"),
        optional_round("ct_equip_value"),
        optional_round("t_equip_value"),
        optional_round("team_a_side"),
        optional_round("team_b_side"),
    );
    let mut rounds_query = connection.prepare(&rounds_sql)?;
    let rounds = rounds_query
        .query_map([match_id], |row| {
            let round_number = row.get::<_, i64>(0)?;
            Ok(RoundSummary {
                round_number,
                winner: row.get(1)?,
                start_tick: row.get(2)?,
                freeze_end_tick: row.get(4)?,
                end_tick: row.get(3)?,
                end_reason: row.get(5)?,
                ct_equip_value: row.get(6)?,
                t_equip_value: row.get(7)?,
                team_a_side: row.get(8)?,
                team_b_side: row.get(9)?,
                combat: combat_by_round
                    .get(&round_number)
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
                deaths: deaths_by_round
                    .as_ref()
                    .map(|groups| groups.get(&round_number).cloned().unwrap_or_default()),
                hurts: hurts_by_round
                    .as_ref()
                    .map(|groups| groups.get(&round_number).cloned().unwrap_or_default()),
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
                        stable_team: records
                            .first()
                            .and_then(|first| first.stable_team.clone())
                            .filter(|team| {
                                records
                                    .iter()
                                    .all(|item| item.stable_team.as_ref() == Some(team))
                            }),
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
                        effective_enemy_health_damage: records
                            .iter()
                            .try_fold(0_i64, |sum, item| {
                                Some(sum + item.effective_enemy_health_damage?)
                            }),
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

    let observed_duels = aggregate_observed_duels(&rounds);
    Ok(MatchDetail {
        summary,
        rounds,
        players,
        utility_events,
        utility_breakdown,
        full_combat_coverage,
        observed_duels,
    })
}

#[cfg(test)]
mod observed_duel_tests {
    use super::*;

    fn round(number: i64, deaths: Option<Vec<RoundDeath>>) -> RoundSummary {
        RoundSummary {
            round_number: number,
            winner: None,
            start_tick: None,
            freeze_end_tick: None,
            end_tick: None,
            end_reason: None,
            team_a_side: None,
            team_b_side: None,
            ct_equip_value: None,
            t_equip_value: None,
            combat: None,
            deaths,
            hurts: None,
        }
    }

    fn death(
        index: i64,
        attacker: Option<&str>,
        attacker_side: Option<&str>,
        victim: &str,
        victim_side: Option<&str>,
    ) -> RoundDeath {
        RoundDeath {
            source_index: index,
            event_order: Some(index),
            tick: index * 10,
            weapon: None,
            attacker_steamid: attacker.map(str::to_owned),
            attacker_side: attacker_side.map(str::to_owned),
            victim_steamid: victim.to_owned(),
            victim_side: victim_side.map(str::to_owned),
            assister_steamid: None,
        }
    }

    #[test]
    fn unavailable_and_verified_empty_streams_are_distinct() {
        assert!(aggregate_observed_duels(&[round(1, None)]).is_none());
        assert!(
            aggregate_observed_duels(&[round(1, Some(vec![]))])
                .expect("verified stream")
                .is_empty()
        );
    }

    #[test]
    fn groups_only_observed_opposing_sides_and_keeps_event_order() {
        let rounds = [
            round(
                1,
                Some(vec![
                    death(1, Some("a"), Some("CT"), "b", Some("T")),
                    death(2, Some("a"), Some("CT"), "a", Some("T")),
                    death(3, Some("a"), Some("CT"), "c", Some("CT")),
                    death(4, None, None, "b", Some("T")),
                    death(5, Some("a"), None, "b", Some("T")),
                    death(6, Some("b"), Some("T"), "a", Some("CT")),
                ]),
            ),
            round(
                2,
                Some(vec![death(7, Some("a"), Some("T"), "b", Some("CT"))]),
            ),
        ];
        let pairs = aggregate_observed_duels(&rounds).expect("verified stream");
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].attacker_steamid, "a");
        assert_eq!(pairs[0].victim_steamid, "b");
        assert_eq!(
            pairs[0]
                .events
                .iter()
                .map(|event| (event.round_number, event.death.source_index))
                .collect::<Vec<_>>(),
            vec![(1, 1), (2, 7)]
        );
        assert_eq!(pairs[1].attacker_steamid, "b");
        assert_eq!(pairs[1].victim_steamid, "a");
        assert_eq!(pairs[1].events[0].death.source_index, 6);
    }

    #[test]
    fn same_pair_tied_tick_keeps_input_event_order() {
        let mut first = death(11, Some("a"), Some("CT"), "b", Some("T"));
        let mut second = death(10, Some("a"), Some("CT"), "b", Some("T"));
        first.tick = 700;
        second.tick = 700;
        first.event_order = Some(1);
        second.event_order = Some(2);
        let rounds = [round(1, Some(vec![first, second]))];
        let pairs = aggregate_observed_duels(&rounds).expect("verified stream");
        assert_eq!(pairs.len(), 1);
        assert_eq!(
            pairs[0]
                .events
                .iter()
                .map(|event| (event.death.tick, event.death.source_index))
                .collect::<Vec<_>>(),
            vec![(700, 11), (700, 10)]
        );
    }
    #[test]
    fn sqlite_tied_tick_order_reaches_observed_duel_events() {
        let directory = tempfile::tempdir().expect("temporary SQLite directory");
        let path = directory.path().join("duels.sqlite");
        let connection = Connection::open(&path).expect("SQLite fixture");
        connection
            .execute_batch(
                r#"
            PRAGMA user_version = 11;
            CREATE TABLE matches (
                id INTEGER PRIMARY KEY, file_name TEXT, map_name TEXT,
                score_ct INTEGER, score_t INTEGER, imported_at TEXT, parser_capability TEXT
            );
            CREATE TABLE players (match_id INTEGER, steamid TEXT, name TEXT);
            CREATE TABLE rounds (
                match_id INTEGER, round_number INTEGER, winner TEXT,
                start_tick INTEGER, end_tick INTEGER
            );
            CREATE TABLE utility_events (
                id INTEGER, match_id INTEGER, round_number INTEGER,
                utility_type TEXT, thrower_name TEXT, thrower_team TEXT,
                throw_tick INTEGER
            );
            CREATE TABLE player_round_combat (
                match_id INTEGER, round_number INTEGER, steamid TEXT, side TEXT,
                enemy_kills INTEGER, deaths INTEGER, enemy_assists INTEGER,
                enemy_health_damage INTEGER
            );
            CREATE TABLE raw_combat_deaths (
                match_id INTEGER, source_index INTEGER, event_order INTEGER,
                tick INTEGER, round_number INTEGER, weapon TEXT,
                attacker_steamid TEXT, attacker_side TEXT,
                victim_steamid TEXT, victim_side TEXT, assister_steamid TEXT
            );
            INSERT INTO matches VALUES
                (1, 'tie.dem', 'de_mirage', 1, 0, '2026-01-01', 'native-combat-full-v2');
            INSERT INTO players VALUES (1, 'a', 'Alice'), (1, 'b', 'Bob');
            INSERT INTO rounds VALUES (1, 1, 'CT', 100, 200);
            -- Insert source index 20 first, but event order 1 belongs to 21.
            INSERT INTO raw_combat_deaths VALUES
                (1, 20, 2, 150, 1, 'ak47', 'a', 'CT', 'b', 'T', NULL),
                (1, 21, 1, 150, 1, 'ak47', 'a', 'CT', 'b', 'T', NULL);
        "#,
            )
            .expect("schema and tied deaths");
        drop(connection);
        let detail =
            get_match_detail(path.to_str().expect("UTF-8 path"), 1).expect("ordered SQLite match");
        let deaths = detail.rounds[0].deaths.as_ref().expect("verified deaths");
        assert_eq!(
            deaths
                .iter()
                .map(|event| event.source_index)
                .collect::<Vec<_>>(),
            vec![21, 20]
        );
        let duel = &detail.observed_duels.as_ref().expect("observed duels")[0];
        assert_eq!(duel.attacker_steamid, "a");
        assert_eq!(duel.victim_steamid, "b");
        assert_eq!(
            duel.events
                .iter()
                .map(|event| event.death.source_index)
                .collect::<Vec<_>>(),
            vec![21, 20]
        );
    }
}

#[cfg(test)]
mod observed_bomb_tests {
    use super::*;

    fn event(index: i64, kind: &str, tick: i64, coordinates: Option<[f64; 3]>) -> ViewerBombEvent {
        ViewerBombEvent {
            source_index: index,
            kind: kind.to_owned(),
            tick,
            actor_steamid: None,
            actor_name: None,
            site_id: None,
            x: coordinates.map(|value| value[0]),
            y: coordinates.map(|value| value[1]),
            z: coordinates.map(|value| value[2]),
        }
    }

    #[test]
    fn selects_only_one_grounded_plant_and_unique_later_outcomes() {
        let events = vec![
            event(1, "bomb_exploded", 90, None),
            event(2, "bomb_planted", 100, Some([1.0, 2.0, 3.0])),
            event(4, "bomb_defused", 110, None),
            event(3, "bomb_exploded", 110, None),
        ];
        assert_eq!(
            select_observed_bomb_events(&events),
            ObservedBombSelection {
                plant_index: Some(1),
                explosion_index: Some(3),
                defusal_index: Some(2),
            }
        );
        assert_eq!(
            select_observed_bomb_events(&[]),
            ObservedBombSelection::default()
        );
        assert_eq!(
            select_observed_bomb_events(&[event(5, "bomb_planted", 100, None)]),
            ObservedBombSelection::default()
        );
        assert_eq!(
            select_observed_bomb_events(&[
                event(2, "bomb_planted", 100, Some([1.0, 2.0, 3.0])),
                event(3, "bomb_planted", 101, Some([1.0, 2.0, 3.0])),
            ]),
            ObservedBombSelection::default()
        );
        assert_eq!(
            select_observed_bomb_events(&[event(
                2,
                "bomb_planted",
                100,
                Some([f64::INFINITY, 2.0, 3.0]),
            )]),
            ObservedBombSelection::default()
        );
    }

    #[test]
    fn ambiguous_outcome_does_not_hide_an_independent_unique_outcome() {
        let events = vec![
            event(1, "bomb_planted", 100, Some([1.0, 2.0, 3.0])),
            event(2, "bomb_exploded", 110, None),
            event(3, "bomb_exploded", 110, None),
            event(4, "bomb_defused", 111, None),
        ];
        assert_eq!(
            select_observed_bomb_events(&events),
            ObservedBombSelection {
                plant_index: Some(0),
                explosion_index: None,
                defusal_index: Some(3),
            }
        );
    }

    #[test]
    fn equal_plant_tick_is_eligible_and_partial_coordinates_are_not() {
        let events = vec![
            event(1, "bomb_planted", 100, Some([1.0, 2.0, 3.0])),
            event(2, "bomb_exploded", 100, None),
            event(3, "bomb_defused", 100, None),
        ];
        assert_eq!(
            select_observed_bomb_events(&events),
            ObservedBombSelection {
                plant_index: Some(0),
                explosion_index: Some(1),
                defusal_index: Some(2),
            }
        );
        let mut partial = event(1, "bomb_planted", 100, Some([1.0, 2.0, 3.0]));
        partial.y = None;
        assert_eq!(
            select_observed_bomb_events(&[partial, event(2, "bomb_exploded", 100, None)]),
            ObservedBombSelection::default()
        );
        let mut partial_outcome = event(2, "bomb_exploded", 100, None);
        partial_outcome.x = Some(9.0);
        assert_eq!(
            select_observed_bomb_events(&[
                event(1, "bomb_planted", 100, Some([1.0, 2.0, 3.0])),
                partial_outcome
            ]),
            ObservedBombSelection {
                plant_index: Some(0),
                explosion_index: Some(1),
                defusal_index: None,
            }
        );
    }

    #[test]
    fn sqlite_viewer_contract_preserves_order_empty_and_unavailable() {
        let directory = tempfile::tempdir().expect("temporary SQLite directory");
        let path = directory.path().join("library.sqlite");
        let connection = Connection::open(&path).expect("SQLite fixture");
        connection
            .execute_batch(
                r#"
                PRAGMA user_version = 11;
                CREATE TABLE matches (
                    id INTEGER PRIMARY KEY, file_name TEXT, map_name TEXT,
                    score_ct INTEGER, score_t INTEGER, imported_at TEXT,
                    parser_capability TEXT, viewer_position_version INTEGER,
                    viewer_bomb_version INTEGER
                );
                CREATE TABLE players (match_id INTEGER, steamid TEXT, name TEXT);
                CREATE TABLE rounds (
                    match_id INTEGER, round_number INTEGER, winner TEXT,
                    start_tick INTEGER, end_tick INTEGER
                );
                CREATE TABLE utility_events (
                    id INTEGER, match_id INTEGER, round_number INTEGER,
                    utility_type TEXT, thrower_name TEXT, thrower_team TEXT,
                    throw_tick INTEGER
                );
                CREATE TABLE viewer_player_positions (match_id INTEGER, round_number INTEGER);
                CREATE TABLE viewer_grenade_positions (match_id INTEGER, round_number INTEGER);
                CREATE TABLE viewer_bomb_events (
                    match_id INTEGER, round_number INTEGER, source_index INTEGER,
                    kind TEXT, tick INTEGER, actor_steamid TEXT, actor_name TEXT,
                    site_id INTEGER, x REAL, y REAL, z REAL
                );
                INSERT INTO matches VALUES
                    (1, 'recorded.dem', 'de_mirage', 1, 0, '2026-01-01',
                     'native-combat-full-v2', 1, 1),
                    (2, 'older.dem', 'de_mirage', 1, 0, '2026-01-01',
                     'native-combat-full-v2', 1, NULL);
                INSERT INTO rounds VALUES
                    (1, 1, 'CT', 100, 200),
                    (1, 2, 'T', 201, 300),
                    (2, 1, 'CT', 100, 200);
                INSERT INTO viewer_bomb_events VALUES
                    (1, 1, 12, 'bomb_exploded', 130, NULL, NULL, NULL, NULL, NULL, NULL),
                    (1, 1, 10, 'bomb_planted', 120, 'a', 'Alice', NULL, 1, 2, 3),
                    (1, 1, 11, 'bomb_defused', 130, 'b', 'Bob', NULL, NULL, NULL, NULL),
                    (1, 1, 9, 'bomb_exploded', 110, NULL, NULL, NULL, NULL, NULL, NULL),
                    (2, 1, 50, 'bomb_planted', 120, 'a', 'Alice', NULL, 1, 2, 3);
                "#,
            )
            .expect("schema and observed events");
        drop(connection);

        let path = path.to_str().expect("UTF-8 path");
        let recorded = get_viewer_round(path, 1, 1).expect("recorded round");
        assert_eq!(
            recorded
                .bomb_events
                .iter()
                .map(|event| event.source_index)
                .collect::<Vec<_>>(),
            vec![9, 10, 11, 12]
        );
        assert!(recorded.bomb_events_available);
        assert_eq!(
            recorded.observed_bomb_selection,
            Some(ObservedBombSelection {
                plant_index: Some(1),
                explosion_index: Some(3),
                defusal_index: Some(2),
            })
        );
        let wire = serde_json::to_value(&recorded).expect("IPC serialization");
        assert_eq!(
            wire["observedBombSelection"],
            serde_json::json!({
                "plantIndex": 1,
                "explosionIndex": 3,
                "defusalIndex": 2,
            })
        );

        let empty = get_viewer_round(path, 1, 2).expect("verified empty round");
        assert!(empty.bomb_events_available);
        assert_eq!(
            empty.observed_bomb_selection,
            Some(ObservedBombSelection::default())
        );
        let unavailable = get_viewer_round(path, 2, 1).expect("older import");
        assert!(!unavailable.bomb_events_available);
        assert!(unavailable.bomb_events.is_empty());
        assert_eq!(unavailable.observed_bomb_selection, None);
        let wire = serde_json::to_value(&unavailable).expect("IPC serialization");
        assert!(wire["observedBombSelection"].is_null());
    }
}

#[cfg(test)]
mod observed_projectile_tests {
    use super::*;

    fn event(entity: Option<i64>, detonate: Option<i64>) -> ViewerUtilityEvent {
        ViewerUtilityEvent {
            id: 1,
            utility_type: "smoke".into(),
            throw_tick: 100,
            detonate_tick: detonate,
            detonate_x: Some(1.0),
            detonate_y: Some(2.0),
            detonate_z: Some(3.0),
            projectile_entity_id: entity,
            observed_trajectory_index: None,
            thrower_steamid: "a".into(),
            thrower_name: "Alice".into(),
            thrower_side: "CT".into(),
        }
    }

    fn sample(tick: i64, trajectory: i64, entity: i64, thrower: Option<&str>) -> ProjectileSample {
        ProjectileSample {
            tick,
            trajectory_index: trajectory,
            entity_id: entity,
            thrower_steamid: thrower.map(str::to_owned),
        }
    }

    #[test]
    fn nearest_observed_sample_preserves_unknown_and_tie_rules() {
        let observed = event(Some(7), Some(130));
        assert_eq!(
            select_projectile_trajectory(&event(None, Some(130)), &[]),
            None
        );
        assert_eq!(
            select_projectile_trajectory(&event(Some(7), None), &[]),
            None
        );
        assert_eq!(select_projectile_trajectory(&observed, &[]), None);
        assert_eq!(
            select_projectile_trajectory(
                &observed,
                &[sample(99, 11, 7, Some("a")), sample(101, 12, 7, Some("a")),],
            ),
            None,
            "equidistant different trajectories are ambiguous"
        );
        assert_eq!(
            select_projectile_trajectory(
                &observed,
                &[
                    sample(99, 11, 7, Some("a")),
                    sample(101, 11, 7, Some("a")),
                    sample(102, 12, 7, Some("a")),
                ],
            ),
            Some(11),
            "same-trajectory nearest ties stay linked"
        );
        assert_eq!(
            select_projectile_trajectory(
                &observed,
                &[
                    sample(100, 1, 8, Some("a")),
                    sample(100, 2, 7, Some("b")),
                    sample(101, 3, 7, None),
                    sample(134, 4, 7, Some("a")),
                ],
            ),
            Some(3),
            "entity, thrower, and detonation windows constrain the link"
        );
    }

    #[test]
    fn sqlite_round_returns_links_without_changing_bounded_frame_rows() {
        let directory = tempfile::tempdir().expect("temporary SQLite directory");
        let path = directory.path().join("projectiles.sqlite");
        let connection = Connection::open(&path).expect("SQLite fixture");
        connection
            .execute_batch(
                r#"
            PRAGMA user_version = 11;
            CREATE TABLE matches (
                id INTEGER PRIMARY KEY, file_name TEXT, map_name TEXT,
                score_ct INTEGER, score_t INTEGER, imported_at TEXT,
                parser_capability TEXT, viewer_position_version INTEGER
            );
            CREATE TABLE players (match_id INTEGER, steamid TEXT, name TEXT);
            CREATE TABLE rounds (
                match_id INTEGER, round_number INTEGER, winner TEXT,
                start_tick INTEGER, end_tick INTEGER
            );
            CREATE TABLE utility_events (
                id INTEGER, match_id INTEGER, round_number INTEGER,
                utility_type TEXT, thrower_name TEXT, thrower_team TEXT,
                throw_tick INTEGER, detonate_tick INTEGER,
                detonate_x REAL, detonate_y REAL, detonate_z REAL,
                projectile_entity_id INTEGER, thrower_steamid TEXT
            );
            CREATE TABLE viewer_player_positions (
                match_id INTEGER, round_number INTEGER, tick INTEGER,
                steamid TEXT, name TEXT, side TEXT, x REAL, y REAL, z REAL,
                is_alive INTEGER, health INTEGER, yaw REAL
            );
            CREATE TABLE viewer_grenade_positions (
                match_id INTEGER, round_number INTEGER, tick INTEGER,
                trajectory_index INTEGER, entity_id INTEGER, grenade_type TEXT,
                thrower_steamid TEXT, x REAL, y REAL, z REAL
            );
            INSERT INTO matches VALUES
                (1, 'links.dem', 'de_mirage', 1, 0, '2026-01-01',
                 'native-combat-full-v2', 1);
            INSERT INTO rounds VALUES (1, 1, 'CT', 90, 200);
            INSERT INTO utility_events VALUES
                (1, 1, 1, 'smoke', 'Alice', 'CT', 100, 130, 1, 2, 3, 7, 'a'),
                (2, 1, 1, 'flash', 'Alice', 'CT', 100, 130, 1, 2, 3, 8, 'a'),
                (3, 1, 1, 'he', 'Alice', 'CT', 100, 130, 1, 2, 3, NULL, 'a'),
                (4, 1, 1, 'smoke', 'Alice', 'CT', 90, 92, 1, 2, 3, 9, 'a'),
                (5, 1, 1, 'flash', 'Alice', 'CT', 200, 202, 1, 2, 3, 10, 'a'),
                (6, 1, 1, 'he', 'Alice', 'CT', 201, 203, 1, 2, 3, 11, 'a'),
                (7, 1, 1, 'decoy', 'Alice', 'CT', 89, 91, 1, 2, 3, 12, 'a');
            INSERT INTO viewer_player_positions VALUES
                (1, 1, 100, 'a', 'Alice', 'CT', 1, 2, 3, 1, 100, 0);
            INSERT INTO viewer_grenade_positions VALUES
                (1, 1, 101, 41, 7, 'SmokeProjectile', 'a', 1, 2, 3),
                (1, 1, 102, 42, 7, 'SmokeProjectile', 'a', 1, 2, 3),
                (1, 1, 99, 51, 8, 'FlashProjectile', 'a', 1, 2, 3),
                (1, 1, 101, 52, 8, 'FlashProjectile', 'a', 1, 2, 3),
                (1, 1, 89, 81, 9, 'SmokeProjectile', 'a', 1, 2, 3),
                (1, 1, 90, 82, 9, 'SmokeProjectile', 'a', 1, 2, 3),
                (1, 1, 200, 92, 10, 'FlashProjectile', 'a', 1, 2, 3),
                (1, 1, 201, 91, 10, 'FlashProjectile', 'a', 1, 2, 3),
                (1, 1, 201, 101, 11, 'HEGrenadeProjectile', 'a', 1, 2, 3),
                (1, 1, 89, 111, 12, 'DecoyProjectile', 'a', 1, 2, 3);
        "#,
            )
            .expect("schema and projectile rows");
        drop(connection);

        let path = path.to_str().expect("UTF-8 path");
        let meta = get_viewer_round(path, 1, 1).expect("recorded round");
        assert!(meta.positions_available);
        assert_eq!(
            meta.grenade_sample_count, 10,
            "SQLite retains the out-of-round tails"
        );
        assert_eq!(
            meta.utility_events
                .iter()
                .map(|event| (event.id, event.observed_trajectory_index))
                .collect::<Vec<_>>(),
            vec![
                (7, None),
                (4, Some(82)),
                (1, Some(41)),
                (2, None),
                (3, None),
                (5, Some(92)),
                (6, None),
            ]
        );
        let wire = serde_json::to_value(&meta).expect("IPC serialization");
        assert!(wire["utilityEvents"][0]["observedTrajectoryIndex"].is_null());
        assert_eq!(wire["utilityEvents"][1]["observedTrajectoryIndex"], 82);
        assert_eq!(wire["utilityEvents"][2]["observedTrajectoryIndex"], 41);
        assert!(wire["utilityEvents"][6]["observedTrajectoryIndex"].is_null());
        let chunk = get_viewer_frames(path, 1, 1, 90, 200).expect("bounded frames");
        assert_eq!(chunk.grenades.len(), 6);
        assert_eq!(
            chunk
                .grenades
                .iter()
                .map(|frame| frame.trajectory_index)
                .collect::<Vec<_>>(),
            vec![82, 51, 41, 52, 42, 92]
        );
    }
}

#[cfg(test)]
mod native_reader_contracts {
    use super::*;
    use rusqlite::Connection;
    use std::path::PathBuf;

    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("native-review.sqlite");
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch(r#"
            PRAGMA user_version = 11;
            CREATE TABLE matches (
                id INTEGER PRIMARY KEY, file_name TEXT, map_name TEXT,
                score_ct INTEGER, score_t INTEGER, imported_at TEXT,
                parser_capability TEXT, file_path TEXT, source_sha256 TEXT,
                tick_rate REAL, duration_ticks INTEGER,
                viewer_position_version INTEGER, viewer_bomb_version INTEGER
            );
            CREATE TABLE players (match_id INTEGER, steamid TEXT, name TEXT);
            CREATE TABLE rounds (match_id INTEGER, round_number INTEGER,
                winner TEXT, start_tick INTEGER, freeze_end_tick INTEGER, end_tick INTEGER);
            CREATE TABLE utility_events (
                id INTEGER, match_id INTEGER, round_number INTEGER, utility_type TEXT,
                thrower_steamid TEXT, thrower_name TEXT, thrower_team TEXT,
                throw_tick INTEGER, throw_x REAL, throw_y REAL, throw_z REAL,
                detonate_tick INTEGER, detonate_x REAL, detonate_y REAL, detonate_z REAL,
                projectile_entity_id INTEGER, trajectory TEXT, favorite INTEGER
            );
            CREATE TABLE event_annotations (event_id INTEGER, user_note TEXT);
            CREATE TABLE viewer_player_positions (
                match_id INTEGER, round_number INTEGER, tick INTEGER, steamid TEXT,
                name TEXT, side TEXT, x REAL, y REAL, z REAL,
                is_alive INTEGER, health INTEGER, yaw REAL
            );
            CREATE TABLE viewer_grenade_positions (
                match_id INTEGER, round_number INTEGER, tick INTEGER,
                trajectory_index INTEGER, entity_id INTEGER, grenade_type TEXT,
                thrower_steamid TEXT, x REAL, y REAL, z REAL
            );
            CREATE TABLE viewer_bomb_events (
                match_id INTEGER, round_number INTEGER, source_index INTEGER,
                kind TEXT, tick INTEGER, actor_steamid TEXT, actor_name TEXT,
                site_id INTEGER, x REAL, y REAL, z REAL
            );
            INSERT INTO matches VALUES
                (1,'native.dem','de_mirage',1,0,'2026-10-23','native-combat-full-v2',
                 '/source/native.dem','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                 64,800,1,1),
                (2,'older.dem','de_mirage',0,0,'2026-10-22','reference-raw-facts',
                 NULL,NULL,NULL,NULL,NULL,NULL);
            INSERT INTO players VALUES (1,'a','Alice'),(1,'b','Bob');
            INSERT INTO rounds VALUES (1,1,NULL,100,120,700),(2,1,NULL,100,NULL,700);
            INSERT INTO utility_events VALUES
                (7,1,1,'smoke','a','Alice','CT',110,1,2,3,130,4,5,6,19,
                 '[[110,1,2,3],[120,4,5,6]]',1);
            INSERT INTO event_annotations VALUES (7,'preserve my annotation');
            INSERT INTO viewer_player_positions VALUES
                (1,1,120,'b','Bob',NULL,10,11,12,NULL,NULL,NULL),
                (1,1,120,'a','Alice','CT',1,2,3,1,100,90),
                (1,1,121,'a','Alice','CT',2,3,4,NULL,NULL,NULL),
                (1,1,700,'a','Alice','CT',9,8,7,0,0,180);
            INSERT INTO viewer_grenade_positions VALUES
                (1,1,120,4,19,'SmokeProjectile','a',4,5,6),
                (1,1,119,4,19,'SmokeProjectile','a',3,4,5),
                (1,1,701,4,19,'SmokeProjectile','a',7,8,9);
            INSERT INTO viewer_bomb_events VALUES
                (1,1,2,'bomb_planted',705,'a','Alice',NULL,7,8,9);
        "#).unwrap();
        drop(connection);
        (directory, path)
    }

    #[test]
    fn review_reader_preserves_database_bytes_schema_and_annotations() {
        let (_directory, path) = fixture();
        let before = std::fs::read(&path).unwrap();
        let path_text = path.to_str().unwrap();
        assert_eq!(list_matches(path_text).unwrap().matches.len(), 2);
        let detail = get_match_detail(path_text, 1).unwrap();
        assert_eq!(
            detail.summary.source_sha256.as_deref(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
        assert_eq!(detail.rounds[0].winner, None);
        assert_eq!(detail.rounds[0].team_a_side, None);
        assert_eq!(detail.rounds[0].ct_equip_value, None);
        assert!(!detail.full_combat_coverage);
        assert!(detail.players.iter().all(|player| player.combat.is_none()));
        assert_eq!(
            get_utility_finder(path_text, 1).unwrap()[0]
                .trajectory
                .len(),
            2
        );
        assert_eq!(
            get_viewer_round(path_text, 1, 1).unwrap().bomb_events[0].tick,
            705
        );
        assert_eq!(
            get_viewer_frames(path_text, 1, 1, 100, 611)
                .unwrap()
                .players
                .len(),
            3
        );
        assert_eq!(
            std::fs::read(&path).unwrap(),
            before,
            "native review must not write or migrate the original database"
        );
        let connection =
            Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            11
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT user_note FROM event_annotations WHERE event_id=7",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "preserve my annotation"
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT favorite FROM utility_events WHERE id=7",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
    }

    #[test]
    fn native_replay_returns_only_observed_rows_and_preserves_nullable_facts() {
        let (_directory, path) = fixture();
        let path = path.to_str().unwrap();
        let meta = get_viewer_round(path, 1, 1).unwrap();
        assert!(meta.positions_available);
        assert_eq!(meta.player_sample_count, 4);
        assert_eq!(
            meta.grenade_sample_count, 3,
            "metadata retains observed out-of-round tails"
        );
        let frames = get_viewer_frames(path, 1, 1, 119, 121).unwrap();
        assert_eq!(
            frames
                .players
                .iter()
                .map(|p| (p.tick, p.steamid.as_str()))
                .collect::<Vec<_>>(),
            vec![(120, "a"), (120, "b"), (121, "a")]
        );
        let unknown = &frames.players[1];
        assert_eq!(
            (
                unknown.side.as_deref(),
                unknown.is_alive,
                unknown.health,
                unknown.yaw
            ),
            (None, None, None, None)
        );
        assert_eq!((unknown.x, unknown.y, unknown.z), (10.0, 11.0, 12.0));
        assert_eq!(
            frames.grenades.iter().map(|p| p.tick).collect::<Vec<_>>(),
            vec![119, 120]
        );
        let missing_tick = get_viewer_frames(path, 1, 1, 122, 122).unwrap();
        assert!(missing_tick.players.is_empty());
        assert!(
            missing_tick.grenades.is_empty(),
            "missing observations must not be interpolated or fabricated"
        );
    }

    #[test]
    fn native_replay_refuses_unavailable_and_out_of_round_windows() {
        let (_directory, path) = fixture();
        let path = path.to_str().unwrap();
        let older = get_viewer_round(path, 2, 1).unwrap();
        assert!(!older.positions_available);
        assert!(!older.bomb_events_available);
        assert_eq!(older.observed_bomb_selection, None);
        assert!(get_viewer_frames(path, 2, 1, 100, 120).is_err());
        for (start, end) in [(99, 120), (699, 701), (121, 120), (100, 612)] {
            assert!(
                matches!(
                    get_viewer_frames(path, 1, 1, start, end),
                    Err(LibraryError::InvalidRequest(_))
                ),
                "accepted {start}..{end}"
            );
        }
    }

    #[test]
    fn missing_and_incompatible_database_reads_never_create_or_migrate_files() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("missing.sqlite");
        assert!(matches!(
            list_matches(missing.to_str().unwrap()),
            Err(LibraryError::MissingFile)
        ));
        assert!(!missing.exists());
        let old = directory.path().join("unrelated.sqlite");
        let connection = Connection::open(&old).unwrap();
        connection.execute_batch("PRAGMA user_version=345; CREATE TABLE user_data(value TEXT); INSERT INTO user_data VALUES('do not modify');").unwrap();
        drop(connection);
        let bytes = std::fs::read(&old).unwrap();
        assert!(matches!(
            list_matches(old.to_str().unwrap()),
            Err(LibraryError::InvalidSchema(_))
        ));
        assert_eq!(std::fs::read(&old).unwrap(), bytes);
    }

    #[test]
    fn round_metadata_rejects_combined_sample_overflow_before_reading_events() {
        let (_directory, path) = fixture();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                r#"
            UPDATE utility_events SET throw_tick='invalid-unread-event';
            DROP TABLE viewer_grenade_positions;
            CREATE TABLE pathological_sample_count(n INTEGER);
            INSERT INTO pathological_sample_count VALUES(1999997);
            CREATE VIEW viewer_grenade_positions AS
                WITH RECURSIVE samples(n) AS (
                    VALUES(1) UNION ALL SELECT n+1 FROM samples
                    WHERE n<(SELECT n FROM pathological_sample_count)
                ) SELECT 1 AS match_id,1 AS round_number,120 AS tick,n AS trajectory_index,
                    19 AS entity_id,'SmokeProjectile' AS grenade_type,'a' AS thrower_steamid,
                    1.0 AS x,2.0 AS y,3.0 AS z FROM samples;
        "#,
            )
            .unwrap();
        drop(connection);
        let before = std::fs::read(&path).unwrap();
        let error = get_viewer_round(path.to_str().unwrap(), 1, 1).unwrap_err();
        assert!(matches!(error, LibraryError::InvalidRequest(_)), "{error}");
        assert!(matches!(
            get_viewer_frames(path.to_str().unwrap(), 1, 1, 120, 120),
            Err(LibraryError::InvalidRequest(_))
        ));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "UPDATE pathological_sample_count SET n=1999996; DELETE FROM utility_events;",
            )
            .unwrap();
        drop(connection);
        let exact = get_viewer_round(path.to_str().unwrap(), 1, 1).unwrap();
        assert_eq!(
            exact.player_sample_count + exact.grenade_sample_count,
            2_000_000
        );
        assert_eq!(exact.grenade_sample_count, 1_999_996);
    }

    #[test]
    fn frame_chunk_enforces_a_combined_sample_budget_without_truncation() {
        let (_directory, path) = fixture();
        let connection = open_library(path.to_str().unwrap()).unwrap();
        // Tick120 has two observed players plus one projectile.
        let exact = read_viewer_frame_chunk(&connection, 1, 1, 120, 120, 3).unwrap();
        assert_eq!((exact.players.len(), exact.grenades.len()), (2, 1));
        for budget in [0, 1, 2] {
            assert!(
                matches!(
                    read_viewer_frame_chunk(&connection, 1, 1, 120, 120, budget),
                    Err(LibraryError::InvalidRequest(_))
                ),
                "silently accepted three samples with budget {budget}"
            );
        }
        let empty = read_viewer_frame_chunk(&connection, 1, 1, 122, 122, 0).unwrap();
        assert!(empty.players.is_empty() && empty.grenades.is_empty());
    }

    #[test]
    fn projectile_link_query_rejects_overflow_and_preserves_exact_order() {
        let (_directory, path) = fixture();
        let connection = open_library(path.to_str().unwrap()).unwrap();
        let exact = read_projectile_link_samples(&connection, 1, 1, 119, 120, 2).unwrap();
        assert_eq!(
            exact.iter().map(|sample| sample.tick).collect::<Vec<_>>(),
            vec![119, 120]
        );
        assert!(matches!(
            read_projectile_link_samples(&connection, 1, 1, 119, 120, 1),
            Err(LibraryError::InvalidRequest(_))
        ));
        assert!(matches!(
            read_projectile_link_samples(&connection, 1, 1, 119, 120, 0),
            Err(LibraryError::InvalidRequest(_))
        ));
        assert!(
            read_projectile_link_samples(&connection, 1, 1, 122, 122, 0)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn sentinel_query_and_collector_do_not_accept_unsafe_budgets_or_truncated_rows() {
        assert_eq!(sample_limit_with_sentinel(0).unwrap(), 1);
        assert_eq!(
            sample_limit_with_sentinel(MAX_VIEWER_SAMPLES_PER_ROUND).unwrap(),
            2_000_001
        );
        assert!(sample_limit_with_sentinel(usize::MAX).is_err());
        let mut visited = 0;
        let rows = (0..100).map(|sample| {
            visited += 1;
            Ok::<_, rusqlite::Error>(sample)
        });
        assert!(matches!(
            collect_sample_rows(rows, 2),
            Err(LibraryError::InvalidRequest(_))
        ));
        assert_eq!(
            visited, 3,
            "stop immediately after the one overflow sentinel"
        );
        let rows = (0..2).map(Ok::<_, rusqlite::Error>);
        assert_eq!(collect_sample_rows(rows, 2).unwrap(), vec![0, 1]);
    }
}
