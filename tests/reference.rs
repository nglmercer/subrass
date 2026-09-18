// libass reference comparison tests (plan #41).
//
// Compares subrass output against frames rendered by a pinned libass
// (via ffmpeg's `ass` filter; see tests/reference/provenance.json).
// References live in tests/reference/*.rgba (raw RGBA over black);
// fixtures mirror tests/golden (same .ass inputs and manifest times).
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
        "opaque-box-multiline",
        "whole-block box vs libass per-line boxes (known divergence)",
    ),
];

/// Fixtures whose libass reference frames have not been generated yet
/// (no ffmpeg+libass in this environment). Missing `.rgba` files for
/// these names are reported as pending, not failures; once
/// `gen_references.ps1` produces them, they gate like all others.
/// Never add an existing divergence here to silence it — that list
/// is `KNOWN_DIVERGENT` above.
const PENDING_REFERENCES: &[&str] = &[
    "relative-fs",
    "shear-rotation",
    "karaoke-kf-frz",
    "karaoke-kf-frx",
    "karaoke-kf-fax",
    "karaoke-kf-fay",
    "opaque-box-multiline",
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

fn compare(ours_rgba: &[u8], ref_rgba: &[u8], w: u32) -> Option<Stats> {
    // Ink masks: our side keys on alpha (a black outline, shadow, or
    // opaque box is real ink even though its RGB is zero); the
    // reference is an opaque frame, so it keys on raw nonzero RGB
    // (this catches its range-shifted black box at level 16).
    let ours_mask: Vec<bool> = ours_rgba.chunks_exact(4).map(|p| p[3] > 0).collect();
    let ref_mask: Vec<bool> = ref_rgba
        .chunks_exact(4)
        .map(|p| p[0] > 0 || p[1] > 0 || p[2] > 0)
        .collect();
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
                normalize_ref(p[0]),
                normalize_ref(p[1]),
                normalize_ref(p[2]),
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
        let ass = std::fs::read_to_string(golden_dir.join(format!("{name}.ass")))
            .expect("golden fixture");
        let mut renderer = SubtitleRenderer::new(&ass).expect("fixture parses");
        renderer.set_video_size(256, 144).expect("video size");
        renderer.render_frame(time_ms).expect("render");
        let ours = renderer.frame_data().to_vec();
        let ref_bytes = match std::fs::read(ref_dir.join(format!("{name}.rgba"))) {
            Ok(b) => b,
            Err(_) => {
                if PENDING_REFERENCES.contains(&name.as_str()) {
                    report.push(format!(
                        "{name:14} [pending: no libass frame yet; run gen_references.ps1]"
                    ));
                    continue;
                }
                failures.push(format!("{name}: missing reference frame"));
                continue;
            }
        };
        let Some(stats) = compare(&ours, &ref_bytes, 256) else {
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
