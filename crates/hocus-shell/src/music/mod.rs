//! The music player (s12): what an MPRIS player plays, its tile in the
//! bar's column over the desktops, and its popover
//! ([`crate::popover::music`]).
//!
//! - **Which player**: the one brought forward in the popover, else the
//!   last to start playing, else the first that came. The others are
//!   listed under it in the popover. No player: no tile.
//! - **The data** ([`mpris`]): read on a thread of its own, published here
//!   ([`publish`]); the iced side is told by a message ([`effects`]), and
//!   places the tile and wakes what shows it then. The position is read
//!   with the time it was read at, and counted on from it while playing:
//!   nothing is polled.
//! - **The tile** ([`tile`]): a surface of its own on each bar, kept as
//!   long as it (a surface made later would take the keys, #617), drawing
//!   nothing and taking no pointer without a player. In the full form
//!   only, 14 px over the desktops; the windows' group goes over it.

mod mpris;
pub mod tile;

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_link::{EDGE, Form, MUSIC_NAMESPACE, POPOVER_NAMESPACE};
use iced::{advanced::image, window};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::{
    Message,
    clock::time::Lang,
    column::GAP,
    popover::{self, Effect, Item},
};

/// The tile's height, its 1 px rim included.
pub const TILE: f32 = 50.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Playing,
    Paused,
    Stopped,
}

/// A cover, decoded for the popover's 72 px and the bar's 34 px.
#[derive(Debug, Clone)]
pub struct Cover {
    pub big: image::Handle,
    pub small: image::Handle,
}

/// A player, as its properties last said.
#[derive(Debug, Clone)]
pub struct Player {
    /// Its name on the bus, `org.mpris.MediaPlayer2.…`.
    pub bus: String,
    /// Its `Identity`: "Spotify", "Firefox".
    pub app: String,
    pub title: String,
    pub artist: String,
    /// The album, and the year after a "·" when the player says it.
    pub album: String,
    pub art_url: String,
    pub cover: Option<Cover>,
    pub track: Option<String>,
    pub length: Option<Duration>,
    /// Its position when read, and when that was.
    pub position: Duration,
    pub read_at: Instant,
    pub status: Status,
    pub can_next: bool,
    pub can_previous: bool,
    pub can_seek: bool,
    pub can_control: bool,
    pub can_raise: bool,
    /// Its `Shuffle`, if it has one.
    pub shuffle: Option<bool>,
    /// The count of starts of playing when it last started (0: never).
    pub started: u64,
}

impl Player {
    /// Its position at `now`, counted on from when it was read while it
    /// plays.
    pub fn position_at(&self, now: Instant) -> Duration {
        let at = if self.status == Status::Playing {
            self.position + now.saturating_duration_since(self.read_at)
        } else {
            self.position
        };
        self.length.map_or(at, |length| at.min(length))
    }

    pub fn playing(&self) -> bool {
        self.status == Status::Playing
    }
}

// --- The players ----------------------------------------------------------------

#[derive(Debug, Default)]
struct State {
    players: Vec<Player>,
    /// The player brought forward in the popover, by its bus name.
    chosen: Option<String>,
    places: HashMap<String, Place>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn state<T>(with: impl FnOnce(&mut State) -> T) -> T {
    let mut state = STATE.lock().unwrap_or_else(PoisonError::into_inner);
    with(state.get_or_insert_with(State::default))
}

/// What shows: the player in front, and the others in their order.
#[derive(Debug, Clone, Default)]
pub struct Shown {
    pub front: Option<Player>,
    pub others: Vec<Player>,
}

impl State {
    fn front(&self) -> Option<usize> {
        let chosen = self
            .chosen
            .as_ref()
            .and_then(|c| self.players.iter().position(|p| p.bus == *c));
        chosen.or_else(|| {
            self.players
                .iter()
                .enumerate()
                .max_by_key(|(i, p)| (p.started, std::cmp::Reverse(*i)))
                .map(|(i, _)| i)
        })
    }

    fn shown(&self) -> Shown {
        let front = self.front();
        Shown {
            front: front.map(|i| self.players[i].clone()),
            others: self
                .players
                .iter()
                .enumerate()
                .filter(|(i, _)| Some(*i) != front)
                .map(|(_, p)| p.clone())
                .collect(),
        }
    }
}

/// What shows now.
pub fn shown() -> Shown {
    state(|s| s.shown())
}

/// Whether a player is there.
pub fn any() -> bool {
    state(|s| !s.players.is_empty())
}

/// The thread read the players: the iced side is told.
fn publish(players: Vec<Player>) {
    state(|s| {
        if s.chosen
            .as_ref()
            .is_some_and(|c| !players.iter().any(|p| p.bus == *c))
        {
            s.chosen = None;
        }
        s.players = players;
    });
    if let Some(changes) = CHANGES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
    {
        let _ = changes.send(());
    }
}

static CHANGES: Mutex<Option<UnboundedSender<()>>> = Mutex::new(None);
static TAKEN: Mutex<Option<UnboundedReceiver<()>>> = Mutex::new(None);
static ASKS: Mutex<Option<UnboundedSender<Ask>>> = Mutex::new(None);

/// What the bar asks a player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    PlayPause(String),
    Next(String),
    Previous(String),
    Seek { name: String, to: Duration },
    Shuffle { name: String, on: bool },
    Raise(String),
}

/// Asks the player.
pub fn ask(ask: Ask) {
    tracing::info!(?ask, "the music player is asked");
    if let Some(asks) = ASKS.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
        let _ = asks.send(ask);
    }
}

/// The player in front's name, to ask it.
pub fn front_bus() -> Option<String> {
    shown().front.map(|p| p.bus)
}

/// Brings `bus`'s player forward.
pub fn choose(bus: &str) {
    state(|s| s.chosen = Some(bus.to_owned()));
    changed();
}

/// Starts following the players.
pub fn start() {
    let (sender, asks) = unbounded_channel();
    *ASKS.lock().unwrap_or_else(PoisonError::into_inner) = Some(sender);
    let (changes, taken) = unbounded_channel();
    *CHANGES.lock().unwrap_or_else(PoisonError::into_inner) = Some(changes);
    *TAKEN.lock().unwrap_or_else(PoisonError::into_inner) = Some(taken);
    mpris::start(asks);
}

/// The players' changes, as [`Message::Music`].
pub fn effects() -> impl iced::futures::Stream<Item = Message> {
    use iced::futures::SinkExt;
    iced::stream::channel(4, async |mut output| {
        let Some(mut changes) = TAKEN.lock().unwrap_or_else(PoisonError::into_inner).take() else {
            std::future::pending::<()>().await;
            return;
        };
        while changes.recv().await.is_some() {
            while changes.try_recv().is_ok() {}
            if output.send(Message::Music).await.is_err() {
                return;
            }
        }
    })
}

/// What was last said in the log, to say only a change.
static SAID: Mutex<Option<String>> = Mutex::new(None);

/// The players changed, or the one in front (on the loop): the tiles come,
/// go or draw again, the column moves, the popover follows.
pub fn changed() {
    let shown = shown();
    let words = Words::new(Lang::of_messages());
    let said = words.said(&shown);
    let new = {
        let mut last = SAID.lock().unwrap_or_else(PoisonError::into_inner);
        let new = last.as_deref() != Some(said.as_str());
        *last = Some(said.clone());
        new
    };
    if new {
        tracing::debug!(shows = %said, "the music player");
    }
    if fit_all() {
        crate::column::moved();
    }
    if shown.front.is_none() && popover::with(|c| c.open_item()) == Some(Item::Music) {
        popover::event(popover::Event::Chosen);
    }
    popover::send(vec![Effect::Rebuild, Effect::Wake(MUSIC_NAMESPACE)]);
    popover::changed(Item::Music);
    if popover::with(|c| c.open_item()) == Some(Item::Music) {
        popover::send(vec![Effect::Wake(POPOVER_NAMESPACE)]);
    }
}

// --- The tile's surface -----------------------------------------------------------

/// The surface's room above and under the tile, for the keyboard's ring.
pub const ROOM: f32 = 5.0;
/// The surface's width, as the windows': the bar's inside.
pub const SURFACE_WIDTH: u32 = 194;
/// The tile's left in the surface, and its width.
pub const SIDE: f32 = 11.0;
pub const WIDTH: f32 = 172.0;
/// The surface's height when it shows nothing (not a pixel, #617): on the
/// CPU, where it would show black, the compositor leaves it out.
const IDLE: u32 = hocus_link::IDLE_HEIGHT as u32;

#[derive(Debug, Clone, Default)]
struct Place {
    id: Option<window::Id>,
    form: Option<Form>,
    switching: bool,
    /// The tile's top under the bar's, as last placed.
    top: f32,
    shows: bool,
    asked: Option<Asked>,
    input: Option<bool>,
}

/// A surface's size and its layer-shell margins, as last asked for.
type Asked = ((u32, u32), (i32, i32, i32, i32));

fn margin(top: f32) -> (i32, i32, i32, i32) {
    (EDGE + top.round().max(0.0) as i32, EDGE + 1, 0, 0)
}

/// What the tile takes over bar `bar`'s desktops in `form`, the 14 px over
/// it included: none without a player, nor in the mini form.
pub fn room(bar: &str, form: Form) -> f32 {
    state(|s| {
        let shows = s.places.get(bar).is_some_and(|p| p.shows);
        if shows && form == Form::Full {
            TILE + GAP
        } else {
            0.0
        }
    })
}

/// The tile's top under bar `bar`'s top and whether it shows, for its
/// view.
pub fn placed(bar: &str) -> Option<(f32, bool)> {
    state(|s| s.places.get(bar).map(|p| (p.top, p.shows)))
}

/// Every bar's tile in its place: whether one came or went.
fn fit_all() -> bool {
    let bars: Vec<String> = state(|s| s.places.keys().cloned().collect());
    let any = any();
    let bottoms: Vec<(String, Option<f32>)> = bars
        .into_iter()
        .map(|bar| {
            let height = popover::with(|c| c.known_height(&bar));
            let above = crate::desktops::draw::top_above_bottom_in(&bar, Form::Full);
            let bottom = height.zip(above).map(|(h, a)| h - a);
            (bar, bottom)
        })
        .collect();
    let mut effects = Vec::new();
    let mut moved = false;
    let mut items = Vec::new();
    state(|s| {
        for (bar, bottom) in bottoms {
            let Some(place) = s.places.get_mut(&bar) else {
                continue;
            };
            let full = place.form == Some(Form::Full) && !place.switching;
            let shows = any && full && bottom.is_some();
            if let Some(bottom) = bottom {
                place.top = bottom - GAP - TILE;
            }
            if shows != place.shows {
                moved = true;
                place.shows = shows;
            }
            let item = shows.then(|| (Item::Music, tile::rect_in_bar(place.top)));
            items.push((bar.clone(), item.into_iter().collect::<Vec<_>>()));
            effects.extend(place.fit());
        }
    });
    // The bar's items for the keyboard and the popover's place.
    popover::with(|c| {
        for (bar, items) in items {
            c.set_items(&bar, MUSIC_NAMESPACE, items);
        }
    });
    if !effects.is_empty() {
        popover::send(effects);
    }
    moved
}

impl Place {
    fn fit(&mut self) -> Vec<Effect> {
        let Some(id) = self.id else {
            return Vec::new();
        };
        let asked = if self.shows {
            (
                (SURFACE_WIDTH, (TILE + 2.0 * ROOM).ceil() as u32),
                margin(self.top - ROOM),
            )
        } else {
            ((SURFACE_WIDTH, IDLE), margin(self.top + TILE - ROOM))
        };
        let mut effects = Vec::new();
        if self.asked != Some(asked) {
            self.asked = Some(asked);
            effects.push(Effect::Resize {
                id,
                size: asked.0,
                margin: Some(asked.1),
            });
        }
        if self.input != Some(self.shows) {
            self.input = Some(self.shows);
            effects.push(Effect::Input {
                id,
                takes: self.shows,
            });
        }
        effects
    }
}

/// Bar `bar` opened in `form`: its tile's surface, drawing nothing until
/// the column moves and a player is there.
pub fn bar_opened(bar: &str, form: Form) -> Vec<Effect> {
    let id = window::Id::unique();
    let top = crate::column::start(form);
    let size = (SURFACE_WIDTH, IDLE);
    state(|s| {
        let place = s.places.entry(bar.to_owned()).or_default();
        place.id = Some(id);
        place.form = Some(form);
        place.top = top;
        place.shows = false;
        place.asked = Some((size, margin(top)));
        place.input = None;
    });
    vec![Effect::Layer {
        id,
        bar: bar.to_owned(),
        namespace: MUSIC_NAMESPACE,
        size,
        margin: margin(top),
    }]
}

/// Bar `bar` in `form`, switching or not: the tile follows as the column
/// moves next.
pub fn form(bar: &str, form: Form, switching: bool) {
    state(|s| {
        if let Some(place) = s.places.get_mut(bar) {
            place.form = Some(form);
            place.switching = switching;
        }
    });
    if fit_all() {
        crate::column::moved();
    }
}

/// The column moved: the tile with it.
pub fn moved() {
    fit_all();
}

/// Bar `bar` went: its tile's surface, to remove.
pub fn forget(bar: &str) -> Option<window::Id> {
    state(|s| s.places.remove(bar)).and_then(|p| p.id)
}

/// The bar whose tile's surface is `id`.
pub fn bar_of(id: window::Id) -> Option<String> {
    state(|s| {
        s.places
            .iter()
            .find(|(_, p)| p.id == Some(id))
            .map(|(bar, _)| bar.clone())
    })
}

// --- Words --------------------------------------------------------------------------

/// The music player's words, in the shell's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    fr: bool,
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self {
            fr: lang == Lang::French,
        }
    }

    fn pick(self, fr: &'static str, en: &'static str) -> &'static str {
        if self.fr { fr } else { en }
    }

    pub fn music(self) -> &'static str {
        self.pick("Musique", "Music")
    }

    /// The tile's second line while paused.
    pub fn paused_line(self) -> &'static str {
        self.pick("En pause", "Paused")
    }

    pub fn paused(self) -> &'static str {
        self.pick("en pause", "paused")
    }

    pub fn playing(self) -> &'static str {
        self.pick("en lecture", "playing")
    }

    pub fn other_player(self, n: usize) -> &'static str {
        match n {
            1 => self.pick("Autre lecteur", "Other player"),
            _ => self.pick("Autres lecteurs", "Other players"),
        }
    }

    pub fn shuffle(self) -> &'static str {
        self.pick("Lecture aléatoire", "Shuffle")
    }

    pub fn previous(self) -> &'static str {
        self.pick("Piste précédente", "Previous track")
    }

    pub fn next(self) -> &'static str {
        self.pick("Piste suivante", "Next track")
    }

    /// The play button's name: what it does.
    pub fn play_pause(self, playing: bool) -> &'static str {
        if playing {
            self.pick("Pause", "Pause")
        } else {
            self.pick("Lecture", "Play")
        }
    }

    pub fn open(self, app: &str) -> String {
        format!("{} {app}", self.pick("Afficher", "Show"))
    }

    /// The source in the popover's header: "Spotify", "Spotify · en pause".
    pub fn source(self, player: &Player) -> String {
        if player.playing() {
            player.app.clone()
        } else {
            format!("{} · {}", player.app, self.paused())
        }
    }

    /// What shows, for the log: "Les Passantes — Georges Brassens (Spotify,
    /// playing); others: …".
    pub fn said(self, shown: &Shown) -> String {
        let one = |p: &Player| {
            format!(
                "{} — {} ({}, {})",
                p.title,
                p.artist,
                p.app,
                if p.playing() { "playing" } else { "paused" }
            )
        };
        match &shown.front {
            None => "no player".to_owned(),
            Some(front) => {
                let others: Vec<String> = shown.others.iter().map(one).collect();
                if others.is_empty() {
                    one(front)
                } else {
                    format!("{}; others: {}", one(front), others.join("; "))
                }
            }
        }
    }
}

/// `d` as the popover says it: "1:31", "1:02:03".
pub fn clock(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}
