use crate::types::{Attachment, AttachmentKind};

/// Parse the lines of a [Fonts] or [Graphics] section into attachments.
///
/// Matches libass: a `fontname:` line starts a new attachment (the rest
/// of the line is the filename); every following line is appended
/// verbatim to the current attachment's encoded data, which is decoded
/// when the next `fontname:` line (or the end of the section) appears.
pub fn parse_attachments(lines: &[&str], kind: AttachmentKind) -> Vec<Attachment> {
    let mut attachments: Vec<Attachment> = Vec::new();
    let mut encoded: Vec<u8> = Vec::new();

    for line in lines {
        let line = line.trim_end_matches(['\r', '\n']);
        if let Some(name) = line.strip_prefix("fontname:") {
            // Flush the previous attachment
            if let Some(last) = attachments.last_mut() {
                last.data = decode_attachment_data(&encoded).unwrap_or_default();
            }
            encoded.clear();
            attachments.push(Attachment {
                kind,
                filename: name.trim().to_string(),
                data: Vec::new(),
            });
        } else if !attachments.is_empty() {
            encoded.extend_from_slice(line.as_bytes());
        }
    }

    if let Some(last) = attachments.last_mut() {
        last.data = decode_attachment_data(&encoded).unwrap_or_default();
    }

    attachments
}

/// Decode ASS embedded-attachment data (the SSA uuencode variant used by
/// libass): each byte contributes `(c - 33) & 63` bits, packed 4 chars
/// into 3 bytes; a final group of 2 chars yields 1 byte, 3 chars yield
/// 2 bytes, and a lone trailing char is invalid.
pub fn decode_attachment_data(encoded: &[u8]) -> Option<Vec<u8>> {
    if encoded.len() % 4 == 1 {
        return None;
    }

    let mut out = Vec::with_capacity(encoded.len() / 4 * 3 + 2);
    for chunk in encoded.chunks(4) {
        let mut value: u32 = 0;
        for (i, &c) in chunk.iter().enumerate() {
            value |= ((c.wrapping_sub(33) & 63) as u32) << (6 * (3 - i));
        }
        out.push((value >> 16) as u8);
        if chunk.len() >= 3 {
            out.push(((value >> 8) & 0xff) as u8);
        }
        if chunk.len() >= 4 {
            out.push((value & 0xff) as u8);
        }
    }

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Inverse of decode_attachment_data, for building test fixtures
    fn encode_attachment_data(data: &[u8]) -> String {
        let mut out = String::new();
        for chunk in data.chunks(3) {
            let mut value = (chunk[0] as u32) << 16;
            if chunk.len() >= 2 {
                value |= (chunk[1] as u32) << 8;
            }
            if chunk.len() >= 3 {
                value |= chunk[2] as u32;
            }
            let chars = match chunk.len() {
                1 => 2,
                2 => 3,
                _ => 4,
            };
            for i in 0..chars {
                out.push((((value >> (6 * (3 - i))) & 63) as u8 + 33) as char);
            }
        }
        out
    }

    #[test]
    fn test_decode_known_vector() {
        // "ABC" packs to 0x414243 -> 6-bit groups 16,20,9,3 -> '1','5','*','$'
        assert_eq!(decode_attachment_data(b"15*$").unwrap(), b"ABC");
    }

    #[test]
    fn test_decode_roundtrip() {
        let payload: Vec<u8> = (0u16..=255).map(|v| v as u8).collect();
        let encoded = encode_attachment_data(&payload);
        assert_eq!(decode_attachment_data(encoded.as_bytes()).unwrap(), payload);
    }

    #[test]
    fn test_decode_rejects_lone_trailing_char() {
        assert!(decode_attachment_data(b"15*$!").is_none());
    }

    #[test]
    fn test_parse_fonts_section() {
        let data = encode_attachment_data(b"fake-ttf-bytes");
        // Split into 80-char-ish lines like real files
        let lines: Vec<String> = data
            .as_bytes()
            .chunks(8)
            .map(|c| String::from_utf8(c.to_vec()).unwrap())
            .collect();
        let mut section: Vec<String> = vec!["fontname: MyFont.ttf".to_string()];
        section.extend(lines);

        let refs: Vec<&str> = section.iter().map(|s| s.as_str()).collect();
        let attachments = parse_attachments(&refs, AttachmentKind::Font);

        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].filename, "MyFont.ttf");
        assert_eq!(attachments[0].kind, AttachmentKind::Font);
        assert_eq!(attachments[0].data, b"fake-ttf-bytes");
    }

    #[test]
    fn test_parse_multiple_attachments() {
        let d1 = encode_attachment_data(b"one");
        let d2 = encode_attachment_data(b"two");
        let section = vec![
            "fontname: a.ttf".to_string(),
            d1,
            "fontname: b.png".to_string(),
            d2,
        ];
        let refs: Vec<&str> = section.iter().map(|s| s.as_str()).collect();
        let attachments = parse_attachments(&refs, AttachmentKind::Graphic);

        assert_eq!(attachments.len(), 2);
        assert_eq!(attachments[0].filename, "a.ttf");
        assert_eq!(attachments[0].data, b"one");
        assert_eq!(attachments[1].filename, "b.png");
        assert_eq!(attachments[1].data, b"two");
        assert!(attachments
            .iter()
            .all(|a| a.kind == AttachmentKind::Graphic));
    }

    #[test]
    fn test_parse_ignores_data_before_fontname() {
        let section = vec!["garbage-before-any-name".to_string()];
        let refs: Vec<&str> = section.iter().map(|s| s.as_str()).collect();
        assert!(parse_attachments(&refs, AttachmentKind::Font).is_empty());
    }
}
