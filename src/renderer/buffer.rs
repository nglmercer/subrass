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
            if !bitmap_has_pixels(bitmap, width, height) {
                return (Vec::new(), 0, 0);
            }
            return (bitmap.to_vec(), width, height);
        }
        let new_width_f = f64::from(width) * scale_x;
        let new_height_f = f64::from(height) * scale_y;
        if !new_width_f.is_finite() || !new_height_f.is_finite() {
            return (Vec::new(), 0, 0);
        }
        let (Some(new_width), Some(new_height)) = (
            finite_to_u32(new_width_f.round().clamp(1.0, f64::from(u32::MAX))),
            finite_to_u32(new_height_f.round().clamp(1.0, f64::from(u32::MAX))),
        ) else {
            return (Vec::new(), 0, 0);
        };
        let out_pixels = u64::from(new_width) * u64::from(new_height);
        if out_pixels == 0 || out_pixels > MAX_GLYPH_BITMAP_PIXELS {
            return (Vec::new(), 0, 0);
        }
        if !bitmap_has_pixels(bitmap, width, height) {
            return (Vec::new(), 0, 0);
        }

        let out_len = match checked_pixel_count(new_width, new_height) {
            Some(n) => n,
            None => return (Vec::new(), 0, 0),
        };
        let mut resized = vec![0u8; out_len];
        for y in 0..new_height {
            let source_y =
                ((f64::from(y) + 0.5) / scale_y - 0.5).clamp(0.0, f64::from(height) - 1.0);
            let y0 = source_y.floor().clamp(0.0, f64::from(height - 1)) as u32;
            let y1 = (y0 + 1).min(height - 1);
            let fy = source_y - f64::from(y0);

            for x in 0..new_width {
                let source_x =
                    ((f64::from(x) + 0.5) / scale_x - 0.5).clamp(0.0, f64::from(width) - 1.0);
                let x0 = source_x.floor().clamp(0.0, f64::from(width - 1)) as u32;
                let x1 = (x0 + 1).min(width - 1);
                let fx = source_x - f64::from(x0);

                let sample = |sx: u32, sy: u32| -> f64 {
                    match (u64::from(sy) * u64::from(width) + u64::from(sx))
                        .try_into()
                        .ok()
                        .and_then(|i: usize| bitmap.get(i))
                    {
                        Some(v) => f64::from(*v),
                        None => 0.0,
                    }
                };
                let top = sample(x0, y0) * (1.0 - fx) + sample(x1, y0) * fx;
                let bottom = sample(x0, y1) * (1.0 - fx) + sample(x1, y1) * fx;
                if let Some(slot) = (u64::from(y) * u64::from(new_width) + u64::from(x))
                    .try_into()
                    .ok()
                    .and_then(|i: usize| resized.get_mut(i))
                {
                    *slot = (top * (1.0 - fy) + bottom * fy).round().clamp(0.0, 255.0) as u8;
                }
            }
        }

        (resized, new_width, new_height)
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
    /// Transform order matches ASS reference semantics (libass
    /// `calc_transform_matrix`, VSFilter `Transform_C`): glyph-local
    /// scaling (done by the caller) → `\fax`/`\fay` shear around the
    /// glyph-space pivot → rotation/perspective → compositing. Folding
    /// the shear into this single resampling pass keeps it exact and
    /// avoids a second lossy warp.
    ///
    /// `shear_pivot` is in absolute source-bitmap pixels.
    ///
    /// Degenerate inputs (empty source, non-finite perspective, shear
    /// or pivot, NaN corners, or an output that would exceed
    /// [`MAX_GLYPH_BITMAP_PIXELS`]) yield an empty bitmap instead of
    /// panicking or over-allocating.
    pub fn projective_transform_coverage_bitmap(
        bitmap: &[u8],
        src_w: u32,
        src_h: u32,
        matrix: &Matrix3x3,
        perspective: f64,
        shear: (f64, f64),
        shear_pivot: (f64, f64),
    ) -> (Vec<u8>, u32, u32, i32, i32) {
        if src_w == 0
            || src_h == 0
            || !perspective.is_finite()
            || perspective <= 0.0
            || !bitmap_has_pixels(bitmap, src_w, src_h)
        {
            return (Vec::new(), 0, 0, 0, 0);
        }
        if matrix.0.iter().any(|v| !v.is_finite()) {
            return (Vec::new(), 0, 0, 0, 0);
        }
        let Some((fax, fay)) = effective_shear(shear) else {
            return (Vec::new(), 0, 0, 0, 0);
        };
        if !shear_pivot.0.is_finite() || !shear_pivot.1.is_finite() {
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
            // Reference order: shear the glyph-local point around the
            // pivot FIRST, then rotate/project the sheared point.
            let (sx, sy) = shear_forward(x + cx, y + cy, fax, fay, shear_pivot);
            let (sx, sy) = (sx - cx, sy - cy);
            if !sx.is_finite() || !sy.is_finite() {
                return (Vec::new(), 0, 0, 0, 0);
            }
            let (x3, y3, z3) = matrix.transform(sx, sy, 0.0);
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
        // +1 matches the historical inclusive bound; cap before adding so
        // u32::MAX can never wrap, then enforce the pixel budget.
        let base_w = span_w.ceil().clamp(0.0, f64::from(u32::MAX - 1)) as u32;
        let base_h = span_h.ceil().clamp(0.0, f64::from(u32::MAX - 1)) as u32;
        let (Some(dst_w), Some(dst_h)) = (base_w.checked_add(1), base_h.checked_add(1)) else {
            return (Vec::new(), 0, 0, 0, 0);
        };
        let out_pixels = u64::from(dst_w) * u64::from(dst_h);
        if out_pixels == 0 || out_pixels > MAX_GLYPH_BITMAP_PIXELS {
            return (Vec::new(), 0, 0, 0, 0);
        }
        let (Some(off_x), Some(off_y)) = (
            finite_to_i32(
                min_x
                    .floor()
                    .clamp(f64::from(i32::MIN), f64::from(i32::MAX)),
            ),
            finite_to_i32(
                min_y
                    .floor()
                    .clamp(f64::from(i32::MIN), f64::from(i32::MAX)),
            ),
        ) else {
            return (Vec::new(), 0, 0, 0, 0);
        };

        let out_len = match checked_pixel_count(dst_w, dst_h) {
            Some(n) => n,
            None => return (Vec::new(), 0, 0, 0, 0),
        };
        let mut new_bitmap = vec![0u8; out_len];
        let m = &matrix.0;
        let inv_matrix = matrix.transpose(); // Rotation matrix inverse is transpose

        // Perspective inverse mapping
        for dy in 0..dst_h {
            for dx in 0..dst_w {
                let px = f64::from(dx) + f64::from(off_x) + 0.5;
                let py = f64::from(dy) + f64::from(off_y) + 0.5;

                // Solve for z3 from the source-plane constraint: the
                // source has z=0, and (sx,sy,0) = M^T*(x3,y3,z3), so
                // 0 = m13*x3 + m23*y3 + m33*z3 with x3 = px*(d+z3)/d
                // (third COLUMN of M: m[2], m[5], m[8]).
                let denom = m[2] * px / perspective + m[5] * py / perspective + m[8];
                if !denom.is_finite() || denom.abs() < 1e-6 {
                    continue;
                }

                let z3 = -(m[2] * px + m[5] * py) / denom;
                if !z3.is_finite() {
                    continue;
                }
                let scale_inv = (perspective + z3) / perspective;
                if !scale_inv.is_finite() {
                    continue;
                }
                let x3 = px * scale_inv;
                let y3 = py * scale_inv;

                // Transform back to source space, then undo the
                // pre-rotation shear around the pivot.
                let (sx_rel, sy_rel, _) = inv_matrix.transform(x3, y3, z3);
                if !sx_rel.is_finite() || !sy_rel.is_finite() {
                    continue;
                }
                let Some((sx, sy)) = shear_inverse(sx_rel + cx, sy_rel + cy, fax, fay, shear_pivot)
                else {
                    continue;
                };
                if !sx.is_finite() || !sy.is_finite() {
                    continue;
                }

                // Bilinear sample
                let (Some(sx0), Some(sy0)) = (
                    finite_to_i32(sx.floor().clamp(f64::from(i32::MIN), f64::from(i32::MAX))),
                    finite_to_i32(sy.floor().clamp(f64::from(i32::MIN), f64::from(i32::MAX))),
                ) else {
                    continue;
                };
                let fx = (sx - sx.floor()).clamp(0.0, 1.0);
                let fy = (sy - sy.floor()).clamp(0.0, 1.0);
                let src_w_i64 = i64::from(src_w);
                let src_h_i64 = i64::from(src_h);
                let sample_at = |xx: i64, yy: i64| -> f64 {
                    if xx < 0 || yy < 0 || xx >= src_w_i64 || yy >= src_h_i64 {
                        return 0.0;
                    }
                    let idx = (yy as u64 * u64::from(src_w) + xx as u64) as usize;
                    bitmap.get(idx).copied().unwrap_or(0) as f64
                };
                let dst_idx = (u64::from(dy) * u64::from(dst_w) + u64::from(dx)) as usize;
                let Some(slot) = new_bitmap.get_mut(dst_idx) else {
                    continue;
                };

                if i64::from(sx0) + 1 < src_w_i64 && i64::from(sy0) + 1 < src_h_i64 {
                    let v00 = sample_at(i64::from(sx0), i64::from(sy0));
                    let v10 = sample_at(i64::from(sx0) + 1, i64::from(sy0));
                    let v01 = sample_at(i64::from(sx0), i64::from(sy0) + 1);
                    let v11 = sample_at(i64::from(sx0) + 1, i64::from(sy0) + 1);

                    let v = v00 * (1.0 - fx) * (1.0 - fy)
                        + v10 * fx * (1.0 - fy)
                        + v01 * (1.0 - fx) * fy
                        + v11 * fx * fy;

                    *slot = v.clamp(0.0, 255.0) as u8;
                } else if i64::from(sx0) < src_w_i64 && i64::from(sy0) < src_h_i64 {
                    let v = sample_at(i64::from(sx0), i64::from(sy0));
                    *slot = (v * (1.0 - fx) * (1.0 - fy)).clamp(0.0, 255.0) as u8;
                }
            }
        }

        (new_bitmap, dst_w, dst_h, off_x, off_y)
    }

    /// Apply box blur (single pass; callers repeat for Gaussian approximation).
    /// The radius is clamped to [`MAX_BLUR_RADIUS`].
    pub fn box_blur(&mut self, radius: u32) {
        self.box_blur_xy(radius, radius);
    }

    /// Apply an axis-specific box blur. libass scales blur horizontally and
    /// vertically from the active layout resolution, which can differ from
    /// PlayRes for anamorphic scripts.
    pub fn box_blur_xy(&mut self, radius_x: u32, radius_y: u32) {
        let radius_x = radius_x.min(MAX_BLUR_RADIUS);
        let radius_y = radius_y.min(MAX_BLUR_RADIUS);
        if (radius_x == 0 && radius_y == 0) || self.width == 0 || self.height == 0 {
            return;
        }
        let rx = radius_x as i32;
        let ry = radius_y as i32;
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

            for dx in -rx..=rx {
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
                let add_x = x + rx;
                if add_x < w {
                    let idx = ((y * w + add_x) * 4) as usize;
                    sum_r += self.pixels[idx] as u32;
                    sum_g += self.pixels[idx + 1] as u32;
                    sum_b += self.pixels[idx + 2] as u32;
                    sum_a += self.pixels[idx + 3] as u32;
                    count += 1;
                }

                // Remove pixel leaving window (left side)
                let remove_x = x - rx - 1;
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

            for dy in -ry..=ry {
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
                let add_y = y + ry;
                if add_y < h {
                    let idx = ((add_y * w + x) * 4) as usize;
                    sum_r += out[idx] as u32;
                    sum_g += out[idx + 1] as u32;
                    sum_b += out[idx + 2] as u32;
                    sum_a += out[idx + 3] as u32;
                    count += 1;
                }

                // Remove pixel leaving window (top)
                let remove_y = y - ry - 1;
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
