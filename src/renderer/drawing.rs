use crate::renderer::buffer::RenderBuffer;

/// Drawing command types
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
        let mut start_x = 0.0_f64;
        let mut start_y = 0.0_f64;

        // Tokenize: split by command letters
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            let ch = chars[i];

            match ch {
                'm' | 'M' => {
                    i += 1;
                    // Parse coordinate pairs after m/M
                    while i < chars.len() {
                        // Skip whitespace and commas
                        while i < chars.len() && (chars[i] == ' ' || chars[i] == ',') {
                            i += 1;
                        }
                        // Check if next char is a digit or sign (coordinate)
                        if i < chars.len()
                            && (chars[i].is_ascii_digit() || chars[i] == '-' || chars[i] == '.')
                        {
                            if let Some((coords, new_i)) = Self::parse_coordinate_pair(&chars, i) {
                                current_x = coords.0;
                                current_y = coords.1;
                                start_x = current_x;
                                start_y = current_y;
                                commands.push(DrawCommand::MoveTo {
                                    x: current_x,
                                    y: current_y,
                                });
                                i = new_i;
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }
                'l' | 'L' => {
                    i += 1;
                    // Parse coordinate pairs after l/L
                    while i < chars.len() {
                        while i < chars.len() && (chars[i] == ' ' || chars[i] == ',') {
                            i += 1;
                        }
                        if i < chars.len()
                            && (chars[i].is_ascii_digit() || chars[i] == '-' || chars[i] == '.')
                        {
                            if let Some((coords, new_i)) = Self::parse_coordinate_pair(&chars, i) {
                                current_x = coords.0;
                                current_y = coords.1;
                                commands.push(DrawCommand::LineTo {
                                    x: current_x,
                                    y: current_y,
                                });
                                i = new_i;
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }
                'b' | 'B' => {
                    i += 1;
                    // Parse 3 coordinate pairs (6 values) after b/B
                    while i < chars.len() {
                        while i < chars.len() && (chars[i] == ' ' || chars[i] == ',') {
                            i += 1;
                        }
                        if i < chars.len()
                            && (chars[i].is_ascii_digit() || chars[i] == '-' || chars[i] == '.')
                        {
                            if let Some((coords1, new_i)) = Self::parse_coordinate_pair(&chars, i) {
                                i = new_i;
                                while i < chars.len() && (chars[i] == ' ' || chars[i] == ',') {
                                    i += 1;
                                }
                                if let Some((coords2, new_i)) =
                                    Self::parse_coordinate_pair(&chars, i)
                                {
                                    i = new_i;
                                    while i < chars.len() && (chars[i] == ' ' || chars[i] == ',') {
                                        i += 1;
                                    }
                                    if let Some((coords3, new_i)) =
                                        Self::parse_coordinate_pair(&chars, i)
                                    {
                                        current_x = coords3.0;
                                        current_y = coords3.1;
                                        commands.push(DrawCommand::CurveTo {
                                            x1: coords1.0,
                                            y1: coords1.1,
                                            x2: coords2.0,
                                            y2: coords2.1,
                                            x: coords3.0,
                                            y: coords3.1,
                                        });
                                        i = new_i;
                                    } else {
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }
                'n' | 'N' => {
                    i += 1;
                    // n = shorthand bezier (ctrl1 = current point)
                    while i < chars.len() {
                        while i < chars.len() && (chars[i] == ' ' || chars[i] == ',') {
                            i += 1;
                        }
                        if i < chars.len()
                            && (chars[i].is_ascii_digit() || chars[i] == '-' || chars[i] == '.')
                        {
                            if let Some((coords1, new_i)) = Self::parse_coordinate_pair(&chars, i) {
                                i = new_i;
                                while i < chars.len() && (chars[i] == ' ' || chars[i] == ',') {
                                    i += 1;
                                }
                                if let Some((coords2, new_i)) =
                                    Self::parse_coordinate_pair(&chars, i)
                                {
                                    current_x = coords2.0;
                                    current_y = coords2.1;
                                    commands.push(DrawCommand::CurveTo {
                                        x1: current_x,
                                        y1: current_y, // Previous point
                                        x2: coords1.0,
                                        y2: coords1.1,
                                        x: coords2.0,
                                        y: coords2.1,
                                    });
                                    i = new_i;
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }
                'c' | 'C' => {
                    i += 1;
                    commands.push(DrawCommand::Close);
                    current_x = start_x;
                    current_y = start_y;
                }
                _ => {
                    i += 1;
                }
            }
        }

        commands
    }

    /// Parse a coordinate pair (x, y) from chars starting at position
    fn parse_coordinate_pair(chars: &[char], start: usize) -> Option<((f64, f64), usize)> {
        let (x, i) = Self::parse_number(chars, start)?;
        let (y, i) = Self::parse_number(chars, i)?;
        Some(((x, y), i))
    }

    /// Parse a number from chars starting at position
    fn parse_number(chars: &[char], start: usize) -> Option<(f64, usize)> {
        let mut i = start;
        let mut num_str = String::new();

        // Skip leading whitespace
        while i < chars.len() && chars[i] == ' ' {
            i += 1;
        }

        // Optional sign
        if i < chars.len() && (chars[i] == '-' || chars[i] == '+') {
            num_str.push(chars[i]);
            i += 1;
        }

        // Digits and optional decimal point
        let mut has_digits = false;
        while i < chars.len() {
            if chars[i].is_ascii_digit() {
                num_str.push(chars[i]);
                has_digits = true;
                i += 1;
            } else if chars[i] == '.' {
                num_str.push(chars[i]);
                i += 1;
            } else {
                break;
            }
        }

        if !has_digits || num_str == "-" || num_str == "+" {
            return None;
        }

        let value: f64 = num_str.parse().ok()?;
        Some((value, i))
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
                DrawCommand::CurveTo {
                    x1,
                    y1,
                    x2,
                    y2,
                    x,
                    y,
                } => {
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
                            let px =
                                mt3 * last.0 + 3.0 * mt2 * t * x1 + 3.0 * mt * t2 * x2 + t3 * x;
                            let py =
                                mt3 * last.1 + 3.0 * mt2 * t * y1 + 3.0 * mt * t2 * y2 + t3 * y;
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
