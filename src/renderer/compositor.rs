use super::buffer::RenderBuffer;
use super::effects;
use super::font::FontManager;
use super::glyph_cache::GlyphCache;
use super::shaper::TextShaper;
use crate::types::color::Color;
use crate::types::override_tag::parse_text_segments;
use crate::types::{Event, EventType, OverrideTag, Style};

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

    /// Apply \t transform animations to a resolved style
    fn apply_transforms(
        resolved: &mut ResolvedStyle,
        transforms: &[OverrideTag],
        time_ms: u64,
        start_ms: u64,
    ) {
        let elapsed = time_ms.saturating_sub(start_ms);

        for tag in transforms {
            if let OverrideTag::Transform {
                t1,
                t2,
                accel,
                tags,
            } = tag
            {
                // Check if current time is within the animation range
                if elapsed < *t1 || elapsed > *t2 {
                    continue;
                }

                // Calculate progress
                let duration = t2 - t1;
                let raw_progress = if duration > 0 {
                    (elapsed - *t1) as f64 / duration as f64
                } else {
                    1.0
                };

                let progress = Self::apply_accel(raw_progress, *accel);

                // Apply each target tag with interpolation
                for target in tags {
                    match target {
                        OverrideTag::Blur(b) => {
                            // Interpolate from current blur to target
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
                        OverrideTag::FontSizeMultiplier(m) => {
                            let from = 1.0;
                            let target = *m;
                            let mult = from + (target - from) * progress;
                            // Apply multiplier to base font size (stored in font_size)
                            // This is approximate; ideally we track the original size
                            resolved.font_size *= mult;
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
                        _ => {
                            // Unsupported animation target - apply directly
                        }
                    }
                }
            }
        }
    }

    /// Resolve an event's style with all override tags applied (no animation)
    fn resolve_base_style(base_style: &Style, tags: &[OverrideTag]) -> ResolvedStyle {
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
        let mut resolved = Self::resolve_base_style(base_style, &event.parsed_tags);

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
            let alignment = resolved.alignment;

            // X offset based on horizontal alignment
            let x = match alignment {
                1 | 4 | 7 => scaled_x,                    // Left: left edge at px
                2 | 5 | 8 => scaled_x - text_width / 2.0, // Center: center at px
                3 | 6 | 9 => scaled_x - text_width,       // Right: right edge at px
                _ => scaled_x - text_width / 2.0,
            };

            // Y offset based on vertical alignment (using baseline)
            let y = match alignment {
                7..=9 => scaled_y,                     // Top: top at py
                4..=6 => scaled_y - text_height / 2.0, // Middle: center at py
                1..=3 => scaled_y - text_height,       // Bottom: bottom at py
                _ => scaled_y - text_height / 2.0,
            };

            return (x, y);
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

    /// Composite a single event into the buffer using per-segment rendering
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

        // Parse text into segments for per-override rendering
        let segments = parse_text_segments(&event.text);

        // Check if this is drawing mode (check first segment for \p1)
        let is_drawing = segments
            .first()
            .map(|s| {
                resolved.drawing_mode > 0
                    || s.tags.iter().any(|t| matches!(t, OverrideTag::Drawing(1)))
            })
            .unwrap_or(false);

        // Find font
        let font = font_manager.find_font(&resolved.font_name, resolved.bold, resolved.italic);
        let font_size = resolved.font_size * (video_height as f64 / play_res_y as f64);

        let scale_x = video_width as f64 / play_res_x as f64;
        let scale_y = video_height as f64 / play_res_y as f64;

        // Calculate position (always need text measurement for alignment offsets)
        let (clean_text, _) = self.extract_clean_text(&event.text);
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

            base_x = move_data.x1 + (move_data.x2 - move_data.x1) * t;
            base_y = move_data.y1 + (move_data.y2 - move_data.y1) * t;
            base_x *= scale_x;
            base_y *= scale_y;
        }

        // Drawing mode: render vector paths
        if is_drawing {
            let avg_scale = (scale_x + scale_y) / 2.0;
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

        // Collect all transform tags from all segments for animation
        let all_transforms: Vec<OverrideTag> = segments
            .iter()
            .flat_map(|s| s.tags.iter().cloned())
            .filter(|t| t.is_animation())
            .collect();

        // Per-segment rendering
        let mut x_offset = 0.0_f64;
        let mut line_y_offset = 0.0_f64;

        for segment in &segments {
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

            // Apply \t animations
            if !all_transforms.is_empty() {
                Self::apply_transforms(&mut segment_resolved, &all_transforms, time_ms, start_ms);
            }

            // Shape this segment's text
            let segment_font = font_manager.find_font(
                &segment_resolved.font_name,
                segment_resolved.bold,
                segment_resolved.italic,
            );

            let shaped = TextShaper::shape(
                &segment.text,
                segment_font,
                font_size,
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
            let outline_scale = segment_resolved.outline * (scale_x + scale_y) / 2.0;
            let outline_color_rgba = segment_resolved.outline_color.to_rgba();
            let outline_alpha = (255 - outline_color_rgba[3] as u32) as u8;

            let shadow_active = segment_resolved.shadow > 0.0;
            let shadow_offset_x = segment_resolved.shadow * scale_x;
            let shadow_offset_y = segment_resolved.shadow * scale_y;
            let shadow_color_rgba = segment_resolved.shadow_color.to_rgba();
            let shadow_alpha = (255 - shadow_color_rgba[3] as u32) as u8;

            // Single pass over glyphs: cache lookup once, render outline + shadow + fill
            for glyph in &shaped.glyphs {
                let cached = self.glyph_cache.get_or_rasterize(
                    segment_font,
                    glyph.glyph_id,
                    font_size,
                    glyph.bold,
                    glyph.italic,
                );

                if cached.width == 0 || cached.height == 0 {
                    continue;
                }

                let gx = (base_x + x_offset + glyph.x + cached.bearing_x as f64) as i32;
                let gy = (base_y + line_y_offset + glyph.y + cached.bearing_y as f64) as i32;

                // Render outline
                if outline_active {
                    effects::apply_outline(
                        buffer,
                        &cached.bitmap,
                        cached.width,
                        cached.height,
                        gx,
                        gy,
                        outline_scale,
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
                        &cached.bitmap,
                        cached.width,
                        cached.height,
                        gx,
                        gy,
                        shadow_offset_x,
                        shadow_offset_y,
                        [
                            shadow_color_rgba[0],
                            shadow_color_rgba[1],
                            shadow_color_rgba[2],
                            (shadow_alpha as f64 * alpha_mult) as u8,
                        ],
                    );
                }

                // Render main text
                let color = glyph.color.to_rgba();
                let color_alpha = 255 - color[3];

                for py in 0..cached.height {
                    for px in 0..cached.width {
                        let coverage = cached.bitmap[(py * cached.width + px) as usize];
                        if coverage > 0 {
                            let a = ((coverage as u32 * color_alpha as u32 / 255) * alpha as u32
                                / 255) as u8;
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

                // Render underline
                if segment_resolved.underline {
                    let line_width = 2;
                    let color = segment_resolved.color.to_rgba();
                    let color_alpha = 255 - color[3];
                    let ul_gy = (gy as f64 + shaped.baseline * 0.9) as i32;
                    buffer.fill_rect(
                        gx,
                        ul_gy,
                        glyph.advance as i32,
                        line_width,
                        color[0],
                        color[1],
                        color[2],
                        (color_alpha as f64 * alpha_mult) as u8,
                    );
                }

                // Render strikeout
                if segment_resolved.strike_out {
                    let line_width = 2;
                    let color = segment_resolved.color.to_rgba();
                    let color_alpha = 255 - color[3];
                    let so_gy = (gy as f64 + shaped.baseline * 0.35) as i32;
                    buffer.fill_rect(
                        gx,
                        so_gy,
                        glyph.advance as i32,
                        line_width,
                        color[0],
                        color[1],
                        color[2],
                        (color_alpha as f64 * alpha_mult) as u8,
                    );
                }
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
