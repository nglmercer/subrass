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

/// Cap on collected diagnostics: each distinct message is kept once,
/// and collection stops here so hostile documents cannot grow memory.
const MAX_WARNINGS: usize = 64;

/// True for characters in right-to-left scripts (Hebrew, Arabic
/// blocks, RTL controls). Used only to emit a shaping diagnostic;
/// layout itself stays left-to-right.
fn is_right_to_left(ch: char) -> bool {
    matches!(ch,
        '\u{0590}'..='\u{05FF}'
        | '\u{0600}'..='\u{06FF}'
        | '\u{0750}'..='\u{077F}'
        | '\u{08A0}'..='\u{08FF}'
        | '\u{200F}' | '\u{202B}' | '\u{202E}'
        | '\u{2066}'..='\u{2069}'
        | '\u{FB50}'..='\u{FDFF}'
        | '\u{FE70}'..='\u{FEFE}')
}

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
    /// Non-fatal diagnostics (e.g. embedded fonts that failed to load).
    /// A malformed attachment *encoding* is a hard parse error, while a
    /// decoded-but-unusable font only warns here and never breaks rendering.
    warnings: Vec<String>,
}

impl SubtitleRenderer {
    /// Create a new renderer from ASS content.
    ///
    /// Fonts embedded in `[Fonts]` attachments are automatically decoded
    /// and best-effort loaded (see [`Self::warnings`] for failures);
    /// [`Self::load_font`] remains available for manual loading.
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
        let mut warnings = Vec::new();
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
                Self::push_warning(&mut warnings, msg);
            }
        }

        // Note: system-font discovery is explicit (see
        // `load_system_fonts`), never eager here, so construction stays
        // deterministic in every feature combination.

        // Static document diagnostics: unknown tags, unsupported
        // effects, missing fonts, and right-to-left text. Collected
        // once here (deduplicated, capped) rather than per frame.
        Self::collect_doc_warnings(&doc, &font_manager, &mut warnings);

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
            warnings,
        })
    }

    /// Non-fatal diagnostics collected while building the renderer:
    /// embedded fonts that decoded but failed to load, unsupported
    /// override tags, unsupported `Effect` fields, requested fonts
    /// with no loaded face (fallback is used), and right-to-left
    /// text (laid out left-to-right). Each distinct message appears
    /// once; the list is capped at [`MAX_WARNINGS`].
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Push a diagnostic unless already present; cap the list and
    /// mirror to the console like before (API observability is new,
    /// console logging is preserved).
    fn push_warning(warnings: &mut Vec<String>, msg: String) {
        if warnings.len() >= MAX_WARNINGS || warnings.contains(&msg) {
            return;
        }
        #[cfg(target_arch = "wasm32")]
        web_sys::console::warn_1(&msg.clone().into());
        #[cfg(not(target_arch = "wasm32"))]
        eprintln!("{}", msg);
        warnings.push(msg);
    }

    /// Scan the document for static compatibility issues.
    fn collect_doc_warnings(
        doc: &AssDocument,
        font_manager: &FontManager,
        warnings: &mut Vec<String>,
    ) {
        use crate::types::{LegacyEffect, OverrideTag};
        let default_style = crate::types::Style::new("Default");
        for event in &doc.events {
            for tag in &event.parsed_tags {
                if let OverrideTag::Unknown(name) = tag {
                    Self::push_warning(
                        warnings,
                        format!("Unsupported override tag '\\{}' (ignored)", name),
                    );
                }
            }
            if !event.effect.trim().is_empty() && LegacyEffect::parse(&event.effect).is_none() {
                Self::push_warning(
                    warnings,
                    format!(
                        "Unsupported Effect field '{}' (renders as plain event)",
                        event.effect.trim()
                    ),
                );
            }
            let style = doc
                .find_style(&event.style)
                .unwrap_or_else(|| doc.get_default_style().unwrap_or(&default_style));
            let resolved = Compositor::resolve_style(style, event);
            if !font_manager.has_family(&resolved.font_name) {
                Self::push_warning(
                    warnings,
                    format!(
                        "Font '{}' has no loaded face (using fallback)",
                        resolved.font_name
                    ),
                );
            }
            if event.text.chars().any(is_right_to_left) {
                Self::push_warning(
                    warnings,
                    "Right-to-left text is laid out left-to-right (no bidi shaping)".to_string(),
                );
            }
        }
    }

    /// Number of loaded fonts (built-in fallback plus embedded/manual).
    pub fn font_count(&self) -> usize {
        self.font_manager.font_count()
    }

    /// Load a font from bytes
    pub fn load_font(&mut self, name: &str, data: &[u8]) -> Result<usize, String> {
        self.font_manager.load_font_auto(name, data)
    }

    /// Discover native system fonts (opt-in `system-fonts` feature,
    /// never WASM). Embedded/manual faces were loaded first and keep
    /// deterministic precedence; returns the number of families added.
    #[cfg(all(feature = "system-fonts", not(target_arch = "wasm32")))]
    pub fn load_system_fonts(&mut self) -> usize {
        self.font_manager.load_system_fonts()
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
            // 0 = unset: `composite_event` owns the libass
            // `ass_layout_res` fallback (both axes set, else video
            // size), so every caller shares one semantic.
            resolved.layout_res_x = self.doc.script_info.layout_res_x.unwrap_or(0);
            resolved.layout_res_y = self.doc.script_info.layout_res_y.unwrap_or(0);
            resolved.kerning = self.doc.script_info.kerning;

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
                // Parse-validated 0-3; clamp so direct struct writes past
                // i32::MAX wrap no fields (a bare `as i32` would go negative).
                self.doc.script_info.wrap_style.min(i32::MAX as u32) as i32,
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
        let renderer = SubtitleRenderer::new(ass).unwrap();
        // Best-effort: construction succeeds but records a warning.
        // Relative to a fontless baseline so optional system-font
        // discovery (which adds faces) cannot break the count.
        let baseline = SubtitleRenderer::new(
            "[Script Info]\nScriptType: v4.00+\nPlayResX: 640\nPlayResY: 480\n",
        )
        .unwrap();
        assert_eq!(renderer.font_count(), baseline.font_count());
        assert_eq!(renderer.warnings().len(), 1);
        assert!(renderer.warnings()[0].contains("Bad.ttf"));
    }

    #[test]
    fn test_valid_embedded_font_loads_automatically() {
        // Round-trip the real fallback bytes through the attachment
        // codec and confirm the renderer auto-loads them: no warnings,
        // and the font count grows past the built-in fallback.
        let encoded = crate::parser::attachment::encode_attachment_data(font::get_fallback_font());
        let mut ass = String::from(
            "[Script Info]\nScriptType: v4.00+\nPlayResX: 640\nPlayResY: 480\n\n[Fonts]\nfontname: Embedded.ttf\n",
        );
        for line in encoded.as_bytes().chunks(80) {
            ass.push_str(std::str::from_utf8(line).unwrap());
            ass.push('\n');
        }
        let renderer = SubtitleRenderer::new(&ass).unwrap();
        assert!(renderer.warnings().is_empty());
        // Relative to a fontless baseline so optional system-font
        // discovery (which adds faces) cannot break the count: the
        // embedded face adds exactly one.
        let baseline = SubtitleRenderer::new(
            "[Script Info]\nScriptType: v4.00+\nPlayResX: 640\nPlayResY: 480\n",
        )
        .unwrap();
        assert_eq!(renderer.font_count(), baseline.font_count() + 1);
    }

    /// System-font discovery is explicit and idempotent; the bundled
    /// fallback keeps precedence over any discovered same-named family.
    /// Env-independent: discovery may find zero faces on a bare system.
    #[cfg(all(feature = "system-fonts", not(target_arch = "wasm32")))]
    #[test]
    fn test_system_font_discovery_is_explicit_and_idempotent() {
        let mut renderer = SubtitleRenderer::new(
            "[Script Info]\nScriptType: v4.00+\nPlayResX: 640\nPlayResY: 480\n",
        )
        .unwrap();
        // Construction alone discovers nothing (deterministic).
        assert_eq!(renderer.font_count(), 1);
        let before = renderer.font_count();
        let added = renderer.load_system_fonts();
        // `added` counts families; collections contribute extra faces.
        assert!(renderer.font_count() >= before + added);
        // Second scan adds nothing (families already present).
        assert_eq!(renderer.load_system_fonts(), 0);
        // Bundled "DejaVu Sans" still resolves to the fallback face.
        let m = renderer
            .font_manager
            .find_font_with_weight("DejaVu Sans", 400, false);
        assert_eq!(m.id, 0);
    }

    /// Plan #58: one document exercising every diagnostic class.
    #[test]
    fn test_doc_warnings_cover_diagnostics() {
        let ass = "[Script Info]\n\
             ScriptType: v4.00+\n\
             PlayResX: 640\n\
             PlayResY: 480\n\
             \n\
             [V4+ Styles]\n\
             Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
             Style: Default,MissingFamilyXYZ,48,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,-1,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1\n\
             \n\
             [Events]\n\
             Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
             Dialogue: 0,0:00:01.00,0:00:04.00,Default,,0,0,0,Karaoke;10,{\\frobnicator1}Hello\n\
             Dialogue: 0,0:00:05.00,0:00:06.00,Default,,0,0,0,,שלום\n";
        let renderer = SubtitleRenderer::new(ass).unwrap();
        let warnings = renderer.warnings();
        assert!(
            warnings.iter().any(|w| w.contains("frobnicator")),
            "{warnings:?}"
        );
        assert!(
            warnings.iter().any(|w| w.contains("Karaoke;10")),
            "{warnings:?}"
        );
        assert!(
            warnings.iter().any(|w| w.contains("MissingFamilyXYZ")),
            "{warnings:?}"
        );
        assert!(
            warnings.iter().any(|w| w.contains("Right-to-left")),
            "{warnings:?}"
        );
        // Supported legacy effects and known tags stay silent.
        let clean = "[Script Info]\n\
             ScriptType: v4.00+\n\
             PlayResX: 640\n\
             PlayResY: 480\n\
             \n\
             [V4+ Styles]\n\
             Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
             Style: Default,DejaVu Sans,48,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,-1,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1\n\
             \n\
             [Events]\n\
             Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
             Dialogue: 0,0:00:01.00,0:00:04.00,Default,,0,0,0,Banner;20,{\\b1}Hello\n";
        let renderer = SubtitleRenderer::new(clean).unwrap();
        assert!(renderer.warnings().is_empty(), "{:?}", renderer.warnings());
    }

    /// Plan #58: diagnostics deduplicate and cap (hostile docs cannot
    /// grow memory through warnings).
    #[test]
    fn test_doc_warnings_dedup_and_cap() {
        let mut ass = String::from(
            "[Script Info]\nScriptType: v4.00+\nPlayResX: 64\nPlayResY: 64\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
        );
        for i in 0..200 {
            ass.push_str(&format!(
                "Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,BogusEffect{i},{{\\unktag{i}Hi}}\n"
            ));
        }
        let renderer = SubtitleRenderer::new(&ass).unwrap();
        let warnings = renderer.warnings();
        assert!(warnings.len() <= super::MAX_WARNINGS, "{}", warnings.len());
        // Same unknown tag repeated collapses to one entry.
        let mut ass2 = String::from(
            "[Script Info]\nScriptType: v4.00+\nPlayResX: 64\nPlayResY: 64\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
        );
        for _ in 0..50 {
            ass2.push_str("Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,{\\samebogus1}x\n");
        }
        let renderer = SubtitleRenderer::new(&ass2).unwrap();
        let tag_warnings: Vec<_> = renderer
            .warnings()
            .iter()
            .filter(|w| w.contains("samebogus"))
            .collect();
        assert_eq!(tag_warnings.len(), 1);
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
