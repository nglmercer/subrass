//! Checked coordinate and shear helpers shared by render-buffer operations.

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
