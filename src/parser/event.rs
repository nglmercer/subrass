use crate::types::Event;

use super::errors::ParseError;

pub fn parse_events(lines: &[&str], start_line: usize) -> Result<Vec<Event>, ParseError> {
    let mut events = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let line = line.trim();

        // Skip empty lines
        if line.is_empty() {
            continue;
        }

        // Skip format lines
        if line.starts_with("Format:") {
            continue;
        }

        // Parse dialogue and comment lines
        if line.starts_with("Dialogue:") || line.starts_with("Comment:") {
            let event = Event::parse_from_line(line)
                .map_err(|e| ParseError::line_error(start_line + i, e))?;
            events.push(event);
        }
    }

    Ok(events)
}

pub fn get_events_at_time(events: &[Event], time_ms: u64) -> Vec<&Event> {
    events.iter().filter(|e| e.is_active_at(time_ms)).collect()
}

pub fn get_dialogue_events(events: &[Event]) -> Vec<&Event> {
    events.iter().filter(|e| e.is_dialogue()).collect()
}

pub fn get_comment_events(events: &[Event]) -> Vec<&Event> {
    events.iter().filter(|e| e.is_comment()).collect()
}

pub fn sort_events_by_layer(events: &mut [Event]) {
    events.sort_by_key(|a| a.layer);
}

pub fn sort_events_by_time(events: &mut [Event]) {
    events.sort_by_key(|a| a.start);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::EventType;

    #[test]
    fn test_parse_events_basic() {
        let lines = vec![
            "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text",
            "Dialogue: 0,0:00:01.00,0:00:04.00,Default,John,0,0,0,,Hello World!",
            "Comment: 0,0:00:00.00,0:00:30.00,Default,,0,0,0,,This is a comment",
        ];

        let events = parse_events(&lines, 0).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, EventType::Dialogue);
        assert_eq!(events[1].event_type, EventType::Comment);
    }

    #[test]
    fn test_parse_events_with_override_tags() {
        let lines = vec!["Dialogue: 0,0:00:01.00,0:00:04.00,Default,,0,0,0,,{\\pos(100,200)}Hello"];

        let events = parse_events(&lines, 0).unwrap();
        assert_eq!(events.len(), 1);
        assert!(!events[0].parsed_tags.is_empty());
    }

    #[test]
    fn test_get_events_at_time() {
        let lines = vec![
            "Dialogue: 0,0:00:01.00,0:00:04.00,Default,,0,0,0,,Event 1",
            "Dialogue: 0,0:00:03.00,0:00:06.00,Default,,0,0,0,,Event 2",
            "Dialogue: 0,0:00:05.00,0:00:08.00,Default,,0,0,0,,Event 3",
        ];

        let events = parse_events(&lines, 0).unwrap();

        // At 2000ms, only Event 1 should be active
        let active = get_events_at_time(&events, 2000);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].text, "Event 1");

        // At 3500ms, Event 1 and Event 2 should be active
        let active = get_events_at_time(&events, 3500);
        assert_eq!(active.len(), 2);

        // At 5500ms, Event 2 and Event 3 should be active
        let active = get_events_at_time(&events, 5500);
        assert_eq!(active.len(), 2);

        // At 0ms, no events should be active
        let active = get_events_at_time(&events, 0);
        assert!(active.is_empty());
    }

    #[test]
    fn test_get_dialogue_events() {
        let lines = vec![
            "Dialogue: 0,0:00:01.00,0:00:04.00,Default,,0,0,0,,Hello",
            "Comment: 0,0:00:00.00,0:00:30.00,Default,,0,0,0,,Comment",
        ];

        let events = parse_events(&lines, 0).unwrap();
        let dialogues = get_dialogue_events(&events);
        assert_eq!(dialogues.len(), 1);
    }

    #[test]
    fn test_sort_events_by_time() {
        let lines = vec![
            "Dialogue: 0,0:00:05.00,0:00:08.00,Default,,0,0,0,,Event 3",
            "Dialogue: 0,0:00:01.00,0:00:04.00,Default,,0,0,0,,Event 1",
            "Dialogue: 0,0:00:03.00,0:00:06.00,Default,,0,0,0,,Event 2",
        ];

        let mut events = parse_events(&lines, 0).unwrap();
        sort_events_by_time(&mut events);

        assert_eq!(events[0].text, "Event 1");
        assert_eq!(events[1].text, "Event 2");
        assert_eq!(events[2].text, "Event 3");
    }
}
