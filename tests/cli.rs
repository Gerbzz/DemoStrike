use std::fs;
use std::process::Command;

#[test]
fn inspects_demo_header_without_claiming_event_import() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("fixture.dem");
    fs::write(&path, b"PBDEMS2\0fixture").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_demostrike-cli"))
        .arg("inspect-demo").arg(&path).output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("CS2 PBDEMS2 header"));
    assert!(text.contains("events not parsed"));
}

#[test]
fn native_probe_rejects_a_truncated_cs2_header_without_crashing() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("truncated.dem");
    fs::write(&path, b"PBDEMS2\0").unwrap();
    for command in ["probe-demo", "probe-grenades"] {
        let output = Command::new(env!("CARGO_BIN_EXE_demostrike-cli"))
            .arg(command).arg(&path).output().unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("truncated CS2 demo header"));
    }
}

#[test]
fn analyzes_existing_raw_facts_from_command_line() {
    let dir = tempfile::TempDir::new().unwrap();
    let raw_path = dir.path().join("raw.json");
    fs::write(
        &raw_path,
        br#"{
        "version": 1,
        "header": {"mapName": "de_mirage", "serverName": null,
            "demoTicks": 6400, "playbackTime": 100.0}
    }"#,
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_demostrike-cli"))
        .arg("analyze")
        .arg(&raw_path)
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("de_mirage"));
    assert!(text.contains("0 utility events"));
}

#[test]
fn packages_existing_clip_from_draft_without_changing_source() {
    let dir = tempfile::TempDir::new().unwrap();
    let source = dir.path().join("source.mp4");
    fs::write(&source, b"abc").unwrap();
    let draft = dir.path().join("draft.json");
    let hash = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    let value = serde_json::json!({
        "package_id": "pilot-002",
        "inputs": [{
            "source_path": source,
            "clip": {
                "clip_id": "clip-1", "relative_path": "", "sha256": "",
                "demo_sha256": hash, "event_id": "event-1",
                "player_steamid64": "76561198000000001", "map": "de_mirage",
                "round": 4, "utility_type": "smoke", "start_tick": 1000,
                "end_tick": 1100, "camera_role": "projectile", "duration_ms": 1000,
                "width": 1920, "height": 1080, "fps_num": 60, "fps_den": 1,
                "audio": {"source": "hlae_wav", "channels": 2,
                    "sample_rate": 48000, "verified": true},
                "qc_status": "accepted", "group_order": 1
            }
        }]
    });
    fs::write(&draft, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    let destination = dir.path().join("package");
    let output = Command::new(env!("CARGO_BIN_EXE_demostrike-cli"))
        .arg("package-structural")
        .arg(&draft)
        .arg(&destination)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(destination.join("manifest.json").is_file());
    assert_eq!(fs::read(source).unwrap(), b"abc");
    assert!(String::from_utf8_lossy(&output.stdout).contains("STRUCTURAL ONLY"));
}

#[test]
fn relative_source_uses_draft_directory_not_process_directory() {
    let dir = tempfile::TempDir::new().unwrap();
    let draft_dir = dir.path().join("drafts");
    let decoy_dir = dir.path().join("decoy");
    fs::create_dir_all(&draft_dir).unwrap();
    fs::create_dir_all(&decoy_dir).unwrap();
    fs::write(draft_dir.join("source.mp4"), b"abc").unwrap();
    fs::write(decoy_dir.join("source.mp4"), b"wrong source").unwrap();
    let draft = draft_dir.join("draft.json");
    let hash = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    let value = serde_json::json!({"package_id":"relative-source","inputs":[{
        "source_path":"source.mp4","clip":{
            "clip_id":"clip-1","relative_path":"","sha256":"",
            "demo_sha256":hash,"event_id":"event-1","player_steamid64":"76561198000000001",
            "map":"de_mirage","round":4,"utility_type":"smoke","start_tick":1000,"end_tick":1100,
            "camera_role":"projectile","duration_ms":1000,"width":1920,"height":1080,
            "fps_num":60,"fps_den":1,"audio":{"source":"hlae_wav","channels":2,
                "sample_rate":48000,"verified":true},"qc_status":"accepted","group_order":1
        }
    }]});
    fs::write(&draft, serde_json::to_vec(&value).unwrap()).unwrap();
    let destination = dir.path().join("package");
    let output = Command::new(env!("CARGO_BIN_EXE_demostrike-cli"))
        .current_dir(&decoy_dir)
        .arg("package-structural").arg(&draft).arg(&destination).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(fs::read(destination.join("clips/clip-1.mp4")).unwrap(), b"abc");
    assert_eq!(fs::read(decoy_dir.join("source.mp4")).unwrap(), b"wrong source");
}

#[test]
fn package_command_does_not_accept_a_fake_mp4_as_audio_proof() {
    let dir = tempfile::TempDir::new().unwrap();
    let source = dir.path().join("fake.mp4");
    fs::write(&source, b"abc").unwrap();
    let draft = dir.path().join("draft.json");
    let hash = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    let value = serde_json::json!({"package_id":"pilot-003","inputs":[{
        "source_path":source,"clip":{
            "clip_id":"clip-1","relative_path":"","sha256":"",
            "demo_sha256":hash,"event_id":"event-1","player_steamid64":"76561198000000001",
            "map":"de_mirage","round":4,"utility_type":"smoke","start_tick":1000,"end_tick":1100,
            "camera_role":"projectile","duration_ms":1000,"width":1920,"height":1080,
            "fps_num":60,"fps_den":1,"audio":{"source":"hlae_wav","channels":2,
                "sample_rate":48000,"verified":true},"qc_status":"accepted","group_order":1
        }
    }]});
    fs::write(&draft, serde_json::to_vec(&value).unwrap()).unwrap();
    let destination = dir.path().join("package");
    let output = Command::new(env!("CARGO_BIN_EXE_demostrike-cli"))
        .arg("package")
        .arg(&draft)
        .arg(&destination)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!destination.exists());
}
