//! Native Rust replay state and indexed recorded scenes.
//!
//! Reads only observed samples in the demo's absolute tick domain. Missing
//! carrier/equipment facts are never represented as false observations. All
//! sample indexing happens at load time, not in the animation frame loop.
use crate::match_review::{
    RoundSummary, ViewerBombEvent, ViewerFrameChunk, ViewerGrenadeFrame, ViewerPlayerFrame,
    ViewerRoundMeta,
};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::time::Duration;

const MAX_STREAM_SAMPLES: usize = 2_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayError {
    InvalidBounds,
    MismatchedRound,
    InvalidTickRate,
    InvalidSpeed,
    InvalidChunk,
    InvalidFrame,
    DuplicatePlayer,
    DuplicateProjectile,
    TooManySamples,
}
impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidBounds => "recorded round tick bounds are unavailable or invalid",
            Self::MismatchedRound => "recorded combat belongs to a different round",
            Self::InvalidTickRate => "recorded tick rate is unavailable or outside 1..=1024 Hz",
            Self::InvalidSpeed => "playback speed must be finite and within 0.25..=8x",
            Self::InvalidChunk => "replay chunk is outside its recorded round",
            Self::InvalidFrame => "replay sample has invalid tick, identity or coordinates",
            Self::DuplicatePlayer => "duplicate player identity at one recorded tick",
            Self::DuplicateProjectile => "duplicate projectile identity at one recorded tick",
            Self::TooManySamples => "replay stream exceeds the native sample limit",
        })
    }
}
impl std::error::Error for ReplayError {}

fn validate_bounds(start: i64, end: i64, rate: f64) -> Result<(), ReplayError> {
    if start < 0 || end <= start {
        return Err(ReplayError::InvalidBounds);
    }
    if !rate.is_finite() || !(1.0..=1024.0).contains(&rate) {
        return Err(ReplayError::InvalidTickRate);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SamplePresence {
    Unavailable,
    OutsideRound,
    MissingAtTick,
    Observed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BombPhase {
    Planted,
    Defused,
    Exploded,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ObservedBomb<'a> {
    pub source_index: i64,
    pub plant_tick: i64,
    pub position: [f64; 3],
    pub phase: BombPhase,
    pub actor_name: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BombState<'a> {
    Unavailable,
    /// No unique observed plant position applies at this tick. In particular,
    /// this says nothing about which player carries or last carried the bomb.
    NoObservedPosition,
    Observed(ObservedBomb<'a>),
}

pub struct ReplayScene<'a> {
    pub tick: i64,
    pub players: &'a [ViewerPlayerFrame],
    pub grenades: &'a [ViewerGrenadeFrame],
    pub player_presence: SamplePresence,
    pub grenade_presence: SamplePresence,
    pub bomb: BombState<'a>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrajectoryPoint {
    pub tick: i64,
    pub position: [f64; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayEventKind {
    Grenade,
    Damage,
    Death,
    Bomb,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayEvent {
    pub key: String,
    pub tick: i64,
    pub kind: ReplayEventKind,
    pub label: String,
    pub seekable: bool,
    /// Known ordering from the recorded combat stream. None is unknown, not
    /// an invented zero. Other same-tick ties use a stable presentation key.
    pub source_order: Option<i64>,
    pub source_index: i64,
}

pub struct ReplayRound {
    pub meta: ViewerRoundMeta,
    pub events: Vec<ReplayEvent>,
    players: BTreeMap<i64, Vec<ViewerPlayerFrame>>,
    grenades: BTreeMap<i64, Vec<ViewerGrenadeFrame>>,
    trajectories: BTreeMap<i64, Vec<TrajectoryPoint>>,
}

impl ReplayRound {
    pub fn new(
        meta: ViewerRoundMeta,
        chunks: Vec<ViewerFrameChunk>,
        round: Option<&RoundSummary>,
    ) -> Result<Self, ReplayError> {
        let (Some(start), Some(end)) = (meta.start_tick, meta.end_tick) else {
            return Err(ReplayError::InvalidBounds);
        };
        let Some(rate) = meta.tick_rate else {
            return Err(ReplayError::InvalidTickRate);
        };
        validate_bounds(start, end, rate)?;
        if round.is_some_and(|round| round.round_number != meta.round_number) {
            return Err(ReplayError::MismatchedRound);
        }
        let mut players: BTreeMap<i64, Vec<ViewerPlayerFrame>> = BTreeMap::new();
        let mut grenades: BTreeMap<i64, Vec<ViewerGrenadeFrame>> = BTreeMap::new();
        let mut trajectories: BTreeMap<i64, Vec<TrajectoryPoint>> = BTreeMap::new();
        let mut player_total = 0_usize;
        let mut grenade_total = 0_usize;
        for chunk in chunks {
            if chunk.start_tick < start || chunk.end_tick > end || chunk.end_tick < chunk.start_tick
            {
                return Err(ReplayError::InvalidChunk);
            }
            player_total = player_total
                .checked_add(chunk.players.len())
                .ok_or(ReplayError::TooManySamples)?;
            grenade_total = grenade_total
                .checked_add(chunk.grenades.len())
                .ok_or(ReplayError::TooManySamples)?;
            if player_total > MAX_STREAM_SAMPLES || grenade_total > MAX_STREAM_SAMPLES {
                return Err(ReplayError::TooManySamples);
            }
            for player in chunk.players {
                if !(chunk.start_tick..=chunk.end_tick).contains(&player.tick)
                    || player.steamid.is_empty()
                    || ![player.x, player.y, player.z].iter().all(|v| v.is_finite())
                    || player.yaw.is_some_and(|v| !v.is_finite())
                    || player.health.is_some_and(|v| v < 0)
                {
                    return Err(ReplayError::InvalidFrame);
                }
                players.entry(player.tick).or_default().push(player);
            }
            for grenade in chunk.grenades {
                if !(chunk.start_tick..=chunk.end_tick).contains(&grenade.tick)
                    || ![grenade.x, grenade.y, grenade.z]
                        .iter()
                        .all(|v| v.is_finite())
                {
                    return Err(ReplayError::InvalidFrame);
                }
                trajectories
                    .entry(grenade.trajectory_index)
                    .or_default()
                    .push(TrajectoryPoint {
                        tick: grenade.tick,
                        position: [grenade.x, grenade.y, grenade.z],
                    });
                grenades.entry(grenade.tick).or_default().push(grenade);
            }
        }
        for samples in players.values_mut() {
            samples.sort_by(|a, b| a.steamid.cmp(&b.steamid));
            if samples.windows(2).any(|p| p[0].steamid == p[1].steamid) {
                return Err(ReplayError::DuplicatePlayer);
            }
        }
        for samples in grenades.values_mut() {
            samples.sort_by_key(|p| p.trajectory_index);
            if samples
                .windows(2)
                .any(|p| p[0].trajectory_index == p[1].trajectory_index)
            {
                return Err(ReplayError::DuplicateProjectile);
            }
        }
        for samples in trajectories.values_mut() {
            samples.sort_by_key(|p| p.tick);
        }
        let names: HashMap<&str, &str> = players
            .values()
            .flatten()
            .map(|p| (p.steamid.as_str(), p.name.as_str()))
            .collect();
        let events = recorded_events(&meta, round, &names);
        Ok(Self {
            meta,
            events,
            players,
            grenades,
            trajectories,
        })
    }

    pub fn meta(&self) -> &ViewerRoundMeta {
        &self.meta
    }
    pub fn start_tick(&self) -> i64 {
        self.meta.start_tick.expect("validated bounds")
    }
    pub fn end_tick(&self) -> i64 {
        self.meta.end_tick.expect("validated bounds")
    }
    pub fn tick_rate(&self) -> f64 {
        self.meta.tick_rate.expect("validated tick rate")
    }
    pub fn players_at(&self, tick: i64) -> &[ViewerPlayerFrame] {
        if !self.meta.positions_available {
            return &[];
        }
        self.players.get(&tick).map_or(&[], Vec::as_slice)
    }
    pub fn grenades_at(&self, tick: i64) -> &[ViewerGrenadeFrame] {
        if !self.meta.positions_available {
            return &[];
        }
        self.grenades.get(&tick).map_or(&[], Vec::as_slice)
    }
    pub fn trajectory_until(&self, trajectory_index: i64, tick: i64) -> &[TrajectoryPoint] {
        if !self.meta.positions_available {
            return &[];
        }
        let Some(path) = self.trajectories.get(&trajectory_index) else {
            return &[];
        };
        &path[..path.partition_point(|p| p.tick <= tick)]
    }
    pub fn scene(&self, tick: i64) -> ReplayScene<'_> {
        let inside = (self.start_tick()..=self.end_tick()).contains(&tick);
        let players = if inside { self.players_at(tick) } else { &[] };
        let grenades = if inside { self.grenades_at(tick) } else { &[] };
        let presence = |empty| {
            if !inside {
                SamplePresence::OutsideRound
            } else if !self.meta.positions_available {
                SamplePresence::Unavailable
            } else if empty {
                SamplePresence::MissingAtTick
            } else {
                SamplePresence::Observed
            }
        };
        ReplayScene {
            tick,
            players,
            grenades,
            player_presence: presence(players.is_empty()),
            grenade_presence: presence(grenades.is_empty()),
            bomb: self.bomb_at(tick),
        }
    }
    pub fn bomb_at(&self, tick: i64) -> BombState<'_> {
        if !self.meta.bomb_events_available {
            return BombState::Unavailable;
        }
        if !(self.start_tick()..=self.end_tick()).contains(&tick) {
            return BombState::NoObservedPosition;
        }
        let Some(selection) = &self.meta.observed_bomb_selection else {
            return BombState::NoObservedPosition;
        };
        let Some(plant) = selection
            .plant_index
            .and_then(|i| self.meta.bomb_events.get(i))
        else {
            return BombState::NoObservedPosition;
        };
        if plant.kind != "bomb_planted"
            || plant.tick > tick
            || !(self.start_tick()..=self.end_tick()).contains(&plant.tick)
        {
            return BombState::NoObservedPosition;
        }
        let (Some(x), Some(y), Some(z)) = (plant.x, plant.y, plant.z) else {
            return BombState::NoObservedPosition;
        };
        if ![x, y, z].iter().all(|v| v.is_finite()) {
            return BombState::NoObservedPosition;
        }
        if selection.explosion_index.is_some() && selection.defusal_index.is_some() {
            return BombState::NoObservedPosition;
        }
        let terminal = |index: Option<usize>, expected: &str| -> Option<&ViewerBombEvent> {
            let event = index.and_then(|i| self.meta.bomb_events.get(i))?;
            (event.kind == expected && event.tick >= plant.tick).then_some(event)
        };
        let phase = if terminal(selection.explosion_index, "bomb_exploded")
            .is_some_and(|e| e.tick <= tick)
        {
            BombPhase::Exploded
        } else if terminal(selection.defusal_index, "bomb_defused").is_some_and(|e| e.tick <= tick)
        {
            BombPhase::Defused
        } else {
            BombPhase::Planted
        };
        BombState::Observed(ObservedBomb {
            source_index: plant.source_index,
            plant_tick: plant.tick,
            position: [x, y, z],
            phase,
            actor_name: plant.actor_name.as_deref(),
        })
    }
}

fn recorded_events(
    meta: &ViewerRoundMeta,
    round: Option<&RoundSummary>,
    names: &HashMap<&str, &str>,
) -> Vec<ReplayEvent> {
    let start = meta.start_tick.expect("validated start");
    let end = meta.end_tick.expect("validated end");
    let mut events = Vec::new();
    let name = |steamid: Option<&str>| {
        steamid.map_or_else(
            || "world".to_owned(),
            |s| names.get(s).copied().unwrap_or(s).to_owned(),
        )
    };
    for event in &meta.utility_events {
        let Some(tick) = event
            .detonate_tick
            .filter(|tick| (start..=end).contains(tick))
        else {
            continue;
        };
        events.push(ReplayEvent {
            key: format!("utility-{}", event.id),
            tick,
            kind: ReplayEventKind::Grenade,
            label: format!("{} detonated · {}", event.utility_type, event.thrower_name),
            seekable: true,
            source_order: None,
            source_index: event.id,
        });
    }
    if let Some(round) = round {
        for event in round
            .hurts
            .iter()
            .flatten()
            .filter(|e| (start..=end).contains(&e.tick))
        {
            let amount = event.effective_damage.map_or_else(
                || format!("{} raw damage", event.raw_damage),
                |v| format!("{v} HP"),
            );
            events.push(ReplayEvent {
                key: format!("hurt-{}", event.source_index),
                tick: event.tick,
                kind: ReplayEventKind::Damage,
                label: format!(
                    "{} → {} · {amount}",
                    name(event.attacker_steamid.as_deref()),
                    name(Some(&event.victim_steamid))
                ),
                seekable: true,
                source_order: event.event_order,
                source_index: event.source_index,
            });
        }
        for event in round
            .deaths
            .iter()
            .flatten()
            .filter(|e| (start..=end).contains(&e.tick))
        {
            events.push(ReplayEvent {
                key: format!("death-{}", event.source_index),
                tick: event.tick,
                kind: ReplayEventKind::Death,
                label: format!(
                    "{} killed {}",
                    name(event.attacker_steamid.as_deref()),
                    name(Some(&event.victim_steamid))
                ),
                seekable: true,
                source_order: event.event_order,
                source_index: event.source_index,
            });
        }
    }
    if meta.bomb_events_available {
        for event in &meta.bomb_events {
            let seekable = (start..=end).contains(&event.tick);
            events.push(ReplayEvent {
                key: format!("bomb-{}", event.source_index),
                tick: event.tick,
                kind: ReplayEventKind::Bomb,
                label: format!(
                    "{} · {} · site unavailable{}",
                    event.kind.replace("bomb_", "Bomb "),
                    event.actor_name.as_deref().unwrap_or("actor unavailable"),
                    if seekable { "" } else { " · between rounds" }
                ),
                seekable,
                source_order: None,
                source_index: event.source_index,
            });
        }
    }
    events.sort_by(|a, b| {
        (a.tick, a.source_order.unwrap_or(i64::MAX), &a.key).cmp(&(
            b.tick,
            b.source_order.unwrap_or(i64::MAX),
            &b.key,
        ))
    });
    events
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackStep {
    pub previous_tick: i64,
    pub tick: i64,
    pub reached_end: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlaybackClock {
    start: i64,
    end: i64,
    rate: f64,
    tick: i64,
    speed: f64,
    playing: bool,
    fractional_ticks: f64,
}
impl PlaybackClock {
    pub fn new(start: i64, end: i64, rate: f64) -> Result<Self, ReplayError> {
        validate_bounds(start, end, rate)?;
        Ok(Self {
            start,
            end,
            rate,
            tick: start,
            speed: 1.0,
            playing: false,
            fractional_ticks: 0.0,
        })
    }
    pub fn current_tick(&self) -> i64 {
        self.tick
    }
    pub fn tick(&self) -> i64 {
        self.tick
    }
    pub fn is_playing(&self) -> bool {
        self.playing
    }
    pub fn speed(&self) -> f64 {
        self.speed
    }
    pub fn tick_rate(&self) -> f64 {
        self.rate
    }
    pub fn start_tick(&self) -> i64 {
        self.start
    }
    pub fn end_tick(&self) -> i64 {
        self.end
    }
    pub fn play(&mut self) {
        self.playing = self.tick < self.end;
    }
    pub fn pause(&mut self) {
        self.playing = false;
    }
    pub fn seek(&mut self, tick: i64) {
        self.tick = tick.clamp(self.start, self.end);
        self.playing = false;
        self.fractional_ticks = 0.0;
    }
    pub fn set_speed(&mut self, speed: f64) -> Result<(), ReplayError> {
        if !speed.is_finite() || !(0.25..=8.0).contains(&speed) {
            return Err(ReplayError::InvalidSpeed);
        }
        self.speed = speed;
        Ok(())
    }
    pub fn reset_round(&mut self, start: i64, end: i64, rate: f64) -> Result<(), ReplayError> {
        *self = Self::new(start, end, rate)?;
        Ok(())
    }
    pub fn advance(&mut self, elapsed: Duration) -> PlaybackStep {
        let previous_tick = self.tick;
        if !self.playing {
            return PlaybackStep {
                previous_tick,
                tick: self.tick,
                reached_end: false,
            };
        }
        let ticks = elapsed.as_secs_f64() * self.rate * self.speed + self.fractional_ticks;
        let remaining = self.end - self.tick;
        if ticks + 1e-10 >= remaining as f64 {
            self.tick = self.end;
            self.playing = false;
            self.fractional_ticks = 0.0;
            return PlaybackStep {
                previous_tick,
                tick: self.tick,
                reached_end: true,
            };
        }
        let whole = (ticks + 1e-10).floor() as i64;
        self.fractional_ticks = (ticks - whole as f64).max(0.0);
        self.tick += whole;
        PlaybackStep {
            previous_tick,
            tick: self.tick,
            reached_end: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::match_review::{ObservedBombSelection, ViewerBombEvent};

    fn meta() -> ViewerRoundMeta {
        ViewerRoundMeta {
            round_number: 3,
            start_tick: Some(100),
            end_tick: Some(300),
            tick_rate: Some(64.0),
            map_name: "de_mirage".into(),
            positions_available: true,
            player_sample_count: 2,
            grenade_sample_count: 1,
            utility_events: vec![],
            bomb_events: vec![bomb("bomb_planted", 200), bomb("bomb_defused", 250)],
            bomb_events_available: true,
            observed_bomb_selection: Some(ObservedBombSelection {
                plant_index: Some(0),
                explosion_index: None,
                defusal_index: Some(1),
            }),
        }
    }
    fn bomb(kind: &str, tick: i64) -> ViewerBombEvent {
        ViewerBombEvent {
            source_index: tick,
            kind: kind.into(),
            tick,
            actor_steamid: None,
            actor_name: None,
            site_id: None,
            x: Some(1.0),
            y: Some(2.0),
            z: Some(3.0),
        }
    }
    fn player(tick: i64) -> ViewerPlayerFrame {
        ViewerPlayerFrame {
            tick,
            steamid: "p1".into(),
            name: "Player One".into(),
            side: None,
            x: 1.0,
            y: 2.0,
            z: 3.0,
            is_alive: None,
            health: None,
            yaw: None,
        }
    }
    fn chunk() -> ViewerFrameChunk {
        ViewerFrameChunk {
            start_tick: 100,
            end_tick: 300,
            players: vec![player(100), player(200)],
            grenades: vec![ViewerGrenadeFrame {
                tick: 150,
                trajectory_index: 7,
                entity_id: 8,
                grenade_type: Some("Smoke".into()),
                thrower_steamid: None,
                x: 4.0,
                y: 5.0,
                z: 6.0,
            }],
        }
    }

    #[test]
    fn exact_sample_index_does_not_invent_sparse_or_unknown_fields() {
        let replay = ReplayRound::new(meta(), vec![chunk()], None).unwrap();
        assert_eq!(replay.players_at(100).len(), 1);
        assert_eq!(replay.players_at(101).len(), 0);
        assert_eq!(replay.scene(100).player_presence, SamplePresence::Observed);
        assert_eq!(
            replay.scene(101).player_presence,
            SamplePresence::MissingAtTick
        );
        assert_eq!(replay.players_at(100)[0].is_alive, None);
        assert_eq!(replay.players_at(100)[0].side, None);
        assert_eq!(replay.players_at(100)[0].health, None);
        assert!(replay.trajectory_until(7, 149).is_empty());
        assert_eq!(replay.trajectory_until(7, 150)[0].position, [4.0, 5.0, 6.0]);
    }

    #[test]
    fn observed_bomb_is_tick_bound_and_rewind_never_retains_future_state() {
        let replay = ReplayRound::new(meta(), vec![chunk()], None).unwrap();
        assert!(matches!(
            replay.scene(199).bomb,
            BombState::NoObservedPosition
        ));
        let BombState::Observed(bomb) = replay.scene(200).bomb else {
            panic!("plant absent at exact tick")
        };
        assert_eq!(bomb.phase, BombPhase::Planted);
        assert_eq!(bomb.position, [1.0, 2.0, 3.0]);
        let BombState::Observed(bomb) = replay.scene(250).bomb else {
            panic!("defusal absent")
        };
        assert_eq!(bomb.phase, BombPhase::Defused);
        assert!(matches!(
            replay.scene(100).bomb,
            BombState::NoObservedPosition
        ));
    }

    #[test]
    fn explosion_and_unavailable_or_ambiguous_selection_are_not_fake_carriers() {
        let mut m = meta();
        m.bomb_events[1] = bomb("bomb_exploded", 250);
        m.observed_bomb_selection = Some(ObservedBombSelection {
            plant_index: Some(0),
            explosion_index: Some(1),
            defusal_index: None,
        });
        let replay = ReplayRound::new(m, vec![chunk()], None).unwrap();
        assert!(matches!(
            replay.scene(250).bomb,
            BombState::Observed(ObservedBomb {
                phase: BombPhase::Exploded,
                ..
            })
        ));
        let mut m = meta();
        m.observed_bomb_selection = None;
        assert!(matches!(
            ReplayRound::new(m, vec![chunk()], None)
                .unwrap()
                .scene(250)
                .bomb,
            BombState::NoObservedPosition
        ));
        let mut m = meta();
        m.bomb_events_available = false;
        m.observed_bomb_selection = None;
        m.positions_available = false;
        let replay = ReplayRound::new(m, vec![], None).unwrap();
        assert!(matches!(replay.scene(200).bomb, BombState::Unavailable));
        assert_eq!(
            replay.scene(100).player_presence,
            SamplePresence::Unavailable
        );
    }

    #[test]
    fn inter_round_event_is_visible_but_cannot_seek_or_draw_an_invented_frame() {
        let mut m = meta();
        m.bomb_events = vec![bomb("bomb_planted", 320)];
        m.observed_bomb_selection = Some(ObservedBombSelection {
            plant_index: Some(0),
            ..Default::default()
        });
        let replay = ReplayRound::new(m, vec![chunk()], None).unwrap();
        assert_eq!(replay.events.len(), 1);
        assert_eq!(replay.events[0].tick, 320);
        assert!(!replay.events[0].seekable);
        assert!(replay.events[0].label.contains("between rounds"));
        assert!(matches!(
            replay.scene(300).bomb,
            BombState::NoObservedPosition
        ));
    }

    #[test]
    fn invalid_rate_bounds_samples_and_duplicate_identity_are_rejected() {
        for rate in [None, Some(f64::NAN), Some(0.0), Some(-64.0), Some(2048.0)] {
            let mut m = meta();
            m.tick_rate = rate;
            assert!(ReplayRound::new(m, vec![chunk()], None).is_err());
        }
        let mut m = meta();
        m.end_tick = Some(100);
        assert!(ReplayRound::new(m, vec![], None).is_err());
        let mut c = chunk();
        c.players.push(player(100));
        assert!(ReplayRound::new(meta(), vec![c], None).is_err());
        let mut c = chunk();
        c.players[0].x = f64::NAN;
        assert!(ReplayRound::new(meta(), vec![c], None).is_err());
        let mut c = chunk();
        c.players[0].tick = 99;
        assert!(ReplayRound::new(meta(), vec![c], None).is_err());
    }

    #[test]
    fn clock_keeps_fractional_ticks_and_selected_speed_without_wall_clock_drift() {
        let mut clock = PlaybackClock::new(100, 10_000, 64.0).unwrap();
        clock.play();
        for _ in 0..1000 {
            clock.advance(Duration::from_millis(1));
        }
        assert_eq!(clock.current_tick(), 164);
        clock.set_speed(1.5).unwrap();
        for _ in 0..1000 {
            clock.advance(Duration::from_millis(1));
        }
        assert_eq!(clock.current_tick(), 260);
        assert_eq!(clock.speed(), 1.5);
        clock.pause();
        clock.advance(Duration::from_secs(100));
        assert_eq!(clock.current_tick(), 260);
    }

    #[test]
    fn seeking_pauses_clamps_and_clears_remainder_while_round_reset_clears_stale_state() {
        let mut clock = PlaybackClock::new(100, 300, 64.0).unwrap();
        clock.play();
        clock.advance(Duration::from_millis(10));
        clock.seek(200);
        assert_eq!(clock.current_tick(), 200);
        assert!(!clock.is_playing());
        clock.play();
        clock.advance(Duration::from_millis(10));
        assert_eq!(clock.current_tick(), 200);
        clock.seek(-999);
        assert_eq!(clock.current_tick(), 100);
        clock.seek(999);
        assert_eq!(clock.current_tick(), 300);
        clock.play();
        assert!(!clock.is_playing());
        clock.reset_round(500, 600, 128.0).unwrap();
        assert_eq!(clock.current_tick(), 500);
        assert_eq!(clock.speed(), 1.0);
        assert!(!clock.is_playing());
        clock.play();
        let step = clock.advance(Duration::MAX);
        assert!(step.reached_end);
        assert_eq!(clock.current_tick(), 600);
        assert!(!clock.is_playing());
    }

    #[test]
    fn invalid_speed_is_rejected_without_changing_the_current_clock() {
        let mut clock = PlaybackClock::new(0, 100, 64.0).unwrap();
        for speed in [0.0, -1.0, f64::INFINITY, f64::NAN, 32.0] {
            assert!(clock.set_speed(speed).is_err());
            assert_eq!(clock.speed(), 1.0);
        }
    }
    fn recorded_round() -> RoundSummary {
        RoundSummary {
            round_number: 3,
            winner: None,
            start_tick: Some(100),
            freeze_end_tick: Some(100),
            end_tick: Some(300),
            end_reason: None,
            team_a_side: None,
            team_b_side: None,
            ct_equip_value: None,
            t_equip_value: None,
            combat: None,
            deaths: None,
            hurts: None,
        }
    }

    #[test]
    fn stale_other_round_combat_cannot_be_painted_into_the_new_round() {
        let mut round = recorded_round();
        round.round_number = 4;
        assert!(ReplayRound::new(meta(), vec![chunk()], Some(&round)).is_err());
    }

    #[test]
    fn same_tick_order_preserves_observed_order_and_marks_unknown_ties_unknown() {
        use crate::match_review::{RoundDeath, RoundHurt, ViewerUtilityEvent};
        let mut round = recorded_round();
        round.deaths = Some(vec![RoundDeath {
            source_index: 8,
            event_order: Some(10),
            tick: 200,
            weapon: None,
            attacker_steamid: Some("p1".into()),
            attacker_side: None,
            victim_steamid: "p2".into(),
            victim_side: None,
            assister_steamid: None,
        }]);
        round.hurts = Some(vec![RoundHurt {
            source_index: 2,
            event_order: Some(11),
            tick: 200,
            weapon: None,
            attacker_steamid: Some("p1".into()),
            attacker_side: None,
            victim_steamid: "p2".into(),
            victim_side: None,
            raw_damage: 140,
            effective_damage: None,
        }]);
        let mut m = meta();
        m.utility_events.push(ViewerUtilityEvent {
            id: 5,
            utility_type: "smoke".into(),
            throw_tick: 150,
            detonate_tick: Some(200),
            detonate_x: None,
            detonate_y: None,
            detonate_z: None,
            projectile_entity_id: None,
            observed_trajectory_index: None,
            thrower_steamid: "p1".into(),
            thrower_name: "Player One".into(),
            thrower_side: String::new(),
        });
        let replay = ReplayRound::new(m, vec![chunk()], Some(&round)).unwrap();
        let at: Vec<_> = replay.events.iter().filter(|e| e.tick == 200).collect();
        assert_eq!(at[0].key, "death-8");
        assert_eq!(at[1].key, "hurt-2");
        assert_eq!(at[0].source_order, Some(10));
        assert_eq!(at[1].source_order, Some(11));
        assert!(at[2..].iter().all(|e| e.source_order.is_none()));
        assert!(at[1].label.contains("140 raw damage"));
        assert!(at[0].label.contains("Player One"));
    }

    #[test]
    fn out_of_round_scene_and_large_absolute_ticks_do_not_reuse_stale_samples() {
        let replay = ReplayRound::new(meta(), vec![chunk()], None).unwrap();
        assert_eq!(
            replay.scene(99).player_presence,
            SamplePresence::OutsideRound
        );
        assert!(replay.scene(99).players.is_empty());
        assert!(replay.scene(i64::MAX).grenades.is_empty());
        let mut clock = PlaybackClock::new(1_000_000_000, 1_000_000_064, 64.0).unwrap();
        clock.play();
        clock.advance(Duration::from_secs(1));
        assert_eq!(clock.current_tick(), 1_000_000_064);
        assert!(!clock.is_playing());
    }
}
