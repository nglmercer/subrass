use super::{libass_dtoi32, parse_libass_f64, OverrideTag};

/// Parse a karaoke duration/start parameter using libass centisecond and
/// signed-millisecond rules. Fractional centiseconds remain available to
/// the timing model instead of being rounded during lexing.
pub(super) fn parse_karaoke_param(params: Option<&str>, mode: u8) -> Option<OverrideTag> {
    let value = params
        .map(parse_libass_f64)
        .unwrap_or(if mode == 3 { 0.0 } else { 100.0 });
    if !value.is_finite() {
        return None;
    }
    let millis = libass_dtoi32(value * 10.0);
    if millis >= 0 && millis % 10 == 0 {
        let cs = (millis / 10) as u64;
        match mode {
            0 => OverrideTag::KaraokeDuration(cs),
            1 => OverrideTag::KaraokeSweep(cs),
            2 => OverrideTag::KaraokeOutline(cs),
            _ => OverrideTag::KaraokeStart(cs),
        }
    } else {
        OverrideTag::KaraokeTiming { mode, millis }
    }
    .into()
}
