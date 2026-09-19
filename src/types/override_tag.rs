use serde::{Deserialize, Serialize};

use super::color::Color;

mod clip;
mod color;
mod drawing;
mod karaoke;
mod lexer;
mod model;
mod scalar;

use self::clip::parse_clip_params;
use self::color::{parse_ass_alpha, parse_ass_color_tag};
use self::drawing::drawing_contains_command;
use self::karaoke::parse_karaoke_param;
use self::lexer::{complex_tag_keyword, is_complex_tag_name, split_tag_args, split_tag_name};
pub use self::model::{ass_bold_weight, TextSegment};
use self::scalar::{libass_dtoi32, parse_libass_f64, parse_libass_i32, sanitize_coord};

/// Override tags in ASS text
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OverrideTag {
    // Text formatting
    /// `\b` as an ASS font weight: 400 = normal, 700 = bold.
    /// See [`ass_bold_weight`] for the `\b0` / `\b1` mapping.
    Bold(u16),
    Italic(bool),
    Underline(bool),
    StrikeOut(bool),
    FontName(String),
    /// Absolute `\fs` size (no leading sign), e.g. `\fs24`.
    /// A computed size `<= 0` resets to the event style size (libass).
    FontSize(f64),
    /// Relative `\fs+N` / `\fs-N` delta (signed number after `\fs`).
    /// libass scales the *current* size: `size * (1 + delta / 10)`,
    /// so `\fs+10` doubles and `\fs-5` halves the current size.
    /// Inside `\t`, progress `p` interpolates the factor:
    /// `size * (1 + p * delta / 10)`.
    FontSizeRelative(f64),
    /// Bare `\fs` (no argument): reset to the event style size (libass).
    FontSizeReset,
    LetterSpacing(f64),
    /// `\fe<id>` font encoding/charset override. Legacy byte-like runs are
    /// decoded before shaping; shaping input remains Unicode.
    FontEncoding(i32),
    /// Restore one property from the active style (bare or invalid value).
    PropertyReset(String),
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
    /// Timed `\move(x1,y1,x2,y2,t1,t2)`: times are `i32` like libass
    /// (`argtoi32`), already swapped so `t1 <= t2`. Negative times are
    /// meaningful there (`t1 <= 0 && t2 <= 0` animates the whole event).
    MoveWithTiming(f64, f64, f64, f64, i32, i32),
    Origin(f64, f64),
    Alignment(i32),
    /// Bare `\an` / `\a` (or a value libass rejects): reset to the
    /// event style alignment. Still consumes the first-wins alignment
    /// slot (libass `PARSED_A`), so later `\an`/`\a` tags are ignored.
    AlignmentReset,

    // Transformations
    RotationX(f64),
    RotationY(f64),
    RotationZ(f64),
    ScaleX(f64),
    ScaleY(f64),
    /// Bare `\fsc` (any glued value ignored): reset both scale axes to
    /// the event style, like libass `tag("fsc")`.
    ScaleReset,
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
    /// `\fad` / 2-arg `\fade`: (fade-in ms, fade-out ms) as `i32` like
    /// libass (`argtoi32`); negative values mean "no fade" on that side.
    Fade(i32, i32),
    /// 7-arg `\fade(a1,a2,a3,t1,t2,t3,t4)`, all `i32` like libass.
    /// Alpha is interpolated full-range and truncated exactly like
    /// `interpolate_alpha`; out-of-range results clamp at application.
    ComplexFade(i32, i32, i32, i32, i32, i32, i32),

    // Animation: \t(t1, t2, [accel,] tags...)
    Transform {
        t1: i32,
        t2: i32,
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
    /// `\kt<cs>`: explicit absolute start (centiseconds from line start)
    /// for the next karaoke syllable. Sets the timeline clock without
    /// starting a syllable; syllables still need `\k`-family durations.
    KaraokeStart(u64),
    /// Signed millisecond timing, including fractional centiseconds.
    KaraokeTiming {
        mode: u8,
        millis: i32,
    },

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

    /// Line-global tags preserved across `\r`: the non-style line
    /// properties (\pos, \move, \org, \clip, \iclip, \fad, \fade) plus
    /// `\an`/`\a`: libass `ass_reset_render_context` (the `\r` handler)
    /// does not touch alignment, so the first alignment tag survives
    /// resets like the other line-global properties. `\r` restores
    /// ordinary override state (fonts, colors, border, rotation, ...)
    /// to the target style; karaoke timing and drawing mode also
    /// survive (the reset touches neither), but stay non-line-global
    /// because they apply positionally, not to the whole line.
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
                | Self::Alignment(..)
                | Self::AlignmentReset
        )
    }

    /// Event-layout tags: line-global tags plus `\q`, which position,
    /// fade, clip, align, and wrap the whole line no matter where they
    /// appear textually (`Hello{\an7}` aligns the entire line).
    /// Resolution is first-wins per libass (`EVENT_POSITIONED`,
    /// `PARSED_FADE`, `PARSED_A`, first vector clip), except `\q`,
    /// which keeps last-wins like libass's plain assignment.
    pub fn is_event_layout(&self) -> bool {
        self.is_line_global() || matches!(self, Self::WrapStyle(..))
    }

    /// True when every numeric payload is finite (layout-safe).
    fn all_finite(&self) -> bool {
        let f = |v: f64| v.is_finite();
        match self {
            Self::FontSize(v)
            | Self::FontSizeRelative(v)
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
                    // Track drawing mode: any \pN with N > 0 enables it,
                    // `\p0` disables it. `\r` is transparent: libass
                    // `ass_reset_render_context` never touches
                    // `drawing_scale`, so a drawing continues across a
                    // reset (`{\p1}...{\r}...` stays one drawing run).
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
/// Maximum `\t` nesting depth. Each level consumes input, so hostile
/// input could otherwise recurse to a stack overflow; real files never
/// nest transforms, and anything past this degrades to Unknown.
const MAX_TAG_NESTING: u32 = 32;

fn parse_tag_group(group: &str) -> Vec<OverrideTag> {
    parse_tag_group_depth(group, 0)
}

fn parse_tag_group_depth(group: &str, depth: u32) -> Vec<OverrideTag> {
    // libass trims trailing spaces before `}`/`)` at the call level
    // (the `end` argument), so bare tags stay bare.
    let group = group.trim_end_matches([' ', '\t']);
    let mut tags = Vec::new();
    let mut chars = group.chars().peekable();

    while let Some(&c) = chars.peek() {
        if c == '\\' {
            chars.next(); // consume '\\'
                          // libass `skip_spaces` after the backslash: `\ pos(1,2)` works.
            while matches!(chars.peek(), Some(' ' | '\t')) {
                chars.next();
            }

            // Read tag name: alphanumerics. libass scans to `(`,
            // `\` or end, so digits belong to the name region too
            // (`\pos2(1,2)` still applies `\pos`); `split_tag_name`
            // re-splits known prefixes identically either way.
            let mut name = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_alphanumeric() {
                    name.push(c);
                    chars.next();
                } else {
                    break;
                }
            }

            if name.is_empty() {
                continue;
            }

            // The reader above is greedy (`\rAltStyle` reads as one run),
            // so split a known tag prefix from a glued value: `\rAltStyle`
            // is tag `r` with value `AltStyle`, `\fnArial` is `fn`+`Arial`.
            let (tag_name, glued) = split_tag_name(&name);

            // libass skips spaces between the name and `(`: `\pos (1,2)`
            // applies. Harmless for plain values (parsers trim anyway).
            while matches!(chars.peek(), Some(' ' | '\t')) {
                chars.next();
            }

            // libass terminates at the FIRST closing parenthesis, including
            // inside transforms; nested transforms consume this argument's tail.
            if let Some(&'(') = chars.peek() {
                chars.next();
                let mut param_str = String::new();
                for c in chars.by_ref() {
                    if c == ')' {
                        break;
                    }
                    param_str.push(c);
                }

                // libass `complex_tag` is a PREFIX match that ignores
                // everything between the keyword and `(`: `\position(1,2)`
                // applies `\pos`. No keyword prefixes another and no longer
                // known tag extends them, so this cannot misroute.
                let (tag_name, glued) = match complex_tag_keyword(&name) {
                    Some(keyword) => (keyword, ""),
                    None => (tag_name, glued),
                };
                if glued.is_empty() {
                    match parse_tag_with_params(tag_name, Some(&param_str), depth) {
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

                // libass complex tags take arguments ONLY from parens
                // (`nargs` counts pushed paren args, never the glued
                // remainder): without parens they are ignored even with
                // glued text (`\pos10,20` does nothing there).
                if is_complex_tag_name(tag_name) {
                    tags.push(OverrideTag::Unknown(format!("{}{}", name, value)));
                    continue;
                }
                let combined = format!("{}{}", glued, value);
                let params = if combined.is_empty() {
                    None
                } else {
                    Some(combined.as_str())
                };
                match parse_tag_with_params(tag_name, params, depth) {
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

fn parse_tag_with_params(name: &str, params: Option<&str>, depth: u32) -> Option<OverrideTag> {
    let params = params.filter(|p| !p.trim_matches([' ', '\t']).is_empty());
    if let Some(raw) = params {
        let lower = raw.to_ascii_lowercase();
        let simple_numeric = matches!(
            name,
            "fs" | "fsp"
                | "fr"
                | "frx"
                | "fry"
                | "frz"
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
        );
        if simple_numeric && (lower.contains("nan") || lower.contains("inf")) {
            return None;
        }
    }
    if params.is_none()
        && matches!(
            name,
            "b" | "i"
                | "u"
                | "s"
                | "fn"
                | "fe"
                | "fsp"
                | "fr"
                | "frx"
                | "fry"
                | "frz"
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
        )
    {
        return Some(OverrideTag::PropertyReset(name.to_string()));
    }
    match name {
        "b" => {
            let val = parse_libass_i32(params?);
            if !(val == 0 || val == 1 || val >= 100) {
                return Some(OverrideTag::PropertyReset(name.to_string()));
            }
            Some(OverrideTag::Bold(ass_bold_weight(val)))
        }
        "i" => {
            let val = parse_libass_i32(params?);
            if !(0..=1).contains(&val) {
                return Some(OverrideTag::PropertyReset(name.to_string()));
            }
            Some(OverrideTag::Italic(val != 0))
        }
        "u" => {
            let val = parse_libass_i32(params?);
            if !(0..=1).contains(&val) {
                return Some(OverrideTag::PropertyReset(name.to_string()));
            }
            Some(OverrideTag::Underline(val != 0))
        }
        "s" => {
            let val = parse_libass_i32(params?);
            if !(0..=1).contains(&val) {
                return Some(OverrideTag::PropertyReset(name.to_string()));
            }
            Some(OverrideTag::StrikeOut(val != 0))
        }
        "fn" => Some(OverrideTag::FontName(params?.trim().to_string())),
        "fs" => {
            // libass `ass_parse.c`: a leading `+`/`-` makes the size
            // relative to the current size; otherwise it is absolute.
            // Bare `\fs` resets to the event style size.
            let raw = params.unwrap_or("").trim();
            if raw.is_empty() {
                return Some(OverrideTag::FontSizeReset);
            }
            if !raw.chars().any(|c| c.is_ascii_digit()) {
                return None;
            }
            let val = parse_libass_f64(raw);
            if raw.starts_with('+') || raw.starts_with('-') {
                Some(OverrideTag::FontSizeRelative(val))
            } else {
                Some(OverrideTag::FontSize(val))
            }
        }
        "fe" => {
            if !params?.chars().any(|c| c.is_ascii_digit()) {
                return None;
            }
            let val = parse_libass_i32(params?);
            Some(OverrideTag::FontEncoding(val))
        }
        "fsp" => {
            let val = parse_libass_f64(params.unwrap_or(""));
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
            // Exactly 2 arguments (libass `nargs == 2`); extra or
            // missing arguments ignore the whole tag. Values are
            // prefix-parsed and never fail (`\pos(x,20)` is (0,20)).
            let parts = split_tag_args(params?);
            if parts.len() == 2 {
                Some(OverrideTag::Position(
                    sanitize_coord(parse_libass_f64(parts[0])),
                    sanitize_coord(parse_libass_f64(parts[1])),
                ))
            } else {
                None
            }
        }
        "move" => {
            // Exactly 4 (untimed) or 6 (timed) arguments; anything
            // else is ignored. Coordinates are prefix-parsed, times
            // are `i32` (`argtoi32`), and reversed times swap (libass
            // parses `t1 > t2` swapped), so resolution never sees them.
            let parts = split_tag_args(params?);
            match parts.len() {
                4 => Some(OverrideTag::Move(
                    sanitize_coord(parse_libass_f64(parts[0])),
                    sanitize_coord(parse_libass_f64(parts[1])),
                    sanitize_coord(parse_libass_f64(parts[2])),
                    sanitize_coord(parse_libass_f64(parts[3])),
                )),
                6 => {
                    let t1 = parse_libass_i32(parts[4]);
                    let t2 = parse_libass_i32(parts[5]);
                    let (t1, t2) = if t1 > t2 { (t2, t1) } else { (t1, t2) };
                    Some(OverrideTag::MoveWithTiming(
                        sanitize_coord(parse_libass_f64(parts[0])),
                        sanitize_coord(parse_libass_f64(parts[1])),
                        sanitize_coord(parse_libass_f64(parts[2])),
                        sanitize_coord(parse_libass_f64(parts[3])),
                        t1,
                        t2,
                    ))
                }
                _ => None,
            }
        }
        "org" => {
            // Exactly 2 arguments, like `\pos`.
            let parts = split_tag_args(params?);
            if parts.len() == 2 {
                Some(OverrideTag::Origin(
                    sanitize_coord(parse_libass_f64(parts[0])),
                    sanitize_coord(parse_libass_f64(parts[1])),
                ))
            } else {
                None
            }
        }
        "an" => {
            // libass `tag("an")` + `argtoi32`: the value is a digit
            // prefix (`\an7x` is 7), garbage means 0, and every form —
            // bare, garbage, out-of-range — consumes PARSED_A with a
            // style fallback for anything outside 1-9.
            let raw = params.unwrap_or("");
            if raw.trim().is_empty() {
                return Some(OverrideTag::AlignmentReset);
            }
            let val = parse_libass_i32(raw);
            if !(1..=9).contains(&val) {
                return Some(OverrideTag::AlignmentReset);
            }
            Some(OverrideTag::Alignment(val))
        }
        "a" => {
            // Legacy SSA alignment numbering, converted to ASS numpad.
            // libass `tag("a")` after `tag("an")`: same prefix-number
            // parsing; values outside 1-11 fall back to the style
            // (slot still consumed), and the VSFilter quirk maps
            // illegal \a4 / \a8 to \a5.
            let raw = params.unwrap_or("");
            if raw.trim().is_empty() {
                return Some(OverrideTag::AlignmentReset);
            }
            let val = parse_libass_i32(raw);
            if !(1..=11).contains(&val) {
                return Some(OverrideTag::AlignmentReset);
            }
            let val = if val == 4 || val == 8 { 5 } else { val };
            Some(OverrideTag::Alignment(super::style::ssa_alignment_to_ass(
                val,
            )))
        }
        "frx" => {
            let val = parse_libass_f64(params.unwrap_or(""));
            Some(OverrideTag::RotationX(val))
        }
        "fry" => {
            let val = parse_libass_f64(params.unwrap_or(""));
            Some(OverrideTag::RotationY(val))
        }
        "frz" | "fr" => {
            let val = parse_libass_f64(params.unwrap_or(""));
            Some(OverrideTag::RotationZ(val))
        }
        "fscx" => {
            let val = parse_libass_f64(params.unwrap_or(""));
            Some(OverrideTag::ScaleX(val))
        }
        "fscy" => {
            let val = parse_libass_f64(params.unwrap_or(""));
            Some(OverrideTag::ScaleY(val))
        }
        // libass `tag("fsc")`: resets both scale axes to the style and
        // ignores any value.
        "fsc" => Some(OverrideTag::ScaleReset),
        "fax" => {
            let val = parse_libass_f64(params.unwrap_or(""));
            Some(OverrideTag::ShearX(val))
        }
        "fay" => {
            let val = parse_libass_f64(params.unwrap_or(""));
            Some(OverrideTag::ShearY(val))
        }
        "bord" => {
            let val = parse_libass_f64(params.unwrap_or(""));
            Some(OverrideTag::Border(val))
        }
        "xbord" => {
            let val = parse_libass_f64(params.unwrap_or(""));
            Some(OverrideTag::BorderX(val))
        }
        "ybord" => {
            let val = parse_libass_f64(params.unwrap_or(""));
            Some(OverrideTag::BorderY(val))
        }
        "shad" => {
            let val = parse_libass_f64(params.unwrap_or(""));
            Some(OverrideTag::Shadow(val))
        }
        "xshad" => {
            let val = parse_libass_f64(params.unwrap_or(""));
            Some(OverrideTag::ShadowX(val))
        }
        "yshad" => {
            let val = parse_libass_f64(params.unwrap_or(""));
            Some(OverrideTag::ShadowY(val))
        }
        "be" => {
            let val = parse_libass_f64(params.unwrap_or(""));
            Some(OverrideTag::EdgeBlur(val))
        }
        "blur" => {
            let val = parse_libass_f64(params.unwrap_or(""));
            Some(OverrideTag::Blur(val))
        }
        // libass parses both names identically (`complex_tag("fade") ||
        // complex_tag("fad")`): argument COUNT selects the behavior, not
        // the name — 2 args is a simple fade, 7 is a complex fade, and
        // anything else ignores the tag. So `\fade(100,200)` fades and
        // `\fad(3-arg)` does not. Values are prefix-parsed `i32` and
        // never fail (`\fad(x,200)` fades out only).
        "fad" | "fade" => {
            let parts = split_tag_args(params?);
            match parts.len() {
                2 => Some(OverrideTag::Fade(
                    parse_libass_i32(parts[0]),
                    parse_libass_i32(parts[1]),
                )),
                7 => Some(OverrideTag::ComplexFade(
                    parse_libass_i32(parts[0]),
                    parse_libass_i32(parts[1]),
                    parse_libass_i32(parts[2]),
                    parse_libass_i32(parts[3]),
                    parse_libass_i32(parts[4]),
                    parse_libass_i32(parts[5]),
                    parse_libass_i32(parts[6]),
                )),
                _ => None,
            }
        }
        "t" => {
            // libass `complex_tag("t")`: parenthesized only (the group
            // parser rejects bare `\t`), and the last argument swallows
            // from the first backslash to `)`, so timing args precede it.
            // `cnt` = nargs - 1 selects the timing form; without inner
            // tags, or with more than 3 timing args, it is ignored.
            // Past the nesting cap the whole tag degrades to Unknown
            // instead of recursing deeper (stack safety).
            if depth >= MAX_TAG_NESTING {
                return None;
            }
            let params = params?;
            let tags_pos = params.find('\\').unwrap_or(params.len());
            let (args_part, tags_str) = params.split_at(tags_pos);
            if !tags_str.contains('\\') {
                return None;
            }
            let mut args: Vec<&str> = args_part
                .split(',')
                .map(|s| s.trim_matches([' ', '\t']))
                .filter(|s| !s.is_empty())
                .collect();
            // A non-comma-terminated tail before the backslash belongs to
            // the swallowed last argument, not to timing (`\t(1,2,x\y)`
            // times (1,2), like libass's backslash swallow).
            if !args_part.trim_matches([' ', '\t']).ends_with(',') {
                args.pop();
            }

            // VSFilter-compatible per-count parsing: the 2-timing-arg
            // form parses floats (`dtoi32`), the 3-arg form parses ints
            // (`argtoi32`), so `\t(1e3,2000,\fs3)` starts at 1000 while
            // `\t(1e3,2000,1,\fs3)` starts at 1. `t2 == 0` means "until
            // the end of the event" (resolved at evaluation).
            let (t1, t2, accel) = match args.len() {
                0 => (0, 0, 1.0),
                1 => (0, 0, parse_libass_f64(args[0])),
                2 => (
                    libass_dtoi32(parse_libass_f64(args[0])),
                    libass_dtoi32(parse_libass_f64(args[1])),
                    1.0,
                ),
                3 => (
                    parse_libass_i32(args[0]),
                    parse_libass_i32(args[1]),
                    parse_libass_f64(args[2]),
                ),
                _ => return None,
            };

            let tags = parse_tag_group_depth(tags_str, depth + 1);

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
            let val = parse_libass_i32(params.unwrap_or("")).max(0);
            Some(OverrideTag::Drawing(val))
        }
        "pbo" => {
            let val = parse_libass_f64(params.unwrap_or(""));
            Some(OverrideTag::DrawingBaseline(val))
        }
        "q" => {
            let val = params.map(parse_libass_i32).unwrap_or(-1);
            Some(OverrideTag::WrapStyle(val))
        }
        // Bare karaoke tags carry libass defaults (`\k`/`\K`/`\kf`/`\ko`
        // default to 100cs, bare `\kt` to 0); empty parens count as bare.
        "k" => parse_karaoke_param(params, 0),
        "K" | "kf" => parse_karaoke_param(params, 1),
        "ko" => parse_karaoke_param(params, 2),
        "kt" => parse_karaoke_param(params, 3),
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
        assert!(matches!(tags[0], OverrideTag::Bold(700)));
    }

    #[test]
    fn test_bold_weight_mapping() {
        // \b0 normal, \b1 bold, explicit numeric weights preserved.
        for (text, want) in [
            ("{\\b0}x", 400),
            ("{\\b1}x", 700),
            ("{\\b100}x", 100),
            ("{\\b400}x", 400),
            ("{\\b700}x", 700),
            ("{\\b900}x", 900),
        ] {
            let tags = OverrideTag::parse_from_text(text);
            assert!(
                matches!(tags[0], OverrideTag::Bold(w) if w == want),
                "{text:?} -> {tags:?}, want Bold({want})"
            );
        }
        assert_eq!(ass_bold_weight(-1), 700);
        assert_eq!(ass_bold_weight(5000), 1000);
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

        let tag = OverrideTag::Bold(700);
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
    fn test_parse_kt_tag() {
        let tags = OverrideTag::parse_from_text("{\\kt120}");
        assert_eq!(tags.len(), 1);
        assert!(matches!(tags[0], OverrideTag::KaraokeStart(120)));
        // The k-splitter must not eat `kt`: mixed group in order.
        let tags = OverrideTag::parse_from_text("{\\k50\\kt120\\kf30}");
        assert_eq!(tags.len(), 3);
        assert!(matches!(tags[0], OverrideTag::KaraokeDuration(50)));
        assert!(matches!(tags[1], OverrideTag::KaraokeStart(120)));
        assert!(matches!(tags[2], OverrideTag::KaraokeSweep(30)));
    }

    #[test]
    fn test_bare_karaoke_tags_take_libass_defaults() {
        // libass: bare \k/\K/\kf/\ko default to 100cs, bare \kt to 0.
        let tags = OverrideTag::parse_from_text("{\\k}x");
        assert!(matches!(tags[0], OverrideTag::KaraokeDuration(100)));
        let tags = OverrideTag::parse_from_text("{\\K}x");
        assert!(matches!(tags[0], OverrideTag::KaraokeSweep(100)));
        let tags = OverrideTag::parse_from_text("{\\kf}x");
        assert!(matches!(tags[0], OverrideTag::KaraokeSweep(100)));
        let tags = OverrideTag::parse_from_text("{\\ko}x");
        assert!(matches!(tags[0], OverrideTag::KaraokeOutline(100)));
        let tags = OverrideTag::parse_from_text("{\\k()}x");
        assert!(matches!(tags[0], OverrideTag::KaraokeDuration(100)));
        let tags = OverrideTag::parse_from_text("{\\kt}x");
        assert!(matches!(tags[0], OverrideTag::KaraokeStart(0)));
        // Signed numeric prefixes are retained like libass.
        let tags = OverrideTag::parse_from_text("{\\k-5}x");
        assert!(matches!(
            tags[0],
            OverrideTag::KaraokeTiming {
                mode: 0,
                millis: -50
            }
        ));
        let tags = OverrideTag::parse_from_text("{\\kfoo}x");
        assert!(matches!(tags[0], OverrideTag::Unknown(_)));
    }

    #[test]
    fn test_parse_fe_tag() {
        let tags = OverrideTag::parse_from_text("{\\fe128}Text");
        assert_eq!(tags.len(), 1);
        assert!(matches!(tags[0], OverrideTag::FontEncoding(128)));
        let tags = OverrideTag::parse_from_text("{\\fe1}");
        assert!(matches!(tags[0], OverrideTag::FontEncoding(1)));
        // A bare encoding tag resets to the style encoding.
        let tags = OverrideTag::parse_from_text("{\\fe}");
        assert!(matches!(tags[0], OverrideTag::PropertyReset(ref name) if name == "fe"));
    }

    #[test]
    fn test_fs_relative_parsing_matches_libass() {
        // libass: a leading +/- makes \fs relative to the current size.
        let tags = OverrideTag::parse_from_text("{\\fs+10}x");
        assert!(matches!(tags[0], OverrideTag::FontSizeRelative(d) if d == 10.0));
        let tags = OverrideTag::parse_from_text("{\\fs-5}x");
        assert!(matches!(tags[0], OverrideTag::FontSizeRelative(d) if d == -5.0));
        // No sign: absolute.
        let tags = OverrideTag::parse_from_text("{\\fs24}x");
        assert!(matches!(tags[0], OverrideTag::FontSize(s) if s == 24.0));
        // Bare \fs resets to the event style size.
        let tags = OverrideTag::parse_from_text("{\\fs}x");
        assert!(matches!(tags[0], OverrideTag::FontSizeReset));
        // Parenthesized and signed forms inside \t parse identically.
        let tags = OverrideTag::parse_from_text("{\\fs(+4)}x");
        assert!(matches!(tags[0], OverrideTag::FontSizeRelative(d) if d == 4.0));
        let tags = OverrideTag::parse_from_text("{\\t(0,1000,\\fs+10)}x");
        match &tags[0] {
            OverrideTag::Transform { tags: inner, .. } => {
                assert!(matches!(inner[0], OverrideTag::FontSizeRelative(d) if d == 10.0));
            }
            other => panic!("expected Transform, got {other:?}"),
        }
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
    fn test_non_finite_numerics_never_reach_layout() {
        // libass `ass_strtod` has no inf/NaN literals: those parse as
        // 0.0 and the tag still applies (`\pos(NaN,10)` is (0,10)).
        for (text, want) in [
            ("{\\pos(NaN,10)}", (0.0, 10.0)),
            ("{\\pos(inf,20)}", (0.0, 20.0)),
            ("{\\pos(-inf,-inf)}", (0.0, 0.0)),
        ] {
            let tags = OverrideTag::parse_from_text(text);
            assert_eq!(tags.len(), 1, "{}", text);
            assert!(
                matches!(tags[0], OverrideTag::Position(x, y) if x == want.0 && y == want.1),
                "{} -> {:?}",
                text,
                tags[0]
            );
        }
        // Overflow (`1e999`) clamps to a huge-but-finite coordinate:
        // off-screen like libass, still consuming the position slot,
        // and never poisoning layout with inf.
        let tags = OverrideTag::parse_from_text("{\\pos(1e999,20)}");
        assert!(
            matches!(tags[0], OverrideTag::Position(x, 20.0) if x == 1e18),
            "{:?}",
            tags[0]
        );
        let tags = OverrideTag::parse_from_text("{\\pos(-9e999,20)}");
        assert!(
            matches!(tags[0], OverrideTag::Position(x, 20.0) if x == -1e18),
            "{:?}",
            tags[0]
        );
        // Simple tags keep strict parsing (documented gap): garbage
        // there stays Unknown rather than prefix-parsing.
        for text in ["{\\fsinf}", "{\\fscx(NaN)}", "{\\blur(-inf)}"] {
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
    fn test_pos_exact_arity() {
        // libass `nargs == 2`: exactly 2, not >= 2.
        let tags = OverrideTag::parse_from_text("{\\pos(10,20)}Hi");
        assert!(matches!(tags[0], OverrideTag::Position(10.0, 20.0)));
        for text in [
            "{\\pos(10,20,30)}Hi",
            "{\\pos(10)}Hi",
            "{\\pos()}Hi",
            "{\\pos(,)}Hi",
            "{\\pos(1,2,3,4,5,6,7,8,9)}Hi",
        ] {
            let tags = OverrideTag::parse_from_text(text);
            assert!(
                matches!(tags[0], OverrideTag::Unknown(_)),
                "{text:?} -> {tags:?}"
            );
        }
        // Empty segments never count (libass `push_arg` skips them):
        // trailing and double commas are still 2-arg tags.
        for text in [
            "{\\pos(10,20,)}Hi",
            "{\\pos(10,,20)}Hi",
            "{\\pos(,10,20,)}Hi",
        ] {
            let tags = OverrideTag::parse_from_text(text);
            assert!(
                matches!(tags[0], OverrideTag::Position(10.0, 20.0)),
                "{text:?} -> {tags:?}"
            );
        }
    }

    #[test]
    fn test_org_exact_arity() {
        let tags = OverrideTag::parse_from_text("{\\org(10,20)}Hi");
        assert!(matches!(tags[0], OverrideTag::Origin(10.0, 20.0)));
        for text in [
            "{\\org(10,20,30)}Hi",
            "{\\org(10)}Hi",
            "{\\org()}Hi",
            "{\\org(10,20,)}Hi",
        ] {
            let tags = OverrideTag::parse_from_text(text);
            let ok = matches!(tags[0], OverrideTag::Origin(10.0, 20.0));
            let ignored = matches!(tags[0], OverrideTag::Unknown(_));
            // Trailing comma still counts as 2 args; the rest are ignored.
            assert_eq!(ok, text.contains("(10,20,)"), "{text:?} -> {tags:?}");
            assert_eq!(ignored, !text.contains("(10,20,)"), "{text:?} -> {tags:?}");
        }
    }

    #[test]
    fn test_move_exact_arity_and_swap() {
        // 4 args: untimed; 6 args: timed with reversed times swapped.
        let tags = OverrideTag::parse_from_text("{\\move(0,0,100,50)}Hi");
        assert!(matches!(tags[0], OverrideTag::Move(0.0, 0.0, 100.0, 50.0)));
        let tags = OverrideTag::parse_from_text("{\\move(0,0,100,0,1000,2000)}Hi");
        assert!(matches!(
            tags[0],
            OverrideTag::MoveWithTiming(0.0, 0.0, 100.0, 0.0, 1000, 2000)
        ));
        let tags = OverrideTag::parse_from_text("{\\move(0,0,100,0,2000,1000)}Hi");
        assert!(
            matches!(
                tags[0],
                OverrideTag::MoveWithTiming(0.0, 0.0, 100.0, 0.0, 1000, 2000)
            ),
            "reversed times swap: {tags:?}"
        );
        // Everything else is ignored.
        for text in [
            "{\\move(0,0,100)}Hi",
            "{\\move(0,0,100,0,1000)}Hi",
            "{\\move(0,0,100,0,1000,2000,3000)}Hi",
            "{\\move()}Hi",
        ] {
            let tags = OverrideTag::parse_from_text(text);
            assert!(
                matches!(tags[0], OverrideTag::Unknown(_)),
                "{text:?} -> {tags:?}"
            );
        }
        // Trailing commas never count: 4- and 6-arg forms stay valid.
        let tags = OverrideTag::parse_from_text("{\\move(0,0,100,50,)}Hi");
        assert!(matches!(tags[0], OverrideTag::Move(0.0, 0.0, 100.0, 50.0)));
        let tags = OverrideTag::parse_from_text("{\\move(0,0,100,0,1000,2000,)}Hi");
        assert!(matches!(
            tags[0],
            OverrideTag::MoveWithTiming(0.0, 0.0, 100.0, 0.0, 1000, 2000)
        ));
    }

    #[test]
    fn test_fad_fade_shared_count_based_arity() {
        // Count selects behavior, not the name: 2 -> simple, 7 -> complex.
        let tags = OverrideTag::parse_from_text("{\\fade(100,200)}Hi");
        assert!(matches!(tags[0], OverrideTag::Fade(100, 200)));
        let tags = OverrideTag::parse_from_text("{\\fad(255,0,255,0,1000,2000,3000)}Hi");
        assert!(matches!(
            tags[0],
            OverrideTag::ComplexFade(255, 0, 255, 0, 1000, 2000, 3000)
        ));
        let tags = OverrideTag::parse_from_text("{\\fad(100,200)}Hi");
        assert!(matches!(tags[0], OverrideTag::Fade(100, 200)));
        let tags = OverrideTag::parse_from_text("{\\fade(255,0,255,0,1000,2000,3000)}Hi");
        assert!(matches!(
            tags[0],
            OverrideTag::ComplexFade(255, 0, 255, 0, 1000, 2000, 3000)
        ));
        // Anything else is ignored (and consumes no fade slot).
        for text in [
            "{\\fad(100,200,300)}Hi",
            "{\\fade(1,2,3)}Hi",
            "{\\fad(100)}Hi",
            "{\\fade(1,2,3,4,5,6)}Hi",
            "{\\fade(1,2,3,4,5,6,7,8)}Hi",
            "{\\fad()}Hi",
            "{\\fade()}Hi",
        ] {
            let tags = OverrideTag::parse_from_text(text);
            assert!(
                matches!(tags[0], OverrideTag::Unknown(_)),
                "{text:?} -> {tags:?}"
            );
        }
    }

    #[test]
    fn test_libass_f64_prefix_values() {
        // (input, expected) — `ass_strtod` value semantics.
        for (input, want) in [
            ("10", 10.0),
            ("10x", 10.0),
            ("+10", 10.0),
            ("-2.5", -2.5),
            ("  12  ", 12.0),
            ("1e3", 1000.0),
            ("1E-2", 0.01),
            (".5", 0.5),
            ("5.", 5.0),
            ("1.2.3", 1.2),
            ("1e", 1.0),
            ("1_0", 1.0),
            ("0x10", 0.0),
            ("", 0.0),
            ("x", 0.0),
            ("inf", 0.0),
            ("-inf", 0.0),
            ("NaN", 0.0),
            ("+", 0.0),
            (".", 0.0),
            ("e5", 0.0),
        ] {
            assert_eq!(parse_libass_f64(input), want, "{input:?}");
        }
        assert!(parse_libass_f64("1e999").is_infinite());
    }

    #[test]
    fn test_libass_i32_prefix_values() {
        for (input, want) in [
            ("7", 7),
            ("7x", 7),
            ("+7", 7),
            ("  -12  ", -12),
            ("0x10", 0),
            ("", 0),
            ("x", 0),
            ("+", 0),
            ("+-5", 0),
            ("99999999999999999999999", i32::MAX),
            ("-99999999999999999999999", i32::MIN),
        ] {
            assert_eq!(parse_libass_i32(input), want, "{input:?}");
        }
    }

    #[test]
    fn test_libass_dtoi32() {
        assert_eq!(libass_dtoi32(1.9), 1);
        assert_eq!(libass_dtoi32(-1.9), -1);
        assert_eq!(libass_dtoi32(f64::NAN), i32::MIN);
        assert_eq!(libass_dtoi32(1e30), i32::MIN);
        assert_eq!(libass_dtoi32(-1e30), i32::MIN);
        assert_eq!(libass_dtoi32(2147483647.0), i32::MAX);
        assert_eq!(libass_dtoi32(2147483648.0), i32::MIN);
        assert_eq!(libass_dtoi32(-2147483648.0), i32::MIN);
    }

    #[test]
    fn test_argument_whitespace_accepted() {
        // libass skips spaces/tabs around every argument.
        let tags = OverrideTag::parse_from_text("{\\pos( 10 , 20 )}Hi");
        assert!(matches!(tags[0], OverrideTag::Position(10.0, 20.0)));
        let tags = OverrideTag::parse_from_text("{\\org( 10 , 20 )}Hi");
        assert!(matches!(tags[0], OverrideTag::Origin(10.0, 20.0)));
        let tags = OverrideTag::parse_from_text("{\\move( 0 , 0 , 100 , 100 )}Hi");
        assert!(matches!(tags[0], OverrideTag::Move(0.0, 0.0, 100.0, 100.0)));
        let tags = OverrideTag::parse_from_text("{\\fad( 100 , 200 )}Hi");
        assert!(matches!(tags[0], OverrideTag::Fade(100, 200)));
        let tags =
            OverrideTag::parse_from_text("{\\fade( 255 , 0 , 255 , 0 , 1000 , 2000 , 3000 )}Hi");
        assert!(matches!(
            tags[0],
            OverrideTag::ComplexFade(255, 0, 255, 0, 1000, 2000, 3000)
        ));
        // Tabs too, plus spaces between the name and `(` and after `\`.
        let tags = OverrideTag::parse_from_text("{\\pos\t(\t10\t,\t20\t)}Hi");
        assert!(matches!(tags[0], OverrideTag::Position(10.0, 20.0)));
        let tags = OverrideTag::parse_from_text("{\\pos (10,20)}Hi");
        assert!(matches!(tags[0], OverrideTag::Position(10.0, 20.0)));
        let tags = OverrideTag::parse_from_text("{\\ pos(10,20)}Hi");
        assert!(matches!(tags[0], OverrideTag::Position(10.0, 20.0)));
    }

    #[test]
    fn test_numeric_edge_values_never_ignore_tag() {
        // Signs, partial numerics, and overflow still apply the tag
        // with prefix-parsed values; the tag is never dropped.
        let tags = OverrideTag::parse_from_text("{\\pos(+10,-20)}Hi");
        assert!(matches!(tags[0], OverrideTag::Position(10.0, -20.0)));
        let tags = OverrideTag::parse_from_text("{\\pos(10x,20y)}Hi");
        assert!(matches!(tags[0], OverrideTag::Position(10.0, 20.0)));
        let tags = OverrideTag::parse_from_text("{\\pos(x,20)}Hi");
        assert!(matches!(tags[0], OverrideTag::Position(0.0, 20.0)));
        let tags = OverrideTag::parse_from_text("{\\move(0,0,100,0,-500,1500)}Hi");
        assert!(matches!(
            tags[0],
            OverrideTag::MoveWithTiming(0.0, 0.0, 100.0, 0.0, -500, 1500)
        ));
        let tags = OverrideTag::parse_from_text("{\\move(0,0,100,0,9999999999999999999,5)}Hi");
        assert!(matches!(
            tags[0],
            OverrideTag::MoveWithTiming(0.0, 0.0, 100.0, 0.0, 5, i32::MAX)
        ));
        let tags = OverrideTag::parse_from_text("{\\fad(x,200)}Hi");
        assert!(matches!(tags[0], OverrideTag::Fade(0, 200)));
        let tags = OverrideTag::parse_from_text("{\\fad(-100,200)}Hi");
        assert!(matches!(tags[0], OverrideTag::Fade(-100, 200)));
    }

    #[test]
    fn test_an_prefix_values_and_slot_markers() {
        // Digit prefixes win; bare/garbage/out-of-range mark the slot
        // consumed with a style fallback (never Unknown).
        for (text, want) in [
            ("{\\an7x}Hi", 7),
            ("{\\an(7,x)}Hi", 7),
            ("{\\an(7,8)}Hi", 7),
            ("{\\an7,8}Hi", 7),
        ] {
            let tags = OverrideTag::parse_from_text(text);
            assert!(
                matches!(tags[0], OverrideTag::Alignment(a) if a == want),
                "{text:?} -> {tags:?}"
            );
        }
        for text in [
            "{\\anfoo}Hi",
            "{\\an}Hi",
            "{\\an99}Hi",
            "{\\an0}Hi",
            "{\\an(x)}Hi",
        ] {
            let tags = OverrideTag::parse_from_text(text);
            assert!(
                matches!(tags[0], OverrideTag::AlignmentReset),
                "{text:?} -> {tags:?}"
            );
        }
        for text in ["{\\afoo}Hi", "{\\a}Hi", "{\\a0}Hi", "{\\a12}Hi"] {
            let tags = OverrideTag::parse_from_text(text);
            assert!(
                matches!(tags[0], OverrideTag::AlignmentReset),
                "{text:?} -> {tags:?}"
            );
        }
    }

    #[test]
    fn test_complex_tag_prefix_names() {
        // libass `complex_tag` is a prefix match: junk between the
        // keyword and `(` is ignored.
        let tags = OverrideTag::parse_from_text("{\\position(10,20)}Hi");
        assert!(matches!(tags[0], OverrideTag::Position(10.0, 20.0)));
        let tags = OverrideTag::parse_from_text("{\\movement(0,0,9,9)}Hi");
        assert!(matches!(tags[0], OverrideTag::Move(0.0, 0.0, 9.0, 9.0)));
        let tags = OverrideTag::parse_from_text("{\\orgx(1,2)}Hi");
        assert!(matches!(tags[0], OverrideTag::Origin(1.0, 2.0)));
        let tags = OverrideTag::parse_from_text("{\\fader(100,200)}Hi");
        assert!(matches!(tags[0], OverrideTag::Fade(100, 200)));
        let tags = OverrideTag::parse_from_text("{\\t2(1,2,\\fs30)}Hi");
        assert!(matches!(
            tags[0],
            OverrideTag::Transform { t1: 1, t2: 2, .. }
        ));
        let tags = OverrideTag::parse_from_text("{\\clipx(1,2,3,4)}Hi");
        assert!(matches!(tags[0], OverrideTag::Clip(1, 2, 3, 4)));
        // Digit junk works too (the name scan covers digits).
        let tags = OverrideTag::parse_from_text("{\\pos2(10,20)}Hi");
        assert!(matches!(tags[0], OverrideTag::Position(10.0, 20.0)));
    }

    #[test]
    fn test_complex_tags_require_parens() {
        // Without parens libass sees `nargs == 0` and ignores complex
        // tags, even with glued text.
        for text in [
            "{\\pos10,20}Hi",
            "{\\move1,2,3,4}Hi",
            "{\\org1,2}Hi",
            "{\\fad100,200}Hi",
            "{\\fade1,2,3,4,5,6,7}Hi",
            "{\\clip1,2,3,4}Hi",
            "{\\t100,200}Hi",
            "{\\pos}Hi",
            "{\\move}Hi",
        ] {
            let tags = OverrideTag::parse_from_text(text);
            assert!(
                matches!(tags[0], OverrideTag::Unknown(_)),
                "{text:?} -> {tags:?}"
            );
        }
    }

    #[test]
    fn test_transform_cnt_forms() {
        // 2-timing-arg form parses floats (`dtoi32`), 3-arg form ints.
        let tags = OverrideTag::parse_from_text("{\\t(1e3,2000,\\fs30)}Hi");
        assert!(matches!(
            tags[0],
            OverrideTag::Transform {
                t1: 1000,
                t2: 2000,
                ..
            }
        ));
        let tags = OverrideTag::parse_from_text("{\\t(1e3,2000,1,\\fs30)}Hi");
        assert!(matches!(
            tags[0],
            OverrideTag::Transform {
                t1: 1,
                t2: 2000,
                ..
            }
        ));
        // A non-comma tail before the backslash is the swallowed arg.
        let tags = OverrideTag::parse_from_text("{\\t(1,2,x\\y)}Hi");
        assert!(matches!(
            tags[0],
            OverrideTag::Transform { t1: 1, t2: 2, .. }
        ));
        // No inner tags, or more than 3 timing args: ignored.
        for text in ["{\\t(100,200)}Hi", "{\\t()}Hi", "{\\t(1,2,3,4,\\fs30)}Hi"] {
            let tags = OverrideTag::parse_from_text(text);
            assert!(
                matches!(tags[0], OverrideTag::Unknown(_)),
                "{text:?} -> {tags:?}"
            );
        }
    }

    #[test]
    fn test_fsc_resets_scale() {
        for text in ["{\\fsc}Hi", "{\\fsc50}Hi", "{\\fsc(1,2)}Hi"] {
            let tags = OverrideTag::parse_from_text(text);
            assert!(
                matches!(tags[0], OverrideTag::ScaleReset),
                "{text:?} -> {tags:?}"
            );
        }
    }

    #[test]
    fn test_multibyte_color_values_fail_without_panicking() {
        // Byte-length slicing must never split a char: multibyte hex
        // fails cleanly (these byte lengths hit the 6/8-digit arms).
        for text in [
            "{\\c&Haébcdef&}Hi",
            "{\\c&Haébcd&}Hi",
            "{\\c&Héééé&}Hi",
            "{\\c&Hééé&}Hi",
            "{\\1a&Hé&}Hi",
        ] {
            let tags = OverrideTag::parse_from_text(text);
            assert!(
                matches!(tags[0], OverrideTag::Unknown(_)),
                "{text:?} -> {tags:?}"
            );
        }
    }

    #[test]
    fn test_deeply_nested_transforms_degrade_without_overflow() {
        // 200 nested `\t(` would recurse 200 deep uncapped; past the
        // cap the inner tags degrade to Unknown instead.
        let mut text = String::from("{");
        for _ in 0..200 {
            text.push_str("\\t(\\");
        }
        text.push_str("fs30");
        for _ in 0..200 {
            text.push(')');
        }
        text.push_str("}Hi");
        let tags = OverrideTag::parse_from_text(&text);
        assert!(!tags.is_empty());
        // Outermost still parses as a transform.
        assert!(matches!(tags[0], OverrideTag::Transform { .. }));
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
        assert!(!OverrideTag::Bold(700).is_line_global());
        assert!(!OverrideTag::FontSize(10.0).is_line_global());
        assert!(!OverrideTag::FontSizeRelative(10.0).is_line_global());
        assert!(!OverrideTag::FontSizeReset.is_line_global());
        // \an is line-global (survives \r, first wins); \q is
        // event-layout but resolved separately (last wins).
        assert!(OverrideTag::Alignment(7).is_line_global());
        assert!(OverrideTag::AlignmentReset.is_line_global());
        assert!(!OverrideTag::WrapStyle(2).is_line_global());
        assert!(!OverrideTag::Drawing(1).is_line_global());
    }

    #[test]
    fn test_event_layout_classification() {
        // Line-global tags are event-layout too.
        assert!(OverrideTag::Position(0.0, 0.0).is_event_layout());
        assert!(OverrideTag::Move(0.0, 0.0, 1.0, 1.0).is_event_layout());
        assert!(OverrideTag::Origin(0.0, 0.0).is_event_layout());
        assert!(OverrideTag::InverseClip(0, 0, 1, 1).is_event_layout());
        assert!(OverrideTag::ComplexFade(0, 0, 0, 0, 0, 0, 0).is_event_layout());
        // Plus whole-line layout properties.
        assert!(OverrideTag::Alignment(7).is_event_layout());
        assert!(OverrideTag::AlignmentReset.is_event_layout());
        assert!(OverrideTag::WrapStyle(2).is_event_layout());
        // Segment-level tags are neither.
        assert!(!OverrideTag::Bold(700).is_event_layout());
        assert!(!OverrideTag::FontSize(10.0).is_event_layout());
        assert!(!OverrideTag::FontSizeRelative(10.0).is_event_layout());
        assert!(!OverrideTag::FontSizeReset.is_event_layout());
        assert!(!OverrideTag::KaraokeDuration(10).is_event_layout());
        assert!(!OverrideTag::Drawing(1).is_event_layout());
        assert!(!OverrideTag::Reset(None).is_event_layout());
    }

    #[test]
    fn test_parse_bare_alignment_resets() {
        // Bare \an / \a reset to the style alignment (libass still
        // consumes PARSED_A); they are known tags, never Unknown.
        for text in ["{\\an}Hi", "{\\a}Hi", "{\\an }Hi"] {
            let tags = OverrideTag::parse_from_text(text);
            assert!(
                matches!(tags[0], OverrideTag::AlignmentReset),
                "{text:?} -> {tags:?}"
            );
        }
    }

    #[test]
    fn test_parse_legacy_a_quirk_and_range() {
        // VSFilter quirk shared by libass: \a4 and \a8 behave as \a5
        // (top-left, ASS numpad 7).
        for text in ["{\\a4}Hi", "{\\a8}Hi"] {
            let tags = OverrideTag::parse_from_text(text);
            assert!(
                matches!(tags[0], OverrideTag::Alignment(7)),
                "{text:?} -> {tags:?}"
            );
        }
        // Values outside 1-11 fall back to the style alignment.
        for text in ["{\\a0}Hi", "{\\a12}Hi", "{\\a-1}Hi"] {
            let tags = OverrideTag::parse_from_text(text);
            assert!(
                matches!(tags[0], OverrideTag::AlignmentReset),
                "{text:?} -> {tags:?}"
            );
        }
        // In-range values convert to ASS numpad as before.
        let tags = OverrideTag::parse_from_text("{\\a6}Hi");
        assert!(matches!(tags[0], OverrideTag::Alignment(8)));
    }

    #[test]
    fn test_parse_reset_keeps_drawing_mode() {
        // libass `ass_reset_render_context` never touches
        // `drawing_scale`: `\r` inside a drawing does NOT re-enable
        // break escapes; the text after it stays drawing commands.
        let segs = parse_text_segments("{\\p1}m 0 0{\\r}a\\Nb");
        let joined: String = segs.iter().map(|s| s.text.as_str()).collect();
        assert!(!joined.contains('\n'), "{joined:?}");
        // Only `\p0` exits drawing mode and re-enables breaks.
        let segs = parse_text_segments("{\\p1}m 0 0{\\p0}a\\Nb");
        let joined: String = segs.iter().map(|s| s.text.as_str()).collect();
        assert!(joined.contains('\n'), "{joined:?}");
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
