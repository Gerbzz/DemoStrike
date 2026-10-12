//! Real `.dem` import seam for the native UI. DemoStrike keeps raw-facts
//! analysis and SQLite transactions in Rust. The optional external extractor
//! preserves import of the richer raw-facts schema while native coverage grows.

use crate::analysis::{AnalyzedMatch, build_match, validate_import};
pub use crate::domain::ParserCapabilities;
use crate::db::{self, queries};
use crate::parser::raw::RawDemo;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

const CS2_DEMO_MAGIC: &[u8; 8] = b"PBDEMS2\0";

const ACTIVE: u8 = 0;
const CANCELLED: u8 = 1;
const FINALIZING: u8 = 2;
const COMPLETE: u8 = 3;

/// Cancellation is accepted only before the database commit boundary.
#[derive(Default, Clone)]
pub struct ImportControl {
    state: Arc<AtomicU8>,
    parsed_tick: Arc<AtomicU32>,
    expected_tick: Arc<AtomicU32>,
    copied_bytes: Arc<AtomicU64>,
    source_bytes: Arc<AtomicU64>,
}

impl ImportControl {
    pub fn new() -> Self { Self::default() }
    pub fn is_cancelled(&self) -> bool { self.state.load(Ordering::Acquire) == CANCELLED }
    pub fn can_cancel(&self) -> bool { self.state.load(Ordering::Acquire) == ACTIVE }
    pub fn request_cancel(&self) -> bool {
        self.state.compare_exchange(ACTIVE, CANCELLED, Ordering::AcqRel, Ordering::Acquire).is_ok()
    }
    pub fn parsing_progress(&self) -> Option<(u32, u32)> {
        let expected = self.expected_tick.load(Ordering::Relaxed);
        (expected > 0).then(|| (self.parsed_tick.load(Ordering::Relaxed).min(expected), expected))
    }
    pub fn snapshot_progress(&self) -> Option<(u64, u64)> {
        let total = self.source_bytes.load(Ordering::Relaxed);
        (total > 0).then(|| (self.copied_bytes.load(Ordering::Relaxed).min(total), total))
    }
    fn set_source_bytes(&self, total: u64) { self.source_bytes.store(total, Ordering::Relaxed); }
    fn set_copied_bytes(&self, copied: u64) { self.copied_bytes.store(copied, Ordering::Relaxed); }
    pub(crate) fn set_expected_ticks(&self, expected: u32) { self.expected_tick.store(expected, Ordering::Relaxed); }
    pub(crate) fn set_parsed_tick(&self, tick: u32) { self.parsed_tick.store(tick, Ordering::Relaxed); }
    fn begin_finalizing(&self) -> Result<(), String> {
        self.state.compare_exchange(ACTIVE, FINALIZING, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| "demo import cancelled before database commit".to_string())
    }
    fn complete(&self) { self.state.store(COMPLETE, Ordering::Release); }
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
            return Err(format!("configured DemoStrike reference extractor script is missing: {}", self.script.display()));
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
            return Err(format!("DemoStrike reference import requires @laihoe/demoparser2 0.41.3; found {version}"));
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

fn source_hash(path: &Path, control: &ImportControl) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| format!("cannot identify {}: {e}", path.display()))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        if control.is_cancelled() { return Err("demo import cancelled while identifying source".into()); }
        let count = file.read(&mut buffer).map_err(|e| format!("cannot hash {}: {e}", path.display()))?;
        if count == 0 { break; }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

/// Parse a private byte snapshot so a mutable source path cannot mix versions
/// during decoding. Keep the snapshot alive through the database commit.
fn snapshot_source(path: &Path, control: &ImportControl) -> Result<(tempfile::TempDir, PathBuf, String), String> {
    let temp = tempfile::tempdir().map_err(|e| format!("cannot create demo snapshot: {e}"))?;
    let snapshot = temp.path().join("source.dem");
    let mut source = File::open(path).map_err(|e| format!("cannot snapshot {}: {e}", path.display()))?;
    control.set_source_bytes(source.metadata().map_err(|e| format!("cannot size {}: {e}", path.display()))?.len());
    let mut target = File::create(&snapshot).map_err(|e| format!("cannot create demo snapshot: {e}"))?;
    let mut digest = Sha256::new();
    // Windows CLI main threads can have a 1 MiB stack; keep copy buffers on
    // the heap so import works outside the larger test/UI worker stacks.
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut copied = 0_u64;
    loop {
        if control.is_cancelled() { return Err("demo import cancelled while identifying source".into()); }
        let count = source.read(&mut buffer).map_err(|e| format!("cannot read source snapshot: {e}"))?;
        if count == 0 { break; }
        target.write_all(&buffer[..count]).map_err(|e| format!("cannot write demo snapshot: {e}"))?;
        digest.update(&buffer[..count]);
        copied += count as u64;
        control.set_copied_bytes(copied);
    }
    target.flush().map_err(|e| format!("cannot finish demo snapshot: {e}"))?;
    drop(target);
    Ok((temp, snapshot, format!("{:x}", digest.finalize())))
}

fn source_path_identity(path: &Path) -> Option<String> {
    let text = std::fs::canonicalize(path).ok()?.to_string_lossy().into_owned();
    #[cfg(windows)] { Some(text.to_lowercase()) }
    #[cfg(not(windows))] { Some(text) }
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
    // An already imported path can be rejected before copying a multi-GB demo.
    // The transactional check below still handles a concurrent import race.
    let connection = db::open(database).map_err(|error| error.to_string())?;
    let canonical = source_path_identity(path);
    for (id, old_path, _) in queries::source_identities(&connection).map_err(|error| error.to_string())? {
        if old_path == path.to_string_lossy() {
            return Err(format!("demo is already imported as match {id}; existing annotations were preserved"));
        }
        if canonical.is_some() && source_path_identity(Path::new(&old_path)) == canonical {
            return Err(format!("demo is already imported as match {id} through an equivalent source path; existing annotations were preserved"));
        }
    }
    drop(connection);
    report_stage("Copying source");
    let (_snapshot_dir, snapshot, fingerprint) = snapshot_source(path, control)?;
    report_stage("Verifying source copy");
    if source_hash(path, control)? != fingerprint {
        return Err("demo bytes changed while creating the import snapshot; no match was saved".into());
    }
    let connection = db::open(database).map_err(|error| error.to_string())?;
    let canonical = source_path_identity(path);
    for (id, old_path, stored_hash) in queries::source_identities(&connection).map_err(|error| error.to_string())? {
        if old_path == path.to_string_lossy() {
            return Err(format!("demo is already imported as match {id}; existing annotations were preserved"));
        }
        if canonical.is_some() && source_path_identity(Path::new(&old_path)) == canonical {
            return Err(format!("demo is already imported as match {id} through an equivalent source path; existing annotations were preserved"));
        }
        let old_hash = match stored_hash {
            Some(hash) => Some(hash),
            None if Path::new(&old_path).is_file() => Some(source_hash(Path::new(&old_path), control)?),
            None => None,
        };
        if old_hash.as_deref() == Some(fingerprint.as_str()) {
            return Err(format!("identical demo bytes are already imported as match {id}; existing annotations were preserved"));
        }
    }
    drop(connection);
    report_stage("Parsing demo");
    let raw = parser.parse(&snapshot, control)?;
    let capabilities = parser.capabilities();
    if control.is_cancelled() { return Err("demo import cancelled".into()); }
    report_stage("Verifying source");
    if source_hash(&snapshot, control)? != fingerprint {
        return Err("demo snapshot changed during import; no match was saved".into());
    }
    if raw.version != 1 { return Err(format!("raw demo version {} is unsupported", raw.version)); }
    report_stage("Analyzing utility events");
    let analyzed = build_match(&raw);
    let summary = validate_import(&raw, &analyzed).map_err(|error| error.to_string())?;
    if capabilities.is_native() && summary.path_count != summary.throw_count {
        return Err(format!("native grenade analysis linked only {} of {} releases to usable trajectories; no match was saved",
            summary.path_count, summary.throw_count));
    }
    if control.is_cancelled() { return Err("demo import cancelled".into()); }
    report_stage("Saving match");
    let mut connection = db::open(database).map_err(|error| error.to_string())?;
    let match_id = queries::insert_match_with_source_hash(
        &mut connection, &path.to_string_lossy(), &analyzed, capabilities.key(), Some(&fingerprint),
        || control.is_cancelled(),
        || { if source_hash(path, control).map_err(crate::error::AppError::Invalid)? != fingerprint {
                return Err(crate::error::AppError::Invalid("demo bytes changed before database commit; no match was saved".into()));
            }
            control.begin_finalizing().map_err(crate::error::AppError::Invalid)?;
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
    fn parsing_progress_is_shared_with_ui_control() {
        let control = ImportControl::new();
        let worker = control.clone();
        assert_eq!(control.parsing_progress(), None);
        assert_eq!(control.snapshot_progress(), None);
        worker.set_source_bytes(100);
        worker.set_copied_bytes(60);
        assert_eq!(control.snapshot_progress(), Some((60, 100)));
        worker.set_copied_bytes(120);
        assert_eq!(control.snapshot_progress(), Some((100, 100)));
        worker.set_expected_ticks(100);
        worker.set_parsed_tick(60);
        assert_eq!(control.parsing_progress(), Some((60, 100)));
        worker.set_parsed_tick(120);
        assert_eq!(control.parsing_progress(), Some((100, 100)));
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
        assert_eq!(stages, ["Copying source", "Verifying source copy", "Parsing demo", "Verifying source", "Analyzing utility events", "Saving match", "Finalizing match", "Complete"]);
        assert_eq!(db::queries::list_matches(&db::open(&database).unwrap()).unwrap().len(), 1);
        let repeat_control = ImportControl::new();
        let repeat = import_demo(&demo, &database, &FixtureParser { cancel_after_parse: false },
            &repeat_control, |_| {}).unwrap_err();
        assert!(repeat.contains("already imported"));
        assert_eq!(repeat_control.snapshot_progress(), None, "duplicate path must not be recopied");
        let other_demo = dir.path().join("other-source.dem");
        std::fs::write(&other_demo, b"PBDEMS2\0different fixture").unwrap();
        let second_control = ImportControl::new();
        let failed = import_demo(&other_demo, &database, &FixtureParser { cancel_after_parse: true },
            &second_control, |_| {});
        assert!(failed.unwrap_err().contains("cancelled"));
        let matches = db::queries::list_matches(&db::open(&database).unwrap()).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].map_name, "de_train");

        let copied = dir.path().join("same-bytes.dem");
        std::fs::copy(&demo, &copied).unwrap();
        let error = import_demo(&copied, &database, &FixtureParser { cancel_after_parse: false },
            &ImportControl::new(), |_| {}).unwrap_err();
        assert!(error.contains("identical demo bytes"));
        assert_eq!(db::queries::list_matches(&db::open(&database).unwrap()).unwrap().len(), 1);
    }

    #[test]
    fn cancellation_before_snapshot_copy_preserves_database() {
        let dir = tempfile::tempdir().unwrap();
        let demo = dir.path().join("fixture.dem");
        let database = dir.path().join("matches.sqlite");
        std::fs::write(&demo, b"PBDEMS2\0fixture").unwrap();
        let control = ImportControl::new();
        let result = import_demo(&demo, &database, &FixtureParser { cancel_after_parse: false },
            &control, |stage| {
                if stage == "Copying source" { assert!(control.request_cancel()); }
            });
        assert!(result.unwrap_err().contains("cancelled"));
        assert!(db::queries::list_matches(&db::open(&database).unwrap()).unwrap().is_empty());
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
    fn source_mutation_at_saving_match_rolls_back_snapshot_import() {
        let dir = tempfile::tempdir().unwrap();
        let demo = dir.path().join("fixture.dem");
        let database = dir.path().join("matches.sqlite");
        std::fs::write(&demo, b"PBDEMS2\0original bytes").unwrap();
        let result = import_demo(&demo, &database,
            &FixtureParser { cancel_after_parse: false }, &ImportControl::new(), |stage| {
                if stage == "Saving match" {
                    std::fs::write(&demo, b"PBDEMS2\0changed bytes").unwrap();
                }
            });
        assert!(result.unwrap_err().contains("changed before database commit"));
        assert!(db::queries::list_matches(&db::open(&database).unwrap()).unwrap().is_empty());
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
