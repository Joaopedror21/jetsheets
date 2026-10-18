//! Connected CSV -> filter -> Portuguese aggregation -> export -> project flow.

use jetsheets_charts::DashboardSpec;
use jetsheets_core::{AggregateOperation, aggregate_rows, evaluate_filter};
use jetsheets_language::{ParsedCommand, parse_command, parse_filter};
use jetsheets_storage::{Project, export_aggregate_csv, import_csv, load_project, save_project};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

struct TempFiles {
    source: PathBuf,
    grouped: PathBuf,
    project: PathBuf,
}

impl TempFiles {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after Unix epoch")
            .as_nanos();
        let prefix =
            std::env::temp_dir().join(format!("jetsheets-group-{}-{nonce}", std::process::id()));
        Self {
            source: prefix.with_extension("csv"),
            grouped: prefix.with_extension("grouped.csv"),
            project: prefix.with_extension("jetsheet"),
        }
    }
}

impl Drop for TempFiles {
    fn drop(&mut self) {
        for path in [&self.source, &self.grouped, &self.project] {
            let _ = fs::remove_file(path);
        }
    }
}

#[test]
fn filtered_grouped_sum_exports_and_reopens() {
    let paths = TempFiles::new();
    fs::write(
        &paths.source,
        "Categoria;Valor;Status\nPapel;10,50;Ativo\nTinta;20,00;Inativo\nPapel;5,25;Ativo\nTinta;;Ativo\n",
    )
    .unwrap();

    let table = import_csv(&paths.source).unwrap();
    let filter = parse_filter("Filtre status igual a Ativo", &table)
        .unwrap()
        .expr;
    let visible = evaluate_filter(&table, &filter).unwrap();
    assert_eq!(visible, vec![0, 2, 3]);

    let ParsedCommand::Aggregate(command) =
        parse_command("Some valor por categoria", &table).unwrap()
    else {
        panic!("expected an aggregate command");
    };
    assert_eq!(
        command.spec.operation,
        AggregateOperation::Sum { value_column: 1 }
    );
    let groups = aggregate_rows(&table, &visible, &command.spec).unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(
        (groups[0].label.as_str(), groups[0].count, groups[0].total),
        ("Papel", 2, 15.75)
    );
    assert_eq!(
        (
            groups[1].label.as_str(),
            groups[1].count,
            groups[1].valid_values
        ),
        ("Tinta", 1, 0)
    );

    assert_eq!(
        export_aggregate_csv(&paths.grouped, &table, &visible, &command.spec).unwrap(),
        2
    );
    let exported = fs::read_to_string(&paths.grouped).unwrap();
    assert!(exported.contains("Papel;2;2;15,75"));
    assert!(exported.contains("Tinta;1;0;0"));

    let project = Project {
        table,
        filter: Some(filter),
        sort: None,
        aggregate: Some(command.spec),
        chart: None,
        dashboard: DashboardSpec::default(),
    };
    save_project(&paths.project, &project).unwrap();
    let reopened = load_project(&paths.project).unwrap();
    let reopened_visible =
        evaluate_filter(&reopened.table, reopened.filter.as_ref().unwrap()).unwrap();
    assert_eq!(reopened.aggregate, project.aggregate);
    assert_eq!(
        aggregate_rows(
            &reopened.table,
            &reopened_visible,
            reopened.aggregate.as_ref().unwrap()
        )
        .unwrap(),
        groups
    );
}
