use crate::analysis::AnalyzedMatch;
use crate::analyze_raw_json;
use crate::capture_request::{build_capture_request, save_request_noclobber};
use crate::demo_import::{DemoFactsParser, ExternalExtractor, ImportControl, ImportedMatch, ParserCapabilities, import_demo, validate_demo_file};
use crate::db::{self, queries};
use crate::domain::{EventFilter, MatchSummary, UtilityEventDetail, UtilityEventSummary};
use crate::native_grenade_parser::NativeGrenadeParser;
use crate::package::{PackageDraft, finalize_stream_probed_capture_package, load_draft};
use eframe::egui;
use clipstrike_ui::ui::{EditorHostResources, EmbeddedEditorWorkspace};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
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
    capture_request_messages: Option<mpsc::Receiver<Result<PathBuf, String>>>,
    capture_request_status: Option<String>,
    package_import_messages: Option<mpsc::Receiver<Result<(PathBuf, String), String>>>,
    package_import_cancel: Arc<AtomicBool>,
    imported_project_path: Option<PathBuf>,
    package_import_summary: Option<String>,
    editor_resources: Option<EditorHostResources>,
    embedded_editor: Option<EmbeddedEditorWorkspace>,
    confirm_discard_editor: bool,
    library_matches: Vec<MatchSummary>,
    library_opened: bool,
    library_events: Vec<UtilityEventSummary>,
    library_match_id: Option<i64>,
    library_page: i64,
    library_search: String,
    library_search_applied: String,
    library_favorites_only: bool,
    selected_event: Option<UtilityEventDetail>,
    tag_draft: String,
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
            capture_request_messages: None, capture_request_status: None,
            package_import_messages: None, imported_project_path: None,
            package_import_cancel: Arc::new(AtomicBool::new(false)),
            package_import_summary: None,
            editor_resources: None,
            embedded_editor: None, confirm_discard_editor: false,
            library_matches: Vec::new(), library_opened: false, library_events: Vec::new(), library_match_id: None,
            library_page: 0, library_search: String::new(), library_search_applied: String::new(),
            library_favorites_only: false, selected_event: None, tag_draft: String::new(),
        }
    }
}

impl Drop for DemoStrikeApp {
    fn drop(&mut self) {
        self.import_cancel.request_cancel();
        self.package_import_cancel.store(true, Ordering::Relaxed);
    }
}

impl DemoStrikeApp {
    fn start_package_import(&mut self, package_dir: &Path, project_path: &Path) -> Result<(), String> {
        if self.package_import_messages.is_some() {
            return Err("another capture package is still being imported".into());
        }
        let package_dir = package_dir.to_path_buf();
        let project_path = project_path.to_path_buf();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new().name("capture-package-import".into()).spawn(move || {
            let result = crate::capture_project::import_package_to_new_project_with_cancel(
                &package_dir, &project_path,
                || worker_cancel.load(Ordering::Relaxed))
                .map(|imported| {
                    let shortened: Vec<_> = imported.coverage.iter()
                        .filter(|clip| clip.editor_duration_ms < clip.declared_duration_ms)
                        .map(|clip| format!("{}: {} ms editor / {} ms source",
                            clip.clip_id, clip.editor_duration_ms, clip.declared_duration_ms))
                        .collect();
                    let summary = if shortened.is_empty() {
                        "All visual clips fit their verified video and audio streams.".to_string()
                    } else {
                        format!("Editor clips stop at verified video/audio coverage; original source durations remain in capture provenance. Shortened: {}", shortened.join(", "))
                    };
                    (project_path, summary)
                }).map_err(|error| error.to_string());
            let _ = sender.send(result);
        }).map_err(|error| format!("could not start package import: {error}"))?;
        self.package_import_messages = Some(receiver);
        self.package_import_cancel = cancel;
        self.imported_project_path = None;
        self.package_import_summary = None;
        Ok(())
    }

    fn poll_package_import(&mut self) {
        let result = self.package_import_messages.as_ref().and_then(|receiver| match receiver.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                "package import worker stopped before reporting a result".into())),
        });
        if let Some(result) = result {
            self.package_import_messages = None;
            match result {
                Ok((path, summary)) => {
                    self.imported_project_path = Some(path);
                    self.package_import_summary = Some(summary);
                    self.error = None;
                }
                Err(error) => {
                    self.package_import_summary = None;
                    self.error = Some(error);
                },
            }
        }
    }

    /// Capture owned WGPU resources while eframe creates the native window.
    /// The same handle safely opens projects imported much later in this run.
    pub fn with_native_context(cc: &eframe::CreationContext<'_>) -> Result<Self, String> {
        let mut app = Self::default();
        app.editor_resources = Some(EditorHostResources::capture(cc)?);
        Ok(app)
    }

    pub fn with_saved_project(cc: &eframe::CreationContext<'_>, path: &Path) -> Result<Self, String> {
        let mut app = Self::with_native_context(cc)?;
        app.open_saved_project(path)?;
        Ok(app)
    }

    fn open_saved_project(&mut self, path: &Path) -> Result<(), String> {
        if self.embedded_editor.is_some() { return Err("return from the current edit before opening another project".into()); }
        let resources = self.editor_resources.as_ref()
            .ok_or("WGPU editor resources are unavailable in this window")?;
        self.embedded_editor = Some(resources.open_project(path)?);
        self.confirm_discard_editor = false;
        self.error = None;
        Ok(())
    }

    fn show_embedded_editor(&mut self, ctx: &egui::Context) {
        let mut return_requested = false;
        if let Some(editor) = self.embedded_editor.as_mut() {
            editor.show(ctx);
            if editor.take_return_request() {
                if editor.is_exporting() {
                    self.error = Some("Finish or cancel the active editor export before returning to capture".into());
                } else if editor.has_unsaved_changes() {
                    self.confirm_discard_editor = true;
                } else {
                    return_requested = true;
                }
            }
        }
        if self.confirm_discard_editor {
            egui::Window::new("Unsaved editor changes")
                .collapsible(false).resizable(false).show(ctx, |ui| {
                    ui.label("Save in the editor before returning, or discard these unsaved edits.");
                    if ui.button("Keep editing").clicked() { self.confirm_discard_editor = false; }
                    if ui.button("Discard edits and return").clicked() {
                        self.confirm_discard_editor = false;
                        return_requested = true;
                    }
                });
        }
        if return_requested { self.embedded_editor = None; }
    }

    fn start_capture_request(&mut self, event_id: i64, destination: &Path) -> Result<(), String> {
        if self.capture_request_messages.is_some() {
            return Err("another capture request is still being prepared".into());
        }
        let database = PathBuf::from(&self.database_path);
        let destination = destination.to_path_buf();
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new().name("capture-request".into()).spawn(move || {
            let result = build_capture_request(&database, event_id)
                .and_then(|request| save_request_noclobber(&request, &destination).map(|_| destination));
            let _ = sender.send(result);
        }).map_err(|error| format!("could not start capture request worker: {error}"))?;
        self.capture_request_messages = Some(receiver);
        self.capture_request_status = Some("Verifying source demo and event".into());
        self.error = None;
        Ok(())
    }

    fn poll_capture_request(&mut self) {
        let result = self.capture_request_messages.as_ref().and_then(|receiver| match receiver.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                "capture request worker stopped before reporting a result".into())),
        });
        if let Some(result) = result {
            self.capture_request_messages = None;
            match result {
                Ok(path) => self.capture_request_status = Some(format!(
                    "Capture request saved: {}. No recording has started; POV remains unverified.", path.display())),
                Err(error) => {
                    self.capture_request_status = Some("Capture request failed".into());
                    self.error = Some(error);
                }
            }
        }
    }

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
        let mut new_match_id = None;
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
                                new_match_id = Some(imported.match_id);
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
        if let Some(id) = new_match_id {
            self.library_match_id = Some(id);
            if let Err(error) = self.refresh_library() { self.error = Some(error); }
        }
    }

    fn refresh_library(&mut self) -> Result<(), String> {
        self.library_opened = true;
        let connection = db::open(Path::new(&self.database_path)).map_err(|e| e.to_string())?;
        self.library_matches = queries::list_matches(&connection).map_err(|e| e.to_string())?;
        if !self.library_matches.iter().any(|m| Some(m.id) == self.library_match_id) {
            self.library_match_id = self.library_matches.first().map(|m| m.id);
        }
        self.refresh_library_events()
    }

    fn refresh_library_events(&mut self) -> Result<(), String> {
        self.library_events.clear();
        self.selected_event = None;
        let Some(id) = self.library_match_id else { return Ok(()); };
        let connection = db::open(Path::new(&self.database_path)).map_err(|e| e.to_string())?;
        self.library_events = queries::query_events(&connection, &EventFilter {
            match_ids: Some(vec![id]), search: Some(self.library_search_applied.clone()),
            favorites_only: Some(self.library_favorites_only), limit: Some(200),
            offset: Some(self.library_page * 200), ..Default::default()
        }).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn select_saved_event(&mut self, id: i64) -> Result<(), String> {
        let connection = db::open(Path::new(&self.database_path)).map_err(|e| e.to_string())?;
        self.selected_event = Some(queries::get_event_detail(&connection, id).map_err(|e| e.to_string())?);
        Ok(())
    }

    fn update_saved_event(&mut self, id: i64, favorite: Option<bool>, tag: Option<&str>, remove_tag: Option<&str>) -> Result<(), String> {
        let connection = db::open(Path::new(&self.database_path)).map_err(|e| e.to_string())?;
        if let Some(value) = favorite { queries::set_favorite(&connection, id, value).map_err(|e| e.to_string())?; }
        if let Some(name) = tag { queries::tag_event(&connection, id, name).map_err(|e| e.to_string())?; }
        if let Some(name) = remove_tag {
            let tag = queries::list_tags(&connection).map_err(|e| e.to_string())?
                .into_iter().find(|t| t.name == name).ok_or("tag no longer exists")?;
            queries::untag_event(&connection, id, tag.id).map_err(|e| e.to_string())?;
        }
        self.refresh_library_events()?;
        self.select_saved_event(id)
    }

    fn show_library(&mut self, ui: &mut egui::Ui) {
        if !self.library_opened { return; }
        let mut choose_match = None;
        let mut choose_event = None;
        let mut refresh_events = false;
        let mut favorite_change = None;
        let mut add_tag = None;
        let mut remove_tag = None;
        let mut prepare_capture = None;
        ui.group(|ui| {
            ui.heading("Saved matches");
            if self.library_matches.is_empty() { ui.label("No imported matches in this database."); return; }
            egui::ComboBox::from_id_salt("saved_match").selected_text(
                self.library_matches.iter().find(|m| Some(m.id) == self.library_match_id)
                    .map(|m| format!("{} · {} · {} events", m.file_name, m.map_name, m.event_count))
                    .unwrap_or_else(|| "Select match".into()))
                .show_ui(ui, |ui| {
                    for m in &self.library_matches {
                        if ui.selectable_label(Some(m.id) == self.library_match_id,
                            format!("{} · {} · {} events", m.file_name, m.map_name, m.event_count)).clicked() {
                            choose_match = Some(m.id);
                        }
                    }
                });
            if let Some(m) = self.library_matches.iter().find(|m| Some(m.id) == self.library_match_id) {
                if m.effect_attribution_version == Some(2) {
                    ui.small(format!("Effect facts: {}; ambiguous: {}; unresolved: {}",
                        m.effect_fact_count, m.ambiguous_effect_count, m.unresolved_effect_count));
                } else if m.parser_capability.supports_combat() {
                    ui.colored_label(egui::Color32::YELLOW, "Older match: per-grenade effect totals are unverified and hidden.");
                }
                ui.small(format!("{} · {} · {}", m.file_path, m.imported_at, m.parser_capability.key()));
            }
            ui.horizontal(|ui| {
                ui.label("Find player, map, utility, or tag");
                ui.text_edit_singleline(&mut self.library_search);
                if ui.button("Search").clicked() { self.library_search_applied = self.library_search.clone(); self.library_page = 0; refresh_events = true; }
                if ui.checkbox(&mut self.library_favorites_only, "Favorites").changed() { self.library_page = 0; refresh_events = true; }
            });
            ui.horizontal(|ui| {
                if ui.add_enabled(self.library_page > 0, egui::Button::new("Previous")).clicked() {
                    self.library_page -= 1; refresh_events = true;
                }
                ui.label(format!("Page {} · {} shown", self.library_page + 1, self.library_events.len()));
                if ui.add_enabled(self.library_events.len() == 200, egui::Button::new("Next")).clicked() {
                    self.library_page += 1; refresh_events = true;
                }
            });
            egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                for event in &self.library_events {
                    let label = format!("{} Round {} · {} {} · tick {}{}",
                        if event.favorite { "★" } else { "☆" }, event.round_number,
                        event.thrower_name, event.utility_type.as_str(), event.throw_tick,
                        if event.tags.is_empty() { String::new() } else { format!(" · {}", event.tags.join(", ")) });
                    if ui.selectable_label(self.selected_event.as_ref().is_some_and(|d| d.summary.id == event.id), label).clicked() {
                        choose_event = Some(event.id);
                    }
                }
            });
            if let Some(detail) = &self.selected_event {
                let e = &detail.summary;
                ui.separator();
                ui.label(format!("{} · {} · round {} · tick {} · SteamID {}",
                    e.thrower_name, e.utility_type.as_str(), e.round_number, e.throw_tick, e.thrower_steamid));
                ui.label(format!("Throw ({:.1}, {:.1}, {:.1}); trajectory {} points; {} nearby players",
                    e.throw_pos.x, e.throw_pos.y, e.throw_pos.z, detail.trajectory.len(), detail.nearby_players.len()));
                if e.parser_capability == ParserCapabilities::GrenadeAnalysisOnlyV1 {
                    ui.small("Combat, flash effects, kills, assists, and economy unavailable. POV slot has not been capture-verified.");
                } else if e.parser_capability == ParserCapabilities::NativeCombatFlashV1 {
                    ui.small("Native grenade, combat, and flash events. Economy unavailable; POV slot has not been capture-verified.");
                    if e.effect_attribution_status != "complete" {
                        ui.colored_label(egui::Color32::YELLOW, format!("Effect attribution: {}. Raw facts are preserved; unavailable totals are not zero.", e.effect_attribution_status));
                    }
                    ui.label(format!("Enemy damage: {}; enemy flash duration: {}; kills: {}; flash assists: {}",
                        e.damage_enemy.map_or("unavailable".into(), |n| n.to_string()),
                        e.blind_enemy_duration.map_or("unavailable".into(), |n| format!("{n:.1}s")),
                        e.kills.map_or("unavailable".into(), |n| n.to_string()),
                        detail.flash_assists.map_or("unavailable".into(), |n| n.to_string())));
                }
                ui.horizontal(|ui| {
                    if ui.button(if e.favorite { "Remove favorite" } else { "Favorite" }).clicked() {
                        favorite_change = Some((e.id, !e.favorite));
                    }
                    ui.text_edit_singleline(&mut self.tag_draft);
                    if ui.button("Add tag").clicked() { add_tag = Some(e.id); }
                });
                ui.horizontal_wrapped(|ui| {
                    for tag in &e.tags {
                        if ui.small_button(format!("Remove {tag}")).clicked() { remove_tag = Some((e.id, tag.clone())); }
                    }
                });
                if ui.add_enabled(self.capture_request_messages.is_none(),
                    egui::Button::new("Prepare source-bound capture request")).clicked() {
                    prepare_capture = Some(e.id);
                }
            }
        });
        if let Some(id) = choose_match { self.library_match_id = Some(id); self.library_page = 0; refresh_events = true; }
        if refresh_events { if let Err(error) = self.refresh_library_events() { self.error = Some(error); } }
        if let Some(id) = choose_event { if let Err(error) = self.select_saved_event(id) { self.error = Some(error); } }
        if let Some(id) = prepare_capture {
            if let Some(path) = rfd::FileDialog::new().add_filter("DemoStrike capture request", &["json"])
                .set_file_name(format!("demostrike-event-{id}-capture-request.json")).save_file() {
                if let Err(error) = self.start_capture_request(id, &path) { self.error = Some(error); }
            }
        }
        if let Some((id, value)) = favorite_change {
            if let Err(error) = self.update_saved_event(id, Some(value), None, None) { self.error = Some(error); }
        }
        if let Some(id) = add_tag {
            let tag = self.tag_draft.clone();
            match self.update_saved_event(id, None, Some(&tag), None) {
                Ok(()) => self.tag_draft.clear(), Err(error) => self.error = Some(error),
            }
        }
        if let Some((id, tag)) = remove_tag {
            if let Err(error) = self.update_saved_event(id, None, None, Some(&tag)) { self.error = Some(error); }
        }
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
        if self.embedded_editor.is_some() {
            self.show_embedded_editor(ctx);
            return;
        }
        self.poll_demo_import();
        self.poll_capture_request();
        self.poll_package_import();
        ctx.set_visuals(egui::Visuals::dark());
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("DemoStrike");
                ui.label("Utility event analysis");
                if ui.add_enabled(self.import_messages.is_none(), egui::Button::new("Import CS2 demo (Rust analysis)" )).clicked() {
                    if let Some(path) = rfd::FileDialog::new().add_filter("CS2 demo", &["dem"]).pick_file() {
                        let _ = self.start_demo_import(&path);
                    }
                }
                if ui.add_enabled(self.import_messages.is_none(), egui::Button::new("Import with reference extractor")).clicked() {
                    if let Some(path) = rfd::FileDialog::new().add_filter("CS2 demo", &["dem"]).pick_file() {
                        let _ = self.start_reference_demo_import(&path);
                    }
                }
                if ui.button("Open saved matches").clicked() {
                    if let Err(error) = self.refresh_library() { self.error = Some(error); }
                }
                if ui.button("Open editable project").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("ClipStrike project", &["cstrike"]).pick_file() {
                        if let Err(error) = self.open_saved_project(&path) { self.error = Some(error); }
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
                if ui.add_enabled(self.package_import_messages.is_none(),
                    egui::Button::new("Import accepted capture package")).clicked() {
                    if let Some(package_dir) = rfd::FileDialog::new().pick_folder() {
                        if let Some(project_path) = rfd::FileDialog::new()
                            .add_filter("ClipStrike project", &["cstrike"])
                            .set_file_name("captured-event.cstrike").save_file() {
                            if let Err(error) = self.start_package_import(&package_dir, &project_path) {
                                self.error = Some(error);
                            }
                        }
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
                ui.small("Rust import uses source2-demo 0.5.9 and validates grenade, combat, and flash event schemas before saving. Unsupported schemas fail with an error. Economy and capture facts remain unavailable. Optional reference extractor settings follow.");
                ui.horizontal(|ui| {
                    ui.label("Node runtime");
                    ui.text_edit_singleline(&mut self.node_path);
                    if ui.button("Browse").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_file() { self.node_path = path.display().to_string(); }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("DemoStrike reference extractor script");
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
            if let Some(status) = &self.capture_request_status { ui.label(status); }
            if self.capture_request_messages.is_some() { ctx.request_repaint_after(Duration::from_millis(100)); }
            if self.package_import_messages.is_some() {
                ui.label("Validating accepted package and creating editable project...");
                if ui.add_enabled(!self.package_import_cancel.load(Ordering::Relaxed),
                    egui::Button::new("Cancel package import")).clicked() {
                    self.package_import_cancel.store(true, Ordering::Relaxed);
                }
                ctx.request_repaint_after(Duration::from_millis(100));
            }
            if let Some(path) = self.imported_project_path.clone() {
                ui.colored_label(egui::Color32::LIGHT_GREEN,
                    format!("Editable project created: {}", path.display()));
                if let Some(summary) = &self.package_import_summary { ui.small(summary.as_str()); }
                if ui.button("Edit imported project").clicked() {
                    if let Err(error) = self.open_saved_project(&path) { self.error = Some(error); }
                }
            }
            if self.import_messages.is_some() {
                if self.import_stage.as_deref() == Some("Parsing demo") {
                    if let Some((tick, total)) = self.import_cancel.parsing_progress() {
                        ui.add(egui::ProgressBar::new(tick as f32 / total as f32)
                            .text(format!("Decoded tick {tick} / {total}")));
                    }
                }
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
            self.show_library(ui);
            ui.separator();
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
                ui.label("Open a CS2 demo for local grenade, combat, and flash event analysis. Unsupported schemas fail before saving; economy and game capture are separate.");
                return;
            };
            if let Some(path) = &self.source_label {
                ui.small(path);
            }
            if self.capabilities == Some(ParserCapabilities::GrenadeAnalysisOnlyV1) {
                ui.colored_label(egui::Color32::YELLOW, "Grenade analysis only: combat damage, blinds, kills, assists, and economy are unavailable. These metrics are not zero-valued results.");
            } else if self.capabilities == Some(ParserCapabilities::NativeCombatFlashV1) {
                ui.colored_label(egui::Color32::YELLOW, "Native grenade, combat, and flash facts: economy and verified POV capture remain unavailable.");
            }
            let ambiguous = analyzed.effect_facts.iter().filter(|fact| fact.resolution == "ambiguous").count();
            let unresolved = analyzed.effect_facts.iter().filter(|fact| fact.resolution == "unresolved").count();
            ui.small(format!("Effect facts: {}; ambiguous: {ambiguous}; unresolved: {unresolved}", analyzed.effect_facts.len()));
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
                        if self.capabilities.is_some_and(ParserCapabilities::supports_combat) {
                            ui.label(format!("Enemy damage: {}; enemy flash duration: {}; kills: {}; flash assists: {}",
                                event.damage_attribution_complete.then(|| event.damage_enemy.to_string()).unwrap_or("unavailable".into()),
                                event.blind_attribution_complete.then(|| format!("{:.1}s", event.blind_enemy_duration)).unwrap_or("unavailable".into()),
                                event.kill_attribution_complete.then(|| event.kills.to_string()).unwrap_or("unavailable".into()),
                                event.assist_attribution_complete.then(|| event.flash_assists.to_string()).unwrap_or("unavailable".into())));
                            if !event.blind_attribution_complete || !event.damage_attribution_complete ||
                                !event.kill_attribution_complete || !event.assist_attribution_complete {
                                ui.small("Overlapping or unresolved effects: raw facts retained; affected totals unavailable.");
                            }
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
        assert_eq!(app.capabilities, Some(ParserCapabilities::NativeCombatFlashV1));
        assert_eq!(app.analyzed.as_ref().unwrap().events.len(), 37);
        let matches = crate::db::queries::list_matches(&crate::db::open(Path::new(&app.database_path)).unwrap()).unwrap();
        assert_eq!(matches[0].parser_capability, ParserCapabilities::NativeCombatFlashV1);
        assert_eq!(app.library_matches.len(), 1);
        assert_eq!(app.library_events.len(), 37);
        let event_id = app.library_events[0].id;
        app.update_saved_event(event_id, Some(true), Some("shortlist"), None).unwrap();
        app.refresh_library().unwrap();
        app.select_saved_event(event_id).unwrap();
        assert!(app.selected_event.as_ref().unwrap().summary.favorite);
        assert_eq!(app.selected_event.as_ref().unwrap().summary.tags, ["shortlist"]);
        app.start_demo_import(&demo).unwrap();
        for _ in 0..200 {
            app.poll_demo_import();
            if app.import_messages.is_none() { break }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(app.error.as_ref().unwrap().contains("already imported"));
        app.select_saved_event(event_id).unwrap();
        assert!(app.selected_event.as_ref().unwrap().summary.favorite);
        assert_eq!(app.selected_event.as_ref().unwrap().summary.tags, ["shortlist"]);
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
