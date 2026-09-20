//! Event-level paint policy: alpha and isolation decisions.

use super::super::super::buffer::{finite_to_i32, RenderBuffer};
use super::super::super::effects;
use super::super::super::font::FontManager;
use super::super::karaoke::complex_fade_opacity;
use super::super::layout_resolution::PreparedEvent;
use super::super::lines::{event_line_boxes, line_align_inset};
use super::super::state::ResolvedStyle;
use super::super::Compositor;
use super::clipping;
use super::decorations;
use super::text;
use crate::types::LegacyEffect;

/// Resolve the event opacity after simple and complex fades. Returning
/// `None` lets orchestration skip a fully transparent event before layout.
pub(crate) fn event_alpha(
    resolved: &ResolvedStyle,
    time_ms: u64,
    start_ms: u64,
    end_ms: u64,
) -> Option<(f64, u8)> {
    let mut alpha_mult = 1.0_f64;
    if resolved.fade_in > 0 || resolved.fade_out > 0 {
        alpha_mult = f64::from(effects::calculate_fade_alpha(
            time_ms,
            start_ms,
            end_ms,
            resolved.fade_in,
            resolved.fade_out,
        )) / 255.0;
    }
    if let Some(fade) = &resolved.complex_fade {
        alpha_mult = complex_fade_opacity(fade, time_ms.saturating_sub(start_ms));
    }
    (alpha_mult > 0.0).then_some((alpha_mult, (alpha_mult * 255.0) as u8))
}

/// Whether the event must render into an isolated buffer before final blend.
pub(crate) fn needs_isolation(resolved: &ResolvedStyle, effect: &str) -> bool {
    let legacy = LegacyEffect::parse(effect);
    let effect_isolated = match legacy {
        Some(LegacyEffect::ScrollUp { .. } | LegacyEffect::ScrollDown { .. }) => true,
        Some(LegacyEffect::Banner { fadeaway, .. }) => fadeaway > 0.0,
        None => false,
    };
    resolved.clip.is_some()
        || resolved.inverse_clip.is_some()
        || resolved.clip_vector.is_some()
        || resolved.inverse_clip_vector.is_some()
        || resolved.blur > 0.0
        || effect_isolated
}

/// Paint a prepared event in libass-compatible effect order.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_prepared(
    compositor: &mut Compositor,
    buffer: &mut RenderBuffer,
    resolved: &ResolvedStyle,
    font_manager: &FontManager,
    prepared: PreparedEvent<'_>,
    video_height: u32,
    play_res_y: u32,
) {
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
        let clamp_i32 =
            |v: f64| finite_to_i32(v.clamp(f64::from(i32::MIN), f64::from(i32::MAX))).unwrap_or(0);
        let clamp_dim = |v: f64| clamp_i32(v.ceil().clamp(0.0, 65_536.0));
        let (box_rx, box_ry) = if resolved.scaled_border_and_shadow {
            (prepared.scale_x, prepared.scale_y)
        } else {
            (prepared.blur_scale_x, prepared.blur_scale_y)
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
            (f64::from(255 - box_color[3]) * prepared.alpha_mult).clamp(0.0, 255.0) as u8,
        ];
        // Box shadow (references shadow the padded box): same
        // offset/color model as glyph shadows, drawn first so all
        // boxes paint over all shadows.
        let shadow_c = resolved.shadow_color.to_ass_components();
        let shadow_fill = [
            shadow_c[0],
            shadow_c[1],
            shadow_c[2],
            (f64::from(255 - shadow_c[3]) * prepared.alpha_mult).clamp(0.0, 255.0) as u8,
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
        let line_boxes = event_line_boxes(&prepared.segments, &prepared.layout.items);
        // A trailing empty last line draws nothing (extent-based).
        let drawable = line_boxes
            .iter()
            .enumerate()
            .filter(|(i, line)| line.has_content || *i + 1 < line_boxes.len());
        let mut rects: Vec<(i32, i32, i32, i32)> = Vec::new();
        for (_, line) in drawable {
            let inset = line_align_inset(line.width, prepared.layout.width, resolved.alignment);
            let rect = (
                clamp_i32(prepared.base_x + inset),
                clamp_i32(prepared.base_y - prepared.layout.baseline + line.y),
                clamp_dim(line.width),
                clamp_dim(line.height),
            );
            rects.push(rect);
        }
        decorations::paint_opaque_boxes(
            buffer,
            &rects,
            shadow_active.then_some((shadow_ox, shadow_oy)),
            pad_x,
            pad_y,
            shadow_fill,
            fill,
        );
    }

    text::render_runs(
        &mut compositor.glyph_cache,
        buffer,
        &prepared.segments,
        &prepared.layout,
        resolved,
        font_manager,
        prepared.font,
        prepared.alpha_mult,
        prepared.alpha,
        prepared.scale_x,
        prepared.scale_y,
        prepared.blur_scale_x,
        prepared.blur_scale_y,
        prepared.base_x,
        prepared.base_y,
        prepared.org_x,
        prepared.org_y,
        video_height,
        play_res_y,
        prepared.elapsed_ms,
    );

    clipping::apply_event_clips(
        buffer,
        resolved,
        prepared.scale_x,
        prepared.scale_y,
        prepared.blur_scale_x,
        prepared.blur_scale_y,
    );

    // Legacy scroll-effect clip bounds and edge fades (VSFilter
    // EF_BANNER/EF_SCROLL clipper). Sequential clips intersect, so
    // these compose with user \clips above.
    match prepared.legacy_effect {
        Some(LegacyEffect::Banner { fadeaway, .. }) => {
            let w = buffer.width as i64;
            let h = buffer.height as i64;
            if w > 0 && h > 0 {
                let x1 = w.saturating_sub(1).min(i32::MAX as i64) as i32;
                let y1 = h.saturating_sub(1).min(i32::MAX as i64) as i32;
                effects::apply_clip(buffer, (0, 0, x1, y1));
            }
            if fadeaway > 0.0 {
                effects::apply_fadeaway_x(buffer, fadeaway * prepared.scale_x);
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
            let y0 = (top * prepared.scale_y).floor() as i64;
            let y1 = ((bottom * prepared.scale_y).ceil() as i64).saturating_sub(1);
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
                    fadeaway * prepared.scale_y,
                );
            }
        }
        None => {}
    }
}
