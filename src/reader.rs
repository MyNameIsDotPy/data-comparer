use anyhow::{anyhow, Context, Result};
use arrow_cast::display::array_value_to_string;
use calamine::{open_workbook_auto, Reader};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Table {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

pub fn read_table(path: &Path) -> Result<Table> {
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "csv" => read_csv(path),
        "xlsx" | "xlsm" | "xls" => read_excel(path),
        "parquet" => read_parquet(path),
        _ => Err(anyhow!(
            "Formato no soportado para {}. Use CSV, XLSX o Parquet.",
            path.display()
        )),
    }
}

fn read_csv(path: &Path) -> Result<Table> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(path)
        .with_context(|| format!("No se pudo abrir CSV {}", path.display()))?;
    let columns = reader.headers()?.iter().map(str::to_owned).collect();
    let mut rows = Vec::new();
    for record in reader.records() {
        rows.push(record?.iter().map(str::to_owned).collect());
    }
    Ok(Table { columns, rows })
}

fn read_excel(path: &Path) -> Result<Table> {
    let mut workbook = open_workbook_auto(path)
        .with_context(|| format!("No se pudo abrir Excel {}", path.display()))?;
    let range = workbook
        .worksheet_range_at(0)
        .ok_or_else(|| anyhow!("El Excel no contiene hojas"))??;
    let mut rows = range.rows();
    let columns = rows
        .next()
        .ok_or_else(|| anyhow!("La primera hoja no contiene cabecera"))?
        .iter()
        .map(|cell| cell.to_string())
        .collect();
    Ok(Table {
        columns,
        rows: rows
            .map(|row| row.iter().map(|cell| cell.to_string()).collect())
            .collect(),
    })
}

fn read_parquet(path: &Path) -> Result<Table> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("No se pudo abrir Parquet {}", path.display()))?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let columns = builder
        .schema()
        .fields()
        .iter()
        .map(|field| field.name().to_string())
        .collect();
    let reader = builder.build()?;
    let mut rows = Vec::new();
    for batch in reader {
        let batch = batch?;
        for row_index in 0..batch.num_rows() {
            let mut row = Vec::with_capacity(batch.num_columns());
            for column in batch.columns() {
                row.push(if column.is_null(row_index) {
                    String::new()
                } else {
                    array_value_to_string(column.as_ref(), row_index)?
                });
            }
            rows.push(row);
        }
    }
    Ok(Table { columns, rows })
}
