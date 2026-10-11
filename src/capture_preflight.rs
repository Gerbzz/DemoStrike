//! Read-only offline-capture preflight. This intentionally does not launch
//! CS2/HLAE, alter a game configuration, or assert version compatibility.
use crate::capture_request::{CAPTURE_REQUEST_VERSION, CaptureRequest, hash_source};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct CaptureTools {
    pub cs2_exe: PathBuf,
    pub hlae_exe: PathBuf,
    pub ffmpeg_exe: PathBuf,
    pub ffprobe_exe: PathBuf,
}

/// The offline pilot must be able to absorb CS2's own config writes without
/// modifying the installed historical runtime. The first real pilot showed
/// that USRLOCALCSGO alone does not redirect ServerConfig.vdf/boot.vcfg.
pub fn require_task_owned_pilot_runtime(cs2_exe: &Path, output_root: &Path) -> Result<(), String> {
    let owned = Path::new(env!("CARGO_MANIFEST_DIR")).canonicalize()
        .map_err(|e| format!("cannot resolve DemoStrike ownership root: {e}"))?;
    let game = cs2_exe.canonicalize()
        .map_err(|e| format!("cannot resolve CS2 executable: {e}"))?;
    let destination_parent = output_root.parent().ok_or("pilot output has no parent")?
        .canonicalize().map_err(|e| format!("cannot resolve pilot output parent: {e}"))?;
    if !game.starts_with(&owned) || !destination_parent.starts_with(&owned) {
        return Err("offline pilot is paused: CS2 rewrites pre-existing game-runtime configuration despite USRLOCALCSGO; use a verified task-owned game runtime before another launch".into());
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturePreflight {
    pub event_id: i64,
    pub demo_sha256: String,
    pub cs2_patch_version: Option<String>,
    pub hlae_version: Option<String>,
    pub local_runtime_provenance: Option<String>,
    pub documented_pair_reference: Option<String>,
    pub offline_hlae_requires_insecure_launch_flag: bool,
    pub tool_paths_present: bool,
    pub no_conflicting_game_or_hlae_process: bool,
    pub issues: Vec<String>,
}

fn version_value(path: &Path, prefix: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()?.lines()
        .find_map(|line| line.strip_prefix(prefix).map(|value| value.trim().to_string()))
        .filter(|value| !value.is_empty())
}

fn matching_historical_hlae_manifest(hlae_exe: &Path, hook: &Path)
    -> Result<Option<String>, String> {
    let Some(portable_root) = hlae_exe.parent() else { return Ok(None) };
    let manifest_path = portable_root.parent().unwrap_or(portable_root)
        .join("daily-cs-nades-runtime-manifest.json");
    if !manifest_path.is_file() { return Ok(None); }
    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(&manifest_path)
        .map_err(|e| format!("cannot read local HLAE provenance: {e}"))?)
        .map_err(|e| format!("invalid local HLAE provenance: {e}"))?;
    let recorded_exe = manifest.pointer("/runtime/hlae/sha256").and_then(|v| v.as_str())
        .ok_or("local HLAE provenance has no executable hash")?;
    let recorded_hook = manifest.pointer("/runtime/afxHookSource2/sha256").and_then(|v| v.as_str())
        .ok_or("local HLAE provenance has no Source 2 hook hash")?;
    let version = manifest.get("version").and_then(|v| v.as_str())
        .ok_or("local HLAE provenance has no version")?;
    if hash_source(hlae_exe)?.0 != recorded_exe.to_ascii_lowercase()
        || hash_source(hook)?.0 != recorded_hook.to_ascii_lowercase() {
        return Err("installed historical HLAE binaries differ from their local provenance manifest".into());
    }
    Ok(Some(version.into()))
}

fn verify_historical_cs2_manifest(cs2_exe: &Path) -> Result<bool, String> {
    let Some(runtime_root) = cs2_exe.parent().and_then(Path::parent)
        .and_then(Path::parent).and_then(Path::parent) else { return Ok(false) };
    let manifest_path = runtime_root.join("daily-cs-nades-runtime-manifest.json");
    if !manifest_path.is_file() { return Ok(false); }
    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(&manifest_path)
        .map_err(|e| format!("cannot read local CS2 provenance: {e}"))?)
        .map_err(|e| format!("invalid local CS2 provenance: {e}"))?;
    let recorded = manifest.pointer("/runtime/cs2Sha256").and_then(|v| v.as_str())
        .ok_or("local CS2 provenance has no executable hash")?;
    if hash_source(cs2_exe)?.0 != recorded.to_ascii_lowercase() {
        return Err("historical CS2 executable differs from its local provenance manifest".into());
    }
    Ok(true)
}

fn conflicts() -> Result<bool, String> {
    #[cfg(windows)] {
        let output = Command::new("tasklist").args(["/FO", "CSV", "/NH"])
            .output().map_err(|e| format!("cannot inspect running game processes: {e}"))?;
        if !output.status.success() { return Err("Windows process inventory failed".into()); }
        let table = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
        Ok(!table.lines().any(|line| line.starts_with("\"cs2.exe\",")
            || line.starts_with("\"hlae.exe\",")))
    }
    #[cfg(not(windows))] {
        Err("CS2/HLAE process preflight requires Windows".into())
    }
}

pub fn preflight_capture(request_path: &Path, tools: &CaptureTools) -> Result<CapturePreflight, String> {
    let request: CaptureRequest = serde_json::from_slice(&std::fs::read(request_path)
        .map_err(|e| format!("cannot read capture request: {e}"))?)
        .map_err(|e| format!("invalid capture request: {e}"))?;
    if request.version != CAPTURE_REQUEST_VERSION {
        return Err(format!("unsupported capture request version {}", request.version));
    }
    let mut issues = Vec::new();
    match hash_source(&request.demo_path) {
        Ok((hash, bytes)) if hash == request.demo_sha256 && bytes == request.demo_bytes => {}
        Ok(_) => issues.push("source demo differs from the saved request".into()),
        Err(error) => issues.push(error),
    }
    let hook = tools.hlae_exe.parent().unwrap_or_else(|| Path::new("."))
        .join("x64/AfxHookSource2.dll");
    for (name, path) in [("CS2", &tools.cs2_exe), ("HLAE", &tools.hlae_exe),
        ("AfxHookSource2", &hook), ("FFmpeg", &tools.ffmpeg_exe), ("FFprobe", &tools.ffprobe_exe)] {
        if !path.is_file() { issues.push(format!("{name} executable is missing: {}", path.display())); }
    }
    let tool_paths_present = !issues.iter().any(|issue| issue.contains("executable is missing"));
    let game_root = tools.cs2_exe.parent().and_then(Path::parent).and_then(Path::parent);
    let cs2_patch_version = game_root.and_then(|root|
        version_value(&root.join("csgo/steam.inf"), "PatchVersion="));
    if cs2_patch_version.is_none() { issues.push("CS2 PatchVersion is unavailable from steam.inf".into()); }
    if tools.cs2_exe.is_file() {
        match verify_historical_cs2_manifest(&tools.cs2_exe) {
            Ok(true) => {
                // The HLAE manifest check below can append the complementary
                // provenance note after both runtimes are known.
            }
            Ok(false) => {}
            Err(error) => issues.push(error),
        }
    }
    let mut hlae_version = tools.hlae_exe.parent().and_then(|root|
        std::fs::read_to_string(root.join("hlae-version.txt")).ok())
        .map(|text| text.trim().to_string()).filter(|text| !text.is_empty());
    let mut local_runtime_provenance = None;
    if hlae_version.is_none() && tools.hlae_exe.is_file() && hook.is_file() {
        match matching_historical_hlae_manifest(&tools.hlae_exe, &hook) {
            Ok(Some(version)) => {
                hlae_version = Some(version);
                local_runtime_provenance = Some(
                    "HLAE executable and Source 2 hook SHA-256 match the existing local runtime manifest; its detached archive signature has not been independently verified".into());
            }
            Ok(None) => {}
            Err(error) => issues.push(error),
        }
    }
    if hlae_version.is_none() { issues.push("HLAE version marker is unavailable".into()); }
    let ffmpeg_registration = tools.hlae_exe.parent().unwrap_or_else(|| Path::new("."))
        .join("ffmpeg/ffmpeg.ini");
    if !ffmpeg_registration.is_file() {
        issues.push(format!("HLAE FFmpeg registration is missing: {}; stage a task-owned runtime rather than editing the installed bundle",
            ffmpeg_registration.display()));
    }
    let no_conflicting_game_or_hlae_process = match conflicts() {
        Ok(clear) => clear,
        Err(error) => { issues.push(error); false }
    };
    if !no_conflicting_game_or_hlae_process {
        issues.push("CS2 or HLAE is already running; reconcile its owner before another capture".into());
    }
    // The tool pair still needs a bounded offline replay pilot. Version
    // markers and file presence are not compatibility or POV evidence.
    if let (Some(cs2), Some(hlae)) = (&cs2_patch_version, &hlae_version) {
        if cs2 == "1.41.9.0" && hlae == "2.191.1" {
            issues.push("installed HLAE 2.191.1 predates CS2 1.41.9.0; no compatible replay proof exists for this pair".into());
        }
    }
    let documented_pair_reference = (cs2_patch_version.as_deref() == Some("1.41.6.7")
        && hlae_version.as_deref() == Some("2.190.2"))
        .then(|| "https://github.com/advancedfx/advancedfx/wiki/Old-HLAE-versions-for-specific-CS2-versions".into());
    Ok(CapturePreflight {
        event_id: request.event_id,
        demo_sha256: request.demo_sha256,
        cs2_patch_version,
        hlae_version,
        local_runtime_provenance,
        documented_pair_reference,
        offline_hlae_requires_insecure_launch_flag: true,
        tool_paths_present,
        no_conflicting_game_or_hlae_process,
        issues,
    })
}
