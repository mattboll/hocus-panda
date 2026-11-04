//! What is in use, and the bar's sign that says so (5.12, variant A; 5.11
//! and 5.10 before it): the microphone, the camera and the screen.
//!
//! - **The microphone**: PipeWire's `Stream/Input/Audio` nodes linked to a
//!   source, from the sound's `pw-dump` ([`crate::levels::sound`]), which
//!   writes the graph's changes as they come: nothing is polled. Grey while
//!   the default source is muted.
//! - **The camera**: `Stream/Input/Video` nodes linked to a camera's node,
//!   from the same graph; and the processes holding `/dev/video*` open,
//!   read from `/proc` off the loop ([`cameras`]), only while a camera's
//!   node exists and at most once a second.
//! - **The screen**: what the compositor says is shared through the
//!   desktop portal (`shares`, 5.11) and its recording (`recording`, 5.10),
//!   which join the sign as its screen, their lines and their buttons in
//!   its popover.
//!
//! Each use shows from the moment it is seen and stays [`LINGER`] after it
//! ends, so that a short one is not missed: a thread of its own wakes at
//! the end of the linger, and sleeps without one (`linger`). The times are
//! the monotonic clock's.
//!
//! The sign shows under the clock while something is in use, in both
//! forms: a surface of its own on each bar, opened when the first use
//! starts and removed when the last one has lingered; the agents' group
//! moves under it ([`room`]). One tile per kind in use ([`draw`]); hovering
//! it (150 ms) or a click opens its popover ([`crate::popover::in_use`]).

pub mod cameras;
pub mod draw;

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError, mpsc},
    time::{Duration, Instant},
};

use hocus_link::{EDGE, Form, IN_USE_NAMESPACE, RecordingShown, ShareShown, SharedThing};
use iced::window;

use crate::{
    levels::icons::Icon,
    popover::{self, Effect, Item},
};

/// A use stays shown this long after it ends.
pub const LINGER: Duration = Duration::from_secs(3);

/// What the sign shows a tile for, in its order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Microphone,
    Camera,
    Screen,
}

/// An application using the microphone or the camera.
#[derive(Debug, Clone, PartialEq)]
pub struct Media {
    /// What tells it from another of the same kind: its application's key
    /// (its streams taken together), or its process's.
    pub key: String,
    /// The name people know it by.
    pub app: String,
    pub icon: Option<Icon>,
    /// The names its windows' application ids may go by, lower case: its
    /// sandbox's app id, its own id, its program, its name.
    pub ids: Vec<String>,
    /// Its stream's `media.name`, if it says one.
    pub media: Option<String>,
}

/// A use as the sign follows it: since when, and when it ended (it then
/// lingers).
#[derive(Debug, Clone, PartialEq)]
pub struct Seen<T> {
    pub what: T,
    pub since: Instant,
    pub ended: Option<Instant>,
}

impl<T> Seen<T> {
    pub fn live(&self) -> bool {
        self.ended.is_none()
    }
}

/// The recording under way, as the compositor told it, and when.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rec {
    pub shown: RecordingShown,
    pub at: Instant,
}

impl Rec {
    /// How long it has recorded at `now`.
    pub fn elapsed(&self, now: Instant) -> Duration {
        let told = Duration::from_millis(self.shown.elapsed);
        if self.shown.paused {
            told
        } else {
            told + now.saturating_duration_since(self.at)
        }
    }
}

/// What is in use, and what lingers.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Uses {
    pub microphone: Vec<Seen<Media>>,
    pub camera: Vec<Seen<Media>>,
    pub shares: Vec<Seen<ShareShown>>,
    pub recording: Option<Seen<Rec>>,
    /// The default source is muted: the microphone's tile is grey.
    pub muted: bool,
    /// When the sign last appeared: it pops in from then.
    pub appeared: Option<Instant>,
}

impl Uses {
    /// The kinds shown, in the sign's order.
    pub fn kinds(&self) -> Vec<Kind> {
        let mut kinds = Vec::new();
        if !self.microphone.is_empty() {
            kinds.push(Kind::Microphone);
        }
        if !self.camera.is_empty() {
            kinds.push(Kind::Camera);
        }
        if !self.shares.is_empty() || self.recording.is_some() {
            kinds.push(Kind::Screen);
        }
        kinds
    }

    pub fn any(&self) -> bool {
        !self.kinds().is_empty()
    }

    /// The shares still on (not lingering).
    pub fn live_shares(&self) -> impl Iterator<Item = &ShareShown> {
        self.shares.iter().filter(|s| s.live()).map(|s| &s.what)
    }

    /// The recording still on.
    pub fn live_recording(&self) -> Option<&Rec> {
        self.recording
            .as_ref()
            .filter(|r| r.live())
            .map(|r| &r.what)
    }

    /// When the next use to have lingered goes.
    pub fn deadline(&self) -> Option<Instant> {
        let ends = self
            .microphone
            .iter()
            .chain(&self.camera)
            .filter_map(|s| s.ended)
            .chain(self.shares.iter().filter_map(|s| s.ended))
            .chain(self.recording.iter().filter_map(|r| r.ended));
        ends.min().map(|end| end + LINGER)
    }

    /// The uses that have lingered long enough at `now` go; when the last
    /// of them had ended, if any went.
    fn prune(&mut self, now: Instant) -> Option<Instant> {
        let mut last = None;
        let mut keep = |ended: Option<Instant>| match ended {
            Some(end) if end + LINGER <= now => {
                last = last.max(Some(end));
                false
            }
            _ => true,
        };
        self.microphone.retain(|s| keep(s.ended));
        self.camera.retain(|s| keep(s.ended));
        self.shares.retain(|s| keep(s.ended));
        if self.recording.as_ref().is_some_and(|r| !keep(r.ended)) {
            self.recording = None;
        }
        last
    }
}

/// `seen` follows what is `now` in use (`live`, each with its key): a new
/// one is seen from `at`, one gone ends at `at` (and lingers), one back
/// before it went is on again, as it was since.
fn follow<T, K: PartialEq>(
    seen: &mut Vec<Seen<T>>,
    live: Vec<T>,
    key: impl Fn(&T) -> K,
    at: Instant,
) {
    for kept in seen.iter_mut() {
        if kept.ended.is_none() && !live.iter().any(|l| key(l) == key(&kept.what)) {
            kept.ended = Some(at);
        }
    }
    for what in live {
        match seen.iter_mut().find(|s| key(&s.what) == key(&what)) {
            Some(kept) => {
                kept.what = what;
                kept.ended = None;
            }
            None => seen.push(Seen {
                what,
                since: at,
                ended: None,
            }),
        }
    }
}

/// What the sources last said, and what the sign shows of it.
#[derive(Debug, Default)]
struct State {
    uses: Uses,
    /// The cameras' users: PipeWire's streams, and the processes holding
    /// a device open.
    camera_streams: Vec<Media>,
    camera_holders: Vec<Media>,
}

impl State {
    fn cameras(&self) -> Vec<Media> {
        let mut all = self.camera_streams.clone();
        for holder in &self.camera_holders {
            if !all.iter().any(|m| m.app == holder.app) {
                all.push(holder.clone());
            }
        }
        all
    }
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn state<T>(with: impl FnOnce(&mut State) -> T) -> T {
    let mut state = STATE.lock().unwrap_or_else(PoisonError::into_inner);
    with(state.get_or_insert_with(State::default))
}

/// What is in use now.
pub fn snapshot() -> Uses {
    state(|s| s.uses.clone())
}

/// The room the sign takes under the clock in `form`, the 14 px after it
/// included: none when nothing is in use.
pub fn room(form: Form) -> f32 {
    let kinds = snapshot().kinds().len();
    if kinds > 0 {
        draw::size(form, kinds).height + draw::AFTER
    } else {
        0.0
    }
}

/// The uses change with `change` at `now` (the monotonic clock's), those
/// that lingered enough go: the sign comes, changes or goes on every bar,
/// its popover follows, and the linger's thread is told when to wake.
fn update(now: Instant, change: impl FnOnce(&mut State)) {
    let (before, after, lingered) = state(|s| {
        let before = s.uses.clone();
        change(s);
        let lingered = s.uses.prune(now);
        if before.kinds().is_empty() && s.uses.any() {
            s.uses.appeared = Some(now);
        }
        (before, s.uses.clone(), lingered)
    });
    linger::wake_at(after.deadline());
    if before == after {
        return;
    }
    let (was, is) = (before.kinds(), after.kinds());
    if is.is_empty() && !was.is_empty() {
        draw::unsaid();
        tracing::debug!(
            lingered_ms = lingered.map(|end| now.saturating_duration_since(end).as_millis()),
            "the in-use sign goes"
        );
    } else if was != is || before.muted != after.muted {
        tracing::debug!(kinds = ?is, muted = after.muted, "the in-use sign shows");
    }
    let live = |u: &Uses| u.live_shares().count() > 0;
    if live(&before) != live(&after) {
        crate::notifications::sharing(live(&after));
    }
    let mut effects = Vec::new();
    if was.is_empty() != is.is_empty() {
        // The sign comes or goes on every bar, the agents move.
        places(|places| {
            for (bar, place) in places.iter_mut() {
                effects.extend(place.show(bar, !is.is_empty()));
            }
        });
        crate::agents::draw::moved();
        if is.is_empty() && popover::with(|c| c.open_item()) == Some(Item::InUse) {
            popover::event(popover::Event::Chosen);
        }
    } else if was.len() != is.len() {
        // Another size: its surfaces and the agents follow.
        effects.extend(places(|places| {
            places
                .values()
                .filter_map(|place| {
                    let id = place.id?;
                    let (size, margin) = placement(place.form, place.switching);
                    Some(Effect::Resize {
                        id,
                        size,
                        margin: Some(margin),
                    })
                })
                .collect::<Vec<_>>()
        }));
        crate::agents::draw::moved();
    }
    effects.push(Effect::Wake(IN_USE_NAMESPACE));
    popover::send(effects);
    popover::changed(Item::InUse);
}

/// The sound's graph: who uses the microphone (their streams linked to a
/// source), whether the default source is muted, and who uses the
/// cameras through PipeWire.
pub fn set_media(microphone: Vec<Media>, muted: bool, camera: Vec<Media>) {
    let now = Instant::now();
    update(now, |s| {
        s.uses.muted = muted;
        follow(&mut s.uses.microphone, microphone, |m| m.key.clone(), now);
        s.camera_streams = camera;
        let all = s.cameras();
        follow(&mut s.uses.camera, all, |m| m.key.clone(), now);
    });
}

/// The processes holding a camera's device open ([`cameras`]).
pub fn set_camera_holders(holders: Vec<Media>) {
    let now = Instant::now();
    update(now, |s| {
        s.camera_holders = holders;
        let all = s.cameras();
        follow(&mut s.uses.camera, all, |m| m.key.clone(), now);
    });
}

/// The compositor's `recording` (`Some`) and `recording-end`.
pub fn set_recording(shown: Option<RecordingShown>) {
    let now = Instant::now();
    tracing::debug!(recording = ?shown, "the recording");
    update(now, |s| {
        let live: Vec<Rec> = shown
            .map(|shown| Rec { shown, at: now })
            .into_iter()
            .collect();
        let mut seen: Vec<Seen<Rec>> = s.uses.recording.take().into_iter().collect();
        follow(&mut seen, live, |r| r.shown.id, now);
        // One at a time: the one on, else the newest.
        s.uses.recording = seen.into_iter().max_by_key(|r| (r.live(), r.since));
    });
}

/// The compositor's `shares`.
pub fn set_shares(shares: Vec<ShareShown>) {
    let now = Instant::now();
    tracing::debug!(
        shares = ?shares.iter().map(|s| (s.id, s.app.clone())).collect::<Vec<_>>(),
        "what is shared"
    );
    update(now, |s| follow(&mut s.uses.shares, shares, |s| s.id, now));
}

/// The thread that wakes when a use has lingered enough: told each
/// deadline, it sleeps until it, or without one until told one.
mod linger {
    use super::{Instant, Mutex, PoisonError, mpsc};

    static WAKE: Mutex<Option<mpsc::Sender<Option<Instant>>>> = Mutex::new(None);

    /// The next use to go goes at `at` (none: nothing lingers).
    pub fn wake_at(at: Option<Instant>) {
        let mut sender = WAKE.lock().unwrap_or_else(PoisonError::into_inner);
        if sender.is_none() {
            if at.is_none() {
                return;
            }
            let (tx, rx) = mpsc::channel();
            let started = std::thread::Builder::new()
                .name("in use: linger".to_owned())
                .spawn(move || run(&rx));
            match started {
                Ok(_) => *sender = Some(tx),
                Err(err) => {
                    tracing::warn!(%err, "cannot time what is in use: it stays");
                    return;
                }
            }
        }
        if let Some(tx) = sender.as_ref() {
            let _ = tx.send(at);
        }
    }

    fn run(told: &mpsc::Receiver<Option<Instant>>) {
        let mut deadline: Option<Instant> = None;
        loop {
            let next = match deadline {
                None => told.recv().ok(),
                Some(at) => match told.recv_timeout(at.saturating_duration_since(Instant::now())) {
                    Ok(next) => Some(next),
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        // What lingered goes; the next deadline comes back.
                        deadline = None;
                        super::update(Instant::now(), |_| {});
                        continue;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => None,
                },
            };
            match next {
                Some(at) => deadline = at,
                None => return,
            }
        }
    }
}

/// The sign's surface on a bar.
#[derive(Debug, Clone, Copy)]
struct Place {
    id: Option<window::Id>,
    form: Form,
    switching: bool,
}

impl Place {
    /// The sign shows on `bar` (`shown`), or goes: what opens or removes it.
    fn show(&mut self, bar: &str, shown: bool) -> Vec<Effect> {
        match (self.id, shown) {
            (None, true) => {
                let id = window::Id::unique();
                self.id = Some(id);
                let (size, margin) = placement(self.form, self.switching);
                popover::with(|c| c.set_in_use(bar, Some(id)));
                vec![Effect::Layer {
                    id,
                    bar: bar.to_owned(),
                    namespace: IN_USE_NAMESPACE,
                    size,
                    margin,
                }]
            }
            (Some(id), false) => {
                self.id = None;
                popover::with(|c| c.set_in_use(bar, None));
                vec![Effect::Remove(id)]
            }
            _ => Vec::new(),
        }
    }
}

static PLACES: Mutex<Option<HashMap<String, Place>>> = Mutex::new(None);

fn places<T>(with: impl FnOnce(&mut HashMap<String, Place>) -> T) -> T {
    let mut places = PLACES.lock().unwrap_or_else(PoisonError::into_inner);
    with(places.get_or_insert_with(HashMap::new))
}

/// The surface's size and its margins in `form` (both forms' places while
/// the bar switches).
fn placement(form: Form, switching: bool) -> ((u32, u32), (i32, i32, i32, i32)) {
    let ((w, h), top) = draw::placement(form, switching, snapshot().kinds().len());
    ((w, h), (EDGE + top.round() as i32, EDGE + 1, 0, 0))
}

/// Bar `bar` opened in `form`: its sign, if something is in use.
pub fn bar_opened(bar: &str, form: Form) -> Vec<Effect> {
    let shown = snapshot().any();
    places(|places| {
        let place = places.entry(bar.to_owned()).or_insert(Place {
            id: None,
            form,
            switching: false,
        });
        place.form = form;
        place.show(bar, shown)
    })
}

/// Bar `bar` in `form`, switching or not: its sign's size and place.
pub fn form(bar: &str, form: Form, switching: bool) -> Vec<Effect> {
    places(|places| {
        let Some(place) = places.get_mut(bar) else {
            return Vec::new();
        };
        if place.form == form && place.switching == switching {
            return Vec::new();
        }
        place.form = form;
        place.switching = switching;
        let Some(id) = place.id else {
            return Vec::new();
        };
        let (size, margin) = placement(form, switching);
        vec![Effect::Resize {
            id,
            size,
            margin: Some(margin),
        }]
    })
}

/// Bar `bar` went: its sign's surface, to remove.
pub fn forget(bar: &str) -> Option<window::Id> {
    places(|places| places.remove(bar)).and_then(|p| p.id)
}

/// The bar whose sign's surface is `id`.
pub fn bar_of(id: window::Id) -> Option<String> {
    places(|places| {
        places
            .iter()
            .find(|(_, p)| p.id == Some(id))
            .map(|(bar, _)| bar.clone())
    })
}

/// The words of the sign's popover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    fr: bool,
}

impl Words {
    pub fn new(lang: crate::clock::time::Lang) -> Self {
        Self {
            fr: lang == crate::clock::time::Lang::French,
        }
    }

    fn pick(self, fr: &'static str, en: &'static str) -> &'static str {
        if self.fr { fr } else { en }
    }

    /// The popover's title.
    pub fn right_now(self) -> &'static str {
        self.pick("En ce moment", "Right now")
    }

    /// What is used, as a use's line starts.
    pub fn kind(self, kind: Kind, muted: bool) -> &'static str {
        match kind {
            Kind::Microphone if muted => self.pick("Micro coupé", "Microphone muted"),
            Kind::Microphone => self.pick("Micro", "Microphone"),
            Kind::Camera => self.pick("Caméra", "Camera"),
            Kind::Screen => self.pick("Écran", "Screen"),
        }
    }

    /// "depuis 12 min": how long it has been used.
    pub fn since(self, time: Duration) -> String {
        let s = time.as_secs();
        let (m, h) = (s / 60, s / 3600);
        match (self.fr, h, m) {
            (true, 0, 0) => format!("depuis {s} s"),
            (false, 0, 0) => format!("for {s} s"),
            (true, 0, m) => format!("depuis {m} min"),
            (false, 0, m) => format!("for {m} min"),
            (true, h, _) => format!("depuis {h} h {:02}", m % 60),
            (false, h, _) => format!("for {h} h {} min", m % 60),
        }
    }

    /// A use's line: "Micro · meet.google.com · depuis 12 min".
    pub fn line(self, kind: Kind, muted: bool, detail: Option<&str>, since: Duration) -> String {
        let mut line = self.kind(kind, muted).to_owned();
        if let Some(detail) = detail.filter(|d| !d.is_empty()) {
            line.push_str(" · ");
            line.push_str(detail);
        }
        line.push_str(" · ");
        line.push_str(&self.since(since));
        line
    }

    /// A share's detail: "partage la fenêtre « … »", or the screen.
    pub fn shares(self, thing: &SharedThing) -> String {
        match (thing, self.fr) {
            (SharedThing::Window(title), true) => format!("partage la fenêtre « {title} »"),
            (SharedThing::Window(title), false) => {
                format!("sharing the window \u{201c}{title}\u{201d}")
            }
            (SharedThing::Screen(name), true) if name.is_empty() => {
                "partage l'écran du portable".to_owned()
            }
            (SharedThing::Screen(name), false) if name.is_empty() => {
                "sharing the laptop screen".to_owned()
            }
            (SharedThing::Screen(name), true) => format!("partage l'écran « {name} »"),
            (SharedThing::Screen(name), false) => {
                format!("sharing the screen \u{201c}{name}\u{201d}")
            }
        }
    }

    /// An application not known by name.
    pub fn an_app(self) -> &'static str {
        self.pick("Une application", "An application")
    }

    /// The recording's line's name (5.10).
    pub fn recording(self) -> &'static str {
        self.pick("Enregistrement d'écran", "Screen recording")
    }

    /// The recording's line: "Écran · enregistrement · 0:12".
    pub fn recording_line(self, paused: bool, elapsed: Duration) -> String {
        let state = if paused {
            self.pick("en pause", "paused")
        } else {
            self.pick("enregistrement", "recording")
        };
        format!(
            "{} · {state} · {}",
            self.kind(Kind::Screen, false),
            draw::elapsed(elapsed)
        )
    }

    /// How many notifications wait for the end of the sharing.
    pub fn held(self, n: usize) -> String {
        match (n, self.fr) {
            (0, true) => "Les notifications attendent la fin".to_owned(),
            (0, false) => "Notifications wait until the end".to_owned(),
            (1, true) => "1 notification en attente".to_owned(),
            (1, false) => "1 notification held".to_owned(),
            (n, true) => format!("{n} notifications en attente"),
            (n, false) => format!("{n} notifications held"),
        }
    }

    /// The microphone's button: muting the default source, or not.
    pub fn mute(self, muted: bool) -> &'static str {
        if muted {
            self.pick("Rétablir le micro", "Unmute")
        } else {
            self.pick("Couper le micro", "Mute the microphone")
        }
    }

    /// A share's button; with several, each names its application.
    pub fn stop(self, app: Option<&str>) -> String {
        match (app, self.fr) {
            (None, true) => "Arrêter le partage".to_owned(),
            (None, false) => "Stop sharing".to_owned(),
            (Some(app), true) => format!("Arrêter le partage avec {app}"),
            (Some(app), false) => format!("Stop sharing with {app}"),
        }
    }

    pub fn stop_recording(self) -> &'static str {
        self.pick("Arrêter l'enregistrement", "Stop recording")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::time::Lang;

    fn media(key: &str) -> Media {
        Media {
            key: key.to_owned(),
            app: key.to_owned(),
            icon: None,
            ids: Vec::new(),
            media: None,
        }
    }

    fn key(m: &Media) -> String {
        m.key.clone()
    }

    #[test]
    fn a_use_shows_at_once_and_lingers_three_seconds_after_it_ends() {
        let t0 = Instant::now();
        let mut uses = Uses::default();
        // Seen at once: no delay before it shows.
        follow(&mut uses.microphone, vec![media("Firefox")], key, t0);
        assert_eq!(uses.kinds(), [Kind::Microphone]);
        assert_eq!(uses.microphone[0].since, t0);
        assert_eq!(uses.deadline(), None);
        // It ends at 5 s: still shown, until 8 s.
        let end = t0 + Duration::from_secs(5);
        follow(&mut uses.microphone, Vec::new(), key, end);
        assert_eq!(uses.kinds(), [Kind::Microphone]);
        assert!(!uses.microphone[0].live());
        assert_eq!(uses.deadline(), Some(end + LINGER));
        assert_eq!(uses.prune(end + Duration::from_millis(2999)), None);
        assert_eq!(uses.kinds(), [Kind::Microphone]);
        assert_eq!(uses.prune(end + LINGER), Some(end));
        assert!(uses.kinds().is_empty());
    }

    #[test]
    fn a_use_back_while_it_lingers_is_the_same_use_since_it_began() {
        let t0 = Instant::now();
        let mut uses = Uses::default();
        follow(&mut uses.camera, vec![media("Zoom")], key, t0);
        follow(
            &mut uses.camera,
            Vec::new(),
            key,
            t0 + Duration::from_secs(1),
        );
        follow(
            &mut uses.camera,
            vec![media("Zoom")],
            key,
            t0 + Duration::from_secs(2),
        );
        assert_eq!(uses.camera.len(), 1);
        assert!(uses.camera[0].live());
        assert_eq!(uses.camera[0].since, t0);
        assert_eq!(uses.deadline(), None);
    }

    #[test]
    fn the_kinds_side_by_side_in_their_order() {
        let t0 = Instant::now();
        let mut uses = Uses::default();
        follow(
            &mut uses.shares,
            vec![ShareShown {
                id: 1,
                app: "org.mozilla.firefox".into(),
                thing: SharedThing::Screen(String::new()),
            }],
            |s| s.id,
            t0,
        );
        follow(&mut uses.camera, vec![media("Zoom")], key, t0);
        follow(&mut uses.microphone, vec![media("Zoom")], key, t0);
        assert_eq!(uses.kinds(), [Kind::Microphone, Kind::Camera, Kind::Screen]);
        let rec = Uses {
            recording: Some(Seen {
                what: Rec {
                    shown: RecordingShown {
                        id: 1,
                        paused: false,
                        elapsed: 0,
                    },
                    at: t0,
                },
                since: t0,
                ended: None,
            }),
            ..Uses::default()
        };
        assert_eq!(rec.kinds(), [Kind::Screen]);
    }

    #[test]
    fn the_popovers_words() {
        let fr = Words::new(Lang::French);
        let en = Words::new(Lang::English);
        assert_eq!(fr.right_now(), "En ce moment");
        assert_eq!(en.right_now(), "Right now");
        let twelve = Duration::from_secs(12 * 60 + 5);
        assert_eq!(
            fr.line(Kind::Microphone, false, Some("meet.google.com"), twelve),
            "Micro · meet.google.com · depuis 12 min"
        );
        assert_eq!(
            en.line(Kind::Microphone, false, Some("meet.google.com"), twelve),
            "Microphone · meet.google.com · for 12 min"
        );
        assert_eq!(
            fr.line(Kind::Microphone, true, None, Duration::from_secs(4)),
            "Micro coupé · depuis 4 s"
        );
        assert_eq!(
            fr.line(Kind::Camera, false, None, Duration::from_secs(3900)),
            "Caméra · depuis 1 h 05"
        );
        assert_eq!(en.since(Duration::from_secs(3900)), "for 1 h 5 min");
        assert_eq!(
            fr.shares(&SharedThing::Window("Visual Studio Code".into())),
            "partage la fenêtre « Visual Studio Code »"
        );
        assert_eq!(
            en.shares(&SharedThing::Screen(String::new())),
            "sharing the laptop screen"
        );
        assert_eq!(
            fr.recording_line(false, Duration::from_secs(12)),
            "Écran · enregistrement · 0:12"
        );
        assert_eq!(
            en.recording_line(true, Duration::from_secs(724)),
            "Screen · paused · 12:04"
        );
        assert_eq!(fr.mute(false), "Couper le micro");
        assert_eq!(fr.mute(true), "Rétablir le micro");
        assert_eq!(en.mute(false), "Mute the microphone");
        assert_eq!(en.mute(true), "Unmute");
        assert_eq!(fr.stop(None), "Arrêter le partage");
        assert_eq!(en.stop(Some("Firefox")), "Stop sharing with Firefox");
        assert_eq!(fr.stop_recording(), "Arrêter l'enregistrement");
        assert_eq!(en.stop_recording(), "Stop recording");
        assert_eq!(fr.held(4), "4 notifications en attente");
        assert_eq!(en.held(0), "Notifications wait until the end");
    }
}
