use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
#[command(
    name = "data-comparer-mcp",
    about = "Servidor MCP para comparar salidas SAS y Spark",
    version
)]
struct Args;

#[tokio::main]
async fn main() -> Result<()> {
    Args::parse();
    data_comparer::mcp::serve().await
}
