//! The Notifications page (#899): "Ne pas déranger", the bell's own, and
//! "PAR APPLICATION", one row per application that notified, the busiest
//! this week first: its icon, its name, "N cette semaine", and a menu of
//! its four levels, written into `[notifications.apps]` with its toast and
//! its undo. Without any: "Aucune application ne t'a encore notifié."
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

/// The page's own messages.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The senders and their icons, read.
    Read(Box<(NotifySenders, Vec<(String, crate::applications::Picture)>)>),
    Dnd,
    /// An application's menu, opened or closed.
    Menu(String),
    CloseMenu,
    Choose(NotifyLevel),
}

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
                let _ = to.unbounded_send(Message::Notifications(Msg::Read(Box::new((
                    senders, icons,
                )))));
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
                let (senders, icons) = *read;
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
                card_box(list.into()),
            ]
            .spacing(8),
        );
        container(page)
            .id(format!("page:{}", SectionId::Notifications.section().slug))
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
                    t(words::this_week(lang, app.week), 11.5, DIM.get()),
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
        let width = at.width.max(MENU_WIDTH);
        let height = 10.0 + 32.0 * 4.0;
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
                })
            })
            .collect();
        serde_json::json!({
            "dnd": self.notifications.dnd,
            "read": self.notifications.senders.is_some(),
            "apps": apps,
            "menu": self.notifications.menu,
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
