//! Text: shaped and laid out by parley, at the physical size so that glyphs
//! land on whole pixels. A text box keeps its layout until its content,
//! style or scale changes, and only breaks lines again when its width does.

use std::{borrow::Cow, cell::RefCell, fmt, sync::Arc};

use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontWeight, Layout, LayoutContext,
    LineHeight, StyleProperty,
    fontique::{
        Blob, Collection, CollectionOptions, FallbackKey, GenericFamily, Script, ScriptExt,
        SourceCache,
    },
};

use crate::{
    geom::Size,
    theme::{Face, Faces, TextStyle},
};

/// The bundled faces (fonts/licenses/): until the system's fonts are
/// given, and always in tests, what is drawn depends on nothing else.
const FONTS: [&[u8]; 5] = [
    include_bytes!("../fonts/Manrope.ttf"),
    include_bytes!("../fonts/Fraunces.ttf"),
    include_bytes!("../fonts/JetBrainsMono.ttf"),
    include_bytes!("../fonts/SpaceGrotesk.ttf"),
    include_bytes!("../fonts/DejaVuSans.ttf"),
];

/// After the theme's face: the bundled symbols (arrows, `ᵉ`, Greek,
/// Cyrillic…), then colour emoji when the system has them. Other scripts
/// fall back on the system's fonts by script.
const FALLBACK: &str = "DejaVu Sans, emoji";

thread_local! {
    /// The system's fonts once given to this thread (their families and
    /// their files, mapped), and how many times they were.
    static SYSTEM: RefCell<(u64, Option<(Collection, SourceCache)>)> =
        const { RefCell::new((0, None)) };
}

fn bundled() -> Collection {
    let mut collection = Collection::new(CollectionOptions {
        shared: false,
        system_fonts: false,
    });
    for data in FONTS {
        collection.register_fonts(Blob::new(Arc::new(data)), None);
    }
    collection
}

/// The bundled faces, which are in memory: no file to map.
fn only_bundled() -> (Collection, SourceCache) {
    (bundled(), SourceCache::default())
}

/// The system's fonts behind the bundled ones, with the fallback of every
/// script already looked up and their files mapped: reading them is slow,
/// so it is done once on a worker thread, then given to the thread that
/// draws, which then opens no font file (#285).
pub struct SystemFonts {
    collection: Collection,
    /// The files of every family a layout may fall back on, mapped:
    /// fontique opens a file it does not find in a layout's cache.
    sources: SourceCache,
    /// The ids of those files' blobs.
    mapped: Vec<u64>,
}

impl fmt::Debug for SystemFonts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SystemFonts")
    }
}

impl SystemFonts {
    /// Blocks: reads the fonts the system has.
    pub fn load() -> Self {
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: true,
        });
        for data in FONTS {
            collection.register_fonts(Blob::new(Arc::new(data)), None);
        }
        // Everything a layout may ask the system later, asked now. Text
        // carries no locale, so each script's fallback is all there is to
        // ask; a script fontique has no sample of falls back on
        // fontconfig's default family, asked for an unknown one.
        let mut families = Vec::new();
        for (script, _) in Script::all_samples() {
            families.extend(collection.fallback_families(FallbackKey::new(*script, None)));
        }
        families.extend(collection.fallback_families(FallbackKey::new(Script::UNKNOWN, None)));
        families.extend(collection.generic_families(GenericFamily::Emoji));
        families.sort_unstable();
        families.dedup();
        // A family's files are opened when a layout first uses one of its
        // faces: mapped here, each once.
        let mut sources = SourceCache::default();
        let mut mapped = Vec::new();
        for id in families {
            let Some(family) = collection.family(id) else {
                continue;
            };
            for font in family.fonts() {
                if let Some(blob) = font.load(Some(&mut sources)) {
                    mapped.push(blob.id());
                }
            }
        }
        mapped.sort_unstable();
        mapped.dedup();
        Self {
            collection,
            sources,
            mapped,
        }
    }

    /// How many families there are, the bundled ones included.
    pub fn families(&mut self) -> usize {
        self.collection.family_names().count()
    }

    /// How many font files are mapped.
    pub fn files(&self) -> usize {
        self.mapped.len()
    }

    /// From now on, text on this thread falls back on these fonts; trees
    /// lay their text out again on their next update.
    pub fn install(self) {
        SYSTEM.with(|s| {
            let mut s = s.borrow_mut();
            s.0 += 1;
            s.1 = Some((self.collection, self.sources));
        });
    }
}

/// Whether text on this thread may use the system's fonts.
pub fn system_fonts_installed() -> bool {
    SYSTEM.with(|s| s.borrow().1.is_some())
}

/// Fonts and the scratch space of the layout engine, shared by a tree; the
/// theme says which face plays which role.
pub struct Fonts {
    fonts: FontContext,
    layouts: LayoutContext<()>,
    faces: Faces,
    /// Which system fonts these are, 0 for the bundled ones only.
    generation: u64,
}

impl fmt::Debug for Fonts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Fonts")
    }
}

impl Fonts {
    pub fn new(faces: Faces) -> Self {
        let (generation, (collection, source_cache)) = SYSTEM.with(|s| {
            let s = s.borrow();
            (s.0, s.1.clone().unwrap_or_else(only_bundled))
        });
        Self {
            fonts: FontContext {
                collection,
                source_cache,
            },
            layouts: LayoutContext::new(),
            faces,
            generation,
        }
    }

    /// Takes the system's fonts given since: whether text must be laid out
    /// again.
    pub fn sync(&mut self) -> bool {
        let newer = SYSTEM.with(|s| {
            let s = s.borrow();
            (s.0 != self.generation).then(|| (s.0, s.1.clone()))
        });
        let Some((generation, system)) = newer else {
            return false;
        };
        let (collection, source_cache) = system.unwrap_or_else(only_bundled);
        self.fonts = FontContext {
            collection,
            source_cache,
        };
        self.generation = generation;
        true
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
    /// The scale, face and fonts the layout was built with.
    scale: f32,
    face: Option<Face>,
    generation: u64,
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
            generation: 0,
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
        let rebuild = self.layout.is_none()
            || self.scale != scale
            || self.face != Some(face)
            || self.generation != fonts.generation;
        if rebuild || (self.single_line && self.broken_at != Some(max_width)) {
            let mut layout = self.build(fonts, &self.text, scale);
            if self.single_line
                && let Some(max) = max_width
            {
                layout.break_all_lines(None);
                // Widths that went through layout arithmetic differ in their
                // last bits: half a pixel is not too wide.
                if layout.width() > max * scale + 0.5 {
                    layout = self.cut(fonts, max * scale, scale);
                }
            }
            self.layout = Some(layout);
            self.scale = scale;
            self.face = Some(face);
            self.generation = fonts.generation;
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
        builder.push_default(StyleProperty::FontFamily(FontFamily::Source(Cow::Owned(
            format!("{}, {FALLBACK}", face.family()),
        ))));
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

#[cfg(test)]
mod tests {
    use parley::PositionedLayoutItem;

    use super::*;
    use crate::theme::Theme;

    /// Glyphs the fonts had not: drawn as empty boxes.
    fn missing(text: &str) -> usize {
        let theme = Theme::default();
        let mut fonts = Fonts::new(theme.faces);
        let mut text = TextBox::new(text, theme.text.body);
        let layout = text.layout(&mut fonts, None, 1.0);
        layout
            .lines()
            .flat_map(|line| line.items().collect::<Vec<_>>())
            .map(|item| match item {
                PositionedLayoutItem::GlyphRun(run) => run.glyphs().filter(|g| g.id == 0).count(),
                PositionedLayoutItem::InlineBox(_) => 0,
            })
            .sum()
    }

    /// The font of each run of `text`.
    fn run_fonts(text: &str) -> Vec<parley::FontData> {
        let theme = Theme::default();
        let mut fonts = Fonts::new(theme.faces);
        let mut text = TextBox::new(text, theme.text.body);
        let layout = text.layout(&mut fonts, None, 1.0);
        layout
            .lines()
            .flat_map(|line| line.items().collect::<Vec<_>>())
            .filter_map(|item| match item {
                PositionedLayoutItem::GlyphRun(run) => Some(run.run().font().clone()),
                PositionedLayoutItem::InlineBox(_) => None,
            })
            .collect()
    }

    #[test]
    fn symbols_come_from_the_bundled_fonts() {
        assert_eq!(missing("1ᵉʳ → ← ✓ ★ ≤ ∞ Ω Ж"), 0);
    }

    #[test]
    fn other_scripts_and_emoji_come_from_the_system() {
        fn send<T: Send>(_: &T) {}
        let started = std::time::Instant::now();
        let mut system = SystemFonts::load();
        eprintln!(
            "{} families, {} files mapped, in {:?}",
            system.families(),
            system.files(),
            started.elapsed()
        );
        send(&system);
        let mapped = system.mapped.clone();
        let text = "Réunion 🐼 こんにちは";
        assert!(missing(text) > 0, "the bundled fonts alone lack them");
        let bundled = Fonts::new(Theme::default().faces);
        // Without fontconfig, the bundled fonts only, and nothing breaks.
        let has_system_fonts = system.families() > FONTS.len();
        system.install();
        let mut fonts = bundled;
        assert!(fonts.sync(), "trees made before take them");
        // A fallback family is not a font that has the characters: ask
        // fontconfig for one that does.
        let covered = |pattern: &str| {
            std::process::Command::new("fc-list")
                .arg(pattern)
                .output()
                .is_ok_and(|o| o.status.success() && !o.stdout.is_empty())
        };
        let japanese = covered(":charset=3053");
        let emoji = covered(":charset=1f43c");
        if !(has_system_fonts && japanese && emoji) {
            eprintln!("skipped: this system has no Japanese or emoji font");
            return;
        }
        assert_eq!(missing(text), 0);
        // Each run's file was mapped on the worker, or is a bundled face
        // in memory: laying the text out opened no file (#285).
        for font in run_fonts(text) {
            let bundled = FONTS.contains(&font.data.data());
            assert!(
                bundled || mapped.binary_search(&font.data.id()).is_ok(),
                "a font file opened by the layout"
            );
        }
    }
}
