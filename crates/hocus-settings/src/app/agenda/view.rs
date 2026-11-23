//! What Settings › Calendar and tasks draws (mockup s31-agenda): the
//! accounts' groups and their calendars' lines, the accounts to add, the
//! board's group and its form, with iced's widgets and the Settings' kit.

use hocus_words::settings::agenda as words;
use hocus_words::shell::setup as day1;
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock, PoisonError},
};

use hocus_day::choice::{self, Refusal, Row};
use iced::{
    Alignment, Background, Color, Length, Padding, Theme,
    widget::{Space, button, column, container, row, svg, text_input},
};

use super::{BoardRead, Connected, Listing, Msg, Online};
use crate::{
    agenda::{self as model, Board},
    app::{
        App, Message,
        style::*,
        view::{El, card_box, divided, label, solid, t},
    },
};

/// The fields' ids.
pub(super) const ADDRESS: &str = "agenda:address";
const TOKEN: &str = "agenda:token";

/// The page's colours: the up-to-date dot, what is wrong, the star.
const OK_DOT: Color = rgb(0x6CC287);
const BAD_DOT: Color = rgb(0xF2945B);
const STAR: Color = rgb(0xF5C46E);
/// The tiles' tints: Google's blue, Planka's green.
const GOOGLE: Color = rgb(0x8FB3FF);
const PLANKA: Color = rgb(0x6CC287);
/// A box, a star, unticked: their outline.
const OUTLINE: Color = rgba(255, 255, 255, 0.38);
const ON_ACCENT: Color = rgb(0x0B1020);

const STAR_PATH: &str =
    "M12 3.2l2.6 5.6 6.1.7-4.5 4.2 1.2 6.1L12 16.8l-5.4 3 1.2-6.1-4.5-4.2 6.1-.7z";
const CHECK_PATH: &str = "M2 5.2l2 2L8 3";

/// A glyph of the page, drawn once per colour.
fn glyph(star: bool, fill: Option<Color>, stroke: Color) -> svg::Handle {
    type Drawn = HashMap<(bool, Option<[u8; 4]>, [u8; 4]), svg::Handle>;
    static CACHE: OnceLock<Mutex<Drawn>> = OnceLock::new();
    let key = (star, fill.map(Color::into_rgba8), stroke.into_rgba8());
    let mut cache = CACHE
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    cache
        .entry(key)
        .or_insert_with(|| {
            let css = |c: [u8; 4]| {
                format!(
                    "rgb({},{},{})\" {}-opacity=\"{}",
                    c[0],
                    c[1],
                    c[2],
                    "X",
                    f32::from(c[3]) / 255.0
                )
            };
            let paint = |what: &str, c: Option<[u8; 4]>| match c {
                Some(c) => format!("{what}=\"{}\"", css(c).replace('X', what)),
                None => format!("{what}=\"none\""),
            };
            let (view, path, width) = if star {
                ("0 0 24 24", STAR_PATH, 1.3)
            } else {
                ("0 0 10 10", CHECK_PATH, 1.8)
            };
            svg::Handle::from_memory(
                format!(
                    "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{view}\"><path d=\"{path}\" \
                     {} {} stroke-width=\"{width}\" stroke-linecap=\"round\" \
                     stroke-linejoin=\"round\"/></svg>",
                    paint("fill", key.1),
                    paint("stroke", Some(key.2)),
                )
                .into_bytes(),
            )
        })
        .clone()
}

/// The mockup's buttons: plain, green, quiet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Plain,
    Primary,
    Quiet,
}

fn action<'a>(said: &str, kind: Kind, press: Option<Message>, id: String) -> El<'a> {
    let (fill, ink) = match kind {
        Kind::Plain => (rgba(255, 255, 255, 0.09), TEXT.get()),
        Kind::Primary => (ACCENT.get(), ON_ACCENT),
        Kind::Quiet => (Color::TRANSPARENT, DIM.get()),
    };
    let alpha = if press.is_some() { 1.0 } else { INERT };
    let mut pressed = button(
        t(said.to_owned(), 12.5, fade(ink, alpha))
            .font(BOLD)
            .wrapping(iced::widget::text::Wrapping::None),
    )
    .padding([6, 11])
    .style(move |_, status| {
        let mut style = solid(
            if kind == Kind::Quiet && matches!(status, button::Status::Hovered) {
                rgba(255, 255, 255, 0.06)
            } else {
                fade(fill, alpha)
            },
            ink,
            status,
        );
        style.text_color = fade(ink, alpha);
        style
    });
    if let Some(press) = press {
        pressed = pressed.on_press(press);
    }
    container(pressed).id(id).into()
}

/// A 32 px tile with its kind's letter.
fn tile<'a>(letter: &'static str, tint: Color) -> El<'a> {
    container(t(letter, 14.0, tint).font(BOLD))
        .width(32)
        .height(32)
        .center(32)
        .style(boxed(fade(tint, 0.16), 8.0, None))
        .into()
}

/// A state's line: its dot, green or orange, and its words.
fn state_line<'a>(said: String, bad: Option<bool>) -> El<'a> {
    let colour = match bad {
        Some(true) => PEACH.get(),
        _ => DIM.get(),
    };
    let words = t(said, 12.0, colour);
    match bad {
        None => words.into(),
        Some(bad) => row![
            container(Space::new().width(7).height(7)).style(boxed(
                if bad { BAD_DOT } else { OK_DOT },
                3.5,
                None
            )),
            words,
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into(),
    }
}

/// An account's or a board's head row: its tile, its name and its line,
/// its buttons.
fn head<'a>(tile: El<'a>, name: String, line: El<'a>, buttons: Vec<El<'a>>) -> El<'a> {
    let mut right = row![].spacing(6).align_y(Alignment::Center);
    for b in buttons {
        right = right.push(b);
    }
    container(
        row![
            tile,
            column![t(name, 13.5, TEXT.get()).font(BOLD), line]
                .spacing(1)
                .width(Length::Fill),
            right,
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding([10, 14])
    .into()
}

/// A group's label, and what it says on its right.
fn group_label<'a>(name: &'static str, right: Option<&'static str>) -> El<'a> {
    let mut line = row![label(name), Space::new().width(Length::Fill)].align_y(Alignment::Center);
    if let Some(right) = right {
        line = line.push(t(right, 12.0, FAINT.get()).font(SEMIBOLD));
    }
    container(line).height(28).center_y(28).into()
}

impl App {
    /// An account's state: what its line says, and whether it is wrong
    /// (`None`: no dot).
    pub(super) fn agenda_state(&self, connected: &Connected<'_>) -> (String, Option<bool>) {
        let lang = self.lang;
        let bad = |w: &hocus_words::Text| (w.get(lang).to_owned(), Some(true));
        let id = connected.account.to_lowercase();
        if self.agenda.plugging.as_deref() == Some(id.as_str()) {
            return (words::READING.get(lang).to_owned(), None);
        }
        match (&self.agenda.online, connected.online) {
            (Online::Unknown, _) => (words::READING.get(lang).to_owned(), None),
            (Online::Missing, _) => bad(&day1::GNOME_ONLINE_ACCOUNTS_MISSING),
            (Online::Listed(_), None) => bad(&words::NOT_IN_ONLINE_ACCOUNTS),
            (_, Some(online)) if online.attention => bad(&words::ACCESS_EXPIRED),
            (_, Some(online)) if !online.calendar => bad(&words::CALENDAR_OFF),
            (_, Some(_)) => match self.agenda.read.get(&id) {
                None | Some(Listing::Reading) => (words::READING.get(lang).to_owned(), None),
                Some(Listing::Failed) => bad(&words::UNREADABLE),
                Some(Listing::Read { at, .. }) => {
                    let minutes = self.now.saturating_duration_since(*at).as_secs() / 60;
                    let said = if minutes == 0 {
                        words::UP_TO_DATE.get(lang).to_owned()
                    } else {
                        words::up_to_date_ago(lang, minutes)
                    };
                    (said, Some(false))
                }
            },
        }
    }

    pub(in crate::app) fn agenda_page<'a>(
        &'a self,
        mut page: iced::widget::Column<'a, Message>,
    ) -> El<'a> {
        let lang = self.lang;
        let connected = self.agenda_connected();
        let others = self.agenda_others();
        // AGENDAS.
        let mut calendars = column![group_label(
            words::CALENDARS.get(lang),
            (!connected.is_empty()).then(|| words::STAR_NOTE.get(lang)),
        )]
        .spacing(10);
        for (n, account) in connected.iter().enumerate() {
            let mut group = self.agenda_account(account);
            if n == 0 {
                group = container(group)
                    .id(format!("row:{}", model::COLOURS))
                    .into();
            }
            calendars = calendars.push(group);
        }
        for other in &others {
            let plug = Message::Agenda(Msg::Plug(other.identity.clone()));
            let kind = if connected.is_empty() {
                Kind::Primary
            } else {
                Kind::Plain
            };
            let line = if connected.is_empty() {
                words::IN_ONLINE_ACCOUNTS
            } else {
                words::NOT_IN_THE_BAR
            };
            calendars = calendars.push(card_box(head(
                tile("G", GOOGLE),
                other.identity.clone(),
                state_line(line.get(lang).to_owned(), None),
                vec![action(
                    words::PLUG.get(lang),
                    kind,
                    Some(plug),
                    format!("agenda:plug:{}", other.identity),
                )],
            )));
        }
        calendars =
            calendars.push(self.agenda_online_row(connected.is_empty() && others.is_empty()));
        page = page.push(container(calendars).id(format!("row:{}", model::CALENDARS)));
        // TÂCHES.
        page = page.push(
            container(
                column![
                    group_label(words::TASKS.get(lang), None),
                    self.agenda_tasks()
                ]
                .spacing(10),
            )
            .id(format!("row:{}", model::BOARD)),
        );
        container(page).id("page:agenda".to_owned()).into()
    }

    /// The row under the accounts: how another comes, and the button
    /// that opens Online Accounts; alone without any account, in a card.
    fn agenda_online_row(&self, alone: bool) -> El<'_> {
        let lang = self.lang;
        let panel = model::panel().map(|_| Message::Agenda(Msg::Panel));
        if !alone {
            return container(
                row![
                    t(words::ANOTHER_ACCOUNT.get(lang), 12.0, FAINT.get()).width(Length::Fill),
                    action(
                        words::OPEN_ONLINE_ACCOUNTS.get(lang),
                        Kind::Quiet,
                        panel,
                        "agenda:panel".to_owned(),
                    ),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            )
            .padding([0, 2])
            .into();
        }
        let (line, panel) = match self.agenda.online {
            Online::Missing => (day1::GNOME_ONLINE_ACCOUNTS_MISSING, None),
            _ => (day1::ADD_A_GOOGLE_ACCOUNT_IN_ONLINE, panel),
        };
        card_box(head(
            tile("G", GOOGLE),
            day1::GOOGLE_CALENDAR.get(lang).to_owned(),
            state_line(line.get(lang).to_owned(), None),
            vec![action(
                words::OPEN_ONLINE_ACCOUNTS.get(lang),
                Kind::Plain,
                panel,
                "agenda:panel".to_owned(),
            )],
        ))
    }

    /// A connected account's group: its head, its calendars.
    fn agenda_account(&self, connected: &Connected<'_>) -> El<'_> {
        let lang = self.lang;
        let account = connected.account.to_owned();
        let (said, bad) = self.agenda_state(connected);
        let mut buttons = Vec::new();
        let expired = connected.online.is_some_and(|o| o.attention || !o.calendar);
        if expired {
            buttons.push(action(
                words::OPEN_ONLINE_ACCOUNTS.get(lang),
                Kind::Plain,
                model::panel().map(|_| Message::Agenda(Msg::Panel)),
                format!("agenda:panel:{account}"),
            ));
        } else if self.agenda.read.get(&account.to_lowercase()) == Some(&Listing::Failed) {
            buttons.push(action(
                words::RETRY.get(lang),
                Kind::Plain,
                Some(Message::Agenda(Msg::Retry(account.clone()))),
                format!("agenda:retry:{account}"),
            ));
        }
        if !expired || connected.online.is_none() {
            buttons.push(action(
                words::REMOVE.get(lang),
                Kind::Quiet,
                self.writable()
                    .then(|| Message::Agenda(Msg::Remove(account.clone()))),
                format!("agenda:remove:{account}"),
            ));
        }
        let mut group = column![head(
            tile("G", GOOGLE),
            account.clone(),
            state_line(said, bad),
            buttons,
        )];
        for (i, calendar) in self.agenda_rows(connected).iter().enumerate() {
            group = group.push(divided(true, self.agenda_calendar(&account, i, calendar)));
        }
        card_box(group.into())
    }

    /// A calendar's line, as Day.1's step 3 draws it, and its colours
    /// unfolded under it.
    fn agenda_calendar(&self, account: &str, i: usize, calendar: &Row) -> El<'_> {
        let lang = self.lang;
        let id = |what: &str| format!("agenda:{what}:{account}:{i}");
        let message = |m: Msg| self.writable().then_some(Message::Agenda(m));
        let on = calendar.shown;
        let tick = button(
            container(if on {
                El::from(svg(glyph(false, None, ON_ACCENT)).width(10).height(10))
            } else {
                Space::new().into()
            })
            .center(Length::Fill),
        )
        .width(16)
        .height(16)
        .padding(0)
        .style(move |_: &Theme, _| button::Style {
            background: on.then(|| Background::Color(ACCENT.get())),
            border: border(4.0, 1.5, if on { ACCENT.get() } else { OUTLINE }),
            ..button::Style::default()
        });
        let tick = match message(Msg::Toggle(account.to_owned(), i)) {
            Some(m) => tick.on_press(m),
            None => tick,
        };
        let colour = colour_of(calendar.colour);
        let swatch =
            button(Space::new().width(16).height(16))
                .padding(0)
                .style(move |_: &Theme, _| button::Style {
                    background: Some(Background::Color(colour)),
                    border: border(8.0, 0.0, Color::TRANSPARENT),
                    ..button::Style::default()
                });
        let swatch = match message(Msg::Unfold(account.to_owned(), i)) {
            Some(m) => swatch.on_press(m),
            None => swatch,
        };
        // Half strength on a calendar not shown: it counts for nothing.
        let alpha = if on { 1.0 } else { 0.5 };
        let star = if calendar.featured {
            glyph(true, Some(fade(STAR, alpha)), fade(STAR, alpha))
        } else {
            glyph(true, None, fade(OUTLINE, alpha))
        };
        let star = button(svg(star).width(16).height(16))
            .width(20)
            .height(20)
            .padding(2)
            .style(|_: &Theme, _| button::Style::default());
        let star = match message(Msg::Feature(account.to_owned(), i)) {
            Some(m) => star.on_press(m),
            None => star,
        };
        let mut line = row![
            container(tick).id(id("box")),
            container(swatch).id(id("swatch")),
            container(star).id(id("star")),
            t(
                calendar.name.clone(),
                13.0,
                if on { TEXT.get() } else { DIM.get() }
            )
            .width(Length::Fill),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        if calendar.read_only {
            line = line.push(t(day1::READ_ONLY.get(lang), 11.5, FAINT.get()));
        }
        let mut whole = column![container(line).height(36).center_y(36)];
        if self.agenda.unfolded.as_ref() == Some(&(account.to_owned(), i)) {
            let mut colours = row![].spacing(6);
            for j in 0..choice::PALETTE.len() {
                let fill = colour_of(j);
                let chosen = j == calendar.colour;
                let dot = button(Space::new().width(20).height(20))
                    .padding(0)
                    .style(move |_: &Theme, _| button::Style {
                        background: Some(Background::Color(fill)),
                        border: border(10.0, if chosen { 1.5 } else { 0.0 }, TEXT.get()),
                        ..button::Style::default()
                    })
                    .on_press(Message::Agenda(Msg::Colour(account.to_owned(), i, j)));
                colours =
                    colours.push(container(dot).id(format!("agenda:colour:{account}:{i}:{j}")));
            }
            whole = whole.push(container(colours).padding(Padding {
                top: 2.0,
                right: 0.0,
                bottom: 10.0,
                left: 52.0,
            }));
        }
        container(whole).padding([0, 14]).into()
    }

    /// The tasks' group: the board, or the local list.
    fn agenda_tasks(&self) -> El<'_> {
        let lang = self.lang;
        let Some(entry) = self.agenda_board() else {
            let n = self.agenda.local.unwrap_or_default();
            let mut group = column![
                container(
                    row![
                        column![
                            t(words::ON_THIS_COMPUTER.get(lang), 13.5, TEXT.get()).font(BOLD),
                            t(words::local_line(lang, n), 12.0, DIM.get()),
                        ]
                        .spacing(1)
                        .width(Length::Fill),
                        action(
                            words::PLUG_A_BOARD.get(lang),
                            Kind::Plain,
                            (self.agenda.form.is_none() && self.writable())
                                .then_some(Message::Agenda(Msg::ChangeBoard)),
                            "agenda:plug-board".to_owned(),
                        ),
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                )
                .padding([10, 14])
            ];
            if let Some(form) = self.agenda_form() {
                group = group.push(divided(true, form));
            }
            return card_box(group.into());
        };
        let (name, line, bad) = match &self.agenda.board {
            BoardRead::Read(Board::Read(found)) => {
                let host = hocus_day::planka::address(&entry.address, &entry.board)
                    .map(|(server, _)| server.host().to_owned())
                    .unwrap_or_default();
                let cards = self
                    .agenda_columns()
                    .and_then(|(_, columns)| found.cards.get(columns[0]).copied())
                    .unwrap_or_default();
                (
                    found.board.name.clone(),
                    format!(
                        "{} · {}",
                        words::planka(lang, &host),
                        words::to_do(lang, cards)
                    ),
                    Some(false),
                )
            }
            BoardRead::Read(Board::Refused(refusal)) => (
                entry.address.clone(),
                refused_words(*refusal).get(lang).to_owned(),
                Some(true),
            ),
            BoardRead::Unknown | BoardRead::Reading => (
                entry.address.clone(),
                day1::SIGNING_IN.get(lang).to_owned(),
                None,
            ),
        };
        let writable = self.writable();
        let mut group = column![head(
            tile("P", PLANKA),
            name,
            state_line(line, bad),
            vec![
                action(
                    words::CHANGE_BOARD.get(lang),
                    Kind::Quiet,
                    (self.agenda.form.is_none() && writable)
                        .then_some(Message::Agenda(Msg::ChangeBoard)),
                    "agenda:change-board".to_owned(),
                ),
                action(
                    words::REMOVE.get(lang),
                    Kind::Quiet,
                    writable.then_some(Message::Agenda(Msg::RemoveBoard)),
                    "agenda:remove-board".to_owned(),
                ),
            ],
        )];
        let names = entry.columns();
        let said = [names.tasks, names.done, names.add];
        let rows = [
            (day1::TASKS_COLUMN, words::TASKS_LINE),
            (day1::DONE_COLUMN, words::DONE_LINE),
            (day1::ADD_TO, words::ADD_LINE),
        ];
        let read = self.agenda_columns();
        for (which, (title, under)) in rows.iter().enumerate() {
            let chosen = match &read {
                Some((found, columns)) => found
                    .board
                    .columns
                    .get(columns[which])
                    .map(|c| c.name.clone())
                    .unwrap_or_default(),
                None => said[which].clone(),
            };
            let open = self.agenda.menu == Some(which);
            let menu = button(
                row![t(chosen, 12.5, TEXT.get()), t(" ▾", 12.5, FAINT.get()),]
                    .align_y(Alignment::Center),
            )
            .padding([5, 10])
            .style(move |_: &Theme, status| button::Style {
                background: Some(Background::Color(
                    if open || matches!(status, button::Status::Hovered) {
                        rgba(255, 255, 255, 0.1)
                    } else {
                        rgba(255, 255, 255, 0.06)
                    },
                )),
                border: border(7.0, 1.0, RIM_12.get()),
                text_color: TEXT.get(),
                ..button::Style::default()
            });
            let menu = match (&read, writable) {
                (Some(_), true) => menu.on_press(Message::Agenda(Msg::Menu(which))),
                _ => menu,
            };
            let mut block = column![
                container(
                    row![
                        column![
                            t(title.get(lang), 13.5, TEXT.get()).font(BOLD),
                            t(under.get(lang), 12.0, DIM.get()),
                        ]
                        .spacing(1)
                        .width(Length::Fill),
                        container(menu).id(format!("agenda:menu:{which}")),
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                )
                .padding([10, 14])
            ];
            if let (true, Some((found, columns))) = (open, &read) {
                let mut list = column![];
                for (j, column) in found.board.columns.iter().enumerate() {
                    let mut item =
                        row![t(column.name.clone(), 13.0, TEXT.get()).width(Length::Fill)];
                    if j == columns[which] {
                        item = item.push(t("✓", 13.0, MINT.get()));
                    }
                    list = list.push(
                        container(
                            button(container(item).center_y(28).padding([0, 10]))
                                .width(Length::Fill)
                                .padding(0)
                                .on_press(Message::Agenda(Msg::Column(which, j)))
                                .style(flat(None, HOVER.get(), 7.0, false)),
                        )
                        .id(format!("agenda:column:{which}:{j}")),
                    );
                }
                block = block.push(
                    row![
                        Space::new().width(Length::Fill),
                        container(list).padding(4).width(200).style(boxed(
                            MENU.get(),
                            10.0,
                            Some((1.0, RIM_12.get()))
                        )),
                    ]
                    .padding(Padding {
                        top: 0.0,
                        right: 14.0,
                        bottom: 10.0,
                        left: 14.0,
                    }),
                );
            }
            group = group.push(divided(true, block.into()));
        }
        if let Some(form) = self.agenda_form() {
            group = group.push(divided(true, form));
        }
        card_box(group.into())
    }

    /// "Changer de tableau…" unfolded: the address, the token, what went
    /// wrong (step 2's words), the boards of a server.
    fn agenda_form(&self) -> Option<El<'_>> {
        let form = self.agenda.form.as_ref()?;
        let lang = self.lang;
        let refused = form.refused;
        let field = |label_said: &'static str,
                     id: &'static str,
                     hint: &'static str,
                     value: &str,
                     secure: bool,
                     wrong: bool,
                     typed: fn(String) -> Msg| {
            let input = text_input(hint, value)
                .id(id)
                .secure(secure)
                .size(13.0)
                .font(MANROPE)
                .padding([8, 10])
                .on_input(move |s| Message::Agenda(typed(s)))
                .on_submit(Message::Agenda(Msg::SignIn))
                .style(move |_: &Theme, status| {
                    let focused = matches!(status, text_input::Status::Focused { .. });
                    text_input::Style {
                        background: Background::Color(rgba(255, 255, 255, 0.05)),
                        border: border(
                            8.0,
                            1.0,
                            if wrong {
                                BAD_DOT
                            } else if focused {
                                ACCENT.get()
                            } else {
                                rgba(255, 255, 255, 0.14)
                            },
                        ),
                        icon: DIM.get(),
                        placeholder: FAINT.get(),
                        value: TEXT.get(),
                        selection: ACCENT.get().scale_alpha(0.35),
                    }
                });
            column![
                t(label_said, 12.0, DIM.get()).font(SEMIBOLD),
                container(input).id(format!("{id}:box")),
            ]
            .spacing(6)
            .width(Length::Fill)
        };
        let ready = !form.address.trim().is_empty() && !form.token.is_empty() && !form.checking;
        let mut body = column![
            row![
                field(
                    day1::BOARD_ADDRESS.get(lang),
                    ADDRESS,
                    "https://…/boards/…",
                    &form.address,
                    false,
                    matches!(
                        refused,
                        Some(Refusal::Address | Refusal::NoServer | Refusal::NotShared)
                    ),
                    Msg::Address,
                ),
                field(
                    day1::ACCESS_TOKEN.get(lang),
                    TOKEN,
                    words::PASTE_IT_HERE.get(lang),
                    &form.token,
                    true,
                    refused == Some(Refusal::Token),
                    Msg::Token,
                ),
            ]
            .spacing(12),
        ]
        .spacing(10);
        if let Some(refusal) = refused {
            body = body.push(
                container(t(refused_words(refusal).get(lang), 12.5, PEACH.get()))
                    .id("agenda:refused".to_owned()),
            );
        }
        if let Some((_, boards)) = &form.boards {
            let mut list = column![t(words::CHOOSE_A_BOARD.get(lang), 12.0, DIM.get())].spacing(4);
            for (i, board) in boards.iter().enumerate() {
                list = list.push(
                    container(
                        button(
                            column![
                                t(board.name.clone(), 13.0, TEXT.get()).font(SEMIBOLD),
                                t(board.project.clone(), 12.0, DIM.get()),
                            ]
                            .spacing(1),
                        )
                        .width(Length::Fill)
                        .padding([6, 10])
                        .on_press(Message::Agenda(Msg::Pick(i)))
                        .style(flat(None, HOVER.get(), 7.0, false)),
                    )
                    .id(format!("agenda:board:{i}")),
                );
            }
            body = body.push(list);
        }
        let sign_in = if form.checking {
            day1::SIGNING_IN
        } else {
            day1::SIGN_IN
        };
        body = body.push(
            row![
                t(
                    day1::IN_PLANKA_ONCE_SIGNED_IN_THE.get(lang),
                    12.0,
                    FAINT.get()
                )
                .width(Length::Fill),
                action(
                    day1::CANCEL.get(lang),
                    Kind::Quiet,
                    Some(Message::Agenda(Msg::CancelBoard)),
                    "agenda:cancel".to_owned(),
                ),
                action(
                    sign_in.get(lang),
                    Kind::Primary,
                    ready.then_some(Message::Agenda(Msg::SignIn)),
                    "agenda:sign-in".to_owned(),
                ),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        );
        Some(container(body).padding([12, 14]).into())
    }

    /// Whether the page's minutes move: an account read, the page shown.
    pub(in crate::app) fn agenda_ticking(&self) -> bool {
        self.page == crate::app::Page::Section(crate::model::SectionId::Agenda)
            && self
                .agenda
                .read
                .values()
                .any(|r| matches!(r, Listing::Read { .. }))
    }
}

/// Colour `i` of the eight.
fn colour_of(i: usize) -> Color {
    let [r, g, b] = choice::colour(i).0;
    Color::from_rgb8(r, g, b)
}

/// What a board's refusal says: step 2's words.
fn refused_words(refusal: Refusal) -> hocus_words::Text {
    match refusal {
        Refusal::Token => day1::TOKEN_REFUSED_OR_EXPIRED_PASTE_A,
        Refusal::NoServer => day1::NO_PLANKA_SERVER_ANSWERS_AT_THIS,
        Refusal::NotShared => day1::THIS_BOARD_DOES_NOT_EXIST_OR,
        Refusal::Address => day1::PASTE_A_BOARD_S_ADDRESS_HTTPS,
    }
}
