use ab_glyph::{point, Font, FontArc, GlyphId, PxScale, ScaleFont};
use std::collections::HashMap;

/// Cached rasterized glyph
#[derive(Debug, Clone)]
pub struct CachedGlyph {
    pub bitmap: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub bearing_x: f32,
    pub bearing_y: f32,
    pub advance: f32,
}

/// Cache key for glyphs. The font identity is part of the key: the same
/// glyph ID has different geometry in different fonts.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct GlyphCacheKey {
    font_id: usize,
    glyph_id: u32,
    font_size_bits: u32, // f32 to bits for exact match
    faux_bold: bool,
    faux_italic: bool,
}

/// Glyph rasterization cache with deterministic least-recently-used eviction.
pub struct GlyphCache {
    cache: HashMap<GlyphCacheKey, (CachedGlyph, u64)>,
    max_size: usize,
    tick: u64,
}

impl GlyphCache {
    pub fn new(max_size: usize) -> Self {
        Self {
            cache: HashMap::with_capacity(max_size.min(1024)),
            max_size: max_size.max(1),
            tick: 0,
        }
    }

    /// Get or rasterize a glyph.
    ///
    /// `font_id` is the stable [`FontManager`](super::font::FontManager)
    /// identity of `font`. `faux_bold`/`faux_italic` describe synthesis the
    /// cache must apply (true only when the face lacks the style); they are
    /// part of the key because they change rasterization.
    pub fn get_or_rasterize(
        &mut self,
        font_id: usize,
        font: &FontArc,
        glyph_id: GlyphId,
        font_size: f64,
        faux_bold: bool,
        faux_italic: bool,
    ) -> &CachedGlyph {
        let key = GlyphCacheKey {
            font_id,
            glyph_id: glyph_id.0 as u32,
            font_size_bits: (font_size as f32).to_bits(),
            faux_bold,
            faux_italic,
        };

        self.tick = self.tick.wrapping_add(1);
        let tick = self.tick;
        if let Some(entry) = self.cache.get_mut(&key) {
            entry.1 = tick;
        } else {
            let glyph = Self::rasterize(font, glyph_id, font_size, faux_bold, faux_italic);
            self.cache.insert(key.clone(), (glyph, tick));
            self.evict_if_needed(&key);
        }

        &self.cache[&key].0
    }

    /// Evict least-recently-used entries (deterministic: oldest tick first,
    /// ties broken by key order). Never evicts the just-inserted key.
    fn evict_if_needed(&mut self, protect: &GlyphCacheKey) {
        if self.cache.len() <= self.max_size {
            return;
        }
        let target = self.max_size / 2;
        let mut entries: Vec<(GlyphCacheKey, u64)> = self
            .cache
            .iter()
            .filter(|(k, _)| *k != protect)
            .map(|(k, (_, t))| (k.clone(), *t))
            .collect();
        // Deterministic order: tick, then key debug form. No HashMap
        // iteration order leaks into the eviction choice.
        entries.sort_by(|a, b| {
            a.1.cmp(&b.1)
                .then_with(|| format!("{:?}", a.0).cmp(&format!("{:?}", b.0)))
        });
        let remove_count = self.cache.len().saturating_sub(target.max(1));
        for (key, _) in entries.into_iter().take(remove_count) {
            self.cache.remove(&key);
        }
    }

    /// Rasterize a glyph to bitmap, applying faux bold (dilation) and faux
    /// italic (horizontal shear) only when the corresponding flag is set.
    fn rasterize(
        font: &FontArc,
        glyph_id: GlyphId,
        font_size: f64,
        faux_bold: bool,
        faux_italic: bool,
    ) -> CachedGlyph {
        let scale = PxScale::from(font_size as f32);
        let scaled = font.as_scaled(scale);

        // Get glyph outline - need to convert GlyphId to Glyph
        let glyph =
            glyph_id.with_scale_and_position(PxScale::from(font_size as f32), point(0.0, 0.0));
        let outlined = scaled.outline_glyph(glyph);

        match outlined {
            Some(outlined) => {
                let bounds = outlined.px_bounds();
                let width = (bounds.width().max(0.0) as u32) + 2;
                let height = (bounds.height().max(0.0) as u32) + 2;

                if width == 0 || height == 0 {
                    return CachedGlyph {
                        bitmap: Vec::new(),
                        width: 0,
                        height: 0,
                        bearing_x: 0.0,
                        bearing_y: 0.0,
                        advance: scaled.h_advance(glyph_id),
                    };
                }

                // Rasterize glyph
                let mut bitmap = vec![0u8; (width * height) as usize];

                // Draw outline
                outlined.draw(|x, y, coverage| {
                    let px = x as i32 + 1;
                    let py = y as i32 + 1;
                    if px >= 0 && px < width as i32 && py >= 0 && py < height as i32 {
                        let idx = (py as u32 * width + px as u32) as usize;
                        bitmap[idx] = (coverage * 255.0) as u8;
                    }
                });

                // Apply faux bold by dilating
                if faux_bold {
                    let mut bold_bitmap = bitmap.clone();
                    for y in 0..height as i32 {
                        for x in 0..width as i32 {
                            let idx = (y as u32 * width + x as u32) as usize;
                            if bitmap[idx] > 0 {
                                // Expand to neighbors
                                for dy in -1i32..=1 {
                                    for dx in 0i32..=1 {
                                        let nx = x + dx;
                                        let ny = y + dy;
                                        if nx >= 0
                                            && nx < width as i32
                                            && ny >= 0
                                            && ny < height as i32
                                        {
                                            let nidx = (ny as u32 * width + nx as u32) as usize;
                                            bold_bitmap[nidx] = bold_bitmap[nidx].max(bitmap[idx]);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    bitmap = bold_bitmap;
                }

                // Apply faux italic: shear top rows right by tan(12°) ≈ 0.21
                // per row, widening the bitmap to fit.
                let (bitmap, width, bearing_x) = if faux_italic {
                    let shear = 0.2126_f32;
                    let extra = (height as f32 * shear).ceil().max(1.0) as u32;
                    let new_width = width + extra;
                    let mut sheared = vec![0u8; (new_width * height) as usize];
                    for y in 0..height {
                        let shift = ((height - 1 - y) as f32 * shear).round() as u32;
                        for x in 0..width {
                            let v = bitmap[(y * width + x) as usize];
                            if v > 0 {
                                sheared[(y * new_width + x + shift) as usize] = v;
                            }
                        }
                    }
                    (sheared, new_width, bounds.min.x)
                } else {
                    (bitmap, width, bounds.min.x)
                };

                CachedGlyph {
                    bitmap,
                    width,
                    height,
                    bearing_x,
                    bearing_y: bounds.min.y,
                    advance: scaled.h_advance(glyph_id),
                }
            }
            None => {
                // No outline - return empty glyph (space, etc.)
                CachedGlyph {
                    bitmap: Vec::new(),
                    width: 0,
                    height: 0,
                    bearing_x: 0.0,
                    bearing_y: 0.0,
                    advance: scaled.h_advance(glyph_id),
                }
            }
        }
    }

    /// Clear the cache
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// Get cache size
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

impl Default for GlyphCache {
    fn default() -> Self {
        Self::new(4096)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::font::{self, FontManager};

    fn manager_with_two_fonts() -> FontManager {
        let mut fm = FontManager::new();
        fm.load_font("DejaVu Sans", font::get_fallback_font(), false, false)
            .unwrap();
        // Second font with the same data but a distinct identity: the cache
        // must still treat it as a different font (geometry may differ).
        fm.load_font("Second Face", font::get_fallback_font(), false, false)
            .unwrap();
        fm
    }

    #[test]
    fn test_cache_distinguishes_fonts() {
        let fm = manager_with_two_fonts();
        let m0 = fm.find_font_with_match("DejaVu Sans", false, false);
        let m1 = fm.find_font_with_match("Second Face", false, false);
        assert_ne!(m0.id, m1.id);

        let mut cache = GlyphCache::new(64);
        let gid = m0.font.glyph_id('A');
        cache.get_or_rasterize(m0.id, m0.font, gid, 48.0, false, false);
        assert_eq!(cache.len(), 1);
        // Same glyph ID/size, different font: must be a second entry.
        cache.get_or_rasterize(m1.id, m1.font, gid, 48.0, false, false);
        assert_eq!(cache.len(), 2);
        // Repeat lookup hits, no growth.
        cache.get_or_rasterize(m0.id, m0.font, gid, 48.0, false, false);
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn test_cache_distinguishes_faux_styles() {
        let fm = manager_with_two_fonts();
        let m0 = fm.find_font_with_match("DejaVu Sans", false, false);
        let mut cache = GlyphCache::new(64);
        let gid = m0.font.glyph_id('A');
        cache.get_or_rasterize(m0.id, m0.font, gid, 48.0, false, false);
        cache.get_or_rasterize(m0.id, m0.font, gid, 48.0, true, false);
        cache.get_or_rasterize(m0.id, m0.font, gid, 48.0, false, true);
        assert_eq!(cache.len(), 3);
    }

    #[test]
    fn test_eviction_is_bounded_and_keeps_new_key() {
        let fm = manager_with_two_fonts();
        let m0 = fm.find_font_with_match("DejaVu Sans", false, false);
        let mut cache = GlyphCache::new(4);
        for (i, ch) in "abcdefgh".chars().enumerate() {
            let gid = m0.font.glyph_id(ch);
            cache.get_or_rasterize(m0.id, m0.font, gid, 48.0, false, false);
            // The just-inserted glyph is always present
            let key = GlyphCacheKey {
                font_id: m0.id,
                glyph_id: gid.0 as u32,
                font_size_bits: (48.0f32).to_bits(),
                faux_bold: false,
                faux_italic: false,
            };
            assert!(cache.cache.contains_key(&key), "iteration {}", i);
            assert!(cache.len() <= 4, "len {}", cache.len());
        }
    }
}
