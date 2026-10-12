use clipstrike_core::capture_package::{CaptureClip, PackageError};
use serde::Deserialize;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub struct VideoProbe {
    pub width: u32,
    pub height: u32,
    pub fps_num: u32,
    pub fps_den: u32,
    pub duration_seconds: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MuxedEditorCoverage {
    pub video_duration_ms: u64,
    pub audio_duration_ms: u64,
    pub visual_duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioProbe {
    pub channels: u16,
    pub sample_rate: u32,
    pub duration_seconds: f64,
}

#[derive(Debug, Deserialize)]
struct ProbeResult {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    format: ProbeFormat,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    codec_type: String,
    width: Option<u32>,
    height: Option<u32>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
    channels: Option<u16>,
    sample_rate: Option<String>,
    start_time: Option<String>,
    duration: Option<String>,
    duration_ts: Option<u64>,
    time_base: Option<String>,
    nb_frames: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FrameProbeResult {
    #[serde(default)]
    frames: Vec<ProbeFrame>,
}

#[derive(Debug, Deserialize)]
struct ProbeFrame {
    best_effort_timestamp_time: Option<String>,
}

const PROBE_TIMEOUT: Duration = Duration::from_secs(60);
const PROBE_STDOUT_LIMIT: u64 = 16 * 1024 * 1024;
const PROBE_STDERR_LIMIT: u64 = 64 * 1024;

fn kill_and_reap(child: &mut Child) -> Result<(), PackageError> {
    if child.try_wait().map_err(|error| PackageError::Io(error.to_string()))?.is_some() {
        return Ok(());
    }
    if let Err(error) = child.kill() {
        if child.try_wait().ok().flatten().is_some() { return Ok(()); }
        return Err(PackageError::Io(format!("could not stop media probe: {error}")));
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => return Err(PackageError::Io("media probe did not exit within two seconds after cancellation".into())),
            Err(error) => return Err(PackageError::Io(format!("could not reap media probe: {error}"))),
        }
    }
}

fn run_probe_command(
    command: &mut Command,
    should_cancel: impl Fn() -> bool,
) -> Result<Vec<u8>, PackageError> {
    run_probe_command_bounded(command, should_cancel, PROBE_TIMEOUT, PROBE_STDOUT_LIMIT, PROBE_STDERR_LIMIT)
}

fn run_probe_command_bounded(
    command: &mut Command,
    should_cancel: impl Fn() -> bool,
    timeout: Duration,
    stdout_limit: u64,
    stderr_limit: u64,
) -> Result<Vec<u8>, PackageError> {
    if should_cancel() { return Err(PackageError::Cancelled("media probe cancelled".into())); }
    let stdout = tempfile::NamedTempFile::new()
        .map_err(|error| PackageError::Io(error.to_string()))?;
    let stderr = tempfile::NamedTempFile::new()
        .map_err(|error| PackageError::Io(error.to_string()))?;
    let mut child = command
        .stdout(Stdio::from(stdout.reopen().map_err(|error| PackageError::Io(error.to_string()))?))
        .stderr(Stdio::from(stderr.reopen().map_err(|error| PackageError::Io(error.to_string()))?))
        .spawn().map_err(|error| PackageError::Io(format!("media probe: {error}")))?;
    let deadline = Instant::now() + timeout;
    let status = loop {
        if should_cancel() {
            kill_and_reap(&mut child)?;
            return Err(PackageError::Cancelled("media probe cancelled; child process stopped".into()));
        }
        let sizes = stdout.as_file().metadata().and_then(|out| {
            stderr.as_file().metadata().map(|err| (out.len(), err.len()))
        });
        let (stdout_len, stderr_len) = match sizes {
            Ok(sizes) => sizes,
            Err(error) => {
                kill_and_reap(&mut child)?;
                return Err(PackageError::Io(format!("media probe output status: {error}")));
            }
        };
        if stdout_len > stdout_limit || stderr_len > stderr_limit {
            kill_and_reap(&mut child)?;
            return Err(PackageError::Invalid("media probe output exceeded its byte limit".into()));
        }
        if Instant::now() >= deadline {
            kill_and_reap(&mut child)?;
            return Err(PackageError::Invalid(format!(
                "media probe exceeded its {} ms deadline", timeout.as_millis()
            )));
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(error) => {
                let _ = kill_and_reap(&mut child);
                return Err(PackageError::Io(format!("media probe wait failed: {error}")));
            }
        }
    };
    if should_cancel() { return Err(PackageError::Cancelled("media probe cancelled".into())); }
    if stdout.as_file().metadata().map_err(|error| PackageError::Io(error.to_string()))?.len() > stdout_limit
        || stderr.as_file().metadata().map_err(|error| PackageError::Io(error.to_string()))?.len() > stderr_limit {
        return Err(PackageError::Invalid("media probe output exceeded its byte limit".into()));
    }
    if !status.success() {
        let detail = std::fs::read_to_string(stderr.path()).unwrap_or_default();
        return Err(PackageError::Invalid(format!(
            "media probe failed: {}",
            detail.trim().chars().take(2000).collect::<String>()
        )));
    }
    std::fs::read(stdout.path()).map_err(|error| PackageError::Io(error.to_string()))
}

fn probe_with_cancel(path: &Path, should_cancel: impl Fn() -> bool) -> Result<ProbeResult, PackageError> {
    let mut command = Command::new(crate::media_tools::ffprobe_program());
    command.args(["-v", "error", "-show_entries", "stream=codec_type,width,height,avg_frame_rate,r_frame_rate,sample_rate,channels,start_time,duration,duration_ts,time_base,nb_frames:format=duration", "-of", "json"])
        .arg(path);
    let bytes = run_probe_command(&mut command, should_cancel)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| PackageError::Json(format!("ffprobe output: {error}")))
}

fn probe_video_frames_with_cancel(
    path: &Path,
    should_cancel: impl Fn() -> bool,
) -> Result<FrameProbeResult, PackageError> {
    let mut command = Command::new(crate::media_tools::ffprobe_program());
    command.args(["-v", "error", "-select_streams", "v:0", "-show_frames",
        "-show_entries", "frame=best_effort_timestamp_time", "-of", "json"])
        .arg(path);
    let bytes = run_probe_command(&mut command, should_cancel)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| PackageError::Json(format!("ffprobe frame output: {error}")))
}

fn duration(result: &ProbeResult) -> Result<f64, PackageError> {
    let seconds = result
        .format
        .duration
        .as_deref()
        .unwrap_or("")
        .parse::<f64>()
        .map_err(|_| PackageError::Invalid("media duration unavailable".into()))?;
    if !seconds.is_finite() || seconds <= 0.0 {
        return Err(PackageError::Invalid(
            "media duration must be positive".into(),
        ));
    }
    Ok(seconds)
}

fn fps(value: &str) -> Result<(u32, u32), PackageError> {
    let (num, den) = value
        .split_once('/')
        .ok_or_else(|| PackageError::Invalid("frame rate is not rational".into()))?;
    let num = num
        .parse::<u32>()
        .map_err(|_| PackageError::Invalid("invalid frame rate".into()))?;
    let den = den
        .parse::<u32>()
        .map_err(|_| PackageError::Invalid("invalid frame rate".into()))?;
    if num == 0 || den == 0 {
        return Err(PackageError::Invalid("zero frame rate".into()));
    }
    Ok((num, den))
}

fn video(result: &ProbeResult) -> Result<VideoProbe, PackageError> {
    let stream = result
        .streams
        .iter()
        .find(|stream| stream.codec_type == "video")
        .ok_or_else(|| PackageError::Invalid("video stream missing".into()))?;
    let rate = stream
        .avg_frame_rate
        .as_deref()
        .filter(|rate| *rate != "0/0")
        .or(stream.r_frame_rate.as_deref())
        .unwrap_or("");
    let (fps_num, fps_den) = fps(rate)?;
    let width = stream
        .width
        .filter(|width| *width > 0)
        .ok_or_else(|| PackageError::Invalid("video width missing".into()))?;
    let height = stream
        .height
        .filter(|height| *height > 0)
        .ok_or_else(|| PackageError::Invalid("video height missing".into()))?;
    Ok(VideoProbe {
        width,
        height,
        fps_num,
        fps_den,
        duration_seconds: duration(result)?,
    })
}

fn audio(result: &ProbeResult) -> Result<AudioProbe, PackageError> {
    let stream = result
        .streams
        .iter()
        .find(|stream| stream.codec_type == "audio")
        .ok_or_else(|| PackageError::Invalid("audio stream missing".into()))?;
    let channels = stream
        .channels
        .filter(|channels| *channels > 0)
        .ok_or_else(|| PackageError::Invalid("audio channel count missing".into()))?;
    let sample_rate = stream
        .sample_rate
        .as_deref()
        .unwrap_or("")
        .parse::<u32>()
        .map_err(|_| PackageError::Invalid("audio sample rate missing".into()))?;
    if sample_rate == 0 {
        return Err(PackageError::Invalid("zero audio sample rate".into()));
    }
    Ok(AudioProbe {
        channels,
        sample_rate,
        duration_seconds: duration(result)?,
    })
}

/// Probes the existing HLAE video and separate WAV. This checks stream headers,
/// not audible content, demo identity, or a complete decode.
pub fn probe_capture_pair(
    video_path: &Path,
    wav_path: &Path,
) -> Result<(VideoProbe, AudioProbe), PackageError> {
    probe_capture_pair_with_cancel(video_path, wav_path, || false)
}

pub fn probe_capture_pair_with_cancel(
    video_path: &Path,
    wav_path: &Path,
    should_cancel: impl Fn() -> bool,
) -> Result<(VideoProbe, AudioProbe), PackageError> {
    let video = video(&probe_with_cancel(video_path, &should_cancel)?)?;
    let audio = audio(&probe_with_cancel(wav_path, &should_cancel)?)?;
    if (video.duration_seconds - audio.duration_seconds).abs() > 0.25 {
        return Err(PackageError::Invalid(
            "video and WAV durations differ by more than 250 ms".into(),
        ));
    }
    Ok((video, audio))
}

/// Require a playable container with both declared streams before labeling a
/// package stream-probed. Caller assertions alone are insufficient.
pub fn probe_muxed_clip(path: &Path, declared: &CaptureClip) -> Result<(), PackageError> {
    probe_muxed_clip_with_cancel(path, declared, || false)
}

fn validate_muxed_clip(result: &ProbeResult, declared: &CaptureClip) -> Result<(), PackageError> {
    let video = video(result)?;
    let audio = audio(result)?;
    if video.width != declared.width
        || video.height != declared.height
        || u64::from(video.fps_num) * u64::from(declared.fps_den)
            != u64::from(declared.fps_num) * u64::from(video.fps_den)
        || audio.channels != declared.audio.channels
        || audio.sample_rate != declared.audio.sample_rate
        || (video.duration_seconds * 1000.0 - declared.duration_ms as f64).abs() > 100.0
    {
        return Err(PackageError::Invalid(format!(
            "media streams differ from declared metadata: {}",
            declared.clip_id
        )));
    }
    Ok(())
}

pub fn probe_muxed_clip_with_cancel(
    path: &Path,
    declared: &CaptureClip,
    should_cancel: impl Fn() -> bool,
) -> Result<(), PackageError> {
    let result = probe_with_cancel(path, should_cancel)?;
    validate_muxed_clip(&result, declared)
}

fn stream_coverage_ms(stream: &ProbeStream) -> Result<u64, PackageError> {
    let label = stream.codec_type.as_str();
    let start = stream.start_time.as_deref().unwrap_or("")
        .parse::<f64>()
        .map_err(|_| PackageError::Invalid(format!("{label} stream start time unavailable")))?;
    if !start.is_finite() || start.abs() > 0.0005 {
        return Err(PackageError::Invalid(format!(
            "{label} stream has a nonzero start time; editor alignment is unsupported"
        )));
    }
    let milliseconds = match (stream.duration_ts, stream.time_base.as_deref()) {
        (Some(ticks), Some(base)) => {
            let (num, den) = fps(base).map_err(|_| {
                PackageError::Invalid(format!("{label} stream time base is invalid"))
            })?;
            let scaled = u128::from(ticks)
                .checked_mul(u128::from(num))
                .and_then(|value| value.checked_mul(1000))
                .ok_or_else(|| PackageError::Invalid(format!("{label} stream duration overflow")))?;
            u64::try_from(scaled / u128::from(den))
                .map_err(|_| PackageError::Invalid(format!("{label} stream duration overflow")))?
        }
        _ => {
            let seconds = stream.duration.as_deref().unwrap_or("")
                .parse::<f64>()
                .map_err(|_| PackageError::Invalid(format!("{label} stream duration unavailable")))?;
            if !seconds.is_finite() || seconds <= 0.0 || seconds * 1000.0 >= u64::MAX as f64 {
                return Err(PackageError::Invalid(format!("{label} stream duration is invalid")));
            }
            (seconds * 1000.0).floor() as u64
        }
    };
    if milliseconds == 0 {
        return Err(PackageError::Invalid(format!("{label} stream has no millisecond coverage")));
    }
    if let Some(seconds) = stream.duration.as_deref() {
        let seconds = seconds.parse::<f64>()
            .map_err(|_| PackageError::Invalid(format!("{label} stream duration is invalid")))?;
        if !seconds.is_finite() || (seconds * 1000.0 - milliseconds as f64).abs() > 1.01 {
            return Err(PackageError::Invalid(format!(
                "{label} stream duration fields disagree"
            )));
        }
    }
    Ok(milliseconds)
}

fn muxed_editor_coverage(
    result: &ProbeResult,
    declared: &CaptureClip,
) -> Result<MuxedEditorCoverage, PackageError> {
    validate_muxed_clip(result, declared)?;
    if result.streams.iter().filter(|stream| stream.codec_type == "video").count() != 1
        || result.streams.iter().filter(|stream| stream.codec_type == "audio").count() != 1
    {
        return Err(PackageError::Invalid(
            "editor import requires exactly one video and one audio stream".into(),
        ));
    }
    let video = result.streams.iter().find(|stream| stream.codec_type == "video")
        .ok_or_else(|| PackageError::Invalid("video stream missing".into()))?;
    let audio = result.streams.iter().find(|stream| stream.codec_type == "audio")
        .ok_or_else(|| PackageError::Invalid("audio stream missing".into()))?;
    let video_duration_ms = stream_coverage_ms(video)?;
    let audio_duration_ms = stream_coverage_ms(audio)?;
    if video_duration_ms.abs_diff(declared.duration_ms) > 100
        || audio_duration_ms.abs_diff(declared.duration_ms) > 100
    {
        return Err(PackageError::Invalid(format!(
            "video or audio stream differs from accepted container by more than 100 ms: {}",
            declared.clip_id
        )));
    }
    let frame_count = video.nb_frames.as_deref().unwrap_or("")
        .parse::<u64>()
        .map_err(|_| PackageError::Invalid(
            "video frame count unavailable; exact editor coverage is unsupported".into()
        ))?;
    if frame_count == 0 {
        return Err(PackageError::Invalid("video stream has no frames".into()));
    }
    let rate = video.avg_frame_rate.as_deref()
        .filter(|rate| *rate != "0/0")
        .or(video.r_frame_rate.as_deref())
        .unwrap_or("");
    let (fps_num, fps_den) = fps(rate)?;
    let frames_ms = frame_count as f64 * 1000.0 * fps_den as f64 / fps_num as f64;
    let frame_ms = 1000.0 * fps_den as f64 / fps_num as f64;
    if (frames_ms - video_duration_ms as f64).abs() > frame_ms + 1.0 {
        return Err(PackageError::Invalid(
            "video frame count and stream duration disagree; editor coverage is unsupported".into()
        ));
    }
    if audio_duration_ms < video_duration_ms {
        return Err(PackageError::Invalid(format!(
            "audio ends before video for {}; silence fill is not supported by this importer",
            declared.clip_id
        )));
    }
    if declared.duration_ms < video_duration_ms {
        return Err(PackageError::Invalid(format!(
            "accepted container duration ends before video for {}",
            declared.clip_id
        )));
    }
    Ok(MuxedEditorCoverage {
        video_duration_ms,
        audio_duration_ms,
        visual_duration_ms: video_duration_ms,
    })
}

fn validate_constant_frame_coverage(
    result: &ProbeResult,
    frames: &FrameProbeResult,
    expected_duration_ms: u64,
) -> Result<(), PackageError> {
    let video = result.streams.iter().find(|stream| stream.codec_type == "video")
        .ok_or_else(|| PackageError::Invalid("video stream missing".into()))?;
    let declared_count = video.nb_frames.as_deref().unwrap_or("").parse::<usize>()
        .map_err(|_| PackageError::Invalid("video frame count unavailable".into()))?;
    if frames.frames.len() != declared_count {
        return Err(PackageError::Invalid(
            "decoded frame count differs from video stream count".into()
        ));
    }
    let rate = video.avg_frame_rate.as_deref()
        .filter(|rate| *rate != "0/0")
        .or(video.r_frame_rate.as_deref()).unwrap_or("");
    let (num, den) = fps(rate)?;
    if num % den != 0 {
        return Err(PackageError::Invalid(
            "fractional FPS capture is not supported by the millisecond editor timeline".into()
        ));
    }
    let frame_seconds = f64::from(den) / f64::from(num);
    for (index, frame) in frames.frames.iter().enumerate() {
        let actual = frame.best_effort_timestamp_time.as_deref().unwrap_or("")
            .parse::<f64>()
            .map_err(|_| PackageError::Invalid(format!("video frame {index} has no timestamp")))?;
        let expected = index as f64 * frame_seconds;
        if !actual.is_finite() || (actual - expected).abs() > 0.0005 {
            return Err(PackageError::Invalid(format!(
                "video frame {index} has a gap or variable timestamp; only continuous constant FPS capture is supported"
            )));
        }
    }
    let frame_duration_ms = (frames.frames.len() as f64 * frame_seconds * 1000.0).floor() as u64;
    if frame_duration_ms.abs_diff(expected_duration_ms) > 1 {
        return Err(PackageError::Invalid(
            "video frame timestamps and stream duration disagree".into()
        ));
    }
    Ok(())
}

pub fn probe_muxed_editor_coverage(
    path: &Path,
    declared: &CaptureClip,
) -> Result<MuxedEditorCoverage, PackageError> {
    probe_muxed_editor_coverage_with_cancel(path, declared, || false)
}

pub fn probe_muxed_editor_coverage_with_cancel(
    path: &Path,
    declared: &CaptureClip,
    should_cancel: impl Fn() -> bool,
) -> Result<MuxedEditorCoverage, PackageError> {
    let result = probe_with_cancel(path, &should_cancel)?;
    let coverage = muxed_editor_coverage(&result, declared)?;
    let frames = probe_video_frames_with_cancel(path, &should_cancel)?;
    validate_constant_frame_coverage(&result, &frames, coverage.video_duration_ms)?;
    Ok(coverage)
}

#[cfg(test)]
mod tests {
    use super::{FrameProbeResult, ProbeResult, audio, fps, muxed_editor_coverage,
        run_probe_command, run_probe_command_bounded, validate_constant_frame_coverage, video};
    use clipstrike_core::capture_package::{CaptureClip, PackageError};
    use std::process::Command;
    use std::time::{Duration, Instant};

    #[cfg(windows)]
    #[test]
    fn selected_probe_executable_runs_with_no_path_lookup() {
        let program = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/whoami.exe");
        let stdout = crate::media_tools::with_selected_media_tools(&program, &program, || {
            let mut command = Command::new(crate::media_tools::ffprobe_program());
            command.env("PATH", "");
            run_probe_command(&mut command, || false)
        }).unwrap().unwrap();
        assert!(!String::from_utf8_lossy(&stdout).trim().is_empty());
    }

    #[test]
    fn probe_child_fixture() {
        let Some(ready) = std::env::var_os("DEMOSTRIKE_PROBE_TEST_READY") else { return };
        std::fs::write(ready, std::process::id().to_string()).unwrap();
        std::thread::sleep(Duration::from_secs(10));
    }

    #[test]
    fn cancelling_active_probe_stops_and_reaps_child_promptly() {
        let dir = tempfile::tempdir().unwrap();
        let ready = dir.path().join("probe-ready");
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "media_probe::tests::probe_child_fixture", "--nocapture"])
            .env("DEMOSTRIKE_PROBE_TEST_READY", &ready);
        let started = Instant::now();
        let result = run_probe_command(&mut command, || ready.is_file());
        assert!(matches!(result, Err(PackageError::Cancelled(_))));
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(ready.is_file());
    }

    #[test]
    fn timed_out_probe_stops_and_reaps_child_promptly() {
        let dir = tempfile::tempdir().unwrap();
        let ready = dir.path().join("probe-ready");
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "media_probe::tests::probe_child_fixture", "--nocapture"])
            .env("DEMOSTRIKE_PROBE_TEST_READY", &ready);
        let started = Instant::now();
        let result = run_probe_command_bounded(
            &mut command, || false, Duration::from_millis(150), 4096, 4096
        );
        assert!(matches!(result, Err(PackageError::Invalid(message)) if message.contains("deadline")));
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn probe_stdout_limit_rejects_excess_output() {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.arg("--help");
        let result = run_probe_command_bounded(
            &mut command, || false, Duration::from_secs(3), 16, 4096
        );
        assert!(matches!(result, Err(PackageError::Invalid(message)) if message.contains("byte limit")));
    }

    #[test]
    fn parses_fractional_frame_rate_without_rounding_to_integer() {
        let result: ProbeResult = serde_json::from_str(
            r#"{
            "streams": [{"codec_type":"video","width":1920,"height":1080,
                "avg_frame_rate":"30000/1001"}], "format":{"duration":"10.010"}
        }"#,
        )
        .unwrap();
        let inspected = video(&result).unwrap();
        assert_eq!((inspected.fps_num, inspected.fps_den), (30000, 1001));
        assert_eq!(fps("60000/2002").unwrap(), (60000, 2002));
    }

    #[test]
    fn rejects_video_only_container_as_audio_evidence() {
        let result: ProbeResult = serde_json::from_str(
            r#"{
            "streams": [{"codec_type":"video","width":1920,"height":1080,
                "avg_frame_rate":"60/1"}], "format":{"duration":"8.5"}
        }"#,
        )
        .unwrap();
        assert!(audio(&result).is_err());
    }

    #[test]
    fn editor_coverage_omits_audio_only_tail_without_rewriting_manifest_duration() {
        let declared: CaptureClip = serde_json::from_value(serde_json::json!({
            "clip_id": "e6-projectile", "relative_path": "clips/e6-projectile.mp4",
            "sha256": "a".repeat(64), "demo_sha256": "b".repeat(64),
            "event_id": "6", "player_steamid64": "76561198000000001",
            "map": "de_train", "round": 2, "utility_type": "he",
            "start_tick": 5484, "end_tick": 5660, "camera_role": "projectile",
            "duration_ms": 2740, "width": 1920, "height": 1080,
            "fps_num": 60, "fps_den": 1,
            "audio": {"source": "hlae_wav", "channels": 2,
                "sample_rate": 44100, "verified": true},
            "qc_status": "accepted", "group_order": 1
        })).unwrap();
        let probe: ProbeResult = serde_json::from_str(r#"{
            "streams": [
                {"codec_type":"video", "width":1920, "height":1080,
                 "avg_frame_rate":"60/1", "time_base":"1/15360",
                 "start_time":"0.000000", "duration_ts":41472,
                 "duration":"2.700000", "nb_frames":"162"},
                {"codec_type":"audio", "sample_rate":"44100", "channels":2,
                 "time_base":"1/44100", "start_time":"0.000000",
                 "duration_ts":120834, "duration":"2.740000"}
            ], "format":{"duration":"2.740000"}
        }"#).unwrap();
        let coverage = muxed_editor_coverage(&probe, &declared).unwrap();
        assert_eq!(coverage.video_duration_ms, 2700);
        assert_eq!(coverage.audio_duration_ms, 2740);
        assert_eq!(coverage.visual_duration_ms, 2700);
        assert_eq!(declared.duration_ms, 2740);
    }

    #[test]
    fn editor_coverage_rejects_streams_without_independent_duration() {
        let declared: CaptureClip = serde_json::from_value(serde_json::json!({
            "clip_id": "clip", "relative_path": "clips/clip.mp4",
            "sha256": "a".repeat(64), "demo_sha256": "b".repeat(64),
            "event_id": "6", "player_steamid64": "76561198000000001",
            "map": "de_train", "round": 2, "utility_type": "he",
            "start_tick": 5484, "end_tick": 5660, "camera_role": "projectile",
            "duration_ms": 2740, "width": 1920, "height": 1080,
            "fps_num": 60, "fps_den": 1,
            "audio": {"source": "hlae_wav", "channels": 2,
                "sample_rate": 44100, "verified": true},
            "qc_status": "accepted", "group_order": 1
        })).unwrap();
        let probe: ProbeResult = serde_json::from_str(r#"{
            "streams": [
                {"codec_type":"video", "width":1920, "height":1080,
                 "avg_frame_rate":"60/1", "start_time":"0.000000",
                 "nb_frames":"162"},
                {"codec_type":"audio", "sample_rate":"44100", "channels":2,
                 "start_time":"0.000000"}
            ], "format":{"duration":"2.740000"}
        }"#).unwrap();
        assert!(muxed_editor_coverage(&probe, &declared).is_err());
    }

    #[test]
    fn shorter_audio_is_unsupported_instead_of_shortening_picture() {
        let declared = serde_json::from_value::<CaptureClip>(serde_json::json!({
            "clip_id": "short-audio", "relative_path": "clips/short-audio.mp4",
            "sha256": "a".repeat(64), "demo_sha256": "b".repeat(64),
            "event_id": "6", "player_steamid64": "76561198000000001",
            "map": "de_train", "round": 2, "utility_type": "he",
            "start_tick": 5484, "end_tick": 5660, "camera_role": "projectile",
            "duration_ms": 2740, "width": 1920, "height": 1080,
            "fps_num": 60, "fps_den": 1,
            "audio": {"source": "hlae_wav", "channels": 2,
                "sample_rate": 44100, "verified": true},
            "qc_status": "accepted", "group_order": 1
        })).unwrap();
        let probe: ProbeResult = serde_json::from_str(r#"{
            "streams": [
                {"codec_type":"video", "width":1920, "height":1080,
                 "avg_frame_rate":"60/1", "time_base":"1/15360",
                 "start_time":"0.000000", "duration_ts":41472,
                 "duration":"2.700000", "nb_frames":"162"},
                {"codec_type":"audio", "sample_rate":"44100", "channels":2,
                 "time_base":"1/44100", "start_time":"0.000000",
                 "duration_ts":118188, "duration":"2.680000"}
            ], "format":{"duration":"2.740000"}
        }"#).unwrap();
        let result = muxed_editor_coverage(&probe, &declared);
        assert!(matches!(result, Err(PackageError::Invalid(message)) if message.contains("audio ends before video")));
        assert_eq!(declared.duration_ms, 2740);
    }

    #[test]
    fn rejects_a_gap_despite_matching_frame_count_and_nominal_duration() {
        let probe: ProbeResult = serde_json::from_str(r#"{
            "streams":[{"codec_type":"video", "avg_frame_rate":"60/1",
                "nb_frames":"4"}], "format":{"duration":"0.066667"}
        }"#).unwrap();
        let regular: FrameProbeResult = serde_json::from_str(r#"{
            "frames":[{"best_effort_timestamp_time":"0.000000"},
                {"best_effort_timestamp_time":"0.016667"},
                {"best_effort_timestamp_time":"0.033333"},
                {"best_effort_timestamp_time":"0.050000"}]
        }"#).unwrap();
        assert!(validate_constant_frame_coverage(&probe, &regular, 66).is_ok());
        let gapped: FrameProbeResult = serde_json::from_str(r#"{
            "frames":[{"best_effort_timestamp_time":"0.000000"},
                {"best_effort_timestamp_time":"0.016667"},
                {"best_effort_timestamp_time":"0.050000"},
                {"best_effort_timestamp_time":"0.066667"}]
        }"#).unwrap();
        assert!(validate_constant_frame_coverage(&probe, &gapped, 66).is_err());
    }
}




