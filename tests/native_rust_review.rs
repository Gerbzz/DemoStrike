//! Hosted Windows proof of the actual Rust egui/WGPU match/round view.
//! Runs only against the separately imported, hash-pinned public demo database.
#![cfg(all(windows, feature = "native-ui"))]
use demostrike::match_review;
use demostrike::native_replay::{PlaybackClock, ReplayRound};
use demostrike::owned_capture::{OwnedLaunch, run_owned};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

fn sha(path: &Path) -> String {
    use std::io::Read;
    let mut input = std::fs::File::open(path).unwrap();
    let mut hasher = Sha256::new();
    let mut bytes = [0; 65536];
    loop {
        let n = input.read(&mut bytes).unwrap();
        if n == 0 {
            break;
        }
        hasher.update(&bytes[..n]);
    }
    format!("{:x}", hasher.finalize())
}
fn launch(
    database: &Path,
    radar: &Path,
    output: &Path,
    receipt: &Path,
    round: i64,
) -> Result<(), String> {
    let program = PathBuf::from(env!("CARGO_BIN_EXE_demostrike"));
    let args = vec![
        "--review-library".into(),
        database.as_os_str().to_owned(),
        "--review-match".into(),
        "1".into(),
        "--review-round".into(),
        round.to_string().into(),
        "--review-radar".into(),
        radar.as_os_str().to_owned(),
        "--capture".into(),
        output.as_os_str().to_owned(),
        "--review-capture-receipt".into(),
        receipt.as_os_str().to_owned(),
    ];
    run_owned(
        OwnedLaunch {
            current_dir: program.parent().unwrap().to_owned(),
            program,
            args,
            environment: vec![],
        },
        &AtomicBool::new(false),
        Duration::from_secs(60),
        |progress| eprintln!("{progress}"),
    )
}
#[test]
#[ignore = "requires the hash-pinned public real-demo import and a genuine Windows desktop/WGPU renderer"]
fn actual_native_rust_viewer_paints_real_rounds_without_webview() {
    let database = PathBuf::from(
        std::env::var_os("DEMOSTRIKE_RUST_REVIEW_DATABASE").expect("pinned imported database"),
    );
    let radar =
        PathBuf::from(std::env::var_os("DEMOSTRIKE_RUST_REVIEW_RADAR").expect("pinned radar PNG"));
    let root = PathBuf::from(
        std::env::var_os("DEMOSTRIKE_RUST_REVIEW_EVIDENCE").expect("fresh evidence destination"),
    );
    std::fs::create_dir(&root).expect("evidence directory must not preexist");
    let database_hash = sha(&database);
    let headless_path = PathBuf::from(
        std::env::var_os("DEMOSTRIKE_RUST_REVIEW_HEADLESS_PROOF")
            .expect("headless source/database proof"),
    );
    let headless: serde_json::Value =
        serde_json::from_slice(&std::fs::read(headless_path).unwrap()).unwrap();
    assert_eq!(headless["result"], "pass");
    assert_eq!(headless["database"]["sha256"], database_hash);
    assert_eq!(
        headless["source"]["sha256"],
        "84a1a4191302bdd2a3bbb5a727842093744b1fb1a228aeec630369e44b622cb2"
    );
    if let Ok(commit) = std::env::var("GITHUB_SHA") {
        assert_eq!(headless["build"]["declaredCommit"], commit);
    }
    let snapshot = match_review::list_matches(database.to_str().unwrap()).unwrap();
    let detail = match_review::get_match_detail(database.to_str().unwrap(), 1).unwrap();
    assert_eq!(snapshot.matches.len(), 1);
    assert_eq!(
        detail.summary.source_sha256.as_deref(),
        Some("84a1a4191302bdd2a3bbb5a727842093744b1fb1a228aeec630369e44b622cb2")
    );
    for (round, events) in [(1, 31), (3, 61)] {
        let output = root.join(format!("mirage-round-{round}-native-rust.png"));
        let receipt = root.join(format!("mirage-round-{round}-native-rust.json"));
        launch(&database, &radar, &output, &receipt, round)
            .expect("owned native Rust renderer must exit successfully within60s");
        let proof: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&receipt).unwrap()).unwrap();
        let paint = &proof["paint"];
        assert_eq!(
            proof["build"]["executable_sha256"],
            sha(Path::new(env!("CARGO_BIN_EXE_demostrike")))
        );
        if let Ok(commit) = std::env::var("GITHUB_SHA") {
            assert_eq!(proof["build"]["declared_commit"], commit);
        }
        assert_eq!(proof["schema"], "demostrike-native-rust-review-capture-v1");
        assert_eq!(proof["renderer"], "native-rust-egui-wgpu");
        assert_eq!(proof["database_sha256"], database_hash);
        assert_eq!(proof["png_sha256"], sha(&output));
        assert_eq!(paint["match_id"], 1);
        assert_eq!(paint["round"], round);
        assert_eq!(paint["map"], "de_mirage");
        assert_eq!(
            paint["source_sha256"],
            detail.summary.source_sha256.as_deref().unwrap()
        );
        assert_eq!(paint["radar_loaded"], true);
        assert!(paint["viewer_paint_frames"].as_u64().unwrap() >= 2);
        assert_eq!(paint["recorded_events"], events);
        let meta = match_review::get_viewer_round(database.to_str().unwrap(), 1, round).unwrap();
        let bounds = detail
            .rounds
            .iter()
            .find(|r| r.round_number == round)
            .unwrap();
        let mut clock = PlaybackClock::new(
            meta.start_tick.unwrap(),
            meta.end_tick.unwrap(),
            meta.tick_rate.unwrap(),
        )
        .unwrap();
        if let Some(tick) = bounds.freeze_end_tick {
            clock.seek(tick);
        }
        let tick = clock.current_tick();
        assert_eq!(paint["tick"], tick);
        let frame =
            match_review::get_viewer_frames(database.to_str().unwrap(), 1, round, tick, tick)
                .unwrap();
        assert!(
            !frame.players.is_empty(),
            "real scene must contain observed source players"
        );
        assert_eq!(paint["observed_players"], frame.players.len());
        assert_eq!(paint["observed_grenades"], frame.grenades.len());
        // Reconstruct expected event count from typed source, not a fixture screenshot label.
        let mut chunks = Vec::new();
        let mut start = meta.start_tick.unwrap();
        let end = meta.end_tick.unwrap();
        while start <= end {
            let last = (start + 511).min(end);
            chunks.push(
                match_review::get_viewer_frames(database.to_str().unwrap(), 1, round, start, last)
                    .unwrap(),
            );
            if last == end {
                break;
            }
            start = last + 1;
        }
        assert_eq!(
            ReplayRound::new(meta, chunks, Some(bounds))
                .unwrap()
                .events
                .len(),
            events as usize
        );
        let png = std::fs::read(&output).unwrap();
        assert!(png.len() > 10000 && png.len() < 5 * 1024 * 1024);
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let image = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
            .unwrap()
            .to_rgba8();
        assert!(image.width() >= 1400 && image.height() >= 850);
        let mut colors = std::collections::HashSet::new();
        for y in (40..image.height()).step_by(37) {
            for x in (40..image.width()).step_by(43) {
                colors.insert(image.get_pixel(x, y).0);
            }
        }
        assert!(
            colors.len() > 40,
            "native radar/roster/events must paint actual detail"
        );
        let old_receipt = std::fs::read(&receipt).unwrap();
        assert!(
            launch(&database, &radar, &output, &receipt, round).is_err(),
            "existing proof must fail rather than overwrite"
        );
        assert_eq!(std::fs::read(&output).unwrap(), png);
        assert_eq!(std::fs::read(&receipt).unwrap(), old_receipt);
        eprintln!(
            "NATIVE_RUST_REVIEW round={round} events={events} tick={tick} observed_players={} PNG={}x{} colors={} sha={}",
            frame.players.len(),
            image.width(),
            image.height(),
            colors.len(),
            sha(&output)
        );
    }
    assert_eq!(
        sha(&database),
        database_hash,
        "native review may not mutate imported facts"
    );
}
