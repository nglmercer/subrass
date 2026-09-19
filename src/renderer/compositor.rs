use super::buffer::{
    add_coord, effective_shear, finite_to_i32, RenderBuffer, MAX_GLYPH_BITMAP_PIXELS,
};
use super::effects;
use super::font::{DecorationMetrics, FontManager};
use super::glyph_cache::GlyphCache;
use super::shaper::{ShapingFont, TextShaper};
use crate::types::color::Color;
use crate::types::override_tag::{parse_text_segments, parse_text_segments_with_wrap, TextSegment};
use crate::types::{Event, EventType, LegacyEffect, OverrideTag, Style};
use crate::utils::Matrix3x3;
use ab_glyph::{Font, FontArc, PxScale, ScaleFont};
use std::borrow::Cow;

/// Resolved style with all overrides applied
#[derive(Debug, Clone)]
pub struct ResolvedStyle {
    pub base_style: Style,
    pub font_name: String,
    pub font_size: f64,
    /// Font encoding/charset id (`\fe`, default from the style).
    /// Stored and reset correctly; glyph selection stays Unicode-based.
    pub font_encoding: i32,
    pub color: Color,
    pub secondary_color: Color,
    pub outline_color: Color,
    pub shadow_color: Color,
    pub back_color: Color,
    /// ASS font weight: 400 = normal, 700 = bold (from `\b` or the style).
    pub font_weight: u16,
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
    /// libass `PARSED_A`: an `\an`/`\a` tag was already consumed for
    /// this event, so later alignment tags are ignored (first wins).
    /// Persists across `\r`, which never resets alignment in libass.
    pub parsed_alignment: bool,
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
    pub fade_in: i32,
    pub fade_out: i32,
    pub complex_fade: Option<ComplexFade>,
    /// libass `PARSED_FADE`: a `\fad`/`\fade` tag was already consumed
    /// for this event, so later fade tags are ignored (first wins).
    /// Needed because `\fad(0,0)` is otherwise indistinguishable from
    /// "no fade tag". Persists across `\r` with the fade values.
    pub parsed_fade: bool,
    /// Effective per-event `\q` override.  Keeping it in the resolved
    /// state makes transformed `\q` participate in wrapping at the same
    /// timestamp as every other event-global property.
    pub wrap_style: Option<i32>,
    /// Script-space canvas used as the initial rectangular clip while a
    /// transform is interpolating a clip that did not previously exist.
    pub clip_canvas: (i32, i32),
    /// Set only for the frame-level state.  Segment resolution uses this
    /// marker to avoid applying event-global transform targets twice.
    event_globals_applied: bool,
    /// ASS-2 layout resolution used for blur and unscaled border/shadow
    /// metrics. Zero means the legacy PlayRes fallback.
    pub layout_res_x: u32,
    pub layout_res_y: u32,
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

/// Line-global state preserved across `\r` resets: the non-style
/// line properties (`\pos`, `\move`, `\org`, `\clip`, `\iclip`,
/// `\fad`, `\fade`) plus alignment, drawing mode, and `\pbo`: libass
/// `ass_reset_render_context` (the `\r` handler) never touches
/// alignment, `drawing_scale`, or `pbo`, so the first `\an`/`\a` and
/// the current drawing state survive resets. Karaoke timing also
/// survives (the reset never touches the effect fields); it is
/// computed from accumulated tags, which `\r` does not clear.
/// Everything else — fonts, colors, border/shadow, rotation —
/// resets to the target style, because `\r` restores ordinary
/// override state.
struct LineGlobalKeep {
    position: Option<(f64, f64)>,
    origin: Option<(f64, f64)>,
    move_data: Option<MoveData>,
    clip: Option<(i32, i32, i32, i32)>,
    inverse_clip: Option<(i32, i32, i32, i32)>,
    clip_vector: Option<VectorClip>,
    inverse_clip_vector: Option<VectorClip>,
    fade_in: i32,
    fade_out: i32,
    complex_fade: Option<ComplexFade>,
    alignment: i32,
    parsed_alignment: bool,
    parsed_fade: bool,
    wrap_style: Option<i32>,
    clip_canvas: (i32, i32),
    event_globals_applied: bool,
    layout_res_x: u32,
    layout_res_y: u32,
    drawing_mode: i32,
    drawing_baseline_offset: f64,
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
            alignment: resolved.alignment,
            parsed_alignment: resolved.parsed_alignment,
            parsed_fade: resolved.parsed_fade,
            wrap_style: resolved.wrap_style,
            clip_canvas: resolved.clip_canvas,
            event_globals_applied: resolved.event_globals_applied,
            layout_res_x: resolved.layout_res_x,
            layout_res_y: resolved.layout_res_y,
            drawing_mode: resolved.drawing_mode,
            drawing_baseline_offset: resolved.drawing_baseline_offset,
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
        resolved.alignment = self.alignment;
        resolved.parsed_alignment = self.parsed_alignment;
        resolved.parsed_fade = self.parsed_fade;
        resolved.wrap_style = self.wrap_style;
        resolved.clip_canvas = self.clip_canvas;
        resolved.event_globals_applied = self.event_globals_applied;
        resolved.layout_res_x = self.layout_res_x;
        resolved.layout_res_y = self.layout_res_y;
        resolved.drawing_mode = self.drawing_mode;
        resolved.drawing_baseline_offset = self.drawing_baseline_offset;
    }
}

/// Move animation data: times are `i32` like libass (`argtoi32`),
/// already swapped so `t1 <= t2`.
#[derive(Debug, Clone)]
pub struct MoveData {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub t1: i32,
    pub t2: i32,
}

/// Complex fade data: all `i32` like libass. Alpha interpolates
/// full-range and truncates exactly like `interpolate_alpha`.
#[derive(Debug, Clone)]
pub struct ComplexFade {
    pub a1: i32,
    pub a2: i32,
    pub a3: i32,
    pub t1: i32,
    pub t2: i32,
    pub t3: i32,
    pub t4: i32,
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
    /// True for CJK continuation pieces: split from the previous word at
    /// a CJK break opportunity, so no space is emitted or measured
    /// between them (unlike `preceded_by_space`, which measures one).
    glued_to_prev: bool,
}

/// Insert '\n' at word boundaries per the ASS wrap style:
/// 0 = smart wrapping (balanced lines), 1 = end-of-line greedy
/// wrapping, 2 = no automatic wrapping, 3 = same as 0 (libass runs
/// `wrap_lines_smart` for every style except 1; VSFilter's
/// bottom-wide style 3 is a known libass divergence).
///
/// Tag groups are opaque and travel with the word that follows them;
/// explicit `\N` breaks (and `\n` in mode 2) split the text into
/// independently wrapped runs. `\n` elsewhere acts as a space. Drawing
/// runs pass through verbatim and are never wrapped. Non-CJK words
/// wider than `max_width` stay on their own line; CJK runs split at
/// break opportunities (glued pieces, no phantom spaces).
///
/// `fonts` is the measurement chain (primary first): word widths use
/// the same per-glyph fallback cascade as shaping, so wrap decisions
/// match rendered widths even when the primary lacks characters.
fn wrap_event_text(
    text: &str,
    wrap_style: i32,
    max_width: f64,
    fonts: &[&FontArc],
    font_size: f64,
    spacing: f64,
) -> String {
    let measure =
        |value: &str| TextShaper::measure_text_with_fallback(value, fonts, font_size, spacing);
    wrap_event_text_with_measure(text, wrap_style, max_width, &measure)
}

fn wrap_event_text_with_measure<F>(
    text: &str,
    wrap_style: i32,
    max_width: f64,
    measure: &F,
) -> String
where
    F: Fn(&str) -> f64,
{
    if wrap_style == 2 || max_width <= 0.0 {
        return text.to_string();
    }

    /// Drawing state after a `{...}` group: the last `\pN` wins in
    /// textual order. `\r` is ignored: libass
    /// `ass_reset_render_context` never touches `drawing_scale`, so
    /// `{\p1\r}` stays drawing — only `\p0` exits.
    fn drawing_state_after_group(group: &str) -> Option<bool> {
        let bytes = group.as_bytes();
        let mut state: Option<bool> = None;
        let mut i = 0;
        while i + 1 < bytes.len() {
            if bytes[i] == b'\\' && bytes[i + 1] == b'p' {
                // `\pN` with digits; `\pbo` has none and is skipped.
                let mut j = i + 2;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if j > i + 2 {
                    if let Ok(mode) = group[i + 2..j].parse::<i32>() {
                        state = Some(mode > 0);
                    }
                    i = j;
                    continue;
                }
            }
            i += 1;
        }
        state
    }

    // Tokenize into words (with pending tag prefixes) and hard breaks.
    let mut words: Vec<WrapWord> = Vec::new();
    // Break markers: index into `words` where a new run starts
    let mut run_starts: Vec<usize> = vec![0];
    let mut prefix = String::new();
    let mut word = String::new();

    // First word is not preceded by a space; the current word holds
    // drawing commands when verbatim (never wrapped).
    let mut preceded_by_space = false;
    let mut word_is_drawing = false;

    let flush = |prefix: &mut String,
                 word: &mut String,
                 words: &mut Vec<WrapWord>,
                 pbys: &mut bool,
                 is_drawing: &mut bool| {
        // Flush when there is text, or when a tag prefix must be preserved
        // (e.g. {\b0} between "Bold" and the following space).
        if word.is_empty() && prefix.is_empty() {
            return;
        }
        let has_text = !word.is_empty();
        // Drawing words measure geometric width, never the advances of
        // their command letters: text-measuring "m 0 0 l 100 ..." forces
        // bogus line breaks before drawings (and strands karaoke timing
        // on the break). Unit scale is unavailable here, so this is in
        // drawing units (exact for the common scale-1/mode-1 case).
        let width = if *is_drawing && has_text {
            let commands = word.split('{').next().unwrap_or(word);
            super::drawing::DrawingParser::measure(commands)
                .map(|(_, _, w, _)| w)
                .filter(|w| w.is_finite())
                .map(|w| w.max(0.0))
                .unwrap_or(0.0)
        } else {
            measure(word)
        };
        *is_drawing = false;
        words.push(WrapWord {
            prefix: std::mem::take(prefix),
            text: std::mem::take(word),
            width,
            preceded_by_space: *pbys,
            glued_to_prev: false,
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
                    word_is_drawing = true;
                } else {
                    flush(
                        &mut prefix,
                        &mut word,
                        &mut words,
                        &mut preceded_by_space,
                        &mut word_is_drawing,
                    );
                    prefix.push_str(&group);
                }
                if let Some(drawing) = drawing_state_after_group(&group) {
                    in_drawing = drawing;
                }
            }
            '\\' if in_drawing => {
                word.push('\\');
                if let Some(&n) = chars.peek() {
                    word.push(n);
                    chars.next();
                }
                word_is_drawing = true;
            }
            '\\' => match chars.peek() {
                Some('N') => {
                    chars.next();
                    flush(
                        &mut prefix,
                        &mut word,
                        &mut words,
                        &mut preceded_by_space,
                        &mut word_is_drawing,
                    );
                    preceded_by_space = true;
                    run_starts.push(words.len());
                }
                Some('n') => {
                    chars.next();
                    // Soft break: a hard break only in mode 2, else a space.
                    if wrap_style == 2 {
                        flush(
                            &mut prefix,
                            &mut word,
                            &mut words,
                            &mut preceded_by_space,
                            &mut word_is_drawing,
                        );
                        preceded_by_space = true;
                        run_starts.push(words.len());
                    } else {
                        flush(
                            &mut prefix,
                            &mut word,
                            &mut words,
                            &mut preceded_by_space,
                            &mut word_is_drawing,
                        );
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
                    word_is_drawing = true;
                } else {
                    flush(
                        &mut prefix,
                        &mut word,
                        &mut words,
                        &mut preceded_by_space,
                        &mut word_is_drawing,
                    );
                    preceded_by_space = true;
                }
            }
            _ => {
                word.push(c);
                if in_drawing {
                    word_is_drawing = true;
                }
            }
        }
    }
    flush(
        &mut prefix,
        &mut word,
        &mut words,
        &mut preceded_by_space,
        &mut word_is_drawing,
    );
    // Trailing tag groups with no word attach as a zero-width word
    if !prefix.is_empty() {
        words.push(WrapWord {
            prefix,
            text: String::new(),
            width: 0.0,
            preceded_by_space: false,
            glued_to_prev: false,
        });
    }
    // CJK break opportunities inside words (conservative UAX #14):
    // overlong CJK runs wrap without spaces. Continuation pieces glue
    // with no gap; run starts remap to the split indices.
    let words = split_cjk_words(words, &mut run_starts, measure);

    let space_width = measure(" ");

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

/// Split plain-text words at CJK break opportunities
/// ([`cjk_break_between`](super::shaper::cjk_break_between)); the first
/// piece keeps the word's prefix/flags, continuations glue with no gap.
/// Skips drawing words (verbatim text with groups), empty words, and
/// breaks right after a backslash (would split `\x` escapes across
/// lines). Remaps `run_starts` to the split indices in place.
fn split_cjk_words<F>(words: Vec<WrapWord>, run_starts: &mut [usize], measure: &F) -> Vec<WrapWord>
where
    F: Fn(&str) -> f64,
{
    use super::shaper::cjk_break_between;
    let mut out: Vec<WrapWord> = Vec::with_capacity(words.len());
    let mut index_of: Vec<usize> = Vec::with_capacity(words.len() + 1);
    for mut word in words {
        index_of.push(out.len());
        let chars: Vec<char> = word.text.chars().collect();
        let splittable = !chars.is_empty() && !word.text.contains('{');
        let mut cuts = vec![false; chars.len()];
        if splittable {
            for i in 0..chars.len().saturating_sub(1) {
                if chars[i] != '\\' && cjk_break_between(chars[i], chars[i + 1]) {
                    cuts[i] = true;
                }
            }
        }
        if !cuts.iter().any(|c| *c) {
            out.push(word);
            continue;
        }
        let mut start = 0usize;
        let mut first = true;
        for (i, cut) in cuts.iter().enumerate() {
            if !cut {
                continue;
            }
            let piece: String = chars[start..=i].iter().collect();
            let width = measure(&piece);
            if first {
                out.push(WrapWord {
                    prefix: std::mem::take(&mut word.prefix),
                    text: piece,
                    width,
                    preceded_by_space: word.preceded_by_space,
                    glued_to_prev: false,
                });
                first = false;
            } else {
                out.push(WrapWord {
                    prefix: String::new(),
                    text: piece,
                    width,
                    preceded_by_space: false,
                    glued_to_prev: true,
                });
            }
            start = i + 1;
        }
        let piece: String = chars[start..].iter().collect();
        let width = measure(&piece);
        if first {
            out.push(WrapWord {
                prefix: word.prefix,
                text: piece,
                width,
                preceded_by_space: word.preceded_by_space,
                glued_to_prev: false,
            });
        } else {
            out.push(WrapWord {
                prefix: String::new(),
                text: piece,
                width,
                preceded_by_space: false,
                glued_to_prev: true,
            });
        }
    }
    index_of.push(out.len());
    for start in run_starts.iter_mut() {
        *start = index_of.get(*start).copied().unwrap_or(out.len());
    }
    out
}

/// Width charged between a line's previous word and `word`: a full
/// space, except CJK continuation pieces glue with no gap (they were
/// split from one word, not separated by a space).
fn gap_before(word: &WrapWord, space_width: f64) -> f64 {
    if word.glued_to_prev {
        0.0
    } else {
        space_width
    }
}

/// Greedy line grouping (wrap style 1): forward fill, top line
/// first. Prefix-only entries ride along without consuming width
/// budget.
fn greedy_wrap_lines(words: &[WrapWord], max_width: f64, space_width: f64) -> Vec<Vec<usize>> {
    let mut lines: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut cur_w = 0.0_f64;
    for (idx, word) in words.iter().enumerate() {
        let ww = word.width;
        if ww == 0.0 && word.text.is_empty() {
            cur.push(idx);
            continue;
        }
        if cur.is_empty() {
            cur_w = ww;
            cur.push(idx);
        } else if cur_w + gap_before(word, space_width) + ww <= max_width {
            cur_w += gap_before(word, space_width) + ww;
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
    lines
}

/// Smart line grouping (wrap styles 0 and 3): greedy fill, then pairwise
/// rebalance (libass `wrap_lines_smart`: move the last word of a line
/// to the next line while it reduces the pair's length difference).
/// This balances lines instead of minimizing leftover (a 224/140
/// greedy split rebalances toward even halves, as in libass).
/// Overlong single words keep their own line. Words only ever move
/// to later lines, so the loop always terminates.
fn smart_wrap_lines(words: &[WrapWord], max_width: f64, space_width: f64) -> Vec<Vec<usize>> {
    let mut lines = greedy_wrap_lines(words, max_width, space_width);
    // Trimmed width of one line (prefix-only entries are free, spaces
    // only between text words).
    let line_len = |line: &[usize]| -> f64 {
        let mut w = 0.0_f64;
        let mut text_words = 0usize;
        for &idx in line {
            let word = &words[idx];
            if word.text.is_empty() && word.width == 0.0 {
                continue;
            }
            if text_words > 0 {
                w += gap_before(word, space_width);
            }
            w += word.width;
            text_words += 1;
        }
        w
    };
    loop {
        let mut moved = false;
        let mut i = 0usize;
        while i + 1 < lines.len() {
            // Never empty a line (merging breaks is never beneficial).
            if lines[i].len() > 1 {
                let l1 = line_len(&lines[i]);
                let l2 = line_len(&lines[i + 1]);
                let mut new_l1 = lines[i].clone();
                let w = new_l1.pop().expect("len > 1");
                let mut new_l2 = Vec::with_capacity(lines[i + 1].len() + 1);
                new_l2.push(w);
                new_l2.extend_from_slice(&lines[i + 1]);
                let l1_new = line_len(&new_l1);
                let l2_new = line_len(&new_l2);
                if (l1_new - l2_new).abs() < (l1 - l2).abs() {
                    lines[i] = new_l1;
                    lines[i + 1] = new_l2;
                    moved = true;
                }
            }
            i += 1;
        }
        if !moved {
            break;
        }
    }
    lines
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

    // libass `wrap_lines_smart` runs for every style except 1 (the
    // rebalance loop is gated on `wrap_style != 1`): styles 0 and 3
    // are the same smart fill, 1 is greedy only. Style 3 is NOT
    // bottom-wide greedy here — that is VSFilter behavior, and
    // libass documents the gap with a FIXME ("implement style 0 and
    // 3 correctly"). Style 2 never reaches this function.
    let lines: Vec<Vec<usize>> = if wrap_style == 1 {
        greedy_wrap_lines(words, max_width, space_width)
    } else {
        smart_wrap_lines(words, max_width, space_width)
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
    /// `\ko` — outline hidden before the syllable starts, visible from start
    Outline,
}

/// Opacity (0.0 = invisible, 1.0 = fully visible) for
/// `\fade(a1,a2,a3,t1,t2,t3,t4)` at `elapsed` ms into the event.
///
/// The fade value comes from [`effects::interpolate_alpha`] (libass
/// `interpolate_alpha`, including its truncation); a value `<= 0`
/// leaves the frame fully opaque (libass `ass_apply_fade` only
/// applies positive fades) and above 255 clamps to transparent
/// (libass wraps mod 256 there, a C-cast artifact).
fn complex_fade_opacity(cf: &ComplexFade, elapsed: u64) -> f64 {
    let now = i64::try_from(elapsed).unwrap_or(i64::MAX);
    let a = effects::interpolate_alpha(
        now,
        i64::from(cf.t1),
        i64::from(cf.t2),
        i64::from(cf.t3),
        i64::from(cf.t4),
        cf.a1,
        cf.a2,
        cf.a3,
    );
    if a <= 0 {
        1.0
    } else {
        (1.0 - f64::from(a.min(255)) / 255.0).clamp(0.0, 1.0)
    }
}

/// `\ko` outline rule: the outline is suppressed *before* the run
/// begins (`elapsed < start`, secondary fill + no outline) and becomes
/// visible from the exact start instant (primary fill + normal outline).
fn karaoke_outline_suppressed(elapsed_ms: u64, start_ms: u64) -> bool {
    elapsed_ms < start_ms
}

/// Leading karaoke state a segment's tag group contributes, mirroring
/// libass's per-glyph effect fields at segment granularity: the state
/// the segment's first emitted glyph would carry. Empty (non-emitting)
/// segments pass accumulated state through untouched.
#[derive(Debug, Clone, Copy, Default)]
struct KaraokeLead {
    kind: Option<KaraokeKind>,
    /// `\k` duration in ms (`effect_timing`; nonzero starts a new run).
    dur_ms: u64,
    /// Accumulated dead time in ms (`effect_skip_timing`).
    skip_ms: u64,
    /// `\kt` clock reset (`reset_effect`).
    reset: bool,
}

/// One piece of a karaoke run: a (segment, line) span. Runs never cross
/// event lines, style keys, drawings, or nonzero `\k` durations.
#[derive(Debug, Clone)]
struct RunPiece {
    seg_idx: usize,
    /// Shaper row (`glyph.y`) for text pieces; `None` for drawings and
    /// empty leading pieces.
    line_y: Option<f64>,
    event_line: usize,
    /// Full pen width of this piece (max glyph edge / drawing width).
    width: f64,
    /// Leading/trailing whitespace trim (visible-span computation).
    trim_front: f64,
    trim_back: f64,
    lead: KaraokeLead,
    is_drawing: bool,
}

/// One karaoke run: maximal same-style, same-line span (libass
/// `starts_new_run` semantics) with its timing window. Sweep
/// interpolation, pop timing, and the frz fill-flip are all per-run.
#[derive(Debug, Clone)]
struct KaraokeRun {
    kind: KaraokeKind,
    /// Window `[start_ms, end_ms)`; `end_ms == start_ms` pops.
    start_ms: u64,
    end_ms: u64,
    /// Visible sweep span in layout px (whitespace-trimmed).
    span: f64,
    /// `\frz` in (90, 270): mirror the sweep and swap the colors.
    flip: bool,
    /// True when this run interpolates a sweep (vs whole-run pop).
    sweep: bool,
    /// Last member glyph `(seg_idx, glyph_idx)` for buffer flushing;
    /// `None` for runs with no text glyphs (timing only / drawings).
    last_glyph: Option<(usize, usize)>,
    /// First member glyph `(seg_idx, glyph_idx)` for baseline-shear
    /// resets (libass `apply_baseline_shear` restarts the accumulator
    /// at every run start); `None` when the run has no text glyphs.
    first_glyph: Option<(usize, usize)>,
}

impl KaraokeRun {
    /// Sweep fraction at `elapsed_ms`: 0 before the window, 1 from its
    /// end, linear inside. Only meaningful when `sweep` is true.
    fn sweep_frac(&self, elapsed_ms: u64) -> f64 {
        if elapsed_ms < self.start_ms {
            0.0
        } else if self.end_ms <= self.start_ms || elapsed_ms >= self.end_ms {
            1.0
        } else {
            (elapsed_ms - self.start_ms) as f64 / (self.end_ms - self.start_ms) as f64
        }
    }
}

/// Cap on buffered sweep bitmaps per event render (bytes of coverage).
/// Legit runs hold kilobytes; past this, remaining members fall back to
/// whole-glyph midpoint coloring against the flushed edge.
const MAX_SWEEP_BUFFER_BYTES: usize = 16 << 20;

/// One transformed glyph awaiting its run's device-space sweep split.
/// All placement and color state is captured so the flush paints in
/// document order with no re-transform.
struct BufferedSweepGlyph {
    bitmap: Vec<u8>,
    w: u32,
    h: u32,
    gx: i32,
    gy: i32,
    primary: [u8; 4],
    primary_alpha: u8,
    secondary: [u8; 4],
    secondary_alpha: u8,
    /// Outline `(rgba, radius_x, radius_y)` when active.
    outline: Option<([u8; 4], f64, f64)>,
    /// Shadow `(rgba, offset_x, offset_y)` when active.
    shadow: Option<([u8; 4], f64, f64)>,
}

/// Buffer for the in-window sweep run currently being transformed.
/// libass splits the run's transformed bitmaps at one device-space x,
/// so the run's ink left edge must be known before any member draws.
/// The buffer holds at most one run (runs are contiguous in document
/// order) and flushes at the run's last glyph, preserving exact
/// paint order.
struct SweepState {
    buf: Vec<BufferedSweepGlyph>,
    bytes: usize,
    run: Option<usize>,
    /// Degraded run `(id, edge, flip)`: members past the buffer cap
    /// paint whole-glyph by center against the flushed edge.
    degraded: Option<(usize, i64, bool)>,
}

impl SweepState {
    fn new() -> Self {
        Self {
            buf: Vec::new(),
            bytes: 0,
            run: None,
            degraded: None,
        }
    }

    /// Drop the degraded fallback when leaving its run.
    fn note_run(&mut self, run_id: Option<usize>) {
        if self.degraded.map(|(id, _, _)| id) != run_id {
            self.degraded = None;
        }
    }

    /// Paint buffered members, split at the run's device-space edge:
    /// `round(ink_left + frac * span)` (mirrored when `flip`), with a
    /// hard boundary (verified against ffmpeg/libass probes: adjacent
    /// primary/secondary columns, outline unsplit). No-op when empty.
    fn flush(
        &mut self,
        buffer: &mut RenderBuffer,
        runs: &[KaraokeRun],
        elapsed_ms: u64,
        fade_alpha: u8,
    ) {
        let run_id = self.run.take();
        if self.buf.is_empty() {
            self.bytes = 0;
            return;
        }
        let Some(run_id) = run_id else {
            self.buf.clear();
            self.bytes = 0;
            return;
        };
        let Some(run) = runs.get(run_id) else {
            self.buf.clear();
            self.bytes = 0;
            return;
        };
        // Device ink bounds over the transformed bitmaps.
        let mut ink: Option<(i64, i64)> = None;
        for glyph in &self.buf {
            let (mut first, mut last) = (u32::MAX, 0u32);
            for (idx, coverage) in glyph.bitmap.iter().enumerate() {
                if *coverage > 0 {
                    let px = (idx as u64 % u64::from(glyph.w.max(1))) as u32;
                    first = first.min(px);
                    last = last.max(px);
                }
            }
            if first != u32::MAX {
                let left = i64::from(glyph.gx) + i64::from(first);
                let right = i64::from(glyph.gx) + i64::from(last) + 1;
                ink = Some(match ink {
                    Some((lo, hi)) => (lo.min(left), hi.max(right)),
                    None => (left, right),
                });
            }
        }
        // `span` is layout units, which match device pixels 1:1 here
        // (shaping already absorbed resolution scale and `\fsc`).
        let edge = match ink {
            Some((left, _)) if run.span.is_finite() && run.span >= 0.0 => {
                let offset = run.sweep_frac(elapsed_ms) * run.span;
                let raw = if run.flip {
                    left as f64 + run.span - offset
                } else {
                    left as f64 + offset
                };
                raw.round().clamp(i64::MIN as f64, i64::MAX as f64) as i64
            }
            // Blank run (whitespace only): nothing paints anyway.
            _ => ink.map(|(left, _)| left).unwrap_or(0),
        };
        for glyph in self.buf.drain(..) {
            if let Some((rgba, rx, ry)) = glyph.outline {
                effects::apply_outline_xy(
                    buffer,
                    &glyph.bitmap,
                    glyph.w,
                    glyph.h,
                    glyph.gx,
                    glyph.gy,
                    rx,
                    ry,
                    rgba,
                );
            }
            if let Some((rgba, ox, oy)) = glyph.shadow {
                effects::apply_shadow(
                    buffer,
                    &glyph.bitmap,
                    glyph.w,
                    glyph.h,
                    glyph.gx,
                    glyph.gy,
                    ox,
                    oy,
                    rgba,
                );
            }
            let (primary, primary_alpha) = (glyph.primary, glyph.primary_alpha);
            let (secondary, secondary_alpha) = (glyph.secondary, glyph.secondary_alpha);
            let flip = run.flip;
            let geom = GlyphGeom {
                w: glyph.w,
                h: glyph.h,
                gx: glyph.gx,
                gy: glyph.gy,
            };
            paint_glyph_fill(buffer, &glyph.bitmap, geom, fade_alpha, |px| {
                let left_side = i64::from(glyph.gx) + i64::from(px) < edge;
                if left_side != flip {
                    (primary, primary_alpha)
                } else {
                    (secondary, secondary_alpha)
                }
            });
        }
        self.bytes = 0;
        // A degraded run keeps its edge for whole-glyph members.
        if self.degraded.map(|(id, _, _)| id) == Some(run_id) {
            self.degraded = Some((run_id, edge, run.flip));
        }
    }
}

/// Placement of one transformed coverage bitmap: size + device origin.
#[derive(Debug, Clone, Copy)]
struct GlyphGeom {
    w: u32,
    h: u32,
    gx: i32,
    gy: i32,
}

/// Paint one transformed glyph's fill: per-pixel coverage blend, the
/// color per bitmap column chosen by `pick` (solid fill and karaoke
/// splits share this path, so blending can never diverge).
fn paint_glyph_fill(
    buffer: &mut RenderBuffer,
    bitmap: &[u8],
    geom: GlyphGeom,
    fade_alpha: u8,
    pick: impl Fn(u32) -> ([u8; 4], u8),
) {
    for py in 0..geom.h {
        for px in 0..geom.w {
            let idx = (u64::from(py) * u64::from(geom.w) + u64::from(px)) as usize;
            let coverage = bitmap.get(idx).copied().unwrap_or(0);
            if coverage > 0 {
                let (color, color_alpha) = pick(px);
                let a = ((u32::from(coverage) * u32::from(color_alpha) / 255)
                    * u32::from(fade_alpha)
                    / 255) as u8;
                // Widen through i64 and bounds-check before u32
                // conversion: never wrap i32 or rely on casts.
                let (Some(sx), Some(sy)) = (
                    add_coord(geom.gx, px, buffer.width),
                    add_coord(geom.gy, py, buffer.height),
                ) else {
                    continue;
                };
                buffer.blend_pixel(sx, sy, color[0], color[1], color[2], a);
            }
        }
    }
}

impl ResolvedStyle {
    /// libass `split_style_runs` key: two segments share a karaoke run
    /// only when every render-affecting field matches. Line-global
    /// state (position, clips, fades, alignment, margins) is excluded,
    /// exactly like upstream.
    fn same_karaoke_run(&self, other: &Self) -> bool {
        self.font_name == other.font_name
            && self.font_size == other.font_size
            && self.color == other.color
            && self.secondary_color == other.secondary_color
            && self.outline_color == other.outline_color
            && self.shadow_color == other.shadow_color
            && self.back_color == other.back_color
            && self.font_weight == other.font_weight
            && self.italic == other.italic
            && self.underline == other.underline
            && self.strike_out == other.strike_out
            && self.scale_x == other.scale_x
            && self.scale_y == other.scale_y
            && self.spacing == other.spacing
            && self.angle == other.angle
            && self.rotation_x == other.rotation_x
            && self.rotation_y == other.rotation_y
            && self.border_style == other.border_style
            && self.outline == other.outline
            && self.outline_x == other.outline_x
            && self.outline_y == other.outline_y
            && self.shadow == other.shadow
            && self.shadow_x == other.shadow_x
            && self.shadow_y == other.shadow_y
            && self.shear_x == other.shear_x
            && self.shear_y == other.shear_y
            && self.blur == other.blur
    }
}

/// Sanitize a measured layout width for sweep math: finite and
/// non-negative, else 0.
fn clean_width(v: f64) -> f64 {
    if v.is_finite() {
        v.max(0.0)
    } else {
        0.0
    }
}

/// Karaoke run build product: the runs, a per-segment per-glyph run
/// assignment (by glyph index into the shaped line), and a
/// per-segment drawing run assignment. `None` means "no karaoke here"
/// (normal rendering).
type KaraokeBuild = (Vec<KaraokeRun>, Vec<Vec<Option<usize>>>, Vec<Option<usize>>);

/// Build karaoke runs for segmented event text, replicating libass
/// (`split_style_runs` + `ass_process_karaoke_effects`, verified against
/// ffmpeg-rendered probes):
///
/// * Tag groups accumulate effect state in order: `\kt` assigns skip
///   and resets, each `\k`-family tag adds the previous duration to
///   skip and sets the new duration (stacked tags accumulate dead
///   time). State clears at the next emitting segment, so `\k0`
///   mid-run adds skip without breaking the run.
/// * Runs break at nonzero durations, effect-type changes, style-key
///   changes, drawings, and event-line changes. Later runs in a
///   syllable pop at the window end instead of sweeping; `\N` runs
///   consume timing invisibly.
/// * Sweep spans exclude trimmed leading/trailing ASCII spaces.
fn build_karaoke_runs(segments: &[TextSegment], items: &[LayoutItem]) -> KaraokeBuild {
    // ---- Pass 1: accumulate tag state, cut (segment, line) pieces. ----
    let mut pieces: Vec<RunPiece> = Vec::new();
    let mut seg_pieces: Vec<Vec<usize>> = vec![Vec::new(); segments.len()];
    let mut pending = KaraokeLead::default();
    let mut prev_tag_count = 0usize;
    // Event-line tracker mirroring the render loop: leading breaks and
    // internal row changes advance, trailing breaks advance at the end.
    let mut event_line = 0usize;

    for (seg_idx, segment) in segments.iter().enumerate() {
        let from = prev_tag_count.min(segment.tags.len());
        fn consume(tag: &OverrideTag, pending: &mut KaraokeLead) {
            match tag {
                // `\kt` assigns (wiping stacked durations) and resets.
                OverrideTag::KaraokeStart(t) => {
                    pending.skip_ms = t.saturating_mul(10);
                    pending.dur_ms = 0;
                    pending.reset = true;
                }
                // Each `\k` banks the previous duration as skip, then
                // takes over (last tag in the group wins the duration).
                OverrideTag::KaraokeDuration(d) => {
                    pending.skip_ms = pending.skip_ms.saturating_add(pending.dur_ms);
                    pending.dur_ms = d.saturating_mul(10);
                    pending.kind = Some(KaraokeKind::Hard);
                }
                OverrideTag::KaraokeSweep(d) => {
                    pending.skip_ms = pending.skip_ms.saturating_add(pending.dur_ms);
                    pending.dur_ms = d.saturating_mul(10);
                    pending.kind = Some(KaraokeKind::Sweep);
                }
                OverrideTag::KaraokeOutline(d) => {
                    pending.skip_ms = pending.skip_ms.saturating_add(pending.dur_ms);
                    pending.dur_ms = d.saturating_mul(10);
                    pending.kind = Some(KaraokeKind::Outline);
                }
                OverrideTag::KaraokeTiming { mode, millis } => {
                    pending.skip_ms = pending.skip_ms.saturating_add(pending.dur_ms);
                    pending.dur_ms = (*millis).max(0) as u64;
                    pending.kind = Some(match mode {
                        1 => KaraokeKind::Sweep,
                        2 => KaraokeKind::Outline,
                        _ => KaraokeKind::Hard,
                    });
                }
                OverrideTag::Transform { tags, .. } => {
                    for nested in tags {
                        consume(nested, pending);
                    }
                }
                _ => {}
            }
        }
        for tag in &segment.tags[from..] {
            consume(tag, &mut pending);
        }
        prev_tag_count = segment.tags.len();

        let item = match items.get(seg_idx) {
            Some(item) => item,
            None => continue,
        };
        let emits = !segment.text.is_empty() || item.drawing.is_some();
        if !emits {
            // Tag carrier: state passes through uncleared.
            continue;
        }
        let mut first_piece = true;
        let mut take_lead = || {
            if first_piece {
                first_piece = false;
                std::mem::take(&mut pending)
            } else {
                KaraokeLead::default()
            }
        };

        if item.drawing.is_some() {
            let width = clean_width(item.drawing.as_ref().map(|d| d.width).unwrap_or(0.0));
            let pid = pieces.len();
            pieces.push(RunPiece {
                seg_idx,
                line_y: None,
                event_line,
                width,
                trim_front: 0.0,
                trim_back: 0.0,
                lead: take_lead(),
                is_drawing: true,
            });
            seg_pieces[seg_idx].push(pid);
        } else {
            // Group shaped glyphs by row (exact `y`, like the render
            // loop's row marker).
            let mut rows: Vec<(f64, Vec<usize>)> = Vec::new();
            for (glyph_idx, glyph) in item.shaped.glyphs.iter().enumerate() {
                match rows.last_mut() {
                    Some((y, idxs)) if *y == glyph.y => idxs.push(glyph_idx),
                    _ => rows.push((glyph.y, vec![glyph_idx])),
                }
            }
            // A leading break means the state rides an empty first
            // piece (the `\n` glyph carries it in libass).
            if segment.text.starts_with('\n') {
                let pid = pieces.len();
                pieces.push(RunPiece {
                    seg_idx,
                    line_y: None,
                    event_line,
                    width: 0.0,
                    trim_front: 0.0,
                    trim_back: 0.0,
                    lead: take_lead(),
                    is_drawing: false,
                });
                seg_pieces[seg_idx].push(pid);
            }
            let leading = segment.text.chars().take_while(|c| *c == '\n').count();
            event_line = event_line.saturating_add(leading);
            for (row_pos, (y, idxs)) in rows.iter().enumerate() {
                if row_pos > 0 {
                    event_line = event_line.saturating_add(1);
                }
                // Pen width mirrors the render loop's `row_pen`: max
                // edge over rendered (non-zero-scale) glyphs.
                let mut pen = 0.0f64;
                for &glyph_idx in idxs {
                    let glyph = &item.shaped.glyphs[glyph_idx];
                    if glyph.scale_x > 0.0 && glyph.scale_y > 0.0 {
                        let edge = glyph.x + glyph.advance;
                        if edge.is_finite() && edge > pen {
                            pen = edge;
                        }
                    }
                }
                // Visible span trims ASCII spaces (libass
                // `IS_WHITESPACE`: space and newline only).
                let mut front = pen;
                let mut back = 0.0f64;
                let mut first_visible: Option<f64> = None;
                let mut last_visible_end = 0.0f64;
                for &glyph_idx in idxs {
                    let glyph = &item.shaped.glyphs[glyph_idx];
                    if glyph.ch != ' ' {
                        if first_visible.is_none() {
                            first_visible = Some(glyph.x);
                        }
                        let end = glyph.x + glyph.advance;
                        if end.is_finite() {
                            last_visible_end = last_visible_end.max(end);
                        }
                    }
                }
                if let Some(start) = first_visible {
                    if start.is_finite() {
                        front = start.max(0.0).min(pen);
                    }
                    back = (pen - last_visible_end).max(0.0);
                }
                let pid = pieces.len();
                pieces.push(RunPiece {
                    seg_idx,
                    line_y: Some(*y),
                    event_line,
                    width: clean_width(pen),
                    trim_front: clean_width(front),
                    trim_back: clean_width(back),
                    lead: take_lead(),
                    is_drawing: false,
                });
                seg_pieces[seg_idx].push(pid);
            }
        }
        if segment.text.ends_with('\n') {
            event_line = event_line.saturating_add(1);
        }
        // Emitted: any state not taken by a first piece is dropped
        // (a segment always takes it on its first piece, so this only
        // clears when a segment somehow produced no pieces).
        pending = KaraokeLead::default();
    }

    // ---- Pass 2: group pieces into runs (libass run breaks). ----
    let mut run_of_piece: Vec<Option<usize>> = vec![None; pieces.len()];
    let mut run_pieces: Vec<Vec<usize>> = Vec::new();
    let mut last_kind: Option<KaraokeKind> = None;
    for (pid, piece) in pieces.iter().enumerate() {
        let breaks = if pid == 0 {
            true
        } else {
            let prev = &pieces[pid - 1];
            piece.lead.dur_ms > 0
                || (piece.lead.kind.is_some() && piece.lead.kind != last_kind)
                || piece.is_drawing
                || prev.is_drawing
                || piece.event_line != prev.event_line
                || !items[piece.seg_idx]
                    .resolved
                    .same_karaoke_run(&items[prev.seg_idx].resolved)
        };
        if piece.lead.kind.is_some() {
            last_kind = piece.lead.kind;
        }
        if breaks || run_pieces.is_empty() {
            run_pieces.push(Vec::new());
        }
        let run_idx = run_pieces.len() - 1;
        run_pieces[run_idx].push(pid);
        run_of_piece[pid] = Some(run_idx);
    }

    // ---- Pass 3: timing per run (`ass_process_karaoke_effects`). ----
    let mut runs: Vec<KaraokeRun> = Vec::new();
    let mut run_id_of_group: Vec<Option<usize>> = vec![None; run_pieces.len()];
    let mut clock = 0u64;
    let mut skip_accum = 0u64;
    let mut effect: Option<KaraokeKind> = None;
    let mut has_reset = false;
    for (group_idx, group) in run_pieces.iter().enumerate() {
        let start_lead = pieces[group[0]].lead;
        if start_lead.kind.is_some() {
            effect = start_lead.kind;
        }
        // Fold non-start pieces' state (persists even through runs
        // without karaoke, exactly like upstream's skip_timing).
        let mut fold_piece = |lead: KaraokeLead| {
            if lead.reset {
                has_reset = true;
                skip_accum = 0;
            }
            skip_accum = skip_accum.saturating_add(lead.skip_ms);
        };
        if effect.is_none() {
            for &pid in &group[1..] {
                fold_piece(pieces[pid].lead);
            }
            continue;
        }
        if start_lead.reset {
            clock = 0;
        }
        let tm_start = clock.saturating_add(start_lead.skip_ms);
        let tm_end = tm_start.saturating_add(start_lead.dur_ms);
        for &pid in &group[1..] {
            fold_piece(pieces[pid].lead);
        }
        clock = (if has_reset { 0 } else { tm_end }).saturating_add(skip_accum);
        has_reset = false;
        skip_accum = 0;

        let kind = effect.unwrap_or(KaraokeKind::Hard);
        let end_eff = if kind == KaraokeKind::Sweep {
            tm_end
        } else {
            tm_start
        };
        let span: f64 = group.iter().map(|&pid| pieces[pid].width).sum::<f64>() + 0.0;
        let front = pieces[group[0]].trim_front;
        let back = pieces[group[group.len() - 1]].trim_back;
        let span = clean_width(span - front - back);
        let angle = items[pieces[group[0]].seg_idx].resolved.angle;
        // Euclidean modulo: equivalent rotations (e.g. -170 and 190)
        // flip identically.
        let frz = angle.rem_euclid(360.0);
        let flip = frz > 90.0 && frz < 270.0;

        // Last member glyph (for sweep-buffer flushing): scan member
        // pieces in reverse for the last text piece with glyphs.
        let mut last_glyph = None;
        for &pid in group.iter().rev() {
            let piece = &pieces[pid];
            if let Some(y) = piece.line_y {
                if let Some(item) = items.get(piece.seg_idx) {
                    if let Some((glyph_idx, _)) = item
                        .shaped
                        .glyphs
                        .iter()
                        .enumerate()
                        .rev()
                        .find(|(_, glyph)| glyph.y == y)
                    {
                        last_glyph = Some((piece.seg_idx, glyph_idx));
                        break;
                    }
                }
            }
        }

        // First member glyph (for shear resets): scan member
        // pieces forward for the first text piece with glyphs.
        let mut first_glyph = None;
        for &pid in group.iter() {
            let piece = &pieces[pid];
            if let Some(y) = piece.line_y {
                if let Some(item) = items.get(piece.seg_idx) {
                    if let Some((glyph_idx, _)) = item
                        .shaped
                        .glyphs
                        .iter()
                        .enumerate()
                        .find(|(_, glyph)| glyph.y == y)
                    {
                        first_glyph = Some((piece.seg_idx, glyph_idx));
                        break;
                    }
                }
            }
        }

        let run_id = runs.len();
        run_id_of_group[group_idx] = Some(run_id);
        runs.push(KaraokeRun {
            kind,
            start_ms: tm_start,
            end_ms: end_eff,
            span,
            flip,
            sweep: kind == KaraokeKind::Sweep && tm_end > tm_start,
            last_glyph,
            first_glyph,
        });
    }

    // ---- Pass 4: glyph/drawing assignment. ----
    let mut glyph_run: Vec<Vec<Option<usize>>> = segments
        .iter()
        .enumerate()
        .map(|(seg_idx, _)| {
            items
                .get(seg_idx)
                .map(|item| vec![None; item.shaped.glyphs.len()])
                .unwrap_or_default()
        })
        .collect();
    let mut drawing_run: Vec<Option<usize>> = vec![None; segments.len()];
    for (pid, piece) in pieces.iter().enumerate() {
        let Some(group_idx) = run_of_piece[pid] else {
            continue;
        };
        let run_id = run_id_of_group[group_idx];
        if piece.is_drawing {
            drawing_run[piece.seg_idx] = run_id;
        } else if let Some(y) = piece.line_y {
            if let Some(item) = items.get(piece.seg_idx) {
                for (glyph_idx, glyph) in item.shaped.glyphs.iter().enumerate() {
                    if glyph.y == y {
                        glyph_run[piece.seg_idx][glyph_idx] = run_id;
                    }
                }
            }
        }
    }

    (runs, glyph_run, drawing_run)
}

/// Measured vector drawing for one segment, in video pixels.
/// `min_x` is the ink's left bearing (libass preserves it: ink at
/// pen + min); the advance is `width` and the box hangs `height`
/// above the baseline. `min_y` needs no field: ink lands at
/// origin + y with the origin one height above the baseline.
#[derive(Debug, Clone)]
struct DrawingLayout {
    mode: i32,
    min_x: f64,
    width: f64,
    height: f64,
    baseline: f64,
}

/// One face in a segment's fallback chain (primary first): the
/// stable font id plus the faux synthesis its glyphs require.
#[derive(Debug, Clone, Copy)]
struct LayoutFace {
    id: usize,
    faux_bold: bool,
    faux_italic: bool,
}

/// One laid-out segment: resolved style, shaped glyphs, fallback faces,
/// and optional drawing geometry.
struct LayoutItem {
    resolved: ResolvedStyle,
    shaped: crate::renderer::shaper::ShapedLine,
    faces: Vec<LayoutFace>,
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

/// Advance the cumulative `\fay` baseline shear by one glyph (or
/// drawing): `fay * scale_y / scale_x * advance` (libass
/// `apply_baseline_shear`). Non-finite increments (degenerate scales
/// or advances) are ignored so one bad value cannot poison the rest
/// of the line; callers reset the accumulator at line breaks.
fn accumulate_fay_shear(accum: &mut f64, shear_y: f64, scale_x: f64, scale_y: f64, advance: f64) {
    if shear_y == 0.0 {
        return;
    }
    if !shear_y.is_finite() || !scale_x.is_finite() || !scale_y.is_finite() || !advance.is_finite()
    {
        return;
    }
    if scale_x.abs() < 1e-9 {
        return;
    }
    let inc = shear_y * scale_y / scale_x * advance;
    if inc.is_finite() {
        *accum += inc;
    }
}

/// True laid-out width of every event line, in render order. Mirrors
/// the render loop exactly — skipped segments contribute nothing,
/// mid-segment shaper rows open new lines (zero-scale glyphs excluded,
/// like the renderer's row marker), and trailing breaks close the line
/// — so entry `i` is the width of the line the renderer calls `i`.
/// First row group of a segment continues the current line (segments
/// never start mid-line content elsewhere); leading breaks open lines.
fn event_line_widths(segments: &[TextSegment], items: &[LayoutItem]) -> Vec<f64> {
    let mut lines = vec![0.0_f64];
    for (segment, item) in segments.iter().zip(items.iter()) {
        if item.skipped {
            continue;
        }
        // Leading breaks open lines (the render loop advances its line
        // index identically). Only '\n': mid-segment '\r' rows are caught
        // by row grouping on both sides; segment-boundary '\r' is ignored
        // by both, matching the existing trailing-break rule below.
        let leading = segment.text.chars().take_while(|c| *c == '\n').count();
        lines.extend(std::iter::repeat_n(0.0, leading));
        if let Some(drawing) = &item.drawing {
            if drawing.width.is_finite() {
                if let Some(last) = lines.last_mut() {
                    *last += drawing.width.max(0.0);
                }
            }
        } else {
            // Row-group widths: `x` restarts at 0 on every shaper row,
            // so each group's width is its furthest `x + advance` edge.
            let mut groups: Vec<f64> = Vec::new();
            let mut row_y: Option<f64> = None;
            let mut row_w = 0.0_f64;
            for glyph in &item.shaped.glyphs {
                if glyph.scale_x <= 0.0 || glyph.scale_y <= 0.0 {
                    continue;
                }
                if row_y != Some(glyph.y) {
                    if row_y.is_some() {
                        groups.push(row_w);
                    }
                    row_y = Some(glyph.y);
                    row_w = 0.0;
                }
                let edge = glyph.x + glyph.advance;
                if edge.is_finite() && edge > row_w {
                    row_w = edge;
                }
            }
            if row_y.is_some() {
                groups.push(row_w);
            }
            let mut groups = groups.into_iter();
            if let Some(w0) = groups.next() {
                if let Some(last) = lines.last_mut() {
                    *last += w0;
                }
            }
            for w in groups {
                lines.push(w);
            }
        }
        if segment.text.ends_with('\n') {
            lines.push(0.0);
        }
    }
    lines
}

/// One event line's box geometry for `BorderStyle=3`: width plus the
/// top offset (relative to the block top) and height.
#[derive(Debug, Clone, Copy)]
struct EventLineBox {
    width: f64,
    y: f64,
    height: f64,
    /// True once a content row (text or drawing) joins the line.
    /// Interior empty lines still draw (they tile the column); a
    /// trailing empty last line is skipped (extent-based, VSFilter
    /// draws no box past the final ink).
    has_content: bool,
}

/// Per-line box geometry, mirroring the render loop's line model (same
/// breaks and shaper rows as [`event_line_widths`]) so each box frames
/// the ink it belongs to:
///
/// * Content rows join the current line (widths add, height takes the
///   max) or open a new one at mid-segment row changes, at their exact
///   shaper y (render truth, gaps included).
/// * Leading breaks append lines (the first fills the pristine initial
///   line); their provisional tops backfill from the segment's first
///   content row so breaks tile exactly.
/// * Interior row gaps synthesize empty lines; a trailing break appends
///   one empty line tiling the previous bottom (libass draws boxes for
///   empty lines too, including trailing ones).
/// * Drawings join at the segment base (the render loop draws them
///   there even when their text holds breaks); their row height for
///   empty-line tiling is the segment's shaped line height.
///
/// Widths match [`event_line_widths`] line for line except for
/// synthesized gap rows (zero width); tops tile without gaps because
/// every render-loop y advance is covered by line heights.
fn event_line_boxes(segments: &[TextSegment], items: &[LayoutItem]) -> Vec<EventLineBox> {
    /// Cap on synthesized gap rows per gap: row gaps are bounded by
    /// the segment's break count in practice; this only bounds float
    /// garbage from reaching the box loop.
    const MAX_GAP_ROWS: i64 = 1_000_000;
    let mut lines = vec![EventLineBox {
        width: 0.0,
        y: 0.0,
        height: 0.0,
        has_content: false,
    }];
    // Pen y of the current segment base (mirrors the render loop's
    // `line_y_offset`).
    let mut rel_y = 0.0f64;
    let clean = |v: f64| {
        if v.is_finite() {
            v.max(0.0)
        } else {
            0.0
        }
    };
    for (segment, item) in segments.iter().zip(items.iter()) {
        if item.skipped {
            continue;
        }
        let seg_base = rel_y;
        let lh = clean(item.shaped.line_height);
        // Leading breaks: the first fills the pristine initial line,
        // the rest append (matching `event_line_widths` counts).
        let leading = segment.text.chars().take_while(|c| *c == '\n').count();
        // Appended leading lines start here (the filled initial line,
        // when pristine, is slot 0 and never backfilled).
        let lead_start = lines.len();
        if leading > 0 {
            let pristine = lines.len() == 1 && lines[0].width == 0.0 && lines[0].height == 0.0;
            if pristine {
                lines[0].height = lh;
            }
            for k in 0..leading {
                // Provisional tops tile forward; content backfills.
                let slot = if pristine { k + 1 } else { k };
                lines.push(EventLineBox {
                    width: 0.0,
                    y: seg_base + slot as f64 * lh,
                    height: lh,
                    has_content: false,
                });
            }
        }
        // Content rows `(y, width, height, is_gap)`: drawings are
        // atomic (single row at the segment base); text rows group
        // exactly like `event_line_widths`.
        let mut rows: Vec<(f64, f64, f64, bool)> = Vec::new();
        if let Some(drawing) = &item.drawing {
            rows.push((0.0, clean(drawing.width), clean(drawing.height), false));
        } else {
            let mut row_y: Option<f64> = None;
            let mut row_w = 0.0f64;
            let mut prev_y: Option<f64> = None;
            let flush_row = |rows: &mut Vec<(f64, f64, f64, bool)>,
                             prev_y: &mut Option<f64>,
                             y: f64,
                             w: f64| {
                // Synthesize empty slots for skipped row indices.
                if lh > 0.0 {
                    if let Some(py) = *prev_y {
                        let diff = y - py;
                        if diff.is_finite() && diff > 0.0 {
                            let ratio = diff / lh;
                            if ratio.is_finite() {
                                let missing = (ratio.round() as i64).clamp(0, MAX_GAP_ROWS) - 1;
                                for m in 1..=missing {
                                    rows.push((py + m as f64 * lh, 0.0, lh, true));
                                }
                            }
                        }
                    }
                }
                rows.push((y, w, lh, false));
                *prev_y = Some(y);
            };
            for glyph in &item.shaped.glyphs {
                if glyph.scale_x <= 0.0 || glyph.scale_y <= 0.0 {
                    continue;
                }
                match row_y {
                    Some(y) if y == glyph.y => {}
                    _ => {
                        if let Some(y) = row_y {
                            flush_row(&mut rows, &mut prev_y, y, row_w);
                        }
                        row_y = Some(glyph.y);
                        row_w = 0.0;
                    }
                }
                let edge = glyph.x + glyph.advance;
                if edge.is_finite() && edge > row_w {
                    row_w = edge;
                }
            }
            if let Some(y) = row_y {
                flush_row(&mut rows, &mut prev_y, y, row_w);
            }
        }
        // First content row joins the current line (overwriting its y
        // with render truth) and backfills leading empties; later rows
        // open new lines.
        let mut first_row = true;
        for (row_y, row_w, row_h, is_gap) in rows {
            let exact_y = seg_base + row_y;
            if first_row {
                first_row = false;
                // Backfill leading slots from the content row so breaks
                // tile exactly (the last appended line takes content).
                if leading > 0 && lh > 0.0 {
                    for k in 0..leading.saturating_sub(1) {
                        if let Some(slot) = lines.get_mut(lead_start + k) {
                            slot.y = exact_y - (leading - 1 - k) as f64 * lh;
                            slot.height = lh;
                        }
                    }
                }
                if let Some(last) = lines.last_mut() {
                    last.y = exact_y;
                    last.height = last.height.max(row_h);
                    last.width += row_w;
                    if !is_gap {
                        last.has_content = true;
                    }
                }
            } else {
                lines.push(EventLineBox {
                    width: row_w,
                    y: exact_y,
                    height: row_h,
                    // Gap-synthesis rows carry no ink but tile the
                    // column; only real content rows mark the line.
                    has_content: !is_gap,
                });
            }
        }
        if segment.text.ends_with('\n') {
            // Advance past the whole segment (mirrors the render loop),
            // then open one empty line tiling the previous bottom.
            let adv = if let Some(drawing) = &item.drawing {
                clean(drawing.height)
            } else {
                clean(item.shaped.height)
            };
            rel_y += adv;
            let (y, h) = match lines.last() {
                Some(last) => (last.y + last.height, lh),
                None => (rel_y, lh),
            };
            lines.push(EventLineBox {
                width: 0.0,
                y,
                height: h,
                has_content: false,
            });
        }
    }
    lines
}

/// Horizontal inset of one line inside the event block for the
/// event-level alignment (libass aligns each line independently:
/// short lines center/right-align on their own width, they do not
/// hug the block edge). `block_width` must be the same width the
/// block origin was computed from. Unknown alignments center, like
/// [`Compositor::calculate_position`].
fn line_align_inset(line_width: f64, block_width: f64, alignment: i32) -> f64 {
    let extra = (block_width - line_width).max(0.0);
    if !extra.is_finite() {
        return 0.0;
    }
    match alignment {
        1 | 4 | 7 => 0.0,
        3 | 6 | 9 => extra,
        _ => extra / 2.0,
    }
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
        let p = f64::from(v) * s;
        if !p.is_finite() {
            return v;
        }
        finite_to_i32(p.clamp(f64::from(i32::MIN), f64::from(i32::MAX))).unwrap_or(v)
    };
    (
        conv(rect.0, scale_x),
        conv(rect.1, scale_y),
        conv(rect.2, scale_x),
        conv(rect.3, scale_y),
    )
}

/// Decoration bar rows for one glyph, in scaled-bitmap pixels:
/// `(top, bottom)` per active bar, relative to the bitmap origin.
/// libass `ass_get_glyph_outline` centers each bar on its font-metric
/// position about the pen: underline `|pos|` below the baseline,
/// strikeout `pos` above. Returns an empty vec when the font lacks
/// metrics or the em size is degenerate (libass draws no bar then).
fn deco_bar_rows(
    underline: bool,
    strikeout: bool,
    metrics: DecorationMetrics,
    pen_y: f64,
    em_px: f64,
) -> Vec<(f64, f64)> {
    let mut rows = Vec::new();
    let upm = f64::from(metrics.units_per_em);
    if upm <= 0.0 || !em_px.is_finite() || em_px <= 0.0 || !pen_y.is_finite() {
        return rows;
    }
    if underline {
        if let Some((pos, thick)) = metrics.underline {
            let center = pen_y + f64::from(pos.unsigned_abs()) / upm * em_px;
            let half = f64::from(thick) / upm * em_px / 2.0;
            if half.is_finite() && half > 0.0 && center.is_finite() {
                rows.push((center - half, center + half));
            }
        }
    }
    if strikeout {
        if let Some((pos, size)) = metrics.strikeout {
            let center = pen_y - f64::from(pos) / upm * em_px;
            let half = f64::from(size) / upm * em_px / 2.0;
            if half.is_finite() && half > 0.0 && center.is_finite() {
                rows.push((center - half, center + half));
            }
        }
    }
    rows
}

/// Paint one decoration bar into a coverage bitmap with box-AA: every
/// overlapped row/column gets proportional coverage, merged via max
/// so bars add ink without erasing glyph pixels. Bounds are
/// fractional bitmap pixels and clamp to the bitmap.
fn paint_deco_bar(bitmap: &mut [u8], w: u32, h: u32, x0: f64, x1: f64, y0: f64, y1: f64) {
    if w == 0 || h == 0 {
        return;
    }
    if !x0.is_finite() || !x1.is_finite() || !y0.is_finite() || !y1.is_finite() {
        return;
    }
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let (w_f, h_f) = (f64::from(w), f64::from(h));
    let (xa, xb) = (x0.clamp(0.0, w_f), x1.clamp(0.0, w_f));
    let (ya, yb) = (y0.clamp(0.0, h_f), y1.clamp(0.0, h_f));
    if xb <= xa || yb <= ya {
        return;
    }
    let (ix0, ix1) = (xa.floor() as i64, xb.ceil() as i64);
    let (iy0, iy1) = (ya.floor() as i64, yb.ceil() as i64);
    for yy in iy0..iy1 {
        if yy < 0 || yy >= i64::from(h) {
            continue;
        }
        let cover_y = (yb.min(yy as f64 + 1.0) - ya.max(yy as f64)).clamp(0.0, 1.0);
        if cover_y <= 0.0 {
            continue;
        }
        for xx in ix0..ix1 {
            if xx < 0 || xx >= i64::from(w) {
                continue;
            }
            let cover_x = (xb.min(xx as f64 + 1.0) - xa.max(xx as f64)).clamp(0.0, 1.0);
            let cover = cover_x * cover_y;
            if cover <= 0.0 {
                continue;
            }
            let idx = (yy as u32 * w + xx as u32) as usize;
            if let Some(px) = bitmap.get_mut(idx) {
                *px = (*px).max((cover * 255.0).round().clamp(0.0, 255.0) as u8);
            }
        }
    }
}

/// Active drawing mode for a segment: the last `\pN` in its tags,
/// falling back to the event-level mode. `\r` is transparent here:
/// libass `ass_reset_render_context` never touches `drawing_scale`,
/// so only an explicit `\p0` exits drawing mode.
fn segment_drawing_mode(tags: &[OverrideTag], event_mode: i32) -> i32 {
    for tag in tags.iter().rev() {
        if let OverrideTag::Drawing(m) = tag {
            return *m;
        }
    }
    event_mode
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

    /// Apply accel function: libass `pow(t, accel)` exactly (accel 1
    /// is linear, accel > 1 starts slow, accel < 1 starts fast).
    /// Accel 0 therefore applies instantly (`pow(t, 0) == 1`) and
    /// negative accel extrapolates beyond 1 while finite, like libass.
    /// Non-finite accel falls back to linear (libass would propagate
    /// NaN); a non-finite result saturates to 1.0.
    fn apply_accel(t: f64, accel: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        if !accel.is_finite() {
            return t;
        }
        let k = t.powf(accel);
        if k.is_finite() {
            k
        } else {
            1.0
        }
    }

    /// Compute a `\t(...)` progress value against the event clock. A zero
    /// `t2` means the event end, matching libass's transform timing rules.
    fn transform_progress(
        t1: i32,
        t2: i32,
        accel: f64,
        time_ms: u64,
        start_ms: u64,
        end_ms: u64,
    ) -> f64 {
        let elapsed = i64::try_from(time_ms.saturating_sub(start_ms)).unwrap_or(i64::MAX);
        let t1 = i64::from(t1);
        let duration = i64::try_from(end_ms.saturating_sub(start_ms)).unwrap_or(i64::MAX);
        let t2_eff = if t2 == 0 { duration } else { i64::from(t2) };
        let raw_progress = if elapsed < t1 {
            0.0
        } else if elapsed >= t2_eff || t2_eff <= t1 {
            1.0
        } else {
            (elapsed - t1) as f64 / (t2_eff - t1) as f64
        };
        if elapsed < t1 {
            0.0
        } else {
            Self::apply_accel(raw_progress, accel)
        }
    }

    /// Apply `\t(...)` inner tags with a given progress (0.0 to 1.0).
    ///
    /// Transformability matrix (per Aegisub/VSFilter: `\t` animates
    /// continuous style properties by interpolation):
    ///
    /// ```text
    /// animated:  \c \1c \2c \3c \4c \alpha \1a \2a \3a \4a
    ///            \fs (absolute lerps; \fs+N/-N scales by 1+p*d/10)
    ///            \fscx \fscy \fsp
    ///            \fr \frx \fry \frz \fax \fay
    ///            \bord \xbord \ybord \shad \xshad \yshad
    ///            \be \blur
    ///            rectangular \clip/\iclip coordinates
    /// consumed inside the transform. Discrete switches, positioning,
    /// fades, drawing state, karaoke, resets, and nested transforms are
    /// handled with libass's non-interpolated semantics.
    /// ```
    ///
    /// Tags without continuous fields still apply their libass discrete
    /// behavior inside `\t`; unsupported tags remain ignored.
    fn apply_transform_tags(
        resolved: &mut ResolvedStyle,
        tags: &[OverrideTag],
        progress: f64,
        time_ms: u64,
        start_ms: u64,
        end_ms: u64,
    ) {
        Self::apply_transform_tags_depth(resolved, tags, progress, 0, time_ms, start_ms, end_ms);
    }

    fn apply_transform_tags_depth(
        resolved: &mut ResolvedStyle,
        tags: &[OverrideTag],
        progress: f64,
        depth: u32,
        time_ms: u64,
        start_ms: u64,
        end_ms: u64,
    ) {
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
                // libass `\t` + `\fs`: absolute targets interpolate
                // linearly; relative deltas scale the current size by
                // (1 + p * d / 10). Non-positive results reset to style.
                OverrideTag::FontSize(s) => {
                    let from = resolved.font_size;
                    let next = from + (s - from) * progress;
                    if next.is_finite() && next > 0.0 {
                        resolved.font_size = next;
                    } else {
                        resolved.font_size = resolved.base_style.font_size;
                    }
                }
                OverrideTag::FontSizeRelative(delta) => {
                    let from = resolved.font_size;
                    let next = from * (1.0 + progress * delta / 10.0);
                    if next.is_finite() && next > 0.0 {
                        resolved.font_size = next;
                    } else {
                        resolved.font_size = resolved.base_style.font_size;
                    }
                }
                OverrideTag::FontSizeReset => {
                    resolved.font_size = resolved.base_style.font_size;
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
                OverrideTag::Clip(x0, y0, x1, y1) | OverrideTag::InverseClip(x0, y0, x1, y1) => {
                    // Rectangular clips are interpolated coordinate by
                    // coordinate by libass. The default clip is the full
                    // PlayRes canvas used by the renderer's default style.
                    let from = resolved.clip.or(resolved.inverse_clip).unwrap_or((
                        0,
                        0,
                        resolved.clip_canvas.0,
                        resolved.clip_canvas.1,
                    ));
                    let lerp = |a: i32, b: i32| {
                        // libass converts the interpolated ASS coordinate
                        // with an integer cast (truncate toward zero), not
                        // with mathematical rounding.
                        (f64::from(a) * (1.0 - progress) + f64::from(b) * progress) as i32
                    };
                    let rect = (
                        lerp(from.0, *x0),
                        lerp(from.1, *y0),
                        lerp(from.2, *x1),
                        lerp(from.3, *y1),
                    );
                    if matches!(target, OverrideTag::Clip(..)) {
                        resolved.clip = Some(rect);
                        resolved.inverse_clip = None;
                    } else {
                        resolved.inverse_clip = Some(rect);
                        resolved.clip = None;
                    }
                }
                OverrideTag::Transform {
                    t1,
                    t2,
                    accel,
                    tags: nested,
                } => {
                    // Nested transforms are recursively applied, with the
                    // parser/runtime depth caps preventing unbounded work.
                    if depth < 32 {
                        let nested_progress =
                            Self::transform_progress(*t1, *t2, *accel, time_ms, start_ms, end_ms);
                        Self::apply_transform_tags_depth(
                            resolved,
                            nested,
                            nested_progress,
                            depth + 1,
                            time_ms,
                            start_ms,
                            end_ms,
                        );
                    }
                }
                // Discrete and event-global tags are consumed inside `\t`.
                _ => Self::apply_single_tag(resolved, target),
            }
        }
    }

    /// Apply only the event-global portion of a transform.  The normal
    /// segment resolver also walks transform tags, but event-global tags
    /// must be resolved before wrapping and block geometry are computed.
    /// Keeping this filtered pass separate prevents a `\t(\fs...)` from
    /// being applied once to the event and again to each segment.
    fn apply_event_global_transform_tags(
        resolved: &mut ResolvedStyle,
        tags: &[OverrideTag],
        progress: f64,
        depth: u32,
        time_ms: u64,
        start_ms: u64,
        end_ms: u64,
    ) {
        for target in tags {
            match target {
                OverrideTag::Transform {
                    t1,
                    t2,
                    accel,
                    tags: nested,
                } if depth < 32 => {
                    let nested_progress =
                        Self::transform_progress(*t1, *t2, *accel, time_ms, start_ms, end_ms);
                    Self::apply_event_global_transform_tags(
                        resolved,
                        nested,
                        nested_progress,
                        depth + 1,
                        time_ms,
                        start_ms,
                        end_ms,
                    );
                }
                OverrideTag::Transform { .. } => {}
                OverrideTag::Clip(x0, y0, x1, y1) | OverrideTag::InverseClip(x0, y0, x1, y1) => {
                    let from = resolved.clip.or(resolved.inverse_clip).unwrap_or((
                        0,
                        0,
                        resolved.clip_canvas.0,
                        resolved.clip_canvas.1,
                    ));
                    let lerp = |a: i32, b: i32| {
                        (f64::from(a) * (1.0 - progress) + f64::from(b) * progress) as i32
                    };
                    let rect = (
                        lerp(from.0, *x0),
                        lerp(from.1, *y0),
                        lerp(from.2, *x1),
                        lerp(from.3, *y1),
                    );
                    if matches!(target, OverrideTag::Clip(..)) {
                        resolved.clip = Some(rect);
                        resolved.inverse_clip = None;
                    } else {
                        resolved.inverse_clip = Some(rect);
                        resolved.clip = None;
                    }
                }
                tag if tag.is_event_layout() => Self::apply_single_tag(resolved, tag),
                _ => {}
            }
        }
    }

    /// Segment-local transform walk used after the event-global pass.  It
    /// retains all font, color, geometry, drawing, and karaoke behavior,
    /// while leaving the already-resolved event layout fields untouched.
    fn apply_transform_segment_tags(
        resolved: &mut ResolvedStyle,
        tags: &[OverrideTag],
        progress: f64,
        depth: u32,
        time_ms: u64,
        start_ms: u64,
        end_ms: u64,
    ) {
        for target in tags {
            match target {
                OverrideTag::Transform {
                    t1,
                    t2,
                    accel,
                    tags: nested,
                } if depth < 32 => {
                    let nested_progress =
                        Self::transform_progress(*t1, *t2, *accel, time_ms, start_ms, end_ms);
                    Self::apply_transform_segment_tags(
                        resolved,
                        nested,
                        nested_progress,
                        depth + 1,
                        time_ms,
                        start_ms,
                        end_ms,
                    );
                }
                OverrideTag::Transform { .. } => {}
                tag if !tag.is_event_layout() => Self::apply_transform_tags_depth(
                    resolved,
                    std::slice::from_ref(target),
                    progress,
                    depth,
                    time_ms,
                    start_ms,
                    end_ms,
                ),
                _ => {}
            }
        }
    }

    /// Resolve the event-global state at a frame timestamp.  This is the
    /// render-level counterpart of `resolve_style`: it evaluates all
    /// top-level transforms in source order and exposes the resulting
    /// position, origin, alignment, wrapping, fade, and clip state to the
    /// complete event render.
    fn resolve_event_globals_at_time(
        resolved: &ResolvedStyle,
        event: &Event,
        time_ms: u64,
        start_ms: u64,
        end_ms: u64,
        play_res_x: u32,
        play_res_y: u32,
    ) -> ResolvedStyle {
        let mut frame = resolved.clone();
        frame.clip_canvas = (
            play_res_x.min(i32::MAX as u32) as i32,
            play_res_y.min(i32::MAX as u32) as i32,
        );
        for tag in &event.parsed_tags {
            if let OverrideTag::Transform {
                t1,
                t2,
                accel,
                tags,
            } = tag
            {
                let progress =
                    Self::transform_progress(*t1, *t2, *accel, time_ms, start_ms, end_ms);
                Self::apply_event_global_transform_tags(
                    &mut frame, tags, progress, 0, time_ms, start_ms, end_ms,
                );
            }
        }
        frame.event_globals_applied = true;
        frame
    }

    /// Resolve an event's style with all override tags applied (no animation)
    fn resolve_base_style(base_style: &Style, tags: &[OverrideTag]) -> ResolvedStyle {
        let mut resolved = ResolvedStyle {
            base_style: base_style.clone(),
            font_name: base_style.font_name.clone(),
            font_size: base_style.font_size,
            font_encoding: base_style.encoding,
            color: base_style.primary_color,
            secondary_color: base_style.secondary_color,
            outline_color: base_style.outline_color,
            shadow_color: base_style.back_color,
            back_color: base_style.back_color,
            // Style Bold is boolean (-1/0): bold style means weight 700.
            font_weight: if base_style.bold { 700 } else { 400 },
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
            parsed_alignment: false,
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
            parsed_fade: false,
            wrap_style: None,
            clip_canvas: (384, 288),
            event_globals_applied: false,
            layout_res_x: 0,
            layout_res_y: 0,
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
            OverrideTag::PropertyReset(name) => match name.as_str() {
                "b" => resolved.font_weight = if resolved.base_style.bold { 700 } else { 400 },
                "i" => resolved.italic = resolved.base_style.italic,
                "u" => resolved.underline = resolved.base_style.underline,
                "s" => resolved.strike_out = resolved.base_style.strike_out,
                "fn" => resolved.font_name = resolved.base_style.font_name.clone(),
                "fe" => resolved.font_encoding = resolved.base_style.encoding,
                "fsp" => resolved.spacing = resolved.base_style.spacing,
                "fr" | "frz" => resolved.angle = resolved.base_style.angle,
                "frx" => resolved.rotation_x = 0.0,
                "fry" => resolved.rotation_y = 0.0,
                "fscx" => resolved.scale_x = resolved.base_style.scale_x,
                "fscy" => resolved.scale_y = resolved.base_style.scale_y,
                "fax" => resolved.shear_x = 0.0,
                "fay" => resolved.shear_y = 0.0,
                "bord" => {
                    resolved.outline = resolved.base_style.outline;
                    resolved.outline_x = resolved.outline;
                    resolved.outline_y = resolved.outline;
                }
                "xbord" => resolved.outline_x = resolved.base_style.outline,
                "ybord" => resolved.outline_y = resolved.base_style.outline,
                "shad" => {
                    resolved.shadow = resolved.base_style.shadow;
                    resolved.shadow_x = resolved.shadow;
                    resolved.shadow_y = resolved.shadow;
                }
                "xshad" => resolved.shadow_x = resolved.base_style.shadow,
                "yshad" => resolved.shadow_y = resolved.base_style.shadow,
                "be" | "blur" => resolved.blur = 0.0,
                _ => {}
            },
            OverrideTag::Bold(w) => resolved.font_weight = *w,
            OverrideTag::Italic(v) => resolved.italic = *v,
            OverrideTag::Underline(v) => resolved.underline = *v,
            OverrideTag::StrikeOut(v) => resolved.strike_out = *v,
            OverrideTag::FontName(name) => resolved.font_name = name.clone(),
            // libass `ass_parse.c` (`\fs` branch): absolute sizes assign;
            // a computed size <= 0 (or non-finite) resets to the style.
            OverrideTag::FontSize(size) => {
                if size.is_finite() && *size > 0.0 {
                    resolved.font_size = *size;
                } else {
                    resolved.font_size = resolved.base_style.font_size;
                }
            }
            // Relative `\fs+N/-N`: scale the current size by (1 + d/10).
            OverrideTag::FontSizeRelative(delta) => {
                let next = resolved.font_size * (1.0 + delta / 10.0);
                if next.is_finite() && next > 0.0 {
                    resolved.font_size = next;
                } else {
                    resolved.font_size = resolved.base_style.font_size;
                }
            }
            OverrideTag::FontSizeReset => {
                resolved.font_size = resolved.base_style.font_size;
            }
            OverrideTag::FontEncoding(enc) => resolved.font_encoding = *enc,
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
            // \pos and \move share one first-wins slot (libass
            // `EVENT_POSITIONED`): whichever comes first wins, and later
            // \pos / \move tags are ignored, so both are never set.
            OverrideTag::Position(x, y) => {
                if resolved.position.is_none() && resolved.move_data.is_none() {
                    resolved.position = Some((*x, *y));
                }
            }
            OverrideTag::Move(x1, y1, x2, y2) => {
                if resolved.position.is_none() && resolved.move_data.is_none() {
                    resolved.move_data = Some(MoveData {
                        x1: *x1,
                        y1: *y1,
                        x2: *x2,
                        y2: *y2,
                        t1: 0,
                        t2: 0,
                    });
                }
            }
            OverrideTag::MoveWithTiming(x1, y1, x2, y2, t1, t2) => {
                if resolved.position.is_none() && resolved.move_data.is_none() {
                    resolved.move_data = Some(MoveData {
                        x1: *x1,
                        y1: *y1,
                        x2: *x2,
                        y2: *y2,
                        t1: *t1,
                        t2: *t2,
                    });
                }
            }
            // First \org wins (libass `have_origin`).
            OverrideTag::Origin(x, y) => {
                if resolved.origin.is_none() {
                    resolved.origin = Some((*x, *y));
                }
            }
            // First alignment tag wins (libass `PARSED_A`, shared by
            // \an and legacy \a). Out-of-range values fall back to
            // the style alignment with the slot still consumed.
            OverrideTag::Alignment(a) => {
                if !resolved.parsed_alignment {
                    resolved.parsed_alignment = true;
                    resolved.alignment = if (1..=9).contains(a) {
                        *a
                    } else {
                        resolved.base_style.alignment
                    };
                }
            }
            OverrideTag::AlignmentReset => {
                if !resolved.parsed_alignment {
                    resolved.parsed_alignment = true;
                    resolved.alignment = resolved.base_style.alignment;
                }
            }
            OverrideTag::ScaleX(s) => resolved.scale_x = *s,
            OverrideTag::ScaleY(s) => resolved.scale_y = *s,
            OverrideTag::ScaleReset => {
                resolved.scale_x = resolved.base_style.scale_x;
                resolved.scale_y = resolved.base_style.scale_y;
            }
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
            // \fad and \fade share one first-wins slot (libass
            // `PARSED_FADE`): the first fade tag wins in either order
            // and later ones are ignored, so both forms never coexist.
            OverrideTag::Fade(fi, fo) => {
                if !resolved.parsed_fade {
                    resolved.parsed_fade = true;
                    resolved.fade_in = *fi;
                    resolved.fade_out = *fo;
                }
            }
            OverrideTag::ComplexFade(a1, a2, a3, t1, t2, t3, t4) => {
                if !resolved.parsed_fade {
                    resolved.parsed_fade = true;
                    // Stored full-range like libass: alpha interpolates
                    // in f64 and truncates in `interpolate_alpha`;
                    // out-of-range results clamp at application.
                    resolved.complex_fade = Some(ComplexFade {
                        a1: *a1,
                        a2: *a2,
                        a3: *a3,
                        t1: *t1,
                        t2: *t2,
                        t3: *t3,
                        t4: *t4,
                    });
                }
            }
            // libass keeps rectangular and vector clipping as separate
            // state: a later rect replaces the earlier rect coordinates
            // and `\clip` vs `\iclip` flips the rect mode, while the
            // first vector clip is retained (either form consumes the
            // vector slot) and both rect and vector clips render.
            OverrideTag::Clip(x1, y1, x2, y2) => {
                resolved.clip = Some((*x1, *y1, *x2, *y2));
                resolved.inverse_clip = None;
            }
            OverrideTag::InverseClip(x1, y1, x2, y2) => {
                resolved.inverse_clip = Some((*x1, *y1, *x2, *y2));
                resolved.clip = None;
            }
            OverrideTag::ClipVector { scale, drawing } => {
                if resolved.clip_vector.is_none() && resolved.inverse_clip_vector.is_none() {
                    resolved.clip_vector = Some(VectorClip {
                        scale: *scale,
                        drawing: drawing.clone(),
                    });
                }
            }
            OverrideTag::InverseClipVector { scale, drawing } => {
                if resolved.clip_vector.is_none() && resolved.inverse_clip_vector.is_none() {
                    resolved.inverse_clip_vector = Some(VectorClip {
                        scale: *scale,
                        drawing: drawing.clone(),
                    });
                }
            }
            OverrideTag::Blur(b) => resolved.blur = *b,
            OverrideTag::EdgeBlur(b) => resolved.blur = *b,
            OverrideTag::Drawing(mode) => resolved.drawing_mode = *mode,
            OverrideTag::DrawingBaseline(pbo) => resolved.drawing_baseline_offset = *pbo,
            OverrideTag::WrapStyle(q) => resolved.wrap_style = Some(*q),
            _ => {}
        }
    }

    /// Resolve an event's style with all override tags applied.
    ///
    /// Segment style tags from the initial override groups establish the
    /// defaults, but event-layout tags (\pos, \move, \org, \clip, \iclip,
    /// \fad, \fade, \an, \q) apply no matter where they appear textually:
    /// they are scanned across all segments in textual order, so
    /// `{\pos(100,100)}Hi` and `Hi{\pos(100,100)}` resolve identically,
    /// as do `{\an7}Hi` and `Hi{\an7}`. Repeated tags resolve first-wins
    /// per libass, except rect clips (later coordinates replace earlier
    /// ones) and `\q` (last wins, consumed separately).
    pub fn resolve_style(base_style: &Style, event: &Event) -> ResolvedStyle {
        let segments = parse_text_segments(&event.text);
        let initial_tags = segments
            .first()
            .map(|segment| segment.tags.as_slice())
            .unwrap_or(&[]);
        let mut resolved = Self::resolve_base_style(base_style, initial_tags);

        // Event-layout tags apply regardless of textual placement.
        // Segments carry accumulated tags, so only newly added tags per
        // segment are considered; each first-wins slot keeps the
        // textually first tag, including re-scanned initial tags.
        // (\q has no ResolvedStyle field and is consumed separately from
        // the event tag list; scanning it here is a harmless no-op.)
        let mut prev_tag_count = 0usize;
        for segment in &segments {
            let from = prev_tag_count.min(segment.tags.len());
            prev_tag_count = segment.tags.len();
            for tag in &segment.tags[from..] {
                if tag.is_event_layout() {
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
        // Tags are evaluated in source order. This matters for libass's
        // first-wins position/origin slots when a tag is inside `\t`.
        for tag in &segment.tags {
            if let OverrideTag::Transform {
                t1,
                t2,
                accel,
                tags,
            } = tag
            {
                let progress =
                    Self::transform_progress(*t1, *t2, *accel, time_ms, start_ms, end_ms);
                if segment_resolved.event_globals_applied {
                    Self::apply_transform_segment_tags(
                        &mut segment_resolved,
                        tags,
                        progress,
                        0,
                        time_ms,
                        start_ms,
                        end_ms,
                    );
                } else {
                    Self::apply_transform_tags(
                        &mut segment_resolved,
                        tags,
                        progress,
                        time_ms,
                        start_ms,
                        end_ms,
                    );
                }
            } else if let OverrideTag::Reset(style_name) = tag {
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
            let font_match = font_manager.find_font_with_weight(
                &seg_resolved.font_name,
                seg_resolved.font_weight,
                seg_resolved.italic,
            );
            let seg_font_size =
                seg_resolved.font_size * (video_height as f64 / play_res_y.max(1) as f64);
            // Per-glyph fallback chain (primary first): characters the
            // primary lacks cascade to the next loaded face.
            let chain = font_manager.fallback_chain(font_match.id);
            let mut faces = Vec::with_capacity(chain.len());
            let mut shape_fonts: Vec<(usize, &FontArc)> = Vec::with_capacity(chain.len());
            let mut opentype_fonts: Vec<ShapingFont<'_>> = Vec::with_capacity(chain.len());
            for id in chain {
                let (faux_bold, faux_italic) =
                    font_manager.faux_for(id, seg_resolved.font_weight, seg_resolved.italic);
                faces.push(LayoutFace {
                    id,
                    faux_bold,
                    faux_italic,
                });
                if let Some(face) = font_manager.get_font(id) {
                    shape_fonts.push((id, face));
                    if let Some((data, face_index)) = font_manager.shaping_data(id) {
                        opentype_fonts.push(ShapingFont {
                            id,
                            raster: face,
                            data,
                            face_index,
                        });
                    }
                }
            }
            let shaped_text =
                TextShaper::decode_font_encoding(&segment.text, seg_resolved.font_encoding);
            let shaped = if opentype_fonts.is_empty() {
                TextShaper::shape_with_fallback(
                    &shaped_text,
                    &shape_fonts,
                    seg_font_size,
                    seg_resolved.scale_x / 100.0,
                    seg_resolved.scale_y / 100.0,
                    seg_resolved.font_weight,
                    seg_resolved.italic,
                    seg_resolved.spacing,
                    seg_resolved.color,
                    seg_resolved.outline_color,
                    seg_resolved.shadow_color,
                    seg_resolved.angle,
                )
            } else {
                TextShaper::shape_with_opentype(
                    &shaped_text,
                    &opentype_fonts,
                    seg_font_size,
                    seg_resolved.scale_x / 100.0,
                    seg_resolved.scale_y / 100.0,
                    seg_resolved.font_weight,
                    seg_resolved.italic,
                    seg_resolved.spacing,
                    seg_resolved.color,
                    seg_resolved.outline_color,
                    seg_resolved.shadow_color,
                    seg_resolved.angle,
                )
            };
            // Per-segment drawing state (mixed drawing/text supported).
            let mode = segment_drawing_mode(&segment.tags, resolved.drawing_mode);
            let drawing = if !skipped && mode > 0 {
                let unit = drawing_unit_scale(scale_x, scale_y, mode);
                super::drawing::DrawingParser::measure(&segment.text).map(|(min_x, _, w, h)| {
                    DrawingLayout {
                        mode,
                        min_x: min_x * unit,
                        width: w * unit,
                        height: h * unit,
                        // `\\pbo` is an authored baseline offset.  Keep
                        // negative and oversized values: libass lets
                        // the drawing move outside its nominal ink box.
                        baseline: h - seg_resolved.drawing_baseline_offset * unit,
                    }
                })
            } else {
                None
            };
            items.push(LayoutItem {
                resolved: seg_resolved,
                shaped,
                faces,
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
                Some(d) => (d.width, d.height, d.baseline),
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
        let frame_resolved = Self::resolve_event_globals_at_time(
            resolved, event, time_ms, start_ms, end_ms, play_res_x, play_res_y,
        );

        if time_ms < start_ms || time_ms >= end_ms {
            return;
        }

        // Scroll effects always clip to their band, and banner/scroll
        // fadeaways scale edge alphas: both mutate the whole buffer, so
        // they need an isolated event buffer like \clip and \blur.
        // A fade-free banner only moves text and renders direct.
        let effect_needs_isolation = match LegacyEffect::parse(&event.effect) {
            Some(LegacyEffect::ScrollUp { .. } | LegacyEffect::ScrollDown { .. }) => true,
            Some(LegacyEffect::Banner { fadeaway, .. }) => fadeaway > 0.0,
            None => false,
        };
        if frame_resolved.clip.is_some()
            || frame_resolved.inverse_clip.is_some()
            || frame_resolved.clip_vector.is_some()
            || frame_resolved.inverse_clip_vector.is_some()
            || frame_resolved.blur > 0.0
            || effect_needs_isolation
        {
            match RenderBuffer::new(video_width, video_height) {
                Ok(mut event_buffer) => {
                    self.composite_event_inner(
                        &mut event_buffer,
                        event,
                        &frame_resolved,
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
                        &frame_resolved,
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
                &frame_resolved,
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

        // Find font (+ fallback chain for measurement/shaping).
        let font_match = font_manager.find_font_with_weight(
            &resolved.font_name,
            resolved.font_weight,
            resolved.italic,
        );
        let font = font_match.font;
        let measure_chain: Vec<&FontArc> = font_manager
            .fallback_chain(font_match.id)
            .iter()
            .filter_map(|id| font_manager.get_font(*id))
            .collect();
        let font_size = resolved.font_size * (video_height as f64 / play_res_y as f64);

        let mut opentype_measure_fonts = Vec::with_capacity(measure_chain.len());
        for id in font_manager.fallback_chain(font_match.id) {
            if let (Some(raster), Some((data, face_index))) =
                (font_manager.get_font(id), font_manager.shaping_data(id))
            {
                opentype_measure_fonts.push(ShapingFont {
                    id,
                    raster,
                    data,
                    face_index,
                });
            }
        }

        let scale_x = video_width as f64 / play_res_x as f64;
        let scale_y = video_height as f64 / play_res_y as f64;
        let layout_res_x = if resolved.layout_res_x == 0 {
            play_res_x.max(1)
        } else {
            resolved.layout_res_x
        };
        let layout_res_y = if resolved.layout_res_y == 0 {
            play_res_y.max(1)
        } else {
            resolved.layout_res_y
        };
        let blur_scale_x = video_width as f64 / layout_res_x as f64;
        let blur_scale_y = video_height as f64 / layout_res_y as f64;

        // Legacy scroll effect (Banner/Scroll up/Scroll down), if any.
        // Parsed once and reused for wrap, positioning, and clipping.
        let legacy_effect = LegacyEffect::parse(&event.effect);
        // Effective wrap style: a per-event \q overrides the script default.
        // Banner forces no-wrap (VSFilter sets wrapStyle 2 for banners).
        let wrap_style = if matches!(legacy_effect, Some(LegacyEffect::Banner { .. })) {
            2
        } else {
            // `\q` is part of the frame-resolved event state, including
            // when it is nested inside `\t`.  Invalid values fall back to
            // the track default just as the parser/render context does.
            resolved
                .wrap_style
                .filter(|q| (0..=3).contains(q))
                .unwrap_or(script_wrap_style)
        };
        let wrap_width =
            (play_res_x as f64 - resolved.margin_l as f64 - resolved.margin_r as f64) * scale_x;
        // Drawing runs pass through the wrapper verbatim (never wrapped).
        let wrap_input = TextShaper::decode_font_encoding(&event.text, resolved.font_encoding);
        let wrapped_text = if opentype_measure_fonts.is_empty() {
            wrap_event_text(
                &wrap_input,
                wrap_style,
                wrap_width,
                &measure_chain,
                font_size,
                resolved.spacing,
            )
        } else {
            let measure = |value: &str| {
                TextShaper::measure_text_with_opentype(
                    value,
                    &opentype_measure_fonts,
                    font_size,
                    resolved.scale_x / 100.0,
                    resolved.spacing,
                )
            };
            wrap_event_text_with_measure(&wrap_input, wrap_style, wrap_width, &measure)
        };

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

        // Apply move animation (libass `complex_tag("move")`
        // evaluation): `t1 <= 0 && t2 <= 0` — including the untimed
        // 4-arg form — animates across the whole event; otherwise the
        // window is literal, the start instant belongs to (x1,y1)
        // (`t <= t1`), and equal nonzero times are an instant step.
        // The parser swaps reversed times, and the branch structure
        // below cannot divide by zero or underflow even if it didn't
        // (the lerp runs only when `t1 < t2` is proven).
        if let Some(ref move_data) = resolved.move_data {
            let elapsed = i64::try_from(time_ms.saturating_sub(start_ms)).unwrap_or(i64::MAX);
            let duration = i64::try_from(end_ms.saturating_sub(start_ms)).unwrap_or(i64::MAX);
            let (t1, t2) = if move_data.t1 <= 0 && move_data.t2 <= 0 {
                (0, duration)
            } else {
                (i64::from(move_data.t1), i64::from(move_data.t2))
            };

            let t = if elapsed <= t1 {
                0.0
            } else if elapsed >= t2 {
                1.0
            } else {
                (elapsed - t1) as f64 / (t2 - t1) as f64
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

        // Legacy scroll effects override position on their axis (VSFilter
        // `fPosOverride`, applied after \move). The other axis keeps its
        // laid-out (or moved) position.
        match legacy_effect {
            Some(LegacyEffect::Banner {
                delay,
                left_to_right,
                ..
            }) => {
                base_x = LegacyEffect::banner_x(
                    time_ms - start_ms,
                    delay,
                    scale_x,
                    left_to_right,
                    0.0,
                    video_width as f64,
                    layout.width,
                );
            }
            Some(
                LegacyEffect::ScrollUp {
                    top, bottom, delay, ..
                }
                | LegacyEffect::ScrollDown {
                    top, bottom, delay, ..
                },
            ) => {
                let down = matches!(legacy_effect, Some(LegacyEffect::ScrollDown { .. }));
                let text_top = LegacyEffect::scroll_top(
                    time_ms - start_ms,
                    delay,
                    scale_y,
                    down,
                    top * scale_y,
                    bottom * scale_y,
                    layout.height,
                );
                base_y = text_top + layout.baseline;
            }
            None => {}
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

        // Border style 3 is an opaque box behind EACH event line
        // (libass/VSFilter draw per-line boxes, not one block box):
        // every line grown by the effective outline on every side.
        // Margins position the text; they are not box padding. The
        // fill is the OUTLINE color (VSFilter copies colors[2] into
        // the box polygon; libass fills the outline bitmap): BackColour
        // only affects the shadow. Boxes stay axis-aligned under
        // rotation (references do not rotate them; text may spill).
        if resolved.border_style == 3 {
            let box_color = resolved.outline_color.to_ass_components();
            let clamp_i32 = |v: f64| {
                finite_to_i32(v.clamp(f64::from(i32::MIN), f64::from(i32::MAX))).unwrap_or(0)
            };
            let clamp_dim = |v: f64| clamp_i32(v.ceil().clamp(0.0, 65_536.0));
            let (box_rx, box_ry) = if resolved.scaled_border_and_shadow {
                (scale_x, scale_y)
            } else {
                (blur_scale_x, blur_scale_y)
            };
            let pad_x = clamp_i32(
                (resolved.outline_x * box_rx * resolved.scale_x / 100.0)
                    .round()
                    .clamp(0.0, 65_536.0),
            );
            let pad_y = clamp_i32(
                (resolved.outline_y * box_ry * resolved.scale_y / 100.0)
                    .round()
                    .clamp(0.0, 65_536.0),
            );
            let fill = [
                box_color[0],
                box_color[1],
                box_color[2],
                (f64::from(255 - box_color[3]) * alpha_mult).clamp(0.0, 255.0) as u8,
            ];
            // Box shadow (references shadow the padded box): same
            // offset/color model as glyph shadows, drawn first so all
            // boxes paint over all shadows.
            let shadow_c = resolved.shadow_color.to_ass_components();
            let shadow_fill = [
                shadow_c[0],
                shadow_c[1],
                shadow_c[2],
                (f64::from(255 - shadow_c[3]) * alpha_mult).clamp(0.0, 255.0) as u8,
            ];
            let shadow_ox = clamp_i32(
                (resolved.shadow_x * box_rx * resolved.scale_x / 100.0)
                    .round()
                    .clamp(f64::from(i32::MIN), f64::from(i32::MAX)),
            );
            let shadow_oy = clamp_i32(
                (resolved.shadow_y * box_ry * resolved.scale_y / 100.0)
                    .round()
                    .clamp(f64::from(i32::MIN), f64::from(i32::MAX)),
            );
            let shadow_active = shadow_ox != 0 || shadow_oy != 0;
            let line_boxes = event_line_boxes(&segments, &layout.items);
            // A trailing empty last line draws nothing (extent-based).
            let drawable = line_boxes
                .iter()
                .enumerate()
                .filter(|(i, line)| line.has_content || *i + 1 < line_boxes.len());
            let mut rects: Vec<(i32, i32, i32, i32)> = Vec::new();
            for (_, line) in drawable {
                let inset = line_align_inset(line.width, layout.width, resolved.alignment);
                let rect = (
                    clamp_i32(base_x + inset),
                    clamp_i32(base_y - layout.baseline + line.y),
                    clamp_dim(line.width),
                    clamp_dim(line.height),
                );
                rects.push(rect);
            }
            if shadow_active {
                for (x, y, w, h) in &rects {
                    effects::apply_opaque_box(
                        buffer,
                        x.saturating_add(shadow_ox),
                        y.saturating_add(shadow_oy),
                        *w,
                        *h,
                        pad_x,
                        pad_y,
                        shadow_fill,
                    );
                }
            }
            for (x, y, w, h) in &rects {
                effects::apply_opaque_box(buffer, *x, *y, *w, *h, pad_x, pad_y, fill);
            }
        }

        // Karaoke runs (empty when the event has no karaoke tags).
        // Spans come from the layout pass, so inline style changes and
        // drawings measure exactly what rendering consumes.
        let (karaoke_runs, glyph_run, drawing_run) = build_karaoke_runs(&segments, &layout.items);
        let elapsed_ms = time_ms.saturating_sub(start_ms);
        // Sweep buffer for the in-window sweep run (see `SweepState`).
        let mut sweep = SweepState::new();

        // Per-segment rendering
        let mut x_offset = 0.0_f64;
        let mut line_y_offset = 0.0_f64;
        // Cumulative `\fay` baseline shear (libass
        // `apply_baseline_shear`, default non-whole-text-layout mode):
        // reset at every line break AND every style-run start
        // (style-key change, drawing boundary, or karaoke-run start),
        // accumulated across same-run segments otherwise.
        let mut fay_line_shear = 0.0_f64;
        // Last segment that fed the shear accumulator (style-run
        // tracking for the reset above; skipped segments never update
        // it, matching libass where empty spans emit no glyphs).
        let mut prev_shear_seg: Option<usize> = None;
        // True line widths for per-line alignment, plus the current
        // line index (advanced on breaks and mid-segment row changes
        // exactly as `event_line_widths` counts them).
        let line_widths = event_line_widths(&segments, &layout.items);
        let mut cur_line: usize = 0;

        for (seg_idx, segment) in segments.iter().enumerate() {
            let item = &layout.items[seg_idx];
            if item.skipped {
                continue;
            }
            // Leading breaks open new lines (mirrors event_line_widths).
            let leading = segment.text.chars().take_while(|c| *c == '\n').count();
            if leading > 0 {
                cur_line = cur_line.saturating_add(leading);
                x_offset = 0.0;
                fay_line_shear = 0.0;
            }

            // Style/shape come from the layout pass; karaoke recolors
            // per glyph/run below (runs can change mid-segment at line
            // breaks, so there is no per-segment sweep state).
            let segment_resolved = item.resolved.clone();

            // Drawing segments render vector paths at the pen position.
            if let Some(drawing) = &item.drawing {
                // Drawings break karaoke runs; a pending sweep (only
                // possible under builder/render skew) flushes first.
                sweep.flush(buffer, &karaoke_runs, elapsed_ms, alpha);
                // Every drawing starts a libass style run: the shear
                // accumulator restarts here (and restarts again at the
                // next text segment via the drawing-boundary check).
                fay_line_shear = 0.0;
                prev_shear_seg = Some(seg_idx);
                let unit = drawing_unit_scale(scale_x, scale_y, drawing.mode);
                // libass drawing placement (`get_outline_glyph`): the
                // drawing origin sits at the pen with the box hanging
                // above the baseline by its HEIGHT (`offset.y = -asc`,
                // `asc = y_max - y_min`), and `offset.x = 0` — the
                // bbox minimum is preserved, not normalized away.
                // Advance is the bbox width (`v->advance = x_max -
                // x_min`), so a drawing starting at x=80 leaves its
                // left bearing empty (probe: 80u gap renders 53px)
                // and min_y pushes ink below the baseline (probe:
                // min_y=30 renders 20px lower; min_y=100 falls
                // fully below a bottom-aligned frame).
                let draw_inset = line_align_inset(
                    line_widths.get(cur_line).copied().unwrap_or(0.0),
                    layout.width,
                    resolved.alignment,
                );
                let draw_x = base_x + x_offset + draw_inset;
                // Known divergence: libass models `\pbo` as
                let draw_y = base_y + line_y_offset - drawing.baseline + fay_line_shear;
                // Karaoke for drawings (libass splits drawing runs
                // exactly like text runs: verified by probe).
                let run = drawing_run[seg_idx].and_then(|id| karaoke_runs.get(id));
                let sweep_split = run.and_then(|run| {
                    if run.kind == KaraokeKind::Sweep
                        && run.sweep
                        && elapsed_ms >= run.start_ms
                        && elapsed_ms < run.end_ms
                        && run.span > 0.0
                    {
                        // Ink-left + full advance width (probe:
                        // libass splits at `leftmost_x + frac *
                        // advance`: a 60u square at x=80 splits at
                        // its ink middle, not at pen + frac).
                        // `run.span` and `min_x` already carry the
                        // unit scale (layout multiplies once), so no
                        // further scaling applies here. Drawings
                        // render unrotated here, so `flip` (a rotation
                        // effect) never applies: mirroring the sweep of
                        // an unrotated drawing would be wrong.
                        let offset = run.sweep_frac(elapsed_ms) * run.span;
                        let left = draw_x + drawing.min_x;
                        if left.is_finite() && offset.is_finite() && unit.is_finite() {
                            return Some((left + offset).round() as i64);
                        }
                    }
                    None
                });
                match (run, sweep_split) {
                    (Some(_), Some(brk)) => {
                        // Two non-overlapping clipped passes with a hard
                        // edge (probe: adjacent primary/secondary
                        // columns, no blended column).
                        let (primary, secondary) =
                            (segment_resolved.color, segment_resolved.secondary_color);
                        let pa = (primary.opacity() as f64 * alpha_mult) as u8;
                        let sa = (secondary.opacity() as f64 * alpha_mult) as u8;
                        let pc = primary.to_ass_components();
                        let sc = secondary.to_ass_components();
                        let (left_c, left_a, right_c, right_a) = (pc, pa, sc, sa);
                        super::drawing::DrawingParser::render_drawing_clipped(
                            buffer,
                            &segment.text,
                            draw_x,
                            draw_y,
                            unit,
                            [left_c[0], left_c[1], left_c[2], left_a],
                            (None, Some(brk)),
                        );
                        super::drawing::DrawingParser::render_drawing_clipped(
                            buffer,
                            &segment.text,
                            draw_x,
                            draw_y,
                            unit,
                            [right_c[0], right_c[1], right_c[2], right_a],
                            (Some(brk), None),
                        );
                    }
                    _ => {
                        let mut color = segment_resolved.color;
                        // Pop runs (and out-of-window sweeps) light at
                        // the window end; for `\k`/`\ko` that is the
                        // run start. Drawings have no outline pass, so
                        // `\ko` needs no separate suppression here.
                        if let Some(run) = run {
                            if elapsed_ms < run.end_ms {
                                color = segment_resolved.secondary_color;
                            }
                        }
                        let components = color.to_ass_components();
                        super::drawing::DrawingParser::render_drawing(
                            buffer,
                            &segment.text,
                            draw_x,
                            draw_y,
                            unit,
                            [
                                components[0],
                                components[1],
                                components[2],
                                (color.opacity() as f64 * alpha_mult) as u8,
                            ],
                        );
                    }
                }
                // Drawings advance the baseline shear like libass glyphs.
                accumulate_fay_shear(
                    &mut fay_line_shear,
                    segment_resolved.shear_y,
                    segment_resolved.scale_x,
                    segment_resolved.scale_y,
                    drawing.width,
                );
                if segment.text.ends_with('\n') {
                    x_offset = 0.0;
                    line_y_offset += drawing.height;
                    fay_line_shear = 0.0;
                    cur_line = cur_line.saturating_add(1);
                } else {
                    x_offset += drawing.width;
                }
                continue;
            }

            // Text path: font, size, and shaping come from the layout pass.
            let segment_font_size =
                segment_resolved.font_size * (video_height as f64 / play_res_y.max(1) as f64);
            let shaped = &item.shaped;

            // Pre-compute effect parameters. With ScaledBorderAndShadow,
            // borders/shadows scale with the resolution ratio; otherwise
            // script units map 1:1 to video pixels.
            let (res_x, res_y) = if segment_resolved.scaled_border_and_shadow {
                (scale_x, scale_y)
            } else {
                (blur_scale_x, blur_scale_y)
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
            // Row marker restarts per segment: the first row continues
            // the current line (leading breaks were handled at segment
            // start); later rows open new lines.
            let mut shear_row_y: Option<f64> = None;
            // Pen within the current row (its furthest advance edge);
            // the segment leaves the pen at its last row's end.
            let mut row_pen = 0.0_f64;
            // The shaper emits exactly one glyph per non-break
            // character in order, so `glyph_idx` is the run-lookup key.
            for (glyph_idx, glyph) in shaped.glyphs.iter().enumerate() {
                // Style-run shear reset (libass `apply_baseline_shear`
                // restarts at every run start, even for skipped glyphs):
                // a new segment whose style key or drawing status differs
                // from the previous emitting segment, or a glyph that
                // opens its karaoke run.
                if prev_shear_seg != Some(seg_idx) {
                    let new_run = match prev_shear_seg.and_then(|p| layout.items.get(p)) {
                        None => false,
                        Some(prev) => {
                            prev.drawing.is_some()
                                || !prev.resolved.same_karaoke_run(&item.resolved)
                        }
                    };
                    if new_run {
                        fay_line_shear = 0.0;
                    }
                    prev_shear_seg = Some(seg_idx);
                }
                let run_start = glyph_run
                    .get(seg_idx)
                    .and_then(|v| v.get(glyph_idx))
                    .copied()
                    .flatten()
                    .and_then(|id| karaoke_runs.get(id))
                    .is_some_and(|run| run.first_glyph == Some((seg_idx, glyph_idx)));
                if run_start {
                    fay_line_shear = 0.0;
                }
                if glyph.scale_x <= 0.0 || glyph.scale_y <= 0.0 {
                    continue;
                }
                // A new shaper row inside this segment starts a new
                // line: reset the shear accumulator, advance the line
                // index (mirrors `event_line_widths`), and restart the
                // pen at the new line's edge.
                match shear_row_y {
                    None => shear_row_y = Some(glyph.y),
                    Some(y) if y == glyph.y => {}
                    Some(_) => {
                        shear_row_y = Some(glyph.y);
                        fay_line_shear = 0.0;
                        cur_line = cur_line.saturating_add(1);
                        x_offset = 0.0;
                        row_pen = 0.0;
                    }
                }
                let edge = glyph.x + glyph.advance;
                if edge.is_finite() && edge > row_pen {
                    row_pen = edge;
                }
                let line_inset = line_align_inset(
                    line_widths.get(cur_line).copied().unwrap_or(0.0),
                    layout.width,
                    resolved.alignment,
                );
                // Baseline shear (libass `apply_baseline_shear`): the
                // glyph rides at the sheared baseline, then contributes
                // its own advance to the following glyphs. Applied even
                // when this glyph later fails to rasterize, matching the
                // reference pass that runs before rasterization.
                let glyph_y = glyph.y + fay_line_shear;
                accumulate_fay_shear(
                    &mut fay_line_shear,
                    segment_resolved.shear_y,
                    glyph.scale_x,
                    glyph.scale_y,
                    glyph.advance,
                );

                // Per-glyph fallback face (primary when the recorded id is
                // absent); each face carries its own faux requirements.
                let face = item
                    .faces
                    .iter()
                    .find(|f| f.id == glyph.font_id)
                    .or(item.faces.first());
                let Some(face) = face else {
                    continue;
                };
                let face_font = font_manager.get_font(face.id).unwrap_or(font);
                let cached = self.glyph_cache.get_or_rasterize(
                    face.id,
                    face_font,
                    glyph.glyph_id,
                    segment_font_size,
                    face.faux_bold,
                    face.faux_italic,
                );

                // Decorations ride in the glyph bitmap (libass: deco
                // bars are outline geometry, so they shear, rotate, and
                // sweep with the glyph). Metrics come from this glyph's
                // own face; like libass, bars need advance > 0 and
                // gated font metrics, else the glyph is undecorated.
                let deco = if segment_resolved.underline || segment_resolved.strike_out {
                    font_manager.decoration_metrics(face.id).filter(|m| {
                        m.units_per_em != 0
                            && ((segment_resolved.underline && m.underline.is_some())
                                || (segment_resolved.strike_out && m.strikeout.is_some()))
                            && glyph.advance > 0.0
                            && glyph.advance.is_finite()
                            && segment_font_size.is_finite()
                            && segment_font_size > 0.0
                    })
                } else {
                    None
                };
                // Empty glyphs (spaces) still need bar-only bitmaps so
                // bars stay continuous; without bars they skip as before.
                let cached_empty = cached.width == 0 || cached.height == 0;
                if cached_empty && deco.is_none() {
                    continue;
                }

                // Effective work inputs for the shared transform path:
                // normal glyphs use the cached raster (resized, padded
                // when bars overhang the bitmap); bar-only glyphs
                // synthesize a transparent advance-wide box.
                let (
                    work_bitmap,
                    glyph_width,
                    glyph_height,
                    eff_bearing_x,
                    eff_bearing_y,
                    eff_scale_x,
                    eff_scale_y,
                ) = if cached_empty {
                    // Bar-only (spaces): transparent box, pen at its
                    // left edge, bars at metric rows. Authored in
                    // device pixels, so the effective scale is 1.
                    let Some(metrics) = deco else {
                        continue;
                    };
                    let rel = deco_bar_rows(
                        segment_resolved.underline,
                        segment_resolved.strike_out,
                        metrics,
                        0.0,
                        segment_font_size,
                    );
                    let top = rel.iter().map(|r| r.0).fold(f64::INFINITY, f64::min);
                    let bot = rel.iter().map(|r| r.1).fold(f64::NEG_INFINITY, f64::max);
                    if rel.is_empty() || !top.is_finite() || !bot.is_finite() || bot <= top {
                        continue;
                    }
                    let w = glyph.advance.ceil().clamp(1.0, 65_536.0) as u32;
                    let h = (bot - top).ceil().clamp(1.0, 1024.0) as u32;
                    if u64::from(w) * u64::from(h) > MAX_GLYPH_BITMAP_PIXELS {
                        continue;
                    }
                    let mut owned = vec![0u8; w as usize * h as usize];
                    let pen_local = -top;
                    for (t, b) in &rel {
                        paint_deco_bar(
                            &mut owned,
                            w,
                            h,
                            0.0,
                            glyph.advance,
                            t + pen_local,
                            b + pen_local,
                        );
                    }
                    (
                        Cow::Owned(owned),
                        w,
                        h,
                        0.0f32,
                        (-pen_local) as f32,
                        1.0,
                        1.0,
                    )
                } else {
                    let (sx, sy) = (glyph.scale_x, glyph.scale_y);
                    let scaled =
                        if (sx - 1.0).abs() < f64::EPSILON && (sy - 1.0).abs() < f64::EPSILON {
                            Cow::Borrowed(cached.bitmap.as_slice())
                        } else {
                            Cow::Owned(
                                RenderBuffer::resize_coverage_bitmap(
                                    &cached.bitmap,
                                    cached.width,
                                    cached.height,
                                    sx,
                                    sy,
                                )
                                .0,
                            )
                        };
                    let scaled_width = {
                        let v = (f64::from(cached.width) * sx).round();
                        if !v.is_finite() {
                            continue;
                        }
                        (v.clamp(1.0, f64::from(u32::MAX)) as u32).max(1)
                    };
                    let scaled_height = {
                        let v = (f64::from(cached.height) * sy).round();
                        if !v.is_finite() {
                            continue;
                        }
                        (v.clamp(1.0, f64::from(u32::MAX)) as u32).max(1)
                    };
                    match deco {
                        None => (
                            scaled,
                            scaled_width,
                            scaled_height,
                            cached.bearing_x,
                            cached.bearing_y,
                            sx,
                            sy,
                        ),
                        Some(metrics) => {
                            // Bars span the full advance from the pen (libass
                            // `add_rect(0, .., adv, ..)`); pad the bitmap so
                            // bearing pixels are covered, else bars would dot
                            // every glyph. Padding stays within the glyph pixel
                            // budget, else bars paint clamped into the bitmap.
                            let mut owned = scaled.into_owned();
                            let pen_x = f64::from(-cached.bearing_x) * sx;
                            let pen_y = f64::from(-cached.bearing_y) * sy;
                            let mut pad_l: u32 = 0;
                            let mut pad_r: u32 = 0;
                            if pen_x.is_finite()
                                && u64::from(scaled_width) * u64::from(scaled_height)
                                    == owned.len() as u64
                            {
                                let need_l = (-pen_x).max(0.0);
                                let need_r =
                                    (pen_x + glyph.advance - f64::from(scaled_width)).max(0.0);
                                let cand_l = need_l.ceil().clamp(0.0, f64::from(u32::MAX)) as u32;
                                let mut cand_r =
                                    need_r.ceil().clamp(0.0, f64::from(u32::MAX)) as u32;
                                // Parity: the projective center math moves the
                                // placement by half the total pad but floors the
                                // output offset, so an odd total pad shifts the
                                // glyph half a pixel (AA fringes differ from the
                                // undecorated render). Keep the total even; the
                                // extra transparent column never affects bars.
                                if (u64::from(cand_l) + u64::from(cand_r)) % 2 == 1 {
                                    cand_r = cand_r.saturating_add(1);
                                }
                                let w1 =
                                    u64::from(scaled_width) + u64::from(cand_l) + u64::from(cand_r);
                                if w1 <= u64::from(u32::MAX)
                                    && w1 * u64::from(scaled_height) <= MAX_GLYPH_BITMAP_PIXELS
                                {
                                    pad_l = cand_l;
                                    pad_r = cand_r;
                                }
                            }
                            let (gw, bear_x) = if pad_l > 0 || pad_r > 0 {
                                let w1 =
                                    (u64::from(scaled_width) + u64::from(pad_l) + u64::from(pad_r))
                                        as u32;
                                let mut padded = vec![0u8; w1 as usize * scaled_height as usize];
                                for (row, src_row) in
                                    owned.chunks_exact(scaled_width as usize).enumerate()
                                {
                                    let dst = row * w1 as usize + pad_l as usize;
                                    padded[dst..dst + scaled_width as usize]
                                        .copy_from_slice(src_row);
                                }
                                owned = padded;
                                (w1, cached.bearing_x - pad_l as f32 / sx as f32)
                            } else {
                                (scaled_width, cached.bearing_x)
                            };
                            let pen_x_eff = f64::from(-bear_x) * sx;
                            for (t, b) in deco_bar_rows(
                                segment_resolved.underline,
                                segment_resolved.strike_out,
                                metrics,
                                pen_y,
                                segment_font_size * sy,
                            ) {
                                paint_deco_bar(
                                    &mut owned,
                                    gw,
                                    scaled_height,
                                    pen_x_eff,
                                    pen_x_eff + glyph.advance,
                                    t,
                                    b,
                                );
                            }
                            (
                                Cow::Owned(owned),
                                gw,
                                scaled_height,
                                bear_x,
                                cached.bearing_y,
                                sx,
                                sy,
                            )
                        }
                    }
                };
                // Transform order (ASS reference, libass
                // `calc_transform_matrix` + VSFilter `Transform_C`):
                // glyph-local scaling → `\fax`/`\fay` shear around the
                // glyph-space pivot → rotation/perspective →
                // compositing. The shear is folded into the projective
                // pass below (single resample), never applied
                // post-rotation.
                let bearing_x = f64::from(eff_bearing_x) * eff_scale_x;
                let bearing_y = f64::from(eff_bearing_y) * eff_scale_y;

                // Calculate original center of the glyph relative to origin
                // (`glyph_y` carries the cumulative `\fay` baseline shear;
                // `line_inset` the line's alignment inside the block).
                let orig_cx =
                    base_x + x_offset + line_inset + glyph.x + bearing_x + glyph_width as f64 / 2.0;
                let orig_cy =
                    base_y + line_y_offset + glyph_y + bearing_y + glyph_height as f64 / 2.0;

                // Shear pivot in scaled-bitmap pixels (libass parity,
                // confirmed by pixel probes): `\fay` pivots at the pen
                // x, `\fax` at the ascender line (one font ascent
                // above the pen). Pen sits at (-bearing) in bitmap
                // pixels. Non-uniform `\fsc` adjusts the factors
                // exactly as the references' pre-scale shear does.
                let ascent = face_font
                    .as_scaled(PxScale::from(segment_font_size as f32))
                    .ascent() as f64;
                let pivot = (
                    f64::from(-eff_bearing_x) * eff_scale_x,
                    (f64::from(-eff_bearing_y) - ascent) * eff_scale_y,
                );

                let dx = orig_cx - org_x;
                let dy = orig_cy - org_y;

                // Rotation matrix (libass `calc_transform_matrix` order:
                // shear happens in the caller, then frz, then frx, then
                // fry, i.e. M = Ry * Rx * Rz). All three ASS angles are
                // negated versus standard math because screen Y grows
                // downward: positive \frz runs counterclockwise on
                // screen, matching the reference frames.
                let rz = segment_resolved.angle;
                let rx = segment_resolved.rotation_x;
                let ry = segment_resolved.rotation_y;

                let mat_z = Matrix3x3::rotation_z((-rz).to_radians());
                let mat_y = Matrix3x3::rotation_y((-ry).to_radians());
                let mat_x = Matrix3x3::rotation_x((-rx).to_radians());

                let matrix = mat_y.multiply(&mat_x).multiply(&mat_z);

                // Perspective distance (libass `calc_transform_matrix`:
                // `dist = 20000 * blur_scale_y` in 1/64px units, i.e.
                // 312.5px times the vertical frame-to-layout scale).
                let perspective = 312.5 * (video_height as f64 / play_res_y as f64);

                // Effective pre-rotation shear. References shear the
                // unscaled glyph, so non-uniform scale adjusts the
                // factors (libass `fax*sx/sy`, `fay*sy/sx`). Non-finite
                // shear skips the glyph.
                let Some((fax, fay)) =
                    effective_shear((segment_resolved.shear_x, segment_resolved.shear_y))
                else {
                    continue;
                };
                let (sx, sy) = (eff_scale_x, eff_scale_y);
                let (fax, fay) =
                    if sx.is_finite() && sy.is_finite() && sx.abs() > 1e-9 && sy.abs() > 1e-9 {
                        (fax * sx / sy, fay * sy / sx)
                    } else {
                        (fax, fay)
                    };

                // Use projective transform for exact perspective warping
                let (rot_bitmap, rot_w, rot_h, rot_ox, rot_oy) =
                    RenderBuffer::projective_transform_coverage_bitmap(
                        &work_bitmap,
                        glyph_width,
                        glyph_height,
                        &matrix,
                        perspective,
                        (fax, fay),
                        pivot,
                    );
                // Calculate 3D position and perspective scale for the glyph center.
                // Guard every value before division: degenerate input skips
                // this glyph instead of propagating NaN/Inf into geometry.
                if rot_bitmap.is_empty() {
                    continue;
                }
                let (x3, y3, z3) = matrix.transform(dx, dy, 0.0);
                if !perspective.is_finite() || !x3.is_finite() || !y3.is_finite() || !z3.is_finite()
                {
                    continue;
                }
                let denom = perspective + z3;
                if !denom.is_finite() || denom.abs() < 1e-6 {
                    continue;
                }
                let scale_factor = perspective / denom;
                if !scale_factor.is_finite() {
                    continue;
                }
                let px = x3 * scale_factor;
                let py = y3 * scale_factor;
                if !px.is_finite() || !py.is_finite() {
                    continue;
                }
                // No post shear: the shear already warped the bitmap
                // inside the projective pass (pre-rotation, glyph-local).
                // The pass offset lands the sheared bitmap exactly.

                // Final screen position (validated; skip glyph if unrepresentable).
                let (Some(final_gx), Some(final_gy)) = (
                    finite_to_i32(
                        (org_x + px + f64::from(rot_ox))
                            .clamp(f64::from(i32::MIN), f64::from(i32::MAX)),
                    ),
                    finite_to_i32(
                        (org_y + py + f64::from(rot_oy))
                            .clamp(f64::from(i32::MIN), f64::from(i32::MAX)),
                    ),
                ) else {
                    continue;
                };
                // Adjust effect scales by perspective factor
                let current_outline_x = outline_scale_x * scale_factor;
                let current_outline_y = outline_scale_y * scale_factor;
                let current_shadow_x = shadow_offset_x * scale_factor;
                let current_shadow_y = shadow_offset_y * scale_factor;

                // Karaoke classification. In-window sweep members
                // buffer for the run's device-space split; everything
                // else paints immediately (pop rule below).
                let glyph_run_id: Option<usize> = glyph_run
                    .get(seg_idx)
                    .and_then(|v| v.get(glyph_idx))
                    .copied()
                    .flatten();
                let run = glyph_run_id.and_then(|id| karaoke_runs.get(id));
                // A new run (or gap) flushes a pending sweep: runs are
                // contiguous, so this fires exactly at run boundaries
                // (plus defensively under builder/render skew).
                if sweep.run != glyph_run_id {
                    sweep.flush(buffer, &karaoke_runs, elapsed_ms, alpha);
                }
                sweep.note_run(glyph_run_id);

                let primary_c = segment_resolved.color.to_ass_components();
                let secondary_c = segment_resolved.secondary_color.to_ass_components();
                let primary_a = segment_resolved.color.opacity();
                let secondary_a = segment_resolved.secondary_color.opacity();
                let outline_rgba = [
                    outline_color_rgba[0],
                    outline_color_rgba[1],
                    outline_color_rgba[2],
                    (outline_alpha as f64 * alpha_mult) as u8,
                ];
                let shadow_rgba = [
                    shadow_color_rgba[0],
                    shadow_color_rgba[1],
                    shadow_color_rgba[2],
                    (shadow_alpha as f64 * alpha_mult) as u8,
                ];

                let sweeping = run.is_some_and(|run| {
                    run.kind == KaraokeKind::Sweep
                        && run.sweep
                        && elapsed_ms >= run.start_ms
                        && elapsed_ms < run.end_ms
                });
                if let (true, Some(id), Some(run)) = (sweeping, glyph_run_id, run) {
                    // Degraded members (past the buffer cap) paint
                    // whole-glyph against the flushed edge.
                    let mut degraded_edge = sweep.degraded.map(|(_, e, f)| (e, f));
                    if degraded_edge.is_none()
                        && sweep.bytes.saturating_add(rot_bitmap.len()) > MAX_SWEEP_BUFFER_BYTES
                        && !sweep.buf.is_empty()
                    {
                        // Marked before flushing so the flush records
                        // the edge for the rest of the run.
                        sweep.degraded = Some((id, 0, run.flip));
                        sweep.flush(buffer, &karaoke_runs, elapsed_ms, alpha);
                        degraded_edge = sweep.degraded.map(|(_, e, f)| (e, f));
                    }
                    if let Some((edge, flip)) = degraded_edge {
                        let center = i64::from(final_gx) + i64::from(rot_w) / 2;
                        let (fill_c, fill_a) = if (center < edge) != flip {
                            (primary_c, primary_a)
                        } else {
                            (secondary_c, secondary_a)
                        };
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
                                outline_rgba,
                            );
                        }
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
                                shadow_rgba,
                            );
                        }
                        paint_glyph_fill(
                            buffer,
                            &rot_bitmap,
                            GlyphGeom {
                                w: rot_w,
                                h: rot_h,
                                gx: final_gx,
                                gy: final_gy,
                            },
                            alpha,
                            |_| (fill_c, fill_a),
                        );
                        continue;
                    }
                    sweep.bytes = sweep.bytes.saturating_add(rot_bitmap.len());
                    sweep.buf.push(BufferedSweepGlyph {
                        bitmap: rot_bitmap,
                        w: rot_w,
                        h: rot_h,
                        gx: final_gx,
                        gy: final_gy,
                        primary: primary_c,
                        primary_alpha: primary_a,
                        secondary: secondary_c,
                        secondary_alpha: secondary_a,
                        outline: outline_active.then_some((
                            outline_rgba,
                            current_outline_x,
                            current_outline_y,
                        )),
                        shadow: shadow_active.then_some((
                            shadow_rgba,
                            current_shadow_x,
                            current_shadow_y,
                        )),
                    });
                    sweep.run = Some(id);
                    if run.last_glyph == Some((seg_idx, glyph_idx)) {
                        sweep.flush(buffer, &karaoke_runs, elapsed_ms, alpha);
                    }
                    continue;
                }

                // Immediate paint. Pop rule (probe-verified): primary
                // from the window end, secondary before it. For
                // `\k`/`\ko` the window end is the run start; finished
                // sweeps are primary, unstarted ones secondary.
                let mut fill = (primary_c, primary_a);
                let mut outline_on = outline_active;
                if let Some(run) = run {
                    if elapsed_ms < run.end_ms {
                        fill = (secondary_c, secondary_a);
                    }
                    if run.kind == KaraokeKind::Outline
                        && karaoke_outline_suppressed(elapsed_ms, run.start_ms)
                    {
                        outline_on = false;
                    }
                }
                if outline_on {
                    effects::apply_outline_xy(
                        buffer,
                        &rot_bitmap,
                        rot_w,
                        rot_h,
                        final_gx,
                        final_gy,
                        current_outline_x,
                        current_outline_y,
                        outline_rgba,
                    );
                }
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
                        shadow_rgba,
                    );
                }
                paint_glyph_fill(
                    buffer,
                    &rot_bitmap,
                    GlyphGeom {
                        w: rot_w,
                        h: rot_h,
                        gx: final_gx,
                        gy: final_gy,
                    },
                    alpha,
                    |_| fill,
                );
            }

            // Decorations need no segment pass: each glyph's bitmap
            // already carries its bar segment (painted pre-transform
            // in the loop above), so bars shear, rotate, and sweep
            // with the text, span spaces via bar-only glyphs, and take
            // outline, shadow, and karaoke colors like glyph ink.

            // Update offsets for next segment
            // Check if segment ends with line break
            if segment.text.ends_with('\n') {
                x_offset = 0.0;
                line_y_offset += shaped.height;
                fay_line_shear = 0.0;
                cur_line = cur_line.saturating_add(1);
            } else {
                // The pen continues at the segment's LAST row end, not
                // its widest row (equal for single-row segments).
                x_offset += row_pen;
            }
        }
        // Event end flushes any pending sweep (normally already
        // flushed at its last glyph; this covers skew cases). Runs
        // routinely span segments (`{\k}a{\pos}b`), so nothing
        // flushes per segment. Decoration bars ride in the glyph
        // bitmaps, hence sweep (and pop) with their glyphs: probe
        // `{\kf100\u1}He` shows the bar split white/red at the
        // midpoint, exactly like the glyph ink.
        sweep.flush(buffer, &karaoke_runs, elapsed_ms, alpha);

        // Blur before clipping: blurring after a clip would bleed
        // pixels outside the clip region.
        if resolved.blur > 0.0 {
            effects::apply_blur_xy(
                buffer,
                resolved.blur * blur_scale_x,
                resolved.blur * blur_scale_y,
            );
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

        // Legacy scroll-effect clip bounds and edge fades (VSFilter
        // EF_BANNER/EF_SCROLL clipper). Sequential clips intersect, so
        // these compose with user \clips above.
        match legacy_effect {
            Some(LegacyEffect::Banner { fadeaway, .. }) => {
                let w = buffer.width as i64;
                let h = buffer.height as i64;
                if w > 0 && h > 0 {
                    let x1 = w.saturating_sub(1).min(i32::MAX as i64) as i32;
                    let y1 = h.saturating_sub(1).min(i32::MAX as i64) as i32;
                    effects::apply_clip(buffer, (0, 0, x1, y1));
                }
                if fadeaway > 0.0 {
                    effects::apply_fadeaway_x(buffer, fadeaway * scale_x);
                }
            }
            Some(
                LegacyEffect::ScrollUp {
                    top,
                    bottom,
                    fadeaway,
                    ..
                }
                | LegacyEffect::ScrollDown {
                    top,
                    bottom,
                    fadeaway,
                    ..
                },
            ) => {
                // Band bottom is exclusive in VSFilter; our clips are
                // inclusive, hence `bottom - 1`. Saturating: finite but
                // absurd band edges (1e19) saturate the `as i64` casts,
                // and plain `-/+ 1` would then overflow in debug builds.
                let y0 = (top * scale_y).floor() as i64;
                let y1 = ((bottom * scale_y).ceil() as i64).saturating_sub(1);
                let w = buffer.width as i64;
                if w > 0 {
                    let x1 = w.saturating_sub(1).min(i32::MAX as i64) as i32;
                    let cy0 = y0.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
                    let cy1 = y1.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
                    effects::apply_clip(buffer, (0, cy0, x1, cy1));
                }
                if fadeaway > 0.0 {
                    effects::apply_fadeaway_y(
                        buffer,
                        y0,
                        y0.max(y1.saturating_add(1)),
                        fadeaway * scale_y,
                    );
                }
            }
            None => {}
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
                            // `\r` exits drawing mode (\p is not line-global).
                            'r' if in_tag => {
                                chars.next();
                                drawing_mode = false;
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

    /// Render `text` at `time_ms` on a 640x480 buffer; returns the buffer.
    fn render_text(text: &str, time_ms: u64) -> RenderBuffer {
        render_text_with_style(text, &Style::new("Default"), time_ms)
    }

    /// Render `text` with an explicit style (640x480 buffer).
    fn render_text_with_style(text: &str, style: &Style, time_ms: u64) -> RenderBuffer {
        let mut comp = Compositor::new();
        let mut fm = FontManager::new();
        fm.load_font("DejaVu Sans", font::get_fallback_font(), false, false)
            .unwrap();
        let line = format!("Dialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,{text}");
        let event = Event::parse_from_line(&line).unwrap();
        let resolved = Compositor::resolve_style(style, &event);
        let mut buf = RenderBuffer::new(640, 480).unwrap();
        comp.composite_event(
            &mut buf,
            &event,
            &resolved,
            &fm,
            time_ms,
            640,
            480,
            640,
            480,
            0,
            &[],
        );
        buf
    }

    /// Render with explicit PlayRes/video sizes and border-scaling flag.
    #[allow(clippy::too_many_arguments)]
    fn render_sized(
        text: &str,
        style: &Style,
        time_ms: u64,
        play_w: u32,
        play_h: u32,
        vid_w: u32,
        vid_h: u32,
        scaled: bool,
    ) -> RenderBuffer {
        let mut comp = Compositor::new();
        let mut fm = FontManager::new();
        fm.load_font("DejaVu Sans", font::get_fallback_font(), false, false)
            .unwrap();
        let line = format!("Dialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,{text}");
        let event = Event::parse_from_line(&line).unwrap();
        let mut resolved = Compositor::resolve_style(style, &event);
        resolved.scaled_border_and_shadow = scaled;
        let mut buf = RenderBuffer::new(vid_w, vid_h).unwrap();
        comp.composite_event(
            &mut buf,
            &event,
            &resolved,
            &fm,
            time_ms,
            play_w,
            play_h,
            vid_w,
            vid_h,
            0,
            &[],
        );
        buf
    }

    /// Bounding box (w, h) of non-transparent pixels, if any.
    fn ink_bbox(buf: &RenderBuffer) -> Option<(u32, u32)> {
        let (mut x0, mut y0) = (u32::MAX, u32::MAX);
        let (mut x1, mut y1) = (0u32, 0u32);
        for y in 0..buf.height {
            for x in 0..buf.width {
                if buf.get_pixel(x, y)[3] > 0 {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
            }
        }
        if x0 > x1 {
            None
        } else {
            Some((x1 - x0 + 1, y1 - y0 + 1))
        }
    }

    /// Ink bands: one (x0, y0, x1, y1) box per maximal run of rows
    /// containing ink. Separates rendered text lines for per-line
    /// alignment assertions.
    fn ink_bands(buf: &RenderBuffer) -> Vec<(u32, u32, u32, u32)> {
        let mut rows = vec![false; buf.height as usize];
        for y in 0..buf.height {
            for x in 0..buf.width {
                if buf.get_pixel(x, y)[3] > 0 {
                    rows[y as usize] = true;
                    break;
                }
            }
        }
        let mut bands = Vec::new();
        let mut y = 0u32;
        while y < buf.height {
            if !rows[y as usize] {
                y += 1;
                continue;
            }
            let top = y;
            while y < buf.height && rows[y as usize] {
                y += 1;
            }
            let (mut x0, mut x1) = (buf.width, 0u32);
            for yy in top..y {
                for x in 0..buf.width {
                    if buf.get_pixel(x, yy)[3] > 0 {
                        x0 = x0.min(x);
                        x1 = x1.max(x);
                    }
                }
            }
            bands.push((x0, top, x1, y - 1));
        }
        bands
    }

    #[test]
    fn test_wrap_style_2_disables_wrapping() {
        let font = fallback_font();
        let text = "aa aa aa aa";
        assert_eq!(wrap_event_text(text, 2, 1.0, &[&font], 48.0, 0.0), text);
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
    fn test_shared_slots_first_tag_wins() {
        // \pos/\move share one slot (libass EVENT_POSITIONED),
        // \fad/\fade share one slot (libass PARSED_FADE), and \an/\a
        // share one slot (libass PARSED_A): in every pair the FIRST
        // tag wins. Rect \clip/\iclip instead replace coordinates and
        // flip the rect mode (libass keeps rect state separate).
        let style = Style::new("Default");
        let resolve = |text: &str| {
            let line = format!("Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{text}");
            let event = Event::parse_from_line(&line).unwrap();
            Compositor::resolve_style(&style, &event)
        };
        // First positioning tag wins in either order; the loser is
        // ignored, so position and move_data never coexist.
        let r = resolve(r"{\pos(1,2)\move(3,4,5,6)}Hi");
        assert_eq!(r.position, Some((1.0, 2.0)));
        assert!(r.move_data.is_none());
        let r = resolve(r"{\move(3,4,5,6)\pos(1,2)}Hi");
        assert!(r.position.is_none());
        assert!(r.move_data.is_some());
        // First fade tag wins in either order.
        let r = resolve(r"{\fad(1,2)\fade(1,2,3,4,5,6,7)}Hi");
        assert!(r.complex_fade.is_none());
        assert_eq!((r.fade_in, r.fade_out), (1, 2));
        let r = resolve(r"{\fade(1,2,3,4,5,6,7)\fad(1,2)}Hi");
        assert!(r.complex_fade.is_some());
        assert_eq!((r.fade_in, r.fade_out), (0, 0));
        // Rect clip then iclip: later coordinates win, mode flips.
        let r = resolve(r"{\clip(1,2,3,4)\iclip(5,6,7,8)}Hi");
        assert!(r.clip.is_none());
        assert_eq!(r.inverse_clip, Some((5, 6, 7, 8)));
        let r = resolve(r"{\iclip(5,6,7,8)\clip(1,2,3,4)}Hi");
        assert_eq!(r.clip, Some((1, 2, 3, 4)));
        assert!(r.inverse_clip.is_none());
        // First alignment tag wins across \a / \an forms.
        let r = resolve(r"{\a1\an7}Hi");
        assert_eq!(r.alignment, 1);
        let r = resolve(r"{\an7\a1}Hi");
        assert_eq!(r.alignment, 7);
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

    #[test]
    fn test_reset_preserves_only_line_global() {
        // \r keeps exactly \pos/\move/\org/\clip/\iclip/\fad/\fade
        // plus alignment, drawing mode, and \pbo (libass
        // `ass_reset_render_context` touches none of those) and resets
        // everything else (fonts, colors, border/shadow, rotation,
        // blur, spacing) to the target style.
        use crate::types::override_tag::TextSegment;
        let base = Style::new("Default");
        let event =
            Event::parse_from_line("Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,x").unwrap();
        let resolved = Compositor::resolve_style(&base, &event);
        let segment = TextSegment {
            text: "B".to_string(),
            tags: vec![
                OverrideTag::Position(5.0, 6.0),
                OverrideTag::Move(1.0, 2.0, 3.0, 4.0),
                OverrideTag::Origin(7.0, 8.0),
                OverrideTag::Clip(0, 0, 10, 10),
                OverrideTag::InverseClip(0, 0, 20, 20),
                OverrideTag::Fade(100, 200),
                OverrideTag::Bold(700),
                OverrideTag::FontName("Other".to_string()),
                OverrideTag::FontSize(99.0),
                OverrideTag::LetterSpacing(9.0),
                OverrideTag::PrimaryColor(Color::new(255, 0, 0, 0)),
                OverrideTag::Border(9.0),
                OverrideTag::Shadow(8.0),
                OverrideTag::Blur(3.0),
                OverrideTag::RotationZ(45.0),
                OverrideTag::ScaleX(150.0),
                OverrideTag::Alignment(7),
                OverrideTag::Drawing(1),
                OverrideTag::DrawingBaseline(5.0),
                OverrideTag::Reset(None),
            ],
        };
        let seg = Compositor::resolve_segment_style(&resolved, &segment, &event, &[], 0, 0, 2000);
        // Line-global survivors: first \pos beats the later \move,
        // later rect coordinates replace earlier ones (iclip mode).
        assert_eq!(seg.position, Some((5.0, 6.0)));
        assert!(seg.move_data.is_none());
        assert_eq!(seg.origin, Some((7.0, 8.0)));
        assert_eq!(seg.clip, None);
        assert_eq!(seg.inverse_clip, Some((0, 0, 20, 20)));
        assert_eq!((seg.fade_in, seg.fade_out), (100, 200));
        assert_eq!(seg.alignment, 7);
        // Ordinary state resets to the style.
        assert_eq!(seg.font_weight, if base.bold { 700 } else { 400 });
        assert_eq!(seg.font_name, base.font_name);
        assert_eq!(seg.font_size, base.font_size);
        assert_eq!(seg.spacing, base.spacing);
        assert_eq!(seg.color, base.primary_color);
        assert_eq!(seg.outline_x, base.outline);
        assert_eq!(seg.shadow_x, base.shadow);
        assert_eq!(seg.blur, 0.0);
        assert_eq!(seg.angle, 0.0);
        assert_eq!(seg.scale_x, base.scale_x);
        // libass `ass_reset_render_context` never touches
        // `drawing_scale` or `pbo`, so both survive the reset.
        assert_eq!(seg.drawing_mode, 1);
        assert_eq!(seg.drawing_baseline_offset, 5.0);
    }

    #[test]
    fn test_reset_keeps_drawing_mode() {
        // libass `ass_reset_render_context` never touches
        // `drawing_scale`: {\p1}...{\r}text stays in drawing mode.
        let segments = parse_text_segments("{\\p1}m 0 0 l 100 0 100 100{\\r}Normal text");
        assert_eq!(segments.len(), 2);
        assert_eq!(segment_drawing_mode(&segments[0].tags, 0), 1);
        assert_eq!(segment_drawing_mode(&segments[1].tags, 0), 1);
        // A later \p0 exits; a later \pN re-enters with a new scale.
        let segments = parse_text_segments("{\\p1}a{\\r}b{\\p0}c{\\p2}d");
        assert_eq!(segment_drawing_mode(&segments[3].tags, 0), 2);
        // Break escapes stay swallowed across \r, work again after \p0.
        let segments = parse_text_segments("{\\p1}a{\\r}x\\Ny");
        let joined: String = segments.iter().map(|s| s.text.as_str()).collect();
        assert!(
            !joined.contains('\n'),
            "post-\\r \\N stays drawing: {joined:?}"
        );
        let segments = parse_text_segments("{\\p1}a{\\p0}x\\Ny");
        let joined: String = segments.iter().map(|s| s.text.as_str()).collect();
        assert!(
            joined.contains('\n'),
            "post-\\p0 \\N must break: {joined:?}"
        );
        // Resolved style agrees.
        let base = Style::new("Default");
        let event = Event::parse_from_line(
            "Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{\\p1}m 0 0{\\r}Hi",
        )
        .unwrap();
        let resolved = Compositor::resolve_style(&base, &event);
        let segments = parse_text_segments(&event.text);
        let seg =
            Compositor::resolve_segment_style(&resolved, &segments[1], &event, &[], 0, 0, 2000);
        assert_eq!(seg.drawing_mode, 1);
    }

    #[test]
    fn test_wrapper_stays_drawing_across_reset() {
        // Wrap tokenizer: {\r} inside a drawing does not resume
        // wrapping — the text after it stays a verbatim drawing run.
        let font = fallback_font();
        let word_w = TextShaper::measure_text("aa", &font, 48.0, 0.0);
        let out = wrap_event_text(
            "{\\p1}m 0 0 l 10 0{\\r}aa aa aa aa",
            0,
            word_w + 1.0,
            &[&font],
            48.0,
            0.0,
        );
        assert!(
            !out.contains('\n'),
            "post-\\r drawing must not wrap: {out:?}"
        );
        // Only \p0 resumes wrapping.
        let out = wrap_event_text(
            "{\\p1}m 0 0 l 10 0{\\p0}aa aa aa aa",
            0,
            word_w + 1.0,
            &[&font],
            48.0,
            0.0,
        );
        assert!(out.contains('\n'), "post-\\p0 text must wrap: {out:?}");
        // Ordered groups: {\r\p1} and {\p1\r} both stay drawing.
        for group in ["{\\r\\p1}", "{\\p1\\r}"] {
            let still = wrap_event_text(
                &format!("{group}m 0 0 l 10 0 aa aa aa aa"),
                0,
                word_w + 1.0,
                &[&font],
                48.0,
                0.0,
            );
            assert!(
                !still.contains('\n'),
                "{group} drawing run must not wrap: {still:?}"
            );
        }
    }

    #[test]
    fn test_late_an_aligns_whole_line() {
        // \an anywhere positions the entire line (first wins).
        let base = Style::new("Default");
        for text in ["{\\an7}Hi", "Hi{\\an7}"] {
            let event = Event::parse_from_line(&format!(
                "Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{text}"
            ))
            .unwrap();
            assert_eq!(
                Compositor::resolve_style(&base, &event).alignment,
                7,
                "{text:?} must align the whole line"
            );
        }
        let event = Event::parse_from_line(
            "Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{\\an7}A{\\an1}B",
        )
        .unwrap();
        assert_eq!(Compositor::resolve_style(&base, &event).alignment, 7);
        // Render-level: late \an7 moves all ink to the top half.
        let top = render_text("AAAA{\\an7}", 1000);
        let bottom = render_text("AAAA", 1000);
        let (_, th) = ink_bbox(&top).expect("top text renders");
        let (_, bh) = ink_bbox(&bottom).expect("bottom text renders");
        assert!(th > 0 && bh > 0);
        let top_y = (0..top.height)
            .find(|&y| (0..top.width).any(|x| top.get_pixel(x, y)[3] > 0))
            .unwrap();
        let bottom_y = (0..bottom.height)
            .find(|&y| (0..bottom.width).any(|x| bottom.get_pixel(x, y)[3] > 0))
            .unwrap();
        assert!(top_y < 240, "late \\an7 must move ink up, got {top_y}");
        assert!(
            bottom_y > 240,
            "default alignment stays down, got {bottom_y}"
        );
    }

    #[test]
    fn test_late_q_sets_event_wrap() {
        // A late \q2 turns the soft \n into a hard break event-wide.
        let one_line = render_text("A\\nB", 1000);
        let two_lines = render_text("A\\nB{\\q2}", 1000);
        let (_, h1) = ink_bbox(&one_line).expect("renders");
        let (_, h2) = ink_bbox(&two_lines).expect("renders");
        assert!(h2 > h1 * 3 / 2, "late \\q2 must hard-break: {h1} vs {h2}");
    }

    #[test]
    fn test_multiple_global_tags_first_wins() {
        // Repeated \pos: first one wins (libass EVENT_POSITIONED).
        let base = Style::new("Default");
        let event = Event::parse_from_line(
            "Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{\\pos(10,10)}A{\\pos(100,100)}B",
        )
        .unwrap();
        assert_eq!(
            Compositor::resolve_style(&base, &event).position,
            Some((10.0, 10.0))
        );
        // Rect clips: later coordinates replace earlier ones and the
        // \clip vs \iclip form flips the rect mode (libass).
        for (text, want_clip, want_iclip) in [
            (
                "{\\clip(0,0,10,10)}A{\\iclip(0,0,20,20)}B",
                None,
                Some((0, 0, 20, 20)),
            ),
            (
                "{\\iclip(0,0,20,20)}A{\\clip(0,0,10,10)}B",
                Some((0, 0, 10, 10)),
                None,
            ),
        ] {
            let event = Event::parse_from_line(&format!(
                "Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{text}"
            ))
            .unwrap();
            let r = Compositor::resolve_style(&base, &event);
            assert_eq!(
                (r.clip, r.inverse_clip),
                (want_clip, want_iclip),
                "{text:?}"
            );
        }
        // Rect and vector clips coexist (libass keeps separate state).
        let event = Event::parse_from_line(
            "Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{\\clip(0,0,10,10)}A{\\clip(m 0 0 l 9 0 l 9 9)}B",
        )
        .unwrap();
        let r = Compositor::resolve_style(&base, &event);
        assert_eq!(r.clip, Some((0, 0, 10, 10)));
        assert!(r.clip_vector.is_some());
        // \pos + \move: the first tag wins, so a leading \pos renders
        // identical to a lone \pos (static across frames).
        let event = Event::parse_from_line(
            "Dialogue: 0,0:00:00.00,0:00:04.00,Default,,0,0,0,,{\\pos(10,10)\\move(0,0,100,0)}Hi",
        )
        .unwrap();
        let r = Compositor::resolve_style(&base, &event);
        assert_eq!(r.position, Some((10.0, 10.0)));
        assert!(r.move_data.is_none());
        let first_wins = render_text("{\\pos(10,10)\\move(0,0,100,0)}Hi", 2000);
        let static_pos = render_text("{\\pos(10,10)}Hi", 2000);
        assert_eq!(first_wins.as_bytes(), static_pos.as_bytes());
    }

    fn render_event_text(text: &str, scaled: bool) -> RenderBuffer {
        render_event_effect(text, "", scaled, 500)
    }

    fn render_event_effect(text: &str, effect: &str, scaled: bool, time_ms: u64) -> RenderBuffer {
        let mut comp = Compositor::new();
        let mut fm = FontManager::new();
        fm.load_font("DejaVu Sans", font::get_fallback_font(), false, false)
            .unwrap();
        let event = Event::parse_from_line(&format!(
            "Dialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,{},{}",
            effect, text
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
            time_ms,
            640,
            200,
            320,
            100,
            0,
            &[],
        );
        buf
    }

    fn painted_cols(buf: &RenderBuffer) -> (u32, u32) {
        let mut min_x = u32::MAX;
        let mut max_x = 0;
        for (i, px) in buf.as_bytes().chunks_exact(4).enumerate() {
            if px[3] > 0 {
                let x = (i as u32) % buf.width;
                min_x = min_x.min(x);
                max_x = max_x.max(x);
            }
        }
        (min_x, max_x)
    }

    fn painted_rows(buf: &RenderBuffer) -> (u32, u32) {
        let mut min_y = u32::MAX;
        let mut max_y = 0;
        for (i, px) in buf.as_bytes().chunks_exact(4).enumerate() {
            if px[3] > 0 {
                let y = (i as u32) / buf.width;
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
        (min_y, max_y)
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

    /// Plan #28: `\pN` modes halve drawing units per step.
    #[test]
    fn test_drawing_unit_scale_p_modes() {
        assert_eq!(drawing_unit_scale(2.0, 2.0, 0), 2.0);
        assert_eq!(drawing_unit_scale(2.0, 2.0, 1), 2.0);
        assert_eq!(drawing_unit_scale(2.0, 2.0, 2), 1.0);
        assert_eq!(drawing_unit_scale(2.0, 2.0, 3), 0.5);
        assert_eq!(drawing_unit_scale(2.0, 2.0, 4), 0.25);
    }

    /// Plan #28: end-to-end `\p2` renders at quarter area of `\p1`.
    #[test]
    fn test_drawing_p2_renders_smaller_than_p1() {
        let square = "m 0 0 l 40 0 l 40 40 l 0 40";
        let p1 = painted_pixels(&render_event_text(&format!("{{\\p1}}{square}"), true));
        let p2 = painted_pixels(&render_event_text(&format!("{{\\p2}}{square}"), true));
        assert!(p1 > 200, "p1 pixels: {p1}");
        assert!(p2 > 20, "p2 pixels: {p2}");
        let ratio = p1 as f64 / p2 as f64;
        assert!(
            (3.0..5.0).contains(&ratio),
            "area ratio ~4x, got {ratio} ({p1}/{p2})"
        );
    }

    /// Plan #28: `\pbo` contributes to line ascent/descent like libass.
    #[test]
    fn test_pbo_shifts_drawing() {
        fn min_row(buf: &RenderBuffer) -> u32 {
            buf.as_bytes()
                .chunks_exact(4)
                .enumerate()
                .filter(|(_, p)| p[3] > 0)
                .map(|(i, _)| (i as u32) / buf.width)
                .min()
                .unwrap_or(u32::MAX)
        }
        let square = "m 0 0 l 40 0 l 40 40 l 0 40";
        let plain = render_event_text(&format!("{{\\p1}}{square}"), true);
        let shifted = render_event_text(&format!("{{\\p1\\pbo-20}}{square}"), true);
        assert_eq!(
            min_row(&shifted),
            min_row(&plain),
            "single drawings keep their ink row"
        );
    }

    #[test]
    fn test_drawing_preserves_min_x() {
        // libass `offset.x = 0`: a drawing's left bearing stays
        // empty and the advance is the bbox width (probe: the
        // second square of `{\p1}...{\r}m 80 0 ...` starts 80u
        // past the pen). At unit scale the ink shifts by exactly
        // min_x with an unchanged width.
        let base = render_text("{\\p1}m 0 0 l 60 0 l 60 60 l 0 60", 1000);
        let off = render_text("{\\p1}m 80 0 l 140 0 l 140 60 l 80 60", 1000);
        let (b0, b1) = painted_cols(&base);
        let (o0, o1) = painted_cols(&off);
        assert_eq!(o0 as i32 - b0 as i32, 80, "ink left: {b0} -> {o0}");
        assert_eq!(o1 - o0, b1 - b0, "ink width unchanged");
    }

    #[test]
    fn test_drawing_preserves_min_y() {
        // libass hangs the box one HEIGHT above the baseline
        // (`offset.y = -asc`, `asc = height`), so min_y pushes ink
        // below the baseline (probe: min_y=30 renders 20px lower
        // at 0.667 scale). Same-height boxes share the layout and
        // differ only by the min_y shift.
        let base = render_text("{\\p1}m 0 0 l 60 0 l 60 30 l 0 30", 1000);
        let off = render_text("{\\p1}m 0 30 l 60 30 l 60 60 l 0 60", 1000);
        let (b0, b1) = painted_rows(&base);
        let (o0, o1) = painted_rows(&off);
        assert_eq!(o0 as i32 - b0 as i32, 30, "ink top: {b0} -> {o0}");
        assert_eq!(o1 - o0, b1 - b0, "ink height unchanged");
    }

    #[test]
    fn test_kf_drawing_split_at_fractional_scale() {
        // `\kf` splits drawings at ink-left + frac * advance with
        // no extra unit scaling (probe: 60u square at x=80 splits
        // at its ink middle). At 0.667 unit scale the midpoint
        // split must sit at the ink middle, not at 0.667x of it.
        let style = Style::new("Default");
        let buf = render_sized(
            "{\\kf100}{\\p1}m 0 0 l 60 0 l 60 60 l 0 60{\\p0}",
            &style,
            500,
            384,
            216,
            256,
            144,
            true,
        );
        let bytes = buf.as_bytes();
        let is_primary = |p: &[u8]| p[0] > 200 && p[1] > 200 && p[2] > 200 && p[3] > 0;
        let is_secondary = |p: &[u8]| p[1] > 200 && p[0] < 50 && p[2] < 50 && p[3] > 0;
        // Solid columns on the middle ink row: primaries left,
        // secondaries right, with one hard edge between them.
        let (_, max_y) = painted_rows(&buf);
        let (min_x, max_x) = painted_cols(&buf);
        let y = (painted_rows(&buf).0 + max_y) / 2;
        let mut last_primary: Option<u32> = None;
        let mut first_secondary: Option<u32> = None;
        for x in min_x..=max_x {
            let i = (y * buf.width + x) as usize * 4;
            let p = &bytes[i..i + 4];
            if is_primary(p) {
                last_primary = Some(x);
            } else if is_secondary(p) && first_secondary.is_none() {
                first_secondary = Some(x);
            }
        }
        let (lp, fs) = (
            last_primary.expect("primary half"),
            first_secondary.expect("secondary half"),
        );
        assert!(fs > lp && fs - lp <= 2, "hard edge at {lp}/{fs}");
        let mid = (f64::from(min_x) + f64::from(max_x)) / 2.0;
        assert!(
            (f64::from(fs) - mid).abs() <= 3.0,
            "split at ink middle {mid}: edge {lp}/{fs} over {min_x}..={max_x}"
        );
    }

    /// Ink mask (alpha > 0) of a buffer.
    fn ink_mask(buf: &RenderBuffer) -> Vec<bool> {
        buf.as_bytes().chunks_exact(4).map(|p| p[3] > 0).collect()
    }

    #[test]
    fn test_deco_underline_continuous_over_spaces() {
        // Bars span every glyph's full advance (libass `add_rect(0,
        // .., adv, ..)`), including spaces via bar-only glyphs: the
        // bar under "H H" has no gap at the space.
        let plain = ink_mask(&render_text("H H", 1000));
        let deco = render_text("{\\u1}H H", 1000);
        let ink = ink_mask(&deco);
        let w = deco.width as usize;
        // Bar rows: ink in deco where plain has none (the 'H's have
        // no descenders, so underline rows are glyph-free).
        let bar_rows: Vec<u32> = (0..deco.height)
            .filter(|y| (*y as usize * w..(*y as usize + 1) * w).any(|i| ink[i] && !plain[i]))
            .collect();
        assert!(!bar_rows.is_empty(), "underline adds rows");
        for y in bar_rows {
            let row: Vec<bool> = (0..w).map(|x| ink[y as usize * w + x]).collect();
            let first = row.iter().position(|b| *b).expect("bar ink");
            let last = row.iter().rposition(|b| *b).expect("bar ink");
            assert!(
                row[first..=last].iter().all(|b| *b),
                "bar row {y} has a gap (space not spanned)"
            );
        }
    }

    #[test]
    fn test_deco_underline_and_strikeout_both_paint() {
        // libass allows both DECO flags at once (up to two bars per
        // glyph); the old segment pass drew only the underline.
        let plain = render_text("U", 1000);
        // Underline: the bar dips below the glyph's outline/shadow.
        let both = render_text("{\\u1\\s1}U", 1000);
        assert!(
            painted_rows(&both).1 > painted_rows(&plain).1,
            "underline extends below the glyph"
        );
        // Strikeout: new ink inside the glyph's vertical span (the
        // bar crosses the U's hollow middle), beyond underline alone.
        let under = ink_mask(&render_text("{\\u1}U", 1000));
        let ink = ink_mask(&both);
        let w = both.width as usize;
        let (top, bottom) = painted_rows(&plain);
        let inside = (top..=bottom)
            .any(|y| (y as usize * w..(y as usize + 1) * w).any(|i| ink[i] && !under[i]));
        assert!(inside, "strikeout row expected inside the glyph");
    }

    #[test]
    fn test_deco_bar_rotates_with_text() {
        // Bars are glyph-outline geometry (probe: `{\frz90\u1}Hello`
        // shows a vertical bar), not axis-aligned rects.
        let plain = ink_mask(&render_text("{\\frz90}H", 1000));
        let deco = render_text("{\\frz90\\u1}H", 1000);
        let ink = ink_mask(&deco);
        let w = deco.width as usize;
        let diff: Vec<(u32, u32)> = ink
            .iter()
            .zip(plain.iter())
            .enumerate()
            .filter(|(_, (d, p))| **d && !**p)
            .map(|(i, _)| ((i % w) as u32, (i / w) as u32))
            .collect();
        assert!(diff.len() > 10, "rotated bar adds ink");
        let (mut x0, mut x1) = (u32::MAX, 0u32);
        let (mut y0, mut y1) = (u32::MAX, 0u32);
        for (x, y) in &diff {
            x0 = x0.min(*x);
            x1 = x1.max(*x);
            y0 = y0.min(*y);
            y1 = y1.max(*y);
        }
        let (bw, bh) = (x1 - x0 + 1, y1 - y0 + 1);
        assert!(
            bh > 3 * bw && bh > 10,
            "bar must be vertical after frz90, got {bw}x{bh}"
        );
    }

    #[test]
    fn test_deco_bar_sweeps_with_karaoke() {
        // Bars ride in the glyph bitmaps, hence sweep with their
        // glyphs (probe: `{\kf100\u1}He` splits the bar white/red).
        // The bar under the space carries no glyph ink, so its
        // colors there are purely the bar's: white left of the
        // midpoint split, secondary right of it.
        let plain = render_text("{\\kf100}H H", 500);
        let plain_ink = ink_mask(&plain);
        let deco = render_text("{\\kf100\\u1}H H", 500);
        let ink = ink_mask(&deco);
        let w = deco.width as usize;
        // Space x-range: the ink gap between the H blocks on a
        // glyph row (outline included, so bar-only remains).
        let mid_row = (painted_rows(&plain).0 + painted_rows(&plain).1) / 2;
        let glyph_cols: Vec<usize> = (0..w)
            .filter(|x| plain_ink[mid_row as usize * w + x])
            .collect();
        let mut gap: Option<(usize, usize)> = None;
        for pair in glyph_cols.windows(2) {
            if pair[1] > pair[0] + 1 {
                gap = Some((pair[0], pair[1]));
                break;
            }
        }
        let (gap_l, gap_r) = gap.expect("ink gap between the H blocks");
        // Bar rows: deco-only ink anywhere on the row.
        let bar_rows: Vec<u32> = (0..deco.height)
            .filter(|y| (*y as usize * w..(*y as usize + 1) * w).any(|i| ink[i] && !plain_ink[i]))
            .collect();
        assert!(!bar_rows.is_empty(), "bar adds rows to the sweep");
        let bytes = deco.as_bytes();
        let is_primary = |p: &[u8]| p[0] > 200 && p[1] > 200 && p[2] > 200 && p[3] > 0;
        let is_secondary = |p: &[u8]| p[1] > 200 && p[0] < 50 && p[2] < 50 && p[3] > 0;
        let split = (gap_l + gap_r) / 2;
        let mut primary = false;
        let mut secondary = false;
        for y in bar_rows {
            for x in gap_l..=gap_r {
                if !ink[y as usize * w + x] || plain_ink[y as usize * w + x] {
                    continue;
                }
                let p = &bytes[(y as usize * w + x) * 4..][..4];
                if x < split {
                    primary |= is_primary(p);
                } else {
                    secondary |= is_secondary(p);
                }
            }
        }
        assert!(primary && secondary, "space bar must split at mid-run");
    }

    #[test]
    fn test_fe_encoding_is_render_neutral() {
        // `\fe` parses and stores (libass parity at the tag level),
        // but charset remapping stays partial by design: shaping
        // consumes Unicode text, so the tag must not change
        // rendering. Byte-identical frames pin the neutrality (see
        // the support matrix).
        let base = render_text("Hello", 1000);
        let tagged = render_text("{\\fe129}Hello", 1000);
        assert_eq!(base.as_bytes(), tagged.as_bytes());
    }

    /// Plan #27: the `\clip(scale, ...)` argument scales drawing units.
    #[test]
    fn test_vector_clip_scale_argument() {
        // Centered band in script coords: covers centered text at
        // scale 1, shrinks away from it at scale 2.
        let band = "m 240 0 l 400 0 l 400 200 l 240 200";
        let keep = render_event_text(&format!("{{\\clip(1,{band})}}Hello"), true);
        assert!(painted_pixels(&keep) > 50, "scale 1 keeps text");
        let drop = render_event_text(&format!("{{\\clip(2,{band})}}Hello"), true);
        assert_eq!(painted_pixels(&drop), 0, "scale 2 drops text");
    }

    /// Plan #27: B-spline vector clips mask without panicking.
    #[test]
    fn test_vector_clip_bspline() {
        let plain = painted_pixels(&render_event_text("Hello", true));
        // Spline hump over the left half only: must remove some pixels.
        let spline = "m 0 0 l 0 200 l 320 200 s 320 100 160 60 0 100 c";
        let clipped = render_event_text(&format!("{{\\clip({spline})}}Hello"), true);
        let kept = painted_pixels(&clipped);
        assert!(kept > 0 && kept < plain, "plain {plain}, kept {kept}");
    }

    /// Plan #27: out-of-viewport vector clips are safe no-ops-ish.
    #[test]
    fn test_vector_clip_outside_viewport_safe() {
        let far = "m 100000 100000 l 200000 100000 l 200000 200000 l 100000 200000";
        let clipped = render_event_text(&format!("{{\\clip({far})}}Hello"), true);
        assert_eq!(painted_pixels(&clipped), 0);
        let iclipped = render_event_text(&format!("{{\\iclip({far})}}Hello"), true);
        assert!(painted_pixels(&iclipped) > 100);
    }

    /// Plan #27: blur happens before clipping, so blurred pixels stay
    /// inside the clip rect instead of bleeding past it.
    #[test]
    fn test_clip_with_blur_stays_inside() {
        let buf = render_event_text("{\\blur5\\clip(240,0,400,200)}Hello", true);
        assert!(painted_pixels(&buf) > 0);
        // Script x 240..400 maps to video x 120..200 at 0.5 scale.
        for (i, px) in buf.as_bytes().chunks_exact(4).enumerate() {
            if px[3] > 0 {
                let x = (i as u32) % buf.width;
                assert!((120..=200).contains(&x), "blur bled to x={x}");
            }
        }
    }

    /// Plan #27: rotation + clip composes (screen-space clip bounds).
    #[test]
    fn test_clip_with_rotation_stays_inside() {
        let buf = render_event_text("{\\frz45\\clip(240,0,400,200)}Hello", true);
        assert!(painted_pixels(&buf) > 0);
        for (i, px) in buf.as_bytes().chunks_exact(4).enumerate() {
            if px[3] > 0 {
                let x = (i as u32) % buf.width;
                assert!((120..=200).contains(&x), "rotated pixel at x={x}");
            }
        }
    }

    /// Plan #24: a right-to-left banner moves left between frames.
    #[test]
    fn test_banner_moves_left_between_frames() {
        let early = render_event_effect("Hello banner", "Banner;20", true, 200);
        let late = render_event_effect("Hello banner", "Banner;20", true, 1200);
        assert!(painted_pixels(&early) > 0);
        assert!(painted_pixels(&late) > 0);
        let (early_min, _) = painted_cols(&early);
        let (late_min, _) = painted_cols(&late);
        assert!(late_min < early_min, "{late_min} < {early_min}");
    }

    /// Plan #24: `lefttoright` banners move right between frames.
    #[test]
    fn test_banner_lefttoright_moves_right() {
        let early = render_event_effect("Hello banner", "Banner;20;1", true, 200);
        let late = render_event_effect("Hello banner", "Banner;20;1", true, 1200);
        assert!(painted_pixels(&early) > 0);
        assert!(painted_pixels(&late) > 0);
        let (_, early_max) = painted_cols(&early);
        let (_, late_max) = painted_cols(&late);
        assert!(late_max > early_max, "{late_max} > {early_max}");
    }

    /// Plan #24: banner disables auto-wrap (single line, VSFilter
    /// wrapStyle 2), while the same text wraps without the effect.
    #[test]
    fn test_banner_disables_wrap() {
        let long = "Hello world this is a long line that must wrap around nicely";
        let plain = render_event_effect(long, "", true, 500);
        let banner = render_event_effect(long, "Banner;5", true, 2500);
        let (plain_top, plain_bottom) = painted_rows(&plain);
        let (banner_top, banner_bottom) = painted_rows(&banner);
        assert!(
            plain_bottom - plain_top > banner_bottom - banner_top,
            "plain {}..{}, banner {}..{}",
            plain_top,
            plain_bottom,
            banner_top,
            banner_bottom
        );
    }

    /// Plan #24: scroll-up stays inside its band and moves up.
    #[test]
    fn test_scroll_up_clips_to_band_and_moves_up() {
        // Band 20..180 script px maps to video rows 10..90.
        let early = render_event_effect("Hello scroll", "Scroll up;20;180;50", true, 1000);
        let late = render_event_effect("Hello scroll", "Scroll up;20;180;50", true, 2000);
        assert!(painted_pixels(&early) > 0);
        assert!(painted_pixels(&late) > 0);
        for buf in [&early, &late] {
            let (top, bottom) = painted_rows(buf);
            assert!((10..90).contains(&top), "top {top}");
            assert!((10..90).contains(&bottom), "bottom {bottom}");
        }
        let (early_top, _) = painted_rows(&early);
        let (late_top, _) = painted_rows(&late);
        assert!(late_top < early_top, "{late_top} < {early_top}");
    }

    /// Plan #24: scroll-down moves down and respects the band.
    #[test]
    fn test_scroll_down_moves_down_inside_band() {
        // Effective delay is 50/0.5 = 100ms/px: at 2000ms the block
        // has entered the band, at 3000ms it has moved 10px further.
        let early = render_event_effect("Hello scroll", "Scroll down;20;180;50", true, 2000);
        let late = render_event_effect("Hello scroll", "Scroll down;20;180;50", true, 3000);
        assert!(painted_pixels(&early) > 0);
        assert!(painted_pixels(&late) > 0);
        for buf in [&early, &late] {
            let (top, bottom) = painted_rows(buf);
            assert!((10..90).contains(&top), "top {top}");
            assert!((10..90).contains(&bottom), "bottom {bottom}");
        }
        let (early_top, _) = painted_rows(&early);
        let (late_top, _) = painted_rows(&late);
        assert!(late_top > early_top, "{late_top} > {early_top}");
    }

    /// Plan #24: `delay = 0` is the fastest finite speed (no hang,
    /// no division by zero): the banner crosses quickly.
    #[test]
    fn test_banner_delay_zero_is_fastest() {
        // Effective delay clamps to 1ms/px: at 100ms the banner has
        // travelled 100px (visible), by 500ms it has left the frame.
        let at100 = render_event_effect("Hi", "Banner;0", true, 100);
        let at500 = render_event_effect("Hi", "Banner;0", true, 500);
        assert!(painted_pixels(&at100) > 0);
        assert_eq!(painted_pixels(&at500), 0);
    }

    /// Plan #24: scroll fadeaway ramps alpha at the band edges.
    #[test]
    fn test_scroll_fadeaway_ramps_alpha() {
        // Three tall lines scrolling down: at 1200ms the block spans
        // the band top (video row 10), so rows 10..30 sit in the top
        // fade ramp (fadeaway 40 script px = 20 video px) while rows
        // past 30 render opaque.
        let buf = render_event_effect("{\\fs40}A\\NB\\NC", "Scroll down;20;180;20;40", true, 1200);
        assert!(painted_pixels(&buf) > 0);
        let row_alpha = |y: u32| -> u8 {
            buf.as_bytes()
                .chunks_exact(4)
                .skip((y * buf.width) as usize)
                .take(buf.width as usize)
                .map(|p| p[3])
                .max()
                .unwrap_or(0)
        };
        assert_eq!(row_alpha(10), 0, "band top fully faded");
        assert!(row_alpha(15) > 0, "ramp row faded in");
        assert!(row_alpha(15) < row_alpha(35), "ramp below opaque rows");
    }

    /// Plan #24: the scroll band clip must not erase other events'
    /// pixels (isolated event buffer + blend-back).
    #[test]
    fn test_scroll_clip_preserves_other_pixels() {
        let mut comp = Compositor::new();
        let mut fm = FontManager::new();
        fm.load_font("DejaVu Sans", font::get_fallback_font(), false, false)
            .unwrap();
        let event = Event::parse_from_line(
            "Dialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,Scroll up;60;140;50,Hi",
        )
        .unwrap();
        let style = Style::new("Default");
        let resolved = Compositor::resolve_style(&style, &event);
        let mut buf = RenderBuffer::new(320, 100).unwrap();
        // Marker "other event" pixel far above the band (rows 30..70).
        buf.blend_pixel(10, 5, 255, 0, 0, 255);
        comp.composite_event(
            &mut buf,
            &event,
            &resolved,
            &fm,
            2500,
            640,
            200,
            320,
            100,
            0,
            &[],
        );
        assert_eq!(buf.get_pixel(10, 5), [255, 0, 0, 255]);
    }

    /// Plan #24: unknown/garbage effects render as plain events.
    #[test]
    fn test_unknown_effect_renders_plain() {
        let plain = render_event_effect("Hello", "", true, 500);
        for effect in ["Karaoke;10", "Banner", "Scroll up;10", "Banner;abc", ";20"] {
            let buf = render_event_effect("Hello", effect, true, 500);
            assert_eq!(
                buf.as_bytes(),
                plain.as_bytes(),
                "effect {effect:?} must be ignored"
            );
        }
    }

    /// Plan #61: `\rName` matching is case-sensitive like every
    /// other style lookup; a case mismatch falls back to the event style.
    #[test]
    fn test_reset_named_style_case_sensitive() {
        let base = Style::new("Default");
        let alt = Style::new("Alt");
        let styles = vec![base.clone(), alt];
        let resolve_after_reset = |text: &str| {
            let event = Event::parse_from_line(&format!(
                "Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{text}"
            ))
            .unwrap();
            let resolved = Compositor::resolve_style(&base, &event);
            let segments = parse_text_segments(&event.text);
            Compositor::resolve_segment_style(&resolved, &segments[1], &event, &styles, 0, 0, 2000)
                .base_style
                .name
                .clone()
        };
        assert_eq!(resolve_after_reset("A{\\rAlt}B"), "Alt");
        assert_eq!(resolve_after_reset("A{\\ralt}B"), "Default");
        assert_eq!(resolve_after_reset("A{\\r Alt }B"), "Alt");
    }

    /// Plan #61: `\rAlt` restores border/margins from the target
    /// style, while nonzero event margins still override the target.
    /// Alignment is NOT restored: libass `ass_reset_render_context`
    /// never touches alignment, so the event alignment survives `\r`
    /// (here the Default style's, since no `\an` tag occurred).
    #[test]
    fn test_reset_restores_target_layout_but_keeps_event_margins() {
        let base = Style::new("Default");
        let mut alt = Style::new("Alt");
        alt.alignment = 7;
        alt.outline = 9.0;
        alt.margin_l = 11;
        let styles = vec![base.clone(), alt];
        let event =
            Event::parse_from_line("Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,33,,A{\\rAlt}B")
                .unwrap();
        let resolved = Compositor::resolve_style(&base, &event);
        let segments = parse_text_segments(&event.text);
        let seg =
            Compositor::resolve_segment_style(&resolved, &segments[1], &event, &styles, 0, 0, 2000);
        assert_eq!(seg.alignment, base.alignment);
        assert_eq!(seg.outline_x, 9.0);
        assert_eq!(seg.margin_l, 11);
        // Event MarginV overrides the target style's margin.
        assert_eq!(seg.margin_v, 33);
    }

    /// `\r` preserves the first `\an`/`\a` (libass `PARSED_A` spans the
    /// whole event): text after the reset keeps the earlier alignment,
    /// and a later alignment tag is ignored.
    #[test]
    fn test_reset_keeps_first_alignment_and_ignores_later() {
        let base = Style::new("Default");
        let styles = vec![base.clone()];
        for (text, want) in [
            ("{\\an7}A{\\r}B", 7),
            ("{\\an7}A{\\r}B{\\an1}C", 7),
            ("A{\\r}B{\\an1}C", 1),
            ("{\\a6}A{\\r}B", 8),
        ] {
            let event = Event::parse_from_line(&format!(
                "Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{text}"
            ))
            .unwrap();
            let resolved = Compositor::resolve_style(&base, &event);
            assert_eq!(resolved.alignment, want, "{text:?} at event level");
            let segments = parse_text_segments(&event.text);
            let last = segments.len() - 1;
            let seg = Compositor::resolve_segment_style(
                &resolved,
                &segments[last],
                &event,
                &styles,
                0,
                0,
                2000,
            );
            assert_eq!(seg.alignment, want, "{text:?} after \\r");
        }
    }

    /// Plan #62: `\q` is event-level and survives `\r`; the last
    /// group wins. Rendered: a `\q2` line stays single-line even
    /// with a mid-line reset, while `\q0` wraps.
    #[test]
    fn test_wrap_override_survives_reset_last_wins() {
        let long = "Hello world this is a long line that must wrap around nicely";
        let nowrap = render_event_text(&format!("{{\\q2}}{long}{{\\r}}tail"), true);
        let (top, bottom) = painted_rows(&nowrap);
        let nowrap_span = bottom - top;
        let wrapped = render_event_text(&format!("{{\\q2}}a{{\\q0}}{long}"), true);
        let (top, bottom) = painted_rows(&wrapped);
        let wrapped_span = bottom - top;
        assert!(
            wrapped_span > nowrap_span + 5,
            "q2 single-line ({nowrap_span}) vs q0 wrapped ({wrapped_span})"
        );
    }

    /// Plan #63: `\an` applies event-wide wherever it appears; the
    /// first occurrence wins (libass `PARSED_A`), including across
    /// `\r` (libass never resets alignment).
    #[test]
    fn test_alignment_first_tag_applies_event_wide() {
        let base = Style::new("Default");
        let resolve = |text: &str| {
            let event = Event::parse_from_line(&format!(
                "Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{text}"
            ))
            .unwrap();
            Compositor::resolve_style(&base, &event).alignment
        };
        assert_eq!(resolve("Hello{\\an7}"), 7);
        assert_eq!(resolve("{\\an7}Hello{\\an1}"), 7);
        assert_eq!(resolve("{\\an9}A{\\r}B"), 9);
        // All \a / \an combinations: first wins.
        assert_eq!(resolve("{\\an7}A{\\an1}B"), 7);
        assert_eq!(resolve("{\\a6}A{\\an1}B"), 8);
        assert_eq!(resolve("{\\an7}A{\\a1}B"), 7);
        // Bare / out-of-range tags reset to the style and still
        // consume the slot, blocking later alignment tags.
        assert_eq!(resolve("{\\an}A{\\an7}B"), base.alignment);
        assert_eq!(resolve("{\\an0}A{\\an7}B"), base.alignment);
        assert_eq!(resolve("{\\a12}A{\\an7}B"), base.alignment);
        // Render-level: first \an7 wins over a later \an1 (ink up top).
        let first = render_text("{\\an7}AAAA{\\an1}", 1000);
        let lone = render_text("{\\an7}AAAA", 1000);
        assert_eq!(first.as_bytes(), lone.as_bytes());
    }

    /// Plan #64/#65: `\pos` and `\move` share one slot — the first
    /// tag wins in either order (libass `EVENT_POSITIONED`).
    #[test]
    fn test_pos_move_first_wins_both_orders() {
        let base = Style::new("Default");
        let resolve = |text: &str| {
            let event = Event::parse_from_line(&format!(
                "Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{text}"
            ))
            .unwrap();
            Compositor::resolve_style(&base, &event)
        };
        // Pos-before-move keeps \pos; the \move is ignored.
        let r = resolve("{\\pos(10,10)}A{\\move(0,0,100,0)}B");
        assert_eq!(r.position, Some((10.0, 10.0)));
        assert!(r.move_data.is_none());
        // Move-before-pos keeps \move; the \pos is ignored.
        let r = resolve("{\\move(0,0,100,0)}A{\\pos(100,100)}B");
        assert!(r.move_data.is_some());
        assert!(r.position.is_none());
        // Repeated tags: first wins.
        let r = resolve("{\\pos(10,10)}A{\\pos(100,100)}B");
        assert_eq!(r.position, Some((10.0, 10.0)));
        let r = resolve("{\\move(0,0,10,0)}A{\\move(0,0,100,0)}B");
        assert!(r.move_data.is_some());
        // Render-level: pos-before-move is static across frames and
        // identical to a lone \pos; move-before-pos animates.
        let text = "{\\pos(10,10)\\move(0,0,100,0)}Hi";
        let a = render_event_effect(text, "", true, 500);
        let b = render_event_effect(text, "", true, 1500);
        assert_eq!(a.as_bytes(), b.as_bytes());
        let lone = render_event_effect("{\\pos(10,10)}Hi", "", true, 500);
        assert_eq!(a.as_bytes(), lone.as_bytes());
        let text = "{\\move(200,150,400,150)\\pos(100,100)}Hi";
        let a = render_event_effect(text, "", true, 500);
        let b = render_event_effect(text, "", true, 1500);
        assert_ne!(a.as_bytes(), b.as_bytes());
    }

    /// Plan #64: `\fad` and `\fade` share one slot — the first tag
    /// wins in either order (libass `PARSED_FADE`).
    #[test]
    fn test_fad_fade_first_wins_both_orders() {
        let base = Style::new("Default");
        let resolve = |text: &str| {
            let event = Event::parse_from_line(&format!(
                "Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{text}"
            ))
            .unwrap();
            Compositor::resolve_style(&base, &event)
        };
        let fade = "\\fade(255,0,255,0,500,1500,2000)";
        // fad-before-fade keeps \fad; fade-before-fad keeps \fade.
        let r = resolve(&format!("{{\\fad(100,200)}}A{{{fade}}}B"));
        assert!(r.complex_fade.is_none());
        assert_eq!((r.fade_in, r.fade_out), (100, 200));
        let r = resolve(&format!("{{{fade}}}A{{\\fad(100,200)}}B"));
        assert!(r.complex_fade.is_some());
        assert_eq!((r.fade_in, r.fade_out), (0, 0));
        // Repeated same-form tags: first wins.
        let r = resolve("{\\fad(100,200)}A{\\fad(300,400)}B");
        assert_eq!((r.fade_in, r.fade_out), (100, 200));
        let r = resolve(&format!("{{{fade}}}A{{{fade}}}B"));
        assert!(r.complex_fade.is_some());
        // Even \fad(0,0) consumes the slot (indistinguishable from
        // unset without the flag, but libass still sets PARSED_FADE).
        let r = resolve(&format!("{{\\fad(0,0)}}A{{{fade}}}B"));
        assert!(r.complex_fade.is_none());
        assert_eq!((r.fade_in, r.fade_out), (0, 0));
        // Render-level: fad-before-fade matches a lone \fad.
        let text = "{\\fad(500,500)}Hi";
        let first = render_event_effect(&format!("{{\\fad(500,500){fade}}}Hi"), "", true, 250);
        let lone = render_event_effect(text, "", true, 250);
        assert_eq!(first.as_bytes(), lone.as_bytes());
    }

    /// Plan #66: libass keeps rectangular and vector clipping as
    /// separate state — later rect coordinates replace earlier ones,
    /// `\clip` vs `\iclip` flips the rect mode, the first vector clip
    /// is retained, and rect + vector clips coexist in rendering.
    #[test]
    fn test_clip_libass_rect_vector_semantics() {
        let base = Style::new("Default");
        let resolve = |text: &str| {
            let event = Event::parse_from_line(&format!(
                "Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{text}"
            ))
            .unwrap();
            Compositor::resolve_style(&base, &event)
        };
        let vector = "\\clip(m 0 0 l 10 0 l 10 10)";
        let ivector = "\\iclip(m 0 0 l 10 0 l 10 10)";
        // rect -> rect: later coordinates win.
        let r = resolve("{\\clip(0,0,10,10)}A{\\clip(1,1,9,9)}B");
        assert_eq!(r.clip, Some((1, 1, 9, 9)));
        assert!(r.inverse_clip.is_none());
        // clip -> iclip and back: mode flips with the latest form.
        let r = resolve("{\\clip(0,0,10,10)}A{\\iclip(1,1,9,9)}B");
        assert!(r.clip.is_none());
        assert_eq!(r.inverse_clip, Some((1, 1, 9, 9)));
        let r = resolve("{\\iclip(1,1,9,9)}A{\\clip(0,0,10,10)}B");
        assert_eq!(r.clip, Some((0, 0, 10, 10)));
        assert!(r.inverse_clip.is_none());
        // rect -> vector and back: both survive (separate state).
        let r = resolve(&format!("{{\\clip(0,0,10,10)}}A{{{vector}}}B"));
        assert_eq!(r.clip, Some((0, 0, 10, 10)));
        assert!(r.clip_vector.is_some());
        let r = resolve(&format!("{{{vector}}}A{{\\clip(0,0,10,10)}}B"));
        assert_eq!(r.clip, Some((0, 0, 10, 10)));
        assert!(r.clip_vector.is_some());
        // vector -> vector: the first vector clip is retained, even
        // across normal/inverse forms (either consumes the slot).
        let r = resolve(&format!("{{{vector}}}A{{{vector}}}B"));
        assert!(r.clip_vector.is_some());
        assert!(r.inverse_clip_vector.is_none());
        let r = resolve(&format!("{{{vector}}}A{{{ivector}}}B"));
        assert!(r.clip_vector.is_some());
        assert!(r.inverse_clip_vector.is_none());
        let r = resolve(&format!("{{{ivector}}}A{{{vector}}}B"));
        assert!(r.clip_vector.is_none());
        assert!(r.inverse_clip_vector.is_some());
        // vector iclip -> rect clip and back: rect never disturbs the
        // vector slot and the vector never disturbs the rect slot.
        let r = resolve(&format!("{{{ivector}}}A{{\\clip(0,0,10,10)}}B"));
        assert_eq!(r.clip, Some((0, 0, 10, 10)));
        assert!(r.inverse_clip_vector.is_some());
        let r = resolve(&format!("{{\\clip(0,0,10,10)}}A{{{ivector}}}B"));
        assert_eq!(r.clip, Some((0, 0, 10, 10)));
        assert!(r.inverse_clip_vector.is_some());
        // Render-level: rect + vector clips both apply. Centered
        // text spans the frame middle, so a left-half rect and a
        // right-half vector each keep ink alone, while their
        // (near-empty) intersection keeps strictly less than either.
        let rect_only = render_event_text("{\\clip(0,0,320,200)}Hello", true);
        let vector_only =
            render_event_text("{\\clip(m 320 0 l 640 0 l 640 200 l 320 200)}Hello", true);
        let both = render_event_text(
            "{\\clip(0,0,320,200)\\clip(m 320 0 l 640 0 l 640 200 l 320 200)}Hello",
            true,
        );
        let rect_ink = painted_pixels(&rect_only);
        let vector_ink = painted_pixels(&vector_only);
        let both_ink = painted_pixels(&both);
        assert!(rect_ink > 0 && vector_ink > 0, "each clip keeps ink alone");
        assert!(
            both_ink < rect_ink,
            "rect+vector ({both_ink}) must keep less than rect-only ({rect_ink})"
        );
        assert!(
            both_ink < vector_ink,
            "rect+vector ({both_ink}) must keep less than vector-only ({vector_ink})"
        );
    }

    /// First `\org` wins (libass `have_origin`); later origins are
    /// ignored. Render-level: the rotation pivot follows the first
    /// origin, so an ignored second origin renders identically.
    #[test]
    fn test_org_first_wins() {
        let base = Style::new("Default");
        let resolve = |text: &str| {
            let event = Event::parse_from_line(&format!(
                "Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{text}"
            ))
            .unwrap();
            Compositor::resolve_style(&base, &event)
        };
        let r = resolve("{\\org(10,20)}A{\\org(100,200)}B");
        assert_eq!(r.origin, Some((10.0, 20.0)));
        let r = resolve("{\\org(100,200)}A{\\org(10,20)}B");
        assert_eq!(r.origin, Some((100.0, 200.0)));
        let first = render_event_text("{\\frz30\\org(10,20)\\org(300,100)}Spun", true);
        let lone = render_event_text("{\\frz30\\org(10,20)}Spun", true);
        assert_eq!(first.as_bytes(), lone.as_bytes());
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
        // \r exits drawing mode too.
        let (text, drawing) = comp.extract_clean_text("{\\p1}m 0 0{\\r}a\\Nb", 0);
        assert!(!drawing);
        assert_eq!(text, "m 0 0a\nb");
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
    fn test_transform_timing_forms() {
        // \t(accel) / \t(t1,t2) / \t(t1,t2,accel) parse distinctly and
        // animate only inside their window.
        let base = Style::new("Default");
        let event =
            Event::parse_from_line("Dialogue: 0,0:00:00.00,0:00:10.00,Default,,0,0,0,,x").unwrap();
        let resolved = Compositor::resolve_style(&base, &event);
        let size_at = |text: &str, time_ms: u64| {
            let segments = parse_text_segments(text);
            Compositor::resolve_segment_style(
                &resolved,
                &segments[0],
                &event,
                &[],
                time_ms,
                0,
                10_000,
            )
            .font_size
        };
        // Full form: idle before t1, midpoint halfway, target after t2.
        assert_eq!(size_at(r"{\t(1000,2000,\fs60)}x", 500), base.font_size);
        assert!((size_at(r"{\t(1000,2000,\fs60)}x", 1500) - 54.0).abs() < 1e-9);
        assert_eq!(size_at(r"{\t(1000,2000,\fs60)}x", 2500), 60.0);
        // Accel-only form spans the whole event (t1=t2=0).
        let tags = OverrideTag::parse_from_text(r"{\t(2.0,\fs60)}");
        assert!(matches!(
            tags[0],
            OverrideTag::Transform { t1: 0, t2: 0, .. }
        ));
        assert!((size_at(r"{\t(2.0,\fs60)}x", 5000) - 51.0).abs() < 1e-9);
        // (t1,t2) form defaults to linear (accel 1).
        let tags = OverrideTag::parse_from_text(r"{\t(100,200,\fs60)}");
        assert!(matches!(
            tags[0],
            OverrideTag::Transform {
                t1: 100,
                t2: 200,
                ..
            }
        ));
        assert!((size_at(r"{\t(100,200,\fs60)}x", 150) - 54.0).abs() < 1e-9);
    }

    #[test]
    fn test_move_reversed_times_render_identical() {
        // libass swaps t1 > t2: reversed renders byte-identical to normal.
        for t in [500, 1500, 2500] {
            let reversed = render_text(r"{\move(20,20,300,20,2000,1000)}X", t);
            let normal = render_text(r"{\move(20,20,300,20,1000,2000)}X", t);
            assert_eq!(
                reversed.as_bytes(),
                normal.as_bytes(),
                "reversed vs normal @ {t}ms"
            );
        }
        // And the midpoint is actually mid-flight (not stuck at an end).
        let mid = render_text(r"{\move(20,20,300,20,2000,1000)}X", 1500);
        let start = render_text(r"{\pos(20,20)}X", 1500);
        let end = render_text(r"{\pos(300,20)}X", 1500);
        assert_ne!(mid.as_bytes(), start.as_bytes());
        assert_ne!(mid.as_bytes(), end.as_bytes());
    }

    #[test]
    fn test_move_equal_times_is_instant_step() {
        // Equal nonzero times are an instant transition AT the stamp:
        // before and exactly at render at (x1,y1), after at (x2,y2).
        // (libass `t <= t1 -> k = 0`, verified against 0.17.5 frames.)
        let before = render_text(r"{\move(20,20,300,20,1000,1000)}X", 999);
        let at = render_text(r"{\move(20,20,300,20,1000,1000)}X", 1000);
        let after = render_text(r"{\move(20,20,300,20,1000,1000)}X", 1001);
        let from = render_text(r"{\pos(20,20)}X", 1000);
        let to = render_text(r"{\pos(300,20)}X", 1001);
        assert_eq!(before.as_bytes(), from.as_bytes(), "before == start pos");
        assert_eq!(at.as_bytes(), from.as_bytes(), "at == start pos");
        assert_eq!(after.as_bytes(), to.as_bytes(), "after == end pos");
        // Only the untimed form animates across the whole event.
        let animated = render_text(r"{\move(20,20,300,20)}X", 999);
        assert_ne!(animated.as_bytes(), from.as_bytes());
    }

    #[test]
    fn test_move_zero_and_negative_times_span_event() {
        // Explicit (0,0), like the untimed form, animates the whole event.
        for t in [1000, 2500, 4000] {
            let explicit = render_text(r"{\move(20,20,300,20,0,0)}X", t);
            let untimed = render_text(r"{\move(20,20,300,20)}X", t);
            assert_eq!(explicit.as_bytes(), untimed.as_bytes(), "t={t}");
        }
        // libass `t1 <= 0 && t2 <= 0` also covers negative pairs.
        for t in [1000, 2500, 4000] {
            let negative = render_text(r"{\move(20,20,300,20,-5,-5)}X", t);
            let untimed = render_text(r"{\move(20,20,300,20)}X", t);
            assert_eq!(negative.as_bytes(), untimed.as_bytes(), "t={t}");
        }
    }

    #[test]
    fn test_move_wrong_arity_renders_unpositioned() {
        // Ignored tags position nothing: identical to plain text.
        for text in [
            r"{\move(20,20,300,20,1000)}X",
            r"{\move(20,20,300)}X",
            r"{\pos(20,20,30)}X",
            r"{\pos(20)}X",
        ] {
            let ignored = render_text(text, 1000);
            let plain = render_text("X", 1000);
            assert_eq!(ignored.as_bytes(), plain.as_bytes(), "{text:?}");
        }
    }

    #[test]
    fn test_malformed_alignment_first_consumes_slot() {
        // Plan P1: the first alignment-like tag consumes PARSED_A even
        // when malformed, so a later `\an7` cannot win. Style alignment
        // is 2 (bottom-center); `\an7` alone would move ink top-left.
        let plain = render_text("X", 1000);
        let top_left = render_text(r"{\an7}X", 1000);
        assert_ne!(plain.as_bytes(), top_left.as_bytes());
        for text in [
            r"{\anfoo\an7}X",
            r"{\afoo\an7}X",
            r"{\an\an7}X",
            r"{\an99\an7}X",
        ] {
            let rendered = render_text(text, 1000);
            assert_eq!(
                rendered.as_bytes(),
                plain.as_bytes(),
                "{text:?} must fall back to the style"
            );
        }
    }

    #[test]
    fn test_transform_equal_times_apply_fully() {
        // Unlike `\move`, `\t` uses strict `<` on the left: at exactly
        // `t1 == t2` the tag fully applies (libass `t >= t2 -> k = 1`).
        let base = Style::new("Default"); // 48.0
        let event =
            Event::parse_from_line("Dialogue: 0,0:00:00.00,0:00:10.00,Default,,0,0,0,,x").unwrap();
        let resolved = Compositor::resolve_style(&base, &event);
        let size_at = |text: &str, time_ms: u64| {
            let segments = parse_text_segments(text);
            Compositor::resolve_segment_style(
                &resolved,
                &segments[0],
                &event,
                &[],
                time_ms,
                0,
                10_000,
            )
            .font_size
        };
        assert_eq!(size_at(r"{\t(1000,1000,\fs60)}x", 999), 48.0);
        assert_eq!(size_at(r"{\t(1000,1000,\fs60)}x", 1000), 60.0);
        assert_eq!(size_at(r"{\t(1000,1000,\fs60)}x", 1001), 60.0);
    }

    #[test]
    fn test_transform_accel_zero_applies_instantly() {
        // libass `pow(t, 0) == 1`: accel 0 applies from the window start.
        let base = Style::new("Default"); // 48.0
        let event =
            Event::parse_from_line("Dialogue: 0,0:00:00.00,0:00:10.00,Default,,0,0,0,,x").unwrap();
        let resolved = Compositor::resolve_style(&base, &event);
        let size_at = |text: &str, time_ms: u64| {
            let segments = parse_text_segments(text);
            Compositor::resolve_segment_style(
                &resolved,
                &segments[0],
                &event,
                &[],
                time_ms,
                0,
                10_000,
            )
            .font_size
        };
        assert_eq!(size_at(r"{\t(100,200,0,\fs60)}x", 99), 48.0);
        assert_eq!(size_at(r"{\t(100,200,0,\fs60)}x", 100), 60.0);
        assert_eq!(size_at(r"{\t(100,200,0,\fs60)}x", 150), 60.0);
    }

    #[test]
    fn test_fsc_resets_both_scale_axes() {
        let base = Style::new("Default"); // scale 100/100
        let event = Event::parse_from_line(
            "Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{\\fscx200\\fscy50\\fsc}Hi",
        )
        .unwrap();
        let r = Compositor::resolve_style(&base, &event);
        assert_eq!((r.scale_x, r.scale_y), (100.0, 100.0));
        let event = Event::parse_from_line(
            "Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{\\fscx200\\fscy50}Hi",
        )
        .unwrap();
        let r = Compositor::resolve_style(&base, &event);
        assert_eq!((r.scale_x, r.scale_y), (200.0, 50.0));
    }

    #[test]
    fn test_wrap_style_out_of_range_falls_back() {
        // libass `tag("q")`: outside 0-3 the track default applies.
        let plain = render_text("A\\nB", 1000);
        for text in ["A\\nB{\\q5}", "A\\nB{\\q-1}", "A\\nB{\\q99}"] {
            assert_eq!(
                render_text(text, 1000).as_bytes(),
                plain.as_bytes(),
                "{text:?}"
            );
        }
        // A valid non-default wrap still takes effect.
        assert_ne!(
            render_text("A\\nB{\\q2}", 1000).as_bytes(),
            plain.as_bytes()
        );
    }

    #[test]
    fn test_relative_fs_scales_current_size() {
        // libass: \fs+10 doubles, \fs-5 halves the *current* size.
        let base = Style::new("Default"); // 48.0
        let event =
            Event::parse_from_line("Dialogue: 0,0:00:00.00,0:00:10.00,Default,,0,0,0,,x").unwrap();
        let resolved = Compositor::resolve_style(&base, &event);
        let size_of = |text: &str| {
            let segments = parse_text_segments(text);
            Compositor::resolve_segment_style(&resolved, &segments[0], &event, &[], 1000, 0, 10_000)
                .font_size
        };
        assert!((size_of(r"{\fs+10}x") - 96.0).abs() < 1e-9);
        assert!((size_of(r"{\fs-5}x") - 24.0).abs() < 1e-9);
        // Relative applies to earlier tags in the same group (chained).
        assert!((size_of(r"{\fs24\fs+10}x") - 48.0).abs() < 1e-9);
        assert!((size_of(r"{\fs+10\fs+10}x") - 192.0).abs() < 1e-9);
        // Absolute still assigns.
        assert_eq!(size_of(r"{\fs24}x"), 24.0);
        // Bare \fs and non-positive results reset to the style size.
        assert_eq!(size_of(r"{\fs24\fs}x"), base.font_size);
        assert_eq!(size_of(r"{\fs0}x"), base.font_size);
        assert_eq!(size_of(r"{\fs-10}x"), base.font_size);
        assert_eq!(
            size_of(r"{\t(1000,2000,\fs+10)}x"),
            base.font_size,
            "transform window not started"
        );
    }

    #[test]
    fn test_relative_fs_inside_transform_uses_progress() {
        // libass: inside \t, relative \fs scales by (1 + p*d/10).
        let base = Style::new("Default"); // 48.0
        let event =
            Event::parse_from_line("Dialogue: 0,0:00:00.00,0:00:10.00,Default,,0,0,0,,x").unwrap();
        let resolved = Compositor::resolve_style(&base, &event);
        let size_at = |text: &str, time_ms: u64| {
            let segments = parse_text_segments(text);
            Compositor::resolve_segment_style(
                &resolved,
                &segments[0],
                &event,
                &[],
                time_ms,
                0,
                10_000,
            )
            .font_size
        };
        // p = 0.5 at t = 1500: 48 * 1.5 = 72.
        assert!((size_at(r"{\t(1000,2000,\fs+10)}x", 1500) - 72.0).abs() < 1e-9);
        // p = 1 after the window: 48 * 2 = 96.
        assert!((size_at(r"{\t(1000,2000,\fs+10)}x", 2500) - 96.0).abs() < 1e-9);
        // Negative delta shrinks toward zero, then resets at the floor.
        assert!((size_at(r"{\t(1000,2000,\fs-5)}x", 1500) - 36.0).abs() < 1e-9);
        assert_eq!(size_at(r"{\t(1000,2000,\fs-10)}x", 2500), base.font_size);
    }

    #[test]
    fn test_transform_animates_supported_set() {
        // Every animatable tag reaches its target at progress 1.
        let base = Style::new("Default");
        let event =
            Event::parse_from_line("Dialogue: 0,0:00:00.00,0:00:10.00,Default,,0,0,0,,x").unwrap();
        let resolved = Compositor::resolve_style(&base, &event);
        let segments = parse_text_segments(
            r"{\t(0,1,\c&H0000FF&\3c&H00FF00&\alpha&H80&\fs60\fscx150\fscy80\fsp4\frz45\frx10\fry20\fax1\fay2\bord5\xbord6\ybord7\shad3\xshad4\yshad5\blur2)}x",
        );
        let seg = Compositor::resolve_segment_style(
            &resolved,
            &segments[0],
            &event,
            &[],
            5000,
            0,
            10_000,
        );
        assert_eq!(seg.color, Color::new(0x80, 255, 0, 0));
        assert_eq!(seg.outline_color.green, 255);
        assert_eq!(seg.font_size, 60.0);
        assert_eq!(seg.scale_x, 150.0);
        assert_eq!(seg.scale_y, 80.0);
        assert_eq!(seg.spacing, 4.0);
        assert_eq!(seg.angle, 45.0);
        assert_eq!(seg.rotation_x, 10.0);
        assert_eq!(seg.rotation_y, 20.0);
        assert_eq!(seg.shear_x, 1.0);
        assert_eq!(seg.shear_y, 2.0);
        assert_eq!(seg.outline_x, 6.0);
        assert_eq!(seg.outline_y, 7.0);
        assert_eq!(seg.shadow_x, 4.0);
        assert_eq!(seg.shadow_y, 5.0);
        assert_eq!(seg.blur, 2.0);
    }

    #[test]
    fn test_transform_applies_libass_discrete_and_global_tags() {
        // libass consumes discrete and event-global tags while recursively
        // parsing \t. Continuous tags still interpolate by transform power.
        let base = Style::new("Default");
        let event =
            Event::parse_from_line("Dialogue: 0,0:00:00.00,0:00:10.00,Default,,0,0,0,,x").unwrap();
        let resolved = Compositor::resolve_style(&base, &event);
        let segments = parse_text_segments(
            r"{\t(0,1,\pos(1,2)\org(5,6)\clip(0,0,9,9)\an7\p1\b1\i1\fnOther\fe2)}x",
        );
        let seg = Compositor::resolve_segment_style(
            &resolved,
            &segments[0],
            &event,
            &[],
            5000,
            0,
            10_000,
        );
        assert_eq!(seg.position, Some((1.0, 2.0)));
        assert_eq!(seg.origin, Some((5.0, 6.0)));
        assert_eq!(seg.clip, Some((0, 0, 9, 9)));
        assert_eq!(seg.alignment, 7);
        assert_eq!(seg.drawing_mode, 1);
        assert_eq!(seg.font_weight, 700);
        assert!(seg.italic);
        assert_eq!(seg.font_name, "Other");
        assert_eq!(seg.font_encoding, 2);
    }

    #[test]
    fn test_nested_transform_uses_own_timing() {
        let base = Style::new("Default");
        let event =
            Event::parse_from_line("Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,x").unwrap();
        let resolved = Compositor::resolve_style(&base, &event);
        let segments = parse_text_segments(r"{\t(0,1000,\fs40\t(500,1000,\fs60))}x");

        let early =
            Compositor::resolve_segment_style(&resolved, &segments[0], &event, &[], 250, 0, 1000);
        let outer_early = base.font_size + (40.0 - base.font_size) * 0.25;
        assert!((early.font_size - outer_early).abs() < 1e-9);

        let late =
            Compositor::resolve_segment_style(&resolved, &segments[0], &event, &[], 750, 0, 1000);
        // Outer progress is .75, nested progress is .5 toward 60pt.
        let outer_late = base.font_size + (40.0 - base.font_size) * 0.75;
        let nested_late = outer_late + (60.0 - outer_late) * 0.5;
        assert!((late.font_size - nested_late).abs() < 1e-9);
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
    fn test_back_colour_override_sets_shadow() {
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
    fn test_wrap_style_0_balances_lines() {
        let font = fallback_font();
        let word_w = TextShaper::measure_text("aa", &font, 48.0, 0.0);
        let space_w = TextShaper::measure_text(" ", &font, 48.0, 0.0);
        // Room for exactly three words per line: greedy alone would emit
        // 3+1, but style 0 rebalances (libass `wrap_lines_smart`) to 2+2.
        let max = word_w * 3.0 + space_w * 2.0 + 0.5;
        let out = wrap_event_text("aa aa aa aa", 0, max, &[&font], 48.0, 0.0);
        assert_eq!(out, "aa aa\naa aa");
    }

    #[test]
    fn test_wrap_style_3_matches_style_0() {
        // libass runs `wrap_lines_smart` for every style except 1, so
        // 3 wraps exactly like 0 (balanced) — not bottom-wide greedy
        // (that is VSFilter behavior libass never implemented).
        let font = fallback_font();
        let word_w = TextShaper::measure_text("aa", &font, 48.0, 0.0);
        let space_w = TextShaper::measure_text(" ", &font, 48.0, 0.0);
        let max = word_w * 3.0 + space_w * 2.0 + 0.5;
        let smart = wrap_event_text("aa aa aa aa", 0, max, &[&font], 48.0, 0.0);
        assert_eq!(smart, "aa aa\naa aa");
        let three = wrap_event_text("aa aa aa aa", 3, max, &[&font], 48.0, 0.0);
        assert_eq!(three, smart);
        // Style 1 stays greedy (no rebalance).
        let greedy = wrap_event_text("aa aa aa aa", 1, max, &[&font], 48.0, 0.0);
        assert_eq!(greedy, "aa aa aa\naa");
    }

    #[test]
    fn test_wrap_preserves_tags_and_hard_breaks() {
        let font = fallback_font();
        let word_w = TextShaper::measure_text("aa", &font, 48.0, 0.0);
        let space_w = TextShaper::measure_text(" ", &font, 48.0, 0.0);
        let max = word_w + space_w + 0.5; // only one word fits per line
        let out = wrap_event_text("{\\c&H00FF00&}aa aa\\Naa", 0, max, &[&font], 48.0, 0.0);
        // Tag group survives, wrapping occurs, and the explicit break is kept
        assert_eq!(out, "{\\c&H00FF00&}aa\naa\naa");
    }

    #[test]
    fn test_wrap_no_spaces_unchanged() {
        let font = fallback_font();
        let out = wrap_event_text("aaaaaaaa", 0, 5.0, &[&font], 48.0, 0.0);
        assert_eq!(out, "aaaaaaaa");
    }

    #[test]
    fn test_wrap_cjk_breaks_without_spaces() {
        let font = fallback_font();
        // Six hiragana, room for ~two per line: must wrap with no
        // spaces inserted and no characters lost.
        let text = "あいうえおか";
        let two = TextShaper::measure_text("あい", &font, 48.0, 0.0);
        let out = wrap_event_text(text, 1, two + 0.5, &[&font], 48.0, 0.0);
        assert!(out.contains('\n'), "CJK run must wrap: {out:?}");
        assert!(!out.contains(' '), "no phantom spaces: {out:?}");
        assert_eq!(out.replace('\n', ""), text);
    }

    #[test]
    fn test_wrap_cjk_open_bracket_sticks() {
        let font = fallback_font();
        // No break after an opening bracket: "あ「あ" splits before
        // the bracket pair, never orphaning "「" at a line end.
        let pair = TextShaper::measure_text("「あ", &font, 48.0, 0.0);
        let out = wrap_event_text("あ「あ", 1, pair - 0.5, &[&font], 48.0, 0.0);
        assert_eq!(out, "あ\n「あ");
    }

    #[test]
    fn test_wrap_cjk_nbsp_glues() {
        let font = fallback_font();
        // U+00A0 (from \h) never breaks from its neighbors: the run
        // stays whole even when overlong.
        let out = wrap_event_text("あ\u{00A0}あ", 1, 5.0, &[&font], 48.0, 0.0);
        assert_eq!(out, "あ\u{00A0}あ");
    }

    #[test]
    fn test_wrap_zwsp_breaks() {
        let font = fallback_font();
        // U+200B ZERO WIDTH SPACE breaks on both sides (UAX #14 ZW):
        // "aa<ZWSP>aa" wraps at ZWSP width with the mark preserved.
        let two = TextShaper::measure_text("aa", &font, 48.0, 0.0);
        let out = wrap_event_text("aa\u{200B}aa", 1, two + 0.5, &[&font], 48.0, 0.0);
        assert!(out.contains('\n'), "ZWSP run must wrap: {out:?}");
        assert!(!out.contains(' '), "no phantom spaces: {out:?}");
        assert_eq!(out.replace('\n', ""), "aa\u{200B}aa");
    }

    #[test]
    fn test_wrap_ideographic_space_breaks() {
        let font = fallback_font();
        // U+3000 breaks around itself like other CJK (UAX #14 ID),
        // even between Latin halves (it is still a space).
        let one = TextShaper::measure_text("a", &font, 48.0, 0.0);
        let out = wrap_event_text("a\u{3000}b", 1, one + 0.5, &[&font], 48.0, 0.0);
        assert!(out.contains('\n'), "ideographic space must break: {out:?}");
        assert_eq!(out.replace('\n', ""), "a\u{3000}b");
    }

    #[test]
    fn test_zwsp_renders_nothing() {
        // ZERO WIDTH SPACE is invisible and zero-advance: shaping
        // "a<ZWSP>b" matches "ab" exactly (DejaVu carries an empty
        // ZWSP glyph, so no .notdef box appears).
        let base = render_text("ab", 1000);
        let zwsp = render_text("a\u{200B}b", 1000);
        assert_eq!(painted_cols(&base), painted_cols(&zwsp));
        assert_eq!(painted_rows(&base), painted_rows(&zwsp));
        assert_eq!(base.as_bytes(), zwsp.as_bytes());
    }

    #[test]
    fn test_wrap_karaoke_no_phantom_spaces() {
        let font = fallback_font();
        // Karaoke text split by tag groups: {\k80}Hel{\k60}lo {\k100}world!
        // "Hel" and "lo" are adjacent (no space) — must NOT get a space inserted.
        let text = "{\\k80}Hel{\\k60}lo {\\k100}world!";
        // Use a huge max_width so no wrapping occurs — we only test space preservation.
        let out = wrap_event_text(text, 0, f64::MAX, &[&font], 48.0, 0.0);
        assert_eq!(out, "{\\k80}Hel{\\k60}lo {\\k100}world!");
    }

    #[test]
    fn test_wrap_karaoke_with_real_spaces() {
        let font = fallback_font();
        // "line " has a trailing space before the next tag group.
        let text = "{\\k80}Out{\\k70}line {\\k80}dis{\\k60}ap{\\k70}pears";
        let out = wrap_event_text(text, 0, f64::MAX, &[&font], 48.0, 0.0);
        // The space between "line" and "dis" must be preserved; no space between "Out"|"line".
        assert_eq!(out, "{\\k80}Out{\\k70}line {\\k80}dis{\\k60}ap{\\k70}pears");
    }

    #[test]
    fn test_wrap_inline_tag_preserves_spaces() {
        let font = fallback_font();
        // Inline tags like {\b0} between words must not eat the space.
        let text = "{\\b1}Bold{\\b0} {\\i1}Italic{\\i0} {\\u1}Under{\\u0}";
        let out = wrap_event_text(text, 0, f64::MAX, &[&font], 48.0, 0.0);
        assert_eq!(out, "{\\b1}Bold{\\b0} {\\i1}Italic{\\i0} {\\u1}Under{\\u0}");
    }

    #[test]
    fn test_wrap_measures_drawings_geometrically() {
        let font = fallback_font();
        // A drawing following a tag group must not be forced onto its
        // own line: command letters ("m 0 0 l 100 ...") are not text,
        // so their advances must not size the word. The 100-unit rect
        // fits beside the prefix; no break may be inserted (a stray
        // break would also strand karaoke timing on the break piece).
        let text = "{\\kf100}{\\p1}m 0 0 l 100 0 l 100 40 l 0 40{\\p0}";
        let out = wrap_event_text(text, 0, 640.0, &[&font], 48.0, 0.0);
        assert_eq!(out, text);
        // A genuinely over-wide drawing still gets its own line.
        let wide = "{\\p1}m 0 0 l 2000 0 l 2000 40 l 0 40{\\p0}";
        let text = format!("Hi {wide}");
        let out = wrap_event_text(&text, 0, 640.0, &[&font], 48.0, 0.0);
        assert_eq!(out, format!("Hi\n{wide}"));
    }

    /// Build karaoke runs for `text` through the real layout pass
    /// (DejaVu at style size, PlayRes == video so layout units are
    /// device pixels). Returns runs, per-segment per-glyph assignment,
    /// and per-segment drawing assignment.
    fn karaoke_test_runs(text: &str) -> KaraokeBuild {
        let mut fm = FontManager::new();
        fm.load_font("DejaVu Sans", font::get_fallback_font(), false, false)
            .unwrap();
        let line = format!("Dialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,{text}");
        let event = Event::parse_from_line(&line).unwrap();
        let resolved = Compositor::resolve_style(&Style::new("Default"), &event);
        let segments = parse_text_segments(&event.text);
        let layout = Compositor::layout_segments(
            &segments,
            &event,
            &resolved,
            &fm,
            1000,
            0,
            5000,
            640,
            480,
            640,
            480,
            &[],
        );
        build_karaoke_runs(&segments, &layout.items)
    }

    /// `(start_ms, end_ms, kind, sweep)` per run, for compact assertions.
    fn run_windows(runs: &[KaraokeRun]) -> Vec<(u64, u64, KaraokeKind, bool)> {
        runs.iter()
            .map(|r| (r.start_ms, r.end_ms, r.kind, r.sweep))
            .collect()
    }

    #[test]
    fn test_shear_applies_to_rotated_text_at_render() {
        // Order-sensitive render test. libass `calc_transform_matrix`
        // shears the glyph-local outline FIRST (x1/y1 shear basis),
        // then rotates (frz → frx → fry); VSFilter `Transform_C`
        // matches. {\frz90}MMMM is a tall narrow column. Pre-rotation
        // \fax1 slants each glyph horizontally first, and the 90°
        // rotation turns that extra width into extra HEIGHT: the
        // column must grow TALLER with width roughly unchanged.
        // (temp_plan #8 claimed post-rotation shear; that claim was
        // wrong — verified against libass source — so this test pins
        // the reference pre-rotation order instead.)
        // Centered (\an5) so rotation cannot push ink off-screen and
        // shrink the box by clipping rather than geometry.
        let plain = render_text(r"{\an5\frz90}MMMM", 1000);
        let sheared = render_text(r"{\an5\fax1\frz90}MMMM", 1000);
        let (pw, ph) = ink_bbox(&plain).expect("rotated text must render");
        let (sw, sh) = ink_bbox(&sheared).expect("sheared text must render");
        assert!(
            sh > ph,
            "pre-rotation fax must lengthen the rotated column ({sh} <= {ph})"
        );
        assert!(
            sw.abs_diff(pw) < pw / 2,
            "pre-rotation fax must roughly preserve column width ({sw} vs {pw})"
        );
        assert_ne!(plain.as_bytes(), sheared.as_bytes());
        // Dual axis: pre-rotation \fay slants vertically, so after a
        // 90° rotation the column grows WIDER with height preserved.
        let fayed = render_text(r"{\an5\fay1\frz90}MMMM", 1000);
        let (fw, fh) = ink_bbox(&fayed).expect("fay text must render");
        assert!(
            fw > pw,
            "pre-rotation fay must widen the rotated column ({fw} <= {pw})"
        );
        assert!(
            fh.abs_diff(ph) < ph / 2,
            "pre-rotation fay must roughly preserve column height ({fh} vs {ph})"
        );
    }

    #[test]
    fn test_combined_fax_fay_frx_fry_frz_org_renders() {
        // Combined transform path (shear + all rotations + explicit
        // origin) must render deterministically and differ from the
        // shear-free variant.
        let a = render_text(r"{\fax0.5\fay-0.25\frx30\fry20\frz10\org(320,240)}Ag", 1000);
        let b = render_text(r"{\frx30\fry20\frz10\org(320,240)}Ag", 1000);
        assert!(ink_bbox(&a).is_some());
        assert!(ink_bbox(&b).is_some());
        assert_ne!(a.as_bytes(), b.as_bytes());
        // Extreme shear clamps instead of exploding or panicking.
        let c = render_text(r"{\fax99999\fay-99999\frz45}Ag", 1000);
        assert!(ink_bbox(&c).is_some());
    }

    #[test]
    fn test_multiline_centers_each_line() {
        // Centered alignment centers every line on its own width (libass
        // behavior): the short second line must not hug the long line's
        // left edge. 640x480 buffer, center x = 320.
        let buf = render_text(r"{\an5}MMMMMMMM\NMM", 1000);
        let bands = ink_bands(&buf);
        assert_eq!(bands.len(), 2, "two ink bands, got {bands:?}");
        for (i, &(x0, _, x1, _)) in bands.iter().enumerate() {
            let center = f64::from(x0 + x1) / 2.0;
            assert!(
                (center - 320.0).abs() <= 3.0,
                "line {i} centered at {center}, band {x0}..{x1}"
            );
        }
        // The short line is strictly inset from the long line's edges.
        assert!(bands[1].0 > bands[0].0 + 10, "bands {bands:?}");
        assert!(bands[1].2 < bands[0].2 - 10, "bands {bands:?}");
    }

    #[test]
    fn test_multiline_right_aligns_each_line() {
        // \an6: every line's right edge lands on the same x.
        let buf = render_text(r"{\an6}MMMMMMMM\NMM", 1000);
        let bands = ink_bands(&buf);
        assert_eq!(bands.len(), 2, "two ink bands, got {bands:?}");
        assert!(
            bands[0].2.abs_diff(bands[1].2) <= 2,
            "right edges align: {bands:?}"
        );
        assert!(bands[1].0 > bands[0].0 + 10, "bands {bands:?}");
    }

    #[test]
    fn test_multiline_left_aligns_each_line() {
        // \an4: every line's left edge lands on the same x.
        let buf = render_text(r"{\an4}MMMMMMMM\NMM", 1000);
        let bands = ink_bands(&buf);
        assert_eq!(bands.len(), 2, "two ink bands, got {bands:?}");
        assert!(
            bands[0].0.abs_diff(bands[1].0) <= 2,
            "left edges align: {bands:?}"
        );
    }

    #[test]
    fn test_fay_baseline_shear_slants_line() {
        // libass `apply_baseline_shear`: `\fay` shifts each glyph's
        // baseline cumulatively, so the line slants (later glyphs ride
        // lower for positive fay) instead of merely slanting each glyph
        // in place. The first glyph stays put; the line grows taller.
        let plain = render_text(r"{\an7\fay0}MMMM", 1000);
        let sheared = render_text(r"{\an7\fay0.5}MMMM", 1000);
        let (pw, ph) = ink_bbox(&plain).expect("plain renders");
        let (sw, sh) = ink_bbox(&sheared).expect("sheared renders");
        assert!(
            sh > ph + 10,
            "fay baseline shear must lengthen the line vertically ({sh} vs {ph})"
        );
        assert!(
            sw.abs_diff(pw) <= pw / 2,
            "fay baseline shear roughly preserves width ({sw} vs {pw})"
        );
        // Slant direction: the right half's ink centroid sits lower
        // (larger y) than the left half's for positive fay.
        let centroid = |buf: &RenderBuffer, x_lo: u32, x_hi: u32| -> f64 {
            let (mut sum, mut n) = (0u64, 0u64);
            for y in 0..buf.height {
                for x in x_lo..x_hi {
                    if buf.get_pixel(x, y)[3] > 0 {
                        sum += u64::from(y);
                        n += 1;
                    }
                }
            }
            sum as f64 / n.max(1) as f64
        };
        let bands = ink_bands(&sheared);
        assert_eq!(bands.len(), 1);
        let (x0, _, x1, _) = bands[0];
        let mid = (x0 + x1) / 2;
        let left = centroid(&sheared, x0, mid);
        let right = centroid(&sheared, mid, x1 + 1);
        assert!(
            right > left + 5.0,
            "positive fay must sink the right side ({right} vs {left})"
        );
    }

    #[test]
    fn test_fay_baseline_shear_resets_each_line() {
        // The cumulative shear restarts on every line: the second line
        // rides at the same height with or without `\fay`. Small fay so
        // the slanted first line cannot bridge the inter-line gap.
        let plain = render_text(r"{\an7}MM\NMM", 1000);
        let sheared = render_text(r"{\an7\fay0.15}MM\NMM", 1000);
        let pb = ink_bands(&plain);
        let sb = ink_bands(&sheared);
        assert_eq!(pb.len(), 2, "plain bands {pb:?}");
        assert_eq!(sb.len(), 2, "sheared bands {sb:?}");
        assert!(
            sb[1].1.abs_diff(pb[1].1) <= 2,
            "second line restarts unshifted: {sb:?} vs {pb:?}"
        );
        // ...while the first line slants (its band is taller).
        assert!(
            sb[0].3 - sb[0].1 > pb[0].3 - pb[0].1,
            "first line slants: {sb:?} vs {pb:?}"
        );
    }

    #[test]
    fn test_fay_baseline_shear_resets_at_karaoke_runs() {
        // libass `apply_baseline_shear` (default mode) restarts the
        // accumulator at every style run, including karaoke runs: with
        // two 2-glyph runs the line rises half as far as one 4-glyph
        // run. Top-anchored so tops align and bottoms differ.
        let continuous = render_text(r"{\an7\fay0.5}MMMM", 1000);
        let split = render_text(r"{\an7\fay0.5\k50}MM{\k50}MM", 1000);
        let (_, ch) = ink_bbox(&continuous).expect("renders");
        let (_, sh) = ink_bbox(&split).expect("renders");
        assert!(
            ch > sh + 10,
            "karaoke runs must restart baseline shear ({sh} vs {ch})"
        );
    }

    #[test]
    fn test_fay_baseline_shear_resets_at_style_change() {
        // Same rule for style-key changes: a mid-line color change
        // restarts the accumulator, while a same-value tag does not
        // (libass compares values, not tag presence).
        let same = render_text(r"{\an7\fay0.5}MM{\c&HFFFFFF&}MM", 1000);
        let changed = render_text(r"{\an7\fay0.5}MM{\c&H0000FF&}MM", 1000);
        let (_, sh) = ink_bbox(&same).expect("renders");
        let (_, ch) = ink_bbox(&changed).expect("renders");
        assert!(
            sh > ch + 10,
            "style change must restart baseline shear ({ch} vs {sh})"
        );
    }

    #[test]
    fn test_accumulate_fay_shear_guards() {
        // Pure helper: scaled advance accumulates; degenerate input is
        // ignored rather than poisoning the line.
        let mut acc = 0.0;
        accumulate_fay_shear(&mut acc, 0.5, 1.0, 1.0, 10.0);
        assert_eq!(acc, 5.0);
        accumulate_fay_shear(&mut acc, 0.5, 2.0, 1.0, 10.0);
        assert_eq!(acc, 7.5);
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            accumulate_fay_shear(&mut acc, bad, 1.0, 1.0, 10.0);
            accumulate_fay_shear(&mut acc, 0.5, bad, 1.0, 10.0);
            accumulate_fay_shear(&mut acc, 0.5, 1.0, bad, 10.0);
            accumulate_fay_shear(&mut acc, 0.5, 1.0, 1.0, bad);
            assert_eq!(acc, 7.5, "bad {bad} ignored");
        }
        accumulate_fay_shear(&mut acc, 0.5, 0.0, 1.0, 10.0);
        accumulate_fay_shear(&mut acc, 0.0, 1.0, 1.0, 10.0);
        assert_eq!(acc, 7.5);
    }

    #[test]
    fn test_frz_positive_runs_counterclockwise() {
        // Positive `\frz` rotates counterclockwise on screen (libass
        // `calc_transform_matrix`): the line's left end sinks while the
        // right end rises.
        let buf = render_text(r"{\an5\frz30}MMMM", 1000);
        let bands = ink_bands(&buf);
        assert_eq!(bands.len(), 1, "one slanted band: {bands:?}");
        let (x0, _, x1, _) = bands[0];
        assert!(x1 > x0 + 20, "wide enough to split: {bands:?}");
        // Left half's lowest ink vs right half's highest ink: with the
        // left end down and right end up, left-bottom exceeds right-top
        // by a clear margin.
        let mut left_bottom = 0u32;
        let mut right_top = buf.height;
        let mid = (x0 + x1) / 2;
        for y in 0..buf.height {
            for x in x0..mid {
                if buf.get_pixel(x, y)[3] > 0 {
                    left_bottom = left_bottom.max(y);
                }
            }
            for x in mid..=x1 {
                if buf.get_pixel(x, y)[3] > 0 {
                    right_top = right_top.min(y);
                }
            }
        }
        assert!(
            left_bottom > right_top + 10,
            "left end must sink below right end ({left_bottom} vs {right_top})"
        );
    }

    #[test]
    fn test_fe_resolve_and_reset() {
        let base = Style::new("Default");
        assert_eq!(base.encoding, 1);
        let event = Event::parse_from_line(
            "Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{\\fe128}A{\\r}B",
        )
        .unwrap();
        let resolved = Compositor::resolve_style(&base, &event);
        let segments = parse_text_segments(&event.text);
        let first =
            Compositor::resolve_segment_style(&resolved, &segments[0], &event, &[], 0, 0, 2000);
        assert_eq!(first.font_encoding, 128);
        // \r resets the encoding to the style default.
        let second =
            Compositor::resolve_segment_style(&resolved, &segments[1], &event, &[], 0, 0, 2000);
        assert_eq!(second.font_encoding, 1);
        // \fe renders (no charset remapping, but never breaks shaping).
        let buf = render_text("{\\fe128}Hi", 1000);
        assert!(ink_bbox(&buf).is_some());
    }

    #[test]
    fn test_kt_explicit_timing() {
        // Explicit absolute starts with a gap between syllables. Hard
        // runs pop at the run start (`end == start`).
        let (runs, glyph_run, _) = karaoke_test_runs("{\\kt0\\k50}a{\\kt200\\k50}b");
        assert_eq!(
            run_windows(&runs),
            vec![
                (0, 0, KaraokeKind::Hard, false),
                (2000, 2000, KaraokeKind::Hard, false),
            ]
        );
        assert_eq!(glyph_run, vec![vec![Some(0)], vec![Some(1)]]);
        // Order within a group follows the verified stacking rule
        // (c3 probe: earlier durations bank as skip before the current
        // window): {\k50\kt200} banks 500, then \kt assigns skip 2000
        // (wiping the banked duration) and resets, so A pops at 2000
        // and B (dur 500, no skip of its own) at 2000.
        let (runs, _, _) = karaoke_test_runs("{\\k50\\kt200}A{\\k50}B");
        assert_eq!(
            run_windows(&runs),
            vec![
                (2000, 2000, KaraokeKind::Hard, false),
                (2000, 2000, KaraokeKind::Hard, false),
            ]
        );
        // Render in the gap: first syllable sung, second pending.
        let buf = render_text("{\\kt0\\k50}a{\\kt200\\k50}b", 1000);
        let bytes = buf.as_bytes();
        assert!(bytes
            .chunks_exact(4)
            .any(|p| { p[0] > 200 && p[1] > 200 && p[2] > 200 && p[3] > 0 }));
        assert!(bytes
            .chunks_exact(4)
            .any(|p| { p[1] > 200 && p[0] < 50 && p[2] < 50 && p[3] > 0 }));
    }

    #[test]
    fn test_mixed_script_and_missing_glyphs_render() {
        // CJK + Latin on the fallback chain renders ink (DejaVu covers
        // Hiragana; anything missing degrades to .notdef, never a gap
        // in layout or a panic).
        let mixed = render_text("A\u{3042}\u{65E5}", 1000);
        assert!(ink_bbox(&mixed).is_some());
        let missing = render_text("A\u{10FFFF}B", 1000);
        assert!(ink_bbox(&missing).is_some());
        // Unknown families still render through manager fallback.
        let unknown = render_text("{\\fnNoSuchFontXYZ}A", 1000);
        assert!(ink_bbox(&unknown).is_some());
    }

    #[test]
    fn test_wrap_chain_matches_single_font_when_covered() {
        // Identical-coverage chains wrap exactly like a single font.
        let font = fallback_font();
        let single = wrap_event_text("aa aa aa aa", 0, 10.0, &[&font], 48.0, 0.0);
        let chained = wrap_event_text("aa aa aa aa", 0, 10.0, &[&font, &font], 48.0, 0.0);
        assert_eq!(single, chained);
    }

    fn opaque_box_style() -> Style {
        let mut s = Style::new("Box");
        s.border_style = 3;
        s.outline_color = Color::new(0, 255, 0, 0); // opaque red box
        s.back_color = Color::new(0, 0, 0, 255); // opaque blue: must NOT paint the box
        s.outline = 2.0;
        s.shadow = 0.0;
        s
    }

    fn count_color(buf: &RenderBuffer, want: [u8; 3]) -> usize {
        buf.as_bytes()
            .chunks_exact(4)
            .filter(|p| p[3] > 0 && p[0] == want[0] && p[1] == want[1] && p[2] == want[2])
            .count()
    }

    /// Bounding box of pixels exactly matching `want` (opaque), if any.
    fn color_bbox(buf: &RenderBuffer, want: [u8; 3]) -> Option<(u32, u32, u32, u32)> {
        let (mut x0, mut y0) = (u32::MAX, u32::MAX);
        let (mut x1, mut y1) = (0u32, 0u32);
        for y in 0..buf.height {
            for x in 0..buf.width {
                let p = buf.get_pixel(x, y);
                if p[3] > 0 && p[0] == want[0] && p[1] == want[1] && p[2] == want[2] {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
            }
        }
        if x0 > x1 {
            None
        } else {
            Some((x0, y0, x1 - x0 + 1, y1 - y0 + 1))
        }
    }

    #[test]
    fn test_opaque_box_ignores_margins() {
        // Position fixed by \pos: margins must not change the box.
        let mut a = opaque_box_style();
        a.margin_l = 0;
        a.margin_r = 0;
        a.margin_v = 0;
        let mut b = opaque_box_style();
        b.margin_l = 60;
        b.margin_r = 60;
        b.margin_v = 60;
        let ra = render_text_with_style(r"{\pos(320,400)}Hi", &a, 1000);
        let rb = render_text_with_style(r"{\pos(320,400)}Hi", &b, 1000);
        assert_eq!(ra.as_bytes(), rb.as_bytes());
    }

    #[test]
    fn test_opaque_box_padding_follows_outline() {
        let mut thin = opaque_box_style();
        thin.outline = 1.0;
        let mut thick = opaque_box_style();
        thick.outline = 8.0;
        let red = [255, 0, 0];
        let n_thin = count_color(
            &render_text_with_style(r"{\pos(320,400)}Hi", &thin, 1000),
            red,
        );
        let n_thick = count_color(
            &render_text_with_style(r"{\pos(320,400)}Hi", &thick, 1000),
            red,
        );
        assert!(n_thin > 0 && n_thick > n_thin);
    }

    #[test]
    fn test_opaque_box_uses_outline_colour() {
        // Reference behavior (VSFilter colors[2], libass outline
        // bitmap): the box fills with the OUTLINE color; BackColour
        // never paints the box.
        let mut transparent = opaque_box_style();
        transparent.outline_color = Color::new(255, 255, 0, 0); // fully transparent
        let buf = render_text_with_style(r"{\pos(320,400)}Hi", &transparent, 1000);
        assert_eq!(count_color(&buf, [255, 0, 0]), 0);
        // Opaque outline paints the box (white glyphs still on top),
        // opaque blue back stays out of the picture.
        let buf = render_text_with_style(r"{\pos(320,400)}Hi", &opaque_box_style(), 1000);
        assert!(count_color(&buf, [255, 0, 0]) > 0);
        assert!(count_color(&buf, [255, 255, 255]) > 0);
        assert_eq!(count_color(&buf, [0, 0, 255]), 0);
        // A \3c override recolors the box.
        let buf =
            render_text_with_style(r"{\pos(320,400)\3c&H00FF00&}Hi", &opaque_box_style(), 1000);
        assert!(count_color(&buf, [0, 255, 0]) > 0);
    }

    #[test]
    fn test_opaque_box_suppresses_glyph_outline() {
        let mut s1 = Style::new("Default");
        s1.outline_color = Color::new(0, 0, 255, 0); // opaque green
        s1.outline = 4.0;
        let mut s3 = opaque_box_style();
        s3.outline_color = Color::new(0, 0, 255, 0);
        s3.outline = 4.0;
        let green = [0, 255, 0];
        assert!(
            count_color(
                &render_text_with_style(r"{\pos(320,400)}Hi", &s1, 1000),
                green
            ) > 0
        );
        // Box mode: green fills the box rect (back blue nowhere).
        let boxed = render_text_with_style(r"{\pos(320,400)}Hi", &s3, 1000);
        assert!(count_color(&boxed, green) > 0);
        assert_eq!(count_color(&boxed, [0, 0, 255]), 0);
        assert!(count_color(&boxed, [255, 255, 255]) > 0);
    }

    #[test]
    fn test_opaque_box_covers_multiline() {
        let style = opaque_box_style();
        let red = [255, 0, 0];
        let one = render_text_with_style(r"{\pos(320,400)}A", &style, 1000);
        let two = render_text_with_style(r"{\pos(320,400)}A\NB", &style, 1000);
        let (_, _, _, h1) = color_bbox(&one, red).expect("single-line box");
        let (_, _, _, h2) = color_bbox(&two, red).expect("multiline box");
        assert!(h2 > h1 * 3 / 2, "box must cover both lines: {h1} vs {h2}");
    }

    /// Horizontal red span `(x0, x1)` on one buffer row, if any.
    fn red_span(buf: &RenderBuffer, y: u32) -> Option<(u32, u32)> {
        let (mut x0, mut x1) = (u32::MAX, 0u32);
        for x in 0..buf.width {
            let p = buf.get_pixel(x, y);
            if p[3] > 0 && p[0] == 255 && p[1] == 0 && p[2] == 0 {
                x0 = x0.min(x);
                x1 = x1.max(x);
            }
        }
        if x0 > x1 {
            None
        } else {
            Some((x0, x1))
        }
    }

    #[test]
    fn test_opaque_box_per_line_widths_left_aligned() {
        // libass draws one box per line: with \an7 a short first line
        // gets a narrow box (step silhouette), not the block width.
        let buf = render_text_with_style(
            r"{\an7\pos(20,20)}A\NLonger second line",
            &opaque_box_style(),
            1000,
        );
        let (_, by, _, bh) = color_bbox(&buf, [255, 0, 0]).expect("boxes");
        // Sample safely inside each line's band (away from shared pads).
        let top = red_span(&buf, by + 4).expect("top line box");
        let bottom = red_span(&buf, by + bh - 5).expect("bottom line box");
        assert_eq!(top.0, bottom.0, "left-aligned boxes share the left edge");
        assert!(
            top.1 + 20 < bottom.1,
            "short line box must be narrower: {top:?} vs {bottom:?}"
        );
    }

    #[test]
    fn test_opaque_box_per_line_boxes_center() {
        // Centered lines center their own boxes (libass aligns each
        // line independently inside the block).
        let buf = render_text_with_style(
            r"{\an5\pos(320,240)}A\NLonger second line",
            &opaque_box_style(),
            1000,
        );
        let (_, by, _, bh) = color_bbox(&buf, [255, 0, 0]).expect("boxes");
        let top = red_span(&buf, by + 4).expect("top line box");
        let bottom = red_span(&buf, by + bh - 5).expect("bottom line box");
        let (top_c, bottom_c) = (
            (top.0 + top.1) as f64 / 2.0,
            (bottom.0 + bottom.1) as f64 / 2.0,
        );
        assert!(
            (top_c - bottom_c).abs() <= 2.0,
            "boxes share the center: {top:?} vs {bottom:?}"
        );
        assert!(
            top.1 - top.0 + 20 < bottom.1 - bottom.0,
            "short line box must be narrower: {top:?} vs {bottom:?}"
        );
    }

    #[test]
    fn test_opaque_box_interior_empty_line_has_box() {
        // "A\N\NB": the empty middle line still gets a box (no gap in
        // the column); without it the lines' pads would leave a hole.
        let buf = render_text_with_style(r"{\an7\pos(20,20)}A\N\NB", &opaque_box_style(), 1000);
        let (_, by, _, bh) = color_bbox(&buf, [255, 0, 0]).expect("boxes");
        // Every row between the outer edges must carry red (adjacent
        // boxes overlap in their pads; a missing middle box leaves a
        // hole of line-height minus two pads).
        for y in by..by + bh {
            assert!(
                red_span(&buf, y).is_some(),
                "box column must have no gap at row {y}"
            );
        }
    }

    #[test]
    fn test_opaque_box_trailing_break_draws_nothing_extra() {
        // Extent-based like VSFilter: a trailing break adds no box. (The
        // whole block legitimately shifts: bottom alignment anchors the
        // taller block's bottom, as in libass. Compare geometry relative
        // to the glyph ink instead of bytes.)
        let style = opaque_box_style();
        let plain = render_text_with_style(r"{\pos(320,400)}Hi", &style, 1000);
        let trailing = render_text_with_style(r"{\pos(320,400)}Hi\N", &style, 1000);
        let white = [255, 255, 255];
        let (pr, tr) = (
            color_bbox(&plain, [255, 0, 0]).expect("plain box"),
            color_bbox(&trailing, [255, 0, 0]).expect("trailing box"),
        );
        let (pi, ti) = (
            color_bbox(&plain, white).expect("plain ink"),
            color_bbox(&trailing, white).expect("trailing ink"),
        );
        assert_eq!((pr.2, pr.3), (tr.2, tr.3), "same box size");
        assert_eq!((pi.2, pi.3), (ti.2, ti.3), "same ink size");
        assert_eq!(
            (pr.0 as i32 - pi.0 as i32, pr.1 as i32 - pi.1 as i32),
            (tr.0 as i32 - ti.0 as i32, tr.1 as i32 - ti.1 as i32),
            "same box position relative to ink"
        );
    }

    #[test]
    fn test_opaque_box_shadow() {
        // References shadow the padded box: shadow color appears offset
        // down-right of the box, and the box still paints over it.
        let mut style = opaque_box_style();
        style.shadow = 8.0;
        style.back_color = Color::new(0, 0, 255, 0); // opaque green shadow
        let buf = render_text_with_style(r"{\pos(320,400)}Hi", &style, 1000);
        let red = color_bbox(&buf, [255, 0, 0]).expect("box");
        let green = color_bbox(&buf, [0, 255, 0]).expect("box shadow");
        // Shadow extends past the box on the offset sides only.
        assert!(green.0 >= red.0 && green.1 >= red.1);
        assert!(green.0 + green.2 > red.0 + red.2);
        assert!(green.1 + green.3 > red.1 + red.3);
        // Zero shadow draws no shadow pixels.
        let plain = render_text_with_style(r"{\pos(320,400)}Hi", &opaque_box_style(), 1000);
        assert!(color_bbox(&plain, [0, 255, 0]).is_none());
    }

    #[test]
    fn test_opaque_box_drawing_line() {
        // A drawing line boxes its geometry (exercises the drawing row
        // path in the box layout).
        let buf = render_text_with_style(
            r"{\pos(320,400)}{\p1}m 0 0 l 60 0 l 60 30 l 0 30{\p0}",
            &opaque_box_style(),
            1000,
        );
        let (_, _, w, h) = color_bbox(&buf, [255, 0, 0]).expect("drawing box");
        assert!(w >= 60 && h >= 30, "box must cover the 60x30 rect");
    }

    #[test]
    fn test_scaled_outline_across_resolutions() {
        // Same PlayRes, three video sizes: with ScaledBorderAndShadow
        // the outline grows with resolution; without, it stays flat.
        // At 1:1 the flag is a no-op (identical bytes).
        let mut style = Style::new("Default");
        style.outline_color = Color::new(0, 0, 255, 0);
        style.outline = 6.0;
        style.shadow = 0.0;
        let green = [0, 255, 0];
        let at = |w: u32, h: u32, scaled: bool| {
            render_sized(r"{\pos(320,180)}H", &style, 1000, 640, 360, w, h, scaled)
        };
        assert_eq!(
            at(640, 360, true).as_bytes(),
            at(640, 360, false).as_bytes()
        );
        let (yes_lo, no_lo) = (
            count_color(&at(640, 360, true), green),
            count_color(&at(640, 360, false), green),
        );
        assert_eq!(yes_lo, no_lo);
        assert!(yes_lo > 0);
        let (yes_hi, no_hi) = (
            count_color(&at(1920, 1080, true), green),
            count_color(&at(1920, 1080, false), green),
        );
        // Scaled outline outgrows the unscaled one by a wide margin.
        assert!(
            yes_hi > no_hi * 2,
            "scaled outline must dominate at 3x: {yes_hi} vs {no_hi}"
        );
        // Middle resolution sits strictly between for the scaled flag.
        let yes_mid = count_color(&at(1280, 720, true), green);
        assert!(yes_mid > yes_lo && yes_mid < yes_hi);
    }

    #[test]
    fn test_scaled_box_padding_across_resolutions() {
        // Box padding honors the flag: box width scales exactly with
        // video size when scaled, and lags when not.
        let mut style = opaque_box_style();
        style.outline = 4.0;
        let red = [255, 0, 0];
        let width_at = |w: u32, h: u32, scaled: bool| {
            let buf = render_sized(r"{\pos(320,180)}Hi", &style, 1000, 640, 360, w, h, scaled);
            color_bbox(&buf, red).map(|b| b.2).unwrap_or(0)
        };
        let (y1, y3) = (width_at(640, 360, true), width_at(1920, 1080, true));
        assert!(
            (y3 as i32 - 3 * y1 as i32).abs() <= 3,
            "scaled box must triple: {y1} -> {y3}"
        );
        let (n1, n3) = (width_at(640, 360, false), width_at(1920, 1080, false));
        assert!(n3 < 3 * n1 - 5, "unscaled box must lag: {n1} -> {n3}");
    }

    #[test]
    fn test_blur_is_flag_and_resolution_independent() {
        // Locked current behavior: \blur radius is in video pixels and
        // ignores ScaledBorderAndShadow (reference scaling of blur is
        // unverified — see CONFORMANCE.md). Outline/shadow are zeroed
        // so the flag has no other render effect to compare through:
        // outlines DO scale with the flag (see scaled-outline tests),
        // so a full-frame comparison with outlines would differ by design.
        let mut style = Style::new("Default");
        style.outline = 0.0;
        style.shadow = 0.0;
        let a = render_sized(
            r"{\pos(320,180)\blur5}H",
            &style,
            1000,
            640,
            360,
            1920,
            1080,
            true,
        );
        let b = render_sized(
            r"{\pos(320,180)\blur5}H",
            &style,
            1000,
            640,
            360,
            1920,
            1080,
            false,
        );
        assert_eq!(a.as_bytes(), b.as_bytes());
        let sharp = render_sized(
            r"{\pos(320,180)}H",
            &style,
            1000,
            640,
            360,
            1920,
            1080,
            true,
        );
        assert_ne!(a.as_bytes(), sharp.as_bytes());
    }

    #[test]
    fn test_scaled_shadow_offset_across_resolutions() {
        // Shadow reach (ink width with shadow minus without) triples
        // with the flag and stays flat without it.
        let mut shadowed = Style::new("Default");
        shadowed.shadow = 12.0;
        shadowed.outline = 0.0;
        let mut plain = Style::new("Default");
        plain.shadow = 0.0;
        plain.outline = 0.0;
        let ink_w = |style: &Style, w: u32, h: u32, scaled: bool| {
            ink_bbox(&render_sized(
                r"{\pos(100,100)}H",
                style,
                1000,
                640,
                360,
                w,
                h,
                scaled,
            ))
            .map(|b| b.0)
            .unwrap_or(0)
        };
        let reach = |w: u32, h: u32, scaled: bool| {
            ink_w(&shadowed, w, h, scaled).saturating_sub(ink_w(&plain, w, h, scaled))
        };
        let (y1, y3) = (reach(640, 360, true), reach(1920, 1080, true));
        assert!(y1 > 0, "shadow must extend ink");
        assert!(
            (y3 as i32 - 3 * y1 as i32).abs() <= 4,
            "scaled shadow reach must triple: {y1} -> {y3}"
        );
        let (n1, n3) = (reach(640, 360, false), reach(1920, 1080, false));
        assert!(
            (n3 as i32 - n1 as i32).abs() <= 4,
            "unscaled shadow reach must stay flat: {n1} -> {n3}"
        );
    }

    #[test]
    fn test_degenerate_font_size_resets_to_style_safely() {
        // libass: \fs0 and sizes computing to <= 0 reset to the event
        // style size (rendering normally), instead of panicking,
        // rendering nothing, or producing garbage.
        let plain = render_text("Hi", 1000);
        assert_eq!(
            render_text("{\\fs0}Hi", 1000).as_bytes(),
            plain.as_bytes(),
            "\\fs0 must reset to style size"
        );
        assert_eq!(
            render_text("{\\fs-10}Hi", 1000).as_bytes(),
            plain.as_bytes(),
            "relative size computing to 0 must reset to style size"
        );
        // Relative sizes that stay positive scale the current size.
        let half = ink_bbox(&render_text("{\\fs-5}Hi", 1000));
        let normal = ink_bbox(&plain);
        assert!(half.is_some() && normal.is_some());
        assert!(
            half.unwrap().0 < normal.unwrap().0,
            "\\fs-5 must render smaller than the style size"
        );
        assert!(ink_bbox(&render_text("{\\fs+10}Hi", 1000)).is_some());
    }

    #[test]
    fn test_bold_weight_resolve_and_render() {
        // Resolve preserves explicit weights.
        let base = Style::new("Default");
        for (text, want) in [("{\\b0}B", 400), ("{\\b1}B", 700), ("{\\b900}B", 900)] {
            let event = Event::parse_from_line(&format!(
                "Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{text}"
            ))
            .unwrap();
            let segments = parse_text_segments(&event.text);
            let resolved = Compositor::resolve_style(&base, &event);
            let seg =
                Compositor::resolve_segment_style(&resolved, &segments[0], &event, &[], 0, 0, 2000);
            assert_eq!(seg.font_weight, want, "{text:?}");
        }
        // Render: \b1 and \b700 take the identical faux-bold path on a
        // regular-only face; both differ from \b0.
        let normal = render_text("{\\b0}Bold?", 1000);
        let one = render_text("{\\b1}Bold?", 1000);
        let seven = render_text("{\\b700}Bold?", 1000);
        let nine = render_text("{\\b900}Bold?", 1000);
        assert_eq!(one.as_bytes(), seven.as_bytes());
        assert_eq!(seven.as_bytes(), nine.as_bytes());
        assert_ne!(normal.as_bytes(), seven.as_bytes());
    }

    #[test]
    fn test_karaoke_runs_hard_tags() {
        // Each nonzero \k starts a run; hard runs pop at their start.
        let (runs, glyph_run, _) = karaoke_test_runs("{\\k50}A{\\k30}B");
        assert_eq!(
            run_windows(&runs),
            vec![
                (0, 0, KaraokeKind::Hard, false),
                (500, 500, KaraokeKind::Hard, false),
            ]
        );
        assert_eq!(glyph_run, vec![vec![Some(0)], vec![Some(1)]]);
        assert!(runs[0].span > 0.0);
        assert!(runs[1].span > 0.0);
    }

    #[test]
    fn test_karaoke_runs_leading_text_has_no_run() {
        // Text before any karaoke tag renders normally (no run).
        let (runs, glyph_run, _) = karaoke_test_runs("pre{\\kf40}X");
        assert_eq!(run_windows(&runs), vec![(0, 400, KaraokeKind::Sweep, true)]);
        assert_eq!(glyph_run[0], vec![None, None, None]);
        assert_eq!(glyph_run[1], vec![Some(0)]);
    }

    #[test]
    fn test_karaoke_runs_break_at_style_change() {
        // Mid-syllable style change splits the run (midstyle probe:
        // `{\kf100}a{\b1}b` sweeps "a" while "b" pops at the window
        // end). Same for \k: "B" pops when A's window ends.
        let (runs, glyph_run, _) = karaoke_test_runs("{\\k50}A{\\c&H00FF00&}B");
        assert_eq!(
            run_windows(&runs),
            vec![
                (0, 0, KaraokeKind::Hard, false),
                (500, 500, KaraokeKind::Hard, false),
            ]
        );
        assert_eq!(glyph_run, vec![vec![Some(0)], vec![Some(1)]]);
        let (runs, glyph_run, _) = karaoke_test_runs("{\\kf100}a{\\b1}b");
        assert_eq!(
            run_windows(&runs),
            vec![
                (0, 1000, KaraokeKind::Sweep, true),
                (1000, 1000, KaraokeKind::Sweep, false),
            ]
        );
        assert_eq!(glyph_run, vec![vec![Some(0)], vec![Some(1)]]);
        // Render check mirroring the probe: at 500ms "a" is split and
        // "b" is all secondary; at 1500ms both are primary.
        let mid = render_text("{\\kf100}a{\\b1}b", 500);
        let is_primary = |p: &[u8]| p[0] > 200 && p[1] > 200 && p[2] > 200 && p[3] > 0;
        let is_secondary = |p: &[u8]| p[1] > 200 && p[0] < 50 && p[2] < 50 && p[3] > 0;
        assert!(mid.as_bytes().chunks_exact(4).any(is_primary));
        assert!(mid.as_bytes().chunks_exact(4).any(is_secondary));
        let done = render_text("{\\kf100}a{\\b1}b", 1500);
        assert!(done.as_bytes().chunks_exact(4).any(is_primary));
        assert!(!done.as_bytes().chunks_exact(4).any(is_secondary));
    }

    #[test]
    fn test_karaoke_runs_noop_reset_joins_run() {
        // A `\r` that changes nothing breaks no run: libass
        // `split_style_runs` compares per-glyph style snapshots, and
        // `ass_reset_render_context` never touches the effect fields,
        // so `{\k100}a{\r}b` keeps one run (probe: `{\k100}a{\r}b`
        // shows no secondary pixel at 50ms — "b" sings with "a").
        let (runs, glyph_run, _) = karaoke_test_runs("{\\k100}a{\\r}b");
        assert_eq!(run_windows(&runs), vec![(0, 0, KaraokeKind::Hard, false)]);
        assert_eq!(glyph_run, vec![vec![Some(0)], vec![Some(0)]]);
        // Render check mirroring the probe: no secondary anywhere.
        let early = render_text("{\\k100}a{\\r}b", 50);
        let is_secondary = |p: &[u8]| p[1] > 200 && p[0] < 50 && p[2] < 50 && p[3] > 0;
        assert!(!early.as_bytes().chunks_exact(4).any(is_secondary));
        // But a `\r` that DOES change style still splits the run.
        let (runs, glyph_run, _) = karaoke_test_runs("{\\b1\\k100}a{\\r}b");
        assert_eq!(
            run_windows(&runs),
            vec![
                (0, 0, KaraokeKind::Hard, false),
                (1000, 1000, KaraokeKind::Hard, false),
            ]
        );
        assert_eq!(glyph_run, vec![vec![Some(0)], vec![Some(1)]]);
    }

    #[test]
    fn test_karaoke_runs_k0_joins_run() {
        // \k0 adds no duration and breaks nothing: "b" joins A's run
        // (corners probe: `{\k100}a{\k0}b{\k100}c` shows a,b primary
        // and c secondary at 500ms).
        let (runs, glyph_run, _) = karaoke_test_runs("{\\k100}a{\\k0}b{\\k100}c");
        assert_eq!(
            run_windows(&runs),
            vec![
                (0, 0, KaraokeKind::Hard, false),
                (1000, 1000, KaraokeKind::Hard, false),
            ]
        );
        assert_eq!(glyph_run, vec![vec![Some(0)], vec![Some(0)], vec![Some(1)]]);
    }

    #[test]
    fn test_karaoke_runs_stacked_tags_accumulate_skip() {
        // Stacked tags bank dead time before the window (c3 probe:
        // `{\k100\k0}a` is still secondary at 500ms).
        let (runs, glyph_run, _) = karaoke_test_runs("{\\k100\\k0}a");
        assert_eq!(
            run_windows(&runs),
            vec![(1000, 1000, KaraokeKind::Hard, false)]
        );
        assert_eq!(glyph_run, vec![vec![Some(0)]]);
        // \kt resets the clock and skips: `{\k100}a{\kt50}b{\k100}c`
        // puts c at 500 (c3 probe: all primary at 500ms).
        let (runs, glyph_run, _) = karaoke_test_runs("{\\k100}a{\\kt50}b{\\k100}c");
        assert_eq!(
            run_windows(&runs),
            vec![
                (0, 0, KaraokeKind::Hard, false),
                (500, 500, KaraokeKind::Hard, false),
            ]
        );
        assert_eq!(glyph_run, vec![vec![Some(0)], vec![Some(0)], vec![Some(1)]]);
    }

    #[test]
    fn test_karaoke_runs_newline_splits_and_pops() {
        // \N splits the sweep run; the second line consumes timing
        // invisibly, then pops at the window end (corners probe: "bb"
        // all secondary at 500ms, all primary at 1500ms).
        let (runs, glyph_run, _) = karaoke_test_runs("{\\kf100}aa\\Nbb");
        assert_eq!(
            run_windows(&runs),
            vec![
                (0, 1000, KaraokeKind::Sweep, true),
                (1000, 1000, KaraokeKind::Sweep, false),
            ]
        );
        assert_eq!(glyph_run.len(), 1);
        assert_eq!(glyph_run[0], vec![Some(0), Some(0), Some(1), Some(1)]);
    }

    #[test]
    fn test_karaoke_runs_sweep_span_trims_spaces() {
        // Sweep spans exclude leading/trailing ASCII spaces (libass
        // visible-span rule), so the edge never starts/ends in a gap.
        let (runs, _, _) = karaoke_test_runs("{\\kf100}a");
        let bare = runs[0].span;
        assert!(bare > 0.0);
        let (runs, _, _) = karaoke_test_runs("{\\kf100} a ");
        assert!((runs[0].span - bare).abs() < 1e-6);
    }

    #[test]
    fn test_karaoke_runs_frz_flip() {
        // \frz in (90, 270) mirrors the sweep (frzflip probe); the
        // bounds themselves do not flip.
        let (runs, _, _) = karaoke_test_runs("{\\frz200\\kf100}ab");
        assert!(runs[0].flip);
        let (runs, _, _) = karaoke_test_runs("{\\frz10\\kf100}ab");
        assert!(!runs[0].flip);
        let (runs, _, _) = karaoke_test_runs("{\\frz90\\kf100}ab");
        assert!(!runs[0].flip);
        let (runs, _, _) = karaoke_test_runs("{\\frz270\\kf100}ab");
        assert!(!runs[0].flip);
    }

    #[test]
    fn test_karaoke_outline_kind() {
        let (runs, glyph_run, _) = karaoke_test_runs("{\\ko20}A");
        assert_eq!(
            run_windows(&runs),
            vec![(0, 0, KaraokeKind::Outline, false)]
        );
        assert_eq!(glyph_run, vec![vec![Some(0)]]);
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
        // \k: secondary before the run pops, primary from the exact pop
        // instant (the run start for hard runs). Second run pops at
        // 1000ms: render checks both sides of the boundary.
        let (runs, glyph_run, _) = karaoke_test_runs("{\\k100}A{\\k100}B");
        assert_eq!(
            run_windows(&runs),
            vec![
                (0, 0, KaraokeKind::Hard, false),
                (1000, 1000, KaraokeKind::Hard, false),
            ]
        );
        assert_eq!(glyph_run, vec![vec![Some(0)], vec![Some(1)]]);
        let is_secondary = |p: &[u8]| p[1] > 200 && p[0] < 50 && p[2] < 50 && p[3] > 0;
        // Just before the pop: B is still secondary.
        let before = render_text("{\\k100}A{\\k100}B", 999);
        assert!(before.as_bytes().chunks_exact(4).any(is_secondary));
        // From the pop instant: no secondary remains.
        for t in [1000, 1500] {
            let after = render_text("{\\k100}A{\\k100}B", t);
            assert!(!after.as_bytes().chunks_exact(4).any(is_secondary));
        }
    }

    #[test]
    fn test_karaoke_outline_suppressed_before_start() {
        // \ko: outline hidden while elapsed < start; visible from the
        // exact start instant (reference ASS behavior).
        assert!(karaoke_outline_suppressed(999, 1000));
        assert!(!karaoke_outline_suppressed(1000, 1000));
        assert!(!karaoke_outline_suppressed(1500, 1000));
        assert!(!karaoke_outline_suppressed(0, 0));
    }

    #[test]
    fn test_ko_multi_syllable_timing() {
        // Two \ko runs pop at 0 and 500ms. Each run is independent:
        // outline suppressed before its own start, visible from it.
        let (runs, glyph_run, _) = karaoke_test_runs("{\\ko50}A{\\ko50}B");
        assert_eq!(
            run_windows(&runs),
            vec![
                (0, 0, KaraokeKind::Outline, false),
                (500, 500, KaraokeKind::Outline, false),
            ]
        );
        assert_eq!(glyph_run, vec![vec![Some(0)], vec![Some(1)]]);
        let (s0, s1) = (&runs[0], &runs[1]);

        // Run 0 (start 0): visible at/after 0.
        assert!(!karaoke_outline_suppressed(0, s0.start_ms));
        // Run 1 across its boundaries: start-1, start, middle,
        // exact end, after end.
        assert!(karaoke_outline_suppressed(499, s1.start_ms));
        assert!(!karaoke_outline_suppressed(500, s1.start_ms));
        assert!(!karaoke_outline_suppressed(750, s1.start_ms));
        assert!(!karaoke_outline_suppressed(1000, s1.start_ms));
        assert!(!karaoke_outline_suppressed(1500, s1.start_ms));
        // Fill pops with the run: secondary before, primary from it.
        let is_secondary = |p: &[u8]| p[1] > 200 && p[0] < 50 && p[2] < 50 && p[3] > 0;
        let before = render_text("{\\ko50}A{\\ko50}B", 499);
        assert!(before.as_bytes().chunks_exact(4).any(is_secondary));
        let after = render_text("{\\ko50}A{\\ko50}B", 500);
        assert!(!after.as_bytes().chunks_exact(4).any(is_secondary));
    }

    #[test]
    fn test_ko_render_fill_and_outline_transition() {
        // \ko render: before a syllable starts it shows the secondary
        // fill with no outline; from its start, primary fill + outline.
        let mut comp = Compositor::new();
        let mut fm = FontManager::new();
        fm.load_font("DejaVu Sans", font::get_fallback_font(), false, false)
            .unwrap();

        let event = Event::parse_from_line(
            "Dialogue: 0,0:00:00.00,0:00:03.00,Default,,0,0,0,,{\\ko100}A{\\ko100}B",
        )
        .unwrap();
        let style = Style::new("Default");
        let resolved = Compositor::resolve_style(&style, &event);
        let render_at = |comp: &mut Compositor, time_ms: u64| {
            let mut buf = RenderBuffer::new(320, 100).unwrap();
            comp.composite_event(
                &mut buf,
                &event,
                &resolved,
                &fm,
                time_ms,
                320,
                100,
                320,
                100,
                0,
                &[],
            );
            buf
        };
        let has_secondary = |buf: &RenderBuffer| {
            buf.as_bytes()
                .chunks_exact(4)
                .any(|p| p[1] > 200 && p[0] < 50 && p[2] < 50 && p[3] > 0)
        };
        let has_primary = |buf: &RenderBuffer| {
            buf.as_bytes()
                .chunks_exact(4)
                .any(|p| p[0] > 200 && p[1] > 200 && p[2] > 200 && p[3] > 0)
        };
        // Dark (outline-black) pixel count: grows when outlines appear.
        let dark_count = |buf: &RenderBuffer| {
            buf.as_bytes()
                .chunks_exact(4)
                .filter(|p| p[3] > 0 && p[0] < 60 && p[1] < 60 && p[2] < 60)
                .count()
        };

        // At 100ms: syllable 0 sung (primary + outline), syllable 1
        // pending (secondary, no outline).
        let early = render_at(&mut comp, 100);
        assert!(has_primary(&early));
        assert!(has_secondary(&early));

        // At 2500ms both sung: primary only, and strictly more outline
        // pixels than while the second syllable was pending.
        let late = render_at(&mut comp, 2500);
        assert!(has_primary(&late));
        assert!(!has_secondary(&late));
        assert!(
            dark_count(&late) > dark_count(&early),
            "outline must appear once the pending \\ko syllable starts"
        );
    }

    #[test]
    fn test_karaoke_runs_use_layout_spans() {
        // Run spans come from the layout pass (shaped advances), so
        // inline \fs/\fn/\fscx/\fsp changes measure exactly what
        // rendering consumes: a doubled size doubles the span.
        let (runs, _, _) = karaoke_test_runs("{\\kf100}MM");
        let base = runs[0].span;
        assert!(base > 0.0);
        let (runs, _, _) = karaoke_test_runs("{\\kf100\\fscx200}MM");
        assert!((runs[0].span - 2.0 * base).abs() < 1e-6);
        // Tag-only text builds no runs (nothing to time). Event
        // text is trimmed, so a lone trailing space never reaches the
        // builder either.
        let (runs, glyph_run, _) = karaoke_test_runs("{\\kf100}");
        assert!(runs.is_empty());
        assert!(glyph_run.iter().all(|v| v.iter().all(|g| g.is_none())));
        let (runs, _, _) = karaoke_test_runs("{\\kf100} ");
        assert!(runs.is_empty());
        // Interior trailing space trims from the span (the space
        // before {\b1} survives event trimming).
        let (runs, _, _) = karaoke_test_runs("{\\kf100}MM {\\b1}b");
        assert!((runs[0].span - base).abs() < 1e-6);
    }

    #[test]
    fn test_kf_drawing_splits_at_midpoint() {
        // Drawings sweep exactly like text (c2 probe): at the window
        // midpoint the rect's left half is primary, right secondary,
        // with a hard edge between adjacent columns.
        let text = "{\\kf100}{\\p1}m 0 0 l 100 0 l 100 40 l 0 40{\\p0}";
        let (runs, _, drawing_run) = karaoke_test_runs(text);
        assert_eq!(
            run_windows(&runs),
            vec![(0, 1000, KaraokeKind::Sweep, true)]
        );
        assert!(drawing_run.contains(&Some(0)));
        let is_primary = |p: &[u8]| p[0] > 200 && p[1] > 200 && p[2] > 200 && p[3] > 0;
        let is_secondary = |p: &[u8]| p[1] > 200 && p[0] < 50 && p[2] < 50 && p[3] > 0;
        let mid = render_text(text, 500);
        let bytes = mid.as_bytes();
        assert!(bytes.chunks_exact(4).any(is_primary));
        assert!(bytes.chunks_exact(4).any(is_secondary));
        // Finished sweep: primary only.
        let done = render_text(text, 1500);
        assert!(done.as_bytes().chunks_exact(4).any(is_primary));
        assert!(!done.as_bytes().chunks_exact(4).any(is_secondary));
    }

    #[test]
    fn test_kf_sweep_splits_within_glyph() {
        // \kf mid-sweep: one wide glyph must contain BOTH primary and
        // secondary pixels (a true within-glyph sweep, not whole-glyph
        // switching). Before start: all secondary; after: all primary.
        let mut comp = Compositor::new();
        let mut fm = FontManager::new();
        fm.load_font("DejaVu Sans", font::get_fallback_font(), false, false)
            .unwrap();

        let event =
            Event::parse_from_line("Dialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,{\\kf200}W")
                .unwrap();
        let style = Style::new("Default");
        let resolved = Compositor::resolve_style(&style, &event);
        let render_at = |comp: &mut Compositor, time_ms: u64| {
            let mut buf = RenderBuffer::new(320, 100).unwrap();
            comp.composite_event(
                &mut buf,
                &event,
                &resolved,
                &fm,
                time_ms,
                320,
                100,
                320,
                100,
                0,
                &[],
            );
            buf
        };
        let is_secondary = |p: &[u8]| p[1] > 200 && p[0] < 50 && p[2] < 50 && p[3] > 0;
        let is_primary = |p: &[u8]| p[0] > 200 && p[1] > 200 && p[2] > 200 && p[3] > 0;

        // Mid-sweep (1000ms of 2000ms): both colors inside one glyph.
        let mid = render_at(&mut comp, 1000);
        let mid_bytes = mid.as_bytes();
        assert!(
            mid_bytes.chunks_exact(4).any(is_primary),
            "mid-sweep glyph must contain primary pixels"
        );
        assert!(
            mid_bytes.chunks_exact(4).any(is_secondary),
            "mid-sweep glyph must contain secondary pixels"
        );

        // Before start the syllable is entirely secondary.
        let event2 =
            Event::parse_from_line("Dialogue: 0,0:00:02.00,0:00:05.00,Default,,0,0,0,,{\\kf200}W")
                .unwrap();
        let resolved2 = Compositor::resolve_style(&style, &event2);
        let mut before = RenderBuffer::new(320, 100).unwrap();
        comp.composite_event(
            &mut before,
            &event2,
            &resolved2,
            &fm,
            2050,
            320,
            100,
            320,
            100,
            0,
            &[],
        );
        // 50ms into a 2000ms sweep: left sliver primary, rest secondary.
        assert!(before.as_bytes().chunks_exact(4).any(is_secondary));

        // After completion the syllable is entirely primary.
        let done = render_at(&mut comp, 2500);
        assert!(done.as_bytes().chunks_exact(4).any(is_primary));
        assert!(!done.as_bytes().chunks_exact(4).any(is_secondary));
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
