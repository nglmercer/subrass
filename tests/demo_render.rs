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
