//! Text: shaped and laid out by parley, at the physical size so that glyphs
//! land on whole pixels. A text box keeps its layout until its content,
//! style or scale changes, and only breaks lines again when its width does.

use std::{borrow::Cow, fmt, sync::Arc};

use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontWeight, Layout, LayoutContext,
    LineHeight, StyleProperty, fontique::Blob,
};

use crate::{
    geom::Size,
    theme::{TextStyle, Typeface},
};

/// The interface face, bundled (SIL Open Font License, see fonts/OFL.txt).
const MANROPE: &[u8] = include_bytes!("../fonts/Manrope.ttf");

/// Fonts and the scratch space of the layout engine, shared by a tree.
pub struct Fonts {
    fonts: FontContext,
    layouts: LayoutContext<()>,
}

impl fmt::Debug for Fonts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Fonts")
    }
}

impl Default for Fonts {
    fn default() -> Self {
        Self::new()
    }
}

impl Fonts {
    /// The bundled fonts only: what is drawn never depends on the system.
    pub fn new() -> Self {
        let mut fonts = FontContext::new();
        fonts
            .collection
            .register_fonts(Blob::new(Arc::new(MANROPE)), None);
        Self {
            fonts,
            layouts: LayoutContext::new(),
        }
    }

    fn family(typeface: Typeface) -> &'static str {
        // Fraunces and JetBrains Mono come with the themes (#22).
        match typeface {
            Typeface::Interface | Typeface::Display | Typeface::Mono => "Manrope",
        }
    }
}

/// A piece of text with its style, and its layout once computed.
pub struct TextBox {
    text: String,
    style: TextStyle,
    layout: Option<Layout<()>>,
    /// The scale the layout was built at.
    scale: f32,
    /// The width lines were broken at, in logical pixels.
    broken_at: Option<Option<f32>>,
}

impl fmt::Debug for TextBox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextBox")
            .field("text", &self.text)
            .field("style", &self.style)
            .finish_non_exhaustive()
    }
}

impl TextBox {
    pub fn new(text: impl Into<String>, style: TextStyle) -> Self {
        Self {
            text: text.into(),
            style,
            layout: None,
            scale: 0.0,
            broken_at: None,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn style(&self) -> &TextStyle {
        &self.style
    }

    /// Whether anything changed.
    pub fn set_text(&mut self, text: &str) -> bool {
        if self.text == text {
            return false;
        }
        text.clone_into(&mut self.text);
        self.layout = None;
        true
    }

    /// Whether anything changed; a colour change keeps the layout.
    pub fn set_style(&mut self, style: TextStyle) -> bool {
        if self.style == style {
            return false;
        }
        let relayout = self.style.with_color(style.color) != style;
        self.style = style;
        if relayout {
            self.layout = None;
        }
        true
    }

    /// Lays the text out within `max_width` logical pixels, and its size.
    pub fn measure(&mut self, fonts: &mut Fonts, max_width: Option<f32>, scale: f32) -> Size {
        let layout = self.layout(fonts, max_width, scale);
        Size::new(layout.width() / scale, layout.height() / scale)
    }

    /// The layout at `scale`, in physical pixels.
    pub fn layout(&mut self, fonts: &mut Fonts, max_width: Option<f32>, scale: f32) -> &Layout<()> {
        if self.layout.is_none() || self.scale != scale {
            let mut builder =
                fonts
                    .layouts
                    .ranged_builder(&mut fonts.fonts, &self.text, scale, true);
            builder.push_default(StyleProperty::FontFamily(FontFamily::Source(
                Cow::Borrowed(Fonts::family(self.style.typeface)),
            )));
            builder.push_default(StyleProperty::FontSize(self.style.size));
            builder.push_default(StyleProperty::FontWeight(FontWeight::new(f32::from(
                self.style.weight,
            ))));
            builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(
                self.style.line_height,
            )));
            self.layout = Some(builder.build(&self.text));
            self.scale = scale;
            self.broken_at = None;
        }
        let layout = self.layout.get_or_insert_with(Layout::new);
        if self.broken_at != Some(max_width) {
            layout.break_all_lines(max_width.map(|w| w * scale));
            layout.align(Alignment::Start, AlignmentOptions::default());
            self.broken_at = Some(max_width);
        }
        layout
    }

    /// The layout as last computed, for drawing.
    pub fn laid_out(&self) -> Option<&Layout<()>> {
        self.layout.as_ref()
    }
}
