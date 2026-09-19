use crate::renderer::buffer::RenderBuffer;

/// Scale the alpha channel of every pixel in a row range by `mult` (0..=1).
fn scale_alpha_rows(buffer: &mut RenderBuffer, y0: i64, y1: i64, mult: f64) {
    if !(0.0..=1.0).contains(&mult) || buffer.width == 0 || buffer.height == 0 {
        return;
    }
    let w = buffer.width as i64;
    let h = buffer.height as i64;
    for y in y0.max(0)..=y1.min(h - 1) {
        let row = (y * w * 4) as usize;
        for x in 0..w {
            let idx = row + (x * 4) as usize + 3;
            if let Some(a) = buffer.pixels.get_mut(idx) {
                *a = (f64::from(*a) * mult).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// Scale the alpha channel of every pixel in a column range by `mult` (0..=1).
fn scale_alpha_cols(buffer: &mut RenderBuffer, x0: i64, x1: i64, mult: f64) {
    if !(0.0..=1.0).contains(&mult) || buffer.width == 0 || buffer.height == 0 {
        return;
    }
    let w = buffer.width as i64;
    let h = buffer.height as i64;
    for y in 0..h {
        let row = (y * w * 4) as usize;
        for x in x0.max(0)..=x1.min(w - 1) {
            let idx = row + (x * 4) as usize + 3;
            if let Some(a) = buffer.pixels.get_mut(idx) {
                *a = (f64::from(*a) * mult).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// Banner `fadeawaywidth`: linear alpha ramps at the left and right
/// frame edges over `fade_px` output pixels each. Multipliers follow
/// VSFilter exactly: left column `i` scales by `i / fade` (edge column
/// fully transparent), right column `j` by `(w - j) / fade`.
/// Overlapping ramps (fade wider than the frame) multiply, as in
/// VSFilter. The loop runs over frame columns, so hostile fade widths
/// stay O(frame). Non-positive widths are a no-op.
pub fn apply_fadeaway_x(buffer: &mut RenderBuffer, fade_px: f64) {
    if !fade_px.is_finite() || fade_px <= 0.0 || buffer.width == 0 {
        return;
    }
    let w = buffer.width as i64;
    let reach = fade_px.ceil().min(w as f64) as i64;
    for i in 0..reach {
        scale_alpha_cols(buffer, i, i, i as f64 / fade_px);
        scale_alpha_cols(buffer, w - 1 - i, w - 1 - i, (i as f64 + 1.0) / fade_px);
    }
}

/// Scroll `fadeawayheight`: linear alpha ramps at the band's top edge
/// (row `top + i` scales by `i / fade`) and bottom edge (row
/// `bottom - 1 - i` scales by `(i + 1) / fade`) over `fade_px` output
/// pixels each. `bottom` is exclusive, matching VSFilter's band clip.
/// The loop runs over affected rows only, so hostile fade heights stay
/// O(frame). Non-positive heights are a no-op.
pub fn apply_fadeaway_y(buffer: &mut RenderBuffer, top: i64, bottom: i64, fade_px: f64) {
    if !fade_px.is_finite() || fade_px <= 0.0 || buffer.height == 0 {
        return;
    }
    let h = buffer.height as i64;
    let reach = fade_px.ceil().min(h as f64) as i64;
    for i in 0..reach {
        scale_alpha_rows(buffer, top + i, top + i, i as f64 / fade_px);
        scale_alpha_rows(
            buffer,
            bottom - 1 - i,
            bottom - 1 - i,
            (i as f64 + 1.0) / fade_px,
        );
    }
}

/// Piecewise-linear fade value (libass `interpolate_alpha`): ASS
/// transparency where 0 = visible and 255 = transparent. Ramp math
/// runs in f64 and truncates toward zero like the C `(int)` cast
/// (Rust `as i32` truncates identically and saturates instead of
/// overflowing). Runs in `i64` so hostile times cannot wrap; for
/// realistic event lengths this equals the libass `i32` computation
/// exactly. The lerp branches only run with a proven-positive
/// denominator, so this cannot divide by zero.
#[allow(clippy::too_many_arguments)]
pub(crate) fn interpolate_alpha(
    now: i64,
    t1: i64,
    t2: i64,
    t3: i64,
    t4: i64,
    a1: i32,
    a2: i32,
    a3: i32,
) -> i32 {
    if now < t1 {
        a1
    } else if now < t2 {
        let cf = (now - t1) as f64 / (t2 - t1) as f64;
        (f64::from(a1) * (1.0 - cf) + f64::from(a2) * cf) as i32
    } else if now < t3 {
        a2
    } else if now < t4 {
        let cf = (now - t3) as f64 / (t4 - t3) as f64;
        (f64::from(a2) * (1.0 - cf) + f64::from(a3) * cf) as i32
    } else {
        a3
    }
}

/// Calculate fade alpha based on time.
///
/// Returns an opacity value: 0 = fully transparent, 255 = fully opaque.
/// This is libass's 2-argument fade conversion evaluated through
/// [`interpolate_alpha`] (`t1 = 0`, `t2 = fade_in`, `t4 = Duration`,
/// `t3 = Duration - fade_out`), so overlapping fades follow the
/// shared piecewise ramp — not the minimum of two independent ramps.
/// Non-positive times mean "no fade" on that side, matching libass.
/// A computed fade `<= 0` leaves the frame fully opaque (libass
/// `ass_apply_fade` only applies positive fades); above 255 clamps to
/// transparent (libass wraps mod 256 there, a C-cast artifact).
pub fn calculate_fade_alpha(
    time_ms: u64,
    start_ms: u64,
    end_ms: u64,
    fade_in_ms: i32,
    fade_out_ms: i32,
) -> u8 {
    if time_ms < start_ms || time_ms >= end_ms {
        return 0;
    }

    let duration = end_ms - start_ms;
    let elapsed = time_ms - start_ms;
    let duration = i64::try_from(duration).unwrap_or(i64::MAX);
    let elapsed = i64::try_from(elapsed).unwrap_or(i64::MAX);
    let a = interpolate_alpha(
        elapsed,
        0,
        i64::from(fade_in_ms),
        duration - i64::from(fade_out_ms),
        duration,
        255,
        0,
        255,
    );
    if a <= 0 {
        255
    } else {
        (255 - a.min(255)) as u8
    }
}
