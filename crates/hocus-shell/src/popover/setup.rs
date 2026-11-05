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

use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme, keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path, Stroke},
        text::Alignment,
    },
    window,
};

use super::{
    Event as Seen, Pressed, event,
    kit::{self, Input, Pointer},
};
use crate::{
    levels::draw::{DIM, FAINT, MANROPE, MANROPE_BOLD, TEXT, faded, rgb, ring},
    setup::{
        self, CHOICES, Calendars, Field, Online, PALETTE, Refusal, Show, Step, Unfold, Words,
        card::{BLUE, GREEN, INK},
    },
};

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
const DOT: f32 = 2.4;
const DOT_ADVANCE: f32 = 7.0;
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

const ROW_FILL: Color = Color::from_rgba8(255, 255, 255, 0.03);
const ROW_HOVER: Color = Color::from_rgba8(255, 255, 255, 0.06);
const FIELD_FILL: Color = Color::from_rgba8(255, 255, 255, 0.05);
const FIELD_RIM: Color = Color::from_rgba8(255, 255, 255, 0.14);
const FIELD_FOCUS: Color = Color::from_rgba8(108, 194, 135, 0.65);
const FIELD_REFUSED: Color = rgb(0xF2945B);
const PEACH: Color = rgb(0xF7B98A);
const PLAIN: Color = Color::from_rgba8(255, 255, 255, 0.06);
const PLAIN_HOVER: Color = Color::from_rgba8(255, 255, 255, 0.10);
const GREEN_HOVER: Color = rgb(0x7DCF97);
const BOX_BORDER: Color = Color::from_rgba8(255, 255, 255, 0.38);
/// A featured calendar's star: the eight's yellow.
const STAR: Color = rgb(0xF5C46E);

/// What can be acted on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
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
const GOOGLE_TINT: Color = BLUE;
const PLANKA_TINT: Color = GREEN;

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
                tint: GOOGLE_TINT,
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
                tint: PLANKA_TINT,
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
                colour: FAINT,
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
                    colour: DIM,
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
                colour: FAINT,
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
                    colour: PEACH,
                    bold: false,
                });
                y += n * ERROR_LINE;
            }
            y += GAP;
            let half = (CONTENT - 8.0) / 2.0;
            let cancel = Rectangle::new(Point::new(LEFT, y), Size::new(half, BUTTON));
            let sign_in = Rectangle::new(Point::new(RIGHT - half, y), Size::new(half, BUTTON));
            let busy = flow.checking.is_some();
            let ready = !flow.address.trim().is_empty() && !flow.token.is_empty();
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
                flow.address,
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
                colour: DIM,
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
                                colour: DIM,
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
                                colour: PEACH,
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
                            colour: TEXT,
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
                            colour: DIM,
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
                            colour: DIM,
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

#[derive(Debug, Default)]
pub struct State {
    pointer: Pointer<u8>,
    over: Option<Target>,
    focus: Option<Target>,
    laid: Laid,
    /// The flow's text fields as last read, to draw.
    address: String,
    token: usize,
    said: Option<String>,
    targets_said: Option<String>,
}

impl State {
    fn target(&self, at: Point) -> Option<Target> {
        // The last laid out on top: a colour over its calendar's row, an
        // option over what is under it.
        self.laid
            .targets
            .iter()
            .rev()
            .filter(|(t, _)| !matches!(t, Target::Calendar(_)))
            .find(|(_, r)| r.contains(at))
            .or_else(|| self.laid.targets.iter().find(|(_, r)| r.contains(at)))
            .map(|(t, _)| *t)
    }

    fn order(&self) -> Vec<Target> {
        self.laid.targets.iter().map(|(t, _)| *t).collect()
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
            Input::Redraw(_) => {
                for pressed in kit::keys(self.id) {
                    let _: Option<canvas::Action<Message>> = self.key(state, &pressed);
                }
                self.refresh(state);
                None
            }
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
        let laid = &state.laid;
        let lit = |t: Target| state.over == Some(t);
        let focused = |t: Target| state.focus == Some(t);
        // The header.
        let title_x = if laid.back {
            let rect = back_rect();
            square(f, rect, lit(Target::Back), focused(Target::Back));
            arrow(f, rect.center());
            LEFT + BUTTON_SQUARE + 4.0
        } else {
            LEFT
        };
        f.fill_text(kit::text_in(
            MANROPE_BOLD,
            kit::text(
                laid.title.to_owned(),
                Point::new(title_x, HEADER_TOP + HEADER / 2.0),
                TITLE,
                TEXT,
                Alignment::Left,
            ),
        ));
        let close = close_rect();
        square(f, close, lit(Target::Close), focused(Target::Close));
        cross(f, close.center());
        for part in &laid.parts {
            self.part(f, state, part);
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        match state.over {
            Some(Target::Field(_)) => mouse::Interaction::Text,
            Some(_) => mouse::Interaction::Pointer,
            None => mouse::Interaction::default(),
        }
    }
}

impl Panel {
    /// What it shows, read afresh while it is open.
    fn refresh(&self, state: &mut State) {
        if !super::with(|c| c.is_open(self.id)) {
            return;
        }
        let read = setup::with(|flow| {
            (
                lay_out(flow, self.words, measure),
                flow.address.clone(),
                flow.token.chars(),
            )
        });
        let Some((laid, address, token)) = read else {
            return;
        };
        // A new step: the keyboard on its first field, else on nothing.
        if laid.step != state.laid.step {
            state.focus = match laid.step {
                Some(Step::Planka) => Some(Target::Field(Field::Address)),
                _ => None,
            };
        }
        state.laid = laid;
        state.address = address;
        state.token = token;
        if let Some(focus) = state.focus
            && !state.order().contains(&focus)
        {
            state.focus = None;
        }
        // Its height follows what it shows.
        super::refit(super::Item::DayChoose);
        // The header's buttons' names, for screen readers to come (§11).
        let names = if state.laid.back {
            format!("«{}» «{}»", self.words.back(), self.words.close())
        } else {
            format!("«{}»", self.words.close())
        };
        let said = format!(
            "{} focus={} title=«{}» buttons={names}",
            state.laid.said,
            state.focus.map_or("none".to_owned(), |t| format!("{t:?}")),
            state.laid.title,
        );
        if state.said.as_ref() != Some(&said) {
            tracing::debug!("the sources' popover shows it: {said}");
            state.said = Some(said);
        }
        let targets: Vec<String> = state
            .laid
            .targets
            .iter()
            .map(|(t, r)| {
                let c = r.center();
                format!(
                    "{}@{:.0},{:.0}",
                    format!("{t:?}").replace(' ', ""),
                    c.x,
                    c.y
                )
            })
            .collect();
        let targets = targets.join(" ");
        if state.targets_said.as_ref() != Some(&targets) {
            tracing::debug!(targets = %targets, "the sources' popover's targets");
            state.targets_said = Some(targets);
        }
    }

    fn act(state: &mut State, target: Option<Target>) {
        let Some(target) = target else {
            return;
        };
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
            Target::Field(_) => state.focus = Some(target),
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

    fn mouse<Message>(
        &self,
        state: &mut State,
        seen: &mouse::Event,
        at: Option<Point>,
    ) -> Option<canvas::Action<Message>> {
        let target = at.and_then(|at| state.target(at));
        let redraw = Some(canvas::Action::request_redraw().and_capture());
        match seen {
            mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                state.pointer.moved();
                if state.over == target {
                    return None;
                }
                state.over = target;
                redraw
            }
            mouse::Event::CursorLeft => {
                state.pointer.left();
                state.over = None;
                redraw
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                if !matches!(target, Some(Target::Field(_))) {
                    state.focus = None;
                }
                Self::act(state, target);
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
        use keyboard::{Key as K, key::Named};
        let redraw = Some(canvas::Action::request_redraw().and_capture());
        let modifiers = pressed.modifiers;
        if let Some(Target::Field(field)) = state.focus {
            match &pressed.key {
                K::Named(Named::Escape) => {
                    state.focus = None;
                    setup::back();
                    return redraw;
                }
                K::Named(Named::Enter) => {
                    setup::sign_in();
                    return redraw;
                }
                K::Named(Named::Backspace) => {
                    setup::erase(field, modifiers.control());
                    return redraw;
                }
                K::Named(Named::Tab) => {}
                key if crate::paste::asks(key, modifiers) => {
                    setup::paste(field);
                    return redraw;
                }
                _ if !(modifiers.control() || modifiers.logo() || modifiers.alt()) => {
                    if let Some(text) = pressed
                        .text
                        .as_deref()
                        .filter(|t| !t.is_empty() && t.chars().all(|c| !c.is_control()))
                    {
                        setup::type_in(field, text);
                        return redraw;
                    }
                    return None;
                }
                _ => return None,
            }
        }
        match &pressed.key {
            K::Named(Named::Escape) => {
                if !setup::back() {
                    event(Seen::Escape);
                }
                return Some(canvas::Action::capture());
            }
            K::Named(Named::Tab) => {
                let order = state.order();
                state.focus = kit::step(&order, state.focus, modifiers.shift());
            }
            K::Named(Named::ArrowDown | Named::ArrowUp) => {
                let rows: Vec<Target> = state.order().into_iter().filter(|t| t.row()).collect();
                let down = matches!(pressed.key, K::Named(Named::ArrowDown));
                state.focus = match state.focus.filter(|f| f.row()) {
                    Some(focus) => kit::cycle(&rows, focus, down),
                    None => kit::step(&rows, None, !down),
                };
            }
            K::Named(Named::Space) => match state.focus {
                Some(Target::Calendar(i)) => setup::toggle(i),
                focus => Self::act(state, focus),
            },
            K::Named(Named::Enter) => {
                let focus = state.focus;
                Self::act(state, focus);
            }
            _ => return None,
        }
        redraw
    }

    fn part(&self, f: &mut Frame, state: &State, part: &Part) {
        let lit = |t: Option<Target>| t.is_some() && state.over == t;
        let focused = |t: Option<Target>| t.is_some() && state.focus == t;
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
                let fill = if lit(*target) { ROW_HOVER } else { ROW_FILL };
                f.fill(&kit::rounded(*rect, ROW_RADIUS), fill);
                if focused(*target) {
                    ring(f, *rect, ROW_RADIUS);
                }
                let tile = Rectangle::new(
                    Point::new(rect.x + TILE_IN, rect.y + TILE_IN),
                    Size::new(TILE, TILE),
                );
                f.fill(
                    &kit::rounded(tile, 8.0),
                    faded(Color { a: 0.16, ..*tint }, alpha),
                );
                f.fill_text(kit::text_in(
                    MANROPE_BOLD,
                    kit::text(
                        letter.to_string(),
                        tile.center(),
                        15.0,
                        faded(*tint, alpha),
                        Alignment::Center,
                    ),
                ));
                f.fill_text(kit::text_in(
                    kit::MANROPE_SEMIBOLD,
                    kit::text(
                        (*name).to_owned(),
                        Point::new(rect.x + NAME_X, rect.y + 19.0),
                        NAME,
                        faded(TEXT, alpha),
                        Alignment::Left,
                    ),
                ));
                for (i, line) in lines.iter().enumerate() {
                    f.fill_text(kit::text(
                        line.clone(),
                        Point::new(rect.x + NAME_X, rect.y + 30.0 + LINE * (i as f32 + 0.5)),
                        LINE_TEXT,
                        faded(DIM, alpha),
                        Alignment::Left,
                    ));
                }
                if *chevron {
                    chevron_right(
                        f,
                        Point::new(rect.x + rect.width - 16.0, rect.y + ROW / 2.0),
                    );
                }
            }
            Part::Account { rect, target, name } => {
                let target = Some(*target);
                if lit(target) {
                    f.fill(&kit::rounded(*rect, 8.0), ROW_HOVER);
                }
                if focused(target) {
                    ring(f, *rect, 8.0);
                }
                let room = rect.width - (NAME_X - 4.0) - 28.0;
                let shown = kit::elide(name, room, |t| measure(t, 13.0, MANROPE));
                f.fill_text(kit::text(
                    shown,
                    Point::new(rect.x + NAME_X - 4.0, rect.center_y()),
                    13.0,
                    TEXT,
                    Alignment::Left,
                ));
                chevron_right(f, Point::new(rect.x + rect.width - 12.0, rect.center_y()));
            }
            Part::Text {
                lines,
                top,
                size,
                line,
                colour,
                bold,
            } => {
                for (i, said) in lines.iter().enumerate() {
                    let text = kit::text(
                        said.clone(),
                        Point::new(LEFT, top + line * (i as f32 + 0.5)),
                        *size,
                        *colour,
                        Alignment::Left,
                    );
                    f.fill_text(if *bold {
                        kit::text_in(kit::MANROPE_SEMIBOLD, text)
                    } else {
                        text
                    });
                }
            }
            Part::Caption { text, top } => {
                let letters = kit::label_letters(text);
                kit::label(f, &letters, top + CAPTION_HEIGHT / 2.0);
                let _ = CAPTION;
            }
            Part::Field {
                field,
                rect,
                refused,
            } => self.field(f, state, *field, *rect, *refused),
            Part::Button {
                rect,
                target,
                word,
                look,
                busy,
            } => {
                let enabled = target.is_some() || *busy;
                let (fill, ink, font) = match look {
                    Look::Plain => (
                        if lit(*target) { PLAIN_HOVER } else { PLAIN },
                        DIM,
                        kit::MANROPE_SEMIBOLD,
                    ),
                    Look::Green => (
                        if lit(*target) { GREEN_HOVER } else { GREEN },
                        INK,
                        MANROPE_BOLD,
                    ),
                };
                let alpha = if enabled { 1.0 } else { 0.45 };
                f.fill(&kit::rounded(*rect, BUTTON_RADIUS), faded(fill, alpha));
                if focused(*target) {
                    ring(f, *rect, BUTTON_RADIUS);
                }
                let mut centre = rect.center();
                if *busy {
                    let w = measure(word, BUTTON_TEXT, font);
                    let spin = Point::new(centre.x - (w + 18.0) / 2.0 + 6.0, centre.y);
                    spinner(f, spin, ink, self.motion);
                    centre.x += 9.0;
                }
                f.fill_text(kit::text_in(
                    font,
                    kit::text(
                        (*word).to_owned(),
                        centre,
                        BUTTON_TEXT,
                        faded(ink, alpha),
                        Alignment::Center,
                    ),
                ));
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
                let target = Some(Target::Calendar(*index));
                if lit(target) {
                    f.fill(&kit::rounded(*rect, 8.0), ROW_HOVER);
                }
                if focused(target) {
                    ring(f, *rect, 8.0);
                }
                let middle = rect.center_y();
                tick(f, Point::new(LEFT, middle - BOX / 2.0), *shown);
                let swatch = swatch_rect(rect.y);
                let inner = Rectangle::new(
                    Point::new(swatch.x + 4.0, swatch.y + 4.0),
                    Size::new(BOX, BOX),
                );
                f.fill(&kit::rounded(inner, 4.0), PALETTE_COLOURS[*colour]);
                let swatch_target = Some(Target::Swatch(*index));
                if lit(swatch_target) {
                    f.stroke(
                        &kit::rounded(swatch, 6.0),
                        Stroke::default().with_color(DIM).with_width(1.0),
                    );
                }
                if focused(swatch_target) {
                    ring(f, swatch, 6.0);
                }
                let star = star_rect(rect.y);
                // A star on a calendar not shown counts for nothing: dim.
                star_mark(f, star.center(), *featured, *shown);
                let star_target = Some(Target::Star(*index));
                if lit(star_target) {
                    f.stroke(
                        &kit::rounded(star, 6.0),
                        Stroke::default().with_color(DIM).with_width(1.0),
                    );
                }
                if focused(star_target) {
                    ring(f, star, 6.0);
                }
                let x = star.x + star.width - 4.0 + 10.0;
                let aside = read_only.then(|| setup::Words::read_only(self.words));
                let aside_width = aside.map_or(0.0, |a| measure(a, 11.5, MANROPE) + 8.0);
                let room = RIGHT - x - aside_width;
                let shown_name = kit::elide(name, room, |t| measure(t, 13.0, MANROPE));
                f.fill_text(kit::text(
                    shown_name,
                    Point::new(x, middle),
                    13.0,
                    if *shown { TEXT } else { DIM },
                    Alignment::Left,
                ));
                if let Some(aside) = aside {
                    f.fill_text(kit::text(
                        aside.to_owned(),
                        Point::new(RIGHT, middle),
                        11.5,
                        FAINT,
                        Alignment::Right,
                    ));
                }
            }
            Part::Colours { index, top, chosen } => {
                for (c, colour) in PALETTE_COLOURS.iter().enumerate() {
                    let rect = colour_rect(*top, c);
                    let centre = rect.center();
                    f.fill(&Path::circle(centre, SWATCH / 2.0), *colour);
                    let target = Some(Target::Colour(*index, c));
                    if c == *chosen || lit(target) {
                        f.stroke(
                            &Path::circle(centre, SWATCH / 2.0 + 2.5),
                            Stroke::default()
                                .with_color(if c == *chosen { TEXT } else { DIM })
                                .with_width(1.5),
                        );
                    }
                    if focused(target) {
                        ring(f, rect, SWATCH / 2.0);
                    }
                }
            }
            Part::Choice {
                target,
                rect,
                label,
                value,
            } => {
                let target = Some(*target);
                if lit(target) {
                    f.fill(&kit::rounded(*rect, 8.0), ROW_HOVER);
                }
                if focused(target) {
                    ring(f, *rect, 8.0);
                }
                let middle = rect.center_y();
                f.fill_text(kit::text(
                    (*label).to_owned(),
                    Point::new(LEFT, middle),
                    13.0,
                    DIM,
                    Alignment::Left,
                ));
                let label_width = measure(label, 13.0, MANROPE) + 12.0;
                let room = CONTENT - label_width - 18.0;
                let shown = kit::elide(value, room, |t| measure(t, 13.0, MANROPE));
                f.fill_text(kit::text(
                    shown,
                    Point::new(RIGHT - 16.0, middle),
                    13.0,
                    TEXT,
                    Alignment::Right,
                ));
                chevron_down(f, Point::new(RIGHT - 5.0, middle));
            }
            Part::Option {
                target,
                rect,
                name,
                chosen,
            } => {
                let target = Some(*target);
                if lit(target) {
                    f.fill(&kit::rounded(*rect, 7.0), ROW_HOVER);
                }
                if focused(target) {
                    ring(f, *rect, 7.0);
                }
                let middle = rect.center_y();
                if *chosen {
                    check(f, Point::new(rect.x + 12.0, middle), GREEN, 12.0);
                }
                let room = rect.width - 34.0;
                let shown = kit::elide(name, room, |t| measure(t, 13.0, MANROPE));
                f.fill_text(kit::text(
                    shown,
                    Point::new(rect.x + 26.0, middle),
                    13.0,
                    if *chosen { TEXT } else { DIM },
                    Alignment::Left,
                ));
            }
        }
    }

    /// A field: its text (the token as dots), its end in view; the caret
    /// while it holds the keyboard.
    fn field(&self, f: &mut Frame, state: &State, field: Field, rect: Rectangle, refused: bool) {
        let focused = state.focus == Some(Target::Field(field));
        f.fill(&kit::rounded(rect, FIELD_RADIUS), FIELD_FILL);
        let rim = Rectangle::new(
            Point::new(rect.x + 0.5, rect.y + 0.5),
            Size::new(rect.width - 1.0, rect.height - 1.0),
        );
        let colour = if refused {
            FIELD_REFUSED
        } else if focused {
            FIELD_FOCUS
        } else {
            FIELD_RIM
        };
        f.stroke(
            &kit::rounded(rim, FIELD_RADIUS - 0.5),
            Stroke::default().with_color(colour).with_width(1.0),
        );
        let left = rect.x + FIELD_PAD;
        let room = rect.width - 2.0 * FIELD_PAD;
        let middle = rect.center_y();
        let end = match field {
            Field::Address if state.address.is_empty() => {
                f.fill_text(kit::text(
                    "https://…/boards/…".to_owned(),
                    Point::new(left, middle),
                    FIELD_TEXT,
                    FAINT,
                    Alignment::Left,
                ));
                left
            }
            Field::Address => {
                // Its end, as a field scrolls to its caret.
                let chars: Vec<char> = state.address.chars().collect();
                let mut from = 0;
                let mut shown: String = chars.iter().collect();
                while from < chars.len() && measure(&shown, FIELD_TEXT, MANROPE) > room - 2.0 {
                    from += 1;
                    shown = std::iter::once('…')
                        .chain(chars[from..].iter().copied())
                        .collect();
                }
                let width = measure(&shown, FIELD_TEXT, MANROPE);
                f.fill_text(kit::text(
                    shown,
                    Point::new(left, middle),
                    FIELD_TEXT,
                    TEXT,
                    Alignment::Left,
                ));
                left + width
            }
            Field::Token if state.token == 0 => {
                f.fill_text(kit::text(
                    "accessToken=…; httpOnlyToken=…".to_owned(),
                    Point::new(left, middle),
                    FIELD_TEXT,
                    FAINT,
                    Alignment::Left,
                ));
                left
            }
            Field::Token => {
                let fit = ((room - 2.0) / DOT_ADVANCE).floor().max(0.0) as usize;
                let n = state.token.min(fit);
                for i in 0..n {
                    let centre = Point::new(left + DOT_ADVANCE * (i as f32 + 0.5), middle);
                    f.fill(&Path::circle(centre, DOT), TEXT);
                }
                left + n as f32 * DOT_ADVANCE
            }
        };
        if focused {
            f.fill_rectangle(
                Point::new(end + 1.0, middle - 8.0),
                Size::new(1.0, 16.0),
                TEXT,
            );
        }
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

/// A 26 × 26 header button: lit under the pointer, ringed with the
/// keyboard.
fn square(f: &mut Frame, rect: Rectangle, lit: bool, focused: bool) {
    if lit {
        f.fill(&kit::rounded(rect, 7.0), ROW_HOVER);
    }
    if focused {
        ring(f, rect, 7.0);
    }
}

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
    f.stroke(&path, line_stroke(1.8, DIM));
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
    f.stroke(&path, line_stroke(1.8, DIM));
}

fn chevron_right(f: &mut Frame, c: Point) {
    let half = CHEVRON / 4.0;
    let path = Path::new(|p| {
        p.move_to(Point::new(c.x - half / 2.0, c.y - half));
        p.line_to(Point::new(c.x + half / 2.0, c.y));
        p.line_to(Point::new(c.x - half / 2.0, c.y + half));
    });
    f.stroke(&path, line_stroke(1.8, FAINT));
}

fn chevron_down(f: &mut Frame, c: Point) {
    let half = CHEVRON / 4.0;
    let path = Path::new(|p| {
        p.move_to(Point::new(c.x - half, c.y - half / 2.0));
        p.line_to(Point::new(c.x, c.y + half / 2.0));
        p.line_to(Point::new(c.x + half, c.y - half / 2.0));
    });
    f.stroke(&path, line_stroke(1.8, FAINT));
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
        f.fill(&kit::rounded(rect, 4.0), GREEN);
        check(f, Point::new(at.x + 2.0, at.y + BOX / 2.0), INK, 12.0);
    } else {
        let inset = 0.75;
        let border = Rectangle::new(
            Point::new(at.x + inset, at.y + inset),
            Size::new(BOX - 1.5, BOX - 1.5),
        );
        f.stroke(
            &kit::rounded(border, 4.0 - inset),
            Stroke::default().with_color(BOX_BORDER).with_width(1.5),
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
        f.fill(&path, faded(STAR, strength));
    } else {
        f.stroke(
            &path,
            Stroke::default()
                .with_color(faded(BOX_BORDER, strength))
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
        flow.address = "https://planka.example.org/boards/1".to_owned();
        assert!(flow.token.push("tok"));
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
