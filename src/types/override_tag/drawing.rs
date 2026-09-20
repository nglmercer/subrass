/// Check whether a vector clip contains at least one supported ASS drawing
/// command. Keeping this lexer predicate separate prevents clip parsing from
/// growing a second drawing grammar.
pub(super) fn drawing_contains_command(drawing: &str) -> bool {
    drawing.chars().any(|c| {
        matches!(
            c,
            'm' | 'n' | 'l' | 'b' | 's' | 'p' | 'c' | 'M' | 'N' | 'L' | 'B' | 'S' | 'P' | 'C'
        )
    })
}

use super::{parse_libass_f64, parse_libass_i32, OverrideTag};

pub(super) fn parse(name: &str, params: Option<&str>) -> Option<OverrideTag> {
    match name {
        "p" => Some(OverrideTag::Drawing(
            parse_libass_i32(params.unwrap_or("")).max(0),
        )),
        "pbo" => Some(OverrideTag::DrawingBaseline(parse_libass_f64(
            params.unwrap_or(""),
        ))),
        _ => None,
    }
}
