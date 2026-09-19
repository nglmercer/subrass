use crate::renderer::buffer::{bitmap_has_pixels, RenderBuffer, MAX_OUTLINE_RADIUS};

/// Apply outline effect to a glyph bitmap (border style 1).
/// Uses squared distance check and quadratic alpha falloff (no sqrt in hot loop).
#[allow(clippy::too_many_arguments)]
pub fn apply_outline(
    buffer: &mut RenderBuffer,
    glyph_bitmap: &[u8],
    glyph_width: u32,
    glyph_height: u32,
    x: i32,
    y: i32,
    outline_width: f64,
    outline_color: [u8; 4],
) {
    if !outline_width.is_finite() || outline_width <= 0.0 {
        return;
    }
    if !bitmap_has_pixels(glyph_bitmap, glyph_width, glyph_height) {
        return;
    }
    let outline_width = outline_width.clamp(0.0, MAX_OUTLINE_RADIUS);
    let radius = outline_width.ceil().clamp(0.0, 4096.0) as i32;
    let max_dist = outline_width + 0.5;
    let max_dist_sq = max_dist * max_dist;
    if !max_dist_sq.is_finite() || max_dist_sq <= 0.0 {
        return;
    }
    let inv_max_dist_sq = 1.0 / max_dist_sq;
    let base_alpha = f64::from(outline_color[3]);

    for gy in 0..glyph_height {
        for gx in 0..glyph_width {
            let idx = (u64::from(gy) * u64::from(glyph_width) + u64::from(gx)) as usize;
            let alpha = glyph_bitmap.get(idx).copied().unwrap_or(0);
            if alpha > 0 {
                let glyph_alpha_mult = f64::from(alpha) / 255.0;
                for dy in -radius..=radius {
                    let dy_sq = f64::from(dy) * f64::from(dy);
                    for dx in -radius..=radius {
                        let dist_sq = dy_sq + f64::from(dx) * f64::from(dx);
                        if dist_sq <= max_dist_sq {
                            let a =
                                (base_alpha * glyph_alpha_mult * (1.0 - dist_sq * inv_max_dist_sq))
                                    .clamp(0.0, 255.0) as u8;
                            if a > 0 {
                                let px_i64 = i64::from(x) + i64::from(gx) + i64::from(dx);
                                let py_i64 = i64::from(y) + i64::from(gy) + i64::from(dy);
                                if px_i64 < 0
                                    || py_i64 < 0
                                    || px_i64 >= i64::from(buffer.width)
                                    || py_i64 >= i64::from(buffer.height)
                                {
                                    continue;
                                }
                                buffer.blend_pixel(
                                    px_i64 as u32,
                                    py_i64 as u32,
                                    outline_color[0],
                                    outline_color[1],
                                    outline_color[2],
                                    a,
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Apply outline with independent X/Y radii (elliptical falloff).
/// A zero radius on one axis falls back to the other (circular).
#[allow(clippy::too_many_arguments)]
pub fn apply_outline_xy(
    buffer: &mut RenderBuffer,
    glyph_bitmap: &[u8],
    glyph_width: u32,
    glyph_height: u32,
    x: i32,
    y: i32,
    outline_x: f64,
    outline_y: f64,
    outline_color: [u8; 4],
) {
    if !outline_x.is_finite() || !outline_y.is_finite() {
        return;
    }
    let ox = outline_x.clamp(0.0, MAX_OUTLINE_RADIUS);
    let oy = outline_y.clamp(0.0, MAX_OUTLINE_RADIUS);
    if ox <= 0.0 && oy <= 0.0 {
        return;
    }
    let (ox, oy) = if ox <= 0.0 {
        (oy, oy)
    } else if oy <= 0.0 {
        (ox, ox)
    } else {
        (ox, oy)
    };
    if !bitmap_has_pixels(glyph_bitmap, glyph_width, glyph_height) {
        return;
    }
    let radius_x = ox.ceil().clamp(0.0, 4096.0) as i32;
    let radius_y = oy.ceil().clamp(0.0, 4096.0) as i32;
    let max_x = ox + 0.5;
    let max_y = oy + 0.5;
    if !max_x.is_finite() || !max_y.is_finite() || max_x <= 0.0 || max_y <= 0.0 {
        return;
    }
    let base_alpha = f64::from(outline_color[3]);

    for gy in 0..glyph_height {
        for gx in 0..glyph_width {
            let idx = (u64::from(gy) * u64::from(glyph_width) + u64::from(gx)) as usize;
            let alpha = glyph_bitmap.get(idx).copied().unwrap_or(0);
            if alpha > 0 {
                let glyph_alpha_mult = f64::from(alpha) / 255.0;
                for dy in -radius_y..=radius_y {
                    let ny = f64::from(dy) / max_y;
                    for dx in -radius_x..=radius_x {
                        let nx = f64::from(dx) / max_x;
                        let dist_sq = nx * nx + ny * ny;
                        if dist_sq <= 1.0 {
                            let a = (base_alpha * glyph_alpha_mult * (1.0 - dist_sq))
                                .clamp(0.0, 255.0) as u8;
                            if a > 0 {
                                let px_i64 = i64::from(x) + i64::from(gx) + i64::from(dx);
                                let py_i64 = i64::from(y) + i64::from(gy) + i64::from(dy);
                                if px_i64 < 0
                                    || py_i64 < 0
                                    || px_i64 >= i64::from(buffer.width)
                                    || py_i64 >= i64::from(buffer.height)
                                {
                                    continue;
                                }
                                buffer.blend_pixel(
                                    px_i64 as u32,
                                    py_i64 as u32,
                                    outline_color[0],
                                    outline_color[1],
                                    outline_color[2],
                                    a,
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
