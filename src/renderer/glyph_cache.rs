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

/// Cache key for glyphs
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct GlyphCacheKey {
    glyph_id: u32,
    font_size_bits: u32, // f32 to bits for exact match
    bold: bool,
    italic: bool,
}

/// Glyph rasterization cache
pub struct GlyphCache {
    cache: HashMap<GlyphCacheKey, CachedGlyph>,
    max_size: usize,
}

impl GlyphCache {
    pub fn new(max_size: usize) -> Self {
        Self {
            cache: HashMap::with_capacity(max_size.min(1024)),
            max_size,
        }
    }

    /// Get or rasterize a glyph
    pub fn get_or_rasterize(
        &mut self,
        font: &FontArc,
        glyph_id: GlyphId,
        font_size: f64,
        bold: bool,
        italic: bool,
    ) -> &CachedGlyph {
        let key = GlyphCacheKey {
            glyph_id: glyph_id.0 as u32,
            font_size_bits: (font_size as f32).to_bits(),
            bold,
            italic,
        };

        if !self.cache.contains_key(&key) {
            let glyph = self.rasterize(font, glyph_id, font_size, bold, italic);
            self.cache.insert(key.clone(), glyph);

            // Evict if too large
            if self.cache.len() > self.max_size {
                // Simple eviction: clear half
                let keys: Vec<_> = self.cache.keys().take(self.max_size / 2).cloned().collect();
                for k in keys {
                    self.cache.remove(&k);
                }
            }
        }

        &self.cache[&key]
    }

    /// Rasterize a glyph to bitmap
    fn rasterize(
        &self,
        font: &FontArc,
        glyph_id: GlyphId,
        font_size: f64,
        bold: bool,
        _italic: bool,
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
                if bold {
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

                CachedGlyph {
                    bitmap,
                    width,
                    height,
                    bearing_x: bounds.min.x,
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
