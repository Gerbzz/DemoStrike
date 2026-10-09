//! Pure analysis: raw demo facts → domain utility events.
//! No IO, no DB — fully unit-testable. New analytics (executes, scoring,
//! heatmaps, lineups) belong in sibling modules of this crate path.

use crate::domain::{ImportSummary, NearbyPlayer, TrajPoint, UtilityPathCoverage, UtilityType, Vec3};
use crate::error::{AppError, AppResult};
use crate::parser::raw::*;
use std::collections::HashMap;

pub const DEFAULT_TICK_RATE: f64 = 64.0;
/// Players within this many units of a detonation are "nearby".
const NEARBY_RADIUS: f64 = 600.0;
/// Max ticks between weapon_fire and first trajectory sample to match.
const THROW_MATCH_WINDOW: i64 = 192;
/// Fire (molotov/incendiary) damage attribution window in seconds.
const FIRE_WINDOW_SECS: f64 = 8.0;
/// Flash assist attribution window in seconds.
const FLASH_ASSIST_WINDOW_SECS: f64 = 5.5;
/// Max stored trajectory points per event (downsampled beyond this).
const MAX_TRAJ_POINTS: usize = 256;
/// Minimum direction change (cosine) that counts as a bounce.
const BOUNCE_COS_THRESHOLD: f64 = 0.75;

#[derive(Debug, Clone)]
pub struct AnalyzedRound {
    pub round_number: i64,
    pub start_tick: Option<i64>,
    pub freeze_end_tick: Option<i64>,
    pub end_tick: Option<i64>,
    pub winner: Option<String>,
    pub end_reason: Option<String>,
    pub ct_equip_value: Option<i64>,
    pub t_equip_value: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct AnalyzedEvent {
    pub round_number: i64,
    pub utility_type: UtilityType,
    pub thrower_steamid: String,
    pub thrower_name: String,
    pub thrower_team: String,
    pub observer_slot: Option<i64>,
    pub throw_tick: i64,
    pub throw_time: f64,
    pub throw_pos: Vec3,
    pub detonate_tick: Option<i64>,
    pub detonate_pos: Option<Vec3>,
    pub land_pos: Option<Vec3>,
    pub trajectory: Vec<TrajPoint>,
    pub bounces: Vec<TrajPoint>,
    pub blind_enemy_duration: f64,
    pub blind_team_duration: f64,
    pub enemies_blinded: i64,
    pub teammates_blinded: i64,
    pub damage_enemy: i64,
    pub damage_team: i64,
    pub kills: i64,
    pub flash_assists: i64,
    pub nearby_players: Vec<NearbyPlayer>,
}

#[derive(Debug, Clone)]
pub struct AnalyzedMatch {
    pub map_name: String,
    pub tick_rate: f64,
    pub duration_ticks: i64,
    pub players: Vec<RawPlayer>,
    pub rounds: Vec<AnalyzedRound>,
    pub events: Vec<AnalyzedEvent>,
    pub score_ct: i64,
    pub score_t: i64,
}

pub fn build_match(raw: &RawDemo) -> AnalyzedMatch {
    let tick_rate = infer_tick_rate(&raw.header);
    let rounds = build_rounds(raw);
    let events = build_events(raw, &rounds, tick_rate);
    let score_ct = rounds.iter().filter(|r| r.winner.as_deref() == Some("CT")).count() as i64;
    let score_t = rounds.iter().filter(|r| r.winner.as_deref() == Some("T")).count() as i64;

    AnalyzedMatch {
        map_name: raw.header.map_name.clone(),
        tick_rate,
        duration_ticks: raw.header.demo_ticks.unwrap_or(0),
        players: raw.players.clone(),
        rounds,
        events,
        score_ct,
        score_t,
    }
}

pub fn validate_import(raw: &RawDemo, analyzed: &AnalyzedMatch) -> AppResult<ImportSummary> {
    let throw_count = raw.throws.len();
    let path_count = analyzed
        .events
        .iter()
        .filter(|event| event.trajectory.len() >= 2)
        .count();
    if !raw.throws.is_empty() && path_count == 0 {
        return Err(AppError::Parse(format!(
            "demo contains {} grenade throws but zero usable replay paths",
            raw.throws.len()
        )));
    }

    let mut by_utility = std::collections::BTreeMap::new();
    for event in &analyzed.events {
        let counts = by_utility
            .entry(event.utility_type.as_str().to_owned())
            .or_insert((event.utility_type, 0_usize, 0_usize));
        counts.1 += 1;
        if event.trajectory.len() >= 2 {
            counts.2 += 1;
        }
    }

    Ok(ImportSummary {
        throw_count,
        path_count,
        trajectory_count: raw.trajectories.len(),
        total_rows: raw.trajectory_stats.total_rows,
        valid_rows: raw.trajectory_stats.valid_rows,
        skipped_rows: raw.trajectory_stats.skipped_rows,
        coverage_by_utility: by_utility
            .into_values()
            .map(|(utility_type, throw_count, path_count)| UtilityPathCoverage {
                utility_type,
                throw_count,
                path_count,
            })
            .collect(),
    })
}

pub fn infer_tick_rate(header: &RawHeader) -> f64 {
    match (header.demo_ticks, header.playback_time) {
        (Some(ticks), Some(secs)) if secs > 1.0 && ticks > 0 => {
            let rate = ticks as f64 / secs;
            if (16.0..=256.0).contains(&rate) {
                rate
            } else {
                DEFAULT_TICK_RATE
            }
        }
        _ => DEFAULT_TICK_RATE,
    }
}

fn build_rounds(raw: &RawDemo) -> Vec<AnalyzedRound> {
    let mut by_round: HashMap<i64, AnalyzedRound> = HashMap::new();
    for ev in &raw.round_events {
        let Some(round) = ev.round else { continue };
        if round < 1 {
            continue;
        }
        let entry = by_round.entry(round).or_insert_with(|| AnalyzedRound {
            round_number: round,
            start_tick: None,
            freeze_end_tick: None,
            end_tick: None,
            winner: None,
            end_reason: None,
            ct_equip_value: None,
            t_equip_value: None,
        });
        match ev.kind.as_str() {
            "round_start" => entry.start_tick = ev.tick.or(entry.start_tick),
            "round_freeze_end" => entry.freeze_end_tick = ev.tick.or(entry.freeze_end_tick),
            "round_end" => {
                entry.end_tick = ev.tick.or(entry.end_tick);
                if ev.winner.is_some() {
                    entry.winner = normalize_team(ev.winner.as_deref());
                }
                if ev.reason.is_some() {
                    entry.end_reason = ev.reason.clone();
                }
            }
            _ => {}
        }
    }

    // Economy: sum equip values per team at each round's freeze-end tick.
    let mut econ_by_tick: HashMap<i64, (i64, i64)> = HashMap::new();
    for e in &raw.economy {
        let (Some(tick), Some(team)) = (e.tick, normalize_team(e.team.as_deref())) else {
            continue;
        };
        let slot = econ_by_tick.entry(tick).or_insert((0, 0));
        match team.as_str() {
            "CT" => slot.0 += e.equip_value,
            "T" => slot.1 += e.equip_value,
            _ => {}
        }
    }
    for round in by_round.values_mut() {
        if let Some(ft) = round.freeze_end_tick {
            if let Some(&(ct, t)) = econ_by_tick.get(&ft) {
                round.ct_equip_value = Some(ct);
                round.t_equip_value = Some(t);
            }
        }
    }

    let mut rounds: Vec<_> = by_round.into_values().collect();
    rounds.sort_by_key(|r| r.round_number);
    rounds
}

/// Map demoparser team names to CT/T.
pub fn normalize_team(team: Option<&str>) -> Option<String> {
    match team {
        Some("CT") | Some("ct") | Some("CounterTerrorist") => Some("CT".into()),
        Some("T") | Some("t") | Some("TERRORIST") | Some("Terrorist") => Some("T".into()),
        _ => None,
    }
}

/// Normalize demoparser grenade_type values onto UtilityType.
fn normalize_grenade_type(t: Option<&str>) -> Option<UtilityType> {
    let t = t?.to_ascii_lowercase();
    if t.contains("smoke") {
        Some(UtilityType::Smoke)
    } else if t.contains("flash") {
        Some(UtilityType::Flash)
    } else if t.contains("he") && !t.contains("hex") {
        Some(UtilityType::He)
    } else if t.contains("molotov") {
        Some(UtilityType::Molotov)
    } else if t.contains("inc") {
        Some(UtilityType::Incendiary)
    } else if t.contains("decoy") {
        Some(UtilityType::Decoy)
    } else if t.contains("fire") {
        Some(UtilityType::Molotov)
    } else {
        None
    }
}

/// Does a detonation `kind` from the extractor match a utility type?
fn detonation_matches(kind: &str, utility: UtilityType) -> bool {
    match kind {
        "smoke" => utility == UtilityType::Smoke,
        "flash" => utility == UtilityType::Flash,
        "he" => utility == UtilityType::He,
        "decoy" => utility == UtilityType::Decoy,
        "fire" => utility == UtilityType::Molotov || utility == UtilityType::Incendiary,
        _ => false,
    }
}

fn dist3(a: Vec3, b: Vec3) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt()
}

/// Detect bounces: significant direction changes between consecutive segments.
pub fn detect_bounces(points: &[TrajPoint]) -> Vec<TrajPoint> {
    let mut bounces = Vec::new();
    if points.len() < 3 {
        return bounces;
    }
    for w in points.windows(3) {
        let (a, b, c) = (w[0], w[1], w[2]);
        let v1 = [b[1] - a[1], b[2] - a[2], b[3] - a[3]];
        let v2 = [c[1] - b[1], c[2] - b[2], c[3] - b[3]];
        let n1 = (v1[0].powi(2) + v1[1].powi(2) + v1[2].powi(2)).sqrt();
        let n2 = (v2[0].powi(2) + v2[1].powi(2) + v2[2].powi(2)).sqrt();
        if n1 < 1.0 || n2 < 1.0 {
            continue; // stationary — not a bounce
        }
        let cos = (v1[0] * v2[0] + v1[1] * v2[1] + v1[2] * v2[2]) / (n1 * n2);
        if cos < BOUNCE_COS_THRESHOLD {
            bounces.push(b);
        }
    }
    bounces
}

/// Downsample a trajectory, always keeping first and last points.
pub fn downsample(points: &[TrajPoint], max: usize) -> Vec<TrajPoint> {
    if points.len() <= max || max < 2 {
        return points.to_vec();
    }
    let step = (points.len() - 1) as f64 / (max - 1) as f64;
    let mut out = Vec::with_capacity(max);
    for i in 0..max {
        out.push(points[(i as f64 * step).round() as usize]);
    }
    *out.last_mut().unwrap() = *points.last().unwrap();
    out
}

fn build_events(raw: &RawDemo, rounds: &[AnalyzedRound], tick_rate: f64) -> Vec<AnalyzedEvent> {
    let freeze_by_round: HashMap<i64, i64> = rounds
        .iter()
        .filter_map(|r| r.freeze_end_tick.map(|t| (r.round_number, t)))
        .collect();

    // Positions grouped by tick for nearby-player lookup.
    let mut pos_by_tick: HashMap<i64, Vec<&RawPosition>> = HashMap::new();
    for p in &raw.positions {
        if let Some(t) = p.tick {
            pos_by_tick.entry(t).or_default().push(p);
        }
    }

    let mut used_traj: Vec<bool> = vec![false; raw.trajectories.len()];
    let mut used_det: Vec<bool> = vec![false; raw.detonations.len()];
    let mut events = Vec::with_capacity(raw.throws.len());

    let mut throws: Vec<&RawThrow> = raw.throws.iter().collect();
    throws.sort_by_key(|t| t.tick.unwrap_or(0));

    for throw in throws {
        let (Some(tick), Some(steamid)) = (throw.tick, throw.steamid.as_deref()) else {
            continue;
        };
        let Some(utility) = UtilityType::from_str(&throw.utility) else {
            continue;
        };
        let round = throw.round.unwrap_or(0).max(1);

        // ---- match a trajectory: same thrower, compatible type, starts near throw tick
        let mut best: Option<(usize, i64)> = None;
        for (i, traj) in raw.trajectories.iter().enumerate() {
            if used_traj[i] || traj.points.is_empty() {
                continue;
            }
            if traj.steamid.as_deref() != Some(steamid) {
                continue;
            }
            if let Some(tt) = normalize_grenade_type(traj.grenade_type.as_deref()) {
                let compatible = tt == utility
                    || (matches!(tt, UtilityType::Molotov | UtilityType::Incendiary)
                        && matches!(utility, UtilityType::Molotov | UtilityType::Incendiary));
                if !compatible {
                    continue;
                }
            }
            let first_tick = traj.points[0][0] as i64;
            let dt = first_tick - tick;
            if (-32..=THROW_MATCH_WINDOW).contains(&dt) {
                if best.map_or(true, |(_, bd)| dt.abs() < bd.abs()) {
                    best = Some((i, dt));
                }
            }
        }

        let (trajectory, bounces, land_pos, entity_id) = match best {
            Some((i, _)) => {
                used_traj[i] = true;
                let traj = &raw.trajectories[i];
                let last = traj.points.last().unwrap();
                (
                    downsample(&traj.points, MAX_TRAJ_POINTS),
                    detect_bounces(&traj.points),
                    Some(Vec3 { x: last[1], y: last[2], z: last[3] }),
                    Some(traj.entity_id),
                )
            }
            None => (Vec::new(), Vec::new(), None, None),
        };

        // ---- match a detonation: entity id first, then thrower + tick proximity
        let mut det_idx: Option<usize> = None;
        if let Some(eid) = entity_id {
            det_idx = raw
                .detonations
                .iter()
                .enumerate()
                .find(|(i, d)| {
                    !used_det[*i]
                        && d.entity_id == Some(eid)
                        && detonation_matches(&d.kind, utility)
                })
                .map(|(i, _)| i);
        }
        if det_idx.is_none() {
            let mut best: Option<(usize, i64)> = None;
            for (i, d) in raw.detonations.iter().enumerate() {
                if used_det[i] || !detonation_matches(&d.kind, utility) {
                    continue;
                }
                if d.steamid.as_deref() != Some(steamid) {
                    continue;
                }
                let Some(dtick) = d.tick else { continue };
                let dt = dtick - tick;
                if dt >= 0 && dt <= (tick_rate * 10.0) as i64 {
                    if best.map_or(true, |(_, bd)| dt < bd) {
                        best = Some((i, dt));
                    }
                }
            }
            det_idx = best.map(|(i, _)| i);
        }

        let (detonate_tick, detonate_pos) = match det_idx {
            Some(i) => {
                used_det[i] = true;
                let d = &raw.detonations[i];
                let pos = match (d.x, d.y) {
                    (Some(x), Some(y)) => Some(Vec3 { x, y, z: d.z.unwrap_or(0.0) }),
                    _ => land_pos,
                };
                (d.tick, pos)
            }
            None => (None, land_pos),
        };

        let thrower_team = normalize_team(throw.team.as_deref()).unwrap_or_default();
        let effect_pos = detonate_pos.or(land_pos);

        // ---- blinds (flash only): same attacker, tick at/after detonation
        let mut blind_enemy = 0.0;
        let mut blind_team = 0.0;
        let mut enemies_blinded = 0;
        let mut teammates_blinded = 0;
        if utility == UtilityType::Flash {
            if let Some(dtick) = detonate_tick {
                for b in &raw.blinds {
                    if b.attacker_steamid.as_deref() != Some(steamid) {
                        continue;
                    }
                    let Some(btick) = b.tick else { continue };
                    if (btick - dtick).abs() > 16 {
                        continue;
                    }
                    let is_self = b.victim_steamid.as_deref() == Some(steamid);
                    if is_self {
                        continue; // self-blind is visible via team totals elsewhere; skip
                    }
                    let Some(victim_team) = normalize_team(b.victim_team.as_deref()) else {
                        continue; // unknown side — don't guess attribution
                    };
                    if victim_team != thrower_team {
                        blind_enemy += b.duration;
                        enemies_blinded += 1;
                    } else {
                        blind_team += b.duration;
                        teammates_blinded += 1;
                    }
                }
            }
        }

        // ---- damage (HE + fire)
        let mut damage_enemy = 0i64;
        let mut damage_team = 0i64;
        if matches!(utility, UtilityType::He | UtilityType::Molotov | UtilityType::Incendiary) {
            if let Some(dtick) = detonate_tick {
                let window = if utility == UtilityType::He {
                    16
                } else {
                    (FIRE_WINDOW_SECS * tick_rate) as i64
                };
                for h in &raw.hurts {
                    if h.attacker_steamid.as_deref() != Some(steamid) {
                        continue;
                    }
                    let hurt_matches_utility = match h.weapon.as_deref() {
                        Some("hegrenade") => utility == UtilityType::He,
                        Some("inferno") | Some("molotov") | Some("incgrenade") => {
                            matches!(utility, UtilityType::Molotov | UtilityType::Incendiary)
                        }
                        _ => false,
                    };
                    if !hurt_matches_utility {
                        continue;
                    }
                    let Some(htick) = h.tick else { continue };
                    if htick < dtick - 8 || htick > dtick + window {
                        continue;
                    }
                    let victim_team = normalize_team(h.victim_team.as_deref()).unwrap_or_default();
                    if !victim_team.is_empty() && victim_team == thrower_team {
                        damage_team += h.damage;
                    } else {
                        damage_enemy += h.damage;
                    }
                }
            }
        }

        // ---- kills + flash assists
        let mut kills = 0i64;
        let mut flash_assists = 0i64;
        if let Some(dtick) = detonate_tick {
            let kill_window = if utility == UtilityType::He {
                16
            } else {
                (FIRE_WINDOW_SECS * tick_rate) as i64
            };
            for d in &raw.deaths {
                let Some(ktick) = d.tick else { continue };
                if d.utility_kill
                    && d.attacker_steamid.as_deref() == Some(steamid)
                    && weapon_matches_utility(d.weapon.as_deref(), utility)
                    && ktick >= dtick - 8
                    && ktick <= dtick + kill_window
                {
                    kills += 1;
                }
                if utility == UtilityType::Flash
                    && d.assisted_flash
                    && d.assister_steamid.as_deref() == Some(steamid)
                    && ktick >= dtick
                    && ktick <= dtick + (FLASH_ASSIST_WINDOW_SECS * tick_rate) as i64
                {
                    flash_assists += 1;
                }
            }
        }

        // ---- nearby players at detonation
        let mut nearby_players = Vec::new();
        if let (Some(dtick), Some(center)) = (detonate_tick, effect_pos) {
            if let Some(rows) = pos_by_tick.get(&dtick) {
                for p in rows {
                    let (Some(x), Some(y)) = (p.x, p.y) else { continue };
                    let ppos = Vec3 { x, y, z: p.z.unwrap_or(center.z) };
                    let d = dist3(ppos, center);
                    if d <= NEARBY_RADIUS {
                        nearby_players.push(NearbyPlayer {
                            steamid: p.steamid.clone().unwrap_or_default(),
                            name: p.name.clone().unwrap_or_default(),
                            team: normalize_team(p.team.as_deref()).unwrap_or_default(),
                            distance: (d * 10.0).round() / 10.0,
                        });
                    }
                }
                nearby_players.sort_by(|a, b| a.distance.total_cmp(&b.distance));
            }
        }

        let throw_time = freeze_by_round
            .get(&round)
            .map(|ft| (tick - ft) as f64 / tick_rate)
            .unwrap_or(0.0);

        events.push(AnalyzedEvent {
            round_number: round,
            utility_type: utility,
            thrower_steamid: steamid.to_string(),
            thrower_name: throw.name.clone().unwrap_or_else(|| "unknown".into()),
            thrower_team,
            observer_slot: throw.observer_slot,
            throw_tick: tick,
            throw_time: (throw_time * 100.0).round() / 100.0,
            throw_pos: Vec3 {
                x: throw.x.unwrap_or(0.0),
                y: throw.y.unwrap_or(0.0),
                z: throw.z.unwrap_or(0.0),
            },
            detonate_tick,
            detonate_pos,
            land_pos,
            trajectory,
            bounces,
            blind_enemy_duration: (blind_enemy * 100.0_f64).round() / 100.0,
            blind_team_duration: (blind_team * 100.0_f64).round() / 100.0,
            enemies_blinded,
            teammates_blinded,
            damage_enemy,
            damage_team,
            kills,
            flash_assists,
            nearby_players,
        });
    }

    events
}

fn weapon_matches_utility(weapon: Option<&str>, utility: UtilityType) -> bool {
    match weapon {
        Some("hegrenade") => utility == UtilityType::He,
        Some("inferno") | Some("molotov") | Some("incgrenade") => {
            matches!(utility, UtilityType::Molotov | UtilityType::Incendiary)
        }
        Some("decoy") => utility == UtilityType::Decoy,
        Some("flashbang") => utility == UtilityType::Flash,
        Some("smokegrenade") => utility == UtilityType::Smoke,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(tick: f64, x: f64, y: f64, z: f64) -> TrajPoint {
        [tick, x, y, z]
    }

    #[test]
    fn bounce_detected_on_direction_change() {
        let points = vec![
            pt(0.0, 0.0, 0.0, 100.0),
            pt(8.0, 100.0, 0.0, 90.0),
            pt(16.0, 200.0, 0.0, 80.0),
            // wall bounce: x reverses
            pt(24.0, 120.0, 10.0, 70.0),
            pt(32.0, 40.0, 20.0, 60.0),
        ];
        let bounces = detect_bounces(&points);
        assert_eq!(bounces.len(), 1);
        assert_eq!(bounces[0][1], 200.0);
    }

    #[test]
    fn no_bounce_on_straight_flight() {
        let points: Vec<TrajPoint> =
            (0..10).map(|i| pt(i as f64 * 8.0, i as f64 * 50.0, 0.0, 100.0 - i as f64)).collect();
        assert!(detect_bounces(&points).is_empty());
    }

    #[test]
    fn downsample_keeps_endpoints() {
        let points: Vec<TrajPoint> = (0..1000).map(|i| pt(i as f64, i as f64, 0.0, 0.0)).collect();
        let ds = downsample(&points, 100);
        assert_eq!(ds.len(), 100);
        assert_eq!(ds[0][0], 0.0);
        assert_eq!(ds[99][0], 999.0);
    }

    #[test]
    fn tick_rate_inferred_and_bounded() {
        let h = RawHeader {
            map_name: "de_mirage".into(),
            server_name: None,
            demo_ticks: Some(64_000),
            playback_time: Some(1000.0),
        };
        assert!((infer_tick_rate(&h) - 64.0).abs() < 0.01);
        let bad = RawHeader { demo_ticks: Some(10), playback_time: Some(1000.0), ..h };
        assert_eq!(infer_tick_rate(&bad), DEFAULT_TICK_RATE);
    }

    #[test]
    fn team_normalization() {
        assert_eq!(normalize_team(Some("CT")).as_deref(), Some("CT"));
        assert_eq!(normalize_team(Some("TERRORIST")).as_deref(), Some("T"));
        assert_eq!(normalize_team(Some("Spectator")), None);
    }

    fn minimal_raw() -> RawDemo {
        serde_json::from_value(serde_json::json!({
            "version": 1,
            "header": { "mapName": "de_mirage", "serverName": null, "demoTicks": 64000, "playbackTime": 1000.0 },
            "players": [{ "steamid": "s1", "name": "alice" }],
            "roundEvents": [
                { "kind": "round_start", "tick": 1000, "round": 1, "winner": null, "reason": null },
                { "kind": "round_freeze_end", "tick": 2000, "round": 1, "winner": null, "reason": null },
                { "kind": "round_end", "tick": 8000, "round": 1, "winner": "CT", "reason": "ct_win_elimination" }
            ],
            "throws": [
                { "tick": 3000, "round": 1, "utility": "flash", "steamid": "s1", "name": "alice",
                  "team": "CT", "x": 0.0, "y": 0.0, "z": 0.0 }
            ],
            "detonations": [
                { "kind": "flash", "tick": 3100, "round": 1, "entityId": 7, "steamid": "s1",
                  "x": 500.0, "y": 0.0, "z": 0.0 }
            ],
            "blinds": [
                { "tick": 3100, "round": 1, "attackerSteamid": "s1", "attackerTeam": "CT",
                  "victimSteamid": "s2", "victimName": "bob", "victimTeam": "TERRORIST",
                  "duration": 2.5, "entityId": 7 }
            ],
            "hurts": [], "deaths": [],
            "trajectories": [
                { "entityId": 7, "type": "CFlashbang", "steamid": "s1", "name": "alice",
                  "points": [[3005.0, 0.0, 0.0, 60.0], [3050.0, 250.0, 0.0, 80.0], [3100.0, 500.0, 0.0, 20.0]] }
            ],
            "economy": [
                { "tick": 2000, "steamid": "s1", "name": "alice", "team": "CT", "equipValue": 4000, "balance": 100 }
            ],
            "positions": [
                { "tick": 3100, "steamid": "s2", "name": "bob", "team": "TERRORIST", "x": 550.0, "y": 30.0, "z": 10.0 }
            ]
        }))
        .expect("valid raw json")
    }

    #[test]
    fn full_pipeline_flash() {
        let raw = minimal_raw();
        let m = build_match(&raw);
        assert_eq!(m.map_name, "de_mirage");
        assert_eq!(m.rounds.len(), 1);
        assert_eq!(m.rounds[0].winner.as_deref(), Some("CT"));
        assert_eq!(m.rounds[0].ct_equip_value, Some(4000));
        assert_eq!(m.events.len(), 1);

        let ev = &m.events[0];
        assert_eq!(ev.utility_type, UtilityType::Flash);
        assert_eq!(ev.detonate_tick, Some(3100));
        assert_eq!(ev.detonate_pos.unwrap().x, 500.0);
        assert_eq!(ev.enemies_blinded, 1);
        assert!((ev.blind_enemy_duration - 2.5).abs() < 1e-9);
        assert_eq!(ev.teammates_blinded, 0);
        assert_eq!(ev.trajectory.len(), 3);
        assert_eq!(ev.nearby_players.len(), 1);
        assert_eq!(ev.nearby_players[0].name, "bob");
        // throw_time: (3000 - 2000) / 64 ≈ 15.63s after freeze end
        assert!((ev.throw_time - 15.63).abs() < 0.02);
    }

    #[test]
    fn import_rejects_grenade_throws_when_every_path_is_missing() {
        let mut raw = minimal_raw();
        raw.trajectories.clear();
        let analyzed = build_match(&raw);
        let error = validate_import(&raw, &analyzed).unwrap_err();
        assert!(error.to_string().contains("zero usable replay paths"));
    }

    #[test]
    fn import_summary_reports_partial_path_coverage() {
        let mut raw = minimal_raw();
        raw.throws.push(RawThrow {
            tick: Some(4000),
            round: Some(1),
            utility: "smoke".into(),
            steamid: Some("s1".into()),
            name: Some("alice".into()),
            team: Some("CT".into()),
            observer_slot: Some(2),
            x: Some(0.0),
            y: Some(0.0),
            z: Some(0.0),
        });
        raw.trajectory_stats.total_rows = 12;
        raw.trajectory_stats.valid_rows = 3;
        raw.trajectory_stats.skipped_rows = 9;
        let analyzed = build_match(&raw);
        let summary = validate_import(&raw, &analyzed).unwrap();
        assert_eq!(summary.throw_count, 2);
        assert_eq!(summary.path_count, 1);
        assert_eq!(summary.trajectory_count, 1);
        assert_eq!(summary.total_rows, 12);
        assert_eq!(summary.valid_rows, 3);
        assert_eq!(summary.skipped_rows, 9);
        assert_eq!(summary.coverage_by_utility[0].utility_type, UtilityType::Flash);
        assert_eq!(summary.coverage_by_utility[0].throw_count, 1);
        assert_eq!(summary.coverage_by_utility[0].path_count, 1);
        assert_eq!(summary.coverage_by_utility[1].utility_type, UtilityType::Smoke);
        assert_eq!(summary.coverage_by_utility[1].throw_count, 1);
        assert_eq!(summary.coverage_by_utility[1].path_count, 0);
    }

    #[test]
    fn acceptance_raw_demo_reports_coverage_when_requested() {
        let Some(path) = std::env::var_os("CSTHROWBACK_ACCEPTANCE_RAW") else {
            return;
        };
        let raw: RawDemo = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let analyzed = build_match(&raw);
        let summary = validate_import(&raw, &analyzed).unwrap();
        eprintln!("real demo import summary: {summary:#?}");
        assert!(summary.throw_count > 0);
        assert!(summary.path_count > 0);
        assert!(summary.valid_rows > 0);
        assert!(!summary.coverage_by_utility.is_empty());
    }

    #[test]
    fn unmatched_throw_still_recorded() {
        let mut raw = minimal_raw();
        raw.detonations.clear();
        raw.trajectories.clear();
        let m = build_match(&raw);
        assert_eq!(m.events.len(), 1);
        assert!(m.events[0].detonate_tick.is_none());
        assert!(m.events[0].trajectory.is_empty());
    }
}
