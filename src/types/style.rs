use serde::{Deserialize, Serialize};

use super::color::Color;

/// Style definition from [V4+ Styles] section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Style {
    pub name: String,
    pub font_name: String,
    pub font_size: f64,
    pub primary_color: Color,
    pub secondary_color: Color,
    pub outline_color: Color,
    pub back_color: Color,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike_out: bool,
    pub scale_x: f64,
    pub scale_y: f64,
    pub spacing: f64,
    pub angle: f64,
    pub border_style: i32,
    pub outline: f64,
    pub shadow: f64,
    pub alignment: i32,
    pub margin_l: i32,
    pub margin_r: i32,
    pub margin_v: i32,
    pub encoding: i32,
}

impl Style {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            font_name: "Arial".to_string(),
            font_size: 48.0,
            primary_color: Color::white(),
            secondary_color: Color::new(0, 0, 255, 0),
            outline_color: Color::black(),
            back_color: Color::new(128, 0, 0, 0),
            bold: false,
            italic: false,
            underline: false,
            strike_out: false,
            scale_x: 100.0,
            scale_y: 100.0,
            spacing: 0.0,
            angle: 0.0,
            border_style: 1,
            outline: 2.0,
            shadow: 1.0,
            alignment: 2,
            margin_l: 10,
            margin_r: 10,
            margin_v: 40,
            encoding: 1,
        }
    }

    pub fn parse_from_line(line: &str) -> Result<Self, String> {
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() < 23 {
            return Err(format!(
                "Expected 23 fields in style definition, got {}",
                fields.len()
            ));
        }

        let parse_bool = |s: &str| -> bool { s.trim() == "-1" };
        let parse_i32 = |s: &str| -> i32 { s.trim().parse().unwrap_or(0) };
        let parse_f64 = |s: &str| -> f64 { s.trim().parse().unwrap_or(0.0) };

        Ok(Self {
            name: fields[0].trim().to_string(),
            font_name: fields[1].trim().to_string(),
            font_size: parse_f64(fields[2]),
            primary_color: fields[3].trim().parse().unwrap_or_default(),
            secondary_color: fields[4].trim().parse().unwrap_or_default(),
            outline_color: fields[5].trim().parse().unwrap_or_default(),
            back_color: fields[6].trim().parse().unwrap_or_default(),
            bold: parse_bool(fields[7]),
            italic: parse_bool(fields[8]),
            underline: parse_bool(fields[9]),
            strike_out: parse_bool(fields[10]),
            scale_x: parse_f64(fields[11]),
            scale_y: parse_f64(fields[12]),
            spacing: parse_f64(fields[13]),
            angle: parse_f64(fields[14]),
            border_style: parse_i32(fields[15]),
            outline: parse_f64(fields[16]),
            shadow: parse_f64(fields[17]),
            alignment: parse_i32(fields[18]),
            margin_l: parse_i32(fields[19]),
            margin_r: parse_i32(fields[20]),
            margin_v: parse_i32(fields[21]),
            encoding: parse_i32(fields[22]),
        })
    }

    pub fn to_line(&self) -> String {
        let bool_str = |b: bool| if b { "-1" } else { "0" }.to_string();

        format!(
            "Style: {},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            self.name,
            self.font_name,
            self.font_size,
            self.primary_color,
            self.secondary_color,
            self.outline_color,
            self.back_color,
            bool_str(self.bold),
            bool_str(self.italic),
            bool_str(self.underline),
            bool_str(self.strike_out),
            self.scale_x,
            self.scale_y,
            self.spacing,
            self.angle,
            self.border_style,
            self.outline,
            self.shadow,
            self.alignment,
            self.margin_l,
            self.margin_r,
            self.margin_v,
            self.encoding
        )
    }
}

pub const DEFAULT_STYLE_FORMAT: &str = "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_style_line() {
        let line = "Default,Arial,48,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,-1,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1";
        let style = Style::parse_from_line(line).unwrap();

        assert_eq!(style.name, "Default");
        assert_eq!(style.font_name, "Arial");
        assert_eq!(style.font_size, 48.0);
        assert!(style.bold);
        assert!(!style.italic);
        assert_eq!(style.alignment, 2);
        assert_eq!(style.margin_l, 10);
        assert_eq!(style.margin_r, 10);
        assert_eq!(style.margin_v, 40);
    }

    #[test]
    fn test_style_to_line() {
        let mut style = Style::new("Test");
        style.bold = true;
        style.italic = true;
        style.font_size = 32.0;

        let line = style.to_line();
        assert!(line.starts_with("Style: Test,"));
        assert!(line.contains("Arial"));
        assert!(line.contains("32"));
        assert!(line.contains("-1"));
    }

    #[test]
    fn test_invalid_style_line() {
        let line = "Invalid";
        assert!(Style::parse_from_line(line).is_err());
    }
}
