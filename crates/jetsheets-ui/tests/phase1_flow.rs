//! Integration of the Phase 1 CSV → Portuguese filter → results → chart →
//! local project flow. No desktop window is needed for the data workflow.

use jetsheets_charts::{ChartKind, ChartSpec, DashboardSpec, prepare_chart};
use jetsheets_core::{ColumnKind, evaluate_filter, summarize};
use jetsheets_language::parse_filter;
use jetsheets_storage::{Project, export_csv_view, import_csv, load_project, save_project};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

struct TempArtifacts {
    csv: PathBuf,
    result_csv: PathBuf,
    project: PathBuf,
}

impl TempArtifacts {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after the Unix epoch")
            .as_nanos();
        let stem = format!("jetsheets-phase1-{}-{nonce}", std::process::id());
        let directory = std::env::temp_dir();
        Self {
            csv: directory.join(format!("{stem}.csv")),
            result_csv: directory.join(format!("{stem}-resultado.csv")),
            project: directory.join(format!("{stem}.jetsheet")),
        }
    }
}

impl Drop for TempArtifacts {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.csv);
        let _ = fs::remove_file(&self.result_csv);
        let _ = fs::remove_file(&self.project);
    }
}

#[test]
fn phase1_csv_filter_results_chart_and_project_roundtrip() {
    let paths = TempArtifacts::new();
    fs::write(
        &paths.csv,
        concat!(
            "Produto;Valor;Quantidade\n",
            "Caneta;R$ 4.000;5.000\n",
            "Notebook;R$ 6.000;4.001\n",
            "Mesa;R$ 5.000;4.001\n",
            "Cadeira;R$ 6.500;4.500\n",
            "Caderno;R$ 7.000;3.999\n",
            "Lápis;;4.100\n",
        ),
    )
    .unwrap();

    let table = import_csv(&paths.csv).unwrap();
    assert_eq!(table.row_count(), 6);
    assert_eq!(table.columns[0].name, "Produto");
    assert_eq!(table.columns[1].kind, ColumnKind::Currency);
    assert_eq!(table.columns[2].kind, ColumnKind::Number);

    let command = "Filtre os produtos com valor acima de 5000 e quantidade maior que 4000";
    let first_filter = parse_filter(command, &table).unwrap();
    assert!(first_filter.explanation.contains("Valor"));
    assert!(first_filter.explanation.contains("Quantidade"));
    let first_rows = evaluate_filter(&table, &first_filter.expr).unwrap();
    assert_eq!(first_rows, vec![1, 3]);
    let summary = summarize(&table, &first_rows).unwrap();
    assert_eq!(summary.original_count, 6);
    assert_eq!(summary.matched_count, 2);
    assert!((summary.percentage - 100.0 / 3.0).abs() < 1e-10);
    let amount = summary
        .columns
        .iter()
        .find(|stats| stats.column == 1)
        .unwrap();
    assert_eq!(amount.sum, 12_500.0);
    assert_eq!(amount.mean, Some(6_250.0));
    export_csv_view(&paths.result_csv, &table, &first_rows).unwrap();
    let exported = import_csv(&paths.result_csv).unwrap();
    assert_eq!(exported.row_count(), 2);
    assert_eq!(exported.rows[0][0], "Notebook");
    assert_eq!(exported.rows[1][0], "Cadeira");

    let chart = ChartSpec {
        kind: ChartKind::Bar,
        x_column: 0,
        y_column: 1,
        title: "Valor dos produtos filtrados".to_owned(),
    };
    let initial_points = prepare_chart(&table, &first_rows, &chart).unwrap();
    assert_eq!(initial_points.len(), 2);
    assert_eq!(initial_points[0].label, "Notebook");
    assert_eq!(initial_points[0].value, 6_000.0);
    assert_eq!(initial_points[1].label, "Cadeira");

    // The chart derives points from the current view, so replacing the filter
    // immediately changes its data without altering the source table.
    let changed_filter = parse_filter(
        "Filtre os produtos com valor acima de 4000 e quantidade maior que 4000",
        &table,
    )
    .unwrap();
    let changed_rows = evaluate_filter(&table, &changed_filter.expr).unwrap();
    assert_eq!(changed_rows, vec![1, 2, 3]);
    let updated_points = prepare_chart(&table, &changed_rows, &chart).unwrap();
    assert_eq!(updated_points.len(), 3);
    assert_eq!(updated_points[1].label, "Mesa");
    assert_eq!(updated_points[1].value, 5_000.0);
    assert_eq!(table.row_count(), 6);

    let project = Project {
        table,
        filter: Some(changed_filter.expr),
        sort: None,
        aggregate: None,
        chart: Some(chart),
        dashboard: DashboardSpec::default(),
    };
    save_project(&paths.project, &project).unwrap();
    let reopened = load_project(&paths.project).unwrap();
    assert_eq!(reopened.table.rows, project.table.rows);
    assert_eq!(reopened.filter, project.filter);
    assert_eq!(reopened.chart, project.chart);
    let reopened_rows = evaluate_filter(
        &reopened.table,
        reopened.filter.as_ref().expect("saved filter is present"),
    )
    .unwrap();
    let reopened_points = prepare_chart(
        &reopened.table,
        &reopened_rows,
        reopened.chart.as_ref().expect("saved chart is present"),
    )
    .unwrap();
    assert_eq!(reopened_points, updated_points);
}
