//! Safe collection-header inspection for TTC/OTC font data.

/// Number of faces in a validated collection, or one for a standalone sfnt.
///
/// The count is bounded before any per-face parsing or allocation occurs.
pub(super) fn collection_face_count(data: &[u8]) -> Option<usize> {
    if data.get(0..4) == Some(b"ttcf") {
        let count = read_u32(data, 8)? as usize;
        if count == 0 || count > 64 {
            return None;
        }
        let offsets_len = count.checked_mul(4)?.checked_add(12)?;
        if data.len() < offsets_len {
            return None;
        }
        Some(count)
    } else {
        Some(1)
    }
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset.saturating_add(4))
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}
