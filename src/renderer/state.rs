//! Render-state value objects shared by transform, layout, karaoke, and paint stages.

use crate::types::color::Color;
use crate::types::Style;

/// Resolved style with all overrides applied
#[derive(Debug, Clone)]
pub struct ResolvedStyle {
    pub base_style: Style,
    pub font_name: String,
    pub font_size: f64,
    /// Font encoding/charset id (`\fe`, default from the style).
    /// Stored and reset correctly; glyph selection stays Unicode-based.
    pub font_encoding: i32,
    pub color: Color,
    pub secondary_color: Color,
    pub outline_color: Color,
    pub shadow_color: Color,
    pub back_color: Color,
    /// ASS font weight: 400 = normal, 700 = bold (from `\b` or the style).
    pub font_weight: u16,
    pub italic: bool,
    pub underline: bool,
    pub strike_out: bool,
    pub scale_x: f64,
    pub scale_y: f64,
    pub spacing: f64,
    pub angle: f64,
    pub rotation_x: f64,
    pub rotation_y: f64,
    pub border_style: i32,
    pub outline: f64,
    pub outline_x: f64,
    pub outline_y: f64,
    pub shadow: f64,
    pub shadow_x: f64,
    pub shadow_y: f64,
    pub shear_x: f64,
    pub shear_y: f64,
    pub alignment: i32,
    /// libass `PARSED_A`: an `\an`/`\a` tag was already consumed for
    /// this event, so later alignment tags are ignored (first wins).
    /// Persists across `\r`, which never resets alignment in libass.
    pub parsed_alignment: bool,
    pub margin_l: i32,
    pub margin_r: i32,
    pub margin_v: i32,
    pub position: Option<(f64, f64)>,
    pub origin: Option<(f64, f64)>,
    pub move_data: Option<MoveData>,
    pub clip: Option<(i32, i32, i32, i32)>,
    pub inverse_clip: Option<(i32, i32, i32, i32)>,
    pub clip_vector: Option<VectorClip>,
    pub inverse_clip_vector: Option<VectorClip>,
    pub fade_in: i32,
    pub fade_out: i32,
    pub complex_fade: Option<ComplexFade>,
    /// libass `PARSED_FADE`: a `\fad`/`\fade` tag was already consumed
    /// for this event, so later fade tags are ignored (first wins).
    /// Needed because `\fad(0,0)` is otherwise indistinguishable from
    /// "no fade tag". Persists across `\r` with the fade values.
    pub parsed_fade: bool,
    /// Effective per-event `\q` override.  Keeping it in the resolved
    /// state makes transformed `\q` participate in wrapping at the same
    /// timestamp as every other event-global property.
    pub wrap_style: Option<i32>,
    /// Script-space canvas used as the initial rectangular clip while a
    /// transform is interpolating a clip that did not previously exist.
    pub clip_canvas: (i32, i32),
    /// Set only for the frame-level state.  Segment resolution uses this
    /// marker to avoid applying event-global transform targets twice.
    pub(super) event_globals_applied: bool,
    /// ASS-2 layout resolution used for blur and unscaled border/shadow
    /// metrics. Zero (either axis) means unset: the video size is used
    /// (the libass storage-size role), so blur and unscaled borders are
    /// 1:1 in video pixels.
    pub layout_res_x: u32,
    pub layout_res_y: u32,
    pub drawing_mode: i32,
    pub drawing_baseline_offset: f64,
    pub blur: f64,
    /// Script `ScaledBorderAndShadow` flag: when true (default), borders
    /// and shadows scale with the script-to-video resolution ratio.
    pub scaled_border_and_shadow: bool,
    /// Script `Kerning:` flag (default off, like libass): enables the
    /// OpenType `kern` feature during shaping.
    pub kerning: bool,
}

/// Vector clip shape in script coordinates with a drawing scale.
#[derive(Debug, Clone)]
pub struct VectorClip {
    pub scale: i32,
    pub drawing: String,
}

/// Line-global state preserved across `\r` resets: the non-style
/// line properties (`\pos`, `\move`, `\org`, `\clip`, `\iclip`,
/// `\fad`, `\fade`) plus alignment, drawing mode, and `\pbo`: libass
/// `ass_reset_render_context` (the `\r` handler) never touches
/// alignment, `drawing_scale`, or `pbo`, so the first `\an`/`\a` and
/// the current drawing state survive resets. Karaoke timing also
/// survives (the reset never touches the effect fields); it is
/// computed from accumulated tags, which `\r` does not clear.
/// Everything else — fonts, colors, border/shadow, rotation —
/// resets to the target style, because `\r` restores ordinary
/// override state.
pub(super) struct LineGlobalKeep {
    position: Option<(f64, f64)>,
    origin: Option<(f64, f64)>,
    move_data: Option<MoveData>,
    clip: Option<(i32, i32, i32, i32)>,
    inverse_clip: Option<(i32, i32, i32, i32)>,
    clip_vector: Option<VectorClip>,
    inverse_clip_vector: Option<VectorClip>,
    fade_in: i32,
    fade_out: i32,
    complex_fade: Option<ComplexFade>,
    alignment: i32,
    parsed_alignment: bool,
    parsed_fade: bool,
    wrap_style: Option<i32>,
    clip_canvas: (i32, i32),
    event_globals_applied: bool,
    layout_res_x: u32,
    layout_res_y: u32,
    drawing_mode: i32,
    drawing_baseline_offset: f64,
}

impl LineGlobalKeep {
    pub(super) fn capture(resolved: &ResolvedStyle) -> Self {
        Self {
            position: resolved.position,
            origin: resolved.origin,
            move_data: resolved.move_data.clone(),
            clip: resolved.clip,
            inverse_clip: resolved.inverse_clip,
            clip_vector: resolved.clip_vector.clone(),
            inverse_clip_vector: resolved.inverse_clip_vector.clone(),
            fade_in: resolved.fade_in,
            fade_out: resolved.fade_out,
            complex_fade: resolved.complex_fade.clone(),
            alignment: resolved.alignment,
            parsed_alignment: resolved.parsed_alignment,
            parsed_fade: resolved.parsed_fade,
            wrap_style: resolved.wrap_style,
            clip_canvas: resolved.clip_canvas,
            event_globals_applied: resolved.event_globals_applied,
            layout_res_x: resolved.layout_res_x,
            layout_res_y: resolved.layout_res_y,
            drawing_mode: resolved.drawing_mode,
            drawing_baseline_offset: resolved.drawing_baseline_offset,
        }
    }

    pub(super) fn restore(self, resolved: &mut ResolvedStyle) {
        resolved.position = self.position;
        resolved.origin = self.origin;
        resolved.move_data = self.move_data;
        resolved.clip = self.clip;
        resolved.inverse_clip = self.inverse_clip;
        resolved.clip_vector = self.clip_vector;
        resolved.inverse_clip_vector = self.inverse_clip_vector;
        resolved.fade_in = self.fade_in;
        resolved.fade_out = self.fade_out;
        resolved.complex_fade = self.complex_fade;
        resolved.alignment = self.alignment;
        resolved.parsed_alignment = self.parsed_alignment;
        resolved.parsed_fade = self.parsed_fade;
        resolved.wrap_style = self.wrap_style;
        resolved.clip_canvas = self.clip_canvas;
        resolved.event_globals_applied = self.event_globals_applied;
        resolved.layout_res_x = self.layout_res_x;
        resolved.layout_res_y = self.layout_res_y;
        resolved.drawing_mode = self.drawing_mode;
        resolved.drawing_baseline_offset = self.drawing_baseline_offset;
    }
}

/// Move animation data: times are `i32` like libass (`argtoi32`),
/// already swapped so `t1 <= t2`.
#[derive(Debug, Clone)]
pub struct MoveData {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub t1: i32,
    pub t2: i32,
}

/// Complex fade data: all `i32` like libass. Alpha interpolates
/// full-range and truncates exactly like `interpolate_alpha`.
#[derive(Debug, Clone)]
pub struct ComplexFade {
    pub a1: i32,
    pub a2: i32,
    pub a3: i32,
    pub t1: i32,
    pub t2: i32,
    pub t3: i32,
    pub t4: i32,
}

impl ResolvedStyle {
    /// libass `split_style_runs` key: two segments share a karaoke run
    /// only when every render-affecting field matches. Line-global
    /// state (position, clips, fades, alignment, margins) is excluded,
    /// exactly like upstream.
    pub(super) fn same_karaoke_run(&self, other: &Self) -> bool {
        self.font_name == other.font_name
            && self.font_size == other.font_size
            && self.color == other.color
            && self.secondary_color == other.secondary_color
            && self.outline_color == other.outline_color
            && self.shadow_color == other.shadow_color
            && self.back_color == other.back_color
            && self.font_weight == other.font_weight
            && self.italic == other.italic
            && self.underline == other.underline
            && self.strike_out == other.strike_out
            && self.scale_x == other.scale_x
            && self.scale_y == other.scale_y
            && self.spacing == other.spacing
            && self.angle == other.angle
            && self.rotation_x == other.rotation_x
            && self.rotation_y == other.rotation_y
            && self.border_style == other.border_style
            && self.outline == other.outline
            && self.outline_x == other.outline_x
            && self.outline_y == other.outline_y
            && self.shadow == other.shadow
            && self.shadow_x == other.shadow_x
            && self.shadow_y == other.shadow_y
            && self.shear_x == other.shear_x
            && self.shear_y == other.shear_y
            && self.blur == other.blur
    }
}
