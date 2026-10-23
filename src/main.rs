use demostrike::ui::DemoStrikeApp;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn parse_args(args: &[OsString]) -> Result<(Option<PathBuf>, Option<PathBuf>), &'static str> {
    if args.len() % 2 != 0 || args.len() > 4 {
        return Err(
            "usage: demostrike [--project saved-project.cstrike|saved-project.fcproj] [--capture ABSOLUTE.png]",
        );
    }
    let (mut project, mut capture) = (None, None);
    for pair in args.chunks_exact(2) {
        let path = PathBuf::from(&pair[1]);
        if pair[0] == "--project" && project.is_none() {
            project = Some(path);
        } else if pair[0] == "--capture" && capture.is_none() {
            if !path.is_absolute()
                || !path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("png"))
            {
                return Err("Capture output must be an absolute .png path");
            }
            capture = Some(path);
        } else {
            return Err(
                "usage: demostrike [--project saved-project.cstrike|saved-project.fcproj] [--capture ABSOLUTE.png]",
            );
        }
    }
    Ok((project, capture))
}

#[derive(Debug)]
struct NativeReviewArgs {
    library: PathBuf,
    match_id: Option<i64>,
    round_number: Option<i64>,
    radar: Option<PathBuf>,
    capture: Option<PathBuf>,
    capture_receipt: Option<PathBuf>,
}

fn parse_review_args(args: &[OsString]) -> Result<NativeReviewArgs, &'static str> {
    if args.is_empty() || args.len() % 2 != 0 || args.len() > 12 {
        return Err(
            "usage: demostrike --review-library ABSOLUTE.sqlite [--review-match ID] [--review-round NUMBER] [--review-radar ABSOLUTE.png] [--capture ABSOLUTE.png --review-capture-receipt ABSOLUTE.json]",
        );
    }
    let (mut library, mut match_id, mut round_number, mut radar, mut capture, mut capture_receipt) =
        (None, None, None, None, None, None);
    for pair in args.chunks_exact(2) {
        let path = PathBuf::from(&pair[1]);
        if pair[0] == "--review-library" && library.is_none() && path.is_absolute() {
            library = Some(path);
        } else if pair[0] == "--review-radar"
            && radar.is_none()
            && path.is_absolute()
            && path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("png"))
        {
            radar = Some(path);
        } else if pair[0] == "--capture"
            && capture.is_none()
            && path.is_absolute()
            && path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("png"))
        {
            capture = Some(path);
        } else if pair[0] == "--review-capture-receipt"
            && capture_receipt.is_none()
            && path.is_absolute()
            && path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("json"))
        {
            capture_receipt = Some(path);
        } else if pair[0] == "--review-match" && match_id.is_none() {
            let value = pair[1]
                .to_str()
                .and_then(|v| v.parse::<i64>().ok())
                .filter(|v| *v > 0)
                .ok_or("review match must be a positive saved match ID")?;
            match_id = Some(value);
        } else if pair[0] == "--review-round" && round_number.is_none() {
            let value = pair[1]
                .to_str()
                .and_then(|v| v.parse::<i64>().ok())
                .filter(|v| *v > 0)
                .ok_or("review round must be a positive recorded round number")?;
            round_number = Some(value);
        } else {
            return Err("unsupported, duplicate or relative native review argument");
        }
    }
    if capture_receipt.is_some() && capture.is_none() {
        return Err("native review capture receipt requires --capture");
    }
    Ok(NativeReviewArgs {
        library: library.ok_or("native review requires --review-library")?,
        match_id,
        round_number,
        radar,
        capture,
        capture_receipt,
    })
}

fn main() -> eframe::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let review = if args.iter().any(|arg| arg == "--review-library") {
        match parse_review_args(&args) {
            Ok(options) => Some(options),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(2);
            }
        }
    } else {
        None
    };
    let parsed = review
        .as_ref()
        .map(|options| (None, options.capture.clone()))
        .map(Ok)
        .unwrap_or_else(|| parse_args(&args));
    let (project, capture) = match parsed {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    let capturing = capture.is_some();
    let capture_status = Arc::new(Mutex::new(None));
    let capture_status_ui = Arc::clone(&capture_status);
    if capturing {
        eprintln!("EDITOR_BOOT before_run_native");
    }
    eframe::run_native(
        "DemoStrike — utility analysis",
        eframe::NativeOptions {
            renderer: eframe::Renderer::Wgpu,
            viewport: {
                let viewport = if capturing {
                    eframe::egui::ViewportBuilder::default().with_inner_size([1440.0, 900.0])
                } else {
                    eframe::egui::ViewportBuilder::default()
                };
                match eframe::icon_data::from_png_bytes(include_bytes!(
                    "../assets/branding/DemoStrike-geometric.png"
                )) {
                    Ok(icon) => viewport.with_icon(icon),
                    Err(_) => viewport,
                }
            },
            ..Default::default()
        },
        Box::new(move |context| {
            if capturing {
                eprintln!("EDITOR_BOOT creation_callback");
            }
            let mut app = match &project {
                Some(path) => DemoStrikeApp::with_saved_project(context, path)
                    .map_err(std::io::Error::other)?,
                None => {
                    DemoStrikeApp::with_native_context(context).map_err(std::io::Error::other)?
                }
            };
            if let Some(options) = &review {
                app.open_native_review(
                    &context.egui_ctx,
                    &options.library,
                    options.match_id,
                    options.round_number,
                    options.radar.as_deref(),
                )
                .map_err(std::io::Error::other)?;
                if let Some(path) = &options.capture_receipt {
                    app.set_native_review_capture_receipt(path.clone())
                        .map_err(std::io::Error::other)?;
                }
            }
            if capturing {
                eprintln!("EDITOR_BOOT project_loaded");
            }
            if let Some(path) = capture.clone() {
                app.capture_once(path, Arc::clone(&capture_status_ui));
                eprintln!("EDITOR_BOOT capture_armed");
            }
            Ok(Box::new(app))
        }),
    )?;
    if capturing {
        match capture_status.lock().unwrap().take() {
            Some(Ok(())) => {}
            Some(Err(error)) => {
                eprintln!("DemoStrike capture: {error}");
                std::process::exit(1);
            }
            None => {
                eprintln!("DemoStrike capture: window closed before screenshot completed");
                std::process::exit(1);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_args;
    use std::ffi::OsString;

    #[test]
    fn capture_requires_absolute_png_and_does_not_select_a_project() {
        let path = std::env::temp_dir().join("demostrike-capture.png");
        let args = [OsString::from("--capture"), path.as_os_str().to_os_string()];
        let (project, capture) = parse_args(&args).unwrap();
        assert!(project.is_none());
        assert_eq!(capture, Some(path.clone()));
        assert!(
            parse_args(&[OsString::from("--capture"), OsString::from("relative.png")]).is_err()
        );
        assert!(
            parse_args(&[
                OsString::from("--capture"),
                OsString::from("C:\\not-a-png.txt")
            ])
            .is_err()
        );
        let combined = [
            OsString::from("--project"),
            OsString::from("offline-edit.fcproj"),
            OsString::from("--capture"),
            path.as_os_str().to_os_string(),
        ];
        let (project, capture) = parse_args(&combined).unwrap();
        assert_eq!(
            project,
            Some(std::path::PathBuf::from("offline-edit.fcproj"))
        );
        assert_eq!(capture, Some(path));
        assert!(
            parse_args(&[
                OsString::from("--project"),
                OsString::from("a.cstrike"),
                OsString::from("--project"),
                OsString::from("b.fcproj"),
            ])
            .is_err()
        );
    }
    #[test]
    fn native_review_args_select_a_rust_library_without_an_editor_or_webview() {
        let library = std::env::temp_dir().join("native review 你好.sqlite");
        let args = vec![
            OsString::from("--review-library"),
            library.clone().into_os_string(),
            OsString::from("--review-match"),
            OsString::from("7"),
            OsString::from("--review-round"),
            OsString::from("3"),
        ];
        let review = super::parse_review_args(&args).unwrap();
        assert_eq!(review.library, library);
        assert_eq!(review.match_id, Some(7));
        assert_eq!(review.round_number, Some(3));
        assert!(review.capture.is_none());
        assert!(
            super::parse_review_args(&[
                OsString::from("--review-library"),
                OsString::from("relative.sqlite")
            ])
            .is_err()
        );
        let mut mixed = args.clone();
        mixed.extend([
            OsString::from("--project"),
            OsString::from("arbitrary.fcproj"),
        ]);
        assert!(super::parse_review_args(&mixed).is_err());
        let mut duplicate = args.clone();
        duplicate.extend([OsString::from("--review-round"), OsString::from("4")]);
        assert!(super::parse_review_args(&duplicate).is_err());
    }
    #[test]
    fn review_capture_receipt_requires_a_real_capture_and_rejects_duplicate_or_relative_paths() {
        let library = std::env::temp_dir().join("native.sqlite");
        let receipt = std::env::temp_dir().join("native-proof.json");
        let capture = std::env::temp_dir().join("native.png");
        let radar = std::env::temp_dir().join("radar.png");
        let mut args = vec![
            OsString::from("--review-library"),
            library.into_os_string(),
            OsString::from("--review-capture-receipt"),
            receipt.clone().into_os_string(),
        ];
        assert!(super::parse_review_args(&args).is_err());
        args.extend([
            OsString::from("--capture"),
            capture.clone().into_os_string(),
            OsString::from("--review-radar"),
            radar.clone().into_os_string(),
        ]);
        let parsed = super::parse_review_args(&args).unwrap();
        assert_eq!(parsed.capture, Some(capture));
        assert_eq!(parsed.capture_receipt, Some(receipt.clone()));
        assert_eq!(parsed.radar, Some(radar));
        args.extend([
            OsString::from("--review-capture-receipt"),
            receipt.into_os_string(),
        ]);
        assert!(super::parse_review_args(&args).is_err());
        args.truncate(8);
        args[3] = OsString::from("relative.json");
        assert!(super::parse_review_args(&args).is_err());
    }
}
