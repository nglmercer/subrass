//! Cluster-aware font fallback selection.

use super::line_break::is_combining_mark;

/// Resolve the fallback pick for every scalar in `text`: a base char
/// plus its following combining marks prefer the first face containing
/// the whole cluster (marks stay on the base's face instead of being
/// stolen by an earlier mark-only — or split from a base-only — face);
/// when no face holds the cluster, each char falls back independently.
/// Breaks resolve to 0 (callers skip them). Deterministic: shaping and
/// measurement share this, so widths always match.
pub fn cluster_font_picks<F>(text: &str, font_count: usize, mut has_glyph: F) -> Vec<usize>
where
    F: FnMut(usize, char) -> bool,
{
    let chars: Vec<char> = text.chars().collect();
    let mut picks = vec![0usize; chars.len()];
    let first_with =
        |ch: char, has_glyph: &mut F| (0..font_count).find(|&f| has_glyph(f, ch)).unwrap_or(0);
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\n' || ch == '\r' {
            i += 1;
            continue;
        }
        if is_combining_mark(ch) {
            // Lone mark (no base before it): independent fallback.
            picks[i] = first_with(ch, &mut has_glyph);
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < chars.len() && is_combining_mark(chars[j]) {
            j += 1;
        }
        let whole = (0..font_count)
            .find(|&f| has_glyph(f, ch) && (i + 1..j).all(|k| has_glyph(f, chars[k])));
        match whole {
            Some(f) => {
                picks[i..j].fill(f);
            }
            None => {
                for (slot, &ch) in picks[i..j].iter_mut().zip(&chars[i..j]) {
                    *slot = first_with(ch, &mut has_glyph);
                }
            }
        }
        i = j;
    }
    picks
}
