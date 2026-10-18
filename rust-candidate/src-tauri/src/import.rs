//! Real native CS2 import into DemoStrike's managed SQLite library.
//! The only synthetic parser seam is compiled into hosted smoke builds.

use demostrike::demo_import::{DemoFactsParser, ImportControl};
use demostrike::native_grenade_parser::NativeGrenadeParser;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReceipt {
    pub path: String,
    pub match_id: i64,
    pub map_name: String,
    pub player_count: usize,
    pub round_count: usize,
}

fn import_with_parser(
    demo: &Path,
    library: &Path,
    parser: &dyn DemoFactsParser,
    control: &ImportControl,
) -> Result<ImportReceipt, String> {
    let imported = demostrike::demo_import::import_demo(demo, library, parser, control, |_| {})?;
    Ok(ImportReceipt {
        path: library.to_string_lossy().into_owned(),
        match_id: imported.match_id,
        map_name: imported.analyzed.map_name,
        player_count: imported.analyzed.players.len(),
        round_count: imported.analyzed.rounds.len(),
    })
}

#[cfg(any(test, feature = "synthetic-import-smoke"))]
struct SyntheticFactsParser {
    facts: PathBuf,
}

#[cfg(any(test, feature = "synthetic-import-smoke"))]
impl DemoFactsParser for SyntheticFactsParser {
    fn parse(
        &self,
        _path: &Path,
        control: &ImportControl,
    ) -> Result<demostrike::parser::raw::RawDemo, String> {
        // Hosted GUI smoke can hold a synthetic parser in its cancellable
        // phase. This seam is absent from feature-free native builds.
        #[cfg(feature = "synthetic-import-smoke")]
        if let Some(wait_ms) = std::env::var("DEMOSTRIKE_SYNTHETIC_PARSE_WAIT_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value <= 15_000)
        {
            let start = std::time::Instant::now();
            while start.elapsed() < std::time::Duration::from_millis(wait_ms) {
                if control.is_cancelled() {
                    return Err("demo import cancelled".into());
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        #[cfg(not(feature = "synthetic-import-smoke"))]
        let _ = control;
        let bytes = std::fs::read(&self.facts).map_err(|error| error.to_string())?;
        serde_json::from_slice(&bytes)
            .map_err(|error| format!("Invalid synthetic smoke facts: {error}"))
    }
    fn capabilities(&self) -> demostrike::domain::ParserCapabilities {
        demostrike::domain::ParserCapabilities::TestSyntheticCombatV1
    }
}

#[tauri::command]
pub async fn csdm_import_demo(
    app: tauri::AppHandle,
    demo_path: String,
) -> Result<ImportReceipt, String> {
    let library = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("demostrike-library.sqlite");
    tauri::async_runtime::spawn_blocking(move || {
        let demo = Path::new(&demo_path);
        import_native(demo, &library)
    })
    .await
    .map_err(|error| error.to_string())?
}

pub(crate) fn import_native(demo: &Path, library: &Path) -> Result<ImportReceipt, String> {
    import_native_with_control(demo, library, &ImportControl::new())
}

pub(crate) fn import_native_with_control(
    demo: &Path,
    library: &Path,
    control: &ImportControl,
) -> Result<ImportReceipt, String> {
    #[cfg(feature = "synthetic-import-smoke")]
    if let Some(facts) = std::env::var_os("DEMOSTRIKE_SYNTHETIC_RAW_FACTS") {
        return import_with_parser(
            demo,
            library,
            &SyntheticFactsParser {
                facts: facts.into(),
            },
            control,
        );
    }
    import_with_parser(demo, library, &NativeGrenadeParser, control)
}

#[tauri::command]
pub async fn csdm_choose_demo(app: tauri::AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("Counter-Strike demo", &["dem"])
            .blocking_pick_file()
            .map(|file| {
                file.into_path()
                    .map_err(|error| error.to_string())?
                    .into_os_string()
                    .into_string()
                    .map_err(|_| "The selected demo path is not valid UTF-8.".to_owned())
            })
            .transpose()
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::get_match_detail;

    #[test]
    fn synthetic_fact_parser_imports_through_real_transaction_and_reader() {
        let directory = tempfile::tempdir().unwrap();
        let demo = directory.path().join("synthetic-combat.dem");
        let database = directory.path().join("managed.sqlite");
        std::fs::write(&demo, b"PBDEMS2\0synthetic public test bytes").unwrap();
        let source_before = std::fs::read(&demo).unwrap();
        let parser = SyntheticFactsParser {
            facts: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../tests/fixtures/combat-raw.json"),
        };
        let imported =
            import_with_parser(&demo, &database, &parser, &ImportControl::new()).unwrap();
        assert_eq!(imported.player_count, 3);
        assert_eq!(imported.round_count, 2);
        let detail = get_match_detail(&imported.path, imported.match_id).unwrap();
        assert!(detail.full_combat_coverage);
        assert_eq!(detail.summary.parser_capability, "test-synthetic-combat-v1");
        let ada = detail
            .players
            .iter()
            .find(|player| player.name == "Synthetic Ada")
            .unwrap();
        assert_eq!(ada.combat.as_ref().unwrap().enemy_kills, Some(1));
        assert_eq!(ada.combat.as_ref().unwrap().deaths, 1);
        assert_eq!(detail.rounds[0].combat.as_ref().unwrap().ct_players, 2);
        assert_eq!(std::fs::read(&demo).unwrap(), source_before);
        let duplicate =
            import_with_parser(&demo, &database, &parser, &ImportControl::new()).unwrap_err();
        assert!(duplicate.contains("already imported"));
        assert_eq!(
            get_match_detail(&imported.path, imported.match_id)
                .unwrap()
                .players
                .len(),
            3
        );
    }
}
