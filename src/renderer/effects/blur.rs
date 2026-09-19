use crate::renderer::buffer::{finite_to_u32, RenderBuffer};

/// Apply Gaussian-like blur to buffer. Non-finite or negative radii are
/// ignored; large radii are clamped by [`RenderBuffer::box_blur`].
pub fn apply_blur(buffer: &mut RenderBuffer, blur_radius: f64) {
    apply_blur_xy(buffer, blur_radius, blur_radius);
}

/// Apply blur with independent horizontal and vertical radii.
pub fn apply_blur_xy(buffer: &mut RenderBuffer, blur_x: f64, blur_y: f64) {
    if !blur_x.is_finite() || !blur_y.is_finite() || blur_x <= 0.0 || blur_y <= 0.0 {
        return;
    }
    let radius_x =
        finite_to_u32((blur_x * 2.0).round().clamp(0.0, f64::from(u32::MAX))).unwrap_or(0);
    let radius_y =
        finite_to_u32((blur_y * 2.0).round().clamp(0.0, f64::from(u32::MAX))).unwrap_or(0);
    buffer.box_blur_xy(radius_x, radius_y);
}
