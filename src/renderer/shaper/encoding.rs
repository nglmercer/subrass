use crate::charset::ass_encoding;
use crate::types::override_tag::{parse_text_segments, OverrideTag};

/// Windows Symbol fonts expose their byte slots through the private-use
/// cmap range U+F000..U+F0FF. This is a font mapping, not a normal text
/// encoding; the caller must still select a Symbol-compatible face.
pub(super) fn decode_symbol_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| char::from_u32(0xF000 + u32::from(*byte)).unwrap_or('\u{FFFD}'))
        .collect()
}

/// Decode a Unicode event string while preserving ASS tags and applying
/// mid-event `\fe` changes. This is the compatibility path for the existing
/// `parse(&str)` API; raw legacy files use [`decode_ass_bytes`] instead.
pub(super) fn decode_ass_text(text: &str, default_encoding: i32) -> String {
    let mut output = String::with_capacity(text.len());
    let mut encoding = default_encoding;
    let mut offset = 0;

    while offset < text.len() {
        if text.as_bytes()[offset] == b'{' {
            if let Some(relative_end) = text[offset..].find('}') {
                let end = offset + relative_end + 1;
                let tag_text = &text[offset..end];
                output.push_str(tag_text);
                if let Some(segment) = parse_text_segments(tag_text).first() {
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

        let end = text[offset..]
            .find('{')
            .map_or(text.len(), |relative| offset + relative);
        output.push_str(&decode_text_run(&text[offset..end], encoding));
        offset = end;
    }

    output
}

fn decode_text_run(text: &str, encoding: i32) -> String {
    if encoding == 2 {
        let mut output = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\\'
                && chars
                    .peek()
                    .is_some_and(|next| matches!(next, 'N' | 'n' | 'h'))
            {
                output.push(ch);
                if let Some(next) = chars.next() {
                    output.push(next);
                }
            } else if (ch as u32) <= u32::from(u8::MAX) {
                output.push_str(&decode_symbol_bytes(&[ch as u8]));
            } else {
                output.push(ch);
            }
        }
        return output;
    }

    let Some(codec) = ass_encoding(encoding) else {
        return text.to_string();
    };
    let mut output = String::with_capacity(text.len());
    let mut bytes = Vec::new();
    let flush = |output: &mut String, bytes: &mut Vec<u8>| {
        if bytes.is_empty() {
            return;
        }
        let (decoded, _) = codec.decode_without_bom_handling(bytes);
        output.push_str(&decoded);
        bytes.clear();
    };
    for ch in text.chars() {
        if (ch as u32) <= u32::from(u8::MAX) {
            bytes.push(ch as u8);
        } else {
            flush(&mut output, &mut bytes);
            output.push(ch);
        }
    }
    flush(&mut output, &mut bytes);
    output
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

        let mut start = offset;
        while offset < bytes.len() && bytes[offset] != b'{' {
            if bytes[offset] == b'\\'
                && bytes
                    .get(offset + 1)
                    .is_some_and(|next| matches!(next, b'N' | b'n' | b'h'))
            {
                if start < offset {
                    output.push_str(&decode_mixed_bytes(&bytes[start..offset], encoding));
                }
                output.extend([b'\\', bytes[offset + 1]].map(char::from));
                offset += 2;
                start = offset;
                continue;
            }
            offset += 1;
        }
        output.push_str(&decode_mixed_bytes(&bytes[start..offset], encoding));
    }

    output
}

fn decode_mixed_bytes(bytes: &[u8], encoding: i32) -> String {
    if encoding == 2 {
        return decode_symbol_bytes(bytes);
    }

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
