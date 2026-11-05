//! The Son page (6.5, settings.md "Son"): the outputs, the inputs and
//! their profiles, each application's volume, all in PipeWire.
//!
//! PipeWire's graph is followed on a thread of its own (`pw-dump
//! --monitor`, through `hocus-sound`, as the bar does) and comes to the
//! window as messages; what the user asks goes to the setter's thread
//! (`wpctl`, `pw-cli`, `pw-metadata`), never run on the window's. Nothing
//! is written in the file: WirePlumber remembers every device's and every
//! application's choices. A choice (an output, an input, a profile, an
//! application's output) toasts with "Annuler" and joins `Ctrl+Z`; a
//! volume, a mute or the balance is heard at once, without a toast.
//!
//! The microphone's meter records only while the input group is on screen
//! and the microphone is not muted: its `pw-record` is a capture stream,
//! which the bar's in-use sign lists as "Réglages".

mod view;

use std::{
    collections::HashMap,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

use hocus_sound::{Ask, Known, Set, Sink, meter::Meter, tester::Side};
use iced::{
    Task,
    futures::channel::mpsc::UnboundedSender,
    keyboard::{Key, Modifiers, key::Named},
};

use super::{App, Message, Page, TOAST, ToastAction, UNDONE};
use crate::{
    lang::{Lang, w},
    model::SectionId,
    sound::{self as model, Graph},
};

/// How long a volume moved on the page shows as asked, should PipeWire
/// not say it back.
const HELD: Duration = Duration::from_millis(1500);
/// The meter's segments.
pub(super) const SEGMENTS: usize = 24;
/// PipeWire tried again after this long when it is not there, up to a
/// minute.
const RETRY: Duration = Duration::from_secs(1);
const RETRY_MOST: Duration = Duration::from_secs(60);

/// A volume of the page: the output's, the input's, an application's.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Which {
    Output,
    Input,
    App(String),
}

/// An input chosen: a source by its node's name, or a headset's
/// microphone, which its call mode's profile brings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    Source(String),
    Headset { card: u32, profile: u32 },
}

/// The page's menus: the output's profile, an application's output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuId {
    Profile,
    App(String),
}

/// What a device's toast offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    /// This sink made the default.
    Default(u32),
}

#[derive(Debug, Clone)]
pub enum Msg {
    Heard(Arc<Graph>),
    /// PipeWire went (or never came).
    Gone,
    Level(f32),
    Testing(Option<Side>),
    Output(String),
    Input(Pick),
    Volume(Which, f32),
    Mute(Which),
    Balance(i32),
    Profile(u32, u32),
    Test,
    Menu(MenuId),
    /// An application's output: a sink by name, or `None` to follow the
    /// default.
    Target(String, Option<String>),
    Act(Act),
}

/// A choice made on the page, for its undo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    Output {
        before: String,
        after: String,
    },
    Input {
        before: Pick,
        after: Pick,
    },
    Profile {
        card: String,
        before: u32,
        after: u32,
    },
    Target {
        app: String,
        before: Option<String>,
        after: Option<String>,
    },
}

impl Choice {
    fn reversed(self) -> Self {
        match self {
            Self::Output { before, after } => Self::Output {
                before: after,
                after: before,
            },
            Self::Input { before, after } => Self::Input {
                before: after,
                after: before,
            },
            Self::Profile {
                card,
                before,
                after,
            } => Self::Profile {
                card,
                before: after,
                after: before,
            },
            Self::Target { app, before, after } => Self::Target {
                app,
                before: after,
                after: before,
            },
        }
    }
}

/// A control of the page that takes the keyboard, in `Tab`'s order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    Outputs,
    OutMute,
    OutVolume,
    Profile,
    Balance,
    Test,
    Inputs,
    InMute,
    InVolume,
    AppMenu(usize),
    AppMute(usize),
    AppVolume(usize),
}

/// An input as the page lists it.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Input {
    pub pick: Pick,
    pub sink: Option<Sink>,
    pub name: String,
}

/// An application as the page lists it: its output streams, else the
/// streams it records with.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct AppRow {
    pub key: String,
    pub name: String,
    pub streams: Vec<u32>,
    pub sound: Option<hocus_sound::Sound>,
    /// The sink it plays on, and whether that is its own choice.
    pub sink: Option<u32>,
    pub target: Option<String>,
    pub plays: bool,
    /// The source it records, if it does.
    pub records: Option<u32>,
}

/// The page's state.
#[derive(Debug)]
pub struct Sound {
    pub(super) graph: Option<Arc<Graph>>,
    asks: Option<mpsc::Sender<Ask>>,
    to_window: Option<UnboundedSender<Message>>,
    /// Volumes moved on the page, shown until PipeWire says them.
    held: HashMap<Which, (f32, Instant)>,
    balance_held: Option<(i32, Instant)>,
    /// The meter recording, and the source's serial it records.
    meter: Option<(u32, Meter)>,
    pub(super) level: f32,
    pub(super) testing: Option<Side>,
    pub(super) menu: Option<(MenuId, Instant)>,
    pub(super) menu_index: usize,
    /// The applications listed, in their order: a new one joins at the
    /// bottom, one goes with its last stream.
    listed: Vec<String>,
    /// A headset switched to its call mode by the page: its card and the
    /// profile it was in.
    switched: Option<(u32, u32)>,
    /// A headset's microphone chosen: made the default once its source
    /// comes.
    awaited: Option<u32>,
    /// The keyboard's row in the outputs' and the inputs' groups.
    pub(super) cursor: (usize, usize),
}

impl Default for Sound {
    fn default() -> Self {
        Self {
            graph: None,
            asks: None,
            to_window: None,
            held: HashMap::new(),
            balance_held: None,
            meter: None,
            level: 0.0,
            testing: None,
            menu: None,
            menu_index: 0,
            listed: Vec::new(),
            switched: None,
            awaited: None,
            cursor: (0, 0),
        }
    }
}

impl Sound {
    /// Follows PipeWire and sets it, on threads of their own: what they
    /// hear comes to the window through `to`.
    pub fn start(to: UnboundedSender<Message>) -> Self {
        let (asks, receive) = mpsc::channel();
        let known = Known::default();
        let reader = known.clone();
        let heard = to.clone();
        let started = std::thread::Builder::new()
            .name("sound".to_owned())
            .spawn(move || follow(&reader, &heard))
            .and_then(|_| {
                std::thread::Builder::new()
                    .name("sound: set".to_owned())
                    .spawn(move || hocus_sound::set::run(&receive, &known))
            });
        if let Err(err) = started {
            tracing::warn!(%err, "cannot follow the sound");
        }
        Self {
            asks: Some(asks),
            to_window: Some(to),
            ..Self::default()
        }
    }

    fn ask(&self, ask: Ask) {
        let sent = self
            .asks
            .as_ref()
            .is_some_and(|a| a.send(ask.clone()).is_ok());
        if !sent {
            tracing::warn!(?ask, "the sound's setter has stopped: dropped");
        }
    }

    /// Whether the meter records now.
    pub(super) fn listening(&self) -> bool {
        self.meter.is_some()
    }

    /// The window goes: the meter's `pw-record` too.
    pub(super) fn stop(&mut self) {
        self.meter = None;
    }

    pub(super) fn output(&self) -> Option<&Sink> {
        self.graph.as_deref()?.output()
    }

    pub(super) fn input(&self) -> Option<&Sink> {
        self.graph.as_deref()?.microphone()
    }

    pub(super) fn outputs(&self) -> Vec<Sink> {
        self.graph
            .as_deref()
            .map(|g| g.sinks.clone())
            .unwrap_or_default()
    }

    /// The inputs: the sources, then the headsets' microphones their call
    /// mode would bring.
    pub(super) fn inputs(&self, lang: Lang) -> Vec<Input> {
        let Some(graph) = self.graph.as_deref() else {
            return Vec::new();
        };
        let mut inputs: Vec<Input> = graph
            .sources
            .iter()
            .map(|s| Input {
                pick: Pick::Source(s.name.clone()),
                sink: Some(s.clone()),
                name: s.description.clone(),
            })
            .collect();
        for (card, profile, _) in model::headsets_asleep(graph) {
            inputs.push(Input {
                pick: Pick::Headset { card, profile },
                sink: None,
                name: w("Micro du casque", "Headset microphone")
                    .get(lang)
                    .to_owned(),
            });
        }
        inputs
    }

    /// The applications playing or recording, in their listed order.
    pub(super) fn apps(&self) -> Vec<AppRow> {
        let Some(graph) = self.graph.as_deref() else {
            return Vec::new();
        };
        let mut rows: Vec<AppRow> = graph
            .apps
            .iter()
            .map(|a| AppRow {
                key: a.key.clone(),
                name: a.name.clone(),
                streams: a.streams.clone(),
                sound: a.sound,
                sink: a.sink,
                target: a.target.clone(),
                plays: true,
                records: None,
            })
            .collect();
        for recorder in graph.recorders.iter().filter(|r| !r.ours) {
            let source = recorder.sources.first().copied();
            match rows.iter_mut().find(|r| r.key == recorder.key) {
                Some(row) => row.records = source,
                None => rows.push(AppRow {
                    key: recorder.key.clone(),
                    name: recorder.name.clone(),
                    streams: recorder.streams.clone(),
                    sound: recorder.sound,
                    sink: None,
                    target: None,
                    plays: false,
                    records: source,
                }),
            }
        }
        rows.sort_by_key(|r| {
            self.listed
                .iter()
                .position(|k| *k == r.key)
                .unwrap_or(usize::MAX)
        });
        rows
    }

    /// The volume shown for `which`: as moved on the page while PipeWire
    /// has not said it.
    pub(super) fn volume(&self, which: &Which, heard: Option<hocus_sound::Sound>) -> Option<f32> {
        match self.held.get(which) {
            Some((v, since)) if since.elapsed() < HELD => Some(*v),
            _ => heard.map(|s| s.volume),
        }
    }

    pub(super) fn balance(&self) -> i32 {
        match self.balance_held {
            Some((b, since)) if since.elapsed() < HELD => b,
            _ => self.output().map_or(0, model::balance),
        }
    }

    /// The meter's lit segments.
    pub(super) fn lit(&self) -> usize {
        if self.input().and_then(|s| s.sound).is_some_and(|s| s.muted) {
            return 0;
        }
        ((self.level.clamp(0.0, 1.0) * SEGMENTS as f32).round() as usize).min(SEGMENTS)
    }
}

/// Follows PipeWire's graph until the window goes, starting `pw-dump`
/// again whenever it ends; each change of what the page lists is sent.
fn follow(known: &Known, to: &UnboundedSender<Message>) {
    let mut wait = RETRY;
    loop {
        let started = Instant::now();
        let mut last: Option<Graph> = None;
        if to.is_closed() {
            return;
        }
        let ended = hocus_sound::dump::dump(|graph| {
            known.learn(graph);
            let now = graph.mixer(|_, _| None::<()>);
            if last.as_ref() != Some(&now) {
                last = Some(now.clone());
                let _ = to.unbounded_send(Message::Sound(Msg::Heard(Arc::new(now))));
            }
        });
        known.forget();
        // Said once it was heard: tries that fail change nothing on the
        // page, nor draw it.
        if last.is_some() && to.unbounded_send(Message::Sound(Msg::Gone)).is_err() {
            return;
        }
        tracing::info!(why = ended, "PipeWire is not followed: tried again later");
        if started.elapsed() >= RETRY_MOST {
            wait = RETRY;
        }
        std::thread::sleep(wait);
        wait = (wait * 2).min(RETRY_MOST);
    }
}

impl App {
    pub(super) fn sound_update(&mut self, msg: Msg) -> Task<Message> {
        match msg {
            Msg::Heard(graph) => self.sound_heard(graph),
            Msg::Gone => {
                self.sound.graph = None;
                self.sound.menu = None;
                Task::none()
            }
            Msg::Level(level) => {
                self.sound.level = level;
                Task::none()
            }
            Msg::Testing(side) => {
                self.sound.testing = side;
                Task::none()
            }
            Msg::Output(name) => {
                let Some(before) = self.sound.output().map(|s| s.name.clone()) else {
                    return Task::none();
                };
                if before == name {
                    return Task::none();
                }
                self.sound_choose(Choice::Output {
                    before,
                    after: name,
                })
            }
            Msg::Input(pick) => {
                let Some(before) = self.sound.input().map(|s| Pick::Source(s.name.clone())) else {
                    return self.sound_choose_input(pick);
                };
                if before == pick {
                    return Task::none();
                }
                self.sound_choose(Choice::Input {
                    before,
                    after: pick,
                })
            }
            Msg::Volume(which, volume) => {
                self.sound_volume(which, volume);
                Task::none()
            }
            Msg::Mute(which) => {
                self.sound_mute(&which);
                Task::none()
            }
            Msg::Balance(balance) => {
                if let Some(sink) = self.sound.output().cloned()
                    && !sink.mono()
                {
                    self.sound.balance_held = Some((balance, Instant::now()));
                    self.sound
                        .ask(Ask::Channels(sink.id, model::balanced(&sink, balance)));
                }
                Task::none()
            }
            Msg::Profile(card, index) => {
                self.sound.menu = None;
                let Some(graph) = self.sound.graph.clone() else {
                    return Task::none();
                };
                let Some(card) = graph.card(card) else {
                    return Task::none();
                };
                let Some(before) = card.profile.filter(|p| *p != index) else {
                    return Task::none();
                };
                self.sound_choose(Choice::Profile {
                    card: card.name.clone(),
                    before,
                    after: index,
                })
            }
            Msg::Test => {
                if self.sound.testing.is_some() {
                    return Task::none();
                }
                let Some(sink) = self.sound.output().cloned() else {
                    return Task::none();
                };
                let (Some(serial), Some(to)) = (sink.serial, self.sound.to_window.clone()) else {
                    return Task::none();
                };
                self.sound.testing = Some(if sink.mono() {
                    Side::Middle
                } else {
                    Side::Left
                });
                hocus_sound::tester::play(serial, sink.mono(), move |side| {
                    let _ = to.unbounded_send(Message::Sound(Msg::Testing(side)));
                });
                Task::none()
            }
            Msg::Menu(id) => {
                self.menu = None;
                self.sound.menu = match self.sound.menu.take() {
                    Some((open, _)) if open == id => None,
                    _ => {
                        self.sound.menu_index = self.sound_menu_current(&id);
                        Some((id, Instant::now()))
                    }
                };
                Task::none()
            }
            Msg::Target(app, to) => {
                self.sound.menu = None;
                let Some(row) = self.sound.apps().into_iter().find(|r| r.key == app) else {
                    return Task::none();
                };
                let before = self.sound_target_name(&row);
                if before == to {
                    return Task::none();
                }
                self.sound_choose(Choice::Target {
                    app,
                    before,
                    after: to,
                })
            }
            Msg::Act(Act::Default(id)) => {
                self.toast = None;
                self.sound.ask(Ask::Default(id));
                Task::none()
            }
        }
    }

    /// PipeWire's graph changed: the page shows it; volumes it now says
    /// are no longer held; a device that came or went toasts.
    fn sound_heard(&mut self, graph: Arc<Graph>) -> Task<Message> {
        let before = self.sound.graph.replace(graph.clone());
        // The volumes PipeWire now says as they were moved.
        let said = |which: &Which| match which {
            Which::Output => graph.output().and_then(|s| s.sound),
            Which::Input => graph.microphone().and_then(|s| s.sound),
            Which::App(key) => graph
                .apps
                .iter()
                .find(|a| a.key == *key)
                .and_then(|a| a.sound),
        };
        self.sound.held.retain(|which, (v, since)| {
            since.elapsed() < HELD && said(which).is_none_or(|s| (s.volume - *v).abs() > 0.6)
        });
        if let Some((b, _)) = self.sound.balance_held
            && graph.output().map(model::balance) == Some(b)
        {
            self.sound.balance_held = None;
        }
        // The applications' order: the listed ones kept, new ones after.
        let now: Vec<String> = graph
            .apps
            .iter()
            .map(|a| a.key.clone())
            .chain(
                graph
                    .recorders
                    .iter()
                    .filter(|r| !r.ours)
                    .map(|r| r.key.clone()),
            )
            .collect();
        self.sound.listed.retain(|k| now.contains(k));
        for key in now {
            if !self.sound.listed.contains(&key) {
                self.sound.listed.push(key);
            }
        }
        // A headset's microphone chosen: its source has come.
        if let Some(card) = self.sound.awaited
            && let Some(source) = graph.sources.iter().find(|s| s.device == Some(card))
        {
            self.sound.awaited = None;
            self.sound.ask(Ask::DefaultSource(source.id));
        }
        match before {
            Some(before) => self.sound_devices_toast(&before, &graph),
            None => Task::none(),
        }
    }

    /// A device of the outputs came or went: said, without "Annuler".
    fn sound_devices_toast(&mut self, before: &Graph, now: &Graph) -> Task<Message> {
        let lang = self.lang;
        let real = |s: &&Sink| s.device.is_some();
        let gone = before
            .output()
            .filter(real)
            .filter(|was| !now.sinks.iter().any(|s| s.name == was.name));
        if let Some(gone) = gone
            && let Some(to) = now.output()
        {
            let text = match lang {
                Lang::French => format!(
                    "{} déconnecté : le son passe par {}.",
                    gone.description, to.description
                ),
                Lang::English => format!(
                    "{} disconnected: sound goes to {}.",
                    gone.description, to.description
                ),
            };
            return self.toast(text, ToastAction::None, TOAST);
        }
        let came = now
            .sinks
            .iter()
            .filter(real)
            .find(|s| !before.sinks.iter().any(|b| b.name == s.name));
        let (Some(came), Some(was)) = (came, before.output()) else {
            return Task::none();
        };
        if came.default {
            let text = match lang {
                Lang::French => format!(
                    "{} connecté : le son repasse par lui, comme tu l’avais choisi.",
                    came.description
                ),
                Lang::English => format!(
                    "{} connected: sound goes back to it, as you chose.",
                    came.description
                ),
            };
            self.toast(text, ToastAction::Sound(Act::Default(was.id)), TOAST)
        } else {
            let text = match lang {
                Lang::French => format!(
                    "{} connecté ; le son reste sur {}.",
                    came.description, was.description
                ),
                Lang::English => format!(
                    "{} connected; sound stays on {}.",
                    came.description, was.description
                ),
            };
            self.toast(text, ToastAction::Sound(Act::Default(came.id)), TOAST)
        }
    }

    /// What a device's toast's button says.
    pub(super) fn sound_act_label(&self, act: Act) -> String {
        let Act::Default(id) = act;
        let name = self
            .sound
            .graph
            .as_deref()
            .and_then(|g| g.node(id))
            .map(|s| s.description.clone())
            .unwrap_or_default();
        let came = self.sound.output().is_some_and(|s| s.id != id)
            && self
                .toast
                .as_ref()
                .is_some_and(|t| t.text.contains(&format!("{name} connect")));
        match (self.lang, came) {
            (Lang::French, true) => format!("Passer à {name}"),
            (Lang::English, true) => format!("Switch to {name}"),
            (Lang::French, false) => format!("Garder {name}"),
            (Lang::English, false) => format!("Keep {name}"),
        }
    }

    /// A choice made: applied, toasted with "Annuler", kept for `Ctrl+Z`.
    fn sound_choose(&mut self, choice: Choice) -> Task<Message> {
        self.sound.menu = None;
        let Some(text) = self.sound_apply(&choice, false) else {
            return Task::none();
        };
        self.undo.push(super::Undo::Sound(choice));
        self.redo.clear();
        self.toast(text, ToastAction::Undo, TOAST)
    }

    /// An input chosen while none is the default: no undo.
    fn sound_choose_input(&mut self, pick: Pick) -> Task<Message> {
        let choice = Choice::Input {
            before: pick.clone(),
            after: pick,
        };
        match self.sound_apply(&choice, false) {
            Some(text) => self.toast(text, ToastAction::None, TOAST),
            None => Task::none(),
        }
    }

    /// `choice` undone (or redone): its other side applied, the row lit.
    pub(super) fn sound_undo(&mut self, choice: Choice, redo: bool) -> Task<Message> {
        let (applied, kept) = if redo {
            (choice.clone(), choice)
        } else {
            (choice.clone().reversed(), choice)
        };
        let Some(said) = self.sound_apply(&applied, true) else {
            return Task::none();
        };
        if redo {
            self.undo.push(super::Undo::Sound(kept));
        } else {
            self.redo.push(super::Undo::Sound(kept));
        }
        let row = match &applied {
            Choice::Output { .. } => "sound.output",
            Choice::Input { .. } => "sound.input",
            Choice::Profile { .. } => "sound.profile",
            Choice::Target { .. } => "sound.apps",
        };
        self.go(Page::Section(SectionId::Sound));
        self.light = Some((row, Instant::now()));
        let text = match (self.lang, redo) {
            (Lang::French, false) => format!("Annulé : {said}."),
            (Lang::English, false) => format!("Undone: {said}."),
            (Lang::French, true) => format!("Rétabli : {said}."),
            (Lang::English, true) => format!("Redone: {said}."),
        };
        self.toast(text, ToastAction::None, UNDONE)
    }

    /// Sets `choice`'s `after` in PipeWire; what the toast says of it
    /// (`back`: as an undo says it), or `None` when it cannot be.
    fn sound_apply(&mut self, choice: &Choice, back: bool) -> Option<String> {
        let graph = self.sound.graph.clone()?;
        let lang = self.lang;
        match choice {
            Choice::Output { after, .. } => {
                let sink = graph.sinks.iter().find(|s| s.name == *after)?;
                self.sound.ask(Ask::Default(sink.id));
                Some(match (lang, back) {
                    (Lang::French, false) => format!("Sortie : {}", sink.description),
                    (Lang::English, false) => format!("Output: {}", sink.description),
                    (Lang::French, true) => format!("la sortie revient à {}", sink.description),
                    (Lang::English, true) => format!("the output is back to {}", sink.description),
                })
            }
            Choice::Input { after, .. } => match after {
                Pick::Source(name) => {
                    let source = graph.sources.iter().find(|s| s.name == *name)?;
                    self.sound.ask(Ask::DefaultSource(source.id));
                    // A headset switched by the page goes back to its
                    // profile before, its microphone left.
                    if let Some((card, profile)) = self.sound.switched
                        && source.device != Some(card)
                    {
                        self.sound.switched = None;
                        self.sound.ask(Ask::Profile {
                            device: card,
                            index: profile,
                        });
                    }
                    self.sound.awaited = None;
                    Some(match (lang, back) {
                        (Lang::French, false) => format!("Entrée : {}", source.description),
                        (Lang::English, false) => format!("Input: {}", source.description),
                        (Lang::French, true) => {
                            format!("l’entrée revient à {}", source.description)
                        }
                        (Lang::English, true) => {
                            format!("the input is back to {}", source.description)
                        }
                    })
                }
                Pick::Headset { card, profile } => {
                    let was = graph.card(*card)?.profile?;
                    self.sound.switched = Some((*card, was));
                    self.sound.awaited = Some(*card);
                    self.sound.ask(Ask::Profile {
                        device: *card,
                        index: *profile,
                    });
                    Some(
                        match lang {
                            Lang::French => "Entrée : Micro du casque · le casque passe en mode appel (HFP)",
                            Lang::English => "Input: Headset microphone · the headset switches to call mode (HFP)",
                        }
                        .to_owned(),
                    )
                }
            },
            Choice::Profile { card, after, .. } => {
                let card = graph.cards.iter().find(|c| c.name == *card)?;
                let profile = card.profiles.iter().find(|p| p.index == *after)?;
                self.sound.ask(Ask::Profile {
                    device: card.id,
                    index: *after,
                });
                Some(match (lang, back) {
                    (Lang::French, false) => {
                        format!("Profil de {} : {}", card.description, profile.description)
                    }
                    (Lang::English, false) => {
                        format!("{}'s profile: {}", card.description, profile.description)
                    }
                    (Lang::French, true) => format!("le profil revient à {}", profile.description),
                    (Lang::English, true) => {
                        format!("the profile is back to {}", profile.description)
                    }
                })
            }
            Choice::Target { app, after, .. } => {
                let row = self.sound.apps().into_iter().find(|r| r.key == *app)?;
                let sink = match after {
                    Some(name) => Some(graph.sinks.iter().find(|s| s.name == *name)?),
                    None => None,
                };
                for stream in &row.streams {
                    self.sound.ask(Ask::Target {
                        stream: *stream,
                        to: sink.and_then(|s| s.serial),
                    });
                }
                Some(match (lang, sink) {
                    (Lang::French, Some(s)) => format!("{} joue sur {}", row.name, s.description),
                    (Lang::English, Some(s)) => format!("{} plays on {}", row.name, s.description),
                    (Lang::French, None) => format!("{} suit la sortie", row.name),
                    (Lang::English, None) => format!("{} follows the output", row.name),
                })
            }
        }
    }

    /// The output an application chose, by its sink's name; `None`: it
    /// follows the default.
    pub(super) fn sound_target_name(&self, row: &AppRow) -> Option<String> {
        let target = row.target.as_deref()?;
        let graph = self.sound.graph.as_deref()?;
        graph
            .sinks
            .iter()
            .find(|s| s.serial.is_some_and(|n| n.to_string() == target) || s.name == target)
            .map(|s| s.name.clone())
    }

    fn sound_volume(&mut self, which: Which, volume: f32) {
        let volume = volume.clamp(0.0, 100.0);
        let (nodes, sound) = self.sound_nodes(&which);
        if nodes.is_empty() {
            return;
        }
        self.sound.held.insert(which, (volume, Instant::now()));
        for id in &nodes {
            self.sound.ask(Ask::Node(*id, Set::Volume(volume)));
        }
        // Moving it unmutes.
        if sound.is_some_and(|s| s.muted) {
            for id in nodes {
                self.sound.ask(Ask::Node(id, Set::Mute(false)));
            }
        }
    }

    fn sound_mute(&mut self, which: &Which) {
        let (nodes, sound) = self.sound_nodes(which);
        let muted = sound.is_some_and(|s| s.muted);
        for id in nodes {
            self.sound.ask(Ask::Node(id, Set::Mute(!muted)));
        }
    }

    /// The nodes a volume of the page sets, and their sound now.
    fn sound_nodes(&self, which: &Which) -> (Vec<u32>, Option<hocus_sound::Sound>) {
        match which {
            Which::Output => self
                .sound
                .output()
                .map_or((Vec::new(), None), |s| (vec![s.id], s.sound)),
            Which::Input => self
                .sound
                .input()
                .map_or((Vec::new(), None), |s| (vec![s.id], s.sound)),
            Which::App(key) => self
                .sound
                .apps()
                .into_iter()
                .find(|r| r.key == *key)
                .map_or((Vec::new(), None), |r| (r.streams, r.sound)),
        }
    }

    /// The meter records while the page shows the input group, the
    /// microphone not muted; stopped otherwise.
    pub(super) fn sound_listen(&mut self) {
        let wanted = self.sound_listened();
        match (wanted, &self.sound.meter) {
            (Some(serial), Some((now, _))) if *now == serial => {}
            (None, None) => {}
            (None, Some(_)) => {
                self.sound.meter = None;
                self.sound.level = 0.0;
                tracing::info!("the meter stops listening");
            }
            (Some(serial), _) => {
                self.sound.meter = None;
                self.sound.level = 0.0;
                let Some(to) = self.sound.to_window.clone() else {
                    return;
                };
                let app = self.title();
                let meter = Meter::start(serial, &app, move |level| {
                    to.unbounded_send(Message::Sound(Msg::Level(level))).is_ok()
                });
                if let Some(meter) = meter {
                    tracing::info!(source = serial, pid = meter.pid(), "the meter listens");
                    self.sound.meter = Some((serial, meter));
                }
            }
        }
    }

    /// The source the meter should record now, by its serial.
    fn sound_listened(&self) -> Option<u32> {
        if self.page != Page::Section(SectionId::Sound) || self.life != super::Life::Open {
            return None;
        }
        let source = self.sound.input()?;
        if source.sound.is_some_and(|s| s.muted) {
            return None;
        }
        let group = self.places.at.get("group:sound.input")?;
        let viewport = self.places.page?.viewport;
        let shown = group.y < viewport.y + viewport.height && group.y + group.height > viewport.y;
        shown.then_some(source.serial).flatten()
    }

    /// The page's controls, in `Tab`'s order.
    pub(super) fn sound_stops(&self) -> Vec<Stop> {
        let Some(graph) = self.sound.graph.as_deref() else {
            return Vec::new();
        };
        let mut stops = Vec::new();
        if let Some(output) = graph.output() {
            stops.extend([Stop::Outputs, Stop::OutMute, Stop::OutVolume]);
            if self.sound_profiles().len() > 1 {
                stops.push(Stop::Profile);
            }
            if !output.mono() {
                stops.push(Stop::Balance);
            }
            stops.push(Stop::Test);
        }
        if graph.microphone().is_some() {
            stops.extend([Stop::Inputs, Stop::InMute, Stop::InVolume]);
        } else if !self.sound.inputs(self.lang).is_empty() {
            stops.push(Stop::Inputs);
        }
        for i in 0..self.sound.apps().len() {
            stops.extend([Stop::AppMenu(i), Stop::AppMute(i), Stop::AppVolume(i)]);
        }
        stops
    }

    /// The output's device's profiles a user may choose.
    pub(super) fn sound_profiles(&self) -> Vec<hocus_sound::Profile> {
        let Some(graph) = self.sound.graph.as_deref() else {
            return Vec::new();
        };
        graph
            .output()
            .and_then(|s| s.device)
            .and_then(|d| graph.card(d))
            .map(|c| c.choices().cloned().collect())
            .unwrap_or_default()
    }

    /// A menu's options: what each sets, as messages.
    pub(super) fn sound_menu_options(
        &self,
        id: &MenuId,
    ) -> Vec<(String, Option<String>, bool, Msg)> {
        let Some(graph) = self.sound.graph.as_deref() else {
            return Vec::new();
        };
        match id {
            MenuId::Profile => {
                let card = graph.output().and_then(|s| s.device);
                let current = card.and_then(|c| graph.card(c)).and_then(|c| c.profile);
                self.sound_profiles()
                    .into_iter()
                    .map(|p| {
                        let msg = Msg::Profile(card.unwrap_or_default(), p.index);
                        (p.description, None, current == Some(p.index), msg)
                    })
                    .collect()
            }
            MenuId::App(key) => {
                let Some(row) = self.sound.apps().into_iter().find(|r| r.key == *key) else {
                    return Vec::new();
                };
                let chosen = self.sound_target_name(&row);
                let mut options = vec![(
                    w("Suit la sortie", "Follows the output")
                        .get(self.lang)
                        .to_owned(),
                    graph.output().map(|s| s.description.clone()),
                    chosen.is_none(),
                    Msg::Target(key.clone(), None),
                )];
                for sink in &graph.sinks {
                    options.push((
                        sink.description.clone(),
                        None,
                        chosen.as_deref() == Some(sink.name.as_str()),
                        Msg::Target(key.clone(), Some(sink.name.clone())),
                    ));
                }
                options
            }
        }
    }

    fn sound_menu_current(&self, id: &MenuId) -> usize {
        self.sound_menu_options(id)
            .iter()
            .position(|(_, _, on, _)| *on)
            .unwrap_or(0)
    }

    pub(super) fn sound_menu_key(&mut self, key: &Key) -> Task<Message> {
        let Some((id, _)) = self.sound.menu.clone() else {
            return Task::none();
        };
        let options = self.sound_menu_options(&id);
        match key.as_ref() {
            Key::Named(Named::ArrowDown) => {
                self.sound.menu_index =
                    (self.sound.menu_index + 1).min(options.len().saturating_sub(1));
            }
            Key::Named(Named::ArrowUp) => {
                self.sound.menu_index = self.sound.menu_index.saturating_sub(1);
            }
            Key::Named(Named::Enter | Named::Space) => {
                if let Some((_, _, _, msg)) = options.into_iter().nth(self.sound.menu_index) {
                    return self.sound_update(msg);
                }
            }
            _ => {}
        }
        Task::none()
    }

    /// A key on the page's control `stop`.
    pub(super) fn sound_key(
        &mut self,
        stop: Stop,
        named: Named,
        modifiers: Modifiers,
    ) -> Task<Message> {
        let press = matches!(named, Named::Enter | Named::Space);
        match stop {
            Stop::Outputs | Stop::Inputs => {
                let outputs = stop == Stop::Outputs;
                let count = if outputs {
                    self.sound.outputs().len()
                } else {
                    self.sound.inputs(self.lang).len()
                };
                let cursor = if outputs {
                    &mut self.sound.cursor.0
                } else {
                    &mut self.sound.cursor.1
                };
                match named {
                    Named::ArrowDown => *cursor = (*cursor + 1).min(count.saturating_sub(1)),
                    Named::ArrowUp => *cursor = cursor.saturating_sub(1),
                    _ if press => {
                        let at = *cursor;
                        let msg = if outputs {
                            self.sound
                                .outputs()
                                .get(at)
                                .map(|s| Msg::Output(s.name.clone()))
                        } else {
                            self.sound
                                .inputs(self.lang)
                                .get(at)
                                .map(|i| Msg::Input(i.pick.clone()))
                        };
                        if let Some(msg) = msg {
                            return self.sound_update(msg);
                        }
                    }
                    _ => {}
                }
                Task::none()
            }
            Stop::OutMute | Stop::InMute | Stop::AppMute(_) if press => {
                let which = match stop {
                    Stop::OutMute => Which::Output,
                    Stop::InMute => Which::Input,
                    Stop::AppMute(i) => match self.sound.apps().get(i) {
                        Some(r) => Which::App(r.key.clone()),
                        None => return Task::none(),
                    },
                    _ => return Task::none(),
                };
                self.sound_update(Msg::Mute(which))
            }
            Stop::OutVolume | Stop::InVolume | Stop::AppVolume(_) => {
                let (which, heard) = match stop {
                    Stop::OutVolume => (Which::Output, self.sound.output().and_then(|s| s.sound)),
                    Stop::InVolume => (Which::Input, self.sound.input().and_then(|s| s.sound)),
                    Stop::AppVolume(i) => match self.sound.apps().get(i) {
                        Some(r) => (Which::App(r.key.clone()), r.sound),
                        None => return Task::none(),
                    },
                    _ => return Task::none(),
                };
                let now = self.sound.volume(&which, heard).unwrap_or(0.0).round();
                let to = match named {
                    Named::ArrowLeft => now - 5.0,
                    Named::ArrowRight => now + 5.0,
                    Named::PageDown => now - 25.0,
                    Named::PageUp => now + 25.0,
                    Named::Home => 0.0,
                    Named::End => 100.0,
                    _ => return Task::none(),
                };
                self.sound_update(Msg::Volume(which, to))
            }
            Stop::Profile | Stop::AppMenu(_)
                if press || (named == Named::ArrowDown && modifiers.alt()) =>
            {
                let id = match stop {
                    Stop::Profile => MenuId::Profile,
                    Stop::AppMenu(i) => match self.sound.apps().get(i) {
                        Some(r) => MenuId::App(r.key.clone()),
                        None => return Task::none(),
                    },
                    _ => return Task::none(),
                };
                self.sound_update(Msg::Menu(id))
            }
            Stop::Balance => {
                let now = self.sound.balance();
                let to = match named {
                    Named::ArrowLeft => now - 10,
                    Named::ArrowRight => now + 10,
                    Named::Home => -100,
                    Named::End => 100,
                    _ => return Task::none(),
                };
                self.sound_update(Msg::Balance(to.clamp(-100, 100)))
            }
            Stop::Test if press => self.sound_update(Msg::Test),
            _ => Task::none(),
        }
    }

    /// The home's card: the output in use and its volume.
    pub(super) fn sound_card(&self) -> String {
        match self.sound.output() {
            Some(sink) => format!(
                "{} · {}",
                sink.description,
                model::say_volume(sink, false, self.lang)
            ),
            None => w("Aucune sortie", "No output").get(self.lang).to_owned(),
        }
    }
}
