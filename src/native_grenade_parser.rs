//! Versioned, deliberately narrow CS2 parser. Only hash-bound evaluation
//! demos are accepted; combat/economy facts are unsupported.
use crate::demo_import::{DemoFactsParser, ImportControl, ParserCapabilities};
use crate::parser::raw::*;
use sha2::{Digest, Sha256};
use source2_demo::prelude::*;
use source2_demo::proto::CSvcMsgServerInfo;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

pub const VALIDATED_ANCIENT_SHA256: &str = "00143fb30897b5697a25ca891bd92bc90293dc29d2ef980c196c5b0f22e0b454";
pub const VALIDATED_ANCIENT_P2_SHA256: &str = "75c464e025350b8a46b7b2808cd55500fea9bd526fcdc659201b1ba97f4f3213";
pub const VALIDATED_NUKE_SHA256: &str = "5c59ee08348a2b7378b8a5dde90f3d4ba3ead21be5b2f74a2cc7f3f8ada39c14";
pub const PARSER_PROVENANCE: &str = "crates.io source2-demo 0.5.9, cs2 feature";

struct ValidatedDemo {
    hash: &'static str,
    map: &'static str,
    throws: usize,
    detonations: usize,
    trajectories: usize,
    trajectory_points: usize,
    positions: usize,
    round_events: usize,
}

const VALIDATED_DEMOS: [ValidatedDemo; 3] = [
    ValidatedDemo { hash: VALIDATED_ANCIENT_SHA256, map: "de_ancient", throws: 37,
        detonations: 37, trajectories: 37, trajectory_points: 23_672, positions: 353,
        round_events: 11 },
    ValidatedDemo { hash: VALIDATED_NUKE_SHA256, map: "de_nuke", throws: 65,
        detonations: 65, trajectories: 65, trajectory_points: 39_618, positions: 583,
        round_events: 15 },
    ValidatedDemo { hash: VALIDATED_ANCIENT_P2_SHA256, map: "de_ancient", throws: 449,
        detonations: 447, trajectories: 449, trajectory_points: 267_400, positions: 4_040,
        round_events: 52 },
];

pub struct NativeGrenadeParser;

fn demo_hash(path: &Path, control: &ImportControl) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        if control.is_cancelled() { return Err("demo import cancelled while hashing".into()); }
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 { break }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

impl DemoFactsParser for NativeGrenadeParser {
    fn capabilities(&self) -> ParserCapabilities { ParserCapabilities::GrenadeAnalysisOnlyV1 }

    fn parse(&self, path: &Path, control: &ImportControl) -> Result<RawDemo, String> {
        let path = path.to_path_buf();
        let control = control.clone();
        std::thread::Builder::new().name("source2-demo-0.5.9".into())
            .stack_size(16 * 1024 * 1024)
            .spawn(move || parse_validated_demo(&path, &control))
            .map_err(|e| format!("could not start native demo parser: {e}"))?
            .join().map_err(|_| "native demo parser worker panicked".to_string())?
    }
}

fn parse_validated_demo(path: &Path, control: &ImportControl) -> Result<RawDemo, String> {
        let hash = demo_hash(path, control)?;
        let validated = VALIDATED_DEMOS.iter().find(|demo| demo.hash == hash)
            .ok_or_else(|| format!("Rust grenade analysis accepts only three hash-validated Ancient/Nuke evaluation demos; this demo is SHA-256 {hash}"))?;
        let mut parser = Parser::from_reader(BufReader::new(File::open(path).map_err(|e| e.to_string())?))
            .map_err(|e| e.to_string())?;
        let ticks = parser.replay_info().playback_ticks.map(i64::from);
        let seconds = parser.replay_info().playback_time.map(f64::from);
        let observer = parser.register_observer::<GrenadeObserver>();
        observer.borrow_mut().control = Some(control.clone());
        parser.run_to_end().map_err(|e| e.to_string())?;
        if control.is_cancelled() { return Err("demo import cancelled".into()); }
        let state = observer.borrow();
        if !state.errors.is_empty() { return Err(format!("native grenade facts incomplete: {}", state.errors.join("; "))); }
        validate_round_outcome_events(&state.round_events, &state.bomb_defused_ticks,
            &state.bomb_exploded_ticks, &state.ct_death_ticks)?;
        let map_name = state.map_name.clone().ok_or("native demo had no server map name")?;
        let trajectories = state.trajectories.clone();
        let points: usize = trajectories.iter().map(|t| t.points.len()).sum();
        if map_name != validated.map || state.throws.len() != validated.throws || state.detonations.len() != validated.detonations
            || trajectories.len() != validated.trajectories || points != validated.trajectory_points || state.positions.len() != validated.positions
            || state.round_events.len() != validated.round_events || state.detonations.iter().any(|d| d.steamid.is_none())
            || trajectories.iter().any(|t| t.steamid.is_none() || t.points.len() < 2) {
            return Err(format!("native grenade facts failed validated-demo coverage gate: map={map_name} throws={} detonations={} trajectories={} points={points} positions={} rounds={} missing_detonation_owner={} missing_trajectory_owner={}",
                state.throws.len(), state.detonations.len(), trajectories.len(), state.positions.len(), state.round_events.len(),
                state.detonations.iter().filter(|d| d.steamid.is_none()).count(), trajectories.iter().filter(|t| t.steamid.is_none()).count()));
        }
        Ok(RawDemo {
            version: 1,
            header: RawHeader { map_name, server_name: state.server_name.clone(), demo_ticks: ticks, playback_time: seconds },
            players: state.players.values().cloned().collect(),
            round_events: state.round_events.clone(),
            throws: state.throws.clone(),
            detonations: state.detonations.clone(),
            blinds: Vec::new(), hurts: Vec::new(), deaths: Vec::new(),
            trajectories,
            trajectory_stats: RawTrajectoryStats { total_rows: points, valid_rows: points, skipped_rows: 0 },
            economy: Vec::new(), positions: state.positions.clone(),
        })
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

fn round_outcome(win: i32, reason: i32) -> Option<(&'static str, &'static str)> {
    match (win, reason) {
        (3, 8) => Some(("CT", "t_killed")),
        // Nuke p1 has bomb_defused game events at the exact ticks where
        // Source 2 GameRules reports win=3, reason=7.
        (3, 7) => Some(("CT", "bomb_defused")),
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
    if &mapped_exploded != exploded {
        return Err(format!("bomb_exploded game-event ticks {exploded:?} do not match round outcomes {mapped_exploded:?}"));
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
    trajectories: Vec<RawTrajectory>, active_projectiles: BTreeMap<u32, usize>,
    projectile_owners: BTreeMap<u32, (String, String)>,
    positions: Vec<RawPosition>, detonation_ticks: BTreeSet<u32>,
    bomb_defused_ticks: BTreeSet<u32>, bomb_exploded_ticks: BTreeSet<u32>, ct_death_ticks: BTreeSet<u32>,
    dead_users_on_tick: BTreeMap<u32, BTreeSet<i32>>,
    round_events: Vec<RawRoundEvent>, last_rules: Option<(i32, bool, i32)>,
    errors: Vec<String>,
}

#[observer]
#[uses_entities]
#[uses_game_events]
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
                let [x, y, z] = origin(pawn_of(ctx, controller)?)?;
                Some((user, steam, player_name, side, x, y, z, game_round(ctx)?))
            })();
            if let Some((user, steam, player_name, side, x, y, z, round)) = fact {
                self.players.entry(steam.clone()).or_insert(RawPlayer { steamid: steam.clone(), name: player_name.clone() });
                self.throws.push(RawThrow { tick: Some(i64::from(ctx.tick())), round: Some(round), utility: utility.into(),
                    steamid: Some(steam), name: Some(player_name), team: Some(side), observer_slot: Some(i64::from(user)),
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
                        self.round_events.push(RawRoundEvent { kind: kind.into(), tick: Some(i64::from(ctx.tick())),
                            round: Some(i64::from(total) + 1), winner, reason });
                    }
                    self.last_rules = Some((total, frozen, win));
                }
            }
        }
        if event == EntityEvents::Created && entity.class().name().contains("Projectile") {
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
                if !include_position(alive_flag, dead_this_tick.contains(&user)) { continue }
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
    fn maps_only_observed_round_outcomes() {
        assert_eq!(round_outcome(3, 8), Some(("CT", "t_killed")));
        assert_eq!(round_outcome(3, 7), Some(("CT", "bomb_defused")));
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
