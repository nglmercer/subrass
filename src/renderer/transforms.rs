//! Time-windowed and nested ASS transform application.

use super::ResolvedStyle;
use crate::types::{Event, OverrideTag};

impl super::Compositor {
    pub(super) fn apply_accel(t: f64, accel: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        if !accel.is_finite() {
            return t;
        }
        let k = t.powf(accel);
        if k.is_finite() {
            k
        } else {
            1.0
        }
    }

    /// Compute a `\t(...)` progress value against the event clock. A zero
    /// `t2` means the event end, matching libass's transform timing rules.
    pub(super) fn transform_progress(
        t1: i32,
        t2: i32,
        accel: f64,
        time_ms: u64,
        start_ms: u64,
        end_ms: u64,
    ) -> f64 {
        let elapsed = i64::try_from(time_ms.saturating_sub(start_ms)).unwrap_or(i64::MAX);
        let t1 = i64::from(t1);
        let duration = i64::try_from(end_ms.saturating_sub(start_ms)).unwrap_or(i64::MAX);
        let t2_eff = if t2 == 0 { duration } else { i64::from(t2) };
        let raw_progress = if elapsed < t1 {
            0.0
        } else if elapsed >= t2_eff || t2_eff <= t1 {
            1.0
        } else {
            (elapsed - t1) as f64 / (t2_eff - t1) as f64
        };
        if elapsed < t1 {
            0.0
        } else {
            Self::apply_accel(raw_progress, accel)
        }
    }

    /// Apply `\t(...)` inner tags with a given progress (0.0 to 1.0).
    ///
    /// Transformability matrix (per Aegisub/VSFilter: `\t` animates
    /// continuous style properties by interpolation):
    ///
    /// ```text
    /// animated:  \c \1c \2c \3c \4c \alpha \1a \2a \3a \4a
    ///            \fs (absolute lerps; \fs+N/-N scales by 1+p*d/10)
    ///            \fscx \fscy \fsp
    ///            \fr \frx \fry \frz \fax \fay
    ///            \bord \xbord \ybord \shad \xshad \yshad
    ///            \be \blur
    ///            rectangular \clip/\iclip coordinates
    /// consumed inside the transform. Discrete switches, positioning,
    /// fades, drawing state, karaoke, resets, and nested transforms are
    /// handled with libass's non-interpolated semantics.
    /// ```
    ///
    /// Tags without continuous fields still apply their libass discrete
    /// behavior inside `\t`; unsupported tags remain ignored.
    pub(super) fn apply_transform_tags(
        resolved: &mut ResolvedStyle,
        tags: &[OverrideTag],
        progress: f64,
        time_ms: u64,
        start_ms: u64,
        end_ms: u64,
    ) {
        Self::apply_transform_tags_depth(resolved, tags, progress, 0, time_ms, start_ms, end_ms);
    }

    pub(super) fn apply_transform_tags_depth(
        resolved: &mut ResolvedStyle,
        tags: &[OverrideTag],
        progress: f64,
        depth: u32,
        time_ms: u64,
        start_ms: u64,
        end_ms: u64,
    ) {
        for target in tags {
            match target {
                OverrideTag::Blur(b) => {
                    let from = resolved.blur;
                    resolved.blur = from + (b - from) * progress;
                }
                OverrideTag::EdgeBlur(b) => {
                    let from = resolved.blur;
                    resolved.blur = from + (b - from) * progress;
                }
                OverrideTag::Border(b) => {
                    let from = resolved.outline;
                    let v = from + (b - from) * progress;
                    resolved.outline = v;
                    resolved.outline_x = v;
                    resolved.outline_y = v;
                }
                OverrideTag::Shadow(s) => {
                    let from = resolved.shadow;
                    let v = from + (s - from) * progress;
                    resolved.shadow = v;
                    resolved.shadow_x = v;
                    resolved.shadow_y = v;
                }
                OverrideTag::PrimaryColor(c) => {
                    resolved.color = Self::interpolate_color(resolved.color, *c, progress);
                }
                OverrideTag::SecondaryColor(c) => {
                    resolved.secondary_color =
                        Self::interpolate_color(resolved.secondary_color, *c, progress);
                }
                OverrideTag::OutlineColor(c) => {
                    resolved.outline_color =
                        Self::interpolate_color(resolved.outline_color, *c, progress);
                }
                OverrideTag::ShadowColor(c) => {
                    resolved.shadow_color =
                        Self::interpolate_color(resolved.shadow_color, *c, progress);
                    resolved.back_color = resolved.shadow_color;
                }
                OverrideTag::Alpha(a) => {
                    let a = *a;
                    resolved.color.alpha = (resolved.color.alpha as f64
                        + (a as f64 - resolved.color.alpha as f64) * progress)
                        as u8;
                    resolved.secondary_color.alpha = (resolved.secondary_color.alpha as f64
                        + (a as f64 - resolved.secondary_color.alpha as f64) * progress)
                        as u8;
                    resolved.outline_color.alpha = (resolved.outline_color.alpha as f64
                        + (a as f64 - resolved.outline_color.alpha as f64) * progress)
                        as u8;
                    resolved.shadow_color.alpha = (resolved.shadow_color.alpha as f64
                        + (a as f64 - resolved.shadow_color.alpha as f64) * progress)
                        as u8;
                    resolved.back_color.alpha = resolved.shadow_color.alpha;
                }
                OverrideTag::PrimaryAlpha(a) => {
                    let from = resolved.color.alpha;
                    resolved.color.alpha =
                        (from as f64 + (*a as f64 - from as f64) * progress) as u8;
                }
                OverrideTag::SecondaryAlpha(a) => {
                    let from = resolved.secondary_color.alpha;
                    resolved.secondary_color.alpha =
                        (from as f64 + (*a as f64 - from as f64) * progress) as u8;
                }
                OverrideTag::OutlineAlpha(a) => {
                    let from = resolved.outline_color.alpha;
                    resolved.outline_color.alpha =
                        (from as f64 + (*a as f64 - from as f64) * progress) as u8;
                }
                OverrideTag::ShadowAlpha(a) => {
                    let from = resolved.shadow_color.alpha;
                    resolved.shadow_color.alpha =
                        (from as f64 + (*a as f64 - from as f64) * progress) as u8;
                    resolved.back_color.alpha = resolved.shadow_color.alpha;
                }
                OverrideTag::ScaleX(s) => {
                    let from = resolved.scale_x;
                    resolved.scale_x = from + (s - from) * progress;
                }
                OverrideTag::ScaleY(s) => {
                    let from = resolved.scale_y;
                    resolved.scale_y = from + (s - from) * progress;
                }
                // libass `\t` + `\fs`: absolute targets interpolate
                // linearly; relative deltas scale the current size by
                // (1 + p * d / 10). Non-positive results reset to style.
                OverrideTag::FontSize(s) => {
                    let from = resolved.font_size;
                    let next = from + (s - from) * progress;
                    if next.is_finite() && next > 0.0 {
                        resolved.font_size = next;
                    } else {
                        resolved.font_size = resolved.base_style.font_size;
                    }
                }
                OverrideTag::FontSizeRelative(delta) => {
                    let from = resolved.font_size;
                    let next = from * (1.0 + progress * delta / 10.0);
                    if next.is_finite() && next > 0.0 {
                        resolved.font_size = next;
                    } else {
                        resolved.font_size = resolved.base_style.font_size;
                    }
                }
                OverrideTag::FontSizeReset => {
                    resolved.font_size = resolved.base_style.font_size;
                }
                OverrideTag::LetterSpacing(s) => {
                    let from = resolved.spacing;
                    resolved.spacing = from + (s - from) * progress;
                }
                OverrideTag::BorderX(b) => {
                    let from = resolved.outline_x;
                    resolved.outline_x = from + (b - from) * progress;
                    resolved.outline = (resolved.outline_x + resolved.outline_y) / 2.0;
                }
                OverrideTag::BorderY(b) => {
                    let from = resolved.outline_y;
                    resolved.outline_y = from + (b - from) * progress;
                    resolved.outline = (resolved.outline_x + resolved.outline_y) / 2.0;
                }
                OverrideTag::ShadowX(s) => {
                    let from = resolved.shadow_x;
                    resolved.shadow_x = from + (s - from) * progress;
                    resolved.shadow = (resolved.shadow_x + resolved.shadow_y) / 2.0;
                }
                OverrideTag::ShadowY(s) => {
                    let from = resolved.shadow_y;
                    resolved.shadow_y = from + (s - from) * progress;
                    resolved.shadow = (resolved.shadow_x + resolved.shadow_y) / 2.0;
                }
                OverrideTag::ShearX(s) => {
                    let from = resolved.shear_x;
                    resolved.shear_x = from + (s - from) * progress;
                }
                OverrideTag::ShearY(s) => {
                    let from = resolved.shear_y;
                    resolved.shear_y = from + (s - from) * progress;
                }
                OverrideTag::RotationZ(r) => {
                    let from = resolved.angle;
                    resolved.angle = from + (r - from) * progress;
                }
                OverrideTag::RotationX(r) => {
                    let from = resolved.rotation_x;
                    resolved.rotation_x = from + (r - from) * progress;
                }
                OverrideTag::RotationY(r) => {
                    let from = resolved.rotation_y;
                    resolved.rotation_y = from + (r - from) * progress;
                }
                OverrideTag::Clip(x0, y0, x1, y1) | OverrideTag::InverseClip(x0, y0, x1, y1) => {
                    // Rectangular clips are interpolated coordinate by
                    // coordinate by libass. The default clip is the full
                    // PlayRes canvas used by the renderer's default style.
                    let from = resolved.clip.or(resolved.inverse_clip).unwrap_or((
                        0,
                        0,
                        resolved.clip_canvas.0,
                        resolved.clip_canvas.1,
                    ));
                    let lerp = |a: i32, b: i32| {
                        // libass converts the interpolated ASS coordinate
                        // with an integer cast (truncate toward zero), not
                        // with mathematical rounding.
                        (f64::from(a) * (1.0 - progress) + f64::from(b) * progress) as i32
                    };
                    let rect = (
                        lerp(from.0, *x0),
                        lerp(from.1, *y0),
                        lerp(from.2, *x1),
                        lerp(from.3, *y1),
                    );
                    if matches!(target, OverrideTag::Clip(..)) {
                        resolved.clip = Some(rect);
                        resolved.inverse_clip = None;
                    } else {
                        resolved.inverse_clip = Some(rect);
                        resolved.clip = None;
                    }
                }
                OverrideTag::Transform {
                    t1,
                    t2,
                    accel,
                    tags: nested,
                } => {
                    // Nested transforms are recursively applied, with the
                    // parser/runtime depth caps preventing unbounded work.
                    if depth < 32 {
                        let nested_progress =
                            Self::transform_progress(*t1, *t2, *accel, time_ms, start_ms, end_ms);
                        Self::apply_transform_tags_depth(
                            resolved,
                            nested,
                            nested_progress,
                            depth + 1,
                            time_ms,
                            start_ms,
                            end_ms,
                        );
                    }
                }
                // Discrete and event-global tags are consumed inside `\t`.
                _ => Self::apply_single_tag(resolved, target),
            }
        }
    }

    /// Apply only the event-global portion of a transform.  The normal
    /// segment resolver also walks transform tags, but event-global tags
    /// must be resolved before wrapping and block geometry are computed.
    /// Keeping this filtered pass separate prevents a `\t(\fs...)` from
    /// being applied once to the event and again to each segment.
    pub(super) fn apply_event_global_transform_tags(
        resolved: &mut ResolvedStyle,
        tags: &[OverrideTag],
        progress: f64,
        depth: u32,
        time_ms: u64,
        start_ms: u64,
        end_ms: u64,
    ) {
        for target in tags {
            match target {
                OverrideTag::Transform {
                    t1,
                    t2,
                    accel,
                    tags: nested,
                } if depth < 32 => {
                    let nested_progress =
                        Self::transform_progress(*t1, *t2, *accel, time_ms, start_ms, end_ms);
                    Self::apply_event_global_transform_tags(
                        resolved,
                        nested,
                        nested_progress,
                        depth + 1,
                        time_ms,
                        start_ms,
                        end_ms,
                    );
                }
                OverrideTag::Transform { .. } => {}
                OverrideTag::Clip(x0, y0, x1, y1) | OverrideTag::InverseClip(x0, y0, x1, y1) => {
                    let from = resolved.clip.or(resolved.inverse_clip).unwrap_or((
                        0,
                        0,
                        resolved.clip_canvas.0,
                        resolved.clip_canvas.1,
                    ));
                    let lerp = |a: i32, b: i32| {
                        (f64::from(a) * (1.0 - progress) + f64::from(b) * progress) as i32
                    };
                    let rect = (
                        lerp(from.0, *x0),
                        lerp(from.1, *y0),
                        lerp(from.2, *x1),
                        lerp(from.3, *y1),
                    );
                    if matches!(target, OverrideTag::Clip(..)) {
                        resolved.clip = Some(rect);
                        resolved.inverse_clip = None;
                    } else {
                        resolved.inverse_clip = Some(rect);
                        resolved.clip = None;
                    }
                }
                tag if tag.is_event_layout() => Self::apply_single_tag(resolved, tag),
                _ => {}
            }
        }
    }

    /// Segment-local transform walk used after the event-global pass.  It
    /// retains all font, color, geometry, drawing, and karaoke behavior,
    /// while leaving the already-resolved event layout fields untouched.
    pub(super) fn apply_transform_segment_tags(
        resolved: &mut ResolvedStyle,
        tags: &[OverrideTag],
        progress: f64,
        depth: u32,
        time_ms: u64,
        start_ms: u64,
        end_ms: u64,
    ) {
        for target in tags {
            match target {
                OverrideTag::Transform {
                    t1,
                    t2,
                    accel,
                    tags: nested,
                } if depth < 32 => {
                    let nested_progress =
                        Self::transform_progress(*t1, *t2, *accel, time_ms, start_ms, end_ms);
                    Self::apply_transform_segment_tags(
                        resolved,
                        nested,
                        nested_progress,
                        depth + 1,
                        time_ms,
                        start_ms,
                        end_ms,
                    );
                }
                OverrideTag::Transform { .. } => {}
                tag if !tag.is_event_layout() => Self::apply_transform_tags_depth(
                    resolved,
                    std::slice::from_ref(target),
                    progress,
                    depth,
                    time_ms,
                    start_ms,
                    end_ms,
                ),
                _ => {}
            }
        }
    }

    /// Resolve the event-global state at a frame timestamp.  This is the
    /// render-level counterpart of `resolve_style`: it evaluates all
    /// top-level transforms in source order and exposes the resulting
    /// position, origin, alignment, wrapping, fade, and clip state to the
    /// complete event render.
    pub(super) fn resolve_event_globals_at_time(
        resolved: &ResolvedStyle,
        event: &Event,
        time_ms: u64,
        start_ms: u64,
        end_ms: u64,
        play_res_x: u32,
        play_res_y: u32,
    ) -> ResolvedStyle {
        let mut frame = resolved.clone();
        frame.clip_canvas = (
            play_res_x.min(i32::MAX as u32) as i32,
            play_res_y.min(i32::MAX as u32) as i32,
        );
        for tag in &event.parsed_tags {
            if let OverrideTag::Transform {
                t1,
                t2,
                accel,
                tags,
            } = tag
            {
                let progress =
                    Self::transform_progress(*t1, *t2, *accel, time_ms, start_ms, end_ms);
                Self::apply_event_global_transform_tags(
                    &mut frame, tags, progress, 0, time_ms, start_ms, end_ms,
                );
            }
        }
        frame.event_globals_applied = true;
        frame
    }
}
