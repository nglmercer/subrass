use super::{Color, YCbCrMatrix};

/// Y'CbCr coefficient families used by ASS/host video conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum VideoMatrix {
    Bt601,
    Bt709,
    Smpte240M,
    Fcc,
}

/// Code-value range of the destination video signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum VideoRange {
    Limited,
    Full,
}

/// Explicit destination video colorspace required for subtitle color
/// conversion. No renderer path guesses this from resolution or headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct VideoColorSpace {
    pub matrix: VideoMatrix,
    pub range: VideoRange,
}

impl VideoColorSpace {
    pub const fn new(matrix: VideoMatrix, range: VideoRange) -> Self {
        Self { matrix, range }
    }

    pub const BT601_LIMITED: Self = Self::new(VideoMatrix::Bt601, VideoRange::Limited);
    pub const BT601_FULL: Self = Self::new(VideoMatrix::Bt601, VideoRange::Full);
    pub const BT709_LIMITED: Self = Self::new(VideoMatrix::Bt709, VideoRange::Limited);
    pub const BT709_FULL: Self = Self::new(VideoMatrix::Bt709, VideoRange::Full);
    pub const SMPTE240M_LIMITED: Self = Self::new(VideoMatrix::Smpte240M, VideoRange::Limited);
    pub const SMPTE240M_FULL: Self = Self::new(VideoMatrix::Smpte240M, VideoRange::Full);
    pub const FCC_LIMITED: Self = Self::new(VideoMatrix::Fcc, VideoRange::Limited);
    pub const FCC_FULL: Self = Self::new(VideoMatrix::Fcc, VideoRange::Full);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ColorConversionError {
    #[error("YCbCr matrix metadata is unknown and cannot be converted")]
    UnknownMatrix,
}

/// Convert authored ASS RGB through the script-declared Y'CbCr source matrix
/// into an explicitly supplied destination video colorspace.
///
/// `YCbCrMatrix::None` returns the authored RGB untouched. `Default` follows
/// libass's documented default source of limited-range BT.601. `Unknown`
/// cannot be converted safely and returns an error. Alpha is never changed.
pub fn convert_ass_rgb(
    color: Color,
    source: YCbCrMatrix,
    destination: VideoColorSpace,
) -> Result<Color, ColorConversionError> {
    let Some((source_matrix, source_range)) = source_spec(source)? else {
        return Ok(color);
    };

    let (y, cb, cr) = rgb_to_ycbcr(color, source_matrix);
    let (y, cb, cr) = encode_range(y, cb, cr, source_range);
    let (y, cb, cr) = decode_range(y, cb, cr, destination.range);
    let (red, green, blue) = ycbcr_to_rgb(y, cb, cr, destination.matrix);

    Ok(Color::new(
        color.alpha,
        to_u8(red),
        to_u8(green),
        to_u8(blue),
    ))
}

fn source_spec(
    source: YCbCrMatrix,
) -> Result<Option<(VideoMatrix, VideoRange)>, ColorConversionError> {
    let spec = match source {
        YCbCrMatrix::Default | YCbCrMatrix::TV601 => {
            Some((VideoMatrix::Bt601, VideoRange::Limited))
        }
        YCbCrMatrix::PC601 => Some((VideoMatrix::Bt601, VideoRange::Full)),
        YCbCrMatrix::TV709 => Some((VideoMatrix::Bt709, VideoRange::Limited)),
        YCbCrMatrix::PC709 => Some((VideoMatrix::Bt709, VideoRange::Full)),
        YCbCrMatrix::TV240M => Some((VideoMatrix::Smpte240M, VideoRange::Limited)),
        YCbCrMatrix::PC240M => Some((VideoMatrix::Smpte240M, VideoRange::Full)),
        YCbCrMatrix::TVFCC => Some((VideoMatrix::Fcc, VideoRange::Limited)),
        YCbCrMatrix::PCFCC => Some((VideoMatrix::Fcc, VideoRange::Full)),
        YCbCrMatrix::None => None,
        YCbCrMatrix::Unknown => return Err(ColorConversionError::UnknownMatrix),
    };
    Ok(spec)
}

fn coefficients(matrix: VideoMatrix) -> (f64, f64, f64) {
    match matrix {
        VideoMatrix::Bt601 => (0.299, 0.114, 0.587),
        VideoMatrix::Bt709 => (0.2126, 0.0722, 0.7152),
        VideoMatrix::Smpte240M => (0.212, 0.087, 0.701),
        VideoMatrix::Fcc => (0.30, 0.11, 0.59),
    }
}

fn rgb_to_ycbcr(color: Color, matrix: VideoMatrix) -> (f64, f64, f64) {
    let red = f64::from(color.red) / 255.0;
    let green = f64::from(color.green) / 255.0;
    let blue = f64::from(color.blue) / 255.0;
    let (kr, kb, kg) = coefficients(matrix);
    let y = kr * red + kg * green + kb * blue;
    let cb = (blue - y) / (2.0 * (1.0 - kb)) + 0.5;
    let cr = (red - y) / (2.0 * (1.0 - kr)) + 0.5;
    (y, cb, cr)
}

fn encode_range(y: f64, cb: f64, cr: f64, range: VideoRange) -> (f64, f64, f64) {
    match range {
        VideoRange::Limited => (
            (16.0 + 219.0 * y) / 255.0,
            (128.0 + 224.0 * (cb - 0.5)) / 255.0,
            (128.0 + 224.0 * (cr - 0.5)) / 255.0,
        ),
        VideoRange::Full => (y, cb, cr),
    }
}

fn decode_range(y: f64, cb: f64, cr: f64, range: VideoRange) -> (f64, f64, f64) {
    match range {
        VideoRange::Limited => (
            (y * 255.0 - 16.0) / 219.0,
            (cb * 255.0 - 128.0) / 224.0 + 0.5,
            (cr * 255.0 - 128.0) / 224.0 + 0.5,
        ),
        VideoRange::Full => (
            y,
            (cb * 255.0 - 128.0) / 255.0 + 0.5,
            (cr * 255.0 - 128.0) / 255.0 + 0.5,
        ),
    }
}

fn ycbcr_to_rgb(y: f64, cb: f64, cr: f64, matrix: VideoMatrix) -> (f64, f64, f64) {
    let (kr, kb, kg) = coefficients(matrix);
    let red = y + 2.0 * (1.0 - kr) * (cr - 0.5);
    let blue = y + 2.0 * (1.0 - kb) * (cb - 0.5);
    let green = (y - kr * red - kb * blue) / kg;
    (red, green, blue)
}

fn to_u8(value: f64) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_preserves_authored_rgb_and_alpha() {
        let color = Color::new(37, 240, 128, 32);
        assert_eq!(
            convert_ass_rgb(color, YCbCrMatrix::None, VideoColorSpace::BT709_LIMITED),
            Ok(color)
        );
    }

    #[test]
    fn explicit_conversion_requires_destination_and_changes_tv_range() {
        let color = Color::new(0, 240, 128, 32);
        let converted =
            convert_ass_rgb(color, YCbCrMatrix::TV601, VideoColorSpace::BT709_LIMITED).unwrap();
        assert_eq!(converted.alpha, color.alpha);
        assert_ne!(converted.to_ass_components(), color.to_ass_components());
    }

    #[test]
    fn unknown_matrix_is_not_guessed() {
        assert_eq!(
            convert_ass_rgb(
                Color::white(),
                YCbCrMatrix::Unknown,
                VideoColorSpace::BT601_LIMITED,
            ),
            Err(ColorConversionError::UnknownMatrix)
        );
    }

    #[test]
    fn matching_full_range_round_trip_is_stable() {
        let color = Color::new(11, 12, 123, 231);
        let converted =
            convert_ass_rgb(color, YCbCrMatrix::PC601, VideoColorSpace::BT601_FULL).unwrap();
        assert!((i16::from(converted.red) - i16::from(color.red)).abs() <= 1);
        assert!((i16::from(converted.green) - i16::from(color.green)).abs() <= 1);
        assert!((i16::from(converted.blue) - i16::from(color.blue)).abs() <= 1);
    }
}
