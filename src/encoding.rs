use anyhow::{anyhow, Result};
use encoding_rs::Encoding;

const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// Quita un BOM UTF-8 inicial si está presente. Se hace antes de decodificar,
/// sin importar el encoding elegido, porque un BOM solo puede corresponder a UTF-8.
pub fn strip_bom(bytes: &[u8]) -> (&[u8], bool) {
    if bytes.starts_with(&BOM) {
        (&bytes[BOM.len()..], true)
    } else {
        (bytes, false)
    }
}

/// Resuelve el encoding a usar: el indicado explícitamente por el usuario, o
/// uno detectado automáticamente a partir de una muestra de bytes (ya sin BOM).
pub fn resolve_encoding(sample: &[u8], override_label: Option<&str>) -> Result<&'static Encoding> {
    match override_label {
        Some(label) => Encoding::for_label(label.as_bytes())
            .ok_or_else(|| anyhow!("Encoding desconocido: {label}")),
        None => {
            let mut detector =
                chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
            detector.feed(sample, true);
            Ok(detector.guess(None, chardetng::Utf8Detection::Allow))
        }
    }
}
