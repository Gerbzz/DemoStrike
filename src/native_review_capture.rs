//! Source-bound receipt for a screenshot of the actual native Rust review view.
//! Not a gameplay/video capture receipt and not a whole-product parity claim.
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize)]
pub(crate) struct NativeReviewPaint {
    pub database: PathBuf,
    pub match_id: i64,
    pub source_sha256: String,
    pub map: String,
    pub round: i64,
    pub tick: i64,
    pub viewer_paint_frames: u64,
    pub observed_players: usize,
    pub observed_grenades: usize,
    pub recorded_events: usize,
    pub radar_loaded: bool,
    pub zoom: f32,
    pub focused_player: Option<String>,
    pub radar_generation: u64,
}

fn hash_file(path: &Path, cap: u64) -> Result<(String, u64), String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("capture binding requires an ordinary file".into());
    }
    let mut digest = Sha256::new();
    let mut bytes = 0u64;
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut chunk).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        bytes = bytes.checked_add(n as u64).ok_or("file size overflow")?;
        if bytes > cap {
            return Err("capture binding exceeds supported file budget".into());
        }
        digest.update(&chunk[..n]);
    }
    Ok((format!("{:x}", digest.finalize()), bytes))
}

pub(crate) fn save_native_review_capture(
    path: &Path,
    screenshot: &Path,
    paint: &NativeReviewPaint,
) -> Result<(), String> {
    if paint.viewer_paint_frames < 2
        || paint.match_id <= 0
        || paint.round <= 0
        || paint.source_sha256.len() != 64
        || !paint.source_sha256.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("native review screenshot has no settled source-bound viewer paint".into());
    }
    // A renderer label is not an attestation: the independent hosted checker must
    // bind this native-only producer to its expected database, executable and source.
    let png = std::fs::File::open(screenshot).map_err(|e| e.to_string())?;
    let mut png_bytes_buffer = Vec::new();
    png.take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut png_bytes_buffer)
        .map_err(|e| e.to_string())?;
    if png_bytes_buffer.len() > 16 * 1024 * 1024 {
        return Err("native screenshot exceeds16MiB".into());
    }
    let mut reader = image::ImageReader::with_format(
        std::io::Cursor::new(&png_bytes_buffer),
        image::ImageFormat::Png,
    );
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(64 * 1024 * 1024);
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|e| format!("native screenshot is not a bounded decodable PNG: {e}"))?;
    if decoded.width() < 64 || decoded.height() < 64 {
        return Err("native screenshot dimensions are too small".into());
    }
    let png_sha256 = format!("{:x}", Sha256::digest(&png_bytes_buffer));
    let png_bytes = png_bytes_buffer.len();
    let (database_sha256, database_bytes) = hash_file(&paint.database, 2 * 1024 * 1024 * 1024)?;
    let receipt = serde_json::json!({
        "schema":"demostrike-native-rust-review-capture-v1",
        "renderer":"native-rust-egui-wgpu",
        "scope":"real native match/round/2D screenshot only; not video, CS2/HLAE invocation, or complete CSDM parity",
        "paint":paint,
        "build":{"declared_commit":option_env!("DEMOSTRIKE_REVIEW_BUILD_COMMIT"),
                 "executable_sha256":hash_file(&std::env::current_exe().map_err(|e|e.to_string())?,512*1024*1024)?.0},
        "png_width":decoded.width(),"png_height":decoded.height(),
        "screenshot":screenshot,
        "png_sha256":png_sha256,"png_bytes":png_bytes,
        "database_sha256":database_sha256,"database_bytes":database_bytes,
    });
    let parent = path
        .parent()
        .ok_or("capture receipt needs a parent directory")?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(&mut temp, &receipt).map_err(|e| e.to_string())?;
    temp.write_all(b"\n").map_err(|e| e.to_string())?;
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    temp.persist_noclobber(path)
        .map_err(|e| format!("create native capture receipt without overwrite: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(dir: &Path) -> (PathBuf, NativeReviewPaint) {
        let database = dir.join("synthetic.sqlite");
        let png = dir.join("synthetic.png");
        std::fs::write(&database, b"SYNTHETIC database only").unwrap();
        std::fs::write(
            &png,
            include_bytes!("../assets/branding/DemoStrike-geometric.png"),
        )
        .unwrap();
        (
            png,
            NativeReviewPaint {
                database,
                match_id: 1,
                source_sha256: "a".repeat(64),
                map: "SYNTHETIC".into(),
                round: 1,
                tick: 100,
                viewer_paint_frames: 2,
                observed_players: 0,
                observed_grenades: 0,
                recorded_events: 0,
                radar_loaded: false,
                zoom: 1.0,
                focused_player: None,
                radar_generation: 0,
            },
        )
    }
    #[test]
    fn capture_receipt_binds_files_and_never_overwrites_existing_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let (png, paint) = fixture(dir.path());
        let path = dir.path().join("proof.json");
        save_native_review_capture(&path, &png, &paint).unwrap();
        let saved = std::fs::read(&path).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&saved).unwrap();
        assert_eq!(
            value["png_sha256"],
            format!("{:x}", Sha256::digest(std::fs::read(&png).unwrap()))
        );
        assert_eq!(
            value["database_sha256"],
            format!(
                "{:x}",
                Sha256::digest(std::fs::read(&paint.database).unwrap())
            )
        );
        assert!(save_native_review_capture(&path, &png, &paint).is_err());
        assert_eq!(std::fs::read(path).unwrap(), saved);
    }
    #[test]
    fn plain_bytes_cannot_be_labeled_a_native_screenshot() {
        let dir = tempfile::tempdir().unwrap();
        let (png, paint) = fixture(dir.path());
        let path = dir.path().join("proof.json");
        std::fs::write(&png, b"not a PNG").unwrap();
        assert!(save_native_review_capture(&path, &png, &paint).is_err());
        assert!(!path.exists());
    }
    #[test]
    fn unsettled_or_unbound_paint_creates_no_receipt() {
        let dir = tempfile::tempdir().unwrap();
        let (png, mut paint) = fixture(dir.path());
        let path = dir.path().join("proof.json");
        paint.viewer_paint_frames = 1;
        assert!(save_native_review_capture(&path, &png, &paint).is_err());
        paint.viewer_paint_frames = 2;
        paint.source_sha256.clear();
        assert!(save_native_review_capture(&path, &png, &paint).is_err());
        assert!(!path.exists());
    }
}
