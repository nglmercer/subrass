//! Shared glyph compositing primitives. Transform and cache ownership stays
//! in `paint::text`; this module owns the final outline/shadow/fill ordering.

use super::super::super::buffer::RenderBuffer;
use super::super::super::effects;
use super::super::karaoke::{paint_glyph_fill, GlyphGeom};

#[allow(clippy::too_many_arguments)]
pub(super) fn paint(
    buffer: &mut RenderBuffer,
    bitmap: &[u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    alpha: u8,
    fill: ([u8; 4], u8),
    outline: Option<([u8; 4], f64, f64)>,
    shadow: Option<([u8; 4], f64, f64)>,
) {
    if let Some((color, ox, oy)) = outline {
        effects::apply_outline_xy(buffer, bitmap, width, height, x, y, ox, oy, color);
    }
    if let Some((color, ox, oy)) = shadow {
        effects::apply_shadow(buffer, bitmap, width, height, x, y, ox, oy, color);
    }
    paint_glyph_fill(
        buffer,
        bitmap,
        GlyphGeom {
            w: width,
            h: height,
            gx: x,
            gy: y,
        },
        alpha,
        |_| fill,
    );
}
