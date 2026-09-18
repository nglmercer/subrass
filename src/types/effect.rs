//! Legacy `Effect` field: `Banner` and `Scroll up`/`Scroll down`.
//!
//! Semantics follow VSFilter (`CRenderedTextSubtitle::ParseEffect` and the
//! `EF_BANNER`/`EF_SCROLL` render paths in MPC-HC's `RTS.cpp`), which the
//! SSA spec describes only loosely:
//!
//! - `Banner;delay[;lefttoright;fadeawaywidth]`: the line scrolls
//!   horizontally across the screen. Auto-wrap is disabled (wrap style 2).
//! - `Scroll up;y1;y2;delay[;fadeawayheight]` / `Scroll down;...`: the
//!   block scrolls vertically inside the `[y1, y2]` band (script pixels),
//!   clipped to the band.
//!
//! Timing: with `delay > 0`, scrolling one script pixel takes `delay`
//! milliseconds, i.e. speed is `1000/delay` script px per second.
//! Resolution scaling divides the effective delay (`delay / scale`), and
//! the effective delay is clamped to a minimum of 1 (`delay = 0` means
//! "as fast as possible", a finite 1000 px/s here instead of a
//! division by zero).

/// A parsed legacy scroll effect with raw script-unit parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LegacyEffect {
    /// `Banner;delay[;lefttoright;fadeawaywidth]`.
    Banner {
        /// Milliseconds per script pixel. Clamped to >= 0 at parse;
        /// the render-time effective delay is `(delay / scale).max(1)`.
        delay: f64,
        /// Scroll left-to-right when true, right-to-left otherwise.
        left_to_right: bool,
        /// Fade ramp width in script pixels (0 disables).
        fadeaway: f64,
    },
    /// `Scroll up;y1;y2;delay[;fadeawayheight]`.
    ScrollUp {
        /// Band top in script pixels.
        top: f64,
        /// Band bottom in script pixels (>= top after swap).
        bottom: f64,
        /// Milliseconds per script pixel (see [`LegacyEffect::Banner`]).
        delay: f64,
        /// Fade ramp height in script pixels (0 disables).
        fadeaway: f64,
    },
    /// `Scroll down;y1;y2;delay[;fadeawayheight]`.
    ScrollDown {
        /// Band top in script pixels.
        top: f64,
        /// Band bottom in script pixels (>= top after swap).
        bottom: f64,
        /// Milliseconds per script pixel (see [`LegacyEffect::Banner`]).
        delay: f64,
        /// Fade ramp height in script pixels (0 disables).
        fadeaway: f64,
    },
}

impl LegacyEffect {
    /// Parse an `Effect` field. Matching is case-insensitive on the
    /// effect name plus its semicolon (`Banner;`, `Scroll up;`,
    /// `Scroll down;`); anything else (including an empty field) is
    /// `None` and renders as a plain event.
    ///
    /// Field counts follow VSFilter: Banner needs at least `delay`,
    /// Scroll needs at least `y1;y2;delay`. Missing optional fields
    /// default to 0. Non-numeric or non-finite fields reject the effect.
    pub fn parse(field: &str) -> Option<Self> {
        let field = field.trim();
        let (name, params) = field.split_once(';')?;
        let mut parts = params.split(';');
        let num = |part: Option<&str>| -> Option<f64> {
            let v: f64 = part?.trim().parse().ok()?;
            v.is_finite().then_some(v)
        };
        if name.eq_ignore_ascii_case("Banner") {
            let delay = num(parts.next())?.max(0.0);
            let left_to_right = num(parts.next()).unwrap_or(0.0) != 0.0;
            let fadeaway = num(parts.next()).unwrap_or(0.0).max(0.0);
            Some(Self::Banner {
                delay,
                left_to_right,
                fadeaway,
            })
        } else if name.eq_ignore_ascii_case("Scroll up") || name.eq_ignore_ascii_case("Scroll down")
        {
            let down = name.eq_ignore_ascii_case("Scroll down");
            let (mut top, mut bottom) = (num(parts.next())?, num(parts.next())?);
            if top > bottom {
                std::mem::swap(&mut top, &mut bottom);
            }
            let delay = num(parts.next())?.max(0.0);
            let fadeaway = num(parts.next()).unwrap_or(0.0).max(0.0);
            if down {
                Some(Self::ScrollDown {
                    top,
                    bottom,
                    delay,
                    fadeaway,
                })
            } else {
                Some(Self::ScrollUp {
                    top,
                    bottom,
                    delay,
                    fadeaway,
                })
            }
        } else {
            None
        }
    }

    /// Milliseconds per *output* pixel along the scroll axis, given the
    /// resolution scale on that axis. Never below 1 and never NaN.
    pub fn effective_delay(delay: f64, scale: f64) -> f64 {
        if delay <= 0.0 || !scale.is_finite() || scale <= 0.0 {
            return 1.0;
        }
        (delay / scale).max(1.0)
    }

    /// Pixels travelled `elapsed_ms` into the event.
    pub fn traveled(elapsed_ms: u64, delay: f64, scale: f64) -> f64 {
        elapsed_ms as f64 / Self::effective_delay(delay, scale)
    }

    /// Left edge (output pixels) of a banner block at `elapsed_ms`.
    /// `screen_left`/`screen_right` are the video frame edges (0 and
    /// video width); `block_width` is the laid-out line width.
    pub fn banner_x(
        elapsed_ms: u64,
        delay: f64,
        scale: f64,
        left_to_right: bool,
        screen_left: f64,
        screen_right: f64,
        block_width: f64,
    ) -> f64 {
        let traveled = Self::traveled(elapsed_ms, delay, scale);
        if left_to_right {
            screen_left - block_width + traveled
        } else {
            screen_right - traveled
        }
    }

    /// Top edge (output pixels) of a scroll block at `elapsed_ms`.
    /// `top`/`bottom` are the band edges in output pixels.
    pub fn scroll_top(
        elapsed_ms: u64,
        delay: f64,
        scale: f64,
        down: bool,
        top: f64,
        bottom: f64,
        block_height: f64,
    ) -> f64 {
        let traveled = Self::traveled(elapsed_ms, delay, scale);
        if down {
            top + traveled - block_height
        } else {
            bottom - traveled
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_banner_forms() {
        assert_eq!(
            LegacyEffect::parse("Banner;20"),
            Some(LegacyEffect::Banner {
                delay: 20.0,
                left_to_right: false,
                fadeaway: 0.0,
            })
        );
        assert_eq!(
            LegacyEffect::parse("banner;20;1;10"),
            Some(LegacyEffect::Banner {
                delay: 20.0,
                left_to_right: true,
                fadeaway: 10.0,
            })
        );
        // Case-insensitive, whitespace-tolerant.
        assert!(LegacyEffect::parse("  BANNER; 20 ; 0 ; 5 ").is_some());
        // Missing delay rejects; garbage rejects.
        assert_eq!(LegacyEffect::parse("Banner;"), None);
        assert_eq!(LegacyEffect::parse("Banner;abc"), None);
        assert_eq!(LegacyEffect::parse("Banner"), None);
        assert_eq!(LegacyEffect::parse(""), None);
        assert_eq!(LegacyEffect::parse("Karaoke;10"), None);
        // Negative delay clamps to 0 (= fastest), negative fadeaway to 0.
        assert_eq!(
            LegacyEffect::parse("Banner;-5;2;-3"),
            Some(LegacyEffect::Banner {
                delay: 0.0,
                left_to_right: true,
                fadeaway: 0.0,
            })
        );
    }

    #[test]
    fn test_parse_scroll_forms() {
        assert_eq!(
            LegacyEffect::parse("Scroll up;10;100;20"),
            Some(LegacyEffect::ScrollUp {
                top: 10.0,
                bottom: 100.0,
                delay: 20.0,
                fadeaway: 0.0,
            })
        );
        assert_eq!(
            LegacyEffect::parse("scroll DOWN;10;100;20;8"),
            Some(LegacyEffect::ScrollDown {
                top: 10.0,
                bottom: 100.0,
                delay: 20.0,
                fadeaway: 8.0,
            })
        );
        // y1 > y2 swaps (VSFilter parity).
        assert_eq!(
            LegacyEffect::parse("Scroll up;100;10;20"),
            LegacyEffect::parse("Scroll up;10;100;20"),
        );
        // Fewer than 3 params rejects.
        assert_eq!(LegacyEffect::parse("Scroll up;10;100"), None);
        assert_eq!(LegacyEffect::parse("Scroll up;10;100;abc"), None);
    }

    #[test]
    fn test_effective_delay_never_zero() {
        assert_eq!(LegacyEffect::effective_delay(0.0, 1.0), 1.0);
        assert_eq!(LegacyEffect::effective_delay(-3.0, 1.0), 1.0);
        assert_eq!(LegacyEffect::effective_delay(20.0, 0.0), 1.0);
        assert_eq!(LegacyEffect::effective_delay(20.0, f64::NAN), 1.0);
        assert_eq!(LegacyEffect::effective_delay(20.0, 2.0), 10.0);
        // Sub-1ms effective delays clamp to 1 (VSFilter `max(..., 1)`).
        assert_eq!(LegacyEffect::effective_delay(1.0, 2.0), 1.0);
    }

    #[test]
    fn test_banner_position_math() {
        // Right-to-left: left edge starts at the right frame edge.
        assert_eq!(
            LegacyEffect::banner_x(0, 20.0, 1.0, false, 0.0, 640.0, 100.0),
            640.0
        );
        assert_eq!(
            LegacyEffect::banner_x(200, 20.0, 1.0, false, 0.0, 640.0, 100.0),
            630.0
        );
        // Left-to-right: right edge starts at the left frame edge.
        assert_eq!(
            LegacyEffect::banner_x(0, 20.0, 1.0, true, 0.0, 640.0, 100.0),
            -100.0
        );
        assert_eq!(
            LegacyEffect::banner_x(200, 20.0, 1.0, true, 0.0, 640.0, 100.0),
            -90.0
        );
    }

    #[test]
    fn test_scroll_position_math() {
        // Scroll up: block top starts at the band bottom, moves up.
        assert_eq!(
            LegacyEffect::scroll_top(0, 20.0, 1.0, false, 10.0, 100.0, 30.0),
            100.0
        );
        assert_eq!(
            LegacyEffect::scroll_top(200, 20.0, 1.0, false, 10.0, 100.0, 30.0),
            90.0
        );
        // Scroll down: block bottom starts at the band top, moves down.
        assert_eq!(
            LegacyEffect::scroll_top(0, 20.0, 1.0, true, 10.0, 100.0, 30.0),
            -20.0
        );
        assert_eq!(
            LegacyEffect::scroll_top(200, 20.0, 1.0, true, 10.0, 100.0, 30.0),
            -10.0
        );
    }
}
