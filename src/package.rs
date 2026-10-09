use crate::media_probe::probe_muxed_clip_with_cancel;
use clipstrike_core::capture_package::{
    CAPTURE_SCHEMA_VERSION, CaptureClip, CaptureManifest, CapturePackage, MediaValidation,
    PackageError, sha256_file_with_cancel, validate_clip_fields,
};
use serde::Deserialize;
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
    for input in inputs {
        probe_muxed_clip_with_cancel(&input.source_path, &input.clip, &should_cancel)?;
    }
    finalize_capture_package_with_cancel_and_level(
        destination,
        package_id,
        inputs,
        should_cancel,
        MediaValidation::StreamProbed,
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
    )
}

fn finalize_capture_package_with_cancel_and_level(
    destination: &Path,
    package_id: &str,
    inputs: &[CaptureInput],
    should_cancel: impl Fn() -> bool,
    media_validation: MediaValidation,
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
            return recover_complete_staged_package_with_cancel(destination, should_cancel);
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
    recover_complete_staged_package_with_cancel(destination, should_cancel)
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
