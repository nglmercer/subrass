use super::buffer::RenderBuffer;
use super::font::FontManager;
use super::glyph_cache::GlyphCache;
#[cfg(test)]
use super::shaper::TextShaper;
use crate::types::{Event, EventType, Style};
#[cfg(test)]
use ab_glyph::FontArc;
#[path = "state.rs"]
mod state;
pub use self::state::{ComplexFade, MoveData, ResolvedStyle, VectorClip};
#[path = "layout/wrapping.rs"]
mod wrapping;
#[cfg(test)]
use self::wrapping::wrap_event_text;
#[path = "karaoke.rs"]
mod karaoke;
#[path = "layout/lines.rs"]
mod lines;
#[cfg(test)]
use self::karaoke::{
    build_karaoke_runs, complex_fade_opacity, karaoke_outline_suppressed, KaraokeKind,
};
#[cfg(test)]
use self::karaoke::{KaraokeBuild, KaraokeRun};
#[cfg(test)]
use self::lines::{accumulate_fay_shear, drawing_unit_scale};
#[path = "compositor_layout.rs"]
mod layout_resolution;
#[path = "paint/mod.rs"]
mod paint;
#[path = "compositor_style.rs"]
mod style_resolution;

#[cfg(test)]
use self::lines::scale_clip_rect;
#[cfg(test)]
use self::lines::segment_drawing_mode;
#[cfg(test)]
use crate::types::color::Color;
#[cfg(test)]
use crate::types::override_tag::parse_text_segments;
#[cfg(test)]
use crate::types::OverrideTag;
#[path = "transforms.rs"]
mod transforms;

/// Compositor - composites resolved subtitle events into a buffer
pub struct Compositor {
    glyph_cache: GlyphCache,
}

/// A frame's measured event. Placement mutates geometry before any pixels paint.
pub(crate) struct MeasuredEvent<'a> {
    resolved: ResolvedStyle,
    prepared: layout_resolution::PreparedEvent<'a>,
    pub(crate) layer: i32,
    pub(crate) collision: bool,
}

/// Only geometry survives the measurement pass. Retaining every shaped
/// event at once would multiply peak memory by the number of active events.
pub(crate) struct EventPlacement {
    pub(crate) index: usize,
    layer: i32,
    collision: bool,
    rect: (f64, f64, f64, f64),
    up: bool,
    pub(crate) shift: f64,
}

impl MeasuredEvent<'_> {
    pub(crate) fn debug_snapshot(
        &self,
        event_index: usize,
        collision_shift: f64,
    ) -> super::debug::EventDebug {
        use super::debug::{EventDebug, RunDebug, MAX_DEBUG_RUNS};
        let p = &self.prepared;
        let rect = self.rect();
        EventDebug {
            event_index,
            layout_bounds: [rect.0, rect.1, rect.2, rect.3],
            collision_eligible: self.collision,
            collision_shift,
            alignment: self.resolved.alignment,
            origin: [p.org_x, p.org_y],
            opacity: p.alpha_mult,
            run_count: p.layout.items.len(),
            runs: p
                .layout
                .items
                .iter()
                .take(MAX_DEBUG_RUNS)
                .map(|item| {
                    let r = &item.resolved;
                    RunDebug {
                        font_name: {
                            let mut name: String = r.font_name.chars().take(256).collect();
                            if name.len() < r.font_name.len() {
                                name.push('…');
                            }
                            name
                        },
                        font_size: r.font_size,
                        fill: r.color.to_straight_rgba(),
                        outline_color: r.outline_color.to_straight_rgba(),
                        scale: [r.scale_x, r.scale_y],
                        rotation: [r.rotation_x, r.rotation_y, r.angle],
                        shear: [r.shear_x, r.shear_y],
                        border: [r.outline_x, r.outline_y],
                        shadow: [r.shadow_x, r.shadow_y],
                        blur: r.blur,
                        edge_blur: r.edge_blur,
                        drawing_mode: r.drawing_mode,
                    }
                })
                .collect(),
        }
    }

    pub(crate) fn placement(&self, index: usize) -> EventPlacement {
        EventPlacement {
            index,
            layer: self.layer,
            collision: self.collision,
            rect: self.rect(),
            up: (1..=3).contains(&self.resolved.alignment),
            shift: 0.0,
        }
    }
    pub(crate) fn shift_y(&mut self, shift: f64) {
        self.prepared.base_y += shift;
        self.prepared.org_y += shift;
    }

    fn rect(&self) -> (f64, f64, f64, f64) {
        let p = &self.prepared;
        let mut bx = 0.0_f64;
        let mut by = 0.0_f64;
        for item in &p.layout.items {
            let r = &item.resolved;
            let (sx, sy) = if r.scaled_border_and_shadow {
                (p.scale_x, p.scale_y)
            } else {
                (p.blur_scale_x, p.blur_scale_y)
            };
            bx = bx.max((r.outline_x * sx).clamp(0.0, super::limits::MAX_OUTLINE_RADIUS));
            by = by.max((r.outline_y * sy).clamp(0.0, super::limits::MAX_OUTLINE_RADIUS));
        }
        (
            p.base_x - bx,
            p.base_y - p.layout.baseline - by,
            p.base_x + p.layout.width + bx,
            p.base_y - p.layout.baseline + p.layout.height + by,
        )
    }
}

/// libass `fit_rect`: only intersecting horizontal extents interact, layers
/// are independent, bottom alignment moves up and top/middle move down.
/// Placement is derived from the frame, making random seeking reproducible.
pub(crate) fn place_collisions(events: &mut [EventPlacement]) {
    let mut used: Vec<(f64, f64, f64, f64)> = Vec::new();
    let mut layer = None;
    for event in events {
        if layer != Some(event.layer) {
            used.clear();
            layer = Some(event.layer);
        }
        if !event.collision {
            continue;
        }
        let r = event.rect;
        if ![r.0, r.1, r.2, r.3].iter().all(|v| v.is_finite()) || r.2 <= r.0 || r.3 <= r.1 {
            continue;
        }
        let up = event.up;
        let mut shift = 0.0;
        let intersects = |f: &(f64, f64, f64, f64), shift: f64| {
            r.3 + shift > f.1 && r.1 + shift < f.3 && r.2 > f.0 && r.0 < f.2
        };
        if up {
            for f in used.iter().rev() {
                if intersects(f, shift) {
                    shift = f.1 - r.3;
                }
            }
        } else {
            for f in &used {
                if intersects(f, shift) {
                    shift = f.3 - r.1;
                }
            }
        }
        event.shift = shift;
        used.push((r.0, r.1 + shift, r.2, r.3 + shift));
        used.sort_by(|a, b| a.1.total_cmp(&b.1));
    }
}

impl Compositor {
    pub fn new() -> Self {
        Self {
            glyph_cache: GlyphCache::new(4096),
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
        if let Some(measured) = Self::measure_event(
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
        ) {
            self.paint_event(buffer, font_manager, measured, video_height, play_res_y);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn measure_event<'a>(
        event: &Event,
        resolved: &ResolvedStyle,
        font_manager: &'a FontManager,
        time_ms: u64,
        play_res_x: u32,
        play_res_y: u32,
        video_width: u32,
        video_height: u32,
        script_wrap_style: i32,
        styles: &[Style],
    ) -> Option<MeasuredEvent<'a>> {
        let start_ms = event.start.to_millis();
        let end_ms = event.end.to_millis();
        if event.event_type == EventType::Comment || time_ms < start_ms || time_ms >= end_ms {
            return None;
        }
        let frame = Self::resolve_event_globals_at_time(
            resolved, event, time_ms, start_ms, end_ms, play_res_x, play_res_y,
        );
        // Fully faded dialogue still reserves a collision rectangle in libass.
        let (alpha_mult, alpha) =
            paint::event::event_alpha(&frame, time_ms, start_ms, end_ms).unwrap_or((0.0, 0));
        let prepared = Self::prepare_event(
            &frame,
            event,
            font_manager,
            alpha_mult,
            alpha,
            time_ms,
            start_ms,
            end_ms,
            play_res_x,
            play_res_y,
            video_width,
            video_height,
            script_wrap_style,
            styles,
        );
        // libass disables collisions for explicit positions/origins, animation
        // tags (even before their interval), and legacy motion effects.
        let collision = frame.position.is_none()
            && frame.move_data.is_none()
            && frame.origin.is_none()
            && prepared.legacy_effect.is_none()
            && !event
                .parsed_tags
                .iter()
                .any(|t| matches!(t, crate::types::OverrideTag::Transform { .. }));
        Some(MeasuredEvent {
            resolved: frame,
            prepared,
            layer: event.layer,
            collision,
        })
    }

    pub(crate) fn paint_event(
        &mut self,
        buffer: &mut RenderBuffer,
        font_manager: &FontManager,
        measured: MeasuredEvent<'_>,
        video_height: u32,
        play_res_y: u32,
    ) {
        let MeasuredEvent {
            resolved, prepared, ..
        } = measured;
        if prepared.alpha == 0 {
            return;
        }
        if paint::event::needs_isolation(&resolved, prepared.legacy_effect) {
            if let Ok(mut isolated) = RenderBuffer::new(buffer.width, buffer.height) {
                paint::event::render_prepared(
                    self,
                    &mut isolated,
                    &resolved,
                    font_manager,
                    prepared,
                    video_height,
                    play_res_y,
                );
                buffer.blend_buffer(&isolated);
                return;
            }
        }
        paint::event::render_prepared(
            self,
            buffer,
            &resolved,
            font_manager,
            prepared,
            video_height,
            play_res_y,
        );
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
#[path = "compositor_tests.rs"]
mod tests;
