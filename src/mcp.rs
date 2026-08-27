use crate::compare::compare_all;
use crate::config::{Defaults, Manifest, PairConfig};
use crate::report::write_reports;
use anyhow::Result;
use serde_json::{json, Value};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub async fn serve() -> Result<()> {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut output = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        let request: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let Some(id) = request.get("id") else {
            continue;
        };
        let response = match request.get("method").and_then(Value::as_str) {
            Some("initialize") => {
                json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"data-comparer","version":"0.1.0"}}})
            }
            Some("tools/list") => {
                json!({"jsonrpc":"2.0","id":id,"result":{"tools":[tool("compare_files", "Compara dos archivos locales CSV, XLSX o Parquet."), tool("compare_batch", "Ejecuta un manifiesto YAML local.") ]}})
            }
            Some("tools/call") => {
                json!({"jsonrpc":"2.0","id":id,"result": call(request.get("params").unwrap_or(&Value::Null))})
            }
            _ => {
                json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Método no soportado"}})
            }
        };
        output
            .write_all(serde_json::to_string(&response)?.as_bytes())
            .await?;
        output.write_all(b"\n").await?;
        output.flush().await?;
    }
    Ok(())
}

fn tool(name: &str, description: &str) -> Value {
    json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":{"sas":{"type":"string"},"spark":{"type":"string"},"manifest":{"type":"string"},"row_order":{"type":"boolean"},"tolerance":{"type":"number"},"key_columns":{"type":"array","items":{"type":"string"}},"trim_values":{"type":"boolean"},"case_insensitive_values":{"type":"boolean"}},"additionalProperties":false}})
}
fn call(params: &Value) -> Value {
    let result = (|| -> Result<Value> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let args = params.get("arguments").unwrap_or(&Value::Null);
        let manifest = match name {
            "compare_files" => Manifest {
                defaults: Defaults {
                    compare_row_order: args
                        .get("row_order")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    numeric_tolerance: args.get("tolerance").and_then(Value::as_f64).unwrap_or(0.1),
                    trim_values: args
                        .get("trim_values")
                        .and_then(Value::as_bool)
                        .unwrap_or(true),
                    case_insensitive_values: args
                        .get("case_insensitive_values")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    ..Defaults::default()
                },
                pairs: vec![PairConfig {
                    name: None,
                    sas: PathBuf::from(
                        args.get("sas")
                            .and_then(Value::as_str)
                            .ok_or_else(|| anyhow::anyhow!("Falta sas"))?,
                    ),
                    spark: PathBuf::from(
                        args.get("spark")
                            .and_then(Value::as_str)
                            .ok_or_else(|| anyhow::anyhow!("Falta spark"))?,
                    ),
                    compare_row_order: None,
                    compare_column_order: None,
                    date_format: None,
                    columns: Default::default(),
                    key_columns: args
                        .get("key_columns")
                        .and_then(Value::as_array)
                        .map(|keys| {
                            keys.iter()
                                .filter_map(Value::as_str)
                                .map(str::to_owned)
                                .collect()
                        })
                        .unwrap_or_default(),
                }],
            },
            "compare_batch" => serde_yaml::from_str(&std::fs::read_to_string(
                args.get("manifest")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow::anyhow!("Falta manifest"))?,
            )?)?,
            _ => anyhow::bail!("Herramienta no soportada"),
        };
        let run = compare_all(&manifest.pairs, &manifest.defaults);
        let report = write_reports(&run, &manifest, &PathBuf::from("reports"))?;
        Ok(
            json!({"content":[{"type":"text","text":serde_json::to_string_pretty(&json!({"passed":run.passed,"report":report.html,"result":run}))?}],"isError":!run.passed}),
        )
    })();
    result.unwrap_or_else(
        |error| json!({"content":[{"type":"text","text":error.to_string()}],"isError":true}),
    )
}
