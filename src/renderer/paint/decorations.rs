//! Decoration paint helpers.
//!
//! Glyph underline/strikeout geometry currently travels with the cached
//! glyph bitmap so it shares transforms, karaoke sweeps, and fallback-face
//! metrics. The module is kept as the ownership boundary for extracting
//! decoration-specific paint operations without changing that ordering.

use crate::renderer::buffer::RenderBuffer;
use crate::renderer::effects;

/// Paint the per-line opaque-box decoration and its optional shadow. Geometry
/// is resolved by the layout stage; this module owns the actual paint order
/// (all shadows first, then all boxes) so overlapping lines match VSFilter.
pub(crate) fn paint_opaque_boxes(
    buffer: &mut RenderBuffer,
    rects: &[(i32, i32, i32, i32)],
    shadow_offset: Option<(i32, i32)>,
    pad_x: i32,
    pad_y: i32,
    shadow_fill: [u8; 4],
    fill: [u8; 4],
) {
    if let Some((shadow_x, shadow_y)) = shadow_offset {
        for (x, y, w, h) in rects {
            effects::apply_opaque_box(
                buffer,
                x.saturating_add(shadow_x),
                y.saturating_add(shadow_y),
                *w,
                *h,
                pad_x,
                pad_y,
                shadow_fill,
            );
        }
    }
    for (x, y, w, h) in rects {
        effects::apply_opaque_box(buffer, *x, *y, *w, *h, pad_x, pad_y, fill);
    }
}
