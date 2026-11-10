//! What a key asks of a text field, as everywhere: the arrows, Home and
//! End (with Ctrl, the text's ends), Shift with any of them to select,
//! Backspace and Delete (with Ctrl, a word), `Ctrl+A`, `Ctrl+C`/`X`/`V`
//! (and `Ctrl+Insert`, `Shift+Delete`, `Shift+Insert`), `Ctrl+Z`,
//! `Ctrl+Shift+Z` and `Ctrl+Y`, and what is typed. Enter, Tab and Escape
//! are the surface's.

use iced::keyboard::{Key, Modifiers, key::Named};

use super::{Act, Motion};

/// What `key` with `modifiers`, typing `text`, asks of a field; `None`
/// when it is not a field's key.
pub fn act(key: &Key, modifiers: Modifiers, text: Option<&str>) -> Option<Act> {
    let ctrl = modifiers.control();
    let shift = modifiers.shift();
    if modifiers.alt() || modifiers.logo() {
        return None;
    }
    let go = |motion| Act::Move {
        motion,
        extend: shift,
    };
    match key {
        Key::Named(named) => Some(match named {
            Named::ArrowLeft if ctrl => go(Motion::WordLeft),
            Named::ArrowRight if ctrl => go(Motion::WordRight),
            Named::ArrowLeft => go(Motion::Left),
            Named::ArrowRight => go(Motion::Right),
            Named::ArrowUp => go(Motion::Up),
            Named::ArrowDown => go(Motion::Down),
            Named::Home if ctrl => go(Motion::Start),
            Named::End if ctrl => go(Motion::End),
            Named::Home => go(Motion::LineStart),
            Named::End => go(Motion::LineEnd),
            Named::Backspace => Act::Backspace { word: ctrl },
            Named::Delete if shift && !ctrl => Act::Cut,
            Named::Delete => Act::Delete { word: ctrl },
            Named::Insert if ctrl && !shift => Act::Copy,
            Named::Insert if shift && !ctrl => Act::Paste,
            Named::Space if !ctrl => Act::Insert(" ".to_owned()),
            _ => return None,
        }),
        Key::Character(c) if ctrl => {
            let c = c.to_lowercase();
            Some(match c.as_str() {
                "a" if !shift => Act::SelectAll,
                "c" if !shift => Act::Copy,
                "x" if !shift => Act::Cut,
                // With Shift too, as terminals take it.
                "v" => Act::Paste,
                "z" if shift => Act::Redo,
                "z" => Act::Undo,
                "y" if !shift => Act::Redo,
                _ => return None,
            })
        }
        _ => text
            .filter(|t| !t.is_empty() && !t.chars().any(char::is_control))
            .map(|t| Act::Insert(t.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(n: Named, m: Modifiers) -> Option<Act> {
        act(&Key::Named(n), m, None)
    }

    fn ctrl(c: &str, m: Modifiers) -> Option<Act> {
        act(&Key::Character(c.into()), m | Modifiers::CTRL, None)
    }

    #[test]
    fn the_arrows_move_and_shift_selects() {
        let to = |motion, extend| Some(Act::Move { motion, extend });
        assert_eq!(
            named(Named::ArrowLeft, Modifiers::empty()),
            to(Motion::Left, false)
        );
        assert_eq!(
            named(Named::ArrowRight, Modifiers::SHIFT),
            to(Motion::Right, true)
        );
        assert_eq!(
            named(Named::ArrowLeft, Modifiers::CTRL),
            to(Motion::WordLeft, false)
        );
        assert_eq!(
            named(Named::ArrowRight, Modifiers::CTRL | Modifiers::SHIFT),
            to(Motion::WordRight, true)
        );
        assert_eq!(
            named(Named::Home, Modifiers::empty()),
            to(Motion::LineStart, false)
        );
        assert_eq!(
            named(Named::End, Modifiers::SHIFT),
            to(Motion::LineEnd, true)
        );
        assert_eq!(
            named(Named::Home, Modifiers::CTRL),
            to(Motion::Start, false)
        );
        assert_eq!(
            named(Named::ArrowUp, Modifiers::empty()),
            to(Motion::Up, false)
        );
        assert_eq!(
            named(Named::ArrowDown, Modifiers::SHIFT),
            to(Motion::Down, true)
        );
    }

    #[test]
    fn deleting_the_clipboard_and_undoing() {
        assert_eq!(
            named(Named::Backspace, Modifiers::CTRL),
            Some(Act::Backspace { word: true })
        );
        assert_eq!(
            named(Named::Delete, Modifiers::empty()),
            Some(Act::Delete { word: false })
        );
        assert_eq!(named(Named::Delete, Modifiers::SHIFT), Some(Act::Cut));
        assert_eq!(named(Named::Insert, Modifiers::SHIFT), Some(Act::Paste));
        assert_eq!(ctrl("a", Modifiers::empty()), Some(Act::SelectAll));
        assert_eq!(ctrl("C", Modifiers::empty()), Some(Act::Copy));
        assert_eq!(ctrl("x", Modifiers::empty()), Some(Act::Cut));
        assert_eq!(ctrl("v", Modifiers::SHIFT), Some(Act::Paste));
        assert_eq!(ctrl("z", Modifiers::empty()), Some(Act::Undo));
        assert_eq!(ctrl("Z", Modifiers::SHIFT), Some(Act::Redo));
        assert_eq!(ctrl("y", Modifiers::empty()), Some(Act::Redo));
        assert_eq!(ctrl("s", Modifiers::empty()), None, "not a field's");
    }

    #[test]
    fn what_is_typed_goes_in_and_the_surface_keeps_its_keys() {
        assert_eq!(
            act(&Key::Character("é".into()), Modifiers::empty(), Some("é")),
            Some(Act::Insert("é".to_owned()))
        );
        assert_eq!(
            act(&Key::Character("b".into()), Modifiers::ALT, Some("b")),
            None
        );
        for n in [Named::Enter, Named::Tab, Named::Escape] {
            assert_eq!(named(n, Modifiers::empty()), None, "{n:?}");
        }
    }
}
