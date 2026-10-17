use demostrike::edit_handoff::{SavedEditHandoff, build_from_library, save_handoff_noclobber};
use std::path::Path;
use tauri::Manager;

/// Hashes the original demo and checks the selected imported event before
/// writing a local-only plan and a native, media-empty ClipStrike project.
#[tauri::command]
pub async fn csdm_export_edit_handoff(
    app: tauri::AppHandle,
    path: String,
    match_id: i64,
    event_id: i64,
) -> Result<SavedEditHandoff, String> {
    let parent = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("offline-edit-handoffs");
    tauri::async_runtime::spawn_blocking(move || {
        let plan = build_from_library(Path::new(&path), event_id)?;
        if plan.capture_request.match_id != match_id {
            return Err("selected throw does not belong to this recorded match".into());
        }
        std::fs::create_dir_all(&parent)
            .map_err(|error| format!("cannot create local handoff directory: {error}"))?;
        let source = &plan.capture_request.demo_sha256[..12];
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_millis();
        let destination = parent.join(format!(
            "{}-event-{}-{source}-{now}",
            plan.capture_request.map, event_id
        ));
        save_handoff_noclobber(&plan, &destination)
    })
    .await
    .map_err(|error| error.to_string())?
}
