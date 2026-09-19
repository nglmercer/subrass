//! Karaoke run construction and sweep painting.

use super::lines::LayoutItem;
use super::state::ComplexFade;
use crate::renderer::buffer::{add_coord, RenderBuffer};
use crate::renderer::effects;
use crate::types::override_tag::{OverrideTag, TextSegment};

/// Karaoke syllable kind
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KaraokeKind {
    /// `\k` — hard color swap when the syllable starts
    Hard,
    /// `\K` / `\kf` — left-to-right color sweep over the syllable
    Sweep,
    /// `\ko` — outline hidden before the syllable starts, visible from start
    Outline,
}

/// Opacity (0.0 = invisible, 1.0 = fully visible) for
/// `\fade(a1,a2,a3,t1,t2,t3,t4)` at `elapsed` ms into the event.
///
/// The fade value comes from [`effects::interpolate_alpha`] (libass
/// `interpolate_alpha`, including its truncation); a value `<= 0`
/// leaves the frame fully opaque (libass `ass_apply_fade` only
/// applies positive fades) and above 255 clamps to transparent
/// (libass wraps mod 256 there, a C-cast artifact).
pub(super) fn complex_fade_opacity(cf: &ComplexFade, elapsed: u64) -> f64 {
    let now = i64::try_from(elapsed).unwrap_or(i64::MAX);
    let a = effects::interpolate_alpha(
        now,
        i64::from(cf.t1),
        i64::from(cf.t2),
        i64::from(cf.t3),
        i64::from(cf.t4),
        cf.a1,
        cf.a2,
        cf.a3,
    );
    if a <= 0 {
        1.0
    } else {
        (1.0 - f64::from(a.min(255)) / 255.0).clamp(0.0, 1.0)
    }
}

/// `\ko` outline rule: the outline is suppressed *before* the run
/// begins (`elapsed < start`, secondary fill + no outline) and becomes
/// visible from the exact start instant (primary fill + normal outline).
pub(super) fn karaoke_outline_suppressed(elapsed_ms: u64, start_ms: u64) -> bool {
    elapsed_ms < start_ms
}

/// Leading karaoke state a segment's tag group contributes, mirroring
/// libass's per-glyph effect fields at segment granularity: the state
/// the segment's first emitted glyph would carry. Empty (non-emitting)
/// segments pass accumulated state through untouched.
#[derive(Debug, Clone, Copy, Default)]
struct KaraokeLead {
    kind: Option<KaraokeKind>,
    /// `\k` duration in ms (`effect_timing`; nonzero starts a new run).
    dur_ms: u64,
    /// Accumulated dead time in ms (`effect_skip_timing`).
    skip_ms: u64,
    /// `\kt` clock reset (`reset_effect`).
    reset: bool,
}

/// One piece of a karaoke run: a (segment, line) span. Runs never cross
/// event lines, style keys, drawings, or nonzero `\k` durations.
#[derive(Debug, Clone)]
struct RunPiece {
    seg_idx: usize,
    /// Shaper row (`glyph.y`) for text pieces; `None` for drawings and
    /// empty leading pieces.
    line_y: Option<f64>,
    event_line: usize,
    /// Full pen width of this piece (max glyph edge / drawing width).
    width: f64,
    /// Leading/trailing whitespace trim (visible-span computation).
    trim_front: f64,
    trim_back: f64,
    lead: KaraokeLead,
    is_drawing: bool,
}

/// One karaoke run: maximal same-style, same-line span (libass
/// `starts_new_run` semantics) with its timing window. Sweep
/// interpolation, pop timing, and the frz fill-flip are all per-run.
#[derive(Debug, Clone)]
pub(super) struct KaraokeRun {
    pub(super) kind: KaraokeKind,
    /// Window `[start_ms, end_ms)`; `end_ms == start_ms` pops.
    pub(super) start_ms: u64,
    pub(super) end_ms: u64,
    /// Visible sweep span in layout px (whitespace-trimmed).
    pub(super) span: f64,
    /// `\frz` in (90, 270): mirror the sweep and swap the colors.
    pub(super) flip: bool,
    /// True when this run interpolates a sweep (vs whole-run pop).
    pub(super) sweep: bool,
    /// Last member glyph `(seg_idx, glyph_idx)` for buffer flushing;
    /// `None` for runs with no text glyphs (timing only / drawings).
    pub(super) last_glyph: Option<(usize, usize)>,
    /// First member glyph `(seg_idx, glyph_idx)` for baseline-shear
    /// resets (libass `apply_baseline_shear` restarts the accumulator
    /// at every run start); `None` when the run has no text glyphs.
    pub(super) first_glyph: Option<(usize, usize)>,
}

impl KaraokeRun {
    /// Sweep fraction at `elapsed_ms`: 0 before the window, 1 from its
    /// end, linear inside. Only meaningful when `sweep` is true.
    pub(super) fn sweep_frac(&self, elapsed_ms: u64) -> f64 {
        if elapsed_ms < self.start_ms {
            0.0
        } else if self.end_ms <= self.start_ms || elapsed_ms >= self.end_ms {
            1.0
        } else {
            (elapsed_ms - self.start_ms) as f64 / (self.end_ms - self.start_ms) as f64
        }
    }
}

/// Cap on buffered sweep bitmaps per event render (bytes of coverage).
/// Legit runs hold kilobytes; past this, remaining members fall back to
/// whole-glyph midpoint coloring against the flushed edge.
pub(super) const MAX_SWEEP_BUFFER_BYTES: usize = 16 << 20;

/// One transformed glyph awaiting its run's device-space sweep split.
/// All placement and color state is captured so the flush paints in
/// document order with no re-transform.
pub(super) struct BufferedSweepGlyph {
    pub(super) bitmap: Vec<u8>,
    pub(super) w: u32,
    pub(super) h: u32,
    pub(super) gx: i32,
    pub(super) gy: i32,
    pub(super) primary: [u8; 4],
    pub(super) primary_alpha: u8,
    pub(super) secondary: [u8; 4],
    pub(super) secondary_alpha: u8,
    /// Outline `(rgba, radius_x, radius_y)` when active.
    pub(super) outline: Option<([u8; 4], f64, f64)>,
    /// Shadow `(rgba, offset_x, offset_y)` when active.
    pub(super) shadow: Option<([u8; 4], f64, f64)>,
}

/// Buffer for the in-window sweep run currently being transformed.
/// libass splits the run's transformed bitmaps at one device-space x,
/// so the run's ink left edge must be known before any member draws.
/// The buffer holds at most one run (runs are contiguous in document
/// order) and flushes at the run's last glyph, preserving exact
/// paint order.
pub(super) struct SweepState {
    pub(super) buf: Vec<BufferedSweepGlyph>,
    pub(super) bytes: usize,
    pub(super) run: Option<usize>,
    /// Degraded run `(id, edge, flip)`: members past the buffer cap
    /// paint whole-glyph by center against the flushed edge.
    pub(super) degraded: Option<(usize, i64, bool)>,
}

impl SweepState {
    pub(super) fn new() -> Self {
        Self {
            buf: Vec::new(),
            bytes: 0,
            run: None,
            degraded: None,
        }
    }

    /// Drop the degraded fallback when leaving its run.
    pub(super) fn note_run(&mut self, run_id: Option<usize>) {
        if self.degraded.map(|(id, _, _)| id) != run_id {
            self.degraded = None;
        }
    }

    /// Paint buffered members, split at the run's device-space edge:
    /// `round(ink_left + frac * span)` (mirrored when `flip`), with a
    /// hard boundary (verified against ffmpeg/libass probes: adjacent
    /// primary/secondary columns, outline unsplit). No-op when empty.
    pub(super) fn flush(
        &mut self,
        buffer: &mut RenderBuffer,
        runs: &[KaraokeRun],
        elapsed_ms: u64,
        fade_alpha: u8,
    ) {
        let run_id = self.run.take();
        if self.buf.is_empty() {
            self.bytes = 0;
            return;
        }
        let Some(run_id) = run_id else {
            self.buf.clear();
            self.bytes = 0;
            return;
        };
        let Some(run) = runs.get(run_id) else {
            self.buf.clear();
            self.bytes = 0;
            return;
        };
        // Device ink bounds over the transformed bitmaps.
        let mut ink: Option<(i64, i64)> = None;
        for glyph in &self.buf {
            let (mut first, mut last) = (u32::MAX, 0u32);
            for (idx, coverage) in glyph.bitmap.iter().enumerate() {
                if *coverage > 0 {
                    let px = (idx as u64 % u64::from(glyph.w.max(1))) as u32;
                    first = first.min(px);
                    last = last.max(px);
                }
            }
            if first != u32::MAX {
                let left = i64::from(glyph.gx) + i64::from(first);
                let right = i64::from(glyph.gx) + i64::from(last) + 1;
                ink = Some(match ink {
                    Some((lo, hi)) => (lo.min(left), hi.max(right)),
                    None => (left, right),
                });
            }
        }
        // `span` is layout units, which match device pixels 1:1 here
        // (shaping already absorbed resolution scale and `\fsc`).
        let edge = match ink {
            Some((left, _)) if run.span.is_finite() && run.span >= 0.0 => {
                let offset = run.sweep_frac(elapsed_ms) * run.span;
                let raw = if run.flip {
                    left as f64 + run.span - offset
                } else {
                    left as f64 + offset
                };
                raw.round().clamp(i64::MIN as f64, i64::MAX as f64) as i64
            }
            // Blank run (whitespace only): nothing paints anyway.
            _ => ink.map(|(left, _)| left).unwrap_or(0),
        };
        for glyph in self.buf.drain(..) {
            if let Some((rgba, rx, ry)) = glyph.outline {
                effects::apply_outline_xy(
                    buffer,
                    &glyph.bitmap,
                    glyph.w,
                    glyph.h,
                    glyph.gx,
                    glyph.gy,
                    rx,
                    ry,
                    rgba,
                );
            }
            if let Some((rgba, ox, oy)) = glyph.shadow {
                effects::apply_shadow(
                    buffer,
                    &glyph.bitmap,
                    glyph.w,
                    glyph.h,
                    glyph.gx,
                    glyph.gy,
                    ox,
                    oy,
                    rgba,
                );
            }
            let (primary, primary_alpha) = (glyph.primary, glyph.primary_alpha);
            let (secondary, secondary_alpha) = (glyph.secondary, glyph.secondary_alpha);
            let flip = run.flip;
            let geom = GlyphGeom {
                w: glyph.w,
                h: glyph.h,
                gx: glyph.gx,
                gy: glyph.gy,
            };
            paint_glyph_fill(buffer, &glyph.bitmap, geom, fade_alpha, |px| {
                let left_side = i64::from(glyph.gx) + i64::from(px) < edge;
                if left_side != flip {
                    (primary, primary_alpha)
                } else {
                    (secondary, secondary_alpha)
                }
            });
        }
        self.bytes = 0;
        // A degraded run keeps its edge for whole-glyph members.
        if self.degraded.map(|(id, _, _)| id) == Some(run_id) {
            self.degraded = Some((run_id, edge, run.flip));
        }
    }
}

/// Placement of one transformed coverage bitmap: size + device origin.
#[derive(Debug, Clone, Copy)]
pub(super) struct GlyphGeom {
    pub(super) w: u32,
    pub(super) h: u32,
    pub(super) gx: i32,
    pub(super) gy: i32,
}

/// Paint one transformed glyph's fill: per-pixel coverage blend, the
/// color per bitmap column chosen by `pick` (solid fill and karaoke
/// splits share this path, so blending can never diverge).
pub(super) fn paint_glyph_fill(
    buffer: &mut RenderBuffer,
    bitmap: &[u8],
    geom: GlyphGeom,
    fade_alpha: u8,
    pick: impl Fn(u32) -> ([u8; 4], u8),
) {
    for py in 0..geom.h {
        for px in 0..geom.w {
            let idx = (u64::from(py) * u64::from(geom.w) + u64::from(px)) as usize;
            let coverage = bitmap.get(idx).copied().unwrap_or(0);
            if coverage > 0 {
                let (color, color_alpha) = pick(px);
                let a = ((u32::from(coverage) * u32::from(color_alpha) / 255)
                    * u32::from(fade_alpha)
                    / 255) as u8;
                // Widen through i64 and bounds-check before u32
                // conversion: never wrap i32 or rely on casts.
                let (Some(sx), Some(sy)) = (
                    add_coord(geom.gx, px, buffer.width),
                    add_coord(geom.gy, py, buffer.height),
                ) else {
                    continue;
                };
                buffer.blend_pixel(sx, sy, color[0], color[1], color[2], a);
            }
        }
    }
}

/// Sanitize a measured layout width for sweep math: finite and
/// non-negative, else 0.
fn clean_width(v: f64) -> f64 {
    if v.is_finite() {
        v.max(0.0)
    } else {
        0.0
    }
}

/// Karaoke run build product: the runs, a per-segment per-glyph run
/// assignment (by glyph index into the shaped line), and a
/// per-segment drawing run assignment. `None` means "no karaoke here"
/// (normal rendering).
pub(super) type KaraokeBuild = (Vec<KaraokeRun>, Vec<Vec<Option<usize>>>, Vec<Option<usize>>);

/// Build karaoke runs for segmented event text, replicating libass
/// (`split_style_runs` + `ass_process_karaoke_effects`, verified against
/// ffmpeg-rendered probes):
///
/// * Tag groups accumulate effect state in order: `\kt` assigns skip
///   and resets, each `\k`-family tag adds the previous duration to
///   skip and sets the new duration (stacked tags accumulate dead
///   time). State clears at the next emitting segment, so `\k0`
///   mid-run adds skip without breaking the run.
/// * Runs break at nonzero durations, effect-type changes, style-key
///   changes, drawings, and event-line changes. Later runs in a
///   syllable pop at the window end instead of sweeping; `\N` runs
///   consume timing invisibly.
/// * Sweep spans exclude trimmed leading/trailing ASCII spaces.
pub(super) fn build_karaoke_runs(segments: &[TextSegment], items: &[LayoutItem]) -> KaraokeBuild {
    // ---- Pass 1: accumulate tag state, cut (segment, line) pieces. ----
    let mut pieces: Vec<RunPiece> = Vec::new();
    let mut seg_pieces: Vec<Vec<usize>> = vec![Vec::new(); segments.len()];
    let mut pending = KaraokeLead::default();
    let mut prev_tag_count = 0usize;
    // Event-line tracker mirroring the render loop: leading breaks and
    // internal row changes advance, trailing breaks advance at the end.
    let mut event_line = 0usize;

    for (seg_idx, segment) in segments.iter().enumerate() {
        let from = prev_tag_count.min(segment.tags.len());
        fn consume(tag: &OverrideTag, pending: &mut KaraokeLead) {
            match tag {
                // `\kt` assigns (wiping stacked durations) and resets.
                OverrideTag::KaraokeStart(t) => {
                    pending.skip_ms = t.saturating_mul(10);
                    pending.dur_ms = 0;
                    pending.reset = true;
                }
                // Each `\k` banks the previous duration as skip, then
                // takes over (last tag in the group wins the duration).
                OverrideTag::KaraokeDuration(d) => {
                    pending.skip_ms = pending.skip_ms.saturating_add(pending.dur_ms);
                    pending.dur_ms = d.saturating_mul(10);
                    pending.kind = Some(KaraokeKind::Hard);
                }
                OverrideTag::KaraokeSweep(d) => {
                    pending.skip_ms = pending.skip_ms.saturating_add(pending.dur_ms);
                    pending.dur_ms = d.saturating_mul(10);
                    pending.kind = Some(KaraokeKind::Sweep);
                }
                OverrideTag::KaraokeOutline(d) => {
                    pending.skip_ms = pending.skip_ms.saturating_add(pending.dur_ms);
                    pending.dur_ms = d.saturating_mul(10);
                    pending.kind = Some(KaraokeKind::Outline);
                }
                OverrideTag::KaraokeTiming { mode, millis } => {
                    pending.skip_ms = pending.skip_ms.saturating_add(pending.dur_ms);
                    pending.dur_ms = (*millis).max(0) as u64;
                    pending.kind = Some(match mode {
                        1 => KaraokeKind::Sweep,
                        2 => KaraokeKind::Outline,
                        _ => KaraokeKind::Hard,
                    });
                }
                OverrideTag::Transform { tags, .. } => {
                    for nested in tags {
                        consume(nested, pending);
                    }
                }
                _ => {}
            }
        }
        for tag in &segment.tags[from..] {
            consume(tag, &mut pending);
        }
        prev_tag_count = segment.tags.len();

        let item = match items.get(seg_idx) {
            Some(item) => item,
            None => continue,
        };
        let emits = !segment.text.is_empty() || item.drawing.is_some();
        if !emits {
            // Tag carrier: state passes through uncleared.
            continue;
        }
        let mut first_piece = true;
        let mut take_lead = || {
            if first_piece {
                first_piece = false;
                std::mem::take(&mut pending)
            } else {
                KaraokeLead::default()
            }
        };

        if item.drawing.is_some() {
            let width = clean_width(item.drawing.as_ref().map(|d| d.width).unwrap_or(0.0));
            let pid = pieces.len();
            pieces.push(RunPiece {
                seg_idx,
                line_y: None,
                event_line,
                width,
                trim_front: 0.0,
                trim_back: 0.0,
                lead: take_lead(),
                is_drawing: true,
            });
            seg_pieces[seg_idx].push(pid);
        } else {
            // Group shaped glyphs by row (exact `y`, like the render
            // loop's row marker).
            let mut rows: Vec<(f64, Vec<usize>)> = Vec::new();
            for (glyph_idx, glyph) in item.shaped.glyphs.iter().enumerate() {
                match rows.last_mut() {
                    Some((y, idxs)) if *y == glyph.y => idxs.push(glyph_idx),
                    _ => rows.push((glyph.y, vec![glyph_idx])),
                }
            }
            // A leading break means the state rides an empty first
            // piece (the `\n` glyph carries it in libass).
            if segment.text.starts_with('\n') {
                let pid = pieces.len();
                pieces.push(RunPiece {
                    seg_idx,
                    line_y: None,
                    event_line,
                    width: 0.0,
                    trim_front: 0.0,
                    trim_back: 0.0,
                    lead: take_lead(),
                    is_drawing: false,
                });
                seg_pieces[seg_idx].push(pid);
            }
            let leading = segment.text.chars().take_while(|c| *c == '\n').count();
            event_line = event_line.saturating_add(leading);
            for (row_pos, (y, idxs)) in rows.iter().enumerate() {
                if row_pos > 0 {
                    event_line = event_line.saturating_add(1);
                }
                // Pen width mirrors the render loop's `row_pen`: max
                // edge over rendered (non-zero-scale) glyphs.
                let mut pen = 0.0f64;
                for &glyph_idx in idxs {
                    let glyph = &item.shaped.glyphs[glyph_idx];
                    if glyph.scale_x > 0.0 && glyph.scale_y > 0.0 {
                        let edge = glyph.x + glyph.advance;
                        if edge.is_finite() && edge > pen {
                            pen = edge;
                        }
                    }
                }
                // Visible span trims ASCII spaces (libass
                // `IS_WHITESPACE`: space and newline only).
                let mut front = pen;
                let mut back = 0.0f64;
                let mut first_visible: Option<f64> = None;
                let mut last_visible_end = 0.0f64;
                for &glyph_idx in idxs {
                    let glyph = &item.shaped.glyphs[glyph_idx];
                    if glyph.ch != ' ' {
                        if first_visible.is_none() {
                            first_visible = Some(glyph.x);
                        }
                        let end = glyph.x + glyph.advance;
                        if end.is_finite() {
                            last_visible_end = last_visible_end.max(end);
                        }
                    }
                }
                if let Some(start) = first_visible {
                    if start.is_finite() {
                        front = start.max(0.0).min(pen);
                    }
                    back = (pen - last_visible_end).max(0.0);
                }
                let pid = pieces.len();
                pieces.push(RunPiece {
                    seg_idx,
                    line_y: Some(*y),
                    event_line,
                    width: clean_width(pen),
                    trim_front: clean_width(front),
                    trim_back: clean_width(back),
                    lead: take_lead(),
                    is_drawing: false,
                });
                seg_pieces[seg_idx].push(pid);
            }
        }
        if segment.text.ends_with('\n') {
            event_line = event_line.saturating_add(1);
        }
        // Emitted: any state not taken by a first piece is dropped
        // (a segment always takes it on its first piece, so this only
        // clears when a segment somehow produced no pieces).
        pending = KaraokeLead::default();
    }

    // ---- Pass 2: group pieces into runs (libass run breaks). ----
    let mut run_of_piece: Vec<Option<usize>> = vec![None; pieces.len()];
    let mut run_pieces: Vec<Vec<usize>> = Vec::new();
    let mut last_kind: Option<KaraokeKind> = None;
    for (pid, piece) in pieces.iter().enumerate() {
        let breaks = if pid == 0 {
            true
        } else {
            let prev = &pieces[pid - 1];
            piece.lead.dur_ms > 0
                || (piece.lead.kind.is_some() && piece.lead.kind != last_kind)
                || piece.is_drawing
                || prev.is_drawing
                || piece.event_line != prev.event_line
                || !items[piece.seg_idx]
                    .resolved
                    .same_karaoke_run(&items[prev.seg_idx].resolved)
        };
        if piece.lead.kind.is_some() {
            last_kind = piece.lead.kind;
        }
        if breaks || run_pieces.is_empty() {
            run_pieces.push(Vec::new());
        }
        let run_idx = run_pieces.len() - 1;
        run_pieces[run_idx].push(pid);
        run_of_piece[pid] = Some(run_idx);
    }

    // ---- Pass 3: timing per run (`ass_process_karaoke_effects`). ----
    let mut runs: Vec<KaraokeRun> = Vec::new();
    let mut run_id_of_group: Vec<Option<usize>> = vec![None; run_pieces.len()];
    let mut clock = 0u64;
    let mut skip_accum = 0u64;
    let mut effect: Option<KaraokeKind> = None;
    let mut has_reset = false;
    for (group_idx, group) in run_pieces.iter().enumerate() {
        let start_lead = pieces[group[0]].lead;
        if start_lead.kind.is_some() {
            effect = start_lead.kind;
        }
        // Fold non-start pieces' state (persists even through runs
        // without karaoke, exactly like upstream's skip_timing).
        let mut fold_piece = |lead: KaraokeLead| {
            if lead.reset {
                has_reset = true;
                skip_accum = 0;
            }
            skip_accum = skip_accum.saturating_add(lead.skip_ms);
        };
        if effect.is_none() {
            for &pid in &group[1..] {
                fold_piece(pieces[pid].lead);
            }
            continue;
        }
        if start_lead.reset {
            clock = 0;
        }
        let tm_start = clock.saturating_add(start_lead.skip_ms);
        let tm_end = tm_start.saturating_add(start_lead.dur_ms);
        for &pid in &group[1..] {
            fold_piece(pieces[pid].lead);
        }
        clock = (if has_reset { 0 } else { tm_end }).saturating_add(skip_accum);
        has_reset = false;
        skip_accum = 0;

        let kind = effect.unwrap_or(KaraokeKind::Hard);
        let end_eff = if kind == KaraokeKind::Sweep {
            tm_end
        } else {
            tm_start
        };
        let span: f64 = group.iter().map(|&pid| pieces[pid].width).sum::<f64>() + 0.0;
        let front = pieces[group[0]].trim_front;
        let back = pieces[group[group.len() - 1]].trim_back;
        let span = clean_width(span - front - back);
        let angle = items[pieces[group[0]].seg_idx].resolved.angle;
        // Euclidean modulo: equivalent rotations (e.g. -170 and 190)
        // flip identically.
        let frz = angle.rem_euclid(360.0);
        let flip = frz > 90.0 && frz < 270.0;

        // Last member glyph (for sweep-buffer flushing): scan member
        // pieces in reverse for the last text piece with glyphs.
        let mut last_glyph = None;
        for &pid in group.iter().rev() {
            let piece = &pieces[pid];
            if let Some(y) = piece.line_y {
                if let Some(item) = items.get(piece.seg_idx) {
                    if let Some((glyph_idx, _)) = item
                        .shaped
                        .glyphs
                        .iter()
                        .enumerate()
                        .rev()
                        .find(|(_, glyph)| glyph.y == y)
                    {
                        last_glyph = Some((piece.seg_idx, glyph_idx));
                        break;
                    }
                }
            }
        }

        // First member glyph (for shear resets): scan member
        // pieces forward for the first text piece with glyphs.
        let mut first_glyph = None;
        for &pid in group.iter() {
            let piece = &pieces[pid];
            if let Some(y) = piece.line_y {
                if let Some(item) = items.get(piece.seg_idx) {
                    if let Some((glyph_idx, _)) = item
                        .shaped
                        .glyphs
                        .iter()
                        .enumerate()
                        .find(|(_, glyph)| glyph.y == y)
                    {
                        first_glyph = Some((piece.seg_idx, glyph_idx));
                        break;
                    }
                }
            }
        }

        let run_id = runs.len();
        run_id_of_group[group_idx] = Some(run_id);
        runs.push(KaraokeRun {
            kind,
            start_ms: tm_start,
            end_ms: end_eff,
            span,
            flip,
            sweep: kind == KaraokeKind::Sweep && tm_end > tm_start,
            last_glyph,
            first_glyph,
        });
    }

    // ---- Pass 4: glyph/drawing assignment. ----
    let mut glyph_run: Vec<Vec<Option<usize>>> = segments
        .iter()
        .enumerate()
        .map(|(seg_idx, _)| {
            items
                .get(seg_idx)
                .map(|item| vec![None; item.shaped.glyphs.len()])
                .unwrap_or_default()
        })
        .collect();
    let mut drawing_run: Vec<Option<usize>> = vec![None; segments.len()];
    for (pid, piece) in pieces.iter().enumerate() {
        let Some(group_idx) = run_of_piece[pid] else {
            continue;
        };
        let run_id = run_id_of_group[group_idx];
        if piece.is_drawing {
            drawing_run[piece.seg_idx] = run_id;
        } else if let Some(y) = piece.line_y {
            if let Some(item) = items.get(piece.seg_idx) {
                for (glyph_idx, glyph) in item.shaped.glyphs.iter().enumerate() {
                    if glyph.y == y {
                        glyph_run[piece.seg_idx][glyph_idx] = run_id;
                    }
                }
            }
        }
    }

    (runs, glyph_run, drawing_run)
}
