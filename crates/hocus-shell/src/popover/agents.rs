//! The agents' group's popovers (bar.md §9; the approved 4.0 mockup),
//! 300 px wide, their top 62 px under the bar's (the mockup's, whichever
//! item opened them):
//!
//! - **Sources** (the secretary's briefing's "Sources", the card's "Un
//!   autre outil ?"): "Sources des agents";
//!   a row per tool (its mark, its name over its state, "Brancher" or
//!   "Débrancher"); in the mini form, opened from the secretary's "+",
//!   "Ne rien brancher" under them, as the card has it (4.7b: the mini
//!   form has no card); the divider, "UN AUTRE OUTIL", the generic
//!   command in a box with "Copier", and the states it takes.
//! - **Consent** (a row's "Brancher"): "Brancher <outil>", what it lets
//!   the desktop know, the file it changes and that a copy is kept;
//!   "Annuler" and "Brancher". Nothing is written before that click.
//!
//! `Tab` goes through their buttons, `Enter` or `Space` acts, `Esc`
//! closes. "Brancher" in Sources replaces it with the consent popover.

use std::sync::{Mutex, PoisonError};

use hocus_a11y::Role;
use hocus_link::{Form, ToolState};
use iced::{
    Color, Element, Font, Length, Point, Rectangle, Size,
    alignment::Vertical,
    widget::{Space, Stack, button, column, container, pin, text::LineHeight},
    window,
};

use super::{
    Effect, Event as Seen, Item, event,
    kit::{self, FILL, FILL_HOVER},
};
use crate::theme::Ink;
use crate::{
    agents::{self, HERDR_MARK, LINK, MARK_INK, Shows, Tool, Words},
    levels::draw::{ACCENT, DIM, FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT},
    ui::{
        Around, control, line,
        parts::{drawn, spaced},
        style::{self, boxed, said},
    },
};

type El<'a> = Element<'a, Msg>;

// --- The mockup's sizes, at ×1 --------------------------------------------------

/// Their width.
pub const WIDTH: u32 = 300;
/// Their top under the bar's when the agents' group is first under the
/// clock (`crate::column::start`): beside it, higher by as much wherever
/// the column puts it.
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
const PLUG: Ink = Ink::Accent(0.18);
const PLUG_HOVER: Ink = Ink::Accent(0.28);
const BLOCK: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.04));
const CODE_FILL: Ink = Ink::Dark(Color::from_rgba8(0, 0, 0, 0.25));
const RULE: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.08));

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
pub enum Target {
    Plug(Tool),
    Unplug(Tool),
    /// herdr, declined, followed again.
    Follow,
    /// "Ne rien brancher", in the mini form (4.7b).
    Decline,
    Copy,
    Cancel,
    Confirm,
}

impl Target {
    /// Its control's id: what tests, and screen readers, know it by.
    fn id(self) -> String {
        format!("agents:{self:?}")
    }
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
    /// "Ne rien brancher", when it is offered.
    decline: Option<Rectangle>,
    rule: f32,
    label: f32,
    any: (f32, Vec<String>),
    code: Rectangle,
    copy: Rectangle,
    note: (f32, Vec<String>),
    end: f32,
}

/// Whether Sources offers "Ne rien brancher" on a bar in `form`: in the
/// mini form, while the group would show the card (the secretary's "+"
/// opens it then), which has no other way to it (4.7b). The full form's
/// card has its own.
fn declines(said: &agents::Said, form: Option<Form>) -> bool {
    form == Some(Form::Mini) && matches!(said.shows(0), Shows::Card(_))
}

fn sources_layout(said: &agents::Said, words: Words, copied: bool, decline: bool) -> SourcesLayout {
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
                Row::Herdr if said.state("herdr") == ToolState::Ignored => {
                    let w = button_width(words.follow(), 12.5, true, 10.0);
                    Some((
                        Target::Follow,
                        Rectangle::new(
                            Point::new(RIGHT - w, rect.center_y() - 13.0),
                            Size::new(w, 26.0),
                        ),
                    ))
                }
                Row::Herdr => None,
            };
            (row, rect, target)
        })
        .collect();
    // Under the rows on the right, as the card's foot has it.
    let decline = decline.then(|| {
        y += GAP;
        let w = kit::measure(words.decline(), LINK, crate::levels::draw::MANROPE);
        let rect = Rectangle::new(Point::new(RIGHT - w, y), Size::new(w, LINK * LINE));
        y += LINK * LINE;
        rect
    });
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
        decline,
        rule,
        label,
        any: (any_top, any),
        code,
        copy,
        note: (note_top, note),
        end: y,
    }
}

/// Sources' height as it shows now on a bar in `form`.
pub fn sources_height(form: Form) -> u32 {
    let said = agents::said();
    let decline = declines(&said, Some(form));
    (sources_layout(&said, words(), false, decline).end + PAD_BOTTOM + BORDER).round() as u32
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

/// What the surface keeps from a build of its view to the next.
#[derive(Debug, Default)]
struct Ui {
    /// Whose popover it is (closing, it shows what it showed).
    item: Option<Item>,
    said: agents::Said,
    focus: Option<Target>,
    copied: bool,
    /// The form of its bar, as it last read it.
    form: Option<Form>,
    /// What was said last, for the log.
    told: Option<(Item, agents::Said, Option<Form>)>,
}

static UI: Mutex<Option<Ui>> = Mutex::new(None);

fn ui<T>(with: impl FnOnce(&mut Ui) -> T) -> T {
    let mut ui = UI.lock().unwrap_or_else(PoisonError::into_inner);
    with(ui.get_or_insert_with(Ui::default))
}

/// One opens: nothing holds the ring, nothing copied.
pub fn opening() {
    ui(|ui| *ui = Ui::default());
}

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A button pressed, or `Enter` on it.
    Act(Target),
    /// The ring comes to it.
    Focus(Target),
    /// A press anywhere on the popover.
    Pressed,
    /// The tools changed under the view.
    Stale,
}

/// One of the agents' popovers on its surface.
pub fn view<'a>(panel: &Panel) -> Element<'a, crate::Message> {
    let (id, words) = (panel.id, panel.words);
    Around::new(id, build(id, words), handle, Msg::Stale)
        .rebuild(move || build(id, words))
        .subtitle(ui(|ui| match ui.item {
            Some(Item::Plug(tool)) => Some(words.plug_title(tool)),
            _ => None,
        }))
        .press(Msg::Pressed)
        .on_glass(panel.glass)
}

/// Its content, as the tools are now.
fn build<'a>(id: window::Id, words: Words) -> Element<'a, Msg> {
    let open = super::with(|c| {
        c.is_open(id)
            .then(|| c.open_item().map(|item| (item, c.form_of(id))))
            .flatten()
    });
    ui(|ui| {
        if let Some((item, form)) = open {
            ui.item = Some(item);
            ui.said = agents::said();
            ui.form = form;
            let told = (item, ui.said.clone(), form);
            if ui.told.as_ref() != Some(&told) {
                tracing::debug!(
                    ?item,
                    tools = ?ui.said.tools.iter().map(|t| (t.tool.as_str(), words.tool_state(t.state).0)).collect::<Vec<_>>(),
                    "the agents' popover shows it"
                );
                ui.told = Some(told);
            }
        }
        match ui.item {
            Some(Item::Plug(tool)) => consent(tool, words, ui),
            Some(Item::Sources | Item::AnotherTool) => sources(words, ui),
            _ => Space::new().into(),
        }
    })
}

/// `content` with its top left corner at `point` on the popover.
fn at<'a>(content: impl Into<El<'a>>, point: Point) -> El<'a> {
    pin(content).x(point.x).y(point.y).into()
}

/// `lines` from `top` at `x`, `size` px on lines `height` em apart, in
/// `colour`.
fn lines<'a>(lines: &[String], size: f32, height: f32, colour: Color) -> El<'a> {
    let mut all = column![];
    for said_line in lines {
        all = all.push(
            said(said_line.clone(), size, MANROPE, colour)
                .line_height(LineHeight::Relative(height)),
        );
    }
    all.into()
}

/// The popover's title, as the others'.
fn title<'a>(title: String) -> El<'a> {
    at(
        container(said(title, kit::TITLE, MANROPE_BOLD, TEXT.get()))
            .height(kit::HEADER)
            .align_y(Vertical::Center),
        Point::new(kit::LEFT, kit::HEADER_TOP),
    )
}

/// A tool's mark, `size` px.
fn mark<'a>(
    (letter, colour): (&'static str, Color),
    (size, radius, letter_size): (f32, f32, f32),
    alpha: f32,
) -> El<'a> {
    drawn(size, size, move |f, _| {
        agents::draw::mark(
            f,
            Point::ORIGIN,
            (letter, colour),
            (size, radius, letter_size),
            alpha,
        );
    })
}

/// A button at `rect` saying `word` (`size`, bold or not): its fills and
/// its words' colours at rest and under the pointer.
#[allow(clippy::too_many_arguments)]
fn button_at<'a>(
    target: Target,
    name: String,
    rect: Rectangle,
    word: &str,
    (size, bold): (f32, bool),
    rest: (Color, Color),
    lit: (Color, Color),
    radius: f32,
    focus: Option<Target>,
) -> El<'a> {
    let font = if bold { MANROPE_BOLD } else { MANROPE };
    let face = container(
        iced::widget::text(word.to_owned())
            .size(size)
            .font(font)
            .line_height(LineHeight::Relative(LINE))
            .shaping(iced::widget::text::Shaping::Advanced)
            .wrapping(iced::widget::text::Wrapping::None),
    )
    .center(Length::Fill);
    let pressed = button(face)
        .padding(0)
        .width(rect.width)
        .height(rect.height)
        .style(move |_, status| {
            let (fill, ink) = if style::lit(status) { lit } else { rest };
            style::filled(fill, radius, ink)
        })
        .on_press(Msg::Act(target));
    at(
        control(pressed, target.id(), Role::Button, name)
            .focus(focus == Some(target), Msg::Focus(target), 8.0)
            .activate(Msg::Act(target))
            .rows(0),
        rect.position(),
    )
}

/// A quiet button's fills and words: at rest, under the pointer.
fn quiet() -> [(Color, Color); 2] {
    [(FILL.get(), DIM.get()), (FILL_HOVER.get(), TEXT.get())]
}

fn sources<'a>(words: Words, ui: &Ui) -> El<'a> {
    let decline = declines(&ui.said, ui.form);
    let l = sources_layout(&ui.said, words, ui.copied, decline);
    let mut layers = vec![title(words.sources_title().to_owned())];
    for (row, rect, target) in &l.rows {
        let (name, glyph, tool_state) = match row {
            Row::Tool(tool) => (tool.title(), tool.mark(), ui.said.state(tool.word())),
            Row::Herdr => ("herdr", HERDR_MARK, ui.said.state("herdr")),
        };
        let there = !matches!(tool_state, ToolState::Absent | ToolState::Stopped);
        let opacity = if there { 1.0 } else { 0.4 };
        let middle = rect.center_y();
        layers.push(at(
            mark(glyph, (SOURCE_MARK, 5.0, 11.0), opacity),
            Point::new(LEFT, middle - SOURCE_MARK / 2.0),
        ));
        let (state_said, green) = words.tool_state(tool_state);
        let column_height = 14.0 * LINE + 1.0 + SMALL * LINE;
        let texts = column![
            said(
                name.to_owned(),
                14.0,
                MANROPE,
                if there { TEXT.get() } else { FAINT.get() }
            ),
            Space::new().height(1.0),
            said(
                state_said.to_owned(),
                SMALL,
                MANROPE,
                if green { ACCENT.get() } else { FAINT.get() }
            ),
        ];
        let texts_at = Point::new(LEFT + SOURCE_MARK + 10.0, middle - column_height / 2.0);
        match target {
            Some((t, button_rect)) => {
                layers.push(at(texts, texts_at));
                let word = match t {
                    Target::Follow => words.follow(),
                    Target::Unplug(_) => words.unplug(),
                    _ => words.plug(),
                };
                let name = format!("{word} {name}");
                let button = match t {
                    Target::Unplug(_) => {
                        let [rest, lit] = quiet();
                        button_at(
                            *t,
                            name,
                            *button_rect,
                            word,
                            (12.5, false),
                            rest,
                            lit,
                            7.0,
                            ui.focus,
                        )
                    }
                    _ => {
                        let ink = crate::theme::ACCENT_TEXT.get();
                        button_at(
                            *t,
                            name,
                            *button_rect,
                            word,
                            (12.5, true),
                            (PLUG.get(), ink),
                            (PLUG_HOVER.get(), ink),
                            13.0,
                            ui.focus,
                        )
                    }
                };
                layers.push(button);
            }
            None => layers.push(at(line(texts, format!("{name}, {state_said}")), texts_at)),
        }
    }
    if let Some(rect) = l.decline {
        let link = button(
            iced::widget::text(words.decline())
                .size(LINK)
                .font(MANROPE)
                .line_height(LineHeight::Relative(LINE))
                .shaping(iced::widget::text::Shaping::Advanced)
                .wrapping(iced::widget::text::Wrapping::None),
        )
        .padding(0)
        .style(|_, status| {
            let mut quiet = style::clear();
            quiet.text_color = if style::lit(status) {
                TEXT.get()
            } else {
                FAINT.get()
            };
            quiet
        })
        .on_press(Msg::Act(Target::Decline));
        layers.push(at(
            control(link, Target::Decline.id(), Role::Button, words.decline())
                .focus(
                    ui.focus == Some(Target::Decline),
                    Msg::Focus(Target::Decline),
                    8.0,
                )
                .activate(Msg::Act(Target::Decline))
                .rows(0),
            rect.position(),
        ));
    }
    layers.push(at(
        container(Space::new().width(ROOM).height(1.0)).style(|_| boxed(RULE.get(), 0.0)),
        Point::new(LEFT, l.rule),
    ));
    layers.push(at(
        line(
            container(spaced(
                words.another_label(),
                kit::LABEL,
                DIM.get(),
                f32::INFINITY,
            ))
            .height(kit::LABEL * LINE)
            .align_y(Vertical::Center),
            words.another_label(),
        ),
        Point::new(LEFT, l.label),
    ));
    layers.push(at(
        line(
            lines(&l.any.1, NOTE, NOTE_LINE, DIM.get()),
            l.any.1.join(" "),
        ),
        Point::new(LEFT, l.any.0),
    ));
    layers.push(at(
        container(Space::new().width(ROOM).height(CODE_BOX)).style(|_| boxed(CODE_FILL.get(), 8.0)),
        l.code.position(),
    ));
    layers.push(at(
        line(
            container(said(words.command(), CODE, MONO, TEXT.get()))
                .height(CODE_BOX)
                .align_y(Vertical::Center),
            words.command(),
        ),
        Point::new(l.code.x + 10.0, l.code.y),
    ));
    let [rest, lit] = quiet();
    layers.push(button_at(
        Target::Copy,
        words.copy(ui.copied).to_owned(),
        l.copy,
        words.copy(ui.copied),
        (SMALL, false),
        rest,
        lit,
        6.0,
        ui.focus,
    ));
    layers.push(at(
        line(
            lines(&l.note.1, SMALL, NOTE_LINE, FAINT.get()),
            l.note.1.join(" "),
        ),
        Point::new(LEFT, l.note.0),
    ));
    Stack::with_children(layers)
        .width(WIDTH as f32)
        .height(l.end + PAD_BOTTOM + BORDER)
        .into()
}

fn consent<'a>(tool: Tool, words: Words, ui: &Ui) -> El<'a> {
    let header_middle = HEADER_TOP + HEADER / 2.0;
    let l = consent_layout(tool, &ui.said, words);
    let mut layers = vec![
        at(
            mark(tool.mark(), (CONSENT_MARK, 6.0, 13.0), 1.0),
            Point::new(LEFT, header_middle - CONSENT_MARK / 2.0),
        ),
        at(
            container(said(
                words.plug_title(tool),
                TITLE,
                MANROPE_BOLD,
                TEXT.get(),
            ))
            .height(HEADER)
            .align_y(Vertical::Center),
            Point::new(LEFT + CONSENT_MARK + 10.0, HEADER_TOP),
        ),
        at(
            line(
                lines(&l.what.1, WHAT, WHAT_LINE, DIM.get()),
                l.what.1.join(" "),
            ),
            Point::new(LEFT, l.what.0),
        ),
    ];
    let block = column![
        line(
            said(words.file_changed(), SMALL, MANROPE, FAINT.get()),
            format!("{} {}", words.file_changed(), l.file),
        ),
        Space::new().height(BLOCK_GAP),
        said(l.file.clone(), CODE, MONO, TEXT.get()),
        Space::new().height(BLOCK_GAP),
        line(lines(&l.kept, SMALL, LINE, FAINT.get()), l.kept.join(" ")),
    ];
    layers.push(at(
        container(block)
            .padding([BLOCK_PAD.0, BLOCK_PAD.1])
            .width(l.block.width)
            .height(l.block.height)
            .style(|_| boxed(BLOCK.get(), 8.0)),
        l.block.position(),
    ));
    let [rest, lit] = quiet();
    layers.push(button_at(
        Target::Cancel,
        words.cancel().to_owned(),
        l.cancel,
        words.cancel(),
        (13.0, false),
        rest,
        lit,
        8.0,
        ui.focus,
    ));
    let ink = MARK_INK.get();
    layers.push(button_at(
        Target::Confirm,
        words.plug().to_owned(),
        l.confirm,
        words.plug(),
        (13.0, true),
        (ACCENT.get(), ink),
        (crate::levels::draw::rgb(0x7FD198), ink),
        8.0,
        ui.focus,
    ));
    Stack::with_children(layers)
        .width(WIDTH as f32)
        .height(l.end + PAD_BOTTOM + BORDER)
        .into()
}

// --- What a message does -------------------------------------------------------

fn handle(message: Msg) {
    match message {
        Msg::Stale => {}
        Msg::Pressed => ui(|ui| ui.focus = None),
        Msg::Focus(target) => ui(|ui| ui.focus = Some(target)),
        Msg::Act(target) => act(target),
    }
    if let Some(item) = ui(|ui| ui.item) {
        super::refit(item);
    }
}

fn act(target: Target) {
    tracing::debug!(?target, "the agents' popover");
    match target {
        Target::Plug(tool) => event(Seen::Replace(Item::Plug(tool))),
        Target::Unplug(tool) => super::send(vec![Effect::Unplug(tool)]),
        Target::Follow => super::send(vec![Effect::FollowHerdr]),
        // As the card's: the group goes, its item with it in the mini
        // form, and Sources with them.
        Target::Decline => {
            super::send(vec![Effect::Decline]);
            event(Seen::Chosen);
        }
        Target::Copy => {
            ui(|ui| ui.copied = true);
            super::send(vec![Effect::Copy(words().command().to_owned())]);
        }
        Target::Cancel => event(Seen::Chosen),
        Target::Confirm => {
            if let Some(Item::Plug(tool)) = ui(|ui| ui.item) {
                super::send(vec![Effect::Plug(tool)]);
            }
            event(Seen::Chosen);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_lists_three_tools_and_the_command() {
        let said = agents::Said::default();
        let l = sources_layout(
            &said,
            Words::new(crate::clock::time::Lang::French),
            false,
            false,
        );
        // Its rows 40 px from 51, 6 px apart; none can be plugged in when
        // nothing is there.
        assert_eq!(l.rows[0].1.y, 51.0);
        assert_eq!(l.rows[2].1.y, 51.0 + 2.0 * 46.0);
        assert!(l.rows.iter().all(|(_, _, t)| t.is_none()));
        assert!(l.code.y > l.rule && l.note.0 > l.code.y + CODE_BOX);
    }

    #[test]
    fn the_mini_forms_sources_offers_to_plug_nothing_in_under_the_rows() {
        let tool = |tool: &str, state| hocus_link::AgentTool {
            tool: tool.to_owned(),
            state,
            file: String::new(),
        };
        let said = agents::Said {
            tools: vec![tool("claude-code", ToolState::Found)],
            agents: Vec::new(),
        };
        // Only in the mini form, and only while the card would show.
        assert!(declines(&said, Some(Form::Mini)));
        assert!(!declines(&said, Some(Form::Full)));
        let plugged = agents::Said {
            tools: vec![tool("claude-code", ToolState::Plugged)],
            agents: Vec::new(),
        };
        assert!(!declines(&plugged, Some(Form::Mini)));
        let words = Words::new(crate::clock::time::Lang::French);
        let without = sources_layout(&said, words, false, false);
        let with = sources_layout(&said, words, false, true);
        let link = with.decline.expect("Ne rien brancher");
        // On the right, 6 px under the last row, the divider under it.
        assert_eq!(link.x + link.width, RIGHT);
        assert_eq!(link.y, with.rows[2].1.y + SOURCE_ROW + GAP);
        assert!(with.rule > link.y + link.height);
        assert!((with.end - without.end - (GAP + LINK * LINE)).abs() < 0.01);
        assert!(without.decline.is_none());
    }
}
