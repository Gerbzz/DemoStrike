//! Supervised, bounded invocation of the existing event-pilot QC/mux script.
//! Technical measurements never set a human capture-review decision.
use crate::event_pilot::EventPilotManifest;
use crate::owned_capture::{OwnedLaunch, run_owned_with_monitor};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const MIN_FREE_BYTES: u64 = 20 * 1024 * 1024 * 1024;
const MAX_INPUT_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const DIAGNOSTIC_LIMIT: u64 = 8 * 1024 * 1024;
const OUTPUT_ALLOWANCE: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct QcPlan {
    pub pilot_root: PathBuf,
    pub request_path: PathBuf,
    pub request_sha256: String,
    pub review_root: PathBuf,
    pub review_name: String,
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub output_budget: u64,
}

#[derive(Debug, Clone)]
pub struct QcTake {
    pub role: String,
    pub muxed_mp4: PathBuf,
    pub hlae_wav: PathBuf,
}

#[derive(Debug, Clone)]
pub struct QcResult {
    pub review_root: PathBuf,
    pub takes: Vec<QcTake>,
}

fn review_name_valid(name: &str) -> bool {
    name == "review" || name.strip_prefix("review-").is_some_and(|suffix|
        !suffix.is_empty() && suffix.len() <= 48
        && suffix.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'))
}

#[cfg(windows)]
fn free_bytes(path: &Path) -> Result<u64, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let mut available = 0_u64;
    // SAFETY: `wide` is a NUL-terminated directory path and the remaining
    // pointers are valid writable u64 outputs for this call.
    if unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut available,
        std::ptr::null_mut(), std::ptr::null_mut()) } == 0 {
        return Err(format!("cannot check QC disk headroom: {}", std::io::Error::last_os_error()));
    }
    Ok(available)
}

#[cfg(not(windows))]
fn free_bytes(_path: &Path) -> Result<u64, String> {
    Err("event-pilot QC requires Windows".into())
}

fn bounded_size(path: &Path, label: &str) -> Result<u64, String> {
    let size = std::fs::metadata(path)
        .map_err(|e| format!("missing {label} {}: {e}", path.display()))?.len();
    if size == 0 || size > MAX_INPUT_BYTES {
        return Err(format!("{label} is empty or exceeds 8 GiB: {}", path.display()));
    }
    Ok(size)
}

pub fn plan_qc(manifest_path: &Path, request_path: &Path, review_name: &str,
    ffmpeg: &Path, ffprobe: &Path) -> Result<QcPlan, String> {
    if !review_name_valid(review_name) { return Err("choose review or a bounded review-* name".into()); }
    if !manifest_path.is_absolute() || !request_path.is_absolute()
        || !ffmpeg.is_absolute() || !ffprobe.is_absolute()
        || !ffmpeg.is_file() || !ffprobe.is_file() {
        return Err("select an absolute pilot manifest, FFmpeg and FFprobe executable".into());
    }
    let bytes = std::fs::read(manifest_path).map_err(|e| format!("pilot manifest: {e}"))?;
    if bytes.len() > 1024 * 1024 { return Err("pilot manifest exceeds 1 MiB".into()); }
    let manifest: EventPilotManifest = serde_json::from_slice(&bytes)
        .map_err(|e| format!("invalid pilot manifest: {e}"))?;
    if request_path.canonicalize().ok() != manifest.request_path.canonicalize().ok() {
        return Err("selected capture request differs from the staged pilot request".into());
    }
    let request_bytes = std::fs::read(request_path)
        .map_err(|e| format!("capture request: {e}"))?;
    if request_bytes.len() > 4 * 1024 * 1024
        || format!("{:x}", Sha256::digest(&request_bytes)) != manifest.request_sha256 {
        return Err("capture request changed since pilot staging".into());
    }
    let resolved_pilot_root = manifest.output_root.canonicalize()
        .map_err(|e| format!("pilot root: {e}"))?;
    if manifest.version != 4
        || manifest_path.parent().and_then(|p| p.canonicalize().ok()).as_deref() != Some(resolved_pilot_root.as_path()) {
        return Err("QC requires the version-4 pilot manifest in its output root".into());
    }
    // Keep the original absolute spelling for PowerShell's path ownership
    // check and its media-qc.json paths; canonicalization can add Windows' \\?\ prefix.
    let pilot_root = manifest_path.parent().ok_or("pilot manifest has no parent")?.to_path_buf();
    let console = pilot_root.join("runtime-console.log");
    if manifest.runtime_console_path.canonicalize().ok().as_deref()
        != console.canonicalize().ok().as_deref() {
        return Err("pilot manifest points to a different runtime console".into());
    }
    if bounded_size(&console, "runtime console")? > 16 * 1024 * 1024 {
        return Err("runtime console exceeds 16 MiB".into());
    }
    let review_root = pilot_root.join(review_name);
    if review_root.exists() { return Err("QC review folder already exists; choose a new review-* name".into()); }
    let mut source_bytes = 0_u64;
    for name in ["pov", "projectile", "third"] {
        let raw = pilot_root.join(format!("raw-{name}/take0000"));
        let video = raw.join("dsEventClean/video.mp4");
        let wav = raw.join("audio.wav");
        if !video.canonicalize().map_err(|e| e.to_string())?.starts_with(&resolved_pilot_root)
            || !wav.canonicalize().map_err(|e| e.to_string())?.starts_with(&resolved_pilot_root) {
            return Err(format!("{name} raw media resolves outside the pilot folder"));
        }
        let video_bytes = bounded_size(&video, "raw video")?;
        let wav_bytes = bounded_size(&wav, "HLAE WAV")?;
        source_bytes = source_bytes.checked_add(video_bytes)
            .and_then(|bytes| bytes.checked_add(wav_bytes))
            .ok_or("QC source sizes exceed the supported bound")?;
    }
    let budget = source_bytes.checked_add(OUTPUT_ALLOWANCE).ok_or("QC output budget overflow")?;
    if free_bytes(&pilot_root)? < MIN_FREE_BYTES.saturating_add(budget) {
        return Err(format!("QC needs its {} MiB output budget plus 20 GiB free headroom",
            budget.div_ceil(1024 * 1024)));
    }
    Ok(QcPlan { pilot_root, request_path: request_path.to_path_buf(),
        request_sha256: manifest.request_sha256, review_root, review_name: review_name.into(),
        ffmpeg: ffmpeg.to_path_buf(), ffprobe: ffprobe.to_path_buf(), output_budget: budget })
}

fn output_size_and_stage(plan: &QcPlan) -> Result<(u64, String), String> {
    if !plan.review_root.exists() { return Ok((0, "Waiting for QC output folder".into())); }
    let mut total = 0_u64;
    let mut latest = "Checking raw takes".to_string();
    for entry in std::fs::read_dir(&plan.review_root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let meta = entry.metadata().map_err(|e| e.to_string())?;
        if !meta.is_file() { return Err("QC wrote an unexpected directory in its review folder".into()); }
        let name = entry.file_name().to_string_lossy().into_owned();
        if (name.ends_with(".log") || name == "qc-error.txt") && meta.len() > DIAGNOSTIC_LIMIT {
            return Err(format!("QC diagnostic exceeds 8 MiB: {name}"));
        }
        if name.ends_with(".mp4") {
            latest = format!("Muxing or decoding {name}");
        }
        total = total.checked_add(meta.len()).ok_or("QC output size overflow")?;
        if total > plan.output_budget { return Err("QC output exceeded its input-bound budget".into()); }
    }
    Ok((total, latest))
}

pub fn run_qc_with_cancel(plan: &QcPlan, cancel: &AtomicBool,
    mut progress: impl FnMut(String)) -> Result<QcResult, String> {
    if cancel.load(Ordering::Relaxed) { return Err("media QC canceled before launch".into()); }
    if plan.review_root.exists() { return Err("QC review folder already exists".into()); }
    let powershell = PathBuf::from(std::env::var_os("SystemRoot")
        .ok_or("Windows SystemRoot is unavailable")?)
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let script = crate::runtime_resources::qc_script_path()?;
    let launch = OwnedLaunch { program: powershell,
        current_dir: plan.pilot_root.clone(), environment: Vec::new(),
        args: [OsString::from("-NoProfile"), OsString::from("-NonInteractive"),
            OsString::from("-File"), script.into_os_string(),
            OsString::from("-PilotRoot"), plan.pilot_root.clone().into_os_string(),
            OsString::from("-Ffmpeg"), plan.ffmpeg.clone().into_os_string(),
            OsString::from("-Ffprobe"), plan.ffprobe.clone().into_os_string(),
            OsString::from("-ReviewName"), OsString::from(&plan.review_name)].into(),
    };
    let mut prior = String::new();
    let mut last_update = Instant::now();
    let run_result = run_owned_with_monitor(launch, cancel, Duration::from_secs(25 * 60),
        "Capture media QC", &mut progress, || {
            let (size, stage) = output_size_and_stage(plan)?;
            if free_bytes(&plan.pilot_root)? < MIN_FREE_BYTES {
                return Err("free space fell below the 20 GiB floor".into());
            }
            if stage != prior || last_update.elapsed() >= Duration::from_secs(5) {
                prior = stage.clone();
                last_update = Instant::now();
                return Ok(Some(format!("{stage}; {} / {} MiB output",
                    size / (1024 * 1024), plan.output_budget / (1024 * 1024))));
            }
            Ok(None)
        });
    if let Err(error) = run_result {
        let diagnostic = plan.review_root.join("qc-error.txt");
        let detail = std::fs::metadata(&diagnostic).ok()
            .filter(|meta| meta.len() <= 4096)
            .and_then(|_| std::fs::read_to_string(&diagnostic).ok());
        return Err(match detail {
            Some(detail) if !detail.trim().is_empty() => format!("{error}: {}", detail.trim()),
            _ => error,
        });
    }
    let request_bytes = std::fs::read(&plan.request_path)
        .map_err(|e| format!("capture request after QC: {e}"))?;
    if request_bytes.len() > 4 * 1024 * 1024
        || format!("{:x}", Sha256::digest(&request_bytes)) != plan.request_sha256 {
        return Err("capture request changed during technical QC; outputs remain unreviewed".into());
    }
    let (size, _) = output_size_and_stage(plan)?;
    if size == 0 || size > plan.output_budget { return Err("QC output is absent or oversized".into()); }
    let summary = plan.review_root.join("media-qc.json");
    let bytes = std::fs::read(&summary).map_err(|e| format!("QC summary missing: {e}"))?;
    if bytes.len() > 1024 * 1024 { return Err("QC summary exceeds 1 MiB".into()); }
    let entries: Vec<serde_json::Value> = serde_json::from_slice(&bytes)
        .map_err(|e| format!("invalid QC summary: {e}"))?;
    if entries.len() != 3 { return Err("QC summary does not contain three roles".into()); }
    let mut takes = Vec::new();
    for (name, role) in [("pov", "player_pov"), ("projectile", "projectile"),
        ("third", "third_person_reference")] {
        let muxed_mp4 = plan.review_root.join(format!("{name}.mp4"));
        let hlae_wav = plan.pilot_root.join(format!("raw-{name}/take0000/audio.wav"));
        if !muxed_mp4.is_file() || !hlae_wav.is_file()
            || !entries.iter().any(|entry| entry.get("role").and_then(|value| value.as_str()) == Some(name)
                && entry.get("muxed").and_then(|value| value.as_str())
                    .and_then(|path| Path::new(path).canonicalize().ok())
                    == muxed_mp4.canonicalize().ok()) {
            return Err(format!("QC summary or muxed file is missing for {role}"));
        }
        takes.push(QcTake { role: role.into(), muxed_mp4, hlae_wav });
    }
    progress("QC/mux completed. All three takes remain unreviewed; inspect the videos and listen before authoring a receipt.".into());
    Ok(QcResult { review_root: plan.review_root.clone(), takes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_output_name_is_a_single_bounded_child() {
        assert!(review_name_valid("review"));
        assert!(review_name_valid("review-second_attempt"));
        let long = format!("review-{}", "a".repeat(49));
        for name in ["../review", "review/elsewhere", "review-", "Review", "review-?", &long] {
            assert!(!review_name_valid(name));
        }
    }

    #[test]
    fn canceled_qc_never_starts_powershell_or_creates_output() {
        let dir = tempfile::tempdir().unwrap();
        let plan = QcPlan { pilot_root: dir.path().to_path_buf(),
            request_path: dir.path().join("request.json"), request_sha256: String::new(),
            review_root: dir.path().join("review"), review_name: "review".into(),
            ffmpeg: PathBuf::from("missing-ffmpeg"), ffprobe: PathBuf::from("missing-ffprobe"),
            output_budget: OUTPUT_ALLOWANCE };
        let cancel = AtomicBool::new(true);
        assert!(run_qc_with_cancel(&plan, &cancel, |_| {}).unwrap_err().contains("canceled"));
        assert!(!plan.review_root.exists());
    }

    #[test]
    fn oversized_qc_diagnostic_stops_monitor_before_acceptance() {
        let dir = tempfile::tempdir().unwrap();
        let review_root = dir.path().join("review");
        std::fs::create_dir(&review_root).unwrap();
        let log = std::fs::File::create(review_root.join("pov-raw-video-decode.log")).unwrap();
        log.set_len(DIAGNOSTIC_LIMIT + 1).unwrap();
        let plan = QcPlan { pilot_root: dir.path().to_path_buf(), review_root,
            request_path: dir.path().join("request.json"), request_sha256: String::new(),
            review_name: "review".into(), ffmpeg: PathBuf::new(), ffprobe: PathBuf::new(),
            output_budget: OUTPUT_ALLOWANCE };
        assert!(output_size_and_stage(&plan).unwrap_err().contains("diagnostic exceeds"));
    }

    #[test]
    #[ignore = "opt-in tiny synthetic raw takes; set DEMOSTRIKE_QC_SMOKE_* paths to an unused local pilot"]
    fn synthetic_raw_takes_mux_in_owned_job_without_human_approval() {
        let get = |name| PathBuf::from(std::env::var_os(name).expect(name));
        let manifest = get("DEMOSTRIKE_QC_SMOKE_MANIFEST");
        let request = get("DEMOSTRIKE_QC_SMOKE_REQUEST");
        let ffmpeg = get("DEMOSTRIKE_QC_SMOKE_FFMPEG");
        let ffprobe = get("DEMOSTRIKE_QC_SMOKE_FFPROBE");
        let review_name = std::env::var("DEMOSTRIKE_QC_SMOKE_REVIEW_NAME")
            .unwrap_or_else(|_| "review".to_string());
        let plan = plan_qc(&manifest, &request, &review_name, &ffmpeg, &ffprobe).unwrap();
        let result = run_qc_with_cancel(&plan, &AtomicBool::new(false), |message|
            println!("{message}")).unwrap();
        assert_eq!(result.takes.len(), 3);
        for take in &result.takes {
            assert!(take.muxed_mp4.is_file());
            assert!(take.hlae_wav.is_file());
        }
        let draft = crate::capture_receipt::draft_takes_for_request(&request).unwrap();
        assert_eq!(draft.len(), 3);
        assert!(draft.iter().all(|take| !take.correct_player && !take.audible_game_audio
            && take.preview_muxed_sha256.is_empty()));
    }
}
