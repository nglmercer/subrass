//! Combining-mark and CJK line-break policy used by wrapping and fallback.

/// True for combining marks (Mn/Mc) of the common scripts — Latin,
/// Greek, Cyrillic, Hebrew, Arabic, Syriac, Thaana, Devanagari, Thai,
/// Lao, Tibetan, CJK kana voicing marks, and the generic blocks.
/// Marks attach to the preceding base for cluster-aware fallback and
/// never start a wrapped line. Glue only (mark reordering itself
/// needs a real shaper); remaining Indic scripts and historic marks
/// stay per-character until UCD tables land.
pub fn is_combining_mark(ch: char) -> bool {
    matches!(
        ch as u32,
        0x0300..=0x036F
            | 0x0483..=0x0489
            | 0x0591..=0x05BD
            | 0x05BF
            | 0x05C1..=0x05C2
            | 0x05C4..=0x05C5
            | 0x05C7
            | 0x0610..=0x061A
            | 0x064B..=0x065F
            | 0x0670
            | 0x06D6..=0x06DC
            | 0x06DF..=0x06E4
            | 0x06E7..=0x06E8
            | 0x06EA..=0x06ED
            | 0x0711
            | 0x0730..=0x074A
            | 0x07A6..=0x07B0
            | 0x0900..=0x0903
            | 0x093A..=0x093C
            | 0x093E..=0x094D
            | 0x094E..=0x094F
            | 0x0951..=0x0957
            | 0x0962..=0x0963
            | 0x0E31
            | 0x0E34..=0x0E3A
            | 0x0E47..=0x0E4E
            | 0x0EB1
            | 0x0EB4..=0x0EBC
            | 0x0EC8..=0x0ECD
            | 0x0F71..=0x0F84
            | 0x0F86..=0x0F87
            | 0x1AB0..=0x1AFF
            | 0x1DC0..=0x1DFF
            | 0x20D0..=0x20FF
            | 0xFE20..=0xFE2F
            | 0x3099..=0x309A
    )
}

/// True for wide CJK characters that allow line breaks around them
/// (conservative UAX #14 approximation for wrapping only): unified and
/// compatibility ideographs, hiragana/katakana, Hangul syllables, and
/// wide symbols, plus U+3000 (ideographic space: UAX #14 ID class
/// breaks around it like other CJK). Excludes conjoining jamo/bopomofo
/// (kept glued), halfwidth forms (narrow), and ASCII-mirroring
/// fullwidth alphanumerics (unbreakable like their ASCII halves).
pub fn is_cjk_breakable(ch: char) -> bool {
    matches!(
        ch as u32,
        0x2E80..=0x2FD5
            | 0x3000..=0x303F
            | 0x3040..=0x30FF
            | 0x31F0..=0x31FF
            | 0x3200..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE44
            | 0xFF01..=0xFF0F
            | 0xFF1A..=0xFF20
            | 0xFF3B..=0xFF40
            | 0xFF5B..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x20000..=0x2FFFF
            | 0x30000..=0x3FFFF
    )
}

/// Opening brackets: no break after (they stick to what follows).
pub fn is_cjk_open(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3008
            | 0x300A
            | 0x300C
            | 0x300E
            | 0x3010
            | 0x3014
            | 0x3016
            | 0x3018
            | 0x301A
            | 0x301D
            | 0xFF08
            | 0xFF3B
            | 0xFF5B
            | 0xFF5F
            | 0xFF62
    )
}

/// Closing punctuation, non-starters (small kana, iteration and sound
/// marks), and fullwidth mirrors of ASCII `!?,;:«»…`: no break before
/// (they stick to what precedes).
pub fn is_cjk_nobreak_before(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3001..=0x3003
            | 0x3005
            | 0x3009
            | 0x300B
            | 0x300D
            | 0x300F
            | 0x3011
            | 0x3015
            | 0x3017
            | 0x3019
            | 0x301B..=0x301C
            | 0x301E..=0x301F
            | 0x3030..=0x3035
            | 0x3041
            | 0x3043
            | 0x3045
            | 0x3047
            | 0x3049
            | 0x3063
            | 0x3083
            | 0x3085
            | 0x3087
            | 0x308E
            | 0x3095..=0x3096
            | 0x309D..=0x309E
            | 0x30A1
            | 0x30A3
            | 0x30A5
            | 0x30A7
            | 0x30A9
            | 0x30C3
            | 0x30E3
            | 0x30E5
            | 0x30E7
            | 0x30EE
            | 0x30F5..=0x30F6
            | 0x30FB..=0x30FE
            | 0x31F0..=0x31FF
            | 0xFE30..=0xFE36
            | 0xFF01
            | 0xFF0C
            | 0xFF0E
            | 0xFF1A..=0xFF1B
            | 0xFF1F
            | 0xFF60..=0xFF61
            | 0xFF63..=0xFF65
            | 0xFFE6
    )
}

/// Break opportunity between two adjacent in-word characters for the
/// wrapper: breaks around CJK wide chars (subject to open/close glues),
/// never inside combining sequences, around joiners, or between a
/// currency sign and its digits — and never between two non-CJK chars
/// (spaces own those). U+200B ZERO WIDTH SPACE breaks on both sides
/// (UAX #14 ZW); default libass builds (no unibreak) never break
/// there, so this is a documented multilingual divergence like CJK.
pub fn cjk_break_between(prev: char, next: char) -> bool {
    if prev == '\u{200C}'
        || next == '\u{200C}'
        || prev == '\u{200D}'
        || next == '\u{200D}'
        || prev == '\u{00A0}'
        || next == '\u{00A0}'
    {
        return false;
    }
    if is_combining_mark(next) {
        return false;
    }
    if prev == '\u{200B}' || next == '\u{200B}' {
        return true;
    }
    if matches!(
        prev as u32,
        0x0024 | 0x00A2 | 0x00A3 | 0x00A5 | 0xFFE0 | 0xFFE1 | 0xFFE5
    ) && matches!(next as u32, 0x0030..=0x0039 | 0xFF10..=0xFF19)
    {
        return false;
    }
    if !is_cjk_breakable(prev) && !is_cjk_breakable(next) {
        return false;
    }
    if is_cjk_open(prev) || is_cjk_nobreak_before(next) {
        return false;
    }
    true
}
