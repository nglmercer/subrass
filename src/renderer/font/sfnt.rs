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

/// Declared font metadata parsed from sfnt tables.
#[derive(Debug, Clone, Default)]
pub(super) struct FontMetadata {
    /// Declared family names (name IDs 1 and 16, plus full name 4).
    pub(super) families: Vec<String>,
    /// OS/2.usWeightClass (1..=1000) when the table parses.
    pub(super) weight: Option<u16>,
    pub(super) is_bold: Option<bool>,
    pub(super) is_italic: Option<bool>,
    /// `post` underline `(position, thickness)` when gated valid.
    pub(super) underline: Option<(i16, i16)>,
    /// OS/2 strikeout `(position, size)` when gated valid.
    pub(super) strikeout: Option<(i16, i16)>,
    /// OS/2 `usWinAscent`/`usWinDescent` (offsets 74/76) when the
    /// table parses: FreeType sizes SFNT faces by their sum.
    pub(super) win_ascent: Option<u16>,
    pub(super) win_descent: Option<u16>,
    /// OS/2 `ulCodePageRange1/2` (offsets 78/82), used as a soft
    /// charset hint when ordering fallback faces.
    pub(super) code_page_ranges: [u32; 2],
    /// `head` unitsPerEm when nonzero.
    pub(super) units_per_em: Option<u16>,
}

/// Inspect a font's own `name`, `OS/2`, and `head` tables.
///
/// Returns `None` when the data is not a parseable sfnt container.
/// Never panics on malformed input: all reads are bounds-checked.
#[cfg(test)]
pub(super) fn inspect_font_metadata(data: &[u8]) -> Option<FontMetadata> {
    inspect_font_metadata_at(data, 0)
}

/// Inspect one face in a single-font sfnt or a TrueType/OpenType collection.
pub(super) fn inspect_font_metadata_at(data: &[u8], face_index: u32) -> Option<FontMetadata> {
    let tables = read_sfnt_table_directory_at(data, face_index)?;
    let mut meta = FontMetadata::default();

    if let Some(name_table) = tables
        .iter()
        .find(|(tag, _, _)| tag == b"name")
        .and_then(|(_, offset, len)| data.get(*offset..offset.saturating_add(*len)))
    {
        meta.families = parse_name_table(name_table);
    }

    // OS/2.usWeightClass at offset 4; fsSelection at 62
    // (bit 0 = italic, bit 5 = bold); yStrikeoutSize at 26 and
    // yStrikeoutPosition at 28 (SHORT, y-up, libass gates
    // position >= 0 and size > 0).
    if let Some(os2) = tables
        .iter()
        .find(|(tag, _, _)| tag == b"OS/2")
        .and_then(|(_, offset, len)| data.get(*offset..offset.saturating_add(*len)))
    {
        if let Some(us_weight) = read_u16(os2, 4) {
            if (1..=1000).contains(&us_weight) {
                meta.weight = Some(us_weight);
            }
        }
        if let Some(fs_selection) = read_u16(os2, 62) {
            meta.is_italic = Some(fs_selection & 0x0001 != 0);
            meta.is_bold = Some(fs_selection & 0x0020 != 0);
        }
        if let (Some(size), Some(pos)) = (read_i16(os2, 26), read_i16(os2, 28)) {
            if pos >= 0 && size > 0 {
                meta.strikeout = Some((pos, size));
            }
        }
        // OS/2.usWinAscent at 74, usWinDescent at 76: the FreeType
        // size divisor (accepted as parsed; a zero sum falls back
        // to the raster height at use).
        if let (Some(win_asc), Some(win_desc)) = (read_u16(os2, 74), read_u16(os2, 76)) {
            meta.win_ascent = Some(win_asc);
            meta.win_descent = Some(win_desc);
        }
        if let Some(range1) = read_u32(os2, 78) {
            meta.code_page_ranges[0] = range1;
        }
        if let Some(range2) = read_u32(os2, 82) {
            meta.code_page_ranges[1] = range2;
        }
    }

    // post.underlinePosition (FWord) at 8, underlineThickness at 10
    // (y-up font units; libass gates position <= 0, thickness > 0).
    if let Some(post) = tables
        .iter()
        .find(|(tag, _, _)| tag == b"post")
        .and_then(|(_, offset, len)| data.get(*offset..offset.saturating_add(*len)))
    {
        if let (Some(pos), Some(thick)) = (read_i16(post, 8), read_i16(post, 10)) {
            if pos <= 0 && thick > 0 {
                meta.underline = Some((pos, thick));
            }
        }
    }

    // head.unitsPerEm at 18 scales all font-unit metrics.
    if let Some(head) = tables
        .iter()
        .find(|(tag, _, _)| tag == b"head")
        .and_then(|(_, offset, len)| data.get(*offset..offset.saturating_add(*len)))
        .and_then(|head| read_u16(head, 18))
    {
        if head != 0 {
            meta.units_per_em = Some(head);
        }
    }

    // head.macStyle fallback: bit 0 = bold, bit 1 = italic.
    if meta.is_bold.is_none() || meta.is_italic.is_none() {
        if let Some(head) = tables
            .iter()
            .find(|(tag, _, _)| tag == b"head")
            .and_then(|(_, offset, len)| data.get(*offset..offset.saturating_add(*len)))
        {
            if let Some(mac_style) = read_u16(head, 44) {
                if meta.is_bold.is_none() {
                    meta.is_bold = Some(mac_style & 0x0001 != 0);
                }
                if meta.is_italic.is_none() {
                    meta.is_italic = Some(mac_style & 0x0002 != 0);
                }
            }
        }
    }

    if meta.families.is_empty()
        && meta.is_bold.is_none()
        && meta.is_italic.is_none()
        && meta.weight.is_none()
        && meta.underline.is_none()
        && meta.strikeout.is_none()
    {
        return None;
    }
    Some(meta)
}

/// Read the sfnt table directory: (tag, offset, length) triples.
///
/// Only single-face containers are accepted (`\0\1\0\0`, `OTTO`,
/// `true`, `typ1`). Font collections (`ttcf`, i.e. .ttc/.otc) are
/// rejected: their header is not a table directory, and silently
/// misreading it would yield wrong family/weight metadata.
/// Read the table directory for one sfnt face.  TTC table offsets are
/// absolute offsets into the collection, while standalone sfnt offsets are
/// relative to the face's directory, which is also the file start.
fn read_sfnt_table_directory_at(
    data: &[u8],
    face_index: u32,
) -> Option<Vec<([u8; 4], usize, usize)>> {
    let face_offset = if data.get(0..4) == Some(b"ttcf") {
        let count = read_u32(data, 8)?;
        if face_index >= count || count > 64 {
            return None;
        }
        let offset = 12usize.checked_add(usize::try_from(face_index).ok()?.checked_mul(4)?)?;
        read_u32(data, offset)? as usize
    } else if face_index == 0 {
        0
    } else {
        return None;
    };
    if data.len() < face_offset.saturating_add(12) {
        return None;
    }
    let magic = data.get(face_offset..face_offset + 4)?;
    if magic != [0x00, 0x01, 0x00, 0x00] && magic != b"OTTO" && magic != b"true" && magic != b"typ1"
    {
        return None;
    }
    let num_tables = read_u16(data, face_offset + 4)? as usize;
    let dir_len = 12usize.checked_add(num_tables.checked_mul(16)?)?;
    if num_tables > 64 || data.len() < face_offset.saturating_add(dir_len) {
        return None;
    }
    let mut tables = Vec::with_capacity(num_tables);
    for i in 0..num_tables {
        let base = face_offset + 12 + i * 16;
        let tag: [u8; 4] = data.get(base..base + 4)?.try_into().ok()?;
        let offset = read_u32(data, base + 8)? as usize;
        let len = read_u32(data, base + 12)? as usize;
        tables.push((tag, offset, len));
    }
    Some(tables)
}

/// Parse name IDs 1 (family), 16 (typographic family), and 4 (full name).
pub(super) fn parse_name_table(table: &[u8]) -> Vec<String> {
    let mut families = Vec::new();
    let count = read_u16(table, 2).unwrap_or(0) as usize;
    let string_offset = read_u16(table, 4).unwrap_or(0) as usize;
    if count > 512 {
        return families;
    }
    for i in 0..count {
        let base = 6 + i * 12;
        let platform = read_u16(table, base);
        let encoding = read_u16(table, base + 2);
        let language = read_u16(table, base + 4);
        let name_id = read_u16(table, base + 6);
        let length = read_u16(table, base + 8);
        let offset = read_u16(table, base + 10);
        let (
            Some(platform),
            Some(encoding),
            Some(language),
            Some(name_id),
            Some(length),
            Some(offset),
        ) = (platform, encoding, language, name_id, length, offset)
        else {
            continue;
        };
        if name_id != 1 && name_id != 4 && name_id != 16 {
            continue;
        }
        // Prefer Unicode (platform 0), Windows Unicode (3,1/10), or Mac Roman (1,0).
        let text = match (platform, encoding) {
            (0, _) | (3, 1) | (3, 10) => {
                let start = string_offset + offset as usize;
                let bytes = table.get(start..start.saturating_add(length as usize));
                bytes.and_then(decode_utf16_be)
            }
            (1, 0) if language == 0 => {
                let start = string_offset + offset as usize;
                let bytes = table.get(start..start.saturating_add(length as usize));
                bytes.map(|b| b.iter().map(|&c| c as char).collect::<String>())
            }
            _ => None,
        };
        if let Some(text) = text {
            let text = text.trim().to_string();
            if !text.is_empty() && !families.contains(&text) {
                families.push(text);
            }
        }
    }
    families
}

pub(super) fn decode_utf16_be(bytes: &[u8]) -> Option<String> {
    if !bytes.len().is_multiple_of(2) || bytes.len() > 4096 {
        return None;
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16(&units).ok()
}
