use super::{parse_libass_f64, parse_libass_i32, sanitize_coord, split_tag_args, OverrideTag};

/// Parse event-position and alignment tags.
pub(super) fn parse(name: &str, params: Option<&str>) -> Option<OverrideTag> {
    match name {
        "pos" => {
            let parts = split_tag_args(params?);
            if parts.len() == 2 {
                Some(OverrideTag::Position(
                    sanitize_coord(parse_libass_f64(parts[0])),
                    sanitize_coord(parse_libass_f64(parts[1])),
                ))
            } else {
                None
            }
        }
        "move" => {
            let parts = split_tag_args(params?);
            match parts.len() {
                4 => Some(OverrideTag::Move(
                    sanitize_coord(parse_libass_f64(parts[0])),
                    sanitize_coord(parse_libass_f64(parts[1])),
                    sanitize_coord(parse_libass_f64(parts[2])),
                    sanitize_coord(parse_libass_f64(parts[3])),
                )),
                6 => {
                    let t1 = parse_libass_i32(parts[4]);
                    let t2 = parse_libass_i32(parts[5]);
                    let (t1, t2) = if t1 > t2 { (t2, t1) } else { (t1, t2) };
                    Some(OverrideTag::MoveWithTiming(
                        sanitize_coord(parse_libass_f64(parts[0])),
                        sanitize_coord(parse_libass_f64(parts[1])),
                        sanitize_coord(parse_libass_f64(parts[2])),
                        sanitize_coord(parse_libass_f64(parts[3])),
                        t1,
                        t2,
                    ))
                }
                _ => None,
            }
        }
        "org" => {
            let parts = split_tag_args(params?);
            if parts.len() == 2 {
                Some(OverrideTag::Origin(
                    sanitize_coord(parse_libass_f64(parts[0])),
                    sanitize_coord(parse_libass_f64(parts[1])),
                ))
            } else {
                None
            }
        }
        "an" => {
            let raw = params.unwrap_or("");
            if raw.trim().is_empty() {
                return Some(OverrideTag::AlignmentReset);
            }
            let val = parse_libass_i32(raw);
            if !(1..=9).contains(&val) {
                return Some(OverrideTag::AlignmentReset);
            }
            Some(OverrideTag::Alignment(val))
        }
        "a" => {
            let raw = params.unwrap_or("");
            if raw.trim().is_empty() {
                return Some(OverrideTag::AlignmentReset);
            }
            let val = parse_libass_i32(raw);
            if !(1..=11).contains(&val) {
                return Some(OverrideTag::AlignmentReset);
            }
            let val = if val == 4 || val == 8 { 5 } else { val };
            Some(OverrideTag::Alignment(
                crate::types::style::ssa_alignment_to_ass(val),
            ))
        }
        _ => None,
    }
}
