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
    /// `\r` (reset to the event style) or `\rStyleName` (reset to a style).
    Reset(Option<String>),

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
    /// Vector clip: `\clip([scale,] drawing commands)`.
    ClipVector {
        scale: i32,
        drawing: String,
    },
    InverseClipVector {
        scale: i32,
        drawing: String,
    },

    // Drawing
    Drawing(i32),
    /// Drawing baseline offset `\pbo<n>`.
    DrawingBaseline(f64),

    // Wrapping
    WrapStyle(i32),

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

    /// Line-global tags: line properties even when they appear textually
    /// after the first visible segment (\pos, \move, \org, \clip, \iclip,
    /// \fad, \fade). Segment style tags are everything else.
    pub fn is_line_global(&self) -> bool {
        matches!(
            self,
            Self::Position(..)
                | Self::Move(..)
                | Self::MoveWithTiming(..)
                | Self::Origin(..)
                | Self::Clip(..)
                | Self::InverseClip(..)
                | Self::ClipVector { .. }
                | Self::InverseClipVector { .. }
                | Self::Fade(..)
                | Self::ComplexFade(..)
        )
    }

    /// True when every numeric payload is finite (layout-safe).
    fn all_finite(&self) -> bool {
        let f = |v: f64| v.is_finite();
        match self {
            Self::FontSize(v)
            | Self::FontSizeMultiplier(v)
            | Self::LetterSpacing(v)
            | Self::RotationX(v)
            | Self::RotationY(v)
            | Self::RotationZ(v)
            | Self::ScaleX(v)
            | Self::ScaleY(v)
            | Self::ShearX(v)
            | Self::ShearY(v)
            | Self::Border(v)
            | Self::BorderX(v)
            | Self::BorderY(v)
            | Self::Shadow(v)
            | Self::ShadowX(v)
            | Self::ShadowY(v)
            | Self::EdgeBlur(v)
            | Self::Blur(v)
            | Self::DrawingBaseline(v) => f(*v),
            Self::Position(x, y) | Self::Origin(x, y) => f(*x) && f(*y),
            Self::Move(x1, y1, x2, y2) => f(*x1) && f(*y1) && f(*x2) && f(*y2),
            Self::MoveWithTiming(x1, y1, x2, y2, ..) => f(*x1) && f(*y1) && f(*x2) && f(*y2),
            Self::Transform { accel, tags, .. } => f(*accel) && tags.iter().all(|t| t.all_finite()),
            _ => true,
        }
    }
}

/// Parse text into segments, each with its accumulated override tags.
/// Text like `Hello {\c&H00FF00&}World` produces:
/// - Segment("Hello ", [])
/// - Segment("World", [PrimaryColor(green)])
///
/// `\N` is a hard line break; `\n` is a soft break (a space) unless the
/// effective wrap style is 2, where it also breaks the line. Use
/// [`parse_text_segments_with_wrap`] when the wrap style is known.
pub fn parse_text_segments(text: &str) -> Vec<TextSegment> {
    parse_text_segments_with_wrap(text, 0)
}

/// [`parse_text_segments`] with an explicit effective wrap style.
pub fn parse_text_segments_with_wrap(text: &str, wrap_style: i32) -> Vec<TextSegment> {
    let soft_breaks = wrap_style == 2;
    let mut segments = Vec::new();
    let mut accumulated_tags: Vec<OverrideTag> = Vec::new();
    let mut current_text = String::new();
    let mut chars = text.chars().peekable();
    let mut in_drawing_mode = false;

    // Push a line break, attaching to the previous segment when the
    // current text is empty (right after a tag group).
    let push_break = |segments: &mut Vec<TextSegment>, current_text: &mut String, ch: char| {
        if current_text.is_empty() {
            if let Some(last) = segments.last_mut() {
                last.text.push(ch);
            }
        } else {
            current_text.push(ch);
        }
    };

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
                    // Track drawing mode: any \pN with N > 0 enables it.
                    if let OverrideTag::Drawing(n) = &tag {
                        in_drawing_mode = *n > 0;
                    }
                    accumulated_tags.push(tag);
                }
            }
            '\\' => {
                chars.next(); // consume '\\'
                if let Some(&next) = chars.peek() {
                    match next {
                        'N' => {
                            chars.next();
                            if !in_drawing_mode {
                                push_break(&mut segments, &mut current_text, '\n');
                            }
                        }
                        'n' => {
                            chars.next();
                            if !in_drawing_mode {
                                // Soft break: a space, unless wrap mode 2.
                                let ch = if soft_breaks { '\n' } else { ' ' };
                                push_break(&mut segments, &mut current_text, ch);
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
    } else if accumulated_tags.len() > segments.last().map(|s| s.tags.len()).unwrap_or(0) {
        // Trailing tag group with no following text: keep an empty carrier
        // segment so line-global tags (and resets) are not silently lost.
        segments.push(TextSegment {
            text: String::new(),
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

            // The reader above is greedy (`\rAltStyle` reads as one run),
            // so split a known tag prefix from a glued value: `\rAltStyle`
            // is tag `r` with value `AltStyle`, `\fnArial` is `fn`+`Arial`.
            let (tag_name, glued) = split_tag_name(&name);

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

                if glued.is_empty() {
                    match parse_tag_with_params(tag_name, Some(&param_str)) {
                        Some(tag) if tag.all_finite() => tags.push(tag),
                        _ => tags.push(OverrideTag::Unknown(format!("{}({})", name, param_str))),
                    }
                } else {
                    tags.push(OverrideTag::Unknown(format!("{}({})", name, param_str)));
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

                let combined = format!("{}{}", glued, value);
                let params = if combined.is_empty() {
                    None
                } else {
                    Some(combined.as_str())
                };
                match parse_tag_with_params(tag_name, params) {
                    Some(tag) if tag.all_finite() => tags.push(tag),
                    // Malformed known tag: preserved as Unknown so it is
                    // distinguishable from "no tag" and never half-applied.
                    _ => tags.push(OverrideTag::Unknown(format!("{}{}", name, value))),
                }
            }
        } else {
            chars.next();
        }
    }

    tags
}

/// Whether `name` is a recognized override tag name.
fn is_known_tag_name(name: &str) -> bool {
    matches!(
        name,
        "b" | "i"
            | "u"
            | "s"
            | "fn"
            | "fs"
            | "fsp"
            | "r"
            | "c"
            | "1c"
            | "2c"
            | "3c"
            | "4c"
            | "alpha"
            | "1a"
            | "2a"
            | "3a"
            | "4a"
            | "pos"
            | "move"
            | "org"
            | "an"
            | "a"
            | "frx"
            | "fry"
            | "frz"
            | "fr"
            | "fscx"
            | "fscy"
            | "fax"
            | "fay"
            | "bord"
            | "xbord"
            | "ybord"
            | "shad"
            | "xshad"
            | "yshad"
            | "be"
            | "blur"
            | "fad"
            | "fade"
            | "t"
            | "clip"
            | "iclip"
            | "p"
            | "pbo"
            | "q"
            | "k"
            | "K"
            | "kf"
            | "ko"
    )
}

/// Split a greedily-read run into (known tag name, glued value).
/// Exact and unknown names pass through; otherwise the longest known
/// prefix wins (`rAltStyle` -> `r` + `AltStyle`).
fn split_tag_name(raw: &str) -> (&str, &str) {
    if is_known_tag_name(raw) {
        return (raw, "");
    }
    for len in (1..raw.len()).rev() {
        if !raw.is_char_boundary(len) {
            continue;
        }
        let (head, tail) = raw.split_at(len);
        if is_known_tag_name(head) {
            return (head, tail);
        }
    }
    (raw, "")
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
        "fn" => Some(OverrideTag::FontName(params?.trim().to_string())),
        "fs" => {
            let val = params?.parse().ok()?;
            Some(OverrideTag::FontSize(val))
        }
        "fsp" => {
            let val = params?.parse().ok()?;
            Some(OverrideTag::LetterSpacing(val))
        }
        "r" => {
            let name = params
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            Some(OverrideTag::Reset(name))
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
        "an" => {
            let val = params?.parse::<i32>().ok()?;
            Some(OverrideTag::Alignment(val))
        }
        "a" => {
            // Legacy SSA alignment numbering, converted to ASS numpad.
            let val = params?.parse::<i32>().ok()?;
            Some(OverrideTag::Alignment(super::style::ssa_alignment_to_ass(
                val,
            )))
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
        "fade" => {
            // \fade(a1, a2, a3, t1, t2, t3, t4)
            let params = params?;
            let parts: Vec<&str> = params.split(',').collect();
            if parts.len() >= 7 {
                let a1 = parts[0].trim().parse().ok()?;
                let a2 = parts[1].trim().parse().ok()?;
                let a3 = parts[2].trim().parse().ok()?;
                let t1 = parts[3].trim().parse().ok()?;
                let t2 = parts[4].trim().parse().ok()?;
                let t3 = parts[5].trim().parse().ok()?;
                let t4 = parts[6].trim().parse().ok()?;
                Some(OverrideTag::ComplexFade(a1, a2, a3, t1, t2, t3, t4))
            } else {
                None
            }
        }
        "t" => {
            // \t([t1, t2,] [accel,] tags...)
            // Leading numeric arguments precede the first backslash; the
            // remainder is the inner tag string (which may itself contain
            // commas, e.g. \clip(0,0,100,100)).
            let params = params?;
            let tags_pos = params.find('\\').unwrap_or(params.len());
            let (args_part, tags_str) = params.split_at(tags_pos);
            let args: Vec<&str> = args_part
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect();

            // Per the ASS spec: 0 args = defaults, 1 = accel, 2 = t1/t2,
            // 3 = t1/t2/accel. t2 == 0 means "until the end of the event".
            let (t1, t2, accel) = match args.len() {
                0 => (0u64, 0u64, 1.0f64),
                1 => (0u64, 0u64, args[0].parse().ok()?),
                2 => (args[0].parse().ok()?, args[1].parse().ok()?, 1.0f64),
                _ => (
                    args[0].parse().ok()?,
                    args[1].parse().ok()?,
                    args[2].parse().ok()?,
                ),
            };

            let tags = parse_tag_group(tags_str);

            Some(OverrideTag::Transform {
                t1,
                t2,
                accel,
                tags,
            })
        }
        "clip" => parse_clip_params(params, false),
        "iclip" => parse_clip_params(params, true),
        "p" => {
            let val = params?.parse().ok()?;
            Some(OverrideTag::Drawing(val))
        }
        "pbo" => {
            let val = params?.parse().ok()?;
            Some(OverrideTag::DrawingBaseline(val))
        }
        "q" => {
            let val = params?.parse().ok()?;
            Some(OverrideTag::WrapStyle(val))
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

/// Parse `\clip` / `\iclip` params: rectangular `(x1,y1,x2,y2)` or
/// vector `(drawing)` / `(scale,drawing)` form.
fn parse_clip_params(params: Option<&str>, inverse: bool) -> Option<OverrideTag> {
    let params = params?;
    let parts: Vec<&str> = params.split(',').collect();
    // Rectangular form: exactly four integers.
    if parts.len() == 4 {
        if let (Ok(x1), Ok(y1), Ok(x2), Ok(y2)) = (
            parts[0].trim().parse::<i32>(),
            parts[1].trim().parse::<i32>(),
            parts[2].trim().parse::<i32>(),
            parts[3].trim().parse::<i32>(),
        ) {
            return Some(if inverse {
                OverrideTag::InverseClip(x1, y1, x2, y2)
            } else {
                OverrideTag::Clip(x1, y1, x2, y2)
            });
        }
    }
    // Vector form: [scale,] drawing commands. The drawing must contain
    // at least one drawing command letter, otherwise this is a malformed
    // rectangle (e.g. "1,2,3"), not a vector clip.
    let (scale, drawing) = match params.split_once(',') {
        Some((head, tail)) if !tail.trim().is_empty() => match head.trim().parse::<i32>() {
            Ok(s) => (s, tail.trim().to_string()),
            Err(_) => (1, params.trim().to_string()),
        },
        _ => (1, params.trim().to_string()),
    };
    if drawing.is_empty() || !drawing_contains_command(&drawing) {
        return None;
    }
    Some(if inverse {
        OverrideTag::InverseClipVector { scale, drawing }
    } else {
        OverrideTag::ClipVector { scale, drawing }
    })
}

fn drawing_contains_command(drawing: &str) -> bool {
    drawing.chars().any(|c| {
        matches!(
            c,
            'm' | 'n' | 'l' | 'b' | 's' | 'p' | 'c' | 'M' | 'N' | 'L' | 'B' | 'S' | 'P' | 'C'
        )
    })
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
    fn test_parse_karaoke_tags() {
        let tags = OverrideTag::parse_from_text("{\\k50}{\\K60}{\\kf70}{\\ko80}");
        assert_eq!(tags.len(), 4);
        assert!(matches!(tags[0], OverrideTag::KaraokeDuration(50)));
        assert!(matches!(tags[1], OverrideTag::KaraokeSweep(60)));
        assert!(matches!(tags[2], OverrideTag::KaraokeSweep(70)));
        assert!(matches!(tags[3], OverrideTag::KaraokeOutline(80)));
    }

    #[test]
    fn test_parse_complex_fade_tag() {
        let tags = OverrideTag::parse_from_text("{\\fade(255,0,255,0,500,2000,2200)}");
        assert_eq!(tags.len(), 1);
        assert!(matches!(
            tags[0],
            OverrideTag::ComplexFade(255, 0, 255, 0, 500, 2000, 2200)
        ));
    }

    #[test]
    fn test_parse_wrap_style_tag() {
        let tags = OverrideTag::parse_from_text("{\\q2}Text");
        assert_eq!(tags.len(), 1);
        assert!(matches!(tags[0], OverrideTag::WrapStyle(2)));
    }

    #[test]
    fn test_parse_reset_tag() {
        let tags = OverrideTag::parse_from_text("{\\r}");
        assert_eq!(tags.len(), 1);
        assert!(matches!(tags[0], OverrideTag::Reset(None)));
    }

    #[test]
    fn test_parse_reset_to_style_tag() {
        let tags = OverrideTag::parse_from_text("{\\rAltStyle}");
        assert_eq!(tags.len(), 1);
        assert!(matches!(tags[0], OverrideTag::Reset(Some(ref s)) if s == "AltStyle"));
    }

    #[test]
    fn test_glued_tag_values_split() {
        // Values glued to the tag name (no separator) still parse.
        let tags = OverrideTag::parse_from_text("{\\fnArial}");
        assert!(
            matches!(tags[0], OverrideTag::FontName(ref s) if s == "Arial"),
            "{:?}",
            tags[0]
        );
        let tags = OverrideTag::parse_from_text("{\\fn Arial}");
        assert!(
            matches!(tags[0], OverrideTag::FontName(ref s) if s == "Arial"),
            "{:?}",
            tags[0]
        );
        let tags = OverrideTag::parse_from_text("{\\fs24}");
        assert!(matches!(tags[0], OverrideTag::FontSize(s) if s == 24.0));
    }

    #[test]
    fn test_legacy_a_alignment_conversion() {
        // SSA 1-3 bottom, 5-7 top, 9-11 middle -> ASS numpad
        for (legacy, numpad) in [
            (1, 1),
            (2, 2),
            (3, 3),
            (5, 7),
            (6, 8),
            (7, 9),
            (9, 4),
            (10, 5),
            (11, 6),
        ] {
            let tags = OverrideTag::parse_from_text(&format!("{{\\a{}}}", legacy));
            assert_eq!(tags.len(), 1, "legacy {}", legacy);
            assert!(
                matches!(tags[0], OverrideTag::Alignment(a) if a == numpad),
                "legacy {} -> {:?}, want {}",
                legacy,
                tags[0],
                numpad
            );
        }
        // \an passes through untouched
        let tags = OverrideTag::parse_from_text("{\\an5}");
        assert!(matches!(tags[0], OverrideTag::Alignment(5)));
    }

    #[test]
    fn test_non_finite_numerics_become_unknown() {
        // NaN/inf must never reach layout as live tags
        for text in [
            "{\\pos(NaN,10)}",
            "{\\fsinf}",
            "{\\fscx(NaN)}",
            "{\\blur(-inf)}",
        ] {
            let tags = OverrideTag::parse_from_text(text);
            assert_eq!(tags.len(), 1, "{}", text);
            assert!(
                matches!(tags[0], OverrideTag::Unknown(_)),
                "{} -> {:?}",
                text,
                tags[0]
            );
        }
        // Finite values still parse
        let tags = OverrideTag::parse_from_text("{\\pos(1.5,-2.5)}");
        assert!(matches!(tags[0], OverrideTag::Position(1.5, -2.5)));
    }

    #[test]
    fn test_malformed_known_tag_preserved_as_unknown() {
        let tags = OverrideTag::parse_from_text("{\\pos(1)}");
        assert_eq!(tags.len(), 1);
        assert!(matches!(tags[0], OverrideTag::Unknown(_)));
    }

    #[test]
    fn test_vector_clip_forms() {
        let tags = OverrideTag::parse_from_text("{\\clip(m 0 0 l 10 0 l 10 10)}");
        assert!(
            matches!(tags[0], OverrideTag::ClipVector { scale: 1, .. }),
            "{:?}",
            tags[0]
        );
        let tags = OverrideTag::parse_from_text("{\\clip(2, m 0 0 l 10 10)}");
        assert!(
            matches!(tags[0], OverrideTag::ClipVector { scale: 2, .. }),
            "{:?}",
            tags[0]
        );
        let tags = OverrideTag::parse_from_text("{\\iclip(m 0 0 l 5 5)}");
        assert!(matches!(tags[0], OverrideTag::InverseClipVector { .. }));
        // Rectangular form still wins for four integers
        let tags = OverrideTag::parse_from_text("{\\clip(0, 0, 100, 100)}");
        assert!(matches!(tags[0], OverrideTag::Clip(0, 0, 100, 100)));
        // Three numbers are neither rect nor vector
        let tags = OverrideTag::parse_from_text("{\\clip(1,2,3)}");
        assert!(matches!(tags[0], OverrideTag::Unknown(_)));
    }

    #[test]
    fn test_pbo_tag() {
        let tags = OverrideTag::parse_from_text("{\\pbo10}");
        assert!(matches!(tags[0], OverrideTag::DrawingBaseline(_)));
    }

    #[test]
    fn test_soft_break_is_space_except_wrap2() {
        let segs = parse_text_segments("a\\nb");
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].text, "a b");
        let segs = parse_text_segments("a\\Nb");
        assert_eq!(segs[0].text, "a\nb");
        let segs = parse_text_segments_with_wrap("a\\nb", 2);
        assert_eq!(segs[0].text, "a\nb");
    }

    #[test]
    fn test_drawing_mode_any_positive_p() {
        let segs = parse_text_segments("{\\p2}m 0 0{\\p0}text");
        assert_eq!(segs.len(), 2);
        // First segment carries the active \p2 drawing state with its
        // coordinate text; the second returns to text mode.
        assert_eq!(segs[0].text, "m 0 0");
        assert!(segs[0]
            .tags
            .iter()
            .any(|t| matches!(t, OverrideTag::Drawing(2))));
        assert_eq!(segs[1].text, "text");
        assert!(segs[1]
            .tags
            .iter()
            .rfind(|t| matches!(t, OverrideTag::Drawing(_)))
            .is_some_and(|t| matches!(t, OverrideTag::Drawing(0))));
    }

    #[test]
    fn test_trailing_tag_group_kept_as_carrier() {
        // A trailing group with no following text must not be lost.
        let segs = parse_text_segments("Hi{\\pos(100,100)}");
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].text, "Hi");
        assert!(segs[0].tags.is_empty());
        assert_eq!(segs[1].text, "");
        assert!(segs[1]
            .tags
            .iter()
            .any(|t| matches!(t, OverrideTag::Position(100.0, 100.0))));
        // Group-only text: single carrier segment.
        let segs = parse_text_segments("{\\pos(1,2)}");
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].text, "");
        assert_eq!(segs[0].tags.len(), 1);
        // No trailing group: unchanged behavior.
        let segs = parse_text_segments("Hi");
        assert_eq!(segs.len(), 1);
    }

    #[test]
    fn test_line_global_classification() {
        assert!(OverrideTag::Position(0.0, 0.0).is_line_global());
        assert!(OverrideTag::Fade(0, 0).is_line_global());
        assert!(OverrideTag::Clip(0, 0, 1, 1).is_line_global());
        assert!(!OverrideTag::Bold(true).is_line_global());
        assert!(!OverrideTag::FontSize(10.0).is_line_global());
    }

    #[test]
    fn test_parse_transform_without_timing() {
        let tags = OverrideTag::parse_from_text("{\\t(\\blur20)}");
        assert_eq!(tags.len(), 1);
        match &tags[0] {
            OverrideTag::Transform {
                t1,
                t2,
                accel,
                tags: inner,
            } => {
                assert_eq!(*t1, 0);
                assert_eq!(*t2, 0);
                assert_eq!(*accel, 1.0);
                assert_eq!(inner.len(), 1);
            }
            _ => panic!("Expected Transform tag"),
        }
    }

    #[test]
    fn test_parse_transform_accel_only() {
        let tags = OverrideTag::parse_from_text("{\\t(2.0,\\blur20)}");
        assert_eq!(tags.len(), 1);
        match &tags[0] {
            OverrideTag::Transform { t1, t2, accel, .. } => {
                assert_eq!(*t1, 0);
                assert_eq!(*t2, 0);
                assert_eq!(*accel, 2.0);
            }
            _ => panic!("Expected Transform tag"),
        }
    }

    #[test]
    fn test_parse_transform_nested_comma_params() {
        let tags = OverrideTag::parse_from_text("{\\t(0,500,\\clip(0,0,100,100))}");
        assert_eq!(tags.len(), 1);
        match &tags[0] {
            OverrideTag::Transform {
                t1,
                t2,
                accel,
                tags: inner,
            } => {
                assert_eq!(*t1, 0);
                assert_eq!(*t2, 500);
                assert_eq!(*accel, 1.0);
                assert_eq!(inner.len(), 1);
                assert!(matches!(inner[0], OverrideTag::Clip(0, 0, 100, 100)));
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
