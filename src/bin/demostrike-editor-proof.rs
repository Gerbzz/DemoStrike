use clipstrike_core::editor::EditorSession;
use clipstrike_core::project::Project;
use clipstrike_media::export::{ExportFormat, ExportSettings, VideoCodec};
use clipstrike_ui::ui::ClipStrikeApp;
use std::path::Path;

struct ProofLogger;
static LOGGER: ProofLogger = ProofLogger;
impl log::Log for ProofLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info && metadata.target().starts_with("clipstrike")
    }
    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) { eprintln!("{}: {}", record.target(), record.args()); }
    }
    fn flush(&self) {}
}

fn main() {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Info);
    if let Err(message) = run() {
        eprintln!("DemoStrike editor proof: {message}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [command, source, destination] if command == "edit" => {
            edit(Path::new(source), Path::new(destination))
        }
        [command, source, destination] if command == "chronological" => {
            chronological(Path::new(source), Path::new(destination))
        }
        [command, source, destination] if command == "chronological-validated" => {
            chronological_validated(Path::new(source), Path::new(destination))
        }
        [command, project, output] if command == "export" => {
            export(Path::new(project), Path::new(output))
        }
        [command, project, output] if command == "export-validated" => {
            export_validated(Path::new(project), Path::new(output))
        }
        _ => Err("Usage: demostrike-editor-proof edit <accepted.cstrike> <new.cstrike> | chronological <edited.cstrike> <new.cstrike> | chronological-validated <video-limited.cstrike> <new.cstrike> | export <edited.cstrike> <new.mp4> | export-validated <video-limited-chronological.cstrike> <new.mp4>".into()),
    }
}

fn loaded(path: &Path) -> Result<Project, String> {
    let (project, warnings) = Project::load_with_capture_warnings(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if !warnings.is_empty() {
        return Err(format!("capture media validation failed: {}", warnings.join("; ")));
    }
    check_sources(&project)?;
    Ok(project)
}

fn check_sources(project: &Project) -> Result<(), String> {
    let expected = [
        ("player_pov", 5260, 5518, 4028_i64),
        ("projectile", 5484, 5660, 2740),
        ("third_person_reference", 5355, 5518, 2531),
    ];
    if project.capture_sources.len() != 3 || project.assets.len() != 3 || !(2..=3).contains(&project.clips.len()) {
        return Err("expected three accepted source clips and assets, with two or three timeline clips".into());
    }
    let asset_durations: Vec<_> = project.assets.iter().map(|asset| asset.duration_ms).collect();
    if asset_durations != [4028, 2740, 2531] && asset_durations != [4016, 2700, 2500] {
        return Err(format!("unexpected accepted-media editor duration limits: {asset_durations:?}"));
    }
    for (index, (role, first_tick, last_tick, duration)) in expected.iter().enumerate() {
        let source = &project.capture_sources[index];
        let media = &project.assets[index];
        let clip = project.clips.iter().find(|clip| clip.id == index + 1);
        if source.asset_id != index + 1 || media.id != index + 1 || clip.is_some_and(|clip| clip.asset_id != media.id) || source.clip.camera_role != *role
            || source.clip.start_tick != *first_tick || source.clip.end_tick != *last_tick
            || source.clip.duration_ms as i64 != *duration
            || source.clip.sha256.len() != 64
            || !media.uri.starts_with("file://")
        {
            return Err(format!("accepted source {} did not retain expected identity, ticks, media URI, or duration", index + 1));
        }
    }
    Ok(())
}

fn timeline(project: &Project) -> Vec<(usize, i64, i64, i64, i64)> {
    project.clips.iter().map(|c| (c.id, c.start, c.end, c.in_point, c.out_point)).collect()
}

fn original_timeline() -> Vec<(usize, i64, i64, i64, i64)> {
    vec![(1, 0, 4028, 0, 4028), (2, 4028, 6768, 0, 2740), (3, 6768, 9299, 0, 2531)]
}

fn edited_timeline() -> Vec<(usize, i64, i64, i64, i64)> {
    vec![(1, 0, 4028, 0, 4028), (2, 4028, 6767, 0, 2739), (3, 6767, 7697, 1600, 2530)]
}

fn chronological_timeline() -> Vec<(usize, i64, i64, i64, i64)> {
    vec![(1, 0, 3500, 0, 3500), (2, 3500, 6239, 0, 2739)]
}

fn validated_import_timeline() -> Vec<(usize, i64, i64, i64, i64)> {
    vec![(1, 0, 4016, 0, 4016), (2, 4016, 6716, 0, 2700), (3, 6716, 9216, 0, 2500)]
}

fn validated_chronological_timeline() -> Vec<(usize, i64, i64, i64, i64)> {
    vec![(1, 0, 3500, 0, 3500), (2, 3500, 6200, 0, 2700)]
}

fn ensure_timeline(project: &Project, expected: &[(usize, i64, i64, i64, i64)]) -> Result<(), String> {
    let actual = timeline(project);
    if actual != expected {
        return Err(format!("unexpected editor timeline: {actual:?}; expected {expected:?}"));
    }
    Ok(())
}

fn ensure_links_match(original: &Project, edited: &Project) -> Result<(), String> {
    let sources_before = serde_json::to_value(&original.capture_sources).map_err(|e| e.to_string())?;
    let sources_after = serde_json::to_value(&edited.capture_sources).map_err(|e| e.to_string())?;
    if sources_before != sources_after {
        return Err("capture source tick ranges or package evidence changed during edit".into());
    }
    let media_before = serde_json::to_value(&original.assets).map_err(|e| e.to_string())?;
    let media_after = serde_json::to_value(&edited.assets).map_err(|e| e.to_string())?;
    if media_before != media_after {
        return Err("media references changed during edit".into());
    }
    Ok(())
}

fn edit(input: &Path, output: &Path) -> Result<(), String> {
    if output.exists() {
        return Err(format!("refusing to overwrite {}", output.display()));
    }
    let original = loaded(input)?;
    ensure_timeline(&original, &original_timeline())?;
    let mut session = EditorSession::open_from_path(input)?;
    ensure_links_match(&original, &session.snapshot_project())?;

    // Remove the first 1.6 seconds of the mostly static third-person reference,
    // then ripple that clip left to close the gap. Both commands use the shared
    // editor undo stack and retain original capture provenance.
    session.trim_clip(2, 4028, 6767)?;
    session.trim_clip(3, 8368, 9298)?;
    session.move_clip(3, 6767, 7697)?;
    let edited = session.snapshot_project();
    ensure_timeline(&edited, &edited_timeline())?;
    ensure_links_match(&original, &edited)?;

    session.undo();
    session.undo();
    session.undo();
    ensure_timeline(&session.snapshot_project(), &original_timeline())?;
    session.redo();
    session.redo();
    session.redo();
    ensure_timeline(&session.snapshot_project(), &edited_timeline())?;

    session.save_to_path(output)?;
    let reopened_session = EditorSession::open_from_path(output)?;
    let reopened = loaded(output)?;
    ensure_timeline(&reopened_session.snapshot_project(), &edited_timeline())?;
    ensure_timeline(&reopened, &edited_timeline())?;
    ensure_links_match(&original, &reopened)?;
    println!("reopened edit: source ticks and media links intact; trim 1 ms from clip 2, 1601 ms from clip 3; 3 clips span 0..7697 ms; undo/redo passed; {}", output.display());
    Ok(())
}

fn chronological(input: &Path, output: &Path) -> Result<(), String> {
    if output.exists() { return Err(format!("refusing to overwrite {}", output.display())); }
    let original = loaded(input)?;
    ensure_timeline(&original, &edited_timeline())?;
    let mut session = EditorSession::open_from_path(input)?;
    session.trim_clip(1, 0, 3500)?;
    session.move_clip(2, 3500, 6239)?;
    session.remove_clip(3)?;
    ensure_timeline(&session.snapshot_project(), &chronological_timeline())?;
    session.undo(); session.undo(); session.undo();
    ensure_timeline(&session.snapshot_project(), &edited_timeline())?;
    session.redo(); session.redo(); session.redo();
    session.save_to_path(output)?;
    let reopened = EditorSession::open_from_path(output)?.snapshot_project();
    ensure_timeline(&reopened, &chronological_timeline())?;
    ensure_links_match(&original, &reopened)?;
    loaded(output)?;
    println!("chronological edit reopened: 0..3500 ms POV (~ticks 5260..5484), 3500..6239 ms projectile (ticks 5484..5660); all 3 source references retained; {}", output.display());
    Ok(())
}

fn chronological_validated(input: &Path, output: &Path) -> Result<(), String> {
    if output.exists() { return Err(format!("refusing to overwrite {}", output.display())); }
    let original = loaded(input)?;
    ensure_timeline(&original, &validated_import_timeline())?;
    let mut session = EditorSession::open_from_path(input)?;
    session.trim_clip(1, 0, 3500)?;
    session.move_clip(2, 3500, 6200)?;
    session.remove_clip(3)?;
    ensure_timeline(&session.snapshot_project(), &validated_chronological_timeline())?;
    session.undo(); session.undo(); session.undo();
    ensure_timeline(&session.snapshot_project(), &validated_import_timeline())?;
    session.redo(); session.redo(); session.redo();
    ensure_timeline(&session.snapshot_project(), &validated_chronological_timeline())?;
    session.save_to_path(output)?;
    let reopened = EditorSession::open_from_path(output)?.snapshot_project();
    ensure_timeline(&reopened, &validated_chronological_timeline())?;
    ensure_links_match(&original, &reopened)?;
    loaded(output)?;
    println!("validated chronological edit reopened: POV 0..3500 ms, projectile 3500..6200 ms (full 2700 ms video); original accepted source facts retained; {}", output.display());
    Ok(())
}

fn export(project_path: &Path, output: &Path) -> Result<(), String> {
    export_expected(project_path, output, &chronological_timeline())
}

fn export_validated(project_path: &Path, output: &Path) -> Result<(), String> {
    export_expected(project_path, output, &validated_chronological_timeline())
}

fn export_expected(project_path: &Path, output: &Path, expected: &[(usize, i64, i64, i64, i64)]) -> Result<(), String> {
    if output.exists() {
        return Err(format!("refusing to overwrite {}", output.display()));
    }
    let project = loaded(project_path)?;
    ensure_timeline(&project, expected)?;
    let _editor = EditorSession::open_from_path(project_path)?;
    let settings = ExportSettings {
        format: ExportFormat::MP4,
        codec: VideoCodec::H264,
        width: 1280,
        height: 720,
        fps_num: 30,
        fps_den: 1,
        bitrate_kbps: 5000,
        use_hardware_acceleration: false,
    };
    ClipStrikeApp::export_saved_project_offscreen(project_path, output, settings)?;
    println!("shared compositor export complete: {}", output.display());
    Ok(())
}






