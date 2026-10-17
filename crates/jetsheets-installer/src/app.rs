use std::{
    path::PathBuf,
    process::Command,
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
};

use eframe::egui::{self, Color32, RichText, Stroke};

use crate::{
    bundle,
    download::{self, Control, DownloadResult},
    install::{self, InstallRecord},
    release::{EXECUTABLE_NAME, REPOSITORY, Release, Version},
};

enum Event {
    Release(Result<Release, String>),
    Progress(u64),
    Verifying,
    Completed(Result<(), String>),
    Cancelled,
}

#[derive(PartialEq, Eq)]
enum Phase {
    Checking,
    Ready,
    Downloading,
    Paused,
    Verifying,
    Installed,
    Error,
}

pub struct InstallerApp {
    folder: PathBuf,
    folder_error: Option<String>,
    installed: Option<InstallRecord>,
    release: Option<Release>,
    phase: Phase,
    status: String,
    downloaded: u64,
    control: Option<Arc<Control>>,
    sender: Sender<Event>,
    receiver: Receiver<Event>,
}

impl InstallerApp {
    pub fn new(ctx: &egui::Context) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(0x10, 0x13, 0x18);
        visuals.window_fill = Color32::from_rgb(0x1B, 0x20, 0x29);
        visuals.override_text_color = Some(Color32::from_rgb(0xF1, 0xF5, 0xF9));
        visuals.weak_text_color = Some(Color32::from_rgb(0x92, 0x9C, 0xAA));
        visuals.selection.bg_fill = Color32::from_rgb(0x1D, 0x42, 0x78);
        visuals.selection.stroke = Stroke::new(1.0, Color32::from_rgb(0x3B, 0x82, 0xF6));
        ctx.set_visuals(visuals);
        let folder = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
            .join("JetSheets");
        let (sender, receiver) = mpsc::channel();
        let mut app = Self {
            folder,
            folder_error: None,
            installed: None,
            release: None,
            phase: Phase::Checking,
            status: "Consultando versões publicadas no GitHub...".into(),
            downloaded: 0,
            control: None,
            sender,
            receiver,
        };
        app.refresh_folder();
        app.check_releases(ctx);
        app
    }

    fn refresh_folder(&mut self) {
        match install::installed(&self.folder) {
            Ok(installed) => {
                self.installed = installed;
                self.folder_error = None;
            }
            Err(error) => {
                self.installed = None;
                self.folder_error = Some(error);
            }
        }
    }

    fn check_releases(&mut self, ctx: &egui::Context) {
        self.phase = Phase::Checking;
        self.status = "Consultando a última Release estável...".into();
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = download::latest_release();
            let _ = sender.send(Event::Release(result));
            ctx.request_repaint();
        });
    }

    fn begin_install(&mut self, ctx: &egui::Context) {
        let Some(release) = self.release.clone() else {
            return;
        };
        if self.folder_error.is_some() {
            return;
        }
        self.downloaded = 0;
        self.phase = Phase::Downloading;
        self.status = "Baixando executável verificado do GitHub...".into();
        let control = Arc::new(Control::default());
        self.control = Some(Arc::clone(&control));
        let folder = self.folder.clone();
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = download::download(&release, &folder, &control, |bytes| {
                let _ = sender.send(Event::Progress(bytes));
                ctx.request_repaint();
            });
            match result {
                Ok(DownloadResult::Cancelled) => {
                    let _ = sender.send(Event::Cancelled);
                }
                Ok(DownloadResult::Complete) => {
                    if !control.begin_verifying() {
                        let _ = std::fs::remove_file(install::partial_path(&folder, &release));
                        let _ = sender.send(Event::Cancelled);
                    } else {
                        let _ = sender.send(Event::Verifying);
                        ctx.request_repaint();
                        let part = install::partial_path(&folder, &release);
                        let result = install::install_verified(&folder, &part, &release);
                        if result
                            .as_ref()
                            .is_err_and(|error| error.contains("SHA-256 diferente"))
                        {
                            let _ = std::fs::remove_file(part);
                        }
                        let _ = sender.send(Event::Completed(result));
                    }
                }
                Err(_) if control.is_cancelled() => {
                    let _ = std::fs::remove_file(install::partial_path(&folder, &release));
                    let _ = sender.send(Event::Cancelled);
                }
                Err(error) => {
                    let _ = sender.send(Event::Completed(Err(error)));
                }
            }
            ctx.request_repaint();
        });
    }

    fn begin_bundle_install(&mut self, ctx: &egui::Context) {
        if !bundle::available() || self.folder_error.is_some() {
            return;
        }
        self.phase = Phase::Verifying;
        self.status = "Verificando e instalando a versão incluída...".into();
        let folder = self.folder.clone();
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = sender.send(Event::Completed(bundle::install(&folder)));
            ctx.request_repaint();
        });
    }

    fn poll_events(&mut self) {
        while let Ok(event) = self.receiver.try_recv() {
            match event {
                Event::Release(Ok(release)) => {
                    if self.phase == Phase::Checking {
                        self.status = format!("Versão {} disponível no GitHub.", release.tag);
                        self.phase = Phase::Ready;
                    }
                    self.release = Some(release);
                }
                Event::Release(Err(error)) => {
                    if self.phase == Phase::Checking {
                        self.status = error;
                        self.phase = Phase::Error;
                    }
                }
                Event::Progress(bytes) => self.downloaded = bytes,
                Event::Verifying => {
                    self.phase = Phase::Verifying;
                    self.status = "Conferindo SHA-256 e ativando a instalação...".into();
                }
                Event::Completed(Ok(())) => {
                    self.control = None;
                    self.refresh_folder();
                    self.phase = Phase::Installed;
                    self.status = "JetSheets instalado e verificado com sucesso.".into();
                }
                Event::Completed(Err(error)) => {
                    self.control = None;
                    self.phase = Phase::Error;
                    self.status = error;
                }
                Event::Cancelled => {
                    self.control = None;
                    self.phase = Phase::Ready;
                    self.status = "Download cancelado; arquivo temporário removido.".into();
                    self.downloaded = 0;
                }
            }
        }
    }

    fn can_install(&self) -> bool {
        let Some(release) = &self.release else {
            return false;
        };
        if self.folder_error.is_some()
            || !matches!(self.phase, Phase::Ready | Phase::Installed | Phase::Error)
        {
            return false;
        }
        self.installed.as_ref().is_none_or(|record| {
            Version::parse(&record.version).is_ok_and(|version| {
                version < release.version
                    || (version == release.version && record.sha256 == release.sha256)
            })
        })
    }

    fn can_install_bundle(&self) -> bool {
        bundle::available()
            && self.folder_error.is_none()
            && matches!(
                self.phase,
                Phase::Checking | Phase::Ready | Phase::Installed | Phase::Error
            )
            && self.installed.as_ref().is_none_or(|record| {
                Version::parse(&record.version).is_ok_and(|version| version < bundle::version())
            })
    }

    fn install_label(&self) -> String {
        let Some(release) = &self.release else {
            return "Instalar".into();
        };
        match &self.installed {
            Some(record) if record.version == release.tag => format!("Reinstalar {}", release.tag),
            Some(_) => format!("Atualizar para {}", release.tag),
            None => format!("Instalar {}", release.tag),
        }
    }
}

impl eframe::App for InstallerApp {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_events();
        let ctx = root.ctx().clone();
        egui::CentralPanel::default().show(root, |ui| {
            ui.add_space(12.0);
            ui.label(RichText::new("◆ JetSheets").size(27.0).strong().color(Color32::from_rgb(0x60, 0xA5, 0xFA)));
            ui.label(RichText::new("Instalador e atualizador para Windows").size(13.0).color(Color32::from_rgb(0x92, 0x9C, 0xAA)));
            ui.add_space(16.0);
            egui::Frame::new()
                .fill(Color32::from_rgb(0x1B, 0x20, 0x29))
                .stroke(Stroke::new(1.0, Color32::from_rgb(0x2C, 0x34, 0x42)))
                .corner_radius(8.0)
                .inner_margin(egui::Margin::same(16))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.heading("Instalação");
                    ui.label(format!("Pasta: {}", self.folder.display()));
                    if ui.add_enabled(!matches!(self.phase, Phase::Downloading | Phase::Paused | Phase::Verifying), egui::Button::new("Escolher pasta...")).clicked()
                        && let Some(folder) = rfd::FileDialog::new().set_directory(&self.folder).pick_folder()
                    {
                        self.folder = folder;
                        self.refresh_folder();
                    }
                    if let Some(error) = &self.folder_error {
                        ui.colored_label(Color32::from_rgb(0xF8, 0x71, 0x71), error);
                    }
                    ui.separator();
                    ui.label(match &self.installed {
                        Some(record) => format!("Instalada: {}", record.version),
                        None => "Nenhuma versão instalada nesta pasta.".into(),
                    });
                    if let Some(release) = &self.release {
                        ui.label(format!("Disponível: {} · {:.1} MB", release.tag, release.size as f64 / 1_000_000.0));
                        if let Some(record) = &self.installed
                            && record.version == release.tag && record.sha256 != release.sha256
                        {
                            ui.colored_label(Color32::from_rgb(0xF5, 0x9E, 0x0B), "A mesma versão foi republicada com outro arquivo. Uma nova tag é necessária.");
                        }
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if bundle::available()
                            && ui.add_enabled(self.can_install_bundle(), egui::Button::new(format!("Instalar versão incluída v{}", env!("CARGO_PKG_VERSION")))).clicked()
                        {
                            self.begin_bundle_install(&ctx);
                        }
                        if ui.add_enabled(self.can_install(), egui::Button::new(RichText::new(self.install_label()).strong().color(Color32::WHITE)).fill(Color32::from_rgb(0x25, 0x63, 0xEB))).clicked() {
                            self.begin_install(&ctx);
                        }
                        if ui.add_enabled(!matches!(self.phase, Phase::Checking | Phase::Downloading | Phase::Paused | Phase::Verifying), egui::Button::new("Buscar atualizações")).clicked() {
                            self.check_releases(&ctx);
                        }
                        if ui.add_enabled(self.installed.is_some() && !matches!(self.phase, Phase::Downloading | Phase::Paused | Phase::Verifying), egui::Button::new("Abrir JetSheets")).clicked()
                            && let Err(error) = Command::new(self.folder.join(EXECUTABLE_NAME)).spawn()
                        {
                            self.status = format!("Não foi possível abrir JetSheets: {error}");
                            self.phase = Phase::Error;
                        }
                    });
                    if matches!(self.phase, Phase::Downloading | Phase::Paused | Phase::Verifying)
                        && self.control.is_some()
                        && let Some(release) = &self.release
                    {
                        ui.add(egui::ProgressBar::new((self.downloaded as f32 / release.size as f32).clamp(0.0, 1.0))
                            .show_percentage()
                            .text(format!("{:.1} / {:.1} MB", self.downloaded as f64 / 1_000_000.0, release.size as f64 / 1_000_000.0)));
                        ui.horizontal(|ui| {
                            if self.phase == Phase::Downloading
                                && ui.button("Pausar download").clicked()
                                && self.control.as_ref().is_some_and(|control| control.pause())
                            {
                                self.phase = Phase::Paused;
                                self.status = "Download pausado. A retomada usa HTTP Range.".into();
                            }
                            if self.phase == Phase::Paused && ui.button("Continuar download").clicked() {
                                if let Some(control) = &self.control { control.resume(); }
                                self.phase = Phase::Downloading;
                                self.status = "Retomando download...".into();
                            }
                            if matches!(self.phase, Phase::Downloading | Phase::Paused) && ui.button("Cancelar download").clicked() {
                                if self.control.as_ref().is_some_and(|control| control.cancel()) {
                                    self.status = "Cancelando download...".into();
                                } else {
                                    self.status = "A verificação já começou; aguarde a conclusão.".into();
                                }
                            }
                        });
                    }
                });
            ui.add_space(12.0);
            if let Some(release) = &self.release {
                egui::Frame::new()
                    .fill(Color32::from_rgb(0x1B, 0x20, 0x29))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(0x2C, 0x34, 0x42)))
                    .corner_radius(8.0)
                    .inner_margin(egui::Margin::same(16))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.heading(format!("Novidades da versão {}", release.tag));
                        ui.hyperlink_to("Ver publicação no GitHub", &release.page_url);
                        egui::ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                            ui.label(if release.notes.trim().is_empty() { "Nenhuma nota de versão publicada." } else { &release.notes });
                        });
                    });
            }
            ui.add_space(12.0);
            let tone = if self.phase == Phase::Error { Color32::from_rgb(0xF8, 0x71, 0x71) } else if self.phase == Phase::Installed { Color32::from_rgb(0x22, 0xC5, 0x5E) } else { Color32::from_rgb(0x92, 0x9C, 0xAA) };
            ui.colored_label(tone, &self.status);
            ui.label(RichText::new(format!("Origem: github.com/{REPOSITORY} · SHA-256 verificado antes da instalação")).small().color(Color32::from_rgb(0x92, 0x9C, 0xAA)));
        });
    }
}
