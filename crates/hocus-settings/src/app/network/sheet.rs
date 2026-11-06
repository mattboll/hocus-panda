//! The sheet over the Réseau page (settings.md "Réseau", states 2): a
//! network joined with what it asks. A WPA or WPA3 network's password; a
//! WEP key or passphrase; an 802.1X network's method, inner
//! authentication, identity, password or certificates, authority and
//! server domain; a hidden network's name and security first. What is
//! typed goes to NetworkManager once (`Ask::Join`), saved with the
//! connection; the sheet forgets it as it closes.

use std::path::PathBuf;

use hocus_network::{
    Ask, Ca, Eap, Join,
    model::{Failure, Security},
    secret::{self, Secret},
};
use iced::{
    Alignment, Background, Color, Gradient, Length, Padding, Shadow, Task, Theme, Vector, gradient,
    widget::{Space, button, column, container, mouse_area, operation, pin, row, text, text_input},
};

use super::{
    Msg as PageMsg, Stop,
    view::{AMBER, EYE, EYE_SHUT, GREEN, draw, secondary, spinner, wifi_glyph},
};
use crate::{
    app::{
        App, Focus, Message,
        style::{self, *},
        view::{El, t},
    },
    lang::{Lang, Words, w},
};

/// A text field of the sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Name,
    Password,
    Identity,
    KeyPassword,
    Domain,
}

impl Field {
    /// Its widget's id, for the keyboard.
    pub(super) fn id(self) -> &'static str {
        match self {
            Self::Name => "network-sheet-name",
            Self::Password => "network-sheet-password",
            Self::Identity => "network-sheet-identity",
            Self::KeyPassword => "network-sheet-key-password",
            Self::Domain => "network-sheet-domain",
        }
    }

    /// Whether it is shown as dots.
    fn secret(self) -> bool {
        matches!(self, Self::Password | Self::KeyPassword)
    }

    fn word(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Password => "password",
            Self::Identity => "identity",
            Self::KeyPassword => "key-password",
            Self::Domain => "domain",
        }
    }
}

/// 802.1X's methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Peap,
    Ttls,
    Tls,
}

impl Method {
    const ALL: [Self; 3] = [Self::Peap, Self::Ttls, Self::Tls];

    fn word(self) -> &'static str {
        match self {
            Self::Peap => "peap",
            Self::Ttls => "ttls",
            Self::Tls => "tls",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Peap => "PEAP",
            Self::Ttls => "TTLS",
            Self::Tls => "TLS",
        }
    }

    /// Its inner authentications, the first the default.
    fn inners(self) -> &'static [Inner] {
        match self {
            Self::Peap => &[Inner::Mschapv2, Inner::Gtc],
            Self::Ttls => &[Inner::Pap, Inner::Mschapv2, Inner::Mschap, Inner::Chap],
            Self::Tls => &[],
        }
    }
}

/// An inner authentication (`phase2-auth`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inner {
    Mschapv2,
    Gtc,
    Pap,
    Mschap,
    Chap,
}

impl Inner {
    fn word(self) -> &'static str {
        match self {
            Self::Mschapv2 => "mschapv2",
            Self::Gtc => "gtc",
            Self::Pap => "pap",
            Self::Mschap => "mschap",
            Self::Chap => "chap",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Mschapv2 => "MSCHAPv2",
            Self::Gtc => "GTC",
            Self::Pap => "PAP",
            Self::Mschap => "MSCHAP",
            Self::Chap => "CHAP",
        }
    }
}

/// How the server's certificate is checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Authority {
    System,
    File,
    Unchecked,
}

impl Authority {
    const ALL: [Self; 3] = [Self::System, Self::File, Self::Unchecked];

    fn label(self) -> Words {
        match self {
            Self::System => w("Ceux du système", "The system's"),
            Self::File => w("Choisir un fichier…", "Choose a file…"),
            Self::Unchecked => w("Ne pas vérifier", "Do not check"),
        }
    }
}

/// A menu of the sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuFor {
    Security,
    Method,
    Inner,
    Authority,
}

impl MenuFor {
    fn word(self) -> &'static str {
        match self {
            Self::Security => "security",
            Self::Method => "method",
            Self::Inner => "inner",
            Self::Authority => "authority",
        }
    }
}

/// A file the sheet asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileFor {
    Authority,
    Certificate,
    Key,
}

/// The sheet's own messages.
#[derive(Debug, Clone)]
pub enum Msg {
    Input(Field, String),
    Security(Security),
    Method(Method),
    Inner(Inner),
    Authority(Authority),
    /// A file to choose, in the desktop portal's chooser.
    File(FileFor),
    /// The chooser's answer: `None`, cancelled or no portal (`true`).
    Chosen(FileFor, Option<PathBuf>, bool),
    Eye,
    Connect,
    Cancel,
    /// A menu opened (or closed), and closed.
    Menu(MenuFor),
    CloseMenu,
}

/// The sheet.
#[derive(Debug)]
pub(in crate::app) struct Sheet {
    /// A hidden network: its name typed, its security chosen.
    pub hidden: bool,
    pub ssid: Vec<u8>,
    pub name: String,
    pub typed_name: String,
    pub security: Security,
    pub password: String,
    pub shown: bool,
    pub method: Method,
    pub inner: Inner,
    pub identity: String,
    pub authority: Authority,
    pub authority_file: Option<PathBuf>,
    pub domain: String,
    pub certificate: Option<PathBuf>,
    pub key: Option<PathBuf>,
    pub key_password: String,
    /// Sent: the attempts before it, by their number, are not its.
    pub sent: Option<u64>,
    pub failed: Option<Failure>,
    /// The menu open.
    pub menu: Option<MenuFor>,
    /// An OpenVPN file's user name and password, asked before it is
    /// imported: the file, its kind, its name.
    pub vpn: Option<(PathBuf, &'static str, String)>,
}

impl Sheet {
    fn new(hidden: bool, ssid: Vec<u8>, name: String, security: Security) -> Self {
        Self {
            hidden,
            ssid,
            name,
            typed_name: String::new(),
            security,
            password: String::new(),
            shown: false,
            method: Method::Peap,
            inner: Inner::Mschapv2,
            identity: String::new(),
            authority: Authority::System,
            authority_file: None,
            domain: String::new(),
            certificate: None,
            key: None,
            key_password: String::new(),
            sent: None,
            failed: None,
            menu: None,
            vpn: None,
        }
    }

    /// A VPN's file's user name and password.
    pub(super) fn vpn(file: PathBuf, kind: &'static str, name: String) -> Self {
        let mut sheet = Self::new(false, Vec::new(), name.clone(), Security::Enterprise);
        sheet.vpn = Some((file, kind, name));
        sheet
    }

    /// A network around's.
    pub(super) fn known(ssid: Vec<u8>, name: String, security: Security) -> Self {
        Self::new(false, ssid, name, security)
    }

    /// A hidden network's, WPA/WPA2 personal to start with.
    pub(super) fn hidden() -> Self {
        Self::new(
            true,
            Vec::new(),
            String::new(),
            Security::Psk { sae: false },
        )
    }

    /// Its text fields, in order.
    pub(super) fn fields(&self) -> Vec<Field> {
        if self.vpn.is_some() {
            return vec![Field::Identity, Field::Password];
        }
        let mut fields = Vec::new();
        if self.hidden {
            fields.push(Field::Name);
        }
        match self.security {
            Security::Psk { .. } | Security::Wep => fields.push(Field::Password),
            Security::Enterprise => {
                fields.push(Field::Identity);
                fields.push(if self.method == Method::Tls {
                    Field::KeyPassword
                } else {
                    Field::Password
                });
                fields.push(Field::Domain);
            }
            Security::Open => {}
        }
        fields
    }

    pub(super) fn text(&self, field: Field) -> &str {
        match field {
            Field::Name => &self.typed_name,
            Field::Password => &self.password,
            Field::Identity => &self.identity,
            Field::KeyPassword => &self.key_password,
            Field::Domain => &self.domain,
        }
    }

    pub(super) fn text_mut(&mut self, field: Field) -> &mut String {
        match field {
            Field::Name => &mut self.typed_name,
            Field::Password => &mut self.password,
            Field::Identity => &mut self.identity,
            Field::KeyPassword => &mut self.key_password,
            Field::Domain => &mut self.domain,
        }
    }

    /// The first field with nothing in it, where the keyboard starts.
    pub(super) fn first_empty(&self) -> Option<Field> {
        let fields = self.fields();
        fields
            .iter()
            .copied()
            .find(|f| self.text(*f).is_empty())
            .or_else(|| fields.first().copied())
    }

    /// Whether what is filled in can be tried: WPA's 8 characters at
    /// least, any WEP key or passphrase, 802.1X's identity and its
    /// password or certificate and key, the authority's file if one is
    /// asked; a hidden network's name.
    pub(super) fn filled(&self) -> bool {
        if self.vpn.is_some() {
            return !self.identity.trim().is_empty() && !self.password.is_empty();
        }
        if self.hidden && self.typed_name.trim().is_empty() {
            return false;
        }
        let n = self.password.chars().count();
        match self.security {
            Security::Open => true,
            Security::Psk { .. } => (secret::SHORTEST..=secret::LONGEST).contains(&n),
            Security::Wep => n > 0,
            Security::Enterprise => {
                let proof = match self.method {
                    Method::Tls => self.certificate.is_some() && self.key.is_some(),
                    _ => n > 0,
                };
                !self.identity.trim().is_empty()
                    && proof
                    && (self.authority != Authority::File || self.authority_file.is_some())
            }
        }
    }

    pub(super) fn waiting(&self) -> bool {
        self.sent.is_some()
    }

    /// What NetworkManager is asked.
    fn ask(&self) -> Ask {
        let secret = |text: &str| {
            let mut secret = Secret::longest(text.chars().count().max(1));
            secret.push(text);
            secret
        };
        let security = match self.security {
            Security::Open => Join::Open,
            Security::Psk { sae } => Join::Psk {
                sae,
                secret: secret(&self.password),
            },
            Security::Wep => Join::Wep {
                secret: secret(&self.password),
            },
            Security::Enterprise => {
                let tls = self.method == Method::Tls;
                Join::Eap(Box::new(Eap {
                    method: self.method.word(),
                    inner: (!tls).then(|| self.inner.word()),
                    identity: self.identity.trim().to_owned(),
                    password: (!tls).then(|| secret(&self.password)),
                    ca: match (self.authority, &self.authority_file) {
                        (Authority::File, Some(file)) => Ca::File(file.clone()),
                        (Authority::Unchecked, _) => Ca::Unchecked,
                        _ => Ca::System,
                    },
                    domain: Some(self.domain.trim().to_owned()).filter(|d| !d.is_empty()),
                    certificate: tls.then(|| self.certificate.clone()).flatten(),
                    key: tls.then(|| self.key.clone()).flatten(),
                    key_password: (tls && !self.key_password.is_empty())
                        .then(|| secret(&self.key_password)),
                }))
            }
        };
        Ask::Join {
            ssid: self.ssid.clone(),
            hidden: self.hidden,
            security,
        }
    }

    /// What was typed, overwritten.
    pub(super) fn wipe(&mut self) {
        for field in [Field::Password, Field::KeyPassword] {
            super::wipe(self.text_mut(field));
        }
    }

    /// What the log says of it: never what is typed.
    pub(super) fn shown(&self) -> serde_json::Value {
        let filled: Vec<&str> = self
            .fields()
            .into_iter()
            .filter(|f| !self.text(*f).is_empty())
            .map(Field::word)
            .collect();
        serde_json::json!({
            "name": if self.hidden { self.typed_name.clone() } else { self.name.clone() },
            "hidden": self.hidden,
            "security": format!("{:?}", self.security),
            "method": self.method.label(),
            "inner": self.inner.label(),
            "authority": format!("{:?}", self.authority),
            "certificate": self.certificate.is_some(),
            "key": self.key.is_some(),
            "fields": self.fields().into_iter().map(Field::word).collect::<Vec<_>>(),
            "typed": filled,
            "filled": self.filled(),
            "waiting": self.waiting(),
            "error": self.failed.map(|f| format!("{f:?}")),
            "menu": self.menu.map(MenuFor::word),
        })
    }
}

impl App {
    /// The sheet opened, the keyboard in its first empty field.
    pub(super) fn network_open_sheet(&mut self, sheet: Sheet) -> Task<Message> {
        let field = sheet.first_empty();
        self.network.sheet = Some(sheet);
        self.network_focus_field(field)
    }

    /// The keyboard to `field` of the sheet.
    fn network_focus_field(&mut self, field: Option<Field>) -> Task<Message> {
        let Some(field) = field else {
            self.focus = Focus::Nothing;
            return Task::none();
        };
        let at = self
            .network_controls()
            .iter()
            .position(|s| *s == Stop::SheetField(field))
            .unwrap_or(0);
        self.focus = Focus::Control(at);
        operation::focus(field.id())
    }

    pub(super) fn network_sheet_update(&mut self, msg: Msg) -> Task<Message> {
        let lang = self.lang;
        match msg {
            Msg::Cancel => {
                self.network_close_sheet();
                return Task::none();
            }
            Msg::Connect => return self.network_sheet_connect(),
            Msg::CloseMenu => {
                if let Some(sheet) = &mut self.network.sheet {
                    sheet.menu = None;
                }
                return Task::none();
            }
            Msg::File(what) => {
                self.network_choose_file(what);
                return Task::none();
            }
            Msg::Chosen(_, None, true) => {
                let text = w(
                    "Aucun sélecteur de fichiers ne répond : le portail du bureau n’est pas là.",
                    "No file chooser answers: the desktop portal is not there.",
                )
                .get(lang)
                .to_owned();
                return self.toast(text, super::super::ToastAction::None, super::super::TOAST);
            }
            _ => {}
        }
        let Some(sheet) = self.network.sheet.as_mut().filter(|s| !s.waiting()) else {
            return Task::none();
        };
        sheet.failed = None;
        if !matches!(msg, Msg::Menu(_) | Msg::Input(..) | Msg::Eye) {
            sheet.menu = None;
        }
        match msg {
            Msg::Menu(menu) => {
                sheet.menu = if sheet.menu == Some(menu) {
                    None
                } else {
                    Some(menu)
                };
            }
            Msg::Input(field, text) => *sheet.text_mut(field) = text,
            Msg::Security(security) => {
                sheet.security = security;
                let first = sheet.first_empty();
                return self.network_focus_field(first);
            }
            Msg::Method(method) => {
                sheet.method = method;
                if let Some(first) = method.inners().first() {
                    sheet.inner = *first;
                }
            }
            Msg::Inner(inner) => sheet.inner = inner,
            Msg::Authority(authority) => {
                sheet.authority = authority;
                if authority == Authority::File && sheet.authority_file.is_none() {
                    self.network_choose_file(FileFor::Authority);
                }
            }
            Msg::Chosen(what, Some(path), _) => match what {
                FileFor::Authority => sheet.authority_file = Some(path),
                FileFor::Certificate => sheet.certificate = Some(path),
                FileFor::Key => sheet.key = Some(path),
            },
            Msg::Eye => sheet.shown = !sheet.shown,
            Msg::Chosen(..) | Msg::Cancel | Msg::Connect | Msg::File(_) | Msg::CloseMenu => {}
        }
        Task::none()
    }

    /// The desktop portal's file chooser, on a thread of its own.
    fn network_choose_file(&self, what: FileFor) {
        let Some(to) = self.network.to.clone() else {
            return;
        };
        let lang = self.lang;
        let (title, filter, globs): (Words, Words, &[&str]) = match what {
            FileFor::Authority => (
                w("Certificat de l’autorité", "CA certificate"),
                w("Certificats", "Certificates"),
                &["*.pem", "*.crt", "*.cer", "*.der"],
            ),
            FileFor::Certificate => (
                w("Certificat personnel", "User certificate"),
                w("Certificats", "Certificates"),
                &["*.pem", "*.crt", "*.cer", "*.der", "*.p12", "*.pfx"],
            ),
            FileFor::Key => (
                w("Clé privée", "Private key"),
                w("Clés", "Keys"),
                &["*.pem", "*.key", "*.der", "*.p12", "*.pfx"],
            ),
        };
        let folder = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        let started = std::thread::Builder::new()
            .name("network: file chooser".to_owned())
            .spawn(move || {
                let types: Vec<(u32, &str)> = globs.iter().map(|g| (0, *g)).collect();
                let chosen =
                    crate::portal::open_file_of(title.get(lang), filter.get(lang), &types, &folder);
                let (path, missing) = match chosen {
                    crate::portal::Chosen::Path(path) => (Some(path), false),
                    crate::portal::Chosen::Cancelled => (None, false),
                    crate::portal::Chosen::NoPortal => (None, true),
                };
                let _ = to.unbounded_send(Message::Network(PageMsg::Sheet(Msg::Chosen(
                    what, path, missing,
                ))));
            });
        if let Err(err) = started {
            tracing::warn!(%err, "no file chooser");
        }
    }

    fn network_sheet_connect(&mut self) -> Task<Message> {
        let before = self
            .network
            .details
            .as_ref()
            .and_then(|d| d.network.attempt.as_ref())
            .map_or(0, |a| a.id);
        let Some(sheet) = self.network.sheet.as_mut() else {
            return Task::none();
        };
        if sheet.waiting() || !sheet.filled() {
            return Task::none();
        }
        // An OpenVPN file's: imported with them, the sheet closed.
        if let Some((file, kind, _)) = sheet.vpn.clone() {
            let credentials = super::vpn::credentials(&sheet.identity, &sheet.password);
            self.network.ask(Ask::Import {
                file,
                kind,
                credentials: Some(credentials),
            });
            self.network_close_sheet();
            return Task::none();
        }
        if sheet.hidden {
            sheet.ssid = sheet.typed_name.trim().as_bytes().to_vec();
        }
        sheet.sent = Some(before);
        sheet.failed = None;
        let ask = sheet.ask();
        tracing::info!(hidden = sheet.hidden, security = ?sheet.security, "the sheet joins");
        self.network.ask(ask);
        Task::none()
    }

    /// The sheet closed: what was typed forgotten, an attempt under way
    /// given up.
    pub(super) fn network_close_sheet(&mut self) {
        if let Some(mut sheet) = self.network.sheet.take() {
            if sheet.waiting() {
                self.network.ask(Ask::Abandon);
            }
            sheet.wipe();
        }
        self.focus = Focus::Nothing;
    }

    /// The sheet over the page, under its scrim.
    pub(in crate::app) fn network_sheet(&self) -> Option<El<'_>> {
        let sheet = self.network.sheet.as_ref()?;
        let lang = self.lang;
        let title = match (sheet.hidden, lang) {
            _ if sheet.vpn.is_some() => match lang {
                Lang::French => format!("VPN « {} »", sheet.name),
                Lang::English => format!("VPN \u{201c}{}\u{201d}", sheet.name),
            },
            (true, _) => w("Rejoindre un réseau masqué", "Join a hidden network")
                .get(lang)
                .to_owned(),
            (false, Lang::French) => format!("Se connecter à « {} »", sheet.name),
            (false, Lang::English) => format!("Connect to \u{201c}{}\u{201d}", sheet.name),
        };
        let waiting = sheet.waiting();
        let mut body = column![
            row![
                wifi_glyph(4, 18.0, TEXT.get()),
                text(title).size(17).font(BOLD).color(TEXT)
            ]
            .spacing(10)
            .align_y(Alignment::Center)
        ]
        .spacing(14);
        if sheet.vpn.is_some() {
            body = body.push(
                t(
                    w(
                        "Ce fichier demande un identifiant et un mot de passe : ceux que te donne ton fournisseur. Ils sont gardés avec le VPN.",
                        "This file asks for a user name and a password: those your provider gives you. They are kept with the VPN.",
                    )
                    .get(lang),
                    13.0,
                    DIM.get(),
                )
                .line_height(1.5),
            );
            body =
                body.push(self.sheet_field(sheet, Field::Identity, w("Identifiant", "User name")));
            body =
                body.push(self.sheet_field(sheet, Field::Password, w("Mot de passe", "Password")));
        } else if sheet.security == Security::Enterprise {
            body = body.push(
                t(
                    w(
                        "Un réseau d’entreprise ou d’école : ce qu’il faut taper est donné par son service informatique.",
                        "A company or school network: what to type comes from its IT department.",
                    )
                    .get(lang),
                    13.0,
                    DIM.get(),
                )
                .line_height(1.5),
            );
        }
        if sheet.hidden {
            body =
                body.push(self.sheet_field(sheet, Field::Name, w("Nom du réseau", "Network name")));
            body = body.push(labelled(
                w("Sécurité", "Security").get(lang),
                menu_button(
                    security_label(sheet.security).get(lang).to_owned(),
                    MenuFor::Security,
                    sheet.menu == Some(MenuFor::Security),
                    waiting,
                ),
                "sheet:security",
            ));
        }
        match sheet.security {
            Security::Psk { .. } | Security::Wep => {
                body = body.push(self.sheet_field(
                    sheet,
                    Field::Password,
                    w("Mot de passe", "Password"),
                ));
            }
            Security::Enterprise if sheet.vpn.is_none() => {
                body = self.sheet_enterprise(sheet, body);
            }
            Security::Enterprise | Security::Open => {}
        }
        if let Some(failure) = sheet.failed {
            let said = match failure {
                Failure::Secret => w(
                    "Mot de passe incorrect. Vérifie-le et réessaie.",
                    "Wrong password. Check it and try again.",
                ),
                Failure::Other => w(
                    "La connexion a échoué. Réessaie.",
                    "Could not connect. Try again.",
                ),
            };
            body = body
                .push(container(t(said.get(lang), 12.0, PEACH.get())).id("sheet:error".to_owned()));
        }
        let filled = sheet.filled();
        let connect_focused = self.network_focused(Stop::SheetConnect);
        let connect: El<'_> = if waiting {
            container(
                row![
                    spinner(14.0, DEEP.get(), !self.reduced_motion),
                    t(w("Connexion…", "Connecting…").get(lang), 13.0, DEEP.get()).font(BOLD)
                ]
                .spacing(7)
                .align_y(Alignment::Center),
            )
            .height(32)
            .padding([0, 14])
            .center_y(32)
            .style(boxed(fade(GREEN.get(), INERT), 8.0, None))
            .into()
        } else {
            let mut b = button(
                container(
                    t(
                        w("Se connecter", "Connect").get(lang),
                        13.0,
                        if filled {
                            DEEP.get()
                        } else {
                            fade(TEXT.get(), INERT)
                        },
                    )
                    .font(BOLD),
                )
                .height(Length::Fill)
                .center_y(Length::Fill),
            )
            .height(32)
            .padding([0, 14])
            .style(move |_, _| button::Style {
                background: Some(Background::Color(if filled {
                    GREEN.get()
                } else {
                    crate::theme::ink(rgba(255, 255, 255, 0.04))
                })),
                text_color: TEXT.get(),
                border: if connect_focused {
                    ring(8.0)
                } else {
                    border(8.0, 1.0, if filled { GREEN.get() } else { RIM_12.get() })
                },
                ..button::Style::default()
            });
            if filled {
                b = b.on_press(Message::Network(PageMsg::Sheet(Msg::Connect)));
            }
            b.into()
        };
        let actions = row![
            Space::new().width(Length::Fill),
            container(secondary(
                w("Annuler", "Cancel").get(lang).to_owned(),
                Some(Message::Network(PageMsg::Sheet(Msg::Cancel))),
                self.network_focused(Stop::SheetCancel),
                false,
            ))
            .id("sheet:cancel".to_owned()),
            container(connect).id("sheet:connect".to_owned()),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        body = body.push(container(actions).padding(Padding::default().top(4)));
        let panel = container(body)
            .width(460)
            .padding(Padding {
                top: 20.0,
                right: 22.0,
                bottom: 18.0,
                left: 22.0,
            })
            .style(|_: &Theme| container::Style {
                // The mockup's #1d2333 → #171c29, top to bottom.
                background: Some(Background::Gradient(Gradient::Linear(
                    gradient::Linear::new(std::f32::consts::PI)
                        .add_stop(0.0, crate::theme::ink(rgb(0x1d2333)))
                        .add_stop(1.0, crate::theme::ink(rgb(0x171c29))),
                ))),
                border: border(14.0, 1.0, RIM_12.get()),
                shadow: Shadow {
                    color: rgba(0, 0, 0, 0.55),
                    offset: Vector::new(0.0, 24.0),
                    blur_radius: 60.0,
                },
                ..container::Style::default()
            })
            .id("sheet".to_owned());
        Some(
            mouse_area(
                container(panel)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center(Length::Fill)
                    .style(|_: &Theme| container::Style {
                        background: Some(Background::Color(rgba(7, 9, 15, 0.62))),
                        ..container::Style::default()
                    }),
            )
            .on_press(Message::Nothing)
            .into(),
        )
    }

    /// 802.1X's fields.
    fn sheet_enterprise<'a>(
        &'a self,
        sheet: &'a Sheet,
        mut body: iced::widget::Column<'a, Message>,
    ) -> iced::widget::Column<'a, Message> {
        let lang = self.lang;
        let waiting = sheet.waiting();
        let mut two = row![labelled(
            w("Méthode", "Method").get(lang),
            menu_button(
                sheet.method.label().to_owned(),
                MenuFor::Method,
                sheet.menu == Some(MenuFor::Method),
                waiting
            ),
            "sheet:method"
        )]
        .spacing(10);
        if sheet.method == Method::Tls {
            two = two.push(Space::new().width(Length::Fill));
        } else {
            two = two.push(labelled(
                w("Authentification interne", "Inner authentication").get(lang),
                menu_button(
                    sheet.inner.label().to_owned(),
                    MenuFor::Inner,
                    sheet.menu == Some(MenuFor::Inner),
                    waiting,
                ),
                "sheet:inner",
            ));
        }
        body = body.push(two);
        body = body.push(self.sheet_field(sheet, Field::Identity, w("Identifiant", "Identity")));
        if sheet.method == Method::Tls {
            body = body.push(
                row![
                    self.sheet_file(
                        sheet.certificate.as_ref(),
                        FileFor::Certificate,
                        w("Certificat personnel", "User certificate"),
                        waiting
                    ),
                    self.sheet_file(
                        sheet.key.as_ref(),
                        FileFor::Key,
                        w("Clé privée", "Private key"),
                        waiting
                    ),
                ]
                .spacing(10),
            );
            body = body.push(self.sheet_field(
                sheet,
                Field::KeyPassword,
                w("Mot de passe de la clé", "Private key password"),
            ));
        } else {
            body =
                body.push(self.sheet_field(sheet, Field::Password, w("Mot de passe", "Password")));
        }
        let authority = match (sheet.authority, &sheet.authority_file) {
            (Authority::File, Some(file)) => file_name(file),
            (authority, _) => authority.label().get(lang).to_owned(),
        };
        body = body.push(
            row![
                labelled(
                    w("Certificat de l’autorité", "CA certificate").get(lang),
                    menu_button(
                        authority,
                        MenuFor::Authority,
                        sheet.menu == Some(MenuFor::Authority),
                        waiting
                    ),
                    "sheet:authority",
                ),
                container(self.sheet_field(
                    sheet,
                    Field::Domain,
                    w("Domaine du serveur", "Server domain")
                ))
                .width(Length::Fill),
            ]
            .spacing(10),
        );
        let hint: (Words, Color) = if sheet.authority == Authority::Unchecked {
            (
                w(
                    "Sans vérification, un faux point d’accès peut lire ton mot de passe.",
                    "Without a check, a fake access point can read your password.",
                ),
                AMBER.get(),
            )
        } else {
            (
                w(
                    "Le domaine vérifie qu’on parle au vrai serveur. Sans lui, un faux point d’accès pourrait lire le mot de passe.",
                    "The domain checks that it is the real server. Without it, a fake access point could read the password.",
                ),
                FAINT.get(),
            )
        };
        body.push(
            container(t(hint.0.get(lang), 12.0, hint.1).line_height(1.4))
                .id("sheet:hint".to_owned()),
        )
    }

    /// A text field of the sheet, its label over it.
    fn sheet_field<'a>(&'a self, sheet: &'a Sheet, field: Field, label: Words) -> El<'a> {
        let lang = self.lang;
        let waiting = sheet.waiting();
        let focused = self.network_focused(Stop::SheetField(field));
        let secret = field.secret();
        let mut input = text_input("", sheet.text(field))
            .id(field.id())
            .secure(secret && !sheet.shown)
            .size(13.5)
            .font(MANROPE)
            .padding(0)
            .style(style::search);
        if !waiting {
            input = input
                .on_input(move |s| Message::Network(PageMsg::Sheet(Msg::Input(field, s))))
                .on_submit(Message::Network(PageMsg::Sheet(Msg::Connect)));
        }
        let wrong = secret && sheet.failed == Some(Failure::Secret);
        let rim = if wrong {
            ORANGE.get()
        } else if focused {
            crate::theme::ink(rgba(108, 194, 135, 0.65))
        } else {
            RIM_12.get()
        };
        let mut inside = row![container(input).width(Length::Fill)]
            .spacing(6)
            .align_y(Alignment::Center);
        if secret {
            inside = inside.push(
                button(draw(
                    if sheet.shown { EYE_SHUT } else { EYE },
                    15.0,
                    DIM.get(),
                ))
                .width(26)
                .height(26)
                .padding(5.5)
                .on_press(Message::Network(PageMsg::Sheet(Msg::Eye)))
                .style(flat(None, HOVER.get(), 6.0, false)),
            );
        }
        let boxed_field = container(inside)
            .height(34)
            .center_y(34)
            .padding(Padding {
                top: 0.0,
                right: if secret { 4.0 } else { 10.0 },
                bottom: 0.0,
                left: 10.0,
            })
            .style(boxed(
                crate::theme::ink(rgba(255, 255, 255, 0.06)),
                9.0,
                Some((1.0, rim)),
            ))
            .id(format!("sheet:{}", field.word()));
        column![t(label.get(lang), 12.0, DIM.get()).font(BOLD), boxed_field]
            .spacing(5)
            .width(Length::Fill)
            .into()
    }

    /// A file of the sheet: its name, or "Choisir un fichier…".
    fn sheet_file<'a>(
        &'a self,
        chosen: Option<&PathBuf>,
        what: FileFor,
        label: Words,
        waiting: bool,
    ) -> El<'a> {
        let lang = self.lang;
        let said = chosen.map_or_else(
            || {
                w("Choisir un fichier…", "Choose a file…")
                    .get(lang)
                    .to_owned()
            },
            |p| file_name(p),
        );
        let id = match what {
            FileFor::Authority => "sheet:authority-file",
            FileFor::Certificate => "sheet:certificate",
            FileFor::Key => "sheet:key",
        };
        column![
            t(label.get(lang), 12.0, DIM.get()).font(BOLD),
            container(secondary(
                said,
                (!waiting).then_some(Message::Network(PageMsg::Sheet(Msg::File(what)))),
                false,
                false,
            ))
            .id(id.to_owned()),
        ]
        .spacing(5)
        .width(Length::Fill)
        .into()
    }
}

/// A file's name as the sheet says it.
fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A labelled control of the sheet, half its width beside another.
fn labelled<'a>(label: &'static str, control: El<'a>, id: &str) -> El<'a> {
    column![
        t(label, 12.0, DIM.get()).font(BOLD),
        container(control).id(id.to_owned())
    ]
    .spacing(5)
    .width(Length::Fill)
    .into()
}

/// What the hidden network's security menu says of `security`.
fn security_label(security: Security) -> Words {
    match security {
        Security::Open => w("Aucune", "None"),
        Security::Psk { sae: false } => w("WPA/WPA2 personnel", "WPA/WPA2 personal"),
        Security::Psk { sae: true } => w("WPA3 personnel", "WPA3 personal"),
        Security::Enterprise => w("WPA/WPA2 entreprise", "WPA/WPA2 enterprise"),
        Security::Wep => w("WEP", "WEP"),
    }
}

/// A menu's button of the sheet: 34 px, as its fields, its value and a
/// chevron.
fn menu_button<'a>(said: String, which: MenuFor, open: bool, inert: bool) -> El<'a> {
    let alpha = if inert { INERT } else { 1.0 };
    let mut b = button(
        row![
            text(said)
                .size(13.5)
                .font(SEMIBOLD)
                .color(fade(TEXT.get(), alpha))
                .wrapping(text::Wrapping::None),
            Space::new().width(Length::Fill),
            draw("M6 9 L12 15 L18 9", 13.0, fade(DIM.get(), alpha)),
        ]
        .spacing(8)
        .height(Length::Fill)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .height(34)
    .padding([0, 10])
    .style(move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
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
            border: border(9.0, 1.0, fade(RIM_12.get(), alpha)),
            ..button::Style::default()
        }
    });
    if !inert {
        b = b.on_press(Message::Network(PageMsg::Sheet(Msg::Menu(which))));
    }
    b.into()
}

impl App {
    /// The sheet's open menu's choices: their words, chosen or not, and
    /// what they do.
    fn sheet_menu_options(&self, sheet: &Sheet, which: MenuFor) -> Vec<(String, bool, Msg)> {
        let lang = self.lang;
        match which {
            MenuFor::Security => [
                Security::Open,
                Security::Psk { sae: false },
                Security::Psk { sae: true },
                Security::Enterprise,
                Security::Wep,
            ]
            .into_iter()
            .map(|s| {
                (
                    security_label(s).get(lang).to_owned(),
                    s == sheet.security,
                    Msg::Security(s),
                )
            })
            .collect(),
            MenuFor::Method => Method::ALL
                .into_iter()
                .map(|m| (m.label().to_owned(), m == sheet.method, Msg::Method(m)))
                .collect(),
            MenuFor::Inner => sheet
                .method
                .inners()
                .iter()
                .map(|i| (i.label().to_owned(), *i == sheet.inner, Msg::Inner(*i)))
                .collect(),
            MenuFor::Authority => Authority::ALL
                .into_iter()
                .map(|a| {
                    (
                        a.label().get(lang).to_owned(),
                        a == sheet.authority,
                        Msg::Authority(a),
                    )
                })
                .collect(),
        }
    }

    /// The sheet's open menu, under its button (over it when the window
    /// ends first); its choices `option:N`.
    pub(in crate::app) fn network_sheet_menu(&self) -> Option<El<'_>> {
        let sheet = self.network.sheet.as_ref()?;
        let which = sheet.menu?;
        let at = *self.places.at.get(&format!("sheet:{}", which.word()))?;
        let options = self.sheet_menu_options(sheet, which);
        let height = options.len() as f32 * 32.0 + 10.0;
        let mut list = column![];
        for (i, (label, on, msg)) in options.into_iter().enumerate() {
            let check: El<'_> = if on {
                draw("M5 12.5 L10 17 L19 7.5", 13.0, GREEN.get())
            } else {
                Space::new().width(13).into()
            };
            list = list.push(
                container(
                    button(
                        row![
                            container(check).width(18).center_x(18),
                            text(label)
                                .size(13.5)
                                .font(if on { SEMIBOLD } else { MANROPE })
                                .color(TEXT)
                                .wrapping(text::Wrapping::None),
                        ]
                        .spacing(6)
                        .height(Length::Fill)
                        .align_y(Alignment::Center),
                    )
                    .width(Length::Fill)
                    .height(32)
                    .padding([0, 6])
                    .on_press(Message::Network(PageMsg::Sheet(msg)))
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
        let panel = container(list)
            .width(at.width.max(180.0))
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
            .id("sheet:menu".to_owned());
        let window = self
            .places
            .at
            .get("sidebar:fr")
            .or_else(|| self.places.at.get("sidebar:en"))
            .map_or(f32::MAX, |s| s.height);
        let y = if at.y + at.height + 4.0 + height > window - 8.0 {
            at.y - height - 4.0
        } else {
            at.y + at.height + 4.0
        };
        Some(pin(panel).x(at.x).y(y.max(0.0)).into())
    }
}
