use crate::types::color::Color;

use super::OverrideTag;

pub(super) fn parse_ass_color_tag(s: &str) -> Option<Color> {
    let s = s.trim().trim_start_matches('&').trim_end_matches('&');
    let s = s
        .strip_prefix('H')
        .or_else(|| s.strip_prefix('h'))
        .unwrap_or(s);
    // Byte-length checks below only imply char boundaries for ASCII;
    // multibyte input must fail, never panic on slicing.
    if !s.is_ascii() {
        return None;
    }

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

pub(super) fn parse_ass_alpha(s: &str) -> Option<u8> {
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

/// Parse primary/secondary/outline/shadow colors and alpha overrides.
pub(super) fn parse(name: &str, params: Option<&str>) -> Option<OverrideTag> {
    match name {
        "c" | "1c" => Some(OverrideTag::PrimaryColor(parse_ass_color_tag(params?)?)),
        "2c" => Some(OverrideTag::SecondaryColor(parse_ass_color_tag(params?)?)),
        "3c" => Some(OverrideTag::OutlineColor(parse_ass_color_tag(params?)?)),
        "4c" => Some(OverrideTag::ShadowColor(parse_ass_color_tag(params?)?)),
        "alpha" => Some(OverrideTag::Alpha(parse_ass_alpha(params?)?)),
        "1a" => Some(OverrideTag::PrimaryAlpha(parse_ass_alpha(params?)?)),
        "2a" => Some(OverrideTag::SecondaryAlpha(parse_ass_alpha(params?)?)),
        "3a" => Some(OverrideTag::OutlineAlpha(parse_ass_alpha(params?)?)),
        "4a" => Some(OverrideTag::ShadowAlpha(parse_ass_alpha(params?)?)),
        _ => None,
    }
}
