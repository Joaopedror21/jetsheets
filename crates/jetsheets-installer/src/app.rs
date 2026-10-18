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
    installer_theme as theme,
    release::{EXECUTABLE_NAME, REPOSITORY, Release, Version},
};

enum Event {
    Release(Result<Release, String>),
    Progress(u64),
    Verifying,
    Completed(Result<(), String>),
    Uninstalled(Result<(), String>),
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
    Uninstalling,
    Error,
}

pub struct InstallerApp {
    folder: PathBuf,
    folder_input: String,
    folder_dirty: bool,
    launch_after_install: bool,
    confirm_uninstall: bool,
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
        theme::apply(ctx);
        let folder = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
            .join("JetSheets");
        let (sender, receiver) = mpsc::channel();
        let mut app = Self {
            folder_input: folder.display().to_string(),
            folder_dirty: false,
            folder,
            launch_after_install: false,
            confirm_uninstall: false,
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
        if self.folder_input.trim().is_empty() {
            self.installed = None;
            self.folder_error = Some("Escolha uma pasta de instalação.".into());
            return;
        }
        if !self.folder.is_absolute() {
            self.installed = None;
            self.folder_error =
                Some("Informe um caminho completo para a pasta de instalação.".into());
            return;
        }
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

    fn begin_uninstall(&mut self, ctx: &egui::Context) {
        if self.installed.is_none() || self.folder_error.is_some() || self.folder_dirty {
            return;
        }
        self.phase = Phase::Uninstalling;
        self.status = "Desinstalando JetSheets...".into();
        let folder = self.folder.clone();
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = sender.send(Event::Uninstalled(install::uninstall(&folder)));
            ctx.request_repaint();
        });
    }

    fn poll_events(&mut self) {
        while let Ok(event) = self.receiver.try_recv() {
            match event {
                Event::Release(Ok(release)) => {
                    if self.phase == Phase::Checking {
                        self.status = if self.installed.as_ref().is_some_and(|record| {
                            Version::parse(&record.version)
                                .is_ok_and(|installed| installed >= release.version)
                        }) {
                            format!(
                                "Nenhuma atualização mais recente publicada; última Release: {}.",
                                release.tag
                            )
                        } else {
                            format!("Versão {} disponível no GitHub.", release.tag)
                        };
                        self.phase = Phase::Ready;
                    }
                    self.release = Some(release);
                }
                Event::Release(Err(error)) => {
                    let _ = jetsheets_diagnostics::record("installer.log", &error);
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
                    if self.launch_after_install {
                        self.open_installed();
                    }
                }
                Event::Completed(Err(error)) => {
                    let _ = jetsheets_diagnostics::record("installer.log", &error);
                    self.control = None;
                    self.phase = Phase::Error;
                    self.status = error;
                }
                Event::Uninstalled(Ok(())) => {
                    self.refresh_folder();
                    self.phase = Phase::Ready;
                    self.status =
                        "JetSheets desinstalado. Outros arquivos da pasta foram mantidos.".into();
                }
                Event::Uninstalled(Err(error)) => {
                    let _ = jetsheets_diagnostics::record("installer.log", &error);
                    self.refresh_folder();
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
            || self.folder_dirty
            || self.confirm_uninstall
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
            && !self.folder_dirty
            && !self.confirm_uninstall
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

    fn open_installed(&mut self) {
        if let Err(error) = Command::new(self.folder.join(EXECUTABLE_NAME)).spawn() {
            self.status = format!("Não foi possível abrir JetSheets: {error}");
            let _ = jetsheets_diagnostics::record("installer.log", &self.status);
            self.phase = Phase::Error;
        }
    }
}

impl eframe::App for InstallerApp {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_events();
        let ctx = root.ctx().clone();
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BACKGROUND))
            .show(root, |ui| {
                let bounds = ui.available_rect_before_wrap();
                self.draw_header(ui, bounds, &ctx);

                let body = egui::Rect::from_min_max(
                    egui::pos2(bounds.left() + 34.0, bounds.top() + 91.0),
                    egui::pos2(bounds.right() - 34.0, bounds.bottom() - 105.0),
                );
                let mut body_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(body)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(&mut body_ui, |ui| {
                        ui.set_width(body.width() - 8.0);
                        self.draw_status_card(ui);
                        ui.add_space(19.0);
                        self.draw_installation_options(ui);
                        ui.add_space(14.0);
                        self.draw_release_details(ui);
                    });
                self.draw_footer(ui, bounds, &ctx);
            });
        if self.confirm_uninstall {
            self.draw_uninstall_confirmation(&ctx);
        }
    }
}

impl InstallerApp {
    fn busy(&self) -> bool {
        matches!(
            self.phase,
            Phase::Downloading | Phase::Paused | Phase::Verifying | Phase::Uninstalling
        )
    }

    fn controls_locked(&self) -> bool {
        self.busy() || self.confirm_uninstall
    }

    fn draw_header(&self, ui: &mut egui::Ui, bounds: egui::Rect, ctx: &egui::Context) {
        let painter = ui.painter();
        let logo = egui::Rect::from_min_size(
            egui::pos2(bounds.left() + 34.0, bounds.top() + 22.0),
            egui::vec2(39.0, 39.0),
        );
        painter.rect_filled(logo, 8.0, theme::SURFACE_ALT);
        for row in 0..2 {
            for column in 0..2 {
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        logo.min
                            + egui::vec2(10.0 + column as f32 * 10.0, 10.0 + row as f32 * 10.0),
                        egui::vec2(8.0, 8.0),
                    ),
                    1.5,
                    theme::BLUE,
                );
            }
        }
        painter.text(
            egui::pos2(logo.right() + 13.0, logo.top() + 11.0),
            egui::Align2::LEFT_CENTER,
            "JetSheets",
            egui::FontId::proportional(21.0),
            theme::TEXT,
        );
        painter.text(
            egui::pos2(logo.right() + 13.0, logo.top() + 30.0),
            egui::Align2::LEFT_CENTER,
            "Instalação e atualizações",
            egui::FontId::proportional(11.0),
            theme::MUTED,
        );
        draw_window_controls(ui, bounds, ctx, self.controls_locked());
    }

    fn draw_status_card(&mut self, ui: &mut egui::Ui) {
        let version = self.installed.as_ref().map(|record| record.version.clone());
        let (title, detail, tone) = if self.folder_dirty {
            (
                "Pasta não confirmada",
                "Confirme o caminho para consultar a instalação.",
                theme::MUTED,
            )
        } else if self.folder_error.is_some() {
            (
                "Não foi possível verificar a pasta",
                "Corrija o caminho antes de continuar.",
                theme::ERROR,
            )
        } else if self.phase == Phase::Uninstalling {
            (
                "Desinstalando JetSheets",
                "Aguarde a remoção dos arquivos do aplicativo.",
                theme::MUTED,
            )
        } else if version.is_some() {
            (
                "Instalado nesta pasta",
                "O aplicativo está presente na pasta selecionada.",
                theme::SUCCESS,
            )
        } else {
            (
                "Não instalado nesta pasta",
                "Nenhuma instalação reconhecida nesta pasta.",
                theme::MUTED,
            )
        };
        egui::Frame::new()
            .fill(theme::SURFACE)
            .stroke(Stroke::new(1.0, theme::BORDER))
            .corner_radius(9.0)
            .inner_margin(egui::Margin::same(18))
            .show(ui, |ui| {
                ui.set_min_width((ui.available_width() - 1.0).max(100.0));
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("ESTADO DA INSTALAÇÃO")
                                .monospace()
                                .size(10.0)
                                .color(theme::DIM),
                        );
                        ui.add_space(3.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("●").size(16.0).color(tone));
                            ui.label(RichText::new(title).size(19.0).strong().color(theme::TEXT));
                        });
                        ui.label(RichText::new(detail).size(12.0).color(theme::MUTED));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(version) = &version {
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new(version)
                                        .size(14.0)
                                        .strong()
                                        .color(theme::TEXT),
                                );
                                if ui
                                    .add_enabled(
                                        !self.controls_locked()
                                            && !self.folder_dirty
                                            && self.folder_error.is_none(),
                                        egui::Button::new("Desinstalar").frame(false),
                                    )
                                    .clicked()
                                {
                                    self.confirm_uninstall = true;
                                }
                            });
                        }
                    });
                });
            });
    }

    fn draw_installation_options(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Local de instalação").size(14.0).strong());
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            let editable = !self.controls_locked();
            let width = (ui.available_width() - 112.0).max(100.0);
            let response = ui.add_enabled(
                editable,
                egui::TextEdit::singleline(&mut self.folder_input).desired_width(width),
            );
            if response.changed() {
                self.folder_dirty = true;
            }
            if response.lost_focus()
                || (response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)))
            {
                self.folder = PathBuf::from(self.folder_input.trim());
                self.folder_dirty = false;
                self.refresh_folder();
            }
            if ui
                .add_enabled(
                    editable,
                    egui::Button::new("Procurar").min_size(egui::vec2(85.0, 29.0)),
                )
                .clicked()
                && let Some(folder) = rfd::FileDialog::new()
                    .set_directory(&self.folder)
                    .pick_folder()
            {
                self.folder = folder;
                self.folder_input = self.folder.display().to_string();
                self.folder_dirty = false;
                self.refresh_folder();
            }
        });
        if let Some(error) = &self.folder_error {
            ui.colored_label(theme::ERROR, error);
        }
        ui.checkbox(
            &mut self.launch_after_install,
            "Abrir JetSheets após a instalação",
        );
    }

    fn draw_release_details(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label(RichText::new("Versões").size(14.0).strong());
        let bundled = if bundle::available() {
            format!("Incluída neste instalador: v{}", env!("CARGO_PKG_VERSION"))
        } else {
            "Esta compilação não inclui o aplicativo.".to_owned()
        };
        ui.label(RichText::new(bundled).size(12.0).color(theme::MUTED));
        if let Some(release) = &self.release {
            ui.label(
                RichText::new(format!(
                    "Publicada no GitHub: {} · {:.1} MB",
                    release.tag,
                    release.size as f64 / 1_000_000.0
                ))
                .size(12.0)
                .color(theme::MUTED),
            );
            if let Some(record) = &self.installed
                && record.version == release.tag
                && record.sha256 != release.sha256
            {
                ui.colored_label(
                    Color32::from_rgb(245, 158, 11),
                    "Esta tag foi republicada com outro arquivo. É necessária uma nova versão.",
                );
            }
        }
        if matches!(
            self.phase,
            Phase::Downloading | Phase::Paused | Phase::Verifying
        ) && let Some(release) = &self.release
        {
            ui.add_space(10.0);
            ui.add(
                egui::ProgressBar::new(
                    (self.downloaded as f32 / release.size as f32).clamp(0.0, 1.0),
                )
                .show_percentage()
                .text(format!(
                    "{:.1} / {:.1} MB",
                    self.downloaded as f64 / 1_000_000.0,
                    release.size as f64 / 1_000_000.0
                )),
            );
            ui.horizontal(|ui| {
                if self.phase == Phase::Downloading
                    && ui.button("Pausar").clicked()
                    && self.control.as_ref().is_some_and(|control| control.pause())
                {
                    self.phase = Phase::Paused;
                    self.status = "Download pausado.".into();
                }
                if self.phase == Phase::Paused && ui.button("Continuar").clicked() {
                    if let Some(control) = &self.control {
                        control.resume();
                    }
                    self.phase = Phase::Downloading;
                    self.status = "Retomando download...".into();
                }
                if matches!(self.phase, Phase::Downloading | Phase::Paused)
                    && ui.button("Cancelar download").clicked()
                {
                    self.status = if self
                        .control
                        .as_ref()
                        .is_some_and(|control| control.cancel())
                    {
                        "Cancelando download...".into()
                    } else {
                        "A verificação já começou; aguarde a conclusão.".into()
                    };
                }
            });
        }
        if let Some(release) = &self.release {
            ui.add_space(7.0);
            ui.collapsing(format!("Notas de {}", release.tag), |ui| {
                ui.hyperlink_to("Ver Release no GitHub", &release.page_url);
                ui.label(if release.notes.trim().is_empty() {
                    "Nenhuma nota publicada."
                } else {
                    &release.notes
                });
            });
        }
        ui.add_space(8.0);
        ui.label(
            RichText::new(format!(
                "Fonte: github.com/{REPOSITORY} · SHA-256 verificado"
            ))
            .size(10.0)
            .color(theme::DIM),
        );
    }

    fn draw_footer(&mut self, ui: &mut egui::Ui, bounds: egui::Rect, ctx: &egui::Context) {
        let footer = egui::Rect::from_min_max(
            egui::pos2(bounds.left() + 34.0, bounds.bottom() - 91.0),
            egui::pos2(bounds.right() - 34.0, bounds.bottom() - 18.0),
        );
        ui.painter().line_segment(
            [
                egui::pos2(footer.left(), footer.top()),
                egui::pos2(footer.right(), footer.top()),
            ],
            Stroke::new(1.0, theme::BORDER),
        );
        let mut footer_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(footer)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        footer_ui.add_space(5.0);
        let tone = if self.phase == Phase::Error {
            theme::ERROR
        } else if self.phase == Phase::Installed {
            theme::SUCCESS
        } else {
            theme::MUTED
        };
        egui::ScrollArea::vertical()
            .max_height(25.0)
            .show(&mut footer_ui, |ui| {
                ui.label(RichText::new(&self.status).size(11.0).color(tone));
            });
        footer_ui.horizontal(|ui| {
            let close_label = if matches!(self.phase, Phase::Downloading | Phase::Paused) {
                "Cancelar download"
            } else {
                "Fechar"
            };
            if ui
                .add_enabled(
                    !matches!(self.phase, Phase::Verifying | Phase::Uninstalling)
                        && !self.confirm_uninstall,
                    egui::Button::new(close_label).min_size(egui::vec2(71.0, 32.0)),
                )
                .clicked()
            {
                if matches!(self.phase, Phase::Downloading | Phase::Paused) {
                    self.status = if self
                        .control
                        .as_ref()
                        .is_some_and(|control| control.cancel())
                    {
                        "Cancelando download...".into()
                    } else {
                        "A verificação já começou; aguarde a conclusão.".into()
                    };
                } else {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            if ui
                .add_enabled(
                    !matches!(self.phase, Phase::Checking) && !self.controls_locked(),
                    egui::Button::new("Buscar atualizações").min_size(egui::vec2(140.0, 32.0)),
                )
                .clicked()
            {
                self.check_releases(ctx);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let can_bundle = self.can_install_bundle();
                let can_remote = self.can_install();
                let install_text = if can_bundle {
                    if self.installed.is_some() {
                        format!("Atualizar para v{}", env!("CARGO_PKG_VERSION"))
                    } else {
                        format!("Instalar v{}", env!("CARGO_PKG_VERSION"))
                    }
                } else {
                    self.install_label()
                };
                if ui
                    .add_enabled(
                        can_bundle || can_remote,
                        egui::Button::new(
                            RichText::new(install_text).strong().color(Color32::WHITE),
                        )
                        .fill(theme::BLUE)
                        .min_size(egui::vec2(125.0, 32.0)),
                    )
                    .clicked()
                {
                    if can_bundle {
                        self.begin_bundle_install(ctx);
                    } else {
                        self.begin_install(ctx);
                    }
                }
                if self.installed.is_some()
                    && ui
                        .add_enabled(
                            !self.controls_locked()
                                && !self.folder_dirty
                                && self.folder_error.is_none(),
                            egui::Button::new("Abrir JetSheets").min_size(egui::vec2(113.0, 32.0)),
                        )
                        .clicked()
                {
                    self.open_installed();
                }
            });
        });
    }

    fn draw_uninstall_confirmation(&mut self, ctx: &egui::Context) {
        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.confirm_uninstall = false;
            return;
        }
        let mut confirm = false;
        let mut cancel = false;
        egui::Window::new("Desinstalar JetSheets")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_min_width(365.0);
                ui.label("Remover JetSheets da pasta selecionada?");
                ui.label(RichText::new(self.folder.display().to_string()).size(11.0).color(theme::MUTED));
                ui.add_space(7.0);
                ui.label(RichText::new("O executável e o registro serão removidos. Projetos e outros arquivos serão mantidos.").size(12.0).color(theme::MUTED));
                ui.add_space(13.0);
                ui.horizontal(|ui| {
                    if ui.button("Manter instalado").clicked() {
                        cancel = true;
                    }
                    if ui.add(egui::Button::new("Desinstalar").fill(theme::ERROR)).clicked() {
                        confirm = true;
                    }
                });
            });
        if confirm {
            self.confirm_uninstall = false;
            self.begin_uninstall(ctx);
        } else if cancel {
            self.confirm_uninstall = false;
        }
    }
}

fn draw_window_controls(ui: &mut egui::Ui, rect: egui::Rect, ctx: &egui::Context, busy: bool) {
    let controls = egui::Rect::from_min_max(
        egui::pos2(rect.right() - 121.0, rect.top() + 15.0),
        egui::pos2(rect.right() - 19.0, rect.top() + 52.0),
    );
    let drag = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 22.0, rect.top() + 5.0),
        egui::pos2(controls.left(), controls.bottom()),
    );
    if ui
        .interact(drag, ui.id().with("title_drag"), egui::Sense::drag())
        .drag_started()
    {
        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    let mut bar = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(controls)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    if bar
        .add(
            egui::Button::new("−")
                .frame(false)
                .min_size(egui::vec2(27.0, 27.0)),
        )
        .clicked()
    {
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    }
    if bar
        .add(
            egui::Button::new("□")
                .frame(false)
                .min_size(egui::vec2(27.0, 27.0)),
        )
        .clicked()
    {
        let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }
    if bar
        .add_enabled(
            !busy,
            egui::Button::new("×")
                .frame(false)
                .min_size(egui::vec2(27.0, 27.0)),
        )
        .clicked()
    {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}
