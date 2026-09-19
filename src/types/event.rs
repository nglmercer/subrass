use serde::{Deserialize, Serialize};

use super::override_tag::OverrideTag;
use super::time::Time;

/// Event type (Dialogue or Comment)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventType {
    Dialogue,
    Comment,
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dialogue => write!(f, "Dialogue"),
            Self::Comment => write!(f, "Comment"),
        }
    }
}

/// Subtitle event from [Events] section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub event_type: EventType,
    pub layer: i32,
    pub start: Time,
    pub end: Time,
    pub style: String,
    pub name: String,
    pub margin_l: i32,
    pub margin_r: i32,
    pub margin_v: i32,
    pub effect: String,
    pub text: String,
    pub parsed_tags: Vec<OverrideTag>,
}

impl Event {
    pub fn new(event_type: EventType, style: &str) -> Self {
        Self {
            event_type,
            layer: 0,
            start: Time::zero(),
            end: Time::zero(),
            style: style.to_string(),
            name: String::new(),
            margin_l: 0,
            margin_r: 0,
            margin_v: 0,
            effect: String::new(),
            text: String::new(),
            parsed_tags: Vec::new(),
        }
    }

    /// Parse with the canonical ASS field order.
    pub fn parse_from_line(line: &str) -> Result<Self, String> {
        Self::parse_from_line_with_format(line, None)
    }

    /// Parse an event line using the section's `Format:` column declaration.
    ///
    /// `format` holds lowercased column names. When `None`, the canonical
    /// ASS order (Layer, Start, End, Style, Name, MarginL, MarginR, MarginV,
    /// Effect, Text) is assumed. The `Text` column must be last since it
    /// may contain commas. Unknown columns are ignored; missing required
    /// columns (Start, End, Style, Text, plus Layer or SSA Marked) are
    /// errors. Numeric fields must parse — malformed values are errors,
    /// never silent zeros.
    pub fn parse_from_line_with_format(
        line: &str,
        format: Option<&[String]>,
    ) -> Result<Self, String> {
        let line = line.trim();

        let (event_type, content) = if let Some(rest) = strip_prefix_ci(line, "Dialogue:") {
            (EventType::Dialogue, rest)
        } else if let Some(rest) = strip_prefix_ci(line, "Comment:") {
            (EventType::Comment, rest)
        } else {
            return Err(format!("Invalid event type: {}", line));
        };

        let columns: Vec<String> = match format {
            Some(cols) if !cols.is_empty() => cols.to_vec(),
            _ => DEFAULT_EVENT_COLUMNS
                .iter()
                .map(|s| s.to_string())
                .collect(),
        };

        // Text must be last: it may contain commas.
        if let Some(text_pos) = columns.iter().position(|c| c == "text") {
            if text_pos != columns.len() - 1 {
                return Err(format!(
                    "Text column must be last in Events Format, got: {}",
                    columns.join(", ")
                ));
            }
        } else {
            return Err("Events Format is missing the required Text column".to_string());
        }
        for required in ["start", "end", "style"] {
            if !columns.iter().any(|c| c == required) {
                return Err(format!(
                    "Events Format is missing the required {} column",
                    capitalize(required)
                ));
            }
        }
        if !columns.iter().any(|c| c == "layer" || c == "marked") {
            return Err("Events Format is missing the required Layer column".to_string());
        }

        let fields: Vec<&str> = content.splitn(columns.len(), ',').collect();
        if fields.len() < columns.len() {
            return Err(format!(
                "Expected {} fields in event, got {}",
                columns.len(),
                fields.len()
            ));
        }

        let field = |name: &str| -> Option<&str> {
            columns
                .iter()
                .position(|c| c == name)
                .map(|i| fields[i].trim())
        };
        let parse_i32 = |name: &str, what: &str| -> Result<i32, String> {
            match field(name) {
                None | Some("") => Ok(0),
                Some(v) => v
                    .parse()
                    .map_err(|_| format!("Invalid {} value: {}", what, v)),
            }
        };

        // SSA "Marked=N" carries no layer; ASS Layer parses strictly.
        let layer = if columns.iter().any(|c| c == "layer") {
            parse_i32("layer", "Layer")?
        } else {
            0
        };

        let start: Time = field("start").unwrap_or("").parse().map_err(|e| {
            format!(
                "Invalid Start value {:?}: {}",
                field("start").unwrap_or(""),
                e
            )
        })?;
        let end: Time = field("end")
            .unwrap_or("")
            .parse()
            .map_err(|e| format!("Invalid End value {:?}: {}", field("end").unwrap_or(""), e))?;

        let text = field("text").unwrap_or("").to_string();
        let parsed_tags = OverrideTag::parse_from_text(&text);

        Ok(Self {
            event_type,
            layer,
            start,
            end,
            style: field("style").unwrap_or("").to_string(),
            name: field("name").unwrap_or("").to_string(),
            margin_l: parse_i32("marginl", "MarginL")?,
            margin_r: parse_i32("marginr", "MarginR")?,
            margin_v: parse_i32("marginv", "MarginV")?,
            effect: field("effect").unwrap_or("").to_string(),
            text,
            parsed_tags,
        })
    }

    pub fn to_line(&self) -> String {
        format!(
            "{}: {},{},{},{},{},{},{},{},{},{}",
            self.event_type,
            self.layer,
            self.start,
            self.end,
            self.style,
            self.name,
            self.margin_l,
            self.margin_r,
            self.margin_v,
            self.effect,
            self.text
        )
    }

    pub fn duration_millis(&self) -> u64 {
        if self.end >= self.start {
            self.end.to_millis() - self.start.to_millis()
        } else {
            0
        }
    }

    pub fn is_active_at(&self, time_ms: u64) -> bool {
        let start_ms = self.start.to_millis();
        let end_ms = self.end.to_millis();
        time_ms >= start_ms && time_ms < end_ms
    }

    pub fn is_dialogue(&self) -> bool {
        self.event_type == EventType::Dialogue
    }

    pub fn is_comment(&self) -> bool {
        self.event_type == EventType::Comment
    }
}

pub const DEFAULT_EVENT_FORMAT: &str =
    "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text";

/// Canonical ASS event columns, lowercased.
const DEFAULT_EVENT_COLUMNS: &[&str] = &[
    "layer", "start", "end", "style", "name", "marginl", "marginr", "marginv", "effect", "text",
];

/// Case-insensitive prefix strip (ASS keywords are matched case-insensitively
/// for compatibility with real-world files).
fn strip_prefix_ci<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    // Byte-based: `line[..prefix.len()]` would panic on multibyte input
    // (a byte length is not a char boundary). ASCII prefixes only.
    debug_assert!(prefix.is_ascii());
    if line.len() >= prefix.len()
        && line.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
    {
        // The ASCII prefix matched, so the boundary is safe.
        Some(&line[prefix.len()..])
    } else {
        None
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_dialogue() {
        let line = "Dialogue: 0,0:00:01.00,0:00:04.00,Default,John,0,0,0,,Hello World!";
        let event = Event::parse_from_line(line).unwrap();

        assert_eq!(event.event_type, EventType::Dialogue);
        assert_eq!(event.layer, 0);
        assert_eq!(event.start, Time::new(0, 0, 1, 0));
        assert_eq!(event.end, Time::new(0, 0, 4, 0));
        assert_eq!(event.style, "Default");
        assert_eq!(event.name, "John");
        assert_eq!(event.text, "Hello World!");
    }

    #[test]
    fn test_parse_comment() {
        let line = "Comment: 0,0:00:00.00,0:00:30.00,Default,,0,0,0,,This is a comment";
        let event = Event::parse_from_line(line).unwrap();

        assert_eq!(event.event_type, EventType::Comment);
        assert!(event.is_comment());
    }

    #[test]
    fn test_parse_with_override_tags() {
        let line = "Dialogue: 0,0:00:01.00,0:00:04.00,Default,,0,0,0,,{\\pos(100,200)}Hello";
        let event = Event::parse_from_line(line).unwrap();

        assert!(!event.parsed_tags.is_empty());
    }

    #[test]
    fn test_duration_millis() {
        let line = "Dialogue: 0,0:00:01.00,0:00:04.00,Default,,0,0,0,,Hello";
        let event = Event::parse_from_line(line).unwrap();

        assert_eq!(event.duration_millis(), 3000);
    }

    #[test]
    fn test_is_active_at() {
        let line = "Dialogue: 0,0:00:01.00,0:00:04.00,Default,,0,0,0,,Hello";
        let event = Event::parse_from_line(line).unwrap();

        assert!(!event.is_active_at(500));
        assert!(event.is_active_at(1000));
        assert!(event.is_active_at(3999));
        assert!(!event.is_active_at(4000));
    }

    /// Plan #59: legacy SSA event rows (`Marked=`, no Layer).
    #[test]
    fn test_parse_ssa_marked_rows() {
        // Canonical SSA order: Marked first, Text last.
        let format: Vec<String> = [
            "marked", "start", "end", "style", "name", "marginl", "marginr", "marginv", "effect",
            "text",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        for marked in ["Marked=0", "Marked=1"] {
            let line = format!(
                "Dialogue: {},0:00:01.00,0:00:04.00,Default,Actor,0,0,0,,Hello",
                marked
            );
            let event = Event::parse_from_line_with_format(&line, Some(&format)).unwrap();
            assert_eq!(event.event_type, EventType::Dialogue);
            assert_eq!(event.layer, 0);
            assert_eq!(event.name, "Actor");
            assert_eq!(event.text, "Hello");
        }
    }

    /// Plan #59: unusual field ordering, commas inside Text, and
    /// SSA Effect values ride through by column name.
    #[test]
    fn test_parse_unusual_field_order() {
        // Shuffled columns with Text still last; commas inside Text
        // must not split into phantom columns.
        let format: Vec<String> = ["text", "style", "end", "start", "marked"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        // Text-first violates the Text-last rule: rejected, not misparsed.
        let line = "Dialogue: Hello,Default,0:00:04.00,0:00:01.00,Marked=0";
        assert!(Event::parse_from_line_with_format(line, Some(&format)).is_err());

        let format: Vec<String> = ["marked", "style", "end", "start", "name", "text"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let line =
            "Dialogue: Marked=0,Default,0:00:04.00,0:00:01.00,Actor,Hello, world, with commas";
        let event = Event::parse_from_line_with_format(line, Some(&format)).unwrap();
        assert_eq!(event.start, Time::new(0, 0, 1, 0));
        assert_eq!(event.end, Time::new(0, 0, 4, 0));
        assert_eq!(event.text, "Hello, world, with commas");
        // Missing Layer/Marked and missing Text are errors.
        let no_layer: Vec<String> = ["start", "end", "style", "text"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(Event::parse_from_line_with_format(
            "Dialogue: 0:00:01.00,0:00:04.00,Default,Hi",
            Some(&no_layer)
        )
        .is_err());
    }

    #[test]
    fn test_event_to_line() {
        let mut event = Event::new(EventType::Dialogue, "Default");
        event.start = Time::new(0, 0, 1, 0);
        event.end = Time::new(0, 0, 4, 0);
        event.text = "Hello World!".to_string();

        let line = event.to_line();
        assert!(line.starts_with("Dialogue:"));
        assert!(line.contains("Default"));
        assert!(line.contains("Hello World!"));
    }
}
