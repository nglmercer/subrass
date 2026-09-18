use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

use crate::parser::AssDocument;
use crate::renderer::SubtitleRenderer as InnerRenderer;

/// Largest integer a JS number / f64 represents exactly. Timestamps
/// must fit here — `u64::MAX as f64` cannot be used as a boundary
/// because it rounds up to 2^64, admitting unrepresentable values.
const JS_MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// Validate a millisecond timestamp coming from JavaScript.
///
/// Requires a finite value in `0..=Number.MAX_SAFE_INTEGER`.
/// Fractional milliseconds are floored (matching media-timestamp callers
/// that pass `currentTime * 1000`). Rejects NaN, infinities, negatives,
/// and out-of-range values instead of silently casting them (where
/// `as u64` maps NaN/negatives to 0 and infinity to u64::MAX).
/// Plain-String validation so the logic is unit-testable on native
/// targets (`JsError::new` requires wasm); the WASM boundary maps the
/// message with `JsError::new`.
fn validate_millis(value: f64) -> Result<u64, String> {
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

fn to_js_millis(value: f64) -> Result<u64, JsError> {
    validate_millis(value).map_err(|e| JsError::new(&e))
}

fn to_js<T: serde::Serialize>(value: &T) -> Result<JsValue, JsError> {
    serde_wasm_bindgen::to_value(value).map_err(|e| JsError::new(&e.to_string()))
}

/// JavaScript-friendly ASS document wrapper
#[wasm_bindgen]
pub struct AssDoc {
    inner: AssDocument,
}

#[wasm_bindgen]
impl AssDoc {
    /// Create a new ASS document from content string
    #[wasm_bindgen(constructor)]
    pub fn new(content: &str) -> Result<AssDoc, JsError> {
        let inner = AssDocument::parse(content).map_err(|e| JsError::new(&e.to_string()))?;
        Ok(Self { inner })
    }

    /// Get script information as a JavaScript object
    pub fn get_script_info(&self) -> Result<JsValue, JsError> {
        to_js(&self.inner.script_info)
    }

    /// Get all styles as a JavaScript array
    pub fn get_styles(&self) -> Result<JsValue, JsError> {
        to_js(&self.inner.styles)
    }

    /// Get all events as a JavaScript array
    pub fn get_events(&self) -> Result<JsValue, JsError> {
        to_js(&self.inner.events)
    }

    /// Get events active at a specific time (in milliseconds)
    pub fn get_events_at_time(&self, time_ms: f64) -> Result<JsValue, JsError> {
        let time_ms = to_js_millis(time_ms)?;
        let events = self.inner.get_events_at_time(time_ms);
        to_js(&events)
    }

    /// Get only dialogue events (not comments)
    pub fn get_dialogue_events(&self) -> Result<JsValue, JsError> {
        let events = self.inner.get_dialogue_events();
        to_js(&events)
    }

    /// Get only comment events
    pub fn get_comment_events(&self) -> Result<JsValue, JsError> {
        let events = self.inner.get_comment_events();
        to_js(&events)
    }

    /// Get the number of events
    pub fn get_event_count(&self) -> usize {
        self.inner.get_event_count()
    }

    /// Get the number of styles
    pub fn get_style_count(&self) -> usize {
        self.inner.get_style_count()
    }

    /// Find a style by name
    pub fn find_style(&self, name: &str) -> Result<JsValue, JsError> {
        match self.inner.find_style(name) {
            Some(style) => to_js(style),
            None => Ok(JsValue::NULL),
        }
    }

    /// Get the default style
    pub fn get_default_style(&self) -> Result<JsValue, JsError> {
        match self.inner.get_default_style() {
            Some(style) => to_js(style),
            None => Ok(JsValue::NULL),
        }
    }

    /// Get play resolution X
    pub fn get_play_res_x(&self) -> u32 {
        self.inner.script_info.play_res_x
    }

    /// Get play resolution Y
    pub fn get_play_res_y(&self) -> u32 {
        self.inner.script_info.play_res_y
    }

    /// Sort events by time
    pub fn sort_events_by_time(&mut self) {
        self.inner.sort_events_by_time();
    }

    /// Sort events by layer
    pub fn sort_events_by_layer(&mut self) {
        self.inner.sort_events_by_layer();
    }

    /// Get event at index
    pub fn get_event(&self, index: usize) -> Result<JsValue, JsError> {
        if index < self.inner.events.len() {
            to_js(&self.inner.events[index])
        } else {
            Ok(JsValue::NULL)
        }
    }

    /// Get style at index
    pub fn get_style(&self, index: usize) -> Result<JsValue, JsError> {
        if index < self.inner.styles.len() {
            to_js(&self.inner.styles[index])
        } else {
            Ok(JsValue::NULL)
        }
    }

    /// Get the number of embedded attachments ([Fonts] + [Graphics])
    pub fn get_attachment_count(&self) -> usize {
        self.inner.attachments.len()
    }

    /// Get the filename of the attachment at index
    pub fn get_attachment_name(&self, index: usize) -> Option<String> {
        self.inner
            .attachments
            .get(index)
            .map(|a| a.filename.clone())
    }

    /// Get the kind of the attachment at index ("font" or "graphic")
    pub fn get_attachment_kind(&self, index: usize) -> Option<String> {
        self.inner.attachments.get(index).map(|a| match a.kind {
            crate::types::AttachmentKind::Font => "font".to_string(),
            crate::types::AttachmentKind::Graphic => "graphic".to_string(),
        })
    }

    /// Get the decoded binary data of the attachment at index (Uint8Array)
    pub fn get_attachment_data(&self, index: usize) -> Option<Vec<u8>> {
        self.inner.attachments.get(index).map(|a| a.data.clone())
    }
}

/// Parse an ASS file and return a document
#[wasm_bindgen]
pub fn parse_ass(content: &str) -> Result<AssDoc, JsError> {
    AssDoc::new(content)
}

/// Validate ASS content without returning the full document.
///
/// Success means the document passed syntactic validation: sections,
/// formats, styles, events, timestamps, and attachments all parsed
/// without malformed values (malformed values are errors, not silent
/// defaults). An empty document is valid; non-empty input without any
/// recognized section is not.
#[wasm_bindgen]
pub fn validate_ass(content: &str) -> Result<bool, JsError> {
    AssDocument::parse(content)
        .map(|_| true)
        .map_err(|e| JsError::new(&e.to_string()))
}

/// Summary struct for JavaScript
#[derive(serde::Serialize)]
pub struct AssSummary {
    pub title: Option<String>,
    pub script_type: String,
    pub play_res_x: u32,
    pub play_res_y: u32,
    pub style_count: usize,
    pub event_count: usize,
    pub dialogue_count: usize,
    pub comment_count: usize,
    pub style_names: Vec<String>,
}

/// Get a summary of the ASS file
#[wasm_bindgen]
pub fn get_ass_summary(content: &str) -> Result<JsValue, JsError> {
    let doc = AssDocument::parse(content).map_err(|e| JsError::new(&e.to_string()))?;

    let summary = AssSummary {
        title: doc.script_info.title.clone(),
        script_type: doc.script_info.script_type.to_string(),
        play_res_x: doc.script_info.play_res_x,
        play_res_y: doc.script_info.play_res_y,
        style_count: doc.styles.len(),
        event_count: doc.events.len(),
        dialogue_count: doc.get_dialogue_events().len(),
        comment_count: doc.get_comment_events().len(),
        style_names: doc.styles.iter().map(|s| s.name.clone()).collect(),
    };

    to_js(&summary)
}

/// Helper function to convert ASS time to milliseconds
#[wasm_bindgen]
pub fn ass_time_to_ms(time_str: &str) -> Result<f64, JsError> {
    let time: crate::types::Time = time_str
        .parse()
        .map_err(|e: crate::types::time::TimeError| JsError::new(&e.to_string()))?;
    Ok(time.to_millis() as f64)
}

/// Helper function to convert milliseconds to ASS time
#[wasm_bindgen]
pub fn ms_to_ass_time(ms: f64) -> Result<String, JsError> {
    let ms = to_js_millis(ms)?;
    Ok(crate::types::Time::from_millis(ms).to_string())
}

/// WASM-exported subtitle renderer
#[wasm_bindgen]
pub struct SubtitleRenderer {
    inner: InnerRenderer,
}

#[wasm_bindgen]
impl SubtitleRenderer {
    /// Create a new renderer from ASS content
    #[wasm_bindgen(constructor)]
    pub fn new(ass_content: &str) -> Result<SubtitleRenderer, JsError> {
        let inner = InnerRenderer::new(ass_content).map_err(|e| JsError::new(&e.to_string()))?;
        Ok(Self { inner })
    }

    /// Load a font from bytes (Uint8Array)
    pub fn load_font(&mut self, name: &str, data: &[u8]) -> Result<usize, JsError> {
        self.inner
            .load_font(name, data)
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Set the canvas element to render to
    pub fn set_canvas(&mut self, canvas: HtmlCanvasElement) -> Result<(), JsError> {
        self.inner
            .set_canvas(canvas)
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Set the video dimensions for scaling. Invalid dimensions throw.
    pub fn set_video_size(&mut self, width: u32, height: u32) -> Result<(), JsError> {
        self.inner
            .set_video_size(width, height)
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Resize the render buffer. Invalid dimensions throw.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), JsError> {
        self.inner
            .resize(width, height)
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Render a single frame at the given time (in milliseconds)
    pub fn render_frame(&mut self, time_ms: f64) -> Result<(), JsError> {
        let time_ms = to_js_millis(time_ms)?;
        self.inner
            .render_frame(time_ms)
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Get the document's play resolution
    pub fn get_play_resolution(&self) -> Vec<u32> {
        let (w, h) = self.inner.get_play_resolution();
        vec![w, h]
    }

    /// Get the last rendered frame as RGBA bytes (Uint8Array).
    /// Works without a canvas, for rendering in a Web Worker.
    ///
    /// Performance note: this copies the full frame on every call
    /// (WASM→JS boundary). Callers that only need some frames should
    /// coalesce render requests; see the worker backend.
    pub fn get_frame_data(&self) -> Vec<u8> {
        self.inner.frame_data().to_vec()
    }

    /// Get the render buffer dimensions [width, height]
    pub fn get_frame_size(&self) -> Vec<u32> {
        let (w, h) = self.inner.frame_size();
        vec![w, h]
    }

    /// Get event count
    pub fn get_event_count(&self) -> usize {
        self.inner.document().get_event_count()
    }

    /// Get style count
    pub fn get_style_count(&self) -> usize {
        self.inner.document().get_style_count()
    }

    /// Clear the glyph cache
    pub fn clear_cache(&mut self) {
        self.inner.clear_cache();
    }

    /// Non-fatal renderer diagnostics (e.g. embedded fonts that failed
    /// to load) as a JS string array.
    pub fn get_warnings(&self) -> Vec<String> {
        self.inner.warnings().to_vec()
    }

    /// Number of loaded fonts (built-in fallback plus embedded/manual).
    pub fn get_font_count(&self) -> usize {
        self.inner.font_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_millis_accepts_normal_values() {
        assert_eq!(validate_millis(0.0).unwrap(), 0);
        assert_eq!(validate_millis(1.0).unwrap(), 1);
        assert_eq!(validate_millis(1500.0).unwrap(), 1500);
        // Fractional millis floor (media timestamps are fractional).
        assert_eq!(validate_millis(1500.9).unwrap(), 1500);
        assert_eq!(validate_millis(0.9).unwrap(), 0);
    }

    #[test]
    fn test_validate_millis_rejects_invalid() {
        assert!(validate_millis(f64::NAN).is_err());
        assert!(validate_millis(f64::INFINITY).is_err());
        assert!(validate_millis(f64::NEG_INFINITY).is_err());
        assert!(validate_millis(-1.0).is_err());
        assert!(validate_millis(-0.5).is_err());
        assert!(validate_millis(1e30).is_err());
    }

    #[test]
    fn test_validate_millis_safe_integer_boundary() {
        // MAX_SAFE_INTEGER is the exact upper bound (u64::MAX-as-f64
        // would round to 2^64 and admit unrepresentable values).
        assert_eq!(
            validate_millis(JS_MAX_SAFE_INTEGER).unwrap(),
            9_007_199_254_740_991u64
        );
        assert!(validate_millis(JS_MAX_SAFE_INTEGER + 1.0).is_err());
        assert!(validate_millis(18_446_744_073_709_516_160.0).is_err()); // 2^64
        assert!(validate_millis(u64::MAX as f64).is_err());
    }
}
