use ab_glyph::{Font, FontArc, GlyphId, PxScale, ScaleFont};
use encoding_rs::{
    Encoding, BIG5, EUC_KR, GBK, ISO_8859_2, ISO_8859_7, ISO_8859_8, SHIFT_JIS, WINDOWS_1251,
    WINDOWS_1252, WINDOWS_1254, WINDOWS_1256, WINDOWS_1257, WINDOWS_1258, WINDOWS_874,
};
use rustybuzz::{BufferClusterLevel, Direction, UnicodeBuffer};
use unicode_bidi::BidiInfo;

use crate::types::Color;

/// A glyph ready for rasterization
#[derive(Debug, Clone)]
pub struct ShapedGlyph {
    pub glyph_id: GlyphId,
    /// Stable [`FontManager`](super::font::FontManager) id of the face
    /// that provided this glyph (primary or fallback).
    pub font_id: usize,
    /// Source scalar (for whitespace-trimmed karaoke spans; libass
    /// excludes trimmed leading/trailing whitespace from sweep spans).
    pub ch: char,
    pub x: f64,
    pub y: f64,
    pub advance: f64,
    pub font_size: f64,
    pub color: Color,
    pub outline_color: Color,
    pub shadow_color: Color,
    pub font_weight: u16,
    pub italic: bool,
    pub scale_x: f64,
    pub scale_y: f64,
    pub rotation: f64,
}

/// A line of shaped text
#[derive(Debug, Clone)]
pub struct ShapedLine {
    pub glyphs: Vec<ShapedGlyph>,
    pub width: f64,
    pub height: f64,
    pub baseline: f64,
    /// Per-row height (primary face line height, scaled): every shaper
    /// row advances `y` by this, so row `k` tops at `k * line_height`
    /// even across empty rows (which emit no glyphs). Zero when empty.
    pub line_height: f64,
    /// Characters missing from every shaping font (rendered as the
    /// primary face's .notdef). Zero when the chain covers the text.
    pub missing_glyphs: u32,
}

/// A raster face paired with the original sfnt bytes used by rustybuzz.
/// `FontArc` remains the rasterizer source of truth; the byte slice and
/// collection index are only used for OpenType substitutions/positioning.
#[derive(Clone, Copy)]
pub struct ShapingFont<'a> {
    pub id: usize,
    pub raster: &'a FontArc,
    pub data: &'a [u8],
    pub face_index: u32,
}

/// Map the numeric ASS/SSA `Encoding`/`\fe` value to the corresponding
/// Windows code page.  Subtitle text arrives here as Rust Unicode, so the
/// only safe compatibility bridge is to reinterpret legacy single-byte
/// values (`U+0000..U+00FF`) while leaving already-Unicode scripts intact.
/// Multibyte legacy characters must already have been decoded by the caller;
/// this bridge still handles their ASCII-compatible portions correctly.
fn ass_encoding(value: i32) -> Option<&'static Encoding> {
    match value {
        0 | 1 | 77 => Some(WINDOWS_1252),
        128 => Some(SHIFT_JIS),
        129 => Some(EUC_KR),
        130 => Some(WINDOWS_1252), // Johab is not in encoding_rs.
        134 => Some(GBK),
        136 => Some(BIG5),
        161 => Some(ISO_8859_7),
        162 => Some(WINDOWS_1254),
        163 => Some(WINDOWS_1258),
        177 => Some(ISO_8859_8),
        178 => Some(WINDOWS_1256),
        186 => Some(WINDOWS_1257),
        204 => Some(WINDOWS_1251),
        222 => Some(WINDOWS_874),
        238 => Some(ISO_8859_2),
        _ => None,
    }
}

/// True for combining marks (Mn/Mc) of the common scripts — Latin,
/// Greek, Cyrillic, Hebrew, Arabic, Syriac, Thaana, Devanagari, Thai,
/// Lao, Tibetan, CJK kana voicing marks, and the generic blocks.
/// Marks attach to the preceding base for cluster-aware fallback and
/// never start a wrapped line. Glue only (mark reordering itself
/// needs a real shaper); remaining Indic scripts and historic marks
/// stay per-character until UCD tables land.
pub fn is_combining_mark(ch: char) -> bool {
    matches!(
        ch as u32,
        0x0300..=0x036F
            | 0x0483..=0x0489
            | 0x0591..=0x05BD
            | 0x05BF
            | 0x05C1..=0x05C2
            | 0x05C4..=0x05C5
            | 0x05C7
            | 0x0610..=0x061A
            | 0x064B..=0x065F
            | 0x0670
            | 0x06D6..=0x06DC
            | 0x06DF..=0x06E4
            | 0x06E7..=0x06E8
            | 0x06EA..=0x06ED
            | 0x0711
            | 0x0730..=0x074A
            | 0x07A6..=0x07B0
            | 0x0900..=0x0903
            | 0x093A..=0x093C
            | 0x093E..=0x094D
            | 0x094E..=0x094F
            | 0x0951..=0x0957
            | 0x0962..=0x0963
            | 0x0E31
            | 0x0E34..=0x0E3A
            | 0x0E47..=0x0E4E
            | 0x0EB1
            | 0x0EB4..=0x0EBC
            | 0x0EC8..=0x0ECD
            | 0x0F71..=0x0F84
            | 0x0F86..=0x0F87
            | 0x1AB0..=0x1AFF
            | 0x1DC0..=0x1DFF
            | 0x20D0..=0x20FF
            | 0xFE20..=0xFE2F
            | 0x3099..=0x309A
    )
}

/// Resolve the fallback pick for every scalar in `text`: a base char
/// plus its following combining marks prefer the first face containing
/// the whole cluster (marks stay on the base's face instead of being
/// stolen by an earlier mark-only — or split from a base-only — face);
/// when no face holds the cluster, each char falls back independently.
/// Breaks resolve to 0 (callers skip them). Deterministic: shaping and
/// measurement share this, so widths always match.
pub fn cluster_font_picks<F>(text: &str, font_count: usize, mut has_glyph: F) -> Vec<usize>
where
    F: FnMut(usize, char) -> bool,
{
    let chars: Vec<char> = text.chars().collect();
    let mut picks = vec![0usize; chars.len()];
    let first_with =
        |ch: char, has_glyph: &mut F| (0..font_count).find(|&f| has_glyph(f, ch)).unwrap_or(0);
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\n' || ch == '\r' {
            i += 1;
            continue;
        }
        if is_combining_mark(ch) {
            // Lone mark (no base before it): independent fallback.
            picks[i] = first_with(ch, &mut has_glyph);
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < chars.len() && is_combining_mark(chars[j]) {
            j += 1;
        }
        let whole = (0..font_count)
            .find(|&f| has_glyph(f, ch) && (i + 1..j).all(|k| has_glyph(f, chars[k])));
        match whole {
            Some(f) => {
                picks[i..j].fill(f);
            }
            None => {
                for (slot, &ch) in picks[i..j].iter_mut().zip(&chars[i..j]) {
                    *slot = first_with(ch, &mut has_glyph);
                }
            }
        }
        i = j;
    }
    picks
}

/// True for wide CJK characters that allow line breaks around them
/// (conservative UAX #14 approximation for wrapping only): unified and
/// compatibility ideographs, hiragana/katakana, Hangul syllables, and
/// wide symbols, plus U+3000 (ideographic space: UAX #14 ID class
/// breaks around it like other CJK). Excludes conjoining jamo/bopomofo
/// (kept glued), halfwidth forms (narrow), and ASCII-mirroring
/// fullwidth alphanumerics (unbreakable like their ASCII halves).
pub fn is_cjk_breakable(ch: char) -> bool {
    matches!(
        ch as u32,
        0x2E80..=0x2FD5
            | 0x3000..=0x303F
            | 0x3040..=0x30FF
            | 0x31F0..=0x31FF
            | 0x3200..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE44
            | 0xFF01..=0xFF0F
            | 0xFF1A..=0xFF20
            | 0xFF3B..=0xFF40
            | 0xFF5B..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x20000..=0x2FFFF
            | 0x30000..=0x3FFFF
    )
}

/// Opening brackets: no break after (they stick to what follows).
pub fn is_cjk_open(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3008
            | 0x300A
            | 0x300C
            | 0x300E
            | 0x3010
            | 0x3014
            | 0x3016
            | 0x3018
            | 0x301A
            | 0x301D
            | 0xFF08
            | 0xFF3B
            | 0xFF5B
            | 0xFF5F
            | 0xFF62
    )
}

/// Closing punctuation, non-starters (small kana, iteration and sound
/// marks), and fullwidth mirrors of ASCII `!?,;:«»…`: no break before
/// (they stick to what precedes).
pub fn is_cjk_nobreak_before(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3001..=0x3003
            | 0x3005
            | 0x3009
            | 0x300B
            | 0x300D
            | 0x300F
            | 0x3011
            | 0x3015
            | 0x3017
            | 0x3019
            | 0x301B..=0x301C
            | 0x301E..=0x301F
            | 0x3030..=0x3035
            | 0x3041
            | 0x3043
            | 0x3045
            | 0x3047
            | 0x3049
            | 0x3063
            | 0x3083
            | 0x3085
            | 0x3087
            | 0x308E
            | 0x3095..=0x3096
            | 0x309D..=0x309E
            | 0x30A1
            | 0x30A3
            | 0x30A5
            | 0x30A7
            | 0x30A9
            | 0x30C3
            | 0x30E3
            | 0x30E5
            | 0x30E7
            | 0x30EE
            | 0x30F5..=0x30F6
            | 0x30FB..=0x30FE
            | 0x31F0..=0x31FF
            | 0xFE30..=0xFE36
            | 0xFF01
            | 0xFF0C
            | 0xFF0E
            | 0xFF1A..=0xFF1B
            | 0xFF1F
            | 0xFF60..=0xFF61
            | 0xFF63..=0xFF65
            | 0xFFE6
    )
}

/// Break opportunity between two adjacent in-word characters for the
/// wrapper: breaks around CJK wide chars (subject to open/close glues),
/// never inside combining sequences, around joiners, or between a
/// currency sign and its digits — and never between two non-CJK chars
/// (spaces own those). U+200B ZERO WIDTH SPACE breaks on both sides
/// (UAX #14 ZW); default libass builds (no unibreak) never break
/// there, so this is a documented multilingual divergence like CJK.
pub fn cjk_break_between(prev: char, next: char) -> bool {
    if prev == '\u{200C}'
        || next == '\u{200C}'
        || prev == '\u{200D}'
        || next == '\u{200D}'
        || prev == '\u{00A0}'
        || next == '\u{00A0}'
    {
        return false;
    }
    if is_combining_mark(next) {
        return false;
    }
    if prev == '\u{200B}' || next == '\u{200B}' {
        return true;
    }
    if matches!(
        prev as u32,
        0x0024 | 0x00A2 | 0x00A3 | 0x00A5 | 0xFFE0 | 0xFFE1 | 0xFFE5
    ) && matches!(next as u32, 0x0030..=0x0039 | 0xFF10..=0xFF19)
    {
        return false;
    }
    if !is_cjk_breakable(prev) && !is_cjk_breakable(next) {
        return false;
    }
    if is_cjk_open(prev) || is_cjk_nobreak_before(next) {
        return false;
    }
    true
}

/// Shaping policy: one cluster per Unicode scalar value — no ligatures,
/// kerning, mark reordering, or complex-script shaping. Advances come
/// from the face that provides each glyph; `\fsp` spacing is added
/// between glyphs (the trailing unit is stripped per line). Per-glyph
/// fallback runs are tracked via [`ShapedGlyph::font_id`]; wrap and
/// karaoke measurement use the same fallback-aware widths as shaping.
pub struct TextShaper;

impl TextShaper {
    /// Apply an ASS charset to the legacy byte-like portion of subtitle text.
    /// ASCII and non-legacy Unicode scalars are preserved.  Unknown and
    /// Symbol encodings intentionally remain Unicode-neutral because there
    /// is no portable code-page mapping for them in the current renderer.
    pub fn decode_font_encoding(text: &str, encoding: i32) -> String {
        let Some(codec) = ass_encoding(encoding) else {
            return text.to_string();
        };
        let mut output = String::with_capacity(text.len());
        let mut bytes = Vec::new();
        let flush = |output: &mut String, bytes: &mut Vec<u8>| {
            if bytes.is_empty() {
                return;
            }
            let (decoded, had_errors) = codec.decode_without_bom_handling(bytes);
            if had_errors {
                output.extend(bytes.iter().copied().map(char::from));
            } else {
                output.push_str(&decoded);
            }
            bytes.clear();
        };
        for ch in text.chars() {
            if (ch as u32) <= u32::from(u8::MAX) {
                bytes.push(ch as u8);
            } else {
                flush(&mut output, &mut bytes);
                output.push(ch);
            }
        }
        flush(&mut output, &mut bytes);
        output
    }

    /// Shape a text string into positioned glyphs (supports multi-line with \n).
    ///
    /// `font_id` is the stable [`FontManager`](super::font::FontManager)
    /// identity of `font`; it is recorded on every glyph for cache keys.
    #[allow(clippy::too_many_arguments)]
    pub fn shape(
        text: &str,
        font_id: usize,
        font: &FontArc,
        font_size: f64,
        _scale_font: f64,
        _scale_y: f64,
        font_weight: u16,
        italic: bool,
        spacing: f64,
        color: Color,
        outline_color: Color,
        shadow_color: Color,
        rotation: f64,
    ) -> ShapedLine {
        Self::shape_with_fallback(
            text,
            &[(font_id, font)],
            font_size,
            _scale_font,
            _scale_y,
            font_weight,
            italic,
            spacing,
            color,
            outline_color,
            shadow_color,
            rotation,
        )
    }

    /// Shape with a per-glyph fallback chain: `fonts[0]` is the primary
    /// face, the rest are tried in order for characters the primary
    /// lacks. Advances are measured from the selected face; baseline
    /// and line height come from the primary face. An empty chain
    /// shapes to nothing.
    #[allow(clippy::too_many_arguments)]
    pub fn shape_with_fallback(
        text: &str,
        fonts: &[(usize, &FontArc)],
        font_size: f64,
        _scale_font: f64,
        _scale_y: f64,
        font_weight: u16,
        italic: bool,
        spacing: f64,
        color: Color,
        outline_color: Color,
        shadow_color: Color,
        rotation: f64,
    ) -> ShapedLine {
        let empty = ShapedLine {
            glyphs: Vec::new(),
            width: 0.0,
            height: 0.0,
            baseline: 0.0,
            line_height: 0.0,
            missing_glyphs: 0,
        };
        // Raw degenerate sizes shape to nothing instead of feeding
        // ab_glyph invalid scales: non-finite, non-positive, or past
        // f32::MAX (the px scale is f32 and cannot represent more).
        // (At the override level, `\fs0` and non-positive results reset
        // to the style size per libass; this guard covers direct
        // callers and style data.)
        if !font_size.is_finite() || font_size <= 0.0 || font_size > f64::from(f32::MAX) {
            return empty;
        }
        let Some((_, primary)) = fonts.first() else {
            return empty;
        };
        let scale_x = _scale_font.max(0.0);
        let scale_y = _scale_y.max(0.0);
        let scale = PxScale::from(font_size as f32);
        let scaled: Vec<_> = fonts.iter().map(|(_, f)| f.as_scaled(scale)).collect();
        let primary_scaled = primary.as_scaled(scale);

        let mut glyphs = Vec::new();
        let mut missing_glyphs = 0u32;
        let mut x = 0.0_f64;
        let mut y = 0.0_f64;

        // Get font metrics (primary face defines the line box).
        let baseline = primary_scaled.ascent() as f64 * scale_y;
        let line_height = primary_scaled.height() as f64 * scale_y;
        let mut max_x = 0.0_f64;

        // Cluster-aware fallback: base + combining marks prefer one
        // face holding the whole cluster (stable per-char picks shared
        // with measurement, so widths always match shaping).
        let faces: Vec<&FontArc> = fonts.iter().map(|(_, f)| *f).collect();
        let picks = cluster_font_picks(text, fonts.len(), |fi, ch| {
            faces.get(fi).is_some_and(|f| f.glyph_id(ch).0 != 0)
        });
        for (char_idx, ch) in text.chars().enumerate() {
            let pick = picks.get(char_idx).copied().unwrap_or(0);
            if ch == '\n' || ch == '\r' {
                // Line break: close this line (stripping its own trailing
                // spacing unit), advance y, reset x.
                let line_width = if x > 0.0 { x - spacing * scale_x } else { 0.0 };
                max_x = max_x.max(line_width);
                y += line_height;
                x = 0.0;
                continue;
            }

            // Cascade: the cluster pick wins; the glyph id comes from
            // the selected face and advances are measured from it.
            let pick = pick.min(fonts.len().saturating_sub(1));
            let glyph_id = fonts
                .get(pick)
                .map(|(_, f)| f.glyph_id(ch))
                .unwrap_or(GlyphId(0));
            let font_id = fonts.get(pick).map(|(id, _)| *id).unwrap_or(0);
            if glyph_id.0 == 0 {
                missing_glyphs += 1;
            }
            let advance = scaled[pick].h_advance(glyph_id) as f64;

            glyphs.push(ShapedGlyph {
                glyph_id,
                font_id,
                ch,
                x,
                y,
                advance: advance * scale_x,
                font_size,
                color,
                outline_color,
                shadow_color,
                font_weight,
                italic,
                scale_x,
                scale_y,
                rotation,
            });

            // Add advance + spacing to next character position
            x += (advance + spacing) * scale_x;
        }

        // Close the final line with the same trailing-spacing rule.
        let line_width = if x > 0.0 { x - spacing * scale_x } else { 0.0 };
        max_x = max_x.max(line_width);

        ShapedLine {
            glyphs,
            width: max_x,
            height: y + line_height,
            baseline,
            line_height,
            missing_glyphs,
        }
    }

    /// Shape with OpenType substitutions/positioning and Unicode bidi.
    ///
    /// Fallback is resolved at cluster boundaries first, then each
    /// fallback run is shaped as a real OpenType run.  The resulting
    /// glyphs, advances, offsets, and clusters are the same data used by
    /// rendering and measurement; no scalar-width approximation is used
    /// on this path.
    #[allow(clippy::too_many_arguments)]
    pub fn shape_with_opentype(
        text: &str,
        fonts: &[ShapingFont<'_>],
        font_size: f64,
        scale_x: f64,
        scale_y: f64,
        font_weight: u16,
        italic: bool,
        spacing: f64,
        color: Color,
        outline_color: Color,
        shadow_color: Color,
        rotation: f64,
    ) -> ShapedLine {
        let empty = ShapedLine {
            glyphs: Vec::new(),
            width: 0.0,
            height: 0.0,
            baseline: 0.0,
            line_height: 0.0,
            missing_glyphs: 0,
        };
        if fonts.is_empty()
            || !font_size.is_finite()
            || font_size <= 0.0
            || font_size > f64::from(f32::MAX)
        {
            return empty;
        }
        let Some(primary) = fonts.first() else {
            return empty;
        };
        let scale_x = scale_x.max(0.0);
        let scale_y = scale_y.max(0.0);
        let px_scale = PxScale::from(font_size as f32);
        let primary_scaled = primary.raster.as_scaled(px_scale);
        let baseline = primary_scaled.ascent() as f64 * scale_y;
        let line_height = primary_scaled.height() as f64 * scale_y;
        let mut glyphs = Vec::new();
        let mut width: f64 = 0.0;
        let mut y = 0.0;
        let mut missing_glyphs = 0u32;

        for (line_index, line) in text.split(['\n', '\r']).enumerate() {
            let (line_glyphs, line_width, missing) = shape_opentype_line(
                line,
                fonts,
                font_size,
                scale_x,
                scale_y,
                font_weight,
                italic,
                spacing,
                color,
                outline_color,
                shadow_color,
                rotation,
            );
            missing_glyphs = missing_glyphs.saturating_add(missing);
            width = width.max(line_width);
            for mut glyph in line_glyphs {
                glyph.y += y;
                glyph.scale_y = scale_y;
                glyph.font_size = font_size;
                glyph.font_weight = font_weight;
                glyph.italic = italic;
                glyph.color = color;
                glyph.outline_color = outline_color;
                glyph.shadow_color = shadow_color;
                glyph.rotation = rotation;
                glyphs.push(glyph);
            }
            if line_index + 1 < text.split(['\n', '\r']).count() {
                y += line_height;
            }
        }

        // A trailing line break still contributes a line box, matching the
        // scalar path and the renderer's line accounting.
        let line_count = text.chars().filter(|c| *c == '\n' || *c == '\r').count() + 1;
        let height = line_height * line_count as f64;
        ShapedLine {
            glyphs,
            width,
            height,
            baseline,
            line_height,
            missing_glyphs,
        }
    }

    /// Measure using the exact OpenType+bidi pipeline used by shaping.
    pub fn measure_text_with_opentype(
        text: &str,
        fonts: &[ShapingFont<'_>],
        font_size: f64,
        scale_x: f64,
        spacing: f64,
    ) -> f64 {
        shape_measure_opentype(text, fonts, font_size, scale_x, spacing)
    }

    /// Measure text width without creating glyphs. For multiline text,
    /// returns the widest line (newlines are breaks, not skips).
    pub fn measure_text(text: &str, font: &FontArc, font_size: f64, spacing: f64) -> f64 {
        Self::measure_text_with_fallback(text, std::slice::from_ref(&font), font_size, spacing)
    }

    /// Fallback-aware measurement: each character's advance comes from
    /// the first font in `fonts` that contains it (same cascade as
    /// [`Self::shape_with_fallback`]), so wrap and karaoke widths match
    /// shaping exactly. Empty chain measures 0.
    pub fn measure_text_with_fallback(
        text: &str,
        fonts: &[&FontArc],
        font_size: f64,
        spacing: f64,
    ) -> f64 {
        if !font_size.is_finite() || font_size <= 0.0 || font_size > f64::from(f32::MAX) {
            return 0.0;
        }
        if fonts.is_empty() {
            return 0.0;
        }
        let scale = PxScale::from(font_size as f32);
        let scaled: Vec<_> = fonts.iter().map(|f| f.as_scaled(scale)).collect();
        // Same cluster-aware picks as shaping (computed over the full
        // text so mark attachments match); per-line char offsets index
        // into it. Widths always match shaping exactly.
        let picks = cluster_font_picks(text, fonts.len(), |fi, ch| {
            fonts.get(fi).is_some_and(|f| f.glyph_id(ch).0 != 0)
        });
        let mut char_idx = 0usize;
        let mut widest = 0.0_f64;
        for line in text.split(['\n', '\r']) {
            let mut width = 0.0_f64;
            let mut first = true;
            for ch in line.chars() {
                let pick = picks
                    .get(char_idx)
                    .copied()
                    .unwrap_or(0)
                    .min(fonts.len().saturating_sub(1));
                char_idx += 1;
                let glyph_id = fonts
                    .get(pick)
                    .map(|f| f.glyph_id(ch))
                    .unwrap_or(GlyphId(0));
                if !first {
                    width += spacing;
                }
                width += scaled
                    .get(pick)
                    .map(|s| s.h_advance(glyph_id) as f64)
                    .unwrap_or(0.0);
                first = false;
            }
            // Account for the split-off break char (except after the
            // last line, where there is none to skip).
            char_idx += 1;
            widest = widest.max(width);
        }
        widest
    }

    /// Split text into lines based on max width
    pub fn wrap_text(
        text: &str,
        font: &FontArc,
        font_size: f64,
        max_width: f64,
        spacing: f64,
    ) -> Vec<String> {
        if max_width <= 0.0 {
            return vec![text.to_string()];
        }

        let mut lines = Vec::new();
        let mut current_line = String::new();
        let mut current_width = 0.0;

        for word in text.split(' ') {
            let word_width = Self::measure_text(word, font, font_size, spacing);

            if current_line.is_empty() {
                current_line = word.to_string();
                current_width = word_width;
            } else if current_width + spacing + word_width <= max_width {
                current_line.push(' ');
                current_line.push_str(word);
                current_width += spacing + word_width;
            } else {
                lines.push(current_line);
                current_line = word.to_string();
                current_width = word_width;
            }
        }

        if !current_line.is_empty() {
            lines.push(current_line);
        }

        lines
    }
}

/// Shape one logical line into visual-order OpenType runs.  This helper is
/// deliberately bounded by the input line length and the caller's document
/// caps; rustybuzz itself performs the GSUB/GPOS work.
fn shape_opentype_line(
    text: &str,
    fonts: &[ShapingFont<'_>],
    font_size: f64,
    scale_x: f64,
    scale_y: f64,
    font_weight: u16,
    italic: bool,
    spacing: f64,
    color: Color,
    outline_color: Color,
    shadow_color: Color,
    rotation: f64,
) -> (Vec<ShapedGlyph>, f64, u32) {
    if text.is_empty() {
        return (Vec::new(), 0.0, 0);
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let picks = cluster_font_picks(text, fonts.len(), |face, ch| {
        fonts
            .get(face)
            .is_some_and(|font| font.raster.glyph_id(ch).0 != 0)
    });

    let bidi = BidiInfo::new(text, None);
    let para = bidi.paragraphs.first();
    let levels = para
        .map(|p| bidi.reordered_levels_per_char(p, p.range.clone()))
        .unwrap_or_else(|| vec![unicode_bidi::LTR_LEVEL; chars.len()]);
    let levels = if levels.len() == chars.len() {
        levels
    } else {
        vec![unicode_bidi::LTR_LEVEL; chars.len()]
    };
    let visual_indices = BidiInfo::reorder_visual(&levels);

    let mut runs: Vec<(usize, usize, usize, bool)> = Vec::new();
    let mut start = 0usize;
    while start < chars.len() {
        let pick = picks.get(start).copied().unwrap_or(0);
        let rtl = levels.get(start).is_some_and(|l| l.is_rtl());
        let mut end = start + 1;
        while end < chars.len()
            && picks.get(end).copied().unwrap_or(0) == pick
            && levels.get(end).is_some_and(|l| l.is_rtl()) == rtl
        {
            end += 1;
        }
        runs.push((start, end, pick, rtl));
        start = end;
    }

    // Bidi L2 returns character indices in visual order.  Stable sorting by
    // the first visual member keeps adjacent fallback sub-runs together.
    let mut visual_run_ids: Vec<usize> = (0..runs.len()).collect();
    visual_run_ids.sort_by_key(|run_id| {
        let (from, to, _, _) = runs[*run_id];
        visual_indices
            .iter()
            .position(|idx| *idx >= from && *idx < to)
            .unwrap_or(usize::MAX)
    });

    let mut output = Vec::new();
    let mut pen_x = 0.0;
    let mut missing = 0u32;
    for run_id in visual_run_ids {
        let (from, to, pick, rtl) = runs[run_id];
        let byte_start = chars[from].0;
        let byte_end = if to < chars.len() {
            chars[to].0
        } else {
            text.len()
        };
        let run_text = &text[byte_start..byte_end];
        let Some(font) = fonts.get(pick.min(fonts.len().saturating_sub(1))) else {
            continue;
        };
        let Some(face) = rustybuzz::Face::from_slice(font.data, font.face_index) else {
            continue;
        };
        // `ab_glyph::PxScale` treats ASS's font size as the requested
        // em-height (`height_unscaled`), rather than as raw OpenType
        // units-per-em.  Use the same raster scale for HarfBuzz positions
        // or shaped advances will be wider than the bitmaps they place.
        let font_scale = font_size / f64::from(font.raster.height_unscaled().max(1.0));
        let mut buffer = UnicodeBuffer::new();
        buffer.set_direction(if rtl {
            Direction::RightToLeft
        } else {
            Direction::LeftToRight
        });
        buffer.set_cluster_level(BufferClusterLevel::MonotoneCharacters);
        buffer.push_str(run_text);
        let shaped = rustybuzz::shape(&face, &[], buffer);
        let infos = shaped.glyph_infos();
        let positions = shaped.glyph_positions();
        if infos.is_empty() {
            continue;
        }
        let advances: Vec<f64> = positions
            .iter()
            .map(|position| f64::from(position.x_advance) * font_scale * scale_x)
            .collect();
        let gaps: Vec<f64> = (0..infos.len())
            .map(|i| {
                if i + 1 < infos.len() && infos[i].cluster != infos[i + 1].cluster {
                    spacing * scale_x
                } else {
                    0.0
                }
            })
            .collect();
        let run_width: f64 = advances
            .iter()
            .zip(&gaps)
            .map(|(advance, gap)| advance + gap)
            .sum();
        let mut cursor = if rtl { run_width } else { 0.0 };
        for (index, (info, position)) in infos.iter().zip(positions).enumerate() {
            let advance = advances[index];
            let gap = gaps[index];
            let local_x = if rtl {
                cursor -= advance;
                let x = cursor;
                cursor -= gap;
                x
            } else {
                let x = cursor;
                cursor += advance + gap;
                x
            };
            let cluster_byte = usize::try_from(info.cluster).unwrap_or(0);
            let ch = run_text
                .get(cluster_byte..)
                .and_then(|s| s.chars().next())
                .unwrap_or('�');
            let glyph_id = GlyphId(info.glyph_id.min(u32::from(u16::MAX)) as u16);
            if glyph_id.0 == 0 {
                missing = missing.saturating_add(1);
            }
            output.push(ShapedGlyph {
                glyph_id,
                font_id: font.id,
                ch,
                x: pen_x + local_x + f64::from(position.x_offset) * font_scale * scale_x,
                y: -f64::from(position.y_offset) * font_scale * scale_y,
                advance,
                font_size,
                color,
                outline_color,
                shadow_color,
                font_weight,
                italic,
                scale_x,
                scale_y,
                rotation,
            });
        }
        pen_x += run_width;
    }
    (output, pen_x, missing)
}

fn shape_measure_opentype(
    text: &str,
    fonts: &[ShapingFont<'_>],
    font_size: f64,
    scale_x: f64,
    spacing: f64,
) -> f64 {
    let shaped = TextShaper::shape_with_opentype(
        text,
        fonts,
        font_size,
        scale_x,
        1.0,
        400,
        false,
        spacing,
        Color::white(),
        Color::black(),
        Color::black(),
        0.0,
    );
    shaped.width
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::font;

    fn fallback_font() -> FontArc {
        let mut manager = crate::renderer::font::FontManager::new();
        manager
            .load_font("DejaVu Sans", font::get_fallback_font(), false, false)
            .unwrap();
        manager.find_font("DejaVu Sans", false, false).clone()
    }

    #[test]
    fn test_text_shaper_new() {
        let _ = TextShaper;
    }

    #[test]
    fn test_ass_charset_mapping_preserves_unicode_scripts() {
        assert_eq!(TextShaper::decode_font_encoding("caf\u{e9}", 1), "café");
        assert_eq!(TextShaper::decode_font_encoding("\u{82}\u{a0}", 128), "あ");
        assert_eq!(
            TextShaper::decode_font_encoding("\u{d6}\u{d0}\u{ce}\u{c4}", 134),
            "中文"
        );
        assert_eq!(TextShaper::decode_font_encoding("\u{3b1}", 161), "α");
        // A real Unicode scalar outside the legacy-byte bridge is left alone.
        assert_eq!(TextShaper::decode_font_encoding("日本語", 128), "日本語");
        // Symbol/unknown values have no portable code-page mapping here.
        assert_eq!(TextShaper::decode_font_encoding("\u{f0}", 2), "ð");
        assert_eq!(TextShaper::decode_font_encoding("\u{f0}", 999), "ð");
    }

    #[test]
    fn test_cluster_picks_cascade_and_stick() {
        // First face holding the char wins; all-missing falls back to 0.
        let has = |face: usize, ch: char| match (face, ch) {
            (0, 'a') => true,
            (1, 'b') => true,
            // Face 1 holds the mark but not the base: the cluster must
            // not split (base-only face 0 wins the pair over mark-only
            // face 1, and no face holds both so each falls back alone).
            (1, '\u{301}') => true,
            _ => false,
        };
        assert_eq!(cluster_font_picks("a", 2, has), vec![0]);
        assert_eq!(cluster_font_picks("b", 2, has), vec![1]);
        assert_eq!(cluster_font_picks("z", 2, has), vec![0]);
        // Base on face 0 + mark on face 1, neither holding both:
        // independent fallback keeps each char's own face.
        assert_eq!(cluster_font_picks("a\u{301}", 2, has), vec![0, 1]);
        // Whole-cluster preference: face 1 holds base AND mark, so the
        // pair sticks to face 1 even though face 0 holds the base.
        let both = |face: usize, ch: char| face == 1 && (ch == 'a' || ch == '\u{301}');
        assert_eq!(cluster_font_picks("xa\u{301}", 2, both), vec![0, 1, 1]);
    }

    #[test]
    fn test_combining_marks_glue_common_scripts() {
        // Marks never start a wrapped line, whatever their script.
        for mark in [
            '\u{301}',  // Latin acute
            '\u{485}',  // Cyrillic dasia pneumata
            '\u{5B0}',  // Hebrew sheva
            '\u{64B}',  // Arabic fathatan
            '\u{7A8}',  // Thaana sukun
            '\u{93E}',  // Devanagari vowel sign AA
            '\u{E38}',  // Thai vowel sign Sara U
            '\u{EB5}',  // Lao vowel sign I
            '\u{F72}',  // Tibetan vowel sign I
            '\u{3099}', // kana voicing mark
        ] {
            assert!(is_combining_mark(mark), "{mark:?} must glue");
            assert!(!cjk_break_between('a', mark));
            assert!(!cjk_break_between('あ', mark));
        }
        // Letters and spacing marks stay break-neutral.
        for ch in ['a', 'あ', '\u{2FF}', '\u{905}'] {
            assert!(!is_combining_mark(ch), "{ch:?} must not glue");
        }
    }

    #[test]
    fn test_zwsp_breaks_both_sides() {
        assert!(cjk_break_between('a', '\u{200B}'));
        assert!(cjk_break_between('\u{200B}', 'a'));
        assert!(cjk_break_between('あ', '\u{200B}'));
        assert!(cjk_break_between('\u{200B}', 'あ'));
        // But not into combining marks or across joiners/NBSP.
        assert!(!cjk_break_between('\u{200B}', '\u{301}'));
        assert!(!cjk_break_between('\u{200B}', '\u{200D}'));
        assert!(!cjk_break_between('\u{00A0}', '\u{200B}'));
    }

    #[test]
    fn test_ideographic_space_breaks_like_cjk() {
        assert!(is_cjk_breakable('\u{3000}'));
        assert!(cjk_break_between('あ', '\u{3000}'));
        assert!(cjk_break_between('\u{3000}', 'あ'));
        assert!(cjk_break_between('a', '\u{3000}'));
        assert!(cjk_break_between('\u{3000}', 'a'));
    }

    #[test]
    fn test_shape_records_font_ids_and_missing() {
        let mut manager = crate::renderer::font::FontManager::new();
        manager
            .load_font("First", font::get_fallback_font(), false, false)
            .unwrap();
        manager
            .load_font("Second", font::get_fallback_font(), false, false)
            .unwrap();
        let f0 = manager.get_font(0).unwrap().clone();
        let f1 = manager.get_font(1).unwrap().clone();
        let shape = |text: &str| {
            TextShaper::shape_with_fallback(
                text,
                &[(0, &f0), (1, &f1)],
                48.0,
                1.0,
                1.0,
                400,
                false,
                0.0,
                Color::white(),
                Color::black(),
                Color::black(),
                0.0,
            )
        };
        // Covered characters resolve to the primary face.
        let line = shape("A");
        assert_eq!(line.glyphs.len(), 1);
        assert_eq!(line.glyphs[0].font_id, 0);
        assert_ne!(line.glyphs[0].glyph_id.0, 0);
        assert_eq!(line.missing_glyphs, 0);
        // U+10FFFF is absent from DejaVu Sans: primary .notdef + count.
        assert_eq!(f0.glyph_id('\u{10FFFF}').0, 0);
        let line = shape("A\u{10FFFF}B");
        assert_eq!(line.glyphs.len(), 3);
        assert_eq!(line.glyphs[1].font_id, 0);
        assert_eq!(line.glyphs[1].glyph_id.0, 0);
        assert_eq!(line.missing_glyphs, 1);
        // Empty chain shapes to nothing.
        let line = TextShaper::shape_with_fallback(
            "A",
            &[],
            48.0,
            1.0,
            1.0,
            400,
            false,
            0.0,
            Color::white(),
            Color::black(),
            Color::black(),
            0.0,
        );
        assert!(line.glyphs.is_empty());
    }

    #[test]
    fn test_opentype_shaping_uses_gsub_bidi_and_marks() {
        let mut manager = crate::renderer::font::FontManager::new();
        manager
            .load_font("Debug", font::get_fallback_font(), false, false)
            .unwrap();
        let raster = manager.get_font(0).unwrap();
        let (data, face_index) = manager.shaping_data(0).unwrap();
        let ot = ShapingFont {
            id: 0,
            raster,
            data,
            face_index,
        };
        let shape = |text: &str| {
            TextShaper::shape_with_opentype(
                text,
                &[ot],
                36.0,
                1.0,
                1.0,
                400,
                false,
                0.0,
                Color::white(),
                Color::black(),
                Color::black(),
                0.0,
            )
        };
        // DejaVu Sans exposes an ffi ligature: GSUB must reduce six source
        // scalars to four raster glyphs.
        let ligature = shape("office");
        assert_eq!(ligature.glyphs.len(), 4);
        assert!(ligature.width > 0.0);
        // Arabic is emitted in visual order with joined glyph IDs rather
        // than five isolated scalar glyphs.
        let arabic = shape("\u{0645}\u{0631}\u{062D}\u{0628}\u{0627}");
        assert_eq!(arabic.glyphs.len(), 5);
        assert!(arabic.glyphs.windows(2).any(|pair| pair[0].x > pair[1].x));
        assert!(arabic.glyphs.iter().all(|glyph| glyph.glyph_id.0 != 0));
        // Combining marks stay in the base cluster and are positioned by
        // GPOS, so the mark does not create an independent advance.
        let mark = shape("A\u{301}");
        assert_eq!(mark.glyphs.len(), 1);
        assert!(mark.glyphs[0].glyph_id.0 != 0);
    }

    #[test]
    fn test_measure_matches_shape_width_with_fallback() {
        let font = fallback_font();
        let fonts = [&font, &font];
        for text in ["AB", "A B", "Hello, world!", "A\u{10FFFF}B"] {
            let shaped = TextShaper::shape_with_fallback(
                text,
                &[(0, &font), (1, &font)],
                48.0,
                1.0,
                1.0,
                400,
                false,
                2.0,
                Color::white(),
                Color::black(),
                Color::black(),
                0.0,
            );
            let measured = TextShaper::measure_text_with_fallback(text, &fonts, 48.0, 2.0);
            assert!(
                (shaped.width - measured).abs() < 1e-9,
                "{text:?}: shaped {} vs measured {measured}",
                shaped.width
            );
        }
        assert_eq!(
            TextShaper::measure_text_with_fallback("AB", &[], 48.0, 0.0),
            0.0
        );
    }

    #[test]
    fn test_shaping_is_scalar_per_glyph() {
        // No ligation, no kerning, no cluster merging: one glyph per
        // scalar, spacing between glyphs, trailing unit stripped.
        let font = fallback_font();
        let shaped = TextShaper::shape(
            "fi A",
            0,
            &font,
            48.0,
            1.0,
            1.0,
            400,
            false,
            3.0,
            Color::white(),
            Color::black(),
            Color::black(),
            0.0,
        );
        assert_eq!(shaped.glyphs.len(), 4);
        let advances: f64 = shaped.glyphs.iter().map(|g| g.advance).sum();
        assert!((shaped.width - (advances + 3.0 * 3.0)).abs() < 1e-9);
        assert_eq!(shaped.missing_glyphs, 0);
    }

    #[test]
    fn test_degenerate_sizes_shape_to_nothing() {
        let font = fallback_font();
        for size in [0.0, -5.0, f64::NAN, f64::INFINITY] {
            let shaped = TextShaper::shape(
                "Hi",
                0,
                &font,
                size,
                1.0,
                1.0,
                400,
                false,
                0.0,
                Color::white(),
                Color::black(),
                Color::black(),
                0.0,
            );
            assert!(shaped.glyphs.is_empty(), "size {size}");
            assert_eq!(shaped.width, 0.0);
            assert_eq!(TextShaper::measure_text("Hi", &font, size, 0.0), 0.0);
        }
    }

    #[test]
    fn test_text_shaper_applies_non_uniform_scale() {
        let font = fallback_font();
        let normal = TextShaper::shape(
            "AB",
            0,
            &font,
            48.0,
            1.0,
            1.0,
            400,
            false,
            0.0,
            Color::white(),
            Color::black(),
            Color::black(),
            0.0,
        );
        let scaled = TextShaper::shape(
            "AB",
            0,
            &font,
            48.0,
            2.0,
            0.5,
            400,
            false,
            0.0,
            Color::white(),
            Color::black(),
            Color::black(),
            0.0,
        );

        assert!((scaled.width - normal.width * 2.0).abs() < 0.01);
        assert!((scaled.height - normal.height * 0.5).abs() < 0.01);
        assert!((scaled.glyphs[1].x - normal.glyphs[1].x * 2.0).abs() < 0.01);
    }
}
