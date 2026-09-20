use super::OverrideTag;

/// Tags whose bare form is a style-property reset in libass.
pub(super) fn is_property_reset_name(name: &str) -> bool {
    matches!(
        name,
        "b" | "i"
            | "u"
            | "s"
            | "fn"
            | "fe"
            | "fsp"
            | "fr"
            | "frx"
            | "fry"
            | "frz"
            | "fscx"
            | "fscy"
            | "fax"
            | "fay"
            | "bord"
            | "xbord"
            | "ybord"
            | "shad"
            | "xshad"
            | "yshad"
            | "be"
            | "blur"
    )
}

pub(super) fn parse(name: &str, params: Option<&str>) -> Option<OverrideTag> {
    if name != "r" {
        return None;
    }
    let style = params
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    Some(OverrideTag::Reset(style))
}
