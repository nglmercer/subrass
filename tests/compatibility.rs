use subrass::types::override_tag::{parse_text_segments_with_wrap, OverrideTag};
use subrass::{parser::AssDocument, renderer::SubtitleRenderer};

fn clean(text: &str, wrap: i32) -> String {
    parse_text_segments_with_wrap(text, wrap)
        .iter()
        .map(|s| s.text.as_str())
        .collect()
}

#[test]
fn exact_breaks_escapes_and_malformed_text() {
    for wrap in 0..=3 {
        for (source, expected) in [
            (r"\N\NText\N\N", "\n\nText\n\n"),
            (r"{\b1}\N\NText", "\n\nText"),
            (r"\{literal\}\{\pos(1,2)\}", "{literal}{\\pos(1,2)}"),
            ("Text\\", "Text\\"),
            ("A\tB", "A B"),
            (r"a{\b1", "a"),
            (r"\\N", "\\\n"),
            (r"\\{literal\}", "\\{literal}"),
        ] {
            assert_eq!(clean(source, wrap), expected, "{source:?}, q{wrap}");
        }
        assert_eq!(
            clean(r"\n\nA\n", wrap),
            if wrap == 2 { "\n\nA\n" } else { "  A " }
        );
    }
    assert!(OverrideTag::parse_from_text(r"\{\pos(1,2)\}").is_empty());
    assert!(matches!(
        OverrideTag::parse_from_text(r"\{literal\}{\pos(1,2)}").as_slice(),
        [OverrideTag::Position(1.0, 2.0)]
    ));
}

#[test]
fn playres_presence_and_libass_inference() {
    for (fields, expected, present) in [
        ("", (384, 288), (false, false)),
        ("PlayResX: 640\n", (640, 480), (true, false)),
        ("PlayResX: 1280\n", (1280, 1024), (true, false)),
        ("PlayResY: 1024\n", (1280, 1024), (false, true)),
        ("PlayResY: 720\n", (960, 720), (false, true)),
        ("PlayResX: 1\n", (1, 1), (true, false)),
        ("PlayResX: 385\n", (385, 288), (true, false)),
        (
            "PlayResX: 1920\nPlayResY: 1080\n",
            (1920, 1080),
            (true, true),
        ),
    ] {
        let script = format!("[Script Info]\n{fields}");
        for document in [
            AssDocument::parse(&script).unwrap(),
            AssDocument::parse_bytes(script.as_bytes()).unwrap(),
        ] {
            let info = document.script_info;
            assert_eq!((info.play_res_x, info.play_res_y), expected, "{fields}");
            assert_eq!((info.play_res_x_present, info.play_res_y_present), present);
        }
    }
    for fields in [
        "PlayResX: 0",
        "PlayResY: -1",
        "PlayResX: nope",
        "PlayResY: 4294967296",
    ] {
        assert!(AssDocument::parse(&format!("[Script Info]\n{fields}")).is_err());
    }
}

#[test]
fn utf16_boms_preserve_unicode_and_reject_malformed_units() {
    let text = "[Script Info]\r\nTitle: 日本語🦀\r\nPlayResX: 384\r\n[Events]\r\nDialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,\\{日本語🦀\\}\\N";
    let expected = AssDocument::parse(text).unwrap();
    for little in [true, false] {
        let mut bytes = if little {
            vec![0xff, 0xfe]
        } else {
            vec![0xfe, 0xff]
        };
        for unit in text.encode_utf16() {
            bytes.extend(if little {
                unit.to_le_bytes()
            } else {
                unit.to_be_bytes()
            });
        }
        let parsed = AssDocument::parse_bytes(&bytes).unwrap();
        assert_eq!(parsed.script_info.title, expected.script_info.title);
        assert_eq!(parsed.events[0].text, expected.events[0].text);
        bytes.pop();
        assert!(AssDocument::parse_bytes(&bytes)
            .unwrap_err()
            .to_string()
            .contains("UTF-16"));
    }
    for bad in [
        &[0xff, 0xfe, 0x00, 0xd8][..],
        &[0xfe, 0xff, 0xdc, 0x00],
        &[0xff, 0xfe, 0x61],
    ] {
        assert!(AssDocument::parse_bytes(bad).is_err());
    }
    let mut utf8 = vec![0xef, 0xbb, 0xbf];
    utf8.extend(text.as_bytes());
    assert_eq!(
        AssDocument::parse_bytes(&utf8).unwrap().events[0].text,
        expected.events[0].text
    );
}

fn sample(script: &str, time: u64) -> Vec<u8> {
    let mut renderer = SubtitleRenderer::new(script).unwrap();
    renderer.set_video_size(384, 216).unwrap();
    renderer.render_frame(time).unwrap();
    renderer.frame_data().to_vec()
}

fn color_bounds(image: &[u8], channel: usize) -> (usize, usize) {
    let rows: Vec<_> = image
        .chunks_exact(4)
        .enumerate()
        .filter_map(|(i, p)| {
            (p[3] > 128
                && p[channel] > 200
                && p[(channel + 1) % 3] < 64
                && p[(channel + 2) % 3] < 64)
                .then_some(i / 384)
        })
        .collect();
    (
        *rows.iter().min().expect("visible color"),
        *rows.iter().max().unwrap(),
    )
}

#[test]
fn eligible_collisions_separate_by_alignment_and_layer() {
    for align in [2, 5, 8] {
        let script =
            std::fs::read_to_string(format!("tests/compatibility/collision-{align}.ass")).unwrap();
        let frame = sample(&script, 1000);
        let (red, green, blue) = (
            color_bounds(&frame, 0),
            color_bounds(&frame, 1),
            color_bounds(&frame, 2),
        );
        if align == 2 {
            assert!(blue.1 < green.0 && green.1 < red.0);
        } else {
            assert!(red.1 < green.0 && green.1 < blue.0);
        }
    }
    let script = include_str!("compatibility/collision-layer.ass");
    let frame = sample(script, 1000);
    let (red, green) = (color_bounds(&frame, 0), color_bounds(&frame, 1));
    assert!(
        red.1 >= green.0 && green.1 >= red.0,
        "different layers may overlap"
    );
}

#[test]
fn collision_frames_are_independent_of_playback_and_seeking() {
    let script = include_str!("compatibility/collision-staggered.ass");
    let mut renderer = SubtitleRenderer::new(script).unwrap();
    renderer.set_video_size(384, 216).unwrap();
    for time in [0, 999, 1000, 2000, 4000, 999, 2000, 1000, 5000, 1000] {
        renderer.render_frame(time).unwrap();
        assert_eq!(
            renderer.frame_data(),
            sample(script, time),
            "seek to {time}"
        );
    }
}

#[test]
fn drawings_animate_and_non_square_scaling_changes_both_extents() {
    let script = include_str!("compatibility/drawing-animated.ass");
    assert_eq!(sample(script, 999), sample(script, 1000));
    assert_ne!(sample(script, 1000), sample(script, 1500));
    assert_ne!(sample(script, 1500), sample(script, 2000));
    assert_eq!(sample(script, 2000), sample(script, 2001));
    let scaled = include_str!("compatibility/drawing-scale.ass");
    let frame = sample(scaled, 1000);
    let pixels: Vec<_> = frame
        .chunks_exact(4)
        .enumerate()
        .filter(|(_, p)| p[3] != 0)
        .map(|(i, _)| (i % 384, i / 384))
        .collect();
    assert_eq!(
        pixels.iter().map(|p| p.0).max().unwrap() - pixels.iter().map(|p| p.0).min().unwrap() + 1,
        144
    );
    assert_eq!(
        pixels.iter().map(|p| p.1).max().unwrap() - pixels.iter().map(|p| p.1).min().unwrap() + 1,
        12
    );
}

#[test]
fn blur_runs_reset_without_changing_neighbouring_ink() {
    let script = include_str!("compatibility/blur-inline.ass");
    let frame = sample(script, 1000);
    assert_eq!(
        frame,
        sample(&script.replace("{\\r}", "{\\blur0\\be0}"), 1000)
    );
    assert_ne!(
        frame,
        sample(&script.replace("\\blur2", "\\be2\\blur0"), 1000)
    );
}

#[test]
fn fully_faded_events_reserve_collision_space() {
    let script = include_str!("compatibility/collision-fade.ass");
    assert_eq!(
        color_bounds(&sample(script, 0), 1),
        color_bounds(&sample(script, 1000), 1)
    );
}

#[test]
fn style_johab_is_explicitly_diagnosed() {
    let script =
        include_str!("compatibility/collision-fade.ass").replace(",10,10,10,1", ",10,10,10,130");
    let renderer = SubtitleRenderer::new(&script).unwrap();
    assert!(renderer
        .warnings()
        .iter()
        .any(|w| w.contains("Encoding 130") && w.contains("Johab")));
}
