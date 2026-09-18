// Golden image tests (plan #42): render fixtures with subrass and
// compare byte-exact against stored raw-RGBA expectations.
//
// Run: cargo test --test golden
// Regenerate (never automatic): UPDATE_GOLDENS=1 cargo test --test golden
//
// Raw RGBA keeps the harness dependency-free; large transparent areas
// compress well in git. On mismatch the failure prints differing
// pixel count, max channel error, and mean error, and writes
// actual/expected/diff artifacts under target/golden-failures/.
use std::path::{Path, PathBuf};

use subrass::renderer::SubtitleRenderer;

const VIDEO_W: u32 = 256;
const VIDEO_H: u32 = 144;
// PlayRes is fixed by HEADER below (384x216).

const HEADER: &str = "[Script Info]\nTitle: golden\nScriptType: v4.00+\nPlayResX: 384\nPlayResY: 216\nWrapStyle: 0\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n";

const DEFAULT_STYLE: &str = "Style: Default,DejaVu Sans,36,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,0,0,0,0,100,100,0,0,1,2,1,2,10,10,10,1\n";
const BOX_STYLE: &str = "Style: Box,DejaVu Sans,36,&H00FFFFFF,&H000000FF,&H00000000,&H000000FF,0,0,0,0,100,100,0,0,3,6,0,5,10,10,10,1\n";
const MISSING_FONT_STYLE: &str = "Style: Missing,NoSuchFamilyXYZ,36,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,0,0,0,0,100,100,0,0,1,2,1,2,10,10,10,1\n";

const EVENTS_HEADER: &str =
    "\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n";

/// (name, event lines, time_ms). The Default/Box/Missing styles
/// ride along in every doc.
fn fixtures() -> Vec<(String, Vec<String>, u64)> {
    let ev = |layer: i32, style: &str, effect: &str, text: &str| {
        format!("Dialogue: {layer},0:00:00.00,0:00:05.00,{style},,0,0,0,{effect},{text}")
    };
    let one = |style: &str, text: &str| vec![ev(0, style, "", text)];
    vec![
        ("plain".to_string(), one("Default", "Hello golden"), 1000),
        (
            "alignment".to_string(),
            one("Default", "{\\an7}Top-left"),
            1000,
        ),
        (
            "position".to_string(),
            one("Default", "{\\pos(300,180)}Positioned"),
            1000,
        ),
        (
            "move".to_string(),
            one("Default", "{\\move(20,20,300,180)}Moving"),
            2500,
        ),
        (
            "fade".to_string(),
            one("Default", "{\\fad(500,500)}Fading"),
            250,
        ),
        (
            "transform".to_string(),
            one("Default", "{\\t(0,2000,\\fscx200\\fscy150)}Growing"),
            1000,
        ),
        (
            "karaoke".to_string(),
            one("Default", "{\\k50}Ka{\\k50}ra{\\k50}o{\\k50}ke"),
            1250,
        ),
        (
            "karaoke-ko".to_string(),
            one("Default", "{\\ko40}Out{\\ko40}line"),
            1300,
        ),
        (
            "karaoke-kf".to_string(),
            one("Default", "{\\kf100}Swe{\\kf100}ep"),
            1500,
        ),
        (
            "shear".to_string(),
            one("Default", "{\\fax0.3\\fay-0.2}Sheared"),
            1000,
        ),
        (
            "rotation".to_string(),
            one("Default", "{\\frz30\\frx20}Rotated"),
            1000,
        ),
        (
            "clip".to_string(),
            one("Default", "{\\clip(60,40,320,180)}Clipped"),
            1000,
        ),
        (
            "vector-clip".to_string(),
            one(
                "Default",
                "{\\clip(m 60 40 l 320 40 l 320 180 l 60 180)}VClip",
            ),
            1000,
        ),
        (
            "drawing".to_string(),
            one("Default", "{\\p1}m 0 0 l 60 0 l 60 60 l 0 60"),
            1000,
        ),
        (
            "wrap".to_string(),
            one(
                "Default",
                "A fairly long line that must wrap across the narrow frame\\Nplus break",
            ),
            1000,
        ),
        (
            "font-fallback".to_string(),
            one("Missing", "Hello \u{4F60}\u{597D}"),
            1000,
        ),
        (
            "border-shadow".to_string(),
            one("Default", "{\\bord4\\shad2\\blur1}Styled"),
            1000,
        ),
        ("opaque-box".to_string(), one("Box", "Boxed text"), 1000),
        (
            "reset".to_string(),
            one("Default", "{\\c&H0000FF&}Red{\\r}White"),
            1000,
        ),
        (
            "mixed-sizes".to_string(),
            one("Default", "{\\fs18}small{\\fs64}BIG"),
            1000,
        ),
        (
            "effect-banner".to_string(),
            vec![ev(0, "Default", "Banner;20", "Scrolling banner line")],
            1500,
        ),
        (
            "effect-scroll".to_string(),
            vec![ev(0, "Default", "Scroll up;20;196;40", "Up we go")],
            1500,
        ),
        (
            "layers".to_string(),
            vec![
                ev(0, "Default", "", "{\\an5\\c&HFF0000&}Back"),
                format!("Dialogue: 5,0:00:00.00,0:00:05.00,Default,,0,0,0,,{{\\an5\\fs20}}Front"),
            ],
            1000,
        ),
    ]
}

fn assemble(events: &[String]) -> String {
    let mut doc = String::from(HEADER);
    doc.push_str(DEFAULT_STYLE);
    doc.push_str(BOX_STYLE);
    doc.push_str(MISSING_FONT_STYLE);
    doc.push_str(EVENTS_HEADER);
    for line in events {
        doc.push_str(line);
        doc.push('\n');
    }
    doc
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

/// Compare rendered bytes to the stored golden. Returns stats text on
/// mismatch: differing pixel count, max channel error, mean error.
fn compare(name: &str, actual: &[u8], expected: &[u8]) -> Result<(), String> {
    if actual == expected {
        return Ok(());
    }
    if actual.len() != expected.len() {
        return Err(format!(
            "size mismatch: actual {} bytes, expected {} bytes",
            actual.len(),
            expected.len()
        ));
    }
    let mut diff_pixels = 0usize;
    let mut max_err = 0u8;
    let mut sum_err = 0u64;
    for (a, e) in actual.iter().zip(expected.iter()) {
        let d = a.abs_diff(*e);
        if d > 0 {
            max_err = max_err.max(d);
            sum_err += u64::from(d);
        }
    }
    for (a, e) in actual.chunks_exact(4).zip(expected.chunks_exact(4)) {
        if a != e {
            diff_pixels += 1;
        }
    }
    let mean_err = sum_err as f64 / actual.len() as f64;
    Err(format!(
        "golden mismatch for '{name}': {diff_pixels} differing pixels, \
         max channel error {max_err}, mean error {mean_err:.4}"
    ))
}

fn write_artifacts(name: &str, actual: &[u8], expected: &[u8]) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/golden-failures")
        .join(name);
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let _ = std::fs::write(dir.join("actual.rgba"), actual);
    let _ = std::fs::write(dir.join("expected.rgba"), expected);
    let diff: Vec<u8> = actual
        .iter()
        .zip(expected.iter())
        .map(|(a, e)| a.abs_diff(*e))
        .collect();
    let _ = std::fs::write(dir.join("diff.rgba"), &diff);
    eprintln!("golden artifacts written to {}", dir.display());
}

#[test]
fn golden_images_match() {
    let dir = golden_dir();
    let update = std::env::var("UPDATE_GOLDENS").is_ok();
    if update {
        std::fs::create_dir_all(&dir).expect("create tests/golden");
    }
    // Manifest shared with the libass reference generator so both
    // harnesses render identical inputs (time, dimensions).
    let mut manifest = String::from("{\"video\": [256, 144], \"fixtures\": {\n");
    let mut failures = Vec::new();
    for (name, events, time_ms) in fixtures() {
        let ass = assemble(&events);
        let mut renderer = SubtitleRenderer::new(&ass).expect("fixture parses");
        renderer
            .set_video_size(VIDEO_W, VIDEO_H)
            .expect("video size");
        renderer.render_frame(time_ms).expect("render");
        assert_eq!(renderer.frame_size(), (VIDEO_W, VIDEO_H));
        let actual = renderer.frame_data().to_vec();

        let rgba_path = dir.join(format!("{name}.rgba"));
        let ass_path = dir.join(format!("{name}.ass"));
        if update {
            std::fs::write(&rgba_path, &actual).expect("write golden");
            std::fs::write(&ass_path, &ass).expect("write fixture");
            manifest.push_str(&format!("  \"{name}\": {{\"time_ms\": {time_ms}}},\n"));
            continue;
        }
        let expected = match std::fs::read(&rgba_path) {
            Ok(bytes) => bytes,
            Err(_) => {
                failures.push(format!(
                    "{name}: missing golden (run with UPDATE_GOLDENS=1)"
                ));
                continue;
            }
        };
        if let Err(stats) = compare(&name, &actual, &expected) {
            write_artifacts(&name, &actual, &expected);
            failures.push(stats);
        }
    }
    if update {
        let manifest = manifest.trim_end_matches(",\n").to_string() + "\n}}\n";
        std::fs::write(dir.join("manifest.json"), &manifest).expect("write manifest");
    }
    assert!(
        failures.is_empty(),
        "{} golden failure(s):\n{}",
        failures.len(),
        failures.join("\n")
    );
}
