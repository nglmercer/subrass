// libass reference comparison tests (plan #41).
//
// Compares subrass output against frames rendered by a pinned libass
// (via ffmpeg's `ass` filter; see tests/reference/provenance.json).
// References live in tests/reference/*.rgba (raw RGBA over black);
// fixtures mirror tests/golden (same .ass inputs and manifest times).
// YCbCr fixtures are retained as explicitly host/video-converted artifacts;
// raw subtitle RGB semantics are tested directly without FFmpeg below.
//
// Different rasterizers never match byte-exact, so this gates on
// structural similarity instead: ink bounding boxes must overlap
// strongly, mean channel error must be small, and hard-mismatch
// pixels must be rare. Known divergences (legacy effects, which
// libass ignores; fontconfig fallback, which is environment-specific)
// are measured and reported but not gated.
//
// Regenerate references (never automatic):
//   $env:FFMPEG = '<ffmpeg with libass>'; ./tests/reference/gen_references.ps1
use std::path::{Path, PathBuf};

use subrass::renderer::SubtitleRenderer;

/// Fixtures measured but never gated, with the reason.
const KNOWN_DIVERGENT: &[(&str, &str)] = &[
    (
        "effect-banner",
        "libass ignores legacy Banner effects (renders static)",
    ),
    (
        "effect-scroll",
        "libass ignores legacy Scroll effects (renders static)",
    ),
    (
        "font-fallback",
        "fontconfig fallback is environment-dependent",
    ),
    (
        "wrap-cjk",
        "default libass (no unibreak) breaks at ASCII spaces only and overflows CJK",
    ),
    (
        "wrap-cjk-punct",
        "default libass (no unibreak) breaks at ASCII spaces only and overflows CJK",
    ),
    (
        "wrap-zwsp",
        "default libass (no unibreak) never breaks at U+200B and overflows",
    ),
    (
        "wrap-mixed",
        "default libass (no unibreak) breaks at ASCII spaces only and overflows CJK",
    ),
];

/// Fixtures whose libass reference frames have not been generated yet.
/// Missing `.rgba` files for these names are reported as pending, not
/// failures; once `gen_references.ps1` produces them, they gate like
/// all others. Never add an existing divergence here to silence it —
/// that list is `KNOWN_DIVERGENT` above. Currently empty: every
/// manifest fixture has a generated libass frame.
const PENDING_REFERENCES: &[&str] = &[];

/// These frames include an FFmpeg video-colorspace stage and therefore cannot
/// prove libass's raw `ASS_Image.color` semantics. They remain integrity-gated
/// artifacts, but are excluded from the raw renderer comparison.
const HOST_CONVERTED_FIXTURES: &[&str] = &[
    "ycbcr-none",
    "ycbcr-tv601",
    "ycbcr-tv709",
    "ycbcr-pc601",
    "ycbcr-pc709",
];

/// Gate thresholds (see CONFORMANCE.md): bbox IoU over full-res ink
/// masks, ink-count ratio bounds, and block-averaged intensity error.
/// Intensity is compared on 4x4 block averages so 1px rasterizer
/// placement/AA differences (unhinted ab_glyph vs hinted FreeType)
/// mostly cancel while color, alpha, and coverage bugs still fail.
const MIN_IOU: f64 = 0.70;
const MIN_INK_RATIO: f64 = 0.5;
const MAX_INK_RATIO: f64 = 2.0;
const MAX_MEAN_ERR: f64 = 25.0;
const MAX_HARD_FRAC: f64 = 0.15;
/// Block size (square) for intensity comparison.
const BLOCK: u32 = 4;
/// Block-average channel error above this counts as a hard mismatch.
const HARD_ERR: f64 = 48.0;

fn manifest_times() -> Vec<(String, u64)> {
    // Minimal JSON read: {"video": [...], "fixtures": {"name": {"time_ms": N}, ...}}.
    // For each "time_ms": N, the fixture name is the nearest quoted
    // string before it.
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/manifest.json"),
    )
    .expect("golden manifest (run UPDATE_GOLDENS=1 golden first)");
    let mut out = Vec::new();
    let mut rest = text.as_str();
    while let Some(tpos) = rest.find("\"time_ms\"") {
        let before = &rest[..tpos];
        let name = before.rfind('"').and_then(|end| {
            before[..end]
                .rfind('"')
                .map(|start| before[start + 1..end].to_string())
        });
        let num: String = rest[tpos..]
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if let (Some(name), Ok(ms)) = (name, num.parse::<u64>()) {
            out.push((name, ms));
        }
        rest = &rest[tpos + 9..];
    }
    out.sort();
    out.dedup_by(|a, b| a.0 == b.0);
    out
}

fn reference_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/reference")
}

/// Flatten RGBA over black; returns RGB triples.
fn flatten(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks_exact(4)
        .flat_map(|p| {
            let a = f64::from(p[3]) / 255.0;
            [
                (f64::from(p[0]) * a).round() as u8,
                (f64::from(p[1]) * a).round() as u8,
                (f64::from(p[2]) * a).round() as u8,
            ]
        })
        .collect()
}

/// Expand one reference channel from the generator's limited range
/// back to full range. Every stored reference frame spans 16..235
/// (opaque black renders as 16, opaque white as 235) because the
/// ffmpeg `ass` filter blends in limited-range YUV; see
/// CONFORMANCE.md. Inverting that map before comparison removes a
/// ~20-level systematic offset that would otherwise dominate the
/// error metrics. Clamps: dimmer-than-black fringes map to 0.
fn normalize_ref(v: u8) -> u8 {
    ((f64::from(v) - 16.0) * 255.0 / 219.0)
        .clamp(0.0, 255.0)
        .round() as u8
}

/// Ink bounding box (min_x, min_y, max_x, max_y) over a pixel mask, if any.
fn mask_bbox(mask: &[bool], w: u32) -> Option<(u32, u32, u32, u32)> {
    let (mut x0, mut y0) = (u32::MAX, u32::MAX);
    let (mut x1, mut y1) = (0u32, 0u32);
    let h = mask.len() / w as usize;
    for (i, ink) in mask.iter().enumerate() {
        if *ink {
            let x = (i % w as usize) as u32;
            let y = (i / w as usize) as u32;
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    if x0 > x1 || y0 as usize >= h {
        None
    } else {
        Some((x0, y0, x1, y1))
    }
}

fn iou(a: (u32, u32, u32, u32), b: (u32, u32, u32, u32)) -> f64 {
    let (ix0, iy0) = (a.0.max(b.0), a.1.max(b.1));
    let (ix1, iy1) = (a.2.min(b.2), a.3.min(b.3));
    if ix1 < ix0 || iy1 < iy0 {
        return 0.0;
    }
    let inter = f64::from(ix1 - ix0 + 1) * f64::from(iy1 - iy0 + 1);
    let area_a = f64::from(a.2 - a.0 + 1) * f64::from(a.3 - a.1 + 1);
    let area_b = f64::from(b.2 - b.0 + 1) * f64::from(b.3 - b.1 + 1);
    inter / (area_a + area_b - inter)
}

struct Stats {
    ours_ink: usize,
    ref_ink: usize,
    iou: f64,
    mean_err: f64,
    hard_frac: f64,
}

/// Ink mask for a libass reference frame. The background level is
/// the frame's most common max-channel value (0 for every frame the
/// current generator emits), and ink is anything that differs from
/// it. A fixed `> 0` test would silently classify an entire frame as
/// ink if a generator ever emitted limited-range black (level 16) as
/// the background; the relative mask stays correct there (a level-16
/// black box on a level-0 background still counts, while on a
/// level-16 background it would be invisible — correctly not ink).
/// See `blank_libass_frame_has_zero_ink`.
fn reference_ink_mask(ref_rgba: &[u8]) -> Vec<bool> {
    let mut hist = [0u32; 256];
    for p in ref_rgba.chunks_exact(4) {
        hist[p[0].max(p[1]).max(p[2]) as usize] += 1;
    }
    let bg = hist
        .iter()
        .enumerate()
        .max_by_key(|(_, &n)| n)
        .map(|(v, _)| v)
        .unwrap_or(0);
    ref_rgba
        .chunks_exact(4)
        .map(|p| p[0].max(p[1]).max(p[2]) as usize != bg)
        .collect()
}

fn compare(ours_rgba: &[u8], ref_rgba: &[u8], w: u32, normalize_reference: bool) -> Option<Stats> {
    // Ink masks: our side keys on alpha (a black outline, shadow, or
    // opaque box is real ink even though its RGB is zero); the
    // reference frame is opaque, so it keys on deviation from its
    // background level (this catches its range-shifted black box at
    // level 16 without ever mistaking background for ink).
    let ours_mask: Vec<bool> = ours_rgba.chunks_exact(4).map(|p| p[3] > 0).collect();
    let ref_mask: Vec<bool> = reference_ink_mask(ref_rgba);
    let a_box = mask_bbox(&ours_mask, w)?;
    let b_box = mask_bbox(&ref_mask, w)?;
    let ours_ink = ours_mask.iter().filter(|m| **m).count();
    let ref_ink = ref_mask.iter().filter(|m| **m).count();
    // Intensities: ours flattened over black, reference expanded back
    // to full range. Error over the union box (background agreement
    // is uninformative).
    let ours_rgb = flatten(ours_rgba);
    let ref_rgb: Vec<u8> = ref_rgba
        .chunks_exact(4)
        .flat_map(|p| {
            [
                if normalize_reference {
                    normalize_ref(p[0])
                } else {
                    p[0]
                },
                if normalize_reference {
                    normalize_ref(p[1])
                } else {
                    p[1]
                },
                if normalize_reference {
                    normalize_ref(p[2])
                } else {
                    p[2]
                },
            ]
        })
        .collect();
    let (ux0, uy0) = (a_box.0.min(b_box.0), a_box.1.min(b_box.1));
    let (ux1, uy1) = (a_box.2.max(b_box.2), a_box.3.max(b_box.3));
    // Block-averaged intensity error over the union box: each BLOCK x
    // BLOCK cell contributes its per-channel mean on each side.
    // Background agreement is uninformative, so cells where both sides
    // average to zero are skipped.
    let h = ours_rgb.len() / (w as usize * 3);
    let mut sum = 0.0f64;
    let mut hard = 0u64;
    let mut n = 0u64;
    let mut by = uy0;
    while by <= uy1 {
        let mut bx = ux0;
        while bx <= ux1 {
            let x1 = (bx + BLOCK - 1).min(ux1).min(w - 1);
            let y1 = (by + BLOCK - 1).min(uy1).min(h as u32 - 1);
            let mut acc_o = [0u64; 3];
            let mut acc_r = [0u64; 3];
            let mut cnt = 0u64;
            for y in by..=y1 {
                for x in bx..=x1 {
                    let i = (y * w + x) as usize * 3;
                    for c in 0..3 {
                        acc_o[c] += u64::from(ours_rgb[i + c]);
                        acc_r[c] += u64::from(ref_rgb[i + c]);
                    }
                    cnt += 1;
                }
            }
            if cnt > 0 {
                for c in 0..3 {
                    let mo = acc_o[c] as f64 / cnt as f64;
                    let mr = acc_r[c] as f64 / cnt as f64;
                    if mo == 0.0 && mr == 0.0 {
                        continue;
                    }
                    let d = (mo - mr).abs();
                    sum += d;
                    n += 1;
                    if d >= HARD_ERR {
                        hard += 1;
                    }
                }
            }
            bx += BLOCK;
        }
        by += BLOCK;
    }
    Some(Stats {
        ours_ink,
        ref_ink,
        iou: iou(a_box, b_box),
        mean_err: sum / n.max(1) as f64,
        hard_frac: hard as f64 / n.max(1) as f64,
    })
}

#[test]
fn libass_reference_comparison() {
    let golden_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let ref_dir = reference_dir();
    if !ref_dir.join("provenance.json").exists() {
        eprintln!("skipping: no tests/reference/provenance.json (run gen_references.ps1)");
        return;
    }
    let mut failures = Vec::new();
    let mut report = Vec::new();
    for (name, time_ms) in manifest_times() {
        if HOST_CONVERTED_FIXTURES.contains(&name.as_str()) {
            report.push(format!(
                "{name:14} [host/video conversion artifact; raw RGBA tested directly]"
            ));
            continue;
        }
        let ass = std::fs::read_to_string(golden_dir.join(format!("{name}.ass")))
            .expect("golden fixture");
        let mut renderer = SubtitleRenderer::new(&ass).expect("fixture parses");
        if name == "indic" {
            renderer
                .load_font(
                    "NotoSansDevanagari.ttf",
                    include_bytes!("../fonts/NotoSansDevanagari.ttf"),
                )
                .expect("load committed Indic fixture font");
        }
        if name == "font-collection" {
            renderer
                .load_font(
                    "SubrassTestCollection.ttc",
                    include_bytes!("../fonts/SubrassTestCollection.ttc"),
                )
                .expect("load committed collection fixture font");
            assert_eq!(renderer.font_count(), 4, "fallback plus three TTC faces");
        }
        renderer.set_video_size(256, 144).expect("video size");
        renderer.render_frame(time_ms).expect("render");
        let ours = renderer.frame_data().to_vec();
        let ref_bytes = match std::fs::read(ref_dir.join(format!("{name}.rgba"))) {
            Ok(b) => b,
            Err(_) => {
                if PENDING_REFERENCES.contains(&name.as_str()) {
                    // Strict release mode: pending references fail the
                    // gate instead of skipping it.
                    if std::env::var("SUBRASS_STRICT_REFERENCES").is_ok() {
                        failures.push(format!("{name}: pending reference (strict mode)"));
                        continue;
                    }
                    report.push(format!(
                        "{name:14} [pending: no libass frame yet; run gen_references.ps1]"
                    ));
                    continue;
                }
                failures.push(format!("{name}: missing reference frame"));
                continue;
            }
        };
        // The legacy corpus omits `YCbCr Matrix`, and FFmpeg/libass emits
        // its subtitle colors through TV range; normalize that historical
        // corpus back to full RGB. YCbCr host-conversion artifacts are
        // excluded above and never claim raw subtitle-color parity.
        let Some(stats) = compare(&ours, &ref_bytes, 256, true) else {
            failures.push(format!("{name}: one side rendered blank"));
            continue;
        };
        let divergent = KNOWN_DIVERGENT.iter().find(|(n, _)| *n == name);
        report.push(format!(
            "{name:14} ours_ink={:5} ref_ink={:5} iou={:.3} mean_err={:5.2} hard_frac={:.4}{}",
            stats.ours_ink,
            stats.ref_ink,
            stats.iou,
            stats.mean_err,
            stats.hard_frac,
            divergent
                .map(|(_, r)| format!("  [known-divergent: {r}]"))
                .unwrap_or_default(),
        ));
        if divergent.is_some() {
            continue;
        }
        let mut problems = Vec::new();
        if stats.iou < MIN_IOU {
            problems.push(format!("iou {:.3} < {MIN_IOU}", stats.iou));
        }
        let ink_ratio = stats.ours_ink as f64 / stats.ref_ink.max(1) as f64;
        if !(MIN_INK_RATIO..=MAX_INK_RATIO).contains(&ink_ratio) {
            problems.push(format!(
                "ink_ratio {ink_ratio:.3} outside [{MIN_INK_RATIO}, {MAX_INK_RATIO}]"
            ));
        }
        if stats.mean_err > MAX_MEAN_ERR {
            problems.push(format!("mean_err {:.2} > {MAX_MEAN_ERR}", stats.mean_err));
        }
        if stats.hard_frac > MAX_HARD_FRAC {
            problems.push(format!(
                "hard_frac {:.4} > {MAX_HARD_FRAC}",
                stats.hard_frac
            ));
        }
        if !problems.is_empty() {
            failures.push(format!("{name}: {}", problems.join(", ")));
        }
    }
    eprintln!("libass comparison report:\n{}", report.join("\n"));
    assert!(
        failures.is_empty(),
        "{} reference failure(s):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn harness_manifest_and_references_are_consistent() {
    // CI validation for the fixture harness itself: every manifest
    // entry must have its .ass + golden .rgba; no orphan fixture or
    // reference files may linger; every manifest entry needs a libass
    // frame unless it is genuinely pending; and pending entries must
    // actually be missing (no stale pending to silence a gate).
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let names: Vec<String> = manifest_times().into_iter().map(|(n, _)| n).collect();
    assert!(!names.is_empty(), "manifest must list fixtures");
    let mut problems = Vec::new();
    for name in &names {
        if !root.join(format!("tests/golden/{name}.ass")).exists() {
            problems.push(format!(
                "{name}: manifest entry lacks tests/golden/{name}.ass"
            ));
        }
        if !root.join(format!("tests/golden/{name}.rgba")).exists() {
            problems.push(format!(
                "{name}: manifest entry lacks tests/golden/{name}.rgba"
            ));
        }
        let has_ref = root.join(format!("tests/reference/{name}.rgba")).exists();
        let pending = PENDING_REFERENCES.contains(&name.as_str());
        if !has_ref && !pending {
            problems.push(format!("{name}: missing reference frame and not pending"));
        }
    }
    for pending in PENDING_REFERENCES {
        if root
            .join(format!("tests/reference/{pending}.rgba"))
            .exists()
        {
            problems.push(format!(
                "{pending}: stale pending entry (reference exists; gate it)"
            ));
        }
        if !names.iter().any(|n| n == pending) {
            problems.push(format!("{pending}: pending entry has no manifest fixture"));
        }
    }
    for (n, _) in KNOWN_DIVERGENT {
        if !names.iter().any(|m| m == n) {
            problems.push(format!(
                "{n}: known-divergent entry has no manifest fixture"
            ));
        }
    }
    let orphans = |dir: &str, ext: &str| {
        let mut orphans = Vec::new();
        if let Ok(entries) = std::fs::read_dir(root.join(dir)) {
            for entry in entries.flatten() {
                let path = entry.path();
                let is_file = path.extension().and_then(|e| e.to_str()) == Some(ext);
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_string();
                if is_file && !names.iter().any(|n| n == &stem) {
                    orphans.push(format!("{dir}/{stem}.{ext}"));
                }
            }
        }
        orphans
    };
    for orphan in orphans("tests/golden", "ass") {
        problems.push(format!("{orphan}: orphan fixture (not in manifest)"));
    }
    for orphan in orphans("tests/golden", "rgba") {
        problems.push(format!("{orphan}: orphan golden (not in manifest)"));
    }
    for orphan in orphans("tests/reference", "rgba") {
        // `blank.rgba` is the mask-control frame (see below), not a
        // comparison fixture, so it has no manifest entry by design.
        if orphan == "tests/reference/blank.rgba" {
            continue;
        }
        problems.push(format!("{orphan}: orphan reference (not in manifest)"));
    }
    let provenance = root.join("tests/reference/provenance.json");
    if !provenance.exists() {
        problems.push("tests/reference/provenance.json: missing".to_string());
    }
    assert!(
        problems.is_empty(),
        "{} harness problem(s):\n{}",
        problems.len(),
        problems.join("\n")
    );
}

#[test]
fn nondefault_playres_samples_are_visible_and_change() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let load = |name: &str| {
        std::fs::read(root.join(format!("tests/reference/{name}.rgba")))
            .expect("committed libass sample")
    };
    let early = load("transform-nondefault-playres-early");
    let mid = load("transform-nondefault-playres");
    let late = load("transform-nondefault-playres-late");
    for (name, frame) in [("early", &early), ("mid", &mid), ("late", &late)] {
        assert!(
            reference_ink_mask(frame).into_iter().any(|ink| ink),
            "{name} non-default-PlayRes sample must contain visible ink"
        );
    }
    assert_ne!(
        early, mid,
        "early and mid samples must exercise interpolation"
    );
    assert_ne!(
        mid, late,
        "mid and late samples must exercise interpolation"
    );
}

#[test]
fn ycbcr_reference_layer_is_explicitly_host_converted() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let provenance = std::fs::read_to_string(root.join("tests/reference/provenance.json"))
        .expect("reference provenance");
    assert!(
        provenance.contains("host/video conversion")
            || provenance.contains("host_video_conversion"),
        "YCbCr reference provenance must identify its host/video conversion layer"
    );
    for name in HOST_CONVERTED_FIXTURES {
        assert!(
            root.join(format!("tests/reference/{name}.rgba")).exists(),
            "host-converted reference exists: {name}"
        );
    }
}

#[test]
fn raw_rgba_ignores_ycbcr_matrix_metadata_without_host_conversion() {
    // This is intentionally a direct SubtitleRenderer test. It does not use
    // FFmpeg or a composited video frame, so it proves the libass-style raw
    // RGBA boundary rather than a downstream video conversion result.
    let render = |matrix: Option<&str>| {
        let matrix_line = matrix
            .map(|value| format!("YCbCr Matrix: {value}\n"))
            .unwrap_or_default();
        let ass = format!(
            "[Script Info]\nPlayResX: 384\nPlayResY: 216\n{matrix_line}\n\
[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
Style: Default,DejaVu Sans,36,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,0,0,0,0,100,100,0,0,1,0,0,7,10,10,10,1\n\
[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,{{\\an7\\pos(24,30)\\bord0\\shad0\\1c&H2080F0&\\p1}}m 0 0 l 90 0 l 90 38 l 0 38"
        );
        let mut renderer = SubtitleRenderer::new(&ass).expect("fixture parses");
        renderer.set_video_size(256, 144).expect("video size");
        renderer.render_frame(1000).expect("frame renders");
        renderer.frame_data().to_vec()
    };

    let raw = render(None);
    for matrix in [
        "Default", "Unknown", "None", "TV.601", "PC.601", "TV.709", "PC.709", "TV.240m", "PC.240m",
        "TV.FCC", "PC.FCC",
    ] {
        assert_eq!(raw, render(Some(matrix)), "raw RGBA changed for {matrix}");
    }
    assert!(
        raw.chunks_exact(4)
            .any(|pixel| pixel == [240, 128, 32, 255]),
        "visible authored RGB sample"
    );
}

#[test]
fn renderer_from_bytes_decodes_real_legacy_event_bytes() {
    let header = b"[Script Info]\nPlayResX: 384\nPlayResY: 216\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,DejaVu Sans,36,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,0,0,0,0,100,100,0,0,1,0,0,7,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,";
    let mut legacy = header.to_vec();
    legacy.extend_from_slice(b"caf");
    legacy.push(0xE9); // actual CP1252 byte for é

    let utf8 = [header.as_slice(), "café".as_bytes()].concat();
    let mut from_bytes = SubtitleRenderer::from_bytes(&legacy).expect("legacy bytes parse");
    let mut from_utf8 =
        SubtitleRenderer::new(std::str::from_utf8(&utf8).unwrap()).expect("UTF-8 text parse");
    from_bytes.set_video_size(256, 144).expect("video size");
    from_utf8.set_video_size(256, 144).expect("video size");
    from_bytes.render_frame(1000).expect("legacy frame renders");
    from_utf8.render_frame(1000).expect("UTF-8 frame renders");
    assert_eq!(from_bytes.frame_data(), from_utf8.frame_data());
    assert!(from_bytes
        .frame_data()
        .chunks_exact(4)
        .any(|pixel| pixel[3] > 0));
}

#[test]
fn blank_libass_frame_has_zero_ink() {
    // `blank.rgba` is a real libass frame with no active event
    // (regenerated by gen_references.ps1 alongside the fixtures): the
    // ink mask must find zero subtitle pixels in it, proving the mask
    // cannot mistake the generator's background level for ink.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let blank = std::fs::read(root.join("tests/reference/blank.rgba")).expect("blank frame");
    assert_eq!(blank.len(), 256 * 144 * 4);
    let mask = reference_ink_mask(&blank);
    let ink = mask.iter().filter(|m| **m).count();
    assert_eq!(ink, 0, "blank libass frame must yield zero ink pixels");
    // The background level itself is uniform (0 for the current
    // generator; the mask stays correct if it ever becomes 16).
    let level = blank[0].max(blank[1]).max(blank[2]);
    assert!(
        blank
            .chunks_exact(4)
            .all(|p| p[0].max(p[1]).max(p[2]) == level),
        "blank frame background must be uniform"
    );
    // Control in the other direction: a mid-fade frame and the opaque
    // box (whose black fill sits at level 16) must yield ink, and
    // every committed reference must be non-blank (catches a broken
    // regen that silently blanks a fixture).
    for name in ["fad-2args", "opaque-box"] {
        let bytes =
            std::fs::read(root.join(format!("tests/reference/{name}.rgba"))).expect("reference");
        let ink = reference_ink_mask(&bytes).iter().filter(|m| **m).count();
        assert!(ink > 100, "{name} must yield ink pixels, got {ink}");
    }
    for (name, _) in manifest_times() {
        let bytes =
            std::fs::read(root.join(format!("tests/reference/{name}.rgba"))).expect("reference");
        let ink = reference_ink_mask(&bytes).iter().filter(|m| **m).count();
        assert!(ink > 0, "{name}: reference frame is blank");
    }
}
