// Real-world corpus tests (plan #67): every file under tests/corpus/
// parses and renders a frame without panicking. Files are minimal
// synthetic reproductions of wild shapes (karaoke, signs, legacy SSA,
// vector drawings, hi-res, embedded fonts, odd wrap, hard transforms),
// not copyrighted subtitle content.
use std::path::Path;

use subrass::parser::AssDocument;
use subrass::renderer::SubtitleRenderer;

fn corpus_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus")
}

#[test]
fn corpus_parses_without_panic() {
    let dir = corpus_dir();
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("tests/corpus exists")
        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
        .filter(|n| n.ends_with(".ass") || n.ends_with(".ssa"))
        .collect();
    names.sort();
    assert!(!names.is_empty(), "corpus must not be empty");
    for name in &names {
        let text = std::fs::read_to_string(dir.join(name)).expect("read corpus file");
        // Parsing may fail (some files are hostile by design) but must
        // never panic; a parsed doc must expose sane counts.
        if let Ok(doc) = AssDocument::parse(&text) {
            let _ = (doc.events.len(), doc.styles.len(), doc.attachments.len());
        }
    }
}

#[test]
fn corpus_renders_without_panic() {
    let dir = corpus_dir();
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("tests/corpus exists")
        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
        .filter(|n| n.ends_with(".ass") || n.ends_with(".ssa"))
        .collect();
    names.sort();
    for name in &names {
        let text = std::fs::read_to_string(dir.join(name)).expect("read corpus file");
        // Rendering may fail (invalid docs) but must never panic; a
        // rendered frame must match the requested dimensions.
        if let Ok(mut renderer) = SubtitleRenderer::new(&text) {
            if renderer.set_video_size(320, 180).is_err() {
                continue;
            }
            for t in [0u64, 1000, 2500, 60_000] {
                if renderer.render_frame(t).is_ok() {
                    assert_eq!(renderer.frame_size(), (320, 180), "{name} @{t}ms");
                    assert_eq!(renderer.frame_data().len(), 320 * 180 * 4);
                }
            }
        }
    }
}
