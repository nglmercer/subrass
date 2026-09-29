//! Filter coverage masks before applying color; run-local state never touches
//! another run or a previously composited event.
use super::super::{
    karaoke::{paint_glyph_fill, GlyphGeom},
    ResolvedStyle,
};
use crate::renderer::buffer::{checked_pixel_count, RenderBuffer, MAX_GLYPH_BITMAP_PIXELS};
use crate::renderer::effects;

#[derive(Clone, Copy, Default)]
pub(crate) struct MaskBlur {
    pub edge: u32,
    pub x: f64,
    pub y: f64,
}
impl MaskBlur {
    pub(super) fn from_style(r: &ResolvedStyle, sx: f64, sy: f64) -> Self {
        Self {
            edge: r.edge_blur,
            x: r.blur * sx,
            y: r.blur * sy,
        }
    }
    fn active(self) -> bool {
        self.edge > 0 || self.x > 0.0 || self.y > 0.0
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn paint(
    buffer: &mut RenderBuffer,
    bitmap: &[u8],
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    alpha: u8,
    fill: ([u8; 4], u8),
    outline: Option<([u8; 4], f64, f64)>,
    shadow: Option<([u8; 4], f64, f64)>,
    blur: MaskBlur,
) {
    paint_split(
        buffer,
        bitmap,
        w,
        h,
        x,
        y,
        alpha,
        |_| fill,
        outline,
        shadow,
        blur,
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_split(
    buffer: &mut RenderBuffer,
    bitmap: &[u8],
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    alpha: u8,
    fill: impl Fn(u32) -> ([u8; 4], u8),
    outline: Option<([u8; 4], f64, f64)>,
    shadow: Option<([u8; 4], f64, f64)>,
    blur: MaskBlur,
) {
    if !blur.active() {
        if let Some((c, ox, oy)) = outline {
            effects::apply_outline_xy(buffer, bitmap, w, h, x, y, ox, oy, c);
        }
        if let Some((c, ox, oy)) = shadow {
            effects::apply_shadow(buffer, bitmap, w, h, x, y, ox, oy, c);
        }
        paint_glyph_fill(
            buffer,
            bitmap,
            GlyphGeom { w, h, gx: x, gy: y },
            alpha,
            fill,
        );
        return;
    }
    if !crate::renderer::buffer::bitmap_has_pixels(bitmap, w, h) {
        return;
    }
    let border = outline
        .map(|(_, ox, oy)| ox.max(oy).clamp(0.0, 128.0))
        .unwrap_or(0.0);
    let spread = (blur.x.max(blur.y).clamp(0.0, 128.0) * 3.0).ceil() as u32 + blur.edge.min(127);
    let pad = spread
        .saturating_add(border.ceil() as u32)
        .saturating_add(1);
    let geometry = w
        .checked_add(2 * pad)
        .zip(h.checked_add(2 * pad))
        .and_then(|(pw, ph)| checked_pixel_count(pw, ph).map(|len| (pw, ph, len)))
        .filter(|(_, _, len)| *len as u64 <= MAX_GLYPH_BITMAP_PIXELS);
    let Some((pw, ph, len)) = geometry else {
        // Keep visible ink when filter padding exceeds the glyph budget.
        // Degrade this run to the existing unfiltered painter, never allocate
        // from untrusted dimensions or silently discard a valid glyph.
        paint_split(
            buffer,
            bitmap,
            w,
            h,
            x,
            y,
            alpha,
            fill,
            outline,
            shadow,
            MaskBlur::default(),
        );
        return;
    };
    let mut mask = vec![0; len];
    for row in 0..h as usize {
        let dst = (row + pad as usize) * pw as usize + pad as usize;
        mask[dst..dst + w as usize]
            .copy_from_slice(&bitmap[row * w as usize..(row + 1) * w as usize]);
    }
    let (gx, gy) = (x.saturating_sub(pad as i32), y.saturating_sub(pad as i32));
    let mut border_mask = outline.and_then(|(_, ox, oy)| {
        let mut temp = RenderBuffer::new(pw, ph).ok()?;
        effects::apply_outline_xy(
            &mut temp, bitmap, w, h, pad as i32, pad as i32, ox, oy, [255; 4],
        );
        Some(
            temp.pixels
                .chunks_exact(4)
                .map(|p| p[3])
                .collect::<Vec<u8>>(),
        )
    });
    // libass filters fill only without a border; an outlined fill stays crisp.
    if border_mask.is_none() {
        filter_mask(&mut mask, pw, ph, blur);
    }
    if let Some(m) = &mut border_mask {
        filter_mask(m, pw, ph, blur);
    }
    if let Some((c, ox, oy)) = shadow {
        effects::apply_shadow(
            buffer,
            border_mask.as_deref().unwrap_or(&mask),
            pw,
            ph,
            gx,
            gy,
            ox,
            oy,
            c,
        );
    }
    if let (Some((c, _, _)), Some(m)) = (outline, border_mask) {
        paint_glyph_fill(
            buffer,
            &m,
            GlyphGeom {
                w: pw,
                h: ph,
                gx,
                gy,
            },
            255,
            |_| (c, c[3]),
        );
    }
    paint_glyph_fill(
        buffer,
        &mask,
        GlyphGeom {
            w: pw,
            h: ph,
            gx,
            gy,
        },
        alpha,
        |px| fill(px.saturating_sub(pad)),
    );
}

/// Gaussian convolution (`sigma = blur * 2/sqrt(ln 256)`) followed by `be`
/// passes of the [1,2,1] x [1,2,1] / 16 kernel. Transparent padding is never
/// renormalized at edges. Radius/pass caps retain the renderer's resource policy.
fn filter_mask(mask: &mut [u8], w: u32, h: u32, blur: MaskBlur) {
    for (horizontal, radius) in [(true, blur.x), (false, blur.y)] {
        if !radius.is_finite() || radius <= 0.0 {
            continue;
        }
        let sigma = radius.clamp(0.0, 128.0) * 2.0 / 256f64.ln().sqrt();
        if sigma > 6.0 {
            // Three variance-matched boxes approximate a wide Gaussian in
            // linear time. Direct convolution's radius-dependent work would
            // make hostile large blur values much slower than the old path.
            let ideal = (4.0 * sigma * sigma + 1.0).sqrt();
            let mut lower = ideal.floor() as u32;
            if lower.is_multiple_of(2) {
                lower = lower.saturating_sub(1);
            }
            let low = f64::from(lower);
            let count = ((12.0 * sigma * sigma - 3.0 * low * low - 12.0 * low - 9.0)
                / (-4.0 * low - 4.0))
                .round()
                .clamp(0.0, 3.0) as u32;
            for pass in 0..3 {
                let radius = if pass < count {
                    lower / 2
                } else {
                    lower / 2 + 1
                };
                box_filter(
                    mask,
                    w,
                    h,
                    horizontal,
                    radius.min(crate::renderer::limits::MAX_BLUR_RADIUS),
                );
            }
            continue;
        }
        let support = (sigma * 3.0).ceil().clamp(1.0, 128.0) as i32;
        let mut kernel: Vec<f64> = (-support..=support)
            .map(|d| (-f64::from(d).powi(2) / (2.0 * sigma * sigma)).exp())
            .collect();
        let sum: f64 = kernel.iter().sum();
        for v in &mut kernel {
            *v /= sum;
        }
        convolve(mask, w, h, horizontal, &kernel);
    }
    let passes = blur.edge.min(127);
    if passes > 1 {
        // libass uses 6-bit coverage for all but the final pass to avoid
        // rounding away low intensity edges after many iterations.
        for v in mask.iter_mut() {
            *v = ((*v >> 1) + 1) >> 1;
        }
        for _ in 1..passes {
            edge_filter(mask, w, h);
        }
        for v in mask.iter_mut() {
            *v = v.saturating_mul(4).saturating_sub(u8::from(*v > 32));
        }
    }
    if passes > 0 {
        edge_filter(mask, w, h);
    }
}

fn box_filter(mask: &mut [u8], w: u32, h: u32, horizontal: bool, radius: u32) {
    let source = mask.to_vec();
    let (lines, length) = if horizontal { (h, w) } else { (w, h) };
    let index = |line: u32, position: u32| {
        if horizontal {
            line as usize * w as usize + position as usize
        } else {
            position as usize * w as usize + line as usize
        }
    };
    let denominator = f64::from(2 * radius + 1);
    for line in 0..lines {
        let mut sum = 0u32;
        for p in 0..length.min(radius + 1) {
            sum += u32::from(source[index(line, p)]);
        }
        for p in 0..length {
            mask[index(line, p)] = (f64::from(sum) / denominator).round() as u8;
            if p >= radius {
                sum -= u32::from(source[index(line, p - radius)]);
            }
            if let Some(next) = p.checked_add(radius + 1).filter(|n| *n < length) {
                sum += u32::from(source[index(line, next)]);
            }
        }
    }
}

fn edge_filter(mask: &mut [u8], w: u32, h: u32) {
    let mut horizontal = vec![0u16; mask.len()];
    for y in 0..h as usize {
        for x in 0..w as usize {
            let i = y * w as usize + x;
            horizontal[i] = 2 * u16::from(mask[i])
                + if x > 0 { u16::from(mask[i - 1]) } else { 0 }
                + if x + 1 < w as usize {
                    u16::from(mask[i + 1])
                } else {
                    0
                };
        }
    }
    for y in 0..h as usize {
        for x in 0..w as usize {
            let i = y * w as usize + x;
            let sum = 2 * horizontal[i]
                + if y > 0 { horizontal[i - w as usize] } else { 0 }
                + if y + 1 < h as usize {
                    horizontal[i + w as usize]
                } else {
                    0
                };
            mask[i] = (sum >> 4) as u8;
        }
    }
}

fn convolve(mask: &mut [u8], w: u32, h: u32, horizontal: bool, kernel: &[f64]) {
    let src = mask.to_vec();
    let half = kernel.len() as i64 / 2;
    for y in 0..h {
        for x in 0..w {
            let mut sum = 0.0;
            for (i, weight) in kernel.iter().enumerate() {
                let d = i as i64 - half;
                let (sx, sy) = if horizontal {
                    (i64::from(x) + d, i64::from(y))
                } else {
                    (i64::from(x), i64::from(y) + d)
                };
                if sx >= 0 && sx < i64::from(w) && sy >= 0 && sy < i64::from(h) {
                    sum += f64::from(src[sy as usize * w as usize + sx as usize]) * weight;
                }
            }
            mask[y as usize * w as usize + x as usize] = sum.round().clamp(0.0, 255.0) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edge_filter_is_binomial_and_independent_of_gaussian() {
        let mut edge = vec![0; 49];
        edge[24] = 255;
        filter_mask(
            &mut edge,
            7,
            7,
            MaskBlur {
                edge: 1,
                ..Default::default()
            },
        );
        assert_eq!(edge[24], 63);
        assert_eq!(edge[23], 31);
        assert_eq!(edge[16], 15);
        assert_eq!(edge[22], 0);
        let mut gaussian = vec![0; 49];
        gaussian[24] = 255;
        filter_mask(
            &mut gaussian,
            7,
            7,
            MaskBlur {
                edge: 0,
                x: 1.0,
                y: 1.0,
            },
        );
        assert_ne!(edge, gaussian);
        let mut both = vec![0; 49];
        both[24] = 255;
        filter_mask(
            &mut both,
            7,
            7,
            MaskBlur {
                edge: 1,
                x: 1.0,
                y: 1.0,
            },
        );
        assert_ne!(both, gaussian);
        assert_ne!(both, edge);
    }
}

/// Contiguous text glyphs with identical style/effect state share a coverage
/// mask, like libass's combined bitmaps. Flush before style changes, drawings,
/// line breaks, and karaoke sweeps; never collect an entire event blindly.
#[derive(Default)]
pub(super) struct BlurRun {
    parts: Vec<RunGlyph>,
    style: Option<ResolvedStyle>,
    bytes: usize,
}
struct RunGlyph {
    bitmap: Vec<u8>,
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    alpha: u8,
    fill: ([u8; 4], u8),
    outline: Option<([u8; 4], f64, f64)>,
    shadow: Option<([u8; 4], f64, f64)>,
    blur: MaskBlur,
}
impl BlurRun {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn push(
        &mut self,
        buffer: &mut RenderBuffer,
        bitmap: Vec<u8>,
        w: u32,
        h: u32,
        x: i32,
        y: i32,
        alpha: u8,
        fill: ([u8; 4], u8),
        outline: Option<([u8; 4], f64, f64)>,
        shadow: Option<([u8; 4], f64, f64)>,
        blur: MaskBlur,
        style: &ResolvedStyle,
    ) {
        let same = self
            .style
            .as_ref()
            .is_some_and(|r| r.same_karaoke_run(style))
            && self.parts.last().is_none_or(|p| {
                p.fill == fill && p.outline == outline && p.shadow == shadow && p.alpha == alpha
            });
        if !same
            || !blur.active()
            || self.bytes.saturating_add(bitmap.len())
                > super::super::karaoke::MAX_SWEEP_BUFFER_BYTES
        {
            self.flush(buffer);
        }
        if !blur.active() {
            paint(
                buffer, &bitmap, w, h, x, y, alpha, fill, outline, shadow, blur,
            );
            return;
        }
        self.style = Some(style.clone());
        self.bytes = self.bytes.saturating_add(bitmap.len());
        self.parts.push(RunGlyph {
            bitmap,
            w,
            h,
            x,
            y,
            alpha,
            fill,
            outline,
            shadow,
            blur,
        });
    }

    pub(super) fn flush(&mut self, buffer: &mut RenderBuffer) {
        if self.parts.is_empty() {
            return;
        }
        let x0 = self.parts.iter().map(|p| i64::from(p.x)).min().unwrap();
        let y0 = self.parts.iter().map(|p| i64::from(p.y)).min().unwrap();
        let x1 = self
            .parts
            .iter()
            .map(|p| i64::from(p.x) + i64::from(p.w))
            .max()
            .unwrap();
        let y1 = self
            .parts
            .iter()
            .map(|p| i64::from(p.y) + i64::from(p.h))
            .max()
            .unwrap();
        let dims = u32::try_from(x1 - x0).ok().zip(u32::try_from(y1 - y0).ok());
        let size = dims.and_then(|(w, h)| checked_pixel_count(w, h).map(|len| (w, h, len)));
        if let Some((w, h, len)) = size.filter(|(_, _, len)| *len as u64 <= MAX_GLYPH_BITMAP_PIXELS)
        {
            let mut coverage = vec![0u8; len];
            for p in &self.parts {
                for row in 0..p.h as usize {
                    for col in 0..p.w as usize {
                        let dst = (i64::from(p.y) - y0 + row as i64) as usize * w as usize
                            + (i64::from(p.x) - x0 + col as i64) as usize;
                        coverage[dst] =
                            coverage[dst].saturating_add(p.bitmap[row * p.w as usize + col]);
                    }
                }
            }
            let p = &self.parts[0];
            paint(
                buffer, &coverage, w, h, x0 as i32, y0 as i32, p.alpha, p.fill, p.outline,
                p.shadow, p.blur,
            );
        } else {
            // Bounded fallback for extremely separated or enormous runs.
            for p in &self.parts {
                paint(
                    buffer, &p.bitmap, p.w, p.h, p.x, p.y, p.alpha, p.fill, p.outline, p.shadow,
                    p.blur,
                );
            }
        }
        self.parts.clear();
        self.style = None;
        self.bytes = 0;
    }
}

/// A karaoke sweep is one style run: filter its combined coverage, then
/// choose primary/secondary color at the device column boundary.
pub(crate) fn paint_sweep_run(
    buffer: &mut RenderBuffer,
    parts: &[super::super::karaoke::BufferedSweepGlyph],
    edge: i64,
    flip: bool,
    alpha: u8,
) -> bool {
    let Some(first) = parts.first() else {
        return false;
    };
    if !first.blur.active()
        || parts
            .iter()
            .any(|p| p.outline != first.outline || p.shadow != first.shadow)
    {
        return false;
    }
    let x0 = parts.iter().map(|p| i64::from(p.gx)).min().unwrap();
    let y0 = parts.iter().map(|p| i64::from(p.gy)).min().unwrap();
    let x1 = parts
        .iter()
        .map(|p| i64::from(p.gx) + i64::from(p.w))
        .max()
        .unwrap();
    let y1 = parts
        .iter()
        .map(|p| i64::from(p.gy) + i64::from(p.h))
        .max()
        .unwrap();
    let Some((w, h)) = u32::try_from(x1 - x0).ok().zip(u32::try_from(y1 - y0).ok()) else {
        return false;
    };
    let Some(len) = checked_pixel_count(w, h).filter(|n| *n as u64 <= MAX_GLYPH_BITMAP_PIXELS)
    else {
        return false;
    };
    let mut mask = vec![0u8; len];
    for p in parts {
        for y in 0..p.h as usize {
            for x in 0..p.w as usize {
                let dst = (i64::from(p.gy) - y0 + y as i64) as usize * w as usize
                    + (i64::from(p.gx) - x0 + x as i64) as usize;
                mask[dst] = mask[dst].saturating_add(p.bitmap[y * p.w as usize + x]);
            }
        }
    }
    paint_split(
        buffer,
        &mask,
        w,
        h,
        x0 as i32,
        y0 as i32,
        alpha,
        |x| {
            if (x0 + i64::from(x) < edge) != flip {
                (first.primary, first.primary_alpha)
            } else {
                (first.secondary, first.secondary_alpha)
            }
        },
        first.outline,
        first.shadow,
        first.blur,
    );
    true
}
