use crate::renderer::buffer::{RenderBuffer, MAX_OUTLINE_RADIUS};

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
    if !shadow_x.is_finite() || !shadow_y.is_finite() {
        return;
    }
    if glyph_bitmap.len() < glyph_width as usize * glyph_height as usize {
        return;
    }
    let sx = shadow_x.round().clamp(-4096.0, 4096.0) as i32;
    let sy = shadow_y.round().clamp(-4096.0, 4096.0) as i32;

    for gy in 0..glyph_height as i32 {
        for gx in 0..glyph_width as i32 {
            let alpha = glyph_bitmap[(gy as u32 * glyph_width + gx as u32) as usize];
            if alpha > 0 {
                let px = x.saturating_add(gx).saturating_add(sx);
                let py = y.saturating_add(gy).saturating_add(sy);
                let a = (alpha as u32 * shadow_color[3] as u32 / 255) as u8;
                if let (Ok(px), Ok(py)) = (u32::try_from(px), u32::try_from(py)) {
                    buffer.blend_pixel(
                        px,
                        py,
                        shadow_color[0],
                        shadow_color[1],
                        shadow_color[2],
                        a,
                    );
                }
            }
        }
    }
}

/// Apply outline effect to a glyph bitmap (border style 1)
/// Uses squared distance check and quadratic alpha falloff (no sqrt in hot loop)
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
    if glyph_bitmap.len() < glyph_width as usize * glyph_height as usize {
        return;
    }
    // Clamp: an unbounded radius turns the dx/dy loops into a hang.
    let outline_width = outline_width.min(MAX_OUTLINE_RADIUS);
    let radius = outline_width.ceil() as i32;
    let max_dist = outline_width + 0.5;
    let max_dist_sq = max_dist * max_dist;
    let inv_max_dist_sq = 1.0 / max_dist_sq;
    let base_alpha = outline_color[3] as f64;

    for gy in 0..glyph_height as i32 {
        for gx in 0..glyph_width as i32 {
            let alpha = glyph_bitmap[(gy as u32 * glyph_width + gx as u32) as usize];
            if alpha > 0 {
                let glyph_alpha_mult = alpha as f64 / 255.0;
                for dy in -radius..=radius {
                    let dy_sq = (dy * dy) as f64;
                    for dx in -radius..=radius {
                        let dist_sq = dy_sq + (dx * dx) as f64;
                        if dist_sq <= max_dist_sq {
                            // Quadratic falloff: alpha = base * (1 - dist²/max²)
                            // Visually nearly identical to linear, but eliminates sqrt
                            let a =
                                (base_alpha * glyph_alpha_mult * (1.0 - dist_sq * inv_max_dist_sq))
                                    as u8;
                            if a > 0 {
                                let px = x.saturating_add(gx).saturating_add(dx);
                                let py = y.saturating_add(gy).saturating_add(dy);
                                if let (Ok(px), Ok(py)) = (u32::try_from(px), u32::try_from(py)) {
                                    buffer.blend_pixel(
                                        px,
                                        py,
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
    // Degenerate axis: circular outline with the live radius.
    let (ox, oy) = if ox <= 0.0 {
        (oy, oy)
    } else if oy <= 0.0 {
        (ox, ox)
    } else {
        (ox, oy)
    };
    if glyph_bitmap.len() < glyph_width as usize * glyph_height as usize {
        return;
    }
    let radius_x = ox.ceil() as i32;
    let radius_y = oy.ceil() as i32;
    let max_x = ox + 0.5;
    let max_y = oy + 0.5;
    let base_alpha = outline_color[3] as f64;

    for gy in 0..glyph_height as i32 {
        for gx in 0..glyph_width as i32 {
            let alpha = glyph_bitmap[(gy as u32 * glyph_width + gx as u32) as usize];
            if alpha > 0 {
                let glyph_alpha_mult = alpha as f64 / 255.0;
                for dy in -radius_y..=radius_y {
                    let ny = dy as f64 / max_y;
                    for dx in -radius_x..=radius_x {
                        let nx = dx as f64 / max_x;
                        let dist_sq = nx * nx + ny * ny;
                        if dist_sq <= 1.0 {
                            let a = (base_alpha * glyph_alpha_mult * (1.0 - dist_sq)) as u8;
                            if a > 0 {
                                let px = x.saturating_add(gx).saturating_add(dx);
                                let py = y.saturating_add(gy).saturating_add(dy);
                                if let (Ok(px), Ok(py)) = (u32::try_from(px), u32::try_from(py)) {
                                    buffer.blend_pixel(
                                        px,
                                        py,
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
}

/// Multiply the buffer alpha by a same-size mask's alpha channel
/// (vector `\clip`). With `inverse`, multiply by the inverted mask
/// (`\iclip`). Size mismatch: no-op.
pub fn apply_alpha_mask(buffer: &mut RenderBuffer, mask: &RenderBuffer, inverse: bool) {
    if buffer.width != mask.width || buffer.height != mask.height {
        return;
    }
    for (dst, m) in buffer
        .pixels
        .chunks_exact_mut(4)
        .zip(mask.pixels.chunks_exact(4))
    {
        let factor = if inverse { 255 - m[3] } else { m[3] };
        dst[3] = (dst[3] as u32 * factor as u32 / 255) as u8;
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
    let box_x = x.saturating_sub(margin_l);
    let box_y = y.saturating_sub(margin_v);
    let box_w = width.saturating_add(margin_l).saturating_add(margin_r);
    let box_h = height.saturating_add(margin_v.saturating_mul(2));

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

/// Normalize a clip rectangle: order corners, intersect with the buffer
/// bounds. Returns `None` when the intersection is empty. Coordinates are
/// inclusive on both ends, matching the historical behavior of these helpers.
fn normalize_clip(
    buffer: &RenderBuffer,
    clip_rect: (i32, i32, i32, i32),
) -> Option<(i64, i64, i64, i64)> {
    if buffer.width == 0 || buffer.height == 0 {
        return None;
    }
    let (x1, y1, x2, y2) = clip_rect;
    let (rx0, rx1) = (x1.min(x2) as i64, x1.max(x2) as i64);
    let (ry0, ry1) = (y1.min(y2) as i64, y1.max(y2) as i64);
    let cx0 = rx0.max(0);
    let cy0 = ry0.max(0);
    let cx1 = rx1.min(buffer.width as i64 - 1);
    let cy1 = ry1.min(buffer.height as i64 - 1);
    if cx0 > cx1 || cy0 > cy1 {
        return None;
    }
    Some((cx0, cy0, cx1, cy1))
}

/// Clear the rectangle [x0..=x1] x [y0..=y1] (buffer coordinates, pre-clamped).
fn clear_rect(buffer: &mut RenderBuffer, x0: i64, y0: i64, x1: i64, y1: i64) {
    if x0 > x1 || y0 > y1 {
        return;
    }
    let stride = buffer.width as usize * 4;
    for y in y0..=y1 {
        let row_start = y as usize * stride;
        let start = row_start + x0 as usize * 4;
        let end = row_start + (x1 as usize + 1) * 4;
        if end <= buffer.pixels.len() {
            buffer.pixels[start..end].fill(0);
        }
    }
}

/// Apply clipping rectangle - only clears edge strips (O(perimeter) not O(area)).
///
/// Pixels outside the rectangle are removed. Fully offscreen or empty clip
/// rectangles clear the whole buffer (nothing is inside).
pub fn apply_clip(buffer: &mut RenderBuffer, clip_rect: (i32, i32, i32, i32)) {
    let w = buffer.width as i64;
    let h = buffer.height as i64;
    let Some((cx0, cy0, cx1, cy1)) = normalize_clip(buffer, clip_rect) else {
        buffer.pixels.fill(0);
        return;
    };

    // Clear top strip (y: 0..cy0)
    clear_rect(buffer, 0, 0, w - 1, cy0 - 1);
    // Clear bottom strip (y: cy1+1..h)
    clear_rect(buffer, 0, cy1 + 1, w - 1, h - 1);
    // Clear left strip (x: 0..cx0) for rows in clip y range
    clear_rect(buffer, 0, cy0, cx0 - 1, cy1);
    // Clear right strip (x: cx1+1..w) for rows in clip y range
    clear_rect(buffer, cx1 + 1, cy0, w - 1, cy1);
}

/// Apply inverse clipping (hide inside clip region) - only clears inside rect.
///
/// Pixels inside the rectangle are removed. Fully offscreen or empty clip
/// rectangles leave the buffer untouched (nothing is inside).
pub fn apply_inverse_clip(buffer: &mut RenderBuffer, clip_rect: (i32, i32, i32, i32)) {
    let Some((cx0, cy0, cx1, cy1)) = normalize_clip(buffer, clip_rect) else {
        return;
    };
    clear_rect(buffer, cx0, cy0, cx1, cy1);
}

/// Apply Gaussian-like blur to buffer. Non-finite or negative radii are
/// ignored; large radii are clamped by [`RenderBuffer::box_blur`].
pub fn apply_blur(buffer: &mut RenderBuffer, blur_radius: f64) {
    if !blur_radius.is_finite() || blur_radius <= 0.0 {
        return;
    }
    let radius = (blur_radius * 2.0).round().clamp(0.0, u32::MAX as f64) as u32;
    buffer.box_blur(radius);
}

/// Calculate fade alpha based on time.
///
/// Returns an opacity value: 0 = fully transparent, 255 = fully opaque.
/// When fade-in and fade-out overlap (fade_out > duration), both ramps apply
/// and the darker of the two wins.
pub fn calculate_fade_alpha(
    time_ms: u64,
    start_ms: u64,
    end_ms: u64,
    fade_in_ms: u64,
    fade_out_ms: u64,
) -> u8 {
    if time_ms < start_ms || time_ms >= end_ms {
        return 0;
    }

    let duration = end_ms - start_ms;
    let elapsed = time_ms - start_ms;

    // Fade in: 0 -> 255 over [0, fade_in)
    let fade_in_alpha = if fade_in_ms > 0 && elapsed < fade_in_ms {
        (elapsed as f64 / fade_in_ms as f64 * 255.0) as u8
    } else {
        255
    };

    // Fade out: 255 -> 0 over (duration - fade_out, duration).
    // saturating_sub defines the overlap behavior: a fade-out longer than
    // the event covers the whole event.
    let fade_out_alpha = if fade_out_ms > 0 {
        let fade_out_start = duration.saturating_sub(fade_out_ms);
        if elapsed > fade_out_start {
            let remaining = duration - elapsed;
            (remaining as f64 / fade_out_ms as f64 * 255.0).clamp(0.0, 255.0) as u8
        } else {
            255
        }
    } else {
        255
    };

    fade_in_alpha.min(fade_out_alpha)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_buffer(w: u32, h: u32) -> RenderBuffer {
        let mut buf = RenderBuffer::new(w, h).unwrap();
        buf.pixels.fill(255);
        buf
    }

    #[test]
    fn test_apply_clip_keeps_inside() {
        let mut buf = solid_buffer(10, 10);
        apply_clip(&mut buf, (2, 2, 5, 5));
        assert_eq!(buf.get_pixel(3, 3), [255, 255, 255, 255]);
        assert_eq!(buf.get_pixel(0, 0), [0, 0, 0, 0]);
        assert_eq!(buf.get_pixel(9, 9), [0, 0, 0, 0]);
        assert_eq!(buf.get_pixel(2, 2), [255, 255, 255, 255]);
        assert_eq!(buf.get_pixel(5, 5), [255, 255, 255, 255]);
        assert_eq!(buf.get_pixel(6, 5), [0, 0, 0, 0]);
    }

    #[test]
    fn test_apply_clip_reversed_coordinates() {
        let mut buf = solid_buffer(10, 10);
        apply_clip(&mut buf, (5, 5, 2, 2));
        assert_eq!(buf.get_pixel(3, 3), [255, 255, 255, 255]);
        assert_eq!(buf.get_pixel(0, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn test_apply_clip_offscreen_clears_all() {
        let mut buf = solid_buffer(10, 10);
        apply_clip(&mut buf, (50, 50, 60, 60));
        assert!(buf.pixels.iter().all(|&p| p == 0));

        let mut buf = solid_buffer(10, 10);
        apply_clip(&mut buf, (-60, -60, -50, -50));
        assert!(buf.pixels.iter().all(|&p| p == 0));
    }

    #[test]
    fn test_apply_clip_extreme_coordinates() {
        let mut buf = solid_buffer(10, 10);
        // Must terminate and never panic
        apply_clip(&mut buf, (i32::MIN, i32::MIN, i32::MAX, i32::MAX));
        // Covers the whole buffer: nothing cleared
        assert!(buf.pixels.iter().all(|&p| p == 255));

        let mut buf = solid_buffer(10, 10);
        apply_clip(&mut buf, (i32::MIN, i32::MIN, -100, -100));
        assert!(buf.pixels.iter().all(|&p| p == 0));
    }

    #[test]
    fn test_apply_inverse_clip_clears_inside() {
        let mut buf = solid_buffer(10, 10);
        apply_inverse_clip(&mut buf, (2, 2, 5, 5));
        assert_eq!(buf.get_pixel(3, 3), [0, 0, 0, 0]);
        assert_eq!(buf.get_pixel(0, 0), [255, 255, 255, 255]);
        assert_eq!(buf.get_pixel(9, 9), [255, 255, 255, 255]);
    }

    #[test]
    fn test_apply_inverse_clip_reversed_and_offscreen() {
        let mut buf = solid_buffer(10, 10);
        apply_inverse_clip(&mut buf, (5, 5, 2, 2));
        assert_eq!(buf.get_pixel(3, 3), [0, 0, 0, 0]);

        // Offscreen: untouched
        let mut buf = solid_buffer(10, 10);
        apply_inverse_clip(&mut buf, (50, 50, 60, 60));
        assert!(buf.pixels.iter().all(|&p| p == 255));

        let mut buf = solid_buffer(10, 10);
        apply_inverse_clip(&mut buf, (i32::MIN, i32::MIN, i32::MAX, i32::MAX));
        assert!(buf.pixels.iter().all(|&p| p == 0));
    }

    #[test]
    fn test_fade_out_longer_than_duration() {
        // fade_out (5000) > duration (2000): no underflow, defined ramp
        let a_start = calculate_fade_alpha(0, 0, 2000, 0, 5000);
        let a_mid = calculate_fade_alpha(1000, 0, 2000, 0, 5000);
        let a_end = calculate_fade_alpha(1999, 0, 2000, 0, 5000);
        assert!(a_start >= a_mid && a_mid >= a_end);
        assert_eq!(calculate_fade_alpha(2000, 0, 2000, 0, 5000), 0);
    }

    #[test]
    fn test_fade_in_out_overlap_takes_minimum() {
        // fade in 1000 + fade out 1000 over a 1000ms event
        let a = calculate_fade_alpha(500, 0, 1000, 1000, 1000);
        // fade-in gives ~127, fade-out gives ~127
        assert!((100..=160).contains(&a));
    }

    #[test]
    fn test_apply_blur_rejects_degenerate() {
        let mut buf = solid_buffer(8, 8);
        apply_blur(&mut buf, f64::NAN);
        apply_blur(&mut buf, f64::INFINITY);
        apply_blur(&mut buf, -5.0);
        assert!(buf.pixels.iter().all(|&p| p == 255));
        apply_blur(&mut buf, 1e300); // clamps, terminates
    }

    #[test]
    fn test_apply_outline_xy_independent_radii() {
        // Wide horizontal, narrow vertical: elliptical spread.
        let mut buf = RenderBuffer::new(32, 32).unwrap();
        let glyph = vec![255u8; 1];
        apply_outline_xy(&mut buf, &glyph, 1, 1, 16, 16, 6.0, 1.0, [255, 0, 0, 255]);
        let painted: Vec<(u32, u32)> = (0..32)
            .flat_map(|y| (0..32).map(move |x| (x, y)))
            .filter(|&(x, y)| buf.get_pixel(x, y)[3] > 0)
            .collect();
        assert!(!painted.is_empty());
        let min_x = painted.iter().map(|p| p.0).min().unwrap();
        let max_x = painted.iter().map(|p| p.0).max().unwrap();
        let min_y = painted.iter().map(|p| p.1).min().unwrap();
        let max_y = painted.iter().map(|p| p.1).max().unwrap();
        // Horizontal spread exceeds vertical spread.
        assert!(
            max_x - min_x > max_y - min_y,
            "{:?}",
            (min_x, max_x, min_y, max_y)
        );
        // Both zero: no-op.
        let mut buf = RenderBuffer::new(32, 32).unwrap();
        apply_outline_xy(&mut buf, &glyph, 1, 1, 16, 16, 0.0, 0.0, [255, 0, 0, 255]);
        assert!(buf.pixels.iter().all(|&p| p == 0));
        // Degenerate axis falls back to circular with the live radius.
        let mut a = RenderBuffer::new(32, 32).unwrap();
        apply_outline_xy(&mut a, &glyph, 1, 1, 16, 16, 4.0, 0.0, [255, 0, 0, 255]);
        let mut b = RenderBuffer::new(32, 32).unwrap();
        apply_outline_xy(&mut b, &glyph, 1, 1, 16, 16, 4.0, 4.0, [255, 0, 0, 255]);
        assert_eq!(a.pixels, b.pixels);
    }

    #[test]
    fn test_apply_alpha_mask_and_inverse() {
        let mut buf = RenderBuffer::new(4, 1).unwrap();
        buf.pixels.fill(255);
        let mut mask = RenderBuffer::new(4, 1).unwrap();
        mask.set_pixel(0, 0, 0, 0, 0, 255);
        mask.set_pixel(1, 0, 0, 0, 0, 128);
        apply_alpha_mask(&mut buf, &mask, false);
        assert_eq!(buf.get_pixel(0, 0)[3], 255);
        assert_eq!(buf.get_pixel(1, 0)[3], 128);
        assert_eq!(buf.get_pixel(2, 0)[3], 0);
        let mut buf = RenderBuffer::new(4, 1).unwrap();
        buf.pixels.fill(255);
        apply_alpha_mask(&mut buf, &mask, true);
        assert_eq!(buf.get_pixel(0, 0)[3], 0);
        assert_eq!(buf.get_pixel(1, 0)[3], 127);
        assert_eq!(buf.get_pixel(2, 0)[3], 255);
        // Size mismatch: no-op
        let small = RenderBuffer::new(2, 2).unwrap();
        apply_alpha_mask(&mut buf, &small, false);
        assert_eq!(buf.get_pixel(0, 0)[3], 0);
    }

    #[test]
    fn test_apply_outline_clamps_huge_width() {
        let mut buf = RenderBuffer::new(16, 16).unwrap();
        let glyph = vec![255u8; 4];
        apply_outline(&mut buf, &glyph, 2, 2, 4, 4, 1e12, [255, 0, 0, 255]);
        apply_outline(&mut buf, &glyph, 2, 2, 4, 4, f64::NAN, [255, 0, 0, 255]);
    }

    #[test]
    fn test_apply_opaque_box_saturates() {
        let mut buf = RenderBuffer::new(16, 16).unwrap();
        apply_opaque_box(
            &mut buf,
            i32::MAX,
            i32::MAX,
            i32::MAX,
            i32::MAX,
            i32::MAX,
            i32::MAX,
            i32::MAX,
            [255, 255, 255, 255],
            640,
            480,
        );
        apply_opaque_box(
            &mut buf,
            i32::MIN,
            i32::MIN,
            i32::MAX,
            i32::MAX,
            i32::MAX,
            i32::MAX,
            i32::MAX,
            [255, 255, 255, 255],
            640,
            480,
        );
    }
}
