mod app;
mod bundle;
mod digest;
mod download;
mod http;
mod install;
mod release;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("JetSheets · Instalador")
            .with_inner_size([760.0, 660.0])
            .with_min_inner_size([620.0, 540.0]),
        ..Default::default()
    };
    eframe::run_native(
        "JetSheets Installer",
        options,
        Box::new(|context| Ok(Box::new(app::InstallerApp::new(&context.egui_ctx)))),
    )
}
