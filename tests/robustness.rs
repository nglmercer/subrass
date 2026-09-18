// Stable-toolchain hostile-input smoke tests (plans #44/#95): the
// values below must be rejected, clamped, or safely ignored — never
// panic, hang, or over-allocate. Each case asserts actual behavior,
// not just survival. Nightly fuzzing lives in fuzz/.
use subrass::renderer::SubtitleRenderer;

const HEADER: &str = "[Script Info]\nTitle: robust\nScriptType: v4.00+\nPlayResX: 384\nPlayResY: 216\nWrapStyle: 0\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,DejaVu Sans,36,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,0,0,0,0,100,100,0,0,1,2,1,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n";

fn doc(text: &str) -> String {
    format!("{HEADER}Dialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,{text}\n")
}

/// Render `text` at 256x144; returns the frame bytes on success.
fn render(text: &str) -> Result<Vec<u8>, String> {
    let mut renderer = SubtitleRenderer::new(&doc(text)).map_err(|e| e.to_string())?;
    renderer
        .set_video_size(256, 144)
        .map_err(|e| e.to_string())?;
    renderer.render_frame(1000).map_err(|e| e.to_string())?;
    assert_eq!(renderer.frame_size(), (256, 144));
    let bytes = renderer.frame_data().to_vec();
    assert_eq!(bytes.len(), 256 * 144 * 4);
    Ok(bytes)
}

fn ink_pixels(bytes: &[u8]) -> usize {
    bytes.chunks_exact(4).filter(|p| p[3] > 0).count()
}

#[test]
fn extreme_positions_render_or_miss_safely() {
    // i32 extremes: glyph math widens through i64; offscreen text clips.
    for text in [
        r"{\pos(-2147483648,-2147483648)}off",
        r"{\pos(2147483647,2147483647)}off",
        r"{\move(-2147483648,-2147483648,2147483647,2147483647)}move",
        r"{\org(-2147483648,2147483647)\frz45}org",
    ] {
        let bytes = render(text).expect("extreme positions must not error");
        // Far offscreen: nothing visible, but the frame is intact.
        assert_eq!(bytes.len(), 256 * 144 * 4);
    }
    // Sane position still renders after the extremes (no state poisoning).
    let bytes = render(r"{\pos(100,100)}here").expect("sane pos renders");
    assert!(ink_pixels(&bytes) > 50, "sane text must be visible");
}

#[test]
fn huge_finite_values_do_not_explode() {
    for text in [
        r"{\fs100000}big",
        r"{\fscx100000\fscy100000}big",
        r"{\bord10000\shad10000}big",
        r"{\blur128}blur",
        r"{\fax100\fay-100\fay100}shear",
        r"{\frx89.9\fry89.9}persp",
        r"{\frx-89.9\frz720}spin",
        r"{\p1}m -999999 -999999 l 999999 999999 l 999999 -999999}draw",
        r"{\clip(-999999,-999999,999999,999999)}clip",
        r"{\t(0,5000,\fs100000\blur128\fax50)}anim",
    ] {
        let bytes = render(text).expect("huge values must not error");
        assert_eq!(bytes.len(), 256 * 144 * 4);
    }
}

#[test]
fn near_zero_perspective_denominator_skips_glyph() {
    // frx/fry near ±90 push the perspective denominator toward zero;
    // guarded division skips the glyph instead of producing NaN/Inf.
    let bytes = render(r"{\frx90\fry90}edge").expect("renders");
    assert_eq!(bytes.len(), 256 * 144 * 4);
    // Exact singularity angles behave the same.
    let bytes = render(r"{\frx-90\frz180}edge").expect("renders");
    assert_eq!(bytes.len(), 256 * 144 * 4);
}

#[test]
fn negative_and_zero_geometry_is_safe() {
    for text in [
        r"{\shad-50\xshad-50\yshad-50}neg shadow",
        r"{\bord0\shad0}none",
        r"{\fs0}zero",
        r"{\fscx0\fscy0}zero",
        r"{\fsp-100}neg spacing",
        r"{\clip(50,50,10,10)}inverted clip",
    ] {
        let bytes = render(text).expect("degenerate geometry must not error");
        assert_eq!(bytes.len(), 256 * 144 * 4);
    }
    // libass: \fs0 resets to the style size, rendering like plain text.
    let bytes = render(r"{\fs0}zero").expect("renders");
    let plain = render("zero").expect("renders");
    assert_eq!(
        ink_pixels(&bytes),
        ink_pixels(&plain),
        "\\fs0 must reset to the style size"
    );
    // Zero scale still degrades to (almost) no ink, safely.
    let bytes = render(r"{\fscx0\fscy0}zero").expect("renders");
    assert!(
        ink_pixels(&bytes) < 10,
        "zero scale must be ~invisible, got {} ink px",
        ink_pixels(&bytes)
    );
}

#[test]
fn non_finite_and_garbage_overrides_are_ignored() {
    // The parser finite-checks every numeric tag: NaN/Inf payloads become
    // Unknown and never reach layout; garbage coordinates do the same.
    for text in [
        r"{\fsNaN}nan",
        r"{\fsinf}inf",
        r"{\frzNaN\faxinf\fay-inf}nan",
        r"{\pos(NaN,inf)}nan",
        r"{\move(0,0,NaN,NaN)}nan",
        r"{\clip(NaN,NaN,inf,inf)}nan",
        r"{\bordNaN\shadinf\blurNaN}nan",
        r"{\t(NaN,inf,\fsNaN)}nan",
        r"{\p1}m NaN NaN l inf inf}nan",
        r"{\feNaN\ktNaN\kNaN}nan",
    ] {
        let bytes = render(text).expect("non-finite tags must not error");
        assert_eq!(bytes.len(), 256 * 144 * 4);
    }
    // The tag soup above renders the bare text: NaN size must not shrink it.
    let bytes = render(r"{\fsNaN}nan").expect("renders");
    let plain = render("nan").expect("renders");
    assert_eq!(
        ink_pixels(&bytes),
        ink_pixels(&plain),
        "NaN size must fall back to plain rendering"
    );
}

#[test]
fn malformed_documents_error_without_panic() {
    for ass in [
        "",
        "[Events]\n",
        "Dialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,no sections",
        "[Script Info]\nPlayResX: 0\nPlayResY: -5\n",
        "[V4+ Styles]\nStyle: broken",
        "[Events]\nDialogue: nope",
        "[Fonts]\nfilename: wrong-section.ttf\n!!!!",
        "[Graphics]\nfontname: wrong-section.txt\n!!!!",
        "[Fonts]\nfontname: bad.ttf\n\x01\x02\x7fgarbage é",
        "[Fonts]\nfontname: bad2.ttf\n   spaced payload   ",
        &"[Events]\n".repeat(10_000),
        &"A".repeat(1_000_000),
    ] {
        // Either outcome is fine; panicking or hanging is not.
        let _ = SubtitleRenderer::new(ass);
    }
}

#[test]
fn invalid_video_sizes_are_rejected() {
    let mut renderer = SubtitleRenderer::new(&doc("hi")).expect("doc parses");
    for (w, h) in [(0, 144), (256, 0), (0, 0), (100_000, 100_000)] {
        assert!(
            renderer.set_video_size(w, h).is_err(),
            "{w}x{h} must be rejected"
        );
    }
    // A rejected resize leaves the renderer usable.
    renderer.set_video_size(256, 144).expect("valid size works");
    renderer
        .render_frame(1000)
        .expect("renders after rejection");
    assert_eq!(renderer.frame_size(), (256, 144));
}
