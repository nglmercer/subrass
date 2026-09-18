use ab_glyph::{Font, FontArc, GlyphId, PxScale, ScaleFont};

use crate::types::Color;

/// A glyph ready for rasterization
#[derive(Debug, Clone)]
pub struct ShapedGlyph {
    pub glyph_id: GlyphId,
    /// Stable [`FontManager`](super::font::FontManager) id of the face
    /// that provided this glyph (primary or fallback).
    pub font_id: usize,
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
    /// Characters missing from every shaping font (rendered as the
    /// primary face's .notdef). Zero when the chain covers the text.
    pub missing_glyphs: u32,
}

/// Pick the shaping font for one character: the first font whose glyph
/// id is nonzero (`.notdef` is id 0). When every font misses, index 0
/// (the primary face's .notdef) wins so rendering stays defined.
fn pick_font(glyph_ids: &[GlyphId]) -> usize {
    glyph_ids.iter().position(|g| g.0 != 0).unwrap_or(0)
}

/// Shaping policy: one cluster per Unicode scalar value — no ligatures,
/// kerning, mark reordering, or complex-script shaping. Advances come
/// from the face that provides each glyph; `\fsp` spacing is added
/// between glyphs (the trailing unit is stripped per line). Per-glyph
/// fallback runs are tracked via [`ShapedGlyph::font_id`]; wrap and
/// karaoke measurement use the same fallback-aware widths as shaping.
pub struct TextShaper;

impl TextShaper {
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
            missing_glyphs: 0,
        };
        // Raw degenerate sizes (<= 0, NaN) shape to nothing instead
        // of feeding ab_glyph invalid scales. (At the override level,
        // `\fs0` and non-positive results reset to the style size per
        // libass; this guard covers direct callers and style data.)
        if !font_size.is_finite() || font_size <= 0.0 {
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

        for ch in text.chars() {
            if ch == '\n' || ch == '\r' {
                // Line break: close this line (stripping its own trailing
                // spacing unit), advance y, reset x.
                let line_width = if x > 0.0 { x - spacing * scale_x } else { 0.0 };
                max_x = max_x.max(line_width);
                y += line_height;
                x = 0.0;
                continue;
            }

            // Cascade: first face with a real glyph wins; advances come
            // from the selected face.
            let ids: Vec<GlyphId> = fonts.iter().map(|(_, f)| f.glyph_id(ch)).collect();
            let pick = pick_font(&ids);
            let (font_id, glyph_id) = (fonts[pick].0, ids[pick]);
            if glyph_id.0 == 0 {
                missing_glyphs += 1;
            }
            let advance = scaled[pick].h_advance(glyph_id) as f64;

            glyphs.push(ShapedGlyph {
                glyph_id,
                font_id,
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
            missing_glyphs,
        }
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
        if !font_size.is_finite() || font_size <= 0.0 {
            return 0.0;
        }
        if fonts.is_empty() {
            return 0.0;
        }
        let scale = PxScale::from(font_size as f32);
        let scaled: Vec<_> = fonts.iter().map(|f| f.as_scaled(scale)).collect();
        let mut widest = 0.0_f64;
        for line in text.split(['\n', '\r']) {
            let mut width = 0.0_f64;
            let mut first = true;
            for ch in line.chars() {
                let ids: Vec<GlyphId> = fonts.iter().map(|f| f.glyph_id(ch)).collect();
                let pick = pick_font(&ids);
                if !first {
                    width += spacing;
                }
                width += scaled[pick].h_advance(ids[pick]) as f64;
                first = false;
            }
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
    fn test_pick_font_cascade_order() {
        // First nonzero glyph id wins; all-missing falls back to 0.
        assert_eq!(pick_font(&[GlyphId(3), GlyphId(0)]), 0);
        assert_eq!(pick_font(&[GlyphId(0), GlyphId(7)]), 1);
        assert_eq!(pick_font(&[GlyphId(0), GlyphId(0)]), 0);
        assert_eq!(pick_font(&[GlyphId(0), GlyphId(0), GlyphId(9)]), 2);
        assert_eq!(pick_font(&[]), 0);
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
