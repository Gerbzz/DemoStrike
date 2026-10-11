//! A capture receipt is separate from ClipStrike's capture-package v1. It
//! binds recorded bytes and review evidence to one source-bound request. The
//! package can be sealed only after every requested take is independently
//! decoded and visually reviewed.
use crate::capture_request::{CAPTURE_REQUEST_VERSION, CaptureRequest};
use crate::package::{CaptureInput, finalize_stream_probed_capture_package};
use clipstrike_core::capture_package::{
    AudioEvidence, AudioSource, CaptureClip, CapturePackage, QcStatus,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const CAPTURE_RECEIPT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureReceipt {
    pub version: u32,
    /// Hash of the exact saved request JSON bytes, not a reformatted copy.
    pub request_sha256: String,
    pub demo_sha256: String,
    pub event_id: i64,
    pub takes: Vec<CaptureTakeReceipt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureTakeReceipt {
    pub camera_role: String,
    pub runtime_identity_log: PathBuf,
    pub runtime_identity_log_sha256: String,
    pub issued_spec_player_value: i64,
    pub resolved_player_steamid64: String,
    pub muxed_mp4: PathBuf,
    pub muxed_sha256: String,
    pub muxed_bytes: u64,
    pub hlae_wav: PathBuf,
    pub hlae_wav_sha256: String,
    pub hlae_wav_bytes: u64,
    pub actual_start_tick: i64,
    pub actual_end_tick: i64,
    pub duration_ms: u64,
    pub width: u32,
    pub height: u32,
    pub fps_num: u32,
    pub fps_den: u32,
    pub audio_channels: u16,
    pub audio_sample_rate: u32,
    /// A full independent decoder pass over the finalized MP4 and WAV.
    pub full_video_decode: bool,
    pub full_audio_decode: bool,
    pub measured_audio_signal: bool,
    pub visual_review: Option<VisualReview>,
}

/// Recorder-side observations, kept separately from the reviewer receipt.
/// The recorder must resolve the issued spectator command through live demo
/// userinfo at setup and release; a self-reported SteamID in a receipt is not
/// enough. These records still do not replace visual POV inspection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeIdentityLog {
    pub version: u32,
    pub request_sha256: String,
    pub demo_sha256: String,
    pub event_id: i64,
    pub camera_role: String,
    pub issued_spec_player_value: i64,
    pub raw_console_log: PathBuf,
    pub raw_console_log_sha256: String,
    pub record_start_tick: i64,
    pub record_end_tick: i64,
    pub detonation_tick: Option<i64>,
    pub setup: IdentityObservation,
    pub release: IdentityObservation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityObservation {
    pub tick: i64,
    pub userinfo_id: i64,
    pub steamid64: String,
}

fn console_observation(console: &str, role: &str, kind: &str, tick: i64,
    entity: i64, steamid64: &str) -> Result<(), String> {
    let prefix = format!("DEMOSTRIKE_OBSERVE role={role} kind={kind} ");
    let matches: Vec<_> = console.lines().filter_map(|line|
        line.split_once(&prefix).map(|(_, rest)| rest)).collect();
    if matches.is_empty() {
        return Err(format!("{role} lacks live {kind} marker"));
    }
    for marker in matches {
        let fields: std::collections::HashMap<_, _> = marker.split_whitespace()
            .filter_map(|field| field.split_once('=')).collect();
        if fields.get("tick") != Some(&tick.to_string().as_str())
            || fields.get("entity") != Some(&entity.to_string().as_str())
            || fields.get("steamid") != Some(&steamid64)
            || fields.get("observerSteamid") != Some(&steamid64) {
            return Err(format!("{role} live {kind} marker conflicts with the requested identity or tick"));
        }
    }
    Ok(())
}

fn validate_runtime_identity_log(path: &Path, request_hash: &str,
    request: &CaptureRequest, take: &CaptureTakeReceipt) -> Result<(), String> {
    let bytes = std::fs::read(path)
        .map_err(|e| format!("cannot read runtime identity log {}: {e}", path.display()))?;
    if format!("{:x}", Sha256::digest(&bytes)) != take.runtime_identity_log_sha256 {
        return Err(format!("{} runtime identity log changed", take.camera_role));
    }
    let log: RuntimeIdentityLog = serde_json::from_slice(&bytes)
        .map_err(|e| format!("{} runtime identity log has unsupported data: {e}", take.camera_role))?;
    if log.version != 2 || log.request_sha256 != request_hash
        || log.demo_sha256 != request.demo_sha256 || log.event_id != request.event_id
        || log.camera_role != take.camera_role
        || log.issued_spec_player_value != take.issued_spec_player_value
        || log.issued_spec_player_value != request.observer_slot_candidate + 1
        || log.record_start_tick != take.actual_start_tick
        || log.record_end_tick != take.actual_end_tick
        || log.setup.tick > request.throw_tick || log.release.tick != request.throw_tick
        || log.setup.tick < take.actual_start_tick || log.release.tick > take.actual_end_tick
        || log.setup.userinfo_id != request.observer_slot_candidate
        || log.release.userinfo_id != request.observer_slot_candidate
        || log.setup.steamid64 != request.player_steamid64
        || log.release.steamid64 != request.player_steamid64 {
        return Err(format!("{} runtime identity observations do not match the requested player and release", take.camera_role));
    }
    let raw = std::fs::read(&log.raw_console_log)
        .map_err(|e| format!("cannot read raw runtime console log: {e}"))?;
    if raw.len() > 16 * 1024 * 1024
        || format!("{:x}", Sha256::digest(&raw)) != log.raw_console_log_sha256 {
        return Err(format!("{} raw runtime console log is unbound or oversized", take.camera_role));
    }
    let console = String::from_utf8(raw).map_err(|_| "runtime console is not UTF-8")?;
    for (kind, tick) in [("record_start_command", log.record_start_tick),
        ("setup", log.setup.tick), ("release", log.release.tick),
        ("record_end_command", log.record_end_tick),
        ("record_end_callback", log.record_end_tick)] {
        console_observation(&console, &take.camera_role, kind, tick,
            log.issued_spec_player_value, &request.player_steamid64)?;
    }
    if take.camera_role == "projectile" {
        if log.detonation_tick != Some(request.detonate_tick)
            || request.detonate_tick > take.actual_end_tick {
            return Err("projectile runtime log lacks the requested detonation tick".into());
        }
        console_observation(&console, &take.camera_role, "detonation",
            request.detonate_tick, log.issued_spec_player_value, &request.player_steamid64)?;
    } else if log.detonation_tick.is_some() {
        return Err(format!("{} claims a detonation outside its recorded role", take.camera_role));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualReview {
    pub reviewed_by: String,
    pub notes: String,
    /// `continuous` is required for the source-contiguous projectile flight.
    pub review_extent: String,
    pub correct_player: bool,
    pub release_observed: bool,
    pub expected_utility_visible_in_pov: bool,
    pub same_event_complete_flight: bool,
    pub model_feet_stance_visible: bool,
    pub clean_image: bool,
    pub proof_frames: Vec<ProofFrame>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofFrame {
    pub path: PathBuf,
    pub sha256: String,
    pub tick: i64,
}

fn file_identity(path: &Path) -> Result<(String, u64), String> {
    let mut file = File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let mut hash = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        if count == 0 { break; }
        hash.update(&buffer[..count]);
        bytes += count as u64;
    }
    Ok((format!("{:x}", hash.finalize()), bytes))
}

fn run_full_decode(path: &Path, with_video: bool, detect_audio_signal: bool,
    duration_ms: u64) -> Result<Option<f64>, String> {
    let stderr = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
    let mut command = Command::new("ffmpeg");
    command.args(["-nostdin", "-hide_banner", "-xerror"]);
    command.args(if detect_audio_signal { &["-v", "info"][..] } else { &["-v", "error"][..] });
    command.arg("-i").arg(path);
    if with_video { command.args(["-map", "0:v:0"]); }
    command.args(["-map", "0:a:0"]);
    if detect_audio_signal { command.args(["-af", "volumedetect"]); }
    command.args(["-f", "null", "-"]);
    let mut child = command.stdin(Stdio::null()).stdout(Stdio::null())
        .stderr(Stdio::from(stderr.reopen().map_err(|e| e.to_string())?))
        .spawn().map_err(|e| format!("cannot start independent FFmpeg decode: {e}"))?;
    let deadline = Instant::now() + Duration::from_secs((60 + duration_ms / 200).clamp(120, 3600));
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(40)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("independent media decode timed out: {}", path.display()));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("independent media decode wait failed: {error}"));
            }
        }
    };
    let diagnostics = std::fs::read_to_string(stderr.path()).unwrap_or_default();
    if !status.success() {
        return Err(format!("independent full decode failed for {}: {}",
            path.display(), diagnostics.trim().chars().take(1000).collect::<String>()));
    }
    if !detect_audio_signal { return Ok(None); }
    let maximum = diagnostics.lines().find_map(|line| {
        line.split_once("max_volume:").and_then(|(_, tail)|
            tail.split_whitespace().next()?.parse::<f64>().ok())
    }).ok_or_else(|| format!("HLAE WAV has no measured audio level: {}", path.display()))?;
    if !maximum.is_finite() || maximum <= -60.0 {
        return Err(format!("HLAE WAV has no meaningful audio signal ({maximum} dB): {}", path.display()));
    }
    Ok(Some(maximum))
}

/// Validate every take before producing `CaptureInput` for the unchanged v1
/// package. Machine flags and human visual claims must be backed by exact
/// immutable files. This still requires an honest reviewer; it cannot infer
/// correct POV from metadata or a screenshot filename.
pub fn reviewed_package_inputs(request_path: &Path, receipt_path: &Path)
    -> Result<(String, Vec<CaptureInput>), String> {
    let request_bytes = std::fs::read(request_path).map_err(|e| format!("cannot read capture request: {e}"))?;
    let request: CaptureRequest = serde_json::from_slice(&request_bytes)
        .map_err(|e| format!("invalid capture request: {e}"))?;
    let receipt: CaptureReceipt = serde_json::from_slice(&std::fs::read(receipt_path)
        .map_err(|e| format!("cannot read capture receipt: {e}"))?)
        .map_err(|e| format!("invalid capture receipt: {e}"))?;
    let request_hash = format!("{:x}", Sha256::digest(&request_bytes));
    if request.version != CAPTURE_REQUEST_VERSION || receipt.version != CAPTURE_RECEIPT_VERSION
        || receipt.request_sha256 != request_hash || receipt.demo_sha256 != request.demo_sha256
        || receipt.event_id != request.event_id {
        return Err("capture receipt does not bind this versioned request and demo event".into());
    }
    if file_identity(&request.demo_path)?.0 != request.demo_sha256 {
        return Err("source demo changed since the capture request was saved".into());
    }
    if receipt.takes.len() != request.takes.len() {
        return Err("capture receipt does not cover every requested camera take".into());
    }
    let mut seen = HashSet::new();
    let mut inputs = Vec::with_capacity(request.takes.len());
    for (order, planned) in request.takes.iter().enumerate() {
        let take = receipt.takes.iter().find(|take| take.camera_role == planned.camera_role)
            .ok_or_else(|| format!("missing {} capture take", planned.camera_role))?;
        if !seen.insert(&take.camera_role) {
            return Err(format!("duplicate {} capture take", take.camera_role));
        }
        if take.actual_start_tick > planned.start_tick || take.actual_end_tick < planned.end_tick
            || take.actual_start_tick >= take.actual_end_tick {
            return Err(format!("{} take does not cover the planned source ticks", take.camera_role));
        }
        if !take.full_video_decode || !take.full_audio_decode || !take.measured_audio_signal {
            return Err(format!("{} take lacks full decode or measured game-audio signal", take.camera_role));
        }
        if take.issued_spec_player_value <= 0
            || take.resolved_player_steamid64 != request.player_steamid64 {
            return Err(format!("{} take lacks hash-bound live player identity proof", take.camera_role));
        }
        validate_runtime_identity_log(&take.runtime_identity_log, &request_hash, &request, take)?;
        let review = take.visual_review.as_ref().ok_or_else(||
            format!("{} take has no visual review", take.camera_role))?;
        if review.reviewed_by.trim().is_empty() || review.notes.trim().is_empty()
            || !review.correct_player || !review.clean_image || review.proof_frames.is_empty()
            || (take.camera_role == "player_pov" && (!review.expected_utility_visible_in_pov || !review.release_observed))
            || (take.camera_role == "projectile" && (!review.same_event_complete_flight || review.review_extent != "continuous"))
            || (take.camera_role == "third_person_reference" && !review.model_feet_stance_visible) {
            return Err(format!("{} take has incomplete player, flight or clean-frame review", take.camera_role));
        }
        for frame in &review.proof_frames {
            if frame.tick < take.actual_start_tick || frame.tick > take.actual_end_tick
                || file_identity(&frame.path)?.0 != frame.sha256 {
                return Err(format!("{} visual proof frame is unbound", take.camera_role));
            }
        }
        if take.width != 1920 || take.height != 1080 || take.fps_num != 60 || take.fps_den != 1
            || take.duration_ms == 0 || take.audio_channels == 0 || take.audio_sample_rate == 0 {
            return Err(format!("{} take is outside the 1080p60 pilot profile", take.camera_role));
        }
        if file_identity(&take.muxed_mp4)? != (take.muxed_sha256.clone(), take.muxed_bytes)
            || file_identity(&take.hlae_wav)? != (take.hlae_wav_sha256.clone(), take.hlae_wav_bytes) {
            return Err(format!("{} take or source WAV changed after QC", take.camera_role));
        }
        run_full_decode(&take.muxed_mp4, true, true, take.duration_ms)?;
        run_full_decode(&take.hlae_wav, false, true, take.duration_ms)?;
        if file_identity(&take.muxed_mp4)? != (take.muxed_sha256.clone(), take.muxed_bytes)
            || file_identity(&take.hlae_wav)? != (take.hlae_wav_sha256.clone(), take.hlae_wav_bytes) {
            return Err(format!("{} take changed during independent full decode", take.camera_role));
        }
        let clip = CaptureClip {
            clip_id: format!("e{}-{}", request.event_id, planned.camera_role),
            relative_path: format!("clips/{}.mp4", planned.camera_role),
            sha256: take.muxed_sha256.clone(),
            demo_sha256: request.demo_sha256.clone(),
            demo_path_hint: Some(request.demo_path.display().to_string()),
            event_id: request.event_id.to_string(),
            player_steamid64: request.player_steamid64.clone(),
            map: request.map.clone(),
            round: u32::try_from(request.round).map_err(|_| "round is not positive".to_string())?,
            utility_type: request.utility_type.clone(),
            start_tick: take.actual_start_tick,
            end_tick: take.actual_end_tick,
            camera_role: planned.camera_role.clone(),
            duration_ms: take.duration_ms,
            width: take.width,
            height: take.height,
            fps_num: take.fps_num,
            fps_den: take.fps_den,
            audio: AudioEvidence { source: AudioSource::HlaeWav, channels: take.audio_channels,
                sample_rate: take.audio_sample_rate, verified: true },
            qc_status: QcStatus::Accepted,
            group_order: order as u32,
        };
        inputs.push(CaptureInput { source_path: take.muxed_mp4.clone(), clip });
    }
    let package_id = format!("demostrike-{}-e{}", &request.demo_sha256[..16], request.event_id);
    Ok((package_id, inputs))
}

pub fn seal_reviewed_capture(request_path: &Path, receipt_path: &Path, destination: &Path)
    -> Result<CapturePackage, String> {
    let (package_id, inputs) = reviewed_package_inputs(request_path, receipt_path)?;
    finalize_stream_probed_capture_package(destination, &package_id, &inputs)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_log_must_bind_live_userinfo_at_setup_and_release() {
        // Synthetic player and ticks; no recorded console or match data is embedded.
        let dir = tempfile::tempdir().unwrap();
        let request: CaptureRequest = serde_json::from_value(serde_json::json!({
            "version": 1, "demoPath": "fixture.dem", "demoSha256": "sourcehash", "demoBytes": 10,
            "matchId": 1, "eventId": 1, "map": "de_fixture", "round": 1,
            "playerSteamid64": "76561198000000011", "utilityType": "he",
            "projectileEntityId": 7, "observerSlotCandidate": 2, "tickRate": 64.0,
            "throwTick": 120, "detonateTick": 130,
            "throwPosition": {"x": 1.0, "y": 2.0, "z": 3.0},
            "detonatePosition": {"x": 4.0, "y": 5.0, "z": 6.0},
            "flightSamples": [[121.0, 1.0, 2.0, 3.0], [129.0, 4.0, 5.0, 6.0]],
            "takes": [{"cameraRole": "player_pov", "startTick": 100, "endTick": 140}]
        })).unwrap();
        let path = dir.path().join("identity.json");
        let console_path = dir.path().join("console.log");
        let console = ["record_start_command 100", "setup 110", "release 120",
            "record_end_command 140", "record_end_callback 140"].iter()
            .map(|item| {
                let (kind, tick) = item.split_once(' ').unwrap();
                format!("DEMOSTRIKE_OBSERVE role=player_pov kind={kind} tick={tick} entity=3 name=fixture_player steamid=76561198000000011 observer=fixture_player observerSteamid=76561198000000011")
            }).collect::<Vec<_>>().join("\n");
        std::fs::write(&console_path, &console).unwrap();
        let mut log = serde_json::json!({
            "version": 2, "requestSha256": "requesthash", "demoSha256": "sourcehash",
            "eventId": 1, "cameraRole": "player_pov", "issuedSpecPlayerValue": 3,
            "rawConsoleLog": console_path,
            "rawConsoleLogSha256": file_identity(&console_path).unwrap().0,
            "recordStartTick": 100, "recordEndTick": 140, "detonationTick": null,
            "setup": {"tick": 110, "userinfoId": 2, "steamid64": "76561198000000011"},
            "release": {"tick": 120, "userinfoId": 2, "steamid64": "76561198000000011"}
        });
        std::fs::write(&path, serde_json::to_vec(&log).unwrap()).unwrap();
        let mut take: CaptureTakeReceipt = serde_json::from_value(serde_json::json!({
            "cameraRole": "player_pov", "runtimeIdentityLog": path,
            "runtimeIdentityLogSha256": file_identity(&path).unwrap().0,
            "issuedSpecPlayerValue": 3, "resolvedPlayerSteamid64": "76561198000000011",
            "muxedMp4": "unused.mp4", "muxedSha256": "", "muxedBytes": 0,
            "hlaeWav": "unused.wav", "hlaeWavSha256": "", "hlaeWavBytes": 0,
            "actualStartTick": 100, "actualEndTick": 140, "durationMs": 4000,
            "width": 1920, "height": 1080, "fpsNum": 60, "fpsDen": 1,
            "audioChannels": 2, "audioSampleRate": 48000,
            "fullVideoDecode": false, "fullAudioDecode": false,
            "measuredAudioSignal": false, "visualReview": null
        })).unwrap();
        validate_runtime_identity_log(&path, "requesthash", &request, &take).unwrap();
        log["release"]["steamid64"] = serde_json::json!("76561198000000012");
        std::fs::write(&path, serde_json::to_vec(&log).unwrap()).unwrap();
        take.runtime_identity_log_sha256 = file_identity(&path).unwrap().0;
        assert!(validate_runtime_identity_log(&path, "requesthash", &request, &take)
            .unwrap_err().contains("do not match"));
        log["release"]["steamid64"] = serde_json::json!("76561198000000011");
        std::fs::write(&path, serde_json::to_vec(&log).unwrap()).unwrap();
        take.runtime_identity_log_sha256 = file_identity(&path).unwrap().0;
        std::fs::write(&console_path, console.replace("observerSteamid=76561198000000011",
            "observerSteamid=76561198000000012")).unwrap();
        log["rawConsoleLogSha256"] = serde_json::json!(file_identity(&console_path).unwrap().0);
        std::fs::write(&path, serde_json::to_vec(&log).unwrap()).unwrap();
        take.runtime_identity_log_sha256 = file_identity(&path).unwrap().0;
        assert!(validate_runtime_identity_log(&path, "requesthash", &request, &take)
            .unwrap_err().contains("live"));
    }

}
