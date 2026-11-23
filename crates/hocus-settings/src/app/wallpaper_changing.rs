//! The wallpaper page's backgrounds that change by themselves (#894):
//! the tiles "Un dossier" and "Jour et nuit", and their groups under the
//! gallery. LE DOSSIER: the folder (chosen with the portal's file
//! chooser, a folder this time) and how many pictures it holds, how often
//! and in which order they change, "Image suivante" (told the compositor
//! on its line, as the actions palette's). JOUR ET NUIT: the two
//! pictures, each from the gallery's, and when they switch: the sun's
//! rise and set at the weather's place, computed here as the compositor
//! does, else the two hours of the file.
//!
//! What the screens show now is the compositor's to say (`wallpaper-now`
//! on its line): the preview draws it; before it does, a folder's first
//! picture and the day's.

use hocus_words::settings::app_wallpaper_page as words;

use hocus_link::wallpaper::Picture;
use iced::{
    Alignment, Background, Color, ContentFit, Length, Padding, Theme,
    widget::{Space, button, column, container, image, row, stack},
};

use super::{
    App, Control, Doing, KeysSaid, Message, ToastAction,
    style::*,
    view::{El, card_box, divided, icon, label, t},
    wallpaper_page::{Msg, Stop, TILE_HIGH, Tile, quoted, same},
};
use crate::{
    model::Icon,
    store::KeyEdit,
    wallpaper::{self as model, Changing, For, Thumb},
};

/// Day and night's tile's halves, in pixels: drawn at their shape, a
/// picture drawn covering a shape not its own overflows it.
const HALF: (u32, u32) = (128, 152);
/// The folder's tile's two cards.
const CARD_SIZE: (u32, u32) = (232, 132);
/// Day's and night's slots in their group.
const SLOT: (u32, u32) = (680, 128);

/// Day's picture or night's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    Day,
    Night,
}

impl When {
    /// What a picture chosen in the file chooser is for.
    pub fn to(self) -> For {
        match self {
            Self::Day => For::Day,
            Self::Night => For::Night,
        }
    }

    fn word(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Night => "night",
        }
    }
}

/// A 28 px secondary button, ringed while it holds the keyboard.
fn small_button<'a>(said: String, press: Option<Message>, focused: bool, id: String) -> El<'a> {
    let alpha = if press.is_some() { 1.0 } else { INERT };
    let mut b = button(t(said, 12.5, fade(TEXT.get(), alpha)).font(SEMIBOLD))
        .height(28)
        .padding([5, 11])
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(Background::Color(fade(
                    crate::theme::ink(rgba(255, 255, 255, if hovered { 0.10 } else { 0.06 })),
                    alpha,
                ))),
                text_color: TEXT.get(),
                border: if focused {
                    ring(7.0)
                } else {
                    border(7.0, 1.0, fade(RIM_12.get(), alpha))
                },
                ..button::Style::default()
            }
        });
    if let Some(press) = press {
        b = b.on_press(press);
    }
    container(b).id(id).into()
}

/// `at` on the wall clock, "07:58": the sun's hours as the Settings say
/// them (the wallpaper's day and night, the night light).
pub(in crate::app) fn clock_hhmm(at: jiff::Timestamp) -> String {
    at.to_zoned(jiff::tz::TimeZone::system())
        .strftime("%H:%M")
        .to_string()
}

/// A row of the page's own: its name, its line, its control on the right.
fn own_row<'a>(id: &str, name: String, line: Option<String>, control: El<'a>) -> El<'a> {
    let mut words = column![t(name, 14.0, TEXT.get()).font(SEMIBOLD)]
        .spacing(2)
        .width(Length::Fill);
    if let Some(line) = line {
        words = words.push(t(line, 12.5, DIM.get()));
    }
    container(
        row![
            words,
            row![Space::new().height(36), control].align_y(Alignment::Center)
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
    .id(format!("row:{id}"))
    .into()
}

impl App {
    /// What changes by itself on the gallery's target, as the file says.
    fn target_changing(&self) -> Changing {
        let target = self.wallpaper_target().map(|s| s.name);
        self.file().changing_of(target.as_deref())
    }

    /// The target's folder, as written (`~/…`).
    fn target_folder(&self) -> Option<String> {
        self.target_changing().folder().map(ToOwned::to_owned)
    }

    /// What `output` shows now: the compositor's word for a folder's or
    /// day and night's; before it says, the folder's first picture, the
    /// day's.
    pub(super) fn shown_picture(&self, output: &str) -> Picture {
        let file = self.file();
        let picture = file.picture_of(output);
        if !picture.changes() {
            return picture;
        }
        let told = self
            .wallpaper
            .now
            .iter()
            .find(|(name, _)| name == output)
            .and_then(|(_, word)| Picture::of(word).ok())
            .filter(|p| !p.changes());
        if let Some(told) = told {
            return told;
        }
        match picture {
            Picture::Folder => self
                .wallpaper
                .counted
                .as_ref()
                .and_then(|(_, _, first)| first.first())
                .map_or_else(Picture::default, |p| Picture::File(p.display().to_string())),
            _ => file.changing_of(Some(output)).day(),
        }
    }

    /// The tiles' and the slots' pictures (#894).
    pub(super) fn changing_thumbs(&self) -> Vec<Thumb> {
        let mut out: Vec<Thumb> = self
            .wallpaper
            .counted
            .iter()
            .flat_map(|(_, _, first)| first.iter())
            .map(|p| self.sized_thumb(&Picture::File(p.display().to_string()), CARD_SIZE))
            .collect();
        let changing = self.target_changing();
        for picture in [changing.day(), changing.night()] {
            out.push(self.sized_thumb(&picture, HALF));
            if self.target_picture() == Picture::DayNight {
                out.push(self.sized_thumb(&picture, SLOT));
            }
        }
        out
    }

    /// The target's folder counted on the page's thread, once a folder.
    pub(super) fn changing_want(&mut self) {
        if !self.wallpaper_shows() || self.snapshot.is_none() {
            return;
        }
        let Some(folder) = self.target_folder() else {
            return;
        };
        let path = hocus_link::wallpaper::expand(&folder);
        if self.wallpaper.count_asked.as_ref() == Some(&path) {
            return;
        }
        let asked = self
            .wallpaper_worker()
            .is_some_and(|w| w.ask(model::Ask::Count(path.clone())));
        if asked {
            self.wallpaper.count_asked = Some(path);
        }
    }

    /// The compositor says what each screen shows now.
    pub(super) fn wallpaper_now(&mut self, now: Vec<(String, String)>) -> iced::Task<Message> {
        if self.wallpaper.now != now {
            tracing::debug!(?now, "the wallpapers shown now");
            self.wallpaper.now = now;
            // The folder may have changed too: counted again.
            self.wallpaper.count_asked = None;
            self.wallpaper_want();
        }
        iced::Task::none()
    }

    /// The page's stops of a folder's or day and night's group.
    pub(super) fn changing_controls(&self) -> Vec<Control> {
        match self.target_picture() {
            Picture::Folder => vec![
                Control::Wallpaper(Stop::ChangeFolder),
                self.setting_control(model::EVERY),
                self.setting_control(model::ORDER),
                Control::Wallpaper(Stop::Next),
            ],
            Picture::DayNight => {
                let mut out = vec![
                    Control::Wallpaper(Stop::Slot(When::Day)),
                    Control::Wallpaper(Stop::Slot(When::Night)),
                ];
                if self.file().place.is_none() {
                    out.push(self.setting_control(model::DAY_STARTS));
                    out.push(self.setting_control(model::NIGHT_STARTS));
                }
                out
            }
            _ => Vec::new(),
        }
    }

    // --- What changes it -------------------------------------------------

    /// The tile "Un dossier": the target's folder, or one chosen first.
    pub(super) fn wallpaper_choose_folder(&mut self) -> iced::Task<Message> {
        if self.target_folder().is_some() {
            self.wallpaper_choose(&Picture::Folder)
        } else {
            self.wallpaper_pick_folder()
        }
    }

    /// "Changer de dossier…": the file chooser, a folder.
    pub(super) fn wallpaper_pick_folder(&mut self) -> iced::Task<Message> {
        if !self.writable() {
            return iced::Task::none();
        }
        if let Some(worker) = self.wallpaper_worker() {
            worker.ask(model::Ask::PickFolder { lang: self.lang });
        }
        iced::Task::none()
    }

    /// The key of the target's own `what` (`folder`, `day`, `night`).
    fn changing_key(&self, what: &str) -> Vec<String> {
        match self.wallpaper_target() {
            Some(screen) => vec!["outputs".into(), screen.name, format!("wallpaper-{what}")],
            None => vec!["wallpaper".into(), what.into()],
        }
    }

    /// The picture key of the target, `"folder"` or `"day-night"`, unless
    /// it says it already.
    fn changing_picture_edit(&self, picture: &Picture) -> Option<KeyEdit> {
        if same(&self.target_picture(), picture) {
            return None;
        }
        let key = match self.wallpaper_target() {
            Some(screen) => vec!["outputs".into(), screen.name, "wallpaper".into()],
            None => vec!["wallpaper".into(), "picture".into()],
        };
        Some(KeyEdit {
            key,
            value: Some(quoted(picture.word())),
        })
    }

    /// A folder chosen: the target's, and its wallpaper the folder's.
    pub(super) fn wallpaper_folder_picked(
        &mut self,
        chosen: crate::portal::Chosen,
    ) -> iced::Task<Message> {
        let lang = self.lang;
        let path = match chosen {
            crate::portal::Chosen::Path(path) => path,
            crate::portal::Chosen::Cancelled => return iced::Task::none(),
            crate::portal::Chosen::NoPortal => {
                let text = self
                    .say(words::NO_FILE_CHOOSER_ANSWERS_THE_DESKTOP)
                    .to_owned();
                return self.toast(text, ToastAction::None, super::TOAST);
            }
        };
        let written = model::written(&path);
        tracing::info!(folder = written, "the chosen folder is the background's");
        let then = self
            .target_folder()
            .unwrap_or_else(|| model::name_of(&self.target_picture(), lang));
        let mut edits = vec![KeyEdit {
            key: self.changing_key("folder"),
            value: Some(quoted(&written)),
        }];
        edits.extend(self.changing_picture_edit(&Picture::Folder));
        let said = KeysSaid {
            done: words::folder_written(lang, &written),
            undone: words::undone_the_folder_is_back_to(lang, &then),
            redone: words::redone_2(lang, &written),
            row: model::FOLDER,
        };
        self.wallpaper.counted = None;
        self.wallpaper.count_asked = None;
        self.ask_keys(edits, said, Doing::New, None);
        iced::Task::none()
    }

    /// "Suivante": the compositor shows the folder's next picture.
    pub(super) fn wallpaper_next(&mut self) -> iced::Task<Message> {
        let line = match self.wallpaper_target() {
            Some(screen) => format!("wallpaper-next {}", screen.name),
            None => "wallpaper-next".to_owned(),
        };
        if !self.screens_ask(line) {
            tracing::info!("no compositor's line: no next picture");
        }
        iced::Task::none()
    }

    /// Day's or night's picture.
    pub(super) fn wallpaper_slot_picture(&mut self, when: When, picture: &Picture) {
        let lang = self.lang;
        let changing = self.target_changing();
        let before = match when {
            When::Day => changing.day(),
            When::Night => changing.night(),
        };
        if same(&before, picture) {
            return;
        }
        let now = model::name_of(picture, lang);
        let then = model::name_of(&before, lang);
        let said = KeysSaid {
            done: match when {
                When::Day => words::day_picture(lang, &now),
                When::Night => words::night_picture(lang, &now),
            },
            undone: match when {
                When::Day => words::undone_the_day_is_back_to(lang, &then),
                When::Night => words::undone_the_night_is_back_to(lang, &then),
            },
            redone: words::redone_2(lang, &now),
            row: model::DAY_NIGHT,
        };
        let mut edits = vec![KeyEdit {
            key: self.changing_key(when.word()),
            value: Some(quoted(picture.word())),
        }];
        edits.extend(self.changing_picture_edit(&Picture::DayNight));
        self.ask_keys(edits, said, Doing::New, None);
    }

    // --- What it draws -----------------------------------------------------

    /// A tile's thumbnail, or the card's colour until it is drawn.
    /// `picture` drawn `size` pixels large, covering it, for the tiles
    /// and slots that are not a tile's shape.
    fn sized_thumb(&self, picture: &Picture, size: (u32, u32)) -> Thumb {
        Thumb {
            size,
            ..self.tile_thumb(picture)
        }
    }

    fn picture_thumb<'a>(&self, picture: &Picture, size: (u32, u32), height: f32) -> El<'a> {
        let thumb = self.sized_thumb(picture, size);
        match self.wallpaper.thumbs.drawn.get(&thumb) {
            Some(handle) => image(handle.clone())
                .width(Length::Fill)
                .height(height)
                .content_fit(ContentFit::Fill)
                .into(),
            None => container(Space::new().width(Length::Fill).height(height))
                .style(|_: &Theme| container::Style {
                    background: Some(Background::Color(CARD.get())),
                    ..container::Style::default()
                })
                .into(),
        }
    }

    /// The tiles "Un dossier" (two of its pictures, one behind the other)
    /// and "Jour et nuit" (the day's beside the night's).
    pub(super) fn changing_tile<'a>(&self, tile: &Tile) -> El<'a> {
        match tile {
            Tile::Folder => {
                let first: Vec<Picture> = self
                    .wallpaper
                    .counted
                    .iter()
                    .flat_map(|(_, _, first)| first.iter())
                    .map(|p| Picture::File(p.display().to_string()))
                    .collect();
                let back = first.get(1).or(first.first()).cloned();
                let card = |picture: Option<Picture>, faded: bool| -> El<'a> {
                    let inner: El<'a> = match picture {
                        Some(p) => self.picture_thumb(&p, CARD_SIZE, TILE_HIGH - 10.0),
                        None => {
                            container(Space::new().width(Length::Fill).height(TILE_HIGH - 10.0))
                                .style(move |_: &Theme| container::Style {
                                    background: Some(Background::Color(CARD.get())),
                                    ..container::Style::default()
                                })
                                .into()
                        }
                    };
                    container(inner)
                        .width(Length::Fill)
                        .style(move |_: &Theme| container::Style {
                            border: border(
                                5.0,
                                1.0,
                                crate::theme::ink(rgba(
                                    255,
                                    255,
                                    255,
                                    if faded { 0.10 } else { 0.22 },
                                )),
                            ),
                            ..container::Style::default()
                        })
                        .into()
                };
                stack![
                    container(card(back, true)).padding(Padding {
                        top: 0.0,
                        right: 0.0,
                        bottom: 10.0,
                        left: 14.0,
                    }),
                    container(card(first.first().cloned(), false)).padding(Padding {
                        top: 10.0,
                        right: 14.0,
                        bottom: 0.0,
                        left: 0.0,
                    }),
                ]
                .width(Length::Fill)
                .height(TILE_HIGH)
                .into()
            }
            _ => {
                let changing = self.target_changing();
                row![
                    self.picture_thumb(&changing.day(), HALF, TILE_HIGH),
                    self.picture_thumb(&changing.night(), HALF, TILE_HIGH),
                ]
                .width(Length::Fill)
                .height(TILE_HIGH)
                .into()
            }
        }
    }

    /// Whether a tile that changes has its pictures drawn (tests).
    pub(super) fn changing_tile_drawn(&self, tile: &Tile) -> bool {
        let drawn = |p: Picture, size| {
            self.wallpaper
                .thumbs
                .drawn
                .contains_key(&self.sized_thumb(&p, size))
        };
        match tile {
            Tile::Folder => self
                .wallpaper
                .counted
                .iter()
                .flat_map(|(_, _, first)| first.iter())
                .all(|p| drawn(Picture::File(p.display().to_string()), CARD_SIZE)),
            _ => {
                let changing = self.target_changing();
                drawn(changing.day(), HALF) && drawn(changing.night(), HALF)
            }
        }
    }

    /// LE DOSSIER or JOUR ET NUIT, as the target's wallpaper is.
    pub(super) fn changing_group(&self) -> Option<El<'_>> {
        match self.target_picture() {
            Picture::Folder => Some(self.folder_group()),
            Picture::DayNight => Some(self.day_night_group()),
            _ => None,
        }
    }

    fn folder_group(&self) -> El<'_> {
        let lang = self.lang;
        let inert = !self.writable();
        let controls = self.controls();
        let folder = self.target_folder();
        let path = folder.as_deref().map(hocus_link::wallpaper::expand);
        let count = self
            .wallpaper
            .counted
            .as_ref()
            .filter(|(p, _, _)| Some(p) == path.as_ref())
            .map(|(_, n, _)| *n);
        let (name, line, press) = match &folder {
            Some(folder) => (
                folder.clone(),
                Some(match count {
                    Some(n) => words::pictures_in_it(lang, n),
                    None => words::COUNTING.get(lang).to_owned(),
                }),
                words::CHANGE_FOLDER,
            ),
            None => (
                words::NO_FOLDER_CHOSEN.get(lang).to_owned(),
                None,
                words::CHOOSE_A_FOLDER,
            ),
        };
        let change = small_button(
            press.get(lang).to_owned(),
            (!inert).then_some(Message::Wallpaper(Msg::ChangeFolder)),
            self.focused_stop(&Stop::ChangeFolder),
            "control:wallpaper.folder".to_owned(),
        );
        let next = small_button(
            words::NEXT.get(lang).to_owned(),
            folder
                .as_ref()
                .filter(|_| !inert)
                .map(|_| Message::Wallpaper(Msg::Next)),
            self.focused_stop(&Stop::Next),
            "control:wallpaper.next".to_owned(),
        );
        let row_of = |id: &str| model::settings().iter().find(|s| s.id == id);
        let mut card = column![own_row(model::FOLDER, name, line, change)];
        for id in [model::EVERY, model::ORDER] {
            if let Some(setting) = row_of(id) {
                card = card.push(divided(true, self.row(setting, &controls)));
            }
        }
        card = card.push(divided(
            true,
            own_row(
                model::NEXT,
                words::NEXT_PICTURE.get(lang).to_owned(),
                Some(words::ALSO_IN_THE_ACTIONS.get(lang).to_owned()),
                next,
            ),
        ));
        let head = row![label(words::THE_FOLDER.get(lang))]
            .align_y(Alignment::Center)
            .height(28);
        column![head, card_box(card.into())].spacing(8).into()
    }

    fn day_night_group(&self) -> El<'_> {
        let lang = self.lang;
        let inert = !self.writable();
        let controls = self.controls();
        let file = self.file();
        let changing = self.target_changing();
        let mut slots = row![].spacing(12);
        for (when, picture, name) in [
            (When::Day, changing.day(), words::THE_DAY),
            (When::Night, changing.night(), words::THE_NIGHT),
        ] {
            let open = self.wallpaper.slot_menu == Some(when);
            let focused = self.focused_stop(&Stop::Slot(when));
            let caption = words::slot(
                lang,
                words::Slot {
                    when: name.get(lang),
                    name: &model::name_of(&picture, lang),
                },
            );
            let content = column![
                container(self.picture_thumb(&picture, SLOT, 64.0))
                    .padding(2)
                    .style(move |_: &Theme| container::Style {
                        border: if open {
                            border(8.0, 2.0, ACCENT.get())
                        } else {
                            border(8.0, 1.0, crate::theme::ink(rgba(255, 255, 255, 0.12)))
                        },
                        ..container::Style::default()
                    }),
                row![
                    t(caption, 12.5, TEXT.get()).font(SEMIBOLD),
                    Space::new().width(Length::Fill),
                    icon(Icon::Chevron, 12.0, DIM.get()),
                ]
                .align_y(Alignment::Center),
            ]
            .spacing(6);
            let mut press = button(content)
                .width(Length::Fill)
                .padding(4)
                .style(move |_, _| button::Style {
                    border: if focused {
                        ring(9.0)
                    } else {
                        border(9.0, 0.0, Color::TRANSPARENT)
                    },
                    ..button::Style::default()
                });
            if !inert {
                press = press.on_press(Message::Wallpaper(Msg::SlotMenu(when)));
            }
            slots = slots.push(
                container(press)
                    .width(Length::FillPortion(1))
                    .id(format!("slot:{}", when.word())),
            );
        }
        let mut top = column![slots].spacing(8);
        if let Some(when) = self.wallpaper.slot_menu {
            top = top.push(self.slot_menu(when, &changing));
        }
        let mut card = column![
            container(top)
                .padding(12)
                .id(format!("row:{}", model::DAY_NIGHT))
        ];
        let (line, how) = match &file.place {
            Some(place) => (self.sun_line(place), words::WITH_THE_SUN),
            None => (
                words::NO_PLACE_THE_HOURS_BELOW.get(lang).to_owned(),
                words::AT_SET_HOURS,
            ),
        };
        let chip = container(t(how.get(lang), 13.0, TEXT.get()).font(SEMIBOLD))
            .padding([6, 12])
            .style(|_: &Theme| container::Style {
                background: Some(Background::Color(
                    crate::theme::Ink::Token(|t| t.control).get(),
                )),
                border: border(8.0, 1.0, RIM_12.get()),
                ..container::Style::default()
            });
        card = card.push(divided(
            true,
            own_row(
                model::SWITCH,
                words::SWITCH.get(lang).to_owned(),
                Some(line),
                container(chip)
                    .id("control:wallpaper.switch".to_owned())
                    .into(),
            ),
        ));
        if file.place.is_none() {
            for id in [model::DAY_STARTS, model::NIGHT_STARTS] {
                if let Some(setting) = model::settings().iter().find(|s| s.id == id) {
                    card = card.push(divided(true, self.row(setting, &controls)));
                }
            }
        }
        let head = row![label(words::DAY_AND_NIGHT_GROUP.get(lang))]
            .align_y(Alignment::Center)
            .height(28);
        column![head, card_box(card.into())].spacing(8).into()
    }

    /// Day's or night's list: the gallery's pictures, and one's own.
    fn slot_menu<'a>(&'a self, when: When, changing: &Changing) -> El<'a> {
        let lang = self.lang;
        let current = match when {
            When::Day => changing.day(),
            When::Night => changing.night(),
        };
        let mut list = column![].spacing(2);
        for tile in self.wallpaper_tiles().into_iter().filter(|t| !t.changes()) {
            let words: El<'a> = match tile.picture() {
                Some(picture) => {
                    let thumb = self.tile_thumb(&picture);
                    let on = same(&picture, &current);
                    row![
                        self.thumb(&thumb, 30.0, 19.0),
                        t(
                            model::name_of(&picture, lang),
                            13.0,
                            if on { MINT.get() } else { TEXT.get() }
                        )
                        .font(SEMIBOLD),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center)
                    .into()
                }
                None => t(
                    hocus_words::settings::app_wallpaper_page::CHOOSE_A_PICTURE.get(lang),
                    13.0,
                    MINT.get(),
                )
                .font(SEMIBOLD)
                .into(),
            };
            let id = format!("slot-option:{}", tile.id(&[]).trim_start_matches("tile:"));
            list = list.push(
                container(
                    button(words)
                        .width(Length::Fill)
                        .padding([4, 8])
                        .on_press(Message::Wallpaper(Msg::SlotPicture(when, tile)))
                        .style(|_, status| button::Style {
                            background: matches!(
                                status,
                                button::Status::Hovered | button::Status::Pressed
                            )
                            .then(|| Background::Color(HOVER.get())),
                            text_color: TEXT.get(),
                            border: border(6.0, 0.0, Color::TRANSPARENT),
                            ..button::Style::default()
                        }),
                )
                .id(id),
            );
        }
        container(list)
            .padding(6)
            .style(|_: &Theme| container::Style {
                background: Some(Background::Color(MENU.get())),
                border: border(10.0, 1.0, RIM_12.get()),
                ..container::Style::default()
            })
            .into()
    }

    /// "Lever 07:58 · coucher 17:12 à Saint-Étienne": today's, at the
    /// place, in the wall clock's time.
    fn sun_line(&self, model::WeatherPlace(place, lat, lon): &model::WeatherPlace) -> String {
        let lang = self.lang;
        match hocus_link::sun::on(jiff::Zoned::now().date(), *lat, *lon) {
            hocus_link::sun::SunDay::Rises { rise, set } => words::sunrise_and_sunset(
                lang,
                words::SunriseAndSunset {
                    rise: &clock_hhmm(rise),
                    set: &clock_hhmm(set),
                    place,
                },
            ),
            hocus_link::sun::SunDay::AlwaysUp => words::the_sun_does_not_set(lang, place),
            hocus_link::sun::SunDay::AlwaysDown => words::the_sun_does_not_rise(lang, place),
        }
    }

    /// What the groups show, for the log (tests).
    pub(super) fn changing_shown(&self) -> serde_json::Value {
        let file = self.file();
        let changing = self.target_changing();
        let folder = changing.folder().map(ToOwned::to_owned);
        let path = folder.as_deref().map(hocus_link::wallpaper::expand);
        let count = self
            .wallpaper
            .counted
            .as_ref()
            .filter(|(p, _, _)| Some(p) == path.as_ref())
            .map(|(_, n, _)| *n);
        serde_json::json!({
            "group": match self.target_picture() {
                Picture::Folder => "folder",
                Picture::DayNight => "day-night",
                _ => "",
            },
            "folder": folder,
            "count": count,
            "day": changing.day().word(),
            "night": changing.night().word(),
            "switch": file.place.as_ref().map(|p| self.sun_line(p)),
            "slot_menu": self.wallpaper.slot_menu.map(When::word),
            "now": self.wallpaper.now.iter().map(|(n, p)| serde_json::json!([n, p])).collect::<Vec<_>>(),
        })
    }
}
