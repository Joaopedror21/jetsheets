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
    folder_input: String,
    folder_touched: bool,
    launch_after_install: bool,
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
            folder_touched: false,
            folder,
            launch_after_install: false,
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
                let outer = ui.available_rect_before_wrap();
                ui.painter().rect_filled(outer, 0.0, theme::BACKGROUND);

                let sidebar = egui::Rect::from_min_max(
                    outer.min + egui::vec2(4.0, 4.0),
                    egui::pos2(outer.left() + 252.0, outer.bottom() - 4.0),
                );
                self.draw_sidebar(ui, sidebar);

                let content = egui::Rect::from_min_max(
                    egui::pos2(sidebar.right() + 31.0, outer.top() + 43.0),
                    egui::pos2(outer.right() - 30.0, outer.bottom() - 24.0),
                );
                self.draw_content(ui, content, &ctx);
                draw_window_controls(ui, outer, &ctx);
                theme::paint_neon_border(ui.painter(), outer.shrink(1.0));
            });
    }
}

impl InstallerApp {
    fn draw_sidebar(&self, ui: &mut egui::Ui, rect: egui::Rect) {
        theme::paint_sidebar(ui.painter(), rect);
        let painter = ui.painter();
        let x = rect.left() + 30.0;
        let logo =
            egui::Rect::from_min_size(egui::pos2(x, rect.top() + 33.0), egui::vec2(52.0, 52.0));
        painter.rect_filled(logo, 9.0, Color32::from_rgb(6, 46, 111));
        painter.rect_stroke(
            logo,
            9.0,
            Stroke::new(1.0, theme::BLUE),
            egui::StrokeKind::Inside,
        );
        for row in 0..3 {
            for col in 0..2 {
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        logo.min + egui::vec2(11.0 + col as f32 * 15.0, 11.0 + row as f32 * 11.0),
                        egui::vec2(12.0, 8.0),
                    ),
                    2.0,
                    if row == 0 && col == 1 {
                        theme::BLUE
                    } else {
                        theme::BLUE_LIGHT
                    },
                );
            }
        }
        painter.text(
            egui::pos2(x + 67.0, logo.center().y),
            egui::Align2::LEFT_CENTER,
            "JetSheets",
            egui::FontId::proportional(24.0),
            theme::TEXT,
        );

        let active = match self.phase {
            Phase::Installed => 3,
            Phase::Downloading | Phase::Paused | Phase::Verifying => 2,
            Phase::Checking | Phase::Ready | Phase::Error => usize::from(self.folder_touched),
        };
        let steps = [
            "Boas-vindas",
            "Pasta de instalação",
            "Instalação",
            "Concluído",
        ];
        for (index, label) in steps.iter().enumerate() {
            let center = egui::pos2(x + 17.0, rect.top() + 132.0 + index as f32 * 43.0);
            if index < 3 {
                painter.line_segment(
                    [
                        center + egui::vec2(0.0, 14.0),
                        center + egui::vec2(0.0, 29.0),
                    ],
                    Stroke::new(1.0, theme::BORDER),
                );
            }
            let selected = index == active;
            painter.circle_filled(
                center,
                13.0,
                if selected {
                    theme::BLUE
                } else {
                    theme::SIDEBAR
                },
            );
            painter.circle_stroke(
                center,
                12.5,
                Stroke::new(
                    1.2,
                    if selected {
                        theme::BLUE_LIGHT
                    } else {
                        theme::DIM
                    },
                ),
            );
            painter.text(
                center,
                egui::Align2::CENTER_CENTER,
                (index + 1).to_string(),
                egui::FontId::proportional(13.0),
                if selected { Color32::WHITE } else { theme::DIM },
            );
            painter.text(
                center + egui::vec2(28.0, 0.0),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::proportional(13.0),
                if selected { theme::TEXT } else { theme::MUTED },
            );
        }
        painter.text(
            egui::pos2(x, rect.bottom() - 60.0),
            egui::Align2::LEFT_BOTTOM,
            "PLANILHAS SIMPLES.",
            egui::FontId::monospace(11.0),
            theme::MUTED,
        );
        painter.text(
            egui::pos2(x, rect.bottom() - 43.0),
            egui::Align2::LEFT_BOTTOM,
            "POSSIBILIDADES MAIORES.",
            egui::FontId::monospace(11.0),
            theme::BLUE_LIGHT,
        );
        painter.line_segment(
            [
                egui::pos2(x, rect.bottom() - 24.0),
                egui::pos2(x + 105.0, rect.bottom() - 24.0),
            ],
            Stroke::new(2.0, theme::BLUE),
        );
    }

    fn draw_content(&mut self, ui: &mut egui::Ui, rect: egui::Rect, ctx: &egui::Context) {
        let footer_top = rect.bottom() - 82.0;
        let body = egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), footer_top - 8.0));
        let mut body_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(body)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(&mut body_ui, |ui| {
                ui.set_width(body.width() - 9.0);
                ui.label(RichText::new("B E M - V I N D O   A O").monospace().size(11.0).color(theme::MUTED));
                ui.add_space(3.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label(RichText::new("Jet").size(51.0).strong().color(theme::TEXT));
                    ui.label(RichText::new("Sheets").size(51.0).strong().color(theme::BLUE_LIGHT));
                });
                ui.label(RichText::new("Uma forma mais rápida e inteligente de trabalhar com seus dados.").size(16.0).color(Color32::from_rgb(200, 219, 247)));
                ui.add_space(12.0);
                ui.label(RichText::new("Organize, analise e visualize informações em uma interface clara, moderna e feita para manter seu foco.").size(13.0).color(theme::MUTED));
                ui.add_space(17.0);

                ui.columns(3, |columns| {
                    feature_card(&mut columns[0], "ϟ", "Rápido", "Trabalhe com tabelas sem perder tempo.");
                    feature_card(&mut columns[1], "▦", "Interface clara", "Concentre-se nos seus dados.");
                    feature_card(&mut columns[2], "▥", "Gráficos", "Transforme dados em análises.");
                });
                ui.add_space(14.0);
                ui.label(RichText::new("Pasta de instalação").size(13.0).strong().color(theme::TEXT));
                ui.horizontal(|ui| {
                    let editable = !matches!(self.phase, Phase::Downloading | Phase::Paused | Phase::Verifying);
                    let width = (ui.available_width() - 52.0).max(100.0);
                    let response = ui.add_enabled(
                        editable,
                        egui::TextEdit::singleline(&mut self.folder_input).desired_width(width),
                    );
                    if response.changed() {
                        self.folder_touched = true;
                        self.folder_error = Some("Confirme a pasta antes de instalar.".into());
                    }
                    if response.lost_focus() || (response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter))) {
                        self.folder = PathBuf::from(self.folder_input.trim());
                        self.refresh_folder();
                    }
                    if ui.add_enabled(editable, egui::Button::new("...").min_size(egui::vec2(36.0, 25.0))).on_hover_text("Escolher pasta").clicked()
                        && let Some(folder) = rfd::FileDialog::new().set_directory(&self.folder).pick_folder()
                    {
                        self.folder_touched = true;
                        self.folder = folder;
                        self.folder_input = self.folder.display().to_string();
                        self.refresh_folder();
                    }
                });
                if let Some(error) = &self.folder_error {
                    ui.colored_label(Color32::from_rgb(255, 111, 111), error);
                }
                ui.checkbox(&mut self.launch_after_install, "Abrir JetSheets após instalar");
                ui.add_space(5.0);
                let installed = self.installed.as_ref().map_or_else(
                    || "Nenhuma versão instalada nesta pasta".to_owned(),
                    |record| format!("Instalado: {}", record.version),
                );
                ui.label(RichText::new(installed).size(12.0).color(theme::MUTED));
                if let Some(release) = &self.release {
                    ui.label(RichText::new(format!("GitHub: {}  ·  {:.1} MB", release.tag, release.size as f64 / 1_000_000.0)).size(12.0).color(theme::MUTED));
                    if let Some(record) = &self.installed
                        && record.version == release.tag && record.sha256 != release.sha256
                    {
                        ui.colored_label(Color32::from_rgb(245, 158, 11), "A mesma versão foi republicada com arquivo diferente. É necessária uma nova tag.");
                    }
                }
                if matches!(self.phase, Phase::Downloading | Phase::Paused | Phase::Verifying)
                    && let Some(release) = &self.release
                {
                    ui.add_space(8.0);
                    ui.add(egui::ProgressBar::new((self.downloaded as f32 / release.size as f32).clamp(0.0, 1.0))
                        .show_percentage()
                        .text(format!("{:.1} / {:.1} MB", self.downloaded as f64 / 1_000_000.0, release.size as f64 / 1_000_000.0)));
                    ui.horizontal(|ui| {
                        if self.phase == Phase::Downloading && ui.button("Pausar").clicked()
                            && self.control.as_ref().is_some_and(|control| control.pause())
                        {
                            self.phase = Phase::Paused;
                            self.status = "Download pausado. A retomada usa HTTP Range.".into();
                        }
                        if self.phase == Phase::Paused && ui.button("Continuar").clicked() {
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
                if let Some(release) = &self.release {
                    ui.add_space(8.0);
                    ui.collapsing(format!("Novidades de {}", release.tag), |ui| {
                        ui.hyperlink_to("Ver publicação no GitHub", &release.page_url);
                        ui.label(if release.notes.trim().is_empty() { "Nenhuma nota de versão publicada." } else { &release.notes });
                    });
                }
                ui.add_space(8.0);
                ui.label(RichText::new(format!("Origem: github.com/{REPOSITORY} · SHA-256 verificado")).size(10.0).color(theme::DIM));
            });

        let footer = egui::Rect::from_min_max(
            egui::pos2(rect.left(), footer_top),
            egui::pos2(rect.right(), rect.bottom()),
        );
        let mut footer_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(footer)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        footer_ui.painter().line_segment(
            [
                egui::pos2(footer.left(), footer.top()),
                egui::pos2(footer.right(), footer.top()),
            ],
            Stroke::new(1.0, theme::BORDER),
        );
        footer_ui.add_space(7.0);
        let tone = if self.phase == Phase::Error {
            Color32::from_rgb(255, 111, 111)
        } else if self.phase == Phase::Installed {
            Color32::from_rgb(34, 197, 94)
        } else {
            theme::MUTED
        };
        egui::ScrollArea::vertical()
            .max_height(30.0)
            .show(&mut footer_ui, |ui| {
                ui.label(RichText::new(&self.status).size(11.0).color(tone));
            });
        footer_ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.phase != Phase::Verifying,
                    egui::Button::new("Cancelar").min_size(egui::vec2(82.0, 33.0)),
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
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.installed.is_some()
                    && ui
                        .add_enabled(
                            !matches!(
                                self.phase,
                                Phase::Downloading | Phase::Paused | Phase::Verifying
                            ),
                            egui::Button::new("Abrir JetSheets").min_size(egui::vec2(105.0, 33.0)),
                        )
                        .clicked()
                {
                    self.open_installed();
                }
                let can_bundle = self.can_install_bundle();
                let can_remote = self.can_install();
                let install_text = if can_bundle {
                    "Instalar  →".to_owned()
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
                        .min_size(egui::vec2(118.0, 33.0)),
                    )
                    .clicked()
                {
                    if can_bundle {
                        self.begin_bundle_install(ctx);
                    } else {
                        self.begin_install(ctx);
                    }
                }
                if ui
                    .add_enabled(
                        !matches!(
                            self.phase,
                            Phase::Checking | Phase::Downloading | Phase::Paused | Phase::Verifying
                        ),
                        egui::Button::new("Atualizações").min_size(egui::vec2(96.0, 33.0)),
                    )
                    .clicked()
                {
                    self.check_releases(ctx);
                }
            });
        });
    }
}

fn feature_card(ui: &mut egui::Ui, icon: &str, title: &str, detail: &str) {
    egui::Frame::new()
        .fill(theme::SURFACE)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(9.0)
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_min_width((ui.available_width() - 1.0).max(80.0));
            ui.set_min_height(92.0);
            ui.label(RichText::new(icon).size(23.0).color(theme::BLUE_LIGHT));
            ui.label(RichText::new(title).size(12.0).strong().color(theme::TEXT));
            ui.label(RichText::new(detail).size(10.0).color(theme::MUTED));
        });
}

fn draw_window_controls(ui: &mut egui::Ui, rect: egui::Rect, ctx: &egui::Context) {
    let controls = egui::Rect::from_min_max(
        egui::pos2(rect.right() - 119.0, rect.top() + 5.0),
        egui::pos2(rect.right() - 7.0, rect.top() + 38.0),
    );
    let drag = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 253.0, rect.top() + 5.0),
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
                .min_size(egui::vec2(30.0, 25.0)),
        )
        .clicked()
    {
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    }
    if bar
        .add(
            egui::Button::new("□")
                .frame(false)
                .min_size(egui::vec2(30.0, 25.0)),
        )
        .clicked()
    {
        let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }
    if bar
        .add(
            egui::Button::new("×")
                .frame(false)
                .min_size(egui::vec2(30.0, 25.0)),
        )
        .clicked()
    {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}
