use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

use crate::parser::AssDocument;
use crate::renderer::SubtitleRenderer as InnerRenderer;

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
    pub fn get_script_info(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.inner.script_info).unwrap_or_default()
    }

    /// Get all styles as a JavaScript array
    pub fn get_styles(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.inner.styles).unwrap_or_default()
    }

    /// Get all events as a JavaScript array
    pub fn get_events(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.inner.events).unwrap_or_default()
    }

    /// Get events active at a specific time (in milliseconds)
    pub fn get_events_at_time(&self, time_ms: f64) -> JsValue {
        let events = self.inner.get_events_at_time(time_ms as u64);
        serde_wasm_bindgen::to_value(&events).unwrap_or_default()
    }

    /// Get only dialogue events (not comments)
    pub fn get_dialogue_events(&self) -> JsValue {
        let events = self.inner.get_dialogue_events();
        serde_wasm_bindgen::to_value(&events).unwrap_or_default()
    }

    /// Get only comment events
    pub fn get_comment_events(&self) -> JsValue {
        let events = self.inner.get_comment_events();
        serde_wasm_bindgen::to_value(&events).unwrap_or_default()
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
    pub fn find_style(&self, name: &str) -> JsValue {
        match self.inner.find_style(name) {
            Some(style) => serde_wasm_bindgen::to_value(style).unwrap_or_default(),
            None => JsValue::NULL,
        }
    }

    /// Get the default style
    pub fn get_default_style(&self) -> JsValue {
        match self.inner.get_default_style() {
            Some(style) => serde_wasm_bindgen::to_value(style).unwrap_or_default(),
            None => JsValue::NULL,
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
    pub fn get_event(&self, index: usize) -> JsValue {
        if index < self.inner.events.len() {
            serde_wasm_bindgen::to_value(&self.inner.events[index]).unwrap_or_default()
        } else {
            JsValue::NULL
        }
    }

    /// Get style at index
    pub fn get_style(&self, index: usize) -> JsValue {
        if index < self.inner.styles.len() {
            serde_wasm_bindgen::to_value(&self.inner.styles[index]).unwrap_or_default()
        } else {
            JsValue::NULL
        }
    }
}

/// Parse an ASS file and return a document
#[wasm_bindgen]
pub fn parse_ass(content: &str) -> Result<AssDoc, JsError> {
    AssDoc::new(content)
}

/// Validate ASS content without returning the full document
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

    serde_wasm_bindgen::to_value(&summary).map_err(|e| JsError::new(&e.to_string()))
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
pub fn ms_to_ass_time(ms: f64) -> String {
    crate::types::Time::from_millis(ms as u64).to_string()
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
        let inner = InnerRenderer::new(ass_content)
            .map_err(|e| JsError::new(&e.to_string()))?;
        Ok(Self { inner })
    }

    /// Load a font from bytes (Uint8Array)
    pub fn load_font(&mut self, name: &str, data: &[u8]) -> Result<usize, JsError> {
        self.inner.load_font(name, data)
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Set the canvas element to render to
    pub fn set_canvas(&mut self, canvas: HtmlCanvasElement) -> Result<(), JsError> {
        self.inner.set_canvas(canvas)
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Set the video dimensions for scaling
    pub fn set_video_size(&mut self, width: u32, height: u32) {
        self.inner.set_video_size(width, height);
    }

    /// Resize the render buffer
    pub fn resize(&mut self, width: u32, height: u32) {
        self.inner.resize(width, height);
    }

    /// Render a single frame at the given time (in milliseconds)
    pub fn render_frame(&mut self, time_ms: f64) -> Result<(), JsError> {
        self.inner.render_frame(time_ms as u64)
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Get the document's play resolution
    pub fn get_play_resolution(&self) -> Vec<u32> {
        let (w, h) = self.inner.get_play_resolution();
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
}
