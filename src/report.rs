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
/// Total de columnas y cuántas de ellas tienen al menos una diferencia.
fn column_diff_counts(pair: &PairResult) -> (usize, usize) {
    let total = pair.column_results.len();
    let differing = pair
        .column_results
        .iter()
        .filter(|column| !column_issues(column).is_empty())
        .count();
    (differing, total)
}
/// Barra de proporción compacta: cuánto rojo (diferencias) hay sobre el total de columnas.
fn diff_bar(differing: usize, total: usize) -> String {
    if total == 0 {
        return "<span class=\"muted\">—</span>".to_string();
    }
    let pct = ((differing as f64 / total as f64) * 100.0).round() as u32;
    format!(
        "<span class=\"bar\" title=\"{} de {} columnas con diferencias\"><span class=\"bar-fill\" style=\"width:{}%\"></span></span><span class=\"bar-label\">{}/{}</span>",
        differing, total, pct, differing, total
    )
}
fn cell(bad: bool, content: String) -> String {
    format!(
        "<td class=\"{}\">{}</td>",
        if bad { "cell-bad" } else { "cell-ok" },
        content
    )
}
fn pairv(sas: String, adp: String, differ: bool) -> String {
    if differ {
        format!("{sas} → <mark>{adp}</mark>")
    } else {
        format!("{sas} / {adp}")
    }
}
fn delta_tag(delta: String) -> String {
    format!("<span class=\"delta\" title=\"Diferencia ADP − SAS\">Δ {delta}</span>")
}
/// Par SAS/ADP para conteos enteros (nulos, distintos): cuando difieren, agrega la
/// diferencia numérica ADP − SAS para no obligar al lector a restar mentalmente.
fn numeric_pair_usize(sas: usize, adp: usize) -> String {
    if sas == adp {
        format!("{sas} / {adp}")
    } else {
        let delta = adp as i64 - sas as i64;
        format!(
            "{sas} → <mark>{adp}</mark> {}",
            delta_tag(format!("{delta:+}"))
        )
    }
}
/// Par SAS/ADP para métricas decimales (suma, ...): cuando `differ` es verdadero,
/// agrega la diferencia numérica ADP − SAS junto a los dos valores.
fn numeric_pair_f64(sas: Option<f64>, adp: Option<f64>, differ: bool) -> String {
    if !differ {
        return format!("{} / {}", number(sas), number(adp));
    }
    let base = format!("{} → <mark>{}</mark>", number(sas), number(adp));
    match (sas, adp) {
        (Some(a), Some(b)) => format!("{base} {}", delta_tag(format!("{:+.4}", b - a))),
        _ => base,
    }
}
fn rule_line(label: &str, value: String, bad: bool) -> String {
    if bad {
        format!("<span class=\"v-bad\">{label}: {value}</span>")
    } else {
        format!("{label}: {value}")
    }
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
/// Mapa visual de columnas: un cuadro verde/rojo por columna para detectar en un
/// vistazo cuántas y cuáles columnas tienen diferencias antes de abrir el detalle.
fn column_diff_map(pair: &PairResult) -> String {
    if pair.column_results.is_empty() {
        return String::new();
    }
    let cells = pair
        .column_results
        .iter()
        .map(|column| {
            let issues = column_issues(column);
            let ok = issues.is_empty();
            let detail = if ok {
                "sin diferencias".to_string()
            } else {
                issues.join(", ")
            };
            format!(
                "<span class=\"diffmap-item {}\" title=\"{}: {}\">{}</span>",
                if ok { "ok" } else { "bad" },
                esc(&column.name),
                esc(&detail),
                if ok { "●" } else { "✕" }
            )
        })
        .collect::<String>();
    let (differing, total) = column_diff_counts(pair);
    format!(
        "<div class=\"diffmap\"><span class=\"diffmap-label\">Mapa de columnas · {} de {} con diferencias</span><div class=\"diffmap-grid\">{}</div></div>",
        count(differing),
        count(total),
        cells
    )
}
/// Matriz de columnas con diferencias: una fila por columna, una celda por tipo de
/// diferencia, resaltada en rojo cuando aplica. Reemplaza el antiguo listado de texto.
fn column_matrix(pair: &PairResult) -> String {
    let columns = pair
        .column_results
        .iter()
        .filter(|column| !column_issues(column).is_empty())
        .collect::<Vec<_>>();
    if columns.is_empty() {
        return "<div class=\"notice success\">No hay diferencias de calidad o agregados por columna.</div>".to_string();
    }
    let mut table = "<tr><th>Columna</th><th>Tipo SAS → ADP</th><th>Nulos SAS → ADP</th><th>Distintos SAS → ADP</th><th>Suma SAS → ADP</th><th>Rango (fuera) SAS / ADP</th><th>Unicidad SAS / ADP</th><th>Nulos oblig. SAS / ADP</th></tr>".to_string();
    let total = columns.len();
    for column in columns {
        let rename = if column.name != column.adp_name {
            format!(
                "<br><span class=\"muted\">→ <code>{}</code></span>",
                esc(&column.adp_name)
            )
        } else {
            String::new()
        };
        table.push_str(&format!(
            "<tr><td><code>{}</code>{}</td>{}{}{}{}{}{}{}</tr>",
            esc(&column.name),
            rename,
            cell(
                !column.types_equal,
                format!("{} → {}", esc(&column.sas_type), esc(&column.adp_type))
            ),
            cell(
                column.sas_nulls != column.adp_nulls,
                numeric_pair_usize(column.sas_nulls, column.adp_nulls)
            ),
            cell(
                column.sas_distinct != column.adp_distinct,
                numeric_pair_usize(column.sas_distinct, column.adp_distinct)
            ),
            cell(
                column.sum_equal == Some(false),
                numeric_pair_f64(column.sas_sum, column.adp_sum, column.sum_equal == Some(false))
            ),
            cell(
                column.sas_out_of_range > 0 || column.adp_out_of_range > 0,
                format!("{} / {}", column.sas_out_of_range, column.adp_out_of_range)
            ),
            cell(
                column.sas_unique_ok == Some(false) || column.adp_unique_ok == Some(false),
                format!(
                    "{} / {}",
                    option_status(column.sas_unique_ok),
                    option_status(column.adp_unique_ok)
                )
            ),
            cell(
                column.sas_nullable_ok == Some(false) || column.adp_nullable_ok == Some(false),
                format!(
                    "{} / {}",
                    option_status(column.sas_nullable_ok),
                    option_status(column.adp_nullable_ok)
                )
            ),
        ));
    }
    format!(
        "<details open><summary>Columnas con diferencias <span class=\"counter danger\">{}</span></summary>{}</details>",
        count(total),
        scroll_table(table)
    )
}
fn complete_columns(pair: &PairResult) -> String {
    let mut table = "<tr><th>Columna SAS / ADP</th><th>Tipo</th><th>Nulos</th><th>Distintos</th><th>Suma</th><th>Mín – Máx SAS<br>Mín – Máx ADP</th><th>Media</th><th>Reglas</th></tr>".to_string();
    for column in &pair.column_results {
        let type_cell = pairv(
            esc(&column.sas_type),
            esc(&column.adp_type),
            !column.types_equal,
        );
        let nulls = numeric_pair_usize(column.sas_nulls, column.adp_nulls);
        let distinct = numeric_pair_usize(column.sas_distinct, column.adp_distinct);
        let sum = numeric_pair_f64(column.sas_sum, column.adp_sum, column.sum_equal == Some(false));
        let mean = pairv(number(column.sas_mean), number(column.adp_mean), false);
        let rules = format!(
            "{}<br>{}<br>{}",
            rule_line(
                "rango",
                format!("{} / {}", column.sas_out_of_range, column.adp_out_of_range),
                column.sas_out_of_range > 0 || column.adp_out_of_range > 0
            ),
            rule_line(
                "único",
                format!(
                    "{} / {}",
                    option_status(column.sas_unique_ok),
                    option_status(column.adp_unique_ok)
                ),
                column.sas_unique_ok == Some(false) || column.adp_unique_ok == Some(false)
            ),
            rule_line(
                "nulos",
                format!(
                    "{} / {}",
                    option_status(column.sas_nullable_ok),
                    option_status(column.adp_nullable_ok)
                ),
                column.sas_nullable_ok == Some(false) || column.adp_nullable_ok == Some(false)
            ),
        );
        table.push_str(&format!(
            "<tr><td><code>{}</code><br><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{} – {}<br><span class=\"muted\">{} – {}</span></td><td>{}</td><td>{}</td></tr>",
            esc(&column.name),
            esc(&column.adp_name),
            type_cell,
            nulls,
            distinct,
            sum,
            number(column.sas_min),
            number(column.sas_max),
            number(column.adp_min),
            number(column.adp_max),
            mean,
            rules,
        ));
    }
    format!(
        "<details><summary>Detalle de todas las columnas <span class=\"counter\">{}</span></summary>{}</details>",
        count(pair.column_results.len()),
        scroll_table(table)
    )
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
/// Diferencias fila a fila cuando se compara por orden posicional: reconstruye cada
/// fila de la muestra y resalta en rojo únicamente las columnas cuyo valor cambió,
/// en vez de un bloque de texto plano difícil de comparar.
fn row_diff_samples(pair: &PairResult) -> String {
    if !pair.row_order_compared || pair.samples.is_empty() {
        return String::new();
    }
    let names = pair
        .column_results
        .iter()
        .map(|column| column.name.as_str())
        .collect::<Vec<_>>();
    let mut table = "<tr><th>Ubicación</th>".to_string();
    for name in &names {
        table.push_str(&format!("<th>{}</th>", esc(name)));
    }
    table.push_str("</tr>");
    for sample in &pair.samples {
        let sas_values = sample.sas.split(" | ").collect::<Vec<_>>();
        let adp_values = sample.adp.split(" | ").collect::<Vec<_>>();
        if names.is_empty() || sas_values.len() != names.len() || adp_values.len() != names.len()
        {
            table.push_str(&format!(
                "<tr><td class=\"value\" colspan=\"{}\"><strong>{}</strong><br>SAS: {}<br>ADP: {}</td></tr>",
                names.len() + 1,
                esc(&sample.location),
                esc(&sample.sas),
                esc(&sample.adp)
            ));
            continue;
        }
        let mut cells = format!("<td class=\"value\">{}</td>", esc(&sample.location));
        for (sas_value, adp_value) in sas_values.iter().zip(adp_values.iter()) {
            if sas_value == adp_value {
                cells.push_str(&format!("<td class=\"value\">{}</td>", esc(sas_value)));
            } else {
                cells.push_str(&format!(
                    "<td class=\"value cell-bad\">{}<br><span class=\"muted\">→ {}</span></td>",
                    esc(sas_value),
                    esc(adp_value)
                ));
            }
        }
        table.push_str(&format!("<tr>{cells}</tr>"));
    }
    format!(
        "<details><summary>Diferencias fila a fila <span class=\"counter danger\">{}</span></summary><p class=\"muted\">Se resalta en rojo cada celda cuyo valor cambia entre SAS y ADP dentro de la fila.</p>{}</details>",
        count(pair.samples.len()),
        scroll_table(table)
    )
}
fn pair_html(index: usize, pair: &PairResult) -> String {
    let (differences, _total_columns) = column_diff_counts(pair);
    let mut html = format!("<section class=\"card\" id=\"par-{index}\"><header class=\"card-head\"><div><span class=\"pair-number\">PAR {index}</span><h2>{}</h2><p>{} <span>vs</span> {}</p></div>{}</header><div class=\"card-body\">", esc(&pair.name), esc(&pair.sas.file_name), esc(&pair.spark.file_name), badge(pair.passed));
    if let Some(error) = &pair.error {
        return format!(
            "{html}<div class=\"notice error\">{}</div></div></section>",
            esc(error)
        );
    }
    html.push_str(&format!("<div class=\"stat-strip\"><div><span>Filas SAS</span><b>{}</b></div><div><span>Filas ADP</span><b>{}</b></div><div><span>Columnas</span><b>{} / {}</b></div><div><span>Columnas con diferencias</span><b class=\"{}\">{}</b></div><div><span>Filas solo SAS / ADP</span><b>{} / {}</b></div></div>", count(pair.sas.rows), count(pair.spark.rows), count(pair.sas.columns), count(pair.spark.columns), if differences == 0 { "ok-text" } else { "bad-text" }, count(differences), count(pair.rows_only_in_sas), count(pair.rows_only_in_adp)));
    html.push_str(&column_diff_map(pair));
    html.push_str(&schema(pair));
    html.push_str(&column_matrix(pair));
    html.push_str(&complete_columns(pair));
    html.push_str(&row_diff_samples(pair));
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
    let legend = "<div class=\"legend\"><span class=\"legend-item\"><span class=\"diffmap-item ok\">●</span> Columna sin diferencias</span><span class=\"legend-item\"><span class=\"diffmap-item bad\">✕</span> Columna con diferencias</span><span class=\"legend-item\"><span class=\"bar\"><span class=\"bar-fill\" style=\"width:45%\"></span></span> Proporción de columnas con diferencias</span><span class=\"legend-item\"><mark>valor</mark> Métrica distinta entre SAS y ADP</span><span class=\"legend-item\"><span class=\"delta\">Δ +12.5</span> Diferencia numérica (ADP − SAS)</span></div>".to_string();
    let mut summary = "<tr><th>#</th><th>Archivo SAS</th><th>Archivo ADP</th><th>Formato SAS / ADP</th><th>Filas SAS / ADP</th><th>Columnas SAS / ADP</th><th>Orden columnas</th><th>Columnas con diferencias</th><th>Estado</th></tr>".to_string();
    for (index, pair) in run.pairs.iter().enumerate() {
        let (differences, total_columns) = column_diff_counts(pair);
        summary.push_str(&format!("<tr><td><a href=\"#par-{}\">{}</a></td><td class=value>{}</td><td class=value>{}</td><td>{} / {}</td><td class=num>{} / {}</td><td class=num>{} / {}</td><td>{}</td><td>{}</td><td>{}</td></tr>", index + 1, index + 1, esc(&pair.sas.file_name), esc(&pair.spark.file_name), esc(&pair.formats.sas_format), esc(&pair.formats.adp_format), count(pair.sas.rows), count(pair.spark.rows), pair.sas.columns, pair.spark.columns, badge(pair.schema.column_order_equal), diff_bar(differences, total_columns), badge(pair.passed)));
    }
    let detail = run
        .pairs
        .iter()
        .enumerate()
        .map(|(index, pair)| pair_html(index + 1, pair))
        .collect::<String>();
    format!(
        r#"<!doctype html><html lang="es"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Comparación SAS vs ADP</title><style>:root{{--primary:#253a52;--accent:#1976b9;--bg:#f0f3f7;--line:#dce3eb;--text:#25303b;--muted:#697586;--good:#155724;--good-bg:#d8f3df;--bad:#842029;--bad-bg:#f8d7da;--warning:#856404;--warning-bg:#fff3cd}}*{{box-sizing:border-box}}body{{margin:0;background:var(--bg);font:15px/1.5 "Segoe UI",system-ui,sans-serif;color:var(--text)}}.page{{max-width:1480px;margin:auto;padding:26px 20px 70px}}h1{{font-size:1.8rem;color:var(--primary);border-bottom:3px solid var(--accent);padding-bottom:10px;margin:0 0 5px}}h2{{margin:3px 0;font-size:1.14rem;color:var(--primary)}}h3{{font-size:.82rem;text-transform:uppercase;letter-spacing:.06em;color:var(--muted);margin:0 0 7px}}.meta,.muted{{color:var(--muted);font-size:.88rem}}.stats{{display:flex;gap:14px;flex-wrap:wrap;margin:22px 0}}.stat{{background:#fff;border-radius:10px;padding:13px 20px;box-shadow:0 2px 7px #1f293711;min-width:145px}}.stat b{{display:block;font-size:2rem;line-height:1.1}}.stat span{{font-size:.78rem;color:var(--muted)}}.card{{background:#fff;border-radius:10px;box-shadow:0 2px 8px #1f293716;margin:24px 0;overflow:hidden}}.card-head{{background:var(--primary);color:#fff;padding:14px 20px;display:flex;align-items:center;justify-content:space-between;gap:14px}}.card-head h2{{color:#fff}}.card-head p{{margin:2px 0 0;font-size:.85rem;opacity:.85;word-break:break-all}}.card-head p span{{margin:0 6px;opacity:.7}}.pair-number{{font-size:.72rem;letter-spacing:.09em;font-weight:700;opacity:.7}}.card-body{{padding:18px 20px}}.toc{{padding:14px 20px}}.toc ol{{margin:8px 0 0;padding-left:22px}}.toc li{{margin:4px 0;font-size:.9rem}}a{{color:var(--accent);text-decoration:none}}a:hover{{text-decoration:underline}}.badge{{display:inline-block;border-radius:12px;padding:2px 9px;font-size:.75rem;font-weight:750;white-space:nowrap}}.badge.good{{color:var(--good);background:var(--good-bg)}}.badge.bad{{color:var(--bad);background:var(--bad-bg)}}.stat-strip{{display:flex;gap:0;border:1px solid var(--line);border-radius:8px;margin-bottom:16px;overflow:hidden;flex-wrap:wrap}}.stat-strip div{{padding:10px 14px;border-right:1px solid var(--line);flex:1;min-width:145px}}.stat-strip span{{display:block;font-size:.72rem;text-transform:uppercase;letter-spacing:.04em;color:var(--muted)}}.stat-strip b{{font-size:1.1rem}}.ok-text{{color:var(--good)}}.bad-text{{color:var(--bad)}}details{{border:1px solid var(--line);border-radius:8px;padding:7px 12px;margin:10px 0}}summary{{cursor:pointer;color:var(--accent);font-weight:700;list-style:none;padding:3px 0}}summary::before{{content:"▶ ";font-size:.8em}}details[open]>summary::before{{content:"▼ "}}details[open]>summary{{margin-bottom:10px}}.counter{{background:#e8f1f8;color:#315b78;border-radius:10px;padding:1px 7px;font-size:.75rem}}.counter.danger,.issue{{background:var(--bad-bg);color:var(--bad);border-radius:10px;padding:2px 7px;font-size:.78rem;font-weight:650}}.two-col{{display:grid;grid-template-columns:1fr 1fr;gap:16px}}dl{{margin:0}}dt{{font-size:.75rem;text-transform:uppercase;color:var(--muted);font-weight:700;margin-top:7px}}dd{{margin:1px 0 7px}}.notice{{padding:10px 14px;border-radius:6px;margin:10px 0;font-size:.87rem;border-left:4px solid}}.notice ul{{margin:6px 0 0;padding-left:20px}}.notice.success{{background:var(--good-bg);color:var(--good);border-color:#45a05c}}.notice.warning{{background:var(--warning-bg);color:var(--warning);border-color:#ffc107}}.notice.error{{background:var(--bad-bg);color:var(--bad);border-color:#dc3545}}.table-scroll{{overflow-x:auto;border:1px solid #e6ebf0;border-radius:7px;margin-bottom:4px}}table{{width:100%;border-collapse:collapse;font-size:.84rem;min-width:700px}}th{{background:#f7f9fb;color:#445160;font-weight:700}}th,td{{padding:8px 11px;border:1px solid var(--line);text-align:left;vertical-align:top}}tr:hover td{{background:#fafcff}}.num{{text-align:right;font-variant-numeric:tabular-nums;white-space:nowrap}}.value{{word-break:break-word;min-width:180px;max-width:330px}}code{{background:#f1f4f7;border-radius:3px;padding:1px 4px;font:inherit}}.legend{{display:flex;flex-wrap:wrap;gap:16px 22px;margin:10px 0 22px;font-size:.82rem;color:var(--muted)}}.legend-item{{display:inline-flex;align-items:center;gap:6px}}.diffmap{{margin:14px 0}}.diffmap-label{{display:block;font-size:.72rem;text-transform:uppercase;letter-spacing:.04em;color:var(--muted);margin-bottom:6px}}.diffmap-grid{{display:flex;flex-wrap:wrap;gap:4px}}.diffmap-item{{display:inline-flex;align-items:center;justify-content:center;width:22px;height:22px;border-radius:5px;font-size:.7rem;font-weight:700}}.diffmap-item.ok{{background:var(--good-bg);color:var(--good)}}.diffmap-item.bad{{background:var(--bad-bg);color:var(--bad)}}.cell-bad{{background:var(--bad-bg);color:var(--bad);font-weight:650}}.cell-ok{{color:var(--muted)}}mark{{background:var(--bad-bg);color:var(--bad);padding:0 3px;border-radius:3px;font-weight:700;font-family:inherit}}.v-bad{{color:var(--bad);font-weight:700}}.delta{{display:inline-block;background:var(--bad-bg);color:var(--bad);border-radius:8px;padding:0 6px;font-size:.72rem;font-weight:700;white-space:nowrap;margin-left:4px;font-variant-numeric:tabular-nums}}.bar{{position:relative;display:inline-block;width:64px;height:8px;border-radius:4px;background:var(--good-bg);overflow:hidden;vertical-align:middle;margin-right:6px}}.bar-fill{{position:absolute;inset:0 auto 0 0;background:var(--bad)}}.bar-label{{font-size:.78rem;color:var(--muted);vertical-align:middle;font-variant-numeric:tabular-nums}}@media(max-width:760px){{.page{{padding:18px 10px}}.card-body{{padding:14px}}.two-col{{grid-template-columns:1fr}}.stat-strip div{{min-width:50%;border-bottom:1px solid var(--line)}}.legend{{gap:10px 16px}}}}</style></head><body><main class="page"><h1>Comparación SAS vs ADP</h1><p class="meta">Informe generado por Data Comparer. Seleccione un par para consultar su detalle.</p>{}<div class="stats"><div class="stat"><b>{}</b><span>Total de pares</span></div><div class="stat"><b class="ok-text">{}</b><span>Iguales</span></div><div class="stat"><b class="bad-text">{}</b><span>Con diferencias</span></div></div><nav class="card toc"><strong>Contenido</strong><ol>{}</ol></nav><section class="card"><header class="card-head"><h2>Resumen global</h2></header><div class="card-body">{}</div></section><h2>Detalle por par</h2>{}</main></body></html>"#,
        legend,
        count(run.pairs.len()),
        count(equal),
        count(different),
        toc,
        scroll_table(summary),
        detail
    )
}
