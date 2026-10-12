//! A capture receipt is separate from ClipStrike's capture-package v1. It
//! binds recorded bytes and review evidence to one source-bound request. The
//! package can be sealed only after every requested take is independently
//! decoded and visually reviewed.
use crate::capture_request::{CAPTURE_REQUEST_VERSION, CaptureRequest};
use crate::media_probe::probe_capture_pair_with_cancel;
use crate::package::{CaptureInput, ReviewBinding,
    finalize_stream_probed_capture_package_with_cancel_and_guard,
    staged_package_path, write_portable_review_evidence_before_commit};
use clipstrike_core::capture_package::{
    AudioEvidence, AudioSource, CaptureClip, CapturePackage, PackageError, QcStatus,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

// v2 requires explicit human confirmation of audible game audio per take.
pub const CAPTURE_RECEIPT_VERSION: u32 = 2;

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
    /// Human confirmation after listening to this take in the source monitor.
    #[serde(default)]
    pub audible_game_audio: bool,
    pub proof_frames: Vec<ProofFrame>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofFrame {
    pub path: PathBuf,
    pub sha256: String,
    pub tick: i64,
}

/// Unsaved, user-authored review. No QC or human verdict is inferred from a
/// probe. Preview hashes bind the decisions to the files opened in FilmCraft's
/// source monitor; the writer hashes those files again before creating a receipt.
#[derive(Debug, Clone, Default)]
pub struct ReviewTakeDraft {
    pub camera_role: String,
    pub runtime_identity_log: String,
    pub muxed_mp4: String,
    pub hlae_wav: String,
    pub preview_muxed_sha256: String,
    pub preview_wav_sha256: String,
    pub reviewed_by: String,
    pub notes: String,
    pub review_extent: String,
    pub correct_player: bool,
    pub release_observed: bool,
    pub expected_utility_visible_in_pov: bool,
    pub same_event_complete_flight: bool,
    pub model_feet_stance_visible: bool,
    pub clean_image: bool,
    pub audible_game_audio: bool,
    pub proof_frames: Vec<ProofFrameDraft>,
}

#[derive(Debug, Clone, Default)]
pub struct ProofFrameDraft {
    pub path: String,
    pub tick: String,
}

pub fn draft_takes_for_request(path: &Path) -> Result<Vec<ReviewTakeDraft>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot open capture request: {e}"))?;
    if bytes.len() > 4 * 1024 * 1024 { return Err("capture request exceeds 4 MiB".into()); }
    let request: CaptureRequest = serde_json::from_slice(&bytes)
        .map_err(|e| format!("invalid capture request: {e}"))?;
    if request.version != CAPTURE_REQUEST_VERSION || request.takes.len() != 3 {
        return Err("review requires a versioned three-role request".into());
    }
    let roles: HashSet<_> = request.takes.iter().map(|take| take.camera_role.as_str()).collect();
    if roles != HashSet::from(["player_pov", "projectile", "third_person_reference"]) {
        return Err("review request has unsupported or duplicate camera roles".into());
    }
    Ok(request.takes.iter().map(|take| ReviewTakeDraft {
        camera_role: take.camera_role.clone(), ..Default::default()
    }).collect())
}

/// Called before opening a source monitor. These are observations of exact
/// bytes, never a claim that the user watched or listened to them.
pub fn preview_identities_with_cancel(video: &Path, wav: &Path,
    should_cancel: impl Fn() -> bool) -> Result<(String, String), String> {
    if !video.is_absolute() || !wav.is_absolute()
        || !video.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("mp4"))
        || !wav.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("wav")) {
        return Err("select absolute MP4 and WAV files for this raw take".into());
    }
    for path in [video, wav] {
        let size = std::fs::metadata(path).map_err(|e| format!("cannot inspect {}: {e}", path.display()))?.len();
        if size == 0 || size > 8 * 1024 * 1024 * 1024 {
            return Err(format!("raw take is empty or exceeds 8 GiB: {}", path.display()));
        }
    }
    Ok((file_identity_with_cancel(video, &should_cancel)?.0,
        file_identity_with_cancel(wav, &should_cancel)?.0))
}

fn observed_tick(console: &str, role: &str, kind: &str, issued: i64,
    steamid: &str) -> Result<i64, String> {
    let prefix = format!("DEMOSTRIKE_OBSERVE role={role} kind={kind} ");
    let mut found = None;
    for tail in console.lines().filter_map(|line| line.split_once(&prefix).map(|(_, tail)| tail)) {
        let tick = tail.split_whitespace().find_map(|field| field.strip_prefix("tick="))
            .ok_or_else(|| format!("{role} {kind} marker has no tick"))?
            .parse::<i64>().map_err(|_| format!("{role} {kind} marker tick is invalid"))?;
        if found.is_some_and(|previous| previous != tick) {
            return Err(format!("{role} has conflicting {kind} observations"));
        }
        found = Some(tick);
    }
    let tick = found.ok_or_else(|| format!("{role} lacks a recorded {kind} observation"))?;
    console_observation(console, role, kind, tick, issued, steamid)?;
    Ok(tick)
}

/// Convert actual pilot console markers into bounded, source-bound identity
/// logs. Every marker is checked against the planned userinfo and observer
/// SteamID. This is machine evidence; visual POV still needs human review.
pub fn author_runtime_identity_logs_with_cancel(manifest_path: &Path,
    destination: &Path, should_cancel: impl Fn() -> bool)
    -> Result<Vec<(String, PathBuf)>, String> {
    if should_cancel() { return Err("runtime identity review canceled".into()); }
    if !destination.is_absolute() || destination.exists() {
        return Err("choose a new absolute folder for runtime identity logs".into());
    }
    let manifest_bytes = std::fs::read(manifest_path).map_err(|e| format!("pilot manifest: {e}"))?;
    if manifest_bytes.len() > 1024 * 1024 { return Err("pilot manifest exceeds 1 MiB".into()); }
    let manifest: crate::event_pilot::EventPilotManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| format!("invalid pilot manifest: {e}"))?;
    if manifest.version != 4 { return Err("unsupported event-pilot manifest version".into()); }
    let request_bytes = std::fs::read(&manifest.request_path).map_err(|e| format!("capture request: {e}"))?;
    if request_bytes.len() > 4 * 1024 * 1024 { return Err("capture request exceeds 4 MiB".into()); }
    let request_hash = format!("{:x}", Sha256::digest(&request_bytes));
    let request: CaptureRequest = serde_json::from_slice(&request_bytes)
        .map_err(|e| format!("invalid capture request: {e}"))?;
    if request.version != CAPTURE_REQUEST_VERSION || manifest.request_sha256 != request_hash
        || manifest.issued_spec_player_value != request.observer_slot_candidate + 1 {
        return Err("pilot manifest does not bind the capture request and observer candidate".into());
    }
    let roles: HashSet<_> = request.takes.iter().map(|take| take.camera_role.as_str()).collect();
    if request.takes.len() != 3
        || roles != HashSet::from(["player_pov", "projectile", "third_person_reference"]) {
        return Err("pilot request does not contain three distinct supported camera roles".into());
    }
    let raw = std::fs::read(&manifest.runtime_console_path)
        .map_err(|e| format!("recorded pilot console: {e}"))?;
    if raw.is_empty() || raw.len() > 16 * 1024 * 1024 {
        return Err("recorded pilot console is empty or exceeds 16 MiB".into());
    }
    let console = String::from_utf8(raw.clone())
        .map_err(|_| "recorded pilot console is not UTF-8")?;
    let console_hash = format!("{:x}", Sha256::digest(&raw));
    let mut prepared = Vec::new();
    for planned in &request.takes {
        if should_cancel() { return Err("runtime identity review canceled".into()); }
        let role = planned.camera_role.as_str();
        if !matches!(role, "player_pov" | "projectile" | "third_person_reference") {
            return Err("unsupported pilot camera role".into());
        }
        let tick = |kind| observed_tick(&console, role, kind,
            manifest.issued_spec_player_value, &request.player_steamid64);
        let start = tick("record_start_command")?;
        let setup = tick("setup")?;
        let release = tick("release")?;
        let end = tick("record_end_command")?;
        let callback = tick("record_end_callback")?;
        let detonation = if role == "projectile" { Some(tick("detonation")?) } else { None };
        if end != callback || start > planned.start_tick || end < planned.end_tick
            || setup < start || setup > request.throw_tick
            || release != request.throw_tick || release > end
            || (role == "projectile" && detonation != Some(request.detonate_tick)) {
            return Err(format!("{role} recorded markers do not cover the requested source ticks"));
        }
        let observation = |tick| IdentityObservation { tick,
            userinfo_id: request.observer_slot_candidate,
            steamid64: request.player_steamid64.clone() };
        let log = RuntimeIdentityLog {
            version: 2, request_sha256: request_hash.clone(),
            demo_sha256: request.demo_sha256.clone(), event_id: request.event_id,
            camera_role: role.into(), issued_spec_player_value: manifest.issued_spec_player_value,
            raw_console_log: manifest.runtime_console_path.clone(),
            raw_console_log_sha256: console_hash.clone(), record_start_tick: start,
            record_end_tick: end, detonation_tick: detonation,
            setup: observation(setup), release: observation(release),
        };
        prepared.push((role.to_string(), serde_json::to_vec_pretty(&log)
            .map_err(|e| e.to_string())?));
    }
    if should_cancel() { return Err("runtime identity review canceled".into()); }
    std::fs::create_dir(destination).map_err(|e| format!("cannot create identity folder: {e}"))?;
    let mut paths = Vec::new();
    use std::io::Write;
    for (role, bytes) in prepared {
        if should_cancel() { return Err("runtime identity review canceled; partial folder remains".into()); }
        let path = destination.join(format!("runtime-identity-{role}.json"));
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&path)
            .map_err(|e| format!("cannot create identity log: {e}"))?;
        file.write_all(&bytes).and_then(|_| file.sync_all())
            .map_err(|e| format!("incomplete identity log {}: {e}", path.display()))?;
        paths.push((role, path));
    }
    Ok(paths)
}

fn complete_review(role: &str, review: &VisualReview) -> Result<(), String> {
    if review.reviewed_by.trim().is_empty() || review.notes.trim().is_empty()
        || !review.correct_player || !review.clean_image || !review.audible_game_audio
        || review.proof_frames.is_empty()
        || (role == "player_pov" && (!review.expected_utility_visible_in_pov || !review.release_observed))
        || (role == "projectile" && (!review.same_event_complete_flight || review.review_extent != "continuous"))
        || (role == "third_person_reference" && !review.model_feet_stance_visible) {
        return Err(format!("{role} has incomplete player, motion, image or listened-audio review"));
    }
    Ok(())
}

/// Validate existing runtime logs and media, then create a new receipt. Human
/// decisions come only from the draft's explicit controls. Cancellation and
/// stale media leave the destination absent.
pub fn author_reviewed_receipt_with_cancel(request_path: &Path, takes: &[ReviewTakeDraft],
    destination: &Path, should_cancel: impl Fn() -> bool) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(20 * 60);
    let should_cancel = || should_cancel() || Instant::now() >= deadline;
    if should_cancel() { return Err("capture review canceled".into()); }
    if !destination.is_absolute() || destination.exists() {
        return Err("choose a new absolute receipt path".into());
    }
    let request_bytes = std::fs::read(request_path).map_err(|e| e.to_string())?;
    if request_bytes.len() > 4 * 1024 * 1024 { return Err("capture request exceeds 4 MiB".into()); }
    let request: CaptureRequest = serde_json::from_slice(&request_bytes).map_err(|e| e.to_string())?;
    if request.version != CAPTURE_REQUEST_VERSION || takes.len() != request.takes.len() {
        return Err("review does not cover the versioned capture request".into());
    }
    let request_hash = format!("{:x}", Sha256::digest(&request_bytes));
    if file_identity_with_cancel(&request.demo_path, &should_cancel)?
        != (request.demo_sha256.clone(), request.demo_bytes) {
        return Err("source demo changed since capture planning".into());
    }
    let mut receipts = Vec::with_capacity(takes.len());
    let mut seen = HashSet::new();
    for planned in &request.takes {
        if should_cancel() { return Err("capture review canceled".into()); }
        let take = takes.iter().find(|take| take.camera_role == planned.camera_role)
            .ok_or_else(|| format!("missing {} review", planned.camera_role))?;
        if !seen.insert(&take.camera_role) { return Err("duplicate review role".into()); }
        let video = PathBuf::from(take.muxed_mp4.trim());
        let wav = PathBuf::from(take.hlae_wav.trim());
        let log_path = PathBuf::from(take.runtime_identity_log.trim());
        if !log_path.is_absolute() || !video.is_absolute() || !wav.is_absolute() {
            return Err(format!("{} review needs absolute media and runtime-log paths", take.camera_role));
        }
        let (video_hash, wav_hash) = preview_identities_with_cancel(&video, &wav, &should_cancel)?;
        if video_hash != take.preview_muxed_sha256 || wav_hash != take.preview_wav_sha256 {
            return Err(format!("{} media changed since source-monitor review", take.camera_role));
        }
        let log_bytes = std::fs::read(&log_path).map_err(|e| format!("runtime log: {e}"))?;
        if log_bytes.len() > 1024 * 1024 { return Err("runtime identity log exceeds 1 MiB".into()); }
        let log: RuntimeIdentityLog = serde_json::from_slice(&log_bytes)
            .map_err(|e| format!("invalid runtime identity log: {e}"))?;
        let (video_probe, audio_probe) = probe_capture_pair_with_cancel(&video, &wav, &should_cancel)
            .map_err(|e| e.to_string())?;
        let duration_ms = (video_probe.duration_seconds * 1000.0).round() as u64;
        if duration_ms == 0 || duration_ms > 120_000 {
            return Err(format!("{} take exceeds the two-minute review limit", take.camera_role));
        }
        let mut proof_frames = Vec::new();
        for frame in &take.proof_frames {
            let path = PathBuf::from(frame.path.trim());
            let tick: i64 = frame.tick.trim().parse().map_err(|_| "proof frame needs a source tick")?;
            if tick < log.record_start_tick || tick > log.record_end_tick {
                return Err(format!("{} proof tick is outside the recorded take", take.camera_role));
            }
            let meta = std::fs::metadata(&path).map_err(|e| format!("proof frame: {e}"))?;
            if !path.is_absolute() || meta.len() < 24 || meta.len() > 32 * 1024 * 1024 {
                return Err("proof PNG must be an absolute, bounded image file".into());
            }
            let mut header = [0_u8; 8];
            File::open(&path).and_then(|mut f| f.read_exact(&mut header))
                .map_err(|e| format!("proof PNG: {e}"))?;
            if header != [137,80,78,71,13,10,26,10] {
                return Err("proof image is not a PNG".into());
            }
            decode_proof_png_with_cancel(&path, &should_cancel)?;
            proof_frames.push(ProofFrame { sha256: file_identity_with_cancel(&path, &should_cancel)?.0,
                path, tick });
        }
        let review = VisualReview {
            reviewed_by: take.reviewed_by.clone(), notes: take.notes.clone(),
            review_extent: take.review_extent.clone(), correct_player: take.correct_player,
            release_observed: take.release_observed,
            expected_utility_visible_in_pov: take.expected_utility_visible_in_pov,
            same_event_complete_flight: take.same_event_complete_flight,
            model_feet_stance_visible: take.model_feet_stance_visible,
            clean_image: take.clean_image, audible_game_audio: take.audible_game_audio,
            proof_frames,
        };
        complete_review(&take.camera_role, &review)?;
        let receipt = CaptureTakeReceipt {
            camera_role: take.camera_role.clone(), runtime_identity_log: log_path.clone(),
            runtime_identity_log_sha256: format!("{:x}", Sha256::digest(&log_bytes)),
            issued_spec_player_value: log.issued_spec_player_value,
            resolved_player_steamid64: log.release.steamid64.clone(),
            muxed_mp4: video.clone(), muxed_sha256: video_hash.clone(),
            muxed_bytes: std::fs::metadata(&video).map_err(|e| e.to_string())?.len(),
            hlae_wav: wav.clone(), hlae_wav_sha256: wav_hash.clone(),
            hlae_wav_bytes: std::fs::metadata(&wav).map_err(|e| e.to_string())?.len(),
            actual_start_tick: log.record_start_tick, actual_end_tick: log.record_end_tick,
            duration_ms, width: video_probe.width, height: video_probe.height,
            fps_num: video_probe.fps_num, fps_den: video_probe.fps_den,
            audio_channels: audio_probe.channels, audio_sample_rate: audio_probe.sample_rate,
            full_video_decode: false, full_audio_decode: false,
            measured_audio_signal: false, visual_review: Some(review),
        };
        validate_runtime_identity_log(&log_path, &request_hash, &request, &receipt)?;
        if receipt.actual_start_tick > planned.start_tick || receipt.actual_end_tick < planned.end_tick {
            return Err(format!("{} runtime log does not cover planned source ticks", take.camera_role));
        }
        let mut receipt = receipt;
        run_full_decode_with_cancel(&video, true, true, duration_ms, &should_cancel)?;
        receipt.full_video_decode = true;
        receipt.measured_audio_signal = run_full_decode_with_cancel(&wav, false, true,
            duration_ms, &should_cancel)?.is_some();
        receipt.full_audio_decode = true;
        if file_identity_with_cancel(&video, &should_cancel)?.0 != video_hash
            || file_identity_with_cancel(&wav, &should_cancel)?.0 != wav_hash
            || file_identity_with_cancel(&log_path, &should_cancel)?.0 != receipt.runtime_identity_log_sha256 {
            return Err(format!("{} source changed during receipt authoring", take.camera_role));
        }
        for frame in &receipt.visual_review.as_ref().unwrap().proof_frames {
            if file_identity_with_cancel(&frame.path, &should_cancel)?.0 != frame.sha256 {
                return Err(format!("{} proof PNG changed during receipt authoring", take.camera_role));
            }
        }
        receipts.push(receipt);
    }
    if should_cancel() { return Err("capture review canceled".into()); }
    if file_identity_with_cancel(request_path, &should_cancel)?.0 != request_hash {
        return Err("capture request changed during review".into());
    }
    if file_identity_with_cancel(&request.demo_path, &should_cancel)?
        != (request.demo_sha256.clone(), request.demo_bytes) {
        return Err("source demo changed during review".into());
    }
    for take in &receipts {
        if file_identity_with_cancel(&take.muxed_mp4, &should_cancel)?
            != (take.muxed_sha256.clone(), take.muxed_bytes)
            || file_identity_with_cancel(&take.hlae_wav, &should_cancel)?
                != (take.hlae_wav_sha256.clone(), take.hlae_wav_bytes) {
            return Err(format!("{} media changed before receipt creation", take.camera_role));
        }
        validate_runtime_identity_log(&take.runtime_identity_log,
            &request_hash, &request, take)?;
        for frame in &take.visual_review.as_ref().unwrap().proof_frames {
            if file_identity_with_cancel(&frame.path, &should_cancel)?.0 != frame.sha256 {
                return Err(format!("{} proof PNG changed before receipt creation", take.camera_role));
            }
        }
    }
    let receipt = CaptureReceipt { version: CAPTURE_RECEIPT_VERSION, request_sha256: request_hash,
        demo_sha256: request.demo_sha256, event_id: request.event_id, takes: receipts };
    let bytes = serde_json::to_vec_pretty(&receipt).map_err(|e| e.to_string())?;
    if bytes.len() > 4 * 1024 * 1024 { return Err("reviewed receipt exceeds 4 MiB".into()); }
    if should_cancel() { return Err("capture review canceled before receipt creation".into()); }
    write_receipt_atomically_with(destination, &bytes, &should_cancel, |file, bytes| {
        use std::io::Write;
        file.write_all(bytes).and_then(|_| file.sync_all())
    })
}

fn write_receipt_atomically_with(destination: &Path, bytes: &[u8],
    should_cancel: &impl Fn() -> bool,
    write_and_sync: impl FnOnce(&mut File, &[u8]) -> std::io::Result<()>) -> Result<(), String> {
    if !destination.is_absolute() || destination.exists() {
        return Err("choose a new absolute receipt path".into());
    }
    let parent = destination.parent().ok_or("review receipt has no parent folder")?;
    let mut temporary = tempfile::Builder::new().prefix(".demostrike-receipt-")
        .tempfile_in(parent).map_err(|e| format!("cannot stage reviewed receipt: {e}"))?;
    write_and_sync(temporary.as_file_mut(), bytes)
        .map_err(|e| format!("receipt write or sync failed; temporary file removed: {e}"))?;
    if should_cancel() { return Err("capture review canceled before receipt commit".into()); }
    temporary.persist_noclobber(destination)
        .map_err(|e| format!("cannot commit reviewed receipt without overwrite: {}", e.error))?;
    Ok(())
}

#[cfg(test)]
fn file_identity(path: &Path) -> Result<(String, u64), String> {
    file_identity_with_cancel(path, &|| false)
}

fn file_identity_with_cancel(path: &Path, should_cancel: &impl Fn() -> bool) -> Result<(String, u64), String> {
    let mut file = File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let mut hash = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        if should_cancel() { return Err("capture receipt validation canceled".into()); }
        let count = file.read(&mut buffer).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        if count == 0 { break; }
        hash.update(&buffer[..count]);
        bytes += count as u64;
    }
    Ok((format!("{:x}", hash.finalize()), bytes))
}

fn require_receipt_unchanged(path: &Path, expected_hash: &str,
    should_cancel: &impl Fn() -> bool) -> Result<(), String> {
    if file_identity_with_cancel(path, should_cancel)?.0 != expected_hash {
        return Err("review receipt changed before package commit; revalidate and seal again".into());
    }
    Ok(())
}

#[cfg(test)]
fn run_full_decode(path: &Path, with_video: bool, detect_audio_signal: bool,
    duration_ms: u64) -> Result<Option<f64>, String> {
    run_full_decode_with_cancel(path, with_video, detect_audio_signal, duration_ms, &|| false)
}

fn run_full_decode_with_cancel(path: &Path, with_video: bool, detect_audio_signal: bool,
    duration_ms: u64, should_cancel: &impl Fn() -> bool) -> Result<Option<f64>, String> {
    let stderr = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
    let mut command = Command::new(crate::media_tools::ffmpeg_program());
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
        if should_cancel() {
            let _ = child.kill();
            let _ = child.wait();
            return Err("capture receipt validation canceled".into());
        }
        if std::fs::metadata(stderr.path()).is_ok_and(|m| m.len() > 8 * 1024 * 1024) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("independent media decode exceeded its 8 MiB diagnostic limit: {}", path.display()));
        }
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

fn decode_proof_png_with_cancel(path: &Path,
    should_cancel: &impl Fn() -> bool) -> Result<(), String> {
    if should_cancel() { return Err("proof PNG decode canceled".into()); }
    let stderr = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
    let mut child = Command::new(crate::media_tools::ffmpeg_program())
        .args(["-nostdin", "-hide_banner", "-v", "error", "-xerror", "-i"])
        .arg(path).args(["-map", "0:v:0", "-frames:v", "1", "-f", "null", "-"])
        .stdin(Stdio::null()).stdout(Stdio::null())
        .stderr(Stdio::from(stderr.reopen().map_err(|e| e.to_string())?))
        .spawn().map_err(|e| format!("cannot decode proof PNG: {e}"))?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if should_cancel() || Instant::now() >= deadline
            || std::fs::metadata(stderr.path()).is_ok_and(|m| m.len() > 1024 * 1024) {
            let _ = child.kill(); let _ = child.wait();
            return Err(format!("proof PNG decode canceled or exceeded its limits: {}", path.display()));
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => {
                return Err(format!("proof PNG cannot be decoded: {}",
                    std::fs::read_to_string(stderr.path()).unwrap_or_default()
                        .trim().chars().take(400).collect::<String>()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => { let _ = child.kill(); let _ = child.wait();
                return Err(format!("proof PNG decoder failed: {e}")); }
        }
    }
}

/// Validate every take before producing `CaptureInput` for the unchanged v1
/// package. Machine flags and human visual claims must be backed by exact
/// immutable files. This still requires an honest reviewer; it cannot infer
/// correct POV from metadata or a screenshot filename.
pub fn reviewed_package_inputs(request_path: &Path, receipt_path: &Path)
    -> Result<(String, Vec<CaptureInput>), String> {
    reviewed_package_inputs_with_cancel(request_path, receipt_path, || false)
}

pub fn reviewed_package_inputs_with_cancel(request_path: &Path, receipt_path: &Path,
    should_cancel: impl Fn() -> bool) -> Result<(String, Vec<CaptureInput>), String> {
    if should_cancel() { return Err("capture receipt validation canceled".into()); }
    let request_bytes = std::fs::read(request_path).map_err(|e| format!("cannot read capture request: {e}"))?;
    let request: CaptureRequest = serde_json::from_slice(&request_bytes)
        .map_err(|e| format!("invalid capture request: {e}"))?;
    let receipt_bytes = std::fs::read(receipt_path)
        .map_err(|e| format!("cannot read capture receipt: {e}"))?;
    if receipt_bytes.len() > 4 * 1024 * 1024 { return Err("capture receipt exceeds 4 MiB".into()); }
    let receipt_hash = format!("{:x}", Sha256::digest(&receipt_bytes));
    let receipt: CaptureReceipt = serde_json::from_slice(&receipt_bytes)
        .map_err(|e| format!("invalid capture receipt: {e}"))?;
    let request_hash = format!("{:x}", Sha256::digest(&request_bytes));
    if request.version != CAPTURE_REQUEST_VERSION || receipt.version != CAPTURE_RECEIPT_VERSION
        || receipt.request_sha256 != request_hash || receipt.demo_sha256 != request.demo_sha256
        || receipt.event_id != request.event_id {
        return Err("capture receipt does not bind this versioned request and demo event".into());
    }
    if file_identity_with_cancel(&request.demo_path, &should_cancel)?.0 != request.demo_sha256 {
        return Err("source demo changed since the capture request was saved".into());
    }
    if receipt.takes.len() != request.takes.len() {
        return Err("capture receipt does not cover every requested camera take".into());
    }
    let mut seen = HashSet::new();
    let mut inputs = Vec::with_capacity(request.takes.len());
    for (order, planned) in request.takes.iter().enumerate() {
        if should_cancel() { return Err("capture receipt validation canceled".into()); }
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
        complete_review(&take.camera_role, review)?;
        for frame in &review.proof_frames {
            if frame.tick < take.actual_start_tick || frame.tick > take.actual_end_tick
                || file_identity_with_cancel(&frame.path, &should_cancel)?.0 != frame.sha256 {
                return Err(format!("{} visual proof frame is unbound", take.camera_role));
            }
            decode_proof_png_with_cancel(&frame.path, &should_cancel)?;
        }
        if take.width != 1920 || take.height != 1080 || take.fps_num != 60 || take.fps_den != 1
            || take.duration_ms == 0 || take.duration_ms > 120_000
            || take.audio_channels == 0 || take.audio_sample_rate == 0 {
            return Err(format!("{} take is outside the 1080p60 pilot profile", take.camera_role));
        }
        if file_identity_with_cancel(&take.muxed_mp4, &should_cancel)? != (take.muxed_sha256.clone(), take.muxed_bytes)
            || file_identity_with_cancel(&take.hlae_wav, &should_cancel)? != (take.hlae_wav_sha256.clone(), take.hlae_wav_bytes) {
            return Err(format!("{} take or source WAV changed after QC", take.camera_role));
        }
        run_full_decode_with_cancel(&take.muxed_mp4, true, true, take.duration_ms, &should_cancel)?;
        run_full_decode_with_cancel(&take.hlae_wav, false, true, take.duration_ms, &should_cancel)?;
        if file_identity_with_cancel(&take.muxed_mp4, &should_cancel)? != (take.muxed_sha256.clone(), take.muxed_bytes)
            || file_identity_with_cancel(&take.hlae_wav, &should_cancel)? != (take.hlae_wav_sha256.clone(), take.hlae_wav_bytes) {
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
    require_receipt_unchanged(receipt_path, &receipt_hash, &should_cancel)?;
    let package_id = format!("demostrike-{}-e{}-r{}",
        &request.demo_sha256[..16], request.event_id, &receipt_hash[..16]);
    Ok((package_id, inputs))
}

pub fn seal_reviewed_capture(request_path: &Path, receipt_path: &Path, destination: &Path)
    -> Result<CapturePackage, String> {
    seal_reviewed_capture_with_cancel(request_path, receipt_path, destination, || false)
}

pub fn seal_reviewed_capture_with_cancel(request_path: &Path, receipt_path: &Path,
    destination: &Path, should_cancel: impl Fn() -> bool) -> Result<CapturePackage, String> {
    let deadline = Instant::now() + Duration::from_secs(20 * 60);
    let stop = || should_cancel() || Instant::now() >= deadline;
    let result = (|| {
        for path in [request_path, receipt_path] {
            if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > 4 * 1024 * 1024 {
                return Err(format!("review source exceeds 4 MiB: {}", path.display()));
            }
        }
        let receipt_hash = file_identity_with_cancel(receipt_path, &stop)?.0;
        let request_hash = file_identity_with_cancel(request_path, &stop)?.0;
        let (package_id, inputs) = reviewed_package_inputs_with_cancel(request_path, receipt_path, &stop)?;
        if !receipt_path.is_absolute() || !request_path.is_absolute() {
            return Err("source-bound sealing requires absolute receipt and request paths".into());
        }
        let binding = ReviewBinding { version: 2, package_id: package_id.clone(),
            request_path: request_path.to_path_buf(), request_sha256: request_hash.clone(),
            receipt_path: receipt_path.to_path_buf(), receipt_sha256: receipt_hash.clone(),
            request_file: Some("demostrike-review-request.json".into()),
            receipt_file: Some("demostrike-review-receipt.json".into()) };
        let stage = staged_package_path(destination);
        let check_sources = || {
            require_receipt_unchanged(receipt_path, &receipt_hash, &stop)
                .map_err(PackageError::Invalid)?;
            require_receipt_unchanged(request_path, &request_hash, &stop)
                .map_err(PackageError::Invalid)?;
            write_portable_review_evidence_before_commit(&stage, &binding)?;
            require_receipt_unchanged(receipt_path, &receipt_hash, &stop)
                .map_err(PackageError::Invalid)?;
            require_receipt_unchanged(request_path, &request_hash, &stop)
                .map_err(PackageError::Invalid)
        };
        require_receipt_unchanged(receipt_path, &receipt_hash, &stop)?;
        require_receipt_unchanged(request_path, &request_hash, &stop)?;
        finalize_stream_probed_capture_package_with_cancel_and_guard(destination, &package_id,
            &inputs, &stop, check_sources)
            .map_err(|e| e.to_string())
    })();
    if Instant::now() >= deadline {
        Err("capture receipt sealing exceeded its 20-minute time budget".into())
    } else { result }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_failed_write_and_cancel_leave_no_final_or_owned_temporary_file() {
        use std::cell::Cell;
        use std::io::{Error, Write};
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("reviewed.json");
        let error = write_receipt_atomically_with(&destination, b"complete", &|| false,
            |file, _| { file.write_all(b"partial")?; Err(Error::other("injected sync failure")) })
            .unwrap_err();
        assert!(error.contains("injected sync failure"));
        assert!(!destination.exists());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);

        let cancel = Cell::new(false);
        assert!(write_receipt_atomically_with(&destination, b"complete", &|| cancel.get(),
            |file, bytes| { file.write_all(bytes)?; file.sync_all()?; cancel.set(true); Ok(()) })
            .unwrap_err().contains("canceled"));
        assert!(!destination.exists());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);

        write_receipt_atomically_with(&destination, b"complete", &|| false,
            |file, bytes| { file.write_all(bytes)?; file.sync_all() }).unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), b"complete");
        assert!(write_receipt_atomically_with(&destination, b"new", &|| false,
            |file, bytes| file.write_all(bytes)).is_err());
        assert_eq!(std::fs::read(&destination).unwrap(), b"complete");
    }

    #[test]
    fn recorded_identity_builder_rejects_conflicting_or_missing_markers() {
        let marker = |tick| format!("DEMOSTRIKE_OBSERVE role=player_pov kind=release tick={tick} entity=6 name=player steamid=76561198000000001 observer=player observerSteamid=76561198000000001");
        let one = marker(1224);
        assert_eq!(observed_tick(&one, "player_pov", "release", 6,
            "76561198000000001").unwrap(), 1224);
        assert!(observed_tick(&format!("{one}\n{}", marker(1225)), "player_pov",
            "release", 6, "76561198000000001").is_err());
        assert!(observed_tick(&one.replace("observerSteamid=76561198000000001",
            "observerSteamid=76561198000000002"), "player_pov", "release", 6,
            "76561198000000001").is_err());
        assert!(observed_tick(&one, "projectile", "release", 6,
            "76561198000000001").is_err());
    }

    #[test]
    fn incomplete_human_review_never_becomes_accepted_qc() {
        let mut review = VisualReview {
            reviewed_by: "reviewer".into(), notes: "checked take".into(),
            review_extent: "continuous".into(), correct_player: true,
            release_observed: true, expected_utility_visible_in_pov: true,
            same_event_complete_flight: true, model_feet_stance_visible: true,
            clean_image: true, audible_game_audio: false,
            proof_frames: vec![ProofFrame { path: PathBuf::from("proof.png"),
                sha256: "hash".into(), tick: 3 }],
        };
        assert!(complete_review("player_pov", &review).unwrap_err().contains("incomplete"));
        review.audible_game_audio = true;
        assert!(complete_review("player_pov", &review).is_ok());
        review.same_event_complete_flight = false;
        assert!(complete_review("projectile", &review).is_err());
        review.same_event_complete_flight = true;
        review.review_extent = "spot-check".into();
        assert!(complete_review("projectile", &review).is_err());
    }

    #[test]
    fn cancelled_authoring_creates_no_receipt() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("reviewed.json");
        assert!(author_reviewed_receipt_with_cancel(Path::new("missing-request.json"),
            &[], &output, || true).unwrap_err().contains("canceled"));
        assert!(!output.exists());
    }

    #[test]
    fn changed_preview_media_cannot_author_a_receipt() {
        let dir = tempfile::tempdir().unwrap();
        let demo = dir.path().join("source.dem");
        std::fs::write(&demo, b"PBDEMS2\0source").unwrap();
        let video = dir.path().join("take.mp4");
        let wav = dir.path().join("take.wav");
        std::fs::write(&video, b"original video bytes").unwrap();
        std::fs::write(&wav, b"original wav bytes").unwrap();
        let (video_hash, wav_hash) = preview_identities_with_cancel(&video, &wav, || false).unwrap();
        let roles = ["player_pov", "projectile", "third_person_reference"];
        let request_path = dir.path().join("request.json");
        let request = serde_json::json!({
            "version": 1, "demoPath": demo, "demoSha256": file_identity(&demo).unwrap().0,
            "demoBytes": file_identity(&demo).unwrap().1, "matchId": 1, "eventId": 2,
            "map": "de_train", "round": 1, "playerSteamid64": "76561198000000001",
            "utilityType": "he", "projectileEntityId": 3, "observerSlotCandidate": 5,
            "tickRate": 64.0, "throwTick": 3, "detonateTick": 4,
            "throwPosition": {"x": 1.0, "y": 2.0, "z": 3.0},
            "detonatePosition": {"x": 4.0, "y": 5.0, "z": 6.0},
            "flightSamples": [[3.0,1.0,2.0,3.0],[4.0,4.0,5.0,6.0]],
            "takes": roles.iter().map(|role| serde_json::json!({
                "cameraRole": role, "startTick": 1, "endTick": 5
            })).collect::<Vec<_>>()
        });
        std::fs::write(&request_path, serde_json::to_vec(&request).unwrap()).unwrap();
        let takes: Vec<_> = roles.iter().map(|role| ReviewTakeDraft {
            camera_role: (*role).into(), muxed_mp4: video.display().to_string(),
            hlae_wav: wav.display().to_string(),
            runtime_identity_log: dir.path().join("not-yet-needed-identity.json").display().to_string(),
            preview_muxed_sha256: video_hash.clone(), preview_wav_sha256: wav_hash.clone(),
            ..Default::default()
        }).collect();
        std::fs::write(&video, b"changed video bytes").unwrap();
        let output = dir.path().join("reviewed.json");
        let error = author_reviewed_receipt_with_cancel(&request_path, &takes, &output,
            || false).unwrap_err();
        assert!(error.contains("changed since source-monitor review"), "{error}");
        assert!(!output.exists());
    }

    #[test]
    fn editing_receipt_invalidates_pending_seal_identity() {
        let dir = tempfile::tempdir().unwrap();
        let receipt = dir.path().join("receipt.json");
        std::fs::write(&receipt, br#"{"notes":"reviewed"}"#).unwrap();
        let original = file_identity(&receipt).unwrap().0;
        require_receipt_unchanged(&receipt, &original, &|| false).unwrap();
        std::fs::write(&receipt, br#"{"notes":"edited"}"#).unwrap();
        assert!(require_receipt_unchanged(&receipt, &original, &|| false)
            .unwrap_err().contains("changed before package commit"));
    }

    #[test]
    fn canceled_receipt_validation_never_reads_or_seals_sources() {
        let missing = Path::new("missing-request.json");
        assert!(reviewed_package_inputs_with_cancel(missing, missing, || true)
            .unwrap_err().contains("canceled"));
    }

    #[test]
    fn runtime_log_must_bind_live_userinfo_at_setup_and_release() {
        let dir = tempfile::tempdir().unwrap();
        let request: CaptureRequest = serde_json::from_value(serde_json::json!({
            "version": 1, "demoPath": "fixture.dem", "demoSha256": "sourcehash", "demoBytes": 10,
            "matchId": 1, "eventId": 6, "map": "de_train", "round": 2,
            "playerSteamid64": "76561198000000001", "utilityType": "he",
            "projectileEntityId": 140, "observerSlotCandidate": 5, "tickRate": 64.0,
            "throwTick": 1224, "detonateTick": 1334,
            "throwPosition": {"x": 1.0, "y": 2.0, "z": 3.0},
            "detonatePosition": {"x": 4.0, "y": 5.0, "z": 6.0},
            "flightSamples": [[1230.0, 1.0, 2.0, 3.0], [1333.0, 4.0, 5.0, 6.0]],
            "takes": [{"cameraRole": "player_pov", "startTick": 1000, "endTick": 1256}]
        })).unwrap();
        let path = dir.path().join("identity.json");
        let console_path = dir.path().join("console.log");
        let console = ["record_start_command 1000", "setup 1138", "release 1224",
            "record_end_command 1256", "record_end_callback 1256"].iter()
            .map(|item| {
                let (kind, tick) = item.split_once(' ').unwrap();
                format!("DEMOSTRIKE_OBSERVE role=player_pov kind={kind} tick={tick} entity=6 name=fixture_player steamid=76561198000000001 observer=fixture_player observerSteamid=76561198000000001")
            }).collect::<Vec<_>>().join("\n");
        std::fs::write(&console_path, &console).unwrap();
        let mut log = serde_json::json!({
            "version": 2, "requestSha256": "requesthash", "demoSha256": "sourcehash",
            "eventId": 6, "cameraRole": "player_pov", "issuedSpecPlayerValue": 6,
            "rawConsoleLog": console_path,
            "rawConsoleLogSha256": file_identity(&console_path).unwrap().0,
            "recordStartTick": 1000, "recordEndTick": 1256, "detonationTick": null,
            "setup": {"tick": 1138, "userinfoId": 5, "steamid64": "76561198000000001"},
            "release": {"tick": 1224, "userinfoId": 5, "steamid64": "76561198000000001"}
        });
        std::fs::write(&path, serde_json::to_vec(&log).unwrap()).unwrap();
        let mut take: CaptureTakeReceipt = serde_json::from_value(serde_json::json!({
            "cameraRole": "player_pov", "runtimeIdentityLog": path,
            "runtimeIdentityLogSha256": file_identity(&path).unwrap().0,
            "issuedSpecPlayerValue": 6, "resolvedPlayerSteamid64": "76561198000000001",
            "muxedMp4": "unused.mp4", "muxedSha256": "", "muxedBytes": 0,
            "hlaeWav": "unused.wav", "hlaeWavSha256": "", "hlaeWavBytes": 0,
            "actualStartTick": 1000, "actualEndTick": 1256, "durationMs": 4000,
            "width": 1920, "height": 1080, "fpsNum": 60, "fpsDen": 1,
            "audioChannels": 2, "audioSampleRate": 48000,
            "fullVideoDecode": false, "fullAudioDecode": false,
            "measuredAudioSignal": false, "visualReview": null
        })).unwrap();
        validate_runtime_identity_log(&path, "requesthash", &request, &take).unwrap();
        log["release"]["steamid64"] = serde_json::json!("76561198000000002");
        std::fs::write(&path, serde_json::to_vec(&log).unwrap()).unwrap();
        take.runtime_identity_log_sha256 = file_identity(&path).unwrap().0;
        assert!(validate_runtime_identity_log(&path, "requesthash", &request, &take)
            .unwrap_err().contains("do not match"));
        log["release"]["steamid64"] = serde_json::json!("76561198000000001");
        std::fs::write(&path, serde_json::to_vec(&log).unwrap()).unwrap();
        take.runtime_identity_log_sha256 = file_identity(&path).unwrap().0;
        std::fs::write(&console_path, console.replace("observerSteamid=76561198000000001",
            "observerSteamid=76561198000000002")).unwrap();
        log["rawConsoleLogSha256"] = serde_json::json!(file_identity(&console_path).unwrap().0);
        std::fs::write(&path, serde_json::to_vec(&log).unwrap()).unwrap();
        take.runtime_identity_log_sha256 = file_identity(&path).unwrap().0;
        assert!(validate_runtime_identity_log(&path, "requesthash", &request, &take)
            .unwrap_err().contains("live"));
    }

    #[test]
    fn existing_hlae_sample_keeps_video_only_source_out_of_muxed_acceptance() {
        let (Some(video), Some(wav)) = (
            std::env::var_os("DEMOSTRIKE_MEDIA_SAMPLE_VIDEO"),
            std::env::var_os("DEMOSTRIKE_MEDIA_SAMPLE_WAV"),
        ) else { return };
        let video = PathBuf::from(video);
        let wav = PathBuf::from(wav);
        assert_eq!(file_identity(&video).unwrap().0,
            "06fd75aaf4efcfd1391a9cd1edaf2d2e2487ec6f363e0eb59e7d2836a37d7444");
        assert_eq!(file_identity(&wav).unwrap().0,
            "f583b197738435bc7fe0aabf51d24b03061d990bd4e2c6ab5a6eed4109fa553f");
        assert!(run_full_decode(&video, true, false, 8517).unwrap_err().contains("full decode failed"));
        assert!(run_full_decode(&wav, false, true, 8510).unwrap().unwrap() > -60.0);
        let temp = tempfile::tempdir().unwrap();
        let muxed = temp.path().join("technical-only.mp4");
        let status = Command::new(crate::media_tools::ffmpeg_program())
            .args(["-nostdin", "-hide_banner", "-v", "error", "-y"])
            .arg("-i").arg(&video).arg("-i").arg(&wav)
            .args(["-map", "0:v:0", "-map", "1:a:0", "-c:v", "copy",
                "-c:a", "aac", "-b:a", "192k", "-shortest"])
            .arg(&muxed).status().unwrap();
        assert!(status.success());
        assert!(run_full_decode(&muxed, true, true, 8517).unwrap().unwrap() > -60.0);
    }
}
