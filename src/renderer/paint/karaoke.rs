//! Karaoke paint primitives shared by the text paint stage.

pub(super) use super::super::karaoke::{
    build_karaoke_runs, karaoke_outline_suppressed, paint_glyph_fill, BufferedSweepGlyph,
    GlyphGeom, KaraokeKind, SweepState, MAX_SWEEP_BUFFER_BYTES,
};
