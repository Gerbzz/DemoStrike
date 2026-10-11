use crate::media_probe::probe_muxed_editor_coverage_with_cancel;
use clipstrike_core::capture_package::{CapturePackage, PackageError};
use clipstrike_core::project::Project;
use std::collections::HashSet;
use std::io::Write;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureImportClipCoverage {
    pub asset_id: usize,
    pub clip_id: String,
    pub declared_duration_ms: u64,
    pub video_duration_ms: u64,
    pub audio_duration_ms: u64,
    pub editor_duration_ms: u64,
}

impl CaptureImportClipCoverage {
    pub fn audio_only_tail_ms(&self) -> u64 {
        self.audio_duration_ms
            .saturating_sub(self.video_duration_ms)
    }
}

pub struct ImportedCaptureProject {
    pub project: Project,
    pub coverage: Vec<CaptureImportClipCoverage>,
}

/// Create a new project without changing the accepted capture manifest or
/// existing projects. Editor asset and timeline durations describe the usable
/// visual A/V range; capture_sources retains the original source duration.
pub fn import_package_to_new_project(
    package_dir: &Path,
    output: &Path,
) -> Result<ImportedCaptureProject, PackageError> {
    import_package_to_new_project_with_cancel(package_dir, output, || false)
}

pub fn import_package_to_new_project_with_cancel(
    package_dir: &Path,
    output: &Path,
    should_cancel: impl Fn() -> bool,
) -> Result<ImportedCaptureProject, PackageError> {
    check_cancel(&should_cancel)?;
    if output.exists() {
        return Err(PackageError::Io(format!(
            "project destination already exists: {}",
            output.display()
        )));
    }
    let package = CapturePackage::load_and_validate_with_cancel(package_dir, &should_cancel)?;
    let first = &package.manifest.clips[0];
    if package.manifest.clips.iter().any(|clip| {
        clip.width != first.width
            || clip.height != first.height
            || u64::from(clip.fps_num) * u64::from(first.fps_den)
                != u64::from(first.fps_num) * u64::from(clip.fps_den)
    }) {
        return Err(PackageError::Invalid(
            "capture clips must share resolution and frame rate".into(),
        ));
    }

    let mut project = Project::new(
        &package.manifest.package_id,
        first.width,
        first.height,
        f64::from(first.fps_num) / f64::from(first.fps_den),
    );
    check_cancel(&should_cancel)?;
    project.import_capture_package(&package)?;
    check_cancel(&should_cancel)?;
    let mut coverage = Vec::with_capacity(project.capture_sources.len());
    for source in &project.capture_sources {
        let probed = probe_muxed_editor_coverage_with_cancel(
            &package.media_path(&source.clip),
            &source.clip,
            &should_cancel,
        )?;
        coverage.push(CaptureImportClipCoverage {
            asset_id: source.asset_id,
            clip_id: source.clip.clip_id.clone(),
            declared_duration_ms: source.clip.duration_ms,
            video_duration_ms: probed.video_duration_ms,
            audio_duration_ms: probed.audio_duration_ms,
            editor_duration_ms: probed.visual_duration_ms,
        });
    }
    apply_editor_coverages(&mut project, &coverage)?;
    // Rehash after probing before saving linked media. A changed accepted
    // package cannot become a project with stale probe results.
    check_cancel(&should_cancel)?;
    project.relink_capture_package(output, &package.manifest.package_id, &package.root)?;
    check_cancel(&should_cancel)?;

    let bytes = project
        .to_json_bytes()
        .map_err(|error| PackageError::Json(error.to_string()))?;
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut pending = tempfile::NamedTempFile::new_in(parent)
        .map_err(|error| PackageError::Io(error.to_string()))?;
    pending
        .write_all(&bytes)
        .and_then(|_| pending.as_file().sync_all())
        .map_err(|error| PackageError::Io(error.to_string()))?;
    check_cancel(&should_cancel)?;
    pending
        .persist_noclobber(output)
        .map_err(|error| PackageError::Io(format!("{}: {}", output.display(), error.error)))?;
    Ok(ImportedCaptureProject { project, coverage })
}

fn check_cancel(should_cancel: &impl Fn() -> bool) -> Result<(), PackageError> {
    if should_cancel() {
        Err(PackageError::Cancelled(
            "capture package import cancelled".into(),
        ))
    } else {
        Ok(())
    }
}

fn apply_editor_coverages(
    project: &mut Project,
    coverage: &[CaptureImportClipCoverage],
) -> Result<(), PackageError> {
    if coverage.len() != project.clips.len()
        || coverage.len() != project.assets.len()
        || coverage.len() != project.capture_sources.len()
    {
        return Err(PackageError::Invalid(
            "capture project and probed stream coverage count differ".into(),
        ));
    }
    let mut seen = HashSet::new();
    let mut changes = Vec::with_capacity(coverage.len());
    let mut start = 0_i64;
    for measured in coverage {
        if !seen.insert(measured.asset_id)
            || measured.editor_duration_ms == 0
            || measured.editor_duration_ms != measured.video_duration_ms
            || measured.video_duration_ms > measured.audio_duration_ms
            || measured.video_duration_ms > measured.declared_duration_ms
        {
            return Err(PackageError::Invalid(format!(
                "invalid editor coverage for {}",
                measured.clip_id
            )));
        }
        let clip_index = project
            .clips
            .iter()
            .position(|clip| clip.asset_id == measured.asset_id)
            .ok_or_else(|| {
                PackageError::Invalid(format!("capture clip missing for {}", measured.clip_id))
            })?;
        let asset_index = project
            .assets
            .iter()
            .position(|asset| asset.id == measured.asset_id)
            .ok_or_else(|| {
                PackageError::Invalid(format!("capture asset missing for {}", measured.clip_id))
            })?;
        if !project.capture_sources.iter().any(|source| {
            source.asset_id == measured.asset_id
                && source.clip.clip_id == measured.clip_id
                && source.clip.duration_ms == measured.declared_duration_ms
        }) {
            return Err(PackageError::Invalid(format!(
                "capture manifest facts differ from probe: {}",
                measured.clip_id
            )));
        }
        let length = i64::try_from(measured.editor_duration_ms)
            .map_err(|_| PackageError::Invalid("editor duration exceeds timeline range".into()))?;
        let end = start
            .checked_add(length)
            .ok_or_else(|| PackageError::Invalid("editor timeline duration overflow".into()))?;
        changes.push((clip_index, asset_index, start, end, length));
        start = end;
    }
    for (clip_index, asset_index, start, end, length) in changes {
        let clip = &mut project.clips[clip_index];
        clip.start = start;
        clip.end = end;
        clip.in_point = 0;
        clip.out_point = length;
        // MediaAsset duration is the supported editor trim limit; the original
        // full container duration remains in capture_sources and the package.
        project.assets[asset_index].duration_ms = length as u64;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    // Synthetic metadata exercises the import boundary without a match or media fixture.
    use super::{
        CaptureImportClipCoverage, apply_editor_coverages,
        import_package_to_new_project_with_cancel,
    };
    use clipstrike_core::capture_package::{CapturedAsset, PackageError};
    use clipstrike_core::project::{MediaAsset, Project};
    use clipstrike_core::timeline::Clip;
    use serde_json::json;

    fn source(asset_id: usize, duration_ms: u64) -> CapturedAsset {
        let clip = serde_json::from_value(json!({
            "clip_id": format!("clip-{asset_id}"),
            "relative_path": format!("clips/clip-{asset_id}.mp4"),
            "sha256": "a".repeat(64),
            "demo_sha256": "b".repeat(64),
            "event_id": "fixture-event",
            "player_steamid64": "76561198000000011",
            "map": "de_fixture",
            "round": 1,
            "utility_type": "he",
            "start_tick": 100,
            "end_tick": 200,
            "camera_role": "projectile",
            "duration_ms": duration_ms,
            "width": 1920,
            "height": 1080,
            "fps_num": 60,
            "fps_den": 1,
            "audio": {
                "source": "hlae_wav", "channels": 2,
                "sample_rate": 44100, "verified": true
            },
            "qc_status": "accepted",
            "group_order": asset_id - 1
        }))
        .unwrap();
        CapturedAsset {
            asset_id,
            package_id: "synthetic-package".into(),
            package_path: Some("accepted-package".into()),
            media_validation: Default::default(),
            clip,
        }
    }

    fn project_with_two_declared_clips() -> Project {
        let mut project = Project::new("synthetic-package", 1920, 1080, 60.0);
        for (id, declared, start) in [(1, 2740_u64, 0_i64), (2, 4028, 2740)] {
            project.assets.push(MediaAsset {
                id,
                name: format!("clip-{id}"),
                uri: format!("file:///clip-{id}.mp4"),
                duration_ms: declared,
                audio_levels: Vec::new(),
            });
            project.clips.push(Clip::new(
                id,
                id,
                1,
                start,
                start + declared as i64,
                0,
                declared as i64,
            ));
            project.capture_sources.push(source(id, declared));
        }
        project
    }

    #[test]
    fn audio_only_tail_is_omitted_without_changing_capture_facts() {
        let mut project = project_with_two_declared_clips();
        let originals = project.capture_sources.clone();
        let coverage = [
            CaptureImportClipCoverage {
                asset_id: 1,
                clip_id: "clip-1".into(),
                declared_duration_ms: 2740,
                video_duration_ms: 2700,
                audio_duration_ms: 2740,
                editor_duration_ms: 2700,
            },
            CaptureImportClipCoverage {
                asset_id: 2,
                clip_id: "clip-2".into(),
                declared_duration_ms: 4028,
                video_duration_ms: 4016,
                audio_duration_ms: 4028,
                editor_duration_ms: 4016,
            },
        ];
        apply_editor_coverages(&mut project, &coverage).unwrap();
        assert_eq!(project.capture_sources, originals);
        assert_eq!(project.assets[0].duration_ms, 2700);
        assert_eq!(project.assets[1].duration_ms, 4016);
        assert_eq!(
            project
                .clips
                .iter()
                .map(|clip| (clip.start, clip.end, clip.in_point, clip.out_point))
                .collect::<Vec<_>>(),
            vec![(0, 2700, 0, 2700), (2700, 6716, 0, 4016)]
        );
        assert_eq!(coverage[0].audio_only_tail_ms(), 40);
    }

    #[test]
    fn invalid_second_coverage_leaves_project_untouched() {
        let mut project = project_with_two_declared_clips();
        let before = project.to_json_bytes().unwrap();
        let coverage = [
            CaptureImportClipCoverage {
                asset_id: 1,
                clip_id: "clip-1".into(),
                declared_duration_ms: 2740,
                video_duration_ms: 2700,
                audio_duration_ms: 2740,
                editor_duration_ms: 2700,
            },
            CaptureImportClipCoverage {
                asset_id: 2,
                clip_id: "clip-2".into(),
                declared_duration_ms: 4028,
                video_duration_ms: 4016,
                audio_duration_ms: 4028,
                editor_duration_ms: 4028,
            },
        ];
        assert!(apply_editor_coverages(&mut project, &coverage).is_err());
        assert_eq!(project.to_json_bytes().unwrap(), before);
    }

    #[test]
    fn shorter_audio_does_not_shorten_a_valid_picture() {
        let mut project = project_with_two_declared_clips();
        let before = project.to_json_bytes().unwrap();
        let coverage = [
            CaptureImportClipCoverage {
                asset_id: 1,
                clip_id: "clip-1".into(),
                declared_duration_ms: 2740,
                video_duration_ms: 2700,
                audio_duration_ms: 2680,
                editor_duration_ms: 2680,
            },
            CaptureImportClipCoverage {
                asset_id: 2,
                clip_id: "clip-2".into(),
                declared_duration_ms: 4028,
                video_duration_ms: 4016,
                audio_duration_ms: 4028,
                editor_duration_ms: 4016,
            },
        ];
        assert!(apply_editor_coverages(&mut project, &coverage).is_err());
        assert_eq!(project.to_json_bytes().unwrap(), before);
    }

    #[test]
    fn pre_cancelled_import_does_not_write_a_project() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("not-created.cstrike");
        let result = import_package_to_new_project_with_cancel(root.path(), &destination, || true);
        assert!(matches!(result, Err(PackageError::Cancelled(_))));
        assert!(!destination.exists());
    }
}
