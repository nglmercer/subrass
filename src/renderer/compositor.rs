use super::buffer::RenderBuffer;
use super::effects;
use super::font::FontManager;
use super::glyph_cache::GlyphCache;
use super::shaper::TextShaper;
use crate::types::color::Color;
use crate::types::override_tag::{parse_text_segments, parse_text_segments_with_wrap, TextSegment};
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
    pub outline_x: f64,
    pub outline_y: f64,
    pub shadow: f64,
    pub shadow_x: f64,
    pub shadow_y: f64,
    pub shear_x: f64,
    pub shear_y: f64,
    pub alignment: i32,
    pub margin_l: i32,
    pub margin_r: i32,
    pub margin_v: i32,
    pub position: Option<(f64, f64)>,
    pub origin: Option<(f64, f64)>,
    pub move_data: Option<MoveData>,
    pub clip: Option<(i32, i32, i32, i32)>,
    pub inverse_clip: Option<(i32, i32, i32, i32)>,
    pub clip_vector: Option<VectorClip>,
    pub inverse_clip_vector: Option<VectorClip>,
    pub fade_in: u64,
    pub fade_out: u64,
    pub complex_fade: Option<ComplexFade>,
    pub drawing_mode: i32,
    pub drawing_baseline_offset: f64,
    pub blur: f64,
    /// Script `ScaledBorderAndShadow` flag: when true (default), borders
    /// and shadows scale with the script-to-video resolution ratio.
    pub scaled_border_and_shadow: bool,
}

/// Vector clip shape in script coordinates with a drawing scale.
#[derive(Debug, Clone)]
pub struct VectorClip {
    pub scale: i32,
    pub drawing: String,
}

/// Line-global state preserved across `\r` resets.
struct LineGlobalKeep {
    position: Option<(f64, f64)>,
    origin: Option<(f64, f64)>,
    move_data: Option<MoveData>,
    clip: Option<(i32, i32, i32, i32)>,
    inverse_clip: Option<(i32, i32, i32, i32)>,
    clip_vector: Option<VectorClip>,
    inverse_clip_vector: Option<VectorClip>,
    fade_in: u64,
    fade_out: u64,
    complex_fade: Option<ComplexFade>,
    drawing_mode: i32,
}

impl LineGlobalKeep {
    fn capture(resolved: &ResolvedStyle) -> Self {
        Self {
            position: resolved.position,
            origin: resolved.origin,
            move_data: resolved.move_data.clone(),
            clip: resolved.clip,
            inverse_clip: resolved.inverse_clip,
            clip_vector: resolved.clip_vector.clone(),
            inverse_clip_vector: resolved.inverse_clip_vector.clone(),
            fade_in: resolved.fade_in,
            fade_out: resolved.fade_out,
            complex_fade: resolved.complex_fade.clone(),
            drawing_mode: resolved.drawing_mode,
        }
    }

    fn restore(self, resolved: &mut ResolvedStyle) {
        resolved.position = self.position;
        resolved.origin = self.origin;
        resolved.move_data = self.move_data;
        resolved.clip = self.clip;
        resolved.inverse_clip = self.inverse_clip;
        resolved.clip_vector = self.clip_vector;
        resolved.inverse_clip_vector = self.inverse_clip_vector;
        resolved.fade_in = self.fade_in;
        resolved.fade_out = self.fade_out;
        resolved.complex_fade = self.complex_fade;
        resolved.drawing_mode = self.drawing_mode;
    }
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
/// 0 = smart wrapping (balanced lines, top line widest),
/// 1 = end-of-line greedy wrapping, 2 = no automatic wrapping,
/// 3 = smart wrapping from the bottom (bottom line widest).
///
/// Tag groups are opaque and travel with the word that follows them;
/// explicit `\N` breaks (and `\n` in mode 2) split the text into
/// independently wrapped runs. `\n` elsewhere acts as a space. Drawing
/// runs pass through verbatim and are never wrapped. Words wider than
/// `max_width` stay on their own line (no character-level splitting).
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

    /// Scan a `{...}` group for a `\pN` drawing-mode switch.
    fn drawing_mode_in_group(group: &str) -> Option<i32> {
        let mut rest = group;
        while let Some(pos) = rest.find("\\p") {
            rest = &rest[pos + 2..];
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() {
                if let Ok(mode) = digits.parse::<i32>() {
                    return Some(mode);
                }
            }
        }
        None
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
    let mut in_drawing = false;
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                let mut group = String::from("{");
                for gc in chars.by_ref() {
                    group.push(gc);
                    if gc == '}' {
                        break;
                    }
                }
                if in_drawing {
                    // Verbatim: drawing runs are never wrapped or split.
                    word.push_str(&group);
                } else {
                    flush(&mut prefix, &mut word, &mut words, &mut preceded_by_space);
                    prefix.push_str(&group);
                }
                if let Some(mode) = drawing_mode_in_group(&group) {
                    in_drawing = mode > 0;
                }
            }
            '\\' if in_drawing => {
                word.push('\\');
                if let Some(&n) = chars.peek() {
                    word.push(n);
                    chars.next();
                }
            }
            '\\' => match chars.peek() {
                Some('N') => {
                    chars.next();
                    flush(&mut prefix, &mut word, &mut words, &mut preceded_by_space);
                    preceded_by_space = true;
                    run_starts.push(words.len());
                }
                Some('n') => {
                    chars.next();
                    // Soft break: a hard break only in mode 2, else a space.
                    if wrap_style == 2 {
                        flush(&mut prefix, &mut word, &mut words, &mut preceded_by_space);
                        preceded_by_space = true;
                        run_starts.push(words.len());
                    } else {
                        flush(&mut prefix, &mut word, &mut words, &mut preceded_by_space);
                        preceded_by_space = true;
                    }
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
                if in_drawing {
                    word.push(c);
                } else {
                    flush(&mut prefix, &mut word, &mut words, &mut preceded_by_space);
                    preceded_by_space = true;
                }
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

    // Wrap each explicit-line run independently, then rejoin with breaks.
    let mut out = String::with_capacity(text.len() + 16);
    let mut run_begin = 0usize;
    for start in run_starts.iter().skip(1) {
        render_wrapped_run(
            &words[run_begin..*start],
            wrap_style,
            max_width,
            space_width,
            &mut out,
        );
        out.push('\n');
        run_begin = *start;
    }
    render_wrapped_run(
        &words[run_begin..],
        wrap_style,
        max_width,
        space_width,
        &mut out,
    );

    out
}

/// Greedy line grouping. Forward fill makes the top line the widest;
/// filling from the end (style 3) makes the bottom the widest.
/// Prefix-only entries ride along without consuming width budget.
fn greedy_wrap_lines(
    words: &[WrapWord],
    bottom_wider: bool,
    max_width: f64,
    space_width: f64,
) -> Vec<Vec<usize>> {
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
    lines
}

/// Smart line grouping (wrap style 0): choose breaks to minimize total
/// raggedness (squared leftover per line, last line free), which balances
/// lines with the top line widest. Overlong single words keep their own
/// line. Dynamic program over word count — subtitle runs are short.
fn smart_wrap_lines(words: &[WrapWord], max_width: f64, space_width: f64) -> Vec<Vec<usize>> {
    let n = words.len();
    // Width of words[i..j] as one line (prefix-only entries are free).
    let span_width = |i: usize, j: usize| -> f64 {
        let mut w = 0.0_f64;
        let mut text_words = 0usize;
        for word in &words[i..j] {
            if word.text.is_empty() && word.width == 0.0 {
                continue;
            }
            if text_words > 0 {
                w += space_width;
            }
            w += word.width;
            text_words += 1;
        }
        w
    };
    // dp[j] = (min cost for words[0..j], best break point)
    let mut dp: Vec<(f64, usize)> = vec![(f64::INFINITY, 0); n + 1];
    dp[0] = (0.0, 0);
    for j in 1..=n {
        for i in 0..j {
            if dp[i].0.is_infinite() {
                continue;
            }
            let w = span_width(i, j);
            let is_last = j == n;
            let cost = if w <= max_width {
                // Last line is free: ragged bottom is expected.
                if is_last {
                    dp[i].0
                } else {
                    let slack = max_width - w;
                    dp[i].0 + slack * slack
                }
            } else if j - i == 1 {
                // Single overlong word: allowed, penalized by overflow.
                let over = w - max_width;
                dp[i].0 + over * over + 1e12
            } else {
                continue;
            };
            // Strictly-less keeps the earliest (top-widest) break on ties.
            if cost < dp[j].0 {
                dp[j] = (cost, i);
            }
        }
    }
    // Fall back to greedy if nothing fit (should not happen: single
    // words always fit), then reconstruct line breaks.
    if dp[n].0.is_infinite() {
        return greedy_wrap_lines(words, false, max_width, space_width);
    }
    let mut breaks = vec![n];
    let mut j = n;
    while j > 0 {
        let i = dp[j].1;
        breaks.push(i);
        if i >= j {
            break;
        }
        j = i;
    }
    breaks.reverse();
    breaks
        .windows(2)
        .map(|w| (w[0]..w[1]).collect::<Vec<usize>>())
        .filter(|line| !line.is_empty())
        .collect()
}

/// Wrap one run of words and append the result to `out`.
fn render_wrapped_run(
    words: &[WrapWord],
    wrap_style: i32,
    max_width: f64,
    space_width: f64,
    out: &mut String,
) {
    if words.is_empty() {
        return;
    }

    // Style 0 balances lines (top widest); 1 fills greedily from the top;
    // 3 fills greedily from the bottom (bottom widest).
    let lines: Vec<Vec<usize>> = if wrap_style == 0 {
        smart_wrap_lines(words, max_width, space_width)
    } else {
        greedy_wrap_lines(words, wrap_style == 3, max_width, space_width)
    };

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

/// Opacity (0.0 = invisible, 1.0 = fully visible) for
/// `\fade(a1,a2,a3,t1,t2,t3,t4)` at `elapsed` ms into the event.
///
/// ASS alpha is transparency (0 = visible, 255 = transparent), so each
/// phase value converts via `opacity = 1 - alpha/255`. Zero-length ramps
/// are instant jumps; inverted ranges saturate instead of underflowing.
fn complex_fade_opacity(cf: &ComplexFade, elapsed: u64) -> f64 {
    fn lerp(a: u8, b: u8, num: u64, denom: u64) -> f64 {
        if denom == 0 {
            return b as f64;
        }
        a as f64 + (num as f64 / denom as f64) * (b as f64 - a as f64)
    }
    let ass_transparency = if elapsed < cf.t1 {
        cf.a1 as f64
    } else if elapsed < cf.t2 {
        lerp(cf.a1, cf.a2, elapsed - cf.t1, cf.t2.saturating_sub(cf.t1))
    } else if elapsed < cf.t3 {
        cf.a2 as f64
    } else if elapsed < cf.t4 {
        lerp(cf.a2, cf.a3, elapsed - cf.t3, cf.t4.saturating_sub(cf.t3))
    } else {
        cf.a3 as f64
    };
    (1.0 - ass_transparency / 255.0).clamp(0.0, 1.0)
}

/// A karaoke syllable: timing relative to event start plus measured width
#[derive(Debug, Clone)]
struct KaraokeSyllable {
    start_ms: u64,
    dur_ms: u64,
    kind: KaraokeKind,
    width: f64,
}

/// `\k` fill rule: secondary before the syllable starts, primary from the
/// exact start instant (not at the end).
fn karaoke_is_primary(syl: &KaraokeSyllable, elapsed_ms: u64) -> bool {
    debug_assert_eq!(syl.kind, KaraokeKind::Hard);
    elapsed_ms >= syl.start_ms
}

/// `\ko` outline rule: the outline is suppressed once the syllable begins.
fn karaoke_outline_suppressed(elapsed_ms: u64, start_ms: u64) -> bool {
    elapsed_ms >= start_ms
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

/// Measured vector drawing for one segment, in video pixels.
#[derive(Debug, Clone)]
struct DrawingLayout {
    mode: i32,
    min_x: f64,
    min_y: f64,
    width: f64,
    height: f64,
}

/// One laid-out segment: resolved style, shaped glyphs, font identity,
/// and optional drawing geometry.
struct LayoutItem {
    resolved: ResolvedStyle,
    shaped: crate::renderer::shaper::ShapedLine,
    font_id: usize,
    faux_bold: bool,
    faux_italic: bool,
    drawing: Option<DrawingLayout>,
    skipped: bool,
}

/// Whole-event layout: per-segment items plus block metrics.
struct LayoutBlock {
    items: Vec<LayoutItem>,
    width: f64,
    height: f64,
    baseline: f64,
}

/// Video pixels per drawing unit for a `\pN` mode: higher modes pack
/// more units per script pixel, so each unit renders smaller.
fn drawing_unit_scale(scale_x: f64, scale_y: f64, mode: i32) -> f64 {
    let res = (scale_x + scale_y) / 2.0;
    if mode > 0 {
        res / 2f64.powi(mode.saturating_sub(1).min(20))
    } else {
        res
    }
}

/// Scale a script-coordinate clip rectangle to video pixels,
/// saturating instead of overflowing on extreme coordinates.
fn scale_clip_rect(rect: (i32, i32, i32, i32), scale_x: f64, scale_y: f64) -> (i32, i32, i32, i32) {
    let conv = |v: i32, s: f64| -> i32 {
        if !s.is_finite() {
            return v;
        }
        (v as f64 * s).clamp(i32::MIN as f64, i32::MAX as f64) as i32
    };
    (
        conv(rect.0, scale_x),
        conv(rect.1, scale_y),
        conv(rect.2, scale_x),
        conv(rect.3, scale_y),
    )
}

/// Active drawing mode for a segment: the last `\pN` in its tags,
/// falling back to the event-level mode.
fn segment_drawing_mode(tags: &[OverrideTag], event_mode: i32) -> i32 {
    tags.iter()
        .rev()
        .find_map(|t| match t {
            OverrideTag::Drawing(m) => Some(*m),
            _ => None,
        })
        .unwrap_or(event_mode)
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

    /// Apply accel function: t' = t^accel (ASS semantics: accel = 1 is
    /// linear, accel > 1 starts slow and finishes fast, accel < 1 starts
    /// fast and finishes slow). Non-finite accel falls back to linear.
    fn apply_accel(t: f64, accel: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        if !accel.is_finite() || accel == 1.0 {
            t
        } else if accel <= 0.0 {
            // Degenerate: treat as instant jump at the end/start boundary.
            if t >= 1.0 {
                1.0
            } else {
                0.0
            }
        } else {
            t.powf(accel).clamp(0.0, 1.0)
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
                    let v = from + (b - from) * progress;
                    resolved.outline = v;
                    resolved.outline_x = v;
                    resolved.outline_y = v;
                }
                OverrideTag::Shadow(s) => {
                    let from = resolved.shadow;
                    let v = from + (s - from) * progress;
                    resolved.shadow = v;
                    resolved.shadow_x = v;
                    resolved.shadow_y = v;
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
                    resolved.back_color = resolved.shadow_color;
                }
                OverrideTag::Alpha(a) => {
                    let a = *a;
                    resolved.color.alpha = (resolved.color.alpha as f64
                        + (a as f64 - resolved.color.alpha as f64) * progress)
                        as u8;
                    resolved.secondary_color.alpha = (resolved.secondary_color.alpha as f64
                        + (a as f64 - resolved.secondary_color.alpha as f64) * progress)
                        as u8;
                    resolved.outline_color.alpha = (resolved.outline_color.alpha as f64
                        + (a as f64 - resolved.outline_color.alpha as f64) * progress)
                        as u8;
                    resolved.shadow_color.alpha = (resolved.shadow_color.alpha as f64
                        + (a as f64 - resolved.shadow_color.alpha as f64) * progress)
                        as u8;
                    resolved.back_color.alpha = resolved.shadow_color.alpha;
                }
                OverrideTag::PrimaryAlpha(a) => {
                    let from = resolved.color.alpha;
                    resolved.color.alpha =
                        (from as f64 + (*a as f64 - from as f64) * progress) as u8;
                }
                OverrideTag::SecondaryAlpha(a) => {
                    let from = resolved.secondary_color.alpha;
                    resolved.secondary_color.alpha =
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
                    resolved.back_color.alpha = resolved.shadow_color.alpha;
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
                    let from = resolved.outline_x;
                    resolved.outline_x = from + (b - from) * progress;
                    resolved.outline = (resolved.outline_x + resolved.outline_y) / 2.0;
                }
                OverrideTag::BorderY(b) => {
                    let from = resolved.outline_y;
                    resolved.outline_y = from + (b - from) * progress;
                    resolved.outline = (resolved.outline_x + resolved.outline_y) / 2.0;
                }
                OverrideTag::ShadowX(s) => {
                    let from = resolved.shadow_x;
                    resolved.shadow_x = from + (s - from) * progress;
                    resolved.shadow = (resolved.shadow_x + resolved.shadow_y) / 2.0;
                }
                OverrideTag::ShadowY(s) => {
                    let from = resolved.shadow_y;
                    resolved.shadow_y = from + (s - from) * progress;
                    resolved.shadow = (resolved.shadow_x + resolved.shadow_y) / 2.0;
                }
                OverrideTag::ShearX(s) => {
                    let from = resolved.shear_x;
                    resolved.shear_x = from + (s - from) * progress;
                }
                OverrideTag::ShearY(s) => {
                    let from = resolved.shear_y;
                    resolved.shear_y = from + (s - from) * progress;
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
            outline_x: base_style.outline,
            outline_y: base_style.outline,
            shadow: base_style.shadow,
            shadow_x: base_style.shadow,
            shadow_y: base_style.shadow,
            shear_x: 0.0,
            shear_y: 0.0,
            alignment: base_style.alignment,
            margin_l: base_style.margin_l,
            margin_r: base_style.margin_r,
            margin_v: base_style.margin_v,
            position: None,
            origin: None,
            move_data: None,
            clip: None,
            inverse_clip: None,
            clip_vector: None,
            inverse_clip_vector: None,
            fade_in: 0,
            fade_out: 0,
            complex_fade: None,
            drawing_mode: 0,
            drawing_baseline_offset: 0.0,
            blur: 0.0,
            scaled_border_and_shadow: true,
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
            OverrideTag::ShadowColor(c) => {
                // ASS calls this the BackColour channel. It is used both
                // for shadows and for the opaque box (BorderStyle 3).
                resolved.shadow_color = *c;
                resolved.back_color = *c;
            }
            OverrideTag::Alpha(a) => {
                // \alpha applies to all four ASS colour channels.
                resolved.color = resolved.color.with_alpha(*a);
                resolved.secondary_color = resolved.secondary_color.with_alpha(*a);
                resolved.outline_color = resolved.outline_color.with_alpha(*a);
                resolved.shadow_color = resolved.shadow_color.with_alpha(*a);
                resolved.back_color = resolved.back_color.with_alpha(*a);
            }
            OverrideTag::PrimaryAlpha(a) => {
                resolved.color = resolved.color.with_alpha(*a);
            }
            OverrideTag::SecondaryAlpha(a) => {
                resolved.secondary_color = resolved.secondary_color.with_alpha(*a);
            }
            OverrideTag::OutlineAlpha(a) => {
                resolved.outline_color = resolved.outline_color.with_alpha(*a);
            }
            OverrideTag::ShadowAlpha(a) => {
                resolved.shadow_color = resolved.shadow_color.with_alpha(*a);
                resolved.back_color = resolved.back_color.with_alpha(*a);
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
            OverrideTag::Border(b) => {
                resolved.outline = *b;
                resolved.outline_x = *b;
                resolved.outline_y = *b;
            }
            OverrideTag::BorderX(b) => {
                resolved.outline_x = *b;
                resolved.outline = (resolved.outline_x + resolved.outline_y) / 2.0;
            }
            OverrideTag::BorderY(b) => {
                resolved.outline_y = *b;
                resolved.outline = (resolved.outline_x + resolved.outline_y) / 2.0;
            }
            OverrideTag::Shadow(s) => {
                resolved.shadow = *s;
                resolved.shadow_x = *s;
                resolved.shadow_y = *s;
            }
            OverrideTag::ShadowX(s) => {
                resolved.shadow_x = *s;
                resolved.shadow = (resolved.shadow_x + resolved.shadow_y) / 2.0;
            }
            OverrideTag::ShadowY(s) => {
                resolved.shadow_y = *s;
                resolved.shadow = (resolved.shadow_x + resolved.shadow_y) / 2.0;
            }
            OverrideTag::ShearX(s) => resolved.shear_x = *s,
            OverrideTag::ShearY(s) => resolved.shear_y = *s,
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
                resolved.clip_vector = None;
            }
            OverrideTag::InverseClip(x1, y1, x2, y2) => {
                resolved.inverse_clip = Some((*x1, *y1, *x2, *y2));
                resolved.inverse_clip_vector = None;
            }
            OverrideTag::ClipVector { scale, drawing } => {
                resolved.clip_vector = Some(VectorClip {
                    scale: *scale,
                    drawing: drawing.clone(),
                });
                resolved.clip = None;
            }
            OverrideTag::InverseClipVector { scale, drawing } => {
                resolved.inverse_clip_vector = Some(VectorClip {
                    scale: *scale,
                    drawing: drawing.clone(),
                });
                resolved.inverse_clip = None;
            }
            OverrideTag::Blur(b) => resolved.blur = *b,
            OverrideTag::EdgeBlur(b) => resolved.blur = *b,
            OverrideTag::Drawing(mode) => resolved.drawing_mode = *mode,
            OverrideTag::DrawingBaseline(pbo) => resolved.drawing_baseline_offset = *pbo,
            _ => {}
        }
    }

    /// Resolve an event's style with all override tags applied.
    ///
    /// Segment style tags from the initial override groups establish the
    /// defaults, but line-global tags (\pos, \move, \org, \clip, \iclip,
    /// \fad, \fade) apply no matter where they appear textually: they are
    /// scanned across all segments (last one wins), so `{\pos(100,100)}Hi`
    /// and `Hi{\pos(100,100)}` resolve identically.
    pub fn resolve_style(base_style: &Style, event: &Event) -> ResolvedStyle {
        let segments = parse_text_segments(&event.text);
        let initial_tags = segments
            .first()
            .map(|segment| segment.tags.as_slice())
            .unwrap_or(&[]);
        let mut resolved = Self::resolve_base_style(base_style, initial_tags);

        // Line-global tags apply regardless of textual placement.
        // Segments carry accumulated tags, so only newly added tags per
        // segment are considered; later ones overwrite earlier ones.
        let mut prev_tag_count = 0usize;
        for segment in &segments {
            let from = prev_tag_count.min(segment.tags.len());
            prev_tag_count = segment.tags.len();
            for tag in &segment.tags[from..] {
                if tag.is_line_global() {
                    Self::apply_single_tag(&mut resolved, tag);
                }
            }
        }

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
            let (x, top) = Self::anchor_to_origin(
                resolved.alignment,
                scaled_x,
                scaled_y,
                text_width,
                text_height,
            );
            return (x, top + baseline);
        }

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

        let top = match alignment {
            7..=9 => margin_v,
            4..=6 => video_height as f64 / 2.0 - text_height / 2.0,
            1..=3 => video_height as f64 - margin_v - text_height,
            _ => video_height as f64 / 2.0 - text_height / 2.0,
        };

        (x, top + baseline)
    }

    /// Resolve one segment's style: base tags, `\r` resets against the
    /// style table, event margins, and `\t` animations at `time_ms`.
    #[allow(clippy::too_many_arguments)]
    fn resolve_segment_style(
        resolved: &ResolvedStyle,
        segment: &TextSegment,
        event: &Event,
        styles: &[Style],
        time_ms: u64,
        start_ms: u64,
        end_ms: u64,
    ) -> ResolvedStyle {
        let mut segment_resolved = resolved.clone();
        for tag in &segment.tags {
            if let OverrideTag::Transform { .. } = tag {
                continue;
            }
            if let OverrideTag::Reset(style_name) = tag {
                let base = match style_name {
                    Some(name) => styles
                        .iter()
                        .find(|s| s.name == *name)
                        .unwrap_or(&segment_resolved.base_style)
                        .clone(),
                    None => segment_resolved.base_style.clone(),
                };
                let keep = LineGlobalKeep::capture(&segment_resolved);
                segment_resolved = Self::resolve_base_style(&base, &[]);
                segment_resolved.scaled_border_and_shadow = resolved.scaled_border_and_shadow;
                keep.restore(&mut segment_resolved);
                continue;
            }
            Self::apply_single_tag(&mut segment_resolved, tag);
        }

        if event.margin_l != 0 {
            segment_resolved.margin_l = event.margin_l;
        }
        if event.margin_r != 0 {
            segment_resolved.margin_r = event.margin_r;
        }
        if event.margin_v != 0 {
            segment_resolved.margin_v = event.margin_v;
        }

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

        segment_resolved
    }

    /// Layout pass: resolve and measure every segment (text shaping or
    /// drawing bounds) and accumulate block metrics with the same
    /// line-break rules the render pass uses.
    #[allow(clippy::too_many_arguments)]
    fn layout_segments(
        segments: &[TextSegment],
        event: &Event,
        resolved: &ResolvedStyle,
        font_manager: &FontManager,
        time_ms: u64,
        start_ms: u64,
        end_ms: u64,
        play_res_x: u32,
        play_res_y: u32,
        video_width: u32,
        video_height: u32,
        styles: &[Style],
    ) -> LayoutBlock {
        let scale_x = video_width as f64 / play_res_x.max(1) as f64;
        let scale_y = video_height as f64 / play_res_y.max(1) as f64;
        let mut items = Vec::with_capacity(segments.len());

        for segment in segments {
            let skipped = segment.text.is_empty();
            let seg_resolved = Self::resolve_segment_style(
                resolved, segment, event, styles, time_ms, start_ms, end_ms,
            );
            let font_match = font_manager.find_font_with_match(
                &seg_resolved.font_name,
                seg_resolved.bold,
                seg_resolved.italic,
            );
            let seg_font_size =
                seg_resolved.font_size * (video_height as f64 / play_res_y.max(1) as f64);
            let shaped = TextShaper::shape(
                &segment.text,
                font_match.font,
                seg_font_size,
                seg_resolved.scale_x / 100.0,
                seg_resolved.scale_y / 100.0,
                seg_resolved.bold,
                seg_resolved.italic,
                seg_resolved.spacing,
                seg_resolved.color,
                seg_resolved.outline_color,
                seg_resolved.shadow_color,
                seg_resolved.angle,
            );
            // Per-segment drawing state (mixed drawing/text supported).
            let mode = segment_drawing_mode(&segment.tags, resolved.drawing_mode);
            let drawing = if !skipped && mode > 0 {
                let unit = drawing_unit_scale(scale_x, scale_y, mode);
                super::drawing::DrawingParser::measure(&segment.text).map(|(min_x, min_y, w, h)| {
                    DrawingLayout {
                        mode,
                        min_x: min_x * unit,
                        min_y: min_y * unit,
                        width: w * unit,
                        height: h * unit,
                    }
                })
            } else {
                None
            };
            items.push(LayoutItem {
                resolved: seg_resolved,
                shaped,
                font_id: font_match.id,
                faux_bold: font_match.faux_bold,
                faux_italic: font_match.faux_italic,
                drawing,
                skipped,
            });
        }

        // Accumulate block metrics with render-pass line rules: segments
        // ending in '\n' close the line; width is the widest line, height
        // the sum of line heights, baseline the first line's baseline.
        let mut block_width = 0.0_f64;
        let mut block_height = 0.0_f64;
        let mut baseline = 0.0_f64;
        let mut line_width = 0.0_f64;
        let mut line_height = 0.0_f64;
        let mut line_baseline = 0.0_f64;
        let mut first_line = true;
        let mut any_content = false;

        for (item, segment) in items.iter().zip(segments.iter()) {
            if item.skipped {
                continue;
            }
            any_content = true;
            let (w, h, b) = match &item.drawing {
                Some(d) => (d.width, d.height, d.height),
                None => (item.shaped.width, item.shaped.height, item.shaped.baseline),
            };
            line_width += w;
            line_height = line_height.max(h);
            line_baseline = line_baseline.max(b);
            if segment.text.ends_with('\n') {
                block_width = block_width.max(line_width);
                block_height += line_height;
                if first_line {
                    baseline = line_baseline;
                    first_line = false;
                }
                line_width = 0.0;
                line_height = 0.0;
                line_baseline = 0.0;
            }
        }
        if line_width > 0.0 || line_height > 0.0 || !any_content {
            block_width = block_width.max(line_width);
            block_height += line_height;
            if first_line {
                baseline = line_baseline;
            }
        }

        LayoutBlock {
            items,
            width: block_width,
            height: block_height,
            baseline,
        }
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
        styles: &[Style],
    ) {
        if event.event_type == EventType::Comment {
            return;
        }

        let start_ms = event.start.to_millis();
        let end_ms = event.end.to_millis();
        if time_ms < start_ms || time_ms >= end_ms {
            return;
        }

        if resolved.clip.is_some()
            || resolved.inverse_clip.is_some()
            || resolved.clip_vector.is_some()
            || resolved.inverse_clip_vector.is_some()
            || resolved.blur > 0.0
        {
            match RenderBuffer::new(video_width, video_height) {
                Ok(mut event_buffer) => {
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
                        styles,
                    );
                    buffer.blend_buffer(&event_buffer);
                }
                Err(_) => {
                    // Degenerate dimensions: degrade to direct rendering.
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
                        styles,
                    );
                }
            }
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
                styles,
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
        styles: &[Style],
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
            alpha_mult = complex_fade_opacity(cf, elapsed);
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

        // Effective wrap style: a per-event \q overrides the script default.
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
        // Drawing runs pass through the wrapper verbatim (never wrapped).
        let wrapped_text = wrap_event_text(
            &event.text,
            wrap_style,
            wrap_width,
            font,
            font_size,
            resolved.spacing,
        );

        // Parse text into segments for per-override rendering
        let segments = parse_text_segments_with_wrap(&wrapped_text, wrap_style);

        // Layout pass: resolve and shape every segment with its own style,
        // so alignment, positioning, rotation origins, and boxes use the
        // same per-segment dimensions as rendering.
        let layout = Self::layout_segments(
            &segments,
            event,
            resolved,
            font_manager,
            time_ms,
            start_ms,
            end_ms,
            play_res_x,
            play_res_y,
            video_width,
            video_height,
            styles,
        );

        let (mut base_x, mut base_y) = Self::calculate_position(
            resolved,
            layout.width,
            layout.height,
            layout.baseline,
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
            let (origin_x, origin_top) = Self::anchor_to_origin(
                resolved.alignment,
                anchor_x,
                anchor_y,
                layout.width,
                layout.height,
            );
            base_x = origin_x;
            base_y = origin_top + layout.baseline;
        }

        // Rotation origin for 3D effects. The default origin follows a move;
        // an explicit \org remains fixed in script coordinates.
        let (org_x, org_y) = if let Some((ox, oy)) = resolved.origin {
            (ox * scale_x, oy * scale_y)
        } else {
            let text_top = base_y - layout.baseline;
            let ax = match resolved.alignment {
                1 | 4 | 7 => base_x,
                2 | 5 | 8 => base_x + layout.width / 2.0,
                3 | 6 | 9 => base_x + layout.width,
                _ => base_x + layout.width / 2.0,
            };
            let ay = match resolved.alignment {
                7..=9 => text_top,
                4..=6 => text_top + layout.height / 2.0,
                1..=3 => text_top + layout.height,
                _ => text_top + layout.height / 2.0,
            };
            (ax, ay)
        };

        // Border style 3 is an opaque box behind the event text.
        if resolved.border_style == 3 {
            let box_color = resolved.back_color.to_rgba();
            effects::apply_opaque_box(
                buffer,
                base_x as i32,
                (base_y - layout.baseline) as i32,
                layout.width.ceil() as i32,
                layout.height.ceil() as i32,
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

        // Karaoke syllable timeline (empty when the event has no karaoke tags)
        let (karaoke_syllables, seg_syllable) =
            build_karaoke_timeline(&segments, font, font_size, resolved.spacing);
        let mut syllable_consumed: Vec<f64> = vec![0.0; karaoke_syllables.len()];
        let elapsed_ms = time_ms.saturating_sub(start_ms);

        // Per-segment rendering
        let mut x_offset = 0.0_f64;
        let mut line_y_offset = 0.0_f64;

        for (seg_idx, segment) in segments.iter().enumerate() {
            let item = &layout.items[seg_idx];
            if item.skipped {
                continue;
            }

            // Style/shape come from the layout pass; karaoke recolors here.
            let mut segment_resolved = item.resolved.clone();

            // Apply karaoke highlighting for this segment's syllable.
            // sweep_boundary holds the sweep edge in segment-local x
            // coordinates when the syllable uses \K / \kf.
            let mut sweep_boundary: Option<f64> = None;
            if let Some(syl_idx) = seg_syllable[seg_idx] {
                let syl = &karaoke_syllables[syl_idx];
                let started = elapsed_ms >= syl.start_ms;
                let finished = elapsed_ms >= syl.start_ms.saturating_add(syl.dur_ms);
                match syl.kind {
                    KaraokeKind::Hard => {
                        if !karaoke_is_primary(syl, elapsed_ms) {
                            segment_resolved.color = segment_resolved.secondary_color;
                        }
                    }
                    KaraokeKind::Outline => {
                        if karaoke_outline_suppressed(elapsed_ms, syl.start_ms) {
                            // Hide the outline once the syllable begins
                            segment_resolved.outline_color.alpha = 255;
                        }
                    }
                    KaraokeKind::Sweep => {
                        let frac = if !started {
                            0.0
                        } else if syl.dur_ms == 0 || finished {
                            1.0
                        } else {
                            (elapsed_ms - syl.start_ms) as f64 / syl.dur_ms as f64
                        };
                        sweep_boundary = Some(frac * syl.width - syllable_consumed[syl_idx]);
                    }
                }
            }

            // Drawing segments render vector paths at the pen position.
            if let Some(drawing) = &item.drawing {
                let unit = drawing_unit_scale(scale_x, scale_y, drawing.mode);
                let color = segment_resolved.color.to_ass_components();
                super::drawing::DrawingParser::render_drawing(
                    buffer,
                    &segment.text,
                    base_x + x_offset - drawing.min_x,
                    base_y + line_y_offset
                        - drawing.min_y
                        - segment_resolved.drawing_baseline_offset * unit,
                    unit,
                    [
                        color[0],
                        color[1],
                        color[2],
                        (segment_resolved.color.opacity() as f64 * alpha_mult) as u8,
                    ],
                );
                if let Some(syl_idx) = seg_syllable[seg_idx] {
                    syllable_consumed[syl_idx] += drawing.width;
                }
                if segment.text.ends_with('\n') {
                    x_offset = 0.0;
                    line_y_offset += drawing.height;
                } else {
                    x_offset += drawing.width;
                }
                continue;
            }

            // Text path: font, size, and shaping come from the layout pass.
            let segment_font = font_manager.get_font(item.font_id).unwrap_or(font);
            let segment_font_size =
                segment_resolved.font_size * (video_height as f64 / play_res_y.max(1) as f64);
            let shaped = &item.shaped;

            // Pre-compute effect parameters. With ScaledBorderAndShadow,
            // borders/shadows scale with the resolution ratio; otherwise
            // script units map 1:1 to video pixels.
            let (res_x, res_y) = if segment_resolved.scaled_border_and_shadow {
                (scale_x, scale_y)
            } else {
                (1.0, 1.0)
            };
            let outline_active = segment_resolved.border_style == 1
                && (segment_resolved.outline_x > 0.0 || segment_resolved.outline_y > 0.0);
            let outline_scale_x =
                segment_resolved.outline_x * res_x * segment_resolved.scale_x / 100.0;
            let outline_scale_y =
                segment_resolved.outline_y * res_y * segment_resolved.scale_y / 100.0;
            let outline_color_rgba = segment_resolved.outline_color.to_ass_components();
            let outline_alpha = segment_resolved.outline_color.opacity();

            let shadow_active =
                segment_resolved.shadow_x != 0.0 || segment_resolved.shadow_y != 0.0;
            let shadow_offset_x =
                segment_resolved.shadow_x * res_x * segment_resolved.scale_x / 100.0;
            let shadow_offset_y =
                segment_resolved.shadow_y * res_y * segment_resolved.scale_y / 100.0;
            let shadow_color_rgba = segment_resolved.shadow_color.to_ass_components();
            let shadow_alpha = segment_resolved.shadow_color.opacity();

            // Single pass over glyphs: cache lookup once, render outline + shadow + fill
            for glyph in &shaped.glyphs {
                if glyph.scale_x <= 0.0 || glyph.scale_y <= 0.0 {
                    continue;
                }

                let cached = self.glyph_cache.get_or_rasterize(
                    item.font_id,
                    segment_font,
                    glyph.glyph_id,
                    segment_font_size,
                    item.faux_bold,
                    item.faux_italic,
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
                let scaled_width = ((cached.width as f64 * glyph.scale_x).round() as u32).max(1);
                let scaled_height = ((cached.height as f64 * glyph.scale_y).round() as u32).max(1);
                // Shear (\fax/\fay) warps the coverage bitmap before rotation.
                let (work_bitmap, glyph_width, glyph_height) =
                    if segment_resolved.shear_x != 0.0 || segment_resolved.shear_y != 0.0 {
                        let (sheared, w, h) = RenderBuffer::shear_coverage_bitmap(
                            &scaled_bitmap,
                            scaled_width,
                            scaled_height,
                            segment_resolved.shear_x,
                            segment_resolved.shear_y,
                        );
                        if sheared.is_empty() {
                            continue;
                        }
                        (Cow::Owned(sheared), w, h)
                    } else {
                        (scaled_bitmap, scaled_width, scaled_height)
                    };
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
                        &work_bitmap,
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
                let current_outline_x = outline_scale_x * scale_factor;
                let current_outline_y = outline_scale_y * scale_factor;
                let current_shadow_x = shadow_offset_x * scale_factor;
                let current_shadow_y = shadow_offset_y * scale_factor;

                // Render outline (independent X/Y radii)
                if outline_active {
                    effects::apply_outline_xy(
                        buffer,
                        &rot_bitmap,
                        rot_w,
                        rot_h,
                        final_gx,
                        final_gy,
                        current_outline_x,
                        current_outline_y,
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
                    _ => segment_resolved.color,
                };
                let color = fill_color.to_ass_components();
                let color_alpha = fill_color.opacity();

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
            }

            // Decorations belong to the whole text segment, not individual
            // glyph bitmaps. Drawing them from the segment baseline avoids
            // gaps between glyphs and keeps them stable across font bearings.
            if segment_resolved.underline || segment_resolved.strike_out {
                let color = segment_resolved.color.to_rgba();
                let color_alpha = 255 - color[3];
                let line_width = if segment_resolved.underline {
                    (2.0 * scale_y).round().max(1.0) as i32
                } else {
                    (3.0 * scale_y).round().max(1.0) as i32
                };
                let mut line_offsets = Vec::new();
                for glyph in &shaped.glyphs {
                    if !line_offsets
                        .iter()
                        .any(|offset: &f64| (*offset - glyph.y).abs() < f64::EPSILON)
                    {
                        line_offsets.push(glyph.y);
                    }
                }
                for line_offset in line_offsets {
                    let decoration_y = if segment_resolved.underline {
                        base_y
                            + line_y_offset
                            + line_offset
                            + (shaped.height - shaped.baseline) * 0.5
                    } else {
                        base_y + line_y_offset + line_offset - shaped.baseline * 0.35
                    };
                    buffer.fill_rect(
                        (base_x + x_offset) as i32,
                        decoration_y as i32,
                        shaped.width.ceil() as i32,
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

        // Blur before clipping: blurring after a clip would bleed
        // pixels outside the clip region.
        if resolved.blur > 0.0 {
            effects::apply_blur(buffer, resolved.blur);
        }

        // Apply clipping (after all segments rendered)
        if let Some(clip_rect) = resolved.clip {
            effects::apply_clip(buffer, scale_clip_rect(clip_rect, scale_x, scale_y));
        }

        if let Some(clip_rect) = resolved.inverse_clip {
            effects::apply_inverse_clip(buffer, scale_clip_rect(clip_rect, scale_x, scale_y));
        }

        if let Some(vector) = &resolved.clip_vector {
            Self::apply_vector_clip(buffer, vector, scale_x, scale_y, false);
        }
        if let Some(vector) = &resolved.inverse_clip_vector {
            Self::apply_vector_clip(buffer, vector, scale_x, scale_y, true);
        }
    }

    /// Render a vector clip shape as an alpha mask over the event buffer
    /// (script coordinates, drawing scale) and apply or invert it.
    fn apply_vector_clip(
        buffer: &mut RenderBuffer,
        clip: &VectorClip,
        scale_x: f64,
        scale_y: f64,
        inverse: bool,
    ) {
        let unit = drawing_unit_scale(scale_x, scale_y, clip.scale.max(1));
        let mut mask = match RenderBuffer::new(buffer.width, buffer.height) {
            Ok(mask) => mask,
            Err(_) => return,
        };
        super::drawing::DrawingParser::render_mask(&mut mask, &clip.drawing, 0.0, 0.0, unit);
        effects::apply_alpha_mask(buffer, &mask, inverse);
    }

    /// Extract clean text from event text (remove override tags)
    /// Returns (clean_text, is_drawing_mode). `\n` is a space unless
    /// `wrap_style` is 2; any `\pN` with N > 0 enables drawing mode.
    #[cfg(test)]
    fn extract_clean_text(&self, text: &str, wrap_style: i32) -> (String, bool) {
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
                            'N' => {
                                chars.next();
                                if !drawing_mode {
                                    result.push('\n');
                                } else {
                                    result.push('\\');
                                    result.push(next);
                                }
                            }
                            'n' => {
                                chars.next();
                                if !drawing_mode {
                                    // Soft break: space, unless wrap mode 2.
                                    result.push(if wrap_style == 2 { '\n' } else { ' ' });
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
                                let mut digits = String::new();
                                while let Some(&d) = chars.peek() {
                                    if d.is_ascii_digit() {
                                        digits.push(d);
                                        chars.next();
                                    } else {
                                        break;
                                    }
                                }
                                if let Ok(mode) = digits.parse::<i32>() {
                                    drawing_mode = mode > 0;
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
    fn test_event_style_uses_only_initial_override_group() {
        let style = Style::new("Default");
        let event = Event::parse_from_line(
            "Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\fs72}Big{\\fs24}small",
        )
        .unwrap();
        let resolved = Compositor::resolve_style(&style, &event);
        assert_eq!(resolved.font_size, 72.0);
    }

    #[test]
    fn test_line_global_tag_applies_after_first_segment() {
        let style = Style::new("Default");
        let event = Event::parse_from_line(
            "Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,Hi{\\pos(100,100)}",
        )
        .unwrap();
        let resolved = Compositor::resolve_style(&style, &event);
        assert_eq!(resolved.position, Some((100.0, 100.0)));
        // Same as the leading placement
        let event2 = Event::parse_from_line(
            "Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{\\pos(100,100)}Hi",
        )
        .unwrap();
        let resolved2 = Compositor::resolve_style(&style, &event2);
        assert_eq!(resolved2.position, Some((100.0, 100.0)));
    }

    #[test]
    fn test_reset_to_named_style_switches_base() {
        let mut alt = Style::new("Alt");
        alt.primary_color = Color::new(0, 255, 0, 0); // opaque red
        let base = Style::new("Default"); // opaque white
        let styles = vec![base.clone(), alt];
        let event = Event::parse_from_line(
            "Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{\\pos(5,5)}A{\\rAlt}B",
        )
        .unwrap();
        let resolved = Compositor::resolve_style(&base, &event);
        // \rAlt segment resolves to the Alt style…
        let segments = parse_text_segments(&event.text);
        let seg =
            Compositor::resolve_segment_style(&resolved, &segments[1], &event, &styles, 0, 0, 2000);
        assert_eq!(seg.color, Color::new(0, 255, 0, 0));
        assert_eq!(seg.base_style.name, "Alt");
        // …while line-global position survives the reset.
        assert_eq!(seg.position, Some((5.0, 5.0)));
        // Unknown style names fall back to the event style.
        let event =
            Event::parse_from_line("Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,A{\\rNope}B")
                .unwrap();
        let segments = parse_text_segments(&event.text);
        let seg =
            Compositor::resolve_segment_style(&resolved, &segments[1], &event, &styles, 0, 0, 2000);
        assert_eq!(seg.base_style.name, "Default");
    }

    fn render_event_text(text: &str, scaled: bool) -> RenderBuffer {
        let mut comp = Compositor::new();
        let mut fm = FontManager::new();
        fm.load_font("DejaVu Sans", font::get_fallback_font(), false, false)
            .unwrap();
        let event = Event::parse_from_line(&format!(
            "Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{}",
            text
        ))
        .unwrap();
        let style = Style::new("Default");
        let mut resolved = Compositor::resolve_style(&style, &event);
        resolved.scaled_border_and_shadow = scaled;
        let mut buf = RenderBuffer::new(320, 100).unwrap();
        comp.composite_event(
            &mut buf,
            &event,
            &resolved,
            &fm,
            500,
            640,
            200,
            320,
            100,
            0,
            &[],
        );
        buf
    }

    fn painted_pixels(buf: &RenderBuffer) -> usize {
        buf.as_bytes().chunks_exact(4).filter(|p| p[3] > 0).count()
    }

    #[test]
    fn test_scaled_border_flag_changes_output() {
        // Video is half the play resolution: scaled borders render at
        // half size, unscaled at full script size.
        let scaled = render_event_text("Hi", true);
        let unscaled = render_event_text("Hi", false);
        assert_ne!(scaled.as_bytes(), unscaled.as_bytes());
        assert!(painted_pixels(&scaled) > 0);
        assert!(painted_pixels(&unscaled) > 0);
    }

    #[test]
    fn test_mixed_drawing_and_text_renders() {
        let buf = render_event_text("{\\p1}m 0 0 l 40 0 l 40 40 l 0 40{\\p0}Hi", true);
        assert!(painted_pixels(&buf) > 50);
        // Drawing-only still renders
        let buf = render_event_text("{\\p1}m 0 0 l 40 0 l 40 40 l 0 40", true);
        assert!(painted_pixels(&buf) > 50);
    }

    #[test]
    fn test_vector_clip_masks_render() {
        // Vector clip over the left quarter of the frame (script coords).
        // Centered text sits outside it, so `clip` removes everything and
        // `iclip` keeps everything.
        let shape = "m 0 0 l 160 0 l 160 200 l 0 200";
        let plain = render_event_text("Hello", true);
        assert!(painted_pixels(&plain) > 100);
        let clipped = render_event_text(&format!("{{\\clip({})}}Hello", shape), true);
        assert_ne!(plain.as_bytes(), clipped.as_bytes());
        assert_eq!(painted_pixels(&clipped), 0);
        let iclipped = render_event_text(&format!("{{\\iclip({})}}Hello", shape), true);
        assert_eq!(painted_pixels(&iclipped), painted_pixels(&plain));
    }

    #[test]
    fn test_extract_clean_text_wrap_aware() {
        let comp = Compositor::new();
        // \N always breaks; \n is a space except in wrap mode 2.
        assert_eq!(comp.extract_clean_text("a\\Nb", 0).0, "a\nb");
        assert_eq!(comp.extract_clean_text("a\\nb", 0).0, "a b");
        assert_eq!(comp.extract_clean_text("a\\nb", 2).0, "a\nb");
        // Tags stripped; any \pN>0 tracks drawing mode.
        let (text, drawing) = comp.extract_clean_text("{\\b1\\p2}m 0 0", 0);
        assert!(drawing);
        assert!(text.contains("m 0 0"));
        let (_, drawing) = comp.extract_clean_text("{\\p2}x{\\p0}y", 0);
        assert!(!drawing);
    }

    #[test]
    fn test_positioned_anchor_is_converted_to_text_origin() {
        let style = Style::new("Default");
        let event = Event::parse_from_line(
            "Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\an5\\pos(100,80)}Text",
        )
        .unwrap();
        let resolved = Compositor::resolve_style(&style, &event);
        let (x, y) =
            Compositor::calculate_position(&resolved, 40.0, 20.0, 15.0, 200, 100, 200, 100);
        assert_eq!((x, y), (80.0, 85.0));
    }

    #[test]
    fn test_complex_fade_alpha_direction() {
        // \fade(255,0,255,...): invisible -> visible -> invisible
        let cf = ComplexFade {
            a1: 255,
            a2: 0,
            a3: 255,
            t1: 0,
            t2: 500,
            t3: 2000,
            t4: 2200,
        };
        assert!((complex_fade_opacity(&cf, 0) - 0.0).abs() < 1e-9); // at t1: a1
        assert!((complex_fade_opacity(&cf, 250) - 0.5).abs() < 0.01); // mid ramp
        assert!((complex_fade_opacity(&cf, 500) - 1.0).abs() < 1e-9); // at t2: a2
        assert!((complex_fade_opacity(&cf, 1000) - 1.0).abs() < 1e-9); // hold
        assert!((complex_fade_opacity(&cf, 2000) - 1.0).abs() < 1e-9); // at t3
        assert!((complex_fade_opacity(&cf, 2100) - 0.5).abs() < 0.01);
        assert!((complex_fade_opacity(&cf, 2200) - 0.0).abs() < 1e-9); // at t4
        assert!((complex_fade_opacity(&cf, 5000) - 0.0).abs() < 1e-9); // after
    }

    #[test]
    fn test_complex_fade_degenerate_timing() {
        // Zero-length and inverted ranges: no division by zero, no panic
        let cf = ComplexFade {
            a1: 0,
            a2: 128,
            a3: 255,
            t1: 500,
            t2: 500,
            t3: 300,
            t4: 300,
        };
        for t in [0, 299, 300, 499, 500, 501, 10000] {
            let o = complex_fade_opacity(&cf, t);
            assert!((0.0..=1.0).contains(&o), "t={}", t);
        }
    }

    #[test]
    fn test_transform_acceleration_direction() {
        // accel = 1 linear; > 1 starts slow; < 1 starts fast
        assert!((Compositor::apply_accel(0.25, 1.0) - 0.25).abs() < 1e-9);
        assert!(Compositor::apply_accel(0.25, 2.0) < 0.25);
        assert!((Compositor::apply_accel(0.25, 2.0) - 0.0625).abs() < 1e-9);
        assert!(Compositor::apply_accel(0.25, 0.5) > 0.25);
        assert!((Compositor::apply_accel(0.25, 0.5) - 0.5).abs() < 1e-9);
        assert_eq!(Compositor::apply_accel(0.0, 2.0), 0.0);
        assert_eq!(Compositor::apply_accel(1.0, 2.0), 1.0);
        // Non-finite accel falls back to linear
        assert!((Compositor::apply_accel(0.3, f64::NAN) - 0.3).abs() < 1e-9);
    }

    #[test]
    fn test_ass_alpha_updates_all_colour_channels() {
        let style = Style::new("Default");
        let event = Event::parse_from_line(
            "Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\alpha&H80&\\2a&H20&}Text",
        )
        .unwrap();
        let resolved = Compositor::resolve_style(&style, &event);

        assert_eq!(resolved.color.alpha, 128);
        assert_eq!(resolved.secondary_color.alpha, 32);
        assert_eq!(resolved.outline_color.alpha, 128);
        assert_eq!(resolved.shadow_color.alpha, 128);
        assert_eq!(resolved.back_color.alpha, 128);
    }

    #[test]
    fn test_back_colour_override_is_used_for_shadow_and_box() {
        let style = Style::new("Default");
        let event = Event::parse_from_line(
            "Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\4c&HFF00FF&}Text",
        )
        .unwrap();
        let resolved = Compositor::resolve_style(&style, &event);

        assert_eq!(resolved.shadow_color, resolved.back_color);
        assert_eq!(resolved.shadow_color.red, 255);
        assert_eq!(resolved.shadow_color.blue, 255);
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
        let mut early = RenderBuffer::new(320, 100).unwrap();
        comp.composite_event(
            &mut early,
            &event,
            &resolved,
            &fm,
            100,
            320,
            100,
            320,
            100,
            0,
            &[],
        );

        let mut late = RenderBuffer::new(320, 100).unwrap();
        comp.composite_event(
            &mut late,
            &event,
            &resolved,
            &fm,
            1500,
            320,
            100,
            320,
            100,
            0,
            &[],
        );

        assert_ne!(early.as_bytes(), late.as_bytes());
    }

    #[test]
    fn test_karaoke_hard_switches_at_syllable_start() {
        // \k: secondary before the syllable starts, primary from the exact
        // start instant (not at the end). Second syllable starts at 1000ms.
        let event = Event::parse_from_line(
            "Dialogue: 0,0:00:00.00,0:00:03.00,Default,,0,0,0,,{\\k100}A{\\k100}B",
        )
        .unwrap();
        let style = Style::new("Default");
        let resolved = Compositor::resolve_style(&style, &event);
        let segments = parse_text_segments("{\\k100}A{\\k100}B");
        let font = fallback_font();
        let (syllables, map) = build_karaoke_timeline(&segments, &font, 48.0, 0.0);

        // First syllable at elapsed 0: started -> primary
        assert!(karaoke_is_primary(&syllables[0], 0));
        // Second syllable before/at boundaries
        assert!(!karaoke_is_primary(&syllables[1], 999));
        assert!(karaoke_is_primary(&syllables[1], 1000));
        assert!(karaoke_is_primary(&syllables[1], 1500));
        assert_eq!(map, vec![Some(0), Some(1)]);
        let _ = resolved;
    }

    #[test]
    fn test_karaoke_outline_suppressed_after_start() {
        // \ko: outline hidden once the syllable begins, not at its end.
        assert!(!karaoke_outline_suppressed(999, 1000));
        assert!(karaoke_outline_suppressed(1000, 1000));
        assert!(karaoke_outline_suppressed(1500, 1000));
    }

    #[test]
    fn test_karaoke_render_second_syllable_secondary_before_start() {
        let mut comp = Compositor::new();
        let mut fm = FontManager::new();
        fm.load_font("DejaVu Sans", font::get_fallback_font(), false, false)
            .unwrap();

        let event = Event::parse_from_line(
            "Dialogue: 0,0:00:00.00,0:00:03.00,Default,,0,0,0,,{\\k100}A{\\k100}B",
        )
        .unwrap();
        let style = Style::new("Default");
        let resolved = Compositor::resolve_style(&style, &event);

        // At 100ms: first syllable sung (primary white), second still
        // secondary (green). Both colors present.
        let mut early = RenderBuffer::new(320, 100).unwrap();
        comp.composite_event(
            &mut early,
            &event,
            &resolved,
            &fm,
            100,
            320,
            100,
            320,
            100,
            0,
            &[],
        );
        let early_bytes = early.as_bytes();
        assert!(early_bytes
            .chunks_exact(4)
            .any(|pixel| pixel[1] > 200 && pixel[0] < 50 && pixel[2] < 50 && pixel[3] > 0));
        assert!(early_bytes
            .chunks_exact(4)
            .any(|pixel| pixel[0] > 200 && pixel[1] > 200 && pixel[2] > 200 && pixel[3] > 0));

        // At 2500ms both syllables sung: only primary remains.
        let mut late = RenderBuffer::new(320, 100).unwrap();
        comp.composite_event(
            &mut late,
            &event,
            &resolved,
            &fm,
            2500,
            320,
            100,
            320,
            100,
            0,
            &[],
        );
        assert!(!late
            .as_bytes()
            .chunks_exact(4)
            .any(|pixel| pixel[1] > 200 && pixel[0] < 50 && pixel[2] < 50 && pixel[3] > 0));
    }
}
