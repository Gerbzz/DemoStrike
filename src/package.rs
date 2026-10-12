use crate::media_probe::probe_muxed_clip_with_cancel;
use crate::capture_receipt::CaptureReceipt;
use crate::capture_request::{CAPTURE_REQUEST_VERSION, CaptureRequest};
use clipstrike_core::capture_package::{
    CAPTURE_SCHEMA_VERSION, CaptureClip, CaptureManifest, CapturePackage, MediaValidation,
    PackageError, sha256_file_with_cancel, validate_clip_fields,
};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::HashSet;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct CaptureInput {
    pub source_path: PathBuf,
    pub clip: CaptureClip,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PackageDraft {
    pub package_id: String,
    pub inputs: Vec<CaptureInput>,
}

const REVIEW_BINDING_FILE: &str = "demostrike-review-binding.json";
const REVIEW_REQUEST_FILE: &str = "demostrike-review-request.json";
const REVIEW_RECEIPT_FILE: &str = "demostrike-review-receipt.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewBinding {
    pub version: u32,
    pub package_id: String,
    pub request_path: PathBuf,
    pub request_sha256: String,
    pub receipt_path: PathBuf,
    pub receipt_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_file: Option<String>,
}

/// Version 1 preserves its original absolute-source rule. Version 2 verifies
/// exact request and receipt evidence carried by the package after relocation.
pub fn verify_review_binding_with_cancel(root: &Path, package_id: &str,
    should_cancel: impl Fn() -> bool) -> Result<(), PackageError> {
    let suffix = package_id.rsplit_once("-r").and_then(|(head, digest)|
        (head.starts_with("demostrike-") && digest.len() == 16
            && digest.bytes().all(|b| b.is_ascii_hexdigit())).then_some(digest));
    let path = root.join(REVIEW_BINDING_FILE);
    if suffix.is_none() && !path.exists() { return Ok(()); }
    let suffix = suffix.ok_or_else(|| PackageError::Invalid("unexpected review binding on package".into()))?;
    if !path.is_file() || std::fs::metadata(&path).map_err(|e| PackageError::Io(e.to_string()))?.len() > 4096 {
        return Err(PackageError::Invalid("source-bound package lacks a bounded review binding".into()));
    }
    let binding: ReviewBinding = serde_json::from_slice(&std::fs::read(&path)
        .map_err(|e| PackageError::Io(e.to_string()))?)
        .map_err(|e| PackageError::Json(e.to_string()))?;
    if !matches!(binding.version, 1 | 2) || binding.package_id != package_id
        || !binding.receipt_path.is_absolute() || !binding.request_path.is_absolute()
        || binding.receipt_sha256.len() != 64 || binding.request_sha256.len() != 64
        || &binding.receipt_sha256[..16] != suffix {
        return Err(PackageError::Invalid("review binding differs from sealed package ID".into()));
    }
    if binding.version == 1 {
        if binding.request_file.is_some() || binding.receipt_file.is_some()
            || sha256_file_with_cancel(&binding.receipt_path, &should_cancel)? != binding.receipt_sha256
            || sha256_file_with_cancel(&binding.request_path, &should_cancel)? != binding.request_sha256 {
            return Err(PackageError::Invalid("review receipt or request changed since package sealing".into()));
        }
    } else {
        if binding.request_file.as_deref() != Some(REVIEW_REQUEST_FILE)
            || binding.receipt_file.as_deref() != Some(REVIEW_RECEIPT_FILE) {
            return Err(PackageError::Invalid("portable review evidence names are invalid".into()));
        }
        let read_bounded = |name: &str| -> Result<Vec<u8>, PackageError> {
            let path = root.join(name);
            if !path.is_file() || std::fs::metadata(&path)
                .map_err(|e| PackageError::Io(e.to_string()))?.len() > 4 * 1024 * 1024 {
                return Err(PackageError::Invalid(format!("portable review evidence is missing or oversized: {name}")));
            }
            std::fs::read(&path).map_err(|e| PackageError::Io(e.to_string()))
        };
        let request_bytes = read_bounded(REVIEW_REQUEST_FILE)?;
        let receipt_bytes = read_bounded(REVIEW_RECEIPT_FILE)?;
        let hash = |bytes: &[u8]| format!("{:x}", sha2::Sha256::digest(bytes));
        if should_cancel() { return Err(PackageError::Cancelled("review verification cancelled".into())); }
        if hash(&request_bytes) != binding.request_sha256
            || hash(&receipt_bytes) != binding.receipt_sha256 {
            return Err(PackageError::Invalid("portable review request or receipt hash differs from seal".into()));
        }
        let request: CaptureRequest = serde_json::from_slice(&request_bytes)
            .map_err(|e| PackageError::Json(e.to_string()))?;
        let receipt: CaptureReceipt = serde_json::from_slice(&receipt_bytes)
            .map_err(|e| PackageError::Json(e.to_string()))?;
        let manifest: CaptureManifest = serde_json::from_slice(&read_bounded_manifest(root)?)
            .map_err(|e| PackageError::Json(e.to_string()))?;
        if request.version != CAPTURE_REQUEST_VERSION || receipt.version != 2
            || receipt.request_sha256 != binding.request_sha256
            || receipt.demo_sha256 != request.demo_sha256 || receipt.event_id != request.event_id
            || manifest.package_id != package_id || manifest.clips.len() != receipt.takes.len()
            || request.takes.len() != receipt.takes.len() {
            return Err(PackageError::Invalid("portable review evidence does not bind this package".into()));
        }
        let mut roles = HashSet::new();
        for take in &receipt.takes {
            if !roles.insert(&take.camera_role)
                || !request.takes.iter().any(|planned| planned.camera_role == take.camera_role)
            || !take.full_video_decode || !take.full_audio_decode
            || !take.measured_audio_signal || take.visual_review.is_none()
            || !manifest.clips.iter().any(|clip| clip.camera_role == take.camera_role
                    && clip.sha256 == take.muxed_sha256
                    && clip.demo_sha256 == request.demo_sha256
                    && clip.event_id == request.event_id.to_string()
                    && clip.start_tick == take.actual_start_tick
                    && clip.end_tick == take.actual_end_tick) {
                return Err(PackageError::Invalid("portable receipt take differs from sealed clip".into()));
            }
        }
    }
    Ok(())
}

fn read_bounded_manifest(root: &Path) -> Result<Vec<u8>, PackageError> {
    let path = root.join("manifest.json");
    if std::fs::metadata(&path).map_err(|e| PackageError::Io(e.to_string()))?.len() > 4 * 1024 * 1024 {
        return Err(PackageError::Invalid("package manifest exceeds 4 MiB".into()));
    }
    std::fs::read(path).map_err(|e| PackageError::Io(e.to_string()))
}

pub fn write_portable_review_evidence_before_commit(stage: &Path, binding: &ReviewBinding)
    -> Result<(), PackageError> {
    if binding.version != 2 { return Err(PackageError::Invalid("portable review binding version required".into())); }
    for (source, name, expected) in [
        (&binding.request_path, REVIEW_REQUEST_FILE, &binding.request_sha256),
        (&binding.receipt_path, REVIEW_RECEIPT_FILE, &binding.receipt_sha256),
    ] {
        let bytes = std::fs::read(source).map_err(|e| PackageError::Io(e.to_string()))?;
        if bytes.len() > 4 * 1024 * 1024
            || format!("{:x}", sha2::Sha256::digest(&bytes)) != *expected {
            return Err(PackageError::Invalid(format!("{name} source changed before package commit")));
        }
        let path = stage.join(name);
        if path.exists() {
            if std::fs::read(&path).map_err(|e| PackageError::Io(e.to_string()))? != bytes {
                return Err(PackageError::Invalid(format!("staged {name} differs from reviewed source")));
            }
        } else {
            let mut temporary = tempfile::NamedTempFile::new_in(stage)
                .map_err(|e| PackageError::Io(e.to_string()))?;
            temporary.write_all(&bytes).and_then(|_| temporary.as_file().sync_all())
                .map_err(|e| PackageError::Io(e.to_string()))?;
            temporary.persist_noclobber(&path)
                .map_err(|e| PackageError::Io(e.error.to_string()))?;
        }
    }
    write_review_binding_before_commit(stage, binding)
}

pub fn write_review_binding_before_commit(stage: &Path,
    binding: &ReviewBinding) -> Result<(), PackageError> {
    let path = stage.join(REVIEW_BINDING_FILE);
    let bytes = serde_json::to_vec_pretty(binding).map_err(|e| PackageError::Json(e.to_string()))?;
    if bytes.len() > 4096 { return Err(PackageError::Invalid("review binding exceeds 4 KiB".into())); }
    if path.exists() {
        if std::fs::read(&path).map_err(|e| PackageError::Io(e.to_string()))? != bytes {
            return Err(PackageError::Invalid("staged review binding differs from current receipt".into()));
        }
    } else {
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&path)
            .map_err(|e| PackageError::Io(e.to_string()))?;
        file.write_all(&bytes).and_then(|_| file.sync_all())
            .map_err(|e| PackageError::Io(e.to_string()))?;
    }
    Ok(())
}

pub fn load_draft(path: &Path) -> Result<PackageDraft, PackageError> {
    let bytes = std::fs::read(path).map_err(|error| PackageError::Io(error.to_string()))?;
    let mut draft: PackageDraft = serde_json::from_slice(&bytes)
        .map_err(|error| PackageError::Json(error.to_string()))?;
    let base = path.parent().unwrap_or_else(|| Path::new("."))
        .canonicalize().map_err(|error| PackageError::Io(error.to_string()))?;
    for input in &mut draft.inputs {
        if input.source_path.is_relative() {
            input.source_path = base.join(&input.source_path);
        }
    }
    Ok(draft)
}

pub fn finalize_capture_package(
    destination: &Path,
    package_id: &str,
    inputs: &[CaptureInput],
) -> Result<CapturePackage, PackageError> {
    finalize_capture_package_with_cancel(destination, package_id, inputs, || false)
}

pub fn finalize_stream_probed_capture_package(
    destination: &Path,
    package_id: &str,
    inputs: &[CaptureInput],
) -> Result<CapturePackage, PackageError> {
    finalize_stream_probed_capture_package_with_cancel(destination, package_id, inputs, || false)
}

pub fn finalize_stream_probed_capture_package_with_cancel(
    destination: &Path,
    package_id: &str,
    inputs: &[CaptureInput],
    should_cancel: impl Fn() -> bool,
) -> Result<CapturePackage, PackageError> {
    finalize_stream_probed_capture_package_with_cancel_and_guard(
        destination, package_id, inputs, should_cancel, || Ok(()))
}

/// Recheck caller-owned evidence immediately before the staged-folder rename.
/// A changed review receipt must never commit an accepted package.
pub fn finalize_stream_probed_capture_package_with_cancel_and_guard(
    destination: &Path,
    package_id: &str,
    inputs: &[CaptureInput],
    should_cancel: impl Fn() -> bool,
    before_commit: impl Fn() -> Result<(), PackageError>,
) -> Result<CapturePackage, PackageError> {
    for input in inputs {
        probe_muxed_clip_with_cancel(&input.source_path, &input.clip, &should_cancel)?;
    }
    finalize_capture_package_with_cancel_and_level(
        destination,
        package_id,
        inputs,
        should_cancel,
        MediaValidation::StreamProbed,
        before_commit,
    )
}

pub fn staged_package_path(destination: &Path) -> PathBuf {
    let mut path = OsString::from(destination.as_os_str());
    path.push(".partial");
    PathBuf::from(path)
}

pub fn recover_complete_staged_package(destination: &Path) -> Result<CapturePackage, PackageError> {
    recover_complete_staged_package_with_cancel(destination, || false)
}

pub fn recover_complete_staged_package_with_cancel(
    destination: &Path,
    should_cancel: impl Fn() -> bool,
) -> Result<CapturePackage, PackageError> {
    recover_complete_staged_package_with_cancel_and_guard(destination, should_cancel, || Ok(()))
}

fn recover_complete_staged_package_with_cancel_and_guard(
    destination: &Path,
    should_cancel: impl Fn() -> bool,
    before_commit: impl Fn() -> Result<(), PackageError>,
) -> Result<CapturePackage, PackageError> {
    if destination.exists() {
        return Err(PackageError::Invalid(
            "capture package destination already exists".into(),
        ));
    }
    let stage = staged_package_path(destination);
    let marker = stage.join(".demostrike-incomplete");
    if !marker.is_file() {
        return Err(PackageError::Invalid(
            "no DemoStrike staged package to recover".into(),
        ));
    }
    let staged = CapturePackage::load_and_validate_with_cancel(&stage, &should_cancel)?;
    let stage_id =
        std::fs::read_to_string(&marker).map_err(|error| PackageError::Io(error.to_string()))?;
    if stage_id != staged.manifest.package_id {
        return Err(PackageError::Invalid(
            "staged package marker does not match manifest".into(),
        ));
    }
    if staged.manifest.media_validation == MediaValidation::StreamProbed {
        for clip in &staged.manifest.clips {
            probe_muxed_clip_with_cancel(&staged.media_path(clip), clip, &should_cancel)?;
        }
    }
    if should_cancel() {
        return Err(PackageError::Cancelled("capture package recovery cancelled".into()));
    }
    before_commit()?;
    verify_review_binding_with_cancel(&stage, &staged.manifest.package_id, &should_cancel)?;
    if should_cancel() {
        return Err(PackageError::Cancelled("capture package cancelled before commit".into()));
    }
    // This rename is the commit point. Cancellation after it cannot undo a
    // finished package, so do not report Cancelled once the folder exists.
    std::fs::rename(&stage, destination).map_err(|error| PackageError::Io(error.to_string()))?;
    let _ = std::fs::remove_file(destination.join(".demostrike-incomplete"));
    let root = destination.canonicalize().map_err(|error| PackageError::Io(error.to_string()))?;
    Ok(CapturePackage { root, manifest: staged.manifest })
}

pub fn finalize_capture_package_with_cancel(
    destination: &Path,
    package_id: &str,
    inputs: &[CaptureInput],
    should_cancel: impl Fn() -> bool,
) -> Result<CapturePackage, PackageError> {
    finalize_capture_package_with_cancel_and_level(
        destination,
        package_id,
        inputs,
        should_cancel,
        MediaValidation::Structural,
        || Ok(()),
    )
}

fn finalize_capture_package_with_cancel_and_level(
    destination: &Path,
    package_id: &str,
    inputs: &[CaptureInput],
    should_cancel: impl Fn() -> bool,
    media_validation: MediaValidation,
    before_commit: impl Fn() -> Result<(), PackageError>,
) -> Result<CapturePackage, PackageError> {
    if destination.exists() {
        return Err(PackageError::Invalid(
            "capture package destination already exists".into(),
        ));
    }
    if package_id.trim().is_empty() || inputs.is_empty() {
        return Err(PackageError::Invalid(
            "package ID and clips are required".into(),
        ));
    }
    let mut ids = HashSet::new();
    let mut orders = HashSet::new();
    for input in inputs {
        let id = &input.clip.clip_id;
        if id.is_empty()
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
            || !ids.insert(id.clone())
            || !orders.insert(input.clip.group_order)
            || !input.source_path.is_file()
            || !input
                .source_path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("mp4"))
        {
            return Err(PackageError::Invalid(format!(
                "invalid capture input: {id}"
            )));
        }
        let mut preview = input.clip.clone();
        preview.relative_path = format!("clips/{id}.mp4");
        preview.sha256 = "0".repeat(64);
        validate_clip_fields(&preview)?;
    }

    let stage = staged_package_path(destination);
    let marker = stage.join(".demostrike-incomplete");
    if stage.exists() {
        let stage_id = std::fs::read_to_string(&marker).map_err(|_| {
            PackageError::Invalid(format!("unrecognized staged folder: {}", stage.display()))
        })?;
        if stage_id != package_id {
            return Err(PackageError::Invalid(
                "staged package ID differs from draft".into(),
            ));
        }
        if stage.join("manifest.json").is_file() {
            let completed = CapturePackage::load_and_validate_with_cancel(&stage, &should_cancel)?;
            if completed.manifest.media_validation != media_validation {
                return Err(PackageError::Invalid(
                    "staged package validation level differs from request".into(),
                ));
            }
            let expected: Vec<_> = inputs.iter().map(|input| {
                let mut clip = input.clip.clone();
                clip.relative_path = format!("clips/{}.mp4", clip.clip_id);
                clip.sha256 = sha256_file_with_cancel(&input.source_path, &should_cancel)?;
                Ok(clip)
            }).collect::<Result<_, PackageError>>()?;
            if completed.manifest.package_id != package_id || completed.manifest.clips != expected {
                return Err(PackageError::Invalid(
                    "completed staged package differs from current draft; recover it separately".into(),
                ));
            }
            return recover_complete_staged_package_with_cancel_and_guard(destination, should_cancel, before_commit);
        }
        std::fs::create_dir_all(stage.join("clips"))
            .map_err(|error| PackageError::Io(error.to_string()))?;
    } else {
        std::fs::create_dir(&stage).map_err(|error| PackageError::Io(error.to_string()))?;
        std::fs::write(&marker, package_id).map_err(|error| PackageError::Io(error.to_string()))?;
        std::fs::create_dir(stage.join("clips"))
            .map_err(|error| PackageError::Io(error.to_string()))?;
    }
    let mut clips = Vec::with_capacity(inputs.len());
    for input in inputs {
        if should_cancel() {
            return Err(PackageError::Cancelled(format!(
                "capture package cancelled; resume from {}",
                stage.display()
            )));
        }
        let mut clip = input.clip.clone();
        clip.relative_path = format!("clips/{}.mp4", clip.clip_id);
        let target = stage.join(&clip.relative_path);
        let source_hash = sha256_file_with_cancel(&input.source_path, &should_cancel)?;
        if target.exists() {
            if sha256_file_with_cancel(&target, &should_cancel)? != source_hash {
                return Err(PackageError::HashMismatch(format!(
                    "staged clip differs from source: {}",
                    clip.clip_id
                )));
            }
        } else {
            let partial = target.with_extension("mp4.partial");
            if partial.exists() {
                std::fs::remove_file(&partial)
                    .map_err(|error| PackageError::Io(error.to_string()))?;
            }
            copy_with_cancel(&input.source_path, &partial, &should_cancel)?;
            if sha256_file_with_cancel(&partial, &should_cancel)? != source_hash {
                return Err(PackageError::HashMismatch(format!(
                    "incomplete or changed capture clip: {}",
                    clip.clip_id
                )));
            }
            std::fs::rename(&partial, &target)
                .map_err(|error| PackageError::Io(error.to_string()))?;
        }
        clip.sha256 = source_hash;
        if media_validation == MediaValidation::StreamProbed {
            probe_muxed_clip_with_cancel(&target, &clip, &should_cancel)?;
        }
        clips.push(clip);
    }
    if should_cancel() {
        return Err(PackageError::Cancelled(format!(
            "capture package cancelled; resume from {}",
            stage.display()
        )));
    }

    let manifest = CaptureManifest {
        schema_version: CAPTURE_SCHEMA_VERSION,
        package_id: package_id.to_owned(),
        media_validation,
        clips,
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| PackageError::Json(error.to_string()))?;
    let partial = stage.join("manifest.json.partial");
    std::fs::write(&partial, manifest_bytes)
        .map_err(|error| PackageError::Io(error.to_string()))?;
    std::fs::rename(&partial, stage.join("manifest.json"))
        .map_err(|error| PackageError::Io(error.to_string()))?;
    recover_complete_staged_package_with_cancel_and_guard(destination, should_cancel, before_commit)
}

fn copy_with_cancel(
    source: &Path,
    destination: &Path,
    should_cancel: &impl Fn() -> bool,
) -> Result<(), PackageError> {
    let mut input = std::fs::File::open(source)
        .map_err(|error| PackageError::Io(error.to_string()))?;
    let mut output = std::fs::File::create_new(destination)
        .map_err(|error| PackageError::Io(error.to_string()))?;
    let mut buffer = [0_u8; 256 * 1024];
    let mut copied = 0_u64;
    loop {
        let count = input.read(&mut buffer)
            .map_err(|error| PackageError::Io(error.to_string()))?;
        if count == 0 { break; }
        output.write_all(&buffer[..count])
            .map_err(|error| PackageError::Io(error.to_string()))?;
        copied += count as u64;
        if should_cancel() {
            output.sync_all().map_err(|error| PackageError::Io(error.to_string()))?;
            return Err(PackageError::Cancelled(format!(
                "capture package cancelled after {copied} bytes; resume from {}",
                destination.display()
            )));
        }
    }
    output.sync_all().map_err(|error| PackageError::Io(error.to_string()))?;
    if copied == 0 {
        return Err(PackageError::Invalid(format!(
            "capture source is empty: {}", source.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        CaptureInput, finalize_capture_package, finalize_capture_package_with_cancel,
        recover_complete_staged_package, staged_package_path,
    };
    use clipstrike_core::capture_package::{
        AudioEvidence, AudioSource, CaptureClip, PackageError, QcStatus,
    };
    use std::fs;

    #[test]
    fn source_bound_seal_rejects_edited_or_missing_receipt() {
        let dir = tempfile::tempdir().unwrap();
        let request = dir.path().join("request.json");
        let receipt = dir.path().join("receipt.json");
        fs::write(&request, b"request-one").unwrap();
        fs::write(&receipt, b"review-one").unwrap();
        let request_hash = clipstrike_core::capture_package::sha256_file_with_cancel(&request,
            &|| false).unwrap();
        let receipt_hash = clipstrike_core::capture_package::sha256_file_with_cancel(&receipt,
            &|| false).unwrap();
        let id = format!("demostrike-source-e3-r{}", &receipt_hash[..16]);
        let binding = super::ReviewBinding { version: 1, package_id: id.clone(),
            request_path: request, request_sha256: request_hash,
            receipt_path: receipt.clone(), receipt_sha256: receipt_hash,
            request_file: None, receipt_file: None };
        super::write_review_binding_before_commit(dir.path(), &binding).unwrap();
        super::verify_review_binding_with_cancel(dir.path(), &id, || false).unwrap();
        fs::write(&receipt, b"review-edited").unwrap();
        assert!(super::verify_review_binding_with_cancel(dir.path(), &id, || false)
            .unwrap_err().to_string().contains("changed since package sealing"));
        fs::remove_file(dir.path().join("demostrike-review-binding.json")).unwrap();
        assert!(super::verify_review_binding_with_cancel(dir.path(), &id, || false).is_err());
    }

    #[test]
    fn portable_review_evidence_survives_relocation_and_refuses_tampering() {
        use crate::capture_receipt::{CaptureReceipt, CaptureTakeReceipt, ProofFrame, VisualReview};
        use crate::capture_request::{CaptureRequest, PlannedTake};
        use crate::domain::Vec3;
        use sha2::{Digest, Sha256};
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old");
        fs::create_dir(&old).unwrap();
        let source_request = old.join("request.json");
        let source_receipt = old.join("receipt.json");
        let demo_hash = "a".repeat(64);
        let request = CaptureRequest { version: 1, demo_path: old.join("source.dem"),
            demo_sha256: demo_hash.clone(), demo_bytes: 3, match_id: 1, event_id: 3,
            map: "de_mirage".into(), round: 4, player_steamid64: "76561198000000001".into(),
            utility_type: "smoke".into(), projectile_entity_id: 7, observer_slot_candidate: 5,
            tick_rate: 64.0, throw_tick: 1050, detonate_tick: 1080,
            throw_position: Vec3 { x: 0.0, y: 0.0, z: 0.0 },
            detonate_position: Vec3 { x: 1.0, y: 1.0, z: 1.0 },
            flight_samples: vec![], takes: vec![PlannedTake { camera_role: "projectile".into(),
                start_tick: 1000, end_tick: 1100 }] };
        let request_bytes = serde_json::to_vec(&request).unwrap();
        fs::write(&source_request, &request_bytes).unwrap();
        let request_hash = format!("{:x}", Sha256::digest(&request_bytes));
        let review = VisualReview { reviewed_by: "reviewer".into(), notes: "watched".into(),
            review_extent: "continuous".into(), correct_player: true, release_observed: true,
            expected_utility_visible_in_pov: true, same_event_complete_flight: true,
            model_feet_stance_visible: true, clean_image: true, audible_game_audio: true,
            proof_frames: vec![ProofFrame { path: old.join("proof.png"),
                sha256: "b".repeat(64), tick: 1050 }] };
        let receipt = CaptureReceipt { version: 2, request_sha256: request_hash.clone(),
            demo_sha256: demo_hash.clone(), event_id: 3, takes: vec![CaptureTakeReceipt {
                camera_role: "projectile".into(), runtime_identity_log: old.join("identity.json"),
                runtime_identity_log_sha256: "c".repeat(64), issued_spec_player_value: 6,
                resolved_player_steamid64: request.player_steamid64.clone(),
                muxed_mp4: old.join("muxed.mp4"), muxed_sha256: "d".repeat(64), muxed_bytes: 123,
                hlae_wav: old.join("audio.wav"), hlae_wav_sha256: "e".repeat(64),
                hlae_wav_bytes: 456, actual_start_tick: 1000, actual_end_tick: 1100,
                duration_ms: 1000, width: 1920, height: 1080, fps_num: 60, fps_den: 1,
                audio_channels: 2, audio_sample_rate: 48000,
                full_video_decode: true, full_audio_decode: true, measured_audio_signal: true,
                visual_review: Some(review),
            }] };
        let receipt_bytes = serde_json::to_vec(&receipt).unwrap();
        fs::write(&source_receipt, &receipt_bytes).unwrap();
        let receipt_hash = format!("{:x}", Sha256::digest(&receipt_bytes));
        let id = format!("demostrike-{}-e3-r{}", &demo_hash[..16], &receipt_hash[..16]);
        let package = old.join("package");
        fs::create_dir(&package).unwrap();
        let mut clip = clip(0);
        clip.camera_role = "projectile".into();
        clip.sha256 = receipt.takes[0].muxed_sha256.clone();
        clip.demo_sha256 = demo_hash;
        clip.event_id = "3".into();
        clip.start_tick = 1000;
        clip.end_tick = 1100;
        let manifest = clipstrike_core::capture_package::CaptureManifest {
            schema_version: clipstrike_core::capture_package::CAPTURE_SCHEMA_VERSION,
            package_id: id.clone(), media_validation: clipstrike_core::capture_package::MediaValidation::Structural,
            clips: vec![clip],
        };
        fs::write(package.join("manifest.json"), serde_json::to_vec(&manifest).unwrap()).unwrap();
        let binding = super::ReviewBinding { version: 2, package_id: id.clone(),
            request_path: source_request.clone(), request_sha256: request_hash,
            receipt_path: source_receipt.clone(), receipt_sha256: receipt_hash,
            request_file: Some("demostrike-review-request.json".into()),
            receipt_file: Some("demostrike-review-receipt.json".into()) };
        super::write_portable_review_evidence_before_commit(&package, &binding).unwrap();
        let moved = dir.path().join("relocated-package");
        fs::rename(&package, &moved).unwrap();
        fs::remove_file(source_request).unwrap();
        fs::remove_file(source_receipt).unwrap();
        super::verify_review_binding_with_cancel(&moved, &id, || false).unwrap();
        fs::write(moved.join("demostrike-review-receipt.json"), b"changed").unwrap();
        assert!(super::verify_review_binding_with_cancel(&moved, &id, || false)
            .unwrap_err().to_string().contains("hash differs"));
    }

    fn clip(order: u32) -> CaptureClip {
        CaptureClip {
            clip_id: format!("clip-{order}"),
            relative_path: String::new(),
            sha256: String::new(),
            demo_sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
            demo_path_hint: None,
            event_id: format!("event-{order}"),
            player_steamid64: "76561198000000001".into(),
            map: "de_mirage".into(),
            round: 4,
            utility_type: "smoke".into(),
            start_tick: 1000 + i64::from(order) * 100,
            end_tick: 1100 + i64::from(order) * 100,
            camera_role: "projectile".into(),
            duration_ms: 1000,
            width: 1920,
            height: 1080,
            fps_num: 60,
            fps_den: 1,
            audio: AudioEvidence {
                source: AudioSource::HlaeWav,
                channels: 2,
                sample_rate: 48000,
                verified: true,
            },
            qc_status: QcStatus::Accepted,
            group_order: order,
        }
    }

    #[test]
    fn finalizes_three_separate_clips_without_changing_sources() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut inputs = Vec::new();
        for order in 1..=3 {
            let source = dir.path().join(format!("source-{order}.mp4"));
            fs::write(&source, b"abc").unwrap();
            inputs.push(CaptureInput {
                source_path: source,
                clip: clip(order),
            });
        }
        let destination = dir.path().join("pilot-package");
        let package = finalize_capture_package(&destination, "pilot-001", &inputs).unwrap();
        assert_eq!(package.manifest.clips.len(), 3);
        assert!(destination.join("manifest.json").is_file());
        assert_eq!(
            package.manifest.clips[0].sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_ne!(
            package.manifest.clips[0].relative_path,
            package.manifest.clips[1].relative_path
        );
        assert!(
            inputs
                .iter()
                .all(|input| fs::read(&input.source_path).unwrap() == b"abc")
        );
    }

    #[test]
    fn failed_review_guard_blocks_atomic_package_commit() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.mp4");
        fs::write(&source, b"abc").unwrap();
        let destination = dir.path().join("package");
        let input = CaptureInput { source_path: source, clip: clip(1) };
        let error = super::finalize_capture_package_with_cancel_and_level(
            &destination, "pilot-guard", &[input], || false,
            clipstrike_core::capture_package::MediaValidation::Structural,
            || Err(PackageError::Invalid("receipt changed before commit".into()))
        ).unwrap_err();
        assert!(error.to_string().contains("receipt changed before commit"));
        assert!(!destination.exists());
        assert!(staged_package_path(&destination).join("manifest.json").is_file());
    }

    #[test]
    fn rejects_silent_clip_before_creating_package_folder() {
        let dir = tempfile::TempDir::new().unwrap();
        let source = dir.path().join("source.mp4");
        fs::write(&source, b"abc").unwrap();
        let mut metadata = clip(1);
        metadata.audio.source = AudioSource::GeneratedSilence;
        let destination = dir.path().join("rejected-package");
        let result = finalize_capture_package(
            &destination,
            "pilot-001",
            &[CaptureInput {
                source_path: source,
                clip: metadata,
            }],
        );
        assert!(result.is_err());
        assert!(!destination.exists());
    }

    #[test]
    fn cancellation_leaves_resumable_stage_and_no_finished_folder() {
        let dir = tempfile::TempDir::new().unwrap();
        let inputs: Vec<_> = (1..=3)
            .map(|order| {
                let source_path = dir.path().join(format!("source-{order}.mp4"));
                fs::write(&source_path, format!("fixture-{order}")).unwrap();
                CaptureInput {
                    source_path,
                    clip: clip(order),
                }
            })
            .collect();
        let destination = dir.path().join("pilot-package");
        let stage = staged_package_path(&destination);
        let result = finalize_capture_package_with_cancel(&destination, "pilot-001", &inputs, || {
            stage.join("clips/clip-1.mp4").is_file()
        });
        assert!(matches!(result, Err(PackageError::Cancelled(_))));
        assert!(!destination.exists());
        assert!(stage.join("clips/clip-1.mp4").is_file());
        assert!(!stage.join("manifest.json").exists());
        let resumed = finalize_capture_package(&destination, "pilot-001", &inputs).unwrap();
        assert_eq!(resumed.manifest.clips.len(), 3);
        assert!(!stage.exists());
        assert!(inputs.iter().all(|input| input.source_path.is_file()));
    }

    #[test]
    fn cancellation_during_large_copy_keeps_partial_and_resumes_without_touching_source() {
        let dir = tempfile::TempDir::new().unwrap();
        let source_path = dir.path().join("large.mp4");
        let original = vec![42_u8; 1024 * 1024];
        fs::write(&source_path, &original).unwrap();
        let destination = dir.path().join("pilot-package");
        let stage = staged_package_path(&destination);
        let input = [CaptureInput { source_path: source_path.clone(), clip: clip(1) }];
        let result = finalize_capture_package_with_cancel(&destination, "pilot-001", &input, || {
            stage.join("clips/clip-1.mp4.partial").metadata()
                .is_ok_and(|metadata| metadata.len() >= 256 * 1024)
        });
        assert!(matches!(result, Err(PackageError::Cancelled(_))));
        assert!(!destination.exists());
        assert!(stage.join("clips/clip-1.mp4.partial").is_file());
        assert!(!stage.join("manifest.json").exists());
        assert_eq!(fs::read(&source_path).unwrap(), original);
        let resumed = finalize_capture_package(&destination, "pilot-001", &input).unwrap();
        assert_eq!(resumed.manifest.clips.len(), 1);
        assert!(!stage.exists());
        assert_eq!(fs::read(destination.join("clips/clip-1.mp4")).unwrap(), original);
    }

    #[test]
    fn completed_stage_cannot_silently_recover_for_changed_draft() {
        let dir = tempfile::TempDir::new().unwrap();
        let source_path = dir.path().join("source.mp4");
        fs::write(&source_path, b"abc").unwrap();
        let destination = dir.path().join("pilot-package");
        let input = [CaptureInput { source_path: source_path.clone(), clip: clip(1) }];
        finalize_capture_package(&destination, "pilot-001", &input).unwrap();
        let stage = staged_package_path(&destination);
        fs::rename(&destination, &stage).unwrap();
        fs::write(stage.join(".demostrike-incomplete"), "pilot-001").unwrap();
        let mut changed = input.clone();
        changed[0].clip.event_id = "other-event".into();
        let error = finalize_capture_package(&destination, "pilot-001", &changed).unwrap_err();
        assert!(error.to_string().contains("differs from current draft"));
        assert!(!destination.exists());
        assert!(stage.is_dir());
        let recovered = recover_complete_staged_package(&destination).unwrap();
        assert_eq!(recovered.manifest.clips[0].event_id, "event-1");
    }

    #[test]
    fn recovers_valid_completed_stage_and_rejects_unmarked_folder() {
        let dir = tempfile::TempDir::new().unwrap();
        let source_path = dir.path().join("source-1.mp4");
        fs::write(&source_path, b"abc").unwrap();
        let inputs = [CaptureInput {
            source_path,
            clip: clip(1),
        }];
        let destination = dir.path().join("pilot-package");
        finalize_capture_package(&destination, "pilot-001", &inputs).unwrap();
        let stage = staged_package_path(&destination);
        fs::rename(&destination, &stage).unwrap();
        assert!(matches!(
            recover_complete_staged_package(&destination),
            Err(PackageError::Invalid(_))
        ));
        assert!(stage.is_dir());
        fs::write(stage.join(".demostrike-incomplete"), "pilot-001").unwrap();
        let recovered = recover_complete_staged_package(&destination).unwrap();
        assert_eq!(recovered.manifest.package_id, "pilot-001");
        assert!(!stage.exists());
    }

    #[test]
    fn missing_source_fails_before_creating_stage() {
        let dir = tempfile::TempDir::new().unwrap();
        let destination = dir.path().join("pilot-package");
        let inputs = [CaptureInput {
            source_path: dir.path().join("missing.mp4"),
            clip: clip(1),
        }];
        assert!(finalize_capture_package(&destination, "pilot-001", &inputs).is_err());
        assert!(!destination.exists());
        assert!(!staged_package_path(&destination).exists());
    }
}
