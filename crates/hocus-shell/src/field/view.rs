//! A field laid out and drawn: what it shows (its text, masked for a
//! secret, an input method's preedit at the caret), in rows (on lines,
//! wrapped between words, a word too long broken where it must),
//! scrolled to keep the caret in view (down its rows on lines, across on
//! one), where a point falls in it, and drawn with its selection behind
//! the text, the preedit underlined and the caret.
//!
//! Offsets here are in what is shown ([`Laid::shown`]); the field's own
//! are mapped to and from them, so a field laid out is also the
//! [`Lines`] its caret moves along.

use std::ops::Range;

use iced::{
    Color, Font, Pixels, Point, Rectangle, Size, alignment,
    widget::{
        canvas::{self, Frame},
        text::{Alignment, LineHeight, Shaping},
    },
};

use super::{Field, Kind, Lines, Scroll};

/// How text is measured: its width at a size in a font.
pub type Measure = fn(&str, f32, Font) -> f32;

/// How a field's text is set.
#[derive(Debug, Clone, Copy)]
pub struct Style {
    pub size: f32,
    /// A row's height.
    pub line: f32,
    pub font: Font,
    /// The rows in view at most (1 on one line).
    pub rows: usize,
    pub measure: Measure,
    /// The placeholder's size and font.
    pub hint: (f32, Font),
}

/// What a secret shows for each of its characters.
const MASK: char = '•';

/// A field laid out.
#[derive(Debug, Clone)]
pub struct Laid {
    /// What it draws: its text (masked), a preedit, or its placeholder.
    pub shown: String,
    /// Its rows, as ranges of `shown`.
    pub rows: Vec<Range<usize>>,
    pub scroll: Scroll,
    /// The top left of its first row in view, and the room across.
    pub origin: Point,
    pub width: f32,
    pub style: Style,
    /// The placeholder is shown: nothing is typed.
    pub placeholder: bool,
    /// The caret, the selection, the preedit, in `shown`.
    pub caret: usize,
    pub selection: Range<usize>,
    pub preedit: Option<Range<usize>>,
    map: Map,
}

/// How the field's offsets are shown: masked (each character's
/// boundary, never what it is), and a preedit of `len` bytes at the caret
/// (`at` in what is shown, `caret` in the field).
#[derive(Debug, Clone, Default)]
struct Map {
    bounds: Option<Vec<usize>>,
    caret: usize,
    at: usize,
    len: usize,
}

impl Map {
    /// Where the field's `real` shows, before the preedit.
    fn to_shown(&self, real: usize) -> usize {
        let base = match &self.bounds {
            Some(bounds) => bounds.partition_point(|b| *b < real) * MASK.len_utf8(),
            None => real,
        };
        if real > self.caret {
            base + self.len
        } else {
            base
        }
    }

    /// The field's place shown at `shown`; in the preedit, the caret.
    fn to_real(&self, shown: usize) -> usize {
        let unshown = if shown >= self.at + self.len {
            shown - self.len
        } else if shown >= self.at {
            return self.caret;
        } else {
            shown
        };
        match &self.bounds {
            Some(bounds) => {
                let n = unshown / MASK.len_utf8();
                bounds
                    .get(n)
                    .copied()
                    .unwrap_or(bounds.last().copied().unwrap_or(0))
            }
            None => unshown,
        }
    }
}

impl Laid {
    /// `field` laid out `width` across in `style`, `placeholder` shown
    /// while it is empty; scrolled from where it was to keep the caret in
    /// view. Its place comes after ([`Laid::place`]), once its height is
    /// known.
    pub fn new(field: &Field, width: f32, style: Style, placeholder: &str) -> Self {
        let preedit = field.preedit();
        if field.is_empty() && preedit.is_none() {
            return Self {
                shown: placeholder.to_owned(),
                rows: std::iter::once(0..placeholder.len()).collect(),
                scroll: Scroll::default(),
                origin: Point::ORIGIN,
                width,
                style,
                placeholder: true,
                caret: 0,
                selection: 0..0,
                preedit: None,
                map: Map::default(),
            };
        }
        let text = field.text();
        let caret = field.caret();
        let masked = field.kind() == Kind::Secret;
        let show = |part: &str| -> String {
            if masked {
                std::iter::repeat_n(MASK, part.chars().count()).collect()
            } else {
                part.to_owned()
            }
        };
        let mut shown = show(&text[..caret]);
        let at = shown.len();
        let pre = preedit.map_or("", |p| p.text.as_str());
        shown.push_str(pre);
        shown.push_str(&show(&text[caret..]));
        let map = Map {
            bounds: masked.then(|| {
                text.char_indices()
                    .map(|(i, _)| i)
                    .chain(std::iter::once(text.len()))
                    .collect()
            }),
            caret,
            at,
            len: pre.len(),
        };
        let measure = |s: &str| (style.measure)(s, style.size, style.font);
        let rows = match field.kind() {
            Kind::Lines => wrap(&shown, width, measure),
            Kind::Secret => std::iter::once(0..shown.len()).collect(),
        };
        let shown_caret = match preedit {
            Some(p) => at + p.cursor.as_ref().map_or(p.text.len(), |c| c.start),
            None => map.to_shown(caret),
        };
        let selection = field.selection();
        let mut laid = Self {
            selection: map.to_shown(selection.start)..map.to_shown(selection.end),
            preedit: preedit.map(|p| at..at + p.text.len()),
            shown,
            rows,
            scroll: field.scroll,
            origin: Point::ORIGIN,
            width,
            style,
            placeholder: false,
            caret: shown_caret,
            map,
        };
        laid.scroll = laid.scrolled(field.scroll);
        laid
    }

    /// Where it is: its first row in view's top left.
    pub fn place(&mut self, origin: Point) {
        self.origin = origin;
    }

    /// The rows in view.
    pub fn in_view(&self) -> usize {
        self.rows.len().clamp(1, self.style.rows.max(1))
    }

    /// Its height in view.
    pub fn height(&self) -> f32 {
        self.in_view() as f32 * self.style.line
    }

    fn measure(&self, text: &str) -> f32 {
        (self.style.measure)(text, self.style.size, self.style.font)
    }

    /// The row `at` (shown) is on.
    fn row_of(&self, at: usize) -> usize {
        self.rows.iter().rposition(|r| r.start <= at).unwrap_or(0)
    }

    /// How far across its row `at` (shown) is.
    fn across(&self, at: usize) -> f32 {
        let row = &self.rows[self.row_of(at)];
        self.shown
            .get(row.start..at.clamp(row.start, row.end))
            .map_or(0.0, |t| self.measure(t))
    }

    /// The boundary in row `row` nearest `x` across (shown).
    fn at_across(&self, row: usize, x: f32) -> usize {
        let Some(range) = self.rows.get(row).cloned() else {
            return self.shown.len();
        };
        let text = &self.shown[range.clone()];
        let bounds: Vec<usize> = text
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(text.len()))
            .collect();
        let width = |k: usize| self.measure(&text[..bounds[k]]);
        // The last boundary whose prefix fits before `x`.
        let (mut lo, mut hi) = (0, bounds.len() - 1);
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if width(mid) <= x {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        let k = if lo + 1 < bounds.len() && x - width(lo) > (width(lo + 1) - width(lo)) / 2.0 {
            lo + 1
        } else {
            lo
        };
        range.start + bounds[k]
    }

    /// Scrolled from `was` so that the caret is in view.
    fn scrolled(&self, was: Scroll) -> Scroll {
        let row = self.row_of(self.caret);
        let view = self.in_view();
        let mut first = was.first.min(self.rows.len().saturating_sub(view));
        if row < first {
            first = row;
        } else if row >= first + view {
            first = row + 1 - view;
        }
        // On one line: across, the caret 2 px inside its right edge.
        let mut across = 0.0;
        if self.style.rows <= 1 {
            let x = self.across(self.caret);
            let room = (self.width - 2.0).max(0.0);
            let whole = self.measure(&self.shown);
            across = was.across;
            if x - across > room {
                across = x - room;
            }
            if x < across {
                across = x;
            }
            across = across.clamp(0.0, (whole - room).max(0.0));
        }
        Scroll { first, across }
    }

    /// The field's place nearest `p`: on the row at its height (above
    /// them, the first in view; under, the last), the nearest across.
    pub fn at(&self, p: Point) -> usize {
        if self.placeholder {
            return 0;
        }
        let dy = ((p.y - self.origin.y) / self.style.line).floor();
        let row = (self.scroll.first as f32 + dy.max(0.0)) as usize;
        let row = row.min(self.rows.len().saturating_sub(1));
        let x = p.x - self.origin.x + self.scroll.across;
        self.map.to_real(self.at_across(row, x))
    }

    /// The middle of row `row`, if it is in view.
    fn middle(&self, row: usize) -> Option<f32> {
        let n = row.checked_sub(self.scroll.first)?;
        (n < self.in_view())
            .then(|| self.origin.y + n as f32 * self.style.line + self.style.line / 2.0)
    }

    /// The rows in view: where each starts and ends across, and its
    /// middle.
    pub fn rows_in_view(&self) -> impl Iterator<Item = (f32, f32, f32)> + '_ {
        self.rows.iter().enumerate().filter_map(|(n, row)| {
            let middle = self.middle(n)?;
            let x0 = self.origin.x - self.scroll.across;
            Some((x0, middle, x0 + self.measure(&self.shown[row.clone()])))
        })
    }

    /// Where the caret is drawn, a little taller than the text.
    pub fn caret_rect(&self) -> Rectangle {
        let row = self.row_of(self.caret);
        let middle = self
            .middle(row)
            .unwrap_or(self.origin.y + self.style.line / 2.0);
        let x = self.origin.x + self.across(self.caret) - self.scroll.across;
        // Just before the placeholder's first letter.
        let x = if self.placeholder { x - 2.0 } else { x };
        let h = (self.style.size * 1.15).round();
        Rectangle::new(Point::new(x, middle - h / 2.0), Size::new(1.5, h))
    }

    /// The boxes behind `range` (shown), row by row in view, a little past
    /// a row's end where it goes on to the next.
    fn boxes(&self, range: &Range<usize>) -> Vec<Rectangle> {
        let mut out = Vec::new();
        if range.is_empty() {
            return out;
        }
        for (n, row) in self.rows.iter().enumerate() {
            let Some(middle) = self.middle(n) else {
                continue;
            };
            let from = range.start.max(row.start);
            let to = range.end.min(row.end);
            let past = range.end > row.end && range.start <= row.end;
            if from > to || (from == to && !past) {
                continue;
            }
            let x = |at: usize| {
                self.origin.x - self.scroll.across + self.measure(&self.shown[row.start..at])
            };
            let x0 = x(from);
            let mut x1 = x(to);
            if past {
                x1 += self.style.size * 0.3;
            }
            out.push(Rectangle::new(
                Point::new(x0, middle - self.style.line / 2.0),
                Size::new(x1 - x0, self.style.line),
            ));
        }
        out
    }

    /// The selection's boxes.
    pub fn highlights(&self) -> Vec<Rectangle> {
        self.boxes(&self.selection)
    }

    /// Where the input method's window goes: around the caret, in the
    /// surface.
    pub fn ime_cursor(&self) -> Rectangle {
        let caret = self.caret_rect();
        Rectangle::new(caret.position(), Size::new(1.0, caret.height))
    }

    /// The field drawn: the selection behind its rows in view, its text
    /// (the placeholder faint), the preedit underlined, and the caret while
    /// it has the keyboard (still: nothing blinks at rest).
    pub fn draw(&self, f: &mut Frame, look: &Look, focused: bool) {
        let clip = Rectangle::new(
            Point::new(self.origin.x - 3.0, self.origin.y),
            Size::new(self.width + 6.0, self.height()),
        );
        f.with_clip(clip, |f| {
            for rect in self.highlights() {
                f.fill_rectangle(rect.position(), rect.size(), look.selection);
            }
            for (n, row) in self.rows.iter().enumerate() {
                let Some(middle) = self.middle(n) else {
                    continue;
                };
                let (size, font) = if self.placeholder {
                    self.style.hint
                } else {
                    (self.style.size, self.style.font)
                };
                f.fill_text(canvas::Text {
                    content: self.shown[row.clone()].to_owned(),
                    position: Point::new(self.origin.x - self.scroll.across, middle),
                    max_width: f32::INFINITY,
                    color: if self.placeholder {
                        look.faint
                    } else {
                        look.text
                    },
                    size: Pixels(size),
                    line_height: LineHeight::Relative(1.366),
                    font,
                    align_x: Alignment::Left,
                    align_y: alignment::Vertical::Center,
                    shaping: Shaping::Advanced,
                });
            }
            if let Some(preedit) = &self.preedit {
                for rect in self.boxes(preedit) {
                    let y = rect.y + rect.height / 2.0 + self.style.size * 0.55;
                    f.fill_rectangle(Point::new(rect.x, y), Size::new(rect.width, 1.0), look.text);
                }
            }
            if focused {
                let caret = self.caret_rect();
                f.fill_rectangle(caret.position(), caret.size(), look.caret);
            }
        });
    }
}

/// A field's colours.
#[derive(Debug, Clone, Copy)]
pub struct Look {
    pub text: Color,
    pub faint: Color,
    pub caret: Color,
    pub selection: Color,
}

impl Lines for Laid {
    fn line_of(&self, at: usize) -> usize {
        self.row_of(self.map.to_shown(at))
    }

    fn line(&self, line: usize) -> Option<Range<usize>> {
        if self.placeholder {
            return (line == 0).then_some(0..0);
        }
        let row = self.rows.get(line)?;
        Some(self.map.to_real(row.start)..self.map.to_real(row.end))
    }

    fn x_of(&self, at: usize) -> f32 {
        if self.placeholder {
            return 0.0;
        }
        self.across(self.map.to_shown(at))
    }

    fn at_x(&self, line: usize, x: f32) -> usize {
        if self.placeholder {
            return 0;
        }
        self.map.to_real(self.at_across(line, x))
    }
}

/// `text` in rows that fit `room` as `width` measures them: its own lines,
/// each broken between words (the space between two rows is neither's), a
/// word too wide alone broken between its characters.
pub fn wrap(text: &str, room: f32, width: impl Fn(&str) -> f32) -> Vec<Range<usize>> {
    let mut rows = Vec::new();
    let mut start = 0;
    for paragraph in text.split('\n') {
        let end = start + paragraph.len();
        let mut row = start;
        let mut at = start;
        for word in paragraph.split(' ') {
            let word_end = at + word.len();
            if row < at && width(&text[row..word_end]) > room {
                // The space before the word ends the row.
                rows.push(row..at - 1);
                row = at;
            }
            // Too wide alone: broken where it must.
            while width(&text[row..word_end]) > room {
                let cut = text[row..word_end]
                    .char_indices()
                    .skip(1)
                    .map(|(i, _)| row + i)
                    .take_while(|i| width(&text[row..*i]) <= room)
                    .last();
                let Some(cut) = cut else {
                    // Not even a character fits: one a row.
                    let one = text[row..].chars().next().map_or(0, char::len_utf8);
                    if row + one >= word_end {
                        break;
                    }
                    rows.push(row..row + one);
                    row += one;
                    continue;
                };
                rows.push(row..cut);
                row = cut;
            }
            at = word_end + 1;
        }
        rows.push(row..end);
        start = end + 1;
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::{Act, Motion, Outcome};

    /// Each character 6 px wide, whatever its size and font.
    fn measure(text: &str, _: f32, _: Font) -> f32 {
        text.chars().count() as f32 * 6.0
    }

    fn style(rows: usize) -> Style {
        Style {
            size: 14.0,
            line: 20.0,
            font: Font::DEFAULT,
            rows,
            measure,
            hint: (14.0, Font::DEFAULT),
        }
    }

    fn lines(text: &str) -> Field {
        let mut field = Field::new(Kind::Lines, 2000);
        field.set(text);
        field
    }

    #[test]
    fn rows_break_between_words_and_a_long_word_where_it_must() {
        let six = |s: &str| s.chars().count() as f32 * 6.0;
        let text = "un deux trois\nquatre";
        let rows = wrap(text, 60.0, six);
        let shown: Vec<&str> = rows.iter().map(|r| &text[r.clone()]).collect();
        assert_eq!(shown, ["un deux", "trois", "quatre"]);
        let text = "https://exemple.fr/un/long/chemin fin";
        let rows = wrap(text, 60.0, six);
        let shown: Vec<&str> = rows.iter().map(|r| &text[r.clone()]).collect();
        assert_eq!(shown, ["https://ex", "emple.fr/u", "n/long/che", "min fin"]);
        // An empty line is a row; a space typed at a row's end stays on it.
        let rows = wrap("a\n\nb ", 60.0, six);
        assert_eq!(rows, [0..1, 2..2, 3..5]);
    }

    #[test]
    fn up_and_down_keep_their_column_across_short_rows() {
        let mut field = lines("première ligne\nab\ntroisième ligne");
        let laid = Laid::new(&field, 400.0, style(5), "");
        field.apply(
            Act::Move {
                motion: Motion::Start,
                extend: false,
            },
            &laid,
        );
        for _ in 0..6 {
            field.apply(
                Act::Move {
                    motion: Motion::Right,
                    extend: false,
                },
                &laid,
            );
        }
        let down = Act::Move {
            motion: Motion::Down,
            extend: false,
        };
        field.apply(down.clone(), &laid);
        assert_eq!(
            &field.text()[..field.caret()],
            "première ligne\nab",
            "the short row's end"
        );
        field.apply(down.clone(), &laid);
        assert_eq!(
            &field.text()[..field.caret()],
            "première ligne\nab\ntroisi",
            "back to its column"
        );
        assert_eq!(
            field.apply(down, &laid),
            Outcome::Moved,
            "past the last: the end"
        );
        assert_eq!(field.caret(), field.text().len());
    }

    #[test]
    fn home_and_end_go_to_a_wrapped_rows_ends() {
        let mut field = lines("un deux trois quatre");
        // Rows of 10 characters: « un deux », « trois », « quatre ».
        let laid = Laid::new(&field, 60.0, style(5), "");
        assert_eq!(laid.rows.len(), 3);
        let home = Act::Move {
            motion: Motion::LineStart,
            extend: true,
        };
        field.apply(home, &laid);
        assert_eq!(&field.text()[field.selection()], "quatre");
        field.apply(
            Act::Move {
                motion: Motion::Up,
                extend: false,
            },
            &laid,
        );
        field.apply(
            Act::Move {
                motion: Motion::LineEnd,
                extend: false,
            },
            &laid,
        );
        assert_eq!(&field.text()[..field.caret()], "un deux trois");
    }

    #[test]
    fn a_point_finds_its_place_and_the_rows_scroll_to_the_caret() {
        let mut field = lines("a\nb\nc\nd\ne\nf\ng");
        let mut laid = Laid::new(&field, 200.0, style(5), "");
        // The caret at the end, on the 7th row: the last five in view.
        assert_eq!(laid.scroll.first, 2);
        assert_eq!(laid.in_view(), 5);
        laid.place(Point::new(10.0, 100.0));
        // The first row in view is « c », 3 px across: before or after it.
        assert_eq!(laid.at(Point::new(11.0, 105.0)), 4);
        assert_eq!(laid.at(Point::new(15.0, 105.0)), 5);
        // Far past a row's end: its end; under the rows: the last.
        assert_eq!(laid.at(Point::new(300.0, 125.0)), 7);
        assert_eq!(laid.at(Point::new(11.0, 400.0)), 12);
        // The caret to the start: it scrolls back up.
        field.apply(
            Act::Move {
                motion: Motion::Start,
                extend: false,
            },
            &laid,
        );
        let laid = Laid::new(&field, 200.0, style(5), "");
        assert_eq!(laid.scroll.first, 0);
    }

    #[test]
    fn one_line_scrolls_across_and_a_secret_shows_masked() {
        let mut field = Field::new(Kind::Secret, 512);
        field.apply(
            Act::Insert("sk-ant-0123456789".to_owned()),
            &crate::field::Paragraphs(""),
        );
        let laid = Laid::new(&field, 60.0, style(1), "");
        assert_eq!(laid.shown, "•".repeat(17));
        // 17 dots of 6 px in 60: the end in view.
        assert_eq!(laid.scroll.across, 17.0 * 6.0 - 58.0);
        field.apply(
            Act::Move {
                motion: Motion::Start,
                extend: false,
            },
            &laid,
        );
        let laid = Laid::new(&field, 60.0, style(1), "");
        assert_eq!(laid.scroll.across, 0.0);
        // A point maps to the secret's own places.
        assert_eq!(laid.at(Point::new(13.0, 5.0)), 2);
    }

    #[test]
    fn a_preedit_shows_at_the_caret_and_points_in_it_are_the_caret() {
        let mut field = lines("le chat");
        let text = field.text().to_owned();
        field.apply(
            Act::Move {
                motion: Motion::WordLeft,
                extend: false,
            },
            &crate::field::Paragraphs(&text),
        );
        field.compose("に", None);
        let laid = Laid::new(&field, 400.0, style(5), "");
        assert_eq!(laid.shown, "le にchat");
        assert_eq!(laid.preedit, Some(3..6));
        assert_eq!(laid.caret, 6, "after the preedit");
        assert_eq!(laid.at(Point::new(19.0, 5.0)), 3, "in it: the caret");
        assert_eq!(laid.at(Point::new(34.0, 5.0)), 5, "after it: the field's");
    }

    #[test]
    fn the_placeholder_shows_while_it_is_empty() {
        let field = Field::new(Kind::Lines, 10);
        let laid = Laid::new(&field, 200.0, style(5), "Dis-lui…");
        assert!(laid.placeholder);
        assert_eq!(laid.at(Point::new(50.0, 5.0)), 0);
        assert_eq!(laid.caret_rect().x, -2.0, "just before its first letter");
        assert!(laid.highlights().is_empty());
    }

    #[test]
    fn the_selection_draws_on_each_of_its_rows() {
        let mut field = lines("un deux trois quatre");
        field.press(3, 1, false);
        field.drag(16);
        let laid = Laid::new(&field, 60.0, style(5), "");
        let boxes = laid.highlights();
        assert_eq!(boxes.len(), 3, "{boxes:?}");
        // « deux » and past its end, all of « trois » and past, « qu ».
        assert_eq!(boxes[0].x, 18.0);
        assert_eq!(boxes[2].width, 12.0);
    }
}
