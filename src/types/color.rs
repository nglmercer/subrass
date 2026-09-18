use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// ASS color representation in &HAABBGGRR format.
///
/// Invariant: `alpha` stores **ASS transparency** — 0 means fully opaque
/// (visible) and 255 means fully transparent (invisible). This is the
/// opposite of conventional RGBA opacity. Every conversion to opacity,
/// coverage, or CSS form must go through the explicit helpers below so
/// the direction of the conversion is visible at the call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Color {
    /// ASS transparency: 0 = opaque, 255 = transparent.
    pub alpha: u8,
    pub blue: u8,
    pub green: u8,
    pub red: u8,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ColorError {
    #[error("Invalid color format: {0}")]
    InvalidFormat(String),
    #[error("Invalid color component: {0}")]
    InvalidComponent(String),
}

impl Color {
    pub fn new(alpha: u8, red: u8, green: u8, blue: u8) -> Self {
        Self {
            alpha,
            blue,
            green,
            red,
        }
    }

    /// Build from components where `alpha` is ASS transparency
    /// (0 = opaque). Prefer [`Color::from_standard_rgba`] for
    /// conventional opacity-based input.
    pub fn from_rgba(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self::new(alpha, red, green, blue)
    }

    /// Build from conventional RGBA where `opacity` is 0 = transparent,
    /// 255 = opaque.
    pub fn from_standard_rgba(red: u8, green: u8, blue: u8, opacity: u8) -> Self {
        Self::new(255 - opacity, red, green, blue)
    }

    /// Raw components `[red, green, blue, ass_transparency]`.
    ///
    /// NOTE: the fourth slot is ASS transparency, **not** standard RGBA
    /// opacity. Prefer [`Color::to_straight_rgba`] when opacity is meant.
    pub fn to_rgba(&self) -> [u8; 4] {
        [self.red, self.green, self.blue, self.alpha]
    }

    /// Raw ASS components `[red, green, blue, ass_transparency]`.
    /// Same as [`Color::to_rgba`], with the meaning in the name.
    pub fn to_ass_components(&self) -> [u8; 4] {
        self.to_rgba()
    }

    /// Conventional straight-alpha `[red, green, blue, opacity]`
    /// (0 = transparent, 255 = opaque).
    pub fn to_straight_rgba(&self) -> [u8; 4] {
        [self.red, self.green, self.blue, 255 - self.alpha]
    }

    /// CSS `#RRGGBBAA` with standard opacity in the `AA` slot.
    pub fn to_hex(&self) -> String {
        format!(
            "#{:02X}{:02X}{:02X}{:02X}",
            self.red,
            self.green,
            self.blue,
            255 - self.alpha
        )
    }

    pub fn to_css_rgba(&self) -> String {
        let a = 1.0 - (self.alpha as f64 / 255.0);
        format!("rgba({}, {}, {}, {})", self.red, self.green, self.blue, a)
    }

    /// ASS transparency: 0 = opaque, 255 = transparent.
    pub fn alpha(&self) -> u8 {
        self.alpha
    }

    /// ASS transparency: 0 = opaque, 255 = transparent.
    pub fn transparency(&self) -> u8 {
        self.alpha
    }

    /// Conventional opacity: 0 = transparent, 255 = opaque.
    pub fn opacity(&self) -> u8 {
        255 - self.alpha
    }

    pub fn red(&self) -> u8 {
        self.red
    }

    pub fn green(&self) -> u8 {
        self.green
    }

    pub fn blue(&self) -> u8 {
        self.blue
    }

    pub fn with_alpha(&self, alpha: u8) -> Self {
        Self { alpha, ..*self }
    }

    pub fn opaque() -> Self {
        Self::new(0, 0, 0, 0)
    }

    pub fn white() -> Self {
        Self::new(0, 255, 255, 255)
    }

    pub fn black() -> Self {
        Self::new(0, 0, 0, 0)
    }
}

impl FromStr for Color {
    type Err = ColorError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();

        // Handle &HAABBGGRR format (ASS standard)
        if let Some(hex) = s.strip_prefix("&H").or_else(|| s.strip_prefix("&h")) {
            return parse_ass_hex_color(hex);
        }

        // Handle #RRGGBB or #RRGGBBAA format (CSS standard)
        if let Some(hex) = s.strip_prefix('#') {
            return parse_css_hex_color(hex);
        }

        // Handle H AABBGGRR format (without &)
        if s.len() == 8 {
            return parse_ass_hex_color(s);
        }

        Err(ColorError::InvalidFormat(s.to_string()))
    }
}

fn parse_ass_hex_color(hex: &str) -> Result<Color, ColorError> {
    let hex = hex.trim_end_matches('&');

    match hex.len() {
        8 => {
            // AABBGGRR format (ASS standard)
            let alpha = u8::from_str_radix(&hex[0..2], 16)
                .map_err(|_| ColorError::InvalidComponent(format!("alpha: {}", &hex[0..2])))?;
            let blue = u8::from_str_radix(&hex[2..4], 16)
                .map_err(|_| ColorError::InvalidComponent(format!("blue: {}", &hex[2..4])))?;
            let green = u8::from_str_radix(&hex[4..6], 16)
                .map_err(|_| ColorError::InvalidComponent(format!("green: {}", &hex[4..6])))?;
            let red = u8::from_str_radix(&hex[6..8], 16)
                .map_err(|_| ColorError::InvalidComponent(format!("red: {}", &hex[6..8])))?;

            Ok(Color::new(alpha, red, green, blue))
        }
        6 => {
            // BGR format for ASS colors without alpha
            let blue = u8::from_str_radix(&hex[0..2], 16)
                .map_err(|_| ColorError::InvalidComponent(format!("blue: {}", &hex[0..2])))?;
            let green = u8::from_str_radix(&hex[2..4], 16)
                .map_err(|_| ColorError::InvalidComponent(format!("green: {}", &hex[2..4])))?;
            let red = u8::from_str_radix(&hex[4..6], 16)
                .map_err(|_| ColorError::InvalidComponent(format!("red: {}", &hex[4..6])))?;

            Ok(Color::new(0, red, green, blue))
        }
        _ => Err(ColorError::InvalidComponent(format!(
            "expected 6 or 8 hex digits, got {}",
            hex.len()
        ))),
    }
}

fn parse_css_hex_color(hex: &str) -> Result<Color, ColorError> {
    match hex.len() {
        8 => {
            // #RRGGBBAA format (CSS standard): AA is opacity, converted
            // to ASS transparency.
            let red = u8::from_str_radix(&hex[0..2], 16)
                .map_err(|_| ColorError::InvalidComponent(format!("red: {}", &hex[0..2])))?;
            let green = u8::from_str_radix(&hex[2..4], 16)
                .map_err(|_| ColorError::InvalidComponent(format!("green: {}", &hex[2..4])))?;
            let blue = u8::from_str_radix(&hex[4..6], 16)
                .map_err(|_| ColorError::InvalidComponent(format!("blue: {}", &hex[4..6])))?;
            let opacity = u8::from_str_radix(&hex[6..8], 16)
                .map_err(|_| ColorError::InvalidComponent(format!("alpha: {}", &hex[6..8])))?;

            Ok(Color::from_standard_rgba(red, green, blue, opacity))
        }
        6 => {
            // #RRGGBB format
            let red = u8::from_str_radix(&hex[0..2], 16)
                .map_err(|_| ColorError::InvalidComponent(format!("red: {}", &hex[0..2])))?;
            let green = u8::from_str_radix(&hex[2..4], 16)
                .map_err(|_| ColorError::InvalidComponent(format!("green: {}", &hex[2..4])))?;
            let blue = u8::from_str_radix(&hex[4..6], 16)
                .map_err(|_| ColorError::InvalidComponent(format!("blue: {}", &hex[4..6])))?;

            Ok(Color::new(0, red, green, blue))
        }
        _ => Err(ColorError::InvalidComponent(format!(
            "expected 6 or 8 hex digits, got {}",
            hex.len()
        ))),
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "&H{:02X}{:02X}{:02X}{:02X}&",
            self.alpha, self.blue, self.green, self.red
        )
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::white()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ass_color() {
        let color: Color = "&H00FFFFFF&".parse().unwrap();
        assert_eq!(color.alpha(), 0);
        assert_eq!(color.red(), 255);
        assert_eq!(color.green(), 255);
        assert_eq!(color.blue(), 255);
    }

    #[test]
    fn test_parse_ass_color_with_alpha() {
        let color: Color = "&H800000FF&".parse().unwrap();
        assert_eq!(color.alpha(), 128);
        assert_eq!(color.red(), 255);
        assert_eq!(color.green(), 0);
        assert_eq!(color.blue(), 0);
    }

    #[test]
    fn test_parse_css_hex_color() {
        // #RRGGBBAA: opaque red
        let color: Color = "#FF0000FF".parse().unwrap();
        assert_eq!(color.red(), 255);
        assert_eq!(color.green(), 0);
        assert_eq!(color.blue(), 0);
        assert_eq!(color.transparency(), 0);
        assert_eq!(color.opacity(), 255);
    }

    #[test]
    fn test_parse_css_hex_half_opacity() {
        // #FF000080: red at ~50% opacity -> ~50% ASS transparency
        let color: Color = "#FF000080".parse().unwrap();
        assert_eq!(color.red(), 255);
        assert_eq!(color.opacity(), 128);
        assert_eq!(color.transparency(), 127);
        assert_eq!(color.to_hex(), "#FF000080");
    }

    #[test]
    fn test_to_rgba() {
        let color = Color::new(0, 255, 128, 0);
        assert_eq!(color.to_rgba(), [255, 128, 0, 0]);
    }

    #[test]
    fn test_straight_rgba_inverts_transparency() {
        let color = Color::new(64, 10, 20, 30);
        assert_eq!(color.to_straight_rgba(), [10, 20, 30, 191]);
        assert_eq!(Color::from_standard_rgba(10, 20, 30, 191), color);
    }

    #[test]
    fn test_to_css_rgba() {
        let color = Color::new(128, 255, 0, 0);
        let css = color.to_css_rgba();
        assert!(css.starts_with("rgba(255, 0, 0,"));
    }

    #[test]
    fn test_invalid_color() {
        assert!("invalid".parse::<Color>().is_err());
        assert!("&HGG&".parse::<Color>().is_err());
    }

    #[test]
    fn test_display() {
        let color = Color::new(0, 255, 128, 0);
        assert_eq!(color.to_string(), "&H000080FF&");
    }
}
