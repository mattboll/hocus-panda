//! The footer's clipboard's popover (5.5), until the clipboard's history
//! (5.6) takes its place: "Presse-papiers" and what the clipboard holds
//! now, its text on one line cut with an ellipsis, or "Rien de copié."
//! when it holds no text. Nothing in it acts; `Esc` closes it.
//!
//! The shell reads the clipboard as a client does, with what the
//! compositor last offered it, which it offers to the client holding the
//! keyboard: the popover reads it as it opens, then 150 ms and 400 ms
//! later, once a click has given it the keyboard and the compositor the
//! clipboard, off the loop each time ([`crate::paste::glance`]). Opened by
//! hovering, it holds no keyboard and shows what the shell last saw. 5.6
//! replaces this module: [`opening`], [`closing`], [`height`] and [`Panel`]
//! are its seam with the controller and the shell.

use std::{
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use hocus_link::POPOVER_WIDTH;
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme, keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry},
        text::Alignment,
    },
    window,
};

use super::{
    Event as Seen, Item, Pressed, event,
    kit::{self, Input, LEFT, NOTE, Pointer, RIGHT, ROW_OUT, text, text_in},
};
use crate::levels::draw::{FAINT, MANROPE_BOLD, TEXT, rgb};

// --- The mockup's sizes, at ×1 ------------------------------------------------

pub const WIDTH: u32 = POPOVER_WIDTH.unsigned_abs();
/// The row under the header: 44 px, 4 px under it, 8 px in; its kind's
/// 28 px tile (radius 8), 10 px before the text (13 px, 600).
const ROW_TOP: f32 = kit::HEADER_TOP + kit::HEADER + 4.0;
pub(crate) const ROW: f32 = 44.0;
const ROW_PAD: f32 = 8.0;
pub(crate) const TILE: f32 = 28.0;
const TILE_RADIUS: f32 = 8.0;
const TILE_FILL: Color = Color::from_rgba8(255, 255, 255, 0.08);
const TILE_INK: Color = rgb(0xC9D0DC);
const TILE_LETTER: f32 = 14.0;
const TILE_GAP: f32 = 10.0;
pub(crate) const LINE_TEXT: f32 = 13.0;
/// What the text has: from after the tile to the row's padding.
const TEXT_ROOM: f32 = RIGHT + ROW_OUT - ROW_PAD - (LEFT - ROW_OUT + ROW_PAD + TILE + TILE_GAP);

/// When the clipboard is read again after the popover opens.
const GLANCES: [Duration; 3] = [
    Duration::ZERO,
    Duration::from_millis(150),
    Duration::from_millis(400),
];
/// How long a client has to write it, past the reader's own wait.
const ANSWER: Duration = Duration::from_secs(3);

/// The row's place.
pub(crate) fn row() -> Rectangle {
    Rectangle::new(
        Point::new(LEFT - ROW_OUT, ROW_TOP),
        Size::new(RIGHT - LEFT + 2.0 * ROW_OUT, ROW),
    )
}

/// Its height: the header and the row.
pub fn height() -> u32 {
    kit::height(ROW_TOP + ROW)
}

// --- What the clipboard holds ---------------------------------------------------

/// What the clipboard was found to hold.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum Copied {
    /// Not read yet.
    #[default]
    Reading,
    /// No text.
    Nothing,
    /// Its text, on one line.
    Text(String),
}

static COPIED: Mutex<Copied> = Mutex::new(Copied::Reading);
/// Each opening's number: a reading for an earlier one is let go.
static OPENED: AtomicU64 = AtomicU64::new(0);

fn copied() -> Copied {
    COPIED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// What a reading found makes of what was shown: a text shows at once;
/// no text shows only from the last reading (a click may still give the
/// shell the clipboard), or in place of a text.
fn after(shown: &Copied, found: Copied, last: bool) -> Copied {
    match (shown, found) {
        (_, Copied::Text(text)) => Copied::Text(text),
        (Copied::Reading, _) if !last => Copied::Reading,
        _ => Copied::Nothing,
    }
}

/// The popover opens: the clipboard is read, off the loop.
pub fn opening() {
    let round = OPENED.fetch_add(1, Ordering::SeqCst) + 1;
    *COPIED.lock().unwrap_or_else(PoisonError::into_inner) = Copied::Reading;
    let started = Instant::now();
    let glancing = std::thread::Builder::new()
        .name("clipboard".to_owned())
        .spawn(move || {
            for (n, at) in GLANCES.iter().enumerate() {
                std::thread::sleep(at.saturating_sub(started.elapsed()));
                if OPENED.load(Ordering::SeqCst) != round {
                    return;
                }
                let found = glance();
                let mut copied = COPIED.lock().unwrap_or_else(PoisonError::into_inner);
                if OPENED.load(Ordering::SeqCst) != round {
                    return;
                }
                let new = after(&copied, found, n + 1 == GLANCES.len());
                if new != *copied {
                    *copied = new;
                    drop(copied);
                    super::changed(Item::Clipboard);
                }
            }
        });
    if let Err(err) = glancing {
        tracing::warn!(%err, "cannot read the clipboard");
        *COPIED.lock().unwrap_or_else(PoisonError::into_inner) = Copied::Nothing;
    }
}

/// The popover closes: what it showed goes.
pub fn closing() {
    OPENED.fetch_add(1, Ordering::SeqCst);
    *COPIED.lock().unwrap_or_else(PoisonError::into_inner) = Copied::Reading;
}

/// The clipboard's text, read now, waiting for it on this thread.
fn glance() -> Copied {
    let (sender, answer) = mpsc::channel();
    let asked = crate::paste::glance(move |text| {
        let _ = sender.send(text.map(str::to_owned));
    });
    if !asked {
        return Copied::Nothing;
    }
    match answer.recv_timeout(ANSWER) {
        Ok(Some(text)) if !text.is_empty() => Copied::Text(text),
        _ => Copied::Nothing,
    }
}

// --- Words ------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    fr: bool,
}

impl Words {
    pub fn new(lang: crate::clock::time::Lang) -> Self {
        Self {
            fr: lang == crate::clock::time::Lang::French,
        }
    }

    pub fn title(self) -> &'static str {
        if self.fr {
            "Presse-papiers"
        } else {
            "Clipboard"
        }
    }

    fn nothing(self) -> &'static str {
        if self.fr {
            "Rien de copié."
        } else {
            "Nothing copied."
        }
    }
}

// --- The panel --------------------------------------------------------------------

/// The popover on its surface.
#[derive(Debug, Clone)]
pub struct Panel {
    /// Its surface: it reacts only while open, not while it fades out.
    pub id: window::Id,
    pub words: Words,
    /// On the compositor's glass (else opaque).
    pub glass: bool,
}

/// What it shows: the text as cut to its row.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Shown {
    Reading,
    Nothing,
    Text(String),
}

/// What the surface keeps between draws.
#[derive(Debug, Default)]
pub struct State {
    shown: Option<Shown>,
    pointer: Pointer<()>,
}

impl<Message> canvas::Program<Message> for Panel {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        match kit::input(self.id, event, bounds, cursor, &mut state.pointer) {
            Input::Redraw(_) => {
                for pressed in kit::keys(self.id) {
                    let _: Option<canvas::Action<Message>> = Self::key(&pressed);
                }
                self.refresh(state);
                None
            }
            Input::Mouse(seen, _) => {
                match seen {
                    mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                        state.pointer.moved();
                    }
                    mouse::Event::CursorLeft => state.pointer.left(),
                    _ => {}
                }
                None
            }
            Input::Key(pressed) => Self::key(&pressed),
            Input::Nothing => None,
        }
    }

    fn draw(
        &self,
        state: &State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        kit::background(&mut frame, bounds.size(), self.glass);
        let f = &mut frame;
        kit::title(f, self.words.title());
        let row = row();
        match &state.shown {
            Some(Shown::Text(line)) => {
                let tile = tile(f, row);
                f.fill_text(text_in(
                    kit::MANROPE_SEMIBOLD,
                    text(
                        line.clone(),
                        Point::new(tile.x + TILE + TILE_GAP, row.center_y()),
                        LINE_TEXT,
                        TEXT,
                        Alignment::Left,
                    ),
                ));
            }
            Some(Shown::Nothing) => {
                f.fill_text(text(
                    self.words.nothing().to_owned(),
                    Point::new(LEFT, row.center_y()),
                    NOTE,
                    FAINT,
                    Alignment::Left,
                ));
            }
            Some(Shown::Reading) | None => {}
        }
        vec![frame.into_geometry()]
    }
}

/// The text's kind's tile in `row`, "T": where it is.
fn tile(f: &mut Frame, row: Rectangle) -> Rectangle {
    let tile = Rectangle::new(
        Point::new(row.x + ROW_PAD, row.center_y() - TILE / 2.0),
        Size::new(TILE, TILE),
    );
    f.fill(&kit::rounded(tile, TILE_RADIUS), TILE_FILL);
    f.fill_text(text_in(
        MANROPE_BOLD,
        text(
            "T".to_owned(),
            tile.center(),
            TILE_LETTER,
            TILE_INK,
            Alignment::Center,
        ),
    ));
    tile
}

impl Panel {
    /// What it shows, read afresh while it is open (closing, it shows
    /// what it showed).
    fn refresh(&self, state: &mut State) {
        if !super::with(|c| c.is_open(self.id)) {
            return;
        }
        let shown = match copied() {
            Copied::Reading => Shown::Reading,
            Copied::Nothing => Shown::Nothing,
            Copied::Text(text) => Shown::Text(
                match iced::advanced::graphics::text::font_system().write() {
                    Ok(mut system) => kit::elide(&text, TEXT_ROOM, |t| {
                        kit::measure_in(system.raw(), t, LINE_TEXT, kit::MANROPE_SEMIBOLD)
                    }),
                    Err(_) => text,
                },
            ),
        };
        if state.shown.as_ref() == Some(&shown) {
            return;
        }
        // What it holds may be a password: said only whether it is text.
        tracing::debug!(
            title = self.words.title(),
            copied = match &shown {
                Shown::Reading => "reading",
                Shown::Nothing => "nothing",
                Shown::Text(_) => "text",
            },
            "the clipboard's popover shows it"
        );
        state.shown = Some(shown);
    }

    fn key<Message>(pressed: &Pressed) -> Option<canvas::Action<Message>> {
        match &pressed.key {
            keyboard::Key::Named(keyboard::key::Named::Escape) => {
                event(Seen::Escape);
                Some(canvas::Action::capture())
            }
            // Nothing in it holds the keyboard.
            keyboard::Key::Named(keyboard::key::Named::Tab) => Some(canvas::Action::capture()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_shows_at_once_and_no_text_only_from_the_last_reading() {
        let text = || Copied::Text("git rebase".to_owned());
        // The shell may not have the clipboard yet: no "nothing copied"
        // before the last reading.
        assert_eq!(
            after(&Copied::Reading, Copied::Nothing, false),
            Copied::Reading
        );
        assert_eq!(after(&Copied::Reading, text(), false), text());
        assert_eq!(
            after(&Copied::Reading, Copied::Nothing, true),
            Copied::Nothing
        );
        // A text read again stays; one gone says so.
        assert_eq!(after(&text(), text(), false), text());
        assert_eq!(after(&text(), Copied::Nothing, false), Copied::Nothing);
    }

    #[test]
    fn its_row_fits_under_its_header() {
        // The header (13 + 32), 4 px, the 44 px row, 14 px and the rim.
        assert_eq!(height(), 13 + 32 + 4 + 44 + 14 + 1);
        assert_eq!(
            TEXT_ROOM,
            280.0 - 2.0 * 15.0 + 2.0 * 6.0 - 2.0 * 8.0 - 28.0 - 10.0
        );
    }
}
