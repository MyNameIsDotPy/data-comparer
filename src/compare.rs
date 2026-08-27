use crate::config::{ColumnRule, Defaults, PairConfig};
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
pub struct NameDifference {
    pub sas: String,
    pub adp: String,
    pub kind: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct SchemaResult {
    pub columns_equal: bool,
    pub column_order_equal: bool,
    pub missing_in_adp: Vec<String>,
    pub missing_in_sas: Vec<String>,
    pub name_differences: Vec<NameDifference>,
    pub ambiguous_columns: Vec<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct FormatResult {
    pub sas_format: String,
    pub adp_format: String,
    pub sas_details: Vec<String>,
    pub adp_details: Vec<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct ColumnResult {
    pub name: String,
    pub adp_name: String,
    pub sas_type: String,
    pub adp_type: String,
    pub types_equal: bool,
    pub sas_nulls: usize,
    pub adp_nulls: usize,
    pub sas_distinct: usize,
    pub adp_distinct: usize,
    pub sas_sum: Option<f64>,
    pub adp_sum: Option<f64>,
    pub sas_min: Option<f64>,
    pub adp_min: Option<f64>,
    pub sas_max: Option<f64>,
    pub adp_max: Option<f64>,
    pub sas_mean: Option<f64>,
    pub adp_mean: Option<f64>,
    pub sas_text_min_length: Option<usize>,
    pub adp_text_min_length: Option<usize>,
    pub sas_text_max_length: Option<usize>,
    pub adp_text_max_length: Option<usize>,
    pub sum_equal: Option<bool>,
    pub tolerance: f64,
    pub sas_out_of_range: usize,
    pub adp_out_of_range: usize,
    pub sas_unique_ok: Option<bool>,
    pub adp_unique_ok: Option<bool>,
    pub sas_nullable_ok: Option<bool>,
    pub adp_nullable_ok: Option<bool>,
}
#[derive(Debug, Clone, Serialize)]
pub struct DifferenceSample {
    pub location: String,
    pub sas: String,
    pub adp: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct RowSample {
    pub row: String,
    pub count: usize,
}
#[derive(Debug, Clone, Serialize)]
pub struct KeyResult {
    pub columns: Vec<String>,
    pub sas_duplicate_keys: usize,
    pub adp_duplicate_keys: usize,
    pub keys_only_in_sas: usize,
    pub keys_only_in_adp: usize,
    pub changed_keys: usize,
    pub samples: Vec<DifferenceSample>,
}
#[derive(Debug, Clone, Serialize)]
pub struct PairResult {
    pub name: String,
    pub sas: FileInfo,
    pub spark: FileInfo,
    pub formats: FormatResult,
    pub row_count_equal: bool,
    pub schema: SchemaResult,
    pub row_order_compared: bool,
    pub rows_equal: bool,
    pub differing_rows: usize,
    pub rows_only_in_sas: usize,
    pub rows_only_in_adp: usize,
    pub sas_only_samples: Vec<RowSample>,
    pub adp_only_samples: Vec<RowSample>,
    pub key_result: Option<KeyResult>,
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

#[derive(Clone)]
struct ColumnMatch {
    sas_index: usize,
    adp_index: usize,
    sas_name: String,
    adp_name: String,
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
    (!is_null(value)).then_some(value).and_then(|v| {
        v.parse::<f64>()
            .ok()
            .or_else(|| v.replace(',', ".").parse::<f64>().ok())
    })
}
fn date(value: &str, configured: &str) -> Option<NaiveDate> {
    let value = value.trim();
    (!is_null(value)).then_some(value).and_then(|v| {
        NaiveDate::parse_from_str(v, configured)
            .ok()
            .or_else(|| NaiveDate::parse_from_str(v, "%Y-%m-%d").ok())
            .or_else(|| NaiveDate::parse_from_str(v, "%d/%m/%Y").ok())
    })
}
fn normalized_name(value: &str) -> String {
    value.trim().to_lowercase()
}
fn normalized(value: &str, date_format: &str, trim: bool, case_insensitive: bool) -> String {
    if is_null(value) {
        return "<NULL>".to_string();
    }
    if let Some(date) = date(value, date_format) {
        return format!("D:{}", date.format("%Y-%m-%d"));
    }
    if let Some(number) = number(value) {
        return format!("N:{number:.12}");
    }
    let value = if trim { value.trim() } else { value };
    if case_insensitive {
        value.to_lowercase()
    } else {
        value.to_string()
    }
}
fn infer_type(values: &[String], format: &str) -> String {
    let values = values.iter().filter(|v| !is_null(v)).collect::<Vec<_>>();
    if values.is_empty() {
        return "empty".to_string();
    }
    if values.iter().all(|v| number(v).is_some()) {
        return "number".to_string();
    }
    if values.iter().all(|v| date(v, format).is_some()) {
        return "date".to_string();
    }
    if values.iter().all(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "true" | "false" | "0" | "1"
        )
    }) {
        return "boolean".to_string();
    }
    "text".to_string()
}
fn match_columns(left: &Table, right: &Table) -> (Vec<ColumnMatch>, SchemaResult) {
    let mut right_by_name: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, name) in right.columns.iter().enumerate() {
        right_by_name
            .entry(normalized_name(name))
            .or_default()
            .push(index);
    }
    let mut matched_right = HashSet::new();
    let mut matches = Vec::new();
    let mut missing_in_adp = Vec::new();
    let mut name_differences = Vec::new();
    let mut ambiguous = Vec::new();
    for (sas_index, sas_name) in left.columns.iter().enumerate() {
        let key = normalized_name(sas_name);
        match right_by_name.get(&key) {
            Some(indices) if indices.len() == 1 && !matched_right.contains(&indices[0]) => {
                let adp_index = indices[0];
                matched_right.insert(adp_index);
                let adp_name = right.columns[adp_index].clone();
                if sas_name != &adp_name {
                    name_differences.push(NameDifference {
                        sas: sas_name.clone(),
                        adp: adp_name.clone(),
                        kind: if sas_name.trim() == adp_name.trim() {
                            "casing".to_string()
                        } else {
                            "spacing_or_casing".to_string()
                        },
                    });
                }
                matches.push(ColumnMatch {
                    sas_index,
                    adp_index,
                    sas_name: sas_name.clone(),
                    adp_name,
                });
            }
            Some(_) => ambiguous.push(format!("{} (normalizado como {})", sas_name, key)),
            None => missing_in_adp.push(sas_name.clone()),
        }
    }
    let missing_in_sas = right
        .columns
        .iter()
        .enumerate()
        .filter(|(i, _)| !matched_right.contains(i))
        .map(|(_, name)| name.clone())
        .collect::<Vec<_>>();
    let normalized_left = left
        .columns
        .iter()
        .map(|v| normalized_name(v))
        .collect::<Vec<_>>();
    let normalized_right = right
        .columns
        .iter()
        .map(|v| normalized_name(v))
        .collect::<Vec<_>>();
    let columns_equal =
        missing_in_adp.is_empty() && missing_in_sas.is_empty() && ambiguous.is_empty();
    (
        matches,
        SchemaResult {
            columns_equal,
            column_order_equal: normalized_left == normalized_right,
            missing_in_adp,
            missing_in_sas,
            name_differences,
            ambiguous_columns: ambiguous,
        },
    )
}
fn rule<'a>(pair: &'a PairConfig, column: &str) -> Option<&'a ColumnRule> {
    pair.columns.get(column)
}
fn values_equal(
    left: &str,
    right: &str,
    pair: &PairConfig,
    defaults: &Defaults,
    column: &str,
) -> bool {
    if is_null(left) || is_null(right) {
        return is_null(left) && is_null(right);
    }
    match (number(left), number(right)) {
        (Some(a), Some(b)) => (a - b).abs() <= pair.tolerance(defaults, column),
        _ => match (
            date(left, pair.date_format(defaults, column)),
            date(right, pair.date_format(defaults, column)),
        ) {
            (Some(a), Some(b)) => a == b,
            _ => {
                normalized(
                    left,
                    pair.date_format(defaults, column),
                    pair.trim_values(defaults, column),
                    pair.case_insensitive_values(defaults, column),
                ) == normalized(
                    right,
                    pair.date_format(defaults, column),
                    pair.trim_values(defaults, column),
                    pair.case_insensitive_values(defaults, column),
                )
            }
        },
    }
}
fn fingerprint(
    row: &[String],
    columns: &[ColumnMatch],
    side_sas: bool,
    pair: &PairConfig,
    defaults: &Defaults,
) -> String {
    let mut hasher = Sha256::new();
    for column in columns {
        let index = if side_sas {
            column.sas_index
        } else {
            column.adp_index
        };
        let name = &column.sas_name;
        let value = row.get(index).map(String::as_str).unwrap_or("");
        let value = match number(value) {
            Some(value) if pair.tolerance(defaults, name) > 0.0 => {
                format!("N:{}", (value / pair.tolerance(defaults, name)).round())
            }
            _ => normalized(
                value,
                pair.date_format(defaults, name),
                pair.trim_values(defaults, name),
                pair.case_insensitive_values(defaults, name),
            ),
        };
        hasher.update(value.as_bytes());
        hasher.update([0x1f]);
    }
    format!("{:x}", hasher.finalize())
}
fn row_text(row: &[String], columns: &[ColumnMatch], side_sas: bool) -> String {
    columns
        .iter()
        .map(|column| {
            row.get(if side_sas {
                column.sas_index
            } else {
                column.adp_index
            })
            .cloned()
            .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" | ")
}
fn row_counts(
    table: &Table,
    columns: &[ColumnMatch],
    side_sas: bool,
    pair: &PairConfig,
    defaults: &Defaults,
) -> BTreeMap<String, (usize, String)> {
    let mut result = BTreeMap::new();
    for row in &table.rows {
        let key = fingerprint(row, columns, side_sas, pair, defaults);
        let entry = result
            .entry(key)
            .or_insert((0, row_text(row, columns, side_sas)));
        entry.0 += 1;
    }
    result
}
fn numeric_stats(values: &[String]) -> (Option<f64>, Option<f64>, Option<f64>, Option<f64>) {
    let values = values.iter().filter_map(|v| number(v)).collect::<Vec<_>>();
    if values.is_empty() {
        return (None, None, None, None);
    }
    let sum = values.iter().sum::<f64>();
    (
        Some(sum),
        values.iter().copied().reduce(f64::min),
        values.iter().copied().reduce(f64::max),
        Some(sum / values.len() as f64),
    )
}
fn text_lengths(values: &[String]) -> (Option<usize>, Option<usize>) {
    let lengths = values
        .iter()
        .filter(|v| !is_null(v))
        .map(|v| v.chars().count())
        .collect::<Vec<_>>();
    (lengths.iter().copied().min(), lengths.iter().copied().max())
}
fn column_result(
    column: &ColumnMatch,
    left: &[String],
    right: &[String],
    pair: &PairConfig,
    defaults: &Defaults,
) -> ColumnResult {
    let name = &column.sas_name;
    let tolerance = pair.tolerance(defaults, name);
    let (sas_sum, sas_min, sas_max, sas_mean) = numeric_stats(left);
    let (adp_sum, adp_min, adp_max, adp_mean) = numeric_stats(right);
    let column_rule = rule(pair, name);
    let range = |values: &[String]| {
        values
            .iter()
            .filter_map(|v| number(v))
            .filter(|value| {
                column_rule
                    .and_then(|r| r.min)
                    .is_some_and(|min| *value < min)
                    || column_rule
                        .and_then(|r| r.max)
                        .is_some_and(|max| *value > max)
            })
            .count()
    };
    let valid_unique = |values: &[String]| {
        column_rule.and_then(|r| r.unique).map(|required| {
            !required
                || values
                    .iter()
                    .filter(|v| !is_null(v))
                    .collect::<HashSet<_>>()
                    .len()
                    == values.iter().filter(|v| !is_null(v)).count()
        })
    };
    let valid_nullable = |values: &[String]| {
        column_rule
            .and_then(|r| r.nullable)
            .map(|allowed| allowed || values.iter().all(|v| !is_null(v)))
    };
    ColumnResult {
        name: name.clone(),
        adp_name: column.adp_name.clone(),
        sas_type: infer_type(left, pair.date_format(defaults, name)),
        adp_type: infer_type(right, pair.date_format(defaults, name)),
        types_equal: infer_type(left, pair.date_format(defaults, name))
            == infer_type(right, pair.date_format(defaults, name)),
        sas_nulls: left.iter().filter(|v| is_null(v)).count(),
        adp_nulls: right.iter().filter(|v| is_null(v)).count(),
        sas_distinct: left
            .iter()
            .map(|v| {
                normalized(
                    v,
                    pair.date_format(defaults, name),
                    pair.trim_values(defaults, name),
                    pair.case_insensitive_values(defaults, name),
                )
            })
            .collect::<HashSet<_>>()
            .len(),
        adp_distinct: right
            .iter()
            .map(|v| {
                normalized(
                    v,
                    pair.date_format(defaults, name),
                    pair.trim_values(defaults, name),
                    pair.case_insensitive_values(defaults, name),
                )
            })
            .collect::<HashSet<_>>()
            .len(),
        sas_sum,
        adp_sum,
        sas_min,
        adp_min,
        sas_max,
        adp_max,
        sas_mean,
        adp_mean,
        sas_text_min_length: text_lengths(left).0,
        adp_text_min_length: text_lengths(right).0,
        sas_text_max_length: text_lengths(left).1,
        adp_text_max_length: text_lengths(right).1,
        sum_equal: match (sas_sum, adp_sum) {
            (Some(a), Some(b)) => {
                Some((a - b).abs() <= tolerance * left.len().max(right.len()) as f64)
            }
            _ => None,
        },
        tolerance,
        sas_out_of_range: range(left),
        adp_out_of_range: range(right),
        sas_unique_ok: valid_unique(left),
        adp_unique_ok: valid_unique(right),
        sas_nullable_ok: valid_nullable(left),
        adp_nullable_ok: valid_nullable(right),
    }
}
fn key_result(
    pair: &PairConfig,
    columns: &[ColumnMatch],
    left: &Table,
    right: &Table,
    defaults: &Defaults,
) -> Option<KeyResult> {
    if pair.key_columns.is_empty() {
        return None;
    }
    let keys = pair
        .key_columns
        .iter()
        .map(|name| {
            columns
                .iter()
                .find(|column| normalized_name(&column.sas_name) == normalized_name(name))
                .cloned()
        })
        .collect::<Option<Vec<_>>>()?;
    let collect = |table: &Table, side_sas: bool| {
        let mut values = BTreeMap::<String, Vec<(String, String)>>::new();
        for row in &table.rows {
            let key = keys
                .iter()
                .map(|column| {
                    row.get(if side_sas {
                        column.sas_index
                    } else {
                        column.adp_index
                    })
                    .cloned()
                    .unwrap_or_default()
                })
                .collect::<Vec<_>>()
                .join(" | ");
            values.entry(key).or_default().push((
                fingerprint(row, columns, side_sas, pair, defaults),
                row_text(row, columns, side_sas),
            ));
        }
        values
    };
    let sas = collect(left, true);
    let adp = collect(right, false);
    let mut samples = Vec::new();
    let mut only_sas = 0;
    let mut only_adp = 0;
    let mut changed = 0;
    for key in sas.keys().chain(adp.keys()).collect::<HashSet<_>>() {
        match (sas.get(key), adp.get(key)) {
            (Some(a), None) => {
                only_sas += 1;
                if samples.len() < SAMPLE_LIMIT {
                    samples.push(DifferenceSample {
                        location: format!("clave {key} solo en SAS"),
                        sas: a
                            .iter()
                            .map(|value| value.1.as_str())
                            .collect::<Vec<_>>()
                            .join(" || "),
                        adp: String::new(),
                    });
                }
            }
            (None, Some(b)) => {
                only_adp += 1;
                if samples.len() < SAMPLE_LIMIT {
                    samples.push(DifferenceSample {
                        location: format!("clave {key} solo en ADP"),
                        sas: String::new(),
                        adp: b
                            .iter()
                            .map(|value| value.1.as_str())
                            .collect::<Vec<_>>()
                            .join(" || "),
                    });
                }
            }
            (Some(a), Some(b))
                if {
                    let mut a = a.iter().map(|value| value.0.as_str()).collect::<Vec<_>>();
                    let mut b = b.iter().map(|value| value.0.as_str()).collect::<Vec<_>>();
                    a.sort_unstable();
                    b.sort_unstable();
                    a != b
                } =>
            {
                changed += 1;
                if samples.len() < SAMPLE_LIMIT {
                    samples.push(DifferenceSample {
                        location: format!("clave {key} con valores distintos"),
                        sas: a
                            .iter()
                            .map(|value| value.1.as_str())
                            .collect::<Vec<_>>()
                            .join(" || "),
                        adp: b
                            .iter()
                            .map(|value| value.1.as_str())
                            .collect::<Vec<_>>()
                            .join(" || "),
                    });
                }
            }
            _ => {}
        }
    }
    Some(KeyResult {
        columns: pair.key_columns.clone(),
        sas_duplicate_keys: sas.values().filter(|v| v.len() > 1).count(),
        adp_duplicate_keys: adp.values().filter(|v| v.len() > 1).count(),
        keys_only_in_sas: only_sas,
        keys_only_in_adp: only_adp,
        changed_keys: changed,
        samples,
    })
}
pub fn compare_pair(pair: &PairConfig, defaults: &Defaults) -> PairResult {
    let result = (|| -> Result<PairResult> {
        let left = read_table(&pair.sas)?;
        let right = read_table(&pair.spark)?;
        let (columns, schema) = match_columns(&left, &right);
        let column_results = columns
            .iter()
            .map(|column| {
                let a = left
                    .rows
                    .iter()
                    .map(|r| r.get(column.sas_index).cloned().unwrap_or_default())
                    .collect::<Vec<_>>();
                let b = right
                    .rows
                    .iter()
                    .map(|r| r.get(column.adp_index).cloned().unwrap_or_default())
                    .collect::<Vec<_>>();
                column_result(column, &a, &b, pair, defaults)
            })
            .collect::<Vec<_>>();
        let left_counts = row_counts(&left, &columns, true, pair, defaults);
        let right_counts = row_counts(&right, &columns, false, pair, defaults);
        let mut sas_only_samples = Vec::new();
        let mut adp_only_samples = Vec::new();
        let mut rows_only_in_sas = 0;
        let mut rows_only_in_adp = 0;
        for key in left_counts
            .keys()
            .chain(right_counts.keys())
            .collect::<HashSet<_>>()
        {
            let a = left_counts.get(key);
            let b = right_counts.get(key);
            let ac = a.map(|v| v.0).unwrap_or(0);
            let bc = b.map(|v| v.0).unwrap_or(0);
            if ac > bc {
                rows_only_in_sas += ac - bc;
                if sas_only_samples.len() < SAMPLE_LIMIT {
                    sas_only_samples.push(RowSample {
                        row: a.unwrap().1.clone(),
                        count: ac - bc,
                    });
                }
            }
            if bc > ac {
                rows_only_in_adp += bc - ac;
                if adp_only_samples.len() < SAMPLE_LIMIT {
                    adp_only_samples.push(RowSample {
                        row: b.unwrap().1.clone(),
                        count: bc - ac,
                    });
                }
            }
        }
        let mut samples = Vec::new();
        let mut differing_rows = rows_only_in_sas + rows_only_in_adp;
        let rows_equal = if !schema.columns_equal {
            false
        } else if pair.row_order(defaults) {
            let limit = left.rows.len().max(right.rows.len());
            for index in 0..limit {
                let a = left.rows.get(index);
                let b = right.rows.get(index);
                let equal = match (a, b) {
                    (Some(a), Some(b)) => columns.iter().all(|column| {
                        values_equal(
                            a.get(column.sas_index).map(String::as_str).unwrap_or(""),
                            b.get(column.adp_index).map(String::as_str).unwrap_or(""),
                            pair,
                            defaults,
                            &column.sas_name,
                        )
                    }),
                    _ => false,
                };
                if !equal {
                    if samples.len() < SAMPLE_LIMIT {
                        samples.push(DifferenceSample {
                            location: format!("fila {}", index + 1),
                            sas: a.map(|r| row_text(r, &columns, true)).unwrap_or_default(),
                            adp: b.map(|r| row_text(r, &columns, false)).unwrap_or_default(),
                        });
                    }
                }
            }
            differing_rows = samples.len().max(differing_rows);
            samples.is_empty()
        } else {
            for sample in &sas_only_samples {
                if samples.len() < SAMPLE_LIMIT {
                    samples.push(DifferenceSample {
                        location: "fila solo en SAS".to_string(),
                        sas: format!("{} x {}", sample.row, sample.count),
                        adp: String::new(),
                    });
                }
            }
            for sample in &adp_only_samples {
                if samples.len() < SAMPLE_LIMIT {
                    samples.push(DifferenceSample {
                        location: "fila solo en ADP".to_string(),
                        sas: String::new(),
                        adp: format!("{} x {}", sample.row, sample.count),
                    });
                }
            }
            rows_only_in_sas == 0 && rows_only_in_adp == 0
        };
        let key_result = key_result(pair, &columns, &left, &right, defaults);
        let row_count_equal = left.rows.len() == right.rows.len();
        let summaries_equal = column_results.iter().all(|r| {
            r.types_equal
                && r.sas_nulls == r.adp_nulls
                && r.sas_distinct == r.adp_distinct
                && r.sum_equal != Some(false)
                && r.sas_out_of_range == 0
                && r.adp_out_of_range == 0
                && r.sas_unique_ok != Some(false)
                && r.adp_unique_ok != Some(false)
                && r.sas_nullable_ok != Some(false)
                && r.adp_nullable_ok != Some(false)
        });
        let passed = row_count_equal
            && schema.columns_equal
            && (!pair.column_order(defaults) || schema.column_order_equal)
            && rows_equal
            && summaries_equal;
        Ok(PairResult {
            name: pair.label(),
            sas: info(&pair.sas, &left),
            spark: info(&pair.spark, &right),
            formats: FormatResult {
                sas_format: left.format.clone(),
                adp_format: right.format.clone(),
                sas_details: left.details.clone(),
                adp_details: right.details.clone(),
            },
            row_count_equal,
            schema,
            row_order_compared: pair.row_order(defaults),
            rows_equal,
            differing_rows,
            rows_only_in_sas,
            rows_only_in_adp,
            sas_only_samples,
            adp_only_samples,
            key_result,
            column_results,
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
        formats: FormatResult {
            sas_format: "-".to_string(),
            adp_format: "-".to_string(),
            sas_details: vec![],
            adp_details: vec![],
        },
        row_count_equal: false,
        schema: SchemaResult {
            columns_equal: false,
            column_order_equal: false,
            missing_in_adp: vec![],
            missing_in_sas: vec![],
            name_differences: vec![],
            ambiguous_columns: vec![],
        },
        row_order_compared: pair.row_order(defaults),
        rows_equal: false,
        differing_rows: 0,
        rows_only_in_sas: 0,
        rows_only_in_adp: 0,
        sas_only_samples: vec![],
        adp_only_samples: vec![],
        key_result: None,
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
