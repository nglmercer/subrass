//! ASS word tokenization and line wrapping.

use crate::renderer::shaper::TextShaper;
use ab_glyph::FontArc;

/// A word plus any raw tag groups that preceded it, with measured width
#[derive(Debug, Clone)]
struct WrapWord {
    prefix: String,
    text: String,
    width: f64,
    /// True when this word was separated from the previous word by a space
    /// in the original text. Used to avoid inserting phantom spaces between
    /// words that were only split by override-tag boundaries (e.g. karaoke).
    preceded_by_space: bool,
    /// True for CJK continuation pieces: split from the previous word at
    /// a CJK break opportunity, so no space is emitted or measured
    /// between them (unlike `preceded_by_space`, which measures one).
    glued_to_prev: bool,
}

/// Insert '\n' at word boundaries per the ASS wrap style:
/// 0 = smart wrapping (balanced lines), 1 = end-of-line greedy
/// wrapping, 2 = no automatic wrapping, 3 = same as 0 (libass runs
/// `wrap_lines_smart` for every style except 1; VSFilter's
/// bottom-wide style 3 is a known libass divergence).
///
/// Tag groups are opaque and travel with the word that follows them;
/// explicit `\N` breaks (and `\n` in mode 2) split the text into
/// independently wrapped runs. `\n` elsewhere acts as a space. Drawing
/// runs pass through verbatim and are never wrapped. Non-CJK words
/// wider than `max_width` stay on their own line; CJK runs split at
/// break opportunities (glued pieces, no phantom spaces).
///
/// `fonts` is the measurement chain (primary first): word widths use
/// the same per-glyph fallback cascade as shaping, so wrap decisions
/// match rendered widths even when the primary lacks characters.
pub(super) fn wrap_event_text(
    text: &str,
    wrap_style: i32,
    max_width: f64,
    fonts: &[&FontArc],
    font_size: f64,
    spacing: f64,
) -> String {
    let measure =
        |value: &str| TextShaper::measure_text_with_fallback(value, fonts, font_size, spacing);
    wrap_event_text_with_measure(text, wrap_style, max_width, &measure)
}

pub(super) fn wrap_event_text_with_measure<F>(
    text: &str,
    wrap_style: i32,
    max_width: f64,
    measure: &F,
) -> String
where
    F: Fn(&str) -> f64,
{
    if wrap_style == 2 || max_width <= 0.0 {
        return text.to_string();
    }

    /// Drawing state after a `{...}` group: the last `\pN` wins in
    /// textual order. `\r` is ignored: libass
    /// `ass_reset_render_context` never touches `drawing_scale`, so
    /// `{\p1\r}` stays drawing — only `\p0` exits.
    fn drawing_state_after_group(group: &str) -> Option<bool> {
        let bytes = group.as_bytes();
        let mut state: Option<bool> = None;
        let mut i = 0;
        while i + 1 < bytes.len() {
            if bytes[i] == b'\\' && bytes[i + 1] == b'p' {
                // `\pN` with digits; `\pbo` has none and is skipped.
                let mut j = i + 2;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if j > i + 2 {
                    if let Ok(mode) = group[i + 2..j].parse::<i32>() {
                        state = Some(mode > 0);
                    }
                    i = j;
                    continue;
                }
            }
            i += 1;
        }
        state
    }

    // Tokenize into words (with pending tag prefixes) and hard breaks.
    let mut words: Vec<WrapWord> = Vec::new();
    // Break markers: index into `words` where a new run starts
    let mut run_starts: Vec<usize> = vec![0];
    let mut prefix = String::new();
    let mut word = String::new();

    // First word is not preceded by a space; the current word holds
    // drawing commands when verbatim (never wrapped).
    let mut preceded_by_space = false;
    let mut word_is_drawing = false;

    let flush = |prefix: &mut String,
                 word: &mut String,
                 words: &mut Vec<WrapWord>,
                 pbys: &mut bool,
                 is_drawing: &mut bool| {
        // Flush when there is text, or when a tag prefix must be preserved
        // (e.g. {\b0} between "Bold" and the following space).
        if word.is_empty() && prefix.is_empty() {
            return;
        }
        let has_text = !word.is_empty();
        // Drawing words measure geometric width, never the advances of
        // their command letters: text-measuring "m 0 0 l 100 ..." forces
        // bogus line breaks before drawings (and strands karaoke timing
        // on the break). Unit scale is unavailable here, so this is in
        // drawing units (exact for the common scale-1/mode-1 case).
        let width = if *is_drawing && has_text {
            let commands = word.split('{').next().unwrap_or(word);
            crate::renderer::drawing::DrawingParser::measure(commands)
                .map(|(_, _, w, _)| w)
                .filter(|w| w.is_finite())
                .map(|w| w.max(0.0))
                .unwrap_or(0.0)
        } else {
            measure(word)
        };
        *is_drawing = false;
        words.push(WrapWord {
            prefix: std::mem::take(prefix),
            text: std::mem::take(word),
            width,
            preceded_by_space: *pbys,
            glued_to_prev: false,
        });
        // Only reset when there's actual text — prefix-only entries don't
        // "consume" the space flag.
        if has_text {
            *pbys = false;
        }
    };

    let mut chars = text.chars().peekable();
    let mut in_drawing = false;
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                let mut group = String::from("{");
                for gc in chars.by_ref() {
                    group.push(gc);
                    if gc == '}' {
                        break;
                    }
                }
                if in_drawing {
                    // Verbatim: drawing runs are never wrapped or split.
                    word.push_str(&group);
                    word_is_drawing = true;
                } else {
                    flush(
                        &mut prefix,
                        &mut word,
                        &mut words,
                        &mut preceded_by_space,
                        &mut word_is_drawing,
                    );
                    prefix.push_str(&group);
                }
                if let Some(drawing) = drawing_state_after_group(&group) {
                    in_drawing = drawing;
                }
            }
            '\\' if in_drawing => {
                word.push('\\');
                if let Some(&n) = chars.peek() {
                    word.push(n);
                    chars.next();
                }
                word_is_drawing = true;
            }
            '\\' => match chars.peek() {
                Some('N') => {
                    chars.next();
                    flush(
                        &mut prefix,
                        &mut word,
                        &mut words,
                        &mut preceded_by_space,
                        &mut word_is_drawing,
                    );
                    preceded_by_space = true;
                    run_starts.push(words.len());
                }
                Some('n') => {
                    chars.next();
                    // Soft break: a hard break only in mode 2, else a space.
                    if wrap_style == 2 {
                        flush(
                            &mut prefix,
                            &mut word,
                            &mut words,
                            &mut preceded_by_space,
                            &mut word_is_drawing,
                        );
                        preceded_by_space = true;
                        run_starts.push(words.len());
                    } else {
                        flush(
                            &mut prefix,
                            &mut word,
                            &mut words,
                            &mut preceded_by_space,
                            &mut word_is_drawing,
                        );
                        preceded_by_space = true;
                    }
                }
                Some('h') => {
                    chars.next();
                    word.push('\u{00A0}');
                }
                Some(&n) => {
                    chars.next();
                    word.push('\\');
                    word.push(n);
                }
                None => word.push('\\'),
            },
            ' ' | '\t' => {
                if in_drawing {
                    word.push(c);
                    word_is_drawing = true;
                } else {
                    flush(
                        &mut prefix,
                        &mut word,
                        &mut words,
                        &mut preceded_by_space,
                        &mut word_is_drawing,
                    );
                    preceded_by_space = true;
                }
            }
            _ => {
                word.push(c);
                if in_drawing {
                    word_is_drawing = true;
                }
            }
        }
    }
    flush(
        &mut prefix,
        &mut word,
        &mut words,
        &mut preceded_by_space,
        &mut word_is_drawing,
    );
    // Trailing tag groups with no word attach as a zero-width word
    if !prefix.is_empty() {
        words.push(WrapWord {
            prefix,
            text: String::new(),
            width: 0.0,
            preceded_by_space: false,
            glued_to_prev: false,
        });
    }
    // CJK break opportunities inside words (conservative UAX #14):
    // overlong CJK runs wrap without spaces. Continuation pieces glue
    // with no gap; run starts remap to the split indices.
    let words = split_cjk_words(words, &mut run_starts, measure);

    let space_width = measure(" ");

    // Wrap each explicit-line run independently, then rejoin with breaks.
    let mut out = String::with_capacity(text.len() + 16);
    let mut run_begin = 0usize;
    for start in run_starts.iter().skip(1) {
        render_wrapped_run(
            &words[run_begin..*start],
            wrap_style,
            max_width,
            space_width,
            &mut out,
        );
        out.push('\n');
        run_begin = *start;
    }
    render_wrapped_run(
        &words[run_begin..],
        wrap_style,
        max_width,
        space_width,
        &mut out,
    );

    out
}

/// Split plain-text words at CJK break opportunities
/// ([`cjk_break_between`](super::shaper::cjk_break_between)); the first
/// piece keeps the word's prefix/flags, continuations glue with no gap.
/// Skips drawing words (verbatim text with groups), empty words, and
/// breaks right after a backslash (would split `\x` escapes across
/// lines). Remaps `run_starts` to the split indices in place.
fn split_cjk_words<F>(words: Vec<WrapWord>, run_starts: &mut [usize], measure: &F) -> Vec<WrapWord>
where
    F: Fn(&str) -> f64,
{
    use crate::renderer::shaper::cjk_break_between;
    let mut out: Vec<WrapWord> = Vec::with_capacity(words.len());
    let mut index_of: Vec<usize> = Vec::with_capacity(words.len() + 1);
    for mut word in words {
        index_of.push(out.len());
        let chars: Vec<char> = word.text.chars().collect();
        let splittable = !chars.is_empty() && !word.text.contains('{');
        let mut cuts = vec![false; chars.len()];
        if splittable {
            for i in 0..chars.len().saturating_sub(1) {
                if chars[i] != '\\' && cjk_break_between(chars[i], chars[i + 1]) {
                    cuts[i] = true;
                }
            }
        }
        if !cuts.iter().any(|c| *c) {
            out.push(word);
            continue;
        }
        let mut start = 0usize;
        let mut first = true;
        for (i, cut) in cuts.iter().enumerate() {
            if !cut {
                continue;
            }
            let piece: String = chars[start..=i].iter().collect();
            let width = measure(&piece);
            if first {
                out.push(WrapWord {
                    prefix: std::mem::take(&mut word.prefix),
                    text: piece,
                    width,
                    preceded_by_space: word.preceded_by_space,
                    glued_to_prev: false,
                });
                first = false;
            } else {
                out.push(WrapWord {
                    prefix: String::new(),
                    text: piece,
                    width,
                    preceded_by_space: false,
                    glued_to_prev: true,
                });
            }
            start = i + 1;
        }
        let piece: String = chars[start..].iter().collect();
        let width = measure(&piece);
        if first {
            out.push(WrapWord {
                prefix: word.prefix,
                text: piece,
                width,
                preceded_by_space: word.preceded_by_space,
                glued_to_prev: false,
            });
        } else {
            out.push(WrapWord {
                prefix: String::new(),
                text: piece,
                width,
                preceded_by_space: false,
                glued_to_prev: true,
            });
        }
    }
    index_of.push(out.len());
    for start in run_starts.iter_mut() {
        *start = index_of.get(*start).copied().unwrap_or(out.len());
    }
    out
}

/// Width charged between a line's previous word and `word`: a full
/// space, except CJK continuation pieces glue with no gap (they were
/// split from one word, not separated by a space).
fn gap_before(word: &WrapWord, space_width: f64) -> f64 {
    if word.glued_to_prev {
        0.0
    } else {
        space_width
    }
}

/// Greedy line grouping (wrap style 1): forward fill, top line
/// first. Prefix-only entries ride along without consuming width
/// budget.
fn greedy_wrap_lines(words: &[WrapWord], max_width: f64, space_width: f64) -> Vec<Vec<usize>> {
    let mut lines: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut cur_w = 0.0_f64;
    for (idx, word) in words.iter().enumerate() {
        let ww = word.width;
        if ww == 0.0 && word.text.is_empty() {
            cur.push(idx);
            continue;
        }
        if cur.is_empty() {
            cur_w = ww;
            cur.push(idx);
        } else if cur_w + gap_before(word, space_width) + ww <= max_width {
            cur_w += gap_before(word, space_width) + ww;
            cur.push(idx);
        } else {
            lines.push(std::mem::take(&mut cur));
            cur_w = ww;
            cur.push(idx);
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// Smart line grouping (wrap styles 0 and 3): greedy fill, then pairwise
/// rebalance (libass `wrap_lines_smart`: move the last word of a line
/// to the next line while it reduces the pair's length difference).
/// This balances lines instead of minimizing leftover (a 224/140
/// greedy split rebalances toward even halves, as in libass).
/// Overlong single words keep their own line. Words only ever move
/// to later lines, so the loop always terminates.
fn smart_wrap_lines(words: &[WrapWord], max_width: f64, space_width: f64) -> Vec<Vec<usize>> {
    let mut lines = greedy_wrap_lines(words, max_width, space_width);
    // Trimmed width of one line (prefix-only entries are free, spaces
    // only between text words).
    let line_len = |line: &[usize]| -> f64 {
        let mut w = 0.0_f64;
        let mut text_words = 0usize;
        for &idx in line {
            let word = &words[idx];
            if word.text.is_empty() && word.width == 0.0 {
                continue;
            }
            if text_words > 0 {
                w += gap_before(word, space_width);
            }
            w += word.width;
            text_words += 1;
        }
        w
    };
    loop {
        let mut moved = false;
        let mut i = 0usize;
        while i + 1 < lines.len() {
            // Never empty a line (merging breaks is never beneficial).
            if lines[i].len() > 1 {
                let l1 = line_len(&lines[i]);
                let l2 = line_len(&lines[i + 1]);
                let mut new_l1 = lines[i].clone();
                let w = new_l1.pop().expect("len > 1");
                let mut new_l2 = Vec::with_capacity(lines[i + 1].len() + 1);
                new_l2.push(w);
                new_l2.extend_from_slice(&lines[i + 1]);
                let l1_new = line_len(&new_l1);
                let l2_new = line_len(&new_l2);
                if (l1_new - l2_new).abs() < (l1 - l2).abs() {
                    lines[i] = new_l1;
                    lines[i + 1] = new_l2;
                    moved = true;
                }
            }
            i += 1;
        }
        if !moved {
            break;
        }
    }
    lines
}

/// Wrap one run of words and append the result to `out`.
fn render_wrapped_run(
    words: &[WrapWord],
    wrap_style: i32,
    max_width: f64,
    space_width: f64,
    out: &mut String,
) {
    if words.is_empty() {
        return;
    }

    // libass `wrap_lines_smart` runs for every style except 1 (the
    // rebalance loop is gated on `wrap_style != 1`): styles 0 and 3
    // are the same smart fill, 1 is greedy only. Style 3 is NOT
    // bottom-wide greedy here — that is VSFilter behavior, and
    // libass documents the gap with a FIXME ("implement style 0 and
    // 3 correctly"). Style 2 never reaches this function.
    let lines: Vec<Vec<usize>> = if wrap_style == 1 {
        greedy_wrap_lines(words, max_width, space_width)
    } else {
        smart_wrap_lines(words, max_width, space_width)
    };

    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let mut prev_had_text = false;
        for (j, &idx) in line.iter().enumerate() {
            let w = &words[idx];
            if j > 0 && prev_had_text && !w.text.is_empty() && w.preceded_by_space {
                out.push(' ');
            }
            out.push_str(&w.prefix);
            out.push_str(&w.text);
            // Once we've emitted text, stay true — prefix-only entries must
            // not flip this back to false or the next word loses its space.
            if !w.text.is_empty() {
                prev_had_text = true;
            }
        }
    }
}
