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

mod knows;
mod queue;

use std::time::Instant;

use hocus_secretary_link::{
    Body, Briefing, BriefingKind, Card, CardKind, Item, Mood, Notebook, Tone,
};
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

use super::{
    Open, Tab, Target, Words,
    select::{self, Hold, Selection, Source, Texts},
};
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
        // The mockup's consent sheet: its glass, its rim.
        CardKind::Consent => (alpha(0xFFFFFF, 0.12), hex(0x1B2130), LABEL),
    }
}

/// A consent sheet's sizes (the s11 mockup's sheet, in the thread).
const SHEET_TITLE: f32 = 14.5;
const SHEET_FROM: f32 = 11.5;
const SHEET_TEXT: f32 = 12.5;
const SHEET_LINE: f32 = SHEET_TEXT * 1.45;
const SHEET_MARK: f32 = 16.0;
/// The hint under her header (how to fix what her status says).
const HINT: f32 = 11.5;
const HINT_LINE: f32 = HINT * 1.4;
const HINT_PAD: f32 = 7.0;
/// Her header's queue button's text, and a sub-view's bar's (#758).
const QUEUE_TEXT: f32 = 11.5;
const SUB_TEXT: f32 = 12.5;

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
    /// A draft's quote: its bar.
    Quote(Rectangle),
    /// A day's divider: its two rules either side of its words.
    Rule(Rectangle),
    /// A line in Martian Mono (a briefing's times).
    Mono(Line),
    /// Her head at a briefing's top, happy.
    Her(Rectangle),
    /// A plan's box, ticked or not.
    Tick(Rectangle, bool),
    /// The search field's lens, 15 px, centred there.
    Lens(Point),
    /// « Noté »'s bar (#758): full when `item` first showed (`since`, set
    /// as it is drawn), empty ten seconds later.
    Countdown {
        rect: Rectangle,
        item: u64,
        since: Option<Instant>,
    },
}

/// How long « Noté » can be taken back (the brain's ten seconds).
pub(super) const NOTED: std::time::Duration = std::time::Duration::from_secs(10);

/// Her first run's steps (#758): « POUR COMMENCER », each a row with its
/// mark (✓ done, its number), its title (bold now), its note; its bottom.
fn lay_out_steps(
    shapes: &mut Vec<Shape>,
    steps: &[hocus_secretary_link::Step],
    x: f32,
    room: f32,
    y: f32,
    words: Words,
) -> f32 {
    use hocus_secretary_link::StepState;
    let mut parts = Vec::new();
    let mut at = y + CARD_TOP;
    parts.push(Shape::Text(Line {
        text: words.pick("POUR COMMENCER", "TO BEGIN").to_owned(),
        at: Point::new(x + CARD_X, at + CARD_LABEL * 0.7),
        size: CARD_LABEL,
        color: LABEL,
        bold: true,
        align: Alignment::Left,
    }));
    at += CARD_LABEL * 1.4 + 6.0;
    for (n, step) in steps.iter().enumerate() {
        let mark = 18.0;
        let note_w = if step.note.is_empty() {
            0.0
        } else {
            width_of(&step.note, SMALL, false) + 8.0
        };
        let now = step.state == StepState::Now;
        let lines = lines_of(
            &step.title,
            13.0,
            now,
            room - 2.0 * CARD_X - mark - 10.0 - note_w,
        );
        let h = (lines.len() as f32 * 13.0 * 1.4).max(mark) + 8.0;
        let centre = Point::new(x + CARD_X + mark / 2.0, at + h / 2.0);
        let (fill, colour, sign) = match step.state {
            StepState::Done => (GREEN, INK, "✓".to_owned()),
            StepState::Now => (AMBER, INK, (n + 1).to_string()),
            StepState::Later => (alpha(0xFFFFFF, 0.08), DIM, (n + 1).to_string()),
        };
        parts.push(Shape::Box {
            rect: Rectangle::new(
                Point::new(centre.x - mark / 2.0, centre.y - mark / 2.0),
                Size::new(mark, mark),
            ),
            radius: mark / 2.0,
            fill,
            rim: None,
            dashed: false,
            sharp: Corner::None,
        });
        parts.push(Shape::Text(Line {
            text: sign,
            at: centre,
            size: 10.5,
            color: colour,
            bold: true,
            align: Alignment::Center,
        }));
        let top = at + (h - lines.len() as f32 * 13.0 * 1.4) / 2.0;
        push_lines(
            &mut parts,
            &lines,
            x + CARD_X + mark + 10.0,
            top,
            13.0,
            13.0 * 1.4,
            match step.state {
                StepState::Later => DIM,
                StepState::Done => hex(0xC9D0DC),
                StepState::Now => TEXT,
            },
            now,
        );
        if !step.note.is_empty() {
            parts.push(Shape::Text(Line {
                text: step.note.clone(),
                at: Point::new(x + room - CARD_X, at + h / 2.0),
                size: SMALL,
                color: LABEL,
                bold: false,
                align: Alignment::Right,
            }));
        }
        at += h;
    }
    at += CARD_BOTTOM - 4.0;
    shapes.push(Shape::Box {
        rect: Rectangle::new(Point::new(x, y), Size::new(room, at - y)),
        radius: 12.0,
        fill: alpha(0xFFFFFF, 0.03),
        rim: Some(alpha(0xFFFFFF, 0.1)),
        dashed: false,
        sharp: Corner::None,
    });
    shapes.extend(parts);
    at
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
    /// Her header's title: her name when she has one (#752).
    pub title: String,
    pub head: Rectangle,
    pub mood: Option<Mood>,
    pub status: String,
    pub tone: Option<Tone>,
    pub model: String,
    /// How to fix what her status says, under her header, and its box.
    pub hint: Vec<String>,
    pub hint_rect: Option<Rectangle>,
    /// Her key's field in the band, when she misses one (#752).
    pub key_band: Option<KeyBand>,
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
    /// "Ce qu'elle sait": what is drawn over the list, unclipped (its
    /// controls, its footer, the toast, a confirmation); no composer.
    overlay: Vec<Shape>,
    pub knows: bool,
    /// Her text, selectable (#757): the thread's or the queue's.
    pub texts: Texts,
    /// Her status's dot: none over "Ce qu'elle sait"'s subtitle (#758).
    pub dot: bool,
    /// Her header's « 21 mails à trier › » (#758).
    pub queue_button: Option<(Rectangle, String)>,
    /// A sub-view's bar in place of the tabs (#758): its way back, its
    /// title; no field under it.
    pub subbar: Option<(Rectangle, String)>,
    pub composer: bool,
}

/// Her key's field in the band under her header (#752): masked, with
/// "Enregistrer", and its lines under it.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyBand {
    pub field: Rectangle,
    pub save: Rectangle,
    /// What is typed, as dots (never the key), or the placeholder.
    pub shown: String,
    pub placeholder: bool,
    /// The keyboard types there.
    pub focused: bool,
    /// Its lines under it, each with its colour.
    pub lines: Vec<(String, Color, Point)>,
    /// What is over it: its title, its words, the console's button.
    shapes: Vec<Shape>,
    /// The last key given was refused: its ring orange.
    pub error: bool,
}

/// The dots a key shows as, at most this many: its length is not drawn
/// past it.
const KEY_DOTS: usize = 32;
const KEY_FIELD: f32 = 30.0;

/// The measures of text, at a size, in a weight.
fn width_of(text: &str, size: f32, bold: bool) -> f32 {
    measure(text, size, if bold { MANROPE_BOLD } else { MANROPE })
}

/// The font text is measured and drawn in.
fn font(bold: bool) -> iced::Font {
    if bold { MANROPE_BOLD } else { MANROPE }
}

/// `text` in lines as [`lines_of`] breaks them, each with its range.
fn ranged(text: &str, size: f32, bold: bool, room: f32) -> Vec<(String, std::ops::Range<usize>)> {
    select::wrap_ranged(text, room, |s| width_of(s, size, bold))
}

/// The lines alone.
fn plain(lines: &[(String, std::ops::Range<usize>)]) -> Vec<String> {
    lines.iter().map(|(l, _)| l.clone()).collect()
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
    /// Her answer to this request as it streams.
    Streaming(u64, &'a str),
    Typing(&'a str),
    /// Perso items hidden in a public moment: how many.
    Hidden(usize),
    /// Her hello while nothing is said today (#758), and what can be
    /// asked of her.
    Greeting(&'a str),
}

/// Whether perso titles are hidden now: a public moment, not revealed.
fn hide(open: &Open) -> bool {
    crate::speak::moment().public()
        && !(open.reveal && crate::speak::moment() != hocus_secretary_link::Moment::Sharing)
}

/// The cards a briefing shows under its sections: not in the thread.
fn in_briefings(c: &Conversation) -> Vec<u64> {
    c.items
        .iter()
        .filter_map(|i| match &i.body {
            Body::Briefing { briefing } => {
                Some(briefing.sections.iter().filter_map(|s| s.proposal))
            }
            _ => None,
        })
        .flatten()
        .collect()
}

fn entries<'a>(c: &'a Conversation, words: Words, hide: bool) -> Vec<Entry<'a>> {
    let mut out = Vec::new();
    let mut day = None;
    let mut hidden = 0;
    let shown_in_briefings = in_briefings(c);
    for (n, item) in c.items.iter().enumerate() {
        if hide && item.notebook == Some(Notebook::Perso) {
            hidden += 1;
            continue;
        }
        if shown_in_briefings.contains(&item.id) {
            continue;
        }
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
                out.push(Entry::Streaming(request, said));
            }
        }
    }
    if hidden > 0 {
        out.push(Entry::Hidden(hidden));
    }
    // Nothing said on her day: her hello, and examples (#758).
    if let Some(look) = c.look.as_ref().filter(|l| !l.greeting.is_empty())
        && let Ok(today) = look.today.parse::<jiff::civil::Date>()
        && c.typing.is_none()
    {
        let zone = jiff::tz::TimeZone::system();
        let said_today = c.items.iter().any(|i| {
            i.at.parse::<jiff::Timestamp>()
                .is_ok_and(|at| at.to_zoned(zone.clone()).date() == today)
        });
        if !said_today {
            let noon = today
                .at(12, 0, 0, 0)
                .to_zoned(zone)
                .map(|z| z.timestamp().to_string())
                .unwrap_or_default();
            if let Some(this) = day_words(&noon, words)
                && day.as_ref() != Some(&this)
            {
                out.push(Entry::Day(this));
            }
            out.push(Entry::Greeting(&look.greeting));
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
        Entry::Streaming(..) | Entry::Typing(_) | Entry::Greeting(_) => true,
        Entry::Day(_) | Entry::Hidden(_) => false,
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
    laid.title = c
        .look
        .as_ref()
        .map(|l| l.name.trim().to_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| words.title().to_owned());
    laid.status = match (&look, line) {
        (Some(look), _) => look.status.clone(),
        (None, Her::Ready) => words.connecting().to_owned(),
        (None, _) => words.unavailable().to_owned(),
    };
    laid.tone = look.as_ref().map(|l| l.tone);
    laid.dot = true;
    if open.tab == Tab::Knows && line == Her::Ready {
        laid.status = words.knows_subtitle().to_owned();
        laid.dot = false;
    }
    // The queue shown: what she does, unless her status says more.
    if open.queue
        && look.as_ref().is_some_and(|l| l.tone == Tone::Ok)
        && let Some(view) = c.queue.as_deref()
        && view.waiting > 0
    {
        laid.status = if words.fr {
            format!("Je trie tes mails · {} en attente", view.waiting)
        } else {
            format!("Sorting your mail · {} waiting", view.waiting)
        };
    }
    // A sub-view (#758): the queue, a briefing, the agents' line.
    let subview = if open.tab != Tab::Talk {
        None
    } else if open.queue {
        Some(words.queue_title().to_owned())
    } else if let Some(point) = open.point {
        Some(point_title(c, point, words))
    } else if open.briefing {
        Some(words.pick("Tes agents", "Your agents").to_owned())
    } else {
        None
    };
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
    // Her triage queue's button, while mails wait (#754, #758): under
    // her status, in the thread only.
    let mut tabs_top = HEAD_TOP + HEAD + HEAD_BOTTOM + TABS_TOP;
    let to_sort = look.as_ref().map_or(0, |l| l.to_sort);
    if line == Her::Ready && open.tab == Tab::Talk && subview.is_none() && to_sort > 0 {
        let label = words.to_sort(to_sort);
        let x = laid.head.x + HEAD + HEAD_GAP;
        let rect = Rectangle::new(
            Point::new(x, laid.head.center_y() + 12.0),
            Size::new(width_of(&label, QUEUE_TEXT, true) + 16.0, 19.0),
        );
        laid.targets.push((Target::Status, rect));
        laid.queue_button = Some((rect, label));
        tabs_top += 6.0;
    } else if line == Her::Ready && look.is_some() && open.tab == Tab::Talk && subview.is_none() {
        // No mail waits: her status line still opens the queue's view, as
        // before #758 (what is set aside is there).
        let x = laid.head.x + HEAD + HEAD_GAP;
        laid.targets.push((
            Target::Status,
            Rectangle::new(
                Point::new(x, laid.head.center_y() + 2.0),
                Size::new((gear.x - 8.0 - x).max(0.0), 18.0),
            ),
        ));
    }
    // How to fix what her status says: a band under her header.
    if let Some(provider) = look.as_ref().and_then(|l| l.key.clone())
        && open.tab == Tab::Talk
    {
        tabs_top = lay_out_key(&mut laid, open, c, words, &provider, tabs_top, right);
    } else if let Some(hint) = look
        .as_ref()
        .map(|l| l.hint.as_str())
        .filter(|h| !h.is_empty())
        && open.tab == Tab::Talk
    {
        let lines = lines_of(hint, HINT, false, right - PAD - 2.0 * (HINT_PAD + 3.0));
        let h = lines.len() as f32 * HINT_LINE + 2.0 * HINT_PAD;
        laid.hint_rect = Some(Rectangle::new(
            Point::new(PAD, tabs_top),
            Size::new(right - PAD, h),
        ));
        laid.hint = lines;
        tabs_top += h + 8.0;
    }
    laid.tabs = Rectangle::new(
        Point::new(PAD, tabs_top),
        Size::new(right - PAD, TAB + 2.0 * TAB_PAD),
    );
    if let Some(title) = subview.clone() {
        // Its way back and its title, in place of the tabs.
        let back = words.back();
        let rect = Rectangle::new(
            Point::new(PAD, tabs_top),
            Size::new(width_of(back, SUB_TEXT, true) + 20.0, laid.tabs.height),
        );
        laid.targets.push((Target::Back, rect));
        laid.subbar = Some((laid.tabs, title));
    } else {
        let tab_width = (laid.tabs.width - 2.0 * TAB_PAD) / 2.0;
        for (n, tab) in [Tab::Talk, Tab::Knows].into_iter().enumerate() {
            let rect = Rectangle::new(
                Point::new(PAD + TAB_PAD + n as f32 * tab_width, tabs_top + TAB_PAD),
                Size::new(tab_width, TAB),
            );
            laid.tab_rects.push((tab, rect));
            laid.targets.push((Target::Tab(tab), rect));
        }
    }
    let mut top = laid.tabs.y + laid.tabs.height + TABS_BOTTOM;
    // The agents' line, while a tool is followed.
    let said = agents::said();
    let shows = said.shows(agents::now_us());
    if open.tab == Tab::Talk
        && subview.is_none()
        && matches!(shows, agents::Shows::Lines(_) | agents::Shows::Idle(_))
    {
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
        vec![words.placeholder(&laid.title)]
    } else {
        lines_of(&open.field, FIELD_TEXT, false, field_room)
    };
    let field_lines = shown_field.len().clamp(1, FIELD_LINES);
    let field_height = FIELD.max(field_lines as f32 * FIELD_LINE + 14.0);
    let foot_height = FOOT * 1.4;
    // No field in a sub-view: nothing is said there (#758).
    laid.composer = subview.is_none() && open.tab == Tab::Talk;
    let composer_top = if laid.composer {
        size.height - COMPOSER_BOTTOM - foot_height - FOOT_GAP - field_height - COMPOSER_TOP
    } else {
        size.height - 4.0
    };
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
    if laid.composer {
        laid.targets.push((Target::Send, laid.send));
        laid.targets.push((Target::Field, laid.field));
    }
    laid.foot = laid.field.y + laid.field.height + FOOT_GAP + foot_height / 2.0;
    laid.thread = Rectangle::new(
        Point::new(0.0, top),
        Size::new(width, (composer_top - top).max(0.0)),
    );
    let mut said_items = Vec::new();
    match (open.tab, open.briefing) {
        (Tab::Talk, _) if open.queue => {
            queue::lay_out(
                &mut laid,
                open,
                c.queue.as_deref(),
                words,
                top,
                right,
                &mut said_items,
            );
            laid.texts = lines_as_texts(&laid.shapes, laid.thread);
        }
        (Tab::Talk, _) if open.point.is_some() => {
            let point = open.point.unwrap_or_default();
            lay_out_point(
                &mut laid,
                open,
                c,
                words,
                top,
                right,
                point,
                &mut said_items,
            );
        }
        (Tab::Knows, _) => {
            // No composer there: its controls and footer instead.
            laid.knows = true;
            laid.targets
                .retain(|(t, _)| !matches!(t, Target::Send | Target::Field));
            knows::lay_out(
                &mut laid,
                open,
                c.knows.as_deref(),
                c.toast.as_ref(),
                words,
                top,
                right,
                &mut said_items,
            );
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
        "title": laid.title,
        "status": laid.status,
        "hint": laid.hint.join(" "),
        "tab": match open.tab { Tab::Talk => "talk", Tab::Knows => "knows" },
        "briefing": open.briefing,
        "point": open.point,
        "queue": open.queue,
        "moment": format!("{:?}", crate::speak::moment()).to_lowercase(),
        "agents": laid.agents.as_ref().map(|(_, text, n, _)| format!("{text} | {n}")),
        "items": said_items,
        "field": open.field.chars().count(),
        // Whether a key is being typed: never what, nor its length.
        "key_field": laid.key_band.as_ref().map(|k| !k.placeholder),
        "key_lines": laid.key_band.as_ref().map(|k| k.lines.iter().map(|(l, _, _)| l.clone()).collect::<Vec<_>>().join(" ")),
        "focus": if !open.focused { "none" } else if laid.key_band.is_some() && open.key_focus { "key" } else { "field" },
        "search": open.search,
        "to_sort": laid.queue_button.as_ref().map(|(_, label)| label.clone()),
        "subview": laid.subbar.as_ref().map(|(_, title)| title.clone()),
    });
    laid
}

/// The lines of text `shapes` draws whole within `within`, each a block
/// of its own (the queue's view, #757): selectable and copied one a line.
fn lines_as_texts(shapes: &[Shape], within: Rectangle) -> Texts {
    let mut texts = Texts::default();
    let mono = iced::Font::with_name("Martian Mono");
    for shape in shapes {
        let (line, font) = match shape {
            Shape::Text(line) => (line, font(line.bold)),
            Shape::Mono(line) => (line, mono),
            _ => continue,
        };
        if line.text.trim().is_empty() {
            continue;
        }
        let height = line.size * 1.45;
        let top = line.at.y - height / 2.0;
        if top < within.y - 0.5 || top + height > within.y + within.height + 0.5 {
            continue;
        }
        let w = measure(&line.text, line.size, font);
        let x = match line.align {
            Alignment::Center => line.at.x - w / 2.0,
            Alignment::Right => line.at.x - w,
            _ => line.at.x,
        };
        let block = texts.block(Source::Queue(texts.blocks.len() as u32));
        texts.blocks[block].rect = Rectangle::new(Point::new(x, top), Size::new(w, height));
        let lines = [(line.text.clone(), 0..line.text.len())];
        texts.put(
            block, &line.text, &lines, x, top, height, line.size, font, &measure,
        );
    }
    texts
}

/// A briefing's sub-view title: « Ton point du lundi 9 novembre ».
fn point_title(c: &Conversation, point: u64, words: Words) -> String {
    let briefing = c
        .items
        .iter()
        .find(|i| i.id == point)
        .and_then(|i| match &i.body {
            Body::Briefing { briefing } => Some(briefing),
            _ => None,
        });
    match briefing {
        Some(b) if b.kind == BriefingKind::Morning && !b.date.is_empty() => {
            let mut date = b.date.chars();
            let date: String = match date.next() {
                Some(first) if words.fr => first.to_lowercase().chain(date).collect(),
                Some(first) => std::iter::once(first).chain(date).collect(),
                None => String::new(),
            };
            if words.fr {
                format!("Ton point du {date}")
            } else {
                format!("Your briefing, {date}")
            }
        }
        Some(b) => b.title.clone(),
        None => words.pick("Ton point", "Your briefing").to_owned(),
    }
}

/// Her key's band at `top` (#752, #758): what a key is, where to make one
/// (« Ouvrir la console d'Anthropic ↗ »), that it is billed apart from a
/// Claude subscription; its field, masked, with « Enregistrer », what
/// became of the last key given under it, and where it is kept. The
/// tabs' top under it.
fn lay_out_key(
    laid: &mut Laid,
    open: &Open,
    c: &Conversation,
    words: Words,
    provider: &str,
    top: f32,
    right: f32,
) -> f32 {
    let inner_x = PAD + HINT_PAD + 4.0;
    let inner = right - inner_x - HINT_PAD - 4.0;
    let name = match provider {
        "anthropic" => "Anthropic",
        other => other,
    };
    let mut shapes = Vec::new();
    let mut at = top + HINT_PAD + 3.0;
    for line in lines_of(&words.key_title(name), 13.5, true, inner) {
        shapes.push(Shape::Text(Line {
            text: line,
            at: Point::new(inner_x, at + 13.5 * 0.7),
            size: 13.5,
            color: hex(0xF5C46E),
            bold: true,
            align: Alignment::Left,
        }));
        at += 13.5 * 1.4;
    }
    at += 3.0;
    let explain = lines_of(words.key_explain(), 12.5, false, inner);
    push_lines(
        &mut shapes,
        &explain,
        inner_x,
        at,
        12.5,
        12.5 * 1.45,
        hex(0xE6EAF1),
        false,
    );
    at += explain.len() as f32 * 12.5 * 1.45 + 6.0;
    let mut targets = Vec::new();
    let console = small_button(
        &mut shapes,
        &mut targets,
        open,
        Target::Console,
        words.key_console(),
        false,
        inner_x,
        at,
    );
    at += console.height + 8.0;
    let save_label = words.pick("Enregistrer", "Save");
    let save_w = width_of(save_label, BUTTON, true) + 2.0 * BUTTON_X;
    let field = Rectangle::new(
        Point::new(inner_x, at),
        Size::new(inner - save_w - 6.0, KEY_FIELD),
    );
    let save = Rectangle::new(
        Point::new(inner_x + inner - save_w, at),
        Size::new(save_w, KEY_FIELD),
    );
    at += KEY_FIELD + 6.0;
    let (shown, placeholder) = if open.key.is_empty() {
        (words.key_placeholder().to_owned(), true)
    } else {
        ("•".repeat(open.key.chars().count().min(KEY_DOTS)), false)
    };
    let mut said: Vec<(String, Color)> = Vec::new();
    let mut error = false;
    if c.key_asked.is_some() {
        said.push((words.pick("Je la vérifie…", "Checking it…").to_owned(), DIM));
    } else if let Some((given, outcome)) = &c.key_said
        && *outcome != hocus_secretary_link::KeyOutcome::Kept
    {
        // Under the field, with what to do (#758).
        error = true;
        said.push((outcome.words(given, words.fr), ORANGE));
    }
    said.push((words.key_note().to_owned(), LABEL));
    let mut lines = Vec::new();
    for (text, colour) in said {
        for line in lines_of(&text, HINT, false, inner) {
            lines.push((line, colour, Point::new(inner_x, at + HINT_LINE / 2.0)));
            at += HINT_LINE;
        }
    }
    let bottom = at + HINT_PAD;
    laid.hint_rect = Some(Rectangle::new(
        Point::new(PAD, top),
        Size::new(right - PAD, bottom - top),
    ));
    laid.targets.extend(targets);
    laid.targets.push((Target::KeySave, save));
    laid.targets.push((Target::KeyField, field));
    laid.key_band = Some(KeyBand {
        field,
        save,
        shown,
        placeholder,
        focused: open.focused && open.key_focus,
        lines,
        shapes,
        error,
    });
    bottom + 8.0
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
    let mut texts = Texts::default();
    let mut y = THREAD_TOP;
    let list = entries(c, words, hide(open));
    let mut previous_hers = false;
    // Each entry's shapes (from this one), its top and its bottom: only
    // whole entries are drawn (the CPU renderer does not clip). Its text's
    // lines and blocks from these.
    let mut spans: Vec<(usize, f32, f32)> = Vec::new();
    let mut text_spans: Vec<(usize, usize)> = Vec::new();
    for (n, entry) in list.iter().enumerate() {
        if n > 0 {
            y += GAP;
        }
        if let Some(last) = spans.last_mut() {
            last.2 = y - GAP;
        }
        spans.push((shapes.len(), y, y));
        text_spans.push((texts.runs.len(), texts.blocks.len()));
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
                    let lines = ranged(text, BUBBLE, false, mine_room - 2.0 * BUBBLE_X);
                    let w = lines
                        .iter()
                        .map(|(l, _)| width_of(l, BUBBLE, false))
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
                    let block = texts.block(Source::Item(item.id));
                    texts.blocks[block].rect = rect;
                    texts.put(
                        block,
                        text,
                        &lines,
                        rect.x + BUBBLE_X,
                        y + BUBBLE_Y,
                        BUBBLE_LINE,
                        BUBBLE,
                        font(false),
                        &measure,
                    );
                    push_lines(
                        &mut shapes,
                        &plain(&lines),
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
                    let block;
                    (y, block) = her_bubble(
                        &mut shapes,
                        &mut texts,
                        Source::Item(item.id),
                        text,
                        note.as_deref(),
                        her_x,
                        her_room,
                        y,
                    );
                    if super::item_copy(item).is_some() {
                        texts.blocks[block].item = Some(item.id);
                    }
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
                    // Her question (#758): a blue « QUESTION » label, her
                    // words in plain text, what she found under them, the
                    // quick replies inside the card. Amber stays for what
                    // waits for the user.
                    let inner = her_room - 2.0 * CARD_X;
                    let mut at = y + CARD_TOP;
                    let mut parts: Vec<Shape> = Vec::new();
                    parts.push(Shape::Text(Line {
                        text: words.question().to_owned(),
                        at: Point::new(her_x + CARD_X, at + CARD_LABEL * 0.7),
                        size: CARD_LABEL,
                        color: hex(0x8FB3FF),
                        bold: true,
                        align: Alignment::Left,
                    }));
                    at += CARD_LABEL * 1.4 + 4.0;
                    let shown = select::unmarked(text);
                    let lines = ranged(&shown, BUBBLE, false, inner);
                    let notes = note
                        .as_deref()
                        .map(|n| ranged(n, SMALL, false, inner))
                        .unwrap_or_default();
                    let block = texts.block(Source::Item(item.id));
                    texts.put(
                        block,
                        &shown,
                        &lines,
                        her_x + CARD_X,
                        at,
                        BUBBLE_LINE,
                        BUBBLE,
                        font(false),
                        &measure,
                    );
                    push_lines(
                        &mut parts,
                        &plain(&lines),
                        her_x + CARD_X,
                        at,
                        BUBBLE,
                        BUBBLE_LINE,
                        TEXT,
                        false,
                    );
                    at += lines.len() as f32 * BUBBLE_LINE;
                    if let Some(note) = note.as_deref() {
                        at += 3.0;
                        texts.put(
                            block,
                            note,
                            &notes,
                            her_x + CARD_X,
                            at,
                            SMALL_LINE,
                            SMALL,
                            font(false),
                            &measure,
                        );
                        push_lines(
                            &mut parts,
                            &plain(&notes),
                            her_x + CARD_X,
                            at,
                            SMALL,
                            SMALL_LINE,
                            DIM,
                            false,
                        );
                        at += notes.len() as f32 * SMALL_LINE;
                    }
                    at += BUTTON_TOP;
                    // Its quick replies: pills, a click sends one and greys
                    // the others.
                    let mut x = her_x + CARD_X;
                    let pill_h = PILL * 1.4 + 2.0 * PILL_Y;
                    for (choice, reply) in replies.iter().enumerate() {
                        let w = width_of(reply, PILL, true) + 2.0 * PILL_X;
                        if x > her_x + CARD_X && x + w > her_x + her_room - CARD_X {
                            x = her_x + CARD_X;
                            at += pill_h + PILL_GAP;
                        }
                        let rect = Rectangle::new(Point::new(x, at), Size::new(w, pill_h));
                        let picked = *chosen == Some(choice as u32);
                        let used = chosen.is_some() && !picked;
                        let target = Target::Reply {
                            item: item.id,
                            choice: choice as u32,
                        };
                        let hover = open.hover == Some(target) && chosen.is_none();
                        let fade = if used { 0.35 } else { 1.0 };
                        parts.push(Shape::Box {
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
                        parts.push(Shape::Text(Line {
                            text: reply.clone(),
                            at: Point::new(x + w / 2.0, at + pill_h / 2.0),
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
                    at += pill_h + CARD_BOTTOM;
                    let rect = Rectangle::new(Point::new(her_x, y), Size::new(her_room, at - y));
                    texts.blocks[block].rect = rect;
                    shapes.push(Shape::Box {
                        rect,
                        radius: 12.0,
                        fill: Color {
                            a: 1.0,
                            ..hex(0x121827)
                        },
                        rim: Some(alpha(0x8FB3FF, 0.45)),
                        dashed: false,
                        sharp: Corner::None,
                    });
                    shapes.push(Shape::Box {
                        rect,
                        radius: 12.0,
                        fill: alpha(0x8FB3FF, 0.07),
                        rim: None,
                        dashed: false,
                        sharp: Corner::None,
                    });
                    shapes.extend(parts);
                    y = at;
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
                        &mut texts,
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
                Body::Briefing { briefing } => {
                    y = lay_out_briefing_card(
                        &mut shapes,
                        &mut targets,
                        open,
                        item.id,
                        briefing,
                        her_x,
                        her_room,
                        y,
                        words,
                    );
                    said.push(format!("briefing:{}", briefing.title));
                }
                Body::Learned {
                    text,
                    undone,
                    settled,
                } => {
                    // « Noté : … · Annuler » (#758), a bar emptying over its
                    // ten seconds; no box to tick.
                    let undo = words.undo();
                    let uw = width_of(undo, LEARNED, true);
                    let can_undo = !*undone && !*settled;
                    let inner = her_room - 20.0 - if can_undo { uw + 12.0 } else { 0.0 };
                    let lines = lines_of(text, LEARNED, false, inner);
                    let h = lines.len() as f32 * LEARNED * 1.4 + 12.0;
                    let rect = Rectangle::new(Point::new(her_x, y), Size::new(her_room, h));
                    shapes.push(Shape::Box {
                        rect,
                        radius: 8.0,
                        fill: alpha(0x8FB3FF, 0.08),
                        rim: Some(alpha(0x8FB3FF, 0.4)),
                        dashed: false,
                        sharp: Corner::None,
                    });
                    push_lines(
                        &mut shapes,
                        &lines,
                        her_x + 10.0,
                        y + 6.0,
                        LEARNED,
                        LEARNED * 1.4,
                        if *undone {
                            alpha(0xB9CEFF, 0.5)
                        } else {
                            hex(0xB9CEFF)
                        },
                        false,
                    );
                    if can_undo {
                        let at = Rectangle::new(
                            Point::new(her_x + her_room - 10.0 - uw - 2.0, y + 2.0),
                            Size::new(uw + 4.0, h - 4.0),
                        );
                        shapes.push(Shape::Text(Line {
                            text: undo.to_owned(),
                            at: Point::new(her_x + her_room - 10.0, y + 6.0 + LEARNED * 0.7),
                            size: LEARNED,
                            color: MINT,
                            bold: true,
                            align: Alignment::Right,
                        }));
                        targets.push((Target::Undo(item.id), at));
                        shapes.push(Shape::Countdown {
                            rect: Rectangle::new(
                                Point::new(her_x + 8.0, y + h - 4.0),
                                Size::new(her_room - 16.0, 2.0),
                            ),
                            item: item.id,
                            since: None,
                        });
                    }
                    said.push(format!(
                        "learned:{text}{}",
                        if *undone { " (undone)" } else { "" }
                    ));
                    y += h;
                }
                Body::Steps { steps } => {
                    y = lay_out_steps(&mut shapes, steps, her_x, her_room, y, words);
                    said.push(format!(
                        "steps:{}",
                        steps
                            .iter()
                            .map(|s| format!("{}={:?}", s.title, s.state))
                            .collect::<Vec<_>>()
                            .join(" | ")
                    ));
                }
            },
            Entry::Streaming(request, text) => {
                (y, _) = her_bubble(
                    &mut shapes,
                    &mut texts,
                    Source::Streaming(*request),
                    text,
                    None,
                    her_x,
                    her_room,
                    y,
                );
                said.push(format!("streaming:{text}"));
            }
            Entry::Greeting(greeting) => {
                (y, _) = her_bubble(
                    &mut shapes,
                    &mut texts,
                    Source::Item(0),
                    greeting,
                    None,
                    her_x,
                    her_room,
                    y,
                );
                // Each example a pill, one under the other: a click says it.
                let pill_h = PILL * 1.4 + 2.0 * PILL_Y;
                for (n, example) in words.examples().iter().enumerate() {
                    y += PILL_GAP;
                    let w = width_of(example, PILL, true) + 2.0 * PILL_X;
                    let rect = Rectangle::new(Point::new(her_x, y), Size::new(w, pill_h));
                    let target = Target::Example(n as u8);
                    let hover = open.hover == Some(target);
                    shapes.push(Shape::Box {
                        rect,
                        radius: pill_h / 2.0,
                        fill: alpha(0x7EE0A0, if hover { 0.16 } else { 0.06 }),
                        rim: Some(alpha(0x7EE0A0, 0.55)),
                        dashed: false,
                        sharp: Corner::None,
                    });
                    shapes.push(Shape::Text(Line {
                        text: (*example).to_owned(),
                        at: Point::new(her_x + w / 2.0, y + pill_h / 2.0),
                        size: PILL,
                        color: hex(0x9BEBB8),
                        bold: true,
                        align: Alignment::Center,
                    }));
                    targets.push((target, rect));
                    y += pill_h;
                }
                said.push(format!("greeting:{greeting}"));
            }
            Entry::Hidden(n) => {
                y = hidden_line(
                    &mut shapes,
                    &mut targets,
                    open,
                    *n,
                    PAD,
                    right - PAD,
                    y,
                    words,
                );
                said.push(format!("hidden:{n}"));
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
    if let Some(last) = spans.last_mut() {
        last.2 = y;
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
    let mut span = 0;
    for (n, shape) in shapes.into_iter().enumerate() {
        while spans.get(span + 1).is_some_and(|(from, _, _)| *from <= n) {
            span += 1;
        }
        let whole = spans.get(span).is_none_or(|(_, from, to)| {
            from + shift >= top - 0.5 && to + shift <= top + room + 0.5
        });
        if whole {
            laid.shapes.push(moved(shape, shift));
        }
    }
    // Her text as it shows: only whole entries' lines take the pointer.
    let whole: Vec<bool> = spans
        .iter()
        .map(|(_, from, to)| from + shift >= top - 0.5 && to + shift <= top + room + 0.5)
        .collect();
    let span_of = |n: usize, part: fn(&(usize, usize)) -> usize| {
        text_spans
            .partition_point(|s| part(s) <= n)
            .saturating_sub(1)
    };
    let mut runs = Vec::new();
    for (n, mut run) in std::mem::take(&mut texts.runs).into_iter().enumerate() {
        if whole.get(span_of(n, |s| s.0)).copied().unwrap_or(false) {
            run.middle += shift;
            runs.push(run);
        }
    }
    texts.runs = runs;
    for (n, block) in texts.blocks.iter_mut().enumerate() {
        block.shown = whole.get(span_of(n, |s| s.1)).copied().unwrap_or(false);
        block.rect.y += shift;
    }
    laid.texts = texts;
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
        Shape::Quote(r) => Shape::Quote(down(r)),
        Shape::Rule(r) => Shape::Rule(down(r)),
        Shape::Mono(mut line) => {
            line.at.y += dy;
            Shape::Mono(line)
        }
        Shape::Her(r) => Shape::Her(down(r)),
        Shape::Tick(r, done) => Shape::Tick(down(r), done),
        Shape::Lens(p) => Shape::Lens(Point::new(p.x, p.y + dy)),
        Shape::Countdown { rect, item, since } => Shape::Countdown {
            rect: down(rect),
            item,
            since,
        },
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

/// Her bubble at `y`, its text a block of `texts` for `source`: its
/// bottom, and that block.
#[allow(clippy::too_many_arguments)]
fn her_bubble(
    shapes: &mut Vec<Shape>,
    texts: &mut Texts,
    source: Source,
    text: &str,
    note: Option<&str>,
    x: f32,
    room: f32,
    y: f32,
) -> (f32, usize) {
    let inner = room - 2.0 * BUBBLE_X;
    // Her markdown's emphasis shows (and copies) as its words (#761).
    let text = &*select::unmarked(text);
    let lines = ranged(text, BUBBLE, false, inner);
    let notes = note
        .map(|n| ranged(n, SMALL, false, inner))
        .unwrap_or_default();
    let w = lines
        .iter()
        .map(|(l, _)| width_of(l, BUBBLE, false))
        .chain(notes.iter().map(|(l, _)| width_of(l, SMALL, false)))
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
    let note_top = y + BUBBLE_Y + lines.len() as f32 * BUBBLE_LINE + 3.0;
    let block = texts.block(source);
    texts.blocks[block].rect = rect;
    texts.put(
        block,
        text,
        &lines,
        x + BUBBLE_X,
        y + BUBBLE_Y,
        BUBBLE_LINE,
        BUBBLE,
        font(false),
        &measure,
    );
    if let Some(note) = note {
        texts.put(
            block,
            note,
            &notes,
            x + BUBBLE_X,
            note_top,
            SMALL_LINE,
            SMALL,
            font(false),
            &measure,
        );
    }
    push_lines(
        shapes,
        &plain(&lines),
        x + BUBBLE_X,
        y + BUBBLE_Y,
        BUBBLE,
        BUBBLE_LINE,
        TEXT,
        false,
    );
    push_lines(
        shapes,
        &plain(&notes),
        x + BUBBLE_X,
        note_top,
        SMALL,
        SMALL_LINE,
        DIM,
        false,
    );
    (y + h, block)
}

/// A card at `y`: its label, its text, its lines, its buttons (at most
/// three, the first the primary), or what came of it; its bottom.
#[allow(clippy::too_many_arguments)]
fn lay_out_card(
    shapes: &mut Vec<Shape>,
    targets: &mut Vec<(Target, Rectangle)>,
    texts: &mut Texts,
    open: &Open,
    item: u64,
    card: &Card,
    x: f32,
    room: f32,
    y: f32,
) -> f32 {
    if let Some(sheet) = &card.sheet {
        return lay_out_sheet(shapes, targets, open, item, card, sheet, x, room, y);
    }
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
    // Its text and its lines, selectable (#757); a draft's « Copier »
    // copies the draft.
    let block = texts.block(Source::Item(item));
    if !card.text.is_empty() {
        let lines = ranged(&card.text, CARD_TEXT, false, inner);
        texts.put(
            block,
            &card.text,
            &lines,
            x + CARD_X,
            at,
            CARD_LINE,
            CARD_TEXT,
            font(false),
            &measure,
        );
        push_lines(
            &mut parts,
            &plain(&lines),
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
        let lines = ranged(&shown, CARD_TEXT, false, inner - indent);
        let h = lines.len() as f32 * CARD_LINE;
        if draft {
            parts.push(Shape::Quote(Rectangle::new(
                Point::new(x + CARD_X, at + 2.0),
                Size::new(2.0, h + 4.0),
            )));
            at += 4.0;
        }
        texts.put(
            block,
            &shown,
            &lines,
            x + CARD_X + indent,
            at,
            CARD_LINE,
            CARD_TEXT,
            font(false),
            &measure,
        );
        push_lines(
            &mut parts,
            &plain(&lines),
            x + CARD_X + indent,
            at,
            CARD_TEXT,
            CARD_LINE,
            hex(0xE6EAF1),
            false,
        );
        at += h + 4.0;
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
        // A card that only says (a rule kept): no buttons.
        (None, None) if card.actions.is_empty() => {}
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
            // A draft says nothing leaves before its click (#758).
            if draft {
                at += 6.0;
                parts.push(Shape::Text(Line {
                    text: super::words().nothing_leaves().to_owned(),
                    at: Point::new(x + CARD_X, at + SMALL_LINE / 2.0),
                    size: SMALL,
                    color: FAINT,
                    bold: false,
                    align: Alignment::Left,
                }));
                at += SMALL_LINE;
            }
        }
    }
    at += CARD_BOTTOM;
    texts.blocks[block].rect = Rectangle::new(Point::new(x, y), Size::new(room, at - y));
    if draft && (!card.lines.is_empty() || !card.text.is_empty()) {
        texts.blocks[block].item = Some(item);
    }
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

/// A consent card at `y` (§10.1, #750): the sheet's title and where it
/// comes from, what she reads (✓), what she never does there (✕), what
/// leaves the machine, the notebook to choose, "Autoriser" and "Pas
/// maintenant"; once granted, what she reads and "Ne plus lire". Its
/// bottom.
#[allow(clippy::too_many_arguments)]
fn lay_out_sheet(
    shapes: &mut Vec<Shape>,
    targets: &mut Vec<(Target, Rectangle)>,
    open: &Open,
    item: u64,
    card: &Card,
    sheet: &hocus_secretary_link::sheets::Sheet,
    x: f32,
    room: f32,
    y: f32,
) -> f32 {
    use hocus_secretary_link::sheets::Mark;
    if !sheet.reads.is_empty() {
        return lay_out_account(shapes, targets, open, item, card, sheet, x, room, y);
    }
    let (rim, fill, _) = card_colours(CardKind::Consent);
    let inner = room - 2.0 * CARD_X;
    let mut parts: Vec<Shape> = Vec::new();
    let mut at = y + CARD_TOP + 2.0;
    let title = lines_of(&sheet.title, SHEET_TITLE, true, inner);
    push_lines(
        &mut parts,
        &title,
        x + CARD_X,
        at,
        SHEET_TITLE,
        SHEET_TITLE * 1.35,
        TEXT,
        true,
    );
    at += title.len() as f32 * SHEET_TITLE * 1.35;
    let from = lines_of(&sheet.from, SHEET_FROM, false, inner);
    push_lines(
        &mut parts,
        &from,
        x + CARD_X,
        at,
        SHEET_FROM,
        SHEET_FROM * 1.4,
        LABEL,
        false,
    );
    at += from.len() as f32 * SHEET_FROM * 1.4;
    let asking = card.outcome.is_none();
    let granted = card.outcome.is_some() && card.decided.is_none();
    // Granted, only what she reads stays shown; declined, nothing.
    for section in sheet
        .sections
        .iter()
        .filter(|s| asking || (granted && s.mark == Mark::Yes))
    {
        at += 8.0;
        parts.push(Shape::Text(Line {
            text: section.label.clone(),
            at: Point::new(x + CARD_X, at + CARD_LABEL * 0.7),
            size: CARD_LABEL,
            color: LABEL,
            bold: true,
            align: Alignment::Left,
        }));
        at += CARD_LABEL * 1.4 + 2.0;
        let (mark, colour) = match section.mark {
            Mark::Yes => ("✓", MINT),
            Mark::No => ("✕", hex(0xF7A77A)),
            Mark::Plain => ("•", FAINT),
        };
        for text in &section.lines {
            let lines = lines_of(text, SHEET_TEXT, false, inner - SHEET_MARK);
            parts.push(Shape::Text(Line {
                text: mark.to_owned(),
                at: Point::new(x + CARD_X, at + SHEET_LINE / 2.0),
                size: SHEET_TEXT,
                color: colour,
                bold: true,
                align: Alignment::Left,
            }));
            push_lines(
                &mut parts,
                &lines,
                x + CARD_X + SHEET_MARK,
                at,
                SHEET_TEXT,
                SHEET_LINE,
                hex(0xC9D0DC),
                false,
            );
            at += lines.len() as f32 * SHEET_LINE + 2.0;
        }
    }
    let button_h = BUTTON * 1.4 + 2.0 * BUTTON_Y;
    if asking && !sheet.calendars.is_empty() {
        // The calendars she will read (#752), each to be left out.
        at += 8.0;
        parts.push(Shape::Text(Line {
            text: hocus_secretary_link::sheets::calendars_label(super::words().fr).to_owned(),
            at: Point::new(x + CARD_X, at + CARD_LABEL * 0.7),
            size: CARD_LABEL,
            color: LABEL,
            bold: true,
            align: Alignment::Left,
        }));
        at += CARD_LABEL * 1.4 + 4.0;
        let left = open.left_out.get(&item);
        for (n, calendar) in sheet.calendars.iter().enumerate() {
            let read = left.is_none_or(|l| !l.contains(&calendar.id));
            let size = 14.0;
            let target = Target::Calendar {
                item,
                index: u32::try_from(n).unwrap_or(u32::MAX),
            };
            let note = calendar.note(super::words().fr);
            let name_lines = lines_of(&calendar.name, SHEET_TEXT, true, inner - size - 8.0);
            let note_lines = if note.is_empty() {
                Vec::new()
            } else {
                lines_of(&note, SMALL, false, inner - size - 8.0)
            };
            let h = name_lines.len() as f32 * SHEET_LINE + note_lines.len() as f32 * SMALL_LINE;
            let rect = Rectangle::new(
                Point::new(x + CARD_X - 3.0, at - 2.0),
                Size::new(inner + 6.0, h + 4.0),
            );
            if open.hover == Some(target) {
                parts.push(Shape::Box {
                    rect,
                    radius: 6.0,
                    fill: alpha(0xFFFFFF, 0.05),
                    rim: None,
                    dashed: false,
                    sharp: Corner::None,
                });
            }
            let check = Rectangle::new(
                Point::new(x + CARD_X, at + (SHEET_LINE - size) / 2.0),
                Size::new(size, size),
            );
            parts.push(Shape::Box {
                rect: check,
                radius: 4.0,
                fill: if read { GREEN } else { alpha(0xFFFFFF, 0.04) },
                rim: (!read).then_some(alpha(0xFFFFFF, 0.35)),
                dashed: false,
                sharp: Corner::None,
            });
            if read {
                parts.push(Shape::Text(Line {
                    text: "✓".to_owned(),
                    at: check.center(),
                    size: 10.5,
                    color: INK,
                    bold: true,
                    align: Alignment::Center,
                }));
            }
            push_lines(
                &mut parts,
                &name_lines,
                x + CARD_X + size + 8.0,
                at,
                SHEET_TEXT,
                SHEET_LINE,
                if read { TEXT } else { DIM },
                true,
            );
            let mut y = at + name_lines.len() as f32 * SHEET_LINE;
            push_lines(
                &mut parts,
                &note_lines,
                x + CARD_X + size + 8.0,
                y,
                SMALL,
                SMALL_LINE,
                DIM,
                false,
            );
            y += note_lines.len() as f32 * SMALL_LINE;
            targets.push((target, rect));
            at = y + 4.0;
        }
    }
    if asking {
        // « C'est privé » (#752): checked for the private notebook (the
        // account's domain suggests it), what it does under it.
        at += 10.0;
        let chosen = open.notebooks.get(&item).copied().unwrap_or(sheet.notebook);
        let on = chosen == Notebook::Perso;
        let target = Target::Private { item };
        let hover = open.hover == Some(target);
        let size = 16.0;
        let label_w = width_of(&sheet.private.label, SHEET_TEXT, true);
        let rect = Rectangle::new(
            Point::new(x + CARD_X - 3.0, at - 3.0),
            Size::new(size + 8.0 + label_w + 6.0, size + 6.0),
        );
        if hover {
            parts.push(Shape::Box {
                rect,
                radius: 6.0,
                fill: alpha(0xFFFFFF, 0.06),
                rim: None,
                dashed: false,
                sharp: Corner::None,
            });
        }
        let check = Rectangle::new(Point::new(x + CARD_X, at), Size::new(size, size));
        parts.push(Shape::Box {
            rect: check,
            radius: 4.5,
            fill: if on { GREEN } else { alpha(0xFFFFFF, 0.04) },
            rim: (!on).then_some(alpha(0xFFFFFF, 0.35)),
            dashed: false,
            sharp: Corner::None,
        });
        if on {
            parts.push(Shape::Text(Line {
                text: "✓".to_owned(),
                at: check.center(),
                size: 12.0,
                color: INK,
                bold: true,
                align: Alignment::Center,
            }));
        }
        parts.push(Shape::Text(Line {
            text: sheet.private.label.clone(),
            at: Point::new(x + CARD_X + size + 8.0, at + size / 2.0),
            size: SHEET_TEXT,
            color: TEXT,
            bold: true,
            align: Alignment::Left,
        }));
        targets.push((target, rect));
        at += size + 6.0;
        for text in &sheet.private.lines {
            let lines = lines_of(text, SMALL, false, inner - size - 8.0);
            push_lines(
                &mut parts,
                &lines,
                x + CARD_X + size + 8.0,
                at,
                SMALL,
                SMALL_LINE,
                if on { hex(0xC9D0DC) } else { DIM },
                false,
            );
            at += lines.len() as f32 * SMALL_LINE + 1.0;
        }
    } else if let Some(outcome) = &card.outcome {
        at += if granted { 10.0 } else { 6.0 };
        let shown = if granted {
            format!("✓ {outcome}")
        } else {
            outcome.clone()
        };
        let lines = lines_of(&shown, SMALL, granted, inner);
        push_lines(
            &mut parts,
            &lines,
            x + CARD_X,
            at,
            SMALL,
            SMALL_LINE,
            if granted { MINT } else { DIM },
            granted,
        );
        at += lines.len() as f32 * SMALL_LINE;
    }
    if !card.actions.is_empty() && card.decided.is_none() {
        at += BUTTON_TOP + 2.0;
        // On the left, the primary first (#758), as everywhere.
        let widths: Vec<f32> = card
            .actions
            .iter()
            .map(|a| width_of(&a.label, BUTTON, true) + 2.0 * BUTTON_X)
            .collect();
        let mut bx = x + CARD_X;
        for (n, action) in card.actions.iter().enumerate() {
            let w = widths[n];
            let rect = Rectangle::new(Point::new(bx, at), Size::new(w, button_h));
            let target = Target::Button {
                item,
                decision: action.decision,
            };
            let hover = open.hover == Some(target);
            let primary = n == 0 && asking;
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
    at += CARD_BOTTOM + 2.0;
    shapes.push(Shape::Box {
        rect: Rectangle::new(Point::new(x, y), Size::new(room, at - y)),
        radius: 14.0,
        fill,
        rim: Some(rim),
        dashed: false,
        sharp: Corner::None,
    });
    shapes.extend(parts);
    at
}

/// A switch at `rect`, on or off (#758).
fn switch(parts: &mut Vec<Shape>, rect: Rectangle, on: bool, hover: bool) {
    parts.push(Shape::Box {
        rect,
        radius: rect.height / 2.0,
        fill: match (on, hover) {
            (true, false) => GREEN,
            (true, true) => hex(0x7ED198),
            (false, false) => alpha(0xFFFFFF, 0.12),
            (false, true) => alpha(0xFFFFFF, 0.18),
        },
        rim: None,
        dashed: false,
        sharp: Corner::None,
    });
    let knob = rect.height - 4.0;
    let kx = if on {
        rect.x + rect.width - 2.0 - knob
    } else {
        rect.x + 2.0
    };
    parts.push(Shape::Box {
        rect: Rectangle::new(Point::new(kx, rect.y + 2.0), Size::new(knob, knob)),
        radius: knob / 2.0,
        fill: hex(0xF4F6FA),
        rim: None,
        dashed: false,
        sharp: Corner::None,
    });
}

/// A small button at (`x`, `y`): primary green, or grey; its rect.
#[allow(clippy::too_many_arguments)]
fn small_button(
    parts: &mut Vec<Shape>,
    targets: &mut Vec<(Target, Rectangle)>,
    open: &Open,
    target: Target,
    label: &str,
    primary: bool,
    x: f32,
    y: f32,
) -> Rectangle {
    let h = BUTTON * 1.4 + 2.0 * BUTTON_Y;
    let w = width_of(label, BUTTON, true) + 2.0 * BUTTON_X;
    let rect = Rectangle::new(Point::new(x, y), Size::new(w, h));
    let hover = open.hover == Some(target);
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
        text: label.to_owned(),
        at: rect.center(),
        size: BUTTON,
        color: if primary { INK } else { hex(0xD5DBE5) },
        bold: true,
        align: Alignment::Center,
    }));
    targets.push((target, rect));
    rect
}

/// An account's card (#758): its account, a switch per kind, her
/// promise, « Ce compte est Travail | Privé » and what « Privé » changes,
/// what she reads exactly folded under a link, « Autoriser l'agenda et
/// les mails » as the switches say, « Pas maintenant »; once answered,
/// what came of it and « Ne plus lire ». The account she asks about next
/// dashed under it. Its bottom.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn lay_out_account(
    shapes: &mut Vec<Shape>,
    targets: &mut Vec<(Target, Rectangle)>,
    open: &Open,
    item: u64,
    card: &Card,
    sheet: &hocus_secretary_link::sheets::Sheet,
    x: f32,
    room: f32,
    y: f32,
) -> f32 {
    use hocus_secretary_link::{consent::SourceKind, sheets::Mark};
    let words = super::words();
    let inner = room - 2.0 * CARD_X;
    let left = x + CARD_X;
    let mut parts: Vec<Shape> = Vec::new();
    let mut at = y + CARD_TOP;
    parts.push(Shape::Text(Line {
        text: card.label.to_uppercase(),
        at: Point::new(left, at + CARD_LABEL * 0.7),
        size: CARD_LABEL,
        color: LABEL,
        bold: true,
        align: Alignment::Left,
    }));
    at += CARD_LABEL * 1.4 + 6.0;
    // The account: its mark, its address, where it is.
    let mark = |parts: &mut Vec<Shape>, at: f32, faint: bool| {
        parts.push(Shape::Box {
            rect: Rectangle::new(Point::new(left, at), Size::new(26.0, 26.0)),
            radius: 7.0,
            fill: alpha(0x4F7BFF, if faint { 0.4 } else { 0.9 }),
            rim: None,
            dashed: false,
            sharp: Corner::None,
        });
        parts.push(Shape::Text(Line {
            text: "G".to_owned(),
            at: Point::new(left + 13.0, at + 13.0),
            size: 13.0,
            color: if faint { DIM } else { hex(0xFFFFFF) },
            bold: true,
            align: Alignment::Center,
        }));
    };
    mark(&mut parts, at + 2.0, false);
    parts.push(Shape::Text(Line {
        text: sheet.title.clone(),
        at: Point::new(left + 36.0, at + 9.0),
        size: SHEET_TITLE,
        color: TEXT,
        bold: true,
        align: Alignment::Left,
    }));
    parts.push(Shape::Text(Line {
        text: sheet.from.clone(),
        at: Point::new(left + 36.0, at + 25.0),
        size: SHEET_FROM,
        color: LABEL,
        bold: false,
        align: Alignment::Left,
    }));
    at += 36.0;
    let asking = card.outcome.is_none();
    let off = open.off.get(&item);
    let on_of =
        |kind: SourceKind, by_default: bool| by_default != off.is_some_and(|o| o.contains(&kind));
    let rule = |parts: &mut Vec<Shape>, at: f32| {
        parts.push(Shape::Box {
            rect: Rectangle::new(Point::new(left, at), Size::new(inner, 1.0)),
            radius: 0.0,
            fill: alpha(0xFFFFFF, 0.07),
            rim: None,
            dashed: false,
            sharp: Corner::None,
        });
    };
    if asking {
        parts.push(Shape::Text(Line {
            text: sheet.ask.clone(),
            at: Point::new(left, at + 12.5 * 0.7),
            size: 12.5,
            color: TEXT,
            bold: true,
            align: Alignment::Left,
        }));
        at += 12.5 * 1.4 + 4.0;
        for read in &sheet.reads {
            rule(&mut parts, at);
            at += 7.0;
            let on = on_of(read.kind, read.on);
            let sw = Rectangle::new(
                Point::new(left + inner - 36.0, at + 4.0),
                Size::new(36.0, 20.0),
            );
            let target = Target::Switch {
                item,
                kind: read.kind,
            };
            switch(&mut parts, sw, on, open.hover == Some(target));
            targets.push((
                target,
                Rectangle::new(Point::new(left, at - 3.0), Size::new(inner, 36.0)),
            ));
            let text_room = inner - 48.0;
            parts.push(Shape::Text(Line {
                text: read.title.clone(),
                at: Point::new(left, at + 13.0 * 0.7),
                size: 13.0,
                color: if on { TEXT } else { DIM },
                bold: true,
                align: Alignment::Left,
            }));
            at += 13.0 * 1.4;
            let choose = read.kind == SourceKind::Calendar && !sheet.calendars.is_empty();
            let mut line = read.line.clone();
            if choose {
                let n = sheet.calendars.len();
                line = if words.fr {
                    format!("{line} · {n} agendas, choisir ›")
                } else {
                    format!("{line} · {n} calendars, choose ›")
                };
            }
            let lines = lines_of(&line, SMALL, false, text_room);
            push_lines(&mut parts, &lines, left, at, SMALL, SMALL_LINE, DIM, false);
            if choose {
                let target = Target::Calendars(item);
                targets.push((
                    target,
                    Rectangle::new(
                        Point::new(left, at + (lines.len() as f32 - 1.0) * SMALL_LINE),
                        Size::new(text_room, SMALL_LINE),
                    ),
                ));
            }
            at += lines.len() as f32 * SMALL_LINE + 6.0;
            // Its calendars, each to be left out, once unfolded.
            if choose && open.unfolded_cards.contains(&(item, true)) {
                let left_out = open.left_out.get(&item);
                for (n, calendar) in sheet.calendars.iter().enumerate() {
                    let read = left_out.is_none_or(|l| !l.contains(&calendar.id));
                    let size = 14.0;
                    let target = Target::Calendar {
                        item,
                        index: u32::try_from(n).unwrap_or(u32::MAX),
                    };
                    let note = calendar.note(words.fr);
                    let shown = if note.is_empty() {
                        calendar.name.clone()
                    } else {
                        format!("{} · {note}", calendar.name)
                    };
                    let lines = lines_of(&shown, SMALL, false, inner - size - 18.0);
                    let check =
                        Rectangle::new(Point::new(left + 8.0, at + 1.0), Size::new(size, size));
                    parts.push(Shape::Box {
                        rect: check,
                        radius: 4.0,
                        fill: if read { GREEN } else { alpha(0xFFFFFF, 0.04) },
                        rim: (!read).then_some(alpha(0xFFFFFF, 0.35)),
                        dashed: false,
                        sharp: Corner::None,
                    });
                    if read {
                        parts.push(Shape::Text(Line {
                            text: "✓".to_owned(),
                            at: check.center(),
                            size: 10.5,
                            color: INK,
                            bold: true,
                            align: Alignment::Center,
                        }));
                    }
                    push_lines(
                        &mut parts,
                        &lines,
                        left + 8.0 + size + 8.0,
                        at,
                        SMALL,
                        SMALL_LINE,
                        if read { hex(0xC9D0DC) } else { DIM },
                        false,
                    );
                    let h = (lines.len() as f32 * SMALL_LINE).max(size + 2.0);
                    targets.push((
                        target,
                        Rectangle::new(Point::new(left, at - 1.0), Size::new(inner, h + 2.0)),
                    ));
                    at += h + 3.0;
                }
                at += 4.0;
            }
        }
        // Her promise.
        at += 2.0;
        let promise = lines_of(&sheet.promise, SMALL, false, inner - 34.0);
        let ph = promise.len() as f32 * SMALL_LINE + 14.0;
        parts.push(Shape::Box {
            rect: Rectangle::new(Point::new(left, at), Size::new(inner, ph)),
            radius: 8.0,
            fill: alpha(0x7EE0A0, 0.05),
            rim: Some(alpha(0x7EE0A0, 0.25)),
            dashed: false,
            sharp: Corner::None,
        });
        parts.push(Shape::Text(Line {
            text: "✓".to_owned(),
            at: Point::new(left + 12.0, at + 7.0 + SMALL_LINE / 2.0),
            size: SMALL,
            color: MINT,
            bold: true,
            align: Alignment::Left,
        }));
        push_lines(
            &mut parts,
            &promise,
            left + 28.0,
            at + 7.0,
            SMALL,
            SMALL_LINE,
            hex(0xC9D0DC),
            false,
        );
        at += ph + 10.0;
        // « Ce compte est Travail | Privé ».
        let chosen = open.notebooks.get(&item).copied().unwrap_or(sheet.notebook);
        let label = words.pick("Ce compte est", "This account is");
        let lw = width_of(label, 12.5, true);
        parts.push(Shape::Text(Line {
            text: label.to_owned(),
            at: Point::new(left, at + 12.0),
            size: 12.5,
            color: TEXT,
            bold: true,
            align: Alignment::Left,
        }));
        let mut sx = left + lw + 10.0;
        let seg_h = 24.0;
        let widths: Vec<f32> = sheet
            .private
            .choice
            .iter()
            .map(|c| width_of(c, 12.0, true) + 22.0)
            .collect();
        let total: f32 = widths.iter().sum::<f32>() + 4.0;
        parts.push(Shape::Box {
            rect: Rectangle::new(Point::new(sx, at), Size::new(total, seg_h)),
            radius: 8.0,
            fill: alpha(0xFFFFFF, 0.04),
            rim: Some(alpha(0xFFFFFF, 0.1)),
            dashed: false,
            sharp: Corner::None,
        });
        sx += 2.0;
        for (n, (word, w)) in sheet.private.choice.iter().zip(&widths).enumerate() {
            let perso = n == 1;
            let selected = (chosen == Notebook::Perso) == perso;
            let rect = Rectangle::new(Point::new(sx, at + 2.0), Size::new(*w, seg_h - 4.0));
            if selected {
                parts.push(Shape::Box {
                    rect,
                    radius: 6.0,
                    fill: if perso {
                        alpha(0xF28FB1, 0.22)
                    } else {
                        alpha(0xFFFFFF, 0.13)
                    },
                    rim: None,
                    dashed: false,
                    sharp: Corner::None,
                });
            }
            parts.push(Shape::Text(Line {
                text: word.clone(),
                at: rect.center(),
                size: 12.0,
                color: match (selected, perso) {
                    (true, true) => hex(0xFBD3E1),
                    (true, false) => TEXT,
                    _ => LABEL,
                },
                bold: true,
                align: Alignment::Center,
            }));
            targets.push((Target::Book { item, perso }, rect));
            sx += w;
        }
        at += seg_h + 8.0;
        let short = lines_of(&sheet.private.short, SMALL, false, inner);
        push_lines(&mut parts, &short, left, at, SMALL, SMALL_LINE, DIM, false);
        at += short.len() as f32 * SMALL_LINE + 8.0;
        // What she reads exactly, and what leaves: folded.
        let unfolded = open.unfolded_cards.contains(&(item, false));
        let more = lines_of(&sheet.more, SMALL, true, inner);
        let more_h = more.len() as f32 * SMALL_LINE;
        push_lines(
            &mut parts,
            &more,
            left,
            at,
            SMALL,
            SMALL_LINE,
            if open.hover == Some(Target::Details(item)) {
                hex(0x9BEBB8)
            } else {
                MINT
            },
            true,
        );
        targets.push((
            Target::Details(item),
            Rectangle::new(
                Point::new(left - 2.0, at - 2.0),
                Size::new(inner + 4.0, more_h + 4.0),
            ),
        ));
        at += more_h + 4.0;
        if unfolded {
            for section in &sheet.sections {
                at += 6.0;
                parts.push(Shape::Text(Line {
                    text: section.label.clone(),
                    at: Point::new(left, at + CARD_LABEL * 0.7),
                    size: CARD_LABEL,
                    color: LABEL,
                    bold: true,
                    align: Alignment::Left,
                }));
                at += CARD_LABEL * 1.4 + 2.0;
                let (sign, colour) = match section.mark {
                    Mark::Yes => ("✓", MINT),
                    Mark::No => ("✕", hex(0xF7A77A)),
                    Mark::Plain => ("•", FAINT),
                };
                for text in &section.lines {
                    let lines = lines_of(text, SMALL, false, inner - SHEET_MARK);
                    parts.push(Shape::Text(Line {
                        text: sign.to_owned(),
                        at: Point::new(left, at + SMALL_LINE / 2.0),
                        size: SMALL,
                        color: colour,
                        bold: true,
                        align: Alignment::Left,
                    }));
                    push_lines(
                        &mut parts,
                        &lines,
                        left + SHEET_MARK,
                        at,
                        SMALL,
                        SMALL_LINE,
                        hex(0xC9D0DC),
                        false,
                    );
                    at += lines.len() as f32 * SMALL_LINE + 2.0;
                }
            }
        }
        // Its buttons, on the left, the primary first: it says what it
        // allows, as the switches are.
        at += BUTTON_TOP;
        let on: Vec<SourceKind> = sheet
            .reads
            .iter()
            .filter(|r| on_of(r.kind, r.on))
            .map(|r| r.kind)
            .collect();
        let mut bx = left;
        let button_h = BUTTON * 1.4 + 2.0 * BUTTON_Y;
        for (n, action) in card.actions.iter().enumerate() {
            let label = if n == 0 && action.decision == hocus_secretary_link::Decision::Accept {
                hocus_secretary_link::sheets::allow_label(&on, words.fr)
            } else {
                action.label.clone()
            };
            let w = width_of(&label, BUTTON, true) + 2.0 * BUTTON_X;
            if bx > left && bx + w > left + inner {
                bx = left;
                at += button_h + 6.0;
            }
            let target = Target::Button {
                item,
                decision: action.decision,
            };
            let rect = small_button(&mut parts, targets, open, target, &label, n == 0, bx, at);
            bx += rect.width + 6.0;
        }
        at += button_h;
    } else if let Some(outcome) = &card.outcome {
        let granted = card.decided.is_none();
        at += 4.0;
        let shown = if granted {
            format!("✓ {outcome}")
        } else {
            outcome.clone()
        };
        let lines = lines_of(&shown, SMALL, granted, inner);
        push_lines(
            &mut parts,
            &lines,
            left,
            at,
            SMALL,
            SMALL_LINE,
            if granted { MINT } else { DIM },
            granted,
        );
        at += lines.len() as f32 * SMALL_LINE;
        if granted {
            at += BUTTON_TOP;
            let mut bx = left;
            for action in &card.actions {
                let target = Target::Button {
                    item,
                    decision: action.decision,
                };
                let rect = small_button(
                    &mut parts,
                    targets,
                    open,
                    target,
                    &action.label,
                    false,
                    bx,
                    at,
                );
                bx += rect.width + 6.0;
            }
            if !card.actions.is_empty() {
                at += BUTTON * 1.4 + 2.0 * BUTTON_Y;
            }
        }
    }
    at += CARD_BOTTOM + 2.0;
    let (rim, fill, _) = card_colours(CardKind::Consent);
    shapes.push(Shape::Box {
        rect: Rectangle::new(Point::new(x, y), Size::new(room, at - y)),
        radius: 14.0,
        fill,
        rim: Some(rim),
        dashed: false,
        sharp: Corner::None,
    });
    shapes.extend(parts);
    // The account asked about next, dashed.
    if asking && let Some(then) = &sheet.then {
        let top = at + GAP;
        let h = 50.0;
        shapes.push(Shape::Box {
            rect: Rectangle::new(Point::new(x, top), Size::new(room, h)),
            radius: 14.0,
            fill: alpha(0xFFFFFF, 0.015),
            rim: Some(alpha(0xFFFFFF, 0.14)),
            dashed: true,
            sharp: Corner::None,
        });
        let mut next = Vec::new();
        mark(&mut next, top + 12.0, true);
        next.push(Shape::Text(Line {
            text: then.identity.clone(),
            at: Point::new(left + 36.0, top + 18.0),
            size: 13.5,
            color: DIM,
            bold: true,
            align: Alignment::Left,
        }));
        next.push(Shape::Text(Line {
            text: then.note.clone(),
            at: Point::new(left + 36.0, top + 34.0),
            size: SHEET_FROM,
            color: FAINT,
            bold: false,
            align: Alignment::Left,
        }));
        shapes.extend(next);
        return top + h;
    }
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
    // Its way back is the sub-view's bar (#758).
    let mut y = top + THREAD_TOP;
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

/// "N choses perso masquées · Montrer" at `y`, from `x`, `room` wide: its
/// bottom. "Montrer" never while the screen is shared.
#[allow(clippy::too_many_arguments)]
fn hidden_line(
    shapes: &mut Vec<Shape>,
    targets: &mut Vec<(Target, Rectangle)>,
    open: &Open,
    n: usize,
    x: f32,
    room: f32,
    y: f32,
    words: Words,
) -> f32 {
    let h = SMALL * 1.4 + 10.0;
    shapes.push(Shape::Box {
        rect: Rectangle::new(Point::new(x, y), Size::new(room, h)),
        radius: 8.0,
        fill: alpha(0xF29DB8, 0.06),
        rim: Some(alpha(0xF29DB8, 0.35)),
        dashed: true,
        sharp: Corner::None,
    });
    shapes.push(Shape::Text(Line {
        text: words.hidden(n),
        at: Point::new(x + 10.0, y + h / 2.0),
        size: SMALL,
        color: hex(0xF4B6C9),
        bold: false,
        align: Alignment::Left,
    }));
    if crate::speak::moment() != hocus_secretary_link::Moment::Sharing {
        let show = words.show();
        let w = width_of(show, SMALL, true);
        shapes.push(Shape::Text(Line {
            text: show.to_owned(),
            at: Point::new(x + room - 10.0, y + h / 2.0),
            size: SMALL,
            color: if open.hover == Some(Target::Reveal) {
                hex(0x9BEBB8)
            } else {
                MINT
            },
            bold: true,
            align: Alignment::Right,
        }));
        targets.push((
            Target::Reveal,
            Rectangle::new(
                Point::new(x + room - 10.0 - w - 4.0, y + 2.0),
                Size::new(w + 8.0, h - 4.0),
            ),
        ));
    }
    y + h
}

/// A briefing in the thread: its title, her sentence, the first lines of
/// its first part (perso hidden in a public moment), and "Ouvrir le
/// point"; its bottom.
#[allow(clippy::too_many_arguments)]
fn lay_out_briefing_card(
    shapes: &mut Vec<Shape>,
    targets: &mut Vec<(Target, Rectangle)>,
    open: &Open,
    item: u64,
    briefing: &Briefing,
    x: f32,
    room: f32,
    y: f32,
    words: Words,
) -> f32 {
    let inner = room - 2.0 * CARD_X;
    let mut parts = Vec::new();
    let mut at = y + CARD_TOP;
    let absence = briefing.kind == BriefingKind::Absence;
    let label = briefing.title.to_uppercase();
    for line in lines_of(&label, CARD_LABEL, true, inner) {
        parts.push(Shape::Text(Line {
            text: line,
            at: Point::new(x + CARD_X, at + CARD_LABEL * 0.7),
            size: CARD_LABEL,
            color: if absence { hex(0xB9CEFF) } else { MINT },
            bold: true,
            align: Alignment::Left,
        }));
        at += CARD_LABEL * 1.4;
    }
    at += 4.0;
    // What is in it, in a line (#758); her sentence before there was one.
    let shown = if briefing.summary.is_empty() {
        &briefing.sentence
    } else {
        &briefing.summary
    };
    if !shown.is_empty() {
        let lines = lines_of(shown, CARD_TEXT, false, inner);
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
        at += lines.len() as f32 * CARD_LINE + 2.0;
    }
    // "Pendant ton absence" says what came, there.
    if absence && let Some(section) = briefing.sections.first() {
        let hide = hide(open);
        let mut hidden = 0;
        for line in section.lines.iter().take(5) {
            if hide && line.notebook == Notebook::Perso {
                hidden += 1;
                continue;
            }
            let shown = format!("· {}", line.title);
            let lines = lines_of(&shown, CARD_TEXT, false, inner);
            push_lines(
                &mut parts,
                &lines,
                x + CARD_X,
                at,
                CARD_TEXT,
                CARD_LINE,
                hex(0xE6EAF1),
                false,
            );
            at += lines.len() as f32 * CARD_LINE;
        }
        if hidden > 0 {
            parts.push(Shape::Text(Line {
                text: words.hidden(hidden),
                at: Point::new(x + CARD_X, at + SMALL_LINE / 2.0),
                size: SMALL,
                color: hex(0xF4B6C9),
                bold: false,
                align: Alignment::Left,
            }));
            at += SMALL_LINE;
        }
    }
    at += BUTTON_TOP;
    let button_h = BUTTON * 1.4 + 2.0 * BUTTON_Y;
    let label = words.open_point();
    let w = width_of(label, BUTTON, true) + 2.0 * BUTTON_X;
    let rect = Rectangle::new(Point::new(x + CARD_X, at), Size::new(w, button_h));
    let target = Target::Point(item);
    let hover = open.hover == Some(target);
    parts.push(Shape::Box {
        rect,
        radius: 8.0,
        fill: if hover { hex(0x7ED198) } else { GREEN },
        rim: None,
        dashed: false,
        sharp: Corner::None,
    });
    parts.push(Shape::Text(Line {
        text: label.to_owned(),
        at: Point::new(rect.center_x(), rect.center_y()),
        size: BUTTON,
        color: INK,
        bold: true,
        align: Alignment::Center,
    }));
    targets.push((target, rect));
    at += button_h + CARD_BOTTOM;
    shapes.push(Shape::Box {
        rect: Rectangle::new(Point::new(x, y), Size::new(room, at - y)),
        radius: 12.0,
        fill: Color {
            a: 1.0,
            ..hex(0x121827)
        },
        rim: Some(if absence {
            alpha(0x8FB3FF, 0.45)
        } else {
            alpha(0x7EE0A0, 0.4)
        }),
        dashed: false,
        sharp: Corner::None,
    });
    shapes.push(Shape::Box {
        rect: Rectangle::new(Point::new(x, y), Size::new(room, at - y)),
        radius: 12.0,
        fill: if absence {
            alpha(0x8FB3FF, 0.08)
        } else {
            alpha(0x7EE0A0, 0.05)
        },
        rim: None,
        dashed: false,
        sharp: Corner::None,
    });
    shapes.extend(parts);
    at
}

/// A briefing's part as the drawer shows it: its key, label, note,
/// lines, proposal.
type Part = (
    String,
    String,
    String,
    Vec<hocus_secretary_link::Line>,
    Option<u64>,
);

/// A section's label colour.
fn section_colour(key: &str) -> Color {
    match key {
        "today" | "answer" | "reminders" => hex(0xF5C46E),
        "leaves" => hex(0xF7A77A),
        "away" => hex(0xB9CEFF),
        "perso" => hex(0xF4B6C9),
        "plan" | "yesterday" => MINT,
        _ => LABEL,
    }
}

/// Her briefing `point` unfolded in the drawer (§6.1): her head, "Bonjour
/// Alex", the date and her sentence; each part with its lines (perso
/// hidden in a public moment, only counted), the proposal under EN
/// CONGÉS, the agents' night (the shell knows them), what was read. It
/// scrolls under the wheel from its top.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn lay_out_point(
    laid: &mut Laid,
    open: &Open,
    c: &Conversation,
    words: Words,
    top: f32,
    right: f32,
    point: u64,
    said: &mut Vec<String>,
) {
    let mut shapes: Vec<Shape> = Vec::new();
    let mut targets: Vec<(Target, Rectangle)> = Vec::new();
    let mut y = THREAD_TOP;
    let briefing = c
        .items
        .iter()
        .find(|i| i.id == point)
        .and_then(|i| match &i.body {
            Body::Briefing { briefing } => Some(briefing),
            _ => None,
        });
    let Some(briefing) = briefing else {
        laid.content = y;
        laid.shapes
            .extend(shapes.into_iter().map(|s| moved(s, top)));
        for (t, r) in targets {
            laid.targets
                .push((t, Rectangle::new(Point::new(r.x, r.y + top), r.size())));
        }
        return;
    };
    said.push(format!("point:{}", briefing.title));
    let room = right - PAD;
    let hide = hide(open);
    // Her head, her greeting.
    let head = 96.0;
    shapes.push(Shape::Her(Rectangle::new(
        Point::new(PAD - 6.0, y),
        Size::new(head, head),
    )));
    let tx = PAD + head + 4.0;
    let troom = right - tx;
    let mut ty = y + 14.0;
    let hello = if briefing.hello.is_empty() {
        briefing.title.clone()
    } else {
        briefing.hello.clone()
    };
    for line in lines_of(&hello, 17.0, true, troom) {
        shapes.push(Shape::Text(Line {
            text: line,
            at: Point::new(tx, ty + 17.0 * 0.7),
            size: 17.0,
            color: TEXT,
            bold: true,
            align: Alignment::Left,
        }));
        ty += 17.0 * 1.4;
    }
    // The date, and what the day is like with its free slot named
    // (#758): « une journée chargée ; ton seul créneau libre : 10 h – 11 h ».
    let busy = match (briefing.busy.is_empty(), briefing.slot.is_empty()) {
        (true, _) => String::new(),
        (false, true) => briefing.busy.clone(),
        (false, false) => format!("{} :", briefing.busy),
    };
    for (text, size, colour) in [(&briefing.date, 12.0, LABEL), (&busy, 12.0, DIM)] {
        if text.is_empty() {
            continue;
        }
        for line in lines_of(text, size, false, troom) {
            shapes.push(Shape::Text(Line {
                text: line,
                at: Point::new(tx, ty + size * 0.72),
                size,
                color: colour,
                bold: false,
                align: Alignment::Left,
            }));
            ty += size * 1.45;
        }
    }
    if !briefing.slot.is_empty() {
        let w = measure(&briefing.slot, 11.5, iced::Font::with_name("Martian Mono")) + 10.0;
        shapes.push(Shape::Box {
            rect: Rectangle::new(Point::new(tx, ty), Size::new(w, 18.0)),
            radius: 5.0,
            fill: alpha(0x7EE0A0, 0.12),
            rim: None,
            dashed: false,
            sharp: Corner::None,
        });
        shapes.push(Shape::Mono(Line {
            text: briefing.slot.clone(),
            at: Point::new(tx + 5.0, ty + 9.0),
            size: 11.5,
            color: hex(0x9BEBB8),
            bold: false,
            align: Alignment::Left,
        }));
        ty += 22.0;
    }
    for (text, size, colour) in [(&briefing.sentence, 13.0, DIM)] {
        if text.is_empty() {
            continue;
        }
        for line in lines_of(text, size, false, troom) {
            shapes.push(Shape::Text(Line {
                text: line,
                at: Point::new(tx, ty + size * 0.72),
                size,
                color: colour,
                bold: false,
                align: Alignment::Left,
            }));
            ty += size * 1.45;
        }
    }
    y = (y + head).max(ty) + 8.0;
    let mut sections: Vec<Part> = briefing
        .sections
        .iter()
        .map(|s| {
            (
                s.key.clone(),
                s.label.clone(),
                s.note.clone(),
                s.lines.clone(),
                s.proposal,
            )
        })
        .collect();
    // The agents' night, from the agents the shell follows: the most
    // urgent first (4.6's words).
    if briefing.kind == BriefingKind::Morning {
        let brief = Brief::of(&agents::said().agents);
        let agent_words = agents::Words::new(if words.fr {
            crate::clock::time::Lang::French
        } else {
            crate::clock::time::Lang::English
        });
        let mut lines = Vec::new();
        for (entries, word) in [
            (&brief.waiting, words.pick("t’attend", "is waiting")),
            (&brief.stuck, words.pick("bloqué", "stuck")),
            (&brief.to_see, words.pick("a fini", "finished")),
        ] {
            for e in entries.iter() {
                lines.push(hocus_secretary_link::Line {
                    title: format!("{} {word}", e.name),
                    detail: e
                        .note
                        .as_ref()
                        .map(|n| agent_words.note(n))
                        .unwrap_or_default(),
                    attention: true,
                    notebook: Notebook::Pro,
                    ..Default::default()
                });
            }
        }
        if !brief.working.is_empty() || !brief.resting.is_empty() {
            lines.push(hocus_secretary_link::Line {
                title: if words.fr {
                    format!(
                        "{} au travail · {} au repos",
                        brief.working.len(),
                        brief.resting.len()
                    )
                } else {
                    format!(
                        "{} at work · {} resting",
                        brief.working.len(),
                        brief.resting.len()
                    )
                },
                notebook: Notebook::Pro,
                ..Default::default()
            });
        }
        if !lines.is_empty() {
            let at = sections
                .iter()
                .position(|s| s.0 == "perso")
                .unwrap_or(sections.len());
            sections.insert(
                at,
                (
                    "agents".to_owned(),
                    words.night().to_owned(),
                    String::new(),
                    lines,
                    None,
                ),
            );
        }
    }
    // The mails to answer, in order: what their actions name (#758).
    let mails: Vec<&str> = briefing
        .sections
        .iter()
        .flat_map(|s| s.lines.iter())
        .filter(|l| !l.pointer.is_empty())
        .map(|l| l.pointer.as_str())
        .collect();
    for (key, label, note, lines, proposal) in &sections {
        let visible: Vec<&hocus_secretary_link::Line> = lines
            .iter()
            .filter(|l| !(hide && l.notebook == Notebook::Perso))
            .collect();
        let hidden = lines.len() - visible.len();
        if visible.is_empty() && hidden == 0 && proposal.is_none() {
            continue;
        }
        // Its label, spaced, and its note after it.
        y += 6.0;
        shapes.push(Shape::Text(Line {
            text: label.clone(),
            at: Point::new(PAD, y + 11.0 * 0.7),
            size: 11.0,
            color: section_colour(key),
            bold: true,
            align: Alignment::Left,
        }));
        if !note.is_empty() {
            let lw = width_of(label, 11.0, true);
            shapes.push(Shape::Text(Line {
                text: note.clone(),
                at: Point::new(PAD + lw + 8.0, y + 11.0 * 0.7),
                size: 11.0,
                color: FAINT,
                bold: false,
                align: Alignment::Left,
            }));
        }
        y += 11.0 * 1.4 + 6.0;
        let mut titles = Vec::new();
        for line in visible {
            let mut x = PAD;
            let line_top = y;
            if !line.time.is_empty() {
                shapes.push(Shape::Mono(Line {
                    text: line.time.clone(),
                    at: Point::new(PAD, y + 13.0 * 0.72),
                    size: 11.5,
                    color: if line.attention { hex(0xF5C46E) } else { DIM },
                    bold: false,
                    align: Alignment::Left,
                }));
                x = PAD + 62.0;
            } else if let Some(done) = line.done {
                shapes.push(Shape::Tick(
                    Rectangle::new(Point::new(PAD, y + 2.5), Size::new(14.0, 14.0)),
                    done,
                ));
                x = PAD + 22.0;
            }
            let when_w = if line.when.is_empty() {
                0.0
            } else {
                width_of(&line.when, 11.5, false) + 8.0
            };
            let title_room = right - x - when_w;
            // The rest, in three lines, is prose: not bold.
            let bold = key != "rest";
            for (n, t) in lines_of(&line.title, 13.0, bold, title_room)
                .into_iter()
                .enumerate()
            {
                shapes.push(Shape::Text(Line {
                    text: t,
                    at: Point::new(x, y + 13.0 * 0.72),
                    size: 13.0,
                    color: if line.attention {
                        hex(0xFFE2A8)
                    } else if bold {
                        TEXT
                    } else {
                        hex(0xD5DBE5)
                    },
                    bold,
                    align: Alignment::Left,
                }));
                if n == 0 && !line.when.is_empty() {
                    shapes.push(Shape::Text(Line {
                        text: line.when.clone(),
                        at: Point::new(right, y + 13.0 * 0.72),
                        size: 11.5,
                        color: LABEL,
                        bold: false,
                        align: Alignment::Right,
                    }));
                }
                y += 13.0 * 1.4;
            }
            if !line.detail.is_empty() {
                for d in lines_of(&line.detail, 12.0, false, right - x) {
                    shapes.push(Shape::Text(Line {
                        text: d,
                        at: Point::new(x, y + 12.0 * 0.72),
                        size: 12.0,
                        color: DIM,
                        bold: false,
                        align: Alignment::Left,
                    }));
                    y += 12.0 * 1.4;
                }
            }
            // A mail to answer: « Ouvrir le mail ↗ » (never marking it
            // read), « Rappelle-le-moi », or when she will (#758).
            if let Some(index) = mails.iter().position(|p| *p == line.pointer) {
                let index = index as u32;
                let mut ax = x;
                let mid = y + 12.0 * 0.72;
                let mut action = |label: &str, target: Option<Target>, colour: Color| {
                    let w = width_of(label, 12.0, true);
                    shapes.push(Shape::Text(Line {
                        text: label.to_owned(),
                        at: Point::new(ax, mid),
                        size: 12.0,
                        color: if target.is_some_and(|t| open.hover == Some(t)) {
                            hex(0x9BEBB8)
                        } else {
                            colour
                        },
                        bold: target.is_some(),
                        align: Alignment::Left,
                    }));
                    if let Some(target) = target {
                        targets.push((
                            target,
                            Rectangle::new(
                                Point::new(ax - 3.0, y - 2.0),
                                Size::new(w + 6.0, 12.0 * 1.4 + 4.0),
                            ),
                        ));
                    }
                    ax += w + 14.0;
                };
                if line.open.is_some() {
                    action(words.open_mail(), Some(Target::OpenMail(index)), MINT);
                }
                if line.reminded.is_empty() {
                    action(words.remind_me(), Some(Target::Remind(index)), MINT);
                } else {
                    action(&format!("✓ {}", line.reminded), None, DIM);
                }
                y += 12.0 * 1.4 + 2.0;
            }
            if line.attention {
                shapes.insert(
                    0,
                    Shape::Box {
                        rect: Rectangle::new(
                            Point::new(PAD - 6.0, line_top - 3.0),
                            Size::new(room + 12.0, y - line_top + 6.0),
                        ),
                        radius: 8.0,
                        fill: alpha(0xF5B44A, 0.07),
                        rim: None,
                        dashed: false,
                        sharp: Corner::None,
                    },
                );
            }
            titles.push(if line.reminded.is_empty() {
                line.title.clone()
            } else {
                format!("{} ({})", line.title, line.reminded)
            });
            y += 6.0;
        }
        if hidden > 0 {
            y = hidden_line(
                &mut shapes,
                &mut targets,
                open,
                hidden,
                PAD,
                room - PAD,
                y,
                words,
            ) + 6.0;
        }
        if let Some(card_id) = proposal
            && let Some(Body::Card { card }) =
                c.items.iter().find(|i| i.id == *card_id).map(|i| &i.body)
        {
            y = lay_out_card(
                &mut shapes,
                &mut targets,
                &mut Texts::default(),
                open,
                *card_id,
                card,
                PAD,
                room - PAD,
                y,
            ) + 6.0;
            titles.push(format!(
                "card:{}{}",
                card.label,
                card.outcome
                    .as_deref()
                    .map_or(String::new(), |o| format!(" → {o}"))
            ));
        }
        said.push(format!(
            "section:{key}:{}{}",
            titles.join(" | "),
            if hidden > 0 {
                format!(" (+{hidden} hidden)")
            } else {
                String::new()
            }
        ));
    }
    if !briefing.footer.is_empty() {
        y += 6.0;
        shapes.push(Shape::Rule(Rectangle::new(
            Point::new(PAD, y),
            Size::new(room - PAD, 0.0),
        )));
        y += 8.0;
        for line in lines_of(&briefing.footer, 11.5, false, room - PAD) {
            shapes.push(Shape::Text(Line {
                text: line,
                at: Point::new(PAD, y + 11.5 * 0.72),
                size: 11.5,
                color: FAINT,
                bold: false,
                align: Alignment::Left,
            }));
            y += 11.5 * 1.4;
        }
    }
    y += THREAD_BOTTOM;
    laid.content = y;
    let shown = laid.thread.height;
    let most = (y - shown).max(0.0);
    let scroll = open.scroll.clamp(0.0, most);
    let shift = top - (most - scroll);
    for shape in shapes {
        laid.shapes.push(moved(shape, shift));
    }
    for (target, rect) in targets {
        let rect = Rectangle::new(Point::new(rect.x, rect.y + shift), rect.size());
        if rect.y + rect.height > top && rect.y < top + shown {
            laid.targets.push((target, rect));
        }
    }
}

// --- Drawing --------------------------------------------------------------------------------

/// A drag held past the thread's edge scrolls it this many px a second
/// for each px past, within these.
const EDGE_PACE: f32 = 10.0;
const EDGE_SLOWEST: f32 = 150.0;
const EDGE_FASTEST: f32 = 1500.0;

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
    laid: Option<(Key, Laid)>,
    logged: String,
    places: String,
    texts: String,
}

/// What a layout is made for.
type Key = (u64, u64, u64, u32, u32, Option<Target>, u8);

/// The key a layout is made for: the conversation's, the drawer's and the
/// agents' changes, its size, what the pointer is on, her line.
fn key(open: &Open, c: &Conversation, size: Size) -> Key {
    (
        c.rev,
        open.rev,
        super::agents_seen(),
        size.width.to_bits(),
        size.height.to_bits(),
        open.hover,
        crate::secretary::state() as u8 | (crate::speak::moment_key() << 4),
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
                let mut laid = lay_out(open, c, crate::secretary::state(), self.words, size);
                // Each « Noté »'s bar from when it first showed (#758).
                let now = Instant::now();
                for shape in &mut laid.shapes {
                    if let Shape::Countdown { item, since, .. } = shape {
                        *since = Some(*open.noted.entry(*item).or_insert(now));
                    }
                }
                // A briefing unfolded scrolls from its top: its scroll kept
                // within it.
                if open.point.is_some() {
                    open.scroll = open
                        .scroll
                        .clamp(0.0, (laid.content - laid.thread.height).max(0.0));
                }
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
        let edge = self.edge(state);
        let Some(laid) = self.laid(state, size) else {
            return false;
        };
        // The selection follows the pointer over the thread scrolled.
        if let Some((p, _)) = edge
            && let Some(spot) = laid.texts.spot_at(p, &measure)
        {
            super::with(|d| {
                if let Some(selection) = d.open_mut(self.id).and_then(|o| o.selection.as_mut())
                    && selection.hold == Hold::Dragging
                {
                    selection.focus = spot;
                }
            });
        }
        let edging = edge.is_some_and(|(_, going)| going);
        let said = laid.said.to_string();
        let places: Vec<String> = laid
            .targets
            .iter()
            .map(|(t, r)| format!("{}@{:.0},{:.0}", t.name(), r.center_x(), r.center_y()))
            .collect();
        let places = places.join(" ");
        // Where her text's blocks start and end (the first line's start,
        // the last's end), never what they say.
        let texts: Vec<String> = laid
            .texts
            .blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| b.shown)
            .filter_map(|(n, b)| {
                let first = laid.texts.runs.iter().find(|r| r.block == n)?;
                let last = laid.texts.runs.iter().rfind(|r| r.block == n)?;
                Some(format!(
                    "{:?}@{:.0},{:.0}>{:.0},{:.0}",
                    b.source,
                    first.x,
                    first.middle,
                    last.x + last.width,
                    last.middle
                ))
            })
            .collect();
        let texts = texts.join(" ");
        let mood = laid.mood;
        if said != state.logged {
            tracing::debug!("the drawer shows {said}");
            state.logged = said;
        }
        if places != state.places {
            tracing::debug!("the drawer's items {places}");
            state.places = places;
        }
        if texts != state.texts {
            tracing::debug!("the drawer's texts {texts}");
            state.texts = texts;
        }
        let now = Instant::now();
        let plays = super::with(|d| {
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
        });
        plays || edging
    }

    /// A drag selecting held past the thread's top or bottom (#761): the
    /// thread scrolls toward it by the time since the last frame, the
    /// faster the further past, until its end. Where the pointer is, and
    /// whether it goes on (another frame is asked for).
    fn edge(&self, state: &State) -> Option<(Point, bool)> {
        let (_, laid) = state.laid.as_ref()?;
        let thread = laid.thread;
        let most = (laid.content - thread.height).max(0.0);
        super::with(|d| {
            let open = d.open_mut(self.id)?;
            let dragging = open.selection.is_some_and(|s| s.hold == Hold::Dragging);
            let Some((p, since)) = open.edge.filter(|_| dragging) else {
                open.edge = None;
                return None;
            };
            let now = Instant::now();
            open.edge = Some((p, now));
            let (past, up) = if p.y < thread.y {
                (thread.y - p.y, true)
            } else {
                (p.y - thread.y - thread.height, false)
            };
            let end = if up { most } else { 0.0 };
            let before = open.scroll.clamp(0.0, most);
            if before == end {
                return Some((p, false));
            }
            let speed = (past * EDGE_PACE).clamp(EDGE_SLOWEST, EDGE_FASTEST);
            let step = speed * now.duration_since(since).as_secs_f32().min(0.1);
            let after = if up { before + step } else { before - step }.clamp(0.0, most);
            open.scroll = after;
            open.menu = None;
            open.changed();
            if after == end {
                tracing::debug!(
                    "the selection scrolled the thread to its {}",
                    if up { "top" } else { "bottom" }
                );
            }
            Some((p, after != end))
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
        // With the queue's button under them, the title and status go up.
        let middle = laid.head.center_y()
            - if laid.queue_button.is_some() {
                7.0
            } else {
                0.0
            };
        f.fill_text(text(
            &laid.title,
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
        let status_x = if laid.dot {
            f.fill(
                &Path::circle(Point::new(text_x + DOT / 2.0, status_y), DOT / 2.0),
                dot,
            );
            text_x + DOT + 6.0
        } else {
            text_x
        };
        let room = laid
            .targets
            .iter()
            .find(|(t, _)| *t == Target::Gear)
            .map_or(right, |(_, r)| r.x)
            - status_x
            - 8.0;
        let status = crate::popover::kit::elide(&laid.status, room, |s| width_of(s, STATUS, false));
        f.fill_text(text(
            &status,
            Point::new(status_x, status_y),
            STATUS,
            DIM,
            false,
            Alignment::Left,
        ));
        // « 21 mails à trier › », a button (#758).
        if let Some((rect, label)) = &laid.queue_button {
            let hover = open.hover == Some(Target::Status);
            f.fill(
                &Path::rounded_rectangle(rect.position(), rect.size(), 6.0.into()),
                alpha(0xFFFFFF, if hover { 0.12 } else { 0.06 }),
            );
            f.stroke(
                &Path::rounded_rectangle(
                    Point::new(rect.x + 0.5, rect.y + 0.5),
                    Size::new(rect.width - 1.0, rect.height - 1.0),
                    5.5.into(),
                ),
                Stroke::default()
                    .with_color(alpha(0xFFFFFF, if hover { 0.28 } else { 0.16 }))
                    .with_width(1.0),
            );
            f.fill_text(text(
                label,
                rect.center(),
                QUEUE_TEXT,
                if hover { MINT } else { hex(0xD5DBE5) },
                true,
                Alignment::Center,
            ));
        }
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
        // The hint under her header.
        if let Some(rect) = laid.hint_rect {
            f.fill(
                &Path::rounded_rectangle(rect.position(), rect.size(), 9.0.into()),
                alpha(0xF5B44A, 0.08),
            );
            f.stroke(
                &Path::rounded_rectangle(
                    Point::new(rect.x + 0.5, rect.y + 0.5),
                    Size::new(rect.width - 1.0, rect.height - 1.0),
                    8.5.into(),
                ),
                Stroke::default()
                    .with_color(alpha(0xF5B44A, 0.35))
                    .with_width(1.0),
            );
            for (n, line) in laid.hint.iter().enumerate() {
                f.fill_text(text(
                    line,
                    Point::new(
                        rect.x + HINT_PAD + 3.0,
                        rect.y + HINT_PAD + n as f32 * HINT_LINE + HINT_LINE / 2.0,
                    ),
                    HINT,
                    hex(0xFFE2A8),
                    false,
                    Alignment::Left,
                ));
            }
        }
        if let Some(band) = &laid.key_band {
            draw_key_band(f, band, open);
        }
        // The tabs, or a sub-view's bar.
        f.fill(
            &Path::rounded_rectangle(laid.tabs.position(), laid.tabs.size(), 9.0.into()),
            alpha(0xFFFFFF, 0.05),
        );
        if let Some((rect, title)) = &laid.subbar {
            let hover = open.hover == Some(Target::Back);
            f.fill_text(text(
                words.back(),
                Point::new(rect.x + 12.0, rect.center_y()),
                SUB_TEXT,
                if hover { hex(0x9BEBB8) } else { MINT },
                true,
                Alignment::Left,
            ));
            let room = rect.width - width_of(words.back(), SUB_TEXT, true) - 40.0;
            let title = crate::popover::kit::elide(title, room, |s| width_of(s, 13.0, true));
            f.fill_text(text(
                &title,
                Point::new(rect.x + rect.width - 12.0, rect.center_y()),
                13.0,
                TEXT,
                true,
                Alignment::Right,
            ));
        }
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
            for shape in &laid.shapes {
                draw_shape(f, shape, now);
            }
            // Her text selected (#757), in the theme's selection colour
            // (#761); nothing at rest.
            if let Some(selection) = open.selection.filter(|s| !s.is_empty()) {
                let tint = crate::theme::SELECTION.get();
                for rect in laid.texts.highlights(&selection, &measure) {
                    f.fill_rectangle(rect.position(), rect.size(), tint);
                }
            }
        });
        if laid.knows || !laid.composer {
            // In a layer of its own, as the list's is: it comes over it.
            f.with_clip(Rectangle::new(Point::ORIGIN, size), |f| {
                for shape in &laid.overlay {
                    draw_shape(f, shape, now);
                }
            });
            // Her text copied, and the right click's menu (#757).
            self.draw_copied(f, laid, open);
            self.draw_menu(f, open, size);
            return;
        }
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
        // Its ring in the accent while the keyboard types there (#752).
        let typing_here = open.focused && laid.key_band.as_ref().is_none_or(|_| !open.key_focus);
        f.stroke(
            &Path::rounded_rectangle(
                Point::new(field.x + 0.5, field.y + 0.5),
                Size::new(field.width - 1.0, field.height - 1.0),
                11.5.into(),
            ),
            Stroke::default()
                .with_color(alpha(0x6CC287, if typing_here { 0.9 } else { 0.3 }))
                .with_width(if typing_here { 1.5 } else { 1.0 }),
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
        // Its caret while the keyboard types there, still as the shell's
        // other fields' (nothing blinks at rest): after what is typed,
        // else just before the placeholder's first letter (#752).
        if typing_here {
            let x = if empty { caret.x - 2.0 } else { caret.x };
            f.fill_rectangle(Point::new(x, caret.y - 8.0), Size::new(1.5, 16.0), MINT);
        }
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
        // Her model's name is the Settings' (#758): nothing here.
        self.draw_copied(f, laid, open);
        self.draw_menu(f, open, size);
    }

    /// « Copié » over the thread's bottom while it shows.
    fn draw_copied(&self, f: &mut Frame, laid: &Laid, open: &Open) {
        if open.copied.is_none() {
            return;
        }
        let said = self.words.copied();
        let w = width_of(said, 12.5, true) + 2.0 * 14.0;
        let h = 12.5 * 1.4 + 2.0 * 7.0;
        let rect = Rectangle::new(
            Point::new(
                (laid.size.width - w) / 2.0,
                laid.thread.y + laid.thread.height - h - 10.0,
            ),
            Size::new(w, h),
        );
        f.fill(
            &Path::rounded_rectangle(rect.position(), rect.size(), (h / 2.0).into()),
            Color::from_rgba8(14, 17, 28, 0.96),
        );
        f.stroke(
            &Path::rounded_rectangle(
                Point::new(rect.x + 0.5, rect.y + 0.5),
                Size::new(rect.width - 1.0, rect.height - 1.0),
                (h / 2.0 - 0.5).into(),
            ),
            Stroke::default()
                .with_color(alpha(0x6CC287, 0.5))
                .with_width(1.0),
        );
        f.fill_text(text(
            said,
            rect.center(),
            12.5,
            TEXT,
            true,
            Alignment::Center,
        ));
    }

    /// The right click's menu: « Copier », and its shortcut.
    fn draw_menu(&self, f: &mut Frame, open: &Open, size: Size) {
        let Some(menu) = &open.menu else {
            return;
        };
        let (outer, row) = menu_rects(menu.at, size, self.words);
        f.fill(
            &Path::rounded_rectangle(outer.position(), outer.size(), 9.0.into()),
            hex(0x1A1F2E),
        );
        f.stroke(
            &Path::rounded_rectangle(
                Point::new(outer.x + 0.5, outer.y + 0.5),
                Size::new(outer.width - 1.0, outer.height - 1.0),
                8.5.into(),
            ),
            Stroke::default()
                .with_color(alpha(0xFFFFFF, 0.16))
                .with_width(1.0),
        );
        if open.hover == Some(Target::CopyMenu) {
            f.fill(
                &Path::rounded_rectangle(row.position(), row.size(), 6.0.into()),
                alpha(0xFFFFFF, 0.09),
            );
        }
        f.fill_text(text(
            self.words.copy(),
            Point::new(row.x + MENU_X, row.center_y()),
            MENU_TEXT,
            TEXT,
            false,
            Alignment::Left,
        ));
        f.fill_text(text(
            MENU_KEYS,
            Point::new(row.x + row.width - MENU_X, row.center_y()),
            11.0,
            FAINT,
            false,
            Alignment::Right,
        ));
    }
}

/// The right click's menu: its text, its row's padding, its shortcut.
const MENU_TEXT: f32 = 13.0;
const MENU_X: f32 = 10.0;
const MENU_ROW: f32 = 28.0;
const MENU_KEYS: &str = "Ctrl+C";

/// The right click's menu at `at`, kept within `size`: its box and its
/// « Copier » row.
fn menu_rects(at: Point, size: Size, words: Words) -> (Rectangle, Rectangle) {
    let w = (width_of(words.copy(), MENU_TEXT, false)
        + width_of(MENU_KEYS, 11.0, false)
        + 3.0 * MENU_X
        + 16.0
        + 8.0)
        .max(140.0);
    let h = MENU_ROW + 8.0;
    let x = at.x.min(size.width - w - 4.0).max(4.0);
    let y = at.y.min(size.height - h - 4.0).max(4.0);
    let outer = Rectangle::new(Point::new(x, y), Size::new(w, h));
    let row = Rectangle::new(Point::new(x + 4.0, y + 4.0), Size::new(w - 8.0, MENU_ROW));
    (outer, row)
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
        Shape::Quote(rect) => f.fill_rectangle(rect.position(), rect.size(), alpha(0x7EE0A0, 0.5)),
        Shape::Mono(line) => {
            let mut t = text(
                &line.text, line.at, line.size, line.color, false, line.align,
            );
            t.font = iced::Font::with_name("Martian Mono");
            f.fill_text(t);
        }
        Shape::Her(rect) => {
            let (handle, _) = pandas::secretary(hocus_pandas::SecretaryMood::Happy, None, now);
            f.draw_svg(*rect, svg::Svg::new(handle));
        }
        Shape::Tick(rect, done) => {
            let path = Path::rounded_rectangle(rect.position(), rect.size(), 4.0.into());
            if *done {
                f.fill(&path, MINT);
                let tick = Path::new(|p| {
                    p.move_to(Point::new(rect.x + 3.5, rect.y + 7.5));
                    p.line_to(Point::new(rect.x + 6.0, rect.y + 10.0));
                    p.line_to(Point::new(rect.x + 10.5, rect.y + 4.5));
                });
                f.stroke(&tick, Stroke::default().with_color(INK).with_width(2.0));
            } else {
                f.stroke(
                    &path,
                    Stroke::default()
                        .with_color(alpha(0xFFFFFF, 0.35))
                        .with_width(1.5),
                );
            }
        }
        Shape::Countdown { rect, since, .. } => {
            let left = since.map_or(1.0, |t| {
                1.0 - now.saturating_duration_since(t).as_secs_f32() / NOTED.as_secs_f32()
            });
            if left > 0.0 {
                f.fill_rectangle(rect.position(), rect.size(), alpha(0x8FB3FF, 0.15));
                f.fill_rectangle(
                    rect.position(),
                    Size::new(rect.width * left.min(1.0), rect.height),
                    alpha(0x8FB3FF, 0.7),
                );
            }
        }
        Shape::Lens(c) => {
            let stroke = Stroke::default().with_color(FAINT).with_width(1.5);
            f.stroke(&Path::circle(Point::new(c.x - 1.5, c.y - 1.5), 5.0), stroke);
            let handle = Path::new(|p| {
                p.move_to(Point::new(c.x + 2.2, c.y + 2.2));
                p.line_to(Point::new(c.x + 6.0, c.y + 6.0));
            });
            f.stroke(&handle, stroke);
        }
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

/// Her key's field, masked, its caret while it has the keyboard, and
/// "Enregistrer".
fn draw_key_band(f: &mut Frame, band: &KeyBand, open: &Open) {
    for shape in &band.shapes {
        draw_shape(f, shape, Instant::now());
    }
    let field = band.field;
    f.fill(
        &Path::rounded_rectangle(field.position(), field.size(), 8.0.into()),
        alpha(0x000000, 0.25),
    );
    f.stroke(
        &Path::rounded_rectangle(
            Point::new(field.x + 0.5, field.y + 0.5),
            Size::new(field.width - 1.0, field.height - 1.0),
            7.5.into(),
        ),
        Stroke::default()
            .with_color(if band.error {
                alpha(0xF2945B, 0.9)
            } else if band.focused {
                alpha(0x6CC287, 0.85)
            } else {
                alpha(0xFFFFFF, 0.16)
            })
            .with_width(if band.focused || band.error { 1.5 } else { 1.0 }),
    );
    let x = field.x + 10.0;
    let y = field.center_y();
    f.fill_text(text(
        &band.shown,
        Point::new(x, y),
        if band.placeholder { 11.5 } else { 13.0 },
        if band.placeholder { FAINT } else { TEXT },
        !band.placeholder,
        Alignment::Left,
    ));
    if band.focused {
        let caret_x = if band.placeholder {
            x - 2.0
        } else {
            x + width_of(&band.shown, 13.0, true) + 1.0
        };
        f.fill_rectangle(Point::new(caret_x, y - 8.0), Size::new(1.5, 16.0), MINT);
    }
    let save = band.save;
    let hover = open.hover == Some(Target::KeySave);
    let ready = !band.placeholder;
    f.fill(
        &Path::rounded_rectangle(save.position(), save.size(), 8.0.into()),
        match (ready, hover) {
            (true, false) => GREEN,
            (true, true) => hex(0x7ED198),
            (false, _) => alpha(0xFFFFFF, 0.07),
        },
    );
    f.fill_text(text(
        super::words().pick("Enregistrer", "Save"),
        save.center(),
        BUTTON,
        if ready { INK } else { DIM },
        true,
        Alignment::Center,
    ));
    for (line, colour, at) in &band.lines {
        f.fill_text(text(line, *at, HINT, *colour, false, Alignment::Left));
    }
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
                // A press on it gives it the keyboard (the compositor's
                // on-demand rule, #755): the toolkit says no "Focused"
                // when the pointer came in first, so it is taken here.
                let pressed = matches!(seen, mouse::Event::ButtonPressed(_))
                    && cursor.position_in(bounds).is_some()
                    && super::focused(self.id, true);
                // Where the pointer is even past its edges, as a drag that
                // selects goes on there.
                let raw = cursor
                    .position()
                    .map(|p| Point::new(p.x - bounds.x, p.y - bounds.y));
                let action =
                    self.mouse(state, seen, cursor.position_in(bounds), raw, bounds.size());
                match action {
                    None if pressed => Some(canvas::Action::request_redraw()),
                    action => action,
                }
            }
            iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) => {
                // Ctrl+C copies her text selected (#757), wherever the
                // keyboard types.
                if copies(key, *modifiers)
                    && let Some(selected) = self.selected(state, bounds.size())
                {
                    super::copy(self.id, selected);
                    return Some(canvas::Action::request_redraw().and_capture());
                }
                super::key_on(self.id, key, *modifiers, text.as_deref())
                    .then(|| canvas::Action::request_redraw().and_capture())
            }
            // The keyboard on the drawer or gone from it: its caret and
            // its ring follow (#752).
            iced::Event::Window(window::Event::Focused) => {
                super::focused(self.id, true).then(canvas::Action::request_redraw)
            }
            iced::Event::Window(window::Event::Unfocused) => {
                super::focused(self.id, false).then(canvas::Action::request_redraw)
            }
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
        state: &State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        // Over her text, or selecting it: the text cursor.
        let on_text = |selecting: bool| {
            selecting
                || cursor.position_in(bounds).is_some_and(|p| {
                    state
                        .laid
                        .as_ref()
                        .is_some_and(|(_, laid)| laid.texts.on_text(p))
                })
        };
        super::with(|d| {
            let Some(open) = d.open_on(self.id) else {
                return mouse::Interaction::default();
            };
            let selecting = open.selection.is_some_and(|s| s.hold == Hold::Dragging);
            match open.hover {
                Some(Target::Field | Target::Search | Target::KeyField) => mouse::Interaction::Text,
                Some(_) if !selecting => mouse::Interaction::Pointer,
                _ if on_text(selecting) => mouse::Interaction::Text,
                _ => mouse::Interaction::default(),
            }
        })
    }
}

/// Whether `key` with `modifiers` asks to copy: `Ctrl+C`.
fn copies(key: &iced::keyboard::Key, modifiers: iced::keyboard::Modifiers) -> bool {
    matches!(key, iced::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("c"))
        && modifiers.control()
        && !modifiers.alt()
        && !modifiers.logo()
}

impl View {
    /// Her text selected now, if any.
    fn selected(&self, state: &mut State, size: Size) -> Option<String> {
        let laid = self.laid(state, size)?;
        let selection = super::with(|d| d.open_on(self.id).and_then(|o| o.selection))?;
        Some(laid.texts.selected(&selection)).filter(|t| !t.is_empty())
    }

    fn mouse<Message>(
        &self,
        state: &mut State,
        seen: &mouse::Event,
        at: Option<Point>,
        raw: Option<Point>,
        size: Size,
    ) -> Option<canvas::Action<Message>> {
        let laid = self.laid(state, size)?;
        let content = laid.content - laid.thread.height;
        enum Then {
            Nothing,
            Draw,
            Act(Target),
        }
        let then = super::with(|d| {
            let open = d.open_mut(self.id)?;
            open.pointer = at;
            // What takes a click there: the menu, then the rest.
            let target = at.and_then(|p| {
                let menu = open
                    .menu
                    .as_ref()
                    .filter(|m| menu_rects(m.at, size, self.words).1.contains(p))
                    .map(|_| Target::CopyMenu);
                menu.or_else(|| {
                    laid.targets
                        .iter()
                        .rev()
                        .find(|(_, r)| r.contains(p))
                        .map(|(t, _)| *t)
                })
            });
            Some(match seen {
                mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                    let mut draw = false;
                    if open.hover != target {
                        open.hover = target;
                        draw = true;
                    }
                    // A press on her text drags its selection; held past
                    // the thread's top or bottom, the thread scrolls under
                    // it (#761), as each frame goes ([`View::edge`]).
                    if let Some(selection) = &mut open.selection
                        && selection.hold == Hold::Dragging
                    {
                        if let Some(spot) = raw.and_then(|p| laid.texts.spot_at(p, &measure))
                            && spot != selection.focus
                        {
                            selection.focus = spot;
                            draw = true;
                        }
                        let thread = laid.thread;
                        let past = raw.filter(|p| p.y < thread.y || p.y > thread.y + thread.height);
                        open.edge = match (past, open.edge) {
                            (Some(p), Some((_, since))) => Some((p, since)),
                            (Some(p), None) => {
                                draw = true;
                                Some((p, Instant::now()))
                            }
                            (None, _) => None,
                        };
                    }
                    if draw { Then::Draw } else { Then::Nothing }
                }
                mouse::Event::CursorLeft => {
                    open.hover = None;
                    Then::Draw
                }
                mouse::Event::ButtonPressed(mouse::Button::Left) => {
                    if target == Some(Target::CopyMenu) {
                        open.pressed = target;
                        return Some(Then::Nothing);
                    }
                    let had = open.menu.take().is_some() || open.selection.is_some();
                    open.pressed = target;
                    // A press on a button never starts a selection.
                    if target.is_some() {
                        open.selection = None;
                        return Some(if had { Then::Draw } else { Then::Nothing });
                    }
                    let Some(p) = at else {
                        return Some(Then::Nothing);
                    };
                    // One, two or three presses in a row: a place, a word,
                    // a paragraph.
                    let now = Instant::now();
                    let clicks = match open.clicks {
                        Some((then, q, n))
                            if now.duration_since(then) <= super::MULTI_CLICK
                                && (q.x - p.x).abs() <= 4.0
                                && (q.y - p.y).abs() <= 4.0 =>
                        {
                            n % 3 + 1
                        }
                        _ => 1,
                    };
                    open.clicks = Some((now, p, clicks));
                    let spot = Some(p)
                        .filter(|p| laid.thread.contains(*p))
                        .and_then(|p| laid.texts.spot_at(p, &measure));
                    open.selection = spot.and_then(|spot| {
                        let (anchor, focus, hold) = match clicks {
                            1 => (spot, spot, Hold::Dragging),
                            2 => {
                                let (a, b) = laid.texts.word_at(spot)?;
                                (a, b, Hold::Done)
                            }
                            _ => {
                                let (a, b) = laid.texts.paragraph_at(spot)?;
                                (a, b, Hold::Done)
                            }
                        };
                        Some(Selection {
                            anchor,
                            focus,
                            hold,
                        })
                    });
                    if clicks > 1 {
                        log_selection(laid, open.selection.as_ref());
                    }
                    Then::Draw
                }
                mouse::Event::ButtonReleased(mouse::Button::Left) => {
                    open.edge = None;
                    if let Some(selection) = &mut open.selection
                        && selection.hold == Hold::Dragging
                    {
                        selection.hold = Hold::Done;
                        if selection.is_empty() {
                            open.selection = None;
                        }
                        log_selection(laid, open.selection.as_ref());
                    }
                    match open.pressed.take().filter(|p| Some(*p) == target) {
                        Some(target) => Then::Act(target),
                        None => Then::Nothing,
                    }
                }
                mouse::Event::ButtonPressed(mouse::Button::Right) => {
                    // « Copier » in a menu: what is selected, else the
                    // answer (or the text) under the pointer.
                    let Some(p) = at else {
                        return Some(Then::Nothing);
                    };
                    if target.is_some() {
                        return Some(Then::Nothing);
                    }
                    let selected = open
                        .selection
                        .map(|s| laid.texts.selected(&s))
                        .filter(|t| !t.is_empty());
                    let text = selected.or_else(|| {
                        let block = laid.texts.block_at(p)?;
                        let item = block.item.and_then(|item| {
                            crate::secretary::conversation(|c| {
                                c.items
                                    .iter()
                                    .find(|i| i.id == item)
                                    .and_then(super::item_copy)
                            })
                        });
                        item.or_else(|| Some(block.text.clone()))
                            .filter(|t| !t.is_empty())
                    });
                    match text {
                        Some(text) => {
                            open.menu = Some(super::Menu { at: p, text });
                            let (_, row) = menu_rects(p, size, self.words);
                            tracing::debug!(
                                "the drawer's menu CopyMenu@{:.0},{:.0}",
                                row.center_x(),
                                row.center_y()
                            );
                            Then::Draw
                        }
                        None => Then::Nothing,
                    }
                }
                mouse::Event::WheelScrolled { delta } => {
                    let step = match delta {
                        mouse::ScrollDelta::Lines { y, .. } => y * 40.0,
                        mouse::ScrollDelta::Pixels { y, .. } => *y,
                    };
                    if open.tab == super::Tab::Knows {
                        // A list from its top: the wheel up goes up.
                        let before = open.knows_scroll;
                        open.knows_scroll = (open.knows_scroll - step).clamp(0.0, content.max(0.0));
                        if (open.knows_scroll - before).abs() < 0.5 {
                            return Some(Then::Nothing);
                        }
                        open.rev += 1;
                        return Some(Then::Draw);
                    }
                    let before = open.scroll;
                    open.scroll = (open.scroll + step).clamp(0.0, content.max(0.0));
                    if (open.scroll - before).abs() < 0.5 {
                        return Some(Then::Nothing);
                    }
                    open.menu = None;
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

/// What is selected, for the tests: how many characters, never which.
fn log_selection(laid: &Laid, selection: Option<&Selection>) {
    let chars = selection.map_or(0, |s| laid.texts.selected(s).chars().count());
    tracing::debug!(chars, "the drawer's selection");
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
            notebook: None,
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
            knows: None,
            queue: None,
            toast: None,
            key_asked: None,
            key_said: None,
            rev: 1,
        };
        let words = Words { fr: true };
        let kinds: Vec<&str> = entries(&c, words, false)
            .iter()
            .map(|e| match e {
                Entry::Day(_) => "day",
                Entry::Item(_) => "item",
                Entry::Streaming(..) => "streaming",
                Entry::Typing(_) => "typing",
                Entry::Hidden(_) => "hidden",
                Entry::Greeting(_) => "greeting",
            })
            .collect();
        assert_eq!(kinds, ["day", "item", "streaming", "item", "typing"]);
        // Once her answer streams, no dots for it.
        let streaming = Conversation {
            streaming: vec![(2, "Ok".to_owned())],
            ..c
        };
        assert!(
            !entries(&streaming, words, false)
                .iter()
                .any(|e| matches!(e, Entry::Typing(_)))
        );
    }
}
