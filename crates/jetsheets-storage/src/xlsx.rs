//! Value-only XLSX interchange. The `.jetsheet` project remains the format for
//! preserving JetSheets views, charts, and dashboard configuration.

use calamine::{Data, Reader, Xlsx, open_workbook};
use jetsheets_core::{ColumnKind, Table, parse_br_number};
use rust_xlsxwriter::{Color, ExcelDateTime, Format, Workbook};
use std::fs;
use std::path::Path;

use super::{StorageError, create_sibling_temp, unique_headers};

#[derive(Debug)]
pub struct ImportedXlsx {
    pub table: Table,
    pub warning: String,
}

/// Return worksheets in workbook order so the user can choose one explicitly.
pub fn list_xlsx_sheets(path: impl AsRef<Path>) -> Result<Vec<String>, StorageError> {
    let workbook: Xlsx<_> = open_workbook(path.as_ref())
        .map_err(|error| StorageError::Xlsx(format!("Não foi possível abrir XLSX: {error}")))?;
    let names = workbook.sheet_names();
    if names.is_empty() {
        return Err(StorageError::Xlsx("O XLSX não contém abas.".into()));
    }
    Ok(names)
}

/// Import cell values from one worksheet. Formulas use Excel's cached values;
/// their expressions and workbook formatting are intentionally not retained.
pub fn import_xlsx(path: impl AsRef<Path>, sheet_name: &str) -> Result<ImportedXlsx, StorageError> {
    let mut workbook: Xlsx<_> = open_workbook(path.as_ref())
        .map_err(|error| StorageError::Xlsx(format!("Não foi possível abrir XLSX: {error}")))?;
    let sheets = workbook.sheet_names();
    if !sheets.iter().any(|name| name == sheet_name) {
        return Err(StorageError::Xlsx(format!(
            "A aba '{sheet_name}' não existe nesse XLSX."
        )));
    }
    let formulas = workbook
        .worksheet_formula(sheet_name)
        .map_err(|error| StorageError::Xlsx(format!("Falha ao ler fórmulas: {error}")))?;
    let formula_count = formulas
        .rows()
        .flat_map(|row| row.iter())
        .filter(|formula| !formula.is_empty())
        .count();
    let range = workbook
        .worksheet_range(sheet_name)
        .map_err(|error| StorageError::Xlsx(format!("Falha ao ler aba: {error}")))?;
    if range.is_empty() {
        return Err(StorageError::Xlsx(format!(
            "A aba '{sheet_name}' está vazia."
        )));
    }
    let mut rows = range.rows();
    let headers = rows
        .next()
        .ok_or_else(|| StorageError::Xlsx("A aba não contém cabeçalho.".into()))?
        .iter()
        .map(cell_text)
        .collect();
    let mut table = Table::new(sheet_name, unique_headers(headers));
    for (index, row) in rows.enumerate() {
        let values = row.iter().map(cell_text).collect();
        table
            .add_row(values)
            .map_err(|error| StorageError::Xlsx(format!("Linha {} da aba: {error}", index + 2)))?;
    }
    while table
        .rows
        .last()
        .is_some_and(|row| row.iter().all(|cell| cell.is_empty()))
    {
        table.rows.pop();
    }
    table.infer_column_types();

    let mut notices = vec![format!("Aba '{sheet_name}' importada como valores.")];
    if sheets.len() > 1 {
        notices.push(format!(
            "As outras {} abas não foram importadas.",
            sheets.len() - 1
        ));
    }
    if formula_count > 0 {
        let label = if formula_count == 1 {
            "fórmula foi convertida"
        } else {
            "fórmulas foram convertidas"
        };
        notices.push(format!(
            "{formula_count} {label} para o valor em cache do Excel; fórmulas sem cache podem aparecer vazias."
        ));
    }
    notices.push("Formatação, gráficos e macros do arquivo original não são preservados.".into());
    Ok(ImportedXlsx {
        table,
        warning: notices.join(" "),
    })
}

fn cell_text(value: &Data) -> String {
    match value {
        Data::Empty => String::new(),
        Data::Int(value) => value.to_string(),
        Data::Float(value) => value.to_string().replace('.', ","),
        Data::String(value) | Data::DateTimeIso(value) | Data::DurationIso(value) => value.clone(),
        Data::Bool(value) => value.to_string(),
        Data::Error(error) => format!("#ERRO: {error:?}"),
        Data::DateTime(value) if value.is_datetime() => {
            let (year, month, day, hour, minute, second, millisecond) = value.to_ymd_hms_milli();
            let date = format!("{year:04}-{month:02}-{day:02}");
            if hour == 0 && minute == 0 && second == 0 && millisecond == 0 {
                date
            } else {
                format!("{date} {hour:02}:{minute:02}:{second:02}")
            }
        }
        Data::DateTime(value) => value.to_string(),
    }
}

/// Export visible source rows to a new one-sheet XLSX in their display order.
/// The file is written beside the destination and replaced after completion.
pub fn export_xlsx_view(
    path: impl AsRef<Path>,
    table: &Table,
    row_indices: &[usize],
) -> Result<(), StorageError> {
    let width = table.column_count();
    if width > 16_384 || row_indices.len() >= 1_048_576 {
        return Err(StorageError::Xlsx(
            "A seleção excede o limite de linhas ou colunas do XLSX.".into(),
        ));
    }
    for &index in row_indices {
        let row = table
            .rows
            .get(index)
            .ok_or_else(|| StorageError::Xlsx(format!("A linha {} não existe mais.", index + 1)))?;
        if row.len() != width {
            return Err(StorageError::Xlsx(format!(
                "A linha {} tem {} células, mas a tabela tem {width} colunas.",
                index + 1,
                row.len()
            )));
        }
    }

    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet
        .set_name("Resultados")
        .map_err(|error| StorageError::Xlsx(error.to_string()))?;
    let header = Format::new()
        .set_bold()
        .set_font_color(Color::RGB(0xF1F5F9))
        .set_background_color(Color::RGB(0x1B2029));
    let currency = Format::new().set_num_format("R$ #,##0.00");
    let percentage = Format::new().set_num_format("0.00%");
    let date_format = Format::new().set_num_format("dd/mm/yyyy");
    for (column, metadata) in table.columns.iter().enumerate() {
        let column = column as u16;
        worksheet
            .write_string_with_format(0, column, &metadata.name, &header)
            .map_err(|error| StorageError::Xlsx(error.to_string()))?;
        worksheet
            .set_column_width(column, 19.0)
            .map_err(|error| StorageError::Xlsx(error.to_string()))?;
    }
    for (display_index, &source_index) in row_indices.iter().enumerate() {
        let row = &table.rows[source_index];
        let sheet_row = (display_index + 1) as u32;
        for (column, value) in row.iter().enumerate() {
            if value.is_empty() {
                continue;
            }
            let sheet_column = column as u16;
            let write_result = match table.columns[column].kind {
                ColumnKind::Number | ColumnKind::Currency | ColumnKind::Percentage => {
                    let number = parse_br_number(value).ok_or_else(|| {
                        StorageError::Xlsx(format!(
                            "Valor numérico inválido na linha {}, coluna '{}'.",
                            source_index + 1,
                            table.columns[column].name
                        ))
                    })?;
                    match table.columns[column].kind {
                        ColumnKind::Currency => worksheet.write_number_with_format(
                            sheet_row,
                            sheet_column,
                            number,
                            &currency,
                        ),
                        ColumnKind::Percentage => worksheet.write_number_with_format(
                            sheet_row,
                            sheet_column,
                            number / 100.0,
                            &percentage,
                        ),
                        _ => worksheet.write_number(sheet_row, sheet_column, number),
                    }
                }
                ColumnKind::Date => {
                    if let Some(date) = parse_excel_date(value) {
                        worksheet.write_datetime_with_format(
                            sheet_row,
                            sheet_column,
                            &date,
                            &date_format,
                        )
                    } else {
                        worksheet.write_string(sheet_row, sheet_column, value)
                    }
                }
                ColumnKind::Text => worksheet.write_string(sheet_row, sheet_column, value),
            };
            write_result.map_err(|error| StorageError::Xlsx(error.to_string()))?;
        }
    }

    let path = path.as_ref();
    let (temp_path, mut temp) = create_sibling_temp(path)?;
    let write_result = workbook
        .save_to_writer(&mut temp)
        .map_err(|error| StorageError::Xlsx(error.to_string()))
        .and_then(|()| temp.sync_all().map_err(StorageError::Io));
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

fn parse_excel_date(value: &str) -> Option<ExcelDateTime> {
    let parts: Vec<&str> = if value.contains('/') {
        value.split('/').collect()
    } else {
        value.split('-').collect()
    };
    if parts.len() != 3 {
        return None;
    }
    let date = if parts[0].len() == 4 {
        format!("{}-{}-{}", parts[0], parts[1], parts[2])
    } else if parts[2].len() == 4 {
        format!("{}-{}-{}", parts[2], parts[1], parts[0])
    } else {
        return None;
    };
    ExcelDateTime::parse_from_str(&date).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn test_path() -> std::path::PathBuf {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "jetsheets-xlsx-test-{}-{sequence}.xlsx",
            std::process::id()
        ))
    }

    #[test]
    fn chooses_one_sheet_and_reports_formula_loss() {
        let path = test_path();
        let mut workbook = Workbook::new();
        let first = workbook.add_worksheet();
        first.set_name("Resumo").unwrap();
        first.write_string(0, 0, "Nome").unwrap();
        first.write_string(1, 0, "Ignorar").unwrap();
        let second = workbook.add_worksheet();
        second.set_name("Vendas").unwrap();
        second.write_string(0, 0, "Produto").unwrap();
        second.write_string(0, 1, "Valor").unwrap();
        second.write_string(0, 2, "Cálculo").unwrap();
        second.write_string(1, 0, "Caderno").unwrap();
        second.write_number(1, 1, 15.75).unwrap();
        second.write_formula(1, 2, "=B2*2").unwrap();
        workbook.save(&path).unwrap();

        assert_eq!(list_xlsx_sheets(&path).unwrap(), ["Resumo", "Vendas"]);
        let imported = import_xlsx(&path, "Vendas").unwrap();
        assert_eq!(imported.table.name, "Vendas");
        assert_eq!(imported.table.rows.len(), 1);
        assert_eq!(imported.table.rows[0][0], "Caderno");
        assert_eq!(imported.table.rows[0][1], "15,75");
        assert_eq!(imported.table.columns[1].kind, ColumnKind::Currency);
        assert!(imported.warning.contains("outras 1 abas"));
        assert!(imported.warning.contains("1 fórmula"));
        assert!(import_xlsx(&path, "Inexistente").is_err());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn exports_visible_order_with_typed_numbers_and_literal_text() {
        let path = test_path();
        let mut table = Table::new(
            "Vendas",
            vec![
                "Produto".into(),
                "Valor".into(),
                "Desconto".into(),
                "Data".into(),
            ],
        );
        table
            .add_row(vec![
                "A".into(),
                "10,50".into(),
                "5%".into(),
                "17/10/2026".into(),
            ])
            .unwrap();
        table
            .add_row(vec![
                "=SUM(1,2)".into(),
                "20".into(),
                "10%".into(),
                "2026-10-18".into(),
            ])
            .unwrap();
        table.infer_column_types();
        export_xlsx_view(&path, &table, &[1, 0]).unwrap();
        let mut workbook: Xlsx<_> = open_workbook(&path).unwrap();
        let range = workbook.worksheet_range("Resultados").unwrap();
        assert_eq!(
            range.get_value((1, 0)),
            Some(&Data::String("=SUM(1,2)".into()))
        );
        assert_eq!(range.get_value((1, 1)), Some(&Data::Float(20.0)));
        assert_eq!(range.get_value((1, 2)), Some(&Data::Float(0.1)));
        assert!(matches!(range.get_value((1, 3)), Some(Data::DateTime(_))));
        assert_eq!(range.get_value((2, 0)), Some(&Data::String("A".into())));
        assert_eq!(range.get_value((2, 1)), Some(&Data::Float(10.5)));
        drop(workbook);
        fs::remove_file(path).unwrap();
    }
}
