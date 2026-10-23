//! Native Rust match review. No WebView, JavaScript bridge or React renderer.
use crate::match_review::{self, LibrarySnapshot, MatchDetail};
use crate::native_replay::{BombPhase, BombState, PlaybackClock, ReplayRound};
use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Overview,
    Rounds,
    Players,
    Utility,
    Viewer,
}

pub(super) enum ReviewAction {
    ImportDemo,
    ChooseLibrary,
    ChooseRadar,
    CaptureTools,
    PrepareCapture {
        event_id: i64,
        database: PathBuf,
        match_id: i64,
        source_sha256: Option<String>,
    },
}

struct LoadedReview {
    library: LibrarySnapshot,
    detail: Option<MatchDetail>,
    replay: Option<ReplayRound>,
    round_number: i64,
    replay_error: Option<String>,
}

struct ReviewRequest {
    database: PathBuf,
    match_id: Option<i64>,
    round_number: Option<i64>,
    cancel: Arc<AtomicBool>,
    reply: mpsc::Sender<Result<LoadedReview, String>>,
}

struct PendingReview {
    generation: u64,
    receiver: mpsc::Receiver<Result<LoadedReview, String>>,
    cancel: Arc<AtomicBool>,
}

pub(super) struct NativeReviewUi {
    pub active: bool,
    database: PathBuf,
    loaded: Option<LoadedReview>,
    pending: Option<PendingReview>,
    worker: Option<mpsc::Sender<ReviewRequest>>,
    generation: u64,
    tab: Tab,
    clock: Option<PlaybackClock>,
    last_frame: Instant,
    error: Option<String>,
    search: String,
    radar: Option<(String, egui::TextureHandle)>,
    radar_generation: u64,
    focused_player: Option<String>,
    selected_event: Option<String>,
    zoom: f32,
    painted_viewer: Option<crate::native_review_capture::NativeReviewPaint>,
}

impl Default for NativeReviewUi {
    fn default() -> Self {
        Self {
            active: false,
            database: PathBuf::new(),
            loaded: None,
            pending: None,
            worker: None,
            generation: 0,
            tab: Tab::Overview,
            clock: None,
            last_frame: Instant::now(),
            error: None,
            search: String::new(),
            radar: None,
            radar_generation: 0,
            focused_player: None,
            selected_event: None,
            zoom: 1.0,
            painted_viewer: None,
        }
    }
}

impl Drop for NativeReviewUi {
    fn drop(&mut self) {
        if let Some(pending) = &self.pending {
            pending.cancel.store(true, Ordering::Relaxed);
        }
    }
}

fn load_review(
    database: &Path,
    match_id: Option<i64>,
    round_number: Option<i64>,
    cancel: &AtomicBool,
) -> Result<LoadedReview, String> {
    let path = database
        .to_str()
        .ok_or("Library path is not valid Unicode")?;
    let library = match_review::list_matches(path).map_err(|e| e.to_string())?;
    if cancel.load(Ordering::Relaxed) {
        return Err("Review selection cancelled".into());
    }
    let Some(id) = match_id.or_else(|| library.matches.first().map(|m| m.id)) else {
        return Ok(LoadedReview {
            library,
            detail: None,
            replay: None,
            round_number: 0,
            replay_error: None,
        });
    };
    let detail = match_review::get_match_detail(path, id).map_err(|e| e.to_string())?;
    let number = round_number
        .or_else(|| detail.rounds.first().map(|r| r.round_number))
        .unwrap_or(0);
    let mut replay = None;
    let mut replay_error = None;
    if let Some(round) = detail.rounds.iter().find(|r| r.round_number == number) {
        let meta = match_review::get_viewer_round(path, id, number).map_err(|e| e.to_string())?;
        if let (true, Some(start), Some(end)) =
            (meta.positions_available, meta.start_tick, meta.end_tick)
        {
            const MAX_REVIEW_SAMPLES: usize = 2_000_000;
            const MAX_REVIEW_TICKS: i64 = 2_000_000;
            if end <= start
                || start < 0
                || end.saturating_sub(start) > MAX_REVIEW_TICKS
                || meta.player_sample_count < 0
                || meta.grenade_sample_count < 0
                || meta
                    .player_sample_count
                    .saturating_add(meta.grenade_sample_count)
                    > MAX_REVIEW_SAMPLES as i64
            {
                return Err("Recorded round exceeds the supported bounded native replay window; match facts remain unchanged".into());
            }
            let mut loaded_samples = 0usize;
            let mut chunks = Vec::new();
            let mut tick = start;
            while tick <= end {
                if cancel.load(Ordering::Relaxed) {
                    return Err("Review selection cancelled".into());
                }
                let last = end.min(tick.saturating_add(511));
                let chunk = match_review::get_viewer_frames(path, id, number, tick, last)
                    .map_err(|e| e.to_string())?;
                loaded_samples = loaded_samples
                    .checked_add(chunk.players.len())
                    .and_then(|n| n.checked_add(chunk.grenades.len()))
                    .ok_or("Native replay sample count overflow")?;
                if loaded_samples > MAX_REVIEW_SAMPLES {
                    return Err("Native replay sample budget exceeded while loading; no source data changed".into());
                }
                chunks.push(chunk);
                if last == end {
                    break;
                }
                tick = last + 1;
            }
            match ReplayRound::new(meta, chunks, Some(round)) {
                Ok(value) => replay = Some(value),
                Err(error) => replay_error = Some(format!("{error:?}")),
            }
        } else {
            replay_error = Some("This import has no verified playable position stream for this round. Recorded match facts remain available.".into());
        }
    } else if number != 0 {
        return Err("Selected round no longer exists in this match".into());
    }
    Ok(LoadedReview {
        library,
        detail: Some(detail),
        replay,
        round_number: number,
        replay_error,
    })
}

impl NativeReviewUi {
    pub fn open(&mut self, database: PathBuf, match_id: Option<i64>, round_number: Option<i64>) {
        self.active = true;
        if let Some(pending) = self.pending.take() {
            pending.cancel.store(true, Ordering::Relaxed);
        }
        if let Some(clock) = self.clock.as_mut() {
            clock.pause();
        }
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let (sender, receiver) = mpsc::channel();
        let worker = if let Some(worker) = &self.worker {
            worker.clone()
        } else {
            let (requests, receiver) = mpsc::channel::<ReviewRequest>();
            match std::thread::Builder::new()
                .name("native-match-review".into())
                .spawn(move || {
                    while let Ok(mut request) = receiver.recv() {
                        // Coalesce repeated navigation rather than spawning one thread per click.
                        while let Ok(newer) = receiver.try_recv() {
                            request.cancel.store(true, Ordering::Relaxed);
                            request = newer;
                        }
                        if request.cancel.load(Ordering::Relaxed) {
                            continue;
                        }
                        let result = load_review(
                            &request.database,
                            request.match_id,
                            request.round_number,
                            &request.cancel,
                        );
                        let _ = request.reply.send(result);
                    }
                }) {
                Ok(_) => {
                    self.worker = Some(requests.clone());
                    requests
                }
                Err(error) => {
                    self.error = Some(format!("Cannot start native review worker: {error}"));
                    return;
                }
            }
        };
        if worker
            .send(ReviewRequest {
                database,
                match_id,
                round_number,
                cancel: worker_cancel,
                reply: sender,
            })
            .is_err()
        {
            self.worker = None;
            self.error = Some("Native review worker stopped; choose the match again".into());
            return;
        }
        self.pending = Some(PendingReview {
            generation,
            receiver,
            cancel,
        });
        self.error = None;
    }

    /// CLI/proof preparation only; normal interactive navigation uses a cancellable worker.
    pub fn open_ready(
        &mut self,
        database: PathBuf,
        match_id: Option<i64>,
        round_number: Option<i64>,
    ) -> Result<(), String> {
        let loaded = load_review(&database, match_id, round_number, &AtomicBool::new(false))?;
        if let Some(pending) = self.pending.take() {
            pending.cancel.store(true, Ordering::Relaxed);
        }
        self.generation = self.generation.wrapping_add(1);
        self.database = database;
        self.active = true;
        self.tab = if round_number.is_some() {
            Tab::Viewer
        } else {
            Tab::Overview
        };
        self.install(loaded);
        Ok(())
    }

    fn install(&mut self, loaded: LoadedReview) {
        self.painted_viewer = None;
        self.database = PathBuf::from(&loaded.library.path);
        self.clock = loaded.replay.as_ref().and_then(|replay| {
            let mut clock = PlaybackClock::new(
                replay.meta.start_tick?,
                replay.meta.end_tick?,
                replay.meta.tick_rate?,
            )
            .ok()?;
            if let Some(tick) = loaded
                .detail
                .as_ref()?
                .rounds
                .iter()
                .find(|r| r.round_number == loaded.round_number)?
                .freeze_end_tick
            {
                clock.seek(tick);
            }
            Some(clock)
        });
        self.loaded = Some(loaded);
        self.focused_player = None;
        self.selected_event = None;
        self.last_frame = Instant::now();
    }

    fn poll(&mut self) {
        let result = self
            .pending
            .as_ref()
            .and_then(|pending| match pending.receiver.try_recv() {
                Ok(value) => Some((pending.generation, value)),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some((
                    pending.generation,
                    Err("Native review worker stopped before reporting a result".into()),
                )),
            });
        if let Some((generation, result)) = result {
            self.pending = None;
            if generation == self.generation {
                match result {
                    Ok(loaded) => self.install(loaded),
                    Err(error) => self.error = Some(error),
                }
            }
        }
    }

    pub fn settled_paint(&self) -> Result<crate::native_review_capture::NativeReviewPaint, String> {
        if !self.active || self.tab != Tab::Viewer {
            return Err("native Rust 2D view is not active".into());
        }
        self.painted_viewer
            .clone()
            .filter(|paint| paint.viewer_paint_frames >= 2)
            .ok_or_else(|| "native Rust 2D view has not painted two settled frames".into())
    }

    pub fn navigation_generation(&self) -> u64 {
        self.generation
    }

    pub fn library_path(&self) -> Option<&Path> {
        self.loaded
            .as_ref()
            .map(|loaded| Path::new(&loaded.library.path))
    }
    pub fn suspend(&mut self) {
        if let Some(clock) = self.clock.as_mut() {
            clock.pause();
        }
        self.last_frame = Instant::now();
    }

    pub fn load_radar(&mut self, ctx: &egui::Context, path: &Path) -> Result<(), String> {
        let map = self
            .loaded
            .as_ref()
            .and_then(|l| l.detail.as_ref())
            .ok_or("Choose a match first")?
            .summary
            .map_name
            .clone();
        if calibration(&map).is_none() {
            return Err("This map has no verified coordinate calibration".into());
        }
        use std::io::Read;
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        file.take(8 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err("Choose a radar PNG smaller than 8 MiB".into());
        }
        let dimensions =
            image::ImageReader::with_format(std::io::Cursor::new(&bytes), image::ImageFormat::Png)
                .into_dimensions()
                .map_err(|e| e.to_string())?;
        if dimensions != (1024, 1024) {
            return Err("Calibrated radar must be 1024 × 1024".into());
        }
        let mut reader =
            image::ImageReader::with_format(std::io::Cursor::new(&bytes), image::ImageFormat::Png);
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(16 * 1024 * 1024);
        reader.limits(limits);
        let image = reader.decode().map_err(|e| e.to_string())?;
        let rgba = image.into_rgba8();
        let texture = ctx.load_texture(
            format!("native-radar-{map}"),
            egui::ColorImage::from_rgba_unmultiplied([1024, 1024], &rgba),
            egui::TextureOptions::LINEAR,
        );
        self.radar_generation = self.radar_generation.wrapping_add(1);
        self.painted_viewer = None;
        self.radar = Some((map, texture));
        Ok(())
    }

    pub fn show(&mut self, ui: &mut egui::Ui) -> Vec<ReviewAction> {
        self.poll();
        let mut actions = Vec::new();
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.last_frame);
        self.last_frame = now;
        if self.pending.is_none() && self.tab == Tab::Viewer {
            if let Some(clock) = self.clock.as_mut() {
                clock.advance(elapsed);
                if clock.is_playing() {
                    ui.ctx().request_repaint();
                }
            }
        }
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("MATCH REVIEW")
                    .strong()
                    .color(egui::Color32::from_rgb(238, 159, 86)),
            );
            ui.separator();
            ui.small("Native Rust · recorded demo facts");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Capture & library tools").clicked() {
                    actions.push(ReviewAction::CaptureTools);
                }
                if ui.button("Choose library…").clicked() {
                    actions.push(ReviewAction::ChooseLibrary);
                }
                if ui.button("Import CS2 demo…").clicked() {
                    actions.push(ReviewAction::ImportDemo);
                }
            });
        });
        ui.separator();
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        if self.pending.is_some() {
            ui.horizontal(|ui| { ui.spinner(); ui.label("Loading recorded match and indexed replay… Previous selection remains unchanged until ready."); });
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(25));
        }
        let mut change_match = None;
        let mut change_round = None;
        let database = self.database.clone();
        let Some(loaded) = self.loaded.as_ref() else {
            ui.add_space(40.0);
            ui.heading("Your match review workspace");
            ui.label("Import a supported CS2 demo or choose an existing DemoStrike library. Match, round and replay views run directly in Rust.");
            ui.small("No demo or playback state has been fabricated.");
            return actions;
        };
        let selected_id = loaded.detail.as_ref().map(|d| d.summary.id);
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("native-review-match")
                .selected_text(
                    loaded
                        .detail
                        .as_ref()
                        .map(|d| format!("{} · {}", d.summary.map_name, d.summary.file_name))
                        .unwrap_or_else(|| "Saved matches".into()),
                )
                .width(360.0)
                .show_ui(ui, |ui| {
                    for item in &loaded.library.matches {
                        if ui
                            .selectable_label(
                                Some(item.id) == selected_id,
                                format!("{} · {}", item.map_name, item.file_name),
                            )
                            .clicked()
                        {
                            change_match = Some(item.id);
                        }
                    }
                });
            if let Some(detail) = &loaded.detail {
                ui.small(format!(
                    "{} rounds · {} players · {} throws",
                    detail.rounds.len(),
                    detail.players.len(),
                    detail.utility_events.len()
                ));
            }
        });
        let Some(detail) = &loaded.detail else {
            ui.add_space(30.0);
            ui.heading("No saved matches yet");
            ui.label("Import a demo to populate this native review workspace.");
            return actions;
        };
        ui.horizontal(|ui| {
            for (tab, label) in [
                (Tab::Overview, "Overview"),
                (Tab::Rounds, "Rounds"),
                (Tab::Players, "Players"),
                (Tab::Utility, "Utility"),
                (Tab::Viewer, "2D Viewer"),
            ] {
                if ui.selectable_label(self.tab == tab, label).clicked() {
                    if self.tab == Tab::Viewer && tab != Tab::Viewer {
                        if let Some(clock) = self.clock.as_mut() {
                            clock.pause();
                        }
                    }
                    self.tab = tab;
                }
            }
        });
        ui.separator();
        match self.tab {
            Tab::Overview => overview(ui, detail),
            Tab::Rounds => {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for round in &detail.rounds {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.strong(format!("Round {}", round.round_number));
                                ui.label(format!(
                                    "Winner: {}",
                                    round.winner.as_deref().unwrap_or("unavailable")
                                ));
                                ui.small(format!(
                                    "{} → {}",
                                    optional_tick(round.start_tick),
                                    optional_tick(round.end_tick)
                                ));
                                if ui.button("Review in 2D").clicked() {
                                    change_round = Some(round.round_number);
                                    self.tab = Tab::Viewer;
                                }
                            });
                        });
                    }
                });
            }
            Tab::Players => {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    egui::Grid::new("native-player-scoreboard")
                        .striped(true)
                        .min_col_width(80.0)
                        .show(ui, |ui| {
                            for label in ["Player", "Kills", "Deaths", "Assists", "Effective HP"] {
                                ui.strong(label);
                            }
                            ui.end_row();
                            for player in &detail.players {
                                ui.label(&player.name);
                                if let Some(combat) = &player.combat {
                                    ui.label(optional_number(combat.enemy_kills));
                                    ui.label(combat.deaths.to_string());
                                    ui.label(optional_number(combat.enemy_assists));
                                    ui.label(optional_number(combat.effective_enemy_health_damage));
                                } else {
                                    for _ in 0..4 {
                                        ui.label("Unavailable");
                                    }
                                }
                                ui.end_row();
                            }
                        });
                });
            }
            Tab::Utility => {
                ui.horizontal(|ui| {
                    ui.label("Find player or utility");
                    ui.text_edit_singleline(&mut self.search);
                });
                let search = self.search.to_lowercase();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for event in detail.utility_events.iter().filter(|e| {
                        format!("{} {}", e.thrower_name, e.utility_type)
                            .to_lowercase()
                            .contains(&search)
                    }) {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.strong(format!(
                                    "{} · {}",
                                    event.utility_type, event.thrower_name
                                ));
                                ui.label(format!(
                                    "Round {} · tick {}",
                                    event.round_number, event.throw_tick
                                ));
                                if ui.button("Review round").clicked() {
                                    change_round = Some(event.round_number);
                                    self.tab = Tab::Viewer;
                                }
                                if ui.button("Prepare capture request").clicked() {
                                    actions.push(ReviewAction::PrepareCapture {
                                        event_id: event.id,
                                        database: PathBuf::from(&loaded.library.path),
                                        match_id: detail.summary.id,
                                        source_sha256: detail.summary.source_sha256.clone(),
                                    });
                                }
                            });
                            ui.small(format!(
                                "Enemy damage: {} · enemies blinded: {}",
                                optional_number(event.damage_enemy),
                                optional_number(event.enemies_blinded)
                            ));
                        });
                    }
                });
            }
            Tab::Viewer => {
                ui.horizontal_wrapped(|ui| {
                    for round in &detail.rounds {
                        if ui
                            .selectable_label(
                                round.round_number == loaded.round_number,
                                round.round_number.to_string(),
                            )
                            .clicked()
                        {
                            change_round = Some(round.round_number);
                        }
                    }
                });
                if let (Some(replay), Some(clock)) = (&loaded.replay, &mut self.clock) {
                    ui.horizontal(|ui| {
                        if ui
                            .button(if clock.is_playing() { "Pause" } else { "Play" })
                            .clicked()
                        {
                            if clock.is_playing() {
                                clock.pause()
                            } else {
                                clock.play()
                            }
                        }
                        if ui.button("Start").clicked() {
                            clock.seek(replay.meta.start_tick.unwrap());
                        }
                        for speed in [0.5, 1.0, 1.5, 2.0] {
                            if ui
                                .selectable_label(
                                    (clock.speed() - speed).abs() < 0.001,
                                    format!("{speed}×"),
                                )
                                .clicked()
                            {
                                let _ = clock.set_speed(speed);
                            }
                        }
                        ui.label(format!(
                            "Tick {} · {}",
                            clock.current_tick(),
                            replay.meta.map_name
                        ));
                        if ui.button("Choose radar PNG…").clicked() {
                            actions.push(ReviewAction::ChooseRadar);
                        }
                    });
                    let mut tick = clock.current_tick();
                    if ui
                        .add(
                            egui::Slider::new(
                                &mut tick,
                                replay.meta.start_tick.unwrap()..=replay.meta.end_tick.unwrap(),
                            )
                            .text("Source tick"),
                        )
                        .changed()
                    {
                        clock.seek(tick);
                    }
                    let frame_tick = clock.current_tick();
                    let available = ui.available_size();
                    let roster_width = 170.0;
                    let events_width = 245.0;
                    let map_width = (available.x - roster_width - events_width - 24.0).max(180.0);
                    let height = available.y.max(220.0);
                    ui.horizontal_top(|ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(roster_width, height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                egui::ScrollArea::vertical()
                                    .id_salt("native-replay-roster")
                                    .max_height(height)
                                    .show(ui, |ui| {
                                        roster(ui, replay, frame_tick, &mut self.focused_player);
                                    });
                            },
                        );
                        ui.allocate_ui_with_layout(
                            egui::vec2(map_width, height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                let radar = self
                                    .radar
                                    .as_ref()
                                    .filter(|(map, _)| map == &replay.meta.map_name)
                                    .map(|(_, texture)| texture);
                                draw_radar(
                                    ui,
                                    replay,
                                    frame_tick,
                                    radar,
                                    &self.focused_player,
                                    &mut self.zoom,
                                );
                                let frames = self
                                    .painted_viewer
                                    .as_ref()
                                    .filter(|paint| {
                                        paint.database == Path::new(&loaded.library.path)
                                            && paint.match_id == detail.summary.id
                                            && paint.round == loaded.round_number
                                            && paint.tick == frame_tick
                                            && paint.radar_generation == self.radar_generation
                                            && paint.focused_player == self.focused_player
                                            && paint.zoom == self.zoom
                                    })
                                    .map_or(1, |paint| paint.viewer_paint_frames.saturating_add(1));
                                self.painted_viewer =
                                    Some(crate::native_review_capture::NativeReviewPaint {
                                        database: PathBuf::from(&loaded.library.path),
                                        match_id: detail.summary.id,
                                        source_sha256: detail
                                            .summary
                                            .source_sha256
                                            .clone()
                                            .unwrap_or_default(),
                                        map: replay.meta.map_name.clone(),
                                        round: loaded.round_number,
                                        tick: frame_tick,
                                        viewer_paint_frames: frames,
                                        observed_players: replay.players_at(frame_tick).len(),
                                        observed_grenades: replay.grenades_at(frame_tick).len(),
                                        recorded_events: replay.events.len(),
                                        radar_loaded: radar.is_some(),
                                        zoom: self.zoom,
                                        focused_player: self.focused_player.clone(),
                                        radar_generation: self.radar_generation,
                                    });
                            },
                        );
                        ui.allocate_ui_with_layout(
                            egui::vec2(events_width, height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.strong(format!("Recorded events ({})", replay.events.len()));
                                ui.separator();
                                egui::ScrollArea::vertical()
                                    .id_salt("native-replay-events")
                                    .max_height(height - 60.0)
                                    .show(ui, |ui| {
                                        for event in &replay.events {
                                            let label = format!(
                                                "{}  {}{}",
                                                event.tick,
                                                event.label,
                                                if event.seekable {
                                                    ""
                                                } else {
                                                    " · between rounds"
                                                }
                                            );
                                            if ui
                                                .add_enabled(
                                                    event.seekable,
                                                    egui::Button::new(label).wrap().selected(
                                                        self.selected_event.as_ref()
                                                            == Some(&event.key),
                                                    ),
                                                )
                                                .clicked()
                                            {
                                                clock.seek(event.tick);
                                                self.selected_event = Some(event.key.clone());
                                            }
                                        }
                                    });
                            },
                        );
                    });
                } else {
                    ui.colored_label(
                        egui::Color32::YELLOW,
                        loaded
                            .replay_error
                            .as_deref()
                            .unwrap_or("Verified replay data is unavailable for this round."),
                    );
                }
            }
        }
        if let Some(id) = change_match {
            self.open(database, Some(id), None);
        } else if let Some(number) = change_round {
            self.open(database, selected_id, Some(number));
        }
        actions
    }
}

fn optional_tick(value: Option<i64>) -> String {
    value.map_or_else(|| "Unavailable".into(), |v| v.to_string())
}
fn optional_number(value: Option<i64>) -> String {
    optional_tick(value)
}
fn overview(ui: &mut egui::Ui, detail: &MatchDetail) {
    ui.add_space(12.0);
    ui.heading(&detail.summary.map_name);
    ui.label(&detail.summary.file_name);
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        ui.group(|ui| {
            ui.label("Counter-Terrorists");
            ui.heading(detail.summary.score_ct.to_string());
        });
        ui.group(|ui| {
            ui.label("Terrorists");
            ui.heading(detail.summary.score_t.to_string());
        });
        ui.group(|ui| {
            ui.label("Recorded rounds");
            ui.heading(detail.rounds.len().to_string());
        });
        ui.group(|ui| {
            ui.label("Utility throws");
            ui.heading(detail.utility_events.len().to_string());
        });
    });
    ui.add_space(16.0);
    ui.label(format!(
        "Native parser: {}",
        detail.summary.parser_capability
    ));
    ui.label(format!(
        "Tick rate: {}",
        detail
            .summary
            .tick_rate
            .map_or_else(|| "Unavailable".into(), |v| format!("{v} Hz"))
    ));
    if let Some(hash) = &detail.summary.source_sha256 {
        ui.small(format!("Source SHA-256: {hash}"));
    }
    ui.add_space(12.0);
    ui.label("Review recorded players, rounds, combat and utility, then seek the same source tick in the native 2D viewer.");
    ui.colored_label(egui::Color32::from_rgb(203,173,113),"Equipment, bomb carrier, smoke/fire effect areas, game audio and unrecorded camera facts remain unavailable; no values are invented.");
}

fn calibration(map: &str) -> Option<(f64, f64, f64)> {
    match map {
        "de_mirage" => Some((-3230.0, 1713.0, 5.0)),
        "de_ancient" => Some((-2953.0, 2164.0, 5.0)),
        _ => None,
    }
}
fn map_position(rect: egui::Rect, map: &str, x: f64, y: f64) -> Option<egui::Pos2> {
    let (px, py, scale) = calibration(map)?;
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    let screen = egui::pos2(
        rect.left() + ((x - px) / scale / 1024.0) as f32 * rect.width(),
        rect.top() + ((py - y) / scale / 1024.0) as f32 * rect.height(),
    );
    screen.is_finite().then_some(screen)
}
fn side_color(side: Option<&str>) -> egui::Color32 {
    match side {
        Some("CT") => egui::Color32::from_rgb(88, 167, 240),
        Some("T" | "TERRORIST") => egui::Color32::from_rgb(238, 164, 80),
        _ => egui::Color32::GRAY,
    }
}
fn roster(ui: &mut egui::Ui, replay: &ReplayRound, tick: i64, focus: &mut Option<String>) {
    for side in ["CT", "T", "unknown"] {
        ui.strong(match side {
            "CT" => "Counter-Terrorists",
            "T" => "Terrorists",
            _ => "Side unavailable",
        });
        ui.separator();
        for player in replay.players_at(tick).iter().filter(|p| match side {
            "CT" => p.side.as_deref() == Some("CT"),
            "T" => matches!(p.side.as_deref(), Some("T" | "TERRORIST")),
            _ => !matches!(p.side.as_deref(), Some("CT" | "T" | "TERRORIST")),
        }) {
            let health = match (player.is_alive, player.health) {
                (Some(false), _) => "Out".into(),
                (_, Some(hp)) => format!("{hp} HP"),
                _ => "Health unavailable".into(),
            };
            if ui
                .selectable_label(
                    focus.as_deref() == Some(player.steamid.as_str()),
                    egui::RichText::new(&player.name).color(side_color(player.side.as_deref())),
                )
                .clicked()
            {
                if focus.as_deref() == Some(&player.steamid) {
                    *focus = None
                } else {
                    *focus = Some(player.steamid.clone());
                }
            }
            ui.small(health);
        }
        ui.add_space(10.0);
    }
}
fn draw_radar(
    ui: &mut egui::Ui,
    replay: &ReplayRound,
    tick: i64,
    radar: Option<&egui::TextureHandle>,
    focus: &Option<String>,
    zoom: &mut f32,
) {
    ui.horizontal(|ui| {
        ui.small("Radar zoom");
        ui.add(egui::Slider::new(zoom, 1.0..=3.0).show_value(false));
    });
    let side = ui
        .available_width()
        .min((ui.available_height() - 65.0).max(180.0));
    let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6.0, egui::Color32::from_rgb(24, 31, 38));
    let map_rect = egui::Rect::from_center_size(rect.center(), rect.size() * *zoom);
    if let Some(texture) = radar {
        painter.image(
            texture.id(),
            map_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    } else {
        for step in 0..=16 {
            let f = step as f32 / 16.0;
            painter.line_segment(
                [
                    egui::pos2(rect.left() + rect.width() * f, rect.top()),
                    egui::pos2(rect.left() + rect.width() * f, rect.bottom()),
                ],
                egui::Stroke::new(0.5, egui::Color32::from_rgb(45, 56, 67)),
            );
            painter.line_segment(
                [
                    egui::pos2(rect.left(), rect.top() + rect.height() * f),
                    egui::pos2(rect.right(), rect.top() + rect.height() * f),
                ],
                egui::Stroke::new(0.5, egui::Color32::from_rgb(45, 56, 67)),
            );
        }
    }
    if calibration(&replay.meta.map_name).is_none() {
        ui.label("Map calibration unavailable; recorded lists remain usable.");
        return;
    }
    for grenade in replay.grenades_at(tick) {
        if let Some(position) = map_position(map_rect, &replay.meta.map_name, grenade.x, grenade.y)
        {
            let points: Vec<_> = replay
                .trajectory_until(grenade.trajectory_index, tick)
                .iter()
                .filter_map(|p| {
                    map_position(
                        map_rect,
                        &replay.meta.map_name,
                        p.position[0],
                        p.position[1],
                    )
                })
                .collect();
            if points.len() > 1 {
                painter.add(egui::Shape::line(
                    points,
                    egui::Stroke::new(1.5, egui::Color32::from_rgb(180, 213, 174)),
                ));
            }
            painter.circle_filled(position, 4.0, egui::Color32::LIGHT_GREEN);
        }
    }
    if let BombState::Observed(bomb) = replay.bomb_at(tick) {
        if let Some(position) = map_position(
            map_rect,
            &replay.meta.map_name,
            bomb.position[0],
            bomb.position[1],
        ) {
            let (color, label, radius) = match bomb.phase {
                BombPhase::Planted => (egui::Color32::from_rgb(248, 190, 72), "B", 9.0),
                BombPhase::Defused => (egui::Color32::from_gray(125), "B", 9.0),
                BombPhase::Exploded => (egui::Color32::LIGHT_RED, "×", 17.0),
            };
            painter.circle_filled(position, radius, egui::Color32::from_black_alpha(220));
            painter.circle_stroke(position, radius, egui::Stroke::new(2.0, color));
            painter.text(
                position,
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(12.0),
                color,
            );
        }
    }
    for player in replay.players_at(tick) {
        if let Some(position) = map_position(map_rect, &replay.meta.map_name, player.x, player.y) {
            let color = side_color(player.side.as_deref());
            match player.is_alive {
                Some(true) => {
                    painter.circle_filled(position, 5.0, color);
                }
                Some(false) => {
                    painter.line_segment(
                        [
                            position - egui::vec2(3.0, 3.0),
                            position + egui::vec2(3.0, 3.0),
                        ],
                        egui::Stroke::new(1.5, color),
                    );
                    painter.line_segment(
                        [
                            position + egui::vec2(-3.0, 3.0),
                            position + egui::vec2(3.0, -3.0),
                        ],
                        egui::Stroke::new(1.5, color),
                    );
                }
                None => {
                    painter.circle_stroke(
                        position,
                        6.0,
                        egui::Stroke::new(1.5, egui::Color32::GRAY),
                    );
                    painter.text(
                        position,
                        egui::Align2::CENTER_CENTER,
                        "?",
                        egui::FontId::proportional(8.0),
                        egui::Color32::WHITE,
                    );
                }
            }
            // CSDM's drawing uses -degreesToRadians(yaw) in radar coordinates.
            // Draw facing only for an observed live player with an observed finite yaw.
            if player.is_alive == Some(true) {
                if let Some(yaw) = player.yaw.filter(|yaw| yaw.is_finite()) {
                    let angle = -yaw.to_radians();
                    let direction = egui::vec2(angle.cos() as f32, angle.sin() as f32);
                    painter.line_segment(
                        [position, position + direction * 12.0],
                        egui::Stroke::new(1.5, egui::Color32::WHITE),
                    );
                }
            }
            if focus.as_deref() == Some(player.steamid.as_str()) {
                painter.circle_stroke(position, 9.0, egui::Stroke::new(2.0, egui::Color32::WHITE));
                painter.text(
                    position + egui::vec2(8.0, -9.0),
                    egui::Align2::LEFT_BOTTOM,
                    &player.name,
                    egui::FontId::proportional(12.0),
                    egui::Color32::WHITE,
                );
            }
            if let Some(hover) = response.hover_pos() {
                if hover.distance(position) < 9.0 {
                    response.clone().on_hover_text(format!(
                        "{} · tick {} · {}",
                        player.name,
                        tick,
                        player
                            .health
                            .map_or_else(|| "HP unavailable".into(), |v| format!("{v} HP"))
                    ));
                }
            }
        }
    }
    ui.small(if radar.is_some() {
        "Recorded source positions · hover a player for identity"
    } else {
        "No radar selected · recorded calibrated coordinates shown"
    });
    ui.small("Bomb carrier, effect areas and game audio unavailable.");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn calibrated_world_coordinates_match_pinned_csdm_math() {
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 1024.0));
        assert_eq!(
            map_position(rect, "de_mirage", -3230.0, 1713.0),
            Some(egui::Pos2::ZERO)
        );
        assert_eq!(
            map_position(rect, "de_mirage", -670.0, -847.0),
            Some(egui::pos2(512.0, 512.0))
        );
        assert!(map_position(rect, "de_unknown", 0.0, 0.0).is_none());
        assert!(map_position(rect, "de_mirage", f64::NAN, 0.0).is_none());
    }
    #[test]
    fn unavailable_values_are_not_rendered_as_zero() {
        assert_eq!(optional_number(None), "Unavailable");
        assert_eq!(optional_number(Some(0)), "0");
    }
}

#[cfg(test)]
#[path = "native_review_ui_tests.rs"]
mod interaction_tests;
