use crate::renderer::buffer::RenderBuffer;

/// Multiply the buffer alpha by a same-size mask's alpha channel
/// (vector `\clip`). With `inverse`, multiply by the inverted mask
/// (`\iclip`). Size mismatch: no-op.
pub fn apply_alpha_mask(buffer: &mut RenderBuffer, mask: &RenderBuffer, inverse: bool) {
    if buffer.width != mask.width || buffer.height != mask.height {
        return;
    }
    for (dst, m) in buffer
        .pixels
        .chunks_exact_mut(4)
        .zip(mask.pixels.chunks_exact(4))
    {
        let factor = if inverse { 255 - m[3] } else { m[3] };
        dst[3] = (dst[3] as u32 * factor as u32 / 255) as u8;
    }
}

/// Apply border style 3 (opaque box background): the event text block
/// grown by outline-derived padding on every side, filled with
/// `fill_color` as straight `[r, g, b, opacity]`.
#[allow(clippy::too_many_arguments)]
pub fn apply_opaque_box(
    buffer: &mut RenderBuffer,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    pad_x: i32,
    pad_y: i32,
    fill_color: [u8; 4],
) {
    let pad_x = pad_x.max(0);
    let pad_y = pad_y.max(0);
    let box_x = x.saturating_sub(pad_x);
    let box_y = y.saturating_sub(pad_y);
    let box_w = width.saturating_add(pad_x.saturating_mul(2));
    let box_h = height.saturating_add(pad_y.saturating_mul(2));

    buffer.fill_rect(
        box_x,
        box_y,
        box_w,
        box_h,
        fill_color[0],
        fill_color[1],
        fill_color[2],
        fill_color[3],
    );
}

fn normalize_clip(
    buffer: &RenderBuffer,
    clip_rect: (i32, i32, i32, i32),
) -> Option<(i64, i64, i64, i64)> {
    if buffer.width == 0 || buffer.height == 0 {
        return None;
    }
    let (x1, y1, x2, y2) = clip_rect;
    let (rx0, rx1) = (x1.min(x2) as i64, x1.max(x2) as i64);
    let (ry0, ry1) = (y1.min(y2) as i64, y1.max(y2) as i64);
    let cx0 = rx0.max(0);
    let cy0 = ry0.max(0);
    let cx1 = rx1.min(buffer.width as i64 - 1);
    let cy1 = ry1.min(buffer.height as i64 - 1);
    if cx0 > cx1 || cy0 > cy1 {
        return None;
    }
    Some((cx0, cy0, cx1, cy1))
}

fn clear_rect(buffer: &mut RenderBuffer, x0: i64, y0: i64, x1: i64, y1: i64) {
    if x0 > x1 || y0 > y1 {
        return;
    }
    let stride = match usize::try_from(buffer.width)
        .ok()
        .and_then(|w| w.checked_mul(4))
    {
        Some(s) => s,
        None => return,
    };
    for y in y0..=y1 {
        let (Ok(yu), Ok(x0u), Ok(x1u)) =
            (usize::try_from(y), usize::try_from(x0), usize::try_from(x1))
        else {
            continue;
        };
        let (Some(row_start), Some(x1p1)) = (yu.checked_mul(stride), x1u.checked_add(1)) else {
            continue;
        };
        let (Some(x0b), Some(x1b)) = (x0u.checked_mul(4), x1p1.checked_mul(4)) else {
            continue;
        };
        let (Some(start), Some(end)) = (row_start.checked_add(x0b), row_start.checked_add(x1b))
        else {
            continue;
        };
        if end <= buffer.pixels.len() && start <= end {
            buffer.pixels[start..end].fill(0);
        }
    }
}

pub fn apply_clip(buffer: &mut RenderBuffer, clip_rect: (i32, i32, i32, i32)) {
    let w = buffer.width as i64;
    let h = buffer.height as i64;
    let Some((cx0, cy0, cx1, cy1)) = normalize_clip(buffer, clip_rect) else {
        buffer.pixels.fill(0);
        return;
    };

    clear_rect(buffer, 0, 0, w - 1, cy0 - 1);
    clear_rect(buffer, 0, cy1 + 1, w - 1, h - 1);
    clear_rect(buffer, 0, cy0, cx0 - 1, cy1);
    clear_rect(buffer, cx1 + 1, cy0, w - 1, cy1);
}

pub fn apply_inverse_clip(buffer: &mut RenderBuffer, clip_rect: (i32, i32, i32, i32)) {
    let Some((cx0, cy0, cx1, cy1)) = normalize_clip(buffer, clip_rect) else {
        return;
    };
    clear_rect(buffer, cx0, cy0, cx1, cy1);
}
