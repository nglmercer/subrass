use ab_glyph::{Font, FontArc, GlyphId, PxScale, ScaleFont};

use crate::types::Color;

/// A glyph ready for rasterization
#[derive(Debug, Clone)]
pub struct ShapedGlyph {
    pub glyph_id: GlyphId,
    pub font_index: usize,
    pub x: f64,
    pub y: f64,
    pub advance: f64,
    pub font_size: f64,
    pub color: Color,
    pub outline_color: Color,
    pub shadow_color: Color,
    pub bold: bool,
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
}

/// Simple text shaper - maps characters to glyphs and positions them
pub struct TextShaper;

impl TextShaper {
    /// Shape a text string into positioned glyphs (supports multi-line with \n)
    pub fn shape(
        text: &str,
        font: &FontArc,
        font_size: f64,
        _scale_font: f64,
        _scale_y: f64,
        bold: bool,
        italic: bool,
        spacing: f64,
        color: Color,
        outline_color: Color,
        shadow_color: Color,
        rotation: f64,
    ) -> ShapedLine {
        let scale = PxScale::from(font_size as f32);
        let scaled = font.as_scaled(scale);

        let mut glyphs = Vec::new();
        let mut x = 0.0_f64;
        let mut y = 0.0_f64;

        // Get font metrics
        let baseline = scaled.ascent() as f64;
        let line_height = scaled.height() as f64;
        let mut max_x = 0.0_f64;

        for ch in text.chars() {
            if ch == '\n' || ch == '\r' {
                // Line break: advance y, reset x
                max_x = max_x.max(x);
                y += line_height;
                x = 0.0;
                continue;
            }

            // Get glyph ID and advance
            let glyph_id = font.glyph_id(ch);
            let advance = scaled.h_advance(glyph_id) as f64;

            glyphs.push(ShapedGlyph {
                glyph_id,
                font_index: 0,
                x,
                y,
                advance,
                font_size,
                color,
                outline_color,
                shadow_color,
                bold,
                italic,
                scale_x: _scale_font,
                scale_y: _scale_y,
                rotation,
            });

            // Add advance + spacing to next character position
            x += advance + spacing;
        }

        max_x = max_x.max(x);

        ShapedLine {
            glyphs,
            width: max_x,
            height: y + line_height,
            baseline,
        }
    }

    /// Measure text width without creating glyphs
    pub fn measure_text(text: &str, font: &FontArc, font_size: f64, spacing: f64) -> f64 {
        let scale = PxScale::from(font_size as f32);
        let scaled = font.as_scaled(scale);
        let mut width = 0.0_f64;
        let chars: Vec<char> = text.chars().collect();

        for (i, ch) in chars.iter().enumerate() {
            if *ch == '\n' || *ch == '\r' {
                continue;
            }
            let glyph_id = font.glyph_id(*ch);
            width += scaled.h_advance(glyph_id) as f64;
            // Add spacing between characters (not after last)
            if i < chars.len() - 1 {
                width += spacing;
            }
        }

        width
    }

    /// Split text into lines based on max width
    pub fn wrap_text(text: &str, font: &FontArc, font_size: f64, max_width: f64, spacing: f64) -> Vec<String> {
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

    #[test]
    fn test_text_shaper_new() {
        let _ = TextShaper;
    }
}
