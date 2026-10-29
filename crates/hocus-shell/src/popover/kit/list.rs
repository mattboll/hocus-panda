//! Lists of rows: their places, their scrolling, what a row says on its
//! right (and its spinner), their text cut to the list on the CPU.

use std::time::{Duration, Instant};

use iced::{
    Color, Point, Radians, Rectangle, Size,
    widget::{
        canvas::{self, Frame, Path},
        text::Alignment,
    },
};

use super::{ICON, ICON_GAP, LEFT, RIGHT, ROW, ROW_GAP, ROW_OUT, ROW_PAD, ROW_RADIUS, SMALL, text};
use crate::levels::draw::{DIM, FAINT, HOVER, LINE, stroke};

/// A list's scrollbar, on the content's right.
pub(crate) const SCROLLBAR: f32 = 6.0;
const THUMB: Color = Color::from_rgba8(255, 255, 255, 0.16);
/// A row lit under the pointer (or unfolded).
const ROW_HOVER: Color = Color::from_rgba8(255, 255, 255, 0.05);
/// The spinners: 12 px, a turn in 900 ms.
const SPINNER: f32 = 12.0;
const TURN: Duration = Duration::from_millis(900);

/// Rows one under the other, [`ROW_GAP`] apart, `heights` tall: each one's
/// top and height, and all their height.
pub(crate) fn stack(heights: impl IntoIterator<Item = f32>) -> (Vec<(f32, f32)>, f32) {
    let mut rows = Vec::new();
    let mut content = 0.0;
    for height in heights {
        rows.push((content, height));
        content += height + ROW_GAP;
    }
    (rows, (content - ROW_GAP).max(0.0))
}

/// A list shown from `top`, `height` tall, its content `content` tall: as
/// wide as its rows, which reach [`ROW_OUT`] past the content's sides.
pub(crate) fn list(top: f32, height: f32, content: f32) -> List {
    List {
        view: Rectangle::new(
            Point::new(LEFT - ROW_OUT, top),
            Size::new(RIGHT - LEFT + 2.0 * ROW_OUT, height),
        ),
        content,
    }
}

/// A list on the popover: its viewport, and its content's height, which
/// scrolls past the viewport's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct List {
    pub view: Rectangle,
    pub content: f32,
}

impl List {
    /// Whether it scrolls.
    pub(crate) fn scrolls(self) -> bool {
        self.content > self.view.height + 0.5
    }

    /// How far it can scroll.
    pub(crate) fn most_scroll(self) -> f32 {
        (self.content - self.view.height).max(0.0)
    }

    /// The row whose top is `top` in the content, its [`ROW`] px, on the
    /// popover, scrolled by `scroll`: to the scrollbar when the list
    /// scrolls, else to the list's right less `end`.
    pub(crate) fn row(self, top: f32, scroll: f32, end: f32) -> Rectangle {
        let view = self.view;
        let right = if self.scrolls() {
            RIGHT - 2.0
        } else {
            view.x + view.width - end
        };
        Rectangle::new(
            Point::new(view.x, view.y + top - scroll),
            Size::new(right - view.x, ROW),
        )
    }

    /// Its thumb, at `scroll`, when it scrolls.
    pub(crate) fn scrollbar(self, f: &mut Frame, scroll: f32) {
        if !self.scrolls() {
            return;
        }
        let view = self.view;
        let height = (view.height * view.height / self.content).max(20.0);
        let travel = view.height - height;
        let share = scroll / self.most_scroll().max(1.0);
        let thumb = Rectangle::new(
            Point::new(RIGHT, view.y + travel * share),
            Size::new(SCROLLBAR, height),
        );
        f.fill(
            &Path::rounded_rectangle(thumb.position(), thumb.size(), (SCROLLBAR / 2.0).into()),
            THUMB,
        );
    }
}

/// The scroll that shows the row at `top`, `height` tall, in a viewport
/// `view` tall scrolled by `scroll`: as little as can be, and `most` at
/// most.
pub(crate) fn reveal((top, height): (f32, f32), view: f32, scroll: f32, most: f32) -> f32 {
    let mut scroll = scroll;
    if top < scroll {
        scroll = top;
    } else if top + height > scroll + view {
        scroll = top + height - view;
    }
    scroll.clamp(0.0, most)
}

/// A list's row lit: the chosen one, or the one under the pointer.
pub(crate) fn row_fill(f: &mut Frame, rect: Rectangle, chosen: bool, hovered: bool) {
    let fill = if chosen {
        Some(HOVER)
    } else if hovered {
        Some(ROW_HOVER)
    } else {
        None
    };
    if let Some(fill) = fill {
        f.fill(
            &Path::rounded_rectangle(rect.position(), rect.size(), ROW_RADIUS.into()),
            fill,
        );
    }
}

/// Where a list's text shows: on the CPU, whose renderer does not cut text
/// to a frame's bounds, a text in a list is drawn only when it shows whole.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Clip(Option<Rectangle>);

impl Clip {
    /// Text in the list `view`, cut on the CPU (`software`).
    pub(crate) fn new(software: bool, view: Option<Rectangle>) -> Self {
        Self(view.filter(|_| software))
    }

    pub(crate) fn fill(self, f: &mut Frame, text: canvas::Text) {
        if let Some(view) = self.0 {
            let half = text.size.0 * LINE / 2.0;
            if text.position.y - half < view.y - 1.0
                || text.position.y + half > view.y + view.height + 1.0
            {
                return;
            }
        }
        f.fill_text(text);
    }
}

/// What a row says on its right: "Connexion…", "enregistré", "dans les
/// Réglages"…
#[derive(Debug, Clone, Copy)]
pub(crate) struct Aside {
    pub text: &'static str,
    pub colour: Color,
    /// With a spinner before it.
    pub spinner: bool,
}

/// `aside`, `width` wide, right-aligned on `right` at `middle`, and its
/// spinner before it at `turn` (none without motion): where it begins.
pub(crate) fn aside(
    f: &mut Frame,
    clip: Clip,
    aside: Aside,
    (right, middle): (f32, f32),
    width: f32,
    turn: Option<f32>,
) -> f32 {
    clip.fill(
        f,
        text(
            aside.text.to_owned(),
            Point::new(right, middle),
            SMALL,
            aside.colour,
            Alignment::Right,
        ),
    );
    let mut right = right - width;
    if aside.spinner
        && let Some(turn) = turn
    {
        right -= 6.0 + SPINNER;
        spinner(f, Point::new(right, middle - SPINNER / 2.0), DIM, turn);
    }
    right
}

/// "Recherche…" on a label's line centred on `middle`, right-aligned,
/// `width` wide, and its spinner before it at `turn` (none without motion).
pub(crate) fn searching(f: &mut Frame, word: &str, width: f32, middle: f32, turn: Option<f32>) {
    f.fill_text(text(
        word.to_owned(),
        Point::new(RIGHT, middle),
        SMALL,
        FAINT,
        Alignment::Right,
    ));
    if let Some(turn) = turn {
        let at = RIGHT - width - 6.0 - SPINNER;
        spinner(f, Point::new(at, middle - SPINNER / 2.0), FAINT, turn);
    }
}

/// The room a row's name has in a row `width` wide: past its icon, before
/// what is on its right, `right` px (its lock and its gap), and its aside,
/// `aside` px wide, with its spinner (`spins`).
pub(crate) fn name_room(width: f32, right: f32, aside: Option<(f32, bool)>) -> f32 {
    let mut room = width - 2.0 * ROW_PAD - ICON - ICON_GAP - right;
    if let Some((aside, spins)) = aside {
        room -= aside + ICON_GAP;
        if spins {
            room -= 6.0 + SPINNER;
        }
    }
    room
}

/// The spinners' turn, from when they began.
#[derive(Debug, Default)]
pub(crate) struct Spin {
    since: Option<Instant>,
}

impl Spin {
    /// Whether spinners show (`on`) at `now`: they turn from when they
    /// began.
    pub(crate) fn run(&mut self, on: bool, now: Instant) -> bool {
        if on {
            self.since.get_or_insert(now);
        } else {
            self.since = None;
        }
        on
    }

    /// Their turn at `now`, radians.
    pub(crate) fn turn(&self, now: Instant) -> f32 {
        let since = self.since.unwrap_or(now);
        let t = now.saturating_duration_since(since).as_secs_f32() / TURN.as_secs_f32();
        t.fract() * std::f32::consts::TAU
    }
}

/// The spinner: `M21 12a9 9 0 1 1-6.2-8.6`, three quarters and more of a
/// circle, turned by `turn`.
fn spinner(f: &mut Frame, at: Point, colour: Color, turn: f32) {
    let s = SPINNER / 24.0;
    let arc = Path::new(|b| {
        b.arc(canvas::path::Arc {
            center: Point::new(at.x + 12.0 * s, at.y + 12.0 * s),
            radius: 9.0 * s,
            start_angle: Radians(turn),
            end_angle: Radians(turn + 288.0_f32.to_radians()),
        });
    });
    f.stroke(&arc, stroke(3.0 * s, colour));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_scrolls_to_show_a_row_as_little_as_can_be() {
        let (rows, content) = stack([36.0, 120.0, 36.0, 36.0]);
        assert_eq!(rows[1], (38.0, 120.0));
        assert_eq!(content, 36.0 + 120.0 + 36.0 + 36.0 + 3.0 * ROW_GAP);
        let view = list(100.0, 152.0, content);
        assert!(view.scrolls());
        assert_eq!(view.most_scroll(), content - 152.0);
        // Below: its bottom on the view's; above: its top on the view's.
        assert_eq!(reveal(rows[3], 152.0, 0.0, view.most_scroll()), 82.0);
        assert_eq!(reveal(rows[1], 152.0, 82.0, view.most_scroll()), 38.0);
        // Shown already: as it is.
        assert_eq!(reveal(rows[0], 152.0, 0.0, view.most_scroll()), 0.0);
        assert_eq!(stack([]), (Vec::new(), 0.0));
    }
}
