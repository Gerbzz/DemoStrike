use crate::analysis::AnalyzedMatch;
use crate::analyze_raw_json;
use crate::demo_import::{DemoFactsParser, ExternalExtractor, ImportControl, ImportedMatch, ParserCapabilities, import_demo, validate_demo_file};
use crate::native_grenade_parser::NativeGrenadeParser;
use crate::package::{PackageDraft, finalize_stream_probed_capture_package, load_draft};
use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::Duration;

enum ImportMessage {
    Stage(&'static str),
    Finished(Result<ImportedMatch, String>),
}

pub struct DemoStrikeApp {
    pub analyzed: Option<AnalyzedMatch>,
    pub error: Option<String>,
    capabilities: Option<ParserCapabilities>,
    source_label: Option<String>,
    pub draft: Option<PackageDraft>,
    draft_label: Option<String>,
    pub package_destination: String,
    pub package_result: Option<String>,
    pub node_path: String,
    pub extractor_path: String,
    pub database_path: String,
    import_source: Option<PathBuf>,
    import_stage: Option<String>,
    import_messages: Option<mpsc::Receiver<ImportMessage>>,
    import_cancel: Arc<ImportControl>,
}

impl Default for DemoStrikeApp {
    fn default() -> Self {
        let data_root = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            analyzed: None, error: None, capabilities: None, source_label: None,
            draft: None, draft_label: None, package_destination: String::new(),
            package_result: None,
            node_path: std::env::var("DEMOSTRIKE_NODE_PATH").unwrap_or_default(),
            extractor_path: std::env::var("DEMOSTRIKE_EXTRACTOR_SCRIPT").unwrap_or_default(),
            database_path: data_root.join("DemoStrike/matches.sqlite").display().to_string(),
            import_source: None, import_stage: None, import_messages: None,
            import_cancel: Arc::new(ImportControl::new()),
        }
    }
}

impl Drop for DemoStrikeApp {
    fn drop(&mut self) {
        self.import_cancel.request_cancel();
    }
}

impl DemoStrikeApp {
    fn start_demo_import(&mut self, path: &Path) -> Result<(), String> {
        self.start_demo_import_with_parser(path, Arc::new(NativeGrenadeParser))
    }

    fn start_reference_demo_import(&mut self, path: &Path) -> Result<(), String> {
        let parser = ExternalExtractor {
            node: PathBuf::from(&self.node_path),
            script: PathBuf::from(&self.extractor_path),
        };
        parser.check_available().map_err(|error| {
            self.error = Some(error.clone()); error
        })?;
        self.start_demo_import_with_parser(path, Arc::new(parser))
    }

    fn start_demo_import_with_parser(
        &mut self,
        path: &Path,
        parser: Arc<dyn DemoFactsParser>,
    ) -> Result<(), String> {
        if self.import_messages.is_some() {
            return Err("another demo import is still running".into());
        }
        validate_demo_file(path).map_err(|error| {
            self.error = Some(error.clone()); error
        })?;
        let path = path.to_path_buf();
        let worker_path = path.clone();
        let database = PathBuf::from(&self.database_path);
        let (sender, receiver) = mpsc::channel();
        let cancelled = Arc::new(ImportControl::new());
        let worker_cancelled = Arc::clone(&cancelled);
        std::thread::Builder::new().name("demo-import".into()).spawn(move || {
            let result = import_demo(&worker_path, &database, parser.as_ref(), &worker_cancelled,
                |stage| { let _ = sender.send(ImportMessage::Stage(stage)); });
            let _ = sender.send(ImportMessage::Finished(result));
        }).map_err(|error| {
            let message = format!("could not start demo import: {error}");
            self.error = Some(message.clone()); message
        })?;
        self.import_source = Some(path);
        self.import_stage = Some("Queued".into());
        self.import_messages = Some(receiver);
        self.import_cancel = cancelled;
        self.error = None;
        Ok(())
    }

    fn poll_demo_import(&mut self) {
        let mut finished = false;
        if let Some(receiver) = &self.import_messages {
            loop {
                let message = match receiver.try_recv() {
                    Ok(message) => message,
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        if !finished {
                            self.error = Some("demo import worker stopped before reporting a result".into());
                            self.import_stage = Some("Failed; previous analysis retained".into());
                            finished = true;
                        }
                        break;
                    }
                };
                match message {
                    ImportMessage::Stage(stage) => self.import_stage = Some(stage.into()),
                    ImportMessage::Finished(result) => {
                        finished = true;
                        match result {
                            Ok(imported) => {
                                self.capabilities = Some(imported.capabilities);
                                self.analyzed = Some(imported.analyzed);
                                self.source_label = self.import_source.as_ref()
                                    .map(|path| format!("{} (match {})", path.display(), imported.match_id));
                                self.error = None;
                                self.import_stage = Some("Complete".into());
                            }
                            Err(error) => {
                                self.error = Some(error);
                                self.import_stage = Some("Failed or cancelled; previous analysis retained".into());
                            }
                        }
                    }
                }
            }
        }
        if finished { self.import_messages = None; }
    }

    pub fn load_raw_json(&mut self, path: &Path) -> Result<(), String> {
        let result = std::fs::read(path)
            .map_err(|error| error.to_string())
            .and_then(|bytes| analyze_raw_json(&bytes).map_err(|error| error.to_string()));
        match result {
            Ok(analyzed) => {
                self.analyzed = Some(analyzed);
                self.capabilities = Some(ParserCapabilities::ReferenceRawFacts);
                self.source_label = Some(path.display().to_string());
                self.error = None;
                Ok(())
            }
            Err(error) => {
                self.error = Some(error.clone());
                Err(error)
            }
        }
    }

    pub fn load_capture_draft(&mut self, path: &Path) -> Result<(), String> {
        match load_draft(path) {
            Ok(draft) => {
                let folder_name = if draft
                    .package_id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
                {
                    format!("{}-package", draft.package_id)
                } else {
                    "new-capture-package".into()
                };
                self.package_destination = path
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(folder_name)
                    .display()
                    .to_string();
                self.draft_label = Some(path.display().to_string());
                self.draft = Some(draft);
                self.package_result = None;
                self.error = None;
                Ok(())
            }
            Err(error) => {
                self.error = Some(error.to_string());
                Err(error.to_string())
            }
        }
    }

    pub fn build_capture_package(&mut self) -> Result<(), String> {
        let draft = self
            .draft
            .as_ref()
            .ok_or_else(|| "open a capture draft first".to_string())?;
        match finalize_stream_probed_capture_package(
            Path::new(&self.package_destination),
            &draft.package_id,
            &draft.inputs,
        ) {
            Ok(package) => {
                self.package_result = Some(format!(
                    "{}: {} stream-probed clips. Listen to and review the capture before use.",
                    package.root.display(),
                    package.manifest.clips.len(),
                ));
                self.error = None;
                Ok(())
            }
            Err(error) => {
                self.error = Some(error.to_string());
                Err(error.to_string())
            }
        }
    }
}

impl eframe::App for DemoStrikeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_demo_import();
        ctx.set_visuals(egui::Visuals::dark());
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("DemoStrike");
                ui.label("Utility event analysis");
                if ui.add_enabled(self.import_messages.is_none(), egui::Button::new("Import validated demo (Rust grenades)" )).clicked() {
                    if let Some(path) = rfd::FileDialog::new().add_filter("CS2 demo", &["dem"]).pick_file() {
                        let _ = self.start_demo_import(&path);
                    }
                }
                if ui.add_enabled(self.import_messages.is_none(), egui::Button::new("Import with reference extractor")).clicked() {
                    if let Some(path) = rfd::FileDialog::new().add_filter("CS2 demo", &["dem"]).pick_file() {
                        let _ = self.start_reference_demo_import(&path);
                    }
                }
                if ui.button("Open capture draft").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Capture draft", &["json"])
                        .pick_file()
                    {
                        let _ = self.load_capture_draft(&path);
                    }
                }
                if ui.button("Open raw demo facts…").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Raw demo facts", &["json"])
                        .pick_file()
                    {
                        let _ = self.load_raw_json(&path);
                    }
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.collapsing("Demo parser configuration", |ui| {
                ui.small("Rust grenade mode uses crates.io source2-demo 0.5.9 and accepts three hash-validated Ancient/Nuke evaluation demos. Combat, flash effects, kills, and economy are unavailable in this mode. The reference extractor remains configurable below.");
                ui.horizontal(|ui| {
                    ui.label("Node runtime");
                    ui.text_edit_singleline(&mut self.node_path);
                    if ui.button("Browse").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_file() { self.node_path = path.display().to_string(); }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("CsThrowBack extractor");
                    ui.text_edit_singleline(&mut self.extractor_path);
                    if ui.button("Browse").clicked() {
                        if let Some(path) = rfd::FileDialog::new().add_filter("JavaScript module", &["mjs"]).pick_file() {
                            self.extractor_path = path.display().to_string();
                        }
                    }
                });
                ui.label(format!("Match database: {}", self.database_path));
            });
            if let Some(stage) = &self.import_stage { ui.label(format!("Demo import: {stage}")); }
            if self.import_messages.is_some() {
                if ui.add_enabled(self.import_cancel.can_cancel(), egui::Button::new("Cancel demo import")).clicked() {
                    self.import_cancel.request_cancel();
                }
                ctx.request_repaint_after(Duration::from_millis(100));
            }
            if let Some(result) = &self.package_result {
                ui.colored_label(egui::Color32::LIGHT_GREEN, result);
            } else {
                ui.colored_label(egui::Color32::YELLOW, "No capture media verified in this view.");
            }
            if let Some(error) = &self.error {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
            let mut build_requested = false;
            if let Some(draft) = &self.draft {
                ui.heading(format!("Capture draft: {}", draft.package_id));
                if let Some(path) = &self.draft_label { ui.small(path); }
                ui.label(format!("{} separate takes", draft.inputs.len()));
                for input in &draft.inputs {
                    ui.collapsing(format!("{} / {} / order {}", input.clip.clip_id, input.clip.event_id, input.clip.group_order), |ui| {
                        ui.label(format!("Source: {}", input.source_path.display()));
                        ui.label(format!("{} / round {} / ticks {}-{}", input.clip.map, input.clip.round, input.clip.start_tick, input.clip.end_tick));
                        ui.label(format!("Declared audio: {:?}, {} channels, {} Hz", input.clip.audio.source, input.clip.audio.channels, input.clip.audio.sample_rate));
                    });
                }
                ui.horizontal(|ui| {
                    ui.label("New package folder");
                    ui.text_edit_singleline(&mut self.package_destination);
                });
                build_requested = ui.button("Build stream-probed package").clicked();
                ui.small("Stream checks do not replace listening, source identity, or visual review.");
                ui.separator();
            }
            if build_requested { let _ = self.build_capture_package(); }
            let Some(analyzed) = &self.analyzed else {
                ui.heading("Import a CS2 demo or open raw facts");
                ui.label("Rust grenade import accepts three validated Ancient/Nuke demos; the configured reference extractor supports the existing raw-facts path. Game capture is separate.");
                return;
            };
            if let Some(path) = &self.source_label {
                ui.small(path);
            }
            if self.capabilities == Some(ParserCapabilities::GrenadeAnalysisOnlyV1) {
                ui.colored_label(egui::Color32::YELLOW, "Grenade analysis only: combat damage, blinds, kills, assists, and economy are unavailable. These metrics are not zero-valued results.");
            }
            ui.horizontal(|ui| {
                ui.label(format!("Map: {}", analyzed.map_name));
                ui.label(format!("Rounds: {}", analyzed.rounds.len()));
                ui.label(format!("Players: {}", analyzed.players.len()));
                ui.label(format!("Events: {}", analyzed.events.len()));
                ui.label(format!("Tick rate: {:.1}", analyzed.tick_rate));
            });
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                for event in &analyzed.events {
                    let label = format!(
                        "Round {} · {} · {} · tick {}",
                        event.round_number,
                        event.thrower_name,
                        event.utility_type.as_str(),
                        event.throw_tick
                    );
                    ui.collapsing(label, |ui| {
                        ui.label(format!("Player SteamID64: {}", event.thrower_steamid));
                        ui.label(format!("Team: {}", event.thrower_team));
                        ui.label(format!("Trajectory samples: {}", event.trajectory.len()));
                        if self.capabilities != Some(ParserCapabilities::GrenadeAnalysisOnlyV1) {
                            ui.label(format!("Enemy damage: {}", event.damage_enemy));
                        }
                        if let Some(tick) = event.detonate_tick {
                            ui.label(format!("Detonation tick: {tick}"));
                        }
                    });
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::DemoStrikeApp;
    use crate::demo_import::ParserCapabilities;
    use crate::demo_import::DemoFactsParser;
    use crate::parser::raw::RawDemo;
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;
    use crate::demo_import::ImportControl;
    use std::time::Duration;

    struct UiFixtureParser { wait_for_cancel: bool }
    impl DemoFactsParser for UiFixtureParser {
        fn parse(&self, _path: &Path, cancelled: &ImportControl) -> Result<RawDemo, String> {
            if self.wait_for_cancel {
                while !cancelled.is_cancelled() {
                    std::thread::sleep(Duration::from_millis(5));
                }
                return Err("demo import cancelled".into());
            }
            serde_json::from_slice(br#"{"version":1,"header":{"mapName":"de_train",
                "serverName":null,"demoTicks":6400,"playbackTime":100.0}}"#)
                .map_err(|error| error.to_string())
        }
    }

    fn wait_for_import(app: &mut DemoStrikeApp) {
        for _ in 0..200 {
            app.poll_demo_import();
            if app.import_messages.is_none() { return; }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("demo import worker did not complete");
    }

    #[test]
    fn native_import_worker_persists_demo_and_cancel_keeps_previous_analysis() {
        let dir = tempfile::tempdir().unwrap();
        let demo = dir.path().join("fixture.dem");
        fs::write(&demo, b"PBDEMS2\0fixture").unwrap();
        let mut app = DemoStrikeApp::default();
        app.database_path = dir.path().join("matches.sqlite").display().to_string();
        app.start_demo_import_with_parser(&demo, Arc::new(UiFixtureParser { wait_for_cancel: false })).unwrap();
        wait_for_import(&mut app);
        assert_eq!(app.analyzed.as_ref().unwrap().map_name, "de_train");
        assert!(app.error.is_none());
        let saved = crate::db::queries::list_matches(&crate::db::open(Path::new(&app.database_path)).unwrap()).unwrap();
        assert_eq!(saved.len(), 1);

        app.start_demo_import_with_parser(&demo, Arc::new(UiFixtureParser { wait_for_cancel: true })).unwrap();
        app.import_cancel.request_cancel();
        wait_for_import(&mut app);
        assert!(app.error.as_ref().unwrap().contains("cancelled"));
        assert_eq!(app.analyzed.as_ref().unwrap().map_name, "de_train");
        let saved = crate::db::queries::list_matches(&crate::db::open(Path::new(&app.database_path)).unwrap()).unwrap();
        assert_eq!(saved.len(), 1);

        app.start_demo_import_with_parser(&demo, Arc::new(UiFixtureParser { wait_for_cancel: true })).unwrap();
        let cancel_signal = Arc::clone(&app.import_cancel);
        drop(app);
        assert!(cancel_signal.is_cancelled());
    }

    #[test]
    fn native_worker_imports_validated_real_demo_with_visible_limited_capability() {
        let Some(demo) = std::env::var_os("DEMOSTRIKE_VALIDATED_DEMO").map(std::path::PathBuf::from) else { return };
        let dir = tempfile::tempdir().unwrap();
        let mut app = DemoStrikeApp::default();
        app.database_path = dir.path().join("real.sqlite").display().to_string();
        app.start_demo_import(&demo).unwrap();
        for _ in 0..3000 {
            app.poll_demo_import();
            if app.import_messages.is_none() { break }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(app.import_messages.is_none(), "native worker did not finish");
        assert!(app.error.is_none(), "{:?}", app.error);
        assert_eq!(app.capabilities, Some(ParserCapabilities::GrenadeAnalysisOnlyV1));
        assert_eq!(app.analyzed.as_ref().unwrap().events.len(), 37);
        let matches = crate::db::queries::list_matches(&crate::db::open(Path::new(&app.database_path)).unwrap()).unwrap();
        assert_eq!(matches[0].parser_capability, ParserCapabilities::GrenadeAnalysisOnlyV1);
    }

    #[test]
    fn native_view_model_loads_analyzed_raw_facts_and_reports_bad_input() {
        let dir = tempfile::TempDir::new().unwrap();
        let good = dir.path().join("raw.json");
        fs::write(
            &good,
            br#"{
            "version": 1,
            "header": {"mapName": "de_mirage", "serverName": null,
                "demoTicks": 6400, "playbackTime": 100.0}
        }"#,
        )
        .unwrap();
        let mut app = DemoStrikeApp::default();
        app.load_raw_json(&good).unwrap();
        assert_eq!(app.analyzed.as_ref().unwrap().map_name, "de_mirage");
        assert!(app.error.is_none());

        let bad = dir.path().join("bad.json");
        fs::write(&bad, b"not JSON").unwrap();
        assert!(app.load_raw_json(&bad).is_err());
        assert!(app.error.is_some());
        assert_eq!(app.analyzed.as_ref().unwrap().map_name, "de_mirage");
    }

    #[test]
    fn native_view_model_reviews_draft_and_refuses_fake_media() {
        let dir = tempfile::TempDir::new().unwrap();
        let source = dir.path().join("fake.mp4");
        fs::write(&source, b"abc").unwrap();
        let draft_path = dir.path().join("draft.json");
        let hash = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let draft = serde_json::json!({"package_id":"native-pilot","inputs":[{
            "source_path":source,"clip":{
                "clip_id":"take-1","relative_path":"","sha256":"",
                "demo_sha256":hash,"event_id":"event-1","player_steamid64":"76561198000000001",
                "map":"de_mirage","round":4,"utility_type":"smoke","start_tick":1000,"end_tick":1100,
                "camera_role":"projectile","duration_ms":1000,"width":1920,"height":1080,
                "fps_num":60,"fps_den":1,"audio":{"source":"hlae_wav","channels":2,
                    "sample_rate":48000,"verified":true},"qc_status":"accepted","group_order":1
            }
        }]});
        fs::write(&draft_path, serde_json::to_vec(&draft).unwrap()).unwrap();
        let mut app = DemoStrikeApp::default();
        app.load_capture_draft(&draft_path).unwrap();
        assert_eq!(app.draft.as_ref().unwrap().inputs.len(), 1);
        assert!(app.package_destination.ends_with("native-pilot-package"));
        assert!(app.build_capture_package().is_err());
        assert!(app.package_result.is_none());
        assert!(!std::path::Path::new(&app.package_destination).exists());
    }
}
