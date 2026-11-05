//! The window's tokens: the bar's (bar.md §13) and the mockup's, its
//! fonts (the shell's, Manrope and Martian Mono) and the styles of its
//! pieces.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock, PoisonError},
};

use iced::{
    Background, Border, Color, Font, Shadow, Theme, Vector, font,
    widget::{button, container, svg, text_input},
};

use crate::{model::Icon, theme::Ink};

/// `#RRGGBB`.
pub const fn rgb(hex: u32) -> Color {
    Color::from_rgb8(
        (hex >> 16) as u8,
        ((hex >> 8) & 0xFF) as u8,
        (hex & 0xFF) as u8,
    )
}

pub const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba8(r, g, b, a)
}

/// `text` / `text.dim` / `text.faint` (bar.md §13), in the theme of the
/// moment ([`crate::theme`]).
pub const TEXT: Ink = Ink::Token(|t| t.text);
pub const DIM: Ink = Ink::Token(|t| t.dim);
pub const FAINT: Ink = Ink::Token(|t| t.faint);
/// The sidebar's names, and the banner's words.
pub const SOFT: Ink = Ink::Token(|t| t.text2);
/// What is chosen, on or focused.
pub const ACCENT: Ink = Ink::Accent(1.0);
pub const ORANGE: Ink = Ink::Dark(rgb(0xF2945B));
pub const PEACH: Ink = Ink::Dark(rgb(0xF7B98A));
pub const BLUE: Ink = Ink::Dark(rgb(0x8FB3FF));
/// Text in the accent (its links, its marks).
pub const MINT: Ink = Ink::AccentText;
/// Text and marks on the accent's fill.
pub const DEEP: Ink = Ink::Token(|t| t.on_accent);

pub const SIDEBAR: Ink = Ink::Token(|t| t.sidebar);
pub const CONTENT: Ink = Ink::Token(|t| t.window);
pub const LINE: Ink = Ink::Token(|t| t.divider);
pub const CARD: Ink = Ink::Token(|t| t.card);
pub const CARD_RIM: Ink = Ink::Token(|t| t.card_rim);
pub const HOVER: Ink = Ink::Dark(rgba(255, 255, 255, 0.05));
pub const CURRENT: Ink = Ink::Accent(0.14);
pub const MENU: Ink = Ink::Token(|t| t.menu);
/// The toast stays dark in both themes, and so does its text.
pub const TOAST: Ink = Ink::Token(|t| t.toast);
pub const RIM_12: Ink = Ink::Token(|t| t.control_rim);

/// The fonts the shell carries (SIL Open Font License 1.1).
pub const FONTS: [&[u8]; 4] = [
    include_bytes!("../../../hocus-shell/fonts/Manrope-Regular.ttf"),
    include_bytes!("../../../hocus-shell/fonts/Manrope-SemiBold.ttf"),
    include_bytes!("../../../hocus-shell/fonts/Manrope-Bold.ttf"),
    include_bytes!("../../../hocus-shell/fonts/MartianMono-Regular.ttf"),
];

pub const MANROPE: Font = Font::with_name("Manrope");
pub const SEMIBOLD: Font = Font {
    weight: font::Weight::Semibold,
    ..MANROPE
};
pub const BOLD: Font = Font {
    weight: font::Weight::Bold,
    ..MANROPE
};
pub const MONO: Font = Font::with_name("Martian Mono");

/// A control that does not apply now is drawn at 45 %.
pub const INERT: f32 = 0.45;

pub fn fade(color: Color, by: f32) -> Color {
    Color {
        a: color.a * by,
        ..color
    }
}

pub fn border(radius: f32, width: f32, color: Color) -> Border {
    Border {
        radius: radius.into(),
        width,
        color,
    }
}

/// The popovers' shadow.
pub fn shadow() -> Shadow {
    Shadow {
        color: crate::theme::ink(rgba(0, 0, 0, 0.45)),
        offset: Vector::new(0.0, 12.0),
        blur_radius: 36.0,
    }
}

/// A box: its fill, its rim.
pub fn boxed(
    fill: Color,
    radius: f32,
    rim: Option<(f32, Color)>,
) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(fill)),
        border: match rim {
            Some((width, color)) => border(radius, width, color),
            None => border(radius, 0.0, Color::TRANSPARENT),
        },
        ..container::Style::default()
    }
}

/// The keyboard's ring (as in the bar): 3 px of the accent.
pub fn ring(radius: f32) -> Border {
    border(radius, 3.0, ACCENT.get())
}

/// A flat button: transparent, `hover` under the pointer, `fill` always
/// (the current row), its ring while it holds the keyboard.
pub fn flat(
    fill: Option<Color>,
    hover: Color,
    radius: f32,
    focused: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let background = match (fill, status) {
            (Some(fill), _) => Some(Background::Color(fill)),
            (None, button::Status::Hovered | button::Status::Pressed) => {
                Some(Background::Color(hover))
            }
            _ => None,
        };
        button::Style {
            background,
            text_color: TEXT.get(),
            border: if focused {
                ring(radius)
            } else {
                border(radius, 0.0, Color::TRANSPARENT)
            },
            ..button::Style::default()
        }
    }
}

/// The search field: its rim green while it holds the keyboard.
pub fn search(_: &Theme, status: text_input::Status) -> text_input::Style {
    let focused = matches!(status, text_input::Status::Focused { .. });
    text_input::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: border(9.0, 0.0, Color::TRANSPARENT),
        icon: DIM.get(),
        placeholder: FAINT.get(),
        value: TEXT.get(),
        selection: Ink::Accent(if focused { 0.35 } else { 0.2 }).get(),
    }
}

/// An icon in `color`, its strokes `width` wide.
pub fn icon(icon: Icon, color: Color, width: f32) -> svg::Handle {
    type Drawn = HashMap<(Icon, [u8; 4], u32), svg::Handle>;
    static CACHE: OnceLock<Mutex<Drawn>> = OnceLock::new();
    let rgba = color.into_rgba8();
    let key = (icon, rgba, width.to_bits());
    let mut cache = CACHE
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    cache
        .entry(key)
        .or_insert_with(|| {
            let [r, g, b, a] = rgba;
            svg::Handle::from_memory(
                format!(
                    "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" width=\"24\" \
                     height=\"24\"><path d=\"{}\" fill=\"none\" stroke=\"rgb({r},{g},{b})\" \
                     stroke-opacity=\"{}\" stroke-width=\"{width}\" stroke-linecap=\"round\" \
                     stroke-linejoin=\"round\"/></svg>",
                    icon.path(),
                    f32::from(a) / 255.0
                )
                .into_bytes(),
            )
        })
        .clone()
}

/// The bundled fonts, in the font system that measures and draws text,
/// before anything is measured (as the shell does: iced loads them only
/// with its first renderer).
pub fn load_fonts() {
    match iced::advanced::graphics::text::font_system().write() {
        Ok(mut system) => {
            for font in FONTS {
                system.load_font(std::borrow::Cow::Borrowed(font));
            }
        }
        Err(err) => tracing::warn!(%err, "the fonts are not loaded before the first draw"),
    }
}

/// `text`'s width in `font` at `size`, measured once.
pub fn measure(text: &str, size: f32, font: Font) -> f32 {
    use iced::advanced::graphics::text::cosmic_text;
    type Widths = HashMap<(String, u32, Font), f32>;
    static MEASURED: OnceLock<Mutex<Widths>> = OnceLock::new();
    let key = (text.to_owned(), size.to_bits(), font);
    let mut measured = MEASURED
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if let Some(width) = measured.get(&key) {
        return *width;
    }
    let width = match iced::advanced::graphics::text::font_system().write() {
        Ok(mut system) => {
            let system = system.raw();
            let mut buffer =
                cosmic_text::Buffer::new(system, cosmic_text::Metrics::new(size, size * 1.3));
            let attrs = iced::advanced::graphics::text::to_attributes(font);
            buffer.set_text(system, text, &attrs, cosmic_text::Shaping::Advanced, None);
            buffer.shape_until_scroll(system, false);
            buffer
                .layout_runs()
                .map(|run| run.line_w)
                .fold(0.0, f32::max)
        }
        Err(_) => text.chars().count() as f32 * size * 0.55,
    };
    measured.insert(key, width);
    width
}
