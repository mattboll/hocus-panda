//! Text through parley, shared by the two vello candidates.

use std::{borrow::Cow, collections::HashMap, sync::Arc};

use parley::{
    FontContext, FontFamily, FontWeight, Layout, LayoutContext, PositionedLayoutItem,
    StyleProperty, fontique::Blob,
};
use ui_render_prototype::{Family, Fonts};

/// One glyph run, ready to draw at physical positions.
pub struct Run<'a> {
    pub font: &'a parley::FontData,
    pub size: f32,
    /// Converted: parley and vello disagree on the type.
    pub coords: Vec<i16>,
    pub glyphs: Vec<(u32, f32, f32)>,
}

pub struct Texts {
    fonts: FontContext,
    layouts: LayoutContext<()>,
    /// Laid out text by content and style: only changed text is laid out again.
    cache: HashMap<(String, u32, u16, Family, u32), Layout<()>>,
    scale: f32,
}

impl Texts {
    pub fn new(scale: f32) -> Self {
        let data = Fonts::load();
        let mut fonts = FontContext::new();
        for bytes in [data.interface, data.display] {
            fonts
                .collection
                .register_fonts(Blob::new(Arc::new(bytes)), None);
        }
        Self {
            fonts,
            layouts: LayoutContext::new(),
            cache: HashMap::new(),
            scale,
        }
    }

    /// The glyph runs of `text`, laid out at the physical size, offset by
    /// `(px, py)` physical pixels.
    #[allow(clippy::too_many_arguments)]
    pub fn runs(
        &mut self,
        text: &str,
        size: f32,
        weight: u16,
        family: Family,
        width: f32,
        px: f32,
        py: f32,
        mut draw: impl FnMut(Run<'_>),
    ) {
        let key = (
            text.to_owned(),
            size.to_bits(),
            weight,
            family,
            width.to_bits(),
        );
        let scale = self.scale;
        let (fonts, layouts) = (&mut self.fonts, &mut self.layouts);
        let layout = self.cache.entry(key).or_insert_with(|| {
            let mut builder = layouts.ranged_builder(fonts, text, scale, true);
            let name = match family {
                Family::Interface => "Manrope",
                Family::Display => "Fraunces",
            };
            builder.push_default(StyleProperty::FontFamily(FontFamily::Source(
                Cow::Borrowed(name),
            )));
            builder.push_default(StyleProperty::FontSize(size));
            builder.push_default(StyleProperty::FontWeight(FontWeight::new(f32::from(
                weight,
            ))));
            builder.push_default(StyleProperty::LineHeight(
                parley::LineHeight::FontSizeRelative(1.35),
            ));
            let mut layout = builder.build(text);
            layout.break_all_lines(Some(width * scale));
            layout.align(
                parley::Alignment::Start,
                parley::AlignmentOptions::default(),
            );
            layout
        });
        for line in layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                    continue;
                };
                let run = glyph_run.run();
                draw(Run {
                    font: run.font(),
                    size: run.font_size(),
                    coords: run
                        .normalized_coords()
                        .iter()
                        .map(|c| c.to_bits())
                        .collect(),
                    glyphs: glyph_run
                        .positioned_glyphs()
                        .map(|g| (g.id, px + g.x, py + g.y))
                        .collect(),
                });
            }
        }
    }
}
