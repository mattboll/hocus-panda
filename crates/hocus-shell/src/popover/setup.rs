//! The sources' popover (Day.1, #425, variant B), from the first-run
//! card's "Choisir…": 320 px, the popovers' glass, padding 12 px at the
//! top, 14 px on the sides and the bottom, its parts 12 px apart, its top
//! on the card's. Its header (28 px): a back arrow from the second step on
//! (26 × 26, "Retour"), the step's title (15 px, 700), ✕ ("Fermer").
//!
//! - **Step 1, "Ajouter un compte"**: one row per kind (radius 10,
//!   `rgba(255,255,255,0.03)`, `0.06` under the pointer): a 32 px tile, the
//!   kind's letter in its tint on 16 % of it, its name (13.5 px, 600), a
//!   line (12 px, `text.dim`), a chevron. Google's lists the Google
//!   accounts of GNOME Online Accounts whose calendar is on, each going
//!   straight to step 3; none, it opens Online Accounts' panel; without
//!   Online Accounts, it is disabled. Under the rows, where tasks stay
//!   (12 px, `text.faint`).
//! - **Step 2, "Tableau Planka"**: the board's address and the access
//!   token (34 px fields, radius 8, `rgba(255,255,255,0.05)`, a 1 px
//!   `0.14` rim, their labels 12 px 600 `text.dim` above; the token masked,
//!   `Ctrl+V` pasting as in 3.5b), the help line, "Annuler" and "Se
//!   connecter" (green; "Connexion…" while it checks). What went wrong in
//!   12.5 px `#F7B98A`, the fields keeping what was typed.
//! - **Step 3, "Quoi montrer ?"**: the account's name and address, then
//!   its calendars (a 16 px box, a 16 px swatch which unfolds the eight
//!   colours, a 16 px star, "Mettre en avant", which features it, the
//!   name, "lecture seule"), or the board's three columns, each a menu,
//!   and how many cards the tasks' column holds. "Terminé".
//!
//! The keyboard: `Tab` through the rows, the fields and the buttons, the
//! arrows through a list's rows, `Space` ticks, `Enter` chooses, `Esc`
//! goes back a step, then closes.

use std::sync::{Mutex, PoisonError};

use hocus_a11y::Role;
use iced::{
    Color, Element, Length, Point, Rectangle, Size,
    alignment::{Horizontal, Vertical},
    keyboard,
    widget::{
        Space, Stack, button,
        canvas::{self, Frame, Path, Stroke},
        container, pin,
    },
    window,
};

use super::{Event as Seen, Pressed, event, kit};
use crate::theme::Ink;
use crate::{
    field::{self, Took, view::Style},
    levels::draw::{DIM, FAINT, LINE as EM, MANROPE, MANROPE_BOLD, TEXT, faded, rgb},
    setup::{
        self, CHOICES, Calendars, Field, Online, PALETTE, Refusal, Show, Step, Unfold, Words,
        card::{BLUE, GREEN, INK},
    },
    ui::{
        self, Around, control,
        parts::{drawn, drawn_lit, moving, spaced},
        style::{self, said},
    },
};

type El<'a> = Element<'a, Msg>;

pub const WIDTH: u32 = 320;
const BORDER: f32 = 1.0;
const LEFT: f32 = BORDER + 14.0;
const RIGHT: f32 = WIDTH as f32 - BORDER - 14.0;
const CONTENT: f32 = RIGHT - LEFT;
const HEADER_TOP: f32 = BORDER + 12.0;
const HEADER: f32 = 28.0;
const GAP: f32 = 12.0;
const BUTTON_SQUARE: f32 = 26.0;
const TITLE: f32 = 15.0;
/// Step 1's rows: 54 px at least, radius 10; the tile 32 px 11 px in; the
/// name 13.5 px, the line 12 px on 16 px lines; an account 34 px.
const ROW: f32 = 54.0;
const ROW_RADIUS: f32 = 10.0;
const ROW_GAP: f32 = 8.0;
const TILE: f32 = 32.0;
const TILE_IN: f32 = 11.0;
const NAME_X: f32 = TILE_IN + TILE + 12.0;
const NAME: f32 = 13.5;
const LINE_TEXT: f32 = 12.0;
const LINE: f32 = 16.0;
const SUB: f32 = 34.0;
const CHEVRON: f32 = 14.0;
/// Step 2's fields and buttons.
const LABEL: f32 = 12.0;
const LABEL_HEIGHT: f32 = 16.0;
const FIELD: f32 = 34.0;
const FIELD_RADIUS: f32 = 8.0;
const FIELD_PAD: f32 = 10.0;
const FIELD_TEXT: f32 = 13.0;
const ERROR: f32 = 12.5;
const ERROR_LINE: f32 = 17.0;
const BUTTON: f32 = 32.0;
const BUTTON_RADIUS: f32 = 8.0;
const BUTTON_TEXT: f32 = 12.5;
/// Step 3's rows: 32 px; the box and the swatch 16 px; options 28 px; the
/// eight colours 20 px, 6 px apart.
const CHECK_ROW: f32 = 32.0;
const BOX: f32 = 16.0;
const CAPTION: f32 = 11.5;
const CAPTION_HEIGHT: f32 = 16.0;
const OPTION: f32 = 28.0;
const SWATCH_ROW: f32 = 32.0;
const SWATCH: f32 = 20.0;
const SWATCH_GAP: f32 = 6.0;
const DONE: f32 = 34.0;

const ROW_FILL: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.03));
const ROW_HOVER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.06));
const FIELD_FILL: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.05));
const FIELD_RIM: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.14));
const FIELD_FOCUS: Ink = Ink::Accent(0.65);
const FIELD_REFUSED: Ink = Ink::Dark(rgb(0xF2945B));
const PEACH: Ink = Ink::Dark(rgb(0xF7B98A));
const PLAIN: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.06));
const PLAIN_HOVER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.10));
const GREEN_HOVER: Ink = Ink::AccentLit;
const BOX_BORDER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.38));
/// A featured calendar's star: the eight's yellow.
const STAR: Ink = Ink::Dark(rgb(0xF5C46E));

/// What can be acted on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Back,
    Close,
    /// Google's row with no account: Online Accounts' panel.
    Online,
    Account(usize),
    Planka,
    Field(Field),
    Cancel,
    SignIn,
    Calendar(usize),
    Swatch(usize),
    /// A calendar's star: featured or not.
    Star(usize),
    Colour(usize, usize),
    Board,
    BoardOption(usize),
    Choice(usize),
    Option(usize, usize),
    Done,
}

impl Target {
    /// Its control's id: what tests, and screen readers, know it by.
    fn id(self) -> String {
        format!("setup:{self:?}").replace(' ', "")
    }

    /// Whether it is a list's row, which the arrows go through.
    fn row(self) -> bool {
        matches!(
            self,
            Self::Online
                | Self::Account(_)
                | Self::Planka
                | Self::Calendar(_)
                | Self::Colour(..)
                | Self::BoardOption(_)
                | Self::Choice(_)
                | Self::Option(..)
        )
    }
}

/// A button's look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Look {
    Plain,
    Green,
}

/// What the popover draws, from its top left.
#[derive(Debug, Clone, PartialEq)]
enum Part {
    /// Step 1's row of a kind.
    Kind {
        rect: Rectangle,
        target: Option<Target>,
        letter: char,
        tint: Color,
        name: &'static str,
        lines: Vec<String>,
        dim: bool,
        chevron: bool,
    },
    /// An account under Google's row.
    Account {
        rect: Rectangle,
        target: Target,
        name: String,
    },
    Text {
        lines: Vec<String>,
        top: f32,
        size: f32,
        line: f32,
        colour: Color,
        bold: bool,
    },
    Caption {
        text: &'static str,
        top: f32,
    },
    Field {
        field: Field,
        rect: Rectangle,
        refused: bool,
    },
    Button {
        rect: Rectangle,
        target: Option<Target>,
        word: &'static str,
        look: Look,
        busy: bool,
    },
    Calendar {
        index: usize,
        rect: Rectangle,
        name: String,
        colour: usize,
        shown: bool,
        featured: bool,
        read_only: bool,
    },
    Colours {
        index: usize,
        top: f32,
        chosen: usize,
    },
    Choice {
        target: Target,
        rect: Rectangle,
        label: &'static str,
        value: String,
    },
    Option {
        target: Target,
        rect: Rectangle,
        name: String,
        chosen: bool,
    },
}

/// The popover laid out.
#[derive(Debug, Clone, PartialEq, Default)]
struct Laid {
    step: Option<Step>,
    title: &'static str,
    back: bool,
    parts: Vec<Part>,
    /// What can be acted on and where, in `Tab`'s order.
    targets: Vec<(Target, Rectangle)>,
    end: f32,
    /// What it shows, for the log.
    said: String,
}

impl Laid {
    fn target(&mut self, target: Target, rect: Rectangle) {
        self.targets.push((target, rect));
    }
}

/// Where the tiles' letters are: Google's blue, Planka's green.
const GOOGLE_TINT: Ink = BLUE;
const PLANKA_TINT: Ink = GREEN;

/// The popover for `flow` as it is, its text measured with `width`.
fn lay_out(flow: &setup::Flow, words: Words, width: impl Fn(&str, f32, iced::Font) -> f32) -> Laid {
    let mut laid = Laid {
        step: Some(flow.step),
        back: flow.step != Step::Accounts,
        title: match flow.step {
            Step::Accounts => words.add_account(),
            Step::Planka => words.planka(),
            Step::Show => words.what_to_show(),
        },
        ..Laid::default()
    };
    if laid.back {
        laid.target(Target::Back, back_rect());
    }
    let mut y = HEADER_TOP + HEADER + GAP;
    let wrap =
        |text: &str, size: f32, room: f32| kit::wrap(text, room, |t| width(t, size, MANROPE));
    match flow.step {
        Step::Accounts => {
            let accounts = flow.accounts();
            let room = CONTENT - NAME_X - 30.0;
            let (line, target, dim) = match &flow.online {
                Online::Unknown => (words.google_line(), None, false),
                Online::Missing => (words.google_missing(), None, true),
                Online::Listed(_) if accounts.is_empty() => (
                    words.google_none(),
                    flow.panel.is_some().then_some(Target::Online),
                    flow.panel.is_none(),
                ),
                Online::Listed(_) => (words.google_line(), None, false),
            };
            let lines = wrap(line, LINE_TEXT, room);
            let head = kind_height(lines.len());
            let height = if accounts.is_empty() {
                head
            } else {
                head + accounts.len() as f32 * SUB + 4.0
            };
            let rect = Rectangle::new(Point::new(LEFT, y), Size::new(CONTENT, height));
            laid.parts.push(Part::Kind {
                rect,
                target,
                letter: 'G',
                tint: GOOGLE_TINT.get(),
                name: words.google(),
                lines,
                dim,
                chevron: target.is_some(),
            });
            if let Some(target) = target {
                laid.target(target, rect);
            }
            let mut sub = y + head;
            for (i, account) in accounts.iter().enumerate() {
                let rect =
                    Rectangle::new(Point::new(LEFT + 4.0, sub), Size::new(CONTENT - 8.0, SUB));
                laid.parts.push(Part::Account {
                    rect,
                    target: Target::Account(i),
                    name: account.name.clone(),
                });
                laid.target(Target::Account(i), rect);
                sub += SUB;
            }
            y += height + ROW_GAP;
            let lines = wrap(words.planka_line(), LINE_TEXT, room);
            let height = kind_height(lines.len());
            let rect = Rectangle::new(Point::new(LEFT, y), Size::new(CONTENT, height));
            laid.parts.push(Part::Kind {
                rect,
                target: Some(Target::Planka),
                letter: 'P',
                tint: PLANKA_TINT.get(),
                name: words.planka(),
                lines,
                dim: false,
                chevron: true,
            });
            laid.target(Target::Planka, rect);
            y += height + GAP;
            let lines = wrap(words.local_note(), LINE_TEXT, CONTENT);
            let n = lines.len() as f32;
            laid.parts.push(Part::Text {
                lines,
                top: y,
                size: LINE_TEXT,
                line: LINE,
                colour: FAINT.get(),
                bold: false,
            });
            y += n * LINE;
            let online = match &flow.online {
                Online::Unknown => "unknown".to_owned(),
                Online::Missing => "missing".to_owned(),
                Online::Listed(_) => format!(
                    "[{}]",
                    accounts
                        .iter()
                        .map(|a| a.identity.as_str())
                        .collect::<Vec<_>>()
                        .join("; ")
                ),
            };
            laid.said = format!(
                "step=accounts google=«{line}» accounts={online} panel={} disabled={dim}",
                flow.panel.is_some()
            );
        }
        Step::Planka => {
            for (field, label) in [
                (Field::Address, words.board_address()),
                (Field::Token, words.token()),
            ] {
                laid.parts.push(Part::Text {
                    lines: vec![label.to_owned()],
                    top: y,
                    size: LABEL,
                    line: LABEL_HEIGHT,
                    colour: DIM.get(),
                    bold: true,
                });
                y += LABEL_HEIGHT + 6.0;
                let rect = Rectangle::new(Point::new(LEFT, y), Size::new(CONTENT, FIELD));
                // The field what went wrong is about.
                let refused = matches!(
                    (field, flow.refused),
                    (Field::Token, Some(Refusal::Token))
                        | (
                            Field::Address,
                            Some(Refusal::Address | Refusal::NoServer | Refusal::NotShared)
                        )
                );
                laid.parts.push(Part::Field {
                    field,
                    rect,
                    refused,
                });
                laid.target(Target::Field(field), rect);
                y += FIELD;
                if field == Field::Address {
                    y += GAP;
                }
            }
            y += 6.0;
            let help = wrap(words.token_help(), LINE_TEXT, CONTENT);
            let n = help.len() as f32;
            laid.parts.push(Part::Text {
                lines: help,
                top: y,
                size: LINE_TEXT,
                line: LINE,
                colour: FAINT.get(),
                bold: false,
            });
            y += n * LINE;
            if let Some(refused) = flow.refused {
                y += 8.0;
                let lines = wrap(words.refusal(refused), ERROR, CONTENT);
                let n = lines.len() as f32;
                laid.parts.push(Part::Text {
                    lines,
                    top: y,
                    size: ERROR,
                    line: ERROR_LINE,
                    colour: PEACH.get(),
                    bold: false,
                });
                y += n * ERROR_LINE;
            }
            y += GAP;
            let half = (CONTENT - 8.0) / 2.0;
            let cancel = Rectangle::new(Point::new(LEFT, y), Size::new(half, BUTTON));
            let sign_in = Rectangle::new(Point::new(RIGHT - half, y), Size::new(half, BUTTON));
            let busy = flow.checking.is_some();
            let ready = !flow.address.text().trim().is_empty() && !flow.token.is_empty();
            laid.parts.push(Part::Button {
                rect: cancel,
                target: Some(Target::Cancel),
                word: words.cancel(),
                look: Look::Plain,
                busy: false,
            });
            laid.target(Target::Cancel, cancel);
            let target = (ready && !busy).then_some(Target::SignIn);
            laid.parts.push(Part::Button {
                rect: sign_in,
                target,
                word: if busy {
                    words.signing_in()
                } else {
                    words.sign_in()
                },
                look: Look::Green,
                busy,
            });
            if let Some(target) = target {
                laid.target(target, sign_in);
            }
            y += BUTTON;
            laid.said = format!(
                "step=planka address=«{}» token={} checking={busy} refused={}",
                flow.address.text(),
                if flow.token.is_empty() {
                    "none"
                } else {
                    "typed"
                },
                flow.refused
                    .map_or("none".to_owned(), |r| format!("«{}»", words.refusal(r)))
            );
        }
        Step::Show => {
            let who = match &flow.show {
                Some(Show::Google { account, .. }) => {
                    format!("{} · {}", account.name, words.google())
                }
                Some(Show::Planka { found, .. }) => {
                    format!("{} · {}", found.user, found.server.host())
                }
                None => String::new(),
            };
            let who = kit::elide(&who, CONTENT, |t| width(t, 12.5, MANROPE));
            laid.parts.push(Part::Text {
                lines: vec![who.clone()],
                top: y,
                size: 12.5,
                line: 18.0,
                colour: DIM.get(),
                bold: false,
            });
            y += 18.0 + GAP;
            let mut ready = false;
            match &flow.show {
                Some(Show::Google { calendars, .. }) => {
                    laid.parts.push(Part::Caption {
                        text: words.calendars(),
                        top: y,
                    });
                    y += CAPTION_HEIGHT + 4.0;
                    let mut rows_said = Vec::new();
                    match calendars {
                        Calendars::Reading(_) => {
                            laid.parts.push(Part::Text {
                                lines: vec![words.reading().to_owned()],
                                top: y + (CHECK_ROW - 18.0) / 2.0,
                                size: 13.0,
                                line: 18.0,
                                colour: DIM.get(),
                                bold: false,
                            });
                            y += CHECK_ROW;
                            rows_said.push("reading".to_owned());
                        }
                        Calendars::Failed => {
                            let lines = wrap(words.unread(), ERROR, CONTENT);
                            let n = lines.len() as f32;
                            laid.parts.push(Part::Text {
                                lines,
                                top: y,
                                size: ERROR,
                                line: ERROR_LINE,
                                colour: PEACH.get(),
                                bold: false,
                            });
                            y += n * ERROR_LINE;
                            rows_said.push("failed".to_owned());
                        }
                        Calendars::Read(rows) => {
                            ready = true;
                            for (i, row) in rows.iter().enumerate() {
                                let rect = Rectangle::new(
                                    Point::new(LEFT - 6.0, y),
                                    Size::new(CONTENT + 12.0, CHECK_ROW),
                                );
                                laid.parts.push(Part::Calendar {
                                    index: i,
                                    rect,
                                    name: row.name.clone(),
                                    colour: row.colour,
                                    shown: row.shown,
                                    featured: row.featured,
                                    read_only: row.read_only,
                                });
                                laid.target(Target::Calendar(i), rect);
                                laid.target(Target::Swatch(i), swatch_rect(y));
                                laid.target(Target::Star(i), star_rect(y));
                                rows_said.push(format!(
                                    "{} {} {} {}{}",
                                    if row.shown { "[x]" } else { "[ ]" },
                                    if row.featured { "★" } else { "☆" },
                                    row.name,
                                    PALETTE[row.colour],
                                    if row.read_only {
                                        format!(" {}", words.read_only())
                                    } else {
                                        String::new()
                                    }
                                ));
                                y += CHECK_ROW;
                                if flow.unfolded == Some(Unfold::Colours(i)) {
                                    laid.parts.push(Part::Colours {
                                        index: i,
                                        top: y,
                                        chosen: row.colour,
                                    });
                                    for c in 0..PALETTE.len() {
                                        laid.target(Target::Colour(i, c), colour_rect(y, c));
                                    }
                                    y += SWATCH_ROW;
                                    rows_said.push(format!("colours of {}", row.name));
                                }
                            }
                        }
                    }
                    laid.said = format!(
                        "step=show account=«{who}» calendars=[{}] star=«{}»",
                        rows_said.join("; "),
                        words.feature()
                    );
                }
                Some(Show::Planka {
                    found,
                    columns,
                    reading,
                }) => {
                    laid.parts.push(Part::Caption {
                        text: words.tasks(),
                        top: y,
                    });
                    y += CAPTION_HEIGHT + 4.0;
                    let mut said = Vec::new();
                    if found.board.is_none() || !found.boards.is_empty() {
                        let chosen = found.board.as_ref().map(|b| b.board.id.as_str().to_owned());
                        let value = found
                            .board
                            .as_ref()
                            .map_or_else(|| words.choose().to_owned(), |b| b.board.name.clone());
                        let rect = Rectangle::new(
                            Point::new(LEFT - 6.0, y),
                            Size::new(CONTENT + 12.0, CHECK_ROW),
                        );
                        laid.parts.push(Part::Choice {
                            target: Target::Board,
                            rect,
                            label: words.board(),
                            value: value.clone(),
                        });
                        laid.target(Target::Board, rect);
                        said.push(format!("board «{value}»"));
                        y += CHECK_ROW;
                        if flow.unfolded == Some(Unfold::Boards) {
                            for (i, board) in found.boards.iter().enumerate() {
                                let rect = option_rect(y);
                                let name = if board.project.is_empty() {
                                    board.name.clone()
                                } else {
                                    format!("{} › {}", board.project, board.name)
                                };
                                laid.parts.push(Part::Option {
                                    target: Target::BoardOption(i),
                                    rect,
                                    name: name.clone(),
                                    chosen: chosen.as_deref() == Some(board.id.as_str()),
                                });
                                laid.target(Target::BoardOption(i), rect);
                                said.push(format!("option «{name}»"));
                                y += OPTION;
                            }
                        }
                    } else if let Some(board) = &found.board {
                        laid.parts.push(Part::Text {
                            lines: vec![board.board.name.clone()],
                            top: y,
                            size: NAME,
                            line: 22.0,
                            colour: TEXT.get(),
                            bold: true,
                        });
                        said.push(format!("board «{}»", board.board.name));
                        y += 22.0;
                    }
                    if reading.is_some() {
                        laid.parts.push(Part::Text {
                            lines: vec![words.signing_in().to_owned()],
                            top: y + 7.0,
                            size: 13.0,
                            line: 18.0,
                            colour: DIM.get(),
                            bold: false,
                        });
                        y += CHECK_ROW;
                    } else if let Some(board) = &found.board {
                        ready = !board.board.columns.is_empty();
                        let name = |i: usize| {
                            board
                                .board
                                .columns
                                .get(i)
                                .map(|c| c.name.clone())
                                .unwrap_or_default()
                        };
                        for (which, column) in columns.iter().enumerate().take(CHOICES) {
                            let rect = Rectangle::new(
                                Point::new(LEFT - 6.0, y),
                                Size::new(CONTENT + 12.0, CHECK_ROW),
                            );
                            laid.parts.push(Part::Choice {
                                target: Target::Choice(which),
                                rect,
                                label: words.choice(which),
                                value: name(*column),
                            });
                            laid.target(Target::Choice(which), rect);
                            said.push(format!("{} «{}»", words.choice(which), name(*column)));
                            y += CHECK_ROW;
                            if flow.unfolded == Some(Unfold::Columns(which)) {
                                for (i, c) in board.board.columns.iter().enumerate() {
                                    let rect = option_rect(y);
                                    laid.parts.push(Part::Option {
                                        target: Target::Option(which, i),
                                        rect,
                                        name: c.name.clone(),
                                        chosen: i == *column,
                                    });
                                    laid.target(Target::Option(which, i), rect);
                                    said.push(format!("option «{}»", c.name));
                                    y += OPTION;
                                }
                            }
                        }
                        let count = words.cards(board.cards.get(columns[0]).copied().unwrap_or(0));
                        laid.parts.push(Part::Text {
                            lines: vec![count.clone()],
                            top: y + 4.0,
                            size: LINE_TEXT,
                            line: LINE,
                            colour: DIM.get(),
                            bold: false,
                        });
                        said.push(count);
                        y += 4.0 + LINE;
                    }
                    laid.said = format!("step=show account=«{who}» tasks=[{}]", said.join("; "));
                }
                None => {}
            }
            y += GAP;
            let rect = Rectangle::new(Point::new(LEFT, y), Size::new(CONTENT, DONE));
            let target = ready.then_some(Target::Done);
            laid.parts.push(Part::Button {
                rect,
                target,
                word: words.done(),
                look: Look::Green,
                busy: false,
            });
            if let Some(target) = target {
                laid.target(target, rect);
            }
            laid.said.push_str(&format!(" done={ready}"));
            y += DONE;
        }
    }
    laid.target(Target::Close, close_rect());
    laid.end = y;
    laid
}

/// A step 1 row's height for its line's `lines`: 54 px for one.
fn kind_height(lines: usize) -> f32 {
    (30.0 + lines.max(1) as f32 * LINE + 8.0).max(ROW)
}

fn back_rect() -> Rectangle {
    Rectangle::new(
        Point::new(LEFT - 4.0, HEADER_TOP + (HEADER - BUTTON_SQUARE) / 2.0),
        Size::new(BUTTON_SQUARE, BUTTON_SQUARE),
    )
}

fn close_rect() -> Rectangle {
    Rectangle::new(
        Point::new(
            RIGHT + 4.0 - BUTTON_SQUARE,
            HEADER_TOP + (HEADER - BUTTON_SQUARE) / 2.0,
        ),
        Size::new(BUTTON_SQUARE, BUTTON_SQUARE),
    )
}

/// A calendar's swatch, its row from `top`: after its box, 10 px apart.
fn swatch_rect(top: f32) -> Rectangle {
    Rectangle::new(
        Point::new(LEFT + BOX + 10.0 - 4.0, top + (CHECK_ROW - BOX) / 2.0 - 4.0),
        Size::new(BOX + 8.0, BOX + 8.0),
    )
}

/// A calendar's star, its row from `top`: after its swatch, 10 px apart.
fn star_rect(top: f32) -> Rectangle {
    let swatch = swatch_rect(top);
    Rectangle::new(Point::new(swatch.x + BOX + 10.0, swatch.y), swatch.size())
}

/// Colour `c` of the eight, their row from `top`, under the swatch.
fn colour_rect(top: f32, c: usize) -> Rectangle {
    Rectangle::new(
        Point::new(
            LEFT + BOX + 10.0 + c as f32 * (SWATCH + SWATCH_GAP),
            top + (SWATCH_ROW - SWATCH) / 2.0,
        ),
        Size::new(SWATCH, SWATCH),
    )
}

fn option_rect(top: f32) -> Rectangle {
    Rectangle::new(
        Point::new(LEFT + 6.0, top),
        Size::new(CONTENT - 6.0, OPTION),
    )
}

fn measure(text: &str, size: f32, font: iced::Font) -> f32 {
    kit::measure(text, size, font)
}

/// Its height for what it shows now.
pub fn height() -> u32 {
    let words = Words::new(crate::clock::time::Lang::of_messages());
    let end = setup::with(|flow| lay_out(flow, words, measure).end).unwrap_or(120.0);
    (end + 14.0 + BORDER).round() as u32
}

/// The popover on its surface.
#[derive(Debug, Clone)]
pub struct Panel {
    pub id: window::Id,
    pub words: Words,
    pub glass: bool,
    /// Its spinner turns (not on the CPU, nor with reduced motion).
    pub motion: bool,
}

/// What it keeps while it is open.
#[derive(Debug, Default)]
struct Ui {
    focus: Option<Target>,
    /// The step last shown: a new one puts the keyboard on its first field.
    step: Option<Step>,
    said: Option<String>,
}

static UI: Mutex<Option<Ui>> = Mutex::new(None);

fn ui<T>(with: impl FnOnce(&mut Ui) -> T) -> T {
    let mut ui = UI.lock().unwrap_or_else(PoisonError::into_inner);
    with(ui.get_or_insert_with(Ui::default))
}

/// It opens: nothing holds the keyboard yet.
pub fn opening() {
    ui(|ui| *ui = Ui::default());
}

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A control pressed, or `Enter` on it.
    Act(Target),
    /// The ring comes to it (a field: the keyboard).
    Focus(Target),
    /// What a field took of the pointer, a key or an input method.
    Took(Field, Took),
    /// `Escape`: a field lets the keyboard go and the step goes back; else
    /// back a step, then closed.
    Escape,
    /// `Enter` in a field: signing in.
    Enter,
    /// `↓` (`true`) or `↑`: through a list's rows.
    Arrow(bool),
    /// A press anywhere on the popover.
    Pressed,
    /// What it shows changed under the view.
    Stale,
}

/// The popover on its surface.
pub fn view<'a>(panel: &Panel) -> Element<'a, crate::Message> {
    let (words, motion) = (panel.words, panel.motion);
    Around::new(panel.id, build(words, motion), handle, Msg::Stale)
        .rebuild(move || build(words, motion))
        .keys(key)
        .subtitle(setup::with(|flow| title_of(flow.step, words).to_owned()))
        .press(Msg::Pressed)
        .on_glass(panel.glass)
}

fn title_of(step: Step, words: Words) -> &'static str {
    match step {
        Step::Accounts => words.add_account(),
        Step::Planka => words.planka(),
        Step::Show => words.what_to_show(),
    }
}

/// Its keys before the kit's: `Escape`; in a field, `Enter`; the arrows
/// through a list's rows.
fn key(pressed: &Pressed) -> Option<Msg> {
    use keyboard::{Key as K, key::Named};
    let in_field = ui(|ui| matches!(ui.focus, Some(Target::Field(_))));
    match &pressed.key {
        K::Named(Named::Escape) => Some(Msg::Escape),
        K::Named(Named::Enter) if in_field => Some(Msg::Enter),
        K::Named(Named::ArrowDown) if !in_field => Some(Msg::Arrow(true)),
        K::Named(Named::ArrowUp) if !in_field => Some(Msg::Arrow(false)),
        _ => None,
    }
}

/// Its content, as the flow is now.
fn build<'a>(words: Words, motion: bool) -> El<'a> {
    let Some(laid) = setup::with(|flow| lay_out(flow, words, measure)) else {
        return Space::new().into();
    };
    ui(|ui| {
        // A new step: the keyboard on its first field, else on nothing.
        if laid.step != ui.step {
            ui.focus = match laid.step {
                Some(Step::Planka) => Some(Target::Field(Field::Address)),
                _ => None,
            };
            ui.step = laid.step;
        }
        if let Some(focus) = ui.focus
            && !laid.targets.iter().any(|(t, _)| *t == focus)
        {
            ui.focus = None;
        }
        say(&laid, words, ui);
        content(&laid, words, ui.focus, motion)
    })
}

/// What it shows, for the log, when it changed.
fn say(laid: &Laid, words: Words, ui: &mut Ui) {
    // The header's buttons' names, for screen readers (§11).
    let names = if laid.back {
        format!("«{}» «{}»", words.back(), words.close())
    } else {
        format!("«{}»", words.close())
    };
    let said = format!(
        "{} focus={} title=«{}» buttons={names}",
        laid.said,
        ui.focus.map_or("none".to_owned(), |t| format!("{t:?}")),
        laid.title,
    );
    if ui.said.as_ref() != Some(&said) {
        tracing::debug!("the sources' popover shows it: {said}");
        ui.said = Some(said);
    }
}

/// `content` with its top left corner at `point`.
fn at<'a>(content: impl Into<El<'a>>, point: Point) -> El<'a> {
    pin(content).x(point.x).y(point.y).into()
}

/// A text's line, `size` px in `font`, on `middle`, from `x` on the side
/// `align` says, within `room` px that way.
fn text_at<'a>(
    content: impl Into<String>,
    (size, font, ink): (f32, iced::Font, Color),
    (x, middle): (f32, f32),
    align: Horizontal,
) -> El<'a> {
    let height = size * EM;
    let width = WIDTH as f32;
    let (left, room) = match align {
        Horizontal::Right => (0.0, x),
        _ => (x, width - x),
    };
    at(
        container(said(content.into(), size, font, ink))
            .width(room)
            .height(height)
            .align_x(align)
            .align_y(Vertical::Center),
        Point::new(left, middle - height / 2.0),
    )
}

/// A button over `rect`'s size, `face` on it, lit under the pointer with
/// `lit` (a fill, or under high contrast an edge).
fn pressable<'a>(
    face: impl Into<El<'a>>,
    rect: Rectangle,
    target: Target,
    fill: Option<Ink>,
    lit: Ink,
    radius: f32,
) -> button::Button<'a, Msg> {
    button(face)
        .padding(0)
        .width(rect.width)
        .height(rect.height)
        .style(move |_, status| {
            if style::lit(status) {
                style::hovered(lit.get(), radius)
            } else {
                match fill {
                    Some(fill) => style::filled(fill.get(), radius, TEXT.get()),
                    None => style::clear(),
                }
            }
        })
        .on_press(Msg::Act(target))
}

/// Control `target`, its ring of `radius` when it holds it.
fn declared<'a>(
    widget: impl Into<El<'a>>,
    target: Target,
    role: Role,
    name: impl Into<String>,
    focus: Option<Target>,
    radius: f32,
) -> ui::control::Control<'a, Msg> {
    control(widget, target.id(), role, name)
        .focus(focus == Some(target), Msg::Focus(target), radius)
        .activate(Msg::Act(target))
        .node(|n| n)
}

fn content<'a>(laid: &Laid, words: Words, focus: Option<Target>, motion: bool) -> El<'a> {
    let mut layers: Vec<El<'a>> = Vec::new();
    // The header.
    let title_x = if laid.back {
        let rect = back_rect();
        let face = drawn(rect.width, rect.height, |f, s| {
            arrow(f, Point::new(s.width / 2.0, s.height / 2.0));
        });
        layers.push(at(
            declared(
                pressable(face, rect, Target::Back, None, ROW_HOVER, 7.0),
                Target::Back,
                Role::Button,
                words.back(),
                focus,
                7.0,
            ),
            rect.position(),
        ));
        LEFT + BUTTON_SQUARE + 4.0
    } else {
        LEFT
    };
    layers.push(text_at(
        laid.title,
        (TITLE, MANROPE_BOLD, TEXT.get()),
        (title_x, HEADER_TOP + HEADER / 2.0),
        Horizontal::Left,
    ));
    let mut caption: Option<&str> = None;
    for part in &laid.parts {
        part_of(&mut layers, part, words, focus, motion, &mut caption);
    }
    // ✕ last, as `Tab` goes.
    let rect = close_rect();
    let face = drawn(rect.width, rect.height, |f, s| {
        cross(f, Point::new(s.width / 2.0, s.height / 2.0));
    });
    layers.push(at(
        declared(
            pressable(face, rect, Target::Close, None, ROW_HOVER, 7.0),
            Target::Close,
            Role::Button,
            words.close(),
            focus,
            7.0,
        ),
        rect.position(),
    ));
    Stack::with_children(layers)
        .width(WIDTH as f32)
        .height(laid.end + 14.0 + BORDER)
        .into()
}

/// The entries of the Planka step's fields, to the kit's field.
fn address(with: &mut dyn FnMut(&mut field::Entry)) {
    setup::with(|flow| with(flow.entry(Field::Address)));
}

fn token(with: &mut dyn FnMut(&mut field::Entry)) {
    setup::with(|flow| with(flow.entry(Field::Token)));
}

#[allow(clippy::too_many_lines)]
fn part_of<'a>(
    layers: &mut Vec<El<'a>>,
    part: &Part,
    words: Words,
    focus: Option<Target>,
    motion: bool,
    caption: &mut Option<&'static str>,
) {
    let fr = words.back() == "Retour";
    match part {
        Part::Kind {
            rect,
            target,
            letter,
            tint,
            name,
            lines,
            dim,
            chevron,
        } => {
            let alpha = if *dim { 0.5 } else { 1.0 };
            let tint = *tint;
            let letter = letter.to_string();
            let tile = container(said(letter, 15.0, MANROPE_BOLD, faded(tint, alpha)))
                .center(TILE)
                .style(move |_| style::boxed(faded(Color { a: 0.16, ..tint }, alpha), 8.0));
            let mut face: Vec<El<'a>> = vec![
                at(tile, Point::new(TILE_IN, TILE_IN)),
                text_at(
                    *name,
                    (NAME, kit::MANROPE_SEMIBOLD, faded(TEXT.get(), alpha)),
                    (NAME_X, 19.0),
                    Horizontal::Left,
                ),
            ];
            for (i, line) in lines.iter().enumerate() {
                face.push(text_at(
                    line.clone(),
                    (LINE_TEXT, MANROPE, faded(DIM.get(), alpha)),
                    (NAME_X, 30.0 + LINE * (i as f32 + 0.5)),
                    Horizontal::Left,
                ));
            }
            if *chevron {
                face.push(at(
                    drawn(CHEVRON, CHEVRON, |f, s| {
                        chevron_right(f, Point::new(s.width / 2.0, s.height / 2.0));
                    }),
                    Point::new(rect.width - 16.0 - CHEVRON / 2.0, ROW / 2.0 - CHEVRON / 2.0),
                ));
            }
            let face = Stack::with_children(face)
                .width(rect.width)
                .height(rect.height);
            let said_lines = lines.join(" ");
            let shown: El<'a> = match target {
                Some(target) => declared(
                    pressable(face, *rect, *target, Some(ROW_FILL), ROW_HOVER, ROW_RADIUS),
                    *target,
                    Role::Button,
                    *name,
                    focus,
                    ROW_RADIUS,
                )
                .node(|n| n.describe(said_lines))
                .into(),
                None => ui::line(
                    container(face).style(|_| style::boxed(ROW_FILL.get(), ROW_RADIUS)),
                    format!("{name}, {said_lines}"),
                )
                .into(),
            };
            layers.push(at(shown, rect.position()));
        }
        Part::Account { rect, target, name } => {
            let room = rect.width - (NAME_X - 4.0) - 28.0;
            let shown = kit::elide(name, room, |t| measure(t, 13.0, MANROPE));
            let face = Stack::with_children([
                text_at(
                    shown,
                    (13.0, MANROPE, TEXT.get()),
                    (NAME_X - 4.0, rect.height / 2.0),
                    Horizontal::Left,
                ),
                at(
                    drawn(CHEVRON, CHEVRON, |f, s| {
                        chevron_right(f, Point::new(s.width / 2.0, s.height / 2.0));
                    }),
                    Point::new(
                        rect.width - 12.0 - CHEVRON / 2.0,
                        (rect.height - CHEVRON) / 2.0,
                    ),
                ),
            ])
            .width(rect.width)
            .height(rect.height);
            layers.push(at(
                declared(
                    pressable(face, *rect, *target, None, ROW_HOVER, 8.0),
                    *target,
                    Role::Button,
                    name.clone(),
                    focus,
                    8.0,
                ),
                rect.position(),
            ));
        }
        Part::Text {
            lines,
            top,
            size,
            line,
            colour,
            bold,
        } => {
            let font = if *bold {
                kit::MANROPE_SEMIBOLD
            } else {
                MANROPE
            };
            let mut all: Vec<El<'a>> = Vec::new();
            for (i, said_line) in lines.iter().enumerate() {
                all.push(text_at(
                    said_line.clone(),
                    (*size, font, *colour),
                    (LEFT, top + line * (i as f32 + 0.5)),
                    Horizontal::Left,
                ));
            }
            layers.push(ui::line(Stack::with_children(all), lines.join(" ")).into());
        }
        Part::Caption { text, top } => {
            *caption = Some(text);
            layers.push(at(
                ui::line(
                    container(spaced(text, kit::LABEL, DIM.get(), f32::INFINITY))
                        .height(CAPTION_HEIGHT)
                        .align_y(Vertical::Center),
                    *text,
                ),
                Point::new(LEFT, *top),
            ));
            let _ = CAPTION;
        }
        Part::Field {
            field,
            rect,
            refused,
        } => {
            let target = Target::Field(*field);
            let focused = focus == Some(target);
            let rim = if *refused {
                FIELD_REFUSED
            } else if focused {
                FIELD_FOCUS
            } else {
                FIELD_RIM
            };
            let (with, placeholder, role): (EntryOf, _, _) = match field {
                Field::Address => (address, "https://…/boards/…", Role::TextField),
                Field::Token => (token, "accessToken=…; httpOnlyToken=…", Role::PasswordField),
            };
            let field = *field;
            let typed = ui::field::Field {
                with: Box::new(with),
                style: Style::one(FIELD_TEXT, MANROPE),
                placeholder: placeholder.into(),
                look: field::view::Look::of(TEXT.get(), FAINT.get()),
                focused,
                left: FIELD_PAD,
                right: FIELD_PAD,
                on_focus: Msg::Focus(target),
                on_took: match field {
                    Field::Address => |took| Msg::Took(Field::Address, took),
                    Field::Token => |took| Msg::Took(Field::Token, took),
                },
            }
            .view(FIELD);
            let boxed = container(typed)
                .width(rect.width)
                .style(move |_| container::Style {
                    background: Some(FIELD_FILL.get().into()),
                    border: iced::Border {
                        color: rim.get(),
                        width: 1.0,
                        radius: FIELD_RADIUS.into(),
                    },
                    ..container::Style::default()
                });
            let value = match field {
                Field::Address => setup::with(|flow| flow.address.text().to_owned()),
                Field::Token => None,
            };
            let refused = *refused;
            layers.push(at(
                control(boxed, target.id(), role, caption.unwrap_or_default())
                    .node(|n| {
                        let n = match value {
                            Some(value) => n.value(value),
                            None => n,
                        };
                        if refused {
                            n.describe(if fr { "Refusé" } else { "Refused" })
                        } else {
                            n
                        }
                    })
                    .focus(focused, Msg::Focus(target), FIELD_RADIUS)
                    .typing(),
                rect.position(),
            ));
        }
        Part::Button {
            rect,
            target,
            word,
            look,
            busy,
        } => {
            let (target, busy) = (*target, *busy);
            let enabled = target.is_some() || busy;
            let (fill, lit, ink, font) = match look {
                Look::Plain => (PLAIN, PLAIN_HOVER, DIM, kit::MANROPE_SEMIBOLD),
                Look::Green => (GREEN, GREEN_HOVER, INK, MANROPE_BOLD),
            };
            let alpha = if enabled { 1.0 } else { 0.45 };
            let mut face = iced::widget::row![].align_y(Vertical::Center);
            if busy {
                let spin = moving(
                    12.0,
                    12.0,
                    move |f, _, _| spinner(f, Point::new(1.0, 6.0), ink.get(), motion),
                    move |_| motion,
                );
                face = face.push(spin).push(Space::new().width(6.0));
            }
            face = face.push(said(*word, BUTTON_TEXT, font, faded(ink.get(), alpha)));
            let face = container(face).center(Length::Fill);
            let style = move |status| {
                let fill = if target.is_some() && style::lit(status) {
                    lit
                } else {
                    fill
                };
                style::filled(faded(fill.get(), alpha), BUTTON_RADIUS, ink.get())
            };
            let pressed = button(face)
                .padding(0)
                .width(rect.width)
                .height(rect.height)
                .style(move |_, status| style(status))
                .on_press_maybe(target.map(Msg::Act));
            let shown: El<'a> = match target {
                Some(t) => declared(pressed, t, Role::Button, *word, focus, BUTTON_RADIUS)
                    .node(|n| n.disabled(busy))
                    .into(),
                None => ui::line(pressed, *word).into(),
            };
            layers.push(at(shown, rect.position()));
        }
        Part::Calendar {
            index,
            rect,
            name,
            colour,
            shown,
            featured,
            read_only,
        } => {
            let (index, colour, shown, featured) = (*index, *colour, *shown, *featured);
            let middle = rect.height / 2.0;
            let swatch = swatch_rect(rect.y);
            let star = star_rect(rect.y);
            let x = star.x + star.width - 4.0 + 10.0 - rect.x;
            let aside = read_only.then(|| setup::Words::read_only(words));
            let aside_width = aside.map_or(0.0, |a| measure(a, 11.5, MANROPE) + 8.0);
            let room = RIGHT - rect.x - x - aside_width;
            let shown_name = kit::elide(name, room, |t| measure(t, 13.0, MANROPE));
            let mut face: Vec<El<'a>> = vec![
                at(
                    drawn(BOX, BOX, move |f, _| tick(f, Point::ORIGIN, shown)),
                    Point::new(LEFT - rect.x, middle - BOX / 2.0),
                ),
                text_at(
                    shown_name,
                    (13.0, MANROPE, if shown { TEXT.get() } else { DIM.get() }),
                    (x, middle),
                    Horizontal::Left,
                ),
            ];
            if let Some(aside) = aside {
                face.push(text_at(
                    aside,
                    (11.5, MANROPE, FAINT.get()),
                    (RIGHT - rect.x, middle),
                    Horizontal::Right,
                ));
            }
            let face = Stack::with_children(face)
                .width(rect.width)
                .height(rect.height);
            let target = Target::Calendar(index);
            let mut row = declared(
                pressable(face, *rect, target, None, ROW_HOVER, 8.0),
                target,
                Role::CheckBox,
                name.clone(),
                focus,
                8.0,
            )
            .node(|n| n.checked(shown));
            if *read_only {
                row = row.node(|n| n.describe(if fr { "lecture seule" } else { "read only" }));
            }
            layers.push(at(row, rect.position()));
            // Its swatch, then its star, over the row.
            let tint = PALETTE_COLOURS[colour];
            let face = drawn_lit(swatch.width, swatch.height, 0.0, move |f, s, lit| {
                let inner = Rectangle::new(Point::new(4.0, 4.0), Size::new(BOX, BOX));
                f.fill(&kit::rounded(inner, 4.0), tint);
                if lit {
                    f.stroke(
                        &kit::rounded(Rectangle::new(Point::ORIGIN, s), 6.0),
                        Stroke::default().with_color(DIM.get()).with_width(1.0),
                    );
                }
            });
            let word = if fr { "Couleur" } else { "Colour" };
            let swatch_target = Target::Swatch(index);
            layers.push(at(
                declared(
                    pressable(
                        face,
                        swatch,
                        swatch_target,
                        None,
                        Ink::Dark(Color::TRANSPARENT),
                        6.0,
                    ),
                    swatch_target,
                    Role::Button,
                    format!("{word} {}, {name}", colour + 1),
                    focus,
                    6.0,
                ),
                swatch.position(),
            ));
            let face = drawn_lit(star.width, star.height, 0.0, move |f, s, lit| {
                // A star on a calendar not shown counts for nothing: dim.
                star_mark(
                    f,
                    Point::new(s.width / 2.0, s.height / 2.0),
                    featured,
                    shown,
                );
                if lit {
                    f.stroke(
                        &kit::rounded(Rectangle::new(Point::ORIGIN, s), 6.0),
                        Stroke::default().with_color(DIM.get()).with_width(1.0),
                    );
                }
            });
            let star_target = Target::Star(index);
            layers.push(at(
                declared(
                    pressable(
                        face,
                        star,
                        star_target,
                        None,
                        Ink::Dark(Color::TRANSPARENT),
                        6.0,
                    ),
                    star_target,
                    Role::ToggleButton,
                    format!("{} {name}", words.feature()),
                    focus,
                    6.0,
                )
                .node(|n| n.checked(featured)),
                star.position(),
            ));
        }
        Part::Colours { index, top, chosen } => {
            let word = if fr { "Couleur" } else { "Colour" };
            for (c, colour) in PALETTE_COLOURS.iter().enumerate() {
                let rect = colour_rect(*top, c);
                let (colour, picked) = (*colour, c == *chosen);
                let face = drawn_lit(SWATCH, SWATCH, 4.0, move |f, s, lit| {
                    let centre = Point::new(s.width / 2.0, s.height / 2.0);
                    f.fill(&Path::circle(centre, SWATCH / 2.0), colour);
                    if picked || lit {
                        f.stroke(
                            &Path::circle(centre, SWATCH / 2.0 + 2.5),
                            Stroke::default()
                                .with_color(if picked { TEXT.get() } else { DIM.get() })
                                .with_width(1.5),
                        );
                    }
                });
                let target = Target::Colour(*index, c);
                layers.push(at(
                    declared(
                        pressable(
                            face,
                            rect,
                            target,
                            None,
                            Ink::Dark(Color::TRANSPARENT),
                            SWATCH / 2.0,
                        ),
                        target,
                        Role::RadioButton,
                        format!("{word} {}", c + 1),
                        focus,
                        SWATCH / 2.0,
                    )
                    .node(|n| n.checked(picked).set(c + 1, PALETTE_COLOURS.len())),
                    rect.position(),
                ));
            }
        }
        Part::Choice {
            target,
            rect,
            label,
            value,
        } => {
            let middle = rect.height / 2.0;
            let label_width = measure(label, 13.0, MANROPE) + 12.0;
            let room = CONTENT - label_width - 18.0;
            let shown = kit::elide(value, room, |t| measure(t, 13.0, MANROPE));
            let face = Stack::with_children([
                text_at(
                    *label,
                    (13.0, MANROPE, DIM.get()),
                    (LEFT - rect.x, middle),
                    Horizontal::Left,
                ),
                text_at(
                    shown,
                    (13.0, MANROPE, TEXT.get()),
                    (RIGHT - 16.0 - rect.x, middle),
                    Horizontal::Right,
                ),
                at(
                    drawn(CHEVRON, CHEVRON, |f, s| {
                        chevron_down(f, Point::new(s.width / 2.0, s.height / 2.0));
                    }),
                    Point::new(RIGHT - 5.0 - rect.x - CHEVRON / 2.0, middle - CHEVRON / 2.0),
                ),
            ])
            .width(rect.width)
            .height(rect.height);
            layers.push(at(
                declared(
                    pressable(face, *rect, *target, None, ROW_HOVER, 8.0),
                    *target,
                    Role::Button,
                    *label,
                    focus,
                    8.0,
                )
                .node(|n| n.value(value.clone())),
                rect.position(),
            ));
        }
        Part::Option {
            target,
            rect,
            name,
            chosen,
        } => {
            let middle = rect.height / 2.0;
            let room = rect.width - 34.0;
            let shown = kit::elide(name, room, |t| measure(t, 13.0, MANROPE));
            let chosen = *chosen;
            let face = Stack::with_children([
                at(
                    drawn(12.0, 12.0, move |f, s| {
                        if chosen {
                            check(f, Point::new(0.0, s.height / 2.0), GREEN.get(), 12.0);
                        }
                    }),
                    Point::new(12.0, middle - 6.0),
                ),
                text_at(
                    shown,
                    (13.0, MANROPE, if chosen { TEXT.get() } else { DIM.get() }),
                    (26.0, middle),
                    Horizontal::Left,
                ),
            ])
            .width(rect.width)
            .height(rect.height);
            layers.push(at(
                declared(
                    pressable(face, *rect, *target, None, ROW_HOVER, 7.0),
                    *target,
                    Role::RadioButton,
                    name.clone(),
                    focus,
                    7.0,
                )
                .node(|n| n.checked(chosen)),
                rect.position(),
            ));
        }
    }
}

// --- What a message does -------------------------------------------------------

fn handle(message: Msg) {
    match message {
        Msg::Stale => {}
        Msg::Pressed => ui(|ui| {
            if !matches!(ui.focus, Some(Target::Field(_))) {
                ui.focus = None;
            }
        }),
        Msg::Focus(target) => {
            // A field takes the keyboard: the other's selection goes.
            if let Target::Field(field) = target {
                let other = match field {
                    Field::Address => Field::Token,
                    Field::Token => Field::Address,
                };
                setup::edit(other, |entry| {
                    entry.deselect();
                    Took::Same
                });
            }
            ui(|ui| ui.focus = Some(target));
        }
        // What the field took: what was refused goes with an edit, a paste
        // is asked for.
        Msg::Took(field, took) => {
            setup::edit(field, |_| took);
        }
        Msg::Escape => {
            let in_field = ui(|ui| {
                let in_field = matches!(ui.focus, Some(Target::Field(_)));
                if in_field {
                    ui.focus = None;
                }
                in_field
            });
            if !setup::back() && !in_field {
                event(Seen::Escape);
            }
        }
        Msg::Enter => setup::sign_in(),
        Msg::Arrow(down) => {
            let words = Words::new(crate::clock::time::Lang::of_messages());
            let rows: Vec<Target> = setup::with(|flow| lay_out(flow, words, measure))
                .map(|laid| {
                    laid.targets
                        .into_iter()
                        .map(|(t, _)| t)
                        .filter(|t| t.row())
                        .collect()
                })
                .unwrap_or_default();
            ui(|ui| {
                ui.focus = match ui.focus.filter(|f| f.row()) {
                    Some(focus) => kit::cycle(&rows, focus, down),
                    None => kit::step(&rows, None, !down),
                };
            });
        }
        Msg::Act(target) => act(target),
    }
    // Its height follows what it shows.
    super::refit(super::Item::DayChoose);
}

fn act(target: Target) {
    tracing::debug!(?target, "the sources' popover acts");
    match target {
        Target::Back | Target::Cancel => {
            setup::back();
        }
        Target::Close => {
            let shows = setup::with(|flow| flow.step == Step::Show).unwrap_or(false);
            if shows {
                // ✕ in step 3 keeps the account with what is ticked.
                setup::done();
            } else {
                event(Seen::Chosen);
            }
        }
        Target::Online => {
            if let Some(effect) = setup::open_panel() {
                super::send(vec![effect]);
            }
        }
        Target::Account(i) => setup::choose_google(i),
        Target::Planka => setup::choose_planka(),
        Target::Field(_) => ui(|ui| ui.focus = Some(target)),
        Target::SignIn => setup::sign_in(),
        Target::Calendar(i) => setup::toggle(i),
        Target::Swatch(i) => setup::unfold(Unfold::Colours(i)),
        Target::Star(i) => setup::feature(i),
        Target::Colour(i, c) => setup::colour(i, c),
        Target::Board => setup::unfold(Unfold::Boards),
        Target::BoardOption(i) => setup::board(i),
        Target::Choice(which) => setup::unfold(Unfold::Columns(which)),
        Target::Option(which, i) => setup::column(which, i),
        Target::Done => setup::done(),
    }
}

const PALETTE_COLOURS: [Color; 8] = {
    let mut out = [Color::BLACK; 8];
    let mut i = 0;
    while i < 8 {
        let [r, g, b] = PALETTE[i].0;
        out[i] = Color::from_rgb8(r, g, b);
        i += 1;
    }
    out
};

fn line_stroke(width: f32, colour: Color) -> Stroke<'static> {
    Stroke {
        width,
        line_cap: canvas::LineCap::Round,
        line_join: canvas::LineJoin::Round,
        ..Stroke::default().with_color(colour)
    }
}

/// The back arrow, 16 px, centred on `c`.
fn arrow(f: &mut Frame, c: Point) {
    let path = Path::new(|p| {
        p.move_to(Point::new(c.x + 6.0, c.y));
        p.line_to(Point::new(c.x - 6.0, c.y));
        p.move_to(Point::new(c.x - 1.0, c.y - 5.0));
        p.line_to(Point::new(c.x - 6.0, c.y));
        p.line_to(Point::new(c.x - 1.0, c.y + 5.0));
    });
    f.stroke(&path, line_stroke(1.8, DIM.get()));
}

/// ✕, 12 px, centred on `c`.
fn cross(f: &mut Frame, c: Point) {
    let arm = 5.0;
    let path = Path::new(|p| {
        p.move_to(Point::new(c.x - arm, c.y - arm));
        p.line_to(Point::new(c.x + arm, c.y + arm));
        p.move_to(Point::new(c.x + arm, c.y - arm));
        p.line_to(Point::new(c.x - arm, c.y + arm));
    });
    f.stroke(&path, line_stroke(1.8, DIM.get()));
}

fn chevron_right(f: &mut Frame, c: Point) {
    let half = CHEVRON / 4.0;
    let path = Path::new(|p| {
        p.move_to(Point::new(c.x - half / 2.0, c.y - half));
        p.line_to(Point::new(c.x + half / 2.0, c.y));
        p.line_to(Point::new(c.x - half / 2.0, c.y + half));
    });
    f.stroke(&path, line_stroke(1.8, FAINT.get()));
}

fn chevron_down(f: &mut Frame, c: Point) {
    let half = CHEVRON / 4.0;
    let path = Path::new(|p| {
        p.move_to(Point::new(c.x - half, c.y - half / 2.0));
        p.line_to(Point::new(c.x, c.y + half / 2.0));
        p.line_to(Point::new(c.x + half, c.y - half / 2.0));
    });
    f.stroke(&path, line_stroke(1.8, FAINT.get()));
}

/// A check, `size` px, its middle-left at `at`.
fn check(f: &mut Frame, at: Point, colour: Color, size: f32) {
    let unit = size / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * unit, at.y + (y - 12.0) * unit);
    let path = Path::new(|b| {
        b.move_to(p(4.0, 12.5));
        b.line_to(p(9.5, 18.0));
        b.line_to(p(20.0, 6.5));
    });
    f.stroke(&path, line_stroke(2.6 * unit * 1.2, colour));
}

/// A 16 px box at `at`: `#6CC287` and a dark check when ticked.
fn tick(f: &mut Frame, at: Point, ticked: bool) {
    let rect = Rectangle::new(at, Size::new(BOX, BOX));
    if ticked {
        f.fill(&kit::rounded(rect, 4.0), GREEN.get());
        check(f, Point::new(at.x + 2.0, at.y + BOX / 2.0), INK.get(), 12.0);
    } else {
        let inset = 0.75;
        let border = Rectangle::new(
            Point::new(at.x + inset, at.y + inset),
            Size::new(BOX - 1.5, BOX - 1.5),
        );
        f.stroke(
            &kit::rounded(border, 4.0 - inset),
            Stroke::default()
                .with_color(BOX_BORDER.get())
                .with_width(1.5),
        );
    }
}

/// A 16 px five-pointed star centred on `c`: filled yellow when lit,
/// else its outline; at half strength on a calendar not shown.
fn star_mark(f: &mut Frame, c: Point, lit: bool, shown: bool) {
    let (outer, inner) = (BOX / 2.0, BOX / 2.0 * 0.45);
    let path = Path::new(|b| {
        for i in 0..10 {
            let r = if i % 2 == 0 { outer } else { inner };
            let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
            // Its points' middle a little low, so that it sits centred.
            let p = Point::new(c.x + r * a.cos(), c.y + 0.6 + r * a.sin());
            if i == 0 {
                b.move_to(p);
            } else {
                b.line_to(p);
            }
        }
        b.close();
    });
    let strength = if shown { 1.0 } else { 0.5 };
    if lit {
        f.fill(&path, faded(STAR.get(), strength));
    } else {
        f.stroke(
            &path,
            Stroke::default()
                .with_color(faded(BOX_BORDER.get(), strength))
                .with_width(1.3)
                .with_line_join(canvas::LineJoin::Round),
        );
    }
}

/// A 12 px spinner, its middle-left at `at`: still without motion.
fn spinner(f: &mut Frame, at: Point, colour: Color, motion: bool) {
    let r = 5.0;
    let centre = Point::new(at.x + r, at.y);
    let turn = if motion {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f32().fract())
            .unwrap_or(0.0);
        t * std::f32::consts::TAU
    } else {
        0.0
    };
    let arc = Path::new(|b| {
        b.arc(canvas::path::Arc {
            center: centre,
            radius: r,
            start_angle: iced::Radians(turn),
            end_angle: iced::Radians(turn + 4.7),
        });
    });
    f.stroke(&arc, line_stroke(1.8, colour));
}

/// A field's entry, reached by the setup's fields.
type EntryOf = fn(&mut dyn FnMut(&mut crate::field::Entry));

#[cfg(test)]
mod tests {
    use super::*;
    use hocus_day::goa::GoogleAccount;

    fn width(t: &str, size: f32, _: iced::Font) -> f32 {
        t.chars().count() as f32 * size * 0.5
    }

    fn flow() -> setup::Flow {
        setup::Flow::for_tests()
    }

    #[test]
    fn step_1_has_a_row_per_kind_and_googles_accounts_under_it() {
        let words = Words::new(crate::clock::time::Lang::French);
        let mut flow = flow();
        flow.online = Online::Listed(vec![
            GoogleAccount {
                path: "/a/1".to_owned(),
                identity: "alex@example.org".to_owned(),
                name: "alex@example.org".to_owned(),
                calendar: true,
            },
            GoogleAccount {
                path: "/a/2".to_owned(),
                identity: "lou@example.org".to_owned(),
                name: "lou@example.org".to_owned(),
                calendar: false,
            },
        ]);
        let laid = lay_out(&flow, words, width);
        assert_eq!(laid.title, "Ajouter un compte");
        assert!(!laid.back);
        let order: Vec<Target> = laid.targets.iter().map(|(t, _)| *t).collect();
        // The account with its calendar on only, then Planka, then ✕.
        assert_eq!(order, [Target::Account(0), Target::Planka, Target::Close]);
        // 13 + 28 + 12: the first row's top; 54 px and an account's 34.
        let Some(Part::Kind { rect, .. }) = laid.parts.first() else {
            panic!("no row");
        };
        assert_eq!(rect.y, 53.0);
        assert_eq!(rect.width, 290.0);
        assert!(
            laid.said.contains("accounts=[alex@example.org]"),
            "{}",
            laid.said
        );
        // No account: the row opens Online Accounts' panel, if it is there.
        flow.online = Online::Listed(Vec::new());
        flow.panel = Some("/usr/bin/gnome-control-center".to_owned());
        let laid = lay_out(&flow, words, width);
        assert_eq!(laid.targets[0].0, Target::Online);
        flow.panel = None;
        let laid = lay_out(&flow, words, width);
        assert_eq!(laid.targets[0].0, Target::Planka);
        flow.online = Online::Missing;
        let laid = lay_out(&flow, words, width);
        assert!(laid.said.contains("disabled=true"), "{}", laid.said);
    }

    #[test]
    fn step_3_has_a_star_after_each_calendars_swatch() {
        let words = Words::new(crate::clock::time::Lang::French);
        let mut flow = flow();
        flow.step = Step::Show;
        let row = |name: &str, shown: bool, featured: bool| setup::Row {
            name: name.to_owned(),
            colour: 0,
            shown,
            featured,
            read_only: false,
        };
        flow.show = Some(Show::Google {
            account: GoogleAccount {
                path: "/a/1".to_owned(),
                identity: "alex@example.org".to_owned(),
                name: "alex@example.org".to_owned(),
                calendar: true,
            },
            calendars: Calendars::Read(vec![
                row("Travail", true, true),
                row("Congés", true, false),
            ]),
        });
        let laid = lay_out(&flow, words, width);
        let order: Vec<Target> = laid.targets.iter().map(|(t, _)| *t).collect();
        assert_eq!(
            order[1..7],
            [
                Target::Calendar(0),
                Target::Swatch(0),
                Target::Star(0),
                Target::Calendar(1),
                Target::Swatch(1),
                Target::Star(1),
            ]
        );
        // 16 px, 10 px after the swatch, both in their 24 px targets.
        let at = |t: Target| laid.targets.iter().find(|(o, _)| *o == t).map(|(_, r)| *r);
        let (swatch, star) = (at(Target::Swatch(0)), at(Target::Star(0)));
        assert_eq!(swatch.map(|r| r.x + 26.0), star.map(|r| r.x));
        assert_eq!(star.map(|r| (r.y, r.width)), swatch.map(|r| (r.y, 24.0)));
        assert!(
            laid.said.contains(
                "calendars=[[x] ★ Travail #8FB3FF; [x] ☆ Congés #8FB3FF] star=«Mettre en avant»"
            ),
            "{}",
            laid.said
        );
    }

    #[test]
    fn step_2_has_its_fields_and_says_what_was_refused() {
        let words = Words::new(crate::clock::time::Lang::French);
        let mut flow = flow();
        flow.step = Step::Planka;
        let laid = lay_out(&flow, words, width);
        assert!(laid.back);
        let order: Vec<Target> = laid.targets.iter().map(|(t, _)| *t).collect();
        // Nothing typed: "Se connecter" does nothing.
        assert_eq!(
            order,
            [
                Target::Back,
                Target::Field(Field::Address),
                Target::Field(Field::Token),
                Target::Cancel,
                Target::Close
            ]
        );
        flow.address.set("https://planka.example.org/boards/1");
        assert!(flow.token.paste("tok"));
        flow.refused = Some(Refusal::Token);
        let laid = lay_out(&flow, words, width);
        assert!(laid.targets.iter().any(|(t, _)| *t == Target::SignIn));
        assert!(
            laid.said
                .contains("refused=«Jeton refusé ou expiré : colle un nouveau jeton.»"),
            "{}",
            laid.said
        );
        let fields: Vec<(f32, bool)> = laid
            .parts
            .iter()
            .filter_map(|p| match p {
                Part::Field { rect, refused, .. } => Some((rect.height, *refused)),
                _ => None,
            })
            .collect();
        assert_eq!(fields, [(34.0, false), (34.0, true)]);
    }
}
