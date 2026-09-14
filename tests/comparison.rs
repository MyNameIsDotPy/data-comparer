use data_comparer::compare::compare_all;
use data_comparer::config::{ColumnRule, Defaults, PairConfig};
use data_comparer::report::render_html;
use std::fs;

fn pair(left: &std::path::Path, right: &std::path::Path) -> PairConfig {
    PairConfig {
        name: Some("prueba".to_string()),
        sas: left.to_path_buf(),
        spark: right.to_path_buf(),
        compare_row_order: None,
        compare_column_order: None,
        date_format: None,
        columns: Default::default(),
        key_columns: vec![],
        delimiter: None,
        sas_delimiter: None,
        spark_delimiter: None,
        encoding: None,
        sas_encoding: None,
        spark_encoding: None,
    }
}

#[test]
fn compares_unordered_csv_with_dates_and_tolerance() {
    let temp = tempfile::tempdir().unwrap();
    let left = temp.path().join("sas.csv");
    let right = temp.path().join("spark.csv");
    fs::write(
        &left,
        "id,fecha,importe\n1,01/02/2024,10.0\n2,02/02/2024,20.0\n",
    )
    .unwrap();
    fs::write(
        &right,
        "id,fecha,importe\n2,2024-02-02,20.04\n1,2024-02-01,10.04\n",
    )
    .unwrap();
    let result = compare_all(&[pair(&left, &right)], &Defaults::default());
    assert!(result.passed, "{result:#?}");
}

#[test]
fn detects_column_order_difference() {
    let temp = tempfile::tempdir().unwrap();
    let left = temp.path().join("sas.csv");
    let right = temp.path().join("spark.csv");
    fs::write(&left, "id,importe\n1,10\n").unwrap();
    fs::write(&right, "importe,id\n10,1\n").unwrap();
    let result = compare_all(&[pair(&left, &right)], &Defaults::default());
    assert!(!result.passed);
    assert!(!result.pairs[0].schema.column_order_equal);
}

#[test]
fn matches_column_names_with_casing_difference() {
    let temp = tempfile::tempdir().unwrap();
    let left = temp.path().join("sas.csv");
    let right = temp.path().join("adp.csv");
    fs::write(&left, "poliza,importe\nA-1,10\n").unwrap();
    fs::write(&right, "POLIZA,IMPORTE\nA-1,10\n").unwrap();
    let result = compare_all(&[pair(&left, &right)], &Defaults::default());
    assert!(result.passed, "{result:#?}");
    assert_eq!(result.pairs[0].schema.name_differences.len(), 2);
    assert!(result.pairs[0].schema.missing_in_adp.is_empty());
}

#[test]
fn reports_rows_and_keys_only_on_each_side() {
    let temp = tempfile::tempdir().unwrap();
    let left = temp.path().join("sas.csv");
    let right = temp.path().join("adp.csv");
    fs::write(&left, "poliza,importe\nA,10\nB,20\n").unwrap();
    fs::write(&right, "poliza,importe\nA,10\nC,30\n").unwrap();
    let mut config = pair(&left, &right);
    config.key_columns = vec!["poliza".to_string()];
    let result = compare_all(&[config], &Defaults::default());
    let pair = &result.pairs[0];
    assert_eq!(pair.rows_only_in_sas, 1);
    assert_eq!(pair.rows_only_in_adp, 1);
    let keys = pair.key_result.as_ref().unwrap();
    assert_eq!(keys.keys_only_in_sas, 1);
    assert_eq!(keys.keys_only_in_adp, 1);
}

#[test]
fn compares_files_with_different_source_encodings() {
    let temp = tempfile::tempdir().unwrap();
    let left = temp.path().join("sas.csv");
    let right = temp.path().join("adp.csv");
    fs::write(&left, "poliza,ciudad\nA-1,Bogotá\n").unwrap();
    let (encoded, _, _) = encoding_rs::WINDOWS_1252.encode("poliza,ciudad\nA-1,Bogotá\n");
    fs::write(&right, &encoded).unwrap();
    let mut config = pair(&left, &right);
    config.spark_encoding = Some("windows-1252".to_string());
    let result = compare_all(&[config], &Defaults::default());
    assert!(result.passed, "{result:#?}");
}

#[test]
fn applies_quality_rules_and_renders_foldable_sections() {
    let temp = tempfile::tempdir().unwrap();
    let left = temp.path().join("sas.csv");
    let right = temp.path().join("adp.csv");
    fs::write(&left, "poliza,importe\nA,150\nA,10\n").unwrap();
    fs::write(&right, "poliza,importe\nA,150\nA,10\n").unwrap();
    let mut config = pair(&left, &right);
    config.columns.insert(
        "poliza".to_string(),
        ColumnRule {
            unique: Some(true),
            nullable: Some(false),
            ..Default::default()
        },
    );
    config.columns.insert(
        "importe".to_string(),
        ColumnRule {
            max: Some(100.0),
            ..Default::default()
        },
    );
    let run = compare_all(&[config], &Defaults::default());
    assert!(!run.passed);
    let amount = run.pairs[0]
        .column_results
        .iter()
        .find(|column| column.name == "importe")
        .unwrap();
    assert_eq!(amount.sas_out_of_range, 1);
    assert!(render_html(&run).contains("<details"));
}
