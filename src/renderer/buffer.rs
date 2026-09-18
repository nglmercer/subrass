use crate::utils::Matrix3x3;

/// Maximum allowed frame-buffer dimension (width or height) in pixels.
pub const MAX_DIMENSION: u32 = 16_384;
/// Maximum total pixels in a single frame buffer (64 megapixels ≈ 256 MiB RGBA).
pub const MAX_BUFFER_PIXELS: u64 = 67_108_864;
/// Maximum total pixels in a transformed/scaled glyph coverage bitmap.
pub const MAX_GLYPH_BITMAP_PIXELS: u64 = 16_777_216;
/// Maximum box-blur radius applied to a buffer.
pub const MAX_BLUR_RADIUS: u32 = 128;
/// Maximum outline radius used by effect loops.
pub const MAX_OUTLINE_RADIUS: f64 = 128.0;

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

/// RGBA render buffer for subtitle compositing
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
            let inv_src = 255 - src_a;
            self.pixels[idx] = ((r as u32 * src_a
                + self.pixels[idx] as u32 * dst_a * inv_src / 255)
                / out_a) as u8;
            self.pixels[idx + 1] = ((g as u32 * src_a
                + self.pixels[idx + 1] as u32 * dst_a * inv_src / 255)
                / out_a) as u8;
            self.pixels[idx + 2] = ((b as u32 * src_a
                + self.pixels[idx + 2] as u32 * dst_a * inv_src / 255)
                / out_a) as u8;
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

        self.pixels[idx] = (r as u32 + self.pixels[idx] as u32 * inv / 255) as u8;
        self.pixels[idx + 1] = (g as u32 + self.pixels[idx + 1] as u32 * inv / 255) as u8;
        self.pixels[idx + 2] = (b as u32 + self.pixels[idx + 2] as u32 * inv / 255) as u8;
        self.pixels[idx + 3] = (a as u32 + self.pixels[idx + 3] as u32 * inv / 255) as u8;
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

    /// Resize a glyph coverage bitmap using bilinear sampling.
    ///
    /// Non-finite or non-positive scales, empty sources, and outputs that
    /// would exceed [`MAX_GLYPH_BITMAP_PIXELS`] yield an empty bitmap.
    pub fn resize_coverage_bitmap(
        bitmap: &[u8],
        width: u32,
        height: u32,
        scale_x: f64,
        scale_y: f64,
    ) -> (Vec<u8>, u32, u32) {
        if !scale_x.is_finite() || !scale_y.is_finite() {
            return (Vec::new(), 0, 0);
        }
        let scale_x = scale_x.max(0.0);
        let scale_y = scale_y.max(0.0);
        if scale_x == 0.0 || scale_y == 0.0 || width == 0 || height == 0 {
            return (Vec::new(), 0, 0);
        }
        if (scale_x - 1.0).abs() < f64::EPSILON && (scale_y - 1.0).abs() < f64::EPSILON {
            if bitmap.len() < (width as u64 * height as u64).min(usize::MAX as u64) as usize {
                return (Vec::new(), 0, 0);
            }
            return (bitmap.to_vec(), width, height);
        }
        let new_width_f = width as f64 * scale_x;
        let new_height_f = height as f64 * scale_y;
        if !new_width_f.is_finite() || !new_height_f.is_finite() {
            return (Vec::new(), 0, 0);
        }
        let new_width = new_width_f.round().clamp(1.0, u32::MAX as f64) as u32;
        let new_height = new_height_f.round().clamp(1.0, u32::MAX as f64) as u32;
        let out_pixels = new_width as u64 * new_height as u64;
        if out_pixels == 0 || out_pixels > MAX_GLYPH_BITMAP_PIXELS {
            return (Vec::new(), 0, 0);
        }
        if bitmap.len() < width as usize * height as usize {
            return (Vec::new(), 0, 0);
        }

        let mut resized = vec![0u8; (new_width as usize) * (new_height as usize)];
        for y in 0..new_height {
            let source_y = ((y as f64 + 0.5) / scale_y - 0.5).clamp(0.0, height as f64 - 1.0);
            let y0 = source_y.floor() as u32;
            let y1 = (y0 + 1).min(height - 1);
            let fy = source_y - y0 as f64;

            for x in 0..new_width {
                let source_x = ((x as f64 + 0.5) / scale_x - 0.5).clamp(0.0, width as f64 - 1.0);
                let x0 = source_x.floor() as u32;
                let x1 = (x0 + 1).min(width - 1);
                let fx = source_x - x0 as f64;

                let sample =
                    |sx: u32, sy: u32| -> f64 { bitmap[(sy * width + sx) as usize] as f64 };
                let top = sample(x0, y0) * (1.0 - fx) + sample(x1, y0) * fx;
                let bottom = sample(x0, y1) * (1.0 - fx) + sample(x1, y1) * fx;
                resized[(y * new_width + x) as usize] =
                    (top * (1.0 - fy) + bottom * fy).round().clamp(0.0, 255.0) as u8;
            }
        }

        (resized, new_width, new_height)
    }

    /// Shear a glyph coverage bitmap (`\fax`/`\fay` factors) with
    /// bilinear resampling. The output is sized to contain the sheared
    /// image. Non-finite factors, empty sources, and outputs over
    /// [`MAX_GLYPH_BITMAP_PIXELS`] yield an empty bitmap.
    pub fn shear_coverage_bitmap(
        bitmap: &[u8],
        width: u32,
        height: u32,
        shear_x: f64,
        shear_y: f64,
    ) -> (Vec<u8>, u32, u32) {
        if !shear_x.is_finite() || !shear_y.is_finite() {
            return (Vec::new(), 0, 0);
        }
        let shear_x = shear_x.clamp(-8.0, 8.0);
        let shear_y = shear_y.clamp(-8.0, 8.0);
        if width == 0 || height == 0 {
            return (Vec::new(), 0, 0);
        }
        if bitmap.len() < width as usize * height as usize {
            return (Vec::new(), 0, 0);
        }
        if shear_x == 0.0 && shear_y == 0.0 {
            return (bitmap.to_vec(), width, height);
        }
        // Output bounds: dst = src + shear * other_axis, shifted so the
        // minimum lands at 0.
        let ox = (-shear_x * height as f64).max(0.0);
        let oy = (-shear_y * width as f64).max(0.0);
        let out_w = (width as f64 + shear_x.abs() * height as f64).ceil();
        let out_h = (height as f64 + shear_y.abs() * width as f64).ceil();
        if !out_w.is_finite() || !out_h.is_finite() {
            return (Vec::new(), 0, 0);
        }
        let out_w = out_w.clamp(1.0, u32::MAX as f64) as u32;
        let out_h = out_h.clamp(1.0, u32::MAX as f64) as u32;
        if out_w as u64 * out_h as u64 > MAX_GLYPH_BITMAP_PIXELS {
            return (Vec::new(), 0, 0);
        }
        let mut out = vec![0u8; out_w as usize * out_h as usize];
        for dy in 0..out_h {
            for dx in 0..out_w {
                // Inverse map (single pass; exact for axis shears).
                let sx = dx as f64 - ox - shear_x * (dy as f64 - oy);
                let sy = dy as f64 - oy - shear_y * (dx as f64 - ox);
                let x0 = sx.floor() as i32;
                let y0 = sy.floor() as i32;
                let fx = (sx - sx.floor()).clamp(0.0, 1.0);
                let fy = (sy - sy.floor()).clamp(0.0, 1.0);
                let sample = |x: i32, y: i32| -> f64 {
                    if x >= 0 && x < width as i32 && y >= 0 && y < height as i32 {
                        bitmap[(y as u32 * width + x as u32) as usize] as f64
                    } else {
                        0.0
                    }
                };
                let v = sample(x0, y0) * (1.0 - fx) * (1.0 - fy)
                    + sample(x0 + 1, y0) * fx * (1.0 - fy)
                    + sample(x0, y0 + 1) * (1.0 - fx) * fy
                    + sample(x0 + 1, y0 + 1) * fx * fy;
                out[(dy * out_w + dx) as usize] = v.clamp(0.0, 255.0) as u8;
            }
        }
        (out, out_w, out_h)
    }

    /// Intersect the rectangle (x, y, w, h) with the buffer bounds.
    /// Returns `None` when the intersection is empty. Handles negative
    /// origins, negative sizes, and i32 overflow via i64 arithmetic.
    fn rect_intersection(&self, x: i32, y: i32, w: i32, h: i32) -> Option<(u32, u32, u32, u32)> {
        if w <= 0 || h <= 0 {
            return None;
        }
        let x0 = (x as i64).max(0);
        let y0 = (y as i64).max(0);
        // x + w computed in i64: no overflow, and clamping before any u32 cast.
        let x1 = (x as i64 + w as i64).min(self.width as i64);
        let y1 = (y as i64 + h as i64).min(self.height as i64);
        if x0 >= x1 || y0 >= y1 {
            return None;
        }
        Some((x0 as u32, y0 as u32, x1 as u32, y1 as u32))
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

    /// Transform (rotate and perspective project) a coverage bitmap using projective mapping.
    /// Returns the new bitmap, its dimensions, and the offset from the original center to the new top-left.
    ///
    /// Degenerate inputs (empty source, non-finite perspective, NaN corners,
    /// or an output that would exceed [`MAX_GLYPH_BITMAP_PIXELS`]) yield an
    /// empty bitmap instead of panicking or over-allocating.
    pub fn projective_transform_coverage_bitmap(
        bitmap: &[u8],
        src_w: u32,
        src_h: u32,
        matrix: &Matrix3x3,
        perspective: f64,
    ) -> (Vec<u8>, u32, u32, i32, i32) {
        if src_w == 0
            || src_h == 0
            || !perspective.is_finite()
            || perspective <= 0.0
            || bitmap.len() < src_w as usize * src_h as usize
        {
            return (Vec::new(), 0, 0, 0, 0);
        }
        if matrix.0.iter().any(|v| !v.is_finite()) {
            return (Vec::new(), 0, 0, 0, 0);
        }

        let cx = src_w as f64 / 2.0;
        let cy = src_h as f64 / 2.0;

        // Compute transformed bounding box by projecting the 4 corners
        let corners = [
            (-cx, -cy),
            (src_w as f64 - cx, -cy),
            (-cx, src_h as f64 - cy),
            (src_w as f64 - cx, src_h as f64 - cy),
        ];

        let mut min_x = f64::MAX;
        let mut max_x = f64::MIN;
        let mut min_y = f64::MAX;
        let mut max_y = f64::MIN;

        for (x, y) in &corners {
            let (x3, y3, z3) = matrix.transform(*x, *y, 0.0);
            if !x3.is_finite() || !y3.is_finite() || !z3.is_finite() {
                return (Vec::new(), 0, 0, 0, 0);
            }
            // Guard the near-zero perspective denominator: clamp z3 away
            // from -perspective so the scale factor stays finite.
            let denom = perspective + z3;
            if !denom.is_finite() || denom.abs() < 1e-6 {
                return (Vec::new(), 0, 0, 0, 0);
            }
            let scale = perspective / denom;
            let px = x3 * scale;
            let py = y3 * scale;
            if !px.is_finite() || !py.is_finite() {
                return (Vec::new(), 0, 0, 0, 0);
            }
            min_x = min_x.min(px);
            max_x = max_x.max(px);
            min_y = min_y.min(py);
            max_y = max_y.max(py);
        }

        let span_w = max_x - min_x;
        let span_h = max_y - min_y;
        if !span_w.is_finite() || !span_h.is_finite() || span_w < 0.0 || span_h < 0.0 {
            return (Vec::new(), 0, 0, 0, 0);
        }
        // +1 matches the historical inclusive bound; saturate instead of wrap.
        let dst_w = span_w.ceil().clamp(0.0, u32::MAX as f64) as u32 + 1;
        let dst_h = span_h.ceil().clamp(0.0, u32::MAX as f64) as u32 + 1;
        let out_pixels = dst_w as u64 * dst_h as u64;
        if out_pixels == 0 || out_pixels > MAX_GLYPH_BITMAP_PIXELS {
            return (Vec::new(), 0, 0, 0, 0);
        }
        let off_x = min_x.floor().clamp(i32::MIN as f64, i32::MAX as f64) as i32;
        let off_y = min_y.floor().clamp(i32::MIN as f64, i32::MAX as f64) as i32;

        let mut new_bitmap = vec![0u8; (dst_w as usize) * (dst_h as usize)];
        let m = &matrix.0;
        let inv_matrix = matrix.transpose(); // Rotation matrix inverse is transpose

        // Perspective inverse mapping
        for dy in 0..dst_h {
            for dx in 0..dst_w {
                let px = dx as f64 + off_x as f64 + 0.5;
                let py = dy as f64 + off_y as f64 + 0.5;

                // Solve for z3: 0 = m31*px*(d+z3)/d + m32*py*(d+z3)/d + m33*z3
                let denom = m[6] * px / perspective + m[7] * py / perspective + m[8];
                if !denom.is_finite() || denom.abs() < 1e-6 {
                    continue;
                }

                let z3 = -(m[6] * px + m[7] * py) / denom;
                if !z3.is_finite() {
                    continue;
                }
                let scale_inv = (perspective + z3) / perspective;
                if !scale_inv.is_finite() {
                    continue;
                }
                let x3 = px * scale_inv;
                let y3 = py * scale_inv;

                // Transform back to source space
                let (sx_rel, sy_rel, _) = inv_matrix.transform(x3, y3, z3);
                if !sx_rel.is_finite() || !sy_rel.is_finite() {
                    continue;
                }
                let sx = sx_rel + cx;
                let sy = sy_rel + cy;
                if !sx.is_finite() || !sy.is_finite() {
                    continue;
                }

                // Bilinear sample
                let sx0 = sx.floor().clamp(i32::MIN as f64, i32::MAX as f64) as i32;
                let sy0 = sy.floor().clamp(i32::MIN as f64, i32::MAX as f64) as i32;
                let fx = (sx - sx.floor()).clamp(0.0, 1.0);
                let fy = (sy - sy.floor()).clamp(0.0, 1.0);

                if sx0 >= 0 && sx0 + 1 < src_w as i32 && sy0 >= 0 && sy0 + 1 < src_h as i32 {
                    let v00 = bitmap[(sy0 as u32 * src_w + sx0 as u32) as usize] as f64;
                    let v10 = bitmap[(sy0 as u32 * src_w + (sx0 + 1) as u32) as usize] as f64;
                    let v01 = bitmap[((sy0 + 1) as u32 * src_w + sx0 as u32) as usize] as f64;
                    let v11 = bitmap[((sy0 + 1) as u32 * src_w + (sx0 + 1) as u32) as usize] as f64;

                    let v = v00 * (1.0 - fx) * (1.0 - fy)
                        + v10 * fx * (1.0 - fy)
                        + v01 * (1.0 - fx) * fy
                        + v11 * fx * fy;

                    new_bitmap[(dy * dst_w + dx) as usize] = v.clamp(0.0, 255.0) as u8;
                } else if sx0 >= 0 && sx0 < src_w as i32 && sy0 >= 0 && sy0 < src_h as i32 {
                    let v = bitmap[(sy0 as u32 * src_w + sx0 as u32) as usize] as f64;
                    new_bitmap[(dy * dst_w + dx) as usize] =
                        (v * (1.0 - fx) * (1.0 - fy)).clamp(0.0, 255.0) as u8;
                }
            }
        }

        (new_bitmap, dst_w, dst_h, off_x, off_y)
    }

    /// Apply box blur (single pass; callers repeat for Gaussian approximation).
    /// The radius is clamped to [`MAX_BLUR_RADIUS`].
    pub fn box_blur(&mut self, radius: u32) {
        let radius = radius.min(MAX_BLUR_RADIUS);
        if radius == 0 || self.width == 0 || self.height == 0 {
            return;
        }
        let r = radius as i32;
        let w = self.width as i32;
        let h = self.height as i32;
        let mut out = vec![0u8; self.pixels.len()];

        // Horizontal pass with sliding window
        for y in 0..h {
            // Initialize window sums for x=0
            let mut sum_r = 0u32;
            let mut sum_g = 0u32;
            let mut sum_b = 0u32;
            let mut sum_a = 0u32;
            let mut count = 0u32;

            for dx in -r..=r {
                let sx = dx;
                if sx >= 0 && sx < w {
                    let idx = ((y * w + sx) * 4) as usize;
                    sum_r += self.pixels[idx] as u32;
                    sum_g += self.pixels[idx + 1] as u32;
                    sum_b += self.pixels[idx + 2] as u32;
                    sum_a += self.pixels[idx + 3] as u32;
                    count += 1;
                }
            }

            let idx = ((y * w) * 4) as usize;
            out[idx] = (sum_r / count) as u8;
            out[idx + 1] = (sum_g / count) as u8;
            out[idx + 2] = (sum_b / count) as u8;
            out[idx + 3] = (sum_a / count) as u8;

            // Slide window: for each subsequent x, add right edge, remove left edge
            for x in 1..w {
                // Add new pixel entering window (right side)
                let add_x = x + r;
                if add_x < w {
                    let idx = ((y * w + add_x) * 4) as usize;
                    sum_r += self.pixels[idx] as u32;
                    sum_g += self.pixels[idx + 1] as u32;
                    sum_b += self.pixels[idx + 2] as u32;
                    sum_a += self.pixels[idx + 3] as u32;
                    count += 1;
                }

                // Remove pixel leaving window (left side)
                let remove_x = x - r - 1;
                if (0..w).contains(&remove_x) {
                    let idx = ((y * w + remove_x) * 4) as usize;
                    sum_r -= self.pixels[idx] as u32;
                    sum_g -= self.pixels[idx + 1] as u32;
                    sum_b -= self.pixels[idx + 2] as u32;
                    sum_a -= self.pixels[idx + 3] as u32;
                    count -= 1;
                }

                let idx = ((y * w + x) * 4) as usize;
                out[idx] = (sum_r / count) as u8;
                out[idx + 1] = (sum_g / count) as u8;
                out[idx + 2] = (sum_b / count) as u8;
                out[idx + 3] = (sum_a / count) as u8;
            }
        }

        // Vertical pass with sliding window
        self.pixels.fill(0);
        for x in 0..w {
            // Initialize window sums for y=0
            let mut sum_r = 0u32;
            let mut sum_g = 0u32;
            let mut sum_b = 0u32;
            let mut sum_a = 0u32;
            let mut count = 0u32;

            for dy in -r..=r {
                let sy = dy;
                if sy >= 0 && sy < h {
                    let idx = ((sy * w + x) * 4) as usize;
                    sum_r += out[idx] as u32;
                    sum_g += out[idx + 1] as u32;
                    sum_b += out[idx + 2] as u32;
                    sum_a += out[idx + 3] as u32;
                    count += 1;
                }
            }

            let idx = (x * 4) as usize;
            self.pixels[idx] = (sum_r / count) as u8;
            self.pixels[idx + 1] = (sum_g / count) as u8;
            self.pixels[idx + 2] = (sum_b / count) as u8;
            self.pixels[idx + 3] = (sum_a / count) as u8;

            // Slide window: for each subsequent y, add bottom edge, remove top edge
            for y in 1..h {
                // Add new pixel entering window (bottom)
                let add_y = y + r;
                if add_y < h {
                    let idx = ((add_y * w + x) * 4) as usize;
                    sum_r += out[idx] as u32;
                    sum_g += out[idx + 1] as u32;
                    sum_b += out[idx + 2] as u32;
                    sum_a += out[idx + 3] as u32;
                    count += 1;
                }

                // Remove pixel leaving window (top)
                let remove_y = y - r - 1;
                if (0..h).contains(&remove_y) {
                    let idx = ((remove_y * w + x) * 4) as usize;
                    sum_r -= out[idx] as u32;
                    sum_g -= out[idx + 1] as u32;
                    sum_b -= out[idx + 2] as u32;
                    sum_a -= out[idx + 3] as u32;
                    count -= 1;
                }

                let idx = ((y * w + x) * 4) as usize;
                self.pixels[idx] = (sum_r / count) as u8;
                self.pixels[idx + 1] = (sum_g / count) as u8;
                self.pixels[idx + 2] = (sum_b / count) as u8;
                self.pixels[idx + 3] = (sum_a / count) as u8;
            }
        }
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
        let (out, w, h, _, _) =
            RenderBuffer::projective_transform_coverage_bitmap(&bitmap, 4, 4, &m, f64::NAN);
        assert!(out.is_empty() && w == 0 && h == 0);
        let (out, w, h, _, _) =
            RenderBuffer::projective_transform_coverage_bitmap(&bitmap, 4, 4, &m, 0.0);
        assert!(out.is_empty() && w == 0 && h == 0);
        let bad = Matrix3x3([f64::NAN; 9]);
        let (out, _, _, _, _) =
            RenderBuffer::projective_transform_coverage_bitmap(&bitmap, 4, 4, &bad, 500.0);
        assert!(out.is_empty());
    }

    #[test]
    fn test_projective_transform_extreme_rotation_bounded() {
        let bitmap = vec![200u8; 64];
        let m = Matrix3x3::rotation_x(89.9f64.to_radians())
            .multiply(&Matrix3x3::rotation_y(89.9f64.to_radians()));
        let (out, w, h, _, _) =
            RenderBuffer::projective_transform_coverage_bitmap(&bitmap, 8, 8, &m, 500.0);
        assert!(w as u64 * h as u64 <= MAX_GLYPH_BITMAP_PIXELS);
        assert_eq!(out.len(), w as usize * h as usize);
    }

    #[test]
    fn test_shear_coverage_bitmap() {
        let bitmap = vec![255u8; 16];
        // shear_x = 1 on 4x4: width grows by height.
        let (out, w, h) = RenderBuffer::shear_coverage_bitmap(&bitmap, 4, 4, 1.0, 0.0);
        assert_eq!((w, h), (8, 4));
        assert_eq!(out.len(), 32);
        assert!(out.iter().any(|&v| v > 0));
        // Zero shear is identity.
        let (out, w, h) = RenderBuffer::shear_coverage_bitmap(&bitmap, 4, 4, 0.0, 0.0);
        assert_eq!((w, h), (4, 4));
        assert_eq!(out, bitmap);
        // Non-finite shear rejected.
        let (out, w, h) = RenderBuffer::shear_coverage_bitmap(&bitmap, 4, 4, f64::NAN, 0.0);
        assert!(out.is_empty() && w == 0 && h == 0);
    }

    #[test]
    fn test_box_blur_clamps_huge_radius() {
        let mut buf = RenderBuffer::new(8, 8).unwrap();
        buf.set_pixel(4, 4, 255, 255, 255, 255);
        buf.box_blur(u32::MAX); // must terminate via clamping
        assert_eq!(buf.pixels.len(), 8 * 8 * 4);
    }
}
