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
        // Repeated style/event sections append: earlier data is kept.
        Section::V4PlusStyles => {
            doc.styles
                .extend(style::parse_styles(lines, first_content_line, false)?);
        }
        Section::V4Styles => {
            doc.styles
                .extend(style::parse_styles(lines, first_content_line, true)?);
        }
        Section::Events => {
            doc.events
                .extend(event::parse_events(lines, first_content_line)?);
        }
        Section::Fonts => {
            doc.attachments.extend(attachment::parse_attachments(
                lines,
                AttachmentKind::Font,
                first_content_line,
            )?);
        }
        Section::Graphics => {
            doc.attachments.extend(attachment::parse_attachments(
                lines,
                AttachmentKind::Graphic,
                first_content_line,
            )?);
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
        // "ABC" encodes to "15*$" in the SSA uuencode variant
        let ass = "[Script Info]\nScriptType: v4.00+\n\n[Fonts]\nfontname: Test.ttf\n15*$\n\n[Graphics]\nfontname: logo.bmp\n15*$\n";
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
}
