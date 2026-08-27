use crate::compare::RunResult;
use crate::config::Manifest;
use anyhow::Result;
use chrono::Local;
use std::fs;
use std::path::{Path, PathBuf};

pub struct ReportPaths {
    pub directory: PathBuf,
    pub html: PathBuf,
    pub json: PathBuf,
    pub manifest: PathBuf,
}

pub fn write_reports(run: &RunResult, manifest: &Manifest, root: &Path) -> Result<ReportPaths> {
    let directory = root.join(Local::now().format("%Y-%m-%d_%H%M%S").to_string());
    fs::create_dir_all(&directory)?;
    let html = directory.join("report.html");
    let json = directory.join("result.json");
    let manifest_path = directory.join("manifest.yaml");
    fs::write(&json, serde_json::to_vec_pretty(run)?)?;
    fs::write(&manifest_path, serde_yaml::to_string(manifest)?)?;
    fs::write(&html, render_html(run))?;
    Ok(ReportPaths {
        directory,
        html,
        json,
        manifest: manifest_path,
    })
}

fn esc(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
fn status(value: bool) -> &'static str {
    if value {
        "OK"
    } else {
        "DIFERENCIA"
    }
}

pub fn render_html(run: &RunResult) -> String {
    let mut body = String::new();
    for pair in &run.pairs {
        body.push_str(&format!(
            "<section><h2>{}</h2><p class=\"{}\">{}</p><p><b>SAS:</b> {}<br><b>Spark:</b> {}</p>",
            if pair.passed { "ok" } else { "bad" },
            status(pair.passed),
            esc(&pair.name),
            esc(&pair.sas.path),
            esc(&pair.spark.path)
        ));
        if let Some(error) = &pair.error {
            body.push_str(&format!("<p class=bad>{}</p></section>", esc(error)));
            continue;
        }
        body.push_str(&format!("<table><tr><th>Validación</th><th>SAS</th><th>Spark</th><th>Resultado</th></tr><tr><td>Filas</td><td>{}</td><td>{}</td><td>{}</td></tr><tr><td>Columnas</td><td>{}</td><td>{}</td><td>{}</td></tr><tr><td>Orden de columnas</td><td colspan=2>comparado</td><td>{}</td></tr><tr><td>Filas {}</td><td colspan=2>{} diferencias</td><td>{}</td></tr></table>", pair.sas.rows, pair.spark.rows, status(pair.row_count_equal), pair.sas.columns, pair.spark.columns, status(pair.schema.columns_equal), status(pair.schema.column_order_equal), if pair.row_order_compared { "en orden" } else { "sin orden" }, pair.differing_rows, status(pair.rows_equal)));
        if !pair.schema.missing_in_spark.is_empty() || !pair.schema.missing_in_sas.is_empty() {
            body.push_str(&format!(
                "<p class=bad>Faltan en Spark: {}. Faltan en SAS: {}.</p>",
                esc(&pair.schema.missing_in_spark.join(", ")),
                esc(&pair.schema.missing_in_sas.join(", "))
            ));
        }
        body.push_str("<h3>Resumen por columna</h3><table><tr><th>Columna</th><th>Nulos SAS/Spark</th><th>Distintos SAS/Spark</th><th>Suma SAS/Spark</th><th>Estado suma</th></tr>");
        for column in &pair.column_results {
            body.push_str(&format!(
                "<tr><td>{}</td><td>{}/{}</td><td>{}/{}</td><td>{}/{}</td><td>{}</td></tr>",
                esc(&column.name),
                column.sas_nulls,
                column.spark_nulls,
                column.sas_distinct,
                column.spark_distinct,
                column
                    .sas_sum
                    .map(|v| format!("{v:.6}"))
                    .unwrap_or_else(|| "-".to_string()),
                column
                    .spark_sum
                    .map(|v| format!("{v:.6}"))
                    .unwrap_or_else(|| "-".to_string()),
                column.sum_equal.map(status).unwrap_or("N/A")
            ));
        }
        body.push_str("</table>");
        if !pair.samples.is_empty() {
            body.push_str("<h3>Muestra de diferencias</h3><table><tr><th>Ubicación</th><th>SAS</th><th>Spark</th></tr>");
            for sample in &pair.samples {
                body.push_str(&format!(
                    "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
                    esc(&sample.location),
                    esc(&sample.sas),
                    esc(&sample.spark)
                ));
            }
            body.push_str("</table>");
        }
        body.push_str("</section>");
    }
    format!("<!doctype html><html lang=\"es\"><head><meta charset=\"utf-8\"><title>Informe Data Comparer</title><style>body{{font-family:system-ui,sans-serif;max-width:1200px;margin:2rem auto;padding:0 1rem;color:#172033}}section{{border:1px solid #d7dce5;border-radius:8px;padding:1rem;margin:1rem 0}}table{{border-collapse:collapse;width:100%;margin:.7rem 0}}th,td{{border:1px solid #d7dce5;padding:.45rem;text-align:left;vertical-align:top;word-break:break-word}}th{{background:#edf2f8}}.ok{{color:#147a43;font-weight:bold}}.bad{{color:#b42318;font-weight:bold}}</style></head><body><h1>Informe de comparación</h1><p class=\"{}\">Resultado global: {}</p>{}</body></html>", if run.passed { "ok" } else { "bad" }, status(run.passed), body)
}
