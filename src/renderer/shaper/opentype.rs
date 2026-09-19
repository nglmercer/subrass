use ab_glyph::{Font, GlyphId};
use harfrust::{
    BufferClusterLevel, Direction, Feature, FontRef, ShapeOptions, ShaperData, Tag, UnicodeBuffer,
};
use unicode_bidi::{BidiInfo, Level};

use super::{cluster_font_picks, ShapedGlyph, ShapingFont, TextShaper};
use crate::types::Color;

/// Shape one logical line into visual-order OpenType runs. This helper is
/// deliberately bounded by the input line length and the caller's document
/// caps; harfrust itself performs the GSUB/GPOS work.
#[allow(clippy::too_many_arguments)]
pub(super) fn shape_opentype_line(
    text: &str,
    fonts: &[ShapingFont<'_>],
    font_size: f64,
    scale_x: f64,
    scale_y: f64,
    font_weight: u16,
    italic: bool,
    spacing: f64,
    color: Color,
    outline_color: Color,
    shadow_color: Color,
    rotation: f64,
    kerning: bool,
    base_level: Option<Level>,
) -> (Vec<ShapedGlyph>, f64, u32) {
    if text.is_empty() {
        return (Vec::new(), 0.0, 0);
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let picks = cluster_font_picks(text, fonts.len(), |face, ch| {
        fonts
            .get(face)
            .is_some_and(|font| font.raster.glyph_id(ch).0 != 0)
    });

    let bidi = BidiInfo::new(text, base_level);
    let para = bidi.paragraphs.first();
    let levels = para
        .map(|p| bidi.reordered_levels_per_char(p, p.range.clone()))
        .unwrap_or_else(|| vec![unicode_bidi::LTR_LEVEL; chars.len()]);
    let levels = if levels.len() == chars.len() {
        levels
    } else {
        vec![unicode_bidi::LTR_LEVEL; chars.len()]
    };
    let visual_indices = BidiInfo::reorder_visual(&levels);

    let mut runs: Vec<(usize, usize, usize, bool)> = Vec::new();
    let mut start = 0usize;
    while start < chars.len() {
        let pick = picks.get(start).copied().unwrap_or(0);
        let rtl = levels.get(start).is_some_and(|l| l.is_rtl());
        let mut end = start + 1;
        while end < chars.len()
            && picks.get(end).copied().unwrap_or(0) == pick
            && levels.get(end).is_some_and(|l| l.is_rtl()) == rtl
        {
            end += 1;
        }
        runs.push((start, end, pick, rtl));
        start = end;
    }

    let mut visual_run_ids: Vec<usize> = (0..runs.len()).collect();
    visual_run_ids.sort_by_key(|run_id| {
        let (from, to, _, _) = runs[*run_id];
        visual_indices
            .iter()
            .position(|idx| *idx >= from && *idx < to)
            .unwrap_or(usize::MAX)
    });

    let mut output = Vec::new();
    let mut pen_x = 0.0;
    let mut missing = 0u32;
    for run_id in visual_run_ids {
        let (from, to, pick, rtl) = runs[run_id];
        let byte_start = chars[from].0;
        let byte_end = if to < chars.len() {
            chars[to].0
        } else {
            text.len()
        };
        let run_text = &text[byte_start..byte_end];
        let Some(font) = fonts.get(pick.min(fonts.len().saturating_sub(1))) else {
            continue;
        };
        let Ok(font_ref) = FontRef::from_index(font.data, font.face_index) else {
            continue;
        };
        let shaper_data = ShaperData::new(&font_ref);
        let shaper = shaper_data.shaper(&font_ref).build();
        let font_scale = font_size / f64::from(font.ft_height.max(1.0));
        let mut buffer = UnicodeBuffer::new();
        buffer.set_direction(if rtl {
            Direction::RightToLeft
        } else {
            Direction::LeftToRight
        });
        buffer.set_cluster_level(BufferClusterLevel::MonotoneCharacters);
        buffer.push_str(run_text);
        buffer.guess_segment_properties();
        let mut features = Vec::with_capacity(3);
        features.push(Feature::new(Tag::new(b"kern"), u32::from(kerning), ..));
        if spacing != 0.0 {
            features.push(Feature::new(Tag::new(b"liga"), 0, ..));
            features.push(Feature::new(Tag::new(b"clig"), 0, ..));
        }
        let shaped = shaper.shape(buffer, ShapeOptions::new().features(&features));
        let infos = shaped.glyph_infos();
        let positions = shaped.glyph_positions();
        if infos.is_empty() {
            continue;
        }
        let advances: Vec<f64> = positions
            .iter()
            .map(|position| f64::from(position.x_advance) * font_scale * scale_x)
            .collect();
        let gaps: Vec<f64> = (0..infos.len())
            .map(|i| {
                if i + 1 < infos.len() && infos[i].cluster != infos[i + 1].cluster {
                    spacing * scale_x
                } else {
                    0.0
                }
            })
            .collect();
        let run_width: f64 = advances
            .iter()
            .zip(&gaps)
            .map(|(advance, gap)| advance + gap)
            .sum();
        let mut cursor = 0.0;
        for (index, (info, position)) in infos.iter().zip(positions).enumerate() {
            let advance = advances[index];
            let gap = gaps[index];
            let local_x = cursor;
            cursor += advance + gap;
            let cluster_byte = usize::try_from(info.cluster).unwrap_or(0);
            let ch = run_text
                .get(cluster_byte..)
                .and_then(|s| s.chars().next())
                .unwrap_or('�');
            let glyph_id = GlyphId(info.glyph_id.min(u32::from(u16::MAX)) as u16);
            if glyph_id.0 == 0 {
                missing = missing.saturating_add(1);
            }
            output.push(ShapedGlyph {
                glyph_id,
                font_id: font.id,
                ch,
                x: pen_x + local_x + f64::from(position.x_offset) * font_scale * scale_x,
                y: -f64::from(position.y_offset) * font_scale * scale_y,
                advance,
                font_size,
                color,
                outline_color,
                shadow_color,
                font_weight,
                italic,
                scale_x,
                scale_y,
                rotation,
            });
        }
        pen_x += run_width;
    }
    (output, pen_x, missing)
}

pub(super) fn shape_measure_opentype(
    text: &str,
    fonts: &[ShapingFont<'_>],
    font_size: f64,
    scale_x: f64,
    spacing: f64,
    kerning: bool,
) -> f64 {
    let shaped = TextShaper::shape_with_opentype(
        text,
        fonts,
        font_size,
        scale_x,
        1.0,
        400,
        false,
        spacing,
        Color::white(),
        Color::black(),
        Color::black(),
        0.0,
        kerning,
        None,
    );
    shaped.width
}
