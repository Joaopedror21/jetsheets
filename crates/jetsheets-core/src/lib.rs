//! UI-independent table model, deterministic filtering, and result statistics.
//!
//! Rows contain the original cell text. A filtered view is represented by row
//! indices, so applying or undoing a filter never removes source data.

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashSet;

/// A named table with a rectangular set of rows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Table {
    pub name: String,
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<String>>,
}

/// Metadata for a table column. Cell text remains in [`Table::rows`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub kind: ColumnKind,
}

/// The type inferred from nonempty cells in a column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnKind {
    Text,
    Number,
    Currency,
    Percentage,
    Date,
}

impl Table {
    /// Creates an empty table with the supplied column names.
    pub fn new(name: impl Into<String>, columns: Vec<String>) -> Self {
        Self {
            name: name.into(),
            columns: columns
                .into_iter()
                .enumerate()
                .map(|(index, name)| Column {
                    name: if name.trim().is_empty() {
                        format!("Coluna {}", index + 1)
                    } else {
                        name
                    },
                    kind: ColumnKind::Text,
                })
                .collect(),
            rows: Vec::new(),
        }
    }

    /// Creates an editable, empty table with one column.
    pub fn blank() -> Self {
        Self::new("Nova tabela", vec!["Coluna 1".to_owned()])
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    /// Appends a row, padding missing trailing cells. Extra cells are rejected.
    pub fn add_row(&mut self, mut values: Vec<String>) -> Result<(), String> {
        if values.len() > self.column_count() {
            return Err(format!(
                "A linha tem {} células, mas a tabela tem {} colunas.",
                values.len(),
                self.column_count()
            ));
        }
        values.resize(self.column_count(), String::new());
        self.rows.push(values);
        Ok(())
    }

    /// Appends a column and extends every existing row with an empty cell.
    pub fn add_column(&mut self, name: String) -> Result<(), String> {
        if name.trim().is_empty() {
            return Err("O nome da coluna não pode estar vazio.".to_owned());
        }
        if self
            .columns
            .iter()
            .any(|column| column.name.trim().eq_ignore_ascii_case(name.trim()))
        {
            return Err(format!("Já existe uma coluna chamada '{name}'."));
        }
        self.columns.push(Column {
            name,
            kind: ColumnKind::Text,
        });
        for row in &mut self.rows {
            row.push(String::new());
        }
        Ok(())
    }

    pub fn remove_row(&mut self, row: usize) -> Result<(), String> {
        if row >= self.row_count() {
            return Err(format!("Linha {row} inexistente."));
        }
        self.rows.remove(row);
        Ok(())
    }

    pub fn remove_column(&mut self, column: usize) -> Result<(), String> {
        if column >= self.column_count() {
            return Err(format!("Coluna {column} inexistente."));
        }
        self.columns.remove(column);
        for row in &mut self.rows {
            if column < row.len() {
                row.remove(column);
            }
        }
        Ok(())
    }

    /// Changes one cell without changing other rows or the current view.
    pub fn set_cell(&mut self, row: usize, column: usize, value: String) -> Result<(), String> {
        if column >= self.column_count() {
            return Err(format!("Coluna {column} inexistente."));
        }
        let row_ref = self
            .rows
            .get_mut(row)
            .ok_or_else(|| format!("Linha {row} inexistente."))?;
        let cell = row_ref
            .get_mut(column)
            .ok_or_else(|| format!("Célula ({row}, {column}) inexistente."))?;
        *cell = value;
        Ok(())
    }

    pub fn set_column_name(&mut self, column: usize, name: String) -> Result<(), String> {
        if name.trim().is_empty() {
            return Err("O nome da coluna não pode estar vazio.".to_owned());
        }
        if self.columns.iter().enumerate().any(|(index, existing)| {
            index != column && existing.name.trim().eq_ignore_ascii_case(name.trim())
        }) {
            return Err(format!("Já existe uma coluna chamada '{name}'."));
        }
        let target = self
            .columns
            .get_mut(column)
            .ok_or_else(|| format!("Coluna {column} inexistente."))?;
        target.name = name;
        Ok(())
    }

    /// Recomputes column types from nonempty cells. A mixed column is text.
    /// Empty columns remain text. Call this after importing or editing values.
    pub fn infer_column_types(&mut self) {
        for column in 0..self.column_count() {
            let values: Vec<&str> = self
                .rows
                .iter()
                .filter_map(|row| row.get(column).map(String::as_str))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .collect();
            let kind = if values.is_empty() {
                ColumnKind::Text
            } else if values.iter().all(|value| parse_date(value).is_some()) {
                ColumnKind::Date
            } else if values.iter().all(|value| parse_br_number(value).is_some()) {
                if values.iter().all(|value| value.ends_with('%')) {
                    ColumnKind::Percentage
                } else if values.iter().any(|value| value.starts_with("R$"))
                    || looks_like_currency(&self.columns[column].name)
                {
                    ColumnKind::Currency
                } else {
                    ColumnKind::Number
                }
            } else {
                ColumnKind::Text
            };
            self.columns[column].kind = kind;
        }
    }
}

fn looks_like_currency(name: &str) -> bool {
    let lower = fold_text(name);
    ["valor", "preco", "receita", "custo", "faturamento", "lucro"]
        .iter()
        .any(|word| lower.contains(word))
}

fn parse_date(value: &str) -> Option<(u32, u32, u32)> {
    let parts: Vec<&str> = if value.contains('/') {
        value.split('/').collect()
    } else if value.contains('-') {
        value.split('-').collect()
    } else {
        return None;
    };
    if parts.len() != 3
        || !parts
            .iter()
            .all(|part| part.chars().all(|ch| ch.is_ascii_digit()))
    {
        return None;
    }
    let (year, month, day) = if parts[0].len() == 4 {
        (parts[0], parts[1], parts[2])
    } else if parts[2].len() == 4 {
        (parts[2], parts[1], parts[0])
    } else {
        return None;
    };
    let year = year.parse::<u32>().ok()?;
    let month = month.parse::<u32>().ok()?;
    let day = day.parse::<u32>().ok()?;
    if year == 0 || !(1..=12).contains(&month) {
        return None;
    }
    let days = match month {
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => 31,
    };
    (1..=days).contains(&day).then_some((year, month, day))
}

/// Sorts visible source row indices without changing the table itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SortSpec {
    pub column: usize,
    pub direction: SortDirection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortDirection {
    Ascending,
    Descending,
}

pub fn sort_rows(
    table: &Table,
    row_indices: &[usize],
    spec: &SortSpec,
) -> Result<Vec<usize>, String> {
    let column = table
        .columns
        .get(spec.column)
        .ok_or_else(|| format!("Coluna {} inexistente na ordenação.", spec.column))?;
    if let Some(&row) = row_indices.iter().find(|&&row| row >= table.row_count()) {
        return Err(format!("Linha {row} inexistente na ordenação."));
    }
    if let Some(&row) = row_indices
        .iter()
        .find(|&&row| table.rows[row].get(spec.column).is_none())
    {
        return Err(format!("Linha {row} incompleta na ordenação."));
    }
    let mut sorted = row_indices.to_vec();
    sorted.sort_by(|&left, &right| {
        let left_value = table.rows[left][spec.column].trim();
        let right_value = table.rows[right][spec.column].trim();
        match column.kind {
            ColumnKind::Number | ColumnKind::Currency | ColumnKind::Percentage => compare_optional(
                parse_br_number(left_value),
                parse_br_number(right_value),
                spec.direction,
                f64::total_cmp,
            ),
            ColumnKind::Date => compare_optional(
                parse_date(left_value),
                parse_date(right_value),
                spec.direction,
                Ord::cmp,
            ),
            ColumnKind::Text => compare_optional(
                (!left_value.is_empty()).then(|| fold_text(left_value)),
                (!right_value.is_empty()).then(|| fold_text(right_value)),
                spec.direction,
                Ord::cmp,
            ),
        }
    });
    Ok(sorted)
}

fn compare_optional<T>(
    left: Option<T>,
    right: Option<T>,
    direction: SortDirection,
    compare: impl FnOnce(&T, &T) -> Ordering,
) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => {
            let ordering = compare(&left, &right);
            match direction {
                SortDirection::Ascending => ordering,
                SortDirection::Descending => ordering.reverse(),
            }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Parses a finite Brazilian number, including `R$`, grouping dots, decimal
/// commas, percentages, and the multiplier `mil` (e.g. `R$ 5.000,50`). A
/// single dot followed by one or two digits is also accepted as a decimal.
pub fn parse_br_number(text: &str) -> Option<f64> {
    let mut value = text.trim().replace('\u{00a0}', " ");
    if value
        .get(..2)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("R$"))
    {
        value = value[2..].trim_start().to_owned();
    }
    value = value.trim_end_matches('%').trim_end().to_owned();
    let lower = value.to_lowercase();
    for suffix in ["reais", "real"] {
        if lower.ends_with(suffix) {
            value.truncate(value.len() - suffix.len());
            value = value.trim_end().to_owned();
            break;
        }
    }
    let multiplier = if value.to_lowercase().ends_with("mil") {
        value.truncate(value.len() - 3);
        value = value.trim_end().to_owned();
        1000.0
    } else {
        1.0
    };
    let compact: String = value.chars().filter(|ch| !ch.is_whitespace()).collect();
    let (sign, digits) = if let Some(rest) = compact.strip_prefix('-') {
        (-1.0, rest)
    } else if let Some(rest) = compact.strip_prefix('+') {
        (1.0, rest)
    } else {
        (1.0, compact.as_str())
    };
    if digits.is_empty() {
        return None;
    }
    let normalized = if let Some((whole, fractional)) = digits.split_once(',') {
        if fractional.is_empty()
            || !fractional.chars().all(|ch| ch.is_ascii_digit())
            || !valid_grouped_integer(whole)
        {
            return None;
        }
        format!("{}.{}", whole.replace('.', ""), fractional)
    } else if digits.contains('.') {
        if valid_grouped_integer(digits) {
            digits.replace('.', "")
        } else if digits.matches('.').count() == 1 {
            let (whole, fractional) = digits.split_once('.')?;
            if !whole.is_empty()
                && whole.chars().all(|ch| ch.is_ascii_digit())
                && (1..=2).contains(&fractional.len())
                && fractional.chars().all(|ch| ch.is_ascii_digit())
            {
                digits.to_owned()
            } else {
                return None;
            }
        } else {
            return None;
        }
    } else if digits.chars().all(|ch| ch.is_ascii_digit()) {
        digits.to_owned()
    } else {
        return None;
    };
    let parsed = normalized.parse::<f64>().ok()? * sign * multiplier;
    parsed.is_finite().then_some(parsed)
}

fn valid_grouped_integer(text: &str) -> bool {
    if !text.contains('.') {
        return !text.is_empty() && text.chars().all(|ch| ch.is_ascii_digit());
    }
    let mut groups = text.split('.');
    let Some(first) = groups.next() else {
        return false;
    };
    if first.is_empty() || first.len() > 3 || !first.chars().all(|ch| ch.is_ascii_digit()) {
        return false;
    }
    groups.all(|group| group.len() == 3 && group.chars().all(|ch| ch.is_ascii_digit()))
}

/// A boolean filter tree. `And` and `Or` must contain at least one child.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FilterExpr {
    Condition(Condition),
    And(Vec<FilterExpr>),
    Or(Vec<FilterExpr>),
    Not(Box<FilterExpr>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    pub column: usize,
    pub operator: Operator,
    pub value: FilterValue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Operator {
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
    Equal,
    NotEqual,
    Between,
    Contains,
    StartsWith,
    EndsWith,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FilterValue {
    Number(f64),
    Text(String),
    Range(f64, f64),
}

/// Returns original row indices matching a validated filter, in table order.
/// Blank or nonnumeric values do not match numeric comparisons.
pub fn evaluate_filter(table: &Table, expression: &FilterExpr) -> Result<Vec<usize>, String> {
    validate_filter(table, expression)?;
    Ok(table
        .rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| matches_filter(row, expression).then_some(index))
        .collect())
}

fn validate_filter(table: &Table, expression: &FilterExpr) -> Result<(), String> {
    match expression {
        FilterExpr::Condition(condition) => {
            if condition.column >= table.column_count() {
                return Err(format!("Coluna {} inexistente.", condition.column));
            }
            let valid = match (condition.operator, &condition.value) {
                (
                    Operator::Greater
                    | Operator::GreaterEqual
                    | Operator::Less
                    | Operator::LessEqual,
                    FilterValue::Number(number),
                ) => number.is_finite(),
                (Operator::Between, FilterValue::Range(low, high)) => {
                    low.is_finite() && high.is_finite() && low <= high
                }
                (Operator::Equal | Operator::NotEqual, FilterValue::Number(number)) => {
                    number.is_finite()
                }
                (
                    Operator::Equal
                    | Operator::NotEqual
                    | Operator::Contains
                    | Operator::StartsWith
                    | Operator::EndsWith,
                    FilterValue::Text(_),
                ) => true,
                _ => false,
            };
            if valid {
                Ok(())
            } else {
                Err("Operador e valor incompatíveis ou limite numérico inválido.".to_owned())
            }
        }
        FilterExpr::And(children) | FilterExpr::Or(children) => {
            if children.is_empty() {
                return Err("O filtro combinado não pode estar vazio.".to_owned());
            }
            for child in children {
                validate_filter(table, child)?;
            }
            Ok(())
        }
        FilterExpr::Not(child) => validate_filter(table, child),
    }
}

fn matches_filter(row: &[String], expression: &FilterExpr) -> bool {
    match expression {
        FilterExpr::Condition(condition) => row
            .get(condition.column)
            .is_some_and(|cell| matches_condition(cell, condition)),
        FilterExpr::And(children) => children.iter().all(|child| matches_filter(row, child)),
        FilterExpr::Or(children) => children.iter().any(|child| matches_filter(row, child)),
        FilterExpr::Not(child) => !matches_filter(row, child),
    }
}

fn matches_condition(cell: &str, condition: &Condition) -> bool {
    if cell.trim().is_empty() {
        return false;
    }
    match (&condition.operator, &condition.value) {
        (Operator::Greater, FilterValue::Number(target)) => {
            parse_br_number(cell).is_some_and(|number| number > *target)
        }
        (Operator::GreaterEqual, FilterValue::Number(target)) => {
            parse_br_number(cell).is_some_and(|number| number >= *target)
        }
        (Operator::Less, FilterValue::Number(target)) => {
            parse_br_number(cell).is_some_and(|number| number < *target)
        }
        (Operator::LessEqual, FilterValue::Number(target)) => {
            parse_br_number(cell).is_some_and(|number| number <= *target)
        }
        (Operator::Equal, FilterValue::Number(target)) => {
            parse_br_number(cell).is_some_and(|number| number == *target)
        }
        (Operator::NotEqual, FilterValue::Number(target)) => {
            parse_br_number(cell).is_some_and(|number| number != *target)
        }
        (Operator::Between, FilterValue::Range(low, high)) => {
            parse_br_number(cell).is_some_and(|number| number >= *low && number <= *high)
        }
        (Operator::Equal, FilterValue::Text(target)) => fold_text(cell) == fold_text(target),
        (Operator::NotEqual, FilterValue::Text(target)) => fold_text(cell) != fold_text(target),
        (Operator::Contains, FilterValue::Text(target)) => {
            fold_text(cell).contains(&fold_text(target))
        }
        (Operator::StartsWith, FilterValue::Text(target)) => {
            fold_text(cell).starts_with(&fold_text(target))
        }
        (Operator::EndsWith, FilterValue::Text(target)) => {
            fold_text(cell).ends_with(&fold_text(target))
        }
        _ => false,
    }
}

fn fold_text(text: &str) -> String {
    text.trim()
        .to_lowercase()
        .chars()
        .map(|ch| match ch {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            other => other,
        })
        .collect()
}

/// Summary of a filtered view, calculated only from the selected rows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FilterSummary {
    pub original_count: usize,
    pub matched_count: usize,
    pub percentage: f64,
    pub columns: Vec<ColumnStats>,
}

/// Statistics for one numeric column. `count` excludes blanks and invalid data.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColumnStats {
    pub column: usize,
    pub count: usize,
    pub sum: f64,
    pub mean: Option<f64>,
    pub median: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

/// Summarizes selected original row indices. Duplicates and invalid indices
/// are rejected to prevent misleading counts or totals.
pub fn summarize(table: &Table, row_indices: &[usize]) -> Result<FilterSummary, String> {
    let mut seen = HashSet::new();
    for &index in row_indices {
        if index >= table.row_count() {
            return Err(format!("Linha {index} inexistente no resumo."));
        }
        if !seen.insert(index) {
            return Err(format!("Linha {index} repetida no resumo."));
        }
    }
    let mut columns = Vec::new();
    for (column_index, column) in table.columns.iter().enumerate() {
        if !matches!(
            column.kind,
            ColumnKind::Number | ColumnKind::Currency | ColumnKind::Percentage
        ) {
            continue;
        }
        let mut values: Vec<f64> = row_indices
            .iter()
            .filter_map(|&row| table.rows[row].get(column_index))
            .filter_map(|cell| parse_br_number(cell))
            .collect();
        values.sort_by(f64::total_cmp);
        let count = values.len();
        let sum: f64 = values.iter().sum();
        let mean = (count > 0).then_some(sum / count as f64);
        let median = if count == 0 {
            None
        } else if count.is_multiple_of(2) {
            Some((values[count / 2 - 1] + values[count / 2]) / 2.0)
        } else {
            Some(values[count / 2])
        };
        columns.push(ColumnStats {
            column: column_index,
            count,
            sum,
            mean,
            median,
            min: values.first().copied(),
            max: values.last().copied(),
        });
    }
    Ok(FilterSummary {
        original_count: table.row_count(),
        matched_count: row_indices.len(),
        percentage: if table.row_count() == 0 {
            0.0
        } else {
            row_indices.len() as f64 / table.row_count() as f64 * 100.0
        },
        columns,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Table {
        let mut table = Table::new(
            "Produtos",
            vec!["Produto".into(), "Valor".into(), "Quantidade".into()],
        );
        for row in [
            ["A", "R$ 5.000", "4.000"],
            ["B", "R$ 5.001", "4.001"],
            ["C", "R$ 6.000", "4.000"],
            ["D", "", "5.000"],
            ["E", "R$ 3.000", "1.000"],
        ] {
            table.add_row(row.map(str::to_owned).to_vec()).unwrap();
        }
        table.infer_column_types();
        table
    }

    fn numeric(column: usize, operator: Operator, value: f64) -> FilterExpr {
        FilterExpr::Condition(Condition {
            column,
            operator,
            value: FilterValue::Number(value),
        })
    }

    #[test]
    fn parses_brazilian_numbers_and_rejects_invalid_values() {
        for (text, expected) in [
            ("5000", 5000.0),
            ("5.000", 5000.0),
            ("R$ 5.000", 5000.0),
            ("5 mil", 5000.0),
            ("5.000,50", 5000.5),
            ("10 mil reais", 10000.0),
            ("-1.250,25", -1250.25),
            ("12,5%", 12.5),
        ] {
            assert_eq!(parse_br_number(text), Some(expected), "{text}");
        }
        for text in ["", "R$", "cinco mil", "1,2,3", "1.23.4", "NaN"] {
            assert_eq!(parse_br_number(text), None, "{text}");
        }
    }

    #[test]
    fn numeric_boundaries_and_missing_values() {
        let table = sample();
        assert_eq!(table.columns[1].kind, ColumnKind::Currency);
        assert_eq!(table.columns[2].kind, ColumnKind::Number);
        assert_eq!(
            evaluate_filter(&table, &numeric(1, Operator::Greater, 5000.0)).unwrap(),
            vec![1, 2]
        );
        assert_eq!(
            evaluate_filter(&table, &numeric(1, Operator::GreaterEqual, 5000.0)).unwrap(),
            vec![0, 1, 2]
        );
        assert_eq!(
            evaluate_filter(&table, &numeric(1, Operator::LessEqual, 5000.0)).unwrap(),
            vec![0, 4]
        );
        assert_eq!(
            evaluate_filter(&table, &numeric(1, Operator::NotEqual, 5000.0)).unwrap(),
            vec![1, 2, 4]
        );
        let between = FilterExpr::Condition(Condition {
            column: 1,
            operator: Operator::Between,
            value: FilterValue::Range(5000.0, 6000.0),
        });
        assert_eq!(evaluate_filter(&table, &between).unwrap(), vec![0, 1, 2]);
    }

    #[test]
    fn combines_and_or_without_removing_rows() {
        let table = sample();
        let and = FilterExpr::And(vec![
            numeric(1, Operator::Greater, 5000.0),
            numeric(2, Operator::Greater, 4000.0),
        ]);
        assert_eq!(evaluate_filter(&table, &and).unwrap(), vec![1]);
        let or = FilterExpr::Or(vec![and, numeric(1, Operator::Less, 4000.0)]);
        assert_eq!(evaluate_filter(&table, &or).unwrap(), vec![1, 4]);
        assert_eq!(table.row_count(), 5);
    }

    #[test]
    fn invalid_columns_and_invalid_ast_return_errors_even_without_rows() {
        let table = Table::new("Vazio", vec!["Valor".into()]);
        assert!(evaluate_filter(&table, &numeric(2, Operator::Greater, 10.0)).is_err());
        assert!(evaluate_filter(&table, &FilterExpr::And(vec![])).is_err());
        assert!(
            evaluate_filter(
                &table,
                &FilterExpr::Condition(Condition {
                    column: 0,
                    operator: Operator::Between,
                    value: FilterValue::Range(10.0, 1.0)
                })
            )
            .is_err()
        );
    }

    #[test]
    fn statistics_use_only_selected_valid_values() {
        let table = sample();
        let summary = summarize(&table, &[0, 1, 3]).unwrap();
        assert_eq!(summary.original_count, 5);
        assert_eq!(summary.matched_count, 3);
        assert_eq!(summary.percentage, 60.0);
        let amount = summary
            .columns
            .iter()
            .find(|stats| stats.column == 1)
            .unwrap();
        assert_eq!(amount.count, 2);
        assert_eq!(amount.sum, 10001.0);
        assert_eq!(amount.mean, Some(5000.5));
        assert_eq!(amount.median, Some(5000.5));
        assert_eq!(amount.min, Some(5000.0));
        assert_eq!(amount.max, Some(5001.0));
        assert!(summarize(&table, &[0, 0]).is_err());
    }

    #[test]
    fn editing_keeps_rows_rectangular_and_inference_is_recomputed() {
        let mut table = Table::blank();
        table.add_row(vec!["1".into()]).unwrap();
        table.add_column("Data".into()).unwrap();
        assert!(table.add_column("data".into()).is_err());
        assert!(table.set_column_name(0, "Data".into()).is_err());
        table.set_cell(0, 1, "17/10/2026".into()).unwrap();
        table.infer_column_types();
        assert_eq!(table.columns[0].kind, ColumnKind::Number);
        assert_eq!(table.columns[1].kind, ColumnKind::Date);
        table.remove_column(0).unwrap();
        assert_eq!(table.rows[0], vec!["17/10/2026"]);
        table.remove_row(0).unwrap();
        assert!(table.rows.is_empty());
    }

    #[test]
    fn sorts_numbers_and_dates_stably_with_missing_values_last() {
        let mut table = Table::new("Estoque", vec!["Valor".into(), "Data".into()]);
        for row in [
            ["R$ 20,50", "20/01/2026"],
            ["R$ 3,00", "02/02/2025"],
            ["R$ 20,50", "01/03/2026"],
            ["", ""],
        ] {
            table.add_row(row.map(str::to_owned).to_vec()).unwrap();
        }
        table.infer_column_types();
        let source = vec![0, 1, 2, 3];
        assert_eq!(table.columns[0].kind, ColumnKind::Currency);
        assert_eq!(table.columns[1].kind, ColumnKind::Date);
        assert_eq!(
            sort_rows(
                &table,
                &source,
                &SortSpec {
                    column: 0,
                    direction: SortDirection::Descending
                }
            )
            .unwrap(),
            vec![0, 2, 1, 3]
        );
        assert_eq!(
            sort_rows(
                &table,
                &source,
                &SortSpec {
                    column: 1,
                    direction: SortDirection::Ascending
                }
            )
            .unwrap(),
            vec![1, 0, 2, 3]
        );
        assert_eq!(source, vec![0, 1, 2, 3]);
        assert!(
            sort_rows(
                &table,
                &source,
                &SortSpec {
                    column: 9,
                    direction: SortDirection::Ascending
                }
            )
            .is_err()
        );
    }
}
