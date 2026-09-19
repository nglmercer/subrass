use ab_glyph::{point, Font, FontArc, GlyphId, PxScale, ScaleFont};
use std::collections::HashMap;

use super::buffer::MAX_GLYPH_BITMAP_PIXELS;

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
    /// identity of `font`. `px_adjust` is the face's
    /// [`px_ratio`](super::font::FontManager::px_ratio): `ab_glyph`
    /// scales by the hhea-basis height, so the requested px scale is
    /// `font_size * px_adjust` for FreeType-basis pixels (identity
    /// for Win==hhea faces). It is a pure function of `font_id`, so
    /// the cache key needs no extra member. `faux_bold`/`faux_italic`
    /// describe synthesis the cache must apply (true only when the
    /// face lacks the style); they are part of the key because they
    /// change rasterization.
    #[allow(clippy::too_many_arguments)]
    pub fn get_or_rasterize(
        &mut self,
        font_id: usize,
        font: &FontArc,
        glyph_id: GlyphId,
        font_size: f64,
        px_adjust: f32,
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
            let glyph =
                Self::rasterize(font, glyph_id, font_size, px_adjust, faux_bold, faux_italic);
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
        px_adjust: f32,
        faux_bold: bool,
        faux_italic: bool,
    ) -> CachedGlyph {
        // Degenerate scales never reach ab_glyph: non-finite and
        // non-positive sizes are empty, and past f32::MAX the px scale
        // cannot even be represented (ab_glyph takes f32). The guard
        // runs on the adjusted px size, so a degenerate multiplier
        // degrades to empty rather than corrupting the raster.
        let px_size = font_size * f64::from(px_adjust);
        if !px_size.is_finite() || px_size <= 0.0 || px_size > f64::from(f32::MAX) {
            return CachedGlyph {
                bitmap: Vec::new(),
                width: 0,
                height: 0,
                bearing_x: 0.0,
                bearing_y: 0.0,
                advance: 0.0,
            };
        }
        let scale = PxScale::from(px_size as f32);
        let scaled = font.as_scaled(scale);

        // Get glyph outline - need to convert GlyphId to Glyph
        let glyph =
            glyph_id.with_scale_and_position(PxScale::from(px_size as f32), point(0.0, 0.0));
        let outlined = scaled.outline_glyph(glyph);

        match outlined {
            Some(outlined) => {
                let bounds = outlined.px_bounds();
                // Saturating `+ 2`: a hostile font size can push bounds
                // past `u32::MAX`, where `as u32` saturates and a plain
                // `+ 2` would overflow (debug panic). The budget check
                // below then degrades to an empty glyph.
                let width = (bounds.width().max(0.0) as u32).saturating_add(2);
                let height = (bounds.height().max(0.0) as u32).saturating_add(2);

                // Hostile font sizes must degrade to an empty glyph: the
                // u64 product can neither wrap (u32 `width * height` would)
                // nor over-allocate past the shared glyph budget.
                let pixels = u64::from(width) * u64::from(height);
                if width == 0 || height == 0 || pixels == 0 || pixels > MAX_GLYPH_BITMAP_PIXELS {
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
                let mut bitmap = vec![0u8; pixels as usize];

                // Draw outline
                outlined.draw(|x, y, coverage| {
                    let px = x as i32 + 1;
                    let py = y as i32 + 1;
                    if px >= 0 && px < width as i32 && py >= 0 && py < height as i32 {
                        let idx = (py as u32 * width + px as u32) as usize;
                        bitmap[idx] = (coverage * 255.0) as u8;
                    }
                });

                // Faux bold (libass/FT embolden): horizontal-only
                // ~1px spread at half coverage. Full-pixel dilation in
                // both axes overshoots badly (probe at 24px: ours +21%
                // ink vs libass +4%, with identical top/bottom/left
                // edges and +1px total width over five glyphs).
                if faux_bold {
                    let mut bold_bitmap = bitmap.clone();
                    for y in 0..height as i32 {
                        for x in 0..width as i32 {
                            let idx = (y as u32 * width + x as u32) as usize;
                            let cov = bitmap[idx];
                            if cov > 0 {
                                let nx = x + 1;
                                if nx < width as i32 {
                                    let nidx = (y as u32 * width + nx as u32) as usize;
                                    bold_bitmap[nidx] = bold_bitmap[nidx].max(cov / 2);
                                }
                            }
                        }
                    }
                    bitmap = bold_bitmap;
                }

                // Faux italic: shear top rows right (libass probe:
                // stem slope ≈ 0.36 over 14px, vs tan(12°) ≈ 0.21),
                // widening the bitmap to fit.
                let (bitmap, width, bearing_x) = if faux_italic {
                    let shear = 0.36_f32;
                    let extra = (height as f32 * shear).ceil().max(1.0) as u32;
                    let new_width = width.saturating_add(extra);
                    // u64 product: `new_width * height` in u32 could wrap
                    // for pathological near-budget bitmaps (debug panic,
                    // then OOB indexing). Over budget degrades to empty.
                    let sheared_pixels = u64::from(new_width) * u64::from(height);
                    if sheared_pixels == 0 || sheared_pixels > MAX_GLYPH_BITMAP_PIXELS {
                        return CachedGlyph {
                            bitmap: Vec::new(),
                            width: 0,
                            height: 0,
                            bearing_x: 0.0,
                            bearing_y: 0.0,
                            advance: scaled.h_advance(glyph_id),
                        };
                    }
                    let mut sheared = vec![0u8; sheared_pixels as usize];
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
        cache.get_or_rasterize(m0.id, m0.font, gid, 48.0, 1.0, false, false);
        assert_eq!(cache.len(), 1);
        // Same glyph ID/size, different font: must be a second entry.
        cache.get_or_rasterize(m1.id, m1.font, gid, 48.0, 1.0, false, false);
        assert_eq!(cache.len(), 2);
        // Repeat lookup hits, no growth.
        cache.get_or_rasterize(m0.id, m0.font, gid, 48.0, 1.0, false, false);
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn test_cache_distinguishes_faux_styles() {
        let fm = manager_with_two_fonts();
        let m0 = fm.find_font_with_match("DejaVu Sans", false, false);
        let mut cache = GlyphCache::new(64);
        let gid = m0.font.glyph_id('A');
        cache.get_or_rasterize(m0.id, m0.font, gid, 48.0, 1.0, false, false);
        cache.get_or_rasterize(m0.id, m0.font, gid, 48.0, 1.0, true, false);
        cache.get_or_rasterize(m0.id, m0.font, gid, 48.0, 1.0, false, true);
        assert_eq!(cache.len(), 3);
    }

    #[test]
    fn test_huge_font_size_degrades_to_empty_glyph() {
        // Hostile `\fs100000`-class sizes must return an empty (skipped)
        // glyph in bounded time/memory — never wrap `width * height` or
        // rasterize billions of pixels. Would take minutes / gigabytes
        // without the cap.
        let fm = manager_with_two_fonts();
        let m0 = fm.find_font_with_match("DejaVu Sans", false, false);
        let mut cache = GlyphCache::new(64);
        let gid = m0.font.glyph_id('A');
        let g = cache.get_or_rasterize(m0.id, m0.font, gid, 100_000.0, 1.0, false, false);
        assert_eq!((g.width, g.height), (0, 0));
        assert!(g.bitmap.is_empty());
        // Sane sizes still rasterize.
        let g = cache.get_or_rasterize(m0.id, m0.font, gid, 48.0, 1.0, false, false);
        assert!(g.width > 0 && g.height > 0);
        assert!(!g.bitmap.is_empty());
    }

    #[test]
    fn test_absurd_font_size_never_overflows_dims() {
        // Past `u32::MAX` pixels, `as u32` saturates: the `+ 2` padding
        // and the faux-italic widen must not overflow (debug panic) or
        // wrap (under-allocation + OOB). All degrade to empty glyphs.
        let fm = manager_with_two_fonts();
        let m0 = fm.find_font_with_match("DejaVu Sans", false, false);
        let mut cache = GlyphCache::new(64);
        let gid = m0.font.glyph_id('A');
        for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
            let g = cache.get_or_rasterize(m0.id, m0.font, gid, 1e12, 1.0, bold, italic);
            assert_eq!((g.width, g.height), (0, 0), "bold={bold} italic={italic}");
            assert!(g.bitmap.is_empty());
        }
    }

    #[test]
    fn test_eviction_is_bounded_and_keeps_new_key() {
        let fm = manager_with_two_fonts();
        let m0 = fm.find_font_with_match("DejaVu Sans", false, false);
        let mut cache = GlyphCache::new(4);
        for (i, ch) in "abcdefgh".chars().enumerate() {
            let gid = m0.font.glyph_id(ch);
            cache.get_or_rasterize(m0.id, m0.font, gid, 48.0, 1.0, false, false);
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
