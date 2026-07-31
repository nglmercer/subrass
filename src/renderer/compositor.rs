use super::buffer::RenderBuffer;
use super::effects;
use super::font::FontManager;
use super::glyph_cache::GlyphCache;
use super::shaper::TextShaper;
use crate::types::color::Color;
use crate::types::override_tag::{parse_text_segments, TextSegment};
use crate::types::{Event, EventType, OverrideTag, Style};
use crate::utils::Matrix3x3;
use ab_glyph::FontArc;
use std::borrow::Cow;

/// Resolved style with all overrides applied
#[derive(Debug, Clone)]
pub struct ResolvedStyle {
    pub base_style: Style,
    pub font_name: String,
    pub font_size: f64,
    pub color: Color,
    pub secondary_color: Color,
    pub outline_color: Color,
    pub shadow_color: Color,
    pub back_color: Color,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike_out: bool,
    pub scale_x: f64,
    pub scale_y: f64,
    pub spacing: f64,
    pub angle: f64,
    pub rotation_x: f64,
    pub rotation_y: f64,
    pub border_style: i32,
    pub outline: f64,
    pub shadow: f64,
    pub alignment: i32,
    pub margin_l: i32,
    pub margin_r: i32,
    pub margin_v: i32,
    pub position: Option<(f64, f64)>,
    pub origin: Option<(f64, f64)>,
    pub move_data: Option<MoveData>,
    pub clip: Option<(i32, i32, i32, i32)>,
    pub inverse_clip: Option<(i32, i32, i32, i32)>,
    pub fade_in: u64,
    pub fade_out: u64,
    pub complex_fade: Option<ComplexFade>,
    pub drawing_mode: i32,
    pub blur: f64,
}

/// Move animation data
#[derive(Debug, Clone)]
pub struct MoveData {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub t1: u64,
    pub t2: u64,
}

/// Complex fade data
#[derive(Debug, Clone)]
pub struct ComplexFade {
    pub a1: u8,
    pub a2: u8,
    pub a3: u8,
    pub t1: u64,
    pub t2: u64,
    pub t3: u64,
    pub t4: u64,
}

/// A word plus any raw tag groups that preceded it, with measured width
#[derive(Debug, Clone)]
struct WrapWord {
    prefix: String,
    text: String,
    width: f64,
    /// True when this word was separated from the previous word by a space
    /// in the original text. Used to avoid inserting phantom spaces between
    /// words that were only split by override-tag boundaries (e.g. karaoke).
    preceded_by_space: bool,
}

/// Insert '\n' at word boundaries per the ASS wrap style:
/// 0/1 = greedy fill (top line widest), 2 = no automatic wrapping,
/// 3 = greedy fill from the bottom (bottom line widest).
///
/// Tag groups are opaque and travel with the word that follows them;
/// explicit `\N`/`\n` breaks split the text into independently wrapped
/// runs. Words wider than `max_width` stay on their own line (no
/// character-level splitting).
fn wrap_event_text(
    text: &str,
    wrap_style: i32,
    max_width: f64,
    font: &FontArc,
    font_size: f64,
    spacing: f64,
) -> String {
    if wrap_style == 2 || max_width <= 0.0 {
        return text.to_string();
    }

    // Tokenize into words (with pending tag prefixes) and hard breaks.
    let mut words: Vec<WrapWord> = Vec::new();
    // Break markers: index into `words` where a new run starts
    let mut run_starts: Vec<usize> = vec![0];
    let mut prefix = String::new();
    let mut word = String::new();

    let mut preceded_by_space = false; // first word is not preceded by a space

    let flush =
        |prefix: &mut String, word: &mut String, words: &mut Vec<WrapWord>, pbys: &mut bool| {
            // Flush when there is text, or when a tag prefix must be preserved
            // (e.g. {\b0} between "Bold" and the following space).
            if word.is_empty() && prefix.is_empty() {
                return;
            }
            let has_text = !word.is_empty();
            let width = TextShaper::measure_text(word, font, font_size, spacing);
            words.push(WrapWord {
                prefix: std::mem::take(prefix),
                text: std::mem::take(word),
                width,
                preceded_by_space: *pbys,
            });
            // Only reset when there's actual text — prefix-only entries don't
            // "consume" the space flag.
            if has_text {
                *pbys = false;
            }
        };

    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                flush(&mut prefix, &mut word, &mut words, &mut preceded_by_space);
                prefix.push('{');
                for gc in chars.by_ref() {
                    prefix.push(gc);
                    if gc == '}' {
                        break;
                    }
                }
            }
            '\\' => match chars.peek() {
                Some('N') | Some('n') => {
                    chars.next();
                    flush(&mut prefix, &mut word, &mut words, &mut preceded_by_space);
                    preceded_by_space = true;
                    run_starts.push(words.len());
                }
                Some('h') => {
                    chars.next();
                    word.push('\u{00A0}');
                }
                Some(&n) => {
                    chars.next();
                    word.push('\\');
                    word.push(n);
                }
                None => word.push('\\'),
            },
            ' ' | '\t' => {
                flush(&mut prefix, &mut word, &mut words, &mut preceded_by_space);
                preceded_by_space = true;
            }
            _ => {
                word.push(c);
            }
        }
    }
    flush(&mut prefix, &mut word, &mut words, &mut preceded_by_space);
    // Trailing tag groups with no word attach as a zero-width word
    if !prefix.is_empty() {
        words.push(WrapWord {
            prefix,
            text: String::new(),
            width: 0.0,
            preceded_by_space: false,
        });
    }

    let space_width = TextShaper::measure_text(" ", font, font_size, spacing);
    let bottom_wider = wrap_style == 3;

    // Wrap each explicit-line run independently, then rejoin with breaks.
    let mut out = String::with_capacity(text.len() + 16);
    let mut run_begin = 0usize;
    for start in run_starts.iter().skip(1) {
        render_wrapped_run(
            &words[run_begin..*start],
            bottom_wider,
            max_width,
            space_width,
            &mut out,
        );
        out.push('\n');
        run_begin = *start;
    }
    render_wrapped_run(
        &words[run_begin..],
        bottom_wider,
        max_width,
        space_width,
        &mut out,
    );

    out
}

/// Greedy-wrap one run of words and append the result to `out`.
fn render_wrapped_run(
    words: &[WrapWord],
    bottom_wider: bool,
    max_width: f64,
    space_width: f64,
    out: &mut String,
) {
    if words.is_empty() {
        return;
    }

    // Group word indices into lines. Greedy fill makes the top line the
    // widest; filling from the end (style 3) makes the bottom the widest.
    let mut lines: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut cur_w = 0.0_f64;
    let order: Box<dyn Iterator<Item = usize>> = if bottom_wider {
        Box::new((0..words.len()).rev())
    } else {
        Box::new(0..words.len())
    };
    for idx in order {
        let ww = words[idx].width;
        if ww == 0.0 && words[idx].text.is_empty() {
            // Prefix-only entry (e.g. trailing tag group): attach to current
            // line without consuming space budget — no space is emitted for it.
            cur.push(idx);
            continue;
        }
        if cur.is_empty() {
            cur_w = ww;
            cur.push(idx);
        } else if cur_w + space_width + ww <= max_width {
            cur_w += space_width + ww;
            cur.push(idx);
        } else {
            lines.push(std::mem::take(&mut cur));
            cur_w = ww;
            cur.push(idx);
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if bottom_wider {
        lines.reverse();
        for line in &mut lines {
            line.reverse();
        }
    }

    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let mut prev_had_text = false;
        for (j, &idx) in line.iter().enumerate() {
            let w = &words[idx];
            if j > 0 && prev_had_text && !w.text.is_empty() && w.preceded_by_space {
                out.push(' ');
            }
            out.push_str(&w.prefix);
            out.push_str(&w.text);
            // Once we've emitted text, stay true — prefix-only entries must
            // not flip this back to false or the next word loses its space.
            if !w.text.is_empty() {
                prev_had_text = true;
            }
        }
    }
}

/// Karaoke syllable kind
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KaraokeKind {
    /// `\k` — hard color swap when the syllable starts
    Hard,
    /// `\K` / `\kf` — left-to-right color sweep over the syllable
    Sweep,
    /// `\ko` — outline hidden once the syllable starts
    Outline,
}

/// A karaoke syllable: timing relative to event start plus measured width
#[derive(Debug, Clone)]
struct KaraokeSyllable {
    start_ms: u64,
    dur_ms: u64,
    kind: KaraokeKind,
    width: f64,
}

/// Build the karaoke syllable timeline for segmented event text.
///
/// Returns the syllables (times in ms relative to event start, with
/// measured widths) and, per segment, the index of the syllable it belongs
/// to (`None` for text before the first karaoke tag). Segments after a
/// karaoke tag keep belonging to that syllable until the next karaoke tag.
fn build_karaoke_timeline(
    segments: &[TextSegment],
    font: &FontArc,
    font_size: f64,
    spacing: f64,
) -> (Vec<KaraokeSyllable>, Vec<Option<usize>>) {
    let mut syllables: Vec<KaraokeSyllable> = Vec::new();
    let mut seg_syllable: Vec<Option<usize>> = vec![None; segments.len()];
    let mut clock = 0u64;
    let mut prev_tag_count = 0usize;

    for (i, segment) in segments.iter().enumerate() {
        // Segments carry the accumulated tag list, so only tags added by
        // this segment can start a new syllable.
        let from = prev_tag_count.min(segment.tags.len());
        let new_tags = &segment.tags[from..];
        prev_tag_count = segment.tags.len();

        let mut started = None;
        for tag in new_tags {
            match tag {
                OverrideTag::KaraokeDuration(d) => started = Some((KaraokeKind::Hard, *d)),
                OverrideTag::KaraokeSweep(d) => started = Some((KaraokeKind::Sweep, *d)),
                OverrideTag::KaraokeOutline(d) => started = Some((KaraokeKind::Outline, *d)),
                _ => {}
            }
        }

        if let Some((kind, dur_cs)) = started {
            syllables.push(KaraokeSyllable {
                start_ms: clock,
                dur_ms: dur_cs.saturating_mul(10),
                kind,
                width: 0.0,
            });
            clock = clock.saturating_add(dur_cs.saturating_mul(10));
        }

        if !syllables.is_empty() {
            let idx = syllables.len() - 1;
            syllables[idx].width +=
                TextShaper::measure_text(&segment.text, font, font_size, spacing);
            seg_syllable[i] = Some(idx);
        }
    }

    (syllables, seg_syllable)
}

/// Compositor - composites resolved subtitle events into a buffer
pub struct Compositor {
    glyph_cache: GlyphCache,
}

impl Compositor {
    pub fn new() -> Self {
        Self {
            glyph_cache: GlyphCache::new(4096),
        }
    }

    /// Interpolate between two colors
    fn interpolate_color(from: Color, to: Color, t: f64) -> Color {
        let t = t.clamp(0.0, 1.0);
        Color {
            alpha: (from.alpha as f64 + (to.alpha as f64 - from.alpha as f64) * t) as u8,
            red: (from.red as f64 + (to.red as f64 - from.red as f64) * t) as u8,
            green: (from.green as f64 + (to.green as f64 - from.green as f64) * t) as u8,
            blue: (from.blue as f64 + (to.blue as f64 - from.blue as f64) * t) as u8,
        }
    }

    /// Apply accel function: t' = 1 - (1-t)^accel
    fn apply_accel(t: f64, accel: f64) -> f64 {
        if accel == 1.0 {
            t
        } else {
            1.0 - (1.0 - t).powf(accel)
        }
    }

    /// Apply transform tags with a given progress (0.0 to 1.0)
    fn apply_transform_tags(resolved: &mut ResolvedStyle, tags: &[OverrideTag], progress: f64) {
        for target in tags {
            match target {
                OverrideTag::Blur(b) => {
                    let from = resolved.blur;
                    resolved.blur = from + (b - from) * progress;
                }
                OverrideTag::EdgeBlur(b) => {
                    let from = resolved.blur;
                    resolved.blur = from + (b - from) * progress;
                }
                OverrideTag::Border(b) => {
                    let from = resolved.outline;
                    resolved.outline = from + (b - from) * progress;
                }
                OverrideTag::Shadow(s) => {
                    let from = resolved.shadow;
                    resolved.shadow = from + (s - from) * progress;
                }
                OverrideTag::PrimaryColor(c) => {
                    resolved.color = Self::interpolate_color(resolved.color, *c, progress);
                }
                OverrideTag::SecondaryColor(c) => {
                    resolved.secondary_color =
                        Self::interpolate_color(resolved.secondary_color, *c, progress);
                }
                OverrideTag::OutlineColor(c) => {
                    resolved.outline_color =
                        Self::interpolate_color(resolved.outline_color, *c, progress);
                }
                OverrideTag::ShadowColor(c) => {
                    resolved.shadow_color =
                        Self::interpolate_color(resolved.shadow_color, *c, progress);
                }
                OverrideTag::PrimaryAlpha(a) => {
                    let from = resolved.color.alpha;
                    resolved.color.alpha =
                        (from as f64 + (*a as f64 - from as f64) * progress) as u8;
                }
                OverrideTag::OutlineAlpha(a) => {
                    let from = resolved.outline_color.alpha;
                    resolved.outline_color.alpha =
                        (from as f64 + (*a as f64 - from as f64) * progress) as u8;
                }
                OverrideTag::ShadowAlpha(a) => {
                    let from = resolved.shadow_color.alpha;
                    resolved.shadow_color.alpha =
                        (from as f64 + (*a as f64 - from as f64) * progress) as u8;
                }
                OverrideTag::ScaleX(s) => {
                    let from = resolved.scale_x;
                    resolved.scale_x = from + (s - from) * progress;
                }
                OverrideTag::ScaleY(s) => {
                    let from = resolved.scale_y;
                    resolved.scale_y = from + (s - from) * progress;
                }
                OverrideTag::FontSize(s) => {
                    let from = resolved.font_size;
                    resolved.font_size = from + (s - from) * progress;
                }
                OverrideTag::LetterSpacing(s) => {
                    let from = resolved.spacing;
                    resolved.spacing = from + (s - from) * progress;
                }
                OverrideTag::BorderX(b) => {
                    let from = resolved.outline;
                    resolved.outline = from + (b - from) * progress;
                }
                OverrideTag::BorderY(b) => {
                    let from = resolved.outline;
                    resolved.outline = from + (b - from) * progress;
                }
                OverrideTag::RotationZ(r) => {
                    let from = resolved.angle;
                    resolved.angle = from + (r - from) * progress;
                }
                OverrideTag::RotationX(r) => {
                    let from = resolved.rotation_x;
                    resolved.rotation_x = from + (r - from) * progress;
                }
                OverrideTag::RotationY(r) => {
                    let from = resolved.rotation_y;
                    resolved.rotation_y = from + (r - from) * progress;
                }
                _ => {}
            }
        }
    }

    /// Resolve an event's style with all override tags applied (no animation)
    fn resolve_base_style(base_style: &Style, tags: &[OverrideTag]) -> ResolvedStyle {
        let mut resolved = ResolvedStyle {
            base_style: base_style.clone(),
            font_name: base_style.font_name.clone(),
            font_size: base_style.font_size,
            color: base_style.primary_color,
            secondary_color: base_style.secondary_color,
            outline_color: base_style.outline_color,
            shadow_color: base_style.back_color,
            back_color: base_style.back_color,
            bold: base_style.bold,
            italic: base_style.italic,
            underline: base_style.underline,
            strike_out: base_style.strike_out,
            scale_x: base_style.scale_x,
            scale_y: base_style.scale_y,
            spacing: base_style.spacing,
            angle: base_style.angle,
            rotation_x: 0.0,
            rotation_y: 0.0,
            border_style: base_style.border_style,
            outline: base_style.outline,
            shadow: base_style.shadow,
            alignment: base_style.alignment,
            margin_l: base_style.margin_l,
            margin_r: base_style.margin_r,
            margin_v: base_style.margin_v,
            position: None,
            origin: None,
            move_data: None,
            clip: None,
            inverse_clip: None,
            fade_in: 0,
            fade_out: 0,
            complex_fade: None,
            drawing_mode: 0,
            blur: 0.0,
        };

        // Apply override tags (skip Transform tags - they're handled separately)
        for tag in tags {
            if let OverrideTag::Transform { .. } = tag {
                continue;
            }
            Self::apply_single_tag(&mut resolved, tag);
        }

        resolved
    }

    /// Apply a single override tag to a resolved style
    fn apply_single_tag(resolved: &mut ResolvedStyle, tag: &OverrideTag) {
        match tag {
            OverrideTag::Bold(v) => resolved.bold = *v,
            OverrideTag::Italic(v) => resolved.italic = *v,
            OverrideTag::Underline(v) => resolved.underline = *v,
            OverrideTag::StrikeOut(v) => resolved.strike_out = *v,
            OverrideTag::FontName(name) => resolved.font_name = name.clone(),
            OverrideTag::FontSize(size) => resolved.font_size = *size,
            OverrideTag::FontSizeMultiplier(mult) => resolved.font_size *= mult,
            OverrideTag::LetterSpacing(sp) => resolved.spacing = *sp,
            OverrideTag::PrimaryColor(c) => resolved.color = *c,
            OverrideTag::SecondaryColor(c) => resolved.secondary_color = *c,
            OverrideTag::OutlineColor(c) => resolved.outline_color = *c,
            OverrideTag::ShadowColor(c) => resolved.shadow_color = *c,
            OverrideTag::Alpha(a) => {
                resolved.color = resolved.color.with_alpha(*a);
            }
            OverrideTag::PrimaryAlpha(a) => {
                resolved.color = resolved.color.with_alpha(*a);
            }
            OverrideTag::OutlineAlpha(a) => {
                resolved.outline_color = resolved.outline_color.with_alpha(*a);
            }
            OverrideTag::ShadowAlpha(a) => {
                resolved.shadow_color = resolved.shadow_color.with_alpha(*a);
            }
            OverrideTag::Position(x, y) => resolved.position = Some((*x, *y)),
            OverrideTag::Move(x1, y1, x2, y2) => {
                resolved.move_data = Some(MoveData {
                    x1: *x1,
                    y1: *y1,
                    x2: *x2,
                    y2: *y2,
                    t1: 0,
                    t2: 0,
                });
            }
            OverrideTag::MoveWithTiming(x1, y1, x2, y2, t1, t2) => {
                resolved.move_data = Some(MoveData {
                    x1: *x1,
                    y1: *y1,
                    x2: *x2,
                    y2: *y2,
                    t1: *t1,
                    t2: *t2,
                });
            }
            OverrideTag::Origin(x, y) => resolved.origin = Some((*x, *y)),
            OverrideTag::Alignment(a) => resolved.alignment = *a,
            OverrideTag::ScaleX(s) => resolved.scale_x = *s,
            OverrideTag::ScaleY(s) => resolved.scale_y = *s,
            OverrideTag::RotationZ(r) => resolved.angle = *r,
            OverrideTag::RotationX(r) => resolved.rotation_x = *r,
            OverrideTag::RotationY(r) => resolved.rotation_y = *r,
            OverrideTag::Border(b) => resolved.outline = *b,
            OverrideTag::BorderX(b) => resolved.outline = *b,
            OverrideTag::BorderY(b) => resolved.outline = *b,
            OverrideTag::Shadow(s) => resolved.shadow = *s,
            OverrideTag::ShadowX(s) => resolved.shadow = *s,
            OverrideTag::ShadowY(s) => resolved.shadow = *s,
            OverrideTag::Fade(fi, fo) => {
                resolved.fade_in = *fi;
                resolved.fade_out = *fo;
            }
            OverrideTag::ComplexFade(a1, a2, a3, t1, t2, t3, t4) => {
                resolved.complex_fade = Some(ComplexFade {
                    a1: *a1 as u8,
                    a2: *a2 as u8,
                    a3: *a3 as u8,
                    t1: *t1,
                    t2: *t2,
                    t3: *t3,
                    t4: *t4,
                });
            }
            OverrideTag::Clip(x1, y1, x2, y2) => {
                resolved.clip = Some((*x1, *y1, *x2, *y2));
            }
            OverrideTag::InverseClip(x1, y1, x2, y2) => {
                resolved.inverse_clip = Some((*x1, *y1, *x2, *y2));
            }
            OverrideTag::Blur(b) => resolved.blur = *b,
            OverrideTag::EdgeBlur(b) => resolved.blur = *b,
            OverrideTag::Drawing(mode) => resolved.drawing_mode = *mode,
            _ => {}
        }
    }

    /// Resolve an event's style with all override tags applied (legacy method)
    pub fn resolve_style(base_style: &Style, event: &Event) -> ResolvedStyle {
        // Override groups after the first visible text segment are
        // segment-local. Only the initial groups establish event-wide
        // positioning and style defaults.
        let initial_segments = parse_text_segments(&event.text);
        let initial_tags = initial_segments
            .first()
            .map(|segment| segment.tags.as_slice())
            .unwrap_or(&[]);
        let mut resolved = Self::resolve_base_style(base_style, initial_tags);

        // Apply event-level margin overrides
        if event.margin_l != 0 {
            resolved.margin_l = event.margin_l;
        }
        if event.margin_r != 0 {
            resolved.margin_r = event.margin_r;
        }
        if event.margin_v != 0 {
            resolved.margin_v = event.margin_v;
        }

        resolved
    }

    /// Convert an alignment anchor into the top-left text origin.
    fn anchor_to_origin(
        alignment: i32,
        anchor_x: f64,
        anchor_y: f64,
        width: f64,
        height: f64,
    ) -> (f64, f64) {
        let x = match alignment {
            1 | 4 | 7 => anchor_x,
            2 | 5 | 8 => anchor_x - width / 2.0,
            3 | 6 | 9 => anchor_x - width,
            _ => anchor_x - width / 2.0,
        };
        let y = match alignment {
            7..=9 => anchor_y,
            4..=6 => anchor_y - height / 2.0,
            1..=3 => anchor_y - height,
            _ => anchor_y - height / 2.0,
        };
        (x, y)
    }

    /// Calculate event position based on alignment, margins, and resolution
    /// Returns the BASELINE position for the text
    #[allow(clippy::too_many_arguments)]
    pub fn calculate_position(
        resolved: &ResolvedStyle,
        text_width: f64,
        text_height: f64,
        baseline: f64,
        play_res_x: u32,
        play_res_y: u32,
        video_width: u32,
        video_height: u32,
    ) -> (f64, f64) {
        let scale_x = video_width as f64 / play_res_x as f64;
        let scale_y = video_height as f64 / play_res_y as f64;

        if let Some((px, py)) = resolved.position {
            // \pos(x,y) specifies the anchor point based on alignment.
            // Adjust position so the anchor point lands at (px, py).
            let scaled_x = px * scale_x;
            let scaled_y = py * scale_y;
            return Self::anchor_to_origin(
                resolved.alignment,
                scaled_x,
                scaled_y,
                text_width,
                text_height,
            );
        }

        let descent = baseline - text_height;
        let alignment = resolved.alignment;
        let margin_l = resolved.margin_l as f64 * scale_x;
        let margin_r = resolved.margin_r as f64 * scale_x;
        let margin_v = resolved.margin_v as f64 * scale_y;

        let x = match alignment {
            1 | 4 | 7 => margin_l,
            2 | 5 | 8 => (video_width as f64 - text_width) / 2.0,
            3 | 6 | 9 => video_width as f64 - margin_r - text_width,
            _ => (video_width as f64 - text_width) / 2.0,
        };

        let y = match alignment {
            7..=9 => margin_v + baseline,
            4..=6 => video_height as f64 / 2.0,
            1..=3 => (video_height as f64 - margin_v) - descent,
            _ => (video_height as f64 - margin_v) - descent,
        };

        (x, y)
    }

    /// Composite a single event into the buffer using per-segment rendering.
    /// Effects that clear or blur pixels are isolated to this event first.
    #[allow(clippy::too_many_arguments)]
    pub fn composite_event(
        &mut self,
        buffer: &mut RenderBuffer,
        event: &Event,
        resolved: &ResolvedStyle,
        font_manager: &FontManager,
        time_ms: u64,
        play_res_x: u32,
        play_res_y: u32,
        video_width: u32,
        video_height: u32,
        script_wrap_style: i32,
    ) {
        if event.event_type == EventType::Comment {
            return;
        }

        let start_ms = event.start.to_millis();
        let end_ms = event.end.to_millis();
        if time_ms < start_ms || time_ms >= end_ms {
            return;
        }

        if resolved.clip.is_some() || resolved.inverse_clip.is_some() || resolved.blur > 0.0 {
            let mut event_buffer = RenderBuffer::new(video_width, video_height);
            self.composite_event_inner(
                &mut event_buffer,
                event,
                resolved,
                font_manager,
                time_ms,
                play_res_x,
                play_res_y,
                video_width,
                video_height,
                script_wrap_style,
            );
            buffer.blend_buffer(&event_buffer);
        } else {
            self.composite_event_inner(
                buffer,
                event,
                resolved,
                font_manager,
                time_ms,
                play_res_x,
                play_res_y,
                video_width,
                video_height,
                script_wrap_style,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn composite_event_inner(
        &mut self,
        buffer: &mut RenderBuffer,
        event: &Event,
        resolved: &ResolvedStyle,
        font_manager: &FontManager,
        time_ms: u64,
        play_res_x: u32,
        play_res_y: u32,
        video_width: u32,
        video_height: u32,
        script_wrap_style: i32,
    ) {
        if event.event_type == EventType::Comment {
            return;
        }

        let start_ms = event.start.to_millis();
        let end_ms = event.end.to_millis();

        if time_ms < start_ms || time_ms >= end_ms {
            return;
        }

        // Calculate global alpha (fade effects)
        let mut alpha_mult = 1.0_f64;

        if resolved.fade_in > 0 || resolved.fade_out > 0 {
            let fade_alpha = effects::calculate_fade_alpha(
                time_ms,
                start_ms,
                end_ms,
                resolved.fade_in,
                resolved.fade_out,
            );
            alpha_mult = fade_alpha as f64 / 255.0;
        }

        if let Some(ref cf) = resolved.complex_fade {
            let elapsed = time_ms - start_ms;
            alpha_mult = if elapsed < cf.t1 {
                cf.a1 as f64 / 255.0
            } else if elapsed < cf.t2 {
                let t = (elapsed - cf.t1) as f64 / (cf.t2 - cf.t1) as f64;
                cf.a1 as f64 / 255.0 + t * (cf.a2 as f64 / 255.0 - cf.a1 as f64 / 255.0)
            } else if elapsed < cf.t3 {
                cf.a2 as f64 / 255.0
            } else if elapsed < cf.t4 {
                let t = (elapsed - cf.t3) as f64 / (cf.t4 - cf.t3) as f64;
                cf.a2 as f64 / 255.0 + t * (cf.a3 as f64 / 255.0 - cf.a2 as f64 / 255.0)
            } else {
                cf.a3 as f64 / 255.0
            };
        }

        if alpha_mult <= 0.0 {
            return;
        }

        let alpha = (alpha_mult * 255.0) as u8;

        // Find font
        let font = font_manager.find_font(&resolved.font_name, resolved.bold, resolved.italic);
        let font_size = resolved.font_size * (video_height as f64 / play_res_y as f64);

        let scale_x = video_width as f64 / play_res_x as f64;
        let scale_y = video_height as f64 / play_res_y as f64;

        // Automatic word wrapping (wrap styles 0-3). A per-event \q
        // overrides the script-level WrapStyle. Drawing mode text is
        // coordinate data and must never be wrapped.
        let is_drawing_event = resolved.drawing_mode > 0
            || event
                .parsed_tags
                .iter()
                .any(|t| matches!(t, OverrideTag::Drawing(d) if *d > 0));
        let wrapped_text = if is_drawing_event {
            event.text.clone()
        } else {
            let wrap_style = event
                .parsed_tags
                .iter()
                .rev()
                .find_map(|tag| match tag {
                    OverrideTag::WrapStyle(q) => Some(*q),
                    _ => None,
                })
                .unwrap_or(script_wrap_style);
            let wrap_width =
                (play_res_x as f64 - resolved.margin_l as f64 - resolved.margin_r as f64) * scale_x;
            wrap_event_text(
                &event.text,
                wrap_style,
                wrap_width,
                font,
                font_size,
                resolved.spacing,
            )
        };

        // Parse text into segments for per-override rendering
        let segments = parse_text_segments(&wrapped_text);

        // Check if this is drawing mode (check first segment for \p1)
        let is_drawing = segments
            .first()
            .map(|s| {
                resolved.drawing_mode > 0
                    || s.tags
                        .iter()
                        .any(|t| matches!(t, OverrideTag::Drawing(d) if *d > 0))
            })
            .unwrap_or(false);

        // Calculate position (always need text measurement for alignment offsets)
        let (clean_text, _) = self.extract_clean_text(&wrapped_text);
        let shaped_full = TextShaper::shape(
            &clean_text,
            font,
            font_size,
            resolved.scale_x / 100.0,
            resolved.scale_y / 100.0,
            resolved.bold,
            resolved.italic,
            resolved.spacing,
            resolved.color,
            resolved.outline_color,
            resolved.shadow_color,
            resolved.angle,
        );

        let (mut base_x, mut base_y) = Self::calculate_position(
            resolved,
            shaped_full.width,
            shaped_full.height,
            shaped_full.baseline,
            play_res_x,
            play_res_y,
            video_width,
            video_height,
        );

        // Apply move animation
        if let Some(ref move_data) = resolved.move_data {
            let elapsed = time_ms - start_ms;
            let duration = end_ms - start_ms;

            let t = if move_data.t1 == move_data.t2 {
                if duration > 0 {
                    (elapsed as f64 / duration as f64).min(1.0)
                } else {
                    0.0
                }
            } else {
                let move_start = move_data.t1.min(duration);
                let move_end = move_data.t2.min(duration);
                let move_duration = move_end - move_start;

                if elapsed < move_start {
                    0.0
                } else if elapsed >= move_end {
                    1.0
                } else if move_duration > 0 {
                    (elapsed - move_start) as f64 / move_duration as f64
                } else {
                    0.0
                }
            };

            let anchor_x = (move_data.x1 + (move_data.x2 - move_data.x1) * t) * scale_x;
            let anchor_y = (move_data.y1 + (move_data.y2 - move_data.y1) * t) * scale_y;
            (base_x, base_y) = Self::anchor_to_origin(
                resolved.alignment,
                anchor_x,
                anchor_y,
                shaped_full.width,
                shaped_full.height,
            );
        }

        // Rotation origin for 3D effects. The default origin follows a move;
        // an explicit \org remains fixed in script coordinates.
        let (org_x, org_y) = if let Some((ox, oy)) = resolved.origin {
            (ox * scale_x, oy * scale_y)
        } else {
            let ax = match resolved.alignment {
                1 | 4 | 7 => base_x,
                2 | 5 | 8 => base_x + shaped_full.width / 2.0,
                3 | 6 | 9 => base_x + shaped_full.width,
                _ => base_x + shaped_full.width / 2.0,
            };
            let ay = match resolved.alignment {
                7..=9 => base_y,
                4..=6 => base_y + shaped_full.height / 2.0,
                1..=3 => base_y + shaped_full.height,
                _ => base_y + shaped_full.height / 2.0,
            };
            (ax, ay)
        };

        // Border style 3 is an opaque box behind the event text.
        if resolved.border_style == 3 {
            let box_color = resolved.back_color.to_rgba();
            effects::apply_opaque_box(
                buffer,
                base_x as i32,
                base_y as i32,
                shaped_full.width.ceil() as i32,
                shaped_full.height.ceil() as i32,
                (resolved.margin_l as f64 * scale_x).round() as i32,
                (resolved.margin_r as f64 * scale_x).round() as i32,
                (resolved.margin_v as f64 * scale_y).round() as i32,
                [
                    box_color[0],
                    box_color[1],
                    box_color[2],
                    ((255 - box_color[3]) as f64 * alpha_mult) as u8,
                ],
                play_res_x,
                play_res_y,
            );
        }

        // Drawing mode: render vector paths
        if is_drawing {
            let drawing_mode = segments
                .first()
                .and_then(|segment| {
                    segment.tags.iter().find_map(|tag| match tag {
                        OverrideTag::Drawing(mode) if *mode > 0 => Some(*mode),
                        _ => None,
                    })
                })
                .unwrap_or(resolved.drawing_mode);
            let drawing_scale = if drawing_mode > 0 {
                2.0_f64.powi(drawing_mode.saturating_sub(1))
            } else {
                1.0
            };
            let avg_scale = (scale_x + scale_y) / 2.0 * drawing_scale;
            let color = resolved.color.to_rgba();
            let color_alpha = 255 - color[3];

            super::drawing::DrawingParser::render_drawing(
                buffer,
                &clean_text,
                base_x,
                base_y,
                avg_scale,
                [
                    color[0],
                    color[1],
                    color[2],
                    (color_alpha as f64 * alpha_mult) as u8,
                ],
            );
            return;
        }

        // Karaoke syllable timeline (empty when the event has no karaoke tags)
        let (karaoke_syllables, seg_syllable) =
            build_karaoke_timeline(&segments, font, font_size, resolved.spacing);
        let mut syllable_consumed: Vec<f64> = vec![0.0; karaoke_syllables.len()];
        let elapsed_ms = time_ms.saturating_sub(start_ms);

        // Per-segment rendering
        let mut x_offset = 0.0_f64;
        let mut line_y_offset = 0.0_f64;

        for (seg_idx, segment) in segments.iter().enumerate() {
            if segment.text.is_empty() {
                continue;
            }

            // Resolve segment style incrementally from parent (no heap allocation)
            let mut segment_resolved = resolved.clone();
            // Apply segment-level overrides (skip Transform tags)
            for tag in &segment.tags {
                if let OverrideTag::Transform { .. } = tag {
                    continue;
                }
                if matches!(tag, OverrideTag::Reset) {
                    let position = segment_resolved.position;
                    let origin = segment_resolved.origin;
                    let move_data = segment_resolved.move_data.clone();
                    let clip = segment_resolved.clip;
                    let inverse_clip = segment_resolved.inverse_clip;
                    let fade_in = segment_resolved.fade_in;
                    let fade_out = segment_resolved.fade_out;
                    let complex_fade = segment_resolved.complex_fade.clone();
                    let drawing_mode = segment_resolved.drawing_mode;
                    segment_resolved = Self::resolve_base_style(&segment_resolved.base_style, &[]);
                    segment_resolved.position = position;
                    segment_resolved.origin = origin;
                    segment_resolved.move_data = move_data;
                    segment_resolved.clip = clip;
                    segment_resolved.inverse_clip = inverse_clip;
                    segment_resolved.fade_in = fade_in;
                    segment_resolved.fade_out = fade_out;
                    segment_resolved.complex_fade = complex_fade;
                    segment_resolved.drawing_mode = drawing_mode;
                    continue;
                }
                Self::apply_single_tag(&mut segment_resolved, tag);
            }

            // Apply event-level margin overrides
            if event.margin_l != 0 {
                segment_resolved.margin_l = event.margin_l;
            }
            if event.margin_r != 0 {
                segment_resolved.margin_r = event.margin_r;
            }
            if event.margin_v != 0 {
                segment_resolved.margin_v = event.margin_v;
            }

            // Apply \t animations - iterate all segments' transform tags directly
            // (avoids allocating a Vec per frame)
            for tag in &segment.tags {
                if let OverrideTag::Transform {
                    t1,
                    t2,
                    accel,
                    tags,
                } = tag
                {
                    let elapsed = time_ms.saturating_sub(start_ms);
                    if elapsed < *t1 {
                        continue;
                    }
                    // t2 == 0 means "until the end of the event" (the spec
                    // default when \t omits its timing arguments)
                    let t2_eff = if *t2 == 0 {
                        end_ms.saturating_sub(start_ms)
                    } else {
                        *t2
                    };
                    let duration = t2_eff.saturating_sub(*t1);
                    let raw_progress = if elapsed >= t2_eff {
                        1.0
                    } else if duration > 0 {
                        (elapsed - *t1) as f64 / duration as f64
                    } else {
                        1.0
                    };
                    let progress = Self::apply_accel(raw_progress, *accel);
                    Self::apply_transform_tags(&mut segment_resolved, tags, progress);
                }
            }

            // Apply karaoke highlighting for this segment's syllable.
            // sweep_boundary holds the sweep edge in segment-local x
            // coordinates when the syllable uses \K / \kf.
            let mut sweep_boundary: Option<f64> = None;
            if let Some(syl_idx) = seg_syllable[seg_idx] {
                let syl = &karaoke_syllables[syl_idx];
                let sung = elapsed_ms >= syl.start_ms;
                match syl.kind {
                    KaraokeKind::Hard => {
                        if !sung {
                            segment_resolved.color = segment_resolved.secondary_color;
                        }
                    }
                    KaraokeKind::Outline => {
                        if sung {
                            // Hide the outline once the syllable has been sung
                            segment_resolved.outline_color.alpha = 255;
                        }
                    }
                    KaraokeKind::Sweep => {
                        let frac = if !sung {
                            0.0
                        } else if syl.dur_ms == 0
                            || elapsed_ms >= syl.start_ms.saturating_add(syl.dur_ms)
                        {
                            1.0
                        } else {
                            (elapsed_ms - syl.start_ms) as f64 / syl.dur_ms as f64
                        };
                        sweep_boundary = Some(frac * syl.width - syllable_consumed[syl_idx]);
                    }
                }
            }

            // Shape this segment's text
            let segment_font = font_manager.find_font(
                &segment_resolved.font_name,
                segment_resolved.bold,
                segment_resolved.italic,
            );

            let segment_font_size =
                segment_resolved.font_size * (video_height as f64 / play_res_y as f64);
            let shaped = TextShaper::shape(
                &segment.text,
                segment_font,
                segment_font_size,
                segment_resolved.scale_x / 100.0,
                segment_resolved.scale_y / 100.0,
                segment_resolved.bold,
                segment_resolved.italic,
                segment_resolved.spacing,
                segment_resolved.color,
                segment_resolved.outline_color,
                segment_resolved.shadow_color,
                segment_resolved.angle,
            );

            // Pre-compute effect parameters
            let outline_active =
                segment_resolved.border_style == 1 && segment_resolved.outline > 0.0;
            let outline_scale = segment_resolved.outline
                * (scale_x * segment_resolved.scale_x / 100.0
                    + scale_y * segment_resolved.scale_y / 100.0)
                / 2.0;
            let outline_color_rgba = segment_resolved.outline_color.to_rgba();
            let outline_alpha = (255 - outline_color_rgba[3] as u32) as u8;

            let shadow_active = segment_resolved.shadow > 0.0;
            let shadow_offset_x =
                segment_resolved.shadow * scale_x * segment_resolved.scale_x / 100.0;
            let shadow_offset_y =
                segment_resolved.shadow * scale_y * segment_resolved.scale_y / 100.0;
            let shadow_color_rgba = segment_resolved.shadow_color.to_rgba();
            let shadow_alpha = (255 - shadow_color_rgba[3] as u32) as u8;

            // Single pass over glyphs: cache lookup once, render outline + shadow + fill
            for glyph in &shaped.glyphs {
                if glyph.scale_x <= 0.0 || glyph.scale_y <= 0.0 {
                    continue;
                }

                let cached = self.glyph_cache.get_or_rasterize(
                    segment_font,
                    glyph.glyph_id,
                    segment_font_size,
                    glyph.bold,
                    glyph.italic,
                );

                if cached.width == 0 || cached.height == 0 {
                    continue;
                }

                let scaled_bitmap = if (glyph.scale_x - 1.0).abs() < f64::EPSILON
                    && (glyph.scale_y - 1.0).abs() < f64::EPSILON
                {
                    Cow::Borrowed(cached.bitmap.as_slice())
                } else {
                    Cow::Owned(
                        RenderBuffer::resize_coverage_bitmap(
                            &cached.bitmap,
                            cached.width,
                            cached.height,
                            glyph.scale_x,
                            glyph.scale_y,
                        )
                        .0,
                    )
                };
                let glyph_width = ((cached.width as f64 * glyph.scale_x).round() as u32).max(1);
                let glyph_height = ((cached.height as f64 * glyph.scale_y).round() as u32).max(1);
                let bearing_x = cached.bearing_x as f64 * glyph.scale_x;
                let bearing_y = cached.bearing_y as f64 * glyph.scale_y;

                // Calculate original center of the glyph relative to origin
                let orig_cx = base_x + x_offset + glyph.x + bearing_x + glyph_width as f64 / 2.0;
                let orig_cy =
                    base_y + line_y_offset + glyph.y + bearing_y + glyph_height as f64 / 2.0;

                let dx = orig_cx - org_x;
                let dy = orig_cy - org_y;

                // Construction of exact 3D rotation matrix (Order: Z, then Y, then X)
                // ASS \frx and \fry rotations are negated compared to standard math
                let rz = segment_resolved.angle;
                let rx = segment_resolved.rotation_x;
                let ry = segment_resolved.rotation_y;

                let mat_z = Matrix3x3::rotation_z(rz.to_radians());
                let mat_y = Matrix3x3::rotation_y(-ry.to_radians());
                let mat_x = Matrix3x3::rotation_x(-rx.to_radians());

                let matrix = mat_x.multiply(&mat_y).multiply(&mat_z);

                // Perspective distance (standard ASS is ~312.5-500 depending on resolution)
                let perspective = 500.0 * (video_height as f64 / play_res_y as f64);

                // Use projective transform for exact perspective warping
                let (rot_bitmap, rot_w, rot_h, rot_ox, rot_oy) =
                    RenderBuffer::projective_transform_coverage_bitmap(
                        &scaled_bitmap,
                        glyph_width,
                        glyph_height,
                        &matrix,
                        perspective,
                    );
                // Calculate 3D position and perspective scale for the glyph center
                let (x3, y3, z3) = matrix.transform(dx, dy, 0.0);
                let scale_factor = perspective / (perspective + z3);
                let px = x3 * scale_factor;
                let py = y3 * scale_factor;

                // Final screen position
                let final_gx = (org_x + px + rot_ox as f64) as i32;
                let final_gy = (org_y + py + rot_oy as f64) as i32;

                // Adjust effect scales by perspective factor
                let current_outline_scale = outline_scale * scale_factor;
                let current_shadow_x = shadow_offset_x * scale_factor;
                let current_shadow_y = shadow_offset_y * scale_factor;

                // Render outline
                if outline_active {
                    effects::apply_outline(
                        buffer,
                        &rot_bitmap,
                        rot_w,
                        rot_h,
                        final_gx,
                        final_gy,
                        current_outline_scale,
                        [
                            outline_color_rgba[0],
                            outline_color_rgba[1],
                            outline_color_rgba[2],
                            (outline_alpha as f64 * alpha_mult) as u8,
                        ],
                    );
                }

                // Render shadow
                if shadow_active {
                    effects::apply_shadow(
                        buffer,
                        &rot_bitmap,
                        rot_w,
                        rot_h,
                        final_gx,
                        final_gy,
                        current_shadow_x,
                        current_shadow_y,
                        [
                            shadow_color_rgba[0],
                            shadow_color_rgba[1],
                            shadow_color_rgba[2],
                            (shadow_alpha as f64 * alpha_mult) as u8,
                        ],
                    );
                }

                // Render main text (karaoke \K/\kf sweeps color per glyph:
                // glyphs past the sweep edge keep the highlight color)
                let fill_color = match sweep_boundary {
                    Some(edge) if glyph.x + glyph.advance / 2.0 > edge => {
                        segment_resolved.secondary_color
                    }
                    _ => glyph.color,
                };
                let color = fill_color.to_rgba();
                let color_alpha = 255 - color[3];

                for py in 0..rot_h {
                    for px in 0..rot_w {
                        let coverage = rot_bitmap[(py * rot_w + px) as usize];
                        if coverage > 0 {
                            let a = ((coverage as u32 * color_alpha as u32 / 255) * alpha as u32
                                / 255) as u8;
                            buffer.blend_pixel(
                                (final_gx + px as i32) as u32,
                                (final_gy + py as i32) as u32,
                                color[0],
                                color[1],
                                color[2],
                                a,
                            );
                        }
                    }
                }

                // Render underline
                if segment_resolved.underline {
                    let line_width = 2;
                    let color = segment_resolved.color.to_rgba();
                    let color_alpha = 255 - color[3];
                    let ul_gy = (final_gy as f64 + shaped.baseline * 0.9) as i32;
                    buffer.fill_rect(
                        final_gx,
                        ul_gy,
                        rot_w as i32,
                        line_width,
                        color[0],
                        color[1],
                        color[2],
                        (color_alpha as f64 * alpha_mult) as u8,
                    );
                }

                // Render strikeout
                if segment_resolved.strike_out {
                    let line_width = 3;
                    let color = segment_resolved.color.to_rgba();
                    let color_alpha = 255 - color[3];
                    let so_gy = (final_gy as f64 + rot_h as f64 * 0.4) as i32;
                    buffer.fill_rect(
                        final_gx,
                        so_gy,
                        rot_w as i32,
                        line_width,
                        color[0],
                        color[1],
                        color[2],
                        (color_alpha as f64 * alpha_mult) as u8,
                    );
                }
            }

            // Track swept width within the syllable
            if let Some(syl_idx) = seg_syllable[seg_idx] {
                syllable_consumed[syl_idx] += shaped.width;
            }

            // Update offsets for next segment
            // Check if segment ends with line break
            if segment.text.ends_with('\n') {
                x_offset = 0.0;
                line_y_offset += shaped.height;
            } else {
                x_offset += shaped.width;
            }
        }

        // Apply clipping (after all segments rendered)
        if let Some(clip_rect) = resolved.clip {
            let scaled_clip = (
                (clip_rect.0 as f64 * scale_x) as i32,
                (clip_rect.1 as f64 * scale_y) as i32,
                (clip_rect.2 as f64 * scale_x) as i32,
                (clip_rect.3 as f64 * scale_y) as i32,
            );
            effects::apply_clip(buffer, scaled_clip);
        }

        if let Some(clip_rect) = resolved.inverse_clip {
            let scaled_clip = (
                (clip_rect.0 as f64 * scale_x) as i32,
                (clip_rect.1 as f64 * scale_y) as i32,
                (clip_rect.2 as f64 * scale_x) as i32,
                (clip_rect.3 as f64 * scale_y) as i32,
            );
            effects::apply_inverse_clip(buffer, scaled_clip);
        }

        // Apply blur
        if resolved.blur > 0.0 {
            effects::apply_blur(buffer, resolved.blur);
        }
    }

    /// Extract clean text from event text (remove override tags)
    /// Returns (clean_text, is_drawing_mode)
    fn extract_clean_text(&self, text: &str) -> (String, bool) {
        let mut result = String::new();
        let mut in_tag = false;
        let mut drawing_mode = false;
        let mut chars = text.chars().peekable();

        while let Some(ch) = chars.next() {
            match ch {
                '{' => in_tag = true,
                '}' => in_tag = false,
                '\\' => {
                    if let Some(&next) = chars.peek() {
                        match next {
                            'N' | 'n' => {
                                chars.next();
                                if !drawing_mode {
                                    result.push('\n');
                                } else {
                                    result.push('\\');
                                    result.push(next);
                                }
                            }
                            'h' => {
                                chars.next();
                                if !drawing_mode {
                                    result.push('\u{00A0}');
                                } else {
                                    result.push('\\');
                                    result.push(next);
                                }
                            }
                            'p' => {
                                chars.next();
                                if let Some(&level) = chars.peek() {
                                    if level == '1' {
                                        chars.next();
                                        drawing_mode = true;
                                    } else if level == '0' {
                                        chars.next();
                                        drawing_mode = false;
                                    }
                                }
                            }
                            _ if in_tag => {}
                            _ => {
                                result.push('\\');
                            }
                        }
                    }
                }
                _ if !in_tag || drawing_mode => result.push(ch),
                _ => {}
            }
        }

        (result, drawing_mode)
    }

    /// Clear the glyph cache
    pub fn clear_cache(&mut self) {
        self.glyph_cache.clear();
    }
}

impl Default for Compositor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::font;

    fn fallback_font() -> FontArc {
        let mut fm = FontManager::new();
        fm.load_font("DejaVu Sans", font::get_fallback_font(), false, false)
            .unwrap();
        fm.find_font("DejaVu Sans", false, false).clone()
    }

    #[test]
    fn test_wrap_style_2_disables_wrapping() {
        let font = fallback_font();
        let text = "aa aa aa aa";
        assert_eq!(wrap_event_text(text, 2, 1.0, &font, 48.0, 0.0), text);
    }

    #[test]
    fn test_wrap_greedy_top_wider() {
        let font = fallback_font();
        let word_w = TextShaper::measure_text("aa", &font, 48.0, 0.0);
        let space_w = TextShaper::measure_text(" ", &font, 48.0, 0.0);
        // Room for exactly three words per line
        let max = word_w * 3.0 + space_w * 2.0 + 0.5;
        let out = wrap_event_text("aa aa aa aa", 0, max, &font, 48.0, 0.0);
        assert_eq!(out, "aa aa aa\naa");
    }

    #[test]
    fn test_wrap_style_3_bottom_wider() {
        let font = fallback_font();
        let word_w = TextShaper::measure_text("aa", &font, 48.0, 0.0);
        let space_w = TextShaper::measure_text(" ", &font, 48.0, 0.0);
        let max = word_w * 3.0 + space_w * 2.0 + 0.5;
        let out = wrap_event_text("aa aa aa aa", 3, max, &font, 48.0, 0.0);
        assert_eq!(out, "aa\naa aa aa");
    }

    #[test]
    fn test_wrap_preserves_tags_and_hard_breaks() {
        let font = fallback_font();
        let word_w = TextShaper::measure_text("aa", &font, 48.0, 0.0);
        let space_w = TextShaper::measure_text(" ", &font, 48.0, 0.0);
        let max = word_w + space_w + 0.5; // only one word fits per line
        let out = wrap_event_text("{\\c&H00FF00&}aa aa\\Naa", 0, max, &font, 48.0, 0.0);
        // Tag group survives, wrapping occurs, and the explicit break is kept
        assert_eq!(out, "{\\c&H00FF00&}aa\naa\naa");
    }

    #[test]
    fn test_wrap_no_spaces_unchanged() {
        let font = fallback_font();
        let out = wrap_event_text("aaaaaaaa", 0, 5.0, &font, 48.0, 0.0);
        assert_eq!(out, "aaaaaaaa");
    }

    #[test]
    fn test_wrap_karaoke_no_phantom_spaces() {
        let font = fallback_font();
        // Karaoke text split by tag groups: {\k80}Hel{\k60}lo {\k100}world!
        // "Hel" and "lo" are adjacent (no space) — must NOT get a space inserted.
        let text = "{\\k80}Hel{\\k60}lo {\\k100}world!";
        // Use a huge max_width so no wrapping occurs — we only test space preservation.
        let out = wrap_event_text(text, 0, f64::MAX, &font, 48.0, 0.0);
        assert_eq!(out, "{\\k80}Hel{\\k60}lo {\\k100}world!");
    }

    #[test]
    fn test_wrap_karaoke_with_real_spaces() {
        let font = fallback_font();
        // "line " has a trailing space before the next tag group.
        let text = "{\\k80}Out{\\k70}line {\\k80}dis{\\k60}ap{\\k70}pears";
        let out = wrap_event_text(text, 0, f64::MAX, &font, 48.0, 0.0);
        // The space between "line" and "dis" must be preserved; no space between "Out"|"line".
        assert_eq!(out, "{\\k80}Out{\\k70}line {\\k80}dis{\\k60}ap{\\k70}pears");
    }

    #[test]
    fn test_wrap_inline_tag_preserves_spaces() {
        let font = fallback_font();
        // Inline tags like {\b0} between words must not eat the space.
        let text = "{\\b1}Bold{\\b0} {\\i1}Italic{\\i0} {\\u1}Under{\\u0}";
        let out = wrap_event_text(text, 0, f64::MAX, &font, 48.0, 0.0);
        assert_eq!(out, "{\\b1}Bold{\\b0} {\\i1}Italic{\\i0} {\\u1}Under{\\u0}");
    }

    #[test]
    fn test_karaoke_timeline_hard_tags() {
        let segments = parse_text_segments("{\\k50}A{\\k30}B");
        let font = fallback_font();
        let (syllables, map) = build_karaoke_timeline(&segments, &font, 48.0, 0.0);

        assert_eq!(syllables.len(), 2);
        assert_eq!(syllables[0].start_ms, 0);
        assert_eq!(syllables[0].dur_ms, 500);
        assert_eq!(syllables[0].kind, KaraokeKind::Hard);
        assert_eq!(syllables[1].start_ms, 500);
        assert_eq!(syllables[1].dur_ms, 300);
        assert_eq!(map, vec![Some(0), Some(1)]);
        assert!(syllables[0].width > 0.0);
        assert!(syllables[1].width > 0.0);
    }

    #[test]
    fn test_karaoke_timeline_leading_text_ignored() {
        let segments = parse_text_segments("pre{\\kf40}X");
        let font = fallback_font();
        let (syllables, map) = build_karaoke_timeline(&segments, &font, 48.0, 0.0);

        assert_eq!(syllables.len(), 1);
        assert_eq!(syllables[0].kind, KaraokeKind::Sweep);
        assert_eq!(syllables[0].dur_ms, 400);
        assert_eq!(map, vec![None, Some(0)]);
    }

    #[test]
    fn test_karaoke_timeline_continuation_segments() {
        // A non-karaoke tag mid-syllable must not start a new syllable
        let segments = parse_text_segments("{\\k50}A{\\c&H00FF00&}B");
        let font = fallback_font();
        let (syllables, map) = build_karaoke_timeline(&segments, &font, 48.0, 0.0);

        assert_eq!(syllables.len(), 1);
        assert_eq!(map, vec![Some(0), Some(0)]);
        let combined = TextShaper::measure_text("A", &font, 48.0, 0.0)
            + TextShaper::measure_text("B", &font, 48.0, 0.0);
        assert!((syllables[0].width - combined).abs() < 1e-6);
    }

    #[test]
    fn test_karaoke_outline_kind() {
        let segments = parse_text_segments("{\\ko20}A");
        let font = fallback_font();
        let (syllables, _) = build_karaoke_timeline(&segments, &font, 48.0, 0.0);

        assert_eq!(syllables.len(), 1);
        assert_eq!(syllables[0].kind, KaraokeKind::Outline);
        assert_eq!(syllables[0].dur_ms, 200);
    }

    #[test]
    fn test_karaoke_render_changes_over_time() {
        let mut comp = Compositor::new();
        let mut fm = FontManager::new();
        fm.load_font("DejaVu Sans", font::get_fallback_font(), false, false)
            .unwrap();

        let event = Event::parse_from_line(
            "Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{\\k100}A{\\k100}B",
        )
        .unwrap();
        let style = Style::new("Default");
        let resolved = Compositor::resolve_style(&style, &event);

        // At 100ms the second syllable is still highlighted (secondary
        // color); at 1500ms both syllables have been sung (primary color).
        let mut early = RenderBuffer::new(320, 100);
        comp.composite_event(
            &mut early, &event, &resolved, &fm, 100, 320, 100, 320, 100, 0,
        );

        let mut late = RenderBuffer::new(320, 100);
        comp.composite_event(
            &mut late, &event, &resolved, &fm, 1500, 320, 100, 320, 100, 0,
        );

        assert_ne!(early.as_bytes(), late.as_bytes());
    }
}
