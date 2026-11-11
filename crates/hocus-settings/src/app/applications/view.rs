//! What the Applications page draws (the approved mockup, variant A): the
//! default applications' rows and their menus, the terminal's field, the
//! start-up rows, the line under them and the sheet to add one.

use crate::lang::French as _;
use hocus_words::settings::app_applications_view as words;
use iced::{
    Alignment, Background, Color, Length, Padding, Shadow, Theme,
    widget::{
        Column, Space, button, column, container, image, mouse_area, pin, row, scrollable, stack,
        svg, text, text_input,
    },
};

use super::{FIELD, Icon, Menu, Msg, SEARCH, Stop};
use crate::{
    app::{
        App, Message,
        style::*,
        view::{El, card_box, divided, icon, label, t},
    },
    applications::{self as model, ADDED, DEFAULTS_GROUP, Opens, STARTUP, STARTUP_GROUP, TERMINAL},
    lang::{Lang, w},
    model::{Icon as Glyph, SectionId},
};

/// The menus' buttons and the terminal's field (logical px).
const MENU_WIDTH: f32 = 200.0;
const FIELD_WIDTH: f32 = 250.0;
/// The amber of a row's warning, and the green of "+ Ajouter".
const AMBER: Ink = Ink::Dark(rgb(0xF7B98A));
const GREEN: Ink = Ink::Dark(rgb(0x7EE0A0));

use crate::theme::Ink;

/// An application's tile's colour, the same for an id each time.
fn tile_colour(id: &str) -> Color {
    const COLOURS: [u32; 7] = [
        0xF5A05A, 0x8FB3FF, 0x7FC8F8, 0xC9A3F5, 0x6CC287, 0xF5C46E, 0xE0567A,
    ];
    let sum = id
        .bytes()
        .fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(u32::from(b)));
    rgb(COLOURS[(sum % COLOURS.len() as u32) as usize])
}

impl App {
    pub(in crate::app) fn apps_page<'a>(&'a self, mut page: Column<'a, Message>) -> El<'a> {
        let lang = self.lang;
        // APPLICATIONS PAR DÉFAUT, its note on the right.
        let head = row![
            label(DEFAULTS_GROUP.label.get(lang)),
            Space::new().width(Length::Fill),
            t(
                w(words::OTHER_APPLICATIONS_USE_THEM_TOO).get(lang),
                12.0,
                FAINT.get(),
            ),
        ]
        .align_y(Alignment::Center)
        .height(28);
        let defaults = column![
            self.default_row(Opens::Browser),
            divided(true, self.default_row(Opens::Mail)),
            divided(true, self.terminal_row()),
            divided(true, self.default_row(Opens::Videos)),
            divided(true, self.default_row(Opens::Images)),
        ];
        page = page.push(column![head, card_box(defaults.into())].spacing(8));
        page = page.push(self.startup_group());
        container(page)
            .id(format!("page:{}", SectionId::Apps.section().slug))
            .into()
    }

    /// An application's icon, `size` wide: from the icon theme, else its
    /// initial on a colour of its own.
    fn app_icon<'a>(&self, icon: Option<&str>, name: &str, id: &str, size: f32) -> El<'a> {
        let radius = if size > 26.0 { 8.0 } else { 6.0 };
        match icon.and_then(|i| self.apps.icons.get(i)) {
            Some(Icon::Image(handle)) => {
                return image(handle.clone()).width(size).height(size).into();
            }
            Some(Icon::Svg(handle)) => {
                return svg(handle.clone()).width(size).height(size).into();
            }
            None => {}
        }
        let initial: String = name
            .chars()
            .next()
            .map(|c| c.to_uppercase().collect())
            .unwrap_or_default();
        let colour = tile_colour(id);
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
            border: border(radius, 0.0, colour),
            ..container::Style::default()
        })
        .into()
    }

    /// A command's tile: `>_` on the dark.
    fn command_icon<'a>(size: f32) -> El<'a> {
        container(text(">_").size(11).font(MONO).color(DIM.get()))
            .width(size)
            .height(size)
            .center(size)
            .style(|_: &Theme| container::Style {
                background: Some(Background::Color(crate::theme::ink(rgb(0x2B3346)))),
                border: border(8.0, 0.0, Color::TRANSPARENT),
                ..container::Style::default()
            })
            .into()
    }

    /// An entry's initial at 45 %: one for another desktop.
    fn faded_tile<'a>(&self, name: &str, id: &str, size: f32) -> El<'a> {
        let initial: String = name
            .chars()
            .next()
            .map(|c| c.to_uppercase().collect())
            .unwrap_or_default();
        let colour = fade(tile_colour(id), INERT);
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
            border: border(8.0, 0.0, colour),
            ..container::Style::default()
        })
        .into()
    }

    /// A row's box, lit when found: its id `row:ID`.
    fn apps_row<'a>(&self, id: &str, lit: f32, body: El<'a>) -> El<'a> {
        container(body)
            .padding(Padding {
                top: 9.0,
                right: 14.0,
                bottom: 9.0,
                left: 16.0,
            })
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

    /// A menu's button, 200 wide: the application's icon and name.
    fn app_button<'a>(
        &self,
        menu: Menu,
        app: Option<(&str, &str, Option<&str>)>,
        said: String,
        inert: bool,
    ) -> El<'a> {
        let alpha = if inert { INERT } else { 1.0 };
        let open = self.apps.menu.is_some_and(|(m, _)| m == menu);
        let focused = self.apps_focused(&Stop::Menu(menu));
        let mut inside = row![].spacing(8).align_y(Alignment::Center);
        if let Some((id, name, icon)) = app {
            inside = inside.push(self.app_icon(icon, name, id, 22.0));
        }
        inside = inside.push(
            text(said)
                .size(13.5)
                .font(SEMIBOLD)
                .color(fade(TEXT.get(), alpha))
                .wrapping(text::Wrapping::None),
        );
        let mut button = button(
            row![
                container(inside).width(Length::Fill).clip(true),
                icon(Glyph::Chevron, 12.0, fade(DIM.get(), alpha)),
            ]
            .spacing(8)
            .height(Length::Fill)
            .align_y(Alignment::Center),
        )
        .height(32)
        .width(MENU_WIDTH)
        .padding(Padding {
            top: 0.0,
            right: 10.0,
            bottom: 0.0,
            left: if app.is_some() { 6.0 } else { 12.0 },
        })
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(Background::Color(fade(
                    crate::theme::ink(rgba(
                        255,
                        255,
                        255,
                        if (hovered || open) && !inert {
                            0.09
                        } else {
                            0.06
                        },
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
            button = button.on_press(Message::Apps(Msg::Menu(menu)));
        }
        container(button)
            .id(format!("control:{}", menu.id()))
            .into()
    }

    /// A row's words: its name, its line (in amber when it warns), and a
    /// line more under it.
    fn app_words<'a>(name: String, line: (String, Color), more: Option<String>) -> El<'a> {
        let mut words = column![
            text(name).size(14).font(SEMIBOLD).color(TEXT.get()),
            t(line.0, 12.5, line.1),
        ]
        .spacing(2)
        .width(Length::Fill);
        if let Some(more) = more {
            words = words.push(t(more, 12.5, FAINT.get()));
        }
        words.into()
    }

    fn default_row(&self, opens: Opens) -> El<'_> {
        let lang = self.lang;
        let known = self.apps.known.as_ref();
        let handlers = known.map(|k| k.handlers(opens)).unwrap_or_default();
        let none = known.is_some() && handlers.is_empty();
        let chosen = known
            .and_then(|k| k.chosen(opens))
            .and_then(|id| known.and_then(|k| k.app(id)));
        let line = if none {
            (opens.none().get(lang).to_owned(), AMBER.get())
        } else {
            (opens.line().get(lang).to_owned(), DIM.get())
        };
        let more = known
            .and_then(|k| k.differs(opens))
            .map(|(mime, app)| model::differs_said(mime, &self.apps_name(app), lang));
        let control = match chosen {
            Some(app) => self.app_button(
                Menu::Default(opens),
                Some((&app.id, &app.name, app.icon.as_deref())),
                app.name.clone(),
                false,
            ),
            None if none => self.app_button(
                Menu::Default(opens),
                None,
                w(words::NONE).get(lang).to_owned(),
                true,
            ),
            None => self.app_button(
                Menu::Default(opens),
                None,
                w(words::CHOOSE).get(lang).to_owned(),
                known.is_none(),
            ),
        };
        self.apps_row(
            opens.id(),
            self.lit(opens.id()),
            row![
                Self::app_words(opens.name().get(lang).to_owned(), line, more),
                control
            ]
            .spacing(16)
            .align_y(Alignment::Center)
            .into(),
        )
    }

    fn terminal_row(&self) -> El<'_> {
        let lang = self.lang;
        let chosen = self.terminal_chosen();
        let other = self.apps.typing.is_some() || (self.apps.known.is_some() && chosen.is_none());
        let line = match &self.apps.missing {
            Some(program) => (
                match lang {
                    Lang::French => format!("Introuvable dans PATH : {program}"),
                    Lang::English => format!("Not found in PATH: {program}"),
                },
                AMBER.get(),
            ),
            None if other => (
                w(words::RUN_AS_IT_IS_BY_SUPER).get(lang).to_owned(),
                DIM.get(),
            ),
            None => (
                w(words::SUPER_RETURN_AND_APPLICATIONS_THAT_OPEN)
                    .get(lang)
                    .to_owned(),
                DIM.get(),
            ),
        };
        let control: El<'_> = if other {
            let value = self
                .apps
                .typing
                .clone()
                .unwrap_or_else(|| self.terminal_value());
            let field = text_input("kitty --single-instance", &value)
                .id(FIELD)
                .font(MONO)
                .size(12)
                .padding([7, 10])
                .width(FIELD_WIDTH)
                .on_input(|text| Message::Apps(Msg::Typed(text)))
                .on_submit(Message::Apps(Msg::Commit))
                .style(field_style);
            // The menu stays a step away: its chevron.
            let back = button(container(icon(Glyph::Chevron, 12.0, DIM.get())).center(28))
                .width(28)
                .height(28)
                .padding(0)
                .style(flat(None, HOVER.get(), 7.0, false))
                .on_press(Message::Apps(Msg::Menu(Menu::Terminal)));
            row![
                container(field).id(format!("control:{TERMINAL}")),
                container(back).id("terminal-menu".to_owned()),
            ]
            .spacing(4)
            .align_y(Alignment::Center)
            .into()
        } else {
            let known = self.apps.known.as_ref();
            match chosen.as_deref() {
                // Automatique: the terminal found, by its entry.
                Some("") => {
                    let found = known.and_then(|k| k.found_terminal.as_deref());
                    let app = found.and_then(|f| {
                        known.and_then(|k| {
                            k.terminals()
                                .into_iter()
                                .find(|a| a.program == f || a.id == f)
                        })
                    });
                    match (app, found) {
                        (Some(app), _) => self.app_button(
                            Menu::Terminal,
                            Some((&app.id, &app.name, app.icon.as_deref())),
                            app.name.clone(),
                            !self.writable(),
                        ),
                        (None, Some(found)) => self.app_button(
                            Menu::Terminal,
                            Some((found, found, None)),
                            found.to_owned(),
                            !self.writable(),
                        ),
                        (None, None) => self.app_button(
                            Menu::Terminal,
                            None,
                            w(words::AUTOMATIC).get(lang).to_owned(),
                            !self.writable(),
                        ),
                    }
                }
                Some(id) => {
                    let app = known.and_then(|k| k.app(id));
                    self.app_button(
                        Menu::Terminal,
                        app.map(|a| (a.id.as_str(), a.name.as_str(), a.icon.as_deref())),
                        app.map_or_else(|| id.to_owned(), |a| a.name.clone()),
                        !self.writable(),
                    )
                }
                None => self.app_button(Menu::Terminal, None, String::new(), true),
            }
        };
        self.apps_row(
            TERMINAL,
            self.lit(TERMINAL),
            row![
                Self::app_words(w(words::TERMINAL).get(lang).to_owned(), line, None),
                control,
            ]
            .spacing(16)
            .align_y(Alignment::Center)
            .into(),
        )
    }

    // --- AU DÉMARRAGE DE LA SESSION -----------------------------------------------------

    fn startup_group(&self) -> El<'_> {
        let lang = self.lang;
        let focused = self.apps_focused(&Stop::Add);
        let add = button(
            text(w(words::ADD_AN_APPLICATION).get(lang))
                .size(13)
                .font(SEMIBOLD)
                .color(GREEN.get()),
        )
        .height(26)
        .padding([0, 8])
        .style(flat(None, HOVER.get(), 8.0, focused))
        .on_press(Message::Apps(Msg::Sheet(true)));
        let head = row![
            label(STARTUP_GROUP.label.get(lang)),
            Space::new().width(Length::Fill),
            container(add).id("control:applications.add".to_owned()),
        ]
        .align_y(Alignment::Center)
        .height(28);
        let rows = self.startup_rows();
        // The last session's applications reopened, first (windows.md §14).
        let mut card = column![];
        if let Some(setting) = crate::model::setting(model::RESTORE) {
            card = card.push(self.row(setting, &self.controls()));
        }
        if rows.is_empty() {
            card = card.push(divided(
                true,
                container(t(
                    if self.apps.known.is_some() {
                        w(words::NO_APPLICATION_STARTS_WITH_THE_SESSION)
                    } else {
                        w(words::ONE_MOMENT)
                    }
                    .get(lang),
                    13.0,
                    DIM.get(),
                ))
                .padding([16, 16])
                .into(),
            ));
        }
        for entry in &rows {
            card = card.push(divided(true, self.startup_row(entry)));
        }
        let mut foot: Vec<El<'_>> = vec![
            t(
                w(words::THEY_START_ONCE_THE_BAR_SHOWS).get(lang),
                12.0,
                FAINT.get(),
            )
            .line_height(1.5)
            .into(),
        ];
        let others = self
            .apps
            .known
            .as_ref()
            .map(|k| k.others())
            .unwrap_or_default();
        if !others.is_empty() {
            let n = others.len();
            let said = match (lang, n) {
                (Lang::French, 1) => {
                    "1 autre est prévue pour d’autres bureaux et ne démarre pas ici ·".to_owned()
                }
                (Lang::French, n) => format!(
                    "{n} autres sont prévues pour d’autres bureaux et ne démarrent pas ici ·"
                ),
                (Lang::English, 1) => {
                    "1 other is for other desktops and does not start here ·".to_owned()
                }
                (Lang::English, n) => {
                    format!("{n} others are for other desktops and do not start here ·")
                }
            };
            let focused = self.apps_focused(&Stop::Others);
            let link = button(
                text(if self.apps.others {
                    w(words::HIDE_THEM).get(lang)
                } else {
                    w(words::SHOW_THEM).get(lang)
                })
                .size(12)
                .font(SEMIBOLD)
                .color(MINT.get()),
            )
            .padding(0)
            .style(flat(None, Color::TRANSPARENT, 4.0, focused))
            .on_press(Message::Apps(Msg::Others));
            foot.push(t(said, 12.0, FAINT.get()).line_height(1.5).into());
            foot.push(container(link).id("applications-others".to_owned()).into());
        }
        let mut group = column![
            head,
            container(card_box(card.into()))
                .id(format!("row:{STARTUP}"))
                .style({
                    let lit = self.lit(STARTUP);
                    move |_: &Theme| container::Style {
                        border: border(12.0, 1.5, crate::theme::Ink::Accent(0.85 * lit).get()),
                        ..container::Style::default()
                    }
                }),
            container(iced::widget::Row::with_children(foot).spacing(6).wrap()).padding([0, 4]),
        ]
        .spacing(8);
        if self.apps.others && !others.is_empty() {
            let mut list = column![];
            for (i, entry) in others.iter().enumerate() {
                let line = if entry.only_in.is_empty() {
                    w(words::NOT_FOR_THIS_DESKTOP).get(lang).to_owned()
                } else {
                    let names = entry.only_in.join(", ");
                    match lang {
                        Lang::French => format!("Prévue pour {names}."),
                        Lang::English => format!("For {names}."),
                    }
                };
                let faint = fade(TEXT.get(), INERT);
                list = list.push(divided(
                    i > 0,
                    container(
                        row![
                            self.faded_tile(&entry.name, &entry.id, 30.0),
                            column![
                                text(entry.name.clone())
                                    .size(14)
                                    .font(SEMIBOLD)
                                    .color(faint),
                                t(line, 12.5, fade(DIM.get(), INERT)),
                            ]
                            .spacing(2)
                            .width(Length::Fill),
                        ]
                        .spacing(12)
                        .align_y(Alignment::Center),
                    )
                    .padding([9, 16])
                    .into(),
                ));
            }
            group = group
                .push(container(card_box(list.into())).id("applications-others-list".to_owned()));
        }
        group.into()
    }

    fn startup_row<'a>(&'a self, entry: &hocus_xdg::autostart::Entry) -> El<'a> {
        let lang = self.lang;
        let own = model::users_own(entry);
        let known = self.apps.known.as_ref();
        // An application's entry: by its id, else by its command.
        let app = known.and_then(|k| {
            k.app(&entry.id).or_else(|| {
                k.apps
                    .iter()
                    .find(|a| a.command == model::joined(&entry.argv))
            })
        });
        let mut line = row![].spacing(0).align_y(Alignment::Center);
        if own {
            line = line.push(t(w(words::ADDED_BY_YOU).get(lang), 12.5, DIM.get()));
            if app.is_none() && !entry.exec.is_empty() {
                line = line.push(t(" · ", 12.5, DIM.get()));
                line = line.push(
                    text(entry.exec.clone())
                        .size(11.5)
                        .font(MONO)
                        .color(DIM.get()),
                );
            }
        } else {
            let mut said = w(words::INSTALLED_WITH_THE_APPLICATION)
                .get(lang)
                .to_owned();
            if let Some(comment) = entry.comment.as_deref().filter(|c| c.chars().count() <= 40) {
                said = format!("{} · {}", said.trim_end_matches('.'), lower_first(comment));
            }
            line = line.push(t(said, 12.5, DIM.get()));
        }
        let words = column![
            text(entry.name.clone())
                .size(14)
                .font(SEMIBOLD)
                .color(TEXT.get()),
            line.wrap(),
        ]
        .spacing(2)
        .width(Length::Fill);
        let tile: El<'_> = if own && app.is_none() && entry.icon.is_none() {
            Self::command_icon(30.0)
        } else {
            self.app_icon(
                entry
                    .icon
                    .as_deref()
                    .or(app.and_then(|a| a.icon.as_deref())),
                &entry.name,
                &entry.id,
                30.0,
            )
        };
        let mut body = row![tile, words].spacing(12).align_y(Alignment::Center);
        if own {
            let focused = self.apps_focused(&Stop::Remove(entry.id.clone()));
            let remove = button(container(icon(Glyph::Close, 12.0, DIM.get())).center(28))
                .width(28)
                .height(28)
                .padding(0)
                .style(flat(None, HOVER.get(), 7.0, focused))
                .on_press(Message::Apps(Msg::Remove(entry.id.clone())));
            body = body.push(container(remove).id(format!("remove:{}", entry.id)));
        }
        let focused = self.apps_focused(&Stop::Switch(entry.id.clone()));
        body = body.push(
            container(crate::app::screens::switch(
                !entry.hidden,
                false,
                focused,
                1.0,
                Some(Message::Apps(Msg::Switch(entry.id.clone()))),
            ))
            .id(format!("switch:{}", entry.id)),
        );
        let lit = if self.apps.lit.as_deref() == Some(entry.id.as_str()) {
            self.lit(ADDED)
        } else {
            0.0
        };
        self.apps_row(&format!("startup:{}", entry.id), lit, body.into())
    }

    // --- The menus and the sheet --------------------------------------------------------

    /// The open menu, under its button.
    pub(in crate::app) fn apps_menu(&self) -> Option<El<'_>> {
        let (menu, since) = self.apps.menu?;
        let lang = self.lang;
        let at = *self
            .places
            .at
            .get(&format!("control:{}", menu.id()))
            .or_else(|| {
                matches!(menu, Menu::Terminal)
                    .then(|| self.places.at.get("terminal-menu"))
                    .flatten()
            })?;
        let opened = if self.reduced_motion {
            1.0
        } else {
            crate::app::view::ease_out(
                self.now.saturating_duration_since(since),
                crate::app::MENU_IN,
            )
        };
        let ids = self.menu_ids(menu);
        let chosen = self.menu_chosen(menu);
        let known = self.apps.known.as_ref()?;
        let mut list = column![];
        let mut rows: f32 = 0.0;
        for (i, id) in ids.iter().enumerate() {
            let is = chosen == Some(i);
            let keyboard = self.apps.menu_at == i;
            let check: El<'_> = if is {
                icon(Glyph::Check, 13.0, fade(ACCENT.get(), opened))
            } else {
                Space::new().width(13).into()
            };
            let (tile, name, aside): (Option<El<'_>>, String, Option<String>) = match id.as_deref()
            {
                Some("") => (
                    None,
                    w(words::AUTOMATIC).get(lang).to_owned(),
                    Some(w(words::THE_FIRST_FOUND).get(lang).to_owned()),
                ),
                Some(id) => {
                    let app = known.app(id);
                    let name = app.map_or_else(|| id.to_owned(), |a| a.name.clone());
                    (
                        Some(self.app_icon(app.and_then(|a| a.icon.as_deref()), &name, id, 22.0)),
                        name,
                        None,
                    )
                }
                None => (None, w(words::OTHER_COMMAND).get(lang).to_owned(), None),
            };
            if id.is_none() {
                list = list.push(
                    container(
                        container(Space::new().width(Length::Fill).height(1)).style(boxed(
                            fade(LINE.get(), opened),
                            0.0,
                            None,
                        )),
                    )
                    .padding([4, 6]),
                );
                rows += 9.0 / 32.0;
            }
            let mut inside = row![container(check).width(18).center_x(18)]
                .spacing(6)
                .height(Length::Fill)
                .align_y(Alignment::Center);
            if let Some(tile) = tile {
                inside = inside.push(tile);
            }
            inside = inside.push(t(name, 13.5, fade(TEXT.get(), opened)).font(if is {
                BOLD
            } else {
                MANROPE
            }));
            if let Some(aside) = aside {
                inside = inside.push(Space::new().width(Length::Fill));
                inside = inside
                    .push(t(aside, 11.5, fade(FAINT.get(), opened)).wrapping(text::Wrapping::None));
            }
            list = list.push(
                container(
                    button(inside)
                        .width(Length::Fill)
                        .height(32)
                        .padding(Padding {
                            top: 0.0,
                            right: 14.0,
                            bottom: 0.0,
                            left: 6.0,
                        })
                        .on_press(Message::Apps(Msg::Choose(menu, i)))
                        .style(flat(
                            keyboard.then(|| crate::theme::ink(rgba(255, 255, 255, 0.06))),
                            crate::theme::ink(rgba(255, 255, 255, 0.07)),
                            7.0,
                            false,
                        )),
                )
                .id(format!("option:{i}")),
            );
            rows += 1.0;
        }
        if let Menu::Default(opens) = menu {
            list = list.push(
                container(t(opens.hint().get(lang), 11.5, fade(FAINT.get(), opened))).padding(
                    Padding {
                        top: 4.0,
                        right: 8.0,
                        bottom: 2.0,
                        left: 30.0,
                    },
                ),
            );
            rows += 0.75;
        }
        let width = at.width.max(MENU_WIDTH).max(260.0);
        let height = (10.0 + 32.0 * rows).min(10.0 + 32.0 * 10.0);
        let panel = container(scrollable(list).height(height - 10.0))
            .width(width)
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
        let x = (at.x + at.width - width).max(0.0);
        let bottom = self
            .places
            .at
            .get(&format!("sidebar:{}", if lang.fr() { "fr" } else { "en" }))
            .map_or(f32::MAX, |s| s.y + s.height);
        let under = at.y + 38.0;
        let y = if under + height > bottom - 8.0 {
            (at.y - 6.0 - height).max(8.0) + 4.0 * (1.0 - opened)
        } else {
            under - 4.0 * (1.0 - opened)
        };
        Some(
            stack![
                mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                    .on_press(Message::Apps(Msg::CloseMenu)),
                pin(panel).x(x).y(y),
            ]
            .into(),
        )
    }

    /// "Ajouter au démarrage": over the window, while it is open.
    pub(in crate::app) fn apps_sheet(&self) -> Option<El<'_>> {
        let sheet = self.apps.sheet.as_ref()?;
        let lang = self.lang;
        let field = text_input(w(words::SEARCH_AN_APPLICATION).get(lang), &sheet.query)
            .id(SEARCH)
            .size(13.5)
            .padding(Padding {
                top: 8.0,
                right: 10.0,
                bottom: 8.0,
                left: 31.0,
            })
            .on_input(|q| Message::Apps(Msg::Query(q)))
            .on_submit(Message::Apps(Msg::Add))
            .style(field_style);
        let search_box = stack![
            field,
            container(icon(Glyph::Search, 15.0, DIM.get())).padding(Padding {
                top: 9.5,
                right: 0.0,
                bottom: 0.0,
                left: 10.0,
            }),
        ];
        let apps = self.sheet_list();
        let mut list = column![].spacing(2);
        for (i, app) in apps.iter().enumerate() {
            let on = i == sheet.at;
            let item = button(
                row![
                    self.app_icon(app.icon.as_deref(), &app.name, &app.id, 22.0),
                    t(app.name.clone(), 13.5, TEXT.get()),
                ]
                .spacing(10)
                .height(Length::Fill)
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .height(38)
            .padding([0, 10])
            .on_press(Message::Apps(Msg::Pick(i)))
            .style(move |_: &Theme, status| {
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: if on {
                        Some(Background::Color(crate::theme::Ink::Accent(0.13).get()))
                    } else if hovered {
                        Some(Background::Color(HOVER.get()))
                    } else {
                        None
                    },
                    text_color: TEXT.get(),
                    border: if on {
                        border(8.0, 1.0, crate::theme::Ink::Accent(0.55).get())
                    } else {
                        border(8.0, 0.0, Color::TRANSPARENT)
                    },
                    ..button::Style::default()
                }
            });
            list = list.push(container(item).id(format!("sheet:{i}")));
        }
        if apps.is_empty() {
            list = list.push(
                container(t(
                    w(words::NO_APPLICATION_TO_ADD).get(lang),
                    13.0,
                    FAINT.get(),
                ))
                .padding([10, 10]),
            );
        }
        let shown = (apps.len().clamp(1, 7) as f32) * 40.0;
        let ready = !apps.is_empty();
        let mut add = button(
            text(w(words::ADD).get(lang))
                .size(13)
                .font(BOLD)
                .color(if ready { DEEP.get() } else { DIM.get() }),
        )
        .height(30)
        .padding([6, 14])
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            let fill = if ready {
                ACCENT.get()
            } else {
                crate::theme::ink(rgba(255, 255, 255, 0.06))
            };
            button::Style {
                background: Some(Background::Color(if hovered && ready {
                    Color {
                        a: (fill.a + 0.04).min(1.0),
                        ..fill
                    }
                } else {
                    fill
                })),
                text_color: DEEP.get(),
                border: border(8.0, 0.0, Color::TRANSPARENT),
                ..button::Style::default()
            }
        });
        if ready {
            add = add.on_press(Message::Apps(Msg::Add));
        }
        let cancel = button(t(w(words::CANCEL).get(lang), 13.0, DIM.get()).font(SEMIBOLD))
            .height(30)
            .padding([6, 13])
            .style(flat(None, HOVER.get(), 8.0, false))
            .on_press(Message::Apps(Msg::Sheet(false)));
        let body = column![
            text(w(words::ADD_TO_START_UP).get(lang))
                .size(16)
                .font(BOLD)
                .color(TEXT.get()),
            search_box,
            scrollable(list).height(shown),
            row![
                Space::new().width(Length::Fill),
                container(cancel).id("sheet:cancel".to_owned()),
                container(add).id("sheet:add".to_owned()),
            ]
            .spacing(8),
        ]
        .spacing(10);
        let panel = container(body)
            .width(420)
            .padding(18)
            .style(|_: &Theme| container::Style {
                background: Some(Background::Color(MENU.get())),
                border: border(14.0, 1.0, RIM_12.get()),
                shadow: shadow(),
                ..container::Style::default()
            })
            .id("applications-sheet".to_owned());
        Some(
            stack![
                mouse_area(
                    container(Space::new().width(Length::Fill).height(Length::Fill)).style(boxed(
                        crate::theme::ink(rgba(0, 0, 0, 0.58)),
                        0.0,
                        None
                    ))
                )
                .on_press(Message::Apps(Msg::Sheet(false))),
                container(panel)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center(Length::Fill),
            ]
            .into(),
        )
    }

    /// What the page shows, for the log.
    pub(in crate::app) fn apps_shown(&self) -> serde_json::Value {
        let lang = self.lang;
        let Some(known) = self.apps.known.as_ref() else {
            return serde_json::json!({ "heard": false });
        };
        let defaults: Vec<serde_json::Value> = Opens::ALL
            .into_iter()
            .map(|opens| {
                serde_json::json!({
                    "id": opens.id(),
                    "app": known.chosen(opens),
                    "name": known.chosen(opens).map(|id| self.apps_name(id)),
                    "choices": known.handlers(opens).iter().map(|a| a.name.clone()).collect::<Vec<_>>(),
                    "inert": known.handlers(opens).is_empty(),
                    "line": if known.handlers(opens).is_empty() { opens.none() } else { opens.line() }.get(lang),
                    "differs": known.differs(opens).map(|(m, a)| model::differs_said(m, &self.apps_name(a), lang)),
                })
            })
            .collect();
        let startup: Vec<serde_json::Value> = self
            .startup_rows()
            .into_iter()
            .map(|e| {
                serde_json::json!({
                    "id": e.id,
                    "name": e.name,
                    "on": !e.hidden,
                    "own": model::users_own(e),
                    "exec": e.exec,
                })
            })
            .collect();
        let menu = self.apps.menu.map(|(m, _)| {
            let names: Vec<String> = self
                .menu_ids(m)
                .into_iter()
                .map(|id| match id.as_deref() {
                    Some("") => "auto".to_owned(),
                    Some(id) => self.apps_name(id),
                    None => "other".to_owned(),
                })
                .collect();
            serde_json::json!({ "id": m.id(), "choices": names, "at": self.apps.menu_at })
        });
        let sheet = self.apps.sheet.as_ref().map(|s| {
            serde_json::json!({
                "query": s.query,
                "at": s.at,
                "list": self.sheet_list().iter().map(|a| a.name.clone()).collect::<Vec<_>>(),
            })
        });
        serde_json::json!({
            "heard": true,
            "defaults": defaults,
            "terminal": {
                "value": self.terminal_value(),
                "chosen": self.terminal_chosen(),
                "found": known.found_terminal,
                "typing": self.apps.typing,
                "missing": self.apps.missing,
                "choices": known.terminals().iter().map(|a| a.name.clone()).collect::<Vec<_>>(),
            },
            "startup": startup,
            "others": known.others().iter().map(|e| e.name.clone()).collect::<Vec<_>>(),
            "others_shown": self.apps.others,
            "menu": menu,
            "sheet": sheet,
            "icons": self.apps.icons.len(),
        })
    }
}

/// Its first letter in lower case: a comment after " · ".
fn lower_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// A field of the page: the sheet's search, the terminal's command.
fn field_style(_: &Theme, status: text_input::Status) -> text_input::Style {
    let focused = matches!(status, text_input::Status::Focused { .. });
    text_input::Style {
        background: Background::Color(crate::theme::ink(rgba(255, 255, 255, 0.05))),
        border: border(
            9.0,
            1.0,
            if focused {
                crate::theme::Ink::Accent(0.55).get()
            } else {
                RIM_12.get()
            },
        ),
        icon: DIM.get(),
        placeholder: FAINT.get(),
        value: TEXT.get(),
        selection: crate::theme::Ink::Accent(0.35).get(),
    }
}
