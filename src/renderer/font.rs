use ab_glyph::{Font, FontArc, FontVec};
use std::collections::HashMap;
#[cfg(all(feature = "system-fonts", not(target_arch = "wasm32")))]
use std::collections::HashSet;
use std::sync::Arc;

/// A resolved font with a stable identity and faux-style requirements.
#[derive(Debug, Clone, Copy)]
pub struct FontMatch<'a> {
    /// Stable numeric identity of the loaded font (index in load order).
    pub id: usize,
    pub font: &'a FontArc,
    /// True when a bold-class weight (>= 700) was requested but the
    /// selected face is not bold-class: the renderer must synthesize
    /// bold (dilation). Never set when the face already has a suitable
    /// bold weight, so real bold faces are never double-bolded.
    pub faux_bold: bool,
    /// True when italic was requested but the face is not italic.
    pub faux_italic: bool,
}

/// Font manager - loads, caches, and provides fonts for rendering
pub struct FontManager {
    fonts: Vec<LoadedFont>,
    /// Additional family names (from font metadata) mapping to a font.
    aliases: HashMap<String, usize>,
    fallback_index: Option<usize>,
}

struct LoadedFont {
    name: String,
    font: FontArc,
    /// ASS font weight (400 = normal, 700 = bold), from OS/2
    /// usWeightClass when available, else from the style flags.
    weight: u16,
    /// OS/2 `usWinAscent`/`usWinDescent`: FreeType sizes SFNT faces
    /// by their sum, so these drive the face's pixel scale.
    win_ascent: Option<u16>,
    win_descent: Option<u16>,
    is_italic: bool,
    /// Underline/strikeout metrics for decorations.
    decorations: DecorationMetrics,
    /// Original bytes are retained for OpenType shaping.  Keeping the
    /// collection index alongside them gives every face a stable shaping
    /// identity without extracting or rewriting sfnt tables.
    data: Arc<Vec<u8>>,
    face_index: u32,
}

/// Underline/strikeout font metrics (libass `ass_get_glyph_outline`
/// `DECO_*`): `post` underline + OS/2 strikeout in font units plus
/// the FreeType size divisor. A `None` member means "draw no bar" —
/// libass skips the bar when its table is missing or fails the
/// validity gate (underline needs position <= 0 and thickness > 0,
/// strikeout needs position >= 0 and size > 0).
#[derive(Debug, Clone, Copy, Default)]
pub struct DecorationMetrics {
    pub units_per_em: u16,
    /// FreeType size divisor in font units (OS/2 Win sum when the
    /// table parses, else the raster height): bar positions scale by
    /// `em_px / scale_height`, matching libass `y_scale`. Zero only
    /// for a default-constructed value, which draws no bars.
    pub scale_height: f32,
    /// `(position, thickness)`; position <= 0 (below baseline).
    pub underline: Option<(i16, i16)>,
    /// `(position, size)`; position >= 0 (above baseline).
    pub strikeout: Option<(i16, i16)>,
}

impl FontManager {
    pub fn new() -> Self {
        Self {
            fonts: Vec::new(),
            aliases: HashMap::new(),
            fallback_index: None,
        }
    }

    /// Discover native system fonts when the opt-in `system-fonts` feature
    /// is enabled. Embedded/manual faces are loaded first and therefore keep
    /// deterministic precedence; a discovered family is skipped when that
    /// family is already present. This method is intentionally unavailable
    /// to WASM builds, whose font set must remain explicit and reproducible.
    #[cfg(all(feature = "system-fonts", not(target_arch = "wasm32")))]
    pub fn load_system_fonts(&mut self) -> usize {
        let mut database = fontdb::Database::new();
        database.load_system_fonts();
        let mut faces: Vec<_> = database.faces().cloned().collect();
        faces.sort_by(|a, b| {
            let family = |face: &fontdb::FaceInfo| {
                face.families
                    .first()
                    .map(|(name, _)| name.to_lowercase())
                    .unwrap_or_default()
            };
            family(a)
                .cmp(&family(b))
                .then(a.weight.0.cmp(&b.weight.0))
                .then(a.index.cmp(&b.index))
        });
        let mut loaded_families = HashSet::new();
        let mut loaded = 0;
        for face in faces {
            let Some(family) = face.families.first().map(|(name, _)| name.clone()) else {
                continue;
            };
            let normalized = family.to_lowercase();
            if loaded_families.contains(&normalized) || self.has_family(&family) {
                continue;
            }
            let Some((data, _face_index)) =
                database.with_face_data(face.id, |bytes, index| (bytes.to_vec(), index))
            else {
                continue;
            };
            if self.load_font_auto(&family, &data).is_ok() {
                loaded_families.insert(normalized);
                loaded += 1;
            }
        }
        loaded
    }

    /// Load a font from bytes with explicit style flags
    /// (weight 700 for bold, 400 otherwise).
    pub fn load_font(
        &mut self,
        name: &str,
        data: &[u8],
        is_bold: bool,
        is_italic: bool,
    ) -> Result<usize, String> {
        self.load_font_with_weight(name, data, if is_bold { 700 } else { 400 }, is_italic)
    }

    /// Load a font from bytes with an explicit ASS weight and italic flag.
    ///
    /// Load every face in a TrueType/OpenType collection.  A collection is
    /// never reduced to face 0: each face gets its own metadata, matcher
    /// identity, fallback slot, and shaping face index.
    pub fn load_font_with_weight(
        &mut self,
        name: &str,
        data: &[u8],
        weight: u16,
        is_italic: bool,
    ) -> Result<usize, String> {
        let count = collection_face_count(data)
            .ok_or_else(|| format!("Invalid font collection header for '{}'", name))?;
        let mut first = None;
        for face_index in 0..count {
            let idx = self.load_one_face(
                name,
                data,
                weight,
                is_italic,
                face_index as u32,
                inspect_font_metadata_at(data, face_index as u32),
            )?;
            first.get_or_insert(idx);
        }
        first.ok_or_else(|| format!("Font '{}' contains no faces", name))
    }

    /// Load a font, detecting family and style from the font's own metadata
    /// tables with filename heuristics as fallback. Registers aliases for
    /// every declared family name so ASS `Fontname` values resolve even
    /// when the embedded filename differs from the family name.
    pub fn load_font_auto(&mut self, name: &str, data: &[u8]) -> Result<usize, String> {
        let base_name = strip_style_words(name);
        let count = collection_face_count(data)
            .ok_or_else(|| format!("Invalid font collection header for '{}'", name))?;
        let filename_style = style_from_filename(name);
        let mut first = None;
        for face_index in 0..count {
            let meta = inspect_font_metadata_at(data, face_index as u32);
            let (is_bold, is_italic) = match meta.as_ref() {
                Some(m) => (
                    m.is_bold.or(filename_style.0),
                    m.is_italic.or(filename_style.1),
                ),
                None => filename_style,
            };
            let is_bold = is_bold.unwrap_or(false);
            let is_italic = is_italic.unwrap_or(false);
            let weight = meta
                .as_ref()
                .and_then(|m| m.weight)
                .filter(|w| (1..=1000).contains(w))
                .unwrap_or(if is_bold { 700 } else { 400 });
            let idx = self.load_one_face(
                &base_name,
                data,
                weight,
                is_italic,
                face_index as u32,
                meta.clone(),
            )?;
            first.get_or_insert(idx);
            if let Some(meta) = meta {
                for family in meta.families {
                    let family = family.trim().to_lowercase();
                    if family.is_empty() || family == base_name {
                        continue;
                    }
                    self.aliases.entry(family).or_insert(idx);
                }
            }
        }
        first.ok_or_else(|| format!("Font '{}' contains no faces", name))
    }

    fn load_one_face(
        &mut self,
        name: &str,
        data: &[u8],
        weight: u16,
        is_italic: bool,
        face_index: u32,
        meta: Option<FontMetadata>,
    ) -> Result<usize, String> {
        let font = FontArc::new(
            FontVec::try_from_vec_and_index(data.to_vec(), face_index).map_err(|e| {
                format!("Failed to parse font '{}' face {}: {}", name, face_index, e)
            })?,
        );
        let win_height = meta
            .as_ref()
            .and_then(|m| match (m.win_ascent, m.win_descent) {
                (Some(a), Some(d)) if u32::from(a) + u32::from(d) > 0 => {
                    Some(f32::from(a) + f32::from(d))
                }
                _ => None,
            });
        let decorations = DecorationMetrics {
            units_per_em: meta.as_ref().and_then(|m| m.units_per_em).unwrap_or(0),
            scale_height: win_height.unwrap_or_else(|| font.height_unscaled().max(1.0)),
            underline: meta.as_ref().and_then(|m| m.underline),
            strikeout: meta.as_ref().and_then(|m| m.strikeout),
        };
        let idx = self.fonts.len();
        self.fonts.push(LoadedFont {
            name: name.to_lowercase(),
            font,
            weight: weight.clamp(1, 1000),
            win_ascent: meta.as_ref().and_then(|m| m.win_ascent),
            win_descent: meta.as_ref().and_then(|m| m.win_descent),
            is_italic,
            decorations,
            data: Arc::new(data.to_vec()),
            face_index,
        });
        if self.fallback_index.is_none() {
            self.fallback_index = Some(idx);
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
        self.find_font_with_weight(name, if bold { 700 } else { 400 }, italic)
    }

    /// Find a font matching the requested name, ASS weight, and italic flag.
    ///
    /// Deterministic precedence (load order breaks ties):
    /// 1. exact family + italic match, nearest weight
    /// 2. exact family (any italic), nearest weight
    /// 3. normalized family match (substring either way), nearest weight
    /// 4. fallback (first loaded) font
    pub fn find_font_with_weight(&self, name: &str, weight: u16, italic: bool) -> FontMatch<'_> {
        let lower = name.to_lowercase();
        let weight = weight.clamp(1, 1000);

        // Family members: primary names plus metadata aliases, load order.
        let mut members: Vec<usize> = self
            .fonts
            .iter()
            .enumerate()
            .filter(|(_, f)| f.name == lower)
            .map(|(i, _)| i)
            .collect();
        if let Some(&alias_idx) = self.aliases.get(&lower) {
            if !members.contains(&alias_idx) {
                members.push(alias_idx);
                members.sort_unstable();
            }
        }
        // 1-2. Exact family (prefer italic match), nearest weight.
        if !members.is_empty() {
            let italic_members: Vec<usize> = members
                .iter()
                .copied()
                .filter(|&i| self.fonts[i].is_italic == italic)
                .collect();
            let pool = if italic_members.is_empty() {
                members
            } else {
                italic_members
            };
            if let Some(&idx) = pool
                .iter()
                .min_by_key(|&&i| weight_dist(self.fonts[i].weight, weight))
            {
                return self.matched(idx, weight, italic);
            }
        }

        // 3. Normalized family match (substring either way), nearest weight.
        let fuzzy: Vec<usize> = self
            .fonts
            .iter()
            .enumerate()
            .filter(|(_, f)| {
                !f.name.is_empty() && (lower.contains(&f.name) || f.name.contains(&lower))
            })
            .map(|(i, _)| i)
            .collect();
        if let Some(&idx) = fuzzy
            .iter()
            .min_by_key(|&&i| weight_dist(self.fonts[i].weight, weight))
        {
            return self.matched(idx, weight, italic);
        }

        // 4. Deterministic fallback font.
        let idx = self.fallback_index.unwrap_or(0);
        self.matched(idx, weight, italic)
    }

    fn matched(&self, idx: usize, weight: u16, italic: bool) -> FontMatch<'_> {
        let loaded = &self.fonts[idx];
        FontMatch {
            id: idx,
            font: &loaded.font,
            faux_bold: weight >= 700 && loaded.weight < 700,
            faux_italic: italic && !loaded.is_italic,
        }
    }

    /// Find a font matching the requested name and style (face only).
    pub fn find_font(&self, name: &str, bold: bool, italic: bool) -> &FontArc {
        self.find_font_with_match(name, bold, italic).font
    }

    /// ASS weight of a loaded font, if the index is valid.
    pub fn font_weight(&self, index: usize) -> Option<u16> {
        self.fonts.get(index).map(|f| f.weight)
    }

    /// True when the font contains a real glyph for `ch`
    /// (glyph id 0 is .notdef). Unknown indices miss.
    pub fn has_glyph(&self, index: usize, ch: char) -> bool {
        self.fonts
            .get(index)
            .is_some_and(|f| f.font.glyph_id(ch).0 != 0)
    }

    /// Per-glyph fallback chain for a primary font: the primary id
    /// first, then every other loaded font in load order. Always
    /// non-empty when at least one font is loaded.
    pub fn fallback_chain(&self, primary: usize) -> Vec<usize> {
        let mut chain = Vec::with_capacity(self.fonts.len());
        if primary < self.fonts.len() {
            chain.push(primary);
        }
        chain.extend((0..self.fonts.len()).filter(|&i| i != primary));
        chain
    }

    /// Faux-style requirements for rendering a glyph from font `index`
    /// under the requested weight/italic: `(faux_bold, faux_italic)`.
    /// Unknown indices conservatively require both syntheses.
    pub fn faux_for(&self, index: usize, weight: u16, italic: bool) -> (bool, bool) {
        match self.fonts.get(index) {
            Some(loaded) => (
                weight >= 700 && loaded.weight < 700,
                italic && !loaded.is_italic,
            ),
            None => (weight >= 700, italic),
        }
    }

    /// Get font at index
    pub fn get_font(&self, index: usize) -> Option<&FontArc> {
        self.fonts.get(index).map(|f| &f.font)
    }

    /// Raw bytes and the collection face index for OpenType shaping.
    pub fn shaping_data(&self, index: usize) -> Option<(&[u8], u32)> {
        self.fonts
            .get(index)
            .map(|f| (f.data.as_slice(), f.face_index))
    }

    /// Underline/strikeout metrics for a loaded font id.
    pub fn decoration_metrics(&self, index: usize) -> Option<DecorationMetrics> {
        self.fonts.get(index).map(|f| f.decorations)
    }

    /// FreeType-compatible face metrics in font units:
    /// `(ascender, descender, height)` with a negative descender.
    /// FreeType sizes SFNT faces (`FT_SIZE_REQUEST_TYPE_REAL_DIM`,
    /// which libass requests) by the OS/2 `usWinAscent` /
    /// `usWinDescent` sum — not hhea — so the ascender/descender
    /// values come from OS/2 too when usable (na10 probe: Noto
    /// advances pin the 1906 divisor; hhea's 1304 inflates them
    /// 46%). Without usable Win metrics the raster (hhea-basis)
    /// values are returned. `None` for unknown ids.
    pub fn ft_metrics(&self, index: usize) -> Option<(f32, f32, f32)> {
        let loaded = self.fonts.get(index)?;
        match (loaded.win_ascent, loaded.win_descent) {
            (Some(a), Some(d)) if u32::from(a) + u32::from(d) > 0 => {
                Some((f32::from(a), -f32::from(d), f32::from(a) + f32::from(d)))
            }
            _ => Some((
                loaded.font.ascent_unscaled(),
                loaded.font.descent_unscaled(),
                loaded.font.height_unscaled().max(1.0),
            )),
        }
    }

    /// `ab_glyph` px-scale multiplier for an ASS font size: the raster
    /// scales by its own (hhea-basis) height, so requesting
    /// `size * px_ratio` yields FreeType-basis pixels. Identity for
    /// Win==hhea faces (DejaVu), degenerate faces, and unknown ids.
    pub fn px_ratio(&self, index: usize) -> f32 {
        let (Some(loaded), Some((_, _, ft_height))) =
            (self.fonts.get(index), self.ft_metrics(index))
        else {
            return 1.0;
        };
        if ft_height <= 0.0 {
            return 1.0;
        }
        let ratio = loaded.font.height_unscaled() / ft_height;
        if ratio.is_finite() && ratio > 0.0 {
            ratio
        } else {
            1.0
        }
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

    /// True when `name` resolves to a loaded family (exact, alias, or
    /// the same substring fallback the matcher uses) rather than the
    /// last-resort fallback font. Used for missing-font diagnostics.
    pub fn has_family(&self, name: &str) -> bool {
        let lower = name.to_lowercase();
        if lower.is_empty() {
            return false;
        }
        if self.fonts.iter().any(|f| f.name == lower) || self.aliases.contains_key(&lower) {
            return true;
        }
        self.fonts
            .iter()
            .any(|f| !f.name.is_empty() && (lower.contains(&f.name) || f.name.contains(&lower)))
    }
}

/// Absolute distance between a face weight and the requested weight.
fn weight_dist(face: u16, requested: u16) -> u16 {
    face.abs_diff(requested)
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
    /// OS/2.usWeightClass (1..=1000) when the table parses.
    weight: Option<u16>,
    is_bold: Option<bool>,
    is_italic: Option<bool>,
    /// `post` underline `(position, thickness)` when gated valid.
    underline: Option<(i16, i16)>,
    /// OS/2 strikeout `(position, size)` when gated valid.
    strikeout: Option<(i16, i16)>,
    /// OS/2 `usWinAscent`/`usWinDescent` (offsets 74/76) when the
    /// table parses: FreeType sizes SFNT faces by their sum.
    win_ascent: Option<u16>,
    win_descent: Option<u16>,
    /// `head` unitsPerEm when nonzero.
    units_per_em: Option<u16>,
}

/// Inspect a font's own `name`, `OS/2`, and `head` tables.
///
/// Returns `None` when the data is not a parseable sfnt container.
/// Never panics on malformed input: all reads are bounds-checked.
#[cfg(test)]
fn inspect_font_metadata(data: &[u8]) -> Option<FontMetadata> {
    inspect_font_metadata_at(data, 0)
}

/// Inspect one face in a single-font sfnt or a TrueType/OpenType collection.
fn inspect_font_metadata_at(data: &[u8], face_index: u32) -> Option<FontMetadata> {
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

/// Number of faces in a validated collection, or one for a standalone sfnt.
/// The count is bounded before any per-face parsing/allocation occurs.
fn collection_face_count(data: &[u8]) -> Option<usize> {
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
    data.get(offset..offset.saturating_add(2))
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
}

fn read_i16(data: &[u8], offset: usize) -> Option<i16> {
    read_u16(data, offset).map(|v| v as i16)
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset.saturating_add(4))
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
    fn test_metadata_decoration_metrics() {
        // DejaVu Sans post/OS/2 tables (values read from the bundled
        // file; gates: underline pos <= 0 thick > 0, strikeout
        // pos >= 0 size > 0, like libass).
        let meta = inspect_font_metadata(get_fallback_font()).expect("metadata");
        assert_eq!(meta.underline, Some((-130, 90)));
        assert_eq!(meta.strikeout, Some((530, 102)));
        let mut fm = FontManager::new();
        fm.load_font("DejaVu Sans", get_fallback_font(), false, false)
            .unwrap();
        let deco = fm.decoration_metrics(0).expect("metrics");
        assert_eq!(deco.units_per_em, 2048);
        assert_eq!(deco.underline, Some((-130, 90)));
        assert_eq!(deco.strikeout, Some((530, 102)));
        // DejaVu Win metrics equal hhea: bars scale by 2384.
        assert_eq!(deco.scale_height, 2384.0);
        assert!(fm.decoration_metrics(9).is_none());
    }

    #[test]
    fn test_win_metrics_drive_ft_scale() {
        // FreeType sizes SFNT faces by OS/2 usWinAscent+usWinDescent
        // (libass na10 probe pins Noto's 1906 divisor; hhea's 1304
        // inflates advances 46%).
        let mut fm = FontManager::new();
        fm.load_font("DejaVu Sans", get_fallback_font(), false, false)
            .unwrap();
        let noto = std::fs::read("fonts/NotoSansDevanagari.ttf").unwrap();
        fm.load_font("Noto Sans Devanagari", &noto, false, false)
            .unwrap();
        assert_eq!(fm.ft_metrics(0), Some((1901.0, -483.0, 2384.0)));
        assert_eq!(fm.px_ratio(0), 1.0);
        assert_eq!(fm.ft_metrics(1), Some((1348.0, -558.0, 1906.0)));
        assert!((fm.px_ratio(1) - 1304.0 / 1906.0).abs() < 1e-6);
        assert_eq!(
            fm.decoration_metrics(1).expect("metrics").scale_height,
            1906.0
        );
        assert_eq!(fm.ft_metrics(9), None);
        assert_eq!(fm.px_ratio(9), 1.0);
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

    #[test]
    fn test_exact_family_beats_substring_regardless_of_order() {
        // "Arial" must resolve to Arial even when "Arial Narrow" loaded
        // first: exact normalized match outranks substring fallback.
        let mut fm = FontManager::new();
        fm.load_font("aaa font extended", get_fallback_font(), false, false)
            .unwrap();
        fm.load_font("aaa font", get_fallback_font(), false, false)
            .unwrap();
        assert_eq!(fm.find_font_with_match("aaa font", false, false).id, 1);
        assert_eq!(
            fm.find_font_with_match("aaa font extended", false, false)
                .id,
            0
        );
        // Case-insensitive exact still beats substring.
        assert_eq!(fm.find_font_with_match("AAA FONT", false, false).id, 1);
        // Genuine substring fallback still works when nothing is exact.
        assert_eq!(fm.find_font_with_match("aaa", false, false).id, 0);
    }

    #[test]
    fn test_weight_selection_picks_nearest() {
        let mut fm = FontManager::new();
        // Same bytes, three declared weights of one family.
        fm.load_font_with_weight("Fam", get_fallback_font(), 400, false)
            .unwrap();
        fm.load_font_with_weight("Fam", get_fallback_font(), 700, false)
            .unwrap();
        fm.load_font_with_weight("Fam", get_fallback_font(), 900, false)
            .unwrap();
        assert_eq!(fm.find_font_with_weight("Fam", 800, false).id, 1);
        assert_eq!(fm.find_font_with_weight("Fam", 500, false).id, 0);
        assert_eq!(fm.find_font_with_weight("Fam", 950, false).id, 2);
        assert_eq!(fm.find_font_with_weight("Fam", 100, false).id, 0);
        // Exact tie (550 between 400 and 700): first loaded wins.
        assert_eq!(fm.find_font_with_weight("Fam", 550, false).id, 0);
    }

    #[test]
    fn test_faux_bold_only_without_bold_face() {
        let mut fm = FontManager::new();
        fm.load_font_with_weight("Fam", get_fallback_font(), 400, false)
            .unwrap();
        // Bold request on a regular face: synthesize.
        assert!(fm.find_font_with_weight("Fam", 700, false).faux_bold);
        // Normal request: never synthesize.
        assert!(!fm.find_font_with_weight("Fam", 400, false).faux_bold);
        // A real bold face must never be double-bolded, even when the
        // request is heavier than the face.
        fm.load_font_with_weight("FamBold", get_fallback_font(), 700, false)
            .unwrap();
        assert!(!fm.find_font_with_weight("FamBold", 700, false).faux_bold);
        assert!(!fm.find_font_with_weight("FamBold", 900, false).faux_bold);
    }

    #[test]
    fn test_has_glyph_chain_and_faux_for() {
        let mut fm = FontManager::new();
        fm.load_font("A", get_fallback_font(), false, false)
            .unwrap();
        fm.load_font("B", get_fallback_font(), true, false).unwrap();
        fm.load_font("C", get_fallback_font(), false, true).unwrap();
        assert!(fm.has_glyph(0, 'A'));
        assert!(!fm.has_glyph(0, '\u{10FFFF}'));
        assert!(!fm.has_glyph(99, 'A'));
        // Chain: primary first, then load order.
        assert_eq!(fm.fallback_chain(1), vec![1, 0, 2]);
        assert_eq!(fm.fallback_chain(0), vec![0, 1, 2]);
        // Per-face faux requirements under a bold+italic request.
        assert_eq!(fm.faux_for(0, 700, true), (true, true));
        assert_eq!(fm.faux_for(1, 700, true), (false, true));
        assert_eq!(fm.faux_for(2, 700, true), (true, false));
        assert_eq!(fm.faux_for(2, 400, false), (false, false));
        assert_eq!(fm.faux_for(99, 700, true), (true, true));
    }

    #[test]
    fn test_metadata_reports_us_weight_class() {
        // DejaVu Sans Regular declares usWeightClass 400.
        let meta = inspect_font_metadata(get_fallback_font()).unwrap();
        assert_eq!(meta.weight, Some(400));
        let mut fm = FontManager::new();
        let idx = fm
            .load_font_auto("DejaVuSans.ttf", get_fallback_font())
            .unwrap();
        assert_eq!(fm.font_weight(idx), Some(400));
    }

    /// Plan #55: full style matrix. All faces share the bundled bytes
    /// (only one real font ships), so this locks selection-by-declared
    /// weight/style plus faux-only-when-needed.
    #[test]
    fn test_style_matrix_selection_and_faux() {
        let mut fm = FontManager::new();
        let bytes = get_fallback_font();
        // Regular Medium Semibold Bold Black Italic BoldItalic.
        for (w, italic) in [
            (400, false),
            (500, false),
            (600, false),
            (700, false),
            (900, false),
            (400, true),
            (700, true),
        ] {
            fm.load_font_with_weight("Fam", bytes, w, italic).unwrap();
        }
        // Each declared weight resolves to its own face, no synthesis.
        for (w, id) in [(400, 0), (500, 1), (600, 2), (700, 3), (900, 4)] {
            let m = fm.find_font_with_weight("Fam", w, false);
            assert_eq!(m.id, id, "weight {w}");
            assert!(!m.faux_bold && !m.faux_italic, "weight {w}");
        }
        // Italic requests resolve to the italic faces.
        let m = fm.find_font_with_weight("Fam", 400, true);
        assert_eq!(m.id, 5);
        assert!(!m.faux_bold && !m.faux_italic);
        let m = fm.find_font_with_weight("Fam", 700, true);
        assert_eq!(m.id, 6);
        assert!(!m.faux_bold && !m.faux_italic);
        // In-between requests pick the nearest face.
        assert_eq!(fm.find_font_with_weight("Fam", 550, false).id, 1);
        assert_eq!(fm.find_font_with_weight("Fam", 650, false).id, 2);
        assert_eq!(fm.find_font_with_weight("Fam", 800, false).id, 3);
        // Request clamping: 0 -> 1 (nearest 400), huge -> 1000 (nearest 900).
        assert_eq!(fm.find_font_with_weight("Fam", 0, false).id, 0);
        assert_eq!(fm.find_font_with_weight("Fam", 2000, false).id, 4);
    }

    /// Plan #55: faux styling only when no suitable real face exists.
    #[test]
    fn test_faux_only_without_suitable_face() {
        let mut fm = FontManager::new();
        fm.load_font_with_weight("Fam", get_fallback_font(), 400, false)
            .unwrap();
        // Nothing bold/italic loaded: both synthesize.
        let m = fm.find_font_with_weight("Fam", 700, true);
        assert_eq!(m.id, 0);
        assert!(m.faux_bold && m.faux_italic);
        // Semibold request on a regular-only family: nearest face, and
        // no faux bold (faux only applies to bold-class requests).
        let m = fm.find_font_with_weight("Fam", 600, false);
        assert_eq!(m.id, 0);
        assert!(!m.faux_bold && !m.faux_italic);
        // Adding a real italic face clears faux italic for that family.
        fm.load_font_with_weight("Fam", get_fallback_font(), 400, true)
            .unwrap();
        let m = fm.find_font_with_weight("Fam", 400, true);
        assert_eq!(m.id, 1);
        assert!(!m.faux_italic);
    }

    /// Plan #56: the sfnt metadata reader never panics on hostile
    /// input: truncations at every scale plus targeted corruptions of
    /// the header, directory, name records, UTF-16, OS/2, and head.
    #[test]
    fn test_metadata_never_panics_on_hostile_input() {
        let valid = get_fallback_font();
        // Every truncation scale, coarse then fine near the header.
        let mut lens: Vec<usize> = (0..64).collect();
        lens.extend((0..=valid.len()).step_by(4096));
        for len in lens {
            let _ = inspect_font_metadata(&valid[..len.min(valid.len())]);
        }
        // Targeted corruptions over a valid copy.
        let mut corrupt = valid.to_vec();
        let poke = |buf: &mut [u8], off: usize, bytes: &[u8]| {
            if off + bytes.len() <= buf.len() {
                buf[off..off + bytes.len()].copy_from_slice(bytes);
            }
        };
        // numTables extremes (offset 4).
        for tables in [0u16, 1, 63, 64, 65, 100, 0xFFFF] {
            let mut buf = valid.to_vec();
            poke(&mut buf, 4, &tables.to_be_bytes());
            let _ = inspect_font_metadata(&buf);
        }
        // Directory record extremes: first record at 12 (tag/offset/len).
        for patch in [
            (12usize, vec![0xFF, 0xFF, 0xFF, 0xFF]),
            (20, vec![0xFF, 0xFF, 0xFF, 0xFF]),
            (24, vec![0xFF, 0xFF, 0xFF, 0xFF]),
            (20, vec![0x00, 0x00, 0x00, 0x00]),
            (24, vec![0x00, 0x00, 0x00, 0x00]),
        ] {
            let mut buf = valid.to_vec();
            poke(&mut buf, patch.0, &patch.1);
            let _ = inspect_font_metadata(&buf);
        }
        // Header magic extremes.
        for magic in [&b"ttcf"[..], b"wOFF", b"\0\0\0\0", b"AAAA"] {
            let mut buf = valid.to_vec();
            poke(&mut buf, 0, magic);
            let _ = inspect_font_metadata(&buf);
        }
        // Blank the whole copy progressively (keeps magic valid).
        for zeros in [16usize, 64, 256, 1024, 8192] {
            corrupt.fill(0);
            let keep = zeros.min(corrupt.len());
            corrupt[..keep].copy_from_slice(&valid[..keep]);
            let _ = inspect_font_metadata(&corrupt);
        }
        // UTF-16 edge cases directly.
        assert!(decode_utf16_be(&[0x00]).is_none());
        assert!(decode_utf16_be(&vec![0u8; 4098]).is_none());
        assert!(decode_utf16_be(&[0xD8, 0x00]).is_none()); // lone surrogate
        assert_eq!(decode_utf16_be(&[0x00, 0x41]).as_deref(), Some("A"));
        // Name table with insane counts/string offsets.
        assert!(parse_name_table(&[0, 1, 0xFF, 0xFF, 0xFF, 0xFF]).is_empty());
        assert!(parse_name_table(&[0, 1, 0, 1, 0, 6, 0, 0]).is_empty());
        // Readers reject out-of-bounds offsets without panicking.
        assert!(read_u16(&[0x01], 0).is_none());
        assert!(read_u16(&[0x01, 0x02], 1).is_none());
        assert!(read_u32(&[0x01, 0x02, 0x03], 0).is_none());
    }

    fn make_test_collection(face: &[u8]) -> Vec<u8> {
        let mut collection = vec![0u8; 20];
        let mut offsets = Vec::new();
        for _ in 0..2 {
            while !collection.len().is_multiple_of(4) {
                collection.push(0);
            }
            let base = collection.len();
            let mut copy = face.to_vec();
            let table_count = read_u16(&copy, 4).unwrap_or(0) as usize;
            for table in 0..table_count {
                let record = 12 + table * 16;
                let Some(offset) = read_u32(&copy, record + 8) else {
                    continue;
                };
                let absolute = offset.saturating_add(base as u32).to_be_bytes();
                if record + 12 <= copy.len() {
                    copy[record + 8..record + 12].copy_from_slice(&absolute);
                }
            }
            offsets.push(base as u32);
            collection.extend(copy);
        }
        collection[0..4].copy_from_slice(b"ttcf");
        collection[4..8].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        collection[8..12].copy_from_slice(&2u32.to_be_bytes());
        for (index, offset) in offsets.into_iter().enumerate() {
            let start = 12 + index * 4;
            collection[start..start + 4].copy_from_slice(&offset.to_be_bytes());
        }
        collection
    }

    /// Plan #57: valid TTC/OTC collections load every face and retain each
    /// face's metadata/shaping index; malformed collections still fail safely.
    #[test]
    fn test_font_collections_load_every_face() {
        let ttc = make_test_collection(get_fallback_font());
        assert!(inspect_font_metadata_at(&ttc, 0).is_some());
        assert!(inspect_font_metadata_at(&ttc, 1).is_some());
        let mut fm = FontManager::new();
        let first = fm
            .load_font_with_weight("Collection", &ttc, 400, false)
            .unwrap();
        assert_eq!(first, 0);
        assert!(fm.get_font(0).is_some());
        assert!(fm.get_font(1).is_some());
        assert_eq!(fm.shaping_data(0).unwrap().1, 0);
        assert_eq!(fm.shaping_data(1).unwrap().1, 1);
    }

    #[test]
    fn test_invalid_font_collections_rejected() {
        // Synthetic TTC header (magic + version + 1 face offset).
        let mut ttc = vec![0u8; 64];
        ttc[0..4].copy_from_slice(b"ttcf");
        ttc[4..8].copy_from_slice(&0x00010000u32.to_be_bytes());
        ttc[8..12].copy_from_slice(&1u32.to_be_bytes());
        let mut fm = FontManager::new();
        let err = fm
            .load_font_with_weight("Fam", &ttc, 400, false)
            .unwrap_err();
        assert!(
            err.contains("Failed to parse") || err.contains("InvalidFont"),
            "{err}"
        );
        assert!(inspect_font_metadata(&ttc).is_none());
        // A real font with TTC magic stamped on is also rejected.
        let mut stamped = get_fallback_font().to_vec();
        stamped[0..4].copy_from_slice(b"ttcf");
        assert!(fm
            .load_font_with_weight("Fam", &stamped, 400, false)
            .is_err());
        assert!(inspect_font_metadata(&stamped).is_none());
        // Unknown (non-sfnt, non-TTC) magics yield no metadata.
        assert!(inspect_font_metadata(b"wOF2garbage-payload........").is_none());
    }
}
