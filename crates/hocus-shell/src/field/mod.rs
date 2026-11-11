//! The shell's text field (#763), edited as everywhere: the caret moves by
//! character, by word, to a line's ends and between lines; Shift with any
//! of them selects; `Ctrl+A` selects it all; Backspace and Delete take a
//! character, or a word with Ctrl; what is typed replaces what is
//! selected; `Ctrl+C`, `Ctrl+X`, `Ctrl+V`; `Ctrl+Z` and `Ctrl+Shift+Z`
//! (or `Ctrl+Y`). The pointer places the caret, a drag selects, a double
//! click takes a word, a triple click its line, Shift+click extends. An
//! input method's preedit shows at the caret, its commit goes in there.
//!
//! The shell draws its own widgets on canvases, so a field is three
//! pieces a surface puts together:
//!
//! - [`Field`], the model: its text, its caret and the selection's anchor
//!   (byte offsets on character boundaries), what the pointer does
//!   ([`Gesture`]), its undo history, an input method's preedit. Pure: it
//!   is told what a key or the pointer asks ([`Act`], [`Field::press`]),
//!   and says what came of it ([`Outcome`]); reading the clipboard and
//!   writing to it are the surface's.
//! - [`keys::act`], what a key asks of a field.
//! - [`view`], a field laid out (its rows, wrapped; scrolled to keep the
//!   caret in view), where a point falls in it, and drawn: its text, the
//!   selection in the theme's selection colour, the preedit underlined,
//!   the caret.
//!
//! [`ime`] asks the compositor for an input method while a field has the
//! keyboard.
//!
//! A [`Kind::Secret`] field (a key, a password) is masked when drawn,
//! never copied nor cut out, keeps no undo history, moves by word as by
//! its whole, and is overwritten with zeros when it empties or goes.
//!
//! A surface holds an [`Entry`] ([`entry`]): the field with its keys, its
//! pointer and where it was last drawn, which is all a surface asks of it
//! (#771). The launcher's, the actions', the emoji picker's and the
//! clipboard's searches, the task to add, the day's sources' address and
//! token, the Wi-Fi's and the authentication's passwords, and the text
//! typed into a capture are each one.

pub mod entry;
pub mod ime;
pub mod keys;
pub mod view;

pub use entry::{Entry, Place, Took};

use std::{cell::Cell, ops::Range};

/// What a field holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Text on one line: a line break pasted is a space.
    Line,
    /// Text on lines of its own (a line break goes in). No surface has one
    /// since the secretary's drawer went.
    #[allow(dead_code)]
    Lines,
    /// A secret on one line: masked, never copied, nothing kept to undo.
    Secret,
}

/// A way the caret moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    WordLeft,
    WordRight,
    LineStart,
    LineEnd,
    Up,
    Down,
    Start,
    End,
}

/// What a key asks of a field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    /// The caret moves; with `extend`, the selection follows it.
    Move {
        motion: Motion,
        extend: bool,
    },
    SelectAll,
    /// Typed, or committed by an input method.
    Insert(String),
    Backspace {
        word: bool,
    },
    Delete {
        word: bool,
    },
    Copy,
    Cut,
    Paste,
    Undo,
    Redo,
}

/// What came of an [`Act`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing changed: at an end already, nothing to undo, too long.
    Same,
    /// The caret or the selection moved.
    Moved,
    /// The text changed.
    Edited,
    /// This goes to the clipboard.
    Copy(String),
    /// This goes to the clipboard, and left the text.
    Cut(String),
    /// The clipboard is to be read, and its text [`Field::paste`]d.
    Paste,
}

/// How a field's text is cut into the lines the caret moves along: by the
/// layout ([`view::Laid`]), or by its line breaks ([`Paragraphs`]).
/// Offsets are the field's text's.
pub trait Lines {
    /// The line `at` is on.
    fn line_of(&self, at: usize) -> usize;
    /// What line `line` holds, if there is one.
    fn line(&self, line: usize) -> Option<Range<usize>>;
    /// How far across its line `at` is.
    fn x_of(&self, at: usize) -> f32;
    /// The place on `line` nearest `x` across.
    fn at_x(&self, line: usize, x: f32) -> usize;
}

/// A text's lines between its line breaks, each character one unit wide:
/// for a field not laid out yet.
#[derive(Debug, Clone, Copy)]
pub struct Paragraphs<'a>(pub &'a str);

impl Paragraphs<'_> {
    fn ranges(&self) -> impl Iterator<Item = Range<usize>> + '_ {
        let mut start = 0;
        self.0.split('\n').map(move |p| {
            let range = start..start + p.len();
            start += p.len() + 1;
            range
        })
    }
}

impl Lines for Paragraphs<'_> {
    fn line_of(&self, at: usize) -> usize {
        self.0[..at.min(self.0.len())].matches('\n').count()
    }

    fn line(&self, line: usize) -> Option<Range<usize>> {
        self.ranges().nth(line)
    }

    fn x_of(&self, at: usize) -> f32 {
        let start = self.0[..at].rfind('\n').map_or(0, |i| i + 1);
        self.0[start..at].chars().count() as f32
    }

    fn at_x(&self, line: usize, x: f32) -> usize {
        let Some(range) = self.line(line) else {
            return self.0.len();
        };
        let wanted = x.round().max(0.0) as usize;
        self.0[range.clone()]
            .char_indices()
            .nth(wanted)
            .map_or(range.end, |(i, _)| range.start + i)
    }
}

/// What the pointer does with a field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gesture {
    Idle,
    /// A press held: the selection grows from `origin` by `unit` as it
    /// drags.
    Dragging {
        unit: Unit,
        origin: Range<usize>,
    },
}

/// What a drag selects by: one, two or three presses in a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Char,
    Word,
    Line,
}

/// What the last step of the undo history gathers: typing goes back a
/// word at a time, a run of deletions at once, the rest one by one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// The caret moved since: the next edit is a step of its own.
    Closed,
    Typing,
    Deleting,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Snapshot {
    text: String,
    caret: usize,
    anchor: usize,
}

/// The steps that can be undone, and redone.
#[derive(Debug, Clone)]
struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    step: Step,
}

/// The most steps kept.
const UNDO_MOST: usize = 200;

/// An input method's text being composed at the caret, and where its own
/// caret or selection is in it (bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preedit {
    pub text: String,
    pub cursor: Option<Range<usize>>,
}

/// Where a field is scrolled: its first row in view (on lines), how far
/// across (on one line).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Scroll {
    pub first: usize,
    pub across: f32,
}

/// A text field. See the module.
#[derive(Clone)]
pub struct Field {
    kind: Kind,
    text: String,
    caret: usize,
    anchor: usize,
    /// The characters it takes at most.
    most: usize,
    /// How far across Up and Down keep the caret, from the first of them.
    goal: Option<f32>,
    history: History,
    preedit: Option<Preedit>,
    pub gesture: Gesture,
    /// Kept by its layout from one to the next: where it draws.
    pub scroll: Cell<Scroll>,
}

/// A secret never says itself, nor what was typed before it: its length
/// only.
impl std::fmt::Debug for Field {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut says = f.debug_struct("Field");
        says.field("kind", &self.kind);
        match self.kind {
            Kind::Secret => says.field("chars", &self.text.chars().count()),
            Kind::Line | Kind::Lines => says.field("text", &self.text),
        };
        says.field("caret", &self.caret)
            .field("anchor", &self.anchor)
            .finish_non_exhaustive()
    }
}

impl Drop for Field {
    fn drop(&mut self) {
        if self.kind == Kind::Secret {
            wipe(&mut self.text);
        }
    }
}

impl Field {
    /// An empty field of `kind` that takes `most` characters.
    pub fn new(kind: Kind, most: usize) -> Self {
        // A secret's room taken once: it never moves in memory, leaving a
        // copy behind.
        let text = match kind {
            Kind::Secret => String::with_capacity(most * 4),
            Kind::Line | Kind::Lines => String::new(),
        };
        Self {
            kind,
            text,
            caret: 0,
            anchor: 0,
            most,
            goal: None,
            history: History {
                undo: Vec::new(),
                redo: Vec::new(),
                step: Step::Closed,
            },
            preedit: None,
            gesture: Gesture::Idle,
            scroll: Cell::default(),
        }
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn caret(&self) -> usize {
        self.caret
    }

    pub fn anchor(&self) -> usize {
        self.anchor
    }

    /// What is selected, in order; empty if nothing.
    pub fn selection(&self) -> Range<usize> {
        self.caret.min(self.anchor)..self.caret.max(self.anchor)
    }

    pub fn preedit(&self) -> Option<&Preedit> {
        self.preedit.as_ref()
    }

    /// `text` in place of what it holds, its caret at its end: a step that
    /// can be undone.
    pub fn set(&mut self, text: &str) {
        if text == self.text {
            return;
        }
        self.remember(None);
        self.history.step = Step::Closed;
        self.text.clear();
        self.text.push_str(&clean(text, self.kind));
        self.caret = self.text.len();
        self.anchor = self.caret;
        self.goal = None;
    }

    /// Its text taken (sent, saved): it empties and forgets its history.
    pub fn take(&mut self) -> String {
        let capacity = self.text.capacity();
        let taken = std::mem::replace(&mut self.text, String::with_capacity(capacity));
        self.forget();
        taken
    }

    /// Its text overwritten with zeros, and gone, with its history.
    pub fn wipe(&mut self) {
        wipe(&mut self.text);
        if self.kind == Kind::Secret {
            self.text.reserve(self.most * 4);
        }
        self.forget();
    }

    fn forget(&mut self) {
        self.caret = 0;
        self.anchor = 0;
        self.goal = None;
        self.preedit = None;
        self.gesture = Gesture::Idle;
        self.scroll.set(Scroll::default());
        for snapshot in self
            .history
            .undo
            .iter_mut()
            .chain(self.history.redo.iter_mut())
        {
            wipe(&mut snapshot.text);
        }
        self.history.undo.clear();
        self.history.redo.clear();
        self.history.step = Step::Closed;
    }

    /// What `act` does, with the text cut into `lines`.
    pub fn apply(&mut self, act: Act, lines: &dyn Lines) -> Outcome {
        match act {
            Act::Move { motion, extend } => self.go(motion, extend, lines),
            Act::SelectAll => {
                if self.selection() == (0..self.text.len()) {
                    return Outcome::Same;
                }
                self.anchor = 0;
                self.caret = self.text.len();
                self.moved();
                Outcome::Moved
            }
            Act::Insert(typed) => self.insert(&typed, Step::Typing),
            Act::Backspace { word } => {
                let range = if !self.selection().is_empty() {
                    self.selection()
                } else if word {
                    self.word_left(self.caret)..self.caret
                } else {
                    before(&self.text, self.caret)..self.caret
                };
                self.remove(range)
            }
            Act::Delete { word } => {
                let range = if !self.selection().is_empty() {
                    self.selection()
                } else if word {
                    self.caret..self.word_right(self.caret)
                } else {
                    self.caret..after(&self.text, self.caret)
                };
                self.remove(range)
            }
            Act::Copy => match self.selected() {
                Some(text) => Outcome::Copy(text),
                None => Outcome::Same,
            },
            Act::Cut => match self.selected() {
                Some(text) => {
                    self.remove(self.selection());
                    Outcome::Cut(text)
                }
                None => Outcome::Same,
            },
            Act::Paste => Outcome::Paste,
            Act::Undo => self.back(true),
            Act::Redo => self.back(false),
        }
    }

    /// What the clipboard held, in at the caret: a step of its own.
    pub fn paste(&mut self, text: &str) -> Outcome {
        self.history.step = Step::Closed;
        let outcome = self.insert(text, Step::Closed);
        self.history.step = Step::Closed;
        outcome
    }

    /// What is selected, if it may leave the field.
    fn selected(&self) -> Option<String> {
        let range = self.selection();
        (self.kind != Kind::Secret && !range.is_empty()).then(|| self.text[range].to_owned())
    }

    /// An input method composes `text` at the caret (empty: it stopped).
    pub fn compose(&mut self, text: &str, cursor: Option<Range<usize>>) -> Outcome {
        let preedit = (!text.is_empty()).then(|| Preedit {
            text: text.to_owned(),
            cursor: cursor.filter(|c| c.end <= text.len()),
        });
        if preedit == self.preedit {
            return Outcome::Same;
        }
        self.preedit = preedit;
        Outcome::Edited
    }

    // --- The pointer ----------------------------------------------------------------------

    /// A press at `at`, the `clicks`th in a row (a place, a word, a line);
    /// with `extend` (Shift), the selection grows to it instead.
    pub fn press(&mut self, at: usize, clicks: u8, extend: bool) {
        let at = floor(&self.text, at);
        self.preedit = None;
        let (unit, range) = match clicks {
            0 | 1 => (Unit::Char, at..at),
            2 => (Unit::Word, self.word_around(at)),
            _ => (Unit::Line, line_around(&self.text, at)),
        };
        if extend && unit == Unit::Char {
            let anchor = self.anchor;
            self.caret = at;
            self.gesture = Gesture::Dragging {
                unit,
                origin: anchor..anchor,
            };
        } else {
            self.anchor = range.start;
            self.caret = range.end;
            self.gesture = Gesture::Dragging {
                unit,
                origin: range,
            };
        }
        self.moved();
    }

    /// The press dragged to `at`: whether the selection changed.
    pub fn drag(&mut self, at: usize) -> bool {
        let Gesture::Dragging { unit, origin } = self.gesture.clone() else {
            return false;
        };
        let at = floor(&self.text, at);
        let here = match unit {
            Unit::Char => at..at,
            Unit::Word => self.word_around(at),
            Unit::Line => line_around(&self.text, at),
        };
        let (anchor, caret) = if here.start < origin.start {
            (origin.end, here.start)
        } else {
            (origin.start, here.end.max(origin.end))
        };
        if (anchor, caret) == (self.anchor, self.caret) {
            return false;
        }
        self.anchor = anchor;
        self.caret = caret;
        self.moved();
        true
    }

    /// The press let go.
    pub fn release(&mut self) {
        self.gesture = Gesture::Idle;
    }

    /// The selection gone, the caret where it is: a press elsewhere.
    pub fn deselect(&mut self) -> bool {
        if self.anchor == self.caret {
            return false;
        }
        self.anchor = self.caret;
        true
    }

    // --- Editing --------------------------------------------------------------------------

    fn go(&mut self, motion: Motion, extend: bool, lines: &dyn Lines) -> Outcome {
        let selection = self.selection();
        let collapse = !extend && !selection.is_empty();
        let caret = match motion {
            Motion::Left if collapse => selection.start,
            Motion::Right if collapse => selection.end,
            Motion::Left => before(&self.text, self.caret),
            Motion::Right => after(&self.text, self.caret),
            Motion::WordLeft => self.word_left(self.caret),
            Motion::WordRight => self.word_right(self.caret),
            Motion::LineStart => lines.line(lines.line_of(self.caret)).map_or(0, |l| l.start),
            Motion::LineEnd => lines
                .line(lines.line_of(self.caret))
                .map_or(self.text.len(), |l| l.end),
            Motion::Up | Motion::Down => {
                let goal = *self.goal.get_or_insert_with(|| lines.x_of(self.caret));
                let line = lines.line_of(self.caret);
                let next = if motion == Motion::Up {
                    line.checked_sub(1)
                } else {
                    Some(line + 1)
                };
                match next.filter(|n| lines.line(*n).is_some()) {
                    Some(n) => lines.at_x(n, goal),
                    None if motion == Motion::Up => 0,
                    None => self.text.len(),
                }
            }
            Motion::Start => 0,
            Motion::End => self.text.len(),
        };
        let caret = floor(&self.text, caret.min(self.text.len()));
        let anchor = if extend { self.anchor } else { caret };
        if !matches!(motion, Motion::Up | Motion::Down) {
            self.goal = None;
        }
        self.history.step = Step::Closed;
        if (caret, anchor) == (self.caret, self.anchor) {
            return Outcome::Same;
        }
        self.caret = caret;
        self.anchor = anchor;
        self.preedit = None;
        Outcome::Moved
    }

    /// `typed` in place of the selection, as a step of `step`.
    fn insert(&mut self, typed: &str, step: Step) -> Outcome {
        let typed = clean(typed, self.kind);
        let range = self.selection();
        let chars = self.text.chars().count() - self.text[range.clone()].chars().count();
        if typed.is_empty() && range.is_empty() {
            return Outcome::Same;
        }
        if chars + typed.chars().count() > self.most {
            return Outcome::Same;
        }
        // A word typed is one step: a space after it starts the next.
        let starts_word = typed.starts_with(char::is_whitespace);
        let merge = step == Step::Typing
            && self.history.step == Step::Typing
            && range.is_empty()
            && !starts_word;
        if !merge {
            self.remember(Some(step));
        }
        self.text.replace_range(range.clone(), &typed);
        self.caret = range.start + typed.len();
        self.anchor = self.caret;
        self.moved();
        self.history.step = step;
        Outcome::Edited
    }

    fn remove(&mut self, range: Range<usize>) -> Outcome {
        if range.is_empty() {
            self.history.step = Step::Closed;
            return Outcome::Same;
        }
        let whole = !self.selection().is_empty();
        if whole || self.history.step != Step::Deleting {
            self.remember(Some(Step::Deleting));
        }
        self.text.replace_range(range.clone(), "");
        self.caret = range.start;
        self.anchor = range.start;
        self.moved();
        self.history.step = if whole { Step::Closed } else { Step::Deleting };
        Outcome::Edited
    }

    /// The field as it is, kept to undo; what is redone forgotten.
    fn remember(&mut self, step: Option<Step>) {
        if self.kind == Kind::Secret {
            return;
        }
        let history = &mut self.history;
        history.undo.push(Snapshot {
            text: self.text.clone(),
            caret: self.caret,
            anchor: self.anchor,
        });
        if history.undo.len() > UNDO_MOST {
            history.undo.remove(0);
        }
        history.redo.clear();
        history.step = step.unwrap_or(Step::Closed);
    }

    /// A step undone (`undo`) or redone.
    fn back(&mut self, undo: bool) -> Outcome {
        let history = &mut self.history;
        let (from, to) = if undo {
            (&mut history.undo, &mut history.redo)
        } else {
            (&mut history.redo, &mut history.undo)
        };
        let Some(snapshot) = from.pop() else {
            return Outcome::Same;
        };
        to.push(Snapshot {
            text: std::mem::replace(&mut self.text, snapshot.text),
            caret: self.caret,
            anchor: self.anchor,
        });
        self.caret = snapshot.caret;
        self.anchor = snapshot.anchor;
        history.step = Step::Closed;
        self.moved();
        Outcome::Edited
    }

    /// After the caret moved by itself: Up and Down measure again, a
    /// preedit is let go.
    fn moved(&mut self) {
        self.goal = None;
        self.preedit = None;
    }

    // --- Words ----------------------------------------------------------------------------

    /// Where the word before `at` starts (a secret has one word: itself).
    fn word_left(&self, at: usize) -> usize {
        if self.kind == Kind::Secret {
            return 0;
        }
        let head = &self.text[..at];
        let past_gap = head.trim_end_matches(|c| !is_word(c)).len();
        head[..past_gap].trim_end_matches(is_word).len()
    }

    /// Where the word after `at` ends.
    fn word_right(&self, at: usize) -> usize {
        if self.kind == Kind::Secret {
            return self.text.len();
        }
        let tail = &self.text[at..];
        let gap = tail.len() - tail.trim_start_matches(|c| !is_word(c)).len();
        let word = tail[gap..].len() - tail[gap..].trim_start_matches(is_word).len();
        at + gap + word
    }

    fn word_around(&self, at: usize) -> Range<usize> {
        if self.kind == Kind::Secret {
            return 0..self.text.len();
        }
        word_around(&self.text, at)
    }
}

/// Whether `c` is a word's: a double click takes it, Ctrl moves over it.
pub fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-'
}

/// The word around `at` in `text` (just past its last letter: that word),
/// or the character there if it is no word's; empty in an empty text.
pub fn word_around(text: &str, at: usize) -> Range<usize> {
    let at = floor(text, at.min(text.len()));
    let after = text[at..].chars().next();
    let before = text[..at].chars().next_back();
    let at = match (after, before) {
        (Some(a), Some(b)) if !is_word(a) && is_word(b) => at - b.len_utf8(),
        (None, Some(b)) => at - b.len_utf8(),
        _ => at,
    };
    let Some(c) = text[at..].chars().next() else {
        return at..at;
    };
    if !is_word(c) {
        return at..at + c.len_utf8();
    }
    let from = text[..at].trim_end_matches(is_word).len();
    let to = text[at..]
        .char_indices()
        .find(|(_, c)| !is_word(*c))
        .map_or(text.len(), |(i, _)| at + i);
    from..to
}

/// The line around `at` in `text`: between its line breaks.
pub fn line_around(text: &str, at: usize) -> Range<usize> {
    let at = floor(text, at.min(text.len()));
    let from = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let to = text[at..].find('\n').map_or(text.len(), |i| at + i);
    from..to
}

/// `text` as `kind` takes it: line breaks kept on lines (`\r\n` one) and a
/// space on one line, tabs a space, other control characters dropped; in
/// a secret, none of them.
fn clean(text: &str, kind: Kind) -> String {
    let text = text.replace("\r\n", "\n");
    text.chars()
        .filter_map(|c| match (c, kind) {
            ('\n' | '\r', Kind::Lines) => Some('\n'),
            ('\n' | '\r', Kind::Line) => Some(' '),
            ('\t', Kind::Line | Kind::Lines) => Some(' '),
            (c, _) if c.is_control() => None,
            (c, _) => Some(c),
        })
        .collect()
}

/// The character boundary before `at` (`at` itself at the start).
fn before(text: &str, at: usize) -> usize {
    text[..at]
        .chars()
        .next_back()
        .map_or(at, |c| at - c.len_utf8())
}

/// The character boundary after `at`.
fn after(text: &str, at: usize) -> usize {
    text[at..].chars().next().map_or(at, |c| at + c.len_utf8())
}

/// `at` moved back to a character's boundary of `text`.
pub fn floor(text: &str, mut at: usize) -> usize {
    at = at.min(text.len());
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// `text` overwritten with zeros, and emptied.
pub fn wipe(text: &mut String) {
    let mut bytes = std::mem::take(text).into_bytes();
    bytes.fill(0);
    // The zeros are read back, so that the write is not left out.
    std::hint::black_box(&bytes);
}

#[cfg(test)]
mod tests;
