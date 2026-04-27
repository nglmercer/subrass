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

    pub fn parse_from_line(line: &str) -> Result<Self, String> {
        let line = line.trim();

        let event_type = if line.starts_with("Dialogue:") {
            EventType::Dialogue
        } else if line.starts_with("Comment:") {
            EventType::Comment
        } else {
            return Err(format!("Invalid event type: {}", line));
        };

        let content = match event_type {
            EventType::Dialogue => line.strip_prefix("Dialogue:").unwrap(),
            EventType::Comment => line.strip_prefix("Comment:").unwrap(),
        };

        let fields: Vec<&str> = content.splitn(10, ',').collect();
        if fields.len() < 10 {
            return Err(format!(
                "Expected 10 fields in event, got {}",
                fields.len()
            ));
        }

        let parse_i32 = |s: &str| -> i32 { s.trim().parse().unwrap_or(0) };

        let start: Time = fields[1]
            .trim()
            .parse()
            .map_err(|_| format!("Invalid start time: {}", fields[1]))?;
        let end: Time = fields[2]
            .trim()
            .parse()
            .map_err(|_| format!("Invalid end time: {}", fields[2]))?;

        let text = fields[9].to_string();
        let parsed_tags = OverrideTag::parse_from_text(&text);

        Ok(Self {
            event_type,
            layer: parse_i32(fields[0]),
            start,
            end,
            style: fields[3].trim().to_string(),
            name: fields[4].trim().to_string(),
            margin_l: parse_i32(fields[5]),
            margin_r: parse_i32(fields[6]),
            margin_v: parse_i32(fields[7]),
            effect: fields[8].trim().to_string(),
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

pub const DEFAULT_EVENT_FORMAT: &str = "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_dialogue() {
        let line =
            "Dialogue: 0,0:00:01.00,0:00:04.00,Default,John,0,0,0,,Hello World!";
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
        let line =
            "Comment: 0,0:00:00.00,0:00:30.00,Default,,0,0,0,,This is a comment";
        let event = Event::parse_from_line(line).unwrap();

        assert_eq!(event.event_type, EventType::Comment);
        assert!(event.is_comment());
    }

    #[test]
    fn test_parse_with_override_tags() {
        let line =
            "Dialogue: 0,0:00:01.00,0:00:04.00,Default,,0,0,0,,{\\pos(100,200)}Hello";
        let event = Event::parse_from_line(line).unwrap();

        assert!(!event.parsed_tags.is_empty());
    }

    #[test]
    fn test_duration_millis() {
        let line =
            "Dialogue: 0,0:00:01.00,0:00:04.00,Default,,0,0,0,,Hello";
        let event = Event::parse_from_line(line).unwrap();

        assert_eq!(event.duration_millis(), 3000);
    }

    #[test]
    fn test_is_active_at() {
        let line =
            "Dialogue: 0,0:00:01.00,0:00:04.00,Default,,0,0,0,,Hello";
        let event = Event::parse_from_line(line).unwrap();

        assert!(!event.is_active_at(500));
        assert!(event.is_active_at(1000));
        assert!(event.is_active_at(3999));
        assert!(!event.is_active_at(4000));
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
