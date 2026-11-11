//! A field as a surface holds it (#771): the model, with what every
//! surface would else do again around it. A key goes to
//! [`Entry::key`] once the surface took its own (Enter, Escape, the
//! arrows of a list under it): what the field copies or cuts goes to the
//! clipboard from here, a paste is the surface's to ask
//! ([`crate::paste::request`]) and to hand back ([`Entry::paste`]). The
//! pointer's presses are counted here (a word, a line), Shift extending
//! with the modifiers the shell last heard. [`Entry::draw`] lays it out
//! where the surface says, in its font, and remembers where: a press
//! finds its place in the text from it.
//!
//! On one line: the surfaces' fields all are.

use std::{
    cell::Cell,
    ops::Range,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use hocus_link::{Clip, ClipKind, ToCompositor};
use iced::{
    Point,
    advanced::input_method,
    keyboard::{Key, Modifiers},
    widget::canvas::Frame,
    window,
};

use super::{
    Act, Field, Gesture, Kind, Lines, Outcome, ime, keys,
    view::{Laid, Look, Style},
};

/// Two presses this close in time, and 4 px in place, are one gesture: a
/// word, then a line.
const MULTI_CLICK: Duration = Duration::from_millis(400);

/// Whether Shift is held, as the shell last heard: a press with it
/// extends the selection.
static SHIFT: AtomicBool = AtomicBool::new(false);

/// The keyboard's modifiers changed.
pub fn modifiers(modifiers: Modifiers) {
    SHIFT.store(modifiers.shift(), Ordering::Relaxed);
}

/// Where a field's text is drawn: its left, its row's middle, the room
/// across, and how it is set.
#[derive(Debug, Clone, Copy)]
pub struct Place {
    pub left: f32,
    pub middle: f32,
    pub width: f32,
    pub style: Style,
}

/// What a field did with a key, or an input method's event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Took {
    /// Not a field's key.
    No,
    /// Its key, and nothing changed.
    Same,
    /// The caret, the selection or a preedit changed: it draws again.
    Moved,
    /// Its text changed.
    Edited,
    /// The clipboard is to be read, and its text [`Entry::paste`]d.
    Paste,
}

impl Took {
    /// Whether the surface draws again.
    pub fn draws(self) -> bool {
        matches!(self, Self::Moved | Self::Edited)
    }
}

/// One line of `len` bytes: Up and Down go to its ends.
struct OneLine(usize);

impl Lines for OneLine {
    fn line_of(&self, _: usize) -> usize {
        0
    }

    fn line(&self, line: usize) -> Option<Range<usize>> {
        (line == 0).then_some(0..self.0)
    }

    fn x_of(&self, at: usize) -> f32 {
        at as f32
    }

    fn at_x(&self, _: usize, x: f32) -> usize {
        (x.max(0.0) as usize).min(self.0)
    }
}

/// A field on a surface. See the module.
#[derive(Debug, Clone)]
pub struct Entry {
    field: Field,
    /// Where it was last drawn.
    seen: Cell<Option<Place>>,
    /// The last press: when, where, and how many in a row.
    clicks: Option<(Instant, Point, u8)>,
}

impl Entry {
    /// An empty field of `kind` that takes `most` characters.
    pub fn new(kind: Kind, most: usize) -> Self {
        Self {
            field: Field::new(kind, most),
            seen: Cell::new(None),
            clicks: None,
        }
    }

    pub fn text(&self) -> &str {
        self.field.text()
    }

    pub fn is_empty(&self) -> bool {
        self.field.is_empty()
    }

    /// How many characters it holds.
    pub fn chars(&self) -> usize {
        self.field.text().chars().count()
    }

    /// Whether the caret is at its end, nothing selected: Right has
    /// nowhere to go in it.
    pub fn at_end(&self) -> bool {
        self.field.caret() == self.field.text().len() && self.field.selection().is_empty()
    }

    /// `text` in place of what it holds, its caret at its end.
    pub fn set(&mut self, text: &str) {
        self.field.set(text);
    }

    /// Its text taken (kept aside, sent): it empties and forgets its
    /// history.
    pub fn take(&mut self) -> String {
        self.clicks = None;
        self.field.take()
    }

    /// Emptied, what it held overwritten and its history forgotten.
    pub fn clear(&mut self) {
        self.field.wipe();
        self.clicks = None;
    }

    /// What `key` does to it, once the surface took its own keys.
    pub fn key(&mut self, key: &Key, modifiers: Modifiers, text: Option<&str>) -> Took {
        match keys::act(key, modifiers, text) {
            Some(act) => self.apply(act),
            None => Took::No,
        }
    }

    fn apply(&mut self, act: Act) -> Took {
        let lines = OneLine(self.field.text().len());
        match self.field.apply(act, &lines) {
            Outcome::Same => Took::Same,
            Outcome::Moved => Took::Moved,
            Outcome::Edited => Took::Edited,
            Outcome::Copy(text) => {
                clip(text);
                Took::Same
            }
            Outcome::Cut(text) => {
                clip(text);
                Took::Edited
            }
            Outcome::Paste => Took::Paste,
        }
    }

    /// What the clipboard held, in at the caret: whether it fit.
    pub fn paste(&mut self, text: &str) -> bool {
        self.field.paste(text) == Outcome::Edited
    }

    /// An input method's preedit, shown at the caret, or its commit.
    pub fn input_method(&mut self, event: &input_method::Event) -> Took {
        use input_method::Event as Ime;
        let composed = match event {
            Ime::Opened => Outcome::Same,
            Ime::Preedit(text, cursor) => self.field.compose(text, cursor.clone()),
            Ime::Commit(text) => {
                self.field.compose("", None);
                return self.apply(Act::Insert(text.clone()));
            }
            Ime::Closed => self.field.compose("", None),
        };
        if composed == Outcome::Same {
            Took::Same
        } else {
            Took::Moved
        }
    }

    /// A press at `p`, where it was last drawn: the caret goes there; the
    /// second in a row takes a word, the third it all; with Shift, the
    /// selection grows to it.
    pub fn press(&mut self, p: Point) {
        let now = Instant::now();
        let clicks = match self.clicks {
            Some((then, q, n))
                if now.duration_since(then) <= MULTI_CLICK
                    && (q.x - p.x).abs() <= 4.0
                    && (q.y - p.y).abs() <= 4.0 =>
            {
                n % 3 + 1
            }
            _ => 1,
        };
        self.clicks = Some((now, p, clicks));
        let at = self.at(p);
        self.field.press(at, clicks, SHIFT.load(Ordering::Relaxed));
    }

    /// Whether a press in it is held.
    pub fn pressed(&self) -> bool {
        self.field.gesture != Gesture::Idle
    }

    /// The press dragged to `p`: whether the selection changed.
    pub fn drag(&mut self, p: Point) -> bool {
        if !self.pressed() {
            return false;
        }
        let at = self.at(p);
        self.field.drag(at)
    }

    /// The press let go: whether one was held.
    pub fn release(&mut self) -> bool {
        let held = self.pressed();
        self.field.release();
        held
    }

    /// Its selection gone, the caret where it is: a press in another
    /// field of its surface.
    pub fn deselect(&mut self) {
        self.field.deselect();
    }

    /// The place in its text nearest `p`, as it was last drawn.
    fn at(&self, p: Point) -> usize {
        self.laid("").map_or(self.field.text().len(), |l| l.at(p))
    }

    /// Laid out where it was last drawn.
    fn laid(&self, placeholder: &str) -> Option<Laid> {
        let place = self.seen.get()?;
        let mut laid = Laid::new(&self.field, place.width, place.style, placeholder);
        laid.place(Point::new(place.left, place.middle - laid.height() / 2.0));
        Some(laid)
    }

    /// Drawn at `place`: its text (`placeholder` while it is empty), its
    /// selection, and its caret while it has the keyboard (`focused`),
    /// where an input method then opens.
    pub fn draw(&self, f: &mut Frame, place: Place, placeholder: &str, look: &Look, focused: bool) {
        self.seen.set(Some(place));
        let Some(laid) = self.laid(placeholder) else {
            return;
        };
        self.field.scroll.set(laid.scroll);
        if focused {
            let purpose = match self.field.kind() {
                Kind::Secret => input_method::Purpose::Secure,
                Kind::Line | Kind::Lines => input_method::Purpose::Normal,
            };
            ime::drawn(laid.ime_cursor(), purpose);
        }
        laid.draw(f, look, focused);
    }
}

/// What a paste read for a field kept in its canvas's state, which the
/// reader's thread cannot reach: the canvas takes it as it next draws.
#[derive(Debug)]
pub struct Pasted(Mutex<Option<(window::Id, String)>>);

impl Pasted {
    pub const fn new() -> Self {
        Self(Mutex::new(None))
    }

    /// Reads the clipboard's text off the loop, cleaned as `clean` says,
    /// for the field on `id`; `wake` then has its canvas draw.
    pub fn ask(&'static self, id: window::Id, clean: crate::paste::Clean, wake: fn()) {
        crate::paste::request(clean, move |pasted| {
            let Some(pasted) = pasted.filter(|t| !t.is_empty()) else {
                return;
            };
            *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some((id, pasted.to_owned()));
            wake();
        });
    }

    /// What was read for the field on `id`, once; what was read for a
    /// surface gone is let go.
    pub fn take(&self, id: window::Id) -> Option<String> {
        let (on, mut text) = self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()?;
        if on == id {
            Some(text)
        } else {
            super::wipe(&mut text);
            None
        }
    }
}

/// `text` copied or cut out of a field: the clipboard takes it, as the
/// compositor offers the shell's copies. Never logged.
fn clip(text: String) {
    tracing::debug!(chars = text.chars().count(), "a field's text copied");
    crate::popover::tell(ToCompositor::Clip {
        paste: false,
        clip: Clip {
            kind: ClipKind::Text,
            data: text.into_bytes(),
        },
    });
}

#[cfg(test)]
mod tests {
    use iced::{Font, keyboard::key::Named};

    use super::*;

    fn measure(text: &str, _: f32, _: Font) -> f32 {
        text.chars().count() as f32 * 6.0
    }

    fn place() -> Place {
        Place {
            left: 10.0,
            middle: 20.0,
            width: 200.0,
            style: Style {
                size: 14.0,
                line: 20.0,
                font: Font::DEFAULT,
                rows: 1,
                measure,
                hint: (14.0, Font::DEFAULT),
                reveal: false,
            },
        }
    }

    fn typed(entry: &mut Entry, text: &str) {
        for c in text.chars() {
            let c = c.to_string();
            entry.key(
                &Key::Character(c.as_str().into()),
                Modifiers::empty(),
                Some(&c),
            );
        }
    }

    #[test]
    fn a_key_edits_at_the_caret_and_the_surfaces_keys_are_not_its() {
        let mut entry = Entry::new(Kind::Line, 20);
        typed(&mut entry, "fox");
        let none = Modifiers::empty();
        assert_eq!(
            entry.key(&Key::Named(Named::ArrowLeft), none, None),
            Took::Moved
        );
        typed(&mut entry, "o");
        assert_eq!(entry.text(), "foox");
        assert_eq!(entry.key(&Key::Named(Named::Enter), none, None), Took::No);
        assert_eq!(entry.key(&Key::Named(Named::Escape), none, None), Took::No);
        // Up and Down: its ends, on one line.
        assert_eq!(
            entry.key(&Key::Named(Named::ArrowUp), none, None),
            Took::Moved
        );
        typed(&mut entry, "a ");
        assert_eq!(entry.text(), "a foox");
        // A line break pasted is a space.
        assert!(entry.paste("b\nc "));
        assert_eq!(entry.text(), "a b c foox");
    }

    #[test]
    fn a_press_finds_its_place_where_it_was_drawn_and_a_second_takes_the_word() {
        let mut entry = Entry::new(Kind::Line, 40);
        typed(&mut entry, "un deux trois");
        entry.seen.set(Some(place()));
        // 6 px a character from x = 10: between « de » and « ux ».
        entry.press(Point::new(10.0 + 5.0 * 6.0, 20.0));
        assert_eq!(entry.field.caret(), 5);
        assert!(entry.pressed());
        assert!(entry.drag(Point::new(10.0 + 9.0 * 6.0, 20.0)));
        assert_eq!(&entry.text()[entry.field.selection()], "ux t");
        assert!(entry.release() && !entry.pressed());
        // Elsewhere, twice in a row: the word there.
        entry.press(Point::new(10.0 + 10.0 * 6.0, 20.0));
        entry.press(Point::new(10.0 + 10.0 * 6.0, 20.0));
        assert_eq!(&entry.text()[entry.field.selection()], "trois");
    }

    #[test]
    fn a_secret_says_its_length_only_and_copies_nothing() {
        let mut entry = Entry::new(Kind::Secret, 64);
        typed(&mut entry, "azerty");
        assert!(!format!("{entry:?}").contains("azerty"));
        let ctrl = Modifiers::CTRL;
        entry.key(&Key::Character("a".into()), ctrl, None);
        assert_eq!(
            entry.key(&Key::Character("c".into()), ctrl, None),
            Took::Same
        );
        assert_eq!(
            entry.key(&Key::Character("x".into()), ctrl, None),
            Took::Same
        );
        assert_eq!(entry.text(), "azerty", "nothing cut out");
        entry.clear();
        assert!(entry.is_empty());
    }
}
