use crate::reader::{read_table, Table};
use anyhow::{anyhow, Context, Result};
use arrow_array::{ArrayRef, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use encoding_rs::Encoding;
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;
use std::sync::Arc;

const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

#[derive(Debug)]
pub struct ConvertReport {
    pub input_format: String,
    pub output_format: String,
    pub detected_encoding: String,
    pub bom_removed: bool,
    pub used_delimiter: Option<char>,
    pub rows: usize,
    pub columns: usize,
    pub columns_with_replacement_char: Vec<String>,
}

pub fn convert_file(
    input: &Path,
    output: &Path,
    input_delimiter: Option<u8>,
    output_delimiter: u8,
    encoding_override: Option<&str>,
) -> Result<ConvertReport> {
    let input_ext = extension_of(input);
    let output_ext = extension_of(output);

    let (table, encoding_used, bom_removed, delimiter_used, bad_columns) =
        if input_ext == "csv" {
            let (table, meta) = read_csv_any_encoding(input, input_delimiter, encoding_override)?;
            (
                table,
                meta.encoding,
                meta.bom_removed,
                Some(meta.delimiter as char),
                meta.columns_with_replacement_char,
            )
        } else {
            let table = read_table(input, input_delimiter.unwrap_or(b','))?;
            (table, "n/a".to_string(), false, None, Vec::new())
        };

    match output_ext.as_str() {
        "csv" => write_csv(&table, output, output_delimiter)?,
        "xlsx" => write_xlsx(&table, output)?,
        "parquet" => write_parquet(&table, output)?,
        other => {
            return Err(anyhow!(
                "Formato de salida no soportado: .{other}. Use csv, xlsx o parquet."
            ))
        }
    }

    Ok(ConvertReport {
        input_format: table.format.clone(),
        output_format: output_ext,
        detected_encoding: encoding_used,
        bom_removed,
        used_delimiter: delimiter_used,
        rows: table.rows.len(),
        columns: table.columns.len(),
        columns_with_replacement_char: bad_columns,
    })
}

fn extension_of(path: &Path) -> String {
    path.extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

struct CsvEncodingMeta {
    encoding: String,
    bom_removed: bool,
    delimiter: u8,
    columns_with_replacement_char: Vec<String>,
}

fn read_csv_any_encoding(
    path: &Path,
    delimiter: Option<u8>,
    encoding_override: Option<&str>,
) -> Result<(Table, CsvEncodingMeta)> {
    let mut raw = Vec::new();
    File::open(path)
        .with_context(|| format!("No se pudo abrir CSV {}", path.display()))?
        .read_to_end(&mut raw)?;

    let bom_removed = raw.starts_with(&BOM);
    let bytes = if bom_removed { &raw[BOM.len()..] } else { &raw[..] };

    let (decoded, encoding_name) = match encoding_override {
        Some(label) => {
            let encoding = Encoding::for_label(label.as_bytes())
                .ok_or_else(|| anyhow!("Encoding desconocido: {label}"))?;
            let (text, _, _) = encoding.decode(bytes);
            (text.into_owned(), encoding.name().to_string())
        }
        None => {
            let mut detector =
                chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
            detector.feed(bytes, true);
            let encoding = detector.guess(None, chardetng::Utf8Detection::Allow);
            let (text, _, _) = encoding.decode(bytes);
            (text.into_owned(), encoding.name().to_string())
        }
    };

    let delimiter = delimiter.unwrap_or_else(|| guess_delimiter(&decoded));

    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .delimiter(delimiter)
        .from_reader(Cursor::new(decoded.into_bytes()));
    let columns: Vec<String> = reader.headers()?.iter().map(str::to_owned).collect();
    let mut rows = Vec::new();
    for record in reader.records() {
        rows.push(record?.iter().map(str::to_owned).collect());
    }

    let columns_with_replacement_char = columns
        .iter()
        .filter(|name| name.contains('\u{FFFD}'))
        .cloned()
        .collect();

    Ok((
        Table {
            columns,
            rows,
            format: "CSV".to_string(),
            details: vec![
                format!("Codificación detectada: {encoding_name}"),
                format!("Delimitador: {}", delimiter as char),
            ],
        },
        CsvEncodingMeta {
            encoding: encoding_name,
            bom_removed,
            delimiter,
            columns_with_replacement_char,
        },
    ))
}

fn guess_delimiter(sample: &str) -> u8 {
    let first_line = sample.lines().next().unwrap_or("");
    [b',', b';', b'\t', b'|']
        .into_iter()
        .max_by_key(|&candidate| first_line.bytes().filter(|&b| b == candidate).count())
        .unwrap_or(b',')
}

fn write_csv(table: &Table, output: &Path, delimiter: u8) -> Result<()> {
    let mut writer = csv::WriterBuilder::new()
        .delimiter(delimiter)
        .from_path(output)
        .with_context(|| format!("No se pudo escribir CSV {}", output.display()))?;
    writer.write_record(&table.columns)?;
    for row in &table.rows {
        writer.write_record(row)?;
    }
    writer.flush()?;
    Ok(())
}

fn write_xlsx(table: &Table, output: &Path) -> Result<()> {
    use rust_xlsxwriter::Workbook;
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    for (col_index, name) in table.columns.iter().enumerate() {
        sheet.write_string(0, col_index as u16, name)?;
    }
    for (row_index, row) in table.rows.iter().enumerate() {
        for (col_index, value) in row.iter().enumerate() {
            sheet.write_string((row_index + 1) as u32, col_index as u16, value)?;
        }
    }
    workbook
        .save(output)
        .with_context(|| format!("No se pudo escribir Excel {}", output.display()))?;
    Ok(())
}

fn write_parquet(table: &Table, output: &Path) -> Result<()> {
    use parquet::arrow::arrow_writer::ArrowWriter;

    let fields: Vec<Field> = table
        .columns
        .iter()
        .map(|name| Field::new(name, DataType::Utf8, true))
        .collect();
    let schema = Arc::new(Schema::new(fields));

    let arrays: Vec<ArrayRef> = (0..table.columns.len())
        .map(|col_index| {
            let values: Vec<Option<&str>> = table
                .rows
                .iter()
                .map(|row| row.get(col_index).map(String::as_str))
                .collect();
            Arc::new(StringArray::from(values)) as ArrayRef
        })
        .collect();

    let batch = RecordBatch::try_new(schema.clone(), arrays)
        .context("No se pudo construir el batch de Arrow")?;

    let file = File::create(output)
        .with_context(|| format!("No se pudo escribir Parquet {}", output.display()))?;
    let mut writer = ArrowWriter::try_new(file, schema, None)?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}
