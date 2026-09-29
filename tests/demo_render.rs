use subrass::{renderer::SubtitleRenderer, types::OverrideTag};

const SAMPLE: &str = include_str!("../demo/sample.ass");

fn color_bounds(data: &[u8], width: u32, keep: impl Fn(&[u8]) -> bool) -> (u32, u32, u32, u32) {
    let mut bounds = (u32::MAX, u32::MAX, 0, 0);
    let mut count = 0;
    for (i, pixel) in data.chunks_exact(4).enumerate() {
        if pixel[3] > 80 && keep(pixel) {
            let (x, y) = (i as u32 % width, i as u32 / width);
            bounds = (
                bounds.0.min(x),
                bounds.1.min(y),
                bounds.2.max(x),
                bounds.3.max(y),
            );
            count += 1;
        }
    }
    assert!(count > 20, "expected visible color coverage");
    bounds
}

#[test]
fn screenshot_color_sample_has_authored_positions_and_colors() {
    let mut renderer = SubtitleRenderer::new(SAMPLE).unwrap();
    renderer.set_video_size(1036, 583).unwrap();
    renderer.render_frame(68109).unwrap();
    let data = renderer.frame_data();
    let red = color_bounds(data, 1036, |p| p[0] > 200 && p[1] < 70 && p[2] < 70);
    let green = color_bounds(data, 1036, |p| p[1] > 200 && p[0] < 70 && p[2] < 70);
    let blue = color_bounds(data, 1036, |p| p[2] > 200 && p[0] < 70 && p[1] < 70);
    let pink = color_bounds(data, 1036, |p| p[0] > 200 && p[2] > 200 && p[1] < 70);
    let yellow = color_bounds(data, 1036, |p| p[0] > 200 && p[1] > 200 && p[2] < 70);
    assert!(red.2 < green.0 && green.2 < blue.0);
    assert!(red.1.abs_diff(green.1) < 4 && green.1.abs_diff(blue.1) < 4);
    assert!(pink.1 > red.3);
    assert!(yellow.0 <= pink.0 && yellow.2 >= pink.2);
    assert!(((red.1 + red.3) / 2).abs_diff(162) < 8);
    assert!(((pink.1 + pink.3) / 2).abs_diff(200) < 8);
}

#[test]
fn screenshot_layer_sample_keeps_explicitly_overlapping_positions() {
    let mut renderer = SubtitleRenderer::new(SAMPLE).unwrap();
    renderer.set_video_size(1036, 583).unwrap();
    renderer.render_frame(289345).unwrap();
    let data = renderer.frame_data();
    let blue = color_bounds(data, 1036, |p| p[2] > 200 && p[0] < 70 && p[1] < 70);
    let green = color_bounds(data, 1036, |p| p[1] > 200 && p[0] < 70 && p[2] < 70);
    let red = color_bounds(data, 1036, |p| p[0] > 200 && p[1] < 70 && p[2] < 70);
    assert!(blue.1 < green.1 && green.1 < red.1);
    assert!(
        blue.3 >= green.1 && green.3 >= red.1,
        "the demo deliberately overlaps layers"
    );
    let expected = data.to_vec();
    renderer.render_frame(68109).unwrap();
    renderer.render_frame(289345).unwrap();
    assert_eq!(renderer.frame_data(), expected);
}

#[test]
fn full_demo_renders_event_boundaries_and_has_no_unknown_override_examples() {
    use std::collections::BTreeSet;
    let mut renderer = SubtitleRenderer::new(SAMPLE).unwrap();
    renderer.set_video_size(512, 288).unwrap();
    let mut times = BTreeSet::new();
    for event in &renderer.document().events {
        for tag in &event.parsed_tags {
            assert!(
                !matches!(tag, OverrideTag::Unknown(_)),
                "unexpected demo override: {tag:?}"
            );
        }
        if event.is_dialogue() {
            let (start, end) = (event.start.to_millis(), event.end.to_millis());
            times.extend([
                start,
                start.saturating_sub(1),
                start + (end - start) / 2,
                end.saturating_sub(1),
                end,
            ]);
        }
    }
    assert!(times.len() > 150);
    eprintln!(
        "DEMO_SWEEP events={} frames={}",
        renderer.document().events.len(),
        times.len()
    );
    for time in times {
        renderer.render_frame(time).unwrap();
        assert_eq!(renderer.frame_data().len(), 512 * 288 * 4);
    }
}

#[test]
fn frame_inspection_is_opt_in_deterministic_and_preserves_pixels() {
    let mut renderer = SubtitleRenderer::new(SAMPLE).unwrap();
    renderer.set_video_size(512, 288).unwrap();
    renderer.render_frame(67000).unwrap();
    let pixels = renderer.frame_data().to_vec();
    assert!(renderer.frame_debug().is_empty());
    renderer.set_debug_enabled(true);
    renderer.render_frame(67000).unwrap();
    assert_eq!(renderer.frame_data(), pixels);
    let events = renderer.frame_debug();
    assert_eq!(events.len(), 2);
    let event = &renderer.document().events[events[0].event_index];
    assert!(event.text.contains("Red"));
    assert!(!events[0].collision_eligible);
    assert_eq!(events[0].runs.len(), 3);
    assert_eq!(events[0].runs[0].fill, [255, 0, 0, 255]);
    assert_eq!(events[0].runs[1].fill, [0, 255, 0, 255]);
    assert_eq!(events[0].runs[2].fill, [0, 0, 255, 255]);
    let snapshot = events.to_vec();
    renderer.render_frame(287000).unwrap();
    assert_eq!(renderer.frame_debug().len(), 3);
    renderer.render_frame(67000).unwrap();
    assert_eq!(renderer.frame_debug(), snapshot);
    renderer.set_debug_enabled(false);
    assert!(renderer.frame_debug().is_empty());
    renderer.render_frame(67000).unwrap();
    assert!(renderer.frame_debug().is_empty());
    assert_eq!(renderer.frame_data(), pixels);
}

#[test]
fn frame_inspection_reports_collision_shifts_and_animated_run_values() {
    let source = "[Script Info]\nPlayResX: 384\nPlayResY: 216\n[Events]\n\
Dialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,First\n\
Dialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,Second\n\
Dialogue: 1,0:00:00.00,0:00:05.00,Default,,0,0,0,,{\\pos(100,100)\\fs20\\t(0,2000,\\fs40)}Animated";
    let mut renderer = SubtitleRenderer::new(source).unwrap();
    renderer.set_debug_enabled(true);
    renderer.render_frame(1000).unwrap();
    let events = renderer.frame_debug();
    assert_eq!(events.len(), 3);
    assert!(events[0].collision_eligible && events[1].collision_eligible);
    assert_eq!(events[0].collision_shift, 0.0);
    assert!(events[1].collision_shift < 0.0);
    assert!(events[1].layout_bounds[3] <= events[0].layout_bounds[1]);
    assert_eq!(events[2].runs[0].font_size, 30.0);
    assert!(!events[2].collision_eligible);
    renderer.set_video_size(768, 432).unwrap();
    assert!(renderer.frame_debug().is_empty());
    renderer.render_frame(1000).unwrap();
    assert_eq!(renderer.frame_debug()[2].runs[0].font_size, 30.0);
}

#[test]
fn frame_inspection_caps_events_runs_and_font_names_and_keeps_source_indices() {
    let mut source = String::from(
        "[Script Info]\nPlayResX: 32\nPlayResY: 18\n[Events]\n\
Comment: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,Not painted\n",
    );
    let long_font = "字".repeat(300);
    let runs = (0..65)
        .map(|i| format!("{{\\fs{}}}x", i % 2 + 1))
        .collect::<String>();
    source.push_str(&format!(
        "Dialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,{{\\pos(16,9)\\fn{long_font}}}{runs}\n"
    ));
    for _ in 0..256 {
        source.push_str("Dialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,{\\pos(16,9)\\fs1}x\n");
    }
    let mut renderer = SubtitleRenderer::new(&source).unwrap();
    renderer.set_debug_enabled(true);
    renderer.render_frame(1000).unwrap();
    let events = renderer.frame_debug();
    assert_eq!(events.len(), 256);
    assert_eq!(events[0].event_index, 1, "indices include comments");
    assert_eq!(events[0].run_count, 65);
    assert_eq!(events[0].runs.len(), 64);
    assert_eq!(events[0].runs[0].font_name.chars().count(), 257);
    assert!(events[0].runs[0].font_name.ends_with('…'));
}
