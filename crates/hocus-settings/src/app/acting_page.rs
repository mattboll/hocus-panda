//! "Agir sur le bureau" (#440, docs/design/mcp.md, mockup scene 5): a
//! card per agent of `[[agents.mcp]]`, « Regarder | Agir » and its rights
//! as check boxes, the applications it may click and type in as chips;
//! « Voir son journal » (the bar's journal, asked of the compositor) and
//! « Débrancher »; under them `[agents] confirm-apps` as chips,
//! `show-before-ms` as a switch, and « Brancher un autre agent… ».
//!
//! Each change is written into the file at once, with its toast and
//! "Annuler", as the other pages' keys are; the compositor reads the
//! rights at its agents' next call. « Brancher » opens a sheet that says
//! what it will do: nothing is written, nothing run, before its own
//! « Brancher ». For Claude Code it then runs `claude mcp add --scope user
//! hocuspanda -- hocus mcp --agent Claude` (on a thread) when `claude` is
//! on the PATH, and adds the agent here, read-only, once it took it; else
//! the sheet shows that line to paste. « Débrancher » takes the agent out,
//! and out of Claude Code (`claude mcp remove`) when the page put it
//! there: those two have no "Annuler", another program's file following
//! them.

use hocus_words::settings::acting as words;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use iced::{
    Alignment, Background, Color, Length, Padding, Shadow, Theme,
    keyboard::{Key, key::Named},
    widget::{Space, button, column, container, mouse_area, pin, row, stack, text},
};

use super::{
    App, Control, Doing, KeysSaid, Message, Page,
    screens::switch,
    style::*,
    view::{El, card_box, divided, icon, label, t},
};
use crate::{
    acting::{self as model, Agent, Right},
    model::{Icon, SectionId},
    store::KeyEdit,
};

/// The agents' colour (the theme's `agent` token).
const VIOLET: Color = rgb(0xB69CFF);
/// "Copié" stays this long.
const COPIED: Duration = Duration::from_secs(2);

/// The page's own messages.
#[derive(Debug, Clone)]
pub enum Msg {
    /// Agent NAME acts (`true`) or only looks.
    Access(String, bool),
    Right(String, Right),
    /// Its "+ Ajouter" opened or closed.
    AppsMenu(String),
    /// The menu's choice `i`.
    AppAdd(String, usize),
    /// Its application `i` taken out.
    AppRemove(String, usize),
    Journal,
    Unplug(String),
    ConfirmMenu,
    ConfirmAdd(usize),
    ConfirmRemove(usize),
    ShowBefore,
    /// "Brancher…": the sheet opens.
    Plug,
    Client(Client),
    /// The sheet's "Brancher" (`true`) or "Annuler".
    Consent(bool),
    Copy(String),
    CloseMenu,
    /// What `claude` answered.
    Claude(Ran, Result<(), String>),
}

/// What `claude` was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ran {
    /// To add agent NAME's server.
    Added(String),
    /// To remove it.
    Removed(String),
}

/// The client the sheet plugs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Client {
    ClaudeCode,
    Other,
}

impl Client {
    const fn word(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Other => "other",
        }
    }
}

/// A control of the page's own that takes the keyboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    Access(String),
    Right(String, Right),
    Add(String),
    Journal(String),
    Unplug(String),
    ConfirmAdd,
    Show,
    Plug,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Menu {
    Apps(String),
    Confirm,
}

impl Menu {
    fn id(&self) -> String {
        match self {
            Self::Apps(name) => format!("act:{name}:add"),
            Self::Confirm => "act:confirm:add".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Sheet {
    client: Client,
    /// Which button has the keyboard.
    on_plug: bool,
}

/// The page's state.
#[derive(Debug, Default)]
pub struct State {
    sheet: Option<Sheet>,
    menu: Option<(Menu, Instant)>,
    copied: Option<Instant>,
    /// `claude` on the PATH, as the sheet last looked.
    claude: Option<PathBuf>,
    /// `claude` runs.
    running: bool,
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

/// An application's name: its desktop entry's, else its id's last part
/// that names it (`seahorse` of `org.gnome.seahorse.Application`).
fn app_name(id: &str) -> String {
    const NO_NAME: [&str; 4] = ["desktop", "application", "app", "*"];
    crate::apps::named(id).map_or_else(
        || {
            id.rsplit('.')
                .find(|s| !s.is_empty() && !NO_NAME.contains(&s.to_lowercase().as_str()))
                .unwrap_or(id)
                .to_owned()
        },
        |a| a.name.clone(),
    )
}

impl App {
    fn acting(&self) -> model::File {
        self.snapshot
            .as_ref()
            .map(|s| (*s.acting).clone())
            .unwrap_or_default()
    }

    fn acting_shows(&self) -> bool {
        self.page == Page::Section(SectionId::Acting)
    }

    /// The keyboard's stops, in `Tab`'s order.
    pub(super) fn acting_controls(&self) -> Vec<Control> {
        let mut out = Vec::new();
        for agent in self.acting().agents {
            let name = agent.name.clone();
            out.push(Stop::Access(name.clone()));
            for right in Right::ALL {
                out.push(Stop::Right(name.clone(), right));
            }
            out.push(Stop::Add(name.clone()));
            out.push(Stop::Journal(name.clone()));
            out.push(Stop::Unplug(name));
        }
        out.extend([Stop::ConfirmAdd, Stop::Show, Stop::Plug]);
        out.into_iter().map(Control::Acting).collect()
    }

    fn acting_focused(&self, stop: &Stop) -> bool {
        matches!(self.focus, super::Focus::Control(i)
            if self.controls().get(i) == Some(&Control::Acting(stop.clone())))
    }

    // --- What changes it -------------------------------------------------------------

    pub(super) fn acting_update(&mut self, msg: Msg) -> iced::Task<Message> {
        let lang = self.lang;
        let file = self.acting();
        match msg {
            Msg::Access(name, acts) => {
                let Some(agent) = file.agent(&name) else {
                    return iced::Task::none();
                };
                if agent.acts == acts {
                    return iced::Task::none();
                }
                let said = if acts {
                    words::may_act(lang, agent.title())
                } else {
                    words::may_look(lang, agent.title())
                };
                let then = if acts {
                    words::may_look(lang, agent.title())
                } else {
                    words::may_act(lang, agent.title())
                };
                let value = if acts { "\"act\"" } else { "\"read\"" };
                self.acting_write(model::key(&name, "access"), Some(value), said, then);
            }
            Msg::Right(name, right) => {
                let Some(agent) = file.agent(&name).filter(|a| a.acts) else {
                    return iced::Task::none();
                };
                let on = !agent.holds(right);
                let name_said = agent.title();
                let right_said = right.label().get(lang);
                let allowed = words::right_on(
                    lang,
                    words::RightOn {
                        name: &name_said,
                        right: right_said,
                    },
                );
                let taken = words::right_off(
                    lang,
                    words::RightOff {
                        name: &name_said,
                        right: right_said,
                    },
                );
                let (said, then) = if on {
                    (allowed, taken)
                } else {
                    (taken, allowed)
                };
                let value = if on { "true" } else { "false" };
                self.acting_write(model::key(&name, right.key()), Some(value), said, then);
            }
            Msg::AppsMenu(name) => self.acting_menu(Menu::Apps(name)),
            Msg::ConfirmMenu => self.acting_menu(Menu::Confirm),
            Msg::CloseMenu => self.acting.menu = None,
            Msg::AppAdd(name, i) => {
                self.acting.menu = None;
                let Some(agent) = file.agent(&name) else {
                    return iced::Task::none();
                };
                let Some((id, app)) = Self::acting_addable(&agent.apps).into_iter().nth(i) else {
                    return iced::Task::none();
                };
                let mut apps = agent.apps.clone();
                apps.push(id);
                let (said, then) = app_said(&agent.title(), &app, lang);
                self.acting_write(
                    model::key(&name, "apps"),
                    Some(&model::list_toml(&apps)),
                    said,
                    then,
                );
            }
            Msg::AppRemove(name, i) => {
                let Some(agent) = file.agent(&name) else {
                    return iced::Task::none();
                };
                let mut apps = agent.apps.clone();
                if i >= apps.len() {
                    return iced::Task::none();
                }
                let gone = apps.remove(i);
                let (then, said) = app_said(&agent.title(), &app_name(&gone), lang);
                let value = (!apps.is_empty()).then(|| model::list_toml(&apps));
                self.acting_write(model::key(&name, "apps"), value.as_deref(), said, then);
            }
            Msg::ConfirmAdd(i) => {
                self.acting.menu = None;
                let Some((id, app)) = Self::acting_addable(&file.confirm_apps).into_iter().nth(i)
                else {
                    return iced::Task::none();
                };
                let mut apps = file.confirm_apps.clone();
                apps.push(id);
                self.acting_write(
                    confirm_key(),
                    Some(&model::list_toml(&apps)),
                    words::confirm_added(lang, &app),
                    words::confirm_removed(lang, &app),
                );
            }
            Msg::ConfirmRemove(i) => {
                let mut apps = file.confirm_apps.clone();
                if i >= apps.len() {
                    return iced::Task::none();
                }
                let app = app_name(&apps.remove(i));
                self.acting_write(
                    confirm_key(),
                    Some(&model::list_toml(&apps)),
                    words::confirm_removed(lang, &app),
                    words::confirm_added(lang, &app),
                );
            }
            Msg::ShowBefore => {
                let on = file.show_before_ms == 0;
                let value = if on {
                    model::default_show_before().to_string()
                } else {
                    "0".to_owned()
                };
                let (said, then) = if on {
                    (words::SHOWN_BEFORE, words::NOT_SHOWN_BEFORE)
                } else {
                    (words::NOT_SHOWN_BEFORE, words::SHOWN_BEFORE)
                };
                self.acting_write(
                    vec!["agents".into(), "show-before-ms".into()],
                    Some(&value),
                    said.get(lang).to_owned(),
                    then.get(lang).to_owned(),
                );
            }
            Msg::Journal => {
                let asked = self.screens_ask("mcp-journal".to_owned());
                tracing::info!(asked, "the agents' journal asked");
            }
            Msg::Plug => {
                if !self.writable() {
                    return iced::Task::none();
                }
                self.acting.claude = std::env::var_os("PATH").and_then(|p| model::find_claude(&p));
                let client = if file.agents.iter().any(Agent::in_claude_code) {
                    Client::Other
                } else {
                    Client::ClaudeCode
                };
                tracing::debug!(
                    client = client.word(),
                    claude = self.acting.claude.is_some(),
                    "the sheet opens"
                );
                self.acting.sheet = Some(Sheet {
                    client,
                    on_plug: true,
                });
            }
            Msg::Client(client) => {
                let claude_taken = file.agents.iter().any(Agent::in_claude_code);
                if let Some(sheet) = &mut self.acting.sheet
                    && !(client == Client::ClaudeCode && claude_taken)
                {
                    sheet.client = client;
                }
            }
            Msg::Consent(yes) => {
                let Some(sheet) = self.acting.sheet.take() else {
                    return iced::Task::none();
                };
                if yes {
                    return self.acting_plug(sheet.client, &file);
                }
                tracing::debug!("nothing plugged in");
            }
            Msg::Copy(line) => {
                self.acting.copied = Some(Instant::now());
                tracing::debug!("copied");
                return iced::Task::batch([
                    iced::clipboard::write(line),
                    iced::Task::perform(tokio::time::sleep(COPIED), |()| Message::Nothing),
                ]);
            }
            Msg::Unplug(name) => {
                let Some(agent) = file.agent(&name).cloned() else {
                    return iced::Task::none();
                };
                self.acting_keys(
                    vec![KeyEdit {
                        key: model::table_key(&name),
                        value: None,
                    }],
                    words::unplugged(lang, agent.title()),
                );
                if agent.in_claude_code() {
                    let claude = std::env::var_os("PATH").and_then(|p| model::find_claude(&p));
                    if let Some(claude) = claude {
                        self.acting_run(claude, model::claude_remove(), Ran::Removed(name));
                    }
                }
            }
            Msg::Claude(ran, result) => {
                self.acting.running = false;
                match (ran, result) {
                    (Ran::Added(name), Ok(())) => {
                        tracing::info!(agent = name, "Claude Code took it");
                        let agent = Agent {
                            name,
                            client: Some(model::CLAUDE_CODE.to_owned()),
                            since: Some(model::today()),
                            ..Agent::default()
                        };
                        self.acting_add(&agent);
                    }
                    (Ran::Removed(name), Ok(())) => {
                        tracing::info!(agent = name, "Claude Code let it go");
                    }
                    (ran, Err(why)) => {
                        tracing::warn!(?ran, why, "claude refused");
                        let text = words::claude_refused(lang, &why);
                        return self.toast(text, super::ToastAction::None, super::TOAST);
                    }
                }
            }
        }
        iced::Task::none()
    }

    fn acting_menu(&mut self, menu: Menu) {
        let same = self.acting.menu.as_ref().is_some_and(|(m, _)| *m == menu);
        self.acting.menu = (!same && self.writable()).then(|| (menu, Instant::now()));
    }

    /// The applications installed that `listed` does not hold, by name.
    fn acting_addable(listed: &[String]) -> Vec<(String, String)> {
        crate::apps::list()
            .iter()
            .filter(|a| !listed.iter().any(|l| l.eq_ignore_ascii_case(&a.id)))
            .map(|a| (a.id.clone(), a.name.clone()))
            .collect()
    }

    /// One key written, its toast saying `said`, its undo `then`.
    fn acting_write(&mut self, key: Vec<String>, value: Option<&str>, said: String, then: String) {
        let lang = self.lang;
        let keys_said = KeysSaid {
            undone: words::undone(lang, &then),
            redone: words::redone(lang, &said),
            done: said,
            row: model::CARDS,
        };
        self.ask_keys(
            vec![KeyEdit {
                key,
                value: value.map(ToOwned::to_owned),
            }],
            keys_said,
            Doing::New,
            None,
        );
    }

    /// An agent plugged in or out: written without "Annuler".
    fn acting_keys(&mut self, edits: Vec<KeyEdit>, said: String) {
        let keys_said = KeysSaid {
            undone: said.clone(),
            redone: said.clone(),
            done: said,
            row: model::CARDS,
        };
        self.ask_keys(edits, keys_said, Doing::Once, None);
    }

    fn acting_add(&mut self, agent: &Agent) {
        tracing::info!(agent = agent.name, "an agent plugged in");
        self.acting_keys(
            vec![KeyEdit {
                key: model::table_key(&agent.name),
                value: Some(agent.toml()),
            }],
            words::plugged(self.lang, agent.title()),
        );
    }

    /// The sheet's "Brancher": Claude Code asked first when it is there.
    fn acting_plug(&mut self, client: Client, file: &model::File) -> iced::Task<Message> {
        match client {
            Client::ClaudeCode => {
                let name = model::CLAUDE.to_owned();
                match self.acting.claude.clone() {
                    Some(claude) => {
                        self.acting_run(claude, model::claude_add(&name), Ran::Added(name));
                    }
                    None => {
                        let agent = Agent {
                            name,
                            client: Some(model::CLAUDE_CODE.to_owned()),
                            since: Some(model::today()),
                            ..Agent::default()
                        };
                        self.acting_add(&agent);
                    }
                }
            }
            Client::Other => {
                let agent = Agent {
                    name: file.new_name(),
                    since: Some(model::today()),
                    ..Agent::default()
                };
                self.acting_add(&agent);
            }
        }
        iced::Task::none()
    }

    /// `claude args` on a thread; its answer comes back as a message.
    fn acting_run(&mut self, claude: PathBuf, args: Vec<String>, ran: Ran) {
        let Some(to) = self.acting.to.clone() else {
            return;
        };
        tracing::info!(?args, "claude asked");
        self.acting.running = true;
        let started = std::thread::Builder::new()
            .name("claude mcp".to_owned())
            .spawn(move || {
                let result = model::run_claude(&claude, &args);
                let _ = to.unbounded_send(Message::Acting(Msg::Claude(ran, result)));
            });
        if let Err(err) = started {
            self.acting.running = false;
            tracing::warn!(%err, "claude is not run");
        }
    }

    /// The sheet or a menu takes the keyboard: `Esc` closes it, `Enter`
    /// acts, `Tab` and the arrows go from one button to the other.
    pub(super) fn acting_key_first(&mut self, key: &Key) -> Option<iced::Task<Message>> {
        if self.acting.menu.is_some() && key.as_ref() == Key::Named(Named::Escape) {
            self.acting.menu = None;
            return Some(iced::Task::none());
        }
        let sheet = self.acting.sheet.as_mut()?;
        match key.as_ref() {
            Key::Named(Named::Escape) => return Some(self.acting_update(Msg::Consent(false))),
            Key::Named(Named::Enter | Named::Space) => {
                let yes = sheet.on_plug;
                return Some(self.acting_update(Msg::Consent(yes)));
            }
            Key::Named(Named::Tab | Named::ArrowLeft | Named::ArrowRight) => {
                sheet.on_plug = !sheet.on_plug;
            }
            _ => {}
        }
        Some(iced::Task::none())
    }

    pub(super) fn acting_key(&mut self, stop: &Stop, named: Named) -> iced::Task<Message> {
        if !matches!(named, Named::Space | Named::Enter) {
            return iced::Task::none();
        }
        let file = self.acting();
        let msg = match stop {
            Stop::Access(name) => {
                let acts = file.agent(name).is_some_and(|a| a.acts);
                Msg::Access(name.clone(), !acts)
            }
            Stop::Right(name, right) => Msg::Right(name.clone(), *right),
            Stop::Add(name) => Msg::AppsMenu(name.clone()),
            Stop::Journal(_) => Msg::Journal,
            Stop::Unplug(name) => Msg::Unplug(name.clone()),
            Stop::ConfirmAdd => Msg::ConfirmMenu,
            Stop::Show => Msg::ShowBefore,
            Stop::Plug => Msg::Plug,
        };
        self.acting_update(msg)
    }

    pub(super) fn acting_moving(&self, now: Instant) -> bool {
        !self.reduced_motion
            && self
                .acting
                .menu
                .as_ref()
                .is_some_and(|(_, since)| now.saturating_duration_since(*since) < super::MENU_IN)
    }

    // --- What it draws ---------------------------------------------------------------

    pub(super) fn acting_page<'a>(&'a self, mut page: iced::widget::Column<'a, Message>) -> El<'a> {
        let lang = self.lang;
        let file = self.acting();
        if file.agents.is_empty() {
            page = page.push(card_box(
                container(t(words::NO_AGENT.get(lang), 13.0, DIM.get()))
                    .padding([14, 16])
                    .id("act:none".to_owned())
                    .into(),
            ));
        }
        for agent in &file.agents {
            page = page.push(self.acting_card(agent));
        }
        page = page.push(
            column![
                container(label(words::CONFIRM_GROUP.get(lang)))
                    .height(28)
                    .center_y(28),
                self.acting_chips(
                    &file.confirm_apps,
                    Msg::ConfirmRemove,
                    Msg::ConfirmMenu,
                    "act:confirm",
                    true,
                ),
            ]
            .spacing(6),
        );
        let show_on = file.show_before_ms > 0;
        let show_line = words::show_before_line(
            lang,
            model::seconds_said(
                if show_on {
                    file.show_before_ms
                } else {
                    model::default_show_before()
                },
                lang,
            ),
        );
        let show = self.acting_row(
            model::SHOW,
            words::SHOW_BEFORE.get(lang),
            show_line,
            container(switch(
                show_on,
                !self.writable(),
                self.acting_focused(&Stop::Show),
                1.0,
                Some(Message::Acting(Msg::ShowBefore)),
            ))
            .id("act:show".to_owned())
            .into(),
        );
        let plug = self.acting_row(
            model::PLUG,
            words::PLUG_ANOTHER.get(lang),
            words::PLUG_ANOTHER_LINE.get(lang).to_owned(),
            container(self.acting_button(
                words::PLUG_BUTTON.get(lang),
                false,
                self.acting_focused(&Stop::Plug),
                self.writable().then_some(Message::Acting(Msg::Plug)),
            ))
            .id("act:plug".to_owned())
            .into(),
        );
        page = page.push(card_box(column![show, divided(true, plug)].into()));
        container(page).id("page:act-on-desktop".to_owned()).into()
    }

    /// A row of the last card: its name, its line, its control.
    fn acting_row<'a>(
        &self,
        id: &'static str,
        name: &'static str,
        line: String,
        control: El<'a>,
    ) -> El<'a> {
        let lit = self.lit(id);
        container(
            row![
                column![
                    t(name, 14.0, TEXT.get()).font(SEMIBOLD),
                    t(line, 12.5, DIM.get()),
                ]
                .spacing(2)
                .width(Length::Fill),
                control,
            ]
            .spacing(16)
            .align_y(Alignment::Center),
        )
        .padding([12, 16])
        .style(move |_: &Theme| container::Style {
            background: (lit > 0.0).then_some(Background::Color(
                crate::theme::Ink::Accent(0.20 * lit).get(),
            )),
            border: border(11.0, 1.5, crate::theme::Ink::Accent(0.85 * lit).get()),
            ..container::Style::default()
        })
        .id(format!("row:{id}"))
        .into()
    }

    /// An agent's mark: its initial in violet on a soft violet round.
    fn acting_mark<'a>(initial: String, size: f32) -> El<'a> {
        container(text(initial).size(size * 0.45).font(BOLD).color(VIOLET))
            .width(size)
            .height(size)
            .center(size)
            .style(boxed(
                fade(VIOLET, 0.16),
                size / 2.0,
                Some((1.0, fade(VIOLET, 0.55))),
            ))
            .into()
    }

    fn acting_card<'a>(&'a self, agent: &Agent) -> El<'a> {
        let lang = self.lang;
        let name = agent.name.clone();
        let writable = self.writable();
        // « Regarder | Agir ».
        let seg_focused = self.acting_focused(&Stop::Access(name.clone()));
        let option = |acts: bool, said: &'static str| -> El<'a> {
            let chosen = agent.acts == acts;
            let mut b =
                button(t(said, 12.5, if chosen { TEXT.get() } else { DIM.get() }).font(SEMIBOLD))
                    .height(28)
                    .padding([5, 12])
                    .style(move |_, _| button::Style {
                        background: chosen.then_some(Background::Color(crate::theme::ink(rgba(
                            255, 255, 255, 0.13,
                        )))),
                        text_color: TEXT.get(),
                        border: border(7.0, 0.0, Color::TRANSPARENT),
                        ..button::Style::default()
                    });
            if writable {
                b = b.on_press(Message::Acting(Msg::Access(name.clone(), acts)));
            }
            container(b)
                .id(format!("act:{name}:{}", if acts { "act" } else { "look" }))
                .into()
        };
        let segmented = container(row![
            option(false, words::LOOK.get(lang)),
            option(true, words::ACT.get(lang))
        ])
        .padding(3)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(crate::theme::ink(rgba(
                255, 255, 255, 0.06,
            )))),
            border: if seg_focused {
                ring(9.0)
            } else {
                border(9.0, 0.0, Color::TRANSPARENT)
            },
            ..container::Style::default()
        });
        let top = row![
            Self::acting_mark(agent.initial(), 30.0),
            column![
                t(agent.title(), 14.0, TEXT.get()).font(BOLD),
                t(model::card_line(agent, lang), 12.0, FAINT.get()),
            ]
            .spacing(2)
            .width(Length::Fill),
            segmented,
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        // The rights, two a line.
        let check = |right: Right| -> El<'a> {
            let on = agent.holds(right);
            let inert = !agent.acts || !writable;
            let alpha = if agent.acts { 1.0 } else { INERT };
            let focused = self.acting_focused(&Stop::Right(name.clone(), right));
            let tick: El<'a> = if on {
                container(icon(Icon::Check, 12.0, fade(DEEP.get(), alpha)))
                    .width(16)
                    .height(16)
                    .center(16)
                    .style(boxed(fade(ACCENT.get(), alpha), 4.0, None))
                    .into()
            } else {
                container(Space::new().width(16).height(16))
                    .style(boxed(
                        Color::TRANSPARENT,
                        4.0,
                        Some((1.5, fade(FAINT.get(), alpha))),
                    ))
                    .into()
            };
            let mut words_col =
                column![t(right.label().get(lang), 12.5, fade(TEXT.get(), alpha)).line_height(1.4)];
            if let Some(line) = right.line() {
                words_col = words_col.push(t(line.get(lang), 11.5, fade(FAINT.get(), alpha)));
            }
            let mut b = button(
                row![
                    container(tick).padding(Padding::default().top(1)),
                    words_col.width(Length::Fill)
                ]
                .spacing(8),
            )
            .width(Length::Fill)
            .padding([6, 8])
            .style(move |_, status| button::Style {
                background: Some(Background::Color(crate::theme::ink(rgba(
                    255,
                    255,
                    255,
                    if matches!(status, button::Status::Hovered) && !inert {
                        0.06
                    } else {
                        0.03
                    },
                )))),
                text_color: TEXT.get(),
                border: if focused {
                    ring(8.0)
                } else {
                    border(8.0, 0.0, Color::TRANSPARENT)
                },
                ..button::Style::default()
            });
            if !inert {
                b = b.on_press(Message::Acting(Msg::Right(name.clone(), right)));
            }
            container(b)
                .width(Length::FillPortion(1))
                .id(format!("act:{name}:{}", right.key()))
                .into()
        };
        let checks = column![
            row![check(Right::Windows), check(Right::Desktops)].spacing(6),
            row![check(Right::Settings), check(Right::Input)].spacing(6),
        ]
        .spacing(6);
        let apps = column![
            label(words::APPS_LABEL.get(lang)),
            self.acting_chips(
                &agent.apps,
                {
                    let name = name.clone();
                    move |i| Msg::AppRemove(name.clone(), i)
                },
                Msg::AppsMenu(name.clone()),
                &format!("act:{name}"),
                true,
            ),
        ]
        .spacing(6);
        let buttons = row![
            container(self.acting_button(
                words::SEE_JOURNAL.get(lang),
                false,
                self.acting_focused(&Stop::Journal(name.clone())),
                Some(Message::Acting(Msg::Journal)),
            ))
            .id(format!("act:{name}:journal")),
            container(self.acting_button(
                words::UNPLUG.get(lang),
                false,
                self.acting_focused(&Stop::Unplug(name.clone())),
                writable.then(|| Message::Acting(Msg::Unplug(name.clone()))),
            ))
            .id(format!("act:{name}:unplug")),
        ]
        .spacing(8);
        let lit = self.lit(model::CARDS);
        container(column![top, checks, apps, buttons].spacing(12))
            .width(Length::Fill)
            .padding(14)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(if lit > 0.0 {
                    crate::theme::Ink::Accent(0.20 * lit).get()
                } else {
                    CARD.get()
                })),
                border: border(
                    12.0,
                    1.0,
                    if lit > 0.0 {
                        crate::theme::Ink::Accent(0.85 * lit).get()
                    } else {
                        CARD_RIM.get()
                    },
                ),
                ..container::Style::default()
            })
            .id(format!("act:{name}"))
            .into()
    }

    /// Applications as chips, each with its ✕, then "+ Ajouter".
    fn acting_chips<'a>(
        &self,
        apps: &[String],
        remove: impl Fn(usize) -> Msg,
        add: Msg,
        id: &str,
        live: bool,
    ) -> El<'a> {
        let lang = self.lang;
        let live = live && self.writable();
        let mut chips = row![].spacing(6).align_y(Alignment::Center);
        for (i, app) in apps.iter().enumerate() {
            let mut x = button(icon(Icon::Close, 11.0, DIM.get()))
                .width(20)
                .height(20)
                .padding(4)
                .style(flat(None, HOVER.get(), 10.0, false));
            if live {
                x = x.on_press(Message::Acting(remove(i)));
            }
            chips = chips.push(
                container(
                    row![
                        t(app_name(app), 12.0, TEXT.get()),
                        container(x).id(format!("{id}:chip:{i}:remove")),
                    ]
                    .spacing(2)
                    .align_y(Alignment::Center),
                )
                .height(26)
                .padding(Padding {
                    top: 0.0,
                    right: 3.0,
                    bottom: 0.0,
                    left: 10.0,
                })
                .center_y(26)
                .style(boxed(
                    crate::theme::ink(rgba(255, 255, 255, 0.09)),
                    13.0,
                    Some((1.0, crate::theme::ink(rgba(255, 255, 255, 0.07)))),
                ))
                .id(format!("{id}:chip:{i}")),
            );
        }
        let mut plus = button(
            container(t(words::ADD.get(lang), 12.0, FAINT.get()))
                .height(Length::Fill)
                .center_y(Length::Fill),
        )
        .height(26)
        .padding([0, 10])
        .style(move |_, status| button::Style {
            background: matches!(status, button::Status::Hovered)
                .then(|| Background::Color(HOVER.get())),
            text_color: FAINT.get(),
            border: border(13.0, 1.0, crate::theme::ink(rgba(255, 255, 255, 0.16))),
            ..button::Style::default()
        });
        if live {
            plus = plus.on_press(Message::Acting(add));
        }
        chips = chips.push(container(plus).id(format!("{id}:add")));
        chips.wrap().into()
    }

    /// A button: green or secondary; inert without `press`.
    fn acting_button<'a>(
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
            (crate::theme::ink(rgba(255, 255, 255, 0.09)), TEXT.get())
        };
        let mut b = button(t(said, 12.5, fade(ink, alpha)).font(BOLD))
            .height(30)
            .padding([6, 12])
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
                    } else if green {
                        border(8.0, 0.0, Color::TRANSPARENT)
                    } else {
                        border(8.0, 1.0, crate::theme::ink(rgba(255, 255, 255, 0.07)))
                    },
                    ..button::Style::default()
                }
            });
        if let Some(press) = press {
            b = b.on_press(press);
        }
        b.into()
    }

    /// The choices of the open menu: (id, name).
    fn acting_choices(&self, menu: &Menu) -> Vec<(String, String)> {
        let file = self.acting();
        match menu {
            Menu::Apps(name) => {
                Self::acting_addable(&file.agent(name).map(|a| a.apps.clone()).unwrap_or_default())
            }
            Menu::Confirm => Self::acting_addable(&file.confirm_apps),
        }
    }

    /// "+ Ajouter"'s menu, under its button.
    pub(super) fn acting_menu_list(&self) -> Option<El<'_>> {
        if !self.acting_shows() {
            return None;
        }
        let (menu, since) = self.acting.menu.as_ref()?;
        let at = *self.places.at.get(&menu.id())?;
        let opened = if self.reduced_motion {
            1.0
        } else {
            super::view::ease_out(self.now.saturating_duration_since(*since), super::MENU_IN)
        };
        let choices = self.acting_choices(menu);
        let mut list = column![];
        for (i, (_, app)) in choices.iter().enumerate() {
            let msg = match menu {
                Menu::Apps(name) => Msg::AppAdd(name.clone(), i),
                Menu::Confirm => Msg::ConfirmAdd(i),
            };
            list = list.push(
                container(
                    button(
                        container(t(app.clone(), 13.5, fade(TEXT.get(), opened)))
                            .height(Length::Fill)
                            .center_y(Length::Fill),
                    )
                    .width(Length::Fill)
                    .height(32)
                    .padding([0, 10])
                    .on_press(Message::Acting(msg))
                    .style(flat(
                        None,
                        crate::theme::ink(rgba(255, 255, 255, 0.07)),
                        7.0,
                        false,
                    )),
                )
                .id(format!("option:{i}")),
            );
        }
        if choices.is_empty() {
            list = list.push(
                container(t(words::NO_APP_TO_ADD.get(self.lang), 13.0, FAINT.get()))
                    .padding([8, 10]),
            );
        }
        let rows = choices.len().clamp(1, 10) as f32;
        let height = 10.0 + 32.0 * rows;
        let panel = container(iced::widget::scrollable(list).height(height - 10.0))
            .width(240)
            .padding(5)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(fade(MENU.get(), opened))),
                border: border(10.0, 1.0, fade(RIM_12.get(), opened)),
                shadow: Shadow {
                    color: rgba(0, 0, 0, 0.45 * opened),
                    ..shadow()
                },
                ..container::Style::default()
            })
            .id("menu".to_owned());
        let bottom = self
            .places
            .at
            .get(&format!(
                "sidebar:{}",
                hocus_words::settings::app_desktops_rules::EN.get(self.lang)
            ))
            .map_or(f32::MAX, |s| s.y + s.height);
        let under = at.y + at.height + 6.0;
        let y = if under + height > bottom - 8.0 {
            (at.y - 6.0 - height).max(8.0) + 4.0 * (1.0 - opened)
        } else {
            under - 4.0 * (1.0 - opened)
        };
        Some(
            stack![
                mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                    .on_press(Message::Acting(Msg::CloseMenu)),
                pin(panel).x(at.x.max(0.0)).y(y),
            ]
            .into(),
        )
    }

    /// The sheet, over the window at 60 %.
    pub(super) fn acting_sheet(&self) -> Option<El<'_>> {
        let sheet = self.acting.sheet?;
        let lang = self.lang;
        let file = self.acting();
        let claude_taken = file.agents.iter().any(Agent::in_claude_code);
        let choice = |client: Client, said: &'static str| -> El<'_> {
            let chosen = sheet.client == client;
            let usable = !(client == Client::ClaudeCode && claude_taken);
            let mut b = button(
                t(
                    said,
                    12.5,
                    fade(
                        if chosen { TEXT.get() } else { DIM.get() },
                        if usable { 1.0 } else { INERT },
                    ),
                )
                .font(SEMIBOLD),
            )
            .height(28)
            .padding([5, 12])
            .style(move |_, _| button::Style {
                background: chosen.then_some(Background::Color(crate::theme::ink(rgba(
                    255, 255, 255, 0.13,
                )))),
                text_color: TEXT.get(),
                border: border(7.0, 0.0, Color::TRANSPARENT),
                ..button::Style::default()
            });
            if usable {
                b = b.on_press(Message::Acting(Msg::Client(client)));
            }
            container(b)
                .id(format!("sheet:client:{}", client.word()))
                .into()
        };
        let clients = container(row![
            choice(Client::ClaudeCode, words::CLAUDE_CODE.get(lang)),
            choice(Client::Other, words::OTHER_CLIENT.get(lang)),
        ])
        .padding(3)
        .style(boxed(
            crate::theme::ink(rgba(255, 255, 255, 0.06)),
            9.0,
            None,
        ));
        let copied = self
            .acting
            .copied
            .is_some_and(|at| self.now.saturating_duration_since(at) < COPIED);
        let copy_box = |head: &'static str, shown: String| -> El<'_> {
            let copy = container(
                button(
                    t(
                        if copied {
                            words::COPIED.get(lang)
                        } else {
                            words::COPY.get(lang)
                        },
                        12.5,
                        if copied { MINT.get() } else { TEXT.get() },
                    )
                    .font(SEMIBOLD),
                )
                .padding([5, 10])
                .on_press(Message::Acting(Msg::Copy(shown.clone())))
                .style(flat(
                    Some(crate::theme::ink(rgba(255, 255, 255, 0.06))),
                    crate::theme::ink(rgba(255, 255, 255, 0.10)),
                    7.0,
                    false,
                )),
            )
            .id("sheet:copy".to_owned());
            column![
                t(head, 11.0, FAINT.get()).font(BOLD),
                container(
                    row![
                        container(t(shown, 11.5, TEXT.get()).font(MONO)).width(Length::Fill),
                        copy,
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .padding([6, 10])
                .style(boxed(crate::theme::ink(rgba(0, 0, 0, 0.25)), 8.0, None))
                .id("sheet:line".to_owned()),
            ]
            .spacing(6)
            .into()
        };
        let body: El<'_> = match sheet.client {
            Client::ClaudeCode => {
                let mut body =
                    column![t(words::CLAUDE_WHAT.get(lang), 13.0, SOFT.get()).line_height(1.5)]
                        .spacing(14);
                if self.acting.claude.is_none() {
                    body = body.push(t(words::NO_CLAUDE.get(lang), 12.5, rgb(0xF5C46E)));
                }
                body.push(copy_box(
                    words::IT_RUNS.get(lang),
                    model::claude_line(model::CLAUDE),
                ))
                .into()
            }
            Client::Other => {
                let name = file.new_name();
                column![
                    t(words::other_what(lang, &name), 13.0, SOFT.get()).line_height(1.5),
                    copy_box(words::ITS_SERVER.get(lang), model::json_snippet(&name)),
                ]
                .spacing(14)
                .into()
            }
        };
        let buttons = row![
            Space::new().width(Length::Fill),
            container(self.acting_button(
                words::CANCEL.get(lang),
                false,
                !sheet.on_plug,
                Some(Message::Acting(Msg::Consent(false))),
            ))
            .id("sheet:cancel".to_owned()),
            container(self.acting_button(
                words::PLUG_IN.get(lang),
                true,
                sheet.on_plug,
                (!self.acting.running).then_some(Message::Acting(Msg::Consent(true))),
            ))
            .id("sheet:plug".to_owned()),
        ]
        .spacing(8);
        let panel = container(
            column![
                row![
                    Self::acting_mark("+".to_owned(), 32.0),
                    text(words::SHEET_TITLE.get(lang))
                        .size(17)
                        .font(BOLD)
                        .color(TEXT),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
                clients,
                body,
                buttons,
            ]
            .spacing(16),
        )
        .width(460)
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
                container(panel)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center(Length::Fill),
            ]
            .into(),
        )
    }

    /// What the page shows, for the log.
    pub(super) fn acting_shown(&self) -> serde_json::Value {
        let lang = self.lang;
        let file = self.acting();
        let agents: Vec<serde_json::Value> = file
            .agents
            .iter()
            .map(|a| {
                serde_json::json!({
                    "name": a.name,
                    "title": a.title(),
                    "line": model::card_line(a, lang),
                    "access": if a.acts { "act" } else { "look" },
                    "rights": Right::ALL
                        .iter()
                        .filter(|r| a.holds(**r))
                        .map(|r| r.key())
                        .collect::<Vec<_>>(),
                    "apps": a.apps.iter().map(|id| app_name(id)).collect::<Vec<_>>(),
                    "client": a.client,
                })
            })
            .collect();
        serde_json::json!({
            "agents": agents,
            "confirm": file.confirm_apps.iter().map(|id| app_name(id)).collect::<Vec<_>>(),
            "show_before": file.show_before_ms > 0,
            "sheet": self.acting.sheet.map(|s| serde_json::json!({
                "client": s.client.word(),
                "on": if s.on_plug { "plug" } else { "cancel" },
                "claude": self.acting.claude.is_some(),
                "line": model::claude_line(model::CLAUDE),
            })),
            "menu": self.acting.menu.as_ref().map(|(m, _)| m.id()),
            "choices": self.acting.menu.as_ref().map(|(m, _)| {
                self.acting_choices(m).into_iter().map(|(_, n)| n).collect::<Vec<_>>()
            }),
            "running": self.acting.running,
        })
    }
}

/// "NAME peut cliquer et taper dans APP", and its other side.
fn app_said(name: &str, app: &str, lang: crate::lang::Lang) -> (String, String) {
    (
        words::app_added(lang, words::AppAdded { app, name }),
        words::app_removed(lang, words::AppRemoved { app, name }),
    )
}

/// `[agents] confirm-apps`.
fn confirm_key() -> Vec<String> {
    vec!["agents".into(), "confirm-apps".into()]
}
