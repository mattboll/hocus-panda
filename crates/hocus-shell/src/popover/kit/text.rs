//! Text: measured, cut with an ellipsis, broken into lines, and the widths
//! a popover measures once and keeps.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use iced::advanced::graphics::text::cosmic_text::FontSystem;

use super::{MANROPE_SEMIBOLD, ROW_TEXT};
use crate::levels::draw::{LINE, MANROPE, MANROPE_BOLD, VALUE};

/// `text`'s width in `font` at `size`, with the font system at hand.
pub(crate) fn measure_in(system: &mut FontSystem, text: &str, size: f32, font: iced::Font) -> f32 {
    use iced::advanced::graphics::text::cosmic_text;
    let mut buffer = cosmic_text::Buffer::new(system, cosmic_text::Metrics::new(size, size * LINE));
    let attrs = iced::advanced::graphics::text::to_attributes(font);
    buffer.set_text(system, text, &attrs, cosmic_text::Shaping::Advanced, None);
    buffer.shape_until_scroll(system, false);
    buffer
        .layout_runs()
        .map(|run| run.line_w)
        .fold(0.0, f32::max)
}

/// `text`'s width, measured (an estimate if the font system is out of
/// reach).
pub(crate) fn measure(text: &str, size: f32, font: iced::Font) -> f32 {
    match iced::advanced::graphics::text::font_system().write() {
        Ok(mut system) => measure_in(system.raw(), text, size, font),
        Err(_) => text.chars().count() as f32 * size * 0.55,
    }
}

/// The `0`'s advance of the values' font, for tabular figures.
pub(crate) fn zero(system: &mut FontSystem) -> Option<f32> {
    crate::clock::measure(system, MANROPE, VALUE, VALUE * LINE).map(|m| m.zero)
}

/// `text` cut with an ellipsis to fit `room` px as `width` measures it.
pub(crate) fn elide(text: &str, room: f32, mut width: impl FnMut(&str) -> f32) -> String {
    if width(text) <= room {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    for keep in (0..chars.len()).rev() {
        let cut: String = chars[..keep]
            .iter()
            .collect::<String>()
            .trim_end()
            .to_owned()
            + "…";
        if width(&cut) <= room {
            return cut;
        }
    }
    "…".to_owned()
}

/// `text` broken between its words into lines that fit `room` as `width`
/// measures them; a word too wide alone keeps its own line.
pub(crate) fn wrap(text: &str, room: f32, mut width: impl FnMut(&str) -> f32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split(' ') {
        match lines.last_mut() {
            Some(line) if width(&format!("{line} {word}")) <= room => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_owned()),
        }
    }
    lines
}

/// `text` at `size` in lines that fit `room`, as [`wrap`] breaks them;
/// measured once each.
pub(crate) fn wrapped(text: &'static str, size: f32, room: f32) -> Vec<String> {
    type Key = (&'static str, u32, u32);
    static WRAPPED: Mutex<Vec<(Key, Vec<String>)>> = Mutex::new(Vec::new());
    let key = (text, tenths(size), room as u32);
    let mut known = WRAPPED.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((_, lines)) = known.iter().find(|(k, _)| *k == key) {
        return lines.clone();
    }
    let lines = wrap(text, room, |t| measure(t, size, MANROPE));
    known.push((key, lines.clone()));
    lines
}

/// A key for a measure: the size in tenths.
fn tenths(size: f32) -> u32 {
    (size * 10.0).round() as u32
}

/// Words measured once, and names cut to their room once: a popover
/// measures ahead of drawing, which only reads.
#[derive(Debug, Default)]
pub(crate) struct Widths {
    /// (text, size, bold) → width.
    widths: HashMap<(&'static str, u32, bool), f32>,
    /// (name, room, semibold) → shown.
    names: HashMap<(String, u32, bool), String>,
}

impl Widths {
    /// A word's width, measured once.
    pub(crate) fn width(&mut self, text: &'static str, size: f32, bold: bool) -> f32 {
        let key = (text, tenths(size), bold);
        if let Some(width) = self.widths.get(&key) {
            return *width;
        }
        let width = measure(text, size, if bold { MANROPE_BOLD } else { MANROPE });
        self.widths.insert(key, width);
        width
    }

    /// A word's width as measured (an estimate if it was not).
    pub(crate) fn known(&self, text: &'static str, size: f32, bold: bool) -> f32 {
        self.widths
            .get(&(text, tenths(size), bold))
            .copied()
            .unwrap_or(text.chars().count() as f32 * size * 0.55)
    }

    /// `name` (a row's, 14 px; `semibold`, a connected block's) cut to fit
    /// `room`, once.
    pub(crate) fn fit(&mut self, name: &str, room: f32, semibold: bool) -> String {
        let key = (name.to_owned(), room.max(0.0) as u32, semibold);
        if let Some(shown) = self.names.get(&key) {
            return shown.clone();
        }
        let font = if semibold { MANROPE_SEMIBOLD } else { MANROPE };
        let shown = match iced::advanced::graphics::text::font_system().write() {
            Ok(mut system) => elide(name, room, |t| measure_in(system.raw(), t, ROW_TEXT, font)),
            Err(_) => name.to_owned(),
        };
        self.names.insert(key, shown.clone());
        shown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_names_are_cut_with_an_ellipsis() {
        let width = |t: &str| t.chars().count() as f32 * 7.0;
        assert_eq!(elide("Casque", 100.0, width), "Casque");
        assert_eq!(
            elide("Haut-parleurs du portable", 100.0, width),
            "Haut-parleurs…"
        );
        assert_eq!(elide("abc", 1.0, width), "…");
    }

    #[test]
    fn the_error_wraps_between_its_words() {
        let width = |t: &str| t.chars().count() as f32 * 6.0;
        assert_eq!(
            wrap(
                "Mot de passe incorrect. Vérifie-le et réessaie.",
                150.0,
                width
            ),
            ["Mot de passe incorrect.", "Vérifie-le et réessaie."]
        );
        assert_eq!(wrap("Short.", 214.0, width), ["Short."]);
        assert_eq!(wrap("a b", 1.0, width), ["a", "b"]);
    }
}
