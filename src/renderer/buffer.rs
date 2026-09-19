use crate::utils::Matrix3x3;
#[path = "buffer_geometry.rs"]
mod buffer_geometry;
pub use super::limits::{
    MAX_BLUR_RADIUS, MAX_BUFFER_PIXELS, MAX_DIMENSION, MAX_GLYPH_BITMAP_PIXELS, MAX_OUTLINE_RADIUS,
};
pub use buffer_geometry::{
    add_coord, bitmap_has_pixels, checked_pixel_count, effective_shear, finite_to_i32,
    finite_to_u32, shear_forward, shear_inverse,
};
#[path = "buffer/blur.rs"]
mod blur;

/// Errors from render-buffer allocation and sizing.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum RenderError {
    #[error("Invalid render dimensions {width}x{height}: dimensions must be non-zero")]
    ZeroDimensions { width: u32, height: u32 },
    #[error("Render dimensions {width}x{height} exceed the {MAX_DIMENSION}px limit")]
    DimensionsTooLarge { width: u32, height: u32 },
    #[error("Render dimensions {width}x{height} exceed the allocation limit")]
    AllocationTooLarge { width: u32, height: u32 },
}

/// Validate dimensions with checked arithmetic and return the RGBA byte length.
fn checked_buffer_len(width: u32, height: u32) -> Result<usize, RenderError> {
    if width == 0 || height == 0 {
        return Err(RenderError::ZeroDimensions { width, height });
    }
    if width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(RenderError::DimensionsTooLarge { width, height });
    }
    let pixels = (width as u64)
        .checked_mul(height as u64)
        .filter(|&p| p <= MAX_BUFFER_PIXELS)
        .ok_or(RenderError::AllocationTooLarge { width, height })?;
    let bytes = pixels
        .checked_mul(4)
        .ok_or(RenderError::AllocationTooLarge { width, height })?;
    usize::try_from(bytes).map_err(|_| RenderError::AllocationTooLarge { width, height })
}

/// RGBA render buffer for subtitle compositing.
///
/// Invariant: `pixels.len() == width * height * 4` with both dimensions
/// within [`MAX_DIMENSION`] and the pixel count within
/// [`MAX_BUFFER_PIXELS`]. `new`/`resize` enforce this; direct field
/// writes that break it can cause panics in pixel methods, so downstream
/// users must preserve it (treat the fields as read-mostly).
#[derive(Debug)]
pub struct RenderBuffer {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl RenderBuffer {
    /// Create a buffer, rejecting invalid or unreasonably large dimensions.
    pub fn new(width: u32, height: u32) -> Result<Self, RenderError> {
        let len = checked_buffer_len(width, height)?;
        Ok(Self {
            width,
            height,
            pixels: vec![0u8; len],
        })
    }

    /// Create a buffer for trusted internal dimensions, falling back to a
    /// 1x1 buffer rather than panicking. Prefer [`RenderBuffer::new`].
    pub fn new_or_empty(width: u32, height: u32) -> Self {
        Self::new(width, height).unwrap_or(Self {
            width: 1,
            height: 1,
            pixels: vec![0u8; 4],
        })
    }

    pub fn clear(&mut self) {
        self.pixels.fill(0);
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), RenderError> {
        let len = checked_buffer_len(width, height)?;
        self.width = width;
        self.height = height;
        self.pixels.resize(len, 0);
        self.clear();
        Ok(())
    }

    /// Get pixel at (x, y) as [R, G, B, A]
    #[inline]
    pub fn get_pixel(&self, x: u32, y: u32) -> [u8; 4] {
        if x >= self.width || y >= self.height {
            return [0, 0, 0, 0];
        }
        let idx = ((y * self.width + x) * 4) as usize;
        [
            self.pixels[idx],
            self.pixels[idx + 1],
            self.pixels[idx + 2],
            self.pixels[idx + 3],
        ]
    }

    /// Set pixel at (x, y) with RGBA values
    #[inline]
    pub fn set_pixel(&mut self, x: u32, y: u32, r: u8, g: u8, b: u8, a: u8) {
        if x >= self.width || y >= self.height {
            return;
        }
        let idx = ((y * self.width + x) * 4) as usize;
        self.pixels[idx] = r;
        self.pixels[idx + 1] = g;
        self.pixels[idx + 2] = b;
        self.pixels[idx + 3] = a;
    }

    /// Blend a pixel with alpha compositing (source-over)
    #[inline]
    #[allow(clippy::manual_checked_ops)]
    pub fn blend_pixel(&mut self, x: u32, y: u32, r: u8, g: u8, b: u8, a: u8) {
        if x >= self.width || y >= self.height || a == 0 {
            return;
        }
        let idx = ((y * self.width + x) * 4) as usize;
        let src_a = a as u32;
        let dst_a = self.pixels[idx + 3] as u32;
        let out_a = src_a + dst_a * (255 - src_a) / 255;

        if out_a > 0 {
            // Integer-division floors let the numerator exceed 255 *
            // out_a (up to ~2x) when out_a is tiny (e.g. src_a=1 over
            // dst_a=2 gives 763/2=381), so saturate: a bare `as u8`
            // would wrap mod 256 and render near-black for white.
            let inv_src = 255 - src_a;
            self.pixels[idx] =
                ((r as u32 * src_a + self.pixels[idx] as u32 * dst_a * inv_src / 255) / out_a)
                    .min(255) as u8;
            self.pixels[idx + 1] =
                ((g as u32 * src_a + self.pixels[idx + 1] as u32 * dst_a * inv_src / 255) / out_a)
                    .min(255) as u8;
            self.pixels[idx + 2] =
                ((b as u32 * src_a + self.pixels[idx + 2] as u32 * dst_a * inv_src / 255) / out_a)
                    .min(255) as u8;
            self.pixels[idx + 3] = ((out_a * 255 + 128) / 255) as u8;
        }
    }

    /// Blend premultiplied alpha pixel
    #[inline]
    pub fn blend_pixel_premul(&mut self, x: u32, y: u32, r: u8, g: u8, b: u8, a: u8) {
        if x >= self.width || y >= self.height || a == 0 {
            return;
        }
        let idx = ((y * self.width + x) * 4) as usize;
        let inv = 255 - a as u32;

        // Each sum can reach 510 (e.g. white over white), so clamp:
        // a bare `as u8` would wrap mod 256 instead of saturating.
        self.pixels[idx] = (r as u32 + self.pixels[idx] as u32 * inv / 255).min(255) as u8;
        self.pixels[idx + 1] = (g as u32 + self.pixels[idx + 1] as u32 * inv / 255).min(255) as u8;
        self.pixels[idx + 2] = (b as u32 + self.pixels[idx + 2] as u32 * inv / 255).min(255) as u8;
        self.pixels[idx + 3] = (a as u32 + self.pixels[idx + 3] as u32 * inv / 255).min(255) as u8;
    }

    /// Composite another straight-alpha RGBA buffer over this one.
    pub fn blend_buffer(&mut self, source: &RenderBuffer) {
        if self.width != source.width || self.height != source.height {
            return;
        }

        for y in 0..self.height {
            for x in 0..self.width {
                let idx = ((y * self.width + x) * 4) as usize;
                let alpha = source.pixels[idx + 3];
                if alpha == 0 {
                    continue;
                }
                self.blend_pixel(
                    x,
                    y,
                    source.pixels[idx],
                    source.pixels[idx + 1],
                    source.pixels[idx + 2],
                    alpha,
                );
            }
        }
    }

    /// Fill a rectangle with color (source-over blend per pixel).
    #[allow(clippy::too_many_arguments)]
    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, r: u8, g: u8, b: u8, a: u8) {
        if a == 0 {
            return;
        }
        let Some((x0, y0, x1, y1)) = self.rect_intersection(x, y, w, h) else {
            return;
        };

        for py in y0..y1 {
            for px in x0..x1 {
                self.blend_pixel(px, py, r, g, b, a);
            }
        }
    }

    /// Get raw pixel slice for ImageData creation
    pub fn as_bytes(&self) -> &[u8] {
        &self.pixels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_buffer() {
        let buf = RenderBuffer::new(100, 100).unwrap();
        assert_eq!(buf.width, 100);
        assert_eq!(buf.height, 100);
        assert_eq!(buf.pixels.len(), 40000);
    }

    #[test]
    fn test_new_rejects_zero_dimensions() {
        assert_eq!(
            RenderBuffer::new(0, 100).unwrap_err(),
            RenderError::ZeroDimensions {
                width: 0,
                height: 100
            }
        );
        assert_eq!(
            RenderBuffer::new(100, 0).unwrap_err(),
            RenderError::ZeroDimensions {
                width: 100,
                height: 0
            }
        );
    }

    #[test]
    fn test_new_rejects_huge_dimensions() {
        assert!(RenderBuffer::new(u32::MAX, u32::MAX).is_err());
        assert!(RenderBuffer::new(u32::MAX, 2).is_err());
        assert!(RenderBuffer::new(2, u32::MAX).is_err());
        // 20000 x 20000 exceeds MAX_DIMENSION
        assert!(matches!(
            RenderBuffer::new(20_000, 20_000).unwrap_err(),
            RenderError::DimensionsTooLarge { .. }
        ));
        // Within per-axis limits but over the pixel budget
        assert!(matches!(
            RenderBuffer::new(16_384, 16_384).unwrap_err(),
            RenderError::AllocationTooLarge { .. }
        ));
    }

    #[test]
    fn test_resize_rejects_invalid() {
        let mut buf = RenderBuffer::new(8, 8).unwrap();
        assert!(buf.resize(0, 8).is_err());
        assert!(buf.resize(u32::MAX, u32::MAX).is_err());
        // Failed resize leaves the old buffer untouched
        assert_eq!((buf.width, buf.height), (8, 8));
        assert_eq!(buf.pixels.len(), 8 * 8 * 4);
    }

    #[test]
    fn test_set_get_pixel() {
        let mut buf = RenderBuffer::new(10, 10).unwrap();
        buf.set_pixel(5, 5, 255, 128, 64, 200);
        let px = buf.get_pixel(5, 5);
        assert_eq!(px, [255, 128, 64, 200]);
    }

    #[test]
    fn test_blend_pixel() {
        let mut buf = RenderBuffer::new(10, 10).unwrap();
        buf.set_pixel(5, 5, 0, 0, 0, 128);
        buf.blend_pixel(5, 5, 255, 255, 255, 128);
        let px = buf.get_pixel(5, 5);
        assert!(px[3] > 128);
    }

    #[test]
    fn test_blend_pixel_saturates_tiny_alpha() {
        // src_a=1 white over dst_a=2 white: the quotient reaches 381
        // (integer-division floors), which must saturate to 255, not
        // wrap mod 256 to near-black.
        let mut buf = RenderBuffer::new(2, 1).unwrap();
        buf.set_pixel(0, 0, 255, 255, 255, 2);
        buf.blend_pixel(0, 0, 255, 255, 255, 1);
        assert_eq!(buf.get_pixel(0, 0), [255, 255, 255, 2]);
    }

    #[test]
    fn test_blend_pixel_premul_saturates() {
        // White over white sums past 255 per channel: must saturate,
        // not wrap mod 256.
        let mut buf = RenderBuffer::new(2, 1).unwrap();
        buf.set_pixel(0, 0, 255, 255, 255, 255);
        buf.blend_pixel_premul(0, 0, 255, 255, 255, 255);
        assert_eq!(buf.get_pixel(0, 0), [255, 255, 255, 255]);
    }

    #[test]
    fn test_blend_buffer_preserves_existing_pixels() {
        let mut destination = RenderBuffer::new(2, 1).unwrap();
        destination.set_pixel(0, 0, 255, 0, 0, 255);

        let mut source = RenderBuffer::new(2, 1).unwrap();
        source.set_pixel(1, 0, 0, 255, 0, 255);
        destination.blend_buffer(&source);

        assert_eq!(destination.get_pixel(0, 0), [255, 0, 0, 255]);
        assert_eq!(destination.get_pixel(1, 0), [0, 255, 0, 255]);
    }

    #[test]
    fn test_resize_coverage_bitmap() {
        let bitmap = vec![255u8, 0, 0, 255];
        let (scaled, width, height) = RenderBuffer::resize_coverage_bitmap(&bitmap, 2, 2, 2.0, 1.5);
        assert_eq!((width, height), (4, 3));
        assert_eq!(scaled.len(), 12);
        assert!(scaled.iter().any(|value| *value > 0));
    }

    #[test]
    fn test_resize_coverage_bitmap_rejects_degenerate() {
        let bitmap = vec![255u8; 16];
        assert_eq!(
            RenderBuffer::resize_coverage_bitmap(&bitmap, 4, 4, f64::NAN, 1.0).1,
            0
        );
        assert_eq!(
            RenderBuffer::resize_coverage_bitmap(&bitmap, 4, 4, 1.0, f64::INFINITY).1,
            0
        );
        assert_eq!(
            RenderBuffer::resize_coverage_bitmap(&bitmap, 4, 4, 1e300, 1e300).1,
            0
        );
    }

    #[test]
    fn test_fill_rect_clamps_malicious_coordinates() {
        let mut buf = RenderBuffer::new(16, 16).unwrap();
        // Must terminate quickly and never panic
        buf.fill_rect(
            -2_000_000_000,
            -2_000_000_000,
            2_000_000_000,
            2_000_000_000,
            255,
            0,
            0,
            255,
        );
        buf.fill_rect(i32::MIN, i32::MIN, i32::MAX, i32::MAX, 0, 255, 0, 255);
        buf.fill_rect(i32::MAX - 5, i32::MAX - 5, 100, 100, 0, 0, 255, 255);
        buf.fill_rect(-100, -100, -50, -50, 255, 255, 255, 255);
        buf.fill_rect(4, 4, -8, -8, 255, 255, 255, 255);
        buf.fill_rect(100, 100, 50, 50, 255, 255, 255, 255);
        buf.fill_rect(-50, 4, 10, 8, 255, 255, 255, 255);
        // Nothing painted: buffer stays transparent
        assert!(buf.pixels.iter().all(|&p| p == 0));
    }

    #[test]
    fn test_fill_rect_partial_overlap() {
        let mut buf = RenderBuffer::new(16, 16).unwrap();
        buf.fill_rect(-4, -4, 8, 8, 255, 0, 0, 255);
        assert_eq!(buf.get_pixel(0, 0), [255, 0, 0, 255]);
        assert_eq!(buf.get_pixel(3, 3), [255, 0, 0, 255]);
        assert_eq!(buf.get_pixel(4, 4), [0, 0, 0, 0]);
    }

    #[test]
    fn test_fill_rect_overflow_addition() {
        // x + w overflows i32: must not panic or wrap into the buffer
        let mut buf = RenderBuffer::new(16, 16).unwrap();
        buf.fill_rect(i32::MAX - 10, 0, 100, 16, 255, 0, 0, 255);
        buf.fill_rect(0, i32::MAX - 10, 16, 100, 255, 0, 0, 255);
        assert!(buf.pixels.iter().all(|&p| p == 0));
    }

    #[test]
    fn test_projective_transform_rejects_degenerate() {
        let bitmap = vec![128u8; 16];
        let m = Matrix3x3::identity();
        let (out, w, h, _, _) = RenderBuffer::projective_transform_coverage_bitmap(
            &bitmap,
            4,
            4,
            &m,
            f64::NAN,
            (0.0, 0.0),
            (0.0, 0.0),
        );
        assert!(out.is_empty() && w == 0 && h == 0);
        let (out, w, h, _, _) = RenderBuffer::projective_transform_coverage_bitmap(
            &bitmap,
            4,
            4,
            &m,
            0.0,
            (0.0, 0.0),
            (0.0, 0.0),
        );
        assert!(out.is_empty() && w == 0 && h == 0);
        let bad = Matrix3x3([f64::NAN; 9]);
        let (out, _, _, _, _) = RenderBuffer::projective_transform_coverage_bitmap(
            &bitmap,
            4,
            4,
            &bad,
            500.0,
            (0.0, 0.0),
            (0.0, 0.0),
        );
        assert!(out.is_empty());
        // Non-finite shear rejected.
        let (out, _, _, _, _) = RenderBuffer::projective_transform_coverage_bitmap(
            &bitmap,
            4,
            4,
            &m,
            500.0,
            (f64::NAN, 0.0),
            (0.0, 0.0),
        );
        assert!(out.is_empty());
        // Non-finite pivot rejected.
        let (out, _, _, _, _) = RenderBuffer::projective_transform_coverage_bitmap(
            &bitmap,
            4,
            4,
            &m,
            500.0,
            (1.0, 0.0),
            (f64::NAN, 0.0),
        );
        assert!(out.is_empty());
    }

    #[test]
    fn test_projective_frx_fry_compress_along_tilt_axis() {
        // Two-pixel impulse pair 12px apart: under a 30-degree tilt the
        // peaks move TOWARD each other (~12*cos30 = 10.4px apart). The
        // old inverse map (third matrix row instead of column) pushed
        // them apart (~15px); frz is unaffected either way (row == col).
        for (axis, m) in [
            ("frx", Matrix3x3::rotation_x((-30f64).to_radians())),
            ("fry", Matrix3x3::rotation_y((-30f64).to_radians())),
        ] {
            let mut bmp = vec![0u8; 21 * 21];
            if axis == "frx" {
                bmp[4 * 21 + 10] = 255;
                bmp[16 * 21 + 10] = 255;
            } else {
                bmp[10 * 21 + 4] = 255;
                bmp[10 * 21 + 16] = 255;
            }
            let (out, ow, oh, _, _) = RenderBuffer::projective_transform_coverage_bitmap(
                &bmp,
                21,
                21,
                &m,
                312.5,
                (0.0, 0.0),
                (0.0, 0.0),
            );
            assert_eq!(out.len(), ow as usize * oh as usize);
            // Peak rows/cols along the tilt axis (max coverage per line).
            let span = if axis == "frx" { oh } else { ow };
            let mut peaks = Vec::new();
            for i in 0..span {
                let mut best = 0u8;
                for j in 0..(if axis == "frx" { ow } else { oh }) {
                    let idx = if axis == "frx" {
                        i as usize * ow as usize + j as usize
                    } else {
                        j as usize * ow as usize + i as usize
                    };
                    best = best.max(out[idx]);
                }
                if best >= 128 {
                    peaks.push(i);
                }
            }
            assert!(
                peaks.len() >= 2,
                "{axis}: impulses must survive the warp ({peaks:?})"
            );
            let dist = peaks[peaks.len() - 1] - peaks[0];
            assert!(
                (8..=12).contains(&dist),
                "{axis}: impulse pair must compress toward ~10.4px, got {dist} ({peaks:?})"
            );
        }
    }

    #[test]
    fn test_projective_transform_extreme_rotation_bounded() {
        let bitmap = vec![200u8; 64];
        let m = Matrix3x3::rotation_x(89.9f64.to_radians())
            .multiply(&Matrix3x3::rotation_y(89.9f64.to_radians()));
        let (out, w, h, _, _) = RenderBuffer::projective_transform_coverage_bitmap(
            &bitmap,
            8,
            8,
            &m,
            500.0,
            (0.0, 0.0),
            (0.0, 0.0),
        );
        assert!(w as u64 * h as u64 <= MAX_GLYPH_BITMAP_PIXELS);
        assert_eq!(out.len(), w as usize * h as usize);
    }

    #[test]
    fn test_projective_identity_shear_matches_bitmap_shear() {
        // With identity rotation, folded pre-rotation shear must equal
        // a plain bitmap shear around the pivot: fax=1 on 4x4 around
        // its center grows width by height and slants content
        // rightward with increasing row.
        let mut bitmap = vec![0u8; 16];
        for y in 0..4 {
            bitmap[y * 4] = 255; // left column filled
        }
        let m = Matrix3x3::identity();
        let (out, w, h, _, _) = RenderBuffer::projective_transform_coverage_bitmap(
            &bitmap,
            4,
            4,
            &m,
            500.0,
            (1.0, 0.0),
            (2.0, 2.0),
        );
        // Span 8x4 plus the inclusive +1 bound.
        assert_eq!((w, h), (9, 5));
        assert_eq!(out.len(), 45);
        // Peak column of each non-empty row increases with the row
        // index (padding rows from the +1 bound stay empty).
        let wu = w as usize;
        let mut peaks: Vec<(usize, usize)> = Vec::new();
        for y in 0..h as usize {
            let row = &out[y * wu..(y + 1) * wu];
            let peak = row
                .iter()
                .enumerate()
                .max_by_key(|(_, v)| **v)
                .map(|(x, _)| x)
                .unwrap();
            if row[peak] > 0 {
                peaks.push((y, peak));
            }
        }
        assert!(peaks.len() >= 3, "sheared bar must span rows");
        for w2 in peaks.windows(2) {
            assert!(
                w2[1].1 >= w2[0].1,
                "shear must slant rightward with row (row {} peak {} < row {} peak {})",
                w2[1].0,
                w2[1].1,
                w2[0].0,
                w2[0].1
            );
        }
        assert!(
            peaks.last().unwrap().1 > peaks.first().unwrap().1,
            "bottom of bar must shift right of its top"
        );
        // Zero shear keeps the span-sized output (4x4 span + 1).
        let (out, w, h, _, _) = RenderBuffer::projective_transform_coverage_bitmap(
            &bitmap,
            4,
            4,
            &m,
            500.0,
            (0.0, 0.0),
            (0.0, 0.0),
        );
        assert_eq!((w, h), (5, 5));
        assert!(out.iter().any(|&v| v > 0));
    }

    #[test]
    fn test_projective_shear_pivot_row_stays_fixed() {
        // The pivot row/column is the fixed line of the shear: fax=1
        // around pivot (px, 1.0) leaves row 1 unshifted while rows
        // above/below move in opposite directions.
        let mut bitmap = vec![0u8; 16];
        for y in 0..4 {
            bitmap[y * 4] = 255;
        }
        let m = Matrix3x3::identity();
        let (out, w, h, ox, oy) = RenderBuffer::projective_transform_coverage_bitmap(
            &bitmap,
            4,
            4,
            &m,
            500.0,
            (1.0, 0.0),
            (0.0, 1.0),
        );
        // Corners (0..4) shear to x -1..7: span 8 + 1 = 9 wide.
        // Offsets are center-relative (cx = cy = 2): min_x = -3,
        // min_y = -2.
        assert_eq!((w, h), (9, 5));
        assert_eq!((ox, oy), (-3, -2));
        // Absolute source row y lands on output row y - cy - oy = y,
        // and the bar sheared to absolute x (y - 1) lands on output
        // column (y - 1) - cx - ox = y. Assert the bar position in
        // center-relative output coords (peak + ox): row 1 (the pivot
        // row) sits at the unshifted -2 while rows 0/3 slant ∓.
        let wu = w as usize;
        for (y, expect_rel_x) in [(0i32, -3i32), (1, -2), (3, 0)] {
            let out_row = (y - 2 - oy) as usize;
            let row = &out[out_row * wu..(out_row + 1) * wu];
            let peak = row
                .iter()
                .enumerate()
                .max_by_key(|(_, v)| **v)
                .map(|(x, _)| x as i32)
                .unwrap();
            assert!(
                (peak + ox - expect_rel_x).abs() <= 1,
                "row {y} peak {peak} (rel {}), expected rel ~{expect_rel_x}",
                peak + ox
            );
        }
    }

    #[test]
    fn test_projective_shear_applies_pre_rotation() {
        // Key order test (libass `calc_transform_matrix`, VSFilter
        // `Transform_C`, pixel-verified): with frz=90° + fax=1 the
        // shear slants the glyph FIRST and the slant rotates second.
        // Shearing the 4x4 corners around (2,2) spans x -4..4, which
        // the 90° rotation turns into a TALL 5x9 output (bounds
        // (-2,-4)..(2,4) + 1). Post-rotation shear would give 9x5
        // instead — so the dimensions alone prove the order.
        let mut bitmap = vec![0u8; 16];
        bitmap[0] = 255; // single top-left pixel
        let m = Matrix3x3::rotation_z(90.0f64.to_radians());
        let (out, w, h, ox, oy) = RenderBuffer::projective_transform_coverage_bitmap(
            &bitmap,
            4,
            4,
            &m,
            500.0,
            (1.0, 0.0),
            (2.0, 2.0),
        );
        assert_eq!((w, h), (5, 9), "pre-rotation bounds must be 5x9");
        assert_eq!((ox, oy), (-2, -4));
        // Forward map of the pixel center rel (-1.5,-1.5): shear
        // around (2,2) in absolute coords (0.5,0.5) -> x + (y-2):
        // (-1.0, 0.5), rel (-3,-1.5); rotate 90° (x,y)->(-y,x):
        // (1.5,-3). Output: (1.5+2, -3+4) = (3.5,1) -> peak (3..4,1).
        let wu = w as usize;
        let peak = out
            .iter()
            .enumerate()
            .max_by_key(|(_, v)| **v)
            .map(|(i, _)| (i % wu, i / wu))
            .unwrap();
        assert!(out[peak.1 * wu + peak.0] > 0);
        assert!(
            (3..=4).contains(&peak.0) && peak.1 <= 2,
            "peak must land at ~(3,1) under pre-rotation shear, got {peak:?}"
        );
        // Rotation-only output differs from shear+rotation output.
        let (plain, _, _, _, _) = RenderBuffer::projective_transform_coverage_bitmap(
            &bitmap,
            4,
            4,
            &m,
            500.0,
            (0.0, 0.0),
            (0.0, 0.0),
        );
        assert_ne!(out, plain);
    }

    #[test]
    fn test_effective_shear_clamps_and_guards_singular() {
        assert_eq!(effective_shear((0.0, 0.0)), Some((0.0, 0.0)));
        assert_eq!(effective_shear((100.0, -100.0)), Some((8.0, -8.0)));
        assert_eq!(effective_shear((f64::NAN, 0.0)), None);
        assert_eq!(effective_shear((0.0, f64::INFINITY)), None);
        // Singular pair (fax*fay == 1) falls back to fax-only.
        assert_eq!(effective_shear((2.0, 0.5)), Some((2.0, 0.0)));
        // Forward/inverse round-trip around a pivot.
        let (x, y) = shear_forward(3.0, -2.0, 1.5, 0.25, (1.0, -1.0));
        let (rx, ry) = shear_inverse(x, y, 1.5, 0.25, (1.0, -1.0)).unwrap();
        assert!((rx - 3.0).abs() < 1e-9 && (ry + 2.0).abs() < 1e-9);
        assert_eq!(shear_inverse(1.0, 1.0, 2.0, 0.5, (0.0, 0.0)), None);
        // The pivot itself is the fixed point.
        assert_eq!(
            shear_forward(1.0, -1.0, 1.5, 0.25, (1.0, -1.0)),
            (1.0, -1.0)
        );
    }

    #[test]
    fn test_box_blur_clamps_huge_radius() {
        let mut buf = RenderBuffer::new(8, 8).unwrap();
        buf.set_pixel(4, 4, 255, 255, 255, 255);
        buf.box_blur(u32::MAX); // must terminate via clamping
        assert_eq!(buf.pixels.len(), 8 * 8 * 4);
    }
}
