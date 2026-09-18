#![no_main]
use libfuzzer_sys::fuzz_target;
use subrass::renderer::SubtitleRenderer;

// Full render path over hostile documents: hostile positions, clips,
// rotations, shears, blurs, drawings, and dimensions must degrade to
// empty output or errors, never panic.
fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let text: String = text.chars().take(20_000).collect();
        let mut time_ms = 0u64;
        for (i, b) in data.iter().take(8).enumerate() {
            time_ms |= u64::from(*b) << (i * 8);
        }
        if let Ok(mut renderer) = SubtitleRenderer::new(&text) {
            // Small fixed frame: renderer caps, not the harness, bound memory.
            if renderer.set_video_size(256, 144).is_ok() {
                let _ = renderer.render_frame(time_ms % 3_600_000);
            }
        }
    }
});
