use serde::{Deserialize, Serialize};

use super::color::Color;

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

    // Animation
    Animation(Vec<AnimationCommand>),
    AnimationWithAccel(Vec<AnimationCommand>, f64),

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

/// Animation command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimationCommand {
    pub accel: f64,
    pub tags: Vec<OverrideTag>,
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
}

fn parse_tag_group(group: &str) -> Vec<OverrideTag> {
    let mut tags = Vec::new();
    let parts: Vec<&str> = group.split('\\').collect();

    for part in parts {
        if part.is_empty() {
            continue;
        }

        if let Some(tag) = parse_single_tag(part) {
            tags.push(tag);
        }
    }

    tags
}

fn parse_single_tag(tag: &str) -> Option<OverrideTag> {
    // Handle tags with parentheses: \pos(100,200), \fad(500,300), etc.
    if let Some(paren_pos) = tag.find('(') {
        let name = &tag[..paren_pos];
        let params = &tag[paren_pos + 1..tag.len() - 1];
        return parse_tag_with_params(name, Some(params));
    }

    // Handle tags without parentheses: \b1, \i1, \c&H00FF00&, \fnArial, etc.
    // Need to figure out where the tag name ends and value begins
    let (name, params) = split_tag_name_and_value(tag);
    parse_tag_with_params(name, params)
}

fn split_tag_name_and_value(tag: &str) -> (&str, Option<&str>) {
    // Single character tags followed by value
    if tag.len() >= 2 {
        let first_char = match tag.chars().next() {
            Some(c) => c,
            None => return (tag, None),
        };

        match first_char {
            'b' | 'i' | 'u' | 's' => {
                // Boolean tags: \b1, \i0, etc. - split at first non-alpha boundary
                let name = &tag[..1];
                let value = &tag[1..];
                return (name, Some(value));
            }
            _ => {}
        }
    }

    // Multi-character tags - find where letters end and value begins
    let name_end = tag.find(|c: char| !c.is_alphabetic()).unwrap_or(tag.len());

    if name_end == 0 {
        return (tag, None);
    }

    if name_end == tag.len() {
        return (tag, None);
    }

    (&tag[..name_end], Some(&tag[name_end..]))
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
            let val = params?.parse::<f64>().ok()?;
            Some(OverrideTag::FontSize(val))
        }
        "fsp" => {
            let val = params?.parse::<f64>().ok()?;
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
}
