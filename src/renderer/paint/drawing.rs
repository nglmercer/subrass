//! Project vector points with the same ASS matrix, shear, and origin as text.
use super::super::ResolvedStyle;
use super::transform;
use crate::renderer::{buffer::RenderBuffer, drawing::DrawingParser};

pub(super) struct DrawingInk {
    pub bitmap: Vec<u8>,
    pub w: u32,
    pub h: u32,
    pub x: i32,
    pub y: i32,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn coverage(
    buffer: &RenderBuffer,
    drawing: &str,
    pen: (f64, f64),
    unit: (f64, f64),
    origin: (f64, f64),
    style: &ResolvedStyle,
    video_height: u32,
    play_res_y: u32,
) -> Option<DrawingInk> {
    if !unit.0.is_finite() || !unit.1.is_finite() || unit.0 <= 0.0 || unit.1 <= 0.0 {
        return None;
    }
    let matrix = transform::rotation(style);
    let distance = transform::perspective(video_height, play_res_y);
    let (fax, fay) = transform::shear(style, unit.0, unit.1)?;
    let (bitmap, w, h, x, y) =
        DrawingParser::projected_coverage(drawing, buffer.width, buffer.height, |x, y| {
            let (x, y) = (x * unit.0, y * unit.1);
            let (x, y) = (
                pen.0 + x + fax * y - origin.0,
                pen.1 + y + fay * x - origin.1,
            );
            let (x, y, z) = matrix.transform(x, y, 0.0);
            let denom = distance + z;
            if !denom.is_finite() || denom < 1e-6 {
                return None;
            }
            let point = (
                origin.0 + x * distance / denom,
                origin.1 + y * distance / denom,
            );
            (point.0.is_finite() && point.1.is_finite()).then_some(point)
        })?;
    Some(DrawingInk { bitmap, w, h, x, y })
}
