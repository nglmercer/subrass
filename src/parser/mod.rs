pub mod attachment;
pub mod errors;
pub mod event;
pub mod script_info;
pub mod style;

use crate::types::{Attachment, AttachmentKind, Event, ScriptInfo, Style};
use errors::{ParseError, Section};

/// Complete ASS document
#[derive(Debug, Clone)]
pub struct AssDocument {
    pub script_info: ScriptInfo,
    pub styles: Vec<Style>,
    pub events: Vec<Event>,
    pub attachments: Vec<Attachment>,
}

impl AssDocument {
    pub fn new() -> Self {
        Self {
            script_info: ScriptInfo::new(),
            styles: Vec::new(),
            events: Vec::new(),
            attachments: Vec::new(),
        }
    }

    /// Parse an ASS file from a string
    pub fn parse(input: &str) -> Result<Self, ParseError> {
        let mut doc = AssDocument::new();
        let mut current_section: Option<Section> = None;
        let mut section_lines: Vec<&str> = Vec::new();
        let mut line_number = 0;
        let mut section_start_line = 0;
        let mut found_section = false;

        for line in input.lines() {
            line_number += 1;
            let trimmed = line.trim();

            // Check for section headers
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                // Process previous section. Content starts on the line
                // after the header, so error line numbers add one.
                if let Some(section) = current_section {
                    process_section(&mut doc, section, &section_lines, section_start_line + 1)?;
                }

                // Start new section
                current_section = Section::from_header(trimmed);
                if current_section.is_some() {
                    found_section = true;
                }
                section_lines.clear();
                section_start_line = line_number;
                continue;
            }

            // Add line to current section
            if current_section.is_some() {
                section_lines.push(line);
            }
        }

        // Process last section
        if let Some(section) = current_section {
            process_section(&mut doc, section, &section_lines, section_start_line + 1)?;
        }

        // Reject non-empty input that contains no recognizable ASS sections
        if !found_section && input.lines().any(|l| !l.trim().is_empty()) {
            return Err(ParseError::Unexpected(
                "No valid ASS sections found".to_string(),
            ));
        }

        Ok(doc)
    }

    pub fn get_event_count(&self) -> usize {
        self.events.len()
    }

    pub fn get_style_count(&self) -> usize {
        self.styles.len()
    }

    /// Total decoded attachment bytes across the whole document.
    /// Bounded by `MAX_TOTAL_ATTACHMENT_BYTES` at parse time; hosts can
    /// use this to account for attachment memory before loading fonts.
    pub fn total_attachment_bytes(&self) -> usize {
        self.attachments.iter().map(|a| a.data.len()).sum()
    }

    pub fn get_events_at_time(&self, time_ms: u64) -> Vec<&Event> {
        event::get_events_at_time(&self.events, time_ms)
    }

    pub fn get_dialogue_events(&self) -> Vec<&Event> {
        event::get_dialogue_events(&self.events)
    }

    pub fn get_comment_events(&self) -> Vec<&Event> {
        event::get_comment_events(&self.events)
    }

    pub fn find_style(&self, name: &str) -> Option<&Style> {
        self.styles.iter().find(|s| s.name == name)
    }

    pub fn get_default_style(&self) -> Option<&Style> {
        self.find_style("Default")
    }

    pub fn sort_events_by_time(&mut self) {
        event::sort_events_by_time(&mut self.events);
    }

    pub fn sort_events_by_layer(&mut self) {
        event::sort_events_by_layer(&mut self.events);
    }
}

impl Default for AssDocument {
    fn default() -> Self {
        Self::new()
    }
}

fn process_section(
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
            let parsed =
                attachment::parse_attachments(lines, AttachmentKind::Font, first_content_line)?;
            check_global_attachments(doc, &parsed, first_content_line)?;
            doc.attachments.extend(parsed);
        }
        Section::Graphics => {
            let parsed =
                attachment::parse_attachments(lines, AttachmentKind::Graphic, first_content_line)?;
            check_global_attachments(doc, &parsed, first_content_line)?;
            doc.attachments.extend(parsed);
        }
    }
    Ok(())
}

/// Enforce a per-document count cap across appended sections with
/// checked arithmetic. Pure over lengths, so boundaries (including
/// `usize` overflow) are unit-testable without huge inputs.
fn check_global_count(
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

/// Enforce the document-wide attachment count and decoded-byte budget
/// before appending a freshly parsed section. Both sums use checked
/// arithmetic; pure over lengths except for the final byte summation.
fn check_global_attachments(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::EventType;

    const TEST_ASS: &str = r#"[Script Info]
; Test script
Title: Test
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,48,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,-1,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:01.00,0:00:04.00,Default,John,0,0,0,,Hello World!
Comment: 0,0:00:00.00,0:00:30.00,Default,,0,0,0,,This is a comment
"#;

    #[test]
    fn test_parse_complete_document() {
        let doc = AssDocument::parse(TEST_ASS).unwrap();

        assert_eq!(doc.script_info.title.as_deref(), Some("Test"));
        assert_eq!(doc.script_info.play_res_x, 1920);
        assert_eq!(doc.styles.len(), 1);
        assert_eq!(doc.styles[0].name, "Default");
        assert_eq!(doc.events.len(), 2);
    }

    #[test]
    fn test_parse_empty_document() {
        let doc = AssDocument::parse("").unwrap();
        assert!(doc.styles.is_empty());
        assert!(doc.events.is_empty());
    }

    #[test]
    fn test_parse_ssa_v4_document() {
        let ssa = r#"[Script Info]
Title: SSA Test
ScriptType: v4.00
PlayResX: 640
PlayResY: 480

[V4 Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, TertiaryColour, BackColour, Bold, Italic, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, AlphaLevel, Encoding
Style: Default,Arial,32,&HFFFF00,&H0000FF,&H000000,&H800000,-1,0,1,2,1,6,10,10,20,0,1

[Events]
Format: Marked, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: Marked=0,0:00:01.00,0:00:04.00,Default,,0,0,0,,Hello SSA
"#;
        let doc = AssDocument::parse(ssa).unwrap();
        assert_eq!(doc.styles.len(), 1);
        // SSA alignment 6 (top-center) is converted to numpad 8
        assert_eq!(doc.styles[0].alignment, 8);
        assert_eq!(doc.styles[0].font_size, 32.0);
        assert_eq!(doc.events.len(), 1);
    }

    #[test]
    fn test_parse_embedded_fonts_and_graphics() {
        // "ABC" encodes to "15*$" in the SSA uuencode variant.
        // Headers are section-aware: fontname: in [Fonts],
        // filename: in [Graphics].
        let ass = "[Script Info]\nScriptType: v4.00+\n\n[Fonts]\nfontname: Test.ttf\n15*$\n\n[Graphics]\nfilename: logo.bmp\n15*$\n";
        let doc = AssDocument::parse(ass).unwrap();

        assert_eq!(doc.attachments.len(), 2);
        assert_eq!(doc.attachments[0].filename, "Test.ttf");
        assert_eq!(doc.attachments[0].kind, crate::types::AttachmentKind::Font);
        assert_eq!(doc.attachments[0].data, b"ABC");
        assert_eq!(doc.attachments[1].filename, "logo.bmp");
        assert_eq!(
            doc.attachments[1].kind,
            crate::types::AttachmentKind::Graphic
        );
        assert_eq!(doc.attachments[1].data, b"ABC");
    }

    #[test]
    fn test_parse_invalid_content() {
        assert!(AssDocument::parse("This is not valid ASS content").is_err());
    }

    #[test]
    fn test_parse_unknown_section_only() {
        assert!(AssDocument::parse("[Bogus Section]\nfoo: bar").is_err());
    }

    #[test]
    fn test_parse_comments_only() {
        assert!(AssDocument::parse("; just a comment\n; another").is_err());
    }

    #[test]
    fn test_get_event_count() {
        let doc = AssDocument::parse(TEST_ASS).unwrap();
        assert_eq!(doc.get_event_count(), 2);
    }

    #[test]
    fn test_get_style_count() {
        let doc = AssDocument::parse(TEST_ASS).unwrap();
        assert_eq!(doc.get_style_count(), 1);
    }

    #[test]
    fn test_find_style() {
        let doc = AssDocument::parse(TEST_ASS).unwrap();
        let style = doc.find_style("Default");
        assert!(style.is_some());
        assert_eq!(style.unwrap().font_name, "Arial");

        let style = doc.find_style("NonExistent");
        assert!(style.is_none());
    }

    #[test]
    fn test_get_events_at_time() {
        let doc = AssDocument::parse(TEST_ASS).unwrap();

        // At 2000ms, both the dialogue and comment are active
        let active = doc.get_events_at_time(2000);
        assert_eq!(active.len(), 2);

        // At 5000ms, only the comment is active (dialogue ended at 4000ms)
        let active = doc.get_events_at_time(5000);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].event_type, EventType::Comment);

        // At 31000ms, no events are active
        let active = doc.get_events_at_time(31000);
        assert!(active.is_empty());
    }

    #[test]
    fn test_get_dialogue_events() {
        let doc = AssDocument::parse(TEST_ASS).unwrap();
        let dialogues = doc.get_dialogue_events();
        assert_eq!(dialogues.len(), 1);
    }

    #[test]
    fn test_get_comment_events() {
        let doc = AssDocument::parse(TEST_ASS).unwrap();
        let comments = doc.get_comment_events();
        assert_eq!(comments.len(), 1);
    }

    #[test]
    fn test_error_line_numbers_are_exact() {
        let ass = "[Script Info]\nTitle: x\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:04.00,Default,,0,0,0,,ok\nDialogue: BROKEN LINE\n";
        let err = AssDocument::parse(ass).unwrap_err();
        // The broken line is source line 7
        assert!(err.to_string().contains("line 7"), "{}", err);
    }

    #[test]
    fn test_duplicate_sections_append() {
        let ass = "[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,one\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:03.00,0:00:04.00,Default,,0,0,0,,two\n";
        let doc = AssDocument::parse(ass).unwrap();
        assert_eq!(doc.events.len(), 2);
        assert_eq!(doc.events[1].text, "two");
    }

    #[test]
    fn test_unknown_section_does_not_leak() {
        // Lines under an unknown section must not attach to Events,
        // and Events parsing must resume afterwards.
        let ass = "[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,one\n\n[Bogus]\nDialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,ghost\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:03.00,0:00:04.00,Default,,0,0,0,,two\n";
        let doc = AssDocument::parse(ass).unwrap();
        assert_eq!(doc.events.len(), 2);
        assert!(!doc.events.iter().any(|e| e.text == "ghost"));
    }

    #[test]
    fn test_script_info_only_document() {
        // Valid: metadata with no styles/events renders nothing.
        let doc = AssDocument::parse("[Script Info]\nTitle: empty\n").unwrap();
        assert!(doc.styles.is_empty() && doc.events.is_empty());
    }

    #[test]
    fn test_global_count_boundaries() {
        // Pure over lengths: exact cap passes, cap+1 fails, and usize
        // overflow fails instead of wrapping around the cap.
        assert_eq!(check_global_count(99, 1, 100, "things", 0).unwrap(), 100);
        assert!(check_global_count(100, 1, 100, "things", 0).is_err());
        assert!(check_global_count(usize::MAX, 1, 100, "things", 0).is_err());
        assert!(check_global_count(usize::MAX, usize::MAX, 100, "things", 0).is_err());
        assert_eq!(check_global_count(0, 0, 100, "things", 0).unwrap(), 0);
    }

    #[test]
    fn test_global_attachments_wiring() {
        use crate::types::AttachmentKind;
        // Small totals append fine and the byte accessor sums them.
        let mut doc = AssDocument::new();
        let one = Attachment {
            kind: AttachmentKind::Font,
            filename: "a.ttf".to_string(),
            data: vec![0u8; 3],
        };
        let two = Attachment {
            kind: AttachmentKind::Graphic,
            filename: "b.bmp".to_string(),
            data: vec![0u8; 5],
        };
        check_global_attachments(&doc, std::slice::from_ref(&one), 0).unwrap();
        doc.attachments.push(one);
        check_global_attachments(&doc, std::slice::from_ref(&two), 0).unwrap();
        doc.attachments.push(two);
        assert_eq!(doc.total_attachment_bytes(), 8);
        // Count cap enforced across the accumulated vec: 257 empty
        // attachments trip the count check before bytes matter.
        let empty = Attachment {
            kind: AttachmentKind::Font,
            filename: "e.ttf".to_string(),
            data: Vec::new(),
        };
        let many = vec![empty; attachment::MAX_ATTACHMENTS + 1];
        let err = check_global_attachments(&AssDocument::new(), &many, 7)
            .unwrap_err()
            .to_string();
        assert!(err.contains("line 7"), "{err}");
        assert!(err.contains("Too many attachments"), "{err}");
    }
}
