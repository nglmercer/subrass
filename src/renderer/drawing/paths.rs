//! ASS drawing path construction and curve flattening.

use super::lexer::DrawCommand;
use super::{CURVE_FLATNESS, CURVE_MAX_DEPTH, MAX_DRAWING_POINTS};

pub(super) fn commands_to_polygons(commands: &[DrawCommand]) -> Vec<Vec<(f64, f64)>> {
    let mut polygons = Vec::new();
    let mut current_polygon = Vec::new();

    for cmd in commands {
        match cmd {
            DrawCommand::MoveTo {
                x,
                y,
                close_previous,
            } => {
                if *close_previous {
                    if current_polygon.len() >= 3 {
                        polygons.push(std::mem::take(&mut current_polygon));
                    } else {
                        current_polygon.clear();
                    }
                    current_polygon = vec![(*x, *y)];
                } else {
                    push_point(&mut current_polygon, (*x, *y));
                }
            }
            DrawCommand::LineTo { x, y } => {
                push_point(&mut current_polygon, (*x, *y));
            }
            DrawCommand::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => {
                if let Some(&last) = current_polygon.last() {
                    append_cubic(&mut current_polygon, last, (*x1, *y1), (*x2, *y2), (*x, *y));
                }
            }
            DrawCommand::SplineTo { points, closed } => {
                // libass converts each four-point uniform B-spline span
                // to a cubic Bezier. The current pen is the first
                // control point; subsequent spans advance by one point.
                let Some(&pen) = current_polygon.last() else {
                    continue;
                };
                let mut control = Vec::with_capacity(points.len() + 4);
                control.push(pen);
                control.extend_from_slice(points);
                if *closed {
                    let wrap = control[..3].to_vec();
                    control.extend_from_slice(&wrap);
                }
                if control.len() < 4 {
                    continue;
                }
                // The first B-spline span replaces the move-only pen.
                let replace_pen = current_polygon.len() == 1;
                if replace_pen {
                    current_polygon.clear();
                }
                for window in control.windows(4) {
                    let bezier = bspline_to_bezier(window);
                    if current_polygon.is_empty() {
                        push_point(&mut current_polygon, bezier[0]);
                    }
                    let start = *current_polygon.last().unwrap_or(&bezier[0]);
                    append_cubic(&mut current_polygon, start, bezier[1], bezier[2], bezier[3]);
                }
            }
            DrawCommand::Close => {
                if current_polygon.len() >= 3 {
                    polygons.push(std::mem::take(&mut current_polygon));
                }
            }
        }
    }

    if current_polygon.len() >= 3 {
        polygons.push(current_polygon);
    }

    polygons
}

fn push_point(polygon: &mut Vec<(f64, f64)>, pt: (f64, f64)) {
    if polygon.len() < MAX_DRAWING_POINTS {
        polygon.push(pt);
    }
}

/// Convert one libass uniform B-spline span into cubic Bezier controls.
fn bspline_to_bezier(points: &[(f64, f64)]) -> [(f64, f64); 4] {
    let [p0, p1, p2, p3] = [points[0], points[1], points[2], points[3]];
    let d01 = ((p1.0 - p0.0) / 3.0, (p1.1 - p0.1) / 3.0);
    let d12 = ((p2.0 - p1.0) / 3.0, (p2.1 - p1.1) / 3.0);
    let d23 = ((p3.0 - p2.0) / 3.0, (p3.1 - p2.1) / 3.0);
    [
        (p1.0 + (d12.0 - d01.0) / 2.0, p1.1 + (d12.1 - d01.1) / 2.0),
        (p1.0 + d12.0, p1.1 + d12.1),
        (p2.0 - d12.0, p2.1 - d12.1),
        (p2.0 + (d23.0 - d12.0) / 2.0, p2.1 + (d23.1 - d12.1) / 2.0),
    ]
}

fn append_cubic(
    polygon: &mut Vec<(f64, f64)>,
    p0: (f64, f64),
    p1: (f64, f64),
    p2: (f64, f64),
    p3: (f64, f64),
) {
    if polygon.is_empty() {
        push_point(polygon, p0);
    }
    flatten_cubic(polygon, [p0, p1, p2, p3], 0);
}

fn flatten_cubic(polygon: &mut Vec<(f64, f64)>, [p0, p1, p2, p3]: [(f64, f64); 4], depth: u8) {
    if polygon.len() >= MAX_DRAWING_POINTS {
        return;
    }
    let flat = |p: (f64, f64), a: (f64, f64), b: (f64, f64)| {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let denom = (dx * dx + dy * dy).sqrt();
        if denom <= f64::EPSILON {
            ((p.0 - a.0).powi(2) + (p.1 - a.1).powi(2)).sqrt()
        } else {
            ((dy * p.0 - dx * p.1 + b.0 * a.1 - b.1 * a.0).abs()) / denom
        }
    };
    if depth >= CURVE_MAX_DEPTH
        || (flat(p1, p0, p3) <= CURVE_FLATNESS && flat(p2, p0, p3) <= CURVE_FLATNESS)
    {
        push_point(polygon, p3);
        return;
    }
    let m01 = ((p0.0 + p1.0) / 2.0, (p0.1 + p1.1) / 2.0);
    let m12 = ((p1.0 + p2.0) / 2.0, (p1.1 + p2.1) / 2.0);
    let m23 = ((p2.0 + p3.0) / 2.0, (p2.1 + p3.1) / 2.0);
    let m012 = ((m01.0 + m12.0) / 2.0, (m01.1 + m12.1) / 2.0);
    let m123 = ((m12.0 + m23.0) / 2.0, (m12.1 + m23.1) / 2.0);
    let mid = ((m012.0 + m123.0) / 2.0, (m012.1 + m123.1) / 2.0);
    flatten_cubic(polygon, [p0, m01, m012, mid], depth + 1);
    flatten_cubic(polygon, [mid, m123, m23, p3], depth + 1);
}
