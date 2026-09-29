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
                // An ASS header fixes the first eight bytes, and the old
                // modulo-hour timestamp sampled normal seeds far past their
                // events. Deliberately exercise one active dialogue interval.
                let sample_time = renderer
                    .document()
                    .events
                    .iter()
                    .find(|event| event.is_dialogue() && event.end > event.start)
                    .map(|event| {
                        let start = event.start.to_millis();
                        start.saturating_add(time_ms % (event.end.to_millis() - start))
                    })
                    .unwrap_or(time_ms % 3_600_000);
                let _ = renderer.render_frame(sample_time);
            }
        }
    }
});
