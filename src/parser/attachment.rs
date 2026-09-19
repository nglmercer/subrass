use crate::types::{Attachment, AttachmentKind};

use super::errors::ParseError;

/// Defensive caps for untrusted subtitle input. All three are enforced
/// per document (across repeated sections), not per section call.
pub const MAX_ATTACHMENTS: usize = 256;
/// Maximum decoded bytes per attachment (64 MiB).
pub const MAX_ATTACHMENT_BYTES: usize = 64 * 1024 * 1024;
/// Maximum total decoded attachment bytes per document (256 MiB).
/// Without this, 256 max-size attachments in one section could
/// decode to 16 GiB before any count cap trips.
pub const MAX_TOTAL_ATTACHMENT_BYTES: usize = 256 * 1024 * 1024;

/// Remaining document-level attachment budget handed to the
/// section parser so oversized data is rejected *while* decoding —
/// before temp buffers grow — rather than after a whole section was
/// decoded and allocated. Especially important for WASM/browser
/// environments, where a hostile section must not transiently
/// allocate hundreds of MiB just to be rejected afterwards.
#[derive(Debug, Clone, Copy)]
pub struct AttachmentBudget {
    /// How many more attachments the document may still accept.
    pub remaining_count: usize,
    /// How many more decoded bytes the document may still accept.
    pub remaining_bytes: usize,
}

impl AttachmentBudget {
    /// Full per-document budget (nothing parsed yet).
    pub fn full() -> Self {
        Self {
            remaining_count: MAX_ATTACHMENTS,
            remaining_bytes: MAX_TOTAL_ATTACHMENT_BYTES,
        }
    }
}

/// Parse the lines of a [Fonts] or [Graphics] section into attachments.
///
/// A header line starts a new attachment — `fontname:` in `[Fonts]`,
/// `filename:` in `[Graphics]` (a header from the wrong section is an
/// explicit error, never silent data). Every following line is validated
/// against the payload alphabet and appended to the current attachment's
/// encoded data, decoded when the next header (or the end of the section)
/// appears. Matching is case-insensitive. Malformed payloads are errors,
/// never silent empty files.
///
/// Uses the full document budget; callers that already hold attachments
/// should prefer [`parse_attachments_with_budget`] so over-budget data
/// is rejected during decoding instead of after allocating it.
pub fn parse_attachments(
    lines: &[&str],
    kind: AttachmentKind,
    start_line: usize,
) -> Result<Vec<Attachment>, ParseError> {
    parse_attachments_with_budget(lines, kind, start_line, AttachmentBudget::full())
}

/// [`parse_attachments`] against a remaining document budget: the count
/// cap trips on the header that would exceed it, the encoded accumulator
/// trips as soon as its implied decoded size exceeds the remaining bytes,
/// and each attachment's exact decoded size is checked before its output
/// buffer is allocated.
pub fn parse_attachments_with_budget(
    lines: &[&str],
    kind: AttachmentKind,
    start_line: usize,
    budget: AttachmentBudget,
) -> Result<Vec<Attachment>, ParseError> {
    let mut attachments: Vec<Attachment> = Vec::new();
    let mut encoded: Vec<u8> = Vec::new();
    let mut header_line = 0usize;
    // Decoded bytes flushed so far in this call, against the remaining
    // document budget (not the full cap: earlier sections already spent
    // part of it, and the caller re-checks lengths on top afterwards).
    let mut decoded_total: usize = 0;

    // Reject an attachment whose decoded size is already known to
    // exceed the remaining budget, before allocating its output.
    let check_decoded_size = |decoded_total: usize, decoded_len: usize, line: usize| {
        budget
            .remaining_bytes
            .checked_sub(decoded_total)
            .filter(|room| decoded_len <= *room)
            .map(|_| ())
            .ok_or_else(|| {
                ParseError::line_error(
                    line,
                    format!(
                        "Total attachment data exceeds the {} MiB document budget",
                        budget.remaining_bytes / (1024 * 1024)
                    ),
                )
            })
    };

    for (i, line) in lines.iter().enumerate() {
        let line = line.trim_end_matches(['\r', '\n']);
        if let Some((prefix, name)) = strip_header(line) {
            let expected = match kind {
                AttachmentKind::Font => "fontname:",
                AttachmentKind::Graphic => "filename:",
            };
            if !prefix.eq_ignore_ascii_case(expected) {
                return Err(ParseError::line_error(
                    start_line + i,
                    format!(
                        "Attachment header {:?} in the wrong section (expected {:?})",
                        prefix.to_lowercase(),
                        expected
                    ),
                ));
            }
            // Flush the previous attachment
            if attachments.len() >= budget.remaining_count {
                return Err(ParseError::line_error(
                    start_line + i,
                    format!("Too many attachments (limit {MAX_ATTACHMENTS} per document)"),
                ));
            }
            if let Some(last) = attachments.last_mut() {
                // Exact decoded size is a pure function of the encoded
                // length: reject over-budget data before `decode_flushed`
                // allocates its output buffer. (A `None` length means a
                // lone trailing char, which decode rejects as invalid.)
                if let Some(decoded_len) = decoded_len_for(encoded.len()) {
                    check_decoded_size(decoded_total, decoded_len, start_line + header_line)?;
                }
                last.data = decode_flushed(&encoded, start_line + header_line, &last.filename)?;
                decoded_total = check_total_budget(
                    decoded_total,
                    last.data.len(),
                    start_line + header_line,
                    budget.remaining_bytes,
                )?;
            }
            encoded.clear();
            header_line = i;
            let filename = name.trim().to_string();
            if filename.is_empty() {
                return Err(ParseError::line_error(
                    start_line + i,
                    "Attachment header with empty filename".to_string(),
                ));
            }
            attachments.push(Attachment {
                kind,
                filename,
                data: Vec::new(),
            });
        } else if !attachments.is_empty() {
            // Empty lines are tolerated (skipped); every other byte must
            // belong to the payload alphabet, with precise line numbers.
            if !line.is_empty() {
                if let Some(&bad) = line.as_bytes().iter().find(|c| !is_payload_byte(**c)) {
                    let current = attachments
                        .last()
                        .map(|a| a.filename.as_str())
                        .unwrap_or("?");
                    return Err(ParseError::line_error(
                        start_line + i,
                        format!(
                            "Invalid encoded byte 0x{:02X} in attachment {:?} (expected '!'..='`')",
                            bad, current
                        ),
                    ));
                }
                encoded.extend_from_slice(line.as_bytes());
            }
            if encoded.len() > MAX_ATTACHMENT_BYTES * 4 / 3 + 8 {
                return Err(ParseError::line_error(
                    start_line + i,
                    format!(
                        "Attachment payload exceeds the {} MiB limit",
                        MAX_ATTACHMENT_BYTES / (1024 * 1024)
                    ),
                ));
            }
            // Early budget trip: full encoded groups already imply this
            // many decoded bytes, and the total only grows from here, so
            // an over-budget accumulator is rejected while still small.
            let room = budget.remaining_bytes.saturating_sub(decoded_total);
            if encoded.len() / 4 * 3 > room {
                return Err(ParseError::line_error(
                    start_line + i,
                    format!(
                        "Total attachment data exceeds the {} MiB document budget",
                        budget.remaining_bytes / (1024 * 1024)
                    ),
                ));
            }
        }
    }

    if let Some(last) = attachments.last_mut() {
        if let Some(decoded_len) = decoded_len_for(encoded.len()) {
            check_decoded_size(decoded_total, decoded_len, start_line + header_line)?;
        }
        last.data = decode_flushed(&encoded, start_line + header_line, &last.filename)?;
        decoded_total = check_total_budget(
            decoded_total,
            last.data.len(),
            start_line + header_line,
            budget.remaining_bytes,
        )?;
    }
    // Keep the total observable for the document-level check without
    // re-summing: a debug assertion documents the invariant instead.
    debug_assert_eq!(
        decoded_total,
        attachments.iter().map(|a| a.data.len()).sum::<usize>()
    );

    Ok(attachments)
}

/// Exact decoded size for an encoded length, or `None` for the invalid
/// lone-trailing-char case (`len % 4 == 1`). Pure over lengths, so the
/// budget pre-check never allocates.
fn decoded_len_for(encoded_len: usize) -> Option<usize> {
    let out = encoded_len.checked_div(4)?.checked_mul(3)?;
    match encoded_len % 4 {
        0 => Some(out),
        2 => out.checked_add(1),
        3 => out.checked_add(2),
        _ => None,
    }
}

/// Add freshly decoded bytes to a running total, enforcing `cap` with
/// checked arithmetic. Pure over lengths, so boundaries are
/// unit-testable without huge buffers.
fn check_total_budget(
    total: usize,
    additional: usize,
    line: usize,
    cap: usize,
) -> Result<usize, ParseError> {
    let sum = total
        .checked_add(additional)
        .ok_or_else(|| ParseError::line_error(line, "Attachment data size overflow".to_string()))?;
    if sum > cap {
        return Err(ParseError::line_error(
            line,
            format!(
                "Total attachment data exceeds the {} MiB document budget",
                cap / (1024 * 1024)
            ),
        ));
    }
    Ok(sum)
}

/// Match `fontname:` / `filename:` headers case-insensitively.
/// Returns the matched prefix (original case) and the name that follows.
/// Byte-based: slicing at `prefix.len()` would panic on multibyte input
/// (a byte length is not a char boundary), so match on bytes first —
/// an ASCII prefix match guarantees the boundary is safe.
fn strip_header(line: &str) -> Option<(&str, &str)> {
    for prefix in ["fontname:", "filename:"] {
        let bytes = line.as_bytes();
        if bytes.len() >= prefix.len()
            && bytes[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
        {
            // `prefix` is ASCII and matched, so both slices are safe.
            return Some((&line[..prefix.len()], &line[prefix.len()..]));
        }
    }
    None
}

/// True for bytes in the SSA attachment payload alphabet: the 64
/// characters `'!'` (33) through `` '`' `` (96). Anything else —
/// controls, spaces, high bytes, other punctuation — is malformed.
fn is_payload_byte(c: u8) -> bool {
    (33..=96).contains(&c)
}

fn decode_flushed(encoded: &[u8], line: usize, filename: &str) -> Result<Vec<u8>, ParseError> {
    let data = decode_attachment_data(encoded).ok_or_else(|| {
        ParseError::line_error(
            line,
            format!("Invalid uuencoded payload for attachment {:?}", filename),
        )
    })?;
    if data.len() > MAX_ATTACHMENT_BYTES {
        return Err(ParseError::line_error(
            line,
            format!("Attachment {:?} exceeds the size limit", filename),
        ));
    }
    Ok(data)
}

/// Encode bytes into ASS embedded-attachment data (test helper and
/// inverse of [`decode_attachment_data`]).
#[cfg(test)]
pub(crate) fn encode_attachment_data(data: &[u8]) -> String {
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

/// Decode ASS embedded-attachment data (the SSA uuencode variant used by
/// libass): each byte contributes `(c - 33) & 63` bits, packed 4 chars
/// into 3 bytes; a final group of 2 chars yields 1 byte, 3 chars yield
/// 2 bytes, and a lone trailing char is invalid.
///
/// Every input byte must belong to the payload alphabet (`'!'`..=`` '`' ``);
/// anything else is rejected rather than masked into silent garbage.
pub fn decode_attachment_data(encoded: &[u8]) -> Option<Vec<u8>> {
    if encoded.len() % 4 == 1 {
        return None;
    }
    if !encoded.iter().all(|&c| is_payload_byte(c)) {
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
    fn test_strip_header_multibyte_fails_without_panicking() {
        // Prefix slicing at a byte length must never split a char.
        assert_eq!(
            strip_header("fontname: A.ttf"),
            Some(("fontname:", " A.ttf"))
        );
        assert_eq!(strip_header("éééééééééé"), None);
        assert_eq!(strip_header("fontnamé: A.ttf"), None);
        assert_eq!(strip_header("éontname: A.ttf"), None);
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
        let attachments = parse_attachments(&refs, AttachmentKind::Font, 0).unwrap();

        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].filename, "MyFont.ttf");
        assert_eq!(attachments[0].kind, AttachmentKind::Font);
        assert_eq!(attachments[0].data, b"fake-ttf-bytes");
    }

    #[test]
    fn test_parse_graphics_filename_header() {
        let data = encode_attachment_data(b"img");
        let section = ["filename: logo.bmp".to_string(), data];
        let refs: Vec<&str> = section.iter().map(|s| s.as_str()).collect();
        let attachments = parse_attachments(&refs, AttachmentKind::Graphic, 0).unwrap();
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].filename, "logo.bmp");
        assert_eq!(attachments[0].data, b"img");
    }

    #[test]
    fn test_parse_header_case_insensitive() {
        let data = encode_attachment_data(b"img");
        let section = ["FontName: Caps.ttf".to_string(), data];
        let refs: Vec<&str> = section.iter().map(|s| s.as_str()).collect();
        let attachments = parse_attachments(&refs, AttachmentKind::Font, 0).unwrap();
        assert_eq!(attachments[0].filename, "Caps.ttf");
    }

    #[test]
    fn test_malformed_payload_is_error() {
        // 5 chars: lone trailing char -> invalid, must not become empty file
        let section = ["fontname: bad.ttf", "15*$!"];
        let attachments = parse_attachments(&section, AttachmentKind::Font, 0);
        assert!(attachments.is_err());
    }

    #[test]
    fn test_parse_multiple_attachments() {
        let d1 = encode_attachment_data(b"one");
        let d2 = encode_attachment_data(b"two");
        let section = [
            "filename: a.bmp".to_string(),
            d1,
            "filename: b.png".to_string(),
            d2,
        ];
        let refs: Vec<&str> = section.iter().map(|s| s.as_str()).collect();
        let attachments = parse_attachments(&refs, AttachmentKind::Graphic, 0).unwrap();

        assert_eq!(attachments.len(), 2);
        assert_eq!(attachments[0].filename, "a.bmp");
        assert_eq!(attachments[0].data, b"one");
        assert_eq!(attachments[1].filename, "b.png");
        assert_eq!(attachments[1].data, b"two");
        assert!(attachments
            .iter()
            .all(|a| a.kind == AttachmentKind::Graphic));
    }

    #[test]
    fn test_parse_ignores_data_before_fontname() {
        let section = ["garbage-before-any-name".to_string()];
        let refs: Vec<&str> = section.iter().map(|s| s.as_str()).collect();
        assert!(parse_attachments(&refs, AttachmentKind::Font, 0)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn test_decode_rejects_out_of_alphabet_bytes() {
        // Bytes the old `(c - 33) & 63` mask accepted silently: space,
        // controls, DEL, high bytes, and punctuation above '`'.
        for bad in [
            &b"!!! "[..],
            &b"\x0015*$"[..],
            &b"15*\x7f"[..],
            &b"15*\xc3"[..],
            &b"15*{"[..],
            &b"15*~"[..],
            &b"15*\n"[..],
        ] {
            assert!(decode_attachment_data(bad).is_none(), "must reject {bad:?}");
        }
        // Boundary bytes are valid.
        assert!(decode_attachment_data(b"!!!!").is_some());
        assert!(decode_attachment_data(b"````").is_some());
    }

    #[test]
    fn test_parse_rejects_bad_payload_line_with_line_number() {
        let section = ["fontname: bad.ttf", "15*$", "!!!! !!!!"];
        let err = parse_attachments(&section, AttachmentKind::Font, 10)
            .unwrap_err()
            .to_string();
        assert!(err.contains("12"), "line number must appear: {err}");
        assert!(err.contains("0x20"), "byte must appear: {err}");
        assert!(err.contains("bad.ttf"), "filename must appear: {err}");
    }

    #[test]
    fn test_total_budget_boundaries() {
        // Pure over lengths: exact budget passes, one byte over fails,
        // and usize overflow fails instead of wrapping.
        let cap = MAX_TOTAL_ATTACHMENT_BYTES;
        assert_eq!(
            check_total_budget(cap - 1, 1, 0, cap).unwrap(),
            MAX_TOTAL_ATTACHMENT_BYTES
        );
        assert!(check_total_budget(cap, 1, 0, cap).is_err());
        assert!(check_total_budget(usize::MAX, 1, 0, cap).is_err());
        assert!(check_total_budget(usize::MAX, usize::MAX, 0, cap).is_err());
        assert_eq!(check_total_budget(0, 0, 0, cap).unwrap(), 0);
        // A tighter remaining budget binds instead of the full cap.
        assert_eq!(check_total_budget(9, 1, 0, 10).unwrap(), 10);
        assert!(check_total_budget(10, 1, 0, 10).is_err());
    }

    #[test]
    fn test_decoded_len_for_boundaries() {
        // Exact decoded sizes without allocating: full groups of 4
        // yield 3 bytes, a final pair yields 1, a triple yields 2, and
        // a lone trailing char is invalid (decode rejects it).
        for (encoded, want) in [
            (0, Some(0)),
            (2, Some(1)),
            (3, Some(2)),
            (4, Some(3)),
            (6, Some(4)),
            (7, Some(5)),
            (8, Some(6)),
            (80, Some(60)),
        ] {
            assert_eq!(decoded_len_for(encoded), want, "len {encoded}");
        }
        assert_eq!(decoded_len_for(1), None);
        assert_eq!(decoded_len_for(5), None);
        // Decoded size is at most 3/4 of the encoded length, so even
        // absurd lengths stay in range (checked math regardless).
        assert!(decoded_len_for(usize::MAX).is_some());
    }

    #[test]
    fn test_budget_count_trips_before_decoding() {
        // No remaining count: the first header fails, before any
        // payload is touched.
        let section = ["fontname: a.ttf", "15*$"];
        let err = parse_attachments_with_budget(
            &section,
            AttachmentKind::Font,
            0,
            AttachmentBudget {
                remaining_count: 0,
                remaining_bytes: MAX_TOTAL_ATTACHMENT_BYTES,
            },
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("Too many attachments"), "{err}");
        // Exact remaining count still parses; the next header fails.
        let budget = AttachmentBudget {
            remaining_count: 1,
            remaining_bytes: MAX_TOTAL_ATTACHMENT_BYTES,
        };
        assert_eq!(
            parse_attachments_with_budget(&section, AttachmentKind::Font, 0, budget)
                .unwrap()
                .len(),
            1
        );
        let two = ["fontname: a.ttf", "15*$", "fontname: b.ttf", "15*$"];
        let err = parse_attachments_with_budget(&two, AttachmentKind::Font, 0, budget)
            .unwrap_err()
            .to_string();
        assert!(err.contains("Too many attachments"), "{err}");
    }

    #[test]
    fn test_budget_bytes_reject_before_allocating_output() {
        // "15*$!!" (6 chars) decodes to exactly 4 bytes: the implied
        // full-group size (3) fits room 3, but the exact size (4) does
        // not, so the pre-decode check — not the accumulator — rejects
        // at the header line, before any output buffer is allocated.
        let section = ["fontname: a.ttf", "15*$!!"];
        let budget = AttachmentBudget {
            remaining_count: MAX_ATTACHMENTS,
            remaining_bytes: 3,
        };
        let err = parse_attachments_with_budget(&section, AttachmentKind::Font, 10, budget)
            .unwrap_err()
            .to_string();
        assert!(err.contains("document budget"), "{err}");
        assert!(err.contains("line 10"), "header line, not payload: {err}");
        // Exact remaining bytes still parse.
        let budget = AttachmentBudget {
            remaining_count: MAX_ATTACHMENTS,
            remaining_bytes: 4,
        };
        let parsed =
            parse_attachments_with_budget(&section, AttachmentKind::Font, 10, budget).unwrap();
        assert_eq!(parsed[0].data.len(), 4);
    }

    #[test]
    fn test_budget_trips_accumulator_while_small() {
        // Room for 10 decoded bytes with three 8-char lines: the
        // accumulator (16 encoded -> 12 implied decoded) trips on the
        // second payload line — reported there, while still small —
        // instead of decoding everything first.
        let section = ["fontname: a.ttf", "!!!!!!!!", "!!!!!!!!", "!!!!!!!!"];
        let budget = AttachmentBudget {
            remaining_count: MAX_ATTACHMENTS,
            remaining_bytes: 10,
        };
        let err = parse_attachments_with_budget(&section, AttachmentKind::Font, 10, budget)
            .unwrap_err()
            .to_string();
        assert!(err.contains("document budget"), "{err}");
        assert!(err.contains("line 12"), "trip line, not header: {err}");
    }

    #[test]
    fn test_headers_are_section_aware() {
        // fontname: belongs to [Fonts], filename: to [Graphics].
        let section = ["fontname: a.ttf", "15*$"];
        assert!(parse_attachments(&section, AttachmentKind::Graphic, 0).is_err());
        let section = ["filename: a.bmp", "15*$"];
        assert!(parse_attachments(&section, AttachmentKind::Font, 0).is_err());
        // Correct headers still parse in their sections.
        let section = ["filename: a.bmp", "15*$"];
        assert_eq!(
            parse_attachments(&section, AttachmentKind::Graphic, 0)
                .unwrap()
                .len(),
            1
        );
    }
}
