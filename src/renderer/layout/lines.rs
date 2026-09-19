//! Event-line geometry, drawing scale, clipping, and decoration helpers.

use super::state::ResolvedStyle;
use crate::renderer::font::DecorationMetrics;
use crate::types::override_tag::{OverrideTag, TextSegment};

/// Measured vector drawing for one segment, in video pixels.
/// `min_x` is the ink's left bearing (libass preserves it: ink at
/// pen + min); the advance is `width` and the box hangs `height`
/// above the baseline. `min_y` needs no field: ink lands at
/// origin + y with the origin one height above the baseline.
#[derive(Debug, Clone)]
pub(super) struct DrawingLayout {
    pub(super) mode: i32,
    pub(super) min_x: f64,
    pub(super) width: f64,
    pub(super) height: f64,
    pub(super) baseline: f64,
}

/// One face in a segment's fallback chain (primary first): the
/// stable font id plus the faux synthesis its glyphs require.
#[derive(Debug, Clone, Copy)]
pub(super) struct LayoutFace {
    pub(super) id: usize,
    pub(super) faux_bold: bool,
    pub(super) faux_italic: bool,
}

/// One laid-out segment: resolved style, shaped glyphs, fallback faces,
/// and optional drawing geometry.
pub(super) struct LayoutItem {
    pub(super) resolved: ResolvedStyle,
    pub(super) shaped: crate::renderer::shaper::ShapedLine,
    pub(super) faces: Vec<LayoutFace>,
    pub(super) drawing: Option<DrawingLayout>,
    pub(super) skipped: bool,
}

/// Whole-event layout: per-segment items plus block metrics.
pub(super) struct LayoutBlock {
    pub(super) items: Vec<LayoutItem>,
    pub(super) width: f64,
    pub(super) height: f64,
    pub(super) baseline: f64,
}

/// Advance the cumulative `\fay` baseline shear by one glyph (or
/// drawing): `fay * scale_y / scale_x * advance` (libass
/// `apply_baseline_shear`). Non-finite increments (degenerate scales
/// or advances) are ignored so one bad value cannot poison the rest
/// of the line; callers reset the accumulator at line breaks.
pub(super) fn accumulate_fay_shear(
    accum: &mut f64,
    shear_y: f64,
    scale_x: f64,
    scale_y: f64,
    advance: f64,
) {
    if shear_y == 0.0 {
        return;
    }
    if !shear_y.is_finite() || !scale_x.is_finite() || !scale_y.is_finite() || !advance.is_finite()
    {
        return;
    }
    if scale_x.abs() < 1e-9 {
        return;
    }
    let inc = shear_y * scale_y / scale_x * advance;
    if inc.is_finite() {
        *accum += inc;
    }
}

/// True laid-out width of every event line, in render order. Mirrors
/// the render loop exactly — skipped segments contribute nothing,
/// mid-segment shaper rows open new lines (zero-scale glyphs excluded,
/// like the renderer's row marker), and trailing breaks close the line
/// — so entry `i` is the width of the line the renderer calls `i`.
/// First row group of a segment continues the current line (segments
/// never start mid-line content elsewhere); leading breaks open lines.
pub(super) fn event_line_widths(segments: &[TextSegment], items: &[LayoutItem]) -> Vec<f64> {
    let mut lines = vec![0.0_f64];
    for (segment, item) in segments.iter().zip(items.iter()) {
        if item.skipped {
            continue;
        }
        // Leading breaks open lines (the render loop advances its line
        // index identically). Only '\n': mid-segment '\r' rows are caught
        // by row grouping on both sides; segment-boundary '\r' is ignored
        // by both, matching the existing trailing-break rule below.
        let leading = segment.text.chars().take_while(|c| *c == '\n').count();
        lines.extend(std::iter::repeat_n(0.0, leading));
        if let Some(drawing) = &item.drawing {
            if drawing.width.is_finite() {
                if let Some(last) = lines.last_mut() {
                    *last += drawing.width.max(0.0);
                }
            }
        } else {
            // Row-group widths: `x` restarts at 0 on every shaper row,
            // so each group's width is its furthest `x + advance` edge.
            let mut groups: Vec<f64> = Vec::new();
            let mut row_y: Option<f64> = None;
            let mut row_w = 0.0_f64;
            for glyph in &item.shaped.glyphs {
                if glyph.scale_x <= 0.0 || glyph.scale_y <= 0.0 {
                    continue;
                }
                if row_y != Some(glyph.y) {
                    if row_y.is_some() {
                        groups.push(row_w);
                    }
                    row_y = Some(glyph.y);
                    row_w = 0.0;
                }
                let edge = glyph.x + glyph.advance;
                if edge.is_finite() && edge > row_w {
                    row_w = edge;
                }
            }
            if row_y.is_some() {
                groups.push(row_w);
            }
            let mut groups = groups.into_iter();
            if let Some(w0) = groups.next() {
                if let Some(last) = lines.last_mut() {
                    *last += w0;
                }
            }
            for w in groups {
                lines.push(w);
            }
        }
        if segment.text.ends_with('\n') {
            lines.push(0.0);
        }
    }
    lines
}

/// One event line's box geometry for `BorderStyle=3`: width plus the
/// top offset (relative to the block top) and height.
#[derive(Debug, Clone, Copy)]
pub(super) struct EventLineBox {
    pub(super) width: f64,
    pub(super) y: f64,
    pub(super) height: f64,
    /// True once a content row (text or drawing) joins the line.
    /// Interior empty lines still draw (they tile the column); a
    /// trailing empty last line is skipped (extent-based, VSFilter
    /// draws no box past the final ink).
    pub(super) has_content: bool,
}

/// Per-line box geometry, mirroring the render loop's line model (same
/// breaks and shaper rows as [`event_line_widths`]) so each box frames
/// the ink it belongs to:
///
/// * Content rows join the current line (widths add, height takes the
///   max) or open a new one at mid-segment row changes, at their exact
///   shaper y (render truth, gaps included).
/// * Leading breaks append lines (the first fills the pristine initial
///   line); their provisional tops backfill from the segment's first
///   content row so breaks tile exactly.
/// * Interior row gaps synthesize empty lines; a trailing break appends
///   one empty line tiling the previous bottom (libass draws boxes for
///   empty lines too, including trailing ones).
/// * Drawings join at the segment base (the render loop draws them
///   there even when their text holds breaks); their row height for
///   empty-line tiling is the segment's shaped line height.
///
/// Widths match [`event_line_widths`] line for line except for
/// synthesized gap rows (zero width); tops tile without gaps because
/// every render-loop y advance is covered by line heights.
pub(super) fn event_line_boxes(
    segments: &[TextSegment],
    items: &[LayoutItem],
) -> Vec<EventLineBox> {
    /// Cap on synthesized gap rows per gap: row gaps are bounded by
    /// the segment's break count in practice; this only bounds float
    /// garbage from reaching the box loop.
    const MAX_GAP_ROWS: i64 = 1_000_000;
    let mut lines = vec![EventLineBox {
        width: 0.0,
        y: 0.0,
        height: 0.0,
        has_content: false,
    }];
    // Pen y of the current segment base (mirrors the render loop's
    // `line_y_offset`).
    let mut rel_y = 0.0f64;
    let clean = |v: f64| {
        if v.is_finite() {
            v.max(0.0)
        } else {
            0.0
        }
    };
    for (segment, item) in segments.iter().zip(items.iter()) {
        if item.skipped {
            continue;
        }
        let seg_base = rel_y;
        let lh = clean(item.shaped.line_height);
        // Leading breaks: the first fills the pristine initial line,
        // the rest append (matching `event_line_widths` counts).
        let leading = segment.text.chars().take_while(|c| *c == '\n').count();
        // Appended leading lines start here (the filled initial line,
        // when pristine, is slot 0 and never backfilled).
        let lead_start = lines.len();
        if leading > 0 {
            let pristine = lines.len() == 1 && lines[0].width == 0.0 && lines[0].height == 0.0;
            if pristine {
                lines[0].height = lh;
            }
            for k in 0..leading {
                // Provisional tops tile forward; content backfills.
                let slot = if pristine { k + 1 } else { k };
                lines.push(EventLineBox {
                    width: 0.0,
                    y: seg_base + slot as f64 * lh,
                    height: lh,
                    has_content: false,
                });
            }
        }
        // Content rows `(y, width, height, is_gap)`: drawings are
        // atomic (single row at the segment base); text rows group
        // exactly like `event_line_widths`.
        let mut rows: Vec<(f64, f64, f64, bool)> = Vec::new();
        if let Some(drawing) = &item.drawing {
            rows.push((0.0, clean(drawing.width), clean(drawing.height), false));
        } else {
            let mut row_y: Option<f64> = None;
            let mut row_w = 0.0f64;
            let mut prev_y: Option<f64> = None;
            let flush_row = |rows: &mut Vec<(f64, f64, f64, bool)>,
                             prev_y: &mut Option<f64>,
                             y: f64,
                             w: f64| {
                // Synthesize empty slots for skipped row indices.
                if lh > 0.0 {
                    if let Some(py) = *prev_y {
                        let diff = y - py;
                        if diff.is_finite() && diff > 0.0 {
                            let ratio = diff / lh;
                            if ratio.is_finite() {
                                let missing = (ratio.round() as i64).clamp(0, MAX_GAP_ROWS) - 1;
                                for m in 1..=missing {
                                    rows.push((py + m as f64 * lh, 0.0, lh, true));
                                }
                            }
                        }
                    }
                }
                rows.push((y, w, lh, false));
                *prev_y = Some(y);
            };
            for glyph in &item.shaped.glyphs {
                if glyph.scale_x <= 0.0 || glyph.scale_y <= 0.0 {
                    continue;
                }
                match row_y {
                    Some(y) if y == glyph.y => {}
                    _ => {
                        if let Some(y) = row_y {
                            flush_row(&mut rows, &mut prev_y, y, row_w);
                        }
                        row_y = Some(glyph.y);
                        row_w = 0.0;
                    }
                }
                let edge = glyph.x + glyph.advance;
                if edge.is_finite() && edge > row_w {
                    row_w = edge;
                }
            }
            if let Some(y) = row_y {
                flush_row(&mut rows, &mut prev_y, y, row_w);
            }
        }
        // First content row joins the current line (overwriting its y
        // with render truth) and backfills leading empties; later rows
        // open new lines.
        let mut first_row = true;
        for (row_y, row_w, row_h, is_gap) in rows {
            let exact_y = seg_base + row_y;
            if first_row {
                first_row = false;
                // Backfill leading slots from the content row so breaks
                // tile exactly (the last appended line takes content).
                if leading > 0 && lh > 0.0 {
                    for k in 0..leading.saturating_sub(1) {
                        if let Some(slot) = lines.get_mut(lead_start + k) {
                            slot.y = exact_y - (leading - 1 - k) as f64 * lh;
                            slot.height = lh;
                        }
                    }
                }
                if let Some(last) = lines.last_mut() {
                    last.y = exact_y;
                    last.height = last.height.max(row_h);
                    last.width += row_w;
                    if !is_gap {
                        last.has_content = true;
                    }
                }
            } else {
                lines.push(EventLineBox {
                    width: row_w,
                    y: exact_y,
                    height: row_h,
                    // Gap-synthesis rows carry no ink but tile the
                    // column; only real content rows mark the line.
                    has_content: !is_gap,
                });
            }
        }
        if segment.text.ends_with('\n') {
            // Advance past the whole segment (mirrors the render loop),
            // then open one empty line tiling the previous bottom.
            let adv = if let Some(drawing) = &item.drawing {
                clean(drawing.height)
            } else {
                clean(item.shaped.height)
            };
            rel_y += adv;
            let (y, h) = match lines.last() {
                Some(last) => (last.y + last.height, lh),
                None => (rel_y, lh),
            };
            lines.push(EventLineBox {
                width: 0.0,
                y,
                height: h,
                has_content: false,
            });
        }
    }
    lines
}

/// Horizontal inset of one line inside the event block for the
/// event-level alignment (libass aligns each line independently:
/// short lines center/right-align on their own width, they do not
/// hug the block edge). `block_width` must be the same width the
/// block origin was computed from. Unknown alignments center, like
/// [`Compositor::calculate_position`].
pub(super) fn line_align_inset(line_width: f64, block_width: f64, alignment: i32) -> f64 {
    let extra = (block_width - line_width).max(0.0);
    if !extra.is_finite() {
        return 0.0;
    }
    match alignment {
        1 | 4 | 7 => 0.0,
        3 | 6 | 9 => extra,
        _ => extra / 2.0,
    }
}

/// Video pixels per drawing unit for a `\pN` mode: higher modes pack
/// more units per script pixel, so each unit renders smaller.
pub(super) fn drawing_unit_scale(scale_x: f64, scale_y: f64, mode: i32) -> f64 {
    let res = (scale_x + scale_y) / 2.0;
    if mode > 0 {
        res / 2f64.powi(mode.saturating_sub(1).min(20))
    } else {
        res
    }
}

/// Scale a script-coordinate clip rectangle to video pixels,
/// saturating instead of overflowing on extreme coordinates.
pub(super) fn scale_clip_rect(
    rect: (i32, i32, i32, i32),
    scale_x: f64,
    scale_y: f64,
) -> (i32, i32, i32, i32) {
    // libass converts ASS clip rects with round-half-up (`+ 0.5`
    // then truncate toward zero) and treats the rect as half-open
    // `[x0,x1) x [y0,y1)` (probe: an iclip y1 sweep keeps rows from
    // `round(y1 * scale)`). Our pixel clips are inclusive, so upper
    // bounds convert with a saturating `- 1`.
    let conv = |v: i32, s: f64, hi: bool| -> i32 {
        if !s.is_finite() {
            return v;
        }
        let p = f64::from(v) * s;
        if !p.is_finite() {
            return v;
        }
        let rounded = (p + 0.5) as i32;
        if hi {
            rounded.saturating_sub(1)
        } else {
            rounded
        }
    };
    (
        conv(rect.0, scale_x, false),
        conv(rect.1, scale_y, false),
        conv(rect.2, scale_x, true),
        conv(rect.3, scale_y, true),
    )
}

/// Decoration bar rows for one glyph, in scaled-bitmap pixels:
/// `(top, bottom)` per active bar, relative to the bitmap origin.
/// libass `ass_get_glyph_outline` centers each bar on its font-metric
/// position about the pen: underline `|pos|` below the baseline,
/// strikeout `pos` above. Returns an empty vec when the font lacks
/// metrics or the em size is degenerate (libass draws no bar then).
pub(super) fn deco_bar_rows(
    underline: bool,
    strikeout: bool,
    metrics: DecorationMetrics,
    pen_y: f64,
    em_px: f64,
) -> Vec<(f64, f64)> {
    let mut rows = Vec::new();
    // libass scales bar positions by the FreeType `y_scale`
    // (Win-basis size divisor), not units-per-em.
    let div = f64::from(metrics.scale_height);
    if div <= 0.0 || !em_px.is_finite() || em_px <= 0.0 || !pen_y.is_finite() {
        return rows;
    }
    if underline {
        if let Some((pos, thick)) = metrics.underline {
            let center = pen_y + f64::from(pos.unsigned_abs()) / div * em_px;
            let half = f64::from(thick) / div * em_px / 2.0;
            if half.is_finite() && half > 0.0 && center.is_finite() {
                rows.push((center - half, center + half));
            }
        }
    }
    if strikeout {
        if let Some((pos, size)) = metrics.strikeout {
            let center = pen_y - f64::from(pos) / div * em_px;
            let half = f64::from(size) / div * em_px / 2.0;
            if half.is_finite() && half > 0.0 && center.is_finite() {
                rows.push((center - half, center + half));
            }
        }
    }
    rows
}

/// Paint one decoration bar into a coverage bitmap with box-AA: every
/// overlapped row/column gets proportional coverage, merged via max
/// so bars add ink without erasing glyph pixels. Bounds are
/// fractional bitmap pixels and clamp to the bitmap.
pub(super) fn paint_deco_bar(
    bitmap: &mut [u8],
    w: u32,
    h: u32,
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
) {
    if w == 0 || h == 0 {
        return;
    }
    if !x0.is_finite() || !x1.is_finite() || !y0.is_finite() || !y1.is_finite() {
        return;
    }
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let (w_f, h_f) = (f64::from(w), f64::from(h));
    let (xa, xb) = (x0.clamp(0.0, w_f), x1.clamp(0.0, w_f));
    let (ya, yb) = (y0.clamp(0.0, h_f), y1.clamp(0.0, h_f));
    if xb <= xa || yb <= ya {
        return;
    }
    let (ix0, ix1) = (xa.floor() as i64, xb.ceil() as i64);
    let (iy0, iy1) = (ya.floor() as i64, yb.ceil() as i64);
    for yy in iy0..iy1 {
        if yy < 0 || yy >= i64::from(h) {
            continue;
        }
        let cover_y = (yb.min(yy as f64 + 1.0) - ya.max(yy as f64)).clamp(0.0, 1.0);
        if cover_y <= 0.0 {
            continue;
        }
        for xx in ix0..ix1 {
            if xx < 0 || xx >= i64::from(w) {
                continue;
            }
            let cover_x = (xb.min(xx as f64 + 1.0) - xa.max(xx as f64)).clamp(0.0, 1.0);
            let cover = cover_x * cover_y;
            if cover <= 0.0 {
                continue;
            }
            let idx = (yy as u32 * w + xx as u32) as usize;
            if let Some(px) = bitmap.get_mut(idx) {
                *px = (*px).max((cover * 255.0).round().clamp(0.0, 255.0) as u8);
            }
        }
    }
}

/// Active drawing mode for a segment: the last `\pN` in its tags,
/// falling back to the event-level mode. `\r` is transparent here:
/// libass `ass_reset_render_context` never touches `drawing_scale`,
/// so only an explicit `\p0` exits drawing mode.
pub(super) fn segment_drawing_mode(tags: &[OverrideTag], event_mode: i32) -> i32 {
    for tag in tags.iter().rev() {
        if let OverrideTag::Drawing(m) = tag {
            return *m;
        }
    }
    event_mode
}
