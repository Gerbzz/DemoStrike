//! Versioned CS2 native facts parser. Economy and capture facts are unsupported.
use crate::demo_import::{DemoFactsParser, ImportControl, ParserCapabilities};
use crate::parser::raw::*;
use source2_demo::prelude::*;
use source2_demo::proto::{CMsgPlayerInfo, CSvcMsgServerInfo, Message};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

pub const PARSER_PROVENANCE: &str = "crates.io source2-demo 0.5.9, cs2 feature";

pub struct NativeGrenadeParser;

/// Reject broken framing before handing bytes to source2-demo. That library
/// seeks to the file-info offset from the header without checking it first.
fn validate_format(path: &Path, control: &ImportControl) -> Result<(), String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let length = file.metadata().map_err(|e| e.to_string())?.len();
    if length < 24 { return Err("truncated CS2 demo header (less than 24 bytes)".into()); }
    let mut header = [0_u8; 16];
    file.read_exact(&mut header).map_err(|e| format!("could not read CS2 header: {e}"))?;
    if &header[..8] != b"PBDEMS2\0" { return Err("not a CS2 PBDEMS2 demo".into()); }
    let info_offset = u32::from_le_bytes(header[8..12].try_into().unwrap()) as u64;
    if info_offset < 16 || info_offset >= length.saturating_sub(8) {
        return Err(format!("unsupported or truncated CS2 file-info offset {info_offset} in {length}-byte demo"));
    }
    if control.is_cancelled() { return Err("demo import cancelled".into()); }
    Ok(())
}

impl DemoFactsParser for NativeGrenadeParser {
    fn capabilities(&self) -> ParserCapabilities { ParserCapabilities::NativeCombatFullV2 }

    fn parse(&self, path: &Path, control: &ImportControl) -> Result<RawDemo, String> {
        let path = path.to_path_buf();
        let control = control.clone();
        std::thread::Builder::new().name("source2-demo-0.5.9".into())
            .stack_size(16 * 1024 * 1024)
            .spawn(move || parse_demo(&path, &control))
            .map_err(|e| format!("could not start native demo parser: {e}"))?
            .join().map_err(|_| "native demo parser worker panicked".to_string())?
    }
}

fn parse_demo(path: &Path, control: &ImportControl) -> Result<RawDemo, String> {
        validate_format(path, control)?;
        let mut parser = Parser::from_reader(BufReader::new(File::open(path).map_err(|e| e.to_string())?))
            .map_err(|e| format!("unsupported or damaged CS2 file-info record: {e}"))?;
        let ticks = parser.replay_info().playback_ticks.map(i64::from);
        let seconds = parser.replay_info().playback_time.map(f64::from);
        if !matches!((ticks, seconds), (Some(t), Some(s)) if t > 0 && s.is_finite() && s > 0.0 && (1.0..=1024.0).contains(&(t as f64 / s))) {
            return Err(format!("unsupported CS2 replay timing: ticks={ticks:?}, seconds={seconds:?}"));
        }
        if let Some(expected) = ticks.and_then(|t| u32::try_from(t).ok()) { control.set_expected_ticks(expected); }
        let observer = parser.register_observer::<GrenadeObserver>();
        observer.borrow_mut().control = Some(control.clone());
        parser.run_to_end().map_err(|e| format!("CS2 stream could not be fully decoded: {e}"))?;
        if control.is_cancelled() { return Err("demo import cancelled".into()); }
        validate_combat_event_schema(parser.context())?;
        let state = observer.borrow();
        if !state.errors.is_empty() { return Err(format!("native CS2 facts incomplete: {}", state.errors.join("; "))); }
        let round_events = state.round_events.clone();
        validate_round_outcome_events(&round_events, &state.bomb_defused_ticks,
            &state.bomb_exploded_ticks, &state.ct_death_ticks)?;
        let map_name = state.map_name.clone().filter(|s| !s.trim().is_empty()).ok_or("unsupported CS2 schema: no server map name")?;
        let trajectories = state.trajectories.clone();
        let points: usize = trajectories.iter().map(|t| t.points.len()).sum();
        validate_coverage(&state, &map_name, &trajectories, points)?;
        Ok(RawDemo {
            version: 1,
            header: RawHeader { map_name, server_name: state.server_name.clone(), demo_ticks: ticks, playback_time: seconds },
            players: state.players.values().cloned().collect(),
            round_events,
            throws: state.throws.clone(),
            detonations: state.detonations.clone(),
            blinds: state.blinds.clone(), hurts: state.hurts.clone(), deaths: state.deaths.clone(),
            combat_hurts: state.combat_hurts.clone(), combat_deaths: state.combat_deaths.clone(),
            round_sides: state.round_sides.clone(),
            trajectories,
            trajectory_stats: RawTrajectoryStats { total_rows: points, valid_rows: points, skipped_rows: 0 },
            economy: Vec::new(), positions: state.positions.clone(),
        })
}

fn validate_coverage(state: &GrenadeObserver, map: &str, trajectories: &[RawTrajectory], points: usize) -> Result<(), String> {
    if state.round_events.is_empty() {
        return Err("unsupported CS2 GameRules schema: no round transitions decoded".into());
    }
    if state.last_start_count.is_none() {
        return Err("unsupported CS2 GameRules schema: round-start counter unavailable".into());
    }
    // A combat-only demo can legitimately contain no grenade releases. The
    // equality and ownership checks below still reject unexplained paths.
    let orphan_trajectories = trajectories.iter().filter(|t| t.steamid.is_none() || t.name.is_none() || t.points.len() < 2).count();
    let orphan_detonations = state.detonations.iter().filter(|d| d.steamid.is_none()).count();
    let invalid_points = trajectories.iter().any(|t| t.points.iter().any(|p| p.iter().any(|v| !v.is_finite()))
        || t.points.windows(2).any(|w| w[0][0] > w[1][0]));
    let invalid_positions = state.positions.iter().any(|p| [p.x,p.y,p.z].iter().any(|v| !v.is_some_and(f64::is_finite)));
    let invalid_releases = state.throws.iter().any(|t| [t.x,t.y,t.z].iter().any(|v| !v.is_some_and(f64::is_finite)));
    let invalid_detonations = state.detonations.iter().any(|d| [d.x,d.y,d.z].iter().any(|v| !v.is_some_and(f64::is_finite)));
    if trajectories.len() != state.throws.len() || orphan_trajectories != 0 || orphan_detonations != 0
        || invalid_points || invalid_positions || invalid_releases || invalid_detonations {
        // This is diagnostic only. A nearby release is not proof of causality;
        // persisted links are validated separately by the analysis pipeline.
        let unexplained_paths: Vec<_> = trajectories.iter().filter(|path| {
            let Some(first_tick) = path.points.first().map(|point| point[0]) else { return true };
            !state.throws.iter().any(|release| {
                release.steamid == path.steamid && release.tick.is_some_and(|tick| {
                    (0.0..=32.0).contains(&(first_tick - tick as f64))
                })
            })
        }).take(5).map(|path| format!("entity {} class {} first tick {:?} owner {:?}",
            path.entity_id, path.grenade_type.as_deref().unwrap_or("unknown"),
            path.points.first().map(|point| point[0] as i64), path.steamid)).collect();
        return Err(format!("native grenade coverage incomplete on {map}: releases={}, trajectories={}, samples={points}, detonations={}, positions={}, unowned/short trajectories={orphan_trajectories}, unowned detonations={orphan_detonations}, invalid coordinates={}; paths without nearby same-owner releases (diagnostic only): [{}]; no match was saved",
            state.throws.len(), trajectories.len(), state.detonations.len(), state.positions.len(),
            invalid_points || invalid_positions || invalid_releases || invalid_detonations,
            unexplained_paths.join("; ")));
    }
    Ok(())
}

fn origin(entity: &Entity) -> Option<[f64; 3]> {
    let prefix = "CBodyComponent.m_skeletonInstance.m_vecOrigin";
    let axis = |name: &str| -> Option<f64> {
        let cell = match entity.get_property(&format!("{prefix}.m_cell{name}")).ok()? {
            FieldValue::Unsigned16(v) => f64::from(*v), _ => return None,
        };
        let offset = match entity.get_property(&format!("{prefix}.m_vec{name}")).ok()? {
            FieldValue::Float(v) => f64::from(*v), _ => return None,
        };
        Some((cell - 32.0) * 512.0 + offset)
    };
    Some([axis("X")?, axis("Y")?, axis("Z")?])
}

fn team(number: u8) -> Option<&'static str> {
    match number { 2 => Some("TERRORIST"), 3 => Some("CT"), _ => None }
}

fn game_round(ctx: &Context) -> Option<i64> {
    let rules = ctx.entities().iter().find(|e| e.class().name().contains("GameRules"))?;
    match rules.get_property("m_pGameRules.m_totalRoundsPlayed").ok()? {
        FieldValue::Signed32(v) => Some(i64::from(*v) + 1), _ => None,
    }
}

fn controller_by_user(ctx: &Context, user: i32) -> Option<&Entity> {
    let index = u32::try_from(user).ok()?.checked_add(1)?;
    ctx.entities().iter().find(|e| e.index() == index && e.class().name().contains("PlayerController"))
}

fn controller_by_pawn(ctx: &Context, pawn: u32) -> Option<&Entity> {
    ctx.entities().iter().find(|e| e.class().name().contains("PlayerController") &&
        matches!(e.get_property("m_hPlayerPawn"), Ok(FieldValue::Unsigned32(v)) if *v == pawn))
}

fn controller_identity(controller: &Entity) -> Option<(String, String, String)> {
    let steam = match controller.get_property("m_steamID").ok()? { FieldValue::Unsigned64(v) if *v != 0 => v.to_string(), _ => return None };
    let name = match controller.get_property("m_iszPlayerName").ok()? { FieldValue::String(v) => v.clone(), _ => return None };
    let side = match controller.get_property("m_iTeamNum").ok()? { FieldValue::Unsigned8(v) => team(*v)?.to_string(), _ => return None };
    Some((steam, name, side))
}

fn remember_identity(players: &mut BTreeMap<String, RawPlayer>, sides: &mut Vec<RawRoundSide>,
    identity: &(String, String, String), round: i64) {
    let (steamid, name, side) = identity;
    players.entry(steamid.clone()).or_insert_with(|| RawPlayer {
        steamid: steamid.clone(), name: name.clone(),
    });
    sides.push(RawRoundSide { round, steamid: steamid.clone(), side: side.clone() });
}

/// The event's userid is accepted as a POV candidate only when the live
/// userinfo table independently binds that index to the same Steam account.
fn userinfo_slot(ctx: &Context, user: i32, steam: &str) -> Option<i64> {
    let row = ctx.string_tables().get_by_name("userinfo").ok()?
        .get_row(usize::try_from(user).ok()?).ok()?;
    let info = CMsgPlayerInfo::decode(row.value()?).ok()?;
    (info.userid == Some(user) && info.steamid?.to_string() == steam)
        .then_some(i64::from(user))
}

fn pawn_of<'a>(ctx: &'a Context, controller: &Entity) -> Option<&'a Entity> {
    let handle = match controller.get_property("m_hPlayerPawn").ok()? { FieldValue::Unsigned32(v) => *v, _ => return None };
    ctx.entities().iter().find(|e| e.handle() == handle && e.class().name().contains("PlayerPawn"))
}

fn event_int(event: &GameEvent, name: &str) -> Option<i32> {
    match event.get_value(name).ok()? { EventValue::Int(v) => Some(*v), _ => None }
}
fn event_float(event: &GameEvent, name: &str) -> Option<f64> {
    match event.get_value(name).ok()? { EventValue::Float(v) => Some(f64::from(*v)), _ => None }
}
fn event_string<'a>(event: &'a GameEvent, name: &str) -> Option<&'a str> {
    match event.get_value(name).ok()? { EventValue::String(v) => Some(v.as_str()), _ => None }
}
fn event_bool(event: &GameEvent, name: &str) -> Option<bool> {
    match event.get_value(name).ok()? { EventValue::Bool(v) => Some(*v), _ => None }
}

fn warmup(ctx: &Context) -> Option<bool> {
    let rules = ctx.entities().iter().find(|e| e.class().name().contains("GameRules"))?;
    match rules.get_property("m_pGameRules.m_bWarmupPeriod").ok()? {
        FieldValue::Boolean(v) => Some(*v), _ => None,
    }
}

fn validate_combat_event_schema(ctx: &Context) -> Result<(), String> {
    let definitions: BTreeMap<_, _> = ctx.game_events().iter()
        .map(|(_, definition)| (definition.name(), definition)).collect();
    for (event, fields) in [
        ("player_blind", &[("userid", "short"), ("attacker", "short"), ("entityid", "short"), ("blind_duration", "float")][..]),
        ("player_hurt", &[("userid", "short"), ("attacker", "short"), ("weapon", "string"), ("dmg_health", "short")][..]),
        ("player_death", &[("userid", "short"), ("attacker", "short"), ("assister", "short"), ("assistedflash", "bool"), ("weapon", "string")][..]),
    ] {
        let definition = definitions.get(event).ok_or_else(|| format!("unsupported CS2 combat schema: missing {event} descriptor"))?;
        let available: BTreeMap<_, _> = definition.keys().map(|key| (key.name(), key.type_name())).collect();
        for (field, expected_type) in fields {
            if available.get(field) != Some(expected_type) {
                return Err(format!("unsupported CS2 combat schema: {event}.{field} must be {expected_type}; found {:?}", available.get(field)));
            }
        }
    }
    Ok(())
}

fn round_outcome(win: i32, reason: i32) -> Option<(&'static str, &'static str)> {
    match (win, reason) {
        (3, 8) => Some(("CT", "t_killed")),
        // Source 2 GameRules can report win=3, reason=7 for a defused bomb.
        (3, 7) => Some(("CT", "bomb_defused")),
        // A round can end because its time limit expired.
        (3, 12) => Some(("CT", "time_ran_out")),
        (2, 1) => Some(("T", "bomb_exploded")),
        (2, 9) => Some(("T", "ct_killed")),
        _ => None,
    }
}

fn round_kind(prior: Option<(i32, bool, i32)>, next: (i32, bool, i32)) -> Option<&'static str> {
    let (total, frozen, win) = next;
    match prior {
        None => Some("round_start"),
        Some((_, _, 0)) if win != 0 => Some("round_end"),
        Some((old_total, old_frozen, _)) if (!old_frozen && frozen) || total < old_total
            || (total > old_total && frozen && win == 0) => Some("round_start"),
        Some((_, true, _)) if !frozen => Some("round_freeze_end"),
        _ => None,
    }
}

fn validate_round_outcome_events(rounds: &[RawRoundEvent], defused: &BTreeSet<u32>,
    exploded: &BTreeSet<u32>, ct_deaths: &BTreeSet<u32>) -> Result<(), String> {
    let ticks_for = |reason: &str| -> BTreeSet<u32> {
        rounds.iter().filter(|r| r.kind == "round_end" && r.reason.as_deref() == Some(reason))
            .filter_map(|r| r.tick.and_then(|t| u32::try_from(t).ok())).collect()
    };
    let mapped_defused = ticks_for("bomb_defused");
    if &mapped_defused != defused {
        return Err(format!("bomb_defused game-event ticks {defused:?} do not match round outcomes {mapped_defused:?}"));
    }
    let mapped_exploded = ticks_for("bomb_exploded");
    let unmatched_exploded: Vec<_> = exploded.difference(&mapped_exploded).copied().collect();
    let post_round_explosion = |tick: u32| rounds.iter()
        .filter(|r| r.tick.is_some_and(|t| t <= i64::from(tick)))
        .max_by_key(|r| r.tick)
        .is_some_and(|r| r.kind == "round_end");
    if !mapped_exploded.is_subset(exploded) || unmatched_exploded.iter().any(|t| !post_round_explosion(*t)) {
        return Err(format!("bomb_exploded game-event ticks {exploded:?} do not match in-round outcomes {mapped_exploded:?}; unmatched post-round explosions may be ignored"));
    }
    let missing_ct_death: Vec<_> = ticks_for("ct_killed").difference(ct_deaths).copied().collect();
    if !missing_ct_death.is_empty() {
        return Err(format!("ct_killed round outcomes lack same-tick CT death game events: {missing_ct_death:?}"));
    }
    Ok(())
}

fn include_position(alive_flag: bool, died_this_tick: bool) -> bool {
    // On a lethal grenade tick, player_death can precede the tick-end sample
    // while m_bPawnIsAlive still reports true. The event is the newer state.
    alive_flag && !died_this_tick
}

fn update_same_tick_deaths(dead_users: &mut BTreeSet<i32>, event: &str, user: i32) {
    match event {
        "player_death" => { dead_users.insert(user); }
        "player_spawn" => { dead_users.remove(&user); }
        _ => {}
    }
}

#[derive(Default)]
struct GrenadeObserver {
    control: Option<ImportControl>,
    map_name: Option<String>, server_name: Option<String>,
    players: BTreeMap<String, RawPlayer>,
    throws: Vec<RawThrow>, detonations: Vec<RawDetonation>,
    blinds: Vec<RawBlind>, hurts: Vec<RawHurt>, deaths: Vec<RawDeath>,
    combat_hurts: Vec<RawCombatHurt>, combat_deaths: Vec<RawCombatDeath>,
    round_sides: Vec<RawRoundSide>,
    trajectories: Vec<RawTrajectory>, active_projectiles: BTreeMap<u32, usize>,
    projectile_owners: BTreeMap<u32, (String, String)>,
    positions: Vec<RawPosition>, detonation_ticks: BTreeSet<u32>,
    bomb_defused_ticks: BTreeSet<u32>, bomb_exploded_ticks: BTreeSet<u32>, ct_death_ticks: BTreeSet<u32>,
    dead_users_on_tick: BTreeMap<u32, BTreeSet<i32>>,
    dead_users_until_spawn: BTreeSet<i32>,
    round_events: Vec<RawRoundEvent>, last_rules: Option<(i32, bool, i32)>,
    last_start_count: Option<u8>,
    errors: Vec<String>,
}

#[observer]
#[uses_entities]
#[uses_game_events]
#[uses_string_tables]
impl GrenadeObserver {
    #[on_message]
    fn on_server_info(&mut self, msg: CSvcMsgServerInfo) -> ObserverResult {
        if self.map_name.is_none() { self.map_name = msg.map_name; self.server_name = msg.host_name; }
        Ok(())
    }

    #[on_game_event]
    fn on_event(&mut self, ctx: &Context, event: &GameEvent) -> ObserverResult {
        let name = event.name();
        if name == "bomb_defused" { self.bomb_defused_ticks.insert(ctx.tick()); }
        if name == "bomb_exploded" { self.bomb_exploded_ticks.insert(ctx.tick()); }
        if matches!(name, "player_death" | "player_spawn") {
            if let Some(user) = event_int(event, "userid") {
                update_same_tick_deaths(self.dead_users_on_tick.entry(ctx.tick()).or_default(), name, user);
                update_same_tick_deaths(&mut self.dead_users_until_spawn, name, user);
                if name == "player_death" && controller_by_user(ctx, user).and_then(controller_identity).is_some_and(|(_, _, side)| side == "CT") {
                    self.ct_death_ticks.insert(ctx.tick());
                }
            }
        }
        let utility = match name {
            "weapon_fire" => match event_string(event, "weapon") {
                Some("weapon_smokegrenade") => Some("smoke"),
                Some("weapon_flashbang") => Some("flash"),
                Some("weapon_hegrenade") => Some("he"),
                Some("weapon_molotov") => Some("molotov"),
                Some("weapon_incgrenade") => Some("incendiary"),
                Some("weapon_decoy") => Some("decoy"), _ => None,
            }, _ => None,
        };
        if let Some(utility) = utility {
            let fact = (|| {
                let user = event_int(event, "userid")?;
                let controller = controller_by_user(ctx, user)?;
                let (steam, player_name, side) = controller_identity(controller)?;
                let slot = userinfo_slot(ctx, user, &steam)?;
                let [x, y, z] = origin(pawn_of(ctx, controller)?)?;
                Some((slot, steam, player_name, side, x, y, z, game_round(ctx)?))
            })();
            if let Some((slot, steam, player_name, side, x, y, z, round)) = fact {
                self.players.entry(steam.clone()).or_insert(RawPlayer { steamid: steam.clone(), name: player_name.clone() });
                self.throws.push(RawThrow { tick: Some(i64::from(ctx.tick())), round: Some(round), utility: utility.into(),
                    steamid: Some(steam), name: Some(player_name), team: Some(side), observer_slot: Some(slot),
                    x: Some(x), y: Some(y), z: Some(z) });
            } else { self.errors.push(format!("incomplete {utility} release at tick {}", ctx.tick())); }
        }
        let kind = match name {
            "smokegrenade_detonate" => Some("smoke"), "flashbang_detonate" => Some("flash"),
            "hegrenade_detonate" => Some("he"), "decoy_started" => Some("decoy"),
            "inferno_startburn" => Some("fire"), _ => None,
        };
        if let Some(kind) = kind {
            self.detonation_ticks.insert(ctx.tick());
            let entity = event_int(event, "entityid").and_then(|v| u32::try_from(v).ok());
            let steam = entity.and_then(|id| self.projectile_owners.get(&id).map(|p| p.0.clone()))
                .or_else(|| event_int(event, "userid").and_then(|u| controller_by_user(ctx, u))
                    .and_then(controller_identity).map(|p| p.0));
            let fact = (|| Some((game_round(ctx)?, entity?, event_float(event, "x")?, event_float(event, "y")?, event_float(event, "z")?)))();
            if let Some((round, entity_id, x, y, z)) = fact {
                self.detonations.push(RawDetonation { kind: kind.into(), tick: Some(i64::from(ctx.tick())),
                    round: Some(round), entity_id: Some(i64::from(entity_id)), steamid: steam,
                    x: Some(x), y: Some(y), z: Some(z) });
            } else { self.errors.push(format!("incomplete {kind} detonation at tick {}", ctx.tick())); }
        }
        if matches!(name, "player_blind" | "player_hurt" | "player_death") {
            match warmup(ctx) {
                Some(true) => return Ok(()),
                None => { self.errors.push(format!("warmup GameRules state unavailable for {name} at tick {}", ctx.tick())); return Ok(()); }
                Some(false) => {}
            }
            let round = game_round(ctx);
            let identity = |key: &str| event_int(event, key)
                .and_then(|user| controller_by_user(ctx, user))
                .and_then(controller_identity);
            if matches!(name, "player_hurt" | "player_death") {
                let victim = identity("userid");
                let attacker = identity("attacker");
                let expected_attacker = event_int(event, "attacker").is_some_and(|user| user > 0);
                let assister = if name == "player_death" { identity("assister") } else { None };
                let expected_assister = name == "player_death"
                    && event_int(event, "assister").is_some_and(|user| user > 0);
                if let (Some(round), Some(victim)) = (round, victim) {
                    remember_identity(&mut self.players, &mut self.round_sides, &victim, round);
                    if let Some(ref attacker) = attacker {
                        remember_identity(&mut self.players, &mut self.round_sides, attacker, round);
                    }
                    if let Some(ref assister) = assister {
                        remember_identity(&mut self.players, &mut self.round_sides, assister, round);
                    }
                    if (expected_attacker && attacker.is_none()) || (expected_assister && assister.is_none()) {
                        self.errors.push(format!("incomplete combat {name} identity at tick {}", ctx.tick()));
                    } else if name == "player_hurt" {
                        match event_int(event, "dmg_health") {
                            Some(damage) if damage >= 0 => self.combat_hurts.push(RawCombatHurt {
                                tick: i64::from(ctx.tick()), round,
                                weapon: event_string(event, "weapon").map(str::to_owned),
                                attacker_steamid: attacker.as_ref().map(|value| value.0.clone()),
                                attacker_team: attacker.as_ref().map(|value| value.2.clone()),
                                victim_steamid: victim.0, victim_team: Some(victim.2),
                                damage: i64::from(damage),
                            }),
                            _ => self.errors.push(format!("incomplete combat player_hurt damage at tick {}", ctx.tick())),
                        }
                    } else {
                        self.combat_deaths.push(RawCombatDeath {
                            tick: i64::from(ctx.tick()), round,
                            weapon: event_string(event, "weapon").map(str::to_owned),
                            attacker_steamid: attacker.as_ref().map(|value| value.0.clone()),
                            attacker_team: attacker.as_ref().map(|value| value.2.clone()),
                            victim_steamid: victim.0, victim_team: Some(victim.2),
                            assister_steamid: assister.as_ref().map(|value| value.0.clone()),
                            assister_team: assister.as_ref().map(|value| value.2.clone()),
                        });
                    }
                } else {
                    self.errors.push(format!("incomplete combat {name} round or victim at tick {}", ctx.tick()));
                }
            }
            match name {
                "player_blind" => {
                    let fact = (|| {
                        let (attacker, _, attacker_team) = identity("attacker")?;
                        let (victim, victim_name, victim_team) = identity("userid")?;
                        let duration = event_float(event, "blind_duration")?;
                        if !duration.is_finite() || duration < 0.0 { return None; }
                        Some(RawBlind { tick: Some(i64::from(ctx.tick())), round,
                            attacker_steamid: Some(attacker), attacker_team: Some(attacker_team),
                            victim_steamid: Some(victim), victim_name: Some(victim_name),
                            victim_team: Some(victim_team), duration,
                            entity_id: event_int(event, "entityid").map(i64::from) })
                    })();
                    if let Some(fact) = fact { self.blinds.push(fact); }
                    else { self.errors.push(format!("incomplete player_blind at tick {}", ctx.tick())); }
                }
                "player_hurt" => {
                    let weapon = event_string(event, "weapon");
                    if matches!(weapon, Some("hegrenade" | "inferno" | "molotov" | "incgrenade")) {
                        let fact = (|| {
                            let (attacker, _, attacker_team) = identity("attacker")?;
                            let (victim, _, victim_team) = identity("userid")?;
                            let damage = i64::from(event_int(event, "dmg_health")?);
                            if damage < 0 { return None; }
                            Some(RawHurt { tick: Some(i64::from(ctx.tick())), round,
                                weapon: weapon.map(str::to_string), attacker_steamid: Some(attacker),
                                attacker_team: Some(attacker_team), victim_steamid: Some(victim),
                                victim_team: Some(victim_team), damage })
                        })();
                        if let Some(fact) = fact { self.hurts.push(fact); }
                        else { self.errors.push(format!("incomplete utility player_hurt at tick {}", ctx.tick())); }
                    }
                }
                "player_death" => {
                    let weapon = event_string(event, "weapon");
                    let utility_kill = matches!(weapon, Some("hegrenade" | "inferno" | "molotov" | "incgrenade" | "decoy" | "flashbang" | "smokegrenade"));
                    let assisted_flash = event_bool(event, "assistedflash");
                    if utility_kill || assisted_flash == Some(true) {
                        let victim = identity("userid");
                        let attacker = identity("attacker");
                        let assister = identity("assister");
                        if victim.is_none() || (utility_kill && attacker.is_none()) || (assisted_flash == Some(true) && assister.is_none()) {
                            self.errors.push(format!("incomplete utility player_death at tick {}", ctx.tick()));
                        } else {
                            self.deaths.push(RawDeath { tick: Some(i64::from(ctx.tick())), round,
                                weapon: weapon.map(str::to_string),
                                attacker_steamid: attacker.map(|v| v.0), victim_steamid: victim.map(|v| v.0),
                                assister_steamid: assister.map(|v| v.0), assisted_flash: assisted_flash.unwrap_or(false), utility_kill });
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    #[on_entity]
    fn on_entity(&mut self, ctx: &Context, event: EntityEvents, entity: &Entity) -> ObserverResult {
        if event == EntityEvents::Deleted {
            if entity.class().name().contains("Projectile") { self.active_projectiles.remove(&entity.index()); }
            return Ok(())
        }
        if entity.class().name().contains("GameRules") {
            let get_i32 = |n: &str| match entity.get_property(&format!("m_pGameRules.{n}")) { Ok(FieldValue::Signed32(v)) => Some(*v), _ => None };
            let total = get_i32("m_totalRoundsPlayed");
            let frozen = match entity.get_property("m_pGameRules.m_bFreezePeriod") { Ok(FieldValue::Boolean(v)) => Some(*v), _ => None };
            let win = get_i32("m_iRoundWinStatus");
            let start_count = match entity.get_property("m_pGameRules.m_nRoundStartCount") {
                Ok(FieldValue::Unsigned8(v)) => Some(*v), _ => None,
            };
            if let (Some(total), Some(frozen), Some(win)) = (total, frozen, win) {
                let prior = self.last_rules;
                if prior != Some((total, frozen, win)) {
                    let kind = round_kind(prior, (total, frozen, win));
                    if let Some(kind) = kind {
                        let (winner, reason) = if kind == "round_end" {
                            let reason = get_i32("m_eRoundWinReason");
                            match reason.and_then(|reason| round_outcome(win, reason)) {
                                Some((winner, reason)) => (Some(winner.into()), Some(reason.into())),
                                None => { self.errors.push(format!("unsupported round outcome ({win}, {reason:?}) at {}", ctx.tick())); (None, None) }
                            }
                        } else { (None, None) };
                        if !self.round_events.iter().any(|r| r.kind == kind && r.tick == Some(i64::from(ctx.tick()))) {
                            self.round_events.push(RawRoundEvent { kind: kind.into(), tick: Some(i64::from(ctx.tick())),
                                round: Some(i64::from(total) + 1), winner, reason });
                        }
                        if kind == "round_freeze_end" {
                            let round = i64::from(total) + 1;
                            for controller in ctx.entities().iter().filter(|item| item.class().name().contains("PlayerController")) {
                                if let Some(identity) = controller_identity(controller) {
                                    remember_identity(&mut self.players, &mut self.round_sides, &identity, round);
                                }
                            }
                        }
                    }
                    self.last_rules = Some((total, frozen, win));
                }
                if let Some(count) = start_count {
                    if self.last_start_count.is_some_and(|old| old != count)
                        && !self.round_events.iter().any(|r| r.kind == "round_start" && r.tick == Some(i64::from(ctx.tick()))) {
                        self.round_events.push(RawRoundEvent { kind: "round_start".into(), tick: Some(i64::from(ctx.tick())),
                            round: Some(i64::from(total) + 1), winner: None, reason: None });
                    }
                    self.last_start_count = Some(count);
                }
            }
        }
        if event == EntityEvents::Created && entity.class().name().contains("Projectile") {
            // Entity indices are reused. Never inherit a prior projectile's
            // thrower when the new lifetime lacks a decodable owner.
            self.projectile_owners.remove(&entity.index());
            if let Ok(FieldValue::Unsigned32(handle)) = entity.get_property("m_hThrower") {
                if let Some(controller) = controller_by_pawn(ctx, *handle) {
                    if let Some((steam, name, _)) = controller_identity(controller) {
                        self.projectile_owners.insert(entity.index(), (steam, name));
                    }
                }
            }
            if matches!(entity.class().name(), "CDecoyProjectile" | "CFlashbangProjectile" | "CHEGrenadeProjectile" | "CMolotovProjectile" | "CSmokeGrenadeProjectile") {
                let owner = self.projectile_owners.get(&entity.index());
                let index = self.trajectories.len();
                self.trajectories.push(RawTrajectory { entity_id: i64::from(entity.index()),
                    grenade_type: Some(entity.class().name().into()), steamid: owner.map(|p| p.0.clone()),
                    name: owner.map(|p| p.1.clone()), points: Vec::new() });
                self.active_projectiles.insert(entity.index(), index);
            }
        }
        if event == EntityEvents::Created && entity.class().name() == "CInferno" {
            if let Ok(FieldValue::Unsigned32(handle)) = entity.get_property("m_hOwnerEntity") {
                if let Some(steam) = controller_by_pawn(ctx, *handle).and_then(controller_identity).map(|v| v.0) {
                    for d in self.detonations.iter_mut().filter(|d| d.entity_id == Some(i64::from(entity.index())) && d.tick == Some(i64::from(ctx.tick()))) {
                        d.steamid = Some(steam.clone());
                    }
                }
            }
        }
        Ok(())
    }

    #[on_tick_end]
    fn on_tick_end(&mut self, ctx: &Context) -> ObserverResult {
        if let Some(control) = &self.control { control.set_parsed_tick(ctx.tick()); }
        if self.control.as_ref().is_some_and(ImportControl::is_cancelled) {
            return Err(ParserError::IoError("demo import cancelled".into()).into());
        }
        for entity in ctx.entities().iter().filter(|e| matches!(e.class().name(),
            "CDecoyProjectile" | "CFlashbangProjectile" | "CHEGrenadeProjectile"
            | "CMolotovProjectile" | "CSmokeGrenadeProjectile")) {
            if let Some([x, y, z]) = origin(entity) {
                if let Some(index) = self.active_projectiles.get(&entity.index()) {
                    self.trajectories[*index].points.push([f64::from(ctx.tick()), x, y, z]);
                }
            }
        }
        let dead_this_tick = self.dead_users_on_tick.remove(&ctx.tick()).unwrap_or_default();
        if self.detonation_ticks.contains(&ctx.tick()) {
            for controller in ctx.entities().iter().filter(|e| e.class().name().contains("PlayerController")) {
                let Some(user) = controller.index().checked_sub(1).and_then(|v| i32::try_from(v).ok()) else { continue };
                let alive_flag = matches!(controller.get_property("m_bPawnIsAlive"), Ok(FieldValue::Boolean(true)));
                if !include_position(alive_flag, dead_this_tick.contains(&user) || self.dead_users_until_spawn.contains(&user)) { continue }
                let Some((steam, name, side)) = controller_identity(controller) else { continue };
                let Some([x, y, z]) = pawn_of(ctx, controller).and_then(origin) else { continue };
                self.positions.push(RawPosition { tick: Some(i64::from(ctx.tick())), steamid: Some(steam),
                    name: Some(name), team: Some(side), x: Some(x), y: Some(y), z: Some(z) });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod round_tests {
    use super::*;

    #[test]
    fn combat_only_demo_can_have_zero_grenade_releases() {
        let mut state = GrenadeObserver::default();
        state.last_start_count = Some(1);
        state.round_events.push(RawRoundEvent { kind: "round_start".into(), tick: Some(100),
            round: Some(1), winner: None, reason: None });
        assert!(validate_coverage(&state, "de_test", &[], 0).is_ok());
    }

    #[test]
    fn malformed_and_cancelled_headers_fail_before_decode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.dem");
        std::fs::write(&path, b"PBDEMS2\0").unwrap();
        assert!(validate_format(&path, &ImportControl::new()).unwrap_err().contains("truncated"));
        let mut header = [0_u8; 32];
        header[..8].copy_from_slice(b"PBDEMS2\0");
        header[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        std::fs::write(&path, header).unwrap();
        assert!(validate_format(&path, &ImportControl::new()).unwrap_err().contains("file-info offset"));
        header[8..12].copy_from_slice(&16_u32.to_le_bytes());
        std::fs::write(&path, header).unwrap();
        let cancelled = ImportControl::new();
        cancelled.request_cancel();
        assert!(validate_format(&path, &cancelled).unwrap_err().contains("cancelled"));
    }

    #[test]
    fn maps_only_observed_round_outcomes() {
        assert_eq!(round_outcome(3, 8), Some(("CT", "t_killed")));
        assert_eq!(round_outcome(3, 7), Some(("CT", "bomb_defused")));
        assert_eq!(round_outcome(3, 12), Some(("CT", "time_ran_out")));
        assert_eq!(round_outcome(2, 9), Some(("T", "ct_killed")));
        assert_eq!(round_outcome(2, 1), Some(("T", "bomb_exploded")));
        assert_eq!(round_outcome(2, 7), None);
        assert_eq!(round_outcome(3, 6), None);
    }

    #[test]
    fn bomb_defuse_requires_matching_game_event_tick() {
        let rounds = [RawRoundEvent { kind: "round_end".into(), tick: Some(20_760), round: Some(4),
            winner: Some("CT".into()), reason: Some("bomb_defused".into()) }];
        assert!(validate_round_outcome_events(&rounds, &BTreeSet::from([20_760]), &BTreeSet::new(), &BTreeSet::new()).is_ok());
        assert!(validate_round_outcome_events(&rounds, &BTreeSet::new(), &BTreeSet::new(), &BTreeSet::new()).is_err());
        assert!(validate_round_outcome_events(&rounds, &BTreeSet::from([20_761]), &BTreeSet::new(), &BTreeSet::new()).is_err());
    }

    #[test]
    fn final_win_takes_priority_over_freeze_and_midstream_round_jump_starts_round() {
        assert_eq!(round_kind(Some((20, false, 0)), (20, true, 2)), Some("round_end"));
        assert_eq!(round_kind(Some((0, true, 0)), (3, true, 0)), Some("round_start"));
        assert_eq!(round_kind(Some((3, true, 0)), (3, false, 0)), Some("round_freeze_end"));
    }

    #[test]
    fn terrorist_outcomes_require_matching_game_events() {
        let rounds = [
            RawRoundEvent { kind: "round_end".into(), tick: Some(45_707), round: Some(10),
                winner: Some("T".into()), reason: Some("bomb_exploded".into()) },
            RawRoundEvent { kind: "round_end".into(), tick: Some(52_063), round: Some(11),
                winner: Some("T".into()), reason: Some("ct_killed".into()) },
        ];
        assert!(validate_round_outcome_events(&rounds, &BTreeSet::new(), &BTreeSet::from([45_707]), &BTreeSet::from([52_063])).is_ok());
        assert!(validate_round_outcome_events(&rounds, &BTreeSet::new(), &BTreeSet::new(), &BTreeSet::from([52_063])).is_err());
        assert!(validate_round_outcome_events(&rounds, &BTreeSet::new(), &BTreeSet::from([45_707]), &BTreeSet::new()).is_err());
    }

    #[test]
    fn explosion_after_a_completed_round_does_not_create_another_outcome() {
        let rounds = [RawRoundEvent { kind: "round_end".into(), tick: Some(100), round: Some(1),
            winner: Some("T".into()), reason: Some("ct_killed".into()) }];
        assert!(validate_round_outcome_events(&rounds, &BTreeSet::new(), &BTreeSet::from([101]), &BTreeSet::from([100])).is_ok());
        let active = [rounds[0].clone(), RawRoundEvent { kind: "round_start".into(), tick: Some(110), round: Some(2), winner: None, reason: None }];
        assert!(validate_round_outcome_events(&active, &BTreeSet::new(), &BTreeSet::from([111]), &BTreeSet::from([100])).is_err());
    }


    #[test]
    fn same_tick_death_overrides_stale_alive_flag_for_position_sampling() {
        let mut dead = BTreeSet::new();
        assert!(include_position(true, dead.contains(&6)));
        update_same_tick_deaths(&mut dead, "player_death", 6);
        assert!(!include_position(true, dead.contains(&6)));
        update_same_tick_deaths(&mut dead, "player_spawn", 6);
        assert!(include_position(true, dead.contains(&6)));
        assert!(!include_position(false, false));
    }
}
