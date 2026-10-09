use demostrike::ui::DemoStrikeApp;

fn main() -> eframe::Result<()> {
    eframe::run_native(
        "DemoStrike — utility analysis",
        eframe::NativeOptions::default(),
        Box::new(|_context| Ok(Box::new(DemoStrikeApp::default()))),
    )
}
