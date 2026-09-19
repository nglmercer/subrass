use super::errors::{ParseError, Section};
use super::sections::process_section;
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

    Ok(doc)
}
