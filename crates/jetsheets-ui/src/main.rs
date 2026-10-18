// Cargo release disables debug assertions; development builds keep the console.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod chart_view;
mod grid_selection;
mod theme;

const LOG_FILE: &str = "jetsheets.log";

fn report_failure(message: &str) {
    eprintln!("{message}");
    let details = match jetsheets_diagnostics::record(LOG_FILE, message) {
        Ok(path) => format!("{message}\n\nDetalhes em: {}", path.display()),
        Err(error) => format!("{message}\n\nNão foi possível gravar o log: {error}"),
    };
    let _ = rfd::MessageDialog::new()
        .set_title("Erro ao iniciar JetSheets")
        .set_description(&details)
        .set_level(rfd::MessageLevel::Error)
        .show();
}

fn main() {
    jetsheets_diagnostics::install_panic_hook(LOG_FILE);
    match std::panic::catch_unwind(run) {
        Ok(Ok(())) => {}
        Ok(Err(error)) => report_failure(&format!("JetSheets não pôde iniciar: {error}")),
        Err(_) => report_failure("JetSheets encontrou uma falha inesperada ao iniciar."),
    }
}

fn run() -> eframe::Result {
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
