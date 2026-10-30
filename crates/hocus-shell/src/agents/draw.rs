//! The agents' group's surface, under the clock, as the approved 4.0 and
//! 4.1 mockups draw it (bar.md §4 group 3): the full form only.
//!
//! Its top 14 px under the clock's group, 100 px under the bar's top; as
//! tall as what the group shows, and one pixel, taking no pointer, when it
//! shows nothing (hidden, or the mini form). A change of what it shows
//! resizes it (layer-shell actions) and draws it alone.
//!
//! The secretary heads it (4.6): her head, her ear in the moment's colour,
//! "AGENTS" and the total over her line; her row tinted amber or orange
//! when someone waits or is stuck. A click on her opens her briefing,
//! where "Sources" is. When an agent starts waiting, her ear radiates
//! twice and she hops twice (3.2 s), once; then she holds still. None of it
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
//! or on the CPU.

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
    AGENT_NAME, AGENT_ROW, AGENT_WORD, BUTTON, BUTTON_TEXT, CARD_RADIUS, FOLLOWED, HEAD, HEAD_GAP,
    HEAD_TOP, HEADER, HEADER_RADIUS, IDLE, INTRO, INTRO_LINE, LABEL, LINK, Layout, Look, MARK,
    MARK_GAP, MARK_INK, MARK_LETTER, MARK_RADIUS, MORE_TEXT, MOST_ROWS, Mood, NAME, PANDA,
    PANDA_GAP, ROW_PAD, ROW_RADIUS, Row, SAYS, Words,
};
use crate::{
    clock::motion::bezier,
    levels::draw::{ACCENT, DIM, FAINT, LINE, MANROPE_BOLD, SIDE, TEXT, faded, pointer, rgb, ring},
    popover::{self, Item, kit},
};

/// The surfaces' widths: the bar inside its rim.
const FULL_WIDTH: u32 = 194;
const MINI_WIDTH: u32 = 54;
/// Around the group: the keyboard's ring, 2 px out and 3 px wide.
const ROOM: f32 = 5.0;
/// The group's top under the bar's: 14 px of padding, the clock's 72 px
/// and the 14 px between groups.
pub const TOP: f32 = 100.0;

const CARD: Color = Color::from_rgba8(255, 255, 255, 0.05);
/// The rows' tints and words (4.1), and a row under the pointer.
const WAITING: (Color, Color) = (rgb(0xF5C46E), Color::from_rgba8(245, 180, 74, 0.12));
const STUCK: (Color, Color) = (rgb(0xF7A77A), Color::from_rgba8(242, 148, 91, 0.10));
const TO_SEE: (Color, Color) = (rgb(0x7EE0A0), Color::from_rgba8(108, 194, 135, 0.10));
const RESTING: Color = rgb(0x6A7384);
const ROW_HOVER: Color = Color::from_rgba8(255, 255, 255, 0.06);
/// The pandas' drawings' box (their view box).
const PANDA_BOX: (f32, f32) = (126.0, 120.5);
/// A resting agent's panda, asleep.
const ASLEEP: f32 = 0.55;
/// The rows' motion: the slide, the fades and the tint, the hop.
const SLIDE: Duration = Duration::from_millis(450);
const SLIDE_CURVE: (f32, f32, f32, f32) = (0.2, 0.9, 0.25, 1.0);
const FADE: Duration = Duration::from_millis(300);
const EASE: (f32, f32, f32, f32) = (0.25, 0.1, 0.25, 1.0);
const HOP: Duration = Duration::from_millis(650);
const EASE_OUT: (f32, f32, f32, f32) = (0.0, 0.0, 0.58, 1.0);
/// The hop's keyframes, as the mockup's: up 4 px, down, up 2 px, down.
const HOP_KEYS: [(f32, f32); 5] = [(0.0, 0.0), (0.3, -4.0), (0.6, 0.0), (0.8, -2.0), (1.0, 0.0)];
const PLUG: Color = Color::from_rgba8(108, 194, 135, 0.18);
const PLUG_HOVER: Color = Color::from_rgba8(108, 194, 135, 0.28);
/// The secretary's row (4.6): lit amber at 8 % when someone waits, orange
/// at 7 % when an agent is stuck; `0.08` while her briefing is open.
const SECRETARY_WAITING: Color = Color::from_rgba8(245, 180, 74, 0.08);
const SECRETARY_STUCK: Color = Color::from_rgba8(242, 148, 91, 0.07);
const ROW_OPEN: Color = Color::from_rgba8(255, 255, 255, 0.08);
/// Her line's colours: `text.dim` calm, then the rows' words'.
const SAYS_WAITING: Color = rgb(0xF5C46E);
const SAYS_STUCK: Color = rgb(0xF7A77A);
/// Her ear's amber, its rings' colour.
const EAR_WAITING: Color = rgb(0xF5B44A);
/// When someone starts waiting, as the mockup's keyframes: two rings out
/// of her ear (`hpRing`, 1.4 s `ease-out`, twice: from 90 % to nothing,
/// from 0.9 to 1.25 times their size), and two hops (`hpBob`, 1.6 s
/// `ease-in-out`, twice: 3 px up at its middle).
const RING: Duration = Duration::from_millis(1400);
const BOB: Duration = Duration::from_millis(1600);
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
    asked: Option<(u32, u32)>,
    input: Option<bool>,
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

/// The surface's size in `form` (the full one while switching).
fn size_of(form: Form, switching: bool) -> (u32, u32) {
    let height = group_height();
    match (switching, form) {
        _ if height <= 0.0 => (
            if form == Form::Mini && !switching {
                MINI_WIDTH
            } else {
                FULL_WIDTH
            },
            1,
        ),
        (false, Form::Mini) => (MINI_WIDTH, 1),
        _ => (FULL_WIDTH, (height + 2.0 * ROOM).ceil() as u32),
    }
}

/// Its layer-shell margins: under the clock.
pub fn margin() -> (i32, i32, i32, i32) {
    (EDGE + (TOP - ROOM) as i32, EDGE + 1, 0, 0)
}

/// Bar `bar`'s surface `id` opens in `form`: its size and margins.
pub fn opened(bar: &str, id: window::Id, form: Form) -> ((u32, u32), (i32, i32, i32, i32)) {
    let size = size_of(form, false);
    places(|places| {
        places.insert(
            bar.to_owned(),
            Place {
                id,
                form,
                switching: false,
                asked: Some(size),
                input: None,
            },
        );
    });
    (size, margin())
}

pub fn forget(bar: &str) {
    places(|places| places.remove(bar));
}

fn refit(place: &mut Place) -> Option<crate::network::draw::Placed> {
    let size = size_of(place.form, place.switching);
    if place.asked == Some(size) {
        return None;
    }
    place.asked = Some(size);
    Some((place.id, size, margin()))
}

/// Bar `bar` in `form`, switching or not: the surface's new size, if it
/// changes.
pub fn form(bar: &str, form: Form, switching: bool) -> Option<crate::network::draw::Placed> {
    places(|places| {
        let place = places.get_mut(bar)?;
        place.form = form;
        place.switching = switching;
        refit(place)
    })
}

/// The group was laid out again: every bar's surface to its height.
pub fn laid_out(layout: Layout) {
    *LAYOUT.lock().unwrap_or_else(PoisonError::into_inner) = Some(layout);
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
    /// Without glass: the bar's opaque colour behind.
    pub backdrop: Option<Color>,
    pub bar: String,
    /// The rows move when they change (else they change at once).
    pub motion: bool,
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
    let rings = (t < RING * TWICE)
        .then(|| bezier(EASE_OUT, (t.as_secs_f32() / RING.as_secs_f32()).fract()));
    (hop, rings)
}

/// How long the secretary moves.
fn secretary_moves(since: Instant, now: Instant) -> bool {
    now.saturating_duration_since(since) < (BOB * TWICE).max(RING * TWICE)
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

/// A row's tint and its word's colour.
fn colours(look: Look) -> (Color, Color) {
    match look {
        Look::Waiting => WAITING,
        Look::Stuck => STUCK,
        Look::ToSee => TO_SEE,
        Look::Working => (FAINT, Color::TRANSPARENT),
        Look::Resting => (RESTING, Color::TRANSPARENT),
    }
}

/// The panda for `look`: the validated drawings, one still mood each.
fn panda(look: Look) -> &'static svg::Handle {
    static PANDAS: OnceLock<[svg::Handle; 5]> = OnceLock::new();
    let pandas = PANDAS.get_or_init(|| {
        [
            include_bytes!("../../assets/pandas/waiting.svg").as_slice(),
            include_bytes!("../../assets/pandas/stuck.svg").as_slice(),
            include_bytes!("../../assets/pandas/done.svg").as_slice(),
            include_bytes!("../../assets/pandas/working.svg").as_slice(),
            include_bytes!("../../assets/pandas/resting.svg").as_slice(),
        ]
        .map(svg::Handle::from_memory)
    });
    &pandas[look as usize]
}

/// The secretary in `mood`: her ear, and the dot on her clipboard, in its
/// colour.
pub fn secretary(mood: Mood) -> &'static svg::Handle {
    static SECRETARY: OnceLock<[svg::Handle; 3]> = OnceLock::new();
    let drawings = SECRETARY.get_or_init(|| {
        [
            include_bytes!("../../assets/pandas/secretary-calm.svg").as_slice(),
            include_bytes!("../../assets/pandas/secretary-waiting.svg").as_slice(),
            include_bytes!("../../assets/pandas/secretary-stuck.svg").as_slice(),
        ]
        .map(svg::Handle::from_memory)
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
                style: canvas::Style::Solid(faded(EAR_WAITING, shown * opacity)),
                width: RING_WIDTH * scale * unit,
                line_cap: canvas::LineCap::Round,
                ..canvas::Stroke::default()
            },
        );
    }
}

/// Where a row hidden under the "+ N" line is: the place of the sixth.
fn hidden_y() -> f32 {
    HEADER + super::GAP + MOST_ROWS as f32 * AGENT_ROW
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
                    from_y: hidden_y(),
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
            gone.rect.y = hidden_y();
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

    /// The motions over by `now` end; whether one goes on.
    fn tick(&mut self, now: Instant) -> bool {
        self.moving.retain(|_, m| !m.done(now));
        self.leaving.retain(|(_, m)| !m.done(now));
        if self
            .radiating
            .is_some_and(|since| !secretary_moves(since, now))
        {
            self.radiating = None;
        }
        !self.moving.is_empty() || !self.leaving.is_empty() || self.radiating.is_some()
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
                let shows = self.full() && group_height() > 0.0;
                takes_pointer(&self.bar, shows);
                if let Some(layout) = layout() {
                    let said = super::said();
                    let agents: HashSet<u64> = said.agents.iter().map(|a| a.id).collect();
                    if layout.rows != state.rows || agents != state.agents {
                        state.follow(&layout, agents, *now, self.motion);
                    }
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
        if let Some(colour) = self.backdrop {
            frame.fill_rectangle(Point::ORIGIN, bounds.size(), colour);
        }
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
        popover::with(|c| c.set_items(&self.bar, AGENTS_NAMESPACE, in_bar));
        let Some(layout) = layout().filter(|l| l.height > 0.0) else {
            return vec![frame.into_geometry()];
        };
        let origin = self.origin(size);
        for &(form, alpha) in &self.forms {
            if form == Form::Full {
                self.secretary(&mut frame, &layout, origin, state, alpha);
                self.group(&mut frame, &layout, origin, state.hover, alpha);
                self.rows(&mut frame, &layout, origin, state, alpha);
            }
        }
        for (item, rect) in items {
            if popover::with(|c| c.focused(&self.bar, item)) {
                let radius = match item {
                    Item::Plug(_) => BUTTON / 2.0,
                    Item::Agent(_) => ROW_RADIUS,
                    Item::Secretary => HEADER_RADIUS,
                    _ => 4.0,
                };
                ring(&mut frame, rect, radius);
            }
        }
        vec![frame.into_geometry()]
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

/// A text from `x` with its line centred on `middle`.
fn said(text: &str, at: Point, size: f32, colour: Color, align: Alignment) -> canvas::Text {
    kit::text(text.to_owned(), at, size, colour, align)
}

impl Group {
    /// Whether the full form shows alone (not switching): the group takes
    /// the pointer.
    fn full(&self) -> bool {
        matches!(self.forms.as_slice(), [(Form::Full, _)])
    }

    /// The group's top left on the surface.
    fn origin(&self, size: Size) -> Point {
        let left = size.width + 2.0 - self.bar_width;
        Point::new(left + SIDE, ROOM)
    }

    /// The group's items and where they are on the surface, in the full
    /// form alone.
    fn items(&self, size: Size) -> Vec<(Item, Rectangle)> {
        if !self.full() {
            return Vec::new();
        }
        let Some(layout) = layout().filter(|l| l.height > 0.0) else {
            return Vec::new();
        };
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
            items.push((Item::Agent(row.id), at(row.rect)));
        }
        items
    }

    /// A point of the surface (`size`) in the bar's frame.
    fn in_bar(&self, at: Point, size: Size) -> Point {
        Point::new(at.x - size.width - 1.0, TOP - ROOM + at.y)
    }

    fn group(&self, f: &mut Frame, layout: &Layout, o: Point, hover: Option<Item>, alpha: f32) {
        let lit = |item: Item, normal: Color| if hover == Some(item) { TEXT } else { normal };
        if let Some(card) = &layout.card {
            let rect = Rectangle::new(
                Point::new(o.x + card.rect.x, o.y + card.rect.y),
                card.rect.size(),
            );
            f.fill(&kit::rounded(rect, CARD_RADIUS), faded(CARD, alpha));
            let left = rect.x + 10.0;
            for (i, line) in card.intro.iter().enumerate() {
                let mut text = said(
                    line,
                    Point::new(
                        left,
                        o.y + card.intro_top + (i as f32 + 0.5) * INTRO * INTRO_LINE,
                    ),
                    INTRO,
                    faded(DIM, alpha),
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
                        faded(TEXT, alpha),
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
                f.fill(&kit::rounded(button, BUTTON / 2.0), faded(fill, alpha));
                f.fill_text(kit::text_in(
                    MANROPE_BOLD,
                    said(
                        self.words.plug(),
                        button.center(),
                        BUTTON_TEXT,
                        faded(ACCENT, alpha),
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
                    faded(lit(item, FAINT), alpha),
                    Alignment::Left,
                ));
            }
        }
        if let Some((first, second, followed)) = &layout.idle {
            f.fill_text(said(
                self.words.no_agent(),
                Point::new(o.x, o.y + first),
                IDLE,
                faded(FAINT, alpha),
                Alignment::Left,
            ));
            f.fill_text(said(
                followed,
                Point::new(o.x, o.y + second),
                FOLLOWED,
                faded(FAINT, alpha),
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
            Mood::Calm => (Color::TRANSPARENT, DIM),
            Mood::Waiting => (SECRETARY_WAITING, SAYS_WAITING),
            Mood::Stuck => (SECRETARY_STUCK, SAYS_STUCK),
        };
        if tint.a > 0.0 {
            f.fill(&kit::rounded(rect, HEADER_RADIUS), faded(tint, alpha));
        }
        let open = popover::with(|c| c.shows(&self.bar, Item::Secretary));
        let lit = match (open, state.hover == Some(Item::Secretary)) {
            (true, _) => Some(ROW_OPEN),
            (false, true) => Some(ROW_HOVER),
            (false, false) => None,
        };
        if let Some(lit) = lit {
            f.fill(&kit::rounded(rect, HEADER_RADIUS), faded(lit, alpha));
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
                    faded(DIM, alpha),
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
                    faded(FAINT, alpha),
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
            let (word, tint) = colours(row.look);
            let (y, tint, shown, hop) = moving.map_or((row.rect.y, tint, 1.0, 0.0), |m| {
                m.at(now, row.rect.y, tint)
            });
            let shown = shown * alpha;
            if shown <= 0.0 {
                continue;
            }
            let rect = Rectangle::new(Point::new(o.x + row.rect.x, o.y + y), row.rect.size());
            if tint.a > 0.0 {
                f.fill(&kit::rounded(rect, ROW_RADIUS), faded(tint, shown));
            }
            if state.hover == Some(Item::Agent(row.id)) {
                f.fill(&kit::rounded(rect, ROW_RADIUS), faded(ROW_HOVER, shown));
            }
            let middle = rect.center_y();
            let asleep = if row.look == Look::Resting {
                ASLEEP
            } else {
                1.0
            };
            // The drawing's box, 126 × 120.5, 30 px wide in its 30 px
            // square (iced scales a drawing to the rectangle given).
            let tall = PANDA * PANDA_BOX.1 / PANDA_BOX.0;
            let place = Rectangle::new(
                Point::new(rect.x + ROW_PAD, middle - tall / 2.0 + hop),
                Size::new(PANDA, tall),
            );
            f.draw_svg(
                place,
                svg::Svg::new(panda(row.look).clone()).opacity(asleep * shown),
            );
            f.fill_text(kit::text_in(
                kit::MANROPE_SEMIBOLD,
                said(
                    &row.name,
                    Point::new(rect.x + ROW_PAD + PANDA + PANDA_GAP, middle),
                    AGENT_NAME,
                    faded(TEXT, shown),
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
                faded(FAINT, alpha),
                Alignment::Left,
            ));
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
            faded(MARK_INK, alpha),
            Alignment::Center,
        ),
    ));
}
