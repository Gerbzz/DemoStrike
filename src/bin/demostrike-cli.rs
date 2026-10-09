use std::path::Path;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [command, demo_path, database] if command == "import-grenades" => {
            let parser = demostrike::native_grenade_parser::NativeGrenadeParser;
            let control = demostrike::demo_import::ImportControl::new();
            let imported = demostrike::demo_import::import_demo(
                Path::new(demo_path), Path::new(database), &parser, &control, |_| {}
            )?;
            println!("match {}: {} grenade events on {}; capability={} (combat and economy unavailable)",
                imported.match_id, imported.analyzed.events.len(), imported.analyzed.map_name,
                imported.capabilities.key());
            Ok(())
        }
        [command, demo_path] if command == "inspect-demo" => {
            let path = Path::new(demo_path);
            demostrike::demo_import::validate_demo_file(path)?;
            let bytes = std::fs::metadata(path).map_err(|error| error.to_string())?.len();
            println!("{}: CS2 PBDEMS2 header, {bytes} bytes; events not parsed", path.display());
            Ok(())
        }
        [command, raw_path] if command == "analyze" => {
            let path = Path::new(raw_path);
            let bytes = std::fs::read(path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            let analyzed = demostrike::analyze_raw_json(&bytes)
                .map_err(|error| error.to_string())?;
            println!(
                "{}: {} utility events, {} rounds, {} players, {:.1} ticks/s",
                analyzed.map_name,
                analyzed.events.len(),
                analyzed.rounds.len(),
                analyzed.players.len(),
                analyzed.tick_rate
            );
            Ok(())
        }
        [command, draft_path, destination] if command == "package" => {
            let draft = demostrike::package::load_draft(Path::new(draft_path))
                .map_err(|error| error.to_string())?;
            let package = demostrike::package::finalize_stream_probed_capture_package(
                Path::new(destination), &draft.package_id, &draft.inputs,
            ).map_err(|error| error.to_string())?;
            println!("{}: {} separate stream-probed clips; audible sound and HLAE origin still require review", package.root.display(), package.manifest.clips.len());
            Ok(())
        }
        [command, draft_path, destination] if command == "package-structural" => {
            let draft = demostrike::package::load_draft(Path::new(draft_path))
                .map_err(|error| error.to_string())?;
            let package = demostrike::package::finalize_capture_package(
                Path::new(destination), &draft.package_id, &draft.inputs,
            ).map_err(|error| error.to_string())?;
            println!("{}: {} separate clips; STRUCTURAL ONLY, media and audio unverified", package.root.display(), package.manifest.clips.len());
            Ok(())
        }
        [command, video, wav] if command == "probe-pair" => {
            let (picture, sound) = demostrike::media_probe::probe_capture_pair(Path::new(video), Path::new(wav))
                .map_err(|error| error.to_string())?;
            println!("video {}x{} {}/{} fps {:.3}s; WAV {} channels {} Hz {:.3}s; stream headers only, sound not verified",
                picture.width, picture.height, picture.fps_num, picture.fps_den, picture.duration_seconds,
                sound.channels, sound.sample_rate, sound.duration_seconds);
            Ok(())
        }
        [command, destination] if command == "recover" => {
            let package = demostrike::package::recover_complete_staged_package(Path::new(destination))
                .map_err(|error| error.to_string())?;
            println!("recovered {}: {} clips", package.root.display(), package.manifest.clips.len());
            Ok(())
        }
        _ => Err("usage: demostrike-cli import-grenades <validated-demo.dem> <matches.sqlite> | inspect-demo <demo.dem> | analyze <raw-demo-facts.json> | package <draft.json> <new-package-folder> | package-structural <draft.json> <new-package-folder> | probe-pair <video.mp4> <audio.wav> | recover <package-folder>".into()),
    }
}
