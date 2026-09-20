//! Drawing paint entry points. Geometry is measured by layout; this module
//! only forwards immutable paths to the bounded rasterizer.

use super::super::super::buffer::RenderBuffer;
use super::super::super::drawing::DrawingParser;

pub(super) fn render_effects(
    buffer: &mut RenderBuffer,
    drawing: &str,
    x: f64,
    y: f64,
    unit: f64,
    outline: Option<([u8; 4], f64, f64)>,
    shadow: Option<([u8; 4], f64, f64)>,
) {
    DrawingParser::render_drawing_effects(buffer, drawing, x, y, unit, outline, shadow);
}

pub(super) fn render(
    buffer: &mut RenderBuffer,
    drawing: &str,
    x: f64,
    y: f64,
    unit: f64,
    color: [u8; 4],
) {
    DrawingParser::render_drawing(buffer, drawing, x, y, unit, color);
}

pub(super) fn render_clipped(
    buffer: &mut RenderBuffer,
    drawing: &str,
    x: f64,
    y: f64,
    unit: f64,
    color: [u8; 4],
    clip: (Option<i64>, Option<i64>),
) {
    DrawingParser::render_drawing_clipped(buffer, drawing, x, y, unit, color, clip);
}
