//! Her text selected and copied (#757, #761): what the drawer shows of
//! her answers, the user's sentences, questions, cards and the queue's
//! lines is laid out as blocks of text ([`Texts`]), each with its lines
//! placed ([`Run`]), so that a point finds a place in a text ([`Spot`])
//! and a selection between two places gives its text back, as in any
//! chat: there is no button to copy.
//!
//! A block is named by what it shows ([`Source`]), not by where: the
//! drawer lays itself out again at each change (the pointer on a button,
//! a word streamed), and the selection holds across. A place is a byte
//! offset in its block's text, on a character's boundary.
//!
//! A block's text is what it shows: its lines as [`wrap`] breaks them,
//! joined by the space the break took, its paragraphs by a line break; a
//! masked secret is the words that mask it; her markdown's emphasis is
//! its words ([`unmarked`]). Blocks copy one per line.

use std::ops::Range;

use iced::{Font, Point, Rectangle, Size};

use crate::popover::kit::wrap;

/// What a block of text shows, kept from one layout to the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    /// The thread's item of this id.
    Item(u64),
    /// Her answer to this request as it streams.
    Streaming(u64),
    /// A view's paragraph of this place among those it lays out (the
    /// queue's, a briefing unfolded, "Ce qu'elle sait", #761).
    Line(u32),
    /// Thread item `0`'s paragraph `1` of those it draws (her briefing's
    /// card, #761).
    Part(u64, u32),
}

/// A block: what it shows, its text, its box, and the thread's item the
/// right click's « Copier » copies whole when nothing is selected (her
/// answer, a draft), if it is one.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub source: Source,
    pub text: String,
    pub rect: Rectangle,
    /// Whole in view: only then does it take the pointer.
    pub shown: bool,
    pub item: Option<u64>,
}

/// A line of a block, placed: its range in the block's text, where it
/// starts and its middle, its height, its size and font, its width.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub block: usize,
    pub range: Range<usize>,
    pub x: f32,
    pub middle: f32,
    pub height: f32,
    pub size: f32,
    pub font: Font,
    pub width: f32,
}

impl Run {
    fn top(&self) -> f32 {
        self.middle - self.height / 2.0
    }

    fn bottom(&self) -> f32 {
        self.middle + self.height / 2.0
    }
}

/// How text is measured: its width at a size in a font.
pub type Measure<'a> = &'a dyn Fn(&str, f32, Font) -> f32;

/// The selectable text of a layout.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Texts {
    pub blocks: Vec<Block>,
    pub runs: Vec<Run>,
}

/// A place in a block's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spot {
    pub source: Source,
    pub at: usize,
}

/// Whether the pointer still holds the selection (its press dragged) or
/// lets it be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hold {
    Dragging,
    Done,
}

/// What is selected: from where the press began to where the pointer is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Spot,
    pub focus: Spot,
    pub hold: Hold,
}

/// `text` broken into lines that fit `room` as [`wrap`] breaks them, each
/// with its range in `text`: a paragraph's lines joined by one space are
/// the paragraph.
pub fn wrap_ranged(
    text: &str,
    room: f32,
    mut width: impl FnMut(&str) -> f32,
) -> Vec<(String, Range<usize>)> {
    let mut out = Vec::new();
    let mut start = 0;
    for paragraph in text.split('\n') {
        let mut at = start;
        for line in wrap(paragraph, room, &mut width) {
            let end = at + line.len();
            out.push((line, at..end));
            at = end + 1;
        }
        start += paragraph.len() + 1;
    }
    out
}

impl Texts {
    /// A new block for `source`; its box comes with its lines.
    pub fn block(&mut self, source: Source) -> usize {
        self.blocks.push(Block {
            source,
            text: String::new(),
            rect: Rectangle::new(Point::ORIGIN, Size::ZERO),
            shown: true,
            item: None,
        });
        self.blocks.len() - 1
    }

    /// `text`'s `lines` (from [`wrap_ranged`]) placed from `x`, the first
    /// one's top at `top`, each `height` high: they end block `block`'s
    /// text, after a line break.
    #[allow(clippy::too_many_arguments)]
    pub fn put(
        &mut self,
        block: usize,
        text: &str,
        lines: &[(String, Range<usize>)],
        x: f32,
        top: f32,
        height: f32,
        size: f32,
        font: Font,
        measure: Measure<'_>,
    ) {
        let Some(b) = self.blocks.get_mut(block) else {
            return;
        };
        if !b.text.is_empty() {
            b.text.push('\n');
        }
        let base = b.text.len();
        b.text.push_str(text);
        for (n, (line, range)) in lines.iter().enumerate() {
            self.runs.push(Run {
                block,
                range: base + range.start..base + range.end,
                x,
                middle: top + n as f32 * height + height / 2.0,
                height,
                size,
                font,
                width: measure(line, size, font),
            });
        }
    }

    /// A line drawn of `text` from `x` around `middle`, `width` wide,
    /// ending block `block`'s text after `sep` (nothing on the same row,
    /// a space where a wrap broke a paragraph, a line break between
    /// lines); placed for the pointer only if `shown`.
    #[allow(clippy::too_many_arguments)]
    pub fn line(
        &mut self,
        block: usize,
        sep: &str,
        text: &str,
        x: f32,
        middle: f32,
        height: f32,
        size: f32,
        font: Font,
        width: f32,
        shown: bool,
    ) {
        let Some(b) = self.blocks.get_mut(block) else {
            return;
        };
        if !b.text.is_empty() {
            b.text.push_str(sep);
        }
        let start = b.text.len();
        b.text.push_str(text);
        if shown {
            self.runs.push(Run {
                block,
                range: start..b.text.len(),
                x,
                middle,
                height,
                size,
                font,
                width,
            });
        }
    }

    /// The block showing `source`, by its place.
    pub fn index(&self, source: Source) -> Option<usize> {
        self.blocks.iter().position(|b| b.source == source)
    }

    /// The place in the text nearest `p`: on the line at its height, the
    /// nearest of them across; between lines, the start of the next one;
    /// under them all, the end of the last.
    pub fn spot_at(&self, p: Point, measure: Measure<'_>) -> Option<Spot> {
        let across = |r: &Run| {
            if p.x < r.x {
                r.x - p.x
            } else {
                (p.x - r.x - r.width).max(0.0)
            }
        };
        let on_row = self
            .runs
            .iter()
            .filter(|r| p.y >= r.top() && p.y < r.bottom())
            .min_by(|a, b| across(a).total_cmp(&across(b)));
        let (run, at) = match on_row {
            Some(run) => (run, self.offset_in(run, p.x, measure)),
            None => match self
                .runs
                .iter()
                .filter(|r| r.top() >= p.y)
                .min_by(|a, b| a.top().total_cmp(&b.top()))
            {
                Some(next) => (next, next.range.start),
                None => {
                    let last = self
                        .runs
                        .iter()
                        .max_by(|a, b| a.bottom().total_cmp(&b.bottom()))?;
                    (last, last.range.end)
                }
            },
        };
        Some(Spot {
            source: self.blocks.get(run.block)?.source,
            at,
        })
    }

    /// The boundary between characters of `run` nearest `x`.
    fn offset_in(&self, run: &Run, x: f32, measure: Measure<'_>) -> usize {
        let Some(text) = self
            .blocks
            .get(run.block)
            .and_then(|b| b.text.get(run.range.clone()))
        else {
            return run.range.start;
        };
        let dx = x - run.x;
        if dx <= 0.0 {
            return run.range.start;
        }
        if dx >= run.width {
            return run.range.end;
        }
        let bounds: Vec<usize> = text
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(text.len()))
            .collect();
        let width = |k: usize| measure(&text[..bounds[k]], run.size, run.font);
        // The last boundary whose prefix fits before `dx`.
        let (mut lo, mut hi) = (0, bounds.len() - 1);
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if width(mid) <= dx {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        let here = width(lo);
        let k = if lo + 1 < bounds.len() {
            let next = width(lo + 1);
            if dx - here > (next - here) / 2.0 {
                lo + 1
            } else {
                lo
            }
        } else {
            lo
        };
        run.range.start + bounds[k]
    }

    /// The word around `spot`, or the character there if it is no word's:
    /// as a field takes one ([`crate::field::word_around`], #763).
    pub fn word_at(&self, spot: Spot) -> Option<(Spot, Spot)> {
        let text = &self.blocks.get(self.index(spot.source)?)?.text;
        let range = crate::field::word_around(text, spot.at);
        (!range.is_empty()).then_some(self.spots(spot.source, range))
    }

    /// The paragraph around `spot`: between its block's line breaks.
    pub fn paragraph_at(&self, spot: Spot) -> Option<(Spot, Spot)> {
        let text = &self.blocks.get(self.index(spot.source)?)?.text;
        Some(self.spots(spot.source, crate::field::line_around(text, spot.at)))
    }

    /// `range` of `source`'s text, as its two ends.
    fn spots(&self, source: Source, range: Range<usize>) -> (Spot, Spot) {
        (
            Spot {
                source,
                at: range.start,
            },
            Spot {
                source,
                at: range.end,
            },
        )
    }

    /// The selection's ends in reading order, as (block, offset); none if
    /// one of its blocks is not shown any more.
    fn ends(&self, selection: &Selection) -> Option<((usize, usize), (usize, usize))> {
        let place = |s: Spot| -> Option<(usize, usize)> {
            let block = self.index(s.source)?;
            let text = &self.blocks[block].text;
            Some((block, crate::field::floor(text, s.at)))
        };
        let (a, b) = (place(selection.anchor)?, place(selection.focus)?);
        Some(if a <= b { (a, b) } else { (b, a) })
    }

    /// The selected range of each block it covers.
    fn ranges(&self, selection: &Selection) -> Vec<(usize, Range<usize>)> {
        let Some((start, end)) = self.ends(selection) else {
            return Vec::new();
        };
        (start.0..=end.0)
            .map(|block| {
                let from = if block == start.0 { start.1 } else { 0 };
                let to = if block == end.0 {
                    end.1
                } else {
                    self.blocks[block].text.len()
                };
                (block, from..to.max(from))
            })
            .collect()
    }

    /// What `selection` selects, a line between blocks; empty if nothing.
    pub fn selected(&self, selection: &Selection) -> String {
        self.ranges(selection)
            .into_iter()
            .filter(|(_, r)| !r.is_empty())
            .filter_map(|(block, r)| self.blocks[block].text.get(r))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The boxes `selection` draws behind its text: each line's selected
    /// part, a little past it when the selection goes on past its end.
    pub fn highlights(&self, selection: &Selection, measure: Measure<'_>) -> Vec<Rectangle> {
        let ranges = self.ranges(selection);
        let mut out = Vec::new();
        for run in &self.runs {
            let Some((_, r)) = ranges.iter().find(|(b, _)| *b == run.block) else {
                continue;
            };
            let from = r.start.max(run.range.start);
            let to = r.end.min(run.range.end);
            let past = r.end > run.range.end && r.start <= run.range.end;
            if from > to || (from == to && !past) {
                continue;
            }
            let text = &self.blocks[run.block].text;
            let width = |end: usize| {
                text.get(run.range.start..end)
                    .map_or(0.0, |t| measure(t, run.size, run.font))
            };
            let x0 = run.x + width(from);
            let mut x1 = run.x + width(to);
            if past {
                x1 += run.size * 0.3;
            }
            out.push(Rectangle::new(
                Point::new(x0, run.top()),
                Size::new(x1 - x0, run.height),
            ));
        }
        out
    }

    /// The shown block whose box holds `p`.
    pub fn block_at(&self, p: Point) -> Option<&Block> {
        self.blocks.iter().find(|b| b.shown && b.rect.contains(p))
    }

    /// Whether `p` is on a line of text.
    pub fn on_text(&self, p: Point) -> bool {
        self.runs.iter().any(|r| {
            p.y >= r.top() && p.y < r.bottom() && p.x >= r.x - 2.0 && p.x <= r.x + r.width + 2.0
        })
    }
}

impl Selection {
    /// Whether it selects nothing.
    pub fn is_empty(&self) -> bool {
        self.anchor == self.focus
    }
}

/// `text` as her drawer shows it and copies it: her markdown's emphasis
/// (`**word**`, `*word*`, `__word__`, `_word_`, and three of them) is its
/// words, without the marks. A mark counts only as markdown counts it: it
/// opens before a word (not before a space, so a list's `* ` stays) and
/// closes after one, on the same line, and an underscore never inside a
/// word (`file_name` stays).
pub fn unmarked(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains(['*', '_']) {
        return std::borrow::Cow::Borrowed(text);
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    strip(&chars, &mut out);
    std::borrow::Cow::Owned(out)
}

/// [`unmarked`] on `chars`, into `out`.
fn strip(chars: &[char], out: &mut String) {
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c != '*' && c != '_' {
            out.push(c);
            i += 1;
            continue;
        }
        let run = chars[i..].iter().take_while(|&&d| d == c).count();
        let before = i.checked_sub(1).map(|k| chars[k]);
        let opens = run <= 3
            && chars.get(i + run).is_some_and(|n| !n.is_whitespace())
            && before.is_none_or(|b| !b.is_alphanumeric());
        if opens && let Some(end) = closing(chars, i + run, c, run) {
            strip(&chars[i + run..end], out);
            i = end + run;
            continue;
        }
        out.extend(&chars[i..i + run]);
        i += run;
    }
}

/// Where the run of `run` marks `c` closing what opened before `from`
/// starts: after a word, before no letter, on the same line.
fn closing(chars: &[char], from: usize, c: char, run: usize) -> Option<usize> {
    let mut k = from;
    while k < chars.len() {
        match chars[k] {
            '\n' => return None,
            d if d == c => {
                let len = chars[k..].iter().take_while(|&&d| d == c).count();
                let after_word = k > from && !chars[k - 1].is_whitespace();
                let ends = chars.get(k + len).is_none_or(|n| !n.is_alphanumeric());
                if len == run && after_word && ends {
                    return Some(k);
                }
                k += len;
            }
            _ => k += 1,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each character 6 px wide, whatever its size and font.
    fn measure(text: &str, _: f32, _: Font) -> f32 {
        text.chars().count() as f32 * 6.0
    }

    /// Two blocks: "Bien noté, je range ta journée." on two lines, then
    /// "mdp : [secret masqué — voir x]" under it.
    fn two() -> Texts {
        let mut texts = Texts::default();
        let a = texts.block(Source::Item(1));
        let text = "Bien noté, je range ta journée.\nÀ demain";
        let lines = wrap_ranged(text, 120.0, |s| measure(s, 0.0, Font::DEFAULT));
        texts.put(
            a,
            text,
            &lines,
            10.0,
            0.0,
            20.0,
            13.0,
            Font::DEFAULT,
            &measure,
        );
        let b = texts.block(Source::Item(2));
        let text = "mdp : [secret masqué — voir x]";
        let lines = wrap_ranged(text, 400.0, |s| measure(s, 0.0, Font::DEFAULT));
        texts.put(
            b,
            text,
            &lines,
            10.0,
            100.0,
            20.0,
            13.0,
            Font::DEFAULT,
            &measure,
        );
        texts
    }

    #[test]
    fn wrapped_lines_keep_their_ranges() {
        let text = "un deux trois\nquatre cinq";
        let lines = wrap_ranged(text, 60.0, |s| measure(s, 0.0, Font::DEFAULT));
        for (line, range) in &lines {
            assert_eq!(&text[range.clone()], line);
        }
        assert_eq!(lines.len(), 4, "{lines:?}");
    }

    #[test]
    fn a_drag_selects_across_lines_and_blocks() -> Result<(), &'static str> {
        let texts = two();
        let anchor = texts
            .spot_at(Point::new(10.0 + 5.0 * 6.0, 10.0), &measure)
            .ok_or("nowhere")?;
        assert_eq!(anchor.at, 5, "before « noté »");
        // Past the end of the second block.
        let focus = texts
            .spot_at(Point::new(600.0, 110.0), &measure)
            .ok_or("nowhere")?;
        let selection = Selection {
            anchor: focus,
            focus: anchor,
            hold: Hold::Done,
        };
        assert_eq!(
            texts.selected(&selection),
            "noté, je range ta journée.\nÀ demain\nmdp : [secret masqué — voir x]",
            "the line breaks the wrap took are spaces again; the mask copies as shown"
        );
        assert!(!texts.highlights(&selection, &measure).is_empty());
        Ok(())
    }

    #[test]
    fn between_lines_a_point_takes_the_next_start_and_under_all_the_end() -> Result<(), &'static str>
    {
        let texts = two();
        let gap = texts
            .spot_at(Point::new(50.0, 80.0), &measure)
            .ok_or("nowhere")?;
        assert_eq!(
            gap,
            Spot {
                source: Source::Item(2),
                at: 0
            }
        );
        let under = texts
            .spot_at(Point::new(0.0, 500.0), &measure)
            .ok_or("nowhere")?;
        assert_eq!(under.source, Source::Item(2));
        assert_eq!(under.at, texts.blocks[1].text.len());
        Ok(())
    }

    #[test]
    fn a_double_click_takes_a_word_and_a_triple_its_paragraph() -> Result<(), &'static str> {
        let texts = two();
        let spot = Spot {
            source: Source::Item(1),
            at: 7,
        };
        let (a, b) = texts.word_at(spot).ok_or("nowhere")?;
        let text = &texts.blocks[0].text;
        assert_eq!(&text[a.at..b.at], "noté");
        // Just past a word's end: still that word.
        let (a, b) = texts
            .word_at(Spot {
                source: Source::Item(1),
                at: 4,
            })
            .ok_or("nowhere")?;
        assert_eq!(&text[a.at..b.at], "Bien");
        let (a, b) = texts.paragraph_at(spot).ok_or("nowhere")?;
        assert_eq!(&text[a.at..b.at], "Bien noté, je range ta journée.");
        Ok(())
    }

    #[test]
    fn her_emphasis_shows_and_copies_without_its_marks() {
        for (marked, plain) in [
            ("Voici **le plan** :", "Voici le plan :"),
            ("relire *l'article*, puis", "relire l'article, puis"),
            (
                "un ***tout*** et __gras__ et _penché_.",
                "un tout et gras et penché.",
            ),
            ("**gras avec *penché* dedans**", "gras avec penché dedans"),
            // Not emphasis: a list's mark, a product, a name, unclosed.
            ("* un point\n* un autre", "* un point\n* un autre"),
            ("2 * 3 * 4", "2 * 3 * 4"),
            (
                "le fichier mon_fichier_long.txt",
                "le fichier mon_fichier_long.txt",
            ),
            ("**pas fermé\nici**", "**pas fermé\nici**"),
            ("rien", "rien"),
        ] {
            assert_eq!(unmarked(marked), plain, "{marked:?}");
        }
    }

    #[test]
    fn nothing_selected_draws_nothing() {
        let texts = two();
        let spot = Spot {
            source: Source::Item(1),
            at: 3,
        };
        let selection = Selection {
            anchor: spot,
            focus: spot,
            hold: Hold::Dragging,
        };
        assert!(selection.is_empty());
        assert_eq!(texts.selected(&selection), "");
        assert!(texts.highlights(&selection, &measure).is_empty());
        // A block gone: nothing either.
        let gone = Selection {
            anchor: Spot {
                source: Source::Item(9),
                at: 0,
            },
            focus: spot,
            hold: Hold::Done,
        };
        assert_eq!(texts.selected(&gone), "");
    }
}
