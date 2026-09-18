use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

const TEST_ASS: &str = r#"[Script Info]
Title: Test
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,48,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,-1,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:01.00,0:00:04.00,Default,John,0,0,0,,Hello World!
Comment: 0,0:00:00.00,0:00:30.00,Default,,0,0,0,,This is a comment
"#;

fn as_array(value: &JsValue) -> js_sys::Array {
    js_sys::Array::from(value)
}

fn field(value: &JsValue, name: &str) -> JsValue {
    js_sys::Reflect::get(value, &JsValue::from_str(name)).unwrap()
}

fn field_string(value: &JsValue, name: &str) -> String {
    field(value, name).as_string().unwrap()
}

fn field_f64(value: &JsValue, name: &str) -> f64 {
    field(value, name).as_f64().unwrap()
}

#[wasm_bindgen_test]
fn test_version() {
    let version = subrass::version();
    assert!(!version.is_empty());
}

#[wasm_bindgen_test]
fn test_is_loaded() {
    assert!(subrass::is_loaded());
}

#[wasm_bindgen_test]
fn test_parse_ass_document() {
    let doc = subrass::api::AssDoc::new(TEST_ASS).unwrap();

    assert_eq!(doc.get_event_count(), 2);
    assert_eq!(doc.get_style_count(), 1);
}

#[wasm_bindgen_test]
fn test_get_script_info() {
    let doc = subrass::api::AssDoc::new(TEST_ASS).unwrap();
    let info = doc.get_script_info().unwrap();

    assert!(!info.is_undefined());
    assert!(!info.is_null());
    assert_eq!(field_f64(&info, "play_res_x"), 1920.0);
    assert_eq!(field_f64(&info, "play_res_y"), 1080.0);
    assert_eq!(field_string(&info, "title"), "Test");
}

#[wasm_bindgen_test]
fn test_get_styles() {
    let doc = subrass::api::AssDoc::new(TEST_ASS).unwrap();
    let styles = as_array(&doc.get_styles().unwrap());

    assert_eq!(styles.length(), 1);
    assert_eq!(field_string(&styles.get(0), "name"), "Default");
    assert_eq!(field_string(&styles.get(0), "font_name"), "Arial");
}

#[wasm_bindgen_test]
fn test_get_events() {
    let doc = subrass::api::AssDoc::new(TEST_ASS).unwrap();
    let events = as_array(&doc.get_events().unwrap());

    assert_eq!(events.length(), 2);
    assert_eq!(field_string(&events.get(0), "text"), "Hello World!");
    assert_eq!(field_string(&events.get(1), "text"), "This is a comment");
}

#[wasm_bindgen_test]
fn test_get_events_at_time() {
    let doc = subrass::api::AssDoc::new(TEST_ASS).unwrap();

    // At 2000ms both the dialogue (1s-4s) and the comment (0s-30s) are
    // active: get_events_at_time does not filter comments.
    let events = as_array(&doc.get_events_at_time(2000.0).unwrap());
    assert_eq!(events.length(), 2);

    // At 0ms only the comment is active (dialogue starts at 1s).
    let events = as_array(&doc.get_events_at_time(0.0).unwrap());
    assert_eq!(events.length(), 1);
    assert_eq!(field_string(&events.get(0), "text"), "This is a comment");

    // At 31000ms nothing is active.
    let events = as_array(&doc.get_events_at_time(31000.0).unwrap());
    assert_eq!(events.length(), 0);
}

#[wasm_bindgen_test]
fn test_get_events_at_time_rejects_invalid() {
    let doc = subrass::api::AssDoc::new(TEST_ASS).unwrap();
    assert!(doc.get_events_at_time(f64::NAN).is_err());
    assert!(doc.get_events_at_time(f64::INFINITY).is_err());
    assert!(doc.get_events_at_time(-100.0).is_err());
}

#[wasm_bindgen_test]
fn test_get_dialogue_events() {
    let doc = subrass::api::AssDoc::new(TEST_ASS).unwrap();
    let dialogues = as_array(&doc.get_dialogue_events().unwrap());

    assert_eq!(dialogues.length(), 1);
    assert_eq!(field_string(&dialogues.get(0), "text"), "Hello World!");
}

#[wasm_bindgen_test]
fn test_get_comment_events() {
    let doc = subrass::api::AssDoc::new(TEST_ASS).unwrap();
    let comments = as_array(&doc.get_comment_events().unwrap());

    assert_eq!(comments.length(), 1);
    assert_eq!(field_string(&comments.get(0), "text"), "This is a comment");
}

#[wasm_bindgen_test]
fn test_find_style() {
    let doc = subrass::api::AssDoc::new(TEST_ASS).unwrap();

    let style = doc.find_style("Default").unwrap();
    assert!(!style.is_undefined());
    assert!(!style.is_null());
    assert_eq!(field_string(&style, "font_name"), "Arial");

    let style = doc.find_style("NonExistent").unwrap();
    assert!(style.is_null());
}

#[wasm_bindgen_test]
fn test_get_default_style() {
    let doc = subrass::api::AssDoc::new(TEST_ASS).unwrap();
    let style = doc.get_default_style().unwrap();

    assert!(!style.is_undefined());
    assert!(!style.is_null());
    assert_eq!(field_string(&style, "name"), "Default");
}

#[wasm_bindgen_test]
fn test_get_play_res() {
    let doc = subrass::api::AssDoc::new(TEST_ASS).unwrap();

    assert_eq!(doc.get_play_res_x(), 1920);
    assert_eq!(doc.get_play_res_y(), 1080);
}

#[wasm_bindgen_test]
fn test_sort_events_by_time() {
    let mut doc = subrass::api::AssDoc::new(TEST_ASS).unwrap();
    doc.sort_events_by_time();

    // Comment (starts 0s) sorts before dialogue (starts 1s).
    let first = doc.get_event(0).unwrap();
    assert_eq!(field_string(&first, "text"), "This is a comment");
}

#[wasm_bindgen_test]
fn test_sort_events_by_layer() {
    let mut doc = subrass::api::AssDoc::new(TEST_ASS).unwrap();
    doc.sort_events_by_layer();

    // Equal layers keep source order (stable sort).
    let events = as_array(&doc.get_events().unwrap());
    assert_eq!(field_string(&events.get(0), "text"), "Hello World!");
    assert_eq!(field_string(&events.get(1), "text"), "This is a comment");
}

#[wasm_bindgen_test]
fn test_get_event() {
    let doc = subrass::api::AssDoc::new(TEST_ASS).unwrap();

    let event = doc.get_event(0).unwrap();
    assert!(!event.is_undefined());
    assert!(!event.is_null());
    assert_eq!(field_string(&event, "text"), "Hello World!");

    let event = doc.get_event(999).unwrap();
    assert!(event.is_null());
}

#[wasm_bindgen_test]
fn test_get_style() {
    let doc = subrass::api::AssDoc::new(TEST_ASS).unwrap();

    let style = doc.get_style(0).unwrap();
    assert!(!style.is_undefined());
    assert!(!style.is_null());
    assert_eq!(field_string(&style, "name"), "Default");

    let style = doc.get_style(999).unwrap();
    assert!(style.is_null());
}

#[wasm_bindgen_test]
fn test_parse_ass_function() {
    let doc = subrass::api::parse_ass(TEST_ASS).unwrap();
    assert_eq!(doc.get_event_count(), 2);
}

#[wasm_bindgen_test]
fn test_validate_ass_function() {
    let result = subrass::api::validate_ass(TEST_ASS);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[wasm_bindgen_test]
fn test_validate_ass_invalid() {
    let result = subrass::api::validate_ass("This is not valid ASS content");
    assert!(result.is_err());
}

#[wasm_bindgen_test]
fn test_validate_ass_rejects_malformed_values() {
    // A bad timestamp is a validation failure, not a silent default.
    let bad = TEST_ASS.replace("0:00:01.00", "0:99:99.99");
    assert!(subrass::api::validate_ass(&bad).is_err());
}

#[wasm_bindgen_test]
fn test_get_ass_summary() {
    let summary = subrass::api::get_ass_summary(TEST_ASS).unwrap();
    assert!(!summary.is_undefined());
    assert!(!summary.is_null());
    assert_eq!(field_f64(&summary, "style_count"), 1.0);
    assert_eq!(field_f64(&summary, "event_count"), 2.0);
    assert_eq!(field_f64(&summary, "dialogue_count"), 1.0);
    assert_eq!(field_f64(&summary, "comment_count"), 1.0);
}

#[wasm_bindgen_test]
fn test_ass_time_to_ms() {
    let ms = subrass::api::ass_time_to_ms("0:00:01.00").unwrap();
    assert_eq!(ms, 1000.0);
    assert!(subrass::api::ass_time_to_ms("0:99:99.99").is_err());
}

#[wasm_bindgen_test]
fn test_ms_to_ass_time() {
    let time = subrass::api::ms_to_ass_time(1000.0).unwrap();
    assert_eq!(time, "0:00:01.00");
    assert!(subrass::api::ms_to_ass_time(f64::NAN).is_err());
    assert!(subrass::api::ms_to_ass_time(-5.0).is_err());
}

#[wasm_bindgen_test]
fn test_invalid_ass_content() {
    let result = subrass::api::AssDoc::new("This is not valid ASS");
    assert!(result.is_err());
}

#[wasm_bindgen_test]
fn test_renderer_frame_size_and_data() {
    let mut renderer = subrass::api::SubtitleRenderer::new(TEST_ASS).unwrap();
    renderer.set_video_size(320, 180).unwrap();
    renderer.render_frame(2000.0).unwrap();

    let size = renderer.get_frame_size();
    assert_eq!(size, vec![320, 180]);
    let data = renderer.get_frame_data();
    assert_eq!(data.len(), 320 * 180 * 4);
    // Something was actually rendered (not all transparent).
    assert!(data.chunks_exact(4).any(|p| p[3] > 0));
}

#[wasm_bindgen_test]
fn test_renderer_rejects_invalid_inputs() {
    let mut renderer = subrass::api::SubtitleRenderer::new(TEST_ASS).unwrap();
    assert!(renderer.set_video_size(0, 180).is_err());
    assert!(renderer.resize(u32::MAX, u32::MAX).is_err());
    assert!(renderer.render_frame(f64::NAN).is_err());
    assert!(renderer.render_frame(-1.0).is_err());
}

// Plan #78: stronger JS-visible behavior coverage.

const TEST_ASS_WITH_ATTACHMENTS: &str = r#"[Script Info]
Title: Test
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,48,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,-1,0,0,0,100,100,0,0,1,2,1,2,10,10,40,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:01.00,0:00:04.00,Default,John,0,0,0,,Hello World!

[Fonts]
fontname: Test.ttf
15*$

[Graphics]
filename: logo.bmp
15*$
"#;

#[wasm_bindgen_test]
fn test_renderer_rejects_invalid_times() {
    let mut renderer = subrass::api::SubtitleRenderer::new(TEST_ASS).unwrap();
    renderer.set_video_size(64, 64).unwrap();
    assert!(renderer.render_frame(f64::INFINITY).is_err());
    assert!(renderer.render_frame(f64::NEG_INFINITY).is_err());
    // Beyond Number.MAX_SAFE_INTEGER.
    assert!(renderer.render_frame(9_007_199_254_740_992.0).is_err());
    assert!(renderer.render_frame(-0.5).is_err());
    // Boundary values render fine.
    assert!(renderer.render_frame(0.0).is_ok());
    assert!(renderer.render_frame(9_007_199_254_740_991.0).is_ok());
    // Times outside any event render an empty (fully transparent) frame.
    renderer.render_frame(60_000.0).unwrap();
    assert!(renderer.get_frame_data().chunks_exact(4).all(|p| p[3] == 0));
}

#[wasm_bindgen_test]
fn test_renderer_rejects_invalid_dimensions() {
    let mut renderer = subrass::api::SubtitleRenderer::new(TEST_ASS).unwrap();
    renderer.set_video_size(64, 64).unwrap();
    assert!(renderer.set_video_size(0, 0).is_err());
    assert!(renderer.set_video_size(64, 0).is_err());
    assert!(renderer.resize(0, 64).is_err());
    assert!(renderer.set_video_size(u32::MAX, u32::MAX).is_err());
    // Failed resizes leave the previous size untouched.
    assert_eq!(renderer.get_frame_size(), vec![64, 64]);
    assert_eq!(renderer.get_frame_data().len(), 64 * 64 * 4);
}

#[wasm_bindgen_test]
fn test_resize_keeps_frame_consistent() {
    let mut renderer = subrass::api::SubtitleRenderer::new(TEST_ASS).unwrap();
    renderer.set_video_size(320, 180).unwrap();
    renderer.render_frame(2000.0).unwrap();
    assert_eq!(renderer.get_frame_data().len(), 320 * 180 * 4);
    renderer.resize(160, 90).unwrap();
    assert_eq!(renderer.get_frame_size(), vec![160, 90]);
    assert_eq!(renderer.get_frame_data().len(), 160 * 90 * 4);
    // Still renders after resize.
    renderer.render_frame(2000.0).unwrap();
    assert!(renderer.get_frame_data().chunks_exact(4).any(|p| p[3] > 0));
}

#[wasm_bindgen_test]
fn test_attachment_access() {
    let doc = subrass::api::AssDoc::new(TEST_ASS_WITH_ATTACHMENTS).unwrap();
    assert_eq!(doc.get_attachment_count(), 2);
    assert_eq!(doc.get_attachment_name(0).as_deref(), Some("Test.ttf"));
    assert_eq!(doc.get_attachment_kind(0).as_deref(), Some("font"));
    assert_eq!(
        doc.get_attachment_data(0).as_deref(),
        Some(b"ABC".as_slice())
    );
    assert_eq!(doc.get_attachment_name(1).as_deref(), Some("logo.bmp"));
    assert_eq!(doc.get_attachment_kind(1).as_deref(), Some("graphic"));
    assert_eq!(
        doc.get_attachment_data(1).as_deref(),
        Some(b"ABC".as_slice())
    );
    // Out-of-range indices are None (undefined in JS), never a throw.
    assert_eq!(doc.get_attachment_name(2), None);
    assert_eq!(doc.get_attachment_kind(2), None);
    assert_eq!(doc.get_attachment_data(2), None);
    assert_eq!(doc.get_attachment_name(usize::MAX), None);
}

#[wasm_bindgen_test]
fn test_load_font_from_bytes() {
    let mut renderer = subrass::api::SubtitleRenderer::new(TEST_ASS).unwrap();
    let before = renderer.get_font_count();
    assert!(before > 0);
    // Garbage is rejected.
    assert!(renderer.load_font("junk", b"not a font").is_err());
    assert!(renderer.load_font("empty", b"").is_err());
    assert_eq!(renderer.get_font_count(), before);
    // A real embedded font registers and renders.
    let dejavu = include_bytes!("../fonts/DejaVuSans.ttf");
    renderer.load_font("DejaVu", dejavu).unwrap();
    assert_eq!(renderer.get_font_count(), before + 1);
    renderer.set_video_size(320, 180).unwrap();
    renderer.render_frame(2000.0).unwrap();
    assert!(renderer.get_frame_data().chunks_exact(4).any(|p| p[3] > 0));
}

#[wasm_bindgen_test]
fn test_renderer_constructor_rejects_garbage() {
    assert!(subrass::api::SubtitleRenderer::new("junk").is_err());
    assert!(subrass::api::SubtitleRenderer::new("[Bogus]\nfoo").is_err());
    // Empty content parses to an empty document (valid, zero events).
    let empty = subrass::api::SubtitleRenderer::new("").unwrap();
    assert_eq!(empty.get_event_count(), 0);
    assert_eq!(empty.get_style_count(), 0);
}

#[wasm_bindgen_test]
fn test_missing_lookups_return_null() {
    let doc = subrass::api::AssDoc::new(TEST_ASS).unwrap();
    assert!(doc.find_style("NoSuchStyle").unwrap().is_null());
    assert!(doc.get_event(usize::MAX).unwrap().is_null());
    assert!(doc.get_style(usize::MAX).unwrap().is_null());
    assert!(doc.get_events_at_time(f64::INFINITY).is_err());
}

#[wasm_bindgen_test]
fn test_event_serialization_shape() {
    // Serialization round-trips the fields JS consumers need.
    let doc = subrass::api::AssDoc::new(TEST_ASS).unwrap();
    let events = as_array(&doc.get_events().unwrap());
    assert_eq!(events.length(), 2);
    let first = events.get(0);
    assert_eq!(field_string(&first, "text"), "Hello World!");
    assert_eq!(field_string(&first, "style"), "Default");
    let event = doc.get_event(0).unwrap();
    assert_eq!(field_string(&event, "text"), "Hello World!");
}
