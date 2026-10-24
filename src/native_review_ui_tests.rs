//! Headless interaction tests of the actual native egui review surface.
//! All data below is explicitly SYNTHETIC. These tests exercise real egui
//! layout/input/paint commands, not a native GPU, window, or real demo.
use super::*;
use crate::match_review::{
    MatchSummary, ObservedBombSelection, PlayerSummary, RoundSummary, ScoreRelation, UtilityCount,
    UtilityEvent, ViewerBombEvent, ViewerFrameChunk, ViewerGrenadeFrame, ViewerPlayerFrame,
    ViewerRoundMeta, ViewerUtilityEvent,
};
use egui::epaint::Shape;

fn summary(id: i64) -> MatchSummary {
    MatchSummary {
        id,
        file_name: format!("SYNTHETIC-only-{id}.dem"),
        demo_file_path: None,
        map_name: "de_mirage".into(),
        score_ct: 1,
        score_t: 1,
        score_ct_relation: ScoreRelation::Tied,
        score_t_relation: ScoreRelation::Tied,
        imported_at: "SYNTHETIC test fixture".into(),
        parser_capability: "test-synthetic-combat-v1".into(),
        source_sha256: None,
        tick_rate: Some(64.0),
        duration_ticks: Some(300),
        player_count: 3,
        round_count: 1,
        utility_count: 1,
    }
}
fn fixture(id: i64) -> LoadedReview {
    let round = RoundSummary {
        round_number: 3,
        winner: None,
        start_tick: Some(100),
        freeze_end_tick: Some(200),
        end_tick: Some(300),
        end_reason: None,
        team_a_side: Some("CT".into()),
        team_b_side: Some("T".into()),
        ct_equip_value: None,
        t_equip_value: None,
        combat: None,
        deaths: None,
        hurts: None,
    };
    let meta = ViewerRoundMeta {
        round_number: 3,
        start_tick: Some(100),
        end_tick: Some(300),
        tick_rate: Some(64.0),
        map_name: "de_mirage".into(),
        positions_available: true,
        player_sample_count: 6,
        grenade_sample_count: 2,
        utility_events: vec![ViewerUtilityEvent {
            id: 7,
            utility_type: "smoke".into(),
            throw_tick: 170,
            detonate_tick: Some(180),
            detonate_x: Some(-1500.0),
            detonate_y: Some(500.0),
            detonate_z: Some(0.0),
            projectile_entity_id: Some(88),
            observed_trajectory_index: Some(9),
            thrower_steamid: "synthetic-alive".into(),
            thrower_name: "SYNTHETIC Alive".into(),
            thrower_side: "CT".into(),
        }],
        bomb_events: vec![
            ViewerBombEvent {
                source_index: 0,
                kind: "bomb_planted".into(),
                tick: 200,
                actor_steamid: Some("synthetic-alive".into()),
                actor_name: Some("SYNTHETIC Alive".into()),
                site_id: None,
                x: Some(-1400.0),
                y: Some(-100.0),
                z: Some(0.0),
            },
            ViewerBombEvent {
                source_index: 1,
                kind: "bomb_exploded".into(),
                tick: 320,
                actor_steamid: None,
                actor_name: None,
                site_id: None,
                x: None,
                y: None,
                z: None,
            },
        ],
        bomb_events_available: true,
        observed_bomb_selection: Some(ObservedBombSelection {
            plant_index: Some(0),
            explosion_index: Some(1),
            defusal_index: None,
        }),
    };
    let mut players = Vec::new();
    for tick in [180, 200] {
        for (steamid, name, side, alive, health, x, y) in [
            (
                "synthetic-alive",
                "SYNTHETIC Alive",
                "CT",
                Some(true),
                Some(75),
                -2000.0,
                600.0,
            ),
            (
                "synthetic-unknown",
                "SYNTHETIC Unknown",
                "T",
                None,
                None,
                -1000.0,
                0.0,
            ),
            (
                "synthetic-dead",
                "SYNTHETIC Dead",
                "T",
                Some(false),
                Some(0),
                -500.0,
                -500.0,
            ),
        ] {
            players.push(ViewerPlayerFrame {
                tick,
                steamid: steamid.into(),
                name: name.into(),
                side: Some(side.into()),
                x,
                y,
                z: 0.0,
                is_alive: alive,
                health,
                yaw: None,
            });
        }
    }
    let grenades = [190, 200]
        .into_iter()
        .map(|tick| ViewerGrenadeFrame {
            tick,
            trajectory_index: 9,
            entity_id: 88,
            grenade_type: Some("Smoke".into()),
            thrower_steamid: Some("synthetic-alive".into()),
            x: if tick == 190 { -1600.0 } else { -1500.0 },
            y: 500.0,
            z: 0.0,
        })
        .collect();
    let replay = ReplayRound::new(
        meta,
        vec![ViewerFrameChunk {
            start_tick: 100,
            end_tick: 300,
            players,
            grenades,
        }],
        Some(&round),
    )
    .unwrap();
    LoadedReview {
        library: LibrarySnapshot {
            path: "SYNTHETIC memory-only library".into(),
            matches: vec![summary(id)],
        },
        detail: Some(MatchDetail {
            summary: summary(id),
            rounds: vec![round],
            players: vec![
                PlayerSummary {
                    steamid: "synthetic-alive".into(),
                    name: "SYNTHETIC Alive".into(),
                    combat: None,
                },
                PlayerSummary {
                    steamid: "synthetic-unknown".into(),
                    name: "SYNTHETIC Unknown".into(),
                    combat: None,
                },
                PlayerSummary {
                    steamid: "synthetic-dead".into(),
                    name: "SYNTHETIC Dead".into(),
                    combat: None,
                },
            ],
            utility_events: vec![UtilityEvent {
                id: 7,
                round_number: 3,
                utility_type: "smoke".into(),
                thrower_name: "SYNTHETIC Alive".into(),
                thrower_team: "CT".into(),
                throw_tick: 170,
                enemies_blinded: None,
                damage_enemy: None,
            }],
            utility_breakdown: vec![UtilityCount {
                utility_type: "smoke".into(),
                count: 1,
            }],
            full_combat_coverage: false,
            observed_duels: None,
        }),
        replay: Some(replay),
        round_number: 3,
        replay_error: None,
    }
}

struct Interaction {
    context: egui::Context,
    review: NativeReviewUi,
    size: egui::Vec2,
    time: f64,
    actions: Vec<ReviewAction>,
}
impl Interaction {
    fn new(width: f32, height: f32) -> Self {
        let mut review = NativeReviewUi::default();
        review.active = true;
        review.install(fixture(1));
        Self {
            context: egui::Context::default(),
            review,
            size: egui::vec2(width, height),
            time: 0.0,
            actions: Vec::new(),
        }
    }
    fn frame(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
        self.time += 1.0 / 60.0;
        let mut output = self.context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.size)),
                time: Some(self.time),
                events,
                ..Default::default()
            },
            |ui| {
                self.actions.extend(self.review.show(ui));
            },
        );
        // This deliberately headless test observes paint commands and CPU
        // meshes, not a GPU texture upload. Acknowledge unused font deltas.
        output.textures_delta.clear();
        output
    }
    fn click_matching(&mut self, predicate: impl Fn(&str) -> bool) -> egui::FullOutput {
        let before = self.frame(vec![]);
        let (label, rect) = text_shapes(&before)
            .into_iter()
            .find(|(text, _)| predicate(text))
            .unwrap_or_else(|| panic!("target not painted; labels: {:?}", texts(&before)));
        let pos = rect.center();
        assert!(
            egui::Rect::from_min_size(egui::Pos2::ZERO, self.size).contains(pos),
            "target outside viewport: {label} {rect:?}"
        );
        self.frame(vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ]);
        self.frame(vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }]);
        self.frame(vec![])
    }
    fn click(&mut self, label: &str) -> egui::FullOutput {
        self.click_matching(|text| text == label)
    }
}
fn visit_shapes<'a>(shape: &'a Shape, out: &mut Vec<&'a Shape>) {
    if let Shape::Vec(children) = shape {
        for child in children {
            visit_shapes(child, out);
        }
    } else {
        out.push(shape);
    }
}
fn shapes(output: &egui::FullOutput) -> Vec<&Shape> {
    let mut out = Vec::new();
    for clipped in &output.shapes {
        visit_shapes(&clipped.shape, &mut out);
    }
    out
}
fn text_shapes(output: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
    shapes(output)
        .into_iter()
        .filter_map(|shape| match shape {
            Shape::Text(text) => Some((
                text.galley.job.text.clone(),
                text.galley.rect.translate(text.pos.to_vec2()),
            )),
            _ => None,
        })
        .collect()
}
fn texts(output: &egui::FullOutput) -> Vec<String> {
    text_shapes(output)
        .into_iter()
        .map(|(text, _)| text)
        .collect()
}
fn has_text(output: &egui::FullOutput, needle: &str) -> bool {
    texts(output).iter().any(|t| t.contains(needle))
}

#[test]
fn actual_native_tabs_receive_pointer_input_and_paint_recorded_fixture_content() {
    let mut ui = Interaction::new(1440.0, 900.0);
    let overview = ui.frame(vec![]);
    assert!(has_text(&overview, "SYNTHETIC-only-1.dem"));
    assert!(has_text(
        &overview,
        "Native parser: test-synthetic-combat-v1"
    ));
    let rounds = ui.click("Rounds");
    assert_eq!(ui.review.tab, Tab::Rounds);
    assert!(has_text(&rounds, "Winner: unavailable"));
    assert!(has_text(&rounds, "100 → 300"));
    let players = ui.click("Players");
    assert_eq!(ui.review.tab, Tab::Players);
    assert!(has_text(&players, "Effective HP"));
    assert!(has_text(&players, "SYNTHETIC Unknown"));
    assert!(
        texts(&players)
            .iter()
            .filter(|t| t.as_str() == "Unavailable")
            .count()
            >= 12
    );
    let utility = ui.click("Utility");
    assert_eq!(ui.review.tab, Tab::Utility);
    assert!(has_text(&utility, "Enemy damage: Unavailable"));
    assert!(has_text(&utility, "enemies blinded: Unavailable"));
    let viewer = ui.click("2D Viewer");
    assert_eq!(ui.review.tab, Tab::Viewer);
    assert!(has_text(&viewer, "Recorded events (3)"));
    assert!(has_text(&viewer, "Health unavailable"));
    assert!(has_text(&viewer, "75 HP"));
    assert!(has_text(&viewer, "Out"));
    assert!(!has_text(&viewer, "SYNTHETIC Unknown 0 HP"));
    let overview = ui.click("Overview");
    assert_eq!(ui.review.tab, Tab::Overview);
    assert!(has_text(&overview, "Recorded rounds"));
}

#[test]
fn actual_event_click_seeks_exact_source_tick_and_disabled_inter_round_event_cannot_seek() {
    let mut ui = Interaction::new(1440.0, 900.0);
    ui.click("2D Viewer");
    assert_eq!(ui.review.clock.as_ref().unwrap().current_tick(), 200);
    ui.click_matching(|text| text.contains("180  smoke detonated"));
    assert_eq!(ui.review.clock.as_ref().unwrap().current_tick(), 180);
    assert!(!ui.review.clock.as_ref().unwrap().is_playing());
    assert_eq!(ui.review.selected_event.as_deref(), Some("utility-7"));
    ui.click_matching(|text| text.starts_with("320  "));
    assert_eq!(ui.review.clock.as_ref().unwrap().current_tick(), 180);
    assert_eq!(ui.review.selected_event.as_deref(), Some("utility-7"));
    ui.click("Start");
    assert_eq!(ui.review.clock.as_ref().unwrap().current_tick(), 100);
    assert!(!ui.review.clock.as_ref().unwrap().is_playing());
}

#[test]
fn native_scene_paints_observed_players_projectile_bomb_and_distinct_unknown_marker() {
    let mut ui = Interaction::new(1440.0, 900.0);
    let output = ui.click("2D Viewer");
    let shapes = shapes(&output);
    let radar = shapes
        .iter()
        .find_map(|shape| match shape {
            Shape::Rect(rect) if rect.fill == egui::Color32::from_rgb(24, 31, 38) => {
                Some(rect.rect)
            }
            _ => None,
        })
        .expect("actual native radar background");
    let expected_alive = map_position(radar, "de_mirage", -2000.0, 600.0).unwrap();
    let expected_unknown = map_position(radar, "de_mirage", -1000.0, 0.0).unwrap();
    let expected_projectile = map_position(radar, "de_mirage", -1500.0, 500.0).unwrap();
    let expected_bomb = map_position(radar, "de_mirage", -1400.0, -100.0).unwrap();
    let circles: Vec<_> = shapes
        .iter()
        .filter_map(|shape| match shape {
            Shape::Circle(c) => Some(c),
            _ => None,
        })
        .collect();
    let at = |center: egui::Pos2, radius: f32| {
        circles
            .iter()
            .find(|c| c.center.distance(center) < 0.1 && (c.radius - radius).abs() < 0.1)
            .copied()
    };
    assert_eq!(
        at(expected_alive, 5.0)
            .expect("observed live player circle")
            .fill,
        side_color(Some("CT"))
    );
    let unknown = at(expected_unknown, 6.0).expect("unknown-alive outline");
    assert_eq!(unknown.fill, egui::Color32::TRANSPARENT);
    assert_eq!(unknown.stroke.color, egui::Color32::GRAY);
    assert!(has_text(&output, "?"));
    assert_eq!(
        at(expected_projectile, 4.0)
            .expect("observed projectile")
            .fill,
        egui::Color32::LIGHT_GREEN
    );
    assert!(at(expected_bomb, 9.0).is_some());
    assert!(has_text(&output, "B"));
    assert!(
        !ui.context
            .tessellate(output.shapes, output.pixels_per_point)
            .is_empty(),
        "real egui paint tessellates to CPU meshes"
    );
}

#[test]
fn important_native_selectors_and_docked_events_fit_1024_and_1440_viewports() {
    for (width, height) in [(1024.0, 768.0), (1440.0, 900.0)] {
        let mut ui = Interaction::new(width, height);
        let output = ui.click("2D Viewer");
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height));
        let text = text_shapes(&output);
        for label in [
            "Overview",
            "Rounds",
            "Players",
            "Utility",
            "2D Viewer",
            "Choose library…",
            "Import CS2 demo…",
            "Capture & library tools",
            "Choose radar PNG…",
            "Recorded events (3)",
        ] {
            let (_, rect) = text
                .iter()
                .find(|(t, _)| t == label)
                .unwrap_or_else(|| panic!("missing {label} at {width}"));
            assert!(
                screen.expand(1.0).contains_rect(*rect),
                "{label} exceeds {width}×{height}: {rect:?}"
            );
        }
        let radar = shapes(&output)
            .into_iter()
            .find_map(|shape| match shape {
                Shape::Rect(rect) if rect.fill == egui::Color32::from_rgb(24, 31, 38) => {
                    Some(rect.rect)
                }
                _ => None,
            })
            .unwrap();
        let (_, events) = text
            .iter()
            .find(|(t, _)| t == "Recorded events (3)")
            .unwrap();
        assert!(
            radar.right() <= events.left(),
            "event panel must be docked, not overlap radar"
        );
    }
}

#[test]
fn stale_worker_generation_cannot_replace_the_current_native_match() {
    let mut ui = Interaction::new(1024.0, 768.0);
    ui.review.install(fixture(2));
    ui.review.generation = 2;
    let (sender, receiver) = mpsc::channel();
    sender.send(Ok(fixture(1))).unwrap();
    ui.review.pending = Some(PendingReview {
        generation: 1,
        receiver,
        cancel: Arc::new(AtomicBool::new(false)),
    });
    let output = ui.frame(vec![]);
    assert!(ui.review.pending.is_none());
    assert_eq!(
        ui.review
            .loaded
            .as_ref()
            .unwrap()
            .detail
            .as_ref()
            .unwrap()
            .summary
            .id,
        2
    );
    assert!(has_text(&output, "SYNTHETIC-only-2.dem"));
    assert!(!has_text(&output, "SYNTHETIC-only-1.dem"));
}

#[test]
fn disconnected_native_loader_clears_pending_and_paints_error_instead_of_spinning_forever() {
    let mut ui = Interaction::new(1024.0, 768.0);
    ui.review.generation = 1;
    let (sender, receiver) = mpsc::channel::<Result<LoadedReview, String>>();
    drop(sender);
    ui.review.pending = Some(PendingReview {
        generation: 1,
        receiver,
        cancel: Arc::new(AtomicBool::new(false)),
    });
    let output = ui.frame(vec![]);
    assert!(ui.review.pending.is_none());
    assert!(
        ui.review
            .error
            .as_deref()
            .unwrap()
            .contains("stopped before reporting")
    );
    assert!(has_text(&output, "stopped before reporting"));
    assert!(!has_text(
        &output,
        "Loading recorded match and indexed replay"
    ));
    assert_eq!(
        ui.review
            .loaded
            .as_ref()
            .unwrap()
            .detail
            .as_ref()
            .unwrap()
            .summary
            .id,
        1
    );
}

#[test]
fn actual_native_play_pause_speed_and_start_controls_own_the_rust_clock() {
    let mut ui = Interaction::new(1440.0, 900.0);
    ui.click("2D Viewer");
    ui.click("1.5×");
    assert_eq!(ui.review.clock.as_ref().unwrap().speed(), 1.5);
    ui.click("Play");
    assert!(ui.review.clock.as_ref().unwrap().is_playing());
    ui.click("Pause");
    assert!(!ui.review.clock.as_ref().unwrap().is_playing());
    let paused_tick = ui.review.clock.as_ref().unwrap().current_tick();
    ui.review.last_frame = Instant::now() - std::time::Duration::from_secs(10);
    ui.frame(vec![]);
    assert_eq!(
        ui.review.clock.as_ref().unwrap().current_tick(),
        paused_tick
    );
    ui.click("Start");
    assert_eq!(ui.review.clock.as_ref().unwrap().current_tick(), 100);
    ui.click("2×");
    ui.click("Play");
    // Advance the real UI's elapsed-time input without a wall-clock sleep.
    // The exact source-tick/rate arithmetic is separately tested in the engine.
    ui.review.last_frame = Instant::now() - std::time::Duration::from_secs(10);
    let ended = ui.frame(vec![]);
    assert_eq!(ui.review.clock.as_ref().unwrap().current_tick(), 300);
    assert!(!ui.review.clock.as_ref().unwrap().is_playing());
    assert!(has_text(&ended, "Play"));
    assert!(!has_text(&ended, "Pause"));
}

#[test]
fn capture_action_keeps_loaded_source_identity_while_another_library_is_requested() {
    let mut ui = Interaction::new(1440.0, 900.0);
    ui.review
        .loaded
        .as_mut()
        .unwrap()
        .detail
        .as_mut()
        .unwrap()
        .summary
        .source_sha256 = Some("b".repeat(64));
    let loaded_path = PathBuf::from(&ui.review.loaded.as_ref().unwrap().library.path);
    ui.review.open(
        PathBuf::from("SYNTHETIC other unavailable library.sqlite"),
        Some(99),
        None,
    );
    ui.click("Utility");
    ui.click("Prepare capture request");
    let action = ui
        .actions
        .iter()
        .find_map(|action| match action {
            ReviewAction::PrepareCapture {
                event_id,
                database,
                match_id,
                source_sha256,
            } => Some((*event_id, database, *match_id, source_sha256)),
            _ => None,
        })
        .expect("actual Utility button must emit a source-bound action");
    assert_eq!(action.0, 7);
    assert_eq!(action.1, &loaded_path);
    assert_eq!(action.2, 1);
    assert_eq!(action.3.as_deref(), Some("b".repeat(64).as_str()));
}
#[test]
fn screenshot_settle_counter_resets_when_the_painted_source_tick_changes() {
    let mut ui = Interaction::new(1440.0, 900.0);
    ui.click("2D Viewer");
    ui.frame(vec![]);
    ui.frame(vec![]);
    assert!(ui.review.settled_paint().is_ok());
    ui.review.clock.as_mut().unwrap().seek(180);
    ui.frame(vec![]);
    assert!(ui.review.settled_paint().is_err());
    ui.frame(vec![]);
    let proof = ui.review.settled_paint().unwrap();
    assert_eq!(proof.tick, 180);
    assert_eq!(proof.viewer_paint_frames, 2);
    ui.review.zoom = 2.0;
    ui.frame(vec![]);
    assert!(ui.review.settled_paint().is_err());
    ui.frame(vec![]);
    assert!(ui.review.settled_paint().is_ok());
}
