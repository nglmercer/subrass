//! Bounded scanline rasterization for ASS drawing polygons.

use crate::renderer::buffer::RenderBuffer;

/// Scanline raster core: calls emit(x, y) for covered pixels.
pub(super) fn scan_polygons(
    buf_width: u32,
    buf_height: u32,
    polygons: &[Vec<(f64, f64)>],
    offset_x: f64,
    offset_y: f64,
    scale: f64,
    mut emit: impl FnMut(i32, i32),
) {
    if buf_width == 0 || buf_height == 0 {
        return;
    }
    let w = buf_width as i64;
    let h = buf_height as i64;

    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for polygon in polygons {
        if polygon.len() < 3 {
            continue;
        }
        for &(_, py) in polygon {
            let screen_y = py * scale + offset_y;
            if !screen_y.is_finite() {
                return;
            }
            min_y = min_y.min(screen_y);
            max_y = max_y.max(screen_y);
        }
    }
    if min_y > max_y {
        return;
    }

    let min_y = (min_y.floor() as i64).max(0).min(h - 1);
    let max_y = (max_y.ceil() as i64).max(0).min(h - 1);

    let total_edges: usize = polygons.iter().map(|p| p.len()).sum();
    let mut intersections = Vec::with_capacity(total_edges.min(4096));

    // Pixel-center sampling: row scan_y covers [scan_y, scan_y + 1),
    // so edges straddle its center scan_y + 0.5.
    for scan_y in min_y..=max_y {
        intersections.clear();
        let yc = scan_y as f64 + 0.5;

        for polygon in polygons {
            if polygon.len() < 3 {
                continue;
            }
            for i in 0..polygon.len() {
                let j = (i + 1) % polygon.len();
                let (x1, y1) = polygon[i];
                let (x2, y2) = polygon[j];

                let sy1 = y1 * scale + offset_y;
                let sy2 = y2 * scale + offset_y;
                let sx1 = x1 * scale + offset_x;
                let sx2 = x2 * scale + offset_x;
                if !(sy1.is_finite() && sy2.is_finite() && sx1.is_finite() && sx2.is_finite()) {
                    continue;
                }

                if (sy1 <= yc && sy2 > yc) || (sy2 <= yc && sy1 > yc) {
                    let denom = sy2 - sy1;
                    if denom.abs() < f64::EPSILON {
                        continue;
                    }
                    let t = (yc - sy1) / denom;
                    let ix = sx1 + t * (sx2 - sx1);
                    if ix.is_finite() {
                        intersections.push(ix);
                    }
                }
            }
        }

        intersections.sort_by(|a, b| a.total_cmp(b));

        let mut i = 0;
        while i + 1 < intersections.len() {
            let x_start = (intersections[i] - 0.5).ceil() as i64;
            let x_end = (intersections[i + 1] - 0.5).floor() as i64;
            if x_start > x_end {
                i += 2;
                continue;
            }
            let x_start = x_start.max(0).min(w - 1);
            let x_end = x_end.max(0).min(w - 1);
            for px in x_start..=x_end {
                emit(px as i32, scan_y as i32);
            }
            i += 2;
        }
    }
}

pub(super) fn fill_polygons(
    buffer: &mut RenderBuffer,
    polygons: &[Vec<(f64, f64)>],
    offset_x: f64,
    offset_y: f64,
    scale: f64,
    color: [u8; 4],
) {
    fill_polygons_clipped(buffer, polygons, offset_x, offset_y, scale, color, None);
}

/// Fill with an optional device-space column range [x_lo, x_hi).
pub(super) fn fill_polygons_clipped(
    buffer: &mut RenderBuffer,
    polygons: &[Vec<(f64, f64)>],
    offset_x: f64,
    offset_y: f64,
    scale: f64,
    color: [u8; 4],
    x_range: Option<(i64, i64)>,
) {
    let (w, h) = (buffer.width, buffer.height);
    scan_polygons(w, h, polygons, offset_x, offset_y, scale, |px, py| {
        if let Some((lo, hi)) = x_range {
            if i64::from(px) < lo || i64::from(px) >= hi {
                return;
            }
        }
        buffer.blend_pixel(px as u32, py as u32, color[0], color[1], color[2], color[3]);
    });
}

pub(super) fn fill_polygons_mask(
    buffer: &mut RenderBuffer,
    polygons: &[Vec<(f64, f64)>],
    offset_x: f64,
    offset_y: f64,
    scale: f64,
) {
    let (w, h) = (buffer.width, buffer.height);
    scan_polygons(w, h, polygons, offset_x, offset_y, scale, |px, py| {
        let idx = ((py as u32 * w + px as u32) * 4) as usize;
        if idx + 3 < buffer.pixels.len() {
            buffer.pixels[idx + 3] = 255;
        }
    });
}
