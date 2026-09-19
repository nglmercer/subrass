#[cfg(test)]
use crate::renderer::buffer::RenderBuffer;

mod blur;
mod clip;
mod fade;
mod outline;
mod shadow;

pub use self::blur::{apply_blur, apply_blur_xy};
pub use self::clip::{apply_alpha_mask, apply_clip, apply_inverse_clip, apply_opaque_box};
pub(crate) use self::fade::interpolate_alpha;
pub use self::fade::{apply_fadeaway_x, apply_fadeaway_y, calculate_fade_alpha};
pub use self::outline::{apply_outline, apply_outline_xy};
pub use self::shadow::apply_shadow;

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_buffer(w: u32, h: u32) -> RenderBuffer {
        let mut buf = RenderBuffer::new(w, h).unwrap();
        buf.pixels.fill(255);
        buf
    }

    #[test]
    fn test_fadeaway_x_ramps_both_edges() {
        let mut buf = solid_buffer(20, 4);
        apply_fadeaway_x(&mut buf, 10.0);
        let col = |x: u32| buf.get_pixel(x, 0)[3];
        // VSFilter multipliers: left col i -> i/10, right col j -> (20-j)/10.
        assert_eq!(col(0), 0);
        assert_eq!(col(1), (255.0_f64 * 0.1).round() as u8);
        assert_eq!(col(9), (255.0_f64 * 0.9).round() as u8);
        assert_eq!(col(10), 255);
        assert_eq!(col(19), (255.0_f64 * 0.1).round() as u8);
        assert_eq!(col(18), (255.0_f64 * 0.2).round() as u8);
    }

    #[test]
    fn test_fadeaway_y_ramps_band_edges() {
        let mut buf = solid_buffer(4, 20);
        apply_fadeaway_y(&mut buf, 4, 16, 6.0);
        let row = |y: u32| buf.get_pixel(0, y)[3];
        assert_eq!(row(4), 0);
        assert_eq!(row(5), (255.0_f64 / 6.0).round() as u8);
        assert_eq!(row(10), 255);
        assert_eq!(row(15), (255.0_f64 / 6.0).round() as u8);
        assert_eq!(row(14), (255.0_f64 * 2.0 / 6.0).round() as u8);
        // Outside the band is untouched by the fade itself (clip clears it).
        assert_eq!(row(0), 255);
        assert_eq!(row(19), 255);
    }

    #[test]
    fn test_fadeaway_hostile_values_safe() {
        let mut buf = solid_buffer(8, 8);
        // Terminates, no panic, no hang on extreme/degenerate input.
        apply_fadeaway_x(&mut buf, 1e18);
        apply_fadeaway_y(&mut buf, -100, 1_000_000, 1e18);
        apply_fadeaway_x(&mut buf, f64::NAN);
        apply_fadeaway_y(&mut buf, 0, 8, f64::INFINITY);
        apply_fadeaway_x(&mut buf, 0.0);
        apply_fadeaway_y(&mut buf, 0, 8, -5.0);
        let mut empty = RenderBuffer::new(1, 1).unwrap();
        empty.pixels.fill(0);
        apply_fadeaway_x(&mut empty, 4.0);
        apply_fadeaway_y(&mut empty, 0, 1, 4.0);
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
    fn test_interpolate_alpha_matches_libass() {
        // Exact `interpolate_alpha` values on binary-exact fractions.
        // libass truncates toward zero: 127.5 -> 127, not 128.
        let t = (0, 1000, 2000, 3000);
        assert_eq!(interpolate_alpha(-5, t.0, t.1, t.2, t.3, 255, 0, 255), 255);
        assert_eq!(interpolate_alpha(0, t.0, t.1, t.2, t.3, 255, 0, 255), 255);
        assert_eq!(interpolate_alpha(500, t.0, t.1, t.2, t.3, 255, 0, 255), 127);
        assert_eq!(interpolate_alpha(1000, t.0, t.1, t.2, t.3, 255, 0, 255), 0);
        assert_eq!(interpolate_alpha(1500, t.0, t.1, t.2, t.3, 255, 0, 255), 0);
        assert_eq!(
            interpolate_alpha(2500, t.0, t.1, t.2, t.3, 255, 0, 255),
            127
        );
        assert_eq!(
            interpolate_alpha(2999, t.0, t.1, t.2, t.3, 255, 0, 255),
            254
        );
        assert_eq!(
            interpolate_alpha(3000, t.0, t.1, t.2, t.3, 255, 0, 255),
            255
        );
        assert_eq!(
            interpolate_alpha(9999, t.0, t.1, t.2, t.3, 255, 0, 255),
            255
        );
        // Zero-length ramps are instant jumps, never div-by-zero.
        assert_eq!(interpolate_alpha(499, 500, 500, 500, 500, 10, 20, 30), 10);
        assert_eq!(interpolate_alpha(500, 500, 500, 500, 500, 10, 20, 30), 30);
        // Hostile times cannot wrap or panic.
        assert_eq!(
            interpolate_alpha(i64::MAX, i32::MIN as i64, 0, 0, i32::MAX as i64, 1, 2, 3),
            3
        );
    }

    #[test]
    fn test_simple_fade_exact_values() {
        // \fad(500,500) over a 5000ms event: t = (0,500,4500,5000).
        assert_eq!(calculate_fade_alpha(0, 0, 5000, 500, 500), 0);
        assert_eq!(calculate_fade_alpha(250, 0, 5000, 500, 500), 128);
        assert_eq!(calculate_fade_alpha(500, 0, 5000, 500, 500), 255);
        assert_eq!(calculate_fade_alpha(4750, 0, 5000, 500, 500), 128);
        assert_eq!(calculate_fade_alpha(4999, 0, 5000, 500, 500), 1);
        assert_eq!(calculate_fade_alpha(5000, 0, 5000, 500, 500), 0);
    }

    #[test]
    fn test_simple_fade_overlap_follows_shared_ramp() {
        // Overlapping fades use the shared piecewise ramp, NOT the
        // minimum of two ramps: \fad(1000,1000) over 1000ms gives
        // opacity 128 at the midpoint (min-of-ramps would give 127).
        assert_eq!(calculate_fade_alpha(0, 0, 1000, 1000, 1000), 0);
        assert_eq!(calculate_fade_alpha(500, 0, 1000, 1000, 1000), 128);
        // At 999 the fade-in ramp is nearly complete: trunc(255 * 0.001)
        // is 0, so the frame is fully opaque (libass truncation).
        assert_eq!(calculate_fade_alpha(999, 0, 1000, 1000, 1000), 255);
        // Fade-out longer than the event covers the whole event:
        // \fad(0,4000) over 2000ms -> t3 = -2000, one long ramp.
        assert_eq!(calculate_fade_alpha(0, 0, 2000, 0, 4000), 128);
        assert_eq!(calculate_fade_alpha(1000, 0, 2000, 0, 4000), 64);
        assert_eq!(calculate_fade_alpha(1999, 0, 2000, 0, 4000), 1);
        assert_eq!(calculate_fade_alpha(2000, 0, 2000, 0, 4000), 0);
        // Non-positive times mean "no fade" on that side, like libass.
        assert_eq!(calculate_fade_alpha(0, 0, 5000, -5, 500), 255);
        assert_eq!(calculate_fade_alpha(4750, 0, 5000, -5, 500), 128);
        assert_eq!(calculate_fade_alpha(2500, 0, 5000, 0, 0), 255);
        assert_eq!(calculate_fade_alpha(2500, 0, 5000, -3, -7), 255);
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
            [255, 255, 255, 255],
        );
        apply_opaque_box(
            &mut buf,
            i32::MIN,
            i32::MIN,
            i32::MAX,
            i32::MAX,
            i32::MAX,
            i32::MAX,
            [255, 255, 255, 255],
        );
    }

    #[test]
    fn test_apply_opaque_box_geometry_and_alpha() {
        // Text block (4,4,8x8) + padding 2 -> box (2,2)-(13,13).
        let mut buf = RenderBuffer::new(16, 16).unwrap();
        apply_opaque_box(&mut buf, 4, 4, 8, 8, 2, 2, [255, 0, 0, 255]);
        assert_eq!(buf.get_pixel(2, 2), [255, 0, 0, 255]);
        assert_eq!(buf.get_pixel(13, 13), [255, 0, 0, 255]);
        assert_eq!(buf.get_pixel(1, 1), [0, 0, 0, 0]);
        assert_eq!(buf.get_pixel(14, 14), [0, 0, 0, 0]);
        // Opacity 0 paints nothing (straight-alpha [3] slot).
        let mut buf = RenderBuffer::new(16, 16).unwrap();
        apply_opaque_box(&mut buf, 4, 4, 8, 8, 2, 2, [255, 0, 0, 0]);
        assert!(buf.pixels.iter().all(|&p| p == 0));
        // Negative padding clamps to a tight box.
        let mut buf = RenderBuffer::new(16, 16).unwrap();
        apply_opaque_box(&mut buf, 4, 4, 8, 8, -5, -5, [255, 0, 0, 255]);
        assert_eq!(buf.get_pixel(4, 4), [255, 0, 0, 255]);
        assert_eq!(buf.get_pixel(3, 3), [0, 0, 0, 0]);
    }
}
