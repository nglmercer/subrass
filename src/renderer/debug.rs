//! Opt-in frame metadata for interactive inspection. No pixels or glyphs retained.
use serde::Serialize;

/// Maximum events/runs retained in a debug snapshot, independent of document size.
pub(crate) const MAX_DEBUG_EVENTS: usize = 256;
pub(crate) const MAX_DEBUG_RUNS: usize = 64;

/// Layout geometry and run properties from the last painted frame.
/// Bounds include borders and collision displacement, but precede rotation,
/// shear, perspective, clipping, shadow and blur. They are not ink bounds.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EventDebug {
    /// Index in the document's event array (including comments).
    pub event_index: usize,
    /// Video pixels: left, top, right, bottom.
    pub layout_bounds: [f64; 4],
    pub collision_eligible: bool,
    pub collision_shift: f64,
    pub alignment: i32,
    /// Effective transform pivot in video pixels.
    pub origin: [f64; 2],
    pub opacity: f64,
    pub run_count: usize,
    pub runs: Vec<RunDebug>,
}

/// Effective properties of one laid-out run at the rendered timestamp.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunDebug {
    /// Requested family, capped to 256 Unicode scalars plus an ellipsis.
    pub font_name: String,
    pub font_size: f64,
    /// Requested color in conventional RGBA; fade/karaoke coverage is separate.
    pub fill: [u8; 4],
    pub outline_color: [u8; 4],
    pub scale: [f64; 2],
    /// Degrees: X, Y, Z.
    pub rotation: [f64; 3],
    pub shear: [f64; 2],
    pub border: [f64; 2],
    pub shadow: [f64; 2],
    pub blur: f64,
    pub edge_blur: u32,
    pub drawing_mode: i32,
}
