/// Parse a double the libass way (`ass_strtod` value semantics, as in
/// `argtod`): skip whitespace, one optional sign, longest digit run
/// with at most one `.`, optional `e`/`E` exponent. There are no
/// `inf`/`NaN` literals (those parse as 0.0) and no hex floats; no
/// digits at all means 0.0. Never fails: even garbage yields a value,
/// so malformed numbers never ignore the whole tag.
pub(super) fn parse_libass_f64(s: &str) -> f64 {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r') {
        i += 1;
    }
    let num_start = i;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let mut digits = 0u32;
    let mut dot = false;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            digits += 1;
            i += 1;
        } else if b[i] == b'.' && !dot {
            dot = true;
            i += 1;
        } else {
            break;
        }
    }
    if digits == 0 {
        return 0.0;
    }
    let mut end = i;
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let exp_start = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > exp_start {
            end = j;
        }
    }
    s[num_start..end].parse().unwrap_or(0.0)
}

/// libass `dtoi32`: double to `i32` without UB — NaN and anything
/// outside `i32` maps to `INT32_MIN` (x86 `cvttsd2si` behavior),
/// otherwise truncate toward zero.
pub(super) fn libass_dtoi32(v: f64) -> i32 {
    if v.is_nan() || v <= -2147483648.0 || v >= 2147483648.0 {
        i32::MIN
    } else {
        v as i32
    }
}

/// Finite-coordinate clamp for prefix-parsed positions: `ass_strtod`
/// overflow yields inf, which would poison layout math. libass renders
/// such coordinates off-screen, so clamp to a huge-but-finite value —
/// slot consumption and off-screen rendering are preserved.
pub(super) fn sanitize_coord(v: f64) -> f64 {
    if v.is_nan() {
        0.0
    } else if v.is_infinite() {
        if v.is_sign_negative() {
            -1e18
        } else {
            1e18
        }
    } else {
        v
    }
}

/// Parse an integer the libass way (`strtoll` base 10 clamped to `i32`,
/// as in `mystrtoi32`): ASCII whitespace skipped, one optional sign,
/// longest digit prefix wins (`7x` parses as 7), no digits means 0,
/// overflow clamps. Never fails: even garbage yields a value, which is
/// exactly why malformed `\an`/`\a` still consume the alignment slot.
pub(super) fn parse_libass_i32(s: &str) -> i32 {
    let t = s.trim_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']);
    let (t, neg) = match t.as_bytes().first() {
        Some(b'+') => (&t[1..], false),
        Some(b'-') => (&t[1..], true),
        _ => (t, false),
    };
    let digits_len = t.bytes().take_while(u8::is_ascii_digit).count();
    if digits_len == 0 {
        return 0;
    }
    let magnitude: i64 = t[..digits_len].parse().unwrap_or(i64::MAX);
    let value = if neg {
        magnitude.saturating_neg()
    } else {
        magnitude
    };
    value.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}
