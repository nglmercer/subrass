use crate::types::Event;

use super::errors::ParseError;

/// Defensive cap on events per file (untrusted subtitle input).
pub const MAX_EVENTS: usize = 100_000;

pub fn parse_events(lines: &[&str], start_line: usize) -> Result<Vec<Event>, ParseError> {
    let mut events = Vec::new();
    let mut format: Option<Vec<String>> = None;

    for (i, line) in lines.iter().enumerate() {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with(';') {
            continue;
        }

        // Track the Format declaration (case-insensitive keyword)
        if let Some(fmt) = strip_prefix_ci(line, "Format:") {
            format = Some(parse_format_columns(fmt));
            continue;
        }

        // Parse dialogue and comment lines (case-insensitive keyword)
        if starts_with_ci(line, "Dialogue:") || starts_with_ci(line, "Comment:") {
            if events.len() >= MAX_EVENTS {
                return Err(ParseError::line_error(
                    start_line + i,
                    format!("Too many events (limit {MAX_EVENTS})"),
                ));
            }
            let event = Event::parse_from_line_with_format(line, format.as_deref())
                .map_err(|e| ParseError::line_error(start_line + i, e))?;
            events.push(event);
        }
    }

    Ok(events)
}

/// Byte-preserving event parser used by `AssDocument::parse_bytes`.
/// Non-event section data may be decoded lossily, but the Text field is kept
/// on `Event` until the renderer knows the active ASS charset/`\fe` state.
pub fn parse_events_bytes(lines: &[&[u8]], start_line: usize) -> Result<Vec<Event>, ParseError> {
    let mut events = Vec::new();
    let mut format: Option<Vec<String>> = None;

    for (i, raw_line) in lines.iter().enumerate() {
        let line = trim_ascii_bytes(raw_line);
        if line.is_empty() || line.first() == Some(&b';') {
            continue;
        }

        if let Some(fmt) = strip_prefix_ci_bytes(line, b"Format:") {
            format = Some(parse_format_columns(&String::from_utf8_lossy(fmt)));
            continue;
        }

        if starts_with_ci_bytes(line, b"Dialogue:") || starts_with_ci_bytes(line, b"Comment:") {
            if events.len() >= MAX_EVENTS {
                return Err(ParseError::line_error(
                    start_line + i,
                    format!("Too many events (limit {MAX_EVENTS})"),
                ));
            }
            let event = Event::parse_from_bytes_with_format(line, format.as_deref())
                .map_err(|e| ParseError::line_error(start_line + i, e))?;
            events.push(event);
        }
    }

    Ok(events)
}

fn parse_format_columns(fmt: &str) -> Vec<String> {
    fmt.split(',')
        .map(|c| c.trim().to_lowercase())
        .filter(|c| !c.is_empty())
        .collect()
}

fn starts_with_ci(line: &str, prefix: &str) -> bool {
    // Byte-based: `line[..prefix.len()]` would panic on multibyte input
    // (a byte length is not a char boundary). ASCII prefixes only.
    debug_assert!(prefix.is_ascii());
    line.len() >= prefix.len()
        && line.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

fn strip_prefix_ci<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    if starts_with_ci(line, prefix) {
        // The ASCII prefix matched, so the boundary is safe.
        Some(&line[prefix.len()..])
    } else {
        None
    }
}

fn starts_with_ci_bytes(line: &[u8], prefix: &[u8]) -> bool {
    line.get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

fn strip_prefix_ci_bytes<'a>(line: &'a [u8], prefix: &[u8]) -> Option<&'a [u8]> {
    if starts_with_ci_bytes(line, prefix) {
        Some(&line[prefix.len()..])
    } else {
        None
    }
}

fn trim_ascii_bytes(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(|b| b.is_ascii_whitespace()) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(|b| b.is_ascii_whitespace()) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
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

/// Stable sort by layer: equal layers keep source order (ASS behavior).
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
    fn test_multibyte_lines_skip_without_panicking() {
        // Keyword-prefix slicing at byte lengths must never split a
        // char; unrecognized lines are skipped, not fatal.
        let lines = vec![
            "éééééééééé,0:00:01.00,0:00:04.00,Default,,0,0,0,,x",
            "Dialogué: 0,0:00:01.00,0:00:04.00,Default,,0,0,0,,x",
            "Formaté: Layer, Start",
        ];
        let events = parse_events(&lines, 0).unwrap();
        assert!(events.is_empty());
        assert!(Event::parse_from_line("éééééééééé").is_err());
    }

    #[test]
    fn test_reordered_format_columns() {
        let lines = vec![
            "Format: Start, End, Style, Text, Layer, Name, MarginL, MarginR, MarginV, Effect",
            "Dialogue: 0:00:01.00,0:00:04.00,Default,Hello,2,John,1,2,3,",
        ];
        // Text is not last here, so this must be rejected, not misparsed
        assert!(parse_events(&lines, 0).is_err());

        let lines = vec![
            "Format: Style, Layer, Start, End, Name, MarginL, MarginR, MarginV, Effect, Text",
            "Dialogue: Default,2,0:00:01.00,0:00:04.00,John,1,2,3,,Hello, with comma",
        ];
        let events = parse_events(&lines, 0).unwrap();
        assert_eq!(events[0].layer, 2);
        assert_eq!(events[0].style, "Default");
        assert_eq!(events[0].margin_v, 3);
        assert_eq!(events[0].text, "Hello, with comma");
    }

    #[test]
    fn test_ssa_marked_format() {
        let lines = vec![
            "Format: Marked, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text",
            "Dialogue: Marked=0,0:00:01.00,0:00:04.00,Default,,0,0,0,,Hello SSA",
        ];
        let events = parse_events(&lines, 0).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].layer, 0);
        assert_eq!(events[0].text, "Hello SSA");
    }

    #[test]
    fn test_unknown_columns_ignored() {
        let lines = vec![
            "Format: Layer, Start, End, Style, Future, Text",
            "Dialogue: 0,0:00:01.00,0:00:04.00,Default,whatever,Hello",
        ];
        let events = parse_events(&lines, 0).unwrap();
        assert_eq!(events[0].text, "Hello");
    }

    #[test]
    fn test_missing_required_column_errors() {
        let lines = vec![
            "Format: Layer, Start, Style, Text",
            "Dialogue: 0,0:00:01.00,Default,Hello",
        ];
        assert!(parse_events(&lines, 0).is_err());
    }

    #[test]
    fn test_malformed_numbers_error() {
        let lines = vec![
            "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text",
            "Dialogue: abc,0:00:01.00,0:00:04.00,Default,,0,0,0,,Hello",
        ];
        assert!(parse_events(&lines, 0).is_err());

        let lines = vec![
            "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text",
            "Dialogue: 0,0:00:01.00,0:00:04.00,Default,,xyz,0,0,,Hello",
        ];
        assert!(parse_events(&lines, 0).is_err());
    }

    #[test]
    fn test_malformed_timestamp_errors() {
        let lines = vec![
            "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text",
            "Dialogue: 0,0:99:99.99,0:00:04.00,Default,,0,0,0,,Hello",
        ];
        assert!(parse_events(&lines, 0).is_err());
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

    #[test]
    fn test_layer_sort_is_stable() {
        let lines = vec![
            "Dialogue: 1,0:00:01.00,0:00:04.00,Default,,0,0,0,,First",
            "Dialogue: 1,0:00:01.00,0:00:04.00,Default,,0,0,0,,Second",
            "Dialogue: 0,0:00:01.00,0:00:04.00,Default,,0,0,0,,Third",
        ];
        let mut events = parse_events(&lines, 0).unwrap();
        sort_events_by_layer(&mut events);
        assert_eq!(events[0].text, "Third");
        assert_eq!(events[1].text, "First");
        assert_eq!(events[2].text, "Second");
    }
}
