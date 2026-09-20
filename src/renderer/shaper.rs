use ab_glyph::{Font, FontArc, GlyphId, PxScale, ScaleFont};
use unicode_bidi::{BidiInfo, Level};

use crate::types::Color;

#[path = "shaper/encoding.rs"]
mod encoding;
#[path = "shaper/fallback.rs"]
mod fallback;
#[path = "shaper/line_break.rs"]
mod line_break;
#[path = "shaper/opentype.rs"]
mod opentype;

use self::encoding::{decode_ass_bytes, decode_ass_text};
pub use self::fallback::cluster_font_picks;
pub use self::line_break::{
    cjk_break_between, is_cjk_breakable, is_cjk_nobreak_before, is_cjk_open, is_combining_mark,
};
use self::opentype::{shape_measure_opentype, shape_opentype_line};

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

/// A raster face paired with the original sfnt bytes used by harfrust.
/// `FontArc` remains the rasterizer source of truth; the byte slice and
/// collection index are only used for OpenType substitutions/positioning.
#[derive(Clone, Copy)]
pub struct ShapingFont<'a> {
    pub id: usize,
    pub raster: &'a FontArc,
    pub data: &'a [u8],
    pub face_index: u32,
    /// FreeType-compatible face metrics in font units (OS/2 Win
    /// basis when the table parses; see
    /// [`FontManager::ft_metrics`](crate::renderer::font::FontManager::ft_metrics)):
    /// ascender, negative descender, and their difference. Advances
    /// and baselines scale by `font_size / ft_height`.
    pub ft_asc: f32,
    pub ft_desc: f32,
    pub ft_height: f32,
}

/// Text shaping entry points. The scalar fallback path remains available for
/// faces without OpenType bytes; normal rendering uses the HarfBuzz-compatible
/// GSUB/GPOS+bidi path below, and layout/rendering share its cluster advances.
pub struct TextShaper;

impl TextShaper {
    /// Apply an ASS charset to the legacy byte-like portion of subtitle text.
    /// ASCII and non-legacy Unicode scalars are preserved. Symbol bytes map
    /// to the Windows Symbol private-use cmap range; Johab remains explicit
    /// Unicode-neutral because no portable Johab codec is bundled. Invalid
    /// byte sequences decode with the WHATWG replacement character, while a
    /// following real Unicode scalar starts a fresh, unaffected run.
    pub fn decode_font_encoding(text: &str, encoding: i32) -> String {
        decode_ass_text(text, encoding)
    }

    /// Decode byte-preserving event text, including mid-event `\fe` changes.
    pub fn decode_ass_bytes(bytes: &[u8], encoding: i32) -> String {
        decode_ass_bytes(bytes, encoding)
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
    /// on this path. `kerning` mirrors libass `track->Kerning` (default
    /// off); non-zero `spacing` additionally disables `liga`/`clig`,
    /// matching libass `ass_shaper` behavior. `base_level` overrides
    /// the per-line paragraph direction (`None` auto-detects): layout
    /// passes the event direction so soft-wrapped lines keep it.
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
        kerning: bool,
        base_level: Option<Level>,
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
        // Primary face defines the line box (libass takes the max
        // over run faces; single-face lines are identical). The
        // values AND the divisor are FreeType-basis: Noto's Win
        // ascender (1348) differs from its hhea/typo one (896), so
        // rescaling `ab_glyph` output would still misplace the pen.
        let ft_height = f64::from(primary.ft_height.max(1.0));
        let baseline = f64::from(primary.ft_asc) / ft_height * font_size * scale_y;
        let line_height =
            f64::from(primary.ft_asc - primary.ft_desc) / ft_height * font_size * scale_y;
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
                kerning,
                base_level,
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
    /// Width is direction-independent, so measurement always
    /// auto-detects (`base_level` is a shaping/layout concern only).
    pub fn measure_text_with_opentype(
        text: &str,
        fonts: &[ShapingFont<'_>],
        font_size: f64,
        scale_x: f64,
        spacing: f64,
        kerning: bool,
    ) -> f64 {
        shape_measure_opentype(text, fonts, font_size, scale_x, spacing, kerning)
    }

    /// Base paragraph level of a text span (first paragraph): the
    /// event-level direction that soft-wrapped lines inherit instead
    /// of re-detecting from their own first strong character.
    pub fn paragraph_base_level(text: &str) -> Level {
        let bidi = BidiInfo::new(text, None);
        bidi.paragraphs
            .first()
            .map(|para| para.level)
            .unwrap_or(Level::ltr())
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
        // Windows single-byte charsets (not ISO lookalikes): bytes whose
        // mappings differ catch accidental regressions to ISO-8859 tables.
        assert_eq!(TextShaper::decode_font_encoding("\u{80}", 161), "€");
        assert_eq!(TextShaper::decode_font_encoding("\u{80}", 177), "€");
        assert_eq!(TextShaper::decode_font_encoding("\u{80}", 238), "€");
        // Multibyte runs decode as one byte stream; incomplete sequences use
        // U+FFFD and never consume or corrupt later genuine Unicode.
        assert_eq!(TextShaper::decode_font_encoding("\u{a4}\u{40}", 136), "一");
        assert_eq!(TextShaper::decode_font_encoding("\u{82}α", 128), "�α");
        // Symbol bytes use the Windows Symbol private-use cmap range.
        assert_eq!(TextShaper::decode_font_encoding("\u{f0}", 2), "\u{f0f0}");
        assert_eq!(
            TextShaper::decode_font_encoding(r"{\fe2}A{\fe0}B", 0),
            "{\\fe2}\u{f041}{\\fe0}B"
        );
        // Johab is not provided by encoding_rs and remains explicitly neutral.
        assert_eq!(
            TextShaper::decode_font_encoding("\u{84}\u{41}", 130),
            "\u{84}A"
        );
        assert_eq!(TextShaper::decode_font_encoding("\u{f0}", 999), "ð");
    }

    #[test]
    fn test_raw_byte_decoding_preserves_utf8_and_switches_fe_state() {
        // These are actual legacy bytes, not Unicode strings containing
        // lookalike code points: CP1252 E9 is é and CP1253 E1 is α.
        assert_eq!(TextShaper::decode_ass_bytes(b"caf\xE9", 1), "café");
        assert_eq!(TextShaper::decode_ass_bytes(b"\x82\xA0", 128), "あ");
        assert_eq!(
            TextShaper::decode_ass_bytes(b"{\\fe161}\xE1{\\fe0}\xE9", 1),
            "{\\fe161}α{\\fe0}é"
        );
        assert_eq!(
            TextShaper::decode_ass_bytes("日本語".as_bytes(), 128),
            "日本語"
        );
    }

    #[test]
    fn test_symbol_bytes_use_private_use_cmap_and_preserve_ass_breaks() {
        assert_eq!(
            TextShaper::decode_ass_bytes(b"A\\N B", 2),
            "\u{f041}\\N\u{f020}\u{f042}"
        );
        assert_eq!(TextShaper::decode_ass_bytes(b"{\\fe0}A", 2), "{\\fe0}A");
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
        let (ft_asc, ft_desc, ft_height) = manager.ft_metrics(0).unwrap();
        let ot = ShapingFont {
            id: 0,
            raster,
            data,
            face_index,
            ft_asc,
            ft_desc,
            ft_height,
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
                false,
                None,
            )
        };
        // DejaVu Sans exposes an ffi ligature: GSUB must reduce six source
        // scalars to four raster glyphs.
        let ligature = shape("office");
        assert_eq!(ligature.glyphs.len(), 4);
        assert!(ligature.width > 0.0);
        // Arabic is emitted in visual order (last logical scalar
        // first, ascending x) with joined glyph IDs rather than five
        // isolated scalar glyphs.
        let arabic = shape("\u{0645}\u{0631}\u{062D}\u{0628}\u{0627}");
        assert_eq!(arabic.glyphs.len(), 5);
        assert!(
            arabic.glyphs.windows(2).all(|pair| pair[0].x < pair[1].x),
            "RTL runs lay out left to right in visual order"
        );
        assert_eq!(arabic.glyphs[0].ch, '\u{0627}');
        assert!(arabic.glyphs.iter().all(|glyph| glyph.glyph_id.0 != 0));
        // Combining marks stay in the base cluster and are positioned by
        // GPOS, so the mark does not create an independent advance.
        let mark = shape("A\u{301}");
        assert_eq!(mark.glyphs.len(), 1);
        assert!(mark.glyphs[0].glyph_id.0 != 0);
    }

    #[test]
    fn test_noto_advances_use_win_divisor() {
        // Isolated DEVANAGARI NA: hmtx advance 555u. FreeType sizes
        // Noto by the OS/2 Win sum (1906), so @24 the advance is
        // 555*24/1906 = 6.99px (libass na10: 7.00px pitch); the hhea
        // divisor (1304) would give 10.22px. Baseline likewise uses
        // the Win ascender (1348), not hhea/typo (896).
        let mut manager = crate::renderer::font::FontManager::new();
        manager
            .load_font("Debug", font::get_fallback_font(), false, false)
            .unwrap();
        let noto = std::fs::read("fonts/NotoSansDevanagari.ttf").unwrap();
        manager.load_font("Noto", &noto, false, false).unwrap();
        let raster = manager.get_font(1).unwrap();
        let (data, face_index) = manager.shaping_data(1).unwrap();
        let (ft_asc, ft_desc, ft_height) = manager.ft_metrics(1).unwrap();
        let ot = ShapingFont {
            id: 1,
            raster,
            data,
            face_index,
            ft_asc,
            ft_desc,
            ft_height,
        };
        let line = TextShaper::shape_with_opentype(
            "\u{928}",
            std::slice::from_ref(&ot),
            24.0,
            1.0,
            1.0,
            400,
            false,
            0.0,
            Color::white(),
            Color::black(),
            Color::black(),
            0.0,
            false,
            None,
        );
        assert_eq!(line.glyphs.len(), 1);
        assert!(
            (line.glyphs[0].advance - 555.0 * 24.0 / 1906.0).abs() < 1e-6,
            "NA advance {}",
            line.glyphs[0].advance
        );
        assert!(
            (line.baseline - 1348.0 * 24.0 / 1906.0).abs() < 1e-6,
            "baseline {}",
            line.baseline
        );
        assert!(
            (line.line_height - 24.0).abs() < 1e-6,
            "line height {}",
            line.line_height
        );
    }

    #[test]
    fn test_opentype_kerning_matches_libass_default_off() {
        // libass leaves `kern` off unless the track enables `Kerning`
        // (reference frames are unkerned): default shaping must match
        // raw hmtx advances, while `kerning = true` tightens kerned
        // pairs like "To". Measurement follows the same flag.
        let mut manager = crate::renderer::font::FontManager::new();
        manager
            .load_font("Debug", font::get_fallback_font(), false, false)
            .unwrap();
        let raster = manager.get_font(0).unwrap();
        let (data, face_index) = manager.shaping_data(0).unwrap();
        let (ft_asc, ft_desc, ft_height) = manager.ft_metrics(0).unwrap();
        let ot = ShapingFont {
            id: 0,
            raster,
            data,
            face_index,
            ft_asc,
            ft_desc,
            ft_height,
        };
        let shape = |kerning: bool| {
            TextShaper::shape_with_opentype(
                "Top-left",
                &[ot],
                24.0,
                1.0,
                1.0,
                400,
                false,
                0.0,
                Color::white(),
                Color::black(),
                Color::black(),
                0.0,
                kerning,
                None,
            )
        };
        let plain = shape(false);
        let kerned = shape(true);
        assert_eq!(plain.glyphs.len(), 8);
        assert_eq!(kerned.glyphs.len(), 8);
        // Unkerned: T advance equals the raw hmtx advance scaled by
        // ab_glyph's height-relative factor.
        let t = raster.glyph_id('T');
        let expected =
            f64::from(raster.h_advance_unscaled(t)) * 24.0 / f64::from(raster.height_unscaled());
        assert!(
            (plain.glyphs[0].advance - expected).abs() < 1e-6,
            "T advance {} vs hmtx {expected}",
            plain.glyphs[0].advance
        );
        // Kerned: strictly narrower overall ("To" tightens).
        assert!(
            kerned.width < plain.width - 1.0,
            "kerned {} vs plain {}",
            kerned.width,
            plain.width
        );
        for kerning in [false, true] {
            let measured =
                TextShaper::measure_text_with_opentype("Top-left", &[ot], 24.0, 1.0, 0.0, kerning);
            let width = shape(kerning).width;
            assert!(
                (measured - width).abs() < 1e-9,
                "kerning={kerning}: measured {measured} vs shaped {width}"
            );
        }
    }

    #[test]
    fn test_opentype_base_level_overrides_line_direction() {
        // A soft-wrapped line starting with RTL text keeps the event
        // base direction instead of re-detecting: with an LTR base,
        // "שלום world" lays the Hebrew run left of "world"; with the
        // auto (RTL) base the runs swap (libass: mixed-bidi).
        let mut manager = crate::renderer::font::FontManager::new();
        manager
            .load_font("Debug", font::get_fallback_font(), false, false)
            .unwrap();
        let raster = manager.get_font(0).unwrap();
        let (data, face_index) = manager.shaping_data(0).unwrap();
        let (ft_asc, ft_desc, ft_height) = manager.ft_metrics(0).unwrap();
        let ot = ShapingFont {
            id: 0,
            raster,
            data,
            face_index,
            ft_asc,
            ft_desc,
            ft_height,
        };
        let shape = |base_level: Option<Level>| {
            TextShaper::shape_with_opentype(
                "שלום world",
                &[ot],
                24.0,
                1.0,
                1.0,
                400,
                false,
                0.0,
                Color::white(),
                Color::black(),
                Color::black(),
                0.0,
                false,
                base_level,
            )
        };
        let min_x = |line: &ShapedLine, ch: char| {
            line.glyphs
                .iter()
                .filter(|g| g.ch == ch)
                .map(|g| g.x)
                .fold(f64::INFINITY, f64::min)
        };
        let ltr = shape(Some(Level::ltr()));
        assert!(
            min_x(&ltr, 'ש') < min_x(&ltr, 'w'),
            "LTR base: Hebrew run must sit left of `world`"
        );
        let rtl = shape(None);
        assert!(
            min_x(&rtl, 'w') < min_x(&rtl, 'ש'),
            "auto (RTL) base: runs must swap"
        );
        assert_eq!(TextShaper::paragraph_base_level("Hello שלום"), Level::ltr());
        assert_eq!(TextShaper::paragraph_base_level("שלום world"), Level::rtl());
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
    fn test_scalar_fallback_is_one_glyph_per_scalar() {
        // This exercises the deliberately simple `shape` fallback, not the
        // normal HarfRust path. It remains one glyph per Unicode scalar.
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
