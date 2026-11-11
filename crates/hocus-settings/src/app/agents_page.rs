//! The "Agents" page (6.16, variant A, docs/design/settings.md "Agents"):
//! the tools followed, the halo and its preview, the pandas.
//!
//! The tools are the compositor's (4.0): it says them on the Settings'
//! line (`agent-tools`), and the page asks it to plug one in or out, or
//! to follow herdr or not; its watcher's thread writes the tool's file and
//! the desktop's state file, and the line says the tool again. "Brancher"
//! asks first, in a sheet over the window: nothing is written before its
//! "Brancher". "Débrancher" needs no consent (it puts the file back); its
//! toast's "Annuler", and `Ctrl+Z`, plug it back in without asking. The
//! page reads, on a thread, whether each tool's file reads and when a tool
//! was declined (`agents.toml`): it never writes them.
//!
//! The halo's preview plays a state once when chosen (the wave in 600 ms,
//! the glow fading in), never with reduced motion; at rest it draws
//! nothing. The five pandas (#649, 72 px) play their moods one after the
//! other (300 ms apart) as the page opens and when they come back, one
//! again when the pointer comes over it; the peek's panda comes up when it
//! is let out: then nothing moves.

use std::{
    collections::HashMap,
    sync::OnceLock,
    time::{Duration, Instant},
};

use iced::{
    Alignment, Background, Color, Length, Padding, Point, Rectangle, Renderer, Size, Theme,
    keyboard::{Key, key::Named},
    mouse,
    widget::{
        Space, button, canvas, column, container, mouse_area, rich_text, row, span, stack, svg,
        text,
    },
};

use super::{
    App, Control, Message, Page, TOAST, ToastAction, UNDONE, Undo,
    style::*,
    view::{El, card_box, divided, label, t},
};
use crate::{
    agents::{self as model, Played, Row, Tool, Tools},
    lang::{Lang, w},
    model::{SectionId, Setting, Value},
    screens::Said,
    store::Slot,
};
/// The wave runs down the preview's bar in this time, the glow fades in
/// over it (bar.md §6).
const WAVE: Duration = Duration::from_millis(600);
/// Each next panda plays its mood this much later than the one before.
const STAGGER: Duration = Duration::from_millis(300);
/// The family, in its order: its moods and their words.
const FAMILY: [(hocus_pandas::Mood, (&str, &str)); 5] = [
    (hocus_pandas::Mood::Working, ("au travail", "at work")),
    (hocus_pandas::Mood::Waiting, ("t’attend", "waiting")),
    (hocus_pandas::Mood::Done, ("a fini", "done")),
    (hocus_pandas::Mood::Stuck, ("bloqué", "stuck")),
    (hocus_pandas::Mood::Resting, ("au repos", "resting")),
];
/// The dots in their place, with the pandas off: the rows' colours.
const DOTS: [u32; 5] = [0x6CC287, 0xF5B44A, 0x7EE0A0, 0xF2945B, 0xA9B2C2];
/// The peek's panda coming up.
const RISE: Duration = Duration::from_millis(420);
/// "Copié" stays this long.
const COPIED: Duration = Duration::from_secs(2);
/// The pandas' drawings' box.
const PANDA_BOX: (f32, f32) = (126.0, 120.5);
/// The pandas, as large as they are shown off (#649).
const PANDA: f32 = 72.0;
const PEEK_PANDA: f32 = 58.0;

/// The page's own messages.
#[derive(Debug, Clone)]
pub enum Msg {
    /// "Brancher": the consent sheet opens.
    Plug(Tool),
    /// The sheet's "Brancher" (`true`) or "Annuler".
    Consent(bool),
    Unplug(Tool),
    /// "Suivre herdr" switched.
    Herdr,
    /// The preview plays this state.
    Play(Played),
    Copy,
    /// "La barre ›".
    Link,
    /// The pointer came over the family's panda at this place: it plays
    /// its mood again.
    Pet(usize),
    /// What the tools' files and the state file say.
    Files(HashMap<Tool, Facts>),
}

/// A control of the page's own that takes the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    Link,
    Action(Tool),
    Preview,
    Copy,
}

/// A tool plugged in or out, herdr followed or not, for its undo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choice {
    pub tool: Tool,
    /// Plugged in (followed) after it.
    pub on: bool,
}

/// What the page read of a tool's files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Facts {
    /// Its settings read (or there are none yet).
    pub reads: bool,
    /// When it was declined, seconds since the epoch.
    pub declined_on: Option<i64>,
}

/// The consent sheet: for which tool, and which button has the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Consent {
    tool: Tool,
    on_plug: bool,
}

/// The page's state.
#[derive(Debug, Default)]
pub struct State {
    /// The tools as the compositor last said them; none before it did, or
    /// without a compositor.
    tools: Option<Tools>,
    files: HashMap<Tool, Facts>,
    consent: Option<Consent>,
    /// The preview's state, and since when it plays.
    played: Option<Played>,
    play: Option<Instant>,
    copied: Option<Instant>,
    /// The family playing its moods, since then; each panda the pointer
    /// came over, since then.
    hop: Option<Instant>,
    petted: [Option<Instant>; 5],
    /// The peek's panda coming up, since then.
    rise: Option<Instant>,
    /// The page was last drawn shown: its pandas hop as it opens.
    seen: bool,
    /// The window's messages: what the files say comes back there.
    to: Option<iced::futures::channel::mpsc::UnboundedSender<Message>>,
}

impl State {
    pub fn with(to: &iced::futures::channel::mpsc::UnboundedSender<Message>) -> Self {
        Self {
            to: Some(to.clone()),
            ..Self::default()
        }
    }
}

/// A panda's still drawing.
fn still(mood: hocus_pandas::Mood) -> svg::Handle {
    static STILL: OnceLock<[svg::Handle; 6]> = OnceLock::new();
    let drawings = STILL.get_or_init(|| {
        hocus_pandas::Mood::ALL.map(|m| svg::Handle::from_memory(m.still().as_bytes()))
    });
    let at = hocus_pandas::Mood::ALL
        .iter()
        .position(|m| *m == mood)
        .unwrap_or(0);
    drawings[at].clone()
}

/// How far the family's panda `i` is into its burst at `now`, while it
/// plays: the family started at `opened` (each 300 ms after the one
/// before), the pointer came over it at `petted`.
fn family_burst(
    i: usize,
    opened: Option<Instant>,
    petted: Option<Instant>,
    now: Instant,
) -> Option<Duration> {
    let (mood, _) = FAMILY.get(i)?;
    let started = [
        opened.map(|t| t + STAGGER * u32::try_from(i).unwrap_or(0)),
        petted,
    ]
    .into_iter()
    .flatten()
    .filter(|t| *t <= now)
    .max()?;
    let t = now.saturating_duration_since(started);
    (t < hocus_pandas::lasts(mood.burst())).then_some(t)
}

impl App {
    fn agents_slot(&self, id: &str) -> Option<Slot> {
        crate::model::setting(id).map(|s| Slot::of(s, self.source))
    }

    fn agents_word(&self, id: &str) -> &'static str {
        match self.agents_slot(id).and_then(|s| self.value(s)) {
            Some(Value::Word(word)) => word,
            _ => match id {
                model::HALO => "all",
                model::REMIND => "5m",
                _ => "waiting-and-done",
            },
        }
    }

    fn agents_pandas(&self) -> bool {
        self.agents_slot(model::PANDAS)
            .and_then(|s| self.value(s))
            .is_none_or(|v| v == Value::Bool(true))
    }

    fn agents_shows(&self) -> bool {
        self.page == Page::Section(SectionId::Agents)
    }

    /// Whether the page moves at `now`.
    pub(super) fn agents_moving(&self, now: Instant) -> bool {
        if !self.agents_shows() || self.reduced_motion {
            return false;
        }
        let within = |since: Option<Instant>, d: Duration| {
            since.is_some_and(|s| now.saturating_duration_since(s) < d)
        };
        within(self.agents.play, WAVE + Duration::from_millis(50))
            || (0..FAMILY.len()).any(|i| self.agents_burst(i, now).is_some())
            || within(self.agents.rise, RISE)
    }

    /// How far the family's panda `i` is into its burst at `now`, while
    /// it plays: from the page's opening (each 300 ms after the one
    /// before), or from the pointer coming over it.
    fn agents_burst(&self, i: usize, now: Instant) -> Option<Duration> {
        family_burst(
            i,
            self.agents.hop,
            self.agents.petted.get(i).copied().flatten(),
            now,
        )
    }

    /// The keyboard's stops, in `Tab`'s order.
    pub(super) fn agents_controls(&self) -> Vec<Control> {
        let mut out = vec![Control::Agents(Stop::Link)];
        if let Some(tools) = &self.agents.tools {
            for tool in Tool::ALL {
                if self.agents_action(tools, tool).is_some() {
                    out.push(Control::Agents(Stop::Action(tool)));
                }
            }
        }
        out.push(Control::Agents(Stop::Copy));
        for setting in crate::agents::settings() {
            out.push(Control::Setting(Slot::of(setting, self.source)));
            if setting.id == model::REMIND {
                out.push(Control::Agents(Stop::Preview));
            }
        }
        out
    }

    fn agents_focused(&self, stop: Stop) -> bool {
        matches!(self.focus, super::Focus::Control(i)
            if self.controls().get(i) == Some(&Control::Agents(stop)))
    }

    /// `tool`'s action, if it has one now: plug in (`Some(true)`), unplug
    /// (`Some(false)`); herdr's switch says whether it is followed.
    fn agents_action(&self, tools: &Tools, tool: Tool) -> Option<bool> {
        let reads = self.agents.files.get(&tool).is_none_or(|f| f.reads);
        match tools.row(tool) {
            Row::Plugged | Row::ToApprove => Some(false),
            Row::Found | Row::Declined if reads => Some(true),
            Row::Running | Row::RunningIgnored | Row::Stopped => Some(!tools.herdr_declined),
            _ => None,
        }
    }

    // --- What changes it -------------------------------------------------------------

    pub(super) fn agents_update(&mut self, msg: Msg) -> iced::Task<Message> {
        match msg {
            Msg::Plug(tool) => {
                if self
                    .agents
                    .tools
                    .as_ref()
                    .and_then(|t| self.agents_action(t, tool))
                    == Some(true)
                {
                    tracing::debug!(tool = tool.word(), "the consent sheet opens");
                    self.agents.consent = Some(Consent {
                        tool,
                        on_plug: true,
                    });
                }
                iced::Task::none()
            }
            Msg::Consent(yes) => {
                let Some(consent) = self.agents.consent.take() else {
                    return iced::Task::none();
                };
                if !yes {
                    tracing::debug!(tool = consent.tool.word(), "not plugged in");
                    return iced::Task::none();
                }
                self.agents_choose(Choice {
                    tool: consent.tool,
                    on: true,
                })
            }
            Msg::Unplug(tool) => self.agents_choose(Choice { tool, on: false }),
            Msg::Herdr => {
                let Some(tools) = &self.agents.tools else {
                    return iced::Task::none();
                };
                let on = tools.herdr_declined;
                self.agents_choose(Choice {
                    tool: Tool::Herdr,
                    on,
                })
            }
            Msg::Pet(i) => {
                let now = Instant::now();
                let plays = self.agents_burst(i, now).is_some();
                if let Some(petted) = self.agents.petted.get_mut(i)
                    && !self.reduced_motion
                    && !plays
                {
                    *petted = Some(now);
                }
                iced::Task::none()
            }
            Msg::Play(played) => {
                self.agents.played = Some(played);
                self.agents.play = (!self.reduced_motion).then(Instant::now);
                iced::Task::none()
            }
            Msg::Copy => {
                self.agents.copied = Some(Instant::now());
                tracing::debug!("the command copied");
                // A frame once "Copié" is over, to say "Copier" again.
                iced::Task::batch([
                    iced::clipboard::write(model::COMMAND.to_owned()),
                    iced::Task::perform(tokio::time::sleep(COPIED), |()| Message::Nothing),
                ])
            }
            Msg::Link => self.show_row(crate::bar::AGENTS),
            Msg::Files(files) => {
                self.agents.files = files;
                iced::Task::none()
            }
        }
    }

    /// A tool plugged in or out, herdr followed or not: asked of the
    /// compositor, toasted with "Annuler", and on `Ctrl+Z`'s stack.
    fn agents_choose(&mut self, choice: Choice) -> iced::Task<Message> {
        if !self.agents_ask(choice) {
            return iced::Task::none();
        }
        self.undo.push(Undo::Agents(choice));
        self.redo.clear();
        let text = self.agents_said(choice);
        self.toast(text, ToastAction::Undo, TOAST)
    }

    /// `choice` asked of the compositor; whether it took it.
    fn agents_ask(&mut self, choice: Choice) -> bool {
        let line = match (choice.tool, choice.on) {
            (Tool::Herdr, true) => "agent-follow herdr".to_owned(),
            (Tool::Herdr, false) => "agent-unfollow herdr".to_owned(),
            (tool, true) => format!("agent-plug {}", tool.word()),
            (tool, false) => format!("agent-unplug {}", tool.word()),
        };
        let asked = self.screens_ask(line.clone());
        tracing::info!(line, asked, "the agents' tools asked");
        if choice.on && choice.tool == Tool::Herdr {
            self.agents.hop = (!self.reduced_motion).then(Instant::now);
        }
        asked
    }

    /// "Claude Code débranché", "herdr suivi".
    fn agents_said(&self, choice: Choice) -> String {
        let name = choice.tool.title();
        match (choice.tool, choice.on, self.lang) {
            (Tool::Herdr, true, Lang::French) => "herdr suivi".to_owned(),
            (Tool::Herdr, true, Lang::English) => "herdr followed".to_owned(),
            (Tool::Herdr, false, Lang::French) => "herdr n’est plus suivi".to_owned(),
            (Tool::Herdr, false, Lang::English) => "herdr no longer followed".to_owned(),
            (_, true, Lang::French) => format!("{name} branché"),
            (_, true, Lang::English) => format!("{name} plugged in"),
            (_, false, Lang::French) => format!("{name} débranché"),
            (_, false, Lang::English) => format!("{name} unplugged"),
        }
    }

    /// `choice` undone (its other side asked, without asking the user: the
    /// consent was given) or redone.
    pub(super) fn agents_undo(&mut self, choice: Choice, redo: bool) -> iced::Task<Message> {
        let now = if redo {
            choice
        } else {
            Choice {
                on: !choice.on,
                ..choice
            }
        };
        self.agents_ask(now);
        if redo {
            self.undo.push(Undo::Agents(choice));
        } else {
            self.redo.push(Undo::Agents(choice));
        }
        self.go(Page::Section(SectionId::Agents));
        self.light = Some((model::TOOLS_ROW, Instant::now()));
        let said = self.agents_said(now);
        let text = match (self.lang, redo) {
            (Lang::French, false) => format!("Annulé : {said}."),
            (Lang::English, false) => format!("Undone: {said}."),
            (Lang::French, true) => format!("Rétabli : {said}."),
            (Lang::English, true) => format!("Redone: {said}."),
        };
        self.toast(text, ToastAction::None, UNDONE)
    }

    /// The consent sheet takes the keyboard: `Esc` cancels, `Enter`
    /// acts, `Tab` and the arrows go from one button to the other.
    pub(super) fn agents_key_first(&mut self, key: &Key) -> Option<iced::Task<Message>> {
        let consent = self.agents.consent.as_mut()?;
        match key.as_ref() {
            Key::Named(Named::Escape) => return Some(self.agents_update(Msg::Consent(false))),
            Key::Named(Named::Enter | Named::Space) => {
                let yes = consent.on_plug;
                return Some(self.agents_update(Msg::Consent(yes)));
            }
            Key::Named(Named::Tab | Named::ArrowLeft | Named::ArrowRight) => {
                consent.on_plug = !consent.on_plug;
            }
            _ => {}
        }
        Some(iced::Task::none())
    }

    pub(super) fn agents_key(&mut self, stop: Stop, named: Named) -> iced::Task<Message> {
        match (stop, named) {
            (Stop::Preview, Named::ArrowLeft | Named::ArrowRight) => {
                let at = self
                    .agents
                    .played
                    .and_then(|p| Played::ALL.iter().position(|q| *q == p));
                let next = match (named, at) {
                    (Named::ArrowLeft, Some(i)) if i > 0 => Played::ALL[i - 1],
                    (Named::ArrowRight, Some(i)) if i + 1 < Played::ALL.len() => Played::ALL[i + 1],
                    (_, None) => Played::ALL[0],
                    _ => return iced::Task::none(),
                };
                self.agents_update(Msg::Play(next))
            }
            (_, Named::Space | Named::Enter) => match stop {
                Stop::Link => self.agents_update(Msg::Link),
                Stop::Copy => self.agents_update(Msg::Copy),
                Stop::Preview => {
                    self.agents_update(Msg::Play(self.agents.played.unwrap_or(Played::Waiting)))
                }
                Stop::Action(tool) => {
                    let action = self
                        .agents
                        .tools
                        .as_ref()
                        .and_then(|t| self.agents_action(t, tool));
                    match (tool, action) {
                        (Tool::Herdr, Some(_)) => self.agents_update(Msg::Herdr),
                        (_, Some(true)) => self.agents_update(Msg::Plug(tool)),
                        (_, Some(false)) => self.agents_update(Msg::Unplug(tool)),
                        (_, None) => iced::Task::none(),
                    }
                }
            },
            _ => iced::Task::none(),
        }
    }

    /// What the compositor's line says of the tools; `false` when it is
    /// not theirs.
    pub(super) fn agents_heard(&mut self, said: &Said) -> bool {
        match said {
            Said::AgentTools(tools) => {
                if self.agents.tools.as_ref() != Some(tools) {
                    let consent_gone = self
                        .agents
                        .consent
                        .is_some_and(|c| self.agents_action(tools, c.tool) != Some(true));
                    if consent_gone {
                        self.agents.consent = None;
                    }
                    self.agents.tools = Some(tools.clone());
                    self.agents_read_files();
                }
                true
            }
            Said::Gone => {
                self.agents.tools = None;
                self.agents.consent = None;
                false
            }
            _ => false,
        }
    }

    /// Whether each tool's file reads, and when each was declined: read
    /// on a thread, said back to the window.
    fn agents_read_files(&mut self) {
        let Some(to) = self.agents.to.clone() else {
            return;
        };
        let started = std::thread::Builder::new()
            .name("agents' files".to_owned())
            .spawn(move || {
                let Some(places) = hocus_agents::tools::Places::from_env(
                    std::path::PathBuf::new(),
                    hocus_config::state_dir(),
                ) else {
                    return;
                };
                let memory = hocus_agents::tools::Memory::load(&places);
                let files: HashMap<Tool, Facts> = Tool::ALL
                    .into_iter()
                    .map(|tool| {
                        let reads = tool
                            .hooks()
                            .is_none_or(|hooks| hocus_agents::tools::reads(hooks, &places));
                        (
                            tool,
                            Facts {
                                reads,
                                declined_on: memory.declined_on.get(tool.word()).copied(),
                            },
                        )
                    })
                    .collect();
                let _ = to.unbounded_send(Message::Agents(Msg::Files(files)));
            });
        if let Err(err) = started {
            tracing::warn!(%err, "the agents' files are not read");
        }
    }

    /// Opened, the page reads the files again (one may have been broken
    /// or mended meanwhile), and its pandas hop.
    pub(super) fn agents_visit(&mut self) {
        let shows = self.agents_shows();
        if shows && !self.agents.seen {
            if self.agents.tools.is_some() {
                self.agents_read_files();
            }
            self.agents.hop = (!self.reduced_motion).then(Instant::now);
        }
        self.agents.seen = shows;
    }

    /// A row of the model's at 45 % and inert: the reminder while the halo
    /// is off.
    pub(super) fn agents_inert(&self, setting: &Setting) -> bool {
        setting.id == model::REMIND && self.agents_word(model::HALO) == "off"
    }

    /// A choice made in one of the page's menus: the peek's panda comes up
    /// when it is let out, the pandas hop when they come back.
    pub(super) fn agents_chose(&mut self, slot: Slot, value: &Value) {
        let motion = !self.reduced_motion;
        match (slot.setting, value) {
            (model::PEEK, Value::Word(word)) if *word != "off" => {
                self.agents.rise = motion.then(Instant::now);
            }
            (model::PANDAS, Value::Bool(true)) => self.agents.hop = motion.then(Instant::now),
            _ => {}
        }
    }

    // --- What it draws ---------------------------------------------------------------

    fn agents_link<'a>(&self) -> El<'a> {
        let focused = self.agents_focused(Stop::Link);
        container(
            button(t(self.say(w("La barre ›", "The bar ›")), 13.0, MINT.get()).font(BOLD))
                .padding([1, 2])
                .on_press(Message::Agents(Msg::Link))
                .style(move |_, status| button::Style {
                    background: matches!(status, button::Status::Hovered)
                        .then(|| Background::Color(crate::theme::ink(rgba(255, 255, 255, 0.05)))),
                    text_color: TEXT.get(),
                    border: if focused {
                        ring(5.0)
                    } else {
                        border(5.0, 0.0, Color::TRANSPARENT)
                    },
                    ..button::Style::default()
                }),
        )
        .id("link:bar.agents".to_owned())
        .into()
    }

    /// A group: its label, a note on its right, its card.
    fn agents_group<'a>(&self, label_words: &'static str, note: String, body: El<'a>) -> El<'a> {
        column![
            row![
                label(label_words),
                Space::new().width(Length::Fill),
                t(note, 12.0, DIM.get()),
            ]
            .align_y(Alignment::Center)
            .height(28),
            card_box(body),
        ]
        .spacing(8)
        .into()
    }

    pub(super) fn agents_page<'a>(&'a self, mut page: iced::widget::Column<'a, Message>) -> El<'a> {
        let lang = self.lang;
        page = page.push(
            row![
                super::view::icon(crate::model::Icon::Bar, 14.0, DIM.get()),
                t(
                    self.say(w(
                        "Le groupe des agents dans la barre, et sa place, se règlent dans",
                        "The agents’ group in the bar, and its place, are set in",
                    )),
                    13.0,
                    DIM.get()
                ),
                self.agents_link(),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .wrap(),
        );
        let controls = self.controls();
        let model_row = |id: &str| -> El<'_> {
            match crate::model::setting(id) {
                Some(s) => self.row(s, &controls),
                None => Space::new().into(),
            }
        };
        // LES OUTILS SUIVIS
        let count = self.agents.tools.as_ref().map_or_else(String::new, |t| {
            model::count_said(t.followed(), t.running, lang)
        });
        let tools_body: El<'_> = match &self.agents.tools {
            Some(tools) if tools.none() => self.agents_no_tool(),
            Some(tools) => {
                let mut body = column![];
                for (i, tool) in Tool::ALL.into_iter().enumerate() {
                    body = body.push(divided(i > 0, self.agents_tool_row(tools, tool)));
                }
                body = body.push(divided(true, self.agents_another()));
                body.into()
            }
            None => column![
                container(t(
                    self.say(w(
                        "Les outils ne sont pas connus : le bureau ne répond pas.",
                        "The tools are not known: the desktop does not answer.",
                    )),
                    13.0,
                    DIM.get()
                ))
                .padding([14, 16]),
                divided(true, self.agents_another()),
            ]
            .into(),
        };
        page = page.push(
            container(self.agents_group(model::TOOLS.label.get(lang), count, tools_body))
                .id(format!("row:{}", model::TOOLS_ROW)),
        );
        // LE HALO
        page = page.push(
            self.agents_group(
                model::HALO_GROUP.label.get(lang),
                self.say(w("Les réunions gardent le leur", "Meetings keep theirs"))
                    .to_owned(),
                column![
                    model_row(model::HALO),
                    divided(true, model_row(model::REMIND)),
                    divided(true, self.agents_preview_row()),
                ]
                .into(),
            ),
        );
        // LES PANDAS
        page = page.push(
            self.agents_group(
                model::PANDAS_GROUP.label.get(lang),
                self.say(w(
                    "Ils ne bougent qu’à un changement",
                    "They move only on a change",
                ))
                .to_owned(),
                column![
                    self.agents_peek_row(model_row(model::PEEK)),
                    divided(true, model_row(model::PANDAS)),
                    self.agents_pandas_sample(),
                    divided(true, self.agents_rest_row()),
                ]
                .into(),
            ),
        );
        container(page).id("page:agents".to_owned()).into()
    }

    /// A tool's mark: a round in its colour, its letter.
    fn agents_mark<'a>(tool: Tool, size: f32, alpha: f32) -> El<'a> {
        let (letter, colour) = tool.mark();
        container(
            text(letter)
                .size(size * 0.5)
                .font(BOLD)
                .color(fade(rgb(0x0B1020), alpha)),
        )
        .width(size)
        .height(size)
        .center(size)
        .style(boxed(fade(rgb(colour), alpha), size / 2.0, None))
        .into()
    }

    fn agents_tool_row<'a>(&'a self, tools: &Tools, tool: Tool) -> El<'a> {
        let lang = self.lang;
        let row_state = tools.row(tool);
        let absent = row_state == Row::Absent;
        let alpha = if absent { 0.4 } else { 1.0 };
        let said = tools.of(tool);
        let file = said.map(|s| s.file.clone()).unwrap_or_default();
        let facts = self.agents.files.get(&tool).copied().unwrap_or(Facts {
            reads: true,
            declined_on: None,
        });
        let (state_words, colour) = row_state.state();
        let declined = facts.declined_on.and_then(|at| model::day_said(at, lang));
        let mut rest = model::line(row_state, tool, &file, declined.as_deref(), lang);
        if tool == Tool::Herdr && row_state == Row::Stopped && tools.herdr_declined {
            rest = self
                .say(w(
                    "pas suivi ; « Suivre herdr » le reprend.",
                    "not followed; “Follow herdr” brings it back.",
                ))
                .to_owned();
        }
        let broken = !facts.reads && tool != Tool::Herdr && !absent;
        let state_span = span(format!("{} ", state_words.get(lang)))
            .font(BOLD)
            .color(fade(rgb(colour), alpha));
        let mut spans: Vec<iced::widget::text::Span<'a, (), iced::Font>> = vec![state_span];
        if broken {
            spans.push(span(format!("· {}", model::broken(&file, lang))).color(PEACH.get()));
        } else if !rest.is_empty() {
            spans.push(span("· ".to_owned()).color(fade(DIM.get(), alpha)));
            match (!file.is_empty()).then(|| rest.find(&file)).flatten() {
                Some(at) => {
                    let (pre, after) = rest.split_at(at);
                    let post = &after[file.len()..];
                    spans.push(span(pre.to_owned()).color(fade(DIM.get(), alpha)));
                    spans.push(
                        span(file.clone())
                            .font(MONO)
                            .size(11.5)
                            .color(fade(SOFT.get(), alpha)),
                    );
                    spans.push(span(post.to_owned()).color(fade(DIM.get(), alpha)));
                }
                None => spans.push(span(rest.clone()).color(fade(DIM.get(), alpha))),
            }
        }
        let words = column![
            t(tool.title(), 14.0, fade(TEXT.get(), alpha)).font(SEMIBOLD),
            rich_text(spans).size(12.5),
        ]
        .spacing(3)
        .width(Length::Fill);
        let focused = self.agents_focused(Stop::Action(tool));
        let action: El<'_> = match (tool, self.agents_action(tools, tool)) {
            (Tool::Herdr, Some(on)) => self.agents_switch(on, focused),
            (_, Some(true)) => self.agents_button(
                self.say(w("Brancher", "Plug in")),
                true,
                focused,
                Some(Message::Agents(Msg::Plug(tool))),
            ),
            (_, Some(false)) => self.agents_button(
                self.say(w("Débrancher", "Unplug")),
                false,
                focused,
                Some(Message::Agents(Msg::Unplug(tool))),
            ),
            (_, None) if broken => {
                self.agents_button(self.say(w("Brancher", "Plug in")), true, false, None)
            }
            (_, None) => Space::new().into(),
        };
        let action: El<'_> = container(action).id(format!("tool:{}", tool.word())).into();
        let action: El<'_> = if tool == Tool::Herdr && !absent {
            row![
                t(self.say(w("Suivre herdr", "Follow herdr")), 12.5, DIM.get()),
                action
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into()
        } else {
            action
        };
        container(
            row![Self::agents_mark(tool, 28.0, alpha), words, action]
                .spacing(12)
                .align_y(Alignment::Center),
        )
        .padding(Padding {
            top: 11.0,
            right: 14.0,
            bottom: 11.0,
            left: 16.0,
        })
        .id(format!("row:agents.{}", tool.word()))
        .into()
    }

    /// A button: green ("Brancher") or secondary; inert without `press`.
    fn agents_button<'a>(
        &self,
        said: &'static str,
        green: bool,
        focused: bool,
        press: Option<Message>,
    ) -> El<'a> {
        let alpha = if press.is_some() { 1.0 } else { INERT };
        let (fill, ink) = if green {
            (ACCENT.get(), DEEP.get())
        } else {
            (crate::theme::ink(rgba(255, 255, 255, 0.08)), TEXT.get())
        };
        let mut button = button(t(said, 13.0, fade(ink, alpha)).font(BOLD))
            .height(32)
            .padding([7, 14])
            .style(move |_, status| {
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: Some(Background::Color(fade(
                        Color {
                            a: if hovered {
                                (fill.a + 0.06).min(1.0)
                            } else {
                                fill.a
                            },
                            ..fill
                        },
                        alpha,
                    ))),
                    text_color: ink,
                    border: if focused {
                        ring(8.0)
                    } else {
                        border(8.0, 0.0, Color::TRANSPARENT)
                    },
                    ..button::Style::default()
                }
            });
        if let Some(press) = press {
            button = button.on_press(press);
        }
        button.into()
    }

    /// herdr's switch.
    fn agents_switch<'a>(&self, on: bool, focused: bool) -> El<'a> {
        let track = if on {
            ACCENT.get()
        } else {
            crate::theme::Ink::Token(|t| t.switch_off).get()
        };
        let knob =
            container(Space::new().width(18).height(18)).style(boxed(Color::WHITE, 9.0, None));
        button(container(knob).padding(Padding::default().left(if on { 20.0 } else { 2.0 }).top(2)))
            .width(40)
            .height(22)
            .padding(0)
            .on_press(Message::Agents(Msg::Herdr))
            .style(move |_, _| button::Style {
                background: Some(Background::Color(track)),
                text_color: TEXT.get(),
                border: if focused {
                    ring(11.0)
                } else {
                    border(11.0, 0.0, Color::TRANSPARENT)
                },
                ..button::Style::default()
            })
            .into()
    }

    /// "Un autre outil": the command any program can run, and "Copier".
    fn agents_another(&self) -> El<'_> {
        let lang = self.lang;
        let copied = self
            .agents
            .copied
            .is_some_and(|at| self.now.saturating_duration_since(at) < COPIED);
        let focused = self.agents_focused(Stop::Copy);
        let copy = container(
            button(
                t(
                    if copied {
                        self.say(w("Copié", "Copied"))
                    } else {
                        self.say(w("Copier", "Copy"))
                    },
                    12.5,
                    if copied { MINT.get() } else { TEXT.get() },
                )
                .font(SEMIBOLD),
            )
            .padding([5, 10])
            .on_press(Message::Agents(Msg::Copy))
            .style(flat(
                Some(crate::theme::ink(rgba(255, 255, 255, 0.06))),
                crate::theme::ink(rgba(255, 255, 255, 0.10)),
                7.0,
                focused,
            )),
        )
        .id("agents:copy".to_owned());
        let command = container(
            row![
                t(model::COMMAND, 12.0, SOFT.get()).font(MONO),
                Space::new().width(Length::Fill),
                copy,
            ]
            .align_y(Alignment::Center),
        )
        .padding(Padding {
            top: 6.0,
            right: 6.0,
            bottom: 6.0,
            left: 12.0,
        })
        .style(boxed(crate::theme::ink(rgba(0, 0, 0, 0.25)), 9.0, None));
        container(
            column![
                t(self.say(w("Un autre outil", "Another tool")), 14.0, TEXT.get()).font(SEMIBOLD),
                t(
                    w(
                        "N’importe quel programme peut dire où en est son agent, d’une commande : travaille, attend, fini, échoue, repos.",
                        "Any program can say where its agent is, with one command: working, waiting, done, failed, idle.",
                    )
                    .get(lang),
                    12.5,
                    DIM.get()
                ),
                command,
            ]
            .spacing(6),
        )
        .padding([12, 16])
        .id(format!("row:{}", model::ANOTHER))
        .into()
    }

    /// No tool on this computer: a dashed line, and the command.
    fn agents_no_tool(&self) -> El<'_> {
        let lang = self.lang;
        let words = container(
            column![
                t(
                    self.say(w(
                        "Aucun outil d’agent sur cet ordinateur",
                        "No agent tool on this computer"
                    )),
                    14.0,
                    TEXT.get()
                )
                .font(SEMIBOLD),
                t(
                    w(
                        "Claude Code, Codex et herdr sont cherchés tout seuls ; un autre outil peut le dire d’une commande.",
                        "Claude Code, Codex and herdr are looked for by themselves; another tool can say it with a command.",
                    )
                    .get(lang),
                    12.5,
                    DIM.get()
                ),
            ]
            .spacing(4)
            .align_x(Alignment::Center),
        )
        .padding([18, 16])
        .width(Length::Fill)
        .center_x(Length::Fill);
        let dashed = stack![
            canvas(Dashed).width(Length::Fill).height(Length::Fill),
            words,
        ]
        .height(88);
        column![
            container(dashed)
                .padding([12, 14])
                .id("agents:none".to_owned()),
            self.agents_another(),
        ]
        .into()
    }

    /// "Aperçu": the bar's top, a segmented control, what the state does.
    fn agents_preview_row(&self) -> El<'_> {
        let lang = self.lang;
        let halo = self.agents_word(model::HALO);
        let played = self.agents.played;
        let shown = played.filter(|p| p.shown_by(halo));
        let line = match (played, shown) {
            (Some(_), None) => self.say(w("Pas de halo pour ça", "No halo for that")),
            (Some(p), Some(_)) => p.said().get(lang),
            (None, _) => self.say(w(
                "Choisis un état : il se joue sur la barre, rien n’est écrit.",
                "Choose a state: it plays on the bar, nothing is written.",
            )),
        };
        let focused = self.agents_focused(Stop::Preview);
        let option = |p: Played| -> El<'_> {
            let chosen = played == Some(p);
            container(
                button(
                    t(
                        p.label().get(lang),
                        12.5,
                        if chosen { TEXT.get() } else { DIM.get() },
                    )
                    .font(SEMIBOLD),
                )
                .height(26)
                .padding([4, 12])
                .on_press(Message::Agents(Msg::Play(p)))
                .style(move |_, _| button::Style {
                    background: chosen.then_some(Background::Color(crate::theme::ink(rgba(
                        255, 255, 255, 0.13,
                    )))),
                    text_color: TEXT.get(),
                    border: border(7.0, 0.0, Color::TRANSPARENT),
                    ..button::Style::default()
                }),
            )
            .id(format!("preview:{}", p.word()))
            .into()
        };
        let segmented =
            container(row(Played::ALL.map(option)))
                .padding(2)
                .style(move |_: &Theme| container::Style {
                    background: Some(Background::Color(crate::theme::ink(rgba(
                        255, 255, 255, 0.05,
                    )))),
                    border: if focused {
                        ring(9.0)
                    } else {
                        border(9.0, 0.0, Color::TRANSPARENT)
                    },
                    ..container::Style::default()
                });
        let since = self
            .agents
            .play
            .map(|at| self.now.saturating_duration_since(at));
        let bar = canvas(PreviewBar {
            shown,
            since,
            reduced: self.reduced_motion,
        })
        .width(46)
        .height(118);
        let lit = self.lit(model::PREVIEW);
        container(
            row![
                container(bar).id("agents:preview-bar".to_owned()),
                column![
                    t(self.say(w("Aperçu", "Preview")), 14.0, TEXT.get()).font(SEMIBOLD),
                    container(t(
                        line,
                        12.5,
                        if shown.is_none() && played.is_some() {
                            FAINT.get()
                        } else {
                            DIM.get()
                        }
                    ))
                    .id("agents:preview-line".to_owned()),
                ]
                .spacing(3)
                .width(Length::Fill),
                segmented,
            ]
            .spacing(16)
            .align_y(Alignment::Center),
        )
        .padding(Padding {
            top: 12.0,
            right: 14.0,
            bottom: 12.0,
            left: 16.0,
        })
        .style(move |_: &Theme| container::Style {
            background: (lit > 0.0).then_some(Background::Color(
                crate::theme::Ink::Accent(0.20 * lit).get(),
            )),
            border: border(11.0, 1.5, crate::theme::Ink::Accent(0.85 * lit).get()),
            ..container::Style::default()
        })
        .id(format!("row:{}", model::PREVIEW))
        .into()
    }

    /// The peek's row with its panda on the left, coming up when let out,
    /// asleep at 40 % when it never comes out.
    fn agents_peek_row<'a>(&'a self, row_el: El<'a>) -> El<'a> {
        let off = self.agents_word(model::PEEK) == "off";
        let rise = self.agents.rise.map_or(1.0, |since| {
            let t = self.now.saturating_duration_since(since);
            super::view::ease_out(t, RISE)
        });
        let tall = PEEK_PANDA * PANDA_BOX.1 / PANDA_BOX.0;
        let drop = (1.0 - rise) * 18.0;
        let head = container(
            svg(still(hocus_pandas::Mood::Waiting))
                .width(PEEK_PANDA)
                .height(tall)
                .opacity(if off { 0.4 } else { rise.max(0.2) }),
        )
        .padding(Padding {
            top: drop,
            bottom: 0.0,
            left: 0.0,
            right: 0.0,
        })
        .height(tall + 4.0)
        .clip(true);
        row![
            container(head)
                .padding(Padding::default().left(16.0))
                .id("agents:peek-panda".to_owned()),
            container(row_el).width(Length::Fill),
        ]
        .align_y(Alignment::Center)
        .into()
    }

    /// Under the pandas' switch: the family, its five moods side by side
    /// (#649), each a frame of its burst while it plays; their dots with
    /// the pandas off.
    fn agents_pandas_sample(&self) -> El<'_> {
        let lang = self.lang;
        let pandas = self.agents_pandas();
        let tall = PANDA * PANDA_BOX.1 / PANDA_BOX.0;
        let mut cells = row![].spacing(10);
        for (i, ((mood, word), dot)) in FAMILY.into_iter().zip(DOTS).enumerate() {
            let face: El<'_> = if pandas {
                let drawing = self
                    .agents_burst(i, self.now)
                    .filter(|_| !self.reduced_motion)
                    .and_then(|t| {
                        let t = hocus_pandas::STEP * hocus_pandas::step(t);
                        hocus_pandas::frame(mood, mood.burst(), t)
                    });
                let handle = match drawing {
                    Some(frame) => svg::Handle::from_memory(frame.into_bytes()),
                    None => still(mood),
                };
                svg(handle).width(PANDA).height(tall).into()
            } else {
                container(container(Space::new().width(10).height(10)).style(boxed(
                    rgb(dot),
                    5.0,
                    None,
                )))
                .width(PANDA)
                .height(tall)
                .center(PANDA)
                .into()
            };
            cells = cells.push(
                mouse_area(
                    container(
                        column![face, t(w(word.0, word.1).get(lang), 12.0, rgb(0xA9B2C2)),]
                            .spacing(4)
                            .align_x(Alignment::Center),
                    )
                    .padding([8, 4])
                    .width(Length::FillPortion(1))
                    .style(boxed(
                        crate::theme::ink(rgba(255, 255, 255, 0.03)),
                        9.0,
                        None,
                    ))
                    .id(format!("agents:sample:{i}")),
                )
                .on_enter(Message::Agents(Msg::Pet(i))),
            );
        }
        container(cells)
            .padding(Padding {
                top: 0.0,
                right: 14.0,
                bottom: 12.0,
                left: 16.0,
            })
            .id("agents:sample".to_owned())
            .into()
    }

    /// "Jamais au repos": what the bar already does, said.
    fn agents_rest_row(&self) -> El<'_> {
        let lang = self.lang;
        let lit = self.lit(model::REST);
        container(
            row![
                column![
                    t(self.say(w("Jamais au repos", "Never at rest")), 14.0, TEXT.get())
                        .font(SEMIBOLD),
                    t(
                        w(
                            "Ils restent immobiles sur économie d’énergie et avec les mouvements réduits (Apparence et langue ›).",
                            "They stay still on power saving and with reduced motion (Appearance and language ›).",
                        )
                        .get(lang),
                        12.5,
                        DIM.get()
                    ),
                ]
                .spacing(2)
                .width(Length::Fill),
                t(self.say(w("Toujours", "Always")), 13.0, DIM.get()).font(SEMIBOLD),
            ]
            .spacing(16)
            .align_y(Alignment::Center),
        )
        .padding(Padding {
            top: 12.0,
            right: 14.0,
            bottom: 12.0,
            left: 16.0,
        })
        .style(move |_: &Theme| container::Style {
            background: (lit > 0.0).then_some(Background::Color(
                crate::theme::Ink::Accent(0.20 * lit).get(),
            )),
            border: border(11.0, 1.5, crate::theme::Ink::Accent(0.85 * lit).get()),
            ..container::Style::default()
        })
        .id(format!("row:{}", model::REST))
        .into()
    }

    /// The consent sheet, over the window at 60 %.
    pub(super) fn agents_sheet(&self) -> Option<El<'_>> {
        let consent = self.agents.consent?;
        let lang = self.lang;
        let tool = consent.tool;
        let file = self
            .agents
            .tools
            .as_ref()
            .and_then(|t| t.of(tool))
            .map(|s| s.file.clone())
            .unwrap_or_default();
        let title = match lang {
            Lang::French => format!("Brancher {}", tool.title()),
            Lang::English => format!("Plug in {}", tool.title()),
        };
        let what = match tool {
            Tool::Codex => w(
                "Hocuspanda ajoute ses crochets et sa notification aux réglages de Codex : il saura quand un agent travaille, t’attend (une permission) ou a fini son tour.",
                "Hocuspanda adds its hooks and its notification to Codex’s settings: it will know when an agent works, waits for you (a permission) or has finished its turn.",
            ),
            _ => w(
                "Hocuspanda ajoute ses crochets aux réglages de Claude Code : il saura quand un agent travaille, t’attend (une permission, une question) ou a fini.",
                "Hocuspanda adds its hooks to Claude Code’s settings: it will know when an agent works, waits for you (a permission, a question) or is done.",
            ),
        };
        let mut body = column![
            row![
                Self::agents_mark(tool, 32.0, 1.0),
                text(title).size(17).font(BOLD).color(TEXT),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            t(what.get(lang), 13.0, SOFT.get()).line_height(1.5),
            column![
                t(
                    self.say(w("FICHIER MODIFIÉ", "FILE CHANGED")),
                    11.0,
                    FAINT.get()
                )
                .font(BOLD),
                container(t(file, 12.0, TEXT.get()).font(MONO))
                    .padding([6, 10])
                    .style(boxed(crate::theme::ink(rgba(0, 0, 0, 0.25)), 8.0, None)),
            ]
            .spacing(6),
            t(
                self.say(w(
                    "Une copie de l’original est gardée. « Débrancher » remet tout comme avant.",
                    "A copy of the original is kept. “Unplug” puts everything back as it was.",
                )),
                12.0,
                DIM.get()
            ),
        ]
        .spacing(14);
        if tool == Tool::Codex {
            body = body.push(t(
                self.say(w(
                    "Codex te demandera ensuite de les approuver avec /hooks.",
                    "Codex will then ask you to approve them with /hooks.",
                )),
                12.0,
                rgb(0xF5C46E),
            ));
        }
        let buttons = row![
            Space::new().width(Length::Fill),
            container(self.agents_button(
                self.say(w("Annuler", "Cancel")),
                false,
                !consent.on_plug,
                Some(Message::Agents(Msg::Consent(false))),
            ))
            .id("sheet:cancel".to_owned()),
            container(self.agents_button(
                self.say(w("Brancher", "Plug in")),
                true,
                consent.on_plug,
                Some(Message::Agents(Msg::Consent(true))),
            ))
            .id("sheet:plug".to_owned()),
        ]
        .spacing(8);
        let sheet = container(column![body, buttons].spacing(20))
            .width(400)
            .padding(20)
            .style(|_: &Theme| container::Style {
                background: Some(Background::Color(rgb(0x1B2130))),
                border: border(14.0, 1.0, RIM_12.get()),
                shadow: shadow(),
                ..container::Style::default()
            })
            .id("sheet".to_owned());
        Some(
            stack![
                mouse_area(
                    container(Space::new().width(Length::Fill).height(Length::Fill)).style(boxed(
                        crate::theme::ink(rgba(0, 0, 0, 0.6)),
                        0.0,
                        None
                    ))
                )
                .on_press(Message::Nothing),
                container(sheet)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center(Length::Fill),
            ]
            .into(),
        )
    }

    /// What the page shows, for the log.
    pub(super) fn agents_shown(&self) -> serde_json::Value {
        let lang = self.lang;
        let tools: Vec<serde_json::Value> =
            self.agents.tools.as_ref().map_or_else(Vec::new, |tools| {
                Tool::ALL
                .into_iter()
                .map(|tool| {
                    let row_state = tools.row(tool);
                    let facts = self.agents.files.get(&tool);
                    serde_json::json!({
                        "tool": tool.word(),
                        "row": row_state.word(),
                        "state": row_state.state().0.get(lang),
                        "action": match (tool, self.agents_action(tools, tool)) {
                            (Tool::Herdr, Some(on)) => if on { "follow-on" } else { "follow-off" },
                            (_, Some(true)) => "plug",
                            (_, Some(false)) => "unplug",
                            (_, None) => "none",
                        },
                        "reads": facts.is_none_or(|f| f.reads),
                        "declined_on": facts.and_then(|f| f.declined_on),
                    })
                })
                .collect()
            });
        serde_json::json!({
            "known": self.agents.tools.is_some(),
            "none": self.agents.tools.as_ref().is_some_and(Tools::none),
            "tools": tools,
            "count": self.agents.tools.as_ref().map(|t| model::count_said(t.followed(), t.running, lang)),
            "consent": self.agents.consent.map(|c| c.tool.word()),
            "consent_on": self.agents.consent.map(|c| if c.on_plug { "plug" } else { "cancel" }),
            "played": self.agents.played.map(Played::word),
            "preview_halo": self.agents.played.map(|p| p.shown_by(self.agents_word(model::HALO))),
            "remind_inert": self.agents_word(model::HALO) == "off",
            "pandas": self.agents_pandas(),
            "copied": self.agents.copied.is_some_and(|at| self.now.saturating_duration_since(at) < COPIED),
        })
    }
}

/// The dashed rim of "no tool" (iced's borders are solid).
struct Dashed;

impl<Message> canvas::Program<Message> for Dashed {
    type State = ();

    fn draw(
        &self,
        (): &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let path = canvas::Path::rounded_rectangle(
            Point::new(0.75, 0.75),
            Size::new(bounds.width - 1.5, bounds.height - 1.5),
            10.0.into(),
        );
        frame.stroke(
            &path,
            canvas::Stroke {
                style: canvas::Style::Solid(crate::theme::ink(rgba(255, 255, 255, 0.22))),
                width: 1.5,
                line_dash: canvas::LineDash {
                    segments: &[5.0, 4.0],
                    offset: 0,
                },
                ..canvas::Stroke::default()
            },
        );
        vec![frame.into_geometry()]
    }
}

/// The preview's bar: its glass and a few grey lines, the halo's glow in
/// the state's colour around it, and the wave down it while it runs.
struct PreviewBar {
    shown: Option<Played>,
    /// Since the state was chosen; none with reduced motion (or before).
    since: Option<Duration>,
    reduced: bool,
}

impl<Message> canvas::Program<Message> for PreviewBar {
    type State = ();

    fn draw(
        &self,
        (): &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let inset = 7.0;
        let bar = Rectangle::new(
            Point::new(inset, inset),
            Size::new(bounds.width - 2.0 * inset, bounds.height - inset),
        );
        let rounded = |r: Rectangle, radius: f32| {
            canvas::Path::rounded_rectangle(r.position(), r.size(), radius.into())
        };
        // How much of the glow shows: faded in over the wave's time.
        let (glow, wave) = match (self.shown, self.since) {
            (None, _) => (0.0, None),
            (Some(_), None) => (1.0, None),
            (Some(played), Some(t)) => {
                let p = (t.as_secs_f32() / WAVE.as_secs_f32()).min(1.0);
                let eased = 1.0 - (1.0 - p) * (1.0 - p);
                let wave = (played == Played::Waiting && !self.reduced && p < 1.0).then_some(p);
                (eased, wave)
            }
        };
        let colour = self.shown.map_or(Color::TRANSPARENT, |p| rgb(p.colour()));
        let (strength, reach) = match self.shown {
            Some(Played::Done) => (0.5, 5.0),
            _ => (1.0, 7.0),
        };
        // The outer glow: rims growing and fading.
        if glow > 0.0 {
            for i in (1..=6).rev() {
                let grow = reach * i as f32 / 6.0;
                let r = Rectangle::new(
                    Point::new(bar.x - grow, bar.y - grow),
                    Size::new(bar.width + 2.0 * grow, bar.height + 2.0 * grow),
                );
                frame.fill(
                    &rounded(r, 12.0 + grow),
                    fade(colour, 0.07 * strength * glow * (1.0 - i as f32 / 7.0)),
                );
            }
        }
        frame.fill(
            &rounded(bar, 12.0),
            crate::theme::ink(rgba(30, 36, 50, 1.0)),
        );
        frame.fill(
            &rounded(bar, 12.0),
            crate::theme::ink(rgba(255, 255, 255, 0.05)),
        );
        // The clock and a few lines.
        let line = |frame: &mut canvas::Frame, y: f32, w: f32, a: f32| {
            frame.fill(
                &rounded(
                    Rectangle::new(Point::new(bar.x + 6.0, bar.y + y), Size::new(w, 3.0)),
                    1.5,
                ),
                crate::theme::ink(rgba(255, 255, 255, a)),
            );
        };
        line(&mut frame, 9.0, bar.width - 12.0, 0.30);
        line(&mut frame, 17.0, bar.width - 18.0, 0.14);
        line(&mut frame, 32.0, bar.width - 14.0, 0.10);
        line(&mut frame, 39.0, bar.width - 20.0, 0.10);
        line(&mut frame, 54.0, bar.width - 16.0, 0.10);
        line(&mut frame, 61.0, bar.width - 22.0, 0.10);
        // The inner glow and the border.
        if glow > 0.0 {
            frame.stroke(
                &rounded(bar, 12.0),
                canvas::Stroke::default()
                    .with_width(1.0)
                    .with_color(fade(colour, 0.85 * strength * glow)),
            );
            let inner = Rectangle::new(
                Point::new(bar.x + 1.5, bar.y + 1.5),
                Size::new(bar.width - 3.0, bar.height - 3.0),
            );
            frame.stroke(
                &rounded(inner, 10.5),
                canvas::Stroke::default()
                    .with_width(3.0)
                    .with_color(fade(colour, 0.14 * strength * glow)),
            );
        } else {
            frame.stroke(
                &rounded(bar, 12.0),
                canvas::Stroke::default()
                    .with_width(1.0)
                    .with_color(crate::theme::ink(rgba(255, 255, 255, 0.10))),
            );
        }
        // The wave: a band 40 % of the bar's height, from above to below.
        if let Some(p) = wave {
            let band = bar.height * 0.4;
            let top = bar.y - band + (bar.height + band) * p;
            for i in 0..12 {
                let y = top + band * i as f32 / 12.0;
                let mid = 1.0 - ((i as f32 + 0.5) / 6.0 - 1.0).abs();
                let slice = Rectangle::new(
                    Point::new(bar.x + 1.0, y),
                    Size::new(bar.width - 2.0, band / 12.0 + 0.5),
                )
                .intersection(&bar);
                if let Some(slice) = slice {
                    frame.fill_rectangle(slice.position(), slice.size(), fade(colour, 0.38 * mid));
                }
            }
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_family_plays_one_after_the_other_then_rests() {
        let opened = Instant::now();
        let at = |ms| opened + Duration::from_millis(ms);
        // 300 ms apart: the third starts at 600 ms.
        assert_eq!(family_burst(2, Some(opened), None, at(599)), None);
        assert_eq!(
            family_burst(2, Some(opened), None, at(700)),
            Some(Duration::from_millis(100))
        );
        // Each about 2.5 s, the resting one a breath; then still.
        assert!(family_burst(0, Some(opened), None, at(2899)).is_some());
        assert_eq!(family_burst(0, Some(opened), None, at(2900)), None);
        assert!(family_burst(4, Some(opened), None, at(1200 + 4299)).is_some());
        assert_eq!(family_burst(4, Some(opened), None, at(1200 + 4300)), None);
        // The pointer over one plays it again.
        assert_eq!(
            family_burst(1, Some(opened), Some(at(9000)), at(9500)),
            Some(Duration::from_millis(500))
        );
    }
}
