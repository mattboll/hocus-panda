//! Text: measured (once each: the kit's views are built again as they
//! change, and measure the same words and names each time), cut with an
//! ellipsis, broken into lines.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use iced::advanced::graphics::text::cosmic_text::FontSystem;

use crate::levels::draw::{LINE, MANROPE, VALUE};

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

/// The widths measured, by text, size (in tenths) and font: forgotten
/// past [`MEASURES`], a name's prefixes measured to cut it being many.
type Measured = HashMap<(String, u32, iced::Font), f32>;
static MEASURED: Mutex<Option<Measured>> = Mutex::new(None);
const MEASURES: usize = 2048;

/// `text`'s width, measured once (an estimate if the font system is out
/// of reach).
pub(crate) fn measure(text: &str, size: f32, font: iced::Font) -> f32 {
    let key = (text.to_owned(), tenths(size), font);
    let mut measured = MEASURED.lock().unwrap_or_else(PoisonError::into_inner);
    let measured = measured.get_or_insert_with(HashMap::new);
    if let Some(width) = measured.get(&key) {
        return *width;
    }
    let width = match iced::advanced::graphics::text::font_system().write() {
        Ok(mut system) => measure_in(system.raw(), text, size, font),
        Err(_) => return text.chars().count() as f32 * size * 0.55,
    };
    if measured.len() >= MEASURES {
        measured.clear();
    }
    measured.insert(key, width);
    width
}

/// The `0`'s advance of the values' font, for tabular figures.
pub(crate) fn zero(system: &mut FontSystem) -> Option<f32> {
    crate::clock::measure(system, MANROPE, VALUE, VALUE * LINE).map(|m| m.zero)
}

/// The `0`'s advance of the values' font, measured once.
pub(crate) fn zero_once() -> f32 {
    static ZERO: std::sync::Mutex<Option<f32>> = std::sync::Mutex::new(None);
    let mut known = ZERO
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *known.get_or_insert_with(|| {
        iced::advanced::graphics::text::font_system()
            .write()
            .ok()
            .and_then(|mut system| zero(system.raw()))
            .unwrap_or(super::ZERO)
    })
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

/// `text` cut at its start with "…" to fit `room` as `width` measures it:
/// a folder, whose end tells it apart.
pub(crate) fn elide_start(text: &str, room: f32, mut width: impl FnMut(&str) -> f32) -> String {
    if width(text) <= room {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    for skip in 1..chars.len() {
        let cut = format!("…{}", chars[skip..].iter().collect::<String>());
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
