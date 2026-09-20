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

        if paint::event::needs_isolation(&frame_resolved, &event.effect) {
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

        let Some((alpha_mult, alpha)) =
            paint::event::event_alpha(resolved, time_ms, start_ms, end_ms)
        else {
            return;
        };
        let prepared = Self::prepare_event(
            resolved,
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

        paint::event::render_prepared(
            self,
            buffer,
            resolved,
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
