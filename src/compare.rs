use crate::config::{Defaults, PairConfig};
use crate::reader::{read_table, Table};
use anyhow::Result;
use chrono::NaiveDate;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

const SAMPLE_LIMIT: usize = 50;

#[derive(Debug, Clone, Serialize)]
pub struct FileInfo {
    pub path: String,
    pub file_name: String,
    pub rows: usize,
    pub columns: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaResult {
    pub columns_equal: bool,
    pub column_order_equal: bool,
    pub missing_in_spark: Vec<String>,
    pub missing_in_sas: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ColumnResult {
    pub name: String,
    pub sas_nulls: usize,
    pub spark_nulls: usize,
    pub sas_distinct: usize,
    pub spark_distinct: usize,
    pub sas_sum: Option<f64>,
    pub spark_sum: Option<f64>,
    pub sum_equal: Option<bool>,
    pub tolerance: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DifferenceSample {
    pub location: String,
    pub sas: String,
    pub spark: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PairResult {
    pub name: String,
    pub sas: FileInfo,
    pub spark: FileInfo,
    pub row_count_equal: bool,
    pub schema: SchemaResult,
    pub row_order_compared: bool,
    pub rows_equal: bool,
    pub differing_rows: usize,
    pub column_results: Vec<ColumnResult>,
    pub samples: Vec<DifferenceSample>,
    pub passed: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunResult {
    pub pairs: Vec<PairResult>,
    pub passed: bool,
}

fn info(path: &Path, table: &Table) -> FileInfo {
    FileInfo {
        path: path.display().to_string(),
        file_name: path
            .file_name()
            .and_then(|x| x.to_str())
            .unwrap_or_default()
            .to_string(),
        rows: table.rows.len(),
        columns: table.columns.len(),
    }
}

fn is_null(value: &str) -> bool {
    value.trim().is_empty()
        || value.eq_ignore_ascii_case("null")
        || value.eq_ignore_ascii_case("nan")
}

fn number(value: &str) -> Option<f64> {
    let value = value.trim();
    if is_null(value) {
        return None;
    }
    value
        .parse::<f64>()
        .ok()
        .or_else(|| value.replace(',', ".").parse::<f64>().ok())
}

fn date(value: &str, configured: &str) -> Option<NaiveDate> {
    let value = value.trim();
    if is_null(value) {
        return None;
    }
    NaiveDate::parse_from_str(value, configured)
        .ok()
        .or_else(|| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
        .or_else(|| NaiveDate::parse_from_str(value, "%d/%m/%Y").ok())
}

fn normalized(value: &str, date_format: &str) -> String {
    if is_null(value) {
        return "<NULL>".to_string();
    }
    if let Some(date) = date(value, date_format) {
        return date.format("%Y-%m-%d").to_string();
    }
    if let Some(number) = number(value) {
        return format!("N:{number:.12}");
    }
    value.trim().to_string()
}

fn values_equal(left: &str, right: &str, date_format: &str, tolerance: f64) -> bool {
    if is_null(left) || is_null(right) {
        return is_null(left) && is_null(right);
    }
    match (number(left), number(right)) {
        (Some(a), Some(b)) => (a - b).abs() <= tolerance,
        _ => match (date(left, date_format), date(right, date_format)) {
            (Some(a), Some(b)) => a == b,
            _ => left.trim() == right.trim(),
        },
    }
}

fn fingerprint(
    row: &[String],
    columns: &[String],
    pair: &PairConfig,
    defaults: &Defaults,
) -> String {
    let mut hasher = Sha256::new();
    for (index, column) in columns.iter().enumerate() {
        let value = row.get(index).map(String::as_str).unwrap_or("");
        let normalized = match number(value) {
            Some(value) if pair.tolerance(defaults, column) > 0.0 => {
                format!("N:{}", (value / pair.tolerance(defaults, column)).round())
            }
            _ => normalized(value, pair.date_format(defaults, column)),
        };
        hasher.update(normalized.as_bytes());
        hasher.update([0x1f]);
    }
    format!("{:x}", hasher.finalize())
}

fn column_result(name: &str, left: &[String], right: &[String], tolerance: f64) -> ColumnResult {
    let left_numbers: Vec<f64> = left.iter().filter_map(|v| number(v)).collect();
    let right_numbers: Vec<f64> = right.iter().filter_map(|v| number(v)).collect();
    let sas_sum: Option<f64> = (!left_numbers.is_empty()).then(|| left_numbers.iter().sum::<f64>());
    let spark_sum: Option<f64> =
        (!right_numbers.is_empty()).then(|| right_numbers.iter().sum::<f64>());
    let sum_equal = match (sas_sum, spark_sum) {
        (Some(a), Some(b)) => Some((a - b).abs() <= tolerance),
        _ => None,
    };
    ColumnResult {
        name: name.to_string(),
        sas_nulls: left.iter().filter(|v| is_null(v)).count(),
        spark_nulls: right.iter().filter(|v| is_null(v)).count(),
        sas_distinct: left.iter().collect::<HashSet<_>>().len(),
        spark_distinct: right.iter().collect::<HashSet<_>>().len(),
        sas_sum,
        spark_sum,
        sum_equal,
        tolerance,
    }
}

pub fn compare_pair(pair: &PairConfig, defaults: &Defaults) -> PairResult {
    let result = (|| -> Result<PairResult> {
        let left = read_table(&pair.sas)?;
        let right = read_table(&pair.spark)?;
        let left_names: HashSet<_> = left.columns.iter().cloned().collect();
        let right_names: HashSet<_> = right.columns.iter().cloned().collect();
        let missing_in_spark = left_names
            .difference(&right_names)
            .cloned()
            .collect::<Vec<_>>();
        let missing_in_sas = right_names
            .difference(&left_names)
            .cloned()
            .collect::<Vec<_>>();
        let columns_equal = missing_in_spark.is_empty() && missing_in_sas.is_empty();
        let column_order_equal = left.columns == right.columns;
        let schema = SchemaResult {
            columns_equal,
            column_order_equal,
            missing_in_spark,
            missing_in_sas,
        };
        let mut samples = Vec::new();
        let mut results = Vec::new();
        let right_index: HashMap<&str, usize> = right
            .columns
            .iter()
            .enumerate()
            .map(|(i, name)| (name.as_str(), i))
            .collect();
        for (left_index, name) in left.columns.iter().enumerate() {
            let Some(&right_column) = right_index.get(name.as_str()) else {
                continue;
            };
            let a = left
                .rows
                .iter()
                .map(|row| row.get(left_index).cloned().unwrap_or_default())
                .collect::<Vec<_>>();
            let b = right
                .rows
                .iter()
                .map(|row| row.get(right_column).cloned().unwrap_or_default())
                .collect::<Vec<_>>();
            results.push(column_result(name, &a, &b, pair.tolerance(defaults, name)));
        }
        let mut differing_rows = 0;
        let rows_equal = if !columns_equal {
            false
        } else if pair.row_order(defaults) {
            let limit = left.rows.len().max(right.rows.len());
            for row_index in 0..limit {
                let a = left.rows.get(row_index);
                let b = right.rows.get(row_index);
                let same = match (a, b) {
                    (Some(a), Some(b)) => left.columns.iter().enumerate().all(|(i, col)| {
                        values_equal(
                            a.get(i).map(String::as_str).unwrap_or(""),
                            b.get(*right_index.get(col.as_str()).unwrap())
                                .map(String::as_str)
                                .unwrap_or(""),
                            pair.date_format(defaults, col),
                            pair.tolerance(defaults, col),
                        )
                    }),
                    _ => false,
                };
                if !same {
                    differing_rows += 1;
                    if samples.len() < SAMPLE_LIMIT {
                        samples.push(DifferenceSample {
                            location: format!("fila {}", row_index + 1),
                            sas: a.map(|r| r.join(" | ")).unwrap_or_default(),
                            spark: b.map(|r| r.join(" | ")).unwrap_or_default(),
                        });
                    }
                }
            }
            differing_rows == 0
        } else {
            let mut left_rows: BTreeMap<String, (usize, String)> = BTreeMap::new();
            let mut right_rows: BTreeMap<String, (usize, String)> = BTreeMap::new();
            for row in &left.rows {
                let key = fingerprint(row, &left.columns, pair, defaults);
                let entry = left_rows.entry(key).or_insert((0, row.join(" | ")));
                entry.0 += 1;
            }
            for row in &right.rows {
                let reordered = left
                    .columns
                    .iter()
                    .map(|col| {
                        row.get(*right_index.get(col.as_str()).unwrap())
                            .cloned()
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>();
                let key = fingerprint(&reordered, &left.columns, pair, defaults);
                let entry = right_rows.entry(key).or_insert((0, reordered.join(" | ")));
                entry.0 += 1;
            }
            for key in left_rows
                .keys()
                .chain(right_rows.keys())
                .collect::<HashSet<_>>()
            {
                let a = left_rows.get(key);
                let b = right_rows.get(key);
                let count_a = a.map(|v| v.0).unwrap_or(0);
                let count_b = b.map(|v| v.0).unwrap_or(0);
                if count_a != count_b {
                    differing_rows += count_a.abs_diff(count_b);
                    if samples.len() < SAMPLE_LIMIT {
                        samples.push(DifferenceSample {
                            location: "fila (sin orden)".to_string(),
                            sas: a.map(|v| format!("{} x {}", v.1, v.0)).unwrap_or_default(),
                            spark: b.map(|v| format!("{} x {}", v.1, v.0)).unwrap_or_default(),
                        });
                    }
                }
            }
            differing_rows == 0
        };
        let row_count_equal = left.rows.len() == right.rows.len();
        let summaries_equal = results.iter().all(|r| {
            r.sas_nulls == r.spark_nulls
                && r.sas_distinct == r.spark_distinct
                && r.sum_equal != Some(false)
        });
        let passed = row_count_equal
            && columns_equal
            && (!pair.column_order(defaults) || column_order_equal)
            && rows_equal
            && summaries_equal;
        Ok(PairResult {
            name: pair.label(),
            sas: info(&pair.sas, &left),
            spark: info(&pair.spark, &right),
            row_count_equal,
            schema,
            row_order_compared: pair.row_order(defaults),
            rows_equal,
            differing_rows,
            column_results: results,
            samples,
            passed,
            error: None,
        })
    })();
    result.unwrap_or_else(|error| PairResult {
        name: pair.label(),
        sas: FileInfo {
            path: pair.sas.display().to_string(),
            file_name: pair
                .sas
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string(),
            rows: 0,
            columns: 0,
        },
        spark: FileInfo {
            path: pair.spark.display().to_string(),
            file_name: pair
                .spark
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string(),
            rows: 0,
            columns: 0,
        },
        row_count_equal: false,
        schema: SchemaResult {
            columns_equal: false,
            column_order_equal: false,
            missing_in_spark: vec![],
            missing_in_sas: vec![],
        },
        row_order_compared: pair.row_order(defaults),
        rows_equal: false,
        differing_rows: 0,
        column_results: vec![],
        samples: vec![],
        passed: false,
        error: Some(error.to_string()),
    })
}

pub fn compare_all(pairs: &[PairConfig], defaults: &Defaults) -> RunResult {
    let pairs = pairs
        .iter()
        .map(|pair| compare_pair(pair, defaults))
        .collect::<Vec<_>>();
    RunResult {
        passed: pairs.iter().all(|pair| pair.passed),
        pairs,
    }
}
