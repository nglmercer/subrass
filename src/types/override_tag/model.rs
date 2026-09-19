use super::OverrideTag;

/// A segment of text with its associated override tags.
#[derive(Debug, Clone)]
pub struct TextSegment {
    pub text: String,
    pub tags: Vec<OverrideTag>,
}

/// Normalize a `\b` value to an ASS font weight (1..=1000):
/// `\b0` is normal (400), `\b1` is bold (700), other values are
/// explicit weights (clamped); negative values fall back to bold,
/// matching the historical nonzero-means-bold behavior.
pub fn ass_bold_weight(value: i32) -> u16 {
    match value {
        0 => 400,
        1 => 700,
        v if v < 0 => 700,
        v => v.clamp(1, 1000) as u16,
    }
}
