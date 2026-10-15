use demostrike::ui::DemoStrikeApp;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn parse_args(args: &[OsString]) -> Result<(Option<PathBuf>, Option<PathBuf>), &'static str> {
    match args {
        [] => Ok((None, None)),
        [flag, path] if flag == "--project" => Ok((Some(PathBuf::from(path)), None)),
        [flag, path] if flag == "--capture" => {
            let path = PathBuf::from(path);
            if !path.is_absolute() || !path.extension().and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("png")) {
                return Err("Capture output must be an absolute .png path");
            }
            Ok((None, Some(path)))
        }
        _ => Err("usage: demostrike [--project saved-project.cstrike|saved-project.fcproj] [--capture ABSOLUTE.png]"),
    }
}

fn main() -> eframe::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let (project, capture) = match parse_args(&args) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    let capturing = capture.is_some();
    let capture_status = Arc::new(Mutex::new(None));
    let capture_status_ui = Arc::clone(&capture_status);
    eframe::run_native(
        "DemoStrike — utility analysis",
        eframe::NativeOptions {
            renderer: eframe::Renderer::Wgpu,
            viewport: if capturing {
                eframe::egui::ViewportBuilder::default().with_inner_size([1440.0, 900.0])
            } else {
                Default::default()
            },
            ..Default::default()
        },
        Box::new(move |context| {
            let mut app = match &project {
                Some(path) => DemoStrikeApp::with_saved_project(context, path)
                    .map_err(std::io::Error::other)?,
                None => {
                    DemoStrikeApp::with_native_context(context).map_err(std::io::Error::other)?
                }
            };
            if let Some(path) = capture.clone() {
                app.capture_once(path, Arc::clone(&capture_status_ui));
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
        assert_eq!(capture, Some(path));
        assert!(
            parse_args(&[OsString::from("--capture"), OsString::from("relative.png")]).is_err()
        );
        assert!(parse_args(&[
            OsString::from("--capture"),
            OsString::from("C:\\not-a-png.txt")
        ])
        .is_err());
    }
}
