use super::{ass_bold_weight, parse_libass_f64, parse_libass_i32, OverrideTag};

/// Parse font and text-formatting tags. The caller handles bare-property
/// resets before dispatching here, so a malformed numeric value can retain
/// libass's distinction between a reset and an ignored tag.
pub(super) fn parse(name: &str, params: Option<&str>) -> Option<OverrideTag> {
    match name {
        "b" => {
            let val = parse_libass_i32(params?);
            if !(val == 0 || val == 1 || val >= 100) {
                return Some(OverrideTag::PropertyReset(name.to_string()));
            }
            Some(OverrideTag::Bold(ass_bold_weight(val)))
        }
        "i" => {
            let val = parse_libass_i32(params?);
            if !(0..=1).contains(&val) {
                return Some(OverrideTag::PropertyReset(name.to_string()));
            }
            Some(OverrideTag::Italic(val != 0))
        }
        "u" => {
            let val = parse_libass_i32(params?);
            if !(0..=1).contains(&val) {
                return Some(OverrideTag::PropertyReset(name.to_string()));
            }
            Some(OverrideTag::Underline(val != 0))
        }
        "s" => {
            let val = parse_libass_i32(params?);
            if !(0..=1).contains(&val) {
                return Some(OverrideTag::PropertyReset(name.to_string()));
            }
            Some(OverrideTag::StrikeOut(val != 0))
        }
        "fn" => Some(OverrideTag::FontName(params?.trim().to_string())),
        "fs" => {
            // A leading sign makes the size relative to the current size.
            // The bare form is handled as a distinct style-size reset.
            let raw = params.unwrap_or("").trim();
            if raw.is_empty() {
                return Some(OverrideTag::FontSizeReset);
            }
            if !raw.chars().any(|c| c.is_ascii_digit()) {
                return None;
            }
            let val = parse_libass_f64(raw);
            if raw.starts_with('+') || raw.starts_with('-') {
                Some(OverrideTag::FontSizeRelative(val))
            } else {
                Some(OverrideTag::FontSize(val))
            }
        }
        "fe" => {
            if !params?.chars().any(|c| c.is_ascii_digit()) {
                return None;
            }
            Some(OverrideTag::FontEncoding(parse_libass_i32(params?)))
        }
        "fsp" => Some(OverrideTag::LetterSpacing(parse_libass_f64(
            params.unwrap_or(""),
        ))),
        _ => None,
    }
}
