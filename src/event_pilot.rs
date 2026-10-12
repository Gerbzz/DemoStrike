//! Three-role offline capture pilot. The output is diagnostic until live
//! account identity, frame content, and muxed audio are independently sealed.
use crate::capture_preflight::{CaptureTools, preflight_capture, require_task_owned_pilot_runtime};
use crate::capture_request::{CaptureRequest, CAPTURE_REQUEST_VERSION, hash_source};
use crate::owned_capture::{OwnedLaunch, run_owned};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::ffi::OsString;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventPilotManifest {
    pub version: u32,
    pub request_path: PathBuf,
    pub request_sha256: String,
    pub output_root: PathBuf,
    pub cs2_exe: PathBuf,
    pub hlae_exe: PathBuf,
    pub ffmpeg_exe: PathBuf,
    pub ffprobe_exe: PathBuf,
    pub issued_spec_player_value: i64,
    pub cfg_sha256: String,
    pub script_sha256: String,
    pub runtime_console_path: PathBuf,
}

fn sha(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }
fn slash(path: &Path) -> String { path.to_string_lossy().replace('\\', "/") }
fn json(value: &str) -> String { serde_json::to_string(value).unwrap() }

fn angles(camera: [f64; 3], target: [f64; 3]) -> (f64, f64) {
    let dx = target[0]-camera[0]; let dy = target[1]-camera[1]; let dz = target[2]-camera[2];
    (-dz.atan2(dx.hypot(dy)).to_degrees(), dy.atan2(dx).to_degrees())
}
fn camera(tick: i64, position: [f64; 3], target: [f64; 3], fov: f64) -> String {
    let (pitch, yaw) = angles(position, target);
    format!("[{tick},{}]", json(&format!("mirv_input camera; mirv_input position {:.3} {:.3} {:.3}; mirv_input angles {:.3} {:.3} 0; mirv_input fov {:.1}",
        position[0], position[1], position[2], pitch, yaw, fov)))
}
fn interpolate(samples: &[[f64; 4]], tick: f64) -> [f64; 3] {
    let first = samples.first().unwrap();
    if tick <= first[0] { return [first[1], first[2], first[3]]; }
    let last = samples.last().unwrap();
    if tick >= last[0] { return [last[1], last[2], last[3]]; }
    let pair = samples.windows(2).find(|pair| pair[0][0] <= tick && tick <= pair[1][0]).unwrap();
    let a = (tick-pair[0][0])/(pair[1][0]-pair[0][0]);
    [1,2,3].map(|i| pair[0][i]+a*(pair[1][i]-pair[0][i]))
}
fn projectile_actions(request: &CaptureRequest) -> Vec<String> {
    let first = request.flight_samples.first().unwrap()[0] as i64;
    let last = request.flight_samples.last().unwrap()[0] as i64;
    let mut actions = Vec::new();
    for tick in (first..=last).step_by(4) {
        let here = interpolate(&request.flight_samples, tick as f64);
        let ahead = interpolate(&request.flight_samples, (tick+4).min(last) as f64);
        let behind = interpolate(&request.flight_samples, (tick-4).max(first) as f64);
        let dx = ahead[0]-behind[0]; let dy = ahead[1]-behind[1];
        let len = dx.hypot(dy).max(0.001);
        let dir = [dx/len, dy/len];
        let position = [here[0]-dir[0]*105.0-dir[1]*32.0,
            here[1]-dir[1]*105.0+dir[0]*32.0, here[2]+35.0];
        let target = [here[0]+dir[0]*16.0, here[1]+dir[1]*16.0, here[2]];
        actions.push(camera(tick, position, target, 80.0));
    }
    let end = request.detonate_position;
    let landing = [end.x, end.y, end.z];
    actions.push(camera(request.detonate_tick, [landing[0]+80.0, landing[1]+100.0,
        landing[2]+125.0], [landing[0], landing[1], landing[2]+20.0], 78.0));
    actions
}

pub fn stage_event_pilot(request_path: &Path, tools: &CaptureTools,
    destination: &Path) -> Result<PathBuf, String> {
    require_task_owned_pilot_runtime(&tools.cs2_exe, destination)?;
    let preflight = preflight_capture(request_path, tools)?;
    if !preflight.issues.is_empty() { return Err(preflight.issues.join("; ")); }
    if preflight.cs2_patch_version.as_deref() != Some("1.41.6.7")
        || preflight.hlae_version.as_deref() != Some("2.190.2")
        || preflight.documented_pair_reference.is_none() {
        return Err("event pilot requires the verified historical CS2/HLAE pair".into());
    }
    let bytes = std::fs::read(request_path).map_err(|e| e.to_string())?;
    let request: CaptureRequest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if request.version != CAPTURE_REQUEST_VERSION || request.takes.len() != 3
        || !(0..=128).contains(&request.observer_slot_candidate)
        || request.flight_samples.len() < 3 || !destination.is_absolute()
        || destination.to_string_lossy().contains('"') {
        return Err("unsupported request, flight path, or destination".into());
    }
    let issued = request.observer_slot_candidate + 1;
    let mut clips = Vec::new();
    for role in ["player_pov", "projectile", "third_person_reference"] {
        let take = request.takes.iter().find(|take| take.camera_role == role)
            .ok_or(format!("missing planned {role} take"))?;
        if take.start_tick < 96 || take.start_tick >= take.end_tick { return Err("invalid take ticks".into()); }
        let jump = (take.start_tick-(request.tick_rate*30.0) as i64).max(97);
        let spectate = (jump+take.start_tick)/2;
        let name = match role { "player_pov" => "pov", "projectile" => "projectile", _ => "third" };
        let record_dir = destination.join(format!("raw-{name}"));
        let record_command = format!("mirv_streams record name \"{}\"", slash(&record_dir));
        let actions = match role {
            "projectile" => projectile_actions(&request),
            "third_person_reference" => {
                let p = request.throw_position;
                let pos = [p.x-28.0, p.y+170.0, p.z+88.0];
                vec![camera(take.start_tick, pos, [p.x,p.y,p.z+30.0], 77.0)]
            }
            _ => Vec::new(),
        };
        clips.push(format!("{{role:{},recordName:{},jump:{jump},spectate:{spectate},start:{},end:{},pov:{},actions:[{}]}}",
            json(role), json(&record_command), take.start_tick-3, take.end_tick,
            json(&format!("mirv_input end; spec_mode 1; spec_player {issued}")), actions.join(",")));
    }
    let script = format!(r#"(() => {{
  const listener = "demostrike-event-pilot-v4";
  const clips = [{}];
  let index = 0, phase = "boot", lastTick, sawPreStart = false, actionIndex = 0;
  const current = () => clips[index];
  const observe = (kind, tick) => {{
    const controller = mirv.getEntityFromIndex({issued});
    const name = controller && controller.isPlayerController()
      ? controller.getSanitizedPlayerName() : "MISSING_CONTROLLER";
    const safeName = name.replace(/[^a-zA-Z0-9_]/g, "_");
    const steamId = controller && controller.isPlayerController() &&
      typeof controller.getSteamId === "function"
      ? String(controller.getSteamId()) : "MISSING_STEAM_ID";
    const localController = mirv.getEntityFromSplitScreenPlayer(0);
    const localPawn = localController &&
      mirv.getEntityFromIndex(mirv.getHandleEntryIndex(localController.getPlayerPawnHandle()));
    const targetPawn = localPawn &&
      mirv.getEntityFromIndex(mirv.getHandleEntryIndex(localPawn.getObserverTargetHandle()));
    const targetController = targetPawn &&
      mirv.getEntityFromIndex(mirv.getHandleEntryIndex(targetPawn.getPlayerControllerHandle()));
    const targetName = targetController && targetController.isPlayerController()
      ? targetController.getSanitizedPlayerName() : "NO_OBSERVER_TARGET";
    const targetSteamId = targetController && targetController.isPlayerController() &&
      typeof targetController.getSteamId === "function"
      ? String(targetController.getSteamId()) : "MISSING_TARGET_STEAM_ID";
    const marker = "DEMOSTRIKE_OBSERVE role=" + current().role +
      " kind=" + kind + " tick=" + tick + " entity={issued} name=" + safeName +
      " steamid=" + steamId.replace(/[^0-9]/g, "") +
      " observer=" + targetName.replace(/[^a-zA-Z0-9_]/g, "_") +
      " observerSteamid=" + targetSteamId.replace(/[^0-9]/g, "");
    mirv.message(marker + "\n"); mirv.exec("echo " + marker);
    if (kind === "setup" || kind === "release") {{
      mirv.exec("status_json"); mirv.exec("status");
    }}
  }};
  const seek = () => {{
    phase = "seeking"; lastTick = undefined; sawPreStart = false; actionIndex = 0;
    mirv.exec("demo_gototick " + current().jump);
  }};
  mirv.events.recordEnd.on(listener, () => {{
    if (phase !== "await_record_end") return;
    observe("record_end_callback", mirv.getDemoTick());
    index++;
    if (index === clips.length) {{ phase = "done"; mirv.exec("quit"); }}
    else seek();
  }});
  mirv.events.clientFrameStageNotify.on(listener, event => {{
    if (event.isBefore || phase === "done" || !mirv.isPlayingDemo()) return;
    const tick = mirv.getDemoTick();
    if (tick === undefined || tick < 0 || tick === lastTick) return;
    lastTick = tick; const clip = current();
    if (phase === "boot" && tick >= 96) {{ seek(); return; }}
    if (phase === "seeking") {{
      if (tick < clip.start) sawPreStart = true;
      if (!sawPreStart) return;
      phase = "spectating";
    }}
    if (phase === "spectating" && tick >= clip.spectate) {{
      mirv.exec(clip.pov); phase = "waiting";
    }}
    if (phase === "waiting" && tick >= clip.start) {{
      mirv.exec(clip.recordName); mirv.exec("mirv_streams record start"); phase = "recording";
      observe("record_start_command", tick); observe("setup", tick);
    }}
    if (phase === "recording") {{
      if (tick >= {throw_tick} && !clip.releaseObserved) {{
        clip.releaseObserved = true; observe("release", tick);
      }}
      if (tick >= {detonate_tick} && !clip.detonationObserved) {{
        clip.detonationObserved = true; observe("detonation", tick);
      }}
      while (actionIndex < clip.actions.length && clip.actions[actionIndex][0] <= tick) {{
        mirv.exec(clip.actions[actionIndex][1]); actionIndex++;
      }}
      if (tick >= clip.end) {{
        observe("record_end_command", tick);
        phase = "await_record_end"; mirv.exec("mirv_streams record end");
      }}
    }}
  }});
}})();
"#, clips.join(",\n"), issued=issued, throw_tick=request.throw_tick, detonate_tick=request.detonate_tick);
    std::fs::create_dir(destination).map_err(|e| format!("destination must be new: {e}"))?;
    std::fs::create_dir(destination.join("cfg")).map_err(|e| e.to_string())?;
    for name in ["pov","projectile","third"] { std::fs::create_dir(destination.join(format!("raw-{name}"))).map_err(|e| e.to_string())?; }
    let script_path = destination.join("cfg/demostrike_event_pilot.js");
    std::fs::write(&script_path, &script).map_err(|e| e.to_string())?;
    let runtime_console_path = destination.join("runtime-console.log");
    let cfg = format!("con_logfile \"{}\"\necho [DemoStrike] offline event pilot loaded\nsv_cheats 1\ncl_drawhud 0\nhidehud 260\ncl_showdemooverlay 0\nspec_show_xray 0\nmirv_snd_timescale 1\nmirv_streams record fps 60\nmirv_streams record startMovieWav 1\nmirv_streams record screen enabled 0\nmirv_streams add normal dsEventClean\nmirv_streams edit dsEventClean capture beforeUi\nmirv_streams edit dsEventClean settings afxFfmpegYuv420p\nmirv_script_load \"{}\"\n", slash(&runtime_console_path), slash(&script_path));
    std::fs::write(destination.join("cfg/demostrike_event_pilot.cfg"), &cfg).map_err(|e| e.to_string())?;
    let manifest = EventPilotManifest { version: 4, request_path: request_path.to_path_buf(),
        request_sha256: sha(&bytes), output_root: destination.to_path_buf(),
        cs2_exe: tools.cs2_exe.clone(), hlae_exe: tools.hlae_exe.clone(),
        ffmpeg_exe: tools.ffmpeg_exe.clone(), ffprobe_exe: tools.ffprobe_exe.clone(),
        issued_spec_player_value: issued, cfg_sha256: sha(cfg.as_bytes()), script_sha256: sha(script.as_bytes()), runtime_console_path };
    let path = destination.join("event-pilot-manifest.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    Ok(path)
}

pub fn event_launch_spec(manifest_path: &Path) -> Result<OwnedLaunch, String> {
    let manifest: EventPilotManifest = serde_json::from_slice(&std::fs::read(manifest_path)
        .map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    require_task_owned_pilot_runtime(&manifest.cs2_exe, &manifest.output_root)?;
    if manifest.version != 4 || manifest_path != manifest.output_root.join("event-pilot-manifest.json")
        || hash_source(&manifest.request_path)?.0 != manifest.request_sha256
        || hash_source(&manifest.output_root.join("cfg/demostrike_event_pilot.cfg"))?.0 != manifest.cfg_sha256
        || hash_source(&manifest.output_root.join("cfg/demostrike_event_pilot.js"))?.0 != manifest.script_sha256 {
        return Err("event pilot manifest or staged files changed".into());
    }
    let tools = CaptureTools { cs2_exe: manifest.cs2_exe.clone(), hlae_exe: manifest.hlae_exe.clone(),
        ffmpeg_exe: manifest.ffmpeg_exe.clone(), ffprobe_exe: manifest.ffprobe_exe.clone() };
    let preflight = preflight_capture(&manifest.request_path, &tools)?;
    if !preflight.issues.is_empty() { return Err(preflight.issues.join("; ")); }
    let request: CaptureRequest = serde_json::from_slice(&std::fs::read(&manifest.request_path)
        .map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    if manifest.issued_spec_player_value != request.observer_slot_candidate+1 {
        return Err("spectator command no longer matches the source userinfo candidate".into());
    }
    let hook = tools.hlae_exe.parent().ok_or("HLAE parent missing")?.join("x64/AfxHookSource2.dll");
    let game_args = format!("-steam -insecure +sv_lan 1 -novid -console -condebug -conclearlog -sw -w 1920 -h 1080 -afxDisableSteamStorage +exec demostrike_event_pilot.cfg +playdemo \"{}\"", request.demo_path.display());
    Ok(OwnedLaunch {
        program: tools.hlae_exe.clone(),
        current_dir: tools.hlae_exe.parent().unwrap().to_path_buf(),
        environment: vec![(OsString::from("USRLOCALCSGO"), manifest.output_root.into_os_string())],
        args: [
            OsString::from("-customLoader"), OsString::from("-noGui"),
            OsString::from("-autoStart"), OsString::from("-noConfig"),
            OsString::from("-afxDisableSteamStorage"), OsString::from("-hookDllPath"),
            hook.into_os_string(), OsString::from("-programPath"),
            tools.cs2_exe.into_os_string(), OsString::from("-cmdLine"),
            OsString::from(game_args),
        ].into(),
    })
}

/// CLI entry point: block until the owned process group exits or times out.
pub fn launch_event_pilot(manifest_path: &Path) -> Result<String, String> {
    let launch = event_launch_spec(manifest_path)?;
    run_owned(launch, &AtomicBool::new(false), Duration::from_secs(25 * 60), |_| {})?;
    Ok("event pilot processes exited; all three takes remain unreviewed".into())
}
