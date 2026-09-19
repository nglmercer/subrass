use super::{drawing_contains_command, parse_libass_i32, OverrideTag};

/// Parse `\clip` / `\iclip` params: rectangular `(x1,y1,x2,y2)` or
/// vector `(drawing)` / `(scale,drawing)` form.
pub(super) fn parse_clip_params(params: Option<&str>, inverse: bool) -> Option<OverrideTag> {
    let params = params?;
    let parts: Vec<&str> = params.split(',').collect();
    // Rectangular form: exactly four integers.
    if parts.len() == 4 {
        let (x1, y1, x2, y2) = (
            parse_libass_i32(parts[0]),
            parse_libass_i32(parts[1]),
            parse_libass_i32(parts[2]),
            parse_libass_i32(parts[3]),
        );
        return Some(if inverse {
            OverrideTag::InverseClip(x1, y1, x2, y2)
        } else {
            OverrideTag::Clip(x1, y1, x2, y2)
        });
    }
    // Vector form: [scale,] drawing commands. The drawing must contain
    // at least one drawing command letter, otherwise this is a malformed
    // rectangle (e.g. "1,2,3"), not a vector clip.
    let (scale, drawing) = match params.split_once(',') {
        Some((head, tail)) if !tail.trim().is_empty() => {
            (parse_libass_i32(head), tail.trim().to_string())
        }
        _ => (1, params.trim().to_string()),
    };
    if drawing.is_empty() || !drawing_contains_command(&drawing) {
        return None;
    }
    Some(if inverse {
        OverrideTag::InverseClipVector { scale, drawing }
    } else {
        OverrideTag::ClipVector { scale, drawing }
    })
}
