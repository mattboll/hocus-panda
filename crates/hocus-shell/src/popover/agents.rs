//! The agents' group's popovers (bar.md §9; the approved 4.0 mockup),
//! 300 px wide, their top 62 px under the bar's (the mockup's, whichever
//! item opened them):
//!
//! - **Sources** ("Sources", "Un autre outil ?"): "Sources des agents";
//!   a row per tool (its mark, its name over its state, "Brancher" or
//!   "Débrancher"); the divider, "UN AUTRE OUTIL", the generic command in
//!   a box with "Copier", and the states it takes.
//! - **Consent** (a row's "Brancher"): "Brancher <outil>", what it lets
//!   the desktop know, the file it changes and that a copy is kept;
//!   "Annuler" and "Brancher". Nothing is written before that click.
//!
//! `Tab` goes through their buttons, `Enter` or `Space` acts, `Esc`
//! closes. "Brancher" in Sources replaces it with the consent popover.

use std::time::Instant;

use hocus_link::ToolState;
use iced::{
    Color, Font, Point, Rectangle, Renderer, Size, Theme, keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry},
        text::{Alignment, LineHeight},
    },
    window,
};

use super::{
    Effect, Event as Seen, Item, Pressed, event,
    kit::{self, FILL, FILL_HOVER, Input, Pointer, text, text_in},
};
use crate::{
    agents::{self, HERDR_MARK, MARK_INK, Tool, Words},
    levels::draw::{ACCENT, DIM, FAINT, LINE, MANROPE_BOLD, TEXT, ring},
};

// --- The mockup's sizes, at ×1 --------------------------------------------------

/// Their width.
pub const WIDTH: u32 = 300;
/// Their top under the bar's.
pub const TOP: f32 = 62.0;
const BORDER: f32 = 1.0;
const PAD_TOP: f32 = 12.0;
const PAD_SIDE: f32 = 14.0;
const PAD_BOTTOM: f32 = 14.0;
const LEFT: f32 = BORDER + PAD_SIDE;
const RIGHT: f32 = WIDTH as f32 - BORDER - PAD_SIDE;
const ROOM: f32 = RIGHT - LEFT;
const HEADER_TOP: f32 = BORDER + PAD_TOP;
const HEADER: f32 = 32.0;
const TITLE: f32 = 16.0;
const MONO: Font = Font::with_name("Martian Mono");
const CODE: f32 = 11.5;
const PLUG: Color = Color::from_rgba8(108, 194, 135, 0.18);
const PLUG_HOVER: Color = Color::from_rgba8(108, 194, 135, 0.28);
const BLOCK: Color = Color::from_rgba8(255, 255, 255, 0.04);
const CODE_FILL: Color = Color::from_rgba8(0, 0, 0, 0.25);
const RULE: Color = Color::from_rgba8(255, 255, 255, 0.08);

/// Sources: rows 40 px, 6 px apart; marks 20 px; the note 13 px on 1.45
/// lines, the states' 12 px; the code's box 38 px.
const GAP: f32 = 6.0;
const SOURCE_ROW: f32 = 40.0;
const SOURCE_MARK: f32 = 20.0;
const NOTE: f32 = 13.0;
const NOTE_LINE: f32 = 1.45;
const SMALL: f32 = 12.0;
const CODE_BOX: f32 = 38.0;
/// Consent: its parts 10 px apart; its mark 22 px; its text 13.5 px on
/// 1.5 lines; the file's block 8 × 10 px in, its parts 3 px apart.
const PARTS: f32 = 10.0;
const CONSENT_MARK: f32 = 22.0;
const WHAT: f32 = 13.5;
const WHAT_LINE: f32 = 1.5;
const BLOCK_PAD: (f32, f32) = (8.0, 10.0);
const BLOCK_GAP: f32 = 3.0;
const BUTTON: f32 = 30.0;

/// The tools' rows in Sources, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Tool(Tool),
    Herdr,
}

const ROWS: [Row; 3] = [
    Row::Tool(Tool::ClaudeCode),
    Row::Tool(Tool::Codex),
    Row::Herdr,
];

/// What can be clicked in them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Plug(Tool),
    Unplug(Tool),
    Copy,
    Cancel,
    Confirm,
}

fn words() -> Words {
    Words::new(crate::clock::time::Lang::of_messages())
}

/// A pill-shaped button's width for `word`.
fn button_width(word: &str, size: f32, bold: bool, pad: f32) -> f32 {
    let font = if bold {
        MANROPE_BOLD
    } else {
        crate::levels::draw::MANROPE
    };
    kit::measure(word, size, font) + 2.0 * pad
}

// --- Sources --------------------------------------------------------------------

/// A tool's row in Sources: whose, where, and its button.
type SourceRow = (Row, Rectangle, Option<(Target, Rectangle)>);

struct SourcesLayout {
    rows: Vec<SourceRow>,
    rule: f32,
    label: f32,
    any: (f32, Vec<String>),
    code: Rectangle,
    copy: Rectangle,
    note: (f32, Vec<String>),
    end: f32,
}

fn sources_layout(said: &agents::Said, words: Words, copied: bool) -> SourcesLayout {
    let mut y = HEADER_TOP + HEADER;
    let rows = ROWS
        .into_iter()
        .map(|row| {
            y += GAP;
            let rect = Rectangle::new(Point::new(LEFT, y), Size::new(ROOM, SOURCE_ROW));
            y += SOURCE_ROW;
            let target = match row {
                Row::Tool(tool) => match said.state(tool.word()) {
                    ToolState::Found | ToolState::Declined => {
                        let w = button_width(words.plug(), 12.5, true, 10.0);
                        Some((
                            Target::Plug(tool),
                            Rectangle::new(
                                Point::new(RIGHT - w, rect.center_y() - 13.0),
                                Size::new(w, 26.0),
                            ),
                        ))
                    }
                    ToolState::Plugged => {
                        let w = button_width(words.unplug(), 12.5, false, 9.0);
                        Some((
                            Target::Unplug(tool),
                            Rectangle::new(
                                Point::new(RIGHT - w, rect.center_y() - 13.0),
                                Size::new(w, 26.0),
                            ),
                        ))
                    }
                    _ => None,
                },
                Row::Herdr => None,
            };
            (row, rect, target)
        })
        .collect();
    y += GAP + 4.0;
    let rule = y;
    y += 1.0 + 4.0 + GAP;
    let label = y;
    y += kit::LABEL * LINE + GAP;
    let any = kit::wrapped(words.any_tool(), NOTE, ROOM);
    let any_top = y;
    y += any.len() as f32 * NOTE * NOTE_LINE + GAP;
    let code = Rectangle::new(Point::new(LEFT, y), Size::new(ROOM, CODE_BOX));
    let w = button_width(words.copy(copied), SMALL, false, 8.0);
    let copy = Rectangle::new(
        Point::new(code.x + code.width - 8.0 - w, code.center_y() - 12.0),
        Size::new(w, 24.0),
    );
    y += CODE_BOX + GAP;
    let note = kit::wrapped(words.states_note(), SMALL, ROOM);
    let note_top = y;
    y += note.len() as f32 * SMALL * NOTE_LINE;
    SourcesLayout {
        rows,
        rule,
        label,
        any: (any_top, any),
        code,
        copy,
        note: (note_top, note),
        end: y,
    }
}

/// Sources' height as it shows now.
pub fn sources_height() -> u32 {
    (sources_layout(&agents::said(), words(), false).end + PAD_BOTTOM + BORDER).round() as u32
}

// --- Consent --------------------------------------------------------------------

struct ConsentLayout {
    what: (f32, Vec<String>),
    block: Rectangle,
    file: String,
    kept: Vec<String>,
    cancel: Rectangle,
    confirm: Rectangle,
    end: f32,
}

fn consent_layout(tool: Tool, said: &agents::Said, words: Words) -> ConsentLayout {
    let mut y = HEADER_TOP + HEADER + PARTS;
    let what = kit::wrapped(words.what(tool), WHAT, ROOM);
    let what_top = y;
    y += what.len() as f32 * WHAT * WHAT_LINE + PARTS;
    let inner = ROOM - 2.0 * BLOCK_PAD.1;
    let kept = kit::wrapped(words.copy_kept(), SMALL, inner);
    let file = kit::elide(&said.file(tool), inner, |t| kit::measure(t, CODE, MONO));
    let block_height = 2.0 * BLOCK_PAD.0
        + SMALL * LINE
        + BLOCK_GAP
        + CODE * LINE
        + BLOCK_GAP
        + kept.len() as f32 * SMALL * LINE;
    let block = Rectangle::new(Point::new(LEFT, y), Size::new(ROOM, block_height));
    y += block_height + PARTS;
    let confirm_w = button_width(words.plug(), 13.0, true, 12.0);
    let cancel_w = button_width(words.cancel(), 13.0, false, 12.0);
    let confirm = Rectangle::new(
        Point::new(RIGHT - confirm_w, y),
        Size::new(confirm_w, BUTTON),
    );
    let cancel = Rectangle::new(
        Point::new(confirm.x - 6.0 - cancel_w, y),
        Size::new(cancel_w, BUTTON),
    );
    y += BUTTON;
    ConsentLayout {
        what: (what_top, what),
        block,
        file,
        kept,
        cancel,
        confirm,
        end: y,
    }
}

/// The consent popover's height for `tool`.
pub fn consent_height(tool: Tool) -> u32 {
    (consent_layout(tool, &agents::said(), words()).end + PAD_BOTTOM + BORDER).round() as u32
}

// --- The panel ------------------------------------------------------------------

/// One of the agents' popovers on its surface.
#[derive(Debug, Clone)]
pub struct Panel {
    pub id: window::Id,
    pub words: Words,
    pub glass: bool,
}

/// What the surface keeps between draws.
#[derive(Debug, Default)]
pub struct State {
    /// Whose popover it is (closing, it shows what it showed).
    item: Option<Item>,
    said: agents::Said,
    pointer: Pointer<Target>,
    focus: Option<Target>,
    copied: bool,
}

impl State {
    fn targets(&self, words: Words) -> Vec<(Target, Rectangle)> {
        match self.item {
            Some(Item::Plug(tool)) => {
                let l = consent_layout(tool, &self.said, words);
                vec![(Target::Cancel, l.cancel), (Target::Confirm, l.confirm)]
            }
            Some(Item::Sources | Item::AnotherTool) => {
                let l = sources_layout(&self.said, words, self.copied);
                l.rows
                    .into_iter()
                    .filter_map(|(_, _, t)| t)
                    .chain([(Target::Copy, l.copy)])
                    .collect()
            }
            _ => Vec::new(),
        }
    }
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
            Input::Redraw(now) => self.refresh(state, now),
            Input::Mouse(seen, at) => self.mouse(state, seen, at),
            Input::Key(pressed) => self.key(state, &pressed),
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
        match state.item {
            Some(Item::Plug(tool)) => self.consent(f, state, tool),
            Some(Item::Sources | Item::AnotherTool) => self.sources(f, state),
            _ => {}
        }
        if let Some((_, rect)) = state.focus.and_then(|focus| {
            state
                .targets(self.words)
                .into_iter()
                .find(|(t, _)| *t == focus)
        }) {
            ring(f, rect, 8.0);
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        state.pointer.interaction(false)
    }
}

/// `lines` from `top` at `x`, `size` px on lines `height` em apart.
fn lines(
    f: &mut Frame,
    lines: &[String],
    (x, top): (f32, f32),
    size: f32,
    height: f32,
    colour: Color,
) {
    for (i, line) in lines.iter().enumerate() {
        let mut said = text(
            line.clone(),
            Point::new(x, top + (i as f32 + 0.5) * size * height),
            size,
            colour,
            Alignment::Left,
        );
        said.line_height = LineHeight::Relative(height);
        f.fill_text(said);
    }
}

/// A button at `rect` saying `word`.
fn button(
    f: &mut Frame,
    rect: Rectangle,
    word: &str,
    (size, bold): (f32, bool),
    (fill, ink): (Color, Color),
    radius: f32,
) {
    f.fill(&kit::rounded(rect, radius), fill);
    let said = text(word.to_owned(), rect.center(), size, ink, Alignment::Center);
    f.fill_text(if bold {
        text_in(MANROPE_BOLD, said)
    } else {
        said
    });
}

impl Panel {
    fn sources(&self, f: &mut Frame, state: &State) {
        let words = self.words;
        let hover = state.pointer.hover;
        kit::title(f, words.sources_title());
        let l = sources_layout(&state.said, words, state.copied);
        for (row, rect, target) in &l.rows {
            let (name, mark, tool_state) = match row {
                Row::Tool(tool) => (tool.title(), tool.mark(), state.said.state(tool.word())),
                Row::Herdr => ("herdr", HERDR_MARK, state.said.state("herdr")),
            };
            let there = !matches!(tool_state, ToolState::Absent | ToolState::Stopped);
            let opacity = if there { 1.0 } else { 0.4 };
            let middle = rect.center_y();
            agents::draw::mark(
                f,
                Point::new(LEFT, middle - SOURCE_MARK / 2.0),
                mark,
                (SOURCE_MARK, 5.0, 11.0),
                opacity,
            );
            let column = 14.0 * LINE + 1.0 + SMALL * LINE;
            let name_middle = middle - column / 2.0 + 14.0 * LINE / 2.0;
            let x = LEFT + SOURCE_MARK + 10.0;
            f.fill_text(text(
                name.to_owned(),
                Point::new(x, name_middle),
                14.0,
                if there { TEXT } else { FAINT },
                Alignment::Left,
            ));
            let (said, green) = words.tool_state(tool_state);
            f.fill_text(text(
                said.to_owned(),
                Point::new(
                    x,
                    name_middle + 14.0 * LINE / 2.0 + 1.0 + SMALL * LINE / 2.0,
                ),
                SMALL,
                if green { ACCENT } else { FAINT },
                Alignment::Left,
            ));
            match target {
                Some((t @ Target::Plug(_), rect)) => {
                    let fill = if hover == Some(*t) { PLUG_HOVER } else { PLUG };
                    button(f, *rect, words.plug(), (12.5, true), (fill, ACCENT), 13.0);
                }
                Some((t @ Target::Unplug(_), rect)) => {
                    let lit = hover == Some(*t);
                    let colours = if lit { (FILL_HOVER, TEXT) } else { (FILL, DIM) };
                    button(f, *rect, words.unplug(), (12.5, false), colours, 7.0);
                }
                _ => {}
            }
        }
        f.fill_rectangle(Point::new(LEFT, l.rule), Size::new(ROOM, 1.0), RULE);
        let letters = kit::label_letters(words.another_label());
        kit::label(f, &letters, l.label + kit::LABEL * LINE / 2.0);
        lines(f, &l.any.1, (LEFT, l.any.0), NOTE, NOTE_LINE, DIM);
        f.fill(&kit::rounded(l.code, 8.0), CODE_FILL);
        f.fill_text(text_in(
            MONO,
            text(
                words.command().to_owned(),
                Point::new(l.code.x + 10.0, l.code.center_y()),
                CODE,
                TEXT,
                Alignment::Left,
            ),
        ));
        let lit = hover == Some(Target::Copy);
        let colours = if lit { (FILL_HOVER, TEXT) } else { (FILL, DIM) };
        button(
            f,
            l.copy,
            words.copy(state.copied),
            (SMALL, false),
            colours,
            6.0,
        );
        lines(f, &l.note.1, (LEFT, l.note.0), SMALL, NOTE_LINE, FAINT);
    }

    fn consent(&self, f: &mut Frame, state: &State, tool: Tool) {
        let words = self.words;
        let hover = state.pointer.hover;
        let header_middle = HEADER_TOP + HEADER / 2.0;
        agents::draw::mark(
            f,
            Point::new(LEFT, header_middle - CONSENT_MARK / 2.0),
            tool.mark(),
            (CONSENT_MARK, 6.0, 13.0),
            1.0,
        );
        f.fill_text(text_in(
            MANROPE_BOLD,
            text(
                words.plug_title(tool),
                Point::new(LEFT + CONSENT_MARK + 10.0, header_middle),
                TITLE,
                TEXT,
                Alignment::Left,
            ),
        ));
        let l = consent_layout(tool, &state.said, words);
        lines(f, &l.what.1, (LEFT, l.what.0), WHAT, WHAT_LINE, DIM);
        f.fill(&kit::rounded(l.block, 8.0), BLOCK);
        let x = l.block.x + BLOCK_PAD.1;
        let mut y = l.block.y + BLOCK_PAD.0;
        f.fill_text(text(
            words.file_changed().to_owned(),
            Point::new(x, y + SMALL * LINE / 2.0),
            SMALL,
            FAINT,
            Alignment::Left,
        ));
        y += SMALL * LINE + BLOCK_GAP;
        f.fill_text(text_in(
            MONO,
            text(
                l.file.clone(),
                Point::new(x, y + CODE * LINE / 2.0),
                CODE,
                TEXT,
                Alignment::Left,
            ),
        ));
        y += CODE * LINE + BLOCK_GAP;
        lines(f, &l.kept, (x, y), SMALL, LINE, FAINT);
        let lit = hover == Some(Target::Cancel);
        let colours = if lit { (FILL_HOVER, TEXT) } else { (FILL, DIM) };
        button(f, l.cancel, words.cancel(), (13.0, false), colours, 8.0);
        let fill = if hover == Some(Target::Confirm) {
            crate::levels::draw::rgb(0x7FD198)
        } else {
            ACCENT
        };
        button(
            f,
            l.confirm,
            words.plug(),
            (13.0, true),
            (fill, MARK_INK),
            8.0,
        );
    }

    /// What it shows, read afresh.
    fn refresh<Message>(
        &self,
        state: &mut State,
        _now: Instant,
    ) -> Option<canvas::Action<Message>> {
        for pressed in kit::keys(self.id) {
            let _: Option<canvas::Action<Message>> = self.key(state, &pressed);
        }
        let item = super::with(|c| c.is_open(self.id).then(|| c.open_item()).flatten());
        if let Some(item) = item {
            state.item = Some(item);
            let said = agents::said();
            if said != state.said {
                tracing::debug!(
                    ?item,
                    tools = ?said.tools.iter().map(|t| (t.tool.as_str(), self.words.tool_state(t.state).0)).collect::<Vec<_>>(),
                    "the agents' popover shows it"
                );
                state.said = said;
                super::refit(item);
            }
        }
        None
    }

    fn act(&self, state: &mut State, target: Target) {
        tracing::debug!(?target, "the agents' popover");
        match target {
            Target::Plug(tool) => event(Seen::Replace(Item::Plug(tool))),
            Target::Unplug(tool) => super::send(vec![Effect::Unplug(tool)]),
            Target::Copy => {
                state.copied = true;
                super::send(vec![
                    Effect::Copy(self.words.command().to_owned()),
                    Effect::Wake(hocus_link::POPOVER_NAMESPACE),
                ]);
            }
            Target::Cancel => event(Seen::Chosen),
            Target::Confirm => {
                if let Some(Item::Plug(tool)) = state.item {
                    super::send(vec![Effect::Plug(tool)]);
                }
                event(Seen::Chosen);
            }
        }
    }

    fn mouse<Message>(
        &self,
        state: &mut State,
        seen: &mouse::Event,
        at: Option<Point>,
    ) -> Option<canvas::Action<Message>> {
        let target = at.and_then(|at| {
            state
                .targets(self.words)
                .into_iter()
                .find(|(_, r)| r.contains(at))
                .map(|(t, _)| t)
        });
        let redraw = Some(canvas::Action::request_redraw().and_capture());
        match seen {
            mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                state.pointer.moved();
                state.pointer.over(target).then_some(redraw).flatten()
            }
            mouse::Event::CursorLeft => {
                state.pointer.left();
                redraw
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                state.focus = None;
                state.pointer.pressed = target;
                redraw
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                if let Some(target) = state.pointer.release(target) {
                    self.act(state, target);
                }
                redraw
            }
            _ => None,
        }
    }

    fn key<Message>(
        &self,
        state: &mut State,
        pressed: &Pressed,
    ) -> Option<canvas::Action<Message>> {
        use keyboard::{Key, key::Named};
        let order: Vec<Target> = state
            .targets(self.words)
            .into_iter()
            .map(|(t, _)| t)
            .collect();
        match &pressed.key {
            Key::Named(Named::Escape) => {
                event(Seen::Escape);
                return Some(canvas::Action::capture());
            }
            Key::Named(Named::Tab) => {
                state.focus = kit::step(&order, state.focus, pressed.modifiers.shift());
            }
            Key::Named(arrow @ (Named::ArrowDown | Named::ArrowUp)) => {
                state.focus = kit::step(&order, state.focus, *arrow == Named::ArrowUp);
            }
            Key::Named(Named::Enter | Named::Space) => {
                if let Some(target) = state.focus.filter(|t| order.contains(t)) {
                    self.act(state, target);
                }
            }
            _ => return None,
        }
        Some(canvas::Action::request_redraw().and_capture())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_lists_three_tools_and_the_command() {
        let said = agents::Said::default();
        let l = sources_layout(&said, Words::new(crate::clock::time::Lang::French), false);
        // Its rows 40 px from 51, 6 px apart; none can be plugged in when
        // nothing is there.
        assert_eq!(l.rows[0].1.y, 51.0);
        assert_eq!(l.rows[2].1.y, 51.0 + 2.0 * 46.0);
        assert!(l.rows.iter().all(|(_, _, t)| t.is_none()));
        assert!(l.code.y > l.rule && l.note.0 > l.code.y + CODE_BOX);
    }
}
