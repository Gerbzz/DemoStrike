//! Real `.dem` import seam for the native UI. CsThrowBack's raw-facts schema,
//! analysis, and SQLite transactions stay in Rust. Its current extractor is an
//! optional transitional backend until a Rust parser matches the event corpus.

use crate::analysis::{AnalyzedMatch, build_match, validate_import};
pub use crate::domain::ParserCapabilities;
use crate::db::{self, queries};
use crate::parser::raw::RawDemo;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

const CS2_DEMO_MAGIC: &[u8; 8] = b"PBDEMS2\0";

const ACTIVE: u8 = 0;
const CANCELLED: u8 = 1;
const FINALIZING: u8 = 2;
const COMPLETE: u8 = 3;

/// Cancellation is accepted only before the database commit boundary.
#[derive(Default, Clone)]
pub struct ImportControl(Arc<AtomicU8>);

impl ImportControl {
    pub fn new() -> Self { Self(Arc::new(AtomicU8::new(ACTIVE))) }
    pub fn is_cancelled(&self) -> bool { self.0.load(Ordering::Acquire) == CANCELLED }
    pub fn can_cancel(&self) -> bool { self.0.load(Ordering::Acquire) == ACTIVE }
    pub fn request_cancel(&self) -> bool {
        self.0.compare_exchange(ACTIVE, CANCELLED, Ordering::AcqRel, Ordering::Acquire).is_ok()
    }
    fn begin_finalizing(&self) -> Result<(), String> {
        self.0.compare_exchange(ACTIVE, FINALIZING, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| "demo import cancelled before database commit".to_string())
    }
    fn complete(&self) { self.0.store(COMPLETE, Ordering::Release); }
}

pub trait DemoFactsParser: Send + Sync {
    fn parse(&self, path: &Path, control: &ImportControl) -> Result<RawDemo, String>;
    fn capabilities(&self) -> ParserCapabilities { ParserCapabilities::ReferenceRawFacts }
}

#[derive(Debug, Clone)]
pub struct ExternalExtractor {
    pub node: PathBuf,
    pub script: PathBuf,
}

impl ExternalExtractor {
    pub fn check_available(&self) -> Result<(), String> {
        if !self.node.is_file() {
            return Err(format!("Node runtime is missing: {}", self.node.display()));
        }
        if !self.script.is_file() {
            return Err(format!("CsThrowBack extractor script is missing: {}", self.script.display()));
        }
        let module = self.script.parent().unwrap_or_else(|| Path::new("."))
            .join("node_modules/@laihoe/demoparser2/package.json");
        if !module.is_file() {
            return Err(format!("@laihoe/demoparser2 is missing beside the extractor: {}", module.display()));
        }
        let package: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&module).map_err(|error| error.to_string())?
        ).map_err(|error| format!("invalid parser package metadata: {error}"))?;
        let version = package.get("version").and_then(|value| value.as_str()).unwrap_or("unknown");
        if version != "0.41.3" {
            return Err(format!("CsThrowBack extractor requires @laihoe/demoparser2 0.41.3; found {version}"));
        }
        Ok(())
    }
}

impl DemoFactsParser for ExternalExtractor {
    fn parse(&self, path: &Path, control: &ImportControl) -> Result<RawDemo, String> {
        self.check_available()?;
        if control.is_cancelled() { return Err("demo import cancelled".into()); }
        let temp = tempfile::tempdir().map_err(|error| error.to_string())?;
        let output = temp.path().join("facts.json");
        let stderr_path = temp.path().join("extractor-error.log");
        let stderr_file = File::create(&stderr_path).map_err(|error| error.to_string())?;
        let mut child = Command::new(&self.node)
            .arg(&self.script)
            .arg(path)
            .arg(&output)
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr_file))
            .spawn()
            .map_err(|error| format!("could not start configured demo extractor: {error}"))?;
        let status = loop {
            if control.is_cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                return Err("demo import cancelled; original demo and prior database entry are intact".into());
            }
            match child.try_wait().map_err(|error| error.to_string())? {
                Some(status) => break status,
                None => std::thread::sleep(Duration::from_millis(50)),
            }
        };
        if !status.success() {
            let detail = std::fs::read_to_string(&stderr_path).unwrap_or_default();
            return Err(format!("demo extractor exited with {status}: {}",
                detail.trim().chars().take(2000).collect::<String>()));
        }
        if control.is_cancelled() { return Err("demo import cancelled".into()); }
        let bytes = std::fs::read(&output)
            .map_err(|error| format!("extractor did not write raw facts: {error}"))?;
        let raw: RawDemo = serde_json::from_slice(&bytes)
            .map_err(|error| format!("invalid raw facts from extractor: {error}"))?;
        if raw.version != 1 {
            return Err(format!("extractor output version {} is unsupported (expected 1)", raw.version));
        }
        Ok(raw)
    }
}

pub fn validate_demo_file(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!("demo file does not exist: {}", path.display()));
    }
    if !path.extension().and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("dem")) {
        return Err(format!("expected a .dem file: {}", path.display()));
    }
    let mut header = [0_u8; 8];
    File::open(path).and_then(|mut file| file.read_exact(&mut header))
        .map_err(|error| format!("could not read demo header: {error}"))?;
    if &header != CS2_DEMO_MAGIC {
        return Err("not a CS2 PBDEMS2 demo".into());
    }
    Ok(())
}

#[derive(Debug)]
pub struct ImportedMatch {
    pub analyzed: AnalyzedMatch,
    pub match_id: i64,
    pub capabilities: ParserCapabilities,
}

pub fn import_demo(
    path: &Path,
    database: &Path,
    parser: &dyn DemoFactsParser,
    control: &ImportControl,
    mut report_stage: impl FnMut(&'static str),
) -> Result<ImportedMatch, String> {
    validate_demo_file(path)?;
    if control.is_cancelled() { return Err("demo import cancelled".into()); }
    report_stage("Parsing demo");
    let raw = parser.parse(path, control)?;
    let capabilities = parser.capabilities();
    if control.is_cancelled() { return Err("demo import cancelled".into()); }
    if raw.version != 1 { return Err(format!("raw demo version {} is unsupported", raw.version)); }
    report_stage("Analyzing utility events");
    let analyzed = build_match(&raw);
    validate_import(&raw, &analyzed).map_err(|error| error.to_string())?;
    if control.is_cancelled() { return Err("demo import cancelled".into()); }
    report_stage("Saving match");
    let mut connection = db::open(database).map_err(|error| error.to_string())?;
    let match_id = queries::insert_match_with_parser_capability(
        &mut connection, &path.to_string_lossy(), &analyzed, capabilities.key(),
        || control.is_cancelled(),
        || { control.begin_finalizing().map_err(crate::error::AppError::Invalid)?;
            report_stage("Finalizing match"); Ok(()) },
    )
        .map_err(|error| error.to_string())?;
    control.complete();
    report_stage("Complete");
    Ok(ImportedMatch { analyzed, match_id, capabilities })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixtureParser { cancel_after_parse: bool }
    impl DemoFactsParser for FixtureParser {
        fn parse(&self, _path: &Path, control: &ImportControl) -> Result<RawDemo, String> {
            let raw = serde_json::from_slice(br#"{
                "version":1,"header":{"mapName":"de_train","serverName":null,
                "demoTicks":6400,"playbackTime":100.0}
            }"#).unwrap();
            if self.cancel_after_parse { control.request_cancel(); }
            Ok(raw)
        }
    }

    #[test]
    fn validates_real_cs2_header_and_rejects_mislabeled_files() {
        let dir = tempfile::tempdir().unwrap();
        let demo = dir.path().join("sample.dem");
        std::fs::write(&demo, b"PBDEMS2\0fixture").unwrap();
        validate_demo_file(&demo).unwrap();
        std::fs::write(&demo, b"fake demo").unwrap();
        assert!(validate_demo_file(&demo).unwrap_err().contains("not a CS2"));
        assert!(validate_demo_file(&dir.path().join("missing.dem")).is_err());
    }

    #[test]
    fn parser_contract_saves_match_and_cancelled_reimport_preserves_it() {
        let dir = tempfile::tempdir().unwrap();
        let demo = dir.path().join("fixture.dem");
        let database = dir.path().join("matches.sqlite");
        std::fs::write(&demo, b"PBDEMS2\0fixture").unwrap();
        let cancelled = ImportControl::new();
        let mut stages = Vec::new();
        let imported = import_demo(&demo, &database, &FixtureParser { cancel_after_parse: false },
            &cancelled, |stage| stages.push(stage)).unwrap();
        assert_eq!(imported.analyzed.map_name, "de_train");
        assert_eq!(stages, ["Parsing demo", "Analyzing utility events", "Saving match", "Finalizing match", "Complete"]);
        assert_eq!(db::queries::list_matches(&db::open(&database).unwrap()).unwrap().len(), 1);
        let second_control = ImportControl::new();
        let failed = import_demo(&demo, &database, &FixtureParser { cancel_after_parse: true },
            &second_control, |_| {});
        assert!(failed.unwrap_err().contains("cancelled"));
        let matches = db::queries::list_matches(&db::open(&database).unwrap()).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].map_name, "de_train");
    }

    #[test]
    fn cancellation_before_commit_rolls_back_and_finalizing_rejects_cancellation() {
        let dir = tempfile::tempdir().unwrap();
        let demo = dir.path().join("fixture.dem");
        let database = dir.path().join("matches.sqlite");
        std::fs::write(&demo, b"PBDEMS2\0fixture").unwrap();
        let parser = FixtureParser { cancel_after_parse: false };

        let early = ImportControl::new();
        let result = import_demo(&demo, &database, &parser, &early, |stage| {
            if stage == "Saving match" { assert!(early.request_cancel()); }
        });
        assert!(result.unwrap_err().contains("cancelled"));
        assert!(db::queries::list_matches(&db::open(&database).unwrap()).unwrap().is_empty());

        let late = ImportControl::new();
        let mut finalizing_seen = false;
        import_demo(&demo, &database, &parser, &late, |stage| {
            if stage == "Finalizing match" {
                finalizing_seen = true;
                assert!(!late.request_cancel());
                assert!(!late.can_cancel());
            }
        }).unwrap();
        assert!(finalizing_seen);
        assert!(!late.request_cancel());
        assert_eq!(db::queries::list_matches(&db::open(&database).unwrap()).unwrap().len(), 1);
    }

    #[test]
    fn missing_external_parser_dependency_fails_without_running_any_binary() {
        let parser = ExternalExtractor {
            node: PathBuf::from("missing-node.exe"),
            script: PathBuf::from("missing-extractor.mjs"),
        };
        assert!(parser.check_available().unwrap_err().contains("Node runtime is missing"));
        let dir = tempfile::tempdir().unwrap();
        let node = dir.path().join("node.exe");
        let script = dir.path().join("extract_demo.mjs");
        std::fs::write(&node, b"not executable").unwrap();
        std::fs::write(&script, b"not executed").unwrap();
        let parser = ExternalExtractor { node, script };
        assert!(parser.check_available().unwrap_err().contains("@laihoe/demoparser2 is missing"));
        let module = dir.path().join("node_modules/@laihoe/demoparser2");
        std::fs::create_dir_all(&module).unwrap();
        std::fs::write(module.join("package.json"), br#"{"version":"0.40.0"}"#).unwrap();
        assert!(parser.check_available().unwrap_err().contains("requires @laihoe/demoparser2 0.41.3"));
    }
}
