use super::super::font::FontManager;
use super::super::shaper::{ShapingFont, TextShaper};
use super::lines::{
    drawing_unit_scale, segment_drawing_mode, DrawingLayout, LayoutBlock, LayoutFace, LayoutItem,
};
use super::state::LineGlobalKeep;
use super::{Compositor, ResolvedStyle};
use crate::types::override_tag::{OverrideTag, TextSegment};
use crate::types::{Event, Style};
use ab_glyph::{Font, FontArc};

impl Compositor {
    /// Convert an alignment anchor into the top-left text origin.
    pub(super) fn anchor_to_origin(
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
    pub(super) fn resolve_segment_style(
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
                segment_resolved.kerning = resolved.kerning;
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
    pub(super) fn layout_segments(
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

        // Event paragraph direction (drawings excluded: their command
        // letters would poison first-strong detection). Soft-wrapped
        // lines inherit it instead of re-detecting from their own
        // first strong character (libass reference: mixed-bidi).
        let mut drawing_mode = 0;
        let mut para_text = String::new();
        for segment in segments {
            drawing_mode = segment_drawing_mode(&segment.tags, drawing_mode);
            if drawing_mode == 0 {
                para_text.push_str(&segment.text);
            }
        }
        let event_base_level = TextShaper::paragraph_base_level(&para_text);

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
                        let (ft_asc, ft_desc, ft_height) = font_manager.ft_metrics(id).unwrap_or((
                            face.ascent_unscaled(),
                            face.descent_unscaled(),
                            face.height_unscaled().max(1.0),
                        ));
                        opentype_fonts.push(ShapingFont {
                            id,
                            raster: face,
                            data,
                            face_index,
                            ft_asc,
                            ft_desc,
                            ft_height,
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
                    seg_resolved.kerning,
                    Some(event_base_level),
                )
            };
            // Per-segment drawing state (mixed drawing/text supported).
            let mode = segment_drawing_mode(&segment.tags, resolved.drawing_mode);
            let drawing = if !skipped && mode > 0 {
                let unit = drawing_unit_scale(scale_x, scale_y, mode);
                super::super::drawing::DrawingParser::measure(&segment.text).map(
                    |(min_x, _, w, h)| {
                        DrawingLayout {
                            mode,
                            min_x: min_x * unit,
                            width: w * unit,
                            height: h * unit,
                            // libass `get_outline_glyph`: a drawing glyph
                            // takes `desc = pbo` and `asc = (y_max -
                            // y_min) - pbo` in drawing units, scaled to
                            // video pixels. Negative and oversized pbo
                            // values move the drawing outside its nominal
                            // ink box; the line pass clamps the descent
                            // contribution at zero like libass `max_desc`.
                            baseline: (h - seg_resolved.drawing_baseline_offset) * unit,
                        }
                    },
                )
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
        // libass `measure_text` tracks max descent beside max ascent
        // (`max_desc` from 0, so negative drawing descents never
        // shrink the line). Text-only lines keep the legacy max(h)
        // accumulation bit-for-bit (equal to max_asc + max_desc for
        // same-metric runs); lines with drawings extend to max_asc +
        // max_desc so positive `\pbo` grows the box downward.
        let mut line_desc = 0.0_f64;
        let mut line_has_drawing = false;
        let mut first_line = true;
        let mut any_content = false;

        for (item, segment) in items.iter().zip(segments.iter()) {
            if item.skipped {
                continue;
            }
            any_content = true;
            let (w, h, b) = match &item.drawing {
                Some(d) => {
                    line_has_drawing = true;
                    (d.width, d.height, d.baseline)
                }
                None => (item.shaped.width, item.shaped.height, item.shaped.baseline),
            };
            line_width += w;
            line_height = line_height.max(h);
            line_baseline = line_baseline.max(b);
            line_desc = line_desc.max(h - b);
            if segment.text.ends_with('\n') {
                block_width = block_width.max(line_width);
                // Drawing lines close at max_asc + max_desc (libass
                // `measure_text`); text-only lines keep max(h).
                let close_h = if line_has_drawing {
                    line_height.max(line_baseline + line_desc)
                } else {
                    line_height
                };
                block_height += close_h;
                if first_line {
                    baseline = line_baseline;
                    first_line = false;
                }
                line_width = 0.0;
                line_height = 0.0;
                line_baseline = 0.0;
                line_desc = 0.0;
                line_has_drawing = false;
            }
        }
        if line_width > 0.0 || line_height > 0.0 || !any_content {
            block_width = block_width.max(line_width);
            let close_h = if line_has_drawing {
                line_height.max(line_baseline + line_desc)
            } else {
                line_height
            };
            block_height += close_h;
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
}
