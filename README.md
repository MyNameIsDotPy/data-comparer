# Data Comparer

Herramienta local en Rust para validar que salidas SAS y Spark sean equivalentes en CSV, XLSX y Parquet.

## Instalación

Descargue los dos binarios correspondientes desde la página de Releases:

- Linux 64 bits: `data-comparer-cli-linux-x86_64` y `data-comparer-mcp-linux-x86_64`
- Windows 64 bits: `data-comparer-cli-windows-x86_64.exe` y `data-comparer-mcp-windows-x86_64.exe`

En Linux, permita su ejecución y úselo directamente:

```bash
chmod +x data-comparer-cli-linux-x86_64 data-comparer-mcp-linux-x86_64
./data-comparer-cli-linux-x86_64 --help
```

En Windows, ejecute el archivo descargado desde PowerShell:

```powershell
.\data-comparer-cli-windows-x86_64.exe --help
```

También se puede compilar localmente con Rust estable:

```bash
cargo build --release
./target/release/data-comparer --help
./target/release/data-comparer-mcp --help
```

## Uso CLI

```bash
data-comparer compare salida_sas.csv salida_spark.parquet
data-comparer compare salida_sas.xlsx salida_spark.parquet --row-order --tolerance 0.01
data-comparer compare salida_sas.csv salida_adp.csv --key poliza,fecha
data-comparer batch comparaciones.yaml
data-comparer validate comparaciones.yaml
```

`compare` compara un par y `batch` ejecuta todos los pares del manifiesto. Ambos generan `reports/<fecha_hora>/report.html`, `result.json` y el manifiesto usado. Devuelven código `0` si no hay diferencias y `1` si existe alguna diferencia o error.

Opciones de `compare`:

```text
--row-order                 Exige el mismo orden de filas.
--tolerance <NUMERO>        Tolerancia numérica absoluta; el valor predeterminado es 0.1.
--key <COLUMNAS>            Columnas de clave, separadas por coma, para comparar entidades.
--delimiter <CARACTER>      Delimitador de los archivos CSV; el valor predeterminado es ",".
--sas-delimiter <CARACTER>  Delimitador solo para el archivo SAS (si difiere del general).
--spark-delimiter <CARACTER> Delimitador solo para el archivo Spark/ADP.
--encoding <NOMBRE>          Encoding de ambos archivos CSV; por defecto se autodetecta.
--sas-encoding <NOMBRE>      Encoding solo para el archivo SAS (p. ej. "windows-1252", "iso-8859-1").
--spark-encoding <NOMBRE>    Encoding solo para el archivo Spark/ADP.
```

Los archivos CSV con encodings distintos entre sí (por ejemplo SAS en Windows-1252 y Spark en UTF-8) se comparan sin problema: cada lado se decodifica de forma independiente, ya sea autodetectando el encoding o usando el indicado explícitamente. Un BOM UTF-8 al inicio del archivo se detecta y descarta automáticamente.

Use `data-comparer <comando> --help` para consultar todas las opciones.

## Manifiesto

```yaml
defaults:
  numeric_tolerance: 0.1
  compare_row_order: false
  compare_column_order: true
  date_format: "%d/%m/%Y"
  delimiter: ","

pairs:
  - name: ventas
    sas: ./sas/ventas.xlsx
    spark: ./spark/ventas.parquet
    sas_encoding: windows-1252
    spark_encoding: utf-8
    columns:
      fecha:
        date_format: "%Y-%m-%d"
      importe:
        numeric_tolerance: 0.01
        min: 0
        unique: false
      poliza:
        unique: true
        nullable: false
    key_columns:
      - poliza
```

Se aceptan fechas `DD/MM/YYYY` y `YYYY-MM-DD`. La primera hoja se utiliza para Excel. Cuando no se compara el orden de filas, se mantienen los conteos de filas duplicadas.

Los nombres de columnas se emparejan sin distinguir mayúsculas/minúsculas y espacios exteriores. Una diferencia como `poliza` frente a `POLIZA` aparece como advertencia de schema, pero no impide comparar los datos. Las reglas opcionales por columna son `numeric_tolerance`, `date_format`, `min`, `max`, `unique`, `nullable`, `trim_values` y `case_insensitive_values`.

`key_columns` habilita la comparación por clave de negocio: informa claves duplicadas, claves exclusivas de SAS o ADP y claves que existen en ambos archivos pero contienen valores diferentes.

`delimiter` (global o por par, un solo carácter) define el separador usado al leer archivos CSV; no afecta a Excel ni Parquet. `sas_delimiter` y `spark_delimiter` sobrescriben `delimiter` solo para ese archivo.

`encoding` (global o por par) define el encoding usado al leer archivos CSV; si se omite, se autodetecta. `sas_encoding` y `spark_encoding` sobrescriben `encoding` solo para ese archivo, útil cuando SAS y Spark exportan en encodings distintos.

## MCP

`data-comparer-mcp` es un ejecutable independiente que implementa JSON-RPC por entrada/salida estándar, con las herramientas `compare_files` y `compare_batch`. Configure su cliente MCP para ejecutar ese binario directamente, sin argumentos adicionales.

## Publicar una versión

Al publicar una etiqueta con formato estricto `vX.Y.Z`, por ejemplo `v0.2.0`, GitHub Actions compila los ejecutables CLI y MCP para Linux y Windows de 64 bits y crea una GitHub Release con los cuatro archivos.
