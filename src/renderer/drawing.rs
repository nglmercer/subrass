use crate::renderer::buffer::RenderBuffer;

/// Drawing command types
#[derive(Debug, Clone)]
enum DrawCommand {
    MoveTo { x: f64, y: f64 },
    LineTo { x: f64, y: f64 },
    CurveTo { x1: f64, y1: f64, x2: f64, y2: f64, x: f64, y: f64 },
    Close,
}

/// Parse and render ASS drawing mode paths
pub struct DrawingParser;

impl DrawingParser {
    /// Parse drawing commands from text and render to buffer
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

        // Collect all polygon points from the commands
        let polygons = Self::commands_to_polygons(&commands);

        // Render each polygon
        for polygon in &polygons {
            if polygon.len() < 3 {
                continue;
            }
            Self::fill_polygon(buffer, polygon, x, y, scale, color);
        }
    }

    /// Parse drawing commands from text
    fn parse(text: &str) -> Vec<DrawCommand> {
        let mut commands = Vec::new();
        let mut chars = text.trim().chars().peekable();
        let mut current_x = 0.0_f64;
        let mut current_y = 0.0_f64;
        let mut start_x = 0.0_f64;
        let mut start_y = 0.0_f64;

        while let Some(&ch) = chars.peek() {
            match ch {
                'm' | 'M' => {
                    chars.next();
                    let coords = Self::parse_coords(&mut chars);
                    if coords.len() >= 2 {
                        current_x = coords[0];
                        current_y = coords[1];
                        start_x = current_x;
                        start_y = current_y;
                        commands.push(DrawCommand::MoveTo { x: current_x, y: current_y });
                    }
                    // Parse additional lineto commands after moveto
                    while coords.len() >= 4 {
                        current_x = coords[2];
                        current_y = coords[3];
                        commands.push(DrawCommand::LineTo { x: current_x, y: current_y });
                        // This is wrong - need to re-parse properly
                        break;
                    }
                }
                'l' | 'L' => {
                    chars.next();
                    let coords = Self::parse_coords(&mut chars);
                    if coords.len() >= 2 {
                        current_x = coords[0];
                        current_y = coords[1];
                        commands.push(DrawCommand::LineTo { x: current_x, y: current_y });
                    }
                }
                'b' | 'B' => {
                    chars.next();
                    let coords = Self::parse_coords(&mut chars);
                    if coords.len() >= 6 {
                        let x1 = coords[0];
                        let y1 = coords[1];
                        let x2 = coords[2];
                        let y2 = coords[3];
                        current_x = coords[4];
                        current_y = coords[5];
                        commands.push(DrawCommand::CurveTo { x1, y1, x2, y2, x: current_x, y: current_y });
                    }
                }
                'n' | 'N' => {
                    chars.next();
                    let coords = Self::parse_coords(&mut chars);
                    if coords.len() >= 6 {
                        // n = shorthand cubic bezier (ctrl1 = previous point, ctrl2 = first coord)
                        let x1 = current_x;
                        let y1 = current_y;
                        let x2 = coords[0];
                        let y2 = coords[1];
                        current_x = coords[2];
                        current_y = coords[3];
                        commands.push(DrawCommand::CurveTo { x1, y1, x2, y2, x: current_x, y: current_y });
                    }
                }
                'c' | 'C' => {
                    chars.next();
                    // c = close path
                    commands.push(DrawCommand::Close);
                    current_x = start_x;
                    current_y = start_y;
                }
                ' ' | ',' => {
                    chars.next(); // skip whitespace
                }
                _ => {
                    chars.next(); // skip unknown chars
                }
            }
        }

        commands
    }

    /// Parse coordinate values from text
    fn parse_coords(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Vec<f64> {
        let mut coords = Vec::new();
        let mut num_str = String::new();

        while let Some(&ch) = chars.peek() {
            match ch {
                '0'..='9' | '.' | '-' | '+' => {
                    num_str.push(ch);
                    chars.next();
                }
                _ if !num_str.is_empty() => {
                    if let Ok(val) = num_str.parse::<f64>() {
                        coords.push(val);
                    }
                    num_str.clear();
                }
                _ => {
                    chars.next();
                }
            }
        }

        if !num_str.is_empty() {
            if let Ok(val) = num_str.parse::<f64>() {
                coords.push(val);
            }
        }

        coords
    }

    /// Convert commands to polygon segments (for simple polygon fill)
    fn commands_to_polygons(commands: &[DrawCommand]) -> Vec<Vec<(f64, f64)>> {
        let mut polygons = Vec::new();
        let mut current_polygon = Vec::new();
        let mut has_move = false;

        for cmd in commands {
            match cmd {
                DrawCommand::MoveTo { x, y } => {
                    if current_polygon.len() >= 3 {
                        polygons.push(current_polygon);
                    }
                    current_polygon = vec![(*x, *y)];
                    has_move = true;
                }
                DrawCommand::LineTo { x, y } => {
                    if has_move {
                        current_polygon.push((*x, *y));
                    }
                }
                DrawCommand::CurveTo { x1, y1, x2, y2, x, y } => {
                    if has_move && !current_polygon.is_empty() {
                        // Approximate bezier with line segments
                        let last = *current_polygon.last().unwrap();
                        let steps = 8;
                        for i in 1..=steps {
                            let t = i as f64 / steps as f64;
                            let t2 = t * t;
                            let t3 = t2 * t;
                        let mt = 1.0 - t;
                        let mt2 = mt * mt;
                        let mt3 = mt2 * mt;
                            let px = mt3 * last.0 + 3.0 * mt2 * t * x1 + 3.0 * mt * t2 * x2 + t3 * x;
                            let py = mt3 * last.1 + 3.0 * mt2 * t * y1 + 3.0 * mt * t2 * y2 + t3 * y;
                            current_polygon.push((px, py));
                        }
                    }
                }
                DrawCommand::Close => {
                    if current_polygon.len() >= 3 {
                        polygons.push(current_polygon);
                        current_polygon = Vec::new();
                        has_move = false;
                    }
                }
            }
        }

        if current_polygon.len() >= 3 {
            polygons.push(current_polygon);
        }

        polygons
    }

    /// Fill a polygon using scanline algorithm
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

        // Find bounding box
        let mut min_y = f64::MAX;
        let mut max_y = f64::MIN;
        for &(_, py) in polygon {
            let screen_y = py * scale + offset_y;
            min_y = min_y.min(screen_y);
            max_y = max_y.max(screen_y);
        }

        let min_y = (min_y as i32).max(0);
        let max_y = (max_y as i32).min(buffer.height as i32 - 1);

        // Scanline fill
        for scan_y in min_y..=max_y {
            let mut intersections = Vec::new();

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

            // Fill between pairs of intersections
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
