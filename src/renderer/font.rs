use ab_glyph::FontArc;
use std::collections::HashMap;

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

    /// Load a font from bytes
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

        let key = format!("{}:{}:{}", name.to_lowercase(), is_bold, is_italic);
        self.name_index.insert(key, idx);

        // Set as fallback if it's the first font loaded
        if self.fallback_index.is_none() {
            self.fallback_index = Some(idx);
        }

        Ok(idx)
    }

    /// Load font from bytes with automatic bold/italic detection from name
    pub fn load_font_auto(&mut self, name: &str, data: &[u8]) -> Result<usize, String> {
        let lower = name.to_lowercase();
        let is_bold = lower.contains("bold") || lower.contains("-bold") || lower.contains("_bold");
        let is_italic = lower.contains("italic")
            || lower.contains("oblique")
            || lower.contains("-italic")
            || lower.contains("_italic");

        // Strip style indicators from name for lookup
        let base_name = lower
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
            .to_string();

        self.load_font(&base_name, data, is_bold, is_italic)
    }

    /// Find a font matching the requested name and style
    pub fn find_font(&self, name: &str, bold: bool, italic: bool) -> &FontArc {
        let lower = name.to_lowercase();

        // Try exact match first
        let key = format!("{}:{}:{}", lower, bold, italic);
        if let Some(&idx) = self.name_index.get(&key) {
            return &self.fonts[idx].font;
        }

        // Try with just the base name
        for (k, &idx) in &self.name_index {
            if k.starts_with(&lower) {
                let font = &self.fonts[idx];
                if font.is_bold == bold && font.is_italic == italic {
                    return &font.font;
                }
            }
        }

        // Try without style matching
        for (k, &idx) in &self.name_index {
            if k.starts_with(&lower) {
                return &self.fonts[idx].font;
            }
        }

        // Try partial name match
        for (k, &idx) in &self.name_index {
            if lower.contains(k.split(':').next().unwrap_or("")) || k.contains(&lower) {
                return &self.fonts[idx].font;
            }
        }

        // Fallback to first font
        self.fallback_index
            .map(|idx| &self.fonts[idx].font)
            .unwrap_or_else(|| {
                // If no fonts loaded, panic - this shouldn't happen
                panic!("No fonts loaded in FontManager")
            })
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

impl Default for FontManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Get the built-in fallback font (DejaVu Sans)
pub fn get_fallback_font() -> &'static [u8] {
    include_bytes!("../../fonts/DejaVuSans.ttf")
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
}
