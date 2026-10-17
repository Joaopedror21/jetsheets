//! Rectangular selection in the currently visible table rows.

use jetsheets_core::Table;
use jetsheets_storage::parse_tsv_records;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridSelection {
    pub anchor: (usize, usize),
    pub focus: (usize, usize),
}

impl GridSelection {
    pub fn single(display_row: usize, column: usize) -> Self {
        Self {
            anchor: (display_row, column),
            focus: (display_row, column),
        }
    }

    pub fn extend(&mut self, display_row: usize, column: usize) {
        self.focus = (display_row, column);
    }

    pub fn bounds(&self) -> (usize, usize, usize, usize) {
        (
            self.anchor.0.min(self.focus.0),
            self.anchor.0.max(self.focus.0),
            self.anchor.1.min(self.focus.1),
            self.anchor.1.max(self.focus.1),
        )
    }

    pub fn contains(&self, display_row: usize, column: usize) -> bool {
        let (top, bottom, left, right) = self.bounds();
        (top..=bottom).contains(&display_row) && (left..=right).contains(&column)
    }

    pub fn cell_count(&self) -> usize {
        let (top, bottom, left, right) = self.bounds();
        (bottom - top + 1) * (right - left + 1)
    }

    /// Serialize selected visible cells as TSV suitable for spreadsheet clipboards.
    pub fn copy_tsv(&self, table: &Table, visible_rows: &[usize]) -> Result<String, String> {
        let (top, bottom, left, right) = self.bounds();
        if bottom >= visible_rows.len() || right >= table.column_count() {
            return Err("A seleção não corresponde mais à tabela visível.".into());
        }
        let mut records = Vec::with_capacity(bottom - top + 1);
        for &source_row in &visible_rows[top..=bottom] {
            let row = table
                .rows
                .get(source_row)
                .ok_or_else(|| "Uma linha selecionada não existe mais.".to_owned())?;
            records.push(
                (left..=right)
                    .map(|column| {
                        row.get(column)
                            .map(|value| quote_tsv(value))
                            .ok_or_else(|| "Uma célula selecionada não existe mais.".to_owned())
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .join("\t"),
            );
        }
        Ok(records.join("\r\n"))
    }

    /// Paste at the upper-left selected cell, using the current visible row order.
    /// Bounds are checked before any cell is changed.
    pub fn paste_tsv(
        &self,
        table: &mut Table,
        visible_rows: &[usize],
        text: &str,
    ) -> Result<usize, String> {
        let records = parse_tsv_records(text).map_err(|error| error.to_string())?;
        if records.is_empty() {
            return Err("A área de transferência está vazia.".into());
        }
        let (top, _, left, _) = self.bounds();
        let max_width = records.iter().map(Vec::len).max().unwrap_or(0);
        if top + records.len() > visible_rows.len() || left + max_width > table.column_count() {
            return Err(
                "O bloco não cabe nas linhas e colunas visíveis. Adicione espaço antes de colar."
                    .into(),
            );
        }
        if visible_rows[top..top + records.len()]
            .iter()
            .any(|&row| row >= table.row_count())
        {
            return Err("Uma linha de destino não existe mais.".into());
        }
        let mut changed = 0;
        for (row_offset, record) in records.into_iter().enumerate() {
            let source_row = visible_rows[top + row_offset];
            for (column_offset, value) in record.into_iter().enumerate() {
                table.set_cell(source_row, left + column_offset, value)?;
                changed += 1;
            }
        }
        Ok(changed)
    }

    /// Clear the selected cells without changing the shape of the source table.
    pub fn clear(&self, table: &mut Table, visible_rows: &[usize]) -> Result<usize, String> {
        let (top, bottom, left, right) = self.bounds();
        if bottom >= visible_rows.len() || right >= table.column_count() {
            return Err("A seleção não corresponde mais à tabela visível.".into());
        }
        if visible_rows[top..=bottom]
            .iter()
            .any(|&row| row >= table.row_count())
        {
            return Err("Uma linha selecionada não existe mais.".into());
        }
        for &source_row in &visible_rows[top..=bottom] {
            for column in left..=right {
                table.set_cell(source_row, column, String::new())?;
            }
        }
        Ok(self.cell_count())
    }
}

fn quote_tsv(value: &str) -> String {
    if value.contains(['\t', '\r', '\n', '"']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_and_pastes_a_visible_rectangle_with_quoted_cells() {
        let mut table = Table::new("Teste", vec!["A".into(), "B".into(), "C".into()]);
        table
            .add_row(vec!["fora".into(), "".into(), "".into()])
            .unwrap();
        table
            .add_row(vec!["um".into(), "a\tb".into(), "".into()])
            .unwrap();
        table
            .add_row(vec!["dois".into(), "c\"d".into(), "".into()])
            .unwrap();
        table
            .add_row(vec!["".into(), "".into(), "".into()])
            .unwrap();
        let visible = vec![1, 2, 3];
        let mut selection = GridSelection::single(0, 0);
        selection.extend(1, 1);
        assert!(selection.contains(1, 1));
        assert!(!selection.contains(2, 1));
        let copied = selection.copy_tsv(&table, &visible).unwrap();
        assert_eq!(copied, "um\t\"a\tb\"\r\ndois\t\"c\"\"d\"");

        let target = GridSelection::single(1, 2);
        assert!(target.paste_tsv(&mut table, &visible, &copied).is_err());
        assert_eq!(table.rows[2][1], "c\"d");
        let target = GridSelection::single(0, 1);
        assert_eq!(target.paste_tsv(&mut table, &visible, &copied).unwrap(), 4);
        assert_eq!(table.rows[1][1], "um");
        assert_eq!(table.rows[1][2], "a\tb");
        assert_eq!(table.rows[2][2], "c\"d");
        assert_eq!(table.rows[0][0], "fora");
    }

    #[test]
    fn cuts_only_selected_visible_cells() {
        let mut table = Table::new("Teste", vec!["A".into(), "B".into()]);
        table.add_row(vec!["oculta".into(), "1".into()]).unwrap();
        table.add_row(vec!["visível".into(), "2".into()]).unwrap();
        let selection = GridSelection::single(0, 0);
        assert_eq!(selection.clear(&mut table, &[1]).unwrap(), 1);
        assert_eq!(table.rows[0][0], "oculta");
        assert_eq!(table.rows[1][0], "");
    }
}
