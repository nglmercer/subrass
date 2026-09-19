//! Primitive bounded readers shared by SFNT metadata parsing.

pub(super) fn read_u16(data: &[u8], offset: usize) -> Option<u16> {
    data.get(offset..offset.saturating_add(2))
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
}

pub(super) fn read_i16(data: &[u8], offset: usize) -> Option<i16> {
    read_u16(data, offset).map(|v| v as i16)
}

pub(super) fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset.saturating_add(4))
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}
