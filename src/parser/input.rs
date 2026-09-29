use super::errors::{ParseError, Section};
use super::sections::{process_section, process_section_bytes};
use super::AssDocument;

/// Parse the current UTF-8 text representation of an ASS document.
///
/// This adapter deliberately owns only BOM handling, bounded section
/// collection, and line accounting. Section-specific parsing stays in
/// `sections` and the existing parser modules so a future byte-oriented
/// entry point can preserve event payload bytes before charset decoding.
pub(super) fn parse_text(input: &str) -> Result<AssDocument, ParseError> {
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);
    let mut doc = AssDocument::new();
    let mut current_section: Option<Section> = None;
    let mut section_lines: Vec<&str> = Vec::new();
    let mut line_number = 0;
    let mut section_start_line = 0;
    let mut found_section = false;

    for line in input.lines() {
        line_number += 1;
        let trimmed = line.trim();

        // Check for section headers.
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            // Process the previous section. Content starts on the line
            // after the header, so error line numbers add one.
            if let Some(section) = current_section {
                process_section(&mut doc, section, &section_lines, section_start_line + 1)?;
            }

            // Start a new section.
            current_section = Section::from_header(trimmed);
            if current_section.is_some() {
                found_section = true;
            }
            section_lines.clear();
            section_start_line = line_number;
            continue;
        }

        // Add line to the current recognized section.
        if current_section.is_some() {
            section_lines.push(line);
        }
    }

    // Process the last section.
    if let Some(section) = current_section {
        process_section(&mut doc, section, &section_lines, section_start_line + 1)?;
    }

    // Reject non-empty input that contains no recognizable ASS sections.
    if !found_section && input.lines().any(|line| !line.trim().is_empty()) {
        return Err(ParseError::Unexpected(
            "No valid ASS sections found".to_string(),
        ));
    }

    doc.script_info.infer_play_resolution();
    Ok(doc)
}

/// Parse raw ASS bytes without replacing an invalid event payload before the
/// charset decoder sees it. Valid UTF-8 (including a UTF-8 BOM) takes the
/// existing string path unchanged; only genuinely non-UTF-8 input uses the
/// byte-preserving section scan.
pub(super) fn parse_bytes(input: &[u8]) -> Result<AssDocument, ParseError> {
    // A UTF-16 BOM is authoritative. Reject truncated units and unpaired
    // surrogates instead of accidentally treating this as legacy byte input.
    let endian = if input.starts_with(&[0xff, 0xfe]) {
        Some(true)
    } else if input.starts_with(&[0xfe, 0xff]) {
        Some(false)
    } else {
        None
    };
    if let Some(little) = endian {
        let bytes = &input[2..];
        if !bytes.len().is_multiple_of(2) {
            return Err(ParseError::Unexpected("Truncated UTF-16 code unit".into()));
        }
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|b| {
                if little {
                    u16::from_le_bytes([b[0], b[1]])
                } else {
                    u16::from_be_bytes([b[0], b[1]])
                }
            })
            .collect();
        let text = String::from_utf16(&units)
            .map_err(|_| ParseError::Unexpected("Invalid UTF-16 surrogate sequence".into()))?;
        return parse_text(&text);
    }
    let input = strip_utf8_bom(input);
    if let Ok(text) = std::str::from_utf8(input) {
        return parse_text(text);
    }

    let mut doc = AssDocument::new();
    let mut current_section: Option<Section> = None;
    let mut section_lines: Vec<Vec<u8>> = Vec::new();
    let mut line_number = 0;
    let mut section_start_line = 0;
    let mut found_section = false;

    for line in split_lines(input) {
        line_number += 1;
        let trimmed = trim_ascii_bytes(line);

        if trimmed.starts_with(b"[") && trimmed.ends_with(b"]") {
            if let Some(section) = current_section {
                process_section_bytes(&mut doc, section, &section_lines, section_start_line + 1)?;
            }
            current_section = Section::from_header_bytes(trimmed);
            if current_section.is_some() {
                found_section = true;
            }
            section_lines.clear();
            section_start_line = line_number;
            continue;
        }

        if current_section.is_some() {
            section_lines.push(line.to_vec());
        }
    }

    if let Some(section) = current_section {
        process_section_bytes(&mut doc, section, &section_lines, section_start_line + 1)?;
    }

    if !found_section && split_lines(input).any(|line| !trim_ascii_bytes(line).is_empty()) {
        return Err(ParseError::Unexpected(
            "No valid ASS sections found".to_string(),
        ));
    }

    super::normalize_byte_metadata(&mut doc);
    doc.script_info.infer_play_resolution();
    Ok(doc)
}

fn strip_utf8_bom(input: &[u8]) -> &[u8] {
    input.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(input)
}

fn split_lines(input: &[u8]) -> impl Iterator<Item = &[u8]> {
    input
        .split(|byte| *byte == b'\n')
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
}

fn trim_ascii_bytes(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(|byte| byte.is_ascii_whitespace()) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(|byte| byte.is_ascii_whitespace()) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}
