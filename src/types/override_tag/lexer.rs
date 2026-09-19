/// libass `complex_tag` keywords: parenthesized tags whose name match
/// is a PREFIX match and whose arguments come only from the parens.
pub(super) fn is_complex_tag_name(name: &str) -> bool {
    matches!(
        name,
        "pos" | "move" | "org" | "fad" | "fade" | "t" | "clip" | "iclip"
    )
}

/// Prefix keyword for a parenthesized tag name (`\position(1,2)` ->
/// `"pos"`). `fad` covers `fade` (both dispatch to the shared fade
/// arm); every other keyword maps to itself.
pub(super) fn complex_tag_keyword(name: &str) -> Option<&'static str> {
    ["pos", "move", "org", "fad", "t", "clip", "iclip"]
        .into_iter()
        .find(|keyword| name.starts_with(keyword))
}

/// Whether `name` is a recognized override tag name.
pub(super) fn is_known_tag_name(name: &str) -> bool {
    matches!(
        name,
        "b" | "i"
            | "u"
            | "s"
            | "fn"
            | "fs"
            | "fsp"
            | "r"
            | "c"
            | "1c"
            | "2c"
            | "3c"
            | "4c"
            | "alpha"
            | "1a"
            | "2a"
            | "3a"
            | "4a"
            | "pos"
            | "move"
            | "org"
            | "an"
            | "a"
            | "frx"
            | "fry"
            | "frz"
            | "fr"
            | "fscx"
            | "fscy"
            | "fsc"
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
            | "fad"
            | "fade"
            | "t"
            | "clip"
            | "iclip"
            | "p"
            | "pbo"
            | "q"
            | "k"
            | "K"
            | "kf"
            | "ko"
            | "kt"
            | "fe"
    )
}

/// Split a greedily-read run into (known tag name, glued value).
/// Exact and unknown names pass through; otherwise the longest known
/// prefix wins (`rAltStyle` -> `r` + `AltStyle`).
pub(super) fn split_tag_name(raw: &str) -> (&str, &str) {
    if is_known_tag_name(raw) {
        return (raw, "");
    }
    for len in (1..raw.len()).rev() {
        if !raw.is_char_boundary(len) {
            continue;
        }
        let (head, tail) = raw.split_at(len);
        if is_known_tag_name(head) {
            // Numeric tags only accept a prefix when the glued value starts
            // with a sign or digit. This keeps `\frobnicator` and
            // `\samebogus` unknown while accepting libass forms such as
            // `\frz30xyz` and `\b1foo`.
            if matches!(
                head,
                "b" | "i"
                    | "u"
                    | "s"
                    | "fs"
                    | "fsp"
                    | "fe"
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
                    | "p"
                    | "pbo"
                    | "q"
                    | "k"
                    | "K"
                    | "kf"
                    | "ko"
                    | "kt"
            ) && !tail
                .trim_start_matches([' ', '\t', '+', '-'])
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())
            {
                continue;
            }
            return (head, tail);
        }
    }
    (raw, "")
}

/// Split comma-separated tag params the libass way: trim spaces/tabs
/// around each argument and drop segments left empty.
pub(super) fn split_tag_args(params: &str) -> Vec<&str> {
    params
        .split(',')
        .map(|a| a.trim_matches([' ', '\t']))
        .filter(|a| !a.is_empty())
        .collect()
}
