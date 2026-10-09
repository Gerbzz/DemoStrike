use clipstrike_core::capture_import::import_package_to_new_project;
use clipstrike_core::capture_package::{AudioEvidence, AudioSource, CaptureClip, QcStatus};
use clipstrike_core::project::Project;
use demostrike::package::{CaptureInput, finalize_capture_package};
use std::fs;

#[test]
fn three_capture_files_become_editable_and_survive_reopen() {
    let dir = tempfile::TempDir::new().unwrap();
    let demo_hash = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    let inputs: Vec<_> = (1..=3)
        .map(|order| {
            let source_path = dir.path().join(format!("take-{order}.mp4"));
            fs::write(&source_path, format!("fixture-{order}")).unwrap();
            CaptureInput {
                source_path,
                clip: CaptureClip {
                    clip_id: format!("clip-{order}"),
                    relative_path: String::new(),
                    sha256: String::new(),
                    demo_sha256: demo_hash.into(),
                    demo_path_hint: Some("match-001.dem".into()),
                    event_id: format!("event-{order}"),
                    player_steamid64: "76561198000000001".into(),
                    map: "de_mirage".into(),
                    round: 4,
                    utility_type: "smoke".into(),
                    start_tick: 1000 + i64::from(order) * 100,
                    end_tick: 1100 + i64::from(order) * 100,
                    camera_role: "projectile".into(),
                    duration_ms: 1000,
                    width: 1920,
                    height: 1080,
                    fps_num: 60,
                    fps_den: 1,
                    audio: AudioEvidence {
                        source: AudioSource::HlaeWav,
                        channels: 2,
                        sample_rate: 48000,
                        verified: true,
                    },
                    qc_status: QcStatus::Accepted,
                    group_order: order,
                },
            }
        })
        .collect();

    let package_dir = dir.path().join("package");
    finalize_capture_package(&package_dir, "pilot-001", &inputs).unwrap();
    let output = dir.path().join("pilot.cstrike");
    let mut project = import_package_to_new_project(&package_dir, &output).unwrap();
    assert_eq!(project.clips.len(), 3);
    assert_eq!(project.assets.len(), 3);
    assert_eq!(project.capture_sources.len(), 3);
    assert_ne!(project.assets[0].uri, project.assets[1].uri);
    project.clips[1].start = 4_000;
    project.clips[1].end = 5_000;
    project.save(&output).unwrap();
    let reopened = Project::load(&output).unwrap();
    assert_eq!(reopened.clips[1].start, 4_000);
    assert_eq!(reopened.capture_sources[1].clip.event_id, "event-2");
    assert_eq!(
        reopened.capture_sources[1].clip.demo_path_hint.as_deref(),
        Some("match-001.dem")
    );
    assert_eq!(fs::read(&inputs[1].source_path).unwrap(), b"fixture-2");

    let moved = dir.path().join("moved");
    fs::create_dir(&moved).unwrap();
    let moved_package = moved.join("package");
    let moved_project_path = moved.join("pilot.cstrike");
    fs::rename(&package_dir, &moved_package).unwrap();
    fs::rename(&output, &moved_project_path).unwrap();
    let mut moved_project = Project::load(&moved_project_path).unwrap();
    assert!(
        moved_project
            .assets
            .iter()
            .all(|asset| asset.uri.contains("/moved/package/"))
    );
    assert_eq!(moved_project.capture_sources[1].clip.event_id, "event-2");

    let renamed_package = moved.join("renamed-package");
    fs::rename(&moved_package, &renamed_package).unwrap();
    moved_project = Project::load(&moved_project_path).unwrap();
    assert!(
        moved_project
            .assets
            .iter()
            .all(|asset| asset.uri.starts_with("capture-offline://"))
    );
    moved_project
        .relink_capture_package(&moved_project_path, "pilot-001", &renamed_package)
        .unwrap();
    moved_project.save(&moved_project_path).unwrap();
    let relinked = Project::load(&moved_project_path).unwrap();
    assert!(
        relinked
            .assets
            .iter()
            .all(|asset| asset.uri.contains("/moved/renamed-package/"))
    );
}
