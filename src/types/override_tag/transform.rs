use super::{libass_dtoi32, parse_libass_f64, parse_libass_i32, OverrideTag, MAX_TAG_NESTING};

/// Parse geometric transform tags and the nested `\t` animation.
pub(super) fn parse(name: &str, params: Option<&str>, depth: u32) -> Option<OverrideTag> {
    match name {
        "frx" => Some(OverrideTag::RotationX(parse_libass_f64(
            params.unwrap_or(""),
        ))),
        "fry" => Some(OverrideTag::RotationY(parse_libass_f64(
            params.unwrap_or(""),
        ))),
        "frz" | "fr" => Some(OverrideTag::RotationZ(parse_libass_f64(
            params.unwrap_or(""),
        ))),
        "fscx" => Some(OverrideTag::ScaleX(parse_libass_f64(params.unwrap_or("")))),
        "fscy" => Some(OverrideTag::ScaleY(parse_libass_f64(params.unwrap_or("")))),
        "fsc" => Some(OverrideTag::ScaleReset),
        "fax" => Some(OverrideTag::ShearX(parse_libass_f64(params.unwrap_or("")))),
        "fay" => Some(OverrideTag::ShearY(parse_libass_f64(params.unwrap_or("")))),
        "t" => parse_transform(params?, depth),
        _ => None,
    }
}

fn parse_transform(params: &str, depth: u32) -> Option<OverrideTag> {
    if depth >= MAX_TAG_NESTING {
        return None;
    }
    let tags_pos = params.find('\\').unwrap_or(params.len());
    let (args_part, tags_str) = params.split_at(tags_pos);
    if !tags_str.contains('\\') {
        return None;
    }
    let mut args: Vec<&str> = args_part
        .split(',')
        .map(|s| s.trim_matches([' ', '\t']))
        .filter(|s| !s.is_empty())
        .collect();
    if !args_part.trim_matches([' ', '\t']).ends_with(',') {
        args.pop();
    }

    let (t1, t2, accel) = match args.len() {
        0 => (0, 0, 1.0),
        1 => (0, 0, parse_libass_f64(args[0])),
        2 => (
            libass_dtoi32(parse_libass_f64(args[0])),
            libass_dtoi32(parse_libass_f64(args[1])),
            1.0,
        ),
        3 => (
            parse_libass_i32(args[0]),
            parse_libass_i32(args[1]),
            parse_libass_f64(args[2]),
        ),
        _ => return None,
    };

    Some(OverrideTag::Transform {
        t1,
        t2,
        accel,
        tags: super::parse_tag_group_depth(tags_str, depth + 1),
    })
}
