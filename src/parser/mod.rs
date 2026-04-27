pub mod errors;
pub mod event;
pub mod script_info;
pub mod style;

use crate::types::{Event, ScriptInfo, Style};
use errors::{ParseError, Section};

/// Complete ASS document
#[derive(Debug, Clone)]
pub struct AssDocument {
    pub script_info: ScriptInfo,
    pub styles: Vec<Style>,
    pub events: Vec<Event>,
}

impl AssDocument {
    pub fn new() -> Self {
        Self {
            script_info: ScriptInfo::new(),
            styles: Vec::new(),
            events: Vec::new(),
        }
    }

    /// Parse an ASS file from a string
    pub fn parse(input: &str) -> Result<Self, ParseError> {
        let mut doc = AssDocument::new();
        let mut current_section: Option<Section> = None;
        let mut section_lines: Vec<&str> = Vec::new();
        let mut line_number = 0;
        let mut section_start_line = 0;

        for line in input.lines() {
            line_number += 1;
            let trimmed = line.trim();

            // Check for section headers
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                // Process previous section
                if let Some(section) = current_section {
                    process_section(&mut doc, section, &section_lines, section_start_line)?;
                }

                // Start new section
                current_section = Section::from_header(trimmed);
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
            process_section(&mut doc, section, &section_lines, section_start_line)?;
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
    start_line: usize,
) -> Result<(), ParseError> {
    match section {
        Section::ScriptInfo => {
            doc.script_info = script_info::parse_script_info(lines, start_line)?;
        }
        Section::V4PlusStyles | Section::V4Styles => {
            doc.styles = style::parse_styles(lines, start_line)?;
        }
        Section::Events => {
            doc.events = event::parse_events(lines, start_line)?;
        }
        _ => {
            // Fonts, Graphics - not implemented yet
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
}
