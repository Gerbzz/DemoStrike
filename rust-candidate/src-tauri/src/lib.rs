pub mod capture_provider;
pub mod csdm;
pub mod library;

use library::{LibrarySnapshot, MatchDetail};

#[tauri::command]
fn list_matches(path: String) -> Result<LibrarySnapshot, String> {
    library::list_matches(&path).map_err(|error| error.to_string())
}

#[tauri::command]
fn get_match_detail(path: String, match_id: i64) -> Result<MatchDetail, String> {
    library::get_match_detail(&path, match_id).map_err(|error| error.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            list_matches,
            get_match_detail,
            csdm::csdm_load_settings,
            csdm::csdm_save_settings,
            csdm::csdm_request,
            csdm::csdm_window_action
        ])
        .run(tauri::generate_context!())
        .expect("DemoStrike candidate failed to start");
}
