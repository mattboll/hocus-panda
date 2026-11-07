//! High contrast for what the compositor draws itself (#423, bar.md §13
//! "High contrast"): the radial menu, the overview's words, marks and
//! boxes, the magnifier's pill. They are drawn in the dark theme's colours;
//! under high contrast each colour is read as `hocus_link::theme::
//! on_contrast` says for the theme chosen (texts at 7:1, no tints nor
//! fills, solid rims), and rims are the cards' rim.
//!
//! What is drawn for an output is drawn on the loop's thread, the output's
//! look set first ([`set`]): a thread's own, so that two compositors in one
//! process (tests) never share it.

use std::cell::Cell;

use hocus_link::theme::{Rgba, Scheme, on_contrast};

thread_local! {
    static CONTRAST: Cell<Option<Scheme>> = const { Cell::new(None) };
}

/// What is drawn from now on this thread is in high contrast for `scheme`
/// (`None`: as drawn).
pub fn set(scheme: Option<Scheme>) {
    CONTRAST.with(|c| c.set(scheme));
}

/// The high contrast drawn in now, if any.
pub fn now() -> Option<Scheme> {
    CONTRAST.with(Cell::get)
}

fn rgba([r, g, b, a]: [u8; 4]) -> Rgba {
    Rgba {
        r,
        g,
        b,
        a: f32::from(a) / 255.0,
    }
}

fn bytes(c: Rgba) -> [u8; 4] {
    [c.r, c.g, c.b, (c.a.clamp(0.0, 1.0) * 255.0).round() as u8]
}

/// A colour (drawn for the dark theme) as it reads now.
pub fn ink(colour: [u8; 4]) -> [u8; 4] {
    match now() {
        None => colour,
        Some(scheme) => bytes(on_contrast(rgba(colour), scheme)),
    }
}

/// A rim's colour as it reads now: under high contrast, a solid one, the
/// cards' rim for a grey, the colour solid else.
pub fn rim(colour: [u8; 4]) -> [u8; 4] {
    let Some(scheme) = now() else {
        return colour;
    };
    if colour[3] == 0 {
        return colour;
    }
    let [r, g, b, _] = colour;
    let grey = r.max(g).max(b) - r.min(g).min(b) <= 0x28;
    if grey {
        bytes(scheme.contrast_tokens().card_rim)
    } else {
        let [r, g, b, _] = ink([r, g, b, 255]);
        [r, g, b, 255]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_read_as_the_contrast_tables_say_and_only_under_it() {
        set(None);
        assert_eq!(ink([14, 17, 28, 235]), [14, 17, 28, 235]);
        set(Some(Scheme::Dark));
        // A faint white veil (a fill, a hover) goes; a rim is solid.
        assert_eq!(ink([255, 255, 255, 10])[3], 0);
        assert_eq!(rim([255, 255, 255, 26]), [0x8E, 0x98, 0xAB, 255]);
        // The text white.
        assert_eq!(ink([0xEE, 0xF1, 0xF6, 0xFF]), [0xFF, 0xFF, 0xFF, 0xFF]);
        set(Some(Scheme::Light));
        assert_eq!(ink([0xEE, 0xF1, 0xF6, 0xFF]), [0, 0, 0, 0xFF]);
        set(None);
    }
}
