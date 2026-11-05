//! What the window draws (the approved mockup, variant A): the sidebar
//! with its search, the content's header, the home and the sections'
//! pages, the menus, the toast and the unreadable file's banner.

use std::time::{Duration, Instant, SystemTime};

use iced::{
    Alignment, Background, Border, Color, Element, Gradient, Length, Padding, Shadow, Theme,
    Vector, gradient,
    widget::{
        Space, button, column, container, mouse_area, pin, rich_text, row, scrollable, slider,
        span, stack, svg, text, text_input,
    },
};

use super::{
    App, Focus, Message, Page, SEARCH, ToastAction,
    places::PAGE,
    style::{self, *},
};
use crate::{
    lang::{Lang, w},
    model::{Group, Icon, Kind, SectionId, Setting, Source, settings_of},
    search::Target,
    store::{FileState, Reading, Slot},
};

pub(super) type El<'a> = Element<'a, Message>;

const SIDEBAR_WIDTH: f32 = 240.0;
const HEADER: f32 = 48.0;
pub(super) const PAGE_WIDTH: f32 = 720.0;

/// A text in Manrope.
pub(super) fn t<'a>(
    content: impl text::IntoFragment<'a>,
    size: f32,
    color: Color,
) -> text::Text<'a> {
    text(content).size(size).color(color).font(MANROPE)
}

pub(super) fn icon<'a>(which: Icon, size: f32, color: Color) -> El<'a> {
    svg(style::icon(which, color, 1.8))
        .width(size)
        .height(size)
        .into()
}

impl App {
    pub fn view(&self) -> El<'_> {
        let base = row![self.sidebar(), self.content()];
        let mut layers: Vec<El<'_>> = vec![base.into()];
        if let Some(menu) = self.menu_list().or_else(|| self.sound_menu_list()) {
            layers.push(
                mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                    .on_press(Message::CloseMenu)
                    .into(),
            );
            layers.push(menu);
        }
        // The Écrans page's menu, and its "Garder ?" over everything.
        if let Some(menu) = self.screens_menu_list() {
            layers.push(menu);
        }
        if let Some(toast) = self.toast_view() {
            layers.push(toast);
        }
        if let Some(dialog) = self.screens_dialog() {
            layers.push(dialog);
        }
        stack(layers)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    // --- The sidebar ------------------------------------------------------------------

    fn sidebar(&self) -> El<'_> {
        let lang = self.lang;
        let header = row![
            container(icon(Icon::Sliders, 15.0, ACCENT.get()))
                .width(24)
                .height(24)
                .center(24)
                .style(boxed(crate::theme::Ink::Accent(0.16).get(), 7.0, None)),
            text(self.title()).size(15).font(BOLD).color(TEXT),
        ]
        .spacing(9)
        .align_y(Alignment::Center)
        .height(HEADER)
        .padding(Padding::default().left(16));
        let field = text_input(
            self.say(w("Rechercher un réglage", "Search settings")),
            &self.query,
        )
        .id(SEARCH)
        .on_input(Message::Query)
        .on_submit(Message::Submit)
        .size(13.0)
        .font(MANROPE)
        .padding(Padding {
            top: 8.0,
            bottom: 8.0,
            left: 29.0,
            right: 30.0,
        })
        .style(style::search);
        let focused = self.focus == Focus::Search;
        let aside: El<'_> = if self.query.is_empty() {
            container(text("Ctrl F").size(9.0).font(MONO).color(FAINT))
                .padding([1, 4])
                .style(boxed(
                    Color::TRANSPARENT,
                    4.0,
                    Some((1.0, crate::theme::ink(rgba(255, 255, 255, 0.14)))),
                ))
                .into()
        } else {
            button(icon(Icon::Close, 13.0, DIM.get()))
                .width(24)
                .height(24)
                .padding(5.5)
                .on_press(Message::ClearSearch)
                .style(flat(None, HOVER.get(), 6.0, false))
                .into()
        };
        let search = container(stack![
            field,
            container(icon(Icon::Search, 15.0, DIM.get()))
                .padding(Padding::default().left(9).top(9.5)),
            container(aside)
                .width(Length::Fill)
                .height(34)
                .align_right(Length::Fill)
                .center_y(34)
                .padding(Padding::default().right(4)),
        ])
        .height(34)
        .style(boxed(
            crate::theme::ink(rgba(255, 255, 255, 0.06)),
            9.0,
            Some((
                1.0,
                if focused {
                    crate::theme::Ink::Accent(0.65).get()
                } else {
                    crate::theme::ink(rgba(255, 255, 255, 0.10))
                },
            )),
        ))
        .id("search-field".to_owned());
        let list: El<'_> = if self.query.is_empty() {
            self.nav()
        } else {
            self.found()
        };
        let sidebar = container(column![
            header,
            container(search).padding([0, 12]),
            scrollable(container(list).padding(Padding {
                top: 8.0,
                right: 10.0,
                bottom: 12.0,
                left: 10.0,
            }))
            .height(Length::Fill)
            .style(thin),
        ])
        .width(SIDEBAR_WIDTH - 1.0)
        .height(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(SIDEBAR.get())),
            ..container::Style::default()
        })
        .id(format!("sidebar:{}", if lang.fr() { "fr" } else { "en" }));
        // A 1 px line on its right.
        row![
            sidebar,
            container(Space::new().width(1).height(Length::Fill)).style(boxed(
                LINE.get(),
                0.0,
                None
            )),
        ]
        .into()
    }

    /// One row of the sidebar: its icon, its name.
    fn nav_row(
        &self,
        which: Icon,
        name: &'static str,
        current: bool,
        focus: bool,
        on: Message,
        id: String,
    ) -> El<'_> {
        let ink = if current { ACCENT } else { DIM };
        let words = text(name)
            .size(13.5)
            .font(if current { BOLD } else { MANROPE })
            .color(if current { TEXT } else { SOFT })
            .wrapping(text::Wrapping::None);
        container(
            button(
                row![icon(which, 17.0, ink.get()), words]
                    .spacing(10)
                    .height(Length::Fill)
                    .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .height(30)
            .padding([0, 10])
            .on_press(on)
            .style(flat(
                current.then_some(CURRENT.get()),
                HOVER.get(),
                8.0,
                focus,
            )),
        )
        .id(id)
        .into()
    }

    fn nav(&self) -> El<'_> {
        let mut list = column![self.nav_row(
            Icon::Home,
            self.say(w("Accueil", "Home")),
            self.page == Page::Home,
            self.focus == Focus::Nav(0),
            Message::Home,
            "nav:home".to_owned(),
        )];
        let mut n = 1;
        for group in Group::ALL {
            list = list.push(
                container(
                    text(spaced(group.label().get(self.lang)))
                        .size(10.5)
                        .font(BOLD)
                        .color(FAINT),
                )
                .height(22)
                .padding(Padding::default().left(10).top(9)),
            );
            for id in SectionId::ALL
                .into_iter()
                .filter(|s| s.section().group == group)
            {
                let section = id.section();
                list = list.push(self.nav_row(
                    section.icon,
                    section.name.get(self.lang),
                    self.page == Page::Section(id),
                    self.focus == Focus::Nav(n),
                    Message::Section(id),
                    format!("nav:{}", section.slug),
                ));
                n += 1;
            }
        }
        list.into()
    }

    /// The results in place of the sections.
    fn found(&self) -> El<'_> {
        let lang = self.lang;
        let count = self.results.len();
        let head = if count == 0 {
            match lang {
                Lang::French => format!(
                    "Aucun réglage ne répond à « {} ». Essaie un autre mot : écran, veille, clavier…",
                    self.query.trim()
                ),
                Lang::English => format!(
                    "No setting matches “{}”. Try another word: screen, sleep, keyboard…",
                    self.query.trim()
                ),
            }
        } else {
            match (lang, count) {
                (Lang::French, 1) => "1 résultat · ↑ ↓ puis Entrée".to_owned(),
                (Lang::French, n) => format!("{n} résultats · ↑ ↓ puis Entrée"),
                (Lang::English, 1) => "1 result · ↑ ↓ then Enter".to_owned(),
                (Lang::English, n) => format!("{n} results · ↑ ↓ then Enter"),
            }
        };
        let mut list = column![container(t(head, 11.5, FAINT.get())).padding([4, 10])].spacing(2);
        for (i, found) in self.results.iter().enumerate() {
            let section = found.target.section().section();
            let name: El<'_> = match found.hit {
                Some((at, len)) => {
                    let (pre, rest) = found.name.split_at(at.min(found.name.len()));
                    let (hit, post) = rest.split_at(len.min(rest.len()));
                    let spans: [text::Span<'_, (), iced::Font>; 3] = [
                        span(pre),
                        span(hit)
                            .background(crate::theme::Ink::Accent(0.30).get())
                            .border(border(3.0, 0.0, Color::TRANSPARENT)),
                        span(post),
                    ];
                    rich_text(spans)
                        .size(13.5)
                        .font(SEMIBOLD)
                        .color(TEXT)
                        .into()
                }
                None => text(found.name)
                    .size(13.5)
                    .font(SEMIBOLD)
                    .color(TEXT)
                    .into(),
            };
            let mut path = match found.target {
                Target::Section(_) => w("Toute la page", "The whole page").get(lang).to_owned(),
                Target::Outside(outside) => format!(
                    "{} › {}",
                    section.name.get(lang),
                    capitalised(outside.group.get(lang))
                ),
                Target::Setting(setting) if setting.kind == Kind::Shortcut => format!(
                    "{} › {} › {}",
                    section.name.get(lang),
                    w("Raccourcis", "Shortcuts").get(lang),
                    capitalised(setting.group.label.get(lang))
                ),
                Target::Setting(setting) => format!(
                    "{} › {}",
                    section.name.get(lang),
                    capitalised(setting.group.label.get(lang))
                ),
            };
            if let Some(via) = &found.via {
                path.push_str(&match lang {
                    Lang::French => format!(" · par « {via} »"),
                    Lang::English => format!(" · via “{via}”"),
                });
            }
            let opened = self.opened.as_deref() == Some(found.target.id().as_str());
            let fill = if opened {
                Some(CURRENT.get())
            } else if i == self.selected {
                Some(crate::theme::ink(rgba(255, 255, 255, 0.07)))
            } else {
                None
            };
            list = list.push(
                container(
                    button(
                        row![
                            container(icon(section.icon, 16.0, DIM.get()))
                                .padding(Padding::default().top(2)),
                            column![name, t(path, 11.5, FAINT.get())].spacing(2),
                        ]
                        .spacing(10),
                    )
                    .width(Length::Fill)
                    .padding([7, 10])
                    .on_press(Message::Result(i))
                    .style(flat(fill, HOVER.get(), 8.0, false)),
                )
                .id(format!("result:{i}")),
            );
        }
        list.into()
    }

    // --- The content ------------------------------------------------------------------

    fn content(&self) -> El<'_> {
        let lang = self.lang;
        let here = match self.page {
            Page::Home => self.title(),
            Page::Section(SectionId::Keyboard) if self.keys.sub => format!(
                "{} › {} › {}",
                self.title(),
                SectionId::Keyboard.name(lang),
                w("Raccourcis", "Shortcuts").get(lang)
            ),
            Page::Section(id) => format!("{} › {}", self.title(), id.name(lang)),
        };
        let (dot, state) = match self.snapshot.as_ref().map(|s| &s.state) {
            _ if self.screens_to_confirm() => (
                crate::theme::ink(rgb(0xF5B44A)),
                w("À confirmer", "To confirm"),
            ),
            Some(FileState::Reads) | None => (ACCENT.get(), w("Enregistré", "Saved")),
            Some(FileState::Broken { .. }) => (ORANGE.get(), w("Lecture seule", "Read-only")),
            Some(FileState::NoFile) => (ORANGE.get(), w("Aucun fichier", "No file")),
        };
        let header = mouse_area(
            row![
                t(here, 13.0, FAINT.get()),
                Space::new().width(Length::Fill),
                container(Space::new().width(7).height(7)).style(boxed(dot, 3.5, None)),
                t(state.get(lang), 12.0, dot),
                container(
                    container(
                        button(icon(Icon::Close, 16.0, DIM.get()))
                            .width(30)
                            .height(30)
                            .padding(7)
                            .on_press(Message::Close)
                            .style(flat(None, HOVER.get(), 8.0, false)),
                    )
                    .id("close".to_owned()),
                )
                .padding(Padding::default().left(10)),
            ]
            .spacing(7)
            .align_y(Alignment::Center)
            .height(HEADER)
            .padding(Padding {
                top: 0.0,
                right: 12.0,
                bottom: 0.0,
                left: 32.0,
            }),
        )
        .on_press(Message::Drag);
        let page: El<'_> = match self.page {
            Page::Home => self.home(),
            Page::Section(id) => self.section(id),
        };
        let mut body = column![].spacing(18);
        if let Some(banner) = self.banner() {
            body = body.push(banner);
        }
        body = body.push(page);
        let scrolled = scrollable(container(container(body).max_width(PAGE_WIDTH)).padding(
            Padding {
                top: 6.0,
                right: 40.0,
                bottom: 72.0,
                left: 32.0,
            },
        ))
        .id(PAGE)
        .on_scroll(|_| Message::Scrolled)
        .height(Length::Fill)
        .width(Length::Fill)
        .style(thin);
        container(column![header, scrolled])
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_: &Theme| container::Style {
                background: Some(Background::Color(CONTENT.get())),
                border: Border {
                    // The sidebar's line on its right.
                    width: 0.0,
                    ..Border::default()
                },
                ..container::Style::default()
            })
            .into()
    }

    fn banner(&self) -> Option<El<'_>> {
        let Some(FileState::Broken { line, message }) = self.snapshot.as_ref().map(|s| &s.state)
        else {
            return None;
        };
        let lang = self.lang;
        let title = banner_title(*line, lang);
        let restore = self.restore_label();
        let mut buttons = row![
            button(
                t(
                    w("Ouvrir le fichier", "Open the file").get(lang),
                    12.5,
                    TEXT.get()
                )
                .font(SEMIBOLD)
            )
            .height(28)
            .padding([5, 12])
            .on_press(Message::OpenFile)
            .style(|_, status| solid(
                crate::theme::ink(rgba(255, 255, 255, 0.08)),
                TEXT.get(),
                status
            )),
        ]
        .spacing(8);
        if let Some(restore) = restore {
            buttons = buttons.push(
                container(
                    button(t(restore, 12.5, DEEP.get()).font(BOLD))
                        .height(28)
                        .padding([5, 12])
                        .on_press(Message::Restore)
                        .style(|_, status| solid(ORANGE.get(), DEEP.get(), status)),
                )
                .id("restore".to_owned()),
            );
        }
        Some(
            container(
                row![
                    container(icon(Icon::Warning, 18.0, PEACH.get())).padding(Padding::default().top(1)),
                    column![
                        t(title, 13.5, PEACH.get()).font(BOLD),
                        t(message.clone(), 12.5, SOFT.get()),
                        t(
                            w(
                                "Le bureau garde la dernière configuration lisible, et les Réglages ne changent rien tant que le fichier n’est pas réparé.",
                                "The desktop keeps the last readable configuration, and Settings changes nothing until the file is fixed.",
                            )
                            .get(lang),
                            12.5,
                            SOFT.get(),
                        ),
                        container(buttons).padding(Padding::default().top(6)),
                    ]
                    .spacing(4),
                ]
                .spacing(12),
            )
            .padding([12, 14])
            .style(boxed(
                crate::theme::ink(rgba(242, 148, 91, 0.12)),
                10.0,
                Some((1.0, crate::theme::ink(rgba(242, 148, 91, 0.40)))),
            ))
            .id("banner".to_owned())
            .into(),
        )
    }

    /// "Reprendre la version de 13:12", when one is kept.
    pub(super) fn restore_label(&self) -> Option<String> {
        let at = self.last_good?;
        let time = local_time(at)?;
        Some(match self.lang {
            Lang::French => format!("Reprendre la version de {time}"),
            Lang::English => format!("Take back the {time} version"),
        })
    }

    // --- The home ---------------------------------------------------------------------

    fn home(&self) -> El<'_> {
        let lang = self.lang;
        let initial: String = self
            .user
            .chars()
            .next()
            .map(|c| c.to_uppercase().collect())
            .unwrap_or_default();
        let avatar = container(text(initial).size(22).font(BOLD).color(DEEP))
            .width(52)
            .height(52)
            .center(52)
            .style(|_: &Theme| container::Style {
                background: Some(Background::Gradient(Gradient::Linear(
                    gradient::Linear::new(std::f32::consts::FRAC_PI_4 * 3.0)
                        .add_stop(0.0, crate::theme::Ink::Accent(1.0).get())
                        .add_stop(1.0, crate::theme::ink(rgb(0x5B8DEF))),
                ))),
                border: border(26.0, 0.0, Color::TRANSPARENT),
                ..container::Style::default()
            });
        let hello = match lang {
            Lang::French => format!("Bonjour {}", self.user),
            Lang::English => format!("Hello {}", self.user),
        };
        let greeting = row![
            avatar,
            column![
                text(hello).size(22).font(BOLD).color(TEXT),
                t(
                    w(
                        "Ce que tu règles ici est enregistré aussitôt, sans bouton à presser.",
                        "What you set here is saved at once, with no button to press.",
                    )
                    .get(lang),
                    13.5,
                    DIM.get(),
                ),
            ]
            .spacing(4),
        ]
        .spacing(16)
        .align_y(Alignment::Center);
        let cards = [
            SectionId::Screens,
            SectionId::Sound,
            SectionId::Network,
            SectionId::Bluetooth,
            SectionId::Power,
            SectionId::Appearance,
        ];
        let mut grid = column![].spacing(10);
        for three in cards.chunks(3) {
            let mut line = row![].spacing(10);
            for id in three {
                line = line.push(self.card(*id));
            }
            grid = grid.push(line);
        }
        let mut recent = column![];
        for (i, change) in self.recent.iter().enumerate() {
            let Some(setting) = change.slot.setting() else {
                continue;
            };
            let name = self.slot_name(setting, change.slot);
            let value = setting.say(&change.value, lang);
            let when = if change.by_hand {
                w("à la main", "by hand").get(lang).to_owned()
            } else {
                when(change.at, lang)
            };
            let mut line = row![
                column![
                    t(name, 13.5, TEXT.get()).font(SEMIBOLD),
                    t(value, 13.0, DIM.get())
                ]
                .spacing(1),
                Space::new().width(Length::Fill),
                t(when, 12.0, FAINT.get()),
            ]
            .align_y(Alignment::Center)
            .padding([0, 16])
            .height(44);
            if i > 0 {
                line = line.padding([0, 16]);
            }
            recent = recent.push(divided(i > 0, line.into()));
        }
        let mut page = column![greeting, grid].spacing(22);
        if !self.recent.is_empty() {
            page = page.push(group_box(
                label(w("CHANGÉS RÉCEMMENT", "CHANGED RECENTLY").get(lang)),
                card_box(recent.into()),
            ));
        }
        let path = self.file.as_ref().map_or_else(String::new, |p| tilde(p));
        let file = column![
            text(path).size(12).font(MONO).color(TEXT),
            t(
                w(
                    "Tu peux toujours l’éditer à la main : les Réglages le relisent dès qu’il change, n’écrivent que les lignes qu’ils modifient et gardent tes commentaires.",
                    "You can still edit it by hand: Settings reads it again as soon as it changes, writes only the lines it changes and keeps your comments.",
                )
                .get(lang),
                12.5,
                DIM.get(),
            )
            .line_height(1.5),
            button(t(w("Ouvrir le fichier", "Open the file").get(lang), 13.0, TEXT.get()).font(SEMIBOLD))
                .height(32)
                .padding([7, 14])
                .on_press(Message::OpenFile)
                .style(|_, status| solid(crate::theme::ink(rgba(255, 255, 255, 0.06)), TEXT.get(), status)),
        ]
        .spacing(8);
        page = page.push(group_box(
            label(w("LE FICHIER", "THE FILE").get(lang)),
            card_box(container(file).padding([14, 16]).into()),
        ));
        container(page).padding(Padding::default().top(14)).into()
    }

    fn card(&self, id: SectionId) -> El<'_> {
        let section = id.section();
        let state = self.card_state(id);
        container(
            button(
                row![
                    container(icon(section.icon, 18.0, BLUE.get()))
                        .width(34)
                        .height(34)
                        .center(34)
                        .style(boxed(
                            crate::theme::ink(rgba(143, 179, 255, 0.12)),
                            9.0,
                            None
                        )),
                    column![
                        text(section.name.get(self.lang))
                            .size(13.5)
                            .font(BOLD)
                            .color(TEXT),
                        t(state, 12.5, DIM.get()).wrapping(text::Wrapping::None),
                    ]
                    .spacing(2),
                ]
                .spacing(12)
                .align_y(Alignment::Center)
                .height(Length::Fill),
            )
            .width(Length::Fill)
            .height(72)
            .padding([0, 14])
            .on_press(Message::Section(id))
            .style(|_, status| {
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: Some(Background::Color(if hovered {
                        crate::theme::ink(rgba(255, 255, 255, 0.06))
                    } else {
                        CARD.get()
                    })),
                    text_color: TEXT.get(),
                    border: border(12.0, 1.0, CARD_RIM.get()),
                    ..button::Style::default()
                }
            }),
        )
        .width(Length::FillPortion(1))
        .id(format!("card:{}", section.slug))
        .into()
    }

    /// A home card's state now.
    fn card_state(&self, id: SectionId) -> String {
        let lang = self.lang;
        match id {
            SectionId::Power => {
                let slot =
                    crate::model::setting("power.suspend-after").map(|s| Slot::of(s, self.source));
                let suspend = slot.map(|slot| self.said_value(slot)).unwrap_or_default();
                let suspend = match lang {
                    Lang::French if suspend == "Jamais" => "jamais de veille".to_owned(),
                    Lang::French => format!("veille après {suspend}"),
                    Lang::English if suspend == "Never" => "never sleeps".to_owned(),
                    Lang::English => format!("sleeps after {suspend}"),
                };
                match (self.battery, lang) {
                    (Some(b), Lang::French) => format!("Batterie {b} % · {suspend}"),
                    (Some(b), Lang::English) => format!("Battery {b} % · {suspend}"),
                    (None, Lang::French) => format!("Sur secteur · {suspend}"),
                    (None, Lang::English) => format!("On AC · {suspend}"),
                }
            }
            SectionId::Sound => self.sound_card(),
            SectionId::Appearance => w("Sombre · français", "Dark · English")
                .get(lang)
                .to_owned(),
            _ => w("À venir", "Coming").get(lang).to_owned(),
        }
    }

    // --- A section's page -------------------------------------------------------------

    fn section(&self, id: SectionId) -> El<'_> {
        let lang = self.lang;
        if id == SectionId::Screens {
            return self.screens_page();
        }
        let section = id.section();
        let mut page = column![
            column![
                text(section.name.get(lang)).size(22).font(BOLD).color(TEXT),
                t(section.line.get(lang), 13.5, DIM.get()).line_height(1.5),
            ]
            .spacing(6),
        ]
        .spacing(18)
        .padding(Padding::default().top(14));
        if id == SectionId::Sound {
            page = page.push(self.sound_groups());
            return container(page).id(format!("page:{}", section.slug)).into();
        }
        if id == SectionId::Mouse {
            return self.mouse_page(page);
        }
        if id == SectionId::Keyboard {
            return self.keyboard_page(page);
        }
        if id == SectionId::Wallpaper {
            return self.wallpaper_page(page);
        }
        let settings = settings_of(id);
        if settings.is_empty() {
            page = page.push(card_box(
                container(
                    column![
                        t(w("À venir", "Coming").get(lang), 14.0, TEXT.get()).font(SEMIBOLD),
                        t(
                            w(
                                "Les réglages de cette page arrivent bientôt. En attendant, le fichier de configuration les garde.",
                                "This page's settings come soon. Meanwhile, the configuration file keeps them.",
                            )
                            .get(lang),
                            12.5,
                            DIM.get(),
                        ),
                    ]
                    .spacing(4),
                )
                .padding([14, 16])
                .into(),
            ));
            return container(page).id(format!("page:{}", section.slug)).into();
        }
        // The rows, group after group.
        let mut groups: Vec<(crate::model::RowGroup, Vec<&'static Setting>)> = Vec::new();
        for setting in settings {
            match groups.last_mut() {
                Some((group, list)) if *group == setting.group => list.push(setting),
                _ => groups.push((setting.group, vec![setting])),
            }
        }
        let controls = self.controls();
        for (group, list) in groups {
            let mut head = row![
                label(group.label.get(lang)),
                Space::new().width(Length::Fill)
            ]
            .align_y(Alignment::Center)
            .height(28);
            if group.per_source {
                let focused = matches!(self.focus, Focus::Control(i) if controls.get(i) == Some(&super::Control::Segmented));
                head = head.push(self.segmented(focused));
            }
            let mut rows = column![];
            for (j, setting) in list.into_iter().enumerate() {
                rows = rows.push(divided(j > 0, self.row(setting, &controls)));
            }
            page = page.push(column![head, card_box(rows.into())].spacing(8));
        }
        container(page).id(format!("page:{}", section.slug)).into()
    }

    fn segmented(&self, focused: bool) -> El<'_> {
        let option = |source: Source| {
            let chosen = self.source == source;
            container(
                button(
                    t(
                        source.name().get(self.lang),
                        12.5,
                        if chosen { TEXT.get() } else { DIM.get() },
                    )
                    .font(SEMIBOLD),
                )
                .height(24)
                .padding([3, 10])
                .on_press(Message::Source(source))
                .style(move |_, _| button::Style {
                    background: chosen.then_some(Background::Color(crate::theme::ink(rgba(
                        255, 255, 255, 0.13,
                    )))),
                    text_color: TEXT.get(),
                    border: border(7.0, 0.0, Color::TRANSPARENT),
                    ..button::Style::default()
                }),
            )
            .id(format!("source:{}", source.word()))
        };
        container(row![option(Source::Battery), option(Source::Ac)])
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
            })
            .into()
    }

    pub(super) fn row(&self, setting: &'static Setting, controls: &[super::Control]) -> El<'_> {
        let lang = self.lang;
        let slot = Slot::of(setting, self.source);
        // A row no device plugged can take is drawn at 45 % (6.12).
        let not_here = self.unavailable(setting);
        let ink = if not_here.is_some() { INERT } else { 1.0 };
        let mut words = column![
            text(setting.name.get(lang))
                .size(14)
                .font(SEMIBOLD)
                .color(fade(TEXT.get(), ink))
        ]
        .spacing(2)
        .width(Length::Fill);
        if setting.kind == Kind::Swatches {
            words = words.push(t(self.accent_line(slot), 12.5, fade(DIM.get(), ink)));
        } else if setting.id == crate::wallpaper::COLOR {
            words = words.push(t(self.colour_line(), 12.5, fade(DIM.get(), ink)));
        } else if setting.id == crate::wallpaper::FIT {
            words = words.push(t(self.fit_line(), 12.5, fade(DIM.get(), ink)));
        } else if let Some(line) = setting.line {
            words = words.push(t(line.get(lang), 12.5, fade(DIM.get(), ink)));
        }
        if let Some(not_here) = not_here {
            words = words.push(t(not_here, 12.5, FAINT.get()));
        }
        if let Some(warning) = self.warning(setting, slot) {
            words = words.push(t(warning, 12.5, PEACH.get()));
        }
        let inert = self.inert(setting);
        let focused = matches!(self.focus, Focus::Control(i) if controls.get(i) == Some(&super::Control::Setting(slot)));
        // The row 54 px at least: 36 inside its padding.
        let control = row![
            Space::new().height(36),
            container(self.control(setting, slot, inert, focused))
                .id(format!("control:{}", setting.id)),
        ]
        .align_y(Alignment::Center);
        let lit = self.lit(setting.id);
        container(row![words, control].spacing(16).align_y(Alignment::Center))
            .padding(Padding {
                top: 9.0,
                right: 14.0,
                bottom: 9.0,
                left: 16.0,
            })
            .height(Length::Shrink)
            .center_y(Length::Shrink)
            .style(move |_: &Theme| container::Style {
                background: (lit > 0.0).then_some(Background::Color(
                    crate::theme::Ink::Accent(0.20 * lit).get(),
                )),
                border: border(11.0, 1.5, crate::theme::Ink::Accent(0.85 * lit).get()),
                ..container::Style::default()
            })
            .id(format!("row:{}", setting.id))
            .into()
    }

    /// "Dans le fichier : « 3 heures », non compris ; 15 min s'applique."
    pub(super) fn warning(&self, setting: &Setting, slot: Slot) -> Option<String> {
        let Reading::NotUnderstood(raw) = self.reading(slot) else {
            return None;
        };
        let applies = setting.say(&self.default_of(slot)?, self.lang);
        Some(match self.lang {
            Lang::French => {
                format!("Dans le fichier : « {raw} », non compris ; {applies} s’applique.")
            }
            Lang::English => format!("In the file: “{raw}”, not understood; {applies} applies."),
        })
    }

    fn control(&self, setting: &'static Setting, slot: Slot, inert: bool, focused: bool) -> El<'_> {
        let alpha = if inert { INERT } else { 1.0 };
        let value = self.value(slot);
        if setting.id == crate::wallpaper::COLOR {
            return self.colour_swatches(slot, inert, focused);
        }
        match &setting.kind {
            Kind::Switch => {
                let on = value == Some(crate::model::Value::Bool(true));
                // The knob slides over 160 ms, ease-out.
                let moved = self.knobs.get(setting.id).map_or(1.0, |since| {
                    ease_out(self.now.saturating_duration_since(*since), super::KNOB)
                });
                let at = if on { moved } else { 1.0 - moved };
                let track = if on {
                    ACCENT.get()
                } else {
                    crate::theme::Ink::Token(|t| t.switch_off).get()
                };
                let knob = container(Space::new().width(18).height(18)).style(move |_: &Theme| {
                    container::Style {
                        background: Some(Background::Color(fade(Color::WHITE, alpha))),
                        border: border(9.0, 0.0, Color::TRANSPARENT),
                        shadow: Shadow {
                            color: rgba(0, 0, 0, 0.3 * alpha),
                            offset: Vector::new(0.0, 1.0),
                            blur_radius: 2.0,
                        },
                        ..container::Style::default()
                    }
                });
                let mut switch = button(
                    container(knob).padding(Padding::default().left(2.0 + 18.0 * at).top(2)),
                )
                .width(40)
                .height(22)
                .padding(0)
                .style(move |_, _| button::Style {
                    background: Some(Background::Color(fade(track, alpha))),
                    text_color: TEXT.get(),
                    border: if focused {
                        ring(11.0)
                    } else {
                        border(11.0, 0.0, Color::TRANSPARENT)
                    },
                    ..button::Style::default()
                });
                if !inert {
                    switch = switch.on_press(Message::Toggle(slot));
                }
                switch.into()
            }
            Kind::Menu(_) => {
                let said = value
                    .map(|v| self.menu_said(setting, &v))
                    .unwrap_or_default();
                // 158 wide at least: its value, the chevron, their gap and
                // its sides.
                let width =
                    (style::measure(&said, 13.5, SEMIBOLD).ceil() + 12.0 + 10.0 + 24.0 + 8.0)
                        .max(158.0);
                let open = self.menu.is_some_and(|(s, _)| s == slot);
                let mut menu = button(
                    row![
                        text(said)
                            .size(13.5)
                            .font(SEMIBOLD)
                            .color(fade(TEXT.get(), alpha)),
                        Space::new().width(Length::Fill),
                        icon(Icon::Chevron, 12.0, fade(DIM.get(), alpha)),
                    ]
                    .spacing(10)
                    .height(Length::Fill)
                    .align_y(Alignment::Center),
                )
                .height(32)
                .width(width)
                .padding([0, 12])
                .style(move |_, status| {
                    let hovered =
                        matches!(status, button::Status::Hovered | button::Status::Pressed);
                    button::Style {
                        background: Some(Background::Color(fade(
                            crate::theme::ink(rgba(
                                255,
                                255,
                                255,
                                if hovered || open { 0.09 } else { 0.06 },
                            )),
                            alpha,
                        ))),
                        text_color: TEXT.get(),
                        border: if focused {
                            ring(8.0)
                        } else {
                            border(8.0, 1.0, fade(RIM_12.get(), alpha))
                        },
                        ..button::Style::default()
                    }
                });
                if !inert {
                    menu = menu.on_press(Message::Menu(slot));
                }
                menu.into()
            }
            Kind::Slider { min, max, step, .. } => {
                let now = match value {
                    Some(crate::model::Value::Int(n)) => n,
                    _ => *min,
                };
                let said = value
                    .map(|v| setting.say(&v, self.lang))
                    .unwrap_or_default();
                let narrow = |n: i64| i32::try_from(n).unwrap_or_default();
                let bar = slider(narrow(*min)..=narrow(*max), narrow(now), move |n| {
                    if inert {
                        Message::Nothing
                    } else {
                        Message::Slide(slot, i64::from(n))
                    }
                })
                .step(narrow(*step))
                .width(220)
                .height(16)
                .style(move |_, _| slider::Style {
                    rail: slider::Rail {
                        backgrounds: (
                            Background::Gradient(Gradient::Linear(
                                gradient::Linear::new(std::f32::consts::FRAC_PI_2)
                                    .add_stop(0.0, fade(ACCENT.get(), alpha))
                                    .add_stop(1.0, fade(crate::theme::Ink::AccentLit.get(), alpha)),
                            )),
                            Background::Color(fade(
                                crate::theme::ink(rgba(255, 255, 255, 0.12)),
                                alpha,
                            )),
                        ),
                        width: 6.0,
                        border: border(3.0, 0.0, Color::TRANSPARENT),
                    },
                    handle: slider::Handle {
                        shape: slider::HandleShape::Circle { radius: 8.0 },
                        background: Background::Color(fade(Color::WHITE, alpha)),
                        // On light, a rim so that the knob shows on the card.
                        border_width: if focused {
                            3.0
                        } else if crate::theme::light() {
                            1.0
                        } else {
                            0.0
                        },
                        border_color: if focused { ACCENT.get() } else { RIM_12.get() },
                    },
                });
                row![
                    bar,
                    container(
                        text(said)
                            .size(12.5)
                            .font(MONO)
                            .color(fade(TEXT.get(), alpha))
                            .wrapping(text::Wrapping::None),
                    )
                    .width(56),
                ]
                .spacing(12)
                .align_y(Alignment::Center)
                .into()
            }
            Kind::Speed { min, max } => self.speed_control(slot, *min, *max, inert, focused),
            // Drawn by their pages (6.8).
            Kind::Shortcut | Kind::Layouts => Space::new().into(),
            // 6.2's.
            Kind::Segmented(choices) => {
                self.segmented_control(setting, slot, choices, inert, focused)
            }
            Kind::Swatches => self.swatches(slot, inert, focused),
            Kind::Link(to) => self.link_control(*to),
            Kind::Text => {
                let said = match value {
                    Some(crate::model::Value::Text(t)) => t,
                    _ => String::new(),
                };
                text(said).size(13.5).color(fade(TEXT.get(), alpha)).into()
            }
        }
    }

    // --- Over the page ----------------------------------------------------------------

    /// The open menu's list: 38 px under its button's top, its right on
    /// the button's.
    fn menu_list(&self) -> Option<El<'_>> {
        let (slot, since) = self.menu?;
        let setting = slot.setting()?;
        let Kind::Menu(choices) = &setting.kind else {
            return None;
        };
        let at = *self.places.at.get(&format!("control:{}", setting.id))?;
        let current = self.value(slot);
        let opened = if self.reduced_motion {
            1.0
        } else {
            ease_out(self.now.saturating_duration_since(since), super::MENU_IN)
        };
        // Wider when its choices say something beside them (6.2's dates).
        let wide = choices
            .iter()
            .any(|c| self.choice_aside(setting, c).is_some());
        let width = at.width.max(if wide { 330.0 } else { 200.0 });
        let mut list = column![].spacing(0);
        for (i, choice) in choices.iter().enumerate() {
            let chosen = current.as_ref() == Some(&choice.value);
            let keyboard = i == self.menu_index;
            let check: El<'_> = if chosen {
                icon(Icon::Check, 13.0, fade(ACCENT.get(), opened))
            } else {
                Space::new().width(13).into()
            };
            list = list.push(
                container(
                    button(
                        row![
                            container(check).width(18).center_x(18),
                            t(choice.label.get(self.lang), 13.5, fade(TEXT.get(), opened))
                                .font(if chosen { SEMIBOLD } else { MANROPE }),
                            Space::new().width(Length::Fill),
                            t(
                                self.choice_aside(setting, choice).unwrap_or_default(),
                                11.5,
                                fade(DIM.get(), opened)
                            )
                            .font(MONO),
                        ]
                        .spacing(6)
                        .height(Length::Fill)
                        .align_y(Alignment::Center),
                    )
                    .width(Length::Fill)
                    .height(32)
                    .padding([0, 8])
                    .on_press(Message::Choose(slot, choice.value.clone()))
                    .style(flat(
                        keyboard.then_some(crate::theme::ink(rgba(255, 255, 255, 0.07))),
                        crate::theme::ink(rgba(255, 255, 255, 0.07)),
                        7.0,
                        false,
                    )),
                )
                .id(format!("option:{i}")),
            );
        }
        let panel = container(list)
            .width(width)
            .padding(5)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(fade(MENU.get(), opened))),
                border: border(10.0, 1.0, fade(RIM_12.get(), opened)),
                shadow: Shadow {
                    color: rgba(0, 0, 0, 0.45 * opened),
                    ..style::shadow()
                },
                ..container::Style::default()
            })
            .id("menu".to_owned());
        let x = (at.x + at.width - width).max(0.0);
        // Under its button; over it when the page's bottom would cut it.
        let height = choices.len() as f32 * 32.0 + 10.0;
        let bottom = self
            .places
            .page
            .map_or(f32::INFINITY, |p| p.viewport.y + p.viewport.height);
        let y = if at.y + 38.0 + height > bottom {
            at.y - 6.0 - height + 4.0 * (1.0 - opened)
        } else {
            at.y + 38.0 - 4.0 * (1.0 - opened)
        };
        Some(pin(panel).x(x).y(y).into())
    }

    fn toast_view(&self) -> Option<El<'_>> {
        let toast = self.toast.as_ref()?;
        let came = if self.reduced_motion {
            1.0
        } else {
            ease_out(
                self.now.saturating_duration_since(toast.since),
                super::TOAST_IN,
            )
        };
        let lang = self.lang;
        // Dark in both themes, and so are its words' colours.
        let dark = hocus_link::theme::DARK;
        let ink = crate::theme::color;
        let [r, g, b] = crate::theme::now().accent().on_dark;
        let mint = Color::from_rgb8(r, g, b);
        let mut line = row![t(toast.text.clone(), 13.0, fade(ink(dark.text), came))]
            .spacing(14)
            .align_y(Alignment::Center);
        let action = match toast.action {
            ToastAction::Undo => Some(w("Annuler", "Undo").get(lang).to_owned()),
            ToastAction::Show(_) => Some(w("Voir", "Show").get(lang).to_owned()),
            ToastAction::Sound(act) => Some(self.sound_act_label(act)),
            ToastAction::None => None,
        };
        if let Some(action) = action {
            line = line.push(
                container(
                    button(text(action).size(13).font(BOLD).color(fade(mint, came)))
                        .padding([4, 6])
                        .on_press(Message::ToastAct)
                        .style(flat(None, rgba(255, 255, 255, 0.06), 6.0, false)),
                )
                .id("toast-action".to_owned()),
            );
        }
        line = line.push(
            button(icon(Icon::Close, 13.0, fade(ink(dark.dim), came)))
                .width(24)
                .height(24)
                .padding(5.5)
                .on_press(Message::ToastClose)
                .style(flat(None, rgba(255, 255, 255, 0.06), 6.0, false)),
        );
        let toast = container(line)
            .height(40)
            .padding(Padding {
                top: 0.0,
                right: 8.0,
                bottom: 0.0,
                left: 16.0,
            })
            .center_y(40)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(fade(TOAST.get(), came))),
                border: border(10.0, 1.0, fade(ink(dark.control_rim), came)),
                shadow: Shadow {
                    color: rgba(0, 0, 0, 0.45 * came),
                    ..style::shadow()
                },
                ..container::Style::default()
            })
            .id("toast".to_owned());
        let rise = 8.0 * (1.0 - came);
        Some(
            row![
                Space::new().width(SIDEBAR_WIDTH),
                container(toast)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_bottom(Length::Fill)
                    .center_x(Length::Fill)
                    .padding(Padding::default().bottom(18.0 - rise)),
            ]
            .into(),
        )
    }
}

/// Labels as the mockup spaces them (0.1 em): a thin space between
/// letters is too much at these sizes, so they stay as they are.
fn spaced(label: &str) -> String {
    label.to_owned()
}

/// "ÉCRAN ET VEILLE" as "Écran et veille".
fn capitalised(label: &str) -> String {
    let lower = label.to_lowercase();
    let mut chars = lower.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

pub(super) fn label<'a>(words: &'static str) -> El<'a> {
    text(words).size(11.5).font(BOLD).color(DIM).into()
}

fn group_box<'a>(head: El<'a>, body: El<'a>) -> El<'a> {
    column![container(head).height(28).center_y(28), body]
        .spacing(8)
        .into()
}

pub(super) fn card_box(body: El<'_>) -> El<'_> {
    container(body)
        .width(Length::Fill)
        .style(boxed(CARD.get(), 12.0, Some((1.0, CARD_RIM.get()))))
        .into()
}

/// A row, with the divider over it after the first.
pub(super) fn divided(divider: bool, row: El<'_>) -> El<'_> {
    if !divider {
        return row;
    }
    column![
        container(
            container(Space::new().width(Length::Fill).height(1)).style(boxed(
                LINE.get(),
                0.0,
                None
            )),
        )
        .padding([0, 16]),
        row,
    ]
    .into()
}

/// A filled button.
fn solid(fill: Color, ink: Color, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: Some(Background::Color(if hovered {
            Color {
                a: (fill.a + 0.04).min(1.0),
                ..fill
            }
        } else {
            fill
        })),
        text_color: ink,
        border: border(8.0, 0.0, Color::TRANSPARENT),
        ..button::Style::default()
    }
}

/// The scrollbars: thin, faint.
pub(super) fn thin(_: &Theme, status: scrollable::Status) -> scrollable::Style {
    let hovered = matches!(
        status,
        scrollable::Status::Hovered { .. } | scrollable::Status::Dragged { .. }
    );
    let rail = scrollable::Rail {
        background: None,
        border: border(3.0, 0.0, Color::TRANSPARENT),
        scroller: scrollable::Scroller {
            background: Background::Color(crate::theme::ink(rgba(
                255,
                255,
                255,
                if hovered { 0.16 } else { 0.08 },
            ))),
            border: border(3.0, 0.0, Color::TRANSPARENT),
        },
    };
    scrollable::Style {
        container: container::Style::default(),
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
        auto_scroll: scrollable::AutoScroll {
            background: Background::Color(TOAST.get()),
            border: border(10.0, 1.0, RIM_12.get()),
            shadow: Shadow::default(),
            icon: DIM.get(),
        },
    }
}

pub(super) fn ease(t: Duration, over: Duration) -> f32 {
    ease_out(t, over)
}

pub(super) fn ease_out(t: Duration, over: Duration) -> f32 {
    let x = (t.as_secs_f32() / over.as_secs_f32()).clamp(0.0, 1.0);
    1.0 - (1.0 - x).powi(3)
}

/// "config.toml ne se lit plus, ligne 42".
pub(super) fn banner_title(line: Option<usize>, lang: Lang) -> String {
    match (line, lang) {
        (Some(line), Lang::French) => format!("config.toml ne se lit plus, ligne {line}"),
        (Some(line), Lang::English) => format!("config.toml no longer reads, line {line}"),
        (None, Lang::French) => "config.toml ne se lit plus".to_owned(),
        (None, Lang::English) => "config.toml no longer reads".to_owned(),
    }
}

/// `13:12`, in the user's time zone.
fn local_time(at: SystemTime) -> Option<String> {
    let at = jiff::Timestamp::try_from(at).ok()?;
    let zoned = at.to_zoned(jiff::tz::TimeZone::system());
    Some(zoned.strftime("%H:%M").to_string())
}

/// When a change was made, as the user reads it.
fn when(at: i64, lang: Lang) -> String {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    let ago = now.saturating_sub(at);
    let fr = lang.fr();
    if ago < 60 {
        return if fr { "à l’instant" } else { "just now" }.to_owned();
    }
    if ago < 3600 {
        let m = ago / 60;
        return if fr {
            format!("il y a {m} min")
        } else {
            format!("{m} min ago")
        };
    }
    let Ok(stamp) = jiff::Timestamp::from_second(at) else {
        return String::new();
    };
    let tz = jiff::tz::TimeZone::system();
    let then = stamp.to_zoned(tz.clone());
    let today = jiff::Timestamp::now().to_zoned(tz).date();
    if then.date() == today {
        let time = then.strftime("%H:%M").to_string();
        return if fr {
            format!("à {time}")
        } else {
            format!("at {time}")
        };
    }
    if today.yesterday().is_ok_and(|y| y == then.date()) {
        return if fr { "hier" } else { "yesterday" }.to_owned();
    }
    then.strftime("%d/%m").to_string()
}

/// `~/.config/hocuspanda/config.toml`.
fn tilde(path: &std::path::Path) -> String {
    let shown = path.display().to_string();
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && shown.starts_with(&format!("{home}/")) => {
            format!("~{}", &shown[home.len()..])
        }
        _ => shown,
    }
}

/// What the window shows, for its log.
pub(super) fn shown(app: &App) -> serde_json::Value {
    let lang = app.lang;
    let page = match app.page {
        Page::Home => "home".to_owned(),
        Page::Section(id) => id.section().slug.to_owned(),
    };
    let sections: Vec<&str> = SectionId::ALL.iter().map(|s| s.name(lang)).collect();
    let results: Vec<serde_json::Value> = app
        .results
        .iter()
        .map(|f| {
            serde_json::json!({
                "name": f.name,
                "target": f.target.id(),
                "via": f.via,
            })
        })
        .collect();
    let rows: Vec<serde_json::Value> = match app.page {
        Page::Section(SectionId::Keyboard) if app.keys.sub => Vec::new(),
        Page::Section(SectionId::Keyboard) => settings_of(SectionId::Keyboard)
            .iter()
            .filter(|s| s.kind != Kind::Shortcut)
            .map(|s| {
                let slot = Slot::of(s, app.source);
                serde_json::json!({
                    "id": s.id,
                    "value": app.said_value(slot),
                    "warn": app.warning(s, slot),
                    "inert": app.inert(s),
                    "not_here": app.unavailable(s),
                })
            })
            .collect(),
        Page::Section(SectionId::Screens) => Vec::new(),
        Page::Section(SectionId::Mouse) => app
            .mouse_visible()
            .into_iter()
            .map(|s| {
                let slot = Slot::of(s, app.source);
                serde_json::json!({
                    "id": s.id,
                    "value": app.said_value(slot),
                    "warn": app.warning(s, slot),
                    "inert": app.inert(s),
                    "not_here": app.not_here(s),
                })
            })
            .collect(),
        Page::Section(id) => settings_of(id)
            .iter()
            .map(|s| {
                let slot = Slot::of(s, app.source);
                serde_json::json!({
                    "id": s.id,
                    "value": app.said_value(slot),
                    "warn": app.warning(s, slot),
                    "inert": app.inert(s),
                })
            })
            .collect(),
        Page::Home => Vec::new(),
    };
    let (file, banner) = match app.snapshot.as_ref().map(|s| &s.state) {
        None => ("reading", None),
        Some(FileState::Reads) => ("saved", None),
        Some(FileState::Broken { line, .. }) => ("read-only", Some(banner_title(*line, lang))),
        Some(FileState::NoFile) => ("no-file", None),
    };
    let lit = app.light.and_then(|(id, since)| {
        (Instant::now().saturating_duration_since(since) < super::LIGHT_END).then_some(id)
    });
    let toast = app.toast.as_ref().map(|t| t.text.clone());
    let action = app.toast.as_ref().and_then(|t| match t.action {
        ToastAction::Undo => Some("undo"),
        ToastAction::Show(_) => Some("show"),
        ToastAction::Sound(_) => Some("sound"),
        ToastAction::None => None,
    });
    let cards: Vec<serde_json::Value> = [
        SectionId::Screens,
        SectionId::Sound,
        SectionId::Network,
        SectionId::Bluetooth,
        SectionId::Power,
        SectionId::Appearance,
    ]
    .iter()
    .map(|id| serde_json::json!([id.name(lang), app.card_state(*id)]))
    .collect();
    serde_json::json!({
        "page": page,
        "sections": sections,
        "query": app.query,
        "results": results,
        "selected": app.selected,
        "lit": lit,
        "toast": toast,
        "toast_action": action,
        "file": file,
        "theme": crate::theme::now().scheme.word(),
        "lang": if app.lang == Lang::French { "fr" } else { "en" },
        "text_scale": app.text_scale,
        "accent": crate::theme::now().accent.word(),
        "banner": banner,
        "restore": if banner.is_some() { app.restore_label() } else { None },
        "source": app.source.word(),
        "rows": rows,
        "menu": app.menu.map(|(slot, _)| slot.word()),
        "cards": if app.page == Page::Home { serde_json::json!(cards) } else { serde_json::Value::Null },
        "undo": app.undo.len(),
        "sound": if app.page == Page::Section(SectionId::Sound) {
            app.sound_shown()
        } else {
            serde_json::json!({ "listening": app.sound.listening() })
        },
        "scale": app.scale,
        "screens": if app.page == Page::Section(SectionId::Screens) { app.screens_shown() } else { serde_json::Value::Null },
        "mouse": if app.page == Page::Section(SectionId::Mouse) { app.mouse_shown() } else { serde_json::Value::Null },
        "keys": if app.page == Page::Section(SectionId::Keyboard) { app.keys_shown() } else { serde_json::Value::Null },
        "wallpaper": if app.page == Page::Section(SectionId::Wallpaper) { app.wallpaper_shown() } else { serde_json::Value::Null },
        "focus": match app.focus {
            Focus::Nothing => "nothing".to_owned(),
            Focus::Search => "search".to_owned(),
            Focus::Nav(i) => format!("nav:{i}"),
            Focus::Control(i) => format!("control:{i}"),
        },
    })
}
