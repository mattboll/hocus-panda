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

/// `s` in `font` at `size`, cut with "…" to fit `room` px when it does not.
pub fn elided(s: &str, size: f32, font: Font, room: f32) -> String {
    fitted(s, room, |cut| measure(cut, size, font))
}

/// `s` as it is when `width` says it fits `room`, else its longest start
/// that fits with "…" after it, never a space or a separator just before
/// the ellipsis ("Sur secteur ·…").
pub fn fitted(s: &str, room: f32, width: impl Fn(&str) -> f32) -> String {
    if width(s) <= room {
        return s.to_owned();
    }
    let ends: Vec<usize> = s.char_indices().map(|(i, _)| i).skip(1).collect();
    // The longest start that fits: cuts only get narrower as they shorten.
    let cut = |end: usize| {
        let kept = s[..end].trim_end_matches(|c: char| c.is_whitespace() || "·/-,:(".contains(c));
        format!("{kept}…")
    };
    let (mut fits, mut over) = (0, ends.len());
    while fits < over {
        let mid = (fits + over).div_ceil(2);
        if width(&cut(ends[mid - 1])) <= room {
            fits = mid;
        } else {
            over = mid - 1;
        }
    }
    if fits == 0 {
        "…".to_owned()
    } else {
        cut(ends[fits - 1])
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character 10 px wide, the ellipsis too.
    fn tens(s: &str) -> f32 {
        s.chars().count() as f32 * 10.0
    }

    #[test]
    fn a_line_too_wide_ends_with_an_ellipsis_at_the_room_it_has() {
        let line = "Sur secteur · jamais de veille";
        // It fits: as it is.
        assert_eq!(fitted(line, 300.0, tens), line);
        assert_eq!(fitted(line, 1000.0, tens), line);
        // One character short: cut, the ellipsis inside the room.
        assert_eq!(fitted(line, 290.0, tens), "Sur secteur · jamais de veil…");
        assert_eq!(fitted(line, 210.0, tens), "Sur secteur · jamais…");
        assert_eq!(fitted(line, 200.0, tens), "Sur secteur · jamai…");
        // Never a separator just before the ellipsis.
        assert_eq!(fitted(line, 140.0, tens), "Sur secteur…");
        assert_eq!(fitted(line, 150.0, tens), "Sur secteur…");
        // Narrower than anything: the ellipsis alone.
        assert_eq!(fitted(line, 15.0, tens), "…");
        assert_eq!(fitted(line, 0.0, tens), "…");
        // Characters of several bytes are cut whole.
        assert_eq!(fitted("Énergie · 80 %", 60.0, tens), "Énerg…");
        // Whatever the room, what comes back fits it (or is the ellipsis).
        for room in 0..400 {
            let room = room as f32;
            let cut = fitted(line, room, tens);
            assert!(tens(&cut) <= room || cut == "…", "{cut:?} in {room}");
        }
    }

    #[test]
    fn measured_lines_fit_their_room_in_the_window_font() {
        load_fonts();
        let line = "Sur secteur · jamais de veille";
        let full = measure(line, 12.5, MANROPE);
        assert!(full > 100.0, "{full}");
        assert_eq!(elided(line, 12.5, MANROPE, full + 1.0), line);
        for room in [40.0, 80.0, 120.0, full - 1.0] {
            let cut = elided(line, 12.5, MANROPE, room);
            assert!(cut.ends_with('…'), "{cut:?}");
            assert!(measure(&cut, 12.5, MANROPE) <= room, "{cut:?} in {room}");
        }
    }
}
