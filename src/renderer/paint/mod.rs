//! Event paint stages that run after style resolution and layout.
//!
//! The compositor owns orchestration and stateful glyph/karaoke traversal;
//! these modules own paint-side effects whose ordering is part of the ASS
//! rendering contract.

pub(super) mod clipping;
pub(super) mod decorations;
