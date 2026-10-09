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
}

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

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
    if should_cancel() { return Err(PackageError::Cancelled("media probe cancelled".into())); }
    let stdout = tempfile::NamedTempFile::new()
        .map_err(|error| PackageError::Io(error.to_string()))?;
    let stderr = tempfile::NamedTempFile::new()
        .map_err(|error| PackageError::Io(error.to_string()))?;
    let mut child = command
        .stdout(Stdio::from(stdout.reopen().map_err(|error| PackageError::Io(error.to_string()))?))
        .stderr(Stdio::from(stderr.reopen().map_err(|error| PackageError::Io(error.to_string()))?))
        .spawn().map_err(|error| PackageError::Io(format!("media probe: {error}")))?;
    let status = loop {
        if should_cancel() {
            kill_and_reap(&mut child)?;
            return Err(PackageError::Cancelled("media probe cancelled; child process stopped".into()));
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
    let mut command = Command::new("ffprobe");
    command.args(["-v", "error", "-show_entries", "stream=codec_type,width,height,avg_frame_rate,r_frame_rate,sample_rate,channels:format=duration", "-of", "json"])
        .arg(path);
    let bytes = run_probe_command(&mut command, should_cancel)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| PackageError::Json(format!("ffprobe output: {error}")))
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

pub fn probe_muxed_clip_with_cancel(
    path: &Path,
    declared: &CaptureClip,
    should_cancel: impl Fn() -> bool,
) -> Result<(), PackageError> {
    let result = probe_with_cancel(path, should_cancel)?;
    let video = video(&result)?;
    let audio = audio(&result)?;
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

#[cfg(test)]
mod tests {
    use super::{ProbeResult, audio, fps, run_probe_command, video};
    use clipstrike_core::capture_package::PackageError;
    use std::process::Command;
    use std::time::{Duration, Instant};

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
}
