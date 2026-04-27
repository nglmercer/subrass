use serde::{Deserialize, Serialize};

use super::color::Color;

/// A segment of text with its associated override tags
#[derive(Debug, Clone)]
pub struct TextSegment {
    pub text: String,
    pub tags: Vec<OverrideTag>,
}

/// Override tags in ASS text
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OverrideTag {
    // Text formatting
    Bold(bool),
    Italic(bool),
    Underline(bool),
    StrikeOut(bool),
    FontName(String),
    FontSize(f64),
    FontSizeMultiplier(f64),
    LetterSpacing(f64),
    Kerning(bool),

    // Colors and alpha
    PrimaryColor(Color),
    SecondaryColor(Color),
    OutlineColor(Color),
    ShadowColor(Color),
    Alpha(u8),
    PrimaryAlpha(u8),
    SecondaryAlpha(u8),
    OutlineAlpha(u8),
    ShadowAlpha(u8),

    // Positioning
    Position(f64, f64),
    Move(f64, f64, f64, f64),
    MoveWithTiming(f64, f64, f64, f64, u64, u64),
    Origin(f64, f64),
    Alignment(i32),

    // Transformations
    RotationX(f64),
    RotationY(f64),
    RotationZ(f64),
    ScaleX(f64),
    ScaleY(f64),
    ShearX(f64),
    ShearY(f64),

    // Borders and shadows
    Border(f64),
    BorderX(f64),
    BorderY(f64),
    Shadow(f64),
    ShadowX(f64),
    ShadowY(f64),
    EdgeBlur(f64),
    Blur(f64),

    // Fading
    Fade(u64, u64),
    ComplexFade(u64, u64, u64, u64, u64, u64, u64),

    // Animation: \t(t1, t2, [accel,] tags...)
    Transform {
        t1: u64,
        t2: u64,
        accel: f64,
        tags: Vec<OverrideTag>,
    },

    // Clipping
    Clip(i32, i32, i32, i32),
    InverseClip(i32, i32, i32, i32),

    // Drawing
    Drawing(i32),

    // Karaoke
    KaraokeDuration(u64),
    KaraokeSweep(u64),
    KaraokeOutline(u64),

    // Line breaks
    HardLineBreak,

    // Unknown tag (for forward compatibility)
    Unknown(String),
}

impl OverrideTag {
    pub fn parse_from_text(text: &str) -> Vec<Self> {
        let mut tags = Vec::new();
        let mut chars = text.chars().peekable();

        while let Some(&c) = chars.peek() {
            if c == '{' {
                chars.next(); // consume '{'
                let mut tag_str = String::new();

                while let Some(&c) = chars.peek() {
                    if c == '}' {
                        chars.next(); // consume '}'
                        break;
                    }
                    tag_str.push(c);
                    chars.next();
                }

                // Parse the tag string
                for tag in parse_tag_group(&tag_str) {
                    tags.push(tag);
                }
            } else {
                chars.next();
            }
        }

        tags
    }

    pub fn is_positioning(&self) -> bool {
        matches!(
            self,
            Self::Position(..) | Self::Move(..) | Self::MoveWithTiming(..) | Self::Origin(..)
        )
    }

    pub fn is_transform(&self) -> bool {
        matches!(
            self,
            Self::RotationX(..)
                | Self::RotationY(..)
                | Self::RotationZ(..)
                | Self::ScaleX(..)
                | Self::ScaleY(..)
                | Self::ShearX(..)
                | Self::ShearY(..)
        )
    }

    pub fn is_color(&self) -> bool {
        matches!(
            self,
            Self::PrimaryColor(..)
                | Self::SecondaryColor(..)
                | Self::OutlineColor(..)
                | Self::ShadowColor(..)
                | Self::Alpha(..)
                | Self::PrimaryAlpha(..)
                | Self::SecondaryAlpha(..)
                | Self::OutlineAlpha(..)
                | Self::ShadowAlpha(..)
        )
    }

    pub fn is_animation(&self) -> bool {
        matches!(self, Self::Transform { .. })
    }
}

/// Parse text into segments, each with its accumulated override tags.
/// Text like `Hello {\c&H00FF00&}World` produces:
/// - Segment("Hello ", [])
/// - Segment("World", [PrimaryColor(green)])
pub fn parse_text_segments(text: &str) -> Vec<TextSegment> {
    let mut segments = Vec::new();
    let mut accumulated_tags: Vec<OverrideTag> = Vec::new();
    let mut current_text = String::new();
    let mut chars = text.chars().peekable();
    let mut in_drawing_mode = false;

    while let Some(&c) = chars.peek() {
        match c {
            '{' => {
                // Finalize current segment if any
                if !current_text.is_empty() {
                    segments.push(TextSegment {
                        text: current_text.clone(),
                        tags: accumulated_tags.clone(),
                    });
                    current_text.clear();
                }

                // Parse tag group
                chars.next(); // consume '{'
                let mut tag_str = String::new();
                while let Some(&c) = chars.peek() {
                    if c == '}' {
                        chars.next();
                        break;
                    }
                    tag_str.push(c);
                    chars.next();
                }

                // Parse tags
                for tag in parse_tag_group(&tag_str) {
                    // Track drawing mode
                    match &tag {
                        OverrideTag::Drawing(1) => in_drawing_mode = true,
                        OverrideTag::Drawing(0) => in_drawing_mode = false,
                        _ => {}
                    }
                    accumulated_tags.push(tag);
                }
            }
            '\\' => {
                chars.next(); // consume '\\'
                if let Some(&next) = chars.peek() {
                    match next {
                        'N' | 'n' => {
                            chars.next();
                            if !in_drawing_mode {
                                // If current_text is empty (right after a tag group),
                                // append line break to the previous segment
                                if current_text.is_empty() {
                                    if let Some(last) = segments.last_mut() {
                                        last.text.push('\n');
                                    }
                                } else {
                                    current_text.push('\n');
                                }
                            }
                        }
                        'h' => {
                            chars.next();
                            if !in_drawing_mode {
                                current_text.push('\u{00A0}');
                            }
                        }
                        _ => {
                            // Keep escape sequences as-is for other tags
                            current_text.push('\\');
                            current_text.push(next);
                            chars.next();
                        }
                    }
                }
            }
            _ => {
                current_text.push(c);
                chars.next();
            }
        }
    }

    // Final segment
    if !current_text.is_empty() {
        segments.push(TextSegment {
            text: current_text,
            tags: accumulated_tags,
        });
    }

    segments
}

/// Parse a group of override tags (content between { and }).
/// Handles nested tags like \t(t1,t2,\blur20\4c&H00BBB0&).
fn parse_tag_group(group: &str) -> Vec<OverrideTag> {
    let mut tags = Vec::new();
    let mut chars = group.chars().peekable();

    while let Some(&c) = chars.peek() {
        if c == '\\' {
            chars.next(); // consume '\\'

            // Read tag name: first char can be alpha or digit (for \4c, \1a),
            // then continue with alphabetic chars (e.g., "blur", "bord", "fad")
            let mut name = String::new();
            if let Some(&c) = chars.peek() {
                if c.is_alphanumeric() {
                    name.push(c);
                    chars.next();
                    // Continue reading alphabetic chars for the rest of the name
                    while let Some(&c) = chars.peek() {
                        if c.is_alphabetic() {
                            name.push(c);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                }
            }

            if name.is_empty() {
                continue;
            }

            // Check for '(' -> read params with depth tracking
            if let Some(&'(') = chars.peek() {
                chars.next(); // consume '('
                let mut param_str = String::new();
                let mut depth = 1u32;
                for c in chars.by_ref() {
                    match c {
                        '(' => depth += 1,
                        ')' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    param_str.push(c);
                }

                if let Some(tag) = parse_tag_with_params(&name, Some(&param_str)) {
                    tags.push(tag);
                }
            } else {
                // Read value until next backslash, brace, or special char
                let mut value = String::new();
                while let Some(&c) = chars.peek() {
                    if c == '\\' || c == '{' || c == '}' {
                        break;
                    }
                    value.push(c);
                    chars.next();
                }

                let params = if value.is_empty() {
                    None
                } else {
                    Some(value.as_str())
                };
                if let Some(tag) = parse_tag_with_params(&name, params) {
                    tags.push(tag);
                }
            }
        } else {
            chars.next();
        }
    }

    tags
}

fn parse_tag_with_params(name: &str, params: Option<&str>) -> Option<OverrideTag> {
    match name {
        "b" => {
            let val = params?.parse::<i32>().ok()?;
            Some(OverrideTag::Bold(val != 0))
        }
        "i" => {
            let val = params?.parse::<i32>().ok()?;
            Some(OverrideTag::Italic(val != 0))
        }
        "u" => {
            let val = params?.parse::<i32>().ok()?;
            Some(OverrideTag::Underline(val != 0))
        }
        "s" => {
            let val = params?.parse::<i32>().ok()?;
            Some(OverrideTag::StrikeOut(val != 0))
        }
        "fn" => Some(OverrideTag::FontName(params?.to_string())),
        "fs" => {
            let val = params?.parse().ok()?;
            Some(OverrideTag::FontSize(val))
        }
        "fsp" => {
            let val = params?.parse().ok()?;
            Some(OverrideTag::LetterSpacing(val))
        }
        "c" | "1c" => {
            let color_str = params?;
            let color = parse_ass_color_tag(color_str)?;
            Some(OverrideTag::PrimaryColor(color))
        }
        "2c" => {
            let color_str = params?;
            let color = parse_ass_color_tag(color_str)?;
            Some(OverrideTag::SecondaryColor(color))
        }
        "3c" => {
            let color_str = params?;
            let color = parse_ass_color_tag(color_str)?;
            Some(OverrideTag::OutlineColor(color))
        }
        "4c" => {
            let color_str = params?;
            let color = parse_ass_color_tag(color_str)?;
            Some(OverrideTag::ShadowColor(color))
        }
        "alpha" => {
            let alpha_str = params?;
            let alpha = parse_ass_alpha(alpha_str)?;
            Some(OverrideTag::Alpha(alpha))
        }
        "1a" => {
            let alpha_str = params?;
            let alpha = parse_ass_alpha(alpha_str)?;
            Some(OverrideTag::PrimaryAlpha(alpha))
        }
        "2a" => {
            let alpha_str = params?;
            let alpha = parse_ass_alpha(alpha_str)?;
            Some(OverrideTag::SecondaryAlpha(alpha))
        }
        "3a" => {
            let alpha_str = params?;
            let alpha = parse_ass_alpha(alpha_str)?;
            Some(OverrideTag::OutlineAlpha(alpha))
        }
        "4a" => {
            let alpha_str = params?;
            let alpha = parse_ass_alpha(alpha_str)?;
            Some(OverrideTag::ShadowAlpha(alpha))
        }
        "pos" => {
            let params = params?;
            let parts: Vec<&str> = params.split(',').collect();
            if parts.len() >= 2 {
                let x = parts[0].parse().ok()?;
                let y = parts[1].parse().ok()?;
                Some(OverrideTag::Position(x, y))
            } else {
                None
            }
        }
        "move" => {
            let params = params?;
            let parts: Vec<&str> = params.split(',').collect();
            match parts.len() {
                4 => {
                    let x1 = parts[0].parse().ok()?;
                    let y1 = parts[1].parse().ok()?;
                    let x2 = parts[2].parse().ok()?;
                    let y2 = parts[3].parse().ok()?;
                    Some(OverrideTag::Move(x1, y1, x2, y2))
                }
                6 => {
                    let x1 = parts[0].parse().ok()?;
                    let y1 = parts[1].parse().ok()?;
                    let x2 = parts[2].parse().ok()?;
                    let y2 = parts[3].parse().ok()?;
                    let t1 = parts[4].parse().ok()?;
                    let t2 = parts[5].parse().ok()?;
                    Some(OverrideTag::MoveWithTiming(x1, y1, x2, y2, t1, t2))
                }
                _ => None,
            }
        }
        "org" => {
            let params = params?;
            let parts: Vec<&str> = params.split(',').collect();
            if parts.len() >= 2 {
                let x = parts[0].parse().ok()?;
                let y = parts[1].parse().ok()?;
                Some(OverrideTag::Origin(x, y))
            } else {
                None
            }
        }
        "an" | "a" => {
            let val = params?.parse::<i32>().ok()?;
            Some(OverrideTag::Alignment(val))
        }
        "frx" => {
            let val = params?.parse().ok()?;
            Some(OverrideTag::RotationX(val))
        }
        "fry" => {
            let val = params?.parse().ok()?;
            Some(OverrideTag::RotationY(val))
        }
        "frz" | "fr" => {
            let val = params?.parse().ok()?;
            Some(OverrideTag::RotationZ(val))
        }
        "fscx" => {
            let val = params?.parse().ok()?;
            Some(OverrideTag::ScaleX(val))
        }
        "fscy" => {
            let val = params?.parse().ok()?;
            Some(OverrideTag::ScaleY(val))
        }
        "fax" => {
            let val = params?.parse().ok()?;
            Some(OverrideTag::ShearX(val))
        }
        "fay" => {
            let val = params?.parse().ok()?;
            Some(OverrideTag::ShearY(val))
        }
        "bord" => {
            let val = params?.parse().ok()?;
            Some(OverrideTag::Border(val))
        }
        "xbord" => {
            let val = params?.parse().ok()?;
            Some(OverrideTag::BorderX(val))
        }
        "ybord" => {
            let val = params?.parse().ok()?;
            Some(OverrideTag::BorderY(val))
        }
        "shad" => {
            let val = params?.parse().ok()?;
            Some(OverrideTag::Shadow(val))
        }
        "xshad" => {
            let val = params?.parse().ok()?;
            Some(OverrideTag::ShadowX(val))
        }
        "yshad" => {
            let val = params?.parse().ok()?;
            Some(OverrideTag::ShadowY(val))
        }
        "be" => {
            let val = params?.parse().ok()?;
            Some(OverrideTag::EdgeBlur(val))
        }
        "blur" => {
            let val = params?.parse().ok()?;
            Some(OverrideTag::Blur(val))
        }
        "fad" => {
            let params = params?;
            let parts: Vec<&str> = params.split(',').collect();
            if parts.len() >= 2 {
                let fade_in = parts[0].parse().ok()?;
                let fade_out = parts[1].parse().ok()?;
                Some(OverrideTag::Fade(fade_in, fade_out))
            } else {
                None
            }
        }
        "t" => {
            // \t(t1, t2, [accel,] tags...)
            let params = params?;
            let parts: Vec<&str> = params.split(',').collect();
            if parts.len() < 3 {
                return None;
            }

            let t1: u64 = parts[0].trim().parse().ok()?;
            let t2: u64 = parts[1].trim().parse().ok()?;

            // Determine if accel is present
            let (accel, tags_start) = if parts.len() >= 4 {
                // Check if third part is a number (accel)
                if let Ok(accel) = parts[2].trim().parse::<f64>() {
                    (accel, 3)
                } else {
                    (1.0, 2)
                }
            } else {
                (1.0, 2)
            };

            // Join remaining parts as the tags string
            let tags_str: String = parts[tags_start..].join(",");

            // Parse the inner tags
            let tags = parse_tag_group(&tags_str);

            Some(OverrideTag::Transform {
                t1,
                t2,
                accel,
                tags,
            })
        }
        "clip" => {
            let params = params?;
            let parts: Vec<&str> = params.split(',').collect();
            if parts.len() >= 4 {
                let x1 = parts[0].parse().ok()?;
                let y1 = parts[1].parse().ok()?;
                let x2 = parts[2].parse().ok()?;
                let y2 = parts[3].parse().ok()?;
                Some(OverrideTag::Clip(x1, y1, x2, y2))
            } else {
                None
            }
        }
        "iclip" => {
            let params = params?;
            let parts: Vec<&str> = params.split(',').collect();
            if parts.len() >= 4 {
                let x1 = parts[0].parse().ok()?;
                let y1 = parts[1].parse().ok()?;
                let x2 = parts[2].parse().ok()?;
                let y2 = parts[3].parse().ok()?;
                Some(OverrideTag::InverseClip(x1, y1, x2, y2))
            } else {
                None
            }
        }
        "p" => {
            let val = params?.parse().ok()?;
            Some(OverrideTag::Drawing(val))
        }
        "k" => {
            let val = params?.parse().ok()?;
            Some(OverrideTag::KaraokeDuration(val))
        }
        "K" | "kf" => {
            let val = params?.parse().ok()?;
            Some(OverrideTag::KaraokeSweep(val))
        }
        "ko" => {
            let val = params?.parse().ok()?;
            Some(OverrideTag::KaraokeOutline(val))
        }
        _ => {
            // Reconstruct tag string for unknown tags
            let tag_str = match params {
                Some(p) => format!("{}{}", name, p),
                None => name.to_string(),
            };
            Some(OverrideTag::Unknown(tag_str))
        }
    }
}

fn parse_ass_color_tag(s: &str) -> Option<Color> {
    let s = s.trim().trim_start_matches('&').trim_end_matches('&');
    let s = s
        .strip_prefix('H')
        .or_else(|| s.strip_prefix('h'))
        .unwrap_or(s);

    match s.len() {
        6 => {
            let blue = u8::from_str_radix(&s[0..2], 16).ok()?;
            let green = u8::from_str_radix(&s[2..4], 16).ok()?;
            let red = u8::from_str_radix(&s[4..6], 16).ok()?;
            Some(Color::new(0, red, green, blue))
        }
        8 => {
            let alpha = u8::from_str_radix(&s[0..2], 16).ok()?;
            let blue = u8::from_str_radix(&s[2..4], 16).ok()?;
            let green = u8::from_str_radix(&s[4..6], 16).ok()?;
            let red = u8::from_str_radix(&s[6..8], 16).ok()?;
            Some(Color::new(alpha, red, green, blue))
        }
        _ => None,
    }
}

fn parse_ass_alpha(s: &str) -> Option<u8> {
    let s = s.trim().trim_start_matches('&').trim_end_matches('&');
    let s = s
        .strip_prefix('H')
        .or_else(|| s.strip_prefix('h'))
        .unwrap_or(s);

    match s.len() {
        2 => u8::from_str_radix(s, 16).ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_position_tag() {
        let tags = OverrideTag::parse_from_text("{\\pos(100,200)}Hello");
        assert_eq!(tags.len(), 1);
        assert!(matches!(tags[0], OverrideTag::Position(100.0, 200.0)));
    }

    #[test]
    fn test_parse_color_tag() {
        let tags = OverrideTag::parse_from_text("{\\c&H00FF00&}Green text");
        assert_eq!(tags.len(), 1);
        assert!(tags[0].is_color());
    }

    #[test]
    fn test_parse_multiple_tags() {
        let tags = OverrideTag::parse_from_text("{\\b1\\i1\\c&H0000FF&}Bold Italic Blue");
        assert!(tags.len() >= 3);
    }

    #[test]
    fn test_parse_bold_tag() {
        let tags = OverrideTag::parse_from_text("{\\b1}Bold");
        assert_eq!(tags.len(), 1);
        assert!(matches!(tags[0], OverrideTag::Bold(true)));
    }

    #[test]
    fn test_parse_fade_tag() {
        let tags = OverrideTag::parse_from_text("{\\fad(500,300)}Fade");
        assert_eq!(tags.len(), 1);
        assert!(matches!(tags[0], OverrideTag::Fade(500, 300)));
    }

    #[test]
    fn test_parse_move_tag() {
        let tags = OverrideTag::parse_from_text("{\\move(100,200,300,400)}Move");
        assert_eq!(tags.len(), 1);
        assert!(matches!(
            tags[0],
            OverrideTag::Move(100.0, 200.0, 300.0, 400.0)
        ));
    }

    #[test]
    fn test_no_tags() {
        let tags = OverrideTag::parse_from_text("Hello World");
        assert!(tags.is_empty());
    }

    #[test]
    fn test_is_positioning() {
        let tag = OverrideTag::Position(100.0, 200.0);
        assert!(tag.is_positioning());

        let tag = OverrideTag::Bold(true);
        assert!(!tag.is_positioning());
    }

    #[test]
    fn test_parse_transform_tag() {
        let tags = OverrideTag::parse_from_text("{\\t(100,500,\\blur20\\4c&H00BBB0&)}");
        assert_eq!(tags.len(), 1);
        match &tags[0] {
            OverrideTag::Transform {
                t1,
                t2,
                accel,
                tags: inner,
            } => {
                assert_eq!(*t1, 100);
                assert_eq!(*t2, 500);
                assert_eq!(*accel, 1.0);
                assert_eq!(inner.len(), 2);
            }
            _ => panic!("Expected Transform tag"),
        }
    }

    #[test]
    fn test_parse_transform_with_accel() {
        let tags = OverrideTag::parse_from_text("{\\t(100,500,2.0,\\blur20)}");
        assert_eq!(tags.len(), 1);
        match &tags[0] {
            OverrideTag::Transform { t1, t2, accel, .. } => {
                assert_eq!(*t1, 100);
                assert_eq!(*t2, 500);
                assert_eq!(*accel, 2.0);
            }
            _ => panic!("Expected Transform tag"),
        }
    }

    #[test]
    fn test_parse_blur_tag() {
        let tags = OverrideTag::parse_from_text("{\\blur20}");
        assert_eq!(tags.len(), 1);
        assert!(matches!(tags[0], OverrideTag::Blur(20.0)));
    }

    #[test]
    fn test_parse_4c_color_tag() {
        let tags = OverrideTag::parse_from_text("{\\4c&H00BBB0&}");
        assert_eq!(tags.len(), 1);
        match &tags[0] {
            OverrideTag::ShadowColor(c) => {
                // ASS format is &HBBGGRR& → blue=0x00, green=0xBB, red=0xB0
                assert_eq!(c.blue, 0x00);
                assert_eq!(c.green, 0xBB);
                assert_eq!(c.red, 0xB0);
            }
            _ => panic!("Expected ShadowColor tag"),
        }
    }

    #[test]
    fn test_parse_text_segments_simple() {
        let segments = parse_text_segments("Hello World");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "Hello World");
        assert!(segments[0].tags.is_empty());
    }

    #[test]
    fn test_parse_text_segments_inline_color() {
        let segments = parse_text_segments("Ti{\\c&H4CE5FF&}ger");
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].text, "Ti");
        assert!(segments[0].tags.is_empty());
        assert_eq!(segments[1].text, "ger");
        assert_eq!(segments[1].tags.len(), 1);
    }

    #[test]
    fn test_parse_text_segments_multiple_colors() {
        let segments = parse_text_segments("A{\\c&H0000FF&}B{\\c&H00FF00&}C");
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].text, "A");
        assert_eq!(segments[1].text, "B");
        assert_eq!(segments[2].text, "C");
    }

    #[test]
    fn test_parse_text_segments_with_line_break() {
        let segments = parse_text_segments("Line1{\\c&H00FF00&}\\NLine2");
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].text, "Line1\n");
        assert_eq!(segments[1].text, "Line2");
    }
}
