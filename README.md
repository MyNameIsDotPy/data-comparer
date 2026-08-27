# Data Comparer

Herramienta local en Rust para validar que salidas SAS y Spark sean equivalentes en CSV, XLSX y Parquet.

## Instalación

Descargue el binario correspondiente desde la página de Releases:

- Linux 64 bits: `data-comparer-linux-x86_64`
- Linux 32 bits: `data-comparer-linux-i686`
- Windows 64 bits: `data-comparer-windows-x86_64.exe`
- Windows 32 bits: `data-comparer-windows-i686.exe`

En Linux, permita su ejecución y úselo directamente:

```bash
chmod +x data-comparer-linux-x86_64
./data-comparer-linux-x86_64 --help
```

En Windows, ejecute el archivo descargado desde PowerShell:

```powershell
.\data-comparer-windows-x86_64.exe --help
```

También se puede compilar localmente con Rust estable:

```bash
cargo build --release
./target/release/data-comparer --help
```

## Uso CLI

```bash
data-comparer compare salida_sas.csv salida_spark.parquet
data-comparer compare salida_sas.xlsx salida_spark.parquet --row-order --tolerance 0.01
data-comparer batch comparaciones.yaml
data-comparer validate comparaciones.yaml
```

`compare` compara un par y `batch` ejecuta todos los pares del manifiesto. Ambos generan `reports/<fecha_hora>/report.html`, `result.json` y el manifiesto usado. Devuelven código `0` si no hay diferencias y `1` si existe alguna diferencia o error.

Opciones de `compare`:

```text
--row-order              Exige el mismo orden de filas.
--tolerance <NUMERO>     Tolerancia numérica absoluta; el valor predeterminado es 0.1.
```

Use `data-comparer <comando> --help` para consultar todas las opciones.

## Manifiesto

```yaml
defaults:
  numeric_tolerance: 0.1
  compare_row_order: false
  compare_column_order: true
  date_format: "%d/%m/%Y"

pairs:
  - name: ventas
    sas: ./sas/ventas.xlsx
    spark: ./spark/ventas.parquet
    columns:
      fecha:
        date_format: "%Y-%m-%d"
      importe:
        numeric_tolerance: 0.01
```

Se aceptan fechas `DD/MM/YYYY` y `YYYY-MM-DD`. La primera hoja se utiliza para Excel. Cuando no se compara el orden de filas, se mantienen los conteos de filas duplicadas.

## MCP

El subcomando `mcp` implementa JSON-RPC por entrada/salida estándar, con las herramientas `compare_files` y `compare_batch`. Configure su cliente MCP para ejecutar el binario con el argumento `mcp`.

## Publicar una versión

Al publicar una etiqueta con formato estricto `vX.Y.Z`, por ejemplo `v0.1.0`, GitHub Actions compila los binarios para Linux y Windows, en 32 y 64 bits, y crea una GitHub Release con los cuatro archivos.
