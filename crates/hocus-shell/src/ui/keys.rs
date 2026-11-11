//! The keyboard over a surface's controls, the same on every surface:
//! `Tab` and `Shift+Tab` take the ring round the controls that can hold
//! it, in the tree's order; the arrows round a group of rows; `Enter`, and
//! `Space` off a field, act on what holds it; `Escape` closes. A control
//! that takes typing keeps the other keys.

use iced::keyboard::{Key, key::Named};

use super::control::Declared;
use crate::popover::{
    Pressed,
    kit::{cycle, step as next},
};

/// What a key comes to.
#[derive(Debug, Clone, PartialEq)]
pub enum Step<M> {
    /// The surface is told this.
    Tell(M),
    /// The surface closes.
    Escape,
    /// The key is the surface's, and does nothing (`Tab` with nothing to
    /// hold the ring).
    Kept,
    /// Not the kit's: the widgets have it.
    Pass,
}

/// What `pressed` comes to over `controls`, in the tree's order.
pub fn step<M: Clone>(controls: &[Declared<M>], pressed: &Pressed) -> Step<M> {
    let focusable: Vec<&Declared<M>> = controls.iter().filter(|d| d.on_focus.is_some()).collect();
    let at = focusable.iter().position(|d| d.focused);
    let focused = at.and_then(|i| focusable.get(i).copied());
    let typing = focused.is_some_and(|d| d.typing);
    let tell = |message: Option<&M>| message.cloned().map_or(Step::Kept, Step::Tell);
    match &pressed.key {
        Key::Named(Named::Escape) => Step::Escape,
        Key::Named(Named::Tab) => {
            let indices: Vec<usize> = (0..focusable.len()).collect();
            let to = next(&indices, at, pressed.modifiers.shift());
            tell(to.and_then(|i| focusable.get(i)?.on_focus.as_ref()))
        }
        Key::Named(Named::Enter) => tell(focused.and_then(|d| d.on_activate.as_ref())),
        Key::Named(Named::Space) if !typing => tell(focused.and_then(|d| d.on_activate.as_ref())),
        Key::Named(arrow @ (Named::ArrowUp | Named::ArrowDown)) if !typing => {
            let (Some(i), Some(group)) = (at, focused.and_then(|d| d.rows)) else {
                return Step::Pass;
            };
            let rows: Vec<usize> = (0..focusable.len())
                .filter(|j| focusable.get(*j).is_some_and(|d| d.rows == Some(group)))
                .collect();
            let to = cycle(&rows, i, *arrow == Named::ArrowDown);
            tell(to.and_then(|j| focusable.get(j)?.on_focus.as_ref()))
        }
        _ => Step::Pass,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hocus_a11y::{Node, Role};
    use iced::keyboard::Modifiers;

    fn declared(name: &'static str, rows: Option<u8>, focused: bool) -> Declared<&'static str> {
        Declared {
            node: Node::new(name, Role::Button, name),
            focused,
            on_focus: Some(name),
            on_activate: Some(name),
            rows,
            typing: false,
        }
    }

    fn key(named: Named, shift: bool) -> Pressed {
        Pressed {
            key: Key::Named(named),
            modifiers: if shift {
                Modifiers::SHIFT
            } else {
                Modifiers::empty()
            },
            text: None,
        }
    }

    #[test]
    fn tab_goes_round_the_controls_and_the_arrows_round_their_rows() {
        let line = Declared {
            on_focus: None,
            on_activate: None,
            ..declared("line", None, false)
        };
        let mut all = vec![
            declared("switch", None, false),
            line,
            declared("a", Some(0), false),
            declared("b", Some(0), false),
            declared("link", None, false),
        ];
        // From nothing, the first; Shift+Tab, the last.
        assert_eq!(step(&all, &key(Named::Tab, false)), Step::Tell("switch"));
        assert_eq!(step(&all, &key(Named::Tab, true)), Step::Tell("link"));
        // Nothing to act on.
        assert_eq!(step(&all, &key(Named::Enter, false)), Step::Kept);
        all[3].focused = true;
        assert_eq!(step(&all, &key(Named::Tab, false)), Step::Tell("link"));
        assert_eq!(step(&all, &key(Named::Tab, true)), Step::Tell("a"));
        assert_eq!(step(&all, &key(Named::ArrowDown, false)), Step::Tell("a"));
        assert_eq!(step(&all, &key(Named::Space, false)), Step::Tell("b"));
        assert_eq!(step(&all, &key(Named::Escape, false)), Step::Escape);
        // A field keeps Space and the arrows.
        all[3].typing = true;
        assert_eq!(step(&all, &key(Named::Space, false)), Step::Pass);
        assert_eq!(step(&all, &key(Named::ArrowUp, false)), Step::Pass);
        assert_eq!(step(&all, &key(Named::Enter, false)), Step::Tell("b"));
    }
}
