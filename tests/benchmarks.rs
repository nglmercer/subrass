//! Reproducible release-mode rendering benchmarks.
//!
//! Run with:
//! `cargo test --release --test benchmarks -- --ignored --nocapture --test-threads=1`

use std::hint::black_box;
use std::time::Instant;

use subrass::renderer::SubtitleRenderer;

const HEADER: &str = r#"[Script Info]
Title: benchmark
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,DejaVu Sans,48,&H00FFFFFF,&H0000FFFF,&H00000000,&H80000000,0,0,0,0,100,100,0,0,1,2,1,2,24,24,32,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
"#;

fn document(events: &str) -> String {
    format!("{HEADER}{events}")
}

fn many_events() -> String {
    (0..96)
        .map(|index| {
            format!(
                "Dialogue: {},0:00:00.00,0:00:10.00,Default,,0,0,0,,Event {index} {{\\pos({},{})}}\n",
                index % 4,
                40 + (index % 16) * 110,
                48 + (index / 16) * 90
            )
        })
        .collect()
}

fn percentile(samples: &mut [u128], fraction: f64) -> u128 {
    samples.sort_unstable();
    let index = ((samples.len() - 1) as f64 * fraction).round() as usize;
    samples[index]
}

#[test]
#[ignore = "manual release benchmark"]
fn render_benchmark_matrix() {
    let cases = [
        (
            "1080p-plain",
            document("Dialogue: 0,0:00:00.00,0:00:10.00,Default,,0,0,0,,Hello benchmark\n"),
            1920,
            1080,
        ),
        (
            "1440p-many-events",
            document(&many_events()),
            2560,
            1440,
        ),
        (
            "4k-effects-drawing",
            document("Dialogue: 0,0:00:00.00,0:00:10.00,Default,,0,0,0,,{\\blur3\\bord4\\shad3\\p1\\kf100}m 0 0 b 0 100 100 100 100 0 c\n"),
            3840,
            2160,
        ),
        (
            "1080p-shaping-karaoke",
            document("Dialogue: 0,0:00:00.00,0:00:10.00,Default,,0,0,0,,{\\kf40}مرحبا {\\kf40}नमस्ते {\\kf40}עברית\n"),
            1920,
            1080,
        ),
    ];

    for (name, ass, width, height) in cases {
        let mut renderer = SubtitleRenderer::new(&ass).expect("benchmark document parses");
        renderer
            .set_video_size(width, height)
            .expect("benchmark dimensions are valid");

        let mut samples = Vec::with_capacity(12);
        for frame in 0..12 {
            let started = Instant::now();
            renderer
                .render_frame(black_box((frame * 137) as u64))
                .expect("benchmark frame renders");
            black_box(renderer.frame_data().len());
            samples.push(started.elapsed().as_micros());
        }

        let p50 = percentile(&mut samples, 0.50);
        let p95 = percentile(&mut samples, 0.95);
        println!("{name}: p50={p50}us p95={p95}us samples={samples:?}");
    }
}
