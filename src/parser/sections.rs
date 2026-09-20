use super::attachment;
use super::errors::{ParseError, Section};
use super::{event, script_info, style, AssDocument};
use crate::types::{Attachment, AttachmentKind};

pub(super) fn process_section(
    doc: &mut AssDocument,
    section: Section,
    lines: &[&str],
    first_content_line: usize,
) -> Result<(), ParseError> {
    match section {
        Section::ScriptInfo => {
            doc.script_info = script_info::parse_script_info(lines, first_content_line)?;
        }
        // Repeated style/event sections append: earlier data is kept,
        // but every cap is enforced per document (checked), so extra
        // sections cannot bypass the per-section limits.
        Section::V4PlusStyles => {
            let parsed = style::parse_styles(lines, first_content_line, false)?;
            check_global_count(
                doc.styles.len(),
                parsed.len(),
                style::MAX_STYLES,
                "styles",
                first_content_line,
            )?;
            doc.styles.extend(parsed);
        }
        Section::V4Styles => {
            let parsed = style::parse_styles(lines, first_content_line, true)?;
            check_global_count(
                doc.styles.len(),
                parsed.len(),
                style::MAX_STYLES,
                "styles",
                first_content_line,
            )?;
            doc.styles.extend(parsed);
        }
        Section::Events => {
            let parsed = event::parse_events(lines, first_content_line)?;
            check_global_count(
                doc.events.len(),
                parsed.len(),
                event::MAX_EVENTS,
                "events",
                first_content_line,
            )?;
            doc.events.extend(parsed);
        }
        Section::Fonts => {
            let parsed = attachment::parse_attachments_with_budget(
                lines,
                AttachmentKind::Font,
                first_content_line,
                remaining_attachment_budget(doc),
            )?;
            check_global_attachments(doc, &parsed, first_content_line)?;
            doc.attachments.extend(parsed);
        }
        Section::Graphics => {
            let parsed = attachment::parse_attachments_with_budget(
                lines,
                AttachmentKind::Graphic,
                first_content_line,
                remaining_attachment_budget(doc),
            )?;
            check_global_attachments(doc, &parsed, first_content_line)?;
            doc.attachments.extend(parsed);
        }
    }
    Ok(())
}

/// Dispatch a byte-preserving section. Syntax is recognized from ASCII bytes;
/// user-visible fields stay raw until the section parser has enough context
/// to apply the appropriate legacy charset.
pub(super) fn process_section_bytes(
    doc: &mut AssDocument,
    section: Section,
    lines: &[Vec<u8>],
    first_content_line: usize,
) -> Result<(), ParseError> {
    let raw_lines: Vec<&[u8]> = lines.iter().map(Vec::as_slice).collect();
    match section {
        Section::ScriptInfo => {
            doc.script_info = script_info::parse_script_info_bytes(&raw_lines, first_content_line)?;
        }
        Section::V4PlusStyles | Section::V4Styles => {
            let parsed = style::parse_styles_bytes(
                &raw_lines,
                first_content_line,
                matches!(section, Section::V4Styles),
            )?;
            check_global_count(
                doc.styles.len(),
                parsed.len(),
                style::MAX_STYLES,
                "styles",
                first_content_line,
            )?;
            doc.styles.extend(parsed);
        }
        Section::Events => {
            let parsed = event::parse_events_bytes(&raw_lines, first_content_line)?;
            check_global_count(
                doc.events.len(),
                parsed.len(),
                event::MAX_EVENTS,
                "events",
                first_content_line,
            )?;
            doc.events.extend(parsed);
        }
        Section::Fonts | Section::Graphics => {
            // Attachment payload syntax is ASCII. Decode only the header and
            // filename bytes as legacy metadata before the strict payload
            // parser validates the encoded data.
            let decoded: Vec<String> = raw_lines
                .iter()
                .map(|line| crate::charset::decode_metadata_bytes(line, 1))
                .collect();
            let text_lines: Vec<&str> = decoded.iter().map(String::as_str).collect();
            process_section(doc, section, &text_lines, first_content_line)?;
        }
    }
    Ok(())
}

/// Enforce a per-document count cap across appended sections with
/// checked arithmetic. Pure over lengths, so boundaries (including
/// `usize` overflow) are unit-testable without huge inputs.
pub(super) fn check_global_count(
    existing: usize,
    additional: usize,
    max: usize,
    what: &str,
    line: usize,
) -> Result<usize, ParseError> {
    let total = existing
        .checked_add(additional)
        .ok_or_else(|| ParseError::line_error(line, format!("Too many {what} (count overflow)")))?;
    if total > max {
        return Err(ParseError::line_error(
            line,
            format!("Too many {what} (limit {max} per document)"),
        ));
    }
    Ok(total)
}

/// Remaining document-level attachment budget before parsing another
/// [Fonts]/[Graphics] section. Passing it into the section parser lets
/// over-budget data be rejected while decoding — before temp buffers
/// grow — instead of only after a full section was allocated.
fn remaining_attachment_budget(doc: &AssDocument) -> attachment::AttachmentBudget {
    attachment::AttachmentBudget {
        remaining_count: attachment::MAX_ATTACHMENTS.saturating_sub(doc.attachments.len()),
        remaining_bytes: attachment::MAX_TOTAL_ATTACHMENT_BYTES
            .saturating_sub(doc.total_attachment_bytes()),
    }
}

/// Enforce the document-wide attachment count and decoded-byte budget
/// before appending a freshly parsed section. Both sums use checked
/// arithmetic; pure over lengths except for the final byte summation.
pub(super) fn check_global_attachments(
    doc: &AssDocument,
    parsed: &[Attachment],
    line: usize,
) -> Result<(), ParseError> {
    check_global_count(
        doc.attachments.len(),
        parsed.len(),
        attachment::MAX_ATTACHMENTS,
        "attachments",
        line,
    )?;
    let mut total = 0usize;
    for len in doc
        .attachments
        .iter()
        .chain(parsed.iter())
        .map(|a| a.data.len())
    {
        total = total.checked_add(len).ok_or_else(|| {
            ParseError::line_error(line, "Attachment data size overflow".to_string())
        })?;
        if total > attachment::MAX_TOTAL_ATTACHMENT_BYTES {
            return Err(ParseError::line_error(
                line,
                format!(
                    "Total attachment data exceeds the {} MiB document budget",
                    attachment::MAX_TOTAL_ATTACHMENT_BYTES / (1024 * 1024)
                ),
            ));
        }
    }
    Ok(())
}
