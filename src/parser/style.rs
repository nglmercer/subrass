use crate::types::Style;

use super::errors::ParseError;

/// Canonical SSA v4.00 style column layout, used when a [V4 Styles]
/// section has no Format line of its own.
const SSA_DEFAULT_FORMAT: &str = "Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, \
    TertiaryColour, BackColour, Bold, Italic, BorderStyle, Outline, Shadow, Alignment, \
    MarginL, MarginR, MarginV, AlphaLevel, Encoding";

pub fn parse_styles(
    lines: &[&str],
    start_line: usize,
    is_ssa: bool,
) -> Result<Vec<Style>, ParseError> {
    let mut styles = Vec::new();
    let mut format: Option<Vec<String>> = None;

    for (i, line) in lines.iter().enumerate() {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with(';') {
            continue;
        }

        // Parse format line
        if let Some(fmt) = line.strip_prefix("Format:") {
            format = Some(parse_format_columns(fmt));
            continue;
        }

        // Parse style lines
        if let Some(style_data) = line.strip_prefix("Style:") {
            let style = parse_style_line(style_data, &format, is_ssa, start_line + i)?;
            styles.push(style);
        }
    }

    Ok(styles)
}

fn parse_format_columns(fmt: &str) -> Vec<String> {
    fmt.split(',')
        .map(|c| c.trim().to_lowercase())
        .filter(|c| !c.is_empty())
        .collect()
}

fn parse_style_line(
    data: &str,
    format: &Option<Vec<String>>,
    is_ssa: bool,
    line_num: usize,
) -> Result<Style, ParseError> {
    let columns = match format {
        Some(cols) if !cols.is_empty() => cols.clone(),
        // No Format line: assume the canonical layout for the section kind
        _ if is_ssa => parse_format_columns(SSA_DEFAULT_FORMAT),
        _ => Vec::new(),
    };

    let mut style = if columns.is_empty() {
        // Legacy fixed V4+ column order
        Style::parse_from_line(data).map_err(|e| ParseError::line_error(line_num, e))?
    } else {
        parse_style_by_columns(data, &columns).map_err(|e| ParseError::line_error(line_num, e))?
    };

    // SSA scripts use the legacy alignment numbering (1-3 bottom, 5-7 top,
    // 9-11 middle); convert to ASS numpad alignment.
    if is_ssa {
        style.alignment = ssa_alignment_to_ass(style.alignment);
    }

    Ok(style)
}

/// Map style fields by column name, so both SSA [V4 Styles] and ASS
/// [V4+ Styles] lines (even with reordered columns) parse correctly.
/// Unknown columns are ignored; missing ones keep their defaults.
fn parse_style_by_columns(data: &str, columns: &[String]) -> Result<Style, String> {
    let fields: Vec<&str> = data.split(',').map(|s| s.trim()).collect();
    let mut style = Style::new("");
    let mut has_name = false;

    for (i, column) in columns.iter().enumerate() {
        let Some(value) = fields.get(i) else {
            break;
        };
        if column == "name" && !value.is_empty() {
            has_name = true;
        }
        apply_style_field(&mut style, column, value);
    }

    if !has_name {
        return Err(format!("Style line is missing a Name value: {}", data));
    }

    Ok(style)
}

fn apply_style_field(style: &mut Style, column: &str, value: &str) {
    let value = value.trim();
    let as_bool = || value.parse::<i32>().map(|v| v != 0).unwrap_or(false);
    let as_i32 = || value.parse::<i32>().unwrap_or(0);
    let as_f64 = || value.parse::<f64>().unwrap_or(0.0);

    match column {
        "name" => style.name = value.to_string(),
        "fontname" => style.font_name = value.to_string(),
        "fontsize" => style.font_size = as_f64(),
        "primarycolour" => style.primary_color = value.parse().unwrap_or_default(),
        "secondarycolour" => style.secondary_color = value.parse().unwrap_or_default(),
        // SSA's TertiaryColour fills the outline role
        "outlinecolour" | "tertiarycolour" => {
            style.outline_color = value.parse().unwrap_or_default()
        }
        "backcolour" => style.back_color = value.parse().unwrap_or_default(),
        "bold" => style.bold = as_bool(),
        "italic" => style.italic = as_bool(),
        "underline" => style.underline = as_bool(),
        "strikeout" => style.strike_out = as_bool(),
        "scalex" => style.scale_x = as_f64(),
        "scaley" => style.scale_y = as_f64(),
        "spacing" => style.spacing = as_f64(),
        "angle" => style.angle = as_f64(),
        "borderstyle" => style.border_style = as_i32(),
        "outline" => style.outline = as_f64(),
        "shadow" => style.shadow = as_f64(),
        "alignment" => style.alignment = as_i32(),
        "marginl" => style.margin_l = as_i32(),
        "marginr" => style.margin_r = as_i32(),
        "marginv" => style.margin_v = as_i32(),
        "alphalevel" => {
            // SSA alpha level applies to the whole style (0 = opaque)
            let alpha = as_i32().clamp(0, 255) as u8;
            style.primary_color.alpha = alpha;
            style.secondary_color.alpha = alpha;
            style.outline_color.alpha = alpha;
            style.back_color.alpha = alpha;
        }
        "encoding" => style.encoding = as_i32(),
        _ => {}
    }
}

/// Convert legacy SSA alignment values to ASS numpad alignment:
/// SSA 1-3 (bottom) stay, 5-7 (top) become 7-9, 9-11 (middle) become 4-6.
fn ssa_alignment_to_ass(alignment: i32) -> i32 {
    match alignment {
        1..=3 => alignment,
        5..=7 => alignment + 2,
        9..=11 => alignment - 5,
        _ => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_styles_basic() {
        let lines = vec![
            "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding",
            "Style: Default,Arial,48,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,-1,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1",
        ];

        let styles = parse_styles(&lines, 0, false).unwrap();
        assert_eq!(styles.len(), 1);
        assert_eq!(styles[0].name, "Default");
        assert_eq!(styles[0].font_name, "Arial");
    }

    #[test]
    fn test_parse_multiple_styles() {
        let lines = vec![
            "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding",
            "Style: Default,Arial,48,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,-1,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1",
            "Style: Signs,Impact,64,&H0000FFFF,&H000000FF,&H00000000,&H80000000,-1,0,0,0,100,100,0,0,1,3,2,8,20,20,20,1",
        ];

        let styles = parse_styles(&lines, 0, false).unwrap();
        assert_eq!(styles.len(), 2);
        assert_eq!(styles[0].name, "Default");
        assert_eq!(styles[1].name, "Signs");
    }

    #[test]
    fn test_parse_styles_skips_comments() {
        let lines = vec![
            "; This is a comment",
            "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding",
            "Style: Default,Arial,48,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,-1,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1",
            "; Another comment",
        ];

        let styles = parse_styles(&lines, 0, false).unwrap();
        assert_eq!(styles.len(), 1);
    }

    #[test]
    fn test_parse_styles_empty() {
        let lines: Vec<&str> = vec![];
        let styles = parse_styles(&lines, 0, false).unwrap();
        assert!(styles.is_empty());
    }

    #[test]
    fn test_parse_ssa_v4_styles() {
        let lines = vec![
            "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, TertiaryColour, BackColour, Bold, Italic, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, AlphaLevel, Encoding",
            "Style: Default,Arial,48,&HFFFF00,&H0000FF,&H000000,&H800000,-1,0,1,2,1,6,10,10,40,0,1",
        ];

        let styles = parse_styles(&lines, 0, true).unwrap();
        assert_eq!(styles.len(), 1);
        let style = &styles[0];
        assert_eq!(style.name, "Default");
        assert_eq!(style.font_name, "Arial");
        assert_eq!(style.font_size, 48.0);
        // SSA TertiaryColour maps to the outline color
        assert_eq!(style.outline_color.blue, 0x00);
        assert!(style.bold);
        assert!(!style.italic);
        // V4+ only fields keep their defaults
        assert!(!style.underline);
        assert_eq!(style.scale_x, 100.0);
        // SSA alignment 6 (top-center) becomes numpad 8
        assert_eq!(style.alignment, 8);
    }

    #[test]
    fn test_parse_ssa_styles_without_format_line() {
        let lines = vec![
            "Style: Default,Arial,48,&HFFFF00,&H0000FF,&H000000,&H800000,-1,0,1,2,1,10,10,10,40,0,1",
        ];

        let styles = parse_styles(&lines, 0, true).unwrap();
        assert_eq!(styles.len(), 1);
        assert_eq!(styles[0].name, "Default");
        // SSA alignment 10 (middle-center) becomes numpad 5
        assert_eq!(styles[0].alignment, 5);
    }

    #[test]
    fn test_ssa_alpha_level_applies_to_colors() {
        let lines = vec![
            "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, TertiaryColour, BackColour, Bold, Italic, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, AlphaLevel, Encoding",
            "Style: Default,Arial,48,&HFFFFFF,&H0000FF,&H000000,&H000000,0,0,1,2,1,2,10,10,40,32,1",
        ];

        let styles = parse_styles(&lines, 0, true).unwrap();
        assert_eq!(styles[0].primary_color.alpha, 32);
        assert_eq!(styles[0].outline_color.alpha, 32);
    }

    #[test]
    fn test_reordered_v4plus_columns() {
        let lines = vec![
            "Format: Name, Fontsize, Fontname, Alignment, PrimaryColour",
            "Style: Minimal,36,Verdana,8,&H00FF00&",
        ];

        let styles = parse_styles(&lines, 0, false).unwrap();
        assert_eq!(styles.len(), 1);
        let style = &styles[0];
        assert_eq!(style.name, "Minimal");
        assert_eq!(style.font_size, 36.0);
        assert_eq!(style.font_name, "Verdana");
        // V4+ alignment is kept as-is (numpad)
        assert_eq!(style.alignment, 8);
        assert_eq!(style.primary_color.green, 0xFF);
        // Untouched columns keep defaults
        assert_eq!(style.outline, 2.0);
    }

    #[test]
    fn test_ssa_alignment_conversion() {
        assert_eq!(ssa_alignment_to_ass(2), 2);
        assert_eq!(ssa_alignment_to_ass(6), 8);
        assert_eq!(ssa_alignment_to_ass(10), 5);
        assert_eq!(ssa_alignment_to_ass(99), 2);
    }
}
