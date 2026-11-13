//! The agents' group's surface, last of the bar's column (`crate::column`)
//! under the to-do list, as the approved 4.0 and 4.1 mockups draw it
//! (bar.md §4 group 3), and in the mini form its item (§5, 4.7).
//!
//! Its top 14 px under the to-do list, or where it would be (100 px under
//! the bar's top with nothing above it but the clock); as tall as what the
//! group shows, and one pixel, taking no pointer, when it shows nothing
//! (hidden). On a short bar its rows give way last, folding into "+ N"
//! ([`fit_rows`]). In the mini form it is the item's, under the to-do
//! item; while the form switches, it holds both. A change of what it
//! shows resizes it (layer-shell actions) and draws it alone.
//!
//! The mini item (4.7, variant A): her head, her ear in the moment's
//! colour, a badge with how many agents need the user (none at 0), and how
//! many work under her; hovering it 150 ms or a click opens her briefing,
//! with who works and who rests. On the card's place she is alone with a
//! "+", and opens Sources.
//!
//! The secretary heads it (4.6): her head, her ear in the moment's colour,
//! "AGENTS" and the total over her line; her row tinted amber or orange
//! when someone waits or is stuck. A click on her opens her briefing,
//! where "Sources" is. When an agent starts waiting, her ear radiates
//! twice and she hops twice (1.2 s), once; then she holds still. None of it
//! with reduced motion or on the CPU.
//!
//! "Un autre outil ?" opens the Sources popover, a row's
//! "Brancher" the consent popover, "Ne rien brancher" declines the tools
//! listed ([`crate::popover`]).
//!
//! The agents' rows (4.1): its panda, its name, the word on its right, a
//! tint for those that concern the user. A click on a row (or `Enter` on
//! it) goes to the agent. When they change, the rows slide to their new
//! places (450 ms), those going under the "+ N" line fading out there and
//! those coming from it fading in (300 ms), a row's tint changes over
//! 300 ms, and the panda of an agent whose state changed (or that came)
//! hops once (650 ms); then nothing moves. None of it with reduced motion
//! or on the CPU. With `[agents] pandas` off (6.16), a 10 px dot of the
//! state's colour stands in each panda's place.

use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock, PoisonError},
    time::{Duration, Instant},
};

use hocus_link::{AGENTS_NAMESPACE, EDGE, Form};
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme,
    advanced::svg,
    keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path},
        text::Alignment,
    },
    window,
};

use super::{
    AGENT_NAME, AGENT_WORD, BADGE, BADGE_TEXT, BUTTON, BUTTON_TEXT, CARD_IN, CARD_NAME_LINE,
    CARD_NOTE, CARD_NOTE_LINE, CARD_PANDA, CARD_PANDA_GAP, CARD_RADIUS, CARD_ROW_RADIUS, CARD_WORD,
    FOLLOWED, HEAD, HEAD_GAP, HEAD_TOP, HEADER, HEADER_RADIUS, IDLE, INTRO, INTRO_LINE, LABEL,
    LINK, Layout, Look, MARK, MARK_GAP, MARK_INK, MARK_LETTER, MARK_RADIUS, MINI_ITEM, MINI_PANDA,
    MINI_PANDA_IN, MINI_RADIUS, MINI_TEXT, MORE_TEXT, Mood, NAME, PANDA, PANDA_GAP, ROW_PAD,
    ROW_RADIUS, Row, SAYS, Words,
};
use crate::pandas::{self, Burst};
use crate::theme::Ink;
use crate::{
    clock::motion::bezier,
    levels::draw::{ACCENT, DIM, FAINT, LINE, MANROPE_BOLD, SIDE, TEXT, faded, pointer, rgb, ring},
    popover::{self, Item, kit},
};

/// The surfaces' widths: the bar inside its rim.
const FULL_WIDTH: u32 = 194;
const MINI_SURFACE: u32 = 54;
/// Around the group: the keyboard's ring, 2 px out and 3 px wide.
const ROOM: f32 = 5.0;

const CARD: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.05));
/// The rows' tints and words (4.1), and a row under the pointer.
const WAITING: (Color, Color) = (rgb(0xF5C46E), Color::from_rgba8(245, 180, 74, 0.12));
const STUCK: (Color, Color) = (rgb(0xF7A77A), Color::from_rgba8(242, 148, 91, 0.10));
const TO_SEE: (Color, Color) = (rgb(0x7EE0A0), Color::from_rgba8(108, 194, 135, 0.10));
/// Good news (#809): the board's violet card.
const NEWS: (Color, Color) = (rgb(0xB9A7F0), Color::from_rgba8(182, 156, 255, 0.12));
const RESTING: Ink = Ink::Dark(rgb(0x6A7384));
const ROW_HOVER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.06));
/// What a card's agent asks, `#C9D0DC`.
const NOTE_INK: Ink = Ink::Dark(rgb(0xC9D0DC));
/// The pandas' drawings' box (their view box).
const PANDA_BOX: (f32, f32) = (126.0, 120.5);
/// A resting agent's panda, asleep.
const ASLEEP: f32 = 0.55;
/// The dot in a panda's place, its diameter.
const DOT: f32 = 10.0;
/// The rows' motion: the slide, the fades and the tint, the hop.
const SLIDE: Duration = Duration::from_millis(450);
const SLIDE_CURVE: (f32, f32, f32, f32) = (0.2, 0.9, 0.25, 1.0);
const FADE: Duration = Duration::from_millis(300);
const EASE: (f32, f32, f32, f32) = (0.25, 0.1, 0.25, 1.0);
const HOP: Duration = Duration::from_millis(650);
const EASE_OUT: (f32, f32, f32, f32) = (0.0, 0.0, 0.58, 1.0);
/// The hop's keyframes, as the mockup's: up 4 px, down, up 2 px, down.
const HOP_KEYS: [(f32, f32); 5] = [(0.0, 0.0), (0.3, -4.0), (0.6, 0.0), (0.8, -2.0), (1.0, 0.0)];
const PLUG: Ink = Ink::Accent(0.18);
const PLUG_HOVER: Ink = Ink::Accent(0.28);
/// The secretary's row (4.6): lit amber at 8 % when someone waits, orange
/// at 7 % when an agent is stuck; `0.08` while her briefing is open.
const SECRETARY_WAITING: Ink = Ink::Dark(Color::from_rgba8(245, 180, 74, 0.08));
const SECRETARY_STUCK: Ink = Ink::Dark(Color::from_rgba8(242, 148, 91, 0.07));
const ROW_OPEN: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.08));
/// Her line's colours: `text.dim` calm, then the rows' words'.
const SAYS_WAITING: Ink = Ink::Dark(rgb(0xF5C46E));
const SAYS_STUCK: Ink = Ink::Dark(rgb(0xF7A77A));
/// The mini item (4.7): its badge's ring, 2 px in the bar's colour, and its
/// number's ink; "au travail" under how many work.
const BADGE_RING: f32 = 2.0;
const BADGE_RING_COLOUR: Ink = Ink::Dark(Color::from_rgba8(14, 17, 28, 0.95));
const BADGE_INK: Ink = Ink::Fixed(rgb(0x1C1406));
const AT_WORK: Ink = Ink::Dark(rgb(0x7C8698));
/// Her ear's amber, its rings' colour.
const EAR_WAITING: Ink = Ink::Fixed(rgb(0xF5B44A));
/// When someone starts waiting, the mockup's keyframes (#503, #649):
/// three rings out of her ear (`hpRing`, 830 ms `ease-out` each: from
/// 90 % to nothing, from 0.9 to 1.25 times their size), about 2.5 s,
/// and two hops at their start (`hpBob`, 600 ms `ease-in-out`, twice:
/// 3 px up at its middle).
const RING: Duration = Duration::from_millis(830);
const RINGS_OUT: u32 = 3;
const BOB: Duration = Duration::from_millis(600);
const TWICE: u32 = 2;
const BOB_UP: f32 = 3.0;
const EASE_IN_OUT: (f32, f32, f32, f32) = (0.42, 0.0, 0.58, 1.0);
/// Her drawing's view box, its ear's middle, and its rings: arcs of
/// radius 15 and 20 round the ear, from 199.9° turning 200° (the
/// drawing's `A15 15 0 1 1` paths), 2.2 units wide, the outer at 60 %.
const VIEW: f32 = 120.0;
const EAR: (f32, f32) = (85.0, 33.0);
const RINGS: [(f32, f32); 2] = [(15.0, 1.0), (20.0, 0.6)];
const RING_FROM: f32 = 199.88;
const RING_TURN: f32 = 200.0;
const RING_WIDTH: f32 = 2.2;

// --- Where the surface goes ------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct Place {
    id: window::Id,
    form: Form,
    switching: bool,
    /// The group's top under the bar's in its column, and the mini item's.
    top: f32,
    mini_top: f32,
    /// Its size and margins as last asked for: what it draws for.
    asked: Option<((u32, u32), Margin)>,
    input: Option<bool>,
}

/// A layer surface's margins: top, right, bottom, left.
type Margin = (i32, i32, i32, i32);

/// Bar `bar`'s group's top under the bar's in the full form, and its mini
/// item's: last of the column, under the to-do list (`crate::column`).
fn column_top(bar: &str, form: Form) -> f32 {
    crate::column::top(bar, form, crate::column::Part::Agents)
}

/// Bar `bar`'s group's top (`form` full) or mini item's, as last placed.
pub fn top(bar: &str, form: Form) -> f32 {
    places(|places| {
        places.get(bar).map(|p| match form {
            Form::Full => p.top,
            Form::Mini => p.mini_top,
        })
    })
    .unwrap_or_else(|| column_top(bar, form))
}

/// What the group takes in `form` as laid out, its 14 px after it left
/// out: the full form's group, or the mini item; 0 when it is hidden.
pub fn height(form: Form) -> f32 {
    let laid = layout();
    let Some(laid) = laid.as_ref().filter(|l| l.height > 0.0) else {
        return 0.0;
    };
    match form {
        Form::Full => laid.height,
        Form::Mini => laid.mini.as_ref().map_or(0.0, |m| m.height),
    }
}

/// Bar `bar`'s surface's top under the bar's, as last asked for.
fn top_of(bar: &str) -> f32 {
    places(|places| {
        places
            .get(bar)
            .and_then(|p| p.asked)
            .map(|(_, margin)| (margin.0 - EDGE) as f32)
    })
    .unwrap_or_else(|| column_top(bar, Form::Full) - ROOM)
}

static PLACES: Mutex<Option<HashMap<String, Place>>> = Mutex::new(None);
static LAYOUT: Mutex<Option<Layout>> = Mutex::new(None);
/// What the compositor said changed since the group was laid out.
static STALE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// What the group shows changed: laid out again as it next draws.
pub fn stale() {
    STALE.store(true, std::sync::atomic::Ordering::Relaxed);
}

fn places<T>(with: impl FnOnce(&mut HashMap<String, Place>) -> T) -> T {
    let mut places = PLACES.lock().unwrap_or_else(PoisonError::into_inner);
    with(places.get_or_insert_with(HashMap::new))
}

/// The group as last laid out.
pub fn layout() -> Option<Layout> {
    LAYOUT
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// The group's height as laid out (0: it shows nothing).
fn group_height() -> f32 {
    layout().map_or(0.0, |l| l.height)
}

/// The surface's top under the bar's in `form` for `place`: the group's,
/// or the mini item's (4.7), less the ring's room; the higher of the two
/// while the form switches, the surface then holding both.
fn surface_top(place: &Place) -> f32 {
    match (place.switching, place.form) {
        (false, Form::Full) => place.top - ROOM,
        (false, Form::Mini) => place.mini_top - ROOM,
        (true, _) => place.top.min(place.mini_top) - ROOM,
    }
}

/// The surface's size for `place` (the full one's width while switching,
/// as tall as both forms' items).
fn size_of(place: &Place) -> (u32, u32) {
    let (full, mini) = (height(Form::Full), height(Form::Mini));
    let tall = |height: f32| (height + 2.0 * ROOM).ceil() as u32;
    match (place.switching, place.form) {
        _ if full <= 0.0 => (
            if place.form == Form::Mini && !place.switching {
                MINI_SURFACE
            } else {
                FULL_WIDTH
            },
            1,
        ),
        (false, Form::Mini) => (MINI_SURFACE, tall(mini)),
        (false, Form::Full) => (FULL_WIDTH, tall(full)),
        (true, _) => {
            let top = place.top.min(place.mini_top);
            let bottom = (place.top + full).max(place.mini_top + mini);
            (FULL_WIDTH, tall(bottom - top))
        }
    }
}

/// Its layer-shell margins for `place`: its top in the column.
fn margin(place: &Place) -> Margin {
    (EDGE + surface_top(place).round() as i32, EDGE + 1, 0, 0)
}

/// Bar `bar`'s surface `id` opens in `form`: its size and margins.
pub fn opened(bar: &str, id: window::Id, form: Form) -> ((u32, u32), (i32, i32, i32, i32)) {
    let mut place = Place {
        id,
        form,
        switching: false,
        top: column_top(bar, Form::Full),
        mini_top: column_top(bar, Form::Mini),
        asked: None,
        input: None,
    };
    let asked = (size_of(&place), margin(&place));
    place.asked = Some(asked);
    places(|places| places.insert(bar.to_owned(), place));
    asked
}

pub fn forget(bar: &str) {
    places(|places| places.remove(bar));
}

fn refit(place: &mut Place) -> Option<crate::network::draw::Placed> {
    let asked = (size_of(place), margin(place));
    if place.asked == Some(asked) {
        return None;
    }
    place.asked = Some(asked);
    Some((place.id, asked.0, asked.1))
}

/// Bar `bar` in `form`, switching or not: the surface's new size, if it
/// changes.
pub fn form(bar: &str, form: Form, switching: bool) -> Option<crate::network::draw::Placed> {
    let tops = (column_top(bar, Form::Full), column_top(bar, Form::Mini));
    places(|places| {
        let place = places.get_mut(bar)?;
        place.form = form;
        place.switching = switching;
        (place.top, place.mini_top) = tops;
        refit(place)
    })
}

/// The column above it moved (the card, the sign, Today or the to-do list
/// came, went or changed height), or the group did: every bar's surface
/// to its place, last of the column.
pub fn moved() {
    // The column's tops read first: not under this lock.
    let bars: Vec<String> = places(|places| places.keys().cloned().collect());
    let tops: Vec<(String, f32, f32)> = bars
        .into_iter()
        .map(|bar| {
            let tops = (column_top(&bar, Form::Full), column_top(&bar, Form::Mini));
            (bar, tops.0, tops.1)
        })
        .collect();
    let mut effects: Vec<popover::Effect> = places(|places| {
        tops.iter()
            .filter_map(|(bar, top, mini_top)| {
                let place = places.get_mut(bar)?;
                (place.top, place.mini_top) = (*top, *mini_top);
                refit(place)
            })
            .map(|(id, size, margin)| popover::Effect::Resize {
                id,
                size,
                margin: Some(margin),
            })
            .collect()
    });
    if !effects.is_empty() {
        // Drawn again where it goes: on the CPU its backdrop is the bar's
        // colour there.
        effects.push(popover::Effect::Wake(AGENTS_NAMESPACE));
        popover::send(effects);
    }
}

/// The room the column leaves its rows (`crate::column::room`): on the
/// shortest bar in the full form, as many as fit there, the others in
/// "+ N" (§4: the agents' rows give way last). When that changes what the
/// group shows, it is laid out again.
pub fn fit_rows() {
    let bars: Vec<String> = places(|places| {
        places
            .iter()
            .filter(|(_, p)| p.form == Form::Full || p.switching)
            .map(|(bar, _)| bar.clone())
            .collect()
    });
    let room = bars
        .iter()
        .map(|bar| crate::column::room(bar, crate::column::Part::Agents))
        .reduce(f32::min)
        .unwrap_or(f32::INFINITY);
    if super::set_rows_room(room) {
        let words = super::Words::new(crate::clock::time::Lang::of_messages());
        tracing::debug!(
            room,
            rows = super::rows_shown(),
            shows = %words.said(&super::said().shows(super::now_us())),
            "the agents' rows' room"
        );
        stale();
        popover::send(vec![popover::Effect::Wake(AGENTS_NAMESPACE)]);
    }
}

/// The group was laid out again: every bar's surface to its height, and
/// the column with it.
pub fn laid_out(layout: Layout) {
    let before = LAYOUT
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .replace(layout.clone())
        .map(|l| (l.height, l.mini.map(|m| m.height)));
    let effects: Vec<popover::Effect> = places(|places| {
        places
            .values_mut()
            .filter_map(refit)
            .map(|(id, size, margin)| popover::Effect::Resize {
                id,
                size,
                margin: Some(margin),
            })
            .collect()
    });
    if !effects.is_empty() {
        popover::send(effects);
    }
    // Today and the to-do list over it keep the room it leaves them.
    if before != Some((layout.height, layout.mini.as_ref().map(|m| m.height))) {
        crate::column::moved();
    }
}

/// Bar `bar`'s surface takes the pointer (`takes`), or lets it through.
fn takes_pointer(bar: &str, takes: bool) {
    let id = places(|places| {
        let place = places.get_mut(bar)?;
        if place.input == Some(takes) {
            return None;
        }
        place.input = Some(takes);
        Some(place.id)
    });
    if let Some(id) = id {
        popover::send(vec![popover::Effect::Input { id, takes }]);
    }
}

// --- The surface ------------------------------------------------------------------

/// The agents' group of one bar.
#[derive(Debug, Clone)]
pub struct Group {
    /// The forms to draw and their opacity (two while the bar switches).
    pub forms: Vec<(Form, f32)>,
    /// The bar's width as drawn now.
    pub bar_width: f32,
    pub words: Words,
    /// Without glass: the bar's opaque gradient behind.
    pub backdrop: Option<Backdrop>,
    pub bar: String,
    /// The rows move when they change (else they change at once).
    pub motion: bool,
}

/// The bar's opaque gradient, from its top to its bottom: the surface
/// fills with its colour where the group is as it draws, on the bar as
/// tall as it is then (the column moves the group without the shell's
/// view being built again).
#[derive(Debug, Clone, Copy)]
pub struct Backdrop {
    pub top: Color,
    pub bottom: Color,
}

impl Backdrop {
    /// Its colour `y` under the top of a bar `height` tall.
    pub fn at(self, y: f32, height: f32) -> Color {
        mix(self.top, self.bottom, (y / height.max(1.0)).clamp(0.0, 1.0))
    }
}

/// What the surface keeps between draws.
#[derive(Debug, Default)]
pub struct State {
    pointer: Option<Point>,
    hover: Option<Item>,
    last: Option<Point>,
    /// The rows as last laid out, and every agent then (shown or not):
    /// what the next change moves from.
    rows: Vec<Row>,
    agents: HashSet<u64>,
    primed: bool,
    /// The rows moving, by agent, and those leaving for the "+ N" line.
    moving: HashMap<u64, Moving>,
    leaving: Vec<(Row, Moving)>,
    now: Option<Instant>,
    /// The agents waiting as last drawn (none known before the first
    /// draw), and since when the secretary radiates for one that started.
    waiting: Option<HashSet<u64>>,
    radiating: Option<Instant>,
    /// The pandas playing their mood (#649), by agent: its row's, or the
    /// mini item's waiting one.
    bursts: HashMap<u64, Burst>,
    /// The mini item's waiting agent as last drawn, once it drew.
    mini_waiting: Option<u64>,
    primed_mini: bool,
}

/// The secretary's motion at `t` since someone started waiting: her hop
/// (px, up negative) and her rings' share of their way (0 to 1), while
/// they run.
fn secretary_motion(t: Duration) -> (f32, Option<f32>) {
    let hop = if t < BOB * TWICE {
        let share = (t.as_secs_f32() / BOB.as_secs_f32()).fract();
        // Up over its first half, down over its second, each eased.
        let up = if share < 0.5 {
            bezier(EASE_IN_OUT, share * 2.0)
        } else {
            1.0 - bezier(EASE_IN_OUT, (share - 0.5) * 2.0)
        };
        -BOB_UP * up
    } else {
        0.0
    };
    let rings = (t < RING * RINGS_OUT)
        .then(|| bezier(EASE_OUT, (t.as_secs_f32() / RING.as_secs_f32()).fract()));
    (hop, rings)
}

/// How long the secretary moves.
fn secretary_moves(since: Instant, now: Instant) -> bool {
    now.saturating_duration_since(since) < (BOB * TWICE).max(RING * RINGS_OUT)
}

/// A row's motion: from where it was, its tint and opacity then, since
/// when; whether its panda hops.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Moving {
    since: Instant,
    from_y: f32,
    from_tint: Color,
    from_alpha: f32,
    to_alpha: f32,
    hop: bool,
}

impl Moving {
    fn done(&self, now: Instant) -> bool {
        let t = now.saturating_duration_since(self.since);
        t >= SLIDE && t >= FADE && (!self.hop || t >= HOP)
    }

    /// Where a row going to `y` is at `now`, its tint going to `tint`, its
    /// opacity, and its panda's hop (px, up negative).
    fn at(&self, now: Instant, y: f32, tint: Color) -> (f32, Color, f32, f32) {
        let t = now.saturating_duration_since(self.since);
        let share = |d: Duration| (t.as_secs_f32() / d.as_secs_f32()).min(1.0);
        let slid = bezier(SLIDE_CURVE, share(SLIDE));
        let faded = bezier(EASE, share(FADE));
        let hop = if self.hop { hop(share(HOP)) } else { 0.0 };
        (
            self.from_y + (y - self.from_y) * slid,
            mix(self.from_tint, tint, faded),
            self.from_alpha + (self.to_alpha - self.from_alpha) * faded,
            hop,
        )
    }
}

/// The hop at `t` of its 650 ms: its keyframes, eased out between them.
fn hop(t: f32) -> f32 {
    HOP_KEYS.windows(2).find(|k| t <= k[1].0).map_or(0.0, |k| {
        let ((t0, y0), (t1, y1)) = (k[0], k[1]);
        y0 + (y1 - y0) * bezier(EASE_OUT, (t - t0) / (t1 - t0))
    })
}

/// From `a` to `b` by `t`, premultiplied as CSS mixes: a transparent end
/// takes the other's colour.
fn mix(a: Color, b: Color, t: f32) -> Color {
    let alpha = a.a + (b.a - a.a) * t;
    if alpha <= 0.0 {
        return Color::TRANSPARENT;
    }
    let channel = |x: f32, y: f32| (x * a.a + (y * b.a - x * a.a) * t) / alpha;
    Color::from_rgba(
        channel(a.r, b.r),
        channel(a.g, b.g),
        channel(a.b, b.b),
        alpha,
    )
}

/// The dot in place of a row's panda (`[agents] pandas` off, 6.16): its
/// state's colour, the streak's (bar.md §8).
fn dot(look: Look) -> Color {
    match look {
        Look::Waiting => rgb(0xF5B44A),
        Look::Stuck => rgb(0xF2945B),
        Look::ToSee => rgb(0x7EE0A0),
        Look::News => rgb(0xB69CFF),
        Look::Working => rgb(0x6CC287),
        Look::Resting => rgb(0xA9B2C2),
    }
}

/// A row's tint and its word's colour. Under high contrast, no tint: the
/// word in the state's colour at 7:1, and a bar on the row's left
/// ([`CONTRAST_BAR`]).
fn colours(look: Look) -> (Color, Color) {
    let (word, tint) = match look {
        Look::Waiting => WAITING,
        Look::Stuck => STUCK,
        Look::ToSee => TO_SEE,
        Look::News => NEWS,
        Look::Working => (FAINT.get(), Color::TRANSPARENT),
        Look::Resting => (RESTING.get(), Color::TRANSPARENT),
    };
    if crate::theme::contrast() && tint.a > 0.0 {
        return (Ink::Dark(word).get(), Color::TRANSPARENT);
    }
    (word, tint)
}

/// An MCP agent's row's word and tint: the agents' violet while it acts
/// (the mockup's `.agent-row.acting`), dim otherwise.
fn mcp_colours(acting: bool) -> (Color, Color) {
    let agent = crate::mcp::tag::AGENT.get();
    if acting {
        (agent, Color { a: 0.16, ..agent })
    } else {
        (DIM.get(), Color::TRANSPARENT)
    }
}

/// An MCP agent's mark, centred on `at`, `side` across: its initial in
/// the agents' violet on its soft fill, ringed.
fn mcp_mark(f: &mut Frame, at: Point, side: f32, name: &str, shown: f32) {
    let agent = crate::mcp::tag::AGENT.get();
    let radius = side / 2.0 - 0.5;
    let circle = Path::circle(at, radius);
    f.fill(&circle, faded(Color { a: 0.16, ..agent }, shown));
    f.stroke(
        &circle,
        iced::widget::canvas::Stroke::default()
            .with_color(faded(Color { a: 0.55, ..agent }, shown))
            .with_width(1.0),
    );
    let letter = name.chars().next().map(String::from).unwrap_or_default();
    f.fill_text(kit::text_in(
        MANROPE_BOLD,
        said(&letter, at, 12.0, faded(agent, shown), Alignment::Center),
    ));
}

/// Under high contrast, the bar on the left of a row that concerns the
/// user (waiting, stuck, to see), in its state's colour, in place of its
/// tint: 3 px wide (bar.md §13).
const CONTRAST_BAR: f32 = 3.0;

/// The secretary in `mood`: her ear, and the dot on her clipboard, in its
/// colour.
pub fn secretary(mood: Mood) -> &'static svg::Handle {
    use hocus_pandas::Secretary;
    static SECRETARY: OnceLock<[svg::Handle; 3]> = OnceLock::new();
    let drawings = SECRETARY.get_or_init(|| {
        [Secretary::Calm, Secretary::Waiting, Secretary::Stuck]
            .map(|s| svg::Handle::from_memory(s.still().as_bytes()))
    });
    &drawings[mood as usize]
}

/// Her ear's two rings over her head drawn at `head`, `share` of their
/// way out: growing round the ear and fading.
fn ear_rings(f: &mut Frame, head: Rectangle, share: f32, alpha: f32) {
    let unit = head.width / VIEW;
    let scale = 0.9 + 0.35 * share;
    let shown = 0.9 * (1.0 - share) * alpha;
    let centre = Point::new(head.x + EAR.0 * unit, head.y + EAR.1 * unit);
    for (radius, opacity) in RINGS {
        let ring = Path::new(|p| {
            p.arc(canvas::path::Arc {
                center: centre,
                radius: radius * scale * unit,
                start_angle: iced::Radians(RING_FROM.to_radians()),
                end_angle: iced::Radians((RING_FROM + RING_TURN).to_radians()),
            });
        });
        f.stroke(
            &ring,
            canvas::Stroke {
                style: canvas::Style::Solid(faded(EAR_WAITING.get(), shown * opacity)),
                width: RING_WIDTH * scale * unit,
                line_cap: canvas::LineCap::Round,
                ..canvas::Stroke::default()
            },
        );
    }
}

/// Where a row hidden under the "+ N" line is: the place of the row
/// after the last shown in `layout`.
fn hidden_y(layout: &Layout) -> f32 {
    layout
        .rows
        .last()
        .map_or(HEADER + super::GAP, |r| r.rect.y + r.rect.height)
}

impl State {
    /// The group was laid out (`layout`, the agents `agents`) at `now`:
    /// the rows that changed move from where they were, if `motion`.
    fn follow(&mut self, layout: &Layout, agents: HashSet<u64>, now: Instant, motion: bool) {
        let before = std::mem::take(&mut self.rows);
        let known = std::mem::replace(&mut self.agents, agents);
        let primed = std::mem::replace(&mut self.primed, true);
        self.rows.clone_from(&layout.rows);
        if !motion || !primed {
            self.moving.clear();
            self.leaving.clear();
            return;
        }
        // Where each row is drawn now, mid-motion or not.
        let drawn = |row: &Row, moving: Option<&Moving>| {
            let tint = colours(row.look).1;
            moving.map_or((row.rect.y, tint, 1.0), |m| {
                let (y, tint, alpha, _) = m.at(now, row.rect.y, tint);
                (y, tint, alpha)
            })
        };
        let mut moving = HashMap::new();
        let mut changed = Vec::new();
        for row in &layout.rows {
            let was = before.iter().find(|r| r.id == row.id);
            let next = match was {
                Some(was) => {
                    let current = self.moving.get(&row.id).copied();
                    let same_look = was.look == row.look;
                    if (was.rect.y - row.rect.y).abs() < 0.01 && same_look {
                        // Unchanged: its motion, if one, goes on.
                        if let Some(m) = current {
                            moving.insert(row.id, m);
                        }
                        continue;
                    }
                    let (y, tint, alpha) = drawn(was, current.as_ref());
                    Moving {
                        since: now,
                        from_y: y,
                        from_tint: tint,
                        from_alpha: alpha,
                        to_alpha: 1.0,
                        hop: !same_look,
                    }
                }
                // From under the "+ N" line, fading in.
                None if known.contains(&row.id) => Moving {
                    since: now,
                    from_y: hidden_y(layout),
                    from_tint: Color::TRANSPARENT,
                    from_alpha: 0.0,
                    to_alpha: 1.0,
                    hop: false,
                },
                // A new agent: in its place, hopping.
                None => Moving {
                    since: now,
                    from_y: row.rect.y,
                    from_tint: colours(row.look).1,
                    from_alpha: 1.0,
                    to_alpha: 1.0,
                    hop: true,
                },
            };
            changed.push(row.id);
            moving.insert(row.id, next);
        }
        // Those going under the "+ N" line (still running) fade out there.
        self.leaving
            .retain(|(row, _)| !layout.rows.iter().any(|r| r.id == row.id));
        for was in &before {
            if layout.rows.iter().any(|r| r.id == was.id) || !self.agents.contains(&was.id) {
                continue;
            }
            let (y, tint, alpha) = drawn(was, self.moving.get(&was.id));
            let mut gone = was.clone();
            gone.rect.y = hidden_y(layout);
            self.leaving.push((
                gone,
                Moving {
                    since: now,
                    from_y: y,
                    from_tint: tint,
                    from_alpha: alpha,
                    to_alpha: 0.0,
                    hop: false,
                },
            ));
        }
        self.leaving
            .retain(|(row, _)| self.agents.contains(&row.id));
        self.moving = moving;
        if !changed.is_empty() || !self.leaving.is_empty() {
            tracing::debug!(rows = ?changed, leaving = self.leaving.len(), "the agents' rows move");
        }
    }

    /// The agents waiting are `waiting` at `now`: when one started (not
    /// as the group first draws), the secretary radiates, if `motion`.
    fn saw_waiting(&mut self, waiting: HashSet<u64>, now: Instant, motion: bool) {
        let started = self
            .waiting
            .as_ref()
            .is_some_and(|before| waiting.iter().any(|id| !before.contains(id)));
        if started && motion {
            tracing::debug!(waiting = waiting.len(), "the secretary radiates");
            self.radiating = Some(now);
        }
        self.waiting = Some(waiting);
    }

    /// Agent `id`'s panda plays its mood from `now` (#649), unless it
    /// plays already: the pointer coming over it replays it once.
    fn play(&mut self, id: u64, look: Look, now: Instant) {
        if self.bursts.get(&id).is_some_and(|b| b.plays(now)) {
            return;
        }
        tracing::debug!(
            id,
            mood = pandas::mood(look).name(),
            "a panda plays its mood"
        );
        self.bursts.insert(id, Burst::of(pandas::mood(look), now));
    }

    /// The motions over by `now` end; whether one goes on.
    fn tick(&mut self, now: Instant) -> bool {
        self.moving.retain(|_, m| !m.done(now));
        self.leaving.retain(|(_, m)| !m.done(now));
        self.bursts.retain(|_, b| b.plays(now));
        if self
            .radiating
            .is_some_and(|since| !secretary_moves(since, now))
        {
            self.radiating = None;
        }
        !self.moving.is_empty()
            || !self.leaving.is_empty()
            || self.radiating.is_some()
            || !self.bursts.is_empty()
    }
}

impl<Message> canvas::Program<Message> for Group {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        match event {
            iced::Event::Window(window::Event::RedrawRequested(now)) => {
                if STALE.swap(false, std::sync::atomic::Ordering::Relaxed) {
                    laid_out(super::lay_out());
                }
                // The full group, or the mini item (always there with the
                // group).
                let shows = self.alone().is_some() && group_height() > 0.0;
                takes_pointer(&self.bar, shows);
                if let Some(layout) = layout() {
                    let said = super::said();
                    let agents: HashSet<u64> = said.agents.iter().map(|a| a.id).collect();
                    if layout.rows != state.rows || agents != state.agents {
                        state.follow(&layout, agents, *now, self.motion);
                        // A panda whose agent changed state, or came,
                        // plays its mood (#649), its hop starting it.
                        if self.lively() {
                            let hopping: Vec<(u64, Look)> = layout
                                .rows
                                .iter()
                                .filter(|r| {
                                    state
                                        .moving
                                        .get(&r.id)
                                        .is_some_and(|m| m.hop && m.since == *now)
                                })
                                .map(|r| (r.id, r.look))
                                .collect();
                            for (id, look) in hopping {
                                state.play(id, look, *now);
                            }
                        }
                    }
                    // The mini item's waiting panda plays as it arrives.
                    let mini = layout.mini.as_ref().and_then(|m| m.waiting).map(|w| w.0);
                    if let Some(id) = mini
                        && self.lively()
                        && state.primed_mini
                        && state.mini_waiting != Some(id)
                    {
                        state.play(id, Look::Waiting, *now);
                    }
                    state.mini_waiting = mini;
                    state.primed_mini = true;
                    let waiting: HashSet<u64> = said
                        .agents
                        .iter()
                        .filter(|a| Look::of(&a.state) == Look::Waiting)
                        .map(|a| a.id)
                        .collect();
                    if state.waiting.as_ref() != Some(&waiting) {
                        state.saw_waiting(waiting, *now, self.motion && layout.height > 0.0);
                    }
                }
                if state
                    .hover
                    .is_some_and(|h| matches!(h, Item::Agent(id) if !state.agents.contains(&id)))
                {
                    state.hover = None;
                }
                state.now = Some(*now);
                state.tick(*now).then(canvas::Action::request_redraw)
            }
            iced::Event::Mouse(event) => self.mouse(state, event, bounds, cursor),
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) => popover::key(key, *modifiers, text.as_deref())
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
        let colour = self.backdrop.map(|backdrop| {
            // At the group's (or the mini item's) middle, where it is now.
            let form = self.alone().unwrap_or(Form::Full);
            let middle = top(&self.bar, form) + height(form) / 2.0;
            let bar = popover::with(|c| c.bar_height(&self.bar));
            backdrop.at(middle, bar)
        });
        let under = crate::backdrop::under(renderer, bounds.size(), colour);
        let size = bounds.size();
        let items = self.items(size);
        let in_bar: Vec<(Item, Rectangle)> = items
            .iter()
            .map(|(item, r)| {
                (
                    *item,
                    Rectangle::new(self.in_bar(r.position(), size), r.size()),
                )
            })
            .collect();
        log_items(&in_bar);
        let said = crate::a11y::Words::new(self.words.lang);
        let (agents, now_us) = (super::said().agents, super::now_us());
        let spots: Vec<crate::ui::ring::Spot> = in_bar
            .into_iter()
            .map(|(item, rect)| crate::ui::ring::Spot {
                node: node(said, item, &agents, now_us),
                ..crate::ui::ring::Spot::at(item, rect)
            })
            .collect();
        let declared = crate::ui::ring::Declared {
            group: group(said, spots.len(), &agents, now_us, super::rows_shown()),
            spots,
            live: Some(live),
        };
        popover::with(|c| c.declare(&self.bar, AGENTS_NAMESPACE, declared));
        let Some(layout) = layout().filter(|l| l.height > 0.0) else {
            return crate::backdrop::over(under, frame);
        };
        let origin = self.origin(size);
        for &(form, alpha) in &self.forms {
            match form {
                Form::Full => {
                    self.secretary(&mut frame, &layout, origin, state, alpha);
                    self.group(&mut frame, &layout, origin, state.hover, alpha);
                    self.rows(&mut frame, &layout, origin, state, alpha);
                }
                Form::Mini => self.mini(&mut frame, &layout, self.mini_origin(size), state, alpha),
            }
        }
        let mini = self.alone() == Some(Form::Mini);
        for (item, rect) in items {
            if popover::with(|c| c.focused(&self.bar, item)) {
                let radius = match item {
                    _ if mini => MINI_RADIUS,
                    Item::Plug(_) => BUTTON / 2.0,
                    Item::Agent(_) => ROW_RADIUS,
                    Item::Secretary => HEADER_RADIUS,
                    _ => 4.0,
                };
                ring(&mut frame, rect, radius);
            }
        }
        crate::backdrop::over(under, frame)
    }

    fn mouse_interaction(
        &self,
        state: &State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.hover.is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

/// The items' middles in the bar's frame, logged when they change (for
/// tests, which click them).
fn log_items(items: &[(Item, Rectangle)]) {
    static LAST: Mutex<String> = Mutex::new(String::new());
    let said: Vec<String> = items
        .iter()
        .map(|(item, r)| {
            let c = r.center();
            format!("{item:?}@{:.0},{:.0}", c.x, c.y)
        })
        .collect();
    let said = said.join(" ");
    let mut last = LAST.lock().unwrap_or_else(PoisonError::into_inner);
    if *last != said {
        tracing::debug!(items = %said, "the agents group's items");
        last.clone_from(&said);
    }
}

/// The mini form's item's popover: her briefing; Sources on the card,
/// where it proposes to plug the tools found in.
fn mini_item(layout: &Layout) -> Item {
    match layout.shows {
        super::Shows::Card(_) => Item::Sources,
        _ => Item::Secretary,
    }
}

/// A text from `x` with its line centred on `middle`.
fn said(text: &str, at: Point, size: f32, colour: Color, align: Alignment) -> canvas::Text {
    kit::text(text.to_owned(), at, size, colour, align)
}

impl Group {
    /// Whether its pandas play their moods (#649): with motion, and not
    /// in power saver.
    fn lively(&self) -> bool {
        self.motion && pandas::lively()
    }

    /// The look of agent `id`'s panda as shown: its row's, or the mini
    /// item's waiting one.
    fn look_of(&self, id: u64) -> Option<Look> {
        let layout = layout()?;
        layout
            .rows
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.look)
            .or_else(|| {
                layout
                    .mini
                    .as_ref()
                    .and_then(|m| m.waiting)
                    .filter(|w| w.0 == id)
                    .map(|_| Look::Waiting)
            })
    }

    /// The form that shows alone (not switching): its items take the
    /// pointer.
    fn alone(&self) -> Option<Form> {
        match self.forms.as_slice() {
            [(form, _)] => Some(*form),
            _ => None,
        }
    }

    /// The group's top left on the surface.
    fn origin(&self, size: Size) -> Point {
        let left = size.width + 2.0 - self.bar_width;
        Point::new(left + SIDE, top(&self.bar, Form::Full) - top_of(&self.bar))
    }

    /// The mini item's top left on the surface: its button centred in the
    /// bar as wide as it is drawn now.
    fn mini_origin(&self, size: Size) -> Point {
        let centre = size.width + 1.0 - self.bar_width / 2.0;
        Point::new(
            centre - MINI_ITEM / 2.0,
            top(&self.bar, Form::Mini) - top_of(&self.bar),
        )
    }

    /// The group's items and where they are on the surface, in a form
    /// alone: the mini form's is its item (4.7), her briefing's, or Sources'
    /// on the card.
    fn items(&self, size: Size) -> Vec<(Item, Rectangle)> {
        let Some(form) = self.alone() else {
            return Vec::new();
        };
        let Some(layout) = layout().filter(|l| l.height > 0.0) else {
            return Vec::new();
        };
        if form == Form::Mini {
            let Some(mini) = &layout.mini else {
                return Vec::new();
            };
            if size.height + 0.5 < mini.height + 2.0 * ROOM {
                return Vec::new();
            }
            let o = self.mini_origin(size);
            let rect = Rectangle::new(o, Size::new(MINI_ITEM, mini.height));
            let mut items = vec![(mini_item(&layout), rect)];
            // The waiting panda: a button of its own, in front.
            if let Some((id, square)) = mini.waiting {
                let square =
                    Rectangle::new(Point::new(o.x + square.x, o.y + square.y), square.size());
                items.insert(0, (Item::Agent(id), square));
            }
            return items;
        }
        // Not before the surface took the group's new height: a press there
        // would reach what is under it (the resize is the compositor's to
        // apply, a frame or more later).
        if size.height + 0.5 < layout.height + 2.0 * ROOM {
            return Vec::new();
        }
        let o = self.origin(size);
        let at = |r: Rectangle| Rectangle::new(Point::new(o.x + r.x, o.y + r.y), r.size());
        let mut items = vec![(Item::Secretary, at(layout.secretary))];
        if let Some(card) = &layout.card {
            for row in &card.rows {
                items.push((Item::Plug(row.tool), at(row.button)));
            }
            items.push((Item::AnotherTool, at(card.another)));
            items.push((Item::Decline, at(card.decline)));
        }
        for row in &layout.rows {
            // An MCP agent's row opens its journal (docs/design/mcp.md).
            let item = if crate::mcp::is_row(row.id) {
                Item::Journal
            } else {
                Item::Agent(row.id)
            };
            items.push((item, at(row.rect)));
        }
        items
    }

    /// A point of the surface (`size`) in the bar's frame.
    fn in_bar(&self, at: Point, size: Size) -> Point {
        Point::new(at.x - size.width - 1.0, top_of(&self.bar) + at.y)
    }

    /// The mini form's item (4.7) from `o`: lit under the pointer and while
    /// its popover is open; her head (hopping, her rings going out of her
    /// ear, while she radiates), the badge at its bottom right, and how
    /// many work under her.
    fn mini(&self, f: &mut Frame, layout: &Layout, o: Point, state: &State, alpha: f32) {
        let Some(mini) = &layout.mini else {
            return;
        };
        let item = mini_item(layout);
        let rect = Rectangle::new(o, Size::new(MINI_ITEM, mini.height));
        let open = popover::with(|c| c.shows(&self.bar, item));
        match (open, state.hover == Some(item)) {
            (true, _) => f.fill(
                &kit::rounded(rect, MINI_RADIUS),
                faded(ROW_OPEN.get(), alpha),
            ),
            (false, true) => crate::theme::hover(
                f,
                &kit::rounded(rect, MINI_RADIUS),
                faded(ROW_HOVER.get(), alpha),
            ),
            (false, false) => {}
        }
        let now = state.now.unwrap_or_else(Instant::now);
        let (hop, rings) = state.radiating.map_or((0.0, None), |since| {
            secretary_motion(now.saturating_duration_since(since))
        });
        let at = |r: Rectangle| Rectangle::new(Point::new(o.x + r.x, o.y + r.y), r.size());
        let head = at(mini.head);
        let head = Rectangle::new(Point::new(head.x, head.y + hop), head.size());
        f.draw_svg(
            head,
            svg::Svg::new(secretary(layout.mood).clone()).opacity(alpha),
        );
        if let Some(share) = rings {
            ear_rings(f, head, share, alpha);
        }
        // Who waits the longest (#649), on the amber tint, lit under the
        // pointer.
        if let Some((id, square)) = mini.waiting {
            let square = at(square);
            f.fill(
                &kit::rounded(square, MINI_RADIUS),
                faded(WAITING.1, alpha * 0.14 / WAITING.1.a),
            );
            if state.hover == Some(Item::Agent(id)) {
                f.fill(
                    &kit::rounded(square, MINI_RADIUS),
                    faded(ROW_HOVER.get(), alpha),
                );
            }
            let place = Rectangle::new(
                Point::new(square.x + MINI_PANDA_IN, square.y + MINI_PANDA_IN),
                Size::new(MINI_PANDA, MINI_PANDA),
            );
            draw_panda(
                f,
                place,
                Look::Waiting,
                state.bursts.get(&id).copied(),
                now,
                alpha,
            );
        }
        if let Some(badge) = &mini.badge {
            let rect = at(badge.rect);
            let rect = Rectangle::new(Point::new(rect.x, rect.y + hop), rect.size());
            let ringed = Rectangle::new(
                Point::new(rect.x - BADGE_RING, rect.y - BADGE_RING),
                Size::new(
                    rect.width + 2.0 * BADGE_RING,
                    rect.height + 2.0 * BADGE_RING,
                ),
            );
            f.fill(
                &kit::rounded(ringed, BADGE / 2.0 + BADGE_RING),
                faded(BADGE_RING_COLOUR.get(), alpha),
            );
            f.fill(&kit::rounded(rect, BADGE / 2.0), faded(badge.colour, alpha));
            f.fill_text(kit::text_in(
                MANROPE_BOLD,
                said(
                    &badge.text,
                    rect.center(),
                    BADGE_TEXT,
                    faded(BADGE_INK.get(), alpha),
                    Alignment::Center,
                ),
            ));
        }
        if let Some((count, first, second)) = &mini.working {
            let x = o.x + MINI_ITEM / 2.0;
            for (text, middle, colour) in [
                (count.as_str(), *first, DIM),
                (self.words.at_work(), *second, AT_WORK),
            ] {
                f.fill_text(kit::text_in(
                    kit::MANROPE_SEMIBOLD,
                    said(
                        text,
                        Point::new(x, o.y + middle),
                        MINI_TEXT,
                        faded(colour.get(), alpha),
                        Alignment::Center,
                    ),
                ));
            }
        }
    }

    fn group(&self, f: &mut Frame, layout: &Layout, o: Point, hover: Option<Item>, alpha: f32) {
        let lit = |item: Item, normal: Color| {
            if hover == Some(item) {
                TEXT.get()
            } else {
                normal
            }
        };
        if let Some(card) = &layout.card {
            let rect = Rectangle::new(
                Point::new(o.x + card.rect.x, o.y + card.rect.y),
                card.rect.size(),
            );
            f.fill(&kit::rounded(rect, CARD_RADIUS), faded(CARD.get(), alpha));
            let left = rect.x + 10.0;
            for (i, line) in card.intro.iter().enumerate() {
                let mut text = said(
                    line,
                    Point::new(
                        left,
                        o.y + card.intro_top + (i as f32 + 0.5) * INTRO * INTRO_LINE,
                    ),
                    INTRO,
                    faded(DIM.get(), alpha),
                    Alignment::Left,
                );
                text.line_height = iced::widget::text::LineHeight::Relative(INTRO_LINE);
                f.fill_text(text);
            }
            for row in &card.rows {
                let middle = o.y + row.top + row.height / 2.0;
                let tool = row.tool;
                mark(
                    f,
                    Point::new(left, middle - MARK / 2.0),
                    tool.mark(),
                    (MARK, MARK_RADIUS, MARK_LETTER),
                    alpha,
                );
                let first = middle - (row.name.len() as f32 - 1.0) * NAME * LINE / 2.0;
                for (i, line) in row.name.iter().enumerate() {
                    f.fill_text(said(
                        line,
                        Point::new(left + MARK + MARK_GAP, first + i as f32 * NAME * LINE),
                        NAME,
                        faded(TEXT.get(), alpha),
                        Alignment::Left,
                    ));
                }
                let button = Rectangle::new(
                    Point::new(o.x + row.button.x, o.y + row.button.y),
                    row.button.size(),
                );
                let fill = if hover == Some(Item::Plug(tool)) {
                    PLUG_HOVER
                } else {
                    PLUG
                };
                f.fill(
                    &kit::rounded(button, BUTTON / 2.0),
                    faded(fill.get(), alpha),
                );
                f.fill_text(kit::text_in(
                    MANROPE_BOLD,
                    said(
                        self.words.plug(),
                        button.center(),
                        BUTTON_TEXT,
                        faded(ACCENT.get(), alpha),
                        Alignment::Center,
                    ),
                ));
            }
            for (item, rect, word) in [
                (Item::AnotherTool, card.another, self.words.another()),
                (Item::Decline, card.decline, self.words.decline()),
            ] {
                f.fill_text(said(
                    word,
                    Point::new(o.x + rect.x, o.y + rect.center_y()),
                    LINK,
                    faded(lit(item, FAINT.get()), alpha),
                    Alignment::Left,
                ));
            }
        }
        if let Some((first, second, followed)) = &layout.idle {
            f.fill_text(said(
                self.words.no_agent(),
                Point::new(o.x, o.y + first),
                IDLE,
                faded(FAINT.get(), alpha),
                Alignment::Left,
            ));
            f.fill_text(said(
                followed,
                Point::new(o.x, o.y + second),
                FOLLOWED,
                faded(FAINT.get(), alpha),
                Alignment::Left,
            ));
        }
    }

    /// The secretary at the group's head (4.6): her row's tint, her head
    /// (hopping, her rings going out of her ear, while she radiates),
    /// "AGENTS" and the total over her line.
    fn secretary(&self, f: &mut Frame, layout: &Layout, o: Point, state: &State, alpha: f32) {
        let rect = Rectangle::new(
            Point::new(o.x + layout.secretary.x, o.y + layout.secretary.y),
            layout.secretary.size(),
        );
        let (tint, says) = match layout.mood {
            Mood::Calm => (Color::TRANSPARENT, DIM.get()),
            Mood::Waiting => (SECRETARY_WAITING.get(), SAYS_WAITING.get()),
            Mood::Stuck => (SECRETARY_STUCK.get(), SAYS_STUCK.get()),
        };
        if tint.a > 0.0 {
            f.fill(&kit::rounded(rect, HEADER_RADIUS), faded(tint, alpha));
        }
        let open = popover::with(|c| c.shows(&self.bar, Item::Secretary));
        match (open, state.hover == Some(Item::Secretary)) {
            (true, _) => f.fill(
                &kit::rounded(rect, HEADER_RADIUS),
                faded(ROW_OPEN.get(), alpha),
            ),
            (false, true) => crate::theme::hover(
                f,
                &kit::rounded(rect, HEADER_RADIUS),
                faded(ROW_HOVER.get(), alpha),
            ),
            (false, false) => {}
        }
        let now = state.now.unwrap_or_else(Instant::now);
        let (hop, rings) = state.radiating.map_or((0.0, None), |since| {
            secretary_motion(now.saturating_duration_since(since))
        });
        let head = Rectangle::new(Point::new(o.x, o.y + HEAD_TOP + hop), Size::new(HEAD, HEAD));
        f.draw_svg(
            head,
            svg::Svg::new(secretary(layout.mood).clone()).opacity(alpha),
        );
        if let Some(share) = rings {
            ear_rings(f, head, share, alpha);
        }
        let x = o.x + HEAD + HEAD_GAP;
        for (ch, at) in kit::spaced(self.words.header(), |ch| {
            kit::measure(ch, LABEL, MANROPE_BOLD)
        }) {
            f.fill_text(kit::text_in(
                MANROPE_BOLD,
                said(
                    &ch.to_string(),
                    Point::new(x + at - kit::LEFT, o.y + layout.label_middle),
                    LABEL,
                    faded(DIM.get(), alpha),
                    Alignment::Left,
                ),
            ));
        }
        if let Some((total, at)) = &layout.total {
            f.fill_text(kit::text_in(
                kit::MANROPE_SEMIBOLD,
                said(
                    total,
                    Point::new(o.x + at, o.y + layout.label_middle),
                    LABEL,
                    faded(FAINT.get(), alpha),
                    Alignment::Left,
                ),
            ));
        }
        f.fill_text(said(
            &layout.line,
            Point::new(x, o.y + layout.line_middle),
            SAYS,
            faded(says, alpha),
            Alignment::Left,
        ));
    }

    /// The agents' rows, where they are now (moving or not), and the line
    /// for the rest.
    fn rows(&self, f: &mut Frame, layout: &Layout, o: Point, state: &State, alpha: f32) {
        let now = state.now.unwrap_or_else(Instant::now);
        let leaving = state.leaving.iter().map(|(row, m)| (row, Some(m)));
        let rows = layout
            .rows
            .iter()
            .map(|row| (row, state.moving.get(&row.id)));
        for (row, moving) in leaving.chain(rows) {
            let mcp = crate::mcp::is_row(row.id);
            let acting = mcp && crate::mcp::row_acting(row.id);
            let (word, tint) = if mcp {
                mcp_colours(acting)
            } else {
                colours(row.look)
            };
            let (y, tint, shown, hop) = moving.map_or((row.rect.y, tint, 1.0, 0.0), |m| {
                m.at(now, row.rect.y, tint)
            });
            let shown = shown * alpha;
            if shown <= 0.0 {
                continue;
            }
            let rect = Rectangle::new(Point::new(o.x + row.rect.x, o.y + y), row.rect.size());
            let radius = if row.card() {
                CARD_ROW_RADIUS
            } else {
                ROW_RADIUS
            };
            if tint.a > 0.0 {
                f.fill(&kit::rounded(rect, radius), faded(tint, shown));
            }
            // A card's rim, the state's colour at 35 % (none under high
            // contrast: its bar says it).
            if row.card() && !crate::theme::contrast() {
                rim(f, rect, radius, faded(dot(row.look), 0.35 * shown));
            }
            if crate::theme::contrast()
                && matches!(row.look, Look::Waiting | Look::Stuck | Look::ToSee)
            {
                let bar = Rectangle::new(rect.position(), Size::new(CONTRAST_BAR, rect.height));
                f.fill(&kit::rounded(bar, CONTRAST_BAR / 2.0), faded(word, shown));
            }
            let hovered = if mcp {
                state.hover == Some(Item::Journal)
            } else {
                state.hover == Some(Item::Agent(row.id))
            };
            if hovered {
                f.fill(&kit::rounded(rect, radius), faded(ROW_HOVER.get(), shown));
            }
            let middle = rect.center_y();
            let asleep = if row.look == Look::Resting {
                ASLEEP
            } else {
                1.0
            };
            let (left, side) = if row.card() {
                (rect.x + CARD_IN, CARD_PANDA)
            } else {
                (rect.x + ROW_PAD, PANDA)
            };
            if mcp {
                use crate::mcp::row::{GAP, MARK, NAME, WORD};
                // Its mark in the agents' violet, its name over what it
                // does.
                let x = rect.x + ROW_PAD;
                mcp_mark(
                    f,
                    Point::new(x + MARK / 2.0, middle),
                    MARK,
                    &row.name,
                    shown,
                );
                let text = x + MARK + GAP;
                let (name, said) = (NAME * LINE, WORD * LINE);
                let top = middle - (name + said) / 2.0;
                f.fill_text(kit::text_in(
                    kit::MANROPE_SEMIBOLD,
                    said_at(
                        &row.name,
                        Point::new(text, top + name / 2.0),
                        NAME,
                        faded(TEXT.get(), shown),
                    ),
                ));
                f.fill_text(said_at(
                    &row.word,
                    Point::new(text, top + name + said / 2.0),
                    WORD,
                    faded(word, shown),
                ));
                continue;
            }
            if super::pandas() {
                let place = Rectangle::new(
                    Point::new(left, middle - side / 2.0 + hop),
                    Size::new(side, side),
                );
                let burst = state.bursts.get(&row.id).copied();
                draw_panda(f, place, row.look, burst, now, asleep * shown);
            } else {
                // Its dot in the panda's place, hopping as it would.
                f.fill(
                    &Path::circle(Point::new(left + side / 2.0, middle + hop), DOT / 2.0),
                    faded(dot(row.look), asleep * shown),
                );
            }
            if row.card() {
                self.card_text(
                    f,
                    row,
                    Point::new(left + side + CARD_PANDA_GAP, middle),
                    word,
                    shown,
                );
                continue;
            }
            f.fill_text(kit::text_in(
                kit::MANROPE_SEMIBOLD,
                said(
                    &row.name,
                    Point::new(rect.x + ROW_PAD + PANDA + PANDA_GAP, middle),
                    AGENT_NAME,
                    faded(TEXT.get(), shown),
                    Alignment::Left,
                ),
            ));
            f.fill_text(said(
                &row.word,
                Point::new(rect.x + rect.width - ROW_PAD, middle),
                AGENT_WORD,
                faded(word, shown),
                Alignment::Right,
            ));
        }
        if let Some((more, middle)) = &layout.more {
            f.fill_text(said(
                more,
                Point::new(o.x, o.y + middle),
                MORE_TEXT,
                faded(FAINT.get(), alpha),
                Alignment::Left,
            ));
        }
    }

    /// A card's text from `at` (its left, its middle): the name over the
    /// state's word over what the agent asks or why it failed.
    fn card_text(&self, f: &mut Frame, row: &Row, at: Point, word: Color, shown: f32) {
        let name = AGENT_NAME * CARD_NAME_LINE;
        let said = CARD_WORD * CARD_NAME_LINE;
        let note = CARD_NOTE * CARD_NOTE_LINE;
        let tall = name + said + row.note.len() as f32 * note;
        let mut y = at.y - tall / 2.0;
        f.fill_text(kit::text_in(
            kit::MANROPE_SEMIBOLD,
            said_at(
                &row.name,
                Point::new(at.x, y + name / 2.0),
                AGENT_NAME,
                faded(TEXT.get(), shown),
            ),
        ));
        y += name;
        f.fill_text(kit::text_in(
            MANROPE_BOLD,
            said_at(
                &row.word,
                Point::new(at.x, y + said / 2.0),
                CARD_WORD,
                faded(word, shown),
            ),
        ));
        y += said;
        for line in &row.note {
            f.fill_text(said_at(
                line,
                Point::new(at.x, y + note / 2.0),
                CARD_NOTE,
                faded(NOTE_INK.get(), shown),
            ));
            y += note;
        }
    }

    fn mouse<Message>(
        &self,
        state: &mut State,
        event: &mouse::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let size = bounds.size();
        let items = self.items(size);
        let at = pointer(event, bounds, cursor, state.pointer);
        state.pointer = at;
        let over = at.and_then(|at| items.iter().find(|(_, r)| r.contains(at)).copied());
        let rect =
            |item: Rectangle| Rectangle::new(self.in_bar(item.position(), size), item.size());
        match event {
            mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                let at = at?;
                let bar_at = self.in_bar(at, size);
                state.last = Some(bar_at);
                let now = over.map(|(item, _)| item);
                if now == state.hover {
                    if now.is_none() {
                        popover::event(popover::Event::Pointer { at: bar_at });
                    }
                    return None;
                }
                if state.hover.take().is_some() {
                    popover::event(popover::Event::ItemLeave { at: bar_at });
                }
                state.hover = now;
                // The pointer over an agent's panda replays its mood once.
                if let Some(Item::Agent(id)) = now
                    && self.lively()
                    && let Some(look) = self.look_of(id)
                {
                    state.play(id, look, Instant::now());
                }
                if let Some((item, r)) = over {
                    popover::event(popover::Event::ItemEnter {
                        bar: self.bar.clone(),
                        item,
                        rect: rect(r),
                    });
                }
                Some(canvas::Action::request_redraw())
            }
            mouse::Event::CursorLeft => {
                state.hover.take()?;
                let at = state.last.unwrap_or_default();
                popover::event(popover::Event::ItemLeave { at });
                Some(canvas::Action::request_redraw())
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let event = match over {
                    Some((item, r)) => popover::Event::ItemPress {
                        bar: self.bar.clone(),
                        item,
                        rect: rect(r),
                    },
                    None => popover::Event::PressElsewhere,
                };
                popover::event(event);
                Some(canvas::Action::request_redraw().and_capture())
            }
            _ => None,
        }
    }
}

/// A text from `at` (its left, its line's middle), its line `size` high.
fn said_at(text: &str, at: Point, size: f32, colour: Color) -> canvas::Text {
    let mut text = said(text, at, size, colour, Alignment::Left);
    text.line_height = iced::widget::text::LineHeight::Absolute(size.into());
    text
}

/// A 1 px rim inside `rect`, rounded by `radius`.
fn rim(f: &mut Frame, rect: Rectangle, radius: f32, colour: Color) {
    let path = Path::rounded_rectangle(
        Point::new(rect.x + 0.5, rect.y + 0.5),
        Size::new(rect.width - 1.0, rect.height - 1.0),
        (radius - 0.5).into(),
    );
    f.stroke(
        &path,
        canvas::Stroke {
            style: canvas::Style::Solid(colour),
            width: 1.0,
            ..canvas::Stroke::default()
        },
    );
}

/// An agent's panda in the square `place`: the drawing's box, 126 ×
/// 120.5, as wide as the square, in its middle; a frame of its mood while
/// `burst` plays (#649), else still.
pub fn draw_panda(
    f: &mut Frame,
    place: Rectangle,
    look: Look,
    burst: Option<Burst>,
    now: Instant,
    alpha: f32,
) {
    let tall = place.width * PANDA_BOX.1 / PANDA_BOX.0;
    let drawn = Rectangle::new(
        Point::new(place.x, place.y + (place.height - tall) / 2.0),
        Size::new(place.width, tall),
    );
    let drawing = pandas::drawing(pandas::mood(look), burst, now);
    f.draw_svg(drawn, svg::Svg::new(drawing).opacity(alpha));
}

/// A tool's mark at `at`: its letter on its colour, `size` px, rounded.
pub fn mark(
    f: &mut Frame,
    at: Point,
    (letter, colour): (&str, Color),
    (size, radius, letter_size): (f32, f32, f32),
    alpha: f32,
) {
    let rect = Rectangle::new(at, Size::new(size, size));
    f.fill(
        &Path::rounded_rectangle(at, rect.size(), radius.into()),
        faded(colour, alpha),
    );
    f.fill_text(kit::text_in(
        MANROPE_BOLD,
        said(
            letter,
            rect.center(),
            letter_size,
            faded(MARK_INK.get(), alpha),
            Alignment::Center,
        ),
    ));
}

/// An item of the agents' group as screen readers read it: her row with
/// who waits and who works, an agent's row with its state and since when
/// (`now_us` on the agents' clock), the card's buttons.
pub fn node(
    words: crate::a11y::Words,
    item: Item,
    agents: &[hocus_link::AgentShown],
    now_us: u64,
) -> Option<hocus_a11y::Node> {
    use hocus_a11y::{Act, Node, Role};
    let lang = words.lang();
    let name = match item {
        Item::Secretary => {
            let brief = super::Brief::of(agents);
            words.agents(
                brief.waiting.len(),
                brief.stuck.len(),
                brief.working.len(),
                brief.to_see.len(),
            )
        }
        Item::Agent(id) => {
            let agent = agents.iter().find(|a| a.id == id)?;
            let look = super::Look::of(&agent.state);
            let minutes = now_us.saturating_sub(agent.since) / 60_000_000;
            words.agent(&crate::a11y::plain(&agent.name), look, minutes)
        }
        Item::Journal => crate::mcp::Words::new(lang).journal().to_owned(),
        Item::Sources => "Sources".to_owned(),
        Item::AnotherTool => super::Words::new(lang).another().to_owned(),
        Item::Plug(tool) => format!("{} {}", super::Words::new(lang).plug(), tool.title()),
        Item::Decline => super::Words::new(lang).decline().to_owned(),
        _ => return None,
    };
    Some(Node::new(crate::ui::items::key(item), Role::Button, name).act(Act::Click))
}

/// An item of the agents' group as screen readers read it now: how long
/// an agent has waited goes on without the group drawing.
fn live(_bar: &str, words: crate::a11y::Words, item: Item) -> Option<hocus_a11y::Node> {
    node(words, item, &super::said().agents, super::now_us())
}

/// The list the agents' `controls` rows make for screen readers, with
/// the line saying how many more there are than `rows` shows; none for a
/// row alone that hides none.
pub fn group(
    words: crate::a11y::Words,
    controls: usize,
    agents: &[hocus_link::AgentShown],
    now_us: u64,
    rows: usize,
) -> Option<hocus_a11y::Node> {
    use hocus_a11y::{Node, Role};
    if controls <= 1 && agents.len() <= rows {
        return None;
    }
    let lines = super::Lines::of_at_most(agents, now_us, rows);
    let list = Node::new("agents", Role::List, "Agents");
    Some(if lines.hidden > 0 {
        list.child(Node::new(
            "agents-more",
            Role::Label,
            words.other_agents(lines.hidden, lines.hidden_rest),
        ))
    } else {
        list
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_secretary_rings_three_times_and_hops_twice() {
        let at = Duration::from_millis;
        let start = Instant::now();
        // About 2.5 s in all, once.
        assert!(secretary_moves(start, start + at(2489)));
        assert!(!secretary_moves(start, start + at(2490)));
        assert_eq!(secretary_motion(at(2490)), (0.0, None));
        // Two hops, 3 px up at their middles, down between them and after.
        for top in [300, 900] {
            assert!((secretary_motion(at(top)).0 + BOB_UP).abs() < 0.01);
        }
        assert!(secretary_motion(at(600)).0.abs() < 0.01);
        assert!(secretary_motion(at(1500)).0.abs() < 0.01);
        // Three rings, each from its start.
        for start in [0, 830, 1660] {
            assert!(secretary_motion(at(start)).1.is_some_and(|r| r < 0.01));
            assert!(
                secretary_motion(at(start + 829))
                    .1
                    .is_some_and(|r| r > 0.99)
            );
        }
    }

    #[test]
    fn a_panda_plays_its_mood_once_then_lies_still() {
        let now = Instant::now();
        let mut state = State::default();
        state.play(1, Look::Waiting, now);
        assert!(state.tick(now + Duration::from_millis(2000)));
        // Played again while it plays: it is not cut, nor lengthened.
        state.play(1, Look::Waiting, now + Duration::from_millis(2000));
        assert_eq!(state.bursts.get(&1).map(|b| b.since), Some(now));
        // Over by 2.9 s: nothing moves, nothing more is drawn.
        assert!(!state.tick(now + Duration::from_millis(2900)));
        assert!(state.bursts.is_empty());
        // The pointer coming back replays it.
        state.play(1, Look::Waiting, now + Duration::from_secs(5));
        assert!(state.tick(now + Duration::from_secs(6)));
    }
}
