use crate::config::{ColumnRule, Defaults, PairConfig};
use crate::row_stream::{open_row_stream, RowStream};
use anyhow::Result;
use chrono::NaiveDate;
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
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

fn file_info(path: &Path, columns: usize, rows: usize) -> FileInfo {
    FileInfo {
        path: path.display().to_string(),
        file_name: path
            .file_name()
            .and_then(|x| x.to_str())
            .unwrap_or_default()
            .to_string(),
        rows,
        columns,
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
fn match_columns(
    left_columns: &[String],
    right_columns: &[String],
) -> (Vec<ColumnMatch>, SchemaResult) {
    let mut right_by_name: std::collections::HashMap<String, Vec<usize>> =
        std::collections::HashMap::new();
    for (index, name) in right_columns.iter().enumerate() {
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
    for (sas_index, sas_name) in left_columns.iter().enumerate() {
        let key = normalized_name(sas_name);
        match right_by_name.get(&key) {
            Some(indices) if indices.len() == 1 && !matched_right.contains(&indices[0]) => {
                let adp_index = indices[0];
                matched_right.insert(adp_index);
                let adp_name = right_columns[adp_index].clone();
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
    let missing_in_sas = right_columns
        .iter()
        .enumerate()
        .filter(|(i, _)| !matched_right.contains(i))
        .map(|(_, name)| name.clone())
        .collect::<Vec<_>>();
    let normalized_left = left_columns
        .iter()
        .map(|v| normalized_name(v))
        .collect::<Vec<_>>();
    let normalized_right = right_columns
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
    let mut hasher = blake3::Hasher::new();
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
        hasher.update(&[0x1f]);
    }
    hasher.finalize().to_hex().to_string()
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

/// Acumula, en una sola pasada por celda, todo lo que antes requería varios
/// escaneos completos de la columna materializada (nulos, distintos, stats
/// numéricas, longitudes de texto, inferencia de tipo, rango y unicidad).
struct ColumnAccumulator {
    date_format: String,
    trim: bool,
    case_insensitive: bool,
    tolerance: f64,
    range_min: Option<f64>,
    range_max: Option<f64>,
    unique_required: Option<bool>,
    nullable_allowed: Option<bool>,

    any_non_null: bool,
    nulls: usize,
    distinct: HashSet<String>,
    all_numeric: bool,
    all_date: bool,
    all_boolean: bool,
    sum: f64,
    count_numeric: usize,
    min: Option<f64>,
    max: Option<f64>,
    text_min_len: Option<usize>,
    text_max_len: Option<usize>,
    out_of_range: usize,
    unique_seen: Option<HashSet<String>>,
    unique_total: usize,
}

impl ColumnAccumulator {
    fn new(pair: &PairConfig, defaults: &Defaults, name: &str) -> Self {
        let column_rule = rule(pair, name);
        Self {
            date_format: pair.date_format(defaults, name).to_string(),
            trim: pair.trim_values(defaults, name),
            case_insensitive: pair.case_insensitive_values(defaults, name),
            tolerance: pair.tolerance(defaults, name),
            range_min: column_rule.and_then(|r| r.min),
            range_max: column_rule.and_then(|r| r.max),
            unique_required: column_rule.and_then(|r| r.unique),
            nullable_allowed: column_rule.and_then(|r| r.nullable),
            any_non_null: false,
            nulls: 0,
            distinct: HashSet::new(),
            all_numeric: true,
            all_date: true,
            all_boolean: true,
            sum: 0.0,
            count_numeric: 0,
            min: None,
            max: None,
            text_min_len: None,
            text_max_len: None,
            out_of_range: 0,
            unique_seen: column_rule.and_then(|r| r.unique).map(|_| HashSet::new()),
            unique_total: 0,
        }
    }

    fn push(&mut self, value: &str) {
        self.distinct.insert(normalized(
            value,
            &self.date_format,
            self.trim,
            self.case_insensitive,
        ));
        if is_null(value) {
            self.nulls += 1;
            return;
        }
        self.any_non_null = true;
        let num = number(value);
        if num.is_none() {
            self.all_numeric = false;
        }
        if self.all_date && date(value, &self.date_format).is_none() {
            self.all_date = false;
        }
        if self.all_boolean
            && !matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "true" | "false" | "0" | "1"
            )
        {
            self.all_boolean = false;
        }
        if let Some(n) = num {
            self.sum += n;
            self.count_numeric += 1;
            self.min = Some(self.min.map_or(n, |m| m.min(n)));
            self.max = Some(self.max.map_or(n, |m| m.max(n)));
            let violates = self.range_min.is_some_and(|min| n < min)
                || self.range_max.is_some_and(|max| n > max);
            if violates {
                self.out_of_range += 1;
            }
        }
        let text_len = value.chars().count();
        self.text_min_len = Some(self.text_min_len.map_or(text_len, |m| m.min(text_len)));
        self.text_max_len = Some(self.text_max_len.map_or(text_len, |m| m.max(text_len)));
        if let Some(seen) = &mut self.unique_seen {
            self.unique_total += 1;
            seen.insert(value.to_string());
        }
    }

    fn infer_type(&self) -> String {
        if !self.any_non_null {
            return "empty".to_string();
        }
        if self.all_numeric {
            "number".to_string()
        } else if self.all_date {
            "date".to_string()
        } else if self.all_boolean {
            "boolean".to_string()
        } else {
            "text".to_string()
        }
    }

    fn unique_ok(&self) -> Option<bool> {
        self.unique_required.map(|required| {
            !required
                || self
                    .unique_seen
                    .as_ref()
                    .is_some_and(|seen| seen.len() == self.unique_total)
        })
    }

    fn nullable_ok(&self) -> Option<bool> {
        self.nullable_allowed
            .map(|allowed| allowed || self.nulls == 0)
    }
}

fn finish_column(
    column: &ColumnMatch,
    left: ColumnAccumulator,
    right: ColumnAccumulator,
    left_rows: usize,
    right_rows: usize,
) -> ColumnResult {
    let sas_type = left.infer_type();
    let adp_type = right.infer_type();
    let types_equal = sas_type == adp_type;
    let sas_sum = (left.count_numeric > 0).then_some(left.sum);
    let adp_sum = (right.count_numeric > 0).then_some(right.sum);
    let sas_mean = (left.count_numeric > 0).then(|| left.sum / left.count_numeric as f64);
    let adp_mean = (right.count_numeric > 0).then(|| right.sum / right.count_numeric as f64);
    let tolerance = left.tolerance;
    ColumnResult {
        name: column.sas_name.clone(),
        adp_name: column.adp_name.clone(),
        sas_type,
        adp_type,
        types_equal,
        sas_nulls: left.nulls,
        adp_nulls: right.nulls,
        sas_distinct: left.distinct.len(),
        adp_distinct: right.distinct.len(),
        sas_sum,
        adp_sum,
        sas_min: left.min,
        adp_min: right.min,
        sas_max: left.max,
        adp_max: right.max,
        sas_mean,
        adp_mean,
        sas_text_min_length: left.text_min_len,
        adp_text_min_length: right.text_min_len,
        sas_text_max_length: left.text_max_len,
        adp_text_max_length: right.text_max_len,
        sum_equal: match (sas_sum, adp_sum) {
            (Some(a), Some(b)) => {
                Some((a - b).abs() <= tolerance * left_rows.max(right_rows) as f64)
            }
            _ => None,
        },
        tolerance,
        sas_out_of_range: left.out_of_range,
        adp_out_of_range: right.out_of_range,
        sas_unique_ok: left.unique_ok(),
        adp_unique_ok: right.unique_ok(),
        sas_nullable_ok: left.nullable_ok(),
        adp_nullable_ok: right.nullable_ok(),
    }
}

type KeyGroups = BTreeMap<String, Vec<(String, String)>>;
type RowCounts = BTreeMap<String, (usize, String)>;

fn resolve_key_columns(pair: &PairConfig, columns: &[ColumnMatch]) -> Option<Vec<ColumnMatch>> {
    if pair.key_columns.is_empty() {
        return None;
    }
    pair.key_columns
        .iter()
        .map(|name| {
            columns
                .iter()
                .find(|column| normalized_name(&column.sas_name) == normalized_name(name))
                .cloned()
        })
        .collect::<Option<Vec<_>>>()
}

/// Actualiza en un solo paso, para una fila ya leída, los acumuladores de
/// columna, el conteo de fingerprints (para filas solo-en-un-lado) y, si hay
/// columnas clave configuradas, el agrupamiento por clave — evitando volver a
/// recorrer la fila o recalcular el fingerprint más de una vez.
#[allow(clippy::too_many_arguments)]
fn accumulate_row(
    row: &[String],
    columns: &[ColumnMatch],
    side_sas: bool,
    pair: &PairConfig,
    defaults: &Defaults,
    column_accs: &mut [ColumnAccumulator],
    row_counts: &mut RowCounts,
    key_columns: Option<&[ColumnMatch]>,
    key_groups: &mut Option<KeyGroups>,
) -> (String, String) {
    for (acc, column) in column_accs.iter_mut().zip(columns) {
        let index = if side_sas {
            column.sas_index
        } else {
            column.adp_index
        };
        let value = row.get(index).map(String::as_str).unwrap_or("");
        acc.push(value);
    }
    let fp = fingerprint(row, columns, side_sas, pair, defaults);
    let text = row_text(row, columns, side_sas);
    let entry = row_counts
        .entry(fp.clone())
        .or_insert_with(|| (0, text.clone()));
    entry.0 += 1;
    if let (Some(key_cols), Some(groups)) = (key_columns, key_groups.as_mut()) {
        let key = key_cols
            .iter()
            .map(|c| {
                row.get(if side_sas { c.sas_index } else { c.adp_index })
                    .cloned()
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" | ");
        groups.entry(key).or_default().push((fp.clone(), text.clone()));
    }
    (fp, text)
}

type SideAccumulation = (
    Vec<ColumnAccumulator>,
    RowCounts,
    Option<KeyGroups>,
    usize,
);

fn run_side(
    mut stream: RowStream,
    columns: &[ColumnMatch],
    side_sas: bool,
    pair: &PairConfig,
    defaults: &Defaults,
    key_columns: Option<&[ColumnMatch]>,
) -> Result<SideAccumulation> {
    let mut accs: Vec<ColumnAccumulator> = columns
        .iter()
        .map(|c| ColumnAccumulator::new(pair, defaults, &c.sas_name))
        .collect();
    let mut counts = RowCounts::new();
    let mut groups = key_columns.map(|_| KeyGroups::new());
    let mut rows = 0usize;
    for row in &mut stream {
        let row = row?;
        rows += 1;
        accumulate_row(
            &row,
            columns,
            side_sas,
            pair,
            defaults,
            &mut accs,
            &mut counts,
            key_columns,
            &mut groups,
        );
    }
    Ok((accs, counts, groups, rows))
}

fn finish_key_result(
    key_names: &[String],
    sas: KeyGroups,
    adp: KeyGroups,
) -> KeyResult {
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
    KeyResult {
        columns: key_names.to_vec(),
        sas_duplicate_keys: sas.values().filter(|v| v.len() > 1).count(),
        adp_duplicate_keys: adp.values().filter(|v| v.len() > 1).count(),
        keys_only_in_sas: only_sas,
        keys_only_in_adp: only_adp,
        changed_keys: changed,
        samples,
    }
}

pub fn compare_pair(pair: &PairConfig, defaults: &Defaults) -> PairResult {
    let result = (|| -> Result<PairResult> {
        let (left_info, left_stream) = open_row_stream(
            &pair.sas,
            pair.sas_delimiter(defaults) as u8,
            pair.sas_encoding(defaults),
        )?;
        let (right_info, right_stream) = open_row_stream(
            &pair.spark,
            pair.spark_delimiter(defaults) as u8,
            pair.spark_encoding(defaults),
        )?;
        let (columns, schema) = match_columns(&left_info.columns, &right_info.columns);
        let key_columns = resolve_key_columns(pair, &columns);
        let row_order = pair.row_order(defaults);

        let left_accs;
        let right_accs;
        let mut left_row_counts;
        let mut right_row_counts;
        let left_key_groups;
        let right_key_groups;
        let left_rows;
        let right_rows;
        let mut samples = Vec::new();

        if row_order {
            let mut left_iter = left_stream;
            let mut right_iter = right_stream;
            let mut la: Vec<ColumnAccumulator> = columns
                .iter()
                .map(|c| ColumnAccumulator::new(pair, defaults, &c.sas_name))
                .collect();
            let mut ra: Vec<ColumnAccumulator> = columns
                .iter()
                .map(|c| ColumnAccumulator::new(pair, defaults, &c.sas_name))
                .collect();
            let mut lc = RowCounts::new();
            let mut rc = RowCounts::new();
            let mut lg = key_columns.as_ref().map(|_| KeyGroups::new());
            let mut rg = key_columns.as_ref().map(|_| KeyGroups::new());
            let mut lr = 0usize;
            let mut rr = 0usize;
            let mut index = 0usize;
            loop {
                let left_next = left_iter.next();
                let right_next = right_iter.next();
                if left_next.is_none() && right_next.is_none() {
                    break;
                }
                let left_row = left_next.transpose()?;
                let right_row = right_next.transpose()?;
                let left_text = left_row.as_ref().map(|row| {
                    lr += 1;
                    accumulate_row(
                        row,
                        &columns,
                        true,
                        pair,
                        defaults,
                        &mut la,
                        &mut lc,
                        key_columns.as_deref(),
                        &mut lg,
                    )
                    .1
                });
                let right_text = right_row.as_ref().map(|row| {
                    rr += 1;
                    accumulate_row(
                        row,
                        &columns,
                        false,
                        pair,
                        defaults,
                        &mut ra,
                        &mut rc,
                        key_columns.as_deref(),
                        &mut rg,
                    )
                    .1
                });
                let equal = match (&left_row, &right_row) {
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
                if !equal && samples.len() < SAMPLE_LIMIT {
                    samples.push(DifferenceSample {
                        location: format!("fila {}", index + 1),
                        sas: left_text.unwrap_or_default(),
                        adp: right_text.unwrap_or_default(),
                    });
                }
                index += 1;
            }
            left_accs = la;
            right_accs = ra;
            left_row_counts = lc;
            right_row_counts = rc;
            left_key_groups = lg;
            right_key_groups = rg;
            left_rows = lr;
            right_rows = rr;
        } else {
            let columns_ref = &columns;
            let key_columns_ref = key_columns.as_deref();
            let (left_result, right_result) = std::thread::scope(|scope| {
                let left_handle = scope.spawn(|| {
                    run_side(left_stream, columns_ref, true, pair, defaults, key_columns_ref)
                });
                let right_handle = scope.spawn(|| {
                    run_side(right_stream, columns_ref, false, pair, defaults, key_columns_ref)
                });
                (
                    left_handle.join().expect("left comparison thread panicked"),
                    right_handle
                        .join()
                        .expect("right comparison thread panicked"),
                )
            });
            let (la, lc, lg, lr) = left_result?;
            let (ra, rc, rg, rr) = right_result?;
            left_accs = la;
            right_accs = ra;
            left_row_counts = lc;
            right_row_counts = rc;
            left_key_groups = lg;
            right_key_groups = rg;
            left_rows = lr;
            right_rows = rr;
        }

        let column_results = columns
            .iter()
            .zip(left_accs)
            .zip(right_accs)
            .map(|((column, left_acc), right_acc)| {
                finish_column(column, left_acc, right_acc, left_rows, right_rows)
            })
            .collect::<Vec<_>>();

        let mut sas_only_samples = Vec::new();
        let mut adp_only_samples = Vec::new();
        let mut rows_only_in_sas = 0;
        let mut rows_only_in_adp = 0;
        for key in left_row_counts
            .keys()
            .chain(right_row_counts.keys())
            .cloned()
            .collect::<HashSet<_>>()
        {
            let a = left_row_counts.remove(&key);
            let b = right_row_counts.remove(&key);
            let ac = a.as_ref().map(|v| v.0).unwrap_or(0);
            let bc = b.as_ref().map(|v| v.0).unwrap_or(0);
            if ac > bc {
                rows_only_in_sas += ac - bc;
                if sas_only_samples.len() < SAMPLE_LIMIT {
                    sas_only_samples.push(RowSample {
                        row: a.unwrap().1,
                        count: ac - bc,
                    });
                }
            }
            if bc > ac {
                rows_only_in_adp += bc - ac;
                if adp_only_samples.len() < SAMPLE_LIMIT {
                    adp_only_samples.push(RowSample {
                        row: b.unwrap().1,
                        count: bc - ac,
                    });
                }
            }
        }
        let mut differing_rows = rows_only_in_sas + rows_only_in_adp;
        let rows_equal = if !schema.columns_equal {
            false
        } else if row_order {
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

        let key_result = match (left_key_groups, right_key_groups) {
            (Some(sas), Some(adp)) => Some(finish_key_result(&pair.key_columns, sas, adp)),
            _ => None,
        };
        let row_count_equal = left_rows == right_rows;
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
            sas: file_info(&pair.sas, left_info.columns.len(), left_rows),
            spark: file_info(&pair.spark, right_info.columns.len(), right_rows),
            formats: FormatResult {
                sas_format: left_info.format.clone(),
                adp_format: right_info.format.clone(),
                sas_details: left_info.details.clone(),
                adp_details: right_info.details.clone(),
            },
            row_count_equal,
            schema,
            row_order_compared: row_order,
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
    use rayon::prelude::*;
    let pairs = pairs
        .par_iter()
        .map(|pair| compare_pair(pair, defaults))
        .collect::<Vec<_>>();
    RunResult {
        passed: pairs.iter().all(|pair| pair.passed),
        pairs,
    }
}
