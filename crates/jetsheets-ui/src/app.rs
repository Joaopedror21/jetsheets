use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use eframe::egui::{
    self, Align2, FontId, Rect, RichText, ScrollArea, Sense, Stroke, StrokeKind, TextEdit, pos2,
    vec2,
};
use jetsheets_charts::{
    ChartKind, ChartPoint, ChartSpec, DashboardSpec, MAX_DASHBOARD_CHARTS, prepare_chart,
};
use jetsheets_core::{
    AggregateOperation, AggregateSpec, ColumnKind, FilterExpr, FilterSummary, FilterValue,
    GroupResult, Operator, SortDirection, SortSpec, Table, aggregate_rows, evaluate_filter,
    sort_rows, summarize,
};
use jetsheets_language::{ParseError, ParsedCommand, parse_command, parse_command_with_override};
use jetsheets_storage::{
    ImportedXlsx, Project, export_aggregate_csv, export_csv_view, export_xlsx_view, import_csv,
    import_xlsx, list_xlsx_sheets, load_project, save_project,
};

use crate::{
    chart_view,
    grid_selection::GridSelection,
    theme::{self, JetTheme},
};

const ROW_HEIGHT: f32 = 30.0;
const HEADER_HEIGHT: f32 = 36.0;
const ROW_NUMBER_WIDTH: f32 = 52.0;
const DEFAULT_COLUMN_WIDTH: f32 = 160.0;

enum TaskOutcome {
    Imported {
        ticket: (u64, u64),
        result: Result<Table, String>,
    },
    XlsxSheets {
        ticket: (u64, u64),
        result: Result<(PathBuf, Vec<String>), String>,
    },
    ImportedXlsx {
        ticket: (u64, u64),
        result: Result<ImportedXlsx, String>,
    },
    Opened {
        ticket: (u64, u64),
        result: Result<(PathBuf, Project), String>,
    },
    Saved {
        path: PathBuf,
        document_id: u64,
        revision: u64,
        result: Result<(), String>,
    },
    Exported {
        path: PathBuf,
        rows: usize,
        result: Result<(), String>,
    },
    ExportedGroups {
        path: PathBuf,
        result: Result<usize, String>,
    },
}

#[derive(Clone, Copy)]
enum ReplaceAction {
    New,
    Import,
    ImportXlsx,
    Open,
}

struct PendingXlsx {
    path: PathBuf,
    sheets: Vec<String>,
    ticket: (u64, u64),
}

enum ClipboardAction {
    Copy,
    Cut,
    Clear,
    Paste(String),
}

#[derive(Clone)]
struct ViewState {
    filter: Option<FilterExpr>,
    sort: Option<SortSpec>,
    aggregate: Option<AggregateSpec>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum MainView {
    #[default]
    Table,
    Dashboard,
}

pub struct JetSheetsApp {
    project: Project,
    path: Option<PathBuf>,
    command: String,
    preview: Option<ParsedCommand>,
    parse_error: Option<ParseError>,
    command_history: Vec<String>,
    undo: Vec<ViewState>,
    redo: Vec<ViewState>,
    visible_rows: Vec<usize>,
    summary: FilterSummary,
    selected_row: Option<usize>,
    selected_column: Option<usize>,
    selection: Option<GridSelection>,
    focused_cell_id: Option<egui::Id>,
    drag_selecting: bool,
    theme: JetTheme,
    show_settings: bool,
    chart_draft: Option<ChartSpec>,
    chart_page: usize,
    chart_data: Option<Result<Vec<ChartPoint>, String>>,
    aggregate_data: Option<Result<Vec<GroupResult>, String>>,
    dashboard_data: Vec<Result<Vec<ChartPoint>, String>>,
    dashboard_pages: Vec<usize>,
    active_view: MainView,
    column_widths: Vec<f32>,
    status: String,
    worker: Option<Receiver<TaskOutcome>>,
    revision: u64,
    saved_revision: u64,
    document_id: u64,
    confirm_replace: Option<ReplaceAction>,
    pending_xlsx: Option<PendingXlsx>,
    confirm_close: bool,
    close_after_save: bool,
    allow_close: bool,
    pending_cell_refresh: bool,
}

impl JetSheetsApp {
    pub fn new(ctx: &egui::Context) -> Self {
        let theme = JetTheme::default();
        theme.apply(ctx);

        let project = Project {
            table: Table::new(
                "Nova tabela",
                vec!["Produto".into(), "Valor".into(), "Quantidade".into()],
            ),
            filter: None,
            sort: None,
            aggregate: None,
            chart: None,
            dashboard: DashboardSpec::default(),
        };
        let mut app = Self {
            project,
            path: None,
            command: String::new(),
            preview: None,
            parse_error: None,
            command_history: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            visible_rows: Vec::new(),
            summary: FilterSummary {
                original_count: 0,
                matched_count: 0,
                percentage: 0.0,
                columns: Vec::new(),
            },
            selected_row: None,
            selected_column: None,
            selection: None,
            focused_cell_id: None,
            drag_selecting: false,
            theme,
            show_settings: false,
            chart_draft: None,
            chart_page: 0,
            chart_data: None,
            aggregate_data: None,
            dashboard_data: Vec::new(),
            dashboard_pages: Vec::new(),
            active_view: MainView::Table,
            column_widths: Vec::new(),
            status: "Adicione linhas ou importe um arquivo CSV para começar.".into(),
            worker: None,
            revision: 0,
            saved_revision: 0,
            document_id: 0,
            confirm_replace: None,
            pending_xlsx: None,
            confirm_close: false,
            close_after_save: false,
            allow_close: false,
            pending_cell_refresh: false,
        };
        app.refresh();
        app
    }

    fn refresh(&mut self) {
        self.project.table.infer_column_types();
        self.column_widths
            .resize(self.project.table.column_count(), DEFAULT_COLUMN_WIDTH);
        let mut visible_rows = match &self.project.filter {
            Some(expr) => match evaluate_filter(&self.project.table, expr) {
                Ok(indices) => indices,
                Err(error) => {
                    self.status = error;
                    Vec::new()
                }
            },
            None => (0..self.project.table.row_count()).collect(),
        };
        if let Some(spec) = &self.project.sort {
            match sort_rows(&self.project.table, &visible_rows, spec) {
                Ok(indices) => visible_rows = indices,
                Err(error) => self.status = error,
            }
        }
        if self.visible_rows != visible_rows {
            self.selection = None;
        }
        self.visible_rows = visible_rows;
        match summarize(&self.project.table, &self.visible_rows) {
            Ok(summary) => self.summary = summary,
            Err(error) => self.status = error,
        }
        self.chart_data = self
            .project
            .chart
            .as_ref()
            .map(|spec| prepare_chart(&self.project.table, &self.visible_rows, spec));
        self.aggregate_data = self
            .project
            .aggregate
            .as_ref()
            .map(|spec| aggregate_rows(&self.project.table, &self.visible_rows, spec));
        self.dashboard_data = self
            .project
            .dashboard
            .charts
            .iter()
            .map(|spec| prepare_chart(&self.project.table, &self.visible_rows, spec))
            .collect();
        self.dashboard_pages
            .resize(self.project.dashboard.charts.len(), 0);
        self.pending_cell_refresh = false;
        if self
            .selected_row
            .is_some_and(|row| row >= self.project.table.row_count())
        {
            self.selected_row = None;
        }
        if self
            .selected_row
            .is_some_and(|row| !self.visible_rows.contains(&row))
        {
            self.selected_row = None;
        }
    }

    fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.refresh();
    }

    fn replace_project(&mut self, project: Project, path: Option<PathBuf>) {
        self.project = project;
        self.path = path;
        self.command.clear();
        self.preview = None;
        self.parse_error = None;
        self.command_history.clear();
        self.undo.clear();
        self.redo.clear();
        self.selected_row = None;
        self.selected_column = None;
        self.selection = None;
        self.focused_cell_id = None;
        self.drag_selecting = false;
        self.chart_draft = None;
        self.chart_page = 0;
        self.chart_data = None;
        self.aggregate_data = None;
        self.pending_xlsx = None;
        self.dashboard_data.clear();
        self.dashboard_pages.clear();
        self.active_view = MainView::Table;
        self.column_widths.clear();
        self.document_id = self.document_id.wrapping_add(1);
        self.revision = 0;
        self.saved_revision = 0;
        self.refresh();
    }

    fn new_table(&mut self) {
        if self.worker.is_some() {
            return;
        }
        self.replace_project(
            Project {
                table: Table::new(
                    "Nova tabela",
                    vec!["Produto".into(), "Valor".into(), "Quantidade".into()],
                ),
                filter: None,
                sort: None,
                aggregate: None,
                chart: None,
                dashboard: DashboardSpec::default(),
            },
            None,
        );
        self.status = "Nova tabela criada. Adicione uma linha para editar os dados.".into();
    }

    fn request_replace(&mut self, action: ReplaceAction, ctx: &egui::Context) {
        if self.worker.is_some() {
            self.status = "Aguarde a operação de arquivo em andamento.".into();
        } else if self.revision != self.saved_revision {
            self.confirm_replace = Some(action);
        } else {
            self.perform_replace(action, ctx);
        }
    }

    fn perform_replace(&mut self, action: ReplaceAction, ctx: &egui::Context) {
        match action {
            ReplaceAction::New => self.new_table(),
            ReplaceAction::Import => self.import_csv(ctx),
            ReplaceAction::ImportXlsx => self.choose_xlsx(ctx),
            ReplaceAction::Open => self.open_project(ctx),
        }
    }

    fn import_csv(&mut self, ctx: &egui::Context) {
        if self.worker.is_some() {
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Arquivos CSV", &["csv"])
            .pick_file()
        else {
            return;
        };
        let (sender, receiver) = mpsc::channel();
        let repaint = ctx.clone();
        let ticket = (self.document_id, self.revision);
        std::thread::spawn(move || {
            let result = import_csv(&path).map_err(|error| error.to_string());
            let _ = sender.send(TaskOutcome::Imported { ticket, result });
            repaint.request_repaint();
        });
        self.worker = Some(receiver);
        self.status = "Importando CSV...".into();
    }

    fn choose_xlsx(&mut self, ctx: &egui::Context) {
        if self.worker.is_some() {
            self.status = "Aguarde a operação de arquivo em andamento.".into();
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Planilha Excel", &["xlsx"])
            .pick_file()
        else {
            return;
        };
        let (sender, receiver) = mpsc::channel();
        let repaint = ctx.clone();
        let ticket = (self.document_id, self.revision);
        std::thread::spawn(move || {
            let result = list_xlsx_sheets(&path)
                .map(|sheets| (path, sheets))
                .map_err(|error| error.to_string());
            let _ = sender.send(TaskOutcome::XlsxSheets { ticket, result });
            repaint.request_repaint();
        });
        self.worker = Some(receiver);
        self.status = "Lendo as abas do XLSX...".into();
    }

    fn import_selected_xlsx(&mut self, ctx: &egui::Context, sheet: String) {
        let Some(pending) = self.pending_xlsx.take() else {
            return;
        };
        if pending.ticket != (self.document_id, self.revision) {
            self.status = "A tabela mudou; escolha o XLSX novamente.".into();
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let repaint = ctx.clone();
        let ticket = pending.ticket;
        self.status = format!("Importando aba '{sheet}' do XLSX...");
        std::thread::spawn(move || {
            let result = import_xlsx(&pending.path, &sheet).map_err(|error| error.to_string());
            let _ = sender.send(TaskOutcome::ImportedXlsx { ticket, result });
            repaint.request_repaint();
        });
        self.worker = Some(receiver);
    }

    fn xlsx_sheet_dialog(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.pending_xlsx else {
            return;
        };
        let mut selected = None;
        let mut cancel = false;
        egui::Window::new("Escolher aba do XLSX")
            .collapsible(false)
            .resizable(false)
            .default_width(390.0)
            .show(ctx, |ui| {
                ui.label("Escolha a aba que será importada como tabela.");
                ui.label("Outras abas, fórmulas e formatação não serão preservadas.");
                for sheet in &pending.sheets {
                    if ui.button(sheet).clicked() {
                        selected = Some(sheet.clone());
                    }
                }
                if ui.button("Cancelar").clicked() {
                    cancel = true;
                }
            });
        if cancel {
            self.pending_xlsx = None;
            self.status = "Importação XLSX cancelada.".into();
        } else if let Some(sheet) = selected {
            self.import_selected_xlsx(ctx, sheet);
        }
    }

    fn open_project(&mut self, ctx: &egui::Context) {
        if self.worker.is_some() {
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Projeto JetSheets", &["jetsheet"])
            .pick_file()
        else {
            return;
        };
        let (sender, receiver) = mpsc::channel();
        let repaint = ctx.clone();
        let ticket = (self.document_id, self.revision);
        std::thread::spawn(move || {
            let result = load_project(&path)
                .map(|project| (path, project))
                .map_err(|error| error.to_string());
            let _ = sender.send(TaskOutcome::Opened { ticket, result });
            repaint.request_repaint();
        });
        self.worker = Some(receiver);
        self.status = "Abrindo projeto...".into();
    }

    fn save_project(&mut self, ctx: &egui::Context, save_as: bool) {
        if self.worker.is_some() {
            self.status = "Aguarde a operação de arquivo em andamento.".into();
            return;
        }
        if self.pending_cell_refresh {
            self.refresh();
        }
        let path = if save_as { None } else { self.path.clone() }.or_else(|| {
            rfd::FileDialog::new()
                .add_filter("Projeto JetSheets", &["jetsheet"])
                .set_file_name("projeto.jetsheet")
                .save_file()
        });
        let Some(mut path) = path else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension("jetsheet");
        }
        let project = self.project.clone();
        let revision = self.revision;
        let document_id = self.document_id;
        let task_path = path.clone();
        let (sender, receiver) = mpsc::channel();
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let result = save_project(&task_path, &project).map_err(|error| error.to_string());
            let _ = sender.send(TaskOutcome::Saved {
                path,
                document_id,
                revision,
                result,
            });
            repaint.request_repaint();
        });
        self.worker = Some(receiver);
        self.status = "Salvando projeto...".into();
    }

    fn export_results(&mut self, ctx: &egui::Context) {
        if self.worker.is_some() {
            self.status = "Aguarde a operação de arquivo em andamento.".into();
            return;
        }
        if self.pending_cell_refresh {
            self.refresh();
        }
        let Some(mut path) = rfd::FileDialog::new()
            .add_filter("Arquivo CSV", &["csv"])
            .set_file_name("resultados.csv")
            .save_file()
        else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension("csv");
        }
        let table = self.project.table.clone();
        let visible_rows = self.visible_rows.clone();
        let rows = visible_rows.len();
        let task_path = path.clone();
        let (sender, receiver) = mpsc::channel();
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let result = export_csv_view(&task_path, &table, &visible_rows)
                .map_err(|error| error.to_string());
            let _ = sender.send(TaskOutcome::Exported { path, rows, result });
            repaint.request_repaint();
        });
        self.worker = Some(receiver);
        self.status = "Exportando resultados CSV...".into();
    }

    fn export_results_xlsx(&mut self, ctx: &egui::Context) {
        if self.worker.is_some() {
            self.status = "Aguarde a operação de arquivo em andamento.".into();
            return;
        }
        if self.pending_cell_refresh {
            self.refresh();
        }
        let Some(mut path) = rfd::FileDialog::new()
            .add_filter("Planilha Excel", &["xlsx"])
            .set_file_name("resultados.xlsx")
            .save_file()
        else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension("xlsx");
        }
        let table = self.project.table.clone();
        let visible_rows = self.visible_rows.clone();
        let rows = visible_rows.len();
        let task_path = path.clone();
        let (sender, receiver) = mpsc::channel();
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let result = export_xlsx_view(&task_path, &table, &visible_rows)
                .map_err(|error| error.to_string());
            let _ = sender.send(TaskOutcome::Exported { path, rows, result });
            repaint.request_repaint();
        });
        self.worker = Some(receiver);
        self.status = "Exportando resultados XLSX...".into();
    }

    fn export_groups(&mut self, ctx: &egui::Context) {
        if self.worker.is_some() {
            self.status = "Aguarde a operação de arquivo em andamento.".into();
            return;
        }
        if self.pending_cell_refresh {
            self.refresh();
        }
        let Some(spec) = self.project.aggregate else {
            return;
        };
        let Some(mut path) = rfd::FileDialog::new()
            .add_filter("Arquivo CSV", &["csv"])
            .set_file_name("agrupamento.csv")
            .save_file()
        else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension("csv");
        }
        let table = self.project.table.clone();
        let visible_rows = self.visible_rows.clone();
        let task_path = path.clone();
        let (sender, receiver) = mpsc::channel();
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let result = export_aggregate_csv(&task_path, &table, &visible_rows, &spec)
                .map_err(|error| error.to_string());
            let _ = sender.send(TaskOutcome::ExportedGroups { path, result });
            repaint.request_repaint();
        });
        self.worker = Some(receiver);
        self.status = "Exportando agrupamento CSV...".into();
    }

    fn poll_worker(&mut self, ctx: &egui::Context) {
        let result = match self.worker.as_ref().map(Receiver::try_recv) {
            Some(Ok(result)) => result,
            Some(Err(TryRecvError::Disconnected)) => {
                self.worker = None;
                self.status = "A operação de arquivo terminou inesperadamente.".into();
                return;
            }
            Some(Err(TryRecvError::Empty)) | None => return,
        };
        self.worker = None;
        match result {
            TaskOutcome::Imported {
                ticket,
                result: Ok(table),
            } if ticket == (self.document_id, self.revision) => {
                let count = table.row_count();
                self.replace_project(
                    Project {
                        table,
                        filter: None,
                        sort: None,
                        aggregate: None,
                        chart: None,
                        dashboard: DashboardSpec::default(),
                    },
                    None,
                );
                self.changed();
                self.status = format!("CSV importado: {count} linhas.");
            }
            TaskOutcome::Imported {
                ticket,
                result: Err(error),
            } if ticket == (self.document_id, self.revision) => {
                self.status = format!("Falha ao importar: {error}")
            }
            TaskOutcome::XlsxSheets {
                ticket,
                result: Ok((path, sheets)),
            } if ticket == (self.document_id, self.revision) => {
                self.pending_xlsx = Some(PendingXlsx {
                    path,
                    sheets,
                    ticket,
                });
                self.status = "Escolha a aba do XLSX para importar.".into();
            }
            TaskOutcome::XlsxSheets {
                ticket,
                result: Err(error),
            } if ticket == (self.document_id, self.revision) => {
                self.status = format!("Falha ao ler XLSX: {error}");
            }
            TaskOutcome::ImportedXlsx {
                ticket,
                result: Ok(imported),
            } if ticket == (self.document_id, self.revision) => {
                let count = imported.table.row_count();
                self.replace_project(
                    Project {
                        table: imported.table,
                        filter: None,
                        sort: None,
                        aggregate: None,
                        chart: None,
                        dashboard: DashboardSpec::default(),
                    },
                    None,
                );
                self.changed();
                self.status = format!("XLSX importado: {count} linhas. {}", imported.warning);
            }
            TaskOutcome::ImportedXlsx {
                ticket,
                result: Err(error),
            } if ticket == (self.document_id, self.revision) => {
                self.status = format!("Falha ao importar XLSX: {error}");
            }
            TaskOutcome::Opened {
                ticket,
                result: Ok((path, project)),
            } if ticket == (self.document_id, self.revision) => {
                self.replace_project(project, Some(path));
                self.status = "Projeto aberto com dados e visualização preservados.".into();
            }
            TaskOutcome::Opened {
                ticket,
                result: Err(error),
            } if ticket == (self.document_id, self.revision) => {
                self.status = format!("Falha ao abrir: {error}")
            }
            TaskOutcome::Imported { .. }
            | TaskOutcome::XlsxSheets { .. }
            | TaskOutcome::ImportedXlsx { .. }
            | TaskOutcome::Opened { .. } => {
                self.status =
                    "A importação/abertura foi descartada porque a tabela mudou durante a leitura."
                        .into();
            }
            TaskOutcome::Saved {
                path,
                document_id,
                revision,
                result: Ok(()),
            } => {
                if document_id == self.document_id {
                    self.path = Some(path);
                    if revision == self.revision {
                        self.saved_revision = revision;
                        self.status = "Projeto salvo.".into();
                        if self.close_after_save {
                            self.allow_close = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    } else {
                        self.status = "Projeto salvo, mas há alterações posteriores que ainda precisam ser salvas.".into();
                    }
                } else {
                    self.status = "O projeto anterior foi salvo; a nova tabela continua sem arquivo associado.".into();
                }
                self.close_after_save = false;
            }
            TaskOutcome::Saved {
                result: Err(error), ..
            } => {
                self.close_after_save = false;
                self.status = format!("Falha ao salvar: {error}");
            }
            TaskOutcome::Exported {
                path,
                rows,
                result: Ok(()),
            } => {
                self.status = format!("{rows} registros exportados para {}.", path.display());
            }
            TaskOutcome::Exported {
                result: Err(error), ..
            } => {
                self.status = format!("Falha ao exportar: {error}");
            }
            TaskOutcome::ExportedGroups {
                path,
                result: Ok(groups),
            } => {
                self.status = format!("{groups} grupos exportados para {}.", path.display());
            }
            TaskOutcome::ExportedGroups {
                result: Err(error), ..
            } => {
                self.status = format!("Falha ao exportar agrupamento: {error}");
            }
        }
    }

    fn interpret(&mut self) {
        if self.pending_cell_refresh {
            self.refresh();
        }
        self.preview = None;
        self.parse_error = None;
        match parse_command(&self.command, &self.project.table) {
            Ok(parsed) => {
                self.status = "Comando interpretado. Confira a prévia e confirme.".into();
                self.preview = Some(parsed);
            }
            Err(error) => {
                self.status = error.to_string();
                self.parse_error = Some(error);
            }
        }
    }

    fn apply_preview(&mut self) {
        let Some(parsed) = self.preview.take() else {
            return;
        };
        let validation = match &parsed {
            ParsedCommand::Filter(parsed) => {
                evaluate_filter(&self.project.table, &parsed.expr).map(|_| ())
            }
            ParsedCommand::Sort(parsed) => {
                sort_rows(&self.project.table, &[], &parsed.spec).map(|_| ())
            }
            ParsedCommand::Aggregate(parsed) => {
                aggregate_rows(&self.project.table, &self.visible_rows, &parsed.spec).map(|_| ())
            }
        };
        if let Err(error) = validation {
            self.status = format!("Prévia desatualizada: {error}. Interprete o comando novamente.");
            return;
        }
        self.push_view_history();
        let operation = match parsed {
            ParsedCommand::Filter(parsed) => {
                self.project.filter = Some(parsed.expr);
                "Filtro"
            }
            ParsedCommand::Sort(parsed) => {
                self.project.sort = Some(parsed.spec);
                "Ordenação"
            }
            ParsedCommand::Aggregate(parsed) => {
                self.project.aggregate = Some(parsed.spec);
                "Agrupamento"
            }
        };
        self.command_history.push(self.command.clone());
        self.changed();
        self.status = format!(
            "{operation} aplicado a {} linhas visíveis.",
            self.visible_rows.len()
        );
    }

    fn view_state(&self) -> ViewState {
        ViewState {
            filter: self.project.filter.clone(),
            sort: self.project.sort,
            aggregate: self.project.aggregate,
        }
    }

    fn push_view_history(&mut self) {
        self.undo.push(self.view_state());
        self.redo.clear();
    }

    fn set_sort(&mut self, sort: Option<SortSpec>) {
        if self.project.sort == sort {
            return;
        }
        self.push_view_history();
        self.project.sort = sort;
        self.changed();
        self.status = if sort.is_some() {
            "Ordenação aplicada aos resultados.".into()
        } else {
            "Ordenação removida.".into()
        };
    }

    fn clear_filter(&mut self) {
        if self.project.filter.is_none() {
            return;
        }
        self.push_view_history();
        self.project.filter = None;
        self.changed();
        self.status = "Filtro removido.".into();
    }

    fn clear_aggregate(&mut self) {
        if self.project.aggregate.is_none() {
            return;
        }
        self.push_view_history();
        self.project.aggregate = None;
        self.changed();
        self.status = "Agrupamento removido.".into();
    }

    fn undo_view(&mut self) {
        if let Some(previous) = self.undo.pop() {
            self.redo.push(self.view_state());
            self.project.filter = previous.filter;
            self.project.sort = previous.sort;
            self.project.aggregate = previous.aggregate;
            self.changed();
            self.status = "Operação desfeita.".into();
        }
    }

    fn redo_view(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(self.view_state());
            self.project.filter = next.filter;
            self.project.sort = next.sort;
            self.project.aggregate = next.aggregate;
            self.changed();
            self.status = "Operação refeita.".into();
        }
    }

    fn header(&mut self, root: &mut egui::Ui) {
        let ctx = root.ctx().clone();
        let colors = self.theme.palette();
        egui::Panel::top("top_bar")
            .exact_size(60.0)
            .frame(
                egui::Frame::new()
                    .fill(colors.surface)
                    .inner_margin(egui::Margin::symmetric(12, 6))
                    .stroke(Stroke::new(1.0, colors.border)),
            )
            .show(root, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("◆ JetSheets")
                            .size(21.0)
                            .strong()
                            .color(colors.accent_text),
                    );
                    ui.separator();
                    let title = if self.revision != self.saved_revision {
                        format!("{} •", self.project.table.name)
                    } else {
                        self.project.table.name.clone()
                    };
                    ui.label(RichText::new(title).size(15.0));
                    ui.separator();
                    ui.selectable_value(&mut self.active_view, MainView::Table, "Tabela");
                    ui.selectable_value(&mut self.active_view, MainView::Dashboard, "Dashboard");
                    ui.separator();
                    ui.menu_button("Arquivo", |ui| {
                        if ui.button("Nova tabela").clicked() {
                            self.request_replace(ReplaceAction::New, &ctx);
                            ui.close();
                        }
                        if ui.button("Importar CSV...").clicked() {
                            self.request_replace(ReplaceAction::Import, &ctx);
                            ui.close();
                        }
                        if ui.button("Importar XLSX...").clicked() {
                            self.request_replace(ReplaceAction::ImportXlsx, &ctx);
                            ui.close();
                        }
                        if ui.button("Abrir projeto...").clicked() {
                            self.request_replace(ReplaceAction::Open, &ctx);
                            ui.close();
                        }
                        if ui.button("Salvar").clicked() {
                            self.save_project(&ctx, false);
                            ui.close();
                        }
                        if ui.button("Salvar como...").clicked() {
                            self.save_project(&ctx, true);
                            ui.close();
                        }
                    });
                    if ui
                        .add_enabled(!self.undo.is_empty(), egui::Button::new("↶ Desfazer"))
                        .clicked()
                    {
                        self.undo_view();
                    }
                    if ui
                        .add_enabled(!self.redo.is_empty(), egui::Button::new("↷ Refazer"))
                        .clicked()
                    {
                        self.redo_view();
                    }
                    if theme::primary_button(ui, "Importar CSV", colors).clicked() {
                        self.request_replace(ReplaceAction::Import, &ctx);
                    }
                    if ui.button("Importar XLSX").clicked() {
                        self.request_replace(ReplaceAction::ImportXlsx, &ctx);
                    }
                    if ui.button("Salvar").clicked() {
                        self.save_project(&ctx, false);
                    }
                    if ui.button("Configurações").clicked() {
                        self.show_settings = true;
                    }
                });
            });
    }

    fn settings_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut open = self.show_settings;
        let mut chosen = self.theme;
        egui::Window::new("Configurações")
            .open(&mut open)
            .resizable(false)
            .default_width(320.0)
            .show(ctx, |ui| {
                theme::section_header(
                    ui,
                    "Aparência",
                    "Escolha o tema da interface.",
                    self.theme.palette(),
                );
                ui.add_space(theme::SPACE);
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut chosen, JetTheme::Dark, JetTheme::Dark.label());
                    ui.selectable_value(&mut chosen, JetTheme::Light, JetTheme::Light.label());
                });
                ui.add_space(theme::SPACE);
                ui.colored_label(self.theme.palette().success, "Tema aplicado nesta sessão.");
            });
        self.show_settings = open;
        if chosen != self.theme {
            self.theme = chosen;
            chosen.apply(ctx);
            ctx.request_repaint();
        }
    }

    fn command_panel(&mut self, root: &mut egui::Ui) {
        let colors = self.theme.palette();
        egui::Panel::right("command_panel")
            .default_size(320.0)
            .min_size(280.0)
            .resizable(true)
            .frame(
                egui::Frame::new()
                    .fill(colors.surface)
                    .inner_margin(egui::Margin::same(12))
                    .stroke(Stroke::new(1.0, colors.border)),
            )
            .show(root, |ui| {
                ui.add_space(10.0);
                ui.heading("Comandos em português");
                ui.label(
                    "Descreva um filtro, ordenação ou agrupamento usando os nomes das colunas.",
                );
                ui.add_space(8.0);
                let input = ui.add(
                    TextEdit::multiline(&mut self.command)
                        .hint_text("Filtre valor acima de 5000 ou ordene por valor decrescente")
                        .desired_rows(3),
                );
                if input.changed() {
                    self.preview = None;
                    self.parse_error = None;
                }
                if theme::primary_button(ui, "Interpretar comando", colors).clicked() {
                    self.interpret();
                }
                ui.separator();

                if let Some(preview) = &self.preview {
                    ui.label(
                        RichText::new("Prévia da operação")
                            .strong()
                            .color(colors.accent_text),
                    );
                    ui.label(preview.explanation());
                    let action = match preview {
                        ParsedCommand::Filter(_) => "Aplicar filtro",
                        ParsedCommand::Sort(_) => "Aplicar ordenação",
                        ParsedCommand::Aggregate(_) => "Criar agrupamento",
                    };
                    if theme::primary_button(ui, action, colors).clicked() {
                        self.apply_preview();
                    }
                }
                let mut chosen_column = None;
                if let Some(error) = &self.parse_error {
                    if matches!(error, ParseError::Ambiguous { .. }) {
                        ui.colored_label(colors.warning, error.to_string());
                    } else {
                        theme::error_message(ui, &error.to_string(), colors);
                    }
                    if let ParseError::Ambiguous { candidates, .. } = error {
                        for candidate in candidates {
                            if ui.button(format!("Usar coluna: {candidate}")).clicked() {
                                chosen_column = self
                                    .project
                                    .table
                                    .columns
                                    .iter()
                                    .position(|column| &column.name == candidate);
                            }
                        }
                    }
                }
                if let Some(column) = chosen_column {
                    match parse_command_with_override(
                        &self.command,
                        &self.project.table,
                        Some(column),
                    ) {
                        Ok(parsed) => {
                            self.preview = Some(parsed);
                            self.parse_error = None;
                            self.status =
                                "Escolha aplicada. Confira a prévia antes de confirmar.".into();
                        }
                        Err(error) => {
                            self.status = error.to_string();
                            self.parse_error = Some(error);
                        }
                    }
                }
                ui.add_space(8.0);
                ui.collapsing("Exemplos e limites", |ui| {
                    ui.label("• valor acima de R$ 5.000");
                    ui.label("• quantidade pelo menos 4 mil e valor até 10.000");
                    ui.label("• categoria contém frutas ou preço entre 100 e 500");
                    ui.label("• ordene por valor decrescente");
                    ui.label("• agrupe por categoria");
                    ui.label("• some valor por categoria");
                    ui.label("• calcule média de valor por categoria");
                    ui.label("Filtros usam E/OU. Comandos de gráfico ainda não estão disponíveis.");
                });
                ui.collapsing("Comandos aplicados nesta sessão", |ui| {
                    for command in self.command_history.iter().rev() {
                        ui.label(command);
                    }
                });
                ui.separator();
                ui.label(RichText::new("Registro selecionado").strong());
                if let Some(row) = self
                    .selected_row
                    .and_then(|index| self.project.table.rows.get(index).map(|row| (index, row)))
                {
                    ui.label(format!("Linha {}", row.0 + 1));
                    for (column, value) in self.project.table.columns.iter().zip(row.1) {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("{}:", column.name)).strong());
                            ui.label(if value.is_empty() { "—" } else { value });
                        });
                    }
                } else {
                    ui.label("Clique em uma célula para ver os detalhes.");
                }
            });
    }

    fn table_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Tabela");
            ui.label(format!("{} linhas · {} colunas", self.project.table.row_count(), self.project.table.column_count()));
            ui.separator();
            if ui.button("+ Linha").clicked() {
                if let Err(error) = self.project.table.add_row(Vec::new()) { self.status = error; }
                else { self.changed(); }
            }
            if ui.button("+ Coluna").clicked() {
                let mut number = self.project.table.column_count() + 1;
                while self.project.table.columns.iter().any(|column| column.name.eq_ignore_ascii_case(&format!("Coluna {number}"))) {
                    number += 1;
                }
                let name = format!("Coluna {number}");
                if let Err(error) = self.project.table.add_column(name) { self.status = error; }
                else { self.preview = None; self.parse_error = None; self.changed(); }
            }
            if ui.add_enabled(self.selected_row.is_some(), egui::Button::new("Excluir linha")).clicked()
                && let Some(row) = self.selected_row.take() {
                if let Err(error) = self.project.table.remove_row(row) { self.status = error; }
                else { self.changed(); }
            }
            if ui.add_enabled(self.selected_column.is_some() && self.project.table.column_count() > 1, egui::Button::new("Excluir coluna")).clicked()
                && let Some(column) = self.selected_column.take() {
                if let Err(error) = self.project.table.remove_column(column) { self.status = error; }
                else {
                    self.project.filter = None;
                    self.project.sort = None;
                    self.project.aggregate = None;
                    self.project.chart = None;
                    self.project.dashboard = DashboardSpec::default();
                    self.dashboard_pages.clear();
                    self.undo.clear();
                    self.redo.clear();
                    self.preview = None;
                    self.parse_error = None;
                    self.selection = None;
                    self.focused_cell_id = None;
                    self.changed();
                    self.status = "Coluna excluída; filtro, ordenação, agrupamento, gráfico e dashboard foram limpos para evitar referências inválidas.".into();
                }
            }
        });
    }

    fn handle_grid_clipboard(&mut self, ui: &mut egui::Ui) {
        let cell_has_focus = self
            .focused_cell_id
            .is_some_and(|id| ui.memory(|memory| memory.has_focus(id)));
        if !cell_has_focus {
            return;
        }
        let block_selected = self
            .selection
            .is_some_and(|selection| selection.cell_count() > 1);
        let action = ui.input_mut(|input| {
            let index = input.events.iter().position(|event| match event {
                egui::Event::Copy | egui::Event::Cut => block_selected,
                egui::Event::Key {
                    key: egui::Key::Delete,
                    pressed: true,
                    modifiers,
                    ..
                } => block_selected && !modifiers.command,
                egui::Event::Paste(text) => text.contains(['\t', '\r', '\n']),
                _ => false,
            })?;
            match input.events.remove(index) {
                egui::Event::Copy => Some(ClipboardAction::Copy),
                egui::Event::Cut => Some(ClipboardAction::Cut),
                egui::Event::Key {
                    key: egui::Key::Delete,
                    ..
                } => Some(ClipboardAction::Clear),
                egui::Event::Paste(text) => Some(ClipboardAction::Paste(text)),
                _ => None,
            }
        });
        let Some(selection) = self.selection else {
            return;
        };
        match action {
            Some(ClipboardAction::Copy | ClipboardAction::Cut) => {
                match selection.copy_tsv(&self.project.table, &self.visible_rows) {
                    Ok(text) => {
                        ui.ctx().copy_text(text);
                        if matches!(action, Some(ClipboardAction::Cut)) {
                            match selection.clear(&mut self.project.table, &self.visible_rows) {
                                Ok(count) => {
                                    self.changed();
                                    self.status = format!("{count} células recortadas.");
                                }
                                Err(error) => self.status = error,
                            }
                        } else {
                            self.status = format!("{} células copiadas.", selection.cell_count());
                        }
                    }
                    Err(error) => self.status = error,
                }
            }
            Some(ClipboardAction::Paste(text)) => {
                match selection.paste_tsv(&mut self.project.table, &self.visible_rows, &text) {
                    Ok(count) => {
                        self.changed();
                        self.status = format!("{count} células coladas.");
                    }
                    Err(error) => self.status = error,
                }
            }
            Some(ClipboardAction::Clear) => {
                match selection.clear(&mut self.project.table, &self.visible_rows) {
                    Ok(count) => {
                        self.changed();
                        self.status = format!("{count} células apagadas.");
                    }
                    Err(error) => self.status = error,
                }
            }
            None => {}
        }
    }

    fn table_grid(&mut self, ui: &mut egui::Ui, height: f32) {
        self.handle_grid_clipboard(ui);
        let colors = self.theme.palette();
        let (pointer_position, pointer_pressed, pointer_down, shift) = ui.input(|input| {
            let shift = input
                .events
                .iter()
                .rev()
                .find_map(|event| {
                    if let egui::Event::PointerButton {
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers,
                        ..
                    } = event
                    {
                        Some(modifiers.shift)
                    } else {
                        None
                    }
                })
                .unwrap_or(input.modifiers.shift);
            (
                input.pointer.hover_pos(),
                input.pointer.primary_pressed(),
                input.pointer.primary_down(),
                shift,
            )
        });
        if !pointer_down {
            self.drag_selecting = false;
        }
        let rows = self.visible_rows.clone();
        let widths = self.column_widths.clone();
        let table_width = ROW_NUMBER_WIDTH + widths.iter().sum::<f32>();
        let mut edits = Vec::new();
        let mut names = Vec::new();
        let mut resized = Vec::new();
        let mut cell_lost_focus = false;
        let mut cell_has_focus = false;
        let mut selected = None;
        let mut selected_column = None;
        let mut selected_cell = None;
        let mut focused_cell = None;

        ui.allocate_ui(vec2(ui.available_width(), height), |ui| {
            ScrollArea::horizontal()
                .id_salt("grid_horizontal")
                .show(ui, |ui| {
                    let (header, _) =
                        ui.allocate_exact_size(vec2(table_width, HEADER_HEIGHT), Sense::hover());
                    ui.painter().rect_filled(header, 0.0, colors.header);
                    ui.painter().text(
                        pos2(header.left() + 13.0, header.center().y),
                        Align2::LEFT_CENTER,
                        "#",
                        FontId::proportional(13.0),
                        colors.text_secondary,
                    );
                    let mut x = header.left() + ROW_NUMBER_WIDTH;
                    for (column, width) in widths.iter().enumerate() {
                        let cell =
                            Rect::from_min_size(pos2(x, header.top()), vec2(*width, HEADER_HEIGHT));
                        if cell.intersects(ui.clip_rect()) {
                            let mut name = self.project.table.columns[column].name.clone();
                            let response = ui.put(
                                cell.shrink2(vec2(8.0, 5.0)),
                                TextEdit::singleline(&mut name)
                                    .id_salt(("column_header", column))
                                    .frame(egui::Frame::NONE),
                            );
                            if response.changed() {
                                names.push((column, name));
                            }
                            if response.clicked() {
                                selected_column = Some(column);
                            }
                            let handle = Rect::from_min_max(
                                pos2(cell.right() - 4.0, cell.top()),
                                pos2(cell.right() + 4.0, cell.bottom()),
                            );
                            let drag = ui.interact(
                                handle,
                                ui.id().with(("resize", column)),
                                Sense::drag(),
                            );
                            if drag.dragged() {
                                resized.push((column, ui.input(|input| input.pointer.delta().x)));
                            }
                            ui.painter().line_segment(
                                [
                                    pos2(cell.right(), cell.top()),
                                    pos2(cell.right(), cell.bottom()),
                                ],
                                Stroke::new(1.0, colors.grid),
                            );
                        }
                        x += width;
                    }
                    ScrollArea::vertical()
                        .id_salt("grid_rows")
                        .max_height((height - HEADER_HEIGHT - 18.0).max(90.0))
                        .show_rows(ui, ROW_HEIGHT, rows.len(), |ui, range| {
                            for display_row in range {
                                let row = rows[display_row];
                                let (rect, _) = ui.allocate_exact_size(
                                    vec2(table_width, ROW_HEIGHT),
                                    Sense::hover(),
                                );
                                ui.painter().rect_filled(
                                    rect,
                                    0.0,
                                    if display_row % 2 == 0 {
                                        colors.row_even
                                    } else {
                                        colors.row_odd
                                    },
                                );
                                if self.selected_row == Some(row) {
                                    ui.painter().rect_stroke(
                                        rect,
                                        0.0,
                                        Stroke::new(1.0, colors.primary),
                                        StrokeKind::Inside,
                                    );
                                }
                                ui.painter().text(
                                    pos2(rect.left() + 11.0, rect.center().y),
                                    Align2::LEFT_CENTER,
                                    (row + 1).to_string(),
                                    FontId::proportional(12.0),
                                    colors.text_secondary,
                                );
                                let mut x = rect.left() + ROW_NUMBER_WIDTH;
                                for (column, width) in widths.iter().enumerate() {
                                    let cell = Rect::from_min_size(
                                        pos2(x, rect.top()),
                                        vec2(*width, ROW_HEIGHT),
                                    );
                                    if cell.intersects(ui.clip_rect()) {
                                        if self.selection.is_some_and(|selection| {
                                            selection.contains(display_row, column)
                                        }) {
                                            ui.painter().rect_filled(cell, 0.0, colors.selection);
                                        }
                                        let mut value =
                                            self.project.table.rows[row][column].clone();
                                        let response = ui.put(
                                            cell.shrink2(vec2(7.0, 3.0)),
                                            TextEdit::singleline(&mut value)
                                                .id_salt(("grid_cell", row, column))
                                                .frame(egui::Frame::NONE),
                                        );
                                        if response.changed() {
                                            edits.push((row, column, value));
                                        }
                                        if response.lost_focus() {
                                            cell_lost_focus = true;
                                        }
                                        if response.has_focus() {
                                            cell_has_focus = true;
                                            focused_cell =
                                                Some((response.id, display_row, column, row));
                                        }
                                        let pointer_in_cell = pointer_position
                                            .is_some_and(|position| cell.contains(position));
                                        if pointer_in_cell
                                            && (pointer_pressed
                                                || (self.drag_selecting && pointer_down))
                                        {
                                            selected = Some(row);
                                            selected_column = Some(column);
                                            selected_cell = Some((
                                                display_row,
                                                column,
                                                row,
                                                pointer_pressed && shift,
                                                pointer_pressed,
                                            ));
                                            if pointer_pressed {
                                                ui.memory_mut(|memory| {
                                                    memory.request_focus(response.id)
                                                });
                                                focused_cell =
                                                    Some((response.id, display_row, column, row));
                                            }
                                        }
                                        ui.painter().line_segment(
                                            [
                                                pos2(cell.right(), cell.top()),
                                                pos2(cell.right(), cell.bottom()),
                                            ],
                                            Stroke::new(1.0, colors.grid),
                                        );
                                    }
                                    x += width;
                                }
                            }
                        });
                });
        });
        for (column, delta) in resized {
            if let Some(width) = self.column_widths.get_mut(column) {
                *width = (*width + delta).clamp(80.0, 420.0);
            }
        }
        for (column, name) in names {
            if let Err(error) = self.project.table.set_column_name(column, name) {
                self.status = error;
            } else {
                self.preview = None;
                self.parse_error = None;
                self.changed();
            }
        }
        for (row, column, value) in edits {
            if let Err(error) = self.project.table.set_cell(row, column, value) {
                self.status = error;
            } else {
                self.revision = self.revision.wrapping_add(1);
                self.pending_cell_refresh = true;
            }
        }
        if self.pending_cell_refresh && (cell_lost_focus || !cell_has_focus) {
            self.refresh();
        }
        if let Some(row) = selected.filter(|row| self.visible_rows.contains(row)) {
            self.selected_row = Some(row);
        }
        if let Some(column) = selected_column {
            self.selected_column = Some(column);
        }
        if let Some((display_row, column, row, shift, pressed)) = selected_cell
            && self.visible_rows.get(display_row) == Some(&row)
        {
            if pressed {
                self.drag_selecting = true;
            }
            if shift || !pressed {
                if let Some(selection) = &mut self.selection {
                    selection.extend(display_row, column);
                } else {
                    self.selection = Some(GridSelection::single(display_row, column));
                }
            } else {
                self.selection = Some(GridSelection::single(display_row, column));
            }
        }
        if let Some((id, display_row, column, row)) = focused_cell {
            if selected_cell.is_none()
                && self.focused_cell_id != Some(id)
                && self.visible_rows.get(display_row) == Some(&row)
            {
                self.selection = Some(GridSelection::single(display_row, column));
                self.selected_row = Some(row);
                self.selected_column = Some(column);
            }
            self.focused_cell_id = Some(id);
        } else {
            self.focused_cell_id = None;
        }
        if let Some(selection) = self.selection
            && selection.cell_count() > 1
        {
            ui.label(format!("{} células selecionadas · Ctrl+C copia, Ctrl+X recorta, Ctrl+V cola e Delete apaga o intervalo.", selection.cell_count()));
        }
        if rows.is_empty() {
            ui.label(
                "Nenhuma linha visível. Adicione uma linha, importe um CSV ou limpe o filtro.",
            );
        }
    }

    fn results(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let colors = self.theme.palette();
        ui.horizontal(|ui| {
            theme::section_header(ui, "Resultados", "Resumo dos registros visíveis", colors);
            if ui
                .add_enabled(
                    self.project.filter.is_some(),
                    egui::Button::new("Limpar filtro"),
                )
                .clicked()
            {
                self.clear_filter();
            }
            if ui.button("Exportar CSV").clicked() {
                self.export_results(&ctx);
            }
            if ui.button("Exportar XLSX").clicked() {
                self.export_results_xlsx(&ctx);
            }
        });
        let mut selected_sort = self.project.sort;
        let sort_label = selected_sort
            .and_then(|spec| {
                self.project.table.columns.get(spec.column).map(|column| {
                    let direction = match spec.direction {
                        SortDirection::Ascending => "crescente",
                        SortDirection::Descending => "decrescente",
                    };
                    format!("{} · {direction}", column.name)
                })
            })
            .unwrap_or_else(|| "Ordem original".into());
        ui.horizontal(|ui| {
            ui.label("Ordenação");
            egui::ComboBox::from_id_salt("results_sort")
                .selected_text(sort_label)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut selected_sort, None, "Ordem original");
                    for (column, metadata) in self.project.table.columns.iter().enumerate() {
                        for (direction, label) in [
                            (SortDirection::Ascending, "crescente"),
                            (SortDirection::Descending, "decrescente"),
                        ] {
                            ui.selectable_value(
                                &mut selected_sort,
                                Some(SortSpec { column, direction }),
                                format!("{} · {label}", metadata.name),
                            );
                        }
                    }
                });
        });
        if selected_sort != self.project.sort {
            self.set_sort(selected_sort);
        }
        ui.horizontal_wrapped(|ui| {
            theme::metric(
                ui,
                "Registros analisados",
                &self.summary.original_count.to_string(),
                colors,
            );
            theme::metric(
                ui,
                "Encontrados",
                &self.summary.matched_count.to_string(),
                colors,
            );
            theme::metric(
                ui,
                "Seleção",
                &format!("{:.1}%", self.summary.percentage),
                colors,
            );
        });
        if let Some(expr) = &self.project.filter {
            ui.label(
                RichText::new(format!(
                    "Condições: {}",
                    describe_filter(expr, &self.project.table)
                ))
                .color(colors.text_secondary),
            );
        }
        if !self.summary.columns.is_empty() {
            ui.add_space(7.0);
            egui::Grid::new("stats_grid")
                .striped(true)
                .spacing([14.0, 5.0])
                .show(ui, |ui| {
                    ui.strong("Coluna");
                    ui.strong("Válidos");
                    ui.strong("Soma");
                    ui.strong("Média");
                    ui.strong("Mediana");
                    ui.strong("Mínimo");
                    ui.strong("Máximo");
                    ui.end_row();
                    for stat in &self.summary.columns {
                        let name = &self.project.table.columns[stat.column].name;
                        ui.label(name);
                        ui.label(stat.count.to_string());
                        ui.label(format_number(stat.sum));
                        ui.label(stat.mean.map(format_number).unwrap_or_else(|| "—".into()));
                        ui.label(stat.median.map(format_number).unwrap_or_else(|| "—".into()));
                        ui.label(stat.min.map(format_number).unwrap_or_else(|| "—".into()));
                        ui.label(stat.max.map(format_number).unwrap_or_else(|| "—".into()));
                        ui.end_row();
                    }
                });
        }
        if let Some(spec) = self.project.aggregate {
            ui.separator();
            ui.horizontal(|ui| {
                ui.strong("Agrupamento");
                if ui.button("Exportar agrupamento CSV").clicked() {
                    self.export_groups(&ctx);
                }
                if ui.button("Remover agrupamento").clicked() {
                    self.clear_aggregate();
                }
            });
            if let Some(data) = &self.aggregate_data {
                match data {
                    Ok(groups) => {
                        let group_name = &self.project.table.columns[spec.group_column].name;
                        ui.label(format!(
                            "{group_name} · {} grupos · {} linhas visíveis",
                            groups.len(),
                            self.visible_rows.len()
                        ));
                        let numeric = !matches!(spec.operation, AggregateOperation::Count);
                        let is_average =
                            matches!(spec.operation, AggregateOperation::Average { .. });
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [190.0, 23.0],
                                egui::Label::new(RichText::new("Grupo").strong()),
                            );
                            ui.add_sized(
                                [85.0, 23.0],
                                egui::Label::new(RichText::new("Registros").strong()),
                            );
                            if numeric {
                                ui.add_sized(
                                    [110.0, 23.0],
                                    egui::Label::new(RichText::new("Válidos").strong()),
                                );
                                ui.strong(if is_average { "Média" } else { "Soma" });
                            }
                        });
                        ScrollArea::vertical()
                            .id_salt("aggregate_rows")
                            .max_height(260.0)
                            .show_rows(ui, 26.0, groups.len(), |ui, range| {
                                for index in range {
                                    let group = &groups[index];
                                    ui.horizontal(|ui| {
                                        ui.add_sized([190.0, 23.0], egui::Label::new(&group.label))
                                            .on_hover_text(&group.label);
                                        ui.add_sized(
                                            [85.0, 23.0],
                                            egui::Label::new(group.count.to_string()),
                                        );
                                        if numeric {
                                            ui.add_sized(
                                                [110.0, 23.0],
                                                egui::Label::new(group.valid_values.to_string()),
                                            );
                                            ui.label(if is_average {
                                                group
                                                    .average()
                                                    .map(format_number)
                                                    .unwrap_or_else(|| "—".into())
                                            } else {
                                                format_number(group.total)
                                            });
                                        }
                                    });
                                }
                            });
                    }
                    Err(error) => theme::error_message(ui, error, colors),
                }
            }
        }
        ui.separator();
        theme::card(ui, colors, |ui| self.chart_section(ui));
    }

    fn chart_section(&mut self, ui: &mut egui::Ui) {
        theme::section_header(
            ui,
            "Gráfico",
            "Visualização dos dados filtrados",
            self.theme.palette(),
        );
        if self.project.table.column_count() < 2 {
            ui.label("Adicione pelo menos duas colunas para criar um gráfico.");
            return;
        }
        let numeric_y = self.project.table.columns.iter().position(|column| {
            matches!(
                column.kind,
                ColumnKind::Number | ColumnKind::Currency | ColumnKind::Percentage
            )
        });
        if numeric_y.is_none() && self.project.chart.is_none() {
            ui.label("Preencha uma coluna numérica para criar um gráfico.");
            return;
        }
        let default_y = numeric_y.unwrap_or(0);
        let mut spec = self
            .project
            .chart
            .clone()
            .or_else(|| self.chart_draft.clone())
            .unwrap_or(ChartSpec {
                kind: ChartKind::Bar,
                x_column: 0,
                y_column: default_y,
                title: "Visão dos dados filtrados".into(),
            });
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("Tipo");
            if ui
                .selectable_value(&mut spec.kind, ChartKind::Bar, "Barras")
                .changed()
            {
                changed = true;
            }
            if ui
                .selectable_value(&mut spec.kind, ChartKind::Line, "Linhas")
                .changed()
            {
                changed = true;
            }
            ui.separator();
            egui::ComboBox::from_id_salt("x_axis")
                .selected_text(format!(
                    "X: {}",
                    self.project.table.columns[spec.x_column].name
                ))
                .show_ui(ui, |ui| {
                    for (index, column) in self.project.table.columns.iter().enumerate() {
                        if ui
                            .selectable_value(&mut spec.x_column, index, &column.name)
                            .changed()
                        {
                            changed = true;
                        }
                    }
                });
            egui::ComboBox::from_id_salt("y_axis")
                .selected_text(format!(
                    "Y: {}",
                    self.project.table.columns[spec.y_column].name
                ))
                .show_ui(ui, |ui| {
                    for (index, column) in self.project.table.columns.iter().enumerate() {
                        if matches!(
                            column.kind,
                            ColumnKind::Number | ColumnKind::Currency | ColumnKind::Percentage
                        ) && ui
                            .selectable_value(&mut spec.y_column, index, &column.name)
                            .changed()
                        {
                            changed = true;
                        }
                    }
                });
        });
        ui.horizontal(|ui| {
            ui.label("Título");
            if ui
                .add(TextEdit::singleline(&mut spec.title).desired_width(290.0))
                .changed()
            {
                changed = true;
            }
        });
        if self.project.chart.is_none() {
            if changed {
                self.chart_draft = Some(spec.clone());
            }
            if theme::primary_button(ui, "Criar gráfico", self.theme.palette()).clicked() {
                self.project.chart = Some(spec.clone());
                self.chart_draft = None;
                self.changed();
            }
        } else if changed {
            self.project.chart = Some(spec.clone());
            self.changed();
        }
        if let (Some(spec), Some(chart_data)) = (&self.project.chart, &self.chart_data) {
            match chart_data {
                Ok(points) => {
                    let pages = points.len().div_ceil(48).max(1);
                    self.chart_page = self.chart_page.min(pages - 1);
                    if pages > 1 {
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(self.chart_page > 0, egui::Button::new("← Anterior"))
                                .clicked()
                            {
                                self.chart_page -= 1;
                            }
                            ui.label(format!("Página {} de {}", self.chart_page + 1, pages));
                            if ui
                                .add_enabled(
                                    self.chart_page + 1 < pages,
                                    egui::Button::new("Próxima →"),
                                )
                                .clicked()
                            {
                                self.chart_page += 1;
                            }
                        });
                    }
                    let start = self.chart_page * 48;
                    let end = (start + 48).min(points.len());
                    chart_view::render(ui, spec, &points[start..end], self.theme.palette());
                    if pages > 1 {
                        ui.label(format!("Pontos {}–{} de {}", start + 1, end, points.len()));
                    }
                }
                Err(error) => {
                    theme::error_message(ui, error, self.theme.palette());
                }
            };
        }
    }

    fn add_current_chart_to_dashboard(&mut self) {
        let Some(chart) = self.project.chart.clone() else {
            self.status = "Crie um gráfico na Tabela antes de adicioná-lo ao dashboard.".into();
            return;
        };
        if self.project.dashboard.charts.len() >= MAX_DASHBOARD_CHARTS {
            self.status = format!("O dashboard permite até {MAX_DASHBOARD_CHARTS} gráficos.");
            return;
        }
        self.project.dashboard.charts.push(chart);
        self.dashboard_pages.push(0);
        self.changed();
        self.status = "Gráfico adicionado ao dashboard. Salve o projeto para preservá-lo.".into();
    }

    fn dashboard_view(&mut self, ui: &mut egui::Ui) {
        let colors = self.theme.palette();
        theme::section_header(
            ui,
            "Dashboard",
            "Indicadores e gráficos calculados das linhas visíveis da tabela.",
            colors,
        );
        ui.add_space(theme::SPACE);
        theme::card(ui, colors, |ui| {
            ui.horizontal_wrapped(|ui| {
                theme::metric(
                    ui,
                    "Registros analisados",
                    &self.summary.original_count.to_string(),
                    colors,
                );
                theme::metric(
                    ui,
                    "Encontrados",
                    &self.summary.matched_count.to_string(),
                    colors,
                );
                theme::metric(
                    ui,
                    "Seleção",
                    &format!("{:.1}%", self.summary.percentage),
                    colors,
                );
            });
            if let Some(expr) = &self.project.filter {
                ui.label(format!(
                    "Filtro: {}",
                    describe_filter(expr, &self.project.table)
                ));
            } else {
                ui.label("Todos os registros estão incluídos.");
            }
            let mut metric_column = self.project.dashboard.metric_column;
            let fallback = self.summary.columns.first().map(|column| column.column);
            let selected = metric_column
                .filter(|column| {
                    self.summary
                        .columns
                        .iter()
                        .any(|stat| stat.column == *column)
                })
                .or(fallback);
            if let Some(column) = selected {
                ui.horizontal(|ui| {
                    ui.label("Métricas de");
                    egui::ComboBox::from_id_salt("dashboard_metric_column")
                        .selected_text(&self.project.table.columns[column].name)
                        .show_ui(ui, |ui| {
                            for stat in &self.summary.columns {
                                ui.selectable_value(
                                    &mut metric_column,
                                    Some(stat.column),
                                    &self.project.table.columns[stat.column].name,
                                );
                            }
                        });
                });
                if let Some(stat) = self
                    .summary
                    .columns
                    .iter()
                    .find(|stat| stat.column == column)
                {
                    ui.horizontal_wrapped(|ui| {
                        theme::metric(ui, "Soma", &format_number(stat.sum), colors);
                        theme::metric(
                            ui,
                            "Média",
                            &stat.mean.map(format_number).unwrap_or_else(|| "—".into()),
                            colors,
                        );
                        theme::metric(
                            ui,
                            "Mínimo",
                            &stat.min.map(format_number).unwrap_or_else(|| "—".into()),
                            colors,
                        );
                        theme::metric(
                            ui,
                            "Máximo",
                            &stat.max.map(format_number).unwrap_or_else(|| "—".into()),
                            colors,
                        );
                    });
                }
            } else {
                ui.label("Preencha uma coluna numérica para exibir soma, média e limites.");
            }
            if metric_column != self.project.dashboard.metric_column {
                self.project.dashboard.metric_column = metric_column;
                self.changed();
            }
        });
        ui.add_space(theme::SPACE);
        if let Some(groups) = &self.aggregate_data {
            theme::card(ui, colors, |ui| {
                theme::section_header(
                    ui,
                    "Agrupamento ativo",
                    "Resumo dos grupos calculado com o filtro atual.",
                    colors,
                );
                match groups {
                    Ok(groups) => {
                        let operation = self.project.aggregate.map(|spec| spec.operation);
                        for group in groups.iter().take(10) {
                            let value = match operation {
                                Some(AggregateOperation::Sum { .. }) => format!(
                                    "{} registros · soma {}",
                                    group.count,
                                    format_number(group.total)
                                ),
                                Some(AggregateOperation::Average { .. }) => format!(
                                    "{} registros · média {}",
                                    group.count,
                                    group
                                        .average()
                                        .map(format_number)
                                        .unwrap_or_else(|| "—".into())
                                ),
                                _ => format!("{} registros", group.count),
                            };
                            ui.horizontal(|ui| {
                                ui.add_sized([180.0, 24.0], egui::Label::new(&group.label));
                                ui.label(value);
                            });
                        }
                        if groups.len() > 10 {
                            ui.label(format!(
                                "Exibindo 10 de {} grupos. Veja todos na Tabela.",
                                groups.len()
                            ));
                        }
                        if groups.is_empty() {
                            ui.label("Nenhuma linha corresponde ao filtro atual.");
                        }
                    }
                    Err(error) => theme::error_message(ui, error, colors),
                }
            });
            ui.add_space(theme::SPACE);
        }
        ui.horizontal(|ui| {
            ui.heading("Gráficos salvos");
            if ui
                .add_enabled(
                    self.project.chart.is_some()
                        && self.project.dashboard.charts.len() < MAX_DASHBOARD_CHARTS,
                    egui::Button::new("Adicionar gráfico atual"),
                )
                .clicked()
            {
                self.add_current_chart_to_dashboard();
            }
        });
        if self.project.dashboard.charts.is_empty() {
            ui.label("Crie um gráfico na Tabela e use “Adicionar gráfico atual” para montar este dashboard.");
            if ui.button("Abrir Tabela").clicked() {
                self.active_view = MainView::Table;
            }
        }
        let mut remove = None;
        for index in 0..self.project.dashboard.charts.len() {
            ui.add_space(theme::SPACE);
            theme::card(ui, colors, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "Gráfico {} de {}",
                        index + 1,
                        self.project.dashboard.charts.len()
                    ));
                    if ui.button("Remover").clicked() {
                        remove = Some(index);
                    }
                });
                match &self.dashboard_data[index] {
                    Ok(points) => {
                        let pages = points.len().div_ceil(48).max(1);
                        let page = &mut self.dashboard_pages[index];
                        *page = (*page).min(pages - 1);
                        if pages > 1 {
                            ui.horizontal(|ui| {
                                if ui
                                    .add_enabled(*page > 0, egui::Button::new("← Anterior"))
                                    .clicked()
                                {
                                    *page -= 1;
                                }
                                ui.label(format!("Página {} de {}", *page + 1, pages));
                                if ui
                                    .add_enabled(*page + 1 < pages, egui::Button::new("Próxima →"))
                                    .clicked()
                                {
                                    *page += 1;
                                }
                            });
                        }
                        let start = *page * 48;
                        chart_view::render(
                            ui,
                            &self.project.dashboard.charts[index],
                            &points[start..(start + 48).min(points.len())],
                            colors,
                        );
                    }
                    Err(error) => theme::error_message(ui, error, colors),
                }
            });
        }
        if let Some(index) = remove {
            self.project.dashboard.charts.remove(index);
            self.dashboard_pages.remove(index);
            self.changed();
            self.status = "Gráfico removido do dashboard.".into();
        }
    }

    fn confirmation_dialog(&mut self, ctx: &egui::Context) {
        let Some(action) = self.confirm_replace else {
            return;
        };
        let mut decision = None;
        egui::Window::new("Alterações não salvas")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label("A tabela atual tem alterações não salvas.");
                ui.label("Continuar vai descartar essas alterações.");
                ui.horizontal(|ui| {
                    if ui.button("Cancelar").clicked() {
                        decision = Some(false);
                    }
                    if ui.button("Descartar e continuar").clicked() {
                        decision = Some(true);
                    }
                });
            });
        if let Some(continue_action) = decision {
            self.confirm_replace = None;
            if continue_action {
                self.perform_replace(action, ctx);
            }
        }
    }

    fn close_dialog(&mut self, ctx: &egui::Context) {
        if ctx.input(|input| input.viewport().close_requested()) && !self.allow_close {
            if self.worker.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.status = "Aguarde a operação de arquivo terminar antes de fechar.".into();
            } else if self.revision != self.saved_revision {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.confirm_close = true;
            }
        }
        if !self.confirm_close {
            return;
        }
        let mut decision = None;
        egui::Window::new("Fechar JetSheets")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label("Existem alterações não salvas nesta tabela.");
                ui.horizontal(|ui| {
                    if ui.button("Continuar editando").clicked() {
                        decision = Some(0);
                    }
                    if ui.button("Salvar").clicked() {
                        decision = Some(1);
                    }
                    if ui.button("Descartar e sair").clicked() {
                        decision = Some(2);
                    }
                });
            });
        match decision {
            Some(0) => self.confirm_close = false,
            Some(1) => {
                self.confirm_close = false;
                self.save_project(ctx, false);
                self.close_after_save = self.worker.is_some();
            }
            Some(2) => {
                self.confirm_close = false;
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            _ => {}
        }
    }
}

impl eframe::App for JetSheetsApp {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_worker(root.ctx());
        self.header(root);
        self.command_panel(root);
        let colors = self.theme.palette();
        egui::Panel::bottom("status_bar")
            .frame(
                egui::Frame::new()
                    .fill(colors.surface)
                    .inner_margin(egui::Margin::symmetric(12, 7))
                    .stroke(Stroke::new(1.0, colors.border)),
            )
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.label(&self.status);
                    if self.worker.is_some() {
                        ui.spinner();
                    }
                });
            });
        egui::CentralPanel::default().show(root, |ui| match self.active_view {
            MainView::Table => {
                self.table_toolbar(ui);
                ui.separator();
                let grid_height = (ui.available_height() * 0.47).clamp(180.0, 470.0);
                self.table_grid(ui, grid_height);
                ui.separator();
                ScrollArea::vertical()
                    .id_salt("results_scroll")
                    .show(ui, |ui| self.results(ui));
            }
            MainView::Dashboard => {
                ScrollArea::vertical()
                    .id_salt("dashboard_scroll")
                    .show(ui, |ui| self.dashboard_view(ui));
            }
        });
        self.confirmation_dialog(root.ctx());
        self.xlsx_sheet_dialog(root.ctx());
        self.close_dialog(root.ctx());
        self.settings_dialog(root.ctx());
    }
}

fn format_number(value: f64) -> String {
    let absolute = value.abs();
    let decimals = if absolute >= 100.0 { 2 } else { 3 };
    format!("{value:.decimals$}")
}

fn describe_filter(expr: &FilterExpr, table: &Table) -> String {
    match expr {
        FilterExpr::Condition(condition) => {
            let column = table
                .columns
                .get(condition.column)
                .map(|column| column.name.as_str())
                .unwrap_or("coluna inexistente");
            let operator = match condition.operator {
                Operator::Greater => "maior que",
                Operator::GreaterEqual => "maior ou igual a",
                Operator::Less => "menor que",
                Operator::LessEqual => "menor ou igual a",
                Operator::Equal => "igual a",
                Operator::NotEqual => "diferente de",
                Operator::Between => "entre",
                Operator::Contains => "contém",
                Operator::StartsWith => "começa com",
                Operator::EndsWith => "termina com",
            };
            let value = match &condition.value {
                FilterValue::Number(value) => format_number(*value),
                FilterValue::Text(value) => value.clone(),
                FilterValue::Range(a, b) => {
                    format!("{} e {}", format_number(*a), format_number(*b))
                }
            };
            format!("{column} {operator} {value}")
        }
        FilterExpr::And(parts) => parts
            .iter()
            .map(|part| describe_filter(part, table))
            .collect::<Vec<_>>()
            .join(" E "),
        FilterExpr::Or(parts) => parts
            .iter()
            .map(|part| describe_filter(part, table))
            .collect::<Vec<_>>()
            .join(" OU "),
        FilterExpr::Not(part) => format!("NÃO ({})", describe_filter(part, table)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xlsx_worker_replaces_table_and_reports_conversion_limit() {
        let ctx = egui::Context::default();
        let mut app = JetSheetsApp::new(&ctx);
        let mut table = Table::new("Vendas", vec!["Produto".into(), "Valor".into()]);
        table.add_row(vec!["Papel".into(), "15,75".into()]).unwrap();
        table.infer_column_types();
        let ticket = (app.document_id, app.revision);
        let (sender, receiver) = mpsc::channel();
        sender
            .send(TaskOutcome::ImportedXlsx {
                ticket,
                result: Ok(ImportedXlsx {
                    table,
                    warning: "Fórmulas não preservadas.".into(),
                }),
            })
            .unwrap();
        app.worker = Some(receiver);
        app.poll_worker(&ctx);
        assert_eq!(app.project.table.name, "Vendas");
        assert_eq!(app.visible_rows, vec![0]);
        assert!(app.status.contains("Fórmulas não preservadas"));
        assert!(app.revision != app.saved_revision);
    }

    #[test]
    fn grouping_updates_when_filter_changes_and_can_be_undone() {
        let ctx = egui::Context::default();
        let mut app = JetSheetsApp::new(&ctx);
        for row in [["A", "100", "1"], ["B", "200", "2"], ["A", "300", "3"]] {
            app.project
                .table
                .add_row(row.map(str::to_owned).to_vec())
                .unwrap();
        }
        app.refresh();
        app.command = "Some valor por produto".into();
        app.interpret();
        assert!(matches!(app.preview, Some(ParsedCommand::Aggregate(_))));
        app.apply_preview();
        let groups = app.aggregate_data.as_ref().unwrap().as_ref().unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].total, 400.0);
        app.command = "valor acima de 150".into();
        app.interpret();
        app.apply_preview();
        let groups = app.aggregate_data.as_ref().unwrap().as_ref().unwrap();
        assert_eq!(groups[0].label, "B");
        assert_eq!(groups[1].total, 300.0);
        app.undo_view();
        assert_eq!(
            app.aggregate_data.as_ref().unwrap().as_ref().unwrap()[0].total,
            400.0
        );
        app.undo_view();
        assert!(app.project.aggregate.is_none());
    }

    #[test]
    fn dashboard_cards_follow_filter_and_render_live_values() {
        egui::__run_test_ui(|ui| {
            let ctx = ui.ctx().clone();
            let mut app = JetSheetsApp::new(&ctx);
            for row in [["A", "100", "1"], ["B", "200", "2"]] {
                app.project
                    .table
                    .add_row(row.map(str::to_owned).to_vec())
                    .unwrap();
            }
            app.project.chart = Some(ChartSpec {
                kind: ChartKind::Bar,
                x_column: 0,
                y_column: 1,
                title: "Valor por produto".into(),
            });
            app.refresh();
            app.add_current_chart_to_dashboard();
            assert_eq!(app.dashboard_data[0].as_ref().unwrap().len(), 2);
            app.project.filter = Some(FilterExpr::Condition(jetsheets_core::Condition {
                column: 1,
                operator: Operator::Greater,
                value: FilterValue::Number(150.0),
            }));
            app.refresh();
            assert_eq!(app.summary.matched_count, 1);
            assert_eq!(app.dashboard_data[0].as_ref().unwrap()[0].label, "B");
            app.dashboard_view(ui);
        });
    }

    #[test]
    fn portuguese_sort_changes_view_and_can_be_undone() {
        let ctx = egui::Context::default();
        let mut app = JetSheetsApp::new(&ctx);
        for row in [["A", "100", "1"], ["B", "300", "2"], ["C", "200", "3"]] {
            app.project
                .table
                .add_row(row.map(str::to_owned).to_vec())
                .unwrap();
        }
        app.refresh();
        app.command = "Ordene do maior para o menor valor".into();
        app.interpret();
        assert!(matches!(app.preview, Some(ParsedCommand::Sort(_))));
        app.apply_preview();
        assert_eq!(app.visible_rows, vec![1, 2, 0]);
        app.undo_view();
        assert_eq!(app.visible_rows, vec![0, 1, 2]);
        app.redo_view();
        assert_eq!(app.visible_rows, vec![1, 2, 0]);
        assert_eq!(
            app.project.sort.unwrap().direction,
            SortDirection::Descending
        );
    }

    #[test]
    fn save_and_exit_closes_only_after_current_document_is_saved() {
        let ctx = egui::Context::default();
        let mut app = JetSheetsApp::new(&ctx);
        app.revision = 2;
        app.close_after_save = true;
        let (sender, receiver) = mpsc::channel();
        app.worker = Some(receiver);
        sender
            .send(TaskOutcome::Saved {
                path: PathBuf::from("test.jetsheet"),
                document_id: app.document_id,
                revision: app.revision,
                result: Ok(()),
            })
            .unwrap();
        app.poll_worker(&ctx);
        assert!(app.allow_close);
        assert_eq!(app.saved_revision, 2);
        assert!(!app.close_after_save);

        let mut app = JetSheetsApp::new(&ctx);
        app.revision = 3;
        app.close_after_save = true;
        let (sender, receiver) = mpsc::channel();
        app.worker = Some(receiver);
        sender
            .send(TaskOutcome::Saved {
                path: PathBuf::from("test.jetsheet"),
                document_id: app.document_id,
                revision: 2,
                result: Ok(()),
            })
            .unwrap();
        app.poll_worker(&ctx);
        assert!(!app.allow_close);
        assert_eq!(app.saved_revision, 0);
        assert!(!app.close_after_save);
    }

    #[test]
    fn interpreting_after_cell_edit_updates_column_type() {
        let ctx = egui::Context::default();
        let mut app = JetSheetsApp::new(&ctx);
        app.project
            .table
            .add_row(vec!["Produto".into(), "5000".into(), "2".into()])
            .unwrap();
        app.pending_cell_refresh = true;
        app.command = "valor acima de 4000".into();
        app.interpret();
        assert!(app.preview.is_some());
        assert_eq!(app.project.table.columns[1].kind, ColumnKind::Currency);
    }

    #[test]
    fn editor_and_results_render_from_a_filtered_table() {
        egui::__run_test_ui(|ui| {
            let ctx = ui.ctx().clone();
            let mut app = JetSheetsApp::new(&ctx);
            app.project
                .table
                .add_row(vec!["Notebook".into(), "6000".into(), "4500".into()])
                .unwrap();
            app.project
                .table
                .add_row(vec!["Mesa".into(), "3000".into(), "2000".into()])
                .unwrap();
            app.project.filter = Some(FilterExpr::Condition(jetsheets_core::Condition {
                column: 1,
                operator: Operator::Greater,
                value: FilterValue::Number(5000.0),
            }));
            app.project.chart = Some(ChartSpec {
                kind: ChartKind::Bar,
                x_column: 0,
                y_column: 1,
                title: "Valores filtrados".into(),
            });
            app.refresh();
            app.table_grid(ui, 250.0);
            app.results(ui);
            if let Some(chart) = &mut app.project.chart {
                chart.kind = ChartKind::Line;
            }
            app.refresh();
            app.chart_section(ui);
            assert_eq!(app.visible_rows, vec![0]);
            assert_eq!(app.summary.matched_count, 1);
        });
    }

    #[test]
    fn light_theme_renders_editor_and_chart() {
        egui::__run_test_ui(|ui| {
            let ctx = ui.ctx().clone();
            let mut app = JetSheetsApp::new(&ctx);
            app.theme = JetTheme::Light;
            app.theme.apply(&ctx);
            app.project
                .table
                .add_row(vec!["Notebook".into(), "6000".into(), "2".into()])
                .unwrap();
            app.project.chart = Some(ChartSpec {
                kind: ChartKind::Line,
                x_column: 0,
                y_column: 1,
                title: "Valores".into(),
            });
            app.refresh();
            app.table_grid(ui, 250.0);
            app.results(ui);
            assert_eq!(ctx.theme(), egui::Theme::Light);
            assert_eq!(app.chart_data.as_ref().unwrap().as_ref().unwrap().len(), 1);
        });
    }

    #[test]
    fn pointer_selects_cells_and_extends_with_shift() {
        let ctx = egui::Context::default();
        let mut app = JetSheetsApp::new(&ctx);
        app.project
            .table
            .add_row(vec!["A".into(), "1".into(), "x".into()])
            .unwrap();
        app.project
            .table
            .add_row(vec!["B".into(), "2".into(), "y".into()])
            .unwrap();
        app.refresh();
        fn frame(ctx: &egui::Context, app: &mut JetSheetsApp, events: Vec<egui::Event>) {
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 500.0))),
                    events,
                    ..Default::default()
                },
                |ui| app.table_grid(ui, 250.0),
            );
            output.drop_without_applying_deltas();
        }
        let press = |pos, shift| {
            let modifiers = egui::Modifiers {
                shift,
                ..Default::default()
            };
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers,
                },
            ]
        };
        let release = |pos, shift| {
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers {
                    shift,
                    ..Default::default()
                },
            }]
        };
        let first = pos2(90.0, 55.0);
        let last = pos2(250.0, 85.0);
        frame(&ctx, &mut app, press(first, false));
        assert_eq!(app.selection, Some(GridSelection::single(0, 0)));
        frame(&ctx, &mut app, release(first, false));
        frame(&ctx, &mut app, press(last, true));
        let mut expected = GridSelection::single(0, 0);
        expected.extend(1, 1);
        assert_eq!(app.selection, Some(expected));
        frame(&ctx, &mut app, release(last, true));
        frame(&ctx, &mut app, vec![egui::Event::Copy]);
        assert_eq!(app.status, "4 células copiadas.");
        frame(
            &ctx,
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::Delete,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::default(),
            }],
        );
        assert_eq!(app.status, "4 células apagadas.");
        assert_eq!(&app.project.table.rows[0][..2], &["", ""]);
        assert_eq!(&app.project.table.rows[1][..2], &["", ""]);

        frame(&ctx, &mut app, press(first, false));
        frame(&ctx, &mut app, vec![egui::Event::PointerMoved(last)]);
        assert_eq!(app.selection, Some(expected));
        frame(&ctx, &mut app, release(last, false));
    }
}
