//! Validated chart specifications and data preparation for the desktop UI.
//! Rendering stays in the UI so charts share its typography and interaction model.

use jetsheets_core::{Table, parse_br_number};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChartKind {
    Bar,
    Line,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChartSpec {
    pub kind: ChartKind,
    pub x_column: usize,
    pub y_column: usize,
    pub title: String,
}

/// Saved dashboard layout. Cards always derive their values from the current
/// filtered rows; no copied data or stale computed numbers are stored.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DashboardSpec {
    pub metric_column: Option<usize>,
    pub charts: Vec<ChartSpec>,
}

pub const MAX_DASHBOARD_CHARTS: usize = 6;

impl DashboardSpec {
    pub fn validate(&self, table: &Table) -> Result<(), String> {
        if self
            .metric_column
            .is_some_and(|column| column >= table.column_count())
        {
            return Err("A coluna das métricas do dashboard não existe.".into());
        }
        if self.charts.len() > MAX_DASHBOARD_CHARTS {
            return Err(format!(
                "O dashboard permite até {MAX_DASHBOARD_CHARTS} gráficos."
            ));
        }
        for (index, chart) in self.charts.iter().enumerate() {
            chart
                .validate(table)
                .map_err(|error| format!("Gráfico {} do dashboard: {error}", index + 1))?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChartPoint {
    pub label: String,
    pub value: f64,
    /// Index of the original table row, for selection and tooltips.
    pub row: usize,
}

impl ChartSpec {
    /// Check column references without requiring any rows or numeric values.
    pub fn validate(&self, table: &Table) -> Result<(), String> {
        if self.x_column >= table.columns.len() {
            return Err("A coluna do eixo X não existe na tabela.".into());
        }
        if self.y_column >= table.columns.len() {
            return Err("A coluna do eixo Y não existe na tabela.".into());
        }
        Ok(())
    }
}

/// Build chart points from the specified table rows. Empty Y values are skipped;
/// nonempty values that cannot be read as numbers receive an explicit error.
pub fn prepare_chart(
    table: &Table,
    row_indices: &[usize],
    spec: &ChartSpec,
) -> Result<Vec<ChartPoint>, String> {
    spec.validate(table)?;

    let mut points = Vec::with_capacity(row_indices.len());
    for &row_index in row_indices {
        let row = table
            .rows
            .get(row_index)
            .ok_or_else(|| format!("A linha {} não existe na tabela.", row_index + 1))?;
        let x = row
            .get(spec.x_column)
            .ok_or_else(|| format!("A linha {} não contém a coluna X.", row_index + 1))?;
        let y = row
            .get(spec.y_column)
            .ok_or_else(|| format!("A linha {} não contém a coluna Y.", row_index + 1))?;
        if y.trim().is_empty() {
            continue;
        }
        let value = parse_br_number(y)
            .filter(|value| value.is_finite())
            .ok_or_else(|| {
                format!(
                    "O valor '{}' na linha {}, coluna '{}', não é numérico.",
                    y,
                    row_index + 1,
                    table.columns[spec.y_column].name
                )
            })?;
        points.push(ChartPoint {
            label: if x.trim().is_empty() {
                format!("Linha {}", row_index + 1)
            } else {
                x.trim().to_owned()
            },
            value,
            row: row_index,
        });
    }

    if points.is_empty() {
        return Err(
            "Não há valores numéricos nas linhas selecionadas para criar o gráfico.".into(),
        );
    }

    Ok(points)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepares_selected_rows_and_skips_missing_values() {
        let mut table = Table::new("Vendas", vec!["Produto".into(), "Valor".into()]);
        table.add_row(vec!["A".into(), "1.234,50".into()]).unwrap();
        table.add_row(vec!["B".into(), String::new()]).unwrap();
        table.add_row(vec!["C".into(), "200".into()]).unwrap();
        let spec = ChartSpec {
            kind: ChartKind::Bar,
            x_column: 0,
            y_column: 1,
            title: "Valores".into(),
        };

        let points = prepare_chart(&table, &[2, 1, 0], &spec).unwrap();
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].row, 2);
        assert_eq!(points[1].value, 1234.5);
    }

    #[test]
    fn reports_invalid_y_value() {
        let mut table = Table::new("Vendas", vec!["Produto".into(), "Valor".into()]);
        table.add_row(vec!["A".into(), "sem valor".into()]).unwrap();
        let spec = ChartSpec {
            kind: ChartKind::Line,
            x_column: 0,
            y_column: 1,
            title: String::new(),
        };
        assert!(
            prepare_chart(&table, &[0], &spec)
                .unwrap_err()
                .to_string()
                .contains("não é numérico")
        );
    }

    #[test]
    fn dashboard_rejects_missing_columns_and_excess_cards() {
        let table = Table::new("Dados", vec!["Nome".into(), "Valor".into()]);
        let chart = ChartSpec {
            kind: ChartKind::Bar,
            x_column: 0,
            y_column: 1,
            title: "Valores".into(),
        };
        assert!(
            DashboardSpec {
                metric_column: Some(2),
                charts: vec![chart.clone()],
            }
            .validate(&table)
            .is_err()
        );
        assert!(
            DashboardSpec {
                metric_column: Some(1),
                charts: vec![chart; MAX_DASHBOARD_CHARTS + 1],
            }
            .validate(&table)
            .is_err()
        );
    }
}
