//! Lexer for ASS vector-drawing commands.

use super::MAX_DRAWING_COMMANDS;

#[derive(Debug, Clone)]
pub(super) enum DrawCommand {
    /// Move to a point. close_previous distinguishes m from n.
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
    /// Cubic B-spline (s, extended by p, closed by c).
    SplineTo {
        points: Vec<(f64, f64)>,
        closed: bool,
    },
    Close,
}

/// Parse ASS drawing commands into bounded intermediate commands.
pub(super) fn parse(text: &str) -> Vec<DrawCommand> {
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
                while let Some((x, y)) = next_coord_pair(&mut chars) {
                    push!(DrawCommand::MoveTo {
                        x,
                        y,
                        close_previous: true,
                    });
                }
            }
            'n' | 'N' => {
                chars.next();
                if !m_seen {
                    return Vec::new();
                }
                spline_active = false;
                while let Some((x, y)) = next_coord_pair(&mut chars) {
                    push!(DrawCommand::MoveTo {
                        x,
                        y,
                        close_previous: false,
                    });
                }
            }
            'l' | 'L' => {
                chars.next();
                if !m_seen {
                    continue;
                }
                while let Some((x, y)) = next_coord_pair(&mut chars) {
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
                    next_coord_pair(&mut chars),
                    next_coord_pair(&mut chars),
                    next_coord_pair(&mut chars),
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
                while let Some(pt) = next_coord_pair(&mut chars) {
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
                while let Some(pt) = next_coord_pair(&mut chars) {
                    points.push(pt);
                }
                if points.is_empty() {
                    continue;
                }
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
    skip_whitespace(chars);
    let (x, _) = parse_number(chars)?;
    let (y, _) = parse_number(chars)?;
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
    skip_whitespace(chars);

    let mut value: f64 = 0.0;
    let mut sign = 1.0;
    let mut has_digits = false;
    let mut decimal_place = 0.0;

    if let Some(&ch) = chars.peek() {
        if ch == '-' {
            sign = -1.0;
            chars.next();
        } else if ch == '+' {
            chars.next();
        }
    }

    while let Some(&ch) = chars.peek() {
        if ch.is_ascii_digit() {
            value = value * 10.0 + (ch as u32 - b'0' as u32) as f64;
            has_digits = true;
            chars.next();
        } else {
            break;
        }
    }

    if let Some(&'.') = chars.peek() {
        chars.next();
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
    if decimal_place > 0.0 {
        value /= 10.0f64.powf(decimal_place);
    }

    Some((sign * value, ()))
}
