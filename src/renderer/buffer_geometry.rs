//! Checked coordinate and shear helpers shared by render-buffer operations.
use super::MAX_GLYPH_BITMAP_PIXELS;
use crate::utils::Matrix3x3;

/// Checked `usize` pixel-count multiplication (WASM32-safe).
///
/// Returns `None` when the conversion or multiplication overflows the
/// target `usize`. Use for every bitmap-length validation and glyph
/// allocation instead of `w as usize * h as usize`.
#[inline]
pub fn checked_pixel_count(w: u32, h: u32) -> Option<usize> {
    usize::try_from(w)
        .ok()
        .and_then(|w| usize::try_from(h).ok().and_then(|h| w.checked_mul(h)))
}

/// True when `bitmap` holds at least `w*h` bytes, using checked arithmetic.
#[inline]
pub fn bitmap_has_pixels(bitmap: &[u8], w: u32, h: u32) -> bool {
    match checked_pixel_count(w, h) {
        Some(need) => bitmap.len() >= need,
        None => false,
    }
}

/// Convert a finite float to `i32` with explicit range validation.
///
/// Returns `None` for non-finite values or values outside `i32` range.
/// Callers choose clamping or skipping; the conversion itself never
/// relies on saturating-cast masking.
#[inline]
pub fn finite_to_i32(v: f64) -> Option<i32> {
    if !v.is_finite() || v < f64::from(i32::MIN) || v > f64::from(i32::MAX) {
        return None;
    }
    Some(v as i32)
}

/// Convert a finite float to `u32` with explicit range validation.
#[inline]
pub fn finite_to_u32(v: f64) -> Option<u32> {
    if !v.is_finite() || v < 0.0 || v > f64::from(u32::MAX) {
        return None;
    }
    Some(v as u32)
}

/// Resolve raw (`\\fax`, `\\fay`) factors into the effective shear:
/// non-finite input yields `None` (caller skips the glyph), values
/// clamp to ±8 for bounded output, and a near-singular pair
/// (`fax*fay` ≈ 1, which would collapse the plane to a line) falls back
/// to fax-only shear, which is always invertible.
#[inline]
pub fn effective_shear(shear: (f64, f64)) -> Option<(f64, f64)> {
    let (fax, fay) = shear;
    if !fax.is_finite() || !fay.is_finite() {
        return None;
    }
    let (fax, fay) = (fax.clamp(-8.0, 8.0), fay.clamp(-8.0, 8.0));
    if (1.0 - fax * fay).abs() < 1e-9 {
        Some((fax, 0.0))
    } else {
        Some((fax, fay))
    }
}

/// Forward ASS shear around a pivot: `x' = x + fax*(y - py)`,
/// `y' = y + fay*(x - px)`. Reference order (libass `x1`/`y1` matrix,
/// VSFilter `Transform_C`) shears glyph-local coordinates BEFORE
/// rotation, so the caller passes the glyph-space pivot.
#[inline]
pub fn shear_forward(x: f64, y: f64, fax: f64, fay: f64, pivot: (f64, f64)) -> (f64, f64) {
    (x + fax * (y - pivot.1), y + fay * (x - pivot.0))
}

/// Inverse of [`shear_forward`]. Returns `None` when the shear is
/// singular (`fax*fay` ≈ 1).
#[inline]
pub fn shear_inverse(
    px: f64,
    py: f64,
    fax: f64,
    fay: f64,
    pivot: (f64, f64),
) -> Option<(f64, f64)> {
    let det = 1.0 - fax * fay;
    if !det.is_finite() || det.abs() < 1e-9 {
        return None;
    }
    let rx = px - pivot.0;
    let ry = py - pivot.1;
    Some((
        (rx - fax * ry) / det + pivot.0,
        (ry - fay * rx) / det + pivot.1,
    ))
}

/// Add a signed base coordinate and an unsigned bitmap offset widening
/// through `i64`, then bounds-check against `limit`.
///
/// Returns the in-bounds `u32` coordinate or `None` when the sum is
/// negative, overflows, or falls outside `[0, limit)`.
#[inline]
pub fn add_coord(base: i32, offset: u32, limit: u32) -> Option<u32> {
    let v = i64::from(base) + i64::from(offset);
    if v < 0 || v >= i64::from(limit) {
        return None;
    }
    Some(v as u32)
}

impl super::RenderBuffer {
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
    pub(super) fn rect_intersection(
        &self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    ) -> Option<(u32, u32, u32, u32)> {
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
}
