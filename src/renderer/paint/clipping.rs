use crate::renderer::buffer::RenderBuffer;
use crate::renderer::drawing::DrawingParser;
use crate::renderer::effects;

use super::super::lines::{drawing_unit_scale, scale_clip_rect};
use super::super::state::{ResolvedStyle, VectorClip};

/// Clip painted runs after mask filtering, so blur cannot bleed outside clips.
pub(crate) fn apply_event_clips(
    buffer: &mut RenderBuffer,
    resolved: &ResolvedStyle,
    scale_x: f64,
    scale_y: f64,
    _blur_scale_x: f64,
    _blur_scale_y: f64,
) {
    if let Some(clip_rect) = resolved.clip {
        effects::apply_clip(buffer, scale_clip_rect(clip_rect, scale_x, scale_y));
    }
    if let Some(clip_rect) = resolved.inverse_clip {
        effects::apply_inverse_clip(buffer, scale_clip_rect(clip_rect, scale_x, scale_y));
    }
    if let Some(vector) = &resolved.clip_vector {
        apply_vector_clip(buffer, vector, scale_x, scale_y, false);
    }
    if let Some(vector) = &resolved.inverse_clip_vector {
        apply_vector_clip(buffer, vector, scale_x, scale_y, true);
    }
}

/// Render one vector clip into an alpha mask and apply or invert it.
pub(crate) fn apply_vector_clip(
    buffer: &mut RenderBuffer,
    clip: &VectorClip,
    scale_x: f64,
    scale_y: f64,
    inverse: bool,
) {
    let unit = drawing_unit_scale(scale_x, scale_y, clip.scale.max(1));
    let mut mask = match RenderBuffer::new(buffer.width, buffer.height) {
        Ok(mask) => mask,
        Err(_) => return,
    };
    DrawingParser::render_mask(&mut mask, &clip.drawing, 0.0, 0.0, unit);
    effects::apply_alpha_mask(buffer, &mask, inverse);
}
