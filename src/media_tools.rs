//! Scoped, per-worker media executables. The capture UI supplies its selected
//! pair once; nested receipt and package probes run on that same worker thread.
use std::cell::RefCell;
use std::path::{Path, PathBuf};

#[derive(Clone)]
struct SelectedTools { ffmpeg: PathBuf, ffprobe: PathBuf }

thread_local! { static SELECTED: RefCell<Option<SelectedTools>> = const { RefCell::new(None) }; }

pub fn with_selected_media_tools<T>(ffmpeg: &Path, ffprobe: &Path,
    operation: impl FnOnce() -> T) -> Result<T, String> {
    if !ffmpeg.is_absolute() || !ffprobe.is_absolute()
        || !ffmpeg.is_file() || !ffprobe.is_file() {
        return Err("select existing absolute FFmpeg and FFprobe executables".into());
    }
    struct Restore(Option<SelectedTools>);
    impl Drop for Restore {
        fn drop(&mut self) {
            SELECTED.with(|cell| *cell.borrow_mut() = self.0.take());
        }
    }
    let prior = SELECTED.with(|cell| cell.replace(Some(SelectedTools {
        ffmpeg: ffmpeg.to_path_buf(), ffprobe: ffprobe.to_path_buf(),
    })));
    let _restore = Restore(prior);
    Ok(operation())
}

pub fn ffmpeg_program() -> PathBuf {
    SELECTED.with(|cell| cell.borrow().as_ref().map(|tools| tools.ffmpeg.clone()))
        .unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

pub fn ffprobe_program() -> PathBuf {
    SELECTED.with(|cell| cell.borrow().as_ref().map(|tools| tools.ffprobe.clone()))
        .unwrap_or_else(|| PathBuf::from("ffprobe"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_paths_survive_nested_probe_calls_and_restore() {
        let exe = std::env::current_exe().unwrap();
        let before = ffprobe_program();
        with_selected_media_tools(&exe, &exe, || {
            assert_eq!(ffprobe_program(), exe);
            assert_eq!(ffmpeg_program(), exe);
            with_selected_media_tools(&exe, &exe, || assert_eq!(ffprobe_program(), exe)).unwrap();
            assert_eq!(ffmpeg_program(), exe);
        }).unwrap();
        assert_eq!(ffprobe_program(), before);
    }
}
