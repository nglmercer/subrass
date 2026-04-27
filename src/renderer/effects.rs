use crate::renderer::buffer::RenderBuffer;

/// Apply shadow effect to a glyph bitmap
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
    let sx = shadow_x.round() as i32;
    let sy = shadow_y.round() as i32;

    for gy in 0..glyph_height as i32 {
        for gx in 0..glyph_width as i32 {
            let alpha = glyph_bitmap[(gy as u32 * glyph_width + gx as u32) as usize];
            if alpha > 0 {
                let px = x + gx + sx;
                let py = y + gy + sy;
                let a = (alpha as u32 * shadow_color[3] as u32 / 255) as u8;
                buffer.blend_pixel(
                    px as u32,
                    py as u32,
                    shadow_color[0],
                    shadow_color[1],
                    shadow_color[2],
                    a,
                );
            }
        }
    }
}

/// Apply outline effect to a glyph bitmap (border style 1)
/// Uses squared distance comparison to avoid sqrt in hot loop
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
    let radius = outline_width.ceil() as i32;
    let max_dist = outline_width + 0.5;
    let max_dist_sq = max_dist * max_dist;
    let inv_dist = 1.0 / (outline_width + 1.0);
    let base_alpha = outline_color[3] as f64;

    for gy in 0..glyph_height as i32 {
        for gx in 0..glyph_width as i32 {
            let alpha = glyph_bitmap[(gy as u32 * glyph_width + gx as u32) as usize];
            if alpha > 0 {
                for dy in -radius..=radius {
                    let dy_sq = (dy * dy) as f64;
                    for dx in -radius..=radius {
                        let dist_sq = dy_sq + (dx * dx) as f64;
                        if dist_sq <= max_dist_sq {
                            let px = x + gx + dx;
                            let py = y + gy + dy;
                            let dist = dist_sq.sqrt();
                            let a = (base_alpha * (1.0 - dist * inv_dist)) as u8;
                            if a > 0 {
                                buffer.blend_pixel(
                                    px as u32,
                                    py as u32,
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

/// Apply border style 3 (opaque box background)
#[allow(clippy::too_many_arguments)]
pub fn apply_opaque_box(
    buffer: &mut RenderBuffer,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    margin_l: i32,
    margin_r: i32,
    margin_v: i32,
    back_color: [u8; 4],
    _play_res_x: u32,
    _play_res_y: u32,
) {
    let box_x = x - margin_l;
    let box_y = y - margin_v;
    let box_w = width + margin_l + margin_r;
    let box_h = height + margin_v * 2;

    buffer.fill_rect(
        box_x,
        box_y,
        box_w,
        box_h,
        back_color[0],
        back_color[1],
        back_color[2],
        back_color[3],
    );
}

/// Apply clipping rectangle - only clears edge strips (O(perimeter) not O(area))
pub fn apply_clip(buffer: &mut RenderBuffer, clip_rect: (i32, i32, i32, i32)) {
    let (x1, y1, x2, y2) = clip_rect;
    let w = buffer.width as i32;
    let h = buffer.height as i32;

    // Clamp clip region to buffer bounds
    let cx0 = x1.max(0);
    let cy0 = y1.max(0);
    let cx1 = x2.min(w - 1);
    let cy1 = y2.min(h - 1);

    // Clear top strip (y: 0..cy0)
    for y in 0..cy0 {
        let row_start = (y as u32 * buffer.width * 4) as usize;
        let row_end = row_start + (buffer.width * 4) as usize;
        buffer.pixels[row_start..row_end].fill(0);
    }

    // Clear bottom strip (y: cy1+1..h)
    for y in (cy1 + 1)..h {
        let row_start = (y as u32 * buffer.width * 4) as usize;
        let row_end = row_start + (buffer.width * 4) as usize;
        buffer.pixels[row_start..row_end].fill(0);
    }

    // Clear left strip (x: 0..cx0) for rows in clip y range
    for y in cy0..=cy1 {
        if cy0 < 0 || cy1 >= h {
            continue;
        }
        let row_start = (y as u32 * buffer.width * 4) as usize;
        let left_end = (cx0.max(0) as u32 * 4) as usize;
        if left_end > 0 {
            let end = (row_start + left_end).min(buffer.pixels.len());
            buffer.pixels[row_start..end].fill(0);
        }
    }

    // Clear right strip (x: cx1+1..w) for rows in clip y range
    for y in cy0..=cy1 {
        if cy0 < 0 || cy1 >= h {
            continue;
        }
        let right_start = ((cx1 + 1).max(0) as u32 * 4) as usize;
        let row_start = (y as u32 * buffer.width * 4) as usize;
        let row_end = row_start + (buffer.width * 4) as usize;
        if right_start < (buffer.width * 4) as usize {
            let start = row_start + right_start;
            if start < buffer.pixels.len() {
                buffer.pixels[start..row_end].fill(0);
            }
        }
    }
}

/// Apply inverse clipping (hide inside clip region) - only clears inside rect
pub fn apply_inverse_clip(buffer: &mut RenderBuffer, clip_rect: (i32, i32, i32, i32)) {
    let (x1, y1, x2, y2) = clip_rect;
    let w = buffer.width as i32;
    let h = buffer.height as i32;

    // Clamp clip region to buffer bounds
    let cx0 = x1.max(0);
    let cy0 = y1.max(0);
    let cx1 = x2.min(w - 1);
    let cy1 = y2.min(h - 1);

    // Only clear the inside of the clip rect
    for y in cy0..=cy1 {
        let row_start = (y as u32 * buffer.width * 4) as usize;
        let left = (cx0.max(0) as u32 * 4) as usize;
        let right = ((cx1 + 1).min(w) as u32 * 4) as usize;
        let start = row_start + left;
        let end = row_start + right;
        if start < buffer.pixels.len() {
            let end = end.min(buffer.pixels.len());
            buffer.pixels[start..end].fill(0);
        }
    }
}

/// Apply Gaussian-like blur to buffer (3-pass box blur)
pub fn apply_blur(buffer: &mut RenderBuffer, blur_radius: f64) {
    let radius = (blur_radius * 2.0).round() as u32;
    buffer.box_blur(radius);
}

/// Calculate fade alpha based on time
pub fn calculate_fade_alpha(
    time_ms: u64,
    start_ms: u64,
    end_ms: u64,
    fade_in_ms: u64,
    fade_out_ms: u64,
) -> u8 {
    if time_ms < start_ms {
        return 0;
    }

    if time_ms >= end_ms {
        return 0;
    }

    let duration = end_ms - start_ms;
    let elapsed = time_ms - start_ms;

    // Fade in
    if fade_in_ms > 0 && elapsed < fade_in_ms {
        return ((elapsed as f64 / fade_in_ms as f64) * 255.0) as u8;
    }

    // Fade out
    if fade_out_ms > 0 && elapsed > duration - fade_out_ms {
        let remaining = duration - elapsed;
        return ((remaining as f64 / fade_out_ms as f64) * 255.0) as u8;
    }

    255
}
