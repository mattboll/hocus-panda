//! An application's tile: its icon, else its letter on a colour of its
//! own, the same for the same name (the sound's applications, 3.4b; a
//! desktop's windows, 3.9b).

use iced::{
    Color, Rectangle,
    widget::{
        canvas::{self, Frame},
        text::Alignment,
    },
};

use super::{rounded, text, text_in};
use crate::levels::{
    draw::{MANROPE_BOLD, rgb},
    icons::Icon,
};

const LETTER_INK: Color = rgb(0x0B1020);
/// The letter tiles' colours (the 3.4 mockup's two first), one per
/// application by its name.
const TILES: [Color; 6] = [
    rgb(0xF5B34A),
    rgb(0x8FB3FF),
    rgb(0x7EE0A0),
    rgb(0xF7B98A),
    rgb(0xC9A7F5),
    rgb(0x6FD3D8),
];

/// Application `name`'s tile at `rect`: its `icon`, else its letter
/// (`letter` px) on its colour, rounded by `radius`; at `opacity`.
pub(crate) fn app_tile(
    f: &mut Frame,
    rect: Rectangle,
    (icon, name): (Option<&Icon>, &str),
    (radius, letter): (f32, f32),
    opacity: f32,
) {
    match icon {
        Some(Icon::Raster(handle)) => {
            f.draw_image(rect, canvas::Image::new(handle.clone()).opacity(opacity));
        }
        Some(Icon::Vector(handle)) => {
            f.draw_svg(
                rect,
                iced::advanced::svg::Svg::new(handle.clone()).opacity(opacity),
            );
        }
        None => {
            let colour = TILES[tile_colour(name)].scale_alpha(opacity);
            f.fill(&rounded(rect, radius), colour);
            let first = name
                .chars()
                .find(|c| c.is_alphanumeric())
                .map(|c| c.to_uppercase().collect::<String>())
                .unwrap_or_default();
            f.fill_text(text_in(
                MANROPE_BOLD,
                text(
                    first,
                    rect.center(),
                    letter,
                    LETTER_INK.scale_alpha(opacity),
                    Alignment::Center,
                ),
            ));
        }
    }
}

/// The colour of an application's letter tile: the same for the same
/// name.
fn tile_colour(name: &str) -> usize {
    let sum = name.bytes().fold(0_usize, |sum, b| {
        sum.wrapping_mul(31).wrapping_add(usize::from(b))
    });
    sum % TILES.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_letter_tile_keeps_its_colour() {
        assert_eq!(tile_colour("Musique"), tile_colour("Musique"));
        let colours: std::collections::HashSet<_> =
            ["Firefox", "Musique", "mpv", "Spotify", "Zoom"]
                .iter()
                .map(|n| tile_colour(n))
                .collect();
        assert!(colours.len() > 1);
    }
}
