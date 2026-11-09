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
    /// A line in Martian Mono (a briefing's times).
    Mono(Line),
    /// Her head at a briefing's top, happy.
    Her(Rectangle),
    /// A plan's box, ticked or not.
    Tick(Rectangle, bool),
    /// The search field's lens, 15 px, centred there.
    Lens(Point),
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
}

/// The dots a key shows as, at most this many: its length is not drawn
/// past it.
const KEY_DOTS: usize = 32;
const KEY_FIELD: f32 = 30.0;

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
    /// Perso items hidden in a public moment: how many.
    Hidden(usize),
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
                out.push(Entry::Streaming(said));
            }
        }
    }
    if hidden > 0 {
        out.push(Entry::Hidden(hidden));
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
    if open.tab == Tab::Knows && line == Her::Ready {
        laid.status = words.knows_subtitle().to_owned();
    }
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
    // Her status line: her triage queue unfolds (#754).
    if line == Her::Ready && look.is_some() && open.tab == Tab::Talk {
        let x = laid.head.x + HEAD + HEAD_GAP;
        let y = laid.head.center_y() + 11.0 - 9.0;
        laid.targets.push((
            Target::Status,
            Rectangle::new(
                Point::new(x, y),
                Size::new((gear.x - 8.0 - x).max(0.0), 18.0),
            ),
        ));
    }
    // How to fix what her status says: a band under her header.
    let mut tabs_top = HEAD_TOP + HEAD + HEAD_BOTTOM + TABS_TOP;
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
        "model": laid.model,
        "search": open.search,
    });
    laid
}

/// Her key's band at `top` (#752): « Colle ta clé Anthropic », masked,
/// « Enregistrer », « Elle va dans ton trousseau, jamais dans un
/// fichier. », what became of the last key given, and the terminal's way.
/// The tabs' top under it.
fn lay_out_key(
    laid: &mut Laid,
    open: &Open,
    c: &Conversation,
    words: Words,
    provider: &str,
    top: f32,
    right: f32,
) -> f32 {
    let inner_x = PAD + HINT_PAD + 3.0;
    let inner = right - inner_x - HINT_PAD - 3.0;
    let mut at = top + HINT_PAD + 2.0;
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
    let name = match provider {
        "anthropic" => "Anthropic",
        other => other,
    };
    let (shown, placeholder) = if open.key.is_empty() {
        (
            if words.fr {
                format!("Colle ta clé {name}")
            } else {
                format!("Paste your {name} key")
            },
            true,
        )
    } else {
        ("•".repeat(open.key.chars().count().min(KEY_DOTS)), false)
    };
    let mut said: Vec<(String, Color)> = vec![(
        words
            .pick(
                "Elle va dans ton trousseau, jamais dans un fichier.",
                "It goes to your keyring, never to a file.",
            )
            .to_owned(),
        hex(0xFFE2A8),
    )];
    if c.key_asked.is_some() {
        said.push((words.pick("Je la vérifie…", "Checking it…").to_owned(), DIM));
    } else if let Some((given, outcome)) = &c.key_said
        && *outcome != hocus_secretary_link::KeyOutcome::Kept
    {
        said.push((outcome.words(given, words.fr), ORANGE));
    }
    if let Some(hint) = c
        .look
        .as_ref()
        .map(|l| l.hint.clone())
        .filter(|h| !h.is_empty())
    {
        said.push((hint, FAINT));
    }
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
    laid.targets.push((Target::KeySave, save));
    laid.targets.push((Target::KeyField, field));
    laid.key_band = Some(KeyBand {
        field,
        save,
        shown,
        placeholder,
        focused: open.focused && open.key_focus,
        lines,
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
    let mut y = THREAD_TOP;
    let list = entries(c, words, hide(open));
    let mut previous_hers = false;
    // Each entry's shapes (from this one), its top and its bottom: only
    // whole entries are drawn (the CPU renderer does not clip).
    let mut spans: Vec<(usize, f32, f32)> = Vec::new();
    for (n, entry) in list.iter().enumerate() {
        if n > 0 {
            y += GAP;
        }
        if let Some(last) = spans.last_mut() {
            last.2 = y - GAP;
        }
        spans.push((shapes.len(), y, y));
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
                Body::Learned { text, undone, .. } => {
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
        Shape::Mono(mut line) => {
            line.at.y += dy;
            Shape::Mono(line)
        }
        Shape::Her(r) => Shape::Her(down(r)),
        Shape::Tick(r, done) => Shape::Tick(down(r), done),
        Shape::Lens(p) => Shape::Lens(Point::new(p.x, p.y + dy)),
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
        // Right-aligned, the primary last (the mockup's sheet).
        let widths: Vec<f32> = card
            .actions
            .iter()
            .map(|a| width_of(&a.label, BUTTON, true) + 2.0 * BUTTON_X)
            .collect();
        let total: f32 = widths.iter().sum::<f32>() + 6.0 * (widths.len() as f32 - 1.0);
        let mut bx = x + room - CARD_X - total;
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
            color: hex(0xB9CEFF),
            bold: true,
            align: Alignment::Left,
        }));
        at += CARD_LABEL * 1.4;
    }
    at += 4.0;
    if !briefing.sentence.is_empty() {
        let lines = lines_of(&briefing.sentence, CARD_TEXT, false, inner);
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
        rim: Some(alpha(0x8FB3FF, 0.45)),
        dashed: false,
        sharp: Corner::None,
    });
    shapes.push(Shape::Box {
        rect: Rectangle::new(Point::new(x, y), Size::new(room, at - y)),
        radius: 12.0,
        fill: alpha(0x8FB3FF, 0.08),
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
    let back = words.back();
    let bw = width_of(back, SMALL, true);
    shapes.push(Shape::Text(Line {
        text: back.to_owned(),
        at: Point::new(PAD, y + SMALL_LINE / 2.0),
        size: SMALL,
        color: MINT,
        bold: true,
        align: Alignment::Left,
    }));
    targets.push((
        Target::Back,
        Rectangle::new(
            Point::new(PAD - 4.0, y - 2.0),
            Size::new(bw + 8.0, SMALL_LINE + 4.0),
        ),
    ));
    y += SMALL_LINE + GAP;
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
    for (text, size, colour) in [
        (&briefing.date, 12.0, LABEL),
        (&briefing.sentence, 13.0, DIM),
    ] {
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
            titles.push(line.title.clone());
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
                let laid = lay_out(open, c, crate::secretary::state(), self.words, size);
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
        // Clickable: a chevron after it, brighter under the pointer (#754).
        let clickable = laid.targets.iter().any(|(t, _)| *t == Target::Status);
        let chevron = if open.queue { " ‹" } else { " ›" };
        let room = if clickable {
            room - width_of(chevron, STATUS, true)
        } else {
            room
        };
        let status = crate::popover::kit::elide(&laid.status, room, |s| width_of(s, STATUS, false));
        let hover = open.hover == Some(Target::Status);
        f.fill_text(text(
            &status,
            Point::new(text_x + DOT + 6.0, status_y),
            STATUS,
            if hover { TEXT } else { DIM },
            false,
            Alignment::Left,
        ));
        if clickable {
            f.fill_text(text(
                chevron,
                Point::new(
                    text_x + DOT + 6.0 + width_of(&status, STATUS, false),
                    status_y,
                ),
                STATUS,
                if hover { MINT } else { FAINT },
                true,
                Alignment::Left,
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
            for shape in &laid.shapes {
                draw_shape(f, shape, now);
            }
        });
        if laid.knows {
            // In a layer of its own, as the list's is: it comes over it.
            f.with_clip(Rectangle::new(Point::ORIGIN, size), |f| {
                for shape in &laid.overlay {
                    draw_shape(f, shape, now);
                }
            });
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
            .with_color(if band.focused {
                alpha(0x6CC287, 0.85)
            } else {
                alpha(0xFFFFFF, 0.16)
            })
            .with_width(if band.focused { 1.5 } else { 1.0 }),
    );
    let x = field.x + 10.0;
    let y = field.center_y();
    f.fill_text(text(
        &band.shown,
        Point::new(x, y),
        if band.placeholder { 12.5 } else { 13.0 },
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
        _state: &State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        super::with(|d| match d.open_on(self.id).and_then(|o| o.hover) {
            Some(Target::Field | Target::Search | Target::KeyField) => mouse::Interaction::Text,
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
                Entry::Streaming(_) => "streaming",
                Entry::Typing(_) => "typing",
                Entry::Hidden(_) => "hidden",
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
