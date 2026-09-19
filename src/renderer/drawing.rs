use crate::renderer::buffer::RenderBuffer;

/// Defensive caps for untrusted drawing input.
pub const MAX_DRAWING_COMMANDS: usize = 100_000;
pub const MAX_DRAWING_POINTS: usize = 1_000_000;
/// Maximum recursion depth for cubic flattening. The flatness threshold is
/// in drawing units; subdivision is therefore stable before the caller's
/// device-scale conversion.
const CURVE_MAX_DEPTH: u8 = 12;
const CURVE_FLATNESS: f64 = 0.05;

#[path = "drawing/lexer.rs"]
mod lexer;
use self::lexer::parse;
#[path = "drawing/paths.rs"]
mod paths;
#[path = "drawing/raster.rs"]
mod raster;
pub struct DrawingParser;

impl DrawingParser {
    pub fn render_drawing(
        buffer: &mut RenderBuffer,
        text: &str,
        x: f64,
        y: f64,
        scale: f64,
        color: [u8; 4],
    ) {
        if !x.is_finite() || !y.is_finite() || !scale.is_finite() || scale <= 0.0 {
            return;
        }
        raster::fill_polygons(buffer, &Self::polygons(text), x, y, scale, color);
    }

    /// Render a drawing as a white alpha mask (alpha 255 inside shapes).
    pub fn render_mask(buffer: &mut RenderBuffer, text: &str, x: f64, y: f64, scale: f64) {
        if !x.is_finite() || !y.is_finite() || !scale.is_finite() || scale <= 0.0 {
            return;
        }
        buffer.pixels.fill(0);
        raster::fill_polygons_mask(buffer, &Self::polygons(text), x, y, scale);
    }

    /// Measure a drawing's bounding box in drawing units.
    /// Returns (min_x, min_y, width, height), or None when empty.
    pub fn measure(text: &str) -> Option<(f64, f64, f64, f64)> {
        let polygons = Self::polygons(text);
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        let mut any = false;
        for polygon in &polygons {
            for &(x, y) in polygon {
                any = true;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
        if !any {
            return None;
        }
        Some((
            min_x,
            min_y,
            (max_x - min_x).max(0.0),
            (max_y - min_y).max(0.0),
        ))
    }

    fn polygons(text: &str) -> Vec<Vec<(f64, f64)>> {
        let commands = parse(text);
        paths::commands_to_polygons(&commands)
    }

    /// Render a drawing clipped to device columns `[x_lo, x_hi)`.
    /// `None` bounds are open; a fully-open range draws everything.
    pub fn render_drawing_clipped(
        buffer: &mut RenderBuffer,
        text: &str,
        x: f64,
        y: f64,
        scale: f64,
        color: [u8; 4],
        x_range: (Option<i64>, Option<i64>),
    ) {
        if !x.is_finite() || !y.is_finite() || !scale.is_finite() || scale <= 0.0 {
            return;
        }
        let range = match x_range {
            (None, None) => None,
            (lo, hi) => Some((lo.unwrap_or(i64::MIN), hi.unwrap_or(i64::MAX))),
        };
        if let Some((lo, hi)) = range {
            if lo >= hi {
                return;
            }
        }
        raster::fill_polygons_clipped(buffer, &Self::polygons(text), x, y, scale, color, range);
    }

    /// Stamp a drawing's outline ring and drop shadow under its fill
    /// (libass BorderStyle 1; radii/offsets in device pixels). The
    /// coverage comes from the same scan as the fill, so the ring hugs
    /// the exact fill pixel set. Karaoke sweeps split only the fill —
    /// like the text sweep path, whose outline/shadow never split —
    /// so callers paint this once per drawing before any clipped
    /// fill passes. No-op when both are `None`.
    pub fn render_drawing_effects(
        buffer: &mut RenderBuffer,
        text: &str,
        x: f64,
        y: f64,
        scale: f64,
        outline: Option<([u8; 4], f64, f64)>,
        shadow: Option<([u8; 4], f64, f64)>,
    ) {
        if outline.is_none() && shadow.is_none() {
            return;
        }
        if !x.is_finite() || !y.is_finite() || !scale.is_finite() || scale <= 0.0 {
            return;
        }
        let (w, h) = (buffer.width, buffer.height);
        if w == 0 || h == 0 {
            return;
        }
        let polygons = Self::polygons(text);
        let stride = w as usize;
        let mut coverage = vec![0u8; stride.saturating_mul(h as usize)];
        raster::scan_polygons(w, h, &polygons, x, y, scale, |px, py| {
            let idx = (py as usize)
                .saturating_mul(stride)
                .saturating_add(px as usize);
            if let Some(c) = coverage.get_mut(idx) {
                *c = 255;
            }
        });
        if let Some((rgba, ox, oy)) = outline {
            crate::renderer::effects::apply_outline_xy(buffer, &coverage, w, h, 0, 0, ox, oy, rgba);
        }
        if let Some((rgba, ox, oy)) = shadow {
            crate::renderer::effects::apply_shadow(buffer, &coverage, w, h, 0, 0, ox, oy, rgba);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered_pixels(text: &str, w: u32, h: u32, scale: f64) -> usize {
        let mut buf = RenderBuffer::new(w, h).unwrap();
        DrawingParser::render_drawing(&mut buf, text, 0.0, 0.0, scale, [255, 255, 255, 255]);
        buf.pixels.chunks_exact(4).filter(|p| p[3] > 0).count()
    }

    #[test]
    fn test_move_line_close_triangle() {
        assert!(rendered_pixels("m 10 10 l 50 10 l 30 40 c", 64, 64, 1.0) > 100);
    }

    /// Even-odd across contours (plan #27): a contour inside another
    /// punches a hole, regardless of winding direction.
    #[test]
    fn test_nested_contour_punches_hole() {
        let square = "m 10 10 l 50 10 l 50 50 l 10 50 ";
        // Same winding: inner square is a hole.
        let same = format!("{square}m 20 20 l 40 20 l 40 40 l 20 40");
        // Reversed winding: still a hole (even-odd, not nonzero).
        let reversed = format!("{square}m 20 20 l 20 40 l 40 40 l 40 20");
        for text in [&same, &reversed] {
            let mut buf = RenderBuffer::new(64, 64).unwrap();
            DrawingParser::render_drawing(&mut buf, text, 0.0, 0.0, 1.0, [255; 4]);
            assert_eq!(buf.get_pixel(15, 15)[3], 255, "{text}");
            assert_eq!(buf.get_pixel(30, 30)[3], 0, "{text}");
            assert_eq!(buf.get_pixel(5, 5)[3], 0, "{text}");
        }
    }

    /// Even-odd overlap (plan #27): doubly-covered region stays empty.
    #[test]
    fn test_overlapping_squares_cancel() {
        let mut buf = RenderBuffer::new(64, 64).unwrap();
        DrawingParser::render_drawing(
            &mut buf,
            "m 10 20 l 40 20 l 40 40 l 10 40 m 25 20 l 55 20 l 55 40 l 25 40",
            0.0,
            0.0,
            1.0,
            [255; 4],
        );
        // Left-only and right-only wings are filled...
        assert_eq!(buf.get_pixel(17, 30)[3], 255);
        assert_eq!(buf.get_pixel(47, 30)[3], 255);
        // ...but the doubly-covered middle cancels out.
        assert_eq!(buf.get_pixel(32, 30)[3], 0);
    }

    /// Self-intersecting bowtie (plan #27): even-odd fills BOTH lobes
    /// (a nonzero rule would leave one lobe empty).
    #[test]
    fn test_bowtie_self_intersection_fills_both_lobes() {
        let mut buf = RenderBuffer::new(64, 64).unwrap();
        DrawingParser::render_drawing(
            &mut buf,
            "m 10 10 l 50 50 l 10 50 l 50 10",
            0.0,
            0.0,
            1.0,
            [255; 4],
        );
        assert_eq!(buf.get_pixel(30, 20)[3], 255);
        assert_eq!(buf.get_pixel(30, 40)[3], 255);
        assert_eq!(buf.get_pixel(5, 30)[3], 0);
    }

    /// Hole through the alpha mask path (plan #27: vector clip mask).
    #[test]
    fn test_mask_punches_hole() {
        let mut buf = RenderBuffer::new(64, 64).unwrap();
        DrawingParser::render_mask(
            &mut buf,
            "m 10 10 l 50 10 l 50 50 l 10 50 m 20 20 l 40 20 l 40 40 l 20 40",
            0.0,
            0.0,
            1.0,
        );
        assert_eq!(buf.get_pixel(15, 15)[3], 255);
        assert_eq!(buf.get_pixel(30, 30)[3], 0);
    }

    /// Outline ring + drop shadow for event drawings (plan #28): the
    /// ring paints in outline color around the fill, the shadow stamps
    /// offset in shadow color, and the fill covers both in the core.
    #[test]
    fn test_drawing_effects_outline_and_shadow() {
        let square = "m 20 20 l 40 20 l 40 40 l 20 40";
        let mut buf = RenderBuffer::new(64, 64).unwrap();
        DrawingParser::render_drawing_effects(
            &mut buf,
            square,
            0.0,
            0.0,
            1.0,
            Some(([0, 0, 255, 255], 2.0, 2.0)),
            Some(([0, 255, 0, 255], 3.0, 0.0)),
        );
        DrawingParser::render_drawing(&mut buf, square, 0.0, 0.0, 1.0, [255, 255, 255, 255]);
        // Fill core covers both passes: opaque white.
        assert_eq!(buf.get_pixel(30, 30), [255, 255, 255, 255]);
        // Outline ring left of the fill: blue only.
        let ring = buf.get_pixel(18, 30);
        assert_eq!((ring[0], ring[1]), (0, 0), "ring pixel {ring:?}");
        assert!(ring[2] > 0 && ring[3] > 0, "ring pixel {ring:?}");
        // Shadow right of fill + ring (center sampling: fill spans
        // 20..=39, ring reaches x=41, shadowed fill spans 23..=42):
        // pure green, then blank.
        assert_eq!(buf.get_pixel(42, 30), [0, 255, 0, 255]);
        assert_eq!(buf.get_pixel(43, 30), [0, 0, 0, 0]);
        // Disabled effects leave the buffer untouched.
        let mut blank = RenderBuffer::new(64, 64).unwrap();
        DrawingParser::render_drawing_effects(&mut blank, square, 0.0, 0.0, 1.0, None, None);
        assert!(blank.pixels.iter().all(|b| *b == 0));
    }

    #[test]
    fn test_move_without_close_continues_outline() {
        // `n` keeps one outline (jump edge); `m` splits into two.
        let merged = DrawingParser::polygons("m 0 0 l 10 0 n 20 0 l 30 0 l 30 10");
        assert_eq!(merged.len(), 1);
        let split = DrawingParser::polygons("m 0 0 l 10 0 l 10 10 m 20 0 l 30 0 l 30 10");
        assert_eq!(split.len(), 2);
    }

    #[test]
    fn test_bezier_renders() {
        assert!(rendered_pixels("m 10 30 b 10 10 50 10 50 30 l 50 50 l 10 50 c", 64, 64, 1.0) > 50);
    }

    #[test]
    fn test_spline_extend_close() {
        // `s` spline, `p` extension, `c` close
        let n = rendered_pixels("m 10 30 s 20 10 40 10 50 30 p 55 40 50 50 c", 64, 64, 1.0);
        assert!(n > 20, "spline pixels: {}", n);
        let polys = DrawingParser::polygons("m 0 0 s 10 0 20 0 30 10 p 35 15 30 20 c");
        assert_eq!(polys.len(), 1);
        assert!(polys[0].len() > 8);
    }

    #[test]
    fn test_measure_bbox() {
        let (min_x, min_y, w, h) = DrawingParser::measure("m 10 20 l 50 20 l 50 60").unwrap();
        assert_eq!((min_x, min_y), (10.0, 20.0));
        assert_eq!((w, h), (40.0, 40.0));
        assert!(DrawingParser::measure("").is_none());
        assert!(DrawingParser::measure("m 0 0").is_none());
    }

    #[test]
    fn test_mask_writes_alpha_only_inside() {
        let mut buf = RenderBuffer::new(64, 64).unwrap();
        DrawingParser::render_mask(&mut buf, "m 10 10 l 50 10 l 50 50 l 10 50", 0.0, 0.0, 1.0);
        assert_eq!(buf.get_pixel(30, 30)[3], 255);
        assert_eq!(buf.get_pixel(0, 0)[3], 0);
    }

    #[test]
    fn test_malicious_coordinates_bounded() {
        // Extreme coordinates: terminates, no panic, no hang.
        let mut buf = RenderBuffer::new(32, 32).unwrap();
        DrawingParser::render_drawing(
            &mut buf,
            "m -1e18 -1e18 l 1e18 -1e18 l 1e18 1e18 l -1e18 1e18",
            0.0,
            0.0,
            1.0,
            [255, 255, 255, 255],
        );
        DrawingParser::render_drawing(&mut buf, "m 0 0 l NaN 5 l 5 5", 0.0, 0.0, 1.0, [255; 4]);
        // Coordinates overflowing to infinity are rejected (no panic).
        let huge = "9".repeat(400);
        assert!(DrawingParser::measure(&format!("m 0 0 l {} 5 l 5 5", huge)).is_none());
    }

    #[test]
    fn test_degenerate_inputs_safe() {
        let mut buf = RenderBuffer::new(16, 16).unwrap();
        DrawingParser::render_drawing(&mut buf, "m 5 5 l 6 6", 0.0, 0.0, f64::NAN, [255; 4]);
        DrawingParser::render_drawing(&mut buf, "m 5 5 l 6 6", 0.0, 0.0, -1.0, [255; 4]);
        DrawingParser::render_drawing(&mut buf, "", 0.0, 0.0, 1.0, [255; 4]);
        assert!(buf.pixels.iter().all(|&p| p == 0));
    }
}
