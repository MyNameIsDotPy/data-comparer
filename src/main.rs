use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use data_comparer::compare::compare_all;
use data_comparer::config::{Defaults, Manifest, PairConfig};
use data_comparer::report::write_reports;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "data-comparer",
    about = "Compara salidas SAS y Spark en disco local"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Compare {
        sas: PathBuf,
        spark: PathBuf,
        #[arg(long)]
        row_order: bool,
        #[arg(long, default_value_t = 0.1)]
        tolerance: f64,
        #[arg(long = "key", value_delimiter = ',')]
        key_columns: Vec<String>,
        #[arg(long, default_value_t = ',')]
        delimiter: char,
        #[arg(long)]
        sas_delimiter: Option<char>,
        #[arg(long)]
        spark_delimiter: Option<char>,
        #[arg(long)]
        encoding: Option<String>,
        #[arg(long)]
        sas_encoding: Option<String>,
        #[arg(long)]
        spark_encoding: Option<String>,
    },
    Batch {
        manifest: PathBuf,
    },
    Validate {
        manifest: PathBuf,
    },
    Serve {
        #[arg(long, default_value = "127.0.0.1:8080")]
        address: String,
    },
    Convert {
        input: PathBuf,
        output: PathBuf,
        #[arg(long)]
        delimiter: Option<char>,
        #[arg(long)]
        output_delimiter: Option<char>,
        #[arg(long)]
        encoding: Option<String>,
    },
}

fn load_manifest(path: &PathBuf) -> Result<Manifest> {
    serde_yaml::from_str(
        &std::fs::read_to_string(path)
            .with_context(|| format!("No se pudo leer {}", path.display()))?,
    )
    .context("Manifiesto YAML inválido")
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Compare {
            sas,
            spark,
            row_order,
            tolerance,
            key_columns,
            delimiter,
            sas_delimiter,
            spark_delimiter,
            encoding,
            sas_encoding,
            spark_encoding,
        } => {
            let defaults = Defaults {
                compare_row_order: row_order,
                numeric_tolerance: tolerance,
                delimiter,
                encoding,
                ..Defaults::default()
            };
            let manifest = Manifest {
                defaults: defaults.clone(),
                pairs: vec![PairConfig {
                    name: None,
                    sas,
                    spark,
                    compare_row_order: None,
                    compare_column_order: None,
                    date_format: None,
                    columns: Default::default(),
                    key_columns,
                    delimiter: None,
                    sas_delimiter,
                    spark_delimiter,
                    encoding: None,
                    sas_encoding,
                    spark_encoding,
                }],
            };
            let run = compare_all(&manifest.pairs, &defaults);
            let paths = write_reports(&run, &manifest, &PathBuf::from("reports"))?;
            println!(
                "{}\nInforme: {}",
                if run.passed {
                    "Sin diferencias"
                } else {
                    "Se encontraron diferencias"
                },
                paths.html.display()
            );
            if !run.passed {
                std::process::exit(1);
            }
        }
        Command::Batch { manifest: path } => {
            let manifest = load_manifest(&path)?;
            let run = compare_all(&manifest.pairs, &manifest.defaults);
            let paths = write_reports(&run, &manifest, &PathBuf::from("reports"))?;
            println!("Informe: {}", paths.html.display());
            if !run.passed {
                std::process::exit(1);
            }
        }
        Command::Validate { manifest } => {
            let manifest = load_manifest(&manifest)?;
            for pair in manifest.pairs {
                if !pair.sas.exists() || !pair.spark.exists() {
                    anyhow::bail!("No existe un archivo del par {}", pair.label());
                }
            }
            println!("Manifiesto válido");
        }
        Command::Serve { address } => data_comparer::web::serve(&address).await?,
        Command::Convert {
            input,
            output,
            delimiter,
            output_delimiter,
            encoding,
        } => {
            let input_delimiter = delimiter.map(|c| c as u8);
            let output_delimiter = output_delimiter
                .map(|c| c as u8)
                .or(input_delimiter)
                .unwrap_or(b',');
            let report = data_comparer::convert::convert_file(
                &input,
                &output,
                input_delimiter,
                output_delimiter,
                encoding.as_deref(),
            )?;
            println!(
                "Convertido {} -> {}",
                report.input_format, report.output_format
            );
            println!("Codificación detectada: {}", report.detected_encoding);
            if report.bom_removed {
                println!("Se detectó y eliminó un BOM UTF-8 al inicio del archivo.");
            }
            if let Some(delim) = report.used_delimiter {
                println!("Delimitador de entrada usado: {delim}");
            }
            println!("Filas: {} | Columnas: {}", report.rows, report.columns);
            if !report.columns_with_replacement_char.is_empty() {
                println!(
                    "Aviso: estas columnas contienen el carácter de reemplazo \u{FFFD}, lo que indica una corrupción de encoding previa e irreversible en el archivo de origen: {}",
                    report.columns_with_replacement_char.join(", ")
                );
            }
            println!("Archivo escrito en {}", output.display());
        }
    }
    Ok(())
}
