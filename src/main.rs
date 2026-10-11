use demostrike::ui::DemoStrikeApp;

fn main() -> eframe::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let project = match args.as_slice() {
        [] => None,
        [flag, path] if flag == "--project" => Some(std::path::PathBuf::from(path)),
        _ => {
            eprintln!("usage: demostrike [--project saved-project.cstrike]");
            std::process::exit(2);
        }
    };
    eframe::run_native(
        "DemoStrike — utility analysis",
        eframe::NativeOptions {
            renderer: eframe::Renderer::Wgpu,
            ..Default::default()
        },
        Box::new(move |context| {
            let app = match &project {
                Some(path) => DemoStrikeApp::with_saved_project(context, path)
                    .map_err(std::io::Error::other)?,
                None => DemoStrikeApp::with_native_context(context)
                    .map_err(std::io::Error::other)?,
            };
            Ok(Box::new(app))
        }),
    )
}
