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

    for gy in 0..glyph_height as i32 {
        for gx in 0..glyph_width as i32 {
            let alpha = glyph_bitmap[(gy as u32 * glyph_width + gx as u32) as usize];
            if alpha > 0 {
                // Draw outline around the glyph
                for dy in -radius..=radius {
                    for dx in -radius..=radius {
                        let dist = ((dx * dx + dy * dy) as f64).sqrt();
                        if dist <= outline_width + 0.5 {
                            let px = x + gx + dx;
                            let py = y + gy + dy;
                            let a = (outline_color[3] as f64 * (1.0 - dist / (outline_width + 1.0)))
                                as u8;
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
    // Calculate box position based on alignment
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

/// Apply clipping rectangle
pub fn apply_clip(buffer: &mut RenderBuffer, clip_rect: (i32, i32, i32, i32)) {
    let (x1, y1, x2, y2) = clip_rect;
    let w = buffer.width as i32;
    let h = buffer.height as i32;

    // Clear pixels outside clip region
    for y in 0..h {
        for x in 0..w {
            if x < x1 || x > x2 || y < y1 || y > y2 {
                let idx = ((y as u32 * buffer.width + x as u32) * 4) as usize;
                buffer.pixels[idx] = 0;
                buffer.pixels[idx + 1] = 0;
                buffer.pixels[idx + 2] = 0;
                buffer.pixels[idx + 3] = 0;
            }
        }
    }
}

/// Apply inverse clipping (hide inside clip region)
pub fn apply_inverse_clip(buffer: &mut RenderBuffer, clip_rect: (i32, i32, i32, i32)) {
    let (x1, y1, x2, y2) = clip_rect;
    let w = buffer.width as i32;
    let h = buffer.height as i32;

    for y in 0..h {
        for x in 0..w {
            if x >= x1 && x <= x2 && y >= y1 && y <= y2 {
                let idx = ((y as u32 * buffer.width + x as u32) * 4) as usize;
                buffer.pixels[idx] = 0;
                buffer.pixels[idx + 1] = 0;
                buffer.pixels[idx + 2] = 0;
                buffer.pixels[idx + 3] = 0;
            }
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
