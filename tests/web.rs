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
    let info = doc.get_script_info();

    // Check that we got a valid object
    assert!(!info.is_undefined());
    assert!(!info.is_null());
}

#[wasm_bindgen_test]
fn test_get_styles() {
    let doc = subrass::api::AssDoc::new(TEST_ASS).unwrap();
    let styles = doc.get_styles();

    assert!(!styles.is_undefined());
    assert!(!styles.is_null());
}

#[wasm_bindgen_test]
fn test_get_events() {
    let doc = subrass::api::AssDoc::new(TEST_ASS).unwrap();
    let events = doc.get_events();

    assert!(!events.is_undefined());
    assert!(!events.is_null());
}

#[wasm_bindgen_test]
fn test_get_events_at_time() {
    let doc = subrass::api::AssDoc::new(TEST_ASS).unwrap();

    // At 2000ms, only the dialogue should be active
    let events = doc.get_events_at_time(2000.0);
    assert!(!events.is_undefined());

    // At 0ms, no events should be active
    let events = doc.get_events_at_time(0.0);
    assert!(!events.is_undefined());
}

#[wasm_bindgen_test]
fn test_get_dialogue_events() {
    let doc = subrass::api::AssDoc::new(TEST_ASS).unwrap();
    let dialogues = doc.get_dialogue_events();

    assert!(!dialogues.is_undefined());
    assert!(!dialogues.is_null());
}

#[wasm_bindgen_test]
fn test_get_comment_events() {
    let doc = subrass::api::AssDoc::new(TEST_ASS).unwrap();
    let comments = doc.get_comment_events();

    assert!(!comments.is_undefined());
    assert!(!comments.is_null());
}

#[wasm_bindgen_test]
fn test_find_style() {
    let doc = subrass::api::AssDoc::new(TEST_ASS).unwrap();

    let style = doc.find_style("Default");
    assert!(!style.is_undefined());
    assert!(!style.is_null());

    let style = doc.find_style("NonExistent");
    assert!(style.is_null());
}

#[wasm_bindgen_test]
fn test_get_default_style() {
    let doc = subrass::api::AssDoc::new(TEST_ASS).unwrap();
    let style = doc.get_default_style();

    assert!(!style.is_undefined());
    assert!(!style.is_null());
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

    // Should not panic
    let _events = doc.get_events();
}

#[wasm_bindgen_test]
fn test_sort_events_by_layer() {
    let mut doc = subrass::api::AssDoc::new(TEST_ASS).unwrap();
    doc.sort_events_by_layer();

    // Should not panic
    let _events = doc.get_events();
}

#[wasm_bindgen_test]
fn test_get_event() {
    let doc = subrass::api::AssDoc::new(TEST_ASS).unwrap();

    let event = doc.get_event(0);
    assert!(!event.is_undefined());
    assert!(!event.is_null());

    let event = doc.get_event(999);
    assert!(event.is_null());
}

#[wasm_bindgen_test]
fn test_get_style() {
    let doc = subrass::api::AssDoc::new(TEST_ASS).unwrap();

    let style = doc.get_style(0);
    assert!(!style.is_undefined());
    assert!(!style.is_null());

    let style = doc.get_style(999);
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
fn test_get_ass_summary() {
    let summary = subrass::api::get_ass_summary(TEST_ASS).unwrap();
    assert!(!summary.is_undefined());
    assert!(!summary.is_null());
}

#[wasm_bindgen_test]
fn test_ass_time_to_ms() {
    let ms = subrass::api::ass_time_to_ms("0:00:01.00").unwrap();
    assert_eq!(ms, 1000.0);
}

#[wasm_bindgen_test]
fn test_ms_to_ass_time() {
    let time = subrass::api::ms_to_ass_time(1000.0);
    assert_eq!(time, "0:00:01.00");
}

#[wasm_bindgen_test]
fn test_invalid_ass_content() {
    let result = subrass::api::AssDoc::new("This is not valid ASS");
    assert!(result.is_err());
}
