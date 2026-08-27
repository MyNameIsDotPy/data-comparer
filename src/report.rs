use crate::compare::{PairResult, RunResult};
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
fn number(value: Option<f64>) -> String {
    value
        .map(|v| format!("{v:.6}"))
        .unwrap_or_else(|| "-".to_string())
}
fn samples(title: &str, rows: &[crate::compare::RowSample], source: &str) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let mut html = format!("<details><summary>{title} <span class=badge>{}</span></summary><table><tr><th>Fila</th><th>Repeticiones solo en {source}</th></tr>", rows.len());
    for row in rows {
        html.push_str(&format!(
            "<tr><td>{}</td><td>{}</td></tr>",
            esc(&row.row),
            row.count
        ));
    }
    html.push_str("</table></details>");
    html
}
fn pair_html(pair: &PairResult) -> String {
    let mut html = format!("<article class=pair><header><div><p class=eyebrow>COMPARACIÓN</p><h2>{}</h2><p class=paths>SAS: {}<br>ADP: {}</p></div><span class=state {}>{}</span></header>", esc(&pair.name), esc(&pair.sas.path), esc(&pair.spark.path), if pair.passed { "pass" } else { "fail" }, status(pair.passed));
    if let Some(error) = &pair.error {
        return format!("{html}<p class=error>{}</p></article>", esc(error));
    }
    html.push_str(&format!("<div class=metrics><div><span>Filas</span><b>{} / {}</b><em>{}</em></div><div><span>Columnas</span><b>{} / {}</b><em>{}</em></div><div><span>Solo SAS</span><b>{}</b><em>filas</em></div><div><span>Solo ADP</span><b>{}</b><em>filas</em></div></div>", pair.sas.rows, pair.spark.rows, status(pair.row_count_equal), pair.sas.columns, pair.spark.columns, status(pair.schema.columns_equal), pair.rows_only_in_sas, pair.rows_only_in_adp));
    html.push_str("<details open><summary>Schema y nombres de columnas</summary><table><tr><th>Regla</th><th>Resultado</th></tr>");
    html.push_str(&format!("<tr><td>Formato de archivo</td><td>SAS: <code>{}</code> ({})<br>ADP: <code>{}</code> ({})</td></tr>", esc(&pair.formats.sas_format), esc(&pair.formats.sas_details.join("; ")), esc(&pair.formats.adp_format), esc(&pair.formats.adp_details.join("; "))));
    html.push_str(&format!("<tr><td>Orden de columnas</td><td>{}</td></tr><tr><td>Orden de filas {}</td><td>{}</td></tr>", status(pair.schema.column_order_equal), if pair.row_order_compared { "comparado" } else { "no requerido" }, status(pair.rows_equal)));
    if !pair.schema.name_differences.is_empty() {
        html.push_str("<tr><td>Diferencias de nombre normalizadas</td><td>");
        for difference in &pair.schema.name_differences {
            html.push_str(&format!(
                "<code>{}</code> / <code>{}</code> ({})<br>",
                esc(&difference.sas),
                esc(&difference.adp),
                esc(&difference.kind)
            ));
        }
        html.push_str("</td></tr>");
    }
    if !pair.schema.missing_in_adp.is_empty() {
        html.push_str(&format!(
            "<tr><td>Solo en SAS</td><td>{}</td></tr>",
            esc(&pair.schema.missing_in_adp.join(", "))
        ));
    }
    if !pair.schema.missing_in_sas.is_empty() {
        html.push_str(&format!(
            "<tr><td>Solo en ADP</td><td>{}</td></tr>",
            esc(&pair.schema.missing_in_sas.join(", "))
        ));
    }
    if !pair.schema.ambiguous_columns.is_empty() {
        html.push_str(&format!(
            "<tr><td>Columnas ambiguas</td><td>{}</td></tr>",
            esc(&pair.schema.ambiguous_columns.join(", "))
        ));
    }
    html.push_str("</table></details>");
    html.push_str("<details><summary>Calidad y agregados por columna</summary><table><tr><th>Columna SAS / ADP</th><th>Tipo SAS / ADP</th><th>Nulos</th><th>Distintos</th><th>Suma</th><th>Mín / Máx</th><th>Media</th><th>Longitud texto</th><th>Reglas</th></tr>");
    for column in &pair.column_results {
        let rules = format!(
            "rango: {}/{}; único: {}/{}; nulos: {}/{}",
            column.sas_out_of_range,
            column.adp_out_of_range,
            column.sas_unique_ok.map(status).unwrap_or("-"),
            column.adp_unique_ok.map(status).unwrap_or("-"),
            column.sas_nullable_ok.map(status).unwrap_or("-"),
            column.adp_nullable_ok.map(status).unwrap_or("-")
        );
        html.push_str(&format!("<tr><td><code>{}</code> / <code>{}</code></td><td>{} / {} {}</td><td>{} / {}</td><td>{} / {}</td><td>{} / {} {}</td><td>{} / {}</td><td>{} / {}</td><td>{:?}-{:?} / {:?}-{:?}</td><td>{}</td></tr>", esc(&column.name), esc(&column.adp_name), column.sas_type, column.adp_type, status(column.types_equal), column.sas_nulls, column.adp_nulls, column.sas_distinct, column.adp_distinct, number(column.sas_sum), number(column.adp_sum), column.sum_equal.map(status).unwrap_or("-"), number(column.sas_min), number(column.sas_max), number(column.sas_mean), number(column.adp_mean), column.sas_text_min_length, column.sas_text_max_length, column.adp_text_min_length, column.adp_text_max_length, rules));
    }
    html.push_str("</table></details>");
    html.push_str(&samples("Filas solo en SAS", &pair.sas_only_samples, "SAS"));
    html.push_str(&samples("Filas solo en ADP", &pair.adp_only_samples, "ADP"));
    if let Some(key) = &pair.key_result {
        html.push_str(&format!("<details><summary>Comparación por clave <span class=badge>{}</span></summary><p>Claves: <code>{}</code>. Duplicadas SAS/ADP: {}/{}. Solo SAS/ADP: {}/{}. Con valores distintos: {}.</p>", key.samples.len(), esc(&key.columns.join(", ")), key.sas_duplicate_keys, key.adp_duplicate_keys, key.keys_only_in_sas, key.keys_only_in_adp, key.changed_keys));
        if !key.samples.is_empty() {
            html.push_str("<table><tr><th>Clave</th><th>SAS</th><th>ADP</th></tr>");
            for row in &key.samples {
                html.push_str(&format!(
                    "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
                    esc(&row.location),
                    esc(&row.sas),
                    esc(&row.adp)
                ));
            }
            html.push_str("</table>");
        }
        html.push_str("</details>");
    }
    if !pair.samples.is_empty() {
        html.push_str("<details><summary>Otras diferencias de filas</summary><table><tr><th>Ubicación</th><th>SAS</th><th>ADP</th></tr>");
        for row in &pair.samples {
            html.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
                esc(&row.location),
                esc(&row.sas),
                esc(&row.adp)
            ));
        }
        html.push_str("</table></details>");
    }
    html.push_str("</article>");
    html
}
pub fn render_html(run: &RunResult) -> String {
    let body = run.pairs.iter().map(pair_html).collect::<String>();
    format!(
        r#"<!doctype html><html lang="es"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Informe Data Comparer</title><style>:root{{--ink:#172033;--muted:#667085;--line:#d9e1ee;--paper:#fff;--canvas:#f4f7fb;--blue:#155eef;--green:#067647;--red:#b42318}}*{{box-sizing:border-box}}body{{margin:0;background:var(--canvas);font:15px/1.45 Inter,ui-sans-serif,system-ui,sans-serif;color:var(--ink)}}main{{max-width:1320px;margin:auto;padding:42px 24px 80px}}h1{{font-size:clamp(2rem,4vw,3.5rem);letter-spacing:-.055em;margin:0}}h2{{margin:0;font-size:1.25rem}}.lead{{color:var(--muted);margin:.4rem 0 2rem}}.pair{{background:var(--paper);border:1px solid var(--line);border-radius:14px;margin:18px 0;overflow:hidden;box-shadow:0 8px 28px #1720330b}}article>header{{padding:22px 24px 16px;display:flex;justify-content:space-between;gap:1rem;border-bottom:1px solid var(--line)}}.eyebrow{{font-size:.7rem;font-weight:800;letter-spacing:.12em;color:var(--blue);margin:0 0 4px}}.paths{{margin:7px 0 0;color:var(--muted);font-size:.82rem;word-break:break-all}}.state{{height:max-content;padding:5px 9px;border-radius:999px;font-size:.7rem;font-weight:800;letter-spacing:.08em}}.pass{{color:var(--green);background:#d1fadf}}.fail{{color:var(--red);background:#fee4e2}}.metrics{{display:grid;grid-template-columns:repeat(4,1fr);border-bottom:1px solid var(--line)}}.metrics div{{padding:15px 24px;border-right:1px solid var(--line)}}.metrics div:last-child{{border:0}}.metrics span,.metrics em{{display:block;color:var(--muted);font-size:.74rem;font-style:normal}}.metrics b{{font-size:1.2rem}}details{{margin:0 24px;border-bottom:1px solid var(--line)}}details:last-child{{border-bottom:0}}summary{{cursor:pointer;padding:15px 0;font-weight:750;list-style:none}}summary::-webkit-details-marker{{display:none}}summary:before{{content:'+';display:inline-grid;place-items:center;width:18px;height:18px;margin-right:8px;border-radius:50%;background:#eaf0ff;color:var(--blue)}}details[open]>summary:before{{content:'-'}}table{{width:100%;border-collapse:collapse;margin:0 0 18px;font-size:.82rem}}th{{color:var(--muted);background:#f8fafc;font-weight:700}}th,td{{padding:9px;border:1px solid var(--line);text-align:left;vertical-align:top;word-break:break-word}}code{{font:inherit;color:#344054;background:#f2f4f7;padding:1px 4px;border-radius:3px}}.badge{{background:#eaf0ff;color:var(--blue);padding:2px 6px;border-radius:99px;font-size:.7rem}}.error{{padding:24px;color:var(--red)}}@media(max-width:720px){{main{{padding:28px 12px}}article>header{{padding:18px;display:block}}.state{{display:inline-block;margin-top:12px}}.metrics{{grid-template-columns:1fr 1fr}}.metrics div{{padding:12px;border-bottom:1px solid var(--line)}}details{{margin:0 14px}}table{{display:block;overflow:auto;white-space:nowrap}}}}</style></head><body><main><p class=eyebrow>DATA COMPARER</p><h1>Informe de validación</h1><p class="lead">Resultado global: <strong class="{}">{}</strong>. Despliegue cada sección para revisar schema, calidad y diferencias.</p>{}</main></body></html>"#,
        if run.passed { "pass" } else { "fail" },
        status(run.passed),
        body
    )
}
