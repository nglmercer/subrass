use crate::renderer::buffer::{bitmap_has_pixels, RenderBuffer};

/// Apply shadow effect to a glyph bitmap.
#[allow(clippy::too_many_arguments)]
pub fn apply_shadow(
    buffer: &mut RenderBuffer,
    glyph_bitmap: &[u8],
    glyph_width: u32,
    glyph_height: u32,
    x: i32,
    y: i32,
    shadow_x: f64,
    shadow_y: f64,
    shadow_color: [u8; 4],
) {
    if !shadow_x.is_finite() || !shadow_y.is_finite() {
        return;
    }
    if !bitmap_has_pixels(glyph_bitmap, glyph_width, glyph_height) {
        return;
    }
    let sx = shadow_x.round().clamp(-4096.0, 4096.0) as i32;
    let sy = shadow_y.round().clamp(-4096.0, 4096.0) as i32;
    let base_x = i64::from(x) + i64::from(sx);
    let base_y = i64::from(y) + i64::from(sy);
    if base_x < i64::from(i32::MIN) - 65_536
        || base_x > i64::from(i32::MAX) + 65_536
        || base_y < i64::from(i32::MIN) - 65_536
        || base_y > i64::from(i32::MAX) + 65_536
    {
        return;
    }

    for gy in 0..glyph_height {
        for gx in 0..glyph_width {
            let idx = (u64::from(gy) * u64::from(glyph_width) + u64::from(gx)) as usize;
            let alpha = glyph_bitmap.get(idx).copied().unwrap_or(0);
            if alpha > 0 {
                let px_i64 = base_x + i64::from(gx);
                let py_i64 = base_y + i64::from(gy);
                if px_i64 < 0
                    || py_i64 < 0
                    || px_i64 >= i64::from(buffer.width)
                    || py_i64 >= i64::from(buffer.height)
                {
                    continue;
                }
                let a = (u32::from(alpha) * u32::from(shadow_color[3]) / 255) as u8;
                buffer.blend_pixel(
                    px_i64 as u32,
                    py_i64 as u32,
                    shadow_color[0],
                    shadow_color[1],
                    shadow_color[2],
                    a,
                );
            }
        }
    }
}
