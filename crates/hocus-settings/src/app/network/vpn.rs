//! The Réseau page's VPN group (settings.md "Réseau"): a row per
//! NetworkManager VPN connection (`vpn`, `wireguard`), its switch, its
//! "⋯" menu (renamed in place, joined by itself or not, deleted with its
//! undo), and "Ajouter un VPN": a WireGuard or OpenVPN file imported with
//! NetworkManager's own importers, off, its user name and password asked
//! first when an OpenVPN file wants them.

use hocus_words::settings::app_network_vpn as words;
use std::path::{Path, PathBuf};

use hocus_network::{
    Ask,
    model::{Vpn, VpnKind, VpnState},
    secret::Secret,
};
use iced::{
    Alignment, Background, Color, Length, Padding, Shadow, Task, Theme,
    widget::{Space, button, column, container, operation, pin, row, text, text_input},
};

use super::{
    Doing, Msg as PageMsg, Stop,
    sheet::Sheet,
    view::{GREEN, draw, spinner},
};
use crate::{
    app::{
        App, Message,
        style::{self, *},
        view::{El, card_box, divided, label, t},
    },
    lang::{Lang, Words},
};

/// The field a VPN is renamed in.
pub(super) const NAME_FIELD: &str = "network-vpn-name";

const SHIELD: &str = "M12 3 L19.5 6 V11.5 C19.5 16 16 19.5 12 21 C8 19.5 4.5 16 4.5 11.5 V6 Z";
const DOTS: &str = "M6 12 V12.1 M12 12 V12.1 M18 12 V12.1";
const FILE: &str = "M6 3.5 H14 L18.5 8 V20.5 H6 Z M14 3.5 V8 H18.5";
const PLUS: &str = "M12 5 V19 M5 12 H19";

#[derive(Debug, Clone)]
pub enum Msg {
    /// On or off, by UUID.
    Switch(String),
    /// Its "⋯" menu opened (or closed), and closed.
    Menu(String),
    CloseMenu,
    Rename(String),
    Name(String),
    SaveName,
    Autoconnect(String),
    Delete(String),
    /// "Importer un fichier…", and the chooser's answer (`true`: no
    /// portal answered).
    Import,
    Chosen(Option<PathBuf>, bool),
}

/// What a file is: WireGuard's `.conf` or OpenVPN's `.ovpn`, and whether
/// it asks a user name and password (`auth-user-pass` without a file).
fn kind_of(file: &Path) -> Option<(&'static str, bool)> {
    let extension = file.extension()?.to_str()?.to_lowercase();
    match extension.as_str() {
        "conf" => Some(("wireguard", false)),
        "ovpn" => {
            let text = std::fs::read_to_string(file).unwrap_or_default();
            let asks = text.lines().any(|line| {
                let mut words = line.split_whitespace();
                words.next() == Some("auth-user-pass") && words.next().is_none()
            });
            Some(("openvpn", asks))
        }
        _ => None,
    }
}

/// "WireGuard", "OpenVPN", or the plugin's word.
fn kind_name(kind: &VpnKind) -> String {
    match kind {
        VpnKind::WireGuard => "WireGuard".to_owned(),
        VpnKind::OpenVpn => "OpenVPN".to_owned(),
        VpnKind::Other(word) => word.clone(),
    }
}

/// Why a VPN failed, from NetworkManager's reason.
fn reason(code: u32) -> Words {
    match code {
        3 => words::THE_DEVICE_DISCONNECTED,
        4 => words::THE_VPN_SERVICE_STOPPED,
        5 => words::NO_ADDRESS_RECEIVED,
        6 => words::THE_SERVER_DOES_NOT_ANSWER,
        7 => words::THE_VPN_SERVICE_DID_NOT_START,
        8 => words::THE_VPN_SERVICE_DID_NOT_START_2,
        9 => words::SECRETS_ARE_MISSING,
        10 => words::USER_NAME_OR_PASSWORD_REFUSED,
        _ => words::UNKNOWN_REASON,
    }
}

/// `09:12`, in the user's time zone.
fn clock(at: u64) -> Option<String> {
    let at = jiff::Timestamp::from_second(i64::try_from(at).ok()?).ok()?;
    Some(
        at.to_zoned(jiff::tz::TimeZone::system())
            .strftime("%H:%M")
            .to_string(),
    )
}

/// A VPN's line, and its colour: "WireGuard · coupé", "Connecté depuis
/// 09:12 · WireGuard · 10.8.0.6", "Connexion…", or why it failed.
pub(super) fn vpn_line(vpn: &Vpn, lang: Lang) -> (String, Color) {
    let kind = kind_name(&vpn.kind);
    match &vpn.state {
        VpnState::On { since, address } => {
            let mut parts = vec![match since.and_then(clock) {
                Some(at) => words::connected_since(lang, &at),
                None => words::CONNECTED.get(lang).to_owned(),
            }];
            parts.push(kind);
            parts.extend(address.clone());
            (parts.join(" · "), GREEN.get())
        }
        VpnState::Connecting => (words::CONNECTING.get(lang).to_owned(), DIM.get()),
        VpnState::Off => match vpn.failed {
            Some(code) => (
                words::the_connection_failed(lang, reason(code).get(lang)),
                PEACH.get(),
            ),
            None => (format!("{kind} · {}", words::OFF.get(lang)), DIM.get()),
        },
    }
}

impl App {
    /// The VPNs, by name.
    pub(super) fn network_vpns(&self) -> Vec<Vpn> {
        let mut vpns = self
            .network
            .details
            .as_deref()
            .map(|d| d.vpns.clone())
            .unwrap_or_default();
        vpns.sort_by_key(|v| v.name.to_lowercase());
        vpns
    }

    pub(super) fn network_vpn_update(&mut self, msg: Msg) -> Task<Message> {
        let find = |app: &Self, uuid: &str| app.network_vpns().into_iter().find(|v| v.uuid == uuid);
        if !matches!(msg, Msg::Menu(_) | Msg::Name(_) | Msg::Chosen(..)) {
            self.network.vpn_menu = None;
        }
        match msg {
            Msg::Switch(uuid) => {
                if let Some(vpn) = find(self, &uuid) {
                    let ask = if vpn.state == VpnState::Off {
                        Ask::Activate { uuid }
                    } else {
                        Ask::Deactivate { uuid }
                    };
                    self.network.ask(ask);
                }
                Task::none()
            }
            Msg::Menu(uuid) => {
                self.network.vpn_menu = if self.network.vpn_menu.as_deref() == Some(&uuid) {
                    None
                } else {
                    Some(uuid)
                };
                Task::none()
            }
            Msg::CloseMenu => Task::none(),
            Msg::Rename(uuid) => {
                let Some(vpn) = find(self, &uuid) else {
                    return Task::none();
                };
                self.network.renaming = Some((uuid, vpn.name));
                Task::batch([
                    operation::focus(NAME_FIELD),
                    operation::select_all(NAME_FIELD),
                ])
            }
            Msg::Name(name) => {
                if let Some((_, typed)) = &mut self.network.renaming {
                    *typed = name;
                }
                Task::none()
            }
            Msg::SaveName => {
                if let Some((uuid, name)) = self.network.renaming.take()
                    && !name.trim().is_empty()
                {
                    self.network.ask(Ask::Rename {
                        uuid,
                        name: name.trim().to_owned(),
                    });
                }
                operation::focus("nothing")
            }
            Msg::Autoconnect(uuid) => {
                if let Some(vpn) = find(self, &uuid) {
                    self.network.ask(Ask::Autoconnect {
                        uuid,
                        on: !vpn.autoconnect,
                    });
                }
                Task::none()
            }
            Msg::Delete(uuid) => {
                self.network.forgetting.push((uuid.clone(), Doing::New));
                self.network.ask(Ask::Forget { uuid });
                Task::none()
            }
            Msg::Import => {
                self.network_choose_vpn_file();
                Task::none()
            }
            Msg::Chosen(None, true) => {
                let text = words::NO_FILE_CHOOSER_ANSWERS_THE_DESKTOP
                    .get(self.lang)
                    .to_owned();
                self.toast(text, super::super::ToastAction::None, super::super::TOAST)
            }
            Msg::Chosen(None, false) => Task::none(),
            Msg::Chosen(Some(file), _) => match kind_of(&file) {
                Some((kind, true)) => {
                    let name = file
                        .file_stem()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    self.network_open_sheet(Sheet::vpn(file, kind, name))
                }
                Some((kind, false)) => {
                    self.network.ask(Ask::Import {
                        file,
                        kind,
                        credentials: None,
                    });
                    Task::none()
                }
                None => {
                    let text = words::THIS_FILE_IS_NEITHER_WIREGUARD_S
                        .get(self.lang)
                        .to_owned();
                    self.toast(text, super::super::ToastAction::None, super::super::TOAST)
                }
            },
        }
    }

    /// The desktop portal's file chooser for a VPN's file, on a thread of
    /// its own.
    fn network_choose_vpn_file(&self) {
        let Some(to) = self.network.to.clone() else {
            return;
        };
        let lang = self.lang;
        let folder = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        let started = std::thread::Builder::new()
            .name("network: VPN file".to_owned())
            .spawn(move || {
                let chosen = crate::portal::open_file_of(
                    words::IMPORT_A_VPN.get(lang),
                    words::WIREGUARD_OR_OPENVPN.get(lang),
                    &[(0, "*.conf"), (0, "*.ovpn")],
                    &folder,
                );
                let (path, missing) = match chosen {
                    crate::portal::Chosen::Path(path) => (Some(path), false),
                    crate::portal::Chosen::Cancelled => (None, false),
                    crate::portal::Chosen::NoPortal => (None, true),
                };
                let _ =
                    to.unbounded_send(Message::Network(PageMsg::Vpn(Msg::Chosen(path, missing))));
            });
        if let Err(err) = started {
            tracing::warn!(%err, "no file chooser");
        }
    }

    /// The VPN group: a row each, then "Ajouter un VPN".
    pub(super) fn network_vpn_group(&self) -> El<'_> {
        let lang = self.lang;
        let vpns = self.network_vpns();
        let mut rows = column![];
        for (i, vpn) in vpns.iter().enumerate() {
            rows = rows.push(divided(i > 0, self.network_vpn_row(i, vpn)));
        }
        let add = super::view::page_row(
            super::view::tile(|c| draw(PLUS, 18.0, c), super::view::Tone::Plain, 1.0),
            super::view::name(words::ADD_A_VPN.get(lang).to_owned(), 1.0, None),
            vec![super::view::line(
                words::FROM_A_WIREGUARD_CONF_OR_OPENVPN.get(lang),
                DIM.get(),
            )],
            container(import_button(
                words::IMPORT_A_FILE.get(lang).to_owned(),
                self.network_focused(Stop::Import),
            ))
            .id("network:import".to_owned())
            .into(),
            None,
            0.0,
            false,
            false,
        );
        rows = rows.push(divided(!vpns.is_empty(), add));
        container(
            column![
                container(label(words::VPN.get(lang)))
                    .height(28)
                    .center_y(28),
                card_box(rows.into())
            ]
            .spacing(8),
        )
        .id("row:network.vpn".to_owned())
        .into()
    }

    fn network_vpn_row(&self, i: usize, vpn: &Vpn) -> El<'_> {
        let lang = self.lang;
        let (said, colour) = vpn_line(vpn, lang);
        let on = matches!(vpn.state, VpnState::On { .. });
        let mut lines: Vec<El<'_>> = Vec::new();
        if vpn.state == VpnState::Connecting {
            lines.push(
                row![
                    spinner(12.0, DIM.get(), !self.reduced_motion),
                    t(said, 12.5, colour)
                ]
                .spacing(6)
                .align_y(Alignment::Center)
                .into(),
            );
        } else {
            lines.push(super::view::line(said, colour));
        }
        let renaming = self
            .network
            .renaming
            .as_ref()
            .filter(|(uuid, _)| *uuid == vpn.uuid);
        let name: El<'_> = match renaming {
            Some((_, typed)) => container(
                text_input("", typed)
                    .id(NAME_FIELD)
                    .on_input(|s| Message::Network(PageMsg::Vpn(Msg::Name(s))))
                    .on_submit(Message::Network(PageMsg::Vpn(Msg::SaveName)))
                    .size(14.0)
                    .font(SEMIBOLD)
                    .padding([4, 8])
                    .style(style::search),
            )
            .width(260)
            .style(boxed(
                crate::theme::ink(rgba(255, 255, 255, 0.06)),
                7.0,
                Some((1.0, crate::theme::ink(rgba(108, 194, 135, 0.65)))),
            ))
            .id("vpn-name".to_owned())
            .into(),
            None => super::view::name(vpn.name.clone(), 1.0, None),
        };
        let dots_focused = self.network_focused(Stop::VpnMenu(i));
        let open = self.network.vpn_menu.as_deref() == Some(vpn.uuid.as_str());
        let dots = container(
            button(draw(DOTS, 16.0, DIM.get()))
                .width(30)
                .height(30)
                .padding(6)
                .on_press(Message::Network(PageMsg::Vpn(Msg::Menu(vpn.uuid.clone()))))
                .style(move |_, status| {
                    let hovered =
                        matches!(status, button::Status::Hovered | button::Status::Pressed);
                    button::Style {
                        background: Some(Background::Color(crate::theme::ink(rgba(
                            255,
                            255,
                            255,
                            if hovered || open { 0.08 } else { 0.04 },
                        )))),
                        text_color: TEXT.get(),
                        border: if dots_focused {
                            ring(8.0)
                        } else {
                            border(8.0, 1.0, crate::theme::ink(rgba(255, 255, 255, 0.10)))
                        },
                        ..button::Style::default()
                    }
                }),
        )
        .id(format!("vpn-menu:{}", vpn.name));
        let switch = container(self.network_switch(
            "network.vpn",
            vpn.state != VpnState::Off,
            Some(Message::Network(PageMsg::Vpn(Msg::Switch(
                vpn.uuid.clone(),
            )))),
            self.network_focused(Stop::VpnSwitch(i)),
        ))
        .id(format!("vpn-switch:{}", vpn.name));
        super::view::page_row(
            super::view::tile(
                |c| draw(SHIELD, 18.0, c),
                if on {
                    super::view::Tone::Green
                } else {
                    super::view::Tone::Plain
                },
                1.0,
            ),
            name,
            lines,
            row![dots, switch]
                .spacing(10)
                .align_y(Alignment::Center)
                .into(),
            Some(format!("vpn:{}", vpn.name)),
            0.0,
            false,
            false,
        )
    }

    /// The open "⋯" menu: "Renommer…", "Se connecter seul", a divider,
    /// "Supprimer"; its choices `option:N`.
    pub(in crate::app) fn network_vpn_menu(&self) -> Option<El<'_>> {
        let uuid = self.network.vpn_menu.as_ref()?;
        let vpn = self.network_vpns().into_iter().find(|v| v.uuid == *uuid)?;
        let at = *self.places.at.get(&format!("vpn-menu:{}", vpn.name))?;
        let lang = self.lang;
        let option = |i: usize, said: &'static str, ink: Color, tick: bool, msg: Msg| -> El<'_> {
            let check: El<'_> = if tick {
                draw("M5 12.5 L10 17 L19 7.5", 13.0, GREEN.get())
            } else {
                Space::new().width(13).into()
            };
            container(
                button(
                    row![
                        container(check).width(18).center_x(18),
                        text(said)
                            .size(13.5)
                            .font(MANROPE)
                            .color(ink)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(6)
                    .height(Length::Fill)
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .height(32)
                .padding(Padding {
                    top: 0.0,
                    right: 14.0,
                    bottom: 0.0,
                    left: 6.0,
                })
                .on_press(Message::Network(PageMsg::Vpn(msg)))
                .style(flat(
                    None,
                    crate::theme::ink(rgba(255, 255, 255, 0.07)),
                    7.0,
                    false,
                )),
            )
            .id(format!("option:{i}"))
            .into()
        };
        let list = column![
            option(
                0,
                words::RENAME.get(lang),
                TEXT.get(),
                false,
                Msg::Rename(vpn.uuid.clone())
            ),
            option(
                1,
                words::CONNECT_AUTOMATICALLY.get(lang),
                TEXT.get(),
                vpn.autoconnect,
                Msg::Autoconnect(vpn.uuid.clone())
            ),
            container(
                container(Space::new().width(Length::Fill).height(1)).style(boxed(
                    crate::theme::ink(rgba(255, 255, 255, 0.08)),
                    0.0,
                    None
                ))
            )
            .padding([4, 6]),
            option(
                2,
                words::DELETE.get(lang),
                PEACH.get(),
                false,
                Msg::Delete(vpn.uuid.clone())
            ),
        ];
        let width = 220.0;
        let height = 3.0 * 32.0 + 9.0 + 10.0;
        let panel = container(list)
            .width(width)
            .padding(5)
            .style(|_: &Theme| container::Style {
                background: Some(Background::Color(MENU.get())),
                border: border(10.0, 1.0, RIM_12.get()),
                shadow: Shadow {
                    color: rgba(0, 0, 0, 0.45),
                    ..style::shadow()
                },
                ..container::Style::default()
            })
            .id("vpn:menu".to_owned());
        let bottom = self
            .places
            .page
            .map_or(f32::MAX, |p| p.viewport.y + p.viewport.height);
        let y = if at.y + at.height + 6.0 + height > bottom - 8.0 {
            at.y - height - 6.0
        } else {
            at.y + at.height + 6.0
        };
        let x = (at.x + at.width - width).max(0.0);
        Some(pin(panel).x(x).y(y.max(0.0)).into())
    }

    /// The VPNs, for the log.
    pub(super) fn network_vpn_shown(&self) -> serde_json::Value {
        let lang = self.lang;
        serde_json::json!(
            self.network_vpns()
                .iter()
                .map(|v| serde_json::json!({
                    "name": v.name,
                    "line": vpn_line(v, lang).0,
                    "on": matches!(v.state, VpnState::On { .. }),
                    "connecting": v.state == VpnState::Connecting,
                    "autoconnect": v.autoconnect,
                }))
                .collect::<Vec<_>>()
        )
    }
}

/// "Importer un fichier…": the file's glyph and its words.
fn import_button<'a>(said: String, focused: bool) -> El<'a> {
    button(
        row![
            draw(FILE, 14.0, DIM.get()),
            t(said, 13.0, TEXT.get()).font(BOLD)
        ]
        .spacing(7)
        .height(Length::Fill)
        .align_y(Alignment::Center),
    )
    .height(32)
    .padding([0, 14])
    .on_press(Message::Network(PageMsg::Vpn(Msg::Import)))
    .style(move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(Background::Color(crate::theme::ink(rgba(
                255,
                255,
                255,
                if hovered { 0.09 } else { 0.06 },
            )))),
            text_color: TEXT.get(),
            border: if focused {
                ring(8.0)
            } else {
                border(8.0, 1.0, RIM_12.get())
            },
            ..button::Style::default()
        }
    })
    .into()
}

/// The credentials an OpenVPN file asks, from the sheet.
pub(super) fn credentials(user: &str, password: &str) -> (String, Secret) {
    let mut secret = Secret::longest(password.chars().count().max(1));
    secret.push(password);
    (user.trim().to_owned(), secret)
}
