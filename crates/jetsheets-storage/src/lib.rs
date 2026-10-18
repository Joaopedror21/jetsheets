//! Local CSV import and versioned JetSheets project files.

use jetsheets_charts::{ChartSpec, DashboardSpec};
use jetsheets_core::{
    AggregateOperation, AggregateSpec, FilterExpr, SortSpec, Table, aggregate_rows,
    evaluate_filter, sort_rows,
};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

mod xlsx;
pub use xlsx::{ImportedXlsx, export_xlsx_view, import_xlsx, list_xlsx_sheets};

const PROJECT_FORMAT: &str = "JetSheets";
const PROJECT_VERSION: u32 = 1;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub table: Table,
    pub filter: Option<FilterExpr>,
    #[serde(default)]
    pub sort: Option<SortSpec>,
    #[serde(default)]
    pub visible_limit: Option<usize>,
    #[serde(default)]
    pub aggregate: Option<AggregateSpec>,
    pub chart: Option<ChartSpec>,
    #[serde(default)]
    pub dashboard: DashboardSpec,
}

#[derive(Deserialize)]
struct ProjectFile {
    format: String,
    version: u32,
    project: Project,
}

#[derive(Serialize)]
struct ProjectFileRef<'a> {
    format: &'static str,
    version: u32,
    project: &'a Project,
}

#[derive(Debug)]
pub enum StorageError {
    Io(io::Error),
    Json(serde_json::Error),
    Csv(String),
    Xlsx(String),
    Project(String),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "Erro de arquivo: {error}"),
            Self::Json(error) => write!(f, "Arquivo de projeto inválido: {error}"),
            Self::Csv(message) | Self::Xlsx(message) | Self::Project(message) => message.fmt(f),
        }
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Csv(_) | Self::Xlsx(_) | Self::Project(_) => None,
        }
    }
}

impl From<io::Error> for StorageError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for StorageError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

/// Import UTF-8 CSV with a header row. Comma and semicolon separators are
/// detected from the header. Quoted fields, escaped quotes and multiline
/// fields follow RFC 4180; a UTF-8 BOM is accepted.
pub fn import_csv(path: impl AsRef<Path>) -> Result<Table, StorageError> {
    let path = path.as_ref();
    let text = fs::read_to_string(path)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let delimiter = detect_delimiter(text);
    let mut records = parse_csv_records(text, delimiter)?;
    if records.is_empty() {
        return Err(StorageError::Csv("O arquivo CSV está vazio.".into()));
    }

    let headers = unique_headers(records.remove(0));
    let name = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("Dados");
    let mut table = Table::new(name, headers);
    for (index, row) in records.into_iter().enumerate() {
        table.add_row(row).map_err(|message| {
            StorageError::Csv(format!("Linha {} do CSV: {message}", index + 2))
        })?;
    }
    table.infer_column_types();
    Ok(table)
}

/// Parse tab-separated clipboard data, including quoted tabs and multiline cells.
/// Unlike [`import_csv`], the first record is data rather than a header.
pub fn parse_tsv_records(text: &str) -> Result<Vec<Vec<String>>, StorageError> {
    parse_csv_records(text, '\t')
}

/// Export selected source rows in their current view order as UTF-8 CSV.
/// A BOM and semicolon separator make Brazilian numbers easier to open in Excel.
pub fn export_csv_view(
    path: impl AsRef<Path>,
    table: &Table,
    row_indices: &[usize],
) -> Result<(), StorageError> {
    let path = path.as_ref();
    let width = table.column_count();
    for &row_index in row_indices {
        let row = table.rows.get(row_index).ok_or_else(|| {
            StorageError::Csv(format!("A linha {} não existe mais.", row_index + 1))
        })?;
        if row.len() != width {
            return Err(StorageError::Csv(format!(
                "A linha {} tem {} células, mas a tabela tem {width} colunas.",
                row_index + 1,
                row.len()
            )));
        }
    }
    let (temp_path, mut temp) = create_sibling_temp(path)?;
    let write_result = (|| -> Result<(), StorageError> {
        temp.write_all(b"\xef\xbb\xbf")?;
        write_csv_record(
            &mut temp,
            table.columns.iter().map(|column| column.name.as_str()),
        )?;
        for &row_index in row_indices {
            write_csv_record(&mut temp, table.rows[row_index].iter().map(String::as_str))?;
        }
        temp.sync_all()?;
        Ok(())
    })();
    drop(temp);
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }
    if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(StorageError::Io(error));
    }
    Ok(())
}

/// Export grouped results as UTF-8 CSV in the same order displayed by the UI.
pub fn export_aggregate_csv(
    path: impl AsRef<Path>,
    table: &Table,
    row_indices: &[usize],
    spec: &AggregateSpec,
) -> Result<usize, StorageError> {
    let groups = aggregate_rows(table, row_indices, spec).map_err(StorageError::Csv)?;
    let path = path.as_ref();
    let (temp_path, mut temp) = create_sibling_temp(path)?;
    let write_result = (|| -> Result<(), StorageError> {
        temp.write_all(b"\xef\xbb\xbf")?;
        let mut header = vec![table.columns[spec.group_column].name.as_str(), "Registros"];
        if !matches!(spec.operation, AggregateOperation::Count) {
            header.extend([
                "Valores válidos",
                if matches!(spec.operation, AggregateOperation::Average { .. }) {
                    "Média"
                } else {
                    "Soma"
                },
            ]);
        }
        write_csv_record(&mut temp, header)?;
        for group in &groups {
            let mut values = vec![group.label.clone(), group.count.to_string()];
            if !matches!(spec.operation, AggregateOperation::Count) {
                values.push(group.valid_values.to_string());
                let result = if matches!(spec.operation, AggregateOperation::Average { .. }) {
                    group.average()
                } else {
                    Some(group.total)
                };
                values.push(
                    result
                        .map(|value| value.to_string().replace('.', ","))
                        .unwrap_or_default(),
                );
            }
            write_csv_record(&mut temp, values.iter().map(String::as_str))?;
        }
        temp.sync_all()?;
        Ok(())
    })();
    drop(temp);
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }
    if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(StorageError::Io(error));
    }
    Ok(groups.len())
}

fn write_csv_record<'a>(
    writer: &mut impl Write,
    values: impl IntoIterator<Item = &'a str>,
) -> io::Result<()> {
    for (index, value) in values.into_iter().enumerate() {
        if index > 0 {
            writer.write_all(b";")?;
        }
        if value.contains([';', '"', '\r', '\n']) {
            writer.write_all(b"\"")?;
            writer.write_all(value.replace('"', "\"\"").as_bytes())?;
            writer.write_all(b"\"")?;
        } else {
            writer.write_all(value.as_bytes())?;
        }
    }
    writer.write_all(b"\r\n")
}

/// Save a versioned `.jetsheet` file. The complete content is written and
/// flushed to a sibling temporary file before replacing the destination.
pub fn save_project(path: impl AsRef<Path>, project: &Project) -> Result<(), StorageError> {
    let path = path.as_ref();
    validate_project(project)?;

    let envelope = ProjectFileRef {
        format: PROJECT_FORMAT,
        version: PROJECT_VERSION,
        project,
    };

    let (temp_path, mut temp) = create_sibling_temp(path)?;
    let write_result = (|| -> Result<(), StorageError> {
        serde_json::to_writer_pretty(&mut temp, &envelope)?;
        temp.write_all(b"\n")?;
        temp.sync_all()?;
        Ok(())
    })();
    drop(temp);
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }

    if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(StorageError::Io(error));
    }
    Ok(())
}

/// Open a `.jetsheet` file, rejecting unknown formats, future versions and
/// inconsistent row/column references.
pub fn load_project(path: impl AsRef<Path>) -> Result<Project, StorageError> {
    let file = File::open(path)?;
    let envelope: ProjectFile = serde_json::from_reader(file)?;
    if envelope.format != PROJECT_FORMAT {
        return Err(StorageError::Project(
            "Este arquivo não é um projeto JetSheets.".into(),
        ));
    }
    if envelope.version != PROJECT_VERSION {
        return Err(StorageError::Project(format!(
            "Versão de projeto {} não suportada (versão atual: {}).",
            envelope.version, PROJECT_VERSION
        )));
    }
    validate_project(&envelope.project)?;
    Ok(envelope.project)
}

fn validate_project(project: &Project) -> Result<(), StorageError> {
    let width = project.table.columns.len();
    let mut column_names = std::collections::HashSet::new();
    for column in &project.table.columns {
        if column.name.trim().is_empty() || !column_names.insert(column.name.trim().to_lowercase())
        {
            return Err(StorageError::Project(
                "Projeto inválido: nomes de coluna vazios ou duplicados.".into(),
            ));
        }
    }
    for (index, row) in project.table.rows.iter().enumerate() {
        if row.len() != width {
            return Err(StorageError::Project(format!(
                "Projeto inválido: a linha {} tem {} células, mas a tabela tem {} colunas.",
                index + 1,
                row.len(),
                width
            )));
        }
    }
    if let Some(filter) = &project.filter {
        evaluate_filter(&project.table, filter)
            .map_err(|error| StorageError::Project(format!("Filtro salvo inválido: {error}")))?;
    }
    if let Some(sort) = &project.sort {
        sort_rows(&project.table, &[], sort)
            .map_err(|error| StorageError::Project(format!("Ordenação salva inválida: {error}")))?;
    }
    if project.visible_limit == Some(0)
        || project.visible_limit.is_some_and(|limit| limit > 1_000_000)
    {
        return Err(StorageError::Project(
            "Limite de registros salvo inválido.".into(),
        ));
    }
    if let Some(aggregate) = &project.aggregate {
        aggregate_rows(&project.table, &[], aggregate).map_err(|error| {
            StorageError::Project(format!("Agrupamento salvo inválido: {error}"))
        })?;
    }
    if let Some(spec) = &project.chart {
        spec.validate(&project.table)
            .map_err(StorageError::Project)?;
    }
    project
        .dashboard
        .validate(&project.table)
        .map_err(StorageError::Project)?;
    Ok(())
}

fn create_sibling_temp(path: &Path) -> Result<(PathBuf, File), StorageError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path.file_name().ok_or_else(|| {
        StorageError::Project("Escolha um nome de arquivo para salvar o projeto.".into())
    })?;
    let name = name.to_string_lossy();
    for _ in 0..32 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temp_path = parent.join(format!(".{name}.{}.{}.tmp", std::process::id(), sequence));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(file) => return Ok((temp_path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(StorageError::Io(error)),
        }
    }
    Err(StorageError::Project(
        "Não foi possível criar um arquivo temporário para salvar o projeto.".into(),
    ))
}

fn unique_headers(headers: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    headers
        .into_iter()
        .enumerate()
        .map(|(index, header)| {
            let base = if header.trim().is_empty() {
                format!("Coluna {}", index + 1)
            } else {
                header.trim().to_owned()
            };
            let mut name = base.clone();
            let mut suffix = 2;
            while !seen.insert(name.to_lowercase()) {
                name = format!("{base} ({suffix})");
                suffix += 1;
            }
            name
        })
        .collect()
}

fn detect_delimiter(text: &str) -> char {
    let mut comma = 0;
    let mut semicolon = 0;
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                chars.next();
            }
            '"' => in_quotes = !in_quotes,
            '\n' | '\r' if !in_quotes && (comma > 0 || semicolon > 0) => break,
            ',' if !in_quotes => comma += 1,
            ';' if !in_quotes => semicolon += 1,
            _ => {}
        }
    }
    if semicolon > comma { ';' } else { ',' }
}

#[derive(Clone, Copy)]
enum FieldState {
    Start,
    Unquoted,
    Quoted,
    AfterQuote,
}

fn parse_csv_records(text: &str, delimiter: char) -> Result<Vec<Vec<String>>, StorageError> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut state = FieldState::Start;
    let mut started = false;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        match state {
            FieldState::Quoted => {
                if ch == '"' {
                    state = FieldState::AfterQuote;
                } else {
                    field.push(ch);
                }
                continue;
            }
            FieldState::AfterQuote if ch == '"' => {
                field.push('"');
                state = FieldState::Quoted;
                continue;
            }
            _ => {}
        }

        if ch == delimiter {
            record.push(std::mem::take(&mut field));
            state = FieldState::Start;
            started = true;
        } else if ch == '\n' || ch == '\r' {
            if ch == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            finish_record(&mut records, &mut record, &mut field, &mut started);
            state = FieldState::Start;
        } else {
            match state {
                FieldState::Start if ch == '"' => {
                    state = FieldState::Quoted;
                    started = true;
                }
                FieldState::Start | FieldState::Unquoted => {
                    if ch == '"' {
                        return Err(StorageError::Csv(format!(
                            "Aspas inesperadas na linha {} do CSV.",
                            records.len() + 1
                        )));
                    }
                    field.push(ch);
                    state = FieldState::Unquoted;
                    started = true;
                }
                FieldState::AfterQuote if ch.is_ascii_whitespace() => {}
                FieldState::AfterQuote | FieldState::Quoted => {
                    return Err(StorageError::Csv(format!(
                        "Texto inesperado após aspas na linha {} do CSV.",
                        records.len() + 1
                    )));
                }
            }
        }
    }

    if matches!(state, FieldState::Quoted) {
        return Err(StorageError::Csv(
            "O CSV termina com aspas não fechadas.".into(),
        ));
    }
    finish_record(&mut records, &mut record, &mut field, &mut started);
    Ok(records)
}

fn finish_record(
    records: &mut Vec<Vec<String>>,
    record: &mut Vec<String>,
    field: &mut String,
    started: &mut bool,
) {
    if *started {
        record.push(std::mem::take(field));
        records.push(std::mem::take(record));
    }
    *started = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_path(extension: &str) -> PathBuf {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "jetsheets-storage-test-{}-{sequence}.{extension}",
            std::process::id()
        ))
    }

    #[test]
    fn imports_bom_semicolon_quotes_and_multiline() {
        let path = test_path("csv");
        fs::write(
            &path,
            "\u{feff}Produto;Valor;Nota\r\n\"Papel; A4\";\"1.234,50\";\"linha 1\r\nlinha 2\"\r\nCaneta;2;\"com \"\"aspas\"\"\"\r\n",
        )
        .unwrap();
        let table = import_csv(&path).unwrap();
        fs::remove_file(path).unwrap();
        assert_eq!(table.columns.len(), 3);
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0][0], "Papel; A4");
        assert_eq!(table.rows[0][1], "1.234,50");
        assert_eq!(table.rows[0][2], "linha 1\r\nlinha 2");
        assert_eq!(table.rows[1][2], "com \"aspas\"");
    }

    #[test]
    fn imports_comma_csv_and_fills_missing_cells() {
        let path = test_path("csv");
        fs::write(&path, "Produto,Valor\nA,10\nB\n").unwrap();
        let table = import_csv(&path).unwrap();
        fs::remove_file(path).unwrap();
        assert_eq!(table.rows[1], vec!["B", ""]);
    }

    #[test]
    fn exported_filtered_csv_roundtrips_quotes_and_row_order() {
        let path = test_path("csv");
        let mut table = Table::new("Teste", vec!["Produto".into(), "Nota; extra".into()]);
        table
            .add_row(vec!["A".into(), "linha 1\nlinha 2".into()])
            .unwrap();
        table
            .add_row(vec!["B".into(), "fora do filtro".into()])
            .unwrap();
        table
            .add_row(vec!["C".into(), "contém \"aspas\"".into()])
            .unwrap();
        export_csv_view(&path, &table, &[2, 0]).unwrap();
        let imported = import_csv(&path).unwrap();
        fs::remove_file(path).unwrap();
        assert_eq!(imported.columns[1].name, "Nota; extra");
        assert_eq!(
            imported.rows,
            vec![table.rows[2].clone(), table.rows[0].clone()]
        );
    }

    #[test]
    fn exports_grouped_sum_as_brazilian_csv() {
        let path = test_path("csv");
        let mut table = Table::new("Vendas", vec!["Categoria".into(), "Valor".into()]);
        for row in [["A", "1.000,50"], ["B", "20"], ["A", "2"]] {
            table.add_row(row.map(str::to_owned).to_vec()).unwrap();
        }
        table.infer_column_types();
        let spec = AggregateSpec {
            group_column: 0,
            operation: AggregateOperation::Sum { value_column: 1 },
        };
        assert_eq!(
            export_aggregate_csv(&path, &table, &[2, 0], &spec).unwrap(),
            1
        );
        let exported = import_csv(&path).unwrap();
        fs::remove_file(path).unwrap();
        assert_eq!(exported.rows, vec![vec!["A", "2", "2", "1002,5"]]);
    }

    #[test]
    fn exports_grouped_average_and_marks_groups_without_values() {
        let path = test_path("csv");
        let mut table = Table::new("Vendas", vec!["Categoria".into(), "Valor".into()]);
        for row in [["A", "10"], ["A", "20"], ["B", ""], ["A", ""]] {
            table.add_row(row.map(str::to_owned).to_vec()).unwrap();
        }
        table.infer_column_types();
        let spec = AggregateSpec {
            group_column: 0,
            operation: AggregateOperation::Average { value_column: 1 },
        };
        export_aggregate_csv(&path, &table, &[0, 1, 2, 3], &spec).unwrap();
        let exported = import_csv(&path).unwrap();
        fs::remove_file(path).unwrap();
        assert_eq!(exported.columns[3].name, "Média");
        assert_eq!(
            exported.rows,
            vec![vec!["A", "3", "2", "15"], vec!["B", "1", "0", ""]]
        );
    }

    #[test]
    fn rejects_unclosed_quotes() {
        assert!(parse_csv_records("A,B\n\"x,y", ',').is_err());
    }

    #[test]
    fn project_roundtrip_and_replace() {
        let path = test_path("jetsheet");
        let mut table = Table::new("Inventário", vec!["Produto".into(), "Valor".into()]);
        table.add_row(vec!["A".into(), "100".into()]).unwrap();
        let project = Project {
            table,
            filter: None,
            sort: Some(SortSpec {
                column: 1,
                direction: jetsheets_core::SortDirection::Descending,
            }),
            visible_limit: Some(1),
            aggregate: Some(AggregateSpec {
                group_column: 0,
                operation: jetsheets_core::AggregateOperation::Count,
            }),
            chart: Some(ChartSpec {
                kind: jetsheets_charts::ChartKind::Bar,
                x_column: 0,
                y_column: 1,
                title: "Valores".into(),
            }),
            dashboard: DashboardSpec {
                metric_column: Some(1),
                charts: vec![ChartSpec {
                    kind: jetsheets_charts::ChartKind::Line,
                    x_column: 0,
                    y_column: 1,
                    title: "Dashboard de valores".into(),
                }],
            },
        };
        save_project(&path, &project).unwrap();
        save_project(&path, &project).unwrap();
        let loaded = load_project(&path).unwrap();
        fs::remove_file(path).unwrap();
        assert_eq!(loaded.table.rows, project.table.rows);
        assert_eq!(loaded.sort, project.sort);
        assert_eq!(loaded.visible_limit, project.visible_limit);
        assert_eq!(loaded.aggregate, project.aggregate);
        assert_eq!(loaded.dashboard, project.dashboard);
        assert_eq!(loaded.chart.unwrap().title, "Valores");
    }

    #[test]
    fn opens_previous_project_without_sort_field() {
        let path = test_path("jetsheet");
        let project = Project {
            table: Table::blank(),
            filter: None,
            sort: None,
            visible_limit: None,
            aggregate: None,
            chart: None,
            dashboard: DashboardSpec::default(),
        };
        save_project(&path, &project).unwrap();
        let mut json: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        json["project"].as_object_mut().unwrap().remove("sort");
        json["project"]
            .as_object_mut()
            .unwrap()
            .remove("visible_limit");
        json["project"].as_object_mut().unwrap().remove("aggregate");
        json["project"].as_object_mut().unwrap().remove("dashboard");
        fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
        assert_eq!(load_project(&path).unwrap().sort, None);
        assert_eq!(load_project(&path).unwrap().visible_limit, None);
        assert_eq!(load_project(&path).unwrap().aggregate, None);
        assert_eq!(
            load_project(&path).unwrap().dashboard,
            DashboardSpec::default()
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_future_project_version() {
        let path = test_path("jetsheet");
        let project = Project {
            table: Table::blank(),
            filter: None,
            sort: None,
            visible_limit: None,
            aggregate: None,
            chart: None,
            dashboard: DashboardSpec::default(),
        };
        save_project(&path, &project).unwrap();
        let mut json: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        json["version"] = 999.into();
        fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
        assert!(
            load_project(&path)
                .unwrap_err()
                .to_string()
                .contains("não suportada")
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_filter_referencing_missing_column() {
        let path = test_path("jetsheet");
        let project = Project {
            table: Table::blank(),
            filter: Some(FilterExpr::Condition(jetsheets_core::Condition {
                column: 5,
                operator: jetsheets_core::Operator::Greater,
                value: jetsheets_core::FilterValue::Number(10.0),
            })),
            sort: None,
            visible_limit: None,
            aggregate: None,
            chart: None,
            dashboard: DashboardSpec::default(),
        };
        assert!(
            save_project(&path, &project)
                .unwrap_err()
                .to_string()
                .contains("Filtro salvo inválido")
        );
        assert!(!path.exists());
    }

    #[test]
    fn rejects_zero_visible_limit_in_saved_project() {
        let path = test_path("jetsheet");
        let project = Project {
            table: Table::blank(),
            filter: None,
            sort: None,
            visible_limit: Some(0),
            aggregate: None,
            chart: None,
            dashboard: DashboardSpec::default(),
        };
        assert!(
            save_project(&path, &project)
                .unwrap_err()
                .to_string()
                .contains("Limite")
        );
        assert!(!path.exists());
    }
}
