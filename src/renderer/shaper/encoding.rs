use crate::types::override_tag::{parse_text_segments, OverrideTag};
use encoding_rs::{
    Encoding, BIG5, EUC_KR, GBK, SHIFT_JIS, WINDOWS_1250, WINDOWS_1251, WINDOWS_1252, WINDOWS_1253,
    WINDOWS_1254, WINDOWS_1255, WINDOWS_1256, WINDOWS_1257, WINDOWS_1258, WINDOWS_874,
};

/// Map the numeric ASS/SSA `Encoding`/`\fe` value to the corresponding
/// Windows code page. Subtitle text arrives here as Rust Unicode, so the
/// compatibility bridge reinterprets contiguous byte-like Unicode values
/// (`U+0000..U+00FF`) as the original byte stream, including multibyte
/// encodings, while leaving already-Unicode scripts intact.
pub(super) fn ass_encoding(value: i32) -> Option<&'static Encoding> {
    match value {
        0 | 1 | 77 => Some(WINDOWS_1252),
        128 => Some(SHIFT_JIS),
        129 => Some(EUC_KR),
        130 => None, // Johab is not provided by encoding_rs.
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

/// Decode raw ASS event bytes while honoring ASCII `\fe` tags encountered
/// between text runs. Valid UTF-8 scalars are preserved as Unicode; other
/// bytes are decoded as one deterministic legacy run under the current ASS
/// encoding. This keeps malformed/legacy bytes out of lossy UTF-8 conversion
/// until the charset state is known.
pub(super) fn decode_ass_bytes(bytes: &[u8], default_encoding: i32) -> String {
    let mut output = String::with_capacity(bytes.len());
    let mut encoding = default_encoding;
    let mut offset = 0;

    while offset < bytes.len() {
        if bytes[offset] == b'{' {
            if let Some(relative_end) = bytes[offset..].iter().position(|b| *b == b'}') {
                let end = offset + relative_end + 1;
                let tag_text = String::from_utf8_lossy(&bytes[offset..end]);
                output.push_str(&tag_text);
                if let Some(segment) = parse_text_segments(&tag_text).first() {
                    for tag in &segment.tags {
                        if let OverrideTag::FontEncoding(value) = tag {
                            encoding = *value;
                        }
                    }
                }
                offset = end;
                continue;
            }
        }

        let start = offset;
        while offset < bytes.len() && bytes[offset] != b'{' {
            offset += 1;
        }
        output.push_str(&decode_mixed_bytes(&bytes[start..offset], encoding));
    }

    output
}

fn decode_mixed_bytes(bytes: &[u8], encoding: i32) -> String {
    let mut output = String::with_capacity(bytes.len());
    let mut legacy = Vec::new();
    let mut offset = 0;

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
