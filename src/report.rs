use crate::compare::{ColumnResult, PairResult, RowSample, RunResult};
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
        "IGUAL"
    } else {
        "DIFERENTE"
    }
}
fn badge(value: bool) -> String {
    format!(
        "<span class=\"badge {}\">{}</span>",
        if value { "good" } else { "bad" },
        status(value)
    )
}
fn count(value: usize) -> String {
    let text = value.to_string();
    text.as_bytes()
        .rchunks(3)
        .rev()
        .map(std::str::from_utf8)
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_default()
        .join(",")
}
fn number(value: Option<f64>) -> String {
    value
        .map(|v| format!("{v:.4}"))
        .unwrap_or_else(|| "-".to_string())
}
fn option_status(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "OK",
        Some(false) => "ERROR",
        None => "-",
    }
}
fn column_issues(column: &ColumnResult) -> Vec<String> {
    let mut issues = Vec::new();
    if !column.types_equal {
        issues.push("tipo".to_string());
    }
    if column.sas_nulls != column.adp_nulls {
        issues.push("nulos".to_string());
    }
    if column.sas_distinct != column.adp_distinct {
        issues.push("distintos".to_string());
    }
    if column.sum_equal == Some(false) {
        issues.push("suma".to_string());
    }
    if column.sas_out_of_range > 0 || column.adp_out_of_range > 0 {
        issues.push("rango".to_string());
    }
    if column.sas_unique_ok == Some(false) || column.adp_unique_ok == Some(false) {
        issues.push("unicidad".to_string());
    }
    if column.sas_nullable_ok == Some(false) || column.adp_nullable_ok == Some(false) {
        issues.push("nulos obligatorios".to_string());
    }
    issues
}
fn scroll_table(content: String) -> String {
    format!("<div class=\"table-scroll\"><table>{content}</table></div>")
}
fn row_samples(title: &str, rows: &[RowSample], side: &str) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let mut table = format!("<tr><th>#</th><th>Fila</th><th>Repeticiones solo en {side}</th></tr>");
    for (index, row) in rows.iter().enumerate() {
        table.push_str(&format!(
            "<tr><td class=num>{}</td><td class=value>{}</td><td class=num>{}</td></tr>",
            index + 1,
            esc(&row.row),
            row.count
        ));
    }
    format!(
        "<details><summary>{title} <span class=\"counter\">{}</span></summary>{}</details>",
        count(rows.len()),
        scroll_table(table)
    )
}
fn column_alerts(pair: &PairResult) -> String {
    let columns = pair
        .column_results
        .iter()
        .filter(|column| !column_issues(column).is_empty())
        .collect::<Vec<_>>();
    if columns.is_empty() {
        return "<div class=\"notice success\">No hay diferencias de calidad o agregados por columna.</div>".to_string();
    }
    let mut table = "<tr><th>Columna SAS</th><th>Columna ADP</th><th>Diferencias</th><th>SAS</th><th>ADP</th></tr>".to_string();
    for column in columns {
        let issues = column_issues(column).join(", ");
        table.push_str(&format!("<tr><td><code>{}</code></td><td><code>{}</code></td><td><span class=\"issue\">{}</span></td><td>nulos: {}<br>distintos: {}<br>suma: {}</td><td>nulos: {}<br>distintos: {}<br>suma: {}</td></tr>", esc(&column.name), esc(&column.adp_name), esc(&issues), column.sas_nulls, column.sas_distinct, number(column.sas_sum), column.adp_nulls, column.adp_distinct, number(column.adp_sum)));
    }
    format!("<details open><summary>Columnas con diferencias <span class=\"counter danger\">{}</span></summary>{}</details>", count(pair.column_results.iter().filter(|column| !column_issues(column).is_empty()).count()), scroll_table(table))
}
fn complete_columns(pair: &PairResult) -> String {
    let mut table = "<tr><th>Columna SAS / ADP</th><th>Tipo</th><th>Nulos SAS / ADP</th><th>Distintos SAS / ADP</th><th>Suma SAS / ADP</th><th>Mín - Máx SAS / ADP</th><th>Media SAS / ADP</th><th>Reglas SAS / ADP</th></tr>".to_string();
    for column in &pair.column_results {
        table.push_str(&format!("<tr><td><code>{}</code><br><code>{}</code></td><td>{} / {}<br>{}</td><td>{} / {}</td><td>{} / {}</td><td>{} / {}</td><td>{} - {}<br>{} - {}</td><td>{} / {}</td><td>rango: {} / {}<br>único: {} / {}<br>nulos: {} / {}</td></tr>", esc(&column.name), esc(&column.adp_name), column.sas_type, column.adp_type, badge(column.types_equal), column.sas_nulls, column.adp_nulls, column.sas_distinct, column.adp_distinct, number(column.sas_sum), number(column.adp_sum), number(column.sas_min), number(column.sas_max), number(column.adp_min), number(column.adp_max), number(column.sas_mean), number(column.adp_mean), column.sas_out_of_range, column.adp_out_of_range, option_status(column.sas_unique_ok), option_status(column.adp_unique_ok), option_status(column.sas_nullable_ok), option_status(column.adp_nullable_ok)));
    }
    format!("<details><summary>Detalle de todas las columnas <span class=\"counter\">{}</span></summary>{}</details>", count(pair.column_results.len()), scroll_table(table))
}
fn schema(pair: &PairResult) -> String {
    let mut html = format!("<details open><summary>Estructura y schema</summary><div class=\"two-col\"><div><h3>Archivos y formatos</h3><dl><dt>SAS</dt><dd><code>{}</code><br>{}</dd><dt>ADP</dt><dd><code>{}</code><br>{}</dd></dl></div><div><h3>Validaciones</h3><dl><dt>Nombres de columnas</dt><dd>{}</dd><dt>Orden de columnas</dt><dd>{}</dd><dt>Orden de filas</dt><dd>{}</dd></dl></div></div>", esc(&pair.formats.sas_format), esc(&pair.formats.sas_details.join(" · ")), esc(&pair.formats.adp_format), esc(&pair.formats.adp_details.join(" · ")), badge(pair.schema.columns_equal), badge(pair.schema.column_order_equal), if pair.row_order_compared { badge(pair.rows_equal) } else { "<span class=\"muted\">No requerido</span>".to_string() });
    if !pair.schema.name_differences.is_empty() {
        html.push_str("<div class=\"notice warning\"><strong>Nombres emparejados tras normalización:</strong><ul>");
        for difference in &pair.schema.name_differences {
            html.push_str(&format!("<li><code>{}</code> en SAS equivale a <code>{}</code> en ADP, diferencia de {}.</li>", esc(&difference.sas), esc(&difference.adp), esc(&difference.kind)));
        }
        html.push_str("</ul></div>");
    }
    if !pair.schema.missing_in_adp.is_empty() {
        html.push_str(&format!(
            "<div class=\"notice error\"><strong>Columnas solo en SAS:</strong> {}</div>",
            esc(&pair.schema.missing_in_adp.join(", "))
        ));
    }
    if !pair.schema.missing_in_sas.is_empty() {
        html.push_str(&format!(
            "<div class=\"notice error\"><strong>Columnas solo en ADP:</strong> {}</div>",
            esc(&pair.schema.missing_in_sas.join(", "))
        ));
    }
    if !pair.schema.ambiguous_columns.is_empty() {
        html.push_str(&format!(
            "<div class=\"notice error\"><strong>Columnas ambiguas:</strong> {}</div>",
            esc(&pair.schema.ambiguous_columns.join(", "))
        ));
    }
    html.push_str("</details>");
    html
}
fn pair_html(index: usize, pair: &PairResult) -> String {
    let differences = pair
        .column_results
        .iter()
        .filter(|column| !column_issues(column).is_empty())
        .count();
    let mut html = format!("<section class=\"card\" id=\"par-{index}\"><header class=\"card-head\"><div><span class=\"pair-number\">PAR {index}</span><h2>{}</h2><p>{} <span>vs</span> {}</p></div>{}</header><div class=\"card-body\">", esc(&pair.name), esc(&pair.sas.file_name), esc(&pair.spark.file_name), badge(pair.passed));
    if let Some(error) = &pair.error {
        return format!(
            "{html}<div class=\"notice error\">{}</div></div></section>",
            esc(error)
        );
    }
    html.push_str(&format!("<div class=\"stat-strip\"><div><span>Filas SAS</span><b>{}</b></div><div><span>Filas ADP</span><b>{}</b></div><div><span>Columnas</span><b>{} / {}</b></div><div><span>Columnas con diferencias</span><b class=\"{}\">{}</b></div><div><span>Filas solo SAS / ADP</span><b>{} / {}</b></div></div>", count(pair.sas.rows), count(pair.spark.rows), count(pair.sas.columns), count(pair.spark.columns), if differences == 0 { "ok-text" } else { "bad-text" }, count(differences), count(pair.rows_only_in_sas), count(pair.rows_only_in_adp)));
    html.push_str(&schema(pair));
    html.push_str(&column_alerts(pair));
    html.push_str(&complete_columns(pair));
    html.push_str(&row_samples(
        "Filas solo en SAS",
        &pair.sas_only_samples,
        "SAS",
    ));
    html.push_str(&row_samples(
        "Filas solo en ADP",
        &pair.adp_only_samples,
        "ADP",
    ));
    if let Some(key) = &pair.key_result {
        let mut table = "<tr><th>Descripción</th><th>SAS</th><th>ADP</th></tr>".to_string();
        table.push_str(&format!("<tr><td>Claves duplicadas</td><td class=num>{}</td><td class=num>{}</td></tr><tr><td>Claves solo en cada archivo</td><td class=num>{}</td><td class=num>{}</td></tr><tr><td colspan=3>Claves con valores diferentes: <strong>{}</strong></td></tr>", key.sas_duplicate_keys, key.adp_duplicate_keys, key.keys_only_in_sas, key.keys_only_in_adp, key.changed_keys));
        for sample in &key.samples {
            table.push_str(&format!(
                "<tr><td>{}</td><td class=value>{}</td><td class=value>{}</td></tr>",
                esc(&sample.location),
                esc(&sample.sas),
                esc(&sample.adp)
            ));
        }
        html.push_str(&format!("<details><summary>Comparación por clave <span class=\"counter\">{}</span></summary><p class=\"muted\">Clave: <code>{}</code></p>{}</details>", count(key.samples.len()), esc(&key.columns.join(", ")), scroll_table(table)));
    }
    html.push_str("</div></section>");
    html
}
pub fn render_html(run: &RunResult) -> String {
    let equal = run.pairs.iter().filter(|pair| pair.passed).count();
    let different = run.pairs.len() - equal;
    let toc = run
        .pairs
        .iter()
        .enumerate()
        .map(|(index, pair)| {
            format!(
                "<li><a href=\"#par-{}\">Par {}: {} vs {}</a></li>",
                index + 1,
                index + 1,
                esc(&pair.sas.file_name),
                esc(&pair.spark.file_name)
            )
        })
        .collect::<String>();
    let mut summary = "<tr><th>#</th><th>Archivo SAS</th><th>Archivo ADP</th><th>Formato SAS / ADP</th><th>Filas SAS / ADP</th><th>Columnas SAS / ADP</th><th>Orden columnas</th><th>Estado</th></tr>".to_string();
    for (index, pair) in run.pairs.iter().enumerate() {
        summary.push_str(&format!("<tr><td><a href=\"#par-{}\">{}</a></td><td class=value>{}</td><td class=value>{}</td><td>{} / {}</td><td class=num>{} / {}</td><td class=num>{} / {}</td><td>{}</td><td>{}</td></tr>", index + 1, index + 1, esc(&pair.sas.file_name), esc(&pair.spark.file_name), esc(&pair.formats.sas_format), esc(&pair.formats.adp_format), count(pair.sas.rows), count(pair.spark.rows), pair.sas.columns, pair.spark.columns, badge(pair.schema.column_order_equal), badge(pair.passed)));
    }
    let detail = run
        .pairs
        .iter()
        .enumerate()
        .map(|(index, pair)| pair_html(index + 1, pair))
        .collect::<String>();
    format!(
        r#"<!doctype html><html lang="es"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Comparación SAS vs ADP</title><style>:root{{--primary:#253a52;--accent:#1976b9;--bg:#f0f3f7;--line:#dce3eb;--text:#25303b;--muted:#697586;--good:#155724;--good-bg:#d8f3df;--bad:#842029;--bad-bg:#f8d7da;--warning:#856404;--warning-bg:#fff3cd}}*{{box-sizing:border-box}}body{{margin:0;background:var(--bg);font:15px/1.5 "Segoe UI",system-ui,sans-serif;color:var(--text)}}.page{{max-width:1480px;margin:auto;padding:26px 20px 70px}}h1{{font-size:1.8rem;color:var(--primary);border-bottom:3px solid var(--accent);padding-bottom:10px;margin:0 0 5px}}h2{{margin:3px 0;font-size:1.14rem;color:var(--primary)}}h3{{font-size:.82rem;text-transform:uppercase;letter-spacing:.06em;color:var(--muted);margin:0 0 7px}}.meta,.muted{{color:var(--muted);font-size:.88rem}}.stats{{display:flex;gap:14px;flex-wrap:wrap;margin:22px 0}}.stat{{background:#fff;border-radius:10px;padding:13px 20px;box-shadow:0 2px 7px #1f293711;min-width:145px}}.stat b{{display:block;font-size:2rem;line-height:1.1}}.stat span{{font-size:.78rem;color:var(--muted)}}.card{{background:#fff;border-radius:10px;box-shadow:0 2px 8px #1f293716;margin:24px 0;overflow:hidden}}.card-head{{background:var(--primary);color:#fff;padding:14px 20px;display:flex;align-items:center;justify-content:space-between;gap:14px}}.card-head h2{{color:#fff}}.card-head p{{margin:2px 0 0;font-size:.85rem;opacity:.85;word-break:break-all}}.card-head p span{{margin:0 6px;opacity:.7}}.pair-number{{font-size:.72rem;letter-spacing:.09em;font-weight:700;opacity:.7}}.card-body{{padding:18px 20px}}.toc{{padding:14px 20px}}.toc ol{{margin:8px 0 0;padding-left:22px}}.toc li{{margin:4px 0;font-size:.9rem}}a{{color:var(--accent);text-decoration:none}}a:hover{{text-decoration:underline}}.badge{{display:inline-block;border-radius:12px;padding:2px 9px;font-size:.75rem;font-weight:750;white-space:nowrap}}.badge.good{{color:var(--good);background:var(--good-bg)}}.badge.bad{{color:var(--bad);background:var(--bad-bg)}}.stat-strip{{display:flex;gap:0;border:1px solid var(--line);border-radius:8px;margin-bottom:16px;overflow:hidden;flex-wrap:wrap}}.stat-strip div{{padding:10px 14px;border-right:1px solid var(--line);flex:1;min-width:145px}}.stat-strip span{{display:block;font-size:.72rem;text-transform:uppercase;letter-spacing:.04em;color:var(--muted)}}.stat-strip b{{font-size:1.1rem}}.ok-text{{color:var(--good)}}.bad-text{{color:var(--bad)}}details{{border:1px solid var(--line);border-radius:8px;padding:7px 12px;margin:10px 0}}summary{{cursor:pointer;color:var(--accent);font-weight:700;list-style:none;padding:3px 0}}summary::before{{content:"▶ ";font-size:.8em}}details[open]>summary::before{{content:"▼ "}}details[open]>summary{{margin-bottom:10px}}.counter{{background:#e8f1f8;color:#315b78;border-radius:10px;padding:1px 7px;font-size:.75rem}}.counter.danger,.issue{{background:var(--bad-bg);color:var(--bad);border-radius:10px;padding:2px 7px;font-size:.78rem;font-weight:650}}.two-col{{display:grid;grid-template-columns:1fr 1fr;gap:16px}}dl{{margin:0}}dt{{font-size:.75rem;text-transform:uppercase;color:var(--muted);font-weight:700;margin-top:7px}}dd{{margin:1px 0 7px}}.notice{{padding:10px 14px;border-radius:6px;margin:10px 0;font-size:.87rem;border-left:4px solid}}.notice ul{{margin:6px 0 0;padding-left:20px}}.notice.success{{background:var(--good-bg);color:var(--good);border-color:#45a05c}}.notice.warning{{background:var(--warning-bg);color:var(--warning);border-color:#ffc107}}.notice.error{{background:var(--bad-bg);color:var(--bad);border-color:#dc3545}}.table-scroll{{overflow-x:auto;border:1px solid #e6ebf0;border-radius:7px;margin-bottom:4px}}table{{width:100%;border-collapse:collapse;font-size:.84rem;min-width:700px}}th{{background:#f7f9fb;color:#445160;font-weight:700}}th,td{{padding:8px 11px;border:1px solid var(--line);text-align:left;vertical-align:top}}tr:hover td{{background:#fafcff}}.num{{text-align:right;font-variant-numeric:tabular-nums;white-space:nowrap}}.value{{word-break:break-word;min-width:180px;max-width:330px}}code{{background:#f1f4f7;border-radius:3px;padding:1px 4px;font:inherit}}@media(max-width:760px){{.page{{padding:18px 10px}}.card-body{{padding:14px}}.two-col{{grid-template-columns:1fr}}.stat-strip div{{min-width:50%;border-bottom:1px solid var(--line)}}}}</style></head><body><main class="page"><h1>Comparación SAS vs ADP</h1><p class="meta">Informe generado por Data Comparer. Seleccione un par para consultar su detalle.</p><div class="stats"><div class="stat"><b>{}</b><span>Total de pares</span></div><div class="stat"><b class="ok-text">{}</b><span>Iguales</span></div><div class="stat"><b class="bad-text">{}</b><span>Con diferencias</span></div></div><nav class="card toc"><strong>Contenido</strong><ol>{}</ol></nav><section class="card"><header class="card-head"><h2>Resumen global</h2></header><div class="card-body">{}</div></section><h2>Detalle por par</h2>{}</main></body></html>"#,
        count(run.pairs.len()),
        count(equal),
        count(different),
        toc,
        scroll_table(summary),
        detail
    )
}
