//! The drawer drawn (docs/design/secretary.md §5, the s11 mockup's panel,
//! at its sizes ×1): its header (her head 52 px in her mood, "Ta
//! secrétaire" 16 px, her status line 12 px with its dot, the gear and ✕),
//! its tabs, the agents' line (36 px), the thread (her bubbles under her
//! 30 px head, the user's on the right in green, cards, quick replies,
//! what she kept, her typing), and the field with its footer.
//!
//! What it shows is laid out once for each change ([`Laid`], kept in the
//! surface's state); her head alone draws again while her mood plays.
//! The thread sits at the bottom and scrolls up under the wheel.
//!
//! What it shows and where its controls are go to the log when they
//! change (`the drawer shows`, `the drawer's items`): the tests read them.

use std::time::Instant;

use hocus_secretary_link::{Body, Card, CardKind, Item, Mood, Tone};
use iced::{
    Color, Pixels, Point, Rectangle, Renderer, Size, Theme,
    advanced::svg,
    alignment, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path, Stroke},
        text::{Alignment, LineHeight, Shaping},
    },
    window,
};

use super::{Open, Tab, Target, Words};
use crate::{
    agents::{self, Brief, Look as AgentLook},
    levels::draw::{MANROPE, MANROPE_BOLD},
    pandas,
    popover::kit::{measure, wrap},
    secretary::{Conversation, State as Her},
};

// --- The mockup's sizes, at ×1 ---------------------------------------------------

const RADIUS: f32 = 14.0;
const PAD: f32 = 12.0;
/// The header: her head, 10 px down, 6 px over the tabs.
const HEAD: f32 = 52.0;
const HEAD_TOP: f32 = 10.0;
const HEAD_BOTTOM: f32 = 6.0;
const HEAD_GAP: f32 = 10.0;
const TITLE: f32 = 16.0;
const STATUS: f32 = 12.0;
const DOT: f32 = 7.0;
const ICON: f32 = 28.0;
const ICON_GAP: f32 = 6.0;
/// The tabs: 26 px in a 2 px padded box, 2 px under the header, 8 px over
/// what follows.
const TABS_TOP: f32 = 2.0;
const TABS_BOTTOM: f32 = 8.0;
const TAB: f32 = 26.0;
const TAB_PAD: f32 = 2.0;
const TAB_TEXT: f32 = 12.5;
/// The agents' line.
const AGENTS: f32 = 36.0;
const AGENTS_BOTTOM: f32 = 6.0;
const AGENT_PANDA: f32 = 30.0;
const AGENTS_TEXT: f32 = 12.5;
/// The thread.
const THREAD_TOP: f32 = 6.0;
const THREAD_BOTTOM: f32 = 10.0;
const GAP: f32 = 10.0;
const INNER_GAP: f32 = 6.0;
const AVATAR: f32 = 30.0;
const AVATAR_GAP: f32 = 8.0;
const BUBBLE: f32 = 13.5;
const BUBBLE_LINE: f32 = BUBBLE * 1.45;
const BUBBLE_X: f32 = 12.0;
const BUBBLE_Y: f32 = 8.0;
const HER_MOST: f32 = 340.0;
const MINE_MOST: f32 = 310.0;
const SMALL: f32 = 12.0;
const SMALL_LINE: f32 = SMALL * 1.45;
const ASK: f32 = 14.5;
const ASK_LINE: f32 = ASK * 1.45;
const PILL: f32 = 12.5;
const PILL_X: f32 = 12.0;
const PILL_Y: f32 = 5.0;
const PILL_GAP: f32 = 6.0;
const CARD_X: f32 = 12.0;
const CARD_TOP: f32 = 9.0;
const CARD_BOTTOM: f32 = 10.0;
const CARD_LABEL: f32 = 10.5;
const CARD_TEXT: f32 = 13.0;
const CARD_LINE: f32 = CARD_TEXT * 1.45;
const BUTTON: f32 = 12.5;
const BUTTON_X: f32 = 11.0;
const BUTTON_Y: f32 = 5.0;
const BUTTON_TOP: f32 = 8.0;
const LEARNED: f32 = 12.0;
const DAY: f32 = 11.0;
/// The field: 44 px at least, 8 px under the thread's rule, its footer
/// 6 px under it, 10 px from the bottom.
const FIELD: f32 = 44.0;
const FIELD_TEXT: f32 = 14.0;
const FIELD_LINE: f32 = FIELD_TEXT * 1.45;
const FIELD_LINES: usize = 5;
const SEND: f32 = 32.0;
const COMPOSER_TOP: f32 = 8.0;
const COMPOSER_BOTTOM: f32 = 10.0;
const FOOT: f32 = 11.0;
const FOOT_GAP: f32 = 6.0;

// --- Its colours (dark, as the mockup) ------------------------------------------------

const fn hex(rgb: u32) -> Color {
    Color::from_rgb8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

const fn alpha(rgb: u32, a: f32) -> Color {
    Color::from_rgba8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, a)
}

const TEXT: Color = hex(0xEEF1F6);
const DIM: Color = hex(0xA9B2C2);
const FAINT: Color = hex(0x7C8698);
const LABEL: Color = hex(0x8C96A8);
const GREEN: Color = hex(0x6CC287);
const MINT: Color = hex(0x7EE0A0);
const AMBER: Color = hex(0xF5B44A);
const ORANGE: Color = hex(0xF2945B);
const GREY: Color = hex(0x8C96A8);
const INK: Color = hex(0x0B1020);

/// What a card's kind is drawn in: its rim, its fill, its label.
fn card_colours(kind: CardKind) -> (Color, Color, Color) {
    match kind {
        CardKind::Plan => (alpha(0x7EE0A0, 0.4), alpha(0x7EE0A0, 0.05), MINT),
        CardKind::Rule => (alpha(0xF5C46E, 0.4), alpha(0xF5B44A, 0.06), hex(0xF5C46E)),
        CardKind::Person => (alpha(0x8FB3FF, 0.4), alpha(0x8FB3FF, 0.06), hex(0xB9CEFF)),
        CardKind::Warning => (alpha(0xF2945B, 0.45), alpha(0xF2945B, 0.07), hex(0xF7A77A)),
        CardKind::Draft => (alpha(0xFFFFFF, 0.18), hex(0x0F1422), LABEL),
        CardKind::Delegation => (alpha(0xB39DFF, 0.45), alpha(0xB39DFF, 0.06), hex(0xD3C6FF)),
    }
}

// --- The layout -------------------------------------------------------------------------

/// A line of text, its middle's height and where it starts.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    pub at: Point,
    pub size: f32,
    pub color: Color,
    pub bold: bool,
    pub align: Alignment,
}

/// What is drawn of the thread, placed.
#[derive(Debug, Clone, PartialEq)]
enum Shape {
    /// A rounded box: its rect, radius, fill, rim (and its rim dashed),
    /// the corner made sharper (top left for hers, bottom right for the
    /// user's).
    Box {
        rect: Rectangle,
        radius: f32,
        fill: Color,
        rim: Option<Color>,
        dashed: bool,
        sharp: Corner,
    },
    Text(Line),
    /// Her head, still, 30 px.
    Avatar(Point),
    /// An agent's panda in her briefing, 32 px.
    Panda(Point, AgentLook),
    /// Her typing's three dots.
    Dots(Point),
    /// What she kept: its check.
    Check(Rectangle),
    /// A draft's quote: its bar.
    Quote(Rectangle),
    /// A day's divider: its two rules either side of its words.
    Rule(Rectangle),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Corner {
    None,
    TopLeft,
    BottomRight,
}

/// Everything laid out for one state of the drawer and its conversation.
#[derive(Debug, Clone, Default)]
pub struct Laid {
    pub size: Size,
    pub head: Rectangle,
    pub mood: Option<Mood>,
    pub status: String,
    pub tone: Option<Tone>,
    pub model: String,
    pub targets: Vec<(Target, Rectangle)>,
    pub tabs: Rectangle,
    pub tab_rects: Vec<(Tab, Rectangle)>,
    pub agents: Option<(Rectangle, String, String, agents::Look)>,
    pub thread: Rectangle,
    shapes: Vec<Shape>,
    /// The thread's height, whole, and how far it may scroll.
    pub content: f32,
    pub field: Rectangle,
    pub field_lines: Vec<String>,
    pub send: Rectangle,
    pub foot: f32,
    /// What it shows, for the log.
    pub said: serde_json::Value,
}

/// The measures of text, at a size, in a weight.
fn width_of(text: &str, size: f32, bold: bool) -> f32 {
    measure(text, size, if bold { MANROPE_BOLD } else { MANROPE })
}

/// `text` in lines that fit `room`: its own lines, each broken.
fn lines_of(text: &str, size: f32, bold: bool, room: f32) -> Vec<String> {
    text.split('\n')
        .flat_map(|p| wrap(p, room, |s| width_of(s, size, bold)))
        .collect()
}

/// A day's words: "Aujourd'hui · lundi 9 novembre".
fn day_words(at: &str, words: Words) -> Option<String> {
    let at: jiff::Timestamp = at.parse().ok()?;
    let zone = jiff::tz::TimeZone::system();
    let day = at.to_zoned(zone.clone()).date();
    let today = jiff::Timestamp::now().to_zoned(zone).date();
    const DAYS_FR: [&str; 7] = [
        "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche",
    ];
    const DAYS_EN: [&str; 7] = [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ];
    const MONTHS_FR: [&str; 12] = [
        "janvier",
        "février",
        "mars",
        "avril",
        "mai",
        "juin",
        "juillet",
        "août",
        "septembre",
        "octobre",
        "novembre",
        "décembre",
    ];
    const MONTHS_EN: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let weekday = day.weekday().to_monday_zero_offset() as usize;
    let month = (day.month() - 1) as usize;
    let date = if words.fr {
        format!("{} {} {}", DAYS_FR[weekday], day.day(), MONTHS_FR[month])
    } else {
        format!("{} {} {}", DAYS_EN[weekday], MONTHS_EN[month], day.day())
    };
    Some(if day == today {
        format!("{} · {date}", words.today())
    } else {
        date
    })
}

/// What the thread shows, in order: its items, her answer as it streams
/// after the sentence it answers, her typing at the end.
#[derive(Debug, Clone)]
enum Entry<'a> {
    Day(String),
    Item(&'a Item),
    Streaming(&'a str),
    Typing(&'a str),
}

fn entries<'a>(c: &'a Conversation, words: Words) -> Vec<Entry<'a>> {
    let mut out = Vec::new();
    let mut day = None;
    for (n, item) in c.items.iter().enumerate() {
        let this = day_words(&item.at, words);
        if this.is_some() && this != day {
            out.push(Entry::Day(this.clone().unwrap_or_default()));
            day = this;
        }
        out.push(Entry::Item(item));
        // Her answer as it comes, after the last item of its request.
        if let Some(request) = item.request {
            let last = !c.items[n + 1..].iter().any(|i| i.request == Some(request));
            if last && let Some((_, said)) = c.streaming.iter().find(|(r, _)| *r == request) {
                out.push(Entry::Streaming(said));
            }
        }
    }
    if let Some((request, doing)) = &c.typing
        && !c
            .streaming
            .iter()
            .any(|(r, s)| r == request && !s.is_empty())
    {
        out.push(Entry::Typing(doing));
    }
    out
}

/// Whether an entry is hers (her head heads a run of them).
fn hers(entry: &Entry<'_>) -> bool {
    match entry {
        Entry::Item(item) => !matches!(item.body, Body::User { .. }),
        Entry::Streaming(_) | Entry::Typing(_) => true,
        Entry::Day(_) => false,
    }
}

/// The drawer laid out at `size` for `open` and `c`.
pub fn lay_out(open: &Open, c: &Conversation, line: Her, words: Words, size: Size) -> Laid {
    let mut laid = Laid {
        size,
        ..Laid::default()
    };
    let width = size.width;
    let right = width - PAD;
    // Her face: hers when she speaks; asleep while she is away.
    let (look, mood) = match (line, &c.look) {
        (Her::Ready, Some(look)) => (Some(look.clone()), look.mood),
        (Her::Ready, None) => (None, Mood::Listen),
        _ => (None, Mood::Sleep),
    };
    laid.mood = Some(mood);
    laid.status = match (&look, line) {
        (Some(look), _) => look.status.clone(),
        (None, Her::Ready) => words.connecting().to_owned(),
        (None, _) => words.unavailable().to_owned(),
    };
    laid.tone = look.as_ref().map(|l| l.tone);
    laid.model = look.as_ref().map(|l| l.model.clone()).unwrap_or_default();
    // The header.
    laid.head = Rectangle::new(Point::new(PAD, HEAD_TOP), Size::new(HEAD, HEAD));
    let icons_y = HEAD_TOP + (HEAD - ICON) / 2.0;
    let close = Rectangle::new(Point::new(right - ICON, icons_y), Size::new(ICON, ICON));
    let gear = Rectangle::new(
        Point::new(close.x - ICON_GAP - ICON, icons_y),
        Size::new(ICON, ICON),
    );
    laid.targets.push((Target::Gear, gear));
    laid.targets.push((Target::Close, close));
    // The tabs.
    let tabs_top = HEAD_TOP + HEAD + HEAD_BOTTOM + TABS_TOP;
    laid.tabs = Rectangle::new(
        Point::new(PAD, tabs_top),
        Size::new(right - PAD, TAB + 2.0 * TAB_PAD),
    );
    let tab_width = (laid.tabs.width - 2.0 * TAB_PAD) / 2.0;
    for (n, tab) in [Tab::Talk, Tab::Knows].into_iter().enumerate() {
        let rect = Rectangle::new(
            Point::new(PAD + TAB_PAD + n as f32 * tab_width, tabs_top + TAB_PAD),
            Size::new(tab_width, TAB),
        );
        laid.tab_rects.push((tab, rect));
        laid.targets.push((Target::Tab(tab), rect));
    }
    let mut top = laid.tabs.y + laid.tabs.height + TABS_BOTTOM;
    // The agents' line, while a tool is followed.
    let said = agents::said();
    let shows = said.shows(agents::now_us());
    if open.tab == Tab::Talk && matches!(shows, agents::Shows::Lines(_) | agents::Shows::Idle(_)) {
        let brief = Brief::of(&said.agents);
        let rect = Rectangle::new(Point::new(PAD, top), Size::new(right - PAD, AGENTS));
        let agent_words = agents::Words::new(if words.fr {
            crate::clock::time::Lang::French
        } else {
            crate::clock::time::Lang::English
        });
        let mut parts: Vec<String> = brief.waiting.iter().map(|e| words.waits(&e.name)).collect();
        parts.extend(brief.stuck.iter().map(|e| words.stuck(&e.name)));
        let text = if parts.is_empty() {
            if said.agents.is_empty() {
                agent_words.no_agent().to_owned()
            } else {
                agent_words.line(&brief)
            }
        } else {
            parts.join(" · ")
        };
        let look = if !brief.waiting.is_empty() {
            AgentLook::Waiting
        } else if !brief.stuck.is_empty() {
            AgentLook::Stuck
        } else if !brief.to_see.is_empty() {
            AgentLook::ToSee
        } else if !brief.working.is_empty() {
            AgentLook::Working
        } else {
            AgentLook::Resting
        };
        laid.agents = Some((rect, text, words.agents(said.agents.len()), look));
        laid.targets.push((Target::Agents, rect));
        top += AGENTS + AGENTS_BOTTOM;
    }
    // The composer, from the bottom.
    let field_room = right - PAD - 14.0 - 8.0 - SEND - 6.0;
    let shown_field = if open.field.is_empty() {
        vec![words.placeholder().to_owned()]
    } else {
        lines_of(&open.field, FIELD_TEXT, false, field_room)
    };
    let field_lines = shown_field.len().clamp(1, FIELD_LINES);
    let field_height = FIELD.max(field_lines as f32 * FIELD_LINE + 14.0);
    let foot_height = FOOT * 1.4;
    let composer_top =
        size.height - COMPOSER_BOTTOM - foot_height - FOOT_GAP - field_height - COMPOSER_TOP;
    laid.field = Rectangle::new(
        Point::new(PAD, composer_top + COMPOSER_TOP),
        Size::new(right - PAD, field_height),
    );
    laid.field_lines = shown_field
        .into_iter()
        .rev()
        .take(FIELD_LINES)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    laid.send = Rectangle::new(
        Point::new(
            laid.field.x + laid.field.width - 6.0 - SEND,
            laid.field.y + laid.field.height - 6.0 - SEND,
        ),
        Size::new(SEND, SEND),
    );
    laid.targets.push((Target::Send, laid.send));
    laid.targets.push((Target::Field, laid.field));
    laid.foot = laid.field.y + laid.field.height + FOOT_GAP + foot_height / 2.0;
    laid.thread = Rectangle::new(
        Point::new(0.0, top),
        Size::new(width, (composer_top - top).max(0.0)),
    );
    let mut said_items = Vec::new();
    match (open.tab, open.briefing) {
        (Tab::Knows, _) => {
            let room = right - PAD - 2.0 * PAD;
            let mut y = top + 40.0;
            for text in lines_of(words.knows_soon(), BUBBLE, false, room) {
                laid.shapes.push(Shape::Text(Line {
                    text,
                    at: Point::new(width / 2.0, y),
                    size: BUBBLE,
                    color: DIM,
                    bold: false,
                    align: Alignment::Center,
                }));
                y += BUBBLE_LINE;
            }
        }
        (Tab::Talk, true) => lay_out_briefing(&mut laid, &said.agents, words, top, right),
        (Tab::Talk, false) => {
            lay_out_thread(&mut laid, open, c, words, top, right, &mut said_items);
        }
    }
    laid.said = serde_json::json!({
        "width": width,
        "height": size.height,
        "mood": mood.word(),
        "status": laid.status,
        "tab": match open.tab { Tab::Talk => "talk", Tab::Knows => "knows" },
        "briefing": open.briefing,
        "agents": laid.agents.as_ref().map(|(_, text, n, _)| format!("{text} | {n}")),
        "items": said_items,
        "field": open.field.chars().count(),
        "model": laid.model,
    });
    laid
}

/// The thread laid out from its bottom, scrolled up by `open.scroll`.
fn lay_out_thread(
    laid: &mut Laid,
    open: &Open,
    c: &Conversation,
    words: Words,
    top: f32,
    right: f32,
    said: &mut Vec<String>,
) {
    let width = laid.size.width;
    let her_x = PAD + AVATAR + AVATAR_GAP;
    let her_room = (right - her_x).min(HER_MOST);
    let mine_room = (right - PAD).min(MINE_MOST);
    // Laid from y = 0, then moved to the bottom.
    let mut shapes: Vec<Shape> = Vec::new();
    let mut targets: Vec<(Target, Rectangle)> = Vec::new();
    let mut y = THREAD_TOP;
    let list = entries(c, words);
    let mut previous_hers = false;
    for (n, entry) in list.iter().enumerate() {
        if n > 0 {
            y += GAP;
        }
        let first_hers = hers(entry) && !previous_hers;
        previous_hers = hers(entry);
        if first_hers {
            shapes.push(Shape::Avatar(Point::new(PAD, y)));
        }
        match entry {
            Entry::Day(text) => {
                let middle = y + DAY * 0.7;
                let upper = text.to_uppercase();
                let w = width_of(&upper, DAY, true) + 0.08 * DAY * upper.chars().count() as f32;
                shapes.push(Shape::Rule(Rectangle::new(
                    Point::new(PAD, middle),
                    Size::new(right - PAD, w + 20.0),
                )));
                shapes.push(Shape::Text(Line {
                    text: upper,
                    at: Point::new(width / 2.0, middle),
                    size: DAY,
                    color: FAINT,
                    bold: true,
                    align: Alignment::Center,
                }));
                said.push(format!("day:{text}"));
                y += DAY * 1.4;
            }
            Entry::Item(item) => match &item.body {
                Body::User { text } => {
                    let lines = lines_of(text, BUBBLE, false, mine_room - 2.0 * BUBBLE_X);
                    let w = lines
                        .iter()
                        .map(|l| width_of(l, BUBBLE, false))
                        .fold(0.0, f32::max)
                        + 2.0 * BUBBLE_X;
                    let h = lines.len() as f32 * BUBBLE_LINE + 2.0 * BUBBLE_Y;
                    let rect = Rectangle::new(Point::new(right - w, y), Size::new(w, h));
                    shapes.push(Shape::Box {
                        rect,
                        radius: 14.0,
                        fill: alpha(0x6CC287, 0.17),
                        rim: Some(alpha(0x6CC287, 0.35)),
                        dashed: false,
                        sharp: Corner::BottomRight,
                    });
                    push_lines(
                        &mut shapes,
                        &lines,
                        rect.x + BUBBLE_X,
                        y + BUBBLE_Y,
                        BUBBLE,
                        BUBBLE_LINE,
                        TEXT,
                        false,
                    );
                    said.push(format!("user:{text}"));
                    y += h;
                }
                Body::Her {
                    text,
                    note,
                    assumed,
                } => {
                    y = her_bubble(&mut shapes, text, note.as_deref(), her_x, her_room, y);
                    if *assumed {
                        let at = y + INNER_GAP;
                        let label = format!("{} · ", words.assumed());
                        let lw = width_of(&label, SMALL, false);
                        shapes.push(Shape::Text(Line {
                            text: label,
                            at: Point::new(her_x, at + SMALL_LINE / 2.0),
                            size: SMALL,
                            color: DIM,
                            bold: false,
                            align: Alignment::Left,
                        }));
                        let correct = words.correct();
                        let cw = width_of(correct, SMALL, false);
                        shapes.push(Shape::Text(Line {
                            text: correct.to_owned(),
                            at: Point::new(her_x + lw, at + SMALL_LINE / 2.0),
                            size: SMALL,
                            color: MINT,
                            bold: false,
                            align: Alignment::Left,
                        }));
                        targets.push((
                            Target::Correct(item.id),
                            Rectangle::new(
                                Point::new(her_x + lw - 2.0, at),
                                Size::new(cw + 4.0, SMALL_LINE),
                            ),
                        ));
                        y = at + SMALL_LINE;
                    }
                    said.push(format!("her:{text}"));
                }
                Body::Question {
                    text,
                    note,
                    replies,
                    chosen,
                } => {
                    // Her question in bold amber, what she found under it.
                    let room = her_room - 2.0 * BUBBLE_X;
                    let lines = lines_of(text, ASK, true, room);
                    let notes = note
                        .as_deref()
                        .map(|n| lines_of(n, SMALL, false, room))
                        .unwrap_or_default();
                    let h = lines.len() as f32 * ASK_LINE
                        + if notes.is_empty() {
                            0.0
                        } else {
                            3.0 + notes.len() as f32 * SMALL_LINE
                        }
                        + 2.0 * BUBBLE_Y;
                    let rect = Rectangle::new(Point::new(her_x, y), Size::new(her_room, h));
                    her_box(&mut shapes, rect);
                    push_lines(
                        &mut shapes,
                        &lines,
                        her_x + BUBBLE_X,
                        y + BUBBLE_Y,
                        ASK,
                        ASK_LINE,
                        hex(0xFFE2A8),
                        true,
                    );
                    let note_top = y + BUBBLE_Y + lines.len() as f32 * ASK_LINE + 3.0;
                    push_lines(
                        &mut shapes,
                        &notes,
                        her_x + BUBBLE_X,
                        note_top,
                        SMALL,
                        SMALL_LINE,
                        DIM,
                        false,
                    );
                    y += h + INNER_GAP;
                    // Its quick replies: pills, a click sends one and greys
                    // the others.
                    let mut x = her_x;
                    let pill_h = PILL * 1.4 + 2.0 * PILL_Y;
                    for (choice, reply) in replies.iter().enumerate() {
                        let w = width_of(reply, PILL, true) + 2.0 * PILL_X;
                        if x > her_x && x + w > her_x + her_room {
                            x = her_x;
                            y += pill_h + PILL_GAP;
                        }
                        let rect = Rectangle::new(Point::new(x, y), Size::new(w, pill_h));
                        let picked = *chosen == Some(choice as u32);
                        let used = chosen.is_some() && !picked;
                        let target = Target::Reply {
                            item: item.id,
                            choice: choice as u32,
                        };
                        let hover = open.hover == Some(target) && chosen.is_none();
                        let fade = if used { 0.35 } else { 1.0 };
                        shapes.push(Shape::Box {
                            rect,
                            radius: pill_h / 2.0,
                            fill: alpha(
                                0x7EE0A0,
                                if picked {
                                    0.2
                                } else if hover {
                                    0.16
                                } else {
                                    0.06 * fade
                                },
                            ),
                            rim: Some(alpha(0x7EE0A0, 0.55 * fade)),
                            dashed: false,
                            sharp: Corner::None,
                        });
                        shapes.push(Shape::Text(Line {
                            text: reply.clone(),
                            at: Point::new(x + w / 2.0, y + pill_h / 2.0),
                            size: PILL,
                            color: alpha(0x9BEBB8, fade),
                            bold: true,
                            align: Alignment::Center,
                        }));
                        if chosen.is_none() {
                            targets.push((target, rect));
                        }
                        x += w + PILL_GAP;
                    }
                    y += pill_h;
                    said.push(format!(
                        "question:{text} [{}]{}",
                        replies.join(" | "),
                        chosen.map_or(String::new(), |c| format!(" chosen {c}"))
                    ));
                }
                Body::Card { card } => {
                    y = lay_out_card(
                        &mut shapes,
                        &mut targets,
                        open,
                        item.id,
                        card,
                        her_x,
                        her_room,
                        y,
                    );
                    said.push(format!(
                        "card:{:?}:{}:{}{}",
                        card.kind,
                        card.label,
                        card.actions
                            .iter()
                            .map(|a| a.label.as_str())
                            .collect::<Vec<_>>()
                            .join("/"),
                        card.outcome
                            .as_deref()
                            .map_or(String::new(), |o| format!(" → {o}"))
                    ));
                }
                Body::Learned { text, undone } => {
                    let h = LEARNED * 1.4 + 10.0;
                    let rect = Rectangle::new(Point::new(her_x, y), Size::new(her_room, h));
                    shapes.push(Shape::Box {
                        rect,
                        radius: 8.0,
                        fill: alpha(0x8FB3FF, 0.08),
                        rim: Some(alpha(0x8FB3FF, 0.4)),
                        dashed: true,
                        sharp: Corner::None,
                    });
                    shapes.push(Shape::Check(Rectangle::new(
                        Point::new(her_x + 10.0, y + (h - 14.0) / 2.0),
                        Size::new(14.0, 14.0),
                    )));
                    let undo = words.undo();
                    let uw = width_of(undo, LEARNED, true);
                    shapes.push(Shape::Text(Line {
                        text: text.clone(),
                        at: Point::new(her_x + 10.0 + 14.0 + 8.0, y + h / 2.0),
                        size: LEARNED,
                        color: if *undone {
                            alpha(0xB9CEFF, 0.5)
                        } else {
                            hex(0xB9CEFF)
                        },
                        bold: false,
                        align: Alignment::Left,
                    }));
                    if !*undone {
                        let at = Rectangle::new(
                            Point::new(her_x + her_room - 10.0 - uw - 2.0, y + 2.0),
                            Size::new(uw + 4.0, h - 4.0),
                        );
                        shapes.push(Shape::Text(Line {
                            text: undo.to_owned(),
                            at: Point::new(her_x + her_room - 10.0, y + h / 2.0),
                            size: LEARNED,
                            color: MINT,
                            bold: true,
                            align: Alignment::Right,
                        }));
                        targets.push((Target::Undo(item.id), at));
                    }
                    said.push(format!(
                        "learned:{text}{}",
                        if *undone { " (undone)" } else { "" }
                    ));
                    y += h;
                }
            },
            Entry::Streaming(text) => {
                y = her_bubble(&mut shapes, text, None, her_x, her_room, y);
                said.push(format!("streaming:{text}"));
            }
            Entry::Typing(doing) => {
                let h = 7.0 + 2.0 * 12.0;
                let rect = Rectangle::new(
                    Point::new(her_x, y),
                    Size::new(14.0 * 2.0 + 3.0 * 7.0 + 2.0 * 5.0, h),
                );
                her_box(&mut shapes, rect);
                shapes.push(Shape::Dots(Point::new(rect.x + 14.0, rect.y + h / 2.0)));
                y += h + INNER_GAP;
                let what = if doing.is_empty() {
                    words.writing()
                } else {
                    doing
                };
                shapes.push(Shape::Text(Line {
                    text: format!("{what}…"),
                    at: Point::new(her_x, y + 11.5 * 0.7),
                    size: 11.5,
                    color: LABEL,
                    bold: false,
                    align: Alignment::Left,
                }));
                said.push(format!("typing:{what}"));
                y += 11.5 * 1.4;
            }
        }
    }
    y += THREAD_BOTTOM;
    let content = y;
    laid.content = content;
    let room = laid.thread.height;
    let scroll = open.scroll.clamp(0.0, (content - room).max(0.0));
    let shift = top + room - content + scroll;
    let shift = if content <= room {
        top + room - content
    } else {
        shift
    };
    for shape in shapes {
        laid.shapes.push(moved(shape, shift));
    }
    for (target, rect) in targets {
        let rect = Rectangle::new(Point::new(rect.x, rect.y + shift), rect.size());
        // Only what shows takes a click.
        if rect.y + rect.height > top && rect.y < top + room {
            laid.targets.push((target, rect));
        }
    }
}

fn moved(shape: Shape, dy: f32) -> Shape {
    let down = |r: Rectangle| Rectangle::new(Point::new(r.x, r.y + dy), r.size());
    match shape {
        Shape::Box {
            rect,
            radius,
            fill,
            rim,
            dashed,
            sharp,
        } => Shape::Box {
            rect: down(rect),
            radius,
            fill,
            rim,
            dashed,
            sharp,
        },
        Shape::Text(mut line) => {
            line.at.y += dy;
            Shape::Text(line)
        }
        Shape::Avatar(p) => Shape::Avatar(Point::new(p.x, p.y + dy)),
        Shape::Panda(p, look) => Shape::Panda(Point::new(p.x, p.y + dy), look),
        Shape::Dots(p) => Shape::Dots(Point::new(p.x, p.y + dy)),
        Shape::Check(r) => Shape::Check(down(r)),
        Shape::Quote(r) => Shape::Quote(down(r)),
        Shape::Rule(r) => Shape::Rule(down(r)),
    }
}

fn her_box(shapes: &mut Vec<Shape>, rect: Rectangle) {
    shapes.push(Shape::Box {
        rect,
        radius: 14.0,
        fill: alpha(0xFFFFFF, 0.055),
        rim: Some(alpha(0xFFFFFF, 0.08)),
        dashed: false,
        sharp: Corner::TopLeft,
    });
}

#[allow(clippy::too_many_arguments)]
fn push_lines(
    shapes: &mut Vec<Shape>,
    lines: &[String],
    x: f32,
    top: f32,
    size: f32,
    height: f32,
    color: Color,
    bold: bool,
) {
    for (n, text) in lines.iter().enumerate() {
        shapes.push(Shape::Text(Line {
            text: text.clone(),
            at: Point::new(x, top + n as f32 * height + height / 2.0),
            size,
            color,
            bold,
            align: Alignment::Left,
        }));
    }
}

/// Her bubble at `y`: its bottom.
fn her_bubble(
    shapes: &mut Vec<Shape>,
    text: &str,
    note: Option<&str>,
    x: f32,
    room: f32,
    y: f32,
) -> f32 {
    let inner = room - 2.0 * BUBBLE_X;
    let lines = lines_of(text, BUBBLE, false, inner);
    let notes = note
        .map(|n| lines_of(n, SMALL, false, inner))
        .unwrap_or_default();
    let w = lines
        .iter()
        .map(|l| width_of(l, BUBBLE, false))
        .chain(notes.iter().map(|l| width_of(l, SMALL, false)))
        .fold(0.0, f32::max)
        + 2.0 * BUBBLE_X;
    let h = lines.len() as f32 * BUBBLE_LINE
        + if notes.is_empty() {
            0.0
        } else {
            3.0 + notes.len() as f32 * SMALL_LINE
        }
        + 2.0 * BUBBLE_Y;
    let rect = Rectangle::new(Point::new(x, y), Size::new(w.min(room), h));
    her_box(shapes, rect);
    push_lines(
        shapes,
        &lines,
        x + BUBBLE_X,
        y + BUBBLE_Y,
        BUBBLE,
        BUBBLE_LINE,
        TEXT,
        false,
    );
    let note_top = y + BUBBLE_Y + lines.len() as f32 * BUBBLE_LINE + 3.0;
    push_lines(
        shapes,
        &notes,
        x + BUBBLE_X,
        note_top,
        SMALL,
        SMALL_LINE,
        DIM,
        false,
    );
    y + h
}

/// A card at `y`: its label, its text, its lines, its buttons (at most
/// three, the first the primary), or what came of it; its bottom.
#[allow(clippy::too_many_arguments)]
fn lay_out_card(
    shapes: &mut Vec<Shape>,
    targets: &mut Vec<(Target, Rectangle)>,
    open: &Open,
    item: u64,
    card: &Card,
    x: f32,
    room: f32,
    y: f32,
) -> f32 {
    let (rim, fill, label_colour) = card_colours(card.kind);
    let inner = room - 2.0 * CARD_X;
    let mut parts: Vec<Shape> = Vec::new();
    let mut at = y + CARD_TOP;
    let label = card.label.to_uppercase();
    parts.push(Shape::Text(Line {
        text: label,
        at: Point::new(x + CARD_X, at + CARD_LABEL * 0.7),
        size: CARD_LABEL,
        color: label_colour,
        bold: true,
        align: Alignment::Left,
    }));
    at += CARD_LABEL * 1.4 + 4.0;
    if !card.text.is_empty() {
        let lines = lines_of(&card.text, CARD_TEXT, false, inner);
        push_lines(
            &mut parts,
            &lines,
            x + CARD_X,
            at,
            CARD_TEXT,
            CARD_LINE,
            TEXT,
            false,
        );
        at += lines.len() as f32 * CARD_LINE;
    }
    let draft = card.kind == CardKind::Draft;
    for (n, text) in card.lines.iter().enumerate() {
        let (indent, shown) = if draft {
            (12.0, text.clone())
        } else if card.lines.len() > 1 {
            (0.0, format!("{}. {text}", n + 1))
        } else {
            (0.0, text.clone())
        };
        let lines = lines_of(&shown, CARD_TEXT, false, inner - indent);
        let h = lines.len() as f32 * CARD_LINE;
        if draft {
            parts.push(Shape::Quote(Rectangle::new(
                Point::new(x + CARD_X, at + 2.0),
                Size::new(2.0, h + 4.0),
            )));
            at += 4.0;
        }
        push_lines(
            &mut parts,
            &lines,
            x + CARD_X + indent,
            at,
            CARD_TEXT,
            CARD_LINE,
            hex(0xE6EAF1),
            false,
        );
        at += h + if draft { 4.0 } else { 4.0 };
    }
    match (&card.decided, &card.outcome) {
        (Some(_), outcome) | (None, outcome @ Some(_)) => {
            let text = outcome.clone().unwrap_or_default();
            at += 4.0;
            let lines = lines_of(&format!("✓ {text}"), SMALL, true, inner);
            push_lines(
                &mut parts,
                &lines,
                x + CARD_X,
                at,
                SMALL,
                SMALL_LINE,
                MINT,
                true,
            );
            at += lines.len() as f32 * SMALL_LINE;
        }
        (None, None) => {
            at += BUTTON_TOP;
            let button_h = BUTTON * 1.4 + 2.0 * BUTTON_Y;
            let mut bx = x + CARD_X;
            for (n, action) in card.actions.iter().take(3).enumerate() {
                let w = width_of(&action.label, BUTTON, true) + 2.0 * BUTTON_X;
                if bx > x + CARD_X && bx + w > x + room - CARD_X {
                    bx = x + CARD_X;
                    at += button_h + 6.0;
                }
                let rect = Rectangle::new(Point::new(bx, at), Size::new(w, button_h));
                let target = Target::Button {
                    item,
                    decision: action.decision,
                };
                let hover = open.hover == Some(target);
                let primary = n == 0;
                parts.push(Shape::Box {
                    rect,
                    radius: 8.0,
                    fill: match (primary, hover) {
                        (true, false) => GREEN,
                        (true, true) => hex(0x7ED198),
                        (false, false) => alpha(0xFFFFFF, 0.07),
                        (false, true) => alpha(0xFFFFFF, 0.12),
                    },
                    rim: (!primary).then_some(alpha(0xFFFFFF, 0.14)),
                    dashed: false,
                    sharp: Corner::None,
                });
                parts.push(Shape::Text(Line {
                    text: action.label.clone(),
                    at: Point::new(bx + w / 2.0, at + button_h / 2.0),
                    size: BUTTON,
                    color: if primary { INK } else { hex(0xD5DBE5) },
                    bold: true,
                    align: Alignment::Center,
                }));
                targets.push((target, rect));
                bx += w + 6.0;
            }
            at += button_h;
        }
    }
    at += CARD_BOTTOM;
    shapes.push(Shape::Box {
        rect: Rectangle::new(Point::new(x, y), Size::new(room, at - y)),
        radius: 12.0,
        fill: if draft {
            fill
        } else {
            Color {
                a: 1.0,
                ..hex(0x121827)
            }
        },
        rim: Some(rim),
        dashed: false,
        sharp: Corner::None,
    });
    if !draft {
        shapes.push(Shape::Box {
            rect: Rectangle::new(Point::new(x, y), Size::new(room, at - y)),
            radius: 12.0,
            fill,
            rim: None,
            dashed: false,
            sharp: Corner::None,
        });
    }
    shapes.extend(parts);
    at
}

/// Her briefing (4.6) in the drawer: who waits, who is stuck, who is to
/// see, who works, who rests; a line goes to its agent.
fn lay_out_briefing(
    laid: &mut Laid,
    agents_shown: &[hocus_link::AgentShown],
    words: Words,
    top: f32,
    right: f32,
) {
    let brief = Brief::of(agents_shown);
    let mut y = top + THREAD_TOP;
    let back = words.back();
    let bw = width_of(back, SMALL, true);
    laid.shapes.push(Shape::Text(Line {
        text: back.to_owned(),
        at: Point::new(PAD, y + SMALL_LINE / 2.0),
        size: SMALL,
        color: MINT,
        bold: true,
        align: Alignment::Left,
    }));
    laid.targets.push((
        Target::Back,
        Rectangle::new(
            Point::new(PAD - 4.0, y - 2.0),
            Size::new(bw + 8.0, SMALL_LINE + 4.0),
        ),
    ));
    y += SMALL_LINE + GAP;
    let agent_words = agents::Words::new(if words.fr {
        crate::clock::time::Lang::French
    } else {
        crate::clock::time::Lang::English
    });
    let now = agents::now_us();
    let sections = [
        (
            words.pick("T’ATTEND", "WAITING"),
            &brief.waiting,
            AgentLook::Waiting,
            hex(0xF5C46E),
        ),
        (
            words.pick("BLOQUÉ", "STUCK"),
            &brief.stuck,
            AgentLook::Stuck,
            hex(0xF7A77A),
        ),
        (
            words.pick("À VOIR", "TO SEE"),
            &brief.to_see,
            AgentLook::ToSee,
            MINT,
        ),
        (
            words.pick("AU TRAVAIL", "AT WORK"),
            &brief.working,
            AgentLook::Working,
            DIM,
        ),
        (
            words.pick("AU REPOS", "RESTING"),
            &brief.resting,
            AgentLook::Resting,
            hex(0x6A7384),
        ),
    ];
    for (label, entries, look, colour) in sections {
        if entries.is_empty() {
            continue;
        }
        laid.shapes.push(Shape::Text(Line {
            text: label.to_owned(),
            at: Point::new(PAD, y + 11.0 * 0.7),
            size: 11.0,
            color: colour,
            bold: true,
            align: Alignment::Left,
        }));
        y += 11.0 * 1.4 + 4.0;
        for entry in entries.iter() {
            let rect = Rectangle::new(
                Point::new(PAD - 6.0, y),
                Size::new(right - PAD + 12.0, 44.0),
            );
            let minutes = now.saturating_sub(entry.since) / 60_000_000;
            let since = if words.fr {
                format!("depuis {minutes} min")
            } else {
                format!("for {minutes} min")
            };
            laid.shapes.push(Shape::Text(Line {
                text: entry.name.clone(),
                at: Point::new(PAD + 40.0, y + 13.0),
                size: 13.0,
                color: TEXT,
                bold: true,
                align: Alignment::Left,
            }));
            laid.shapes.push(Shape::Text(Line {
                text: since,
                at: Point::new(right, y + 13.0),
                size: 11.5,
                color: LABEL,
                bold: false,
                align: Alignment::Right,
            }));
            // What its tool said, for a waiting or a stuck one (4.6).
            let note = entry
                .note
                .as_ref()
                .filter(|_| matches!(look, AgentLook::Waiting | AgentLook::Stuck))
                .map(|n| agent_words.note(n))
                .unwrap_or_default();
            if !note.is_empty() {
                let room = right - PAD - 40.0;
                let shown = crate::popover::kit::elide(&note, room, |s| width_of(s, 12.5, false));
                laid.shapes.push(Shape::Text(Line {
                    text: shown,
                    at: Point::new(PAD + 40.0, y + 31.0),
                    size: 12.5,
                    color: DIM,
                    bold: false,
                    align: Alignment::Left,
                }));
            }
            laid.shapes
                .push(Shape::Panda(Point::new(PAD, y + 3.0), look));
            laid.targets.push((Target::Agent(entry.id), rect));
            y += 46.0;
        }
        y += 6.0;
    }
}

// --- Drawing --------------------------------------------------------------------------------

/// The drawer on its surface.
#[derive(Debug, Clone)]
pub struct View {
    pub id: window::Id,
    pub words: Words,
    /// The compositor blurs behind: the tint over it, else opaque.
    pub glass: bool,
}

/// What the surface keeps between draws: the layout for its key, what it
/// last logged.
#[derive(Debug, Default)]
pub struct State {
    laid: Option<((u64, u64, u32, u32, Option<Target>, u8), Laid)>,
    logged: String,
    places: String,
}

/// The key a layout is made for: the conversation's and the drawer's
/// changes, its size, what the pointer is on, her line.
fn key(open: &Open, c: &Conversation, size: Size) -> (u64, u64, u32, u32, Option<Target>, u8) {
    (
        c.rev,
        open.rev,
        size.width.to_bits(),
        size.height.to_bits(),
        open.hover,
        crate::secretary::state() as u8,
    )
}

impl View {
    /// The layout for now, made again when what it shows changed.
    fn laid<'a>(&self, state: &'a mut State, size: Size) -> Option<&'a Laid> {
        let made = super::with(|d| {
            let open = d.open_mut(self.id)?;
            open.size = size;
            crate::secretary::conversation(|c| {
                let key = key(open, c, size);
                if state.laid.as_ref().is_some_and(|(k, _)| *k == key) {
                    return Some(None);
                }
                let laid = lay_out(open, c, crate::secretary::state(), self.words, size);
                Some(Some((key, laid)))
            })
        });
        match made {
            None => None,
            Some(Some(fresh)) => {
                state.laid = Some(fresh);
                state.laid.as_ref().map(|(_, l)| l)
            }
            Some(None) => state.laid.as_ref().map(|(_, l)| l),
        }
    }

    /// Before it draws: her mood followed (a change plays her motion),
    /// what it shows and where logged when they changed.
    fn prepare(&self, state: &mut State, size: Size) -> bool {
        let Some(laid) = self.laid(state, size) else {
            return false;
        };
        let said = laid.said.to_string();
        let places: Vec<String> = laid
            .targets
            .iter()
            .map(|(t, r)| format!("{}@{:.0},{:.0}", t.name(), r.center_x(), r.center_y()))
            .collect();
        let places = places.join(" ");
        let mood = laid.mood;
        if said != state.logged {
            tracing::debug!("the drawer shows {said}");
            state.logged = said;
        }
        if places != state.places {
            tracing::debug!("the drawer's items {places}");
            state.places = places;
        }
        let now = Instant::now();
        super::with(|d| {
            let Some(open) = d.open_mut(self.id) else {
                return false;
            };
            if let Some(mood) = mood
                && open.mood.map(|(m, _)| m) != Some(mood)
            {
                tracing::debug!(mood = mood.word(), "her mood in the drawer");
                // Opening plays her mood from the start.
                let since = if open.mood.is_none() {
                    open.opened
                } else {
                    now
                };
                open.mood = Some((mood, since));
            }
            let Some((mood, since)) = open.mood else {
                return false;
            };
            pandas::secretary(
                super::panda_mood(mood),
                Some((since, super::mood_window(mood))),
                now,
            )
            .1
        })
    }

    fn background(&self, f: &mut Frame, size: Size) {
        let (top, bottom) = if self.glass {
            (alpha(0x1C2133, 0.985), alpha(0x10131E, 0.99))
        } else {
            (hex(0x1C2133), hex(0x10131E))
        };
        let tint = canvas::gradient::Linear::new(Point::ORIGIN, Point::new(0.0, size.height))
            .add_stop(0.0, top)
            .add_stop(1.0, bottom);
        f.fill(
            &Path::rounded_rectangle(Point::ORIGIN, size, RADIUS.into()),
            canvas::Fill::from(tint),
        );
        f.stroke(
            &Path::rounded_rectangle(
                Point::new(0.5, 0.5),
                Size::new(size.width - 1.0, size.height - 1.0),
                (RADIUS - 0.5).into(),
            ),
            Stroke::default()
                .with_color(alpha(0xFFFFFF, 0.13))
                .with_width(1.0),
        );
    }

    fn draw_laid(&self, f: &mut Frame, laid: &Laid, open: &Open, now: Instant) {
        let words = self.words;
        let size = laid.size;
        let right = size.width - PAD;
        // Her head, in her mood.
        let (handle, _) = match open.mood {
            Some((mood, since)) => pandas::secretary(
                super::panda_mood(mood),
                Some((since, super::mood_window(mood))),
                now,
            ),
            None => pandas::secretary(
                super::panda_mood(laid.mood.unwrap_or(Mood::Listen)),
                None,
                now,
            ),
        };
        f.draw_svg(laid.head, svg::Svg::new(handle));
        let text_x = laid.head.x + HEAD + HEAD_GAP;
        let middle = laid.head.center_y();
        f.fill_text(text(
            words.title(),
            Point::new(text_x, middle - 10.0),
            TITLE,
            TEXT,
            true,
            Alignment::Left,
        ));
        let dot = match laid.tone {
            Some(Tone::Ok) => GREEN,
            Some(Tone::Warn) => AMBER,
            Some(Tone::Bad) => ORANGE,
            Some(Tone::Off) | None => GREY,
        };
        let status_y = middle + 11.0;
        f.fill(
            &Path::circle(Point::new(text_x + DOT / 2.0, status_y), DOT / 2.0),
            dot,
        );
        let room = laid
            .targets
            .iter()
            .find(|(t, _)| *t == Target::Gear)
            .map_or(right, |(_, r)| r.x)
            - text_x
            - DOT
            - 6.0
            - 8.0;
        let status = crate::popover::kit::elide(&laid.status, room, |s| width_of(s, STATUS, false));
        f.fill_text(text(
            &status,
            Point::new(text_x + DOT + 6.0, status_y),
            STATUS,
            DIM,
            false,
            Alignment::Left,
        ));
        for (target, rect) in &laid.targets {
            if !matches!(target, Target::Gear | Target::Close) {
                continue;
            }
            let hover = open.hover == Some(*target);
            f.fill(
                &Path::rounded_rectangle(rect.position(), rect.size(), 7.0.into()),
                alpha(0xFFFFFF, if hover { 0.1 } else { 0.05 }),
            );
            match target {
                Target::Gear => gear(f, rect.center()),
                _ => cross(f, rect.center()),
            }
        }
        // The tabs.
        f.fill(
            &Path::rounded_rectangle(laid.tabs.position(), laid.tabs.size(), 9.0.into()),
            alpha(0xFFFFFF, 0.05),
        );
        f.stroke(
            &Path::rounded_rectangle(
                Point::new(laid.tabs.x + 0.5, laid.tabs.y + 0.5),
                Size::new(laid.tabs.width - 1.0, laid.tabs.height - 1.0),
                8.5.into(),
            ),
            Stroke::default()
                .with_color(alpha(0xFFFFFF, 0.08))
                .with_width(1.0),
        );
        for (tab, rect) in &laid.tab_rects {
            let on = *tab == open.tab;
            if on {
                f.fill(
                    &Path::rounded_rectangle(rect.position(), rect.size(), 7.0.into()),
                    alpha(0xFFFFFF, 0.12),
                );
            }
            let word = match tab {
                Tab::Talk => words.talk(),
                Tab::Knows => words.knows(),
            };
            f.fill_text(text(
                word,
                rect.center(),
                TAB_TEXT,
                if on { TEXT } else { DIM },
                true,
                Alignment::Center,
            ));
        }
        // The agents' line.
        if let Some((rect, said, count, look)) = &laid.agents {
            let hover = open.hover == Some(Target::Agents);
            f.fill(
                &Path::rounded_rectangle(rect.position(), rect.size(), 9.0.into()),
                alpha(0xF5B44A, if hover { 0.16 } else { 0.10 }),
            );
            let mood = pandas::mood(*look);
            let panda = pandas::drawing(mood, Some(pandas::Burst::of(mood, open.opened)), now);
            f.draw_svg(
                Rectangle::new(
                    Point::new(rect.x + 4.0, rect.y + 3.0),
                    Size::new(AGENT_PANDA, AGENT_PANDA),
                ),
                svg::Svg::new(panda),
            );
            let cw = width_of(count, 12.0, false);
            f.fill_text(text(
                count,
                Point::new(rect.x + rect.width - 10.0, rect.center_y()),
                12.0,
                DIM,
                false,
                Alignment::Right,
            ));
            let room = rect.width - 4.0 - AGENT_PANDA - 8.0 - 10.0 - cw - 8.0;
            let shown = crate::popover::kit::elide(said, room, |s| width_of(s, AGENTS_TEXT, true));
            f.fill_text(text(
                &shown,
                Point::new(rect.x + 4.0 + AGENT_PANDA + 8.0, rect.center_y()),
                AGENTS_TEXT,
                hex(0xF5C46E),
                true,
                Alignment::Left,
            ));
        }
        // The thread, under its rule.
        f.fill_rectangle(
            Point::new(1.0, laid.thread.y),
            Size::new(size.width - 2.0, 1.0),
            alpha(0xFFFFFF, 0.06),
        );
        let clip = Rectangle::new(
            Point::new(0.0, laid.thread.y + 1.0),
            Size::new(size.width, laid.thread.height - 1.0),
        );
        f.with_clip(clip, |f| {
            // The frame is translated to the clip's corner.
            f.translate(iced::Vector::new(0.0, -clip.y));
            for shape in &laid.shapes {
                draw_shape(f, shape, now);
            }
        });
        // The composer.
        f.fill_rectangle(
            Point::new(1.0, laid.field.y - COMPOSER_TOP),
            Size::new(size.width - 2.0, 1.0),
            alpha(0xFFFFFF, 0.07),
        );
        let field = laid.field;
        f.fill(
            &Path::rounded_rectangle(field.position(), field.size(), 12.0.into()),
            alpha(0xFFFFFF, 0.05),
        );
        f.stroke(
            &Path::rounded_rectangle(
                Point::new(field.x + 0.5, field.y + 0.5),
                Size::new(field.width - 1.0, field.height - 1.0),
                11.5.into(),
            ),
            Stroke::default()
                .with_color(alpha(0x6CC287, 0.45))
                .with_width(1.0),
        );
        let empty = open.field.is_empty();
        let lines_top = field.y + (field.height - laid.field_lines.len() as f32 * FIELD_LINE) / 2.0;
        let mut caret = Point::new(field.x + 14.0, lines_top + FIELD_LINE / 2.0);
        for (n, line) in laid.field_lines.iter().enumerate() {
            let at = Point::new(
                field.x + 14.0,
                lines_top + n as f32 * FIELD_LINE + FIELD_LINE / 2.0,
            );
            f.fill_text(text(
                line,
                at,
                FIELD_TEXT,
                if empty { FAINT } else { TEXT },
                false,
                Alignment::Left,
            ));
            if !empty {
                caret = Point::new(at.x + width_of(line, FIELD_TEXT, false) + 1.0, at.y);
            }
        }
        // Its caret, still: nothing blinks at rest.
        f.fill_rectangle(
            Point::new(caret.x, caret.y - 8.0),
            Size::new(1.5, 16.0),
            MINT,
        );
        let send = laid.send;
        let hover = open.hover == Some(Target::Send);
        f.fill(
            &Path::rounded_rectangle(send.position(), send.size(), 9.0.into()),
            if hover { hex(0x7ED198) } else { GREEN },
        );
        arrow(f, send.center());
        f.fill_text(text(
            words.keys(),
            Point::new(PAD, laid.foot),
            FOOT,
            FAINT,
            false,
            Alignment::Left,
        ));
        if !laid.model.is_empty() {
            let mut model = text(
                &laid.model,
                Point::new(right, laid.foot),
                10.5,
                FAINT,
                false,
                Alignment::Right,
            );
            model.font = iced::Font::with_name("Martian Mono");
            f.fill_text(model);
        }
    }
}

fn text(
    content: &str,
    position: Point,
    size: f32,
    color: Color,
    bold: bool,
    align: Alignment,
) -> canvas::Text {
    canvas::Text {
        content: content.to_owned(),
        position,
        max_width: f32::INFINITY,
        color,
        size: Pixels(size),
        line_height: LineHeight::Relative(1.366),
        font: if bold { MANROPE_BOLD } else { MANROPE },
        align_x: align,
        align_y: alignment::Vertical::Center,
        shaping: Shaping::Advanced,
    }
}

/// A rounded box whose `sharp` corner has a 4 px radius.
fn bubble_path(rect: Rectangle, radius: f32, sharp: Corner) -> Path {
    let radii = match sharp {
        Corner::None => [radius; 4],
        Corner::TopLeft => [4.0, radius, radius, radius],
        Corner::BottomRight => [radius, radius, 4.0, radius],
    };
    Path::rounded_rectangle(
        rect.position(),
        rect.size(),
        iced::border::Radius {
            top_left: radii[0],
            top_right: radii[1],
            bottom_right: radii[2],
            bottom_left: radii[3],
        },
    )
}

fn draw_shape(f: &mut Frame, shape: &Shape, now: Instant) {
    match shape {
        Shape::Box {
            rect,
            radius,
            fill,
            rim,
            dashed,
            sharp,
        } => {
            let path = bubble_path(*rect, *radius, *sharp);
            f.fill(&path, *fill);
            if let Some(rim) = rim {
                let inner = Rectangle::new(
                    Point::new(rect.x + 0.5, rect.y + 0.5),
                    Size::new(rect.width - 1.0, rect.height - 1.0),
                );
                let mut stroke = Stroke::default().with_color(*rim).with_width(1.0);
                if *dashed {
                    stroke.line_dash = canvas::LineDash {
                        segments: &[4.0, 3.0],
                        offset: 0,
                    };
                }
                f.stroke(&bubble_path(inner, radius - 0.5, *sharp), stroke);
            }
        }
        Shape::Text(line) => f.fill_text(text(
            &line.text, line.at, line.size, line.color, line.bold, line.align,
        )),
        Shape::Panda(at, look) => {
            let panda = pandas::drawing(pandas::mood(*look), None, now);
            f.draw_svg(
                Rectangle::new(*at, Size::new(32.0, 32.0)),
                svg::Svg::new(panda),
            );
        }
        Shape::Avatar(at) => {
            let (handle, _) = pandas::secretary(hocus_pandas::SecretaryMood::Listen, None, now);
            f.draw_svg(
                Rectangle::new(*at, Size::new(AVATAR, AVATAR)),
                svg::Svg::new(handle),
            );
        }
        Shape::Dots(at) => {
            for n in 0..3 {
                f.fill(
                    &Path::circle(Point::new(at.x + 3.5 + n as f32 * 12.0, at.y), 3.5),
                    DIM,
                );
            }
        }
        Shape::Check(rect) => {
            f.fill(
                &Path::rounded_rectangle(rect.position(), rect.size(), 4.0.into()),
                hex(0x8FB3FF),
            );
            let tick = Path::new(|p| {
                p.move_to(Point::new(rect.x + 3.5, rect.y + 7.5));
                p.line_to(Point::new(rect.x + 6.0, rect.y + 10.0));
                p.line_to(Point::new(rect.x + 10.5, rect.y + 4.5));
            });
            f.stroke(&tick, Stroke::default().with_color(INK).with_width(2.0));
        }
        Shape::Quote(rect) => f.fill_rectangle(rect.position(), rect.size(), alpha(0x7EE0A0, 0.5)),
        Shape::Rule(rect) => {
            // Either side of the words, centred on the drawer.
            let middle = rect.x + rect.width / 2.0;
            let half = rect.height / 2.0;
            let line = alpha(0xFFFFFF, 0.08);
            f.fill_rectangle(
                Point::new(rect.x, rect.y),
                Size::new((middle - half - rect.x).max(0.0), 1.0),
                line,
            );
            f.fill_rectangle(
                Point::new(middle + half, rect.y),
                Size::new((rect.x + rect.width - middle - half).max(0.0), 1.0),
                line,
            );
        }
    }
}

fn gear(f: &mut Frame, c: Point) {
    let teeth = 8;
    let cog = Path::new(|b| {
        for i in 0..teeth * 2 {
            let a = i as f32 * std::f32::consts::PI / teeth as f32;
            let r = if i % 2 == 0 { 7.0 } else { 5.4 };
            let half = std::f32::consts::PI / teeth as f32 / 2.0;
            let at = |a: f32| Point::new(c.x + r * a.cos(), c.y + r * a.sin());
            if i == 0 {
                b.move_to(at(a - half));
            } else {
                b.line_to(at(a - half));
            }
            b.line_to(at(a + half));
        }
        b.close();
    });
    let stroke = Stroke::default().with_color(DIM).with_width(1.5);
    f.stroke(&cog, stroke);
    f.stroke(&Path::circle(c, 2.4), stroke);
}

fn cross(f: &mut Frame, c: Point) {
    let r = 5.0;
    let path = Path::new(|p| {
        p.move_to(Point::new(c.x - r, c.y - r));
        p.line_to(Point::new(c.x + r, c.y + r));
        p.move_to(Point::new(c.x + r, c.y - r));
        p.line_to(Point::new(c.x - r, c.y + r));
    });
    f.stroke(&path, Stroke::default().with_color(DIM).with_width(1.6));
}

fn arrow(f: &mut Frame, c: Point) {
    let path = Path::new(|p| {
        p.move_to(Point::new(c.x - 5.0, c.y));
        p.line_to(Point::new(c.x + 5.0, c.y));
        p.move_to(Point::new(c.x + 1.0, c.y - 4.0));
        p.line_to(Point::new(c.x + 5.0, c.y));
        p.line_to(Point::new(c.x + 1.0, c.y + 4.0));
    });
    f.stroke(&path, Stroke::default().with_color(INK).with_width(2.0));
}

impl<Message> canvas::Program<Message> for View {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        match event {
            iced::Event::Window(window::Event::RedrawRequested(_)) => {
                let plays = self.prepare(state, bounds.size());
                plays.then(canvas::Action::request_redraw)
            }
            iced::Event::Mouse(seen) => {
                let at = cursor.position_in(bounds);
                self.mouse(state, seen, at, bounds.size())
            }
            iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) => super::key_on(self.id, key, *modifiers, text.as_deref())
                .then(|| canvas::Action::request_redraw().and_capture()),
            _ => None,
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
        self.background(&mut frame, bounds.size());
        let now = Instant::now();
        super::with(|d| {
            let Some(open) = d.open_on(self.id) else {
                return;
            };
            match &state.laid {
                Some((_, laid)) => self.draw_laid(&mut frame, laid, open, now),
                None => {
                    let laid = crate::secretary::conversation(|c| {
                        lay_out(
                            open,
                            c,
                            crate::secretary::state(),
                            self.words,
                            bounds.size(),
                        )
                    });
                    self.draw_laid(&mut frame, &laid, open, now);
                }
            }
        });
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        super::with(|d| match d.open_on(self.id).and_then(|o| o.hover) {
            Some(Target::Field) => mouse::Interaction::Text,
            Some(_) => mouse::Interaction::Pointer,
            None => mouse::Interaction::default(),
        })
    }
}

impl View {
    fn mouse<Message>(
        &self,
        state: &mut State,
        seen: &mouse::Event,
        at: Option<Point>,
        size: Size,
    ) -> Option<canvas::Action<Message>> {
        let targets: Vec<(Target, Rectangle)> = self
            .laid(state, size)
            .map(|l| l.targets.clone())
            .unwrap_or_default();
        let content = state
            .laid
            .as_ref()
            .map_or(0.0, |(_, l)| l.content - l.thread.height);
        let target = at.and_then(|at| {
            targets
                .iter()
                .rev()
                .find(|(_, r)| r.contains(at))
                .map(|(t, _)| *t)
        });
        enum Then {
            Nothing,
            Draw,
            Act(Target),
        }
        let then = super::with(|d| {
            let open = d.open_mut(self.id)?;
            open.pointer = at;
            Some(match seen {
                mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                    if open.hover == target {
                        Then::Nothing
                    } else {
                        open.hover = target;
                        Then::Draw
                    }
                }
                mouse::Event::CursorLeft => {
                    open.hover = None;
                    Then::Draw
                }
                mouse::Event::ButtonPressed(mouse::Button::Left) => {
                    open.pressed = target;
                    Then::Nothing
                }
                mouse::Event::ButtonReleased(mouse::Button::Left) => {
                    match open.pressed.take().filter(|p| Some(*p) == target) {
                        Some(target) => Then::Act(target),
                        None => Then::Nothing,
                    }
                }
                mouse::Event::WheelScrolled { delta } => {
                    let step = match delta {
                        mouse::ScrollDelta::Lines { y, .. } => y * 40.0,
                        mouse::ScrollDelta::Pixels { y, .. } => *y,
                    };
                    let before = open.scroll;
                    open.scroll = (open.scroll + step).clamp(0.0, content.max(0.0));
                    if (open.scroll - before).abs() < 0.5 {
                        return Some(Then::Nothing);
                    }
                    open.rev += 1;
                    Then::Draw
                }
                _ => Then::Nothing,
            })
        });
        match then {
            None | Some(Then::Nothing) => None,
            Some(Then::Draw) => Some(canvas::Action::request_redraw().and_capture()),
            Some(Then::Act(target)) => {
                super::act(self.id, target);
                Some(canvas::Action::request_redraw().and_capture())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn her_answer_streams_after_the_sentence_and_her_typing_ends_the_thread() {
        let item = |id, request, body| Item {
            id,
            at: "2026-11-09T08:00:00Z".to_owned(),
            request: Some(request),
            body,
        };
        let c = Conversation {
            items: vec![
                item(
                    1,
                    1,
                    Body::User {
                        text: "a".to_owned(),
                    },
                ),
                item(
                    2,
                    2,
                    Body::User {
                        text: "b".to_owned(),
                    },
                ),
            ],
            streaming: vec![(1, "Bien".to_owned())],
            typing: Some((2, String::new())),
            look: None,
            rev: 1,
        };
        let words = Words { fr: true };
        let kinds: Vec<&str> = entries(&c, words)
            .iter()
            .map(|e| match e {
                Entry::Day(_) => "day",
                Entry::Item(_) => "item",
                Entry::Streaming(_) => "streaming",
                Entry::Typing(_) => "typing",
            })
            .collect();
        assert_eq!(kinds, ["day", "item", "streaming", "item", "typing"]);
        // Once her answer streams, no dots for it.
        let streaming = Conversation {
            streaming: vec![(2, "Ok".to_owned())],
            ..c
        };
        assert!(
            !entries(&streaming, words)
                .iter()
                .any(|e| matches!(e, Entry::Typing(_)))
        );
    }
}
