use encoding_rs::{
    Encoding, BIG5, EUC_KR, GBK, SHIFT_JIS, WINDOWS_1250, WINDOWS_1251, WINDOWS_1252, WINDOWS_1253,
    WINDOWS_1254, WINDOWS_1255, WINDOWS_1256, WINDOWS_1257, WINDOWS_1258, WINDOWS_874,
};

/// Map an ASS/SSA `Encoding` value to the corresponding Windows codec.
/// Johab (130) is intentionally left unmapped until a real codec is
/// available; callers must not silently reinterpret it as Unicode.
pub(crate) fn ass_encoding(value: i32) -> Option<&'static Encoding> {
    match value {
        0 | 1 | 77 => Some(WINDOWS_1252),
        128 => Some(SHIFT_JIS),
        129 => Some(EUC_KR),
        130 => None,
        134 => Some(GBK),
        136 => Some(BIG5),
        161 => Some(WINDOWS_1253),
        162 => Some(WINDOWS_1254),
        163 => Some(WINDOWS_1258),
        177 => Some(WINDOWS_1255),
        178 => Some(WINDOWS_1256),
        186 => Some(WINDOWS_1257),
        204 => Some(WINDOWS_1251),
        222 => Some(WINDOWS_874),
        238 => Some(WINDOWS_1250),
        _ => None,
    }
}

/// Decode a metadata field without discarding valid Unicode runs.
///
/// ASS metadata is commonly encoded in the style's `Encoding` code page,
/// while modern files may mix valid UTF-8 text into the same byte stream.
/// Decode only invalid/non-UTF-8 runs through the requested legacy codec;
/// valid UTF-8 scalars pass through unchanged.
pub(crate) fn decode_metadata_bytes(bytes: &[u8], encoding: i32) -> String {
    let mut output = String::with_capacity(bytes.len());
    let mut legacy = Vec::new();
    let flush = |output: &mut String, legacy: &mut Vec<u8>| {
        if legacy.is_empty() {
            return;
        }
        if let Some(codec) = ass_encoding(encoding) {
            let (decoded, _) = codec.decode_without_bom_handling(legacy);
            output.push_str(&decoded);
        } else {
            output.push_str(&String::from_utf8_lossy(legacy));
        }
        legacy.clear();
    };

    let mut offset = 0;
    while offset < bytes.len() {
        let width = utf8_scalar_width(bytes[offset]);
        if width > 1 && offset + width <= bytes.len() {
            if let Ok(text) = std::str::from_utf8(&bytes[offset..offset + width]) {
                flush(&mut output, &mut legacy);
                output.push_str(text);
                offset += width;
                continue;
            }
        }
        legacy.push(bytes[offset]);
        offset += 1;
    }
    flush(&mut output, &mut legacy);
    output
}

fn utf8_scalar_width(first: u8) -> usize {
    match first {
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::decode_metadata_bytes;

    #[test]
    fn legacy_metadata_preserves_utf8_and_decodes_invalid_runs() {
        let mut bytes = "Café ".as_bytes().to_vec();
        bytes.push(0xE9);
        assert_eq!(decode_metadata_bytes(&bytes, 1), "Café é");
    }

    #[test]
    fn legacy_multibyte_metadata_decodes_with_style_encoding() {
        assert_eq!(decode_metadata_bytes(b"\x82\xA0", 128), "あ");
    }
}
