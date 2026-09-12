use anyhow::{anyhow, Context, Result};
use arrow_array::RecordBatch;
use arrow_cast::display::array_value_to_string;
use calamine::{open_workbook_auto, Data, Range, Reader};
use parquet::arrow::arrow_reader::{ParquetRecordBatchReader, ParquetRecordBatchReaderBuilder};
use std::fs::File;
use std::path::Path;

pub struct StreamInfo {
    pub columns: Vec<String>,
    pub format: String,
    pub details: Vec<String>,
}

pub struct ParquetRowIter {
    reader: ParquetRecordBatchReader,
    current: Option<RecordBatch>,
    row_in_batch: usize,
}

pub struct ExcelRowIter {
    range: Range<Data>,
    width: usize,
    height: usize,
    next_row: usize,
}

pub enum RowStream {
    Csv(csv::StringRecordsIntoIter<File>),
    Parquet(ParquetRowIter),
    Excel(ExcelRowIter),
}

impl Iterator for RowStream {
    type Item = Result<Vec<String>>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            RowStream::Csv(iter) => iter.next().map(|record| {
                record
                    .map(|r| r.iter().map(str::to_owned).collect())
                    .map_err(anyhow::Error::from)
            }),
            RowStream::Parquet(iter) => iter.next(),
            RowStream::Excel(iter) => iter.next(),
        }
    }
}

impl Iterator for ParquetRowIter {
    type Item = Result<Vec<String>>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(batch) = &self.current {
                if self.row_in_batch < batch.num_rows() {
                    let row_index = self.row_in_batch;
                    self.row_in_batch += 1;
                    let mut row = Vec::with_capacity(batch.num_columns());
                    for column in batch.columns() {
                        let value = if column.is_null(row_index) {
                            Ok(String::new())
                        } else {
                            array_value_to_string(column.as_ref(), row_index)
                                .map_err(anyhow::Error::from)
                        };
                        match value {
                            Ok(v) => row.push(v),
                            Err(e) => return Some(Err(e)),
                        }
                    }
                    return Some(Ok(row));
                }
            }
            match self.reader.next() {
                Some(Ok(batch)) => {
                    self.current = Some(batch);
                    self.row_in_batch = 0;
                }
                Some(Err(e)) => return Some(Err(anyhow::Error::from(e))),
                None => return None,
            }
        }
    }
}

impl Iterator for ExcelRowIter {
    type Item = Result<Vec<String>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next_row >= self.height {
            return None;
        }
        let row_index = self.next_row;
        self.next_row += 1;
        let row = (0..self.width)
            .map(|col| {
                self.range
                    .get((row_index, col))
                    .map(|cell| cell.to_string())
                    .unwrap_or_default()
            })
            .collect();
        Some(Ok(row))
    }
}

pub fn open_row_stream(path: &Path, delimiter: u8) -> Result<(StreamInfo, RowStream)> {
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "csv" => open_csv(path, delimiter),
        "xlsx" | "xlsm" | "xls" => open_excel(path),
        "parquet" => open_parquet(path),
        _ => Err(anyhow!(
            "Formato no soportado para {}. Use CSV, XLSX o Parquet.",
            path.display()
        )),
    }
}

fn open_csv(path: &Path, delimiter: u8) -> Result<(StreamInfo, RowStream)> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .delimiter(delimiter)
        .from_path(path)
        .with_context(|| format!("No se pudo abrir CSV {}", path.display()))?;
    let columns = reader.headers()?.iter().map(str::to_owned).collect();
    let info = StreamInfo {
        columns,
        format: "CSV".to_string(),
        details: vec![
            format!("Delimitador: {}", delimiter as char),
            "Codificación: UTF-8".to_string(),
        ],
    };
    Ok((info, RowStream::Csv(reader.into_records())))
}

fn open_excel(path: &Path) -> Result<(StreamInfo, RowStream)> {
    let mut workbook = open_workbook_auto(path)
        .with_context(|| format!("No se pudo abrir Excel {}", path.display()))?;
    let sheets = workbook.sheet_names().to_vec();
    let sheet_name = sheets.first().cloned().unwrap_or_default();
    let range = workbook
        .worksheet_range_at(0)
        .ok_or_else(|| anyhow!("El Excel no contiene hojas"))??;
    let (height, width) = range.get_size();
    if height == 0 {
        return Err(anyhow!("La primera hoja no contiene cabecera"));
    }
    let columns = (0..width)
        .map(|col| {
            range
                .get((0, col))
                .map(|cell| cell.to_string())
                .unwrap_or_default()
        })
        .collect();
    let info = StreamInfo {
        columns,
        format: "Excel".to_string(),
        details: vec![
            format!("Hoja utilizada: {sheet_name}"),
            format!("Número de hojas: {}", sheets.len()),
        ],
    };
    let iter = ExcelRowIter {
        range,
        width,
        height,
        next_row: 1,
    };
    Ok((info, RowStream::Excel(iter)))
}

fn open_parquet(path: &Path) -> Result<(StreamInfo, RowStream)> {
    let file = File::open(path)
        .with_context(|| format!("No se pudo abrir Parquet {}", path.display()))?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let details = builder
        .schema()
        .fields()
        .iter()
        .map(|field| format!("{}: {}", field.name(), field.data_type()))
        .collect::<Vec<_>>();
    let columns = builder
        .schema()
        .fields()
        .iter()
        .map(|field| field.name().to_string())
        .collect();
    let reader = builder.build()?;
    let info = StreamInfo {
        columns,
        format: "Parquet".to_string(),
        details,
    };
    let iter = ParquetRowIter {
        reader,
        current: None,
        row_in_batch: 0,
    };
    Ok((info, RowStream::Parquet(iter)))
}
