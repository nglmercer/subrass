use crate::renderer::buffer::RenderBuffer;

/// Defensive caps for untrusted drawing input.
pub const MAX_DRAWING_COMMANDS: usize = 100_000;
pub const MAX_DRAWING_POINTS: usize = 1_000_000;
/// Maximum recursion depth for cubic flattening. The flatness threshold is
/// in drawing units; subdivision is therefore stable before the caller's
/// device-scale conversion.
const CURVE_MAX_DEPTH: u8 = 12;
const CURVE_FLATNESS: f64 = 0.05;

#[derive(Debug, Clone)]
enum DrawCommand {
    /// Move to a point. `close_previous` distinguishes `m` (close the
    /// previous outline into its own polygon) from `n` (move without
    /// closing: the outline continues with a pen jump).
    MoveTo {
        x: f64,
        y: f64,
        close_previous: bool,
    },
    LineTo {
        x: f64,
        y: f64,
    },
    CurveTo {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        x: f64,
        y: f64,
    },
    /// Cubic B-spline (`s`, extended by `p`, closed by `c`).
    SplineTo {
        points: Vec<(f64, f64)>,
        closed: bool,
    },
    Close,
}

pub struct DrawingParser;

impl DrawingParser {
    pub fn render_drawing(
        buffer: &mut RenderBuffer,
        text: &str,
        x: f64,
        y: f64,
        scale: f64,
        color: [u8; 4],
    ) {
        if !x.is_finite() || !y.is_finite() || !scale.is_finite() || scale <= 0.0 {
            return;
        }
        Self::fill_polygons(buffer, &Self::polygons(text), x, y, scale, color);
    }

    /// Render a drawing as a white alpha mask (alpha 255 inside shapes).
    pub fn render_mask(buffer: &mut RenderBuffer, text: &str, x: f64, y: f64, scale: f64) {
        if !x.is_finite() || !y.is_finite() || !scale.is_finite() || scale <= 0.0 {
            return;
        }
        buffer.pixels.fill(0);
        Self::fill_polygons_mask(buffer, &Self::polygons(text), x, y, scale);
    }

    /// Measure a drawing's bounding box in drawing units.
    /// Returns (min_x, min_y, width, height), or None when empty.
    pub fn measure(text: &str) -> Option<(f64, f64, f64, f64)> {
        let polygons = Self::polygons(text);
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        let mut any = false;
        for polygon in &polygons {
            for &(x, y) in polygon {
                any = true;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
        if !any {
            return None;
        }
        Some((
            min_x,
            min_y,
            (max_x - min_x).max(0.0),
            (max_y - min_y).max(0.0),
        ))
    }

    fn polygons(text: &str) -> Vec<Vec<(f64, f64)>> {
        let commands = Self::parse(text);
        Self::commands_to_polygons(&commands)
    }

    /// Parse ASS drawing commands:
    /// m = move (close previous), n = move (no close), l = line,
    /// b = cubic Bezier, s = cubic B-spline, p = extend spline,
    /// c = close spline/outline.
    fn parse(text: &str) -> Vec<DrawCommand> {
        let mut commands = Vec::new();
        let mut chars = text.chars().peekable();
        let mut m_seen = false;
        let mut spline_active = false;

        macro_rules! push {
            ($cmd:expr) => {
                if commands.len() < MAX_DRAWING_COMMANDS {
                    commands.push($cmd);
                } else {
                    return commands;
                }
            };
        }

        while let Some(&ch) = chars.peek() {
            match ch {
                'm' | 'M' => {
                    chars.next();
                    m_seen = true;
                    spline_active = false;
                    while let Some((x, y)) = Self::next_coord_pair(&mut chars) {
                        push!(DrawCommand::MoveTo {
                            x,
                            y,
                            close_previous: true
                        });
                    }
                }
                'n' | 'N' => {
                    chars.next();
                    if !m_seen {
                        // libass rejects a drawing whose first usable
                        // command is `n` without a preceding `m`.
                        return Vec::new();
                    }
                    spline_active = false;
                    while let Some((x, y)) = Self::next_coord_pair(&mut chars) {
                        push!(DrawCommand::MoveTo {
                            x,
                            y,
                            close_previous: false
                        });
                    }
                }
                'l' | 'L' => {
                    chars.next();
                    if !m_seen {
                        continue;
                    }
                    while let Some((x, y)) = Self::next_coord_pair(&mut chars) {
                        push!(DrawCommand::LineTo { x, y });
                    }
                }
                'b' | 'B' => {
                    chars.next();
                    if !m_seen {
                        continue;
                    }
                    spline_active = false;
                    while let (Some((x1, y1)), Some((x2, y2)), Some((x, y))) = (
                        Self::next_coord_pair(&mut chars),
                        Self::next_coord_pair(&mut chars),
                        Self::next_coord_pair(&mut chars),
                    ) {
                        push!(DrawCommand::CurveTo {
                            x1,
                            y1,
                            x2,
                            y2,
                            x,
                            y,
                        });
                    }
                }
                's' | 'S' => {
                    chars.next();
                    if !m_seen {
                        continue;
                    }
                    let mut points = Vec::new();
                    while let Some(pt) = Self::next_coord_pair(&mut chars) {
                        points.push(pt);
                    }
                    if points.len() >= 3 {
                        spline_active = true;
                        push!(DrawCommand::SplineTo {
                            points,
                            closed: false,
                        });
                    }
                }
                'p' | 'P' => {
                    chars.next();
                    let mut points = Vec::new();
                    while let Some(pt) = Self::next_coord_pair(&mut chars) {
                        points.push(pt);
                    }
                    if points.is_empty() {
                        continue;
                    }
                    // Extend the open spline, or start one from the
                    // current point when there is none.
                    if !m_seen || !spline_active {
                        continue;
                    }
                    if let Some(DrawCommand::SplineTo {
                        points: existing,
                        closed: false,
                    }) = commands.last_mut()
                    {
                        existing.extend(points);
                    }
                }
                'c' | 'C' => {
                    chars.next();
                    // Close an open spline (connect end to start), then
                    // close the outline.
                    if spline_active {
                        if let Some(DrawCommand::SplineTo { closed, .. }) = commands.last_mut() {
                            *closed = true;
                        }
                        push!(DrawCommand::Close);
                        spline_active = false;
                    } else if commands.last().is_some_and(|command| {
                        matches!(
                            command,
                            DrawCommand::LineTo { .. } | DrawCommand::CurveTo { .. }
                        )
                    }) {
                        push!(DrawCommand::Close);
                    }
                }
                ' ' | ',' | '\n' | '\r' | '\t' => {
                    chars.next();
                }
                _ => {
                    chars.next();
                }
            }
        }

        commands
    }

    fn next_coord_pair(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<(f64, f64)> {
        Self::skip_whitespace(chars);
        let (x, _) = Self::parse_number(chars)?;
        let (y, _) = Self::parse_number(chars)?;
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        Some((x, y))
    }

    fn skip_whitespace(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
        while let Some(&ch) = chars.peek() {
            if ch == ' ' || ch == ',' || ch == '\n' || ch == '\r' || ch == '\t' {
                chars.next();
            } else {
                break;
            }
        }
    }

    fn parse_number(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<(f64, ())> {
        Self::skip_whitespace(chars);

        let mut value: f64 = 0.0;
        let mut sign = 1.0;
        let mut has_digits = false;
        let mut decimal_place = 0.0;

        // Sign
        if let Some(&ch) = chars.peek() {
            if ch == '-' {
                sign = -1.0;
                chars.next();
            } else if ch == '+' {
                chars.next();
            }
        }

        // Integer part
        while let Some(&ch) = chars.peek() {
            if ch.is_ascii_digit() {
                value = value * 10.0 + (ch as u32 - b'0' as u32) as f64;
                has_digits = true;
                chars.next();
            } else {
                break;
            }
        }

        // Decimal part
        if let Some(&'.') = chars.peek() {
            chars.next(); // consume '.'
            while let Some(&ch) = chars.peek() {
                if ch.is_ascii_digit() {
                    value = value * 10.0 + (ch as u32 - b'0' as u32) as f64;
                    decimal_place += 1.0;
                    chars.next();
                } else {
                    break;
                }
            }
        }

        if !has_digits {
            return None;
        }

        // Apply decimal places
        if decimal_place > 0.0 {
            value /= 10.0f64.powf(decimal_place);
        }

        Some((sign * value, ()))
    }

    fn push_point(polygon: &mut Vec<(f64, f64)>, pt: (f64, f64)) {
        if polygon.len() < MAX_DRAWING_POINTS {
            polygon.push(pt);
        }
    }

    fn commands_to_polygons(commands: &[DrawCommand]) -> Vec<Vec<(f64, f64)>> {
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
                        // `n`: pen jump without closing; the outline
                        // continues (the fill closes it implicitly).
                        Self::push_point(&mut current_polygon, (*x, *y));
                    }
                }
                DrawCommand::LineTo { x, y } => {
                    Self::push_point(&mut current_polygon, (*x, *y));
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
                        Self::append_cubic(
                            &mut current_polygon,
                            last,
                            (*x1, *y1),
                            (*x2, *y2),
                            (*x, *y),
                        );
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
                    // The first B-spline span replaces the move-only pen
                    // point. For an already-started outline, its existing
                    // endpoint is the Bezier span's start instead.
                    let replace_pen = current_polygon.len() == 1;
                    if replace_pen {
                        current_polygon.clear();
                    }
                    for window in control.windows(4) {
                        let bezier = Self::bspline_to_bezier(window);
                        if current_polygon.is_empty() {
                            Self::push_point(&mut current_polygon, bezier[0]);
                        }
                        let start = *current_polygon.last().unwrap_or(&bezier[0]);
                        Self::append_cubic(
                            &mut current_polygon,
                            start,
                            bezier[1],
                            bezier[2],
                            bezier[3],
                        );
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
            Self::push_point(polygon, p0);
        }
        Self::flatten_cubic(polygon, [p0, p1, p2, p3], 0);
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
            Self::push_point(polygon, p3);
            return;
        }
        let m01 = ((p0.0 + p1.0) / 2.0, (p0.1 + p1.1) / 2.0);
        let m12 = ((p1.0 + p2.0) / 2.0, (p1.1 + p2.1) / 2.0);
        let m23 = ((p2.0 + p3.0) / 2.0, (p2.1 + p3.1) / 2.0);
        let m012 = ((m01.0 + m12.0) / 2.0, (m01.1 + m12.1) / 2.0);
        let m123 = ((m12.0 + m23.0) / 2.0, (m12.1 + m23.1) / 2.0);
        let mid = ((m012.0 + m123.0) / 2.0, (m012.1 + m123.1) / 2.0);
        Self::flatten_cubic(polygon, [p0, m01, m012, mid], depth + 1);
        Self::flatten_cubic(polygon, [mid, m123, m23, p3], depth + 1);
    }

    /// Scanline raster core: calls `emit(x, y)` for covered pixels.
    /// All coordinates are validated finite and clamped to the buffer
    /// before any loop runs; intersections use total ordering (no
    /// `partial_cmp().unwrap()` on potentially-NaN values).
    ///
    /// Fill rule is even-odd across the whole outline: crossings from
    /// every contour pool together, so nested contours punch holes and
    /// doubly-covered regions stay empty regardless of winding. This
    /// matches reference ASS drawing/clip behavior for hollow shapes.
    fn scan_polygons(
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

        // Pixel-center sampling (FreeType convention, which libass
        // uses for drawings and vector clips): row scan_y covers
        // [scan_y, scan_y + 1), so edges straddle its center
        // scan_y + 0.5, and a span [ix1, ix2] fills the columns whose
        // centers fall inside. Edge-based spans overfill by a pixel
        // (a rect ending exactly at x=148 paints column 148, which
        // libass leaves to the outline) and drop edge rows on 1ulp
        // float boundaries (probe: pbo-shifted top edge at 104+eps
        // lost row 104); center sampling is robust to both.
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
                // Columns with centers inside [ix1, ix2], clamped to
                // the buffer before looping.
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

    fn fill_polygons(
        buffer: &mut RenderBuffer,
        polygons: &[Vec<(f64, f64)>],
        offset_x: f64,
        offset_y: f64,
        scale: f64,
        color: [u8; 4],
    ) {
        Self::fill_polygons_clipped(buffer, polygons, offset_x, offset_y, scale, color, None);
    }

    /// Fill with an optional device-space column range `[x_lo, x_hi)`.
    /// Karaoke sweeps draw drawings in two non-overlapping vertical
    /// halves (libass splits drawing runs exactly like text runs).
    fn fill_polygons_clipped(
        buffer: &mut RenderBuffer,
        polygons: &[Vec<(f64, f64)>],
        offset_x: f64,
        offset_y: f64,
        scale: f64,
        color: [u8; 4],
        x_range: Option<(i64, i64)>,
    ) {
        let (w, h) = (buffer.width, buffer.height);
        Self::scan_polygons(w, h, polygons, offset_x, offset_y, scale, |px, py| {
            if let Some((lo, hi)) = x_range {
                if i64::from(px) < lo || i64::from(px) >= hi {
                    return;
                }
            }
            buffer.blend_pixel(px as u32, py as u32, color[0], color[1], color[2], color[3]);
        });
    }

    /// Render a drawing clipped to device columns `[x_lo, x_hi)`.
    /// `None` bounds are open; a fully-open range draws everything.
    pub fn render_drawing_clipped(
        buffer: &mut RenderBuffer,
        text: &str,
        x: f64,
        y: f64,
        scale: f64,
        color: [u8; 4],
        x_range: (Option<i64>, Option<i64>),
    ) {
        if !x.is_finite() || !y.is_finite() || !scale.is_finite() || scale <= 0.0 {
            return;
        }
        let range = match x_range {
            (None, None) => None,
            (lo, hi) => Some((lo.unwrap_or(i64::MIN), hi.unwrap_or(i64::MAX))),
        };
        if let Some((lo, hi)) = range {
            if lo >= hi {
                return;
            }
        }
        Self::fill_polygons_clipped(buffer, &Self::polygons(text), x, y, scale, color, range);
    }

    /// Stamp a drawing's outline ring and drop shadow under its fill
    /// (libass BorderStyle 1; radii/offsets in device pixels). The
    /// coverage comes from the same scan as the fill, so the ring hugs
    /// the exact fill pixel set. Karaoke sweeps split only the fill —
    /// like the text sweep path, whose outline/shadow never split —
    /// so callers paint this once per drawing before any clipped
    /// fill passes. No-op when both are `None`.
    pub fn render_drawing_effects(
        buffer: &mut RenderBuffer,
        text: &str,
        x: f64,
        y: f64,
        scale: f64,
        outline: Option<([u8; 4], f64, f64)>,
        shadow: Option<([u8; 4], f64, f64)>,
    ) {
        if outline.is_none() && shadow.is_none() {
            return;
        }
        if !x.is_finite() || !y.is_finite() || !scale.is_finite() || scale <= 0.0 {
            return;
        }
        let (w, h) = (buffer.width, buffer.height);
        if w == 0 || h == 0 {
            return;
        }
        let polygons = Self::polygons(text);
        let stride = w as usize;
        let mut coverage = vec![0u8; stride.saturating_mul(h as usize)];
        Self::scan_polygons(w, h, &polygons, x, y, scale, |px, py| {
            let idx = (py as usize)
                .saturating_mul(stride)
                .saturating_add(px as usize);
            if let Some(c) = coverage.get_mut(idx) {
                *c = 255;
            }
        });
        if let Some((rgba, ox, oy)) = outline {
            crate::renderer::effects::apply_outline_xy(buffer, &coverage, w, h, 0, 0, ox, oy, rgba);
        }
        if let Some((rgba, ox, oy)) = shadow {
            crate::renderer::effects::apply_shadow(buffer, &coverage, w, h, 0, 0, ox, oy, rgba);
        }
    }

    fn fill_polygons_mask(
        buffer: &mut RenderBuffer,
        polygons: &[Vec<(f64, f64)>],
        offset_x: f64,
        offset_y: f64,
        scale: f64,
    ) {
        let (w, h) = (buffer.width, buffer.height);
        Self::scan_polygons(w, h, polygons, offset_x, offset_y, scale, |px, py| {
            let idx = ((py as u32 * w + px as u32) * 4) as usize;
            if idx + 3 < buffer.pixels.len() {
                buffer.pixels[idx + 3] = 255;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered_pixels(text: &str, w: u32, h: u32, scale: f64) -> usize {
        let mut buf = RenderBuffer::new(w, h).unwrap();
        DrawingParser::render_drawing(&mut buf, text, 0.0, 0.0, scale, [255, 255, 255, 255]);
        buf.pixels.chunks_exact(4).filter(|p| p[3] > 0).count()
    }

    #[test]
    fn test_move_line_close_triangle() {
        assert!(rendered_pixels("m 10 10 l 50 10 l 30 40 c", 64, 64, 1.0) > 100);
    }

    /// Even-odd across contours (plan #27): a contour inside another
    /// punches a hole, regardless of winding direction.
    #[test]
    fn test_nested_contour_punches_hole() {
        let square = "m 10 10 l 50 10 l 50 50 l 10 50 ";
        // Same winding: inner square is a hole.
        let same = format!("{square}m 20 20 l 40 20 l 40 40 l 20 40");
        // Reversed winding: still a hole (even-odd, not nonzero).
        let reversed = format!("{square}m 20 20 l 20 40 l 40 40 l 40 20");
        for text in [&same, &reversed] {
            let mut buf = RenderBuffer::new(64, 64).unwrap();
            DrawingParser::render_drawing(&mut buf, text, 0.0, 0.0, 1.0, [255; 4]);
            assert_eq!(buf.get_pixel(15, 15)[3], 255, "{text}");
            assert_eq!(buf.get_pixel(30, 30)[3], 0, "{text}");
            assert_eq!(buf.get_pixel(5, 5)[3], 0, "{text}");
        }
    }

    /// Even-odd overlap (plan #27): doubly-covered region stays empty.
    #[test]
    fn test_overlapping_squares_cancel() {
        let mut buf = RenderBuffer::new(64, 64).unwrap();
        DrawingParser::render_drawing(
            &mut buf,
            "m 10 20 l 40 20 l 40 40 l 10 40 m 25 20 l 55 20 l 55 40 l 25 40",
            0.0,
            0.0,
            1.0,
            [255; 4],
        );
        // Left-only and right-only wings are filled...
        assert_eq!(buf.get_pixel(17, 30)[3], 255);
        assert_eq!(buf.get_pixel(47, 30)[3], 255);
        // ...but the doubly-covered middle cancels out.
        assert_eq!(buf.get_pixel(32, 30)[3], 0);
    }

    /// Self-intersecting bowtie (plan #27): even-odd fills BOTH lobes
    /// (a nonzero rule would leave one lobe empty).
    #[test]
    fn test_bowtie_self_intersection_fills_both_lobes() {
        let mut buf = RenderBuffer::new(64, 64).unwrap();
        DrawingParser::render_drawing(
            &mut buf,
            "m 10 10 l 50 50 l 10 50 l 50 10",
            0.0,
            0.0,
            1.0,
            [255; 4],
        );
        assert_eq!(buf.get_pixel(30, 20)[3], 255);
        assert_eq!(buf.get_pixel(30, 40)[3], 255);
        assert_eq!(buf.get_pixel(5, 30)[3], 0);
    }

    /// Hole through the alpha mask path (plan #27: vector clip mask).
    #[test]
    fn test_mask_punches_hole() {
        let mut buf = RenderBuffer::new(64, 64).unwrap();
        DrawingParser::render_mask(
            &mut buf,
            "m 10 10 l 50 10 l 50 50 l 10 50 m 20 20 l 40 20 l 40 40 l 20 40",
            0.0,
            0.0,
            1.0,
        );
        assert_eq!(buf.get_pixel(15, 15)[3], 255);
        assert_eq!(buf.get_pixel(30, 30)[3], 0);
    }

    /// Outline ring + drop shadow for event drawings (plan #28): the
    /// ring paints in outline color around the fill, the shadow stamps
    /// offset in shadow color, and the fill covers both in the core.
    #[test]
    fn test_drawing_effects_outline_and_shadow() {
        let square = "m 20 20 l 40 20 l 40 40 l 20 40";
        let mut buf = RenderBuffer::new(64, 64).unwrap();
        DrawingParser::render_drawing_effects(
            &mut buf,
            square,
            0.0,
            0.0,
            1.0,
            Some(([0, 0, 255, 255], 2.0, 2.0)),
            Some(([0, 255, 0, 255], 3.0, 0.0)),
        );
        DrawingParser::render_drawing(&mut buf, square, 0.0, 0.0, 1.0, [255, 255, 255, 255]);
        // Fill core covers both passes: opaque white.
        assert_eq!(buf.get_pixel(30, 30), [255, 255, 255, 255]);
        // Outline ring left of the fill: blue only.
        let ring = buf.get_pixel(18, 30);
        assert_eq!((ring[0], ring[1]), (0, 0), "ring pixel {ring:?}");
        assert!(ring[2] > 0 && ring[3] > 0, "ring pixel {ring:?}");
        // Shadow right of fill + ring (center sampling: fill spans
        // 20..=39, ring reaches x=41, shadowed fill spans 23..=42):
        // pure green, then blank.
        assert_eq!(buf.get_pixel(42, 30), [0, 255, 0, 255]);
        assert_eq!(buf.get_pixel(43, 30), [0, 0, 0, 0]);
        // Disabled effects leave the buffer untouched.
        let mut blank = RenderBuffer::new(64, 64).unwrap();
        DrawingParser::render_drawing_effects(&mut blank, square, 0.0, 0.0, 1.0, None, None);
        assert!(blank.pixels.iter().all(|b| *b == 0));
    }

    #[test]
    fn test_move_without_close_continues_outline() {
        // `n` keeps one outline (jump edge); `m` splits into two.
        let merged = DrawingParser::polygons("m 0 0 l 10 0 n 20 0 l 30 0 l 30 10");
        assert_eq!(merged.len(), 1);
        let split = DrawingParser::polygons("m 0 0 l 10 0 l 10 10 m 20 0 l 30 0 l 30 10");
        assert_eq!(split.len(), 2);
    }

    #[test]
    fn test_bezier_renders() {
        assert!(rendered_pixels("m 10 30 b 10 10 50 10 50 30 l 50 50 l 10 50 c", 64, 64, 1.0) > 50);
    }

    #[test]
    fn test_spline_extend_close() {
        // `s` spline, `p` extension, `c` close
        let n = rendered_pixels("m 10 30 s 20 10 40 10 50 30 p 55 40 50 50 c", 64, 64, 1.0);
        assert!(n > 20, "spline pixels: {}", n);
        let polys = DrawingParser::polygons("m 0 0 s 10 0 20 0 30 10 p 35 15 30 20 c");
        assert_eq!(polys.len(), 1);
        assert!(polys[0].len() > 8);
    }

    #[test]
    fn test_measure_bbox() {
        let (min_x, min_y, w, h) = DrawingParser::measure("m 10 20 l 50 20 l 50 60").unwrap();
        assert_eq!((min_x, min_y), (10.0, 20.0));
        assert_eq!((w, h), (40.0, 40.0));
        assert!(DrawingParser::measure("").is_none());
        assert!(DrawingParser::measure("m 0 0").is_none());
    }

    #[test]
    fn test_mask_writes_alpha_only_inside() {
        let mut buf = RenderBuffer::new(64, 64).unwrap();
        DrawingParser::render_mask(&mut buf, "m 10 10 l 50 10 l 50 50 l 10 50", 0.0, 0.0, 1.0);
        assert_eq!(buf.get_pixel(30, 30)[3], 255);
        assert_eq!(buf.get_pixel(0, 0)[3], 0);
    }

    #[test]
    fn test_malicious_coordinates_bounded() {
        // Extreme coordinates: terminates, no panic, no hang.
        let mut buf = RenderBuffer::new(32, 32).unwrap();
        DrawingParser::render_drawing(
            &mut buf,
            "m -1e18 -1e18 l 1e18 -1e18 l 1e18 1e18 l -1e18 1e18",
            0.0,
            0.0,
            1.0,
            [255, 255, 255, 255],
        );
        DrawingParser::render_drawing(&mut buf, "m 0 0 l NaN 5 l 5 5", 0.0, 0.0, 1.0, [255; 4]);
        // Coordinates overflowing to infinity are rejected (no panic).
        let huge = "9".repeat(400);
        assert!(DrawingParser::measure(&format!("m 0 0 l {} 5 l 5 5", huge)).is_none());
    }

    #[test]
    fn test_degenerate_inputs_safe() {
        let mut buf = RenderBuffer::new(16, 16).unwrap();
        DrawingParser::render_drawing(&mut buf, "m 5 5 l 6 6", 0.0, 0.0, f64::NAN, [255; 4]);
        DrawingParser::render_drawing(&mut buf, "m 5 5 l 6 6", 0.0, 0.0, -1.0, [255; 4]);
        DrawingParser::render_drawing(&mut buf, "", 0.0, 0.0, 1.0, [255; 4]);
        assert!(buf.pixels.iter().all(|&p| p == 0));
    }
}
