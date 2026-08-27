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
    Mcp,
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
        } => {
            let defaults = Defaults {
                compare_row_order: row_order,
                numeric_tolerance: tolerance,
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
        Command::Mcp => data_comparer::mcp::serve().await?,
    }
    Ok(())
}
