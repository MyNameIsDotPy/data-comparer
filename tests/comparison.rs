use data_comparer::compare::compare_all;
use data_comparer::config::{Defaults, PairConfig};
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
