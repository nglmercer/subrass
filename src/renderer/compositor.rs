use crate::types::{Event, EventType, OverrideTag, Style};
use crate::types::color::Color;
use super::buffer::RenderBuffer;
use super::glyph_cache::GlyphCache;
use super::font::FontManager;
use super::shaper::TextShaper;
use super::effects;

/// Resolved style with all overrides applied
#[derive(Debug, Clone)]
pub struct ResolvedStyle {
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

    /// Resolve an event's style with all override tags applied
    pub fn resolve_style(base_style: &Style, event: &Event) -> ResolvedStyle {
        let mut resolved = ResolvedStyle {
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

        // Apply override tags
        for tag in &event.parsed_tags {
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
                        x1: *x1, y1: *y1, x2: *x2, y2: *y2,
                        t1: 0, t2: 0,
                    });
                }
                OverrideTag::MoveWithTiming(x1, y1, x2, y2, t1, t2) => {
                    resolved.move_data = Some(MoveData {
                        x1: *x1, y1: *y1, x2: *x2, y2: *y2,
                        t1: *t1, t2: *t2,
                    });
                }
                OverrideTag::Origin(x, y) => resolved.origin = Some((*x, *y)),
                OverrideTag::Alignment(a) => resolved.alignment = *a,
                OverrideTag::ScaleX(s) => resolved.scale_x = *s,
                OverrideTag::ScaleY(s) => resolved.scale_y = *s,
                OverrideTag::RotationZ(r) => resolved.angle = *r,
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

        resolved
    }

    /// Calculate event position based on alignment, margins, and resolution
    /// Returns the BASELINE position for the text
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
        // Scale factors from play resolution to video resolution
        let scale_x = video_width as f64 / play_res_x as f64;
        let scale_y = video_height as f64 / play_res_y as f64;

        // If position is explicitly set, use it as baseline position
        if let Some((px, py)) = resolved.position {
            return (px * scale_x, py * scale_y);
        }

        // Calculate descent from baseline and height
        // height = ascent - descent, so descent = ascent - height = baseline - height
        let descent = baseline - text_height;

        // Calculate position from alignment (numpad layout)
        // 7 8 9  (top-left, top-center, top-right)
        // 4 5 6  (mid-left, mid-center, mid-right)
        // 1 2 3  (bottom-left, bottom-center, bottom-right)
        let alignment = resolved.alignment;
        let margin_l = resolved.margin_l as f64 * scale_x;
        let margin_r = resolved.margin_r as f64 * scale_x;
        let margin_v = resolved.margin_v as f64 * scale_y;

        // X position (LEFT EDGE of text)
        // Glyphs are rendered from glyph.x = 0 at the left edge
        let x = match alignment {
            1 | 4 | 7 => margin_l,                              // Left: start at left margin
            2 | 5 | 8 => (video_width as f64 - text_width) / 2.0, // Center: center the text
            3 | 6 | 9 => video_width as f64 - margin_r - text_width, // Right: end at right margin
            _ => (video_width as f64 - text_width) / 2.0,
        };

        // Y position (BASELINE of text)
        // For top alignment: text top at margin_v, baseline at margin_v + ascent
        // For middle alignment: baseline at center
        // For bottom alignment: text bottom at video_height - margin_v, baseline at (video_height - margin_v) - descent
        // Note: descent is negative, so (video_height - margin_v) - descent = (video_height - margin_v) + |descent|
        let y = match alignment {
            7 | 8 | 9 => margin_v + baseline,                          // Top: baseline at margin + ascent
            4 | 5 | 6 => video_height as f64 / 2.0,                    // Middle: baseline at center
            1 | 2 | 3 => (video_height as f64 - margin_v) - descent,   // Bottom: baseline above margin by |descent|
            _ => (video_height as f64 - margin_v) - descent,
        };

        (x, y)
    }

    /// Composite a single event into the buffer
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
    ) {
        if event.event_type == EventType::Comment {
            return;
        }

        // Check fade
        let start_ms = event.start.to_millis();
        let end_ms = event.end.to_millis();

        if time_ms < start_ms || time_ms >= end_ms {
            return;
        }

        let mut alpha_mult = 1.0_f64;

        // Apply simple fade
        if resolved.fade_in > 0 || resolved.fade_out > 0 {
            let fade_alpha = effects::calculate_fade_alpha(
                time_ms, start_ms, end_ms,
                resolved.fade_in, resolved.fade_out,
            );
            alpha_mult = fade_alpha as f64 / 255.0;
        }

        // Apply complex fade
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

        // Get clean text (without override tags)
        let clean_text = self.extract_clean_text(&event.text);

        if clean_text.is_empty() {
            return;
        }

        // Find font
        let font = font_manager.find_font(
            &resolved.font_name,
            resolved.bold,
            resolved.italic,
        );

        // Shape text
        let font_size = resolved.font_size * (video_height as f64 / play_res_y as f64);
        let _scaled_font_size = font_size * resolved.scale_x / 100.0;

        let shaped = TextShaper::shape(
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

        // Calculate position (returns baseline position)
        let (mut pos_x, mut pos_y) = Self::calculate_position(
            &resolved,
            shaped.width,
            shaped.height,
            shaped.baseline,
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
                // Simple move over full duration
                if duration > 0 {
                    (elapsed as f64 / duration as f64).min(1.0)
                } else {
                    0.0
                }
            } else {
                // Move with timing
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

            pos_x = move_data.x1 + (move_data.x2 - move_data.x1) * t;
            pos_y = move_data.y1 + (move_data.y2 - move_data.y1) * t;

            // Scale to video resolution
            let scale_x = video_width as f64 / play_res_x as f64;
            let scale_y = video_height as f64 / play_res_y as f64;
            pos_x *= scale_x;
            pos_y *= scale_y;
        }

        // Render each glyph
        let scale_x = video_width as f64 / play_res_x as f64;
        let scale_y = video_height as f64 / play_res_y as f64;

        // Calculate alpha
        let alpha = (alpha_mult * 255.0) as u8;

        // Apply outline (border style 1)
        if resolved.border_style == 1 && resolved.outline > 0.0 {
            let outline_scale = resolved.outline * (scale_x + scale_y) / 2.0;
            let outline_color = resolved.outline_color.to_rgba();
            // ASS alpha: 0 = opaque, 255 = transparent -> invert
            let outline_alpha = 255 - outline_color[3];

            for glyph in &shaped.glyphs {
                let cached = self.glyph_cache.get_or_rasterize(
                    font, glyph.glyph_id, font_size, glyph.bold, glyph.italic,
                );

                if cached.width > 0 && cached.height > 0 {
                    let gx = (pos_x + glyph.x + cached.bearing_x as f64) as i32;
                    let gy = (pos_y + glyph.y + cached.bearing_y as f64) as i32;

                    effects::apply_outline(
                        buffer,
                        &cached.bitmap,
                        cached.width,
                        cached.height,
                        gx,
                        gy,
                        outline_scale,
                        [
                            outline_color[0],
                            outline_color[1],
                            outline_color[2],
                            (outline_alpha as f64 * alpha_mult) as u8,
                        ],
                    );
                }
            }
        }

        // Apply shadow
        if resolved.shadow > 0.0 {
            let shadow_offset_x = resolved.shadow * scale_x;
            let shadow_offset_y = resolved.shadow * scale_y;
            let shadow_color = resolved.shadow_color.to_rgba();
            // ASS alpha: 0 = opaque, 255 = transparent -> invert
            let shadow_alpha = 255 - shadow_color[3];

            for glyph in &shaped.glyphs {
                let cached = self.glyph_cache.get_or_rasterize(
                    font, glyph.glyph_id, font_size, glyph.bold, glyph.italic,
                );

                if cached.width > 0 && cached.height > 0 {
                    let gx = (pos_x + glyph.x + cached.bearing_x as f64) as i32;
                    let gy = (pos_y + glyph.y + cached.bearing_y as f64) as i32;

                    effects::apply_shadow(
                        buffer,
                        &cached.bitmap,
                        cached.width,
                        cached.height,
                        gx,
                        gy,
                        shadow_offset_x,
                        shadow_offset_y,
                        [
                            shadow_color[0],
                            shadow_color[1],
                            shadow_color[2],
                            (shadow_alpha as f64 * alpha_mult) as u8,
                        ],
                    );
                }
            }
        }

        // Render main text
        for glyph in &shaped.glyphs {
            let cached = self.glyph_cache.get_or_rasterize(
                font, glyph.glyph_id, font_size, glyph.bold, glyph.italic,
            );

            if cached.width > 0 && cached.height > 0 {
                // glyph.x = horizontal position, glyph.y = vertical offset for multi-line
                let gx = (pos_x + glyph.x + cached.bearing_x as f64) as i32;
                let gy = (pos_y + glyph.y + cached.bearing_y as f64) as i32;

                let color = glyph.color.to_rgba();
                // ASS alpha: 0 = opaque, 255 = transparent -> invert for standard alpha
                let color_alpha = 255 - color[3];

                // Render glyph pixels
                for py in 0..cached.height {
                    for px in 0..cached.width {
                        let coverage = cached.bitmap[(py * cached.width + px) as usize];
                        if coverage > 0 {
                            // Combine coverage, color alpha, and fade alpha
                            let a = ((coverage as u32 * color_alpha as u32 / 255) as u32 * alpha as u32 / 255) as u8;
                            buffer.blend_pixel(
                                (gx + px as i32) as u32,
                                (gy + py as i32) as u32,
                                color[0],
                                color[1],
                                color[2],
                                a,
                            );
                        }
                    }
                }
            }
        }

        // Apply underline
        if resolved.underline {
            let line_width = 2;
            let color = resolved.color.to_rgba();
            let color_alpha = 255 - color[3];

            for glyph in &shaped.glyphs {
                let gx = (pos_x + glyph.x) as i32;
                let gy = (pos_y + glyph.y + shaped.baseline * 0.9) as i32;
                let gw = glyph.advance as i32;

                buffer.fill_rect(
                    gx,
                    gy,
                    gw,
                    line_width,
                    color[0],
                    color[1],
                    color[2],
                    (color_alpha as f64 * alpha_mult) as u8,
                );
            }
        }

        // Apply strikeout
        if resolved.strike_out {
            let line_width = 2;
            let color = resolved.color.to_rgba();
            let color_alpha = 255 - color[3];

            for glyph in &shaped.glyphs {
                let gx = (pos_x + glyph.x) as i32;
                let gy = (pos_y + glyph.y + shaped.baseline * 0.35) as i32;
                let gw = glyph.advance as i32;

                buffer.fill_rect(
                    gx,
                    gy,
                    gw,
                    line_width,
                    color[0],
                    color[1],
                    color[2],
                    (color_alpha as f64 * alpha_mult) as u8,
                );
            }
        }

        // Apply clipping
        if let Some(clip_rect) = resolved.clip {
            let scaled_clip = (
                (clip_rect.0 as f64 * scale_x) as i32,
                (clip_rect.1 as f64 * scale_y) as i32,
                (clip_rect.2 as f64 * scale_x) as i32,
                (clip_rect.3 as f64 * scale_y) as i32,
            );
            effects::apply_clip(buffer, scaled_clip);
        }

        // Apply inverse clipping
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
    fn extract_clean_text(&self, text: &str) -> String {
        let mut result = String::new();
        let mut in_tag = false;
        let mut chars = text.chars().peekable();

        while let Some(ch) = chars.next() {
            match ch {
                '{' => in_tag = true,
                '}' => in_tag = false,
                '\\' => {
                    // Check for \N, \n (line break) and \h (hard space) - works both inside and outside tags
                    if let Some(&next) = chars.peek() {
                        if next == 'N' || next == 'n' {
                            chars.next();
                            result.push('\n');
                        } else if next == 'h' {
                            chars.next();
                            result.push('\u{00A0}'); // Hard space
                        }
                        // Other backslash sequences inside tags are skipped
                    }
                }
                _ if !in_tag => result.push(ch),
                _ => {}
            }
        }

        result
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
