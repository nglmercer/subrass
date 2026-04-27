pub mod buffer;
pub mod compositor;
pub mod drawing;
pub mod effects;
pub mod font;
pub mod glyph_cache;
pub mod shaper;

use wasm_bindgen::prelude::*;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData};

use crate::parser::AssDocument;
use crate::types::Event;
use self::buffer::RenderBuffer;
use self::compositor::Compositor;
use self::font::FontManager;

/// Main subtitle renderer
pub struct SubtitleRenderer {
    doc: AssDocument,
    font_manager: FontManager,
    compositor: Compositor,
    buffer: RenderBuffer,
    canvas: Option<HtmlCanvasElement>,
    ctx: Option<CanvasRenderingContext2d>,
    video_width: u32,
    video_height: u32,
}

impl SubtitleRenderer {
    /// Create a new renderer from ASS content
    pub fn new(ass_content: &str) -> Result<Self, String> {
        let doc = AssDocument::parse(ass_content)
            .map_err(|e| format!("Failed to parse ASS: {}", e))?;

        let mut font_manager = FontManager::new();

        // Load built-in fallback font
        let fallback_data = font::get_fallback_font();
        font_manager.load_font("DejaVu Sans", fallback_data, false, false)
            .map_err(|e| format!("Failed to load fallback font: {}", e))?;

        let play_res_x = doc.script_info.play_res_x;
        let play_res_y = doc.script_info.play_res_y;

        Ok(Self {
            doc,
            font_manager,
            compositor: Compositor::new(),
            buffer: RenderBuffer::new(play_res_x, play_res_y),
            canvas: None,
            ctx: None,
            video_width: play_res_x,
            video_height: play_res_y,
        })
    }

    /// Load a font from bytes
    pub fn load_font(&mut self, name: &str, data: &[u8]) -> Result<usize, String> {
        self.font_manager.load_font_auto(name, data)
    }

    /// Set the canvas element to render to
    pub fn set_canvas(&mut self, canvas: HtmlCanvasElement) -> Result<(), String> {
        let ctx = canvas
            .get_context("2d")
            .map_err(|e| format!("Failed to get 2d context: {:?}", e))?
            .ok_or_else(|| "Failed to get 2d context".to_string())?
            .dyn_into::<CanvasRenderingContext2d>()
            .map_err(|e| format!("Failed to cast context: {:?}", e))?;

        self.canvas = Some(canvas);
        self.ctx = Some(ctx);
        Ok(())
    }

    /// Set the video dimensions for scaling
    pub fn set_video_size(&mut self, width: u32, height: u32) {
        self.video_width = width;
        self.video_height = height;
        self.buffer.resize(width, height);
    }

    /// Resize the render buffer
    pub fn resize(&mut self, width: u32, height: u32) {
        self.buffer.resize(width, height);
    }

    /// Render a single frame at the given time
    pub fn render_frame(&mut self, time_ms: u64) -> Result<(), String> {
        // Clear buffer
        self.buffer.clear();

        // Get active events
        let active_events: Vec<Event> = self.doc.events.iter()
            .filter(|e| e.is_active_at(time_ms))
            .cloned()
            .collect();

        // Sort by layer
        let mut sorted_events = active_events;
        sorted_events.sort_by(|a, b| a.layer.cmp(&b.layer));

        // Render each event
        let default_style = crate::types::Style::new("Default");

        for event in &sorted_events {
            let style = self.doc.find_style(&event.style)
                .unwrap_or_else(|| {
                    self.doc.get_default_style().unwrap_or(&default_style)
                });

            let resolved = Compositor::resolve_style(style, event);

            self.compositor.composite_event(
                &mut self.buffer,
                event,
                &resolved,
                &self.font_manager,
                time_ms,
                self.doc.script_info.play_res_x,
                self.doc.script_info.play_res_y,
                self.video_width,
                self.video_height,
            );
        }

        // Transfer buffer to canvas
        self.transfer_to_canvas()?;

        Ok(())
    }

    /// Transfer the render buffer to the canvas
    fn transfer_to_canvas(&self) -> Result<(), String> {
        let ctx = self.ctx.as_ref().ok_or("No canvas context set")?;
        let canvas = self.canvas.as_ref().ok_or("No canvas set")?;

        // Resize canvas if needed
        if canvas.width() != self.video_width || canvas.height() != self.video_height {
            canvas.set_width(self.video_width);
            canvas.set_height(self.video_height);
        }

        // Create ImageData from buffer
        let image_data = ImageData::new_with_u8_clamped_array_and_sh(
            wasm_bindgen::Clamped(self.buffer.as_bytes()),
            self.video_width,
            self.video_height,
        )
        .map_err(|e| format!("Failed to create ImageData: {:?}", e))?;

        // Draw to canvas
        ctx.put_image_data(&image_data, 0.0, 0.0)
            .map_err(|e| format!("Failed to put image data: {:?}", e))?;

        Ok(())
    }

    /// Get the document's play resolution
    pub fn get_play_resolution(&self) -> (u32, u32) {
        (self.doc.script_info.play_res_x, self.doc.script_info.play_res_y)
    }

    /// Get the document reference
    pub fn document(&self) -> &AssDocument {
        &self.doc
    }

    /// Clear the glyph cache
    pub fn clear_cache(&mut self) {
        self.compositor.clear_cache();
    }
}
