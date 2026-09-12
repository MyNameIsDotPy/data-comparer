use crate::compare::compare_all;
use crate::config::{Defaults, Manifest, PairConfig};
use crate::report::write_reports;
use anyhow::Result;
use axum::{
    extract::{DefaultBodyLimit, Multipart},
    http::StatusCode,
    response::Html,
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, net::SocketAddr, path::PathBuf};
use tokio::io::AsyncWriteExt;
use tower_http::services::ServeDir;
use uuid::Uuid;

pub async fn serve(address: &str) -> Result<()> {
    let app = Router::new()
        .route("/", get(index))
        .route("/api/compare", post(compare))
        .nest_service("/reports", ServeDir::new("reports"))
        // Los XLSX pueden superar ampliamente el límite HTTP por defecto de Axum.
        .layer(DefaultBodyLimit::disable());
    let address: SocketAddr = address.parse()?;
    println!("Vista web disponible en http://{address}");
    axum::serve(tokio::net::TcpListener::bind(address).await?, app).await?;
    Ok(())
}

async fn index() -> Html<&'static str> {
    Html(INDEX)
}

async fn compare(mut multipart: Multipart) -> Result<Json<Value>, (StatusCode, String)> {
    let mut files: BTreeMap<String, (Option<PathBuf>, Option<PathBuf>)> = BTreeMap::new();
    let mut tolerance = 0.1;
    let mut row_order = false;
    let mut column_order = true;
    let mut date_format = "%d/%m/%Y".to_string();
    while let Some(mut field) = multipart.next_field().await.map_err(internal)? {
        let name = field.name().unwrap_or_default().to_string();
        if name == "tolerance" {
            tolerance = field.text().await.map_err(internal)?.parse().unwrap_or(0.1);
            continue;
        }
        if name == "row_order" {
            row_order = true;
            continue;
        }
        if name == "column_order" {
            column_order = field.text().await.map_err(internal)? != "false";
            continue;
        }
        if name == "date_format" {
            date_format = field.text().await.map_err(internal)?;
            continue;
        }
        let Some((side, id)) = name.split_once('_') else {
            continue;
        };
        if side != "sas" && side != "spark" {
            continue;
        }
        let file_name = field
            .file_name()
            .unwrap_or("archivo")
            .replace(['/', '\\'], "_");
        let upload_dir = PathBuf::from("reports").join("uploads");
        tokio::fs::create_dir_all(&upload_dir)
            .await
            .map_err(internal)?;
        let path = upload_dir.join(format!("{}-{}", Uuid::new_v4(), file_name));
        let mut output = tokio::fs::File::create(&path).await.map_err(internal)?;
        let mut received = false;
        while let Some(chunk) = field.chunk().await.map_err(internal)? {
            received = true;
            output.write_all(&chunk).await.map_err(internal)?;
        }
        output.flush().await.map_err(internal)?;
        if !received {
            tokio::fs::remove_file(&path).await.map_err(internal)?;
            continue;
        }
        let pair = files.entry(id.to_string()).or_default();
        if side == "sas" {
            pair.0 = Some(path);
        } else {
            pair.1 = Some(path);
        }
    }
    let pairs = files
        .into_iter()
        .enumerate()
        .map(|(index, (_id, (sas, spark)))| match (sas, spark) {
            (Some(sas), Some(spark)) => Ok(PairConfig {
                name: Some(format!("Par {}", index + 1)),
                sas,
                spark,
                compare_row_order: None,
                compare_column_order: None,
                date_format: None,
                columns: Default::default(),
                key_columns: vec![],
                delimiter: None,
                sas_delimiter: None,
                spark_delimiter: None,
            }),
            _ => Err((
                StatusCode::BAD_REQUEST,
                format!("El par {} debe incluir ambos archivos", index + 1),
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if pairs.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Agregue al menos un par de archivos".to_string(),
        ));
    }
    let manifest = Manifest {
        defaults: Defaults {
            numeric_tolerance: tolerance,
            compare_row_order: row_order,
            compare_column_order: column_order,
            date_format,
            ..Defaults::default()
        },
        pairs,
    };
    let work = tokio::task::spawn_blocking(move || {
        let run = compare_all(&manifest.pairs, &manifest.defaults);
        let paths = write_reports(&run, &manifest, &PathBuf::from("reports"));
        (run, paths)
    })
    .await
    .map_err(internal)?;
    let (run, paths) = work;
    let paths = paths.map_err(internal)?;
    let report_url = format!(
        "/reports/{}/report.html",
        paths
            .directory
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
    );
    Ok(Json(
        json!({ "passed": run.passed, "report": report_url, "result": run }),
    ))
}

fn internal(error: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

const INDEX: &str = r#"<!doctype html><html lang="es"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Data Comparer</title><style>body{font-family:system-ui,sans-serif;max-width:900px;margin:2rem auto;padding:0 1rem;background:#f5f7fb;color:#172033}main{background:white;padding:1.5rem;border-radius:12px;box-shadow:0 2px 12px #17203318}.pair{display:grid;grid-template-columns:1fr 1fr auto;gap:1rem;border-top:1px solid #d7dce5;padding:1rem 0}label{display:grid;gap:.35rem;font-weight:600}input{font:inherit}.settings{display:flex;gap:1rem;flex-wrap:wrap;margin:1rem 0}.settings label{display:flex;align-items:center;gap:.4rem}button{background:#155eef;color:white;border:0;border-radius:6px;padding:.65rem 1rem;font-weight:700;cursor:pointer}.remove{background:#667085;align-self:end}#result{margin-top:1rem;padding:1rem;border-radius:6px;white-space:pre-wrap}.ok{background:#dcfae6}.bad{background:#fee4e2}@media(max-width:650px){.pair{grid-template-columns:1fr}.remove{align-self:auto}}</style></head><body><main><h1>Data Comparer</h1><p>Compare salidas SAS y Spark. Los archivos seleccionados se cargan solo en este servidor local.</p><form id="form"><div class="settings"><label>Tolerancia numérica <input name="tolerance" type="number" value="0.1" min="0" step="any"></label><label>Formato fecha <select name="date_format"><option value="%d/%m/%Y">DD/MM/YYYY</option><option value="%Y-%m-%d">YYYY-MM-DD</option></select></label><label><input type="checkbox" name="row_order"> Comparar orden de filas</label><label><input type="checkbox" name="column_order" checked> Comparar orden de columnas</label></div><div id="pairs"></div><p><button type="button" id="add">Agregar par</button> <button>Comparar y generar informe</button></p></form><div id="result" hidden></div></main><template id="pair"><div class="pair"><label>Archivo SAS<input type="file" accept=".csv,.xlsx,.xls,.xlsm,.parquet" required></label><label>Archivo Spark<input type="file" accept=".csv,.xlsx,.xls,.xlsm,.parquet" required></label><button type="button" class="remove">Quitar</button></div></template><script>const pairs=document.querySelector('#pairs'),t=document.querySelector('#pair');function add(){const n=crypto.randomUUID(),e=t.content.cloneNode(true),i=e.querySelectorAll('input[type=file]');i[0].name='sas_'+n;i[1].name='spark_'+n;e.querySelector('.remove').onclick=x=>x.target.parentElement.remove();pairs.append(e)}document.querySelector('#add').onclick=add;add();document.querySelector('#form').onsubmit=async e=>{e.preventDefault();const r=document.querySelector('#result');r.hidden=false;r.className='';r.textContent='Comparando...';try{const x=await fetch('/api/compare',{method:'POST',body:new FormData(e.target)}),d=await x.json();if(!x.ok)throw Error(typeof d==='string'?d:JSON.stringify(d));r.className=d.passed?'ok':'bad';r.innerHTML=(d.passed?'Sin diferencias. ':'Se encontraron diferencias. ')+'<a href="'+d.report+'">Abrir informe</a><br>'+JSON.stringify(d.result,null,2)}catch(x){r.className='bad';r.textContent='Error: '+x.message}};</script></body></html>"#;
