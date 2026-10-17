mod app;
mod chart_view;
mod grid_selection;
mod theme;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("JetSheets")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([940.0, 620.0]),
        ..Default::default()
    };
    eframe::run_native(
        "JetSheets",
        options,
        Box::new(|context| Ok(Box::new(app::JetSheetsApp::new(&context.egui_ctx)))),
    )
}
