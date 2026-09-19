//! Shared renderer safety limits.
//!
//! Keeping these caps outside the pixel-buffer implementation makes it
//! harder for glyph, drawing, clipping, and effect code to accidentally
//! establish different resource policies.

/// Maximum allowed frame-buffer dimension (width or height) in pixels.
pub const MAX_DIMENSION: u32 = 16_384;
/// Maximum total pixels in a single frame buffer (64 megapixels ≈ 256 MiB RGBA).
pub const MAX_BUFFER_PIXELS: u64 = 67_108_864;
/// Maximum total pixels in a transformed/scaled glyph coverage bitmap.
pub const MAX_GLYPH_BITMAP_PIXELS: u64 = 16_777_216;
/// Maximum box-blur radius applied to a buffer.
pub const MAX_BLUR_RADIUS: u32 = 128;
/// Maximum outline radius used by effect loops.
pub const MAX_OUTLINE_RADIUS: f64 = 128.0;
