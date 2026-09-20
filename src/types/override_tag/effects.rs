use super::{parse_libass_f64, parse_libass_i32, split_tag_args, OverrideTag};

/// Parse borders, shadows, blur, and fading tags.
pub(super) fn parse(name: &str, params: Option<&str>) -> Option<OverrideTag> {
    match name {
        "bord" => Some(OverrideTag::Border(parse_libass_f64(params.unwrap_or("")))),
        "xbord" => Some(OverrideTag::BorderX(parse_libass_f64(params.unwrap_or("")))),
        "ybord" => Some(OverrideTag::BorderY(parse_libass_f64(params.unwrap_or("")))),
        "shad" => Some(OverrideTag::Shadow(parse_libass_f64(params.unwrap_or("")))),
        "xshad" => Some(OverrideTag::ShadowX(parse_libass_f64(params.unwrap_or("")))),
        "yshad" => Some(OverrideTag::ShadowY(parse_libass_f64(params.unwrap_or("")))),
        "be" => Some(OverrideTag::EdgeBlur(parse_libass_f64(
            params.unwrap_or(""),
        ))),
        "blur" => Some(OverrideTag::Blur(parse_libass_f64(params.unwrap_or("")))),
        "fad" | "fade" => {
            let parts = split_tag_args(params?);
            match parts.len() {
                2 => Some(OverrideTag::Fade(
                    parse_libass_i32(parts[0]),
                    parse_libass_i32(parts[1]),
                )),
                7 => Some(OverrideTag::ComplexFade(
                    parse_libass_i32(parts[0]),
                    parse_libass_i32(parts[1]),
                    parse_libass_i32(parts[2]),
                    parse_libass_i32(parts[3]),
                    parse_libass_i32(parts[4]),
                    parse_libass_i32(parts[5]),
                    parse_libass_i32(parts[6]),
                )),
                _ => None,
            }
        }
        _ => None,
    }
}
