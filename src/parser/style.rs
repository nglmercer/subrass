use crate::types::Style;

use super::errors::ParseError;

pub fn parse_styles(
    lines: &[&str],
    start_line: usize,
) -> Result<Vec<Style>, ParseError> {
    let mut styles = Vec::new();
    let mut format_line: Option<String> = None;

    for (i, line) in lines.iter().enumerate() {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with(';') {
            continue;
        }

        // Parse format line
        if let Some(format) = line.strip_prefix("Format:") {
            format_line = Some(format.to_string());
            continue;
        }

        // Parse style lines
        if let Some(style_data) = line.strip_prefix("Style:") {
            let style = parse_style_line(style_data, &format_line, start_line + i)?;
            styles.push(style);
        }
    }

    Ok(styles)
}

fn parse_style_line(
    data: &str,
    _format: &Option<String>,
    line_num: usize,
) -> Result<Style, ParseError> {
    // The format is always the same for V4+ styles
    // Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour,
    //         Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle,
    //         Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding

    let style = Style::parse_from_line(data)
        .map_err(|e| ParseError::line_error(line_num, e))?;

    Ok(style)
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

        let styles = parse_styles(&lines, 0).unwrap();
        assert_eq!(styles.len(), 1);
        assert_eq!(styles[0].name, "Default");
        assert_eq!(styles[0].font_name, "Arial");
    }

    #[test]
    fn test_parse_multiple_styles() {
        let lines = vec![
            "Style: Default,Arial,48,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,-1,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1",
            "Style: Signs,Impact,64,&H0000FFFF,&H000000FF,&H00000000,&H80000000,-1,0,0,0,100,100,0,0,1,3,2,8,20,20,20,1",
        ];

        let styles = parse_styles(&lines, 0).unwrap();
        assert_eq!(styles.len(), 2);
        assert_eq!(styles[0].name, "Default");
        assert_eq!(styles[1].name, "Signs");
    }

    #[test]
    fn test_parse_styles_skips_comments() {
        let lines = vec![
            "; This is a comment",
            "Style: Default,Arial,48,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,-1,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1",
            "; Another comment",
        ];

        let styles = parse_styles(&lines, 0).unwrap();
        assert_eq!(styles.len(), 1);
    }

    #[test]
    fn test_parse_styles_empty() {
        let lines: Vec<&str> = vec![];
        let styles = parse_styles(&lines, 0).unwrap();
        assert!(styles.is_empty());
    }
}
