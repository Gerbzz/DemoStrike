//! Bounded, task-owned HLAE pilot. This records only a POV diagnostic take;
//! it does not produce a reviewed capture receipt or claim camera identity.
use crate::capture_preflight::{CaptureTools, preflight_capture, require_task_owned_pilot_runtime};
use crate::capture_request::{CAPTURE_REQUEST_VERSION, CaptureRequest, hash_source};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PilotManifest {
    pub version: u32,
    pub request_path: PathBuf,
    pub request_sha256: String,
    pub output_root: PathBuf,
    pub cs2_exe: PathBuf,
    pub hlae_exe: PathBuf,
    pub ffmpeg_exe: PathBuf,
    pub ffprobe_exe: PathBuf,
    pub start_tick: i64,
    pub end_tick: i64,
    pub observer_candidate: i64,
    pub issued_spec_player_value: i64,
    pub source_demo_sha256: String,
    pub cfg_sha256: String,
    pub script_sha256: String,
}

fn sha(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }
fn game_path(path: &Path) -> String { path.to_string_lossy().replace('\\', "/") }

pub fn stage_pov_pilot(request_path: &Path, tools: &CaptureTools,
    destination: &Path) -> Result<PathBuf, String> {
    require_task_owned_pilot_runtime(&tools.cs2_exe, destination)?;
    let preflight = preflight_capture(request_path, tools)?;
    if !preflight.issues.is_empty() {
        return Err(format!("capture preflight has blocking issues: {}", preflight.issues.join("; ")));
    }
    if preflight.cs2_patch_version.as_deref() != Some("1.41.6.7")
        || preflight.hlae_version.as_deref() != Some("2.190.2")
        || preflight.documented_pair_reference.is_none() {
        return Err("pilot requires the locally verified CS2 1.41.6.7 / HLAE 2.190.2 pair".into());
    }
    let request_bytes = std::fs::read(request_path).map_err(|e| e.to_string())?;
    let request: CaptureRequest = serde_json::from_slice(&request_bytes).map_err(|e| e.to_string())?;
    if request.version != CAPTURE_REQUEST_VERSION || request.takes.len() != 3 {
        return Err("unsupported capture request shape".into());
    }
    let pov = request.takes.iter().find(|take| take.camera_role == "player_pov")
        .ok_or("request has no player POV take")?;
    if pov.start_tick < 96 || pov.start_tick >= request.throw_tick
        || pov.end_tick <= request.throw_tick || !(0..=128).contains(&request.observer_slot_candidate) {
        return Err("unsupported POV timing or observer candidate".into());
    }
    // The parser's controller_by_user resolves event userid to controller
    // entity index userid + 1. CS2's spec_player command targets an entity
    // index. This remains a candidate until pixels and live identity agree.
    let issued_spec_player_value = request.observer_slot_candidate + 1;
    if !destination.is_absolute() || destination.to_string_lossy().contains('"') {
        return Err("pilot destination must be an absolute path without quotes".into());
    }
    std::fs::create_dir(destination).map_err(|e| format!("pilot destination must be new: {e}"))?;
    let output_root = destination.to_path_buf();
    std::fs::create_dir(output_root.join("cfg")).map_err(|e| e.to_string())?;
    std::fs::create_dir(output_root.join("raw-pov")).map_err(|e| e.to_string())?;
    let jump = (pov.start_tick - (30.0 * request.tick_rate) as i64).max(97);
    let spectate = (jump + pov.start_tick) / 2;
    let record_path = game_path(&output_root.join("raw-pov"));
    let script = format!(r#"(() => {{
  const listener = "demostrike-pov-pilot-v1";
  const jump = {jump}, spectate = {spectate};
  const start = {start}, end = {end};
  let phase = "boot", lastTick, sawPreStart = false;
  mirv.events.recordEnd.on(listener, () => {{
    if (phase === "await_record_end") {{ phase = "done"; mirv.exec("quit"); }}
  }});
  mirv.events.clientFrameStageNotify.on(listener, event => {{
    if (event.isBefore || phase === "done" || !mirv.isPlayingDemo()) return;
    const tick = mirv.getDemoTick();
    if (tick === undefined || tick < 0 || tick === lastTick) return;
    lastTick = tick;
    if (phase === "boot" && tick >= 96) {{
      phase = "seeking"; mirv.exec("demo_gototick " + jump); return;
    }}
    if (phase === "seeking") {{
      if (tick < start) sawPreStart = true;
      if (!sawPreStart) return;
      phase = "spectating";
    }}
    if (phase === "spectating" && tick >= spectate) {{
      mirv.exec("mirv_input end; spec_mode 1; spec_player {observer}");
      phase = "waiting";
    }}
    if (phase === "waiting" && tick >= start) {{
      mirv.exec("mirv_streams record name \"{record_path}\"");
      mirv.exec("mirv_streams record start"); phase = "recording";
    }}
    if (phase === "recording" && tick >= end) {{
      phase = "await_record_end"; mirv.exec("mirv_streams record end");
    }}
  }});
}})();
"#, start=pov.start_tick, end=pov.end_tick, observer=issued_spec_player_value);
    let script_path = output_root.join("cfg/demostrike_pov_pilot.js");
    std::fs::write(&script_path, &script).map_err(|e| e.to_string())?;
    let cfg = format!("echo [DemoStrike] offline POV pilot loaded\nsv_cheats 1\ncl_drawhud 0\nhidehud 260\ncl_showdemooverlay 0\nspec_show_xray 0\nmirv_snd_timescale 1\nmirv_streams record fps 60\nmirv_streams record startMovieWav 1\nmirv_streams record screen enabled 0\nmirv_streams add normal dsPilotClean\nmirv_streams edit dsPilotClean capture beforeUi\nmirv_streams edit dsPilotClean settings afxFfmpegYuv420p\nmirv_script_load \"{}\"\n", game_path(&script_path));
    let cfg_path = output_root.join("cfg/demostrike_pov_pilot.cfg");
    std::fs::write(&cfg_path, &cfg).map_err(|e| e.to_string())?;
    let manifest = PilotManifest {
        version: 2, request_path: request_path.to_path_buf(),
        request_sha256: sha(&request_bytes), output_root: output_root.clone(),
        cs2_exe: tools.cs2_exe.clone(), hlae_exe: tools.hlae_exe.clone(),
        ffmpeg_exe: tools.ffmpeg_exe.clone(), ffprobe_exe: tools.ffprobe_exe.clone(),
        start_tick: pov.start_tick, end_tick: pov.end_tick,
        observer_candidate: request.observer_slot_candidate,
        issued_spec_player_value,
        source_demo_sha256: request.demo_sha256,
        cfg_sha256: sha(cfg.as_bytes()), script_sha256: sha(script.as_bytes()),
    };
    let manifest_path = output_root.join("pilot-manifest.json");
    std::fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    Ok(manifest_path)
}

/// Launches the preflighted local demo in a session-only offline mode. HLAE's
/// launcher usually exits before CS2; the output is diagnostic, not QC.
pub fn launch_staged_pilot(manifest_path: &Path) -> Result<String, String> {
    let manifest: PilotManifest = serde_json::from_slice(&std::fs::read(manifest_path)
        .map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    if manifest.version != 2 || manifest_path != manifest.output_root.join("pilot-manifest.json") {
        return Err("pilot manifest path or version is unsupported".into());
    }
    require_task_owned_pilot_runtime(&manifest.cs2_exe, &manifest.output_root)?;
    let tools = CaptureTools { cs2_exe: manifest.cs2_exe.clone(), hlae_exe: manifest.hlae_exe.clone(),
        ffmpeg_exe: manifest.ffmpeg_exe.clone(), ffprobe_exe: manifest.ffprobe_exe.clone() };
    let preflight = preflight_capture(&manifest.request_path, &tools)?;
    if !preflight.issues.is_empty() { return Err(format!("preflight blocked launch: {}", preflight.issues.join("; "))); }
    if hash_source(&manifest.request_path)?.0 != manifest.request_sha256
        || hash_source(&manifest.output_root.join("cfg/demostrike_pov_pilot.cfg"))?.0 != manifest.cfg_sha256
        || hash_source(&manifest.output_root.join("cfg/demostrike_pov_pilot.js"))?.0 != manifest.script_sha256 {
        return Err("staged pilot source files changed".into());
    }
    let request: CaptureRequest = serde_json::from_slice(&std::fs::read(&manifest.request_path)
        .map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    if request.demo_sha256 != manifest.source_demo_sha256 { return Err("pilot demo identity changed".into()); }
    let hook = tools.hlae_exe.parent().ok_or("HLAE path has no parent")?.join("x64/AfxHookSource2.dll");
    let game_args = format!("-steam -insecure +sv_lan 1 -novid -console -sw -w 1920 -h 1080 -afxDisableSteamStorage +exec demostrike_pov_pilot.cfg +playdemo \"{}\"", request.demo_path.display());
    let child = Command::new(&tools.hlae_exe)
        .current_dir(tools.hlae_exe.parent().unwrap())
        .env("USRLOCALCSGO", &manifest.output_root)
        .args(["-customLoader", "-noGui", "-autoStart", "-noConfig", "-afxDisableSteamStorage"])
        .arg("-hookDllPath").arg(&hook)
        .arg("-programPath").arg(&tools.cs2_exe)
        .arg("-cmdLine").arg(&game_args)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .spawn().map_err(|e| format!("could not launch offline HLAE: {e}"))?;
    Ok(format!("launched HLAE PID {}; output remains unreviewed in {}", child.id(), manifest.output_root.display()))
}
