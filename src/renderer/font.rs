use ab_glyph::FontArc;
use std::collections::HashMap;

/// A resolved font with a stable identity and faux-style requirements.
#[derive(Debug, Clone, Copy)]
pub struct FontMatch<'a> {
    /// Stable numeric identity of the loaded font (index in load order).
    pub id: usize,
    pub font: &'a FontArc,
    /// True when bold was requested but the face is not bold: the
    /// renderer must synthesize bold (dilation) instead of double-applying.
    pub faux_bold: bool,
    /// True when italic was requested but the face is not italic.
    pub faux_italic: bool,
}

/// Font manager - loads, caches, and provides fonts for rendering
pub struct FontManager {
    fonts: Vec<LoadedFont>,
    name_index: HashMap<String, usize>,
    fallback_index: Option<usize>,
}

struct LoadedFont {
    name: String,
    font: FontArc,
    is_bold: bool,
    is_italic: bool,
}

impl FontManager {
    pub fn new() -> Self {
        Self {
            fonts: Vec::new(),
            name_index: HashMap::new(),
            fallback_index: None,
        }
    }

    /// Load a font from bytes with explicit style flags.
    pub fn load_font(
        &mut self,
        name: &str,
        data: &[u8],
        is_bold: bool,
        is_italic: bool,
    ) -> Result<usize, String> {
        let font = FontArc::try_from_vec(data.to_vec())
            .map_err(|e| format!("Failed to parse font '{}': {}", name, e))?;

        let idx = self.fonts.len();
        self.fonts.push(LoadedFont {
            name: name.to_lowercase(),
            font,
            is_bold,
            is_italic,
        });

        let key = style_key(&name.to_lowercase(), is_bold, is_italic);
        self.name_index.insert(key, idx);

        // Set as fallback if it's the first font loaded
        if self.fallback_index.is_none() {
            self.fallback_index = Some(idx);
        }

        Ok(idx)
    }

    /// Load a font, detecting family and style from the font's own metadata
    /// tables with filename heuristics as fallback. Registers aliases for
    /// every declared family name so ASS `Fontname` values resolve even
    /// when the embedded filename differs from the family name.
    pub fn load_font_auto(&mut self, name: &str, data: &[u8]) -> Result<usize, String> {
        let meta = inspect_font_metadata(data);
        let (is_bold, is_italic) = match meta {
            Some(ref m) => {
                let filename_style = style_from_filename(name);
                (
                    m.is_bold.or(filename_style.0),
                    m.is_italic.or(filename_style.1),
                )
            }
            None => {
                let (b, i) = style_from_filename(name);
                (b, i)
            }
        };
        let is_bold = is_bold.unwrap_or(false);
        let is_italic = is_italic.unwrap_or(false);

        let base_name = strip_style_words(name);
        let idx = self.load_font(&base_name, data, is_bold, is_italic)?;

        // Alias every declared family name to this font.
        if let Some(meta) = meta {
            for family in meta.families {
                let family = family.trim().to_lowercase();
                if family.is_empty() || family == base_name {
                    continue;
                }
                // First registration wins: deterministic, load-order precedence.
                self.name_index
                    .entry(style_key(&family, is_bold, is_italic))
                    .or_insert(idx);
            }
        }
        Ok(idx)
    }

    /// Find a font matching the requested name and style.
    ///
    /// Deterministic precedence (load order breaks ties):
    /// 1. exact family + exact style
    /// 2. exact family, style ignored
    /// 3. normalized family match (substring either way)
    /// 4. fallback (first loaded) font
    pub fn find_font_with_match(&self, name: &str, bold: bool, italic: bool) -> FontMatch<'_> {
        let lower = name.to_lowercase();

        // 1. Exact family + exact style.
        let key = style_key(&lower, bold, italic);
        if let Some(&idx) = self.name_index.get(&key) {
            return self.matched(idx, bold, italic);
        }

        // 2. Exact family, nearest style — scan in load order.
        if let Some(idx) = self.fonts.iter().position(|f| f.name == lower) {
            return self.matched(idx, bold, italic);
        }

        // 3. Normalized family match in load order (deterministic).
        if let Some((idx, _)) = self.fonts.iter().enumerate().find(|(_, f)| {
            !f.name.is_empty() && (lower.contains(&f.name) || f.name.contains(&lower))
        }) {
            return self.matched(idx, bold, italic);
        }

        // 4. Deterministic fallback font.
        let idx = self.fallback_index.unwrap_or(0);
        self.matched(idx, bold, italic)
    }

    fn matched(&self, idx: usize, bold: bool, italic: bool) -> FontMatch<'_> {
        let loaded = &self.fonts[idx];
        FontMatch {
            id: idx,
            font: &loaded.font,
            faux_bold: bold && !loaded.is_bold,
            faux_italic: italic && !loaded.is_italic,
        }
    }

    /// Find a font matching the requested name and style (face only).
    pub fn find_font(&self, name: &str, bold: bool, italic: bool) -> &FontArc {
        self.find_font_with_match(name, bold, italic).font
    }

    /// Get font at index
    pub fn get_font(&self, index: usize) -> Option<&FontArc> {
        self.fonts.get(index).map(|f| &f.font)
    }

    /// Get number of loaded fonts
    pub fn font_count(&self) -> usize {
        self.fonts.len()
    }

    /// Check if any fonts are loaded
    pub fn has_fonts(&self) -> bool {
        !self.fonts.is_empty()
    }

    /// Get all loaded font names
    pub fn font_names(&self) -> Vec<&str> {
        self.fonts.iter().map(|f| f.name.as_str()).collect()
    }
}

fn style_key(name: &str, bold: bool, italic: bool) -> String {
    format!("{}:{}:{}", name, bold, italic)
}

/// Filename-based style guess, used only when font metadata is absent.
fn style_from_filename(name: &str) -> (Option<bool>, Option<bool>) {
    let lower = name.to_lowercase();
    let bold = if lower.contains("bold") {
        Some(true)
    } else {
        None
    };
    let italic = if lower.contains("italic") || lower.contains("oblique") {
        Some(true)
    } else {
        None
    };
    (bold, italic)
}

/// Strip style indicators from a filename stem for lookup.
fn strip_style_words(name: &str) -> String {
    name.to_lowercase()
        .replace(" bold", "")
        .replace(" italic", "")
        .replace(" oblique", "")
        .replace("-bold", "")
        .replace("-italic", "")
        .replace("-oblique", "")
        .replace("_bold", "")
        .replace("_italic", "")
        .replace("_oblique", "")
        .trim()
        .to_string()
}

impl Default for FontManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Get the built-in fallback font (DejaVu Sans)
pub fn get_fallback_font() -> &'static [u8] {
    include_bytes!("../../fonts/DejaVuSans.ttf")
}

/// Declared font metadata parsed from sfnt tables.
#[derive(Debug, Clone, Default)]
struct FontMetadata {
    /// Declared family names (name IDs 1 and 16, plus full name 4).
    families: Vec<String>,
    is_bold: Option<bool>,
    is_italic: Option<bool>,
}

/// Inspect a font's own `name`, `OS/2`, and `head` tables.
///
/// Returns `None` when the data is not a parseable sfnt container.
/// Never panics on malformed input: all reads are bounds-checked.
fn inspect_font_metadata(data: &[u8]) -> Option<FontMetadata> {
    let tables = read_sfnt_table_directory(data)?;
    let mut meta = FontMetadata::default();

    if let Some(name_table) = tables
        .iter()
        .find(|(tag, _, _)| tag == b"name")
        .and_then(|(_, offset, len)| data.get(*offset..offset.saturating_add(*len)))
    {
        meta.families = parse_name_table(name_table);
    }

    // OS/2.fsSelection: bit 0 = italic, bit 5 = bold.
    if let Some(os2) = tables
        .iter()
        .find(|(tag, _, _)| tag == b"OS/2")
        .and_then(|(_, offset, len)| data.get(*offset..offset.saturating_add(*len)))
    {
        if let Some(fs_selection) = read_u16(os2, 62) {
            meta.is_italic = Some(fs_selection & 0x0001 != 0);
            meta.is_bold = Some(fs_selection & 0x0020 != 0);
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

    if meta.families.is_empty() && meta.is_bold.is_none() && meta.is_italic.is_none() {
        return None;
    }
    Some(meta)
}

/// Read the sfnt table directory: (tag, offset, length) triples.
fn read_sfnt_table_directory(data: &[u8]) -> Option<Vec<([u8; 4], usize, usize)>> {
    if data.len() < 12 {
        return None;
    }
    let num_tables = read_u16(data, 4)? as usize;
    if num_tables > 64 || data.len() < 12 + num_tables * 16 {
        return None;
    }
    let mut tables = Vec::with_capacity(num_tables);
    for i in 0..num_tables {
        let base = 12 + i * 16;
        let tag: [u8; 4] = data.get(base..base + 4)?.try_into().ok()?;
        let offset = read_u32(data, base + 8)? as usize;
        let len = read_u32(data, base + 12)? as usize;
        tables.push((tag, offset, len));
    }
    Some(tables)
}

/// Parse name IDs 1 (family), 16 (typographic family), and 4 (full name).
fn parse_name_table(table: &[u8]) -> Vec<String> {
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

fn decode_utf16_be(bytes: &[u8]) -> Option<String> {
    if !bytes.len().is_multiple_of(2) || bytes.len() > 4096 {
        return None;
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16(&units).ok()
}

fn read_u16(data: &[u8], offset: usize) -> Option<u16> {
    data.get(offset..offset + 2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_font_manager_new() {
        let fm = FontManager::new();
        assert!(!fm.has_fonts());
        assert_eq!(fm.font_count(), 0);
    }

    #[test]
    fn test_metadata_detects_dejavu_family() {
        let meta = inspect_font_metadata(get_fallback_font()).expect("metadata");
        assert!(
            meta.families.iter().any(|f| f == "DejaVu Sans"),
            "{:?}",
            meta.families
        );
        assert_eq!(meta.is_bold, Some(false));
        assert_eq!(meta.is_italic, Some(false));
    }

    #[test]
    fn test_metadata_rejects_garbage() {
        assert!(inspect_font_metadata(b"not a font").is_none());
        assert!(inspect_font_metadata(&[]).is_none());
        assert!(inspect_font_metadata(&[0u8; 100]).is_none());
    }

    #[test]
    fn test_auto_load_registers_family_alias() {
        let mut fm = FontManager::new();
        // Filename stem differs from the declared family name
        fm.load_font_auto("weird-filename-xyz", get_fallback_font())
            .unwrap();
        // Resolves via declared family name, not just filename
        let m = fm.find_font_with_match("DejaVu Sans", false, false);
        assert_eq!(m.id, 0);
        assert!(!m.faux_bold && !m.faux_italic);
    }

    #[test]
    fn test_faux_flags_only_when_face_lacks_style() {
        let mut fm = FontManager::new();
        fm.load_font("DejaVu Sans", get_fallback_font(), false, false)
            .unwrap();
        fm.load_font("DejaVu Sans Bold", get_fallback_font(), true, false)
            .unwrap();
        // Exact bold face: no faux bold
        let m = fm.find_font_with_match("DejaVu Sans Bold", true, false);
        assert_eq!(m.id, 1);
        assert!(!m.faux_bold);
        // Regular face asked for bold: faux bold required
        let m = fm.find_font_with_match("DejaVu Sans", true, false);
        assert_eq!(m.id, 0);
        assert!(m.faux_bold);
    }

    #[test]
    fn test_selection_is_deterministic() {
        let mut fm = FontManager::new();
        fm.load_font("aaa font", get_fallback_font(), false, false)
            .unwrap();
        fm.load_font("aaa font extended", get_fallback_font(), false, false)
            .unwrap();
        // Overlapping names: first loaded wins, every time
        for _ in 0..10 {
            let m = fm.find_font_with_match("aaa font", false, false);
            assert_eq!(m.id, 0);
        }
    }
}
