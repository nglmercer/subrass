//! ASS transform order shared by text coverage and vector drawings.
use super::super::ResolvedStyle;
use crate::renderer::buffer::effective_shear;
use crate::utils::Matrix3x3;

pub(super) fn rotation(style: &ResolvedStyle) -> Matrix3x3 {
    Matrix3x3::rotation_y((-style.rotation_y).to_radians())
        .multiply(&Matrix3x3::rotation_x((-style.rotation_x).to_radians()))
        .multiply(&Matrix3x3::rotation_z((-style.angle).to_radians()))
}

pub(super) fn shear(style: &ResolvedStyle, sx: f64, sy: f64) -> Option<(f64, f64)> {
    let (fax, fay) = effective_shear((style.shear_x, style.shear_y))?;
    if sx.abs() > 1e-9 && sy.abs() > 1e-9 {
        Some((fax * sx / sy, fay * sy / sx))
    } else {
        Some((fax, fay))
    }
}

pub(super) fn perspective(video_height: u32, play_res_y: u32) -> f64 {
    312.5 * f64::from(video_height) / f64::from(play_res_y.max(1))
}
