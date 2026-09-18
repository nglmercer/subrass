pub mod buffer;
pub mod compositor;
pub mod drawing;
pub mod effects;
pub mod font;
pub mod glyph_cache;
pub mod shaper;

use wasm_bindgen::prelude::*;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData};

use self::buffer::RenderBuffer;
use self::compositor::Compositor;
use self::font::FontManager;
use crate::parser::AssDocument;
use crate::types::Event;

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
        let doc =
            AssDocument::parse(ass_content).map_err(|e| format!("Failed to parse ASS: {}", e))?;

        let mut font_manager = FontManager::new();

        // Load built-in fallback font
        let fallback_data = font::get_fallback_font();
        font_manager
            .load_font("DejaVu Sans", fallback_data, false, false)
            .map_err(|e| format!("Failed to load fallback font: {}", e))?;

        // Best-effort load of fonts embedded in the [Fonts] section
        for attachment in doc
            .attachments
            .iter()
            .filter(|a| a.kind == crate::types::AttachmentKind::Font)
        {
            let stem = attachment
                .filename
                .rsplit_once('.')
                .map(|(s, _)| s)
                .unwrap_or(&attachment.filename);
            if let Err(e) = font_manager.load_font_auto(stem, &attachment.data) {
                let msg = format!(
                    "Failed to load embedded font {}: {}",
                    attachment.filename, e
                );
                #[cfg(target_arch = "wasm32")]
                web_sys::console::warn_1(&msg.into());
                #[cfg(not(target_arch = "wasm32"))]
                eprintln!("{}", msg);
            }
        }

        let play_res_x = doc.script_info.play_res_x;
        let play_res_y = doc.script_info.play_res_y;
        let buffer = RenderBuffer::new(play_res_x, play_res_y)
            .map_err(|e| format!("Invalid play resolution: {}", e))?;

        Ok(Self {
            doc,
            font_manager,
            compositor: Compositor::new(),
            buffer,
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

    /// Set the video dimensions for scaling. All state (video size,
    /// buffer size, canvas transfer size) is updated together; invalid
    /// dimensions are rejected and leave the old size untouched.
    pub fn set_video_size(&mut self, width: u32, height: u32) -> Result<(), String> {
        self.buffer
            .resize(width, height)
            .map_err(|e| format!("Invalid video size: {}", e))?;
        self.video_width = self.buffer.width;
        self.video_height = self.buffer.height;
        Ok(())
    }

    /// Resize the render buffer (same as [`Self::set_video_size`]: the
    /// buffer, video, and canvas dimensions stay consistent).
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        self.set_video_size(width, height)
    }

    /// Render a single frame at the given time
    pub fn render_frame(&mut self, time_ms: u64) -> Result<(), String> {
        // Clear buffer
        self.buffer.clear();

        // Active dialogue events only: comments are never rendered, so
        // filter them before any style resolution or allocation. Collect
        // references (no per-frame event cloning) and stable-sort by
        // layer so equal layers keep source order.
        let mut active_events: Vec<&Event> = self
            .doc
            .events
            .iter()
            .filter(|e| e.is_dialogue() && e.is_active_at(time_ms))
            .collect();
        active_events.sort_by_key(|a| a.layer);

        // Render each event
        let default_style = crate::types::Style::new("Default");

        for event in &active_events {
            let style = self
                .doc
                .find_style(&event.style)
                .unwrap_or_else(|| self.doc.get_default_style().unwrap_or(&default_style));

            let mut resolved = Compositor::resolve_style(style, event);
            resolved.scaled_border_and_shadow = self.doc.script_info.scaled_border_and_shadow;

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
                self.doc.script_info.wrap_style as i32,
                &self.doc.styles,
            );
        }

        // Transfer buffer to canvas when one is set. Without a canvas the
        // frame stays in the buffer for retrieval via frame_data() (used
        // by the Web Worker path).
        if self.canvas.is_some() {
            self.transfer_to_canvas()?;
        }

        Ok(())
    }

    /// Raw RGBA bytes of the last rendered frame
    pub fn frame_data(&self) -> &[u8] {
        self.buffer.as_bytes()
    }

    /// Dimensions of the render buffer (source of truth for frame size)
    pub fn frame_size(&self) -> (u32, u32) {
        (self.buffer.width, self.buffer.height)
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
        ctx.clear_rect(0.0, 0.0, canvas.width() as f64, canvas.height() as f64);
        ctx.put_image_data(&image_data, 0.0, 0.0)
            .map_err(|e| format!("Failed to put image data: {:?}", e))?;

        Ok(())
    }

    /// Get the document's play resolution
    pub fn get_play_resolution(&self) -> (u32, u32) {
        (
            self.doc.script_info.play_res_x,
            self.doc.script_info.play_res_y,
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedded_garbage_font_does_not_break_renderer() {
        let ass = "[Script Info]\nScriptType: v4.00+\nPlayResX: 640\nPlayResY: 480\n\n[Fonts]\nfontname: Bad.ttf\n15*$\n";
        assert!(SubtitleRenderer::new(ass).is_ok());
    }

    #[test]
    fn test_resize_keeps_state_consistent() {
        let ass = "[Script Info]\nScriptType: v4.00+\nPlayResX: 640\nPlayResY: 480\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:04.00,Default,,0,0,0,,Hi";
        let mut renderer = SubtitleRenderer::new(ass).unwrap();
        renderer.resize(640, 360).unwrap();
        assert_eq!(renderer.frame_size(), (640, 360));
        assert_eq!(renderer.frame_data().len(), 640 * 360 * 4);
        // Invalid resize rejected, old size kept
        assert!(renderer.resize(0, 100).is_err());
        assert!(renderer.set_video_size(u32::MAX, u32::MAX).is_err());
        assert_eq!(renderer.frame_size(), (640, 360));
        renderer.render_frame(2000).unwrap();
    }

    #[test]
    fn test_render_sample_feature_sections() {
        let mut renderer = SubtitleRenderer::new(include_str!("../../demo/sample.ass")).unwrap();
        renderer.set_video_size(320, 180).unwrap();

        for time_ms in [
            11_000, 26_000, 41_000, 53_000, 66_000, 81_000, 96_000, 111_000, 126_000, 131_000,
            151_000, 166_000, 181_000, 196_000, 211_000, 226_000, 241_000, 256_000, 271_000,
            286_000, 301_000,
        ] {
            renderer.render_frame(time_ms).unwrap();
            assert_eq!(renderer.frame_size(), (320, 180));
        }
    }
}
