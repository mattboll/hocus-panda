//! The screen corners and touchpad swipes an action's line holds beside
//! its keys (issue #893, `hocus_config::triggers`), as the Settings say
//! them: "3 doigts ←" and "Coin ◤" on a chip, "Le coin en haut à gauche"
//! in a sentence.

pub use hocus_config::triggers::{Corner, FINGERS, Toward, Trigger};
use hocus_words::settings::triggers as words;

use crate::lang::Lang;

/// The corner drawn as an arrow: ◤ ◥ ◣ ◢.
pub fn arrow(corner: Corner) -> &'static str {
    match corner {
        Corner::TopLeft => "◤",
        Corner::TopRight => "◥",
        Corner::BottomLeft => "◣",
        Corner::BottomRight => "◢",
    }
}

/// "en haut à gauche".
pub fn place(corner: Corner, lang: Lang) -> &'static str {
    match corner {
        Corner::TopLeft => words::TOP_LEFT,
        Corner::TopRight => words::TOP_RIGHT,
        Corner::BottomLeft => words::BOTTOM_LEFT,
        Corner::BottomRight => words::BOTTOM_RIGHT,
    }
    .get(lang)
}

/// "gauche", "haut".
pub fn way(toward: Toward, lang: Lang) -> &'static str {
    match toward {
        Toward::Left => words::LEFT,
        Toward::Right => words::RIGHT,
        Toward::Up => words::UP,
        Toward::Down => words::DOWN,
    }
    .get(lang)
}

/// "3 doigts ← →": `fingers` that way, or those ways.
pub fn fingers(fingers: u8, ways: &[Toward], lang: Lang) -> String {
    let arrows: Vec<&str> = ways.iter().map(|t| t.arrow()).collect();
    words::fingers(
        lang,
        words::Fingers {
            fingers,
            arrows: arrows.join(" "),
        },
    )
}

/// On a chip: "3 doigts ←", "Coin ◤".
pub fn short(trigger: Trigger, lang: Lang) -> String {
    match trigger {
        Trigger::Swipe { fingers: n, toward } => fingers(n, &[toward], lang),
        Trigger::Corner(corner) => words::corner(lang, arrow(corner)),
    }
}

/// In a sentence: "3 doigts ↑", "Le coin en haut à gauche".
pub fn long(trigger: Trigger, lang: Lang) -> String {
    match trigger {
        Trigger::Swipe { .. } => short(trigger, lang),
        Trigger::Corner(corner) => words::the_corner(lang, place(corner, lang)),
    }
}

/// What the shortcuts' search finds it by, in both languages: "3 doigts",
/// "coin", "haut".
pub fn found_by(trigger: Trigger) -> String {
    let mut hay = Vec::new();
    for lang in [Lang::French, Lang::English] {
        hay.push(long(trigger, lang));
        hay.push(short(trigger, lang));
        match trigger {
            Trigger::Swipe { toward, .. } => {
                hay.push(way(toward, lang).to_owned());
                hay.push(words::GESTURE.get(lang).to_owned());
            }
            Trigger::Corner(corner) => hay.push(place(corner, lang).to_owned()),
        }
    }
    hay.push(trigger.written());
    hay.join(" ")
}

/// An item of a line as the window shows it: a trigger's chip words, else
/// the keycaps ("Super + Maj + E").
pub fn shown(text: &str, lang: Lang) -> String {
    match Trigger::parse(text) {
        Some(trigger) => short(trigger, lang),
        None => crate::keys::shown(text, lang),
    }
}

/// The triggers of a line, in its order.
pub fn of(list: &[String]) -> Vec<Trigger> {
    list.iter().filter_map(|t| Trigger::parse(t)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trigger_is_said_on_its_chip_and_in_a_sentence() {
        let up = Trigger::Swipe {
            fingers: 3,
            toward: Toward::Up,
        };
        assert_eq!(short(up, Lang::French), "3 doigts ↑");
        assert_eq!(short(up, Lang::English), "3 fingers ↑");
        let corner = Trigger::Corner(Corner::TopLeft);
        assert_eq!(short(corner, Lang::French), "Coin ◤");
        assert_eq!(long(corner, Lang::French), "Le coin en haut à gauche");
        assert_eq!(long(corner, Lang::English), "The top left corner");
        assert_eq!(
            fingers(3, &[Toward::Left, Toward::Right], Lang::French),
            "3 doigts ← →"
        );
        assert_eq!(shown("swipe:4:down", Lang::French), "4 doigts ↓");
        assert_eq!(shown("Super+Shift+E", Lang::French), "Super + Maj + E");
        assert!(found_by(up).contains("haut") && found_by(corner).contains("coin"));
        assert_eq!(of(&["Super+W".into(), "corner:top-left".into()]), [corner]);
    }
}
