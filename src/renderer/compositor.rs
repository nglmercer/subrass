use super::buffer::{effective_shear, finite_to_i32, RenderBuffer, MAX_GLYPH_BITMAP_PIXELS};
use super::effects;
use super::font::FontManager;
use super::glyph_cache::GlyphCache;
use super::shaper::{ShapingFont, TextShaper};
use crate::types::override_tag::parse_text_segments_with_wrap;
use crate::types::{Event, EventType, LegacyEffect, Style};
use crate::utils::Matrix3x3;
use ab_glyph::{Font, FontArc};
use std::borrow::Cow;
#[path = "state.rs"]
mod state;
pub use self::state::{ComplexFade, MoveData, ResolvedStyle, VectorClip};
#[path = "layout/wrapping.rs"]
mod wrapping;
use self::wrapping::{wrap_event_text, wrap_event_text_with_measure};
#[path = "layout/lines.rs"]
mod lines;
use self::lines::{
    accumulate_fay_shear, deco_bar_rows, drawing_unit_scale, event_line_boxes, event_line_widths,
    line_align_inset, paint_deco_bar,
};
#[path = "karaoke.rs"]
mod karaoke;
use self::karaoke::{
    build_karaoke_runs, complex_fade_opacity, karaoke_outline_suppressed, paint_glyph_fill,
    BufferedSweepGlyph, GlyphGeom, KaraokeKind, SweepState, MAX_SWEEP_BUFFER_BYTES,
};
#[cfg(test)]
use self::karaoke::{KaraokeBuild, KaraokeRun};
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
                let (ft_asc, ft_desc, ft_height) = font_manager.ft_metrics(id).unwrap_or((
                    raster.ascent_unscaled(),
                    raster.descent_unscaled(),
                    raster.height_unscaled().max(1.0),
                ));
                opentype_measure_fonts.push(ShapingFont {
                    id,
                    raster,
                    data,
                    face_index,
                    ft_asc,
                    ft_desc,
                    ft_height,
                });
            }
        }

        let scale_x = video_width as f64 / play_res_x as f64;
        let scale_y = video_height as f64 / play_res_y as f64;
        // libass `ass_layout_res` + `init_font_scale`: the blur and
        // unscaled-border denominators come from LayoutRes, but only
        // when BOTH axes are set. subrass has square pixels and renders
        // straight into the video frame, so unset LayoutRes maps to the
        // video size (the libass storage-size role): blur radii and
        // unscaled borders/shadows are then 1:1 in video pixels, which
        // is also the documented VSFilter/legacy-libass contract for
        // `ScaledBorderAndShadow: no`.
        let (layout_res_x, layout_res_y) = if resolved.layout_res_x > 0 && resolved.layout_res_y > 0
        {
            (resolved.layout_res_x, resolved.layout_res_y)
        } else {
            (video_width.max(1), video_height.max(1))
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
        let wrap_input = match event.source_text_bytes.as_deref() {
            Some(bytes) => TextShaper::decode_ass_bytes(bytes, resolved.font_encoding),
            None => TextShaper::decode_font_encoding(&event.text, resolved.font_encoding),
        };
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
                    resolved.kerning,
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
            paint::decorations::paint_opaque_boxes(
                buffer,
                &rects,
                shadow_active.then_some((shadow_ox, shadow_oy)),
                pad_x,
                pad_y,
                shadow_fill,
                fill,
            );
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
                // libass `get_outline_glyph` hangs the drawing origin
                // one ascent above the pen (`offset.y = -asc` with
                // `asc = height - pbo`); `drawing.baseline` carries
                // that ascent, so positive `\pbo` sinks the drawing
                // toward the baseline and negative `\pbo` lifts it.
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
                // Outline + shadow (libass BorderStyle 1 draws both
                // under the fill): painted once for the whole drawing
                // — karaoke sweeps split only the fill, exactly like
                // the text sweep path whose outline/shadow never split.
                let (res_x, res_y) = if segment_resolved.scaled_border_and_shadow {
                    (scale_x, scale_y)
                } else {
                    (blur_scale_x, blur_scale_y)
                };
                let draw_outline = if segment_resolved.border_style == 1
                    && (segment_resolved.outline_x > 0.0 || segment_resolved.outline_y > 0.0)
                {
                    let oc = segment_resolved.outline_color.to_ass_components();
                    Some((
                        [
                            oc[0],
                            oc[1],
                            oc[2],
                            (f64::from(segment_resolved.outline_color.opacity()) * alpha_mult)
                                as u8,
                        ],
                        segment_resolved.outline_x * res_x * segment_resolved.scale_x / 100.0,
                        segment_resolved.outline_y * res_y * segment_resolved.scale_y / 100.0,
                    ))
                } else {
                    None
                };
                let draw_shadow = if segment_resolved.shadow_x != 0.0
                    || segment_resolved.shadow_y != 0.0
                {
                    let sc = segment_resolved.shadow_color.to_ass_components();
                    Some((
                        [
                            sc[0],
                            sc[1],
                            sc[2],
                            (f64::from(segment_resolved.shadow_color.opacity()) * alpha_mult) as u8,
                        ],
                        segment_resolved.shadow_x * res_x * segment_resolved.scale_x / 100.0,
                        segment_resolved.shadow_y * res_y * segment_resolved.scale_y / 100.0,
                    ))
                } else {
                    None
                };
                super::drawing::DrawingParser::render_drawing_effects(
                    buffer,
                    &segment.text,
                    draw_x,
                    draw_y,
                    unit,
                    draw_outline,
                    draw_shadow,
                );
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

            // Pre-compute effect parameters (libass `init_font_scale`):
            // with ScaledBorderAndShadow, borders/shadows scale with
            // the video/PlayRes ratio; otherwise with the blur scale
            // (video/LayoutRes, 1:1 when LayoutRes is unset).
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
                    font_manager.px_ratio(face.id),
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
                        m.scale_height > 0.0
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
                // The ascender is FreeType-basis (OS/2 Win value and
                // divisor), matching the shaped baseline.
                let (ft_asc, _, ft_height) = font_manager.ft_metrics(face.id).unwrap_or((
                    face_font.ascent_unscaled(),
                    face_font.descent_unscaled(),
                    face_font.height_unscaled().max(1.0),
                ));
                let ascent = f64::from(ft_asc) / f64::from(ft_height.max(1.0)) * segment_font_size;
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

        paint::clipping::apply_event_clips(
            buffer,
            resolved,
            scale_x,
            scale_y,
            blur_scale_x,
            blur_scale_y,
        );

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
