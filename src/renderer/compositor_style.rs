use super::state::{ComplexFade, MoveData, ResolvedStyle, VectorClip};
use super::Compositor;
use crate::types::color::Color;
use crate::types::override_tag::parse_text_segments;
use crate::types::{Event, OverrideTag, Style};

impl Compositor {
    /// Interpolate between two colors
    pub(super) fn interpolate_color(from: Color, to: Color, t: f64) -> Color {
        let t = t.clamp(0.0, 1.0);
        Color {
            alpha: (from.alpha as f64 + (to.alpha as f64 - from.alpha as f64) * t) as u8,
            red: (from.red as f64 + (to.red as f64 - from.red as f64) * t) as u8,
            green: (from.green as f64 + (to.green as f64 - from.green as f64) * t) as u8,
            blue: (from.blue as f64 + (to.blue as f64 - from.blue as f64) * t) as u8,
        }
    }

    pub(super) fn resolve_base_style(base_style: &Style, tags: &[OverrideTag]) -> ResolvedStyle {
        let mut resolved = ResolvedStyle {
            base_style: base_style.clone(),
            font_name: base_style.font_name.clone(),
            font_size: base_style.font_size,
            font_encoding: base_style.encoding,
            color: base_style.primary_color,
            secondary_color: base_style.secondary_color,
            outline_color: base_style.outline_color,
            shadow_color: base_style.back_color,
            back_color: base_style.back_color,
            // Style Bold is boolean (-1/0): bold style means weight 700.
            font_weight: if base_style.bold { 700 } else { 400 },
            italic: base_style.italic,
            underline: base_style.underline,
            strike_out: base_style.strike_out,
            scale_x: base_style.scale_x,
            scale_y: base_style.scale_y,
            spacing: base_style.spacing,
            angle: base_style.angle,
            rotation_x: 0.0,
            rotation_y: 0.0,
            border_style: base_style.border_style,
            outline: base_style.outline,
            outline_x: base_style.outline,
            outline_y: base_style.outline,
            shadow: base_style.shadow,
            shadow_x: base_style.shadow,
            shadow_y: base_style.shadow,
            shear_x: 0.0,
            shear_y: 0.0,
            alignment: base_style.alignment,
            parsed_alignment: false,
            margin_l: base_style.margin_l,
            margin_r: base_style.margin_r,
            margin_v: base_style.margin_v,
            position: None,
            origin: None,
            move_data: None,
            clip: None,
            inverse_clip: None,
            clip_vector: None,
            inverse_clip_vector: None,
            fade_in: 0,
            fade_out: 0,
            complex_fade: None,
            parsed_fade: false,
            wrap_style: None,
            clip_canvas: (384, 288),
            event_globals_applied: false,
            layout_res_x: 0,
            layout_res_y: 0,
            drawing_mode: 0,
            drawing_baseline_offset: 0.0,
            blur: 0.0,
            scaled_border_and_shadow: true,
            kerning: false,
        };

        // Apply override tags (skip Transform tags - they're handled separately)
        for tag in tags {
            if let OverrideTag::Transform { .. } = tag {
                continue;
            }
            Self::apply_single_tag(&mut resolved, tag);
        }

        resolved
    }

    /// Apply a single override tag to a resolved style
    pub(super) fn apply_single_tag(resolved: &mut ResolvedStyle, tag: &OverrideTag) {
        match tag {
            OverrideTag::PropertyReset(name) => match name.as_str() {
                "b" => resolved.font_weight = if resolved.base_style.bold { 700 } else { 400 },
                "i" => resolved.italic = resolved.base_style.italic,
                "u" => resolved.underline = resolved.base_style.underline,
                "s" => resolved.strike_out = resolved.base_style.strike_out,
                "fn" => resolved.font_name = resolved.base_style.font_name.clone(),
                "fe" => resolved.font_encoding = resolved.base_style.encoding,
                "fsp" => resolved.spacing = resolved.base_style.spacing,
                "fr" | "frz" => resolved.angle = resolved.base_style.angle,
                "frx" => resolved.rotation_x = 0.0,
                "fry" => resolved.rotation_y = 0.0,
                "fscx" => resolved.scale_x = resolved.base_style.scale_x,
                "fscy" => resolved.scale_y = resolved.base_style.scale_y,
                "fax" => resolved.shear_x = 0.0,
                "fay" => resolved.shear_y = 0.0,
                "bord" => {
                    resolved.outline = resolved.base_style.outline;
                    resolved.outline_x = resolved.outline;
                    resolved.outline_y = resolved.outline;
                }
                "xbord" => resolved.outline_x = resolved.base_style.outline,
                "ybord" => resolved.outline_y = resolved.base_style.outline,
                "shad" => {
                    resolved.shadow = resolved.base_style.shadow;
                    resolved.shadow_x = resolved.shadow;
                    resolved.shadow_y = resolved.shadow;
                }
                "xshad" => resolved.shadow_x = resolved.base_style.shadow,
                "yshad" => resolved.shadow_y = resolved.base_style.shadow,
                "be" | "blur" => resolved.blur = 0.0,
                _ => {}
            },
            OverrideTag::Bold(w) => resolved.font_weight = *w,
            OverrideTag::Italic(v) => resolved.italic = *v,
            OverrideTag::Underline(v) => resolved.underline = *v,
            OverrideTag::StrikeOut(v) => resolved.strike_out = *v,
            OverrideTag::FontName(name) => resolved.font_name = name.clone(),
            // libass `ass_parse.c` (`\fs` branch): absolute sizes assign;
            // a computed size <= 0 (or non-finite) resets to the style.
            OverrideTag::FontSize(size) => {
                if size.is_finite() && *size > 0.0 {
                    resolved.font_size = *size;
                } else {
                    resolved.font_size = resolved.base_style.font_size;
                }
            }
            // Relative `\fs+N/-N`: scale the current size by (1 + d/10).
            OverrideTag::FontSizeRelative(delta) => {
                let next = resolved.font_size * (1.0 + delta / 10.0);
                if next.is_finite() && next > 0.0 {
                    resolved.font_size = next;
                } else {
                    resolved.font_size = resolved.base_style.font_size;
                }
            }
            OverrideTag::FontSizeReset => {
                resolved.font_size = resolved.base_style.font_size;
            }
            OverrideTag::FontEncoding(enc) => resolved.font_encoding = *enc,
            OverrideTag::LetterSpacing(sp) => resolved.spacing = *sp,
            OverrideTag::PrimaryColor(c) => resolved.color = *c,
            OverrideTag::SecondaryColor(c) => resolved.secondary_color = *c,
            OverrideTag::OutlineColor(c) => resolved.outline_color = *c,
            OverrideTag::ShadowColor(c) => {
                // ASS calls this the BackColour channel. It is used both
                // for shadows and for the opaque box (BorderStyle 3).
                resolved.shadow_color = *c;
                resolved.back_color = *c;
            }
            OverrideTag::Alpha(a) => {
                // \alpha applies to all four ASS colour channels.
                resolved.color = resolved.color.with_alpha(*a);
                resolved.secondary_color = resolved.secondary_color.with_alpha(*a);
                resolved.outline_color = resolved.outline_color.with_alpha(*a);
                resolved.shadow_color = resolved.shadow_color.with_alpha(*a);
                resolved.back_color = resolved.back_color.with_alpha(*a);
            }
            OverrideTag::PrimaryAlpha(a) => {
                resolved.color = resolved.color.with_alpha(*a);
            }
            OverrideTag::SecondaryAlpha(a) => {
                resolved.secondary_color = resolved.secondary_color.with_alpha(*a);
            }
            OverrideTag::OutlineAlpha(a) => {
                resolved.outline_color = resolved.outline_color.with_alpha(*a);
            }
            OverrideTag::ShadowAlpha(a) => {
                resolved.shadow_color = resolved.shadow_color.with_alpha(*a);
                resolved.back_color = resolved.back_color.with_alpha(*a);
            }
            // \pos and \move share one first-wins slot (libass
            // `EVENT_POSITIONED`): whichever comes first wins, and later
            // \pos / \move tags are ignored, so both are never set.
            OverrideTag::Position(x, y) => {
                if resolved.position.is_none() && resolved.move_data.is_none() {
                    resolved.position = Some((*x, *y));
                }
            }
            OverrideTag::Move(x1, y1, x2, y2) => {
                if resolved.position.is_none() && resolved.move_data.is_none() {
                    resolved.move_data = Some(MoveData {
                        x1: *x1,
                        y1: *y1,
                        x2: *x2,
                        y2: *y2,
                        t1: 0,
                        t2: 0,
                    });
                }
            }
            OverrideTag::MoveWithTiming(x1, y1, x2, y2, t1, t2) => {
                if resolved.position.is_none() && resolved.move_data.is_none() {
                    resolved.move_data = Some(MoveData {
                        x1: *x1,
                        y1: *y1,
                        x2: *x2,
                        y2: *y2,
                        t1: *t1,
                        t2: *t2,
                    });
                }
            }
            // First \org wins (libass `have_origin`).
            OverrideTag::Origin(x, y) => {
                if resolved.origin.is_none() {
                    resolved.origin = Some((*x, *y));
                }
            }
            // First alignment tag wins (libass `PARSED_A`, shared by
            // \an and legacy \a). Out-of-range values fall back to
            // the style alignment with the slot still consumed.
            OverrideTag::Alignment(a) => {
                if !resolved.parsed_alignment {
                    resolved.parsed_alignment = true;
                    resolved.alignment = if (1..=9).contains(a) {
                        *a
                    } else {
                        resolved.base_style.alignment
                    };
                }
            }
            OverrideTag::AlignmentReset => {
                if !resolved.parsed_alignment {
                    resolved.parsed_alignment = true;
                    resolved.alignment = resolved.base_style.alignment;
                }
            }
            OverrideTag::ScaleX(s) => resolved.scale_x = *s,
            OverrideTag::ScaleY(s) => resolved.scale_y = *s,
            OverrideTag::ScaleReset => {
                resolved.scale_x = resolved.base_style.scale_x;
                resolved.scale_y = resolved.base_style.scale_y;
            }
            OverrideTag::RotationZ(r) => resolved.angle = *r,
            OverrideTag::RotationX(r) => resolved.rotation_x = *r,
            OverrideTag::RotationY(r) => resolved.rotation_y = *r,
            OverrideTag::Border(b) => {
                resolved.outline = *b;
                resolved.outline_x = *b;
                resolved.outline_y = *b;
            }
            OverrideTag::BorderX(b) => {
                resolved.outline_x = *b;
                resolved.outline = (resolved.outline_x + resolved.outline_y) / 2.0;
            }
            OverrideTag::BorderY(b) => {
                resolved.outline_y = *b;
                resolved.outline = (resolved.outline_x + resolved.outline_y) / 2.0;
            }
            OverrideTag::Shadow(s) => {
                resolved.shadow = *s;
                resolved.shadow_x = *s;
                resolved.shadow_y = *s;
            }
            OverrideTag::ShadowX(s) => {
                resolved.shadow_x = *s;
                resolved.shadow = (resolved.shadow_x + resolved.shadow_y) / 2.0;
            }
            OverrideTag::ShadowY(s) => {
                resolved.shadow_y = *s;
                resolved.shadow = (resolved.shadow_x + resolved.shadow_y) / 2.0;
            }
            OverrideTag::ShearX(s) => resolved.shear_x = *s,
            OverrideTag::ShearY(s) => resolved.shear_y = *s,
            // \fad and \fade share one first-wins slot (libass
            // `PARSED_FADE`): the first fade tag wins in either order
            // and later ones are ignored, so both forms never coexist.
            OverrideTag::Fade(fi, fo) => {
                if !resolved.parsed_fade {
                    resolved.parsed_fade = true;
                    resolved.fade_in = *fi;
                    resolved.fade_out = *fo;
                }
            }
            OverrideTag::ComplexFade(a1, a2, a3, t1, t2, t3, t4) => {
                if !resolved.parsed_fade {
                    resolved.parsed_fade = true;
                    // Stored full-range like libass: alpha interpolates
                    // in f64 and truncates in `interpolate_alpha`;
                    // out-of-range results clamp at application.
                    resolved.complex_fade = Some(ComplexFade {
                        a1: *a1,
                        a2: *a2,
                        a3: *a3,
                        t1: *t1,
                        t2: *t2,
                        t3: *t3,
                        t4: *t4,
                    });
                }
            }
            // libass keeps rectangular and vector clipping as separate
            // state: a later rect replaces the earlier rect coordinates
            // and `\clip` vs `\iclip` flips the rect mode, while the
            // first vector clip is retained (either form consumes the
            // vector slot) and both rect and vector clips render.
            OverrideTag::Clip(x1, y1, x2, y2) => {
                resolved.clip = Some((*x1, *y1, *x2, *y2));
                resolved.inverse_clip = None;
            }
            OverrideTag::InverseClip(x1, y1, x2, y2) => {
                resolved.inverse_clip = Some((*x1, *y1, *x2, *y2));
                resolved.clip = None;
            }
            OverrideTag::ClipVector { scale, drawing } => {
                if resolved.clip_vector.is_none() && resolved.inverse_clip_vector.is_none() {
                    resolved.clip_vector = Some(VectorClip {
                        scale: *scale,
                        drawing: drawing.clone(),
                    });
                }
            }
            OverrideTag::InverseClipVector { scale, drawing } => {
                if resolved.clip_vector.is_none() && resolved.inverse_clip_vector.is_none() {
                    resolved.inverse_clip_vector = Some(VectorClip {
                        scale: *scale,
                        drawing: drawing.clone(),
                    });
                }
            }
            OverrideTag::Blur(b) => resolved.blur = *b,
            OverrideTag::EdgeBlur(b) => resolved.blur = *b,
            OverrideTag::Drawing(mode) => resolved.drawing_mode = *mode,
            OverrideTag::DrawingBaseline(pbo) => resolved.drawing_baseline_offset = *pbo,
            OverrideTag::WrapStyle(q) => resolved.wrap_style = Some(*q),
            _ => {}
        }
    }

    /// Resolve an event's style with all override tags applied.
    ///
    /// Segment style tags from the initial override groups establish the
    /// defaults, but event-layout tags (\pos, \move, \org, \clip, \iclip,
    /// \fad, \fade, \an, \q) apply no matter where they appear textually:
    /// they are scanned across all segments in textual order, so
    /// `{\pos(100,100)}Hi` and `Hi{\pos(100,100)}` resolve identically,
    /// as do `{\an7}Hi` and `Hi{\an7}`. Repeated tags resolve first-wins
    /// per libass, except rect clips (later coordinates replace earlier
    /// ones) and `\q` (last wins, consumed separately).
    pub fn resolve_style(base_style: &Style, event: &Event) -> ResolvedStyle {
        let segments = parse_text_segments(&event.text);
        let initial_tags = segments
            .first()
            .map(|segment| segment.tags.as_slice())
            .unwrap_or(&[]);
        let mut resolved = Self::resolve_base_style(base_style, initial_tags);

        // Event-layout tags apply regardless of textual placement.
        // Segments carry accumulated tags, so only newly added tags per
        // segment are considered; each first-wins slot keeps the
        // textually first tag, including re-scanned initial tags.
        // (\q has no ResolvedStyle field and is consumed separately from
        // the event tag list; scanning it here is a harmless no-op.)
        let mut prev_tag_count = 0usize;
        for segment in &segments {
            let from = prev_tag_count.min(segment.tags.len());
            prev_tag_count = segment.tags.len();
            for tag in &segment.tags[from..] {
                if tag.is_event_layout() {
                    Self::apply_single_tag(&mut resolved, tag);
                }
            }
        }

        // Apply event-level margin overrides
        if event.margin_l != 0 {
            resolved.margin_l = event.margin_l;
        }
        if event.margin_r != 0 {
            resolved.margin_r = event.margin_r;
        }
        if event.margin_v != 0 {
            resolved.margin_v = event.margin_v;
        }

        resolved
    }
}
