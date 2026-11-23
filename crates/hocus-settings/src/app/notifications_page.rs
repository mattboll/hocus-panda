//! The Notifications page (#899): "Ne pas déranger", the bell's own, and
//! "PAR APPLICATION", one row per application that notified, the busiest
//! this week first: its icon, its name, "N cette semaine", and a menu of
//! its four levels, written into `[notifications.apps]` with its toast and
//! its undo; under them a check box, « Vues dès que je l'ouvre » (#923,
//! `[notifications.seen-on-focus]`), the row saying "pas vues à
//! l'ouverture" while it is off. Without any: "Aucune application ne t'a
//! encore notifié." Then "Journal des 30 derniers jours", the shell's
//! file, and « Ouvrir », inert while there is no file.
//!
//! The senders are read from the shell's file as the page opens, off the
//! window's thread, their icons with them. Do not disturb is the shell's:
//! the compositor's line says it (`{"dnd":…}`) and sets it (`dnd on`,
//! `dnd off`); without the line its switch is inert.

use hocus_link::{NotifyLevel, NotifySenders};
use hocus_words::settings::notifications as words;
use std::collections::HashMap;

use iced::{
    Alignment, Background, Length, Padding, Shadow, Theme,
    widget::{Space, button, column, container, image, mouse_area, pin, row, stack, svg, text},
};

use super::captures_page::control_style;
use super::{
    App, Doing, KeysSaid, Message, Page,
    applications::Icon,
    screens::{menu_button, switch},
    style::*,
    view::{El, card_box, divided, icon, label, t},
};
use crate::{
    model::{Icon as Glyph, SectionId},
    notifications::{self as model, AppRow},
    screens::Said,
};

/// What the page keeps.
#[derive(Debug, Default)]
pub struct State {
    /// Whether the user is not to be disturbed, as the compositor last said
    /// it (none yet).
    pub(super) dnd: Option<bool>,
    /// The senders, as last read.
    senders: Option<NotifySenders>,
    /// Their icons, by what they are known by.
    icons: HashMap<String, Icon>,
    /// Read for this visit of the page (read again at the next).
    read: bool,
    /// The application whose menu is open.
    menu: Option<String>,
    /// Whether the notifications' journal is there, as read with the
    /// senders.
    journal: bool,
    to: Option<iced::futures::channel::mpsc::UnboundedSender<Message>>,
}

impl State {
    pub(super) fn with(to: &iced::futures::channel::mpsc::UnboundedSender<Message>) -> Self {
        Self {
            to: Some(to.clone()),
            ..Self::default()
        }
    }
}

/// What the page reads as it opens, off the window's thread.
#[derive(Debug, Clone)]
pub struct Read {
    senders: NotifySenders,
    /// Their icons, by what they are known by.
    icons: Vec<(String, crate::applications::Picture)>,
    /// Whether the notifications' journal is there (#923).
    journal: bool,
}

/// The page's own messages.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The senders, their icons and the journal, read.
    Read(Box<Read>),
    Dnd,
    /// An application's menu, opened or closed.
    Menu(String),
    CloseMenu,
    Choose(NotifyLevel),
    /// The menu's « Vues dès que je l'ouvre » (#923).
    SeenOnOpen,
    /// « Ouvrir » the journal.
    OpenJournal,
}

/// A row of the levels' menu, its check box's, and the rule between.
const MENU_ROW: f32 = 32.0;
const MENU_RULE: f32 = 9.0;

/// The width of the levels' menus.
const MENU_WIDTH: f32 = 200.0;

impl App {
    /// The rows, as the senders and the file say them.
    fn notifications_rows(&self) -> Vec<AppRow> {
        let named = self
            .snapshot
            .as_ref()
            .map(|s| s.notifications.clone())
            .unwrap_or_default();
        match &self.notifications.senders {
            Some(senders) => model::rows(senders, &named, model::today()),
            None => Vec::new(),
        }
    }

    /// Application `id`'s level, as the file says it.
    fn notifications_level(&self, id: &str) -> NotifyLevel {
        self.snapshot
            .as_ref()
            .map_or(NotifyLevel::All, |s| model::level_of(&s.notifications, id))
    }

    /// Whether opening application `id` folds its notifications, as the
    /// file says it.
    fn notifications_seen(&self, id: &str) -> bool {
        self.snapshot
            .as_ref()
            .is_none_or(|s| model::seen_on_open(&s.seen_on_focus, id))
    }

    /// After each message: the senders read as the page opens.
    pub(super) fn notifications_after(&mut self) {
        if self.page != Page::Section(SectionId::Notifications) {
            self.notifications.read = false;
            self.notifications.menu = None;
            return;
        }
        if self.notifications.read {
            return;
        }
        self.notifications.read = true;
        let Some(to) = self.notifications.to.clone() else {
            return;
        };
        let started = std::thread::Builder::new()
            .name("notifications' senders".to_owned())
            .spawn(move || {
                let senders = model::read_senders();
                let rows = model::rows(&senders, &Default::default(), model::today());
                let icons = model::icons(&rows);
                let journal = model::journal_file().is_some_and(|f| f.is_file());
                let _ = to.unbounded_send(Message::Notifications(Msg::Read(Box::new(Read {
                    senders,
                    icons,
                    journal,
                }))));
            });
        if let Err(err) = started {
            tracing::warn!(%err, "the notifications' senders are not read");
        }
    }

    /// The compositor's line said something of the page's: do not disturb.
    pub(super) fn notifications_said(&mut self, said: &Said) -> bool {
        match said {
            Said::Dnd(on) => {
                self.notifications.dnd = Some(*on);
                true
            }
            _ => false,
        }
    }

    pub(super) fn notifications_update(&mut self, msg: Msg) -> iced::Task<Message> {
        match msg {
            Msg::Read(read) => {
                let Read {
                    senders,
                    icons,
                    journal,
                } = *read;
                self.notifications.journal = journal;
                tracing::info!(
                    senders = senders.apps.len(),
                    "the notifications' senders read"
                );
                for (name, picture) in icons {
                    let icon = match picture {
                        crate::applications::Picture::Rgba {
                            width,
                            height,
                            pixels,
                        } => Icon::Image(image::Handle::from_rgba(width, height, pixels)),
                        crate::applications::Picture::Svg(bytes) => {
                            Icon::Svg(svg::Handle::from_memory(bytes))
                        }
                    };
                    self.notifications.icons.insert(name, icon);
                }
                self.notifications.senders = Some(senders);
            }
            Msg::Dnd => {
                let on = self.notifications.dnd.unwrap_or(false);
                let line = if on { "dnd off" } else { "dnd on" };
                if self.screens_ask(line.to_owned()) {
                    tracing::info!(on = !on, "do not disturb, from the Settings");
                    self.notifications.dnd = Some(!on);
                }
            }
            Msg::Menu(id) => {
                self.notifications.menu = if self.notifications.menu.as_deref() == Some(&id) {
                    None
                } else {
                    Some(id)
                };
            }
            Msg::CloseMenu => self.notifications.menu = None,
            // A check box: the menu stays.
            Msg::SeenOnOpen => self.notifications_seen_on_open(),
            Msg::OpenJournal => {
                if let Some(file) = model::journal_file().filter(|f| f.is_file()) {
                    tracing::info!("the notifications' journal is opened");
                    super::open_place(file, false);
                }
            }
            Msg::Choose(level) => {
                let Some(id) = self.notifications.menu.take() else {
                    return iced::Task::none();
                };
                let before = self.notifications_level(&id);
                if before == level {
                    return iced::Task::none();
                }
                let name = self
                    .notifications_rows()
                    .into_iter()
                    .find(|r| r.id == id)
                    .map_or_else(|| id.clone(), |r| r.name);
                let lang = self.lang;
                let say = |l: NotifyLevel| level_name(l, lang);
                let said = KeysSaid {
                    done: words::level_done(
                        lang,
                        words::LevelDone {
                            name: &name,
                            level: say(level),
                        },
                    ),
                    undone: words::level_undone(
                        lang,
                        words::LevelUndone {
                            name: &name,
                            level: say(before),
                        },
                    ),
                    redone: words::level_redone(
                        lang,
                        words::LevelRedone {
                            name: &name,
                            level: say(level),
                        },
                    ),
                    row: "notifications.apps",
                };
                let named = self
                    .snapshot
                    .as_ref()
                    .map(|s| s.notifications.clone())
                    .unwrap_or_default();
                tracing::info!(
                    app = id,
                    level = level.word(),
                    "an application's level chosen"
                );
                self.ask_keys(
                    model::level_keys(&named, &id, level),
                    said,
                    Doing::New,
                    None,
                );
            }
        }
        iced::Task::none()
    }

    /// « Vues dès que je l'ouvre » of the open menu's application flipped,
    /// with its toast and its undo.
    fn notifications_seen_on_open(&mut self) {
        let Some(id) = self.notifications.menu.clone() else {
            return;
        };
        let before = self.notifications_seen(&id);
        let name = self
            .notifications_rows()
            .into_iter()
            .find(|r| r.id == id)
            .map_or_else(|| id.clone(), |r| r.name);
        let lang = self.lang;
        let said = |on: bool| {
            if on {
                words::SEEN_ON_OPEN_LOWER.get(lang)
            } else {
                words::NOT_SEEN_ON_OPEN.get(lang)
            }
        };
        let named = self
            .snapshot
            .as_ref()
            .map(|s| s.seen_on_focus.clone())
            .unwrap_or_default();
        tracing::info!(
            app = id,
            on = !before,
            "an application's seen on opening chosen"
        );
        let said = KeysSaid {
            done: words::seen_done(
                lang,
                words::SeenDone {
                    name: &name,
                    said: said(!before),
                },
            ),
            undone: words::seen_undone(
                lang,
                words::SeenUndone {
                    name: &name,
                    said: said(before),
                },
            ),
            redone: words::seen_redone(
                lang,
                words::SeenRedone {
                    name: &name,
                    said: said(!before),
                },
            ),
            row: model::SEEN_ON_OPEN,
        };
        self.ask_keys(
            model::seen_on_open_keys(&named, &id, !before),
            said,
            Doing::New,
            None,
        );
    }

    pub(super) fn notifications_page<'a>(
        &'a self,
        mut page: iced::widget::Column<'a, Message>,
    ) -> El<'a> {
        let lang = self.lang;
        // Ne pas déranger.
        let dnd = self.notifications.dnd;
        let inert = dnd.is_none() || !self.screens_linked();
        let dnd_row = container(
            row![
                column![
                    t(
                        words::DND.get(lang),
                        14.0,
                        fade(TEXT.get(), if inert { INERT } else { 1.0 })
                    )
                    .font(SEMIBOLD),
                    t(words::DND_LINE.get(lang), 12.5, DIM.get()),
                ]
                .spacing(2)
                .width(Length::Fill),
                Space::new().height(36),
                container(switch(
                    dnd.unwrap_or(false),
                    inert,
                    false,
                    1.0,
                    Some(Message::Notifications(Msg::Dnd)),
                ))
                .id("notifications:dnd".to_owned()),
            ]
            .spacing(16)
            .align_y(Alignment::Center),
        )
        .padding(Padding {
            top: 9.0,
            right: 14.0,
            bottom: 9.0,
            left: 16.0,
        })
        .id(format!("row:{}", model::DND));
        page = page.push(card_box(dnd_row.into()));
        // PAR APPLICATION.
        let rows = self.notifications_rows();
        let mut list = column![];
        for (i, app) in rows.iter().enumerate() {
            list = list.push(divided(i > 0, self.notifications_app_row(app)));
        }
        if rows.is_empty() {
            list = list.push(
                container(t(words::NOBODY.get(lang), 13.0, FAINT.get()))
                    .padding([18, 16])
                    .id("notifications:nobody".to_owned()),
            );
        }
        page = page.push(
            column![
                container(label(words::BY_APP.get(lang)))
                    .height(28)
                    .center_y(28),
                container(card_box(list.into())).id(format!("row:{}", model::SEEN_ON_OPEN)),
            ]
            .spacing(8),
        );
        page = page.push(card_box(self.notifications_journal()));
        container(page)
            .id(format!("page:{}", SectionId::Notifications.section().slug))
            .into()
    }

    /// The journal of the last 30 days (#923): its place, and « Ouvrir »,
    /// inert while there is no file.
    fn notifications_journal<'a>(&self) -> El<'a> {
        let lang = self.lang;
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        let line = model::journal_file()
            .as_deref()
            .map(|f| model::journal_line(f, home.as_deref()))
            .unwrap_or_default();
        let there = self.notifications.journal;
        let alpha = if there { 1.0 } else { INERT };
        let mut open = button(
            container(t(words::OPEN.get(lang), 13.0, fade(TEXT.get(), alpha)).font(SEMIBOLD))
                .height(Length::Fill)
                .center_y(Length::Fill),
        )
        .height(32)
        .padding([0, 12])
        .style(control_style(false));
        if there {
            open = open.on_press(Message::Notifications(Msg::OpenJournal));
        }
        container(
            row![
                column![
                    t(words::JOURNAL.get(lang), 14.0, TEXT.get()).font(SEMIBOLD),
                    t(line, 12.5, DIM.get()),
                ]
                .spacing(2)
                .width(Length::Fill),
                container(open).id("notifications:journal".to_owned()),
            ]
            .spacing(16)
            .align_y(Alignment::Center),
        )
        .padding(Padding {
            top: 9.0,
            right: 14.0,
            bottom: 9.0,
            left: 16.0,
        })
        .id(format!("row:{}", model::JOURNAL))
        .into()
    }

    /// An application's row: its icon, its name, its week, its menu.
    fn notifications_app_row<'a>(&self, app: &AppRow) -> El<'a> {
        let lang = self.lang;
        let level = self.notifications_level(&app.id);
        let open = self.notifications.menu.as_deref() == Some(app.id.as_str());
        let menu = container(menu_button(
            level_name(level, lang).to_owned(),
            !self.writable(),
            false,
            open,
            Some(Message::Notifications(Msg::Menu(app.id.clone()))),
        ))
        .id(format!("notifications:menu:{}", app.id));
        container(
            row![
                self.notifications_icon(app, 26.0),
                column![
                    t(app.name.clone(), 13.5, TEXT.get()).font(SEMIBOLD),
                    t(self.notifications_week(app), 11.5, DIM.get()),
                ]
                .spacing(1)
                .width(Length::Fill),
                menu,
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding(Padding {
            top: 9.0,
            right: 14.0,
            bottom: 9.0,
            left: 14.0,
        })
        .id(format!("notifications:app:{}", app.id))
        .into()
    }

    /// An application's row's line: "9 cette semaine", and "· pas vues à
    /// l'ouverture" while opening it folds nothing (#923).
    fn notifications_week(&self, app: &AppRow) -> String {
        let week = words::this_week(self.lang, app.week);
        if self.notifications_seen(&app.id) {
            week
        } else {
            format!("{week} · {}", words::NOT_SEEN_ON_OPEN.get(self.lang))
        }
    }

    /// An application's icon, `size` wide: from the icon theme, else its
    /// initial on a colour of its own.
    fn notifications_icon<'a>(&self, app: &AppRow, size: f32) -> El<'a> {
        match app
            .icon
            .as_deref()
            .and_then(|i| self.notifications.icons.get(i))
        {
            Some(Icon::Image(handle)) => {
                return image(handle.clone()).width(size).height(size).into();
            }
            Some(Icon::Svg(handle)) => {
                return svg(handle.clone()).width(size).height(size).into();
            }
            None => {}
        }
        let initial: String = app
            .name
            .chars()
            .find(|c| c.is_alphanumeric())
            .map(|c| c.to_uppercase().collect())
            .unwrap_or_default();
        let colour = super::applications::view::tile_colour(&app.id);
        container(
            text(initial)
                .size(size * 0.5)
                .font(BOLD)
                .color(rgb(0x0b1020)),
        )
        .width(size)
        .height(size)
        .center(size)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(colour)),
            border: border(7.0, 0.0, colour),
            ..container::Style::default()
        })
        .into()
    }

    /// The open menu's list, under its button, its right on the button's.
    pub(super) fn notifications_menu_list(&self) -> Option<El<'_>> {
        if self.page != Page::Section(SectionId::Notifications) {
            return None;
        }
        let id = self.notifications.menu.as_deref()?;
        let at = *self.places.at.get(&format!("notifications:menu:{id}"))?;
        let current = self.notifications_level(id);
        let mut list = column![];
        for (i, level) in NotifyLevel::ALL.into_iter().enumerate() {
            let chosen = level == current;
            let check: El<'_> = if chosen {
                icon(Glyph::Check, 13.0, ACCENT.get())
            } else {
                Space::new().width(13).into()
            };
            list = list.push(
                container(
                    button(
                        row![
                            container(check).width(18).center_x(18),
                            t(level_name(level, self.lang), 13.5, TEXT.get()).font(if chosen {
                                SEMIBOLD
                            } else {
                                MANROPE
                            }),
                        ]
                        .spacing(6)
                        .height(Length::Fill)
                        .align_y(Alignment::Center),
                    )
                    .width(Length::Fill)
                    .height(32)
                    .padding([0, 8])
                    .on_press(Message::Notifications(Msg::Choose(level)))
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
        // Under a rule, « Vues dès que je l'ouvre » (#923).
        list = list
            .push(
                container(container(Space::new().width(Length::Fill).height(1)).style(
                    |_: &Theme| container::Style {
                        background: Some(Background::Color(crate::theme::ink(rgba(
                            255, 255, 255, 0.08,
                        )))),
                        ..container::Style::default()
                    },
                ))
                .padding([4, 6]),
            )
            .push(self.notifications_seen_box(self.notifications_seen(id)));
        let width = at.width.max(MENU_WIDTH);
        let height = 10.0 + MENU_ROW * 5.0 + MENU_RULE;
        let panel = container(list)
            .width(width)
            .padding(5)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(MENU.get())),
                border: border(10.0, 1.0, RIM_12.get()),
                shadow: Shadow {
                    color: rgba(0, 0, 0, 0.45),
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
                hocus_words::settings::app_view::EN.get(self.lang)
            ))
            .map_or(f32::MAX, |s| s.y + s.height);
        let under = at.y + at.height + 6.0;
        let y = if under + height > bottom - 8.0 {
            (at.y - 6.0 - height).max(8.0)
        } else {
            under
        };
        let x = (at.x + at.width - width).max(0.0);
        Some(
            stack![
                mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                    .on_press(Message::Notifications(Msg::CloseMenu)),
                pin(panel).x(x).y(y),
            ]
            .into(),
        )
    }

    /// The menu's check box, « Vues dès que je l'ouvre », ticked `on`.
    fn notifications_seen_box<'a>(&self, on: bool) -> El<'a> {
        let tick: El<'a> = if on {
            container(icon(Glyph::Check, 12.0, DEEP.get()))
                .width(16)
                .height(16)
                .center(16)
                .style(move |_: &Theme| container::Style {
                    background: Some(Background::Color(ACCENT.get())),
                    border: border(4.0, 0.0, ACCENT.get()),
                    ..container::Style::default()
                })
                .into()
        } else {
            container(Space::new().width(16).height(16))
                .style(move |_: &Theme| container::Style {
                    border: border(4.0, 1.5, FAINT.get()),
                    ..container::Style::default()
                })
                .into()
        };
        container(
            button(
                row![
                    container(tick).width(18).center_x(18),
                    t(words::SEEN_ON_OPEN.get(self.lang), 13.5, TEXT.get()),
                ]
                .spacing(6)
                .height(Length::Fill)
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .height(MENU_ROW)
            .padding([0, 8])
            .on_press(Message::Notifications(Msg::SeenOnOpen))
            .style(flat(
                None,
                crate::theme::ink(rgba(255, 255, 255, 0.07)),
                7.0,
                false,
            )),
        )
        .id("option:seen-on-open".to_owned())
        .into()
    }

    /// What the page shows, for the log (tests).
    pub(super) fn notifications_shown(&self) -> serde_json::Value {
        let apps: Vec<serde_json::Value> = self
            .notifications_rows()
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.id,
                    "name": r.name,
                    "week": r.week,
                    "level": self.notifications_level(&r.id).word(),
                    "seen_on_open": self.notifications_seen(&r.id),
                })
            })
            .collect();
        serde_json::json!({
            "dnd": self.notifications.dnd,
            "read": self.notifications.senders.is_some(),
            "apps": apps,
            "menu": self.notifications.menu,
            "journal": self.notifications.journal,
        })
    }
}

/// A level's name, as the bubbles' menu says it.
pub(super) fn level_name(level: NotifyLevel, lang: crate::lang::Lang) -> &'static str {
    use hocus_words::shell::notify_levels as levels;
    match level {
        NotifyLevel::All => levels::ALL.get(lang),
        NotifyLevel::Silent => levels::SILENT.get(lang),
        NotifyLevel::History => levels::HISTORY.get(lang),
        NotifyLevel::None => levels::NONE.get(lang),
    }
}
