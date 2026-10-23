//! Headless native Rust parser/reader/replay acceptance for one pinned fixture.
//! This does not create or approve a window, GPU rendering, footage or audio.
use demostrike::demo_import::{ImportControl, import_demo};
use demostrike::match_review::{self, LibraryError, RoundSummary};
use demostrike::native_grenade_parser::NativeGrenadeParser;
use demostrike::native_replay::{
    BombPhase, BombState, PlaybackClock, ReplayEventKind, ReplayRound, SamplePresence,
};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const FIXTURE_BYTES: u64 = 60_601_900;
const FIXTURE_SHA256: &str = "84a1a4191302bdd2a3bbb5a727842093744b1fb1a228aeec630369e44b622cb2";
const ROUND_STARTS: [i64; 10] = [
    67, 9419, 15680, 21905, 26512, 31645, 39268, 45540, 52166, 56407,
];
const USAGE: &str = "usage: demostrike-rust-review-proof --demo ABSOLUTE.dem --database NEW_ABSOLUTE.sqlite --proof NEW_ABSOLUTE.json";

#[derive(Debug)]
struct ProofPlan {
    demo: PathBuf,
    database: PathBuf,
    proof: PathBuf,
}

fn require(condition: bool, message: impl Into<String>) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn new_output_path(path: &Path, extension: &str) -> Result<PathBuf, String> {
    require(path.is_absolute(), "proof outputs must be absolute paths")?;
    require(
        path.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case(extension)),
        format!("expected a .{extension} output"),
    )?;
    let name = path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or("output needs a Unicode file name")?;
    require(
        !name.contains([':', '/', '\\', '<', '>', '"', '|', '?', '*'])
            && !name.chars().any(char::is_control)
            && !name.ends_with([' ', '.']),
        "output names cannot use Windows streams, reserved characters or aliases",
    )?;
    let stem = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches([' ', '.'])
        .to_uppercase();
    let device = matches!(
        stem.as_str(),
        "CON" | "NUL" | "AUX" | "PRN" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        stem.strip_prefix(prefix).is_some_and(|n| {
            matches!(
                n,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    });
    require(!device, "output name is a reserved Windows device")?;
    let parent = path
        .parent()
        .ok_or("output needs an existing parent folder")?
        .canonicalize()
        .map_err(|e| format!("cannot resolve output parent: {e}"))?;
    require(parent.is_dir(), "output parent is not a folder")?;
    let output = parent.join(name);
    match std::fs::symlink_metadata(&output) {
        Ok(_) => Err(format!(
            "output already exists; nothing will be overwritten: {}",
            output.display()
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(output),
        Err(e) => Err(format!("cannot inspect output: {e}")),
    }
}

fn parse_args(args: &[OsString]) -> Result<ProofPlan, String> {
    require(args.len() == 6, USAGE)?;
    let (mut demo, mut database, mut proof) = (None, None, None);
    for pair in args.chunks_exact(2) {
        let target = if pair[0] == "--demo" {
            &mut demo
        } else if pair[0] == "--database" {
            &mut database
        } else if pair[0] == "--proof" {
            &mut proof
        } else {
            return Err(USAGE.into());
        };
        require(target.is_none(), "duplicate proof argument")?;
        *target = Some(PathBuf::from(&pair[1]));
    }
    let demo = demo.ok_or(USAGE)?;
    require(
        demo.is_absolute() && demo.is_file(),
        "demo must be an existing absolute file",
    )?;
    require(
        demo.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("dem")),
        "expected a .dem source",
    )?;
    let demo = demo
        .canonicalize()
        .map_err(|e| format!("cannot resolve demo: {e}"))?;
    let database = new_output_path(&database.ok_or(USAGE)?, "sqlite")?;
    let proof = new_output_path(&proof.ok_or(USAGE)?, "json")?;
    require(
        database != proof && database != demo && proof != demo,
        "source and outputs must be distinct",
    )?;
    Ok(ProofPlan {
        demo,
        database,
        proof,
    })
}

fn file_hash(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
fn verify_fixture(path: &Path) -> Result<(), String> {
    require(
        std::fs::metadata(path).map_err(|e| e.to_string())?.len() == FIXTURE_BYTES,
        "pinned Mirage fixture byte length differs",
    )?;
    require(
        file_hash(path)? == FIXTURE_SHA256,
        "pinned Mirage fixture SHA-256 differs",
    )
}
fn digest_record<T: Serialize>(digest: &mut Sha256, record: &T) -> Result<(), String> {
    digest.update(serde_json::to_vec(record).map_err(|e| e.to_string())?);
    digest.update(b"\n");
    Ok(())
}
fn bomb_value(bomb: BombState<'_>) -> Value {
    match bomb {
        BombState::Unavailable => json!({"state":"unavailable"}),
        BombState::NoObservedPosition => {
            json!({"state":"no_observed_position","carrier":"unavailable"})
        }
        BombState::Observed(bomb) => {
            json!({"state":"observed","plantTick":bomb.plant_tick,"sourceIndex":bomb.source_index,"position":bomb.position,"phase":match bomb.phase {BombPhase::Planted=>"planted",BombPhase::Defused=>"defused",BombPhase::Exploded=>"exploded"}})
        }
    }
}

fn prove_round(database: &Path, match_id: i64, round: &RoundSummary) -> Result<Value, String> {
    let path = database
        .to_str()
        .ok_or("proof database path must be Unicode")?;
    let meta = match_review::get_viewer_round(path, match_id, round.round_number)
        .map_err(|e| e.to_string())?;
    require(
        meta.start_tick == round.start_tick && meta.end_tick == round.end_tick,
        "round/replay bounds differ",
    )?;
    require(
        meta.map_name == "de_mirage" && meta.tick_rate == Some(64.0),
        "pinned replay map/rate differ",
    )?;
    let start = meta.start_tick.ok_or("missing recorded start tick")?;
    let end = meta.end_tick.ok_or("missing recorded end tick")?;
    let mut chunks = Vec::new();
    let mut player_count = 0;
    let mut grenade_count = 0;
    let (mut direct_players, mut direct_grenades) = (Sha256::new(), Sha256::new());
    if meta.positions_available {
        let mut first = start;
        while first <= end {
            let last = first.saturating_add(511).min(end);
            let chunk =
                match_review::get_viewer_frames(path, match_id, round.round_number, first, last)
                    .map_err(|e| e.to_string())?;
            for player in &chunk.players {
                digest_record(&mut direct_players, player)?;
            }
            for grenade in &chunk.grenades {
                digest_record(&mut direct_grenades, grenade)?;
            }
            player_count += chunk.players.len();
            grenade_count += chunk.grenades.len();
            chunks.push(chunk);
            first = last.checked_add(1).ok_or("tick overflow")?;
        }
    }
    let direct_player_digest = format!("{:x}", direct_players.finalize());
    let direct_grenade_digest = format!("{:x}", direct_grenades.finalize());
    require(
        player_count as i64 <= meta.player_sample_count
            && grenade_count as i64 <= meta.grenade_sample_count,
        "bounded replay invented sample rows",
    )?;
    let replay = ReplayRound::new(meta, chunks, Some(round)).map_err(|e| e.to_string())?;
    let (mut indexed_players, mut indexed_grenades) = (Sha256::new(), Sha256::new());
    let (mut indexed_player_count, mut indexed_grenade_count, mut missing_player_ticks) =
        (0_usize, 0_usize, 0_usize);
    for tick in start..=end {
        let scene = replay.scene(tick);
        for player in scene.players {
            digest_record(&mut indexed_players, player)?;
        }
        for grenade in scene.grenades {
            digest_record(&mut indexed_grenades, grenade)?;
        }
        indexed_player_count += scene.players.len();
        indexed_grenade_count += scene.grenades.len();
        if scene.players.is_empty() {
            missing_player_ticks += 1;
        }
        let expected = if !replay.meta.positions_available {
            SamplePresence::Unavailable
        } else if scene.players.is_empty() {
            SamplePresence::MissingAtTick
        } else {
            SamplePresence::Observed
        };
        require(
            scene.player_presence == expected,
            "scene fabricated a player observation",
        )?;
    }
    let indexed_player_digest = format!("{:x}", indexed_players.finalize());
    let indexed_grenade_digest = format!("{:x}", indexed_grenades.finalize());
    require(
        player_count == indexed_player_count
            && grenade_count == indexed_grenade_count
            && direct_player_digest == indexed_player_digest
            && direct_grenade_digest == indexed_grenade_digest,
        "indexed Rust replay differs from exact ordered SQLite samples",
    )?;
    for tick in [start - 1, end + 1] {
        let outside = replay.scene(tick);
        require(
            outside.player_presence == SamplePresence::OutsideRound
                && outside.players.is_empty()
                && outside.grenades.is_empty(),
            "replay extrapolated outside recorded round",
        )?;
    }
    if replay.meta.positions_available {
        for (first, last) in [
            (start - 1, start),
            (end, end + 1),
            (end, start),
            (start, start + 512),
        ] {
            if last > end || first < start || last < first || last - first > 511 {
                require(
                    matches!(
                        match_review::get_viewer_frames(
                            path,
                            match_id,
                            round.round_number,
                            first,
                            last
                        ),
                        Err(LibraryError::InvalidRequest(_))
                    ),
                    "native reader accepted invalid sample window",
                )?;
            }
        }
    }
    let mut clock = PlaybackClock::new(start, end, 64.0).map_err(|e| e.to_string())?;
    clock.play();
    clock.advance(Duration::from_secs(1));
    let one_x = clock.current_tick();
    require(
        one_x == (start + 64).min(end),
        "native 1x playback is not64source ticks/second",
    )?;
    clock.pause();
    clock.advance(Duration::from_secs(1));
    require(clock.current_tick() == one_x, "paused source tick moved")?;
    clock.seek(start);
    clock.set_speed(1.5).map_err(|e| e.to_string())?;
    clock.play();
    clock.advance(Duration::from_secs(1));
    let fast = clock.current_tick();
    require(
        fast == (start + 96).min(end),
        "native1.5x playback is not96source ticks/second",
    )?;
    clock.seek(end - 1);
    clock.play();
    let terminal = clock.advance(Duration::from_secs(1));
    require(
        terminal.reached_end && clock.current_tick() == end && !clock.is_playing(),
        "round-end playback boundary did not stop",
    )?;
    clock.seek(start - 1);
    require(
        clock.current_tick() == start && !clock.is_playing(),
        "seek did not clamp/pause at recorded start",
    )?;
    let mut special = Value::Null;
    if round.round_number == 1 {
        let scene = replay.scene(2648);
        require(
            scene.players.len() == 10 && scene.grenades.len() == 1,
            "pinned first-throw scene differs",
        )?;
        let event = replay
            .meta
            .utility_events
            .iter()
            .find(|event| event.id == 1)
            .ok_or("pinned first utility event missing")?;
        require(
            event.throw_tick == 2641
                && event.detonate_tick == Some(3025)
                && event.observed_trajectory_index == Some(11),
            "pinned first utility/projectile identity differs",
        )?;
        let trajectory = replay.trajectory_until(11, 2648);
        require(
            !trajectory.is_empty() && trajectory.iter().all(|p| p.tick <= 2648),
            "Rust projectile path absent or contains future samples",
        )?;
        require(
            matches!(replay.bomb_at(8258), BombState::NoObservedPosition),
            "pre-plant unknown carrier became an observed bomb",
        )?;
        require(
            matches!(replay.bomb_at(8259),BombState::Observed(b) if b.plant_tick==8259 && b.phase==BombPhase::Planted),
            "exact observed plant was not rendered into Rust scene",
        )?;
        special = json!({"tick":2648,"players":scene.players.len(),"grenades":scene.grenades.len(),"eventId":1,"throwTick":2641,"detonateTick":3025,"trajectoryIndex":11,"trajectoryPointsAtTick":trajectory.len(),"beforePlant":bomb_value(replay.bomb_at(8258)),"atPlant":bomb_value(replay.bomb_at(8259))});
    }
    if round.round_number == 3 {
        let plant = replay
            .events
            .iter()
            .find(|event| event.kind == ReplayEventKind::Bomb && event.tick == 21853)
            .ok_or("inter-round source plant missing")?;
        require(
            !plant.seekable && end < 21853 && replay.events.len() == 61,
            "round3inter-round event became seekable or event stream differs",
        )?;
        require(
            matches!(replay.bomb_at(21853), BombState::NoObservedPosition),
            "inter-round plant invented a playable bomb frame",
        )?;
        special = json!({"interRoundPlantTick":21853,"seekable":plant.seekable,"eventCount":replay.events.len(),"recordedEndTick":end,"outsideScene":bomb_value(replay.bomb_at(21853))});
    }
    if let Some(tick) = match round.round_number {
        6 => Some(38820),
        8 => Some(51718),
        _ => None,
    } {
        require(
            matches!(replay.bomb_at(tick),BombState::Observed(b) if b.phase==BombPhase::Exploded),
            "recorded explosion phase differs",
        )?;
        special = json!({"explosionTick":tick,"bomb":bomb_value(replay.bomb_at(tick))});
    }
    let mut kinds = BTreeMap::new();
    for event in &replay.events {
        *kinds.entry(format!("{:?}", event.kind)).or_insert(0_usize) += 1;
    }
    eprintln!(
        "RUST_REVIEW_ROUND {} players={} projectiles={} events={}",
        round.round_number,
        player_count,
        grenade_count,
        replay.events.len()
    );
    Ok(
        json!({"roundNumber":round.round_number,"startTick":start,"endTick":end,"tickRate":64,"positionsAvailable":replay.meta.positions_available,"metadataPlayerRows":replay.meta.player_sample_count,"metadataProjectileRows":replay.meta.grenade_sample_count,"boundedPlayerRows":player_count,"boundedProjectileRows":grenade_count,"playerSampleSha256":indexed_player_digest,"projectileSampleSha256":indexed_grenade_digest,"exactSQLiteSceneParity":true,"missingPlayerTicks":missing_player_ticks,"eventCount":replay.events.len(),"eventKinds":kinds,"nonseekableEvents":replay.events.iter().filter(|e|!e.seekable).map(|e|json!({"key":e.key,"tick":e.tick,"kind":format!("{:?}",e.kind)})).collect::<Vec<_>>(),"clock":{"oneSecond1xTick":one_x,"oneSecond1_5xTick":fast,"pauseStable":true,"roundEndStops":true,"seekClampsAndPauses":true},"pinnedCase":special}),
    )
}

fn source_bindings() -> Value {
    let files: [(&str, &[u8]); 13] = [
        ("Cargo.toml", include_bytes!("../../Cargo.toml")),
        ("Cargo.lock", include_bytes!("../../Cargo.lock")),
        ("src/lib.rs", include_bytes!("../lib.rs")),
        ("src/parser/raw.rs", include_bytes!("../parser/raw.rs")),
        ("src/domain/mod.rs", include_bytes!("../domain/mod.rs")),
        (
            "src/native_grenade_parser.rs",
            include_bytes!("../native_grenade_parser.rs"),
        ),
        ("src/demo_import.rs", include_bytes!("../demo_import.rs")),
        ("src/analysis/mod.rs", include_bytes!("../analysis/mod.rs")),
        ("src/db/mod.rs", include_bytes!("../db/mod.rs")),
        ("src/db/queries.rs", include_bytes!("../db/queries.rs")),
        ("src/match_review.rs", include_bytes!("../match_review.rs")),
        (
            "src/native_replay.rs",
            include_bytes!("../native_replay.rs"),
        ),
        (
            "src/bin/demostrike-rust-review-proof.rs",
            include_bytes!("demostrike-rust-review-proof.rs"),
        ),
    ];
    Value::Array(files.into_iter().map(|(path,bytes)|json!({"path":path,"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(bytes))})).collect())
}

fn publish_file_noclobber(source: &Path, destination: &Path) -> Result<(), String> {
    let mut staged =
        tempfile::NamedTempFile::new_in(destination.parent().ok_or("output parent missing")?)
            .map_err(|e| e.to_string())?;
    let mut input = File::open(source).map_err(|e| e.to_string())?;
    std::io::copy(&mut input, &mut staged)
        .and_then(|_| staged.flush())
        .and_then(|_| staged.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    staged
        .persist_noclobber(destination)
        .map_err(|e| format!("cannot publish new output without overwrite: {}", e.error))?;
    Ok(())
}
fn write_proof_noclobber(path: &Path, document: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(document).map_err(|e| e.to_string())?;
    let mut staged = tempfile::NamedTempFile::new_in(path.parent().ok_or("proof parent missing")?)
        .map_err(|e| e.to_string())?;
    staged
        .write_all(&bytes)
        .and_then(|_| staged.write_all(b"\n"))
        .and_then(|_| staged.flush())
        .and_then(|_| staged.as_file().sync_all())
        .map_err(|e| format!("cannot finish staged proof; no completed proof published: {e}"))?;
    staged.persist_noclobber(path).map_err(|e| {
        format!(
            "cannot publish completed proof without overwrite: {}",
            e.error
        )
    })?;
    Ok(())
}

fn run(plan: &ProofPlan) -> Result<Value, String> {
    verify_fixture(&plan.demo)?;
    // Never give the importer a user-selected database. It operates only in
    // a newly owned private scratch folder; completed bytes publish no-clobber.
    let scratch = tempfile::tempdir_in(plan.database.parent().ok_or("database parent missing")?)
        .map_err(|e| e.to_string())?;
    let scratch_database = scratch.path().join("native-import.sqlite");
    let started = Instant::now();
    let imported = import_demo(
        &plan.demo,
        &scratch_database,
        &NativeGrenadeParser,
        &ImportControl::new(),
        |stage| eprintln!("RUST_IMPORT_STAGE {stage}"),
    )
    .map_err(|e| format!("native import failed: {e}"))?;
    require(
        imported.capabilities.key() == "native-combat-full-v2",
        "native parser capability differs",
    )?;
    let import_milliseconds = started.elapsed().as_millis();
    {
        let connection =
            rusqlite::Connection::open(&scratch_database).map_err(|e| e.to_string())?;
        connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|e| e.to_string())?;
        let mode: String = connection
            .query_row("PRAGMA journal_mode=DELETE", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        require(
            mode == "delete",
            "scratch SQLite did not checkpoint to a standalone file",
        )?;
    }
    let before_review = file_hash(&scratch_database)?;
    let database_text = scratch_database
        .to_str()
        .ok_or("scratch path must be Unicode")?;
    let library = match_review::list_matches(database_text).map_err(|e| e.to_string())?;
    require(
        library.matches.len() == 1,
        "fresh scratch import must contain exactly one match",
    )?;
    let detail = match_review::get_match_detail(database_text, imported.match_id)
        .map_err(|e| e.to_string())?;
    require(
        detail.summary.source_sha256.as_deref() == Some(FIXTURE_SHA256),
        "stored source binding differs",
    )?;
    require(
        detail.summary.map_name == "de_mirage"
            && detail.rounds.len() == 10
            && detail.players.len() == 10
            && detail.utility_events.len() == 150,
        "pinned Mirage match/round/player/utility counts differ",
    )?;
    require(
        detail.full_combat_coverage,
        "native combat coverage unavailable",
    )?;
    require(
        detail
            .rounds
            .iter()
            .map(|r| r.start_tick)
            .collect::<Vec<_>>()
            == ROUND_STARTS.map(Some),
        "pinned recorded round starts differ",
    )?;
    let deaths: usize = detail
        .rounds
        .iter()
        .map(|r| r.deaths.as_ref().map_or(0, Vec::len))
        .sum();
    let hurts: usize = detail
        .rounds
        .iter()
        .map(|r| r.hurts.as_ref().map_or(0, Vec::len))
        .sum();
    require(
        deaths == 64 && hurts == 264,
        "pinned in-round combat counts differ",
    )?;
    require(
        detail
            .rounds
            .iter()
            .all(|r| r.deaths.is_some() && r.hurts.is_some()),
        "verified combat stream became unavailable",
    )?;
    require(
        detail.rounds.iter().all(|r| {
            r.team_a_side.as_deref() == Some("CT") && r.team_b_side.as_deref() == Some("T")
        }),
        "observed stable team sides differ",
    )?;
    let overkill = detail
        .rounds
        .iter()
        .flat_map(|r| r.hurts.iter().flatten())
        .filter(|h| {
            h.effective_damage
                .is_some_and(|effective| h.raw_damage > effective)
        })
        .count();
    require(
        overkill > 0
            && detail
                .rounds
                .iter()
                .flat_map(|r| r.hurts.iter().flatten())
                .all(|h| h.effective_damage.is_some()),
        "effective versus raw HP damage was lost",
    )?;
    let throws = match_review::get_utility_finder(database_text, imported.match_id)
        .map_err(|e| e.to_string())?;
    require(
        throws.len() == 150 && throws.iter().all(|t| t.trajectory.len() >= 2),
        "native finder trajectory coverage differs",
    )?;
    let rounds = detail
        .rounds
        .iter()
        .map(|r| prove_round(&scratch_database, imported.match_id, r))
        .collect::<Result<Vec<_>, _>>()?;
    let raw_counts = {
        let c = rusqlite::Connection::open_with_flags(
            &scratch_database,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|e| e.to_string())?;
        let mut counts = BTreeMap::new();
        for name in [
            "matches",
            "players",
            "rounds",
            "utility_events",
            "raw_combat_deaths",
            "raw_combat_hurts",
            "viewer_player_positions",
            "viewer_grenade_positions",
            "viewer_bomb_events",
        ] {
            counts.insert(
                name,
                c.query_row(&format!("SELECT COUNT(*) FROM {name}"), [], |r| {
                    r.get::<_, i64>(0)
                })
                .map_err(|e| e.to_string())?,
            );
        }
        counts
    };
    require(
        raw_counts.get("raw_combat_deaths") == Some(&73)
            && raw_counts.get("raw_combat_hurts") == Some(&264),
        "pinned raw ingest counts differ from scoped in-round facts",
    )?;
    require(
        file_hash(&scratch_database)? == before_review,
        "read-only native review changed scratch database bytes",
    )?;
    verify_fixture(&plan.demo)?;
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let document = json!({"proofVersion":1,"result":"pass","scope":"headless native Rust source2-demo parser -> SQLite -> shared match reader -> indexed replay and playback clock; not GPU, window, media, audio, HLAE or full-product acceptance","source":{"fixture":"LaihoE/demoparser@45ca85aeac8fb0de9d385124d2c7fe0e7b8ff0c8/src/parser/test_demo.dem","path":plan.demo,"bytes":FIXTURE_BYTES,"sha256":FIXTURE_SHA256},"build":{"packageVersion":env!("CARGO_PKG_VERSION"),"declaredCommit":option_env!("DEMOSTRIKE_REVIEW_BUILD_COMMIT"),"executableSha256":file_hash(&executable)?,"sourceFiles":source_bindings()},"database":{"path":plan.database,"sha256":before_review,"scratchImportOnly":true,"outputsNoOverwrite":true,"reviewDidNotChangeDatabaseBytes":true,"rawRowCounts":raw_counts},"match":{"map":"de_mirage","rounds":detail.rounds.len(),"players":detail.players.len(),"utilityEvents":detail.utility_events.len(),"scopedInRoundDeaths":deaths,"scopedInRoundHurts":hurts,"overkillHurts":overkill,"parserCapability":imported.capabilities.key()},"timings":{"importMilliseconds":import_milliseconds,"totalHeadlessMilliseconds":started.elapsed().as_millis()},"rounds":rounds,"notEstablished":["GPU/window rendering or visual polish","real captured footage or audio","CS2/HLAE provider/camera runtime","full CSDM feature parity or release packaging"]});
    publish_file_noclobber(&scratch_database, &plan.database)?;
    require(
        file_hash(&plan.database)? == before_review,
        "published scratch database differs from validated bytes",
    )?;
    write_proof_noclobber(&plan.proof, &document).map_err(|e| {
        format!(
            "new scratch database retained at {}; proof was not accepted: {e}",
            plan.database.display()
        )
    })?;
    Ok(document)
}

fn main() {
    let result =
        parse_args(&std::env::args_os().skip(1).collect::<Vec<_>>()).and_then(|plan| run(&plan));
    match result {
        Ok(document) => println!(
            "RUST_NATIVE_REVIEW_PROOF {}",
            json!({"result":"pass","sourceSha256":FIXTURE_SHA256,"rounds":document["rounds"].as_array().map(Vec::len),"databaseSha256":document["database"]["sha256"]})
        ),
        Err(error) => {
            eprintln!("DemoStrike Rust review proof: {error}");
            std::process::exit(1);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(demo: &Path, database: &Path, proof: &Path) -> Vec<OsString> {
        [
            OsString::from("--demo"),
            demo.as_os_str().to_owned(),
            OsString::from("--database"),
            database.as_os_str().to_owned(),
            OsString::from("--proof"),
            proof.as_os_str().to_owned(),
        ]
        .into()
    }

    #[test]
    fn accepts_only_three_explicit_absolute_paths_without_creating_outputs() {
        let folder = tempfile::tempdir().unwrap();
        let demo = folder.path().join("source.dem");
        std::fs::write(&demo, b"unparsed test bytes").unwrap();
        let database = folder.path().join("new.sqlite");
        let proof = folder.path().join("new.json");
        let plan = parse_args(&arguments(&demo, &database, &proof)).unwrap();
        assert_eq!(plan.demo, demo.canonicalize().unwrap());
        // Windows TEMP may use an 8.3 alias while canonicalize returns a
        // verbatim long path. The parent is resolved; new files do not exist.
        let canonical_parent = folder.path().canonicalize().unwrap();
        assert_eq!(plan.database, canonical_parent.join("new.sqlite"));
        assert_eq!(plan.proof, canonical_parent.join("new.json"));
        assert!(!database.exists());
        assert!(!proof.exists());
    }
    #[test]
    fn existing_parent_aliases_resolve_without_creating_output_files_or_directories() {
        let folder = tempfile::tempdir().unwrap();
        let demo = folder.path().join("source.dem");
        std::fs::write(&demo, b"unparsed test bytes").unwrap();
        let child = folder.path().join("existing-child");
        std::fs::create_dir(&child).unwrap();
        let alias = child.join("..");
        let database = alias.join("new.sqlite");
        let proof = alias.join("new.json");
        let plan = parse_args(&arguments(&demo, &database, &proof)).unwrap();
        let canonical_parent = folder.path().canonicalize().unwrap();
        assert_eq!(plan.database, canonical_parent.join("new.sqlite"));
        assert_eq!(plan.proof, canonical_parent.join("new.json"));
        assert!(!database.exists());
        assert!(!proof.exists());
        assert!(!plan.database.exists());
        assert!(!plan.proof.exists());
        let absent = folder.path().join("missing-child");
        assert!(parse_args(&arguments(&demo, &absent.join("new.sqlite"), &proof)).is_err());
        assert!(!absent.exists());
        assert_eq!(std::fs::read(&demo).unwrap(), b"unparsed test bytes");
    }
    #[test]
    fn existing_outputs_are_rejected_without_overwriting_either_file() {
        for existing in ["database", "proof"] {
            let folder = tempfile::tempdir().unwrap();
            let demo = folder.path().join("source.dem");
            std::fs::write(&demo, b"test bytes").unwrap();
            let database = folder.path().join("new.sqlite");
            let proof = folder.path().join("new.json");
            let target = if existing == "database" {
                &database
            } else {
                &proof
            };
            std::fs::write(target, b"user original bytes").unwrap();
            assert!(parse_args(&arguments(&demo, &database, &proof)).is_err());
            assert_eq!(std::fs::read(target).unwrap(), b"user original bytes");
            assert!(
                !if existing == "database" {
                    proof
                } else {
                    database
                }
                .exists()
            );
        }
    }

    #[test]
    fn unknown_duplicate_relative_and_stream_arguments_are_rejected() {
        let folder = tempfile::tempdir().unwrap();
        let demo = folder.path().join("source.dem");
        std::fs::write(&demo, b"test bytes").unwrap();
        let database = folder.path().join("new.sqlite");
        let proof = folder.path().join("new.json");
        let mut unknown = arguments(&demo, &database, &proof);
        unknown[0] = "--program".into();
        assert!(parse_args(&unknown).is_err());
        let mut duplicate = arguments(&demo, &database, &proof);
        duplicate[4] = "--demo".into();
        assert!(parse_args(&duplicate).is_err());
        assert!(parse_args(&arguments(&demo, Path::new("relative.sqlite"), &proof)).is_err());
        for name in [
            "user-file:stream.sqlite",
            "CON.sqlite",
            "NUL.sqlite",
            "COM1.sqlite",
            "LPT9.sqlite",
            "name.sqlite ",
        ] {
            assert!(
                parse_args(&arguments(&demo, &folder.path().join(name), &proof)).is_err(),
                "accepted {name}"
            );
        }
        assert!(!database.exists());
        assert!(!proof.exists());
    }

    #[test]
    fn wrong_fixture_is_rejected_before_any_database_or_proof_is_created() {
        let folder = tempfile::tempdir().unwrap();
        let demo = folder.path().join("wrong.dem");
        std::fs::write(&demo, b"PBDEMS2\0not the pinned real fixture").unwrap();
        let database = folder.path().join("new.sqlite");
        let proof = folder.path().join("new.json");
        let plan = parse_args(&arguments(&demo, &database, &proof)).unwrap();
        assert!(run(&plan).unwrap_err().contains("fixture byte length"));
        assert!(!database.exists());
        assert!(!proof.exists());
        assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 1);
    }

    #[test]
    fn publication_races_never_replace_existing_database_or_proof_bytes() {
        let folder = tempfile::tempdir().unwrap();
        let source = folder.path().join("owned.sqlite");
        std::fs::write(&source, b"owned validated database").unwrap();
        let destination = folder.path().join("user.sqlite");
        std::fs::write(&destination, b"user database").unwrap();
        assert!(publish_file_noclobber(&source, &destination).is_err());
        assert_eq!(std::fs::read(&destination).unwrap(), b"user database");
        let proof = folder.path().join("user.json");
        std::fs::write(&proof, b"user proof").unwrap();
        assert!(write_proof_noclobber(&proof, &json!({"result":"pass"})).is_err());
        assert_eq!(std::fs::read(&proof).unwrap(), b"user proof");
    }

    #[test]
    fn completed_new_outputs_publish_exact_bytes_without_touching_source() {
        let folder = tempfile::tempdir().unwrap();
        let source = folder.path().join("owned.sqlite");
        std::fs::write(&source, b"owned validated database").unwrap();
        let destination = folder.path().join("new.sqlite");
        publish_file_noclobber(&source, &destination).unwrap();
        assert_eq!(
            std::fs::read(&destination).unwrap(),
            b"owned validated database"
        );
        assert_eq!(std::fs::read(&source).unwrap(), b"owned validated database");
        let proof = folder.path().join("new.json");
        let document = json!({"result":"pass","scope":"headless only"});
        write_proof_noclobber(&proof, &document).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&std::fs::read(proof).unwrap()).unwrap(),
            document
        );
    }
}
