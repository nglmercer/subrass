use crate::renderer::buffer::RenderBuffer;

#[derive(Debug, Clone)]
enum DrawCommand {
    MoveTo {
        x: f64,
        y: f64,
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
        let commands = Self::parse(text);
        if commands.is_empty() {
            return;
        }

        let polygons = Self::commands_to_polygons(&commands);

        for polygon in &polygons {
            if polygon.len() < 3 {
                continue;
            }
            Self::fill_polygon(buffer, polygon, x, y, scale, color);
        }
    }

    fn parse(text: &str) -> Vec<DrawCommand> {
        let mut commands = Vec::new();
        let mut chars = text.chars().peekable();
        let mut start_x = 0.0_f64;
        let mut start_y = 0.0_f64;

        while let Some(&ch) = chars.peek() {
            match ch {
                'm' | 'M' => {
                    chars.next();
                    while let Some((x, y)) = Self::next_coord_pair(&mut chars) {
                        start_x = x;
                        start_y = y;
                        commands.push(DrawCommand::MoveTo { x, y });
                    }
                }
                'l' | 'L' => {
                    chars.next();
                    while let Some((x, y)) = Self::next_coord_pair(&mut chars) {
                        commands.push(DrawCommand::LineTo { x, y });
                    }
                }
                'b' | 'B' => {
                    chars.next();
                    while let (Some((x1, y1)), Some((x2, y2)), Some((x, y))) = (
                        Self::next_coord_pair(&mut chars),
                        Self::next_coord_pair(&mut chars),
                        Self::next_coord_pair(&mut chars),
                    ) {
                        commands.push(DrawCommand::CurveTo {
                            x1,
                            y1,
                            x2,
                            y2,
                            x,
                            y,
                        });
                    }
                }
                'n' | 'N' => {
                    chars.next();
                    let mut prev_x = start_x;
                    let mut prev_y = start_y;
                    if let Some(cmd) = commands.last() {
                        match cmd {
                            DrawCommand::MoveTo { x, y }
                            | DrawCommand::LineTo { x, y }
                            | DrawCommand::CurveTo { x, y, .. } => {
                                prev_x = *x;
                                prev_y = *y;
                            }
                            DrawCommand::Close => {}
                        }
                    }
                    while let (Some((x1, y1)), Some((x2, y2))) = (
                        Self::next_coord_pair(&mut chars),
                        Self::next_coord_pair(&mut chars),
                    ) {
                        commands.push(DrawCommand::CurveTo {
                            x1: prev_x,
                            y1: prev_y,
                            x2: x1,
                            y2: y1,
                            x: x2,
                            y: y2,
                        });
                        prev_x = x2;
                        prev_y = y2;
                    }
                }
                'c' | 'C' => {
                    chars.next();
                    commands.push(DrawCommand::Close);
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

    fn commands_to_polygons(commands: &[DrawCommand]) -> Vec<Vec<(f64, f64)>> {
        let mut polygons = Vec::new();
        let mut current_polygon = Vec::new();

        for cmd in commands {
            match cmd {
                DrawCommand::MoveTo { x, y } => {
                    if current_polygon.len() >= 3 {
                        polygons.push(std::mem::take(&mut current_polygon));
                    }
                    current_polygon = vec![(*x, *y)];
                }
                DrawCommand::LineTo { x, y } => {
                    current_polygon.push((*x, *y));
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
                        let steps = 8;
                        for i in 1..=steps {
                            let t = i as f64 / steps as f64;
                            let mt = 1.0 - t;
                            let px = mt.powi(3) * last.0
                                + 3.0 * mt.powi(2) * t * x1
                                + 3.0 * mt * t.powi(2) * x2
                                + t.powi(3) * x;
                            let py = mt.powi(3) * last.1
                                + 3.0 * mt.powi(2) * t * y1
                                + 3.0 * mt * t.powi(2) * y2
                                + t.powi(3) * y;
                            current_polygon.push((px, py));
                        }
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

    fn fill_polygon(
        buffer: &mut RenderBuffer,
        polygon: &[(f64, f64)],
        offset_x: f64,
        offset_y: f64,
        scale: f64,
        color: [u8; 4],
    ) {
        if polygon.len() < 3 {
            return;
        }

        let mut min_y = f64::MAX;
        let mut max_y = f64::MIN;
        for &(_, py) in polygon {
            let screen_y = py * scale + offset_y;
            min_y = min_y.min(screen_y);
            max_y = max_y.max(screen_y);
        }

        let min_y = (min_y as i32).max(0);
        let max_y = (max_y as i32).min(buffer.height as i32 - 1);

        // Reuse intersections Vec across scanlines
        let mut intersections = Vec::with_capacity(polygon.len());

        for scan_y in min_y..=max_y {
            intersections.clear();

            for i in 0..polygon.len() {
                let j = (i + 1) % polygon.len();
                let (x1, y1) = polygon[i];
                let (x2, y2) = polygon[j];

                let sy1 = y1 * scale + offset_y;
                let sy2 = y2 * scale + offset_y;
                let sx1 = x1 * scale + offset_x;
                let sx2 = x2 * scale + offset_x;

                if (sy1 <= scan_y as f64 && sy2 > scan_y as f64)
                    || (sy2 <= scan_y as f64 && sy1 > scan_y as f64)
                {
                    let t = (scan_y as f64 - sy1) / (sy2 - sy1);
                    let ix = sx1 + t * (sx2 - sx1);
                    intersections.push(ix);
                }
            }

            intersections.sort_by(|a, b| a.partial_cmp(b).unwrap());

            let mut i = 0;
            while i + 1 < intersections.len() {
                let x_start = intersections[i] as i32;
                let x_end = intersections[i + 1] as i32;

                for px in x_start..=x_end {
                    if px >= 0 && px < buffer.width as i32 {
                        buffer.blend_pixel(
                            px as u32,
                            scan_y as u32,
                            color[0],
                            color[1],
                            color[2],
                            color[3],
                        );
                    }
                }
                i += 2;
            }
        }
    }
}
