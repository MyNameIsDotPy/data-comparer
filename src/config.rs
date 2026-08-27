use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn default_tolerance() -> f64 {
    0.1
}
fn default_date_format() -> String {
    "%d/%m/%Y".to_string()
}
fn default_column_order() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Defaults {
    #[serde(default = "default_tolerance")]
    pub numeric_tolerance: f64,
    #[serde(default)]
    pub compare_row_order: bool,
    #[serde(default = "default_column_order")]
    pub compare_column_order: bool,
    #[serde(default = "default_date_format")]
    pub date_format: String,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            numeric_tolerance: default_tolerance(),
            compare_row_order: false,
            compare_column_order: true,
            date_format: default_date_format(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ColumnRule {
    pub numeric_tolerance: Option<f64>,
    pub date_format: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairConfig {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(alias = "left")]
    pub sas: PathBuf,
    #[serde(alias = "right")]
    pub spark: PathBuf,
    #[serde(default)]
    pub compare_row_order: Option<bool>,
    #[serde(default)]
    pub compare_column_order: Option<bool>,
    #[serde(default)]
    pub date_format: Option<String>,
    #[serde(default)]
    pub columns: BTreeMap<String, ColumnRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    #[serde(default)]
    pub defaults: Defaults,
    pub pairs: Vec<PairConfig>,
}

impl PairConfig {
    pub fn label(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| format!("{} vs {}", self.sas.display(), self.spark.display()))
    }
    pub fn row_order(&self, defaults: &Defaults) -> bool {
        self.compare_row_order.unwrap_or(defaults.compare_row_order)
    }
    pub fn column_order(&self, defaults: &Defaults) -> bool {
        self.compare_column_order
            .unwrap_or(defaults.compare_column_order)
    }
    pub fn date_format<'a>(&'a self, defaults: &'a Defaults, column: &'a str) -> &'a str {
        self.columns
            .get(column)
            .and_then(|r| r.date_format.as_deref())
            .or(self.date_format.as_deref())
            .unwrap_or(&defaults.date_format)
    }
    pub fn tolerance(&self, defaults: &Defaults, column: &str) -> f64 {
        self.columns
            .get(column)
            .and_then(|r| r.numeric_tolerance)
            .unwrap_or(defaults.numeric_tolerance)
    }
}
