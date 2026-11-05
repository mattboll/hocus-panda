//! What the Compte page draws (the approved mockup, variant A, its pandas
//! made larger): the avatar's row and its popover, the name, the login,
//! the account's type, the password's row and its sheet, the link to the
//! automatic locking, what comes later.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock, PoisonError},
};

use iced::{
    Alignment, Background, Color, Gradient, Length, Padding, Theme, gradient,
    widget::{
        Column, Space, button, column, container, image, mouse_area, pin, row, stack, svg, text,
        text_input,
    },
};

use super::{FIELDS, Msg, NAME_FIELD, Sheet, Stop};
use crate::{
    account as model,
    app::{
        App, Message,
        style::*,
        view::{El, card_box, divided, label, t},
    },
    lang::{Lang, Words, w},
    model::SectionId,
    theme::Ink,
};

const IMAGE: &str =
    "M3.5 5 H20.5 V19 H3.5 Z M3.5 15.5 L8.5 10.5 L13 15 L15.5 12.5 L20.5 17.5 M15.5 9.5 V9.6";
const KEY: &str = "M8 15.5 A3.5 3.5 0 1 0 8 8.5 A3.5 3.5 0 1 0 8 15.5 Z M11.5 12 H20.5 M17.5 12 V15 M20.5 12 V14.5";
const PENCIL: &str = "M4 20 L4.5 16.5 L15.5 5.5 L18.5 8.5 L7.5 19.5 Z M13.5 7.5 L16.5 10.5";
const EYE: &str = "M2.5 12 C5 7 8.5 5 12 5 C15.5 5 19 7 21.5 12 C19 17 15.5 19 12 19 C8.5 19 5 17 2.5 12 Z M12 15 A3 3 0 1 0 12 9 A3 3 0 1 0 12 15 Z";
const GLOBE: &str = "M12 21 A9 9 0 1 0 12 3 A9 9 0 1 0 12 21 Z M3 12 H21 M12 3 C8.5 6.5 8.5 17.5 12 21 C15.5 17.5 15.5 6.5 12 3";

/// The avatar on the page, and each panda in the popover (logical px).
const AVATAR: f32 = 88.0;
const PANDA: f32 = 76.0;
const GAP: f32 = 10.0;
/// The popover: four pandas wide, each with room for its ring.
const POPOVER: f32 = 4.0 * (PANDA + 8.0) + 3.0 * GAP + 2.0 * 16.0;

/// The orange of a field refused, and the green of two that match.
const REFUSED: Ink = Ink::Dark(rgb(0xF2945B));

impl App {
    pub(in crate::app) fn account_page<'a>(&'a self, mut page: Column<'a, Message>) -> El<'a> {
        let lang = self.lang;
        page = page.push(group(
            w("PROFIL", "PROFILE").get(lang),
            column![
                self.account_avatar_row(),
                divided(true, self.account_name_row()),
                divided(true, self.account_login_row()),
                divided(true, self.account_type_row()),
            ]
            .into(),
        ));
        page = page.push(group(
            w("SÉCURITÉ", "SECURITY").get(lang),
            column![
                self.account_password_row(),
                divided(true, self.account_lock_row()),
            ]
            .into(),
        ));
        page = page.push(group(w("PLUS TARD", "LATER").get(lang), later_row(lang)));
        container(page)
            .id(format!("page:{}", SectionId::Account.section().slug))
            .into()
    }

    /// The account's avatar: its picture, else its initial on the
    /// accent's gradient, `side` wide.
    pub(super) fn account_avatar(&self, side: f32) -> El<'_> {
        if let Some((_, handle)) = &self.account.picture {
            return image(handle.clone()).width(side).height(side).into();
        }
        let initial: String = self
            .account_said_name()
            .chars()
            .next()
            .map(|c| c.to_uppercase().collect())
            .unwrap_or_default();
        container(text(initial).size(side * 0.42).font(BOLD).color(DEEP))
            .width(side)
            .height(side)
            .center(side)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Gradient(Gradient::Linear(
                    gradient::Linear::new(std::f32::consts::FRAC_PI_4 * 3.0)
                        .add_stop(0.0, crate::theme::Ink::Accent(1.0).get())
                        .add_stop(1.0, crate::theme::ink(rgb(0x5B8DEF))),
                ))),
                border: border(side / 2.0, 0.0, Color::TRANSPARENT),
                ..container::Style::default()
            })
            .into()
    }

    fn account_avatar_row(&self) -> El<'_> {
        let lang = self.lang;
        let there = self.account.account().is_some();
        let open = self.account.popover.is_some();
        let focused = self.account_focused(Stop::Avatar);
        let mut change = button(
            row![
                stroke(IMAGE, DIM.get(), 14.0),
                t(
                    w("Changer l’avatar…", "Change avatar…").get(lang),
                    13.0,
                    TEXT.get()
                )
                .font(SEMIBOLD),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
        )
        .height(30)
        .padding([0, 12])
        .style(move |_: &Theme, status| secondary(status, open, focused, !there));
        if there {
            change = change.on_press(Message::Account(Msg::Popover(!open)));
        }
        self.account_row(
            model::AVATAR,
            row![
                container(self.account_avatar(AVATAR)).id("account-avatar".to_owned()),
                column![
                    text(self.account_said_name())
                        .size(18)
                        .font(BOLD)
                        .color(TEXT),
                    t(
                        w(
                            "Sur l’écran de verrouillage, à l’ouverture de session et dans la barre.",
                            "On the lock screen, at login and in the bar.",
                        )
                        .get(lang),
                        12.5,
                        DIM.get(),
                    ),
                ]
                .spacing(3)
                .width(Length::Fill),
                container(change).id(format!("control:{}", model::AVATAR)),
            ]
            .spacing(16)
            .align_y(Alignment::Center)
            .into(),
            Padding {
                top: 14.0,
                right: 14.0,
                bottom: 14.0,
                left: 16.0,
            },
        )
    }

    fn account_name_row(&self) -> El<'_> {
        let lang = self.lang;
        let there = self.account.account().is_some();
        let value = self.account_name();
        let field: El<'_> = if there {
            text_input("", &value)
                .id(NAME_FIELD)
                .on_input(|s| Message::Account(Msg::Name(s)))
                .on_submit(Message::Account(Msg::SaveName))
                .size(13.5)
                .font(MANROPE)
                .padding(Padding {
                    top: 7.0,
                    right: 30.0,
                    bottom: 7.0,
                    left: 10.0,
                })
                .width(240)
                .style(|_: &Theme, status| field_style(status, false))
                .into()
        } else {
            container(t(value, 13.5, fade(TEXT.get(), INERT)))
                .width(240)
                .padding([7, 10])
                .into()
        };
        let field = stack![
            field,
            container(stroke(PENCIL, FAINT.get(), 13.0))
                .width(240)
                .height(Length::Fill)
                .align_right(240)
                .center_y(Length::Fill)
                .padding(Padding::default().right(10)),
        ];
        self.simple_row(
            model::NAME,
            w("Nom affiché", "Display name"),
            w(
                "Ton nom complet, tel que le bureau te salue.",
                "Your full name, as the desktop greets you.",
            )
            .get(lang)
            .to_owned(),
            DIM.get(),
            container(field).id(format!("box:{NAME_FIELD}")).into(),
        )
    }

    fn account_login_row(&self) -> El<'_> {
        let login = self
            .account
            .account()
            .map(|a| a.login.clone())
            .unwrap_or_default();
        self.simple_row(
            model::LOGIN,
            w("Identifiant", "Login name"),
            w(
                "Le nom de connexion ; il ne se change pas ici.",
                "The login name; it is not changed here.",
            )
            .get(self.lang)
            .to_owned(),
            DIM.get(),
            text(login).size(12.5).font(MONO).color(SOFT).into(),
        )
    }

    fn account_type_row(&self) -> El<'_> {
        let lang = self.lang;
        let admin = self.account.account().is_some_and(|a| a.admin);
        let (value, line) = if admin {
            (
                w("Administrateur", "Administrator"),
                w(
                    "Peut installer des logiciels et changer les réglages du système.",
                    "Can install software and change system settings.",
                ),
            )
        } else {
            (
                w("Standard", "Standard"),
                w(
                    "Les réglages du système demandent le mot de passe d’un administrateur.",
                    "System settings ask for an administrator’s password.",
                ),
            )
        };
        let value = if self.account.account().is_some() {
            value.get(lang)
        } else {
            ""
        };
        self.simple_row(
            model::KIND,
            w("Type de compte", "Account type"),
            line.get(lang).to_owned(),
            DIM.get(),
            t(value, 13.5, SOFT.get()).into(),
        )
    }

    fn account_password_row(&self) -> El<'_> {
        let lang = self.lang;
        let can = self.account.passwd;
        let focused = self.account_focused(Stop::Password);
        let mut change = button(
            row![
                stroke(KEY, DIM.get(), 14.0),
                t(w("Changer…", "Change…").get(lang), 13.0, TEXT.get()).font(SEMIBOLD),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
        )
        .height(30)
        .padding([0, 12])
        .style(move |_: &Theme, status| secondary(status, false, focused, !can));
        if can {
            change = change.on_press(Message::Account(Msg::Sheet(true)));
        }
        let line = if can {
            w(
                "Il ouvre la session, la déverrouille et confirme les actions d’administrateur.",
                "It opens the session, unlocks it and confirms administrator actions.",
            )
        } else {
            w(
                "Le mot de passe se change avec les outils du système.",
                "The password is changed with the system’s tools.",
            )
        };
        self.simple_row(
            model::PASSWORD,
            w("Mot de passe", "Password"),
            line.get(lang).to_owned(),
            DIM.get(),
            container(change)
                .id(format!("control:{}", model::PASSWORD))
                .into(),
        )
    }

    fn account_lock_row(&self) -> El<'_> {
        let lang = self.lang;
        let focused = self.account_focused(Stop::Lock);
        let said = format!("{} ›", SectionId::Power.name(lang));
        let link = button(t(said, 13.5, MINT.get()).font(SEMIBOLD))
            .padding([4, 6])
            .on_press(Message::Account(Msg::Lock))
            .style(flat(
                None,
                crate::theme::ink(rgba(255, 255, 255, 0.05)),
                7.0,
                focused,
            ));
        self.simple_row(
            model::LOCK,
            w("Verrouillage automatique", "Automatic locking"),
            w(
                "Quand la session se verrouille d’elle-même.",
                "When the session locks by itself.",
            )
            .get(lang)
            .to_owned(),
            DIM.get(),
            container(link).id(format!("link:{}", model::LOCK)).into(),
        )
    }

    /// A row: its name and line, its control on the right.
    fn simple_row<'a>(
        &'a self,
        id: &'static str,
        name: Words,
        line: String,
        ink: Color,
        control: El<'a>,
    ) -> El<'a> {
        self.account_row(
            id,
            row![
                column![
                    text(name.get(self.lang))
                        .size(14)
                        .font(SEMIBOLD)
                        .color(TEXT),
                    t(line, 12.5, ink),
                ]
                .spacing(2)
                .width(Length::Fill),
                row![Space::new().height(36), control].align_y(Alignment::Center),
            ]
            .spacing(16)
            .align_y(Alignment::Center)
            .into(),
            Padding {
                top: 9.0,
                right: 14.0,
                bottom: 9.0,
                left: 16.0,
            },
        )
    }

    /// A row's box, lit when found.
    fn account_row<'a>(&'a self, id: &'static str, body: El<'a>, padding: Padding) -> El<'a> {
        let lit = self.lit(id);
        container(body)
            .padding(padding)
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

    /// The avatar's popover, under its button, while it is open.
    pub(in crate::app) fn account_popover(&self) -> Option<El<'_>> {
        let at_key = self.account.popover?;
        let lang = self.lang;
        let at = *self.places.at.get(&format!("control:{}", model::AVATAR))?;
        let current = self.account_panda();
        let mut grid = column![].spacing(GAP);
        for four in (0..self.account.pandas.len()).collect::<Vec<_>>().chunks(4) {
            let mut line = row![].spacing(GAP);
            for &i in four {
                let chosen = current == Some(i);
                let keyboard = at_key == i;
                let ring = if chosen {
                    Some((2.0, crate::theme::Ink::Accent(1.0).get()))
                } else if keyboard {
                    Some((2.0, crate::theme::ink(rgba(255, 255, 255, 0.55))))
                } else {
                    None
                };
                line = line.push(
                    container(
                        button(
                            image(self.account.pandas[i].clone())
                                .width(PANDA)
                                .height(PANDA),
                        )
                        .padding(2)
                        .on_press(Message::Account(Msg::Panda(i)))
                        .style(move |_: &Theme, status| {
                            let hovered =
                                matches!(status, button::Status::Hovered | button::Status::Pressed);
                            button::Style {
                                background: None,
                                text_color: TEXT.get(),
                                border: match ring {
                                    Some((width, color)) => border(PANDA / 2.0 + 2.0, width, color),
                                    None if hovered => border(
                                        PANDA / 2.0 + 2.0,
                                        2.0,
                                        crate::theme::ink(rgba(255, 255, 255, 0.25)),
                                    ),
                                    None => border(PANDA / 2.0 + 2.0, 0.0, Color::TRANSPARENT),
                                },
                                ..button::Style::default()
                            }
                        }),
                    )
                    .padding(2)
                    .id(format!("panda:{i}")),
                );
            }
            grid = grid.push(line);
        }
        let removable = self.account.picture.is_some();
        let mut remove = button(
            t(
                w("Retirer", "Remove").get(lang),
                13.0,
                fade(TEXT.get(), if removable { 1.0 } else { INERT }),
            )
            .font(SEMIBOLD),
        )
        .height(30)
        .padding([0, 12])
        .style(flat(None, HOVER.get(), 8.0, false));
        if removable {
            remove = remove.on_press(Message::Account(Msg::Remove));
        }
        let choose = button(
            row![
                stroke(IMAGE, DIM.get(), 14.0),
                t(
                    w("Choisir une image…", "Choose a picture…").get(lang),
                    13.0,
                    TEXT.get()
                )
                .font(SEMIBOLD),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
        )
        .height(30)
        .padding([0, 12])
        .on_press(Message::Account(Msg::Choose))
        .style(|_: &Theme, status| secondary(status, false, false, false));
        let panel = container(
            column![
                label(w("LES PANDAS", "THE PANDAS").get(lang)),
                grid,
                container(Space::new().width(Length::Fill).height(1)).style(boxed(
                    LINE.get(),
                    0.0,
                    None
                )),
                row![
                    container(choose).id("avatar:choose".to_owned()),
                    container(remove).id("avatar:remove".to_owned()),
                ]
                .spacing(8),
                t(
                    w(
                        "Une image est recadrée en carré au centre ; « Retirer » remet l’initiale.",
                        "A picture is cropped to a centred square; “Remove” brings the initial back.",
                    )
                    .get(lang),
                    12.0,
                    FAINT.get(),
                )
                .line_height(1.45),
            ]
            .spacing(12),
        )
        .width(POPOVER)
        .padding(16)
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(MENU.get())),
            border: border(12.0, 1.0, RIM_12.get()),
            shadow: shadow(),
            ..container::Style::default()
        })
        .id("avatar-popover".to_owned());
        let x = (at.x + at.width - POPOVER).max(0.0);
        let y = at.y + at.height + 6.0;
        Some(
            stack![
                mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                    .on_press(Message::Account(Msg::Popover(false))),
                pin(panel).x(x).y(y),
            ]
            .into(),
        )
    }

    /// The password's sheet, over the window while it is open.
    pub(in crate::app) fn account_sheet(&self) -> Option<El<'_>> {
        let sheet = self.account.sheet.as_ref()?;
        let lang = self.lang;
        let names = [
            w("Mot de passe actuel", "Current password"),
            w("Nouveau mot de passe", "New password"),
            w("Encore une fois", "Once more"),
        ];
        let mut fields = column![].spacing(12);
        for (i, name) in names.iter().enumerate() {
            let wrong = (i == 0 && sheet.wrong_current) || (i == 1 && sheet.refused.is_some());
            let mut block = column![
                t(name.get(lang), 12.5, SOFT.get()).font(SEMIBOLD),
                password_field(sheet, i, wrong),
            ]
            .spacing(6);
            let under: Option<(String, Color)> = match i {
                0 if sheet.wrong_current => Some((
                    w(
                        "Ce n’est pas le mot de passe actuel. Vérifie-le et réessaie.",
                        "That is not the current password. Check it and try again.",
                    )
                    .get(lang)
                    .to_owned(),
                    REFUSED.get(),
                )),
                1 => sheet.refused.clone().map(|why| (why, REFUSED.get())),
                2 => sheet.matching().map(|same| {
                    if same {
                        (
                            w("Les deux sont identiques.", "Both match.")
                                .get(lang)
                                .to_owned(),
                            MINT.get(),
                        )
                    } else {
                        (
                            w("Les deux ne sont pas identiques.", "They do not match.")
                                .get(lang)
                                .to_owned(),
                            PEACH.get(),
                        )
                    }
                }),
                _ => None,
            };
            if let Some((said, ink)) = under {
                block =
                    block.push(container(t(said, 12.0, ink)).id(format!("under:{}", FIELDS[i])));
            }
            fields = fields.push(block);
        }
        let ready = sheet.ready();
        let mut change = button(
            text(if sheet.running {
                w("Changement…", "Changing…").get(lang)
            } else {
                w("Changer", "Change").get(lang)
            })
            .size(13)
            .font(BOLD)
            .color(if ready || sheet.running {
                DEEP.get()
            } else {
                DIM.get()
            }),
        )
        .height(32)
        .padding([7, 16])
        .style(move |_: &Theme, status| {
            let fill = if ready {
                ACCENT.get()
            } else if sheet.running {
                fade(ACCENT.get(), 0.6)
            } else {
                crate::theme::ink(rgba(255, 255, 255, 0.06))
            };
            solid(fill, TEXT.get(), status)
        });
        if ready {
            change = change.on_press(Message::Account(Msg::Change));
        }
        let mut cancel =
            button(t(w("Annuler", "Cancel").get(lang), 13.0, TEXT.get()).font(SEMIBOLD))
                .height(32)
                .padding([7, 14])
                .style(|_: &Theme, status| {
                    solid(
                        crate::theme::ink(rgba(255, 255, 255, 0.08)),
                        TEXT.get(),
                        status,
                    )
                });
        if !sheet.running {
            cancel = cancel.on_press(Message::Account(Msg::Sheet(false)));
        }
        let mut body = column![
            row![
                stroke(KEY, TEXT.get(), 18.0),
                text(w("Changer le mot de passe", "Change the password").get(lang))
                    .size(17)
                    .font(BOLD)
                    .color(TEXT),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            t(
                w(
                    "Celui qui ouvre ta session, la déverrouille et confirme les actions d’administrateur.",
                    "The one that opens your session, unlocks it and confirms administrator actions.",
                )
                .get(lang),
                13.0,
                SOFT.get(),
            )
            .line_height(1.45),
            fields,
            t(
                w(
                    "Le système peut refuser un mot de passe trop court ou trop simple : sa raison s’affiche ici.",
                    "The system may refuse a password that is too short or too simple: its reason shows here.",
                )
                .get(lang),
                12.0,
                FAINT.get(),
            )
            .line_height(1.45),
        ]
        .spacing(14);
        if let Some(failed) = &sheet.failed {
            body = body.push(
                container(t(failed.clone(), 12.5, REFUSED.get())).id("password-failed".to_owned()),
            );
        }
        body = body.push(
            row![
                Space::new().width(Length::Fill),
                container(cancel).id("password:cancel".to_owned()),
                container(change).id("password:change".to_owned()),
            ]
            .spacing(8),
        );
        let panel = container(body)
            .width(420)
            .padding(20)
            .style(|_: &Theme| container::Style {
                background: Some(Background::Color(MENU.get())),
                border: border(14.0, 1.0, RIM_12.get()),
                shadow: shadow(),
                ..container::Style::default()
            })
            .id("password-sheet".to_owned());
        Some(
            stack![
                mouse_area(
                    container(Space::new().width(Length::Fill).height(Length::Fill)).style(boxed(
                        crate::theme::ink(rgba(0, 0, 0, 0.58)),
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

    /// What the page shows, for the log: never a password, nor its length.
    pub(in crate::app) fn account_shown(&self) -> serde_json::Value {
        let account = self.account.account();
        let sheet = self.account.sheet.as_ref().map(|s| {
            serde_json::json!({
                "filled": s.typed.iter().map(|t| !t.is_empty()).collect::<Vec<_>>(),
                "matching": s.matching(),
                "ready": s.ready(),
                "running": s.running,
                "wrong_current": s.wrong_current,
                "refused": s.refused,
                "failed": s.failed,
                "shown": s.shown,
            })
        });
        serde_json::json!({
            "heard": self.account.heard.is_some(),
            "service": account.is_some(),
            "name": account.map(|a| a.real_name.clone()),
            "said_name": self.account_said_name(),
            "draft": self.account.draft,
            "login": account.map(|a| a.login.clone()),
            "admin": account.map(|a| a.admin),
            "picture": self.account.picture.is_some(),
            "panda": self.account_panda(),
            "pandas": self.account.pandas.len(),
            "popover": self.account.popover,
            "choosing": self.account.choosing,
            "passwd": self.account.passwd,
            "sheet": sheet,
        })
    }
}

/// A field of the sheet: dots, or the text while its eye is on.
fn password_field(sheet: &Sheet, i: usize, wrong: bool) -> El<'_> {
    let field = text_input("", sheet.typed[i].as_str())
        .id(FIELDS[i])
        .secure(!sheet.shown[i])
        .on_input_maybe((!sheet.running).then_some(move |s| Message::Account(Msg::Typed(i, s))))
        .on_submit(Message::Account(Msg::Next(i)))
        .size(13.5)
        .font(MONO)
        .padding(Padding {
            top: 8.0,
            right: 38.0,
            bottom: 8.0,
            left: 10.0,
        })
        .width(Length::Fill)
        .style(move |_: &Theme, status| field_style(status, wrong));
    let eye = button(stroke(
        EYE,
        if sheet.shown[i] {
            TEXT.get()
        } else {
            FAINT.get()
        },
        15.0,
    ))
    .padding(6)
    .on_press(Message::Account(Msg::Show(i)))
    .style(flat(None, HOVER.get(), 6.0, false));
    container(stack![
        field,
        container(container(eye).id(format!("eye:{}", FIELDS[i])))
            .width(Length::Fill)
            .height(Length::Fill)
            .align_right(Length::Fill)
            .center_y(Length::Fill)
            .padding(Padding::default().right(4)),
    ])
    .id(format!("box:{}", FIELDS[i]))
    .into()
}

fn field_style(status: text_input::Status, wrong: bool) -> text_input::Style {
    let focused = matches!(status, text_input::Status::Focused { .. });
    text_input::Style {
        background: Background::Color(crate::theme::ink(rgba(255, 255, 255, 0.05))),
        border: border(
            8.0,
            1.0,
            if wrong {
                REFUSED.get()
            } else if focused {
                ACCENT.get()
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

/// A secondary button: its rim, lighter under the pointer or while open.
fn secondary(status: button::Status, open: bool, focused: bool, inert: bool) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    let alpha = if inert { INERT } else { 1.0 };
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

/// A group: its label over its card.
fn group<'a>(name: &'static str, body: El<'a>) -> El<'a> {
    column![
        container(label(name)).height(28).center_y(28),
        card_box(body)
    ]
    .spacing(8)
    .into()
}

/// "Comptes en ligne": later, inert.
fn later_row<'a>(lang: Lang) -> El<'a> {
    let chip = container(
        text(w("Plus tard", "Later").get(lang))
            .size(10.5)
            .font(BOLD)
            .color(rgb(0xF5B44A)),
    )
    .padding([1, 7])
    .style(boxed(
        crate::theme::ink(rgba(245, 180, 74, 0.12)),
        8.0,
        Some((1.0, crate::theme::ink(rgba(245, 180, 74, 0.25)))),
    ));
    container(
        row![
            container(stroke(GLOBE, fade(DIM.get(), INERT), 16.0))
                .width(32)
                .height(32)
                .center(32)
                .style(boxed(crate::theme::ink(rgba(255, 255, 255, 0.04)), 16.0, None)),
            column![
                row![
                    text(w("Comptes en ligne", "Online accounts").get(lang))
                        .size(14)
                        .font(SEMIBOLD)
                        .color(fade(TEXT.get(), INERT)),
                    chip,
                ]
                .spacing(8)
                .align_y(Alignment::Center),
                t(
                    w(
                        "Google, Nextcloud… partagés par l’agenda et les fichiers. Les agendas se branchent déjà dans « Agenda et tâches ».",
                        "Google, Nextcloud… shared by the calendar and the files. Calendars already plug in under “Agenda and tasks”.",
                    )
                    .get(lang),
                    12.5,
                    fade(DIM.get(), INERT),
                ),
            ]
            .spacing(2)
            .width(Length::Fill),
        ]
        .spacing(14)
        .align_y(Alignment::Center),
    )
    .padding([12, 16])
    .into()
}

/// A stroke icon of the mockup's, `size` wide, drawn once per colour.
fn stroke<'a>(path: &'static str, color: Color, size: f32) -> El<'a> {
    type Drawn = HashMap<(&'static str, [u8; 4]), svg::Handle>;
    static CACHE: OnceLock<Mutex<Drawn>> = OnceLock::new();
    let rgba = color.into_rgba8();
    let handle = CACHE
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry((path, rgba))
        .or_insert_with(|| {
            let [r, g, b, a] = rgba;
            svg::Handle::from_memory(
                format!(
                    "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" width=\"24\" \
                     height=\"24\"><path d=\"{path}\" fill=\"none\" stroke=\"rgb({r},{g},{b})\" \
                     stroke-opacity=\"{}\" stroke-width=\"1.8\" stroke-linecap=\"round\" \
                     stroke-linejoin=\"round\"/></svg>",
                    f32::from(a) / 255.0
                )
                .into_bytes(),
            )
        })
        .clone();
    svg(handle).width(size).height(size).into()
}
