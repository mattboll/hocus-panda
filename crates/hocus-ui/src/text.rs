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
    theme::{Face, Faces, TextStyle},
};

/// The bundled faces (SIL Open Font License, fonts/licenses/): what is
/// drawn never depends on the fonts of the system.
const FONTS: [&[u8]; 4] = [
    include_bytes!("../fonts/Manrope.ttf"),
    include_bytes!("../fonts/Fraunces.ttf"),
    include_bytes!("../fonts/JetBrainsMono.ttf"),
    include_bytes!("../fonts/SpaceGrotesk.ttf"),
];

/// Fonts and the scratch space of the layout engine, shared by a tree; the
/// theme says which face plays which role.
pub struct Fonts {
    fonts: FontContext,
    layouts: LayoutContext<()>,
    faces: Faces,
}

impl fmt::Debug for Fonts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Fonts")
    }
}

impl Fonts {
    pub fn new(faces: Faces) -> Self {
        let mut fonts = FontContext::new();
        for data in FONTS {
            fonts
                .collection
                .register_fonts(Blob::new(Arc::new(data)), None);
        }
        Self {
            fonts,
            layouts: LayoutContext::new(),
            faces,
        }
    }

    /// Text is laid out again in the new faces when it is next measured.
    pub fn set_faces(&mut self, faces: Faces) {
        self.faces = faces;
    }
}

/// A piece of text with its style, and its layout once computed.
pub struct TextBox {
    text: String,
    style: TextStyle,
    layout: Option<Layout<()>>,
    /// The scale and face the layout was built with.
    scale: f32,
    face: Option<Face>,
    /// The width lines were broken at, in logical pixels.
    broken_at: Option<Option<f32>>,
    /// One line, cut with an ellipsis when too long (window titles).
    single_line: bool,
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
            face: None,
            broken_at: None,
            single_line: false,
        }
    }

    /// Keeps the text on one line, cut with "…" when it does not fit.
    pub fn single_line(mut self) -> Self {
        self.single_line = true;
        self
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
        let face = fonts.faces.get(self.style.typeface);
        let rebuild = self.layout.is_none() || self.scale != scale || self.face != Some(face);
        if rebuild || (self.single_line && self.broken_at != Some(max_width)) {
            let mut layout = self.build(fonts, &self.text, scale);
            if self.single_line
                && let Some(max) = max_width
            {
                layout.break_all_lines(None);
                if layout.width() > max * scale {
                    layout = self.cut(fonts, max * scale, scale);
                }
            }
            self.layout = Some(layout);
            self.scale = scale;
            self.face = Some(face);
            self.broken_at = None;
        }
        let layout = self.layout.get_or_insert_with(Layout::new);
        if self.broken_at != Some(max_width) {
            let width = if self.single_line { None } else { max_width };
            layout.break_all_lines(width.map(|w| w * scale));
            layout.align(Alignment::Start, AlignmentOptions::default());
            self.broken_at = Some(max_width);
        }
        layout
    }

    fn build(&self, fonts: &mut Fonts, text: &str, scale: f32) -> Layout<()> {
        let face = fonts.faces.get(self.style.typeface);
        let mut builder = fonts
            .layouts
            .ranged_builder(&mut fonts.fonts, text, scale, true);
        builder.push_default(StyleProperty::FontFamily(FontFamily::Source(
            Cow::Borrowed(face.family()),
        )));
        builder.push_default(StyleProperty::FontSize(self.style.size));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(f32::from(
            self.style.weight,
        ))));
        builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(
            self.style.line_height,
        )));
        builder.build(text)
    }

    /// The longest start of the text that fits in `max` physical pixels with
    /// an ellipsis after it.
    fn cut(&self, fonts: &mut Fonts, max: f32, scale: f32) -> Layout<()> {
        let ends: Vec<usize> = self.text.char_indices().map(|(i, _)| i).collect();
        // `k` characters end at `ends[k]`; the whole text is known too wide.
        let (mut lo, mut hi) = (0, ends.len().saturating_sub(1));
        let mut best = self.build(fonts, "…", scale);
        best.break_all_lines(None);
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            let candidate = format!("{}…", self.text[..ends[mid]].trim_end());
            let mut layout = self.build(fonts, &candidate, scale);
            layout.break_all_lines(None);
            if layout.width() <= max {
                best = layout;
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        best
    }

    /// The layout as last computed, for drawing.
    pub fn laid_out(&self) -> Option<&Layout<()>> {
        self.layout.as_ref()
    }
}
