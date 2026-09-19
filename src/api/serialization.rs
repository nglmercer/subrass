use wasm_bindgen::prelude::*;

/// Largest integer a JS number / f64 represents exactly. Timestamps
/// must fit here — `u64::MAX as f64` cannot be used as a boundary
/// because it rounds up to 2^64, admitting unrepresentable values.
pub(super) const JS_MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// Validate a millisecond timestamp coming from JavaScript.
///
/// Requires a finite value in `0..=Number.MAX_SAFE_INTEGER`.
/// Fractional milliseconds are floored (matching media-timestamp callers
/// that pass `currentTime * 1000`). Rejects NaN, infinities, negatives,
/// and out-of-range values instead of silently casting them (where
/// `as u64` maps NaN/negatives to 0 and infinity to u64::MAX).
/// Plain-String validation keeps the logic unit-testable on native
/// targets (`JsError::new` requires wasm); the WASM boundary maps the
/// message with `JsError::new`.
pub(super) fn validate_millis(value: f64) -> Result<u64, String> {
    if !value.is_finite() {
        return Err("Time must be a finite number of milliseconds".to_string());
    }
    if value < 0.0 {
        return Err("Time must not be negative".to_string());
    }
    if value > JS_MAX_SAFE_INTEGER {
        return Err("Time exceeds Number.MAX_SAFE_INTEGER milliseconds".to_string());
    }
    Ok(value as u64)
}

pub(super) fn to_js_millis(value: f64) -> Result<u64, JsError> {
    validate_millis(value).map_err(|e| JsError::new(&e))
}

pub(super) fn to_js<T: serde::Serialize>(value: &T) -> Result<JsValue, JsError> {
    serde_wasm_bindgen::to_value(value).map_err(|e| JsError::new(&e.to_string()))
}
